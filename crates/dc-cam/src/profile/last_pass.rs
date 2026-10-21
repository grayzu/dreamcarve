// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A Profile's last pass allowance (#284; `docs/specs/phase-2/design.md`,
//! "Phase 2 addendum (#73)", A8; VCarve's 2D Profile "Last Pass
//! Allowance", Fusion's 2D Contour stock to leave and finishing pass).
//! PureCutCNC has no counterpart: it leaves stock with a separate rough
//! operation, so this is DreamCarve's own.
//!
//! Every pass but the last runs on the allowance path, `r + a` from the
//! part on the waste side, and the last on the part's path at `r`. Each
//! allowance path is the same offset as the part's path with the larger
//! distance; this module makes it, refuses a drawing it cannot follow,
//! and carries the part path's tabs onto it.

use super::{Offset, check_band, region_shapes};
use crate::error::{CamError, ShapeRef};
use crate::overlap::within;
use crate::passes::pass_z_mm;
use crate::tabs::{PathTabs, RaisedSpan};
use dc_geometry::{
    Contour, EPS_MM, Point, Polyline, Region, Vertex, distance_mm, nearest_point, offset,
};
use dc_project::{LastPass, ProfileOp, Side, TabShape, Tool};
use std::collections::BTreeMap;

/// Refuses a last pass the generator cannot cut (validation reports each
/// first): one on a side with no wall to leave material on
/// ([`CamError::LastPassNeedsSide`]), an allowance that is not more than
/// 0 and at most a third of the tool's `diameter_mm`, or a thickness that
/// is not more than 0 and at most `depth_mm` and the `step_down_mm` in
/// effect (#310 review: the last pass is never thicker than a step)
/// ([`CamError::InvalidLastPass`]).
pub(super) fn check(
    op: &ProfileOp,
    last_pass: &LastPass,
    diameter_mm: f64,
    depth_mm: f64,
    step_down_mm: f64,
) -> Result<(), CamError> {
    if !op.side.has_waste_area() {
        return Err(CamError::LastPassNeedsSide { op: op.id.clone() });
    }
    let limit_mm = diameter_mm / 3.0;
    let allowance_mm = last_pass.allowance_mm;
    if !(allowance_mm.is_finite() && allowance_mm > 0.0 && allowance_mm <= limit_mm) {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "allowance_mm",
            value: allowance_mm,
            range: format!(
                "more than 0 and at most {limit_mm:.3} mm, a third of the {diameter_mm} mm tool"
            ),
        });
    }
    if let Some(thickness_mm) = last_pass.thickness_mm
        && !(thickness_mm.is_finite() && thickness_mm > 0.0 && thickness_mm <= depth_mm + EPS_MM)
    {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "thickness_mm",
            value: thickness_mm,
            range: format!("more than 0 and at most the {depth_mm} mm depth"),
        });
    }
    if let Some(thickness_mm) = last_pass.thickness_mm
        && thickness_mm > step_down_mm + EPS_MM
    {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "thickness_mm",
            value: thickness_mm,
            range: format!("at most the {step_down_mm} mm step-down"),
        });
    }
    Ok(())
}

/// The pass Zs of a cut `depth_mm` deep into stock `thickness_mm` thick
/// at `step_down_mm` ([`pass_z_mm`]), with `last_pass`'s thickness `t`:
/// the usual passes to `depth − t`, then one at the full depth. Without a
/// thickness, or with none set, the usual passes.
pub(super) fn pass_z(
    op: &ProfileOp,
    thickness_mm: f64,
    depth_mm: f64,
    step_down_mm: f64,
    last_pass: Option<&LastPass>,
) -> Result<Vec<f64>, CamError> {
    let full = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        step_down_mm,
        op.through_cut_allowance_mm,
    )?;
    let Some(last_mm) = last_pass.and_then(|l| l.thickness_mm) else {
        return Ok(full);
    };
    let rough_mm = depth_mm - last_mm;
    let mut z_mm = if rough_mm > EPS_MM {
        pass_z_mm(
            &op.id,
            thickness_mm,
            rough_mm,
            step_down_mm,
            op.through_cut_allowance_mm,
        )?
    } else {
        Vec::new()
    };
    z_mm.push(thickness_mm - depth_mm);
    Ok(z_mm)
}

