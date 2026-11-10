// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The file I/O both DreamCarve binaries share and the `dc-*` crates may
//! not do (`docs/specs/phase-4/design.md`, "Overview" and "Module
//! layout"): the staged write that puts every file of an export in place
//! or none ([`staged`]), the user config directory ([`config_dir`]),
//! `config.toml` ([`config`]), the app's `gui.toml` ([`gui_prefs`]) and
//! the tool library file ([`library`]), today's date for a program header
//! ([`clock`]) and the check that a file is a DreamCarve preview before an
//! export writes over it ([`is_preview`]).
//! Moved from the `dreamcarve` CLI, which keeps printing and exit codes;
//! errors here are [`ShellError`]s, which the CLI prints as it printed its
//! own.
//!
//! Not a `dc-*` crate: it reads and writes files and the environment. It
//! still prints nothing and never panics.

pub mod clock;
pub mod config;
pub mod config_dir;
mod error;
pub mod gui_prefs;
pub mod library;
pub mod staged;

pub use config::{Config, ExportConfig};
pub use config_dir::Paths;
pub use error::ShellError;
pub use gui_prefs::GuiPrefs;
pub use staged::{StagedFile, commit_all, is_preview};
