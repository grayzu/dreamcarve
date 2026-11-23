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
use dc_job::picture::PictureSize;
use dc_project::{
    BitmapId, Heights, InstanceId, MachineOrigin, Operation, OperationId, OriginXy, OriginZ,
    ParkXy, PlacedTab, PostProcessorRef, Project, Stock, TabId, TabShape, Tool, ToolId, Units,
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

/// *Add picture* on `tests/fixtures/photo/gradient.png` at 80 mm wide.
fn import_gradient() -> Command {
    let bytes = common::read(&common::fixture("photo/gradient.png"));
    let (pixels, _) =
        dc_import::bitmap::import(&bytes, dc_import::ImageFormat::Png).expect("it decodes");
    Command::ImportPicture {
        name: "gradient.png".into(),
        pixels: std::sync::Arc::new(pixels),
        size: PictureSize::Width { width_mm: 80.0 },
    }
}

/// The plaque with the gradient picture on it, as bitmap `b1`.
fn plaque_with_picture() -> Project {
    import_gradient()
        .apply(&plaque())
        .expect("the picture imports")
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
        Command::NewTool(_) => "NewTool",
        Command::UpdateTool(_) => "UpdateTool",
        Command::DeleteTool(_) => "DeleteTool",
        Command::ImportDrawing { .. } => "ImportDrawing",
        Command::ImportPicture { .. } => "ImportPicture",
        Command::SetBitmapFlags { .. } => "SetBitmapFlags",
        Command::RenameBitmap { .. } => "RenameBitmap",
        Command::DeleteBitmaps(_) => "DeleteBitmaps",
        Command::MoveBitmaps { .. } => "MoveBitmaps",
        Command::ResizeBitmap { .. } => "ResizeBitmap",
        Command::SetBitmapOpacity { .. } => "SetBitmapOpacity",
        Command::PlaceBitmap { .. } => "PlaceBitmap",
        Command::TransformInstances { .. } => "TransformInstances",
        Command::RotateInstances { .. } => "RotateInstances",
        Command::FlipInstances { .. } => "FlipInstances",
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
        Command::UpdateTabs(_) => "UpdateTabs",
        Command::DeleteTabs(_) => "DeleteTabs",
        Command::CentreOnStock(_) => "CentreOnStock",
        Command::FitStockToShapes { .. } => "FitStockToShapes",
        Command::AddShapes(_) => "AddShapes",
        Command::SetOutline { .. } => "SetOutline",
        Command::ConvertToPath(_) => "ConvertToPath",
        Command::ReplaceShapes { .. } => "ReplaceShapes",
        Command::CutInstances(_) => "CutInstances",
        Command::PasteShapes(_) => "PasteShapes",
        Command::MakeUnique(_) => "MakeUnique",
        Command::CopyInstances { .. } => "CopyInstances",
        Command::PlaceInstances { .. } => "PlaceInstances",
        Command::SetConstruction { .. } => "SetConstruction",
        Command::NewFolder { .. } => "NewFolder",
        Command::RenameFolder { .. } => "RenameFolder",
        Command::DeleteFolder(_) => "DeleteFolder",
        Command::Ungroup(_) => "Ungroup",
        Command::MoveRows { .. } => "MoveRows",
    }
}

/// A slot 40 × 12 mm, turned 30° and placed at (60, 40): a parametric
/// shape with the outline its sizes give (phase 5, ADR-0006).
fn new_slot(name: &str) -> dc_app::command::NewShape {
    let shape = dc_project::Shape::Slot {
        length_mm: 40.0,
        width_mm: 12.0,
    };
    dc_app::command::NewShape {
        name: name.to_owned(),
        profiles: shape.outline(0.01).unwrap().unwrap(),
        shape: Some(shape),
        transform: Transform::rotate(std::f64::consts::FRAC_PI_6)
            .then(Transform::translate(60.0, 40.0)),
        construction: false,
        folder: None,
    }
}

