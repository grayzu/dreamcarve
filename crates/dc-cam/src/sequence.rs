// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/multiFeature.ts
//   src/engine/toolpaths/geometry.ts (greedyNearestNeighbor)
//   src/engine/toolpaths/pocket.ts (transitionToCutEntry)

//! Sequencing (`docs/specs/phase-2/design.md`, "Algorithms → Sequence";
//! `docs/research/toolpath-ordering.md` §5.5, decisions D5 and D10): the
//! one step every generator hands its [`CutBlock`]s to. It orders the
//! blocks by machining order, containment precedence and nearest neighbour,
//! writes the links between them, and produces the [`Toolpath`].
//!
//! Nothing here depends on Y-up or Y-down: distances and containment are
//! symmetric under the flip. What was re-derived rather than copied is the
//! start of the walk (the first shape in drawing order, D5, where upstream
//! starts a drilling walk at the origin) and the containment rule, which
//! upstream does not have.

use crate::block::CutBlock;
use crate::entry::{CLEARED_POINT_TOLERANCE_MM, ENTRY_CLEARANCE_MM};
use crate::error::{BlockProblem, CamError, ShapeRef, at, at_mut};
use crate::toolpath::{Move, MoveKind, Toolpath};
use dc_geometry::{
    Arc, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3, Region, Segment,
    point_segment_distance_mm, union,
};
use dc_project::MachiningOrder;

/// Two nearest-first candidates whose distances differ by no more than
/// this, mm, are equally near, and the one first in drawing order is
/// taken (#215). It is the finest step a post-processor writes in mm
/// (0.001), so candidates closer than it cannot be told apart in a
/// program, and it is more than the noise an `f32` import adds to a
/// distance between points within about 2 m of the origin (`2⁻²⁴`
/// relative, twice over), so an SVG and a DXF of one drawing give one
/// order.
pub(crate) const NEAREST_TIE_MM: f64 = 1e-3;

/// Nearest first (#215): of `candidates`, each with its distance in mm,
/// the first in the order given whose distance is within
/// [`NEAREST_TIE_MM`] of the nearest one's. So a candidate is taken
/// before an earlier one only when it is nearer by more than the
/// tolerance, and the order given (drawing order) settles every tie, not
/// float noise. `None` when there are none; the first when no distance is
/// a number.
pub(crate) fn nearest_of<T>(candidates: impl IntoIterator<Item = (T, f64)>) -> Option<T> {
    let candidates: Vec<(T, f64)> = candidates.into_iter().collect();
    let pick = nearest_index(&candidates);
    candidates.into_iter().nth(pick).map(|(t, _)| t)
}

/// All of `candidates` nearest first, each next one chosen by
/// [`nearest_of`]'s rule from those left: so distances within
/// [`NEAREST_TIE_MM`] keep the order given (#392: a raster Pocket's
/// stay-down links, where the lines one step-over above and below are a
/// tie that float noise in the scan lines' Y used to settle).
pub(crate) fn nearest_first_order<T>(candidates: Vec<(T, f64)>) -> Vec<T> {
    let mut left = candidates;
    let mut out = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let pick = nearest_index(&left);
        out.push(left.remove(pick).0);
    }
    out
}

/// The index [`nearest_of`] takes: the first whose distance is within
/// [`NEAREST_TIE_MM`] of the nearest; 0 when none is a number.
fn nearest_index<T>(candidates: &[(T, f64)]) -> usize {
    let nearest_mm = candidates
        .iter()
        .map(|(_, d)| *d)
        .fold(f64::INFINITY, f64::min);
    candidates
        .iter()
        .position(|(_, d)| *d <= nearest_mm + NEAREST_TIE_MM)
        .unwrap_or(0)
}

