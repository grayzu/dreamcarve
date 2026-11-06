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
    shift_command_key, window_commands,
};
use dc_app::Command;
use dreamcarve_app::{PanelShown, theme};
use dreamcarve_shell::{GuiPrefs, Paths, gui_prefs};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, wgpu::WgpuTestRenderer};

/// Whether this machine has a wgpu adapter to render with, asked the way
/// egui_kittest's renderer will ask (the backends `WGPU_BACKEND` allows).
fn wgpu_adapter_available() -> bool {
    use eframe::egui_wgpu::{WgpuSetup, wgpu};
    let WgpuSetup::CreateNew(setup) = WgpuSetup::without_display_handle() else {
        return false;
    };
    let instance = wgpu::Instance::new(setup.instance_descriptor);
    let adapters = block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    !adapters.is_empty()
}

/// Runs a future that native wgpu completes without waiting.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::yield_now();
    }
}

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
            .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options))
    } else {
        println!(
            "opens_plaque_renders_and_exports: no wgpu adapter on this machine, so the \
             frame is not rendered; the open and save still run (set WGPU_BACKEND or \
             install Mesa's lavapipe to render)"
        );
        Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options))
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
    assert!(title.contains("plaque.dcv"), "the title names the file: {title}");
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
    harness.get_by_role(Role::Dialog).get_by_label(label).click();
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
    assert!(h.state().title().contains('•'), "the title marks the change");
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
    assert!(h.query_by_role(Role::Dialog).is_none(), "Cancel closes the sheet");
    assert!(!h.state().close_allowed(), "Cancel keeps the window open");
    assert_eq!(h.state().app().project(), &edited, "and the project as it was");
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

    let (light, dark) = h.ctx.options(|o| {
        (
            o.light_style.visuals.clone(),
            o.dark_style.visuals.clone(),
        )
    });
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
    assert_eq!(h.ctx.global_style().visuals.panel_fill, theme::LIGHT.surface_panel);
    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.run();
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(h.ctx.global_style().visuals.panel_fill, theme::DARK.surface_panel);

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
        !fonts.font_data.keys().any(|k| k.contains("Ubuntu") || k.contains("Hack")),
        "egui's own fonts are not bundled: {:?}",
        fonts.font_data.keys()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
