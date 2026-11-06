// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/cam/operationFields.test.ts

//! Slice 10 of phase 4 (#539): the operation editor's table
//! (`docs/specs/phase-4/design.md`, "Architecture → The editors"; functional
//! spec, capabilities 66 and 68, acceptance 73). The shape is upstream's
//! `operationFields.test.ts`: the table is the only place a field's group
//! and order are decided, so these tests check the structural claims that
//! rest on it (every row in a declared group, the groups in one fixed
//! order, every row reachable, no kind without a group), and then the
//! claim upstream could not make, since its table names fields rather than
//! file paths: the table is the record. Every leaf of every record a kind
//! can hold has a row, every row is a path of some record, and each row
//! set through the editor's own path survives a save and a load as the CLI
//! reads it.
//!
//! What changed in the port: upstream's kinds, passes and trochoidal
//! fields do not exist here, and its `appliesTo` predicates become "the
//! row's key path is in the record" (with a predicate only where the file
//! keeps a value the generator ignores). Its exhaustive enumeration of
//! operation shapes becomes a closure over the table's own choices and
//! switches, from each kind's new operation and every committed fixture's
//! operations.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeSet;

use dc_app::editor::{self, FieldKind, Group, clearing, fields};
use dc_app::operations::{self, Kind};
use dc_app::{Command, StrKey, Strings};
use dc_job::Tier;
use dc_project::{Entry, Operation, OperationId, Project, ToolKind, Units};
use serde_json::Value;

/// The leaves of each kind's records that have no row yet (an entry holds
/// the paths under it too), with the slice that adds them. A slice adding a kind's rows deletes its entries here,
/// which turns its kind's check on: from then on every leaf of that kind
/// must have a row. An entry that gains a row fails the test until it is
/// deleted, so the list can only shrink.
fn still_to_come(kind: Kind) -> &'static [&'static str] {
    match kind {
        // Complete in slice 10 (#539), but for the tab request's block,
        // whose renderer slice 11 (#540) fills in.
        Kind::Profile => &[],
        // Slice 11 (#540).
        Kind::Pocket => &[
            "step_down_mm",
            "through_cut_allowance_mm",
            "stock_to_leave_mm",
            "strategy",
            "strategy.raster.angle_deg",
            "wall_pass",
            "step_over_fraction",
            "direction",
            "rest_of",
            "entry",
            "entry.ramp.angle_deg",
            "entry.helix.diameter_fraction",
            "entry.helix.angle_deg",
            "corner_relief",
            "machining_order",
        ],
        // Slice 11 (#540).
        Kind::Engrave => &[
            "step_down_mm",
            "through_cut_allowance_mm",
            "alternate_direction",
            "machining_order",
        ],
        // Slice 12 (#541).
        Kind::Drill => &[
            "retract_mm",
            "through_cut_allowance_mm",
            "cycle",
            "cycle.peck.peck_depth_mm",
            "cycle.peck.retract",
            "cycle.countersink.diameter_mm",
            "cycle.countersink.start_depth_mm",
            "cycle.countersink.start_plane_cleared_externally",
            "dwell_s",
            "allow_tool_wider_than_hole",
        ],
        // Slice 12 (#541).
        Kind::Bore => &[
            "pitch_mm",
            "stock_to_leave_mm",
            "through_cut_allowance_mm",
            "direction",
        ],
        // Complete in slice 13 (#542).
        Kind::VCarve => &[],
        // Slice 14 (#543).
        Kind::Inlay => &[
            "glue_gap_mm",
            "plug_depth_mm",
            "margin_mm",
            "outline_rounding_mm",
            "through_cut_allowance_mm",
            "male_board.width_mm",
            "male_board.height_mm",
            "male_board.thickness_mm",
            "clearing",
            "step_over_fraction",
            "cut_out",
            "machining_order",
        ],
        // Slice 15 (#544): no record before phase 3.4.
        Kind::PhotoVCarve => &[],
    }
}

/// A project with every kind's tools and targets: the phase 3 sign (end
/// mills, a V-bit, circles, letters) with an open path of the plaque's.
fn project() -> Project {
    common::load_dcv(&common::fixture("sign.dcv"))
}

/// Every committed project's operations, and each kind's new operation on
/// [`project`]: the records the table is checked against.
fn seeds() -> Vec<Operation> {
    let strings = Strings::english();
    let base = project();
    let targets: Vec<_> = base.instances.iter().map(|i| i.id.clone()).collect();
    let mut seeds: Vec<Operation> = Kind::ALL
        .into_iter()
        .filter_map(|kind| operations::new_operation(&base, &strings, kind, &targets))
        .collect();
    for (_, path) in common::quick_start_projects()
        .into_iter()
        .chain(common::golden_projects())
    {
        seeds.extend(common::load_dcv(&path).operations);
    }
    seeds
}

