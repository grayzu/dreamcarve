// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 6 (`docs/specs/phase-6/design.md`, "dc-app" and "The
//! worker"; functional spec, capabilities 108 and 123): when the app
//! hands out a simulation, what it reuses of the last one, and what Stop,
//! an edit and Undo do to it. The simulation thread is the shell's; these
//! run each job inline with `dc_app::sim::execute`, as the harness runs
//! the generators.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::sync::Arc;

use dc_app::Command;
use dc_app::sim::{self, Blocked, Built, SimResult, SimState};
use dc_app::steps::{self, Session, State, Step};
use dc_app::test_support::Harness;
use dc_job::EstimateConfig;
use dc_job::sim::{CellSpec, Quality};
use dc_project::OperationId;

use common::plaque;

/// The plaque, its toolpaths generated, at Draft so the tests are quick.
fn generated() -> Harness {
    let mut harness = Harness::from_project(plaque());
    harness.app.prefs_mut().sim.quality = Quality::Draft;
    harness.generate();
    harness
}

/// Runs the job the app hands out, as the shell's thread does, and hands
/// the result back.
fn simulate(harness: &mut Harness) -> Arc<Built> {
    let (job, runs) = harness.app.next_sim_job().expect("a job is due");
    let result = sim::execute(
        harness.app.project(),
        &runs,
        &job,
        &EstimateConfig::default(),
        &mut |_| true,
    );
    assert!(harness.app.accept_sim(result));
    Arc::clone(harness.app.sim_built().expect("a simulation"))
}

/// The id of the enabled operation at `index` in the project's list.
fn op(harness: &Harness, index: usize) -> OperationId {
    harness.app.project().operations[index].id().clone()
}

fn enable(harness: &mut Harness, index: usize, enabled: bool) {
    let id = op(harness, index);
    harness
        .run(Command::SetOperationEnabled { id, enabled })
        .unwrap();
    harness.generate();
}

/// Capability 123: the simulation waits for the toolpaths, one job is
/// out at a time, and a simulation that is current is not made again.
#[test]
fn a_job_is_given_only_with_generation_idle() {
    let mut harness = Harness::from_project(plaque());
    harness.app.prefs_mut().sim.quality = Quality::Draft;
    assert_eq!(harness.app.sim_blocked(), Some(Blocked::Generating));
    assert!(harness.app.next_sim_job().is_none());
    assert_eq!(harness.app.sim_state(), SimState::Empty);

    harness.generate();
    assert_eq!(harness.app.sim_blocked(), None);
    let (job, runs) = harness.app.next_sim_job().expect("a job is due");
    assert_eq!(job.snapshot, harness.app.snapshot());
    assert_eq!(job.settings.spec, CellSpec::Quality(Quality::Draft));
    assert!(job.previous.is_none());
    assert!(!runs.is_empty());
    // One at a time.
    assert!(harness.app.next_sim_job().is_none());
    assert_eq!(harness.app.sim_state(), SimState::Building(None));
    assert!(!harness.app.sim_wants_stop());

    let result = sim::execute(
        harness.app.project(),
        &runs,
        &job,
        &EstimateConfig::default(),
        &mut |_| true,
    );
    assert!(harness.app.accept_sim(result));
    assert_eq!(harness.app.sim_state(), SimState::Current);
    assert!(harness.app.next_sim_job().is_none());
    let built = harness.app.sim_built().unwrap();
    assert_eq!(built.plan.runs.len(), 5);
    assert_eq!(built.runs_cut, 5);
    assert_eq!(built.finished.grid().nx(), 512);
}

/// Acceptance 144: after a change only the runs from the first changed
/// one are cut again, and the stock is the stock a build from nothing
/// leaves.
#[test]
fn a_change_starts_from_the_first_changed_run() {
    let mut harness = generated();
    simulate(&mut harness);

    // The last operation off: the four before it are as they were.
    enable(&mut harness, 4, false);
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    let without_last = simulate(&mut harness);
    assert_eq!(without_last.plan.runs.len(), 4);
    assert_eq!(without_last.runs_cut, 0);
    assert_eq!(harness.app.sim_state(), SimState::Current);

    // The third off as well: the two before it hold, the fourth is cut
    // again on what they left.
    enable(&mut harness, 2, false);
    let reused = simulate(&mut harness);
    assert_eq!(reused.plan.runs.len(), 3);
    assert_eq!(reused.runs_cut, 1);

    let mut fresh = Harness::from_project(harness.app.project().clone());
    fresh.app.prefs_mut().sim.quality = Quality::Draft;
    fresh.generate();
    let whole = simulate(&mut fresh);
    assert_eq!(whole.runs_cut, 3);
    assert_eq!(reused.finished, whole.finished);
    assert_eq!(reused.facts, whole.facts);
    assert_eq!(reused.comparison, whole.comparison);
}

