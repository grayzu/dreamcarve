// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 10 of phase 4 (#539): the Operations list (functional spec,
//! capability 65; acceptance 72 and 74; `docs/specs/phase-4/design.md`,
//! "Test plan → Operations and editors"). *Add* is partitioned by what the
//! selection can take, as PureCutCNC's `OperationAddMenu.tsx` partitions
//! it (behaviour only); a new operation targets the selection, takes the
//! first project tool that fits and the CLI's defaults; reorder, enable,
//! duplicate and delete are one step each, and delete keeps placed tabs
//! (ADR-0005).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::editor::fields;
use dc_app::operations::{self, Kind, Status};
use dc_app::test_support::Harness;
use dc_app::{Command, Selection, StrKey, Strings};
use dc_project::{InstanceId, Operation, OperationId, Project, Tool, ToolId, ToolKind, Units};
use serde_json::Value;

/// `ids` as instance ids.
fn ids(ids: &[&str]) -> Vec<InstanceId> {
    ids.iter().map(|id| InstanceId::new(*id)).collect()
}

/// The coaster as the app's *New from drawing* makes it (no tool, no
/// operation: `with_defaults: false`), with a 6.35 mm end mill added, and
/// the same drawing as `dreamcarve init` writes it.
fn coaster() -> (Project, Project) {
    let post = dc_project::latest_definition("grblhal")
        .unwrap()
        .default_ref();
    let drawing = || {
        let bytes = common::read(&common::repo_root().join("fixtures/coaster.svg"));
        let (import, _) =
            dc_import::import_svg(&bytes, &dc_import::SvgImportOptions::default()).unwrap();
        dc_job::Drawing::Svg(import)
    };
    let app = dc_job::project_from::drawing("coaster", drawing(), post.clone(), Units::Mm, false)
        .unwrap();
    let init = dc_job::project_from::drawing("coaster", drawing(), post, Units::Mm, true).unwrap();
    (app, init)
}

/// An end mill, as the Tool Library would add it.
fn end_mill(id: &str) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: "1/4 end mill".into(),
        vendor: None,
        product_id: None,
        number: None,
        cutter: ToolKind::EndMill,
        diameter_mm: 6.35,
        flutes: 2,
        feed_mm_min: None,
        plunge_mm_min: None,
        rpm: None,
        step_down_mm: None,
        step_over_fraction: None,
    }
}

/// The operation `id` of `harness`'s project.
fn op<'a>(harness: &'a Harness, id: &str) -> &'a Operation {
    harness
        .app
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .unwrap_or_else(|| panic!("no operation {id}"))
}

