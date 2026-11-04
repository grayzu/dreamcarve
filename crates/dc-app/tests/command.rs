// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → dc-app
//! core (slice 2)"): every command's result survives the file, the undo
//! labels name what changed (functional acceptance 86), and a locked
//! instance refuses every transform (acceptance 66).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::f64::consts::FRAC_PI_2;

use dc_app::test_support::Harness;
use dc_app::{Command, CommandError, Strings};
use dc_geometry::Transform;
use dc_project::{
    Heights, InstanceId, MachineOrigin, Operation, OperationId, OriginXy, OriginZ, ParkXy,
    PlacedTab, PostProcessorRef, Project, Stock, TabId, TabShape, Tool, ToolId, Units,
};

use common::{plaque, svg_drawing};

fn instance(id: &str) -> InstanceId {
    InstanceId::new(id)
}

fn op(id: &str) -> OperationId {
    OperationId::new(id)
}

fn operation<'a>(project: &'a Project, id: &str) -> &'a Operation {
    project
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .unwrap_or_else(|| panic!("plaque has operation {id}"))
}

fn tool<'a>(project: &'a Project, id: &str) -> &'a Tool {
    project
        .tools
        .iter()
        .find(|t| t.id.as_str() == id)
        .unwrap_or_else(|| panic!("plaque has tool {id}"))
}

/// The plaque's Cut out Profile with another depth: one field changed.
fn cut_out_at_depth(project: &Project, depth_mm: f64) -> Operation {
    match operation(project, "op4") {
        Operation::Profile(profile) => Operation::Profile(dc_project::ProfileOp {
            depth_mm: Some(depth_mm),
            ..profile.clone()
        }),
        other => panic!("op4 is a Profile, not {}", other.kind()),
    }
}

/// The plaque with its Cut out Profile's four tabs placed, so the tab
/// commands have tabs to work on.
fn plaque_with_tabs() -> Project {
    Command::PlaceTabs { op: op("op4") }
        .apply(&plaque())
        .expect("the plaque's tab request places")
}

/// A tab on the plaque's outline, for `AddTab`.
fn new_tab() -> PlacedTab {
    PlacedTab {
        id: TabId::new("tab-new"),
        instance: instance("i1"),
        profile: 0,
        x_mm: 20.0,
        y_mm: 0.0,
        width_mm: 8.0,
        height_mm: 3.0,
        shape: TabShape::Rect,
        holds_slug: None,
    }
}

/// The name of a `Command` variant, exhaustively: a variant added without
/// a case in [`every_command`] fails to compile here.
fn kind(command: &Command) -> &'static str {
    match command {
        Command::SetStock(_) => "SetStock",
        Command::SetOrigin(_) => "SetOrigin",
        Command::SetHeights(_) => "SetHeights",
        Command::SetTolerances { .. } => "SetTolerances",
        Command::SetDisplayUnits(_) => "SetDisplayUnits",
        Command::SwitchPost { .. } => "SwitchPost",
        Command::SetPostSettings(_) => "SetPostSettings",
        Command::AddTools(_) => "AddTools",
        Command::UpdateTool(_) => "UpdateTool",
        Command::RemoveTool(_) => "RemoveTool",
        Command::ImportDrawing { .. } => "ImportDrawing",
        Command::TransformInstances { .. } => "TransformInstances",
        Command::SetInstanceFlags { .. } => "SetInstanceFlags",
        Command::RenameInstance { .. } => "RenameInstance",
        Command::DuplicateInstances(_) => "DuplicateInstances",
        Command::DeleteInstances(_) => "DeleteInstances",
        Command::AddOperation(_) => "AddOperation",
        Command::UpdateOperation(_) => "UpdateOperation",
        Command::ReorderOperation { .. } => "ReorderOperation",
        Command::SetOperationEnabled { .. } => "SetOperationEnabled",
        Command::DuplicateOperation(_) => "DuplicateOperation",
        Command::DeleteOperation(_) => "DeleteOperation",
        Command::PlaceTabs { .. } => "PlaceTabs",
        Command::AddTab(_) => "AddTab",
        Command::MoveTab { .. } => "MoveTab",
        Command::UpdateTab(_) => "UpdateTab",
        Command::DeleteTabs(_) => "DeleteTabs",
        Command::CentreOnStock(_) => "CentreOnStock",
        Command::FitStockToShapes { .. } => "FitStockToShapes",
    }
}

