// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/playback.ts (`PlaybackController`)

//! Playing a job back (`docs/specs/phase-6/design.md`, "Algorithms → The
//! timeline and playback"; functional capabilities 113 and 114).
//!
//! From upstream's controller: a cursor over the path and a live stock
//! cloned from a base it never writes; a forward seek cuts on from where
//! it is, since cuts only lower cells; a backward seek restores and
//! replays; the tip's pose; a rectangle of changed cells that grows
//! until the caller takes it, the whole grid after a restore. Changed:
//! the cursor runs over every run of the job and is placed by time;
//! a backward seek restores the nearest checkpoint, not the start; a
//! seek is bounded by cells tested, so a heavy job slows the clock and
//! not the window; the move under way is cut by the part of it the tip
//! has passed, exactly, where upstream's part-played ramp differs from
//! its whole one (the survey, item 4). Upstream splits moves into pieces
//! itself; here the cut does ([`crate::Heightfield::cut`]).

use std::sync::Arc;

use dc_geometry::Point3;

use crate::build::SimRun;
use crate::checkpoints::Checkpoints;
use crate::field::{Heightfield, UNCUT};
use crate::grid::CellRect;
use crate::timeline::{Cursor, Timeline};

/// Where the tool is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The tip, mm, project space.
    pub tip: Point3,
    /// The run under way, as its place in the plan.
    pub run: usize,
    /// Whether the move under way cuts.
    pub cuts: bool,
}

/// A job being played: where it has got to and the stock as it is there.
#[derive(Debug, Clone)]
pub struct Playback {
    runs: Arc<[SimRun]>,
    timeline: Timeline,
    checkpoints: Checkpoints,
    finished: Arc<Heightfield>,
    live: Heightfield,
    at: Cursor,
    dirty: Option<CellRect>,
}

impl Playback {
    /// The job at its end: `finished` is the stock every run of `runs`
    /// leaves, and `checkpoints` the stock kept after earlier runs, the
    /// uncut stock among them.
    #[must_use]
    pub fn at_end(
        runs: Arc<[SimRun]>,
        checkpoints: Checkpoints,
        finished: Arc<Heightfield>,
    ) -> Self {
        let at = Cursor::at_run(runs.len());
        Self {
            timeline: Timeline::of(&runs),
            live: (*finished).clone(),
            runs,
            checkpoints,
            finished,
            at,
            dirty: None,
        }
    }

