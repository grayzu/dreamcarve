// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Phase 6, slice 3 (`docs/specs/phase-6/design.md`, "Test plan → Runs
//! and reuse"): the plan is the runs export writes, each board has its
//! own, and the stock they leave is what the fixtures cut.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::{Point3, assert_approx_eq};
use dc_job::sim::{CellSpec, Grid, Heightfield, Motion, Outcome, Quality, build, facts};
use dc_job::simulate::{BoardPlan, Plan, plan, sim_moves};
use dc_job::{EmitOp, EstimateConfig, Move, ProgramSplit, Toolpath};
use dc_project::{Board, Operation, PostProcessorRef, Project, ToolId};

use common::{DATE, load_dcv, repo_root};

/// The six bundled post-processors.
const POSTS: [&str; 6] = ["generic", "grbl", "grblhal", "linuxcnc", "mach3", "uccnc"];

fn fixture(name: &str) -> Project {
    load_dcv(
        &repo_root()
            .join("tests/fixtures")
            .join(format!("{name}.dcv")),
    )
}

fn generated(project: &Project) -> Vec<EmitOp> {
    let built = dc_job::build(project, ProgramSplit::ProjectDefault, DATE);
    assert!(!built.findings.has_errors(), "{:?}", built.findings);
    built.generated.expect("a project with no error generates")
}

fn planned(project: &Project) -> Plan {
    plan(project, &generated(project), &EstimateConfig::default()).expect("a valid project plans")
}

fn cut(board: &BoardPlan, spec: CellSpec) -> Heightfield {
    let grid = Grid::of(board.width_mm, board.height_mm, spec).unwrap();
    let stock = Heightfield::uncut(grid, board.thickness_mm).unwrap();
    match build(stock, &board.sim_runs(), 0, &mut |_, _| {}, &mut |_| true).unwrap() {
        Outcome::Finished(field) => field,
        Outcome::Stopped => panic!("not told to stop"),
    }
}

/// Acceptance 142 (S22): the simulation reads the runs export writes, in
/// the programs' order. On all six post-processors and both program
/// splits, the tool runs of the programs, in the order the programs are
/// run, are the plan's runs grouped where the tool changes, operation
/// for operation; and the plan's cutting moves are the generated
/// toolpaths' cutting moves, move for move.
#[test]
fn the_plan_is_the_runs_export_writes() {
    for name in ["plaque", "sign", "dc"] {
        for post in POSTS {
            let mut project = fixture(name);
            project.post_processor = PostProcessorRef::latest(post).expect("a bundled post");
            for split in [ProgramSplit::ProgramPerTool, ProgramSplit::OneProgram] {
                let case = format!("{name} on {post}, {split:?}");
                let built = dc_job::build(&project, split, DATE);
                if built.findings.has_errors() {
                    // GRBL has no M6: one program for all tools is refused
                    // for a project with two.
                    assert!(
                        split == ProgramSplit::OneProgram,
                        "{case}: {:?}",
                        built.findings
                    );
                    continue;
                }
                let runs = built.generated.as_deref().expect("generated");
                let plan = plan(&project, runs, &EstimateConfig::default()).unwrap();

                // The programs' tool runs, in the order they are run.
                let finished =
                    dc_job::finish(&project, runs, split, DATE, &EstimateConfig::default());
                let mut written: Vec<(Board, ToolId, Vec<String>)> = Vec::new();
                for program in &finished.programs {
                    for run in &program.runs {
                        written.push((
                            program.program.board,
                            run.tool.clone(),
                            run.operations.iter().map(ToString::to_string).collect(),
                        ));
                    }
                }
                // The plan's runs, grouped where the tool changes.
                let mut grouped: Vec<(Board, ToolId, Vec<String>)> = Vec::new();
                for board in &plan.boards {
                    for run in &board.runs {
                        let op = run.op.to_string();
                        match grouped.last_mut() {
                            Some((b, tool, ops)) if *b == board.board && *tool == run.tool => {
                                if ops.last() != Some(&op) {
                                    ops.push(op);
                                }
                            }
                            _ => grouped.push((board.board, run.tool.clone(), vec![op])),
                        }
                    }
                }
                assert_eq!(grouped, written, "{case}");

                // Move for move, the generated toolpaths.
                let enabled: Vec<&EmitOp> = runs.iter().filter(|r| r.op.enabled()).collect();
                let planned: Vec<_> = plan.boards.iter().flat_map(|b| &b.runs).collect();
                let by_board = |board: Board| enabled.iter().filter(move |r| r.board == board);
                let in_order: Vec<&&EmitOp> = by_board(Board::Female)
                    .chain(by_board(Board::Male))
                    .collect();
                assert_eq!(planned.len(), in_order.len(), "{case}");
                for (run, emitted) in planned.iter().zip(in_order) {
                    assert_eq!(run.op, *emitted.op.id(), "{case}");
                    assert_eq!(run.label, emitted.label, "{case}");
                    assert_eq!(run.sim.moves.len(), emitted.toolpath.moves.len(), "{case}");
                    for (k, (sim, mv)) in run
                        .sim
                        .moves
                        .iter()
                        .zip(&emitted.toolpath.moves)
                        .enumerate()
                    {
                        assert_eq!(sim.to, mv.to, "{case}: move {k} of {}", run.op);
                        assert_eq!(sim.cuts(), k > 0 && mv.is_cut(), "{case}: move {k}");
                    }
                }
            }
        }
    }
}

