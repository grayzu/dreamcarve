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
use crate::error::{BlockProblem, CamError, ShapeRef};
use crate::toolpath::{Move, Toolpath};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3, Region, union};
use dc_project::MachiningOrder;

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
    /// Absolute safe Z, mm: every rapid that moves in XY runs here.
    pub safe_z_mm: f64,
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
}

/// Longest gap between sample points when checking that a stay-down link
/// lies inside cleared material, mm. Links are at most a tool diameter plus
/// a step-over long, so this is a handful of points.
const LINK_SAMPLE_STEP_MM: f64 = 0.5;

/// The order the blocks of `input` are cut in, as indices into
/// `input.blocks` (spec steps 1–3): every unit (one shape at one pass) is
/// a run of consecutive indices in generator order; units are walked by
/// `input.order` with containment precedence and nearest neighbour, ties by
/// drawing order. [`sequence`] emits exactly this order.
///
/// Fails with [`CamError::InvalidBlock`] when a block's cuts go below its
/// pass Z, a shape's passes are not consecutive from 0, or a plunging block
/// starts away from its entry, and with [`CamError::ContainmentCycle`]
/// when the containment pairs leave no shape cuttable.
pub fn plan(input: &SequenceInput<'_>) -> Result<Vec<usize>, CamError> {
    let shapes = Shapes::new(input)?;
    shapes.walk(input.order)
}

/// Orders and links the blocks of `input` into one toolpath (spec steps
/// 4–5): the order is [`plan`]'s; between consecutive blocks `a → b` the
/// link is nothing when `b` is the next pass of `a`'s shape and its descent
/// starts where `a` ended; a stay-down `Linear` at feed when both blocks
/// have `cleared` areas at one Z and the straight link lies inside their
/// union and is at most `tool_diameter_mm + stepover_mm` long, which runs to
/// `b`'s entry and skips `b`'s descent (the tool is already at `b`'s Z in
/// cleared material); otherwise a rapid up to safe Z, a rapid in XY, and
/// the descent. A block's empty
/// descent is a straight plunge at the plunge rate; its own descent moves
/// are written as given, after a plunge to `descent_from_z_mm` when that is
/// below the height the tool arrives at. Zero-length moves are never
/// written. The first block is reached by a rapid at safe Z; the last is
/// followed by a rapid up to safe Z.
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
        if let Some(block) = input.blocks.get(index) {
            emitter.emit(block);
        }
    }
    emitter.finish();
    Ok(Toolpath {
        moves: emitter.moves,
    })
}

/// One shape's blocks at one pass, in generator order.
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
        let mut shapes: Vec<Shape> = Vec::new();
        // Blocks of each shape, by shape index, in generator order.
        let mut members: Vec<Vec<usize>> = Vec::new();
        for (index, block) in input.blocks.iter().enumerate() {
            validate_block(block)?;
            let position = shapes
                .iter()
                .position(|s| s.reference == block.shape)
                .unwrap_or_else(|| {
                    shapes.push(Shape {
                        reference: block.shape.clone(),
                        units: Vec::new(),
                        enclosed: Vec::new(),
                    });
                    members.push(Vec::new());
                    shapes.len() - 1
                });
            if let Some(list) = members.get_mut(position) {
                list.push(index);
            }
        }

        for (shape, member) in shapes.iter_mut().zip(&members) {
            let mut passes: Vec<usize> = member
                .iter()
                .filter_map(|&i| input.blocks.get(i).map(|b| b.pass))
                .collect();
            passes.sort_unstable();
            passes.dedup();
            if passes.iter().enumerate().any(|(k, &pass)| k != pass) {
                let first = member
                    .first()
                    .and_then(|&i| input.blocks.get(i))
                    .map_or(0, |b| b.pass);
                return Err(CamError::InvalidBlock {
                    shape: shape.reference.clone(),
                    pass: first,
                    problem: BlockProblem::PassesNotConsecutive { passes },
                });
            }
            for pass in passes {
                let blocks: Vec<usize> = member
                    .iter()
                    .copied()
                    .filter(|&i| input.blocks.get(i).is_some_and(|b| b.pass == pass))
                    .collect();
                let first = blocks.first().and_then(|&i| input.blocks.get(i));
                let last = blocks.last().and_then(|&i| input.blocks.get(i));
                shape.units.push(Unit {
                    start: first.map_or(Point::new(0.0, 0.0), |b| b.descent_from),
                    exit: last.map_or(Point::new(0.0, 0.0), |b| b.exit),
                    frees: blocks
                        .iter()
                        .any(|&i| input.blocks.get(i).is_some_and(|b| b.frees)),
                    blocks,
                });
            }
        }

        for (inner, encloser) in input.containment {
            let inner = shapes.iter().position(|s| &s.reference == inner);
            let encloser = shapes.iter().position(|s| &s.reference == encloser);
            if let (Some(inner), Some(encloser)) = (inner, encloser)
                && let Some(shape) = shapes.get_mut(encloser)
                && !shape.enclosed.contains(&inner)
            {
                shape.enclosed.push(inner);
            }
        }
        Ok(Self { shapes })
    }

    /// The greedy walk (spec step 3): from the current exit, the nearest
    /// available unit or stack by `f64::total_cmp`, ties by drawing order.
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
                MachiningOrder::FullDepthFirst => self.nearest_stack(&done, current),
                MachiningOrder::PassFirst => self.nearest_unit(&done, current),
            };
            let Some(index) = next else {
                // Blocks remain, so some shape is unfinished; it is on
                // the cycle or waits on one.
                let Some(stuck) = self
                    .shapes
                    .iter()
                    .zip(&done)
                    .find(|(s, d)| **d < s.units.len())
                    .map(|(s, _)| s.reference.clone())
                else {
                    break;
                };
                return Err(CamError::ContainmentCycle { shape: stuck });
            };
            let Some(shape) = self.shapes.get(index) else {
                break;
            };
            let emitted = match order {
                MachiningOrder::FullDepthFirst => shape.units.len(),
                MachiningOrder::PassFirst => done.get(index).copied().unwrap_or(0) + 1,
            };
            let from = done.get(index).copied().unwrap_or(0);
            for unit in shape.units.iter().take(emitted).skip(from) {
                out.extend(unit.blocks.iter().copied());
                current = unit.exit;
            }
            if let Some(d) = done.get_mut(index) {
                *d = emitted;
            }
        }
        Ok(out)
    }

    /// Full depth first: the nearest unstarted shape whose enclosed shapes
    /// are all finished.
    fn nearest_stack(&self, done: &[usize], current: Point) -> Option<usize> {
        self.shapes
            .iter()
            .enumerate()
            .filter(|(s, shape)| {
                done.get(*s).copied().unwrap_or(0) == 0
                    && shape.enclosed.iter().all(|&e| self.finished(done, e))
            })
            .filter_map(|(s, shape)| shape.units.first().map(|u| (s, u.start)))
            .min_by(|(sa, a), (sb, b)| {
                current
                    .distance_to(*a)
                    .total_cmp(&current.distance_to(*b))
                    .then(sa.cmp(sb))
            })
            .map(|(s, _)| s)
    }

    /// Pass first: among the available next units, the shallowest pass,
    /// then the nearest. A unit at pass `k` is available when every
    /// enclosed shape has finished pass `k` (or has no pass `k` and is
    /// finished) and, when the unit frees, has finished entirely.
    fn nearest_unit(&self, done: &[usize], current: Point) -> Option<usize> {
        self.shapes
            .iter()
            .enumerate()
            .filter_map(|(s, shape)| {
                let k = done.get(s).copied().unwrap_or(0);
                let unit = shape.units.get(k)?;
                let available = shape.enclosed.iter().all(|&e| {
                    let required = self
                        .shapes
                        .get(e)
                        .map_or(0, |enclosed| enclosed.units.len().min(k + 1));
                    let done_e = done.get(e).copied().unwrap_or(0);
                    done_e >= required && (!unit.frees || self.finished(done, e))
                });
                available.then_some((s, k, unit.start))
            })
            .min_by(|(sa, ka, a), (sb, kb, b)| {
                ka.cmp(kb)
                    .then(current.distance_to(*a).total_cmp(&current.distance_to(*b)))
                    .then(sa.cmp(sb))
            })
            .map(|(s, _, _)| s)
    }

    fn finished(&self, done: &[usize], shape: usize) -> bool {
        self.shapes
            .get(shape)
            .is_some_and(|s| done.get(shape).copied().unwrap_or(0) >= s.units.len())
    }
}

