// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve image-prep`.

use std::process::ExitCode;

use crate::cli::{Failure, ImagePrepArgs};

/// Runs `image-prep`.
pub fn run(_args: &ImagePrepArgs) -> Result<ExitCode, Failure> {
    Ok(ExitCode::SUCCESS)
}