/// A rapid and a dwell cut nothing; a toolpath's first move starts where
/// it ends.
#[test]
fn a_rapid_and_a_dwell_cut_nothing() {
    let p = |x: f64, z: f64| Point3::new(x, 5.0, z);
    let toolpath = Toolpath {
        moves: vec![
            Move::rapid(p(10.0, 20.0)),
            Move::rapid(p(10.0, 13.0)),
            Move::linear(p(10.0, 6.0), 300.0),
            Move::dwell(p(10.0, 6.0), 1.5),
            Move::linear(p(40.0, 6.0), 1200.0),
            Move::rapid(p(40.0, 20.0)),
        ],
    };
    let moves = sim_moves(&toolpath, &EstimateConfig::default());
    let motions: Vec<Motion> = moves.iter().map(|m| m.motion).collect();
    assert_eq!(
        motions,
        [
            Motion::Travel,
            Motion::Travel,
            Motion::Cut,
            Motion::Travel,
            Motion::Cut,
            Motion::Travel
        ]
    );
    assert_eq!(moves[0].from, moves[0].to);
    for pair in moves.windows(2) {
        assert_eq!(pair[0].to, pair[1].from);
    }
}

/// A move's seconds are its length over its feed; a dwell's are its
/// own; a rapid takes none until the user sets a rapid rate; the scale
/// factor multiplies them all, as it does the estimate.
#[test]
fn a_runs_seconds_are_its_lengths_over_its_feeds() {
    let p = |x: f64, z: f64| Point3::new(x, 5.0, z);
    let toolpath = Toolpath {
        moves: vec![
            Move::rapid(p(10.0, 20.0)),
            Move::rapid(p(10.0, 13.0)),
            Move::linear(p(10.0, 6.0), 300.0),
            Move::dwell(p(10.0, 6.0), 1.5),
            Move::linear(p(40.0, 6.0), 1200.0),
        ],
    };
    let plain = sim_moves(&toolpath, &EstimateConfig::default());
    let seconds: Vec<f64> = plain.iter().map(|m| m.seconds).collect();
    // 7 mm at 300 mm/min is 1.4 s; 30 mm at 1200 mm/min is 1.5 s.
    for (got, want) in seconds.iter().zip([0.0, 0.0, 1.4, 1.5, 1.5]) {
        assert_approx_eq!(*got, want, 1e-12);
    }
    let set = EstimateConfig {
        rapid_mm_min: Some(4200.0),
        scale_factor: 2.0,
    };
    let scaled: Vec<f64> = sim_moves(&toolpath, &set)
        .iter()
        .map(|m| m.seconds)
        .collect();
    // The 7 mm rapid at 4200 mm/min is 0.1 s; everything doubled.
    for (got, want) in scaled.iter().zip([0.0, 0.2, 2.8, 3.0, 3.0]) {
        assert_approx_eq!(*got, want, 1e-12);
    }
}