/// How much more than `√2 · allowance` the last pass may take off, mm
/// (#310 review): the rounding of offsets and sampling, so that a square
/// corner, where it takes exactly that, is cut.
pub(super) const THICKNESS_SLACK_MM: f64 = 0.05;

/// What [`twins`] needs to know about one outline and its paths.
pub(super) struct Outline<'a> {
    /// The region the outline bounds a side of.
    pub region: &'a Region,
    /// The outline.
    pub contour: &'a Contour,
    /// The shape the outline is.
    pub shape: &'a ShapeRef,
    /// Which way the part's paths were offset from it.
    pub how: Offset,
    /// The part's paths: the outline offset by the tool radius.
    pub centres: &'a [Contour],
}

/// The allowance paths of one outline (#284): the outline offset by the
/// radius plus `allowance_mm` the same way as its part paths, one for
/// each of `outline.centres`, in their order. The design's "band and
/// narrowness checks with `D + 2a`" (A8 step 2):
///
/// - Inside, an allowance path that comes closer than `r + a` to another
///   line of the region would cut that line's skin, or the part beyond
///   it: [`CamError::BandNarrowerThanAllowance`] naming both lines.
/// - Where the outline is wider than the bit but narrower than `D + 2a`
///   (a neck, a slot, a small hole, the mouth of an inlet), the
///   allowance offset vanishes, splits or merges, so its loops are not
///   one for each part path: [`CamError::ShapeNarrowerThanAllowance`].
/// - The last pass takes off, at any point, at most how far its path
///   runs from the allowance path there: that must be at most
///   `√2 · a` (a square corner's) plus [`THICKNESS_SLACK_MM`] (#310
///   review). A notch, slot or tip the earlier passes skip or stop short
///   of, or a corner sharper than 90°, would leave the last pass more to
///   cut at full depth: [`CamError::ShapeNarrowerThanAllowance`] too.
pub(super) fn twins(
    op: &ProfileOp,
    tool: &Tool,
    outline: &Outline<'_>,
    shapes: &[(ShapeRef, Contour)],
    allowance_mm: f64,
    tolerance_mm: f64,
) -> Result<Vec<Contour>, CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let narrow = || CamError::ShapeNarrowerThanAllowance {
        op: op.id.clone(),
        shape: outline.shape.clone(),
        diameter_mm: tool.diameter_mm,
        allowance_mm,
    };
    let reach_mm = radius_mm + allowance_mm;
    let found = match outline.how {
        Offset::Grow => offset(outline.contour, reach_mm),
        Offset::Shrink => offset(outline.contour, -reach_mm),
        // Refused before: On line has no allowance.
        Offset::None => return Err(narrow()),
    };
    if found.len() != outline.centres.len() {
        return Err(narrow());
    }
    check_band(
        &op.id,
        op.side,
        tool,
        allowance_mm,
        outline.region,
        (outline.contour, outline.shape),
        &found,
        shapes,
        tolerance_mm,
    )?;
    // Each part path with the allowance path nearest it, never one twice.
    let mut left: Vec<Option<Contour>> = found.into_iter().map(Some).collect();
    let mut twins = Vec::with_capacity(left.len());
    for centre in outline.centres {
        let nearest = left
            .iter()
            .enumerate()
            .filter_map(|(k, t)| {
                t.as_ref()
                    .map(|t| (k, distance_mm(centre.polyline(), t.polyline())))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        let Some(twin) = nearest.and_then(|k| left.get_mut(k)).and_then(Option::take) else {
            return Err(narrow());
        };
        // The last pass takes off at most how far its path runs from the
        // allowance path's (its cutter reaches no farther past theirs).
        let step_mm = (radius_mm / 8.0).min(0.25);
        let thickest_mm = std::f64::consts::SQRT_2 * allowance_mm + THICKNESS_SLACK_MM;
        if farthest_mm(centre.polyline(), twin.polyline(), step_mm) > thickest_mm {
            return Err(narrow());
        }
        twins.push(twin);
    }
    Ok(twins)
}

/// #284: an allowance path `twin` of `shape`, an outline of region
/// `region`, must keep the tool radius from every line of every other
/// region and, Outside, lie outside every other region's material, or its
/// passes would cut into that part (S9 holds them to the radius; only a
/// lap at the radius itself is D6's warning instead):
/// [`CamError::BandNarrowerThanAllowance`] naming both shapes, in drawing
/// order, and the gap between them. Inside, an allowance path lies in its
/// own region, which no other region's material overlaps.
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the gap; a struct would only rename them"
)]
pub(super) fn check_neighbours(
    op: &ProfileOp,
    tool: &Tool,
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
    (twin, shape, contour): (&Polyline, &ShapeRef, &Contour),
    region: usize,
    allowance_mm: f64,
) -> Result<(), CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let drawn = |s: &ShapeRef| shapes.iter().position(|(x, _)| x == s);
    for (index, other_region) in regions.iter().enumerate() {
        if index == region {
            continue;
        }
        // #310 review: a path one radius or more past another part's line
        // is inside that part, which `within` does not see.
        let inside = op.side == Side::Outside
            && twin
                .vertices()
                .first()
                .is_some_and(|v| other_region.contains_point(v.point()));
        for (line, other) in region_shapes(other_region, shapes) {
            if !inside && !within(twin, line.polyline(), radius_mm) {
                continue;
            }
            let (first, second) = if drawn(shape) <= drawn(other) {
                (shape, other)
            } else {
                (other, shape)
            };
            return Err(CamError::BandNarrowerThanAllowance {
                op: op.id.clone(),
                shape: first.clone(),
                other: Box::new(second.clone()),
                band_mm: distance_mm(contour.polyline(), line.polyline()),
                diameter_mm: tool.diameter_mm,
                allowance_mm,
                needed_mm: tool.diameter_mm + allowance_mm,
            });
        }
    }
    Ok(())
}

