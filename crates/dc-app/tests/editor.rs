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
//!
//! Slice 11 (#540) completes the Profile, Pocket and Engrave rows: the tab
//! request's settings, `rest_of` (earlier Pockets only), and the start
//! point clicked on the Viewport.
//!
//! Slice 12 (#541) completes the Drill and the Bore: the Drill's cycle and
//! each cycle's rows (a peck's retract is a variant inside a variant, so
//! the closure of records follows a choice's own choices), the
//! countersink's depth, derived and shown, never typed (S18), and the
//! generator's warnings about a target the kind cannot cut, under the
//! Targets group.
//!
//! Slice 14 (#543) completes the Inlay: its values once, the Male board
//! and Cut-out groups (capability 68a, which add two groups to capability
//! 66's nine for the Inlay alone), the derived line under Depth and the
//! base under the band, both computed from `dc-project`'s own rules.

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
use dc_project::{
    DrillCycle, DrillOp, Entry, Operation, OperationId, PeckRetract, Project, ToolKind, Units,
};
use serde_json::Value;

/// The leaves of each kind's records that have no row yet (an entry holds
/// the paths under it too), with the slice that adds them. A slice adding a kind's rows deletes its entries here,
/// which turns its kind's check on: from then on every leaf of that kind
/// must have a row. An entry that gains a row fails the test until it is
/// deleted, so the list can only shrink.
fn still_to_come(kind: Kind) -> &'static [&'static str] {
    match kind {
        // Complete in slice 10 (#539) and slice 11 (#540).
        Kind::Profile | Kind::Pocket | Kind::Engrave => &[],
        // Complete in slice 12 (#541).
        Kind::Drill | Kind::Bore => &[],
        // Complete in slice 13 (#542).
        Kind::VCarve => &[],
        // Complete in slice 14 (#543).
        Kind::Inlay => &[],
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

/// `seed` and each record the table's own choices and switches make of
/// it: every option of every choice row and both states of every switch,
/// one at a time; then, in each record so made, the same for the rows it
/// has that its parent had not (a variant's own choice, as a peck's
/// retract under a Drill's cycle), and so on down. That reaches every row,
/// since a row is reached through the variants and features above it.
fn reachable(project: &Project, seed: &Operation) -> Vec<Operation> {
    let mut found = vec![seed.clone()];
    explore(project, seed, &[], &mut found);
    found
}

/// [`reachable`] from `op`, through its rows whose keys are not in
/// `known`, its parent's.
fn explore(project: &Project, op: &Operation, known: &[&str], found: &mut Vec<Operation>) {
    let rows = fields::rows(op);
    let keys: Vec<&str> = rows.iter().map(|r| r.key).collect();
    for row in rows {
        if known.contains(&row.key) {
            continue;
        }
        for next in one_step(project, op, row) {
            if !found.contains(&next) {
                found.push(next.clone());
                explore(project, &next, &keys, found);
            }
        }
    }
}

/// The records one of `row`'s choices or switches makes of `op`.
fn one_step(project: &Project, op: &Operation, row: &fields::Row) -> Vec<Operation> {
    match row.kind {
        FieldKind::Choice(options) => (0..options.len())
            .filter_map(|i| editor::choose(project, op, row, i).ok().flatten())
            .filter_map(updated)
            .collect(),
        FieldKind::Toggle(_) | FieldKind::TabRequest => [true, false]
            .into_iter()
            .filter_map(|on| editor::switch(project, op, row, on).ok().flatten())
            .filter_map(updated)
            .collect(),
        // None, or a Pocket to rest on: the file reads any id, and
        // validation says whether it is an earlier one.
        FieldKind::EarlierPocket => [None, Some(dc_project::OperationId::new("op1"))]
            .iter()
            .filter_map(|id| {
                editor::set_earlier_pocket(op, row, id.as_ref())
                    .ok()
                    .flatten()
            })
            .filter_map(updated)
            .collect(),
        _ => Vec::new(),
    }
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
/// key paths. An internally tagged variant's `kind` (a tab request's
/// `placement`) is its tag, so its leaf is the variant's own path, the
/// path of the row that chooses it.
fn leaves(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "kind" && !prefix.is_empty() && value.is_string() {
                    out.insert(prefix.to_owned());
                    continue;
                }
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
        FieldKind::LengthList => {
            if current.as_array().is_some_and(Vec::is_empty) {
                Value::from(vec![12.5, 60.0])
            } else {
                Value::Array(Vec::new())
            }
        }
        // The file reads any id; validation says whether it is an earlier
        // Pocket.
        FieldKind::EarlierPocket => {
            if current.is_null() {
                Value::from("op1")
            } else {
                Value::Null
            }
        }
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
            Group::MaleBoard,
            Group::Strategy,
            Group::EntryAndLeads,
            Group::CornersAndTabs,
            Group::CutOut,
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
            "tabs.placement",
            "tabs.placement.count",
            "tabs.width_mm",
            "tabs.height_mm",
            "tabs.shape",
            "tabs.on_holes",
            "tabs.avoid_corners",
            "machining_order",
            "safe_z_mm",
        ]
    );
}

/// The golden render order of the other two kinds this slice completes
/// (capability 68's Pocket and Engrave lines): a new Pocket (offset, a
/// last wall pass, `init`'s ramp) and a new Engrave. A raster's angle
/// follows its strategy; the stepover is the tool's while `null` and shows
/// as a percentage.
#[test]
fn pocket_and_engrave_golden_render_order() {
    let strings = Strings::english();
    let base = project();
    let pocket = operations::new_operation(&base, &strings, Kind::Pocket, &[]).unwrap();
    let keys: Vec<&str> = fields::rows(&pocket).iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        [
            "name",
            "enabled",
            "targets",
            "tool",
            "depth_mm",
            "step_down_mm",
            "stock_to_leave_mm",
            "through_cut_allowance_mm",
            "strategy",
            "wall_pass",
            "step_over_fraction",
            "direction",
            "rest_of",
            "entry",
            "entry.ramp.angle_deg",
            "corner_relief",
            "machining_order",
            "safe_z_mm",
        ]
    );
    let raster = fields::set(
        &pocket,
        "strategy",
        serde_json::json!({ "raster": { "angle_deg": 0.0 } }),
    )
    .unwrap();
    let keys: Vec<&str> = fields::rows(&raster).iter().map(|r| r.key).collect();
    assert_eq!(
        keys.get(8..10),
        Some(["strategy", "strategy.raster.angle_deg"].as_slice()),
        "the raster angle follows the strategy"
    );
    let stepover = fields::rows(&pocket)
        .into_iter()
        .find(|r| r.key == "step_over_fraction")
        .unwrap();
    assert_eq!(stepover.tool_default, Some(fields::ToolDefault::StepOver));
    assert_eq!(stepover.unit(), fields::Unit::Percent);

    let engrave = operations::new_operation(&base, &strings, Kind::Engrave, &[]).unwrap();
    let keys: Vec<&str> = fields::rows(&engrave).iter().map(|r| r.key).collect();
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
            "alternate_direction",
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

/// The row of `op` at `key`, which must apply.
fn row_of(op: &Operation, key: &str) -> &'static fields::Row {
    fields::rows(op)
        .into_iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{}: no row `{key}`", op.name()))
}

/// The operation `id` of the harness's project.
fn op_of(h: &dc_app::test_support::Harness, id: &str) -> Operation {
    h.app
        .project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("no operation {id}"))
}

