// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The one error type of the crate (`docs/rust-guidelines.md`, "Errors and
//! warnings").

use crate::library::LibraryToolId;

/// Why a library could not be read, written or edited.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LibraryError {
    /// The bytes are not valid JSON. NaN and infinite numbers fail here,
    /// since JSON cannot express them.
    #[error("library is not valid JSON at line {line}, column {column}: {message}")]
    Json {
        /// 1-based line of the error.
        line: usize,
        /// 1-based column of the error.
        column: usize,
        /// The parser's description.
        message: String,
    },
    /// The JSON root is not an object.
    #[error("library is not a DreamCarve tool library: the top level is not a JSON object")]
    NotAnObject,
    /// The root object has no `version` field.
    #[error("library has no `version` field")]
    MissingVersion,
    /// `version` is not a positive integer that fits in 32 bits.
    #[error("library version `{found}` is not a valid version number")]
    InvalidVersion {
        /// The `version` value as written.
        found: String,
    },
    /// The library was written by a newer DreamCarve.
    #[error("library version {found} is newer than this build supports (version {supported})")]
    NewerVersion {
        /// The file's version.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// The JSON does not match the library schema (unknown or missing
    /// field, wrong type).
    #[error("library does not match the tool library format at `{path}`: {message}")]
    Schema {
        /// Where in the library the mismatch is, as keys and indices:
        /// `tools[0].tool.flutes`. Empty for the top level.
        path: String,
        /// serde's description, naming the offending field.
        message: String,
    },
    /// A tool breaks one of the validation rules (module docs of
    /// [`crate::library`]).
    #[error("tool `{name}` (`{id}`): {field} {message}")]
    InvalidTool {
        /// The tool's library id.
        id: LibraryToolId,
        /// The tool's display name.
        name: String,
        /// The field within the tool, such as `diameter_mm`.
        field: String,
        /// What is wrong with it, such as `must be greater than 0, got -1`.
        message: String,
    },
    /// A tool's number is NaN or infinite.
    #[error("tool `{name}` (`{id}`): {field} is not a finite number")]
    NonFinite {
        /// The tool's library id.
        id: LibraryToolId,
        /// The tool's display name.
        name: String,
        /// The field within the tool.
        field: String,
    },
    /// Two library tools share an id.
    #[error("two library tools have the id `{id}`")]
    DuplicateId {
        /// The repeated id.
        id: LibraryToolId,
    },
    /// No tool has `key` as its id or name.
    #[error("no library tool has the id or name `{key}`")]
    NotFound {
        /// The id or name looked for.
        key: String,
    },
    /// More than one tool has `key` as its name.
    #[error("{} library tools are named `{key}`: {}; use the id", matches.len(), matches.join(", "))]
    Ambiguous {
        /// The name looked for.
        key: String,
        /// The tools that have it, each as `` `id` ``.
        matches: Vec<String>,
    },
    /// The serializer failed.
    #[error("library could not be serialized: {message}")]
    Serialize {
        /// serde's description.
        message: String,
    },
    /// The bytes are a Fusion 360 tool library, which this build cannot
    /// read yet (`docs/specs/phase-2/design.md`, "Slices" item 21).
    #[error("this is a Fusion 360 tool library; Fusion import lands in slice 21")]
    FusionNotSupported,
}
