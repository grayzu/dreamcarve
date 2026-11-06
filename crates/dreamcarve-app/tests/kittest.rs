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
    QueuedDialogs, command_key, copy_fixture, harness, options, press_close, saved_form, scratch,
    shift_command_key, wgpu_adapter_available, window_commands,
};
use dc_app::Command;
use dreamcarve_app::{PanelShown, theme};
use dreamcarve_shell::{GuiPrefs, Paths, gui_prefs};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, wgpu::WgpuTestRenderer};

/// Acceptance 92 (three platforms), until slice 18 brings export: the app
/// launches with `--open plaque.dcv`, shows the plaque (not the empty
/// state), renders one frame through wgpu, and saves the project to a new
/// path byte-identical to what the CLI's serializer writes (acceptance
/// 62). The file is the phase 2 quick start's.
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
        harness.query_by_label("Import a drawing…").is_none(),
        "the empty state is not shown over a project with shapes"
    );
    let title = harness.state().title().to_owned();
    assert!(
        title.contains("plaque.dcv"),
        "the title names the file: {title}"
    );
    assert!(!title.contains('•'), "and has no unsaved marker: {title}");
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
/// project asks; *Cancel* keeps it open and unchanged; *Discard* loses the
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
        h.state().title().contains('•'),
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
    for label in ["Save", "Discard", "Cancel"] {
        assert!(
            sheet.query_by_label(label).is_some(),
            "the sheet offers {label}"
        );
    }
    assert!(
        sheet.query_by_label_contains("plaque.dcv").is_some(),
        "the sheet names the file"
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

    // Discard closes without writing.
    press_close(&mut h);
    h.run();
    click_in_sheet(&mut h, "Discard");
    assert!(h.state().close_allowed(), "Discard lets the window close");
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

    // Opening a second project asks too: Cancel keeps the first, Discard
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
    click_in_sheet(&mut h, "Discard");
    assert_eq!(dialogs.asked(), ["open_project"]);
    assert_eq!(h.state().app().source(), Some(sign.as_path()));
    assert!(!h.state().app().is_dirty());
    assert!(!h.state().close_allowed());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 50 (capability 50, open question 12): at 1280 × 768 both
/// side panels are open at no less than their minimum (240 and 300 px,
/// `experience.md` section 3) with the Viewport at least 560 px wide;
/// below that size they collapse to strips instead of the window
/// refusing, without forgetting that the user had them open; a strip
/// reopens its panel; and the layout persists in `gui.toml`.
#[test]
fn panels_collapse_below_the_minimum() {
    let dir = scratch("panels");
    let config = dir.join("config");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, Some(plaque), &dialogs), 1280.0, 768.0);

    let open_at_minimum = |h: &Harness<'_, dreamcarve_app::Shell>| {
        let layout = h.state().layout();
        let PanelShown::Open { width: left } = layout.left else {
            panic!("left panel open at 1280 × 768: {layout:?}");
        };
        let PanelShown::Open { width: right } = layout.right else {
            panic!("right panel open at 1280 × 768: {layout:?}");
        };
        assert!(left >= 240.0 - 0.5, "left panel {left} px");
        assert!(right >= 300.0 - 0.5, "right panel {right} px");
        assert!(
            layout.viewport.width() >= 560.0 - 0.5,
            "Viewport {} px wide",
            layout.viewport.width()
        );
    };
    open_at_minimum(&h);

    for (width, height) in [(1279.0, 768.0), (1280.0, 767.0), (900.0, 600.0)] {
        h.set_size(egui::vec2(width, height));
        h.run();
        let layout = h.state().layout();
        assert_eq!(
            (layout.left, layout.right),
            (PanelShown::Strip, PanelShown::Strip),
            "{width} × {height}"
        );
        let panels = h.state().app().prefs().panels;
        assert!(
            panels.left_open && panels.right_open,
            "collapsing for size keeps the preference"
        );
    }

    // A strip reopens its panel, even in a small window.
    h.get_by_label("Show the Design Tree").click();
    h.run();
    assert!(matches!(h.state().layout().left, PanelShown::Open { .. }));

    // Back at full size both are open again.
    h.set_size(egui::vec2(1280.0, 768.0));
    h.run();
    open_at_minimum(&h);

    // Hiding a panel from the View menu is a preference, kept in gui.toml.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("CAM panel").click();
    h.run();
    assert_eq!(h.state().layout().right, PanelShown::Strip);
    assert!(!h.state().app().prefs().panels.right_open);
    let saved: GuiPrefs = gui_prefs::load(&Paths::in_dir(config.clone())).prefs;
    assert!(!saved.panels.right_open, "gui.toml has the hidden panel");
    assert!(saved.panels.left_open);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 50a and `experience.md` sections 4, 5 and 12: both palettes