/// Runs `command`, which must be an edit, and returns its undo label.
fn run_edit(
    h: &mut dc_app::test_support::Harness,
    command: Result<Option<Command>, dc_app::CommandError>,
) -> String {
    let command = command
        .expect("the edit is accepted")
        .expect("and changes something");
    h.run(command).expect("the edit runs")
}

/// Capability 68's Pocket line and capability 13: `rest_of` offers the
/// earlier enabled Pockets only, in list order, so a Pocket can never
/// rest on itself, a later Pocket, a disabled one or another kind
/// (validation's `rest_of_invalid` rule, `dc-project`'s `pocket`). The
/// plaque's list is Bore, Engrave, Panel (op1), Square recess (op2),
/// Profile; a disabled Pocket and a later one are added. Choosing one
/// writes its id and choosing none writes `null`, one step each; a
/// hand-edited `rest_of` naming a later Pocket still shows, with
/// validation's message under the row.
#[test]
fn rest_of_offers_earlier_pockets_only() {
    let mut plaque = common::plaque();
    let Some(Operation::Pocket(panel)) = plaque
        .operations
        .iter()
        .find(|o| o.id().as_str() == "op1")
        .cloned()
    else {
        panic!("the plaque's Panel is a Pocket");
    };
    let off = dc_project::PocketOp {
        id: dc_project::OperationId::new("op6"),
        name: "Off".into(),
        enabled: false,
        ..panel.clone()
    };
    let finish = dc_project::PocketOp {
        id: dc_project::OperationId::new("op7"),
        name: "Finish".into(),
        ..panel
    };
    let profile_at = plaque
        .operations
        .iter()
        .position(|o| o.id().as_str() == "op4")
        .unwrap();
    plaque.operations.insert(profile_at, Operation::Pocket(off));
    plaque.operations.push(Operation::Pocket(finish));
    let mut h = dc_app::test_support::Harness::from_project(plaque);
    let offered = |h: &dc_app::test_support::Harness, id: &str| -> Vec<String> {
        editor::earlier_pockets(h.app.project(), &op_of(h, id))
            .into_iter()
            .map(|(id, name)| format!("{id} {name}"))
            .collect()
    };
    assert!(offered(&h, "op1").is_empty(), "the first Pocket has none");
    assert_eq!(offered(&h, "op2"), ["op1 Panel"]);
    assert_eq!(
        offered(&h, "op7"),
        ["op1 Panel", "op2 Square recess"],
        "not itself, not the disabled Pocket"
    );
    assert!(
        offered(&h, "op4").is_empty(),
        "a Profile rests on nothing, and has no row"
    );
    assert!(
        fields::rows(&op_of(&h, "op4"))
            .iter()
            .all(|r| r.key != "rest_of")
    );

    let finish = op_of(&h, "op7");
    let row = row_of(&finish, "rest_of");
    assert!(matches!(row.kind, FieldKind::EarlierPocket));
    assert_eq!(row.group, Group::Strategy);
    let label = {
        let edit =
            editor::set_earlier_pocket(&finish, row, Some(&dc_project::OperationId::new("op1")));
        run_edit(&mut h, edit)
    };
    assert!(label.ends_with("of “Finish”"), "{label}");
    let Operation::Pocket(rest) = op_of(&h, "op7") else {
        panic!("still a Pocket");
    };
    assert_eq!(
        rest.rest_of.as_ref().map(dc_project::OperationId::as_str),
        Some("op1")
    );
    assert!(
        !h.warnings()
            .iter()
            .any(|f| f.code.as_deref() == Some("rest_of_invalid")),
        "an earlier enabled Pocket of the same shapes is valid: {:?}",
        h.warnings()
    );
    assert_eq!(
        editor::set_earlier_pocket(
            &op_of(&h, "op7"),
            row,
            Some(&dc_project::OperationId::new("op1"))
        )
        .unwrap(),
        None,
        "choosing it again is no step"
    );
    {
        let edit = editor::set_earlier_pocket(&op_of(&h, "op7"), row, None);
        run_edit(&mut h, edit)
    };
    let Operation::Pocket(cleared) = op_of(&h, "op7") else {
        panic!("still a Pocket");
    };
    assert_eq!(cleared.rest_of, None);

    // A hand-edited file's later Pocket is kept and reported under the row.
    let panel = op_of(&h, "op1");
    let later = fields::set(&panel, "rest_of", Value::from("op7")).unwrap();
    h.run(Command::UpdateOperation(later.clone())).unwrap();
    let located = editor::Located::of(h.warnings(), &later);
    let status = located
        .status(row_of(&later, "rest_of"))
        .expect("validation's message is under the row");
    assert_eq!(status.tier, Tier::Error);
}

/// Capability 68's Profile line, "Near a point: typed or clicked": with
/// the start point *Near a point*, its pick row takes a click on the
/// Viewport, and the click writes `start.near` as the project point the
/// camera puts under it (`Camera::to_world`, the one Y flip), in mm
/// whatever the display units, one undo step. Choosing Near writes the
/// stock's centre first; Auto and First vertex have no point to pick, and
/// a row that is not a pick refuses one.
#[test]
fn start_point_near_can_be_clicked_on_the_viewport() {
    let mut h = dc_app::test_support::Harness::from_project(common::plaque());
    let cut_out = op_of(&h, "op4");
    assert!(
        fields::rows(&cut_out)
            .iter()
            .all(|r| !r.key.starts_with("start.near")),
        "an automatic start has no point"
    );
    let start = row_of(&cut_out, "start");
    let near_index = match start.kind {
        FieldKind::Choice(options) => options.iter().position(|o| o.tag == "near").unwrap(),
        _ => panic!("the start point is a choice"),
    };
    {
        let edit = editor::choose(h.app.project(), &cut_out, start, near_index);
        run_edit(&mut h, edit)
    };
    let near = op_of(&h, "op4");
    let Operation::Profile(profile) = &near else {
        panic!("still a Profile");
    };
    // The plaque's stock is 170 × 120 mm.
    assert_eq!(
        profile.start,
        dc_project::StartAt::Near {
            x_mm: 85.0,
            y_mm: 60.0
        },
        "Near starts at the stock's centre"
    );
    let pick = row_of(&near, "start.near");
    assert!(matches!(pick.kind, FieldKind::PointPick));
    assert_eq!(pick.group, Group::Strategy);

    // The Viewport's camera, fitted to the plaque in an 800 × 600 pane.
    let mut camera = h.app.view().camera;
    camera.viewport_px = dc_geometry::Vec2::new(800.0, 600.0);
    camera.fit(dc_app::viewport::camera::fit_bounds(
        h.app.project(),
        h.app.scene(),
    ));
    let want = dc_geometry::Point::new(20.0, 30.0);
    let at = camera.to_screen(want);
    let clicked = camera.to_world(dc_app::Pos {
        x_px: at.x_px.round(),
        y_px: at.y_px.round(),
    });
    let label = run_edit(&mut h, editor::pick_point(&near, pick, clicked));
    assert_eq!(label, "Set start of “Cut out”");
    let picked = op_of(&h, "op4");
    let Operation::Profile(profile) = &picked else {
        panic!("still a Profile");
    };
    let dc_project::StartAt::Near { x_mm, y_mm } = profile.start else {
        panic!("still Near: {:?}", profile.start);
    };
    common::assert_near(
        dc_geometry::Point::new(x_mm, y_mm),
        clicked,
        1e-9,
        "the start is the point under the click",
    );
    common::assert_near(clicked, want, 0.5, "the camera maps the click back");
    assert_eq!(
        editor::value_text(h.app.project(), &picked, row_of(&picked, "start.near.x_mm")),
        dc_app::units::field_text(x_mm, Units::Mm),
        "the typed rows show the clicked point"
    );
    assert_eq!(
        editor::pick_point(&picked, pick, clicked).unwrap(),
        None,
        "the same point again is no step"
    );

    // In an inch project the file still holds millimetres.
    let mut inch = h.app.project().clone();
    inch.display_units = Units::Inch;
    let elsewhere = dc_geometry::Point::new(25.4, 50.8);
    let Some(Command::UpdateOperation(moved)) =
        editor::pick_point(&picked, pick, elsewhere).unwrap()
    else {
        panic!("a click elsewhere moves it");
    };
    let Operation::Profile(profile) = &moved else {
        panic!("still a Profile");
    };
    assert_eq!(
        profile.start,
        dc_project::StartAt::Near {
            x_mm: 25.4,
            y_mm: 50.8
        }
    );
    assert_eq!(
        editor::value_text(&inch, &moved, row_of(&moved, "start.near.x_mm")),
        "1"
    );

    // Only a pick row takes a click.
    assert!(editor::pick_point(&picked, start, clicked).is_err());
}

