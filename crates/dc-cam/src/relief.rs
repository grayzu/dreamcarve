// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/cornerRelief.ts
//   src/engine/toolpaths/cornerRelief.test.ts

//! Corner relief: dogbone and T-bone notches (`docs/specs/phase-2/design.md`,
//! "Algorithms → Corner relief (dc-cam::relief)"; functional spec
//! capability 18, acceptance 29). VCarve calls these "Create Fillets":
//! Dog-Bone and T-Bone.
//!
//! As upstream, relief is a pass of its own, stepping down at the tool's
//! step-down, never a change to the cleared region (a radius-`r` lobe
//! eroded by a radius-`r` cutter vanishes) nor a detour in the wall ring.
//! Per corner and level: plunge at the descend point, where the main path
//! cuts to full depth, cut out to the end point, and cut back.
//!
//! **When.** A Pocket's notches follow all its main passes, as upstream: a
//! Pocket never frees material. A Profile's are cut among its shape's own
//! passes instead (#189), because a Profile through the stock frees its
//! part or slug with its final lap, and a notch cut after that cuts beside
//! a loose piece. Each notch level is cut in the unit of the shape's
//! first pass at or below it, after that pass's lap, so the lap has
//! already opened the column at the descend point. The levels of the
//! freeing pass are cut from inside the final lap (#193), where it first
//! reaches each descend point at depth, so they never plunge through the
//! skin and the rest of the lap still holds the piece. Nothing of a shape
//! is cut after its freeing pass.
//!
//! **The touch-the-corner rule.** For corner `C` of the cleared region,
//! with unit edge directions `u`, `v` from `C`, bisector `b`, cleared-side
//! interior angle `θ` and cutter radius `r`, the excursion ends where the
//! cutter just touches `C`: `C + b·r` for a dogbone, `C + u·r` for a T-bone
//! along the longer adjacent edge `u` (VCarve's placement; on a tie the more
//! X-parallel edge, then the one pointing more to `+x`). Upstream's
//! X-parallel `t_bone` and its `longest_edge` variant are one style here.
//! The notch reaches `r(1 − sin(θ/2))` past each wall for a dogbone and `r`
//! past its own wall for a T-bone.
//!
//! **Which corners.** Each cleared loop is walked with the cleared side on
//! its left (a Pocket's outline and an inside Profile's outline are
//! cleared inside, a Pocket's islands and an outside Profile's part are
//! cleared outside); a vertex where the path turns left by more than
//! [`RELIEF_MIN_TURN_RAD`] is a corner. DreamCarve keeps arcs as arcs, so
//! the turn is measured between the two segments' tangents: a fillet or a
//! circle has no corner at all, where upstream relies on the turn
//! threshold to skip tessellation steps.
//!
//! Re-derived for Y-up (ADR-0001): "cleared on the left" is the same rule
//! in both frames once the loop is wound for it, and the winding is taken
//! from the loop's signed area, never assumed.
//!
//! Upstream's keep-outs are dropped (design spec): a corner under a tab is
//! caught by the one guard, because the tab lifts the main path over the
//! descend point at the deepest level.

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::footprint::{capsule, move_bulge};
use crate::passes::pass_z_mm;
use crate::profile::{Wall, rates_of, shape_of};
use crate::sequence::{SequenceInput, nearest_of, sequence};
use crate::tabs::LapRates;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::warning::{ReliefCornerReason, ReliefSkippedReason, Warning};
use dc_geometry::{
    Contour, EPS_MM, Point, Point3, Polyline, Region, Segment, Vec2, point_segment_distance_mm,
    union,
};
use dc_project::{CornerRelief, MachiningOrder, OperationId, ProfileOp, Project, Side, Tool};
use std::f64::consts::PI;

/// The smallest turn toward the cleared side at one vertex that counts as
/// a corner, 15° (design spec). Upstream derives its threshold from the
/// arc-flattening step; DreamCarve measures turns between tangents, so arcs
/// never reach it, and the corners it skips (interior angle above 165°)
/// have a notch under `0.01 r` deep.
pub const RELIEF_MIN_TURN_RAD: f64 = PI / 12.0;

/// Position tolerance, mm (upstream `RELIEF_POSITION_TOLERANCE`): the
/// slack on the descend-point search window and on the guard.
pub const RELIEF_POSITION_TOLERANCE_MM: f64 = 1e-3;

/// Z slack for "the main path cut at or below this level", mm.
const RELIEF_Z_TOLERANCE_MM: f64 = 1e-6;

/// Two unit-vector components closer than this are equal for the T-bone
/// tie-break.
const UNIT_EPSILON: f64 = 1e-9;

/// The two relief styles (`CornerRelief` less `none`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    /// A notch along the corner's bisector.
    Dogbone,
    /// A notch along the corner's longer edge.
    TBone,
}

impl Style {
    /// The style `relief` asks for; `None` for no relief.
    pub(crate) const fn of(relief: CornerRelief) -> Option<Self> {
        match relief {
            CornerRelief::None => None,
            CornerRelief::Dogbone => Some(Self::Dogbone),
            CornerRelief::TBone => Some(Self::TBone),
        }
    }
}

/// A corner of the cleared region, with everything the styles need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerGeometry {
    /// `C`, the corner, mm.
    pub corner: Point,
    /// Unit direction from `C` along the incoming edge.
    pub edge_a: Vec2,
    /// Unit direction from `C` along the outgoing edge.
    pub edge_b: Vec2,
    /// Length of the incoming edge, mm.
    pub length_a_mm: f64,
    /// Length of the outgoing edge, mm.
    pub length_b_mm: f64,
    /// Unit bisector of the cleared-side wedge, pointing into it.
    pub bisector: Vec2,
    /// The cleared-side interior angle at `C`, radians, below π.
    pub interior_angle_rad: f64,
}

/// One closed boundary loop of the region an operation clears.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReliefLoop {
    /// The loop, either winding.
    pub polyline: Polyline,
    /// True when the cleared material is inside the loop (a Pocket's
    /// outline, an inside Profile's outline), false when outside (a
    /// Pocket's island, an outside Profile's part).
    pub cleared_inside: bool,
    /// The shape warnings and notch blocks name.
    pub shape: ShapeRef,
}

/// The cleared loops of one region and the tool-centre rings the
/// operation cut along them, where the descend points are found.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Site {
    /// The cleared loops.
    pub loops: Vec<ReliefLoop>,
    /// The tool-centre rings: a Pocket's tool-centre region's outlines, a
    /// Profile's tool-centre contours.
    pub walls: Vec<Polyline>,
}

/// A corner to relieve.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReliefCorner {
    /// The corner.
    pub geometry: CornerGeometry,
    /// `P`, where the tool-centre ring crosses the bisector: the notch
    /// plunges here, where the main path already cut.
    pub descend: Point,
    /// `E`, where the excursion ends, the cutter just touching `C`.
    pub end: Point,
    /// The shape of the loop the corner is on.
    pub shape: ShapeRef,
}

/// The edge a T-bone slots into: the longer one (VCarve's placement); on
/// a tie the more X-parallel, then the one pointing more to `+x`, then
/// more to `+y`, so the choice never depends on where the loop starts.
fn t_bone_edge(g: &CornerGeometry) -> Vec2 {
    let (a, b) = (g.edge_a, g.edge_b);
    if (g.length_a_mm - g.length_b_mm).abs() > RELIEF_POSITION_TOLERANCE_MM {
        return if g.length_a_mm > g.length_b_mm { a } else { b };
    }
    if (a.y.abs() - b.y.abs()).abs() > UNIT_EPSILON {
        return if a.y.abs() < b.y.abs() { a } else { b };
    }
    if (a.x - b.x).abs() > UNIT_EPSILON {
        return if a.x > b.x { a } else { b };
    }
    if a.y >= b.y { a } else { b }
}

/// Where the excursion ends, `distance_mm` from the corner: along the
/// bisector for a dogbone, along the T-bone edge for a T-bone. The
/// distance is the cutter radius in production; it is a parameter so the
/// tests can show that ending short fails.
pub(crate) fn excursion_end(geometry: &CornerGeometry, style: Style, distance_mm: f64) -> Point {
    let direction = match style {
        Style::Dogbone => geometry.bisector,
        Style::TBone => t_bone_edge(geometry),
    };
    geometry.corner + direction * distance_mm
}

/// `l` wound so its cleared side is on the left of travel.
fn canonical(l: &ReliefLoop) -> Polyline {
    let ccw = l.polyline.signed_area_mm2() > 0.0;
    if ccw == l.cleared_inside {
        l.polyline.clone()
    } else {
        l.polyline.reversed()
    }
}

/// The corners of `polyline` (closed, cleared on the left): every vertex
/// where the path turns left, toward the cleared side, by more than
/// [`RELIEF_MIN_TURN_RAD`], measured between the tangents of the segments
/// meeting there.
fn corners_of(polyline: &Polyline) -> Vec<CornerGeometry> {
    let segments: Vec<Segment> = polyline
        .segments()
        .filter(|s| s.length_mm() > EPS_MM)
        .collect();
    let n = segments.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    for (k, next) in segments.iter().enumerate() {
        let Some(prev) = segments.get((k + n - 1) % n) else {
            continue;
        };
        let (Some((_, incoming)), Some((_, outgoing))) =
            (prev.point_at(prev.length_mm()), next.point_at(0.0))
        else {
            continue;
        };
        // The edges as directions away from the corner.
        let edge_a = -incoming;
        let edge_b = outgoing;
        let turn = (-edge_a.cross(edge_b)).atan2(-edge_a.dot(edge_b));
        if turn.is_nan() || turn <= RELIEF_MIN_TURN_RAD {
            continue;
        }
        let Some(bisector) = Vec2::new(edge_a.x + edge_b.x, edge_a.y + edge_b.y).normalized()
        else {
            continue;
        };
        out.push(CornerGeometry {
            corner: next.start,
            edge_a,
            edge_b,
            length_a_mm: prev.length_mm(),
            length_b_mm: next.length_mm(),
            bisector,
            interior_angle_rad: PI - turn,
        });
    }
    out
}

/// The straight pieces of `walls`, arcs flattened within `tolerance_mm`.
fn wall_segments(walls: &[Polyline], tolerance_mm: f64) -> Vec<(Point, Point)> {
    walls
        .iter()
        .flat_map(|w| {
            w.with_arcs_flattened(tolerance_mm)
                .segments()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>()
        })
        .filter(|(a, b)| a.distance_to(*b) > EPS_MM)
        .collect()
}

/// March out from the corner along the bisector to the first crossing of
/// the tool-centre ring, at `t ∈ [r, r/sin(θ/2)]`: the exact miter is the
/// furthest it can be (a fillet or a round join only pulls it nearer), so
/// the window is one-sided. `None` when the ring does not cross there.
fn locate_descend(
    g: &CornerGeometry,
    segments: &[(Point, Point)],
    radius_mm: f64,
) -> Option<Point> {
    let direction = g.bisector;
    let min_t = radius_mm - RELIEF_POSITION_TOLERANCE_MM;
    let max_t = radius_mm / (g.interior_angle_rad / 2.0).sin() + RELIEF_POSITION_TOLERANCE_MM;
    let mut best = f64::INFINITY;
    for &(a, b) in segments {
        let edge = b - a;
        let denominator = direction.cross(edge);
        if denominator.abs() < UNIT_EPSILON {
            continue;
        }
        let to_start = a - g.corner;
        let t = to_start.cross(edge) / denominator;
        if t.is_nan() || t < min_t || t > max_t || t >= best {
            continue;
        }
        let along = to_start.cross(direction) / denominator;
        let slack = RELIEF_POSITION_TOLERANCE_MM / edge.length().max(EPS_MM);
        if along < -slack || along > 1.0 + slack {
            continue;
        }
        best = t;
    }
    best.is_finite().then(|| g.corner + direction * best)
}

/// [`Warning::ReliefCornerNotCut`] for corner `g` of `shape`.
fn not_cut_warning(op: &OperationId, shape: &ShapeRef, g: &CornerGeometry) -> Warning {
    Warning::ReliefCornerNotCut {
        op: op.clone(),
        shape: shape.clone(),
        x_mm: g.corner.x,
        y_mm: g.corner.y,
    }
}

