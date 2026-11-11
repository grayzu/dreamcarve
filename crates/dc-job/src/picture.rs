// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures placed in a project (`docs/specs/phase-3/design.md`, "CLI";
//! functional spec, capability 46).

use dc_geometry::Point;
use dc_project::{BitmapId, Grayscale, OperationId, Project};

use crate::error::JobError;

/// How big a picture is on the stock: one side, the other following the
/// pixels' proportions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PictureSize {
    /// The footprint's width, mm.
    Width {
        /// The width, mm.
        width_mm: f64,
    },
    /// The footprint's height, mm.
    Height {
        /// The height, mm.
        height_mm: f64,
    },
}

/// One row of `image list`.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureRow {
    /// The bitmap's id.
    pub id: BitmapId,
    /// The bitmap's name.
    pub name: String,
    /// The picture's width, pixels, from its PNG's header.
    pub width_px: u32,
    /// The picture's height, pixels, from its PNG's header.
    pub height_px: u32,
    /// X of the footprint's bottom-left corner, mm.
    pub x_mm: f64,
    /// Y of the footprint's bottom-left corner, mm.
    pub y_mm: f64,
    /// The footprint's width, mm.
    pub width_mm: f64,
    /// The footprint's height, mm.
    pub height_mm: f64,
    /// The Photo V-Carve operations that carve it.
    pub used_by: Vec<OperationId>,
}

/// `project` with the picture appended as its next bitmap.
///
/// # Errors
///
/// Not built yet.
pub fn picture_into(
    _project: &Project,
    _name: &str,
    _pixels: &Grayscale,
    _size: PictureSize,
    _corner: Point,
) -> Result<(Project, BitmapId), JobError> {
    Err(JobError::NoShapes)
}

/// The rows of `image list`.
///
/// # Errors
///
/// Not built yet.
pub fn picture_rows(_project: &Project) -> Result<Vec<PictureRow>, dc_project::LoadError> {
    Ok(Vec::new())
}
