// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The little linear algebra the 3D view needs (phase 6 design, open
//! question 2): vectors of three, and the 4 × 4 matrices of a view and a
//! perspective, in `f64`. It is narrowed to `f32` once, where the
//! renderer takes it. The world is project space: X to the right, Y
//! away from the operator, Z up, millimetres (ADR-0001).

use std::ops::{Add, Mul, Neg, Sub};

use dc_geometry::Point3;

/// A direction or an offset in space, mm.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// Along X.
    pub x: f64,
    /// Along Y.
    pub y: f64,
    /// Along Z.
    pub z: f64,
}

impl Vec3 {
    /// The vector (`x`, `y`, `z`).
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// From `a` to `b`.
    #[must_use]
    pub fn between(a: Point3, b: Point3) -> Self {
        Self::new(b.x - a.x, b.y - a.y, b.z - a.z)
    }

    /// The dot product.
    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product, right-handed.
    #[must_use]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Its length.
    #[must_use]
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// The same direction, one long; `None` for a vector of no length.
    #[must_use]
    pub fn normalized(self) -> Option<Self> {
        let length = self.length();
        (length > 0.0 && length.is_finite()).then(|| self * (1.0 / length))
    }

    /// `p` moved by this vector.
    #[must_use]
    pub fn from(self, p: Point3) -> Point3 {
        Point3::new(p.x + self.x, p.y + self.y, p.z + self.z)
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, k: f64) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }
}

/// A 4 × 4 matrix, `m[row][column]`, applied to column vectors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    /// The view of an eye at `eye` whose right, up and forward
    /// directions are `right`, `up` and `forward`, each one long and
    /// square to the others: world to eye space, the eye looking down
    /// its −Z.
    #[must_use]
    pub fn view(eye: Point3, right: Vec3, up: Vec3, forward: Vec3) -> Self {
        let e = Vec3::new(eye.x, eye.y, eye.z);
        Self([
            [right.x, right.y, right.z, -right.dot(e)],
            [up.x, up.y, up.z, -up.dot(e)],
            [-forward.x, -forward.y, -forward.z, forward.dot(e)],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// A perspective of `fov_y_rad` up and down, `aspect` wide for its
    /// height, from `near` to `far`: eye space to clip space with depth
    /// from 0 at `near` to 1 at `far`, as wgpu reads it.
    #[must_use]
    pub fn perspective(fov_y_rad: f64, aspect: f64, near: f64, far: f64) -> Self {
        let f = 1.0 / (fov_y_rad / 2.0).tan();
        Self([
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, far / (near - far), near * far / (near - far)],
            [0.0, 0.0, -1.0, 0.0],
        ])
    }

    /// This matrix after `other`: `self · other`.
    #[must_use]
    pub fn after(&self, other: &Self) -> Self {
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.at(r, k) * other.at(k, c)).sum();
            }
        }
        Self(out)
    }

    /// The element at `row`, `column`; 0 outside the matrix.
    #[must_use]
    pub fn at(&self, row: usize, column: usize) -> f64 {
        self.0
            .get(row)
            .and_then(|r| r.get(column))
            .copied()
            .unwrap_or(0.0)
    }

    /// `p` through the matrix, with its `w`: clip space for a view and
    /// perspective.
    #[must_use]
    pub fn apply(&self, p: Point3) -> [f64; 4] {
        let v = [p.x, p.y, p.z, 1.0];
        let mut out = [0.0; 4];
        for (r, slot) in out.iter_mut().enumerate() {
            *slot = (0..4)
                .map(|k| self.at(r, k) * v.get(k).copied().unwrap_or(0.0))
                .sum();
        }
        out
    }

    /// The matrix as the renderer takes it: `f32`, column by column.
    #[must_use]
    pub fn to_columns_f32(&self) -> [[f32; 4]; 4] {
        let mut out = [[0.0_f32; 4]; 4];
        for (c, column) in out.iter_mut().enumerate() {
            for (r, cell) in column.iter_mut().enumerate() {
                // The one place the view leaves `f64`: the GPU's edge.
                #[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
                {
                    *cell = self.at(r, c) as f32;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    #[test]
    fn cross_is_right_handed() {
        let (x, y, z) = (
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        assert_eq!(x.cross(y), z);
        assert_eq!(y.cross(z), x);
        assert_eq!(z.cross(x), y);
        assert_approx_eq!(Vec3::new(3.0, 4.0, 12.0).length(), 13.0, 1e-12);
        assert_eq!(Vec3::default().normalized(), None);
    }

    /// An eye 10 mm in front of the origin, looking along +Y with Z up:
    /// the origin is straight ahead, 10 mm deep; a point to its right is
    /// to the right on screen, one above it is up.
    #[test]
    fn a_view_puts_what_is_ahead_at_the_centre() {
        let view = Mat4::view(
            Point3::new(0.0, -10.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let ahead = view.apply(Point3::new(0.0, 0.0, 0.0));
        assert_approx_eq!(ahead[0], 0.0, 1e-12);
        assert_approx_eq!(ahead[1], 0.0, 1e-12);
        assert_approx_eq!(ahead[2], -10.0, 1e-12);
        let right_and_up = view.apply(Point3::new(2.0, 0.0, 3.0));
        assert_approx_eq!(right_and_up[0], 2.0, 1e-12);
        assert_approx_eq!(right_and_up[1], 3.0, 1e-12);
    }

    /// The near plane is depth 0 and the far plane depth 1, and a point
    /// at the edge of the field of view is at the edge of the picture.
    #[test]
    fn a_perspective_runs_from_near_to_far() {
        let p = Mat4::perspective(std::f64::consts::FRAC_PI_2, 2.0, 1.0, 100.0);
        let depth = |z: f64| {
            let clip = p.apply(Point3::new(0.0, 0.0, z));
            clip[2] / clip[3]
        };
        assert_approx_eq!(depth(-1.0), 0.0, 1e-12);
        assert_approx_eq!(depth(-100.0), 1.0, 1e-12);
        // 90° up and down: at 10 deep, 10 up is the top edge, and 20
        // across is the right edge of a picture twice as wide as high.
        let corner = p.apply(Point3::new(20.0, 10.0, -10.0));
        assert_approx_eq!(corner[0] / corner[3], 1.0, 1e-12);
        assert_approx_eq!(corner[1] / corner[3], 1.0, 1e-12);
    }

    #[test]
    fn after_composes_and_columns_are_columns() {
        let view = Mat4::view(
            Point3::new(1.0, 2.0, 3.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let p = Mat4::perspective(1.0, 1.5, 0.5, 50.0);
        let both = p.after(&view);
        let point = Point3::new(4.0, 9.0, 5.0);
        let eye = view.apply(point);
        let direct = p.apply(Point3::new(eye[0], eye[1], eye[2]));
        let composed = both.apply(point);
        for k in 0..4 {
            assert_approx_eq!(composed[k], direct[k], 1e-9);
        }
        let columns = both.to_columns_f32();
        assert_approx_eq!(f64::from(columns[3][0]), both.at(0, 3), 1e-5);
        assert_approx_eq!(f64::from(columns[0][3]), both.at(3, 0), 1e-5);
    }
}
