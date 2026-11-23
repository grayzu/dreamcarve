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
//!
//! `redesign_d3_screenshots` writes the redesign's slice D3 set
//! (`docs/specs/phase-4/redesign.md`, "Slices") into
//! `docs/specs/phase-4/review/d3/`, where it is committed:
//!
//! ```sh
//! cargo test -p dreamcarve-app --test review redesign_d3 -- --ignored
//! ```
//!
//! `redesign_d6_screenshots` writes slice D6's set (pictures, Photo
//! V-Carve and the tapered ball nose in the window) into `review/d6/` the
//! same way, and `redesign_d4_screenshots` slice D4's (Export as a step,
//! the Viewport's floating controls, the Warnings drawer, the sheets and
//! the status bar) into `review/d4/`, and `redesign_d9_screenshots` slice
//! D9's (what the fixes to the defects the user guide found look like)
//! into `review/d9/`.
//!
//! The slice D2 set beside it, `review/d2/`, shows the tokens and the
//! components in the layout before the steps; the test that wrote it was
//! this one's earlier form.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::{Path, PathBuf};

use common::{
    QueuedDialogs, command_key, copy_fixture, fixture, go_to, open_operation, options, scratch,
};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
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
    shoot_in(
        h,
        &out_dir(),
        scene,
        &[dc_app::Theme::Light, dc_app::Theme::Dark],
    );
}

/// Writes `scene` into `dir` in each of `looks` at the current pass's
/// scale, as `<scene>-<light|dark>-<scale>x.png`, and leaves the window in
/// the appearance it had. The appearance is set as the preference, which
/// wins over the OS's.
fn shoot_in(h: &mut H, dir: &Path, scene: &str, looks: &[dc_app::Theme]) {
    let ppp = SCALE.get();
    let before = h.state().app().prefs().theme;
    for theme in looks {
        let look = match theme {
            dc_app::Theme::Light => "light",
            dc_app::Theme::Dark | dc_app::Theme::System => "dark",
        };
        h.state_mut().app_mut().prefs_mut().theme = *theme;
        h.run();
        let name = format!("{scene}-{look}-{ppp}x");
        let options = SnapshotOptions::new().output_path(dir);
        let _ = std::fs::remove_file(dir.join(format!("{name}.png")));
        // No reference exists, so this fails having written `.new.png`.
        let _ = h.try_snapshot_options(&name, &options);
        let new = dir.join(format!("{name}.new.png"));
        std::fs::rename(&new, dir.join(format!("{name}.png"))).expect("the picture was written");
    }
    h.state_mut().app_mut().prefs_mut().theme = before;
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
    settle(&mut h);
    Some(h)
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

/// Opens the toolpath view from the Viewport's *Layers* button, clicks
/// each of `labels` in it, and closes it with Escape.
fn toolpath_view(h: &mut H, labels: &[&str]) {
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    for label in labels {
        click(h, label);
    }
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
    shoot(&mut h, "03-workspace-export-step");
    go_to(&mut h, "Stock");
    shoot(&mut h, "03b-stock-step");
    click(&mut h, "File");
    shoot(&mut h, "04-file-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "View");
    shoot(&mut h, "05-view-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    go_to(&mut h, "Design");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i4"),
        ]));
    h.run();
    shoot(&mut h, "06-shape-selected");
    // The Add menu: every operation kind's glyph.
    go_to(&mut h, "Operations");
    click(&mut h, "Add");
    shoot(&mut h, "06b-add-menu");
    h.key_press(egui::Key::Escape);
    h.run();
    go_to(&mut h, "Design");
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
    go_to(&mut h, "Tools");
    shoot(&mut h, "12-tools-step");
    go_to(&mut h, "Operations");
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    shoot(&mut h, "13-toolpath-view");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "By depth");
    shoot(&mut h, "13-by-depth-legend");
    click(&mut h, "By operation");
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
    click(&mut h, "By depth");
    toolpath_view(&mut h, &["Program extents"]);
    shoot(&mut h, "16-minimum-window-by-depth");
    toolpath_view(&mut h, &["Program extents"]);
    click(&mut h, "By operation");
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
    shoot(&mut h, "20-export-step");
    // Program Details opened (#624): closed until the user opens it.
    click(&mut h, "Program Details");
    shoot(&mut h, "20-export-step-details");
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    shoot(&mut h, "21-export-result");
    // Run instructions opened (#625): closed whenever the result shows.
    click(&mut h, "Run instructions");
    shoot(&mut h, "21-export-result-instructions");
    click(&mut h, "Done");
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

/// Takes the pointer off the window, so no control shows its hover.
fn park_pointer(h: &mut H) {
    h.event(egui::Event::PointerGone);
    h.run();
}

/// Where the redesign's slice D3 pictures are committed.
fn d3_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/d3"),
        PathBuf::from,
    )
}

/// The plaque's first tool that an operation cuts with, without its feed:
/// an error that blocks export.
fn delete_a_feed(h: &mut H) {
    let project = h.state().app().project();
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(tool))
        .unwrap();
    settle(h);
}

/// The redesign's slice D3 set (`docs/specs/phase-4/redesign.md`,
/// "Slices"): the start screen; each of the five steps on the plaque;
/// the Inspector on a shape, a tool and an operation; Export with an
/// error blocking it; the rail with steps that need attention; the
/// Warnings drawer; the window at its minimum size; and a narrower one,
/// where the Inspector lies over the Viewport. Each in dark and light at
/// 1×, and the first two in dark at 2×.
#[test]
#[ignore = "writes the committed D3 review pictures; run by hand"]
fn redesign_d3_screenshots() {
    let out = d3_dir();
    std::fs::create_dir_all(&out).unwrap();
    let both = [dc_app::Theme::Dark, dc_app::Theme::Light];
    for (ppp, looks, all) in [(1.0, &both[..], true), (2.0, &both[..1], false)] {
        SCALE.set(ppp);
        let dir = scratch(&format!("review-d3-{ppp}"));
        // Recent files for the start screen to list.
        std::fs::create_dir_all(dir.join("config")).unwrap();
        let recent: Vec<String> = ["plaque.dcv", "sign.dcv", "dc.dcv"]
            .iter()
            .map(|name| format!("{:?}", fixture(name).canonicalize().unwrap()))
            .collect();
        std::fs::write(
            dir.join("config/gui.toml"),
            format!("recent = [{}]\n", recent.join(", ")),
        )
        .unwrap();
        let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
            return;
        };
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "01-start-screen", looks);
        drop(h);

        let plaque = fixture("plaque.dcv");
        let mut h = window(&dir, Some(plaque.clone()), 1280.0, 800.0).unwrap();
        go_to(&mut h, "Design");
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "02-design-step", looks);
        if !all {
            drop(h);
            std::fs::remove_dir_all(&dir).unwrap();
            continue;
        }
        for (step, scene) in [
            ("Stock", "03-stock-step"),
            ("Tools", "04-tools-step"),
            ("Operations", "05-operations-step"),
            ("Export", "06-export-step"),
        ] {
            go_to(&mut h, step);
            park_pointer(&mut h);
            shoot_in(&mut h, &out, scene, looks);
        }

        // The Inspector: a shape's Properties with the Arrange group, a
        // tool's editor, an operation's editor.
        go_to(&mut h, "Design");
        h.get_by_role_and_label(Role::TreeItem, "square").click();
        h.run();
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "07-design-shape-selected", looks);
        h.state_mut().app_mut().select(dc_app::Selection::Nothing);
        go_to(&mut h, "Tools");
        h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
            .click();
        h.run();
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "08-tools-tool-selected", looks);
        open_operation(&mut h, "Panel");
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "09-operations-operation-selected", looks);

        // An error that blocks export: the primary button is disabled,
        // and Tools and Export are marked on the rail.
        delete_a_feed(&mut h);
        go_to(&mut h, "Export");
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "10-export-blocked", looks);
        // The tool that lacks it, in the Inspector.
        go_to(&mut h, "Tools");
        let lacking = h.state().app().project().operations[0].tool().clone();
        let name = h
            .state()
            .app()
            .project()
            .tools
            .iter()
            .find(|t| t.id == lacking)
            .unwrap()
            .name
            .clone();
        h.get_by_role_and_label(Role::ListItem, &name).click();
        h.run();
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "11-rail-needs-attention", looks);

        // The Warnings drawer, opened from the rail.
        h.get_by_label_contains("Warnings:").click();
        h.run();
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "12-warnings-drawer", looks);
        drop(h);

        // The minimum window, with all four regions.
        let mut h = window(&dir, Some(plaque), 1280.0, 768.0).unwrap();
        open_operation(&mut h, "Cut out");
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "13-minimum-window", looks);
        // Narrower than the minimum: the Inspector lies over the
        // Viewport.
        h.set_size(egui::vec2(1100.0, 768.0));
        h.run();
        park_pointer(&mut h);
        shoot_in(&mut h, &out, "14-narrow-window-inspector-over", looks);
        drop(h);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// Where the slice D6 pictures are committed.
fn d6_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/d6"),
        PathBuf::from,
    )
}

/// A tapered ball nose named `name` under vendor *IDC Woodcraft*, with
/// the rates of `like` and the cutter values given: a 6.35 mm cutting
/// diameter, the tip, and (with a taper) 19.05 mm of flute.
fn tapered_like(
    like: &dc_project::Tool,
    name: &str,
    tip_mm: Option<f64>,
    taper_deg: Option<f64>,
) -> dc_project::Tool {
    dc_project::Tool {
        name: name.into(),
        vendor: Some("IDC Woodcraft".into()),
        product_id: None,
        number: None,
        cutter: dc_project::ToolKind::TaperedBallNose {
            tip_diameter_mm: tip_mm,
            taper_angle_deg: taper_deg,
            flute_length_mm: taper_deg.map(|_| 19.05),
        },
        diameter_mm: 6.35,
        flutes: 2,
        ..like.clone()
    }
}

