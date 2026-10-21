// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Parallel offset of contours with round joins, via cavalier_contours in
//! f64 with bulge arcs (`docs/specs/phase-1/design.md`, "Data model →
//! dc-geometry"). Replaces PureCutCNC's Clipper offset; its integer scaling
//! has no counterpart here.

use crate::contour::Contour;
use crate::polyline::{Polyline, Vertex};
use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut};

pub(crate) type CavPolyline = cavalier_contours::polyline::Polyline<f64>;

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
        .filter_map(|p| Contour::oriented(from_cavalier(p)).ok())
        .map(|c| if ccw { c } else { c.reversed() })
        .collect()
}

/// Whether every point inside `contour` lies within `inset_mm` of it: the
/// contour offset inward by `inset_mm` is empty (#223, a part a bit of
/// radius `inset_mm` cut on its line removes whole).
///
/// The points inside a closed line of length `L` and within `t` of it
/// cover at most `L·t`: each inner parallel curve is no longer than the
/// line itself. So an area above `L · inset_mm` always keeps a point
/// farther in, and only a smaller one is offset to find out, which keeps a
/// large outline of many chords from being offset at all.
#[must_use]
pub fn inset_is_empty(contour: &Contour, inset_mm: f64) -> bool {
    let area_mm2 = contour.signed_area_mm2().abs();
    if inset_mm.is_nan() || inset_mm <= 0.0 || area_mm2 > contour.polyline().length_mm() * inset_mm
    {
        return false;
    }
    offset(contour, -inset_mm).is_empty()
}

