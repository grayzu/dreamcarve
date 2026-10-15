// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Regions: an outer contour with zero or more holes, and [`nest`], which
//! groups loose contours into regions by even-odd containment
//! (`docs/specs/phase-1/design.md`, "Data model → dc-geometry").

use crate::GeometryError;
use crate::GeometryWarning;
use crate::contour::Contour;
use crate::offset::to_cavalier;
use crate::point::{EPS_MM, Point, Vec2};
use crate::polyline::{Arc, Segment};
use cavalier_contours::polyline::{BooleanOp, PlineSource};
use std::f64::consts::{PI, TAU};

/// An area to machine: one counter-clockwise outer contour and zero or more
/// clockwise holes inside it (ADR-0001). Coordinates are millimetres, Y-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    outer: Contour,
    holes: Vec<Contour>,
}

impl Region {
    /// The outer boundary; always counter-clockwise.
    #[must_use]
    pub fn outer(&self) -> &Contour {
        &self.outer
    }

    /// The holes, in the input order given to [`nest`]; each is clockwise
    /// and lies inside [`Region::outer`].
    #[must_use]
    pub fn holes(&self) -> &[Contour] {
        &self.holes
    }
}

/// Groups closed contours (millimetres, Y-up) into regions by even-odd
/// containment. A contour inside no other contour (depth 0) is an outer;
/// one inside exactly one (depth 1) is a hole of its innermost container;
/// depth 2 is an island and starts a new region, and so on. Outers come
/// back counter-clockwise and holes clockwise, whatever the input winding.
///
/// Regions are ordered by the input position of their outer contour, and
/// holes by their own input position, so the result depends only on the
/// input.
///
/// Every pair of contours is first compared by the area they share,
/// computed on the true curves. Contours that cross (share area, but
/// neither contains the other) fail with [`GeometryError::ContoursCross`];
/// touching is not crossing. A contour identical to an earlier one,
/// whatever its winding, is dropped with
/// [`GeometryWarning::DuplicateContourDropped`]. The rules, with figures,
/// are in the user guide chapter "Shapes, holes, and islands".
pub fn nest(contours: Vec<Contour>) -> Result<(Vec<Region>, Vec<GeometryWarning>), GeometryError> {
    let mut keep = vec![true; contours.len()];
    let mut warnings = Vec::new();
    for (i, first) in contours.iter().enumerate() {
        for (j, second) in contours.iter().enumerate().skip(i + 1) {
            let both_kept =
                keep.get(i).copied().unwrap_or(false) && keep.get(j).copied().unwrap_or(false);
            if !both_kept {
                continue;
            }
            match overlap(first, second) {
                Overlap::Same => {
                    if let Some(flag) = keep.get_mut(j) {
                        *flag = false;
                    }
                    warnings.push(GeometryWarning::DuplicateContourDropped {
                        index: j,
                        duplicate_of: i,
                    });
                }
                Overlap::Cross => {
                    return Err(GeometryError::ContoursCross {
                        first: i,
                        second: j,
                    });
                }
                Overlap::Apart | Overlap::Nested => {}
            }
        }
    }
    let kept = contours
        .into_iter()
        .zip(keep)
        .filter_map(|(contour, kept)| kept.then_some(contour))
        .collect();
    Ok((nest_unchecked(kept), warnings))
}

/// How two closed contours relate, judged by the area they share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlap {
    /// No shared area: separate, or touching from outside.
    Apart,
    /// One lies wholly inside the other (edges may touch).
    Nested,
    /// The same shape, whatever the winding.
    Same,
    /// They share some area but neither contains the other.
    Cross,
}

