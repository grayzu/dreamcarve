// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/replay.ts (`applyMoveKernel`)
//   src/engine/toolpaths/types.ts (`ToolpathMove`)

//! The cut of one move into the stock (`docs/specs/phase-6/design.md`,
//! "Algorithms → The cut of one move").
//!
//! From upstream: the box of cells a move can reach (its XY box grown by
//! the cutter's radius, clamped to the grid), cells that only ever go
//! down and never below the bottom, and the rectangle of cells that
//! changed. Changed: a cell's new height is the lowest the cutter's
//! surface reaches over it anywhere along the move ([`crate::sweep`]),
//! where upstream reads the tip at the nearest point; a move is an arc or
//! a line, where upstream's moves are chords only; a move can be cut in
//! part, by a span of its length, for playback; a move off the grid or
//! above the stock reads no cell. The arithmetic has no handedness: an
//! arc's turn is the only direction here, and it is Y-up's
//! (anticlockwise seen from +Z).

use std::f64::consts::{PI, TAU};

use dc_geometry::{Arc, Point, Point3};

use crate::cutter::{Cutter, RIM_SLACK_MM};
use crate::field::Heightfield;
use crate::grid::CellRect;
use crate::sweep::{CutterSweep, LineSweep};

/// How far an arc's chords may stray from it, mm, where an arc is cut as
/// chords: one whose Z changes.
pub const CHORD_MM: f64 = 0.001;

/// Under this change in Z, mm, an arc is level and is cut whole.
const LEVEL_MM: f64 = 1e-12;

/// The longest a line's box may run across the grid's axes before the
/// line is cut in pieces, in cutter diameters and in cells: a long
/// diagonal's box is otherwise a square of cells it never touches.
/// Upstream's playback cuts pieces of twice the radius for the same
/// reason.
const PIECE_DIAMETERS: f64 = 1.0;
const PIECE_CELLS: f64 = 8.0;

/// The most pieces one line is cut in.
const MAX_PIECES: f64 = 4096.0;

/// Whether a move removes material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// A cut: a straight feed move or an arc.
    Cut,
    /// A rapid or a dwell: the tool moves, or waits, and nothing is cut.
    Travel,
}

/// Which way an arc turns, seen from +Z with Y up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Clockwise.
    Cw,
    /// Anticlockwise.
    Ccw,
}

/// An arc move's centre and turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimArc {
    /// The centre, mm.
    pub centre: Point,
    /// Which way it turns.
    pub turn: Turn,
}

/// One move as the simulation reads it: the tip's start and end in
/// project space, an arc's centre, whether it cuts, and how long it
/// takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimMove {
    /// Where the tip starts, mm.
    pub from: Point3,
    /// Where it ends, mm.
    pub to: Point3,
    /// The arc it follows in XY, Z changing evenly along it; `None` for a
    /// straight move.
    pub arc: Option<SimArc>,
    /// Whether it cuts.
    pub motion: Motion,
    /// Its time on the timeline, seconds; 0 for a move that takes none.
    pub seconds: f64,
}

impl SimMove {
    /// A straight cut from `from` to `to` that takes `seconds`.
    #[must_use]
    pub const fn cut(from: Point3, to: Point3, seconds: f64) -> Self {
        Self {
            from,
            to,
            arc: None,
            motion: Motion::Cut,
            seconds,
        }
    }

    /// A move from `from` to `to` that cuts nothing and takes `seconds`.
    #[must_use]
    pub const fn travel(from: Point3, to: Point3, seconds: f64) -> Self {
        Self {
            from,
            to,
            arc: None,
            motion: Motion::Travel,
            seconds,
        }
    }

    /// The same move along an arc about `centre`.
    #[must_use]
    pub const fn along(mut self, centre: Point, turn: Turn) -> Self {
        self.arc = Some(SimArc { centre, turn });
        self
    }

    /// Whether it removes material.
    #[must_use]
    pub const fn cuts(&self) -> bool {
        matches!(self.motion, Motion::Cut)
    }