/// One command of every variant, each with the project it applies to.
fn every_command() -> Vec<(Project, Command)> {
    let plaque = plaque();
    let with_tabs = plaque_with_tabs();
    let first_tab = with_tabs.tabs[0].clone();
    let post = &plaque.post_processor;
    let mut t9 = tool(&plaque, "t2").clone();
    t9.id = ToolId::new("t9");
    t9.name = "1/2 end mill".into();
    let renamed_t2 = Tool {
        name: "quarter inch".into(),
        ..tool(&plaque, "t2").clone()
    };
    let new_profile = match operation(&plaque, "op4") {
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
        (
            plaque.clone(),
            Command::SetStock(Stock {
                width_mm: 200.0,
                height_mm: 150.0,
                thickness_mm: Some(18.0),
            }),
        ),
        (
            plaque.clone(),
            Command::SetOrigin(MachineOrigin {
                xy: OriginXy::Centre,
                z: OriginZ::StockBottom,
            }),
        ),
        (
            plaque.clone(),
            Command::SetHeights(Heights {
                clearance_mm: Some(20.0),
                tool_change_machine_z_mm: None,
                park: Some(ParkXy {
                    x_mm: 0.0,
                    y_mm: 0.0,
                }),
            }),
        ),
        (
            plaque.clone(),
            Command::SetTolerances {
                flatten_mm: 0.02,
                arc_fit_mm: None,
            },
        ),
        (plaque.clone(), Command::SetDisplayUnits(Units::Inch)),
        (
            plaque.clone(),
            Command::SwitchPost {
                id: "grbl".to_owned(),
            },
        ),
        (
            plaque.clone(),
            Command::SetPostSettings(PostProcessorRef {
                file_extension: "ngc".into(),
                ..post.clone()
            }),
        ),
        (plaque.clone(), Command::AddTools(vec![t9])),
        (plaque.clone(), Command::UpdateTool(renamed_t2)),
        (plaque.clone(), Command::RemoveTool(ToolId::new("t1"))),
        (
            plaque.clone(),
            Command::ImportDrawing {
                drawing: svg_drawing("sign"),
                name: "sign.svg".into(),
            },
        ),
        (
            plaque.clone(),
            Command::TransformInstances {
                ids: vec![instance("i1")],
                delta: Transform::rotate(FRAC_PI_2),
            },
        ),
        (
            plaque.clone(),
            Command::SetInstanceFlags {
                ids: vec![instance("i4")],
                visible: Some(false),
                locked: Some(true),
            },
        ),
        (
            plaque.clone(),
            Command::RenameInstance {
                id: instance("i4"),
                name: "Square".into(),
            },
        ),
        (
            plaque.clone(),
            Command::DuplicateInstances(vec![instance("i4")]),
        ),
        (
            with_tabs.clone(),
            Command::DeleteInstances(vec![instance("i1")]),
        ),
        (plaque.clone(), Command::AddOperation(new_profile)),
        (
            plaque.clone(),
            Command::UpdateOperation(cut_out_at_depth(&plaque, 10.0)),
        ),
        (
            plaque.clone(),
            Command::ReorderOperation {
                id: op("op4"),
                to: 0,
            },
        ),
        (
            plaque.clone(),
            Command::SetOperationEnabled {
                id: op("op5"),
                enabled: false,
            },
        ),
        (plaque.clone(), Command::DuplicateOperation(op("op1"))),
        (plaque.clone(), Command::DeleteOperation(op("op3"))),
        (plaque.clone(), Command::PlaceTabs { op: op("op4") }),
        (plaque.clone(), Command::AddTab(new_tab())),
        (
            with_tabs.clone(),
            Command::MoveTab {
                id: first_tab.id.clone(),
                x_mm: first_tab.x_mm + 1.0,
                y_mm: first_tab.y_mm,
            },
        ),
        (
            with_tabs.clone(),
            Command::UpdateTab(PlacedTab {
                width_mm: 12.0,
                ..first_tab.clone()
            }),
        ),
        (with_tabs.clone(), Command::DeleteTabs(vec![first_tab.id])),
        (plaque.clone(), Command::CentreOnStock(vec![instance("i4")])),
        (plaque, Command::FitStockToShapes { margin_mm: 5.0 }),
    ]
}

