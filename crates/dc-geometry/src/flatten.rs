// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Flattening of Bézier paths into line-segment polylines at a distance
//! tolerance, via kurbo (`docs/specs/phase-1/design.md`, "Data model →
//! dc-geometry"). Importers flatten once; generators keep arcs as bulges
//! and never re-flatten.

use crate::polyline::Polyline;

/// Default flattening tolerance, mm: the furthest a flattened polyline may
/// stray from the source curve. Matches the project's
/// `flatten_tolerance_mm` default.
pub const FLATTEN_TOLERANCE_DEFAULT_MM: f64 = 0.01;

/// PureCutCNC's samples per Bézier curve (`DEFAULT_FLATTEN_CURVE_SAMPLES` in
/// `src/engine/toolpaths/geometry.ts`). Not used for flattening here, which
/// is tolerance-based. Kept for golden-corpus comparison against PureCutCNC
/// output.
pub const DEFAULT_FLATTEN_CURVE_SAMPLES: u32 = 24;

/// PureCutCNC's arc sampling step, radians (`DEFAULT_FLATTEN_ARC_STEP`,
/// π/36). Kept for the same reason as [`DEFAULT_FLATTEN_CURVE_SAMPLES`].
pub const DEFAULT_FLATTEN_ARC_STEP_RAD: f64 = std::f64::consts::PI / 36.0;

/// Flattens `path` into one polyline per subpath, with no vertex or
/// segment further than `tolerance_mm` from the curve. Coordinates pass
/// through unchanged: the caller has already transformed, scaled, and
/// flipped the path into project space. A subpath ending in `ClosePath`
/// becomes a closed polyline; any other subpath stays open. Subpaths with
/// fewer than two distinct points are dropped.
#[must_use]
pub fn flatten(path: &kurbo::BezPath, tolerance_mm: f64) -> Vec<Polyline> {
    let _ = (path, tolerance_mm);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::Point;
    use kurbo::Shape;

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
