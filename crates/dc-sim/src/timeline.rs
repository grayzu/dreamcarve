// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where each move is in time (`docs/specs/phase-6/design.md`,
//! "Algorithms → The timeline and playback"): the running sum of the
//! moves' seconds over every run, and the [`Cursor`] a time falls on.
//!
//! Upstream's playback is placed by path length, one operation at a
//! time, with rapids at the cut feed (the survey, section 8.6); here the
//! whole job is placed by time, each move as long as its feed makes it.

use crate::build::SimRun;

/// A place in a job: every move before move `mv` of run `run` is done,
/// and that move is done as far as `fraction` of its length. The end of
/// a run is the start of the next; the end of the job is run
/// `runs.len()`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Cursor {
    /// The run, as its place in the plan.
    pub run: usize,
    /// The move, as its place in the run.
    pub mv: usize,
    /// How far along that move, from 0 to 1.
    pub fraction: f64,
}

impl Cursor {
    /// The start of run `run`, which is the end of the run before it.
    #[must_use]
    pub const fn at_run(run: usize) -> Self {
        Self {
            run,
            mv: 0,
            fraction: 0.0,
        }
    }
}

/// The times at which each move of a job ends.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// For each run, the time each of its moves ends, seconds from the
    /// job's start.
    ends_s: Vec<Vec<f64>>,
    /// The time each run starts, and after the last, the job's length.
    run_starts_s: Vec<f64>,
}

impl Timeline {
    /// The timeline of `runs`. A move whose seconds are no number or
    /// below 0 takes none.
    #[must_use]
    pub fn of(runs: &[SimRun]) -> Self {
        let mut now_s = 0.0;
        let mut run_starts_s = Vec::with_capacity(runs.len() + 1);
        let mut ends_s = Vec::with_capacity(runs.len());
        for run in runs {
            run_starts_s.push(now_s);
            let mut ends = Vec::with_capacity(run.moves.len());
            for mv in &run.moves {
                if mv.seconds.is_finite() && mv.seconds > 0.0 {
                    now_s += mv.seconds;
                }
                ends.push(now_s);
            }
            ends_s.push(ends);
        }
        run_starts_s.push(now_s);
        Self {
            ends_s,
            run_starts_s,
        }
    }

    /// The job's length, seconds.
    #[must_use]
    pub fn total_s(&self) -> f64 {
        self.run_starts_s.last().copied().unwrap_or(0.0)
    }

    /// How many runs it has.
    #[must_use]
    pub fn runs(&self) -> usize {
        self.ends_s.len()
    }

    /// When run `run` starts and ends, seconds; `None` for a run it does
    /// not have.
    #[must_use]
    pub fn run_span_s(&self, run: usize) -> Option<(f64, f64)> {
        Some((
            *self.run_starts_s.get(run)?,
            *self.run_starts_s.get(run + 1)?,
        ))
    }

    /// When the move at (`run`, `mv`) starts and ends.
    fn move_span_s(&self, run: usize, mv: usize) -> Option<(f64, f64)> {
        let ends = self.ends_s.get(run)?;
        let end = *ends.get(mv)?;
        let start = match mv.checked_sub(1) {
            Some(before) => *ends.get(before)?,
            None => *self.run_starts_s.get(run)?,
        };
        Some((start, end))
    }

    /// The place `t_s` seconds into the job falls on: the first move
    /// that ends after it, as far along as the time is; a move that
    /// takes no time is passed as soon as it is reached. A time at or
    /// past the job's length is its end, and one before 0 its start.
    #[must_use]
    pub fn at(&self, t_s: f64) -> Cursor {
        // A time that is no number is the start.
        let t_s = if t_s.is_nan() { 0.0 } else { t_s.max(0.0) };
        if t_s >= self.total_s() {
            return Cursor::at_run(self.runs());
        }
        // The first run that ends after `t_s`, then its first such move.
        let run = self
            .run_starts_s
            .partition_point(|&start| start <= t_s)
            .saturating_sub(1)
            .min(self.runs().saturating_sub(1));
        let mut run = run;
        while self
            .run_starts_s
            .get(run + 1)
            .is_some_and(|&end| end <= t_s)
        {
            run += 1;
        }
        let Some(ends) = self.ends_s.get(run) else {
            return Cursor::at_run(self.runs());
        };
        let mv = ends.partition_point(|&end| end <= t_s);
        match self.move_span_s(run, mv) {
            Some((start, end)) if end > start => Cursor {
                run,
                mv,
                fraction: ((t_s - start) / (end - start)).clamp(0.0, 1.0),
            },
            _ => Cursor::at_run(run + 1),
        }
    }