/// Capability 69 and acceptance 78's request half: the coaster's Profile
/// (`init`'s request of four 8 × 3 mm rect tabs, not on holes, off the
/// corners) edits its tab request through its rows. Off writes `null` and
/// on writes `init`'s request again; count, width, height, shape, on holes
/// and avoid corners write their fields; Distances keeps the size and
/// takes typed distances in the display units; Count again writes `init`'s
/// four. Each edit is one undo step, and the saved file read back by
/// `dc_project::load` (the CLI's reader) holds the request as edited.
/// Placing it is slice 17's (#546).
#[test]
fn profile_tab_request_round_trips() {
    let mut h = dc_app::test_support::Harness::from_project(common::coaster());
    let id = h.app.project().operations[0].id().as_str().to_owned();
    let saved_tabs = |h: &dc_app::test_support::Harness| -> Option<dc_project::TabSpec> {
        let text = h.save().expect("the project saves");
        let loaded = dc_project::load(text.as_bytes()).expect("and the CLI reads it");
        match loaded
            .project
            .operations
            .iter()
            .find(|o| o.id().as_str() == id)
        {
            Some(Operation::Profile(p)) => p.tabs.clone(),
            other => panic!("the Profile is saved: {other:?}"),
        }
    };
    let init = dc_job::project_from::default_profile(Vec::new(), dc_project::ToolId::new(""))
        .tabs
        .expect("init writes a request");
    assert_eq!(saved_tabs(&h), Some(init.clone()), "init's request");
    let profile = op_of(&h, &id);
    let tabs = row_of(&profile, "tabs");
    assert!(matches!(tabs.kind, FieldKind::TabRequest));
    assert_eq!(tabs.group, Group::CornersAndTabs);
    assert!(editor::switched(&profile, tabs));

    // Off is `null`, and its rows go.
    {
        let edit = editor::switch(h.app.project(), &profile, tabs, false);
        run_edit(&mut h, edit)
    };
    assert_eq!(saved_tabs(&h), None);
    let off = op_of(&h, &id);
    assert!(
        fields::rows(&off)
            .iter()
            .all(|r| !r.key.starts_with("tabs.")),
        "no request, no settings"
    );
    // On is `init`'s request.
    {
        let edit = editor::switch(h.app.project(), &off, tabs, true);
        run_edit(&mut h, edit)
    };
    assert_eq!(saved_tabs(&h), Some(init));

    let typed = |h: &mut dc_app::test_support::Harness, key: &str, text: &str| {
        let op = op_of(h, &id);
        let project = h.app.project().clone();
        run_edit(
            h,
            editor::commit_text(&project, &op, row_of(&op, key), text),
        )
    };
    typed(&mut h, "tabs.placement.count", "6");
    typed(&mut h, "tabs.width_mm", "10");
    typed(&mut h, "tabs.height_mm", "2.5");
    let op = op_of(&h, &id);
    let shape = row_of(&op, "tabs.shape");
    {
        let edit = editor::choose(h.app.project(), &op, shape, 1);
        run_edit(&mut h, edit)
    };
    let op = op_of(&h, &id);
    run_edit(
        &mut h,
        editor::set_checked(&op, row_of(&op, "tabs.on_holes"), true),
    );
    let op = op_of(&h, &id);
    run_edit(
        &mut h,
        editor::set_checked(&op, row_of(&op, "tabs.avoid_corners"), false),
    );
    let edited = dc_project::TabSpec {
        placement: dc_project::TabPlacement::Count { count: 6 },
        width_mm: 10.0,
        height_mm: 2.5,
        shape: dc_project::TabShape::Smooth,
        on_holes: true,
        avoid_corners: false,
    };
    assert_eq!(saved_tabs(&h), Some(edited.clone()));

    // Distances keep the size; typed in inches they are saved in mm.
    let op = op_of(&h, &id);
    let placement = row_of(&op, "tabs.placement");
    assert_eq!(editor::choice(&op, placement), Some(0), "Count");
    {
        let edit = editor::choose(h.app.project(), &op, placement, 1);
        run_edit(&mut h, edit)
    };
    let op = op_of(&h, &id);
    assert_eq!(editor::choice(&op, placement), Some(1), "Distances");
    assert!(
        fields::rows(&op)
            .iter()
            .all(|r| r.key != "tabs.placement.count"),
        "the count goes with its variant"
    );
    let distances = row_of(&op, "tabs.placement.distances_mm");
    assert!(matches!(distances.kind, FieldKind::LengthList));
    let mut inch = h.app.project().clone();
    inch.display_units = Units::Inch;
    run_edit(&mut h, editor::commit_text(&inch, &op, distances, "1, 2.5"));
    let op = op_of(&h, &id);
    assert_eq!(editor::value_text(&inch, &op, distances), "1, 2.5");
    assert_eq!(
        editor::value_text(h.app.project(), &op, distances),
        "25.4, 63.5"
    );
    assert_eq!(
        saved_tabs(&h),
        Some(dc_project::TabSpec {
            placement: dc_project::TabPlacement::AtDistances {
                distances_mm: vec![25.4, 63.5]
            },
            ..edited.clone()
        })
    );
    assert!(
        editor::commit_text(h.app.project(), &op, distances, "10, deep").is_err(),
        "every distance is a length"
    );

    // Count again is `init`'s four, the size kept: the request acceptance
    // 78 places.
    {
        let edit = editor::choose(h.app.project(), &op, placement, 0);
        run_edit(&mut h, edit)
    };
    assert_eq!(
        saved_tabs(&h),
        Some(dc_project::TabSpec {
            placement: dc_project::TabPlacement::Count { count: 4 },
            ..edited
        })
    );
    // A count of 0 is the file's, and validation's message is under it;
    // more than 255 is not.
    typed(&mut h, "tabs.placement.count", "0");
    let zero = op_of(&h, &id);
    let status = editor::Located::of(h.warnings(), &zero)
        .status(row_of(&zero, "tabs.placement.count"))
        .expect("a count of 0 is reported under the count");
    assert_eq!(status.tier, Tier::Error);
    assert!(
        editor::commit_text(
            h.app.project(),
            &zero,
            row_of(&zero, "tabs.placement.count"),
            "300"
        )
        .is_err(),
        "a count is at most 255"
    );
}

