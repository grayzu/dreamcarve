// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/replay.ts (`replayItemsIntoGrid`, `replayItemsIntoGridChunked`)
//   src/app/simulationCompute.ts

//! Runs cut in order (`docs/specs/phase-6/design.md`, "Algorithms → Runs
//! in order, and what is reused").
//!
//! From upstream: the items in cut order onto one grid, each move in
//! order, moves that do not cut passed over; a build that can be told to
//! stop between chunks of work and then gives no part-built result.
//! Changed: the caller is handed the stock after each run, to keep as a
//! checkpoint; a build can start part-way, from the stock the earlier
//! runs left; progress is reported; a chunk is a count of cells tested,
//! where upstream's is 16 ms on the browser's one thread. Not taken:
//! upstream's *Selected* mode, one operation alone on fresh stock (the
//! survey, section 7.2).

use crate::SimError;
use crate::cut::SimMove;
use crate::cutter::CutterShape;
use crate::field::{Heightfield, UNCUT};

/// How many cells a build tests between asking whether to go on: about
/// four million, a few milliseconds of work.
pub const CHUNK_CELLS: u64 = 1 << 22;

/// One tool run: a cutter and the moves it makes, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct SimRun {
    /// What it cuts with.
    pub cutter: CutterShape,
    /// Its moves, cutting or not.
    pub moves: Vec<SimMove>,
}

/// How far a build has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The run being cut, as its place in the plan.
    pub run: usize,
    /// Moves done, of the runs this build cuts.
    pub moves_done: usize,
    /// Moves in the runs this build cuts.
    pub moves: usize,
}

impl Progress {
    /// The part done, from 0 to 1; 1 for a build with nothing to cut.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        if self.moves == 0 {
            1.0
        } else {
            // Counts of moves, far under 2⁵³.
            #[expect(clippy::cast_precision_loss, reason = "move counts are far under 2⁵³")]
            {
                self.moves_done as f64 / self.moves as f64
            }
        }
    }
}

/// How a build ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Every run was cut: the stock they leave.
    Finished(Heightfield),
    /// It was told to stop. There is no part-built stock.
    Stopped,
}

