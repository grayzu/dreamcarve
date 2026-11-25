// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/playback.test.ts

//! Tests of playback. Ported from `playback.test.ts`, on its grid of
//! 40 × 4 cells of 1 with a flat cutter of radius 1 and its slot (a
//! plunge, then a cut 30 long): a slow move advances slower
//! (`testReducedFeedMoveAdvancesSlower`), forward seeks equal one seek
//! (`testForwardSeekMatchesFreshReplay`), a backward seek restores the
//! earlier state (`testBackwardSeekRestoresEarlierState`) and the dirty
//! region grows until cleared (`testDirtyRegionAccumulatesUntilCleared`).
//! Upstream places its cursor by path length with feed-scaled budgets;
//! here time is the measure, so a move at a tenth of the feed is ten
//! times as long on the timeline. Its two subdivision tests are not
//! ported: the cut splits moves here, and `cut`'s tests cover it. The
//! rest are acceptance tests 139 and 140.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use super::*;
use crate::build::{Outcome, build};
use crate::cut::{SimMove, Turn};
use crate::cutter::{Cutter, CutterShape};
use crate::grid::{CellSpec, Grid};
use dc_geometry::{Point, assert_approx_eq};

fn shape(cutter: Cutter) -> CutterShape {
    CutterShape {
        cutter,
        plain: false,
    }
}

/// The stock `runs` leave on a board, with every checkpoint.
fn built(
    (width_mm, height_mm, cell_mm, top_mm): (f64, f64, f64, f64),
    runs: &[SimRun],
) -> (Checkpoints, Arc<Heightfield>) {
    let grid = Grid::of(width_mm, height_mm, CellSpec::CellMm(cell_mm)).unwrap();
    let stock = Heightfield::uncut(grid, top_mm).unwrap();
    let mut kept = Checkpoints::default();
    let finished = match build(stock, runs, 0, &mut |cut, f| kept.keep(cut, f), &mut |_| {
        true
    })
    .unwrap()
    {
        Outcome::Finished(field) => field,
        Outcome::Stopped => panic!("not told to stop"),
    };
    (kept, Arc::new(finished))
}

fn playback(board: (f64, f64, f64, f64), runs: Vec<SimRun>) -> Playback {
    let (kept, finished) = built(board, &runs);
    Playback::at_end(runs.into(), kept, finished)
}

/// The build of the job up to `cursor`, the move under way cut as far
/// as the cursor is along it: what playback must show there.
fn build_to(board: (f64, f64, f64, f64), runs: &[SimRun], cursor: Cursor) -> Heightfield {
    let grid = Grid::of(board.0, board.1, CellSpec::CellMm(board.2)).unwrap();
    let mut field = Heightfield::uncut(grid, board.3).unwrap();
    for (r, run) in runs.iter().enumerate() {
        for (m, mv) in run.moves.iter().enumerate() {
            let span = match (r, m).cmp(&(cursor.run, cursor.mv)) {
                std::cmp::Ordering::Less => (0.0, 1.0),
                std::cmp::Ordering::Equal => (0.0, cursor.fraction),
                std::cmp::Ordering::Greater => return field,
            };
            field.cut(mv, span, &run.cutter.cutter, u16::try_from(r).unwrap());
        }
    }
    field
}

fn assert_same(a: &Heightfield, b: &Heightfield, what: &str) {
    let worst = a
        .heights()
        .iter()
        .zip(b.heights())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max);
    assert!(worst <= 1e-9, "{what}: heights differ by {worst}");
}

const SLOT_BOARD: (f64, f64, f64, f64) = (40.0, 4.0, 1.0, 6.0);

/// Upstream's slot: a plunge to 2 below the top, then a cut 30 long, as
/// one run; here with a rapid before and after. The plunge takes 1 s
/// and the cut 3 s.
fn slot() -> Vec<SimRun> {
    let p = |x: f64, z: f64| Point3::new(x, 2.0, z);
    vec![SimRun {
        cutter: shape(Cutter::Flat { radius_mm: 1.0 }),
        moves: vec![
            SimMove::travel(p(5.0, 11.0), p(5.0, 7.0), 0.0),
            SimMove::cut(p(5.0, 7.0), p(5.0, 4.0), 1.0),
            SimMove::cut(p(5.0, 4.0), p(35.0, 4.0), 3.0),
            SimMove::travel(p(35.0, 4.0), p(35.0, 11.0), 0.0),
        ],
    }]
}

