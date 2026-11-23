// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/helpers/segmentIntersection.ts
//   src/store/helpers/segmentIntersection.test.ts

//! Where two segments cross, and how far along each
//! (`docs/specs/phase-5/design.md`, "Algorithms → Intersections and
//! Trim"): line with line, line with arc, arc with arc. Snapping to an
//! intersection and trimming at one both read it.
//!
//! Changed from upstream: a segment is a [`Segment`], an arc by its bulge
//! (positive counter-clockwise in Y-up), where upstream resolves each to
//! a centre, a radius and two angles first; `along` is a fraction of the
//! segment's length, which for an arc is upstream's fraction of its
//! sweep. The slack at a segment's ends is a length, [`EPS_MM`], where
//! upstream's is `1e-9` of whatever unit the test is in. Upstream's
//! `rayA` option, a first segment extended past its end, serves its
//! Extend tool, which is not in phase 5, and is not ported.

use crate::point::{EPS_MM, Point};
use crate::polyline::{Arc, Segment};
use std::f64::consts::TAU;

/// One point two segments share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// The point, millimetres.
    pub at: Point,
    /// How far along the first segment it is, 0 at its start to 1 at its
    /// end, by length.
    pub along_a: f64,
    /// How far along the second segment, the same way.
    pub along_b: f64,
}

/// Every point `a` and `b` share, zero to two of them
/// (`segmentIntersections`): where they cross, where one ends on the
/// other, and where they touch as tangents. Segments that run along each
/// other (parallel lines, arcs of one circle) share no single point and
/// give none. A point within [`EPS_MM`] past an end counts as the end.
#[must_use]
pub fn crossings(a: Segment, b: Segment) -> Vec<Crossing> {
    match (a.arc(), b.arc()) {
        (None, None) => line_line(a, b).into_iter().collect(),
        (None, Some(arc)) => line_arc(a, arc)
            .into_iter()
            .map(|(at, along_a, along_b)| Crossing {
                at,
                along_a,
                along_b,
            })
            .collect(),
        (Some(arc), None) => line_arc(b, arc)
            .into_iter()
            .map(|(at, along_b, along_a)| Crossing {
                at,
                along_a,
                along_b,
            })
            .collect(),
        (Some(first), Some(second)) => arc_arc(first, second),
    }
}

/// Whether `value` is a finite number above zero.
fn is_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// How far past an end, as a fraction of a line `length_mm` long, still
/// counts as on it.
fn end_slack(length_mm: f64) -> f64 {
    EPS_MM / length_mm
}

/// `t` held to the line when it is on it, slack included; `None` off it.
fn on_line(t: f64, slack: f64) -> Option<f64> {
    (t >= -slack && t <= 1.0 + slack).then(|| t.clamp(0.0, 1.0))
}

/// Where two lines cross (`lineLineParams`).
fn line_line(a: Segment, b: Segment) -> Option<Crossing> {
    let (d1, d2) = (a.end - a.start, b.end - b.start);
    let (len1, len2) = (d1.length(), d2.length());
    if len1 <= EPS_MM || len2 <= EPS_MM {
        return None;
    }
    let det = d1.cross(d2);
    // Parallel, or as near as the two lengths can tell.
    if det.abs() <= 1e-12 * len1 * len2 {
        return None;
    }
    let between = b.start - a.start;
    let along_a = on_line(between.cross(d2) / det, end_slack(len1))?;
    let along_b = on_line(between.cross(d1) / det, end_slack(len2))?;
    Some(Crossing {
        at: a.start + d1 * along_a,
        along_a,
        along_b,
    })
}

