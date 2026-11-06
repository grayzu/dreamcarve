// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `dreamcarve-app` binary (`docs/specs/phase-4/design.md`, open
//! question 1: a separate binary from the CLI): parses `--open <path>` (or
//! a bare path, as an OS file association passes it), finds the config
//! directory, and runs the [`Shell`] in an eframe window on wgpu. On
//! Windows it is a GUI-subsystem program, with no console.

#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use dc_app::Strings;
use dreamcarve_app::{NativeDialogs, Options, Shell};
use dreamcarve_shell::Paths;
use eframe::egui;

/// The DreamCarve app.
#[derive(Debug, Parser)]
#[command(name = "dreamcarve-app", version, about)]
struct Args {
    /// A project to open.
    #[arg(long, value_name = "PATH")]
    open: Option<PathBuf>,
    /// A project to open, as a file association passes it.
    #[arg(value_name = "FILE", conflicts_with = "open")]
    file: Option<PathBuf>,
}

/// The window's size and place from `gui.toml`, or 1280 × 800.
fn viewport(paths: Option<&Paths>) -> egui::ViewportBuilder {
    let window = paths
        .map(|p| {
            dreamcarve_app::files::prefs_from_file(&dreamcarve_shell::gui_prefs::load(p).prefs)
        })
        .and_then(|prefs| prefs.window);
    let mut builder = egui::ViewportBuilder::default()
        .with_title("DreamCarve")
        .with_app_id("dreamcarve")
        .with_drag_and_drop(true)
        .with_inner_size([1280.0, 800.0]);
    if let Some(window) = window {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "window geometry in logical pixels, exact in f32"
        )]
        let (x, y, w, h) = (
            window.x as f32,
            window.y as f32,
            window.width as f32,
            window.height as f32,
        );
        builder = builder
            .with_position([x, y])
            .with_inner_size([w, h])
            .with_maximized(window.maximized);
    }
    builder
}

fn main() -> ExitCode {
    let args = Args::parse();
    let config = Paths::from_env().ok();
    let options = eframe::NativeOptions {
        viewport: viewport(config.as_ref()),
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    };
    let shell_options = Options {
        config,
        open: args.open.or(args.file),
        dialogs: Box::new(NativeDialogs::new(&Strings::english())),
    };
    let result = eframe::run_native(
        "DreamCarve",
        options,
        Box::new(move |cc| Ok(Box::new(Shell::new(cc, shell_options)?))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
