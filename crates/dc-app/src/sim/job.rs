// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The simulation as a job (phase 6 functional spec, capabilities 108
//! and 123; `docs/specs/phase-6/design.md`, "Runs in order, and what is
//! reused" and "The worker"): what the shell hands its simulation
//! worker, what comes back, and the bookkeeping that says whether the
//! stock on screen is the current project's.
//!
//! A [`SimJob`] names a snapshot, a board and the settings, and carries
//! the last simulation of that board. [`execute`] plans the snapshot's
//! runs, finds the first whose key differs from the last simulation's,
//! and cuts from the checkpoint before it, so a change to the third of
//! five operations cuts three runs and not five. It is the one place the
//! app runs the simulation's engine, as `generation::execute` is the one
//! place it runs the generators; the shell calls it on a thread of its
//! own. Nothing here is read by a generator or reaches a program (S22).

use std::collections::BTreeMap;
use std::sync::Arc;

use dc_job::sim::{
    CellSpec, Checkpoints, Comparison, Design, Facts, Grid, Heightfield, Outcome, Playback,
    Progress, SimRun, build, compare, facts,
};
use dc_job::simulate::{BoardPlan, design, plan};
use dc_job::{EmitOp, EstimateConfig};
use dc_project::{Board, Project};

use crate::generation::SnapshotId;

/// What a simulation is made with: change any and it is made again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimSettings {
    /// The board shown.
    pub board: Board,
    /// How finely it is divided.
    pub spec: CellSpec,
    /// How far above the design the stock stands before it is uncut, mm.
    pub uncut_mm: f64,
}

/// One simulation to make.
#[derive(Debug, Clone, PartialEq)]
pub struct SimJob {
    /// The snapshot it is for.
    pub snapshot: SnapshotId,
    /// What to make it with.
    pub settings: SimSettings,
    /// The last simulation of this board, to reuse what still holds.
    pub previous: Option<Arc<Built>>,
}

/// A finished simulation of one board.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The snapshot it was made for.
    pub snapshot: SnapshotId,
    /// What it was made with.
    pub settings: SimSettings,
    /// The board and its runs, in cut order.
    pub plan: BoardPlan,
    /// The runs as the engine cut them.
    pub runs: Arc<[SimRun]>,
    /// The stock after runs, for playback and the next build.
    pub checkpoints: Checkpoints,
    /// The stock every run leaves.
    pub finished: Arc<Heightfield>,
    /// What the finished stock is like.
    pub facts: Facts,
    /// What the operations' shapes ask for.
    pub design: Arc<Design>,
    /// Where the finished stock stands above it.
    pub comparison: Arc<Comparison>,
    /// How many of the runs were cut for this build; the rest were
    /// taken from the last one.
    pub runs_cut: usize,
}

impl Built {
    /// The job at its end, ready to play.
    #[must_use]
    pub fn playback(&self) -> Playback {
        Playback::at_end(
            Arc::clone(&self.runs),
            self.checkpoints.clone(),
            Arc::clone(&self.finished),
        )
    }
}

/// What a job came to.
#[derive(Debug, Clone, PartialEq)]
pub enum SimResult {
    /// The simulation.
    Built(Arc<Built>),
    /// It was told to stop.
    Stopped {
        /// The snapshot it was for.
        snapshot: SnapshotId,
    },
    /// It could not be made: the project cuts no such board, or the
    /// grid or a tool was refused.
    Failed {
        /// The snapshot it was for.
        snapshot: SnapshotId,
        /// Why, as the engine says it.
        reason: String,
    },
}

impl SimResult {
    /// The snapshot the job was for.
    #[must_use]
    pub fn snapshot(&self) -> SnapshotId {
        match self {
            Self::Built(built) => built.snapshot,
            Self::Stopped { snapshot } | Self::Failed { snapshot, .. } => *snapshot,
        }
    }
}