/// The tapered ball nose's part of the slice D6 set (#685), on the
/// tapered ball nose test part (`tests/fixtures/dc-tbn.dcv`): the Tools step with a tapered ball nose
/// whose values are still unset, its three fields and its rates marked
/// required; the Tool Library with tapered ball nose rows (the Tip
/// column, the angle per side, the unset mark); and an Inlay cut with a
/// tapered ball nose, its editor showing the bit's cutting height and the
/// seat line.
fn d6_tools(out: &Path, looks: &[dc_app::Theme]) {
    let dir = scratch("review-d6-tools");

    // A library to show: the Fusion fixtures as `tools import` writes
    // them, and two tapered ball noses, one measured and one not.
    let mut library = dc_library::Library::default();
    for name in ["fusion/sample.tools", "fusion/vendors.json"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        library = dc_app::library::import(&library, &bytes, false)
            .unwrap()
            .library;
    }
    let like = library
        .tools
        .iter()
        .find(|t| t.tool.name == "1/4 in Flat End Mill")
        .unwrap()
        .tool
        .clone();
    let measured = dc_project::Tool {
        product_id: Some("UHD-25".into()),
        ..tapered_like(&like, "Ultra-High Detail", Some(0.762), Some(6.7))
    };
    let unmeasured = dc_project::Tool {
        feed_mm_min: None,
        ..tapered_like(&like, "Fine Detail", None, None)
    };
    for tool in [measured, unmeasured] {
        dc_library::add(&mut library, tool, None).unwrap();
    }
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/tools.json"),
        dc_library::save(&library).unwrap(),
    )
    .unwrap();

    let Some(mut h) = window(&dir, Some(fixture("dc-tbn.dcv")), 1280.0, 800.0) else {
        return;
    };

    // The Tools step: a tapered ball nose just made, nothing measured yet.
    let project = h.state().app().project().clone();
    let dc_app::Command::NewTool(placeholder) = dc_app::tools::new_tool(&project) else {
        panic!("New tool makes a tool");
    };
    let dc_app::tools::KindSwitch::Update(tapered) =
        dc_app::tools::set_kind(&placeholder, dc_app::tools::KindChoice::TaperedBallNose)
    else {
        panic!("a placeholder becomes a tapered ball nose");
    };
    let tapered = dc_project::Tool {
        name: "New tapered ball nose".into(),
        ..*tapered
    };
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::NewTool(tapered))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "New tapered ball nose")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "04-tools-tapered-ball-nose-unset", looks);

    // The Tool Library: the first vendor's group closed, so the tapered
    // ball noses' group stands beside V-bits and drills; the measured
    // one selected, the other showing what it still needs.
    click(&mut h, "Tool Library…");
    h.get_by_role(Role::Dialog)
        .get_by_label("Acme Router Bits · 5")
        .click();
    h.run();
    for name in ["Fine Detail", "Ultra-High Detail"] {
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::ListItem, name)
            .scroll_to_me();
        h.run();
    }
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Ultra-High Detail")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "05-tool-library-tapered-ball-nose", looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // The Inlay cut with the 0.762 mm tip: the cutting height under the
    // Tool picker, and under Depth the male's planes, the engagement
    // against the cutting height, and the seat line.
    open_operation(&mut h, "DC inlay");
    let depth = h
        .query_all_by_role_and_label(Role::Button, "Depth")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the Depth group");
    depth.click();
    h.run();
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, out, "06-inlay-tapered-ball-nose-seat-line", looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The pictures' part of the slice D6 set, on a project made round a
/// picture (a landscape drawn here, with a sun in its top right and a
/// hill rising to its left, so a picture turned or mirrored shows): the
/// Add picture sheet over the Design step; the Design step with the
/// picture in the tree and on the Viewport, selected, its Properties in
/// the Inspector; and the Operations step with its Photo V-Carve in the
/// editor and the grooves over the picture.
///
/// The project is also written to `target/review/d6-landscape.dcv`, for
/// `dreamcarve preview` to draw the same picture and grooves.
fn d6_pictures(out: &Path, looks: &[dc_app::Theme]) {
    let dir = scratch("review-d6-pictures");
    let (project, picture, carve) = landscape_project(&dir);
    let kept = common::repo_root().join("target/review");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::copy(&project, kept.join("d6-landscape.dcv")).unwrap();

    let dialogs = QueuedDialogs::default();
    let Some(mut h) = window_with(&dir, Some(project), 1280.0, 800.0, &dialogs) else {
        return;
    };

    // The Design step shows the picture itself: the Photo V-Carve's eye
    // is closed, as its row's eye closes it, so no groove lies over it.
    let op = h.state().app().project().operations[0].id().clone();
    h.state_mut().app_mut().toggle_hidden(&op);
    go_to(&mut h, "Design");

    // The Add picture sheet, over the Design step: a second copy of the
    // picture, its width typed and its height following. The click on
    // the title leaves the field, as a click anywhere does.
    dialogs.answer(&picture);
    h.get_by_role_and_label(Role::Button, "Add picture").click();
    common::settle_picture(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Width").focus();
    h.run();
    h.event(egui::Event::Text("90".to_owned()));
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_label("Add picture to project")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "01-add-picture-sheet", looks);
    h.get_by_role(Role::Dialog).get_by_label("Close").click();
    h.run();

    // The picture selected on the Design step: its row, its outline and
    // handles on the Viewport, its Properties in the Inspector.
    h.get_by_role_and_label(Role::TreeItem, "landscape").click();
    h.run();
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, out, "02-design-picture-selected", looks);

    // The Photo V-Carve in the editor, its grooves over the picture. The
    // toolpath view shows the cuts alone, so the grooves read over the
    // picture they are of.
    h.state_mut().app_mut().toggle_hidden(&op);
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    let cuts = dc_app::Visibility {
        rapids: false,
        plunges: false,
        start_points: false,
        tab_marks: false,
        ..h.state().app().view().visibility
    };
    h.state_mut().app_mut().set_visibility(cuts);
    open_operation(&mut h, &carve);
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, out, "03-operations-photo-v-carve", looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A project made round a picture, written into `dir`: a landscape drawn
/// here (a sun in its top right, a hill rising to its left, so a picture
/// turned or mirrored shows), 120 mm wide, as File ▸ New Project from
/// Picture… makes it, then what its Photo V-Carve needs to cut: a
/// thickness, the V-bit's rates and a max depth. Returns the project's
/// path, the picture's, and the Photo V-Carve's name.
fn landscape_project(dir: &Path) -> (PathBuf, PathBuf, String) {
    let picture = common::write_colour_png(dir, "landscape.png", &common::landscape(480, 320));
    let mut model = dc_app::import::PictureImport::open(
        dc_import::ImageFormat::Png,
        "landscape.png",
        &std::fs::read(&picture).unwrap(),
        dc_app::import::Purpose::New,
    );
    model.set_width_mm(120.0);
    let mut app = dc_app::App::new(
        dc_app::App::new_project().unwrap(),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    app.new_from_picture(&model).unwrap();
    let mut stock = app.project().stock.clone();
    stock.thickness_mm = Some(18.0);
    app.run(dc_app::Command::SetStock(stock)).unwrap();
    let mut tool = app.project().tools[0].clone();
    tool.feed_mm_min = Some(1200.0);
    tool.plunge_mm_min = Some(400.0);
    tool.rpm = Some(18000);
    app.run(dc_app::Command::UpdateTool(tool)).unwrap();
    let mut op = app.project().operations[0].clone();
    let dc_project::Operation::PhotoVCarve(photo) = &mut op else {
        panic!("a project from a picture has its Photo V-Carve");
    };
    photo.max_depth_mm = Some(2.0);
    let carve = photo.name.clone();
    app.run(dc_app::Command::UpdateOperation(op)).unwrap();
    let project = dir.join("landscape.dcv");
    std::fs::write(&project, app.serialize().unwrap()).unwrap();
    (project, picture, carve)
}

/// Where the slice D4 pictures are committed.
fn d4_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/d4"),
        PathBuf::from,
    )
}

/// The Export step's part of the slice D4 set, on a copy of the plaque
/// (export writes beside it): the step ready to export, its programs
/// listed and its folder chosen; blocked by a tool without its feed, the
/// finding listed; and after an export, the result in the plan's place.
fn d4_export(out: &Path, looks: &[dc_app::Theme]) {
    // A short path, so the folder reads whole in the pictures.
    let dir = std::env::temp_dir().join("dreamcarve-plaque");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let copy = copy_fixture("plaque.dcv", &dir);
    let Some(mut h) = window(&dir, Some(copy), 1280.0, 800.0) else {
        return;
    };
    go_to(&mut h, "Export");
    settle(&mut h);
    click(&mut h, "Program Details");
    park_pointer(&mut h);
    shoot_in(&mut h, out, "01-export-ready", looks);

    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "03-export-result", looks);
    click(&mut h, "Done");

    delete_a_feed(&mut h);
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, out, "02-export-blocked", looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Viewport's part of the slice D4 set, on the plaque: the floating
/// controls with the toolpaths coloured by operation; by depth, with the
/// legend hung under them; the Layers popover open; and the window at its
/// minimum size, 1280 × 768, with an operation in the Inspector and By
/// depth on, the stock fitted under the legend.
fn d4_viewport(out: &Path, looks: &[dc_app::Theme]) {
    let dir = scratch("review-d4-viewport");
    let plaque = fixture("plaque.dcv");
    let Some(mut h) = window(&dir, Some(plaque.clone()), 1280.0, 800.0) else {
        return;
    };
    go_to(&mut h, "Operations");
    // The pointer over the stock, so the read-out says where it is.
    let over = common::on_screen(&h, dc_geometry::Point::new(64.2, 51.8));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    shoot_in(&mut h, out, "04-viewport-by-operation", looks);
    click(&mut h, "By depth");
    toolpath_view(&mut h, &["Program extents"]);
    let over = common::on_screen(&h, dc_geometry::Point::new(64.2, 51.8));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    shoot_in(&mut h, out, "05-viewport-by-depth-legend", looks);
    toolpath_view(&mut h, &["Program extents"]);
    click(&mut h, "By operation");
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "06-layers-popover", looks);
    drop(h);

    let mut h = window(&dir, Some(plaque), 1280.0, 768.0).unwrap();
    open_operation(&mut h, "Cut out");
    click(&mut h, "By depth");
    park_pointer(&mut h);
    shoot_in(&mut h, out, "13-minimum-window-by-depth", looks);
    click(&mut h, "By operation");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Warnings drawer's and the sheets' part of the slice D4 set: the
/// drawer over the Viewport's bottom on the plaque with an error; the
/// Import drawing sheet, the Add picture sheet, the Tool Library and the
/// unsaved-changes sheet; and the start screen of a blank project, which
/// counts no problem.
fn d4_sheets(out: &Path, looks: &[dc_app::Theme]) {
    let dir = scratch("review-d4-sheets");

    // A library to show: the Fusion fixtures as `tools import` writes
    // them.
    let mut library = dc_library::Library::default();
    for name in ["fusion/sample.tools", "fusion/vendors.json"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        library = dc_app::library::import(&library, &bytes, false)
            .unwrap()
            .library;
    }
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/tools.json"),
        dc_library::save(&library).unwrap(),
    )
    .unwrap();

    // The start screen of a blank project: no count, no badge.
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    park_pointer(&mut h);
    shoot_in(&mut h, out, "12-start-screen-blank", looks);
    drop(h);

    let dialogs = QueuedDialogs::default();
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window_with(&dir, Some(plaque), 1280.0, 800.0, &dialogs).unwrap();

    // The drawer: an error, a hint, and two groups' worth of rows.
    delete_a_feed(&mut h);
    go_to(&mut h, "Operations");
    h.get_by_label_contains("Warnings:").click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "07-warnings-drawer", looks);
    h.get_by_label("Close the Warnings drawer").click();
    h.run();

    // The Import drawing sheet, from a dropped drawing.
    go_to(&mut h, "Design");
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("sign.svg"))));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "08-sheet-import-drawing", looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // The Add picture sheet.
    let picture = common::write_colour_png(&dir, "landscape.png", &common::landscape(480, 320));
    dialogs.answer(&picture);
    h.get_by_role_and_label(Role::Button, "Add picture").click();
    common::settle_picture(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, out, "09-sheet-add-picture", looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // The Tool Library, a tool selected.
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "1/4 in Flat End Mill")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "10-sheet-tool-library", looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // The unsaved-changes sheet: the feed was deleted above.
    common::press_close(&mut h);
    h.step();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, out, "11-sheet-unsaved-changes", looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The slice D4 set (`docs/specs/phase-4/redesign.md`, "Slices"): Export
