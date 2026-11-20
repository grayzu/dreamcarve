// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The window's behaviour, driven through its accessibility tree in
//! egui's kittest harness (`docs/specs/phase-4/design.md`, "Test plan →
//! Shell"; slice 3, #532). Every test but the smoke's rendered frame runs
//! on egui's CPU harness, so it runs on any CI runner; the smoke renders
//! one frame through wgpu when the machine has an adapter (Mesa lavapipe
//! on Linux CI, Metal on macOS, WARP on Windows) and says so when not.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use common::{
    QueuedDialogs, command_key, copy_fixture, go_to, harness, open_operation, options, press_close,
    saved_form, scratch, shift_command_key, wgpu_adapter_available, window_commands,
};
use dc_app::Command;
use dreamcarve_app::theme;
use dreamcarve_shell::{GuiPrefs, Paths, gui_prefs};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, wgpu::WgpuTestRenderer};

/// Acceptance 92 (three platforms): the app launches with `--open
/// plaque.dcv`, shows the plaque (not the empty state), renders one frame
/// through wgpu, saves the project to a new path byte-identical to what
/// the CLI's serializer writes (acceptance 62), and exports it: ⌘E, then
/// *Export* writes the programs `dc_app::export::plan` builds beside it
/// (#547). The file is the phase 2 quick start's.
#[test]
fn opens_plaque_renders_and_exports() {
    let dir = scratch("smoke");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let options = options(&dir.join("config"), Some(plaque.clone()), &dialogs);

    let gpu = wgpu_adapter_available();
    let mut harness = if gpu {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .renderer(WgpuTestRenderer::new())
            .build_eframe(move |cc| {
                dreamcarve_app::Shell::new(cc, options).expect("the shell starts")
            })
    } else {
        println!(
            "opens_plaque_renders_and_exports: no wgpu adapter on this machine, so the \
             frame is not rendered; the open and save still run (set WGPU_BACKEND or \
             install Mesa's lavapipe to render)"
        );
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(move |cc| {
                dreamcarve_app::Shell::new(cc, options).expect("the shell starts")
            })
    };
    harness.run();

    let app = harness.state().app();
    assert_eq!(app.source(), Some(plaque.as_path()));
    assert!(!app.project().instances.is_empty(), "the plaque has shapes");
    assert!(!app.is_dirty(), "a project just opened is clean");
    assert!(
        harness.query_by_label("New from drawing…").is_none(),
        "the start screen is not shown over a project with shapes"
    );
    let title = harness.state().title().to_owned();
    assert_eq!(
        title, "plaque — DreamCarve",
        "the title names the project, with no unsaved marker"
    );
    // The opened file heads the recent list, and gui.toml says so.
    assert_eq!(harness.state().app().prefs().recent.first(), Some(&plaque));

    if gpu {
        let image = harness.render().expect("one frame renders");
        assert_eq!((image.width(), image.height()), (1280, 800));
        let stock_fill = theme::DARK.viewport_stock_fill;
        let found = image.pixels().any(|p| {
            p.0[..3] == stock_fill.to_array()[..3]
                || p.0[..3] == theme::LIGHT.viewport_stock_fill.to_array()[..3]
        });
        assert!(found, "the stock is drawn on the Viewport");
        let colours: std::collections::BTreeSet<[u8; 4]> = image.pixels().map(|p| p.0).collect();
        assert!(colours.len() > 50, "a real frame, not a blank one");
    }

    // Save As to a new path: byte-identical to the CLI's serializer.
    let copy = dir.join("plaque-copy.dcv");
    dialogs.answer(&copy);
    shift_command_key(&harness, egui::Key::S);
    harness.run();
    assert_eq!(dialogs.asked(), ["save_project plaque.dcv"]);
    let written = std::fs::read_to_string(&copy).expect("Save As wrote the file");
    assert_eq!(written, saved_form(&plaque));
    assert_eq!(harness.state().app().source(), Some(copy.as_path()));
    assert!(!harness.state().app().is_dirty());

    // Export: every program the plan lists, beside the project.
    settle_export(&mut harness);
    command_key(&harness, egui::Key::E);
    harness.run();
    settle_export(&mut harness);
    press_export(&mut harness);
    let options = dc_app::export::Options::new(
        &dreamcarve_shell::clock::today_utc(),
        dc_job::EstimateConfig::default(),
    );
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(harness.state().app(), &options)
    else {
        panic!("the plaque has generated");
    };
    for (name, text) in ready.files("plaque-copy") {
        let written = std::fs::read_to_string(dir.join(&name)).expect("program written");
        assert!(written == text, "{name} is the plan's program");
    }

    // Closing a clean project closes at once and remembers the session.
    press_close(&mut harness);
    harness.step();
    assert!(harness.state().close_allowed());
    eframe::App::on_exit(harness.state_mut());
    let saved = gui_prefs::load(&Paths::in_dir(dir.join("config")));
    assert!(saved.notes.is_empty(), "{:?}", saved.notes);
    assert_eq!(saved.prefs.recent.first(), Some(&copy));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// One edit to the open plaque: a wider stock.
fn widen_stock(harness: &mut Harness<'_, dreamcarve_app::Shell>) {
    let mut stock = harness.state().app().project().stock.clone();
    stock.width_mm += 10.0;
    harness
        .state_mut()
        .app_mut()
        .run(Command::SetStock(stock))
        .expect("the stock is set");
    harness.run();
    assert!(harness.state().app().is_dirty());
}

/// Clicks `label` in the unsaved-changes sheet.
fn click_in_sheet(harness: &mut Harness<'_, dreamcarve_app::Shell>, label: &str) {
    harness
        .get_by_role(Role::Dialog)
        .get_by_label(label)
        .click();
    harness.run();
}

/// Acceptance 88 (unsaved changes, capability 51): closing a changed
/// project asks; *Cancel* keeps it open and unchanged; *Don't Save* loses the
/// changes; *Save* writes the file before closing. Opening a second
/// project asks the same.
#[test]
fn unsaved_changes_prompt() {
    let dir = scratch("unsaved");
    let config = dir.join("config");

    // Cancel keeps the window and the edit.
    let plaque = copy_fixture("plaque.dcv", &dir);
    let original = std::fs::read(&plaque).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&config, Some(plaque.clone()), &dialogs),
        1280.0,
        800.0,
    );
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "no sheet on a clean project"
    );
    widen_stock(&mut h);
    assert!(
        h.state().title().starts_with("plaque*"),
        "the title marks the change"
    );
    let edited = h.state().app().project().clone();
    press_close(&mut h);
    h.step();
    assert!(
        window_commands(&h).contains(&egui::ViewportCommand::CancelClose),
        "the close waits for the answer"
    );
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(
            sheet.query_by_label(label).is_some(),
            "the sheet offers {label}"
        );
    }
    assert!(
        sheet.query_by_label_contains("“plaque”").is_some(),
        "the sheet names the project (capability 51)"
    );
    click_in_sheet(&mut h, "Cancel");
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "Cancel closes the sheet"
    );
    assert!(!h.state().close_allowed(), "Cancel keeps the window open");
    assert_eq!(
        h.state().app().project(),
        &edited,
        "and the project as it was"
    );
    assert!(h.state().app().is_dirty());

    // Don't Save closes without writing.
    press_close(&mut h);
    h.run();
    click_in_sheet(&mut h, "Don't Save");
    assert!(
        h.state().close_allowed(),
        "Don't Save lets the window close"
    );
    assert_eq!(std::fs::read(&plaque).unwrap(), original, "nothing written");

    // Save writes the file, then closes.
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&config, Some(plaque.clone()), &dialogs),
        1280.0,
        800.0,
    );
    widen_stock(&mut h);
    let expected = h.state().app().serialize().unwrap();
    press_close(&mut h);
    h.run();
    click_in_sheet(&mut h, "Save");
    assert!(h.state().close_allowed(), "Save then closes");
    assert_eq!(std::fs::read_to_string(&plaque).unwrap(), expected);
    assert!(dialogs.asked().is_empty(), "a saved file needs no dialog");

    // Opening a second project asks too: Cancel keeps the first, Don't Save
    // opens the second.
    let sign = copy_fixture("sign.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&config, Some(plaque.clone()), &dialogs),
        1280.0,
        800.0,
    );
    widen_stock(&mut h);
    command_key(&h, egui::Key::O);
    h.run();
    click_in_sheet(&mut h, "Cancel");
    assert!(dialogs.asked().is_empty(), "Cancel asks for no file");
    assert_eq!(h.state().app().source(), Some(plaque.as_path()));
    assert!(h.state().app().is_dirty());
    dialogs.answer(&sign);
    command_key(&h, egui::Key::O);
    h.run();
    click_in_sheet(&mut h, "Don't Save");
    assert_eq!(dialogs.asked(), ["open_project"]);
    assert_eq!(h.state().app().source(), Some(sign.as_path()));
    assert!(!h.state().app().is_dirty());
    assert!(!h.state().close_allowed());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 50 (capability 50; `redesign.md`, "The window"): at
/// 1280 × 768 the rail (64 px), the step panel (at least 280), the
/// Viewport (at least 520) and the Inspector (at least 300) are side by
/// side, 8 px apart and 8 px from the window's edge; in a narrower window
/// the Inspector lies over the Viewport's right side instead of the
/// window refusing, and the step panel keeps its place; with nothing
/// selected there is no Inspector and the Viewport takes the room. Hiding
/// the step panel from the View menu is a preference, kept in `gui.toml`;
/// choosing a step shows it again.
#[test]
fn inspector_lies_over_the_viewport_below_the_minimum() {
    let dir = scratch("panels");
    let config = dir.join("config");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 768.0);
    go_to(&mut h, "Operations");
    let wide = h.state().layout();
    assert_eq!(wide.inspector, None, "nothing selected: no Inspector");
    open_operation(&mut h, "Panel");

    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    let side_by_side = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let layout = h.state().layout();
        let step = layout.step_panel.expect("the step panel shows");
        let inspector = layout.inspector.expect("the Inspector shows");
        assert!(near(layout.rail.width(), 64.0), "rail {:?}", layout.rail);
        assert!(step.width() >= 280.0 - 0.5, "step panel {step:?}");
        assert!(inspector.width() >= 300.0 - 0.5, "Inspector {inspector:?}");
        assert!(
            layout.viewport.width() >= 520.0 - 0.5,
            "Viewport {:?}",
            layout.viewport
        );
        for (what, gap) in [
            ("window to rail", layout.rail.left()),
            ("rail to step panel", step.left() - layout.rail.right()),
            (
                "step panel to Viewport",
                layout.viewport.left() - step.right(),
            ),
            (
                "Viewport to Inspector",
                inspector.left() - layout.viewport.right(),
            ),
            ("Inspector to window", layout.window.x - inspector.right()),
        ] {
            assert!(near(gap, 8.0), "{what}: {gap} px in {layout:?}");
        }
    };
    side_by_side(&h);
    assert!(
        h.state().layout().viewport.width() < wide.viewport.width(),
        "the Inspector takes its room from the Viewport"
    );

    for (width, height) in [(1279.0, 768.0), (1024.0, 700.0)] {
        h.set_size(egui::vec2(width, height));
        h.run();
        let layout = h.state().layout();
        let inspector = layout.inspector.expect("the Inspector still shows");
        assert!(layout.step_panel.is_some(), "{width} × {height}");
        assert!(
            near(layout.window.x - layout.viewport.right(), 8.0),
            "the Viewport reaches the window's edge at {width} × {height}: {layout:?}"
        );
        assert!(
            near(inspector.right(), layout.viewport.right())
                && inspector.left() > layout.viewport.left()
                && near(inspector.top(), layout.viewport.top()),
            "the Inspector lies over the Viewport's right side at {width} × {height}: {layout:?}"
        );
        assert!(inspector.width() >= 300.0 - 0.5, "{inspector:?}");
    }
    // The editor works there: its fields are the Inspector's.
    let depth = h.get_by_role_and_label(Role::TextInput, "Depth").rect();
    assert!(
        h.state()
            .layout()
            .inspector
            .is_some_and(|i| i.contains_rect(depth)),
        "the Depth field is in the Inspector"
    );

    // Back at full size they are side by side again.
    h.set_size(egui::vec2(1280.0, 768.0));
    h.run();
    side_by_side(&h);

    // Hiding the step panel from the View menu is a preference, kept in
    // gui.toml; a step chosen from the rail shows it again.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Step Panel").click();
    h.run();
    assert_eq!(h.state().layout().step_panel, None);
    assert!(!h.state().app().prefs().panels.left_open);
    let saved: GuiPrefs = gui_prefs::load(&Paths::in_dir(config.clone())).prefs;
    assert!(!saved.panels.left_open, "gui.toml has the hidden panel");
    go_to(&mut h, "Design");
    assert!(h.state().layout().step_panel.is_some());
    assert!(h.state().app().prefs().panels.left_open);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50a and `experience.md` sections 4, 5 and 12: both palettes
/// are installed; the app is dark until the user chooses, whatever the OS
/// shows (`redesign.md`, principle 5); with the preference at System it
/// follows the OS appearance as it changes; a Light or Dark preference
/// overrides the OS and is kept in `gui.toml`; the bundled fonts and the
/// type scale are in force.
#[test]
fn theme_follows_system_and_both_palettes_apply() {
    let dir = scratch("theme");
    let config = dir.join("config");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, None, &dialogs), 1280.0, 800.0);

    let (light, dark) = h
        .ctx
        .options(|o| (o.light_style.visuals.clone(), o.dark_style.visuals.clone()));
    assert_eq!(light.panel_fill, theme::LIGHT.surface_panel);
    assert_eq!(light.extreme_bg_color, theme::LIGHT.surface_sunken);
    assert_eq!(light.selection.bg_fill, theme::LIGHT.accent_soft);
    assert!(!light.dark_mode);
    assert_eq!(dark.panel_fill, theme::DARK.surface_panel);
    assert_eq!(dark.window_fill, theme::DARK.surface_raised);
    assert_eq!(dark.error_fg_color, theme::DARK.status_error.text);
    assert!(dark.dark_mode);

    // Dark by default, over a light OS.
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Dark);
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();
    assert_eq!(h.ctx.theme(), egui::Theme::Dark, "dark until chosen");
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        theme::DARK.surface_panel
    );

    // System, once chosen: follow the OS, both ways.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Appearance").hover();
    h.run();
    h.get_by_label("System").click();
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::System);
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        theme::LIGHT.surface_panel
    );
    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.run();
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        theme::DARK.surface_panel
    );

    // A Light preference wins over a dark OS, and is remembered.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Appearance").hover();
    h.run();
    h.get_by_label("Light").click();
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Light);
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    let saved = gui_prefs::load(&Paths::in_dir(config.clone())).prefs;
    assert_eq!(saved.theme, gui_prefs::Theme::Light);

    // The type scale of section 4 on the bundled faces.
    let style = h.ctx.global_style();
    let body = &style.text_styles[&egui::TextStyle::Body];
    assert_eq!(
        (body.size, &body.family),
        (13.0, &egui::FontFamily::Proportional)
    );
    let small = &style.text_styles[&egui::TextStyle::Small];
    assert_eq!(small.size, 12.0);
    let heading = &style.text_styles[&egui::TextStyle::Heading];
    assert_eq!(
        (heading.size, &heading.family),
        (15.0, &egui::FontFamily::Name("semibold".into()))
    );
    let mono = &style.text_styles[&egui::TextStyle::Monospace];
    assert_eq!(
        (mono.size, &mono.family),
        (12.0, &egui::FontFamily::Monospace)
    );
    let fonts = theme::fonts();
    assert_eq!(
        fonts.families[&egui::FontFamily::Proportional].first(),
        Some(&"Inter-Regular".to_owned())
    );
    assert_eq!(
        fonts.families[&egui::FontFamily::Monospace].first(),
        Some(&"JetBrainsMono-Regular".to_owned())
    );
    assert!(
        !fonts
            .font_data
            .keys()
            .any(|k| k.contains("Ubuntu") || k.contains("Hack")),
        "egui's own fonts are not bundled: {:?}",
        fonts.font_data.keys()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 64 and acceptance 71 in the window (slice 8, #537):
/// Project ▸ Tool Library… opens the sheet over the library in the config
/// directory; *Import…* reads `tests/fixtures/fusion/sample.tools` and
/// shows its review in place of the table (slice 31, #623), whose
/// *Import* writes the library the CLI writes; ticking two
/// rows and *Add 2 tools to project* copies them into the project under
/// new ids as one undo step, as `tools add` does, closes the sheet, and
/// marks them "in
/// project"; the search narrows the table without losing the ticks.
#[test]
fn library_window_adds_ticked_rows() {
    let dir = scratch("library");
    let config = dir.join("config");
    // Opened in place and never saved: the project file is only read.
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 800.0);
    let before = h.state().app().project().clone();

    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label("Tool Library").is_some(),
        "the header is the sheet's title (mock-up 04)"
    );
    assert!(
        sheet
            .query_by_label(
                "The library is empty. Import a Fusion or DreamCarve library to fill it."
            )
            .is_some(),
        "no library yet, so no rows"
    );
    // The empty library's two ways to fill it, under its sentence (slice
    // 30, #622): *New tool* and *Import…* (the header has Import… too).
    assert!(sheet.query_by_label("New tool").is_some());
    assert_eq!(sheet.query_all_by_label("Import…").count(), 2);

    // Import… the Fusion sample: the review replaces the table (slice 31,
    // #623), with the counts `tools import` reports, and nothing is
    // written until *Import*.
    let sample = common::fixture("fusion/sample.tools");
    dialogs.answer(&sample);
    h.get_by_role(Role::Dialog)
        .query_all_by_label("Import…")
        .next()
        .unwrap()
        .click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_library"]);
    let bytes = std::fs::read(&sample).unwrap();
    let merged = dc_app::library::import(&dc_library::Library::default(), &bytes, false).unwrap();
    let sheet = h.get_by_role(Role::Dialog);
    for count in [
        "13 New",
        "0 Already in your library",
        "0 Changed",
        "10 Skipped",
    ] {
        assert!(
            sheet.query_by_label(count).is_some(),
            "the review counts {count}"
        );
    }
    assert!(
        sheet.query_by_label("5 mm Drill · New").is_some(),
        "and lists the file's tools"
    );
    assert!(
        sheet
            .query_by_role_and_label(Role::CheckBox, "5 mm Drill")
            .is_none(),
        "in place of the table"
    );
    assert!(
        !config.join("tools.json").exists(),
        "nothing is written before Import"
    );
    h.get_by_role(Role::Dialog)
        .get_by_label("Import 13 tools")
        .click();
    h.run();
    let written = std::fs::read_to_string(config.join("tools.json")).expect("library written");
    assert_eq!(
        written,
        dc_library::save(&merged.library).unwrap(),
        "the library file is the CLI's"
    );
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label("Imported 13 tools from sample.tools. 10 skipped.")
            .is_some(),
        "back on the table with the banner"
    );

    // Tick two rows; the primary action counts them.
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
    // A search hides a ticked row but keeps its tick. The search is the
    // sheet's first text field; the editing pane's follow it.
    h.get_by_role(Role::Dialog)
        .query_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .focus();
    h.run();
    h.get_by_role(Role::Dialog)
        .query_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("drill");
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet
            .query_by_role_and_label(Role::CheckBox, "1/4 in Flat End Mill")
            .is_none(),
        "the search hides the end mill"
    );
    assert!(
        sheet
            .query_by_role_and_label(Role::CheckBox, "5 mm Drill")
            .is_some()
    );
    h.get_by_role(Role::Dialog)
        .get_by_label("Add 2 tools to project")
        .click();
    h.run();

    let app = h.state().app();
    let project = app.project();
    assert_eq!(project.tools.len(), before.tools.len() + 2);
    let by_name = |name: &str| {
        merged
            .library
            .tools
            .iter()
            .find(|t| t.tool.name == name)
            .unwrap()
            .tool
            .clone()
    };
    // Library order: the drill (lt6) before the end mill (lt11).
    let drill = dc_project::Tool {
        id: dc_project::ToolId::new("t4"),
        ..by_name("5 mm Drill")
    };
    let end_mill = dc_project::Tool {
        id: dc_project::ToolId::new("t5"),
        ..by_name("1/4 in Flat End Mill")
    };
    assert_eq!(
        project.tools[3..],
        [drill, end_mill],
        "the copies `tools add` makes"
    );
    assert_eq!(project.tools[..3], before.tools[..]);
    assert_eq!(app.undo_label(), Some("Add 2 tools"), "one undo step");
    assert!(app.is_dirty());

    // Adding closes the sheet (`library::add_to_project_closes_the_sheet…`).
    // Opened again, both rows are marked and nothing is ticked.
    assert!(h.query_by_role(Role::Dialog).is_none());
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert_eq!(sheet.query_all_by_label("in project").count(), 2);
    assert!(
        sheet.query_by_label("Add to project").is_some(),
        "nothing ticked"
    );

    // Escape closes the sheet; adding left the library file alone.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none());
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The camera the shell holds.
fn camera(h: &Harness<'_, dreamcarve_app::Shell>) -> dc_app::Camera {
    h.state().app().view().camera
}

/// The camera fitted to `bounds` in the shell's pane, clear of the
/// Viewport's floating controls.
fn fitted(h: &Harness<'_, dreamcarve_app::Shell>, bounds: dc_geometry::Rect) -> dc_app::Camera {
    let mut want = camera(h);
    want.fit_inside(bounds, dreamcarve_app::FIT_INSETS);
    want
}

/// Two cameras agree to a thousandth of a pixel per millimetre and a
/// thousandth of a millimetre.
fn assert_same_view(got: dc_app::Camera, want: dc_app::Camera, what: &str) {
    let close = (got.px_per_mm - want.px_per_mm).abs() < 1e-3
        && got.centre_mm.distance_to(want.centre_mm) < 1e-3;
    assert!(close, "{what}: {got:?}, want {want:?}");
}

/// The project point under window point `at`.
fn world_at(h: &Harness<'_, dreamcarve_app::Shell>, at: egui::Pos2) -> dc_geometry::Point {
    camera(h).to_world(common::in_pane(h, at))
}

/// Two project points agree to a hundredth of a millimetre.
fn assert_near(got: dc_geometry::Point, want: dc_geometry::Point, what: &str) {
    assert!(
        got.distance_to(want) < 1e-2,
        "{what}: {got:?}, want {want:?}"
    );
}

/// Functional capability 72 and open question 8 (`experience.md`, section
/// 9): the camera is fitted to the stock and shapes when a project opens;
/// a middle-button drag, a Space-drag and a two-finger scroll pan it, the
/// content following the pointer; the wheel and a pinch zoom about the
/// cursor; F fits again, the Viewport's Fit too, and Shift F frames the
/// selection. The status bar shows the cursor in project units.
#[test]
fn pan_zoom_fit_reach_the_camera() {
    let dir = scratch("viewport-camera");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );

    let pane = h.state().layout().viewport;
    let cam = camera(&h);
    assert!(
        (cam.viewport_px.x - f64::from(pane.width())).abs() < 1e-3
            && (cam.viewport_px.y - f64::from(pane.height())).abs() < 1e-3,
        "the camera knows the pane: {cam:?} in {pane:?}"
    );
    let app = h.state().app();
    let fit = dc_app::viewport::camera::fit_bounds(app.project(), app.scene());
    let fit_view = fitted(&h, fit);
    assert_same_view(cam, fit_view, "opened");

    // Middle-button drag: the grabbed point stays under the pointer.
    let from = pane.center();
    let to = from + egui::vec2(40.0, 30.0);
    let grabbed = world_at(&h, from);
    common::drag(&mut h, from, to, egui::PointerButton::Middle);
    assert_near(world_at(&h, to), grabbed, "middle-drag");
    assert!(
        (camera(&h).px_per_mm - fit_view.px_per_mm).abs() < 1e-9,
        "a pan does not zoom"
    );

    // Space-drag with the primary button pans the same way, and selects
    // nothing.
    let from = pane.center() + egui::vec2(-100.0, 50.0);
    let to = from + egui::vec2(-60.0, -25.0);
    let grabbed = world_at(&h, from);
    h.key_down(egui::Key::Space);
    h.run();
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    h.key_up(egui::Key::Space);
    h.run();
    assert_near(world_at(&h, to), grabbed, "Space-drag");
    assert!(
        h.state().app().selection().is_empty(),
        "a pan selects nothing"
    );

    // A two-finger scroll pans by its delta.
    let at = pane.center() + egui::vec2(20.0, 20.0);
    h.hover_at(at);
    h.run();
    let under = world_at(&h, at);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(12.0, -8.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Trackpad,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_near(
        world_at(&h, at + egui::vec2(12.0, -8.0)),
        under,
        "two-finger scroll",
    );

    // The wheel zooms in about the cursor; a pinch by its factor.
    let before = camera(&h);
    let under = world_at(&h, at);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(
        camera(&h).px_per_mm > before.px_per_mm * 1.05,
        "the wheel zooms in"
    );
    assert_near(world_at(&h, at), under, "wheel zoom about the cursor");
    let before = camera(&h);
    h.event(egui::Event::Zoom(1.5));
    h.run();
    assert!(
        (camera(&h).px_per_mm - before.px_per_mm * 1.5).abs() < 1e-6,
        "a pinch zooms by its factor"
    );
    assert_near(world_at(&h, at), under, "pinch about the cursor");

    // The cursor's project coordinates in the status bar, one decimal.
    let p = world_at(&h, at);
    let readout = format!("X {:.1}  Y {:.1}", p.x, p.y);
    assert!(
        h.query_by_label(&readout).is_some(),
        "the status bar reads {readout}"
    );

    // F fits again.
    h.key_press(egui::Key::F);
    h.run();
    assert_same_view(camera(&h), fit_view, "F");

    // Shift F frames the selection: the square.
    let square = dc_project::InstanceId::new("i4");
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![square.clone()]));
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F);
    h.run();
    let bounds = h
        .state()
        .app()
        .scene()
        .bounds_of(&[square])
        .expect("the square has bounds");
    assert_same_view(camera(&h), fitted(&h, bounds), "Shift F");

    // The Viewport's Fit.
    h.get_by_label("Fit").click();
    h.run();
    assert_same_view(camera(&h), fit_view, "the Fit button");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `redesign.md`, "The Viewport's floating controls" (slice D4), top
/// left: *Select* and the *Tab tool* in one pill, the one in use pressed;
/// then *Fit*, *Zoom out*, the zoom as a percentage and *Zoom in*. The
/// zoom controls step the scale by a quarter about the pane's centre, and
/// a click on the percentage sets it to 100 %, an inch of the work on 96
/// pixels. Bottom left: the cursor's read-out, there while the pointer is
/// over the Viewport. None of it is in the status bar.
#[test]
fn floating_tools_select_fit_and_zoom() {
    use egui_kittest::kittest::NodeT as _;

    let dir = scratch("floating-tools");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    let pane = h.state().layout().viewport;
    let rect = |h: &Harness<'_, dreamcarve_app::Shell>, label: &str| {
        h.get_by_role_and_label(Role::Button, label).rect()
    };
    // One row at the pane's top left, in reading order.
    let order = ["Select", "Tab tool", "Fit", "Zoom out", "Zoom in"];
    let rects: Vec<egui::Rect> = order.iter().map(|label| rect(&h, label)).collect();
    for (label, r) in order.iter().zip(&rects) {
        assert!(
            pane.contains_rect(*r) && r.top() < pane.top() + 24.0 && r.left() < pane.center().x,
            "{label} floats at the top left: {r:?}"
        );
    }
    assert!(
        rects
            .windows(2)
            .all(|pair| pair[0].right() <= pair[1].left()),
        "in order: {rects:?}"
    );
    let level = h.get_by_label_contains("Zoom level").rect();
    assert!(rects[3].right() <= level.left() && level.right() <= rects[4].left());
    let status_top = 800.0 - STATUS_BAR_PX;
    assert!(
        h.query_all_by_label_contains("Zoom")
            .all(|node| node.rect().bottom() <= status_top),
        "no zoom in the status bar"
    );

    // Select is the tool in use; the Tab tool takes over and gives back.
    let pressed = |h: &Harness<'_, dreamcarve_app::Shell>, at: egui::Rect| {
        // The pressed wash is a filled rounded rectangle on the button.
        h.output().shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.rect == at && shape.fill == theme::DARK.surface_active)
        })
    };
    assert!(pressed(&h, rects[0]), "Select shows pressed");
    assert!(!pressed(&h, rects[1]));
    h.get_by_role_and_label(Role::Button, "Tab tool").click();
    h.run();
    assert!(pressed(&h, rects[1]), "the Tab tool shows pressed");
    assert!(!pressed(&h, rects[0]));
    h.get_by_role_and_label(Role::Button, "Select").click();
    h.run();
    assert!(pressed(&h, rects[0]), "Select takes the tool back");

    // The zoom: a quarter in, a quarter out, about the pane's centre.
    let fit_view = camera(&h);
    let centre = world_at(&h, pane.center());
    h.get_by_role_and_label(Role::Button, "Zoom in").click();
    h.run();
    assert!((camera(&h).px_per_mm / fit_view.px_per_mm - 1.25).abs() < 1e-9);
    assert_near(world_at(&h, pane.center()), centre, "zoom in");
    let percent = format!("Zoom level: {:.0} %", camera(&h).zoom_percent());
    assert!(
        h.query_by_label(&percent).is_some(),
        "the read-out says {percent}"
    );
    h.get_by_role_and_label(Role::Button, "Zoom out").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Zoom out").click();
    h.run();
    assert!((camera(&h).px_per_mm / fit_view.px_per_mm - 0.8).abs() < 1e-9);
    assert_near(world_at(&h, pane.center()), centre, "zoom out");

    // A click on the percentage: 100 %, 96 px to the inch.
    h.get_by_label_contains("Zoom level").click();
    h.run();
    assert!((camera(&h).px_per_mm - 96.0 / 25.4).abs() < 1e-9);
    assert!(h.query_by_label("Zoom level: 100 %").is_some());
    assert_near(world_at(&h, pane.center()), centre, "100 %");
    // The zoom is the user's now: a wider pane does not fit again.
    go_to(&mut h, "Design");
    assert!((camera(&h).px_per_mm - 96.0 / 25.4).abs() < 1e-9);
    h.get_by_role_and_label(Role::Button, "Fit").click();
    h.run();
    let app = h.state().app();
    let bounds = dc_app::viewport::camera::fit_bounds(app.project(), app.scene());
    assert_same_view(camera(&h), fitted(&h, bounds), "Fit");
    assert!(!h.state().app().is_dirty(), "none of it edits the project");

    // The read-out: the cursor's position, at the pane's bottom left.
    h.event(egui::Event::PointerGone);
    h.run();
    assert!(
        h.query_by_label_contains("X ").is_none(),
        "off the Viewport"
    );
    let pane = h.state().layout().viewport;
    let at = common::on_screen(&h, dc_geometry::Point::new(64.2, 51.8));
    h.event(egui::Event::PointerMoved(at));
    h.run();
    let readout = h.get_by_label("X 64.2  Y 51.8");
    assert!(
        pane.contains_rect(readout.rect())
            && readout.rect().bottom() > pane.bottom() - 48.0
            && readout.rect().left() < pane.left() + 24.0,
        "the read-out floats at the bottom left: {:?}",
        readout.rect()
    );
    assert!(!readout.accesskit_node().is_disabled());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `redesign.md`, "The Viewport's floating controls": they never cover
