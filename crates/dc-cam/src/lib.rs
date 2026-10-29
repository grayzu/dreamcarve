// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Toolpath generators for DreamCarve. Pure functions from a project and an
//! operation to a toolpath plus warnings (`docs/specs/phase-1/design.md`,
//! "dc-cam"). Phase 1 adds Profile, with tabs. Phase 2 moves every
//! generator onto cut blocks ([`CutBlock`]) that one shared step,
//! [`sequence`](fn@sequence), orders and links (`docs/specs/phase-2/design.md`,
//! "Algorithms → Sequence"), and adds [`order::check`], which warns when the
//! order of operations cuts inside a part an earlier one freed. Phase 2
//! also adds Pocket ([`generate_pocket`]), Drill ([`generate_drill`]),
//! Bore ([`generate_bore`], format 18) and Engrave ([`generate_engrave`]),
//! and entries, leads and corner relief. Phase 3 adds V-Carve
//! ([`generate_vcarve`], format 20), whose result is a list of
//! [`ToolRun`]s.
//! Tabs are placed on the project's outlines ([`placed_tabs`], #105,
//! ADR-0005), and every Profile that cuts a tab's outline rises over it.
//!
//! Toolpaths use the project's absolute frame: Y-up, Z-up, millimetres
//! (ADR-0001), stock bottom at Z = 0 and stock top at Z = thickness.

mod block;
mod bore;
mod drill;
mod engrave;
pub mod entry;
mod error;
mod footprint;
mod lead;
pub mod order;
mod overlap;
pub mod passes;
pub mod placed_tabs;
pub mod pocket;
mod profile;
mod relief;
mod run;
mod sequence;
mod stock;
mod tabs;
mod toolpath;
pub mod vcarve;
mod warning;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use block::CutBlock;
pub use bore::{BORE_MIN_HELIX_RADIUS_MM, generate_bore};
pub use drill::{CannedKind, DrillHole, DrillToolpath, PECK_REENTRY_MM, generate_drill};
pub use engrave::generate_engrave;
pub use error::{BlockProblem, CamError, ClearingProblem, FeedField, ShapeRef};
pub use footprint::footprint;
pub use pocket::generate_pocket;
pub use profile::{Wall, generate_profile, last_pass_step_reach_mm, traversal_is_ccw};
pub use run::{RunLabel, ToolRun};
pub use sequence::{SequenceInput, clearance_z_mm, plan, sequence};
pub use stock::{OVERHANG_TOLERANCE_MM, Overhang, stock_overhang, swept_bounds};
pub use tabs::{TAB_CORNER_TURN_DEG, tab_centres_mm};
pub use toolpath::{Move, MoveKind, Toolpath};
pub use vcarve::generate_vcarve;
pub use warning::{
    IslandNeighbour, LeadDroppedReason, OpenPathReason, ReliefCornerReason, ReliefSkippedReason,
    TabOnCornerReason, TabsRangeReason, Warning, WarningCode,
};
