// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Polylines of line and arc segments with DXF-style bulges, and the
//! measurements on them: length, signed area, bounds, and point at distance
//! (`docs/specs/phase-1/design.md`, "Data model → dc-geometry").

use crate::point::{Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

/// Bulges smaller than this are straight segments. Dimensionless: a bulge
/// of 1e-9 on a 1 m chord bows the arc by 0.5 nm.
const BULGE_EPS: f64 = 1e-9;

/// A polyline vertex in millimetres, Y-up. `bulge` is tan(θ/4) of the arc
/// from this vertex to the next, where θ is the signed sweep: positive is
/// counter-clockwise, 0 is a straight segment. Same convention as
/// cavalier_contours and DXF.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vertex {
    /// X, millimetres.
    pub x: f64,
    /// Y, millimetres.
    pub y: f64,
    /// tan(θ/4) of the segment that starts here; dimensionless.
    pub bulge: f64,
}

impl Vertex {
    /// A vertex at (`x`, `y`) mm whose outgoing segment has `bulge`.
    #[must_use]
    pub const fn new(x: f64, y: f64, bulge: f64) -> Self {
        Self { x, y, bulge }
    }

    /// A vertex whose outgoing segment is straight.
    #[must_use]
    pub const fn line(x: f64, y: f64) -> Self {
        Self::new(x, y, 0.0)
    }

    /// The vertex position.
    #[must_use]
    pub const fn point(self) -> Point {
        Point::new(self.x, self.y)
    }
}

/// One segment of a polyline: a line when `bulge` is 0, else a circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Start point, mm.
    pub start: Point,
    /// End point, mm.
    pub end: Point,
    /// tan(θ/4) of the signed sweep; positive is counter-clockwise.
    pub bulge: f64,
}

/// Centre, radius, start angle, and signed sweep of an arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// Arc centre, mm.
    pub centre: Point,
    /// Radius, mm.
    pub radius_mm: f64,
    /// Angle of the start point about the centre, radians from +X.
    pub start_angle_rad: f64,
    /// Signed sweep, radians; positive is counter-clockwise.
    pub sweep_rad: f64,
}

impl Segment {
    /// True when the segment is a straight line.
    #[must_use]
    pub fn is_line(&self) -> bool {
        self.bulge.abs() < BULGE_EPS
    }

    /// The arc geometry, or `None` for a straight or zero-length segment.
    #[must_use]
    pub fn arc(&self) -> Option<Arc> {
        todo!()
    }

    /// Length along the segment, mm.
    #[must_use]
    pub fn length_mm(&self) -> f64 {
        todo!()
    }

    /// Position and unit tangent (direction of travel) at `distance_mm`
    /// from the start, clamped to the segment. `None` for a zero-length
    /// segment.
    #[must_use]
    pub fn point_at(&self, distance_mm: f64) -> Option<(Point, Vec2)> {
        let _ = distance_mm;
        todo!()
    }
}

/// An open or closed sequence of line and arc segments, millimetres, Y-up.
/// On an open polyline the last vertex's bulge is unused.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Polyline {
    vertices: Vec<Vertex>,
    closed: bool,
}

impl Polyline {
    /// A polyline through `vertices`; `closed` adds the segment from the
    /// last vertex back to the first.
    #[must_use]
    pub fn new(vertices: Vec<Vertex>, closed: bool) -> Self {
        Self { vertices, closed }
    }

    /// A closed axis-aligned rectangle, counter-clockwise from `min`.
    #[must_use]
    pub fn rect(min: Point, width_mm: f64, height_mm: f64) -> Self {
        let (x0, y0, x1, y1) = (min.x, min.y, min.x + width_mm, min.y + height_mm);
        Self::new(
            vec![
                Vertex::line(x0, y0),
                Vertex::line(x1, y0),
                Vertex::line(x1, y1),
                Vertex::line(x0, y1),
            ],
            true,
        )
    }

    /// A closed counter-clockwise circle made of two half-circle arcs,
    /// starting at the point on +X from `centre`.
    #[must_use]
    pub fn circle(centre: Point, radius_mm: f64) -> Self {
        Self::new(
            vec![
                Vertex::new(centre.x + radius_mm, centre.y, 1.0),
                Vertex::new(centre.x - radius_mm, centre.y, 1.0),
            ],
            true,
        )
    }

