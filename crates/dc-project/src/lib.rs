// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project data model and `.dcv` serialization for DreamCarve (ADR-0002).
//!
//! Defined by `docs/specs/phase-1/design.md`, "Data model → dc-project".
//! A [`Project`] is saved with [`save`] and read back with [`load`], which
//! migrates older files to [`CURRENT_VERSION`] and runs [`validate`].

mod ids;
mod migrate;
mod model;
mod post_processor;
mod serialize;
mod validate;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use ids::{DefinitionId, InstanceId, OperationId, ToolId};
pub use migrate::{CURRENT_VERSION, migrate};
pub use model::{
    CutDirection, FeatureDefinition, FeatureInstance, MachineOrigin, Operation, OriginXy, OriginZ,
    ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolKind, Units,
};
pub use post_processor::{
    ArcFormat, Axis, CannedCycles, Coolant, CoordinateSystem, DecimalPlaces, FeedSpeed, Motion,
    NumberFormat, PostProcessor, ProgramFormat, RetractMode, Stop, ToolChange, UnitCommands,
    WorkCoordinates,
};
pub use serialize::{LoadError, Loaded, SaveError, load, save};
pub use validate::{Problem, Severity, validate};
