// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The flat floor of a V-Carve (`docs/specs/phase-3/design.md`,
//! "Algorithms → V-Carve generator", step 6; functional capability 39).
//! New in DreamCarve: PureCutCNC clamps the depth at `maxCarveDepth` and
//! cuts nothing more.
//!
//! With a flat depth `F` and slope `tan(α/2)`, the floor is each region
//! inset by `F · slope` ([`dc_geometry::floor_regions`]): where the tip is
//! on it at `F` below the start plane, the cone's disc at the start plane
//! (radius `F · slope`) stays inside the region, so every move of the
//! floor passes the cone test by construction. Each floor region is cleared
//! as `pocket::rings` clears a tool-centre region, for a tool of radius
//! `F · slope` (the groove's half-width at the floor), at step-over
//! `step_over_fraction · 2 · F · slope`, innermost first. The tree's root
//! rings, the floor's outline and islands, are the **boundary trace**, cut
//! last, climb (the region's own winding), so the flank meets the outline
//! at the start plane and the wall is a clean slope.
//!
//! Two additions to the Pocket rings:
//!
//! - **Centre rings.** The innermost ring of a branch can be up to one
//!   step-over from the middle of what it encloses, which would leave a
//!   ridge there twice the one between rings. Each innermost ring gets one
//!   more ring half a step-over inside it ([`residual_regions`], upstream's
//!   `appendResidualCoreGuides`), so the middle is no higher than the rest.
//! - **Arcs are flattened to the floor's side.** The cone test takes
//!   straight moves. A chord of an arc that turns clockwise (the round join
//!   at a reflex corner of the outline, round an island's corner) would cut
//!   toward the wall by up to the tolerance, so those arcs are replaced by
//!   lines tangent to them, outside the circle, on the floor's side; an arc
//!   that turns counter-clockwise keeps its chords, which already are.

use super::{Carve, VBit, carve_regions, v_bit};
use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::pocket::rest::{self, Reference};
use crate::pocket::rings::{
    Ring, RingKind, RingTree, WallRings, inset_region, is_noise, nearest, residual_regions,
};
use crate::pocket::{MAX_RINGS, ring_path};
use crate::tabs::LapRates;
use crate::toolpath::Move;
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Region, Segment, Vec2, floor_regions};
use dc_project::{CutDirection, OperationId, Project, Tool, VCarveOp};
use std::f64::consts::FRAC_PI_2;

/// The flat floor of a carve with one V-bit: where it lies and how its
/// rings step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Floor {
    /// The floor's Z, mm, project space: the start plane less the flat
    /// depth.
    pub z_mm: f64,
    /// `F · tan(α/2)`, mm: how far the floor lies inside the walls, and
    /// the half-width at the start plane of the groove the tip cuts on it.
    pub inset_mm: f64,
    /// The step-over fraction of the cut width `2 · inset_mm`, from the
    /// operation or the tool; `None` when neither sets one, which is an
    /// error only once there is a floor to cut.
    pub step_over_fraction: Option<f64>,
}

impl Floor {
    /// The floor of `carve` cut with a V-bit of slope `tan(α/2)`; `None`
    /// without a flat depth.
    pub(crate) fn of(carve: &Carve<'_>, slope: f64) -> Option<Self> {
        let flat_depth_mm = carve.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0)?;
        Some(Self {
            z_mm: carve.start_plane_z_mm - flat_depth_mm,
            inset_mm: flat_depth_mm * slope,
            step_over_fraction: carve.step_over_fraction,
        })
    }

    /// The floor regions of `region` that are cut: the region inset by
    /// [`Floor::inset_mm`] ([`floor_regions`]), less any thinner than twice
    /// `tolerance_mm` (Pocket's noise rule), which the medial chains cover.
    /// Empty where the region is nowhere wider than `2 · inset_mm`.
    pub(crate) fn regions(&self, region: &Region, tolerance_mm: f64) -> Vec<Region> {
        floor_regions(std::slice::from_ref(region), self.inset_mm, tolerance_mm)
            .into_iter()
            .filter(|r| !is_noise(r, tolerance_mm))
            .collect()
    }

    /// The ring step-over, mm, `step_over_fraction · 2 · inset_mm`.
    ///
    /// # Errors
    ///
    /// [`CamError::MissingStepOver`] when neither the operation nor `tool`
    /// sets a fraction, [`CamError::InvalidStepOver`] for one outside
    /// (0, 1].
    fn stepover_mm(&self, op: &OperationId, tool: &Tool) -> Result<f64, CamError> {
        let fraction = self
            .step_over_fraction
            .ok_or_else(|| CamError::MissingStepOver {
                op: op.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
            })?;
        if !(fraction.is_finite() && fraction > 0.0 && fraction <= 1.0) {
            return Err(CamError::InvalidStepOver {
                op: op.clone(),
                step_over_fraction: fraction,
            });
        }
        Ok(fraction * 2.0 * self.inset_mm)
    }
}