    /// The arc it follows; `None` for a straight move and for an arc that
    /// sweeps nothing (one that ends where it starts, as
    /// [`Arc::from_ends`] reads it).
    fn swept(&self) -> Option<Arc> {
        let arc = self.arc?;
        let swept = Arc::from_ends(
            self.from.xy(),
            self.to.xy(),
            arc.centre,
            arc.turn == Turn::Ccw,
        );
        (swept.sweep_rad.abs() > 0.0 && swept.radius_mm > 0.0).then_some(swept)
    }

    /// Where the tip is `fraction` of the way along the move, from 0 at
    /// its start to 1 at its end.
    #[must_use]
    pub fn point_at(&self, fraction: f64) -> Point3 {
        let t = fraction.clamp(0.0, 1.0);
        let z = self.from.z + (self.to.z - self.from.z) * t;
        match self.swept() {
            Some(arc) => {
                let p = on_arc(&arc, arc.start_angle_rad + arc.sweep_rad * t);
                Point3::new(p.x, p.y, z)
            }
            None => Point3::new(
                self.from.x + (self.to.x - self.from.x) * t,
                self.from.y + (self.to.y - self.from.y) * t,
                z,
            ),
        }
    }

    /// Its length in space, mm: an arc's as a helix.
    #[must_use]
    pub fn length_mm(&self) -> f64 {
        let dz = self.to.z - self.from.z;
        match self.swept() {
            Some(arc) => (arc.radius_mm * arc.sweep_rad).hypot(dz),
            None => (self.to.x - self.from.x)
                .hypot(self.to.y - self.from.y)
                .hypot(dz),
        }
    }
}

/// The point of `arc`'s circle at `angle_rad`.
fn on_arc(arc: &Arc, angle_rad: f64) -> Point {
    Point::new(
        arc.centre.x + arc.radius_mm * angle_rad.cos(),
        arc.centre.y + arc.radius_mm * angle_rad.sin(),
    )
}

/// What a cut did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp {
    /// The cells that changed: the smallest rectangle that holds them
    /// all. `None` when none did.
    pub changed: Option<CellRect>,
    /// How many cells were tested against the cutter: the work done,
    /// which playback and a build's progress count instead of time.
    pub tested: u64,
}

impl Stamp {
    fn grow(&mut self, i: u32, j: u32) {
        let cell = CellRect {
            i0: i,
            i1: i + 1,
            j0: j,
            j1: j + 1,
        };
        self.changed = Some(self.changed.map_or(cell, |r| r.union(cell)));
    }

    /// Both stamps as one.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self {
            changed: match (self.changed, other.changed) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            },
            tested: self.tested + other.tested,
        }
    }
}

