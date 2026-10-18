// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Points, vectors, rectangles, and the float tolerance helpers every crate
//! uses instead of `==` (`docs/rust-guidelines.md`, "Floating point").

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

/// Tolerance for treating two lengths as equal, in millimetres. Far below
/// any machine's resolution, far above f64 rounding at workpiece scale.
pub const EPS_MM: f64 = 1e-6;

/// Largest absolute X or Y coordinate, and largest stock side, DreamCarve
/// accepts, mm (6 m). It bounds the numeric range so offsets, areas and
/// spatial indexes stay far inside f64 precision and never overflow, and it
/// catches typos such as a stray exponent. It is not the machine envelope:
/// that is the controller's soft limits. The SVG importer and `.dcv`
/// validation share it, so a drawing that imports never fails validation
/// on size.
pub const MAX_COORDINATE_MM: f64 = 6_000.0;

/// True when `a` and `b` differ by at most `tolerance`.
#[must_use]
pub fn approx_eq(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// Asserts two floats are within a tolerance: `assert_approx_eq!(a, b, tol)`.
/// For tests; library code uses [`approx_eq`].
#[macro_export]
macro_rules! assert_approx_eq {
    ($a:expr, $b:expr, $tol:expr $(,)?) => {{
        let (a, b, tol): (f64, f64, f64) = ($a, $b, $tol);
        assert!(
            (a - b).abs() <= tol,
            "assert_approx_eq failed: {} = {a}, {} = {b}, difference {} > tolerance {tol}",
            stringify!($a),
            stringify!($b),
            (a - b).abs(),
        );
    }};
}

/// A 2D point in millimetres, Y-up.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// X, millimetres.
    pub x: f64,
    /// Y, millimetres, growing away from the operator.
    pub y: f64,
}

impl Point {
    /// A point at (`x`, `y`) millimetres.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to `other`, millimetres.
    #[must_use]
    pub fn distance_to(self, other: Point) -> f64 {
        (other - self).length()
    }
}

/// A 2D displacement or direction, millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
}

impl Vec2 {
    /// A vector (`x`, `y`).
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Euclidean length.
    #[must_use]
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Unit vector in the same direction, or `None` for a vector shorter
    /// than [`EPS_MM`].
    #[must_use]
    pub fn normalized(self) -> Option<Vec2> {
        let len = self.length();
        (len > EPS_MM).then(|| Vec2::new(self.x / len, self.y / len))
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, other: Vec2) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Z component of the 3D cross product; positive when `other` is
    /// counter-clockwise from `self`.
    #[must_use]
    pub fn cross(self, other: Vec2) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// The vector rotated 90° counter-clockwise (to the left of travel).
    #[must_use]
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
}

impl Sub for Point {
    type Output = Vec2;
    fn sub(self, rhs: Point) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Add<Vec2> for Point {
    type Output = Point;
    fn add(self, rhs: Vec2) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f64) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// A 3D point in millimetres, Y-up, Z-up; stock bottom is Z = 0.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point3 {
    /// X, millimetres.
    pub x: f64,
    /// Y, millimetres.
    pub y: f64,
    /// Z, millimetres.
    pub z: f64,
}

impl Point3 {
    /// A point at (`x`, `y`, `z`) millimetres.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// The XY projection.
    #[must_use]
    pub const fn xy(self) -> Point {
        Point::new(self.x, self.y)
    }
}

/// An axis-aligned rectangle in millimetres, `min` ≤ `max` on both axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Lower-left corner (Y-up).
    pub min: Point,
    /// Upper-right corner.
    pub max: Point,
}

impl Rect {
    /// The zero-size rectangle at `p`.
    #[must_use]
    pub const fn from_point(p: Point) -> Self {
        Self { min: p, max: p }
    }

    /// The smallest rectangle containing `self` and `p`.
    #[must_use]
    pub fn including(self, p: Point) -> Self {
        Self {
            min: Point::new(self.min.x.min(p.x), self.min.y.min(p.y)),
            max: Point::new(self.max.x.max(p.x), self.max.y.max(p.y)),
        }
    }

    /// Extent along X, millimetres.
    #[must_use]
    pub fn width_mm(self) -> f64 {
        self.max.x - self.min.x
    }

    /// Extent along Y, millimetres.
    #[must_use]
    pub fn height_mm(self) -> f64 {
        self.max.y - self.min.y
    }
}