/// as a step, the Viewport's floating controls, the Warnings drawer and
/// the sheets, and the status bar's pills. Each in dark and light at 1×,
/// written to `docs/specs/phase-4/review/d4/`, where they are committed.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review redesign_d4 -- --ignored
/// ```
#[test]
#[ignore = "writes the committed D4 review pictures; run by hand"]
fn redesign_d4_screenshots() {
    let out = d4_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    d4_export(&out, &looks);
    d4_viewport(&out, &looks);
    d4_sheets(&out, &looks);
}

/// The slice D6 set (`docs/alpha-worklog.md`, slice D6): the features
/// built headless, drawn in the window. Pictures ([`d6_pictures`]): the
/// Add picture sheet; a picture selected on the Design step; a Photo
/// V-Carve in the editor. The tapered ball nose ([`d6_tools`]): its
/// fields unset on the Tools step; its rows in the Tool Library; an Inlay
/// cut with one. Each in dark and light at 1×, written to
/// `docs/specs/phase-4/review/d6/`, where they are committed.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review redesign_d6 -- --ignored
/// ```
#[test]
#[ignore = "writes the committed D6 review pictures; run by hand"]
fn redesign_d6_screenshots() {
    let out = d6_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    d6_pictures(&out, &looks);
    d6_tools(&out, &looks);
}

/// Where the slice D9 pictures are committed.
fn d9_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/d9"),
        PathBuf::from,
    )
}

/// Opens the Warnings drawer from the rail.
fn open_drawer(h: &mut H) {
    h.get_by_label_contains("Warnings:").click();
    h.run();
}

/// The drawer's row about `field`, by its name.
fn row_about(h: &H, field: &str) -> String {
    dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|group| group.rows)
        .find(|row| row.field.as_deref() == Some(field))
        .unwrap_or_else(|| panic!("a row about {field}"))
        .name
}

/// The slice D9 set (`docs/alpha-worklog.md`, slice D9): what the fixes
/// to the defects the user guide found look like, in dark at 1×, written
/// to `docs/specs/phase-4/review/d9/`, where they are committed.
///
/// - `01`: the sign on the Operations step with its first operation
///   unticked and the Layers popover open. The Background V-Carve is two
///   tool runs, its V-bit's lines and its clearing pass's rings; both are
///   its row's colour, the rings lighter, and every later operation still
///   has its row's colour on the Viewport and in the key.
/// - `02`: an Inlay after an unticked V-Carve: both boards in the
///   Inlay's colour, the second.
/// - `03`: the plaque after the square, the only shape its Square recess
///   cuts, was deleted: the row is marked with the error and no toolpath
///   is drawn where the recess was.
/// - `04`: the plaque with a plunge rate above the feed on the 1/4 end
///   mill, after a click on that warning's row in the drawer: the Tools
///   step, the tool in the Inspector, the Plunge field focused, and the
///   warning mark on the tool's row.
/// - `05`: the drawer on the plaque with its outline moved off the
///   stock: the message names `Cut out` where the engine's text gives
///   `op4` alone.
/// - `06`: an Inlay with no tool picked: the error, in the drawer and
///   under the Tool row, names `Inlay 1` and says *(none chosen)*.
/// - `07`, `08`: the window started on the plaque with a `gui.toml` that
///   does not parse: the status bar's message, and the same as a hint in
///   the drawer.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review redesign_d9 -- --ignored
/// ```
#[test]
#[ignore = "writes the committed D9 review pictures; run by hand"]
fn redesign_d9_screenshots() {
    let out = d9_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark];
    SCALE.set(1.0);
    let dir = scratch("review-d9");

    // 01: the sign, its first operation unticked, the key open.
    let Some(mut h) = window(&dir, Some(fixture("sign.dcv")), 1280.0, 800.0) else {
        return;
    };
    let first = h.state().app().project().operations[0].id().clone();
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetOperationEnabled {
            id: first,
            enabled: false,
        })
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Operations");
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    park_pointer(&mut h);
    shoot_in(
        &mut h,
        &out,
        "01-sign-rows-toolpaths-key-one-colour",
        &looks,
    );
    drop(h);

    // 02: an Inlay, both boards, after an unticked V-Carve.
    let mut h = window(&dir, Some(fixture("dc-tbn.dcv")), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Operations");
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-inlay-both-boards-one-colour", &looks);
    drop(h);

    // 03: the only target of the Square recess deleted.
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::DeleteInstances(vec![
            dc_project::InstanceId::new("i4"),
        ]))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Operations");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-target-deleted-no-toolpath", &looks);
    drop(h);

    // 04: a warning about a tool, its row clicked in the drawer.
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    let mut fast = h.state().app().project().tools[1].clone();
    fast.plunge_mm_min = Some(5000.0);
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(fast.clone()))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Design");
    open_drawer(&mut h);
    let name = row_about(&h, &format!("tools[{}].plunge_mm_min", fast.id));
    click(&mut h, &name);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-tool-finding-field-focused", &looks);
    drop(h);

    // 05: a generation error, which the engine words with the
    // operation's id, named in the drawer.
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::TransformInstances {
            ids: vec![dc_project::InstanceId::new("i1")],
            delta: dc_geometry::Transform::translate(60.0, 0.0),
        })
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Operations");
    open_drawer(&mut h);
    park_pointer(&mut h);
    shoot_in(
        &mut h,
        &out,
        "05-drawer-message-names-the-operation",
        &looks,
    );

    // 06: a validation error with an empty tool id, in the drawer and
    // under the field.
    h.state_mut().app_mut().undo();
    let project = h.state().app().project().clone();
    let inlay = dc_app::operations::new_operation(
        &project,
        h.state().app().strings(),
        dc_app::operations::Kind::Inlay,
        &[dc_project::InstanceId::new("i4")],
    )
    .unwrap();
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::AddOperation(inlay))
        .unwrap();
    settle(&mut h);
    open_operation(&mut h, "Inlay 1");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-no-tool-named-in-words", &looks);
    drop(h);

    // 07, 08: a gui.toml that does not parse. A short path, so the
    // message reads whole in the pictures.
    let short = std::env::temp_dir().join("dreamcarve-d9");
    let _ = std::fs::remove_dir_all(&short);
    std::fs::create_dir_all(short.join("config")).unwrap();
    let damaged = "theme = \"light\"\n[panels\nleft_width = 250.0\n";
    std::fs::write(short.join("config/gui.toml"), damaged).unwrap();
    let mut h = window(&short, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "07-damaged-gui-toml-status-bar", &looks);
    open_drawer(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "08-damaged-gui-toml-drawer-hint", &looks);
    drop(h);
    assert_eq!(
        std::fs::read_to_string(short.join("config/gui.toml")).unwrap(),
        damaged,
        "the pictures were taken without the file being written over"
    );
    std::fs::remove_dir_all(&short).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the final review's pictures are committed.
fn final_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/final"),
        PathBuf::from,
    )
}

/// The window's height for a scene that shows an editor with every group
/// open: tall enough that nothing scrolls out of the picture.
const TALL_PX: f32 = 1700.0;

/// The final review's pictures: where they go, and how each is taken.
struct Shots {
    out: PathBuf,
}

impl Shots {
    /// `scene` in dark and light, the pointer off the window.
    fn take(&self, h: &mut H, scene: &str) {
        park_pointer(h);
        self.take_as_is(h, scene);
    }

    /// `scene` in dark and light, the pointer where it is.
    fn take_as_is(&self, h: &mut H, scene: &str) {
        shoot_in(
            h,
            &self.out,
            scene,
            &[dc_app::Theme::Dark, dc_app::Theme::Light],
        );
    }
}

/// Presses Tab until the control named `label` of `role` has the
/// keyboard focus, so its ring is drawn.
fn tab_to(h: &mut H, role: Role, label: &str) {
    for _ in 0..500 {
        h.key_press(egui::Key::Tab);
        h.run();
        let there = h
            .query_all(egui_kittest::kittest::by().predicate(|node| node.is_focused()))
            .next()
            .is_some_and(|node| {
                let node = node.accesskit_node();
                node.role() == role && node.label().as_deref() == Some(label)
            });
        if there {
            return;
        }
    }
    panic!("Tab never reached the {role:?} {label}");
}

/// The Fusion fixtures as `tools import` writes them, and two tapered
/// ball noses, one measured and one not, as `config/tools.json` of `dir`.
fn write_library(dir: &Path) {
    let mut library = dc_library::Library::default();
    for name in ["fusion/sample.tools", "fusion/vendors.json"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        library = dc_app::library::import(&library, &bytes, false)
            .unwrap()
            .library;
    }
    let like = library
        .tools
        .iter()
        .find(|t| t.tool.name == "1/4 in Flat End Mill")
        .unwrap()
        .tool
        .clone();
    let measured = dc_project::Tool {
        product_id: Some("UHD-25".into()),
        ..tapered_like(&like, "Ultra-High Detail", Some(0.762), Some(6.7))
    };
    let unmeasured = dc_project::Tool {
        feed_mm_min: None,
        ..tapered_like(&like, "Fine Detail", None, None)
    };
    for tool in [measured, unmeasured] {
        dc_library::add(&mut library, tool, None).unwrap();
    }
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/tools.json"),
        dc_library::save(&library).unwrap(),
    )
    .unwrap();
}

/// Every group of every kind's editor open, as the preferences keep them.
fn open_every_group(h: &mut H) {
    let sections = &mut h.state_mut().app_mut().prefs_mut().sections;
    for kind in dc_app::operations::Kind::ALL {
        for group in dc_app::editor::Group::ALL {
            sections.insert(group.section(kind), true);
        }
    }
}

/// The start screen and each step of a project with nothing in it.
fn final_blank(shots: &Shots) -> bool {
    let dir = scratch("review-final-blank");
    // Recent files for the start screen to list.
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let recent: Vec<String> = ["plaque.dcv", "sign.dcv", "dc.dcv"]
        .iter()
        .map(|name| format!("{:?}", fixture(name).canonicalize().unwrap()))
        .collect();
    std::fs::write(
        dir.join("config/gui.toml"),
        format!("recent = [{}]\n", recent.join(", ")),
    )
    .unwrap();
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return false;
    };
    shots.take(&mut h, "a01-start-screen");
    for (step, scene) in [
        ("Stock", "a02-stock-empty"),
        ("Tools", "a03-tools-empty"),
        ("Operations", "a04-operations-empty"),
        ("Export", "a05-export-empty"),
    ] {
        go_to(&mut h, step);
        settle(&mut h);
        shots.take(&mut h, scene);
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
    true
}

