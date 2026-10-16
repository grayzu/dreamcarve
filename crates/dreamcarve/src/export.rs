// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve export`.

use std::process::ExitCode;

use crate::cli::{ExportArgs, Failure};

/// Runs `export`.
pub fn run(_args: &ExportArgs) -> Result<ExitCode, Failure> {
    Err(Failure::error("not implemented"))
}
