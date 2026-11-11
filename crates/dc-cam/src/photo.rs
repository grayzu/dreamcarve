// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Photo V-Carve (`docs/specs/phase-3/design.md`, "Algorithms → Photo
//! V-Carve"). Not built yet: the tests come first (#646).

use crate::error::{CamError, PhotoProblem};
use crate::toolpath::{Move, Toolpath};
use crate::warning::Warning;
use dc_geometry::{Point, Rect, Vec2};
use dc_project::{Bitmap, Grayscale, OperationId, PhotoVCarveOp, Project};

/// The most samples a Photo V-Carve's grooves may hold (security review
/// L3).
pub const MAX_PHOTO_MOVES: usize = 2_000_000;

/// One family of grooves.
#[derive(Debug, Clone, PartialEq)]
pub struct Grooves {
    /// The family's direction.
    pub direction: Vec2,
    /// The spacing, mm.
    pub spacing_mm: f64,
    /// The grooves.
    pub lines: Vec<Vec<Point>>,
}

/// What `check` prints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The footprint.
    pub footprint: Rect,
    /// The groove count.
    pub grooves: usize,
    /// The full-depth width, mm.
    pub width_mm: f64,
}

/// Not built yet.
pub fn grooves(
    op: &OperationId,
    _footprint: Rect,
    _angle_deg: f64,
    spacing_mm: f64,
    _sample_mm: f64,
) -> Result<Grooves, CamError> {
    Err(CamError::PhotoSetting {
        op: op.clone(),
        field: "line_spacing_fraction",
        value: Some(spacing_mm),
        problem: PhotoProblem::Unset,
    })
}

/// Not built yet.
#[must_use]
pub fn sample(_bitmap: &Bitmap, _pixels: &Grayscale, _p: Point) -> f64 {
    -1.0
}

/// Not built yet.
#[must_use]
pub fn layout(_project: &Project, _op: &PhotoVCarveOp) -> Option<Layout> {
    None
}

/// Not built yet.
pub fn generate_photo(
    _project: &Project,
    op: &PhotoVCarveOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let _ = Move::rapid;
    Err(CamError::PhotoNotBuilt { op: op.id.clone() })
}

#[cfg(test)]
mod tests;
