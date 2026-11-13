// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Image prep of a DXF (#669). Not yet written: the tests come first.

use super::run::{PrepOptions, Prepared};
use crate::{ImportError, ImportWarning};

/// Prepares the DXF in `bytes`, as [`super::prepare`] prepares an SVG.
///
/// # Errors
///
/// Always, for now: [`ImportError::PrepNothingLeft`].
pub fn prepare_dxf(
    bytes: &[u8],
    options: &PrepOptions,
) -> Result<(Prepared, Vec<ImportWarning>), ImportError> {
    let _ = (bytes, options);
    Err(ImportError::PrepNothingLeft)
}
