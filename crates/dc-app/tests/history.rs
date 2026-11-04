// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4: undo and redo walk the labelled snapshots
//! (functional acceptance 62 and 86).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::f64::consts::FRAC_PI_2;

use dc_app::Command;
use dc_app::test_support::Harness;
use dc_geometry::Transform;
use dc_project::{InstanceId, Operation, OperationId, Project, Stock, Units};

use common::{fixture, plaque_bytes, read, svg_drawing};

fn instance(id: &str) -> InstanceId {
    InstanceId::new(id)
}

fn op(id: &str) -> OperationId {
    OperationId::new(id)
}

/// Twenty edits of every family acceptance 86 lists: move, field, reorder,
/// add, delete, import, place tabs, and the flags, names and project
/// settings between them. Each is built against the project it will apply
/// to, so ids and names exist.
fn twenty_edits(project: &Project) -> Vec<Command> {
    let cut_out = project
        .operations
        .iter()
        .find(|o| o.id().as_str() == "op4")
        .expect("the plaque has op4");
    let deeper = match cut_out {
        Operation::Profile(profile) => Operation::Profile(dc_project::ProfileOp {
            depth_mm: Some(10.0),
            ..profile.clone()
        }),
        other => panic!("op4 is a Profile, not {}", other.kind()),
    };
    let second_cut = match cut_out {
        Operation::Profile(profile) => Operation::Profile(dc_project::ProfileOp {
            id: op("op9"),
            name: "Second cut".into(),
            targets: vec![instance("i4")],
            tabs: None,
            ..profile.clone()
        }),
        other => panic!("op4 is a Profile, not {}", other.kind()),
    };
    vec![
        Command::TransformInstances {
            ids: vec![instance("i1")],
            delta: Transform::translate(10.0, 5.0),
        },
        Command::UpdateOperation(deeper),
        Command::ReorderOperation {
            id: op("op4"),
            to: 0,
        },
        Command::AddOperation(second_cut),
        Command::DeleteOperation(op("op3")),
        Command::ImportDrawing {
            drawing: svg_drawing("sign"),
            name: "sign.svg".into(),
        },
        Command::PlaceTabs { op: op("op4") },
        Command::TransformInstances {
            ids: vec![instance("i2"), instance("i3")],
            delta: Transform::rotate(FRAC_PI_2),
        },
        Command::SetInstanceFlags {
            ids: vec![instance("i4")],
            visible: Some(false),
            locked: None,
        },
        Command::RenameInstance {
            id: instance("i4"),
            name: "Square".into(),
        },
        Command::DuplicateInstances(vec![instance("i4")]),
        Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(18.0),
        }),
        Command::SetDisplayUnits(Units::Inch),
        Command::SetOperationEnabled {
            id: op("op5"),
            enabled: false,
        },
        Command::DuplicateOperation(op("op1")),
        Command::TransformInstances {
            ids: vec![instance("i5")],
            delta: Transform::scale(0.5, 0.5),
        },
        Command::DeleteInstances(vec![instance("i10")]),
        Command::SetTolerances {
            flatten_mm: 0.02,
            arc_fit_mm: Some(0.02),
        },
        Command::CentreOnStock(vec![instance("i1")]),
        Command::FitStockToShapes { margin_mm: 10.0 },
    ]
}

/// Acceptance 86: twenty mixed edits undo in reverse order to the opening
/// state and redo to the final state, each with the labelled name; the
/// Edit menu's label is the step undo would take.
#[test]
fn twenty_mixed_edits_undo_and_redo_in_order() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let opening = harness.app.project().clone();
    let edits = twenty_edits(&opening);
    assert_eq!(edits.len(), 20);
    let mut snapshots = Vec::new();
    let mut labels = Vec::new();
    for (k, command) in edits.into_iter().enumerate() {
        snapshots.push(harness.app.project().clone());
        let label = harness
            .run(command)
            .unwrap_or_else(|e| panic!("edit {k}: {e}"));
        assert!(!label.is_empty(), "edit {k} has no label");
        labels.push(label);
    }
    let final_state = harness.app.project().clone();
    assert_ne!(final_state, opening);

    for k in (0..20).rev() {
        assert_eq!(
            harness.app.undo_label(),
            Some(labels[k].as_str()),
            "step {k}"
        );
        assert_eq!(harness.app.undo(), Some(labels[k].clone()), "step {k}");
        assert_eq!(harness.app.project(), &snapshots[k], "undo of step {k}");
        assert_eq!(
            harness.app.redo_label(),
            Some(labels[k].as_str()),
            "step {k}"
        );
    }
    assert_eq!(harness.app.project(), &opening);
    assert_eq!(harness.app.undo(), None);
    assert_eq!(harness.app.undo_label(), None);

    for k in 0..20 {
        assert_eq!(
            harness.app.redo(),
            Some(labels[k].clone()),
            "redo of step {k}"
        );
        let expected = snapshots.get(k + 1).unwrap_or(&final_state);
        assert_eq!(harness.app.project(), expected, "redo of step {k}");
    }
    assert_eq!(harness.app.project(), &final_state);
    assert_eq!(harness.app.redo(), None);
}

/// Acceptance 86: a new edit after undo clears redo.
#[test]
fn new_edit_clears_redo() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    harness
        .run(Command::RenameInstance {
            id: instance("i4"),
            name: "Square".into(),
        })
        .expect("rename");
    assert_eq!(harness.app.undo(), Some("Rename “square”".to_owned()));
    assert!(harness.app.can_redo());
    harness
        .run(Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(18.0),
        }))
        .expect("stock");
    assert!(!harness.app.can_redo());
    assert_eq!(harness.app.redo(), None);
    assert_eq!(harness.app.redo_label(), None);
    assert_eq!(harness.app.undo_label(), Some("Set stock"));
}

/// Acceptance 62: opening a file clears the history; the opened project is
/// the file's, and nothing of the previous project remains to undo to.
#[test]
fn open_clears_history() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    for command in twenty_edits(harness.app.project()).into_iter().take(3) {
        harness.run(command).expect("edit");
    }
    assert!(harness.app.can_undo());
    let sign = read(&fixture("sign.dcv"));
    harness.app.open(&sign, None).expect("the sign opens");
    assert!(!harness.app.can_undo());
    assert!(!harness.app.can_redo());
    assert_eq!(harness.app.undo(), None);
    assert_eq!(harness.app.redo(), None);
    assert_eq!(
        harness.app.project(),
        &common::load_dcv(&fixture("sign.dcv"))
    );
    assert!(!harness.app.is_dirty());
}
