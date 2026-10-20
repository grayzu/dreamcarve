// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project data model and `.dcv` serialization for DreamCarve (ADR-0002).
//!
//! Defined by `docs/specs/phase-1/design.md`, "Data model → dc-project".
//! A [`Project`] is saved with [`save`] and read back with [`load`], which
//! migrates older files to [`CURRENT_VERSION`] and runs [`validate`].

mod history;
mod ids;
mod layout;
mod locate;
mod migrate;
mod model;
mod post_processor;
mod serialize;
mod validate;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use history::{History, MAX_UNDO_STEPS};
pub use ids::{DefinitionId, InstanceId, OperationId, ToolId};
pub use locate::{PathSegment, SourceMap, TextPosition};
pub use migrate::{CURRENT_VERSION, migrate};
pub use model::{
    CutDirection, FeatureDefinition, FeatureInstance, MachineOrigin, Operation, OriginXy, OriginZ,
    ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolKind, Units,
};
pub use post_processor::{
    ArcFormat, Axis, BundledDefinition, CannedCycles, Coolant, CoordinateSystem, DecimalPlaces,
    FeedSpeed, MAX_FILE_EXTENSION_CHARS, Motion, NumberFormat, PostProcessor, PostProcessorError,
    PostProcessorRef, ProgramFormat, RetractMode, SpindleControl, SpindleDwell, Stop, ToolChange,
    ToolChangeMode, UnitCommands, WorkCoordinates, bundled_definition, bundled_definitions,
    is_m6_line, is_safe_file_extension, latest_definition,
};
pub use serialize::{LoadError, Loaded, SaveError, load, save};
pub use validate::{MAX_RATE_MM_MIN, MAX_RPM, MAX_TOOL_DIAMETER_MM, Problem, Severity, validate};
