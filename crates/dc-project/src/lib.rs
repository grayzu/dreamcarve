// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project data model and `.dcv` serialization for DreamCarve (ADR-0002).
//!
//! Defined by `docs/specs/phase-1/design.md`, "Data model → dc-project".
//! A [`Project`] is saved with [`save`] and read back with [`load`], which
//! migrates older files to [`CURRENT_VERSION`] and runs [`validate`](fn@validate).

mod countersink;
mod history;
mod ids;
mod inlay;
mod layout;
mod locate;
mod migrate;
mod model;
mod post_processor;
mod project;
mod serialize;
mod units_input;
mod validate;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use countersink::{COUNTERSINK_TOLERANCE_MM, countersink_depth_mm};
pub use history::{History, MAX_UNDO_STEPS};
pub use ids::{DefinitionId, InstanceId, OperationId, TabId, ToolId};
pub use inlay::{BandEdge, EdgePass, MaleBand, edge_pass, male_band};
pub use locate::{PathSegment, SourceMap, TextPosition};
pub use migrate::{CURRENT_VERSION, migrate};
pub use model::{
    BoreOp, ClearanceStrategy, ClearingPass, CornerRelief, CutDirection, DrillCycle, DrillOp,
    EngraveOp, Entry, FeatureDefinition, FeatureInstance, Heights, InlayCutOut, InlayOp, InlaySide,
    LastPass, Lead, MachineOrigin, MachiningOrder, Operation, OriginXy, OriginZ, ParkXy,
    PeckRetract, PlacedTab, PocketOp, ProfileOp, Project, Side, StartAt, Stock, TabPlacement,
    TabShape, TabSpec, Tool, ToolKind, Units, VCarveOp, VCarveStrategy, WallPass,
};
pub use post_processor::{
    ArcFormat, Axis, BundledDefinition, CannedCycles, Coolant, CoordinateSystem, DecimalPlaces,
    DwellUnits, FeedSpeed, MAX_FILE_EXTENSION_CHARS, Motion, NumberFormat, PostProcessor,
    PostProcessorError, PostProcessorRef, ProgramFormat, ResetReason, RetractMode, SettingReset,
    SpindleControl, SpindleDwell, Stop, ToolChange, ToolChangeMode, UnitCommands, WorkCoordinates,
    bundled_definition, bundled_definitions, is_m6_line, is_safe_file_extension, latest_definition,
    latest_definitions, switch_post,
};
pub use project::{ClearedPlane, cleared_start_plane};
pub use serialize::{LoadError, Loaded, SaveError, load, save};
pub use units_input::{LENGTH_DECIMALS, RATE_DECIMALS, round_length_mm, round_rate_mm_min};
pub use validate::{
    INLAY_MIN_PLUG_BASE_MM, MAX_FLATTEN_TOLERANCE_MM, MAX_RATE_MM_MIN, MAX_RPM,
    MAX_TOOL_DIAMETER_MM, Problem, Severity, validate,
};