/// The keys of `op`'s rows, in render order.
fn keys(op: &Operation) -> Vec<&'static str> {
    fields::rows(op).iter().map(|r| r.key).collect()
}

/// The index of the option tagged `tag` of choice row `row`.
fn option(row: &fields::Row, tag: &str) -> usize {
    match row.kind {
        FieldKind::Choice(options) => options
            .iter()
            .position(|o| o.tag == tag)
            .unwrap_or_else(|| panic!("`{}` has no option `{tag}`", row.key)),
        _ => panic!("`{}` is not a choice", row.key),
    }
}

/// Drill `id` of the harness's project, as the saved file reads back
/// through `dc_project::load`, the CLI's reader.
fn saved_drill(h: &dc_app::test_support::Harness, id: &str) -> DrillOp {
    let text = h.save().expect("the project saves");
    let loaded = dc_project::load(text.as_bytes()).expect("and the CLI reads it");
    match loaded
        .project
        .operations
        .into_iter()
        .find(|o| o.id().as_str() == id)
    {
        Some(Operation::Drill(drill)) => drill,
        other => panic!("the Drill is saved: {other:?}"),
    }
}

/// Capability 68's Drill line (mock-up 07): depth, retract height and
/// through-cut allowance under Depth; the cycle, then dwell and *allow
/// tool wider than hole*, under Strategy; safe Z under Heights. The cycle
/// is a variant: Simple shows nothing more; Peck shows its peck depth
/// (the tool's step-down while `null`) and its retract, itself a variant,
/// Full or Chip break with a gap; Countersink shows its mouth diameter,
/// start depth and *start plane cleared externally*, and its derived depth
/// in place of the typed one. Choosing a variant writes its defaults (the
/// chapter's: a full retract, a 0.5 mm gap, a 6 mm mouth at the stock
/// top), one undo step each, and never a depth (S20); the saved file
/// reads back each choice. A countersink never offers *allow tool wider
/// than hole*, which it ignores, unless the file sets it, when the row
/// shows with validation's warning so it can be cleared. The Bore's line:
/// depth, pitch (the tool's step-down while `null`), stock to leave and
/// through-cut allowance under Depth; direction under Strategy.
#[test]
fn drill_cycle_fields_follow_the_cycle() {
    let strings = Strings::english();
    let mut h = dc_app::test_support::Harness::from_project(project());
    let hole = vec![dc_project::InstanceId::new("i4")];
    let command = operations::add(h.app.project(), &strings, Kind::Drill, &hole)
        .expect("a circle takes a Drill");
    h.run(command).unwrap();
    let id = h
        .app
        .project()
        .operations
        .last()
        .unwrap()
        .id()
        .as_str()
        .to_owned();
    // The sign's 1/8 end mill steps 2 mm.
    let new = op_of(&h, &id);
    run_edit(
        &mut h,
        editor::set_tool(&new, row_of(&new, "tool"), &dc_project::ToolId::new("t3")),
    );
    let simple = op_of(&h, &id);
    let simple_keys = [
        "name",
        "enabled",
        "targets",
        "tool",
        "depth_mm",
        "retract_mm",
        "through_cut_allowance_mm",
        "cycle",
        "dwell_s",
        "allow_tool_wider_than_hole",
        "safe_z_mm",
    ];
    assert_eq!(keys(&simple), simple_keys, "a simple Drill");
    for (key, group) in [
        ("depth_mm", Group::Depth),
        ("retract_mm", Group::Depth),
        ("through_cut_allowance_mm", Group::Depth),
        ("cycle", Group::Strategy),
        ("dwell_s", Group::Strategy),
        ("allow_tool_wider_than_hole", Group::Strategy),
        ("safe_z_mm", Group::Heights),
    ] {
        assert_eq!(row_of(&simple, key).group, group, "`{key}`");
    }
    assert_eq!(row_of(&simple, "dwell_s").unit(), fields::Unit::Seconds);
    assert_eq!(row_of(&simple, "retract_mm").unit(), fields::Unit::Length);
    let cycle = row_of(&simple, "cycle");
    assert_eq!(
        editor::choice(&simple, cycle),
        Some(option(cycle, "simple"))
    );
    let drill = |h: &dc_app::test_support::Harness| match op_of(h, &id) {
        Operation::Drill(drill) => drill,
        other => panic!("still a Drill: {other:?}"),
    };

    // Peck: the tool's step-down, a full retract.
    let label = {
        let edit = editor::choose(h.app.project(), &simple, cycle, option(cycle, "peck"));
        run_edit(&mut h, edit)
    };
    assert!(label.starts_with("Set cycle of"), "{label}");
    assert_eq!(
        drill(&h).cycle,
        DrillCycle::Peck {
            peck_depth_mm: None,
            retract: PeckRetract::Full
        }
    );
    assert_eq!(drill(&h).depth_mm, None, "no depth of the app's own (S20)");
    let peck = op_of(&h, &id);
    assert_eq!(
        keys(&peck),
        [
            "name",
            "enabled",
            "targets",
            "tool",
            "depth_mm",
            "retract_mm",
            "through_cut_allowance_mm",
            "cycle",
            "cycle.peck.peck_depth_mm",
            "cycle.peck.retract",
            "dwell_s",
            "allow_tool_wider_than_hole",
            "safe_z_mm",
        ]
    );
    let peck_depth = row_of(&peck, "cycle.peck.peck_depth_mm");
    assert_eq!(peck_depth.group, Group::Strategy);
    assert_eq!(peck_depth.tool_default, Some(fields::ToolDefault::StepDown));
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &peck, peck_depth).as_deref(),
        Some("tool's: 2 mm")
    );
    {
        let edit = editor::commit_text(h.app.project(), &peck, peck_depth, "3");
        run_edit(&mut h, edit)
    };
    let pecked = op_of(&h, &id);
    assert_eq!(
        drill(&h).cycle,
        DrillCycle::Peck {
            peck_depth_mm: Some(3.0),
            retract: PeckRetract::Full
        }
    );
    run_edit(&mut h, editor::reset(&pecked, peck_depth));
    assert_eq!(op_of(&h, &id), peck, "⟲ writes null again");

    // The retract is a variant inside the cycle's: Chip break writes the
    // chapter's 0.5 mm gap, and its gap row comes with it.
    let retract = row_of(&peck, "cycle.peck.retract");
    assert_eq!(retract.group, Group::Strategy);
    assert_eq!(
        editor::choice(&peck, retract),
        Some(option(retract, "full"))
    );
    {
        let edit = editor::choose(
            h.app.project(),
            &peck,
            retract,
            option(retract, "chip_break"),
        );
        run_edit(&mut h, edit)
    };
    assert_eq!(
        drill(&h).cycle,
        DrillCycle::Peck {
            peck_depth_mm: None,
            retract: PeckRetract::ChipBreak { gap_mm: 0.5 }
        }
    );
    let chip = op_of(&h, &id);
    let gap = row_of(&chip, "cycle.peck.retract.chip_break.gap_mm");
    assert_eq!(gap.unit(), fields::Unit::Length);
    {
        let edit = editor::commit_text(h.app.project(), &chip, gap, "0.8");
        run_edit(&mut h, edit)
    };
    assert_eq!(
        saved_drill(&h, &id).cycle,
        DrillCycle::Peck {
            peck_depth_mm: None,
            retract: PeckRetract::ChipBreak { gap_mm: 0.8 }
        },
        "the CLI reads the chip break back"
    );
    let chip = op_of(&h, &id);
    {
        let edit = editor::choose(h.app.project(), &chip, retract, option(retract, "full"));
        run_edit(&mut h, edit)
    };
    assert!(
        keys(&op_of(&h, &id))
            .iter()
            .all(|k| !k.starts_with("cycle.peck.retract.")),
        "Full has no gap"
    );

    // Countersink: the chapter's 6 mm mouth at the stock top; the typed
    // depth and *allow tool wider than hole* give way to the derived depth.
    let full = op_of(&h, &id);
    {
        let edit = editor::choose(h.app.project(), &full, cycle, option(cycle, "countersink"));
        run_edit(&mut h, edit)
    };
    assert_eq!(
        drill(&h).cycle,
        DrillCycle::Countersink {
            diameter_mm: 6.0,
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false
        }
    );
    assert_eq!(drill(&h).depth_mm, None, "no depth of the app's own (S20)");
    let sink = op_of(&h, &id);
    assert_eq!(
        keys(&sink),
        [
            "name",
            "enabled",
            "targets",
            "tool",
            "cycle.countersink",
            "retract_mm",
            "through_cut_allowance_mm",
            "cycle",
            "cycle.countersink.diameter_mm",
            "cycle.countersink.start_depth_mm",
            "cycle.countersink.start_plane_cleared_externally",
            "dwell_s",
            "safe_z_mm",
        ]
    );
    assert_eq!(row_of(&sink, "cycle.countersink").group, Group::Depth);
    for key in [
        "cycle.countersink.diameter_mm",
        "cycle.countersink.start_depth_mm",
        "cycle.countersink.start_plane_cleared_externally",
    ] {
        assert_eq!(row_of(&sink, key).group, Group::Strategy, "`{key}`");
    }
    assert_eq!(
        saved_drill(&h, &id).cycle,
        drill(&h).cycle,
        "the CLI reads the countersink back"
    );

    // A file's `allow_tool_wider_than_hole: true` on a countersink shows,
    // with validation's warning under it, until it is cleared.
    let widened = fields::set(&sink, "allow_tool_wider_than_hole", Value::Bool(true)).unwrap();
    h.run(Command::UpdateOperation(widened.clone())).unwrap();
    let allow = row_of(&widened, "allow_tool_wider_than_hole");
    let status = editor::Located::of(h.warnings(), &widened)
        .status(allow)
        .expect("countersink_ignores_tool_width is under the row");
    assert_eq!(status.tier, Tier::Warning);
    run_edit(&mut h, editor::set_checked(&widened, allow, false));
    assert!(
        keys(&op_of(&h, &id))
            .iter()
            .all(|k| *k != "allow_tool_wider_than_hole"),
        "cleared, it is not offered again"
    );

    // Simple again: no cycle rows, the typed depth back, still unset.
    let sink = op_of(&h, &id);
    {
        let edit = editor::choose(h.app.project(), &sink, cycle, option(cycle, "simple"));
        run_edit(&mut h, edit)
    };
    assert_eq!(keys(&op_of(&h, &id)), simple_keys);
    let saved = saved_drill(&h, &id);
    assert_eq!(saved.cycle, DrillCycle::Simple);
    assert_eq!(saved.depth_mm, None);

    // The Bore's line.
    let command = operations::add(h.app.project(), &strings, Kind::Bore, &hole)
        .expect("a circle takes a Bore");
    h.run(command).unwrap();
    let bore = h.app.project().operations.last().unwrap().clone();
    assert_eq!(
        keys(&bore),
        [
            "name",
            "enabled",
            "targets",
            "tool",
            "depth_mm",
            "pitch_mm",
            "stock_to_leave_mm",
            "through_cut_allowance_mm",
            "direction",
            "safe_z_mm",
        ]
    );
    for (key, group) in [
        ("depth_mm", Group::Depth),
        ("pitch_mm", Group::Depth),
        ("stock_to_leave_mm", Group::Depth),
        ("through_cut_allowance_mm", Group::Depth),
        ("direction", Group::Strategy),
        ("safe_z_mm", Group::Heights),
    ] {
        assert_eq!(row_of(&bore, key).group, group, "`{key}`");
    }
    let pitch = row_of(&bore, "pitch_mm");
    assert_eq!(pitch.tool_default, Some(fields::ToolDefault::StepDown));
    {
        let edit = editor::set_tool(&bore, row_of(&bore, "tool"), &dc_project::ToolId::new("t3"));
        run_edit(&mut h, edit)
    };
    let bore = h.app.project().operations.last().unwrap().clone();
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &bore, pitch).as_deref(),
        Some("tool's: 2 mm")
    );
}

