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
            best_mm = best_mm.min(segments_apart_mm(s, t, [s1, s2, t1, t2]));
            if best_mm <= 0.0 {
                return 0.0;
            }
        }
    }
    best_mm
}

/// Shortest distance between segments `s` and `t`, mm, following arcs; 0
/// when they touch or cross (within [`EPS_MM`]). A zero-length segment is
/// its point.
#[must_use]
pub fn segment_distance_mm(s: Segment, t: Segment) -> f64 {
    let vertex = |p: Point, bulge: f64| PlineVertex::new(p.x, p.y, bulge);
    segments_apart_mm(
        s,
        t,
        [
            vertex(s.start, s.bulge),
            vertex(s.end, 0.0),
            vertex(t.start, t.bulge),
            vertex(t.end, 0.0),
        ],
    )
}

/// Shortest distance from `p` to `seg`, mm, following arcs.
#[must_use]
pub fn point_segment_distance_mm(seg: Segment, p: Point) -> f64 {
    distance_to_segment(seg, p)
}

/// The closest pair of points of segments `s` and `t`, the first on `s`,
/// following arcs; where they touch or cross, a point where they meet,
/// twice. The candidates are those of [`segment_distance_mm`].
#[must_use]
pub fn closest_points(s: Segment, t: Segment) -> (Point, Point) {
    let vertex = |p: Point, bulge: f64| PlineVertex::new(p.x, p.y, bulge);
    let degenerate = |seg: Segment| seg.start.distance_to(seg.end) <= EPS_MM;
    if !degenerate(s) && !degenerate(t) {
        let crossing = match pline_seg_intr(
            vertex(s.start, s.bulge),
            vertex(s.end, 0.0),
            vertex(t.start, t.bulge),
            vertex(t.end, 0.0),
            EPS_MM,
        ) {
            PlineSegIntr::NoIntersect => None,
            PlineSegIntr::TangentIntersect { point } | PlineSegIntr::OneIntersect { point } => {
                Some(point)
            }
            PlineSegIntr::TwoIntersects { point1, .. }
            | PlineSegIntr::OverlappingLines { point1, .. }
            | PlineSegIntr::OverlappingArcs { point1, .. } => Some(point1),
        };
        if let Some(point) = crossing {
            let p = Point::new(point.x, point.y);
            return (p, p);
        }
    }
    let from_s = [s.start, s.end]
        .into_iter()
        .chain(normal_candidates(s, t))
        .map(|p| (p, closest_on_segment(t, p)));
    let from_t = [t.start, t.end]
        .into_iter()
        .chain(normal_candidates(t, s))
        .map(|q| (closest_on_segment(s, q), q));
    from_s
        .chain(from_t)
        .min_by(|(a, b), (c, d)| a.distance_to(*b).total_cmp(&c.distance_to(*d)))
        .unwrap_or((s.start, t.start))
}

/// The points where `a` and `b` meet, mm: where they cross or touch, and
/// where one comes within `tolerance_mm` of the other (an end that stops
/// that short of the other line, or a near tangent). Where two segments
/// overlap, the ends of the overlap. In segment order of `a`, then of
/// `b`; a meeting may be listed more than once.
#[must_use]
pub fn meeting_points(a: &Polyline, b: &Polyline, tolerance_mm: f64) -> Vec<Point> {
    let mut points = Vec::new();
    for s in a.segments() {
        for t in b.segments() {
            segment_meetings(s, t, tolerance_mm, &mut points);
        }
    }
    points
}