/// Each step on the plaque, and the Inspector on what a step selects: a
/// shape, several shapes, a locked shape, a placed tab, several tabs, an
/// operation with its groups closed, and a picture.
fn final_steps_and_inspectors(shots: &Shots) {
    let dir = scratch("review-final-steps");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window(&dir, Some(plaque), 1280.0, 800.0).unwrap();
    for (step, scene) in [
        ("Design", "b01-design"),
        ("Stock", "b02-stock"),
        ("Tools", "b03-tools"),
        ("Operations", "b04-operations"),
        ("Export", "b05-export"),
    ] {
        go_to(&mut h, step);
        settle(&mut h);
        shots.take(&mut h, scene);
    }

    // A field that refused what was typed: the error outline and the
    // message under it. Then a label under the pointer: its help and the
    // `.dcv` field it writes, in a tooltip.
    go_to(&mut h, "Stock");
    h.get_by_role_and_label(Role::TextInput, "Width").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("wide".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    shots.take(&mut h, "b06-field-refused");
    let label = h
        .query_all_by_role_and_label(Role::Label, "Thickness")
        .next()
        .expect("the Thickness row's label")
        .rect();
    h.event(egui::Event::PointerMoved(
        label.left_center() + egui::vec2(12.0, 0.0),
    ));
    for _ in 0..12 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    shots.take_as_is(&mut h, "b07-label-tooltip");
    park_pointer(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Width").focus();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();

    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    shots.take(&mut h, "c01-inspector-shape");
    let two = ["i3", "i4"].map(dc_project::InstanceId::new).to_vec();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(two));
    h.run();
    shots.take(&mut h, "c02-inspector-several-shapes");
    let square = vec![dc_project::InstanceId::new("i4")];
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetInstanceFlags {
            ids: square.clone(),
            visible: None,
            locked: Some(true),
        })
        .unwrap();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(square.clone()));
    h.run();
    shots.take(&mut h, "c03-inspector-locked-shape");
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetInstanceFlags {
            ids: square,
            visible: None,
            locked: Some(false),
        })
        .unwrap();

    // An operation in the Inspector, its groups closed: each says what it
    // holds.
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    shots.take(&mut h, "c04-inspector-operation-groups-closed");

    // Its tabs placed, then one and several of them selected.
    click(&mut h, "Place tabs");
    settle(&mut h);
    let tabs: Vec<dc_project::TabId> = h
        .state()
        .app()
        .project()
        .tabs
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert!(tabs.len() >= 2, "Place tabs placed the request's tabs");
    go_to(&mut h, "Design");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Tabs(vec![tabs[0].clone()]));
    h.run();
    shots.take(&mut h, "c05-inspector-tab");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Tabs(tabs));
    h.run();
    shots.take(&mut h, "c06-inspector-several-tabs");
    drop(h);

    // An Inlay with its groups closed: each says what it holds, its
    // Cut-out the tool and the tabs; and with no cut-out, *None*.
    let mut h = window(&dir, Some(fixture("dc.dcv")), 1280.0, 800.0).unwrap();
    open_operation(&mut h, "DC");
    settle(&mut h);
    shots.take(&mut h, "c08-inspector-inlay-groups-closed");
    let mut without = h.state().app().project().operations[0].clone();
    let dc_project::Operation::Inlay(inlay) = &mut without else {
        panic!("the inlay pair's Inlay");
    };
    inlay.cut_out = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateOperation(without))
        .unwrap();
    settle(&mut h);
    shots.take(&mut h, "c09-inspector-inlay-no-cut-out");
    drop(h);

    // A picture.
    let (project, _, _) = landscape_project(&dir);
    let mut h = window(&dir, Some(project), 1280.0, 800.0).unwrap();
    let op = h.state().app().project().operations[0].id().clone();
    h.state_mut().app_mut().toggle_hidden(&op);
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "landscape").click();
    h.run();
    settle(&mut h);
    shots.take(&mut h, "c07-inspector-picture");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The tool editor on a tool of each kind, and on a tapered ball nose
