// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 17 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan → The
//! window"): editing in the window. A double-click to edit a path's
//! nodes, a node dragged, the Edit nodes bar's buttons, Trim from the
//! Draw popover, and the Shape menu's Offset…, Weld and Join…
//! (functional capabilities 92 to 97).
//!
//! What each edit makes is tested with no window in `dc-app`
//! (`tests/nodes.rs`, `edit.rs`, `trim.rs`, `adjust.rs`); these tests
//! hold the window to passing the pointer, the keys and the menus on.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use common::{QueuedDialogs, click_at, copy_fixture, on_screen, options, scratch};
use dc_app::tool::Adjust;
use dc_app::{Selection, Tool};
use dc_geometry::Point;
use dc_project::InstanceId;
use dreamcarve_app::Shell;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

/// A window on the plaque, 1280 × 800, snapping off.
fn plaque(name: &str) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(name);
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let options = options(&dir.join("config"), Some(plaque), &dialogs);
    // Frames 1/60 s apart, so two clicks a few frames apart are a
    // double-click (egui's limit is 0.3 s; the harness's default step is
    // 0.25 s).
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| Shell::new(cc, options).expect("the shell starts"));
    h.run();
    common::settle(&mut h);
    h.state_mut().app_mut().prefs_mut().snap.enabled = false;
    (h, dir)
}

fn tool(h: &Harness<'_, Shell>) -> Tool {
    h.state().app().tool()
}

fn key(h: &mut Harness<'_, Shell>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

fn select(h: &mut Harness<'_, Shell>, ids: &[&str]) {
    h.state_mut().app_mut().select(Selection::Instances(
        ids.iter().map(|id| InstanceId::new(*id)).collect(),
    ));
    h.run();
}

/// The square's corners as it is placed: `i4` of the plaque.
fn square_corners(h: &Harness<'_, Shell>) -> Vec<Point> {
    let project = h.state().app().project();
    let instance = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == "i4")
        .unwrap();
    project.definitions[&instance.definition].profiles[0]
        .vertices()
        .iter()
        .map(|v| instance.transform.apply(v.point()))
        .collect()
}

fn names(h: &Harness<'_, Shell>) -> Vec<String> {
    h.state()
        .app()
        .project()
        .instances
        .iter()
        .map(|i| i.name.clone())
        .collect()
}

/// Draws a line from `a` to `b` with the Line tool, snapping held off.
fn draw_line(h: &mut Harness<'_, Shell>, a: (f64, f64), b: (f64, f64)) {
    key(h, egui::Key::P);
    for (x, y) in [a, b] {
        let at = on_screen(h, Point::new(x, y));
        click_at(h, at, egui::Modifiers::NONE);
    }
    key(h, egui::Key::Enter);
    key(h, egui::Key::V);
}