/// Everything [`sequence`] needs, in project space (mm, Y-up, Z-up).
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceInput<'a> {
    /// The blocks to order, in generator order. The order in which shapes
    /// first appear here is their drawing order.
    pub blocks: Vec<CutBlock>,
    /// Full depth first or pass first.
    pub order: MachiningOrder,
    /// `(inner, encloser)` pairs: `inner` is cut before `encloser` reaches
    /// its freeing pass, and under full depth first before `encloser`
    /// starts at all. Pairs naming shapes with no blocks are ignored.
    pub containment: &'a [(ShapeRef, ShapeRef)],
    /// Absolute safe Z, mm: every rapid that moves in XY between blocks of
    /// one instance (passes, holes, notches) runs here.
    pub safe_z_mm: f64,
    /// Absolute clearance Z, mm (#283; design addendum A7): a rapid that
    /// moves in XY between blocks of different instances runs here. The
    /// project's `heights.clearance_mm` above the stock top, or
    /// `safe_z_mm` when that is unset; never below `safe_z_mm`.
    pub clearance_z_mm: f64,
    /// The stock top, mm: a rapid straight down to a descent may stop
    /// [`ENTRY_CLEARANCE_MM`] above it anywhere (S2, #214).
    pub stock_top_mm: f64,
    /// The tool's diameter, mm.
    pub tool_diameter_mm: f64,
    /// The operation's step-over, mm; 0 for a profile. A stay-down link
    /// may be at most `tool_diameter_mm + stepover_mm` long.
    pub stepover_mm: f64,
    /// The tool's cutting feed, mm/min, for stay-down links.
    pub feed_mm_min: f64,
    /// The tool's plunge rate, mm/min, for the straight plunges of blocks
    /// with an empty descent.
    pub plunge_mm_min: f64,
    /// The operation's moves already written before these blocks (a
    /// Pocket's main path before its relief pass), empty otherwise: a
    /// rapid straight down may stop [`ENTRY_CLEARANCE_MM`] above the
    /// floors they cut, as above the toolpath's own (S2, #227).
    pub cut_before: &'a [Move],
    /// Photo V-Carve only (format 27, `docs/specs/phase-3/design.md`,
    /// "Photo V-Carve", step 4): `Some(z_mm)` joins each block to the
    /// next block of the same shape and group by a groove link at that
    /// absolute Z: a `Linear` rise at the feed rate to it, a `Linear`
    /// move across at it at the feed rate, and the next block's descent
    /// (its straight plunge at the plunge rate) from there, never a
    /// retract to safe Z. The caller vouches that the Z is in the air
    /// over every block (security review L1). `None` for every other
    /// generator, whose output it leaves as it was. It is not the
    /// stay-down link through cleared area that [`sequence`](fn@sequence)
    /// writes between two Pocket blocks, nor a raster Pocket's stay-down
    /// links (`pocket::raster`): both run at the cut's own depth.
    pub groove_link_z_mm: Option<f64>,
}

/// The clearance Z, mm, absolute, for [`SequenceInput::clearance_z_mm`]
/// (#283; design addendum A7): the project's `heights.clearance_mm` above
/// the stock top at `thickness_mm`, or `safe_z_mm` when it is unset. Never
/// below `safe_z_mm`: validation refuses a clearance below an enabled
/// operation's safe Z, and a link between parts is never lower than one
/// between passes whatever a caller passes.
#[must_use]
pub fn clearance_z_mm(project: &dc_project::Project, thickness_mm: f64, safe_z_mm: f64) -> f64 {
    project
        .heights
        .clearance_mm
        .filter(|c| c.is_finite())
        .map_or(safe_z_mm, |c| (thickness_mm + c).max(safe_z_mm))
}

/// Longest gap between sample points when checking that a stay-down link
/// lies inside cleared material, mm. Links are at most a tool diameter plus
/// a step-over long, so this is a handful of points.
const LINK_SAMPLE_STEP_MM: f64 = 0.5;

/// The order the blocks of `input` are cut in, as indices into
/// `input.blocks` (spec steps 1–3): every unit (one shape at one pass) is
/// a run of consecutive indices in generator order, except that the
/// blocks that free material come last in it, so nothing of a shape is cut
/// after its freeing pass (#189); units are walked by
/// `input.order` with containment precedence and nearest neighbour, ties by
/// drawing order. [`sequence`] emits exactly this order.
///
/// Fails with [`CamError::InvalidBlock`] when a block's cuts go below its
/// pass Z, a shape's passes are not consecutive from 0, a plunging block
/// starts away from its entry, its entry Z is below its pass Z, or its
/// descent ends away from its entry at its entry Z, and with
/// [`CamError::ContainmentCycle`] when the containment pairs leave no shape
/// cuttable.
pub fn plan(input: &SequenceInput<'_>) -> Result<Vec<usize>, CamError> {
    let shapes = Shapes::new(input)?;
    shapes.walk(input.order)
}

/// Orders and links the blocks of `input` into one toolpath (spec steps
/// 4–5): the order is [`plan`]'s; between consecutive blocks `a → b` the
/// link is nothing when `b` is the next pass of `a`'s shape and its descent
/// starts where `a` ended; a stay-down `Linear` at feed when both blocks
/// have `cleared` areas at one Z and the link is no longer than the
/// flatten tolerance (#213), or the straight link lies inside their union
/// and is at most `tool_diameter_mm + stepover_mm` long, which runs to
/// `b`'s entry and skips `b`'s descent (the tool is already at `b`'s entry
/// Z in cleared material); otherwise a rapid up to safe Z, a rapid in XY,
/// and the descent, or, between blocks of different instances, the same at
/// the clearance Z (#283). A block's empty descent is a straight plunge at
/// the plunge rate to its entry Z, never its pass Z (security review H1);
/// its own descent moves are written as given, from `descent_from_z_mm`: below safe Z the tool
/// gets there by a rapid straight down that stops in the air (S2 as
/// changed in #214) and a plunge for the rest. Zero-length moves are never
/// written. The first block is reached by a rapid at safe Z; the last is
/// followed by a rapid up to safe Z.
/// With [`SequenceInput::groove_link_z_mm`] set (a Photo V-Carve, #646),
/// consecutive blocks of one shape and group are joined instead by a
/// groove link: a rise at feed to that Z, a move across there at feed,
/// and the next block's plunge.
///
/// Fails as [`plan`] does.
#[must_use = "the toolpath is the whole point"]
pub fn sequence(input: &SequenceInput<'_>) -> Result<Toolpath, CamError> {
    let order = plan(input)?;
    let mut emitter = Emitter {
        input,
        moves: Vec::new(),
        current: None,
        previous: None,
    };
    for index in order {
        emitter.emit(at(
            &input.blocks,
            index,
            "the cut order names a block the sequencer was not given",
        )?);
    }
    emitter.finish();
    Ok(Toolpath {
        moves: emitter.moves,
    })
}

