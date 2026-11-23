// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 9 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan → The
//! window"): drawing in the window. The Draw button and its popover, the
//! tool bar, a shape drawn by clicks and by typed sizes, the tools' keys,
//! View ▸ Snapping, *Draw from scratch*, and the Properties panel's Shape
//! section (functional capabilities 81 to 85; acceptance 129 and 130).
//!
//! What a tool does with its clicks is tested with no window in
//! `dc-app` (`tests/draw.rs`, `draw_path.rs`, `draw_gear.rs`,
//! `shape.rs`); these tests hold the window to passing them on.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use common::{QueuedDialogs, click_at, copy_fixture, harness, on_screen, options, scratch};
use dc_app::{Drawing, Tool};
use dc_geometry::Point;
use dc_project::{Corner, Shape};
use dreamcarve_app::Shell;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

/// A window on the plaque, 1280 × 800, with snapping off so a point is
/// where the test puts it.
fn plaque(name: &str) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(name);
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    common::settle(&mut h);
    (h, dir)
}

/// A window on a new, empty project.
fn blank(name: &str) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(name);
    let dialogs = QueuedDialogs::default();
    let h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    (h, dir)
}

fn tool(h: &Harness<'_, Shell>) -> Tool {
    h.state().app().tool()
}

fn key(h: &mut Harness<'_, Shell>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

/// Clicks project point `(x, y)` with snapping held off (⌘ or Ctrl).
fn click_mm(h: &mut Harness<'_, Shell>, x: f64, y: f64) {
    let at = on_screen(h, Point::new(x, y));
    click_at(h, at, egui::Modifiers::COMMAND);
}

/// Whether something on the window is labelled `text`: the tool bar's
/// prompt is also the status bar's, so there may be two.
fn says(h: &Harness<'_, Shell>, text: &str) -> bool {
    h.query_all_by_label(text).next().is_some()
}

/// Types `text` over what the field named `label` holds, and Enter.
fn type_in(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

fn labels(h: &Harness<'_, Shell>, role: Role) -> Vec<String> {
    h.query_all_by_role(role)
        .filter_map(|node| node.accesskit_node().label())
        .collect()
}

/// Capability 81, as the review of 2026-11-23 regrouped it: the tool
/// strip down the Viewport's left has a button for each group of drawing
/// tools. A click arms the tool the group holds and opens the group's
/// list beside it, each tool with its name and key; a row arms that tool
/// and makes it the one the group holds. Text, Trim and Measure have
/// buttons of their own, and Snapping lists what a point snaps to.
#[test]
fn the_tool_strip_groups_the_drawing_tools() {
    use dc_app::tool::ToolGroup;
    let (mut h, dir) = plaque("tool-strip");
    assert_eq!(tool(&h), Tool::Select);
    let listed = |h: &Harness<'_, Shell>, names: &[&str]| -> Vec<String> {
        labels(h, Role::Button)
            .into_iter()
            .filter(|label| names.contains(&label.as_str()))
            .collect()
    };
    let every = [
        "Line",
        "Arc",
        "Spline",
        "Rectangle",
        "Circle",
        "Ellipse",
        "Polygon",
        "Slot",
        "Gear",
    ];
    assert!(listed(&h, &every).is_empty(), "no list is open");

    // The shapes: the Rectangle first, then the one picked from the list.
    h.get_by_role_and_label(Role::Button, "Shape tools").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Rectangle));
    assert_eq!(
        listed(&h, &every),
        ["Rectangle", "Circle", "Ellipse", "Polygon"]
    );
    h.get_by_role_and_label(Role::Button, "Ellipse").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Ellipse));
    assert_eq!(h.state().app().held(ToolGroup::Shapes), Drawing::Ellipse);
    assert!(listed(&h, &every).is_empty(), "the list closed");

    // The lines, and the special ones.
    h.get_by_role_and_label(Role::Button, "Line tools").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Line));
    assert_eq!(listed(&h, &every), ["Line", "Arc", "Spline"]);
    // Escape with the list showing closes the list and keeps the tool.
    key(&mut h, egui::Key::Escape);
    assert!(listed(&h, &every).is_empty());
    assert_eq!(tool(&h), Tool::Draw(Drawing::Line));
    h.get_by_role_and_label(Role::Button, "Line tools").click();
    h.run_steps(8);
    assert_eq!(listed(&h, &every), ["Line", "Arc", "Spline"]);
    // A press on the Viewport with the list showing is the tool's: the
    // list is no menu to dismiss first.
    click_mm(&mut h, 120.0, 20.0);
    assert!(listed(&h, &every).is_empty());
    let prompt = h.state().app().tool_bar().map(|bar| bar.prompt);
    assert!(
        prompt
            .as_deref()
            .is_some_and(|prompt| prompt.starts_with("Click the next point")),
        "the first point was taken: {prompt:?}"
    );
    key(&mut h, egui::Key::Escape);
    key(&mut h, egui::Key::Escape);
    h.get_by_role_and_label(Role::Button, "Slot and gear tools")
        .click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Slot));
    assert_eq!(listed(&h, &every), ["Slot", "Gear"]);
    h.get_by_role_and_label(Role::Button, "Gear").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Gear));
    assert_eq!(h.state().app().prefs().drawing_tool, Drawing::Gear);

    // Text, Trim and Measure each have a button.
    for (name, armed) in [
        ("Text", Tool::Draw(Drawing::Text)),
        ("Trim", Tool::Trim),
        ("Measure", Tool::Measure),
        ("Select", Tool::Select),
    ] {
        h.get_by_role_and_label(Role::Button, name).click();
        h.run_steps(8);
        assert_eq!(tool(&h), armed, "{name}");
    }

    // Snapping: the master switch and a tick for each kind.
    assert!(h.state().app().prefs().snap.enabled);
    let button = h.get_by_role_and_label(Role::Button, "Snapping").rect();
    h.get_by_role_and_label(Role::Button, "Snapping").click();
    h.run_steps(8);
    for kind in ["Endpoint", "Midpoint", "Centre", "Grid"] {
        assert!(
            h.query_all_by_label(kind).next().is_some(),
            "the list has {kind}"
        );
    }
    // The list's own Snapping row, which is not the strip's button.
    h.query_all_by_label("Snapping")
        .find(|node| node.rect() != button)
        .expect("the list's Snapping row")
        .click();
    h.run_steps(8);
    assert!(!h.state().app().prefs().snap.enabled, "switched off there");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 81: while a tool is armed the tool bar floats over the
