// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 9 of phase 4 (`docs/specs/phase-4/design.md`, "Digest and
//! staleness"; #538): which operations regenerate after an edit, which
//! results are kept, and what Stop and Regenerate all do (functional
//! capability 71, acceptance 87). The worker thread is the shell's; these
//! run the jobs inline through the harness, with the real `dc_job`
//! functions.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use dc_app::generation::{self, Work};
use dc_app::test_support::Harness;
use dc_app::{Command, JobResult};
use dc_geometry::{Point, Polyline, Transform};
use dc_job::Generated;
use dc_project::test_support::{
    sample_bore, sample_engrave, sample_inlay, sample_pocket, sample_project, sample_tab,
    sample_v_carve, tool_v_bit_90,
};
use dc_project::{
    DefinitionId, DrillCycle, FeatureDefinition, FeatureInstance, InstanceId, Operation,
    OperationId, Project, ToolId,
};
use serde_json::Value;

use common::{golden_projects, load_dcv, quick_start_projects};

/// A definition `id` with `profile`, placed once as instance `instance`
/// at (`x_mm`, `y_mm`).
fn place(project: &mut Project, id: &str, instance: &str, profile: Polyline, x_mm: f64, y_mm: f64) {
    let definition = DefinitionId::new(id);
    project.definitions.insert(
        definition.clone(),
        FeatureDefinition {
            id: definition.clone(),
            name: id.to_owned(),
            profiles: vec![profile],
        },
    );
    project.instances.push(FeatureInstance {
        id: InstanceId::new(instance),
        definition,
        transform: Transform::translate(x_mm, y_mm),
        name: instance.to_owned(),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
}

/// Every kind of operation, and every way one operation reads another,
/// on small shapes so each generates in moments. On the sample's
/// 100 × 100 × 6 mm stock, its 50 × 30 mm rectangle `i1` at (10, 10) and
/// its 6.35 mm end mill `t1`, plus a 90° V-bit `t2`, a 3.175 mm end mill
/// `t3`, a 4 mm square `i3` inside the rectangle, a 3 mm hole `i2` and a
/// 10 mm hole `i4`. In cut order:
///
/// - `o2` a 3 mm Pocket of the rectangle with `t1`;
/// - `o8` a rest Pocket of it with `t3`, reading `o2` (`rest_of`);
/// - `o3` a countersink Drill of the 3 mm hole with the V-bit, which reads
///   the floors before it;
/// - `o5` a Bore of the 10 mm hole with `t3`;
/// - `o4` an Engrave of the 10 mm hole with `t3`;
/// - `o1` the sample's outside Profile of the rectangle, through, with one
///   placed tab and no tab request;
/// - `o6` a V-Carve of the square from 3 mm down, its start plane the
///   floor of `o2` (`cleared_start_plane`);
/// - `o7` an Inlay of the square with the V-bit, its female only.
///
/// The two carves come last: they read every operation before them, so
/// few mutations leave their digest alone, and each of their
/// generations costs the most.
pub fn every_kind() -> Project {
    let mut project = sample_project();
    // Shown in inches, so the carves snap to the inch grid the
    // post-processor's `decimal_places.inch` sets (`WrittenGrid`): a
    // generator input outside the project's geometry.
    project.display_units = dc_project::Units::Inch;
    project.tools.push(tool_v_bit_90());
    let mut small = project.tools[0].clone();
    small.id = ToolId::new("t3");
    small.name = "3.175 mm upcut".into();
    small.diameter_mm = 3.175;
    project.tools.push(small);
    place(
        &mut project,
        "d3",
        "i3",
        Polyline::rect(Point::new(0.0, 0.0), 4.0, 4.0),
        30.0,
        20.0,
    );
    place(
        &mut project,
        "d2",
        "i2",
        Polyline::circle(Point::new(0.0, 0.0), 1.5),
        80.0,
        80.0,
    );
    place(
        &mut project,
        "d4",
        "i4",
        Polyline::circle(Point::new(0.0, 0.0), 5.0),
        80.0,
        30.0,
    );

    let Some(Operation::Profile(mut profile)) = project.operations.pop() else {
        panic!("the sample starts with its Profile");
    };
    profile.tabs = None;
    project.tabs.push(sample_tab("tab1", 25.0, 0.0));

    let rough = sample_pocket();
    let mut rest = sample_pocket();
    rest.id = OperationId::new("o8");
    rest.name = "Rest".into();
    rest.tool = ToolId::new("t3");
    rest.rest_of = Some(rough.id.clone());

    let mut carve = sample_v_carve();
    carve.targets = vec![InstanceId::new("i3")];
    carve.start_depth_mm = 3.0;

    let mut countersink = dc_project::test_support::sample_drill();
    countersink.targets = vec![InstanceId::new("i2")];
    countersink.tool = ToolId::new("t2");
    countersink.cycle = DrillCycle::Countersink {
        diameter_mm: 5.0,
        start_depth_mm: 0.0,
        start_plane_cleared_externally: false,
    };

    let mut bore = sample_bore();
    bore.targets = vec![InstanceId::new("i4")];
    bore.tool = ToolId::new("t3");

    let mut engrave = sample_engrave();
    engrave.targets = vec![InstanceId::new("i4")];
    engrave.tool = ToolId::new("t3");

    let mut inlay = sample_inlay();
    inlay.targets = vec![InstanceId::new("i3")];

    project.operations = vec![
        Operation::Pocket(rough),
        Operation::Pocket(rest),
        Operation::Drill(countersink),
        Operation::Bore(bore),
        Operation::Engrave(engrave),
        Operation::Profile(profile),
        Operation::VCarve(carve),
        Operation::Inlay(inlay),
    ];
    project
}

/// The ids of the operations `jobs` name, in order.
fn ops(jobs: &[dc_app::Job]) -> Vec<String> {
    jobs.iter().map(|j| j.op.to_string()).collect()
}

/// `id` as an `OperationId`.
fn op(id: &str) -> OperationId {
    OperationId::new(id)
}

/// The every-kind project, validated and generated once.
fn generated_every_kind() -> Harness {
    let project = every_kind();
    let validation = dc_job::validation(&project);
    assert!(
        !validation.has_errors(),
        "the fixture validates: {:?}",
        validation.errors
    );
    let mut harness = Harness::from_project(project);
    harness.generate();
    assert!(harness.app.pending().is_empty(), "everything generated");
    harness
}

/// The leaves of `value` to mutate, as JSON pointers. A definition's
/// vertex lists contribute their first vertex only: moving one vertex
/// moves the outline, and the rest would add time and nothing else.
fn leaves(value: &Value, at: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                leaves(inner, &format!("{at}/{key}"), out);
            }
        }
        Value::Array(items) => {
            let shape = at.contains("/profiles/");
            for (i, inner) in items.iter().enumerate() {
                if shape && i > 0 && !inner.is_number() {
                    break;
                }
                leaves(inner, &format!("{at}/{i}"), out);
            }
        }
        _ => out.push(at.to_owned()),
    }
}