/// `seed` and each record one of the table's own choices or switches
/// makes of it: every option of every choice row and both states of every
/// switch, one at a time. One step reaches every row, since a row's key
/// path is at most one variant or feature deep.
fn reachable(project: &Project, seed: &Operation) -> Vec<Operation> {
    let mut found = vec![seed.clone()];
    for row in fields::rows(seed) {
        let next: Vec<Operation> = match row.kind {
            FieldKind::Choice(options) => (0..options.len())
                .filter_map(|i| editor::choose(project, seed, row, i).ok().flatten())
                .filter_map(updated)
                .collect(),
            FieldKind::Toggle(_) => [true, false]
                .into_iter()
                .filter_map(|on| editor::switch(project, seed, row, on).ok().flatten())
                .filter_map(updated)
                .collect(),
            _ => Vec::new(),
        };
        for op in next {
            if !found.contains(&op) {
                found.push(op);
            }
        }
    }
    found
}

/// The record a command writes, when it is an operation's update.
fn updated(command: Command) -> Option<Operation> {
    match command {
        Command::UpdateOperation(op) => Some(op),
        _ => None,
    }
}

/// The leaves of `value` under `prefix`: every scalar, `null` and array
/// (an array is one field: the targets, the clearing list, the tab
/// distances), and every one-key object's key (a variant's tag), as dotted
/// key paths.
fn leaves(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                leaves(value, &path, out);
            }
        }
        _ => {
            out.insert(prefix.to_owned());
        }
    }
}

/// Whether `row` edits the leaf at `path`: the row's own key, or a path
/// inside a block row's value (the tab request, a clearing list).
fn covers(row: &fields::Row, path: &str) -> bool {
    path == row.key
        || (row.kind.is_block()
            && path
                .strip_prefix(row.key)
                .is_some_and(|rest| rest.starts_with('.')))
}

/// Acceptance 73 (capabilities 66 and 68), table-driven over every kind:
/// the editor exposes exactly the record. For each kind, over every record
/// the table's choices reach from its new operation and the committed
/// fixtures, every leaf of the record has a row or is listed as still to
/// come ([`still_to_come`]); every row is a path of some record; nothing
/// still to come already has a row; and each value-holding row, set to a
/// value other than the record's through the editor's own path, is that
/// value after `dc_project::save` and `dc_project::load` (the CLI's reader),
/// and set back is the record again. `kind` and `id` are the record's
/// identity, not settings.
#[test]
fn fields_table_matches_the_records() {
    let project = project();
    let seeds = seeds();
    for kind in Kind::ALL {
        let table = fields::table(kind);
        let mut records = Vec::new();
        for seed in seeds.iter().filter(|s| Kind::of(s) == kind) {
            for op in reachable(&project, seed) {
                if !records.contains(&op) {
                    records.push(op);
                }
            }
        }
        // Round-tripped: the new operation and every record one step from
        // it, and each fixture's own record.
        let mut round_trip: Vec<Operation> = Vec::new();
        for (n, seed) in seeds.iter().filter(|s| Kind::of(s) == kind).enumerate() {
            let near = if n == 0 {
                reachable(&project, seed)
            } else {
                vec![seed.clone()]
            };
            for op in near {
                if !round_trip.contains(&op) {
                    round_trip.push(op);
                }
            }
        }
        if kind == Kind::PhotoVCarve {
            assert!(table.is_empty(), "no Photo V-Carve record before 3.4");
            assert!(records.is_empty());
            continue;
        }
        assert!(!records.is_empty(), "{kind:?} has a new operation");
        let pending = still_to_come(kind);
        let mut reached: BTreeSet<&str> = BTreeSet::new();
        for op in &records {
            let record = fields::record(op).expect("a record serializes");
            let mut found = BTreeSet::new();
            leaves(&record, "", &mut found);
            for leaf in found {
                if leaf == "kind" || leaf == "id" {
                    continue;
                }
                // A row whose rule hides it here still covers the leaf: the
                // file keeps a value the generator ignores (a countersink's
                // depth).
                let covered = table.iter().any(|row| covers(row, &leaf));
                let to_come = pending
                    .iter()
                    .any(|p| leaf == *p || leaf.starts_with(&format!("{p}.")));
                assert!(
                    covered || to_come,
                    "{kind:?}: `{leaf}` of {} has no row and is not listed as to come",
                    op.name()
                );
            }
            for row in fields::rows(op) {
                reached.insert(row.key);
                assert!(
                    fields::get(&record, row.key).is_some(),
                    "{kind:?}: row `{}` is applicable but not in the record",
                    row.key
                );
            }
        }
        for row in table {
            assert!(
                reached.contains(row.key),
                "{kind:?}: row `{}` applies to no record the table reaches",
                row.key
            );
            assert!(
                !pending.contains(&row.key),
                "{kind:?}: `{}` has a row; delete it from still_to_come",
                row.key
            );
        }
        let slim = Project {
            definitions: std::collections::BTreeMap::new(),
            instances: Vec::new(),
            ..project.clone()
        };
        for op in &round_trip {
            round_trip_every_row(&slim, op);
        }
    }
}