/// S18 and capability 68's Drill line (mock-up 07): a countersink's depth
/// is derived from its mouth and the V-bit's angle, `start_depth_mm +
/// diameter_mm / (2 · tan(α/2))` below the stock top (the depth `export`
/// states), and shown read-only in the Depth group where the typed depth
/// would be; it is never typed and never stored, so the file's `depth_mm`
/// stays `null`. It follows the mouth, the start depth and the display
/// units. With a tool that is not a V-bit there is no depth: the row shows
/// validation's message instead of a number.
#[test]
fn countersink_depth_is_derived_and_read_only() {
    let strings = Strings::english();
    let mut h = dc_app::test_support::Harness::from_project(project());
    // The sign's Countersinks: a 6 mm mouth 2.5 mm down, the 90° V-bit.
    let sink = op_of(&h, "op5");
    let Operation::Drill(drill) = &sink else {
        panic!("op5 is a Drill");
    };
    assert!(drill.cycle.is_countersink());
    assert_eq!(drill.depth_mm, None);
    assert!(
        keys(&sink).iter().all(|k| *k != "depth_mm"),
        "the typed depth is not offered"
    );
    let derived = row_of(&sink, "cycle.countersink");
    assert!(matches!(derived.kind, FieldKind::CountersinkDepth));
    assert_eq!(derived.group, Group::Depth);
    assert_eq!(strings.get(derived.label), "Depth");
    assert_eq!(derived.unit(), fields::Unit::Length);
    let depth_group = fields::groups(&sink)
        .into_iter()
        .find(|(g, _)| *g == Group::Depth)
        .expect("a Depth group")
        .1;
    assert_eq!(
        depth_group[0].key, "cycle.countersink",
        "first, as Depth is"
    );

    let tip = 2.5 + dc_project::countersink_depth_mm(6.0, 90.0).unwrap();
    let at = editor::countersink_depth_mm(h.app.project(), &sink).expect("a V-bit's depth");
    assert!((at - tip).abs() < 1e-9, "{at} = {tip}");
    assert_eq!(
        editor::derived(h.app.project(), &strings, h.warnings(), &sink, derived),
        Some(Ok("derived: 5.5".to_owned()))
    );
    let mut inch = h.app.project().clone();
    inch.display_units = Units::Inch;
    assert_eq!(
        editor::derived(&inch, &strings, h.warnings(), &sink, derived),
        Some(Ok(format!(
            "derived: {}",
            dc_app::units::field_text(tip, Units::Inch)
        )))
    );
    // Only the derived row is derived.
    assert_eq!(
        editor::derived(
            h.app.project(),
            &strings,
            h.warnings(),
            &sink,
            row_of(&sink, "retract_mm")
        ),
        None
    );

    // Read-only: nothing typed, no placeholder, nothing to reset.
    assert!(editor::commit_text(h.app.project(), &sink, derived, "4").is_err());
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &sink, derived),
        None
    );
    assert!(!editor::can_reset(&sink, derived));
    assert!(editor::reset(&sink, derived).unwrap().is_none());

    // It follows the mouth and the start depth.
    {
        let edit = editor::commit_text(
            h.app.project(),
            &sink,
            row_of(&sink, "cycle.countersink.diameter_mm"),
            "4",
        );
        run_edit(&mut h, edit)
    };
    let sink = op_of(&h, "op5");
    assert_eq!(
        editor::derived(h.app.project(), &strings, h.warnings(), &sink, derived),
        Some(Ok("derived: 4.5".to_owned()))
    );
    {
        let edit = editor::commit_text(
            h.app.project(),
            &sink,
            row_of(&sink, "cycle.countersink.start_depth_mm"),
            "0",
        );
        run_edit(&mut h, edit)
    };
    let sink = op_of(&h, "op5");
    assert_eq!(
        editor::derived(h.app.project(), &strings, h.warnings(), &sink, derived),
        Some(Ok("derived: 2".to_owned()))
    );
    assert_eq!(
        saved_drill(&h, "op5").depth_mm,
        None,
        "the derived depth is shown, never stored"
    );

    // Not a V-bit: validation's message in place of the number.
    run_edit(
        &mut h,
        editor::set_tool(&sink, row_of(&sink, "tool"), &dc_project::ToolId::new("t3")),
    );
    let sink = op_of(&h, "op5");
    assert_eq!(editor::countersink_depth_mm(h.app.project(), &sink), None);
    let needs = h
        .warnings()
        .iter()
        .find(|f| f.field.as_deref() == Some("operations[op5].tool") && f.tier == Tier::Error)
        .expect("validation's countersink_needs_v_bit")
        .clone();
    assert!(needs.message.contains("use a V-bit"), "{}", needs.message);
    assert_eq!(
        editor::derived(h.app.project(), &strings, h.warnings(), &sink, derived),
        Some(Err(needs.message))
    );
    // Disabled, validation says nothing of it; the row still says why.
    let off = fields::set(&sink, "enabled", Value::Bool(false)).unwrap();
    assert_eq!(
        editor::derived(h.app.project(), &strings, &[], &off, derived),
        Some(Err(strings.get(StrKey::EditorDerivedNeedsVBit).to_owned()))
    );
    assert_eq!(saved_drill(&h, "op5").depth_mm, None);
}