/// Acceptance 150: the lettered inlay's two boards have their own runs,
/// thickness and stock; the stock's runs are every operation's but the
/// male's.
#[test]
fn each_board_has_its_own_runs() {
    let project = fixture("dc");
    let runs = generated(&project);
    let plan = planned(&project);
    assert_eq!(
        plan.boards.iter().map(|b| b.board).collect::<Vec<_>>(),
        [Board::Female, Board::Male]
    );
    let (female, male) = (
        plan.board(Board::Female).unwrap(),
        plan.board(Board::Male).unwrap(),
    );
    assert_eq!(
        female.runs.len(),
        runs.iter().filter(|r| r.board == Board::Female).count()
    );
    assert_eq!(
        male.runs.len(),
        runs.iter().filter(|r| r.board == Board::Male).count()
    );
    assert!(!female.runs.is_empty() && !male.runs.is_empty());
    let (_, male_stock) = project.male_board().expect("the pair cuts a male");
    assert_approx_eq!(male.thickness_mm, male_stock.thickness_mm.unwrap(), 0.0);
    assert_approx_eq!(male.width_mm, male_stock.width_mm, 0.0);
    assert_approx_eq!(
        female.thickness_mm,
        project.stock.thickness_mm.unwrap(),
        0.0
    );
    // A project with no Inlay has the stock alone, and a project with no
    // run still has its stock.
    assert_eq!(planned(&fixture("plaque")).boards.len(), 1);
    let none = plan_of_nothing(&fixture("plaque"));
    assert_eq!(none.boards.len(), 1);
    assert!(none.boards[0].runs.is_empty());

    // Each board is cut by its own runs: cutting the female leaves the
    // male's stock uncut and the reverse.
    let female_field = cut(female, CellSpec::Quality(Quality::Draft));
    let male_field = cut(male, CellSpec::Quality(Quality::Draft));
    assert!(facts(&female_field, &female.sim_runs()).removed_mm3 > 0.0);
    assert!(facts(&male_field, &male.sim_runs()).removed_mm3 > 0.0);
}

fn plan_of_nothing(project: &Project) -> Plan {
    plan(project, &[], &EstimateConfig::default()).unwrap()
}

/// A run's key changes with its moves and its board, and with nothing
/// else: the same project plans the same keys twice, a moved shape
/// changes its operation's runs' keys and no other's, and a thicker
/// stock changes them all.
#[test]
fn a_runs_key_follows_what_the_simulation_reads() {
    let project = fixture("plaque");
    let keys =
        |p: &Project| -> Vec<u64> { planned(p).boards[0].runs.iter().map(|r| r.key).collect() };
    let base = keys(&project);
    assert_eq!(base, keys(&project));
    assert!(base.len() >= 5);

    let mut deeper = project.clone();
    let changed = deeper
        .operations
        .iter_mut()
        .position(|op| match op {
            Operation::Pocket(pocket) => {
                pocket.depth_mm = pocket.depth_mm.map(|d| d - 0.5);
                true
            }
            _ => false,
        })
        .expect("the plaque has a Pocket");
    let after = keys(&deeper);
    let runs = &planned(&deeper).boards[0].runs;
    for (k, run) in runs.iter().enumerate() {
        assert_eq!(
            base[k] != after[k],
            run.op_index == changed,
            "run {k} of {}",
            run.op
        );
    }

    let mut thicker = project.clone();
    thicker.stock.thickness_mm = project.stock.thickness_mm.map(|t| t + 1.0);
    let after = keys(&thicker);
    assert!(base.iter().zip(&after).all(|(a, b)| a != b));
}