/// The last key of `pointer` that is not an array index.
fn key_of(pointer: &str) -> &str {
    pointer
        .rsplit('/')
        .find(|step| step.parse::<usize>().is_err())
        .unwrap_or("")
}

/// The keys whose string values name other records: changing one breaks
/// a reference, which validation rejects, so they are not mutated.
const REFERENCES: [&str; 7] = [
    "id",
    "targets",
    "tool",
    "rest_of",
    "definition",
    "instance",
    "version",
];

/// The other values a mutation tries for a leaf at `pointer` holding
/// `leaf`: a number moved, a flag flipped, a name lengthened, an unset
/// value set to 1, and an enum's every other value seen under its key in
/// `alternatives`.
fn mutations(
    pointer: &str,
    leaf: &Value,
    alternatives: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Value> {
    let key = key_of(pointer);
    match leaf {
        Value::Bool(b) => vec![Value::Bool(!b)],
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                // Up one, down one, and to nothing: a post-processor's
                // decimal places at 0 make the written grid 1 mm, which
                // moves what a generator snaps to it.
                let mut moved = vec![Value::from(i + 1)];
                if i > 0 {
                    moved.push(Value::from(i - 1));
                }
                if i > 1 {
                    moved.push(Value::from(0));
                }
                moved
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                vec![Value::from(f * 1.1 + 0.05)]
            }
        }
        Value::Null => vec![Value::from(1.0)],
        Value::String(s) if REFERENCES.contains(&key) => {
            let _ = s;
            Vec::new()
        }
        Value::String(s) if key.ends_with("name") => vec![Value::from(format!("{s} 2"))],
        Value::String(s) => alternatives
            .get(key)
            .into_iter()
            .flatten()
            .filter(|other| *other != s)
            .map(|other| Value::from(other.as_str()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every string value under each key, over `projects`.
fn string_values(projects: &[Value]) -> BTreeMap<String, BTreeSet<String>> {
    fn walk(value: &Value, key: &str, out: &mut BTreeMap<String, BTreeSet<String>>) {
        match value {
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(v, k, out)),
            Value::Array(items) => items.iter().for_each(|v| walk(v, key, out)),
            Value::String(s) => {
                out.entry(key.to_owned()).or_default().insert(s.clone());
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for project in projects {
        walk(project, "", &mut out);
    }
    out
}

/// The design's rule (`docs/specs/phase-4/design.md`, "Digest and
/// staleness"): the digest covers everything an operation's generator
/// reads, so a result whose digest is current is the result the
/// generator would give now. Each leaf of the every-kind project and of a
/// rest-machining golden case is mutated in turn (every number, flag,
/// name, unset value and enum, a definition's first vertex); for each
/// mutation validation accepts, each operation whose digest did not
/// change must generate exactly what it generated before. A missing
/// input (a tool, a target's definition, the post-processor's decimal
/// places, an earlier operation it reads) fails here, naming the leaf.
/// Over-invalidation is allowed, so a changed digest asserts nothing.
#[test]
fn digest_covers_every_generator_input() {
    let mut corpus: Vec<Value> = quick_start_projects()
        .into_iter()
        .chain(golden_projects())
        .map(|(_, path)| serde_json::to_value(load_dcv(&path)).unwrap())
        .collect();
    let rest_case = golden_projects()
        .into_iter()
        .find(|(name, _)| name == "pocket-rough-then-rest")
        .map(|(_, path)| load_dcv(&path))
        .expect("the rest-machining golden case");
    let fixtures = [("every-kind", every_kind()), ("rough-then-rest", rest_case)];
    corpus.extend(
        fixtures
            .iter()
            .map(|(_, p)| serde_json::to_value(p).unwrap()),
    );
    let alternatives = string_values(&corpus);

    let (mut tried, mut regenerated, mut changed) = (0, 0, 0);
    for (name, base) in fixtures {
        assert!(!dc_job::validation(&base).has_errors(), "{name} validates");
        let ids: Vec<OperationId> = base
            .operations
            .iter()
            .filter(|o| o.enabled())
            .map(|o| o.id().clone())
            .collect();
        let before: BTreeMap<OperationId, (u64, Generated)> = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    (
                        generation::digest(&base, id),
                        dc_job::generate_operation(&base, id),
                    ),
                )
            })
            .collect();
        let value = serde_json::to_value(&base).unwrap();
        let mut pointers = Vec::new();
        leaves(&value, "", &mut pointers);
        for pointer in pointers {
            let leaf = value.pointer(&pointer).unwrap();
            for replacement in mutations(&pointer, leaf, &alternatives) {
                let mut mutated = value.clone();
                *mutated.pointer_mut(&pointer).unwrap() = replacement.clone();
                let Ok(project) = serde_json::from_value::<Project>(mutated) else {
                    continue;
                };
                if dc_job::validation(&project).has_errors() {
                    continue;
                }
                tried += 1;
                for id in &ids {
                    let (digest, generated) = &before[id];
                    if generation::digest(&project, id) != *digest {
                        continue;
                    }
                    regenerated += 1;
                    let now = dc_job::generate_operation(&project, id);
                    if now != *generated {
                        changed += 1;
                    }
                    assert!(
                        now == *generated,
                        "{name}: setting {pointer} to {replacement} changes what `{id}` \
                         generates but not its digest"
                    );
                }
            }
        }
    }
    // The walk exercised the fixture: most mutations validate, and some
    // left an operation's digest alone, so its output was compared.
    assert!(tried > 100, "{tried} mutations validated");
    assert!(
        regenerated > 100,
        "{regenerated} unchanged digests compared"
    );
    assert_eq!(changed, 0);
}

/// Acceptance 87 and the design's dependency rule: after an edit only the
/// operations whose inputs it touched are pending. An Engrave reads its
/// own record, tool and targets, so editing it regenerates it and the two
/// carves after it (which read every earlier operation's record);
/// editing the rough Pocket regenerates it, the rest Pocket that rests on
/// it, the countersink (which reads earlier floors), and the two carves,
/// whose start plane is its floor. A post-processor setting no generator
/// reads regenerates nothing, and `finish` reruns after every change,
/// since the order check reads everything.
#[test]
fn editing_one_operation_regenerates_only_dependents() {
    let mut harness = generated_every_kind();
    assert!(harness.app.generation().finished().is_some());

    let mut engrave = harness.app.project().operations[4].clone();
    let Operation::Engrave(e) = &mut engrave else {
        panic!("o4 is the Engrave");
    };
    e.depth_mm = Some(0.75);
    harness.run(Command::UpdateOperation(engrave)).unwrap();
    assert_eq!(ops(&harness.app.pending()), ["o4", "o6", "o7"]);
    for id in ["o2", "o8", "o3", "o5", "o1"] {
        assert!(
            !harness.app.generation().is_stale(&op(id)),
            "{id} keeps its result"
        );
    }
    assert!(harness.app.generation().is_stale(&op("o4")));
    assert!(harness.app.generation().finished().is_none());
    harness.generate();
    assert!(harness.app.pending().is_empty());
    assert!(
        harness.app.generation().finished().is_some(),
        "finish reruns"
    );

    let mut rough = harness.app.project().operations[0].clone();
    let Operation::Pocket(p) = &mut rough else {
        panic!("o2 is the rough Pocket");
    };
    p.step_down_mm = Some(1.5);
    harness.run(Command::UpdateOperation(rough)).unwrap();
    assert_eq!(ops(&harness.app.pending()), ["o2", "o8", "o3", "o6", "o7"]);
    harness.generate();

    let mut post = harness.app.project().post_processor.clone();
    post.spindle_on_dwell_s += 1.0;
    harness.run(Command::SetPostSettings(post)).unwrap();
    assert!(
        harness.app.pending().is_empty(),
        "no generator reads the dwell"
    );
    assert!(harness.app.needs_finish(), "but finish reruns");
    harness.generate();
    assert!(harness.app.generation().finished().is_some());

    // Undo is a new snapshot too: back to digests whose results are kept.
    harness.app.undo();
    assert!(harness.app.pending().is_empty());
    assert!(harness.app.needs_finish());
}

/// The design's rule: a result whose snapshot is not the current one is
/// dropped, even when it arrives after the edit, so a late answer never
/// replaces what the current project generates; the operation is pending
/// again and the Scene keeps drawing it stale.
#[test]
fn results_from_an_old_snapshot_are_dropped() {
    let mut harness = Harness::from_project(every_kind());
    let Some(Work::Operation(job)) = harness.app.next_work() else {
        panic!("the first work is an operation");
    };
    assert_eq!(job.op, op("o2"));
    let project = harness.app.project().clone();
    let old = generation::execute(
        &project,
        Work::Operation(job.clone()),
        dc_app::test_support::DATE,
        &dc_job::EstimateConfig::default(),
    );
    let mut stock = project.stock.clone();
    stock.width_mm = 120.0;
    harness.run(Command::SetStock(stock)).unwrap();
    assert!(
        !harness.app.accept(old),
        "the old snapshot's result is dropped"
    );
    assert!(harness.app.generation().result(&job.op).is_none());
    assert!(harness.app.generation().is_stale(&job.op));
    assert!(ops(&harness.app.pending()).contains(&"o2".to_owned()));
    assert!(
        harness.app.scene().operations().iter().all(|o| o.stale),
        "the Scene draws every operation stale"
    );
    // An old `finish` is dropped too.
    let finished = JobResult::Finished {
        snapshot: job.snapshot,
        finished: dc_job::Finished {
            findings: dc_job::Findings::default(),
            programs: Vec::new(),
            total_line: None,
        },
    };
    assert!(!harness.app.accept(finished));
}

/// Acceptance 87's Stop (design open question 4): Stop clears what is
/// pending at once; the job the worker is running cannot be interrupted,
/// so its result is dropped when it arrives, and the operations stay
/// stale. An edit afterwards regenerates what it touched; Regenerate all
/// regenerates everything, results already current included.
#[test]
fn stop_clears_pending() {
    let mut harness = Harness::from_project(every_kind());
    let Some(Work::Operation(running)) = harness.app.next_work() else {
        panic!("an operation runs first");
    };
    assert!(harness.app.generation().is_regenerating(&running.op));
    assert!(harness.app.generation().progress().is_some());

    harness.app.stop();
    assert!(harness.app.pending().is_empty(), "Stop clears pending");
    assert!(
        harness.app.next_work().is_none(),
        "and nothing is handed out"
    );
    assert!(harness.app.generation().progress().is_none());
    assert!(!harness.app.generation().busy());
    let project = harness.app.project().clone();
    for operation in &project.operations {
        assert!(
            !harness.app.generation().is_regenerating(operation.id()),
            "{} shows stopped, not spinning",
            operation.id()
        );
        assert!(harness.app.generation().is_stale(operation.id()));
    }
    let late = generation::execute(
        &project,
        Work::Operation(running.clone()),
        dc_app::test_support::DATE,
        &dc_job::EstimateConfig::default(),
    );
    assert!(
        !harness.app.accept(late),
        "the running job's result is dropped"
    );
    assert!(harness.app.generation().result(&running.op).is_none());

    // An edit regenerates what it touched (and what reads it).
    let mut engrave = project.operations[4].clone();
    let Operation::Engrave(e) = &mut engrave else {
        panic!("o4 is the Engrave");
    };
    e.depth_mm = Some(0.75);
    harness.run(Command::UpdateOperation(engrave)).unwrap();
    assert_eq!(ops(&harness.app.pending()), ["o4", "o6", "o7"]);
    harness.generate();
    assert!(!harness.app.generation().is_stale(&op("o4")));
    assert!(
        harness.app.generation().is_stale(&op("o2")),
        "still stopped"
    );

    // Regenerate all forces every operation, current or not.
    harness.app.regenerate_all();
    assert_eq!(
        ops(&harness.app.pending()),
        ["o2", "o8", "o3", "o5", "o4", "o1", "o6", "o7"]
    );
    harness.generate();
    assert!(harness.app.pending().is_empty());
    assert!(harness.app.generation().finished().is_some());
    for operation in &project.operations {
        assert!(!harness.app.generation().is_stale(operation.id()));
    }
}