    /// The job's timeline.
    #[must_use]
    pub const fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Where it has got to.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.at
    }

    /// Its time into the job, seconds.
    #[must_use]
    pub fn time_s(&self) -> f64 {
        self.timeline.time_of(self.at)
    }

    /// Whether it is at the job's end.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.at.run >= self.runs.len()
    }

    /// The stock as it is at the cursor.
    #[must_use]
    pub const fn field(&self) -> &Heightfield {
        &self.live
    }

    /// The cells that changed since the last call, and forgets them: the
    /// whole grid after a seek backwards, since restoring raises cells.
    pub fn take_dirty(&mut self) -> Option<CellRect> {
        self.dirty.take()
    }

    /// Moves toward `t_s` seconds into the job, cutting what it passes,
    /// until it is there or `budget_cells` cells have been tested, and
    /// returns the time reached. A move is always finished or cut to the
    /// asked place once started, so a budget of 0 still makes progress.
    pub fn seek(&mut self, t_s: f64, budget_cells: u64) -> f64 {
        self.seek_to(self.timeline.at(t_s), budget_cells);
        self.time_s()
    }

    /// Moves to the end of run `run`: the stock as that run leaves it.
    pub fn to_run_end(&mut self, run: usize) {
        self.seek_to(Cursor::at_run((run + 1).min(self.runs.len())), u64::MAX);
    }

    /// Steps `n` cutting moves on (or back, for a negative `n`): to the
    /// end of the `n`-th cutting move after the cursor, or of the
    /// `−n`-th before the one it is on.
    pub fn step_moves(&mut self, n: i64) {
        let mut ends = Vec::new();
        for (run, r) in self.runs.iter().enumerate() {
            for (mv, m) in r.moves.iter().enumerate() {
                if m.cuts() {
                    ends.push(self.after(run, mv));
                }
            }
        }
        // How many cutting moves are wholly done. Part-way along a
        // cutting move, the first step back is to that move's start;
        // anywhere else (on a move's end, or among the rapids after the
        // last cut, as at the job's end) it undoes a whole cutting move,
        // so a step back always changes the stock.
        let done = ends.partition_point(|end| *end <= self.at);
        let inside_a_cut = self.at.fraction > 0.0
            && self
                .runs
                .get(self.at.run)
                .and_then(|r| r.moves.get(self.at.mv))
                .is_some_and(crate::cut::SimMove::cuts);
        let back_to_start = i64::from(n < 0 && inside_a_cut);
        let wanted = i64::try_from(done)
            .unwrap_or(i64::MAX)
            .saturating_add(n)
            .saturating_add(back_to_start);
        let target = match usize::try_from(wanted) {
            Ok(0) | Err(_) => Cursor::default(),
            Ok(count) => ends
                .get(count - 1)
                .copied()
                .unwrap_or_else(|| Cursor::at_run(self.runs.len())),
        };
        self.seek_to(target, u64::MAX);
    }

    /// The place just after move `mv` of run `run`.
    fn after(&self, run: usize, mv: usize) -> Cursor {
        let last = self.runs.get(run).is_none_or(|r| mv + 1 >= r.moves.len());
        if last {
            Cursor::at_run(run + 1)
        } else {
            Cursor {
                run,
                mv: mv + 1,
                fraction: 0.0,
            }
        }
    }

    /// Whether it is at the job's start: no time has passed.
    #[must_use]
    pub fn is_at_start(&self) -> bool {
        self.time_s() <= 0.0
    }

    /// Where the tool is: on the move under way. `None` at the very
    /// start and at the very end.
    #[must_use]
    pub fn pose(&self) -> Option<Pose> {
        if self.is_at_start() || self.is_finished() {
            return None;
        }
        let mv = self.runs.get(self.at.run)?.moves.get(self.at.mv)?;
        Some(Pose {
            tip: mv.point_at(self.at.fraction),
            run: self.at.run,
            cuts: mv.cuts(),
        })
    }

    fn seek_to(&mut self, target: Cursor, budget_cells: u64) {
        if target < self.at {
            self.restore(target);
        }
        let mut spent = 0;
        while self.at < target {
            let Some(run) = self.runs.get(self.at.run) else {
                break;
            };
            let Some(mv) = run.moves.get(self.at.mv) else {
                // A run with no move left: on to the next.
                self.at = Cursor::at_run(self.at.run + 1);
                continue;
            };
            let on_target = (self.at.run, self.at.mv) == (target.run, target.mv);
            let to = if on_target { target.fraction } else { 1.0 };
            if mv.cuts() && to > self.at.fraction {
                let owner = u16::try_from(self.at.run).unwrap_or(UNCUT - 1);
                let stamp = self
                    .live
                    .cut(mv, (self.at.fraction, to), &run.cutter.cutter, owner);
                spent += stamp.tested;
                if let Some(changed) = stamp.changed {
                    self.dirty = Some(self.dirty.map_or(changed, |d| d.union(changed)));
                }
            }
            self.at = if on_target {
                target
            } else {
                self.after(self.at.run, self.at.mv)
            };
            if spent >= budget_cells {
                break;
            }
        }
        if self.is_finished() {
            // The end is the build itself, to the bit and the owner: a
            // move played in parts agrees with it only to rounding.
            if self.live.restore(&self.finished) {
                self.dirty = Some(self.whole());
            }
        }
    }

    /// Puts the stock back as it was at or before `target`'s run.
    fn restore(&mut self, target: Cursor) {
        let kept = self
            .checkpoints
            .at_or_before(target.run.min(self.runs.len()))
            .map(|(runs_cut, field)| (runs_cut, Arc::clone(field)));
        if let Some((runs_cut, field)) = kept {
            self.live.restore(&field);
            self.at = Cursor::at_run(runs_cut);
            self.dirty = Some(self.whole());
        }
        // With no checkpoint at all (a caller that kept none), the
        // cursor stays: there is nothing to go back to.
    }

    fn whole(&self) -> CellRect {
        let grid = self.live.grid();
        CellRect {
            i0: 0,
            i1: grid.nx(),
            j0: 0,
            j1: grid.ny(),
        }
    }
}

#[cfg(test)]
mod tests;
