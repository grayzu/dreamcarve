// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The toolpath an operation produces (`docs/specs/phase-1/design.md`,
//! "dc-cam"): ordered moves in project coordinates, before any
//! post-processor sees them.

use dc_geometry::{Point, Point3};

/// Ordered tool motion in project space: Y-up, Z-up, millimetres, stock
/// bottom at Z = 0.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Toolpath {
    /// The moves, in machine order.
    pub moves: Vec<Move>,
}

/// How the tool travels to a move's end point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoveKind {
    /// Maximum-speed positioning, never in material.
    Rapid,
    /// Straight cut at the move's feed.
    Linear,
    /// Clockwise arc (viewed from +Z, Y-up) about `centre`, in the plane
    /// of the end point's Z.
    ArcCw {
        /// Arc centre, mm.
        centre: Point,
    },
    /// Counter-clockwise arc (viewed from +Z, Y-up) about `centre`.
    ArcCcw {
        /// Arc centre, mm.
        centre: Point,
    },
}

/// One tool move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    /// How the tool gets there.
    pub kind: MoveKind,
    /// End point, mm, absolute project coordinates.
    pub to: Point3,
    /// Feed rate, mm/min. `None` for rapids; `Some` for every linear and
    /// arc move, always a value the user supplied on the tool.
    pub feed_mm_min: Option<f64>,
}

impl Move {
    /// A rapid to `to`. Rapids carry no feed.
    #[must_use]
    pub const fn rapid(to: Point3) -> Self {
        Self {
            kind: MoveKind::Rapid,
            to,
            feed_mm_min: None,
        }
    }

    /// A straight cut to `to` at `feed_mm_min`.
    #[must_use]
    pub const fn linear(to: Point3, feed_mm_min: f64) -> Self {
        Self {
            kind: MoveKind::Linear,
            to,
            feed_mm_min: Some(feed_mm_min),
        }
    }

    /// True for a rapid.
    #[must_use]
    pub const fn is_rapid(&self) -> bool {
        matches!(self.kind, MoveKind::Rapid)
    }
}
