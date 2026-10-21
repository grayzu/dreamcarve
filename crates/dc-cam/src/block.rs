// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Cut blocks (`docs/specs/phase-2/design.md`, "Data model → dc-cam"): what
//! a generator produces before ordering and linking. One block is one
//! contour, open path, drill point or relief notch at one pass depth;
//! [`crate::sequence`] orders the blocks and writes the links between them
//! (`docs/research/toolpath-ordering.md`, decision D10).

use crate::error::ShapeRef;
use crate::toolpath::Move;
use dc_geometry::{Point, Region};

/// One contour, open path, drill point or relief notch at one pass depth,
/// as the generator produced it, before ordering and linking. Coordinates
/// are project space: Y-up, Z-up, millimetres, stock bottom at Z = 0.
///
/// A block's motion is `descent` (from `descent_from` at
/// `descent_from_z_mm` to `entry` at `z_mm`) followed by `cuts`, which
/// end at `exit`. The sequencer supplies the rapid that reaches
/// `descent_from`, or a stay-down link, and never changes a block's own
/// moves. A stay-down link reaches `entry` at `z_mm` directly, so the
/// block's descent is not cut then.
#[derive(Debug, Clone, PartialEq)]
pub struct CutBlock {
    /// The target shape (instance, profile index) this block belongs to.
    /// Blocks sharing a shape form that shape's passes. The order in which
    /// shapes first appear in a generator's block list is their drawing
    /// order, the tie-break for every ordering choice.
    pub shape: ShapeRef,
    /// 0-based pass index, top first. A shape's passes are consecutive
    /// from 0.
    pub pass: usize,
    /// The pass Z, mm, in project space. Every move in `cuts` is at or
    /// above it (tabs rise above it).
    pub z_mm: f64,
    /// XY, mm, where the descent begins: `entry` for a straight plunge,
    /// the ramp or helix start otherwise. The rapid in at safe Z ends here.
    pub descent_from: Point,
    /// The Z, mm, the generator built `descent` to start from: safe Z, or
    /// the previous pass Z of the same shape when the generator expects to
    /// stay down between passes or (a Pocket's ramp or helix) the floor
    /// that pass cleared.
    pub descent_from_z_mm: f64,
    /// Ramp, helix or plunge moves from `descent_from` to `entry` at `z_mm`,
    /// every one `Linear` or an arc at the plunge or feed rate (a ramp or
    /// helix move at the feed rate scaled so it descends no faster than the
    /// plunge rate, S10), except a Drill's single rapid straight down to
    /// its retract height above the stock. Empty means a straight plunge at the plunge rate, which the
    /// sequencer writes.
    pub descent: Vec<Move>,
    /// XY, mm, where the descent ends and cutting begins.
    pub entry: Point,
    /// The cutting moves at `z_mm`, each with its feed. Tabs may raise
    /// moves above `z_mm` within.
    pub cuts: Vec<Move>,
    /// XY, mm, of the last cut.
    pub exit: Point,
    /// True when finishing this block frees material: the final pass of a
    /// contour reaching the stock bottom. A shape that encloses this one
    /// may not reach its own freeing pass before this block is cut.
    pub frees: bool,
    /// The area this block has cleared at `z_mm`, for pockets; `None` for
    /// profiles. Two consecutive blocks with cleared areas at one Z may be
    /// joined by a stay-down link that lies inside their union.
    pub cleared: Option<Region>,
    /// Blocks that must stay in generator order share a group (the raster
    /// lines of one pass, the notches of a corner relief). Blocks of one
    /// shape at one pass are always kept in generator order.
    pub group: usize,
}