/// One shape's blocks at one pass, in generator order with the freeing
/// blocks last.
#[derive(Debug)]
struct Unit {
    /// Indices into `SequenceInput::blocks`.
    blocks: Vec<usize>,
    /// Where the unit's first descent begins.
    start: Point,
    /// Where the unit's last block ends.
    exit: Point,
    /// Some block of the unit frees material.
    frees: bool,
}

impl Unit {
    /// The unit of `blocks`, each with its index in
    /// `SequenceInput::blocks`, in the order they are cut. A unit has a
    /// block: [`CamError::Internal`] for none, which has no start to
    /// travel to.
    fn new(blocks: &[(usize, &CutBlock)]) -> Result<Self, CamError> {
        let (Some((_, first)), Some((_, last))) = (blocks.first(), blocks.last()) else {
            return Err(CamError::internal("a pass of a shape has no block to cut"));
        };
        Ok(Self {
            blocks: blocks.iter().map(|(index, _)| *index).collect(),
            start: first.descent_from,
            exit: last.exit,
            frees: blocks.iter().any(|(_, b)| b.frees),
        })
    }
}

/// One shape as the walk sees it.
#[derive(Debug)]
struct Shape {
    reference: ShapeRef,
    /// The shape's units, pass 0 first.
    units: Vec<Unit>,
    /// Indices of the shapes this one encloses.
    enclosed: Vec<usize>,
}

/// The validated shapes of an input, in drawing order.
struct Shapes {
    shapes: Vec<Shape>,
}

impl Shapes {
    /// Validates the blocks (spec step 1) and groups them into shapes and
    /// units (step 2).
    fn new(input: &SequenceInput<'_>) -> Result<Self, CamError> {
        // Each shape with its blocks and their indices, in generator
        // order; the shapes in drawing order.
        let mut grouped: Vec<(Shape, Vec<(usize, &CutBlock)>)> = Vec::new();
        for (index, block) in input.blocks.iter().enumerate() {
            validate_block(block)?;
            match grouped
                .iter_mut()
                .find(|(shape, _)| shape.reference == block.shape)
            {
                Some((_, member)) => member.push((index, block)),
                None => grouped.push((
                    Shape {
                        reference: block.shape.clone(),
                        units: Vec::new(),
                        enclosed: Vec::new(),
                    },
                    vec![(index, block)],
                )),
            }
        }

        let mut shapes: Vec<Shape> = Vec::with_capacity(grouped.len());
        for (mut shape, member) in grouped {
            let mut passes: Vec<usize> = member.iter().map(|(_, b)| b.pass).collect();
            passes.sort_unstable();
            passes.dedup();
            if passes.iter().enumerate().any(|(k, &pass)| k != pass) {
                let (_, first) = at(&member, 0, "a shape the sequencer grouped has no block")?;
                return Err(CamError::InvalidBlock {
                    shape: shape.reference.clone(),
                    pass: first.pass,
                    problem: BlockProblem::PassesNotConsecutive { passes },
                });
            }
            for pass in passes {
                let mut blocks: Vec<(usize, &CutBlock)> = member
                    .iter()
                    .copied()
                    .filter(|(_, b)| b.pass == pass)
                    .collect();
                // #189: nothing of a shape is cut after its freeing pass,
                // so the unit's freeing blocks go last (stable: generator
                // order otherwise), after a Profile's relief notches.
                blocks.sort_by_key(|(_, b)| b.frees);
                shape.units.push(Unit::new(&blocks)?);
            }
            shapes.push(shape);
        }

        // Pairs naming a shape with no blocks are ignored
        // (`SequenceInput::containment`).
        for (inner, encloser) in input.containment {
            let Some(inner) = shapes.iter().position(|s| &s.reference == inner) else {
                continue;
            };
            if let Some(shape) = shapes.iter_mut().find(|s| &s.reference == encloser)
                && !shape.enclosed.contains(&inner)
            {
                shape.enclosed.push(inner);
            }
        }
        Ok(Self { shapes })
    }

