// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Geometry primitives for DreamCarve. Y-up, Z-up, millimetres (ADR-0001).
//!
//! Defined by `docs/specs/phase-1/design.md`, "Data model → dc-geometry".
//! Polylines carry arcs as DXF-style bulges; closed outer contours are
//! counter-clockwise and holes clockwise.

mod contour;
mod flatten;
mod offset;
mod point;
mod polyline;
mod region;
mod transform;

pub use contour::Contour;
pub use flatten::{FLATTEN_TOLERANCE_DEFAULT_MM, flatten};
pub use offset::offset;
pub use point::{EPS_MM, Point, Point3, Rect, Vec2, approx_eq};
pub use polyline::{Arc, Polyline, Segment, Vertex};
pub use region::{Region, nest};

/// Something the caller should know about that still gives a usable
/// result. Indices refer to positions in the caller's input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeometryWarning {
    /// Contour `index` is the same shape as contour `duplicate_of` (an
    /// earlier input) and was dropped, so it is not cut twice.
    DuplicateContourDropped {
        /// Input position of the dropped contour.
        index: usize,
        /// Input position of the contour that was kept.
        duplicate_of: usize,
    },
}

impl std::fmt::Display for GeometryWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateContourDropped {
                index,
                duplicate_of,
            } => write!(
                f,
                "shape {index} duplicates shape {duplicate_of} and was dropped"
            ),
        }
    }
}
pub use transform::Transform;

/// Why a polyline cannot be used as a [`Contour`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegenerateReason {
    /// The polyline is open; a contour must be closed.
    Open,
    /// Fewer than two vertices, so no segment encloses anything.
    TooFewVertices,
    /// The enclosed area is zero within [`EPS_MM`] squared.
    ZeroArea,
    /// A transform with zero determinant collapsed the shape.
    SingularTransform,
}

impl std::fmt::Display for DegenerateReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Open => "the polyline is open",
            Self::TooFewVertices => "the polyline has fewer than two vertices",
            Self::ZeroArea => "the polyline encloses no area",
            Self::SingularTransform => "the transform collapses the shape to a line or point",
        })
    }
}

/// Errors from geometry construction and operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GeometryError {
    /// The shape is degenerate and cannot be used.
    #[error("degenerate geometry: {0}")]
    Degenerate(DegenerateReason),
    /// The contour crosses itself.
    #[error("contour intersects itself")]
    SelfIntersecting,
    /// Two closed contours overlap without one containing the other, so
    /// neither part-and-hole nor separate parts describes them. Indices are
    /// input positions, `first` < `second`.
    #[error(
        "shapes {first} and {second} cross each other; separate them or merge them into one shape"
    )]
    ContoursCross {
        /// Input position of the earlier contour.
        first: usize,
        /// Input position of the later contour.
        second: usize,
    },
    /// An offset by `delta_mm` consumed the whole contour.
    #[error("offset by {delta_mm} mm leaves nothing of the contour")]
    OffsetVanished {
        /// The requested offset distance, millimetres.
        delta_mm: f64,
    },
    /// A non-uniform transform would turn a circular arc into an ellipse,
    /// which a bulge cannot represent.
    #[error("transform does not preserve circular arcs (non-uniform scale or shear)")]
    ArcUnderNonUniformTransform,
}
