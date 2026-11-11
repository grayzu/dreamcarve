// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 5 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → Tree
//! and placement"; #534): the Design Tree's rows (functional capability
//! 57), its context menu's Duplicate and Delete (capability 59,
//! acceptance 67 and 68), and inline rename as one undo step
//! (`experience.md`, section 8, "Inline rename").

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::Command;
use dc_app::test_support::Harness;
use dc_app::tree::{Line, MenuItem, Section, ShapeGlyph, Tree};
use dc_geometry::Transform;
use dc_project::{InstanceId, OperationId};

use common::{instance_of, plaque, plaque_bytes};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

/// Capability 57: one row per instance in file order, each with its eye,
/// padlock, shape glyph (the plaque's squiggle is its one open path) and
/// *linked* badge (only rows whose definition another row shares); below,
/// the Tabs section groups placed tabs by the outline they sit on, and the
/// Pictures section is empty until phase 3.5. The section header's eye and
/// padlock read the rows' common state, mixed when they differ.
#[test]
fn rows_follow_file_order_with_badges() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::DuplicateInstances(vec![id("i4")]))
        .expect("duplicates");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i3")],
            visible: Some(false),
            locked: None,
        })
        .expect("hides");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i5")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    harness
        .run(Command::PlaceTabs {
            op: OperationId::new("op4"),
        })
        .expect("places the Cut out's tabs");
    let project = harness.app.project();
    let tree = Tree::build(project);

    let names: Vec<&str> = tree.shapes.iter().map(|r| r.name.as_str()).collect();
    let file_order: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, file_order, "rows are the instances in file order");
    assert_eq!(names.last(), Some(&"square (2)"), "the copy is appended");

    for row in &tree.shapes {
        let name = row.name.as_str();
        assert_eq!(
            row.glyph == ShapeGlyph::Open,
            name == "squiggle",
            "{name}: open-path glyph"
        );
        assert_eq!(
            row.linked,
            name == "square" || name == "square (2)",
            "{name}: linked badge"
        );
        assert_eq!(row.visible, name != "island", "{name}: eye");
        assert_eq!(row.locked, name == "holes-1", "{name}: padlock");
    }

    let header = tree.shapes_header();
    assert_eq!(header.visible, None, "one hidden row makes the eye mixed");
    assert_eq!(
        header.locked, None,
        "one locked row makes the padlock mixed"
    );

    assert_eq!(tree.tabs.len(), 1, "the tabs sit on one outline");
    let tabs = &tree.tabs[0];
    assert_eq!(tabs.instance, id("i1"));
    assert_eq!(tabs.name, "outline");
    assert_eq!(tabs.tabs.len(), project.tabs.len());
    assert!(!tabs.tabs.is_empty());
    assert!(tree.pictures.is_empty(), "no pictures before phase 3.5");

    // Every row shown and unlocked: the header reads so, not mixed.
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i3")],
            visible: Some(true),
            locked: None,
        })
        .expect("shows");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i5")],
            visible: None,
            locked: Some(false),
        })
        .expect("unlocks");
    let header = Tree::build(harness.app.project()).shapes_header();
    assert_eq!(header.visible, Some(true));
    assert_eq!(header.locked, Some(false));

    // The header's eye hides every row in one step; its padlock locks them.
    let all: Vec<InstanceId> = harness
        .app
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    let hide = dc_app::tree::toggle_visible(harness.app.project(), &all).expect("a command");
    harness.run(hide).expect("hides all");
    assert!(harness.app.project().instances.iter().all(|i| !i.visible));
    let lock = dc_app::tree::toggle_locked(harness.app.project(), &all).expect("a command");
    harness.run(lock).expect("locks all");
    assert!(harness.app.project().instances.iter().all(|i| i.locked));
}

/// Slice 25 (#617; capability 57, acceptance 96): every shape row leads
/// with a glyph saying what the shape is: the plaque's screw holes are
/// circles, its outline a closed outline, its squiggle an open path. No
/// row carries a text badge: the word "open" is in no row's model.
#[test]
fn rows_carry_a_shape_glyph() {
    let tree = Tree::build(&plaque());
    let glyph = |name: &str| {
        tree.shapes
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("a row for {name}"))
            .glyph
    };
    assert_eq!(glyph("holes-1"), ShapeGlyph::Circle);
    assert_eq!(glyph("outline"), ShapeGlyph::Closed);
    assert_eq!(glyph("squiggle"), ShapeGlyph::Open);
    assert_eq!(glyph("square"), ShapeGlyph::Closed);
    for row in &tree.shapes {
        let model = format!("{row:?}");
        assert!(
            !model.contains("open"),
            "{}: no text badge in the row model: {model}",
            row.name
        );
    }
}