/// The drawing fixture's slot, `i9`, made 8 mm wide: a `SetOutline` with
/// the outline the new sizes give.
fn wider_slot() -> Command {
    let shape = dc_project::Shape::Slot {
        length_mm: 40.0,
        width_mm: 8.0,
    };
    Command::SetOutline {
        instance: instance("i9"),
        profiles: shape.outline(0.01).unwrap().unwrap(),
        shape: Some(shape),
        what: dc_app::command::Edit::Size(dc_app::tool::SizeField::SlotWidth),
    }
}

/// One command of every variant, each with the project it applies to.
fn every_command() -> Vec<(Project, Command)> {
    let drawing = dc_project::test_support::drawing_project();
    let plaque = plaque();
    let with_tabs = plaque_with_tabs();
    let with_picture = plaque_with_picture();
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
        (plaque.clone(), dc_app::tools::new_tool(&plaque)),
        (plaque.clone(), Command::UpdateTool(renamed_t2)),
        (plaque.clone(), Command::DeleteTool(ToolId::new("t1"))),
        (
            plaque.clone(),
            Command::ImportDrawing {
                drawing: svg_drawing("sign"),
                name: "sign.svg".into(),
            },
        ),
        (plaque.clone(), import_gradient()),
        (
            with_picture.clone(),
            Command::SetBitmapFlags {
                ids: vec![BitmapId::new("b1")],
                visible: Some(false),
                locked: Some(true),
            },
        ),
        (
            with_picture.clone(),
            Command::RenameBitmap {
                id: BitmapId::new("b1"),
                name: "ramp".into(),
            },
        ),
        (
            with_picture.clone(),
            Command::DeleteBitmaps(vec![BitmapId::new("b1")]),
        ),
        (
            with_picture.clone(),
            Command::MoveBitmaps {
                ids: vec![BitmapId::new("b1")],
                dx_mm: 12.345_678_9,
                dy_mm: -3.0,
            },
        ),
        (
            with_picture.clone(),
            Command::ResizeBitmap {
                id: BitmapId::new("b1"),
                size: PictureSize::Height { height_mm: 33.3 },
            },
        ),
        (
            with_picture.clone(),
            Command::SetBitmapOpacity {
                ids: vec![BitmapId::new("b1")],
                opacity_fraction: 0.4,
            },
        ),
        (
            with_picture,
            Command::PlaceBitmap {
                id: BitmapId::new("b1"),
                x_mm: -5.0,
                y_mm: 2.5,
                width_mm: 64.0,
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
            Command::RotateInstances {
                ids: vec![instance("i4")],
                angle_deg: -90.0,
            },
        ),
        (
            plaque.clone(),
            Command::FlipInstances {
                ids: vec![instance("i4")],
                flip: dc_app::arrange::Flip::Horizontal,
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
        (
            with_tabs.clone(),
            Command::UpdateTabs(
                with_tabs
                    .tabs
                    .iter()
                    .map(|t| PlacedTab {
                        shape: TabShape::Smooth,
                        ..t.clone()
                    })
                    .collect(),
            ),
        ),
        (with_tabs.clone(), Command::DeleteTabs(vec![first_tab.id])),
        (plaque.clone(), Command::CentreOnStock(vec![instance("i4")])),
        (plaque.clone(), Command::FitStockToShapes { margin_mm: 5.0 }),
        (plaque.clone(), Command::AddShapes(vec![new_slot("Slot 1")])),
        (plaque.clone(), Command::CutInstances(vec![instance("i4")])),
        (
            plaque.clone(),
            Command::PasteShapes(vec![dc_app::clipboard::Clip {
                definition: plaque.definitions[&plaque.instances[3].definition].clone(),
                instance: plaque.instances[3].clone(),
            }]),
        ),
        (
            Command::DuplicateInstances(vec![instance("i4")])
                .apply(&plaque)
                .expect("the square duplicates"),
            Command::MakeUnique(vec![instance("i4")]),
        ),
        (
            plaque.clone(),
            Command::SetConstruction {
                ids: vec![instance("i4")],
                construction: true,
            },
        ),
        (
            plaque.clone(),
            Command::NewFolder {
                name: "Folder 1".into(),
                ids: vec![instance("i4"), instance("i2")],
                group: false,
            },
        ),
        (
            drawing.clone(),
            Command::RenameFolder {
                id: dc_project::FolderId::new("f1"),
                name: "Feet".into(),
            },
        ),
        (
            drawing.clone(),
            Command::DeleteFolder(dc_project::FolderId::new("f1")),
        ),
        (
            drawing.clone(),
            Command::Ungroup(vec![dc_project::FolderId::new("f1")]),
        ),
        (
            plaque.clone(),
            Command::MoveRows {
                ids: vec![instance("i4")],
                before: Some(instance("i1")),
                folder: None,
            },
        ),
        (
            plaque.clone(),
            Command::PlaceInstances {
                moves: vec![(instance("i4"), Transform::translate(0.0, 3.0))],
                what: dc_app::command::Placing::Align,
            },
        ),
        (
            plaque,
            Command::CopyInstances {
                ids: vec![instance("i4")],
                placements: vec![Transform::translate(5.0, 0.0)],
                folder: Some("square array".into()),
            },
        ),
        (drawing.clone(), wider_slot()),
        (
            drawing.clone(),
            Command::ConvertToPath(vec![instance("i9")]),
        ),
        (
            drawing,
            Command::ReplaceShapes {
                from: vec![instance("i9")],
                keep: false,
                add: vec![new_slot("Slot 1"), new_slot("Slot 1 (2)")],
                inherit: vec![(instance("i9"), 0)],
                what: dc_app::command::Edit::Break,
            },
        ),
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
    // Phase 5: a drawn shape is named; several are counted.
    assert_eq!(
        label(Command::AddShapes(vec![new_slot("Slot 1")])),
        "Draw “Slot 1”"
    );
    assert_eq!(
        label(Command::AddShapes(vec![
            new_slot("Slot 1"),
            new_slot("Slot 2")
        ])),
        "Add 2 shapes"
    );
    // A size names the shape and the size; a conversion the shape, or
    // how many.
    let drawing = dc_project::test_support::drawing_project();
    let drawn = |command: Command| {
        let after = command.apply(&drawing).unwrap();
        command.label(&strings, &drawing, &after)
    };
    assert_eq!(drawn(wider_slot()), "Set width of “Slot 1”");
    assert_eq!(
        drawn(Command::ConvertToPath(vec![instance("i9")])),
        "Convert “Slot 1” to a path"
    );
    assert_eq!(
        drawn(Command::ConvertToPath(vec![instance("i2"), instance("i3")])),
        "Convert 2 shapes to paths"
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
        label(Command::DeleteTool(ToolId::new("t1"))),
        "Delete tool “Set me”"
    );
    assert_eq!(label(dc_app::tools::new_tool(&before)), "New tool “tool4”");
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

    // A picture's steps name the file it came from, then the bitmap.
    assert_eq!(label(import_gradient()), "Import gradient.png");
    let before = plaque_with_picture();
    let label = |command: Command| {
        let after = command
            .apply(&before)
            .unwrap_or_else(|e| panic!("{}: {e}", kind(&command)));
        command.label(&strings, &before, &after)
    };
    let b1 = || vec![BitmapId::new("b1")];
    let flags = |visible, locked| Command::SetBitmapFlags {
        ids: b1(),
        visible,
        locked,
    };
    assert_eq!(label(flags(Some(false), None)), "Hide “gradient”");
    assert_eq!(label(flags(None, Some(true))), "Lock “gradient”");
    assert_eq!(
        label(Command::RenameBitmap {
            id: BitmapId::new("b1"),
            name: "ramp".into(),
        }),
        "Rename “gradient”"
    );
    assert_eq!(label(Command::DeleteBitmaps(b1())), "Delete “gradient”");
    assert_eq!(
        label(Command::MoveBitmaps {
            ids: b1(),
            dx_mm: 5.0,
            dy_mm: 0.0,
        }),
        "Move “gradient”"
    );
    assert_eq!(
        label(Command::ResizeBitmap {
            id: BitmapId::new("b1"),
            size: PictureSize::Width { width_mm: 40.0 },
        }),
        "Resize “gradient”"
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
        Command::RotateInstances {
            ids: vec![instance("i1")],
            angle_deg: 30.0,
        },
        Command::FlipInstances {
            ids: vec![instance("i1")],
            flip: dc_app::arrange::Flip::Vertical,
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

/// Capability 59 for an Inlay's drawn boundary (format 26): deleting the
/// shape that draws it takes it off the Inlay's `boundary`, as it is
/// taken off a target list, so the file never names a shape it no longer
/// has (`inlay_boundary_unknown`); the Inlay stays, its targets untouched.
#[test]
fn delete_takes_a_shape_off_an_inlays_boundary() {
    let before = common::load_dcv(&common::fixture("dc-boundary.dcv"));
    let Operation::Inlay(drawn) = operation(&before, "op1") else {
        panic!("op1 is an Inlay");
    };
    assert_eq!(drawn.boundary, [instance("i3")]);
    let after = Command::DeleteInstances(vec![instance("i3")])
        .apply(&before)
        .expect("deletes");
    let Operation::Inlay(inlay) = operation(&after, "op1") else {
        panic!("still an Inlay");
    };
    assert!(inlay.boundary.is_empty(), "{:?}", inlay.boundary);
    assert_eq!(inlay.targets, drawn.targets);
    assert!(
        !dc_project::validate(&after)
            .iter()
            .any(|p| p.code() == "inlay_boundary_unknown")
    );
}

/// Capability 63 (slice 8 refines it): a tool an operation uses cannot be
/// removed, and the refusal names the operations.
#[test]
fn remove_tool_in_use_is_refused_naming_operations() {
    match Command::DeleteTool(ToolId::new("t2")).apply(&plaque()) {
        Err(CommandError::ToolInUse { tool, operations }) => {
            assert_eq!(tool, "1/4 end mill");
            assert_eq!(operations, vec!["Panel", "Square recess", "Cut out"]);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Phase 5, capability 90: `AddShapes` gives each shape the next free
/// definition and instance ids, at the end of the Design Tree, and
/// refuses what no project can hold: no shape, a shape with no outline,
/// an outline that is not a number, a folder that is not there.
#[test]
fn add_shapes_takes_the_next_ids_and_refuses_what_cannot_be() {
    use dc_app::CommandError;
    use dc_app::command::next_name;
    let before = plaque();
    let after = Command::AddShapes(vec![new_slot("Slot 1"), new_slot("Slot 2")])
        .apply(&before)
        .unwrap();
    assert_eq!(after.instances.len(), before.instances.len() + 2);
    let added = &after.instances[before.instances.len()..];
    // The plaque's ids run to d10 and i10.
    assert_eq!(added[0].id, instance("i11"));
    assert_eq!(added[1].id, instance("i12"));
    assert_eq!(added[0].definition, dc_project::DefinitionId::new("d11"));
    assert_eq!(added[1].definition, dc_project::DefinitionId::new("d12"));
    assert_eq!(after.definitions[&added[1].definition].name, "Slot 2");
    assert!(
        added
            .iter()
            .all(|i| i.visible && !i.locked && i.folder.is_none())
    );
    assert_eq!(dc_project::validate(&after), dc_project::validate(&before));

    let refuse = |shapes: Vec<dc_app::command::NewShape>| Command::AddShapes(shapes).apply(&before);
    assert!(matches!(refuse(Vec::new()), Err(CommandError::NothingToDo)));
    let mut empty = new_slot("Slot 1");
    empty.profiles.clear();
    assert!(matches!(
        refuse(vec![empty]),
        Err(CommandError::NothingToDo)
    ));
    let mut far = new_slot("Slot 1");
    far.transform = Transform::translate(1e9, 0.0);
    assert!(matches!(refuse(vec![far]), Err(CommandError::Geometry(_))));
    let mut filed = new_slot("Slot 1");
    filed.folder = Some(dc_project::FolderId::new("f9"));
    assert!(matches!(
        refuse(vec![filed]),
        Err(CommandError::UnknownFolder(_))
    ));

    // The next free number, by the names in use.
    assert_eq!(next_name(&before, "Slot"), "Slot 1");
    assert_eq!(next_name(&after, "Slot"), "Slot 3");
    assert_eq!(next_name(&after, "Circle"), "Circle 1");
}

/// `ReplaceShapes` (phase 5): the new shapes take the removed ones' place
/// in the Design Tree; what cut the inherited shape cuts the first new
/// one, its tabs go to that one's outline, the other removed shapes
/// leave every target list, and a locked shape is not replaced.
#[test]
fn replace_shapes_hands_targets_and_tabs_to_the_first() {
    use dc_app::CommandError;
    use dc_app::command::Edit;
    let before = plaque_with_tabs();
    // The Cut out Profile targets the outline, `i1`, which has the tabs.
    let cut_out = |project: &Project| match operation(project, "op4") {
        Operation::Profile(profile) => profile.targets.clone(),
        other => panic!("op4 is a Profile, not {}", other.kind()),
    };
    assert_eq!(cut_out(&before), [instance("i1")]);
    assert!(before.tabs.iter().all(|tab| tab.instance == instance("i1")));
    let at = before
        .instances
        .iter()
        .position(|i| i.id == instance("i1"))
        .unwrap();
    let command = Command::ReplaceShapes {
        from: vec![instance("i1"), instance("i4")],
        keep: false,
        add: vec![new_slot("Slot 1"), new_slot("Slot 2")],
        inherit: vec![(instance("i1"), 0)],
        what: Edit::JoinEnds,
    };
    let after = command.apply(&before).unwrap();
    assert_eq!(after.instances.len(), before.instances.len());
    let first = after.instances[at].id.clone();
    assert_eq!(
        after.instances[at].name, "Slot 1",
        "in the removed one's place"
    );
    assert_eq!(after.instances[at + 1].name, "Slot 2");
    assert!(!after.instances.iter().any(|i| i.id == instance("i1")));
    assert_eq!(cut_out(&after), std::slice::from_ref(&first));
    // Nothing targets a shape that is gone.
    for operation in &after.operations {
        let text = format!("{operation:?}");
        assert!(
            !text.contains("InstanceId(\"i4\")") && !text.contains("InstanceId(\"i1\")"),
            "{}",
            operation.name()
        );
    }
    assert_eq!(after.tabs.len(), before.tabs.len());
    let slot = &after.definitions[&after.instances[at].definition].profiles[0];
    for tab in &after.tabs {
        assert_eq!(tab.instance, first);
        let on = dc_geometry::nearest_point(slot, dc_geometry::Point::new(tab.x_mm, tab.y_mm));
        assert!(on.unwrap().gap_mm < 1e-9, "on the new outline");
    }
    assert!(
        !after
            .definitions
            .contains_key(&before.instances[at].definition)
    );
    assert_eq!(
        command.label(&Strings::default(), &before, &after),
        "Join 2 shapes"
    );

    let mut locked = before.clone();
    locked.instances[at].locked = true;
    assert!(matches!(
        command.apply(&locked),
        Err(CommandError::Locked { .. })
    ));
    // Kept, the sources stay as they are, locked or not, and the new
    // shapes stand after the last of them; nothing is retargeted.
    let kept = Command::ReplaceShapes {
        from: vec![instance("i1")],
        keep: true,
        add: vec![new_slot("Slot 1")],
        inherit: Vec::new(),
        what: Edit::Offset,
    };
    let beside = kept.apply(&locked).unwrap();
    assert_eq!(beside.instances.len(), before.instances.len() + 1);
    assert_eq!(beside.instances[at + 1].name, "Slot 1");
    assert_eq!(cut_out(&beside), [instance("i1")]);
    assert_eq!(beside.tabs, before.tabs);
    assert_eq!(
        kept.label(&Strings::default(), &locked, &beside),
        "Offset “outline”"
    );
    let nothing = Command::ReplaceShapes {
        from: Vec::new(),
        keep: false,
        add: vec![new_slot("Slot 1")],
        inherit: Vec::new(),
        what: Edit::Break,
    };
    assert!(matches!(
        nothing.apply(&before),
        Err(CommandError::NothingSelected)
    ));
}
