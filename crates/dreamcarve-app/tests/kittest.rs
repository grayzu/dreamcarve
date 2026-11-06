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