/// Where a line meets an arc: the point, how far along the line and how
/// far along the arc (`lineArcCandidates`).
fn line_arc(line: Segment, arc: Arc) -> Vec<(Point, f64, f64)> {
    let d = line.end - line.start;
    let length = d.length();
    if length <= EPS_MM || !is_positive(arc.radius_mm) {
        return Vec::new();
    }
    let v = line.start - arc.centre;
    let a = d.dot(d);
    let b = 2.0 * d.dot(v);
    let c = v.dot(v) - arc.radius_mm * arc.radius_mm;
    // The line's distance from the centre, against the radius: within
    // `EPS_MM` of touching is a tangent, one point.
    let offset_mm = (d.cross(v) / length).abs();
    if offset_mm > arc.radius_mm + EPS_MM {
        return Vec::new();
    }
    let tangent = (offset_mm - arc.radius_mm).abs() <= EPS_MM;
    let root = (b * b - 4.0 * a * c).max(0.0).sqrt();
    let roots: &[f64] = if tangent { &[0.0] } else { &[-root, root] };
    roots
        .iter()
        .filter_map(|root| {
            let t = on_line((-b + root) / (2.0 * a), end_slack(length))?;
            let at = line.start + d * t;
            let along_arc = on_sweep(arc, at)?;
            Some((at, t, along_arc))
        })
        .collect()
}

/// Where two arcs meet (`arcArcCandidates`).
fn arc_arc(first: Arc, second: Arc) -> Vec<Crossing> {
    let between = second.centre - first.centre;
    let d = between.length();
    let (r1, r2) = (first.radius_mm, second.radius_mm);
    // The same centre: concentric, or one circle, with no single point.
    if d <= EPS_MM || !is_positive(r1) || !is_positive(r2) {
        return Vec::new();
    }
    if d > r1 + r2 + EPS_MM || d < (r1 - r2).abs() - EPS_MM {
        return Vec::new();
    }
    // From the first centre toward the second, to the chord the two
    // circles share; then half that chord either side.
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let half_chord_sq = r1 * r1 - along * along;
    let tangent = (d - (r1 + r2)).abs() <= EPS_MM || (d - (r1 - r2).abs()).abs() <= EPS_MM;
    let half_chord = if tangent {
        0.0
    } else {
        half_chord_sq.max(0.0).sqrt()
    };
    let foot = first.centre + between * (along / d);
    let across = between.perp() * (half_chord / d);
    let candidates: &[f64] = if tangent { &[1.0] } else { &[-1.0, 1.0] };
    candidates
        .iter()
        .filter_map(|sign| {
            let at = foot + across * *sign;
            Some(Crossing {
                at,
                along_a: on_sweep(first, at)?,
                along_b: on_sweep(second, at)?,
            })
        })
        .collect()
}