/// What every floor block of one region carries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FloorCut<'a> {
    /// The operation, for errors.
    pub op: &'a OperationId,
    /// The V-bit, for errors.
    pub tool: &'a Tool,
    /// The region's shape.
    pub shape: &'a ShapeRef,
    /// The bit's feed and plunge rates.
    pub rates: LapRates,
    /// Absolute safe Z, mm.
    pub safe_z_mm: f64,
    /// The project's flatten tolerance, mm.
    pub tolerance_mm: f64,
}

/// The blocks that cut `floors` (from [`Floor::regions`] of one region) at
/// the floor's Z: per floor region its rings innermost first, each
/// innermost ring after its centre ring, and the boundary trace (the
/// floor's outline and islands) last; every ring climb, starting at its
/// point nearest the end of the ring before it, from `anchor` (updated to
/// the last ring's end). Each block is a closed lap at pass 0, entered by
/// a straight plunge, with its ring's tool-centre region as `cleared`, so
/// `sequence` may link two of them at the floor inside the floor. Nothing
/// when `floors` is empty.
///
/// After the clearing passes `reached_by` (#406; design step 7; empty
/// without them) the boundary trace is still the whole floor's outline and
/// islands, cut last, but the rings clear only what no pass reached: each
/// floor region less every pass's reach ([`rest::left_by`], from the
/// passes' radii, never from their toolpaths: S14). Each piece left is
/// cleared as a floor region is, its own outline included
/// (`WallRings::Last`): that outline runs along the edge the end mills left
/// as well as along the trace, and without it the groove next to that edge
/// would stand a whole step-over off it. A piece's rings carry the piece as
/// `cleared`, and the trace the floor region, which the passes and the
/// V-bit's rings have cleared by then. When the passes reach everything,
/// the trace alone is cut, and it needs no step-over.
///
/// # Errors
///
/// [`CamError::MissingStepOver`] or [`CamError::InvalidStepOver`] as
/// [`Floor`]'s step-over, when there are rings to cut, and
/// `InvalidStepOver` when a floor would need more than [`MAX_RINGS`] rings.
pub(crate) fn blocks(
    floor: &Floor,
    floors: &[Region],
    reached_by: &[Reference],
    cut: &FloorCut<'_>,
    anchor: &mut Option<Point>,
) -> Result<Vec<CutBlock>, CamError> {
    if floors.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for region in floors {
        // A pass whose end mill does not fit this floor region reaches none
        // of it; with none that fits, the region is cut as with no passes.
        let reaching: Vec<Reference> = reached_by
            .iter()
            .copied()
            .filter(|r| !inset_region(region, r.radius_mm, cut.tolerance_mm).is_empty())
            .collect();
        if reaching.is_empty() {
            out.extend(rings_clearing(region, floor, cut, anchor)?);
            continue;
        }
        for piece in rest::left_by(region, 0.0, &reaching, cut.tolerance_mm) {
            out.extend(rings_clearing(&piece, floor, cut, anchor)?);
        }
        for ring in rings_of(region, 0, anchor) {
            out.push(ring_block(ring, region, floor, cut));
        }
    }
    if let Some(last) = out.last() {
        *anchor = Some(last.exit);
    }
    Ok(out)
}