/// Acceptance 72 (capability 65): with the coaster selected, *Add ▸
/// Profile* appends a Profile targeting it with `init`'s Profile defaults
/// (the same record `dreamcarve init` writes, but for its id, name, tool
/// and targets), the project's end mill and the name `Profile 1`, as one
/// undo step that selects it for the editor; a second is `Profile 2`. With
/// nothing selected the menu says to select shapes and offers nothing;
/// with an open path selected, Pocket is below the divider with its
/// reason. A new operation of every kind takes its kind's fitting tool,
/// or none, and no depth of the app's own (S20).
#[test]
fn add_targets_the_selection_with_cli_defaults() {
    let strings = Strings::english();
    let (project, init) = coaster();
    let mut harness = Harness::from_project(project);
    harness
        .run(Command::AddTools(vec![end_mill("t1")]))
        .unwrap();
    let coaster: Vec<InstanceId> = harness
        .app
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    harness.app.select(Selection::Instances(coaster.clone()));

    let menu = operations::add_menu(harness.app.project(), &strings, &coaster);
    assert_eq!(menu.prompt, None);
    assert!(menu.available.contains(&Kind::Profile), "{menu:?}");
    let command = operations::add(harness.app.project(), &strings, Kind::Profile, &coaster)
        .expect("the coaster can take a Profile");
    let label = harness.run(command).unwrap();
    assert_eq!(label, "Add “Profile 1”");
    assert_eq!(
        harness.app.history().undo_labels().count(),
        2,
        "one step for the add"
    );

    let Operation::Profile(added) = harness.app.project().operations[0].clone() else {
        panic!("a Profile");
    };
    let Operation::Profile(mut expected) = init.operations[0].clone() else {
        panic!("init writes a Profile");
    };
    expected.name = "Profile 1".into();
    expected.targets.clone_from(&coaster);
    expected.tool = ToolId::new("t1");
    assert_eq!(
        added, expected,
        "init's Profile, the selection, the end mill"
    );
    assert_eq!(added.depth_mm, None, "no depth of the app's own (S20)");
    assert_eq!(
        harness.app.editing(),
        Some(&added.id),
        "the new operation is the editor's"
    );

    let again = operations::add(harness.app.project(), &strings, Kind::Profile, &coaster).unwrap();
    harness.run(again).unwrap();
    assert_eq!(harness.app.project().operations[1].name(), "Profile 2");
    assert_ne!(
        harness.app.project().operations[1].id(),
        harness.app.project().operations[0].id()
    );

    // Nothing selected: one sentence, nothing offered.
    let empty = operations::add_menu(harness.app.project(), &strings, &[]);
    assert_eq!(
        empty.prompt.as_deref(),
        Some(strings.get(StrKey::OperationsAddSelectShapes))
    );
    assert!(empty.available.is_empty());
    assert_eq!(empty.unavailable.len(), Kind::ALL.len());
    assert!(operations::add(harness.app.project(), &strings, Kind::Profile, &[]).is_none());

    // An open path: Pocket is below the divider, with its reason.
    let plaque = common::plaque();
    let squiggle = ids(&["i10"]);
    let menu = operations::add_menu(&plaque, &strings, &squiggle);
    let (_, reason) = menu
        .unavailable
        .iter()
        .find(|(kind, _)| *kind == Kind::Pocket)
        .expect("an open path cannot take a Pocket");
    assert_eq!(reason.as_deref(), Some("An open path cannot take a Pocket"));
    assert!(operations::add(&plaque, &strings, Kind::Pocket, &squiggle).is_none());

    // Each kind's tool: the sign has end mills, a V-bit; no drill.
    let sign = common::load_dcv(&common::fixture("sign.dcv"));
    let letters = ids(&["i6"]);
    let tool_of = |kind| {
        operations::new_operation(&sign, &strings, kind, &letters)
            .map(|op| op.tool().as_str().to_owned())
    };
    assert_eq!(tool_of(Kind::VCarve).as_deref(), Some("t4"), "the V-bit");
    assert_eq!(tool_of(Kind::Inlay).as_deref(), Some("t4"), "the V-bit");
    assert_eq!(
        tool_of(Kind::Drill).as_deref(),
        Some("t1"),
        "the first end mill"
    );
    assert_eq!(
        tool_of(Kind::Pocket).as_deref(),
        Some("t1"),
        "the first end mill"
    );
    assert_eq!(
        tool_of(Kind::PhotoVCarve),
        None,
        "no Photo V-Carve before 3.4"
    );
    // The plaque has no V-bit: a V-Carve takes none, and validation says so.
    let carve = operations::new_operation(&plaque, &strings, Kind::VCarve, &ids(&["i4"])).unwrap();
    assert_eq!(carve.tool().as_str(), "");
    assert_eq!(operations::fitting_tool(&plaque, Kind::VCarve), None);

    // No kind's new operation has a depth of the app's own (S20).
    for kind in Kind::ALL {
        let Some(new) = operations::new_operation(&sign, &strings, kind, &letters) else {
            continue;
        };
        assert!(new.enabled(), "{kind:?} starts enabled");
        assert_eq!(
            new.targets(),
            letters.as_slice(),
            "{kind:?} targets the selection"
        );
        let record = fields::record(&new).unwrap();
        for depth in ["depth_mm", "flat_depth_mm", "glue_gap_mm", "plug_depth_mm"] {
            if let Some(value) = fields::get(&record, depth) {
                assert_eq!(value, &Value::Null, "{kind:?}'s {depth} is the user's");
            }
        }
        assert_eq!(new.depth_mm(), None, "{kind:?} has no depth of its own");
    }
}

