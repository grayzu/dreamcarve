// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fusion 360 tool library import (`docs/specs/phase-2/design.md`, "Fusion
//! import (dc-library::fusion)"). The import itself is slice 21 of phase 2;
//! this slice only recognises a Fusion library so `tools import` can say
//! what it is instead of reporting it as a malformed DreamCarve library.

use crate::LibraryError;

/// Whether `bytes` look like a Fusion 360 tool library rather than a
/// DreamCarve one: a `.tools` archive (a zip, by its signature) or a
/// `.json` document whose top-level object has a `data` array, which the
/// DreamCarve format never has.
#[must_use]
pub fn is_fusion_library(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"PK\x03\x04") {
        return true;
    }
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|root| root.get("data").map(serde_json::Value::is_array))
        .unwrap_or(false)
}

/// Imports a Fusion 360 library. Always
/// [`LibraryError::FusionNotSupported`] in this build; slice 21 adds the
/// import and its result type.
pub fn import(_bytes: &[u8]) -> Result<(), LibraryError> {
    Err(LibraryError::FusionNotSupported)
}