/// Capability 92: a double-click on a path edits its nodes: the shape is
/// selected alone, the Edit nodes tool is armed and shows the path's
/// nodes; `N` does the same for the selected shape; Escape leaves with
/// the shape selected.
#[test]
fn a_double_click_shows_nodes() {
    let (mut h, dir) = plaque("double-click-nodes");
    let corners = square_corners(&h);
    let side = Point::new(
        (corners[0].x + corners[1].x) / 2.0,
        (corners[0].y + corners[1].y) / 2.0,
    );
    let at = on_screen(&h, side);
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(tool(&h), Tool::Select, "one click selects");
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(tool(&h), Tool::Nodes, "the second, in place, edits");
    let app = h.state().app();
    assert_eq!(app.node_editing(), Some(InstanceId::new("i4")));
    assert_eq!(app.draft().nodes.len(), corners.len());
    assert!(
        h.query_all_by_label("Drag a node or a handle; double-click a segment to add a node")
            .next()
            .is_some(),
        "the bar says what to do"
    );
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    assert_eq!(
        h.state().app().selection().instances(),
        [InstanceId::new("i4")]
    );
    // N, with the shape selected; and the pill's button.
    key(&mut h, egui::Key::N);
    assert_eq!(tool(&h), Tool::Nodes);
    key(&mut h, egui::Key::V);
    h.get_by_role_and_label(Role::Button, "Edit nodes").click();
    h.run();
    assert_eq!(tool(&h), Tool::Nodes);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 117 in the window: a corner node dragged moves that vertex
/// and nothing else, one undo step; the bar's Fillet rounds the selected
/// corner; Delete deletes the selected node, not the shape.
#[test]
fn a_node_is_dragged_and_a_corner_filleted() {
    let (mut h, dir) = plaque("drag-node");
    select(&mut h, &["i4"]);
    key(&mut h, egui::Key::N);
    let corners = square_corners(&h);
    let from = on_screen(&h, corners[0]);
    let to_mm = Point::new(corners[0].x - 4.0, corners[0].y - 3.0);
    let to = on_screen(&h, to_mm);
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    let moved = square_corners(&h);
    let px_mm = 1.0 / h.state().app().view().camera.px_per_mm;
    assert!(
        (moved[0].x - to_mm.x).abs() <= px_mm && (moved[0].y - to_mm.y).abs() <= px_mm,
        "{:?}",
        moved[0]
    );
    assert_eq!(&moved[1..], &corners[1..], "the other corners stay");
    assert_eq!(h.state().app().undo_label(), Some("Move node of “square”"));

    // The opposite corner, selected by a click, then Fillet.
    let at = on_screen(&h, corners[2]);
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(h.state().app().selected_nodes().len(), 1);
    h.get_by_role_and_label(Role::Button, "Fillet").click();
    h.run_steps(8);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Fillet corner of “square”")
    );
    assert_eq!(square_corners(&h).len(), corners.len() + 1);

    // Delete is the tool's: a node goes, the shape stays.
    let count = h.state().app().project().instances.len();
    let at = on_screen(&h, corners[1]);
    click_at(&mut h, at, egui::Modifiers::NONE);
    key(&mut h, egui::Key::Delete);
    assert_eq!(h.state().app().project().instances.len(), count);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Delete node of “square”")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 96 in the window: Trim is armed from the Draw popover; the
/// piece under the pointer is highlighted and a click takes it out.
#[test]
fn trim_from_the_popover() {
    let (mut h, dir) = plaque("trim-popover");
    // Two lines that cross, below the plaque's shapes.
    draw_line(&mut h, (10.0, 6.0), (60.0, 6.0));
    draw_line(&mut h, (40.0, 2.0), (40.0, 12.0));
    let before = names(&h);
    h.get_by_role_and_label(Role::Button, "Drawing tools")
        .click();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Trim").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Trim);
    // Over the arm past the crossing: the piece shows.
    let arm = on_screen(&h, Point::new(52.0, 6.0));
    h.event(egui::Event::PointerMoved(arm));
    h.run();
    assert_eq!(h.state().app().draft().highlights.len(), 1);
    click_at(&mut h, arm, egui::Modifiers::NONE);
    assert_eq!(h.state().app().undo_label(), Some("Trim “Line 1”"));
    assert_eq!(names(&h), before, "shortened, the same shapes");
    let project = h.state().app().project();
    let line = project
        .instances
        .iter()
        .find(|i| i.name == "Line 1")
        .unwrap();
    let outline = &project.definitions[&line.definition].profiles[0];
    let end = outline.vertices().last().unwrap();
    assert!((end.x - 40.0).abs() < 0.5, "trimmed back to the crossing");
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 94, 95 and 97 in the window: Shape ▸ Offset… takes its
/// distance in the tool bar and makes the offset on Done; Shape ▸ Weld
/// acts at once on two selected shapes; one that cannot act says why.
#[test]
fn offset_and_weld_from_the_shape_menu() {
    let (mut h, dir) = plaque("shape-menu");
    select(&mut h, &["i4"]);
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    for row in [
        "Convert to Path",
        "Fillet Corners…",
        "Weld",
        "Subtract",
        "Intersect",
    ] {
        assert!(
            h.query_by_role_and_label(Role::Button, row).is_some(),
            "{row}"
        );
    }
    let offset = h
        .query_all_by_role(Role::Button)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Offset…"))
        })
        .expect("Shape has Offset…");
    offset.click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Adjust(Adjust::Offset));
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Distance")
            .is_some()
    );
    assert_eq!(h.state().app().draft().outlines.len(), 1, "the preview");
    let count = h.state().app().project().instances.len();
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Offset “square”"));
    assert_eq!(h.state().app().project().instances.len(), count + 1);
    assert_eq!(tool(&h), Tool::Select);
    assert!(names(&h).contains(&"square offset".to_owned()));

    // The square and its offset, welded: one shape where there were two.
    let made = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .find(|i| i.name == "square offset")
        .unwrap()
        .id
        .clone();
    select(&mut h, &["i4", made.as_str()]);
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Weld").click();
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Weld 2 shapes"));
    assert_eq!(h.state().app().project().instances.len(), count);
    assert!(names(&h).contains(&"square weld".to_owned()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 97 in the window: Shape ▸ Join… takes its tolerance in the
/// tool bar; Done joins the selected open paths that touch into one
/// shape. With nothing to join there is no Done, and the bar says why.
#[test]
fn join_from_the_shape_menu() {
    let (mut h, dir) = plaque("join-menu");
    // Two lines that share an end, below the plaque's shapes.
    draw_line(&mut h, (10.0, 6.0), (40.0, 6.0));
    draw_line(&mut h, (40.0, 6.0), (60.0, 10.0));
    let lines: Vec<InstanceId> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .filter(|i| i.name.starts_with("Line"))
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(lines.len(), 2);
    let count = h.state().app().project().instances.len();
    h.state_mut()
        .app_mut()
        .select(Selection::Instances(lines.clone()));
    h.run();
    let join = |h: &mut Harness<'_, Shell>| {
        h.get_by_role_and_label(Role::Button, "Shape").click();
        h.run();
        h.query_all_by_role(Role::Button)
            .find(|node| {
                node.accesskit_node()
                    .label()
                    .is_some_and(|label| label.starts_with("Join…"))
            })
            .expect("Shape has Join…")
            .click();
        h.run_steps(8);
    };
    join(&mut h);
    assert_eq!(tool(&h), Tool::Adjust(Adjust::Join));
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Tolerance")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Join 2 shapes"));
    assert_eq!(h.state().app().project().instances.len(), count - 1);
    assert_eq!(tool(&h), Tool::Select);

    // One shape alone has nothing to join: the row is off.
    join(&mut h);
    assert_eq!(tool(&h), Tool::Select, "Join… is off for one shape");
    key(&mut h, egui::Key::Escape);

    // Two paths with no ends near each other: no Done, and the bar says
    // why.
    draw_line(&mut h, (70.0, 2.0), (90.0, 2.0));
    let open: Vec<InstanceId> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .filter(|i| i.name.starts_with("Line") || i.name.starts_with("Path"))
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(open.len(), 2);
    h.state_mut().app_mut().select(Selection::Instances(open));
    h.run();
    join(&mut h);
    assert_eq!(tool(&h), Tool::Adjust(Adjust::Join));
    let bar = h.state().app().tool_bar().expect("Join has a bar");
    assert!(!bar.can_finish);
    assert!(bar.fields[0].problem.is_some(), "the field says why");
    assert!(h.query_by_role_and_label(Role::Button, "Done").is_none());
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The square's middle, in the project.
fn square_middle(h: &Harness<'_, Shell>) -> Point {
    let corners = square_corners(h);
    let (mut x, mut y) = (0.0, 0.0);
    for corner in &corners {
        x += corner.x;
        y += corner.y;
    }
    #[allow(clippy::cast_precision_loss, reason = "a handful of corners")]
    let n = corners.len() as f64;
    Point::new(x / n, y / n)
}

