// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Closed polylines with known orientation (`docs/specs/phase-1/design.md`,
//! "Data model → dc-geometry"). Outer boundaries are counter-clockwise and
//! holes clockwise (ADR-0001).

use crate::offset::to_cavalier;
use crate::point::{EPS_MM, Point};
use crate::polyline::Polyline;
use crate::{DegenerateReason, GeometryError};
use cavalier_contours::polyline::{
    PlineIntersect, PlineSelfIntersectOptions, PlineSource, SelfIntersectsInclude,
};

/// A closed polyline with non-zero area and known orientation.
/// [`Contour::new`] normalises to counter-clockwise; [`Contour::reversed`]
/// gives the clockwise form used for holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour(Polyline);

impl Contour {
    /// A counter-clockwise contour from a closed polyline, reversing it if
    /// it winds clockwise. Fails when the polyline is open, has fewer than
    /// two vertices, encloses no area, or crosses itself.
    ///
    /// Crossing means two segments that are not neighbours meet anywhere,
    /// even at a single point, or neighbours meet anywhere but their shared
    /// vertex (an arc that loops back over the next segment, or a spike
    /// that doubles back). Arcs are tested as true arcs. Vertices repeated
    /// back to back are ignored, as zero-length segments.
    pub fn new(polyline: Polyline) -> Result<Self, GeometryError> {
        let contour = Self::oriented(polyline)?;
        match first_self_intersection(contour.polyline()) {
            Some(at) => Err(GeometryError::SelfIntersecting { at }),
            None => Ok(contour),
        }
    }

    /// [`Contour::new`] without the self-intersection test, for polylines
    /// that cannot cross themselves by construction, such as the loops
    /// cavalier_contours returns from an offset.
    pub(crate) fn oriented(polyline: Polyline) -> Result<Self, GeometryError> {
        if !polyline.is_closed() {
            return Err(GeometryError::Degenerate(DegenerateReason::Open));
        }
        if polyline.vertices().len() < 2 {
            return Err(GeometryError::Degenerate(DegenerateReason::TooFewVertices));
        }
        let area = polyline.signed_area_mm2();
        if area.abs() <= EPS_MM * EPS_MM {
            return Err(GeometryError::Degenerate(DegenerateReason::ZeroArea));
        }
        Ok(if area > 0.0 {
            Self(polyline)
        } else {
            Self(polyline.reversed())
        })
    }

    /// The underlying polyline.
    #[must_use]
    pub fn polyline(&self) -> &Polyline {
        &self.0
    }

    /// The underlying polyline, by value.
    #[must_use]
    pub fn into_polyline(self) -> Polyline {
        self.0
    }

    /// Signed area, mm²: positive when counter-clockwise.
    #[must_use]
    pub fn signed_area_mm2(&self) -> f64 {
        self.0.signed_area_mm2()
    }

    /// True when the contour winds counter-clockwise (an outer boundary).
    #[must_use]
    pub fn is_ccw(&self) -> bool {
        self.signed_area_mm2() > 0.0
    }

    /// The same contour with the opposite orientation.
    #[must_use]
    pub fn reversed(&self) -> Contour {
        Contour(self.0.reversed())
    }
}