/// Makes the simulation `job` asks for, of `project`'s `generated` runs,
/// with the user's `estimate` settings for the moves' times.
/// `keep_going` is asked between chunks of work with the progress;
/// `false` stops it. Pure; the shell calls it on its simulation thread.
#[must_use]
pub fn execute(
    project: &Project,
    generated: &[EmitOp],
    job: &SimJob,
    estimate: &EstimateConfig,
    keep_going: &mut dyn FnMut(Progress) -> bool,
) -> SimResult {
    let snapshot = job.snapshot;
    let failed = |reason: String| SimResult::Failed { snapshot, reason };
    let plan = match plan(project, generated, estimate) {
        Ok(plan) => plan,
        Err(error) => return failed(error.to_string()),
    };
    let Some(board) = plan.board(job.settings.board) else {
        return failed(format!(
            "the project cuts no {} board",
            job.settings.board.as_str()
        ));
    };
    let grid = match Grid::of(board.width_mm, board.height_mm, job.settings.spec) {
        Ok(grid) => grid,
        Err(error) => return failed(error.to_string()),
    };
    let runs: Arc<[SimRun]> = board.sim_runs().into();

    // What still holds of the last simulation: the runs before the
    // first whose key differs, on the same grid of the same board.
    let reusable = job.previous.as_deref().filter(|last| {
        *last.finished.grid() == grid
            && (last.finished.top_z_mm() - board.thickness_mm).abs() <= 0.0
            && last.settings.board == job.settings.board
    });
    let same = reusable.map_or(0, |last| {
        last.plan
            .runs
            .iter()
            .zip(&board.runs)
            .take_while(|(a, b)| a.key == b.key)
            .count()
    });
    let mut checkpoints = Checkpoints::default();
    let mut base = None;
    if let Some(last) = reusable
        && let Some((cut, field)) = last.checkpoints.at_or_before(same)
    {
        for k in 0..=cut {
            if let Some(kept) = last.checkpoints.get(k) {
                checkpoints.keep_shared(k, Arc::clone(kept));
            }
        }
        base = Some((cut, Arc::clone(field)));
    }
    let (from, stock) = match base {
        Some((cut, field)) => (cut, (*field).clone()),
        None => match Heightfield::uncut(grid, board.thickness_mm) {
            Ok(stock) => (0, stock),
            Err(error) => return failed(error.to_string()),
        },
    };
    let finished = match build(
        stock,
        &runs,
        from,
        &mut |cut, field| {
            if checkpoints.get(cut).is_none() {
                checkpoints.keep(cut, field);
            }
        },
        keep_going,
    ) {
        Ok(Outcome::Finished(field)) => Arc::new(field),
        Ok(Outcome::Stopped) => return SimResult::Stopped { snapshot },
        Err(error) => return failed(error.to_string()),
    };

    let found = facts(&finished, &runs);
    let asked = design(project, board, generated, grid);
    let uncut = compare(&finished, &asked, job.settings.uncut_mm);
    SimResult::Built(Arc::new(Built {
        snapshot,
        settings: job.settings,
        plan: board.clone(),
        runs_cut: runs.len() - from.min(runs.len()),
        runs,
        checkpoints,
        finished,
        facts: found,
        design: Arc::new(asked),
        comparison: Arc::new(uncut),
    }))
}

/// Where a board's simulation stands for the current snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum SimState {
    /// Nothing has been simulated and nothing is under way.
    Empty,
    /// One is being made; the progress once the first chunk is in.
    Building(Option<Progress>),
    /// What is shown is the current snapshot's, made with these
    /// settings.
    Current,
    /// What is shown is an earlier snapshot's, or other settings'.
    Stale,
    /// The last build was stopped, and nothing since has changed.
    Stopped,
    /// The last build could not be made.
    Failed(String),
}

/// The simulations the app holds: the last made of each board, and what
/// is under way.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Simulation {
    built: BTreeMap<Board, Arc<Built>>,
    /// The job handed out and not yet answered.
    in_flight: Option<(SnapshotId, SimSettings)>,
    progress: Option<Progress>,
    /// What Stop held: not made again until it changes or the user asks.
    stopped: Option<(SnapshotId, SimSettings)>,
    failed: Option<(SnapshotId, SimSettings, String)>,
}

