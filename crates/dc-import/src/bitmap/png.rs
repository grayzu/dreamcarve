// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! PNG decoding and the stored PNG's encoding, with the `png` crate.

use dc_project::Grayscale;

use super::Decoded;
use crate::ImportError;

/// Decodes a PNG picture.
pub(super) fn decode(_bytes: &[u8]) -> Result<Decoded, ImportError> {
    Err(ImportError::ImageDecode {
        reason: "not built yet".into(),
    })
}

/// Encodes the PNG a bitmap stores.
pub(super) fn encode(_pixels: &Grayscale) -> Result<Vec<u8>, ImportError> {
    Err(ImportError::ImageDecode {
        reason: "not built yet".into(),
    })
}