/// How far the point of `path` farthest from `other` lies from it, mm,
/// sampled at every vertex and at most `step_mm` apart along each
/// segment.
fn farthest_mm(path: &Polyline, other: &Polyline, step_mm: f64) -> f64 {
    let gap = |p: Point| nearest_point(other, p).map_or(f64::INFINITY, |n| n.gap_mm);
    let mut farthest: f64 = 0.0;
    for segment in path.segments() {
        let length_mm = segment.length_mm();
        let mut along_mm = 0.0;
        loop {
            if let Some((p, _)) = segment.point_at(along_mm) {
                farthest = farthest.max(gap(p));
            }
            if along_mm >= length_mm {
                break;
            }
            along_mm = (along_mm + step_mm.max(EPS_MM)).min(length_mm);
        }
    }
    farthest
}

/// `path`, closed, run the other way from the same start: its last pass
/// reversed (#284).
pub(super) fn reversed_from_start(path: &Polyline) -> Polyline {
    let reversed = path.reversed();
    let mut vertices: Vec<Vertex> = reversed.vertices().to_vec();
    // `reversed` starts at the last vertex; the first is now its last.
    if let Some(first) = vertices.pop() {
        vertices.insert(0, first);
    }
    Polyline::new(vertices, true)
}

/// `layout`, the tabs of the closed `path`, carried onto `back`, the same
/// path run the other way ([`reversed_from_start`]), so they stay where
/// they are ([`PathTabs::carried`]): a distance along `path` lands where
/// `back` passes its point.
pub(super) fn carry_reversed(path: &Polyline, layout: &PathTabs, back: &Polyline) -> PathTabs {
    let along = |distance_mm: f64| {
        path.point_at(distance_mm)
            .and_then(|(p, _)| nearest_point(back, p))
            .map_or(0.0, |n| n.along_mm)
    };
    layout.carried(back.length_mm(), along, true, 0.0)
}

/// One sample of the last pass for [`twin_tabs`]: its cutter centre,
/// its Z, and the top and shape of the tab it rises over there.
type LastCut = (Point, f64, Option<(f64, TabShape)>);