    /// The time of a place, seconds: the inverse of [`Timeline::at`] for
    /// a place on a move that takes time.
    #[must_use]
    pub fn time_of(&self, cursor: Cursor) -> f64 {
        match self.move_span_s(cursor.run, cursor.mv) {
            Some((start, end)) => start + (end - start) * cursor.fraction.clamp(0.0, 1.0),
            // Past a run's last move, or past the last run.
            None => self
                .run_starts_s
                .get(cursor.run + usize::from(cursor.mv > 0))
                .copied()
                .unwrap_or_else(|| self.total_s()),
        }
    }
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
    use crate::cut::SimMove;
    use crate::cutter::{Cutter, CutterShape};
    use dc_geometry::{Point3, assert_approx_eq};

    fn run(seconds: &[f64]) -> SimRun {
        let p = Point3::new(0.0, 0.0, 0.0);
        SimRun {
            cutter: CutterShape {
                cutter: Cutter::Flat { radius_mm: 1.0 },
                plain: false,
            },
            moves: seconds.iter().map(|&s| SimMove::cut(p, p, s)).collect(),
        }
    }

    #[test]
    fn a_runs_span_is_its_moves_seconds() {
        let timeline = Timeline::of(&[run(&[1.0, 2.0]), run(&[]), run(&[4.0])]);
        assert_approx_eq!(timeline.total_s(), 7.0, 1e-12);
        assert_eq!(timeline.runs(), 3);
        assert_eq!(timeline.run_span_s(0), Some((0.0, 3.0)));
        assert_eq!(timeline.run_span_s(1), Some((3.0, 3.0)));
        assert_eq!(timeline.run_span_s(2), Some((3.0, 7.0)));
        assert_eq!(timeline.run_span_s(3), None);
        assert_approx_eq!(Timeline::of(&[]).total_s(), 0.0, 0.0);
    }

    /// A time falls on the move it is in, and the place gives the time
    /// back.
    #[test]
    fn time_and_cursor_round_trip() {
        let timeline = Timeline::of(&[run(&[1.0, 2.0]), run(&[4.0, 0.5])]);
        for (t, run, mv, fraction) in [
            (0.0, 0, 0, 0.0),
            (0.25, 0, 0, 0.25),
            (1.0, 0, 1, 0.0),
            (2.0, 0, 1, 0.5),
            (3.0, 1, 0, 0.0),
            (5.0, 1, 0, 0.5),
            (7.25, 1, 1, 0.5),
        ] {
            let cursor = timeline.at(t);
            assert_eq!((cursor.run, cursor.mv), (run, mv), "at {t} s");
            assert_approx_eq!(cursor.fraction, fraction, 1e-12);
            assert_approx_eq!(timeline.time_of(cursor), t, 1e-12);
        }
        // The end, and anything past it; before the start; no number.
        assert_eq!(timeline.at(7.5), Cursor::at_run(2));
        assert_eq!(timeline.at(99.0), Cursor::at_run(2));
        assert_eq!(timeline.at(-3.0), timeline.at(0.0));
        assert_eq!(timeline.at(f64::NAN), timeline.at(0.0));
        assert_approx_eq!(timeline.time_of(Cursor::at_run(2)), 7.5, 1e-12);
        assert_approx_eq!(timeline.time_of(Cursor::at_run(1)), 3.0, 1e-12);
        assert_approx_eq!(timeline.time_of(Cursor::at_run(9)), 7.5, 1e-12);
    }

    /// A move that takes no time (a rapid with no rapid rate set) is
    /// passed as soon as it is reached; a run of them ends at once.
    #[test]
    fn a_zero_second_move_is_passed() {
        let timeline = Timeline::of(&[run(&[0.0, 2.0, 0.0, 0.0]), run(&[0.0]), run(&[0.0, 1.0])]);
        // At 0 the leading rapid is done and the cut is under way.
        assert_eq!(
            timeline.at(0.0),
            Cursor {
                run: 0,
                mv: 1,
                fraction: 0.0
            }
        );
        // At 2 s the cut, the two rapids after it, the empty-timed run
        // and the next run's leading rapid are all done.
        assert_eq!(
            timeline.at(2.0),
            Cursor {
                run: 2,
                mv: 1,
                fraction: 0.0
            }
        );
        assert_eq!(timeline.at(3.0), Cursor::at_run(3));
        // A place on a move that takes no time is at that move's time.
        let on_rapid = Cursor {
            run: 0,
            mv: 2,
            fraction: 0.5,
        };
        assert_approx_eq!(timeline.time_of(on_rapid), 2.0, 1e-12);
        // Seconds that are no number, or below 0, take none.
        let odd = Timeline::of(&[run(&[f64::NAN, -4.0, 1.0])]);
        assert_approx_eq!(odd.total_s(), 1.0, 1e-12);
    }
}