/// Slice 25 (#617; capability 57, acceptance 96): an empty section is its
/// header alone. The plaque has no tabs placed and no pictures, so its
/// Tabs header is followed straight by the Pictures header, and that is
/// the tree's last line, with no *None* row after it.
#[test]
fn empty_sections_have_no_placeholder_row() {
    let tree = Tree::build(&plaque());
    let lines = tree.lines();
    assert!(matches!(lines.first(), Some(Line::Header(Section::Shapes))));
    let shapes = lines.iter().filter(|l| matches!(l, Line::Shape(_))).count();
    assert_eq!(shapes, tree.shapes.len(), "a line per shape");
    let pictures = lines
        .iter()
        .position(|l| matches!(l, Line::Header(Section::Pictures)))
        .expect("a Pictures header");
    assert_eq!(
        pictures + 1,
        lines.len(),
        "nothing follows the Pictures header"
    );
    let tabs = lines
        .iter()
        .position(|l| matches!(l, Line::Header(Section::Tabs)))
        .expect("a Tabs header");
    assert_eq!(tabs + 1, pictures, "the empty Tabs section is its header");
}

/// Capability 57: each row counts the operations targeting it, and the
/// hover lists them by name in project order. The plaque's Panel pocket
/// targets the pocket and the island; its Screw holes Bore the four holes.
#[test]
fn operations_targeting_each_instance() {
    let tree = Tree::build(&plaque());
    let listed = |name: &str| -> Vec<String> {
        tree.shapes
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("a row for {name}"))
            .operations
            .iter()
            .map(|o| o.name.clone())
            .collect()
    };
    assert_eq!(listed("outline"), ["Cut out"]);
    assert_eq!(listed("pocket"), ["Panel"]);
    assert_eq!(listed("island"), ["Panel"]);
    assert_eq!(listed("square"), ["Square recess"]);
    for hole in ["holes-1", "holes-2", "holes-3", "holes-4"] {
        assert_eq!(listed(hole), ["Screw holes"], "{hole}");
    }
    assert_eq!(listed("border"), ["Border and squiggle"]);
    assert_eq!(listed("squiggle"), ["Border and squiggle"]);

    // An instance no operation targets has a count of zero.
    let after = Command::DeleteOperation(OperationId::new("op2"))
        .apply(&plaque())
        .expect("deletes the Square recess");
    let tree = Tree::build(&after);
    let square = tree.shapes.iter().find(|r| r.name == "square").unwrap();
    assert!(square.operations.is_empty());
}

/// Acceptance 67 (capability 59): the context menu's Duplicate yields a
/// second instance with the same definition id; the saved file holds one
/// definition and two instances of it, both rows show the linked badge;
/// moving one leaves the other.
#[test]
fn duplicate_is_a_linked_copy() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let command =
        dc_app::tree::menu_command(harness.app.project(), MenuItem::Duplicate, &[id("i4")])
            .expect("Duplicate is a command");
    harness.run(command).expect("duplicates");
    let copy = harness
        .app
        .selection()
        .instances()
        .first()
        .cloned()
        .expect("the copy is selected");

    let saved = harness.save().expect("saves");
    let reloaded = dc_project::load(saved.as_bytes()).expect("reloads").project;
    assert_eq!(reloaded.definitions.len(), before.definitions.len());
    let d4 = &instance_of(&before, "i4").definition;
    let sharing: Vec<&str> = reloaded
        .instances
        .iter()
        .filter(|i| i.definition == *d4)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        sharing,
        ["square", "square (2)"],
        "one definition, two instances"
    );
    let tree = Tree::build(&reloaded);
    assert!(
        tree.shapes
            .iter()
            .filter(|r| r.linked)
            .map(|r| r.name.as_str())
            .eq(["square", "square (2)"]),
        "both rows are linked"
    );

    harness
        .run(Command::TransformInstances {
            ids: vec![copy.clone()],
            delta: Transform::translate(5.0, 0.0),
        })
        .expect("moves the copy");
    assert_eq!(
        instance_of(harness.app.project(), "i4").transform,
        instance_of(&before, "i4").transform,
        "the sibling stays"
    );
    assert_eq!(
        harness.app.project().definitions,
        before.definitions,
        "no definition is written"
    );
}

/// Acceptance 68 (capability 59): deleting the plaque's outline from the
/// context menu removes it from the Cut out Profile's targets, leaves the
/// Profile (enabled) with `operation_has_no_targets` in the Warnings panel,
/// and removes the outline's placed tabs; undo restores all three.
#[test]
fn delete_removes_targets_and_tabs_and_warns() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let op4 = OperationId::new("op4");
    harness
        .run(Command::PlaceTabs { op: op4.clone() })
        .expect("places tabs");
    let placed = harness.app.project().clone();
    assert!(!placed.tabs.is_empty());
    // Validation's `operation_has_no_targets`: an error, so the panel
    // prints its message without the code, located at the targets.
    let no_targets = |h: &Harness| {
        h.warnings().iter().any(|f| {
            f.field.as_deref() == Some("operations[op4].targets")
                && f.op.as_ref() == Some(&op4)
                && f.message.contains("Cut out")
        })
    };
    assert!(!no_targets(&harness));

    let command = dc_app::tree::menu_command(harness.app.project(), MenuItem::Delete, &[id("i1")])
        .expect("Delete is a command");
    harness.run(command).expect("deletes");
    let project = harness.app.project();
    let cut_out = project
        .operations
        .iter()
        .find(|o| *o.id() == op4)
        .expect("the operation stays");
    assert!(cut_out.targets().is_empty(), "its target went");
    assert!(cut_out.enabled(), "and it stays enabled");
    assert!(project.tabs.is_empty(), "the outline's tabs went with it");
    assert!(no_targets(&harness), "the Warnings panel says so");
    assert!(Tree::build(project).tabs.is_empty());

    assert_eq!(harness.app.undo().as_deref(), Some("Delete “outline”"));
    assert_eq!(harness.app.project(), &placed, "undo restores all three");
    assert!(!no_targets(&harness));
}