/// Capability 67 and the issue: the generator's warnings about a target
/// the kind cannot cut name the operation and the shape but no field, and
/// the editor draws them under the Targets group: a Drill's
/// `drill_target_not_circle` (the plaque's square) and
/// `drill_hole_too_small` (a 5 mm hole and the 1/4 end mill), and a Bore's
/// `bore_no_room` (the same hole) and `bore_leaves_core` (the 20 mm
/// island). A closed Targets group counts them.
#[test]
fn drill_and_bore_target_warnings_land_under_targets() {
    let strings = Strings::english();
    let mut plaque = common::plaque();
    let targets: Vec<dc_project::InstanceId> = ["i4", "i5"]
        .into_iter()
        .map(dc_project::InstanceId::new)
        .collect();
    let Some(Operation::Drill(mut drill)) =
        operations::new_operation(&plaque, &strings, Kind::Drill, &targets)
    else {
        panic!("a new Drill");
    };
    drill.tool = dc_project::ToolId::new("t2");
    drill.depth_mm = Some(6.0);
    let drill_id = drill.id.as_str().to_owned();
    plaque.operations.push(Operation::Drill(drill));
    for op in &mut plaque.operations {
        if let Operation::Bore(bore) = op {
            bore.tool = dc_project::ToolId::new("t2");
            bore.targets = ["i5", "i3"]
                .into_iter()
                .map(dc_project::InstanceId::new)
                .collect();
        }
    }
    let mut h = dc_app::test_support::Harness::from_project(plaque);
    h.generate();
    for (id, codes) in [
        (
            drill_id.as_str(),
            ["drill_target_not_circle", "drill_hole_too_small"],
        ),
        ("op3", ["bore_no_room", "bore_leaves_core"]),
    ] {
        let op = op_of(&h, id);
        let mine: Vec<&dc_job::Finding> = h
            .warnings()
            .iter()
            .filter(|f| f.op.as_ref() == Some(op.id()) && f.field.is_none())
            .collect();
        for code in codes {
            assert!(
                mine.iter().any(|f| f.code.as_deref() == Some(code)),
                "{id}: generation warns {code}: {mine:?}"
            );
        }
        let located = editor::Located::of(h.warnings(), &op);
        let status = located
            .status(row_of(&op, "targets"))
            .expect("the warnings are under the Targets group");
        assert_eq!(status.tier, Tier::Warning);
        assert!(
            mine.iter().any(|f| f.message == status.message
                && f.code.as_deref().is_some_and(|c| codes.contains(&c))),
            "{id}: {}",
            status.message
        );
        let (tier, count) = located
            .group(Group::Targets)
            .expect("a closed Targets group shows them");
        assert_eq!(tier, Tier::Warning);
        assert!(count >= codes.len(), "{id}: {count}");
    }
}

// ── Slice 13 (#542): the V-Carve editor and the clearing-pass list. ───

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
    let lettering = op_of(&h, "op3");
    let list = row_of(&lettering, "clearing");
    assert!(matches!(list.kind, FieldKind::ClearingList));
    assert_eq!(list.group, Group::Strategy);
    assert!(clearing::passes(&lettering, list).is_empty());

    // The floor to clear: the flat depth is off at `null` until typed.
    let flat = row_of(&lettering, "flat_depth_mm");
    assert!(matches!(flat.kind, FieldKind::SwitchedLength));
    assert_eq!(flat.group, Group::Depth);
    assert!(!editor::switched(&lettering, flat));
    let typed = editor::commit_text(h.app.project(), &lettering, flat, "2.5");
    run_edit(&mut h, typed);
    assert!(editor::switched(&op_of(&h, "op3"), flat));

    for _ in 0..2 {
        let op = op_of(&h, "op3");
        let add = clearing::add(h.app.project(), &op, list);
        run_edit(&mut h, add);
    }
    let op = op_of(&h, "op3");
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
    run_edit(&mut h, drag);
    let op = op_of(&h, "op3");
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
    assert_eq!(pass_tools(&op_of(&h, "op3"), list), ["t1", "t3"]);
    assert_eq!(
        h.app.undo_label().map(str::to_owned),
        undo_before,
        "the drag was one step"
    );
    assert!(coded(&h, "clearing_not_descending").is_empty());

    let op = op_of(&h, "op3");
    assert!(clearing::remove(&op, list, 5).is_err(), "no such pass");
    let remove = clearing::remove(&op, list, 1);
    run_edit(&mut h, remove);
    assert_eq!(pass_tools(&op_of(&h, "op3"), list), ["t1"]);
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
    let background = op_of(&h, "op2");
    let list = row_of(&background, "clearing");
    assert_eq!(pass_tools(&background, list), ["t2"]);
    for _ in 0..2 {
        let op = op_of(&h, "op2");
        let add = clearing::add(h.app.project(), &op, list);
        run_edit(&mut h, add);
    }
    let op = op_of(&h, "op2");
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
    let background = op_of(&h, "op2");
    let list = row_of(&background, "clearing");
    let add = clearing::add(h.app.project(), &background, list);
    run_edit(&mut h, add);
    let op = op_of(&h, "op2");
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
    run_edit(&mut h, raster);
    let op = op_of(&h, "op2");
    let angle = clearing::rows(&op, list, 1)
        .into_iter()
        .find(|s| s.row.key == "strategy.raster.angle_deg")
        .expect("the raster's angle follows its strategy");
    let typed = angle.commit_text(&project, &op, "30");
    run_edit(&mut h, typed);
    let op = op_of(&h, "op2");
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
    run_edit(&mut h, picked);
    assert_eq!(pass_tools(&op_of(&h, "op2"), list), ["t2", "t1"]);
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

    let lettering = op_of(&h, "op3");
    let flat = row_of(&lettering, "flat_depth_mm");
    let typed = editor::commit_text(h.app.project(), &lettering, flat, "1");
    run_edit(&mut h, typed);
    assert!(summary(&h, "op3").is_empty(), "stale until it regenerates");
    h.generate();
    let edited = h.app.project().clone();
    let line = check(&edited, "op3");
    assert!(line[0].contains("clamped at a flat depth of"), "{line:?}");
    assert_eq!(summary(&h, "op3"), line);
}

