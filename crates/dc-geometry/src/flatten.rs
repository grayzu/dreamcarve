// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Flattening of Bézier paths into line-segment polylines at a distance
//! tolerance, via kurbo (`docs/specs/phase-1/design.md`, "Data model →
//! dc-geometry"). Importers flatten once; generators keep arcs as bulges
//! and never re-flatten.

use crate::point::{EPS_MM, Point};
use crate::polyline::{Polyline, Vertex};

/// Default flattening tolerance, mm: the furthest a flattened polyline may
/// stray from the source curve. Matches the project's
/// `flatten_tolerance_mm` default.
pub const FLATTEN_TOLERANCE_DEFAULT_MM: f64 = 0.01;

/// Flattens `path` into one polyline per subpath, with no vertex or
/// segment further than `tolerance_mm` from the curve. Coordinates pass
/// through unchanged: the caller has already transformed, scaled, and
/// flipped the path into project space. A subpath ending in `ClosePath`
/// becomes a closed polyline; any other subpath stays open. Subpaths with
/// fewer than two distinct points are dropped.
#[must_use]
pub fn flatten(path: &kurbo::BezPath, tolerance_mm: f64) -> Vec<Polyline> {
    let mut out = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    kurbo::flatten(path, tolerance_mm, |el| match el {
        kurbo::PathEl::MoveTo(p) => {
            finish(&mut out, std::mem::take(&mut current), false);
            current.push(Point::new(p.x, p.y));
        }
        kurbo::PathEl::LineTo(p) => push_distinct(&mut current, Point::new(p.x, p.y)),
        kurbo::PathEl::ClosePath => finish(&mut out, std::mem::take(&mut current), true),
        // `flatten` emits only moves, lines, and closes.
        kurbo::PathEl::QuadTo(..) | kurbo::PathEl::CurveTo(..) => {}
    });
    finish(&mut out, current, false);
    out
}

fn push_distinct(points: &mut Vec<Point>, p: Point) {
    if points
        .last()
        .is_none_or(|last| last.distance_to(p) > EPS_MM)
    {
        points.push(p);
    }
}

fn finish(out: &mut Vec<Polyline>, mut points: Vec<Point>, closed: bool) {
    if closed
        && points.len() > 1
        && let (Some(first), Some(last)) = (points.first(), points.last())
        && first.distance_to(*last) <= EPS_MM
    {
        points.pop();
    }
    if points.len() < 2 {
        return;
    }
    let vertices = points.into_iter().map(|p| Vertex::line(p.x, p.y)).collect();
    out.push(Polyline::new(vertices, closed));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::Point;
    use kurbo::{ParamCurveNearest, Shape};

    #[test]
    fn circle_within_tolerance() {
        let radius = 45.0;
        let tol = FLATTEN_TOLERANCE_DEFAULT_MM;
        // kurbo's own Bézier approximation of the circle is far tighter
        // than the flattening tolerance under test.
        let path = kurbo::Circle::new((10.0, 20.0), radius).to_path(1e-5);
        let out = flatten(&path, tol);
        assert_eq!(out.len(), 1);
        let centre = Point::new(10.0, 20.0);
        let pl = &out[0];
        assert!(pl.vertices().len() > 8);
        for seg in pl.segments() {
            assert!(seg.is_line());
            for p in [seg.start, seg.start + (seg.end - seg.start) * 0.5] {
                let err = (p.distance_to(centre) - radius).abs();
                assert!(err <= tol + 1e-5, "point {p:?} is {err} mm off the circle");
            }
        }
    }

    /// #225: kurbo's flattening bound is "not absolutely guaranteed", and
    /// on this cubic (the property test's shrunk case) a vertex lands
    /// 1.13 × the tolerance from the curve. Every point along every
    /// segment (sixteenths) must lie within the tolerance, measured with
    /// `CubicBez::nearest`.
    #[test]
    fn shrunk_cubic_stays_within_tolerance() {
        let curve = kurbo::CubicBez::new(
            (-86.789_338_779_879_57, -5.461_446_589_400_106),
            (91.760_128_305_684_76, -32.071_008_963_757_265),
            (-42.504_781_479_741_624, -16.363_669_339_829_194),
            (0.0, 60.115_254_630_605_09),
        );
        let tolerance_mm = 0.414_318_758_935_438_07;
        let mut path = kurbo::BezPath::new();
        path.move_to(curve.p0);
        path.curve_to(curve.p1, curve.p2, curve.p3);
        let out = flatten(&path, tolerance_mm);
        assert_eq!(out.len(), 1);
        for seg in out[0].segments() {
            for k in 0..=16 {
                let p = seg.start + (seg.end - seg.start) * (f64::from(k) / 16.0);
                let d_mm = curve
                    .nearest(kurbo::Point::new(p.x, p.y), 1e-12)
                    .distance_sq
                    .sqrt();
                assert!(
                    d_mm <= tolerance_mm,
                    "{p:?} is {d_mm} mm off the curve, tolerance {tolerance_mm}"
                );
            }
        }
    }

    #[test]
    fn preserves_closed_flag() {
        let closed = kurbo::Rect::new(0.0, 0.0, 10.0, 5.0).to_path(0.1);
        let out = flatten(&closed, 0.01);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_closed());
        assert_eq!(
            out[0].vertices().len(),
            4,
            "closing point is not duplicated"
        );

        let mut open = kurbo::BezPath::new();
        open.move_to((0.0, 0.0));
        open.line_to((10.0, 0.0));
        open.line_to((10.0, 10.0));
        let out = flatten(&open, 0.01);
        assert_eq!(out.len(), 1);
        assert!(!out[0].is_closed());
        assert_eq!(out[0].vertices().len(), 3);
    }

    #[test]
    fn each_subpath_becomes_its_own_polyline() {
        let mut path = kurbo::Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        path.extend(kurbo::Rect::new(20.0, 0.0, 30.0, 10.0).to_path(0.1));
        path.move_to((50.0, 0.0));
        path.line_to((60.0, 0.0));
        let out = flatten(&path, 0.01);
        let closed: Vec<bool> = out.iter().map(Polyline::is_closed).collect();
        assert_eq!(closed, [true, true, false]);
    }

    #[test]
    fn lone_move_to_is_dropped() {
        let mut path = kurbo::BezPath::new();
        path.move_to((1.0, 1.0));
        assert!(flatten(&path, 0.01).is_empty());
    }
}
