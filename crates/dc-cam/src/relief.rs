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
//! Relief steps down at the tool's step-down and is never a change to the
//! cleared region (a radius-`r` lobe eroded by a radius-`r` cutter
//! vanishes). Per corner and level the notch goes from the descend point,
//! where the main path cuts to full depth, out to the end point and back.
//!
//! **Inline on the final wall** (design
//! "Corner relief", steps 5 and 6). Upstream cuts relief as a pass of its
//! own after the main path, lifting and plunging at every corner.
//! DreamCarve cuts it from the path that cuts the final wall: a Pocket's
//! wall and island rings ([`Relief::inline_pocket`]) and a Profile's laps
//! ([`Relief::interleave`]), the last pass's only when it has one. Each
//! notch level goes to the first pass at or below it ([`level_groups`]).
//! Where that pass's path reaches the descend point at its Z, the tool
//! rises in the column it just opened, cuts the levels out and back,
//! stepping down at the descend point, and carries on ([`Relief::splice`],
//! #193's splice on every pass), with no lift. Upstream kept relief out of
//! the wall because a finishing pass cut full depth at once would slot each
//! notch full depth; the levels still step at the tool's step-down here, so
//! that cannot happen. The separate pass ([`Relief::cut`]) is the
//! fallback: for every corner of a Pocket with stock to leave (its rings are
//! not the final wall), for a Pocket corner some pass has no ring for, and,
//! on a Profile, for a pass whose lap does not reach the corner. A Pocket's
//! separate pass follows all its main passes, as upstream (a Pocket never
//! frees material); a Profile's notch blocks follow their pass's lap in its
//! unit. Nothing of a shape is cut after its freeing pass.
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
use crate::context::rates_of;
use crate::error::{CamError, ShapeRef};
use crate::footprint::{capsule, move_bulge};
use crate::passes::pass_z_mm;
use crate::profile::{Wall, shape_of};
use crate::sequence::{SequenceInput, clearance_z_mm, nearest_of, sequence};
use crate::tabs::LapRates;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::warning::{ReliefCornerReason, ReliefSkippedReason, Warning};
use dc_geometry::{
    Contour, EPS_MM, Point, Point3, Polyline, Region, Segment, Vec2, point_segment_distance_mm,
    segment_distance_mm, union,
};
use dc_project::{
    CornerRelief, InstanceId, MachiningOrder, OperationId, ProfileOp, Project, Side, Tool,
};
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
    // Each segment with the one before it round the path.
    let before = segments.iter().cycle().skip(n - 1);
    for (prev, next) in before.zip(&segments) {
        // A segment with no tangent at the vertex turns no corner there.
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
    let mut best: Option<f64> = None;
    for &(a, b) in segments {
        let edge = b - a;
        let denominator = direction.cross(edge);
        if denominator.abs() < UNIT_EPSILON {
            continue;
        }
        let to_start = a - g.corner;
        let t = to_start.cross(edge) / denominator;
        if t.is_nan() || t < min_t || t > max_t || best.is_some_and(|nearest| t >= nearest) {
            continue;
        }
        let along = to_start.cross(direction) / denominator;
        let slack = RELIEF_POSITION_TOLERANCE_MM / edge.length().max(EPS_MM);
        if along < -slack || along > 1.0 + slack {
            continue;
        }
        best = Some(t);
    }
    best.filter(|t| t.is_finite())
        .map(|t| g.corner + direction * t)
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
/// edges fail it), no tool-centre ring crossing within reach, or a notch
/// that would come nearer another wall of the site than `r`. A
/// relief that is not where the geometry says is worse than none, so
/// corners are rejected rather than approximated.
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
    let edges = loop_edges(site);
    let min_edge_mm = 2.0 * radius_mm - EPS_MM;
    for (index, l) in site.loops.iter().enumerate() {
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
            let Some(descend) = locate_descend(&g, &segments, radius_mm) else {
                warnings.push(too_tight(ReliefCornerReason::NoWallPath));
                continue;
            };
            let end = excursion_end(&g, style, radius_mm);
            if reaches_another_wall(&g, index, descend, end, &edges, radius_mm) {
                warnings.push(too_tight(ReliefCornerReason::NearOtherWall));
                continue;
            }
            corners.push(ReliefCorner {
                geometry: g,
                descend,
                end,
                shape: l.shape.clone(),
            });
        }
    }
    (corners, warnings)
}

