// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! V-Carve (`docs/specs/phase-3/design.md`, "Algorithms → V-Carve
//! generator"; `docs/specs/phase-3/functional.md`, capabilities 38–43):
//! a V-bit along the medial lines of each closed target, as deep as its
//! flanks touch both walls, in one pass.
//!
//! [`generate_vcarve`] checks the tool and the start plane, nests the
//! targets into regions, builds each region's blocks with [`blocks`]
//! (the medial strategy is [`medial`]), sequences them, and before it
//! returns runs the exact cone test, [`cone_in_material`], on its own
//! toolpath (step 10): a library crate never hands back motion it can
//! prove wrong.
//!
//! This slice (#404) carves with the medial strategy and clamps at the
//! flat depth only, as PureCutCNC does; the floor trace and the V-bit
//! floor (#405), clearing passes (#406) and the offset strategy (#407)
//! are refused with [`CamError::NotSupportedYet`] until they land.

mod cone;
pub(crate) mod medial;
#[cfg(test)]
mod tests;

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::profile::{crossing_error, outer_shape, rates_of};
use crate::run::{RunLabel, ToolRun};
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::toolpath::{MoveKind, Toolpath};
use crate::warning::Warning;
use cone::Walls;
use dc_geometry::{Contour, EPS_MM, GeometryWarning, Point3, Region, nest};
use dc_project::{
    InstanceId, Operation, OperationId, Project, Tool, ToolKind, VCarveOp, VCarveStrategy,
    containing_pocket,
};
use medial::{Depth, Tolerances};

/// What a V-bit carve is, independent of which operation asked for it:
/// V-Carve builds one from its targets (an Inlay will build one per side).
/// Project space: mm, Y-up, Z-up, stock bottom at Z = 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Carve<'a> {
    /// The carved area: the targets nested even-odd, outers
    /// counter-clockwise and holes clockwise.
    pub regions: Vec<Region>,
    /// The shape each region's outer came from, one per region, which the
    /// region's blocks and errors name.
    pub shapes: Vec<ShapeRef>,
    /// The start plane, mm: `thickness − start_depth`. Every depth of the
    /// carve counts from it.
    pub start_plane_z_mm: f64,
    /// The Z, mm, the flank is engaged from: the stock top, or the start
    /// plane when an enabled earlier Pocket has cleared it (security
    /// review H4). The cone check uses `engaged_from_z_mm − deepest Z`.
    pub engaged_from_z_mm: f64,
    /// The flat depth below the start plane, mm; `None` carves to full
    /// depth.
    pub flat_depth_mm: Option<f64>,
    /// Medial lines or offset rings.
    pub strategy: &'a VCarveStrategy,
    /// The V-bit floor's step-over as a fraction of its cut width at the
    /// flat depth (#405), from the operation or the tool; `None` when
    /// neither sets one.
    pub step_over_fraction: Option<f64>,
    /// The clearing passes' tools, in order (#406).
    pub cleared_by: Vec<Clearing>,
}

/// One clearing pass of a [`Carve`], as the carve needs it: what its end
/// mill can reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clearing {
    /// The end mill's radius, mm.
    pub radius_mm: f64,
}

/// Why [`cone_in_material`] refused a toolpath.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConeViolation {
    /// The cone's disc at the start plane crosses a wall by `overlap_mm`
    /// at `at`, or the tip leaves the carved area, or the move cuts
    /// sideways `overlap_mm` above the start plane.
    Wall {
        /// The worst point, mm, project space.
        at: Point3,
        /// How far past the wall (or above the start plane), mm.
        overlap_mm: f64,
    },
    /// The flank is engaged `engaged_mm` deep at `at`, past the cone.
    Shank {
        /// The point, mm.
        at: Point3,
        /// The engagement, mm.
        engaged_mm: f64,
    },
    /// An arc move: the test is for the straight moves a V-bit carve is
    /// made of.
    NotStraight {
        /// Where the arc ends, mm.
        at: Point3,
    },
}

/// A V-bit's cone height, mm: the depth at which its flanks reach its full
/// diameter, `diameter / (2 tan(α/2))`. `None` for any other cutter, or a
/// V-bit with an angle outside (0°, 180°) or no positive diameter.
#[must_use]
pub fn cone_height_mm(tool: &Tool) -> Option<f64> {
    v_bit(tool).map(|bit| bit.cone_mm)
}

/// A usable V-bit's slope and cone.
#[derive(Debug, Clone, Copy, PartialEq)]
struct VBit {
    /// `tan(α/2)`.
    slope: f64,
    /// Cone height, mm.
    cone_mm: f64,
}

fn v_bit(tool: &Tool) -> Option<VBit> {
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    let usable = included_angle_deg.is_finite()
        && included_angle_deg > 0.0
        && included_angle_deg < 180.0
        && tool.diameter_mm.is_finite()
        && tool.diameter_mm > 0.0;
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    (usable && slope.is_finite() && slope > 0.0).then(|| VBit {
        slope,
        cone_mm: tool.diameter_mm / (2.0 * slope),
    })
}