/// Each value-holding row of `op`: a different value through the editor,
/// saved and loaded in `project` (its tools, no shapes: a target the file
/// lacks is validation's problem, not the loader's), reads back; set back,
/// the record is `op` again.
fn round_trip_every_row(project: &Project, op: &Operation) {
    for row in fields::rows(op) {
        let record = fields::record(op).unwrap();
        let before = fields::get(&record, row.key).cloned().unwrap();
        let Some(other) = other_value(project, op, row, &before) else {
            continue;
        };
        let edited = fields::set(op, row.key, other.clone())
            .unwrap_or_else(|e| panic!("{}: `{}` = {other}: {e}", op.name(), row.key));
        let mut saved = project.clone();
        saved.operations = vec![edited.clone()];
        let text = dc_project::save(&saved).expect("the project saves");
        let loaded = dc_project::load(text.as_bytes()).expect("and loads");
        let reread = fields::record(&loaded.project.operations[0]).unwrap();
        assert_eq!(
            fields::get(&reread, row.key),
            Some(&other),
            "{}: `{}` set to {other} reads back from the file",
            op.name(),
            row.key
        );
        let back = fields::set(&edited, row.key, before.clone()).unwrap();
        assert_eq!(
            &back,
            op,
            "{}: `{}` set back to {before} is the record",
            op.name(),
            row.key
        );
    }
}

/// A value for `row` other than `current`, of the kind the record holds;
/// `None` for the rows whose values are other rows' (a choice, a switch,
/// a block).
fn other_value(
    project: &Project,
    op: &Operation,
    row: &fields::Row,
    current: &Value,
) -> Option<Value> {
    let number = current.as_f64();
    Some(match row.kind {
        FieldKind::Text => Value::from(format!("{} edited", current.as_str().unwrap_or(""))),
        FieldKind::Length | FieldKind::Rate | FieldKind::Seconds => {
            Value::from(number.map_or(2.5, |n| n + 1.25))
        }
        FieldKind::Angle => Value::from(if number == Some(10.0) { 20.0 } else { 10.0 }),
        FieldKind::Percentage => Value::from(if number == Some(0.3) { 0.35 } else { 0.3 }),
        FieldKind::Count => Value::from(current.as_u64().map_or(1, |n| n + 1)),
        FieldKind::Checkbox => Value::from(!current.as_bool().unwrap_or(false)),
        FieldKind::ToolPicker => {
            let other = project
                .tools
                .iter()
                .find(|t| t.id != *op.tool())
                .expect("the sign has several tools");
            Value::from(other.id.as_str())
        }
        FieldKind::InstanceList => Value::from(vec!["i1", "i2"]),
        // A length whose switch is off at `null` (a V-Carve's flat depth):
        // a length when off, off when on.
        FieldKind::SwitchedLength => number.map_or(Value::from(2.5), |_| Value::Null),
        // A clearing list: one pass of the project's first end mill when
        // empty, empty otherwise.
        FieldKind::ClearingList => {
            if current.as_array().is_some_and(Vec::is_empty) {
                let end_mill = project
                    .tools
                    .iter()
                    .find(|t| t.cutter == ToolKind::EndMill)
                    .expect("the sign has end mills");
                serde_json::json!([{
                    "tool": end_mill.id.as_str(),
                    "strategy": { "raster": { "angle_deg": 30.0 } },
                    "direction": "conventional",
                    "entry": "plunge",
                }])
            } else {
                Value::Array(Vec::new())
            }
        }
        FieldKind::Choice(_) | FieldKind::Toggle(_) => return None,
        _ => return None,
    })
}

/// Upstream's `testEveryFieldNamesADeclaredGroup` and
/// `testEveryKindRendersAtLeastOneGroup`: the groups render once each, in
/// capability 66's order, every row names one of them, and every kind
/// with a record shows Identity, Targets, Tool and Heights; a group with
/// no row for the operation is not shown.
#[test]
fn groups_render_in_the_fixed_order() {
    assert_eq!(
        Group::ALL,
        [
            Group::Identity,
            Group::Targets,
            Group::Tool,
            Group::Depth,
            Group::Strategy,
            Group::EntryAndLeads,
            Group::CornersAndTabs,
            Group::Order,
            Group::Heights,
        ]
    );
    let strings = Strings::english();
    let base = project();
    for kind in Kind::ALL {
        let Some(op) = operations::new_operation(&base, &strings, kind, &[]) else {
            continue;
        };
        let shown: Vec<Group> = fields::groups(&op).into_iter().map(|(g, _)| g).collect();
        let mut sorted = shown.clone();
        sorted.sort_by_key(|g| Group::ALL.iter().position(|a| a == g));
        assert_eq!(shown, sorted, "{kind:?}: groups in the fixed order");
        for group in [Group::Identity, Group::Targets, Group::Tool, Group::Heights] {
            assert!(shown.contains(&group), "{kind:?} shows {group:?}");
        }
        for (group, rows) in fields::groups(&op) {
            assert!(!rows.is_empty(), "{kind:?}: {group:?} is shown with no row");
            assert!(rows.iter().all(|r| r.group == group));
        }
    }
    // The Engrave has no entry, lead or corner: those groups are absent.
    let engrave = operations::new_operation(&base, &strings, Kind::Engrave, &[]).unwrap();
    let shown: Vec<Group> = fields::groups(&engrave)
        .into_iter()
        .map(|(g, _)| g)
        .collect();
    assert!(!shown.contains(&Group::EntryAndLeads), "{shown:?}");
    assert!(!shown.contains(&Group::CornersAndTabs), "{shown:?}");
}