impl Heightfield {
    /// Cuts the part of `mv` between two fractions of its length, `span`
    /// (`(0.0, 1.0)` is the whole move), with `cutter`, as run `owner`.
    /// A move that does not cut changes nothing. Every cell the cutter's
    /// surface passes under is lowered to the lowest it reaches there; a
    /// move cut in any number of parts, in any order, leaves what the
    /// whole move leaves.
    pub fn cut(&mut self, mv: &SimMove, span: (f64, f64), cutter: &Cutter, owner: u16) -> Stamp {
        let mut stamp = Stamp::default();
        let (f0, f1) = (span.0.clamp(0.0, 1.0), span.1.clamp(0.0, 1.0));
        // A span that is no number holds nothing, as one that runs
        // backwards does.
        if !mv.cuts() || f0.is_nan() || f1.is_nan() || f0 > f1 {
            return stamp;
        }
        // The span's ends in height; a falling arc's are on its chords,
        // a hair off the arc, which changes no height.
        let z_at = |f: f64| mv.from.z + (mv.to.z - mv.from.z) * f;
        let low_mm = z_at(f0).min(z_at(f1));
        // A height that is no number cuts nothing.
        if low_mm.is_nan() || low_mm >= self.top_z_mm() {
            return stamp;
        }
        let sweep = CutterSweep::new(cutter);
        let reach_mm = self.reach_mm(cutter, low_mm);
        match mv.swept() {
            None => {
                let (a, b) = (mv.point_at(f0), mv.point_at(f1));
                self.cut_line(a, b, &sweep, reach_mm, owner, &mut stamp);
            }
            Some(full) if (mv.to.z - mv.from.z).abs() < LEVEL_MM => {
                let part = Arc {
                    start_angle_rad: full.start_angle_rad + full.sweep_rad * f0,
                    sweep_rad: full.sweep_rad * (f1 - f0),
                    ..full
                };
                let ends = (mv.point_at(f0), mv.point_at(f1));
                self.cut_level_arc(&part, ends, &sweep, reach_mm, owner, &mut stamp);
            }
            Some(full) => {
                // An arc that falls is its chords, the same chords
                // whatever part of it is cut: a part is cut along them,
                // so the parts together are the whole to rounding.
                let n = chords(&full);
                let corner = |k: u32| {
                    if k == 0 {
                        return mv.from;
                    }
                    if k >= n {
                        return mv.to;
                    }
                    let t = f64::from(k) / f64::from(n);
                    let p = on_arc(&full, full.start_angle_rad + full.sweep_rad * t);
                    Point3::new(p.x, p.y, z_at(t))
                };
                let steps = f64::from(n);
                // Whole numbers from 0 to `n`.
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "whole numbers from 0 to a million"
                )]
                let (first, last) = (
                    (f0 * steps).floor().clamp(0.0, steps - 1.0) as u32,
                    (f1 * steps).ceil().clamp(1.0, steps) as u32,
                );
                for k in first..last {
                    let (p, q) = (corner(k), corner(k + 1));
                    // The part of chord `k` inside the span.
                    let (lo, hi) = (
                        (f0 * steps - f64::from(k)).clamp(0.0, 1.0),
                        (f1 * steps - f64::from(k)).clamp(0.0, 1.0),
                    );
                    let on = |t: f64| {
                        Point3::new(
                            p.x + (q.x - p.x) * t,
                            p.y + (q.y - p.y) * t,
                            p.z + (q.z - p.z) * t,
                        )
                    };
                    let a = if lo <= 0.0 { p } else { on(lo) };
                    let b = if hi >= 1.0 { q } else { on(hi) };
                    self.cut_line(a, b, &sweep, reach_mm, owner, &mut stamp);
                }
            }
        }
        stamp
    }

    /// How far from the tip's path a cell can be and still be cut, mm:
    /// the radius, or for a V-bit the radius its cone has at the stock's
    /// top when that is less.
    fn reach_mm(&self, cutter: &Cutter, low_mm: f64) -> f64 {
        let radius_mm = cutter.radius_mm() + RIM_SLACK_MM;
        match *cutter {
            Cutter::Vee { slope, .. } => radius_mm.min((self.top_z_mm() - low_mm) * slope),
            Cutter::Flat { .. } | Cutter::Ball { .. } | Cutter::TaperedBall { .. } => radius_mm,
        }
    }

    /// Cuts the straight move `a → b`, in pieces when its box would
    /// cover far more cells than it passes over.
    fn cut_line(
        &mut self,
        a: Point3,
        b: Point3,
        sweep: &CutterSweep,
        reach_mm: f64,
        owner: u16,
        stamp: &mut Stamp,
    ) {
        let limit_mm = (2.0 * reach_mm * PIECE_DIAMETERS).max(PIECE_CELLS * self.grid().cell_mm());
        let across_mm = (b.x - a.x).abs().min((b.y - a.y).abs());
        let pieces = (across_mm / limit_mm).ceil().clamp(1.0, MAX_PIECES);
        if pieces <= 1.0 {
            self.cut_piece(a, b, sweep, reach_mm, owner, stamp);
            return;
        }
        // A whole number from 2 to `MAX_PIECES`.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a whole number from 2 to 4096"
        )]
        let n = pieces as u32;
        let at = |k: u32| {
            let t = f64::from(k) / f64::from(n);
            Point3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            )
        };
        for k in 0..n {
            let (p, q) = (
                if k == 0 { a } else { at(k) },
                if k + 1 == n { b } else { at(k + 1) },
            );
            self.cut_piece(p, q, sweep, reach_mm, owner, stamp);
        }
    }

    /// Cuts the straight move `a → b` into every cell of its box.
    fn cut_piece(
        &mut self,
        a: Point3,
        b: Point3,
        sweep: &CutterSweep,
        reach_mm: f64,
        owner: u16,
        stamp: &mut Stamp,
    ) {
        let (top_mm, low_mm) = (self.top_z_mm(), a.z.min(b.z));
        if low_mm.is_nan() || low_mm >= top_mm {
            return;
        }
        let grid = *self.grid();
        let (i0, i1) = grid.span(a.x.min(b.x) - reach_mm, a.x.max(b.x) + reach_mm, grid.nx());
        let (j0, j1) = grid.span(a.y.min(b.y) - reach_mm, a.y.max(b.y) + reach_mm, grid.ny());
        let line = LineSweep::new(a, b);
        for j in j0..j1 {
            for i in i0..i1 {
                stamp.tested += 1;
                let z_mm = sweep.low_mm(&line, grid.centre(i, j));
                if z_mm < top_mm
                    && let Some(index) = grid.index(i, j)
                    && self.lower(index, z_mm, owner)
                {
                    stamp.grow(i, j);
                }
            }
        }
    }

    /// Cuts a level arc whole: a cell's distance from the arc is its
    /// distance from the circle where the cell is abreast of the arc,
    /// and from the nearer end elsewhere.
    fn cut_level_arc(
        &mut self,
        arc: &Arc,
        (a, b): (Point3, Point3),
        sweep: &CutterSweep,
        reach_mm: f64,
        owner: u16,
        stamp: &mut Stamp,
    ) {
        let top_mm = self.top_z_mm();
        let grid = *self.grid();
        let (mut lo, mut hi) = (
            Point::new(a.x.min(b.x), a.y.min(b.y)),
            Point::new(a.x.max(b.x), a.y.max(b.y)),
        );
        for p in arc.axis_extremes() {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let (i0, i1) = grid.span(lo.x - reach_mm, hi.x + reach_mm, grid.nx());
        let (j0, j1) = grid.span(lo.y - reach_mm, hi.y + reach_mm, grid.ny());
        for j in j0..j1 {
            for i in i0..i1 {
                stamp.tested += 1;
                let q = grid.centre(i, j);
                let z_mm = a.z + sweep.height_mm(arc_distance_mm(arc, (a.xy(), b.xy()), q));
                if z_mm < top_mm
                    && let Some(index) = grid.index(i, j)
                    && self.lower(index, z_mm, owner)
                {
                    stamp.grow(i, j);
                }
            }
        }
    }
}