    /// The greedy walk (spec step 3): from the current exit, the nearest
    /// available unit or stack ([`nearest_of`]: distances within
    /// [`NEAREST_TIE_MM`] are a tie), ties by drawing order.
    /// `done[s]` counts the passes of shape `s` already emitted.
    fn walk(&self, order: MachiningOrder) -> Result<Vec<usize>, CamError> {
        let total: usize = self
            .shapes
            .iter()
            .flat_map(|s| &s.units)
            .map(|u| u.blocks.len())
            .sum();
        let mut out = Vec::with_capacity(total);
        let mut done = vec![0usize; self.shapes.len()];
        let Some(first) = self.shapes.first().and_then(|s| s.units.first()) else {
            return Ok(out);
        };
        let mut current = first.start;
        while out.len() < total {
            let next = match order {
                MachiningOrder::FullDepthFirst => self.nearest_stack(&done, current)?,
                MachiningOrder::PassFirst => self.nearest_unit(&done, current)?,
            };
            let Some(index) = next else {
                // Blocks remain, so some shape is unfinished; it is on
                // the cycle or waits on one.
                let stuck = self
                    .shapes
                    .iter()
                    .zip(&done)
                    .find(|(s, d)| **d < s.units.len())
                    .map(|(s, _)| s.reference.clone())
                    .ok_or_else(|| {
                        CamError::internal("blocks remain to order, but every shape is finished")
                    })?;
                return Err(CamError::ContainmentCycle { shape: stuck });
            };
            let shape = at(
                &self.shapes,
                index,
                "the walk chose a shape it does not have",
            )?;
            let passes_done = at_mut(&mut done, index, "the walk chose a shape it does not count")?;
            let from = *passes_done;
            let emitted = match order {
                MachiningOrder::FullDepthFirst => shape.units.len(),
                MachiningOrder::PassFirst => from + 1,
            };
            for unit in shape.units.iter().take(emitted).skip(from) {
                out.extend(unit.blocks.iter().copied());
                current = unit.exit;
            }
            *passes_done = emitted;
        }
        Ok(out)
    }

    /// Full depth first: the nearest unstarted shape whose enclosed shapes
    /// are all finished. `done` counts each shape's passes emitted, in
    /// the shapes' order.
    fn nearest_stack(&self, done: &[usize], current: Point) -> Result<Option<usize>, CamError> {
        let mut candidates = Vec::new();
        for (s, (shape, &passes_done)) in self.shapes.iter().zip(done).enumerate() {
            if passes_done != 0 {
                continue;
            }
            let mut free = true;
            for &e in &shape.enclosed {
                if !self.finished(done, e)? {
                    free = false;
                    break;
                }
            }
            if free && let Some(unit) = shape.units.first() {
                candidates.push((s, current.distance_to(unit.start)));
            }
        }
        Ok(nearest_of(candidates))
    }

    /// Pass first: among the available next units, the shallowest pass,
    /// then the nearest. A unit at pass `k` is available when every
    /// enclosed shape has finished pass `k` (or has no pass `k` and is
    /// finished) and, when the unit frees, has finished entirely.
    fn nearest_unit(&self, done: &[usize], current: Point) -> Result<Option<usize>, CamError> {
        let mut available: Vec<(usize, usize, Point)> = Vec::new();
        for (s, (shape, &k)) in self.shapes.iter().zip(done).enumerate() {
            // A shape with no pass `k` is finished.
            let Some(unit) = shape.units.get(k) else {
                continue;
            };
            let mut free = true;
            for &e in &shape.enclosed {
                let (enclosed, done_e) = self.progress(done, e)?;
                let required = enclosed.units.len().min(k + 1);
                if done_e < required || (unit.frees && done_e < enclosed.units.len()) {
                    free = false;
                    break;
                }
            }
            if free {
                available.push((s, k, unit.start));
            }
        }
        let Some(shallowest) = available.iter().map(|(_, k, _)| *k).min() else {
            return Ok(None);
        };
        Ok(nearest_of(
            available
                .into_iter()
                .filter(|(_, k, _)| *k == shallowest)
                .map(|(s, _, start)| (s, current.distance_to(start))),
        ))
    }

    /// Shape `shape` and how many of its passes `done` counts as emitted.
    /// A shape's `enclosed` holds indices of `self.shapes`, which `done`
    /// parallels, so both are there: [`CamError::Internal`] otherwise,
    /// where taking the shape as finished would cut its encloser free
    /// before it.
    fn progress(&self, done: &[usize], shape: usize) -> Result<(&Shape, usize), CamError> {
        Ok((
            at(
                &self.shapes,
                shape,
                "a shape encloses one the sequencer does not have",
            )?,
            *at(
                done,
                shape,
                "a shape encloses one the sequencer does not count",
            )?,
        ))
    }

    fn finished(&self, done: &[usize], shape: usize) -> Result<bool, CamError> {
        let (shape, passes_done) = self.progress(done, shape)?;
        Ok(passes_done >= shape.units.len())
    }
}