/// Upstream's golden render order, for the one kind complete in this
/// slice: reordering the table or retagging a row into another group shows
/// here as a diff, not as a control that moved. A new Profile (`init`'s:
/// a ramp entry, no lead, no last pass, an automatic start).
#[test]
fn profile_golden_render_order() {
    let strings = Strings::english();
    let base = project();
    let profile = operations::new_operation(&base, &strings, Kind::Profile, &[]).unwrap();
    let keys: Vec<&str> = fields::rows(&profile).iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        [
            "name",
            "enabled",
            "targets",
            "tool",
            "depth_mm",
            "step_down_mm",
            "through_cut_allowance_mm",
            "side",
            "direction",
            "start",
            "last_pass",
            "entry",
            "entry.ramp.angle_deg",
            "lead",
            "corner_relief",
            "tabs",
            "machining_order",
            "safe_z_mm",
        ]
    );
}

/// `fields::get` and `fields::set` by key path on the record, through
/// `serde_json::Value`, round-trip exactly: for every operation of every
/// committed project, setting each applicable row to what `get` reads is
/// the operation unchanged. A nested path sets inside a variant
/// (`entry.ramp.angle_deg`); a path the record does not have, a value the
/// file refuses and an unknown key are refused, and nothing changes.
#[test]
fn set_and_get_by_key_path_round_trip() {
    for (name, path) in common::quick_start_projects()
        .into_iter()
        .chain(common::golden_projects())
    {
        for op in common::load_dcv(&path).operations {
            let record = fields::record(&op).unwrap();
            for row in fields::rows(&op) {
                let value = fields::get(&record, row.key).cloned().unwrap();
                let same = fields::set(&op, row.key, value)
                    .unwrap_or_else(|e| panic!("{name} {}: `{}`: {e}", op.name(), row.key));
                assert_eq!(same, op, "{name} {}: `{}` round-trips", op.name(), row.key);
            }
        }
    }

    let plaque = common::plaque();
    let cut_out = plaque
        .operations
        .iter()
        .find(|o| o.id().as_str() == "op4")
        .unwrap();
    let steeper = fields::set(cut_out, "entry.ramp.angle_deg", Value::from(20.0)).unwrap();
    let Operation::Profile(profile) = &steeper else {
        panic!("still a Profile");
    };
    assert_eq!(profile.entry, Entry::Ramp { angle_deg: 20.0 });
    let record = fields::record(&steeper).unwrap();
    assert_eq!(
        fields::get(&record, "entry.ramp.angle_deg"),
        Some(&Value::from(20.0))
    );
    assert_eq!(fields::get(&record, "depth_mm"), Some(&Value::from(12.0)));
    assert_eq!(fields::get(&record, "step_down_mm"), Some(&Value::Null));
    assert_eq!(fields::get(&record, "entry.helix.angle_deg"), None);

    assert!(fields::set(cut_out, "entry.helix.angle_deg", Value::from(5.0)).is_err());
    assert!(fields::set(cut_out, "depth_mm", Value::from("deep")).is_err());
    assert!(fields::set(cut_out, "depht_mm", Value::from(3.0)).is_err());
    assert!(fields::set(cut_out, "side", Value::from("upside")).is_err());
}

/// Capability 66: a field whose file value is `null` and whose default is
/// the tool's (step-down) shows the tool's value as a grey placeholder,
/// `tool's: 4 mm`, in the project's units; typing a value writes it, ⟲
/// writes `null` again, and leaving the empty field unchanged is no step.
/// A tool without the value says so. Only tool-derived rows reset.
#[test]
fn tool_derived_defaults_show_the_tools_value_and_write_null() {
    let strings = Strings::english();
    let mut plaque = common::plaque();
    let cut_out = plaque.operations[4].clone();
    assert_eq!(cut_out.id().as_str(), "op4");
    let row = fields::rows(&cut_out)
        .into_iter()
        .find(|r| r.key == "step_down_mm")
        .expect("a Profile has a step-down row");
    assert_eq!(row.tool_default, Some(fields::ToolDefault::StepDown));
    assert_eq!(row.group, Group::Depth);

    // The plaque's 1/4 end mill steps 4 mm.
    assert_eq!(editor::value_text(&plaque, &cut_out, row), "");
    assert_eq!(
        editor::placeholder(&plaque, &strings, &cut_out, row).as_deref(),
        Some("tool's: 4 mm")
    );
    assert!(
        !editor::can_reset(&cut_out, row),
        "nothing to reset at null"
    );
    assert_eq!(
        editor::commit_text(&plaque, &cut_out, row, "").unwrap(),
        None
    );

    let Some(Command::UpdateOperation(stepped)) =
        editor::commit_text(&plaque, &cut_out, row, "2").unwrap()
    else {
        panic!("typing a step-down writes it");
    };
    let record = fields::record(&stepped).unwrap();
    assert_eq!(
        fields::get(&record, "step_down_mm"),
        Some(&Value::from(2.0))
    );
    assert_eq!(editor::value_text(&plaque, &stepped, row), "2");
    assert!(editor::can_reset(&stepped, row));
    let Some(Command::UpdateOperation(reset)) = editor::reset(&stepped, row).unwrap() else {
        panic!("⟲ writes null");
    };
    assert_eq!(reset, cut_out, "and the record is as before");

    // In inches the placeholder is the tool's value in inches.
    plaque.display_units = Units::Inch;
    assert_eq!(
        editor::placeholder(&plaque, &strings, &cut_out, row).as_deref(),
        Some("tool's: 0.15748 in")
    );
    // A tool without a step-down says so.
    plaque.display_units = Units::Mm;
    let set_me = plaque
        .tools
        .iter()
        .find(|t| t.id.as_str() == "t1")
        .unwrap()
        .clone();
    assert_eq!(set_me.step_down_mm, None);
    let unset = fields::set(&cut_out, "tool", Value::from("t1")).unwrap();
    assert_eq!(
        editor::placeholder(&plaque, &strings, &unset, row).as_deref(),
        Some(strings.get(StrKey::EditorToolsUnset))
    );

    // A depth is the user's: no placeholder, no reset, and the file's
    // `null` stays null until typed (S20).
    let depth = fields::rows(&cut_out)
        .into_iter()
        .find(|r| r.key == "depth_mm")
        .unwrap();
    assert_eq!(depth.tool_default, None);
    assert_eq!(
        editor::placeholder(&plaque, &strings, &cut_out, depth),
        None
    );
    assert!(editor::reset(&cut_out, depth).unwrap().is_none());
}

