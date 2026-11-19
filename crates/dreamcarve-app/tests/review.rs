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
//! same way.
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
    let picture = common::write_colour_png(&dir, "landscape.png", &common::landscape(480, 320));

    // What File ▸ New Project from Picture… makes of it, 120 mm wide,
    // then what its Photo V-Carve needs to cut: a thickness, the V-bit's
    // rates and a max depth.
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
    h.run();
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
    h.run();
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