/// A click of the secondary button at `at`.
fn right_click(h: &mut Harness<'_, Shell>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(8);
}

fn definition_of(h: &Harness<'_, Shell>, instance: &str) -> String {
    h.state()
        .app()
        .project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap()
        .definition
        .as_str()
        .to_owned()
}

/// Capability 98 in a window, which is sent Cut, Copy and Paste as
/// events of their own and not as keys: Copy fills the clipboard and
/// says so, Paste puts the shapes under the pointer, Cut takes them out.
/// While a field is typed in they are the field's.
#[test]
fn copy_and_paste_events() {
    let (mut h, dir) = plaque("clipboard-events");
    select(&mut h, &["i4"]);
    let count = h.state().app().project().instances.len();
    h.event(egui::Event::Copy);
    h.run();
    assert!(h.state().app().can_paste());
    assert_eq!(h.state().message(), Some("Copied “square”"));
    assert_eq!(h.state().app().project().instances.len(), count);

    // Under the pointer: the copy's middle is where the pointer is.
    let at_mm = Point::new(150.0, 12.0);
    let at = on_screen(&h, at_mm);
    h.event(egui::Event::PointerMoved(at));
    h.run();
    h.event(egui::Event::Paste("square".to_owned()));
    h.run();
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Paste “square”"));
    assert_eq!(app.project().instances.len(), count + 1);
    let pasted = app.project().instances.last().unwrap();
    assert_eq!(pasted.name, "square (2)");
    assert_eq!(
        app.selection().instances(),
        std::slice::from_ref(&pasted.id)
    );
    assert_ne!(pasted.definition.as_str(), definition_of(&h, "i4"));
    let outline = pasted
        .transform
        .apply_polyline(&app.project().definitions[&pasted.definition].profiles[0])
        .unwrap()
        .bounds()
        .unwrap();
    let px_mm = 1.0 / app.view().camera.px_per_mm;
    assert!(
        (f64::midpoint(outline.min.x, outline.max.x) - at_mm.x).abs() <= px_mm
            && (f64::midpoint(outline.min.y, outline.max.y) - at_mm.y).abs() <= px_mm,
        "{outline:?}"
    );

    // Cut takes the selection, which is what was pasted.
    h.event(egui::Event::Cut);
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Cut “square (2)”"));
    assert_eq!(h.state().app().project().instances.len(), count);

    // In a field, Copy is the field's: the shapes' clipboard is not
    // refilled from the selection.
    select(&mut h, &["i1"]);
    h.get_by_role_and_label(Role::TextInput, "X").focus();
    h.run();
    h.event(egui::Event::Copy);
    h.run();
    h.event(egui::Event::Paste("12".to_owned()));
    h.run();
    assert_eq!(h.state().app().project().instances.len(), count);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Viewport's context menu: on a shape, the shape is selected and
/// the menu is its Design Tree row's, with Cut, Copy and Duplicate; on
/// nothing it is Paste and Select All, and Paste puts the shapes where
/// the menu was opened.
#[test]
fn the_viewport_has_a_context_menu() {
    let (mut h, dir) = plaque("context-menu");
    let count = h.state().app().project().instances.len();
    let middle = on_screen(&h, square_middle(&h));
    right_click(&mut h, middle);
    assert_eq!(
        h.state().app().selection().instances(),
        [InstanceId::new("i4")],
        "a right-click selects the shape under it"
    );
    for row in ["Rename", "Cut", "Copy", "Duplicate", "Delete", "Zoom to"] {
        assert!(
            h.query_by_role_and_label(Role::Button, row).is_some(),
            "{row}"
        );
    }
    assert!(
        h.query_by_role_and_label(Role::Button, "Paste").is_none(),
        "nothing was copied yet"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Make Unique")
            .is_none(),
        "the square is no linked copy"
    );
    h.get_by_role_and_label(Role::Button, "Copy").click();
    h.run_steps(8);
    assert!(h.state().app().can_paste());
    assert!(h.query_by_role_and_label(Role::Button, "Copy").is_none());

    // On nothing: the plaque's outline holds the whole stock, so beside
    // it.
    let at_mm = Point::new(-6.0, 60.0);
    let bare = on_screen(&h, at_mm);
    assert!(h.state().layout().viewport.contains(bare));
    right_click(&mut h, bare);
    assert!(h.query_by_role_and_label(Role::Button, "Cut").is_none());
    assert!(
        h.query_by_role_and_label(Role::Button, "Select All")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(8);
    let app = h.state().app();
    assert_eq!(app.project().instances.len(), count + 1);
    let pasted = app.project().instances.last().unwrap();
    let outline = pasted
        .transform
        .apply_polyline(&app.project().definitions[&pasted.definition].profiles[0])
        .unwrap()
        .bounds()
        .unwrap();
    let px_mm = 1.0 / app.view().camera.px_per_mm;
    assert!(
        (f64::midpoint(outline.min.x, outline.max.x) - at_mm.x).abs() <= px_mm,
        "pasted where the menu was opened: {outline:?}"
    );

    // A Design Tree row's menu is the same one.
    h.get_by_label("square").click_secondary();
    h.run_steps(8);
    for row in ["Cut", "Copy", "Paste"] {
        assert!(
            h.query_by_role_and_label(Role::Button, row).is_some(),
            "{row}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In the Edit nodes tool the context menu is the tool bar's buttons for
/// the node under the pointer, which the right-click selects.
#[test]
fn the_node_context_menu_fillets() {
    let (mut h, dir) = plaque("node-menu");
    select(&mut h, &["i4"]);
    key(&mut h, egui::Key::N);
    let corners = square_corners(&h);
    let at = on_screen(&h, corners[2]);
    right_click(&mut h, at);
    assert_eq!(h.state().app().selected_nodes().len(), 1);
    // The bar has a Fillet too: the menu's is the last drawn.
    let fillet = h
        .query_all_by_role_and_label(Role::Button, "Fillet")
        .last()
        .expect("the menu has Fillet");
    fillet.click();
    h.run_steps(8);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Fillet corner of “square”")
    );
    assert_eq!(tool(&h), Tool::Nodes, "the tool stays armed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A drag with Alt held leaves a linked copy where it ends and the
/// original where it was (capability 98); Shape ▸ Make Unique then gives
/// the copy a shape of its own.
#[test]
fn alt_drag_copies_and_make_unique_unlinks() {
    let (mut h, dir) = plaque("alt-drag");
    select(&mut h, &["i4"]);
    let count = h.state().app().project().instances.len();
    let before = square_corners(&h);
    let from_mm = square_middle(&h);
    let from = on_screen(&h, from_mm);
    let to = on_screen(&h, Point::new(from_mm.x + 20.0, from_mm.y - 8.0));
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Copy “square”"));
    assert_eq!(app.project().instances.len(), count + 1);
    assert_eq!(square_corners(&h), before, "the original stays");
    let copy = app.project().instances.last().unwrap().clone();
    assert_eq!(copy.name, "square (2)");
    assert_eq!(copy.definition.as_str(), definition_of(&h, "i4"), "linked");
    assert_eq!(app.selection().instances(), std::slice::from_ref(&copy.id));

    // Make Unique, from the Shape menu.
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Make Unique").click();
    h.run();
    assert_eq!(
        h.state().app().undo_label(),
        Some("Make “square (2)” unique")
    );
    assert_ne!(definition_of(&h, copy.id.as_str()), definition_of(&h, "i4"));
    // Nothing is linked now: the row is off.
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    assert!(
        h.get_by_role_and_label(Role::Button, "Make Unique")
            .accesskit_node()
            .is_disabled()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