/// Capability 66 and the design's "located warnings matched by key
/// path": a finding whose `field` is `operations[<id>].<path>` is drawn
/// under the row whose key path is the longest that holds it, errors
/// before warnings; a tool-derived row also takes its tool's field; a
/// closed group's header counts what it holds.
#[test]
fn located_warning_matches_its_row() {
    let mut harness = dc_app::test_support::Harness::from_project(project());
    let strings = Strings::english();
    let targets = vec![harness.app.project().instances[0].id.clone()];
    let command = operations::add(harness.app.project(), &strings, Kind::Profile, &targets)
        .expect("a Profile can be added");
    harness.run(command).unwrap();
    let id = harness
        .app
        .project()
        .operations
        .last()
        .unwrap()
        .id()
        .clone();
    let op = |h: &dc_app::test_support::Harness| {
        h.app
            .project()
            .operations
            .iter()
            .find(|o| *o.id() == id)
            .unwrap()
            .clone()
    };

    // A new Profile's depth is unset (S20): validation's error is on it.
    let profile = op(&harness);
    let path = format!("operations[{id}].depth_mm");
    let finding = harness
        .warnings()
        .iter()
        .find(|f| f.field.as_deref() == Some(path.as_str()))
        .expect("validation locates the unset depth")
        .clone();
    assert_eq!(finding.tier, Tier::Error);
    let row = fields::row_for(&profile, &path).expect("the depth row");
    assert_eq!(row.key, "depth_mm");
    let project = harness.app.project().clone();
    let status = editor::status(harness.warnings(), &project, &profile, row).unwrap();
    assert_eq!(status.tier, Tier::Error);
    assert_eq!(status.message, finding.message);
    // Closed, the Depth header counts the depth's error and the tool's
    // unset step-down, which the step-down row takes while it is `null`.
    assert_eq!(
        editor::group_status(harness.warnings(), &project, &profile, Group::Depth),
        Some((Tier::Error, 2))
    );
    let name = fields::rows(&profile)
        .into_iter()
        .find(|r| r.key == "name")
        .unwrap();
    assert_eq!(
        editor::status(harness.warnings(), &project, &profile, name),
        None
    );

    // The longest key path wins: a helix on an On line cut is the entry
    // row's; a nested value is its own row's, not its parent's.
    let mut on_line = fields::set(&profile, "side", Value::from("on_line")).unwrap();
    on_line = fields::set(
        &on_line,
        "entry",
        serde_json::json!({ "helix": { "diameter_fraction": 0.8, "angle_deg": 15.0 } }),
    )
    .unwrap();
    harness
        .run(Command::UpdateOperation(on_line.clone()))
        .unwrap();
    let entry = format!("operations[{id}].entry");
    assert!(
        harness
            .warnings()
            .iter()
            .any(|f| f.field.as_deref() == Some(entry.as_str())),
        "a helix needs a waste side"
    );
    assert_eq!(
        fields::row_for(&on_line, &entry).map(|r| r.key),
        Some("entry")
    );
    assert_eq!(
        fields::row_for(&on_line, &format!("operations[{id}].entry.helix.angle_deg"))
            .map(|r| r.key),
        Some("entry.helix.angle_deg")
    );
    assert_eq!(
        fields::row_for(
            &on_line,
            &format!("operations[{id}].last_pass.stock_to_leave_mm")
        )
        .map(|r| r.key),
        Some("last_pass"),
        "with no last pass in the record, the nearest row that holds the path"
    );
    let mut with_last = fields::set(&profile, "side", Value::from("outside")).unwrap();
    with_last = fields::set(
        &with_last,
        "last_pass",
        serde_json::json!({ "stock_to_leave_mm": 0.5, "thickness_mm": null, "reverse": false }),
    )
    .unwrap();
    assert_eq!(
        fields::row_for(
            &with_last,
            &format!("operations[{id}].last_pass.stock_to_leave_mm")
        )
        .map(|r| r.key),
        Some("last_pass.stock_to_leave_mm")
    );
    // Another operation's field is not this one's.
    assert_eq!(
        fields::row_for(&with_last, "operations[elsewhere].depth_mm").map(|r| r.key),
        None
    );

    // A tool-derived row takes its tool's field while it is null.
    let tool = profile.tool().clone();
    let step_down = fields::row_for(&profile, &format!("tools[{tool}].step_down_mm"))
        .expect("the step-down row takes the tool's step-down");
    assert_eq!(step_down.key, "step_down_mm");
    let own = fields::set(&profile, "step_down_mm", Value::from(1.0)).unwrap();
    assert_eq!(
        fields::row_for(&own, &format!("tools[{tool}].step_down_mm")).map(|r| r.key),
        None,
        "not once the operation has its own"
    );
}