/// One sample of an allowance path for [`twin_tabs`]: its distance
/// along the path and, where a tab is in reach, the height it must keep
/// above, the top of that tab, and whether every tab in reach is smooth.
type Need = (f64, Option<(f64, f64, bool)>);

/// The sampling step of [`twin_tabs`], mm: a twelfth of the tool radius,
/// at least 0.02 and at most 0.25.
fn tab_step_mm(radius_mm: f64) -> f64 {
    (radius_mm / 12.0).clamp(0.02, 0.25)
}

/// Points of `path` every `step_mm` along it from its start (and its
/// end), with their distance along it, walking only segments longer
/// than [`EPS_MM`], as [`Polyline::point_at`] does.
fn path_samples(path: &Polyline, step_mm: f64) -> Vec<(f64, Point)> {
    let mut out = Vec::new();
    let mut walked_mm = 0.0;
    let mut next_mm = 0.0;
    for segment in path.segments() {
        let length_mm = segment.length_mm();
        if length_mm <= EPS_MM {
            continue;
        }
        while next_mm <= walked_mm + length_mm {
            if let Some((p, _)) = segment.point_at(next_mm - walked_mm) {
                out.push((next_mm, p));
            }
            next_mm += step_mm;
        }
        walked_mm += length_mm;
    }
    out
}

/// Points bucketed by a square cell, for finding the points near one.
struct Buckets<T> {
    cell_mm: f64,
    cells: BTreeMap<(i64, i64), Vec<(Point, T)>>,
}

impl<T: Copy> Buckets<T> {
    fn new(cell_mm: f64, points: impl IntoIterator<Item = (Point, T)>) -> Self {
        let mut buckets = Self {
            cell_mm,
            cells: BTreeMap::new(),
        };
        for (p, value) in points {
            let key = buckets.key(p);
            buckets.cells.entry(key).or_default().push((p, value));
        }
        buckets
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "cell indices of coordinates within MAX_COORDINATE_MM"
    )]
    fn key(&self, p: Point) -> (i64, i64) {
        (
            (p.x / self.cell_mm).floor() as i64,
            (p.y / self.cell_mm).floor() as i64,
        )
    }

    /// The values of the points closer than `reach_mm`, at most the cell
    /// size, to `p`.
    fn near(&self, p: Point, reach_mm: f64) -> impl Iterator<Item = T> + '_ {
        let (i, j) = self.key(p);
        (-1..=1)
            .flat_map(move |di| (-1..=1).map(move |dj| (i + di, j + dj)))
            .filter_map(|key| self.cells.get(&key))
            .flatten()
            .filter(move |(q, _)| q.distance_to(p) < reach_mm)
            .map(|(_, value)| *value)
    }
}