/// The V-bit of operation `op`, or the error that says why `tool` is not
/// one: [`CamError::VCarveNeedsVBit`], [`CamError::VBitAngleRange`] or
/// [`CamError::InvalidToolDiameter`].
fn checked_v_bit(op: &OperationId, tool: &Tool) -> Result<VBit, CamError> {
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return Err(CamError::VCarveNeedsVBit {
            op: op.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    };
    if !(included_angle_deg.is_finite() && included_angle_deg > 0.0 && included_angle_deg < 180.0) {
        return Err(CamError::VBitAngleRange {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            angle_deg: included_angle_deg,
        });
    }
    v_bit(tool).ok_or_else(|| CamError::InvalidToolDiameter {
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        diameter_mm: tool.diameter_mm,
    })
}

/// Generator-side S17 (`docs/specs/phase-3/design.md`, "Safety pass",
/// the S17 bullet; step 10): the exact test, on `toolpath`, that a V-bit
/// of `slope` (`tan(α/2)`) and cone height `cone_mm` cuts only inside
/// `regions` (mm, Y-up; arcs flattened to chords within `tolerance_mm`,
/// as the medial axis flattens them) below the start plane
/// `start_plane_z_mm`, with its flanks engaged from `engaged_from_z_mm`.
///
/// For every cutting move: a move with no XY motion is a descent or a
/// rise in place and is checked at its lower end only, and not at all
/// when that end is above the start plane (the descent exemption,
/// security review H3: it is in the air or in material above the start
/// plane the project vouches for). Every other move must not run above
/// the start plane; its tip path must stay inside the regions; the cone's
/// disc at the start plane, radius `(start_plane − z) · slope`, must stay
/// inside them everywhere along it, found exactly (the room to the walls
/// less the radius is convex along a straight move, so its minimum is
/// searched, not sampled); and the engagement `engaged_from − z` must not
/// pass the cone. Each limit is allowed `tolerance_mm` (plus [`EPS_MM`]),
/// the flattening's own error. Rapids and dwells are not checked (S2
/// keeps rapids in the air).
///
/// # Errors
///
/// The first violation, as a [`ConeViolation`].
pub fn cone_in_material(
    toolpath: &Toolpath,
    regions: &[Region],
    start_plane_z_mm: f64,
    engaged_from_z_mm: f64,
    slope: f64,
    cone_mm: f64,
    tolerance_mm: f64,
) -> Result<(), ConeViolation> {
    let _ = (
        toolpath,
        regions,
        start_plane_z_mm,
        engaged_from_z_mm,
        slope,
        cone_mm,
        tolerance_mm,
    );
    Ok(())
}

/// The cut blocks of `carve` for operation `op`, cut with `tool` (a
/// V-bit), in region order and, within a region, in the generator's cut
/// order (`docs/specs/phase-3/design.md`, "V-Carve generator", steps 3–5).
/// Each block is one run of medial chains the bit cuts without lifting,
/// at pass 0, entered by a straight plunge to its first point's Z.
///
/// # Errors
///
/// [`CamError::VCarveNeedsVBit`] or [`CamError::VBitAngleRange`] for an
/// unusable tool, a missing rate, [`CamError::UnknownOperation`],
/// [`CamError::MissingThickness`], [`CamError::VBitConeExceeded`] when a
/// region's deepest point engages the flank past the cone,
/// [`CamError::BelowMaterialBottom`] when it is below the stock bottom
/// less the allowance (S1), and [`CamError::NotSupportedYet`] for the
/// offset strategy and clearing passes.
pub fn blocks(
    project: &Project,
    op: &OperationId,
    tool: &Tool,
    carve: &Carve<'_>,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<CutBlock>, CamError> {
    let _ = (project, tool, carve, warnings);
    Err(CamError::NotSupportedYet {
        op: op.clone(),
        feature: "V-carving (#404)".into(),
    })
}

/// Generates the V-Carve toolpath for `op` in `project`: one
/// [`ToolRun`], [`RunLabel::Main`], with the operation's V-bit, in project
/// space (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness +
/// op.safe_z_mm`).
///
/// The targets' closed outlines are nested even-odd into regions (an open
/// path warns [`Warning::VCarveTargetOpen`] and is skipped); each region
/// is carved along its medial lines, every point as deep below the start
/// plane as the bit's flanks touch both walls, `clearance / tan(α/2)`,
/// clamped at the flat depth, in one pass whatever the depth. Regions are
/// visited nearest-neighbour from the first in drawing order, a region
/// inside another's hole first; within a region, runs of chains in the
/// generator's order, each entered by a straight plunge at the plunge
/// rate from the entry clearance above the stock top. Before returning,
/// the toolpath passes [`cone_in_material`].
///
/// # Errors
///
/// As [`blocks`], and: an unknown tool or instance, a missing rate or
/// thickness, a safe Z or flat depth that is not positive,
/// [`CamError::InvalidStartDepth`], [`CamError::StartDepthUncleared`]
/// (security review M2), crossing targets, [`CamError::OffStock`], and
/// [`CamError::ConeOffMaterial`] when the generator's own toolpath fails
/// the cone test.
#[must_use = "the toolpath is the whole point"]
pub fn generate_vcarve(
    project: &Project,
    op: &VCarveOp,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    let _ = project;
    Err(CamError::NotSupportedYet {
        op: op.id.clone(),
        feature: "V-carving (#404)".into(),
    })
}

/// Steps 9 and 10: sequences `blocks` of `carve` (operation `op`, `tool`)
/// with `containment` by `order`, checks the cutter stays on the stock
/// (#62, with the full tool radius as S7 reads it), and refuses the result
/// unless it passes [`cone_in_material`].
pub(crate) fn finish(
    project: &Project,
    op: &OperationId,
    tool: &Tool,
    carve: &Carve<'_>,
    blocks: Vec<CutBlock>,
    containment: &[(ShapeRef, ShapeRef)],
    order: dc_project::MachiningOrder,
) -> Result<Toolpath, CamError> {
    let _ = (project, tool, carve, blocks, containment, order);
    Err(CamError::NotSupportedYet {
        op: op.clone(),
        feature: "V-carving (#404)".into(),
    })
}

/// The shape of the region of `carve` that holds `at`, or else the one
/// whose walls are nearest it.
fn shape_at(carve: &Carve<'_>, at: Point3) -> ShapeRef {
    let p = at.xy();
    let holding = carve
        .regions
        .iter()
        .position(|r| r.contains_point(p))
        .or_else(|| {
            carve
                .regions
                .iter()
                .map(|r| Walls::new(std::slice::from_ref(r), EPS_MM).clearance_mm(p))
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
    holding
        .and_then(|i| carve.shapes.get(i))
        .or_else(|| carve.shapes.first())
        .cloned()
        .unwrap_or_else(|| ShapeRef {
            instance: InstanceId::new(""),
            instance_name: String::new(),
            profile: 0,
        })
}

/// The start plane of `op` and the Z its flank is engaged from, mm
/// (`docs/specs/phase-3/design.md`, "V-Carve generator", steps 2 and 3;
/// security review H4, M2): `thickness − start_depth`, engaged from the
/// stock top unless an enabled earlier Pocket at the start depth
/// ([`containing_pocket`], depth within [`EPS_MM`]) contains the targets,
/// as validation decides it.
///
/// # Errors
///
/// [`CamError::MissingThickness`], [`CamError::InvalidStartDepth`] for a
/// start depth that is not from 0 to the thickness, and
/// [`CamError::StartDepthUncleared`] for one below the stock top that no
/// earlier Pocket contains and the operation does not vouch for.
pub(crate) fn start_plane(project: &Project, op: &VCarveOp) -> Result<(f64, f64), CamError> {
    let _ = project;
    Err(CamError::NotSupportedYet {
        op: op.id.clone(),
        feature: "V-carving (#404)".into(),
    })
}

/// The regions of `targets` (operation `op`), with the shape each outer
/// came from and the containment pairs for `sequence`: closed profiles
/// placed and nested even-odd, as a Pocket nests them; an open path warns
/// [`Warning::VCarveTargetOpen`] and is skipped; a duplicate outline warns
/// [`Warning::DuplicateShapeDropped`]. A region inside another's hole is
/// cut first (capability 38: a shape before any shape enclosing it).
///
/// # Errors
///
/// An unknown instance or definition, a target that is not a valid
/// outline, or crossing outlines ([`CamError::TargetsCross`]).
pub(crate) fn carve_regions(
    project: &Project,
    op: &OperationId,
    targets: &[InstanceId],
    warnings: &mut Vec<Warning>,
) -> Result<Targets, CamError> {
    let _ = (project, targets, warnings);
    Err(CamError::NotSupportedYet {
        op: op.clone(),
        feature: "V-carving (#404)".into(),
    })
}

/// A carve's targets, resolved: [`carve_regions`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Targets {
    /// The regions, nested even-odd.
    pub regions: Vec<Region>,
    /// The shape of each region's outer.
    pub shapes: Vec<ShapeRef>,
    /// `(inner, encloser)` pairs for `sequence`.
    pub containment: Vec<(ShapeRef, ShapeRef)>,
}

/// True when `p` is inside hole `contour` (either winding).
fn hole_contains(contour: &Contour, p: dc_geometry::Point) -> bool {
    crate::profile::contour_contains(contour, p)
}
