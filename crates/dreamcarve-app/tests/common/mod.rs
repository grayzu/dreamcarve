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
    /// `save_project <file name>`, `open_drawing`, `open_picture`,
    /// `open_library`,
    /// `save_library <file name>`, `export_folder`,
    /// `save_setup_sheet <suggested path>`.
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

    fn open_picture(&mut self, _title: &str) -> Option<PathBuf> {
        self.next("open_picture".to_owned())
    }

    fn open_library(&mut self, _title: &str) -> Option<PathBuf> {
        self.next("open_library".to_owned())
    }

    fn open_font(&mut self, _title: &str) -> Option<PathBuf> {
        self.next("open_font".to_owned())
    }

    fn save_library(&mut self, _title: &str, file_name: &str) -> Option<PathBuf> {
        self.next(format!("save_library {file_name}"))
    }

    fn export_folder(&mut self, _title: &str, _start: Option<&Path>) -> Option<PathBuf> {
        self.next("export_folder".to_owned())
    }

    fn save_setup_sheet(&mut self, _title: &str, dir: &Path, file_name: &str) -> Option<PathBuf> {
        self.next(format!(
            "save_setup_sheet {}",
            dir.join(file_name).display()
        ))
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

/// Opens step `name` (*Design*, *Stock*, *Tools*, *Operations*, *Export*)
/// from the rail.
pub fn go_to(harness: &mut Harness<'_, Shell>, name: &str) {
    use egui_kittest::kittest::Queryable as _;
    harness
        .get_by_role_and_label(egui::accesskit::Role::Tab, name)
        .click();
    harness.run();
}

/// Opens operation `name` in the Inspector, from the Operations step's
/// list.
pub fn open_operation(harness: &mut Harness<'_, Shell>, name: &str) {
    use egui_kittest::kittest::Queryable as _;
    go_to(harness, "Operations");
    harness
        .get_by_role_and_label(egui::accesskit::Role::ListItem, name)
        .click();
    harness.run();
}

/// Draws frames until the picture the Add picture sheet waits for is
/// decoded (on its own thread) and the sheet is open, or its refusal is
/// in the status bar. Call it straight after the click or the drop that
/// asks for the picture, with no `run` between.
///
/// The frames are drawn one at a time (`step`) until the picture has
/// landed, and only then is the window left to settle (`run`). The
/// thread lands when it lands; `run` gives a window four frames to stop
/// asking for more, and a picture that landed between two of them left
/// the sheet it opens too few: the test then failed with "exceeded
/// max_steps (4). It would have settled after 6 steps", three times in
/// 150 runs of `drop_of_png_opens_the_picture_sheet` (the phase 4
/// audit's "`Harness::run` and `max_steps`"). The first few frames take
/// the click and start the thread.
pub fn settle_picture(harness: &mut Harness<'_, Shell>) {
    let started = std::time::Instant::now();
    for _ in 0..3 {
        harness.step();
    }
    while harness.state().opening_picture() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the picture never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        harness.step();
    }
    harness.run();
}

/// A picture file for a test: `pixels` as an 8-bit RGB PNG at `dir/name`,
/// tinted warm so it is a colour picture (the importer converts it to
/// grayscale and says so), so no picture needs committing to show one
/// the fixtures lack. Written without compression: a zlib stream of
/// stored blocks.
pub fn write_colour_png(dir: &Path, name: &str, pixels: &dc_project::Grayscale) -> PathBuf {
    let (width, height) = (pixels.width_px as usize, pixels.height_px as usize);
    // Each row: filter 0, then R, G, B a sample: a sepia of the gray.
    let mut raw = Vec::with_capacity(height * (1 + 3 * width));
    for row in pixels.bytes.chunks(width).take(height) {
        raw.push(0);
        for gray in row {
            let g = f64::from(*gray);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to a byte"
            )]
            let byte = |v: f64| v.clamp(0.0, 255.0).round() as u8;
            raw.extend([byte(g * 1.06 + 6.0), byte(g * 0.98), byte(g * 0.84)]);
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks = raw.chunks(0xFFFF).count();
    for (k, block) in raw.chunks(0xFFFF).enumerate() {
        let len = u16::try_from(block.len()).expect("a stored block's length");
        zlib.push(u8::from(k + 1 == blocks));
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend(block);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        png.extend(
            u32::try_from(body.len())
                .expect("a chunk's length")
                .to_be_bytes(),
        );
        let mut crc = !0_u32;
        for byte in kind.iter().chain(body) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        png.extend(kind);
        png.extend(body);
        png.extend((!crc).to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend(pixels.width_px.to_be_bytes());
    header.extend(pixels.height_px.to_be_bytes());
    // 8 bits a channel, colour type 2 (RGB), no interlace.
    header.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    let path = dir.join(name);
    std::fs::write(&path, png).expect("the PNG is written");
    path
}

/// A picture with no symmetry, `width_px × height_px`: a pale sky that
/// darkens toward the bottom, a white sun in the top right, a dark hill
/// rising to the left along the bottom. Turned or mirrored, it shows.
pub fn landscape(width_px: u32, height_px: u32) -> dc_project::Grayscale {
    let (w, h) = (f64::from(width_px), f64::from(height_px));
    let mut bytes = Vec::with_capacity(width_px as usize * height_px as usize);
    for row in 0..height_px {
        for column in 0..width_px {
            let (x, y) = (f64::from(column) / w, f64::from(row) / h);
            // The sky: 215 at the top down to 150 at the horizon.
            let mut gray = 215.0 - 65.0 * y;
            // The sun, top right, with a soft edge.
            let sun = ((x - 0.78).powi(2) * (w / h).powi(2) + (y - 0.24).powi(2)).sqrt();
            if sun < 0.13 {
                gray = 255.0;
            } else if sun < 0.16 {
                gray += (255.0 - gray) * (0.16 - sun) / 0.03;
            }
            // The far hill, mid grey, and the near one, dark, higher at
            // the left.
            let far = 0.62 + 0.10 * (x * 5.0).sin() + 0.08 * x;
            let near = 0.70 + 0.22 * x - 0.06 * (x * 9.0).cos();
            if y > far {
                gray = 105.0 - 25.0 * (y - far);
            }
            if y > near {
                gray = 38.0 + 30.0 * (y - near);
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to a byte"
            )]
            bytes.push(gray.clamp(0.0, 255.0).round() as u8);
        }
    }
    dc_project::Grayscale {
        width_px,
        height_px,
        bytes,
    }
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

/// Draws frames until generation, the worker and any export plan are
/// done, so nothing asks for another frame while a test drags or looks.
pub fn settle(harness: &mut Harness<'_, Shell>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while harness.state().app().generation().busy()
        || harness.state().worker_busy()
        || harness.state().export_planning()
    {
        assert!(std::time::Instant::now() < deadline, "the window settles");
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    harness.run();
}