/// Every variant, applied to the plaque, saved and loaded, is the project
/// `apply` returned, and each changed the project (a command that changes
/// nothing is not an undo step).
#[test]
fn every_command_round_trips_through_the_file() {
    let commands = every_command();
    let mut kinds: Vec<&str> = commands.iter().map(|(_, c)| kind(c)).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), commands.len(), "one command per variant");
    for (before, command) in &commands {
        let case = kind(command);
        let applied = command
            .apply(before)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_ne!(&applied, before, "{case}: changed nothing");
        let text = dc_project::save(&applied).unwrap_or_else(|e| panic!("{case}: {e}"));
        let loaded = dc_project::load(text.as_bytes())
            .unwrap_or_else(|e| panic!("{case}: {e}"))
            .project;
        assert_eq!(loaded, applied, "{case}: the file does not hold the result");
    }
}

/// The undo labels of functional capability 52: *Move "Oval"*, *Set depth
/// of "Outside cut"*, *Reorder operations*, *Import plaque.dxf*, *Place 4
/// tabs*; a transform is named for what it does; several instances are
/// counted.
#[test]
fn labels_name_the_thing() {
    let strings = Strings::default();
    let before = plaque();
    let label = |command: Command| {
        let after = command
            .apply(&before)
            .unwrap_or_else(|e| panic!("{}: {e}", kind(&command)));
        command.label(&strings, &before, &after)
    };
    let one = |delta: Transform| Command::TransformInstances {
        ids: vec![instance("i1")],
        delta,
    };
    assert_eq!(
        label(one(Transform::translate(10.0, 5.0))),
        "Move “outline”"
    );
    assert_eq!(label(one(Transform::rotate(FRAC_PI_2))), "Rotate “outline”");
    assert_eq!(label(one(Transform::scale(0.5, 0.5))), "Scale “outline”");
    assert_eq!(label(one(Transform::scale(-1.0, 1.0))), "Mirror “outline”");
    assert_eq!(
        label(Command::TransformInstances {
            ids: vec![instance("i1"), instance("i2")],
            delta: Transform::translate(1.0, 0.0),
        }),
        "Move 2 shapes"
    );
    assert_eq!(
        label(Command::UpdateOperation(cut_out_at_depth(&before, 10.0))),
        "Set depth of “Cut out”"
    );
    assert_eq!(
        label(Command::ReorderOperation {
            id: op("op4"),
            to: 0,
        }),
        "Reorder operations"
    );
    assert_eq!(
        label(Command::ImportDrawing {
            drawing: svg_drawing("plaque"),
            name: "plaque.dxf".into(),
        }),
        "Import plaque.dxf"
    );
    assert_eq!(label(Command::PlaceTabs { op: op("op4") }), "Place 4 tabs");
    assert_eq!(
        label(Command::DeleteInstances(vec![instance("i4")])),
        "Delete “square”"
    );
    assert_eq!(
        label(Command::SetInstanceFlags {
            ids: vec![instance("i4")],
            visible: Some(false),
            locked: None,
        }),
        "Hide “square”"
    );
    assert_eq!(
        label(Command::SetInstanceFlags {
            ids: vec![instance("i4"), instance("i5")],
            visible: None,
            locked: Some(true),
        }),
        "Lock 2 shapes"
    );
    assert_eq!(
        label(Command::RenameInstance {
            id: instance("i4"),
            name: "Square".into(),
        }),
        "Rename “square”"
    );
    assert_eq!(
        label(Command::DuplicateInstances(vec![instance("i4")])),
        "Duplicate “square”"
    );
    assert_eq!(
        label(Command::SetOperationEnabled {
            id: op("op5"),
            enabled: false,
        }),
        "Disable “Border and squiggle”"
    );
    assert_eq!(
        label(Command::DeleteOperation(op("op3"))),
        "Delete “Screw holes”"
    );
    assert_eq!(
        label(Command::RemoveTool(ToolId::new("t1"))),
        "Remove tool “Set me”"
    );
    assert_eq!(
        label(Command::CentreOnStock(vec![instance("i4")])),
        "Centre “square” on stock"
    );
    assert_eq!(
        label(Command::FitStockToShapes { margin_mm: 10.0 }),
        "Fit stock to shapes"
    );
    assert_eq!(
        label(Command::SwitchPost {
            id: "grbl".to_owned()
        }),
        "Switch post-processor to grbl"
    );
}