/// The tabs of an allowance path `twin` (#310 review, replacing design A8
/// step 3's widening by `a`), from the tabs the last pass leaves on its
/// path `path` (`layout`, its floor at `floor_z_mm`), cut with a tool of
/// `radius_mm`.
///
/// A tab is the stock the last pass leaves standing above its floor: a
/// point within the radius of the path stands as high as the lowest the
/// last pass cuts within the radius of it. Each point of `twin` is raised
/// to at least the highest such point within the radius of it, so no
/// pass before the last cuts any of it, however the two paths turn: at a
/// sharp corner the allowance path runs past the corner beside the far
/// edge's tabs, which an arc length along it does not see. The heights are
/// sampled every [`tab_step_mm`], the stock on a lattice of that step;
/// the cutter is taken that much wider and each raised stretch that much
/// longer at both ends, so the lattice and sampling never let a cut
/// through. A stretch is rect if any tab it carries is, else smooth, its
/// ramps no longer than keeps it at or above every height sampled under
/// them. Each earlier pass at `z` cuts a smooth stretch's fraction `f` of
/// the way from `z` up to its top; the heights are the last pass's, at
/// its floor, which is lower, so the same fraction is at least as high
/// there.
pub(super) fn twin_tabs(
    path: &Polyline,
    layout: &PathTabs,
    twin: &Polyline,
    radius_mm: f64,
    floor_z_mm: f64,
) -> PathTabs {
    let twin_mm = twin.length_mm();
    let none = || PathTabs::from_spans(twin_mm, layout.tab_count(), &[]);
    if layout.is_untabbed() || twin_mm <= EPS_MM || path.length_mm() <= EPS_MM {
        return none();
    }
    let step_mm = tab_step_mm(radius_mm);
    let raised = |z_mm: f64| z_mm > floor_z_mm + EPS_MM;
    // The last pass: its cutter centre every half step, with its Z and
    // the top and shape of the tab it rises over.
    let last: Vec<LastCut> = path_samples(path, step_mm / 2.0)
        .into_iter()
        .map(|(d, p)| (p, layout.z_at_mm(d, floor_z_mm), layout.span_at(d)))
        .collect();
    let cuts = Buckets::new(radius_mm, last.iter().map(|&(p, z, _)| (p, z)));
    // The tabs: lattice points within the radius of a raised cut, at the
    // height the last pass leaves them, with the top and shape of the
    // tab whose cut stands over them.
    let mut lattice: BTreeMap<(i64, i64), (f64, TabShape)> = BTreeMap::new();
    let reach = lattice_reach(radius_mm, step_mm);
    for &(p, z, span) in &last {
        let Some(span) = span.filter(|_| raised(z)) else {
            continue;
        };
        let (ci, cj) = lattice_index(p, step_mm);
        for di in -reach..=reach {
            for dj in -reach..=reach {
                lattice
                    .entry((ci + di, cj + dj))
                    .and_modify(|(top, shape)| {
                        *top = top.max(span.0);
                        if span.1 == TabShape::Rect {
                            *shape = TabShape::Rect;
                        }
                    })
                    .or_insert(span);
            }
        }
    }
    let material: Vec<(Point, (f64, f64, TabShape))> = lattice
        .into_iter()
        .filter_map(|((i, j), (top, shape))| {
            let p = lattice_point(i, j, step_mm);
            let height = cuts.near(p, radius_mm).reduce(f64::min)?;
            raised(height).then_some((p, (height, top, shape)))
        })
        .collect();
    if material.is_empty() {
        return none();
    }
    let stock = Buckets::new(radius_mm + step_mm, material);
    // Each point of the allowance path: the height it must keep above,
    // the tab top and whether every tab under it is smooth.
    let need: Vec<Need> = path_samples(twin, step_mm / 2.0)
        .into_iter()
        .map(|(d, q)| {
            let under = stock.near(q, radius_mm + step_mm).fold(
                None,
                |acc: Option<(f64, f64, bool)>, (height, top, shape)| {
                    let smooth = shape == TabShape::Smooth;
                    Some(match acc {
                        None => (height, top, smooth),
                        Some((h, t, s)) => (h.max(height), t.max(top), s && smooth),
                    })
                },
            );
            (d, under)
        })
        .collect();
    let spans = raised_runs(&need, twin_mm, step_mm, floor_z_mm);
    PathTabs::from_spans(twin_mm, layout.tab_count(), &spans)
}

/// How many lattice steps of `step_mm` reach past the radius.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a radius over a step of at least 0.02 mm"
)]
fn lattice_reach(radius_mm: f64, step_mm: f64) -> i64 {
    (radius_mm / step_mm).ceil() as i64 + 1
}

/// The lattice point nearest `p`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "lattice indices of coordinates within MAX_COORDINATE_MM"
)]
fn lattice_index(p: Point, step_mm: f64) -> (i64, i64) {
    (
        (p.x / step_mm).round() as i64,
        (p.y / step_mm).round() as i64,
    )
}

/// The point of lattice index `(i, j)`.
#[allow(
    clippy::cast_precision_loss,
    reason = "lattice indices of coordinates within MAX_COORDINATE_MM"
)]
fn lattice_point(i: i64, j: i64, step_mm: f64) -> Point {
    Point::new(i as f64 * step_mm, j as f64 * step_mm)
}