/// The rings that clear `region` (a floor region, or a piece of one) at
/// the floor: a Pocket ring tree, innermost first, each innermost ring
/// after its centre ring, the region's own outline and islands last; each
/// ring with its node's region as `cleared`.
fn rings_clearing(
    region: &Region,
    floor: &Floor,
    cut: &FloorCut<'_>,
    anchor: &mut Option<Point>,
) -> Result<Vec<CutBlock>, CamError> {
    let stepover_mm = floor.stepover_mm(cut.op, cut.tool)?;
    let too_many = || CamError::InvalidStepOver {
        op: cut.op.clone(),
        step_over_fraction: floor.step_over_fraction.unwrap_or(0.0),
    };
    let tree = RingTree::build(
        region.clone(),
        stepover_mm,
        floor.inset_mm,
        cut.tolerance_mm,
        MAX_RINGS,
    )
    .ok_or_else(too_many)?;
    let mut out = Vec::new();
    let mut centred: Vec<usize> = Vec::new();
    for ring in tree.walk(WallRings::Last, anchor) {
        let Some(node) = tree.nodes.get(ring.node) else {
            continue;
        };
        if node.children.is_empty() && !node.residual && !centred.contains(&ring.node) {
            centred.push(ring.node);
            let mut from = Some(ring.start);
            for centre in residual_regions(&node.region, &[], stepover_mm / 2.0, cut.tolerance_mm) {
                for centre_ring in rings_of(&centre, ring.node, &mut from) {
                    out.push(ring_block(centre_ring, &centre, floor, cut));
                }
            }
        }
        let cleared = node.region.clone();
        out.push(ring_block(ring, &cleared, floor, cut));
    }
    if let Some(last) = out.last() {
        *anchor = Some(last.exit);
    }
    Ok(out)
}

/// The outer and holes of `region` as rings of `node`, nearest `anchor`
/// first, each starting at its point nearest the end of the one before.
fn rings_of(region: &Region, node: usize, anchor: &mut Option<Point>) -> Vec<Ring> {
    let mut pending: Vec<(Ring, f64)> = std::iter::once((region.outer(), RingKind::Outer))
        .chain(region.holes().iter().map(|h| (h, RingKind::Hole)))
        .map(|(contour, kind)| {
            (
                Ring {
                    contour: contour.clone(),
                    kind,
                    node,
                    start: Point::new(0.0, 0.0),
                    cleanups: Vec::new(),
                },
                0.0,
            )
        })
        .collect();
    let mut out = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        for (ring, distance) in &mut pending {
            *distance = nearest(&ring.contour, *anchor).1;
        }
        let pick = pending
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map_or(0, |(i, _)| i);
        let (mut ring, _) = pending.remove(pick);
        ring.start = nearest(&ring.contour, *anchor).0;
        *anchor = Some(ring.start);
        out.push(ring);
    }
    out
}

/// One ring as a closed lap at the floor's Z: climb, from its start, arcs
/// flattened to the floor's side ([`flatten_inside`]), every move at the
/// feed.
fn ring_block(ring: Ring, cleared: &Region, floor: &Floor, cut: &FloorCut<'_>) -> CutBlock {
    let start = ring.start;
    let path = ring_path(ring, CutDirection::Climb);
    let points = flatten_inside(&path, cut.tolerance_mm);
    let z_mm = floor.z_mm;
    let mut exit = start;
    let cuts = points
        .iter()
        .skip(1)
        .filter_map(|&p| {
            if p.distance_to(exit) <= EPS_MM {
                return None;
            }
            exit = p;
            Some(Move::linear(
                Point3::new(p.x, p.y, z_mm),
                cut.rates.feed_mm_min,
            ))
        })
        .collect();
    CutBlock {
        shape: cut.shape.clone(),
        pass: 0,
        z_mm,
        descent_from: start,
        descent_from_z_mm: cut.safe_z_mm,
        descent: Vec::new(),
        entry: start,
        entry_z_mm: z_mm,
        cuts,
        exit,
        frees: false,
        cleared: Some(cleared.clone()),
        group: 0,
    }
}

