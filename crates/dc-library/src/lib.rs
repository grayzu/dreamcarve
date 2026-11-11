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
//! file, [`save`] writes it, [`add`], [`update`], [`find`] and [`remove`]
//! edit a library in memory. The shells own the file at `<config dir>/tools.json`
//! (`dreamcarve-shell` reads and writes it for the CLI and the app).
//! Fusion 360 import is [`fusion`]; [`import`] reads either kind of file
//! into the library with the per-tool report both shells show, and
//! [`project_copy`] is the copy `tools add` puts in a project.

pub mod error;
pub mod fusion;
pub mod import;
pub mod library;

pub use error::LibraryError;
pub use library::{
    AddOutcome, LIBRARY_VERSION, Library, LibraryTool, LibraryToolId, add, find, load, next_id,
    next_project_tool_id, project_copy, remove, replace, save, update, validate_tool,
};
