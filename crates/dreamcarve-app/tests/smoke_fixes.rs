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
use egui_kittest::kittest::{NodeT as _, Queryable as _};

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

/// The Inlay of the shell's project.
fn inlay_of(h: &Harness<'_, Shell>) -> dc_project::InlayOp {
    match h.state().app().project().operations.first() {
        Some(dc_project::Operation::Inlay(inlay)) => inlay.clone(),
        other => panic!("the pair's first operation is its Inlay: {other:?}"),
    }
}

/// The lettered inlay pair, open in a 1280 × 900 window, after `edit`.
fn pair(
    name: &str,
    edit: impl FnOnce(&mut dc_project::Project),
) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(name);
    let bytes = std::fs::read(common::fixture("dc.dcv")).unwrap();
    let mut project = dc_project::load(&bytes).unwrap().project;
    edit(&mut project);
    let path = dir.join("dc.dcv");
    std::fs::write(&path, dc_project::save(&project).unwrap()).unwrap();
    let dialogs = QueuedDialogs::default();
    let h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        900.0,
    );
    (h, dir)
}

/// The smoke test's item 4 (capability 68a): with a shape chosen as the
/// Boundary, the Band margin row is disabled and says *Set by the
/// boundary* where its value was, the Outline rounding with it, and the
/// file holds no margin; the closed Depth group's summary names the
/// boundary and no margin. Choosing *None* enables both again.
#[test]
fn band_margin_is_disabled_while_a_boundary_is_chosen() {
    let (mut h, dir) = pair("inlay-margin", |_| {});
    open_operation(&mut h, "DC");
    open_group(&mut h, "Depth");
    let margin = |h: &Harness<'_, Shell>| {
        let node = h.get_by_role_and_label(Role::TextInput, "Band margin");
        (
            node.accesskit_node().is_disabled(),
            node.value().unwrap_or_default(),
            node.accesskit_node().placeholder().map(str::to_owned),
        )
    };
    assert!(!margin(&h).0, "enabled with no boundary");
    assert_eq!(margin(&h).1, "8");
    assert_eq!(inlay_of(&h).margin_mm, Some(8.0));

    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::ComboBox, "Boundary").click();
    h.run();
    h.get_by_label("frame").click();
    h.run();
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert_eq!(h.state().app().undo_label(), Some("Set boundary of “DC”"));
    assert_eq!(
        margin(&h),
        (true, String::new(), Some("Set by the boundary".to_owned())),
        "disabled, saying why in the value's place"
    );
    let inlay = inlay_of(&h);
    assert_eq!(inlay.margin_mm, None, "no margin is written");
    assert_eq!(inlay.outline_rounding_mm, None);
    let rounding = h.get_by_role_and_label(Role::TextInput, "Outline rounding");
    assert!(rounding.accesskit_node().is_disabled());
    // Typing reaches nothing: the row is not a field while it is off.
    assert!(
        h.query_by_label_contains("does nothing").is_none(),
        "and validation has nothing to say of it"
    );

    // Closed, the group says what draws the band.
    open_group(&mut h, "Depth");
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Band margin")
            .is_none(),
        "closed"
    );
    let header = h
        .query_all_by_role_and_label(Role::Button, "Depth")
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap();
    let summary = header.value().unwrap_or_default();
    assert!(summary.starts_with("Boundary frame"), "{summary}");
    assert!(!summary.to_lowercase().contains("margin"), "{summary}");
    open_group(&mut h, "Depth");

    // None: the margin is the band's again, empty and asked for.
    h.get_by_role_and_label(Role::ComboBox, "Boundary").click();
    h.run();
    h.get_by_label("None (use the band margin)").click();
    h.run();
    assert!(!margin(&h).0, "enabled again");
    type_into(&mut h, "Band margin", "7");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(inlay_of(&h).margin_mm, Some(7.0));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's item 5 (capabilities 66a, 66b and 68a): the male
