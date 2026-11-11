// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! JPEG decoding, with `zune-jpeg`.

use super::Decoded;
use crate::ImportError;

/// Decodes a JPEG picture.
pub(super) fn decode(_bytes: &[u8]) -> Result<Decoded, ImportError> {
    Err(ImportError::ImageDecode {
        reason: "not built yet".into(),
    })
}