/// are installed; with the preference at System the app follows the OS
/// appearance as it changes; a Light or Dark preference overrides the OS
/// and is kept in `gui.toml`; the bundled fonts and the type scale are in
/// force.
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

    // System: follow the OS, both ways.
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::System);
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();
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
/// directory; *Import…* reads `tests/fixtures/fusion/sample.tools` in
/// place, writes the library the CLI writes, and shows the report
/// `tools import` prints in place of the table, with *Back*; ticking two
/// rows and *Add 2 tools to project* copies them into the project under
/// new ids as one undo step, as `tools add` does, and marks them "in
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
        sheet.query_by_label_contains("tools.json").is_some(),
        "the header names the library file"
    );
    assert!(
        sheet
            .query_by_label(
                "The library is empty. Import a Fusion or DreamCarve library to fill it."
            )
            .is_some(),
        "no library yet, so no rows"
    );

    // Import… the Fusion sample: the report replaces the table.
    let sample = common::fixture("fusion/sample.tools");
    dialogs.answer(&sample);
    h.get_by_role(Role::Dialog).get_by_label("Import…").click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_library"]);
    let bytes = std::fs::read(&sample).unwrap();
    let merged = dc_app::library::import(&dc_library::Library::default(), &bytes, false).unwrap();
    let sheet = h.get_by_role(Role::Dialog);
    for line in merged.report.lines() {
        assert!(
            sheet.query_by_label(&line).is_some(),
            "the report shows `{line}`"
        );
    }
    assert!(
        sheet
            .query_by_label_contains("13 imported, 0 already present, 0 replaced, 10 skipped")
            .is_some(),
        "and the counts"
    );
    assert!(
        sheet
            .query_by_role_and_label(Role::CheckBox, "5 mm Drill")
            .is_none(),
        "in place of the table"
    );
    let written = std::fs::read_to_string(config.join("tools.json")).expect("library written");
    assert_eq!(
        written,
        dc_library::save(&merged.library).unwrap(),
        "the library file is the CLI's"
    );
    h.get_by_role(Role::Dialog).get_by_label("Back").click();
    h.run();

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
    // A search hides a ticked row but keeps its tick.
    h.get_by_role(Role::Dialog)
        .get_by_role(Role::TextInput)
        .focus();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role(Role::TextInput)
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

    // With the search cleared, both rows are marked and no longer ticked.
    h.get_by_role(Role::Dialog)
        .get_by_label("Clear the search")
        .click();
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