/// Cuts `runs[from..]` into `base`, the stock `runs[..from]` left (uncut
/// stock for `from` 0), each run as the owner of the cells it lowers.
/// `after_run` is given the number of runs cut so far and the stock they
/// leave, once for `from` before anything is cut and once after each
/// run: the checkpoints. `keep_going` is asked between chunks of
/// [`CHUNK_CELLS`] cells tested, with the progress; `false` stops the
/// build.
///
/// The result does not depend on the chunks: every move is cut whole.
///
/// # Errors
///
/// [`SimError::TooManyRuns`] for more runs than a cell can name.
pub fn build(
    base: Heightfield,
    runs: &[SimRun],
    from: usize,
    after_run: &mut dyn FnMut(usize, &Heightfield),
    keep_going: &mut dyn FnMut(Progress) -> bool,
) -> Result<Outcome, SimError> {
    if runs.len() >= usize::from(UNCUT) {
        return Err(SimError::TooManyRuns { runs: runs.len() });
    }
    let mut field = base;
    let todo = runs.get(from..).unwrap_or(&[]);
    let moves = todo.iter().map(|r| r.moves.len()).sum();
    let (mut moves_done, mut since_asked) = (0, 0);
    after_run(from.min(runs.len()), &field);
    for (k, run) in todo.iter().enumerate() {
        let run_index = from + k;
        // Checked above: under `UNCUT`.
        let owner = u16::try_from(run_index).unwrap_or(UNCUT - 1);
        for mv in &run.moves {
            since_asked += field.cut(mv, (0.0, 1.0), &run.cutter.cutter, owner).tested;
            moves_done += 1;
            if since_asked >= CHUNK_CELLS {
                since_asked = 0;
                let progress = Progress {
                    run: run_index,
                    moves_done,
                    moves,
                };
                if !keep_going(progress) {
                    return Ok(Outcome::Stopped);
                }
            }
        }
        after_run(run_index + 1, &field);
    }
    Ok(Outcome::Finished(field))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use crate::cutter::Cutter;
    use crate::grid::{CellSpec, Grid};
    use dc_geometry::Point3;

    fn stock() -> Heightfield {
        let grid = Grid::of(60.0, 40.0, CellSpec::CellMm(0.25)).unwrap();
        Heightfield::uncut(grid, 12.0).unwrap()
    }

    fn flat(radius_mm: f64) -> CutterShape {
        CutterShape {
            cutter: Cutter::Flat { radius_mm },
            plain: false,
        }
    }

    /// A run of level passes across the board at `z_mm`, `y` from `y0`,
    /// with a rapid between each.
    fn passes(cutter: CutterShape, y0: f64, z_mm: f64, count: u32) -> SimRun {
        let mut moves = Vec::new();
        for k in 0..count {
            let y = y0 + f64::from(k) * 2.0;
            let (a, b) = (Point3::new(5.0, y, z_mm), Point3::new(55.0, y, z_mm));
            moves.push(SimMove::travel(Point3::new(5.0, y, 17.0), a, 0.0));
            moves.push(SimMove::cut(a, b, 2.0));
            moves.push(SimMove::travel(b, Point3::new(55.0, y, 17.0), 0.0));
        }
        SimRun { cutter, moves }
    }

    fn five_runs() -> Vec<SimRun> {
        vec![
            passes(flat(3.0), 6.0, 9.0, 4),
            passes(flat(1.5), 20.0, 6.0, 3),
            passes(flat(3.0), 8.0, 7.0, 2),
            passes(flat(1.0), 30.0, 2.0, 4),
            passes(flat(3.0), 10.0, 5.0, 1),
        ]
    }

    fn finished(built: Result<Outcome, SimError>) -> Heightfield {
        match built.unwrap() {
            Outcome::Finished(field) => field,
            Outcome::Stopped => panic!("the build was not told to stop"),
        }
    }

    /// Acceptance 144: a build that starts at the third of five runs,
    /// from the stock the first two left, is the build from nothing,
    /// owners and all; and the stock handed back after each run is the
    /// build of the runs so far.
    #[test]
    fn a_build_from_a_checkpoint_equals_a_build_from_nothing() {
        let runs = five_runs();
        let mut kept = Vec::new();
        let whole = finished(build(
            stock(),
            &runs,
            0,
            &mut |cut, field| kept.push((cut, field.clone())),
            &mut |_| true,
        ));
        assert_eq!(
            kept.iter().map(|k| k.0).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(kept[0].1, stock());
        assert_eq!(kept[5].1, whole);

        for (from, base) in kept.iter().enumerate().skip(1) {
            let mut handed = Vec::new();
            let resumed = finished(build(
                base.1.clone(),
                &runs,
                from,
                &mut |cut, _| handed.push(cut),
                &mut |_| true,
            ));
            assert_eq!(resumed, whole, "a build from run {from}");
            assert_eq!(handed, (from..=5).collect::<Vec<_>>());
        }
        // Each checkpoint is the build of the runs before it.
        for (cut, field) in &kept {
            let alone = finished(build(
                stock(),
                &runs[..*cut],
                0,
                &mut |_, _| {},
                &mut |_| true,
            ));
            assert_eq!(&alone, field, "after {cut} runs");
        }
    }

    /// Acceptance 151: every cut cell names the run that lowered it
    /// last; a cell a later run passes over without lowering keeps its
    /// owner.
    #[test]
    fn the_owner_is_the_last_run_to_lower_a_cell() {
        let runs = five_runs();
        let field = finished(build(stock(), &runs, 0, &mut |_, _| {}, &mut |_| true));
        let at = |x: f64, y: f64| {
            let (i, j) = field.grid().cell_at(dc_geometry::Point::new(x, y)).unwrap();
            (field.z_mm(i, j).unwrap(), field.cut_by(i, j))
        };
        // Run 0 cuts y 3..15 to 9; run 2 cuts y 5..13 to 7; run 4 cuts
        // y 7..13 to 5.
        assert_eq!(at(30.0, 3.6), (9.0, Some(0)));
        assert_eq!(at(30.0, 5.6), (7.0, Some(2)));
        assert_eq!(at(30.0, 10.1), (5.0, Some(4)));
        // Run 1 at 6 mm, y 18.5..25.5; nothing later reaches it.
        assert_eq!(at(30.0, 22.1), (6.0, Some(1)));
        assert_eq!(at(30.0, 16.6), (12.0, None));
        // Run 2 passes over run 4's later, deeper cut: the deeper wins
        // whatever the order.
        let mut swapped = runs.clone();
        swapped.swap(2, 4);
        let other = finished(build(stock(), &swapped, 0, &mut |_, _| {}, &mut |_| true));
        assert_eq!(other.heights(), field.heights());
        let (i, j) = other
            .grid()
            .cell_at(dc_geometry::Point::new(30.0, 10.1))
            .unwrap();
        assert_eq!(other.cut_by(i, j), Some(2), "the deeper run, now third");
    }

    /// Acceptance 145: a build told to stop returns within one chunk of
    /// work and gives no stock; one never asked to stop reports progress
    /// that only grows.
    #[test]
    fn a_stopped_build_stops_within_a_chunk() {
        // Enough work for several chunks: wide passes on a fine grid.
        let grid = Grid::of(400.0, 400.0, CellSpec::CellMm(0.2)).unwrap();
        let big = Heightfield::uncut(grid, 12.0).unwrap();
        let mut moves = Vec::new();
        for k in 0..60 {
            let y = 20.0 + f64::from(k) * 6.0;
            moves.push(SimMove::cut(
                Point3::new(10.0, y, 6.0),
                Point3::new(390.0, y, 6.0),
                1.0,
            ));
        }
        let runs = vec![SimRun {
            cutter: flat(20.0),
            moves,
        }];

        let mut seen = Vec::new();
        let done = build(big.clone(), &runs, 0, &mut |_, _| {}, &mut |p| {
            seen.push(p);
            true
        });
        assert!(matches!(done, Ok(Outcome::Finished(_))));
        assert!(seen.len() >= 3, "only {} chunks", seen.len());
        assert!(seen.windows(2).all(|w| w[0].moves_done < w[1].moves_done));
        assert!(seen.iter().all(|p| p.moves == 60 && p.run == 0));
        assert!(
            seen.iter()
                .all(|p| p.fraction() > 0.0 && p.fraction() <= 1.0)
        );

        let mut asked = 0;
        let mut checkpoints = 0;
        let stopped = build(big, &runs, 0, &mut |_, _| checkpoints += 1, &mut |_| {
            asked += 1;
            asked < 2
        });
        assert_eq!(stopped, Ok(Outcome::Stopped));
        assert_eq!(asked, 2, "it stopped at the chunk that said so");
        assert_eq!(checkpoints, 1, "only the base was handed back");
    }

    #[test]
    fn a_build_of_nothing_is_the_base() {
        let runs = five_runs();
        let mut handed = Vec::new();
        let field = finished(build(
            stock(),
            &runs,
            5,
            &mut |cut, _| handed.push(cut),
            &mut |_| true,
        ));
        assert_eq!(field, stock());
        assert_eq!(handed, [5]);
        let none = finished(build(stock(), &[], 0, &mut |_, _| {}, &mut |_| true));
        assert_eq!(none, stock());
        assert!(
            (Progress {
                run: 0,
                moves_done: 0,
                moves: 0
            })
            .fraction()
                >= 1.0
        );
    }
}
