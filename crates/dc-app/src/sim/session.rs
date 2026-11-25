// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The simulation as the app holds it (phase 6 functional spec,
//! capabilities 107, 108, 118 and 123): which board is shown, what
//! stands in a simulation's way, and the hand-over of jobs and results
//! between [`App`] and the shell's simulation worker.
//!
//! The simulation waits for the toolpaths: a job is handed out only when
//! generation has nothing left to do, so a toolpath always comes first.
//! It reads every enabled operation's runs that generated; an operation
//! an error stopped has none and is left out, and the panel says so.

use std::sync::Arc;

use dc_job::EmitOp;
use dc_job::sim::{CellSpec, Progress};
use dc_project::{Board, OperationId};

use super::job::{Built, SimJob, SimResult, SimSettings, SimState, Simulation};
use crate::app::App;

/// The app's simulation state: nothing of it is saved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SimSession {
    /// The simulations made, and what is under way.
    pub(crate) simulation: Simulation,
    /// The board the step shows.
    pub(crate) board: Board,
}

/// Why no simulation of the project as it stands can be made now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// No operation is enabled: there is nothing to cut.
    NoOperations,
    /// Validation refuses the project: this many errors.
    Errors(usize),
    /// The toolpaths are being generated; the simulation follows them.
    Generating,
    /// Generation was stopped: the toolpaths are not the project's.
    Held,
}

impl App {
    /// The simulations made and what is under way.
    #[must_use]
    pub fn simulation(&self) -> &Simulation {
        &self.sim.simulation
    }

    /// Whether the project has a male board to show (capability 118).
    #[must_use]
    pub fn has_male_board(&self) -> bool {
        self.scene().male_board().is_some()
    }

    /// The board the Simulate step shows: the stock, unless the male
    /// board was chosen and the project still has one.
    #[must_use]
    pub fn sim_board(&self) -> Board {
        if self.sim.board == Board::Male && self.has_male_board() {
            Board::Male
        } else {
            Board::Female
        }
    }

    /// Chooses the board the Simulate step shows.
    pub fn set_sim_board(&mut self, board: Board) {
        self.sim.board = board;
    }

    /// What a simulation is made with now: the board shown and the
    /// preferences.
    #[must_use]
    pub fn sim_settings(&self) -> SimSettings {
        let prefs = &self.prefs().sim;
        SimSettings {
            board: self.sim_board(),
            spec: CellSpec::Quality(prefs.quality),
            uncut_mm: prefs.uncut_mm(),
        }
    }

    /// Where the simulation of the project as it stands, with the
    /// settings as they are, has got to.
    #[must_use]
    pub fn sim_state(&self) -> SimState {
        self.sim
            .simulation
            .state(self.snapshot(), self.sim_settings())
    }

    /// The last simulation made of the board shown, current or not: what
    /// stays on screen while a newer one is made.
    #[must_use]
    pub fn sim_built(&self) -> Option<&Arc<Built>> {
        self.sim.simulation.built(self.sim_board())
    }

    /// What stands in the way of a simulation of the project as it is,
    /// if anything does.
    #[must_use]
    pub fn sim_blocked(&self) -> Option<Blocked> {
        if !self.project().operations.iter().any(|op| op.enabled()) {
            return Some(Blocked::NoOperations);
        }
        let errors = self.validation().errors.len();
        if errors > 0 {
            return Some(Blocked::Errors(errors));
        }
        if self.generation().busy() {
            return Some(Blocked::Generating);
        }
        if self.generation().current_runs().is_none() {
            return Some(Blocked::Held);
        }
        None
    }

    /// The enabled operations an error left without runs, in the
    /// project's order: the simulation leaves them out (capability 108).
    #[must_use]
    pub fn sim_left_out(&self) -> Vec<OperationId> {
        let generation = self.generation();
        self.project()
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(|op| op.id())
            .filter(|id| {
                generation.is_current(id)
                    && generation
                        .result(id)
                        .is_some_and(|result| result.findings.has_errors())
            })
            .cloned()
            .collect()
    }

    /// The next job for the simulation worker, marked handed out, with
    /// the runs it reads: `None` unless nothing blocks a simulation, the
    /// current one is not made, and no job is out. The shell asks only
    /// while the Simulate step is open.
    pub fn next_sim_job(&mut self) -> Option<(SimJob, Vec<EmitOp>)> {
        if self.sim_blocked().is_some() {
            return None;
        }
        let generated = self.generation().current_runs()?;
        let (snapshot, settings) = (self.snapshot(), self.sim_settings());
        let job = self.sim.simulation.next_job(snapshot, settings)?;
        Some((job, generated))
    }

    /// Whether the worker should be told to stop the job it is on.
    #[must_use]
    pub fn sim_wants_stop(&self) -> bool {
        self.sim
            .simulation
            .wants_stop(self.snapshot(), self.sim_settings())
    }

    /// Notes the progress of the job that is out.
    pub fn sim_progressed(&mut self, progress: Progress) {
        self.sim.simulation.progressed(progress);
    }

    /// Takes the worker's result. Returns whether anything shown changed.
    pub fn accept_sim(&mut self, result: SimResult) -> bool {
        self.sim.simulation.accept(result)
    }

    /// Stop (capability 123): the job out is abandoned and not made
    /// again until the project changes or [`App::simulate_again`].
    pub fn stop_sim(&mut self) {
        self.sim.simulation.stop();
    }

    /// Simulate again: what Stop held, or what could not be made, may be.
    pub fn simulate_again(&mut self) {
        self.sim.simulation.again();
    }
}