/// Spec step 1: cuts and descent at or above the pass Z; a plunging block
/// starts at its entry.
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
    Ok(())
}

/// How the tool gets from one block to the next (spec step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    /// The next block's descent starts where the tool is.
    None,
    /// A straight cut at feed through cleared material at the current Z.
    StayDown,
    /// Up to safe Z, across, and down.
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
        match (self.previous, self.current) {
            (Some(previous), Some(current)) => match self.link(previous, block, current) {
                Link::None => {}
                Link::StayDown => {
                    // Already at the block's Z in cleared material: straight
                    // to its entry, and its descent is not cut.
                    let to = Point3::new(block.entry.x, block.entry.y, current.z);
                    self.push(Move::linear(to, self.input.feed_mm_min));
                    self.cut(block);
                    return;
                }
                Link::Retract => {
                    self.push(Move::rapid(Point3::new(current.x, current.y, safe_z)));
                    self.push(Move::rapid(Point3::new(
                        block.descent_from.x,
                        block.descent_from.y,
                        safe_z,
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
        if let (Some(cleared_a), Some(cleared_b)) = (&a.cleared, &b.cleared)
            && (a.z_mm - b.z_mm).abs() <= EPS_MM
            && (current.z - b.z_mm).abs() <= EPS_MM
        {
            let length_mm = current.xy().distance_to(b.entry);
            let limit_mm = self.input.tool_diameter_mm + self.input.stepover_mm;
            if length_mm <= limit_mm + EPS_MM
                && link_inside_cleared(cleared_a, cleared_b, current.xy(), b.entry)
            {
                return Link::StayDown;
            }
        }
        Link::Retract
    }

    /// The descent into `block` from wherever the tool is: a straight
    /// plunge at the plunge rate for an empty descent, else the block's
    /// own moves after a plunge to `descent_from_z_mm` when the tool is
    /// above it.
    fn descend(&mut self, block: &CutBlock) {
        if block.descent.is_empty() {
            self.push(Move::linear(
                Point3::new(block.entry.x, block.entry.y, block.z_mm),
                self.input.plunge_mm_min,
            ));
            return;
        }
        if self
            .current
            .is_some_and(|c| c.z > block.descent_from_z_mm + EPS_MM)
        {
            self.push(Move::linear(
                Point3::new(
                    block.descent_from.x,
                    block.descent_from.y,
                    block.descent_from_z_mm,
                ),
                self.input.plunge_mm_min,
            ));
        }
        for mv in &block.descent {
            self.push(*mv);
        }
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

fn distance_3d(a: Point3, b: Point3) -> f64 {
    let (dx, dy, dz) = (b.x - a.x, b.y - a.y, b.z - a.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// True when the straight link `from → to` lies inside the union of the
/// two cleared areas: its ends are in one piece of the union, it crosses
/// no boundary edge of that piece properly, and every sample point along
/// it (at most [`LINK_SAMPLE_STEP_MM`] apart) is inside.
fn link_inside_cleared(a: &Region, b: &Region, from: Point, to: Point) -> bool {
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
    let boundary = std::iter::once(piece.outer()).chain(piece.holes());
    for contour in boundary {
        let flat = contour
            .polyline()
            .with_arcs_flattened(FLATTEN_TOLERANCE_DEFAULT_MM);
        if flat
            .segments()
            .any(|s| crosses_properly(from, to, s.start, s.end))
        {
            return false;
        }
    }
    let length_mm = from.distance_to(to);
    // Rounds up; the count is small, so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (length_mm / LINK_SAMPLE_STEP_MM).ceil().max(1.0) as usize;
    (1..steps).all(|k| {
        let t = k as f64 / steps as f64;
        let p = Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        piece.contains_point(p)
    })
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
mod tests {
    use super::*;
    use crate::error::BlockProblem;
    use crate::test_support::{Limits, assert_safe_within};
    use crate::toolpath::{Move, MoveKind};
    use dc_geometry::{Contour, EPS_MM, Point, Point3, Polyline, Region, nest};
    use dc_project::InstanceId;

    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    const DIAMETER_MM: f64 = 6.35;
    /// Pass Zs of a 6 mm cut in 2 mm steps on 6 mm stock, project space.
    const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];

    fn shape(name: &str) -> ShapeRef {
        ShapeRef {
            instance: InstanceId::new(name),
            instance_name: name.into(),
            profile: 0,
        }
    }

    /// A square `size` mm wide with its lower-left corner at (`x`, `y`),
    /// cut counter-clockwise from that corner at `pass` (Z `PASS_Z_MM[pass]`),
    /// plunging. `frees` on the last of `passes`.
    fn square(name: &str, x: f64, y: f64, size: f64, pass: usize, passes: usize) -> CutBlock {
        let z = PASS_Z_MM[pass];
        let start = Point::new(x, y);
        let corner = |px: f64, py: f64| Move::linear(Point3::new(px, py, z), FEED_MM_MIN);
        CutBlock {
            shape: shape(name),
            pass,
            z_mm: z,
            descent_from: start,
            descent_from_z_mm: if pass == 0 {
                SAFE_Z_MM
            } else {
                PASS_Z_MM[pass - 1]
            },
            descent: Vec::new(),
            entry: start,
            cuts: vec![
                corner(x + size, y),
                corner(x + size, y + size),
                corner(x, y + size),
                corner(x, y),
            ],
            exit: start,
            frees: pass + 1 == passes && PASS_Z_MM[pass] <= EPS_MM,
            cleared: None,
            group: 0,
        }
    }

    /// Every pass of [`square`], top first.
    fn squares(name: &str, x: f64, y: f64, size: f64, passes: usize) -> Vec<CutBlock> {
        (0..passes)
            .map(|pass| square(name, x, y, size, pass, passes))
            .collect()
    }

    fn pairs(pairs: &[(&str, &str)]) -> Vec<(ShapeRef, ShapeRef)> {
        pairs
            .iter()
            .map(|(inner, outer)| (shape(inner), shape(outer)))
            .collect()
    }

    fn input<'a>(
        blocks: Vec<CutBlock>,
        order: MachiningOrder,
        containment: &'a [(ShapeRef, ShapeRef)],
    ) -> SequenceInput<'a> {
        SequenceInput {
            blocks,
            order,
            containment,
            safe_z_mm: SAFE_Z_MM,
            tool_diameter_mm: DIAMETER_MM,
            stepover_mm: 0.0,
            feed_mm_min: FEED_MM_MIN,
            plunge_mm_min: PLUNGE_MM_MIN,
        }
    }

    fn limits() -> Limits {
        Limits {
            safe_z_mm: SAFE_Z_MM,
            stock_top_mm: THICKNESS_MM,
            floor_z_mm: 0.0,
            feed_mm_min: FEED_MM_MIN,
            plunge_mm_min: PLUNGE_MM_MIN,
        }
    }

    /// Sequences, and checks the result with `assert_safe_within`.
    fn run(input: &SequenceInput<'_>) -> Toolpath {
        let toolpath = sequence(input).unwrap();
        assert_safe_within(&toolpath, &limits());
        toolpath
    }

    /// The blocks in the order their first cut appears in `toolpath`, as
    /// `(shape name, pass)`.
    fn visits(toolpath: &Toolpath, blocks: &[CutBlock]) -> Vec<(String, usize)> {
        let mut found: Vec<(usize, &CutBlock)> = blocks
            .iter()
            .map(|b| {
                let first = b.cuts[0];
                let at = toolpath
                    .moves
                    .iter()
                    .position(|m| *m == first)
                    .unwrap_or_else(|| panic!("block {:?} pass {} not emitted", b.shape, b.pass));
                (at, b)
            })
            .collect();
        found.sort_by_key(|(at, _)| *at);
        found
            .into_iter()
            .map(|(_, b)| (b.shape.instance_name.clone(), b.pass))
            .collect()
    }

    fn names(visits: &[(String, usize)]) -> Vec<&str> {
        visits.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Port of `cutZTransitions` (PureCutCNC `toolpaths.test.ts`): the Z of
    /// each non-rapid move, with repeats collapsed.
    fn cut_z_transitions(toolpath: &Toolpath) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for mv in toolpath.moves.iter().filter(|m| !m.is_rapid()) {
            if out.last().is_none_or(|z| (z - mv.to.z).abs() > EPS_MM) {
                out.push(mv.to.z);
            }
        }
        out
    }

    fn assert_z(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() <= EPS_MM, "{actual:?} vs {expected:?}");
        }
    }

    const BOTH: [MachiningOrder; 2] = [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst];

    /// O1: with a (hole, outer) pair every pass of the hole precedes the
    /// outer's freeing pass, whichever was drawn first, under both orders.
    #[test]
    fn hole_is_cut_before_its_outline() {
        let outer = squares("outer", 0.0, 0.0, 100.0, 2);
        let hole = squares("hole", 40.0, 40.0, 20.0, 2);
        let pairs = pairs(&[("hole", "outer")]);
        for order in BOTH {
            for blocks in [
                [outer.clone(), hole.clone()].concat(),
                [hole.clone(), outer.clone()].concat(),
            ] {
                let tp = run(&input(blocks.clone(), order, &pairs));
                let expected: Vec<(String, usize)> = match order {
                    MachiningOrder::FullDepthFirst => {
                        [("hole", 0), ("hole", 1), ("outer", 0), ("outer", 1)]
                    }
                    MachiningOrder::PassFirst => {
                        [("hole", 0), ("outer", 0), ("hole", 1), ("outer", 1)]
                    }
                }
                .map(|(n, p)| (n.to_string(), p))
                .to_vec();
                assert_eq!(visits(&tp, &blocks), expected, "{order:?}");
            }
        }
    }

    /// O1 (`docs/research/toolpath-ordering.md`): a frame (outline A, hole
    /// B) with a coaster C inside the hole. Whatever the drawing order, the
    /// coaster is cut first, then the hole that frees the waste around it,
    /// then the outline.
    #[test]
    fn island_is_cut_before_the_hole_around_it() {
        let a = squares("A", 0.0, 0.0, 200.0, 2);
        let b = squares("B", 20.0, 20.0, 160.0, 2);
        let c = squares("C", 70.0, 45.0, 60.0, 2);
        let pairs = pairs(&[("B", "A"), ("C", "B")]);
        for order in BOTH {
            for blocks in [
                [a.clone(), b.clone(), c.clone()].concat(),
                [c.clone(), b.clone(), a.clone()].concat(),
                [b.clone(), c.clone(), a.clone()].concat(),
            ] {
                let tp = run(&input(blocks.clone(), order, &pairs));
                let got = visits(&tp, &blocks);
                let expected = match order {
                    MachiningOrder::FullDepthFirst => ["C", "C", "B", "B", "A", "A"],
                    MachiningOrder::PassFirst => ["C", "B", "A", "C", "B", "A"],
                };
                assert_eq!(names(&got), expected, "{order:?}: {got:?}");
            }
        }
    }

    /// Four nested contours drawn outermost first: the innermost is cut
    /// first, and each hole still comes before its own outline.
    #[test]
    fn four_levels_cut_innermost_first() {
        let blocks = [
            squares("outer", 0.0, 0.0, 200.0, 2),
            squares("hole", 20.0, 20.0, 160.0, 2),
            squares("island", 40.0, 40.0, 120.0, 2),
            squares("island hole", 60.0, 60.0, 80.0, 2),
        ]
        .concat();
        let pairs = pairs(&[
            ("hole", "outer"),
            ("island", "hole"),
            ("island hole", "island"),
        ]);
        let tp = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        assert_eq!(
            names(&visits(&tp, &blocks)),
            [
                "island hole",
                "island hole",
                "island",
                "island",
                "hole",
                "hole",
                "outer",
                "outer"
            ]
        );
    }

    /// O2: sequencing is a permutation. Three parts with holes, one of them
    /// with a tab-raised cut, give the same multiset of cutting moves at
    /// every Z under both orders.
    #[test]
    fn orders_cut_the_same_moves() {
        let mut blocks = Vec::new();
        for (i, x) in [0.0, 60.0, 120.0].into_iter().enumerate() {
            blocks.extend(squares(&format!("part {i}"), x, 0.0, 50.0, 3));
            blocks.extend(squares(&format!("hole {i}"), x + 20.0, 20.0, 10.0, 3));
        }
        // A tab on the first part's final pass: the third edge rises.
        let tabbed = &mut blocks[2];
        assert_eq!(tabbed.pass, 2);
        tabbed.cuts[2].to.z = 3.0;
        tabbed
            .cuts
            .insert(2, Move::linear(Point3::new(50.0, 50.0, 3.0), PLUNGE_MM_MIN));
        tabbed
            .cuts
            .insert(4, Move::linear(Point3::new(0.0, 50.0, 0.0), PLUNGE_MM_MIN));
        let pairs = pairs(&[
            ("hole 0", "part 0"),
            ("hole 1", "part 1"),
            ("hole 2", "part 2"),
        ]);
        let cutting = |tp: &Toolpath| {
            let mut out: Vec<String> = Vec::new();
            let mut pos = tp.moves[0].to;
            for mv in &tp.moves {
                if !mv.is_rapid() && pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                    out.push(format!("{:?}", mv));
                }
                pos = mv.to;
            }
            out.sort();
            out
        };
        let full = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        let pass = run(&input(blocks.clone(), MachiningOrder::PassFirst, &pairs));
        assert_ne!(full.moves, pass.moves, "the orders differ");
        assert_eq!(cutting(&full), cutting(&pass));
        assert_eq!(
            cutting(&full).len(),
            6 * 3 * 4,
            "the tab lifts are vertical"
        );
    }

    /// O3: between two shapes the tool goes up to safe Z, across, and down
    /// again: a vertical rapid, an XY rapid at safe Z, then the plunge.
    #[test]
    fn links_between_shapes_retract_to_safe_z() {
        let blocks = [
            squares("a", 0.0, 0.0, 20.0, 1),
            squares("b", 50.0, 0.0, 20.0, 1),
        ]
        .concat();
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let last_cut_of_a = 1 + 4;
        let m = &tp.moves;
        assert_eq!(m[last_cut_of_a].to, Point3::new(0.0, 0.0, 4.0));
        assert_eq!(
            m[last_cut_of_a + 1],
            Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM))
        );
        assert_eq!(
            m[last_cut_of_a + 2],
            Move::rapid(Point3::new(50.0, 0.0, SAFE_Z_MM))
        );
        assert_eq!(
            m[last_cut_of_a + 3],
            Move::linear(Point3::new(50.0, 0.0, 4.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.iter().filter(|m| m.is_rapid()).count(), 4);
        for mv in m.iter().filter(|m| m.is_rapid()) {
            assert!((mv.to.z - SAFE_Z_MM).abs() <= EPS_MM, "{mv:?}");
        }
    }

    fn region(x: f64, y: f64, size: f64) -> Region {
        let contour = Contour::new(Polyline::rect(Point::new(x, y), size, size)).unwrap();
        nest(vec![contour]).unwrap().0.remove(0)
    }

    /// Two cleared blocks at one Z whose starts are `link_mm` apart in X.
    fn cleared_pair(b_x: f64, b_y: f64) -> Vec<CutBlock> {
        let mut a = square("a", 0.0, 0.0, 20.0, 0, 1);
        a.cleared = Some(region(0.0, 0.0, 20.0));
        let mut b = square("b", b_x, b_y, 20.0, 0, 1);
        b.cleared = Some(region(b_x, b_y, 20.0));
        vec![a, b]
    }

    /// The move right after `a`'s last cut (index 5).
    fn link_move(tp: &Toolpath) -> Move {
        tp.moves[6]
    }

    /// O3: a short link inside the union of two cleared areas stays down as
    /// a `Linear` at feed; a link outside the union, longer than
    /// `tool_diameter + stepover`, or from a block without a cleared area
    /// retracts to safe Z.
    #[test]
    fn stay_down_link_only_inside_cleared_area() {
        // b starts at (10, 5), inside a's cleared square; 11.18 mm away.
        let mut inside = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        inside.stepover_mm = 10.0;
        let tp = run(&inside);
        assert_eq!(
            link_move(&tp),
            Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN)
        );
        assert_eq!(
            tp.moves.iter().filter(|m| m.is_rapid()).count(),
            2,
            "{:?}",
            tp.moves
        );
        // The link is followed straight by b's cuts: no plunge at the same Z.
        assert_eq!(
            tp.moves[7],
            Move::linear(Point3::new(30.0, 5.0, 4.0), FEED_MM_MIN)
        );

        // Too long for the threshold: 6.35 + 2 < 11.18.
        let mut long = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        long.stepover_mm = 2.0;
        assert!(link_move(&run(&long)).is_rapid());

        // Outside the union: b starts at (10, 30), above both squares.
        let mut outside = input(
            cleared_pair(10.0, 30.0),
            MachiningOrder::FullDepthFirst,
            &[],
        );
        outside.stepover_mm = 30.0;
        assert!(link_move(&run(&outside)).is_rapid());

        // No cleared area on a: a profile never stays down.
        let mut none = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        none.stepover_mm = 10.0;
        none.blocks[0].cleared = None;
        assert!(link_move(&run(&none)).is_rapid());
    }

    /// Slice 9: a block whose descent is a ramp or helix keeps it for when
    /// the tool arrives from above. Reached by a stay-down link, the tool
    /// is already at the block's Z in cleared material, so the link runs
    /// straight to the block's entry and the descent is not cut: replaying
    /// it from `descent_from` would climb back up through cleared air.
    #[test]
    fn stay_down_link_skips_the_descent() {
        let mut blocks = cleared_pair(10.0, 5.0);
        // b ramps in from (14, 5) at Z 4.4 down to its entry (10, 5) at Z 4.
        blocks[1].descent_from = Point::new(14.0, 5.0);
        blocks[1].descent_from_z_mm = 4.4;
        blocks[1].descent = vec![
            Move::linear(Point3::new(12.0, 5.0, 4.2), FEED_MM_MIN),
            Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN),
        ];
        let mut stay = input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]);
        stay.stepover_mm = 10.0;
        let tp = run(&stay);
        assert_eq!(
            link_move(&tp),
            Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN),
            "{:?}",
            tp.moves
        );
        assert_eq!(
            tp.moves[7],
            Move::linear(Point3::new(30.0, 5.0, 4.0), FEED_MM_MIN),
            "b's cuts follow the link: {:?}",
            tp.moves
        );
        assert!(
            tp.moves
                .iter()
                .all(|m| m.to.z <= 4.0 + EPS_MM || m.is_rapid()),
            "no descent move is replayed: {:?}",
            tp.moves
        );

        // Reached by a retract, the descent is cut as given.
        let mut retract = input(blocks, MachiningOrder::FullDepthFirst, &[]);
        retract.stepover_mm = 2.0;
        let tp = run(&retract);
        let at = tp
            .moves
            .iter()
            .position(|m| *m == Move::linear(Point3::new(12.0, 5.0, 4.2), FEED_MM_MIN))
            .expect("the descent is cut");
        assert_eq!(
            tp.moves[at - 1],
            Move::linear(Point3::new(14.0, 5.0, 4.4), PLUNGE_MM_MIN)
        );
        assert_eq!(
            tp.moves[at - 2],
            Move::rapid(Point3::new(14.0, 5.0, SAFE_Z_MM))
        );
    }

    /// Port of `testPocketFeatureFirstOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): two disjoint shapes, two
    /// passes, full depth first. Upstream reads `[-2, -4, -2, -4]` below the
    /// stock top; here Z is above the stock bottom of 6 mm stock, so the
    /// same passes are `[4, 2, 4, 2]` (O4).
    #[test]
    fn full_depth_first_z_transitions() {
        let blocks = [
            squares("left", 0.0, 0.0, 10.0, 2),
            squares("right", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        assert_z(&cut_z_transitions(&tp), &[4.0, 2.0, 4.0, 2.0]);
        // Each half of the move list holds one shape at both depths.
        let mid = tp.moves.len() / 2;
        for half in [&tp.moves[..mid], &tp.moves[mid..]] {
            let zs: Vec<f64> = half
                .iter()
                .filter(|m| !m.is_rapid())
                .map(|m| m.to.z)
                .collect();
            assert!(zs.iter().any(|z| (z - 4.0).abs() <= EPS_MM), "{zs:?}");
            assert!(zs.iter().any(|z| (z - 2.0).abs() <= EPS_MM), "{zs:?}");
        }
    }

    /// Port of `testPocketLevelFirstOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): the same two shapes under
    /// pass first cut one depth across both, then the next: `[4, 2]`
    /// (upstream `[-2, -4]`) (O4).
    #[test]
    fn pass_first_z_transitions() {
        let blocks = [
            squares("left", 0.0, 0.0, 10.0, 2),
            squares("right", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_z(&cut_z_transitions(&tp), &[4.0, 2.0]);
        let got = visits(&tp, &blocks);
        for name in ["left", "right"] {
            for pass in 0..2 {
                assert!(got.contains(&(name.to_string(), pass)), "{got:?}");
            }
        }
    }

    /// Under pass first each layer's walk starts from the previous layer's
    /// last exit, so the second layer runs back the other way.
    #[test]
    fn pass_first_walk_starts_from_the_previous_pass_exit() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
            squares("c", 60.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_eq!(names(&visits(&tp, &blocks)), ["a", "b", "c", "c", "b", "a"]);
    }

    /// O5: two shapes at the same distance from the current exit are
    /// visited in drawing order.
    #[test]
    fn equidistant_shapes_keep_drawing_order() {
        let a = squares("a", 0.0, 0.0, 10.0, 1);
        let b = squares("b", 30.0, 0.0, 10.0, 1);
        let c = squares("c", -30.0, 0.0, 10.0, 1);
        for (blocks, expected) in [
            ([a.clone(), b.clone(), c.clone()].concat(), ["a", "b", "c"]),
            ([a.clone(), c.clone(), b.clone()].concat(), ["a", "c", "b"]),
        ] {
            for order in BOTH {
                let tp = run(&input(blocks.clone(), order, &[]));
                assert_eq!(names(&visits(&tp, &blocks)), expected, "{order:?}");
            }
        }
    }

    /// Port of `testPocketFeatureFirstNearestBlockOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): shapes at X 0, 30 and 200
    /// targeted as a, c, b are cut a, b, c, each with both passes (O5).
    #[test]
    fn nearest_neighbour_orders_a_c_b_as_a_b_c() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("c", 200.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]));
        assert_eq!(names(&visits(&tp, &blocks)), ["a", "a", "b", "b", "c", "c"]);
        assert_eq!(cut_z_transitions(&tp).len(), 6, "two passes per shape");
    }

    /// O5: the same input twice gives the same toolpath.
    #[test]
    fn same_input_twice_is_identical() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 3),
            squares("c", 200.0, 0.0, 10.0, 3),
            squares("b", 30.0, 0.0, 10.0, 3),
        ]
        .concat();
        for order in BOTH {
            let first = run(&input(blocks.clone(), order, &[]));
            let second = run(&input(blocks.clone(), order, &[]));
            assert_eq!(first, second, "{order:?}");
        }
    }

    /// O6: a one-shape operation is unchanged by the ordering setting, and
    /// is the phase 1 recipe: rapid in, per pass a plunge and the cuts,
    /// rapid out.
    #[test]
    fn single_shape_is_identical_in_both_orders() {
        let blocks = squares("only", 10.0, 10.0, 20.0, 3);
        let full = run(&input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]));
        let pass = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_eq!(full, pass);
        let mut expected = vec![Move::rapid(Point3::new(10.0, 10.0, SAFE_Z_MM))];
        for block in &blocks {
            expected.push(Move::linear(
                Point3::new(10.0, 10.0, block.z_mm),
                PLUNGE_MM_MIN,
            ));
            expected.extend(block.cuts.iter().copied());
        }
        expected.push(Move::rapid(Point3::new(10.0, 10.0, SAFE_Z_MM)));
        assert_eq!(full.moves, expected);
    }

    /// Under pass first an encloser's freeing pass waits until the shape it
    /// encloses has finished entirely, even when that shape has more
    /// passes: the encloser's pass 1 (which frees) comes after the enclosed
    /// shape's pass 2.
    #[test]
    fn pass_first_waits_for_enclosed_shapes_freeing_pass() {
        let mut encloser = squares("E", 0.0, 0.0, 100.0, 2);
        encloser[1].frees = true;
        let enclosed = squares("I", 40.0, 40.0, 20.0, 3);
        let blocks = [encloser, enclosed].concat();
        let pairs = pairs(&[("I", "E")]);
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &pairs));
        let expected: Vec<(String, usize)> = [("I", 0), ("E", 0), ("I", 1), ("I", 2), ("E", 1)]
            .map(|(n, p)| (n.to_string(), p))
            .to_vec();
        assert_eq!(visits(&tp, &blocks), expected);
        let tp = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        assert_eq!(names(&visits(&tp, &blocks)), ["I", "I", "I", "E", "E"]);
    }

    /// A relief-notch-like block of shape `name` at `pass`: a plunge at
    /// (`x`, `y`) from safe Z, out 1 mm down and left, and back.
    fn notch(name: &str, x: f64, y: f64, pass: usize) -> CutBlock {
        let z = PASS_Z_MM[pass];
        let at = Point::new(x, y);
        CutBlock {
            shape: shape(name),
            pass,
            z_mm: z,
            descent_from: at,
            descent_from_z_mm: SAFE_Z_MM,
            descent: Vec::new(),
            entry: at,
            cuts: vec![
                Move::linear(Point3::new(x - 1.0, y - 1.0, z), FEED_MM_MIN),
                Move::linear(Point3::new(x, y, z), FEED_MM_MIN),
            ],
            exit: at,
            frees: false,
            cleared: None,
            group: 0,
        }
    }

    /// #189: nothing of a shape is cut after its freeing pass. Each pass of
    /// an outline and of its hole is followed, in generator order, by a
    /// notch block at that pass, as `relief::profile_notches` hands them
    /// over. The sequencer cuts a unit's freeing blocks last, so the
    /// notches at the last pass come before the lap that frees, under both
    /// orders and both drawing orders, and O1 still holds: the hole is
    /// finished before the outline's freeing lap.
    #[test]
    fn nothing_is_cut_after_a_shapes_freeing_pass() {
        let with_notches = |name: &str, x: f64, y: f64, size: f64| -> Vec<CutBlock> {
            let mut blocks = squares(name, x, y, size, 3);
            blocks.extend((0..3).map(|pass| notch(name, x, y, pass)));
            blocks
        };
        let outer = with_notches("outer", 0.0, 0.0, 100.0);
        let hole = with_notches("hole", 40.0, 40.0, 20.0);
        let pairs = pairs(&[("hole", "outer")]);
        for order in BOTH {
            for blocks in [
                [outer.clone(), hole.clone()].concat(),
                [hole.clone(), outer.clone()].concat(),
            ] {
                let tp = run(&input(blocks.clone(), order, &pairs));
                // Where each block's first cut is in the toolpath.
                let at: Vec<usize> = blocks
                    .iter()
                    .map(|b| {
                        tp.moves
                            .iter()
                            .position(|m| *m == b.cuts[0])
                            .unwrap_or_else(|| panic!("{:?} pass {} not cut", b.shape, b.pass))
                    })
                    .collect();
                let freeing = |name: &str| {
                    blocks
                        .iter()
                        .zip(&at)
                        .find(|(b, _)| b.frees && b.shape.instance_name == name)
                        .map(|(_, &i)| i)
                        .unwrap()
                };
                for (b, &i) in blocks.iter().zip(&at) {
                    let name = b.shape.instance_name.as_str();
                    assert!(
                        i <= freeing(name),
                        "{order:?}: {name} pass {} is cut after its freeing pass",
                        b.pass
                    );
                    if name == "hole" {
                        assert!(i < freeing("outer"), "{order:?}: O1");
                    }
                }
            }
        }
    }

    /// Link rule 1: consecutive passes of one shape whose descent starts
    /// where the last pass ended have no link; the plunge goes straight
    /// from the previous pass Z.
    #[test]
    fn same_shape_next_pass_stays_down() {
        let tp = run(&input(
            squares("s", 0.0, 0.0, 20.0, 2),
            MachiningOrder::FullDepthFirst,
            &[],
        ));
        let m = &tp.moves;
        assert_eq!(m[5].to, Point3::new(0.0, 0.0, 4.0), "last cut of pass 0");
        assert_eq!(
            m[6],
            Move::linear(Point3::new(0.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.len(), 1 + 2 * 5 + 1);
    }

    /// Link rule 2: a next pass whose descent starts elsewhere (a moved
    /// seam) is reached by a retract to safe Z, an XY rapid and a plunge.
    #[test]
    fn moved_entry_retracts_between_passes() {
        let mut blocks = squares("s", 0.0, 0.0, 20.0, 2);
        let moved = &mut blocks[1];
        moved.descent_from = Point::new(20.0, 0.0);
        moved.entry = Point::new(20.0, 0.0);
        moved.exit = Point::new(20.0, 0.0);
        moved.cuts.rotate_left(1);
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let m = &tp.moves;
        assert_eq!(m[6], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
        assert_eq!(m[7], Move::rapid(Point3::new(20.0, 0.0, SAFE_Z_MM)));
        assert_eq!(
            m[8],
            Move::linear(Point3::new(20.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
    }

    #[test]
    fn first_and_last_moves_are_rapids_at_safe_z() {
        let blocks = [
            squares("a", 5.0, 5.0, 10.0, 2),
            squares("b", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        // Full depth first ends at b; pass first walks back and ends at a.
        for (order, last_xy) in [
            (MachiningOrder::FullDepthFirst, Point::new(40.0, 0.0)),
            (MachiningOrder::PassFirst, Point::new(5.0, 5.0)),
        ] {
            let tp = run(&input(blocks.clone(), order, &[]));
            assert_eq!(tp.moves[0], Move::rapid(Point3::new(5.0, 5.0, SAFE_Z_MM)));
            assert_eq!(
                *tp.moves.last().unwrap(),
                Move::rapid(Point3::new(last_xy.x, last_xy.y, SAFE_Z_MM)),
                "{order:?}"
            );
        }
    }

    /// A block's own descent moves are written as given; a descent move
    /// that ends where the tool already is (the previous pass's exit) is
    /// dropped rather than written as a zero-length move.
    #[test]
    fn generator_descent_is_used_and_zero_length_moves_are_dropped() {
        let mut blocks = squares("s", 5.0, 0.0, 20.0, 2);
        // Pass 0 ramps in from (0, 0): plunge to the stock top, ramp to
        // the entry at Z 4, 2 mm down over 5 mm at the feed scaled so it
        // falls at the plunge rate (S10).
        let ramp_mm_min = PLUNGE_MM_MIN * 29f64.sqrt() / 2.0;
        blocks[0].descent_from = Point::new(0.0, 0.0);
        blocks[0].descent = vec![
            Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN),
            Move::linear(Point3::new(5.0, 0.0, 4.0), ramp_mm_min),
        ];
        // Pass 1's descent starts with a move to where pass 0 ended.
        blocks[1].descent = vec![
            Move::linear(Point3::new(5.0, 0.0, 4.0), PLUNGE_MM_MIN),
            Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN),
        ];
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let m = &tp.moves;
        assert_eq!(m[0], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
        assert_eq!(
            m[1],
            Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN)
        );
        assert_eq!(m[2], Move::linear(Point3::new(5.0, 0.0, 4.0), ramp_mm_min));
        assert_eq!(m[6].to, Point3::new(5.0, 0.0, 4.0), "last cut of pass 0");
        assert_eq!(
            m[7],
            Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.len(), 1 + 2 + 4 + 1 + 4 + 1);
    }

    /// A dwell ends where the tool already is, but it is a pause, not a
    /// zero-length move: it is written.
    #[test]
    fn dwell_moves_are_kept() {
        let mut blocks = squares("s", 5.0, 0.0, 20.0, 1);
        let at = blocks[0].cuts[0].to;
        blocks[0].cuts.insert(1, Move::dwell(at, 0.5));
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        assert!(
            tp.moves
                .iter()
                .any(|m| m.kind == MoveKind::Dwell { seconds: 0.5 }),
            "{:?}",
            tp.moves
        );
    }

    /// Two blocks of one shape at one pass are written in generator order
    /// under both machining orders, even when the nearer one is second.
    #[test]
    fn blocks_of_one_unit_keep_generator_order() {
        let mut near = square("s", 0.0, 0.0, 10.0, 0, 2);
        let mut far = square("s", 50.0, 0.0, 10.0, 0, 2);
        near.group = 1;
        far.group = 1;
        let mut near_1 = square("s", 0.0, 0.0, 10.0, 1, 2);
        let mut far_1 = square("s", 50.0, 0.0, 10.0, 1, 2);
        near_1.group = 1;
        far_1.group = 1;
        // Pass 0 ends at the far loop; pass 1 lists the near loop first.
        let blocks = vec![near, far, near_1, far_1];
        for order in BOTH {
            let tp = run(&input(blocks.clone(), order, &[]));
            let starts: Vec<f64> = tp
                .moves
                .iter()
                .filter(|m| !m.is_rapid() && m.to.z < SAFE_Z_MM)
                .filter(|m| {
                    m.to.y.abs() <= EPS_MM
                        && ((m.to.x - 10.0).abs() <= EPS_MM || (m.to.x - 60.0).abs() <= EPS_MM)
                })
                .map(|m| m.to.x)
                .collect();
            assert_eq!(starts, [10.0, 60.0, 10.0, 60.0], "{order:?}");
        }
    }

    #[test]
    fn cut_below_pass_z_is_an_error() {
        let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
        blocks[0].cuts[1].to.z = 3.5;
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert_eq!(
            err,
            CamError::InvalidBlock {
                shape: shape("s"),
                pass: 0,
                problem: BlockProblem::MoveBelowPass {
                    z_mm: 4.0,
                    lowest_mm: 3.5
                }
            }
        );
    }

    #[test]
    fn non_consecutive_passes_are_an_error() {
        let blocks = vec![
            square("s", 0.0, 0.0, 10.0, 0, 3),
            square("s", 0.0, 0.0, 10.0, 2, 3),
        ];
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert!(
            matches!(
                &err,
                CamError::InvalidBlock {
                    problem: BlockProblem::PassesNotConsecutive { passes },
                    ..
                } if passes == &[0, 2]
            ),
            "{err:?}"
        );
    }

    #[test]
    fn plunge_away_from_entry_is_an_error() {
        let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
        blocks[0].descent_from = Point::new(3.0, 0.0);
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::InvalidBlock {
                    problem: BlockProblem::PlungeAwayFromEntry,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn containment_cycle_is_an_error() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 1),
            squares("b", 30.0, 0.0, 10.0, 1),
        ]
        .concat();
        let pairs = pairs(&[("a", "b"), ("b", "a")]);
        for order in BOTH {
            let err = sequence(&input(blocks.clone(), order, &pairs)).unwrap_err();
            assert!(matches!(err, CamError::ContainmentCycle { .. }), "{err:?}");
        }
    }

    /// `plan` is the order `sequence` writes: each unit a run of generator
    /// order indices.
    #[test]
    fn plan_matches_the_emitted_order() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("c", 200.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
        ]
        .concat();
        let input = input(blocks, MachiningOrder::PassFirst, &[]);
        assert_eq!(plan(&input).unwrap(), [0, 4, 2, 3, 5, 1]);
    }
}
