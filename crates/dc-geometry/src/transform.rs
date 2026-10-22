// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! 2D affine transforms of points and polylines (`docs/specs/phase-1/design.md`,
//! "Data model → dc-geometry"). Feature instances place their definition's
//! profiles with one of these.

use crate::point::{EPS_MM, MAX_COORDINATE_MM, Point};
use crate::polyline::{Polyline, Vertex};
use crate::{DegenerateReason, GeometryError};
use serde::{Deserialize, Serialize};

/// Smallest determinant of a transform's linear part that is not
/// singular. The determinant is the factor areas scale by, so it has no
/// unit; below this a unit square shrinks to less than a square
/// [`EPS_MM`] on a side, mm², which is as good as nothing.
const MIN_DETERMINANT: f64 = EPS_MM * EPS_MM;

/// A 2D affine transform in millimetres. Serialized as the six coefficients
/// `[a, b, c, d, e, f]` mapping (x, y) to (a·x + c·y + e, b·x + d·y + f).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 6]", into = "[f64; 6]")]
pub struct Transform(kurbo::Affine);

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<[f64; 6]> for Transform {
    fn from(coeffs: [f64; 6]) -> Self {
        Self(kurbo::Affine::new(coeffs))
    }
}

impl From<Transform> for [f64; 6] {
    fn from(t: Transform) -> Self {
        t.0.as_coeffs()
    }
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Transform = Transform(kurbo::Affine::IDENTITY);

    /// A translation by (`dx_mm`, `dy_mm`).
    #[must_use]
    pub fn translate(dx_mm: f64, dy_mm: f64) -> Self {
        Self(kurbo::Affine::translate((dx_mm, dy_mm)))
    }

    /// A counter-clockwise rotation about the origin by `angle_rad`.
    #[must_use]
    pub fn rotate(angle_rad: f64) -> Self {
        Self(kurbo::Affine::rotate(angle_rad))
    }

    /// A scale about the origin by `sx` and `sy`; dimensionless factors.
    #[must_use]
    pub fn scale(sx: f64, sy: f64) -> Self {
        Self(kurbo::Affine::scale_non_uniform(sx, sy))
    }

    /// `self` applied first, then `next`.
    #[must_use]
    pub fn then(self, next: Transform) -> Self {
        Self(next.0 * self.0)
    }

    /// The six coefficients `[a, b, c, d, e, f]`, as serialized: (x, y)
    /// maps to (a·x + c·y + e, b·x + d·y + f), `e` and `f` in millimetres.
    #[must_use]
    pub fn coeffs(self) -> [f64; 6] {
        self.0.as_coeffs()
    }

    /// The transform that undoes this one, or `None` when it is singular
    /// (its determinant below the area of a square [`EPS_MM`] on a side).
    #[must_use]
    pub fn inverse(self) -> Option<Transform> {
        let [a, b, c, d, _, _] = self.0.as_coeffs();
        let det = a * d - b * c;
        (det.is_finite() && det.abs() >= MIN_DETERMINANT).then(|| Self(self.0.inverse()))
    }

    /// The transformed point.
    #[must_use]
    pub fn apply(self, p: Point) -> Point {
        let q = self.0 * kurbo::Point::new(p.x, p.y);
        Point::new(q.x, q.y)
    }