/// Where `polyline` (closed) first crosses or touches itself, or `None`.
/// "First" is on the lowest-numbered segment involved, nearest its start,
/// so the answer does not depend on the order cavalier_contours reports in.
fn first_self_intersection(polyline: &Polyline) -> Option<Point> {
    // Non-finite coordinates are rejected where input enters (#59);
    // cavalier_contours' spatial index asserts on them, so never pass any.
    let finite = polyline
        .vertices()
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.bulge.is_finite());
    if !finite {
        return None;
    }
    let source = to_cavalier(polyline);
    // A zero-length segment meets both neighbours at its one point, which
    // is not a crossing; drop it before testing.
    let cleaned = source.remove_repeat_pos(EPS_MM);
    let pline = cleaned.as_ref().unwrap_or(&source);
    let position = |i: usize| {
        let v = pline.at(i);
        Point::new(v.x, v.y)
    };
    // The vertex segments `i` and `j` share, when they are neighbours.
    let shared = |i: usize, j: usize| -> Vec<Point> {
        let mut out = Vec::new();
        if pline.next_wrapping_index(i) == j {
            out.push(position(j));
        }
        if pline.next_wrapping_index(j) == i {
            out.push(position(i));
        }
        out
    };
    let at_shared = |p: Point, shared: &[Point]| shared.iter().any(|s| s.distance_to(p) <= EPS_MM);

    let mut first: Option<(usize, f64, Point)> = None;
    let mut consider = |i: usize, j: usize, points: &[Point]| {
        let shared = shared(i, j);
        // Neighbours always meet at their shared vertex; that alone is fine.
        let Some(&at) = points.iter().find(|p| !at_shared(**p, &shared)) else {
            return;
        };
        let segment = i.min(j);
        let along_mm = position(segment).distance_to(at);
        let earlier = first
            .as_ref()
            .is_none_or(|&(s, d, _)| segment < s || (segment == s && along_mm < d));
        if earlier {
            first = Some((segment, along_mm, at));
        }
    };
    let mut options = PlineSelfIntersectOptions::new();
    options.pos_equal_eps = EPS_MM;
    options.include = SelfIntersectsInclude::All;
    let mut visitor = |intersect: PlineIntersect<f64>| match intersect {
        PlineIntersect::Basic(b) => consider(
            b.start_index1,
            b.start_index2,
            &[Point::new(b.point.x, b.point.y)],
        ),
        PlineIntersect::Overlapping(o) => consider(
            o.start_index1,
            o.start_index2,
            &[
                Point::new(o.point1.x, o.point1.y),
                Point::new(o.point2.x, o.point2.y),
            ],
        ),
    };
    pline.visit_self_intersects_opt(&mut visitor, &options);
    first.map(|(_, _, at)| at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::point::Point;
    use crate::polyline::Vertex;

    fn cw_square() -> Polyline {
        Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(0.0, 10.0),
                Vertex::line(10.0, 10.0),
                Vertex::line(10.0, 0.0),
            ],
            true,
        )
    }

    #[test]
    fn normalises_cw_input_to_ccw() {
        let input = cw_square();
        assert!(input.signed_area_mm2() < 0.0);
        let c = Contour::new(input).unwrap();
        assert!(c.is_ccw());
        assert_approx_eq!(c.signed_area_mm2(), 100.0, EPS_MM);
    }

    #[test]
    fn ccw_input_is_kept_as_is() {
        let input = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        let c = Contour::new(input.clone()).unwrap();
        assert_eq!(c.polyline(), &input);
    }

    #[test]
    fn area_sign_matches_orientation() {
        let ccw = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 5.0)).unwrap();
        assert!(ccw.is_ccw());
        assert!(ccw.signed_area_mm2() > 0.0);
        let cw = ccw.reversed();
        assert!(!cw.is_ccw());
        assert!(cw.signed_area_mm2() < 0.0);
    }

    #[test]
    fn open_polyline_is_rejected() {
        let open = Polyline::new(cw_square().vertices().to_vec(), false);
        assert_eq!(
            Contour::new(open),
            Err(GeometryError::Degenerate(DegenerateReason::Open))
        );
    }

    #[test]
    fn zero_area_is_rejected() {
        let flat = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(10.0, 0.0),
                Vertex::line(20.0, 0.0),
            ],
            true,
        );
        assert_eq!(
            Contour::new(flat),
            Err(GeometryError::Degenerate(DegenerateReason::ZeroArea))
        );
    }

    /// The bowtie from issue #58. Edges (0,0)-(60,40) and (60,0)-(0,30)
    /// cross where y = 2x/3 meets y = 30 - x/2: x = 180/7, y = 120/7. The
    /// lobes wind opposite ways but differ in area, so the net area is not
    /// zero and only the crossing check can reject it.
    #[test]
    fn bowtie_is_rejected_as_self_intersecting() {
        let bowtie = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(60.0, 40.0),
                Vertex::line(60.0, 0.0),
                Vertex::line(0.0, 30.0),
            ],
            true,
        );
        let Err(GeometryError::SelfIntersecting { at }) = Contour::new(bowtie) else {
            panic!("bowtie must be rejected as self-intersecting");
        };
        assert_approx_eq!(at.x, 180.0 / 7.0, 1e-6);
        assert_approx_eq!(at.y, 120.0 / 7.0, 1e-6);
    }

    /// A figure-8 whose two crossing strokes are bulge arcs, A→B and C→D,
    /// mirror images of each other across the Y axis, so they cross once,
    /// at x = 0. The right loop (bulge 2, about 253°) is bigger than the
    /// left (a semicircle), so the net area is not zero.
    #[test]
    fn figure_eight_with_arcs_is_rejected() {
        let figure_eight = Polyline::new(
            vec![
                Vertex::new(-5.0, 5.0, 0.25),
                Vertex::new(5.0, -5.0, 2.0),
                Vertex::new(5.0, 5.0, -0.25),
                Vertex::new(-5.0, -5.0, -1.0),
            ],
            true,
        );
        assert!(figure_eight.signed_area_mm2().abs() > 1.0);
        let Err(GeometryError::SelfIntersecting { at }) = Contour::new(figure_eight) else {
            panic!("figure-8 must be rejected as self-intersecting");
        };
        assert_approx_eq!(at.x, 0.0, 1e-6);
        assert!(at.y.abs() < 5.0, "crossing between the lobes: {at:?}");
    }

    /// Outlines that touch themselves only where neighbouring segments
    /// share a vertex still construct: a square, a concave star, a square
    /// with an extra vertex in the middle of an edge, one with a vertex
    /// repeated, and a circle made of two arcs.
    #[test]
    fn simple_polygons_are_accepted() {
        let star: Vec<Vertex> = (0..10)
            .map(|k| {
                let angle = f64::from(k) * std::f64::consts::PI / 5.0;
                let r = if k % 2 == 0 { 10.0 } else { 4.0 };
                Vertex::line(r * angle.cos(), r * angle.sin())
            })
            .collect();
        let cases = [
            ("square", Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0)),
            ("star", Polyline::new(star, true)),
            (
                "collinear vertex",
                Polyline::new(
                    vec![
                        Vertex::line(0.0, 0.0),
                        Vertex::line(5.0, 0.0),
                        Vertex::line(10.0, 0.0),
                        Vertex::line(10.0, 10.0),
                        Vertex::line(0.0, 10.0),
                    ],
                    true,
                ),
            ),
            (
                "repeated vertex",
                Polyline::new(
                    vec![
                        Vertex::line(0.0, 0.0),
                        Vertex::line(10.0, 0.0),
                        Vertex::line(10.0, 0.0),
                        Vertex::line(10.0, 10.0),
                        Vertex::line(0.0, 10.0),
                        Vertex::line(0.0, 0.0),
                    ],
                    true,
                ),
            ),
            ("circle", Polyline::circle(Point::new(0.0, 0.0), 5.0)),
        ];
        for (name, polyline) in cases {
            assert!(
                Contour::new(polyline).is_ok(),
                "{name} must construct as a contour"
            );
        }
    }

    #[test]
    fn single_vertex_is_rejected() {
        let one = Polyline::new(vec![Vertex::line(1.0, 1.0)], true);
        assert_eq!(
            Contour::new(one),
            Err(GeometryError::Degenerate(DegenerateReason::TooFewVertices))
        );
    }
}