impl Simulation {
    /// The last simulation made of `board`, current or not.
    #[must_use]
    pub fn built(&self, board: Board) -> Option<&Arc<Built>> {
        self.built.get(&board)
    }

    /// Whether the last simulation of the settings' board is `snapshot`'s
    /// and made with `settings`.
    #[must_use]
    pub fn is_current(&self, snapshot: SnapshotId, settings: SimSettings) -> bool {
        self.built(settings.board)
            .is_some_and(|built| built.snapshot == snapshot && built.settings == settings)
    }

    /// Where the simulation of `settings` stands for `snapshot`.
    #[must_use]
    pub fn state(&self, snapshot: SnapshotId, settings: SimSettings) -> SimState {
        if self.is_current(snapshot, settings) {
            return SimState::Current;
        }
        if self.in_flight == Some((snapshot, settings)) {
            return SimState::Building(self.progress);
        }
        if self.stopped == Some((snapshot, settings)) {
            return SimState::Stopped;
        }
        if let Some((at, with, reason)) = &self.failed
            && (*at, *with) == (snapshot, settings)
        {
            return SimState::Failed(reason.clone());
        }
        if self.built(settings.board).is_some() {
            SimState::Stale
        } else {
            SimState::Empty
        }
    }

    /// Whether a job is out and not yet answered.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// The job to hand the worker for `snapshot` with `settings`, when
    /// one is wanted: none is out, the current one is not made yet, and
    /// it was neither stopped nor refused. A job out for another snapshot
    /// or other settings is to be told to stop ([`Self::wants_stop`]).
    pub fn next_job(&mut self, snapshot: SnapshotId, settings: SimSettings) -> Option<SimJob> {
        if self.is_current(snapshot, settings)
            || self.in_flight.is_some()
            || self.stopped == Some((snapshot, settings))
            || self
                .failed
                .as_ref()
                .is_some_and(|(at, with, _)| (*at, *with) == (snapshot, settings))
        {
            return None;
        }
        self.in_flight = Some((snapshot, settings));
        self.progress = None;
        Some(SimJob {
            snapshot,
            settings,
            previous: self.built(settings.board).cloned(),
        })
    }

    /// Whether the worker should be told to stop the job that is out:
    /// it is no longer the one wanted, or the user's Stop holds it.
    #[must_use]
    pub fn wants_stop(&self, snapshot: SnapshotId, settings: SimSettings) -> bool {
        self.in_flight
            .is_some_and(|out| out != (snapshot, settings) || self.stopped == Some(out))
    }

    /// Notes the progress of the job that is out.
    pub fn progressed(&mut self, progress: Progress) {
        if self.in_flight.is_some() {
            self.progress = Some(progress);
        }
    }

    /// Takes a job's result. A simulation is kept, to show until a
    /// newer one lands and to reuse; whether it is the current one is
    /// [`Simulation::state`]'s to say. Returns whether anything shown
    /// changed.
    pub fn accept(&mut self, result: SimResult) -> bool {
        let Some((snapshot, settings)) = self.in_flight.take() else {
            return false;
        };
        self.progress = None;
        match result {
            SimResult::Built(built) => {
                self.built.insert(built.settings.board, built);
                true
            }
            // Held only when the user's Stop asked for it
            // ([`Simulation::stop`]): a job stopped because a newer one
            // was wanted is made again when its turn comes back.
            SimResult::Stopped { .. } => true,
            SimResult::Failed { reason, .. } => {
                self.failed = Some((snapshot, settings, reason));
                true
            }
        }
    }

    /// Stop: the job out is to be stopped by the shell, and its
    /// snapshot and settings are held until they change or
    /// [`Simulation::again`].
    pub fn stop(&mut self) {
        if let Some(out) = self.in_flight {
            self.stopped = Some(out);
        }
    }

    /// Simulate again: what Stop held, or what was refused, may be made.
    pub fn again(&mut self) {
        self.stopped = None;
        self.failed = None;
    }

    /// Forgets everything: another project was opened.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
