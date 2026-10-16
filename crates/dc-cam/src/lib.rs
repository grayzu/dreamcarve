// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Toolpath generators for DreamCarve. Pure functions from a project and an
//! operation to a toolpath plus warnings (`docs/specs/phase-1/design.md`,
//! "dc-cam"). Phase 1 adds Profile, with holding tabs.
//!
//! Toolpaths use the project's absolute frame: Y-up, Z-up, millimetres
//! (ADR-0001), stock bottom at Z = 0 and stock top at Z = thickness.

mod error;
pub mod passes;
mod profile;
pub mod tab_profile;
mod tabs;
mod toolpath;
mod warning;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use error::{CamError, FeedField, ShapeRef};
pub use profile::{Wall, generate_profile, traversal_is_ccw};
pub use tabs::tab_centres_mm;
pub use toolpath::{Move, MoveKind, Toolpath};
pub use warning::{Warning, WarningCode};
