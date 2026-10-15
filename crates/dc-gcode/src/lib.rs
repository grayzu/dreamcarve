// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Post-processor model and G-code emission for DreamCarve. Phase 1 adds grblHAL and GRBL.
//!
//! The [`PostProcessor`] type itself lives in `dc-project`, which embeds it in
//! every project; this crate bundles the definitions and emits G-code.

mod error;
pub mod post;

pub use dc_project::PostProcessor;
pub use error::ExportError;