/// Two runs with every kind of move: ramps, a helix, level arcs, a V
/// and a ball, on a board fine enough for a move to cover many cells.
const BOARD: (f64, f64, f64, f64) = (40.0, 30.0, 0.5, 10.0);

fn job() -> Vec<SimRun> {
    let p = Point3::new;
    let centre = Point::new(28.0, 18.0);
    vec![
        SimRun {
            cutter: shape(Cutter::Ball { radius_mm: 2.0 }),
            moves: vec![
                SimMove::travel(p(5.0, 5.0, 15.0), p(5.0, 5.0, 11.0), 0.0),
                SimMove::cut(p(5.0, 5.0, 11.0), p(5.0, 5.0, 7.0), 1.6),
                SimMove::cut(p(5.0, 5.0, 7.0), p(30.0, 12.0, 5.0), 4.0),
                SimMove::cut(p(30.0, 12.0, 5.0), p(12.0, 24.0, 8.5), 3.5),
                SimMove::travel(p(12.0, 24.0, 8.5), p(12.0, 24.0, 15.0), 0.0),
            ],
        },
        SimRun {
            cutter: shape(Cutter::Flat { radius_mm: 1.5 }),
            moves: vec![
                SimMove::travel(p(12.0, 24.0, 15.0), p(31.0, 18.0, 15.0), 0.0),
                SimMove::cut(p(31.0, 18.0, 10.5), p(25.0, 18.0, 8.0), 2.0).along(centre, Turn::Ccw),
                SimMove::cut(p(25.0, 18.0, 8.0), p(31.0, 18.0, 6.0), 2.0).along(centre, Turn::Ccw),
                SimMove::cut(p(31.0, 18.0, 6.0), p(25.0, 18.0, 6.0), 1.0).along(centre, Turn::Cw),
                SimMove::travel(p(25.0, 18.0, 6.0), p(25.0, 18.0, 15.0), 0.0),
            ],
        },
        SimRun {
            cutter: shape(Cutter::Vee {
                slope: 1.0,
                radius_mm: 3.0,
            }),
            moves: vec![
                SimMove::travel(p(25.0, 18.0, 15.0), p(8.0, 14.0, 15.0), 0.0),
                SimMove::cut(p(8.0, 14.0, 11.0), p(8.0, 14.0, 8.0), 1.0),
                SimMove::cut(p(8.0, 14.0, 8.0), p(20.0, 20.0, 7.0), 2.5),
            ],
        },
    ]
}

/// Opening a job shows the finished stock, at the job's end, with no
/// tool.
#[test]
fn a_job_opens_at_its_end() {
    let (kept, finished) = built(BOARD, &job());
    let play = Playback::at_end(job().into(), kept, Arc::clone(&finished));
    assert!(play.is_finished());
    assert_approx_eq!(play.time_s(), play.timeline().total_s(), 0.0);
    assert_approx_eq!(play.timeline().total_s(), 17.6, 1e-12);
    assert_eq!(play.field(), &*finished);
    assert_eq!(play.pose(), None);
}

/// Ported from `testReducedFeedMoveAdvancesSlower`: a move at a tenth of
/// the feed is ten times as long on the timeline, so a second of
/// playback covers a tenth as much of it.
#[test]
fn a_slow_move_takes_longer() {
    let p = |x: f64| Point3::new(x, 2.0, 4.0);
    let runs = vec![SimRun {
        cutter: shape(Cutter::Flat { radius_mm: 1.0 }),
        moves: vec![
            // 10 mm at the feed, then 10 mm at a tenth of it.
            SimMove::cut(p(5.0), p(15.0), 1.0),
            SimMove::cut(p(15.0), p(25.0), 10.0),
        ],
    }];
    let mut play = playback(SLOT_BOARD, runs);
    play.seek(1.0, u64::MAX);
    assert_approx_eq!(play.pose().unwrap().tip.x, 15.0, 1e-12);
    play.seek(2.0, u64::MAX);
    assert_approx_eq!(play.pose().unwrap().tip.x, 16.0, 1e-12);
}

