// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve check`.

use std::process::ExitCode;

use crate::cli::{CheckArgs, Failure};

/// Runs `check`.
pub fn run(_args: &CheckArgs) -> Result<ExitCode, Failure> {
    Err(Failure::error("not implemented"))
}
