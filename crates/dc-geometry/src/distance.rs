// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Shortest distances between points, segments, and polylines, following
//! arcs exactly rather than flattening them.

use crate::offset::to_cavalier;
use crate::point::{EPS_MM, Point, Vec2};
use crate::polyline::{Arc, Polyline, Segment};
use cavalier_contours::polyline::{PlineSegIntr, PlineSource, PlineVertex, pline_seg_intr};
use std::f64::consts::TAU;

/// Shortest distance between `a` and `b`, mm, following arcs; 0 when they
/// touch or cross (within [`EPS_MM`]). Each polyline counts as its line
/// only, not the area it encloses: a closed polyline wholly inside another
/// is as far from it as the gap between their lines. `f64::INFINITY` when
/// either has no segments.
#[must_use]
pub fn distance_mm(a: &Polyline, b: &Polyline) -> f64 {
    let (cav_a, cav_b) = (to_cavalier(a), to_cavalier(b));
    let mut best_mm = f64::INFINITY;
    for (s, (s1, s2)) in a.segments().zip(cav_a.iter_segments()) {
        for (t, (t1, t2)) in b.segments().zip(cav_b.iter_segments()) {
            best_mm = best_mm.min(segment_distance_mm(s, t, [s1, s2, t1, t2]));
            if best_mm <= 0.0 {
                return 0.0;
            }
        }
    }
    best_mm
}

/// Shortest distance between segments `s` and `t`, mm; `cav` is the same
/// two segments as cavalier_contours vertices, for the crossing test.
///
/// When they do not meet, the closest pair of points has an end point of
/// one segment, or else is interior to both, where the line between them is
/// normal to both: then a point on an arc lies along the other line's
/// normal, or on the line through both arcs' centres. Those candidates
/// cover every case.
fn segment_distance_mm(s: Segment, t: Segment, cav: [PlineVertex<f64>; 4]) -> f64 {
    let [s1, s2, t1, t2] = cav;
    let degenerate = |seg: Segment| seg.start.distance_to(seg.end) <= EPS_MM;
    if !degenerate(s)
        && !degenerate(t)
        && !matches!(
            pline_seg_intr(s1, s2, t1, t2, EPS_MM),
            PlineSegIntr::NoIntersect
        )
    {
        return 0.0;
    }
    let ends = [
        distance_to_segment(t, s.start),
        distance_to_segment(t, s.end),
        distance_to_segment(s, t.start),
        distance_to_segment(s, t.end),
    ];
    let interior = normal_candidates(s, t)
        .map(|p| distance_to_segment(t, p))
        .chain(normal_candidates(t, s).map(|p| distance_to_segment(s, p)));
    ends.into_iter()
        .chain(interior)
        .fold(f64::INFINITY, f64::min)
}

/// The points of `s`, if it is an arc, where its normal is also normal to
/// `t`: along `t`'s normal when `t` is a line, on the line through both
/// centres when `t` is an arc. Only points within `s`'s sweep.
fn normal_candidates(s: Segment, t: Segment) -> impl Iterator<Item = Point> {
    let arc = s.arc();
    let direction = arc.and_then(|arc| match t.arc() {
        Some(other) => (other.centre - arc.centre).normalized(),
        None => (t.end - t.start).perp().normalized(),
    });
    let points = match (arc, direction) {
        (Some(arc), Some(d)) => [d, -d].map(|d| on_arc(arc, d)),
        _ => [None, None],
    };
    points.into_iter().flatten()
}

/// The point of `arc` in direction `d` (unit) from its centre, if the sweep
/// covers that direction.
fn on_arc(arc: Arc, d: Vec2) -> Option<Point> {
    sweep_contains(arc, d.y.atan2(d.x)).then(|| arc.centre + d * arc.radius_mm)
}

/// Shortest distance from `p` to `seg`, mm, following arcs.
pub(crate) fn distance_to_segment(seg: Segment, p: Point) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::polyline::Vertex;

    const TOL: f64 = 1e-9;

    fn square(x: f64, y: f64, side_mm: f64) -> Polyline {
        Polyline::rect(Point::new(x, y), side_mm, side_mm)
    }

    #[test]
    fn squares_side_by_side_are_their_gap_apart() {
        assert_approx_eq!(
            distance_mm(&square(0.0, 0.0, 20.0), &square(22.0, 5.0, 20.0)),
            2.0,
            TOL
        );
    }

    #[test]
    fn diagonal_squares_measure_corner_to_corner() {
        let d = distance_mm(&square(0.0, 0.0, 10.0), &square(13.0, 14.0, 10.0));
        assert_approx_eq!(d, 5.0, TOL);
    }

    #[test]
    fn crossing_or_touching_is_zero() {
        assert_approx_eq!(
            distance_mm(&square(0.0, 0.0, 10.0), &square(5.0, 5.0, 10.0)),
            0.0,
            TOL
        );
        assert_approx_eq!(
            distance_mm(&square(0.0, 0.0, 10.0), &square(10.0, 0.0, 10.0)),
            0.0,
            TOL
        );
    }

    #[test]
    fn nested_shapes_are_the_gap_between_their_lines() {
        let d = distance_mm(&square(0.0, 0.0, 100.0), &square(10.0, 30.0, 20.0));
        assert_approx_eq!(d, 10.0, TOL);
    }

    #[test]
    fn circle_in_a_square_follows_the_arc() {
        // Radius 29 about (50, 50) in a square from 20 to 80: 1 mm from
        // each wall, at the middle of each arc, not at a vertex.
        let circle = Polyline::circle(Point::new(50.0, 50.0), 29.0);
        assert_approx_eq!(distance_mm(&circle, &square(20.0, 20.0, 60.0)), 1.0, TOL);
        assert_approx_eq!(distance_mm(&square(20.0, 20.0, 60.0), &circle), 1.0, TOL);
    }

    #[test]
    fn facing_arcs_measure_along_the_centre_line() {
        // Two circles of radius 10, centres 25 apart at 45°: 5 mm apart.
        let step = 25.0 / 2f64.sqrt();
        let a = Polyline::circle(Point::new(0.0, 0.0), 10.0);
        let b = Polyline::circle(Point::new(step, step), 10.0);
        assert_approx_eq!(distance_mm(&a, &b), 5.0, TOL);
    }

    #[test]
    fn concentric_arcs_are_the_radius_difference() {
        let a = Polyline::circle(Point::new(0.0, 0.0), 10.0);
        let b = Polyline::circle(Point::new(0.0, 0.0), 13.0);
        assert_approx_eq!(distance_mm(&a, &b), 3.0, TOL);
    }

    #[test]
    fn line_to_arc_bulge_uses_the_arc_not_the_chord() {
        // A half-disc bulging up to y = 5 below a line at y = 7.
        let half = Polyline::new(
            vec![Vertex::new(5.0, 0.0, 1.0), Vertex::line(-5.0, 0.0)],
            true,
        );
        let line = Polyline::new(
            vec![Vertex::line(-20.0, 7.0), Vertex::line(20.0, 7.0)],
            false,
        );
        assert_approx_eq!(distance_mm(&half, &line), 2.0, TOL);
    }

    #[test]
    fn empty_polyline_is_infinitely_far() {
        let empty = Polyline::new(Vec::new(), true);
        assert!(distance_mm(&empty, &square(0.0, 0.0, 1.0)).is_infinite());
    }
}