/// with nothing measured yet.
fn final_tools(shots: &Shots) {
    let dir = scratch("review-final-tools");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window(&dir, Some(plaque), 1280.0, 800.0).unwrap();
    let like = h.state().app().project().tools[1].clone();
    let of = |id: &str, name: &str, cutter: dc_project::ToolKind| dc_project::Tool {
        id: dc_project::ToolId::new(id),
        name: name.into(),
        number: None,
        cutter,
        ..like.clone()
    };
    let unset = dc_project::Tool {
        id: dc_project::ToolId::new("t16"),
        number: None,
        feed_mm_min: None,
        plunge_mm_min: None,
        rpm: None,
        ..tapered_like(&like, "New tapered ball nose", None, None)
    };
    let added = vec![
        of("t11", "1/4 ball nose", dc_project::ToolKind::BallNose),
        of(
            "t12",
            "60 degree V-bit",
            dc_project::ToolKind::VBit {
                included_angle_deg: 60.0,
            },
        ),
        of(
            "t13",
            "Diamond drag engraver",
            dc_project::ToolKind::Engraver,
        ),
        of("t14", "5 mm drill", dc_project::ToolKind::Drill),
        dc_project::Tool {
            id: dc_project::ToolId::new("t15"),
            number: None,
            ..tapered_like(&like, "Ultra-High Detail", Some(0.762), Some(6.7))
        },
        unset,
    ];
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::AddTools(added))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Tools");
    for (name, scene) in [
        ("1/4 end mill", "d01-tool-end-mill"),
        ("1/4 ball nose", "d02-tool-ball-nose"),
        ("60 degree V-bit", "d03-tool-v-bit"),
        ("Diamond drag engraver", "d04-tool-engraver"),
        ("5 mm drill", "d05-tool-drill"),
        ("Ultra-High Detail", "d06-tool-tapered-ball-nose"),
        ("New tapered ball nose", "d07-tool-tapered-ball-nose-unset"),
    ] {
        h.get_by_role_and_label(Role::ListItem, name).scroll_to_me();
        h.run();
        h.get_by_role_and_label(Role::ListItem, name).click();
        h.run();
        shots.take(&mut h, scene);
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The operation editor on each of the eight kinds with every group
/// open, in a window tall enough to show all of it.
fn final_editors(shots: &Shots) {
    let dir = scratch("review-final-editors");
    let (landscape, _, carve) = landscape_project(&dir);
    for (project, operations) in [
        (
            fixture("plaque.dcv"),
            vec![
                ("Cut out", "l01-editor-profile"),
                ("Panel", "l02-editor-pocket"),
                ("Screw holes", "l04-editor-bore"),
                ("Border and squiggle", "l05-editor-engrave"),
            ],
        ),
        (
            fixture("sign.dcv"),
            vec![
                ("Hanging holes", "l03-editor-drill"),
                ("Lettering", "l06-editor-v-carve"),
            ],
        ),
        (fixture("dc.dcv"), vec![("DC", "l07-editor-inlay")]),
        (
            fixture("dc-tbn.dcv"),
            vec![("DC inlay", "l08-editor-inlay-tapered-ball-nose")],
        ),
        (
            landscape,
            vec![(carve.as_str(), "l09-editor-photo-v-carve")],
        ),
    ] {
        let mut h = window(&dir, Some(project), 1280.0, TALL_PX).unwrap();
        open_every_group(&mut h);
        for (name, scene) in operations {
            open_operation(&mut h, name);
            settle(&mut h);
            shots.take(&mut h, scene);
        }
        drop(h);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every menu and popover: the header's six menus, the Add menu, the
/// rail's Appearance and Help, the status bar's units, a row's Target
/// actions, the tool picker and a dropdown.
fn final_menus(shots: &Shots) {
    let dir = scratch("review-final-menus");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window(&dir, Some(plaque), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    for (menu, scene) in [
        ("File", "e01-menu-file"),
        ("Edit", "e02-menu-edit"),
        ("Arrange", "e03-menu-arrange"),
        ("View", "e04-menu-view"),
        ("Project", "e05-menu-project"),
        ("Help", "e06-menu-help"),
    ] {
        click(&mut h, menu);
        shots.take(&mut h, scene);
        h.key_press(egui::Key::Escape);
        h.run();
    }
    go_to(&mut h, "Operations");
    click(&mut h, "Add");
    shots.take(&mut h, "e07-menu-add-operation");
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label_contains("Appearance:").click();
    h.run();
    shots.take(&mut h, "e08-menu-appearance");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "Help and About");
    shots.take(&mut h, "e09-menu-rail-help");
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::Button, "mm").click();
    h.run();
    shots.take(&mut h, "e10-menu-units");
    h.key_press(egui::Key::Escape);
    h.run();
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    click(&mut h, "Target actions");
    shots.take(&mut h, "e11-menu-target-actions");
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Tool").click();
    h.run();
    shots.take(&mut h, "e12-popover-tool-picker");
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Side").click();
    h.run();
    shots.take(&mut h, "e13-dropdown-side");
    h.key_press(egui::Key::Escape);
    h.run();
    // The Arrange group's Rotate popover, from a shape's Properties.
    go_to(&mut h, "Design");
    click(&mut h, "Rotate by an angle");
    shots.take(&mut h, "e14-popover-rotate");
    h.key_press(egui::Key::Escape);
    h.run();
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Viewport's floating controls and what hangs from them: the
/// read-out under the pointer, the Layers popover, the depth legend with
/// the program extents on, the target-editing bar, and the Warnings
/// drawer holding an error, a warning and a hint.
fn final_viewport(shots: &Shots) {
    let dir = scratch("review-final-viewport");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window(&dir, Some(plaque), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Operations");
    settle(&mut h);
    let over = common::on_screen(&h, dc_geometry::Point::new(64.2, 51.8));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    shots.take_as_is(&mut h, "f01-viewport-by-operation-read-out");
    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    shots.take(&mut h, "f02-layers-popover");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "By depth");
    toolpath_view(&mut h, &["Program extents"]);
    let over = common::on_screen(&h, dc_geometry::Point::new(64.2, 51.8));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    shots.take_as_is(&mut h, "f03-depth-legend-and-extents");
    toolpath_view(&mut h, &["Program extents"]);
    click(&mut h, "By operation");

    // Target editing: the bar across the Viewport, the tree in the step
    // panel, the rest of the window inert.
    open_operation(&mut h, "Panel");
    click(&mut h, "Edit targets");
    shots.take(&mut h, "f04-target-editing");
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);

    // The drawer: an error (a cutting tool with no feed), a warning (a
    // tool that plunges faster than it cuts) and the plaque's hint.
    delete_a_feed(&mut h);
    let without = h.state().app().project().operations[0].tool().clone();
    let mut fast = h
        .state()
        .app()
        .project()
        .tools
        .iter()
        .find(|t| t.id != without && t.feed_mm_min.is_some())
        .expect("a tool that still has its feed")
        .clone();
    fast.plunge_mm_min = Some(5000.0);
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(fast))
        .unwrap();
    settle(&mut h);
    let counts = dc_app::warnings::counts(h.state().app());
    assert!(
        counts.errors > 0 && counts.warnings > 0 && counts.hints > 0,
        "the drawer's scene has every tier: {counts:?}"
    );
    go_to(&mut h, "Operations");
    h.get_by_label_contains("Warnings:").click();
    h.run();
    shots.take(&mut h, "g01-warnings-drawer");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every sheet: Import drawing on an SVG and on a DXF, Add picture, the
/// Tool Library (a row selected, the Delete question, the import review,
/// and empty) and unsaved changes.
fn final_sheets(shots: &Shots) {
    let dir = scratch("review-final-sheets");
    write_library(&dir);
    let dialogs = QueuedDialogs::default();
    let plaque = copy_fixture("plaque.dcv", &dir);
    let mut h = window_with(&dir, Some(plaque), 1280.0, 800.0, &dialogs).unwrap();
    go_to(&mut h, "Design");
    for (drawing, scene) in [
        ("sign.svg", "h01-sheet-import-drawing-svg"),
        ("dxf/layers.dxf", "h02-sheet-import-drawing-dxf"),
    ] {
        h.input_mut()
            .dropped_files
            .push(std::sync::Arc::new(Dropped(fixture(drawing))));
        h.run();
        shots.take(&mut h, scene);
        h.key_press(egui::Key::Escape);
        h.run();
    }

    let picture = common::write_colour_png(&dir, "landscape.png", &common::landscape(480, 320));
    dialogs.answer(&picture);
    h.get_by_role_and_label(Role::Button, "Add picture").click();
    common::settle_picture(&mut h);
    shots.take(&mut h, "h03-sheet-add-picture");
    h.key_press(egui::Key::Escape);
    h.run();

    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    // Scrolled to the tapered ball noses, the measured one selected: the
    // Tip column, the angle per side, and the dash a tool still needs.
    for name in ["Fine Detail", "Ultra-High Detail"] {
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::ListItem, name)
            .scroll_to_me();
        h.run();
    }
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Ultra-High Detail")
        .click();
    h.run();
    shots.take(&mut h, "h04-sheet-tool-library");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Acme Router Bits 1/4\" Downcut")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Acme Router Bits 1/4\" Downcut")
        .click();
    h.run();
    for name in ["ACME Router Bits 1/8\" Ball Nose", "Birch Tooling BT-DR5"] {
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::CheckBox, name)
            .click();
        h.run();
    }
    shots.take(&mut h, "h05-sheet-tool-library-editing");
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    shots.take(&mut h, "h06-sheet-tool-library-delete");
    h.key_press(egui::Key::Escape);
    h.run();
    dialogs.answer(&fixture("fusion/sample.tools"));
    h.get_by_role(Role::Dialog).get_by_label("Import…").click();
    h.run();
    shots.take(&mut h, "h07-sheet-tool-library-import-review");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();

    // Unsaved changes, on closing the window.
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
    shots.take(&mut h, "h09-sheet-unsaved-changes");
    drop(h);

    // The Tool Library with nothing in it.
    let empty = scratch("review-final-sheets-empty");
    let mut h = window(&empty, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    shots.take(&mut h, "h08-sheet-tool-library-empty");
    drop(h);
    std::fs::remove_dir_all(&empty).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Export step: ready, with its settings groups open, blocked by an
/// error, and after an export, with the run instructions closed and open.
fn final_export(shots: &Shots) {
    // A short path, so the folder reads whole in the pictures.
    let dir = std::env::temp_dir().join("dreamcarve-plaque");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let copy = copy_fixture("plaque.dcv", &dir);
    let mut h = window(&dir, Some(copy.clone()), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Export");
    settle(&mut h);
    click(&mut h, "Program Details");
    shots.take(&mut h, "i01-export-ready");
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    shots.take(&mut h, "i04-export-done");
    click(&mut h, "Run instructions");
    shots.take(&mut h, "i05-export-done-run-instructions");
    click(&mut h, "Done");
    delete_a_feed(&mut h);
    settle(&mut h);
    shots.take(&mut h, "i03-export-blocked");
    drop(h);

    // The settings groups open, in a window tall enough for all three.
    let mut h = window(&dir, Some(copy), 1280.0, TALL_PX).unwrap();
    go_to(&mut h, "Export");
    settle(&mut h);
    for group in ["Post-processor", "Heights", "Tolerances"] {
        click(&mut h, group);
    }
    shots.take(&mut h, "i02-export-settings-open");
    // The result with its run instructions open, all of them in view.
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    click(&mut h, "Run instructions");
    shots.take(&mut h, "i06-export-done-run-instructions-whole");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The window at its minimum size, with all four cards, and narrower,
/// where the Inspector lies over the Viewport.
fn final_windows(shots: &Shots) {
    let dir = scratch("review-final-windows");
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 768.0).unwrap();
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    shots.take(&mut h, "j01-window-1280x768");
    h.set_size(egui::vec2(1100.0, 768.0));
    h.run();
    shots.take(&mut h, "j02-window-1100-wide");
    go_to(&mut h, "Export");
    settle(&mut h);
    shots.take(&mut h, "j03-window-1100-wide-export");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The focus ring after Tab, on each kind of control: a field, a
/// dropdown, a segmented control's pill, a section header, a rail step,
/// the header's primary pill, a tree row, a list row, a bare checkbox, a
/// switch and a status-bar pill.
fn final_focus(shots: &Shots) {
    let dir = scratch("review-final-focus");
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    settle(&mut h);
    go_to(&mut h, "Stock");
    for (role, label, scene) in [
        (Role::Button, "Continue", "k01-focus-primary-pill"),
        (Role::Tab, "Tools", "k02-focus-rail-step"),
        (Role::RadioButton, "in", "k03-focus-segment"),
        (Role::TextInput, "Thickness", "k04-focus-field"),
        (Role::Button, "Origin", "k05-focus-section-header"),
        (Role::ComboBox, "XY zero", "k06-focus-dropdown"),
        (Role::Button, "mm", "k07-focus-status-pill"),
    ] {
        tab_to(&mut h, role, label);
        shots.take(&mut h, scene);
    }
    h.key_press(egui::Key::Escape);
    h.run();
    go_to(&mut h, "Design");
    tab_to(&mut h, Role::TreeItem, "pocket");
    shots.take(&mut h, "k08-focus-tree-row");
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    for (role, label, scene) in [
        (Role::ListItem, "Panel", "k09-focus-list-row"),
        (Role::CheckBox, "Enable Panel", "k10-focus-checkbox"),
        (Role::Switch, "Tab request", "k11-focus-switch"),
    ] {
        tab_to(&mut h, role, label);
        shots.take(&mut h, scene);
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The final review's set (`docs/alpha-worklog.md`, row D5;
/// `docs/specs/phase-4/redesign.md`, "Slices", D5): one picture of every
/// state the window can show, in dark and light at 1×, written to
/// `docs/specs/phase-4/review/final/`, where they are committed and where
/// `README.md` ticks `experience.md`'s checklist against them.
///
/// - `a`: the start screen and each step of an empty project.
/// - `b`: each step on the plaque.
/// - `c`: the Inspector on a shape, several, a locked one, an operation
///   with its groups closed, a tab, several, and a picture.
/// - `d`: the tool editor on each kind of tool.
/// - `e`: every menu and popover.
/// - `f`, `g`: the Viewport's floating controls, the depth legend, target
///   editing, and the Warnings drawer.
/// - `h`: every sheet.
/// - `i`: the Export step ready, blocked and done.
/// - `j`: the window at 1280 × 768 and 1100 px wide.
/// - `k`: the focus ring on each kind of control.
/// - `l`: the operation editor on each kind, every group open.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review redesign_review -- --ignored
/// ```
#[test]
#[ignore = "writes the committed final review pictures; run by hand"]
fn redesign_review() {
    let out = final_dir();
    std::fs::create_dir_all(&out).unwrap();
    SCALE.set(1.0);
    let shots = Shots { out };
    if !final_blank(&shots) {
        return;
    }
    final_steps_and_inspectors(&shots);
    final_tools(&shots);
    final_editors(&shots);
    final_menus(&shots);
    final_viewport(&shots);
    final_sheets(&shots);
    final_export(&shots);
    final_windows(&shots);
    final_focus(&shots);
}

/// Where the smoke-test pictures are committed.
fn smoke_dir() -> PathBuf {
    common::repo_root().join("docs/specs/phase-4/review/smoke")
}

/// A press at `from`, the pointer moved to `to` a frame at a time, and
/// the release there.
fn drag(h: &mut H, from: egui::Pos2, to: egui::Pos2) {
    h.event(egui::Event::PointerMoved(from));
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=4_u8 {
        let t = f32::from(k) / 4.0;
        h.event(egui::Event::PointerMoved(from + (to - from) * t));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

/// What the fixes to the maintainer's smoke test of the alpha look like
/// (2026-11-20), written into `docs/specs/phase-4/review/smoke/`, dark at
/// 1×: the Stock step with a field showing its help; the Tool Library
/// dragged larger with a column made narrower for the names; the tool
/// editor with the stepover's distance; the Inlay's editor with a
/// boundary chosen and the Male board group open; the Export step ready
/// and blocked, its button at its foot; and the export's result sheet.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review redesign_smoke -- --ignored
/// ```
#[test]
#[ignore = "writes the committed smoke-test pictures; run by hand"]
fn redesign_smoke() {
    let out = smoke_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark];
    SCALE.set(1.0);
    let dir = scratch("review-smoke");

    // 01: the Stock step, the thickness being typed, its help at the foot.
    let Some(mut h) = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0) else {
        return;
    };
    go_to(&mut h, "Stock");
    h.get_by_role_and_label(Role::TextInput, "Thickness")
        .focus();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-stock-field-help", &looks);

    // 02: the tool editor, the stepover's distance under its field, and
    // a warning under the plunge it is about.
    let mut fast = h.state().app().project().tools[1].clone();
    fast.plunge_mm_min = Some(5000.0);
    fast.feed_mm_min = Some(1000.0);
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(fast))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Stepover").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("8".to_owned()));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-tool-stepover-distance", &looks);
    drop(h);

    // 03: the Tool Library, dragged larger, Type made narrower.
    write_library(&dir);
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1440.0, 900.0).unwrap();
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    let grip = h
        .get_by_role_and_label(Role::Splitter, "Resize the Tool Library")
        .rect()
        .center();
    drag(&mut h, grip, grip + egui::vec2(120.0, 90.0));
    let divider = h
        .get_by_role_and_label(Role::Splitter, "Resize Type")
        .rect()
        .center();
    drag(&mut h, divider, divider + egui::vec2(-60.0, 0.0));
    let divider = h
        .get_by_role_and_label(Role::Splitter, "Resize Product")
        .rect()
        .center();
    drag(&mut h, divider, divider + egui::vec2(-40.0, 0.0));
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Ultra-High Detail")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-library-resized-columns", &looks);
    drop(h);

    // 04: the Inlay, a boundary chosen: Band margin and Outline rounding
    // off, Male board open with Board thickness first.
    let mut h = window(&dir, Some(fixture("dc.dcv")), 1280.0, 1000.0).unwrap();
    open_operation(&mut h, "DC");
    for group in ["Depth", "Male board"] {
        h.query_all_by_role_and_label(Role::Button, group)
            .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
            .unwrap()
            .click();
        h.run();
    }
    h.get_by_role_and_label(Role::ComboBox, "Boundary").click();
    h.run();
    click(&mut h, "frame");
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-inlay-boundary-and-male-board", &looks);
    drop(h);

    // 05 to 07: the Export step ready, its result's sheet, and blocked.
    let work = scratch("review-smoke-export");
    let plaque = copy_fixture("plaque.dcv", &work);
    let mut h = window(&work, Some(plaque), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Export");
    settle(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "05-export-ready", &looks);
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    click(&mut h, "Run instructions");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-export-result-sheet", &looks);
    click(&mut h, "Done");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "07-export-after", &looks);
    delete_a_feed(&mut h);
    settle(&mut h);
    go_to(&mut h, "Export");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "08-export-blocked", &looks);
}