/// The points of `path` (closed, wound with the floor on its left, as a
/// climb ring is) from its first vertex back to it, every arc replaced by
/// straight lines within `tolerance_mm` of it on its left: chords for a
/// counter-clockwise arc, whose chords lie inside its circle, on its left;
/// lines tangent to a clockwise arc, outside its circle, on its left. So
/// no line comes nearer the walls than the arc it stands for.
pub(crate) fn flatten_inside(path: &Polyline, tolerance_mm: f64) -> Vec<Point> {
    let tolerance_mm = tolerance_mm.max(EPS_MM);
    let mut out: Vec<Point> = path
        .vertices()
        .first()
        .map(|v| v.point())
        .into_iter()
        .collect();
    for segment in path.segments() {
        out.extend(flatten_segment(segment, tolerance_mm));
    }
    out
}

/// The points after `segment`'s start that stand for it, ending at its
/// end ([`flatten_inside`]).
fn flatten_segment(segment: Segment, tolerance_mm: f64) -> Vec<Point> {
    let Some(arc) = segment.arc() else {
        return vec![segment.end];
    };
    let radius_mm = arc.radius_mm;
    let sweep_rad = arc.sweep_rad;
    let ratio = if sweep_rad >= 0.0 {
        // Chords: the sagitta `r (1 − cos(θ/2))` within the tolerance.
        1.0 - tolerance_mm / radius_mm
    } else {
        // Tangents: the corners `r (1 / cos(θ/2) − 1)` out.
        radius_mm / (radius_mm + tolerance_mm)
    };
    let half_rad = ratio.clamp(-1.0, 1.0).acos();
    let step_rad = (2.0 * half_rad).clamp(1e-6, FRAC_PI_2);
    let pieces = (sweep_rad.abs() / step_rad).ceil().max(1.0);
    // `pieces` is at least 1 and at most 2π / 1e-6.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = pieces as usize;
    let delta_rad = sweep_rad / pieces;
    let at = |angle_rad: f64, r: f64| arc.centre + Vec2::new(angle_rad.cos(), angle_rad.sin()) * r;
    let mut out = Vec::with_capacity(n + 1);
    if sweep_rad >= 0.0 {
        for i in 1..n {
            out.push(at(arc.start_angle_rad + delta_rad * i as f64, radius_mm));
        }
    } else {
        let corner_mm = radius_mm / (delta_rad / 2.0).cos();
        for k in 0..n {
            out.push(at(
                arc.start_angle_rad + delta_rad * (k as f64 + 0.5),
                corner_mm,
            ));
        }
    }
    out.push(segment.end);
    out
}

/// [`super::floor_ridge_height_mm`]: `s / (2 tan(α/2))` for the floor's
/// step-over `s`, when `op` has a floor for its V-bit to cut.
pub(crate) fn ridge_height_mm(project: &Project, op: &VCarveOp) -> Option<f64> {
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let VBit { slope, .. } = v_bit(tool)?;
    let flat_depth_mm = op.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0)?;
    let fraction = op
        .step_over_fraction
        .or(tool.step_over_fraction)
        .filter(|f| f.is_finite() && *f > 0.0 && *f <= 1.0)?;
    let floor = Floor {
        z_mm: 0.0,
        inset_mm: flat_depth_mm * slope,
        step_over_fraction: Some(fraction),
    };
    let targets = carve_regions(project, &op.id, &op.targets, &mut Vec::new()).ok()?;
    let tolerance_mm = project.flatten_tolerance_mm;
    let has_floor = targets
        .regions
        .iter()
        .any(|r| !floor.regions(r, tolerance_mm).is_empty());
    let stepover_mm = fraction * 2.0 * floor.inset_mm;
    has_floor.then(|| stepover_mm / (2.0 * slope))
}
