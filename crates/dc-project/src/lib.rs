// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project data model and `.dcv` serialization for DreamCarve (ADR-0002). Phase 1 adds the model, versioning, and migrations.

mod post_processor;

pub use post_processor::{
    ArcFormat, Axis, CannedCycles, Coolant, CoordinateSystem, DecimalPlaces, FeedSpeed, Motion,
    NumberFormat, PostProcessor, ProgramFormat, RetractMode, Stop, ToolChange, Units,
    WorkCoordinates,
};