/// board's thickness is the first row of the Male board group, not a
/// Setup row. The group is closed until opened; while the thickness is
/// unset it opens by itself, and the field shows the required look, so
/// the value the male needs is in view the moment the Inlay is.
#[test]
fn board_thickness_is_the_male_boards_first_row() {
    // Set: Setup has no thickness, and the group is closed.
    let (mut h, dir) = pair("inlay-thickness-set", |_| {});
    open_operation(&mut h, "DC");
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Board thickness")
            .is_none(),
        "not in Setup, and the Male board group starts closed"
    );
    open_group(&mut h, "Male board");
    let top = |h: &Harness<'_, Shell>, label: &str| {
        h.get_by_role_and_label(Role::TextInput, label).rect().top()
    };
    assert!(top(&h, "Board thickness") < top(&h, "Board width"));
    assert!(top(&h, "Board width") < top(&h, "Board height"));
    assert!(top(&h, "Plug depth") < top(&h, "Board thickness"));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();

    // Unset: the group opens by itself, and the field says Required.
    let (mut h, dir) = pair("inlay-thickness-unset", |project| {
        let Some(dc_project::Operation::Inlay(inlay)) = project.operations.first_mut() else {
            panic!("the pair's Inlay");
        };
        inlay.male_board.thickness_mm = None;
    });
    open_operation(&mut h, "DC");
    // The warning is the generator's: it is there once the toolpaths are.
    common::settle(&mut h);
    let thickness = h.get_by_role_and_label(Role::TextInput, "Board thickness");
    assert_eq!(
        thickness.accesskit_node().placeholder(),
        Some("Required"),
        "the group opened for its unset thickness"
    );
    type_into(&mut h, "Board thickness", "12");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(inlay_of(&h).male_board.thickness_mm, Some(12.0));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The help line at the foot of a panel: the node whose name starts with
/// `label` and a colon, when it shows.
fn help_line(h: &Harness<'_, Shell>, label: &str) -> Option<(String, egui::Rect)> {
    let start = format!("{label}: ");
    h.query_all_by_role(Role::Label)
        .filter_map(|node| {
            let node_ak = node.accesskit_node();
            let name = node_ak.label().or_else(|| node_ak.value())?;
            name.starts_with(&start).then(|| (name, node.rect()))
        })
        .next()
}

/// The smoke test's item 6 (capability 66; `experience.md`, section 7,
/// "Help affordance"): the field that has the keyboard shows its help on
/// one line at the foot of the panel that holds it, the step panel's,
/// the Inspector's or a sheet's, and the line goes when the field is
/// left. A control that is not a text field has it too.
#[test]
fn the_focused_field_shows_its_help_at_the_foot_of_its_panel() {
    let (mut h, dir, _dialogs) = plaque("field-help");
    let strings = dc_app::Strings::english();

    // The Stock step: nothing until a field has the keyboard.
    go_to(&mut h, "Stock");
    assert!(help_line(&h, "Thickness").is_none());
    h.get_by_role_and_label(Role::TextInput, "Thickness")
        .focus();
    h.run();
    let (text, at) = help_line(&h, "Thickness").expect("the stock thickness's help");
    assert!(
        text.ends_with(strings.get(dc_app::StrKey::ProjectStockThicknessHelp)),
        "{text}"
    );
    let panel = h.state().layout().step_panel.expect("the step panel");
    assert!(
        panel.contains_rect(at) && at.bottom() > panel.bottom() - 12.0,
        "at the foot of the step panel: {at:?} in {panel:?}"
    );
    let field = h.get_by_role_and_label(Role::TextInput, "Thickness").rect();
    assert!(field.bottom() < at.top(), "clear of the field it is about");
    // Another field: its help takes the line.
    h.get_by_role_and_label(Role::TextInput, "Width").focus();
    h.run();
    assert!(help_line(&h, "Thickness").is_none());
    assert!(help_line(&h, "Width").is_some());
    // A control that is not typed into.
    h.get_by_role_and_label(Role::ComboBox, "XY zero").focus();
    h.run();
    let (text, _) = help_line(&h, "XY zero").expect("the origin's help");
    assert!(text.contains("X0 Y0"), "{text}");
    // Left: the line goes.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(help_line(&h, "XY zero").is_none());

    // The Inspector: a tool's field, then an operation's.
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Stepover").focus();
    h.run();
    let (text, at) = help_line(&h, "Stepover").expect("a tool's stepover has help");
    assert!(text.contains("above 0 and at most 100 %"), "{text}");
    let inspector = h.state().layout().inspector.expect("the Inspector");
    assert!(
        inspector.contains_rect(at),
        "at the foot of the Inspector: {at:?} in {inspector:?}"
    );
    open_operation(&mut h, "Cut out");
    h.get_by_role_and_label(Role::TextInput, "Depth").focus();
    h.run();
    let (text, at) = help_line(&h, "Depth").expect("an operation's depth has help");
    assert!(text.contains("How deep the cut goes"), "{text}");
    assert!(h.state().layout().inspector.unwrap().contains_rect(at));

    // A sheet: the Tool Library's editing pane.
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    std::fs::remove_dir_all(&dir).unwrap();
}
