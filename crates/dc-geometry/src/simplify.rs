// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Douglas-Peucker simplification of closed contours (sub-phase 3.4 Image
//! Prep, `docs/specs/phase-3/design.md`, "Image prep" step 5c; slice 21b,
//! #595).
//!
//! In-house and about sixty lines of algorithm: `geo`'s `Simplify` is a
//! dependency for one job and `kurbo::simplify` fits Béziers, which a
//! polygonal outline does not want. Every dropped vertex lies within the
//! tolerance of the chord that replaces it, so a corner between two
//! segments longer than twice the tolerance is never rounded.

use crate::GeometryError;
use crate::contour::Contour;
use crate::point::{EPS_MM, Point};
use crate::polyline::{Polyline, Segment, Vertex};
use crate::region::{Region, nest};
use std::cmp::Ordering;

/// Arcs are flattened to this fraction of the simplify tolerance first, so
/// the result stays within 1.25 tolerances of the true curve.
const FLATTEN_FRACTION: f64 = 0.25;

/// `contour` with the vertices that lie within `tolerance_mm` of the chord
/// between their kept neighbours dropped (Douglas-Peucker), with at least
/// three vertices kept. Arcs are flattened first, within a quarter of the
/// tolerance, so the result is polygonal. A tolerance that is not a
/// positive finite number drops only vertices exactly on a chord.
///
/// Deterministic: the two anchors, always kept, are the contour's
/// lowest-then-leftmost vertex and the vertex farthest from it (the lowest,
/// leftmost of equals), so the result does not depend on where the input
/// starts. The result starts at the first anchor and keeps the input's
/// winding. A contour whose simplified form would cross itself comes back
/// flattened but otherwise as it was.
#[must_use]
pub fn simplify(contour: &Contour, tolerance_mm: f64) -> Contour {
    simplified(contour, tolerance_mm).unwrap_or_else(|| {
        Contour::new(flattened(contour, tolerance_mm)).unwrap_or_else(|_| contour.clone())
    })
}

/// `simplify` on every contour of every region. A region whose simplified
/// contours cross a contour of another region, or each other (a hole out
/// through its outer, or a contour across itself), is returned as it was;
/// the second element lists those regions' positions in `regions`,
/// ascending (`prep_smooth_skipped`). Regions stay in order.
///
/// The crossing test is [`nest`]'s own (the area two contours share). A
/// region is put back when its simplified form crosses the *original* of
/// the other, since then it is the one at fault; when only the two
/// simplified forms together cross, both are put back. Run once, not to a
/// fixed point.
#[must_use]
pub fn simplify_regions(regions: &[Region], tolerance_mm: f64) -> (Vec<Region>, Vec<usize>) {
    let originals: Vec<Vec<Contour>> = regions.iter().map(contours_of).collect();
    // `None`: this region cannot be simplified (a contour would cross
    // itself, or the contours no longer nest as one region).
    let mut simplified: Vec<Option<Vec<Contour>>> = originals
        .iter()
        .map(|contours| {
            let candidate: Option<Vec<Contour>> = contours
                .iter()
                .map(|c| simplified(c, tolerance_mm))
                .collect();
            candidate.filter(|c| is_one_region(c))
        })
        .collect();
    // Put regions back until nothing crosses; each pass puts back at least
    // one, so it ends.
    while let Some((first, second)) = crossing(&originals, &simplified) {
        let faulty = |own: usize, other: usize| {
            let (Some(Some(mine)), Some(theirs)) = (simplified.get(own), originals.get(other))
            else {
                return false;
            };
            let both: Vec<Contour> = mine.iter().chain(theirs).cloned().collect();
            matches!(nest(both), Err(GeometryError::ContoursCross { .. }))
        };
        let (a, b) = (faulty(first, second), faulty(second, first));
        let to_revert: Vec<usize> = match (a, b) {
            (true, false) => vec![first],
            (false, true) => vec![second],
            _ => vec![first, second],
        };
        for r in to_revert {
            if let Some(slot) = simplified.get_mut(r) {
                *slot = None;
            }
        }
    }
    let mut skipped = Vec::new();
    let out = regions
        .iter()
        .zip(simplified)
        .enumerate()
        .map(|(k, (region, new))| {
            new.and_then(|contours| region.with_contours(contours))
                .unwrap_or_else(|| {
                    skipped.push(k);
                    region.clone()
                })
        })
        .collect();
    (out, skipped)
}

/// A region's contours: outer first, then its holes.
fn contours_of(region: &Region) -> Vec<Contour> {
    std::iter::once(region.outer().clone())
        .chain(region.holes().iter().cloned())
        .collect()
}

/// Whether `contours` nest into exactly one region.
fn is_one_region(contours: &[Contour]) -> bool {
    nest(contours.to_vec()).is_ok_and(|(regions, _)| regions.len() == 1)
}

/// The first two regions, by position, whose current contours (simplified
/// where `simplified` has them, else the original) cross.
fn crossing(
    originals: &[Vec<Contour>],
    simplified: &[Option<Vec<Contour>>],
) -> Option<(usize, usize)> {
    let mut contours = Vec::new();
    let mut owner = Vec::new();
    for (k, (original, new)) in originals.iter().zip(simplified).enumerate() {
        for c in new.as_ref().unwrap_or(original) {
            contours.push(c.clone());
            owner.push(k);
        }
    }
    match nest(contours) {
        Err(GeometryError::ContoursCross { first, second }) => {
            let (a, b) = (owner.get(first).copied()?, owner.get(second).copied()?);
            // Two contours of one region crossing cannot happen for a
            // region that passed `is_one_region`, or for an original.
            (a != b).then_some((a.min(b), a.max(b)))
        }
        _ => None,
    }
}