/// Ported from `testForwardSeekMatchesFreshReplay`: seeking forward in
/// steps leaves what one seek from the start leaves.
#[test]
fn forward_seeks_equal_one_seek() {
    let total = Timeline::of(&slot()).total_s();
    let mut stepped = playback(SLOT_BOARD, slot());
    stepped.seek(0.0, u64::MAX);
    // Upstream seeks to 8, 20 and 27.5 of a path 32 long.
    for part in [8.0, 20.0, 27.5] {
        stepped.seek(part / 32.0 * total, u64::MAX);
    }
    let mut direct = playback(SLOT_BOARD, slot());
    direct.seek(0.0, u64::MAX);
    direct.seek(27.5 / 32.0 * total, u64::MAX);
    assert_same(stepped.field(), direct.field(), "three seeks against one");
    assert_eq!(stepped.cursor(), direct.cursor());
    // The slot is cut as far as the tip, and no further.
    let tip = direct.pose().unwrap().tip;
    let z_at = |x: f64| {
        let (i, j) = direct.field().grid().cell_at(Point::new(x, 2.0)).unwrap();
        direct.field().z_mm(i, j).unwrap()
    };
    assert_approx_eq!(z_at(tip.x - 0.6), 4.0, 0.0);
    assert_approx_eq!(z_at(tip.x + 1.6), 6.0, 0.0);
}

/// Ported from `testBackwardSeekRestoresEarlierState`: after a seek
/// forward and one back, the stock is what a seek straight to the
/// earlier time leaves.
#[test]
fn a_backward_seek_restores_the_earlier_field() {
    let mut there_and_back = playback(BOARD, job());
    there_and_back.seek(0.0, u64::MAX);
    there_and_back.seek(13.0, u64::MAX);
    there_and_back.seek(4.2, u64::MAX);
    let mut direct = playback(BOARD, job());
    direct.seek(0.0, u64::MAX);
    direct.seek(4.2, u64::MAX);
    assert_same(there_and_back.field(), direct.field(), "back to 4.2 s");
    assert_eq!(there_and_back.cursor(), direct.cursor());
    assert_approx_eq!(there_and_back.time_s(), 4.2, 1e-12);
    // Back within a later run restores that run's checkpoint, not the
    // start: the cells the first run cut are still cut.
    there_and_back.seek(15.0, u64::MAX);
    there_and_back.seek(10.0, u64::MAX);
    assert_same(
        there_and_back.field(),
        &build_to(BOARD, &job(), there_and_back.cursor()),
        "back to 10 s",
    );
}

/// Ported from `testDirtyRegionAccumulatesUntilCleared`: the changed
/// rectangle grows across seeks until it is taken, and a seek backwards
/// dirties the whole grid, since restoring raises cells.
#[test]
fn the_dirty_rectangle_grows_until_taken() {
    let mut play = playback(SLOT_BOARD, slot());
    assert_eq!(play.take_dirty(), None, "nothing has changed yet");
    play.seek(0.0, u64::MAX);
    let whole = CellRect {
        i0: 0,
        i1: 40,
        j0: 0,
        j1: 4,
    };
    assert_eq!(play.take_dirty(), Some(whole), "a seek backwards");
    assert_eq!(play.take_dirty(), None, "taken");

    play.seek(1.5, u64::MAX);
    let first = play
        .dirty
        .expect("the plunge and the cut's start changed cells");
    play.seek(2.5, u64::MAX);
    let second = play.dirty.expect("more of the cut");
    assert!(second.i1 > first.i1 && second.i0 == first.i0);
    assert_eq!(play.take_dirty(), Some(second));
    // A seek that changes no cell leaves nothing dirty.
    play.seek(2.5, u64::MAX);
    assert_eq!(play.take_dirty(), None);
    // Reaching the end by cutting whole moves leaves the build itself,
    // so the snap to it changes nothing more than the cut did.
    play.seek(99.0, u64::MAX);
    assert!(play.is_finished());
    assert!(play.take_dirty().is_some());
}