/// Every segment of `site`'s loops with length, arcs as arcs, and the
/// index of its loop.
fn loop_edges(site: &Site) -> Vec<(usize, Segment)> {
    site.loops
        .iter()
        .enumerate()
        .flat_map(|(index, l)| {
            l.polyline
                .segments()
                .filter(|s| s.length_mm() > EPS_MM)
                .map(move |s| (index, s))
        })
        .collect()
}

/// Whether the notch at corner `g` of loop `index`, cut from `descend` to
/// `end` and back, brings the cutter nearer than `radius_mm` to an edge
/// of `edges` other than the corner's own two. A notch may bite
/// past its own corner's walls, by design; nearer another wall it cuts
/// into material past that one too. For an outside Profile that wall is
/// the far side of a part thinner than the notch is deep, which the notch
/// would cut through; for a Pocket, an island. S9's notch rule refuses
/// that cut (`dc_gcode`'s `material`, "Corner relief, the notch rule"),
/// so the corner is skipped here, where the warning can name it.
fn reaches_another_wall(
    g: &CornerGeometry,
    index: usize,
    descend: Point,
    end: Point,
    edges: &[(usize, Segment)],
    radius_mm: f64,
) -> bool {
    let excursion = Segment {
        start: descend,
        end,
        bulge: 0.0,
    };
    let own = |s: &Segment| {
        s.start.distance_to(g.corner) <= EPS_MM || s.end.distance_to(g.corner) <= EPS_MM
    };
    edges
        .iter()
        .filter(|(i, s)| *i != index || !own(s))
        .any(|(_, s)| segment_distance_mm(excursion, *s) < radius_mm - RELIEF_POSITION_TOLERANCE_MM)
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
/// [`crate::sequence::NEAREST_TIE_MM`] are a tie.
fn nearest_neighbour(mut corners: Vec<ReliefCorner>, start: Point) -> Vec<ReliefCorner> {
    let mut ordered = Vec::with_capacity(corners.len());
    let mut from = start;
    // `nearest_of` has none to give only when none is left.
    while let Some(next) = nearest_of(
        corners
            .iter()
            .enumerate()
            .map(|(i, c)| (i, from.distance_to(c.descend))),
    ) {
        let corner = corners.remove(next);
        from = corner.descend;
        ordered.push(corner);
    }
    ordered
}

/// The relief `levels` (top first) grouped by the pass that cuts them
/// (design "Corner relief", step 5): each goes to the first of `passes`
/// (pass index and Z, top first) whose Z is at or below it, else the last.
/// That pass has opened the column at a descend point down to the level
/// by the time it reaches it. A pass gets the levels between it and the
/// pass above: one when the two step-downs agree, several when the
/// operation steps deeper than its tool, none when it steps finer.
fn level_groups(levels: &[f64], passes: &[(usize, f64)]) -> Vec<(usize, Vec<f64>)> {
    let mut groups: Vec<(usize, Vec<f64>)> = Vec::new();
    for &level in levels {
        // With no pass at all there is none to cut the level in.
        let Some(pass) = passes
            .iter()
            .find(|(_, z)| *z <= level + RELIEF_Z_TOLERANCE_MM)
            .or(passes.last())
            .map(|(pass, _)| *pass)
        else {
            continue;
        };
        match groups.last_mut() {
            Some((at, group)) if *at == pass => group.push(level),
            _ => groups.push((pass, vec![level])),
        }
    }
    groups
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
    /// Absolute clearance Z, mm: a notch of another instance than
    /// the one before is reached at it.
    pub clearance_z_mm: f64,
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
    /// Absolute clearance Z, mm.
    pub clearance_z_mm: f64,
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
            clearance_z_mm: self.clearance_z_mm,
            stock_top_mm: self.thickness_mm,
            rates: self.rates,
            tolerance_mm: self.tolerance_mm,
        }))
    }
}

