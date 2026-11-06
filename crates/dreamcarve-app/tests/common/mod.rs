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
        .build_eframe(move |cc| Shell::new(cc, options).expect("the shell starts"));
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

/// Whether this machine has a wgpu adapter to render with, asked the way
/// egui_kittest's renderer will ask (the backends `WGPU_BACKEND` allows).
pub fn wgpu_adapter_available() -> bool {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let WgpuSetup::CreateNew(setup) = WgpuSetup::without_display_handle() else {
        return false;
    };
    let instance = wgpu::Instance::new(setup.instance_descriptor);
    let adapters = block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    !adapters.is_empty()
}

/// Runs a future that native wgpu completes without waiting.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::yield_now();
    }
}

/// A shell on the wgpu renderer when this machine has an adapter, else
/// `None`, having printed why `test` renders nothing.
pub fn wgpu_harness(
    test: &str,
    options: Options,
    width: f32,
    height: f32,
) -> Option<Harness<'static, Shell>> {
    if !wgpu_adapter_available() {
        println!(
            "{test}: no wgpu adapter on this machine, so nothing is rendered (set WGPU_BACKEND \
             or install Mesa's lavapipe to render)"
        );
        return None;
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, height))
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
        .build_eframe(move |cc| Shell::new(cc, options).expect("the shell starts"));
    harness.run();
    Some(harness)
}

/// Where project point `p` (mm) is on the window: the Viewport pane's
/// corner plus the camera's screen position for it.
pub fn on_screen(harness: &Harness<'_, Shell>, p: dc_geometry::Point) -> egui::Pos2 {
    let pane = harness.state().layout().viewport;
    let s = harness.state().app().view().camera.to_screen(p);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "window coordinates, exact enough in f32"
    )]
    let at = pane.min + egui::vec2(s.x_px as f32, s.y_px as f32);
    at
}

/// Where window point `at` is in the Viewport pane, as the camera reads it.
pub fn in_pane(harness: &Harness<'_, Shell>, at: egui::Pos2) -> dc_app::viewport::camera::Pos {
    let pane = harness.state().layout().viewport;
    dc_app::viewport::camera::Pos::new(f64::from(at.x - pane.min.x), f64::from(at.y - pane.min.y))
}

/// A press and release of the primary button at `at`, with `modifiers`
/// held.
pub fn click_at(harness: &mut Harness<'_, Shell>, at: egui::Pos2, modifiers: egui::Modifiers) {
    harness.event(egui::Event::PointerMoved(at));
    harness.event(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
}

/// A drag of `button` from `from` to `to`, a frame per step.
pub fn drag(
    harness: &mut Harness<'_, Shell>,
    from: egui::Pos2,
    to: egui::Pos2,
    button: egui::PointerButton,
) {
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    for k in 1..=4_u8 {
        let t = f32::from(k) / 4.0;
        harness.event(egui::Event::PointerMoved(from + (to - from) * t));
        harness.run();
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
}