/// Where the field-problem pictures are committed.
fn problems_dir() -> PathBuf {
    common::repo_root().join("docs/specs/phase-4/review/field-problems")
}

/// Types `text` over what the text field `label` holds and leaves it
/// with Tab.
fn type_over(h: &mut H, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
    settle(h);
}

/// Rests the pointer at `at` until a tooltip there has had time to show.
fn rest_at(h: &mut H, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for _ in 0..10 {
        h.step();
    }
    h.run();
}

/// The centre of the problem glyph whose name starts with `start`.
fn glyph_named(h: &H, start: &str) -> egui::Pos2 {
    h.query_all(egui_kittest::kittest::by().predicate(|node| {
        node.label().is_some_and(|name| name.starts_with(start))
            && matches!(node.role(), Role::Button | Role::Image)
    }))
    .next()
    .unwrap_or_else(|| panic!("no glyph named {start}…"))
    .rect()
    .center()
}

/// What a field's problem looks like now that it takes no line (the
/// maintainer's review, 2026-11-20; `experience.md`, section 7, "Field
/// problem"), written into `docs/specs/phase-4/review/field-problems/`,
/// dark and light at 1×: an error field and a warning field in one
/// panel; a hint on a dropdown beside an error; the tooltip open on an
/// error's glyph; a field with two problems, its tooltip open; a refused
/// entry with its tooltip; the help line showing the focused field's
/// problem; the drawer opened from a glyph, its row selected; a tool
/// picker and a Targets row marked; the Tool Library's editing pane; an
/// import sheet with a problem field; and the import sheet dragged
/// larger.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review field_problems -- --ignored
/// ```
#[test]
#[ignore = "writes the committed field-problem pictures; run by hand"]
fn field_problems_review() {
    let out = problems_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-field-problems");

    // 01: the tool editor with an error field (the RPM an operation
    // needs, unset) and a warning field (a plunge above the feed).
    let Some(mut h) = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0) else {
        return;
    };
    let mut tool = h.state().app().project().tools[1].clone();
    tool.plunge_mm_min = Some(5000.0);
    tool.feed_mm_min = Some(1000.0);
    tool.rpm = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(tool))
        .unwrap();
    settle(&mut h);
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-error-and-warning-fields", &looks);

    // 02: the tooltip open on the error's glyph.
    let glyph = glyph_named(&h, "error: ");
    rest_at(&mut h, glyph);
    shoot_in(&mut h, &out, "02-error-glyph-tooltip", &looks);

    // 03: the help line, the Plunge field having the keyboard.
    park_pointer(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Plunge").focus();
    h.run();
    shoot_in(&mut h, &out, "03-help-line-shows-the-problem", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 04: the drawer opened from the warning's glyph, its row selected.
    let glyph = glyph_named(&h, "warning: plunge_above_feed");
    common::click_at(&mut h, glyph, egui::Modifiers::NONE);
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-drawer-opened-from-a-glyph", &looks);
    click(&mut h, "Close the Warnings drawer");

    // 05: a field with two problems, its tooltip open: a count of 0 the
    // file holds, typed over with what is no count.
    open_operation(&mut h, "Cut out");
    type_over(&mut h, "Count", "0");
    type_over(&mut h, "Count", "a few");
    h.key_press(egui::Key::Escape);
    h.run();
    let glyph = h
        .query_all(egui_kittest::kittest::by().predicate(|node| {
            node.label().is_some_and(|name| name.contains('\n')) && node.role() == Role::Button
        }))
        .next()
        .expect("the count's glyph names both")
        .rect()
        .center();
    rest_at(&mut h, glyph);
    shoot_in(&mut h, &out, "05-two-problems-tooltip", &looks);

    // 06: a tool picker and a Targets row marked: a Bore with no tool
    // chosen and no target.
    park_pointer(&mut h);
    let dc_project::Operation::Bore(mut bore) = h
        .state()
        .app()
        .project()
        .operations
        .iter()
        .find(|op| op.name() == "Screw holes")
        .unwrap()
        .clone()
    else {
        panic!("Screw holes is a Bore");
    };
    bore.targets.clear();
    bore.tool = dc_project::ToolId::new("gone");
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateOperation(
            dc_project::Operation::Bore(bore),
        ))
        .unwrap();
    settle(&mut h);
    open_operation(&mut h, "Screw holes");
    open_every_group(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-operation-rows-marked", &looks);

    // 07: a refused entry and its tooltip, on the Stock step.
    go_to(&mut h, "Stock");
    type_over(&mut h, "Width", "wide");
    h.key_press(egui::Key::Escape);
    h.run();
    let glyph = glyph_named(&h, "error: ");
    rest_at(&mut h, glyph);
    shoot_in(&mut h, &out, "07-refused-entry-tooltip", &looks);
    drop(h);

    // 08: a hint on a dropdown beside an error: a project on an older
    // version of its post-processor, and a file name no file can have.
    let work = scratch("review-field-problems-older");
    let older = copy_fixture("plaque.dcv", &work);
    let text = std::fs::read_to_string(&older).unwrap();
    std::fs::write(
        &older,
        text.replace("\"definition_version\": 4", "\"definition_version\": 3"),
    )
    .unwrap();
    let mut h = window(&work, Some(older), 1280.0, 800.0).unwrap();
    go_to(&mut h, "Export");
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Post-processor")
        .click();
    h.run();
    type_over(&mut h, "File name", " ");
    h.key_press(egui::Key::Escape);
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "08-hint-dropdown-and-error", &looks);
    let glyph = glyph_named(&h, "hint: ");
    rest_at(&mut h, glyph);
    shoot_in(&mut h, &out, "09-hint-glyph-tooltip", &looks);
    drop(h);

    // 10: the Tool Library's editing pane, a tool that still needs its
    // values: each field marked, no line under any.
    write_library(&dir);
    let mut h = window(&dir, Some(fixture("plaque.dcv")), 1280.0, 800.0).unwrap();
    click(&mut h, "Project");
    click(&mut h, "Tool Library…");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Fine Detail")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "Fine Detail")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "10-library-editing-pane", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 11: the Import drawing sheet, a DXF that declares no units: the
    // Units dropdown in the error state, and a join tolerance refused.
    go_to(&mut h, "Design");
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("dxf/no_units.dxf"))));
    h.run();
    type_over(&mut h, "Join tolerance", "tight");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "11-import-sheet-problem-fields", &looks);
    let glyph = glyph_named(&h, "error: ");
    rest_at(&mut h, glyph);
    shoot_in(&mut h, &out, "12-import-sheet-tooltip", &looks);

    // 13: the Import drawing sheet dragged larger by its grip (the smoke
    // test's leftover): the preview is larger with it.
    park_pointer(&mut h);
    h.key_press(egui::Key::Escape);
    h.run();
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("dxf/layers.dxf"))));
    h.run();
    let grip = h
        .get_by_role_and_label(Role::Splitter, "Resize the sheet")
        .rect()
        .center();
    drag(&mut h, grip, grip + egui::vec2(110.0, 60.0));
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "13-import-sheet-resized", &looks);
}

/// Where the artwork-check pictures are committed.
fn import_checks_dir() -> PathBuf {
    common::repo_root().join("docs/specs/phase-4/review/import-checks")
}

/// The artwork checks in the window (functional spec, capabilities 55
/// and 76, acceptance 106), written into
/// `docs/specs/phase-4/review/import-checks/`, dark at 1×:
///
/// - `01`: the Import drawing sheet on the messy SVG
///   (`tests/fixtures/prep/messy.svg`): the first of its eight checks
///   under the preview, each with its code and its message whole.
/// - `02`: the same sheet scrolled to its end: the last checks, the line
///   that names `dreamcarve image-prep` with the link to *Preparing
///   artwork*, and the button still at the sheet's foot.
/// - `03`: the sheet on the tray's DXF (`tests/fixtures/prep/tool-tray.dxf`),
///   as it opens: at its top, its options and layers, the preview, and
///   the list starting at its foot.
/// - `04`: the same sheet scrolled to its end: the overlap of its two
///   pockets and its speck, read from the shapes of the layers ticked.
/// - `05`: the tray imported, the Warnings drawer open and the overlap's
///   row clicked (the pointer still on it): the Design step, the two
///   pockets selected on the Viewport and in the list.
/// - `06`: the same project on the Stock step: the rail's Design button
///   carries the warning mark.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review import_checks -- --ignored
/// ```
#[test]
#[ignore = "writes the committed artwork-check pictures; run by hand"]
fn import_checks_review() {
    let out = import_checks_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark];
    SCALE.set(1.0);
    let dir = scratch("review-import-checks");

    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    // 01, 02: an SVG with one of every check.
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("prep/messy.svg"))));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-sheet-svg-checks", &looks);
    h.get_by_role(Role::Dialog)
        .get_by_label("Preparing artwork")
        .scroll_to_me();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-sheet-svg-checks-end", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 03, 04: a DXF whose pockets overlap.
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("prep/tool-tray.dxf"))));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-sheet-dxf", &looks);
    h.get_by_role(Role::Dialog)
        .get_by_label("Preparing artwork")
        .scroll_to_me();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-sheet-dxf-overlap", &looks);

    // 05: imported; the overlap's row in the drawer, clicked.
    h.get_by_role(Role::Dialog)
        .get_by_label("Add 6 shapes")
        .click();
    h.run();
    settle(&mut h);
    go_to(&mut h, "Stock");
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.run();
    open_drawer(&mut h);
    let row = dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|group| group.rows)
        .find(|row| row.code.as_deref() == Some("shapes_overlap"))
        .expect("the overlap's row")
        .name;
    h.get_by_label(&row).click();
    h.run();
    assert_eq!(h.state().step(), dc_app::steps::Step::Design);
    assert_eq!(h.state().app().selection().instances().len(), 2);
    shoot_in(
        &mut h,
        &out,
        "05-drawer-overlap-row-selects-the-shapes",
        &looks,
    );

    // 06: the rail's mark on Design, seen from another step.
    click(&mut h, "Close the Warnings drawer");
    go_to(&mut h, "Stock");
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-rail-design-marked", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the phase 5 drawing pictures are committed.
fn p5_drawing_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/drawing"),
        PathBuf::from,
    )
}