/// The raised stretches of an allowance path `length_mm` long, from the
/// heights `need` sampled along it every half `step_mm` ([`twin_tabs`]):
/// each run of samples that must keep above `floor_z_mm`, a step longer
/// at both ends, at the highest top under it, rect unless every tab under
/// it is smooth.
fn raised_runs(need: &[Need], length_mm: f64, step_mm: f64, floor_z_mm: f64) -> Vec<RaisedSpan> {
    let up = |k: usize| {
        need.get(k)
            .and_then(|(_, n)| *n)
            .filter(|(h, _, _)| *h > floor_z_mm + EPS_MM)
    };
    let n = need.len();
    // Start at a sample that stands clear, so no run wraps the scan.
    let Some(first_clear) = (0..n).find(|&k| up(k).is_none()) else {
        let top = need
            .iter()
            .filter_map(|(_, n)| n.map(|(_, t, _)| t))
            .fold(floor_z_mm, f64::max);
        let smooth = need.iter().all(|(_, n)| n.is_none_or(|(_, _, s)| s));
        return vec![RaisedSpan {
            start_mm: 0.0,
            length_mm,
            top_z_mm: top,
            shape: if smooth {
                TabShape::Smooth
            } else {
                TabShape::Rect
            },
            ramp_in_mm: 0.0,
            ramp_out_mm: 0.0,
        }];
    };
    let mut spans = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for offset in 1..=n {
        let k = (first_clear + offset) % n;
        if up(k).is_some() {
            run.push(k);
            continue;
        }
        if !run.is_empty() {
            spans.push(raised_span(need, &run, length_mm, step_mm, floor_z_mm));
            run.clear();
        }
    }
    spans
}

/// The raised stretch over the samples `run` of `need`, in order along
/// the path (possibly through its start), for [`raised_runs`].
fn raised_span(
    need: &[Need],
    run: &[usize],
    length_mm: f64,
    step_mm: f64,
    floor_z_mm: f64,
) -> RaisedSpan {
    let at = |k: usize| need.get(k).map_or(0.0, |(d, _)| *d);
    let first = run.first().map_or(0.0, |&k| at(k));
    let last = run.last().map_or(first, |&k| at(k));
    let start_mm = first - step_mm;
    let run_mm = (last - first).rem_euclid(length_mm) + 2.0 * step_mm;
    let samples: Vec<(f64, f64)> = run
        .iter()
        .filter_map(|&k| need.get(k).and_then(|(_, n)| n.map(|(h, _, _)| (at(k), h))))
        .collect();
    let top = run
        .iter()
        .filter_map(|&k| need.get(k).and_then(|(_, n)| n.map(|(_, t, _)| t)))
        .fold(floor_z_mm, f64::max);
    let smooth = run
        .iter()
        .all(|&k| need.get(k).is_none_or(|(_, n)| n.is_none_or(|(_, _, s)| s)));
    if !smooth || top <= floor_z_mm + EPS_MM {
        return RaisedSpan {
            start_mm,
            length_mm: run_mm,
            top_z_mm: top,
            shape: TabShape::Rect,
            ramp_in_mm: 0.0,
            ramp_out_mm: 0.0,
        };
    }
    // The longest ramps that keep the stretch at or above every sample:
    // the fraction of the way up from the floor it must be at each.
    let mut ramp_in_mm = run_mm / 2.0;
    let mut ramp_out_mm = run_mm / 2.0;
    for (d, height) in samples {
        let fraction = ((height - floor_z_mm) / (top - floor_z_mm)).clamp(0.0, 1.0);
        if fraction <= 0.0 {
            continue;
        }
        let from_start_mm = (d - start_mm).rem_euclid(length_mm);
        let to_end_mm = run_mm - from_start_mm;
        ramp_in_mm = ramp_in_mm.min(from_start_mm / fraction);
        ramp_out_mm = ramp_out_mm.min(to_end_mm / fraction);
    }
    RaisedSpan {
        start_mm,
        length_mm: run_mm,
        top_z_mm: top,
        shape: TabShape::Smooth,
        ramp_in_mm: ramp_in_mm.max(EPS_MM),
        ramp_out_mm: ramp_out_mm.max(EPS_MM),
    }
}
