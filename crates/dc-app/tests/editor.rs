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

use dc_app::editor::{self, FieldKind, Group, fields};
use dc_app::operations::{self, Kind};
use dc_app::{Command, StrKey, Strings};
use dc_job::Tier;
use dc_project::{Entry, Operation, Project, Units};
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
        // Slice 13 (#542).
        Kind::VCarve => &[
            "flat_depth_mm",
            "start_depth_mm",
            "start_plane_cleared_externally",
            "through_cut_allowance_mm",
            "strategy",
            "strategy.offset.step_over_mm",
            "strategy.offset.direction",
            "step_over_fraction",
            "clearing",
            "machining_order",
        ],
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
            FieldKind::Toggle(_) | FieldKind::TabRequest => [true, false]
                .into_iter()
                .filter_map(|on| editor::switch(project, seed, row, on).ok().flatten())
                .filter_map(updated)
                .collect(),
            // None, or a Pocket to rest on: the file reads any id, and
            // validation says whether it is an earlier one.
            FieldKind::EarlierPocket => [None, Some(dc_project::OperationId::new("op1"))]
                .iter()
                .filter_map(|id| {
                    editor::set_earlier_pocket(seed, row, id.as_ref())
                        .ok()
                        .flatten()
                })
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
