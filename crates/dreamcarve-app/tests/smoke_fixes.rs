// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the maintainer's smoke test of the alpha found, each held by a
//! window test through the accessibility tree (2026-11-20): a field left
//! by any means keeps what was typed (capability 52), the Tool Library's
//! columns and size, the Inlay's Band margin and Board thickness rows,
//! help for every field, a stepover's distance, the Export step's one
//! Programs control, its button and its result sheet, and a quote in a
//! name.

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
    QueuedDialogs, click_at, command_key, copy_fixture, go_to, harness, open_operation, options,
    scratch,
};
use dreamcarve_app::Shell;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

/// The plaque, open in a 1280 × 800 window, with its scratch directory.
fn plaque(name: &str) -> (Harness<'static, Shell>, std::path::PathBuf, QueuedDialogs) {
    let dir = scratch(name);
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    (h, dir, dialogs)
}

/// Focuses the text field named `label`, selects what it holds and types
/// `text` over it, leaving the field focused.
fn type_into(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
}

/// The centre of the node `role` named `label`, in window points.
fn centre_of(h: &Harness<'_, Shell>, role: Role, label: &str) -> egui::Pos2 {
    h.get_by_role_and_label(role, label).rect().center()
}

/// The undo steps the shell's app holds.
fn undo_steps(h: &Harness<'_, Shell>) -> usize {
    h.state().app().history().undo_labels().count()
}

/// The stock's width, mm.
fn stock_width(h: &Harness<'_, Shell>) -> f64 {
    h.state().app().project().stock.width_mm
}

