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
    window_with(test_dir, open, width, height, &QueuedDialogs::default())
}

/// [`window`], its file dialogs answered from `dialogs`.
fn window_with(
    test_dir: &Path,
    open: Option<PathBuf>,
    width: f32,
    height: f32,
    dialogs: &QueuedDialogs,
) -> Option<H> {
    if !common::wgpu_adapter_available() {
        println!("review_screenshots: no wgpu adapter on this machine, so nothing is rendered");
        return None;
    }
    let options = options(&test_dir.join("config"), open, dialogs);
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

/// Opens the toolpath view from the status bar (#616), clicks each of
/// `labels` in it, and closes it with Escape.
fn toolpath_view(h: &mut H, labels: &[&str]) {
    h.get_by_role_and_label(Role::Button, "Toolpaths").click();
    h.run();
    for label in labels {
        click(h, label);
    }
    h.key_press(egui::Key::Escape);
    h.run();
}

/// Opens `name` in the editor and lets the project-level check finish, so
/// the status bar shows the settled state.
fn open_operation(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::ListItem, name).click();
    h.run();
    settle(h);
}

/// Opens the menu bar's `title` menu, shoots it as `scene`, and closes it.
fn menu(h: &mut H, title: &str, scene: &str) {
    click(h, title);
    shoot(h, scene);
    h.key_press(egui::Key::Escape);
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
    // The sheet closed, a new tool on the empty project: the rates
    // required (#621).
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::Button, "New tool").click();
    h.run();
    shoot(&mut h, "12b-new-tool-required");
    drop(h);

    // The plaque, in place and never saved, with a library to show (#549:
    // "Should show this with data, not empty"): the Fusion fixtures as
    // `tools import` writes them.
    let mut library = dc_library::Library::default();
    for name in ["fusion/sample.tools", "fusion/vendors.json"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        library = dc_app::library::import(&library, &bytes, false)
            .unwrap()
            .library;
    }
    // One sample tool edited since, so importing the sample again shows a
    // Changed tool and the review's choice (#623, scene 14d).
    let ball = library
        .tools
        .iter()
        .find(|t| t.tool.name == "3 mm Ball End Mill")
        .unwrap()
        .id
        .clone();
    let library = dc_app::library::edit(
        &library,
        &ball,
        dc_app::tools::ToolField::Rpm,
        "16000",
        dc_project::Units::Mm,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/tools.json"),
        dc_library::save(&library).unwrap(),
    )
    .unwrap();
    let plaque = fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = window_with(dir, Some(plaque.clone()), 1280.0, 800.0, &dialogs).unwrap();
    shoot(&mut h, "03-workspace-project-panel");
    menu(&mut h, "File", "04-file-menu");
    menu(&mut h, "Edit", "04b-edit-menu");
    menu(&mut h, "Project", "04c-project-menu");
    menu(&mut h, "View", "05-view-menu");
    // View ▸ Appearance: System, Light and Dark (#627).
    click(&mut h, "View");
    h.get_by_label("Appearance").hover();
    h.run();
    shoot(&mut h, "05b-appearance-submenu");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i4"),
        ]));
    h.run();
    shoot(&mut h, "06-shape-selected");
    // The Arrange menu and the Rotate popover, with the square selected
    // (#617).
    menu(&mut h, "Arrange", "06d-arrange-menu");
    click(&mut h, "Rotate by an angle");
    shoot(&mut h, "06e-rotate-popover");
    h.key_press(egui::Key::Escape);
    h.run();
    // The Design Tree's context menu: Rename and the target actions.
    h.get_by_label("square").click_secondary();
    h.run();
    shoot(&mut h, "06f-tree-context-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    // The Add menu: every operation kind's glyph.
    click(&mut h, "Add");
    shoot(&mut h, "06b-add-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    // The same shape locked: grey handles and the padlock.
    let lock = |locked: bool| dc_app::Command::SetInstanceFlags {
        ids: vec![dc_project::InstanceId::new("i4")],
        visible: None,
        locked: Some(locked),
    };
    h.state_mut().app_mut().run(lock(true)).unwrap();
    h.run();
    shoot(&mut h, "06c-locked-shape");
    h.state_mut().app_mut().run(lock(false)).unwrap();
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
    // Targets on the drawing (#619): the Panel's targets marked with a
    // target selected too, the row's menu, then target editing with the
    // square added to the draft.
    open_operation(&mut h, "Panel");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i3"),
        ]));
    h.run();
    shoot(&mut h, "10b-targets-marked-island-selected");
    click(&mut h, "Target actions");
    shoot(&mut h, "10c-targets-row-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "Edit targets");
    h.state_mut()
        .app_mut()
        .toggle_target(&dc_project::InstanceId::new("i4"));
    h.run();
    shoot(&mut h, "10d-target-editing-square-added");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "Warnings");
    shoot(&mut h, "11-warnings-drawer");
    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    h.get_by_label_contains("Tools ·").click();
    h.run();
    shoot(&mut h, "12-tools-tab");
    h.get_by_label_contains("Operations ·").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Toolpaths").click();
    h.run();
    shoot(&mut h, "13-toolpath-view");
    h.key_press(egui::Key::Escape);
    h.run();
    toolpath_view(&mut h, &["By depth"]);
    shoot(&mut h, "13-by-depth-legend");
    toolpath_view(&mut h, &["By operation"]);
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    shoot(&mut h, "14-tool-library-sheet");
    // Two rows ticked, the drill selected for editing, in inches.
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::CheckBox, name)
            .scroll_to_me();
        h.run();
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::CheckBox, name)
            .click();
        h.run();
    }
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "5 mm Drill")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "5 mm Drill")
        .click();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "in")
        .click();
    h.run();
    shoot(&mut h, "14b-tool-library-editing");
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    shoot(&mut h, "14c-tool-library-delete");
    h.key_press(egui::Key::Escape);
    h.run();
    // The import review in place of the panes, one tool changed (#623).
    dialogs.answer(&fixture("fusion/sample.tools"));
    h.get_by_role(Role::Dialog).get_by_label("Import…").click();
    h.run();
    shoot(&mut h, "14d-tool-library-import-review");
    click(&mut h, "Import 1 tool");
    shoot(&mut h, "14e-tool-library-imported");
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
    toolpath_view(&mut h, &["By depth"]);
    toolpath_view(&mut h, &["Program extents"]);
    shoot(&mut h, "16-minimum-window-by-depth");
    toolpath_view(&mut h, &["Program extents"]);
    toolpath_view(&mut h, &["By operation"]);
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
    // The drawer with the Lettering's carve note (#620).
    h.get_by_role_and_label(Role::Button, "Warnings").click();
    h.run();
    shoot(&mut h, "11c-warnings-drawer-carve-notes");
    drop(h);

    // The inlay pair: the Inlay editor and the male board.
    let mut h = window(dir, Some(fixture("dc.dcv")), 1280.0, 800.0).unwrap();
    open_operation(&mut h, "DC");
    shoot(&mut h, "19-editor-inlay");
    // Its drawer: warnings, then the note with the two boards.
    h.get_by_role_and_label(Role::Button, "Warnings").click();
    h.run();
    shoot(&mut h, "11b-warnings-drawer-inlay");
    // The status bar's message cell opens the drawer at the Inlay's note,
    // its facts under the two boards' headings (#620).
    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    h.get_by_role_and_label(
        Role::Button,
        "DC · deepest cut 4.000 mm, flat floor at 4.000 mm",
    )
    .click();
    h.run();
    h.run();
    shoot(&mut h, "11d-warnings-drawer-inlay-note");
    drop(h);

    // Export and its result, beside a copy (export writes there).
    let copy = copy_fixture("plaque.dcv", dir);
    let dialogs = QueuedDialogs::default();
    let mut h = window_with(dir, Some(copy), 1280.0, 800.0, &dialogs).unwrap();
    command_key(&h, egui::Key::E);
    h.run();
    settle(&mut h);
    shoot(&mut h, "20-export-sheet");
    // Program Details opened (#624): closed whenever the sheet opens.
    h.get_by_role(Role::Dialog)
        .get_by_label("Program Details")
        .click();
    h.run();
    shoot(&mut h, "20-export-sheet-details");
    h.get_by_role(Role::Dialog).get_by_label("Export").click();
    h.run();
    shoot(&mut h, "21-export-result-sheet");
    // Run instructions opened (#625): closed whenever the result shows.
    h.get_by_role(Role::Dialog)
        .get_by_label("Run instructions")
        .click();
    h.run();
    shoot(&mut h, "21-export-result-instructions");
    // The setup sheet (#625), written beside the pictures; the review
    // renders it to `23-setup-sheet.png` in a browser.
    dialogs.answer(&out_dir().join("23-setup-sheet.html"));
    h.get_by_role(Role::Dialog)
        .get_by_label("Save setup sheet…")
        .click();
    h.run();
    h.get_by_role(Role::Dialog).get_by_label("Close").click();
    h.run();
    // A change, generation stopped: Regenerate all in the status bar
    // (#616); then the unsaved-changes sheet.
    let mut stock = h.state().app().project().stock.clone();
    stock.width_mm += 10.0;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetStock(stock))
        .unwrap();
    h.step();
    h.get_by_label("Stop").click();
    h.step();
    h.step();
    shoot(&mut h, "13b-stopped-regenerate-all");
    common::press_close(&mut h);
    h.step();
    h.run();
    shoot(&mut h, "22-unsaved-changes-sheet");
    drop(h);
    true
}