/// Acceptance 139: playing from the start in steps of the whole job
/// divided by 7, 53, 197 and 1000 (upstream's step counts), on ramps, a
/// helix and arcs, ends with every cell equal to the direct build, its
/// owner too; and so does any sequence of seeks forwards and backwards
/// that ends at the end.
#[test]
fn playing_in_any_steps_ends_at_the_build() {
    let (_, finished) = built(BOARD, &job());
    for steps in [7_u32, 53, 197, 1000] {
        let mut play = playback(BOARD, job());
        play.seek(0.0, u64::MAX);
        let total = play.timeline().total_s();
        for k in 1..=steps {
            play.seek(total * f64::from(k) / f64::from(steps), u64::MAX);
            if k == steps / 2 {
                // Before the end, the parts agree with the build so far
                // to rounding.
                assert_same(
                    play.field(),
                    &build_to(BOARD, &job(), play.cursor()),
                    &format!("half way in {steps} steps"),
                );
            }
        }
        assert!(play.is_finished(), "{steps} steps");
        assert_eq!(play.field(), &*finished, "{steps} steps");
    }
    let mut play = playback(BOARD, job());
    for t in [3.0, 16.0, 9.5, 0.0, 12.25, 5.0, 17.0, 99.0] {
        play.seek(t, u64::MAX);
    }
    assert_eq!(play.field(), &*finished, "a wander that ends at the end");
}

/// Acceptance 140: at any time into a job, the stock is what a build of
/// the moves up to that time leaves, with the move under way cut as far
/// as the tool has gone; the tool is at that point.
#[test]
fn part_way_is_the_build_of_the_moves_so_far() {
    let runs = job();
    let mut play = playback(BOARD, runs.clone());
    play.seek(0.0, u64::MAX);
    for t in [
        0.4, 1.6, 3.0, 5.59, 5.6, 8.0, 9.1, 10.3, 12.0, 14.1, 14.6, 16.0, 17.59,
    ] {
        play.seek(t, u64::MAX);
        let cursor = play.cursor();
        assert_approx_eq!(play.time_s(), t, 1e-9);
        assert_same(
            play.field(),
            &build_to(BOARD, &runs, cursor),
            &format!("{t} s"),
        );
        let pose = play.pose().expect("mid-job there is a tool");
        let mv = &runs[cursor.run].moves[cursor.mv];
        assert_eq!(pose.tip, mv.point_at(cursor.fraction), "{t} s");
        assert_eq!((pose.run, pose.cuts), (cursor.run, mv.cuts()), "{t} s");
    }
}

/// A click on a run's row: the stock as that run leaves it is the
/// checkpoint after it, to the bit, and the rows after it are uncut.
#[test]
fn to_run_end_is_the_runs_checkpoint() {
    let runs = job();
    let (kept, finished) = built(BOARD, &runs);
    let mut play = Playback::at_end(runs.clone().into(), kept.clone(), finished);
    for run in [1, 0, 2, 0] {
        play.to_run_end(run);
        assert_eq!(play.cursor(), Cursor::at_run(run + 1));
        assert_eq!(
            play.field(),
            &**kept.get(run + 1).unwrap(),
            "after run {run}"
        );
        assert_approx_eq!(
            play.time_s(),
            play.timeline().run_span_s(run).unwrap().1,
            1e-12
        );
    }
    play.to_run_end(99);
    assert!(play.is_finished());
}

