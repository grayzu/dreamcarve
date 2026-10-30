// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Inlay (`docs/specs/phase-3/design.md`, "Algorithms → Inlay
//! (dc-cam::inlay)"; functional capability 45, acceptance 56 and 57).

#[cfg(test)]
mod tests;

use crate::error::CamError;
use crate::run::ToolRun;
use crate::warning::Warning;
use dc_geometry::Transform;
use dc_project::{InlayOp, InstanceId, Project};

/// The male's mirror: X reflected about the targets' joint bounding box
/// centre line, in project space.
///
/// # Errors
///
/// Not yet.
pub fn male_mirror(_project: &Project, _targets: &[InstanceId]) -> Result<Transform, CamError> {
    Ok(Transform::IDENTITY)
}

/// The height of the V-bit floor's ridges, mm; not yet.
#[must_use]
pub fn floor_ridge_height_mm(_project: &Project, _op: &InlayOp) -> Option<f64> {
    None
}

/// Generates one side of an inlay; not yet.
///
/// # Errors
///
/// [`CamError::NotSupportedYet`] until #436.
#[must_use = "the toolpath is the whole point"]
pub fn generate_inlay(
    _project: &Project,
    op: &InlayOp,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    Err(CamError::NotSupportedYet {
        op: op.id.clone(),
        feature: "a V-bit inlay (`inlay`)".into(),
    })
}