/// Every corner of `site` that qualifies for relief, with its descend
/// point and end point, and a [`Warning::ReliefCornerTooTight`] for
/// each corner rejected: an adjacent edge under `2r` (a T-bone's slot and
/// its cutter need that much along the wall, and arc-like runs of short
/// edges fail it), or no tool-centre ring crossing within reach. A relief
/// that is not where the geometry says is worse than none, so corners are
/// rejected rather than approximated.
pub(crate) fn collect(
    op: &OperationId,
    style: Style,
    radius_mm: f64,
    site: &Site,
    tolerance_mm: f64,
) -> (Vec<ReliefCorner>, Vec<Warning>) {
    let mut corners = Vec::new();
    let mut warnings = Vec::new();
    if !(radius_mm.is_finite() && radius_mm > 0.0) {
        return (corners, warnings);
    }
    let segments = wall_segments(&site.walls, tolerance_mm);
    let min_edge_mm = 2.0 * radius_mm - EPS_MM;
    for l in &site.loops {
        for g in corners_of(&canonical(l)) {
            let too_tight = |reason| Warning::ReliefCornerTooTight {
                op: op.clone(),
                shape: l.shape.clone(),
                x_mm: g.corner.x,
                y_mm: g.corner.y,
                reason,
            };
            if g.length_a_mm < min_edge_mm || g.length_b_mm < min_edge_mm {
                warnings.push(too_tight(ReliefCornerReason::ShortEdge));
                continue;
            }
            match locate_descend(&g, &segments, radius_mm) {
                Some(descend) => corners.push(ReliefCorner {
                    geometry: g,
                    descend,
                    end: excursion_end(&g, style, radius_mm),
                    shape: l.shape.clone(),
                }),
                None => warnings.push(too_tight(ReliefCornerReason::NoWallPath)),
            }
        }
    }
    (corners, warnings)
}

/// The guard: whether a cutting move of `toolpath` with both ends at or
/// below `max_z_mm` passes within `tolerance_mm` of `point` (arcs as
/// arcs). A flat cutter there at that depth has opened the whole column
/// above, so a notch may plunge at `point`. Rapids never count; a plunge
/// from above the level does not either.
pub(crate) fn main_path_cuts_at(
    toolpath: &Toolpath,
    point: Point,
    max_z_mm: f64,
    tolerance_mm: f64,
) -> bool {
    moves_cut_at(None, &toolpath.moves, point, max_z_mm, tolerance_mm)
}

/// [`main_path_cuts_at`] for one block's cuts, from where its descent
/// leaves the tool (its entry at its pass Z, or the tab a lap starts on).
fn block_cuts_at(block: &CutBlock, point: Point, max_z_mm: f64, tolerance_mm: f64) -> bool {
    let start = block
        .descent
        .last()
        .map_or(Point3::new(block.entry.x, block.entry.y, block.z_mm), |m| {
            m.to
        });
    moves_cut_at(Some(start), &block.cuts, point, max_z_mm, tolerance_mm)
}

/// The guard on `moves`, the tool at `start` before the first.
fn moves_cut_at(
    start: Option<Point3>,
    moves: &[Move],
    point: Point,
    max_z_mm: f64,
    tolerance_mm: f64,
) -> bool {
    let z_limit = max_z_mm + RELIEF_Z_TOLERANCE_MM;
    let mut position = start;
    for mv in moves {
        if let Some(from) = position
            && mv.is_cut()
            && from.z <= z_limit
            && mv.to.z <= z_limit
        {
            let path = Segment {
                start: from.xy(),
                end: mv.to.xy(),
                bulge: move_bulge(from.xy(), mv),
            };
            if point_segment_distance_mm(path, point) <= tolerance_mm {
                return true;
            }
        }
        position = Some(mv.to);
    }
    false
}

/// `corners` in nearest-neighbour order from `start`, by descend point,
/// ties in the order given: distances within
/// [`crate::sequence::NEAREST_TIE_MM`] are a tie (#215).
fn nearest_neighbour(mut corners: Vec<ReliefCorner>, start: Point) -> Vec<ReliefCorner> {
    let mut ordered = Vec::with_capacity(corners.len());
    let mut from = start;
    while !corners.is_empty() {
        let next = nearest_of(
            corners
                .iter()
                .enumerate()
                .map(|(i, c)| (i, from.distance_to(c.descend))),
        )
        .unwrap_or(0);
        let corner = corners.remove(next);
        from = corner.descend;
        ordered.push(corner);
    }
    ordered
}

/// The relief pass of one operation: what [`Setup::plan`] works out.
#[derive(Debug, Clone)]
pub(crate) struct Relief {
    /// The operation, for warnings.
    pub op: OperationId,
    /// The style.
    pub style: Style,
    /// The tool's radius, mm.
    pub radius_mm: f64,
    /// The relief levels, top first, at the tool's step-down.
    pub levels: Vec<f64>,
    /// Absolute safe Z, mm.
    pub safe_z_mm: f64,
    /// The stock top, mm.
    pub stock_top_mm: f64,
    /// The tool's feed and plunge rates.
    pub rates: LapRates,
    /// The project's flatten tolerance, mm.
    pub tolerance_mm: f64,
}

/// What a generator hands over to plan its relief pass.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Setup<'a> {
    /// The operation.
    pub op: &'a OperationId,
    /// The relief it asks for.
    pub relief: CornerRelief,
    /// Its tool.
    pub tool: &'a Tool,
    /// The stock thickness, mm.
    pub thickness_mm: f64,
    /// The operation's depth, mm.
    pub depth_mm: f64,
    /// Its through-cut allowance, mm.
    pub through_cut_allowance_mm: f64,
    /// Absolute safe Z, mm.
    pub safe_z_mm: f64,
    /// The tool's rates.
    pub rates: LapRates,
    /// The flatten tolerance, mm.
    pub tolerance_mm: f64,
}

impl Setup<'_> {
    /// The relief pass, or `None` when the operation asks for none. Its
    /// levels step down at the tool's step-down, never the operation's
    /// (upstream: an operation's step-down may be a stale or coarse value
    /// for another purpose), to the operation's depth. A tool with no
    /// step-down skips the pass with [`Warning::ReliefSkipped`]
    /// rather than cutting each notch in one full-depth slot.
    ///
    /// # Errors
    ///
    /// As [`pass_z_mm`] for the tool's step-down.
    pub(crate) fn plan(&self, warnings: &mut Vec<Warning>) -> Result<Option<Relief>, CamError> {
        let Some(style) = Style::of(self.relief) else {
            return Ok(None);
        };
        let Some(step_down_mm) = self.tool.step_down_mm else {
            warnings.push(skipped(
                self.op,
                self.relief,
                ReliefSkippedReason::NoStepDown,
            ));
            return Ok(None);
        };
        let levels = pass_z_mm(
            self.op,
            self.thickness_mm,
            self.depth_mm,
            step_down_mm,
            self.through_cut_allowance_mm,
        )?;
        Ok(Some(Relief {
            op: self.op.clone(),
            style,
            radius_mm: self.tool.diameter_mm / 2.0,
            levels,
            safe_z_mm: self.safe_z_mm,
            stock_top_mm: self.thickness_mm,
            rates: self.rates,
            tolerance_mm: self.tolerance_mm,
        }))
    }
}

impl Relief {
    /// Collects the corners of `sites` and appends the relief pass to
    /// `toolpath`, the operation's main path; returns the corners cut.
    ///
    /// # Errors
    ///
    /// As [`sequence`], which never fails on the blocks built here.
    pub(crate) fn append(
        &self,
        sites: &[Site],
        toolpath: &mut Toolpath,
        warnings: &mut Vec<Warning>,
    ) -> Result<Vec<ReliefCorner>, CamError> {
        let mut corners = Vec::new();
        for site in sites {
            let (found, rejected) = collect(
                &self.op,
                self.style,
                self.radius_mm,
                site,
                self.tolerance_mm,
            );
            corners.extend(found);
            warnings.extend(rejected);
        }
        self.cut(corners, toolpath, warnings)
    }

    /// The pass itself (upstream `generateCornerReliefPass`): each corner
    /// whose descend point the main path cut at the deepest level, else
    /// [`Warning::ReliefCornerNotCut`]; nearest-neighbour from where
    /// the main path ended; per corner one [`CutBlock`] per level (plunge
    /// at the descend point, cut to the end, cut back), sequenced as one
    /// shape so the levels follow each other without a retract, and a
    /// retract between corners.
    fn cut(
        &self,
        corners: Vec<ReliefCorner>,
        toolpath: &mut Toolpath,
        warnings: &mut Vec<Warning>,
    ) -> Result<Vec<ReliefCorner>, CamError> {
        let Some(deepest) = self.levels.iter().copied().reduce(f64::min) else {
            return Ok(Vec::new());
        };
        // The descend point lies on the wall ring flattened within the
        // tolerance, so the path the main pass cut may be that far off it.
        let guard_mm = RELIEF_POSITION_TOLERANCE_MM + self.tolerance_mm;
        let mut cut = Vec::with_capacity(corners.len());
        for corner in corners {
            if main_path_cuts_at(toolpath, corner.descend, deepest, guard_mm) {
                cut.push(corner);
            } else {
                warnings.push(not_cut_warning(&self.op, &corner.shape, &corner.geometry));
            }
        }
        let Some(start) = toolpath
            .moves
            .last()
            .map(|m| m.to.xy())
            .or_else(|| cut.first().map(|c| c.descend))
        else {
            return Ok(cut);
        };
        let ordered = nearest_neighbour(cut, start);
        for corner in &ordered {
            let input = SequenceInput {
                blocks: self.blocks(corner),
                order: MachiningOrder::FullDepthFirst,
                containment: &[],
                safe_z_mm: self.safe_z_mm,
                stock_top_mm: self.stock_top_mm,
                tool_diameter_mm: 2.0 * self.radius_mm,
                stepover_mm: 0.0,
                feed_mm_min: self.rates.feed_mm_min,
                plunge_mm_min: self.rates.plunge_mm_min,
                // #227: the notch's rapid down stops just above the floor
                // the main path cut at its descend point.
                cut_before: &toolpath.moves,
            };
            let notch = sequence(&input)?;
            toolpath.moves.extend(notch.moves);
        }
        Ok(ordered)
    }

    /// One block per level for `corner`: a straight plunge at the descend
    /// point from safe Z or the level above, out to the end and back at
    /// the feed rate.
    fn blocks(&self, corner: &ReliefCorner) -> Vec<CutBlock> {
        let (p, e) = (corner.descend, corner.end);
        let feed = self.rates.feed_mm_min;
        self.levels
            .iter()
            .enumerate()
            .map(|(pass, &z_mm)| CutBlock {
                shape: corner.shape.clone(),
                pass,
                z_mm,
                descent_from: p,
                descent_from_z_mm: pass
                    .checked_sub(1)
                    .and_then(|k| self.levels.get(k).copied())
                    .unwrap_or(self.safe_z_mm),
                descent: Vec::new(),
                entry: p,
                cuts: vec![
                    Move::linear(Point3::new(e.x, e.y, z_mm), feed),
                    Move::linear(Point3::new(p.x, p.y, z_mm), feed),
                ],
                exit: p,
                frees: false,
                cleared: None,
                group: 0,
            })
            .collect()
    }