/// `,` and `.`: a step is one cutting move, to its end; rapids between
/// are passed with it.
#[test]
fn a_step_is_one_cutting_move() {
    let runs = job();
    let mut play = playback(BOARD, runs.clone());
    play.seek(0.0, u64::MAX);
    play.step_moves(1);
    // The first cutting move is the plunge, move 1 of run 0.
    assert_eq!(
        play.cursor(),
        Cursor {
            run: 0,
            mv: 2,
            fraction: 0.0
        }
    );
    play.step_moves(2);
    assert_eq!(
        play.cursor(),
        Cursor {
            run: 0,
            mv: 4,
            fraction: 0.0
        }
    );
    // The next cutting move is in the next run.
    play.step_moves(1);
    assert_eq!(
        play.cursor(),
        Cursor {
            run: 1,
            mv: 2,
            fraction: 0.0
        }
    );
    assert_same(
        play.field(),
        &build_to(BOARD, &runs, play.cursor()),
        "four steps",
    );
    play.step_moves(-1);
    assert_eq!(
        play.cursor(),
        Cursor {
            run: 0,
            mv: 4,
            fraction: 0.0
        }
    );
    // From part-way along a move, a step back is to the end of the move
    // before it, and a step on to its own end.
    play.seek(3.0, u64::MAX);
    assert_eq!((play.cursor().run, play.cursor().mv), (0, 2));
    play.step_moves(1);
    assert_eq!((play.cursor().run, play.cursor().mv), (0, 3));
    play.seek(3.0, u64::MAX);
    play.step_moves(-1);
    assert_eq!((play.cursor().run, play.cursor().mv), (0, 2));
    assert_approx_eq!(play.cursor().fraction, 0.0, 0.0);
    // Past either end it stops there.
    play.step_moves(-99);
    assert_eq!(play.cursor(), Cursor::default());
    assert!(play.is_at_start());
    play.step_moves(99);
    assert!(play.is_finished());
}

/// A seek with a budget stops when the cells are spent and says how far
/// it got; asked again it goes on, and ends where an unbounded seek
/// does. A budget of 0 still finishes a move a time.
#[test]
fn a_spent_budget_returns_the_time_reached() {
    let mut bounded = playback(BOARD, job());
    bounded.seek(0.0, u64::MAX);
    let total = bounded.timeline().total_s();
    let reached = bounded.seek(total, 200);
    assert!(
        reached > 0.0 && reached < total,
        "reached {reached} of {total}"
    );
    assert_approx_eq!(bounded.time_s(), reached, 0.0);
    assert_same(
        bounded.field(),
        &build_to(BOARD, &job(), bounded.cursor()),
        "where the budget ran out",
    );
    let mut calls = 1;
    while !bounded.is_finished() {
        let before = bounded.cursor();
        let now = bounded.seek(total, 0);
        assert!(bounded.cursor() > before, "no progress at {now} s");
        calls += 1;
        assert!(calls < 100);
    }
    let (_, finished) = built(BOARD, &job());
    assert_eq!(bounded.field(), &*finished);
    // A bounded seek backwards restores, then goes forward as far as it
    // can.
    let back = bounded.seek(9.0, 0);
    assert!(back <= 9.0);
    assert_same(
        bounded.field(),
        &build_to(BOARD, &job(), bounded.cursor()),
        "a bounded seek back",
    );
}

/// The tool is not drawn at the very start or the very end, and between
/// runs it is at the next run's first move.
#[test]
fn the_pose_is_none_at_both_ends() {
    let runs = job();
    let mut play = playback(BOARD, runs.clone());
    assert_eq!(play.pose(), None, "at the end");
    play.seek(0.0, u64::MAX);
    assert!(play.is_at_start());
    assert_eq!(play.pose(), None, "at the start");
    play.to_run_end(0);
    let pose = play.pose().expect("between runs");
    assert_eq!(pose.run, 1);
    assert_eq!(pose.tip, runs[1].moves[0].from);
    assert!(!pose.cuts);
}

/// A job with nothing to cut plays: it is always at its end.
#[test]
fn an_empty_job_plays() {
    let mut play = playback(BOARD, Vec::new());
    assert!(play.is_finished());
    play.seek(0.0, u64::MAX);
    play.step_moves(3);
    play.to_run_end(0);
    assert!(play.is_finished() && play.pose().is_none());
    assert_approx_eq!(play.time_s(), 0.0, 0.0);
}