/// Viewport with the tool's name, what the next click does and the
/// fields of the step; the status bar says the same; the selection's
/// handles are gone and a click selects nothing.
#[test]
fn arming_a_tool_shows_its_bar() {
    let (mut h, dir) = plaque("tool-bar");
    assert!(!says(&h, "Click the first corner"));
    key(&mut h, egui::Key::M);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Rectangle));
    let pane = h.state().layout().viewport;
    let prompt = h
        .query_all_by_label("Click the first corner")
        .map(|node| node.rect())
        .find(|rect| pane.contains_rect(*rect))
        .expect("the tool bar's prompt is over the Viewport");
    assert!(
        prompt.top() < pane.top() + 120.0,
        "the bar floats at the Viewport's top: {prompt:?}"
    );
    assert!((prompt.center().x - pane.center().x).abs() < pane.width() / 2.0);
    // Before the first point: X and Y from the origin.
    let fields: Vec<String> = labels(&h, Role::TextInput);
    assert!(fields.contains(&"X".to_owned()) && fields.contains(&"Y".to_owned()));
    // The status bar's pointer cell says the same.
    assert!(
        h.query_all_by_label("Click the first corner").count() >= 2,
        "the bar and the status bar both say what the next click does"
    );

    // A click on a shape is the tool's first corner, not a selection.
    let before = h.state().app().selection().clone();
    click_mm(&mut h, 70.0, 33.0);
    assert_eq!(h.state().app().selection(), &before);
    assert!(says(&h, "Click the opposite corner, or type its size"));
    let fields: Vec<String> = labels(&h, Role::TextInput);
    assert!(fields.contains(&"W".to_owned()) && fields.contains(&"H".to_owned()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 129: with the Rectangle armed, two clicks on the Viewport
/// draw a rectangle between them, one undo step, and the tool stays
/// armed for the next.
#[test]
fn two_clicks_draw_a_rectangle() {
    let (mut h, dir) = plaque("two-clicks");
    let before = h.state().app().project().instances.len();
    key(&mut h, egui::Key::M);
    click_mm(&mut h, 10.0, 60.0);
    assert_eq!(h.state().app().project().instances.len(), before);
    // The preview follows the pointer.
    let to = on_screen(&h, Point::new(40.0, 80.0));
    h.event(egui::Event::PointerMoved(to));
    h.run();
    assert_eq!(h.state().app().draft().outlines.len(), 1);
    click_mm(&mut h, 40.0, 80.0);

    let app = h.state().app();
    let project = app.project();
    assert_eq!(project.instances.len(), before + 1);
    let drawn = project.instances.last().unwrap();
    assert_eq!(drawn.name, "Rectangle 1");
    let Some(Shape::Rectangle {
        width_mm,
        height_mm,
        corner,
    }) = project.definitions[&drawn.definition].shape.clone()
    else {
        panic!("a rectangle");
    };
    // A click lands on a pixel: within a pixel's worth of the sizes.
    let px_mm = 1.0 / app.view().camera.px_per_mm;
    assert!((width_mm - 30.0).abs() <= px_mm && (height_mm - 20.0).abs() <= px_mm);
    assert_eq!(corner, Corner::Square);
    assert_eq!(app.undo_label(), Some("Draw “Rectangle 1”"));
    assert_eq!(tool(&h), Tool::Draw(Drawing::Rectangle), "it stays armed");
    assert!(says(&h, "Click the first corner"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 83: a size typed in the tool bar is pinned, and Enter in
/// the field commits the step with the fields as they read.
#[test]
fn typed_sizes_commit_on_enter() {
    let (mut h, dir) = plaque("typed-sizes");
    let before = h.state().app().project().instances.len();
    key(&mut h, egui::Key::M);
    click_mm(&mut h, 10.0, 60.0);
    let to = on_screen(&h, Point::new(30.0, 75.0));
    h.event(egui::Event::PointerMoved(to));
    h.run();

    // Tab, with nothing focused, goes to the bar's first field, its text
    // selected: typing replaces what the pointer gave.
    key(&mut h, egui::Key::Tab);
    assert!(
        h.get_by_role_and_label(Role::TextInput, "W").is_focused(),
        "Tab goes to the first field"
    );
    h.event(egui::Event::Text("42".to_owned()));
    h.run();
    // Tab leaves the field, which writes it: the width is pinned, and
    // the height has the keyboard.
    key(&mut h, egui::Key::Tab);
    let bar = h.state().app().tool_bar().unwrap();
    let w = &bar.fields[0];
    assert!(w.pinned && w.text == "42", "{w:?}");
    assert_eq!(h.state().app().project().instances.len(), before);
    assert!(h.get_by_role_and_label(Role::TextInput, "H").is_focused());
    h.event(egui::Event::Text("17.5".to_owned()));
    h.run();
    key(&mut h, egui::Key::Enter);

    let project = h.state().app().project();
    assert_eq!(project.instances.len(), before + 1);
    let drawn = project.instances.last().unwrap();
    assert_eq!(
        project.definitions[&drawn.definition].shape,
        Some(Shape::Rectangle {
            width_mm: 42.0,
            height_mm: 17.5,
            corner: Corner::Square,
        })
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 81: each tool's key arms it; Escape steps out of what the
/// tool has begun, then out of the tool; `V` puts a tool down; `T` still
/// turns the Tab tool on and off.
#[test]
fn tool_keys_arm_and_escape_disarms() {
    let (mut h, dir) = plaque("tool-keys");
    for (pressed, drawing) in [
        (egui::Key::P, Drawing::Line),
        (egui::Key::A, Drawing::Arc),
        (egui::Key::S, Drawing::Spline),
        (egui::Key::M, Drawing::Rectangle),
        (egui::Key::C, Drawing::Circle),
        (egui::Key::E, Drawing::Ellipse),
        (egui::Key::G, Drawing::Polygon),
        (egui::Key::O, Drawing::Slot),
    ] {
        key(&mut h, pressed);
        assert_eq!(tool(&h), Tool::Draw(drawing), "{pressed:?}");
    }
    key(&mut h, egui::Key::V);
    assert_eq!(tool(&h), Tool::Select);
    key(&mut h, egui::Key::T);
    assert_eq!(tool(&h), Tool::Tab);
    key(&mut h, egui::Key::T);
    assert_eq!(tool(&h), Tool::Select);
    key(&mut h, egui::Key::T);
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select, "Escape puts the Tab tool down");

    // Escape: the draft first, then the tool. Delete is the tool's too.
    let before = h.state().app().project().instances.len();
    let shape = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![shape]));
    key(&mut h, egui::Key::P);
    click_mm(&mut h, 10.0, 60.0);
    click_mm(&mut h, 30.0, 60.0);
    click_mm(&mut h, 30.0, 75.0);
    key(&mut h, egui::Key::Backspace);
    assert_eq!(
        h.state().app().project().instances.len(),
        before,
        "Backspace took back a point and deleted nothing"
    );
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().app().project().instances.len(), before + 1);
    assert_eq!(
        h.state().app().project().instances.last().unwrap().name,
        "Line 1"
    );
    click_mm(&mut h, 10.0, 60.0);
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Draw(Drawing::Line), "the draft went");
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 82: View ▸ Snapping has the master switch and a tick per
/// kind; ⇧S toggles the switch; the choice is the app's, kept in
/// `gui.toml`.
#[test]
fn the_snapping_menu_toggles_a_kind() {
    let (mut h, dir) = plaque("snapping-menu");
    let snap = |h: &Harness<'_, Shell>| h.state().app().prefs().snap.clone();
    assert!(snap(&h).enabled);
    assert!(snap(&h).kinds.contains(&dc_app::SnapKind::Midpoint));
    assert!(!snap(&h).kinds.contains(&dc_app::SnapKind::Perpendicular));

    // The switch is a row of the View menu, with its key; the kinds are
    // under Snap To.
    h.get_by_role_and_label(Role::Button, "View").click();
    h.run();
    let switch = h
        .query_all_by_role(Role::Button)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Snapping "))
        })
        .expect("View has the Snapping switch");
    assert_eq!(
        switch.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    h.get_by_role_and_label(Role::Button, "Snap To").click();
    h.run();
    for kind in [
        "Endpoint",
        "Midpoint",
        "Centre",
        "Intersection",
        "On outline",
        "Perpendicular",
        "Grid",
    ] {
        // *Grid* is also the menu beside this one.
        assert!(
            h.query_all_by_role_and_label(Role::Button, kind)
                .next()
                .is_some(),
            "{kind}"
        );
    }
    h.get_by_role_and_label(Role::Button, "Perpendicular")
        .click();
    h.run();
    assert!(snap(&h).kinds.contains(&dc_app::SnapKind::Perpendicular));

    // ⇧S: off, then on again.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
    h.run();
    assert!(!snap(&h).enabled);
    assert_eq!(tool(&h), Tool::Select, "⇧S is not the Spline's S");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::S);
    h.run();
    assert!(snap(&h).enabled);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 130: on a new project the start screen's *Draw from
/// scratch* arms the drawing tool on the empty stock, the Design step
/// open; a shape drawn there is the project's first.
#[test]
fn draw_from_scratch_opens_design() {
    let (mut h, dir) = blank("draw-from-scratch");
    assert!(
        h.query_by_label("Start with a drawing").is_some(),
        "the start screen"
    );
    h.get_by_role_and_label(Role::Button, "Draw from scratch")
        .click();
    h.run();
    assert_eq!(tool(&h), Tool::Draw(Drawing::Rectangle));
    assert!(
        h.query_by_label("Start with a drawing").is_none(),
        "the stock shows, to draw on"
    );
    assert!(says(&h, "Click the first corner"));
    click_mm(&mut h, 20.0, 20.0);
    click_mm(&mut h, 60.0, 50.0);
    assert_eq!(h.state().app().project().instances.len(), 1);
    // Put down, the tool leaves the drawing: the start screen stays gone.
    key(&mut h, egui::Key::V);
    assert!(h.query_by_label("Start with a drawing").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 84: the drawn shape, selected, shows its sizes in the
/// Properties panel's Shape section; a size typed there is one undo step
/// named for it, and Shape ▸ Convert to Path takes the section away.
#[test]
fn the_shape_section_edits_a_size() {
    let (mut h, dir) = plaque("shape-section");
    key(&mut h, egui::Key::M);
    click_mm(&mut h, 10.0, 60.0);
    click_mm(&mut h, 40.0, 80.0);
    key(&mut h, egui::Key::V);
    let drawn = h
        .state()
        .app()
        .project()
        .instances
        .last()
        .unwrap()
        .id
        .clone();
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![drawn]));
    h.run();

    assert!(
        h.query_by_role_and_label(Role::TextInput, "Width")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Height")
            .is_some()
    );
    type_in(&mut h, "Width", "50");
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Set width of “Rectangle 1”"));
    let definition =
        &app.project().definitions[&app.project().instances.last().unwrap().definition];
    let Some(Shape::Rectangle { width_mm, .. }) = definition.shape else {
        panic!("a rectangle");
    };
    assert!((width_mm - 50.0).abs() < 1e-9);

    // Rounded corners bring the corner size's row.
    h.get_by_role_and_label(Role::RadioButton, "Rounded")
        .click();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Corner size")
            .is_some()
    );
    assert_eq!(
        h.state().app().undo_label(),
        Some("Set corners of “Rectangle 1”")
    );

    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Convert to Path")
        .click();
    h.run();
    assert_eq!(
        h.state().app().undo_label(),
        Some("Convert “Rectangle 1” to a path")
    );
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Width")
            .is_none()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 86 in the window: the Gear tool takes its centre and its
/// outside radius by two clicks, then shows the gear's sizes and choices
/// in the tool bar until Done makes it.
#[test]
fn the_gear_tool_shows_its_sizes() {
    let (mut h, dir) = blank("gear-tool");
    h.get_by_role_and_label(Role::Button, "Draw from scratch")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Slot and gear tools")
        .click();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Gear").click();
    h.run();
    assert_eq!(tool(&h), Tool::Draw(Drawing::Gear));
    click_mm(&mut h, 50.0, 50.0);
    assert!(says(
        &h,
        "Click a point on the outside circle, or type its diameter"
    ));
    click_mm(&mut h, 70.0, 50.0);
    assert!(says(&h, "Set the gear's sizes, then Done"));
    let fields = labels(&h, Role::TextInput);
    for field in [
        "Teeth",
        "Outside diameter",
        "Pressure angle",
        "Bore diameter",
    ] {
        assert!(fields.contains(&field.to_owned()), "{field}: {fields:?}");
    }
    assert!(
        h.query_by_role_and_label(Role::RadioButton, "Involute")
            .is_some()
    );
    assert!(says(&h, "Module 1.54"));
    h.get_by_role_and_label(Role::Button, "Done").click();
    // The bar goes from three rows to one, which it measures over a few
    // frames: more than `run` waits for.
    h.run_steps(8);
    let project = h.state().app().project();
    assert_eq!(project.instances.len(), 1, "{:?}", h.state().message());
    assert_eq!(project.instances[0].name, "Gear 1");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 85 and 85b in the window: the Rectangle's corners and
/// the Polygon's sides are set in the tool bar, before the shape is
/// drawn.
#[test]
fn the_tool_bar_sets_a_tools_options() {
    let (mut h, dir) = plaque("tool-options");
    key(&mut h, egui::Key::M);
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Corner size")
            .is_none(),
        "square corners have no size"
    );
    h.get_by_role_and_label(Role::RadioButton, "Rounded")
        .click();
    h.run_steps(8);
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Corner size")
            .is_some()
    );
    click_mm(&mut h, 10.0, 60.0);
    click_mm(&mut h, 40.0, 80.0);
    let project = h.state().app().project();
    let drawn = project.instances.last().unwrap();
    let Some(Shape::Rectangle { corner, .. }) = project.definitions[&drawn.definition].shape else {
        panic!("a rectangle");
    };
    assert_eq!(corner, Corner::Rounded { radius_mm: 5.0 });

    key(&mut h, egui::Key::G);
    h.run_steps(8);
    // Enter in an option's field writes the option and takes no point.
    type_in(&mut h, "Sides", "8");
    assert_eq!(h.state().app().prefs().drawing.polygon_sides, 8);
    assert!(says(&h, "Click the centre"), "no centre was taken");
    click_mm(&mut h, 20.0, 70.0);
    click_mm(&mut h, 28.0, 70.0);
    assert_eq!(
        h.state().app().project().instances.last().unwrap().name,
        "Octagon 1"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