/// The camera fitted to `bounds` in the shell's pane.
fn fitted(h: &Harness<'_, dreamcarve_app::Shell>, bounds: dc_geometry::Rect) -> dc_app::Camera {
    let mut want = camera(h);
    want.fit(bounds);
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
/// cursor; F fits again, the toolbar's Fit too, and Shift F frames the
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

    // The toolbar's Fit.
    h.get_by_label("Fit").click();
    h.run();
    assert_same_view(camera(&h), fit_view, "the Fit button");
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

/// Capability 60 and acceptance 69 (#536): with nothing selected the
/// Properties area is the Project panel, and the park point is typed or
/// clicked on the Viewport. *Click on the view* arms the Viewport; the next
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
    assert!(
        h.query_by_label("Project · plaque.dcv").is_some(),
        "the Properties area is the Project panel"
    );
    assert_eq!(h.state().app().project().heights.park, None);

    h.get_by_label("Click on the view").scroll_to_me();
    h.run();
    h.get_by_label("Click on the view").click();
    h.run();
    // Empty stock near the plaque's top-right corner (stock 170 × 120).
    let want = dc_geometry::Point::new(165.0, 115.0);
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

/// Drops `path` on the window, as the OS hands a dropped file over.
fn drop_file(h: &mut Harness<'_, dreamcarve_app::Shell>, path: &std::path::Path) {
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path.to_path_buf())));
    h.run();
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
        sheet.query_by_label("Import drawing").is_some(),
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

    h.get_by_role(Role::Dialog).get_by_label("Import").click();
    h.run();
    assert!(
        h.query_by_role(Role::Dialog).is_none(),
        "Import closes the sheet"
    );
    let app = h.state().app();
    let (import, _) = dc_import::import_svg(
        &std::fs::read(&sign).unwrap(),
        &dc_import::SvgImportOptions::default(),
    )
    .unwrap();
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
        sheet.get_by_label("Import").accesskit_node().is_disabled(),
        "Import is disabled until units are chosen"
    );
    // No Cancel button (`experience.md`, section 13, item 9): the
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
    click_in_sheet(&mut h, "Discard");
    assert_eq!(h.state().app().source(), Some(plaque.as_path()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 51 and acceptance 63 (#535): File ▸ New from Drawing… asks
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
    h.get_by_label("New from Drawing…").click();
    h.run();
    assert_eq!(dialogs.asked(), ["open_drawing"]);
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label("New from drawing").is_some());
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

/// Capability 76 on the window: once the version 14 plaque (whose Drill
/// predates `allow_tool_wider_than_hole`, so its holes are too small for
/// its bit) has generated, the status bar's counts open the Warnings
/// drawer; a row about a hole selects the hole; *Copy* hands egui
/// `check`'s text (`dc_app::warnings::copy_text`), which reaches the OS
/// clipboard once eframe's `clipboard` feature is on (#532 left it off
/// for its licences); the close button closes it.
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
    h.get_by_label(&row.line).click();
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

/// Steps `h` once and returns how long the UI's frame took: the shell's
/// update and egui's layout on this thread, while the worker runs.
fn timed_step(h: &mut Harness<'_, dreamcarve_app::Shell>) -> std::time::Duration {
    let start = std::time::Instant::now();
    h.step();
    start.elapsed()
}

/// Acceptance 87 (the window never blocks; capability 71): on 500 shapes
/// with a Pocket and a V-Carve on all of them, generation runs on the
/// worker while the window answers input within 100 ms a frame, both
/// operations' rows show the spinner while they wait, the status bar
/// reads *Generating k of n…*, and *Stop* shows generation stopped on
/// the next frame, well within the second the spec allows: the worker
/// finishes the job it is on (design open question 4), and its result is
/// dropped when it lands. The frames are timed on the UI thread, not the
/// worker's jobs.
#[test]
fn window_answers_input_while_generating() {
    const FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(100);
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

    // The change settles, the worker takes the first job, and every frame
    // meanwhile stays inside the budget.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut slowest = std::time::Duration::ZERO;
    while !h.state().worker_busy() {
        assert!(
            std::time::Instant::now() < deadline,
            "the worker never started"
        );
        slowest = slowest.max(timed_step(&mut h));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..2 {
        slowest = slowest.max(timed_step(&mut h));
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
    // Warnings drawer, read in one frame and drawn in the next.
    h.get_by_label("Warnings").click();
    slowest = slowest.max(timed_step(&mut h));
    assert!(h.state().warnings_open(), "the click was read");
    slowest = slowest.max(timed_step(&mut h));
    assert!(
        h.query_by_label("Copy").is_some(),
        "the drawer is drawn in the next frame"
    );
    println!("window_answers_input_while_generating: slowest frame {slowest:?}");
    assert!(
        slowest < FRAME_BUDGET,
        "a frame took {slowest:?} while generating"
    );

    // Stop: stopped on the UI at once, nothing pending, the running job's
    // result dropped when it lands.
    assert!(h.state().app().generation().busy());
    let stopped_at = std::time::Instant::now();
    h.get_by_label("Stop").click();
    timed_step(&mut h);
    timed_step(&mut h);
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
    h.get_by_role_and_label(Role::ListItem, "Cut out").click();
    h.run();
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
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 67: the Targets group's *Pick…* arms the Viewport, and each
/// click on a shape adds it to the operation's targets, one step each,
/// without changing the selection, until Escape; after Escape a click
/// selects as usual.
#[test]
fn pick_targets_until_escape() {
    let dir = scratch("pick-targets");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    h.get_by_role_and_label(Role::ListItem, "Border and squiggle")
        .click();
    h.run();
    let targets = |h: &Harness<'_, dreamcarve_app::Shell>| -> Vec<String> {
        operation_of(h, "op5")
            .targets()
            .iter()
            .map(|t| t.as_str().to_owned())
            .collect()
    };
    assert_eq!(targets(&h), ["i9", "i10"]);
    // A point of each shape's outline, where a click picks it.
    let outline_point = |h: &Harness<'_, dreamcarve_app::Shell>, id: &str| {
        let shape = h
            .state()
            .app()
            .scene()
            .shape(&dc_project::InstanceId::new(id))
            .expect("the shape is drawn")
            .clone();
        common::on_screen(h, shape.flat[0].vertices()[0].point())
    };

    h.get_by_label("Pick…").click();
    h.run();
    let selection = h.state().app().selection().clone();
    let steps = undo_steps(&h);
    let square = outline_point(&h, "i4");
    common::click_at(&mut h, square, egui::Modifiers::NONE);
    assert_eq!(
        targets(&h),
        ["i9", "i10", "i4"],
        "the click adds the square"
    );
    let pocket = outline_point(&h, "i2");
    common::click_at(&mut h, pocket, egui::Modifiers::NONE);
    assert_eq!(targets(&h), ["i9", "i10", "i4", "i2"], "and the next one");
    assert_eq!(undo_steps(&h), steps + 2, "one step each");
    assert_eq!(h.state().app().selection(), &selection, "nothing selected");

    h.key_press(egui::Key::Escape);
    h.run();
    let outline = outline_point(&h, "i1");
    common::click_at(&mut h, outline, egui::Modifiers::NONE);
    assert_eq!(
        targets(&h),
        ["i9", "i10", "i4", "i2"],
        "Escape ended picking"
    );
    assert_eq!(
        h.state().app().selection(),
        &dc_app::Selection::Instances(vec![dc_project::InstanceId::new("i1")]),
        "and a click selects again"
    );
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
    h.get_by_label(&row.line).click();
    h.run();
    assert_eq!(
        h.state()
            .app()
            .editing()
            .map(dc_project::OperationId::as_str),
        Some("op4"),
        "the row opens the operation in the editor"
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
    h.get_by_role_and_label(Role::ListItem, "Cut out").click();
    h.run();
    assert!(
        h.get_by_label("Place tabs").accesskit_node().is_disabled(),
        "placing tabs is slice 17's"
    );

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

    // One click: the Viewport is disarmed after it.
    let elsewhere = common::on_screen(&h, dc_geometry::Point::new(160.0, 110.0));
    common::click_at(&mut h, elsewhere, egui::Modifiers::NONE);
    let dc_project::Operation::Profile(still) = operation_of(&h, "op4") else {
        panic!("still a Profile");
    };
    assert_eq!(still.start, after.start);
    std::fs::remove_dir_all(&dir).unwrap();
}