// ── Slice 13 (#542): the V-Carve editor and the clearing-pass list. ───

/// The row of `op` at `key`, which must apply to it.
fn applicable(op: &Operation, key: &str) -> &'static fields::Row {
    fields::rows(op)
        .into_iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{}: no row `{key}`", op.name()))
}

/// The operation `id` of the harness's project.
fn operation(h: &dc_app::test_support::Harness, id: &str) -> Operation {
    h.app
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("no operation {id}"))
}

/// Runs an edit that must change something; its undo label.
fn edit(
    h: &mut dc_app::test_support::Harness,
    command: Result<Option<Command>, dc_app::CommandError>,
) -> String {
    let command = command
        .expect("the edit is accepted")
        .expect("and changes something");
    h.run(command).expect("the edit runs")
}

/// The tools of `op`'s clearing passes, by id.
fn pass_tools(op: &Operation, list: &fields::Row) -> Vec<String> {
    clearing::passes(op, list)
        .iter()
        .map(|p| p.tool.as_str().to_owned())
        .collect()
}

/// The findings of `h` with `code`.
fn coded(h: &dc_app::test_support::Harness, code: &str) -> Vec<dc_job::Finding> {
    h.warnings()
        .iter()
        .filter(|f| f.code.as_deref() == Some(code))
        .cloned()
        .collect()
}

/// A clearing pass of `tool`, as the chapter's first.
fn pass(tool: &str) -> dc_project::ClearingPass {
    dc_project::ClearingPass {
        tool: dc_project::ToolId::new(tool),
        strategy: dc_project::ClearanceStrategy::Offset,
        direction: dc_project::CutDirection::Climb,
        entry: Entry::Plunge,
    }
}

/// Acceptance 75 (capability 70): on the sign's Lettering (no flat depth,
/// no passes), typing a flat depth and pressing *Add pass* twice writes
/// the 6.35 mm end mill's pass and then the 3.175 mm one's, in that order
/// in the file, each with `init`'s direction and entry and an offset
/// clearance. Dragging the smaller above the larger is one step and shows
/// `clearing_not_descending` under the list, as `check` reports it; undo
/// puts the order back in one step. Dropping a pass where it was is no
/// step, and *Remove* drops one pass.
#[test]
fn clearing_list_orders_and_warns() {
    let mut h = dc_app::test_support::Harness::from_project(project());
    let lettering = operation(&h, "op3");
    let list = applicable(&lettering, "clearing");
    assert!(matches!(list.kind, FieldKind::ClearingList));
    assert_eq!(list.group, Group::Strategy);
    assert!(clearing::passes(&lettering, list).is_empty());

    // The floor to clear: the flat depth is off at `null` until typed.
    let flat = applicable(&lettering, "flat_depth_mm");
    assert!(matches!(flat.kind, FieldKind::SwitchedLength));
    assert_eq!(flat.group, Group::Depth);
    assert!(!editor::switched(&lettering, flat));
    let typed = editor::commit_text(h.app.project(), &lettering, flat, "2.5");
    edit(&mut h, typed);
    assert!(editor::switched(&operation(&h, "op3"), flat));

    for _ in 0..2 {
        let op = operation(&h, "op3");
        let add = clearing::add(h.app.project(), &op, list);
        edit(&mut h, add);
    }
    let op = operation(&h, "op3");
    assert_eq!(
        pass_tools(&op, list),
        ["t1", "t3"],
        "6.35 mm, then 3.175 mm"
    );
    let init = dc_job::project_from::default_profile(Vec::new(), dc_project::ToolId::new(""));
    for added in clearing::passes(&op, list) {
        assert_eq!(added.strategy, dc_project::ClearanceStrategy::Offset);
        assert_eq!(added.direction, init.direction);
        assert_eq!(added.entry, init.entry);
    }
    let saved = dc_project::load(h.save().unwrap().as_bytes())
        .expect("the project loads")
        .project;
    let Some(Operation::VCarve(carve)) = saved.operations.iter().find(|o| o.id().as_str() == "op3")
    else {
        panic!("the Lettering is saved");
    };
    let in_file: Vec<&str> = carve.clearing.iter().map(|p| p.tool.as_str()).collect();
    assert_eq!(in_file, ["t1", "t3"], "the file holds them in that order");
    assert!(coded(&h, "clearing_not_descending").is_empty());

    // The smaller dragged above the larger: one step, and the warning.
    let undo_before = h.app.undo_label().map(str::to_owned);
    let drag = clearing::reorder(&op, list, 1, 0);
    edit(&mut h, drag);
    let op = operation(&h, "op3");
    assert_eq!(pass_tools(&op, list), ["t3", "t1"]);
    let found = coded(&h, "clearing_not_descending");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].tier, Tier::Warning);
    assert_eq!(
        found[0].field.as_deref(),
        Some("operations[op3].clearing[1].tool")
    );
    let located = editor::Located::of(h.warnings(), &op);
    assert!(
        located
            .all(list)
            .iter()
            .any(|s| s.tier == Tier::Warning && s.message == found[0].message),
        "under the list"
    );
    let built = dc_job::build(
        h.app.project(),
        dc_job::ProgramSplit::ProjectDefault,
        dc_app::test_support::DATE,
    );
    assert!(
        built
            .findings
            .warnings
            .iter()
            .any(|w| w.code.as_deref() == Some("clearing_not_descending")
                && w.message == found[0].message),
        "as check reports it"
    );
    assert_eq!(
        clearing::reorder(&op, list, 0, 0).unwrap(),
        None,
        "dropped where it was"
    );
    h.app.undo();
    assert_eq!(pass_tools(&operation(&h, "op3"), list), ["t1", "t3"]);
    assert_eq!(
        h.app.undo_label().map(str::to_owned),
        undo_before,
        "the drag was one step"
    );
    assert!(coded(&h, "clearing_not_descending").is_empty());

    let op = operation(&h, "op3");
    assert!(clearing::remove(&op, list, 5).is_err(), "no such pass");
    let remove = clearing::remove(&op, list, 1);
    edit(&mut h, remove);
    assert_eq!(pass_tools(&operation(&h, "op3"), list), ["t1"]);
}