/// [`meeting_points`] for one segment of each line, added to `points`.
pub(crate) fn segment_meetings(s: Segment, t: Segment, tolerance_mm: f64, points: &mut Vec<Point>) {
    let vertex = |p: Point, bulge: f64| PlineVertex::new(p.x, p.y, bulge);
    let degenerate = |seg: Segment| seg.start.distance_to(seg.end) <= EPS_MM;
    if !degenerate(s) && !degenerate(t) {
        match pline_seg_intr(
            vertex(s.start, s.bulge),
            vertex(s.end, 0.0),
            vertex(t.start, t.bulge),
            vertex(t.end, 0.0),
            EPS_MM,
        ) {
            PlineSegIntr::NoIntersect => {}
            PlineSegIntr::TangentIntersect { point } | PlineSegIntr::OneIntersect { point } => {
                points.push(Point::new(point.x, point.y));
            }
            PlineSegIntr::TwoIntersects { point1, point2 }
            | PlineSegIntr::OverlappingLines { point1, point2 }
            | PlineSegIntr::OverlappingArcs { point1, point2 } => {
                points.push(Point::new(point1.x, point1.y));
                points.push(Point::new(point2.x, point2.y));
            }
        }
    }
    // Within the tolerance without touching: the closest pair has an end
    // of one segment, or a normal point of an arc.
    let near_t = [s.start, s.end].into_iter().chain(normal_candidates(s, t));
    let near_s = [t.start, t.end].into_iter().chain(normal_candidates(t, s));
    points.extend(near_t.filter(|&p| distance_to_segment(t, p) <= tolerance_mm));
    points.extend(near_s.filter(|&p| distance_to_segment(s, p) <= tolerance_mm));
}

