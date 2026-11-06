// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The job pipeline both DreamCarve shells drive (`docs/specs/phase-4/design.md`,
//! "Overview" and "Data model → dc-job"): from a project to its programs,
//! with every error and warning found on the way; the toolpath preview;
//! the export summary's lines; a new project from a drawing; a drawing
//! imported into a project; placed tabs. Moved down from the `dreamcarve`
//! CLI so the app calls the same functions and gets the same bytes, since
//! it has no other way to get them (functional spec, S20): the app's
//! crates depend on `dc-cam` and `dc-gcode` only through this crate, whose
//! re-exports below are the types a shell needs.
//!
//! Pure, as every `dc-*` crate: no file, terminal, clock or network I/O,
//! no panics. Problems that leave the result usable are [`Findings`];
//! those that do not are [`JobError`]. A shell reads the files, picks the
//! date, writes the programs and prints.

pub mod build;
mod error;
pub mod import;
pub mod names;
pub mod preview;
pub mod project_from;
pub mod summary;
pub mod tabs;

pub use build::{
    Built, Finding, Findings, Finished, Generated, ProgramSummary, Tier, build, build_board,
    finish, generate_operation, project_checks, validation,
};
pub use error::JobError;
pub use import::{Drawing, drawing_into};
pub use names::{program_names, program_suffixes};
pub use summary::{EstimateConfig, Estimates, describe_carves, describe_operations};

// The `dc-cam` and `dc-gcode` types a shell needs, so the app's crates
// list neither (functional spec, S20).
pub use dc_cam::{
    CamError, Move, MoveKind, RunLabel, ShapeRef, ToolRun, Toolpath, Warning, WarningCode,
};
pub use dc_gcode::{
    AxisRange, EmitOp, Estimate, ExportError, ExportWarning, Extents, Program, ProgramSplit,
};
