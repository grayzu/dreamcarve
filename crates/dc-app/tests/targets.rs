// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 27 of the phase 4 review redesign (`docs/specs/phase-4/design.md`,
//! "Review redesign slices"; #619; functional capability 67, acceptance
//! 98): targets on the drawing. Target editing is a mode: entering it on
//! an operation holds a draft of its targets, a click on a shape toggles
//! it, a marquee adds what it touches and an Alt-marquee removes, *Done*
//! writes the draft as one undo step and *Cancel* drops it. While it is
//! on, nothing else is selected or moved. Setup's Targets row summarises
//! the targets in one line.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::editor::targets;
use dc_app::scene::ShapeStyle;
use dc_app::test_support::Harness;
use dc_app::{Command, CommandError, Selection};
use dc_geometry::Transform;
use dc_project::{InstanceId, Operation, OperationId};

use common::plaque_bytes;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn panel() -> OperationId {
    OperationId::new("op1")
}

fn operation(harness: &Harness, op: &str) -> Operation {
    harness
        .app
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == op)
        .cloned()
        .unwrap_or_else(|| panic!("operation {op}"))
}

/// The names of `ids`, as the plaque names them.
fn names(harness: &Harness, ids: &[InstanceId]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            harness
                .app
                .project()
                .instances
                .iter()
                .find(|i| i.id == *id)
                .map_or_else(|| id.as_str().to_owned(), |i| i.name.clone())
        })
        .collect()
}

/// The draft's targets by name.
fn draft(harness: &Harness) -> Vec<String> {
    let ids = harness.app.target_editing().expect("editing").targets();
    names(harness, ids)
}

/// Acceptance 98: entering on the Panel, clicking the square then the
/// island, and *Done* emits one `UpdateOperation` whose targets are
/// `pocket, square`, labelled `Set targets of "Panel"`. The highlight
/// follows each click. *Cancel* emits nothing: the saved file is
/// byte-identical and there is nothing to undo.
#[test]
fn editing_toggles_and_commits_one_step() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let saved = harness.save().expect("saves");
    assert!(harness.app.target_editing().is_none());

    // Cancel: nothing.
    assert!(harness.app.edit_targets(&panel()));
    harness.app.toggle_target(&id("i4"));
    harness.app.toggle_target(&id("i3"));
    harness.app.cancel_targets();
    assert!(harness.app.target_editing().is_none());
    assert_eq!(harness.save().expect("saves"), saved, "byte-identical");
    assert!(!harness.app.can_undo(), "Cancel is no step");
    assert!(!harness.app.is_dirty());

    // Done: one step.
    assert!(harness.app.edit_targets(&panel()));
    let editing = harness.app.target_editing().expect("editing").clone();
    assert_eq!(editing.op(), &panel());
    assert_eq!(draft(&harness), ["pocket", "island"]);
    assert_eq!(
        editing.command(harness.app.project()).expect("no refusal"),
        None,
        "nothing toggled, nothing to commit"
    );

    harness.app.toggle_target(&id("i4"));
    assert_eq!(draft(&harness), ["pocket", "island", "square"]);
    // The highlight follows the click, before anything is written.
    let style = |harness: &Harness, i: &str| harness.app.scene().style(&id(i));
    assert_eq!(style(&harness, "i4"), ShapeStyle::Target { colour: 2 });
    assert_eq!(operation(&harness, "op1").targets(), [id("i2"), id("i3")]);

    harness.app.toggle_target(&id("i3"));
    assert_eq!(style(&harness, "i3"), ShapeStyle::Faded);
    let editing = harness.app.target_editing().expect("editing").clone();
    let command = editing
        .command(harness.app.project())
        .expect("no refusal")
        .expect("a change");
    let Command::UpdateOperation(updated) = &command else {
        panic!("Done is an UpdateOperation, not {command:?}");
    };
    assert_eq!(names(&harness, updated.targets()), ["pocket", "square"]);

    let label = harness.app.commit_targets().expect("commits");
    assert_eq!(label.as_deref(), Some("Set targets of “Panel”"));
    assert!(harness.app.target_editing().is_none(), "Done ends the mode");
    assert_eq!(operation(&harness, "op1").targets(), [id("i2"), id("i4")]);
    assert_eq!(
        harness.app.history().undo_labels().count(),
        1,
        "the whole edit is one step"
    );
    harness.app.undo();
    assert_eq!(harness.save().expect("saves"), saved);

    // Done with nothing changed is no step.
    assert!(harness.app.edit_targets(&panel()));
    harness.app.toggle_target(&id("i4"));
    harness.app.toggle_target(&id("i4"));
    assert_eq!(harness.app.commit_targets().expect("commits"), None);
    assert!(harness.app.target_editing().is_none());
    assert!(!harness.app.can_undo());

    // An operation the project lacks cannot be entered.
    assert!(!harness.app.edit_targets(&OperationId::new("nope")));
    assert!(harness.app.target_editing().is_none());
}

