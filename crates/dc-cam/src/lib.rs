// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Toolpath generators for DreamCarve. Pure functions from a project and an
//! operation to a toolpath plus warnings (`docs/specs/phase-1/design.md`,
//! "dc-cam"). Phase 1 adds Profile, with holding tabs. Phase 2 moves every
//! generator onto cut blocks ([`CutBlock`]) that one shared step,
//! [`sequence`], orders and links (`docs/specs/phase-2/design.md`,
//! "Algorithms → Sequence"), and adds [`order::check`], which warns when the
//! order of operations cuts inside a part an earlier one freed.
//!
//! Toolpaths use the project's absolute frame: Y-up, Z-up, millimetres
//! (ADR-0001), stock bottom at Z = 0 and stock top at Z = thickness.

mod block;
mod engrave;
mod error;
mod footprint;
pub mod order;
mod overlap;
pub mod passes;
pub mod pocket;
mod profile;
mod sequence;
mod stock;
mod tabs;
mod toolpath;
mod warning;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use block::CutBlock;
pub use engrave::generate_engrave;
pub use error::{BlockProblem, CamError, FeedField, ShapeRef};
pub use footprint::footprint;
pub use pocket::generate_pocket;
pub use profile::{Wall, generate_profile, traversal_is_ccw};
pub use sequence::{SequenceInput, plan, sequence};
pub use stock::{OVERHANG_TOLERANCE_MM, Overhang, stock_overhang, swept_bounds};
pub use tabs::tab_centres_mm;
pub use toolpath::{Move, MoveKind, Toolpath};
pub use warning::{Warning, WarningCode};
