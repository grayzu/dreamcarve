// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts
//   src/engine/toolpaths/xyLead.ts

//! The waste domain of a Profile (`docs/specs/phase-2/design.md`,
//! "Algorithms → Entry", "Waste domain for Profile"): where the tool centre
//! may be while it ramps or helixes down to a wall, because the material
//! there is cut away anyway. Upstream builds it in `edge.ts` (the band's
//! inset regions for an inside route, `outsideDomain` through
//! `domainOutsideLoops` for an outside one) and keeps tabs out of it with
//! `tabKeepOutWithin` and `withEntryKeepOut`.
//!
//! - **Inside:** the region's tool-centre area, its outer offset in by the
//!   radius and its islands out.
//! - **Outside:** a frame round the path (upstream's
//!   `OUTSIDE_DOMAIN_REACH_DIAMETERS` beyond its bounds, so a route's
//!   entry does not depend on parts far from it, #909), cut to the stock
//!   less the radius (the cutter stays on the stock, #62), less every target
//!   region grown by the radius.
//! - Both less every other region's kerf, its tool-centre paths grown by
//!   the radius, so no entry enters a neighbouring part's cut.
//!
//! Each tool-centre offset is a radius less [`WALL_SLACK_MM`], so the
//! path, and the entry point on it, lies just inside the domain whatever
//! the booleans round.
//!
//! Tabs are DreamCarve's distances along the path, not upstream's XY
//! rectangles: a tab's material is the kerf over the stretch
//! [`crate::tabs::PathTabs::stretches_mm`] gives, and the cutter keeps
//! off it when its centre is two radii from that stretch.

use crate::footprint::footprint_at;
use crate::pocket::rings::inset_region;
use crate::toolpath::{Move, Toolpath};
use dc_geometry::{
    Contour, EPS_MM, Point, Point3, Polyline, Rect, Region, difference, nest, offset,
};
use dc_project::{Side, Stock};

/// How much less than the radius the domain's offsets are, mm: the
/// tool-centre path sits this far inside the domain instead of on its
/// edge, where the booleans' rounding (well under 1e-6 mm at any stock
/// size) could put it outside. The cutter may come this much closer to a
/// wall than its path does.
pub(super) const WALL_SLACK_MM: f64 = 1e-4;

/// How far the frame of an outside path's domain reaches beyond its
/// bounds, in tool diameters (upstream `OUTSIDE_DOMAIN_REACH_DIAMETERS`).
pub(super) const OUTSIDE_REACH_DIAMETERS: f64 = 3.5;

/// The target regions grown for one allowance (#284): the allowance, each
/// region grown by the reach plus it, and the bounds of each.
type Grown = (f64, Vec<Vec<Region>>, Vec<Option<Rect>>);

/// One tool-centre path of a Profile, with what its domain needs.
pub(super) struct WallPath<'a> {
    /// The tool-centre path, closed.
    pub path: &'a Polyline,
    /// Index into the regions of the region whose wall it cuts.
    pub region: usize,
    /// How much farther than the radius the path runs from that wall,
    /// mm: a last pass allowance (#284), 0 for the part's own path. Its
    /// domain keeps the tool centre that much farther from the region.
    pub allowance_mm: f64,
}

