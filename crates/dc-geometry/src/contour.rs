// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Closed polylines with known orientation (`docs/specs/phase-1/design.md`,
//! "Data model → dc-geometry"). Outer boundaries are counter-clockwise and
//! holes clockwise (ADR-0001).

use crate::point::EPS_MM;
use crate::polyline::Polyline;
use crate::{DegenerateReason, GeometryError};

/// A closed polyline with non-zero area and known orientation.
/// [`Contour::new`] normalises to counter-clockwise; [`Contour::reversed`]
/// gives the clockwise form used for holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour(Polyline);

impl Contour {
    /// A counter-clockwise contour from a closed polyline, reversing it if
    /// it winds clockwise. Fails when the polyline is open, has fewer than
    /// two vertices, or encloses no area.
    pub fn new(polyline: Polyline) -> Result<Self, GeometryError> {
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

    #[test]
    fn single_vertex_is_rejected() {
        let one = Polyline::new(vec![Vertex::line(1.0, 1.0)], true);
        assert_eq!(
            Contour::new(one),
            Err(GeometryError::Degenerate(DegenerateReason::TooFewVertices))
        );
    }
}
