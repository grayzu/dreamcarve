// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Material-removal simulation for DreamCarve (phase 6 functional spec;
//! `docs/specs/phase-6/design.md`, "dc-sim (new crate)"; ADR-0003): the
//! stock as a grid of heights, the shapes cutters have, and the cuts that
//! lower the grid.
//!
//! A board is divided into square cells ([`Grid`]); a [`Heightfield`]
//! holds the stock's height at each cell's centre, from the board's
//! bottom, and the run that cut the cell last. A [`Cutter`] is a tool's
//! cutting surface, and [`Heightfield::cut`] lowers the stock under one
//! [`SimMove`] of it. [`build`] cuts a job's [`SimRun`]s in order,
//! handing back the stock after each as a checkpoint; a [`Timeline`]
//! places every move in time, and a [`Playback`] shows the stock at any
//! moment of it. [`facts`] says what the cut stock is like, and
//! [`compare`] where it still stands above the [`Design`] its operations'
//! shapes ask for: uncut material. Everything is in project space: Y-up, Z-up,
//! millimetres, Z = 0 at the board's bottom (ADR-0001). The grid has no
//! flip and no handedness rule.
//!
//! The crate is a reader (S22): it knows `dc-geometry` and `dc-project`
//! and nothing above them, takes its own plain moves, and nothing in a
//! generator or a post-processor reads what it finds. It does no I/O,
//! starts no thread and never panics; what it cannot do is a
//! [`SimError`].

mod build;
mod checkpoints;
mod compare;
mod cut;
mod cutter;
mod design;
mod digest;
mod edt;
mod facts;
mod field;
mod grid;
mod playback;
pub mod sweep;
mod timeline;

pub use build::{CHUNK_CELLS, Outcome, Progress, SimRun, build};
pub use checkpoints::{CHECKPOINT_BYTES, Checkpoints};
pub use compare::{
    Comparison, MIN_PLACE_CELLS, Place, UNCUT_FLAG, UNCUT_MM, UNCUT_RANGE_MM, compare,
};
pub use cut::{CHORD_MM, Motion, SimArc, SimMove, Stamp, Turn};
pub use cutter::{Cutter, CutterShape};
pub use design::{BandSide, Design};
pub use digest::Digest;
pub use facts::{Facts, facts};
pub use field::{Heightfield, UNCUT};
pub use grid::{CellRect, CellSpec, Grid, MAX_CELLS, MIN_CELL_MM, Quality};
pub use playback::{Playback, Pose};
pub use timeline::{Cursor, Timeline};

/// Why a simulation cannot be set up.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimError {
    /// The board has no size the grid can cover.
    #[error("a board {width_mm} × {height_mm} mm has no size to simulate")]
    BadBoard {
        /// The width asked, mm.
        width_mm: f64,
        /// The height asked, mm.
        height_mm: f64,
    },
    /// The board has no thickness to cut into.
    #[error("a board {thickness_mm} mm thick has nothing to cut")]
    BadThickness {
        /// The thickness asked, mm.
        thickness_mm: f64,
    },
    /// The cell size asked is no size, or is under [`MIN_CELL_MM`].
    #[error("a cell must be at least {MIN_CELL_MM} mm, not {cell_mm}")]
    BadCell {
        /// The cell size asked, mm.
        cell_mm: f64,
    },
    /// The board at this cell size has more than [`MAX_CELLS`] cells.
    #[error(
        "cells of {cell_mm} mm make {cells} cells, more than the {MAX_CELLS} a simulation holds; the smallest cell that fits is {least_cell_mm} mm"
    )]
    TooManyCells {
        /// The cell size asked, mm.
        cell_mm: f64,
        /// How many cells it makes.
        cells: u64,
        /// The smallest cell the board fits in, mm, rounded up to a
        /// micrometre.
        least_cell_mm: f64,
    },
    /// The board's thickness is not set. Validation refuses the project
    /// first.
    #[error("the board's thickness is not set")]
    NoThickness,
    /// More runs than a cell can name the last of.
    #[error("{runs} tool runs are more than a simulation holds")]
    TooManyRuns {
        /// How many runs were asked.
        runs: usize,
    },
    /// A tool has no shape: its diameter or angle is no size, or a
    /// tapered ball nose has a value
    /// missing or out of range. Validation refuses the project first.
    #[error("tool `{tool}` has no shape to cut with: a size it needs is unset or out of range")]
    NoShape {
        /// The tool's name.
        tool: String,
    },
}
