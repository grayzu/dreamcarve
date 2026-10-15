// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Parallel offset of contours with round joins, via cavalier_contours in
//! f64 with bulge arcs (`docs/specs/phase-1/design.md`, "Data model →
//! dc-geometry"). Replaces PureCutCNC's Clipper offset; its integer scaling
//! has no counterpart here.

use crate::contour::Contour;
use crate::polyline::{Polyline, Vertex};
use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut};

type CavPolyline = cavalier_contours::polyline::Polyline<f64>;

/// Offsets `contour` by `delta_mm`. Positive `delta_mm` moves the contour
/// away from the area it encloses (grows it); negative moves it inward.
/// The rule is the same for a clockwise hole contour, so a hole offset by a
/// negative delta gets smaller. Results keep the input's orientation, and
/// corners become arcs (round joins). An inward offset can split a contour
/// into several or consume it: an empty result means nothing is left.
#[must_use]
pub fn offset(contour: &Contour, delta_mm: f64) -> Vec<Contour> {
    let ccw = contour.is_ccw();
    let source = to_cavalier(contour.polyline());
    // cavalier_contours offsets to the left of travel for a positive value:
    // inward on a CCW contour, outward on a CW one.
    let cav_delta = if ccw { -delta_mm } else { delta_mm };
    source
        .parallel_offset(cav_delta)
        .iter()
        .filter_map(|p| Contour::new(from_cavalier(p)).ok())
        .map(|c| if ccw { c } else { c.reversed() })
        .collect()
}

fn to_cavalier(polyline: &Polyline) -> CavPolyline {
    let mut out = CavPolyline::with_capacity(polyline.vertices().len(), polyline.is_closed());
    for v in polyline.vertices() {
        out.add(v.x, v.y, v.bulge);
    }
    out
}

fn from_cavalier(polyline: &CavPolyline) -> Polyline {
    let vertices = polyline
        .iter_vertexes()
        .map(|v| Vertex::new(v.x, v.y, v.bulge))
        .collect();
    Polyline::new(vertices, polyline.is_closed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::point::Point;
    use crate::polyline::Polyline;

    const TOL: f64 = 1e-6;
    const TOOL_RADIUS_MM: f64 = 3.175;

    fn rect_100x50() -> Contour {
        Contour::new(Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)).unwrap()
    }

    #[test]
    fn outside_rect_grows_bounds_by_diameter() {
        let out = offset(&rect_100x50(), TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 106.35, TOL);
        assert_approx_eq!(b.height_mm(), 56.35, TOL);
        assert_approx_eq!(b.min.x, -TOOL_RADIUS_MM, TOL);
        assert_approx_eq!(b.min.y, -TOOL_RADIUS_MM, TOL);
    }

    #[test]
    fn inside_rect_shrinks_bounds_by_diameter() {
        let out = offset(&rect_100x50(), -TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 93.65, TOL);
        assert_approx_eq!(b.height_mm(), 43.65, TOL);
    }

    #[test]
    fn inside_offset_larger_than_shape_vanishes() {
        assert!(offset(&rect_100x50(), -30.0).is_empty());
    }

    #[test]
    fn round_joins_produce_arc_vertices() {
        let out = offset(&rect_100x50(), TOOL_RADIUS_MM);
        let arcs = out[0]
            .polyline()
            .segments()
            .filter(|s| !s.is_line())
            .count();
        assert_eq!(arcs, 4);
    }

    #[test]
    fn offset_keeps_ccw_orientation() {
        for delta in [TOOL_RADIUS_MM, -TOOL_RADIUS_MM] {
            assert!(offset(&rect_100x50(), delta).iter().all(Contour::is_ccw));
        }
    }

    #[test]
    fn negative_offset_of_cw_hole_shrinks_it_and_stays_cw() {
        let hole = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 15.0))
            .unwrap()
            .reversed();
        let out = offset(&hole, -TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        assert!(!out[0].is_ccw());
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 30.0 - 6.35, TOL);
    }

    #[test]
    fn circle_offset_stays_a_circle() {
        let c = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 45.0)).unwrap();
        let out = offset(&c, TOOL_RADIUS_MM);
        let r = 45.0 + TOOL_RADIUS_MM;
        assert_approx_eq!(out[0].signed_area_mm2(), std::f64::consts::PI * r * r, 1e-6);
    }
}
