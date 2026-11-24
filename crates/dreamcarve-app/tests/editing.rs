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
    for row in ["Convert to Path", "Fillet Corners…", "Tab Tool"] {
        assert!(
            h.query_all_by_role(Role::Button).any(|node| node
                .accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with(row))),
            "{row}"
        );
    }
    // Weld, Subtract and Intersect are rows of the menu and buttons of
    // the tool strip.
    for row in ["Weld", "Subtract", "Intersect"] {
        assert_eq!(
            h.query_all_by_role_and_label(Role::Button, row).count(),
            2,
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
    // The menu's row, which is not the strip's button of the same name.
    let in_strip = h.get_by_role_and_label(Role::Button, "Weld").rect();
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    h.query_all_by_role_and_label(Role::Button, "Weld")
        .find(|node| node.rect() != in_strip)
        .expect("Shape has Weld")
        .click();
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
    common::go_to(&mut h, "Design");
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

    // On nothing: the plaque's outline holds the whole stock, so above
    // it, clear of the tool strip at the pane's left.
    let at_mm = Point::new(85.0, 126.0);
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

/// Where instance `id` lies in the project.
fn box_of(h: &Harness<'_, Shell>, id: &str) -> dc_geometry::Rect {
    let project = h.state().app().project();
    let instance = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .unwrap();
    instance
        .transform
        .apply_polyline(&project.definitions[&instance.definition].profiles[0])
        .unwrap()
        .bounds()
        .unwrap()
}

/// Capability 98a in the window: the Properties panel's Align buttons
/// and the Arrange menu's rows. One shape aligns to the stock; several
/// align to their own box, or to the stock with *To stock* on;
/// Distribute shows with three shapes or more.
#[test]
fn align_and_distribute_from_properties_and_the_menu() {
    let (mut h, dir) = plaque("align");
    // The plaque opens on Export; Properties is the Design step's.
    common::go_to(&mut h, "Design");
    select(&mut h, &["i4"]);
    assert!(
        h.query_by_role_and_label(Role::Switch, "To stock")
            .is_none(),
        "one shape always aligns to the stock"
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Distribute horizontally by gaps")
            .is_none()
    );
    h.get_by_role_and_label(Role::Button, "Align left").click();
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Align “square”"));
    assert!(box_of(&h, "i4").min.x.abs() < 1e-9, "to the stock's left");

    // The four holes: to their own box, then to the stock.
    select(&mut h, &["i5", "i6", "i7", "i8"]);
    let top = ["i5", "i6", "i7", "i8"]
        .iter()
        .map(|id| box_of(&h, id).max.y)
        .fold(f64::MIN, f64::max);
    h.get_by_role_and_label(Role::Button, "Align top").click();
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Align 4 shapes"));
    for id in ["i5", "i6", "i7", "i8"] {
        assert!((box_of(&h, id).max.y - top).abs() < 1e-9, "{id}");
    }
    h.get_by_role_and_label(Role::Switch, "To stock").click();
    h.run();
    assert!(h.state().app().prefs().adjust.align_to_stock);
    h.get_by_role_and_label(Role::Button, "Align bottom")
        .click();
    h.run();
    for id in ["i5", "i6", "i7", "i8"] {
        assert!(box_of(&h, id).min.y.abs() < 1e-9, "{id}");
    }

    // Distribute, with three or more; and the Arrange menu's rows.
    assert!(
        h.query_by_role_and_label(Role::Button, "Distribute vertically by centres")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Arrange").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Distribute").click();
    h.run_steps(8);
    assert!(
        h.query_by_role_and_label(Role::Button, "Vertically by Centres")
            .is_some()
    );
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::Button, "Arrange").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Align").click();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Right").click();
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Align 4 shapes"));
    for id in ["i5", "i6", "i7", "i8"] {
        assert!((box_of(&h, id).max.x - 170.0).abs() < 1e-9, "{id}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Shapes section as the tree lists it: a group as `{name}`, or as
/// `[name]` when its shapes are selected one at a time; a row in a group
/// indented.
fn listed(h: &Harness<'_, Shell>) -> Vec<String> {
    use dc_app::tree::{Row, Tree};
    let tree = Tree::build(h.state().app().project());
    let mut out = Vec::new();
    for row in tree.rows() {
        match row {
            Row::Group(f) if f.together => out.push(format!("{{{}}}", f.name)),
            Row::Group(f) => out.push(format!("[{}]", f.name)),
            Row::Shape(s) if s.group.is_some() => out.push(format!("  {}", s.name)),
            Row::Shape(s) => out.push(s.name.clone()),
            _ => {}
        }
    }
    out
}

fn selected(h: &Harness<'_, Shell>) -> Vec<String> {
    h.state()
        .app()
        .selection()
        .instances()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect()
}

/// Capability 99 in the Design Tree: *New Group* files the selection;
/// the group's row folds its rows away, selects them, is renamed in
/// place, and its menu ungroups or deletes it.
#[test]
fn groups_in_the_design_tree() {
    let (mut h, dir) = plaque("groups");
    common::go_to(&mut h, "Design");
    select(&mut h, &["i5", "i6"]);
    h.get_by_role_and_label(
        Role::Button,
        "New group holding the selected shapes, or an empty one to drag shapes into",
    )
    .click();
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Group 2 shapes"));
    assert_eq!(
        listed(&h)[4..8],
        ["{Group 1}", "  holes-1", "  holes-2", "holes-3"]
    );
    assert!(
        h.query_by_role_and_label(Role::TreeItem, "Group 1")
            .is_some()
    );

    // Folded, its rows are not listed; unfolded, they are.
    h.get_by_role_and_label(Role::Button, "Collapse Group 1")
        .click();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::TreeItem, "holes-1")
            .is_none()
    );
    h.get_by_role_and_label(Role::Button, "Expand Group 1")
        .click();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::TreeItem, "holes-1")
            .is_some()
    );

    // A click on the group's row selects its shapes.
    select(&mut h, &["i1"]);
    h.get_by_role_and_label(Role::TreeItem, "Group 1").click();
    h.run();
    assert_eq!(selected(&h), ["i5", "i6"]);

    // Rename, from its menu, in place.
    h.get_by_role_and_label(Role::TreeItem, "Group 1")
        .click_secondary();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Rename").click();
    h.run_steps(8);
    h.get_by_label("Rename group Group 1").type_text("Feet");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Rename group “Group 1”"));
    assert_eq!(listed(&h)[4], "{Feet}");

    // Select Together, in the row's menu, is ticked for a new group: off,
    // a click on one of its shapes on the Viewport selects that one.
    h.get_by_role_and_label(Role::TreeItem, "Feet")
        .click_secondary();
    h.run_steps(8);
    h.get_by_label("Select Together").click();
    h.run_steps(8);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Select the shapes of “Feet” one at a time")
    );
    assert_eq!(listed(&h)[4], "[Feet]");
    assert!(!h.state().app().project().groups[0].together);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert!(h.state().app().project().groups[0].together);

    // Ungroup keeps the shapes; Delete takes them with the group.
    let count = h.state().app().project().instances.len();
    h.get_by_role_and_label(Role::TreeItem, "Feet")
        .click_secondary();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Ungroup").click();
    h.run_steps(8);
    assert!(h.state().app().project().groups.is_empty());
    assert_eq!(h.state().app().project().instances.len(), count);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    h.get_by_role_and_label(Role::TreeItem, "Feet")
        .click_secondary();
    h.run_steps(8);
    h.get_by_role_and_label(Role::Button, "Delete").click();
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Delete group “Feet”"));
    assert_eq!(h.state().app().project().instances.len(), count - 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Rows drag within the Design Tree (capability 99): onto a group to
/// file them, between rows to reorder, and past the last shape to take
/// one out.
#[test]
fn groups_drag_and_drop() {
    let (mut h, dir) = plaque("drag-rows");
    common::go_to(&mut h, "Design");
    select(&mut h, &["i5", "i6"]);
    h.get_by_role_and_label(
        Role::Button,
        "New group holding the selected shapes, or an empty one to drag shapes into",
    )
    .click();
    h.run();
    let row =
        |h: &Harness<'_, Shell>, name: &str| h.get_by_role_and_label(Role::TreeItem, name).rect();
    let upper = |rect: egui::Rect| egui::pos2(rect.center().x, rect.top() + 6.0);
    let lower = |rect: egui::Rect| egui::pos2(rect.center().x, rect.bottom() - 6.0);

    // The square, onto the group's lower half: filed in it, first.
    let from = row(&h, "square").center();
    let to = lower(row(&h, "Group 1"));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Move “square” to “Group 1”")
    );
    assert_eq!(
        listed(&h)[3..8],
        ["{Group 1}", "  square", "  holes-1", "  holes-2", "holes-3"]
    );

    // Between rows: the island, before the outline.
    let from = row(&h, "island").center();
    let to = upper(row(&h, "outline"));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(h.state().app().undo_label(), Some("Reorder “island”"));
    assert_eq!(listed(&h)[..3], ["island", "outline", "pocket"]);

    // Out of the group, to the end: onto the Tabs header. A selected
    // row takes the selection with it, here both holes.
    assert_eq!(selected(&h), ["i5", "i6"]);
    let from = row(&h, "holes-1").center();
    let to = h
        .get_by_role_and_label(Role::Heading, "Tabs")
        .rect()
        .center();
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(h.state().app().undo_label(), Some("Reorder 2 shapes"));
    let rows = listed(&h);
    assert_eq!(rows[rows.len() - 2..], ["holes-1", "holes-2"]);
    assert_eq!(rows[3..6], ["{Group 1}", "  square", "holes-3"]);

    // Let go where it was: nothing changes, and nothing is said.
    let steps = h.state().app().undo_label().map(str::to_owned);
    let said = h.state().message().map(str::to_owned);
    let from = row(&h, "pocket").center();
    let to = upper(row(&h, "pocket"));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    assert_eq!(h.state().app().undo_label().map(str::to_owned), steps);
    assert_eq!(h.state().message().map(str::to_owned), said);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A group is selected as one on the Viewport, and a double-click
/// reaches the one shape (capability 99); Arrange ▸ Ungroup dissolves it.
#[test]
fn a_group_is_selected_as_one() {
    let (mut h, dir) = plaque("group");
    select(&mut h, &["i5", "i6"]);
    h.get_by_role_and_label(Role::Button, "Arrange").click();
    h.run();
    let group = h
        .query_all_by_role(Role::Button)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Group"))
        })
        .expect("Arrange has Group");
    group.click();
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Group 2 shapes"));
    assert!(listed(&h).contains(&"{Group 1}".to_owned()));

    // A click on one hole selects both; a double-click, the one. The
    // menu's click is let grow old first: egui would count a third click
    // within its delay of two others as a triple-click.
    select(&mut h, &["i1"]);
    h.run_steps(30);
    // On the hole's own outline: the plaque's and the pocket's insides
    // hold its middle too.
    let hole = box_of(&h, "i5");
    let at = on_screen(
        &h,
        Point::new(hole.max.x, f64::midpoint(hole.min.y, hole.max.y)),
    );
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(selected(&h), ["i5", "i6"]);
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(selected(&h), ["i5"], "the double-click reaches the one");
    assert_eq!(tool(&h), Tool::Select);

    // Dragging one moves both.
    select(&mut h, &["i1"]);
    let before = box_of(&h, "i6");
    let to = egui::pos2(at.x + 40.0, at.y);
    // Frames far enough apart that the press is no double-click.
    h.run_steps(30);
    common::drag(&mut h, at, to, egui::PointerButton::Primary);
    assert_eq!(h.state().app().undo_label(), Some("Move 2 shapes"));
    assert!(box_of(&h, "i6").min.x > before.min.x + 1.0);

    select(&mut h, &["i5"]);
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::G,
    );
    h.run();
    assert_eq!(h.state().app().undo_label(), Some("Ungroup “Group 1”"));
    assert!(h.state().app().project().groups.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 98 in the window: Arrange ▸ Array… takes its values in the
/// tool bar and previews the copies; Done makes them, linked and in one
/// group. A circular array's centre is a click on the Viewport.
#[test]
fn an_array_from_the_arrange_menu() {
    let (mut h, dir) = plaque("array");
    select(&mut h, &["i4"]);
    let count = h.state().app().project().instances.len();
    let open = |h: &mut Harness<'_, Shell>| {
        h.get_by_role_and_label(Role::Button, "Arrange").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Array…").click();
        h.run_steps(8);
    };
    open(&mut h);
    assert_eq!(tool(&h), Tool::Adjust(Adjust::Array));
    for field in ["Columns", "Rows", "Spacing X", "Spacing Y"] {
        assert!(
            h.query_by_role_and_label(Role::TextInput, field).is_some(),
            "{field}"
        );
    }
    assert_eq!(h.state().app().draft().outlines.len(), 1, "one copy shown");
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Array “square”"));
    assert_eq!(app.project().instances.len(), count + 1);
    assert_eq!(tool(&h), Tool::Select);
    assert!(listed(&h).contains(&"{square array}".to_owned()));
    assert_eq!(
        definition_of(&h, "i4"),
        app.project().instances[4].definition.as_str(),
        "the copy is linked, and filed beside its source"
    );

    // Circular: the centre is clicked.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    select(&mut h, &["i4"]);
    open(&mut h);
    h.get_by_role_and_label(Role::RadioButton, "Circular")
        .click();
    h.run_steps(8);
    assert!(
        h.query_by_role_and_label(Role::Button, "Done").is_none(),
        "no centre yet"
    );
    let middle = square_middle(&h);
    let at = on_screen(&h, Point::new(middle.x + 30.0, middle.y));
    click_at(&mut h, at, egui::Modifiers::NONE);
    assert_eq!(h.state().app().draft().outlines.len(), 3);
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    assert_eq!(h.state().app().project().instances.len(), count + 3);
    assert_eq!(selected(&h).len(), 4, "the array is selected whole");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 100 in the window: the Properties panel's Construction
/// switch marks the selection, which leaves its operation; the Design
/// Tree's row says so; the row's menu and the Shape menu tick it; and a
/// drawing tool's bar has the switch for the shapes that follow.
#[test]
fn construction_geometry_in_the_window() {
    let (mut h, dir) = plaque("construction");
    common::go_to(&mut h, "Design");
    select(&mut h, &["i4"]);
    let targeted = |h: &Harness<'_, Shell>| {
        h.state()
            .app()
            .project()
            .operations
            .iter()
            .any(|op| op.targets().iter().any(|t| t.as_str() == "i4"))
    };
    assert!(targeted(&h));
    let switch = h.get_by_role_and_label(Role::Switch, "Construction");
    assert_eq!(
        switch.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    switch.click();
    h.run();
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Turn Construction on for “square”"));
    assert!(app.project().instances[3].construction);
    assert!(!targeted(&h), "it left the Square recess");
    assert_eq!(
        h.get_by_role_and_label(Role::Switch, "Construction")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::True)
    );

    // The row's menu: Construction is ticked; choosing it turns it off.
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_secondary();
    h.run_steps(8);
    let row = h.get_by_role_and_label(Role::Button, "Construction");
    assert_eq!(
        row.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    row.click();
    h.run_steps(8);
    assert!(!h.state().app().project().instances[3].construction);

    // The Shape menu's row turns it on again.
    h.get_by_role_and_label(Role::Button, "Shape").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Construction")
        .click();
    h.run_steps(8);
    assert!(h.state().app().project().instances[3].construction);

    // A drawing tool's bar: the shapes that follow are construction.
    key(&mut h, egui::Key::M);
    h.run_steps(8);
    h.get_by_role_and_label(Role::RadioButton, "On").click();
    h.run_steps(8);
    for (x, y) in [(150.0, 6.0), (160.0, 12.0)] {
        let at = on_screen(&h, Point::new(x, y));
        click_at(&mut h, at, egui::Modifiers::NONE);
    }
    let drawn = h.state().app().project().instances.last().unwrap();
    assert!(drawn.construction, "{}", drawn.name);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 91 in the window (acceptance 129): ⇧T takes up the Text
/// tool; a click places the text and the bar's field takes the words,
/// previewed as they are typed; Done makes the shape, whose words, font
/// and sizes Properties then show and edit.
#[test]
fn the_text_tool_previews_as_typed() {
    let (mut h, dir) = plaque("text-tool");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::T);
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Draw(dc_app::Drawing::Text));
    assert!(
        h.query_all_by_label("Click where the text starts")
            .next()
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::ComboBox, "Font").is_some(),
        "the Font list"
    );
    for field in ["Height", "Letter spacing", "Line spacing"] {
        assert!(
            h.query_by_role_and_label(Role::TextInput, field).is_some(),
            "{field}"
        );
    }
    let at = on_screen(&h, Point::new(20.0, 100.0));
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(8);
    let words = h.get_by_role_and_label(Role::MultilineTextInput, "Words");
    assert!(words.is_focused(), "the words field takes the keyboard");
    words.type_text("Hook");
    h.run_steps(8);
    assert!(
        !h.state().app().draft().outlines.is_empty(),
        "previewed as typed"
    );
    let count = h.state().app().project().instances.len();
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Draw “Text 1”"));
    assert_eq!(app.project().instances.len(), count + 1);

    // Put the tool down: the text is selected, and Properties has it.
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    assert_eq!(h.state().app().selection().instances().len(), 1);
    common::go_to(&mut h, "Design");
    let words = h.get_by_role_and_label(Role::MultilineTextInput, "Words");
    assert_eq!(words.accesskit_node().value().as_deref(), Some("Hook"));
    let height = h.get_by_role_and_label(Role::TextInput, "Height");
    height.focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("15".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run_steps(8);
    assert_eq!(h.state().app().undo_label(), Some("Set height of “Text 1”"));
    // The words, edited in place: written when the field is left.
    let words = h.get_by_role_and_label(Role::MultilineTextInput, "Words");
    words.focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("Hooks".to_owned()));
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(8);
    assert_eq!(
        h.state().app().undo_label(),
        Some("Edit the text of “Text 1”")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// *Add font…* (capability 91): the file is checked to be a font, copied
/// to the app's fonts folder and chosen; it is on hand again when the
/// app is next started. A file that is no font is refused, and not
/// copied.
#[test]
fn add_font_copies_the_file_and_chooses_it() {
    let dir = scratch("add-font");
    let config = dir.join("config");
    let font = common::repo_root()
        .join("crates/dreamcarve-app/assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
    let not_a_font = dir.join("notes.ttf");
    std::fs::write(&not_a_font, b"these are notes").unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe({
            let options = options(&config, None, &dialogs);
            move |cc| Shell::new(cc, options).expect("the shell starts")
        });
    h.run();
    common::settle(&mut h);
    assert_eq!(h.state().app().fonts().entries().len(), 8);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::T);
    h.run_steps(8);
    let add = |h: &mut Harness<'_, Shell>| {
        h.get_by_role_and_label(Role::ComboBox, "Font").click();
        h.run_steps(8);
        h.get_by_role_and_label(Role::Button, "Add font…")
            .scroll_to_me();
        h.run_steps(8);
        h.get_by_role_and_label(Role::Button, "Add font…").click();
        h.run_steps(8);
    };

    dialogs.answer(&not_a_font);
    add(&mut h);
    let said = h.state().message().unwrap_or_default().to_owned();
    assert!(
        said.starts_with("Cannot add notes.ttf: this is not a font file the app can read"),
        "{said}"
    );
    assert_eq!(h.state().app().fonts().entries().len(), 8);
    assert!(!config.join("fonts").join("notes.ttf").exists());

    dialogs.answer(&font);
    add(&mut h);
    assert_eq!(h.state().message(), Some("Added the font JetBrains Mono"));
    let app = h.state().app();
    assert_eq!(app.fonts().entries().len(), 9);
    assert_eq!(app.prefs().text.font.family, "JetBrains Mono", "chosen");
    assert!(
        config
            .join("fonts")
            .join("JetBrainsMono-Regular.ttf")
            .is_file()
    );
    assert!(dialogs.asked().iter().all(|asked| asked == "open_font"));
    drop(h);

    // The next start reads the folder.
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe({
            let options = options(&config, None, &dialogs);
            move |cc| Shell::new(cc, options).expect("the shell starts")
        });
    h.run();
    assert_eq!(h.state().app().fonts().entries().len(), 9);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Types `text` over what the field `label` holds, then `Enter`.
fn retype(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run_steps(8);
}

/// Slice 25 (capability 101, acceptance 126): a picture as a backdrop.
/// Its Properties fade it, and *Set scale…* scales it by two clicks on
/// the Viewport and the distance typed in the tool bar. Locked, a click
/// on it selects nothing.
#[test]
fn a_backdrop_is_faded_and_scaled_by_two_points() {
    let (mut h, dir) = plaque("backdrop");
    common::go_to(&mut h, "Design");
    let mut sheet = dc_app::import::PictureImport::open(
        dc_import::ImageFormat::Png,
        "gradient.png",
        &std::fs::read(common::fixture("photo/gradient.png")).unwrap(),
        dc_app::import::Purpose::Import,
    );
    sheet.set_width_mm(100.0);
    h.state_mut().app_mut().import_picture(&sheet).unwrap();
    h.run_steps(8);
    let footprint = |h: &Harness<'_, Shell>| {
        let bitmap = &h.state().app().project().bitmaps[0];
        (bitmap.x_mm, bitmap.y_mm, bitmap.width_mm, bitmap.height_mm)
    };
    let (x, y, width, height) = footprint(&h);
    assert_eq!((width, height), (100.0, 50.0));
    h.get_by_role_and_label(Role::TreeItem, "gradient").click();
    h.run_steps(8);

    // Opacity: a percentage, 100 until it is faded.
    let opacity = h.get_by_role_and_label(Role::TextInput, "Opacity");
    assert_eq!(opacity.accesskit_node().value().as_deref(), Some("100"));
    retype(&mut h, "Opacity", "40");
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Set opacity of “gradient”"));
    assert_eq!(app.project().bitmaps[0].opacity_fraction, 0.4);
    assert_eq!(app.scene().bitmaps()[0].opacity_fraction, 0.4);
    retype(&mut h, "Opacity", "5");
    assert_eq!(
        h.state().app().project().bitmaps[0].opacity_fraction,
        0.4,
        "under 10 % is refused"
    );

    // Set scale…: two points 40 mm apart that are really 80 apart.
    h.get_by_role_and_label(Role::Button, "Set scale…").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Adjust(Adjust::PictureScale));
    assert!(
        h.query_all_by_label("Click a point on the picture")
            .next()
            .is_some()
    );
    let first = Point::new(x + 20.0, y + 20.0);
    let at = on_screen(&h, first);
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(30);
    let at = on_screen(&h, Point::new(x + 60.0, y + 20.0));
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(8);
    assert_eq!(h.state().app().draft().outlines.len(), 1);
    let distance = h.get_by_role_and_label(Role::TextInput, "Distance");
    assert_eq!(distance.accesskit_node().value().as_deref(), Some("40"));
    retype(&mut h, "Distance", "80");
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run_steps(8);
    let app = h.state().app();
    assert_eq!(app.undo_label(), Some("Scale “gradient”"));
    assert_eq!(tool(&h), Tool::Select);
    // The clicks are whole pixels, so the points are 40 mm apart to a
    // few microns, and the picture twice its size to as much.
    let (to_x, to_y, to_width, to_height) = footprint(&h);
    for (got, wanted) in [
        (to_x, x - 20.0),
        (to_y, y - 20.0),
        (to_width, 200.0),
        (to_height, 100.0),
    ] {
        assert!((got - wanted).abs() < 1e-3, "{got} for {wanted}");
    }
    assert_eq!(app.selected_pictures().len(), 1, "still selected");

    // Locked, it is a backdrop: a click on it picks nothing, and its
    // row still selects it.
    let id = app.project().bitmaps[0].id.clone();
    let lock = dc_app::tree::toggle_bitmap_locked(app.project(), std::slice::from_ref(&id));
    h.state_mut().app_mut().run(lock.unwrap()).unwrap();
    h.state_mut().app_mut().select(Selection::Nothing);
    h.run_steps(30);
    let (x, y, ..) = footprint(&h);
    let at = on_screen(&h, Point::new(x + 2.0, y + 2.0));
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(8);
    assert!(h.state().app().selected_pictures().is_empty());
    h.get_by_role_and_label(Role::TreeItem, "gradient").click();
    h.run_steps(8);
    assert_eq!(h.state().app().selected_pictures(), [id]);
    assert!(
        h.get_by_role_and_label(Role::Button, "Set scale…")
            .accesskit_node()
            .is_disabled(),
        "a locked picture is not scaled"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Slice 26 (capability 103, acceptance 126): the Measure tool in the
/// window. `K` takes it up, two clicks lay a tape whose readings are in
/// the tool bar, the popover's row arms it too, and with a shape
/// selected the bar reads its length and area.
#[test]
fn measure_reads_a_tape_and_the_selected_shape() {
    let (mut h, dir) = plaque("measure");
    common::go_to(&mut h, "Design");
    h.state_mut().app_mut().select(Selection::Nothing);
    h.key_press(egui::Key::K);
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Measure);
    assert!(
        h.query_all_by_label("Click the point to measure from")
            .next()
            .is_some()
    );
    let steps = h.state().app().undo_label().map(str::to_owned);
    let at = on_screen(&h, Point::new(20.0, 100.0));
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(30);
    let at = on_screen(&h, Point::new(40.0, 100.0));
    click_at(&mut h, at, egui::Modifiers::NONE);
    h.run_steps(8);
    let read = h.state().app().measurement().expect("a tape is laid");
    assert!((read.distance_mm - 20.0).abs() < 0.01, "{read:?}");
    assert!(read.dy_mm.abs() < 0.01 && read.angle_deg.abs() < 0.1);
    for label in ["Distance", "ΔX", "ΔY", "Angle"] {
        assert!(
            h.query_all_by_label_contains(label).next().is_some(),
            "the bar reads {label}"
        );
    }
    assert_eq!(
        h.state().app().undo_label().map(str::to_owned),
        steps,
        "nothing is done to the project"
    );
    assert!(h.state().app().selection().instances().is_empty());

    // Escape puts the tool down; the popover's row takes it up again,
    // and a selected shape is read in the bar.
    key(&mut h, egui::Key::Escape);
    assert_eq!(tool(&h), Tool::Select);
    let first = h.state().app().project().instances[0].id.clone();
    h.state_mut()
        .app_mut()
        .select(Selection::Instances(vec![first.clone()]));
    h.get_by_role_and_label(Role::Button, "Measure").click();
    h.run_steps(8);
    assert_eq!(tool(&h), Tool::Measure);
    assert_eq!(h.state().app().selection().instances(), [first]);
    for label in ["Length", "Area"] {
        assert!(
            h.query_all_by_label_contains(label).next().is_some(),
            "the bar reads the shape's {label}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Review feedback, 2026-11-23: a shape is stretched one way in the
/// window by the handle at the middle of a side. The dragged side follows
/// the pointer, the far side stays, and the other axis is untouched; it
/// is one undo step.
#[test]
fn a_side_handle_stretches_a_shape_one_way() {
    let (mut h, dir) = plaque("side-handle");
    common::go_to(&mut h, "Design");
    select(&mut h, &["i4"]);
    let before = box_of(&h, "i4");
    let middle = f64::midpoint(before.min.y, before.max.y);
    let from = on_screen(&h, Point::new(before.max.x, middle));
    let to = on_screen(&h, Point::new(before.max.x + 12.0, middle + 3.0));
    common::drag(&mut h, from, to, egui::PointerButton::Primary);
    h.run_steps(8);
    let after = box_of(&h, "i4");
    assert!(
        (after.min.x - before.min.x).abs() < 1e-6,
        "the left side stays"
    );
    assert!(
        (after.max.x - before.max.x - 12.0).abs() < 0.3,
        "the right side follows the pointer: {after:?}"
    );
    assert!((after.min.y - before.min.y).abs() < 1e-6 && (after.max.y - before.max.y).abs() < 1e-6);
    let undo = h.state().app().undo_label().map(str::to_owned);
    assert!(
        undo.as_deref()
            .is_some_and(|label| label.contains("square")),
        "one step, named for the shape: {undo:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The plaque with a 40 mm bit of its own for *Square recess*, which no
/// longer fits the recess at all.
fn widen_the_recess_bit(h: &mut Harness<'_, Shell>) {
    let app = h.state_mut().app_mut();
    let project = app.project();
    let op = project
        .operations
        .iter()
        .find(|op| op.name() == "Square recess")
        .unwrap();
    let tool = project.tools.iter().find(|t| t.id == *op.tool()).unwrap();
    let wide = dc_project::Tool {
        id: dc_project::ToolId::new("t9"),
        name: "wide".to_owned(),
        number: Some(9),
        diameter_mm: 40.0,
        ..tool.clone()
    };
    let dc_project::Operation::Pocket(pocket) = op else {
        panic!("a pocket");
    };
    let mut pocket = pocket.clone();
    pocket.tool = wide.id.clone();
    app.run(dc_app::Command::AddTools(vec![wide])).unwrap();
    app.run(dc_app::Command::UpdateOperation(
        dc_project::Operation::Pocket(pocket),
    ))
    .unwrap();
}

/// Review feedback, 2026-11-23: an operation left with nothing to cut by
/// its tool says so in its row and at the top of its editor, as an
/// alert, with the reason.
#[test]
fn an_operation_that_cuts_nothing_is_announced() {
    let (mut h, dir) = plaque("cuts-nothing");
    widen_the_recess_bit(&mut h);
    common::settle(&mut h);
    common::go_to(&mut h, "Operations");
    common::open_operation(&mut h, "Square recess");
    h.run_steps(8);
    let alert = h
        .query_all_by_role(Role::Alert)
        .find_map(|node| node.accesskit_node().label())
        .expect("the editor has an alert");
    assert!(
        alert.starts_with("This operation cuts nothing with this tool."),
        "{alert}"
    );
    assert!(alert.contains("wide"), "and why: {alert}");
    let row = dc_app::operations::rows(h.state().app())
        .into_iter()
        .find(|row| row.name == "Square recess")
        .unwrap();
    assert!(row.cuts_nothing && row.detail.starts_with("Cuts nothing"));
    let _ = std::fs::remove_dir_all(dir);
}

/// The maintainer's review of 2026-11-23: the origin of a selection, the
/// point changes to it are made about, is chosen in Properties from nine
/// points of its box. A typed width then grows the shape away from it,
/// and a turn from the Arrange menu swings the shape round it.
#[test]
fn the_origin_is_chosen_and_changes_are_about_it() {
    use dc_app::origin::Origin;
    let (mut h, dir) = plaque("origin");
    common::go_to(&mut h, "Design");
    select(&mut h, &["i4"]);
    h.run_steps(8);
    let before = box_of(&h, "i4");
    assert_eq!(h.state().app().prefs().origin, Origin::Centre);
    let centre = h.get_by_role_and_label(Role::RadioButton, "Origin: Centre");
    assert_eq!(
        centre.accesskit_node().toggled(),
        Some(eframe::egui::accesskit::Toggled::True)
    );
    h.get_by_role_and_label(Role::RadioButton, "Origin: Bottom left")
        .click();
    h.run_steps(8);
    assert_eq!(h.state().app().prefs().origin, Origin::BottomLeft);
    assert!(
        h.state().app().undo_label() != Some("Origin"),
        "choosing it is no change to the project"
    );

    // A typed width, with the proportions kept: the corner stays.
    retype(&mut h, "W", "60");
    let wider = box_of(&h, "i4");
    assert!((wider.min.x - before.min.x).abs() < 1e-6 && (wider.min.y - before.min.y).abs() < 1e-6);
    assert!((wider.width_mm() - 60.0).abs() < 1e-6, "{wider:?}");
    h.state_mut().app_mut().undo();
    h.run_steps(8);

    // A quarter turn anticlockwise from the Arrange menu: the square
    // lands to the left of the corner it turned about.
    h.get_by_role_and_label(Role::Button, "Arrange").click();
    h.run();
    h.query_all_by_role(Role::Button)
        .find(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Rotate 90° Anticlockwise"))
        })
        .expect("Arrange has the turn")
        .click();
    h.run_steps(8);
    let turned = box_of(&h, "i4");
    assert!(
        (turned.max.x - before.min.x).abs() < 1e-6 && (turned.min.y - before.min.y).abs() < 1e-6,
        "swung round its bottom-left corner: {turned:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