pub(crate) fn to_cavalier(polyline: &Polyline) -> CavPolyline {
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

    /// A circle of radius 2.5 and a 3 mm square lie within 3.175 mm of
    /// their lines everywhere; a 3.3 mm circle and a 7 mm square do not,
    /// nor does a large circle of many chords, which the area bound settles
    /// without an offset.
    #[test]
    fn inset_is_empty_when_the_part_lies_within_the_inset() {
        let contour = |p: Polyline| Contour::new(p).unwrap();
        let at = Point::new(0.0, 0.0);
        assert!(inset_is_empty(
            &contour(Polyline::circle(at, 2.5)),
            TOOL_RADIUS_MM
        ));
        assert!(inset_is_empty(
            &contour(Polyline::rect(at, 3.0, 3.0)),
            TOOL_RADIUS_MM
        ));
        assert!(inset_is_empty(
            &contour(Polyline::rect(at, 50.0, 3.0)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(
            &contour(Polyline::circle(at, 3.3)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(
            &contour(Polyline::rect(at, 7.0, 7.0)),
            TOOL_RADIUS_MM
        ));
        let chords = (0..2000)
            .map(|k| {
                let a = f64::from(k) * std::f64::consts::TAU / 2000.0;
                Vertex::line(14.0 * a.cos(), 14.0 * a.sin())
            })
            .collect();
        assert!(!inset_is_empty(
            &contour(Polyline::new(chords, true)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(&contour(Polyline::circle(at, 2.5)), 0.0));
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

    fn open(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    fn ends(p: &Polyline) -> (Point, Point) {
        let vs = p.vertices();
        (vs[0].point(), vs[vs.len() - 1].point())
    }

    /// #207: a positive `left_mm` moves an open path to the left of
    /// travel from its first vertex to its last, which in Y-up is the
    /// normal `(−dy, dx)`; a negative one to the right. The result runs
    /// the same way as the path and ends square, on the normals through
    /// the path's end vertices.
    #[test]
    fn open_offset_moves_left_of_travel_and_ends_square() {
        let line = open(&[(0.0, 0.0), (100.0, 0.0)]);
        for (left_mm, y) in [
            (TOOL_RADIUS_MM, TOOL_RADIUS_MM),
            (-TOOL_RADIUS_MM, -TOOL_RADIUS_MM),
        ] {
            let out = offset_open(&line, left_mm);
            assert_eq!(out.len(), 1, "{left_mm}: {out:?}");
            assert!(!out[0].is_closed());
            let (start, end) = ends(&out[0]);
            assert!(start.distance_to(Point::new(0.0, y)) <= TOL, "{start:?}");
            assert!(end.distance_to(Point::new(100.0, y)) <= TOL, "{end:?}");
        }
        // Drawn the other way, left is the other side.
        let back = offset_open(&line.reversed(), TOOL_RADIUS_MM);
        let (start, end) = ends(&back[0]);
        assert!(start.distance_to(Point::new(100.0, -TOOL_RADIUS_MM)) <= TOL);
        assert!(end.distance_to(Point::new(0.0, -TOOL_RADIUS_MM)) <= TOL);
        // Upward, left is −X.
        let up = offset_open(&open(&[(0.0, 0.0), (0.0, 50.0)]), TOOL_RADIUS_MM);
        let (start, _) = ends(&up[0]);
        assert!(start.distance_to(Point::new(-TOOL_RADIUS_MM, 0.0)) <= TOL);
    }

    /// #207: on the outside of a turn the offset rounds the corner with
    /// an arc about the vertex, as a closed offset does; on the inside the
    /// two offset edges are trimmed where they cross. Either way every
    /// point of the result keeps the distance from the path.
    #[test]
    fn open_offset_rounds_outer_turns_and_trims_inner_ones() {
        // Turns left at (50, 0): left is the inside of the turn.
        let l_path = open(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]);
        let inner = offset_open(&l_path, TOOL_RADIUS_MM);
        assert_eq!(inner.len(), 1, "{inner:?}");
        let corner = Point::new(50.0 - TOOL_RADIUS_MM, TOOL_RADIUS_MM);
        assert!(
            inner[0]
                .vertices()
                .iter()
                .any(|v| v.point().distance_to(corner) <= TOL),
            "trimmed at {corner:?}: {inner:?}"
        );
        assert!(inner[0].segments().all(|s| s.is_line()));
        let outer = offset_open(&l_path, -TOOL_RADIUS_MM);
        assert_eq!(outer.len(), 1, "{outer:?}");
        assert_eq!(outer[0].segments().filter(|s| !s.is_line()).count(), 1);
        let (start, end) = ends(&outer[0]);
        assert!(start.distance_to(Point::new(0.0, -TOOL_RADIUS_MM)) <= TOL);
        assert!(end.distance_to(Point::new(50.0 + TOOL_RADIUS_MM, 50.0)) <= TOL);
        for piece in inner.iter().chain(&outer) {
            for s in piece.segments() {
                for k in 0..=8 {
                    let (p, _) = s.point_at(s.length_mm() * f64::from(k) / 8.0).unwrap();
                    let d = l_path
                        .segments()
                        .map(|t| crate::point_segment_distance_mm(t, p))
                        .fold(f64::INFINITY, f64::min);
                    assert!(d >= TOOL_RADIUS_MM - TOL, "{p:?} is {d} from the path");
                }
            }
        }
    }

    /// #207: an arc whose radius is less than the offset on the side its
    /// centre lies on collapses: nothing of the path is left to cut.
    #[test]
    fn open_offset_of_a_tight_arc_toward_its_centre_vanishes() {
        // A half circle of radius 2 from (2, 0) to (−2, 0) counter-clockwise
        // round the origin: its centre is on its left.
        let arc = Polyline::new(
            vec![Vertex::new(2.0, 0.0, 1.0), Vertex::line(-2.0, 0.0)],
            false,
        );
        assert!(offset_open(&arc, TOOL_RADIUS_MM).is_empty());
        let grown = offset_open(&arc, -TOOL_RADIUS_MM);
        assert_eq!(grown.len(), 1);
        assert_approx_eq!(
            grown[0].length_mm(),
            std::f64::consts::PI * (2.0 + TOOL_RADIUS_MM),
            1e-6
        );
    }

    #[test]
    fn circle_offset_stays_a_circle() {
        let c = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 45.0)).unwrap();
        let out = offset(&c, TOOL_RADIUS_MM);
        let r = 45.0 + TOOL_RADIUS_MM;
        assert_approx_eq!(out[0].signed_area_mm2(), std::f64::consts::PI * r * r, 1e-6);
    }
}
