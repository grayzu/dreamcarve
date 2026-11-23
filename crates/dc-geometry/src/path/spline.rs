// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/types/project.ts (splineProfile)
//   src/store/helpers/profileEdit.ts (buildArcSegmentFromThreePoints,
//     appendSplineDraftSegment, arcControlPoint)
//   src/store/helpers/profileEdit.test.ts

//! The two constructions the drawing tools build a path from: an arc
//! through three points and a smooth curve through a run of points
//! (`docs/specs/phase-5/design.md`, "Algorithms → Paths").
//!
//! Re-derived for Y-up: upstream stores an arc as a centre and a
//! `clockwise` flag that is true when the angle decreases, which is
//! clockwise in Y-up numbers. Here an arc is its bulge, positive when
//! counter-clockwise, so upstream's `clockwise = cross < 0` is a negative
//! bulge for a negative cross product, and no centre is kept.

use super::{Path, PathNode, Span};
use crate::point::Point;
use crate::polyline::Segment;

/// Three points closer to a line than this make no arc: twice the area
/// of their triangle, square millimetres (upstream's `denominator` test).
const COLLINEAR_MM2: f64 = 1e-9;

/// The bulge of the circular arc from `start` to `end` that passes
/// through `through` (`buildArcSegmentFromThreePoints`): positive when
/// the arc runs counter-clockwise. `None` when the three points are in
/// line, or two of them coincide, so no circle passes through them.
#[must_use]
pub fn bulge_through(start: Point, end: Point, through: Point) -> Option<f64> {
    // Positive when `through` is to the right of the way from `start` to
    // `end`, which is where a counter-clockwise arc bows.
    let cross = (through - start).cross(end - start);
    if !cross.is_finite() || (2.0 * cross).abs() < COLLINEAR_MM2 {
        return None;
    }
    // The angle the chord subtends at a point of the arc is half the
    // rest of the turn: sweep = 2·(π − angle), bulge = tan(sweep / 4).
    let (to_start, to_end) = (start - through, end - through);
    let angle = to_start.cross(to_end).abs().atan2(to_start.dot(to_end));
    let bulge = ((std::f64::consts::PI - angle) / 2.0).tan();
    Some(bulge.copysign(cross))
}

/// The middle point of the arc from `start` to `end` with `bulge`
/// (`arcControlPoint`): the handle an arc is dragged by. The chord's
/// middle for a bulge of 0.
#[must_use]
pub fn arc_middle(start: Point, end: Point, bulge: f64) -> Point {
    let segment = Segment { start, end, bulge };
    segment
        .point_at(segment.length_mm() / 2.0)
        .map_or_else(|| start + (end - start) * 0.5, |(point, _)| point)
}