/// Compares the area `a` and `b` share with each one's own area. Exact for
/// arcs, since the intersection is computed on the curves themselves. The
/// tolerance is a strip [`EPS_MM`] wide along both boundaries, so shapes
/// that only touch count as apart or nested, not crossing.
fn overlap(a: &Contour, b: &Contour) -> Overlap {
    let (Some(box_a), Some(box_b)) = (a.polyline().bounds(), b.polyline().bounds()) else {
        return Overlap::Apart;
    };
    if box_a.max.x < box_b.min.x
        || box_b.max.x < box_a.min.x
        || box_a.max.y < box_b.min.y
        || box_b.max.y < box_a.min.y
    {
        return Overlap::Apart;
    }
    let tolerance_mm2 = EPS_MM * (a.polyline().length_mm() + b.polyline().length_mm());
    let shared_mm2 = shared_area_mm2(a, b);
    if shared_mm2 <= tolerance_mm2 {
        return Overlap::Apart;
    }
    let all_of = |c: &Contour| (c.signed_area_mm2().abs() - shared_mm2).abs() <= tolerance_mm2;
    match (all_of(a), all_of(b)) {
        (true, true) => Overlap::Same,
        (true, false) | (false, true) => Overlap::Nested,
        (false, false) => Overlap::Cross,
    }
}

/// Area of the intersection of `a` and `b`, mm².
fn shared_area_mm2(a: &Contour, b: &Contour) -> f64 {
    let ccw = |c: &Contour| to_cavalier(with_ccw(c.clone(), true).polyline());
    let result = ccw(a).boolean(&ccw(b), BooleanOp::And);
    let filled: f64 = result.pos_plines.iter().map(|p| p.pline.area().abs()).sum();
    let cut_out: f64 = result.neg_plines.iter().map(|p| p.pline.area().abs()).sum();
    (filled - cut_out).max(0.0)
}

/// Nesting proper, for contours already known not to cross or repeat.
fn nest_unchecked(contours: Vec<Contour>) -> Vec<Region> {
    let areas_mm2: Vec<f64> = contours.iter().map(|c| c.signed_area_mm2().abs()).collect();
    let sized: Vec<(&Contour, f64)> = contours.iter().zip(areas_mm2.iter().copied()).collect();

    // For every contour, the indices of the contours that enclose it.
    let containers: Vec<Vec<usize>> = sized
        .iter()
        .enumerate()
        .map(|(i, &inner)| {
            sized
                .iter()
                .enumerate()
                .filter(|&(j, &outer)| j != i && encloses(outer, inner))
                .map(|(j, _)| j)
                .collect()
        })
        .collect();
    let depths: Vec<usize> = containers.iter().map(Vec::len).collect();
    let is_outer = |j: usize| depths.get(j).is_some_and(|d| d % 2 == 0);

    let roles: Vec<Role> = containers
        .iter()
        .zip(&depths)
        .map(|(enclosing, depth)| {
            if depth % 2 == 0 {
                return Role::Outer;
            }
            // A hole belongs to the innermost enclosing outer: the one with
            // the least area, lowest input index on a tie.
            enclosing
                .iter()
                .copied()
                .filter(|&j| is_outer(j))
                .min_by(|&a, &b| {
                    let area = |k: usize| areas_mm2.get(k).copied().unwrap_or(f64::INFINITY);
                    area(a).total_cmp(&area(b)).then(a.cmp(&b))
                })
                .map_or(Role::Outer, Role::HoleOf)
        })
        .collect();

    let mut regions: Vec<(usize, Region)> = Vec::new();
    let mut holes: Vec<(usize, Contour)> = Vec::new();
    for ((index, contour), role) in contours.into_iter().enumerate().zip(roles) {
        match role {
            Role::Outer => regions.push((
                index,
                Region {
                    outer: with_ccw(contour, true),
                    holes: Vec::new(),
                },
            )),
            Role::HoleOf(parent) => holes.push((parent, with_ccw(contour, false))),
        }
    }
    for (parent, hole) in holes {
        if let Some((_, region)) = regions.iter_mut().find(|(index, _)| *index == parent) {
            region.holes.push(hole);
        }
    }
    regions.into_iter().map(|(_, region)| region).collect()
}

/// What [`nest`] makes of one input contour.
#[derive(Debug, Clone, Copy)]
enum Role {
    /// Starts a region.
    Outer,
    /// A hole of the region whose outer has this input index.
    HoleOf(usize),
}