/// How far round `arc` the point `at` of its circle is, 0 to 1; `None`
/// when the arc does not reach it (`angleInSweep`).
fn on_sweep(arc: Arc, at: Point) -> Option<f64> {
    let direction = at - arc.centre;
    let angle = direction.y.atan2(direction.x);
    let sweep = arc.sweep_rad.abs();
    if !is_positive(sweep) {
        return None;
    }
    let mut from_start = if arc.sweep_rad > 0.0 {
        (angle - arc.start_angle_rad).rem_euclid(TAU)
    } else {
        (arc.start_angle_rad - angle).rem_euclid(TAU)
    };
    let slack = EPS_MM / arc.radius_mm;
    // A point at the start can come out a hair short of a full turn.
    if TAU - from_start <= slack && sweep < TAU - slack {
        from_start = 0.0;
    }
    (from_start <= sweep + slack).then(|| (from_start / sweep).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::distance::point_segment_distance_mm;
    use std::f64::consts::PI;

    /// Upstream's tolerance for a point being on a primitive.
    const ON: f64 = 1e-6;

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn line(from: Point, to: Point) -> Segment {
        Segment {
            start: from,
            end: to,
            bulge: 0.0,
        }
    }

    /// The arc of the circle about `centre` from angle `a0` to `a1`,
    /// counter-clockwise, as upstream's `{ center, radius, a0, a1, ccw:
    /// true }`. A bulge cannot hold a whole turn, so a test that uses a
    /// full circle upstream takes its two halves here ([`circle`]).
    fn arc(centre: Point, radius: f64, a0: f64, a1: f64) -> Segment {
        let at = |angle: f64| {
            pt(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
            )
        };
        Segment {
            start: at(a0),
            end: at(a1),
            bulge: ((a1 - a0) / 4.0).tan(),
        }
    }

    /// A whole circle as its lower and upper halves, from −π round to π.
    fn circle(centre: Point, radius: f64) -> [Segment; 2] {
        [arc(centre, radius, -PI, 0.0), arc(centre, radius, 0.0, PI)]
    }

    /// Every crossing of `a` with each of `others`, as upstream's one
    /// full-circle arc would give them.
    fn with_all(a: Segment, others: &[Segment]) -> Vec<Crossing> {
        let mut hits: Vec<Crossing> = Vec::new();
        for other in others {
            for hit in crossings(a, *other) {
                // The two halves share their ends: one point, not two.
                if !hits.iter().any(|h| h.at.distance_to(hit.at) < ON) {
                    hits.push(hit);
                }
            }
        }
        hits
    }

    #[track_caller]
    fn assert_on(segment: Segment, p: Point) {
        let gap = point_segment_distance_mm(segment, p);
        assert!(gap < ON, "{p:?} is {gap} mm off {segment:?}");
    }

    /// Each crossing is on both segments, and `along` says where: the
    /// point at that fraction of each segment's length is the crossing.
    #[track_caller]
    fn assert_consistent(a: Segment, b: Segment, hits: &[Crossing]) {
        for hit in hits {
            assert_on(a, hit.at);
            assert_on(b, hit.at);
            for (segment, along) in [(a, hit.along_a), (b, hit.along_b)] {
                assert!((0.0..=1.0).contains(&along), "{along}");
                let (at, _) = segment.point_at(segment.length_mm() * along).unwrap();
                assert!(at.distance_to(hit.at) < ON, "{at:?} is not {:?}", hit.at);
            }
        }
    }

    // --- line × line ---

    /// `testLineLineCrossing`.
    #[test]
    fn line_line_crossing() {
        let (a, b) = (
            line(pt(0.0, 0.0), pt(10.0, 10.0)),
            line(pt(0.0, 10.0), pt(10.0, 0.0)),
        );
        let hits = crossings(a, b);
        assert_eq!(hits.len(), 1);
        assert_approx_eq!(hits[0].at.x, 5.0, ON);
        assert_approx_eq!(hits[0].at.y, 5.0, ON);
        assert_approx_eq!(hits[0].along_a, 0.5, 1e-12);
        assert_approx_eq!(hits[0].along_b, 0.5, 1e-12);
        assert_consistent(a, b, &hits);
    }

    /// `testLineLineParallel`, `testLineLineNoIntersection`: parallel
    /// lines, and lines end to end with a gap, share nothing.
    #[test]
    fn line_line_parallel_or_apart() {
        let a = line(pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!(crossings(a, line(pt(0.0, 5.0), pt(10.0, 5.0))), vec![]);
        let short = line(pt(0.0, 0.0), pt(3.0, 0.0));
        assert_eq!(crossings(short, line(pt(5.0, 0.0), pt(8.0, 0.0))), vec![]);
        // One on top of the other: no single point.
        assert_eq!(crossings(a, line(pt(2.0, 0.0), pt(7.0, 0.0))), vec![]);
    }

    /// `testLineLineEndpointTouch`, `testLineLineTJunction`.
    #[test]
    fn line_line_end_on_and_t_junction() {
        let (a, b) = (
            line(pt(0.0, 0.0), pt(5.0, 0.0)),
            line(pt(5.0, 0.0), pt(5.0, 5.0)),
        );
        let hits = crossings(a, b);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.distance_to(pt(5.0, 0.0)) < ON);
        assert_approx_eq!(hits[0].along_a, 1.0, 1e-12);
        assert_approx_eq!(hits[0].along_b, 0.0, 1e-12);
        let (a, b) = (
            line(pt(0.0, 5.0), pt(10.0, 5.0)),
            line(pt(4.0, 0.0), pt(4.0, 10.0)),
        );
        let hits = crossings(a, b);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.distance_to(pt(4.0, 5.0)) < ON);
        assert_consistent(a, b, &hits);
        // A line that stops a micrometre short of another misses it, and
        // one that stops a nanometre short ends on it.
        let stem = |gap: f64| line(pt(4.0, 0.0), pt(4.0, 5.0 - gap));
        assert_eq!(crossings(a, stem(1e-3)), vec![]);
        assert_eq!(crossings(a, stem(1e-9)).len(), 1);
    }

    // --- line × arc ---

    /// `testLineArcTwoHits`, `testArcLineTwoHits`: a line through a
    /// circle crosses it twice, whichever is given first.
    #[test]
    fn line_arc_two_hits() {
        let through = line(pt(-10.0, 0.0), pt(10.0, 0.0));
        let whole = circle(pt(0.0, 0.0), 5.0);
        let mut xs: Vec<f64> = with_all(through, &whole).iter().map(|h| h.at.x).collect();
        xs.sort_by(f64::total_cmp);
        assert_eq!(xs.len(), 2);
        assert_approx_eq!(xs[0], -5.0, ON);
        assert_approx_eq!(xs[1], 5.0, ON);
        // A chord across one half: two crossings of one arc, each way.
        let chord = line(pt(-10.0, 3.0), pt(10.0, 3.0));
        let upper = whole[1];
        let hits = crossings(chord, upper);
        assert_eq!(hits.len(), 2);
        assert_consistent(chord, upper, &hits);
        let swapped = crossings(upper, chord);
        assert_eq!(swapped.len(), 2);
        assert_consistent(upper, chord, &swapped);
        for hit in &swapped {
            assert!(hits.iter().any(|h| h.at.distance_to(hit.at) < ON
                && (h.along_a - hit.along_b).abs() < 1e-9
                && (h.along_b - hit.along_a).abs() < 1e-9));
        }
    }

    /// `testLineArcTangent`: a line that touches the arc meets it once.
    #[test]
    fn line_arc_tangent() {
        let touching = line(pt(5.0, -10.0), pt(5.0, 10.0));
        let right_half = arc(pt(0.0, 0.0), 5.0, -PI / 2.0, PI / 2.0);
        let hits = crossings(touching, right_half);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.distance_to(pt(5.0, 0.0)) < ON);
        assert_approx_eq!(hits[0].along_a, 0.5, 1e-9);
        assert_approx_eq!(hits[0].along_b, 0.5, 1e-9);
    }

    /// `testLineArcZeroHits`, `testLineArcHitOutsideSweep`: a line clear
    /// of the circle misses; a line through the circle meets only the
    /// part the arc covers.
    #[test]
    fn line_arc_misses_and_sweep() {
        let outside = line(pt(10.0, 0.0), pt(20.0, 0.0));
        assert_eq!(crossings(outside, arc(pt(0.0, 0.0), 5.0, 0.0, PI)), vec![]);
        let vertical = line(pt(0.0, -10.0), pt(0.0, 10.0));
        // From the bottom of the circle counter-clockwise to its right.
        let quarter = arc(pt(0.0, 0.0), 5.0, -PI / 2.0, 0.0);
        let hits = crossings(vertical, quarter);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.y < 0.0, "the bottom of the circle");
        assert_approx_eq!(hits[0].along_b, 0.0, 1e-9);
        assert_consistent(vertical, quarter, &hits);
    }

    // --- arc × arc ---

    /// `testArcArcTwoHits`: circles of radius 5 about (0,0) and (8,0)
    /// meet at (4, ±3).
    #[test]
    fn arc_arc_two_hits() {
        let (first, second) = (circle(pt(0.0, 0.0), 5.0), circle(pt(8.0, 0.0), 5.0));
        let mut hits: Vec<Crossing> = Vec::new();
        for a in first {
            hits.extend(with_all(a, &second));
        }
        assert_eq!(hits.len(), 2);
        let mut ys: Vec<f64> = hits.iter().map(|h| h.at.y).collect();
        ys.sort_by(f64::total_cmp);
        assert_approx_eq!(ys[0], -3.0, ON);
        assert_approx_eq!(ys[1], 3.0, ON);
        for hit in &hits {
            assert_approx_eq!(hit.at.x, 4.0, ON);
        }
        // The right half of the first and the left half of the second
        // hold both points.
        let right = arc(pt(0.0, 0.0), 5.0, -PI / 2.0, PI / 2.0);
        let left = arc(pt(8.0, 0.0), 5.0, PI / 2.0, 3.0 * PI / 2.0);
        let both = crossings(right, left);
        assert_eq!(both.len(), 2, "testArcArcSweepFilters");
        assert_consistent(right, left, &both);
    }

    /// `testArcArcTangent`: circles that touch meet once.
    #[test]
    fn arc_arc_tangent() {
        let right = arc(pt(0.0, 0.0), 5.0, -PI / 2.0, PI / 2.0);
        let left = arc(pt(10.0, 0.0), 5.0, PI / 2.0, 3.0 * PI / 2.0);
        let hits = crossings(right, left);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.distance_to(pt(5.0, 0.0)) < ON);
        // One inside the other, touching.
        let inner = arc(pt(2.0, 0.0), 3.0, -PI / 2.0, PI / 2.0);
        let hits = crossings(right, inner);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].at.distance_to(pt(5.0, 0.0)) < ON);
    }

    /// `testArcArcDisjoint`, `testArcArcConcentric`.
    #[test]
    fn arc_arc_apart_or_concentric() {
        let a = arc(pt(0.0, 0.0), 3.0, 0.0, PI);
        assert_eq!(crossings(a, arc(pt(10.0, 0.0), 3.0, 0.0, PI)), vec![]);
        assert_eq!(crossings(arc(pt(0.0, 0.0), 5.0, 0.0, PI), a), vec![]);
        // The same circle: no single point.
        assert_eq!(crossings(a, arc(pt(0.0, 0.0), 3.0, PI / 2.0, PI)), vec![]);
        // One wholly inside the other.
        assert_eq!(
            crossings(
                arc(pt(0.0, 0.0), 9.0, 0.0, PI),
                arc(pt(1.0, 0.0), 2.0, 0.0, PI)
            ),
            vec![]
        );
    }

    /// A clockwise arc is the same curve run the other way: the same
    /// points, each as far from its other end.
    #[test]
    fn a_clockwise_arc_counts_along_from_its_own_start() {
        let chord = line(pt(-10.0, 3.0), pt(10.0, 3.0));
        let upper = arc(pt(0.0, 0.0), 5.0, 0.0, PI);
        let reversed = Segment {
            start: upper.end,
            end: upper.start,
            bulge: -upper.bulge,
        };
        let (forward, backward) = (crossings(chord, upper), crossings(chord, reversed));
        assert_eq!(backward.len(), 2);
        assert_consistent(chord, reversed, &backward);
        for hit in &forward {
            assert!(
                backward.iter().any(|b| b.at.distance_to(hit.at) < ON
                    && (b.along_b - (1.0 - hit.along_b)).abs() < 1e-9)
            );
        }
        // An arc past a half turn: a bulge above one.
        let most = arc(pt(0.0, 0.0), 5.0, -3.0 * PI / 4.0, 3.0 * PI / 4.0);
        assert!(most.bulge > 1.0);
        let vertical = line(pt(0.0, -10.0), pt(0.0, 10.0));
        let hits = crossings(vertical, most);
        assert_eq!(hits.len(), 2);
        assert_consistent(vertical, most, &hits);
    }

    /// `testDegenerateZeroLengthLine`, `testDegenerateZeroRadiusArc`: a
    /// segment with no length meets nothing, and nothing panics.
    #[test]
    fn degenerate_segments_meet_nothing() {
        let point = line(pt(1.0, 1.0), pt(1.0, 1.0));
        let diagonal = line(pt(0.0, 0.0), pt(2.0, 2.0));
        assert_eq!(crossings(point, diagonal), vec![]);
        assert_eq!(crossings(diagonal, point), vec![]);
        let dot = Segment {
            start: pt(5.0, 0.0),
            end: pt(5.0, 0.0),
            bulge: 1.0,
        };
        assert_eq!(crossings(line(pt(0.0, 0.0), pt(10.0, 0.0)), dot), vec![]);
        let nan = line(pt(f64::NAN, 0.0), pt(1.0, 0.0));
        assert_eq!(crossings(nan, diagonal), vec![]);
    }
}
