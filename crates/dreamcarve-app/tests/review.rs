// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Review screenshots for `experience.md`'s section 13 checklist (#549):
//! every panel and sheet of the window, in light and dark, at 1× and 2×,
//! rendered through wgpu. Ignored, since it writes files and needs an
//! adapter; run it by hand:
//!
//! ```sh
//! DC_REVIEW_DIR=/some/folder cargo test -p dreamcarve-app --test review -- --ignored
//! ```
//!
//! Each scene is written as `<scene>-<light|dark>-<1x|2x>.png` in
//! `DC_REVIEW_DIR` (default `target/review`). These are pictures for a
//! person; the compared snapshots are `kittest::snapshot_workspace_light_and_dark`'s.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::{Path, PathBuf};

use common::{QueuedDialogs, command_key, copy_fixture, fixture, options, scratch};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, SnapshotOptions};

type H = Harness<'static, dreamcarve_app::Shell>;

/// A file dropped on the window: a path on disk.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Where the pictures go.
fn out_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR")
        .map_or_else(|| common::repo_root().join("target/review"), PathBuf::from)
}

/// Steps the window until generation, the worker and any export plan are
/// done.
fn settle(h: &mut H) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while h.state().app().generation().busy()
        || h.state().worker_busy()
        || h.state().export_planning()
    {
        assert!(std::time::Instant::now() < deadline, "the window settles");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
}

thread_local! {
    /// The scale the current pass renders at.
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Writes `scene` in both appearances at the current pass's scale.
fn shoot(h: &mut H, scene: &str) {
    let dir = out_dir();
    let ppp = SCALE.get();
    for (theme, look) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        h.input_mut().system_theme = Some(theme);
        h.run();
        let name = format!("{scene}-{look}-{ppp}x");
        let options = SnapshotOptions::new().output_path(&dir);
        let _ = std::fs::remove_file(dir.join(format!("{name}.png")));
        // No reference exists, so this fails having written `.new.png`.
        let _ = h.try_snapshot_options(&name, &options);
        let new = dir.join(format!("{name}.new.png"));
        std::fs::rename(&new, dir.join(format!("{name}.png"))).expect("the picture was written");
    }
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();
}

/// A window on `open` (if any), `width` x `height` points at the current
/// pass's scale, settled; `None` with no wgpu adapter.
fn window(test_dir: &Path, open: Option<PathBuf>, width: f32, height: f32) -> Option<H> {
    if !common::wgpu_adapter_available() {
        println!("review_screenshots: no wgpu adapter on this machine, so nothing is rendered");
        return None;
    }
    let dialogs = QueuedDialogs::default();
    let options = options(&test_dir.join("config"), open, &dialogs);
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, height))
        .with_pixels_per_point(SCALE.get())
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
        .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options).expect("the shell starts"));
    h.input_mut().system_theme = Some(egui::Theme::Light);
    settle(&mut h);
    Some(h)
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn open_operation(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::ListItem, name).click();
    h.run();
}

#[test]
#[ignore = "writes review pictures; run by hand with DC_REVIEW_DIR"]
fn review_screenshots() {
    std::fs::create_dir_all(out_dir()).unwrap();
    for ppp in [1.0, 2.0] {
        SCALE.set(ppp);
        let dir = scratch(&format!("review-{ppp}"));
        if !scenes(&dir) {
            return;
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// Every scene, in order; false when nothing can be rendered.
fn scenes(dir: &Path) -> bool {
    // The empty state.
    let Some(mut h) = window(dir, None, 1280.0, 800.0) else {
        return false;
    };
    shoot(&mut h, "01-empty-state");
    // The import sheet, from a dropped drawing.
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("sign.svg"))));
    h.run();
    shoot(&mut h, "02-import-sheet");
    drop(h);

    // The plaque, in place and never saved.
    let plaque = fixture("plaque.dcv");
    let mut h = window(dir, Some(plaque.clone()), 1280.0, 800.0).unwrap();
    shoot(&mut h, "03-workspace-project-panel");
    click(&mut h, "File");
    shoot(&mut h, "04-file-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "View");
    shoot(&mut h, "05-view-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i4"),
        ]));
    h.run();
    shoot(&mut h, "06-shape-selected");
    // The Add menu: every operation kind's glyph.
    click(&mut h, "Add");
    shoot(&mut h, "06b-add-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    for (name, scene) in [
        ("Panel", "07-editor-pocket"),
        ("Cut out", "08-editor-profile"),
        ("Screw holes", "09-editor-bore"),
        ("Border and squiggle", "10-editor-engrave"),
    ] {
        open_operation(&mut h, name);
        shoot(&mut h, scene);
    }
    click(&mut h, "Warnings");
    shoot(&mut h, "11-warnings-drawer");
    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    h.get_by_label_contains("Tools ·").click();
    h.run();
    shoot(&mut h, "12-tools-tab");
    h.get_by_label_contains("Operations ·").click();
    h.run();
    h.query_all_by_label("By depth")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the card's By depth segment")
        .click();
    h.run();
    shoot(&mut h, "13-by-depth-legend");
    h.query_all_by_label("By operation")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the card's By operation segment")
        .click();
    h.run();
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    shoot(&mut h, "14-tool-library-sheet");
    h.key_press(egui::Key::Escape);
    h.run();
    for _ in 0..3 {
        h.key_press(egui::Key::Tab);
        h.run();
    }
    shoot(&mut h, "15-focus-ring-after-tab");
    drop(h);

    // The minimum window of spec 50, in By depth (#549's first comment).
    let mut h = window(dir, Some(plaque), 1280.0, 768.0).unwrap();
    h.query_all_by_label("By depth")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the card's By depth segment")
        .click();
    h.run();
    click(&mut h, "Program extents");
    shoot(&mut h, "16-minimum-window-by-depth");
    click(&mut h, "Program extents");
    h.query_all_by_label("By operation")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the card's By operation segment")
        .click();
    h.run();
    drop(h);

    // The sign: Drill and V-Carve.
    let mut h = window(dir, Some(fixture("sign.dcv")), 1280.0, 800.0).unwrap();
    for (name, scene) in [
        ("Hanging holes", "17-editor-drill"),
        ("Lettering", "18-editor-vcarve"),
    ] {
        open_operation(&mut h, name);
        shoot(&mut h, scene);
    }
    drop(h);

    // The inlay pair: the Inlay editor and the male board.
    let mut h = window(dir, Some(fixture("dc.dcv")), 1280.0, 800.0).unwrap();
    open_operation(&mut h, "DC");
    shoot(&mut h, "19-editor-inlay");
    drop(h);

    // Export and its result, beside a copy (export writes there).
    let copy = copy_fixture("plaque.dcv", dir);
    let mut h = window(dir, Some(copy), 1280.0, 800.0).unwrap();
    command_key(&h, egui::Key::E);
    h.run();
    settle(&mut h);
    shoot(&mut h, "20-export-sheet");
    h.get_by_role(Role::Dialog).get_by_label("Export").click();
    h.run();
    shoot(&mut h, "21-export-result-sheet");
    h.get_by_role(Role::Dialog).get_by_label("Close").click();
    h.run();
    // The unsaved-changes sheet.
    let mut stock = h.state().app().project().stock.clone();
    stock.width_mm += 10.0;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetStock(stock))
        .unwrap();
    h.run();
    common::press_close(&mut h);
    h.step();
    h.run();
    shoot(&mut h, "22-unsaved-changes-sheet");
    drop(h);
    true
}
