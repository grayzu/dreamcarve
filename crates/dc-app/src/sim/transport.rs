// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The playback transport (phase 6 functional spec, capability 113):
//! play and pause, a place the scrubber asks for, and a speed, over a
//! [`Playback`]. The window calls [`Transport::frame`] once a frame with
//! the time since the last; everything else is here, with no window.
//!
//! A frame cuts at most [`FRAME_CELLS`] cells' worth of moves, so on a
//! job too heavy to keep up the transport's clock slows and the window
//! does not: the guard is a count of work, never a time.

use dc_job::sim::{CellRect, Heightfield, Playback, Pose};

/// The cells a frame may test: about two million, a few milliseconds.
pub const FRAME_CELLS: u64 = 1 << 21;

/// How long a job should take to play at the speed it opens with,
/// seconds.
pub const OPENING_PLAY_S: f64 = 30.0;

/// How fast playback runs, as a multiple of the programmed feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Speed {
    /// The feed itself.
    X1,
    /// Five times.
    X5,
    /// Twenty-five times.
    #[default]
    X25,
    /// A hundred times.
    X100,
    /// Five hundred times.
    X500,
}

impl Speed {
    /// Every speed, slowest first.
    pub const ALL: [Self; 5] = [Self::X1, Self::X5, Self::X25, Self::X100, Self::X500];

    /// The multiple.
    #[must_use]
    pub const fn factor(self) -> f64 {
        match self {
            Self::X1 => 1.0,
            Self::X5 => 5.0,
            Self::X25 => 25.0,
            Self::X100 => 100.0,
            Self::X500 => 500.0,
        }
    }

    /// The multiple as a whole number: what `gui.toml` keeps.
    #[must_use]
    pub const fn multiple(self) -> u32 {
        match self {
            Self::X1 => 1,
            Self::X5 => 5,
            Self::X25 => 25,
            Self::X100 => 100,
            Self::X500 => 500,
        }
    }

    /// The speed of `multiple`, if it is one of the five.
    #[must_use]
    pub fn of_multiple(multiple: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.multiple() == multiple)
    }

    /// As the button shows it: `25×`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::X1 => "1×",
            Self::X5 => "5×",
            Self::X25 => "25×",
            Self::X100 => "100×",
            Self::X500 => "500×",
        }
    }

    /// The next speed up, or this one at the top.
    #[must_use]
    pub fn faster(self) -> Self {
        Self::ALL
            .into_iter()
            .find(|speed| *speed > self)
            .unwrap_or(self)
    }

    /// The next speed down, or this one at the bottom.
    #[must_use]
    pub fn slower(self) -> Self {
        Self::ALL
            .into_iter()
            .rev()
            .find(|speed| *speed < self)
            .unwrap_or(self)
    }

    /// The speed that plays a job `total_s` long in nearest to
    /// [`OPENING_PLAY_S`], by ratio: what a job opens at.
    #[must_use]
    pub fn opening(total_s: f64) -> Self {
        let wanted = (total_s / OPENING_PLAY_S).max(f64::MIN_POSITIVE);
        let mut best = Self::X1;
        for speed in Self::ALL {
            let off = |s: Self| (s.factor() / wanted).ln().abs();
            if off(speed) < off(best) {
                best = speed;
            }
        }
        best
    }
}

/// A job being played, paused or scrubbed.
#[derive(Debug, Clone)]
pub struct Transport {
    playback: Playback,
    playing: bool,
    speed: Speed,
    /// A place asked for and not yet reached, seconds into the job.
    wanted_s: Option<f64>,
}

impl Transport {
    /// `playback`, paused where it is, at `speed`.
    #[must_use]
    pub const fn new(playback: Playback, speed: Speed) -> Self {
        Self {
            playback,
            playing: false,
            speed,
            wanted_s: None,
        }
    }

    /// Whether it is playing.
    #[must_use]
    pub const fn playing(&self) -> bool {
        self.playing
    }

    /// The speed.
    #[must_use]
    pub const fn speed(&self) -> Speed {
        self.speed
    }

