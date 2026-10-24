// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts (the `helical` drill type)
//   src/engine/toolpaths/entry.ts (`emitCenterLockedCircularBore`)

//! Bore toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Bore";
//! format 18). Not built yet: the tests come first.

use crate::error::CamError;
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_project::{BoreOp, Project};

/// The least helix radius a Bore cuts, mm.
pub const BORE_MIN_HELIX_RADIUS_MM: f64 = 0.05;

/// Generates the Bore toolpath for `op` in `project`. Not built yet.
///
/// # Errors
///
/// None yet.
pub fn generate_bore(
    _project: &Project,
    _op: &BoreOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    Ok((Toolpath::default(), Vec::new()))
}

// What the tests take from this module once it is built.
#[cfg(test)]
use crate::toolpath::{Move, MoveKind};
#[cfg(test)]
use dc_geometry::{EPS_MM, Point, Point3};
#[cfg(test)]
use std::f64::consts::{FRAC_PI_2, TAU};

#[cfg(test)]
mod tests;