/// Ported from PureCutCNC's `tabbedEdgeSimulation.test.ts`, on the
/// plaque's own outline in place of upstream's 50 × 40 rectangle: at two
/// qualities, the stock stands at its tab's height at each of the
/// Profile's four tabs and is cut through on both sides of it, along the
/// outline (acceptance 138). The simulation has no notion of a tab: one
/// stands because the generated path rises over it. The plaque stands in
/// for the functional spec's coaster, which has no committed project.
#[test]
fn the_plaques_tabs_stand() {
    let project = fixture("plaque");
    let plan = planned(&project);
    let board = plan.board(Board::Female).unwrap();
    let profile = project
        .operations
        .iter()
        .find_map(|op| match op {
            Operation::Profile(p) => Some(p),
            _ => None,
        })
        .expect("the plaque is cut out by a Profile");
    // The phase 2 test part's Profile: four tabs, 8 mm long and 3 mm
    // high, placed by count.
    let tab_top_mm = profile
        .tabs
        .as_ref()
        .expect("the Cut out has tabs")
        .height_mm;
    assert_approx_eq!(tab_top_mm, 3.0, 0.0);
    let run = board
        .runs
        .iter()
        .position(|r| r.op == profile.id)
        .expect("the Profile's run");
    let thickness_mm = board.thickness_mm;
    for quality in [Quality::Draft, Quality::Standard] {
        let field = cut(board, CellSpec::Quality(quality));
        // No cell is below the bottom, and none above the top.
        assert!(
            field
                .heights()
                .iter()
                .all(|&z| (0.0..=thickness_mm).contains(&z))
        );
        let found = facts(&field, &board.sim_runs());
        assert!(found.through_mm2 > 100.0, "{quality:?}: {found:?}");
        assert!(
            found.through_runs.contains(&u16::try_from(run).unwrap()),
            "{quality:?}"
        );

        // What the Profile's run left standing in its own trench: the
        // tabs, each a patch of cells, the rest of the trench cut
        // through.
        let (nx, ny) = (field.grid().nx() as usize, field.grid().ny() as usize);
        let standing: Vec<bool> = field
            .heights()
            .iter()
            .zip(field.owners())
            .map(|(&z, &owner)| usize::from(owner) == run && z > 0.5)
            .collect();
        let mut seen = vec![false; nx * ny];
        let mut patches: Vec<f64> = Vec::new();
        for start in 0..nx * ny {
            if !standing[start] || seen[start] {
                continue;
            }
            let (mut stack, mut top) = (vec![start], 0.0_f64);
            seen[start] = true;
            while let Some(k) = stack.pop() {
                top = top.max(field.heights()[k]);
                let (i, j) = ((k % nx) as isize, (k / nx) as isize);
                for (di, dj) in [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let (a, b) = (i + di, j + dj);
                    if a < 0 || b < 0 || a >= nx as isize || b >= ny as isize {
                        continue;
                    }
                    let n = b as usize * nx + a as usize;
                    if standing[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            patches.push(top);
        }
        // Four of them are the tabs, each standing at the tab's height.
        // The other is beside the trench at the back: the lead-in arc,
        // which the first pass alone cuts, 4 mm deep.
        let tabs = patches
            .iter()
            .filter(|&&top| (top - tab_top_mm).abs() < 1e-9)
            .count();
        assert_eq!(tabs, 4, "{quality:?}: {patches:?}");
        for top in patches {
            assert!(
                (top - tab_top_mm).abs() < 1e-9 || top > tab_top_mm + 0.5,
                "{quality:?}: a patch stands {top} mm high"
            );
        }
    }
}