/// The lettered inlay fixture: one Inlay (`op1`) cutting the letters D and
/// C into both boards from one set of values (#498): 4 mm deep, a 1 mm
/// glue gap, a 2 mm plug depth, an 8 mm margin, the 60° 6.35 mm V-bit, a
/// 12 mm male board the stock's size, and a cut-out with four tabs.
fn inlay_fixture() -> dc_app::test_support::Harness {
    dc_app::test_support::Harness::from_project(common::load_dcv(&common::fixture("dc.dcv")))
}

/// The texts of `notes`, and whether any is drawn as an alert.
fn texts(notes: &[editor::Note]) -> (Vec<String>, bool) {
    (
        notes.iter().map(|n| n.text.clone()).collect(),
        notes.iter().any(|n| n.alert),
    )
}

/// `check`'s `inlay_male_mirrored` for `project`, as the CLI prints it.
fn cli_mirrored(project: &Project) -> Vec<String> {
    let built = dc_job::build(
        project,
        dc_job::ProgramSplit::ProjectDefault,
        dc_app::test_support::DATE,
    );
    built
        .findings
        .warnings
        .iter()
        .chain(&built.findings.errors)
        .filter(|f| f.code.as_deref() == Some("inlay_male_mirrored"))
        .map(|f| f.message.clone())
        .collect()
}