/// The waste domain of each of `paths`, in order: tool-centre regions
/// (mm, Y-up) for a Profile on `side` of `regions` with a tool of radius
/// `radius_mm`, booleans at `tolerance_mm`. On line has none.
pub(super) fn domains(
    side: Side,
    regions: &[Region],
    paths: &[WallPath<'_>],
    stock: &Stock,
    radius_mm: f64,
    tolerance_mm: f64,
) -> Vec<Vec<Region>> {
    if !side.has_waste_area() {
        return paths.iter().map(|_| Vec::new()).collect();
    }
    let reach = radius_mm - WALL_SLACK_MM;
    // Each region's kerf, and its bounds for cheap culling.
    let mut kerfs: Vec<Vec<Region>> = vec![Vec::new(); regions.len()];
    for p in paths {
        if let Some(k) = kerfs.get_mut(p.region) {
            k.extend(band(p.path, radius_mm, tolerance_mm));
        }
    }
    let kerf_bounds: Vec<Option<Rect>> = kerfs.iter().map(|k| bounds_of(k)).collect();
    // Each region grown by the reach plus each allowance a path has
    // (#284: 0, and the last pass allowance), with its bounds.
    let mut allowances: Vec<f64> = Vec::new();
    for p in paths {
        if !allowances
            .iter()
            .any(|a| a.to_bits() == p.allowance_mm.to_bits())
        {
            allowances.push(p.allowance_mm);
        }
    }
    let grown: Vec<Grown> = match side {
        Side::Outside => allowances
            .iter()
            .map(|&allowance_mm| {
                let grown: Vec<Vec<Region>> = regions
                    .iter()
                    .map(|r| inset_region(r, -(reach + allowance_mm), tolerance_mm))
                    .collect();
                let bounds = grown.iter().map(|g| bounds_of(g)).collect();
                (allowance_mm, grown, bounds)
            })
            .collect(),
        Side::Inside | Side::OnLine | Side::Left | Side::Right => Vec::new(),
    };
    // Inside domains depend only on the region and the allowance.
    let mut inside_cache: Vec<Option<Vec<Region>>> = vec![None; regions.len() * allowances.len()];

    paths
        .iter()
        .map(|p| {
            let others = |frame: Option<Rect>| -> Vec<Region> {
                kerfs
                    .iter()
                    .zip(&kerf_bounds)
                    .enumerate()
                    .filter(|(j, (_, b))| {
                        *j != p.region && b.zip(frame).is_some_and(|(b, f)| overlaps(b, f))
                    })
                    .flat_map(|(_, (k, _))| k.iter().cloned())
                    .collect()
            };
            match side {
                Side::Inside => {
                    let slot = allowances
                        .iter()
                        .position(|a| a.to_bits() == p.allowance_mm.to_bits())
                        .map_or(usize::MAX, |k| k * regions.len() + p.region);
                    if let Some(Some(cached)) = inside_cache.get(slot) {
                        return cached.clone();
                    }
                    let Some(region) = regions.get(p.region) else {
                        return Vec::new();
                    };
                    let area = inset_region(region, reach + p.allowance_mm, tolerance_mm);
                    let clip = others(bounds_of(&area));
                    let domain = if clip.is_empty() {
                        area
                    } else {
                        difference(&area, &clip, tolerance_mm)
                    };
                    if let Some(cached) = inside_cache.get_mut(slot) {
                        *cached = Some(domain.clone());
                    }
                    domain
                }
                Side::Outside => {
                    let Some(frame) = outside_frame(p.path, stock, radius_mm) else {
                        return Vec::new();
                    };
                    let Some(frame_region) = rect_region(frame) else {
                        return Vec::new();
                    };
                    let mut clip: Vec<Region> = grown
                        .iter()
                        .filter(|(a, _, _)| a.to_bits() == p.allowance_mm.to_bits())
                        .flat_map(|(_, grown, bounds)| grown.iter().zip(bounds))
                        .filter(|(_, b)| b.is_some_and(|b| overlaps(b, frame)))
                        .flat_map(|(g, _)| g.iter().cloned())
                        .collect();
                    clip.extend(others(Some(frame)));
                    difference(&[frame_region], &clip, tolerance_mm)
                }
                Side::OnLine | Side::Left | Side::Right => Vec::new(),
            }
        })
        .collect()
}

/// The frame of an outside path's domain: its bounds grown by
/// [`OUTSIDE_REACH_DIAMETERS`] diameters, cut to the stock less
/// `radius_mm` (no slack: the stock edge is not a wall the path rides,
/// and the cutter must not pass it). `None` when nothing is left.
fn outside_frame(path: &Polyline, stock: &Stock, radius_mm: f64) -> Option<Rect> {
    let b = path.bounds()?;
    let reach = 2.0 * radius_mm * OUTSIDE_REACH_DIAMETERS;
    let inset = radius_mm;
    let min = Point::new((b.min.x - reach).max(inset), (b.min.y - reach).max(inset));
    let max = Point::new(
        (b.max.x + reach).min(stock.width_mm - inset),
        (b.max.y + reach).min(stock.height_mm - inset),
    );
    (max.x - min.x > EPS_MM && max.y - min.y > EPS_MM).then_some(Rect { min, max })
}

/// The kerf a tool of radius `radius_mm` cuts along the closed `path`: the
/// band between its offsets either way.
fn band(path: &Polyline, radius_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let Ok(contour) = Contour::new(path.clone()) else {
        return Vec::new();
    };
    let wide: Vec<Region> = offset(&contour, radius_mm)
        .into_iter()
        .filter_map(filled)
        .collect();
    let narrow: Vec<Region> = offset(&contour, -radius_mm)
        .into_iter()
        .filter_map(filled)
        .collect();
    if narrow.is_empty() {
        return wide;
    }
    difference(&wide, &narrow, tolerance_mm)
}

/// The area `contour` encloses, whichever way it winds.
fn filled(contour: Contour) -> Option<Region> {
    nest(vec![contour])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

fn rect_region(r: Rect) -> Option<Region> {
    Contour::new(Polyline::rect(r.min, r.max.x - r.min.x, r.max.y - r.min.y))
        .ok()
        .and_then(filled)
}

/// The bounds of every outer of `regions`.
fn bounds_of(regions: &[Region]) -> Option<Rect> {
    regions
        .iter()
        .filter_map(|r| r.outer().polyline().bounds())
        .reduce(|a, b| a.including(b.min).including(b.max))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x + EPS_MM
        && b.min.x <= a.max.x + EPS_MM
        && a.min.y <= b.max.y + EPS_MM
        && b.min.y <= a.max.y + EPS_MM
}

/// Where the cutter's centre must not be while the tabs stand: every
/// stretch of `stretches_mm` along the closed `path` grown by twice
/// `radius_mm` (the tab is the kerf over the stretch, a radius either side
/// of it; the cutter reaches a radius more).
pub(super) fn tab_keep_out(
    path: &Polyline,
    stretches_mm: &[(f64, f64)],
    radius_mm: f64,
    tolerance_mm: f64,
) -> Vec<Region> {
    let length_mm = path.length_mm();
    let flat = path.with_arcs_flattened(tolerance_mm);
    let flat_length_mm = flat.length_mm();
    if !(length_mm > EPS_MM && flat_length_mm > EPS_MM) {
        return Vec::new();
    }
    // Distances along the path, measured on the flattened path.
    let scale = flat_length_mm / length_mm;
    let mut points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
    if let Some(&first) = points.first() {
        points.push(first);
    }
    let mut moves = Vec::new();
    for &(from, to) in stretches_mm {
        let stretch = sub_path(&points, from * scale, to * scale, flat_length_mm);
        let Some((&first, rest)) = stretch.split_first() else {
            continue;
        };
        moves.push(Move::rapid(Point3::new(first.x, first.y, 0.0)));
        if rest.is_empty() {
            // A stretch with no length still holds its tab.
            moves.push(Move::linear(
                Point3::new(first.x + EPS_MM, first.y, 0.0),
                1.0,
            ));
        }
        moves.extend(
            rest.iter()
                .map(|p| Move::linear(Point3::new(p.x, p.y, 0.0), 1.0)),
        );
    }
    if moves.is_empty() {
        return Vec::new();
    }
    // A margin of the tolerance covers the flattening of the path and of
    // the keep-out itself.
    footprint_at(
        &Toolpath { moves },
        2.0 * radius_mm + tolerance_mm,
        tolerance_mm,
    )
}

/// The points of the closed polygon `ring` (first point repeated last)
/// from `from_mm` to `to_mm` along it, `to_mm` possibly past `length_mm`
/// (through the start).
fn sub_path(ring: &[Point], from_mm: f64, to_mm: f64, length_mm: f64) -> Vec<Point> {
    let mut out = Vec::new();
    let mut walked = 0.0;
    // Twice round covers a stretch through the start.
    let laps = ring.windows(2).chain(ring.windows(2));
    for w in laps {
        let [a, b] = w else { continue };
        let len = a.distance_to(*b);
        let (s0, s1) = (walked, walked + len);
        walked = s1;
        if s1 < from_mm || len <= 0.0 {
            continue;
        }
        if s0 > to_mm {
            break;
        }
        let at = |s: f64| *a + (*b - *a) * ((s - s0) / len);
        if out.is_empty() {
            out.push(at(from_mm.max(s0)));
        }
        out.push(at(to_mm.min(s1)));
        if s1 >= to_mm || walked >= 2.0 * length_mm {
            break;
        }
    }
    out
}