/// Capability 65 (PureCutCNC's `OperationAddMenu.tsx`, behaviour only):
/// the kinds the selection can take come first, in the list's order, and
/// the rest below the divider each with the reason it cannot: an open path
/// takes no Pocket, V-Carve or Inlay; a shape that is not a circle takes
/// no Drill or Bore; a Photo V-Carve needs a picture. A kind is offered
/// when every selected shape can take it.
#[test]
fn add_menu_partitions_by_eligibility() {
    let strings = Strings::english();
    let plaque = common::plaque();
    let available = |selected: &[&str]| operations::add_menu(&plaque, &strings, &ids(selected));

    // Circles take every kind with a record.
    let holes = available(&["i5", "i6"]);
    assert_eq!(
        holes.available,
        [
            Kind::Profile,
            Kind::Pocket,
            Kind::Drill,
            Kind::Bore,
            Kind::Engrave,
            Kind::VCarve,
            Kind::Inlay,
        ]
    );
    assert_eq!(
        holes.unavailable,
        [(
            Kind::PhotoVCarve,
            Some("A Photo V-Carve needs a picture".to_owned())
        )]
    );

    // A square is closed but no circle.
    let square = available(&["i4"]);
    assert_eq!(
        square.available,
        [
            Kind::Profile,
            Kind::Pocket,
            Kind::Engrave,
            Kind::VCarve,
            Kind::Inlay,
        ]
    );
    let reasons: Vec<(Kind, Option<String>)> = square.unavailable.clone();
    assert_eq!(
        reasons[..2],
        [
            (Kind::Drill, Some("A Drill needs circles".to_owned())),
            (Kind::Bore, Some("A Bore needs circles".to_owned())),
        ]
    );

    // An open path takes a Profile and an Engrave only.
    let squiggle = available(&["i10"]);
    assert_eq!(squiggle.available, [Kind::Profile, Kind::Engrave]);
    let below: Vec<Kind> = squiggle.unavailable.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        below,
        [
            Kind::Pocket,
            Kind::Drill,
            Kind::Bore,
            Kind::VCarve,
            Kind::Inlay,
            Kind::PhotoVCarve,
        ]
    );
    assert!(squiggle.unavailable.iter().all(|(_, r)| r.is_some()));

    // Mixed: one open path among circles is enough to refuse a Pocket.
    let mixed = available(&["i5", "i10"]);
    assert!(!mixed.available.contains(&Kind::Pocket));
    assert!(!mixed.available.contains(&Kind::Drill));
    assert!(mixed.available.contains(&Kind::Engrave));
}