    /// The transformed polyline. Arcs stay arcs under rotation, uniform
    /// scale, translation, and mirroring; a mirror negates every bulge.
    /// Fails when the transform is singular, or when the polyline has arcs
    /// and the transform scales non-uniformly or shears, or when the result
    /// reaches past [`MAX_COORDINATE_MM`] or is not finite.
    pub fn apply_polyline(self, polyline: &Polyline) -> Result<Polyline, GeometryError> {
        let [a, b, c, d, _, _] = self.0.as_coeffs();
        let det = a * d - b * c;
        if det.abs() < MIN_DETERMINANT {
            return Err(GeometryError::Degenerate(
                DegenerateReason::SingularTransform,
            ));
        }
        // Rotation × uniform scale is [[a, -b], [b, a]]; its mirror is
        // [[a, b], [b, -a]]. Anything else maps circles to ellipses.
        let scale = a.abs().max(b.abs()).max(c.abs()).max(d.abs());
        let tol = scale * 1e-9;
        let conformal = if det > 0.0 {
            (a - d).abs() <= tol && (b + c).abs() <= tol
        } else {
            (a + d).abs() <= tol && (b - c).abs() <= tol
        };
        let has_arcs = polyline.segments().any(|s| !s.is_line());
        if has_arcs && !conformal {
            return Err(GeometryError::ArcUnderNonUniformTransform);
        }
        let bulge_sign = if det > 0.0 { 1.0 } else { -1.0 };
        let vertices = polyline
            .vertices()
            .iter()
            .map(|v| {
                let p = self.apply(v.point());
                Vertex::new(p.x, p.y, v.bulge * bulge_sign)
            })
            .collect();
        let out = Polyline::new(vertices, polyline.is_closed());
        let reach_mm = out.reach_mm();
        if reach_mm > MAX_COORDINATE_MM {
            return Err(GeometryError::CoordinateOutOfRange {
                reach_mm,
                limit_mm: MAX_COORDINATE_MM,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;

    use std::f64::consts::PI;

    /// #105: a tab's point goes into definition space and back unchanged.
    #[test]
    fn inverse_undoes_the_transform() {
        let t = Transform::scale(-2.0, 2.0)
            .then(Transform::rotate(0.7))
            .then(Transform::translate(12.5, -3.0));
        let p = Point::new(3.25, -7.5);
        let back = t.inverse().unwrap().apply(t.apply(p));
        assert_approx_eq!(back.x, p.x, 1e-12);
        assert_approx_eq!(back.y, p.y, 1e-12);
        assert_eq!(Transform::scale(1.0, 0.0).inverse(), None);
    }

    #[test]
    fn translate_then_rotate_composes_in_order() {
        let t = Transform::translate(10.0, 0.0).then(Transform::rotate(PI / 2.0));
        let p = t.apply(Point::new(0.0, 0.0));
        assert_approx_eq!(p.x, 0.0, 1e-12);
        assert_approx_eq!(p.y, 10.0, 1e-12);
    }

    #[test]
    fn mirror_negates_bulges_and_keeps_area_magnitude() {
        let c = Polyline::circle(Point::new(0.0, 0.0), 5.0);
        let m = Transform::scale(-1.0, 1.0).apply_polyline(&c).unwrap();
        assert!(m.vertices().iter().all(|v| v.bulge < 0.0));
        assert_approx_eq!(m.signed_area_mm2(), -c.signed_area_mm2(), 1e-9);
    }

    #[test]
    fn non_uniform_scale_of_arcs_is_an_error() {
        let c = Polyline::circle(Point::new(0.0, 0.0), 5.0);
        assert_eq!(
            Transform::scale(2.0, 1.0).apply_polyline(&c),
            Err(GeometryError::ArcUnderNonUniformTransform)
        );
    }

    #[test]
    fn non_uniform_scale_of_lines_is_fine() {
        let r = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        let s = Transform::scale(2.0, 1.0).apply_polyline(&r).unwrap();
        assert_approx_eq!(s.signed_area_mm2(), 200.0, 1e-9);
    }

    /// #59: a scale that overflows to infinity is an error, not a polyline
    /// that panics an offset later.
    #[test]
    fn transform_overflow_is_an_error() {
        let r = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        assert!(matches!(
            Transform::scale(1e308, 1e308).apply_polyline(&r),
            Err(GeometryError::CoordinateOutOfRange { .. })
        ));
    }

    /// #59: a result exactly at [`MAX_COORDINATE_MM`] is accepted; one
    /// just past it is an error that names the reach and the limit.
    #[test]
    fn transform_past_coordinate_limit_is_an_error() {
        let r = Polyline::rect(Point::new(0.0, 0.0), 1.0, 1.0);
        let at_limit = Transform::translate(MAX_COORDINATE_MM - 1.0, 0.0);
        assert!(at_limit.apply_polyline(&r).is_ok());
        let past = Transform::translate(MAX_COORDINATE_MM - 0.5, 0.0);
        match past.apply_polyline(&r) {
            Err(GeometryError::CoordinateOutOfRange { reach_mm, limit_mm }) => {
                assert_approx_eq!(reach_mm, MAX_COORDINATE_MM + 0.5, 1e-9);
                assert_approx_eq!(limit_mm, MAX_COORDINATE_MM, 0.0);
            }
            other => panic!("expected CoordinateOutOfRange, got {other:?}"),
        }
    }

    /// #59: an arc can bulge past the limit while its vertices stay
    /// inside; the check covers arc extremes too.
    #[test]
    fn arc_bulging_past_coordinate_limit_is_an_error() {
        let c = Polyline::circle(Point::new(0.0, 0.0), 1.0);
        let up = Transform::translate(0.0, MAX_COORDINATE_MM - 0.5);
        assert!(matches!(
            up.apply_polyline(&c),
            Err(GeometryError::CoordinateOutOfRange { .. })
        ));
    }

    #[test]
    fn serializes_as_six_coefficients() {
        let t = Transform::translate(3.0, 4.0);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "[1.0,0.0,0.0,1.0,3.0,4.0]");
        assert_eq!(serde_json::from_str::<Transform>(&json).unwrap(), t);
    }
}