/// Shortest distance between `a` and `b`, mm, as [`distance_mm`], over
/// the pairs of points of which at least one lies outside every disc of
/// `radius_mm` about `around`: what is left once the stretches of both
/// near those points are set aside. With nothing `around`, the plain
/// distance. `f64::INFINITY` when every pair is set aside.
#[must_use]
pub fn distance_away_from_mm(a: &Polyline, b: &Polyline, around: &[Point], radius_mm: f64) -> f64 {
    let pieces = |p: &Polyline| -> Vec<Segment> {
        p.segments()
            .flat_map(|s| s.outside_discs(around, radius_mm))
            .collect()
    };
    let (far_a, far_b) = (pieces(a), pieces(b));
    let mut best_mm = f64::INFINITY;
    for (far, whole) in [(&far_a, b), (&far_b, a)] {
        for &s in far {
            for t in whole.segments() {
                best_mm = best_mm.min(segment_distance_mm(s, t));
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
fn segments_apart_mm(s: Segment, t: Segment, cav: [PlineVertex<f64>; 4]) -> f64 {
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

/// The point of `seg` closest to `p`, following arcs.
fn closest_on_segment(seg: Segment, p: Point) -> Point {
    let nearer_end = || {
        if p.distance_to(seg.start) <= p.distance_to(seg.end) {
            seg.start
        } else {
            seg.end
        }
    };
    match seg.arc() {
        Some(arc) => {
            let v = p - arc.centre;
            match v.normalized() {
                Some(d) if sweep_contains(arc, v.y.atan2(v.x)) => arc.centre + d * arc.radius_mm,
                _ => nearer_end(),
            }
        }
        None => {
            let d = seg.end - seg.start;
            let len2 = d.dot(d);
            if len2 <= EPS_MM * EPS_MM {
                return nearer_end();
            }
            let t = ((p - seg.start).dot(d) / len2).clamp(0.0, 1.0);
            seg.start + d * t
        }
    }
}

/// The point of a polyline nearest a given point ([`nearest_point`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    /// The nearest point of the polyline.
    pub point: Point,
    /// How far along the polyline from its first vertex `point` is, mm,
    /// counting only segments longer than [`EPS_MM`], as
    /// [`Polyline::point_at`] walks it.
    pub along_mm: f64,
    /// The distance from the given point to `point`, mm.
    pub gap_mm: f64,
    /// The index of the segment `point` is on, in [`Polyline::segments`]
    /// order.
    pub segment: usize,
    /// How far along that segment `point` is, mm.
    pub along_segment_mm: f64,
}

/// The point of `path` nearest `p`, following arcs, with how far along
/// `path` it is (#105). Where several points are equally near, the first
/// along the path wins, except on an arc centred on `p`, every point of
/// which is equally near: there the arc's middle is taken, so a point at
/// a corner maps to the middle of the arc an offset path turns round it.
/// `None` when `path` has no segment longer than [`EPS_MM`].
#[must_use]
pub fn nearest_point(path: &Polyline, p: Point) -> Option<Nearest> {
    let mut best: Option<Nearest> = None;
    // Whether `best` is the middle of an arc centred on `p`, which the
    // ends of the segments beside it tie with.
    let mut best_is_centred = false;
    let mut walked_mm = 0.0;
    for (segment, seg) in path.segments().enumerate() {
        let length_mm = seg.length_mm();
        if length_mm <= EPS_MM {
            continue;
        }
        let centred = seg
            .arc()
            .is_some_and(|arc| arc.centre.distance_to(p) <= EPS_MM);
        let (point, along_segment_mm) = if centred {
            let half_mm = length_mm / 2.0;
            let point = seg.point_at(half_mm).map_or(seg.start, |(q, _)| q);
            (point, half_mm)
        } else {
            let point = closest_on_segment(seg, p);
            (point, along_segment(seg, point))
        };
        let gap_mm = p.distance_to(point);
        let better = best.is_none_or(|b| {
            if centred {
                gap_mm <= b.gap_mm + EPS_MM
            } else if best_is_centred {
                gap_mm < b.gap_mm - EPS_MM
            } else {
                gap_mm < b.gap_mm
            }
        });
        if better {
            best_is_centred = centred;
            best = Some(Nearest {
                point,
                along_mm: walked_mm + along_segment_mm,
                gap_mm,
                segment,
                along_segment_mm,
            });
        }
        walked_mm += length_mm;
    }
    best
}

/// How far along `seg` its point `q` is, mm: along the chord for a line,
/// round the sweep for an arc.
fn along_segment(seg: Segment, q: Point) -> f64 {
    match seg.arc() {
        Some(arc) => {
            let v = q - arc.centre;
            let angle_rad = v.y.atan2(v.x);
            let swept_rad = if arc.sweep_rad > 0.0 {
                (angle_rad - arc.start_angle_rad).rem_euclid(TAU)
            } else {
                (arc.start_angle_rad - angle_rad).rem_euclid(TAU)
            };
            // A point at the start can come out a hair short of a full turn.
            let swept_rad = if swept_rad > arc.sweep_rad.abs() + EPS_MM / arc.radius_mm {
                0.0
            } else {
                swept_rad.min(arc.sweep_rad.abs())
            };
            swept_rad * arc.radius_mm
        }
        None => seg.start.distance_to(q),
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

    /// The single-segment forms agree with `distance_mm`: a half circle of
    /// radius 5 about the origin bulging up to y = 5, a line at y = 7, a
    /// point at (0, 8), and a zero-length segment (a point) at (0, 9).
    #[test]
    fn single_segments_follow_arcs() {
        let half = Segment {
            start: Point::new(5.0, 0.0),
            end: Point::new(-5.0, 0.0),
            bulge: 1.0,
        };
        let line = Segment {
            start: Point::new(-20.0, 7.0),
            end: Point::new(20.0, 7.0),
            bulge: 0.0,
        };
        let dot = Segment {
            start: Point::new(0.0, 9.0),
            end: Point::new(0.0, 9.0),
            bulge: 0.0,
        };
        assert_approx_eq!(segment_distance_mm(half, line), 2.0, TOL);
        assert_approx_eq!(segment_distance_mm(line, half), 2.0, TOL);
        assert_approx_eq!(segment_distance_mm(dot, half), 4.0, TOL);
        assert_approx_eq!(segment_distance_mm(half, dot), 4.0, TOL);
        assert_approx_eq!(
            point_segment_distance_mm(half, Point::new(0.0, 8.0)),
            3.0,
            TOL
        );
        assert_approx_eq!(segment_distance_mm(line, line), 0.0, TOL);
    }

    fn open_line(from: (f64, f64), to: (f64, f64)) -> Polyline {
        Polyline::new(
            vec![Vertex::line(from.0, from.1), Vertex::line(to.0, to.1)],
            false,
        )
    }

    fn all_near(points: &[Point], at: Point) -> bool {
        !points.is_empty() && points.iter().all(|p| p.distance_to(at) <= 1e-6)
    }

    /// A slot from a square's edge meets it at its end; two crossing lines
    /// at the crossing; an end that stops within the tolerance of the
    /// other line meets it there; lines farther apart do not meet.
    #[test]
    fn meeting_points_are_where_lines_touch_cross_or_nearly_touch() {
        let square = square(0.0, 0.0, 20.0);
        let slot = open_line((10.0, 0.0), (10.0, 8.0));
        assert!(all_near(
            &meeting_points(&slot, &square, 0.01),
            Point::new(10.0, 0.0)
        ));
        let across = open_line((0.0, 5.0), (8.0, 5.0));
        let up = open_line((4.0, 1.0), (4.0, 9.0));
        assert!(all_near(
            &meeting_points(&across, &up, 0.01),
            Point::new(4.0, 5.0)
        ));
        let short = open_line((10.0, 0.005), (10.0, 8.0));
        assert!(all_near(
            &meeting_points(&short, &square, 0.01),
            Point::new(10.0, 0.005)
        ));
        let apart = open_line((10.0, 1.0), (10.0, 8.0));
        assert!(meeting_points(&apart, &square, 0.01).is_empty());
        // A circle a hair from the square's top edge meets it at its top.
        let circle = Polyline::circle(Point::new(10.0, 15.0), 4.995);
        assert!(all_near(
            &meeting_points(&circle, &square, 0.01),
            Point::new(10.0, 19.995)
        ));
    }

    /// A slot square to an edge, set aside within 3 mm of where it meets
    /// the edge, is 3 mm from it at the disc; a slot 1 mm beside another
    /// edge is still 1 mm from it beyond the disc; with nothing set aside,
    /// the plain distance.
    /// The closest points agree with `segment_distance_mm`: a line 2 mm
    /// above a half circle's top meets it above the centre; a line's end
    /// 3 mm beside another line is closest to its foot; crossing lines
    /// give the crossing twice.
    #[test]
    fn closest_points_follow_arcs_and_ends() {
        let half = Segment {
            start: Point::new(5.0, 0.0),
            end: Point::new(-5.0, 0.0),
            bulge: 1.0,
        };
        let line = Segment {
            start: Point::new(-20.0, 7.0),
            end: Point::new(20.0, 7.0),
            bulge: 0.0,
        };
        let (p, q) = closest_points(half, line);
        assert!(p.distance_to(Point::new(0.0, 5.0)) <= TOL, "{p:?}");
        assert!(q.distance_to(Point::new(0.0, 7.0)) <= TOL, "{q:?}");
        let stub = Segment {
            start: Point::new(3.0, 10.0),
            end: Point::new(3.0, 20.0),
            bulge: 0.0,
        };
        let wall = Segment {
            start: Point::new(0.0, 0.0),
            end: Point::new(0.0, 30.0),
            bulge: 0.0,
        };
        let (p, q) = closest_points(stub, wall);
        assert_approx_eq!(p.distance_to(q), 3.0, TOL);
        assert_approx_eq!(segment_distance_mm(stub, wall), 3.0, TOL);
        let across = Segment {
            start: Point::new(-5.0, 15.0),
            end: Point::new(5.0, 15.0),
            bulge: 0.0,
        };
        let (p, q) = closest_points(across, wall);
        assert!(
            p.distance_to(Point::new(0.0, 15.0)) <= 1e-6 && p == q,
            "{p:?} {q:?}"
        );
    }

    #[test]
    fn distance_away_from_meetings_sets_the_stretch_near_them_aside() {
        let square = square(0.0, 0.0, 20.0);
        let slot = open_line((10.0, 0.0), (10.0, 8.0));
        let at = [Point::new(10.0, 0.0)];
        assert_approx_eq!(distance_away_from_mm(&slot, &square, &at, 3.0), 3.0, TOL);
        assert_approx_eq!(distance_away_from_mm(&slot, &square, &[], 3.0), 0.0, TOL);
        let beside = open_line((1.0, 0.0), (1.0, 8.0));
        let at = [Point::new(1.0, 0.0)];
        assert_approx_eq!(distance_away_from_mm(&beside, &square, &at, 3.0), 1.0, TOL);
        // Wholly inside the disc: nothing is left to measure.
        let stub = open_line((10.0, 0.0), (10.0, 2.0));
        let near = distance_away_from_mm(
            &stub,
            &open_line((9.0, 0.0), (11.0, 0.0)),
            &[Point::new(10.0, 0.0)],
            3.0,
        );
        assert!(near.is_infinite(), "{near}");
    }

    /// #105: the nearest point of a polyline names how far along it lies,
    /// on the segment it is on.
    #[test]
    fn nearest_point_on_a_line_reports_distance_along() {
        let square = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        let near = nearest_point(&square, Point::new(13.0, 4.0)).unwrap();
        assert_approx_eq!(near.point.x, 10.0, 1e-12);
        assert_approx_eq!(near.point.y, 4.0, 1e-12);
        assert_approx_eq!(near.gap_mm, 3.0, 1e-12);
        assert_approx_eq!(near.along_mm, 14.0, 1e-12);
        assert_eq!(near.segment, 1);
        assert_approx_eq!(near.along_segment_mm, 4.0, 1e-12);
        // A point on the path is its own nearest point.
        let (on, _) = square.point_at(27.5).unwrap();
        let near = nearest_point(&square, on).unwrap();
        assert_approx_eq!(near.gap_mm, 0.0, 1e-12);
        assert_approx_eq!(near.along_mm, 27.5, 1e-9);
    }

    /// #105: on an arc the distance along follows the sweep, either way
    /// round.
    #[test]
    fn nearest_point_on_an_arc_follows_the_sweep() {
        for circle in [
            Polyline::circle(Point::new(0.0, 0.0), 10.0),
            Polyline::circle(Point::new(0.0, 0.0), 10.0).reversed(),
        ] {
            for along_mm in [0.5, 10.0, 31.0, 50.0, 62.0] {
                let (on, _) = circle.point_at(along_mm).unwrap();
                let outside = on + (on - Point::new(0.0, 0.0)) * 0.3;
                let near = nearest_point(&circle, outside).unwrap();
                assert_approx_eq!(near.gap_mm, 3.0, 1e-9);
                assert_approx_eq!(near.along_mm, along_mm, 1e-9);
            }
        }
    }

    /// #105: every point of an arc centred on the given point is equally
    /// near it, so the arc's middle is taken: a tab at a corner lands in
    /// the middle of the arc an offset path turns round that corner.
    #[test]
    fn nearest_point_at_an_arc_centre_takes_the_arc_middle() {
        // A quarter arc of radius 2 round (10, 0), from (10, -2) to (12, 0).
        let path = Polyline::new(
            vec![
                Vertex::line(0.0, -2.0),
                Vertex::new(10.0, -2.0, (std::f64::consts::PI / 8.0).tan()),
                Vertex::line(12.0, 0.0),
                Vertex::line(12.0, 10.0),
            ],
            false,
        );
        let near = nearest_point(&path, Point::new(10.0, 0.0)).unwrap();
        let middle = std::f64::consts::FRAC_PI_4;
        assert_approx_eq!(near.point.x, 10.0 + 2.0 * middle.sin(), 1e-9);
        assert_approx_eq!(near.point.y, -2.0 * middle.cos(), 1e-9);
        assert_approx_eq!(near.along_mm, 10.0 + std::f64::consts::PI / 2.0, 1e-9);
        assert!(nearest_point(&Polyline::new(Vec::new(), false), Point::new(0.0, 0.0)).is_none());
    }
}