    /// A Profile's relief pass, cut among its shapes' own passes (#189):
    /// the notch blocks to hand to [`sequence`] with `blocks`, the main
    /// blocks. A corner is cut when its own shape's path cuts its descend
    /// point at the deepest level (else [`Warning::ReliefCornerNotCut`]).
    /// Each level goes to the unit of the shape's first pass at or below
    /// it, which has opened the column at the descend point by then, and
    /// is cut after that pass's lap. The levels of a freeing pass are cut
    /// from inside its lap instead (#193, [`Relief::splice`]), where the
    /// lap first reaches the descend point at depth, so no notch plunges
    /// into the skin and nothing is cut after the lap closes. A shape's
    /// corners are taken nearest-neighbour from where its first block
    /// ends, the same order at every pass.
    fn interleave(
        &self,
        sites: &[Site],
        blocks: &mut [CutBlock],
        warnings: &mut Vec<Warning>,
    ) -> Vec<CutBlock> {
        let mut corners = Vec::new();
        for site in sites {
            let (found, rejected) = collect(
                &self.op,
                self.style,
                self.radius_mm,
                site,
                self.tolerance_mm,
            );
            corners.extend(found);
            warnings.extend(rejected);
        }
        let Some(deepest) = self.levels.iter().copied().reduce(f64::min) else {
            return Vec::new();
        };
        let guard_mm = RELIEF_POSITION_TOLERANCE_MM + self.tolerance_mm;
        let mut cut = Vec::with_capacity(corners.len());
        for corner in corners {
            let own_path_cuts = blocks
                .iter()
                .filter(|b| b.shape == corner.shape)
                .any(|b| block_cuts_at(b, corner.descend, deepest, guard_mm));
            if own_path_cuts {
                cut.push(corner);
            } else {
                warnings.push(not_cut_warning(&self.op, &corner.shape, &corner.geometry));
            }
        }
        // Each corner's levels, grouped by the pass whose unit cuts them.
        let mut planned: Vec<(ReliefCorner, usize, Vec<f64>)> = Vec::new();
        let mut shapes: Vec<&ShapeRef> = Vec::new();
        for block in blocks.iter() {
            if !shapes.contains(&&block.shape) {
                shapes.push(&block.shape);
            }
        }
        for shape in shapes {
            let own: Vec<&CutBlock> = blocks.iter().filter(|b| &b.shape == shape).collect();
            let mine: Vec<ReliefCorner> =
                cut.iter().filter(|c| &c.shape == shape).cloned().collect();
            let Some(first) = own.first() else {
                continue;
            };
            // The shape's passes, top first, with their Zs.
            let mut passes: Vec<(usize, f64)> = own.iter().map(|b| (b.pass, b.z_mm)).collect();
            passes.sort_by_key(|(pass, _)| *pass);
            passes.dedup_by_key(|(pass, _)| *pass);
            for corner in nearest_neighbour(mine, first.exit) {
                let mut groups: Vec<(usize, Vec<f64>)> = Vec::new();
                for &level in &self.levels {
                    let Some(pass) = passes
                        .iter()
                        .find(|(_, z)| *z <= level + RELIEF_Z_TOLERANCE_MM)
                        .or(passes.last())
                        .map(|(pass, _)| *pass)
                    else {
                        continue;
                    };
                    match groups.last_mut() {
                        Some((at, levels)) if *at == pass => levels.push(level),
                        _ => groups.push((pass, vec![level])),
                    }
                }
                planned.extend(
                    groups
                        .into_iter()
                        .map(|(pass, levels)| (corner.clone(), pass, levels)),
                );
            }
        }
        let mut notches = Vec::new();
        for (corner, pass, levels) in planned {
            // The freeing lap of the corner's shape at this pass that
            // reaches the descend point at depth, if any (#193).
            let freeing = blocks.iter_mut().find(|b| {
                b.frees
                    && b.shape == corner.shape
                    && b.pass == pass
                    && block_cuts_at(b, corner.descend, deepest, guard_mm)
            });
            if let Some(block) = freeing
                && self.splice(block, &corner, &levels, deepest, guard_mm)
            {
                continue;
            }
            notches.extend(self.notch_block(&corner, pass, &levels));
        }
        notches
    }

    /// Cuts `levels` (top first) of `corner`'s notch from inside `block`,
    /// a lap that frees its piece (#193). Where the lap first reaches the
    /// descend point `P` at or below `deepest` (within `guard_mm`), the
    /// tool goes to `P`, rises in the column the lap has just opened to the
    /// first level, cuts out to the end and back at each level at the feed
    /// rate, stepping down at `P` between levels, returns to the lap's Z
    /// and point, and the lap carries on. A lap move that passes `P` in its
    /// middle is split there. Every Z change is at `P`, in the open column,
    /// at the plunge rate, so nothing plunges into the skin, and the rest
    /// of the lap still holds the piece while the notch is cut. False, with
    /// `block` unchanged, when the lap never reaches `P` at depth.
    fn splice(
        &self,
        block: &mut CutBlock,
        corner: &ReliefCorner,
        levels: &[f64],
        deepest: f64,
        guard_mm: f64,
    ) -> bool {
        let z_limit = deepest + RELIEF_Z_TOLERANCE_MM;
        let p = corner.descend;
        let mut position = block
            .descent
            .last()
            .map_or(Point3::new(block.entry.x, block.entry.y, block.z_mm), |m| {
                m.to
            });
        // Where to insert, the tool's position there, and the two halves
        // of the move it splits, if it splits one.
        let mut found: Option<(usize, Point3, Option<[Move; 2]>)> = None;
        for (i, mv) in block.cuts.iter().enumerate() {
            if position.z <= z_limit && position.xy().distance_to(p) <= guard_mm {
                found = Some((i, position, None));
                break;
            }
            if mv.is_cut() && position.z <= z_limit && mv.to.z <= z_limit {
                let path = Segment {
                    start: position.xy(),
                    end: mv.to.xy(),
                    bulge: move_bulge(position.xy(), mv),
                };
                if point_segment_distance_mm(path, p) <= guard_mm {
                    found = Some(if mv.to.xy().distance_to(p) <= guard_mm {
                        (i + 1, mv.to, None)
                    } else {
                        let at = split_point(position, mv, p);
                        (i, at, Some([Move { to: at, ..*mv }, *mv]))
                    });
                    break;
                }
            }
            position = mv.to;
        }
        let Some((index, from, split)) = found else {
            return false;
        };
        let excursion = self.excursion(corner, levels, from);
        let tail = block.cuts.split_off(index);
        match split {
            Some([first, rest]) => {
                block.cuts.push(first);
                block.cuts.extend(excursion);
                block.cuts.push(rest);
                block.cuts.extend(tail.into_iter().skip(1));
            }
            None => {
                block.cuts.extend(excursion);
                block.cuts.extend(tail);
            }
        }
        true
    }

    /// The notch moves of [`Relief::splice`], from and back to `from`, the
    /// tool on the lap at (or within the guard of) the descend point.
    fn excursion(&self, corner: &ReliefCorner, levels: &[f64], from: Point3) -> Vec<Move> {
        let (p, e) = (corner.descend, corner.end);
        let (feed, plunge) = (self.rates.feed_mm_min, self.rates.plunge_mm_min);
        let at = |point: Point, z_mm: f64| Point3::new(point.x, point.y, z_mm);
        let off_p = from.xy().distance_to(p) > EPS_MM;
        let mut moves = Vec::with_capacity(3 * levels.len() + 3);
        if off_p {
            moves.push(Move::linear(at(p, from.z), feed));
        }
        let mut z_mm = from.z;
        for &level in levels {
            if (level - z_mm).abs() > EPS_MM {
                moves.push(Move::linear(at(p, level), plunge));
                z_mm = level;
            }
            moves.push(Move::linear(at(e, z_mm), feed));
            moves.push(Move::linear(at(p, z_mm), feed));
        }
        if (z_mm - from.z).abs() > EPS_MM {
            moves.push(Move::linear(at(p, from.z), plunge));
        }
        if off_p {
            moves.push(Move::linear(from, feed));
        }
        moves
    }

    /// The notch block for `corner` in its shape's pass `pass`, cutting
    /// `levels` (top first): from safe Z a plunge at the descend point to
    /// the first level, out to the end and back at the feed rate, then the
    /// same at each further level, plunging straight from the one above.
    /// Its pass Z is the deepest level; its descent ends at the first.
    fn notch_block(&self, corner: &ReliefCorner, pass: usize, levels: &[f64]) -> Option<CutBlock> {
        let (p, e) = (corner.descend, corner.end);
        let (feed, plunge) = (self.rates.feed_mm_min, self.rates.plunge_mm_min);
        let at = |point: Point, z_mm: f64| Point3::new(point.x, point.y, z_mm);
        let (&first, rest) = levels.split_first()?;
        let mut cuts = vec![
            Move::linear(at(e, first), feed),
            Move::linear(at(p, first), feed),
        ];
        for &z_mm in rest {
            cuts.push(Move::linear(at(p, z_mm), plunge));
            cuts.push(Move::linear(at(e, z_mm), feed));
            cuts.push(Move::linear(at(p, z_mm), feed));
        }
        Some(CutBlock {
            shape: corner.shape.clone(),
            pass,
            z_mm: levels.iter().copied().fold(first, f64::min),
            descent_from: p,
            descent_from_z_mm: self.safe_z_mm,
            descent: vec![Move::linear(at(p, first), plunge)],
            entry: p,
            cuts,
            exit: p,
            frees: false,
            cleared: None,
            group: 0,
        })
    }
}

/// The point of flat move `mv`, from `from`, nearest `p`: on a line its
/// foot, on an arc the point at `p`'s angle about the centre.
fn split_point(from: Point3, mv: &Move, p: Point) -> Point3 {
    let point = match mv.kind {
        MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
            let radius_mm = from.xy().distance_to(centre);
            (p - centre)
                .normalized()
                .map_or(from.xy(), |u| centre + u * radius_mm)
        }
        _ => {
            let (a, b) = (from.xy(), mv.to.xy());
            let ab = b - a;
            let length2 = ab.dot(ab);
            if length2 <= EPS_MM * EPS_MM {
                a
            } else {
                a + ab * ((p - a).dot(ab) / length2).clamp(0.0, 1.0)
            }
        }
    };
    Point3::new(point.x, point.y, mv.to.z)
}

/// S9's notch rule: the area beyond the cleared region a notch at
/// `corner` may remove. It is the triangle of `C + b·r` (where the cutter
/// touches the corner along the bisector), the miter point `C +
/// b·r/sin(θ/2)` (the furthest a descend point can be) and the end point,
/// grown by the radius. Every notch the pass cuts sweeps inside it: its
/// descend point lies on the bisector between the first two, so its
/// excursion lies in the triangle. The generator's footprint check allows
/// these lobes on top of the operation's own area.
pub(crate) fn lobe(corner: &ReliefCorner, radius_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let g = &corner.geometry;
    let near = g.corner + g.bisector * radius_mm;
    let miter = g.corner + g.bisector * (radius_mm / (g.interior_angle_rad / 2.0).sin());
    let pieces: Vec<Region> = [(near, miter), (miter, corner.end), (corner.end, near)]
        .into_iter()
        .filter_map(|(a, b)| capsule(a, b, radius_mm))
        .collect();
    if pieces.is_empty() {
        return Vec::new();
    }
    union(&pieces, &[], tolerance_mm)
}

/// The relief site of one Pocket region: the area it clears (`cleared`,
/// the region less the allowance) and its tool-centre region (`centre`,
/// restricted for rest machining), all named after the pocket's outline.
pub(crate) fn pocket_site(cleared: &[Region], centre: &[Region], shape: &ShapeRef) -> Site {
    let outlines = |r: &Region| {
        std::iter::once(r.outer())
            .chain(r.holes())
            .map(|c| c.polyline().clone())
            .collect::<Vec<_>>()
    };
    let loops = cleared
        .iter()
        .flat_map(|r| {
            std::iter::once((r.outer(), true)).chain(r.holes().iter().map(|h| (h, false)))
        })
        .map(|(c, cleared_inside)| ReliefLoop {
            polyline: c.polyline().clone(),
            cleared_inside,
            shape: shape.clone(),
        })
        .collect();
    Site {
        loops,
        walls: centre.iter().flat_map(outlines).collect(),
    }
}

/// A Profile's regions and tool-centre paths, as `generate_profile` built
/// them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProfileGeometry<'a> {
    /// The nested target regions.
    pub regions: &'a [Region],
    /// Every target shape and its contour.
    pub shapes: &'a [(ShapeRef, Contour)],
    /// Every tool-centre path and the shape it cuts.
    pub paths: &'a [(Polyline, &'a ShapeRef)],
    /// The wall each path cuts and the index of its region.
    pub walls: &'a [(Wall, usize)],
}

/// The relief sites of a Profile on `side`: per region, its outline and
/// holes with the side the kerf clears (Inside: inside the outline and
/// outside the holes; Outside: the reverse), and the region's tool-centre
/// paths. On line has none.
fn profile_sites(side: Side, geometry: &ProfileGeometry<'_>) -> Vec<Site> {
    if side == Side::OnLine {
        return Vec::new();
    }
    let inside = side == Side::Inside;
    geometry
        .regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let mut loops = Vec::new();
            let contours = std::iter::once((region.outer(), inside))
                .chain(region.holes().iter().map(|h| (h, !inside)));
            for (contour, cleared_inside) in contours {
                if let Some(shape) = shape_of(geometry.shapes, contour) {
                    loops.push(ReliefLoop {
                        polyline: contour.polyline().clone(),
                        cleared_inside,
                        shape: shape.clone(),
                    });
                }
            }
            let walls = geometry
                .paths
                .iter()
                .zip(geometry.walls)
                .filter(|(_, (_, r))| *r == index)
                .map(|((path, _), _)| path.clone())
                .collect();
            Site { loops, walls }
        })
        .collect()
}