    /// Sets the speed.
    pub fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
    }

    /// The job's length, seconds.
    #[must_use]
    pub fn total_s(&self) -> f64 {
        self.playback.timeline().total_s()
    }

    /// The time into the job the stock is shown at, seconds.
    #[must_use]
    pub fn time_s(&self) -> f64 {
        self.playback.time_s()
    }

    /// The time the scrubber shows: the place asked for while it is
    /// still being reached, else the time shown.
    #[must_use]
    pub fn shown_s(&self) -> f64 {
        self.wanted_s.unwrap_or_else(|| self.time_s())
    }

    /// The job as it is played.
    #[must_use]
    pub const fn playback(&self) -> &Playback {
        &self.playback
    }

    /// The stock as it is shown.
    #[must_use]
    pub const fn field(&self) -> &Heightfield {
        self.playback.field()
    }

    /// Where the tool is, when it is drawn.
    #[must_use]
    pub fn pose(&self) -> Option<Pose> {
        self.playback.pose()
    }

    /// The cells that changed since the last call.
    pub fn take_dirty(&mut self) -> Option<CellRect> {
        self.playback.take_dirty()
    }

    /// Play: from the start when the job is at its end. The end is the
    /// job's last moment, whether or not the rapids that take no time
    /// after the last cut have been passed: a step forward to the last
    /// cutting move leaves the job there, and Play would otherwise end
    /// at once.
    pub fn play(&mut self) {
        let at_end = self.playback.is_finished() || self.time_s() >= self.total_s();
        if at_end && self.wanted_s.is_none() {
            self.wanted_s = Some(0.0);
        }
        self.playing = true;
    }

    /// Pause.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Play when paused, pause when playing.
    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Asks for the stock as it is `t_s` seconds into the job: a drag of
    /// the scrubber. Playing stops.
    pub fn scrub(&mut self, t_s: f64) {
        self.playing = false;
        self.wanted_s = Some(t_s.clamp(0.0, self.total_s()));
    }

    /// Shows the stock as run `run` leaves it. Playing stops.
    pub fn to_run_end(&mut self, run: usize) {
        self.playing = false;
        self.wanted_s = None;
        self.playback.to_run_end(run);
    }

    /// Steps `n` cutting moves on, or back. Playing stops.
    pub fn step_moves(&mut self, n: i64) {
        self.playing = false;
        self.wanted_s = None;
        self.playback.step_moves(n);
    }

    /// One frame, `dt_s` seconds after the last: moves toward the place
    /// asked for, or on by the time passed at the speed while playing,
    /// cutting at most [`FRAME_CELLS`] cells. Returns whether another
    /// frame is wanted: it is still playing, or a place asked for is not
    /// reached.
    pub fn frame(&mut self, dt_s: f64) -> bool {
        let dt_s = if dt_s.is_finite() {
            dt_s.clamp(0.0, 0.1)
        } else {
            0.0
        };
        if let Some(wanted) = self.wanted_s {
            let reached = self.playback.seek(wanted, FRAME_CELLS);
            // A seek back restores first and may stop short; a seek on
            // stops short when the cells are spent.
            if (reached - wanted).abs() <= 1e-9
                || (wanted >= self.total_s() && self.playback.is_finished())
            {
                self.wanted_s = None;
            }
        } else if self.playing {
            let target = self.time_s() + dt_s * self.speed.factor();
            self.playback.seek(target, FRAME_CELLS);
            if self.playback.is_finished() {
                self.playing = false;
            }
        }
        self.playing || self.wanted_s.is_some()
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

    use std::sync::Arc;

    use super::*;
    use dc_geometry::{Point3, assert_approx_eq};
    use dc_job::sim::{
        CellSpec, Checkpoints, Cutter, CutterShape, Grid, Outcome, SimMove, SimRun, build,
    };

    /// Two runs of level passes on a board 60 × 40, 20 s and 10 s long.
    fn transport() -> (Transport, Arc<Heightfield>) {
        let pass = |y: f64, z: f64, seconds: f64| {
            SimMove::cut(Point3::new(5.0, y, z), Point3::new(55.0, y, z), seconds)
        };
        let shape = CutterShape {
            cutter: Cutter::Flat { radius_mm: 3.0 },
            plain: false,
        };
        let runs = vec![
            SimRun {
                cutter: shape,
                moves: vec![pass(8.0, 9.0, 10.0), pass(14.0, 9.0, 10.0)],
            },
            SimRun {
                cutter: shape,
                moves: vec![pass(30.0, 6.0, 10.0)],
            },
        ];
        let grid = Grid::of(60.0, 40.0, CellSpec::CellMm(0.5)).unwrap();
        let stock = Heightfield::uncut(grid, 12.0).unwrap();
        let mut kept = Checkpoints::default();
        let Outcome::Finished(finished) = build(
            stock,
            &runs,
            0,
            &mut |cut, f| kept.keep(cut, f),
            &mut |_| true,
        )
        .unwrap() else {
            panic!("not told to stop");
        };
        let finished = Arc::new(finished);
        let playback = Playback::at_end(runs.into(), kept, Arc::clone(&finished));
        (Transport::new(playback, Speed::X1), finished)
    }

    /// A job opens at the speed that plays it in nearest to half a
    /// minute; the speeds step up and down and stop at their ends.
    #[test]
    fn a_job_opens_at_the_speed_that_plays_it_in_half_a_minute() {
        assert_eq!(Speed::opening(20.0), Speed::X1);
        assert_eq!(Speed::opening(150.0), Speed::X5);
        assert_eq!(Speed::opening(14.0 * 60.0), Speed::X25);
        assert_eq!(Speed::opening(3600.0), Speed::X100);
        assert_eq!(Speed::opening(8.0 * 3600.0), Speed::X500);
        assert_eq!(Speed::opening(0.0), Speed::X1);
        assert_eq!(Speed::X25.faster(), Speed::X100);
        assert_eq!(Speed::X500.faster(), Speed::X500);
        assert_eq!(Speed::X25.slower(), Speed::X5);
        assert_eq!(Speed::X1.slower(), Speed::X1);
        assert_eq!(Speed::X500.label(), "500×");
    }

    /// Acceptance 139: a job opens at its end; Play starts it from the
    /// start, the time runs at the speed, the tool is drawn while it
    /// runs, and at the end it stops with the stock it opened with.
    #[test]
    fn play_runs_from_the_start_to_the_build() {
        let (mut transport, finished) = transport();
        assert!(!transport.playing());
        assert_approx_eq!(transport.time_s(), 30.0, 1e-12);
        assert!(transport.pose().is_none());
        assert!(!transport.frame(0.016), "paused and nowhere to go");

        transport.play();
        assert!(transport.frame(0.0), "back to the start, and playing");
        assert_approx_eq!(transport.time_s(), 0.0, 1e-12);
        transport.set_speed(Speed::X25);
        assert!(transport.frame(0.1));
        // A frame is held to a tenth of a second: 2.5 s at 25×.
        assert_approx_eq!(transport.time_s(), 2.5, 1e-9);
        assert!(transport.frame(5.0));
        assert_approx_eq!(transport.time_s(), 5.0, 1e-9);
        assert!(transport.pose().is_some());
        transport.pause();
        assert!(!transport.frame(0.1));
        assert_approx_eq!(transport.time_s(), 5.0, 1e-9);
        transport.toggle();
        let mut frames = 0;
        while transport.frame(0.1) {
            frames += 1;
            assert!(frames < 100);
        }
        assert!(!transport.playing());
        assert_eq!(transport.field(), &*finished);
        assert!(transport.pose().is_none());

        // A step back and on again leaves the job at its last moment
        // with only rapids that take no time still to pass: Play starts
        // again from there too, and does not end at once.
        transport.step_moves(-1);
        transport.step_moves(1);
        assert_approx_eq!(transport.time_s(), transport.total_s(), 1e-12);
        transport.play();
        assert!(transport.frame(0.0));
        assert_approx_eq!(transport.time_s(), 0.0, 1e-12);
        assert!(transport.playing());
    }

    /// A drag of the scrubber shows the stock at that moment, backwards
    /// as well as forwards, and stops the playing; a click on a run shows
    /// the stock as it leaves it.
    #[test]
    fn the_scrubber_and_the_rows_place_the_job() {
        let (mut transport, _) = transport();
        transport.play();
        transport.scrub(12.0);
        assert!(!transport.playing());
        assert_approx_eq!(transport.shown_s(), 12.0, 0.0);
        let mut frames = 0;
        while transport.frame(0.016) {
            frames += 1;
            assert!(frames < 100);
        }
        assert_approx_eq!(transport.time_s(), 12.0, 1e-9);
        assert_approx_eq!(transport.pose().unwrap().tip.x, 15.0, 1e-9);
        transport.scrub(999.0);
        assert_approx_eq!(transport.shown_s(), 30.0, 0.0);
        while transport.frame(0.016) {}
        assert!(transport.playback().is_finished());

        transport.to_run_end(0);
        assert_approx_eq!(transport.time_s(), 20.0, 1e-12);
        assert!(transport.take_dirty().is_some());
        transport.step_moves(-1);
        assert_approx_eq!(transport.time_s(), 10.0, 1e-12);
        transport.step_moves(2);
        assert_approx_eq!(transport.time_s(), 30.0, 1e-12);
    }
}
