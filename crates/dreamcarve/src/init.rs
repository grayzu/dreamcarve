// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve init`.

use std::process::ExitCode;

use crate::cli::{Failure, InitArgs};

/// Runs `init`.
pub fn run(_args: &InitArgs) -> Result<ExitCode, Failure> {
    Err(Failure::error("not implemented"))
}