/// Capability 67: "nothing else can be selected or moved". While editing,
/// a click never changes the instance selection (the Viewport's click and
/// the tree's both go through `App::select`), and a drag emits no
/// `TransformInstances`: the app refuses every command and every undo
/// until *Done* or *Cancel*.
#[test]
fn editing_blocks_selection_and_transforms() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.app.select(Selection::Instances(vec![id("i1")]));
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i1")],
            delta: Transform::translate(1.0, 0.0),
        })
        .expect("moves before editing");
    let before = harness.app.project().clone();
    let selection = harness.app.selection().clone();
    assert!(harness.app.edit_targets(&panel()));

    // A click on the square, on nothing, and on another operation.
    for clicked in [
        Selection::Instances(vec![id("i4")]),
        Selection::Nothing,
        Selection::Operation(OperationId::new("op2")),
    ] {
        harness.app.select(clicked);
        assert_eq!(harness.app.selection(), &selection);
    }
    harness.app.edit(Some(OperationId::new("op2")));
    assert_eq!(harness.app.editing(), Some(&panel()), "the editor stays");

    // A drag.
    let moved = harness.run(Command::TransformInstances {
        ids: vec![id("i1")],
        delta: Transform::translate(5.0, 5.0),
    });
    assert!(
        matches!(moved, Err(CommandError::EditingTargets)),
        "{moved:?}"
    );
    assert_eq!(harness.app.project(), &before);
    assert_eq!(harness.app.undo(), None, "no undo under the draft");
    assert_eq!(harness.app.project(), &before);

    // The mode over, both work again.
    harness.app.cancel_targets();
    harness.app.select(Selection::Instances(vec![id("i4")]));
    assert_eq!(harness.app.selection().instances(), [id("i4")]);
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i4")],
            delta: Transform::translate(5.0, 5.0),
        })
        .expect("moves after editing");

    // Replacing the project ends the mode.
    assert!(harness.app.edit_targets(&panel()));
    harness.app.open(&plaque_bytes(), None).expect("reopens");
    assert!(harness.app.target_editing().is_none());
}

