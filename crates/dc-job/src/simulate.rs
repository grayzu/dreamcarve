// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! From generated runs to a simulation (phase 6 functional spec,
//! capability 108; `docs/specs/phase-6/design.md`, "Runs in order, and
//! what is reused"): the plan both shells hand to `dc-sim`.
//!
//! A [`Plan`] has the runs of each board in the order the programs cut
//! them. That order is the order of the generated runs: every enabled
//! operation in the project's order, each operation's runs in its own cut
//! order, the stock's before the male board's. `dc_gcode::emit_programs`
//! writes that sequence and only divides it into programs where the tool
//! changes; it never reorders it. The simulation therefore reads the runs
//! export writes (S22), whatever the split.
//!
//! Each toolpath move becomes a [`SimMove`] with its time: a cut's length
//! over its feed, a dwell's seconds, a rapid's length over the user's
//! `[estimate]` rapid rate, or no time when that is unset; all times the
//! user's scale factor. This is the estimator's arithmetic on the
//! toolpath, where the estimator reads the written program, so a run's
//! time here leaves out what only the program has: the spindle's spin-up
//! dwell, and what arc fitting does to a length.

mod design;
mod picture;
mod report;

pub use design::design;
pub use picture::{PICTURE_SOFTWARE, heightmap_png, is_heightmap};
pub use report::{describe, describe_duration, describe_uncut};

use dc_geometry::Point3;
use dc_project::{Board, Operation, OperationId, Project, Stock, ToolId};
use dc_sim::{CutterShape, Digest, Motion, SimError, SimMove, SimRun, Turn};

use crate::summary::EstimateConfig;
use crate::{EmitOp, Move, MoveKind, RunLabel, Toolpath};

const SECONDS_PER_MINUTE: f64 = 60.0;

/// What a project's runs cut, board by board.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The stock, then the male board when a run cuts it.
    pub boards: Vec<BoardPlan>,
}

impl Plan {
    /// The plan of `board`, if the project cuts it.
    #[must_use]
    pub fn board(&self, board: Board) -> Option<&BoardPlan> {
        self.boards.iter().find(|b| b.board == board)
    }
}

/// One board and the runs that cut it, in cut order.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardPlan {
    /// Which board.
    pub board: Board,
    /// Its extent along X, mm.
    pub width_mm: f64,
    /// Its extent along Y, mm.
    pub height_mm: f64,
    /// Its thickness, mm.
    pub thickness_mm: f64,
    /// Its runs, in the order the programs cut them.
    pub runs: Vec<PlannedRun>,
}

impl BoardPlan {
    /// The runs as the simulation cuts them.
    #[must_use]
    pub fn sim_runs(&self) -> Vec<SimRun> {
        self.runs.iter().map(|r| r.sim.clone()).collect()
    }
}

/// One tool run of the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRun {
    /// Its operation.
    pub op: OperationId,
    /// The operation's place in the project's list.
    pub op_index: usize,
    /// Which of the operation's runs it is.
    pub label: RunLabel,
    /// Its tool.
    pub tool: ToolId,
    /// The run as the simulation reads it.
    pub sim: SimRun,
    /// What changes when anything the simulation reads of the run does:
    /// its moves, its cutter and its board's size.
    pub key: u64,
}

/// The plan of `project`'s `generated` runs, with the user's `estimate`
/// settings for the moves' times. Runs of disabled operations are left
/// out, as the emitter leaves them out.
///
/// # Errors
///
/// [`SimError::NoThickness`] for a board whose thickness is unset, and
/// [`SimError::NoShape`] for a run whose tool has no shape. Validation
/// refuses both before any run is generated.
pub fn plan(
    project: &Project,
    generated: &[EmitOp],
    estimate: &EstimateConfig,
) -> Result<Plan, SimError> {
    let mut boards = Vec::new();
    for board in [Board::Female, Board::Male] {
        let on_board: Vec<&EmitOp> = generated
            .iter()
            .filter(|run| run.op.enabled() && run.board == board)
            .collect();
        let stock = match board {
            Board::Female => project.stock.clone(),
            Board::Male => match male_stock(project, &on_board) {
                Some(stock) => stock,
                None => continue,
            },
        };
        let thickness_mm = stock.thickness_mm.ok_or(SimError::NoThickness)?;
        let mut runs = Vec::with_capacity(on_board.len());
        for run in on_board {
            let sim = SimRun {
                cutter: CutterShape::of(&run.tool)?,
                moves: sim_moves(&run.toolpath, estimate),
            };
            let mut key = Digest::new();
            key.u64(sim.key());
            key.f64(stock.width_mm);
            key.f64(stock.height_mm);
            key.f64(thickness_mm);
            let id = run.op.id();
            runs.push(PlannedRun {
                op: id.clone(),
                op_index: project
                    .operations
                    .iter()
                    .position(|op| op.id() == id)
                    .unwrap_or(usize::MAX),
                label: run.label.clone(),
                tool: run.tool.id.clone(),
                sim,
                key: key.finish(),
            });
        }
        boards.push(BoardPlan {
            board,
            width_mm: stock.width_mm,
            height_mm: stock.height_mm,
            thickness_mm,
            runs,
        });
    }
    Ok(Plan { boards })
}

/// The male board the runs `on_board` cut, as the emitter finds it: the
/// board of the first run's Inlay. `None` when nothing cuts a male.
fn male_stock(project: &Project, on_board: &[&EmitOp]) -> Option<Stock> {
    on_board.iter().find_map(|run| match &run.op {
        Operation::Inlay(inlay) => Some(inlay.male_board.stock(&project.stock)),
        _ => None,
    })
}

/// A toolpath's moves as the simulation reads them. A toolpath's first
/// move has no start of its own: it starts where it ends, and cuts
/// nothing, as phase 3's reference reads it.
#[must_use]
pub fn sim_moves(toolpath: &Toolpath, estimate: &EstimateConfig) -> Vec<SimMove> {
    let mut moves = Vec::with_capacity(toolpath.moves.len());
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        moves.push(sim_move(from, mv, estimate));
        from = Some(mv.to);
    }
    moves
}

fn sim_move(from: Option<Point3>, mv: &Move, estimate: &EstimateConfig) -> SimMove {
    let start = from.unwrap_or(mv.to);
    let shape = match mv.kind {
        MoveKind::Rapid | MoveKind::Dwell { .. } => SimMove::travel(start, mv.to, 0.0),
        _ if from.is_none() => SimMove::travel(start, mv.to, 0.0),
        MoveKind::Linear => SimMove::cut(start, mv.to, 0.0),
        MoveKind::ArcCw { centre } => SimMove::cut(start, mv.to, 0.0).along(centre, Turn::Cw),
        MoveKind::ArcCcw { centre } => SimMove::cut(start, mv.to, 0.0).along(centre, Turn::Ccw),
    };
    let length_mm = shape.length_mm();
    let seconds = match (mv.kind, shape.motion) {
        (MoveKind::Dwell { seconds }, _) => seconds,
        (MoveKind::Rapid, _) => estimate
            .rapid_mm_min
            .map_or(0.0, |rate| length_mm / rate * SECONDS_PER_MINUTE),
        (_, Motion::Cut) => mv
            .feed_mm_min
            .map_or(0.0, |feed| length_mm / feed * SECONDS_PER_MINUTE),
        (_, Motion::Travel) => 0.0,
    };
    let seconds = seconds * estimate.scale_factor;
    SimMove {
        // A time that is no number (a feed of 0, which validation
        // refuses) takes none.
        seconds: if seconds.is_finite() && seconds > 0.0 {
            seconds
        } else {
            0.0
        },
        ..shape
    }
}