/// Acceptance 145: a job the project has moved on from is to be stopped;
/// what it made is kept to show, dimmed, and to build on, and is not the
/// current simulation.
#[test]
fn an_old_snapshots_result_is_kept_but_is_not_current() {
    let mut harness = generated();
    let (job, runs) = harness.app.next_sim_job().expect("a job is due");
    let before = harness.app.project().clone();

    enable(&mut harness, 4, false);
    assert!(harness.app.sim_wants_stop());
    // The thread saw the flag late and finished.
    let late = sim::execute(
        &before,
        &runs,
        &job,
        &EstimateConfig::default(),
        &mut |_| true,
    );
    assert!(matches!(late, SimResult::Built(_)));
    assert!(harness.app.accept_sim(late));
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    assert_eq!(harness.app.sim_built().unwrap().plan.runs.len(), 5);

    let current = simulate(&mut harness);
    assert_eq!(current.plan.runs.len(), 4);
    assert_eq!(current.runs_cut, 0);
    assert_eq!(harness.app.sim_state(), SimState::Current);

    // A job that did stop for a newer one is not held as Stop holds
    // one: Standard is asked for, then Draft again before it is made.
    harness.app.prefs_mut().sim.quality = Quality::Standard;
    let (job, runs) = harness.app.next_sim_job().expect("a job is due");
    harness.app.prefs_mut().sim.quality = Quality::Draft;
    assert!(harness.app.sim_wants_stop());
    // The thread stops and says so. (A job this small is cut before the
    // engine first asks whether to go on: `dc_sim::build`'s own test
    // holds it to a chunk.)
    drop(runs);
    harness.app.accept_sim(SimResult::Stopped {
        snapshot: job.snapshot,
    });
    assert_eq!(harness.app.sim_state(), SimState::Current);
    harness.app.prefs_mut().sim.quality = Quality::Standard;
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    assert!(harness.app.next_sim_job().is_some());
}

/// Capability 127: the simulation follows Undo, cutting again only what
/// the step undone had changed.
#[test]
fn undo_brings_the_earlier_result_back() {
    let mut harness = generated();
    let first = simulate(&mut harness);

    enable(&mut harness, 4, false);
    simulate(&mut harness);

    assert!(harness.app.undo().is_some());
    harness.generate();
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    let again = simulate(&mut harness);
    assert_eq!(again.plan.runs.len(), 5);
    assert_eq!(again.runs_cut, 1, "only the operation that came back");
    assert_eq!(again.finished, first.finished);
}

/// Capability 123: Stop abandons the build and holds it until the user
/// asks again or the project changes.
#[test]
fn stop_holds_until_simulate_again() {
    let mut harness = generated();
    let (job, runs) = harness.app.next_sim_job().expect("a job is due");
    harness.app.stop_sim();
    assert!(harness.app.sim_wants_stop());
    assert!(!runs.is_empty());
    let stopped = SimResult::Stopped {
        snapshot: job.snapshot,
    };
    assert!(harness.app.accept_sim(stopped));
    assert_eq!(harness.app.sim_state(), SimState::Stopped);
    assert!(harness.app.next_sim_job().is_none());

    harness.app.simulate_again();
    assert_eq!(harness.app.sim_state(), SimState::Empty);
    assert!(harness.app.next_sim_job().is_some());
}

/// Capability 112: Quality is part of what a simulation is made with.
#[test]
fn another_quality_is_another_simulation() {
    let mut harness = generated();
    let draft = simulate(&mut harness);
    assert_eq!(draft.finished.grid().nx(), 512);

    harness.app.prefs_mut().sim.quality = Quality::Standard;
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    let standard = simulate(&mut harness);
    assert_eq!(standard.finished.grid().nx(), 1024);
    // Another grid: nothing of the last one is reused.
    assert_eq!(standard.runs_cut, 5);

    // The threshold too, though the cut is the same.
    harness.app.prefs_mut().sim.uncut_mm = 1.0;
    assert_eq!(harness.app.sim_state(), SimState::Stale);
    let looser = simulate(&mut harness);
    assert_eq!(looser.runs_cut, 0);
    assert_eq!(looser.finished, standard.finished);
}