/// the stock at Fit. At the minimum window with an operation in the
/// Inspector, the Viewport at its narrowest, the stock as Fit frames it
/// is clear of the tools, the colour mode, *Layers* and the read-out; and
/// with By depth on (the polish item of slice D4) it is clear of the
/// legend too and still fills most of the pane's width, where a strip
/// kept beside the legend left it small.
#[test]
fn floating_controls_never_cover_the_stock_at_fit() {
    let dir = scratch("floating-fit");
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        768.0,
    );
    settle_export(&mut h);
    open_operation(&mut h, "Cut out");
    let stock_rect = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let stock = &h.state().app().project().stock;
        egui::Rect::from_two_pos(
            common::on_screen(h, dc_geometry::Point::new(0.0, 0.0)),
            common::on_screen(h, dc_geometry::Point::new(stock.width_mm, stock.height_mm)),
        )
    };
    let controls = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<(String, egui::Rect)> {
        ["Select", "Tab tool", "Fit", "Zoom out", "Zoom in", "Layers"]
            .iter()
            .map(|label| {
                (
                    (*label).to_owned(),
                    h.get_by_role_and_label(Role::Button, label).rect(),
                )
            })
            .chain(["By operation", "By depth"].iter().map(|label| {
                (
                    (*label).to_owned(),
                    h.get_by_role_and_label(Role::RadioButton, label).rect(),
                )
            }))
            .collect()
    };
    let pane = h.state().layout().viewport;
    assert!(
        pane.width() < 560.0,
        "the Viewport is near its least width, 520 px: {pane:?}"
    );

    // The pointer over the stock, so the read-out shows.
    let over = common::on_screen(&h, dc_geometry::Point::new(85.0, 60.0));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    let stock = stock_rect(&h);
    let shown = controls(&h);
    for (label, rect) in &shown {
        assert!(
            pane.contains_rect(*rect),
            "{label} is on the pane: {rect:?}"
        );
        assert!(!rect.intersects(stock), "{label} {rect:?} covers {stock:?}");
    }
    // The two pills do not run into each other.
    let zoom_in = &shown[4].1;
    let by_operation = &shown[6].1;
    assert!(
        zoom_in.right() < by_operation.left(),
        "the pills are apart: {zoom_in:?}, {by_operation:?}"
    );
    let readout = h.get_by_label_contains("X 85.0").rect();
    assert!(!readout.intersects(stock), "the read-out {readout:?}");
    // Nor the origin's label, under the stock's bottom-left corner (row
    // D5 of the worklog: the read-out lay over the `0,0`).
    let origin_label = |h: &Harness<'_, dreamcarve_app::Shell>| {
        text_rects(h, "0,0")
            .into_iter()
            .next()
            .expect("the origin's label")
    };
    let origin = origin_label(&h);
    assert!(
        pane.contains_rect(origin),
        "the origin's label is on the pane: {origin:?}"
    );
    assert!(
        !readout.expand(2.0).intersects(origin),
        "the read-out {readout:?} covers the origin's label {origin:?}"
    );
    let plain_width = stock.width();
    assert!(
        plain_width > 0.75 * pane.width(),
        "the stock fills the pane's width: {plain_width} of {}",
        pane.width()
    );

    // By depth: the legend hangs under the top-right pill, and Fit
    // stands clear of it.
    h.get_by_role_and_label(Role::RadioButton, "By depth")
        .click();
    h.run();
    let stock = stock_rect(&h);
    let legend = [
        "Z (mm)",
        "stock top",
        "Screw holes, Cut out",
        "Grey: rapids",
    ]
    .iter()
    .flat_map(|label| text_rects(&h, label))
    .reduce(|a, b| a.union(b))
    .expect("the legend's labels");
    let layers = h.get_by_role_and_label(Role::Button, "Layers").rect();
    assert!(
        legend.top() >= layers.bottom() && legend.right() <= pane.right(),
        "the legend hangs under the top-right pill: {legend:?}"
    );
    assert!(
        !legend.expand(8.0).intersects(stock),
        "the legend {legend:?} covers the stock {stock:?}"
    );
    for (label, rect) in controls(&h) {
        assert!(!rect.intersects(stock), "{label} {rect:?} covers {stock:?}");
    }
    assert!(
        stock.width() > 0.7 * pane.width(),
        "the stock is not squeezed beside the legend: {} of {}",
        stock.width(),
        pane.width()
    );

    // A square stock, which the pane's height limits: Fit puts its bottom
    // at the bottom inset, and its origin, at its bottom-left corner,
    // over where the read-out floats. The read-out, with the cursor and
    // the Z range in it, stays clear of the origin's label.
    h.get_by_role_and_label(Role::RadioButton, "By operation")
        .click();
    h.run();
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    go_to(&mut h, "Stock");
    let mut square = h.state().app().project().stock.clone();
    square.width_mm = 120.0;
    square.height_mm = 120.0;
    h.state_mut()
        .app_mut()
        .run(Command::SetStock(square))
        .unwrap();
    h.run();
    let visibility = dc_app::Visibility {
        extents: true,
        ..h.state().app().view().visibility
    };
    h.state_mut().app_mut().set_visibility(visibility);
    h.get_by_role_and_label(Role::Button, "Fit").click();
    h.run();
    let over = common::on_screen(&h, dc_geometry::Point::new(60.0, 60.0));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    let pane = h.state().layout().viewport;
    let stock = stock_rect(&h);
    assert!(
        stock.height() > 0.8 * pane.height(),
        "the pane's height limits the stock: {stock:?} in {pane:?}"
    );
    let readout = h.get_by_label_contains("X 60.0").rect();
    let origin = origin_label(&h);
    assert!(
        readout.left() < origin.right() && origin.left() < readout.right(),
        "the origin's label {origin:?} is over the read-out's place {readout:?}"
    );
    assert!(
        pane.contains_rect(origin) && !readout.expand(2.0).intersects(origin),
        "the read-out {readout:?} covers the origin's label {origin:?}"
    );
    assert!(!readout.intersects(stock), "the read-out {readout:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Functional capability 75 (`experience.md`, section 8, "Selection" and
/// "Hover"): a click on a shape's outline selects it and the status bar
/// names it; Shift-click adds a second; Ctrl/⌘-click toggles one out; a
/// click on empty stock clears; a drag on empty stock is a marquee that
/// selects what it touches; Escape clears. The selection is `dc_app`'s,
/// the one the Design Tree reads.
#[test]
fn click_selects_and_escape_clears() {
    let dir = scratch("viewport-select");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let selected = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<String> {
        h.state()
            .app()
            .selection()
            .instances()
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect()
    };

    // The square's left edge (x 70, y 18–48 in project space). Its name
    // is in the Design Tree's rows; hovering it adds it to the status bar.
    let named = |h: &Harness<'_, dreamcarve_app::Shell>| h.query_all_by_label("square").count();
    let in_tree = named(&h);
    let square = common::on_screen(&h, dc_geometry::Point::new(70.0, 33.0));
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    assert_eq!(selected(&h), ["i4"], "a click selects the shape");
    assert_eq!(
        named(&h),
        in_tree + 1,
        "hovering it names it in the status bar"
    );

    // The pocket's left edge (x 35): Shift adds it.
    let pocket = common::on_screen(&h, dc_geometry::Point::new(35.0, 77.0));
    common::click_at(&mut h, pocket, egui::Modifiers::SHIFT);
    assert_eq!(selected(&h), ["i4", "i2"], "Shift-click adds");

    // Ctrl/⌘ toggles the square out.
    common::click_at(&mut h, square, egui::Modifiers::COMMAND);
    assert_eq!(selected(&h), ["i2"], "Ctrl-click toggles");

    // Empty stock clears.
    let empty = common::on_screen(&h, dc_geometry::Point::new(3.0, 3.0));
    common::click_at(&mut h, empty, egui::Modifiers::NONE);
    assert!(
        h.state().app().selection().is_empty(),
        "a click on nothing clears"
    );

    // A marquee from empty stock across the outline's left edge (x 10).
    let to = common::on_screen(&h, dc_geometry::Point::new(12.0, 30.0));
    common::drag(&mut h, empty, to, egui::PointerButton::Primary);
    assert_eq!(selected(&h), ["i1"], "the marquee takes what it touches");

    // Escape clears.
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().app().selection(), &dc_app::Selection::Nothing);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The undo steps the shell's app holds.
fn undo_steps(h: &Harness<'_, dreamcarve_app::Shell>) -> usize {
    h.state().app().history().undo_labels().count()
}

/// The transform of instance `id` in the shell's project.
fn transform_of(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> dc_geometry::Transform {
    h.state()
        .app()
        .project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .unwrap_or_else(|| panic!("the plaque has {id}"))
        .transform
}

/// Two transforms agree to a millionth.
fn assert_same_transform(got: dc_geometry::Transform, want: dc_geometry::Transform, what: &str) {
    let close = got
        .coeffs()
        .iter()
        .zip(want.coeffs())
        .all(|(a, b)| (a - b).abs() < 1e-6);
    assert!(close, "{what}: {got:?}, want {want:?}");
}

/// Capability 58 (`experience.md`, section 8, "Drag to move"): pressing
/// on a shape and moving more than 4 px drags it; while the button is
/// down the project and its history are untouched (the Viewport draws a
/// preview transform, not a snapshot) and the status bar shows the
/// travel; the release commits one `TransformInstances`, one undo step,
/// moved by the pointer's travel in project millimetres (Y up). The
/// arrow keys nudge the selection by 1 mm, 10 mm with Shift and 0.1 mm
/// with Alt, a step each.
#[test]
fn drag_moves_on_release_as_one_step() {
    let dir = scratch("drag-move");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let before = h.state().app().project().clone();
    let steps = undo_steps(&h);
    let square = transform_of(&h, "i4");
    let px_per_mm = h.state().app().view().camera.px_per_mm;

    // The square's left edge, dragged right and up in four frames.
    let from = common::on_screen(&h, dc_geometry::Point::new(70.0, 33.0));
    let travel = egui::vec2(60.0, -40.0);
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
        h.event(egui::Event::PointerMoved(from + travel * t));
        h.run();
        assert_eq!(
            h.state().app().project(),
            &before,
            "frame {k}: the drag previews, it does not edit"
        );
        assert_eq!(undo_steps(&h), steps, "frame {k}: no undo step yet");
    }
    assert!(
        h.query_by_label_contains("ΔX").is_some(),
        "the status bar shows the travel"
    );
    h.event(egui::Event::PointerButton {
        pos: from + travel,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();

    assert_eq!(undo_steps(&h), steps + 1, "the release is one undo step");
    assert_eq!(h.state().app().undo_label(), Some("Move “square”"));
    let dx = f64::from(travel.x) / px_per_mm;
    let dy = -f64::from(travel.y) / px_per_mm;
    assert_same_transform(
        transform_of(&h, "i4"),
        square.then(dc_geometry::Transform::translate(dx, dy)),
        "the square moved by the pointer's travel",
    );
    assert_eq!(
        h.state().app().selection().instances(),
        [dc_project::InstanceId::new("i4")],
        "the dragged shape is the selection"
    );
    assert_eq!(
        transform_of(&h, "i2"),
        before.instances[1].transform,
        "nothing else moved"
    );

    // The arrow keys: 1 mm, 10 mm with Shift, 0.1 mm with Alt.
    let at = transform_of(&h, "i4");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
    h.run();
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    h.run();
    assert_eq!(undo_steps(&h), steps + 4, "a step per nudge");
    assert_same_transform(
        transform_of(&h, "i4"),
        at.then(dc_geometry::Transform::translate(0.9, 10.0)),
        "nudged by 1, 10 and 0.1 mm",
    );

    // Undo walks back to the file as opened.
    for _ in 0..4 {
        h.state_mut().app_mut().undo();
    }
    assert_eq!(h.state().app().project(), &before);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 58 and acceptance 66: a locked shape can be selected but
/// refuses every transform with the reason in the status bar: a drag
/// moves nothing, a nudge moves nothing, and neither is an undo step.
#[test]
fn locked_refuses_with_status_message() {
    let dir = scratch("drag-locked");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    h.state_mut()
        .app_mut()
        .run(Command::SetInstanceFlags {
            ids: vec![dc_project::InstanceId::new("i4")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks the square");
    h.run();
    let locked = h.state().app().project().clone();
    let steps = undo_steps(&h);
    let message = "shape “square” is locked";

    let from = common::on_screen(&h, dc_geometry::Point::new(70.0, 33.0));
    common::drag(
        &mut h,
        from,
        from + egui::vec2(50.0, 30.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(h.state().app().project(), &locked, "the drag moved nothing");
    assert_eq!(undo_steps(&h), steps);
    assert_eq!(
        h.state().message(),
        Some(message),
        "the status bar says why"
    );
    assert!(
        h.query_by_label(message).is_some(),
        "and shows it: {:?}",
        h.state().message()
    );
    assert_eq!(
        h.state().app().selection().instances(),
        [dc_project::InstanceId::new("i4")],
        "a locked shape can still be selected"
    );

    // A nudge is refused the same way.
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(
        h.state().app().project(),
        &locked,
        "the nudge moved nothing"
    );
    assert_eq!(undo_steps(&h), steps);
    assert_eq!(h.state().message(), Some(message));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 66's and 95's clause for a locked shape and the other
/// three transforms (capabilities 58 and 58a), in the window. A drag and a
/// nudge are refused with a status-bar message
/// (`locked_refuses_with_status_message`); a typed value, Rotate and Flip
/// cannot be asked for at all. With the locked square selected on the
/// Design step the Inspector says it is locked, its X, Y, W and H fields
/// are read-only, and Rotate and the two flips are disabled with the
/// reason as their tooltip; pressing one changes nothing.
#[test]
fn locked_shape_has_read_only_fields_and_a_disabled_arrange_group() {
    let dir = scratch("locked-inspector");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.1)
        .build_eframe(|cc| {
            dreamcarve_app::Shell::new(cc, options(&dir.join("config"), Some(plaque), &dialogs))
                .expect("the shell starts")
        });
    h.run();
    h.state_mut()
        .app_mut()
        .run(Command::SetInstanceFlags {
            ids: vec![dc_project::InstanceId::new("i4")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks the square");
    go_to(&mut h, "Design");
    select_only(&mut h, "i4");
    let locked = h.state().app().project().clone();
    let steps = undo_steps(&h);

    assert!(
        h.query_by_label("Locked: cannot be moved or resized. Operations still cut it.")
            .is_some(),
        "the Inspector says why nothing can be typed"
    );
    for field in ["X", "Y", "W", "H"] {
        assert!(
            h.get_by_role_and_label(Role::TextInput, field)
                .accesskit_node()
                .is_disabled(),
            "{field} is read-only"
        );
    }
    let reason = "Every selected shape is locked: unlock one to arrange it";
    assert!(h.query_by_label(reason).is_none(), "not said until asked");
    for name in ["Rotate 90° clockwise", "Flip horizontal", "Flip vertical"] {
        let button = h.get_by_role_and_label(Role::Button, name);
        assert!(button.accesskit_node().is_disabled(), "{name} is disabled");
        button.click();
        h.run();
        assert_eq!(h.state().app().project(), &locked, "{name} turned nothing");
        h.get_by_role_and_label(Role::Button, name).hover();
        wait_for_tooltip(&mut h);
        assert!(
            h.query_by_label(reason).is_some(),
            "{name}'s tooltip says why"
        );
    }
    assert_eq!(undo_steps(&h), steps, "nothing was a step");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 57 in the window (`experience.md`, section 8): a click on a
/// Design Tree row selects the shape, Ctrl/⌘-click adds or removes one,
/// Shift-click selects the range from the last click; a row's eye hides
/// its shape in one step; a double-click renames in place, Enter commits
/// as one undo step.
#[test]
fn design_tree_rows_select_hide_and_rename() {
    let dir = scratch("tree-rows");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let options = options(&dir.join("config"), Some(plaque), &dialogs);
    // Frames 1/60 s apart, so two clicks a frame apart are a double-click
    // (egui's limit is 0.3 s; the harness's default step is 0.25 s).
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options).expect("the shell starts"));
    h.run();
    let selected = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<String> {
        h.state()
            .app()
            .selection()
            .instances()
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect()
    };

    // The Design step, by its key: no click yet.
    command_key(&h, egui::Key::Num1);
    h.run();
    // Double-click renames in place; Enter commits one step. Done first,
    // so no earlier click in the harness counts toward the double-click.
    let at = h.get_by_label("border").rect().center();
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    h.get_by_label("Rename border").type_text("frame");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let names: Vec<String> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.name.clone())
        .collect();
    assert!(names.contains(&"frame".to_owned()), "renamed: {names:?}");
    assert_eq!(h.state().app().undo_label(), Some("Rename “border”"));
    assert_eq!(undo_steps(&h), 1, "the rename is one step");

    h.get_by_label("square").click();
    h.run();
    assert_eq!(selected(&h), ["i4"], "a click selects the row's shape");
    h.get_by_label("pocket")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(selected(&h), ["i4", "i2"], "Ctrl/⌘-click adds");
    h.get_by_label("holes-2")
        .click_modifiers(egui::Modifiers::SHIFT);
    h.run();
    assert_eq!(
        selected(&h),
        ["i4", "i2", "i3", "i5", "i6"],
        "Shift-click adds the range from the last click"
    );
    h.get_by_label("square")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(selected(&h), ["i2", "i3", "i5", "i6"], "and toggles");

    // The island's eye: its row's third "Hide" (after the outline's and
    // the pocket's); the section header's reads "Hide all".
    let steps = undo_steps(&h);
    h.query_all_by_label("Hide")
        .nth(2)
        .expect("each row has an eye")
        .click();
    h.run();
    let island = |h: &Harness<'_, dreamcarve_app::Shell>| {
        h.state()
            .app()
            .project()
            .instances
            .iter()
            .find(|i| i.id.as_str() == "i3")
            .cloned()
            .expect("the island")
    };
    assert!(!island(&h).visible, "the eye hid the island");
    assert_eq!(h.state().app().undo_label(), Some("Hide “island”"));
    assert_eq!(undo_steps(&h), steps + 1);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 60 and acceptance 69 (#536): the park point is among the
/// Export step's Heights, typed or clicked on the Viewport. *Click on the
/// view* arms the Viewport; the next
/// click there sets `heights.park` to the project point the camera puts
/// under the cursor (`Camera::to_world`, the one Y flip), one undo step,
/// disarms it, and selects nothing. Typing `20, 30` into the field and
/// pressing Enter writes the same field.
#[test]
fn park_point_click_on_the_viewport() {
    let dir = scratch("park");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    assert!(h.state().app().selection().is_empty());
    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    assert!(
        h.query_by_role_and_label(Role::Button, "Heights").is_some(),
        "the Export step has the Heights"
    );
    assert_eq!(h.state().app().project().heights.park, None);
    // The group is closed until it is wanted.
    assert!(h.query_by_label("Click on the view").is_none());
    h.get_by_role_and_label(Role::Button, "Heights").click();
    h.run();

    h.get_by_label("Click on the view").scroll_to_me();
    h.run();
    h.get_by_label("Click on the view").click();
    h.run();
    // Empty stock near the plaque's bottom-right corner (stock 170 × 120),
    // clear of the read-out at the pane's bottom-left.
    let want = dc_geometry::Point::new(165.0, 5.0);
    let at = common::on_screen(&h, want);
    let under = world_at(&h, at);
    common::click_at(&mut h, at, egui::Modifiers::NONE);

    let park = h
        .state()
        .app()
        .project()
        .heights
        .park
        .expect("the click sets the park point");
    assert_near(
        dc_geometry::Point::new(park.x_mm, park.y_mm),
        under,
        "the park point is the point under the click",
    );
    assert!(
        under.distance_to(want) < 0.5,
        "the camera maps the click back: {under:?}"
    );
    assert_eq!(h.state().app().undo_label(), Some("Set heights"));
    assert!(h.state().app().selection().is_empty(), "nothing selected");

    // One click: the Viewport is disarmed after it.
    let elsewhere = common::on_screen(&h, dc_geometry::Point::new(3.0, 3.0));
    common::click_at(&mut h, elsewhere, egui::Modifiers::NONE);
    assert_eq!(h.state().app().project().heights.park, Some(park));

    // The typed entry writes the same field.
    h.get_by_role_and_label(Role::TextInput, "Park point")
        .focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Park point")
        .set_text_value_accesskit("20, 30");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(
        h.state().app().project().heights.park,
        Some(dc_project::ParkXy {
            x_mm: 20.0,
            y_mm: 30.0
        })
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 69's clause "the Viewport's crosshair and park mark move",
/// on what the Viewport paints. The origin's crosshair is four arms that
/// start 3 px from the work zero, with the label `0,0` beside them; the
/// park mark is a 6-px ring. On the plaque the arms and the label stand
/// at the stock's bottom-left corner; with the origin set to the stock's
/// centre they stand there, the label as far from the zero as before, and
/// none is left at the corner. A park point set at (20, 30) draws the
/// ring there, and moved to (100, 60) the ring moves with it.
#[test]
fn origin_crosshair_and_park_mark_move_on_the_viewport() {
    let dir = scratch("marks-move");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    // The crosshair's arms: line segments with an end 3 px from `at`.
    let arms_at = |h: &Harness<'_, dreamcarve_app::Shell>, at: egui::Pos2| {
        h.output()
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { points, .. } => {
                    points.iter().any(|p| (p.distance(at) - 3.0).abs() < 0.26)
                }
                _ => false,
            })
            .count()
    };
    // The park mark's rings: 6-px circles centred on `at`.
    let rings_at = |h: &Harness<'_, dreamcarve_app::Shell>, at: egui::Pos2| {
        h.output()
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) => {
                    circle.center.distance(at) < 0.5 && (circle.radius - 6.0).abs() < 0.01
                }
                _ => false,
            })
            .count()
    };
    let label = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let rects = text_rects(h, "0,0");
        assert_eq!(rects.len(), 1, "one origin label: {rects:?}");
        rects[0].right_top()
    };
    let zero = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let marks = dc_app::project::marks(h.state().app().project());
        (common::on_screen(h, marks.origin_mm), marks)
    };

    let (corner, marks) = zero(&h);
    assert_eq!(marks.origin_mm, dc_geometry::Point::new(0.0, 0.0));
    assert_eq!(marks.park_mm, None);
    assert_eq!(arms_at(&h, corner), 4, "the crosshair is at the corner");
    let beside = label(&h) - corner;

    let centre = dc_app::project::set_origin_xy(
        h.state().app().project(),
        dc_app::project::OriginPreset::Centre,
    )
    .expect("the origin changes");
    h.state_mut()
        .app_mut()
        .run(centre)
        .expect("the origin is set");
    h.run();
    let (middle, marks) = zero(&h);
    assert_eq!(marks.origin_mm, dc_geometry::Point::new(85.0, 60.0));
    assert!(middle.distance(corner) > 50.0, "the zero moved on screen");
    assert_eq!(arms_at(&h, middle), 4, "the crosshair is at the centre");
    let old_corner = common::on_screen(&h, dc_geometry::Point::new(0.0, 0.0));
    assert_eq!(arms_at(&h, old_corner), 0, "and no longer at the corner");
    let now = label(&h) - middle;
    assert!(
        (now - beside).length() < 0.5,
        "the label moved with it: {now:?} against {beside:?}"
    );

    for (x_mm, y_mm) in [(20.0, 30.0), (100.0, 60.0)] {
        let before = h.state().app().project().heights.park;
        let at_mm = dc_geometry::Point::new(x_mm, y_mm);
        let park = dc_app::project::set_park(h.state().app().project(), at_mm)
            .expect("the park point changes");
        h.state_mut().app_mut().run(park).expect("the park is set");
        h.run();
        assert_eq!(
            rings_at(&h, common::on_screen(&h, at_mm)),
            1,
            "the park mark is at ({x_mm}, {y_mm})"
        );
        if let Some(old) = before {
            let old = common::on_screen(&h, dc_geometry::Point::new(old.x_mm, old.y_mm));
            assert_eq!(rings_at(&h, old), 0, "and gone from where it was");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 70's offers, in the window (capability 62): on the Export
/// step's Post-processor group, for each of the six bundled
/// post-processors in turn, *Tool change* lists *One program with M6*
/// exactly where the definition's tool change has `M6` (grblHAL, and not
/// GRBL), and *Canned drill cycles* is a checkbox exactly where the
/// definition has canned cycles, which is LinuxCNC and Mach3; elsewhere
/// its place holds the line that says the controller has none. The
/// post-processor is switched by the Controller dropdown's own command.
#[test]
fn export_step_offers_m6_and_canned_cycles_where_the_post_processor_has_them() {
    let dir = scratch("post-offers");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        900.0,
    );
    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    h.get_by_role_and_label(Role::Button, "Post-processor")
        .click();
    h.run();

    let mut with_canned = Vec::new();
    let mut with_m6 = Vec::new();
    for definition in dc_project::latest_definitions().expect("bundled") {
        let post = definition.post_processor();
        if h.state().app().project().post_processor.id != definition.id {
            h.state_mut()
                .app_mut()
                .run(Command::SwitchPost {
                    id: definition.id.to_owned(),
                })
                .expect("the post-processor switches");
            h.run();
        }
        let id = definition.id;

        let checkbox = h
            .query_by_role_and_label(Role::CheckBox, "Canned drill cycles")
            .is_some();
        let none = format!("Not offered: {} has no canned cycles.", post.name);
        assert_eq!(checkbox, post.canned_cycles.is_some(), "{id}: the checkbox");
        assert_eq!(
            h.query_by_label(&none).is_some(),
            post.canned_cycles.is_none(),
            "{id}: the line in its place"
        );
        if checkbox {
            with_canned.push(id);
        }

        h.get_by_role_and_label(Role::ComboBox, "Tool change")
            .scroll_to_me();
        h.run();
        h.get_by_role_and_label(Role::ComboBox, "Tool change")
            .click();
        h.run();
        assert!(
            h.query_by_label("One program per tool").is_some(),
            "{id}: the list is open"
        );
        let m6 = h.query_by_label("One program with M6").is_some();
        assert_eq!(m6, post.tool_change.has_m6(), "{id}: one program with M6");
        if m6 {
            with_m6.push(id);
        }
        h.key_press(egui::Key::Escape);
        h.run();
    }
    with_canned.sort_unstable();
    assert_eq!(with_canned, ["linuxcnc", "mach3"]);
    assert!(with_m6.contains(&"grblhal") && !with_m6.contains(&"grbl"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 68 in the window (capability 59): the plaque's `outline`
/// deleted with the Delete key leaves the Cut out Profile with no target,
/// and the Warnings drawer lists validation's `operation_has_no_targets`
/// error for it, in the error's own words; undo takes the row away again.
#[test]
fn deleting_a_target_lists_the_no_targets_error_in_the_drawer() {
    let dir = scratch("delete-target");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    go_to(&mut h, "Design");
    select_only(&mut h, "i1");
    h.key_press(egui::Key::Delete);
    h.run();
    settle_generation(&mut h, "the plaque without its outline");
    let project = h.state().app().project();
    assert!(project.instances.iter().all(|i| i.id.as_str() != "i1"));
    let problems = dc_project::validate(project);
    let message = problems
        .iter()
        .find(|p| p.code() == "operation_has_no_targets")
        .expect("the Cut out Profile has no target left")
        .to_string();
    assert!(message.contains("Cut out"), "{message}");

    // The drawer's row is `check`'s line for it: `error: ` and the
    // message, with no code.
    let line = format!("error: {message}");
    assert!(h.query_by_label(&line).is_none(), "the drawer is closed");
    h.get_by_label("Warnings").click();
    h.run();
    assert!(h.state().warnings_open());
    assert!(
        h.query_all_by_label(&line).count() >= 1,
        "the drawer lists the error: {line}"
    );
    // Capability 71: it cannot be worked out, so it draws nothing, where
    // it kept its old toolpath; the other four keep theirs.
    let cut_out = dc_project::OperationId::new("op4");
    let toolpaths = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<(String, bool)> {
        let scene = h.state().app().scene();
        let drawn = scene.operations().iter();
        drawn
            .map(|op| (op.id.to_string(), !op.runs.is_empty()))
            .collect()
    };
    for (id, drawn) in toolpaths(&h) {
        assert_eq!(drawn, id != cut_out.as_str(), "{id} after the delete");
    }

    command_key(&h, egui::Key::Z);
    h.run();
    settle_generation(&mut h, "the plaque again");
    assert!(
        toolpaths(&h).iter().all(|(_, drawn)| *drawn),
        "undo brings its toolpath back"
    );
    assert!(
        h.state()
            .app()
            .project()
            .instances
            .iter()
            .any(|i| i.id.as_str() == "i1"),
        "undo brings the outline back"
    );
    assert_eq!(
        h.query_all_by_label_contains(&message).count(),
        0,
        "and the row goes"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file dropped on the window: a path on disk.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Drops `path` on the window, as the OS hands a dropped file over. One
/// frame takes the drop. The window is then left to settle, unless the
/// file is a picture being decoded on its thread: that is
/// `common::settle_picture`'s to wait for, a frame at a time, since a
/// `run` the picture lands in the middle of has too few frames left for
/// the sheet it opens.
fn drop_file(h: &mut Harness<'_, dreamcarve_app::Shell>, path: &std::path::Path) {
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path.to_path_buf())));
    h.step();
    if !h.state().opening_picture() {
        h.run();
    }
}

/// Capability 55 (#535): a dropped `.svg` opens the Import drawing sheet,
/// which names the file and the shapes it holds and offers no options for
/// an SVG; nothing is imported until *Import*, which adds the shapes as
/// one undo step and selects them. A dropped DXF that declares no units
/// opens the sheet with *Import* disabled and the importer's message
/// until units are chosen; closing the sheet imports nothing. A dropped
/// `.dcv` still opens the project
/// (slice 3), through the unsaved-changes sheet when there are changes.
#[test]
fn drop_of_svg_opens_the_import_sheet() {
    use egui_kittest::kittest::NodeT as _;

    let dir = scratch("drop-drawing");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    assert!(h.query_by_role(Role::Dialog).is_none());

    let sign = common::fixture("sign.svg");
    drop_file(&mut h, &sign);
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label("Add drawing to project").is_some(),
        "the sheet's title"
    );
    assert!(
        sheet.query_by_label_contains("sign.svg").is_some(),
        "the sheet names the file"
    );
    assert!(
        sheet.query_by_label_contains("closed").is_some(),
        "and counts its shapes"
    );
    assert!(
        sheet.query_all_by_label("Join tolerance").next().is_none()
            && sheet.query_all_by_label("Units").next().is_none(),
        "an SVG has no options"
    );
    assert!(
        h.state().app().project().instances.is_empty(),
        "nothing is imported before Import"
    );

    let (import, _) = dc_import::import_svg(
        &std::fs::read(&sign).unwrap(),
        &dc_import::SvgImportOptions::default(),
    )
    .unwrap();
    // Mock-up 02: the footer counts what it adds.
    let add = format!("Add {} shapes", import.instances.len());
    h.get_by_role(Role::Dialog).get_by_label(&add).click();
    h.run();
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "{add} closes the sheet"
    );
    let app = h.state().app();
    assert_eq!(app.project().instances.len(), import.instances.len());
    assert_eq!(
        app.selection().instances().len(),
        import.instances.len(),
        "the new shapes are selected"
    );
    assert_eq!(app.undo_label(), Some("Import sign.svg"));
    assert!(app.is_dirty());

    // A DXF with no `$INSUNITS`: Import waits for the units.
    let no_units = common::fixture("dxf/no_units.dxf");
    drop_file(&mut h, &no_units);
    let refusal = dc_import::import_dxf(
        &std::fs::read(&no_units).unwrap(),
        &dc_import::DxfImportOptions::new(),
    )
    .unwrap_err()
    .to_string();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label_contains(&refusal).is_some(),
        "the sheet shows the importer's message: {refusal}"
    );
    assert!(
        sheet.query_all_by_label("Join tolerance").next().is_some(),
        "a DXF has options"
    );
    assert!(
        sheet
            .get_by_label("Add shapes")
            .accesskit_node()
            .is_disabled(),
        "Import is disabled until units are chosen"
    );
    // No Cancel button (`experience.md`, section 13, item 76): the
    // header's close button closes the sheet.
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label("Cancel")
            .is_none()
    );
    h.get_by_role(Role::Dialog).get_by_label("Close").click();
    h.run();
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "Close closes the sheet"
    );
    assert_eq!(
        h.state().app().undo_label(),
        Some("Import sign.svg"),
        "and imports nothing"
    );

    // A dropped project still opens, asking first about the import.
    let plaque = common::fixture("plaque.dcv");
    drop_file(&mut h, &plaque);
    click_in_sheet(&mut h, "Don't Save");
    assert_eq!(h.state().app().source(), Some(plaque.as_path()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 51 and acceptance 63 (#535): File ▸ New Project from
/// Drawing… asks
/// for a drawing, shows it in the same sheet, and *Create project* makes
/// the project `dc_app::App::new_from_drawing` makes: `init`'s stock and
/// shapes, no tool, no operation, no file yet.
#[test]
fn new_from_drawing_through_the_sheet() {
    let dir = scratch("new-from-drawing");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    let coaster = common::repo_root().join("fixtures/coaster.svg");
    dialogs.answer(&coaster);
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("New Project from Drawing…").click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_drawing"]);
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label("New project from drawing").is_some());
    sheet.get_by_label("Create project").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none());

    let bytes = std::fs::read(&coaster).unwrap();
    let model = dc_app::import::DrawingImport::open(
        dc_app::import::DrawingFormat::Svg,
        "coaster.svg",
        bytes,
        dc_app::import::Purpose::New,
    );
    let mut expected = dc_app::App::new(
        dc_app::App::new_project().unwrap(),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    expected.new_from_drawing(&model).unwrap();
    let app = h.state().app();
    assert_eq!(app.project(), expected.project());
    assert!(app.project().tools.is_empty() && app.project().operations.is_empty());
    assert_eq!(app.source(), None);
    assert!(!app.can_undo());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Types `text` into the text field named `label` and leaves it, so the
/// field commits what was typed.
fn type_in(h: &mut Harness<'_, dreamcarve_app::Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
}

/// What the text field named `label` shows.
fn field_value(h: &Harness<'_, dreamcarve_app::Shell>, label: &str) -> String {
    h.get_by_role_and_label(Role::TextInput, label)
        .accesskit_node()
        .value()
        .unwrap_or_default()
}

/// The project's bitmap `id`.
fn bitmap_of(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> dc_project::Bitmap {
    h.state()
        .app()
        .project()
        .bitmaps
        .iter()
        .find(|b| b.id.as_str() == id)
        .unwrap_or_else(|| panic!("no bitmap {id}"))
        .clone()
}

/// Where the wide control named `label` is: a menu's item or a button
/// with its text, not the 20-px glyph button of a tree row that shares
/// the name (a row's eye is *Hide* and its padlock *Lock* or *Unlock*).
fn wide_button(h: &Harness<'_, dreamcarve_app::Shell>, label: &str) -> Option<egui::Pos2> {
    h.query_all_by_label(label)
        .find(|n| n.rect().width() > 40.0)
        .map(|n| n.rect().center())
}

/// The ids of the selected pictures.
fn selected_pictures(h: &Harness<'_, dreamcarve_app::Shell>) -> Vec<String> {
    h.state()
        .app()
        .selected_pictures()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect()
}

/// Capability 56 (slice D6): a dropped `.png` opens the Add picture
/// sheet, which names the file and its size in pixels and asks for the
/// width or the height; *Add picture* is disabled, saying why, until one
/// is typed, and the other side follows the pixels (64 × 32, so 100 wide
/// is 50 high). *Add picture* adds the bitmap `image add` adds, at the
/// stock's corner, as one undo step, and selects it: its row is in the
/// Design Tree and its Properties in the Inspector. A colour JPEG lists
/// its conversion to grayscale; a file that is not a picture is refused
/// in the sheet; a file of no known kind is refused in the status bar.
#[test]
fn drop_of_png_opens_the_picture_sheet() {
    let dir = scratch("drop-picture");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    assert!(h.query_by_role(Role::Dialog).is_none());

    let gradient = common::fixture("photo/gradient.png");
    drop_file(&mut h, &gradient);
    common::settle_picture(&mut h);
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label("Add picture to project").is_some(),
        "the sheet's title"
    );
    assert!(
        sheet.query_by_label_contains("gradient.png").is_some(),
        "the sheet names the file"
    );
    assert!(
        sheet.query_by_label("PNG · 64 × 32 px").is_some(),
        "and its size in pixels"
    );
    assert!(
        sheet
            .get_by_label("Add picture")
            .accesskit_node()
            .is_disabled(),
        "nothing to add until a size is typed"
    );
    assert!(
        sheet
            .query_by_label("Type the picture's width or its height")
            .is_some(),
        "and the sheet says so"
    );
    assert!(
        h.state().app().project().bitmaps.is_empty(),
        "nothing is added before Add picture"
    );

    // The width typed: the height follows the pixels.
    type_in(&mut h, "Width", "100");
    assert_eq!(field_value(&h, "Height"), "50");
    // The height typed instead: the width follows.
    type_in(&mut h, "Height", "20");
    assert_eq!(field_value(&h, "Width"), "40");
    // Not a length: refused under the field, and the size stays.
    type_in(&mut h, "Width", "wide");
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label_contains("wide")
            .is_some(),
        "the refusal names what was typed"
    );
    type_in(&mut h, "Width", "100");
    assert_eq!(field_value(&h, "Height"), "50");
    h.get_by_role(Role::Dialog)
        .get_by_label("Add picture")
        .click();
    h.run();
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "Add picture closes the sheet"
    );

    let bitmap = bitmap_of(&h, "b1");
    assert_eq!(bitmap.name, "gradient");
    assert_eq!((bitmap.width_px, bitmap.height_px), (64, 32));
    assert!(
        bitmap.x_mm.abs() < 1e-9 && bitmap.y_mm.abs() < 1e-9,
        "at the stock's corner: {bitmap:?}"
    );
    assert!((bitmap.width_mm - 100.0).abs() < 1e-9 && (bitmap.height_mm - 50.0).abs() < 1e-9);
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Import gradient.png"));
    assert_eq!(undo_steps(&h), 1, "one undo step");
    assert_eq!(selected_pictures(&h), ["b1"], "the new picture is selected");
    assert_eq!(h.state().step(), dc_app::steps::Step::Design);
    let row = h.get_by_role_and_label(Role::TreeItem, "gradient");
    assert_eq!(
        row.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "its row in the Design Tree, selected"
    );
    assert!(
        h.query_by_label("Properties · gradient").is_some(),
        "and its Properties in the Inspector"
    );
    assert!(
        h.query_by_label("Start with a drawing").is_none(),
        "a project with a picture is past the start screen"
    );

    // A colour JPEG: the sheet lists what decoding converted.
    let colour = common::fixture("photo/colour.jpg");
    drop_file(&mut h, &colour);
    common::settle_picture(&mut h);
    let (_, warnings) = dc_import::bitmap::import(
        &std::fs::read(&colour).unwrap(),
        dc_import::ImageFormat::Jpeg,
    )
    .unwrap();
    let note = warnings.first().expect("a conversion note").to_string();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label_contains("JPEG").is_some());
    assert!(
        sheet.query_by_label(&note).is_some(),
        "the grayscale note: {note}"
    );
    sheet.get_by_label("Close").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none());
    assert_eq!(undo_steps(&h), 1, "Close adds nothing");

    // Bytes that are no picture: the sheet says so and adds nothing.
    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"not a picture").unwrap();
    drop_file(&mut h, &broken);
    common::settle_picture(&mut h);
    let refusal = dc_import::bitmap::import(b"not a picture", dc_import::ImageFormat::Png)
        .unwrap_err()
        .to_string();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label(&refusal).is_some(),
        "the importer's message: {refusal}"
    );
    assert!(
        sheet
            .get_by_label("Add picture")
            .accesskit_node()
            .is_disabled()
    );
    sheet.get_by_label("Close").click();
    h.run();

    // A file of no kind DreamCarve reads: refused, naming it.
    let notes = dir.join("notes.txt");
    std::fs::write(&notes, b"hello").unwrap();
    drop_file(&mut h, &notes);
    common::settle_picture(&mut h);
    assert!(h.query_by_role(Role::Dialog).is_none());
    assert_eq!(
        h.state().message(),
        Some(
            "notes.txt is not a project (.dcv), a drawing (.svg, .dxf) or a picture (.png, \
             .jpg, .jpeg)"
        )
    );
    assert_eq!(undo_steps(&h), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 56 and 57 (slice D6): the Design step's *Add picture*
/// asks for a picture and shows it in the sheet; added, the picture is a
/// row of the Design Tree's Pictures section with an eye, a padlock and
/// its name. The eye hides it as one step, F2 renames it in place, the
/// section header's eye acts on every picture, the row's menu offers a
/// Photo V-Carve on it, and Delete deletes it. File ▸ Add Picture to
/// Project… is the same command.
#[test]
fn add_picture_and_see_its_tree_row() {
    let dir = scratch("add-picture");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    let gradient = common::fixture("photo/gradient.png");
    dialogs.answer(&gradient);
    button_named(&h, "Add picture").click();
    common::settle_picture(&mut h);
    assert_eq!(dialogs.asked(), ["open_picture"]);
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label("Add picture to project")
            .is_some()
    );
    type_in(&mut h, "Height", "20");
    click_in_sheet(&mut h, "Add picture");
    let bitmap = bitmap_of(&h, "b1");
    assert!((bitmap.width_mm - 40.0).abs() < 1e-9 && (bitmap.height_mm - 20.0).abs() < 1e-9);

    // The Pictures section: its header, then the row.
    let header = h.get_by_role_and_label(Role::Heading, "Pictures").rect();
    let row = h.get_by_role_and_label(Role::TreeItem, "gradient").rect();
    assert!(header.bottom() <= row.top() + 0.5, "the row is under it");
    assert!(
        h.query_by_label("No shapes yet.").is_none(),
        "the tree shows once there is a picture"
    );

    // The row's eye: hidden as one step, and shown again.
    h.get_by_label("Hide").click();
    h.run();
    assert!(!bitmap_of(&h, "b1").visible);
    assert_eq!(h.state().app().undo_label(), Some("Hide “gradient”"));
    assert!(
        h.state().app().scene().bitmaps().is_empty(),
        "a hidden picture is not drawn"
    );
    // The header's eye acts on every picture.
    h.get_by_label("Show all pictures").click();
    h.run();
    assert!(bitmap_of(&h, "b1").visible);

    // F2 renames the selected picture in its row.
    h.get_by_role_and_label(Role::TreeItem, "gradient").click();
    h.run();
    assert_eq!(selected_pictures(&h), ["b1"]);
    h.key_press(egui::Key::F2);
    h.run();
    let field = h.get_by_role_and_label(Role::TextInput, "Rename gradient");
    assert_eq!(field.accesskit_node().value().as_deref(), Some("gradient"));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("portrait".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(bitmap_of(&h, "b1").name, "portrait");
    assert!(
        h.query_by_role_and_label(Role::TreeItem, "portrait")
            .is_some()
    );

    // The row's menu: a picture's items, and a Photo V-Carve on it.
    h.get_by_role_and_label(Role::TreeItem, "portrait")
        .click_secondary();
    h.run();
    for item in [
        "Rename",
        "Hide",
        "Lock",
        "Delete",
        "Zoom to",
        "Add operation",
    ] {
        assert!(wide_button(&h, item).is_some(), "{item} in the row's menu");
    }
    assert!(
        h.query_by_label("Duplicate").is_none(),
        "a picture has no linked copy"
    );
    let lock = wide_button(&h, "Lock").expect("the menu's Lock");
    common::click_at(&mut h, lock, egui::Modifiers::NONE);
    assert!(bitmap_of(&h, "b1").locked);

    // File ▸ Add Picture to Project… asks the same dialog.
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Add Picture to Project…").click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_picture", "open_picture"]);
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "the dialog was cancelled"
    );

    // Delete deletes the selected picture.
    h.get_by_role_and_label(Role::TreeItem, "portrait").click();
    h.run();
    h.key_press(egui::Key::Delete);
    h.run();
    assert!(h.state().app().project().bitmaps.is_empty());
    assert!(h.state().app().can_undo());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 51 (slice D6): the start screen's *New from picture…* (and
/// File ▸ New Project from Picture…) asks for a picture, shows it in the
/// same sheet titled for a new project, and *Create project* makes the
/// project `dc_app::App::new_from_picture` makes: `init`'s stock round
/// the picture, the bitmap, the V-bit and one Photo V-Carve, with no file
/// yet and nothing to undo.
#[test]
fn new_from_picture_through_the_sheet() {
    let dir = scratch("new-from-picture");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    let gradient = common::fixture("photo/gradient.png");
    dialogs.answer(&gradient);
    button_named(&h, "New from picture…").click();
    common::settle_picture(&mut h);
    assert_eq!(dialogs.asked(), ["open_picture"]);
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label("New project from picture").is_some());
    assert!(
        sheet
            .get_by_label("Create project")
            .accesskit_node()
            .is_disabled(),
        "a size first"
    );
    type_in(&mut h, "Width", "100");
    click_in_sheet(&mut h, "Create project");
    assert!(h.query_by_role(Role::Dialog).is_none());

    let mut model = dc_app::import::PictureImport::open(
        dc_import::ImageFormat::Png,
        "gradient.png",
        &std::fs::read(&gradient).unwrap(),
        dc_app::import::Purpose::New,
    );
    model.set_width_mm(100.0);
    let mut expected = dc_app::App::new(
        dc_app::App::new_project().unwrap(),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    expected.new_from_picture(&model).unwrap();
    let app = h.state().app();
    assert_eq!(app.project(), expected.project());
    assert_eq!(app.project().bitmaps.len(), 1);
    assert!(matches!(
        app.project().operations.as_slice(),
        [dc_project::Operation::PhotoVCarve(_)]
    ));
    assert_eq!(app.source(), None);
    assert!(!app.can_undo());
    assert!(
        h.query_by_label("Start with a drawing").is_none(),
        "the start screen is gone"
    );

    // The File menu's item asks the same dialog, after the unsaved sheet
    // has had its say when there are changes (there are none here).
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("New Project from Picture…").click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_picture", "open_picture"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A project round the gradient picture, 100 mm wide (bitmap `b1`, 100 ×
/// 50 mm at 10,10, with its Photo V-Carve `op1`), saved in `dir`.
fn picture_project(dir: &std::path::Path) -> std::path::PathBuf {
    let gradient = common::fixture("photo/gradient.png");
    let mut model = dc_app::import::PictureImport::open(
        dc_import::ImageFormat::Png,
        "gradient.png",
        &std::fs::read(gradient).unwrap(),
        dc_app::import::Purpose::New,
    );
    model.set_width_mm(100.0);
    let mut app = dc_app::App::new(
        dc_app::App::new_project().unwrap(),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    app.new_from_picture(&model).unwrap();
    let path = dir.join("photo.dcv");
    std::fs::write(&path, app.serialize().unwrap()).unwrap();
    path
}

/// Capabilities 56 to 58 (slice D6): a picture is a thing to select and
/// place. A click inside its footprint selects it and shows its
/// Properties; a drag moves it, previewed until the release, as one undo
/// step; the arrow keys nudge it; X typed in Properties moves it and W
/// resizes it about its centre with the height following; a corner
/// handle resizes it the same way; a marquee from empty stage takes it;
/// locked, it refuses a drag with the reason in the status bar; Escape
/// clears the selection.
#[test]
fn picture_is_selected_and_moved_on_the_viewport() {
    let dir = scratch("picture-move");
    let photo = picture_project(&dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(photo), &dialogs),
        1280.0,
        800.0,
    );
    go_to(&mut h, "Design");
    let start = bitmap_of(&h, "b1");
    assert!((start.x_mm - 10.0).abs() < 1e-9 && (start.y_mm - 10.0).abs() < 1e-9);
    assert!(selected_pictures(&h).is_empty());
    assert!(h.query_by_label("Properties · gradient").is_none());
    let steps = undo_steps(&h);

    // A click inside the footprint selects the picture.
    let inside = common::on_screen(&h, dc_geometry::Point::new(60.0, 35.0));
    common::click_at(&mut h, inside, egui::Modifiers::NONE);
    assert_eq!(selected_pictures(&h), ["b1"]);
    assert!(h.query_by_label("Properties · gradient").is_some());
    assert_eq!(field_value(&h, "X"), "10");
    assert_eq!(field_value(&h, "W"), "100");
    assert_eq!(field_value(&h, "H"), "50");
    assert_eq!(field_value(&h, "Pixels"), "64 × 32 px");
    assert!(
        h.get_by_role_and_label(Role::TextInput, "Pixels")
            .accesskit_node()
            .is_disabled(),
        "the pixels are the picture: read-only"
    );
    assert_eq!(undo_steps(&h), steps, "selecting is no undo step");

    // A drag moves it: a preview until the release, then one step. (The
    // Inspector has opened, so the fitted view has a new scale.)
    let px_per_mm = h.state().app().view().camera.px_per_mm;
    let inside = common::on_screen(&h, dc_geometry::Point::new(60.0, 35.0));
    let travel = egui::vec2(40.0, -24.0);
    h.event(egui::Event::PointerMoved(inside));
    h.event(egui::Event::PointerButton {
        pos: inside,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=4_u8 {
        h.event(egui::Event::PointerMoved(
            inside + travel * (f32::from(k) / 4.0),
        ));
        h.run();
        assert_eq!(bitmap_of(&h, "b1"), start, "frame {k}: a preview only");
    }
    assert!(
        h.query_by_label_contains("ΔX").is_some(),
        "the status bar shows the travel"
    );
    h.event(egui::Event::PointerButton {
        pos: inside + travel,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(undo_steps(&h), steps + 1, "the release is one undo step");
    assert_eq!(h.state().app().undo_label(), Some("Move “gradient”"));
    let moved = bitmap_of(&h, "b1");
    let (dx, dy) = (
        f64::from(travel.x) / px_per_mm,
        -f64::from(travel.y) / px_per_mm,
    );
    assert!(
        (moved.x_mm - start.x_mm - dx).abs() < 1e-3 && (moved.y_mm - start.y_mm - dy).abs() < 1e-3,
        "moved by the pointer's travel, Y up: {moved:?}"
    );
    assert!(
        (moved.width_mm - 100.0).abs() < 1e-9,
        "a move keeps its size"
    );

    // The arrow keys nudge it, a step each.
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
    h.run();
    let nudged = bitmap_of(&h, "b1");
    assert!((nudged.x_mm - moved.x_mm - 1.0).abs() < 1e-6);
    assert!((nudged.y_mm - moved.y_mm - 10.0).abs() < 1e-6);
    assert_eq!(undo_steps(&h), steps + 3);

    // Properties: X moves it; W resizes it about its centre, H following.
    type_in(&mut h, "X", "20");
    assert!((bitmap_of(&h, "b1").x_mm - 20.0).abs() < 1e-9);
    let before = bitmap_of(&h, "b1");
    type_in(&mut h, "W", "60");
    let resized = bitmap_of(&h, "b1");
    assert!((resized.width_mm - 60.0).abs() < 1e-9 && (resized.height_mm - 30.0).abs() < 1e-9);
    assert!(
        (resized.x_mm + 30.0 - (before.x_mm + 50.0)).abs() < 1e-6
            && (resized.y_mm + 15.0 - (before.y_mm + 25.0)).abs() < 1e-6,
        "about its centre"
    );
    assert_eq!(h.state().app().undo_label(), Some("Resize “gradient”"));
    assert_eq!(field_value(&h, "H"), "30");

    // The top-right corner handle, dragged outward along the diagonal:
    // a resize about the centre with the proportions kept.
    let at = bitmap_of(&h, "b1");
    let corner = dc_geometry::Point::new(at.x_mm + at.width_mm, at.y_mm + at.height_mm);
    let from = common::on_screen(&h, corner);
    let outward = egui::vec2(20.0, -10.0);
    common::drag(&mut h, from, from + outward, egui::PointerButton::Primary);
    let grown = bitmap_of(&h, "b1");
    assert!(grown.width_mm > at.width_mm + 1.0, "it grew: {grown:?}");
    assert!(
        (grown.height_mm * 2.0 - grown.width_mm).abs() < 1e-3,
        "two to one, as its pixels are"
    );
    assert!(
        (grown.x_mm + grown.width_mm / 2.0 - (at.x_mm + at.width_mm / 2.0)).abs() < 1e-3,
        "about its centre"
    );
    assert_eq!(h.state().app().undo_label(), Some("Resize “gradient”"));

    // Escape clears the selection, and the Inspector goes. (The first
    // press leaves the field the keyboard is still in.)
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(selected_pictures(&h).is_empty());
    assert!(h.query_by_label("Properties · gradient").is_none());

    // A marquee from the empty stage across the picture takes it.
    let now = bitmap_of(&h, "b1");
    let outside = common::on_screen(&h, dc_geometry::Point::new(-3.0, -3.0));
    let across = common::on_screen(&h, dc_geometry::Point::new(now.x_mm + 5.0, now.y_mm + 5.0));
    common::drag(&mut h, outside, across, egui::PointerButton::Primary);
    assert_eq!(selected_pictures(&h), ["b1"], "the marquee touched it");

    // Locked, from its Properties: a drag is refused, and says why.
    h.get_by_role_and_label(Role::Switch, "Locked").click();
    h.run();
    assert!(bitmap_of(&h, "b1").locked);
    let locked = bitmap_of(&h, "b1");
    let steps = undo_steps(&h);
    let centre = common::on_screen(
        &h,
        dc_geometry::Point::new(
            locked.x_mm + locked.width_mm / 2.0,
            locked.y_mm + locked.height_mm / 2.0,
        ),
    );
    common::drag(
        &mut h,
        centre,
        centre + egui::vec2(30.0, 30.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(bitmap_of(&h, "b1"), locked, "a locked picture stays");
    assert_eq!(undo_steps(&h), steps);
    assert_eq!(h.state().message(), Some("picture “gradient” is locked"));
    assert!(
        h.get_by_role_and_label(Role::TextInput, "X")
            .accesskit_node()
            .is_disabled(),
        "and its fields are read-only"
    );
    let unlock = wide_button(&h, "Unlock").expect("the locked note's Unlock");
    common::click_at(&mut h, unlock, egui::Modifiers::NONE);
    assert!(!bitmap_of(&h, "b1").locked);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 65, 67 and 68 (slice D6): with a picture in the project,
/// the Operations step's *Add* offers *Photo V-Carve*; added, it carves
/// the picture and opens in the editor, whose Setup shows the Picture
/// dropdown (the project's pictures, the one carved chosen), the Tool and
/// the max depth, required until typed. With a second picture, the
/// dropdown changes which one it carves, one undo step. Without a
/// picture the menu lists the kind with its reason.
#[test]
fn photo_v_carve_is_added_and_its_editor_shows_its_rows() {
    let dir = scratch("photo-v-carve");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    // No picture yet: the kind is listed with why it cannot be added.
    go_to(&mut h, "Operations");
    button_named(&h, "Add").click();
    h.run();
    let item = h.get_by_role_and_label(Role::MenuItem, "Photo V-Carve");
    assert!(
        item.accesskit_node().is_disabled(),
        "a Photo V-Carve needs a picture"
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // Two pictures, added as the sheet adds them.
    for (name, width_mm) in [("gradient.png", 60.0), ("ramp.png", 30.0)] {
        let mut model = dc_app::import::PictureImport::open(
            dc_import::ImageFormat::Png,
            name,
            &std::fs::read(common::fixture(&format!("photo/{name}"))).unwrap(),
            dc_app::import::Purpose::Import,
        );
        model.set_width_mm(width_mm);
        h.state_mut().app_mut().import_picture(&model).unwrap();
    }
    h.state_mut()
        .app_mut()
        .select_pictures(vec![dc_project::BitmapId::new("b1")]);
    h.run();
    let operations = h.state().app().project().operations.len();

    // Add ▸ Photo V-Carve carves the selected picture.
    button_named(&h, "Add").click();
    h.run();
    let item = h.get_by_role_and_label(Role::MenuItem, "Photo V-Carve");
    assert!(!item.accesskit_node().is_disabled());
    item.click();
    h.run();
    let app = h.state().app();
    assert_eq!(app.project().operations.len(), operations + 1);
    let Some(dc_project::Operation::PhotoVCarve(photo)) = app.project().operations.last() else {
        panic!("a Photo V-Carve was added");
    };
    assert_eq!(photo.image.as_str(), "b1");
    assert_eq!(app.undo_label(), Some("Add “Photo V-Carve 1”"));
    assert_eq!(app.editing(), Some(&photo.id), "open in the editor");
    assert_eq!(h.state().step(), dc_app::steps::Step::Operations);
    let id = photo.id.as_str().to_owned();

    // The editor's Setup: the Picture dropdown, the Tool, the max depth.
    let picture = h.get_by_role_and_label(Role::ComboBox, "Picture");
    assert_eq!(
        picture.accesskit_node().value().as_deref(),
        Some("gradient"),
        "the picture carved is the one chosen"
    );
    assert!(h.query_by_role_and_label(Role::Label, "Tool").is_some());
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Max depth")
            .is_some()
    );
    assert!(
        h.query_by_label("Targets").is_none(),
        "a Photo V-Carve has a picture, not targets"
    );

    // The dropdown lists the project's pictures; choosing one is a step.
    h.get_by_role_and_label(Role::ComboBox, "Picture").click();
    h.run();
    h.get_by_label("ramp").click();
    h.run();
    let dc_project::Operation::PhotoVCarve(photo) = operation_of(&h, &id) else {
        panic!("still a Photo V-Carve");
    };
    assert_eq!(photo.image.as_str(), "b2");
    assert_eq!(
        h.state().app().undo_label(),
        Some("Set image of “Photo V-Carve 1”")
    );

    // On the Design step the picture's row has its own menu, with *Add
    // operation* for a Photo V-Carve on it.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "ramp")
        .click_secondary();
    h.run();
    assert!(h.query_by_label("Rename").is_some());
    assert!(h.query_by_label("Add operation").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 76 on the window: once the version 14 plaque (whose Drill
/// predates `allow_tool_wider_than_hole`, so its holes are too small for
/// its bit) has generated, the status bar's counts open the Warnings
/// drawer; a row about a hole selects the hole; *Copy* hands egui
/// `check`'s text (`dc_app::warnings::copy_text`), which reaches the OS
/// clipboard (`warnings_copy_reaches_the_clipboard`, #581); the close
/// button closes it.
#[test]
fn warnings_drawer_selects_and_copies() {
    let dir = scratch("warnings-drawer");
    let plaque = dir.join("plaque.dcv");
    std::fs::copy(
        common::repo_root()
            .join("crates/dreamcarve/tests/fixtures/holds-slug-migration/plaque.dcv"),
        &plaque,
    )
    .expect("fixture copied");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    assert!(h.state().app().generation().finished().is_some());

    h.get_by_label("Warnings").click();
    h.run();
    assert!(h.state().warnings_open());
    let row = dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|g| g.rows)
        .find(|r| r.code.as_deref() == Some("drill_hole_too_small"))
        .expect("the plaque's holes are too small for its drill");
    h.get_by_label(&row.name).click();
    h.run();
    assert_eq!(Some(h.state().app().selection().clone()), row.selection());

    h.get_by_label("Copy").click();
    h.step();
    let copied: Vec<String> = h
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        copied,
        [dc_app::warnings::copy_text(h.state().app())],
        "Copy hands egui check's text"
    );
    h.run();

    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    assert!(!h.state().warnings_open());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 76 and acceptance 82 (slice D9): a hint is the drawer's
/// third tier, told by its code, and `check`, which has no such tier,
/// prints it as a `warning:` line; *Copy* is `check`'s text. Both hold on
/// the plaque, whose `Set me` tool no operation uses. The drawer's row
/// for it says *hint* wherever it names its tier: its chip, the drawer's
/// title and its accessible name, which used to be `check`'s line and so
/// read `warning: unused_tool: …` under a chip saying *hint*. *Copy*
/// still writes that line and counts it with the warnings, as `check`
/// does, and its tooltip says so.
#[test]
fn a_hint_reads_as_a_hint_in_the_drawer_and_as_a_warning_in_copy() {
    let dir = scratch("warnings-hint");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_generation(&mut h, "the plaque");
    h.get_by_label("Warnings").click();
    h.run();

    assert!(
        h.query_by_label("Warnings · 1 hint").is_some(),
        "the title counts it as a hint"
    );
    let rows: Vec<String> = h
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| label.contains("unused_tool"))
        .collect();
    assert_eq!(rows.len(), 1, "one row about the unused tool: {rows:?}");
    assert!(
        rows[0].starts_with("hint: unused_tool: tool `t1` (`Set me`)"),
        "the row names its tier as the drawer does: {}",
        rows[0]
    );
    assert!(
        h.query_by_label_contains("warning: unused_tool").is_none(),
        "nothing in the drawer calls it a warning"
    );

    let copied = dc_app::warnings::copy_text(h.state().app());
    let line = copied
        .lines()
        .find(|line| line.contains("unused_tool"))
        .expect("Copy lists it");
    assert!(
        line.contains(": warning: unused_tool: tool `t1` (`Set me`)"),
        "as check prints it: {line}"
    );
    assert!(
        copied.ends_with("plaque.dcv: 0 error(s), 1 warning(s)\n"),
        "and counts it, as check does: {copied}"
    );
    h.get_by_label("Copy").hover();
    wait_for_tooltip(&mut h);
    assert!(
        h.query_by_label_contains("a hint as a warning").is_some(),
        "Copy's tooltip says what its text is"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice D9 (capability 76: "the message with the operation and shape
/// named"), in the window. The plaque's outline moved half off the stock
/// makes its Cut out leave the stock, which the engine reports as
/// `operation `op4` cuts off the stock`. The drawer's row and the Export
/// step's list say `operation `Cut out` (`op4`)`; *Copy* keeps the
/// engine's line, which is `check`'s. A new Inlay with no V-bit to take
/// says under its Tool row that it uses tool *(none chosen)*, where it
/// said `tool ```.
#[test]
fn findings_are_shown_with_names_not_ids() {
    let dir = scratch("warnings-names");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_generation(&mut h, "the plaque");
    h.state_mut()
        .app_mut()
        .run(Command::TransformInstances {
            ids: vec![dc_project::InstanceId::new("i1")],
            delta: dc_geometry::Transform::translate(60.0, 0.0),
        })
        .unwrap();
    h.run();
    settle_generation(&mut h, "the plaque with its outline off the stock");
    go_to(&mut h, "Design");
    h.get_by_label_contains("Warnings:").click();
    h.run();
    let named = "operation `Cut out` (`op4`) cuts off the stock";
    assert!(
        h.query_all_by_label_contains(named).count() >= 1,
        "the drawer names the operation"
    );
    assert_eq!(
        h.query_all_by_label_contains("operation `op4` cuts off")
            .count(),
        0,
        "and nowhere gives its id alone"
    );
    let copied = dc_app::warnings::copy_text(h.state().app());
    assert!(
        copied.contains("error: operation `op4` cuts off the stock") && !copied.contains(named),
        "Copy is check's text: {copied}"
    );
    go_to(&mut h, "Export");
    assert!(
        in_export_step_containing(&h, named) >= 1,
        "the Export step lists it by name too"
    );

    h.state_mut().app_mut().undo();
    let project = h.state().app().project().clone();
    let inlay = dc_app::operations::new_operation(
        &project,
        h.state().app().strings(),
        dc_app::operations::Kind::Inlay,
        &[dc_project::InstanceId::new("i4")],
    )
    .expect("an Inlay of the square");
    h.state_mut()
        .app_mut()
        .run(Command::AddOperation(inlay))
        .unwrap();
    h.run();
    open_operation(&mut h, "Inlay 1");
    let inspector = h.state().layout().inspector.expect("the editor shows");
    let under_the_field = h
        .query_all_by_label_contains("uses tool (none chosen), which does not exist")
        .filter(|node| inspector.contains_rect(node.rect()))
        .count();
    assert!(under_the_field >= 1, "the line under the Tool row says so");
    assert_eq!(h.query_all_by_label_contains("uses tool ``").count(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Export step's panel: the card the plan, the blocking findings and
/// the result are drawn in.
fn export_panel(h: &Harness<'_, dreamcarve_app::Shell>) -> egui::Rect {
    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    h.state().layout().step_panel.expect("the step panel shows")
}

/// The nodes labelled exactly `label` in the Export step's panel.
fn in_export_step(h: &Harness<'_, dreamcarve_app::Shell>, label: &str) -> usize {
    let panel = export_panel(h);
    h.query_all_by_label(label)
        .filter(|node| panel.contains_rect(node.rect()))
        .count()
}

/// The nodes whose label contains `text` in the Export step's panel.
fn in_export_step_containing(h: &Harness<'_, dreamcarve_app::Shell>, text: &str) -> usize {
    let panel = export_panel(h);
    h.query_all_by_label_contains(text)
        .filter(|node| panel.contains_rect(node.rect()))
        .count()
}

/// Clicks the Warnings drawer's row named `line` (`Row::name`). On the Export
/// step a finding that blocks export is listed in the step's panel too,
/// under the same name: the drawer's is the one outside the panel.
fn click_drawer_row(h: &mut Harness<'_, dreamcarve_app::Shell>, line: &str) {
    let panel = h.state().layout().step_panel;
    h.query_all_by_label(line)
        .find(|node| panel.is_none_or(|panel| !panel.contains_rect(node.rect())))
        .expect("the drawer's row")
        .click();
    h.run();
}

/// Acceptance 84's second half, in the window: "a project with only
/// warnings exports, and the status bar counts them". The version 14
/// plaque's holes are too small for its drill, which `check` warns of
/// and does not refuse. Its status bar's counts cell says how many
/// warnings there are, the number of warning rows the drawer lists and no
/// error count; *Export G-code* is enabled on the Export step, and
/// pressed it writes every program the plan lists.
#[test]
fn a_project_with_only_warnings_exports_and_the_status_bar_counts_them() {
    let dir = scratch("warnings-export");
    let plaque = dir.join("plaque.dcv");
    std::fs::copy(
        common::repo_root()
            .join("crates/dreamcarve/tests/fixtures/holds-slug-migration/plaque.dcv"),
        &plaque,
    )
    .expect("fixture copied");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);

    let counts = dc_app::warnings::counts(h.state().app());
    assert_eq!(counts.errors, 0, "only warnings");
    assert!(counts.warnings > 0, "the holes are too small for the drill");
    let listed = dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|group| group.rows)
        .filter(|row| row.note.is_none() && row.level == dc_app::warnings::Level::Warning)
        .count();
    assert_eq!(counts.warnings, listed, "what the drawer lists");

    // The status bar's cell: the count, in words, in its bottom strip.
    let status_top = 800.0 - STATUS_BAR_PX;
    let said = if counts.warnings == 1 {
        "1 warning".to_owned()
    } else {
        format!("{} warnings", counts.warnings)
    };
    assert!(
        h.query_all_by_label(&said)
            .any(|node| node.rect().top() >= status_top),
        "the status bar says `{said}`"
    );
    assert!(
        h.query_all_by_label_contains(" error")
            .all(|node| node.rect().top() < status_top),
        "and counts no error"
    );
    assert!(
        h.query_all_by_label("No problems")
            .all(|node| node.rect().top() < status_top)
    );

    // And it exports: the button is enabled, and writes the plan's files.
    go_to(&mut h, "Export");
    settle_export(&mut h);
    assert!(
        !h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled(),
        "warnings do not block export"
    );
    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    assert_eq!(ready.blocked(), None);
    press_export(&mut h);
    let files = ready.files("plaque");
    assert!(!files.is_empty());
    for (name, text) in files {
        let written = std::fs::read_to_string(dir.join(&name)).expect("program written");
        assert!(written == text, "{name} is the plan's program");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Presses the header's *Export G-code*, the export command.
fn press_export(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
}

/// Waits until generation is over and the Export step's plan is ready,
/// stepping the window.
fn settle_export(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while h.state().app().generation().busy()
        || h.state().worker_busy()
        || h.state().export_planning()
    {
        assert!(std::time::Instant::now() < deadline, "the export settles");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
}

/// Capability 77 and acceptance 84 in the window (#547; redesign slice
/// D4): ⌘E opens the Export step, which lists the programs
/// `dc_app::export::plan` builds; the header's *Export G-code* writes
/// them beside the project. A program whose destination cannot be
/// replaced (a folder is in the way) stops the export with nothing
/// written: every file or none. Cleared, *Export G-code* writes them
/// all, and the result takes the plan's place, listing them with *Show in
/// folder*; the project file is never written. A tool without a feed
/// disables *Export G-code*, and the step lists the CLI's error.
#[test]
fn export_sheet_disabled_on_errors_and_writes_all_or_none() {
    use egui_kittest::kittest::NodeT as _;

    let dir = scratch("export-sheet");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let project_bytes = std::fs::read(&plaque).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque.clone()), &dialogs),
        1280.0,
        900.0,
    );
    settle_export(&mut h);
    go_to(&mut h, "Design");
    command_key(&h, egui::Key::E);
    h.run();
    settle_export(&mut h);
    assert_eq!(current_step(&h), "Export", "⌘E opens the Export step");
    assert!(h.query_by_role(Role::Dialog).is_none(), "and no sheet");

    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    let names = ready.names("plaque");
    assert!(names.len() > 1, "the plaque writes a program per tool");
    open_program_details(&mut h);
    for name in &names {
        assert!(in_export_step(&h, name) > 0, "the step lists {name}");
    }

    // A folder where the last program goes: nothing is written.
    let blocker = dir.join(names.last().unwrap());
    std::fs::create_dir_all(blocker.join("keep")).unwrap();
    let naming = |h: &Harness<'_, dreamcarve_app::Shell>| {
        in_export_step_containing(h, names.last().unwrap())
    };
    let named = naming(&h);
    press_export(&mut h);
    for name in &names[..names.len() - 1] {
        assert!(!dir.join(name).exists(), "{name} was not written");
    }
    assert!(blocker.join("keep").is_dir(), "the folder is untouched");
    assert!(
        in_export_step_containing(&h, "Nothing was written") > 0,
        "the step says nothing was written"
    );
    assert_eq!(naming(&h), named + 1, "the failure names the file");

    std::fs::remove_dir_all(&blocker).unwrap();
    press_export(&mut h);
    for ((name, text), program) in ready.files("plaque").iter().zip(&names) {
        assert_eq!(name, program);
        let written = std::fs::read_to_string(dir.join(name)).expect("program written");
        assert!(written == *text, "{name} is the plan's program");
    }
    assert!(in_export_step(&h, "Show in folder") > 0);
    assert!(
        in_export_step_containing(&h, &names[0]) > 0,
        "the result names the files"
    );
    assert_eq!(
        in_export_step(&h, "Program Details"),
        0,
        "the result takes the plan's place"
    );
    assert_eq!(
        std::fs::read(&plaque).unwrap(),
        project_bytes,
        "export never writes the project"
    );
    assert!(!h.state().app().is_dirty());
    assert_eq!(
        h.state().app().prefs().export_dirs.get(&plaque),
        Some(&dir),
        "the folder is remembered for this project"
    );
    h.get_by_label("Done").click();
    h.run();
    assert_eq!(in_export_step(&h, "Show in folder"), 0, "Done lets it go");
    assert_eq!(in_export_step(&h, "Program Details"), 1, "the plan is back");

    // A tool without a feed: the CLI's error, and Export disabled.
    // The first operation's tool: the plaque has one tool it never uses.
    let project = h.state().app().project();
    let used = project.operations[0].run_tools()[0].clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    h.state_mut()
        .app_mut()
        .run(Command::UpdateTool(tool))
        .unwrap();
    settle_export(&mut h);
    let built = dc_job::build(
        h.state().app().project(),
        dc_job::ProgramSplit::ProjectDefault,
        &date,
    );
    assert_eq!(built.findings.errors.len(), 1, "one tool lacks a feed");
    let error = built.findings.errors[0].message.clone();
    // One line says so and Export G-code is disabled; the step lists the
    // finding under the line, as the drawer does (redesign slice D4).
    assert_eq!(
        in_export_step(&h, "1 error blocks export"),
        1,
        "the blocking line"
    );
    assert_eq!(
        in_export_step_containing(&h, &error),
        1,
        "the step lists the finding: {error}"
    );
    assert!(
        h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled(),
        "Export G-code is disabled"
    );
    // Show opens the drawer at the error.
    assert!(!h.state().warnings_open());
    h.get_by_role_and_label(Role::Button, "Show").click();
    h.run();
    assert!(h.state().warnings_open(), "the drawer is open");
    assert!(
        h.query_all_by_label_contains(&error).count() > 1,
        "the drawer lists the error too: {error}"
    );
    // The finding in the step opens the step that fixes it: the tool's,
    // with the tool in the Inspector.
    let panel = export_panel(&h);
    h.query_all_by_label_contains(&error)
        .find(|node| panel.contains_rect(node.rect()))
        .expect("the finding's row")
        .click();
    h.run();
    assert_eq!(current_step(&h), "Tools", "the row opens the Tools step");
    assert!(
        h.state().layout().inspector.is_some(),
        "with the tool in the Inspector"
    );
    // Which tool: the one that lacks the feed, by its name in the
    // editor's Name field (acceptance 84, "a click on it opens the tool").
    let lacking = h
        .state()
        .app()
        .project()
        .tools
        .iter()
        .find(|t| t.id == used)
        .expect("the tool is still there")
        .name
        .clone();
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Name")
            .accesskit_node()
            .value()
            .as_deref(),
        Some(lacking.as_str()),
        "the Inspector holds the tool without a feed"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Redesign slice D4: a project never saved has no folder to export to,
/// so *Export G-code* asks for one first with the folder dialog. With the
/// dialog cancelled nothing is written and the step still shows the plan;
/// with a folder picked the programs go there, under the project's name,
/// and the step shows the folder. The step's *Choose…* asks the same
/// dialog.
#[test]
fn export_asks_for_a_folder_when_none_is_chosen() {
    let dir = scratch("export-folder");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 900.0);
    // The plaque example opens untitled: no file, so no folder.
    h.get_by_role_and_label(Role::Button, "Plaque").click();
    h.run();
    settle_export(&mut h);
    assert_eq!(h.state().app().source(), None);
    assert_eq!(current_step(&h), "Export", "a finished project opens here");
    settle_export(&mut h);
    assert_eq!(in_export_step(&h, "Not chosen yet"), 1, "no folder yet");

    // Cancelled: asked, and nothing written.
    press_export(&mut h);
    assert_eq!(dialogs.asked(), ["export_folder"]);
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "nothing written"
    );
    assert_eq!(in_export_step(&h, "Program Details"), 1, "still the plan");
    assert_eq!(in_export_step_containing(&h, "written"), 0);

    // A folder picked: the programs are written there.
    dialogs.answer(&out);
    press_export(&mut h);
    assert_eq!(dialogs.asked(), ["export_folder", "export_folder"]);
    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    let stem = h.state().app().project().name.clone();
    let files = ready.files(&stem);
    assert!(files.len() > 1, "a program per tool");
    for (name, text) in &files {
        let written = std::fs::read_to_string(out.join(name)).expect("program written");
        assert!(written == *text, "{name} is the plan's program");
    }
    assert_eq!(in_export_step(&h, "2 programs written"), 1, "the result");
    assert_eq!(in_export_step(&h, &out.display().to_string()), 1);

    // Choose… asks the same dialog, and the plan shows the folder picked.
    h.get_by_label("Done").click();
    h.run();
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    dialogs.answer(&other);
    h.get_by_role_and_label(Role::Button, "Choose…").click();
    h.run();
    assert_eq!(dialogs.asked().len(), 3);
    assert_eq!(in_export_step(&h, &other.display().to_string()), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Clicks the Export step's *Program Details* disclosure.
fn open_program_details(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    h.get_by_label("Program Details").click();
    h.run();
}

/// Opens the Export step on the plaque, generated and planned.
fn plaque_export_sheet(
    name: &str,
) -> (std::path::PathBuf, Harness<'static, dreamcarve_app::Shell>) {
    let dir = scratch(name);
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        900.0,
    );
    settle_export(&mut h);
    command_key(&h, egui::Key::E);
    h.run();
    settle_export(&mut h);
    assert_eq!(current_step(&h), "Export", "the step is open");
    (dir, h)
}

/// Capability 77 and acceptance 102 (#624): the trimmed export has no
/// toolpath preview tick, no warnings list, no estimated total and no
/// note about how files are written; whatever warnings the plan found
/// are the status bar's and the drawer's, not listed here.
#[test]
fn export_sheet_has_no_preview_warnings_total_or_note() {
    use egui_kittest::kittest::NodeT as _;

    let (dir, mut h) = plaque_export_sheet("export-trimmed");
    open_program_details(&mut h);
    for gone in [
        "toolpath preview",
        "preview.svg",
        "Warnings",
        "Estimated total",
        "Every file is written",
    ] {
        assert_eq!(
            in_export_step_containing(&h, gone),
            0,
            "no node labelled with `{gone}`"
        );
    }
    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    for finding in &ready.findings.warnings {
        assert_eq!(
            in_export_step_containing(&h, &finding.message),
            0,
            "the warning is not listed: {}",
            finding.message
        );
    }
    assert!(
        in_export_step_containing(&h, "blocks export") == 0
            && in_export_step_containing(&h, "block export") == 0,
        "no error, no blocking line"
    );
    assert!(
        !h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled(),
        "warnings do not block"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 77 and acceptance 102 (#624): *Program Details* is closed
/// until the user opens it, its rows hidden; opened, its rows are the
/// plan's, in run order: file name, estimate and the step before each.
#[test]
fn program_details_start_closed() {
    use eframe::egui::accesskit::Toggled;
    use egui_kittest::kittest::NodeT as _;

    let (dir, mut h) = plaque_export_sheet("export-details");
    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    let names = ready.names("plaque");
    assert!(names.len() > 1);

    let header = h.get_by_label("Program Details");
    assert_eq!(
        header.accesskit_node().toggled(),
        Some(Toggled::False),
        "closed"
    );
    for name in &names {
        assert_eq!(
            in_export_step(&h, name),
            0,
            "{name} is hidden while the disclosure is closed"
        );
    }
    // After its title, as every closed section of the step says what it
    // holds: the summary is the header's value.
    let summary = format!("{} programs, run in this order", names.len());
    assert_eq!(
        h.get_by_label("Program Details").value().as_deref(),
        Some(summary.as_str()),
        "the closed disclosure says how many"
    );

    open_program_details(&mut h);
    assert_eq!(
        h.get_by_label("Program Details").accesskit_node().toggled(),
        Some(Toggled::True),
        "open"
    );
    // The rows, top to bottom, are the plan's programs in run order.
    let mut tops: Vec<(f64, &String)> = names
        .iter()
        .map(|name| {
            let node = h.get_by_label(name);
            let top = node.accesskit_node().bounding_box().expect("laid out").y0;
            (top, name)
        })
        .collect();
    tops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let order: Vec<&String> = tops.into_iter().map(|(_, n)| n).collect();
    assert_eq!(order, names.iter().collect::<Vec<_>>(), "in run order");
    for (k, summary) in ready.programs.iter().enumerate() {
        let estimate = ready.estimate(k).expect("the plaque's programs estimate");
        assert!(
            in_export_step(&h, &estimate) > 0,
            "{}: its estimate {estimate}",
            names[k]
        );
        assert!(
            in_export_step_containing(&h, &summary.before) > 0,
            "{}: the step before it",
            names[k]
        );
    }
    // Opened, it stays open: the step is no sheet that opens anew.
    go_to(&mut h, "Design");
    go_to(&mut h, "Export");
    assert_eq!(
        h.get_by_label("Program Details").accesskit_node().toggled(),
        Some(Toggled::True),
        "still open on coming back"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 77a and 77b, acceptance 102 (#625): after *Export
/// G-code* the result is in the plan's place: `2 programs written`, the
/// files in run order and the folder with *Show in folder*, and *Run
/// instructions* closed, whose steps and notes show once it is opened.
/// *Save setup sheet…* asks where, suggesting `plaque-setup.html` beside
/// the programs, and writes the page the plan renders; nothing else is
/// written and the project is unchanged. *Done* brings the plan back, and
/// the setup sheet can still be saved from the File menu.
#[test]
fn export_result_shows_three_parts_and_saves_the_setup_sheet() {
    use eframe::egui::accesskit::Toggled;
    use egui_kittest::kittest::NodeT as _;

    let dir = scratch("export-result");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let project_bytes = std::fs::read(&plaque).unwrap();
    let dialogs = QueuedDialogs::default();
    // Tall, so the result with its instructions open shows whole.
    let mut h = harness(
        options(&dir.join("config"), Some(plaque.clone()), &dialogs),
        1280.0,
        1500.0,
    );
    settle_export(&mut h);
    command_key(&h, egui::Key::E);
    h.run();
    settle_export(&mut h);
    let date = dreamcarve_shell::clock::today_utc();
    let options = dc_app::export::Options::new(&date, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(h.state().app(), &options) else {
        panic!("the plaque has generated");
    };
    let names = ready.names("plaque");
    press_export(&mut h);

    let strings = dc_app::Strings::english();
    let result = ready.result(&names, &dir, &strings);
    assert_eq!(in_export_step(&h, "2 programs written"), 1, "Status");
    for name in &names {
        assert_eq!(in_export_step(&h, name), 1, "Files lists {name}");
    }
    assert_eq!(
        in_export_step(&h, &dir.display().to_string()),
        1,
        "the folder"
    );
    assert_eq!(in_export_step(&h, "Show in folder"), 1);
    assert_eq!(in_export_step(&h, "Copy path"), 1);
    let header = h.get_by_label("Run instructions");
    assert_eq!(header.accesskit_node().toggled(), Some(Toggled::False));
    let first_step = &result.instructions.steps[0].before;
    assert_eq!(
        in_export_step(&h, first_step),
        0,
        "the steps are hidden while closed"
    );
    h.get_by_label("Run instructions").click();
    h.run();
    // Each step once, under the window's own number: the report's line
    // without the number the report gave it ("1. 1. plaque-1-T2.nc" was
    // the number said twice).
    for (k, step) in result.instructions.steps.iter().enumerate() {
        let number = format!("{}.", k + 1);
        let line = step
            .before
            .strip_prefix(&number)
            .expect("the report numbers the plaque's two programs")
            .trim_start();
        assert_eq!(in_export_step(&h, line), 1, "{line}");
        assert_eq!(in_export_step(&h, &number), 1, "numbered {number}");
        assert_eq!(in_export_step(&h, &step.before), 0, "{}", step.before);
    }
    for note in &result.instructions.notes {
        assert_eq!(in_export_step(&h, note), 1, "the note {note}");
    }

    // Save setup sheet…: the dialog suggests the page beside the programs.
    let page = dir.join("plaque-setup.html");
    dialogs.answer(&page);
    h.get_by_label("Save setup sheet…").scroll_to_me();
    h.run();
    h.get_by_label("Save setup sheet…").click();
    h.run();
    assert_eq!(
        dialogs.asked().last().cloned(),
        Some(format!("save_setup_sheet {}", page.display()))
    );
    let written = std::fs::read_to_string(&page).expect("the setup sheet is written");
    assert!(
        written == ready.setup_sheet(&names, env!("CARGO_PKG_VERSION"), &strings),
        "the page the plan renders"
    );
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "config")
        .collect();
    files.sort();
    let mut expected: Vec<String> = names.clone();
    expected.extend(["plaque-setup.html".to_owned(), "plaque.dcv".to_owned()]);
    expected.sort();
    assert_eq!(files, expected, "the programs, the page and the project");
    assert_eq!(std::fs::read(&plaque).unwrap(), project_bytes);
    assert!(!h.state().app().is_dirty(), "saving it changes nothing");

    h.get_by_label("Done").click();
    h.run();
    assert_eq!(
        in_export_step(&h, "2 programs written"),
        0,
        "Done lets the result go"
    );
    assert_eq!(in_export_step(&h, "Program Details"), 1, "the plan is back");
    h.get_by_label("File").click();
    h.run();
    assert!(
        h.query_by_label("Save Setup Sheet…").is_some(),
        "File ▸ Save Setup Sheet… after an export"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A project of 500 shapes, built here and never committed: 500 linked
/// copies of a 12 mm circle on a 25 × 20 grid at 20 mm pitch, on
/// 510 × 410 × 12 mm stock, with a 3 mm Pocket and a medial V-Carve on
/// all of them (acceptance 87's project).
fn five_hundred_shapes() -> dc_project::Project {
    use dc_geometry::{Point, Polyline, Transform};
    use dc_project::{
        ClearanceStrategy, CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition,
        FeatureInstance, InstanceId, MachiningOrder, Operation, OperationId, PocketOp, Tool,
        ToolId, ToolKind, VCarveOp, VCarveStrategy, WallPass,
    };

    let mut project = dc_app::App::new_project().expect("a new project");
    project.name = "five-hundred".into();
    project.stock.width_mm = 510.0;
    project.stock.height_mm = 410.0;
    project.stock.thickness_mm = Some(12.0);
    let definition = DefinitionId::new("d1");
    project.definitions.insert(
        definition.clone(),
        FeatureDefinition {
            id: definition.clone(),
            name: "circle".into(),
            profiles: vec![Polyline::circle(Point::new(0.0, 0.0), 6.0)],
        },
    );
    for k in 0..500_u32 {
        let (col, row) = (f64::from(k % 25), f64::from(k / 25));
        project.instances.push(FeatureInstance {
            id: InstanceId::new(format!("i{k}")),
            definition: definition.clone(),
            transform: Transform::translate(15.0 + 20.0 * col, 15.0 + 20.0 * row),
            name: format!("circle {k}"),
            visible: true,
            locked: false,
            z_top_mm: Some(12.0),
            z_bottom_mm: Some(0.0),
        });
    }
    let end_mill = Tool {
        id: ToolId::new("t1"),
        name: "6.35 mm upcut".into(),
        vendor: None,
        product_id: None,
        number: Some(1),
        cutter: ToolKind::EndMill,
        diameter_mm: 6.35,
        flutes: 2,
        feed_mm_min: Some(1000.0),
        plunge_mm_min: Some(300.0),
        rpm: Some(18000),
        step_down_mm: Some(2.0),
        step_over_fraction: Some(0.4),
    };
    let v_bit = Tool {
        id: ToolId::new("t2"),
        name: "90° V-bit".into(),
        number: Some(2),
        cutter: ToolKind::VBit {
            included_angle_deg: 90.0,
        },
        step_down_mm: None,
        ..end_mill.clone()
    };
    project.tools = vec![end_mill, v_bit];
    let targets: Vec<InstanceId> = project.instances.iter().map(|i| i.id.clone()).collect();
    project.operations = vec![
        Operation::Pocket(PocketOp {
            id: OperationId::new("o1"),
            name: "Pocket".into(),
            enabled: true,
            targets: targets.clone(),
            tool: ToolId::new("t1"),
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: Some(3.0),
            step_down_mm: None,
            step_over_fraction: None,
            direction: CutDirection::Climb,
            stock_to_leave_mm: 0.0,
            rest_of: None,
            entry: Entry::Plunge,
            corner_relief: CornerRelief::None,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }),
        Operation::VCarve(VCarveOp {
            id: OperationId::new("o2"),
            name: "V-Carve".into(),
            enabled: true,
            targets,
            tool: ToolId::new("t2"),
            strategy: VCarveStrategy::Medial,
            clearing: Vec::new(),
            flat_depth_mm: None,
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false,
            step_over_fraction: None,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }),
    ];
    project
}

/// Acceptance 87's budget: the window answers input within 100 ms.
const FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);
/// The fewest frames [`FrameTimes::assert_within_budget`] judges.
const FRAME_SAMPLE: usize = 60;
/// No frame may take longer than this, whatever the others do.
const FRAME_CEILING: std::time::Duration = std::time::Duration::from_millis(250);

/// The UI thread's frames, each one egui pass timed on its own: the
/// shell's update and egui's layout on this thread, while the worker runs.
#[derive(Default)]
struct FrameTimes(Vec<std::time::Duration>);

impl FrameTimes {
    /// Runs one frame and times it. Nothing may be queued by
    /// `Node::click` and the like, which `Harness::step` runs as one pass
    /// per event: [`FrameTimes::click`] times each of those.
    fn step(&mut self, h: &mut Harness<'_, dreamcarve_app::Shell>) {
        let start = std::time::Instant::now();
        h.step();
        self.0.push(start.elapsed());
    }

    /// Clicks the widget labelled `label` as `Node::click` does (the
    /// pointer moves over it, presses and releases) and times each of the
    /// three frames on its own.
    fn click(&mut self, h: &mut Harness<'_, dreamcarve_app::Shell>, label: &str) {
        let pos = h.get_by_label(label).rect().center();
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        for event in [egui::Event::PointerMoved(pos), button(true), button(false)] {
            h.input_mut().events.push(event);
            self.step(h);
        }
    }

    /// At least [`FRAME_SAMPLE`] frames, the 95th percentile within
    /// [`FRAME_BUDGET`] and none over [`FRAME_CEILING`]
    /// (`window_answers_input_while_generating` says why).
    fn assert_within_budget(&self) {
        let mut sorted = self.0.clone();
        sorted.sort();
        let n = sorted.len();
        assert!(n >= FRAME_SAMPLE, "only {n} frames were timed");
        let p95 = sorted[(n * 95).div_ceil(100) - 1];
        let slowest = sorted[n - 1];
        // Printed before judging, so a failure on CI shows the spread.
        println!(
            "window_answers_input_while_generating: {n} frames, median {:?}, p95 {p95:?}, slowest {slowest:?}",
            sorted[n / 2]
        );
        assert!(
            p95 <= FRAME_BUDGET,
            "the 95th-percentile frame took {p95:?} over {n} frames (slowest {slowest:?})"
        );
        assert!(
            slowest <= FRAME_CEILING,
            "a frame took {slowest:?}, over the {FRAME_CEILING:?} ceiling"
        );
    }
}

/// Acceptance 87 (the window never blocks; capability 71): on 500 shapes
/// with a Pocket and a V-Carve on all of them, generation runs on the
/// worker while the window answers input within 100 ms a frame, both
/// operations' rows show the spinner while they wait, the status bar
/// reads *Generating k of n…*, and *Stop* shows generation stopped on
/// the next frame, well within the second the spec allows: the worker
/// finishes the job it is on (design open question 4), and its result is
/// dropped when it lands. *Regenerate all*, which the status bar offers
/// after Stop (#616), then runs generation to its end, so the frames that
/// accept each result are timed too, and it runs again until at least 60
/// frames are timed.
///
/// Every frame from opening to the end is timed on the UI thread, one
/// egui pass each (a click is three: hover, press, release), and the
/// frames are judged together: at least 60 of them, the 95th percentile
/// within 100 ms, and none over 250 ms. A single frame's maximum is the
/// wrong statistic on CI (#573). Every kittest test runs its own window
/// and generation worker in parallel on a few shared cores, so a frame
/// with 10 to 20 ms of work can wait tens of milliseconds to be
/// scheduled. The percentile fails any cost paid on many frames, and the
/// ceiling fails any one frame that works for long.
#[test]
fn window_answers_input_while_generating() {
    let dir = scratch("five-hundred");
    let path = dir.join("five-hundred.dcv");
    let text = dc_project::save(&five_hundred_shapes()).expect("the project saves");
    std::fs::write(&path, text).expect("the project is written");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        800.0,
    );
    assert_eq!(h.state().app().project().instances.len(), 500);
    let mut frames = FrameTimes::default();
    // The Operations step, for its rows: by its key, one frame.
    command_key(&h, egui::Key::Num4);
    frames.step(&mut h);

    // The change settles and the worker takes the first job. The frame
    // that posts it does so at its end, after drawing, and a result is
    // accepted only at the start of a later frame, so in this frame both
    // operations still wait, whatever the worker's speed (#593): o1 in
    // flight, o2 pending.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !h.state().worker_busy() {
        assert!(
            std::time::Instant::now() < deadline,
            "the worker never started"
        );
        frames.step(&mut h);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let generation = h.state().app().generation();
    assert!(generation.busy(), "generation is still running");
    for op in ["o1", "o2"] {
        assert!(
            generation.is_regenerating(&dc_project::OperationId::new(op)),
            "{op} is waiting for its result"
        );
    }
    assert_eq!(
        h.query_all_by_label("Regenerating").count(),
        2,
        "both rows show the spinner"
    );
    assert!(
        h.state().app().scene().operations().iter().all(|o| o.stale),
        "the Scene marks both stale, for the Viewport to fade (slice 16 draws runs)"
    );
    assert!(
        h.query_by_label_contains("Generating").is_some(),
        "the status bar says it is generating"
    );

    // Input is answered at once: the status bar's counts open the
    // Warnings drawer, read in the click's last frame and drawn in the
    // next.
    frames.click(&mut h, "Warnings");
    assert!(h.state().warnings_open(), "the click was read");
    frames.step(&mut h);
    assert!(
        h.query_by_label("Copy").is_some(),
        "the drawer is drawn in the next frame"
    );

    // Stop: stopped on the UI at once, nothing pending, the running job's
    // result dropped when it lands.
    assert!(h.state().app().generation().busy());
    let stopped_at = std::time::Instant::now();
    frames.click(&mut h, "Stop");
    frames.step(&mut h);
    assert!(
        stopped_at.elapsed() < std::time::Duration::from_secs(1),
        "Stop took {:?}",
        stopped_at.elapsed()
    );
    let app = h.state().app();
    assert!(!app.generation().busy(), "generation stopped");
    assert!(app.pending().is_empty());
    assert!(app.generation().finished().is_none());
    assert_eq!(h.query_all_by_label("Regenerating").count(), 0);
    assert!(
        h.query_by_label("Stopped").is_some(),
        "the status bar says so"
    );

    // Regenerate all, from the status bar after Stop (#616): generation
    // runs to its end, and the frames that accept each result are timed
    // with the rest; again (through the app, since the status bar offers
    // it only after a Stop) until there are enough frames to judge, since
    // a fast machine generates in fewer of them.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let mut first = true;
    while frames.0.len() < FRAME_SAMPLE {
        if first {
            frames.click(&mut h, "Regenerate all");
            first = false;
        } else {
            h.state_mut().app_mut().regenerate_all();
            frames.step(&mut h);
        }
        assert!(h.state().app().generation().busy(), "generation restarted");
        while h.state().app().generation().finished().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "generation never finished"
            );
            frames.step(&mut h);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!h.state().app().generation().busy(), "generation finished");
    }
    frames.assert_within_budget();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The operation `id` of the shell's project.
fn operation_of(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> dc_project::Operation {
    h.state()
        .app()
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("no operation {id}"))
}

/// Acceptance 86's last clause (capability 66; `experience.md`, section 7,
/// "Text field with trailing unit"): typing three characters in an editor
/// field changes nothing while the field has focus, and leaving it (Tab)
/// is one undo step, labelled with the field. Clicking an operation's row
/// opens it in the editor.
#[test]
fn field_commits_on_blur_not_per_keystroke() {
    let dir = scratch("field-blur");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    open_operation(&mut h, "Cut out");
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op4"),
        "the row opens the Profile in the editor"
    );
    let before = operation_of(&h, "op4");
    let steps = undo_steps(&h);

    h.get_by_role_and_label(Role::TextInput, "Depth").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    for typed in ["9", ".", "5"] {
        h.event(egui::Event::Text(typed.to_owned()));
        h.run();
        assert_eq!(
            operation_of(&h, "op4"),
            before,
            "typing `{typed}` changes nothing yet"
        );
        assert_eq!(undo_steps(&h), steps, "and adds no step");
    }
    h.key_press(egui::Key::Tab);
    h.run();
    let dc_project::Operation::Profile(after) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    assert_eq!(after.depth_mm, Some(9.5), "leaving the field writes it");
    assert_eq!(undo_steps(&h), steps + 1, "as one step");
    assert_eq!(h.state().app().undo_label(), Some("Set depth of “Cut out”"));

    // Escape restores the field and writes nothing (section 7), though the
    // Viewport takes the key before the editor draws (#618).
    h.get_by_role_and_label(Role::TextInput, "Depth").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("7".to_owned()));
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    let dc_project::Operation::Profile(after) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    assert_eq!(after.depth_mm, Some(9.5), "Escape writes nothing");
    assert_eq!(undo_steps(&h), steps + 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The targets of operation `id`, as ids.
fn targets_of(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> Vec<String> {
    operation_of(h, id)
        .targets()
        .iter()
        .map(|t| t.as_str().to_owned())
        .collect()
}

/// A point of shape `id`'s outline on the window, where a click picks it.
fn outline_point(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> egui::Pos2 {
    let shape = h
        .state()
        .app()
        .scene()
        .shape(&dc_project::InstanceId::new(id))
        .expect("the shape is drawn")
        .clone();
    common::on_screen(h, shape.flat[0].vertices()[0].point())
}

/// The target-editing bar's text for the Panel.
const EDITING_PANEL: &str = "Editing targets of “Panel”: click shapes to add or remove them";

/// The draft's targets while target editing is on, as ids.
fn draft_of(h: &Harness<'_, dreamcarve_app::Shell>) -> Option<Vec<String>> {
    h.state()
        .app()
        .target_editing()
        .map(|e| e.targets().iter().map(|t| t.as_str().to_owned()).collect())
}

/// Slice 27 (#619; capability 67, acceptance 98): the Targets row's
/// *Edit targets* enters target editing, and a bar across the top of the
/// Viewport says so, with *Done* and *Cancel*. A click on a shape or on a
/// Design Tree row toggles it in the draft; nothing is selected, nothing
/// moves and nothing is written. Escape is *Cancel*: the file is as it
/// was. Enter is *Done*: one undo step, `Set targets of "Panel"`. The
/// mode over, a click selects again.
#[test]
fn target_editing_bar_enter_is_done_escape_is_cancel() {
    let dir = scratch("target-editing");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    select_only(&mut h, "i1");
    open_operation(&mut h, "Panel");
    assert_eq!(targets_of(&h, "op1"), ["i2", "i3"]);
    assert!(h.query_by_label(EDITING_PANEL).is_none(), "no bar yet");
    // The row says how many and which; there is no list of names.
    h.get_by_label("2 shapes · pocket, island");
    assert!(h.query_by_label("Remove island").is_none());
    let start = h.state().app().project().clone();
    let selection = h.state().app().selection().clone();

    h.get_by_label("Edit targets").click();
    h.run();
    h.get_by_label(EDITING_PANEL);
    h.get_by_label("Done");
    h.get_by_label("Cancel");
    assert_eq!(draft_of(&h).unwrap(), ["i2", "i3"]);

    // A click on the square adds it to the draft, and only to the draft.
    let square = outline_point(&h, "i4");
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    assert_eq!(draft_of(&h).unwrap(), ["i2", "i3", "i4"]);
    h.get_by_label("3 shapes · pocket, island, square");
    assert_eq!(h.state().app().selection(), &selection, "nothing selected");
    assert_eq!(h.state().app().project(), &start, "nothing written");
    // A drag from a shape moves nothing: it is a marquee, which adds.
    common::drag(
        &mut h,
        square,
        square + egui::vec2(60.0, -40.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(h.state().app().project(), &start, "nothing moved");
    assert_eq!(h.state().app().selection(), &selection);
    assert_eq!(undo_steps(&h), 0);
    // The rest of the window is inert: the rail's steps and Delete do
    // nothing, and the step panel shows the Design Tree, not the list.
    assert!(
        h.query_by_role_and_label(Role::ListItem, "Cut out")
            .is_none(),
        "the Operations list makes way for the Design Tree"
    );
    h.get_by_role_and_label(Role::Tab, "Tools").click();
    h.run();
    assert_eq!(h.state().step(), dc_app::steps::Step::Operations);
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op1")
    );
    assert_eq!(h.state().app().project(), &start);

    // Escape is Cancel.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(EDITING_PANEL).is_none(), "the bar went");
    assert!(draft_of(&h).is_none());
    assert_eq!(h.state().app().project(), &start, "Cancel writes nothing");
    assert_eq!(undo_steps(&h), 0);
    assert_eq!(h.state().app().selection(), &selection);

    // Again: the square on the Viewport, the island's row in the tree,
    // then Enter.
    h.get_by_label("Edit targets").click();
    h.run();
    let square = outline_point(&h, "i4");
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    h.get_by_role_and_label(Role::TreeItem, "island").click();
    h.run();
    assert_eq!(draft_of(&h).unwrap(), ["i2", "i4"]);
    assert_eq!(h.state().app().selection(), &selection, "the row toggled");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.query_by_label(EDITING_PANEL).is_none(), "the bar went");
    assert_eq!(targets_of(&h, "op1"), ["i2", "i4"]);
    assert_eq!(undo_steps(&h), 1, "the whole edit is one step");
    assert_eq!(h.state().app().undo_label(), Some("Set targets of “Panel”"));

    // The bar's own buttons: Cancel, then Done.
    h.get_by_label("Edit targets").click();
    h.run();
    let outline = outline_point(&h, "i1");
    common::click_at(&mut h, outline, egui::Modifiers::NONE);
    h.get_by_label("Cancel").click();
    h.run();
    assert!(draft_of(&h).is_none());
    assert_eq!(targets_of(&h, "op1"), ["i2", "i4"]);
    h.get_by_label("Edit targets").click();
    h.run();
    common::click_at(&mut h, outline, egui::Modifiers::NONE);
    h.get_by_label("Done").click();
    h.run();
    assert_eq!(targets_of(&h, "op1"), ["i2", "i4", "i1"]);
    assert_eq!(undo_steps(&h), 2);

    // The mode over, a click selects as usual.
    let square = outline_point(&h, "i4");
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    assert_eq!(
        h.state().app().selection(),
        &dc_app::Selection::Instances(vec![dc_project::InstanceId::new("i4")]),
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 27 (#619; capability 67): in target editing a marquee adds every
/// shape it touches and an Alt-marquee removes them; the Targets row's
/// `⋯` menu holds *Use selection* and *Add selection*, one step each.
#[test]
fn target_marquee_and_the_selection_shortcuts() {
    let dir = scratch("target-marquee");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    open_operation(&mut h, "Square recess");
    assert_eq!(targets_of(&h, "op2"), ["i4"]);

    // A box round the four screw holes' first, from empty stock.
    let hole = h
        .state()
        .app()
        .scene()
        .shape(&dc_project::InstanceId::new("i5"))
        .expect("drawn")
        .bounds
        .expect("a box");
    let from = common::on_screen(
        &h,
        dc_geometry::Point::new(hole.min.x - 1.0, hole.min.y - 1.0),
    );
    let to = common::on_screen(
        &h,
        dc_geometry::Point::new(hole.max.x + 1.0, hole.max.y + 1.0),
    );
    h.get_by_label("Edit targets").click();
    h.run();
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(draft_of(&h).unwrap(), ["i4", "i5"], "the marquee adds");
    // Alt held: the same box removes.
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    assert_eq!(draft_of(&h).unwrap(), ["i4"], "the Alt-marquee removes");
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(targets_of(&h, "op2"), ["i4", "i5"]);
    assert_eq!(undo_steps(&h), 1);

    // The row's menu, with the island selected.
    select_only(&mut h, "i3");
    h.get_by_label("Target actions").click();
    h.run();
    h.get_by_label("Add selection").click();
    h.run();
    assert_eq!(targets_of(&h, "op2"), ["i4", "i5", "i3"]);
    h.get_by_label("Target actions").click();
    h.run();
    h.get_by_label("Use selection").click();
    h.run();
    assert_eq!(targets_of(&h, "op2"), ["i3"]);
    assert_eq!(undo_steps(&h), 3);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Set targets of “Square recess”")
    );

    // The window's close button is Cancel first: the unsaved-changes
    // sheet opens over a window that answers again.
    h.get_by_label("Edit targets").click();
    h.run();
    assert!(draft_of(&h).is_some());
    press_close(&mut h);
    h.step();
    h.run();
    assert!(draft_of(&h).is_none(), "closing cancels target editing");
    click_in_sheet(&mut h, "Cancel");
    assert_eq!(targets_of(&h, "op2"), ["i3"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 27 (#619; capability 67): hovering an operation's row in the
/// list previews its targets' marks, on the Viewport and in the Design
/// Tree, until the pointer leaves; the selected operation's come back.
#[test]
fn hovering_an_operation_row_previews_its_targets() {
    let dir = scratch("target-preview");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let marked = |h: &Harness<'_, dreamcarve_app::Shell>| {
        h.state()
            .app()
            .scene()
            .targets()
            .map(|m| (m.op.as_str().to_owned(), m.targets.len()))
    };
    assert_eq!(marked(&h), None, "no operation is selected");
    open_operation(&mut h, "Panel");
    assert_eq!(marked(&h), Some(("op1".to_owned(), 2)));

    let row = h
        .get_by_role_and_label(Role::ListItem, "Screw holes")
        .rect()
        .center();
    h.event(egui::Event::PointerMoved(row));
    h.run();
    assert_eq!(marked(&h), Some(("op3".to_owned(), 4)), "previewed");
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op1"),
        "the editor stays on the Panel"
    );

    let viewport = h.state().layout().viewport.center();
    h.event(egui::Event::PointerMoved(viewport));
    h.run();
    assert_eq!(marked(&h), Some(("op1".to_owned(), 2)), "the pointer left");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 27 (#619; capability 57, acceptance 98): while an operation is
/// selected the Design Tree's context menu offers *Add to "Panel"* on a
/// row that is not its target and *Remove from "Panel"* on one that is,
/// each one undo step; with no operation selected it offers neither.
#[test]
fn tree_context_menu_adds_and_removes_targets() {
    let dir = scratch("tree-targets");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let (add, remove) = ("Add to “Panel”", "Remove from “Panel”");

    // No operation selected: neither.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_secondary();
    h.run();
    h.get_by_label("Rename");
    assert!(h.query_by_label(add).is_none() && h.query_by_label(remove).is_none());
    h.key_press(egui::Key::Escape);
    h.run();

    // The Panel in the editor, then back to the tree: it is still the
    // operation the menu names.
    open_operation(&mut h, "Panel");
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_secondary();
    h.run();
    assert!(
        h.query_by_label(remove).is_none(),
        "the square is no target"
    );
    h.get_by_label(add).click();
    h.run();
    assert_eq!(targets_of(&h, "op1"), ["i2", "i3", "i4"]);
    assert_eq!(h.state().app().undo_label(), Some("Set targets of “Panel”"));
    assert_eq!(undo_steps(&h), 1);

    h.get_by_role_and_label(Role::TreeItem, "island")
        .click_secondary();
    h.run();
    assert!(h.query_by_label(add).is_none(), "the island is a target");
    h.get_by_label(remove).click();
    h.run();
    assert_eq!(targets_of(&h, "op1"), ["i2", "i4"]);
    assert_eq!(undo_steps(&h), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 76 with the editor (#539): clicking a Warnings row located
/// on an operation's field opens that operation in the editor with the
/// field focused, so typing goes straight into it.
#[test]
fn warnings_row_focuses_its_field() {
    let dir = scratch("warnings-focus");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let dc_project::Operation::Profile(mut cut_out) = operation_of(&h, "op4") else {
        panic!("the plaque's Cut out is a Profile");
    };
    cut_out.depth_mm = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateOperation(
            dc_project::Operation::Profile(cut_out),
        ))
        .expect("the depth is cleared");
    h.run();
    assert_eq!(h.state().app().editing(), None);

    h.get_by_label("Warnings").click();
    h.run();
    let row = dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|g| g.rows)
        .find(|r| r.field.as_deref() == Some("operations[op4].depth_mm"))
        .expect("validation locates the unset depth");
    click_drawer_row(&mut h, &row.name);
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op4"),
        "the row opens the operation in the editor"
    );
    assert_eq!(
        h.state().step(),
        dc_app::steps::Step::Operations,
        "on the step that owns it"
    );
    h.event(egui::Event::Text("7".to_owned()));
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
    let dc_project::Operation::Profile(after) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    assert_eq!(after.depth_mm, Some(7.0), "the depth field had the focus");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Opens the operation editor's group `title` (slice 26, #618: every group
/// but Setup starts closed): its header, the rightmost button of that
/// name, since the editor is the right-hand panel.
fn open_group(h: &mut Harness<'_, dreamcarve_app::Shell>, title: &str) {
    let header = h
        .query_all_by_role_and_label(Role::Button, title)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no {title} group"));
    header.scroll_to_me();
    h.run();
    h.query_all_by_role_and_label(Role::Button, title)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no {title} group"))
        .click();
    h.run();
}

/// Clicks the toolpath view's button `label`, opening the view from the
/// status bar first: the lowest of that name, since the Design Tree's
/// group rows have a *Show all* and *Hide all* of their own, for shapes.
fn click_card_button(h: &mut Harness<'_, dreamcarve_app::Shell>, label: &str) {
    open_toolpath_view(h);
    // The popover sits on the status bar, below any namesake in a panel.
    h.query_all_by_label(label)
        .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no {label} button"))
        .click();
    h.run();
}

/// Opens the toolpath view from the Viewport's *Layers* button
/// (redesign slice D4), unless it is open.
fn open_toolpath_view(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    if h.query_by_label("Cuts").is_none() {
        h.get_by_role_and_label(Role::Button, "Layers").click();
        h.run();
    }
    assert!(
        h.query_by_label("Cuts").is_some(),
        "the toolpath view is open"
    );
}

/// Closes the toolpath view (Escape).
fn close_toolpath_view(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        h.query_by_label("Cuts").is_none(),
        "the toolpath view is closed"
    );
}

/// Capability 73 (#545; `experience.md`, section 9, "Toolpath view popover"):
/// on the plaque, once generated, the Viewport draws every operation's
/// toolpath, one buffer each. Closing one operation's eye in the
/// Operations list (#539) drops that one and keeps
/// the rest; the toolpath view's *Show all* draws them all again, *Hide
/// all* none, and *Show all* restores them. A layer toggle in the view is
/// remembered in the preferences. None of it touches the project: it
/// stays clean, with nothing to undo.
#[test]
fn eye_hides_one_operation_show_all_restores() {
    let dir = scratch("toolpath-eyes");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    let all: Vec<dc_project::OperationId> = h
        .state()
        .app()
        .scene()
        .operations()
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(all.len(), 5, "the plaque's five operations");
    assert_eq!(h.state().drawn_toolpaths(), all, "every toolpath is drawn");

    // The Panel pocket's eye in the Operations list (#539).
    let panel = dc_project::OperationId::new("op1");
    go_to(&mut h, "Operations");
    h.get_by_label("Hide the toolpath of Panel").click();
    h.run();
    assert!(h.state().app().view().hidden_ops.contains(&panel));
    let rest: Vec<_> = all.iter().filter(|id| **id != panel).cloned().collect();
    assert_eq!(
        h.state().drawn_toolpaths(),
        rest,
        "the eye hides that one alone"
    );

    click_card_button(&mut h, "Show all");
    assert!(h.state().app().view().hidden_ops.is_empty());
    assert_eq!(h.state().drawn_toolpaths(), all, "Show all restores it");

    click_card_button(&mut h, "Hide all");
    assert!(h.state().drawn_toolpaths().is_empty(), "Hide all hides all");
    click_card_button(&mut h, "Show all");
    assert_eq!(
        h.state().drawn_toolpaths(),
        all,
        "and Show all brings them back"
    );

    open_toolpath_view(&mut h);
    h.get_by_label("Rapids").click();
    h.run();
    assert!(!h.state().app().view().visibility.rapids);
    assert!(
        !h.state().app().prefs().visibility.rapids,
        "the toggle is remembered between sessions"
    );

    let app = h.state().app();
    assert!(!app.is_dirty(), "the project is untouched");
    assert!(!app.can_undo(), "and nothing is in the history");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The tools of operation `id`'s clearing passes, by id.
fn clearing_tools(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> Vec<String> {
    match operation_of(h, id) {
        dc_project::Operation::VCarve(carve) => carve
            .clearing
            .iter()
            .map(|p| p.tool.as_str().to_owned())
            .collect(),
        other => panic!("{} is not a V-Carve", other.name()),
    }
}

/// Capability 70 and acceptance 75 in the window (slice 13, #542): on the
/// sign's Background, *Add pass* appends the 3.175 mm end mill's pass;
/// dragging its grip above the first pass is one step and shows
/// `clearing_not_descending`'s message under the list. The carve summary
/// is no longer the editor's foot (slice 26, #618):
/// `editor_has_no_check_says_box`.
#[test]
fn clearing_list_adds_and_drags() {
    let dir = scratch("clearing-list");
    let sign = copy_fixture("sign.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(sign), &dialogs),
        1280.0,
        2400.0,
    );
    open_operation(&mut h, "Background");
    assert_eq!(clearing_tools(&h, "op2"), ["t2"]);
    let steps = undo_steps(&h);

    h.get_by_label("Add pass").click();
    h.run();
    assert_eq!(clearing_tools(&h, "op2"), ["t2", "t3"], "the next smaller");
    assert_eq!(undo_steps(&h), steps + 1);

    let first = h.get_by_label("Drag to reorder pass 1").rect().center();
    let second = h.get_by_label("Drag to reorder pass 2").rect().center();
    common::drag(
        &mut h,
        second,
        first - egui::vec2(0.0, 12.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(clearing_tools(&h, "op2"), ["t3", "t2"], "dragged above");
    assert_eq!(undo_steps(&h), steps + 2, "one step");
    let warning = h
        .state()
        .app()
        .warnings()
        .iter()
        .find(|f| f.code.as_deref() == Some("clearing_not_descending"))
        .expect("validation warns")
        .message
        .clone();
    assert!(
        h.query_by_label(&warning).is_some(),
        "the warning shows under the list"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 68's "Near a point: typed or clicked" (#540) through the
/// window: with the plaque's Cut out starting near a point, *Pick on the
/// view* arms the Viewport, and the next click there sets `start.near` to
/// the project point the camera puts under the cursor, one undo step,
/// selects nothing, and disarms it. Capability 69: the Profile's *Place
/// tabs* is shown, disabled until slice 17 (#546).
#[test]
fn start_point_click_on_the_viewport() {
    use egui_kittest::kittest::NodeT as _;
    let dir = scratch("start-pick");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let dc_project::Operation::Profile(mut cut_out) = operation_of(&h, "op4") else {
        panic!("the plaque's Cut out is a Profile");
    };
    cut_out.start = dc_project::StartAt::Near {
        x_mm: 85.0,
        y_mm: 60.0,
    };
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateOperation(
            dc_project::Operation::Profile(cut_out),
        ))
        .expect("the start is set");
    h.run();
    open_operation(&mut h, "Cut out");
    assert!(
        !h.get_by_label("Place tabs").accesskit_node().is_disabled(),
        "the Profile's tab request can be placed (#546)"
    );
    // The start point is Strategy's, closed until opened (#618).
    open_group(&mut h, "Strategy");

    h.get_by_label("Pick on the view").scroll_to_me();
    h.run();
    h.get_by_label("Pick on the view").click();
    h.run();
    let steps = undo_steps(&h);
    // Empty stock near the plaque's bottom-left corner (stock 170 × 120).
    let want = dc_geometry::Point::new(5.0, 4.0);
    let at = common::on_screen(&h, want);
    let under = world_at(&h, at);
    common::click_at(&mut h, at, egui::Modifiers::NONE);
    let dc_project::Operation::Profile(after) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    let dc_project::StartAt::Near { x_mm, y_mm } = after.start else {
        panic!("still near a point: {:?}", after.start);
    };
    assert_near(
        dc_geometry::Point::new(x_mm, y_mm),
        under,
        "the start is the point under the click",
    );
    assert_eq!(undo_steps(&h), steps + 1, "one step");
    assert_eq!(h.state().app().undo_label(), Some("Set start of “Cut out”"));
    assert!(h.state().app().selection().is_empty(), "nothing selected");

    // One click: the Viewport is disarmed after it. Empty stock near the
    // bottom-right corner, clear of the depth legend (#616).
    let elsewhere = common::on_screen(&h, dc_geometry::Point::new(165.0, 5.0));
    common::click_at(&mut h, elsewhere, egui::Modifiers::NONE);
    let dc_project::Operation::Profile(still) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    assert_eq!(still.start, after.start);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// S18 in the editor (capability 68's Drill line, mock-up 07): the sign's
/// countersink shows its depth, derived from the mouth and the V-bit
/// (`2.5 + 6 / (2 · tan 45°)`), in a read-only Depth field where a Drill's
/// typed depth would be, and typing there writes nothing.
#[test]
fn countersink_depth_shows_derived_read_only() {
    use egui_kittest::kittest::NodeT as _;
    let dir = scratch("countersink-derived");
    let sign = copy_fixture("sign.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(sign), &dialogs),
        1280.0,
        800.0,
    );
    open_operation(&mut h, "Countersinks");
    let before = operation_of(&h, "op5");
    let depth = h.get_by_role_and_label(Role::TextInput, "Depth");
    let node = depth.accesskit_node();
    assert!(node.is_disabled(), "the derived depth is read-only");
    assert_eq!(node.value().as_deref(), Some("derived: 5.5"));
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Mouth diameter")
            .is_some(),
        "the countersink's own rows are shown"
    );
    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::TextInput, "Depth").click();
    h.run();
    h.event(egui::Event::Text("9".to_owned()));
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(operation_of(&h, "op5"), before, "nothing is typed");
    assert_eq!(undo_steps(&h), steps);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 68a in the window (slice 14, #543; mock-up 06, right): the
/// lettered inlay's editor shows the Male board group, the width's *same
/// as stock* placeholder, the derived line under Depth and the base under
/// the band (12 − 5 = 7 mm); clearing the thickness writes `null`, in one
/// step.
#[test]
fn inlay_editor_shows_the_male_board() {
    let dir = scratch("inlay-editor");
    let dc = copy_fixture("dc.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(dc), &dialogs),
        1280.0,
        900.0,
    );
    open_operation(&mut h, "DC");
    // The derived line and the margin are Depth's, closed until opened;
    // the values and the board thickness are Setup's (#618).
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Band margin")
            .is_none()
    );
    open_group(&mut h, "Depth");
    for label in [
        "Inlay depth",
        "Glue gap",
        "Plug depth",
        "Band margin",
        "Board thickness",
    ] {
        assert!(
            h.query_by_role_and_label(Role::TextInput, label).is_some(),
            "{label} is shown"
        );
    }
    assert!(
        h.query_by_label("male start plane 3 mm · band floor 5 mm below the board top")
            .is_some(),
        "the derived line"
    );
    assert!(
        h.query_by_label("base under the band 7 mm (at least 6 mm)")
            .is_some(),
        "the board left under the band, 12 − 5 mm"
    );
    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::TextInput, "Board thickness")
        .focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
    let dc_project::Operation::Inlay(inlay) = operation_of(&h, "op1") else {
        panic!("op1 is an Inlay");
    };
    assert_eq!(inlay.male_board.thickness_mm, None, "cleared to null");
    assert_eq!(undo_steps(&h), steps + 1, "one step");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Steps the window until generation is over, for `what`.
fn settle_generation(h: &mut Harness<'_, dreamcarve_app::Shell>, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "{what} generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
}

/// Capability 68 for an Inlay's drawn boundary in the window (phase 3's
/// capability 49f; the phase 4 audit's one functional gap): the lettered
/// inlay's Depth group has a *Boundary* dropdown reading *None (use the
/// band margin)*. Choosing `frame` writes `boundary: ["i3"]` as one undo
/// step, the dropdown reads `frame`, validation's line that the margin
/// now does nothing is under *Band margin*, and once the Inlay has
/// generated again the male board holds the frame as its drawn edge and
/// the male's cuts end at it, the end mill's centre its radius inside:
/// the band follows the boundary. *None* empties the list again.
#[test]
fn inlay_boundary_is_picked_in_the_editor_and_the_male_band_follows_it() {
    let dir = scratch("inlay-boundary");
    let dc = copy_fixture("dc.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(dc), &dialogs),
        1280.0,
        900.0,
    );
    settle_generation(&mut h, "the inlay pair");
    let boundary_of = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<String> {
        let dc_project::Operation::Inlay(inlay) = operation_of(h, "op1") else {
            panic!("op1 is an Inlay");
        };
        inlay
            .boundary
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect()
    };
    // The box of the male's cutting moves, in its board's own space.
    let cut_box = |h: &Harness<'_, dreamcarve_app::Shell>| {
        h.state()
            .app()
            .scene()
            .runs_on(dc_project::Board::Male)
            .flat_map(|run| run.toolpath.moves.iter())
            .filter(|m| !matches!(m.kind, dc_job::MoveKind::Rapid))
            .map(|m| dc_geometry::Point::new(m.to.x, m.to.y))
            .fold(None::<dc_geometry::Rect>, |acc, p| {
                Some(acc.map_or_else(|| dc_geometry::Rect::from_point(p), |r| r.including(p)))
            })
            .expect("the male is cut")
    };
    let margin_box = cut_box(&h);
    assert!(
        h.state()
            .app()
            .scene()
            .male_board()
            .expect("a male board")
            .boundaries
            .is_empty()
    );

    open_operation(&mut h, "DC");
    assert!(
        h.query_by_role_and_label(Role::ComboBox, "Boundary")
            .is_none(),
        "the row is Depth's, closed until opened"
    );
    open_group(&mut h, "Depth");
    let row = h.get_by_role_and_label(Role::ComboBox, "Boundary");
    assert_eq!(
        row.accesskit_node().value().as_deref(),
        Some("None (use the band margin)")
    );
    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::ComboBox, "Boundary").click();
    h.run();
    assert!(
        h.query_by_label("letter-D").is_none(),
        "a target is not offered as its own boundary"
    );
    h.get_by_label("frame").click();
    h.run();
    assert_eq!(boundary_of(&h), ["i3"]);
    assert_eq!(undo_steps(&h), steps + 1, "one step");
    assert_eq!(h.state().app().undo_label(), Some("Set boundary of “DC”"));
    assert_eq!(
        h.get_by_role_and_label(Role::ComboBox, "Boundary")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("frame")
    );

    settle_generation(&mut h, "the drawn boundary");
    assert!(
        h.query_by_label_contains("so margin_mm does nothing")
            .is_some(),
        "validation's line is under the margin the boundary replaces"
    );
    let app = h.state().app();
    let board = app.scene().male_board().expect("a male board");
    assert_eq!(board.boundaries.len(), 1, "the frame, as the male cuts it");
    let edge = board.boundaries[0].bounds().expect("an outline has a box");
    let (left, right) = (edge.min.x - board.offset.x, edge.max.x - board.offset.x);
    let cut = cut_box(&h);
    let radius_mm = 3.175 / 2.0;
    let near = |a: f64, b: f64| (a - b).abs() <= 0.05;
    assert!(
        near(cut.min.x, left + radius_mm)
            && near(cut.max.x, right - radius_mm)
            && near(cut.min.y, edge.min.y + radius_mm)
            && near(cut.max.y, edge.max.y - radius_mm),
        "the male's cuts {cut:?} end at the drawn edge {edge:?}"
    );
    assert!(
        !(near(margin_box.min.x, cut.min.x) && near(margin_box.max.y, cut.max.y)),
        "the margin's band was another: {margin_box:?}"
    );

    // None: the band is the margin's again.
    h.get_by_role_and_label(Role::ComboBox, "Boundary").click();
    h.run();
    h.get_by_label("None (use the band margin)").click();
    h.run();
    assert!(boundary_of(&h).is_empty());
    assert_eq!(undo_steps(&h), steps + 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #581, decision 1, in the window: choosing *V-bit* for an end mill in
/// the Tools step changes nothing in the project and shows an empty,
/// required *Angle* field; typing an angle makes the switch, one undo
/// step; leaving the field empty discards it.
#[test]
fn tools_tab_holds_v_bit_until_an_angle_is_typed() {
    let dir = scratch("v-bit-hold");
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let required = "Type the included angle to make this tool a V-bit";
    let choose_v_bit = |h: &mut Harness<'_, dreamcarve_app::Shell>| {
        h.get_by_label("1/4 end mill").click();
        h.run();
        h.get_by_label("Type").click();
        h.run();
        h.get_by_label("V-bit").click();
        h.run();
    };
    go_to(&mut h, "Tools");
    let before = h.state().app().project().clone();
    let steps = undo_steps(&h);

    // Held: nothing changes, and the angle field asks.
    choose_v_bit(&mut h);
    assert_eq!(h.state().app().project(), &before, "no project change");
    assert_eq!(undo_steps(&h), steps);
    assert!(h.query_by_label(required).is_some(), "the field asks");
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Angle")
            .is_some()
    );

    // Left empty: discarded.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(required).is_none(), "the hold is dropped");
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Angle")
            .is_none()
    );
    assert_eq!(h.state().app().project(), &before);

    // Typed: the switch, one undo step.
    choose_v_bit(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Angle")
        .type_text("60");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let t2 = h
        .state()
        .app()
        .project()
        .tools
        .iter()
        .find(|t| t.id.as_str() == "t2")
        .unwrap()
        .clone();
    assert_eq!(
        t2.cutter,
        dc_project::ToolKind::VBit {
            included_angle_deg: 60.0
        }
    );
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert!(h.query_by_label(required).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// Slice 29 (#621; functional 63, 79; acceptance 100): an empty
/// project's Tools step says *No tools yet* with *New tool* and *Add from
/// library…*. *New tool* adds `tool1`, selected, with its Name field
/// focused and the name selected, so typing replaces it; its rates show
/// *Required*. *Delete* is in the list's actions and deletes it.
#[test]
fn new_tool_focuses_its_name() {
    let dir = scratch("new-tool");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    go_to(&mut h, "Tools");
    assert!(
        h.query_by_label("No tools yet.").is_some(),
        "the empty list"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Add from library…")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Delete").is_none(),
        "nothing to delete"
    );
    let steps = undo_steps(&h);

    h.get_by_role_and_label(Role::Button, "New tool").click();
    h.run();
    let tools = &h.state().app().project().tools;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "tool1");
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert_eq!(h.state().app().undo_label(), Some("New tool “tool1”"));
    assert!(h.query_by_label("No tools yet.").is_none());
    let name = h.get_by_role_and_label(Role::TextInput, "Name");
    assert!(name.is_focused(), "the Name field has the focus");
    assert_eq!(name.value().as_deref(), Some("tool1"));
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Feed")
            .accesskit_node()
            .placeholder(),
        Some("Required"),
        "the unset feed is required"
    );

    // The name is selected: typing replaces it.
    h.get_by_role_and_label(Role::TextInput, "Name")
        .type_text("Upcut");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().app().project().tools[0].name, "Upcut");
    assert_eq!(undo_steps(&h), steps + 2);

    h.get_by_role_and_label(Role::Button, "Delete").click();
    h.run();
    assert!(h.state().app().project().tools.is_empty(), "deleted");
    assert_eq!(h.state().app().undo_label(), Some("Delete tool “Upcut”"));
    std::fs::remove_dir_all(&dir).ok();
}

/// Ticks library row `name` in the open Tool Library sheet.
fn tick_library_row(h: &mut Harness<'_, dreamcarve_app::Shell>, name: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::CheckBox, name)
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::CheckBox, name)
        .click();
    h.run();
}

/// #581, decision 2 (#537 deviation 7), and slice 30 (#622, capability
/// 64): the Tool Library's *Delete* asks first, naming the ticked tools,
/// in place of the panes (never a sheet above a sheet). *Cancel* keeps
/// them and leaves the file alone; *Delete* takes them out of the library
/// file. With nothing ticked it asks about the selected tool. The library
/// is outside project history, so the project does not change either way.
#[test]
fn library_delete_asks_in_place_and_names_the_tools() {
    let dir = scratch("library-remove");
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let bytes = std::fs::read(common::fixture("fusion/sample.tools")).unwrap();
    let library = dc_app::library::import(&dc_library::Library::default(), &bytes, false)
        .unwrap()
        .library;
    let written = dc_library::save(&library).unwrap();
    std::fs::write(config.join("tools.json"), &written).unwrap();
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 800.0);
    let project = h.state().app().project().clone();

    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    tick_library_row(&mut h, "1/4 in Flat End Mill");
    tick_library_row(&mut h, "5 mm Drill");

    // Delete asks, naming both in library order, and writes nothing yet.
    let question = "Delete 2 tools from your library: 5 mm Drill, 1/4 in Flat End Mill? \
                    Projects keep their copies.";
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label(question)
            .is_some(),
        "the sheet asks, naming the tools"
    );
    assert_eq!(
        h.query_all_by_role(Role::Dialog).count(),
        1,
        "in place: no second sheet"
    );
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_role(Role::CheckBox).is_none()
            && sheet.query_by_role(Role::ListItem).is_none()
            && sheet.query_by_label("Revert changes").is_none(),
        "the question replaces the panes"
    );
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written,
        "nothing is removed before the answer"
    );

    // Cancel keeps them, still ticked.
    h.get_by_role(Role::Dialog).get_by_label("Cancel").click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet.query_by_label(question).is_none(),
        "the question goes"
    );
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        assert!(
            sheet
                .query_by_role_and_label(Role::CheckBox, name)
                .is_some(),
            "{name} is still in the library"
        );
    }
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written,
        "Cancel leaves the library file alone"
    );

    // Delete, then Delete: both go, from the file too.
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label(question)
            .is_some()
    );
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label(question).is_none());
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        assert!(
            sheet
                .query_by_role_and_label(Role::CheckBox, name)
                .is_none(),
            "{name} is removed"
        );
    }
    let mut expected = library.clone();
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        let id = expected
            .tools
            .iter()
            .find(|t| t.tool.name == name)
            .unwrap()
            .id
            .clone();
        dc_library::remove(&mut expected, id.as_str()).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        dc_library::save(&expected).unwrap(),
        "the file is the library without them"
    );
    assert_eq!(
        h.state().app().project(),
        &project,
        "projects keep their copies"
    );

    // Nothing ticked: Delete asks about the selected tool, by name.
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "3 mm Ball End Mill")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "3 mm Ball End Mill")
        .click();
    h.run();
    h.get_by_role(Role::Dialog).get_by_label("Delete").click();
    h.run();
    let one = "Delete 1 tool from your library: 3 mm Ball End Mill? Projects keep their copies.";
    assert!(
        h.get_by_role(Role::Dialog).query_by_label(one).is_some(),
        "the selected tool, named"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        h.get_by_role(Role::Dialog).query_by_label(one).is_none(),
        "Escape is Cancel"
    );
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_role_and_label(Role::ListItem, "3 mm Ball End Mill")
            .is_some(),
        "and keeps the tool"
    );
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        dc_library::save(&expected).unwrap()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 31 (#623, capability 64b; `experience.md`, "Sheets"): *Import…*
/// replaces the Tool Library's panes with the import review in the same
/// sheet (never a second sheet). *Back*, or Escape, returns to the table
/// as it was: the ticks, the selection and the library file unchanged,
/// and no banner. The table has no *Replace existing* switch; the review
/// holds that choice.
#[test]
fn import_review_replaces_the_panes_in_place() {
    let dir = scratch("library-review-in-place");
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let bytes = std::fs::read(common::fixture("fusion/sample.tools")).unwrap();
    let library = dc_app::library::import(&dc_library::Library::default(), &bytes, false)
        .unwrap()
        .library;
    let written = dc_library::save(&library).unwrap();
    std::fs::write(config.join("tools.json"), &written).unwrap();
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 800.0);

    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    tick_library_row(&mut h, "5 mm Drill");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "3 mm Ball End Mill")
        .click();
    h.run();
    let table_before = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let sheet = h.get_by_role(Role::Dialog);
        assert!(
            sheet.query_by_label("Replace existing").is_none(),
            "no Replace existing on the table"
        );
        assert!(
            sheet.query_by_label_contains("1 ticked").is_some(),
            "the tick is kept"
        );
        assert_eq!(
            sheet
                .get_by_role_and_label(Role::CheckBox, "5 mm Drill")
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::True)
        );
        assert_eq!(
            sheet
                .get_by_role_and_label(Role::ListItem, "3 mm Ball End Mill")
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::True),
            "the selection is kept"
        );
        assert!(
            sheet.query_by_label_contains("Imported").is_none(),
            "nothing imported, so no banner"
        );
    };
    table_before(&h);

    for leave in ["Back", "Escape"] {
        let sample = common::fixture("fusion/sample.tools");
        dialogs.answer(&sample);
        h.get_by_role(Role::Dialog).get_by_label("Import…").click();
        h.run();
        assert_eq!(
            h.query_all_by_role(Role::Dialog).count(),
            1,
            "no second sheet"
        );
        let sheet = h.get_by_role(Role::Dialog);
        assert!(
            sheet.query_by_label("Import sample.tools").is_some(),
            "the review is in the sheet"
        );
        assert!(
            sheet.query_by_role(Role::CheckBox).is_none()
                && sheet.query_by_role(Role::ListItem).is_none()
                && sheet.query_by_label("Revert changes").is_none(),
            "the review replaces the panes"
        );
        assert!(sheet.query_by_label("Replace existing").is_none());
        if leave == "Back" {
            h.get_by_role(Role::Dialog).get_by_label("Back").click();
        } else {
            h.key_press(egui::Key::Escape);
        }
        h.run();
        assert_eq!(
            h.query_all_by_role(Role::Dialog).count(),
            1,
            "{leave} keeps the sheet"
        );
        table_before(&h);
        assert_eq!(
            std::fs::read_to_string(config.join("tools.json")).unwrap(),
            written,
            "{leave} writes nothing"
        );
    }
    assert_eq!(dialogs.asked(), ["open_library", "open_library"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #581, decision 3 (#538): the Warnings drawer's *Copy* reaches the OS
/// clipboard. The shell hands egui `check`'s text; eframe's window loop
/// gives it to egui-winit's clipboard, which is the OS clipboard (arboard)
/// only with eframe's `clipboard` feature, and a store inside the process
/// without it. The test checks the three links: *Copy* hands egui
/// `check`'s text; the app's own (non-dev) dependencies reach arboard
/// through egui-winit's `clipboard` feature; and the OS clipboard, through
/// arboard, holds that text as copied. egui-winit's clipboard itself is
/// not built here: its constructor is `unsafe`, which the workspace
/// forbids. The last link is skipped, saying why, where the machine has
/// no clipboard (Linux CI has no display server).
#[test]
fn warnings_copy_reaches_the_clipboard() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let tree = std::process::Command::new(cargo)
        .current_dir(common::repo_root())
        .args(["tree", "--locked", "-p", "dreamcarve-app", "-i", "arboard"])
        .args([
            "-e",
            "normal,features",
            "--target",
            "all",
            "--color",
            "never",
        ])
        .output()
        .expect("cargo tree runs");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(
        tree.contains("egui-winit feature \"clipboard\"")
            && tree.contains("eframe feature \"clipboard\""),
        "the app links arboard through eframe's `clipboard` feature:\n{tree}"
    );

    let dir = scratch("warnings-clipboard");
    let plaque = dir.join("plaque.dcv");
    std::fs::copy(
        common::repo_root()
            .join("crates/dreamcarve/tests/fixtures/holds-slug-migration/plaque.dcv"),
        &plaque,
    )
    .expect("fixture copied");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    let check = dc_app::warnings::copy_text(h.state().app());
    assert!(!check.is_empty(), "the plaque has findings to copy");

    h.get_by_label("Warnings").click();
    h.run();
    h.get_by_label("Copy").click();
    h.step();
    let copied: Vec<String> = h
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        copied,
        std::slice::from_ref(&check),
        "Copy hands egui check's text"
    );

    h.run();
    std::fs::remove_dir_all(&dir).unwrap();

    // What egui-winit's clipboard does with the command, with the feature on.
    let Ok(mut os) = arboard::Clipboard::new() else {
        println!(
            "warnings_copy_reaches_the_clipboard: this machine has no clipboard (no \
             display server), so the OS clipboard is not read back"
        );
        return;
    };
    for text in copied {
        os.set_text(text).expect("the clipboard takes the text");
    }
    assert_eq!(
        os.get_text().expect("the clipboard holds text"),
        check,
        "the OS clipboard holds check's text"
    );
}

/// The ids of the instances selected in the shell.
fn selected_ids(h: &Harness<'_, dreamcarve_app::Shell>) -> Vec<String> {
    h.state()
        .app()
        .selection()
        .instances()
        .iter()
        .map(|i| i.as_str().to_owned())
        .collect()
}

/// Instance `id` of the shell's project.
fn instance_of(h: &Harness<'_, dreamcarve_app::Shell>, id: &str) -> dc_project::FeatureInstance {
    h.state()
        .app()
        .project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("the project has {id}"))
}

/// Selects instance `id` as a click would.
fn select_only(h: &mut Harness<'_, dreamcarve_app::Shell>, id: &str) {
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new(id),
        ]));
    h.run();
}

/// Acceptance 54 (functional spec, capability 54): every shortcut in
/// `dreamcarve_app::keyboard::SHORTCUTS` reaches its command, and each with
/// a menu row shows its keys in that row. Ctrl/⌘ E is listed and bound to
/// nothing until slice 18 (#547) brings the export sheet: its row is
/// disabled and pressing it asks no dialog.
#[test]
fn keyboard_shortcuts_reach_their_commands() {
    use dreamcarve_app::keyboard::{self, Command as Key};

    let dir = scratch("keyboard-shortcuts");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque.clone()), &dialogs),
        1280.0,
        800.0,
    );
    let mut reached = std::collections::BTreeSet::new();

    // Every menu row with a shortcut shows it: the row's accessible name
    // is its label, then the keys as egui formats them for this platform.
    for menu in [
        keyboard::Menu::File,
        keyboard::Menu::Edit,
        keyboard::Menu::Arrange,
        keyboard::Menu::View,
    ] {
        let title = h.state().app().strings().get(menu.title()).to_owned();
        h.get_by_label(&title).click();
        h.run();
        for shortcut in keyboard::SHORTCUTS {
            let Some((row_menu, label)) = shortcut.menu else {
                continue;
            };
            if row_menu != menu {
                continue;
            }
            let keys = h.ctx.format_shortcut(&egui::KeyboardShortcut::new(
                shortcut.modifiers,
                shortcut.keys[0],
            ));
            let name = format!("{} {keys}", h.state().app().strings().get(label));
            assert!(
                h.query_by_label(&name).is_some(),
                "{title} shows `{name}` for {:?}",
                shortcut.command
            );
        }
        h.key_press(egui::Key::Escape);
        h.run();
    }

    // Ctrl 1 to 5 open the five steps, in order.
    for (key, command, step) in [
        (
            egui::Key::Num1,
            Key::GoToDesign,
            dc_app::steps::Step::Design,
        ),
        (egui::Key::Num2, Key::GoToStock, dc_app::steps::Step::Stock),
        (egui::Key::Num3, Key::GoToTools, dc_app::steps::Step::Tools),
        (
            egui::Key::Num4,
            Key::GoToOperations,
            dc_app::steps::Step::Operations,
        ),
        (
            egui::Key::Num5,
            Key::GoToExport,
            dc_app::steps::Step::Export,
        ),
    ] {
        command_key(&h, key);
        h.run();
        assert_eq!(h.state().step(), step, "{command:?} opens its step");
        reached.insert(command);
    }

    // Selection and placement.
    command_key(&h, egui::Key::A);
    h.run();
    let all: Vec<String> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .filter(|i| i.visible)
        .map(|i| i.id.as_str().to_owned())
        .collect();
    assert_eq!(selected_ids(&h), all, "Ctrl A selects every visible shape");
    reached.insert(Key::SelectAll);

    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().app().selection().is_empty(), "Escape clears it");
    reached.insert(Key::Escape);

    select_only(&mut h, "i4");
    let count = h.state().app().project().instances.len();
    command_key(&h, egui::Key::D);
    h.run();
    assert_eq!(
        h.state().app().project().instances.len(),
        count + 1,
        "Ctrl D makes a linked copy"
    );
    reached.insert(Key::Duplicate);
    command_key(&h, egui::Key::Z);
    h.run();
    assert_eq!(
        h.state().app().project().instances.len(),
        count,
        "Ctrl Z undoes it"
    );
    reached.insert(Key::Undo);
    shift_command_key(&h, egui::Key::Z);
    h.run();
    assert_eq!(
        h.state().app().project().instances.len(),
        count + 1,
        "Shift Ctrl Z redoes it"
    );
    reached.insert(Key::Redo);
    command_key(&h, egui::Key::Z);
    h.run();

    select_only(&mut h, "i4");
    h.key_press(egui::Key::H);
    h.run();
    assert!(!instance_of(&h, "i4").visible, "H hides the selection");
    reached.insert(Key::Hide);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    h.run();
    assert!(instance_of(&h, "i4").visible, "Shift H shows it");
    reached.insert(Key::Show);
    h.key_press(egui::Key::L);
    h.run();
    assert!(instance_of(&h, "i4").locked, "L locks the selection");
    h.key_press(egui::Key::L);
    h.run();
    assert!(!instance_of(&h, "i4").locked, "and L again unlocks it");
    reached.insert(Key::Lock);

    // Arrange (slice 25, #617): each turn is one step, undone here.
    for (modifiers, key, label, command) in [
        (
            egui::Modifiers::NONE,
            egui::Key::R,
            "Rotate “square” 90° clockwise",
            Key::RotateClockwise,
        ),
        (
            egui::Modifiers::SHIFT,
            egui::Key::R,
            "Rotate “square” 90° anticlockwise",
            Key::RotateAnticlockwise,
        ),
        (
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::H,
            "Flip “square” horizontally",
            Key::FlipHorizontal,
        ),
        (
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::V,
            "Flip “square” vertically",
            Key::FlipVertical,
        ),
    ] {
        h.key_press_modifiers(modifiers, key);
        h.run();
        assert_eq!(h.state().app().undo_label(), Some(label), "{command:?}");
        h.state_mut().app_mut().undo();
        h.run();
        reached.insert(command);
    }
    h.key_press(egui::Key::F2);
    h.run();
    assert!(
        h.get_by_label("Rename square").is_focused(),
        "F2 renames the selected shape in place"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    reached.insert(Key::Rename);

    for (modifiers, step_mm, command) in [
        (egui::Modifiers::NONE, 1.0, Key::Nudge),
        (egui::Modifiers::SHIFT, 10.0, Key::NudgeCoarse),
        (egui::Modifiers::ALT, 0.1, Key::NudgeFine),
    ] {
        let before = transform_of(&h, "i4");
        h.key_press_modifiers(modifiers, egui::Key::ArrowRight);
        h.run();
        assert_same_transform(
            transform_of(&h, "i4"),
            before.then(dc_geometry::Transform::translate(step_mm, 0.0)),
            &format!("{command:?} moves {step_mm} mm"),
        );
        reached.insert(command);
    }

    h.key_press(egui::Key::F);
    h.run();
    let bounds =
        dc_app::viewport::camera::fit_bounds(h.state().app().project(), h.state().app().scene());
    assert_same_view(camera(&h), fitted(&h, bounds), "F fits the view");
    reached.insert(Key::FitView);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F);
    h.run();
    let ids = h.state().app().selection().instances().to_vec();
    let bounds = h
        .state()
        .app()
        .scene()
        .bounds_of(&ids)
        .expect("i4 has bounds");
    assert_same_view(
        camera(&h),
        fitted(&h, bounds),
        "Shift F zooms to the selection",
    );
    reached.insert(Key::ZoomToSelection);

    // T arms the Tab tool (#546): the next Escape disarms it rather than
    // clearing the selection.
    h.key_press(egui::Key::T);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        !h.state().app().selection().is_empty(),
        "T armed the Tab tool, so Escape disarmed it"
    );
    reached.insert(Key::TabTool);

    let count = h.state().app().project().instances.len();
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(
        h.state().app().project().instances.len(),
        count - 1,
        "Delete deletes the selection"
    );
    reached.insert(Key::Delete);
    command_key(&h, egui::Key::Z);
    h.run();

    // Files: Save writes, Save As asks where, Open and Import ask what.
    command_key(&h, egui::Key::S);
    h.run();
    assert!(!h.state().app().is_dirty(), "Ctrl S saves");
    reached.insert(Key::Save);
    shift_command_key(&h, egui::Key::S);
    h.run();
    command_key(&h, egui::Key::O);
    h.run();
    command_key(&h, egui::Key::I);
    h.run();
    assert_eq!(
        dialogs.asked(),
        ["save_project plaque.dcv", "open_project", "open_drawing"],
        "Shift Ctrl S, Ctrl O and Ctrl I ask"
    );
    assert_ne!(h.state().step(), dc_app::steps::Step::Export);
    command_key(&h, egui::Key::E);
    h.run();
    assert_eq!(
        h.state().step(),
        dc_app::steps::Step::Export,
        "Ctrl E opens the Export step (#547)"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Export G-code")
            .is_some(),
        "where Export G-code is the primary button"
    );
    assert!(h.query_by_role(Role::Dialog).is_none(), "and no sheet");
    reached.extend([Key::SaveAs, Key::Open, Key::Import, Key::Export]);

    command_key(&h, egui::Key::N);
    h.run();
    assert!(
        h.state().app().project().instances.is_empty(),
        "Ctrl N starts a new project"
    );
    reached.insert(Key::New);
    command_key(&h, egui::Key::Q);
    h.run();
    assert!(h.state().close_allowed(), "Ctrl Q quits a saved project");
    reached.insert(Key::Quit);

    let listed: std::collections::BTreeSet<_> =
        keyboard::SHORTCUTS.iter().map(|s| s.command).collect();
    assert_eq!(reached, listed, "every shortcut in the table was pressed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `experience.md`, section 8, "Keyboard focus", and section 11: Tab walks
/// the window top-left to bottom-right (header, rail, step panel,
/// Viewport, Inspector, status bar) and wraps; Shift Tab walks back. The
/// five steps are stops of the walk, in order. The 2-px ring shows for
/// keyboard focus only: a click into a field focuses it with no ring, and
/// Tab from there draws it again.
#[test]
fn focus_order_and_visible_ring() {
    use dreamcarve_app::FocusRegion as R;

    let dir = scratch("focus-order");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    // The Operations step with an operation in the Inspector: every
    // region is there.
    open_operation(&mut h, "Panel");
    h.event(egui::Event::PointerGone);
    h.run();
    assert_eq!(h.state().focus(), None, "nothing has the focus at first");

    // Tab until the walk comes back to the header.
    let mut steps: Vec<String> = Vec::new();
    let mut walked: Vec<R> = Vec::new();
    for _ in 0..1000 {
        h.key_press(egui::Key::Tab);
        h.run();
        let Some(region) = h.state().focus() else {
            continue;
        };
        assert!(
            h.state().focus_ring().is_some(),
            "Tab's focus in {region:?} shows the ring"
        );
        if walked.last() != Some(&region) {
            walked.push(region);
        }
        if walked.len() > 1 && region == R::Header {
            break;
        }
        // The rail's step buttons, as Tab reaches them.
        if region == R::Rail
            && let Some(tab) = h
                .query_all_by_role(Role::Tab)
                .find(|node| node.accesskit_node().is_focused())
        {
            steps.push(tab.accesskit_node().label().unwrap_or_default());
        }
    }
    assert_eq!(
        walked,
        [
            R::Header,
            R::Rail,
            R::Step,
            R::Viewport,
            R::Inspector,
            R::Status,
            R::Header
        ],
        "Tab walks the regions in order and wraps"
    );
    assert_eq!(
        steps,
        ["Design", "Stock", "Tools", "Operations", "Export"],
        "the five steps are in the focus order"
    );

    // Shift Tab from the first stop goes back to the status bar.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run();
    assert_eq!(h.state().focus(), Some(R::Status), "Shift Tab walks back");

    // A click into a field focuses it with no ring.
    go_to(&mut h, "Stock");
    let width = h.get_by_role_and_label(Role::TextInput, "Width").rect();
    common::click_at(&mut h, width.center(), egui::Modifiers::NONE);
    assert_eq!(
        h.state().focus(),
        Some(R::Step),
        "the click focuses the field"
    );
    assert_eq!(h.state().focus_ring(), None, "a click shows no ring");
    h.key_press(egui::Key::Tab);
    h.run();
    assert!(
        h.state().focus_ring().is_some(),
        "Tab from it shows the ring"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 78 (functional capability 69; `experience.md`, section 8,
/// "Tab dragging"; #546): the Viewport's Tab tool arms the Viewport; a
/// click on the square's outline adds a tab there, in the square's
/// definition space, with the last-used size from the preferences, as one
/// undo step. The tool stays armed until Escape; after it a click on the
/// new tab selects the tab, which wins over the square it sits on.
#[test]
fn tab_tool_click_adds_a_tab() {
    let dir = scratch("tab-tool");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let before = h.state().app().project().tabs.clone();
    h.get_by_label("Tab tool").click();
    h.run();

    // The square's left edge (x 70, y 18–48 in project space).
    let square = common::on_screen(&h, dc_geometry::Point::new(70.0, 33.0));
    let under = world_at(&h, square);
    common::click_at(&mut h, square, egui::Modifiers::NONE);

    let project = h.state().app().project().clone();
    assert_eq!(project.tabs.len(), before.len() + 1, "one tab added");
    let added = project.tabs.last().unwrap().clone();
    assert_eq!(added.instance.as_str(), "i4", "on the square");
    let instance = project
        .instances
        .iter()
        .find(|i| i.id == added.instance)
        .unwrap();
    let placed = instance
        .transform
        .apply(dc_geometry::Point::new(added.x_mm, added.y_mm));
    assert!(
        (placed.x - 70.0).abs() < 1e-6 && (placed.y - under.y).abs() < 1e-2,
        "the outline's point under the click: {placed:?}, clicked {under:?}"
    );
    let size = h.state().app().prefs().tab;
    assert_eq!(
        (added.width_mm, added.height_mm, added.shape),
        (size.width_mm, size.height_mm, size.shape),
        "the last-used size"
    );
    assert_eq!(h.state().app().undo_label(), Some("Add tab"));

    // Escape disarms; a click on the tab selects it.
    h.key_press(egui::Key::Escape);
    h.run();
    let tab = common::on_screen(&h, placed);
    common::click_at(&mut h, tab, egui::Modifiers::NONE);
    assert_eq!(h.state().app().project().tabs.len(), before.len() + 1);
    assert_eq!(
        h.state().app().selection(),
        &dc_app::Selection::Tabs(vec![added.id.clone()])
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 78 in the window (#546): the editor's *Place tabs* places
/// the Cut out's request as one step; a tab dragged along the outline
/// lands on the outline, previewed until the release commits one
/// `MoveTab`, the other tabs untouched; Delete removes the selected tab.
#[test]
fn place_drag_and_delete_tabs() {
    let dir = scratch("tab-drag");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    open_operation(&mut h, "Cut out");
    h.get_by_label("Place tabs").scroll_to_me();
    h.run();
    h.get_by_label("Place tabs").click();
    h.run();
    let placed = h.state().app().project().tabs.clone();
    assert!(!placed.is_empty(), "the request is placed");
    assert_eq!(
        h.state().app().undo_label().map(str::to_owned),
        Some(format!("Place {} tabs", placed.len()))
    );

    // The lowest tab, on the plaque's bottom edge, dragged 15 mm along it.
    let mark = h
        .state()
        .app()
        .scene()
        .tabs()
        .iter()
        .min_by(|a, b| a.at_mm.y.total_cmp(&b.at_mm.y))
        .unwrap()
        .clone();
    let steps = undo_steps(&h);
    let from = common::on_screen(&h, mark.at_mm);
    let to = common::on_screen(
        &h,
        dc_geometry::Point::new(mark.at_mm.x + 15.0, mark.at_mm.y + 1.0),
    );
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(undo_steps(&h), steps + 1, "one step on release");
    assert_eq!(h.state().app().undo_label(), Some("Move tab"));
    let project = h.state().app().project().clone();
    let moved = project.tabs.iter().find(|t| t.id == mark.id).unwrap();
    let (_, gap_mm) = dc_job::tabs::along_outline_mm(&project, moved).unwrap();
    assert!(gap_mm < 1e-6, "on the outline: {gap_mm}");
    let before = placed.iter().find(|t| t.id == mark.id).unwrap();
    assert!((moved.x_mm - before.x_mm).abs() > 10.0, "it moved along");
    for (a, b) in project.tabs.iter().zip(&placed) {
        if a.id != mark.id {
            assert_eq!(a, b, "the others stay");
        }
    }
    assert_eq!(
        h.state().app().selection(),
        &dc_app::Selection::Tabs(vec![mark.id.clone()])
    );

    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(h.state().app().project().tabs.len(), placed.len() - 1);
    assert_eq!(h.state().app().undo_label(), Some("Delete tab"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A window point on operation `op`'s cuts, well inside the pane, that
/// the Viewport picks as that cut, with no shape outline within twice
/// the pick tolerance.
fn on_a_cut(h: &Harness<'_, dreamcarve_app::Shell>, op: &dc_project::OperationId) -> egui::Pos2 {
    use dc_app::viewport::pick::{self, PICK_TOLERANCE_PX};
    let app = h.state().app();
    let camera = app.view().camera;
    let pane = h.state().layout().viewport.shrink(20.0);
    let cuts = pick::shown_cuts(app.scene(), app.view(), app.selection());
    let clear_mm = camera.px_to_mm(2.0 * PICK_TOLERANCE_PX);
    let away = |p: dc_geometry::Point| {
        app.scene()
            .shapes()
            .iter()
            .flat_map(|s| s.outlines.iter())
            .flat_map(dc_geometry::Polyline::segments)
            .all(|seg| dc_geometry::point_segment_distance_mm(seg, p) > clear_mm)
    };
    app.scene()
        .operation(op)
        .expect("the operation is in the Scene")
        .runs
        .iter()
        .flat_map(|r| dc_app::scene::strokes(&r.run.toolpath))
        .filter(|s| s.kind == dc_app::scene::StrokeKind::Cut && s.arc.is_none())
        .map(|s| dc_geometry::Point::new((s.from.x + s.to.x) / 2.0, (s.from.y + s.to.y) / 2.0))
        .filter(|p| away(*p))
        .map(|p| common::on_screen(h, p))
        .find(|at| {
            pane.contains(*at)
                && pick::pick_with_cuts(
                    app.scene(),
                    &camera,
                    common::in_pane(h, *at),
                    PICK_TOLERANCE_PX,
                    0,
                    &cuts,
                ) == dc_app::Hit::Toolpath(op.clone())
        })
        .expect("a cut away from every outline")
}

/// Functional capability 72 (#576): on the generated plaque, hovering
/// the Panel pocket's raster away from every outline names the operation
/// in the status bar, and a click on it opens the Pocket in the editor;
/// the shape selection is unchanged.
#[test]
fn clicking_a_cut_opens_its_operation() {
    let dir = scratch("toolpath-click");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();

    // Select the square first: the click on the cut must leave it.
    let square = common::on_screen(&h, dc_geometry::Point::new(70.0, 33.0));
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    let selected = h.state().app().selection().clone();
    assert_eq!(selected.instances().len(), 1, "the square is selected");
    assert_eq!(h.state().app().editing(), None);

    let panel = dc_project::OperationId::new("op1");
    let at = on_a_cut(&h, &panel);

    // Hovering names the operation in the status bar.
    let named = |h: &Harness<'_, dreamcarve_app::Shell>| h.query_all_by_label("Panel").count();
    let before = named(&h);
    h.event(egui::Event::PointerMoved(at));
    h.run();
    assert_eq!(named(&h), before + 1, "the status bar names the Panel");

    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    common::click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(
        h.state().app().editing(),
        Some(&panel),
        "the click opens the Panel in the editor"
    );
    assert_eq!(
        h.state().step(),
        dc_app::steps::Step::Operations,
        "and the step that owns it"
    );
    assert!(
        h.query_by_role_and_label(Role::ListItem, "Panel").is_some(),
        "its row is in the step panel"
    );
    assert_eq!(
        h.state().app().selection(),
        &selected,
        "the shape selection is unchanged"
    );
    assert!(
        h.query_by_label("Panel · Pocket").is_some(),
        "the editor shows the Pocket"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 76 (`redesign.md`, "The window"): a Warnings row opens the
/// step that edits what it names. A row about a tool's field opens the
/// Tools step with that tool in the Inspector; a row about the stock
/// opens the Stock step.
#[test]
fn warnings_row_opens_the_step_that_owns_it() {
    use dc_app::steps::Step;
    let dir = scratch("warnings-step");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    // The stock loses its thickness and a used tool its feed: two errors.
    let project = h.state().app().project().clone();
    let mut stock = project.stock.clone();
    stock.thickness_mm = None;
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    for command in [Command::SetStock(stock), Command::UpdateTool(tool.clone())] {
        h.state_mut().app_mut().run(command).unwrap();
    }
    h.run();
    assert_eq!(h.state().step(), Step::Export);

    let row_about = |h: &Harness<'_, dreamcarve_app::Shell>, field: &str| {
        dc_app::warnings::panel(h.state().app())
            .into_iter()
            .flat_map(|g| g.rows)
            .find(|r| r.field.as_deref() == Some(field))
            .unwrap_or_else(|| panic!("a row about {field}"))
            .name
    };
    // The rail's Warnings button opens the drawer.
    h.get_by_label_contains("Warnings: 2 errors").click();
    h.run();
    assert!(h.state().warnings_open());

    let feed = format!("tools[{used}].feed_mm_min");
    let line = row_about(&h, &feed);
    click_drawer_row(&mut h, &line);
    assert_eq!(h.state().step(), Step::Tools);
    assert!(
        h.state().layout().inspector.is_some(),
        "the tool is in the Inspector"
    );
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Name")
            .value()
            .as_deref(),
        Some(tool.name.as_str())
    );

    let line = row_about(&h, "stock.thickness_mm");
    click_drawer_row(&mut h, &line);
    assert_eq!(h.state().step(), Step::Stock);
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Thickness")
            .is_some()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 76 ("clicking a row selects the operation and scrolls to
/// the field"; slice D9): a row about a field that is not an operation's
/// focuses it too, so typing goes straight into it, as
/// `warnings_row_focuses_its_field` shows for an operation's. On the
/// plaque with a used tool's feed and the stock's thickness cleared and
/// the clearance height under the operations' safe Z: the feed's row
/// opens the Tools step on that tool and what is typed becomes its feed;
/// the thickness's row opens the Stock step and what is typed becomes the
/// thickness; the clearance's row opens the Export step with its Heights
/// group, which starts closed, and what is typed becomes the clearance.
/// Each used to open the step and leave the focus where it was.
#[test]
fn warnings_row_focuses_a_tools_a_stocks_and_a_settings_field() {
    use dc_app::steps::Step;
    let dir = scratch("warnings-focus-fields");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let project = h.state().app().project().clone();
    let mut stock = project.stock.clone();
    stock.thickness_mm = None;
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    let mut heights = project.heights;
    heights.clearance_mm = Some(2.0);
    for command in [
        Command::SetStock(stock),
        Command::UpdateTool(tool),
        Command::SetHeights(heights),
    ] {
        h.state_mut().app_mut().run(command).unwrap();
    }
    h.run();
    h.get_by_label_contains("Warnings:").click();
    h.run();
    assert!(h.state().warnings_open());

    // Clicks the first row about `field`, from the Design step (the
    // Export step lists the errors too, under the same names), and types
    // `text` into whatever has the focus, leaving it with Tab.
    let type_at_row = |h: &mut Harness<'_, dreamcarve_app::Shell>, field: &str, text: &str| {
        go_to(h, "Design");
        let line = dc_app::warnings::panel(h.state().app())
            .into_iter()
            .flat_map(|g| g.rows)
            .find(|r| r.field.as_deref() == Some(field))
            .unwrap_or_else(|| panic!("a row about {field}"))
            .name;
        // The drawer lists more rows than it shows at once.
        h.get_by_label(&line).scroll_to_me();
        h.run();
        h.get_by_label(&line).click();
        h.run();
        h.event(egui::Event::Text(text.to_owned()));
        h.run();
        h.key_press(egui::Key::Tab);
        h.run();
    };

    type_at_row(&mut h, &format!("tools[{used}].feed_mm_min"), "1234");
    assert_eq!(h.state().step(), Step::Tools);
    let feed = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let project = h.state().app().project();
        project
            .tools
            .iter()
            .find(|t| t.id == used)
            .unwrap()
            .feed_mm_min
    };
    assert_eq!(
        feed(&h),
        Some(1234.0),
        "the tool's feed field had the focus"
    );

    type_at_row(&mut h, "stock.thickness_mm", "12");
    assert_eq!(h.state().step(), Step::Stock);
    assert_eq!(
        h.state().app().project().stock.thickness_mm,
        Some(12.0),
        "the thickness field had the focus"
    );

    // The field holds `2`, and the caret is put after it.
    type_at_row(&mut h, "heights.clearance_mm", "5");
    assert_eq!(h.state().step(), Step::Export);
    assert_eq!(
        h.state().app().project().heights.clearance_mm,
        Some(25.0),
        "the clearance field had the focus, in its opened group"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50 (`redesign.md`, "The window"): pinned to the rail's
/// bottom are Warnings, which names the counts and opens the drawer;
/// Appearance, a menu of the three choices with the current one checked,
/// which sets the preference; and Help, the Help menu's rows.
#[test]
fn rail_bottom_has_warnings_appearance_and_help() {
    let dir = scratch("rail-bottom");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let rail = h.state().layout().rail;
    let steps = rail_steps(&h);
    let last_step = steps.last().expect("five steps").2;
    let names = ["Warnings: 1 hint", "Appearance: Dark", "Help and About"];
    let mut top = last_step.bottom();
    for name in names {
        let rect = h.get_by_role_and_label(Role::Button, name).rect();
        assert!(rail.contains_rect(rect), "{name} is on the rail");
        assert!(rect.top() >= top, "{name} is under what comes before it");
        top = rect.bottom();
    }
    assert!(
        rail.bottom() - top < 16.0,
        "the three are pinned to the rail's bottom"
    );

    assert!(!h.state().warnings_open());
    h.get_by_label("Warnings: 1 hint").click();
    h.run();
    assert!(h.state().warnings_open(), "Warnings opens the drawer");
    h.get_by_label("Warnings: 1 hint").click();
    h.run();
    assert!(!h.state().warnings_open(), "and closes it");

    h.get_by_label("Appearance: Dark").click();
    h.run();
    let checked = |h: &Harness<'_, dreamcarve_app::Shell>, name: &str| {
        h.get_by_role_and_label(Role::Button, name)
            .accesskit_node()
            .toggled()
            == Some(egui::accesskit::Toggled::True)
    };
    assert!(checked(&h, "Dark") && !checked(&h, "Light") && !checked(&h, "System"));
    h.get_by_role_and_label(Role::Button, "Light").click();
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Light);
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    assert!(h.query_by_label("Appearance: Light").is_some());

    h.get_by_label("Help and About").click();
    h.run();
    assert!(h.query_by_label("User Guide").is_some());
    h.get_by_label("About DreamCarve").click();
    h.run();
    assert!(
        h.state()
            .message()
            .is_some_and(|m| m.starts_with("DreamCarve ")),
        "About says the version"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `experience.md` section 13 and design question 2 (#549): the plaque's
/// workspace, generated, on the Operations step with its Panel pocket
/// open in the Inspector, in a 1280 × 800 window, compared with the
/// committed pair
/// `tests/snapshots/workspace_light.png` and `workspace_dark.png` under
/// `kittest.toml`'s thresholds. Linux only: font rasterisation differs per
/// OS, so the other two run the behaviour tests without image comparison.
/// Like every rendering test it says so and renders nothing where the
/// machine has no wgpu adapter (CI's Linux job needs Mesa's lavapipe).
/// `UPDATE_SNAPSHOTS=1` rewrites the pair; a mismatch leaves
/// `<name>.new.png` and `<name>.diff.png` beside it.
#[test]
fn snapshot_workspace_light_and_dark() {
    if !cfg!(target_os = "linux") {
        println!(
            "snapshot_workspace_light_and_dark: image snapshots are compared on Linux only \
             (design question 2)"
        );
        return;
    }
    let dir = scratch("snapshot-workspace");
    // Opened in place and never saved: the fixture is only read.
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = common::wgpu_harness(
        "snapshot_workspace_light_and_dark",
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    ) else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    open_operation(&mut h, "Panel");
    assert!(h.query_by_label("Panel · Pocket").is_some(), "the editor");

    let mut failures = Vec::new();
    // The appearance is the preference's: dark by default, whatever the
    // OS shows.
    for (choice, theme, name) in [
        (dc_app::Theme::Light, egui::Theme::Light, "workspace_light"),
        (dc_app::Theme::Dark, egui::Theme::Dark, "workspace_dark"),
    ] {
        h.state_mut().app_mut().prefs_mut().theme = choice;
        h.run();
        assert_eq!(h.ctx.theme(), theme);
        if let Err(err) = h.try_snapshot(name) {
            failures.push(format!("{name}: {err}"));
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What the status bar takes of the window's bottom, px (`experience.md`,
/// section 3): its 28 px and the 6-px margin under it.
const STATUS_BAR_PX: f32 = 34.0;

/// Where in the last frame's paint order the first text shape containing
/// `needle` is.
fn painted_at(h: &Harness<'_, dreamcarve_app::Shell>, needle: &str) -> Option<usize> {
    h.output().shapes.iter().position(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().contains(needle))
    })
}

/// `experience.md` section 8, "Sheets" (#549 review): a sheet sits above
/// everything on the Viewport, the depth legend included. In *By depth*,
/// with the Tool Library sheet open, the legend's labels paint before
/// (under) the sheet's title and its scrim.
#[test]
fn depth_legend_paints_under_a_sheet() {
    let dir = scratch("legend-under-sheet");
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    h.get_by_label("By depth").click();
    h.run();
    assert!(painted_at(&h, "stock top").is_some(), "the legend is drawn");
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_some(), "the sheet is open");
    let legend = painted_at(&h, "stock top").expect("the legend is still drawn");
    let sheet = painted_at(&h, "Tool Library").expect("the sheet's title");
    assert!(
        legend < sheet,
        "the legend ({legend}) paints under the sheet ({sheet})"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 80's clause "the clearance height and safe Z above the
/// stock top (positive, in the rapid grey band)", on what the legend
/// paints. The plaque with a 10 mm clearance height and 5 mm safe Z, *By
/// depth* on: the legend has one bar filled with the rapid grey (the
/// Viewport's rapid colour at 35 %); the `+10 clearance` and `+5 safe Z`
/// labels are level with it, clearance above safe Z; and the stock top's
/// label and every cut's are below it, on the ramp.
#[test]
fn depth_legend_puts_clearance_and_safe_z_in_the_rapid_grey_band() {
    let dir = scratch("legend-band");
    let text = std::fs::read_to_string(common::fixture("plaque.dcv")).unwrap();
    let text = text
        .replace("\"safe_z_mm\": 15.0", "\"safe_z_mm\": 5.0")
        .replace("\"clearance_mm\": null", "\"clearance_mm\": 10.0");
    let plaque = dir.join("plaque.dcv");
    std::fs::write(&plaque, text).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    h.get_by_label("By depth").click();
    h.run();

    let grey = theme::DARK.viewport_rapid.gamma_multiply(0.35);
    let bands: Vec<egui::Rect> = h
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.fill == grey => Some(rect.rect),
            _ => None,
        })
        .collect();
    assert_eq!(bands.len(), 1, "one rapid-grey band: {bands:?}");
    let band = bands[0];
    let label = |h: &Harness<'_, dreamcarve_app::Shell>, text: &str| {
        let rects = text_rects(h, text);
        assert_eq!(rects.len(), 1, "one `{text}` label: {rects:?}");
        rects[0]
    };
    let clearance = label(&h, "+10  clearance");
    let safe_z = label(&h, "+5  safe Z");
    for (name, rect) in [("clearance", clearance), ("safe Z", safe_z)] {
        assert!(
            band.y_range().contains(rect.center().y),
            "{name}'s label {rect:?} is level with the band {band:?}"
        );
        assert!(rect.left() > band.right(), "{name} is beside the band");
    }
    assert!(
        clearance.center().y < safe_z.center().y,
        "clearance above safe Z"
    );
    for cut in ["0  stock top", "Panel", "Screw holes, Cut out"] {
        let rect = label(&h, cut);
        assert!(
            rect.center().y >= band.bottom() - 0.5,
            "`{cut}` {rect:?} is on the ramp, under the band {band:?}"
        );
    }
    assert!(
        clearance.center().y < label(&h, "0  stock top").center().y,
        "the band is above the stock top"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The window rectangles of the last frame's text shapes whose text
/// contains `needle`.
fn text_rects(h: &Harness<'_, dreamcarve_app::Shell>, needle: &str) -> Vec<egui::Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().contains(needle) => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// #549, first comment (seen by slice 16): at the minimum window, 1280 ×
/// 768, in *By depth* with *Program extents* on, the depth legend must
/// not cover the extents caption. The caption moves under the bit-edge
/// box when its place at the box's top-left is under the legend or the
/// card.
#[test]
fn depth_legend_leaves_the_extents_caption_readable() {
    let dir = scratch("legend-caption");
    let plaque = common::fixture("plaque.dcv");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        768.0,
    );
    settle_export(&mut h);
    h.get_by_label("By depth").click();
    h.run();
    open_toolpath_view(&mut h);
    h.get_by_label("Program extents").click();
    h.run();
    close_toolpath_view(&mut h);
    let caption = text_rects(&h, "Program extents (bit edge");
    assert_eq!(caption.len(), 1, "the caption is drawn once: {caption:?}");
    let legend = ["stock top", "Screw holes, Cut out"]
        .iter()
        .flat_map(|label| text_rects(&h, label))
        .reduce(|a, b| a.union(b))
        .expect("the legend's labels");
    assert!(
        !caption[0].intersects(legend),
        "the caption {:?} is clear of the legend {legend:?}",
        caption[0]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Functional 66 and acceptance 97 (slice 26, #618): the editor's header
/// holds the name, editable in place. Clicking it, typing and Enter is one
/// undo step; Escape reverts. There is no *Enabled* checkbox and no
/// *Name* field in the editor: enabled is the list's.
#[test]
fn editor_header_renames_in_place() {
    let dir = scratch("header-rename");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    open_operation(&mut h, "Cut out");
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Enabled")
            .is_none(),
        "no enabled checkbox in the editor"
    );
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Name").is_none(),
        "no name field in the editor"
    );
    assert!(h.query_by_label("Identity").is_none(), "no Identity group");
    let steps = undo_steps(&h);

    h.get_by_role_and_label(Role::Button, "Cut out").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Rename Cut out")
        .type_text("Outline");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(operation_of(&h, "op4").name(), "Outline", "renamed");
    assert_eq!(undo_steps(&h), steps + 1, "as one step");
    assert_eq!(h.state().app().undo_label(), Some("Rename “Cut out”"));
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Rename Cut out")
            .is_none(),
        "Enter leaves the field"
    );

    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Rename Outline")
        .type_text("Other");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(operation_of(&h, "op4").name(), "Outline", "Escape reverts");
    assert_eq!(undo_steps(&h), steps + 1, "and adds no step");
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op4"),
        "the editor stays open"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Steps the window until a tooltip has had time to show (egui's
/// `tooltip_delay`, 0.5 s), at 0.1 s a frame.
fn wait_for_tooltip(h: &mut Harness<'_, dreamcarve_app::Shell>) {
    for _ in 0..10 {
        h.step();
    }
    h.run();
}

/// Asserts the text field `label` shows the required-field look
/// (functional 66b; `experience.md`, section 7, "required (unset)"):
/// *Required* as its placeholder, no line under it carrying `message`, and
/// `message` as its tooltip.
fn assert_required(h: &mut Harness<'_, dreamcarve_app::Shell>, label: &str, message: &str) {
    use egui_kittest::kittest::NodeT as _;
    let field = h.get_by_role_and_label(Role::TextInput, label);
    assert_eq!(
        field.accesskit_node().placeholder(),
        Some("Required"),
        "{label}'s placeholder"
    );
    assert_eq!(field.accesskit_node().value().as_deref(), Some(""));
    // Acceptance 97, "and the glyph in the Feed field": `circle-x`, by
    // its codepoint in the bundled icon font's own map, is painted once
    // on the field's line, inside its frame, right of where the text
    // starts and before the unit.
    let frame = field.rect();
    let circle_x = include_str!("../assets/icons/lucide/codepoints.json")
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"circle-x\": "))
        .and_then(|code| code.trim_end_matches(',').parse::<u32>().ok())
        .and_then(char::from_u32)
        .expect("the bundled map names circle-x")
        .to_string();
    let glyphs: Vec<egui::Rect> = text_rects(h, &circle_x)
        .into_iter()
        .filter(|glyph| frame.y_range().contains(glyph.center().y))
        .collect();
    assert_eq!(
        glyphs.len(),
        1,
        "{label}: one glyph on its line: {glyphs:?}"
    );
    assert!(
        glyphs[0].left() > frame.left() && glyphs[0].right() < frame.right() + 64.0,
        "{label}: the glyph {:?} is in the field {frame:?}, before its unit",
        glyphs[0]
    );
    assert!(
        h.query_by_label(message).is_none(),
        "no line under {label} carries the message"
    );
    h.get_by_role_and_label(Role::TextInput, label).hover();
    wait_for_tooltip(h);
    assert!(
        h.query_by_label(message).is_some(),
        "{label}'s tooltip is the file's message: {message}"
    );
}

/// Functional 66b and acceptance 97 (slice 26, #618): a tool with no feed
/// shows *Required* in its Feed field, not a paragraph under it, and the
/// file's message as the field's tooltip; the Stock step's thickness on a
/// new project the same.
#[test]
fn required_field_shows_required_not_a_message() {
    let dir = scratch("required-field");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let options_for = |project| options(&dir.join("config"), project, &dialogs);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.1)
        .build_eframe(|cc| {
            dreamcarve_app::Shell::new(cc, options_for(Some(plaque))).expect("the shell starts")
        });
    h.run();
    go_to(&mut h, "Tools");
    h.get_by_label("Set me").click();
    h.run();
    let tool = h.state().app().project().tools[0].clone();
    assert_eq!(
        tool.feed_mm_min, None,
        "the plaque's first tool has no feed"
    );
    let message = dc_app::tools::unset_rates(&tool)
        .into_iter()
        .find(|(field, _)| *field == "feed_mm_min")
        .map(|(_, message)| message)
        .expect("the unset feed's message");
    assert_required(&mut h, "Feed", &message);
    drop(h);

    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.1)
        .build_eframe(|cc| {
            dreamcarve_app::Shell::new(cc, options_for(None)).expect("the shell starts")
        });
    h.run();
    go_to(&mut h, "Stock");
    assert_eq!(h.state().app().project().stock.thickness_mm, None);
    let message = h
        .state()
        .app()
        .warnings()
        .iter()
        .find(|f| f.field.as_deref() == Some("stock.thickness_mm"))
        .map(|f| f.message.clone())
        .expect("validation locates the unset thickness");
    assert_required(&mut h, "Thickness", &message);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 71's clause for *Delete* on a tool in use (capability 63),
/// in the window: on the plaque's Tools step with the 1/4 end mill
/// selected, which three operations cut with, *Delete* is disabled and
/// its tooltip is the command's refusal, naming each of them; with the
/// unused placeholder selected it is enabled and has no such tooltip.
#[test]
fn delete_is_disabled_for_a_tool_in_use_and_names_its_operations() {
    let dir = scratch("delete-in-use");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.1)
        .build_eframe(|cc| {
            dreamcarve_app::Shell::new(cc, options(&dir.join("config"), Some(plaque), &dialogs))
                .expect("the shell starts")
        });
    h.run();
    go_to(&mut h, "Tools");
    let refusal = "tool “1/4 end mill” is used by “Panel”, “Square recess” and “Cut out”";

    h.get_by_label("1/4 end mill").click();
    h.run();
    let delete = h.get_by_role_and_label(Role::Button, "Delete");
    assert!(
        delete.accesskit_node().is_disabled(),
        "a tool in use cannot be deleted"
    );
    assert!(h.query_by_label(refusal).is_none(), "not said until asked");
    h.get_by_role_and_label(Role::Button, "Delete").hover();
    wait_for_tooltip(&mut h);
    assert!(
        h.query_by_label(refusal).is_some(),
        "the tooltip names the operations that use it"
    );
    let tools = h.state().app().project().tools.len();
    h.get_by_role_and_label(Role::Button, "Delete").click();
    h.run();
    assert_eq!(h.state().app().project().tools.len(), tools, "nothing went");

    // The unused placeholder can go.
    h.get_by_label("Set me").click();
    h.run();
    assert!(
        !h.get_by_role_and_label(Role::Button, "Delete")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_role_and_label(Role::Button, "Delete").hover();
    wait_for_tooltip(&mut h);
    assert!(h.query_by_label_contains("is used by").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Functional 66 and experience checklist 67 (slice 26, #618): the carve
/// summary `check` prints is not in the editor (slice 28 makes it a note
/// in the Warnings drawer). For every kind with a record, in the
/// committed fixtures, once generation has settled, no node is labelled
/// `check says`; the V-Carves have a summary that would have shown.
#[test]
fn editor_has_no_check_says_box() {
    let dir = scratch("no-check-says");
    for (fixture, names) in [
        (
            "plaque.dcv",
            &["Panel", "Cut out", "Screw holes", "Border and squiggle"][..],
        ),
        (
            "sign.dcv",
            &["Hanging holes", "Lettering", "Countersinks"][..],
        ),
        ("dc.dcv", &["DC"][..]),
    ] {
        let path = copy_fixture(fixture, &dir);
        let dialogs = QueuedDialogs::default();
        let mut h = harness(
            options(&dir.join("config"), Some(path), &dialogs),
            1280.0,
            800.0,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while h.state().app().generation().busy() || h.state().worker_busy() {
            assert!(std::time::Instant::now() < deadline, "generation settles");
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        h.run();
        for name in names {
            open_operation(&mut h, name);
            let id = h
                .state()
                .app()
                .editing()
                .cloned()
                .expect("an operation is open");
            let op = operation_of(&h, id.as_str());
            if matches!(op, dc_project::Operation::VCarve(_)) {
                assert!(
                    !dc_app::editor::carve_summary(h.state().app(), &id).is_empty(),
                    "{name} has a carve summary"
                );
            }
            assert!(
                h.query_by_label("check says").is_none(),
                "{fixture}, {name}: no check says box"
            );
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Functional 50b and 76a (slice 28, #620): selecting the sign's
/// Lettering shows its first facts in the status bar's message cell;
/// clicking the cell opens the Warnings drawer, where the note is a row
/// labelled with `check`'s line for it.
#[test]
fn message_cell_opens_the_drawer_at_the_note() {
    let dir = scratch("message-cell-note");
    let path = copy_fixture("sign.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        800.0,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "generation settles");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
    open_operation(&mut h, "Lettering");
    assert!(!h.state().warnings_open());
    h.get_by_role_and_label(
        Role::Button,
        "Lettering · deepest cut 1.658 mm, no flat depth",
    )
    .click();
    h.run();
    assert!(h.state().warnings_open(), "the cell opens the drawer");
    assert!(
        h.query_by_label(
            "V-Carve op3 `Lettering`: deepest cut 1.658 mm below the stock top, no flat depth."
        )
        .is_some(),
        "the drawer shows the Lettering's note"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// No node of the accessibility tree names the file with its extension:
/// the project's name is its stem, in the OS window title and the header
/// (capability 50).
fn assert_no_file_name(h: &Harness<'_, dreamcarve_app::Shell>, what: &str) {
    let found: Vec<String> = h
        .query_all_by_label_contains("plaque.dcv")
        .map(|n| {
            format!(
                "{:?} {:?} {:?}",
                n.accesskit_node().role(),
                n.accesskit_node().label(),
                n.accesskit_node().value()
            )
        })
        .collect();
    assert!(found.is_empty(), "{what}: {found:?}");
}

/// Slice 23 (#615), acceptance 93, and capability 50: the window title
/// is the project's name without its extension, `*` while unsaved,
/// `Untitled` before the first save; the header shows the same name in
/// its centre, with a dot while unsaved; no widget shows `plaque.dcv`.
#[test]
fn window_title_is_the_project_name_without_extension() {
    let dir = scratch("title-name");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    assert_eq!(h.state().title(), "plaque — DreamCarve");
    assert_no_file_name(&h, "the plaque, clean");
    let name = h.get_by_role_and_label(Role::Label, "plaque").rect();
    assert!(
        name.max.y <= 48.0 && (name.center().x - 640.0).abs() < 1.0,
        "the header names the project in its centre: {name:?}"
    );

    widen_stock(&mut h);
    assert_eq!(h.state().title(), "plaque* — DreamCarve");
    assert_no_file_name(&h, "the plaque, edited");
    assert!(
        h.query_by_label("plaque · unsaved changes").is_some(),
        "the header marks the unsaved changes"
    );

    command_key(&h, egui::Key::S);
    h.run();
    assert!(!h.state().app().is_dirty(), "Save wrote the file");
    assert_eq!(h.state().title(), "plaque — DreamCarve");
    assert!(h.query_by_role_and_label(Role::Label, "plaque").is_some());

    command_key(&h, egui::Key::N);
    h.run();
    assert!(h.state().app().source().is_none(), "New has no file");
    assert_eq!(h.state().title(), "Untitled — DreamCarve");
    assert!(h.query_by_role_and_label(Role::Label, "Untitled").is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The rail's steps, top to bottom: label, whether current, rect.
fn rail_steps(h: &Harness<'_, dreamcarve_app::Shell>) -> Vec<(String, bool, egui::Rect)> {
    let mut steps: Vec<(String, bool, egui::Rect)> = h
        .query_all_by_role(Role::Tab)
        .map(|n| {
            (
                n.accesskit_node().label().unwrap_or_default(),
                n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True),
                n.rect(),
            )
        })
        .collect();
    steps.sort_by(|a, b| a.2.min.y.total_cmp(&b.2.min.y));
    steps
}

/// The step whose rail button is current.
fn current_step(h: &Harness<'_, dreamcarve_app::Shell>) -> String {
    let current: Vec<String> = rail_steps(h)
        .into_iter()
        .filter(|step| step.1)
        .map(|step| step.0)
        .collect();
    assert_eq!(current.len(), 1, "one step is current: {current:?}");
    current[0].clone()
}

/// Capabilities 50 and 50a (`redesign.md`, "The steps"): the rail has the
/// five steps in the workflow's order, each a 56 × 52 button in the 64-px
/// card; the step panel is titled for the current one. A new project
/// opens on Design and the finished plaque on Export; the step picked
/// stays, through an edit, until the next project is opened.
#[test]
fn rail_has_five_steps_and_a_project_opens_on_its_first_incomplete_one() {
    use dc_app::steps::Step;
    let dir = scratch("rail-steps");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();

    // A new project: nothing in it, so Design.
    let h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    let steps = rail_steps(&h);
    let labels: Vec<&str> = steps.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(labels, ["Design", "Stock", "Tools", "Operations", "Export"]);
    assert_eq!(current_step(&h), "Design", "a new project opens on Design");
    assert_eq!(h.state().step(), Step::Design);
    let rail = h.state().layout().rail;
    for (label, _, rect) in &steps {
        assert!(rail.contains_rect(*rect), "{label} is on the rail");
        assert_eq!(
            (rect.width(), rect.height()),
            (56.0, 52.0),
            "{label}'s button"
        );
    }
    drop(h);

    // The plaque: every step done but the export.
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    assert_eq!(current_step(&h), "Export", "the plaque opens on Export");
    let panel = h.state().layout().step_panel.expect("the step panel");
    let title = h.get_by_role_and_label(Role::Heading, "Export").rect();
    assert!(
        panel.contains_rect(title),
        "the step panel is titled Export"
    );
    assert!(
        h.query_by_label("Ready to export.").is_some(),
        "and says what the step holds"
    );

    // The step picked stays until the next project: New opens on Design.
    go_to(&mut h, "Tools");
    assert_eq!(current_step(&h), "Tools");
    assert!(h.query_by_role_and_label(Role::Heading, "Tools").is_some());
    widen_stock(&mut h);
    assert_eq!(current_step(&h), "Tools", "an edit keeps the user's pick");
    command_key(&h, egui::Key::N);
    h.run();
    click_in_sheet(&mut h, "Don't Save");
    assert_eq!(current_step(&h), "Design", "New opens on Design");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50 (`redesign.md`, "The window"): the header's one primary
/// button is *Continue*, which opens the next step that is not complete,
/// and on the Export step *Export G-code*, the export command, disabled
/// while an error blocks export. The rail marks the steps: a finished
/// plaque's are complete, and a tool without its feed marks Tools and
/// Export with an error.
#[test]
fn continue_goes_to_the_next_step_and_export_is_the_primary_on_export() {
    use dc_app::steps::{Severity, State, Step};
    let dir = scratch("continue");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();

    // A new project: Continue walks on from Design.
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    for step in Step::ALL {
        assert_eq!(h.state().step_states().of(step), State::Empty, "{step:?}");
    }
    h.get_by_role_and_label(Role::Button, "Continue").click();
    h.run();
    assert_eq!(current_step(&h), "Stock");
    drop(h);

    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    for step in [Step::Design, Step::Stock, Step::Tools, Step::Operations] {
        assert_eq!(
            h.state().step_states().of(step),
            State::Complete,
            "{step:?}"
        );
    }
    assert_eq!(h.state().step_states().of(Step::Export), State::Empty);
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::Button, "Continue").click();
    h.run();
    assert_eq!(current_step(&h), "Export", "every step between is complete");
    assert!(
        h.query_by_role_and_label(Role::Button, "Continue")
            .is_none(),
        "one primary button"
    );
    // Export G-code is the export command: it waits for the programs to
    // be built, then writes them beside the project, and the step shows
    // what was written.
    assert!(
        h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled()
            || !h.state().export_planning(),
        "disabled while the programs are still being built"
    );
    settle_export(&mut h);
    let export = h.get_by_role_and_label(Role::Button, "Export G-code");
    assert!(!export.accesskit_node().is_disabled());
    export.click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none(), "no sheet opens");
    assert!(
        h.query_by_label("2 programs written").is_some(),
        "Export G-code writes the programs"
    );
    assert_eq!(h.state().step_states().of(Step::Export), State::Complete);
    assert!(
        h.query_by_label("Exported. Nothing has changed since.")
            .is_some(),
        "and the step says so"
    );

    // A tool an operation cuts with loses its feed: export is blocked.
    let project = h.state().app().project();
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    h.state_mut()
        .app_mut()
        .run(Command::UpdateTool(tool))
        .unwrap();
    h.run();
    let failed = State::Attention(Severity::Error);
    assert_eq!(h.state().step_states().of(Step::Tools), failed);
    assert_eq!(h.state().step_states().of(Step::Export), failed);
    assert_eq!(h.state().step_states().of(Step::Design), State::Complete);
    assert!(
        h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled(),
        "disabled while an error blocks export"
    );
    assert!(
        h.query_by_label("1 error blocks export.").is_some(),
        "the step says why"
    );
    // The step shows what blocks it: the finding, where the result was.
    assert!(
        h.query_by_label("2 programs written").is_none(),
        "the result goes when the project changes"
    );
    assert_eq!(
        in_export_step_containing(&h, "feed_mm_min is not set"),
        1,
        "the blocking finding is listed"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Inspector's title for a selection of shapes, when it shows.
fn properties_title(h: &Harness<'_, dreamcarve_app::Shell>) -> Option<egui::Rect> {
    h.query_by_label_contains("Properties ·").map(|n| n.rect())
}

/// Capability 50 (`redesign.md`, "The window" and "The steps"): the
/// Inspector is there only while the step has something selected. On
/// Design it is the selection's Properties, with the Arrange group in
/// its header; its close button deselects, and the Viewport takes the
/// room. The same selection on another step shows no Inspector. The
/// panels' widths are `gui.toml`'s.
#[test]
fn inspector_shows_properties_only_with_a_selection() {
    let dir = scratch("inspector");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let config = dir.join("config");
    let mut file = GuiPrefs::default();
    file.panels.left_width = 340.0;
    file.panels.right_width = 310.0;
    gui_prefs::save(&Paths::in_dir(config.clone()), &file).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 800.0);
    go_to(&mut h, "Design");
    let empty = h.state().layout();
    assert_eq!(empty.inspector, None, "nothing selected: no Inspector");
    assert!(properties_title(&h).is_none());
    assert!(
        (empty.step_panel.unwrap().width() - 340.0).abs() < 0.5,
        "the step panel's width is the preference's: {empty:?}"
    );

    select_only(&mut h, "i4");
    let layout = h.state().layout();
    let inspector = layout
        .inspector
        .expect("the Inspector, with a shape selected");
    assert!(
        (inspector.width() - 310.0).abs() < 0.5,
        "the Inspector's width is the preference's: {inspector:?}"
    );
    assert!(
        layout.viewport.width() < empty.viewport.width(),
        "the Inspector takes its room from the Viewport"
    );
    let title = properties_title(&h).expect("Properties with the square selected");
    let x = h.get_by_role_and_label(Role::TextInput, "X").rect();
    assert!(inspector.contains_rect(title) && inspector.contains_rect(x));
    // The Arrange group is in the header, above the fields.
    for name in ["Rotate 90° clockwise", "Flip horizontal", "Flip vertical"] {
        let button = h.get_by_role_and_label(Role::Button, name).rect();
        assert!(
            inspector.contains_rect(button) && button.max.y <= x.min.y,
            "{name} is in the Inspector's header: {button:?}"
        );
    }

    // The same selection on another step: no Inspector.
    go_to(&mut h, "Stock");
    assert_eq!(h.state().layout().inspector, None);
    assert!(properties_title(&h).is_none());
    let width = h.get_by_role_and_label(Role::TextInput, "Width").rect();
    assert!(
        h.state()
            .layout()
            .step_panel
            .is_some_and(|panel| panel.contains_rect(width)),
        "the stock's fields are the step panel's"
    );

    // Its close button deselects.
    go_to(&mut h, "Design");
    assert!(h.state().layout().inspector.is_some());
    h.get_by_label("Close the Inspector").click();
    h.run();
    assert!(h.state().app().selection().is_empty());
    assert_eq!(h.state().layout().inspector, None);
    assert_eq!(
        h.state().layout().viewport.width(),
        empty.viewport.width(),
        "the Viewport takes the room back"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 60 (`redesign.md`, "The steps"): a project whose stock has
/// no thickness under a drawing (the Coaster as `init` makes it) opens on
/// the Stock step, which the rail marks with an error; its Thickness
/// field says *Required*. Project ▸ Project Settings… opens that step.
#[test]
fn a_project_without_a_thickness_opens_on_stock() {
    use dc_app::steps::{Severity, State, Step};
    let dir = scratch("stock-error");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let config = dir.join("config");
    let dialogs = QueuedDialogs::default();

    let mut h = harness(options(&config, None, &dialogs), 1280.0, 800.0);
    assert_eq!(
        h.state().app().project().stock.thickness_mm,
        None,
        "a new project has no thickness"
    );
    assert_eq!(
        h.state().step_states().of(Step::Stock),
        State::Empty,
        "which is no error while there is nothing to cut"
    );
    h.get_by_role_and_label(Role::Button, "Coaster").click();
    h.run();
    assert_eq!(current_step(&h), "Stock", "the Coaster opens on Stock");
    assert_eq!(
        h.state().step_states().of(Step::Stock),
        State::Attention(Severity::Error)
    );
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Thickness")
            .accesskit_node()
            .placeholder(),
        Some("Required")
    );
    drop(h);

    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 800.0);
    assert_eq!(current_step(&h), "Export");
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Project Settings…").click();
    h.run();
    assert_eq!(
        current_step(&h),
        "Stock",
        "Project Settings… opens the Stock step"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 23 (#615), capability 53: Units is the Stock step's first row,
/// a `mm` / `in` segmented control; choosing `in` is one undo step and
/// changes no stored length.
#[test]
fn units_is_the_first_row_of_the_stock_step() {
    let dir = scratch("settings-units");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    go_to(&mut h, "Stock");
    let panel = h.state().layout().step_panel.expect("the step panel");
    let title = h.get_by_role_and_label(Role::Heading, "Stock").rect();
    let mm = h.get_by_role_and_label(Role::RadioButton, "mm");
    assert_eq!(
        mm.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "in mm"
    );
    let mm = mm.rect();
    let inch = h.get_by_role_and_label(Role::RadioButton, "in").rect();
    assert!(title.max.y <= mm.min.y, "under the title");
    assert!((mm.min.y - inch.min.y).abs() < 0.5, "one row");
    // No field or button sits between the title and the control.
    let first = h
        .query_all_by_role(Role::TextInput)
        .chain(h.query_all_by_role(Role::Button))
        .chain(h.query_all_by_role(Role::ComboBox))
        .map(|n| n.rect())
        .filter(|r| panel.contains_rect(*r))
        .map(|r| r.min.y)
        .fold(f32::INFINITY, f32::min);
    assert!(mm.min.y <= first, "Units is the first row: {mm:?}, {first}");
    let width = h.get_by_role_and_label(Role::TextInput, "Width").rect();
    assert!(mm.max.y <= width.min.y, "above the stock's size");

    let before = h.state().app().project().clone();
    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::RadioButton, "in").click();
    h.run();
    let after = h.state().app().project().clone();
    assert_eq!(after.display_units, dc_project::Units::Inch);
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert_eq!(h.state().app().undo_label(), Some("Set display units"));
    let mut same = after;
    same.display_units = before.display_units;
    assert_eq!(same, before, "no stored length changes");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The eight toggles of the toolpath view (capability 73).
const TOOLPATH_TOGGLES: [&str; 8] = [
    "Cuts",
    "Rapids",
    "Plunges",
    "Direction arrows",
    "Start points",
    "Tab marks",
    "Only the selected operation",
    "Program extents",
];

/// The plaque, generated, in a 1280 × 800 window; the project is a copy
/// in `dir` when `copy`, else the fixture opened in place.
fn generated_plaque(dir: &std::path::Path, copy: bool) -> Harness<'static, dreamcarve_app::Shell> {
    let plaque = if copy {
        copy_fixture("plaque.dcv", dir)
    } else {
        common::fixture("plaque.dcv")
    };
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    h
}

/// Capability 73, acceptance 94 (#549, row 03; redesign slice D4): the
/// toolpath view is a popover from the *Layers* button of the Viewport's
/// floating controls, not a card on the Viewport. The colour mode and the
/// button are absent while the project has no toolpath (the plaque's
/// shapes with no operation) and float at the Viewport's top right on the
/// generated plaque; a click shows the popover, titled *Toolpaths*, with
/// the eight toggles, under the button; Escape closes it. The status bar
/// has no Toolpaths button.
#[test]
fn toolpaths_button_opens_the_popover_and_escape_closes_it() {
    let dir = scratch("toolpaths-popover");
    let bytes = std::fs::read(common::fixture("plaque.dcv")).unwrap();
    let mut shapes_only = dc_project::load(&bytes).unwrap().project;
    shapes_only.operations.clear();
    let path = dir.join("shapes.dcv");
    std::fs::write(&path, dc_project::save(&shapes_only).unwrap()).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        800.0,
    );
    settle_export(&mut h);
    for absent in ["Layers", "By operation", "By depth", "Toolpaths"] {
        assert!(
            h.query_by_label(absent).is_none(),
            "no {absent} without a toolpath"
        );
    }
    assert!(h.query_by_label("Cuts").is_none(), "and no card");
    assert!(h.query_by_label("Fit").is_some(), "the tools still float");

    let mut h = generated_plaque(&dir.join("plaque"), false);
    assert!(
        h.query_by_label("Toolpaths").is_none(),
        "no Toolpaths button in the status bar"
    );
    let pane = h.state().layout().viewport;
    let button = h.get_by_role_and_label(Role::Button, "Layers").rect();
    assert!(
        pane.contains_rect(button)
            && button.center().x > pane.center().x
            && button.center().y < pane.top() + 48.0,
        "Layers floats at the Viewport's top right: {button:?} in {pane:?}"
    );
    // The colour mode is beside it, one control of two.
    for mode in ["By operation", "By depth"] {
        let rect = h.get_by_role_and_label(Role::RadioButton, mode).rect();
        assert!(
            pane.contains_rect(rect) && rect.right() <= button.left(),
            "{mode} is left of Layers: {rect:?}"
        );
    }
    for toggle in TOOLPATH_TOGGLES {
        assert!(h.query_by_label(toggle).is_none(), "no card: {toggle}");
    }

    h.get_by_role_and_label(Role::Button, "Layers").click();
    h.run();
    assert_eq!(
        h.query_all_by_label("Toolpaths").count(),
        1,
        "the popover's title"
    );
    for toggle in TOOLPATH_TOGGLES {
        let rect = h.get_by_label(toggle).rect();
        assert!(
            rect.min.y >= button.max.y && rect.right() <= button.right() + 1.0,
            "{toggle} is in the popover under the button: {rect:?}"
        );
    }
    assert_eq!(
        h.query_all_by_label("By depth").count(),
        1,
        "the colour mode is the Viewport's control only"
    );

    h.key_press(egui::Key::Escape);
    h.run();
    for toggle in TOOLPATH_TOGGLES {
        assert!(h.query_by_label(toggle).is_none(), "closed: {toggle}");
    }
    assert!(h.query_by_label("Toolpaths").is_none(), "closed");

    // Slice 35 (#627), capability 79a: View ▸ Toolpaths… opens it too.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Toolpaths…").click();
    h.run();
    for toggle in TOOLPATH_TOGGLES {
        let rect = h.get_by_label(toggle).rect();
        assert!(rect.min.y >= button.max.y, "from the menu: {toggle}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50b (redesign slice D4): the status bar's cells are quiet
/// pills in two groups, so that none moves when another comes or goes.
/// The units and the counts are at the bar's left; the estimate ends at
/// its right. Hovering a shape adds the pointer's cell left of the
/// estimate, and an edit puts the generation cell where the estimate
/// was: through both, the units and the counts stay where they are and
/// the right-hand cell still ends at the bar's right.
#[test]
fn status_bar_cells_hold_still() {
    let dir = scratch("status-still");
    let mut h = generated_plaque(&dir, true);
    let status_top = 800.0 - STATUS_BAR_PX;
    let in_bar = |h: &Harness<'_, dreamcarve_app::Shell>, label: &str| {
        h.query_all_by_label(label)
            .map(|node| node.rect())
            .find(|rect| rect.top() >= status_top)
            .unwrap_or_else(|| panic!("{label} is in the status bar"))
    };
    let units = in_bar(&h, "mm");
    let counts = in_bar(&h, "Warnings");
    let estimate = h.get_by_label_contains("Est. ").rect();
    assert!(units.left() < counts.left() && counts.right() < estimate.left());
    // 8 px of window margin and 10 px of pill padding from the right.
    let right_end = 1280.0 - 8.0 - 10.0;
    assert!(
        (estimate.right() - right_end).abs() < 6.0,
        "the estimate ends at the bar's right: {estimate:?}"
    );
    for gone in ["Toolpaths", "X "] {
        assert!(
            h.query_all_by_label_contains(gone)
                .all(|node| node.rect().top() < status_top),
            "no {gone} cell in the status bar"
        );
    }

    // Hovering the square names it, left of the estimate.
    go_to(&mut h, "Design");
    let square = h
        .state()
        .app()
        .scene()
        .shape(&dc_project::InstanceId::new("i4"))
        .expect("drawn")
        .bounds
        .expect("a box");
    let on_outline = common::on_screen(
        &h,
        dc_geometry::Point::new(square.min.x, (square.min.y + square.max.y) / 2.0),
    );
    h.event(egui::Event::PointerMoved(on_outline));
    h.run();
    let hover = in_bar(&h, "square");
    assert!(hover.right() < h.get_by_label_contains("Est. ").rect().left());
    assert_eq!(in_bar(&h, "mm"), units, "the units hold still");
    assert_eq!(in_bar(&h, "Warnings"), counts, "the counts hold still");
    assert_eq!(h.get_by_label_contains("Est. ").rect(), estimate);
    h.event(egui::Event::PointerGone);
    h.run();

    // An edit: generation takes the estimate's place at the right.
    let mut stock = h.state().app().project().stock.clone();
    stock.thickness_mm = Some(12.5);
    h.state_mut()
        .app_mut()
        .run(Command::SetStock(stock))
        .unwrap();
    h.step();
    assert!(h.query_by_label_contains("Est. ").is_none());
    let stop = in_bar(&h, "Stop");
    assert!(
        (stop.right() - (1280.0 - 8.0 - 10.0)).abs() < 6.0,
        "generation ends at the bar's right: {stop:?}"
    );
    assert_eq!(in_bar(&h, "mm"), units, "the units hold still");
    assert_eq!(in_bar(&h, "Warnings").left(), counts.left());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The polish item of slice D4: a blank, untitled project has nothing to
/// cut, so nothing is counted against it. The status bar says *No
/// problems*, the rail's Warnings button has no count, and no step is
/// marked; `check`'s one error, the unset thickness, is still in the
/// drawer. Once a drawing is in the project it counts.
#[test]
fn a_blank_project_shows_no_problem() {
    let dir = scratch("blank-no-problem");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    assert!(
        h.query_all_by_label_contains("1 error").next().is_none(),
        "no error count anywhere"
    );
    assert!(h.query_by_label("No problems").is_some(), "the status bar");
    assert!(
        h.query_by_label("Warnings: No problems").is_some(),
        "the rail's button counts nothing"
    );
    // The drawer still lists what check reports.
    h.get_by_label("Warnings: No problems").click();
    h.run();
    assert!(h.state().warnings_open());
    assert!(
        h.query_all_by_label_contains("stock.thickness_mm is not set")
            .next()
            .is_some(),
        "the drawer lists check's error"
    );
    h.get_by_label("Close the Warnings drawer").click();
    h.run();

    // The coaster, as its drawing makes it: now it is a problem.
    h.get_by_role_and_label(Role::Button, "Coaster").click();
    h.run();
    assert!(
        h.query_all_by_label_contains("error").next().is_some(),
        "the unset thickness counts once there is something to cut"
    );
    assert!(h.query_by_label("No problems").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `redesign.md`, "The window" (slice D4): the Warnings drawer is a card
/// over the bottom of the Viewport's card, 240 px high and 8 px in from
/// its edges, not a panel under it: the Viewport's card keeps its size
/// when the drawer opens. The stock as Fit frames it, and the cursor's
/// read-out, stand clear of it. The grab handle at its top drags it
/// taller, and no further than leaves the Viewport's controls in view.
/// The pointer over the drawer is the drawer's: it names no shape under
/// it.
#[test]
fn warnings_drawer_is_a_card_over_the_viewport() {
    let dir = scratch("drawer-over");
    let mut h = generated_plaque(&dir, true);
    go_to(&mut h, "Design");
    let before = h.state().layout().viewport;
    assert_eq!(h.state().layout().drawer, None);
    let stock_rect = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let stock = &h.state().app().project().stock;
        egui::Rect::from_two_pos(
            common::on_screen(h, dc_geometry::Point::new(0.0, 0.0)),
            common::on_screen(h, dc_geometry::Point::new(stock.width_mm, stock.height_mm)),
        )
    };
    let fitted_before = stock_rect(&h);

    h.get_by_label_contains("Warnings: ").click();
    h.run();
    let pane = h.state().layout().viewport;
    assert_eq!(pane, before, "the Viewport's card keeps its size");
    let drawer = h.state().layout().drawer.expect("the drawer is open");
    assert!(pane.contains_rect(drawer), "{drawer:?} is over {pane:?}");
    for (edge, gap) in [
        ("left", drawer.left() - pane.left()),
        ("right", pane.right() - drawer.right()),
        ("bottom", pane.bottom() - drawer.bottom()),
    ] {
        assert!((gap - 8.0).abs() < 0.5, "8 px in at the {edge}: {gap}");
    }
    assert!((drawer.height() - 240.0).abs() < 0.5, "{drawer:?}");
    let copy = h.get_by_label("Copy").rect();
    assert!(drawer.contains_rect(copy), "its header is on it: {copy:?}");

    // Fit stands clear of it, and so does the read-out.
    let stock = stock_rect(&h);
    assert!(
        stock.bottom() < drawer.top(),
        "the stock {stock:?} is clear of the drawer {drawer:?}"
    );
    assert!(
        stock.height() < fitted_before.height(),
        "it was fitted again"
    );
    let over = common::on_screen(&h, dc_geometry::Point::new(85.0, 60.0));
    h.event(egui::Event::PointerMoved(over));
    h.run();
    let readout = h.get_by_label_contains("X 85.0").rect();
    assert!(
        readout.bottom() < drawer.top(),
        "the read-out {readout:?} is above the drawer"
    );

    // The pointer on the drawer is not on the drawing under it.
    h.event(egui::Event::PointerMoved(
        drawer.center() + egui::vec2(0.0, 60.0),
    ));
    h.run();
    assert!(
        h.query_by_label_contains("X ").is_none(),
        "no cursor read-out under the drawer"
    );

    // The grab handle: 60 px up is 60 px taller.
    let handle = egui::pos2(drawer.center().x, drawer.top() + 6.0);
    common::drag(
        &mut h,
        handle,
        handle - egui::vec2(0.0, 60.0),
        egui::PointerButton::Primary,
    );
    let taller = h.state().layout().drawer.expect("still open");
    assert!(
        (taller.height() - 300.0).abs() < 1.0,
        "dragged 60 px taller: {taller:?}"
    );
    assert_eq!(h.state().layout().viewport, before);
    // And no taller than leaves the floating controls in view.
    let handle = egui::pos2(taller.center().x, taller.top() + 6.0);
    common::drag(
        &mut h,
        handle,
        handle - egui::vec2(0.0, 2000.0),
        egui::PointerButton::Primary,
    );
    let tallest = h.state().layout().drawer.expect("still open");
    let fit = h.get_by_role_and_label(Role::Button, "Fit").rect();
    assert!(
        tallest.top() > fit.bottom(),
        "the tools stay in view: {tallest:?}"
    );
    assert!(
        h.state().app().selection().is_empty(),
        "nothing was selected"
    );
    assert!(!h.state().app().is_dirty(), "and nothing edited");

    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    assert_eq!(h.state().layout().drawer, None);
    assert_eq!(h.state().layout().viewport, before);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 71, acceptance 94 (#549, row 11): *Regenerate all* is in
/// neither the View menu nor the Warnings drawer. After Stop the status
/// bar offers it, once; clicking it makes every enabled operation
/// pending again (one may already be on the worker, which takes the first
/// job in the frame that applies the click once the edit has settled).
#[test]
fn regenerate_all_only_after_stop() {
    let dir = scratch("regenerate-after-stop");
    let mut h = generated_plaque(&dir, true);
    h.get_by_label("View").click();
    h.run();
    assert!(
        h.query_by_label("Appearance").is_some(),
        "the View menu is open"
    );
    assert!(
        h.query_by_label("Regenerate all").is_none(),
        "not in the View menu"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.get_by_role_and_label(Role::Button, "Warnings").click();
    h.run();
    assert!(h.state().warnings_open(), "the drawer is open");
    assert!(h.query_by_label("Copy").is_some(), "the drawer is drawn");
    assert!(
        h.query_by_label("Regenerate all").is_none(),
        "not in the drawer"
    );

    // A thicker stock makes every operation stale; Stop before the change
    // settles.
    let mut stock = h.state().app().project().stock.clone();
    stock.thickness_mm = Some(12.0);
    h.state_mut()
        .app_mut()
        .run(Command::SetStock(stock))
        .unwrap();
    h.step();
    h.get_by_label("Stop").click();
    // The click is read in one frame and drawn in the next.
    h.step();
    h.step();
    assert!(h.query_by_label("Stopped").is_some(), "stopped");
    let offered: Vec<egui::Rect> = h
        .query_all_by_label("Regenerate all")
        .map(|n| n.rect())
        .collect();
    assert_eq!(offered.len(), 1, "one Regenerate all");
    assert!(
        offered[0].min.y >= 800.0 - STATUS_BAR_PX,
        "in the status bar"
    );

    h.get_by_label("Regenerate all").click();
    h.step();
    // Pending, or already handed to the worker when the edit had settled.
    let app = h.state().app();
    let pending: Vec<dc_project::OperationId> = app.pending().into_iter().map(|j| j.op).collect();
    let enabled: Vec<&dc_project::Operation> = app
        .project()
        .operations
        .iter()
        .filter(|o| o.enabled())
        .collect();
    assert_eq!(enabled.len(), 5);
    assert!(pending.len() >= enabled.len() - 1, "{pending:?}");
    for op in enabled {
        assert!(
            app.generation().is_regenerating(op.id()),
            "{} is pending again",
            op.name()
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50b, acceptance 94: the status bar's units button opens a
/// two-row menu; choosing inches is one undo step, labelled as Project
/// Settings' switch labels it, and the button then reads `in`.
#[test]
fn units_menu_switches_display_units() {
    let dir = scratch("status-units");
    let mut h = generated_plaque(&dir, true);
    let steps = undo_steps(&h);
    let button = h.get_by_role_and_label(Role::Button, "mm").rect();
    assert!(
        button.min.y >= 800.0 - STATUS_BAR_PX,
        "in the status bar: {button:?}"
    );
    h.get_by_role_and_label(Role::Button, "mm").click();
    h.run();
    h.get_by_label("Inches (in)").click();
    h.run();
    assert_eq!(
        h.state().app().project().display_units,
        dc_project::Units::Inch
    );
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert_eq!(h.state().app().undo_label(), Some("Set display units"));
    assert!(
        h.query_by_role_and_label(Role::Button, "in").is_some(),
        "the button reads in"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The names of the shell's project's instances, in file order.
fn instance_names(h: &Harness<'_, dreamcarve_app::Shell>) -> Vec<String> {
    h.state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.name.clone())
        .collect()
}

/// Slice 25 (#617; capability 57, acceptance 96): F2 on the selected
/// square, and *Rename* in its row's context menu, each turn its name into
/// a field in place, focused, with the whole name selected, so typing
/// replaces it. Typing and Enter is one undo step `Rename "square"`;
/// Escape leaves the file unchanged.
#[test]
fn f2_and_context_menu_rename_in_place() {
    let dir = scratch("tree-rename");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let start = h.state().app().project().clone();

    // F2: the field, focused, holding the name; typing replaces it.
    select_only(&mut h, "i4");
    h.key_press(egui::Key::F2);
    h.run();
    let field = h.get_by_label("Rename square");
    assert!(field.is_focused(), "the field has the keyboard");
    assert_eq!(field.value().as_deref(), Some("square"));
    field.type_text("Recess");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(
        instance_names(&h).contains(&"Recess".to_owned()),
        "the typed name replaced the selected one: {:?}",
        instance_names(&h)
    );
    assert_eq!(h.state().app().undo_label(), Some("Rename “square”"));
    assert_eq!(undo_steps(&h), 1, "the rename is one step");
    assert!(
        h.query_by_label("Rename square").is_none(),
        "the field went"
    );

    // Escape after F2: the file unchanged, no step.
    h.state_mut().app_mut().undo();
    h.run();
    assert_eq!(h.state().app().project(), &start);
    h.key_press(egui::Key::F2);
    h.run();
    h.get_by_label("Rename square").type_text("zzz");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().app().project(), &start, "Escape reverts");
    assert_eq!(undo_steps(&h), 0, "and is no step");
    assert!(h.query_by_label("Rename square").is_none());

    // The context menu's Rename, on a row not selected.
    h.get_by_label("border").click_secondary();
    h.run();
    h.get_by_label("Rename").click();
    h.run();
    let field = h.get_by_label("Rename border");
    assert!(field.is_focused(), "the field has the keyboard");
    field.type_text("Frame");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(
        instance_names(&h).contains(&"Frame".to_owned()),
        "{:?}",
        instance_names(&h)
    );
    assert_eq!(h.state().app().undo_label(), Some("Rename “border”"));
    assert_eq!(undo_steps(&h), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Slice 25 (#617; capabilities 54 and 58a): R, Shift R, Ctrl Shift H and
/// Ctrl Shift V each do what the Inspector's Arrange group does: the same
/// transform and the same undo label. R is the Rotate button, Shift R its
/// popover's angle 90, the flips their buttons. With nothing selected the
/// group is disabled.
#[test]
fn shortcuts_reach_the_arrange_commands() {
    let dir = scratch("arrange-shortcuts");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    // The Arrange group is in the header of the Design step's Inspector,
    // which comes with a selection.
    go_to(&mut h, "Design");
    for name in ["Rotate 90° clockwise", "Flip horizontal", "Flip vertical"] {
        assert!(
            h.query_by_label(name).is_none(),
            "{name} is not shown with nothing selected"
        );
    }
    select_only(&mut h, "i4");
    for name in ["Rotate 90° clockwise", "Flip horizontal", "Flip vertical"] {
        assert!(
            !h.get_by_label(name).accesskit_node().is_disabled(),
            "{name} is enabled with the square selected"
        );
    }
    let before = transform_of(&h, "i4");

    type Group = fn(&mut Harness<'_, dreamcarve_app::Shell>);
    let rotate: Group = |h| {
        h.get_by_label("Rotate 90° clockwise").click();
        h.run();
    };
    let popover_90: Group = |h| {
        h.get_by_label("Rotate by an angle").click();
        h.run();
        h.get_by_label("Angle").click();
        h.run();
        h.get_by_label("Angle").type_text("90");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
    };
    let flip_h: Group = |h| {
        h.get_by_label("Flip horizontal").click();
        h.run();
    };
    let flip_v: Group = |h| {
        h.get_by_label("Flip vertical").click();
        h.run();
    };
    for (modifiers, key, group, what) in [
        (egui::Modifiers::NONE, egui::Key::R, rotate, "R"),
        (egui::Modifiers::SHIFT, egui::Key::R, popover_90, "Shift R"),
        (
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::H,
            flip_h,
            "Ctrl Shift H",
        ),
        (
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::V,
            flip_v,
            "Ctrl Shift V",
        ),
    ] {
        let steps = undo_steps(&h);
        group(&mut h);
        assert_eq!(undo_steps(&h), steps + 1, "{what}: the group's one step");
        let by_group = transform_of(&h, "i4");
        let label = h.state().app().undo_label().map(str::to_owned);
        assert_ne!(by_group, before, "{what}: the group turned it");
        h.state_mut().app_mut().undo();
        h.run();
        assert_eq!(transform_of(&h, "i4"), before);

        h.key_press_modifiers(modifiers, key);
        h.run();
        assert_eq!(undo_steps(&h), steps + 1, "{what}: one step");
        assert_same_transform(transform_of(&h, "i4"), by_group, what);
        assert_eq!(
            h.state().app().undo_label().map(str::to_owned),
            label,
            "{what}: the group's command"
        );
        assert!(
            instance_of(&h, "i4").visible,
            "{what}: H's own command (hide) did not run"
        );
        h.state_mut().app_mut().undo();
        h.run();
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A window with no project open: the start screen.
fn empty_window(
    dir: &std::path::Path,
    dialogs: &QueuedDialogs,
) -> Harness<'static, dreamcarve_app::Shell> {
    harness(options(&dir.join("config"), None, dialogs), 1280.0, 800.0)
}

/// The button named exactly `label`.
fn button_named<'h>(
    h: &'h Harness<'_, dreamcarve_app::Shell>,
    label: &str,
) -> egui_kittest::Node<'h> {
    h.query_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button {label}"))
}

/// Slice 34 (#626), capability 79 and acceptance 103 (`redesign.md`,
/// "The start screen"): a project with nothing in it shows the start
/// screen in the Viewport's card: a heading and one sentence; *New from
/// drawing…*, *New from picture…* and *Open…* in a row; the three
/// examples as cards in a row, each with its line and a thumbnail of the
/// example's own shapes; and the recent files. The copy is short and says
/// nothing about phases or unsaved copies. The step panel shows the
/// Design step's empty sentence, and another step shows the stock.
#[test]
fn start_screen_offers_a_drawing_the_examples_and_recent_files() {
    let dir = scratch("empty-two-paths");
    let config = dir.join("config");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let file = GuiPrefs {
        recent: vec![plaque.clone(), dir.join("gone.dcv")],
        ..GuiPrefs::default()
    };
    gui_prefs::save(&Paths::in_dir(config), &file).unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = empty_window(&dir, &dialogs);
    let viewport = h.state().layout().viewport;

    let heading = h.get_by_label("Start with a drawing").rect();
    assert!(viewport.contains_rect(heading), "in the Viewport's card");
    assert!(
        h.query_by_label(
            "Bring in an SVG or DXF drawing to make toolpaths from, or open an example to look \
             round."
        )
        .is_some(),
        "the one sentence under it"
    );
    assert!(
        h.query_by_label("No shapes yet.").is_some(),
        "the Design step's empty sentence"
    );

    // The three ways in, in one row, left to right.
    let actions: Vec<egui::Rect> = ["New from drawing…", "New from picture…", "Open…"]
        .into_iter()
        .map(|label| button_named(&h, label).rect())
        .collect();
    for pair in actions.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{actions:?}");
        assert_eq!(pair[0].center().y, pair[1].center().y, "one row");
    }
    assert!(heading.bottom() <= actions[0].top());
    assert!(
        !button_named(&h, "New from picture…")
            .accesskit_node()
            .is_disabled(),
        "a picture starts a project too (`new_from_picture_through_the_sheet`)"
    );

    // The examples: three cards in a row under their heading.
    let examples = h.get_by_label("Examples").rect();
    assert!(actions[0].bottom() <= examples.top());
    let cards: Vec<egui::Rect> = ["Coaster", "Plaque", "Sign"]
        .into_iter()
        .map(|name| button_named(&h, name).rect())
        .collect();
    let lines = [
        "Profile with tabs",
        "Pockets and holes",
        "V-carved lettering",
    ];
    for (card, teaches) in cards.iter().zip(lines) {
        assert!(card.top() >= examples.bottom(), "{teaches}: under Examples");
        assert_eq!(card.top(), cards[0].top(), "one row");
        assert_eq!(card.size(), cards[0].size(), "three cards of one size");
        let line = h.get_by_label(teaches).rect();
        assert!(card.contains_rect(line), "{teaches} is on its card");
    }
    assert!(cards[0].right() < cards[1].left() && cards[1].right() < cards[2].left());

    // Each card's thumbnail is the example's own shapes.
    let thumbnails = h.state().example_thumbnails();
    assert_eq!(thumbnails.len(), 3, "three thumbnails");
    for (example, thumbnail) in thumbnails {
        assert!(
            !thumbnail.outlines.is_empty(),
            "{example:?}: the thumbnail draws the example's shapes"
        );
    }

    // The recent files that still exist, under the examples.
    let recent = h.get_by_label("Recent").rect();
    assert!(cards[0].bottom() <= recent.top());
    let row = button_named(&h, "plaque.dcv").rect();
    assert!(recent.bottom() <= row.top());
    assert!(
        h.query_by_label("gone.dcv").is_none(),
        "a file that is gone is not listed"
    );

    for word in ["phase", "Phase", "unsaved", "Unsaved"] {
        let found: Vec<String> = h
            .query_all_by_label_contains(word)
            .map(|n| n.accesskit_node().label().unwrap_or_default())
            .collect();
        assert!(found.is_empty(), "no label says {word}: {found:?}");
    }

    // Another step shows the stock in its place; Design brings it back.
    go_to(&mut h, "Stock");
    assert!(h.query_by_label("Start with a drawing").is_none());
    go_to(&mut h, "Design");
    assert!(h.query_by_label("Start with a drawing").is_some());

    // A recent file opens.
    button_named(&h, "plaque.dcv").click();
    h.run();
    assert_eq!(h.state().app().source(), Some(plaque.as_path()));
    assert!(h.query_by_label("Start with a drawing").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// Slice 34 (#626), capability 79: an example opens as an untitled copy,
/// so the title is `Untitled — DreamCarve` and Save asks for a path.
#[test]
fn example_opens_untitled() {
    let dir = scratch("empty-example-untitled");
    for example in ["Coaster", "Plaque", "Sign"] {
        let dialogs = QueuedDialogs::default();
        let mut h = empty_window(&dir, &dialogs);
        button_named(&h, example).click();
        h.run();
        assert!(
            !h.state().app().project().instances.is_empty(),
            "{example} opened"
        );
        assert!(h.state().app().source().is_none(), "{example} has no file");
        assert_eq!(h.state().title(), "Untitled — DreamCarve", "{example}");
        assert!(
            h.query_by_label("Start with a drawing").is_none(),
            "{example}: the start screen goes when shapes arrive"
        );

        command_key(&h, egui::Key::S);
        h.run();
        let asked = dialogs.asked();
        assert_eq!(asked.len(), 1, "{example}: Save asks once: {asked:?}");
        assert!(
            asked[0].starts_with("save_project "),
            "{example}: Save asks where, got {asked:?}"
        );
        assert!(h.state().app().source().is_none(), "cancelled: no file");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The labels of the open `title` menu's rows, top to bottom: its
/// buttons, checkboxes and submenus. A row's label ends with its
/// shortcut's text, when it has one.
fn menu_rows(h: &Harness<'_, dreamcarve_app::Shell>, title: &str) -> Vec<String> {
    let button = h
        .query_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no {title} menu"));
    button
        .query_all_by_role(Role::Menu)
        .next()
        .unwrap_or_else(|| panic!("the {title} menu is open"))
        .query_all(
            egui_kittest::kittest::by()
                .predicate(|node| matches!(node.role(), Role::Button | Role::CheckBox)),
        )
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect()
}

/// The open `title` menu's rows are `expected`, in order, each followed
/// by nothing or its shortcut.
fn assert_menu(h: &Harness<'_, dreamcarve_app::Shell>, title: &str, expected: &[&str]) {
    let rows = menu_rows(h, title);
    let same = rows.len() == expected.len()
        && rows
            .iter()
            .zip(expected)
            .all(|(row, want)| row == want || row.starts_with(&format!("{want} ")));
    assert!(same, "{title} menu: {rows:?}, want {expected:?}");
}

/// Opens the menu bar's `title` menu.
fn open_menu(h: &mut Harness<'_, dreamcarve_app::Shell>, title: &str) {
    h.get_by_label(title).click();
    h.run();
}

/// Capability 79's last sentence: "Help ▸, and the rail's Help, open the
/// book's quick start for the app, the Keyboard chapter, and About". The
/// Help menu's rows are *Quick Start*, *Keyboard Shortcuts*, *User Guide*
/// and *About DreamCarve*, in the header's menu and under the rail's Help
/// alike; the first three ask the operating system to open the book's
/// page (`quick-start/app.html`, `user-guide/keyboard.html`, its front
/// page), and About says the version in the status bar.
#[test]
fn help_opens_the_quick_start_the_keyboard_chapter_and_about() {
    let dir = scratch("help-menu");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    let book = "https://grayzu.github.io/dreamcarve/";
    let rows = [
        ("Quick Start", format!("{book}quick-start/app.html")),
        (
            "Keyboard Shortcuts",
            format!("{book}user-guide/keyboard.html"),
        ),
        ("User Guide", book.to_owned()),
    ];
    for opener in ["Help", "Help and About"] {
        for (row, url) in &rows {
            h.get_by_label(opener).click();
            h.run();
            if opener == "Help" {
                assert_menu(
                    &h,
                    "Help",
                    &[
                        "Quick Start",
                        "Keyboard Shortcuts",
                        "User Guide",
                        "About DreamCarve",
                    ],
                );
            }
            h.get_by_label(row).click();
            h.step();
            let opened: Vec<String> = h
                .output()
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(opened, std::slice::from_ref(url), "{opener} ▸ {row}");
            h.run();
        }
        h.get_by_label(opener).click();
        h.run();
        h.get_by_label("About DreamCarve").click();
        h.run();
        assert_eq!(
            h.state().message(),
            Some(concat!("DreamCarve ", env!("CARGO_PKG_VERSION"))),
            "{opener} ▸ About says the version"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Slice 35 (#627), capability 79a and acceptance 103: the File menu's
/// rows are 79a's list in its order, new-project and add-to-project
/// apart (*Save Setup Sheet…* joins them only after an export). The View
/// and Project menus are 79a's too.
#[test]
fn file_menu_labels_in_order() {
    let dir = scratch("file-menu");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );

    open_menu(&mut h, "File");
    assert_menu(
        &h,
        "File",
        &[
            "New",
            "New Project from Drawing…",
            "New Project from Picture…",
            "Open…",
            "Open Recent",
            "Save",
            "Save As…",
            "Add Drawing to Project…",
            "Add Picture to Project…",
            "Export G-code…",
            "Quit",
        ],
    );
    h.key_press(egui::Key::Escape);
    h.run();

    open_menu(&mut h, "View");
    assert_menu(
        &h,
        "View",
        &[
            "Step Panel",
            "Go to Design",
            "Go to Stock",
            "Go to Tools",
            "Go to Operations",
            "Go to Export",
            "Toolpaths…",
            "Warnings",
            "Fit View",
            "Zoom to Selection",
            "Appearance",
        ],
    );
    h.key_press(egui::Key::Escape);
    h.run();

    open_menu(&mut h, "Project");
    assert_menu(
        &h,
        "Project",
        &["Project Settings…", "Tool Library…", "Fit Stock to Shapes…"],
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Slice 35 (#627), capability 79a: View ▸ Appearance holds three checked
/// choices; Dark is checked by default (`redesign.md`, principle 5) and
/// shows over a light OS; choosing Light checks it alone, sets
/// `Prefs::theme`, keeps it in `gui.toml` and applies the light visuals
/// over a dark OS.
#[test]
fn appearance_has_three_checked_choices() {
    use egui::accesskit::Toggled;

    let dir = scratch("appearance");
    let config = dir.join("config");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, None, &dialogs), 1280.0, 800.0);
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();

    let checked = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<(String, bool)> {
        ["System", "Light", "Dark"]
            .into_iter()
            .map(|label| {
                let node = h.get_by_label(label);
                assert_ne!(
                    node.accesskit_node().role(),
                    Role::CheckBox,
                    "{label}: a choice of three is not a checkbox"
                );
                let toggled = node.accesskit_node().toggled();
                assert!(toggled.is_some(), "{label} is a checked choice");
                (label.to_owned(), toggled == Some(Toggled::True))
            })
            .collect()
    };
    let open_appearance = |h: &mut Harness<'_, dreamcarve_app::Shell>| {
        open_menu(h, "View");
        h.get_by_label("Appearance").hover();
        h.run();
    };

    open_appearance(&mut h);
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Dark);
    assert_eq!(
        h.ctx.theme(),
        egui::Theme::Dark,
        "Dark is the default, over a light OS"
    );
    assert_eq!(
        checked(&h),
        [
            ("System".to_owned(), false),
            ("Light".to_owned(), false),
            ("Dark".to_owned(), true)
        ]
    );

    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.get_by_label("Light").click();
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Light);
    assert_eq!(
        h.ctx.theme(),
        egui::Theme::Light,
        "Light wins over a dark OS"
    );
    assert!(!h.ctx.global_style().visuals.dark_mode);
    assert_eq!(
        h.ctx.global_style().visuals.panel_fill,
        theme::LIGHT.surface_panel
    );
    let saved = gui_prefs::load(&Paths::in_dir(config.clone())).prefs;
    assert_eq!(saved.theme, gui_prefs::Theme::Light);

    open_appearance(&mut h);
    assert_eq!(
        checked(&h),
        [
            ("System".to_owned(), false),
            ("Light".to_owned(), true),
            ("Dark".to_owned(), false)
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The labels of the unsaved-changes sheet's buttons, sorted.
fn sheet_buttons(h: &Harness<'_, dreamcarve_app::Shell>) -> Vec<String> {
    let mut labels: Vec<String> = h
        .get_by_role(Role::Dialog)
        .query_all_by_role(Role::Button)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect();
    labels.sort();
    labels
}

/// What asks the unsaved-changes sheet, in a test.
#[derive(Debug, Clone, Copy)]
enum Ask {
    Close,
    Quit,
    Open,
    New,
    NewFromDrawing,
    NewFromPicture,
}

impl Ask {
    const ALL: [Self; 6] = [
        Self::Close,
        Self::Quit,
        Self::Open,
        Self::New,
        Self::NewFromDrawing,
        Self::NewFromPicture,
    ];

    fn press(self, h: &mut Harness<'_, dreamcarve_app::Shell>) {
        match self {
            Self::Close => press_close(h),
            Self::Quit => command_key(h, egui::Key::Q),
            Self::Open => command_key(h, egui::Key::O),
            Self::New => command_key(h, egui::Key::N),
            Self::NewFromDrawing => {
                open_menu(h, "File");
                h.get_by_label("New Project from Drawing…").click();
            }
            Self::NewFromPicture => {
                open_menu(h, "File");
                h.get_by_label("New Project from Picture…").click();
            }
        }
        h.run();
    }
}

/// Slice 35 (#627), capability 51 and acceptance 88: on close, quit,
/// *Open* and each *New* (*New*, *New Project from Drawing…* and *New
/// Project from Picture…*) with unsaved changes,
/// the sheet's buttons are exactly *Save*, *Don't Save* and *Cancel*;
/// *Cancel* keeps the project and its changes; *Don't Save* goes ahead
/// without writing the file.
#[test]
fn unsaved_sheet_is_save_dont_save_cancel() {
    let dir = scratch("unsaved-three");
    let config = dir.join("config");
    let sign = copy_fixture("sign.dcv", &dir);
    for ask in Ask::ALL {
        let plaque = copy_fixture("plaque.dcv", &dir);
        let original = std::fs::read(&plaque).unwrap();
        let dialogs = QueuedDialogs::default();
        let mut h = harness(
            options(&config, Some(plaque.clone()), &dialogs),
            1280.0,
            800.0,
        );
        widen_stock(&mut h);
        let edited = h.state().app().project().clone();

        ask.press(&mut h);
        assert_eq!(
            sheet_buttons(&h),
            ["Cancel", "Don't Save", "Save"],
            "{ask:?}: the sheet's buttons"
        );
        click_in_sheet(&mut h, "Cancel");
        assert!(h.query_by_role(Role::Dialog).is_none(), "{ask:?}: Cancel");
        assert_eq!(h.state().app().project(), &edited, "{ask:?}: kept");
        assert!(h.state().app().is_dirty());
        assert!(!h.state().close_allowed());
        assert!(dialogs.asked().is_empty(), "{ask:?}: Cancel asks nothing");

        if matches!(ask, Ask::Open) {
            dialogs.answer(&sign);
        }
        ask.press(&mut h);
        click_in_sheet(&mut h, "Don't Save");
        assert_eq!(
            std::fs::read(&plaque).unwrap(),
            original,
            "{ask:?}: Don't Save writes nothing"
        );
        let app = h.state().app();
        match ask {
            Ask::Close | Ask::Quit => assert!(h.state().close_allowed(), "{ask:?}"),
            Ask::Open => {
                assert_eq!(dialogs.asked(), ["open_project"]);
                assert_eq!(app.source(), Some(sign.as_path()));
            }
            Ask::New => {
                assert_eq!(app.source(), None);
                assert!(app.project().instances.is_empty(), "a new project");
            }
            Ask::NewFromDrawing => assert_eq!(dialogs.asked(), ["open_drawing"]),
            Ask::NewFromPicture => assert_eq!(dialogs.asked(), ["open_picture"]),
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Slice 35 (#627), capability 51: *Save* on the sheet of an untitled
/// project asks where first; cancelling that dialog cancels the whole
/// action, so the window stays, nothing opens and the changes are kept;
/// answering it writes the file and then goes ahead.
#[test]
fn save_on_untitled_asks_where_and_cancel_cancels_all() {
    let dir = scratch("unsaved-untitled");
    let config = dir.join("config");
    for ask in Ask::ALL {
        let dialogs = QueuedDialogs::default();
        let mut h = harness(options(&config, None, &dialogs), 1280.0, 800.0);
        widen_stock(&mut h);
        let edited = h.state().app().project().clone();

        ask.press(&mut h);
        click_in_sheet(&mut h, "Save");
        let asked = dialogs.asked();
        assert_eq!(asked.len(), 1, "{ask:?}: one dialog: {asked:?}");
        assert!(
            asked[0].starts_with("save_project "),
            "{ask:?}: Save asks where first: {asked:?}"
        );
        assert!(h.query_by_role(Role::Dialog).is_none());
        assert!(!h.state().close_allowed(), "{ask:?}: the window stays");
        assert_eq!(h.state().app().project(), &edited, "{ask:?}: kept");
        assert!(h.state().app().is_dirty(), "{ask:?}: still unsaved");
        assert_eq!(h.state().app().source(), None);

        // Answered, Save writes there and then goes ahead.
        let path = dir.join(format!("{ask:?}.dcv"));
        dialogs.answer(&path);
        let unsaved = h.state().app().serialize().expect("the project saves");
        ask.press(&mut h);
        click_in_sheet(&mut h, "Save");
        let written = std::fs::read_to_string(&path).expect("the file is written");
        assert!(
            written == unsaved,
            "{ask:?}: Save writes the changed project before going ahead"
        );
        let last = dialogs.asked().last().cloned().unwrap_or_default();
        match ask {
            Ask::Close | Ask::Quit => assert!(h.state().close_allowed(), "{ask:?}"),
            Ask::Open => assert_eq!(last, "open_project"),
            Ask::New => assert!(h.state().app().source().is_none(), "a new project"),
            Ask::NewFromDrawing => assert_eq!(last, "open_drawing"),
            Ask::NewFromPicture => assert_eq!(last, "open_picture"),
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

// Slice D6, area C (#685; capabilities 63, 66b, 68, 68a): the tapered ball
// nose in the window.

/// Types `text` into the Inspector's field `label` and leaves it with
/// Enter.
fn type_in_field(h: &mut Harness<'_, dreamcarve_app::Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

/// A 90° V-bit `id` named `name`, with the rates of the project's second
/// tool and no number.
fn v_bit_90(h: &Harness<'_, dreamcarve_app::Shell>, id: &str, name: &str) -> dc_project::Tool {
    dc_project::Tool {
        id: dc_project::ToolId::new(id),
        name: name.into(),
        number: None,
        cutter: dc_project::ToolKind::VBit {
            included_angle_deg: 90.0,
        },
        ..h.state().app().project().tools[1].clone()
    }
}

/// Capability 63 (slice D9), in the window: a warning that names a tool
/// marks the tool's row in the Tools step with the mark an operation's
/// row has, a warning glyph and the count, and hovering it lists the
/// message. The plaque's 1/4 end mill, given a plunge rate above its
/// feed, gets *1 warning*; the row used to show nothing, since only a
/// missing value marked a tool. The 1/8 end mill's row stays clean.
#[test]
fn a_warning_about_a_tool_marks_its_row() {
    let dir = scratch("tool-row-warning");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    go_to(&mut h, "Tools");
    let marks = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<egui::Rect> {
        let panel = h.state().layout().step_panel.expect("the step panel shows");
        h.query_all_by_label("1 warning")
            .map(|node| node.rect())
            .filter(|rect| panel.contains_rect(*rect))
            .collect()
    };
    assert!(marks(&h).is_empty(), "the plaque's tools have no warning");

    let project = h.state().app().project().clone();
    let mut fast = project.tools[1].clone();
    assert_eq!(fast.name, "1/4 end mill");
    fast.plunge_mm_min = Some(5000.0);
    h.state_mut()
        .app_mut()
        .run(Command::UpdateTool(fast))
        .unwrap();
    h.run();
    let marked = marks(&h);
    assert_eq!(marked.len(), 1, "one row is marked");
    let row = h
        .get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .rect();
    assert!(
        row.contains_rect(marked[0]),
        "the mark is on the 1/4 end mill's row: {:?} in {row:?}",
        marked[0]
    );
    let panel = h.state().layout().step_panel.unwrap();
    h.query_all_by_label("1 warning")
        .find(|node| panel.contains_rect(node.rect()))
        .unwrap()
        .hover();
    wait_for_tooltip(&mut h);
    assert!(
        h.query_by_label_contains("plunges at 5000 mm/min")
            .is_some(),
        "hovering it lists the message"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// How many rows of the Tools list show the red dot, as assistive tools
/// read it.
fn rows_lacking_a_value(h: &Harness<'_, dreamcarve_app::Shell>) -> usize {
    h.query_all_by_label("Has a value still to set").count()
}

/// Capability 63 and 66b (#685): choosing *Tapered ball nose* in the type
/// dropdown, the sixth type, writes the cutter with none of its three
/// values set, one undo step, and the editor draws *Tip diameter*, *Taper
/// per side* and *Flute length* between the type and *Diameter*, each
/// with the required-field look and the file's message as its tooltip.
/// The row keeps its red dot until the last is typed; each value typed is
/// one more step, nothing is invented, and the row's second line then
/// names the tip and the taper per side.
#[test]
fn tapered_ball_nose_fields_are_required_until_typed() {
    let dir = scratch("tapered-fields");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.1)
        .build_eframe(|cc| {
            dreamcarve_app::Shell::new(cc, options(&dir.join("config"), Some(plaque), &dialogs))
                .expect("the shell starts")
        });
    h.run();
    go_to(&mut h, "Tools");
    let own = ["Tip diameter", "Taper per side", "Flute length"];
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    for label in own {
        assert!(
            h.query_by_role_and_label(Role::TextInput, label).is_none(),
            "an end mill has no {label}"
        );
    }
    let lacking = rows_lacking_a_value(&h);
    let steps = undo_steps(&h);

    // The dropdown lists the six types; the last is the tapered ball nose.
    h.get_by_label("Type").click();
    h.run();
    for kind in [
        "End mill",
        "Ball nose",
        "V-bit",
        "Engraver",
        "Drill",
        "Tapered ball nose",
    ] {
        assert!(h.query_all_by_label(kind).next().is_some(), "{kind}");
    }
    h.get_by_label("Tapered ball nose").click();
    h.run();
    let tool = |h: &Harness<'_, dreamcarve_app::Shell>| {
        h.state()
            .app()
            .project()
            .tools
            .iter()
            .find(|t| t.name == "1/4 end mill")
            .expect("the tool")
            .clone()
    };
    assert_eq!(
        tool(&h).cutter,
        dc_project::ToolKind::TaperedBallNose {
            tip_diameter_mm: None,
            taper_angle_deg: None,
            flute_length_mm: None,
        },
        "nothing is invented"
    );
    assert_eq!(undo_steps(&h), steps + 1, "the switch is one step");
    assert_eq!(
        rows_lacking_a_value(&h),
        lacking + 1,
        "the row takes the red dot"
    );

    // The three fields, in order, between the type and the diameter.
    let top = |h: &Harness<'_, dreamcarve_app::Shell>, label: &str| {
        h.get_by_role_and_label(Role::TextInput, label).rect().min.y
    };
    let order = ["Name", own[0], own[1], own[2], "Diameter", "Flutes", "Feed"];
    for pair in order.windows(2) {
        assert!(
            top(&h, pair[0]) < top(&h, pair[1]),
            "{} is above {}",
            pair[0],
            pair[1]
        );
    }
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Angle")
            .is_none(),
        "the included angle is a V-bit's"
    );
    // Each is required, with the file's message as its tooltip.
    for (label, field) in own.into_iter().zip([
        dc_app::tools::ToolField::TipDiameter,
        dc_app::tools::ToolField::Taper,
        dc_app::tools::ToolField::FluteLength,
    ]) {
        let message = dc_app::tools::required(&tool(&h), field).expect("it is required");
        assert!(
            message.contains(field.file_name()),
            "the file names it: {message}"
        );
        assert_required(&mut h, label, &message);
    }
    assert_eq!(
        h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
            .value()
            .as_deref(),
        Some("6.35 mm · T1"),
        "an unset value is the row's red dot, not a made-up figure"
    );

    // Typed, one step each: the marks go, and the row names what it is.
    for (n, (label, text)) in own.into_iter().zip(["0.762", "6.7", "19.05"]).enumerate() {
        assert_eq!(rows_lacking_a_value(&h), lacking + 1, "before {label}");
        type_in_field(&mut h, label, text);
        assert_eq!(undo_steps(&h), steps + 2 + n, "{label} is one step");
        let field = h.get_by_role_and_label(Role::TextInput, label);
        assert_eq!(field.value().as_deref(), Some(text), "{label}");
        assert_ne!(
            field.accesskit_node().placeholder(),
            Some("Required"),
            "{label} is no longer required"
        );
    }
    assert_eq!(
        tool(&h).cutter,
        dc_project::ToolKind::TaperedBallNose {
            tip_diameter_mm: Some(0.762),
            taper_angle_deg: Some(6.7),
            flute_length_mm: Some(19.05),
        }
    );
    assert_eq!(rows_lacking_a_value(&h), lacking, "the red dot goes");
    assert_eq!(
        h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
            .value()
            .as_deref(),
        Some("6.35 mm · tip 0.762 mm · 6.7°/side · T1"),
        "the cutting diameter, the tip, the taper per side, the number"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 63 (#685): the Tools list's second line is
/// `tools::row_detail`'s. On the tapered ball nose test part a tapered
/// ball nose's row names its cutting diameter, its tip and its angle per
/// side; an end mill's its diameter alone; and a V-bit's included angle
/// stands unlabelled by *side*, so the two cannot be mixed up.
#[test]
fn tools_list_names_the_tip_and_the_angle_per_side() {
    let dir = scratch("tapered-rows");
    let part = copy_fixture("dc-tbn.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(part), &dialogs),
        1280.0,
        800.0,
    );
    let v_bit = v_bit_90(&h, "t9", "90 V");
    h.state_mut()
        .app_mut()
        .run(Command::AddTools(vec![v_bit]))
        .unwrap();
    go_to(&mut h, "Tools");
    let detail = |name: &str| {
        h.get_by_role_and_label(Role::ListItem, name)
            .value()
            .unwrap_or_default()
    };
    assert_eq!(
        detail("0.762 mm tip tapered ball nose"),
        "6.35 mm · tip 0.762 mm · 6.7°/side · T2"
    );
    assert_eq!(
        detail("2.286 mm tip tapered ball nose"),
        "6.35 mm · tip 2.286 mm · 6.7°/side · T1"
    );
    assert_eq!(detail("1/8 end mill"), "3.175 mm · T3");
    assert_eq!(detail("90 V"), "6.35 mm · 90°");
    // Every line is the model's.
    let app = h.state().app();
    for tool in &app.project().tools {
        assert_eq!(
            detail(&tool.name),
            dc_app::tools::row_detail(app.strings(), tool, dc_project::Units::Mm),
            "{}",
            tool.name
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Capabilities 68 and 68a (#685): cut with a tapered ball nose, a
/// V-Carve's Tool row shows the bit's cutting height under its picker and
/// an Inlay's says one bit cuts both boards; the Inlay's derived lines
/// under Depth compare the flank's engagement with that cutting height
/// and end with the seat line, `check`'s figures, in millimetres. The
/// lines are the model's (`editor::notes`, `tool_note`, `seat_line`).
#[test]
fn carve_editors_show_the_cutting_height_and_the_seat_line() {
    let dir = scratch("tapered-notes");
    let part = copy_fixture("dc-tbn.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(part), &dialogs),
        1280.0,
        900.0,
    );
    let op = |h: &Harness<'_, dreamcarve_app::Shell>, id: &str| operation_of(h, id);
    let tool_of = |h: &Harness<'_, dreamcarve_app::Shell>, id: &str| {
        let app = h.state().app();
        let tool = op(h, id).tool().clone();
        let tool = app.project().tools.iter().find(|t| t.id == tool).unwrap();
        dc_app::editor::tool_note(app.project(), app.strings(), tool).expect("a cutting height")
    };

    // The V-Carve, cut with the 2.286 mm tip: its cone reaches the
    // cutting diameter below the flutes' top.
    open_operation(&mut h, "DC carve");
    let carve_note = tool_of(&h, "op1");
    assert!(
        carve_note.starts_with("cutting height 18.3") && carve_note.ends_with(" mm"),
        "{carve_note}"
    );
    assert!(h.query_by_label(&carve_note).is_some(), "{carve_note}");

    // The Inlay, cut with the 0.762 mm tip, whose flutes end first.
    open_operation(&mut h, "DC inlay");
    let inlay_note = tool_of(&h, "op2");
    assert_eq!(inlay_note, "cutting height 19.05 mm");
    assert!(
        h.query_by_label("one bit for both boards · cutting height 19.05 mm")
            .is_some(),
        "under the Inlay's Tool picker"
    );
    assert!(
        h.query_by_label_contains("seat: moves").is_none(),
        "the derived lines are Depth's, closed until opened"
    );
    open_group(&mut h, "Depth");
    let app = h.state().app();
    let seat = dc_app::editor::seat_line(app.project(), app.strings(), &op(&h, "op2"))
        .expect("an Inlay cut with a tapered ball nose has a seat line");
    assert!(
        seat.starts_with("seat: moves ")
            && seat.contains(" mm per 0.5° of taper, ")
            && seat.ends_with(" mm per 0.05 mm of tip radius; test-fit before gluing"),
        "{seat}"
    );
    assert!(h.query_by_label(&seat).is_some(), "{seat}");
    assert!(
        h.query_by_label_contains("of the tapered ball nose's 19.05 mm cutting height")
            .is_some(),
        "the engagement is compared with the cutting height"
    );
    // The seat line is the third of the derived lines.
    let engagement = h
        .get_by_label_contains("of the tapered ball nose's 19.05 mm cutting height")
        .rect();
    let seat_at = h.get_by_label(&seat).rect();
    assert!(engagement.min.y < seat_at.min.y, "the seat line is last");
    assert!(
        seat_at.max.x <= h.state().layout().inspector.unwrap().max.x,
        "the seat line wraps inside the Inspector"
    );
    // A V-bit has no seat line.
    let v_bit = v_bit_90(&h, "t9", "90 V");
    h.state_mut()
        .app_mut()
        .run(Command::AddTools(vec![v_bit]))
        .unwrap();
    let dc_project::Operation::Inlay(mut inlay) = op(&h, "op2") else {
        panic!("op2 is an Inlay");
    };
    inlay.tool = dc_project::ToolId::new("t9");
    h.state_mut()
        .app_mut()
        .run(Command::UpdateOperation(dc_project::Operation::Inlay(
            inlay,
        )))
        .unwrap();
    h.run();
    assert!(
        h.query_by_label_contains("seat: moves").is_none(),
        "a V-bit's Inlay has no seat line"
    );
    assert!(
        h.query_by_label_contains("one bit for both boards · cone height")
            .is_some(),
        "a V-bit shows its cone height"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The open item of #685, in the window: on a project whose only carving
/// bit is a tapered ball nose, *Add ▸ V-Carve* makes a V-Carve that cuts
/// with it (the first tool its picker offers), not one with no tool.
#[test]
fn a_new_carve_takes_the_tapered_ball_nose() {
    let dir = scratch("tapered-new-carve");
    let part = copy_fixture("dc-tbn.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(part), &dialogs),
        1280.0,
        800.0,
    );
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![
            dc_project::InstanceId::new("i1"),
        ]));
    go_to(&mut h, "Operations");
    let before = h.state().app().project().operations.len();
    h.get_by_role_and_label(Role::Button, "Add").click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "V-Carve").click();
    h.run();
    let project = h.state().app().project();
    assert_eq!(project.operations.len(), before + 1);
    let added = project.operations.last().unwrap();
    assert!(matches!(added, dc_project::Operation::VCarve(_)));
    assert_eq!(
        added.tool().as_str(),
        "t2",
        "the first tapered ball nose, as its picker offers"
    );
    assert!(
        !h.state()
            .app()
            .warnings()
            .iter()
            .any(|f| f.code.as_deref() == Some("dangling_tool")),
        "the new carve names a tool"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// One stop of a Tab walk: the focused control as assistive tools see it,
/// and the ring drawn for it.
#[derive(Debug)]
struct Stop {
    role: Role,
    label: String,
    node: egui::Rect,
    ring: dreamcarve_app::FocusRing,
}

/// Tabs once round the window, from nothing focused until the walk is
/// back in the header, and returns every stop.
fn tab_round(h: &mut Harness<'_, dreamcarve_app::Shell>) -> Vec<Stop> {
    use dreamcarve_app::FocusRegion as R;

    let mut stops = Vec::new();
    let mut left_header = false;
    for _ in 0..400 {
        h.key_press(egui::Key::Tab);
        h.run();
        let Some(region) = h.state().focus() else {
            continue;
        };
        if region != R::Header {
            left_header = true;
        } else if left_header {
            break;
        }
        let Some(node) = h
            .query_all(egui_kittest::kittest::by().predicate(|node| node.is_focused()))
            .next()
        else {
            continue;
        };
        let ring = h.state().focus_ring().expect("Tab's focus shows the ring");
        stops.push(Stop {
            role: node.accesskit_node().role(),
            label: node.accesskit_node().label().unwrap_or_default(),
            node: node.rect(),
            ring,
        });
    }
    stops
}

/// Row D5 of the worklog, defect 3 (`experience.md`, section 7, "focus"):
/// the ring sits round the control it names, on every kind of control.
/// It was drawn round the focused widget's own rectangle, which for a
/// field is the line of text inside its frame, and for a rail button or a
/// list row the slot that takes its clicks; and a custom control drew a
/// second ring of its own.
///
/// Tab walks each step of the plaque with something in the Inspector. At
/// every stop the ring's control is the shape the user sees: a field's
/// 32-px frame round its text; a rail step's 56 × 52 button; a list or
/// tree row's surface inside its slot; a pill's own radius; and for every
/// other control the rectangle assistive tools are given for it.
#[test]
fn focus_ring_goes_round_the_control_it_names() {
    let dir = scratch("focus-ring-shape");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut wrong: Vec<String> = Vec::new();
    for step in ["Design", "Stock", "Tools", "Operations", "Export"] {
        go_to(&mut h, step);
        match step {
            "Design" => h.get_by_role_and_label(Role::TreeItem, "square").click(),
            "Tools" => h
                .get_by_role_and_label(Role::ListItem, "1/4 end mill")
                .click(),
            "Operations" => {
                open_operation(&mut h, "Cut out");
                // Every group open, so every kind of row is walked.
                for group in ["Depth", "Strategy", "Entry and leads", "Corners and tabs"] {
                    let header = h
                        .query_all_by_role_and_label(Role::Button, group)
                        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
                        .expect("the group's header");
                    header.click();
                    h.run();
                }
            }
            _ => {}
        }
        h.run();
        h.event(egui::Event::PointerGone);
        h.run();
        // Nothing focused, so the walk starts at the header.
        h.key_press(egui::Key::Escape);
        h.run();
        for stop in tab_round(&mut h) {
            let Stop {
                role,
                label,
                node,
                ring,
            } = &stop;
            let control = ring.control;
            // Every stop is a control with a name: no pill, band or
            // read-out takes the focus.
            if label.is_empty() || matches!(role, Role::Label | Role::Unknown) {
                wrong.push(format!(
                    "{step}: {role:?} {label:?} at {node:?} is a Tab stop"
                ));
            }
            let kind = format!("{role:?}");
            *seen.entry(kind).or_default() += 1;
            let close = |a: f32, b: f32| (a - b).abs() <= 0.6;
            let same = |a: egui::Rect, b: egui::Rect| {
                close(a.left(), b.left())
                    && close(a.right(), b.right())
                    && close(a.top(), b.top())
                    && close(a.bottom(), b.bottom())
            };
            let ok = match role {
                // The frame round the text: a control tall, and wider
                // than the text by the field's padding at least.
                Role::TextInput => {
                    close(control.height(), 32.0)
                        && control.expand(0.6).contains_rect(*node)
                        && control.left() <= node.left() - 8.0
                }
                // A rail step: the 56 × 52 button in its slot.
                Role::Tab => {
                    close(control.width(), 56.0)
                        && close(control.height(), 52.0)
                        && close(ring.radius_px, 10.0)
                }
                // A row: its surface, 8 px in from its card's edges (the
                // step panel's card starts 80 px from the window's left
                // and is 312 px wide).
                Role::ListItem | Role::TreeItem => {
                    node.expand(0.6).contains_rect(control)
                        && close(control.left(), 88.0)
                        && close(control.right(), 384.0)
                        && close(ring.radius_px, 8.0)
                }
                // A box with no label: the 16-px box, not its hit area.
                Role::CheckBox if node.width() <= 24.6 => {
                    close(control.width(), 16.0)
                        && close(control.height(), 16.0)
                        && close(control.center().x, node.center().x)
                        && close(control.center().y, node.center().y)
                        && close(ring.radius_px, 4.0)
                }
                // The Viewport is a card.
                Role::Canvas => same(control, *node) && close(ring.radius_px, 14.0),
                // A switch: the 36 × 20 pill in the row that takes its
                // clicks.
                Role::Switch => {
                    close(control.width(), 36.0)
                        && close(control.height(), 20.0)
                        && node.expand(0.6).contains_rect(control)
                        && close(ring.radius_px, 10.0)
                }
                _ => same(control, *node),
            };
            // A ring's corner is never rounder than a pill's.
            let radius_fits = ring.radius_px <= control.height() / 2.0 + 0.6;
            if !ok || !radius_fits {
                wrong.push(format!(
                    "{step}: {role:?} {label:?} is {node:?}, ringed round {control:?} r {}",
                    ring.radius_px
                ));
            }
            // `experience.md`, section 11, "Hit sizes": nothing a pointer
            // aims at is under 24 × 24 px (a panel's resize handle is its
            // 8-px gap, with slop either side).
            let small = node.width() < 23.4 || node.height() < 23.4;
            if small && !matches!(role, Role::Splitter) {
                wrong.push(format!(
                    "{step}: {role:?} {label:?} is {:?}, under 24 × 24",
                    node.size()
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "these rings are not round their controls:\n{}",
        wrong.join("\n")
    );
    for kind in [
        "TextInput",
        "Tab",
        "ListItem",
        "TreeItem",
        "Button",
        "ComboBox",
        "CheckBox",
    ] {
        assert!(seen.contains_key(kind), "a {kind} was walked: {seen:?}");
    }
    assert_eq!(dreamcarve_app::RING_PX, 2.0);
    assert_eq!(dreamcarve_app::RING_OFFSET_PX, 2.0);
    std::fs::remove_dir_all(&dir).unwrap();
}