/// Capability 70's *Add pass*: the end mill just smaller than the
/// smallest pass's, the largest end mill for the first pass (the first
/// listed of equals), never another cutter; when none is smaller the pass
/// names no tool and validation says so under the list
/// (`clearing_dangling_tool`).
#[test]
fn add_pass_picks_the_next_smaller_end_mill() {
    let base = project();
    let id = |s: &str| Some(dc_project::ToolId::new(s));
    // The sign's end mills: t1 and t2 6.35 mm, t3 3.175 mm; t4 a 6.35 mm
    // V-bit.
    assert_eq!(clearing::next_smaller_end_mill(&base, &[]), id("t1"));
    assert_eq!(
        clearing::next_smaller_end_mill(&base, &[pass("t2")]),
        id("t3")
    );
    assert_eq!(
        clearing::next_smaller_end_mill(&base, &[pass("t2"), pass("t3")]),
        None
    );
    assert_eq!(
        clearing::next_smaller_end_mill(&base, &[pass("t3"), pass("t2")]),
        None,
        "smaller than every pass, not only the last"
    );
    assert_eq!(
        clearing::next_smaller_end_mill(&base, &[pass("t2"), pass("")]),
        id("t3"),
        "a pass with no tool does not count"
    );
    // A smaller ball nose is not an end mill.
    let mut with_ball = base.clone();
    let mut ball = with_ball.tools[2].clone();
    ball.id = dc_project::ToolId::new("t9");
    ball.cutter = ToolKind::BallNose;
    ball.diameter_mm = 1.5;
    with_ball.tools.push(ball);
    assert_eq!(
        clearing::next_smaller_end_mill(&with_ball, &[pass("t3")]),
        None
    );
    // No end mill at all: the first pass names none.
    let mut no_mills = base.clone();
    no_mills.tools.retain(|t| t.cutter != ToolKind::EndMill);
    assert_eq!(clearing::next_smaller_end_mill(&no_mills, &[]), None);

    // Through the list: the Background's 6.35 mm pass, then 3.175 mm,
    // then none.
    let mut h = dc_app::test_support::Harness::from_project(base);
    let background = operation(&h, "op2");
    let list = applicable(&background, "clearing");
    assert_eq!(pass_tools(&background, list), ["t2"]);
    for _ in 0..2 {
        let op = operation(&h, "op2");
        let add = clearing::add(h.app.project(), &op, list);
        edit(&mut h, add);
    }
    let op = operation(&h, "op2");
    assert_eq!(pass_tools(&op, list), ["t2", "t3", ""]);
    // `clearing_dangling_tool` is an error, which carries no code.
    let dangling: Vec<dc_job::Finding> = h
        .warnings()
        .iter()
        .filter(|f| f.field.as_deref() == Some("operations[op2].clearing[2].tool"))
        .cloned()
        .collect();
    assert_eq!(dangling.len(), 1, "{dangling:?}");
    assert_eq!(dangling[0].tier, Tier::Error);
    assert!(
        dangling[0].message.contains("which does not exist"),
        "{}",
        dangling[0].message
    );
    assert!(
        editor::Located::of(h.warnings(), &op)
            .all(list)
            .iter()
            .any(|s| s.tier == Tier::Error && s.message == dangling[0].message),
        "validation says so under the list"
    );
}