/// `contour`'s arcs flattened within a quarter of `tolerance_mm`.
fn flattened(contour: &Contour, tolerance_mm: f64) -> Polyline {
    let flatten_mm = if tolerance_mm.is_finite() && tolerance_mm > 0.0 {
        (tolerance_mm * FLATTEN_FRACTION).max(EPS_MM)
    } else {
        EPS_MM
    };
    contour.polyline().with_arcs_flattened(flatten_mm)
}

/// [`simplify`], or `None` when the result would cross itself or would
/// not be a contour.
fn simplified(contour: &Contour, tolerance_mm: f64) -> Option<Contour> {
    let tolerance_mm = if tolerance_mm.is_finite() && tolerance_mm > 0.0 {
        tolerance_mm
    } else {
        0.0
    };
    let polyline = flattened(contour, tolerance_mm);
    let mut points: Vec<Point> = Vec::with_capacity(polyline.vertices().len());
    for v in polyline.vertices() {
        let p = v.point();
        if points.last() != Some(&p) {
            points.push(p);
        }
    }
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let n = points.len();
    if n < 3 {
        return None;
    }
    // Rotate to the first anchor: the lowest, then leftmost, vertex.
    let start = (0..n).reduce(|best, k| {
        if lower(points.get(k), points.get(best)) {
            k
        } else {
            best
        }
    })?;
    points.rotate_left(start);
    let anchor = points.first().copied()?;
    // The second anchor: the vertex farthest from the first.
    let far = (1..n).reduce(|best, k| {
        let (dk, db) = (
            dist2(points.get(k), anchor),
            dist2(points.get(best), anchor),
        );
        if dk.total_cmp(&db) == Ordering::Greater
            || (dk.total_cmp(&db).is_eq() && lower(points.get(k), points.get(best)))
        {
            k
        } else {
            best
        }
    })?;
    let mut keep = vec![false; n];
    for k in [0, far] {
        if let Some(flag) = keep.get_mut(k) {
            *flag = true;
        }
    }
    // The two chains between the anchors; the second closes on vertex 0.
    chain(&points, 0, far, tolerance_mm, &mut keep);
    chain(&points, far, n, tolerance_mm, &mut keep);
    let kept = keep.iter().filter(|&&k| k).count();
    // A contour has at least three vertices: keep the one farthest from
    // the line through the anchors.
    if kept < 3 {
        let spine = Segment {
            start: anchor,
            end: points.get(far).copied()?,
            bulge: 0.0,
        };
        let extra = (1..n)
            .filter(|&k| !keep.get(k).copied().unwrap_or(true))
            .fold(None, |best: Option<(usize, f64)>, k| {
                let d = points.get(k).map_or(0.0, |p| {
                    crate::distance::point_segment_distance_mm(spine, *p)
                });
                match best {
                    Some((_, bd)) if bd >= d => best,
                    _ => Some((k, d)),
                }
            });
        let (k, _) = extra?;
        if let Some(flag) = keep.get_mut(k) {
            *flag = true;
        }
    }
    let vertices: Vec<Vertex> = points
        .iter()
        .zip(&keep)
        .filter(|&(_, &k)| k)
        .map(|(p, _)| Vertex::line(p.x, p.y))
        .collect();
    let out = Contour::new(Polyline::new(vertices, true)).ok()?;
    // `Contour::new` winds counter-clockwise; a hole stays clockwise.
    Some(if contour.is_ccw() {
        out
    } else {
        out.reversed()
    })
}

/// Douglas-Peucker on `points[from..=to]` (index `n` is `points[0]`, the
/// ring closing), marking the vertices it keeps. Iterative, so a long
/// contour cannot exhaust the stack.
fn chain(points: &[Point], from: usize, to: usize, tolerance_mm: f64, keep: &mut [bool]) {
    let at = |k: usize| points.get(k % points.len().max(1)).copied();
    let mut stack = vec![(from, to)];
    while let Some((i, j)) = stack.pop() {
        let (Some(a), Some(b)) = (at(i), at(j)) else {
            continue;
        };
        let chord = Segment {
            start: a,
            end: b,
            bulge: 0.0,
        };
        let mut worst: Option<(usize, f64)> = None;
        for k in (i + 1)..j {
            let Some(p) = at(k) else { continue };
            let d = crate::distance::point_segment_distance_mm(chord, p);
            if worst.is_none_or(|(_, wd)| d > wd) {
                worst = Some((k, d));
            }
        }
        if let Some((k, d)) = worst
            && d > tolerance_mm
        {
            if let Some(flag) = keep.get_mut(k) {
                *flag = true;
            }
            stack.push((i, k));
            stack.push((k, j));
        }
    }
}

/// Whether `a` is lower than `b`, then further left.
fn lower(a: Option<&Point>, b: Option<&Point>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)).is_lt(),
        _ => false,
    }
}

/// Squared distance from `p` to `to`, mm².
fn dist2(p: Option<&Point>, to: Point) -> f64 {
    p.map_or(0.0, |p| (p.x - to.x).powi(2) + (p.y - to.y).powi(2))
}

#[cfg(test)]
mod tests;