    /// The vertices in order.
    #[must_use]
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// True when the last vertex connects back to the first.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The segments in order, including the closing segment of a closed
    /// polyline.
    pub fn segments(&self) -> impl Iterator<Item = Segment> + '_ {
        let open = self
            .vertices
            .iter()
            .zip(self.vertices.iter().skip(1))
            .map(|(a, b)| Segment {
                start: a.point(),
                end: b.point(),
                bulge: a.bulge,
            });
        let closing = match (self.closed, self.vertices.first(), self.vertices.last()) {
            (true, Some(first), Some(last)) if self.vertices.len() > 1 => Some(Segment {
                start: last.point(),
                end: first.point(),
                bulge: last.bulge,
            }),
            _ => None,
        };
        open.chain(closing)
    }

    /// Total length along all segments, mm.
    #[must_use]
    pub fn length_mm(&self) -> f64 {
        todo!()
    }

    /// Signed enclosed area, mm²: positive when counter-clockwise, negative
    /// when clockwise, 0 for an open polyline.
    #[must_use]
    pub fn signed_area_mm2(&self) -> f64 {
        todo!()
    }

    /// The tight bounding rectangle, including arc extremes; `None` when
    /// there are no vertices.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        todo!()
    }

    /// Position and unit tangent at `distance_mm` along the polyline. A
    /// closed polyline wraps the distance modulo its length; an open one
    /// clamps it. `None` when the polyline has zero length.
    #[must_use]
    pub fn point_at(&self, distance_mm: f64) -> Option<(Point, Vec2)> {
        let _ = distance_mm;
        todo!()
    }

    /// The same path traversed in the opposite direction, with each arc's
    /// bulge negated. A closed polyline keeps its first vertex's position
    /// last-to-first, so the vertex order is fully reversed.
    #[must_use]
    pub fn reversed(&self) -> Polyline {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use std::f64::consts::PI;

    const TOL: f64 = 1e-9;

    #[test]
    fn closed_rect_has_expected_length() {
        let rect = Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0);
        assert_approx_eq!(rect.length_mm(), 300.0, TOL);
    }

    #[test]
    fn open_rect_omits_closing_segment() {
        let rect = Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0);
        let open = Polyline::new(rect.vertices().to_vec(), false);
        assert_approx_eq!(open.length_mm(), 250.0, TOL);
    }

    #[test]
    fn circle_length_and_area() {
        let c = Polyline::circle(Point::new(5.0, 5.0), 10.0);
        assert_approx_eq!(c.length_mm(), 2.0 * PI * 10.0, TOL);
        assert_approx_eq!(c.signed_area_mm2(), PI * 100.0, TOL);
    }

    #[test]
    fn circle_bounds_include_arc_extremes() {
        let b = Polyline::circle(Point::new(5.0, 5.0), 10.0)
            .bounds()
            .unwrap();
        assert_approx_eq!(b.min.x, -5.0, TOL);
        assert_approx_eq!(b.min.y, -5.0, TOL);
        assert_approx_eq!(b.max.x, 15.0, TOL);
        assert_approx_eq!(b.max.y, 15.0, TOL);
    }

    #[test]
    fn reversed_circle_is_clockwise_and_same_shape() {
        let c = Polyline::circle(Point::new(0.0, 0.0), 10.0);
        let r = c.reversed();
        assert_approx_eq!(r.signed_area_mm2(), -PI * 100.0, TOL);
        let b = r.bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 20.0, TOL);
        assert_approx_eq!(b.height_mm(), 20.0, TOL);
        assert_eq!(r.reversed(), c);
    }

    #[test]
    fn point_at_tangent_is_unit_and_along_segment() {
        // Line segment: (0,0) → (100,0), then a CCW quarter arc to (110,10)
        // about (100,10), then lines back.
        let quarter = (PI / 8.0).tan();
        let p = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::new(100.0, 0.0, quarter),
                Vertex::line(110.0, 10.0),
                Vertex::line(0.0, 10.0),
            ],
            true,
        );

        let (pt, tan) = p.point_at(30.0).unwrap();
        assert_approx_eq!(tan.length(), 1.0, TOL);
        assert_approx_eq!(pt.x, 30.0, TOL);
        assert_approx_eq!(pt.y, 0.0, TOL);
        assert_approx_eq!(tan.x, 1.0, TOL);
        assert_approx_eq!(tan.y, 0.0, TOL);

        // Halfway round the arc: 45° from the start angle of −90°.
        let arc_len = PI / 2.0 * 10.0;
        let (pt, tan) = p.point_at(100.0 + arc_len / 2.0).unwrap();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert_approx_eq!(tan.length(), 1.0, TOL);
        assert_approx_eq!(pt.x, 100.0 + 10.0 * s, TOL);
        assert_approx_eq!(pt.y, 10.0 - 10.0 * s, TOL);
        // CCW travel: tangent is the radius rotated +90°.
        assert_approx_eq!(tan.x, s, TOL);
        assert_approx_eq!(tan.y, s, TOL);
        // Tangent is perpendicular to the radius.
        assert_approx_eq!((pt - Point::new(100.0, 10.0)).dot(tan), 0.0, TOL);
    }

    #[test]
    fn point_at_wraps_on_closed_and_clamps_on_open() {
        let rect = Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0);
        let (pt, _) = rect.point_at(310.0).unwrap();
        assert_approx_eq!(pt.x, 10.0, TOL);
        assert_approx_eq!(pt.y, 0.0, TOL);

        let open = Polyline::new(rect.vertices().to_vec(), false);
        let (pt, _) = open.point_at(1000.0).unwrap();
        assert_approx_eq!(pt.x, 0.0, TOL);
        assert_approx_eq!(pt.y, 50.0, TOL);
    }

    #[test]
    fn clockwise_arc_tangent_points_along_travel() {
        let p = Polyline::new(
            vec![Vertex::new(10.0, 0.0, -1.0), Vertex::line(-10.0, 0.0)],
            false,
        );
        // CW half circle from (10,0) to (-10,0) passes through (0,-10).
        let (pt, tan) = p.point_at(PI * 5.0).unwrap();
        assert_approx_eq!(pt.x, 0.0, TOL);
        assert_approx_eq!(pt.y, -10.0, TOL);
        assert_approx_eq!(tan.x, -1.0, TOL);
        assert_approx_eq!(tan.y, 0.0, TOL);
    }

    #[test]
    fn polyline_round_trips_through_json() {
        let c = Polyline::circle(Point::new(1.0, 2.0), 3.0);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Polyline>(&json).unwrap(), c);
    }
}