/// How many chords `arc` is cut in so that none strays from it by more
/// than [`CHORD_MM`].
fn chords(arc: &Arc) -> u32 {
    let per_chord_rad = if arc.radius_mm > CHORD_MM {
        2.0 * (1.0 - CHORD_MM / arc.radius_mm).acos()
    } else {
        PI
    };
    let n = (arc.sweep_rad.abs() / per_chord_rad).ceil().clamp(1.0, 1e6);
    // A whole number from 1 to a million.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number from 1 to 1e6"
    )]
    {
        n as u32
    }
}

/// The distance from `q` to `arc`, whose ends are `ends`, mm.
fn arc_distance_mm(arc: &Arc, (start, end): (Point, Point), q: Point) -> f64 {
    let angle_rad = (q.y - arc.centre.y).atan2(q.x - arc.centre.x);
    let from_start_rad = if arc.sweep_rad > 0.0 {
        (angle_rad - arc.start_angle_rad).rem_euclid(TAU)
    } else {
        (arc.start_angle_rad - angle_rad).rem_euclid(TAU)
    };
    let to_ends_mm = q.distance_to(start).min(q.distance_to(end));
    if from_start_rad <= arc.sweep_rad.abs() {
        // Abreast of the arc: its circle is nearest, and never further
        // than an end.
        (q.distance_to(arc.centre) - arc.radius_mm)
            .abs()
            .min(to_ends_mm)
    } else {
        to_ends_mm
    }
}

#[cfg(test)]
mod tests;