/// Spec step 1: cuts and descent at or above the pass Z; a plunging block
/// starts at its entry; the entry Z at or above the pass Z, and the first
/// cut starting at the entry at the entry Z (security review H1).
fn validate_block(block: &CutBlock) -> Result<(), CamError> {
    let lowest = block
        .cuts
        .iter()
        .chain(&block.descent)
        .map(|m| m.to.z)
        .fold(f64::INFINITY, f64::min);
    if lowest < block.z_mm - EPS_MM {
        return Err(CamError::InvalidBlock {
            shape: block.shape.clone(),
            pass: block.pass,
            problem: BlockProblem::MoveBelowPass {
                z_mm: block.z_mm,
                lowest_mm: lowest,
            },
        });
    }
    if block.descent.is_empty() && block.descent_from.distance_to(block.entry) > EPS_MM {
        return Err(CamError::InvalidBlock {
            shape: block.shape.clone(),
            pass: block.pass,
            problem: BlockProblem::PlungeAwayFromEntry,
        });
    }
    if block.entry_z_mm < block.z_mm - EPS_MM {
        return Err(CamError::InvalidBlock {
            shape: block.shape.clone(),
            pass: block.pass,
            problem: BlockProblem::EntryBelowPass {
                z_mm: block.z_mm,
                entry_z_mm: block.entry_z_mm,
            },
        });
    }
    // An empty descent is the sequencer's plunge, which ends at the entry
    // Z by construction; a generator's own descent must end there.
    if let Some(last) = block.descent.last()
        && (last.to.xy().distance_to(block.entry) > EPS_MM
            || (last.to.z - block.entry_z_mm).abs() > EPS_MM)
    {
        return Err(CamError::InvalidBlock {
            shape: block.shape.clone(),
            pass: block.pass,
            problem: BlockProblem::FirstCutAwayFromEntry { start: last.to },
        });
    }
    Ok(())
}

/// How the tool gets from one block to the next (spec step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    /// The next block's descent starts where the tool is.
    None,
    /// The next block, of the same shape, opens with a straight plunge
    /// where the tool is, in the column the previous block cut at least
    /// that deep (#240): the plunge goes from where the tool is.
    InColumn,
    /// A straight cut at feed through cleared material at the current Z.
    StayDown,
    /// Up to safe Z (the clearance Z to another instance), across, and
    /// down.
    Retract,
}

/// Writes the linked blocks.
struct Emitter<'a, 'b> {
    input: &'a SequenceInput<'b>,
    moves: Vec<Move>,
    /// Where the tool is, once the first block has started.
    current: Option<Point3>,
    previous: Option<&'a CutBlock>,
}