/// Capability 108: with nothing enabled there is nothing to simulate,
/// and a project validation refuses is not simulated.
#[test]
fn nothing_enabled_and_a_refused_project_are_blocked() {
    let mut harness = generated();
    for index in 0..5 {
        enable(&mut harness, index, false);
    }
    assert_eq!(harness.app.sim_blocked(), Some(Blocked::NoOperations));
    assert!(harness.app.next_sim_job().is_none());

    let mut project = plaque();
    project.stock.thickness_mm = None;
    let mut refused = Harness::from_project(project);
    refused.generate();
    assert!(matches!(
        refused.app.sim_blocked(),
        Some(Blocked::Errors(n)) if n >= 1
    ));
    assert!(refused.app.next_sim_job().is_none());
    assert!(refused.app.sim_left_out().is_empty());
}

/// Acceptance 155: the step is empty before a build, complete after one
/// and empty again after any change; it never needs attention, whatever
/// the project's findings; and Export's state is what it is without it.
#[test]
fn simulate_is_empty_then_complete_and_never_needs_attention() {
    let session = Session::default();
    let state = |harness: &Harness, step: Step| steps::states(&harness.app, &session).of(step);
    let mut harness = generated();
    let export = state(&harness, Step::Export);
    assert_eq!(state(&harness, Step::Simulate), State::Empty);

    simulate(&mut harness);
    assert_eq!(state(&harness, Step::Simulate), State::Complete);
    assert_eq!(state(&harness, Step::Export), export, "Export is as it was");
    assert_eq!(
        steps::summary(&harness.app, &session, Step::Simulate),
        format!(
            "5 operations · {}",
            dc_job::simulate::describe_duration(
                sim::rows(
                    harness.app.project(),
                    harness.app.strings(),
                    harness.app.sim_built().unwrap(),
                    0.0
                )
                .total_s
            )
        )
    );

    enable(&mut harness, 4, false);
    assert_eq!(state(&harness, Step::Simulate), State::Empty);
    simulate(&mut harness);
    assert_eq!(state(&harness, Step::Simulate), State::Complete);

    // A project validation refuses: Simulate has nothing to say of it.
    let mut project = plaque();
    project.stock.thickness_mm = None;
    let mut refused = Harness::from_project(project);
    refused.generate();
    assert_eq!(state(&refused, Step::Simulate), State::Empty);
    assert!(matches!(state(&refused, Step::Stock), State::Attention(_)));
    assert_eq!(
        steps::summary(&refused.app, &session, Step::Simulate),
        "1 error blocks the simulation. The stock is shown uncut."
    );
}

/// Capability 114: a row an operation in cut order, with its colour, its
/// tool and its time; the rows after the playback's place are ahead.
#[test]
fn rows_are_in_cut_order_and_follow_the_playback() {
    let mut harness = generated();
    let built = simulate(&mut harness);
    let project = harness.app.project();
    let at_end = sim::rows(project, harness.app.strings(), &built, f64::INFINITY);
    assert_eq!(at_end.rows.len(), 5);
    for (index, row) in at_end.rows.iter().enumerate() {
        let operation = &project.operations[index];
        assert_eq!(&row.op, operation.id());
        assert_eq!(row.name, operation.name());
        assert_eq!(row.colour, index);
        assert_eq!(row.runs, index..index + 1);
        assert!(row.parts.is_empty(), "one run an operation on the plaque");
        assert!(!row.tool.is_empty() && !row.time.is_empty());
        assert_eq!(row.reached, sim::Reached::Done);
    }
    assert!(at_end.total_s > 60.0);

    let at_start = sim::rows(project, harness.app.strings(), &built, 0.0);
    assert!(
        at_start
            .rows
            .iter()
            .all(|row| row.reached == sim::Reached::Ahead)
    );

    // Part-way through the third operation.
    let timeline = dc_job::sim::Timeline::of(&built.runs);
    let (from, to) = timeline.run_span_s(2).unwrap();
    let part_way = sim::rows(
        project,
        harness.app.strings(),
        &built,
        from + (to - from) * 0.25,
    );
    let reached: Vec<_> = part_way.rows.iter().map(|row| row.reached).collect();
    assert_eq!(reached[..2], [sim::Reached::Done; 2]);
    assert!(matches!(reached[2], sim::Reached::Cutting(f) if (f - 0.25).abs() < 1e-9));
    assert_eq!(reached[3..], [sim::Reached::Ahead; 2]);
}