/// Moves the pointer to project point `(x, y)`, with Ctrl/⌘ held when
/// `free`, so the point is not snapped.
fn point_at(h: &mut H, x: f64, y: f64, free: bool) -> egui::Pos2 {
    let at = common::on_screen(h, dc_geometry::Point::new(x, y));
    let modifiers = if free {
        egui::Modifiers::COMMAND
    } else {
        egui::Modifiers::NONE
    };
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerMoved(at));
    h.run();
    at
}

/// A click at project point `(x, y)`, snapped as the preferences say.
fn click_mm(h: &mut H, x: f64, y: f64) {
    let at = point_at(h, x, y, false);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
}

/// The phase 5 drawing set (`docs/phase-5-worklog.md`, slice 9): the
/// start screen with *Draw from scratch*; the Draw popover; a rectangle
/// being drawn, with the tool bar, the snap marker and the size label; a
/// path of a line and an arc; the Gear tool's sizes; and a drawn shape's
/// Shape section in Properties. Dark and light at 1×, written to
/// `docs/specs/phase-5/review/drawing/`, where they are committed.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_drawing -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 drawing review pictures; run by hand"]
fn phase5_drawing_screenshots() {
    let out = p5_drawing_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-drawing");

    // 01: the start screen offers Draw from scratch.
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-start-screen-draw-from-scratch", &looks);

    // 02: the Draw popover, on the empty stock.
    click(&mut h, "Draw from scratch");
    h.get_by_role_and_label(Role::Button, "Drawing tools")
        .click();
    h.run();
    shoot_in(&mut h, &out, "02-draw-popover", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 03: a rectangle being drawn: the bar, the preview, its size.
    click_mm(&mut h, 20.0, 20.0);
    point_at(&mut h, 80.0, 60.0, false);
    shoot_in(&mut h, &out, "03-rectangle-being-drawn", &looks);
    click_mm(&mut h, 80.0, 60.0);

    // 04: a circle snapping to the rectangle's corner.
    h.key_press(egui::Key::C);
    h.run();
    click_mm(&mut h, 90.0, 80.0);
    point_at(&mut h, 80.5, 59.5, false);
    shoot_in(&mut h, &out, "04-circle-snapping-to-a-corner", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 05: a path: two lines, then an arc waiting for its through point.
    h.key_press(egui::Key::P);
    h.run();
    click_mm(&mut h, 10.0, 70.0);
    click_mm(&mut h, 40.0, 70.0);
    click_mm(&mut h, 40.0, 84.0);
    h.key_press(egui::Key::A);
    h.run();
    click_mm(&mut h, 80.0, 84.0);
    point_at(&mut h, 60.0, 76.0, true);
    shoot_in(&mut h, &out, "05-path-with-an-arc", &looks);
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();

    // 06: the Gear tool's sizes. Nothing is selected, so no Inspector
    // lies over the stock.
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.state_mut()
        .app_mut()
        .arm(dc_app::Tool::Draw(dc_app::Drawing::Gear));
    h.run();
    click_mm(&mut h, 30.0, 78.0);
    click_mm(&mut h, 42.0, 78.0);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-gear-sizes", &looks);
    h.key_press(egui::Key::Enter);
    h.run();
    h.key_press(egui::Key::V);
    h.run();

    // 07: the drawn rectangle selected, its corners rounded: the Shape
    // section in Properties.
    let rectangle = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![rectangle]));
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "Rounded")
        .click();
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "07-shape-section", &looks);

    // 08: View ▸ Snap To.
    click(&mut h, "View");
    h.get_by_role_and_label(Role::Button, "Snap To").click();
    h.run();
    shoot_in(&mut h, &out, "08-view-snap-to", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the phase 5 editing pictures are committed.
fn p5_editing_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/editing"),
        PathBuf::from,
    )
}

/// The phase 5 editing set (`docs/phase-5-worklog.md`, slice 17): a path
/// in the Edit nodes tool with a corner selected and the bar's buttons;
/// the corner filleted; the Shape menu; Offset… in the tool bar with its
/// preview; the Trim tool over the piece a click would take out;
/// (slice 18) the Viewport's context menu and the Edit menu; (slice 20)
/// Align and Distribute in Properties; (slice 21) a group and a
/// folder in the Design Tree; (slice 19) a grid and a circular array in
/// the tool bar; and (slice 22) construction geometry. Dark
/// and light at 1×, written to `docs/specs/phase-5/review/editing/`,
/// where they are committed.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_editing -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 editing review pictures; run by hand"]
fn phase5_editing_screenshots() {
    let out = p5_editing_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-editing");
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    click(&mut h, "Draw from scratch");

    // A closed path of five corners, drawn with the Line tool.
    h.key_press(egui::Key::P);
    h.run();
    for (x, y) in [
        (20.0, 20.0),
        (80.0, 20.0),
        (80.0, 60.0),
        (50.0, 78.0),
        (20.0, 60.0),
        (20.0, 20.0),
    ] {
        click_mm(&mut h, x, y);
    }
    assert_eq!(h.state().app().project().instances.len(), 1);

    // 01: its nodes, the top corner selected.
    h.key_press(egui::Key::N);
    h.run();
    click_mm(&mut h, 50.0, 78.0);
    park_pointer(&mut h);
    h.run_steps(8);
    shoot_in(&mut h, &out, "01-edit-nodes-corner-selected", &looks);

    // 02: that corner filleted.
    h.get_by_role_and_label(Role::Button, "Fillet").click();
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-corner-filleted", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 03: the Shape menu.
    click(&mut h, "Shape");
    shoot_in(&mut h, &out, "03-shape-menu", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 04: Offset… in the tool bar, its preview on the stock.
    h.state_mut()
        .app_mut()
        .arm(dc_app::Tool::Adjust(dc_app::tool::Adjust::Offset));
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-offset-in-the-tool-bar", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 05: Trim over the piece of a line that lies inside the shape.
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.key_press(egui::Key::P);
    h.run();
    for (x, y) in [(6.0, 40.0), (94.0, 40.0)] {
        let at = point_at(&mut h, x, y, true);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::COMMAND,
            });
        }
        h.run();
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.key_press(egui::Key::Enter);
    h.run();
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.state_mut().app_mut().arm(dc_app::Tool::Trim);
    h.run_steps(8);
    point_at(&mut h, 50.0, 40.0, false);
    shoot_in(&mut h, &out, "05-trim-the-piece-inside", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 06: the Viewport's context menu on a shape, something copied.
    let shape = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![shape]));
    h.state_mut().app_mut().copy_selection();
    h.run();
    let at = point_at(&mut h, 80.0, 30.0, false);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(8);
    shoot_in(&mut h, &out, "06-context-menu-on-a-shape", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 07: the Edit menu, with Cut, Copy and Paste.
    click(&mut h, "Edit");
    shoot_in(&mut h, &out, "07-edit-menu", &looks);
    h.key_press(egui::Key::Escape);
    h.run();

    // 08: three shapes selected: Align and Distribute in Properties
    // (slice 20).
    let first = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::DuplicateInstances(vec![first]))
        .unwrap();
    let all: Vec<_> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(all));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "08-align-and-distribute", &looks);

    // 09: a group and a folder in the Design Tree (slice 21).
    let ids: Vec<_> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    let app = h.state_mut().app_mut();
    app.run(dc_app::Command::NewFolder {
        name: "Group 1".to_owned(),
        ids: vec![ids[0].clone(), ids[2].clone()],
        group: true,
    })
    .unwrap();
    app.run(dc_app::Command::NewFolder {
        name: "Guides".to_owned(),
        ids: vec![ids[1].clone()],
        group: false,
    })
    .unwrap();
    app.select(dc_app::Selection::Instances(vec![
        ids[0].clone(),
        ids[2].clone(),
    ]));
    h.run();
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "09-a-group-and-a-folder", &looks);
    drop(h);

    // 10 and 11: Array… in the tool bar (slice 19): a grid of a small
    // rectangle, then a circular array of it, each previewed.
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    click(&mut h, "Draw from scratch");
    // The Rectangle tool, whichever the last window left armed.
    h.key_press(egui::Key::M);
    h.run();
    click_mm(&mut h, 12.0, 14.0);
    click_mm(&mut h, 24.0, 22.0);
    h.key_press(egui::Key::V);
    h.run();
    assert_eq!(h.state().app().project().instances.len(), 1);
    {
        let app = h.state_mut().app_mut();
        let array = &mut app.prefs_mut().adjust.array;
        array.columns = 4;
        array.rows = 3;
        array.x_mm = Some(8.0);
        array.y_mm = Some(10.0);
        app.arm(dc_app::Tool::Adjust(dc_app::tool::Adjust::Array));
    }
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "10-array-grid", &looks);
    h.get_by_role_and_label(Role::RadioButton, "Circular")
        .click();
    h.run_steps(8);
    {
        let app = h.state_mut().app_mut();
        app.prefs_mut().adjust.array.count = 8;
    }
    click_mm(&mut h, 50.0, 50.0);
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "11-array-circular", &looks);

    // 12: construction geometry (slice 22): the array made, its source
    // and three of its copies marked Construction, one of them selected.
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    let ids: Vec<_> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    {
        let app = h.state_mut().app_mut();
        app.run(dc_app::Command::SetConstruction {
            ids: ids.iter().take(4).cloned().collect(),
            construction: true,
        })
        .unwrap();
        app.select(dc_app::Selection::Instances(vec![ids[1].clone()]));
    }
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "12-construction-geometry", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the phase 5 text pictures are committed.
fn p5_text_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/text"),
        PathBuf::from,
    )
}

