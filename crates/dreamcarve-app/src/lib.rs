// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DreamCarve app's shell (`docs/specs/phase-4/design.md`,
//! "Architecture → The shell"): the egui window on the engine the CLI
//! drives. It owns the window, the file dialogs, the files, the
//! preferences file and the OS appearance; `dc_app::App` owns the project,
//! its history and everything the panels show. The look is
//! `docs/specs/phase-4/experience.md`'s, realised in [`theme`].
//!
//! A library as well as a binary so the window tests
//! (`tests/kittest.rs`) can build the [`Shell`] in egui's harness.

mod panels;
mod shell;
mod viewport;
mod widgets;

pub use shell::files;
pub use shell::theme;
pub use shell::window::{Dialogs, Layout, NativeDialogs, Options, PanelShown, Shell};