/// Acceptance 76 (capabilities 68, 68a and 72), the editor's and the
/// Scene's half (the export dialog's rows are slice 18's). The fixture's
/// Inlay shows its values once (no side to choose) and a Male board
/// group. With the male board's thickness cleared, the generator's
/// `inlay_male_board_unset` sits under the thickness, the male board
/// carries no shapes and no runs, and every run is the female's. With it
/// set to 18 mm the male's runs come back tagged with their board, the
/// male board shows the design mirrored as the generator mirrors it, and
/// `inlay_male_mirrored` names the X the CLI prints (55 mm, the board's
/// middle). The derived line under Depth shows the male's start plane
/// (4 − 1 = 3 mm), its band floor (3 + 2 = 5 mm) and the flank's
/// engagement against the 60° bit's 5.4993 mm cone, red once a 3 mm plug
/// depth takes it to 6 mm; the base under the band is 18 − 5 = 13 mm,
/// red below 6 mm (`inlay_plug_base_thin`'s threshold). Width and height
/// show *same as stock* while `null` and write `null` back with ⟲.
#[test]
fn inlay_both_boards() {
    let strings = Strings::english();
    let mut h = inlay_fixture();
    let inlay = op_of(&h, "op1");

    // The values once, in capability 68's groups; no side.
    let shown: Vec<(Group, Vec<&str>)> = fields::groups(&inlay)
        .into_iter()
        .map(|(g, rows)| (g, rows.into_iter().map(|r| r.key).collect()))
        .collect();
    assert_eq!(
        shown,
        vec![
            (Group::Identity, vec!["name", "enabled"]),
            (Group::Targets, vec!["targets"]),
            (Group::Tool, vec!["tool"]),
            (
                Group::Depth,
                vec![
                    "depth_mm",
                    "glue_gap_mm",
                    "plug_depth_mm",
                    "margin_mm",
                    "outline_rounding_mm",
                    "through_cut_allowance_mm",
                    "male_board",
                ]
            ),
            (
                Group::MaleBoard,
                vec![
                    "male_board.width_mm",
                    "male_board.height_mm",
                    "male_board.thickness_mm",
                ]
            ),
            (Group::Strategy, vec!["step_over_fraction", "clearing"]),
            (
                Group::CutOut,
                vec![
                    "cut_out",
                    "cut_out.tool",
                    "cut_out.tabs",
                    "cut_out.tabs.placement",
                    "cut_out.tabs.placement.count",
                    "cut_out.tabs.width_mm",
                    "cut_out.tabs.height_mm",
                    "cut_out.tabs.shape",
                    "cut_out.tabs.on_holes",
                    "cut_out.tabs.avoid_corners",
                    "cut_out.through_cut_allowance_mm",
                ]
            ),
            (Group::Order, vec!["machining_order"]),
            (Group::Heights, vec!["safe_z_mm"]),
        ]
    );
    assert!(matches!(
        row_of(&inlay, "outline_rounding_mm").kind,
        FieldKind::SwitchedLength
    ));
    assert!(matches!(
        row_of(&inlay, "clearing").kind,
        FieldKind::ClearingList
    ));
    let derived = row_of(&inlay, "male_board");
    assert!(matches!(derived.kind, FieldKind::InlayDerived));
    assert!(editor::commit_text(h.app.project(), &inlay, derived, "4").is_err());
    // Labels unique within the editor (they are the controls' names).
    let mut labels: Vec<&str> = fields::rows(&inlay)
        .iter()
        .map(|r| strings.get(r.label))
        .collect();
    labels.sort_unstable();
    let count = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), count, "{labels:?}");

    // The cut-out's tool picker offers the project's end mills; the
    // Inlay's own, V-bits.
    let offered = |h: &dc_app::test_support::Harness, key: &str| -> Vec<String> {
        let op = op_of(h, "op1");
        editor::offered_for(h.app.project(), &op, row_of(&op, key))
            .iter()
            .map(|t| t.id.as_str().to_owned())
            .collect()
    };
    assert_eq!(offered(&h, "tool"), ["t3"]);
    assert_eq!(offered(&h, "cut_out.tool"), ["t1", "t2"]);
    // One bit for both boards, its cone under the picker.
    let (tool_note, _) = texts(&editor::notes(
        h.app.project(),
        &strings,
        &inlay,
        &editor::Slot::of(row_of(&inlay, "tool")),
    ));
    assert_eq!(
        tool_note,
        ["one bit for both boards · cone height 5.4993 mm"]
    );

    // Width and height: the stock's while `null`, typed, and back.
    let width = row_of(&inlay, "male_board.width_mm");
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &inlay, width).as_deref(),
        Some("same as stock: 110")
    );
    assert_eq!(
        editor::placeholder(
            h.app.project(),
            &strings,
            &inlay,
            row_of(&inlay, "male_board.height_mm")
        )
        .as_deref(),
        Some("same as stock: 80")
    );
    assert!(!editor::can_reset(&inlay, width));
    run_edit(
        &mut h,
        editor::commit_text(h.app.project(), &inlay, width, "120"),
    );
    let wide = op_of(&h, "op1");
    let Operation::Inlay(board) = &wide else {
        panic!("op1 is an Inlay");
    };
    assert_eq!(board.male_board.width_mm, Some(120.0));
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &wide, width),
        None
    );
    assert!(editor::can_reset(&wide, width));
    run_edit(&mut h, editor::reset(&wide, width));
    assert_eq!(op_of(&h, "op1"), inlay, "⟲ writes null again");

    // Thickness cleared: the generator's warning under the field, the
    // male board empty, every run the female's.
    let thickness = row_of(&inlay, "male_board.thickness_mm");
    assert_eq!(
        editor::placeholder(h.app.project(), &strings, &inlay, thickness),
        None,
        "required: no default"
    );
    run_edit(
        &mut h,
        editor::commit_text(h.app.project(), &inlay, thickness, ""),
    );
    let unset = op_of(&h, "op1");
    let Operation::Inlay(cleared) = &unset else {
        panic!("op1 is an Inlay");
    };
    assert_eq!(cleared.male_board.thickness_mm, None);
    h.generate();
    let warning = coded(&h, "inlay_male_board_unset");
    assert_eq!(warning.len(), 1, "{:?}", h.warnings());
    assert_eq!(
        warning[0].field.as_deref(),
        Some("operations[op1].male_board.thickness_mm")
    );
    let under = editor::Located::of(h.warnings(), &unset)
        .status(row_of(&unset, "male_board.thickness_mm"))
        .expect("a message under the thickness");
    assert_eq!(under.message, warning[0].message);
    let scene = h.app.scene();
    let board = scene.male_board().expect("the board is drawn");
    assert!(board.shapes.is_empty(), "the male board is empty");
    assert!(scene.runs().count() > 0);
    assert!(scene.runs().all(|r| r.board == dc_project::Board::Female));
    assert_eq!(scene.runs_on(dc_project::Board::Male).count(), 0);
    assert!(coded(&h, "inlay_male_mirrored").is_empty());
    // No base to show while the board is unmeasured.
    let (base, _) = texts(&editor::notes(
        h.app.project(),
        &strings,
        &unset,
        &editor::Slot::of(row_of(&unset, "male_board.thickness_mm")),
    ));
    assert!(base.is_empty(), "{base:?}");

    // Set to 18 mm: the male's runs, mirrored, on their board.
    run_edit(
        &mut h,
        editor::commit_text(h.app.project(), &unset, thickness, "18"),
    );
    h.generate();
    assert!(coded(&h, "inlay_male_board_unset").is_empty());
    let both = op_of(&h, "op1");
    let project = h.app.project().clone();
    let scene = h.app.scene();
    assert!(
        scene.runs_on(dc_project::Board::Male).count() > 0,
        "the male's runs"
    );
    assert!(scene.runs_on(dc_project::Board::Female).count() > 0);
    let board = scene.male_board().expect("the male board");
    assert_eq!(board.op, OperationId::new("op1"));
    assert!(!board.shapes.is_empty(), "the design, mirrored");
    let mirrored = coded(&h, "inlay_male_mirrored");
    assert_eq!(mirrored.len(), 1, "{:?}", h.warnings());
    assert_eq!(
        vec![mirrored[0].message.clone()],
        cli_mirrored(&project),
        "the X the CLI prints"
    );
    assert!(
        mirrored[0].message.contains("mirrored about X = 55 mm"),
        "{}",
        mirrored[0].message
    );

    // The derived line: start plane, band floor, engagement and cone.
    let planes = editor::male_planes(&project, &both).expect("every value is set");
    assert!((planes.start_plane_mm - 3.0).abs() < 1e-9);
    assert!((planes.band_floor_mm - 5.0).abs() < 1e-9);
    assert!((planes.engagement_mm - 5.0).abs() < 1e-9);
    let cone = 6.35 / (2.0 * 30f64.to_radians().tan());
    assert!((planes.cone_height_mm.unwrap() - cone).abs() < 1e-9);
    assert!(!planes.exceeds_cone());
    let line = |h: &dc_app::test_support::Harness| {
        let op = op_of(h, "op1");
        texts(&editor::notes(
            h.app.project(),
            &strings,
            &op,
            &editor::Slot::of(row_of(&op, "male_board")),
        ))
    };
    assert_eq!(
        line(&h),
        (
            vec![
                "male start plane 3 mm · band floor 5 mm below the board top".to_owned(),
                "flank engagement 5 mm of the 60° bit's 5.4993 mm cone".to_owned(),
            ],
            false
        )
    );
    let base = |h: &dc_app::test_support::Harness| {
        let op = op_of(h, "op1");
        texts(&editor::notes(
            h.app.project(),
            &strings,
            &op,
            &editor::Slot::of(row_of(&op, "male_board.thickness_mm")),
        ))
    };
    assert_eq!(
        base(&h),
        (
            vec!["base under the band 13 mm (at least 6 mm)".to_owned()],
            false
        )
    );
    assert_eq!(editor::male_base_mm(&both), Some(13.0));

    // A 3 mm plug depth: the male's flank engages 6 mm, past the cone.
    run_edit(
        &mut h,
        editor::commit_text(h.app.project(), &both, row_of(&both, "plug_depth_mm"), "3"),
    );
    let deep = op_of(&h, "op1");
    let planes = editor::male_planes(h.app.project(), &deep).unwrap();
    assert!((planes.engagement_mm - 6.0).abs() < 1e-9);
    assert!(planes.exceeds_cone());
    let (lines, alert) = line(&h);
    assert!(alert, "red when the engagement exceeds the cone: {lines:?}");
    assert_eq!(
        lines[1],
        "flank engagement 6 mm of the 60° bit's 5.4993 mm cone"
    );
    // Validation agrees: its cone error, on the plug depth.
    h.generate();
    assert!(
        h.warnings().iter().any(
            |f| f.field.as_deref() == Some("operations[op1].plug_depth_mm")
                && f.message.contains("cone")
        ),
        "{:?}",
        h.warnings()
    );

    // A 10 mm male board under a 6 mm floor leaves 4 mm: red, as
    // validation's `inlay_plug_base_thin` warns.
    run_edit(
        &mut h,
        editor::commit_text(h.app.project(), &deep, thickness, "10"),
    );
    let thin = op_of(&h, "op1");
    assert_eq!(editor::male_base_mm(&thin), Some(4.0));
    assert_eq!(
        base(&h),
        (
            vec!["base under the band 4 mm (at least 6 mm)".to_owned()],
            true
        )
    );
    h.generate();
    assert_eq!(coded(&h, "inlay_plug_base_thin").len(), 1);

    // A value missing: no planes, and the line says what to set.
    let mut bare = thin.clone();
    if let Operation::Inlay(i) = &mut bare {
        i.glue_gap_mm = None;
    }
    assert!(editor::male_planes(h.app.project(), &bare).is_none());
    let (lines, alert) = texts(&editor::notes(
        h.app.project(),
        &strings,
        &bare,
        &editor::Slot::of(row_of(&bare, "male_board")),
    ));
    assert_eq!(
        lines,
        ["set the inlay depth, glue gap and plug depth to see the male's planes"]
    );
    assert!(!alert);
}
