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

    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's item 7: beside a stepover percentage the distance
/// between passes it comes to, following what is typed. The plaque's
/// 1/4 end mill at 40 % says 2.54 mm; `8` typed says 0.508 mm and `0.08`
/// says 0.005 mm before the field is left, so the slip is seen while it
/// is made. A Pocket's own Stepover says its distance too, the tool's
/// while it is empty.
#[test]
fn a_stepover_field_says_its_distance_as_it_is_typed() {
    let (mut h, dir, _dialogs) = plaque("stepover-note");
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    let field = h.get_by_role_and_label(Role::TextInput, "Stepover").rect();
    let note = h.get_by_label("2.54 mm between passes").rect();
    assert!(
        note.top() >= field.bottom() - 1.0 && note.left() >= field.left() - 12.0,
        "the note is under its field: {note:?}, {field:?}"
    );
    let steps = undo_steps(&h);
    type_into(&mut h, "Stepover", "8");
    assert!(h.query_by_label("0.508 mm between passes").is_some());
    assert!(h.query_by_label("2.54 mm between passes").is_none());
    type_into(&mut h, "Stepover", "0.08");
    assert!(
        h.query_by_label("0.005 mm between passes").is_some(),
        "0.08 % of 6.35 mm, as it is typed"
    );
    assert_eq!(undo_steps(&h), steps, "nothing is written while typing");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("2.54 mm between passes").is_some());

    // A Pocket's own stepover: empty takes the tool's, and says so.
    open_operation(&mut h, "Panel");
    open_group(&mut h, "Strategy");
    let field = h.get_by_role_and_label(Role::TextInput, "Stepover");
    let empty = field.value().unwrap_or_default().is_empty();
    let shown = if empty { "2.54 mm between passes" } else { "" };
    if empty {
        assert!(h.query_by_label(shown).is_some(), "the tool's 40 %");
    }
    type_into(&mut h, "Stepover", "50");
    assert!(h.query_by_label("3.175 mm between passes").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's item 7, second half: a finding located at a tool's
/// field shows under that field in the tool editor, in its tier's colour,
/// as an operation's does. A plunge rate typed above the feed is
/// `plunge_above_feed`, a warning at the plunge.
#[test]
fn a_tool_field_shows_the_warning_located_at_it() {
    let (mut h, dir, _dialogs) = plaque("tool-warning");
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    assert!(h.query_by_label_contains("plunges at").is_none());
    let feed: f64 = h
        .get_by_role_and_label(Role::TextInput, "Feed")
        .value()
        .unwrap_or_default()
        .parse()
        .expect("the plaque's end mill has a feed");
    type_into(&mut h, "Plunge", &format!("{}", feed + 500.0));
    h.key_press(egui::Key::Tab);
    h.run();
    common::settle(&mut h);
    let line = h.get_by_label_contains("plunges at");
    let field = h.get_by_role_and_label(Role::TextInput, "Plunge").rect();
    assert!(
        line.rect().top() >= field.bottom() - 1.0 && line.rect().left() >= field.left() - 1.0,
        "the warning is under the Plunge field"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's item 8 (capabilities 62 and 77): how several tools
/// are written is one control, **Programs** on the Export step, with two
/// plainly named choices. It is the project's own setting: choosing *One
/// program for all tools* writes `tool_change_mode`, one undo step, and
/// there is no "as the project says", no "this export only" and no second
/// row for it in the Post-processor group. No label names a G-code word.
#[test]
fn programs_is_one_control_that_sets_the_project() {
    let (mut h, dir, _dialogs) = plaque("programs-control");
    common::settle(&mut h);
    go_to(&mut h, "Export");
    common::settle(&mut h);
    let mode = |h: &Harness<'_, Shell>| h.state().app().project().post_processor.tool_change_mode;
    assert_eq!(mode(&h), dc_project::ToolChangeMode::SeparatePrograms);
    assert_eq!(
        h.query_all_by_role_and_label(Role::ComboBox, "Programs")
            .count(),
        1
    );
    let shown = h
        .get_by_role_and_label(Role::ComboBox, "Programs")
        .value()
        .unwrap_or_default();
    assert_eq!(shown, "One program per tool");
    for gone in ["As the project says", "this export only", "M6"] {
        assert!(h.query_by_label_contains(gone).is_none(), "no `{gone}`");
    }

    let steps = undo_steps(&h);
    h.get_by_role_and_label(Role::ComboBox, "Programs").click();
    h.run();
    for choice in ["One program per tool", "One program for all tools"] {
        assert!(
            h.query_all_by_label(choice).next().is_some(),
            "{choice} is a choice"
        );
    }
    assert!(h.query_by_label_contains("M6").is_none(), "no G-code word");
    h.query_all_by_label("One program for all tools")
        .last()
        .expect("the choice")
        .click();
    h.run();
    assert_eq!(
        mode(&h),
        dc_project::ToolChangeMode::M6,
        "the project's setting"
    );
    assert_eq!(undo_steps(&h), steps + 1, "one undo step");
    assert!(
        h.state().app().is_dirty(),
        "a project change, to save or not"
    );
    command_key(&h, egui::Key::Z);
    h.run();
    assert_eq!(mode(&h), dc_project::ToolChangeMode::SeparatePrograms);

    // The Post-processor group has no row for it.
    h.get_by_role_and_label(Role::Button, "Post-processor")
        .click();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::ComboBox, "Tool change")
            .is_none()
    );
    assert_eq!(
        h.query_all_by_role_and_label(Role::ComboBox, "Programs")
            .count(),
        1,
        "one place only"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's items 9 and 10 (capabilities 77 and 77a): **Export
/// G-code** is at the foot of the Export step's panel, pinned under its
/// scrolling settings, and the header shows no primary button on that
/// step. While something blocks the export the button is off, with the
/// reason beside it. A successful export shows its result as a sheet;
/// *Done* closes it, and the step then says what was written and opens
/// the result again.
#[test]
fn export_button_is_at_the_foot_of_the_step_and_the_result_is_a_sheet() {
    let (mut h, dir, _dialogs) = plaque("export-foot");
    common::settle(&mut h);
    // Another step: the header's primary is Continue.
    go_to(&mut h, "Stock");
    let header_bottom = h.state().layout().viewport.top();
    let continue_button = h.get_by_role_and_label(Role::Button, "Continue").rect();
    assert!(continue_button.bottom() <= header_bottom);
    assert!(
        h.query_by_role_and_label(Role::Button, "Export G-code")
            .is_none()
    );

    // ⌘E opens the Export step; its button is in the panel, at its foot.
    command_key(&h, egui::Key::E);
    h.run();
    common::settle(&mut h);
    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    assert!(
        h.query_by_role_and_label(Role::Button, "Continue")
            .is_none(),
        "no primary button in the header on the Export step"
    );
    let foot = |h: &Harness<'_, Shell>| {
        let panel = h.state().layout().step_panel.expect("the step panel");
        let button = h
            .get_by_role_and_label(Role::Button, "Export G-code")
            .rect();
        assert!(panel.contains_rect(button), "{button:?} in {panel:?}");
        assert!(
            button.bottom() > panel.bottom() - 28.0,
            "at the foot: {button:?} in {panel:?}"
        );
        button
    };
    let at = foot(&h);
    let ready = h.get_by_label("Ready to write 2 programs.").rect();
    assert!(
        ready.right() <= at.left() && ready.center().y > at.top() - 8.0,
        "what it will write is beside it"
    );
    // Every group open: the settings scroll, the button stays.
    for group in ["Post-processor", "Heights", "Tolerances"] {
        h.get_by_role_and_label(Role::Button, group).click();
        h.run();
    }
    assert_eq!(foot(&h), at, "pinned");
    assert!(
        !h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled()
    );

    // The export: a sheet with what was written.
    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    for part in [
        "2 programs written",
        "Show in folder",
        "Save setup sheet…",
        "Copy path",
        "Run instructions",
        "Done",
    ] {
        assert!(sheet.query_by_label(part).is_some(), "the sheet has {part}");
    }
    assert!(dir.join("plaque-1-T2.nc").is_file() && dir.join("plaque-2-T1.nc").is_file());
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none(), "Done closes it");
    assert!(
        h.query_by_label_contains("2 programs written.").is_some(),
        "the step says what was written"
    );
    assert!(
        h.query_by_label("Exported. Nothing has changed since.")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Show result").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_some(), "the result again");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none());

    // Blocked: the button is off, and the reason is beside it.
    let project = h.state().app().project();
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(tool))
        .unwrap();
    h.run();
    common::settle(&mut h);
    let button = foot(&h);
    assert!(
        h.get_by_role_and_label(Role::Button, "Export G-code")
            .accesskit_node()
            .is_disabled()
    );
    let beside = h
        .query_all_by_label("1 error blocks export")
        .map(|node| node.rect())
        .find(|rect| rect.center().y > button.top() - 8.0)
        .expect("the reason is beside the button");
    assert!(beside.right() <= button.left());
    assert!(
        h.query_by_label("Show result").is_none(),
        "the result goes when the project changes"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The smoke test's item 11, a check: the maintainer's hand-made tools
/// read `IDC Woodcraft 1/4\" Fine Detail Tapered Ball Nose`, with a
/// backslash before the quote. A name with a double quote typed into the
/// app is kept and shown as typed, with no backslash, through every way
/// a name travels: a tool's name, *Duplicate*, *Save to library* (the
/// library file read back), the Tool Library's own name field and its
/// table, *Add to project*, an operation's name and a shape's, the
/// project saved and read again, and every label on screen along the
/// way. So the app does not make that name: it is one that was typed, or
/// pasted, with the backslash in it.
#[test]
fn a_quote_in_a_name_is_kept_and_shown_as_typed() {
    const NAME: &str = "IDC 1/4\" Fine Detail";
    let (mut h, dir, dialogs) = plaque("quote-in-name");
    common::settle(&mut h);
    let escaped = |h: &Harness<'_, Shell>| h.query_all_by_label_contains("\\\"").count();
    let tool_names = |h: &Harness<'_, Shell>| -> Vec<String> {
        let tools = &h.state().app().project().tools;
        tools.iter().map(|t| t.name.clone()).collect()
    };

    // A tool's name, and its row in the list.
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    type_into(&mut h, "Name", NAME);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(tool_names(&h)[1], NAME);
    assert!(h.query_by_role_and_label(Role::ListItem, NAME).is_some());
    assert_eq!(escaped(&h), 0, "no label shows an escaped quote");

    // Duplicate, then Save to library: the library file holds the name.
    h.get_by_role_and_label(Role::Button, "Duplicate").click();
    h.run();
    assert_eq!(
        tool_names(&h).last().map(String::as_str),
        Some("IDC 1/4\" Fine Detail (2)")
    );
    h.get_by_role_and_label(Role::ListItem, NAME).click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Save to library")
        .click();
    h.run();
    let library_file = dir.join("config").join("tools.json");
    let library = dc_library::load(&std::fs::read(&library_file).unwrap()).unwrap();
    assert_eq!(library.tools.len(), 1);
    assert_eq!(library.tools[0].tool.name, NAME);
    assert_eq!(escaped(&h), 0, "the status bar's message among them");

    // The Tool Library: its table, its name field, and Add to project.
    h.get_by_role_and_label(Role::Button, "Tool Library…")
        .click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(
        sheet
            .query_by_role_and_label(Role::ListItem, NAME)
            .is_some()
    );
    assert_eq!(
        sheet
            .get_by_role_and_label(Role::TextInput, "Name")
            .value()
            .as_deref(),
        Some(NAME)
    );
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, "Name")
        .focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("3/8\" \"Whiteside\"".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let library = dc_library::load(&std::fs::read(&library_file).unwrap()).unwrap();
    assert_eq!(library.tools[0].tool.name, "3/8\" \"Whiteside\"");
    assert_eq!(escaped(&h), 0, "the library's table and pane");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::CheckBox, "3/8\" \"Whiteside\"")
        .click();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_label("Add 1 tool to project")
        .click();
    h.run();
    assert_eq!(
        tool_names(&h).last().map(String::as_str),
        Some("3/8\" \"Whiteside\"")
    );
    assert_eq!(escaped(&h), 0);

    // An operation's name, in its header, its row and the undo label.
    open_operation(&mut h, "Cut out");
    h.get_by_role_and_label(Role::Button, "Cut out").click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("Cut \"out\"".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let op = h.state().app().project().operations[4].name().to_owned();
    assert_eq!(op, "Cut \"out\"");
    assert!(
        h.query_by_role_and_label(Role::ListItem, "Cut \"out\"")
            .is_some(),
        "its row in the list"
    );
    // The next step names it in the Edit menu.
    type_into(&mut h, "Depth", "9");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(
        h.state().app().undo_label(),
        Some("Set depth of “Cut \"out\"”")
    );
    assert_eq!(escaped(&h), 0);

    // A shape's name, in the Design Tree.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    h.key_press(egui::Key::F2);
    h.run();
    h.event(egui::Event::Text("2\" square".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(
        h.state()
            .app()
            .project()
            .instances
            .iter()
            .any(|i| i.name == "2\" square")
    );
    assert!(
        h.query_by_role_and_label(Role::TreeItem, "2\" square")
            .is_some()
    );
    assert_eq!(
        escaped(&h),
        0,
        "the tree, the Properties' title, the status bar"
    );

    // Saved and read again: the names are the ones typed.
    let saved = dir.join("quoted.dcv");
    dialogs.answer(&saved);
    common::shift_command_key(&h, egui::Key::S);
    h.run();
    let read = dc_project::load(&std::fs::read(&saved).unwrap())
        .unwrap()
        .project;
    assert_eq!(&read, h.state().app().project());
    assert_eq!(read.tools[1].name, NAME);
    // In the file a quote is JSON's `\"`, which is the only place a
    // backslash stands before it.
    let text = std::fs::read_to_string(&saved).unwrap();
    assert!(text.contains("IDC 1/4\\\" Fine Detail"));
    assert!(!text.contains("\\\\\""), "never a literal backslash");
    std::fs::remove_dir_all(&dir).unwrap();
}
