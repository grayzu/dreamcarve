// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The flat floor of a V-Carve (`docs/specs/phase-3/design.md`,
//! "Algorithms → V-Carve generator", step 6; functional capability 39).

use dc_project::{Project, VCarveOp};

/// [`super::floor_ridge_height_mm`].
pub(crate) fn ridge_height_mm(_project: &Project, _op: &VCarveOp) -> Option<f64> {
    None
}