/// Acceptance 66: a locked instance refuses each transform with a message
/// naming it, and the project and the history are untouched by the refusal.
/// Unlocking it lets the transform through.
#[test]
fn locked_instances_refuse_transforms() {
    let mut harness = Harness::open(&common::plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![instance("i1")],
            visible: None,
            locked: Some(true),
        })
        .expect("locking is allowed");
    let locked = harness.app.project().clone();
    let transforms = [
        Command::TransformInstances {
            ids: vec![instance("i1"), instance("i2")],
            delta: Transform::translate(10.0, 5.0),
        },
        Command::TransformInstances {
            ids: vec![instance("i1")],
            delta: Transform::rotate(FRAC_PI_2),
        },
        Command::TransformInstances {
            ids: vec![instance("i1")],
            delta: Transform::scale(0.5, 0.5),
        },
        Command::TransformInstances {
            ids: vec![instance("i1")],
            delta: Transform::scale(-1.0, 1.0),
        },
        Command::CentreOnStock(vec![instance("i1")]),
    ];
    for command in transforms {
        let case = kind(&command);
        match harness.run(command) {
            Err(CommandError::Locked { names }) => {
                assert_eq!(names, vec!["outline".to_owned()], "{case}");
            }
            other => panic!("{case}: expected a locked refusal, got {other:?}"),
        }
        assert_eq!(harness.app.project(), &locked, "{case}: project changed");
        assert_eq!(
            harness.app.undo_label(),
            Some("Lock “outline”"),
            "{case}: the refusal is not an undo step"
        );
    }
    let message = CommandError::Locked {
        names: vec!["outline".to_owned()],
    }
    .to_string();
    assert!(message.contains("outline"), "{message}");

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![instance("i1")],
            visible: None,
            locked: Some(false),
        })
        .expect("unlocking is allowed");
    harness
        .run(Command::TransformInstances {
            ids: vec![instance("i1")],
            delta: Transform::translate(10.0, 5.0),
        })
        .expect("an unlocked instance moves");
}

/// Capability 59: a duplicate is a linked copy, sharing the definition,
/// named `<name> (2)`, offset by 10 mm; moving one leaves the other.
#[test]
fn duplicate_is_a_linked_copy_offset_by_10_mm() {
    let before = plaque();
    let after = Command::DuplicateInstances(vec![instance("i4")])
        .apply(&before)
        .expect("duplicates");
    assert_eq!(after.instances.len(), before.instances.len() + 1);
    assert_eq!(after.definitions.len(), before.definitions.len());
    let original = before
        .instances
        .iter()
        .find(|i| i.id.as_str() == "i4")
        .unwrap();
    let copy = after
        .instances
        .iter()
        .find(|i| i.name == "square (2)")
        .expect("the copy is named after the original");
    assert_eq!(copy.definition, original.definition);
    assert_ne!(copy.id, original.id);
    let expected = original.transform.then(Transform::translate(10.0, 10.0));
    for (k, (a, b)) in copy
        .transform
        .coeffs()
        .iter()
        .zip(expected.coeffs())
        .enumerate()
    {
        assert!((a - b).abs() < 1e-9, "coefficient {k}: {a} vs {b}");
    }
    let moved = Command::TransformInstances {
        ids: vec![copy.id.clone()],
        delta: Transform::translate(5.0, 0.0),
    }
    .apply(&after)
    .expect("moves");
    let still = moved
        .instances
        .iter()
        .find(|i| i.id.as_str() == "i4")
        .unwrap();
    assert_eq!(still.transform, original.transform, "the sibling moved");
}

/// Capability 59 and acceptance 68: deleting an instance removes it from
/// every operation's targets and takes its placed tabs and its unused
/// definition with it; the operation stays.
#[test]
fn delete_removes_targets_tabs_and_the_unused_definition() {
    let before = plaque_with_tabs();
    assert!(!before.tabs.is_empty(), "the plaque's tabs were placed");
    let after = Command::DeleteInstances(vec![instance("i1")])
        .apply(&before)
        .expect("deletes");
    assert!(after.instances.iter().all(|i| i.id.as_str() != "i1"));
    assert!(after.tabs.is_empty(), "the outline's tabs went with it");
    assert_eq!(after.operations.len(), before.operations.len());
    assert!(operation(&after, "op4").targets().is_empty());
    let d1 = before
        .instances
        .iter()
        .find(|i| i.id.as_str() == "i1")
        .unwrap();
    assert!(
        !after.definitions.contains_key(&d1.definition),
        "the definition no instance uses is dropped"
    );
}

/// Capability 63 (slice 8 refines it): a tool an operation uses cannot be
/// removed, and the refusal names the operations.
#[test]
fn remove_tool_in_use_is_refused_naming_operations() {
    match Command::RemoveTool(ToolId::new("t2")).apply(&plaque()) {
        Err(CommandError::ToolInUse { tool, operations }) => {
            assert_eq!(tool, "1/4 end mill");
            assert_eq!(operations, vec!["Panel", "Square recess", "Cut out"]);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}