/// Capability 67: a marquee adds every shape it touches, after the
/// targets there are and without repeating one; an Alt-marquee removes
/// every shape it touches. One *Done* writes the lot.
#[test]
fn marquee_adds_alt_marquee_removes() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    assert!(harness.app.edit_targets(&panel()));

    // What a marquee round the square and the island takes.
    let scene = harness.app.scene();
    let (square, island) = (
        scene.shape(&id("i4")).expect("drawn").bounds.expect("box"),
        scene.shape(&id("i3")).expect("drawn").bounds.expect("box"),
    );
    let boxed =
        dc_app::viewport::pick::touching(scene, square.including(island.min).including(island.max));
    assert!(boxed.contains(&id("i4")) && boxed.contains(&id("i3")));

    harness.app.box_targets(&boxed, false);
    let mut want = vec![id("i2"), id("i3")];
    want.extend(
        boxed
            .iter()
            .filter(|i| ![id("i2"), id("i3")].contains(i))
            .cloned(),
    );
    assert_eq!(
        harness.app.target_editing().expect("editing").targets(),
        want,
        "added once each, after the old ones"
    );
    assert_eq!(
        harness.app.scene().style(&id("i4")),
        ShapeStyle::Target { colour: 2 }
    );

    // Alt: everything the box touched goes.
    harness.app.box_targets(&boxed, true);
    let left = draft(&harness);
    assert!(!left.contains(&"island".to_owned()) && !left.contains(&"square".to_owned()));
    assert_eq!(harness.app.scene().style(&id("i4")), ShapeStyle::Faded);
    assert!(!harness.app.can_undo(), "nothing is written before Done");

    harness.app.box_targets(&[id("i4")], false);
    let label = harness.app.commit_targets().expect("commits");
    assert_eq!(label.as_deref(), Some("Set targets of “Panel”"));
    assert_eq!(harness.app.history().undo_labels().count(), 1);
    let written = operation(&harness, "op1");
    assert!(written.targets().contains(&id("i4")));
    assert!(!written.targets().contains(&id("i3")));

    // Removing every target is accepted, as a hand-edited file is: the
    // Warnings panel has validation's `operation_has_no_targets`, an
    // error located at the targets.
    assert!(harness.app.edit_targets(&panel()));
    let all = operation(&harness, "op1").targets().to_vec();
    harness.app.box_targets(&all, true);
    harness.app.commit_targets().expect("commits");
    assert!(operation(&harness, "op1").targets().is_empty());
    assert!(
        harness
            .warnings()
            .iter()
            .any(|f| f.field.as_deref() == Some("operations[op1].targets"))
    );
}

/// Capability 67, "Setup's Targets row": the count and the names on one
/// line, `2 shapes · pocket, island`; the row carries the warning flag
/// when a target carries a warning, as a Pocket targeting the squiggle
/// (an open path it cannot clear) does. While its targets are being
/// edited the row reads the draft. *Use selection* replaces the targets
/// and *Add selection* appends, one step each.
#[test]
fn row_summary() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.generate();
    let summary =
        |harness: &Harness| targets::row_summary(&harness.app, &operation(harness, "op1"));
    let row = summary(&harness);
    assert_eq!(row.text, "2 shapes · pocket, island");
    assert_eq!(row.names, "pocket, island");
    assert!(!row.warning, "the plaque's Panel is clean");

    let recess = targets::row_summary(&harness.app, &operation(&harness, "op2"));
    assert_eq!(recess.text, "1 shape · square");

    // The draft, live.
    assert!(harness.app.edit_targets(&panel()));
    harness.app.toggle_target(&id("i10"));
    assert_eq!(
        summary(&harness).text,
        "3 shapes · pocket, island, squiggle"
    );
    harness.app.commit_targets().expect("commits");
    harness.generate();
    let row = summary(&harness);
    assert_eq!(row.text, "3 shapes · pocket, island, squiggle");
    assert!(
        row.warning,
        "the squiggle is an open path: {:?}",
        harness.warnings()
    );

    // *Use selection* and *Add selection*, each one step.
    let steps = harness.app.history().undo_labels().count();
    let used = targets::replace(&operation(&harness, "op1"), &[id("i4")])
        .expect("the file takes it")
        .expect("a change");
    assert_eq!(
        harness.run(used).expect("replaces"),
        "Set targets of “Panel”"
    );
    assert_eq!(summary(&harness).text, "1 shape · square");
    let added = targets::add(&operation(&harness, "op1"), &[id("i2"), id("i4")])
        .expect("the file takes it")
        .expect("a change");
    harness.run(added).expect("adds");
    assert_eq!(summary(&harness).text, "2 shapes · square, pocket");
    assert_eq!(harness.app.history().undo_labels().count(), steps + 2);

    let all = operation(&harness, "op1").targets().to_vec();
    let none = targets::remove(&operation(&harness, "op1"), &all)
        .expect("the file takes it")
        .expect("a change");
    harness.run(none).expect("removes");
    assert_eq!(summary(&harness).text, "No shapes");
}