/// A Profile's relief notches (Inside: the hole's corners; Outside: the
/// part's concave corners), as blocks to sequence with `blocks`, the
/// Profile's main blocks, among the shapes' own passes (#189; see
/// [`Relief::interleave`]). On line has no waste side for a notch:
/// [`Warning::ReliefSkipped`].
///
/// # Errors
///
/// As [`Setup::plan`].
pub(crate) fn profile_notches(
    project: &Project,
    op: &ProfileOp,
    tool: &Tool,
    geometry: &ProfileGeometry<'_>,
    blocks: &mut [CutBlock],
    warnings: &mut Vec<Warning>,
) -> Result<Vec<CutBlock>, CamError> {
    if op.corner_relief == CornerRelief::None {
        return Ok(Vec::new());
    }
    if op.side == Side::OnLine {
        warnings.push(skipped(
            &op.id,
            op.corner_relief,
            ReliefSkippedReason::OnLine,
        ));
        return Ok(Vec::new());
    }
    let (Some(thickness_mm), Some(depth_mm)) = (project.stock.thickness_mm, op.depth_mm) else {
        return Ok(Vec::new());
    };
    let setup = Setup {
        op: &op.id,
        relief: op.corner_relief,
        tool,
        thickness_mm,
        depth_mm,
        through_cut_allowance_mm: op.through_cut_allowance_mm,
        safe_z_mm: thickness_mm + op.safe_z_mm,
        rates: rates_of(tool)?,
        tolerance_mm: project.flatten_tolerance_mm,
    };
    Ok(match setup.plan(warnings)? {
        Some(relief) => relief.interleave(&profile_sites(op.side, geometry), blocks, warnings),
        None => Vec::new(),
    })
}

/// A [`Warning::ReliefSkipped`] for `op`, which asked for `relief`.
pub(crate) fn skipped(
    op: &OperationId,
    relief: CornerRelief,
    reason: ReliefSkippedReason,
) -> Warning {
    Warning::ReliefSkipped {
        op: op.clone(),
        requested: relief,
        reason,
    }
}

#[cfg(test)]
mod tests {
    //! Ported from PureCutCNC's `src/engine/toolpaths/cornerRelief.test.ts`
    //! (Copyright 2026 Franja (Frank) Povazanj, Apache-2.0), re-derived for
    //! Y-up, plus the DreamCarve acceptance 29 and S9 tests.
    //!
    //! As upstream, the central assertion is coverage of the emitted
    //! motion: the swept capsules of the cutting moves must contain the
    //! corner wedge, and the same model with the excursion ending short
    //! must not.

    use super::*;
    use crate::footprint::footprint;
    use crate::pocket::generate_pocket;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{
        Arc, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point3, Segment, Transform, Vertex,
        assert_approx_eq, difference, nest, point_segment_distance_mm,
    };
    use dc_project::test_support::sample_project;
    use dc_project::{
        ClearanceStrategy, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, Operation, PocketOp, ProfileOp, Project, Stock, TabPlacement,
        TabShape, TabSpec, ToolId, ToolKind, WallPass,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

    /// Every drawn shape is placed this far from the stock origin in X
    /// and Y, so outside cuts stay on the stock.
    const O: f64 = 20.0;
    const THICKNESS_MM: f64 = 6.0;
    /// The 4 mm test tool's radius.
    const R: f64 = 2.0;
    const TEST_ANGLES: [f64; 7] = [150.0, 120.0, 90.0, 75.0, 60.0, 45.0, 30.0];

    fn approx(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn at(x: f64, y: f64) -> Point {
        Point::new(x + O, y + O)
    }

    fn shape() -> ShapeRef {
        ShapeRef {
            instance: InstanceId::new("i1"),
            instance_name: "Shape".into(),
            profile: 0,
        }
    }

    fn op_id() -> OperationId {
        OperationId::new("o1")
    }

    // ── Coverage model ──────────────────────────────────────────────────

    /// One cutting move: its path in XY and the Z at its two ends.
    type Cut = (Segment, f64, f64);

    /// The bulge of `mv` from `from`: 0 for a line.
    fn bulge_of(from: Point, mv: &Move) -> f64 {
        let (centre, ccw) = match mv.kind {
            MoveKind::ArcCcw { centre } => (centre, true),
            MoveKind::ArcCw { centre } => (centre, false),
            _ => return 0.0,
        };
        (Arc::from_ends(from, mv.to.xy(), centre, ccw).sweep_rad / 4.0).tan()
    }

    /// The cutting moves of `moves`, from `start`.
    fn cuts_from(start: Point3, moves: &[Move]) -> Vec<Cut> {
        let mut pos = start;
        let mut out = Vec::new();
        for mv in moves {
            if mv.is_cut() {
                let segment = Segment {
                    start: pos.xy(),
                    end: mv.to.xy(),
                    bulge: bulge_of(pos.xy(), mv),
                };
                out.push((segment, pos.z, mv.to.z));
            }
            pos = mv.to;
        }
        out
    }

    fn cuts(toolpath: &Toolpath) -> Vec<Cut> {
        match toolpath.moves.first() {
            Some(first) => cuts_from(first.to, &toolpath.moves),
            None => Vec::new(),
        }
    }

    fn line(a: Point, b: Point, z: f64) -> Cut {
        (
            Segment {
                start: a,
                end: b,
                bulge: 0.0,
            },
            z,
            z,
        )
    }

    /// True when a flat cutter of `radius` sweeping `cuts` removes the
    /// material at `point` at Z `z`: a cut at or below `z` clears the
    /// column above it too.
    fn swept_covers(point: Point, cuts: &[Cut], radius: f64, z: f64) -> bool {
        let tolerance = radius * 1e-6;
        cuts.iter().any(|(segment, z0, z1)| {
            *z0 <= z + 1e-9
                && *z1 <= z + 1e-9
                && point_segment_distance_mm(*segment, point) <= radius + tolerance
        })
    }

    /// Points strictly inside the corner wedge within `radius` of the
    /// corner: what a mating square part needs removed.
    fn wedge_samples(geometry: &CornerGeometry, radius: f64) -> Vec<Point> {
        let base = geometry.edge_a.y.atan2(geometry.edge_a.x);
        let towards_b = geometry.edge_a.cross(geometry.bisector).signum();
        let mut samples = Vec::new();
        let mut fraction = 0.02;
        while fraction <= 0.98 {
            let angle = base + towards_b * geometry.interior_angle_rad * fraction;
            let mut depth = 0.1;
            while depth <= 1.0001 {
                samples
                    .push(geometry.corner + Vec2::new(angle.cos(), angle.sin()) * (radius * depth));
                depth += 0.1;
            }
            fraction += 0.06;
        }
        samples
    }

    /// A corner at the origin opening symmetrically about +X, and the
    /// wall contour a miter offset by `radius` gives: two legs meeting at
    /// the descend point.
    fn synthetic_corner(angle_deg: f64, radius: f64) -> (CornerGeometry, Vec<Cut>, Point) {
        let half = angle_deg.to_radians() / 2.0;
        let edge_a = Vec2::new(half.cos(), half.sin());
        let edge_b = Vec2::new(half.cos(), -half.sin());
        let geometry = CornerGeometry {
            corner: Point::new(0.0, 0.0),
            edge_a,
            edge_b,
            length_a_mm: 40.0,
            length_b_mm: 40.0,
            bisector: Vec2::new(1.0, 0.0),
            interior_angle_rad: 2.0 * half,
        };
        let descend = Point::new(radius / half.sin(), 0.0);
        let walls = vec![
            line(descend + edge_a * 40.0, descend, 0.0),
            line(descend, descend + edge_b * 40.0, 0.0),
        ];
        (geometry, walls, descend)
    }

    /// The right-angled corner at `corner` whose edges run along +X·`sx`
    /// and +Y·`sy`.
    fn right_corner(
        corner: Point,
        sx: f64,
        sy: f64,
        length_x: f64,
        length_y: f64,
    ) -> CornerGeometry {
        CornerGeometry {
            corner,
            edge_a: Vec2::new(sx, 0.0),
            edge_b: Vec2::new(0.0, sy),
            length_a_mm: length_x,
            length_b_mm: length_y,
            bisector: Vec2::new(sx * FRAC_1_SQRT_2, sy * FRAC_1_SQRT_2),
            interior_angle_rad: PI / 2.0,
        }
    }

    /// Distance from `p` to the line through `corner` along `edge`.
    fn off_line(edge: Vec2, p: Point, corner: Point) -> f64 {
        edge.cross(p - corner).abs()
    }

    // ── Project fixtures ────────────────────────────────────────────────

    fn tool(id: &str, diameter_mm: f64, step_down_mm: Option<f64>) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: format!("{diameter_mm} mm end mill"),
            number: None,
            cutter: ToolKind::EndMill,
            diameter_mm,
            flutes: 2,
            feed_mm_min: Some(800.0),
            plunge_mm_min: Some(300.0),
            rpm: Some(18000),
            step_down_mm,
            step_over_fraction: Some(0.4),
        }
    }

