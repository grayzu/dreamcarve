// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type.

/// Why G-code could not be exported.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// No bundled post-processor has this id.
    #[error("post-processor `{id}` is not bundled")]
    UnknownPostProcessor {
        /// The id that was asked for.
        id: String,
    },
    /// A bundled definition file failed to parse. Only a broken build can
    /// produce this; `post::tests::bundled_definitions_validate` guards it.
    #[error("bundled post-processor definition `{file}` is invalid")]
    BundledDefinition {
        /// File name under `definitions/`.
        file: &'static str,
        /// The parse failure.
        #[source]
        source: serde_json::Error,
    },
}