/// The phase 5 text set (`docs/phase-5-worklog.md`, slice 24): the Text
/// tool with words typed and previewed; the Font list; two lines centred
/// in a script; the single-line font; a text's Properties; and a text
/// whose font is not on this computer. Dark and light at 1×, written to
/// `docs/specs/phase-5/review/text/`, where they are committed.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_text -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 text review pictures; run by hand"]
fn phase5_text_screenshots() {
    let out = p5_text_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-text");
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    click(&mut h, "Draw from scratch");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::T);
    h.run_steps(8);

    // 01: the words typed, previewed where the click put them.
    click_mm(&mut h, 10.0, 70.0);
    h.run_steps(8);
    h.get_by_role_and_label(Role::MultilineTextInput, "Words")
        .type_text("Hook plate");
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-text-typed-and-previewed", &looks);

    // 02: the Font list, with Add font… last.
    h.get_by_role_and_label(Role::ComboBox, "Font").click();
    h.run_steps(8);
    shoot_in(&mut h, &out, "02-font-list", &looks);
    h.get_by_label("Pacifico").click();
    h.run_steps(8);

    // 03: two lines, centred, in the script.
    h.state_mut()
        .app_mut()
        .tool_event(dc_app::tool::Event::Text("Hook\nplate".to_owned()));
    h.get_by_role_and_label(Role::RadioButton, "Centre").click();
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-two-lines-centred-script", &looks);
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);

    // 04: the single-line font, for Engrave.
    {
        let app = h.state_mut().app_mut();
        app.tool_event(dc_app::tool::Event::Option(dc_app::tool::ToolOption::Font(
            dc_project::FontRef {
                family: "Simple Stroke".to_owned(),
                style: "Regular".to_owned(),
            },
        )));
        app.tool_event(dc_app::tool::Event::Option(
            dc_app::tool::ToolOption::TextAlign(dc_project::Align::Left),
        ));
    }
    click_mm(&mut h, 10.0, 20.0);
    h.run_steps(8);
    h.get_by_role_and_label(Role::MultilineTextInput, "Words")
        .type_text("NO. 7");
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-single-line-font", &looks);
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);

    // 05: a text's Properties.
    h.key_press(egui::Key::Escape);
    h.run();
    let first = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![first.clone()]));
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "05-text-properties", &looks);

    // 06: the same text on a computer without its font: the file names
    // a font that is not on hand.
    let mut project = h.state().app().project().clone();
    for definition in project.definitions.values_mut() {
        if let Some(dc_project::Shape::Text { font, .. }) = &mut definition.shape
            && font.family == "Pacifico"
        {
            font.family = "Brush Script Std".to_owned();
        }
    }
    let saved = dir.join("elsewhere.dcv");
    std::fs::write(&saved, dc_project::save(&project).unwrap()).unwrap();
    drop(h);
    let Some(mut h) = window(&dir, Some(saved), 1280.0, 800.0) else {
        return;
    };
    go_to(&mut h, "Design");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![first]));
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "06-font-not-on-this-computer", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the phase 5 backdrop review pictures go.
fn p5_backdrop_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/backdrop"),
        PathBuf::from,
    )
}

/// The phase 5 backdrop set (`docs/phase-5-worklog.md`, slice 25): a
/// picture's Properties with Opacity and *Set scale…*; the picture faded
/// to 40 % with a shape drawn over it; *Set scale…* with its two points
/// clicked and the distance to type; and the picture after, locked.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_backdrop -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 backdrop review pictures; run by hand"]
fn phase5_backdrop_screenshots() {
    let out = p5_backdrop_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-backdrop");
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    click(&mut h, "Draw from scratch");
    h.key_press(egui::Key::Escape);
    h.run_steps(8);
    let picture = common::write_colour_png(&dir, "sketch.png", &common::landscape(480, 320));
    let mut sheet = dc_app::import::PictureImport::open(
        dc_import::ImageFormat::Png,
        "sketch.png",
        &std::fs::read(&picture).unwrap(),
        dc_app::import::Purpose::Import,
    );
    sheet.set_width_mm(120.0);
    h.state_mut().app_mut().import_picture(&sheet).unwrap();
    let id = h.state().app().project().bitmaps[0].id.clone();
    let select = |h: &mut H| {
        h.state_mut()
            .app_mut()
            .select(dc_app::Selection::Bitmaps(vec![id.clone()]));
        h.run_steps(8);
    };

    // 01: the picture's Properties: Opacity and Set scale….
    select(&mut h);
    h.key_press(egui::Key::F);
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-picture-properties", &looks);

    // 02: faded to 40 %, and a shape drawn over it.
    {
        let app = h.state_mut().app_mut();
        app.run(dc_app::Command::SetBitmapOpacity {
            ids: vec![id.clone()],
            opacity_fraction: 0.4,
        })
        .unwrap();
        app.arm(dc_app::Tool::Draw(dc_app::Drawing::Rectangle));
    }
    h.run_steps(8);
    click_mm(&mut h, 30.0, 20.0);
    h.run_steps(30);
    click_mm(&mut h, 90.0, 60.0);
    h.run_steps(8);
    h.key_press(egui::Key::Escape);
    h.run_steps(8);
    select(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-faded-with-a-shape-drawn-over", &looks);

    // 03: Set scale…, its two points clicked.
    h.get_by_role_and_label(Role::Button, "Set scale…").click();
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-set-scale-first-point", &looks);
    click_mm(&mut h, 20.0, 15.0);
    h.run_steps(30);
    click_mm(&mut h, 100.0, 15.0);
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "04-set-scale-distance", &looks);

    // 05: scaled, then locked: a backdrop.
    h.state_mut()
        .app_mut()
        .tool_event(dc_app::tool::Event::Field {
            field: dc_app::tool::SizeField::Distance,
            text: "100".to_owned(),
        });
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    let lock =
        dc_app::tree::toggle_bitmap_locked(h.state().app().project(), std::slice::from_ref(&id))
            .unwrap();
    h.state_mut().app_mut().run(lock).unwrap();
    select(&mut h);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "05-scaled-and-locked", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the phase 5 Measure review pictures go.
fn p5_measure_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/measure"),
        PathBuf::from,
    )
}

/// The phase 5 Measure set (`docs/phase-5-worklog.md`, slice 26): the
/// Draw popover with *Measure*; a tape between two corners with its
/// readings; a shape's length and area; and a selection reading its size
/// while it is moved and while it is resized.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_measure -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 Measure review pictures; run by hand"]
fn phase5_measure_screenshots() {
    let out = p5_measure_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-measure");
    let Some(mut h) = window(&dir, None, 1280.0, 800.0) else {
        return;
    };
    click(&mut h, "Draw from scratch");
    // A plate of 80 × 50 and a hole in it.
    h.key_press(egui::Key::M);
    h.run();
    click_mm(&mut h, 20.0, 20.0);
    h.run_steps(30);
    click_mm(&mut h, 100.0, 70.0);
    h.run_steps(30);
    h.key_press(egui::Key::C);
    h.run();
    click_mm(&mut h, 40.0, 45.0);
    h.run_steps(30);
    click_mm(&mut h, 50.0, 45.0);
    h.run_steps(30);
    h.key_press(egui::Key::Escape);
    h.run_steps(8);
    assert_eq!(h.state().app().project().instances.len(), 2);
    let plate = h.state().app().project().instances[0].id.clone();
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.run_steps(8);

    // 01: the Draw popover, with Trim and Measure under the rule.
    h.get_by_label("Drawing tools").click();
    h.run_steps(8);
    shoot_in(&mut h, &out, "01-draw-popover-with-measure", &looks);
    h.get_by_role_and_label(Role::Button, "Measure").click();
    h.run_steps(8);

    // The pointer moved to a project point; the bar takes some frames
    // to settle when its readings appear.
    let move_to = |h: &mut H, x: f64, y: f64| {
        let at = common::on_screen(h, dc_geometry::Point::new(x, y));
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(12);
        at
    };
    let press = |h: &mut H, at: egui::Pos2, pressed: bool| {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(12);
    };

    // 02: the first point taken, the tape following the pointer.
    click_mm(&mut h, 20.0, 20.0);
    h.run_steps(30);
    move_to(&mut h, 70.0, 45.0);
    shoot_in(&mut h, &out, "02-tape-following-the-pointer", &looks);

    // 03: a tape laid corner to corner.
    let at = move_to(&mut h, 100.0, 70.0);
    press(&mut h, at, true);
    press(&mut h, at, false);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "03-tape-corner-to-corner", &looks);

    // 04: with a shape selected, its length and area.
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![plate.clone()]));
    h.run_steps(8);
    shoot_in(&mut h, &out, "04-shape-length-and-area", &looks);
    h.key_press(egui::Key::Escape);
    h.run_steps(8);

    // 05: the plate dragged, its size under it.
    let from = move_to(&mut h, 60.0, 20.0);
    press(&mut h, from, true);
    let to = move_to(&mut h, 75.0, 32.0);
    shoot_in(&mut h, &out, "05-moved-reading-its-size", &looks);
    press(&mut h, to, false);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(8);

    // 06: resized from a corner.
    let from = move_to(&mut h, 100.0, 70.0);
    press(&mut h, from, true);
    let to = move_to(&mut h, 112.0, 77.5);
    shoot_in(&mut h, &out, "06-resized-reading-its-size", &looks);
    press(&mut h, to, false);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Where the pictures of the 2026-11-23 review feedback go.
fn p5_feedback_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-5/review/feedback"),
        PathBuf::from,
    )
}

/// The review feedback of 2026-11-23, as built: an operation that cuts
/// nothing, in its row and its editor; and a selected shape's handles,
/// with one at the middle of each side.
///
/// ```sh
/// cargo test -p dreamcarve-app --test review phase5_feedback -- --ignored
/// ```
#[test]
#[ignore = "writes the committed phase 5 feedback review pictures; run by hand"]
fn phase5_feedback_screenshots() {
    let out = p5_feedback_dir();
    std::fs::create_dir_all(&out).unwrap();
    let looks = [dc_app::Theme::Dark, dc_app::Theme::Light];
    SCALE.set(1.0);
    let dir = scratch("review-p5-feedback");
    let plaque = common::copy_fixture("plaque.dcv", &dir);
    let Some(mut h) = window(&dir, Some(plaque), 1280.0, 800.0) else {
        return;
    };
    // 01: a shape's handles, corners and sides.
    go_to(&mut h, "Design");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i4"),
        ]));
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F);
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "01-side-handles", &looks);

    // 02: an operation its tool leaves with nothing to cut.
    {
        let app = h.state_mut().app_mut();
        let project = app.project();
        let op = project
            .operations
            .iter()
            .find(|op| op.name() == "Square recess")
            .unwrap();
        let tool = project.tools.iter().find(|t| t.id == *op.tool()).unwrap();
        let wide = dc_project::Tool {
            id: dc_project::ToolId::new("t9"),
            name: "40 mm surfacing bit".to_owned(),
            number: Some(9),
            diameter_mm: 40.0,
            ..tool.clone()
        };
        let dc_project::Operation::Pocket(pocket) = op else {
            panic!("a pocket");
        };
        let mut pocket = pocket.clone();
        pocket.tool = wide.id.clone();
        app.run(dc_app::Command::AddTools(vec![wide])).unwrap();
        app.run(dc_app::Command::UpdateOperation(
            dc_project::Operation::Pocket(pocket),
        ))
        .unwrap();
        app.select(dc_app::Selection::Nothing);
    }
    common::settle(&mut h);
    go_to(&mut h, "Operations");
    common::open_operation(&mut h, "Square recess");
    h.key_press(egui::Key::F);
    h.run_steps(8);
    park_pointer(&mut h);
    shoot_in(&mut h, &out, "02-operation-cuts-nothing", &looks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