/// Acceptance 74 (capability 65): dragging the plaque's Bore below its
/// Panel pocket reorders the file's list, one undo step labelled *Reorder
/// operations*, and the programs follow it; dropping a row where it is is
/// no step. Unticking an operation writes `enabled: false`, takes its
/// toolpath off the Viewport's Scene and its runs out of the programs;
/// *Duplicate* copies it below itself with its targets and `(2)` in its
/// name. The list's rows say each operation's kind, tool and state; the
/// eye is view state, not an undo step (acceptance 81).
#[test]
fn reorder_enable_duplicate() {
    let mut harness = Harness::open(&common::plaque_bytes()).unwrap();
    let order = |h: &Harness| -> Vec<String> {
        h.app
            .project()
            .operations
            .iter()
            .map(|o| o.id().as_str().to_owned())
            .collect()
    };
    assert_eq!(order(&harness), ["op3", "op5", "op1", "op2", "op4"]);

    let rows = operations::rows(&harness.app);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].name, "Screw holes");
    assert_eq!(rows[0].kind, Kind::Bore);
    assert_eq!(rows[0].detail, "Bore · 1/8 end mill");
    assert!(
        rows.iter().all(|r| r.status == Status::Generating),
        "{rows:?}"
    );

    let bore = OperationId::new("op3");
    assert!(operations::reorder(harness.app.project(), &bore, 0).is_none());
    let steps = harness.app.history().undo_labels().count();
    harness
        .run(operations::reorder(harness.app.project(), &bore, 2).unwrap())
        .unwrap();
    assert_eq!(order(&harness), ["op5", "op1", "op3", "op2", "op4"]);
    assert_eq!(harness.app.undo_label(), Some("Reorder operations"));
    assert_eq!(harness.app.history().undo_labels().count(), steps + 1);
    let saved = dc_project::load(harness.save().unwrap().as_bytes()).unwrap();
    let file_order: Vec<&str> = saved
        .project
        .operations
        .iter()
        .map(|o| o.id().as_str())
        .collect();
    assert_eq!(file_order, ["op5", "op1", "op3", "op2", "op4"]);
    harness.generate();
    let program_order: Vec<String> = harness
        .programs()
        .iter()
        .flat_map(|p| p.operations.iter().map(|o| o.as_str().to_owned()))
        .collect();
    let at = |id: &str| program_order.iter().position(|o| o == id);
    assert!(
        at("op1").is_some() && at("op3").is_some(),
        "{program_order:?}"
    );
    assert!(
        at("op1") < at("op3"),
        "the programs follow the list: {program_order:?}"
    );

    // Untick: the file holds `enabled: false`; the Scene and the programs drop it.
    harness
        .run(Command::SetOperationEnabled {
            id: OperationId::new("op5"),
            enabled: false,
        })
        .unwrap();
    assert_eq!(
        harness.app.undo_label(),
        Some("Disable “Border and squiggle”")
    );
    assert!(!op(&harness, "op5").enabled());
    let record = fields::record(op(&harness, "op5")).unwrap();
    assert_eq!(fields::get(&record, "enabled"), Some(&Value::Bool(false)));
    assert!(
        !harness
            .app
            .scene()
            .operations()
            .iter()
            .any(|o| o.id.as_str() == "op5"),
        "a disabled operation draws nothing"
    );
    harness.generate();
    assert!(
        harness
            .programs()
            .iter()
            .all(|p| p.operations.iter().all(|o| o.as_str() != "op5")),
        "and is in no program"
    );
    let row = operations::rows(&harness.app)
        .into_iter()
        .find(|r| r.id.as_str() == "op5")
        .unwrap();
    assert!(!row.enabled);

    // Duplicate: below itself, its targets, `(2)`.
    harness
        .run(Command::DuplicateOperation(OperationId::new("op1")))
        .unwrap();
    let ops = &harness.app.project().operations;
    let at = ops.iter().position(|o| o.id().as_str() == "op1").unwrap();
    let copy = &ops[at + 1];
    assert_eq!(copy.name(), "Panel (2)");
    assert_eq!(copy.targets(), op(&harness, "op1").targets());
    assert_eq!(copy.kind(), "pocket");

    // The eye hides a toolpath as view state: no step, not in the file.
    let steps = harness.app.history().undo_labels().count();
    let before = harness.save().unwrap();
    harness.app.toggle_hidden(&OperationId::new("op1"));
    assert!(
        harness
            .app
            .view()
            .hidden_ops
            .contains(&OperationId::new("op1"))
    );
    assert!(
        operations::rows(&harness.app)
            .iter()
            .find(|r| r.id.as_str() == "op1")
            .unwrap()
            .hidden
    );
    assert_eq!(harness.app.history().undo_labels().count(), steps);
    assert_eq!(harness.save().unwrap(), before);
    harness.app.toggle_hidden(&OperationId::new("op1"));
    assert!(harness.app.view().hidden_ops.is_empty());
}

/// Capability 65 and ADR-0005: deleting a Profile removes it and keeps
/// the tabs placed for it, which are the part's; the step undoes. A
/// deleted operation leaves the editor.
#[test]
fn delete_keeps_placed_tabs() {
    let mut harness = Harness::open(&common::plaque_bytes()).unwrap();
    let cut_out = OperationId::new("op4");
    harness
        .run(Command::PlaceTabs {
            op: cut_out.clone(),
        })
        .unwrap();
    let tabs = harness.app.project().tabs.clone();
    assert_eq!(tabs.len(), 4, "the request places four tabs");
    harness.app.edit(Some(cut_out.clone()));
    assert_eq!(harness.app.editing(), Some(&cut_out));

    let label = harness
        .run(Command::DeleteOperation(cut_out.clone()))
        .unwrap();
    assert_eq!(label, "Delete “Cut out”");
    assert!(
        !harness
            .app
            .project()
            .operations
            .iter()
            .any(|o| *o.id() == cut_out)
    );
    assert_eq!(harness.app.project().tabs, tabs, "the placed tabs stay");
    assert_eq!(harness.app.editing(), None, "the editor lets it go");

    harness.app.undo();
    assert!(
        harness
            .app
            .project()
            .operations
            .iter()
            .any(|o| *o.id() == cut_out)
    );
}
