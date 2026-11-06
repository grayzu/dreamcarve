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

use dc_app::editor::{self, FieldKind, Group, fields};
use dc_app::operations::{self, Kind};
use dc_app::{Command, StrKey, Strings};
use dc_job::Tier;
use dc_project::{Entry, Operation, Project, Units};
use serde_json::Value;

/// The leaves of each kind's records that have no row yet, with the slice
/// that adds them. A slice adding a kind's rows deletes its entries here,
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
            "cycle.peck.retract.chip_break.gap_mm",
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
            "cut_out.tool",
            "cut_out.tabs",
            "cut_out.through_cut_allowance_mm",
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

/// `seed` and every record the table's own choices and switches reach from
/// it, each row's every option tried in turn until nothing new appears.
fn reachable(project: &Project, seed: &Operation) -> Vec<Operation> {
    let mut found = vec![seed.clone()];
    let mut at = 0;
    while let Some(op) = found.get(at).cloned() {
        at += 1;
        for row in fields::rows(&op) {
            let next: Vec<Operation> = match row.kind {
                FieldKind::Choice(options) => (0..options.len())
                    .filter_map(|i| editor::choose(project, &op, row, i).ok().flatten())
                    .filter_map(updated)
                    .collect(),
                FieldKind::Toggle(_) => [true, false]
                    .into_iter()
                    .filter_map(|on| editor::switch(project, &op, row, on).ok().flatten())
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
                let covered = fields::rows(op).iter().any(|row| covers(row, &leaf));
                assert!(
                    covered || pending.contains(&leaf.as_str()),
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
        for op in &records {
            round_trip_every_row(&project, op);
        }
    }
}

/// Each value-holding row of `op`: a different value through the editor,
/// saved and loaded, reads back; set back, the record is `op` again.
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
        FieldKind::InstanceList => {
            let first = project.instances.first().expect("the sign has shapes");
            Value::from(vec![first.id.as_str()])
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
    assert_eq!(
        editor::group_status(harness.warnings(), &project, &profile, Group::Depth),
        Some((Tier::Error, 1))
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
        fields::row_for(&with_last, "operations[elsewhere].depth_mm"),
        None
    );

    // A tool-derived row takes its tool's field while it is null.
    let tool = profile.tool().clone();
    let step_down = fields::row_for(&profile, &format!("tools[{tool}].step_down_mm"))
        .expect("the step-down row takes the tool's step-down");
    assert_eq!(step_down.key, "step_down_mm");
    let own = fields::set(&profile, "step_down_mm", Value::from(1.0)).unwrap();
    assert_eq!(
        fields::row_for(&own, &format!("tools[{tool}].step_down_mm")),
        None,
        "not once the operation has its own"
    );
}