impl<'a> Emitter<'a, '_> {
    fn emit(&mut self, block: &'a CutBlock) {
        let safe_z = self.input.safe_z_mm;
        // A Photo V-Carve's grooves of one family: up to the link height,
        // across and down, all fed (S19).
        if let (Some(link_z), Some(previous), Some(current)) =
            (self.input.groove_link_z_mm, self.previous, self.current)
            && previous.shape == block.shape
            && previous.group == block.group
        {
            let feed = self.input.feed_mm_min;
            self.push(Move::linear(
                Point3::new(current.x, current.y, link_z),
                feed,
            ));
            self.push(Move::linear(
                Point3::new(block.descent_from.x, block.descent_from.y, link_z),
                feed,
            ));
            self.descend(block);
            self.cut(block);
            return;
        }
        match (self.previous, self.current) {
            (Some(previous), Some(current)) => match self.link(previous, block, current) {
                Link::None => {}
                Link::InColumn => {
                    // Straight up or down at the descent's start, then the
                    // rest of the descent as given.
                    if block.descent.is_empty() {
                        let entry = Point3::new(block.entry.x, block.entry.y, block.entry_z_mm);
                        self.push(Move::linear(entry, self.input.plunge_mm_min));
                    }
                    for mv in &block.descent {
                        self.push(*mv);
                    }
                    self.cut(block);
                    return;
                }
                Link::StayDown => {
                    // Already at the block's entry Z in cleared material:
                    // straight to its entry, and its descent is not cut.
                    let to = Point3::new(block.entry.x, block.entry.y, current.z);
                    self.push(Move::linear(to, self.input.feed_mm_min));
                    self.cut(block);
                    return;
                }
                Link::Retract => {
                    // #283: between parts at the clearance height, between
                    // passes and holes of one part at safe Z.
                    let link_z = if previous.shape.instance == block.shape.instance {
                        safe_z
                    } else {
                        self.input.clearance_z_mm.max(safe_z)
                    };
                    self.push(Move::rapid(Point3::new(current.x, current.y, link_z)));
                    self.push(Move::rapid(Point3::new(
                        block.descent_from.x,
                        block.descent_from.y,
                        link_z,
                    )));
                }
            },
            _ => {
                self.push(Move::rapid(Point3::new(
                    block.descent_from.x,
                    block.descent_from.y,
                    safe_z,
                )));
            }
        }
        self.descend(block);
        self.cut(block);
    }

    /// Writes `block`'s cuts and makes it the previous block.
    fn cut(&mut self, block: &'a CutBlock) {
        for cut in &block.cuts {
            self.push(*cut);
        }
        self.previous = Some(block);
    }

    /// Spec step 4, rules 1–4, for `a → b` with the tool at `current`.
    fn link(&self, a: &CutBlock, b: &CutBlock, current: Point3) -> Link {
        let at_start = current.xy().distance_to(b.descent_from) <= EPS_MM;
        if a.shape == b.shape && b.pass == a.pass + 1 {
            return if at_start { Link::None } else { Link::Retract };
        }
        if a.shape == b.shape && at_start && cut_down_to_plunge(a, b) {
            return Link::InColumn;
        }
        if let (Some(cleared_a), Some(cleared_b)) = (&a.cleared, &b.cleared)
            && (a.z_mm - b.z_mm).abs() <= EPS_MM
            && (current.z - b.entry_z_mm).abs() <= EPS_MM
        {
            let length_mm = current.xy().distance_to(b.entry);
            let limit_mm = self.input.tool_diameter_mm + self.input.stepover_mm;
            // #213: a link no longer than the flattening tolerance is a
            // stay-down: its end lies on the cleared area as drawn, if not
            // on its flattened chords (a raster line's end, clipped on
            // those chords, and the wall ring's start on the arc).
            if length_mm <= FLATTEN_TOLERANCE_DEFAULT_MM
                || (length_mm <= limit_mm + EPS_MM
                    && link_inside_cleared(cleared_a, cleared_b, current.xy(), b.entry))
            {
                return Link::StayDown;
            }
        }
        Link::Retract
    }

    /// The descent into `block` from wherever the tool is: a straight
    /// plunge at the plunge rate for an empty descent, else the block's
    /// own moves from `descent_from_z_mm`. The tool, at safe Z above
    /// `descent_from`, gets there by a rapid straight down as far as S2
    /// lets it stop in the air ([`Self::rapid_floor_mm`]), then a plunge at
    /// the plunge rate for the rest (from lower, the plunge alone); below
    /// it (the next pass of a shape
    /// whose descent starts above the floor just cut), by a move up at the
    /// plunge rate.
    fn descend(&mut self, block: &CutBlock) {
        if block.descent.is_empty() {
            // #227: a straight plunge is reached by the same rapid down.
            // H1: it ends where cutting begins, not at the block's deepest.
            let entry = Point3::new(block.entry.x, block.entry.y, block.entry_z_mm);
            self.rapid_down(entry);
            self.push(Move::linear(entry, self.input.plunge_mm_min));
            return;
        }
        let at = block.descent_from;
        let start = Point3::new(at.x, at.y, block.descent_from_z_mm);
        // #227: a descent that opens with a straight plunge at its start
        // (onto a tab, or a corner-relief notch) is reached by the rapid
        // down as far as the plunge goes.
        let plunge_to = block
            .descent
            .first()
            .filter(|mv| {
                mv.kind == MoveKind::Linear
                    && mv.to.xy().distance_to(at) <= EPS_MM
                    && mv.to.z < start.z - EPS_MM
            })
            .map(|mv| mv.to);
        if let Some(current) = self.current {
            if current.z < start.z - EPS_MM {
                self.push(Move::linear(start, self.input.plunge_mm_min));
            } else if let Some(plunge_to) = plunge_to {
                // The plunge goes on straight down from where the rapid
                // stops, at or above its end.
                self.rapid_down(plunge_to);
            } else if current.z > start.z + EPS_MM {
                self.rapid_down(start);
                self.push(Move::linear(start, self.input.plunge_mm_min));
            }
        }
        for mv in &block.descent {
            self.push(*mv);
        }
    }

    /// From safe Z only (S2), a rapid straight down towards `to` that stops
    /// in the air: at [`Self::rapid_floor_mm`] over `to`, never below `to`.
    /// Nothing when the tool is below safe Z or already that low.
    fn rapid_down(&mut self, to: Point3) {
        let Some(current) = self.current else {
            return;
        };
        if current.z < self.input.safe_z_mm - EPS_MM {
            return;
        }
        let rapid_z = self.rapid_floor_mm(to.xy()).max(to.z);
        if rapid_z < current.z - EPS_MM {
            self.push(Move::rapid(Point3::new(to.x, to.y, rapid_z)));
        }
    }

    /// The lowest Z, mm, a rapid straight down at `at` may stop at (S2 as
    /// changed in #214): [`ENTRY_CLEARANCE_MM`] above the stock top, or
    /// that far above the lowest Z a cut of the operation written so far
    /// ([`SequenceInput::cut_before`], then this toolpath) passed through
    /// `at` (within [`CLEARED_POINT_TOLERANCE_MM`]), whichever is lower.
    /// A cut that falls as it runs counts at its highest Z within
    /// [`CLEARED_POINT_TOLERANCE_MM`] of where it comes nearest `at`
    /// ([`highest_cut_near`]): the safety pass finds that nearest point on
    /// the cut as written, whose rounded ends and `at` move it along the
    /// cut by up to a few units in the last place, and on a V-bit's steep
    /// descent its Z with it (#464).
    fn rapid_floor_mm(&self, at: Point) -> f64 {
        let above_stock = self.input.stock_top_mm + ENTRY_CLEARANCE_MM;
        let lowest = |moves: &[Move]| highest_cut_near(moves, at, CLEARED_POINT_TOLERANCE_MM);
        [lowest(self.input.cut_before), lowest(&self.moves)]
            .into_iter()
            .flatten()
            .fold(above_stock, |floor, z| floor.min(z + ENTRY_CLEARANCE_MM))
    }

    /// Appends `mv` unless it ends where the tool already is.
    fn push(&mut self, mv: Move) {
        if let Some(current) = self.current
            && !mv.is_dwell()
            && distance_3d(current, mv.to) <= EPS_MM
        {
            return;
        }
        self.moves.push(mv);
        self.current = Some(mv.to);
    }

    /// The rapid up to safe Z after the last block.
    fn finish(&mut self) {
        if let Some(current) = self.current
            && current.z < self.input.safe_z_mm - EPS_MM
        {
            self.push(Move::rapid(Point3::new(
                current.x,
                current.y,
                self.input.safe_z_mm,
            )));
        }
    }
}