    /// A closed polyline of lines through `points`, offset by (`O`, `O`).
    fn poly(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points
                .iter()
                .map(|&(x, y)| Vertex::line(x + O, y + O))
                .collect(),
            true,
        )
    }

    fn rect(width: f64, height: f64) -> Polyline {
        poly(&[(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)])
    }

    /// A `size` square with all four corners filleted by `radius`.
    fn filleted_square(size: f64, radius: f64) -> Polyline {
        let b = (PI / 8.0).tan();
        let v = |x: f64, y: f64, bulge: f64| Vertex::new(x + O, y + O, bulge);
        Polyline::new(
            vec![
                v(radius, 0.0, 0.0),
                v(size - radius, 0.0, b),
                v(size, radius, 0.0),
                v(size, size - radius, b),
                v(size - radius, size, 0.0),
                v(radius, size, b),
                v(0.0, size - radius, 0.0),
                v(0.0, radius, b),
            ],
            true,
        )
    }

    const L_PART: [(f64, f64); 6] = [
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 60.0),
        (30.0, 60.0),
        (30.0, 30.0),
        (0.0, 30.0),
    ];

    /// A 200 × 200 × 6 mm project with one instance of `profiles`.
    fn project(profiles: Vec<Polyline>, tools: Vec<Tool>) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 200.0,
            height_mm: 200.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Shape".into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::IDENTITY,
            name: "Shape".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        }];
        p.tools = tools;
        p.operations.clear();
        p
    }

    fn one_tool(profiles: Vec<Polyline>) -> Project {
        project(profiles, vec![tool("t1", 4.0, Some(2.0))])
    }

    /// An offset Pocket of the instance, 2 mm deep, with `relief`.
    fn pocket_op(relief: CornerRelief) -> PocketOp {
        PocketOp {
            id: op_id(),
            name: "Pocket".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: Some(2.0),
            step_down_mm: None,
            step_over_fraction: Some(0.4),
            direction: CutDirection::Climb,
            allowance_mm: 0.0,
            rest_of: None,
            entry: Entry::Plunge,
            corner_relief: relief,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// A Profile of the instance on `side`, 2 mm deep, with `relief`.
    fn profile_op(side: Side, relief: CornerRelief) -> ProfileOp {
        ProfileOp {
            id: op_id(),
            name: "Profile".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(2.0),
            step_down_mm: None,
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: None,
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: relief,
        }
    }

    /// Generates `op` with its relief and without it, checks the one with
    /// [`assert_safe`] and that the relief left the main path untouched,
    /// and returns the toolpath with relief, its warnings and the relief
    /// cutting moves alone.
    fn pocket_run(project: &Project, op: &PocketOp) -> (Toolpath, Vec<Warning>, Vec<Cut>) {
        let (with, warnings) = generate_pocket(project, op).unwrap();
        let none = PocketOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_pocket(project, &none).unwrap();
        assert_safe(&with, project, &Operation::Pocket(op.clone()));
        let relief = relief_cuts(&with, &without);
        (with, warnings, relief)
    }

    fn profile_run(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>, Vec<Cut>) {
        let (with, warnings) = generate_profile(project, op).unwrap();
        let none = ProfileOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_profile(project, &none).unwrap();
        assert_safe(&with, project, &Operation::Profile(op.clone()));
        let relief = interleaved_relief(&with, &without)
            .into_iter()
            .map(|(_, cut)| cut)
            .collect();
        (with, warnings, relief)
    }

    /// The XY cutting moves of `moves` from `start`, each with its index.
    fn xy_cuts(start: Point3, moves: &[Move]) -> Vec<(usize, Cut)> {
        let mut pos = start;
        let mut out = Vec::new();
        for (i, mv) in moves.iter().enumerate() {
            if mv.is_cut() && pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                let segment = Segment {
                    start: pos.xy(),
                    end: mv.to.xy(),
                    bulge: bulge_of(pos.xy(), mv),
                };
                out.push((i, (segment, pos.z, mv.to.z)));
            }
            pos = mv.to;
        }
        out
    }

    /// The XY cutting moves `with` (a Profile with relief) adds to
    /// `without` (the same Profile without), each with its index in
    /// `with`. A Profile's notches are cut among its passes (#189), so
    /// every cut of `without` must appear in `with` in the same order, and
    /// the rest are the notches.
    fn interleaved_relief(with: &Toolpath, without: &Toolpath) -> Vec<(usize, Cut)> {
        let first = |tp: &Toolpath| {
            tp.moves
                .first()
                .map_or(Point3::new(0.0, 0.0, 0.0), |m| m.to)
        };
        let main: Vec<Cut> = xy_cuts(first(without), &without.moves)
            .into_iter()
            .map(|(_, cut)| cut)
            .collect();
        let mut matched = 0;
        let mut relief = Vec::new();
        for (i, cut) in xy_cuts(first(with), &with.moves) {
            if main.get(matched) == Some(&cut) {
                matched += 1;
            } else {
                relief.push((i, cut));
            }
        }
        assert_eq!(
            matched,
            main.len(),
            "relief must leave the main path's cuts in place and in order"
        );
        relief
    }

    /// The XY cutting moves `with` adds after `without`, which must be
    /// its prefix: relief is appended after the main path.
    fn relief_cuts(with: &Toolpath, without: &Toolpath) -> Vec<Cut> {
        let n = without.moves.len();
        assert_eq!(
            with.moves.get(..n),
            Some(without.moves.as_slice()),
            "relief must leave the main path untouched"
        );
        let Some(last) = without.moves.last() else {
            return cuts(with);
        };
        cuts_from(last.to, &with.moves[n..])
            .into_iter()
            .filter(|(s, _, _)| s.start.distance_to(s.end) > EPS_MM)
            .collect()
    }

    /// The outward excursions: every other relief cut, from the first.
    fn outward(relief: &[Cut]) -> Vec<Segment> {
        relief.iter().step_by(2).map(|(s, _, _)| *s).collect()
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code()).collect()
    }

    fn relief_codes(warnings: &[Warning]) -> Vec<WarningCode> {
        codes(warnings)
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    WarningCode::ReliefCornerTooTight
                        | WarningCode::ReliefCornerNotCut
                        | WarningCode::ReliefSkipped
                )
            })
            .collect()
    }

    /// The four corners of a `w` × `h` rectangle at (`O`, `O`), each with
    /// its inward edge directions, the X-parallel edge first.
    fn rect_corners(w: f64, h: f64) -> [CornerGeometry; 4] {
        [
            right_corner(at(0.0, 0.0), 1.0, 1.0, w, h),
            right_corner(at(w, 0.0), -1.0, 1.0, w, h),
            right_corner(at(w, h), -1.0, -1.0, w, h),
            right_corner(at(0.0, h), 1.0, -1.0, w, h),
        ]
    }

    /// Asserts every corner wedge of `corners` is swept at `z` by `cuts`.
    fn assert_wedges_cleared(corners: &[CornerGeometry], cuts: &[Cut], radius: f64, z: f64) {
        for g in corners {
            for p in wedge_samples(g, radius) {
                assert!(
                    swept_covers(p, cuts, radius, z),
                    "({:.3}, {:.3}) uncut near corner {:?}",
                    p.x,
                    p.y,
                    g.corner
                );
            }
        }
    }

    // ── 1. The endpoint rule ────────────────────────────────────────────

    /// Upstream: "dogbone ends on the bisector at exactly the cutter
    /// radius".
    #[test]
    fn dogbone_ends_on_bisector_at_radius() {
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, 3.0);
            let end = excursion_end(&g, Style::Dogbone, 3.0);
            assert_approx_eq!(end.distance_to(g.corner), 3.0, 1e-9);
            assert_approx_eq!(end.y, 0.0, 1e-9);
        }
    }

    /// Upstream: "t-bone ends on an adjacent edge" and "t-bone takes the
    /// X-parallel edge; longest_edge takes the longer one". DreamCarve's
    /// T-bone is VCarve's: the longer edge, the X-parallel one on a tie.
    #[test]
    fn t_bone_ends_on_longer_edge_at_radius() {
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, 3.0);
            let end = excursion_end(&g, Style::TBone, 3.0);
            assert_approx_eq!(end.distance_to(g.corner), 3.0, 1e-9);
            let on_a = off_line(g.edge_a, end, g.corner) < 1e-9;
            let on_b = off_line(g.edge_b, end, g.corner) < 1e-9;
            assert!(on_a || on_b, "t-bone at {angle}° ends on an edge");
        }
        // +X is the short edge, +Y the long one: the slot goes along +Y.
        let g = right_corner(Point::new(0.0, 0.0), 1.0, 1.0, 10.0, 40.0);
        let end = excursion_end(&g, Style::TBone, 2.0);
        assert!(
            approx(end.x, 0.0, 1e-9) && approx(end.y, 2.0, 1e-9),
            "{end:?}"
        );
        // Equal edges: the X-parallel one.
        let g = right_corner(Point::new(0.0, 0.0), 1.0, 1.0, 40.0, 40.0);
        let end = excursion_end(&g, Style::TBone, 2.0);
        assert!(
            approx(end.x, 2.0, 1e-9) && approx(end.y, 0.0, 1e-9),
            "{end:?}"
        );
        // The same corner with its edges the other way round.
        let swapped = CornerGeometry {
            edge_a: g.edge_b,
            edge_b: g.edge_a,
            ..g
        };
        assert_eq!(excursion_end(&swapped, Style::TBone, 2.0), end);
    }

    /// Upstream: "notch depth past the wall is r(1 − sin(θ/2)) for a
    /// dogbone, r for a t-bone", and `r(1 − sin θ)` into the other wall.
    #[test]
    fn notch_depth_past_wall() {
        let r = 3.0;
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, r);
            let half = g.interior_angle_rad / 2.0;
            let bite = |end: Point, edge: Vec2| r - off_line(edge, end, g.corner);
            let dogbone = excursion_end(&g, Style::Dogbone, r);
            assert_approx_eq!(bite(dogbone, g.edge_a), r * (1.0 - half.sin()), 1e-9);
            let t_bone = excursion_end(&g, Style::TBone, r);
            let on_a = off_line(g.edge_a, t_bone, g.corner) < 1e-9;
            let (chosen, other) = if on_a {
                (g.edge_a, g.edge_b)
            } else {
                (g.edge_b, g.edge_a)
            };
            assert_approx_eq!(bite(t_bone, chosen), r, 1e-9);
            assert_approx_eq!(
                bite(t_bone, other),
                (r * (1.0 - g.interior_angle_rad.sin())).max(0.0),
                1e-9
            );
        }
    }

    /// Upstream: "at a right angle the t-bone leaves the perpendicular
    /// wall untouched".
    #[test]
    fn right_angle_t_bone_leaves_perpendicular_wall() {
        let r = 3.0;
        let (g, _, _) = synthetic_corner(90.0, r);
        let end = excursion_end(&g, Style::TBone, r);
        let on_a = off_line(g.edge_a, end, g.corner) < 1e-9;
        let other = if on_a { g.edge_b } else { g.edge_a };
        assert!(r - off_line(other, end, g.corner) <= 1e-9);
    }

    // ── 2. Coverage: touch clears, ending short does not ────────────────

    /// The wall legs and an excursion from `descend` to `end` and back.
    fn excursion_cuts(walls: &[Cut], descend: Point, end: Point) -> Vec<Cut> {
        let mut all = walls.to_vec();
        all.push(line(descend, end, 0.0));
        all.push(line(end, descend, 0.0));
        all
    }

    /// Upstream: "{style} at r clears the whole wedge at every angle", and
    /// "without relief the wall contour alone always leaves the corner
    /// uncut" (else the coverage test proves nothing).
    #[test]
    fn clears_whole_wedge_at_every_angle() {
        let r = 3.0;
        for style in [Style::Dogbone, Style::TBone] {
            for angle in TEST_ANGLES {
                let (g, walls, descend) = synthetic_corner(angle, r);
                let end = excursion_end(&g, style, r);
                let all = excursion_cuts(&walls, descend, end);
                for p in wedge_samples(&g, r) {
                    assert!(
                        swept_covers(p, &all, r, 0.0),
                        "{style:?} at {angle}° left {p:?} uncut"
                    );
                }
                let uncut = wedge_samples(&g, r)
                    .into_iter()
                    .any(|p| !swept_covers(p, &walls, r, 0.0));
                assert!(uncut, "the {angle}° corner is uncut before relief");
            }
        }
    }

    /// Upstream: "{style} stopping short at 1.2r leaves the corner uncut"
    /// (a T-bone only up to 90°: past about 105° its excursion runs across
    /// the corner whatever its length), and "a very obtuse t-bone clears
    /// even when short".
    #[test]
    fn stopping_short_leaves_corner() {
        let r = 3.0;
        for style in [Style::Dogbone, Style::TBone] {
            for angle in TEST_ANGLES {
                if style == Style::TBone && angle > 90.0 {
                    continue;
                }
                let (g, walls, descend) = synthetic_corner(angle, r);
                let short = excursion_end(&g, style, r * 1.2);
                let all = excursion_cuts(&walls, descend, short);
                let uncut = wedge_samples(&g, r)
                    .into_iter()
                    .any(|p| !swept_covers(p, &all, r, 0.0));
                assert!(uncut, "{style:?} at {angle}° clears the wedge from 1.2r");
            }
        }
        let (g, walls, descend) = synthetic_corner(150.0, r);
        let short = excursion_end(&g, Style::TBone, r * 1.2);
        let all = excursion_cuts(&walls, descend, short);
        assert!(
            wedge_samples(&g, r)
                .into_iter()
                .all(|p| swept_covers(p, &all, r, 0.0))
        );
    }

    // ── 3. Corner detection ─────────────────────────────────────────────

    fn site(loops: Vec<(Polyline, bool)>, walls: Vec<Polyline>) -> Site {
        Site {
            loops: loops
                .into_iter()
                .map(|(polyline, cleared_inside)| ReliefLoop {
                    polyline,
                    cleared_inside,
                    shape: shape(),
                })
                .collect(),
            walls,
        }
    }

    fn square_site() -> Site {
        site(
            vec![(rect(40.0, 40.0), true)],
            vec![poly(&[(2.0, 2.0), (38.0, 2.0), (38.0, 38.0), (2.0, 38.0)])],
        )
    }

    /// Upstream: "a square boundary yields four corners; its wall path
    /// yields the descend points".
    #[test]
    fn square_pocket_yields_four_corners_and_descend_points() {
        let (corners, warnings) = collect(&op_id(), Style::Dogbone, R, &square_site(), 0.01);
        assert_eq!(corners.len(), 4);
        assert!(warnings.is_empty(), "{warnings:?}");
        for c in &corners {
            let miter = R / (c.geometry.interior_angle_rad / 2.0).sin();
            assert_approx_eq!(c.descend.distance_to(c.geometry.corner), miter, 1e-6);
            assert_approx_eq!(c.end.distance_to(c.geometry.corner), R, 1e-9);
        }
    }

    /// #215: from the first corner of a square, the two next to it are
    /// equally near. A later one nearer by less than
    /// [`crate::sequence::NEAREST_TIE_MM`] (the 1e-6 mm an `f32` import
    /// moves a coordinate) is still a tie, taken in drawing order, so the
    /// noisy square's notches are cut in the clean square's order.
    #[test]
    fn equidistant_corners_follow_drawing_order_despite_noise() {
        let (clean, _) = collect(&op_id(), Style::Dogbone, R, &square_site(), 0.01);
        assert_eq!(clean.len(), 4);
        let start = clean[0].descend;
        let order = |corners: Vec<ReliefCorner>| -> Vec<Point> {
            nearest_neighbour(corners, start)
                .iter()
                .map(|c| c.geometry.corner)
                .collect()
        };
        let expected = order(clean.clone());
        for noise_mm in [1e-6, 1e-4, 0.9 * crate::sequence::NEAREST_TIE_MM] {
            // Each of the two neighbours in turn moved nearer the start.
            for moved in [1, 3] {
                let mut noisy = clean.clone();
                let towards = (start - noisy[moved].descend)
                    .normalized()
                    .unwrap_or(Vec2::new(0.0, 0.0));
                noisy[moved].descend = noisy[moved].descend + towards * noise_mm;
                assert_eq!(
                    order(noisy),
                    expected,
                    "corner {moved} {noise_mm} mm nearer"
                );
            }
        }
    }

    /// Upstream: "a cleared-outside loop relieves its concave corners
    /// only".
    #[test]
    fn cleared_outside_loop_relieves_concave_only() {
        let wall = poly(&[
            (28.0, 32.0),
            (28.0, 62.0),
            (62.0, 62.0),
            (62.0, -2.0),
            (-2.0, -2.0),
            (-2.0, 32.0),
        ]);
        let (corners, warnings) = collect(
            &op_id(),
            Style::Dogbone,
            R,
            &site(vec![(poly(&L_PART), false)], vec![wall]),
            0.01,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(corners.len(), 1);
        assert!(corners[0].geometry.corner.distance_to(at(30.0, 30.0)) < 1e-9);
        assert!(corners[0].descend.distance_to(at(28.0, 32.0)) < 1e-6);
    }

    /// Upstream: "a corner whose adjacent edges cannot hold the notch is
    /// rejected, with a warning naming it".
    #[test]
    fn too_tight_corner_rejected_with_warning() {
        let notch = poly(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (37.0, 20.0),
            (37.0, 23.0),
            (0.0, 23.0),
        ]);
        let (_, warnings) = collect(
            &op_id(),
            Style::Dogbone,
            4.0,
            &site(vec![(notch.clone(), true)], vec![notch]),
            0.01,
        );
        let tight: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code() == WarningCode::ReliefCornerTooTight)
            .collect();
        assert!(!tight.is_empty());
        for w in tight {
            let Warning::ReliefCornerTooTight {
                op,
                shape,
                x_mm,
                y_mm,
                ..
            } = w
            else {
                panic!("{w:?}");
            };
            assert!(x_mm.is_finite() && y_mm.is_finite(), "{w:?}");
            assert_eq!(op.as_str(), "o1");
            assert_eq!(shape.instance.as_str(), "i1");
        }
    }

    /// Upstream: "style 'none' collects nothing at all" and "'none' and a
    /// legacy operation with the field absent are identical to today":
    /// `none` has no style, and a dogbone Pocket is the `none` Pocket with
    /// the relief appended ([`relief_cuts`] checks the prefix).
    #[test]
    fn none_collects_nothing() {
        assert_eq!(Style::of(CornerRelief::None), None);
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (_, warnings) = generate_pocket(&p, &pocket_op(CornerRelief::None)).unwrap();
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let (_, _, relief) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(!relief.is_empty(), "a real style changes the toolpath");
    }

    // ── 4. The step-down and the descend guard ──────────────────────────

    /// Upstream: "the guard reads fed moves at or below the level, and
    /// ignores rapids and plunges".
    #[test]
    fn main_path_cuts_at_reads_cuts_at_or_below_the_level() {
        let path = |from: (f64, f64, f64), mv: Move| Toolpath {
            moves: vec![Move::rapid(Point3::new(from.0, from.1, from.2)), mv],
        };
        let cut = |x: f64, y: f64, z: f64| Move::linear(Point3::new(x, y, z), 800.0);
        let p = Point::new(5.0, 0.0);
        let tol = RELIEF_POSITION_TOLERANCE_MM;
        let level = |z: f64| path((0.0, 0.0, z), cut(10.0, 0.0, z));
        assert!(main_path_cuts_at(&level(-5.0), p, -5.0, tol));
        assert!(main_path_cuts_at(&level(-6.0), p, -5.0, tol));
        assert!(!main_path_cuts_at(&level(-4.0), p, -5.0, tol));
        let rapid = Move::rapid(Point3::new(10.0, 0.0, -5.0));
        assert!(!main_path_cuts_at(
            &path((0.0, 0.0, -5.0), rapid),
            p,
            -5.0,
            tol
        ));
        let plunge = cut(0.0, 0.0, -5.0);
        assert!(!main_path_cuts_at(
            &path((0.0, 0.0, 5.0), plunge),
            Point::new(0.0, 0.0),
            -5.0,
            tol
        ));
        assert!(!main_path_cuts_at(
            &level(-5.0),
            Point::new(5.0, 1.0),
            -5.0,
            tol
        ));
        // An arc through the point counts.
        let arc = Move {
            kind: MoveKind::ArcCcw {
                centre: Point::new(0.0, 0.0),
            },
            to: Point3::new(0.0, 5.0, -5.0),
            feed_mm_min: Some(800.0),
        };
        let on_arc = Point::new(5.0 * FRAC_1_SQRT_2, 5.0 * FRAC_1_SQRT_2);
        assert!(main_path_cuts_at(
            &path((5.0, 0.0, -5.0), arc),
            on_arc,
            -5.0,
            tol
        ));
    }

    fn relief_for(levels: Vec<f64>) -> Relief {
        Relief {
            op: op_id(),
            style: Style::Dogbone,
            radius_mm: R,
            levels,
            safe_z_mm: 11.0,
            stock_top_mm: 6.0,
            rates: LapRates {
                feed_mm_min: 800.0,
                plunge_mm_min: 300.0,
            },
            tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        }
    }

    /// Upstream: "a corner the main path never cut emits no motion and
    /// one warning".
    #[test]
    fn uncut_corner_emits_no_motion_and_warns() {
        let mut toolpath = Toolpath::default();
        let mut warnings = Vec::new();
        let cut = relief_for(vec![4.0, 2.0])
            .append(&[square_site()], &mut toolpath, &mut warnings)
            .unwrap();
        assert!(cut.is_empty());
        assert!(toolpath.moves.is_empty());
        assert_eq!(codes(&warnings), vec![WarningCode::ReliefCornerNotCut; 4]);
        assert!(warnings.iter().all(|w| matches!(
            w,
            Warning::ReliefCornerNotCut { x_mm, .. } if x_mm.is_finite()
        )));
    }

    // ── 5. Pocket ───────────────────────────────────────────────────────

    /// Upstream: "{style} on a square pocket clears all four corners of
    /// the emitted motion": four out-and-back pairs, every wedge swept.
    #[test]
    fn pocket_square_clears_all_four_corners_eight_cuts() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        for relief in [CornerRelief::Dogbone, CornerRelief::TBone] {
            let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(relief));
            assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
            assert_eq!(cut.len(), 8, "{relief:?}");
            assert_wedges_cleared(&rect_corners(40.0, 40.0), &cuts(&toolpath), R, 4.0);
        }
    }

    /// #227, S2: a notch's plunge at its descend point, where the main
    /// path has cut the floor at Z 4, starts from a rapid straight down
    /// from safe Z that stops 1 mm above that floor, not at safe Z: for a
    /// Pocket's relief pass, cut after the main path, and for an inside
    /// Profile's notches, cut among its passes. The last rapid down at a
    /// descend point is the notch's. The Profile's lap starts and ends at
    /// (22, 22), so that corner's notch is cut without a lift (#240,
    /// [`notch_at_the_lap_end_does_not_lift`]) and is left out here.
    #[test]
    fn notch_rapids_to_just_above_the_cut_floor() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let descend_points = [at(2.0, 2.0), at(38.0, 2.0), at(38.0, 38.0), at(2.0, 38.0)];
        let safe_z_mm = THICKNESS_MM + 5.0;
        let pocket = generate_pocket(&p, &pocket_op(CornerRelief::Dogbone))
            .unwrap()
            .0;
        let profile = generate_profile(&p, &profile_op(Side::Inside, CornerRelief::Dogbone))
            .unwrap()
            .0;
        for (name, tp, lifted) in [
            ("pocket", pocket, &descend_points[..]),
            ("profile", profile, &descend_points[1..]),
        ] {
            for &d in lifted {
                let at_d = |m: &Move| m.to.xy().distance_to(d) < 1e-6;
                let i = tp
                    .moves
                    .iter()
                    .rposition(|m| m.is_rapid() && at_d(m) && m.to.z < safe_z_mm - EPS_MM)
                    .unwrap_or_else(|| panic!("{name}: no rapid down at {d:?}: {:?}", tp.moves));
                assert_eq!(
                    tp.moves[i - 1],
                    Move::rapid(Point3::new(d.x, d.y, safe_z_mm)),
                    "{name} at {d:?}"
                );
                assert_eq!(
                    tp.moves[i],
                    Move::rapid(Point3::new(d.x, d.y, 5.0)),
                    "{name} at {d:?}"
                );
                assert_eq!(
                    tp.moves[i + 1],
                    Move::linear(Point3::new(d.x, d.y, 4.0), 300.0),
                    "{name} at {d:?}"
                );
            }
        }
    }

    /// Upstream: "acute, right and obtuse corners of one pocket are all
    /// relieved"; the acute corner needs the longest excursion,
    /// `r(1/sin(θ/2) − 1)`.
    #[test]
    fn acute_right_and_obtuse_corners_are_relieved() {
        let p = one_tool(vec![poly(&[(0.0, 0.0), (60.0, 0.0), (0.0, 25.0)])]);
        let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(cut.len(), 6);
        let mut lengths: Vec<f64> = outward(&cut).iter().map(Segment::length_mm).collect();
        lengths.sort_by(f64::total_cmp);
        let acute_half = 25.0f64.atan2(60.0) / 2.0;
        assert_approx_eq!(lengths[2], R * (1.0 / acute_half.sin() - 1.0), 1e-3);
        assert_approx_eq!(lengths[0], R * (SQRT_2 - 1.0), 1e-3);
    }

    /// Upstream: "reflex island corners are relieved and convex island
    /// corners are not".
    #[test]
    fn island_reflex_corners_relieved_convex_not() {
        let island = poly(&[
            (20.0, 20.0),
            (40.0, 20.0),
            (40.0, 40.0),
            (30.0, 40.0),
            (30.0, 30.0),
            (20.0, 30.0),
        ]);
        let p = one_tool(vec![rect(60.0, 60.0), island]);
        let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(
            out.len(),
            5,
            "four wall corners and the island's reflex one"
        );
        let near_island = out
            .iter()
            .filter(|s| s.start.distance_to(at(30.0, 30.0)) < 5.0)
            .count();
        assert_eq!(near_island, 1);
    }

    /// Upstream: "tessellated arcs and existing fillets produce zero
    /// relief": a circle and squares filleted at 1, 5 and 12 mm.
    #[test]
    fn arcs_and_fillets_produce_no_relief() {
        let mut drawn = vec![Polyline::circle(at(25.0, 25.0), 20.0)];
        drawn.extend([1.0, 5.0, 12.0].map(|r| filleted_square(40.0, r)));
        for shape in drawn {
            let p = one_tool(vec![shape]);
            let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
            assert!(cut.is_empty(), "{cut:?}");
            assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        }
    }

    /// Upstream: "the relief pass steps down at the tool stepdown, not at
    /// operation.stepdown".
    #[test]
    fn steps_down_at_tool_step_down() {
        let p = project(vec![rect(40.0, 40.0)], vec![tool("t1", 4.0, Some(1.5))]);
        let op = PocketOp {
            depth_mm: Some(6.0),
            step_down_mm: Some(6.0),
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, _, cut) = pocket_run(&p, &op);
        let mut levels: Vec<f64> = cut.iter().map(|(_, z, _)| *z).collect();
        levels.sort_by(|a, b| b.total_cmp(a));
        levels.dedup_by(|a, b| approx(*a, *b, 1e-9));
        assert_eq!(levels.len(), 4, "{levels:?}");
        for pair in levels.windows(2) {
            assert!(pair[0] - pair[1] <= 1.5 + 1e-9);
        }
        assert_approx_eq!(levels[3], 0.0, 1e-9);
    }

    /// Upstream: "a tool with no stepdown fails the relief closed with a
    /// warning".
    #[test]
    fn no_step_down_skips_with_warning() {
        let p = project(vec![rect(40.0, 40.0)], vec![tool("t1", 4.0, None)]);
        let op = PocketOp {
            step_down_mm: Some(2.0),
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(cut.is_empty());
        assert_eq!(relief_codes(&warnings), vec![WarningCode::ReliefSkipped]);
        let w = &warnings[0];
        assert!(
            matches!(
                w,
                Warning::ReliefSkipped {
                    requested: CornerRelief::Dogbone,
                    reason: ReliefSkippedReason::NoStepDown,
                    ..
                }
            ),
            "{w:?}"
        );
    }

    /// Upstream: "a pocket finish with finishWalls off skips every corner
    /// and says which": with no wall pass the main path never reaches the
    /// descend points.
    #[test]
    fn wall_pass_none_skips_every_corner() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let op = PocketOp {
            wall_pass: WallPass::None,
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(cut.is_empty());
        assert_eq!(
            relief_codes(&warnings),
            vec![WarningCode::ReliefCornerNotCut; 4]
        );
    }

    /// Upstream: "radial stock sizes the relief to the wall the pass
    /// actually leaves": the excursion ends one radius from the corner of
    /// the region inset by the allowance.
    #[test]
    fn allowance_sizes_relief_to_the_wall_left() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let op = PocketOp {
            allowance_mm: 0.5,
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 4);
        let cleared_corner = at(39.5, 39.5);
        let nearest = out
            .iter()
            .map(|s| s.end.distance_to(cleared_corner))
            .fold(f64::INFINITY, f64::min);
        assert_approx_eq!(nearest, R, 1e-3);
    }

    /// Upstream: "rough big with stock and no relief, then finish small
    /// with relief, clears the corner", the finish as DreamCarve's rest
    /// machining: a 10 mm rough at a 0.4 mm allowance, then a rest Pocket
    /// with a 3 mm bit and dogbones. Together they sweep the corner wedge,
    /// and the finish bites no further past a wall than the notch depth,
    /// `r(1 − sin 45°)`.
    #[test]
    fn rough_with_allowance_then_rest_with_relief_clears_corner() {
        let mut p = project(
            vec![rect(40.0, 40.0)],
            vec![
                tool("rough", 10.0, Some(3.0)),
                tool("finish", 3.0, Some(1.0)),
            ],
        );
        let rough = PocketOp {
            id: OperationId::new("rough"),
            tool: ToolId::new("rough"),
            depth_mm: Some(4.0),
            allowance_mm: 0.4,
            ..pocket_op(CornerRelief::None)
        };
        let finish = PocketOp {
            id: OperationId::new("finish"),
            tool: ToolId::new("finish"),
            depth_mm: Some(4.0),
            rest_of: Some(rough.id.clone()),
            ..pocket_op(CornerRelief::Dogbone)
        };
        p.operations = vec![
            Operation::Pocket(rough.clone()),
            Operation::Pocket(finish.clone()),
        ];
        let (rough_path, _) = generate_pocket(&p, &rough).unwrap();
        let (finish_path, warnings) = generate_pocket(&p, &finish).unwrap();
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_safe(&finish_path, &p, &Operation::Pocket(finish.clone()));
        let mut all = cuts(&rough_path);
        all.extend(cuts(&finish_path));
        let r = 1.5;
        let corner = right_corner(at(40.0, 40.0), -1.0, -1.0, 40.0, 40.0);
        assert_wedges_cleared(&[corner], &all, r, THICKNESS_MM - 4.0);
        let depth = r * (1.0 - FRAC_1_SQRT_2);
        for (s, _, _) in cuts(&finish_path) {
            for q in [s.start, s.end] {
                let bite = r - (O + 40.0 - q.x).min(O + 40.0 - q.y);
                assert!(bite <= depth + 1e-6, "{q:?} bites {bite} past a wall");
            }
        }
    }

    /// Upstream: "the parallel pocket pattern relieves the same corners as
    /// the offset pattern".
    #[test]
    fn raster_relieves_the_same_corners_as_offset() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let ends = |strategy| {
            let op = PocketOp {
                strategy,
                ..pocket_op(CornerRelief::Dogbone)
            };
            let (_, _, cut) = pocket_run(&p, &op);
            let mut ends: Vec<String> = outward(&cut)
                .iter()
                .map(|s| format!("{:.4},{:.4}", s.end.x, s.end.y))
                .collect();
            ends.sort();
            ends
        };
        let offset = ends(ClearanceStrategy::Offset);
        assert_eq!(offset.len(), 4);
        assert_eq!(offset, ends(ClearanceStrategy::Raster { angle_deg: 0.0 }));
    }

    // ── 6. Profile ──────────────────────────────────────────────────────

    /// Upstream: "an inside edge route relieves the convex corners of its
    /// boundary".
    #[test]
    fn inside_profile_relieves_hole_corners() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (toolpath, warnings, cut) =
            profile_run(&p, &profile_op(Side::Inside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(cut.len(), 8);
        assert_wedges_cleared(&rect_corners(40.0, 40.0), &cuts(&toolpath), R, 4.0);
    }

    /// Upstream: "an outside edge route relieves concave part corners
    /// only": one notch in an L part, descending at the wall path's corner
    /// outside (30, 30), and none on a square part.
    #[test]
    fn outside_profile_relieves_concave_part_corners() {
        let p = one_tool(vec![poly(&L_PART)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 1);
        assert!(out[0].start.distance_to(at(28.0, 32.0)) < 1e-6, "{out:?}");

        let square = one_tool(vec![rect(40.0, 40.0)]);
        let (_, _, cut) = profile_run(&square, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(cut.is_empty(), "the cutter wraps a convex part's corners");
    }

    /// The distance along the Profile's lap at Z `z` from its start to
    /// `point`, where the lap passes through it.
    fn distance_along_lap(toolpath: &Toolpath, z: f64, point: Point) -> f64 {
        let mut along = 0.0;
        for (s, z0, z1) in cuts(toolpath) {
            if !(approx(z0, z, 1e-9) && approx(z1, z, 1e-9)) {
                continue;
            }
            if s.end.distance_to(point) < 1e-6 {
                return along + s.length_mm();
            }
            along += s.length_mm();
        }
        panic!("the lap at {z} does not pass through {point:?}");
    }

    /// Upstream: "a corner under a tab is skipped rather than plunged
    /// into" and "a tab clear of the corner leaves the relief in place".
    /// Keep-outs are dropped (design spec): the tab lifts the lap over the
    /// descend point, so the guard skips the corner.
    #[test]
    fn corner_under_tab_is_skipped() {
        let p = one_tool(vec![poly(&L_PART)]);
        let plain = profile_op(Side::Outside, CornerRelief::Dogbone);
        let (untabbed, _) = generate_profile(&p, &plain).unwrap();
        let d = distance_along_lap(&untabbed, 4.0, at(28.0, 32.0));
        let tabbed = |distance_mm: f64| ProfileOp {
            tabs: Some(TabSpec {
                placement: TabPlacement::AtDistances {
                    distances_mm: vec![distance_mm],
                },
                width_mm: 6.0,
                height_mm: 5.0,
                shape: TabShape::Rect,
                on_holes: false,
            }),
            ..plain.clone()
        };
        let (_, warnings, cut) = profile_run(&p, &tabbed(d));
        assert!(cut.is_empty(), "{cut:?}");
        assert_eq!(
            relief_codes(&warnings),
            vec![WarningCode::ReliefCornerNotCut]
        );
        let (_, warnings, cut) = profile_run(&p, &tabbed(d + 40.0));
        assert_eq!(cut.len(), 2);
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
    }

    /// Relief composes with a lead (slice 11): the lead re-seams each lap
    /// and the notches follow the last pass, in the same four corners.
    #[test]
    fn inside_profile_with_lead_relieves_the_same_corners() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let op = ProfileOp {
            lead: Some(dc_project::Lead {
                radius_mm: 3.0,
                sweep_deg: 90.0,
                lead_out: true,
                overcut_mm: 1.0,
            }),
            ..profile_op(Side::Inside, CornerRelief::Dogbone)
        };
        let (toolpath, warnings, cut) = profile_run(&p, &op);
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(cut.len(), 8);
        assert_wedges_cleared(&rect_corners(40.0, 40.0), &cuts(&toolpath), R, 4.0);
    }

    /// Decision: On line has no waste side for a notch, so the relief is
    /// skipped with a warning.
    #[test]
    fn on_line_profile_skips_with_warning() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::OnLine, CornerRelief::TBone));
        assert!(cut.is_empty());
        assert_eq!(relief_codes(&warnings), vec![WarningCode::ReliefSkipped]);
        assert!(
            matches!(
                warnings[0],
                Warning::ReliefSkipped {
                    requested: CornerRelief::TBone,
                    reason: ReliefSkippedReason::OnLine,
                    ..
                }
            ),
            "{:?}",
            warnings[0]
        );
    }

    /// #189: on a Profile through the stock with no tabs, every notch of a
    /// shape, at every level down to the bottom, is cut before that
    /// shape's freeing lap (the final lap at Z 0) closes, the bottom level
    /// from within it (#193), and nothing of the shape is cut after it:
    /// inside (two 40 mm square holes, four notches
    /// each) and outside (two L parts, one notch each), under both
    /// machining orders. Shapes lie apart in X, so a move's X names its
    /// shape.
    #[test]
    fn profile_notches_precede_the_freeing_pass() {
        let shifted = |points: &[(f64, f64)], dx: f64| -> Polyline {
            let moved: Vec<(f64, f64)> = points.iter().map(|&(x, y)| (x + dx, y)).collect();
            poly(&moved)
        };
        let square = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)];
        // (side, the two shapes, the X that parts them, notches per shape)
        let cases = [
            (
                Side::Inside,
                vec![shifted(&square, 0.0), shifted(&square, 60.0)],
                O + 50.0,
                4,
            ),
            (
                Side::Outside,
                vec![shifted(&L_PART, 0.0), shifted(&L_PART, 80.0)],
                O + 70.0,
                1,
            ),
        ];
        for (side, profiles, parting_x, corners) in cases {
            let p = one_tool(profiles);
            for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
                let op = ProfileOp {
                    depth_mm: Some(THICKNESS_MM),
                    machining_order: order,
                    ..profile_op(side, CornerRelief::Dogbone)
                };
                let (with, warnings) = generate_profile(&p, &op).unwrap();
                assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
                assert_safe(&with, &p, &Operation::Profile(op.clone()));
                let none = ProfileOp {
                    corner_relief: CornerRelief::None,
                    ..op.clone()
                };
                let (without, _) = generate_profile(&p, &none).unwrap();
                let relief = interleaved_relief(&with, &without);
                let first = with.moves.first().map(|m| m.to).unwrap();
                let all = xy_cuts(first, &with.moves);
                let is_relief = |i: usize| relief.iter().any(|(k, _)| *k == i);
                let at_bottom = |c: &Cut| c.1.abs() <= EPS_MM && c.2.abs() <= EPS_MM;
                for left in [true, false] {
                    let mine = |c: &Cut| (c.0.end.x < parting_x) == left;
                    let notches: Vec<usize> = relief
                        .iter()
                        .filter(|(_, c)| mine(c))
                        .map(|(i, _)| *i)
                        .collect();
                    // Three levels (Z 4, 2, 0), out and back at each corner.
                    assert_eq!(notches.len(), 3 * 2 * corners, "{side:?} {order:?}");
                    let bottom_notches = relief
                        .iter()
                        .filter(|(_, c)| mine(c) && at_bottom(c))
                        .count();
                    assert_eq!(bottom_notches, 2 * corners, "{side:?} {order:?}");
                    let freeing: Vec<usize> = all
                        .iter()
                        .filter(|(i, c)| !is_relief(*i) && mine(c) && at_bottom(c))
                        .map(|(i, _)| *i)
                        .collect();
                    let (Some(&lap_first), Some(&lap_last)) = (freeing.first(), freeing.last())
                    else {
                        panic!("{side:?} {order:?}: no freeing lap");
                    };
                    // The levels above the bottom come before the final
                    // lap; the bottom level is cut within it (#193),
                    // after the pass above and before the lap's last move
                    // (right after the plunge when the lap starts at the
                    // corner).
                    let pass_above = all
                        .iter()
                        .filter(|(i, c)| !is_relief(*i) && mine(c) && !at_bottom(c))
                        .map(|(i, _)| *i)
                        .max()
                        .unwrap();
                    assert!(
                        relief
                            .iter()
                            .filter(|(_, c)| mine(c))
                            .all(|(i, c)| if at_bottom(c) {
                                pass_above < *i && *i < lap_last
                            } else {
                                *i < lap_first
                            }),
                        "{side:?} {order:?}: a notch at {notches:?} is not before the \
                         freeing lap {lap_first}..{lap_last} closes"
                    );
                    assert!(
                        all.iter()
                            .filter(|(_, c)| mine(c))
                            .all(|(i, _)| *i <= lap_last),
                        "{side:?} {order:?}: a cut follows the freeing lap"
                    );
                }
            }
        }
    }

    /// #193: an inside 40 × 40 mm Profile through the stock with a ramp
    /// entry, and no tabs, never plunges into uncut material at a descend
    /// point: every straight plunge there ends at or above a Z some earlier
    /// cut already reached at that point. The bottom-level notches
    /// are cut from inside the final lap, after the lap reaches them at
    /// depth and before it closes. Cases: the operation's step-down equal
    /// to the tool's (one notch level per pass), and coarser, 3 mm against
    /// the tool's 2 mm with a lead, so the freeing pass holds two levels
    /// (Z 2 and 0) and the lap is re-seamed; both machining orders.
    #[test]
    fn bottom_level_notches_do_not_plunge() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let descend_points = [at(2.0, 2.0), at(38.0, 2.0), at(38.0, 38.0), at(2.0, 38.0)];
        let lead = dc_project::Lead {
            radius_mm: 3.0,
            sweep_deg: 90.0,
            lead_out: true,
            overcut_mm: 1.0,
        };
        for (step_down_mm, lead) in [(None, None), (Some(3.0), Some(lead))] {
            for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
                let op = ProfileOp {
                    depth_mm: Some(THICKNESS_MM),
                    step_down_mm,
                    machining_order: order,
                    entry: Entry::Ramp { angle_deg: 5.0 },
                    lead: lead.clone(),
                    ..profile_op(Side::Inside, CornerRelief::Dogbone)
                };
                let case = format!("{step_down_mm:?} {order:?}");
                let (with, warnings) = generate_profile(&p, &op).unwrap();
                assert!(relief_codes(&warnings).is_empty(), "{case}: {warnings:?}");
                assert_safe(&with, &p, &Operation::Profile(op.clone()));

                // Every descent at a descend point stays in the column
                // already cut there.
                let mut deepest_at = [f64::INFINITY; 4];
                let mut pos = with.moves.first().map(|m| m.to).unwrap();
                for (i, mv) in with.moves.iter().enumerate() {
                    let flat = (pos.z - mv.to.z).abs() <= EPS_MM;
                    let path = Segment {
                        start: pos.xy(),
                        end: mv.to.xy(),
                        bulge: bulge_of(pos.xy(), mv),
                    };
                    for (k, &d) in descend_points.iter().enumerate() {
                        let plunges = mv.to.z < pos.z - EPS_MM
                            && pos.xy().distance_to(d) < 1e-6
                            && mv.to.xy().distance_to(d) < 1e-6;
                        if mv.is_cut() && plunges {
                            assert!(
                                mv.to.z >= deepest_at[k] - EPS_MM,
                                "{case}: move {i} goes down to Z {} at {d:?}, below the \
                                 Z {} cut there so far",
                                mv.to.z,
                                deepest_at[k]
                            );
                        }
                        let reaches = mv.to.xy().distance_to(d) < 1e-6
                            || (flat && point_segment_distance_mm(path, d) < 1e-6);
                        if mv.is_cut() && reaches {
                            deepest_at[k] = deepest_at[k].min(mv.to.z);
                        }
                    }
                    pos = mv.to;
                }

                // Every bottom notch lies between the final lap's arrival
                // at the bottom and its last move.
                let none = ProfileOp {
                    corner_relief: CornerRelief::None,
                    ..op.clone()
                };
                let (without, _) = generate_profile(&p, &none).unwrap();
                let relief = interleaved_relief(&with, &without);
                let first = with.moves.first().map(|m| m.to).unwrap();
                let all = xy_cuts(first, &with.moves);
                let is_relief = |i: usize| relief.iter().any(|(k, _)| *k == i);
                let at_bottom = |z: f64| z.abs() <= EPS_MM;
                assert_eq!(relief.len(), 3 * 2 * 4, "{case}");
                let bottom: Vec<usize> = relief
                    .iter()
                    .filter(|(_, c)| at_bottom(c.1) && at_bottom(c.2))
                    .map(|(i, _)| *i)
                    .collect();
                assert_eq!(bottom.len(), 2 * 4, "{case}");
                let lap_reaches_bottom = all
                    .iter()
                    .find(|(i, c)| !is_relief(*i) && at_bottom(c.2))
                    .map(|(i, _)| *i)
                    .unwrap();
                let lap_last = all
                    .iter()
                    .rfind(|(i, c)| !is_relief(*i) && at_bottom(c.1) && at_bottom(c.2))
                    .map(|(i, _)| *i)
                    .unwrap();
                assert!(
                    bottom
                        .iter()
                        .all(|&i| lap_reaches_bottom < i && i < lap_last),
                    "{case}: bottom notches {bottom:?} outside the final lap \
                     {lap_reaches_bottom}..{lap_last}"
                );
            }
        }
    }

    /// #240: an inside 40 × 40 mm Profile with dogbones starts and ends
    /// each lap on the descend point at (22, 22), and that corner's notch
    /// is cut right after the lap. It is reached from where the lap left
    /// the tool, in the column the lap just cut, with no rapid between the
    /// lap's last cut and the notch's first: no lift to safe Z and back.
    /// Cases: one pass (2 mm deep) and two (4 mm, a notch level per pass),
    /// both machining orders.
    #[test]
    fn notch_at_the_lap_end_does_not_lift() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let d = at(2.0, 2.0);
        for depth_mm in [2.0, 4.0] {
            for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
                let op = ProfileOp {
                    depth_mm: Some(depth_mm),
                    machining_order: order,
                    ..profile_op(Side::Inside, CornerRelief::Dogbone)
                };
                let case = format!("{depth_mm} mm {order:?}");
                let (with, warnings, _) = profile_run(&p, &op);
                assert!(relief_codes(&warnings).is_empty(), "{case}: {warnings:?}");
                let none = ProfileOp {
                    corner_relief: CornerRelief::None,
                    ..op.clone()
                };
                let (without, _) = generate_profile(&p, &none).unwrap();
                let relief = interleaved_relief(&with, &without);
                let is_relief = |i: usize| relief.iter().any(|(k, _)| *k == i);
                let first = with.moves.first().map(|m| m.to).unwrap();
                let all = xy_cuts(first, &with.moves);
                // The first cut of each notch at `d` and the main-path cut
                // just before it.
                let mut notches = 0;
                for pair in all.windows(2) {
                    let [(lap_i, lap), (notch_i, notch)] = pair else {
                        continue;
                    };
                    if is_relief(*lap_i)
                        || !is_relief(*notch_i)
                        || notch.0.start.distance_to(d) > 1e-6
                    {
                        continue;
                    }
                    notches += 1;
                    assert!(
                        lap.0.end.distance_to(d) < 1e-6,
                        "{case}: the lap before the notch at {d:?} ends at {:?}",
                        lap.0.end
                    );
                    let between = &with.moves[lap_i + 1..*notch_i];
                    assert!(
                        between.iter().all(|m| !m.is_rapid()),
                        "{case}: the notch at {d:?} lifts after the lap: {between:?}"
                    );
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let passes = (depth_mm / 2.0) as usize;
                assert_eq!(notches, passes, "{case}: notches at {d:?} after a lap");
            }
        }
    }

    // ── 7. Acceptance 29 and S9 ─────────────────────────────────────────

    /// The 40 × 30 rectangle's corners' dogbone ends: one radius past each
    /// corner along its inward bisector.
    fn dogbone_ends_40x30() -> Vec<Point> {
        rect_corners(40.0, 30.0)
            .iter()
            .map(|g| g.corner + g.bisector * R)
            .collect()
    }

    /// Acceptance 29: a 40 × 30 mm pocket with dogbones gets four notches
    /// along the corner bisectors, after the main passes, each ending
    /// exactly one tool radius past the corner, so a 40 × 30 block fits.
    #[test]
    fn acceptance_29_pocket_dogbone_four_notches_on_bisectors() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let corners = rect_corners(40.0, 30.0);
        let out = outward(&cut);
        assert_eq!(out.len(), 4, "{out:?}");
        for g in &corners {
            let end = g.corner + g.bisector * R;
            assert_approx_eq!(end.distance_to(g.corner), R, 1e-12);
            let notch = out
                .iter()
                .find(|s| s.end.distance_to(end) < 1e-9)
                .unwrap_or_else(|| panic!("no notch ends at {end:?}: {out:?}"));
            // Inward along the bisector, toward the corner.
            let along = (notch.end - notch.start).normalized().unwrap();
            assert!(along.cross(g.bisector).abs() < 1e-9, "{notch:?}");
            assert!(along.dot(g.bisector) < 0.0, "{notch:?}");
        }
        assert_wedges_cleared(&corners, &cuts(&toolpath), R, 4.0);
    }

    /// Acceptance 29: `t_bone` puts each notch along the longer, 40 mm,
    /// edge: it ends one radius from the corner on that edge's line.
    #[test]
    fn acceptance_29_t_bone_along_the_40_mm_edges() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::TBone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let corners = rect_corners(40.0, 30.0);
        let out = outward(&cut);
        assert_eq!(out.len(), 4);
        for g in &corners {
            let end = g.corner + g.edge_a * R;
            assert!(
                out.iter().any(|s| s.end.distance_to(end) < 1e-9),
                "{end:?}: {out:?}"
            );
        }
        assert_wedges_cleared(&corners, &cuts(&toolpath), R, 4.0);
    }

    /// Acceptance 29: an inside Profile of the same rectangle gets the
    /// same four notches.
    #[test]
    fn acceptance_29_inside_profile_same_four() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (_, _, pocket_cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Inside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let key = |cut: &[Cut]| {
            let mut ends: Vec<String> = outward(cut)
                .iter()
                .map(|s| {
                    format!(
                        "{:.6},{:.6}->{:.6},{:.6}",
                        s.start.x, s.start.y, s.end.x, s.end.y
                    )
                })
                .collect();
            ends.sort();
            ends
        };
        assert_eq!(key(&cut).len(), 4);
        assert_eq!(key(&cut), key(&pocket_cut));
    }

    /// Acceptance 29: an outside Profile of an L-shaped part gets one
    /// notch, in its concave corner: from the miter (28, 32) to one radius
    /// up and left of (30, 30).
    #[test]
    fn acceptance_29_outside_l_part_one_notch() {
        let p = one_tool(vec![poly(&L_PART)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 1);
        let want = at(30.0 - R * FRAC_1_SQRT_2, 30.0 + R * FRAC_1_SQRT_2);
        assert!(out[0].end.distance_to(want) < 1e-9, "{out:?}");
        assert!(out[0].start.distance_to(at(28.0, 32.0)) < 1e-6, "{out:?}");
    }

    /// S9: the notches cut past the pocket's walls only in their corners'
    /// lobes, within one radius of each dogbone end and at most the notch
    /// depth `r(1 − sin 45°)` past a wall, and the generator's own S9
    /// check accepted them.
    #[test]
    fn footprint_of_notch_stays_in_removed_material() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let op = pocket_op(CornerRelief::Dogbone);
        let (with, _) = generate_pocket(&p, &op).unwrap();
        let none = PocketOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_pocket(&p, &none).unwrap();
        let notches = Toolpath {
            moves: with.moves[without.moves.len() - 1..].to_vec(),
        };
        let swept = footprint(&notches, R);
        assert!(!swept.is_empty());
        let (pocket, _) = nest(vec![Contour::new(rect(40.0, 30.0)).unwrap()]).unwrap();
        let outside = difference(&swept, &pocket, FLATTEN_TOLERANCE_DEFAULT_MM);
        assert!(!outside.is_empty(), "the notches do cut past the walls");
        let ends = dogbone_ends_40x30();
        let depth = R * (1.0 - FRAC_1_SQRT_2);
        let tol = FLATTEN_TOLERANCE_DEFAULT_MM + 1e-6;
        for region in &outside {
            for v in region.outer().polyline().vertices() {
                let q = v.point();
                assert!(
                    ends.iter().any(|e| e.distance_to(q) <= R + tol),
                    "{q:?} is outside every corner's lobe"
                );
                let past = (O - q.x)
                    .max(q.x - O - 40.0)
                    .max(O - q.y)
                    .max(q.y - O - 30.0);
                assert!(past <= depth + tol, "{q:?} is {past} past a wall");
            }
        }
    }
}
