// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The window's frame (`docs/specs/phase-4/design.md`, "Module layout"):
//! the shell itself, its menu bar, toolbar and status bar, its files, and
//! the theme.

pub mod files;
pub mod focus;
pub(crate) mod menu;
pub(crate) mod status;
pub mod theme;
pub(crate) mod toolbar;
pub(crate) mod toolpaths;
pub mod window;
pub(crate) mod worker;