/// Link rule 1 for a block of the same shape (#240): true when `b`'s
/// descent opens with a straight plunge at `descent_from` (an empty
/// descent, or a first move straight up or down there) and `a`'s cuts
/// passed through `descent_from` (within [`CLEARED_POINT_TOLERANCE_MM`]) at
/// or below the Z that plunge ends at. With the tool at `descent_from`
/// after `a`, the plunge then runs in the column `a` has just opened, so
/// it is cut from where the tool is (a Profile's relief notch after a lap
/// that ends on its descend point). Only same-shape blocks: links to
/// another shape keep O3's retract.
fn cut_down_to_plunge(a: &CutBlock, b: &CutBlock) -> bool {
    let plunge_to_z_mm = match b.descent.first() {
        None => b.entry_z_mm,
        Some(mv)
            if mv.kind == MoveKind::Linear && mv.to.xy().distance_to(b.descent_from) <= EPS_MM =>
        {
            mv.to.z
        }
        Some(_) => return false,
    };
    let start = a
        .descent
        .last()
        .map_or(Point3::new(a.entry.x, a.entry.y, a.entry_z_mm), |m| m.to);
    let moves: Vec<Move> = std::iter::once(Move::rapid(start))
        .chain(a.cuts.iter().copied())
        .collect();
    lowest_cut_through(&moves, b.descent_from, CLEARED_POINT_TOLERANCE_MM)
        .is_some_and(|z| z <= plunge_to_z_mm + EPS_MM)
}

/// The lowest Z, mm, at which a cutting move of `moves` passed within
/// `tolerance_mm` of `at` in XY: a straight move's Z where it comes
/// nearest `at`, an arc's (flat, at its end Z) its Z. `None` when no cut
/// passed there.
pub(crate) fn lowest_cut_through(moves: &[Move], at: Point, tolerance_mm: f64) -> Option<f64> {
    let mut lowest: Option<f64> = None;
    let mut from: Option<Point3> = None;
    for mv in moves {
        if let Some(start) = from
            && mv.is_cut()
            && let Some(z) = cut_z_near(start, mv, at, tolerance_mm)
        {
            lowest = Some(lowest.map_or(z, |l| l.min(z)));
        }
        from = Some(mv.to);
    }
    lowest
}

/// [`lowest_cut_through`], except that a straight move's Z is its highest
/// within `tolerance_mm` along it of where it comes nearest `at`: the
/// floor a rapid down to `at` stops [`ENTRY_CLEARANCE_MM`] above, robust
/// to the numbers being rounded as written (#464: a rapid stopped 1 mm
/// over the nearest point of a 15° V-bit's descent falling 4.4 mm per mm,
/// and once rounded to three places that point was 0.002 mm higher, so
/// S2 refused the rapid). A level cut's Z is the same.
fn highest_cut_near(moves: &[Move], at: Point, tolerance_mm: f64) -> Option<f64> {
    let mut lowest: Option<f64> = None;
    let mut from: Option<Point3> = None;
    for mv in moves {
        if let Some(start) = from
            && mv.is_cut()
            && let Some(z) = cut_z_within(start, mv, at, tolerance_mm, tolerance_mm)
        {
            lowest = Some(lowest.map_or(z, |l| l.min(z)));
        }
        from = Some(mv.to);
    }
    lowest
}

/// The Z, mm, at which cutting move `mv` from `start` passes within
/// `tolerance_mm` of `at` in XY, if it does.
fn cut_z_near(start: Point3, mv: &Move, at: Point, tolerance_mm: f64) -> Option<f64> {
    cut_z_within(start, mv, at, tolerance_mm, 0.0)
}