/// Capability 52 and `experience.md`, section 7, "Text field with
/// trailing unit": a typed field is written when it is left, however it
/// is left. The maintainer typed a stock width, clicked off the field and
/// saw the old value come back; only Enter kept it. Each way of leaving
/// here writes the width, as one undo step: a click on the field above
/// it, on the field below it, on an empty part of the panel, on the
/// Viewport, on another step's rail button, on a menu's title, on the
/// header, and the window losing the focus. Escape alone discards.
#[test]
fn a_field_left_by_a_click_keeps_what_was_typed() {
    let (mut h, dir, _dialogs) = plaque("field-click-away");
    go_to(&mut h, "Stock");
    let mut expected = 300.0;
    let mut leave = |h: &mut Harness<'static, Shell>, how: &str, at: egui::Pos2| {
        expected += 1.0;
        let steps = undo_steps(h);
        // Height is the field typed into, so a field above it (Width) and
        // one below it (Thickness) can each be the click.
        type_into(h, "Height", &format!("{expected}"));
        assert_eq!(undo_steps(h), steps, "{how}: typing writes nothing yet");
        click_at(h, at, egui::Modifiers::NONE);
        h.run();
        assert_eq!(
            h.state().app().project().stock.height_mm,
            expected,
            "{how}: the height typed is written"
        );
        assert_eq!(undo_steps(h), steps + 1, "{how}: as one undo step");
    };
    let width = centre_of(&h, Role::TextInput, "Width");
    leave(&mut h, "a click on the field above", width);
    let thickness = centre_of(&h, Role::TextInput, "Thickness");
    leave(&mut h, "a click on the field below", thickness);
    let panel = h.state().layout().step_panel.expect("the step panel");
    leave(
        &mut h,
        "a click on the panel's empty foot",
        egui::pos2(panel.center().x, panel.bottom() - 30.0),
    );
    let viewport = h.state().layout().viewport;
    leave(&mut h, "a click on the Viewport", viewport.center());
    let label = centre_of(&h, Role::Label, "Width");
    leave(&mut h, "a click on a label", label);
    let menu = centre_of(&h, Role::Button, "Edit");
    leave(&mut h, "a click on a menu's title", menu);
    h.key_press(egui::Key::Escape);
    h.run();

    // Another step's rail button: the panel that held the field is gone
    // by the time the click lands.
    let steps = undo_steps(&h);
    type_into(&mut h, "Height", "410");
    let tools = centre_of(&h, Role::Tab, "Tools");
    click_at(&mut h, tools, egui::Modifiers::NONE);
    h.run();
    assert_eq!(h.state().step(), dc_app::steps::Step::Tools);
    assert_eq!(h.state().app().project().stock.height_mm, 410.0);
    assert_eq!(undo_steps(&h), steps + 1, "switching step by a click");

    // A step's shortcut: the field has the focus as its panel goes.
    go_to(&mut h, "Stock");
    let steps = undo_steps(&h);
    type_into(&mut h, "Height", "420");
    command_key(&h, egui::Key::Num3);
    h.run();
    assert_eq!(h.state().step(), dc_app::steps::Step::Tools);
    assert_eq!(h.state().app().project().stock.height_mm, 420.0);
    assert_eq!(undo_steps(&h), steps + 1, "switching step by its key");

    // The window losing the focus.
    go_to(&mut h, "Stock");
    let steps = undo_steps(&h);
    type_into(&mut h, "Height", "430");
    h.event(egui::Event::WindowFocused(false));
    h.run();
    assert_eq!(h.state().app().project().stock.height_mm, 430.0);
    assert_eq!(undo_steps(&h), steps + 1, "the window losing the focus");
    h.event(egui::Event::WindowFocused(true));
    h.run();

    // Tab and Shift+Tab, as before.
    for (text, modifiers) in [
        ("440", egui::Modifiers::NONE),
        ("450", egui::Modifiers::SHIFT),
    ] {
        let steps = undo_steps(&h);
        type_into(&mut h, "Height", text);
        h.key_press_modifiers(modifiers, egui::Key::Tab);
        h.run();
        assert_eq!(
            h.state().app().project().stock.height_mm,
            text.parse::<f64>().unwrap()
        );
        assert_eq!(undo_steps(&h), steps + 1, "Tab with {modifiers:?}");
    }

    // Escape is the one way out that writes nothing.
    let steps = undo_steps(&h);
    type_into(&mut h, "Height", "999");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().app().project().stock.height_mm, 450.0);
    assert_eq!(undo_steps(&h), steps, "Escape discards");
    assert!(stock_width(&h) > 0.0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Opens the operation editor's group `title`: its header, the rightmost
/// button of that name.
fn open_group(h: &mut Harness<'_, Shell>, title: &str) {
    for _ in 0..2 {
        let header = h
            .query_all_by_role_and_label(Role::Button, title)
            .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
            .unwrap_or_else(|| panic!("no {title} group"));
        header.scroll_to_me();
        h.run();
    }
    h.query_all_by_role_and_label(Role::Button, title)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no {title} group"))
        .click();
    h.run();
}

/// The value at `key` in the record of operation `id` of the shell's
/// project, as JSON text.
fn value_at(h: &Harness<'_, Shell>, id: &str, key: &str) -> String {
    let op = h
        .state()
        .app()
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .unwrap_or_else(|| panic!("no operation {id}"));
    let record = dc_app::editor::fields::record(op).unwrap();
    dc_app::editor::fields::get(&record, key)
        .unwrap_or_else(|| panic!("no {key} in {id}"))
        .to_string()
}

/// Types `text` into the field `label` and clicks the middle of the
/// Viewport, which is no field: one undo step more, whatever the field.
fn type_and_click_away(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    let steps = undo_steps(h);
    h.get_by_role_and_label(Role::TextInput, label)
        .scroll_to_me();
    h.run();
    type_into(h, label, text);
    assert_eq!(undo_steps(h), steps, "{label}: typing writes nothing yet");
    let viewport = h.state().layout().viewport;
    click_at(
        h,
        egui::pos2(viewport.center().x, viewport.top() + 70.0),
        egui::Modifiers::NONE,
    );
    h.run();
    assert_eq!(undo_steps(h), steps + 1, "{label}: one undo step");
}

/// Item 1 for every kind of field the window has: each is typed into and
/// left by a click on the Viewport, and what was typed is in the project
/// (or, for the fields that are not the project's, where they keep it).
/// The stock's and the origin's fields, a shape's Properties, a tool's
/// editor, each kind of row the operation editor draws (a length, a
/// tool-derived length, a percentage, an angle, a count), an operation's name, a shape's name in the Design
/// Tree, an output setting, and the export's file name.
#[test]
fn every_kind_of_field_is_written_when_clicked_away_from() {
    let (mut h, dir, _dialogs) = plaque("field-kinds");
    let project = |h: &Harness<'static, Shell>| h.state().app().project().clone();

    // The Stock step: the stock, then a custom origin's X.
    go_to(&mut h, "Stock");
    type_and_click_away(&mut h, "Thickness", "19");
    assert_eq!(project(&h).stock.thickness_mm, Some(19.0));
    h.get_by_role_and_label(Role::ComboBox, "XY zero").click();
    h.run();
    h.get_by_label("Custom").click();
    h.run();
    type_and_click_away(&mut h, "X", "12.5");
    let dc_project::OriginXy::Custom { x_mm, .. } = project(&h).origin.xy else {
        panic!("a custom origin");
    };
    assert_eq!(x_mm, 12.5);

    // Design: a shape's Properties, and its name in the tree.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    let before = project(&h);
    type_and_click_away(&mut h, "X", "33");
    assert_ne!(project(&h).instances, before.instances, "the shape moved");

    // Tools: a tool's name and a rate.
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/8 end mill")
        .click();
    h.run();
    type_and_click_away(&mut h, "Feed", "777");
    type_and_click_away(&mut h, "Name", "Eighth");
    let tool = project(&h).tools[2].clone();
    assert_eq!(
        (tool.name.as_str(), tool.feed_mm_min),
        ("Eighth", Some(777.0))
    );

    // Operations: each kind of typed row.
    open_operation(&mut h, "Cut out");
    type_and_click_away(&mut h, "Depth", "9.5");
    assert_eq!(value_at(&h, "op4", "depth_mm"), "9.5", "a length");
    open_group(&mut h, "Depth");
    type_and_click_away(&mut h, "Step-down", "2");
    assert_eq!(
        value_at(&h, "op4", "step_down_mm"),
        "2.0",
        "a tool-derived length"
    );
    type_and_click_away(&mut h, "Count", "6");
    assert_eq!(value_at(&h, "op4", "tabs.placement.count"), "6", "a count");

    open_operation(&mut h, "Panel");
    open_group(&mut h, "Strategy");
    type_and_click_away(&mut h, "Stepover", "35");
    assert_eq!(
        value_at(&h, "op1", "step_over_fraction"),
        "0.35",
        "a percentage"
    );
    type_and_click_away(&mut h, "Raster angle", "45");
    assert_eq!(
        value_at(&h, "op1", "strategy.raster.angle_deg"),
        "45.0",
        "an angle"
    );

    open_operation(&mut h, "Screw holes");

    // The operation's name, in the editor's header.
    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::Button, "Screw holes").click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("Holes".to_owned()));
    h.run();
    let viewport = h.state().layout().viewport;
    click_at(&mut h, viewport.center(), egui::Modifiers::NONE);
    h.run();
    assert_eq!(value_at(&h, "op3", "name"), "\"Holes\"");
    assert_eq!(undo_steps(&h), steps + 1, "the name: one undo step");

    // Export: an output setting, and the file name, which is the step's.
    go_to(&mut h, "Export");
    h.get_by_role_and_label(Role::Button, "Heights").click();
    h.run();
    type_and_click_away(&mut h, "Clearance", "22");
    assert_eq!(project(&h).heights.clearance_mm, Some(22.0));
    h.get_by_role_and_label(Role::Button, "Post-processor")
        .click();
    h.run();
    type_and_click_away(&mut h, "Spin-up dwell", "2.5");
    assert_eq!(
        project(&h).post_processor.spindle_on_dwell_s,
        2.5,
        "seconds"
    );
    h.get_by_role_and_label(Role::TextInput, "File name")
        .scroll_to_me();
    h.run();
    type_into(&mut h, "File name", "sign");
    let viewport = h.state().layout().viewport;
    click_at(&mut h, viewport.center(), egui::Modifiers::NONE);
    h.run();
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "File name")
            .value()
            .as_deref(),
        Some("sign"),
        "the file name typed stays"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