/// The rows of one clearing pass (capability 70: tool, clearance strategy,
/// direction, entry) are a table of their own, edited at the pass's key
/// path (`clearing[1].strategy`) through the editor's own functions: every
/// leaf of every pass the table's choices reach has a row, each choice
/// writes only its pass, and the tool picker offers the project's end
/// mills only.
#[test]
fn clearing_pass_rows_cover_a_pass() {
    let mut h = dc_app::test_support::Harness::from_project(project());
    let background = operation(&h, "op2");
    let list = applicable(&background, "clearing");
    let add = clearing::add(h.app.project(), &background, list);
    edit(&mut h, add);
    let op = operation(&h, "op2");
    let project = h.app.project().clone();

    let offered: Vec<&str> = clearing::tools(&project)
        .iter()
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(offered, ["t1", "t2", "t3"], "end mills only");

    // Every option of every choice, on the second pass.
    let mut records = vec![op.clone()];
    for slot in clearing::rows(&op, list, 1) {
        assert!(slot.key().starts_with("clearing[1]."), "{}", slot.key());
        if let FieldKind::Choice(options) = slot.row.kind {
            for i in 0..options.len() {
                if let Some(Command::UpdateOperation(next)) = slot.choose(&project, &op, i).unwrap()
                {
                    assert_eq!(
                        clearing::passes(&next, list)[0],
                        clearing::passes(&op, list)[0],
                        "`{}` writes only its pass",
                        slot.key()
                    );
                    records.push(next);
                }
            }
        }
    }
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for record in &records {
        let passes = clearing::passes(record, list);
        for (index, each) in passes.iter().enumerate() {
            let value = serde_json::to_value(each).unwrap();
            let mut found = BTreeSet::new();
            leaves(&value, "", &mut found);
            let keys: Vec<&str> = clearing::rows(record, list, index)
                .iter()
                .map(|s| s.row.key)
                .collect();
            for leaf in found {
                assert!(
                    clearing::PASS.iter().any(|r| r.key == leaf),
                    "pass leaf `{leaf}` has no row"
                );
                assert!(keys.contains(&leaf.as_str()), "`{leaf}` applies");
                reached.insert(leaf);
            }
        }
    }
    for row in clearing::PASS {
        assert!(
            reached.contains(row.key),
            "pass row `{}` is reached",
            row.key
        );
    }

    // A raster at 30° on the second pass, typed; then the first end mill.
    let strategy = clearing::rows(&op, list, 1)
        .into_iter()
        .find(|s| s.row.key == "strategy")
        .unwrap();
    let raster = strategy.choose(&project, &op, 1);
    edit(&mut h, raster);
    let op = operation(&h, "op2");
    let angle = clearing::rows(&op, list, 1)
        .into_iter()
        .find(|s| s.row.key == "strategy.raster.angle_deg")
        .expect("the raster's angle follows its strategy");
    let typed = angle.commit_text(&project, &op, "30");
    edit(&mut h, typed);
    let op = operation(&h, "op2");
    assert_eq!(
        clearing::passes(&op, list)[1].strategy,
        dc_project::ClearanceStrategy::Raster { angle_deg: 30.0 }
    );
    assert_eq!(angle.value_text(&project, &op), "30");
    let tool = clearing::rows(&op, list, 1)
        .into_iter()
        .find(|s| s.row.key == "tool")
        .unwrap();
    let picked = tool.set_tool(&op, &dc_project::ToolId::new("t1"));
    edit(&mut h, picked);
    assert_eq!(pass_tools(&operation(&h, "op2"), list), ["t2", "t1"]);
}

/// The editor's status line is `check`'s carve summary for the edited
/// operation, from the generation results the app already holds: the
/// sign's two V-Carves' lines equal those `check` prints (deepest cut,
/// flat floor, and the ridges where no clearing pass reaches); a Drill has
/// none; and after an edit the line waits for the carve to regenerate,
/// then is `check`'s for the edited project.
#[test]
fn carve_summary_matches_check() {
    let base = project();
    let check = |project: &Project, id: &str| -> Vec<String> {
        let built = dc_job::build(
            project,
            dc_job::ProgramSplit::ProjectDefault,
            dc_app::test_support::DATE,
        );
        let generated = built.generated.expect("the sign generates");
        dc_job::describe_carves(project, &generated)
            .into_iter()
            .filter(|line| line.starts_with(&format!("V-Carve {id} ")))
            .collect()
    };
    let mut h = dc_app::test_support::Harness::from_project(base.clone());
    let summary = |h: &dc_app::test_support::Harness, id: &str| {
        editor::carve_summary(&h.app, &OperationId::new(id))
    };
    assert!(summary(&h, "op2").is_empty(), "nothing before generation");
    h.generate();
    for id in ["op2", "op3"] {
        let line = check(&base, id);
        assert_eq!(line.len(), 1, "{id}: {line:?}");
        assert_eq!(summary(&h, id), line, "{id}");
    }
    let background = &summary(&h, "op2")[0];
    assert!(background.contains("deepest cut"), "{background}");
    assert!(
        background.contains("clamped at a flat depth of"),
        "{background}"
    );
    assert!(
        background.contains("where no clearing pass reaches"),
        "{background}"
    );
    assert!(summary(&h, "op1").is_empty(), "a Drill has no carve line");

    let lettering = operation(&h, "op3");
    let flat = applicable(&lettering, "flat_depth_mm");
    let typed = editor::commit_text(h.app.project(), &lettering, flat, "1");
    edit(&mut h, typed);
    assert!(summary(&h, "op3").is_empty(), "stale until it regenerates");
    h.generate();
    let edited = h.app.project().clone();
    let line = check(&edited, "op3");
    assert!(line[0].contains("clamped at a flat depth of"), "{line:?}");
    assert_eq!(summary(&h, "op3"), line);
}
