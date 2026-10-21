// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Post-processor model and G-code emission for DreamCarve. Phase 1 adds grblHAL and GRBL.
//!
//! The [`PostProcessor`] type and the bundled definitions live in
//! `dc-project`, which loads a project's reference to one (#106); this crate
//! emits G-code with the effective definition. [`emit`] writes a program
//! from toolpaths, then parses it back and checks the safety rules on the
//! text (`docs/specs/phase-1/design.md`, "Emission").

pub mod arcfit;
pub mod emit;
mod error;
pub mod format;
pub mod parse;
#[cfg(test)]
mod post;
mod safety;
pub mod transform;

pub use dc_project::PostProcessor;
pub use emit::{ExportWarning, FirstTool, Pause, Program, emit, emit_programs};
pub use error::ExportError;
pub use parse::{MachineMove, parse_motion};
pub use safety::{AxisRange, Extents, MAX_LINE_CHARS};