/// `experience.md`, section 8, "Inline rename": the typed name, trimmed,
/// renames the instance as one undo step; an empty or unchanged name is
/// no step at all.
#[test]
fn rename_is_one_step() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let project = harness.app.project().clone();
    assert_eq!(dc_app::tree::rename(&project, &id("i4"), "   "), None);
    assert_eq!(dc_app::tree::rename(&project, &id("i4"), "square"), None);
    assert_eq!(dc_app::tree::rename(&project, &id("i4"), " square "), None);
    let command = dc_app::tree::rename(&project, &id("i4"), "  Recess  ").expect("a rename");
    assert_eq!(
        command,
        Command::RenameInstance {
            id: id("i4"),
            name: "Recess".to_owned()
        }
    );
    let steps = harness.app.history().undo_labels().count();
    harness.run(command).expect("renames");
    assert_eq!(
        harness.app.history().undo_labels().count(),
        steps + 1,
        "one undo step"
    );
    assert_eq!(instance_of(harness.app.project(), "i4").name, "Recess");
    assert_eq!(harness.app.undo_label(), Some("Rename “square”"));
    harness.app.undo();
    assert_eq!(harness.app.project(), &project, "undo restores the name");
}

/// Slice 27 (#619; capabilities 57 and 67): while an operation is selected
/// its targets' rows are marked, in its colour. Selecting the plaque's
/// Panel marks `pocket` and `island` and no other row; with no operation
/// selected no row is marked. The context menu then offers *Add to
/// "Panel"* for rows that are not its targets and *Remove from "Panel"*
/// for rows that are, each one undo step.
#[test]
fn target_rows_marked_for_selected_operation() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let marked = |harness: &Harness| -> Vec<(String, usize)> {
        Tree::build(harness.app.project())
            .with_targets(harness.app.scene().targets())
            .shapes
            .iter()
            .filter_map(|r| Some((r.name.clone(), r.target?)))
            .collect()
    };
    assert!(marked(&harness).is_empty(), "no operation is selected");

    harness.app.edit(Some(OperationId::new("op1")));
    assert_eq!(
        marked(&harness),
        [("pocket".to_owned(), 2), ("island".to_owned(), 2)]
    );

    // The menu on the square: Add to; on the island: Remove from; on
    // both: both; with no operation selected: neither.
    let panel = |harness: &Harness| {
        harness
            .app
            .project()
            .operations
            .iter()
            .find(|o| o.id().as_str() == "op1")
            .cloned()
            .expect("the Panel")
    };
    let items = |harness: &Harness, ids: &[InstanceId]| {
        dc_app::tree::menu(harness.app.project(), ids, Some(&panel(harness)))
    };
    let square = items(&harness, &[id("i4")]);
    assert!(square.contains(&MenuItem::AddTo) && !square.contains(&MenuItem::RemoveFrom));
    let island = items(&harness, &[id("i3")]);
    assert!(island.contains(&MenuItem::RemoveFrom) && !island.contains(&MenuItem::AddTo));
    let both = items(&harness, &[id("i3"), id("i4")]);
    assert!(both.contains(&MenuItem::AddTo) && both.contains(&MenuItem::RemoveFrom));
    let none = dc_app::tree::menu(harness.app.project(), &[id("i4")], None);
    assert!(!none.contains(&MenuItem::AddTo) && !none.contains(&MenuItem::RemoveFrom));

    let add = dc_app::tree::target_command(&panel(&harness), MenuItem::AddTo, &[id("i4")])
        .expect("the file takes it")
        .expect("a change");
    assert_eq!(harness.run(add).expect("adds"), "Set targets of “Panel”");
    assert_eq!(
        marked(&harness),
        [
            ("pocket".to_owned(), 2),
            ("island".to_owned(), 2),
            ("square".to_owned(), 2)
        ]
    );
    let remove = dc_app::tree::target_command(&panel(&harness), MenuItem::RemoveFrom, &[id("i3")])
        .expect("the file takes it")
        .expect("a change");
    harness.run(remove).expect("removes");
    assert_eq!(
        marked(&harness),
        [("pocket".to_owned(), 2), ("square".to_owned(), 2)]
    );
    assert_eq!(harness.app.history().undo_labels().count(), 2);

    harness.app.edit(None);
    assert!(marked(&harness).is_empty());
}