/// `contour` wound counter-clockwise when `ccw`, else clockwise.
fn with_ccw(contour: Contour, ccw: bool) -> Contour {
    if contour.is_ccw() == ccw {
        contour
    } else {
        contour.reversed()
    }
}

/// True when `outer` (contour, |area| mm²) encloses `inner`. Assumes the two
/// do not cross: `inner` must be smaller, and its first segment midpoint
/// that is off `outer`'s boundary must lie inside `outer`.
fn encloses(outer: (&Contour, f64), inner: (&Contour, f64)) -> bool {
    let ((outer, outer_mm2), (inner, inner_mm2)) = (outer, inner);
    if inner_mm2 >= outer_mm2 {
        return false;
    }
    inner
        .polyline()
        .segments()
        .filter_map(|seg| seg.point_at(seg.length_mm() / 2.0).map(|(p, _)| p))
        .find(|&p| !on_boundary(outer, p))
        .is_some_and(|p| winding_number(outer, p) != 0)
}

/// True when `p` is within [`EPS_MM`] of any segment of `contour`.
fn on_boundary(contour: &Contour, p: Point) -> bool {
    contour
        .polyline()
        .segments()
        .any(|seg| distance_to_segment(seg, p) <= EPS_MM)
}

/// Shortest distance from `p` to `seg`, mm, following arcs.
fn distance_to_segment(seg: Segment, p: Point) -> f64 {
    let to_ends = || p.distance_to(seg.start).min(p.distance_to(seg.end));
    match seg.arc() {
        Some(arc) => {
            let v = p - arc.centre;
            if sweep_contains(arc, v.y.atan2(v.x)) {
                (v.length() - arc.radius_mm).abs()
            } else {
                to_ends()
            }
        }
        None => {
            let d = seg.end - seg.start;
            let len2 = d.dot(d);
            if len2 <= EPS_MM * EPS_MM {
                return to_ends();
            }
            let t = ((p - seg.start).dot(d) / len2).clamp(0.0, 1.0);
            p.distance_to(seg.start + d * t)
        }
    }
}

/// True when the direction `angle_rad` from the arc centre falls within the
/// arc's sweep.
fn sweep_contains(arc: Arc, angle_rad: f64) -> bool {
    let from_start = if arc.sweep_rad > 0.0 {
        (angle_rad - arc.start_angle_rad).rem_euclid(TAU)
    } else {
        (arc.start_angle_rad - angle_rad).rem_euclid(TAU)
    };
    from_start <= arc.sweep_rad.abs()
}

/// The winding number of `contour` about `p`: +1 inside a counter-clockwise
/// contour, −1 inside a clockwise one, 0 outside. Arc segments count by
/// their true curve, not their chord. Undefined for `p` on the boundary.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a whole number of turns, bounded by the segment count"
)]
fn winding_number(contour: &Contour, p: Point) -> i32 {
    let total_rad: f64 = contour
        .polyline()
        .segments()
        .map(|seg| subtended_angle_rad(seg, p))
        .sum();
    (total_rad / TAU).round() as i32
}