impl Relief {
    /// Collects the corners of `sites` and appends the separate relief
    /// pass to `toolpath`, the operation's main path; returns the corners
    /// cut. The generators cut inline first ([`Relief::inline_pocket`])
    /// and hand only the rest to [`Relief::cut`]; the tests drive the pass
    /// alone.
    ///
    /// # Errors
    ///
    /// As [`sequence`], which never fails on the blocks built here.
    #[cfg(test)]
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

    /// Every corner of `sites` that qualifies for relief, the corners
    /// rejected warned into `warnings` ([`collect`]).
    fn corners(&self, sites: &[Site], warnings: &mut Vec<Warning>) -> Vec<ReliefCorner> {
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
        corners
    }

    /// A Pocket's notches cut inline on its final wall (design "Corner
    /// relief", steps 5 and 6): the
    /// corners of `sites` whose levels are spliced into the rings of
    /// `blocks`, the Pocket's main blocks before sequencing (its passes at
    /// `pass_z`), and the corners left for the separate pass
    /// ([`Relief::cut`]).
    ///
    /// Each level goes to the first pass at or below it
    /// ([`level_groups`]). A corner is cut inline when every pass with
    /// levels for it has a closed ring of the corner's shape (the wall
    /// ring or an island ring) that reaches its descend point at the
    /// pass's Z; each such ring then carries that pass's levels
    /// ([`Relief::splice`]). Any other corner is left whole for the
    /// separate pass, so its levels are still cut top first. With
    /// `final_wall` false (a stock to leave: the rings are not the final
    /// wall) every corner is left for it.
    pub(crate) fn inline_pocket(
        &self,
        sites: &[Site],
        pass_z: &[f64],
        final_wall: bool,
        blocks: &mut [CutBlock],
        warnings: &mut Vec<Warning>,
    ) -> (Vec<ReliefCorner>, Vec<ReliefCorner>) {
        let corners = self.corners(sites, warnings);
        if !final_wall {
            return (Vec::new(), corners);
        }
        let passes: Vec<(usize, f64)> = pass_z.iter().copied().enumerate().collect();
        let groups = level_groups(&self.levels, &passes);
        let guard_mm = RELIEF_POSITION_TOLERANCE_MM + self.tolerance_mm;
        let (mut inline, mut fallback) = (Vec::new(), Vec::new());
        for corner in corners {
            let ring_at = |pass: usize| {
                blocks.iter().position(|b| {
                    b.shape == corner.shape
                        && b.pass == pass
                        && !b.cuts.is_empty()
                        && b.entry.distance_to(b.exit) <= EPS_MM
                        && block_cuts_at(b, corner.descend, b.z_mm, guard_mm)
                })
            };
            let rings: Option<Vec<(usize, &[f64])>> = groups
                .iter()
                .map(|(pass, levels)| ring_at(*pass).map(|i| (i, levels.as_slice())))
                .collect();
            let Some(rings) = rings.filter(|r| !r.is_empty()) else {
                fallback.push(corner);
                continue;
            };
            let mut all = true;
            for (index, levels) in rings {
                all &= match blocks.get_mut(index) {
                    Some(block) => self.splice(block, &corner, levels, guard_mm),
                    // A ring just found is among the blocks; were it not,
                    // its levels are not cut inline.
                    None => false,
                };
            }
            // Never in practice: the ring was found by the same test the
            // splice makes. The separate pass then cuts the levels again,
            // in material already gone, rather than leave one uncut.
            if all {
                inline.push(corner);
            } else {
                fallback.push(corner);
            }
        }
        (inline, fallback)
    }

