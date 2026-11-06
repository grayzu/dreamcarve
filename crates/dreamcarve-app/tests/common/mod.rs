// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the window tests share (`docs/specs/phase-4/design.md`, "Test
//! harness"): a scratch config directory per test, the committed fixtures,
//! a shell in egui's kittest harness, file dialogs that answer from a
//! queue, and the window's close button.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dreamcarve_app::{Dialogs, Options, Shell};
use dreamcarve_shell::Paths;
use eframe::egui;
use egui_kittest::Harness;

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `tests/fixtures/<name>`.
pub fn fixture(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures").join(name)
}

/// A fresh, empty directory for test `name`.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// Fixture `name` copied into `dir`, as a user's file would be.
pub fn copy_fixture(name: &str, dir: &Path) -> PathBuf {
    let to = dir.join(name);
    std::fs::copy(fixture(name), &to).expect("fixture copied");
    to
}

/// File dialogs that answer from a queue the test fills: each call takes
/// the next answer, `None` (the user cancelled) when the queue is empty.
#[derive(Debug, Clone, Default)]
pub struct QueuedDialogs {
    answers: Arc<Mutex<VecDeque<Option<PathBuf>>>>,
    /// What each dialog was asked, in order: `open_project`,
    /// `save_project <file name>`, `open_drawing`.
    pub asked: Arc<Mutex<Vec<String>>>,
}

impl QueuedDialogs {
    /// Queues `path` as the next dialog's answer.
    pub fn answer(&self, path: &Path) {
        self.answers
            .lock()
            .expect("queue")
            .push_back(Some(path.to_path_buf()));
    }

    /// The dialogs asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("asked").clone()
    }

    fn next(&self, asked: String) -> Option<PathBuf> {
        self.asked.lock().expect("asked").push(asked);
        self.answers.lock().expect("queue").pop_front().flatten()
    }
}

impl Dialogs for QueuedDialogs {
    fn open_project(&mut self, _title: &str) -> Option<PathBuf> {
        self.next("open_project".to_owned())
    }

    fn save_project(&mut self, _title: &str, file_name: &str) -> Option<PathBuf> {
        self.next(format!("save_project {file_name}"))
    }

    fn open_drawing(&mut self, _title: &str) -> Option<PathBuf> {
        self.next("open_drawing".to_owned())
    }
}

/// The options a test window starts with: `gui.toml` under `config`, the
/// project `open` passed as `--open`, and `dialogs`.
pub fn options(config: &Path, open: Option<PathBuf>, dialogs: &QueuedDialogs) -> Options {
    Options {
        config: Some(Paths::in_dir(config.to_path_buf())),
        open,
        dialogs: Box::new(dialogs.clone()),
    }
}

/// A shell in a `width` × `height` window on egui's CPU harness: every
/// behaviour test but the smoke's rendered frame runs on it, with no GPU.
pub fn harness(options: Options, width: f32, height: f32) -> Harness<'static, Shell> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, height))
        .build_eframe(move |cc| Shell::new(cc, options));
    harness.run();
    harness
}

/// Presses the window's close button, as the OS reports it.
pub fn press_close(harness: &mut Harness<'_, Shell>) {
    harness
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
}

/// The commands the shell sent the window in the last frame.
pub fn window_commands(harness: &Harness<'_, Shell>) -> Vec<egui::ViewportCommand> {
    harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| v.commands.clone())
        .unwrap_or_default()
}

/// ⌘ on macOS, Ctrl elsewhere, with `key`.
pub fn command_key(harness: &Harness<'_, Shell>, key: egui::Key) {
    harness.key_press_modifiers(egui::Modifiers::COMMAND, key);
}

/// ⇧⌘ on macOS, Shift+Ctrl elsewhere, with `key`.
pub fn shift_command_key(harness: &Harness<'_, Shell>, key: egui::Key) {
    harness.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key);
}

/// The project bytes the app writes for the `.dcv` at `path`: the file
/// loaded and serialized, as the CLI would save it.
pub fn saved_form(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("fixture readable");
    let loaded = dc_project::load(&bytes).expect("fixture loads");
    dc_project::save(&loaded.project).expect("fixture saves")
}