/// Signed angle, radians, that `seg` sweeps as seen from `p` (positive is
/// counter-clockwise). For an arc this is the chord's angle plus a full
/// turn when `p` lies in the circular segment between chord and arc, since
/// arc-then-chord-back is a loop around exactly that area.
fn subtended_angle_rad(seg: Segment, p: Point) -> f64 {
    let a = seg.start - p;
    let b = seg.end - p;
    let chord_rad = a.cross(b).atan2(a.dot(b));
    let Some(arc) = seg.arc() else {
        return chord_rad;
    };
    let chord = seg.end - seg.start;
    let chord_len = chord.length();
    let side = chord.cross(p - seg.start) / chord_len;
    if side.abs() <= EPS_MM {
        // On the chord's line. Between the ends, the arc passes round `p`
        // by half a turn in its own direction; beyond them `p` is outside
        // the disc and the chord angle (0) stands.
        let along = (p - seg.start).dot(chord);
        return if along > 0.0 && along < chord_len * chord_len {
            PI.copysign(arc.sweep_rad)
        } else {
            chord_rad
        };
    }
    let mid_rad = arc.start_angle_rad + arc.sweep_rad / 2.0;
    let mid = arc.centre + Vec2::new(mid_rad.cos(), mid_rad.sin()) * arc.radius_mm;
    let mid_side = chord.cross(mid - seg.start);
    let in_bow = p.distance_to(arc.centre) < arc.radius_mm && side * mid_side > 0.0;
    if in_bow {
        chord_rad + TAU.copysign(arc.sweep_rad)
    } else {
        chord_rad
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::polyline::Polyline;

    fn circle(x: f64, y: f64, r: f64) -> Contour {
        Contour::new(Polyline::circle(Point::new(x, y), r)).unwrap()
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(x, y), w, h)).unwrap()
    }

    /// `nest` on input expected to succeed without warnings.
    fn nest_ok(contours: Vec<Contour>) -> Vec<Region> {
        let (regions, warnings) = nest(contours).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        regions
    }

    fn area(c: &Contour) -> f64 {
        c.signed_area_mm2()
    }

    #[test]
    fn circle_in_circle_becomes_hole() {
        let regions = nest_ok(vec![circle(0.0, 0.0, 10.0), circle(0.0, 0.0, 5.0)]);
        assert_eq!(regions.len(), 1);
        let region = &regions[0];
        assert!(region.outer().is_ccw());
        assert_approx_eq!(area(region.outer()), PI * 100.0, 1e-9);
        assert_eq!(region.holes().len(), 1);
        assert!(!region.holes()[0].is_ccw(), "hole must be clockwise");
        assert_approx_eq!(area(&region.holes()[0]), -PI * 25.0, 1e-9);
    }

    #[test]
    fn disjoint_shapes_are_separate_regions() {
        let regions = nest_ok(vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 10.0, 5.0)]);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.holes().is_empty()));
        assert!(regions.iter().all(|r| r.outer().is_ccw()));
        assert_approx_eq!(area(regions[0].outer()), 100.0, EPS_MM);
        assert_approx_eq!(area(regions[1].outer()), 50.0, EPS_MM);
    }

    #[test]
    fn island_in_hole_becomes_its_own_region() {
        let regions = nest_ok(vec![
            circle(0.0, 0.0, 10.0),
            circle(0.0, 0.0, 7.0),
            circle(0.0, 0.0, 3.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), PI * 100.0, 1e-9);
        assert_eq!(regions[0].holes().len(), 1);
        assert_approx_eq!(area(&regions[0].holes()[0]), -PI * 49.0, 1e-9);
        assert_approx_eq!(area(regions[1].outer()), PI * 9.0, 1e-9);
        assert!(regions[1].outer().is_ccw());
        assert!(regions[1].holes().is_empty());
    }

    #[test]
    fn four_levels_alternate_outer_and_hole() {
        let regions = nest_ok(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            rect(5.0, 5.0, 30.0, 30.0),
            rect(10.0, 10.0, 20.0, 20.0),
            rect(15.0, 15.0, 10.0, 10.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), 1600.0, EPS_MM);
        assert_approx_eq!(area(&regions[0].holes()[0]), -900.0, EPS_MM);
        assert_approx_eq!(area(regions[1].outer()), 400.0, EPS_MM);
        assert_approx_eq!(area(&regions[1].holes()[0]), -100.0, EPS_MM);
    }

    #[test]
    fn circle_inside_arc_bulge_is_hole() {
        // The outer circle is two half-circle arcs whose chords coincide on
        // the X axis; a chord-only test would see no area at all. The inner
        // circle sits wholly in the upper bulge.
        let regions = nest_ok(vec![circle(0.0, 0.0, 10.0), circle(0.0, 6.0, 2.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
    }

    #[test]
    fn circle_outside_arc_bulge_is_separate() {
        // Inside the outer circle's bounding box but outside the curve.
        let regions = nest_ok(vec![circle(0.0, 0.0, 10.0), circle(9.0, 9.0, 1.0)]);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.holes().is_empty()));
    }

    #[test]
    fn several_holes_in_one_outer_keep_input_order() {
        let regions = nest_ok(vec![
            rect(0.0, 0.0, 100.0, 50.0),
            circle(20.0, 25.0, 5.0),
            circle(50.0, 25.0, 3.0),
            circle(80.0, 25.0, 4.0),
        ]);
        assert_eq!(regions.len(), 1);
        let holes = regions[0].holes();
        assert_eq!(holes.len(), 3);
        assert!(holes.iter().all(|h| !h.is_ccw()));
        assert_approx_eq!(area(&holes[0]), -PI * 25.0, 1e-9);
        assert_approx_eq!(area(&holes[1]), -PI * 9.0, 1e-9);
        assert_approx_eq!(area(&holes[2]), -PI * 16.0, 1e-9);
    }

    #[test]
    fn containment_does_not_depend_on_input_order() {
        let a = circle(0.0, 0.0, 10.0);
        let b = circle(0.0, 0.0, 7.0);
        let c = circle(0.0, 0.0, 3.0);
        let d = rect(30.0, 0.0, 5.0, 5.0);
        let orders = [
            vec![a.clone(), b.clone(), c.clone(), d.clone()],
            vec![c.clone(), b.clone(), a.clone(), d.clone()],
            vec![d.clone(), c.clone(), a.clone(), b.clone()],
            vec![b.clone(), d.clone(), c.clone(), a.clone()],
        ];
        for input in orders {
            // Areas rounded to whole mm², as strings so they compare exactly.
            let mut summary: Vec<String> = nest_ok(input)
                .iter()
                .map(|r| {
                    let holes: Vec<String> = r
                        .holes()
                        .iter()
                        .map(|h| format!("{:.0}", area(h)))
                        .collect();
                    format!("{:.0}[{}]", area(r.outer()), holes.join(","))
                })
                .collect();
            summary.sort();
            assert_eq!(summary, vec!["25[]", "28[]", "314[-154]"]);
        }
    }

    #[test]
    fn regions_follow_input_order_of_outers() {
        // The hole comes first in the input; region order follows the outers.
        let regions = nest_ok(vec![
            circle(50.0, 0.0, 2.0),
            rect(100.0, 0.0, 5.0, 5.0),
            circle(50.0, 0.0, 8.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), 25.0, EPS_MM);
        assert!(regions[0].holes().is_empty());
        assert_approx_eq!(area(regions[1].outer()), PI * 64.0, 1e-9);
        assert_eq!(regions[1].holes().len(), 1);
    }

    #[test]
    fn lone_contour_is_one_region_without_holes() {
        let regions = nest_ok(vec![circle(3.0, 4.0, 2.0)]);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].outer().is_ccw());
        assert!(regions[0].holes().is_empty());
    }

    #[test]
    fn hole_touching_outer_edge_is_still_a_hole() {
        // The inner rect shares part of the outer's left edge.
        let regions = nest_ok(vec![rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 2.0, 5.0, 2.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
    }

    #[test]
    fn no_contours_give_no_regions() {
        assert!(nest_ok(Vec::new()).is_empty());
    }

    #[test]
    fn clockwise_input_is_normalised() {
        let regions = nest_ok(vec![
            rect(0.0, 0.0, 10.0, 10.0).reversed(),
            rect(2.0, 2.0, 2.0, 2.0),
        ]);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].outer().is_ccw());
        assert!(!regions[0].holes()[0].is_ccw());
    }

    #[test]
    fn winding_counts_arc_bulge_not_chord() {
        let c = circle(0.0, 0.0, 5.0);
        assert_eq!(winding_number(&c, Point::new(0.0, 4.0)), 1);
        assert_eq!(winding_number(&c, Point::new(0.0, -4.0)), 1);
        assert_eq!(winding_number(&c, Point::new(0.0, 0.5)), 1);
        // On both half-circle chords at once (the diameter).
        assert_eq!(winding_number(&c, Point::new(0.0, 0.0)), 1);
        assert_eq!(winding_number(&c, Point::new(-2.0, 0.0)), 1);
        assert_eq!(winding_number(&c, Point::new(7.0, 0.0)), 0);
        assert_eq!(winding_number(&c, Point::new(4.0, 4.0)), 0);
        assert_eq!(winding_number(&c, Point::new(-20.0, 0.0)), 0);
        assert_eq!(winding_number(&c.reversed(), Point::new(0.0, 4.0)), -1);
    }

    #[test]
    fn winding_of_rect_is_one_inside_zero_outside() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(winding_number(&r, Point::new(5.0, 5.0)), 1);
        assert_eq!(winding_number(&r, Point::new(15.0, 5.0)), 0);
        assert_eq!(winding_number(&r, Point::new(5.0, -1.0)), 0);
    }

    #[test]
    fn several_parts_each_get_their_own_hole() {
        // Holes listed before and after their parts, interleaved.
        let regions = nest_ok(vec![
            rect(0.0, 0.0, 30.0, 30.0),
            circle(55.0, 15.0, 8.0),
            rect(40.0, 0.0, 30.0, 30.0),
            circle(15.0, 15.0, 8.0),
        ]);
        assert_eq!(regions.len(), 2);
        for (region, centre_x) in regions.iter().zip([15.0, 55.0]) {
            assert_eq!(region.holes().len(), 1);
            let b = region.holes()[0].polyline().bounds().unwrap();
            assert_approx_eq!((b.min.x + b.max.x) / 2.0, centre_x, 1e-9);
            let o = region.outer().polyline().bounds().unwrap();
            assert!(
                o.min.x < b.min.x && b.max.x < o.max.x,
                "hole lies in its own part"
            );
        }
    }

    #[test]
    fn crossing_circles_are_an_error() {
        let result = nest(vec![circle(0.0, 0.0, 10.0), circle(8.0, 0.0, 5.0)]);
        assert_eq!(
            result,
            Err(GeometryError::ContoursCross {
                first: 0,
                second: 1
            })
        );
    }

    #[test]
    fn crossing_rects_are_an_error_whatever_the_order() {
        // The small rect's first edge midpoint lies inside the large one,
        // which fooled the sample-point containment test.
        let big = rect(0.0, 0.0, 30.0, 20.0);
        let small = rect(10.0, 5.0, 30.0, 10.0);
        let separate = rect(100.0, 0.0, 5.0, 5.0);
        assert_eq!(
            nest(vec![separate.clone(), big.clone(), small.clone()]),
            Err(GeometryError::ContoursCross {
                first: 1,
                second: 2
            })
        );
        assert_eq!(
            nest(vec![small, separate, big]),
            Err(GeometryError::ContoursCross {
                first: 0,
                second: 2
            })
        );
    }

    #[test]
    fn shapes_touching_from_outside_do_not_cross() {
        let regions = nest_ok(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(10.0, 0.0, 10.0, 10.0),
        ]);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn identical_shapes_keep_the_first_and_warn() {
        let (regions, warnings) = nest(vec![
            rect(0.0, 0.0, 20.0, 10.0),
            circle(50.0, 0.0, 3.0),
            rect(0.0, 0.0, 20.0, 10.0),
        ])
        .unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(
            warnings,
            vec![GeometryWarning::DuplicateContourDropped {
                index: 2,
                duplicate_of: 0
            }]
        );
    }

    #[test]
    fn duplicate_drawn_the_other_way_is_still_a_duplicate() {
        let (regions, warnings) = nest(vec![
            circle(0.0, 0.0, 5.0),
            circle(0.0, 0.0, 5.0).reversed(),
        ])
        .unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn duplicate_hole_is_dropped_and_part_keeps_one_hole() {
        let (regions, warnings) = nest(vec![
            circle(0.0, 0.0, 10.0),
            circle(0.0, 0.0, 4.0),
            circle(0.0, 0.0, 4.0),
        ])
        .unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
        assert_eq!(
            warnings,
            vec![GeometryWarning::DuplicateContourDropped {
                index: 2,
                duplicate_of: 1
            }]
        );
    }
}
