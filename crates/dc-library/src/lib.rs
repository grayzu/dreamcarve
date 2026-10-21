// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The tool library: the user's tools, kept outside any project
//! (`docs/specs/phase-2/design.md`, "Data model → dc-library";
//! `docs/specs/phase-2/functional.md`, capability 21).
//!
//! A [`Library`] is a list of [`LibraryTool`]s, each a [`dc_project::Tool`]
//! under a library id (`lt<n>`) with an optional `source` saying where it
//! came from. Projects keep embedding copies of their tools (ADR-0002):
//! `dreamcarve tools add` copies a library tool into a project under a
//! fresh project id, and a later library edit never changes the project.
//!
//! The crate takes bytes and returns values: [`load`] parses the library
//! file, [`save`] writes it, [`add`], [`find`] and [`remove`] edit a
//! library in memory. The `dreamcarve` binary owns the file at
//! `<config dir>/tools.json`. Fusion 360 import is [`fusion`].

pub mod error;
pub mod fusion;
pub mod library;

pub use error::LibraryError;
pub use library::{
    AddOutcome, LIBRARY_VERSION, Library, LibraryTool, LibraryToolId, add, find, load, remove,
    save, validate_tool,
};