impl Path {
    /// A smooth curve through `points`, in order (`splineProfile` for a
    /// closed one, `appendSplineDraftSegment` for an open one): a cubic
    /// between each two neighbours whose handles follow the line between
    /// the points either side, the uniform Catmull-Rom tangents. With
    /// `p0 … p3` four points in a row, the span from `p1` to `p2` has
    /// handles `p1 + (p2 − p0)/6` and `p2 − (p3 − p1)/6`. A closed curve
    /// wraps round; an open one takes each end's missing neighbour to be
    /// the end itself. Every node is smooth.
    ///
    /// Fewer than three points cannot close: two points give a straight
    /// line, as upstream's fall-back to a polygon does, and fewer give a
    /// path with no span.
    #[must_use]
    pub fn spline(points: &[Point], closed: bool) -> Self {
        let n = points.len();
        if n < 3 {
            let nodes = points.iter().map(|p| PathNode::line(p.x, p.y)).collect();
            return Self::new(nodes, false);
        }
        let at = |index: isize| -> Point {
            let last = n - 1;
            let index = if closed {
                index
                    .rem_euclid(isize::try_from(n).unwrap_or(isize::MAX))
                    .unsigned_abs()
            } else {
                usize::try_from(index).unwrap_or(0).min(last)
            };
            points.get(index).copied().unwrap_or_default()
        };
        let nodes = (0..n)
            .map(|i| {
                let i = isize::try_from(i).unwrap_or(isize::MAX);
                let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                PathNode {
                    x: p1.x,
                    y: p1.y,
                    span: Span::Cubic {
                        handle_out: p1 + (p2 - p0) * (1.0 / 6.0),
                        handle_in: p2 + (p1 - p3) * (1.0 / 6.0),
                    },
                    smooth: true,
                }
            })
            .collect();
        Self::new(nodes, closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

    /// `testBuildArcSegmentFromThreePoints`: from (1, 0) to (0, 1) through
    /// the point between them on the unit circle. Upstream finds the
    /// centre (0, 0) and `clockwise: false`; here that is a quarter turn
    /// counter-clockwise, a bulge of tan(π/8), about that centre.
    #[test]
    fn arc_through_three_points_bulges_toward_the_third() {
        let (start, end) = (Point::new(1.0, 0.0), Point::new(0.0, 1.0));
        let through = Point::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2);
        let bulge = bulge_through(start, end, through).unwrap();
        assert_approx_eq!(bulge, (PI / 8.0).tan(), 1e-12);
        let arc = Segment { start, end, bulge }.arc().unwrap();
        assert_approx_eq!(arc.centre.x, 0.0, 1e-12);
        assert_approx_eq!(arc.centre.y, 0.0, 1e-12);
        assert_approx_eq!(arc.sweep_rad, FRAC_PI_2, 1e-12);
        // The other way round the same circle: three quarters of a turn,
        // clockwise.
        let far = Point::new(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
        let bulge = bulge_through(start, end, far).unwrap();
        let arc = Segment { start, end, bulge }.arc().unwrap();
        assert_approx_eq!(arc.sweep_rad, -3.0 * FRAC_PI_2, 1e-12);
        assert_approx_eq!(arc.centre.x, 0.0, 1e-12);
        // The arc passes through the third point: its middle is it.
        let middle = arc_middle(start, end, bulge);
        assert_approx_eq!(middle.x, far.x, 1e-12);
        assert_approx_eq!(middle.y, far.y, 1e-12);
    }

    /// The arc passes through its third point wherever that is on it,
    /// not only at its middle.
    #[test]
    fn the_arc_passes_through_the_third_point() {
        let (start, end) = (Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        for through in [
            Point::new(1.0, 4.0),
            Point::new(9.0, -0.5),
            Point::new(5.0, 30.0),
            Point::new(-3.0, 2.0),
            Point::new(12.0, -7.0),
        ] {
            let bulge = bulge_through(start, end, through).unwrap();
            let arc = Segment { start, end, bulge }.arc().unwrap();
            assert_approx_eq!(arc.centre.distance_to(through), arc.radius_mm, 1e-9);
            // And on the arc's own side of the circle, not its complement.
            let gap = crate::point_segment_distance_mm(Segment { start, end, bulge }, through);
            assert!(gap < 1e-9, "{through:?} is {gap} mm off the arc");
            // Above the chord the arc runs clockwise, below it
            // counter-clockwise.
            assert_eq!(bulge > 0.0, through.y < 0.0, "{through:?}");
        }
    }

    /// Three points in a line, or two of them the same, make no arc.
    #[test]
    fn collinear_points_make_no_arc() {
        let (start, end) = (Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(bulge_through(start, end, Point::new(5.0, 0.0)), None);
        assert_eq!(bulge_through(start, end, Point::new(20.0, 0.0)), None);
        assert_eq!(bulge_through(start, end, start), None);
        assert_eq!(bulge_through(start, end, end), None);
        assert_eq!(bulge_through(start, start, Point::new(3.0, 3.0)), None);
        assert_eq!(bulge_through(start, end, Point::new(f64::NAN, 1.0)), None);
        // A straight span's middle is its chord's.
        let middle = arc_middle(start, end, 0.0);
        assert_approx_eq!(middle.x, 5.0, 1e-12);
    }

    /// A closed spline passes through its points and is smooth at each:
    /// the handle arriving and the handle leaving are in line with the
    /// point, the same length either side (`splineProfile`).
    #[test]
    fn a_closed_spline_passes_through_its_points() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(40.0, 5.0),
            Point::new(50.0, 40.0),
            Point::new(10.0, 30.0),
        ];
        let path = Path::spline(&points, true);
        assert!(path.is_closed());
        assert_eq!(path.span_count(), 4);
        for (i, node) in path.nodes().iter().enumerate() {
            assert_eq!(node.point(), points[i]);
            assert!(node.smooth);
            let Span::Cubic { handle_out, .. } = node.span else {
                panic!("a cubic");
            };
            let before = path.nodes()[(i + 3) % 4];
            let Span::Cubic { handle_in, .. } = before.span else {
                panic!("a cubic");
            };
            let (out, back) = (handle_out - node.point(), handle_in - node.point());
            assert_approx_eq!(out.x + back.x, 0.0, 1e-12);
            assert_approx_eq!(out.y + back.y, 0.0, 1e-12);
            // The tangent is a sixth of the way between the neighbours.
            let expected = (points[(i + 1) % 4] - points[(i + 3) % 4]) * (1.0 / 6.0);
            assert_approx_eq!(out.x, expected.x, 1e-12);
            assert_approx_eq!(out.y, expected.y, 1e-12);
        }
        // The flattened outline goes through every point.
        let flat = path.to_polyline(0.01);
        for point in points {
            assert!(flat.vertices().iter().any(|v| v.point() == point));
        }
    }

    /// An open spline is what upstream builds a click at a time: the
    /// first span leaves along the line to the second point, a sixth of
    /// its length, and the last arrives the same way.
    #[test]
    fn an_open_spline_has_short_end_tangents() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(30.0, 0.0),
            Point::new(30.0, 30.0),
        ];
        let path = Path::spline(&points, false);
        assert!(!path.is_closed());
        assert_eq!(path.span_count(), 2);
        let Span::Cubic {
            handle_out,
            handle_in,
        } = path.nodes()[0].span
        else {
            panic!("a cubic");
        };
        // `control1 = current + (to − previous)/6` with no previous point.
        assert_eq!(handle_out, Point::new(5.0, 0.0));
        // Rewritten when the third point came: `current − (to − previous)/6`.
        assert_eq!(handle_in, Point::new(25.0, -5.0));
        let Span::Cubic {
            handle_out,
            handle_in,
        } = path.nodes()[1].span
        else {
            panic!("a cubic");
        };
        assert_eq!(handle_out, Point::new(35.0, 5.0));
        // `control2 = to − (to − current)/6`.
        assert_eq!(handle_in, Point::new(30.0, 25.0));
    }

    /// Two points are a line and one is nothing to draw; neither closes.
    #[test]
    fn too_few_points_make_no_curve() {
        let two = Path::spline(&[Point::new(0.0, 0.0), Point::new(5.0, 5.0)], true);
        assert!(!two.is_closed());
        assert!(!two.has_curves());
        assert_eq!(two.span_count(), 1);
        assert_eq!(Path::spline(&[Point::new(1.0, 1.0)], false).span_count(), 0);
        assert_eq!(Path::spline(&[], true).nodes().len(), 0);
    }
}