/// [`cut_z_near`], a straight move's Z taken as its highest within
/// `along_mm` along it of the point nearest `at`.
fn cut_z_within(
    start: Point3,
    mv: &Move,
    at: Point,
    tolerance_mm: f64,
    along_mm: f64,
) -> Option<f64> {
    let (a, b) = (start.xy(), mv.to.xy());
    let bulge = match mv.kind {
        MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
            let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
            (Arc::from_ends(a, b, centre, ccw).sweep_rad / 4.0).tan()
        }
        _ => 0.0,
    };
    let segment = Segment {
        start: a,
        end: b,
        bulge,
    };
    if point_segment_distance_mm(segment, at) > tolerance_mm {
        return None;
    }
    if bulge != 0.0 {
        return Some(start.z.max(mv.to.z));
    }
    let ab = b - a;
    let length_sq = ab.dot(ab);
    let t = if length_sq > EPS_MM * EPS_MM {
        ((at - a).dot(ab) / length_sq).clamp(0.0, 1.0)
    } else {
        // Straight down or up: the tool reached both ends' Z here.
        return Some(start.z.min(mv.to.z));
    };
    let z = |t: f64| start.z + (mv.to.z - start.z) * t.clamp(0.0, 1.0);
    let dt = along_mm / length_sq.sqrt();
    Some(z(t - dt).max(z(t + dt)))
}

fn distance_3d(a: Point3, b: Point3) -> f64 {
    let (dx, dy, dz) = (b.x - a.x, b.y - a.y, b.z - a.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// True when the straight link `from → to` lies inside the union of the
/// two cleared areas: its ends are in one piece of the union, it crosses
/// no boundary edge of that piece properly, the middle of every stretch
/// between the points where it touches the boundary is inside, and so is
/// every sample point along it (at most [`LINK_SAMPLE_STEP_MM`] apart).
///
/// The stretches (#232): a link that crosses no edge properly can still
/// leave the piece where it touches the boundary, at a vertex or at its
/// own ends. Between two touches it is wholly inside or wholly outside
/// the flattened boundary, so its middle tells which. A link from one
/// raster end to the next on the round tool-centre boundary about an
/// island corner is one such stretch, and at 0.34 mm it had no sample
/// point at all.
pub(crate) fn link_inside_cleared(a: &Region, b: &Region, from: Point, to: Point) -> bool {
    let pieces = union(
        std::slice::from_ref(a),
        std::slice::from_ref(b),
        FLATTEN_TOLERANCE_DEFAULT_MM,
    );
    let Some(piece) = pieces.iter().find(|r| r.contains_point(from)) else {
        return false;
    };
    if !piece.contains_point(to) {
        return false;
    }
    let link = to - from;
    let length_sq = link.dot(link);
    let at = |t: f64| Point::new(from.x + link.x * t, from.y + link.y * t);
    // Where along the link, 0 to 1, it touches the boundary.
    let mut touches = vec![0.0, 1.0];
    let boundary = std::iter::once(piece.outer()).chain(piece.holes());
    for contour in boundary {
        let flat = contour
            .polyline()
            .with_arcs_flattened(FLATTEN_TOLERANCE_DEFAULT_MM);
        for s in flat.segments() {
            if crosses_properly(from, to, s.start, s.end) {
                return false;
            }
            if length_sq > EPS_MM * EPS_MM {
                let t = (s.start - from).dot(link) / length_sq;
                if t > 0.0 && t < 1.0 && at(t).distance_to(s.start) <= EPS_MM {
                    touches.push(t);
                }
            }
        }
    }
    touches.sort_by(f64::total_cmp);
    let stretches_inside = touches
        .windows(2)
        .all(|w| matches!(w, [t0, t1] if piece.contains_point(at((t0 + t1) / 2.0))));
    if !stretches_inside {
        return false;
    }
    let length_mm = from.distance_to(to);
    // Rounds up; the count is small, so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (length_mm / LINK_SAMPLE_STEP_MM).ceil().max(1.0) as usize;
    (1..steps).all(|k| piece.contains_point(at(k as f64 / steps as f64)))
}

/// True when segments `p → q` and `r → s` cross at interior points of
/// both, each end being more than [`EPS_MM`] off the other segment's line.
/// Touching or collinear segments do not count.
fn crosses_properly(p: Point, q: Point, r: Point, s: Point) -> bool {
    let side = |a: Point, b: Point, c: Point| -> f64 {
        // Signed distance of `c` from the line `a → b`, mm.
        let d = b - a;
        let len = d.length();
        if len <= EPS_MM {
            return 0.0;
        }
        d.cross(c - a) / len
    };
    let opposite = |x: f64, y: f64| (x > EPS_MM && y < -EPS_MM) || (x < -EPS_MM && y > EPS_MM);
    opposite(side(p, q, r), side(p, q, s)) && opposite(side(r, s, p), side(r, s, q))
}

#[cfg(test)]
mod tests;