    /// The pass itself (upstream `generateCornerReliefPass`): each corner
    /// whose descend point the main path cut at the deepest level, else
    /// [`Warning::ReliefCornerNotCut`]; nearest-neighbour from where
    /// the main path ended; per corner one [`CutBlock`] per level (plunge
    /// at the descend point, cut to the end, cut back), sequenced as one
    /// shape so the levels follow each other without a retract, and a
    /// retract between corners.
    pub(crate) fn cut(
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
        // A notch of another instance is reached at the clearance
        // Z, as the sequencer links blocks of different instances. Where
        // the main path ended is not known here, so the first notch is
        // reached at it too.
        let mut previous: Option<&InstanceId> = None;
        for corner in &ordered {
            if previous != Some(&corner.shape.instance)
                && self.clearance_z_mm > self.safe_z_mm + EPS_MM
                && let Some(at) = toolpath.moves.last().map(|m| m.to)
                && at.z < self.clearance_z_mm - EPS_MM
            {
                toolpath
                    .moves
                    .push(Move::rapid(Point3::new(at.x, at.y, self.clearance_z_mm)));
            }
            previous = Some(&corner.shape.instance);
            let input = SequenceInput {
                blocks: self.blocks(corner),
                order: MachiningOrder::FullDepthFirst,
                containment: &[],
                safe_z_mm: self.safe_z_mm,
                // One corner is one instance's: its levels link at safe Z.
                clearance_z_mm: self.safe_z_mm,
                stock_top_mm: self.stock_top_mm,
                tool_diameter_mm: 2.0 * self.radius_mm,
                stepover_mm: 0.0,
                feed_mm_min: self.rates.feed_mm_min,
                plunge_mm_min: self.rates.plunge_mm_min,
                // The notch's rapid down stops just above the floor
                // the main path cut at its descend point.
                cut_before: &toolpath.moves,
                groove_link_z_mm: None,
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
        // What each level's plunge starts from: safe Z, then the level
        // above.
        let above = std::iter::once(self.safe_z_mm).chain(self.levels.iter().copied());
        self.levels
            .iter()
            .zip(above)
            .enumerate()
            .map(|(pass, (&z_mm, above_z_mm))| CutBlock {
                shape: corner.shape.clone(),
                pass,
                z_mm,
                descent_from: p,
                descent_from_z_mm: above_z_mm,
                descent: Vec::new(),
                entry: p,
                entry_z_mm: z_mm,
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

    /// A Profile's relief, cut among its shapes' own passes and
    /// inline on their laps (design "Corner relief", step 5; maintainer
    /// decision 2026-10-24): the notch blocks left to hand to [`sequence`]
    /// with `blocks`, the main blocks. A corner is cut when its own shape's
    /// path cuts its descend point at the deepest level (else
    /// [`Warning::ReliefCornerNotCut`]). Each level goes to the shape's
    /// first pass at or below it ([`level_groups`]), whose lap has opened
    /// the column at the descend point by the time it reaches it. Each
    /// pass's levels are cut from inside the lap of that pass that reaches
    /// the descend point at its Z ([`Relief::splice`], #193 on every pass):
    /// no lift, and nothing of the shape cut after a freeing lap closes. A
    /// pass whose lap does not reach it (step 6) cuts those levels as a
    /// separate notch block in its unit, after the lap. A shape's corners
    /// are taken nearest-neighbour from where its first block ends, the
    /// same order at every pass. With `only_pass` every level goes
    /// to that pass, the last pass, whose lap is the only one on the
    /// part's path: the stock passes never carry a notch.
    fn interleave(
        &self,
        sites: &[Site],
        only_pass: Option<usize>,
        blocks: &mut [CutBlock],
        warnings: &mut Vec<Warning>,
    ) -> Vec<CutBlock> {
        let corners = self.corners(sites, warnings);
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
        // Each shape, with where its first block ends.
        let mut shapes: Vec<(&ShapeRef, Point)> = Vec::new();
        for block in blocks.iter() {
            if !shapes.iter().any(|(shape, _)| *shape == &block.shape) {
                shapes.push((&block.shape, block.exit));
            }
        }
        for (shape, first_exit) in shapes {
            let own: Vec<&CutBlock> = blocks.iter().filter(|b| &b.shape == shape).collect();
            let mine: Vec<ReliefCorner> =
                cut.iter().filter(|c| &c.shape == shape).cloned().collect();
            // The shape's passes, top first, with their Zs.
            let mut passes: Vec<(usize, f64)> = own.iter().map(|b| (b.pass, b.z_mm)).collect();
            passes.sort_by_key(|(pass, _)| *pass);
            passes.dedup_by_key(|(pass, _)| *pass);
            if let Some(only) = only_pass {
                passes.retain(|(pass, _)| *pass == only);
            }
            let groups = level_groups(&self.levels, &passes);
            for corner in nearest_neighbour(mine, first_exit) {
                planned.extend(
                    groups
                        .iter()
                        .map(|(pass, levels)| (corner.clone(), *pass, levels.clone())),
                );
            }
        }
        let mut notches = Vec::new();
        for (corner, pass, levels) in planned {
            // The lap of the corner's shape at this pass that reaches the
            // descend point at the pass's Z (design "Corner relief", step
            // 5: inline on every pass, not only the freeing one).
            let lap = blocks.iter_mut().find(|b| {
                b.shape == corner.shape
                    && b.pass == pass
                    && block_cuts_at(b, corner.descend, b.z_mm, guard_mm)
            });
            if let Some(block) = lap
                && self.splice(block, &corner, &levels, guard_mm)
            {
                continue;
            }
            // Step 6: no lap reaches it, so the separate notch block.
            notches.extend(self.notch_block(&corner, pass, &levels));
        }
        notches
    }

    /// Cuts `levels` (top first) of `corner`'s notch from inside `block`,
    /// a lap or ring on the final wall (#193 for a lap that frees its
    /// piece; every pass's since the maintainer decision of 2026-10-24).
    /// Where the path first reaches the descend point `P` at or below its
    /// pass's Z (within `guard_mm`), the tool goes to `P`, rises in the
    /// column the path has just opened to the first level, cuts out to the
    /// end and back at each level at the feed rate, stepping down at `P`
    /// between levels, returns to the path's Z and point, and the path
    /// carries on. A move that passes `P` in its middle is split there.
    /// Every Z change is at `P`, in the open column, at the plunge rate, so
    /// nothing plunges into uncut material, and the rest of a freeing lap
    /// still holds the piece while the notch is cut. False, with `block`
    /// unchanged, when the path never reaches `P` at its Z.
    fn splice(
        &self,
        block: &mut CutBlock,
        corner: &ReliefCorner,
        levels: &[f64],
        guard_mm: f64,
    ) -> bool {
        let z_limit = block.z_mm + RELIEF_Z_TOLERANCE_MM;
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
            // The descent ends at the first level, where the notch starts.
            entry_z_mm: first,
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
/// the region less the stock to leave) and its tool-centre region (`centre`,
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
    /// The only pass whose laps run on `paths` (the last pass of a
    /// Profile with a last pass stock to leave, whose earlier passes run on the
    /// stock path and open no column where a notch descends); `None`
    /// when every pass does.
    pub only_pass: Option<usize>,
}

/// The relief sites of a Profile on `side`: per region, its outline and
/// holes with the side the kerf clears (Inside: inside the outline and
/// outside the holes; Outside: the reverse), and the region's tool-centre
/// paths. On line, Left and Right have none.
fn profile_sites(side: Side, geometry: &ProfileGeometry<'_>) -> Vec<Site> {
    if !side.has_waste_area() {
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
/// Profile's main blocks, among the shapes' own passes (see
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
    let unrelieved = match op.side {
        Side::OnLine => Some(ReliefSkippedReason::OnLine),
        Side::Left | Side::Right => Some(ReliefSkippedReason::OpenPath),
        Side::Inside | Side::Outside => None,
    };
    if let Some(reason) = unrelieved {
        warnings.push(skipped(&op.id, op.corner_relief, reason));
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
        clearance_z_mm: clearance_z_mm(project, thickness_mm, thickness_mm + op.safe_z_mm),
        rates: rates_of(tool)?,
        tolerance_mm: project.flatten_tolerance_mm,
    };
    Ok(match setup.plan(warnings)? {
        Some(relief) => relief.interleave(
            &profile_sites(op.side, geometry),
            geometry.only_pass,
            blocks,
            warnings,
        ),
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
mod tests;
