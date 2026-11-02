// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! V-Carve (`docs/specs/phase-3/design.md`, "Algorithms → V-Carve
//! generator"; `docs/specs/phase-3/functional.md`, capabilities 38–43):
//! a V-bit along the medial lines of each closed target, as deep as its
//! flanks touch both walls, in one pass; or, with the offset strategy, in
//! rings stepping in from the outline, each as deep as its inset.
//!
//! [`generate_vcarve`] checks the tool and the start plane, nests the
//! targets into regions, builds each region's blocks with [`blocks`]
//! (the medial strategy is the `medial` module, the offset strategy the
//! `offset` one), sequences them, and before it
//! returns runs the exact cone test, [`cone_in_material`], on its own
//! toolpath (step 10): a library crate never hands back motion it can
//! prove wrong.
//!
//! The medial strategy (#404) clamps at the flat depth, as PureCutCNC
//! does; the offset strategy's rings (#407) step in until the next would
//! reach it. Where a region is wider than the groove there, the `floor`
//! module traces the floor boundary and clears the floor with the V-bit
//! (#405), and the medial chains it covers are dropped. With clearing
//! passes (#406) the `clearing` module cuts the floor with each pass's end
//! mill first, one tool run per pass, and the V-bit clears only what they
//! cannot reach, whichever the strategy.

pub(crate) mod clearing;
mod cone;
pub(crate) mod floor;
pub(crate) mod medial;
mod offset;
#[cfg(test)]
mod tests;

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::pocket::rest::Reference;
use crate::profile::{crossing_error, outer_shape, rates_of};
use crate::run::{RunLabel, ToolRun};
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::toolpath::{MoveKind, Toolpath};
use crate::warning::Warning;
use cone::{OpenEdgeWalls, Walls};
use dc_geometry::medial::CornerReach;
use dc_geometry::{
    Contour, EPS_MM, GeometryWarning, Point3, Region, floor_regions, is_design_hole, nest,
};
use dc_project::{
    InstanceId, Operation, OperationId, Project, Tool, ToolKind, VCarveOp, VCarveStrategy,
    cleared_start_plane,
};
use floor::{Floor, FloorCut, OpenFloor};
use medial::{Depth, Tolerances};

/// What a V-bit carve is, independent of which operation asked for it:
/// V-Carve builds one from its targets, an Inlay one per side
/// ([`crate::inlay`]).
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
    /// flat depth, from the operation or the tool; `None` when neither
    /// sets one, which is [`CamError::MissingStepOver`] once there is a
    /// floor to cut (#405).
    pub step_over_fraction: Option<f64>,
    /// The clearing passes' tools, in order (#406).
    pub cleared_by: Vec<Clearing>,
    /// A male Inlay's band edge when an end mill cuts it upright (#448);
    /// `None` for every other carve, whose every wall is a V wall. Only
    /// the medial strategy honours it.
    pub open_edge: Option<OpenEdge>,
    /// A male Inlay's mirrored design, nested (#452): the plug's outline,
    /// which its band regions have as holes and its design-hole regions
    /// as every loop. `None` for every other carve.
    pub design: Option<Vec<Region>>,
}

impl Carve<'_> {
    /// Whether material may stand above the start plane where the carve
    /// cuts: the flank is engaged from above it (a male Inlay's plug,
    /// never cleared; a V-Carve whose start plane is cleared by other
    /// means).
    fn stands_above(&self) -> bool {
        self.engaged_from_z_mm > self.start_plane_z_mm + EPS_MM
    }

    /// How far the medial axis of `region` reaches into its corners, for
    /// a V-bit of `slope`: every convex corner of the loops the carve's
    /// walls are (#452, #456). A chain ending at a corner, its tip at the
    /// start plane, cuts the corner crisp, a disc of radius `h · tan(α/2)`
    /// at height `h` above the tip about it; upstream's passes leave some
    /// corners with no branch, which blunts a V-Carve's or a female's
    /// corner and, where material stands above the start plane (a male
    /// Inlay's plug), keeps wood up to the stock top there. A male's band
    /// region has the plug's outline as its holes only; its outer edge is
    /// not the plug's, so its corners keep upstream's reach.
    pub(crate) fn corner_reach(&self, region: &Region, slope: f64) -> CornerReach {
        CornerReach::Every {
            outer: self
                .design
                .as_ref()
                .is_none_or(|design| is_design_hole(region, design)),
            floor_inset_mm: self.flat_depth_mm.map(|f| f * slope),
        }
    }

    /// How far the medial chains of a region sampled every `step_mm` may
    /// be simplified (#452): upstream's tolerances, and where material
    /// may stand above the start plane, no stretch left more than
    /// [`DEEPEN_MM`] shallower than its own depth.
    pub(crate) fn tolerances(&self, step_mm: f64) -> Tolerances {
        Tolerances {
            deepen_mm: self.stands_above().then_some(DEEPEN_MM),
            ..Tolerances::at_step(step_mm)
        }
    }
}

/// How far a medial stretch may run shallower than its own depth where
/// material stands above the start plane, mm (#452): a hundredth of a
/// millimetre, below what the plug's seat can feel.
const DEEPEN_MM: f64 = 0.01;

/// The open outer edge of a male Inlay's band (#448;
/// `docs/specs/phase-3/design.md`, "Algorithms → Inlay", step 4). The
/// carve's regions are still the surround, `band` less `design`, and its
/// floor runs out to `band`'s edge, which the edge pass cuts upright. The
/// V-bit carves only the walls against `design` and the floor no pass
/// reaches, and its cone never crosses `band`'s edge below the stock top,
/// where the board stands beyond it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenEdge {
    /// The band, `B`: the design grown by the margin and closed by the
    /// edge's rounding ([`dc_geometry::rounded_band`]).
    pub band: Vec<Region>,
    /// The mirrored design, nested.
    pub design: Vec<Region>,
    /// The stock top, mm.
    pub top_z_mm: f64,
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

/// The height, mm, of the ridges the V-bit leaves between its floor rings
/// in `op` (`docs/specs/phase-3/design.md`, "V-Carve generator", step 6):
/// two grooves `s` apart at the flat depth leave a ridge `s / (2 tan(α/2))`
/// high, which with `s = step_over_fraction · 2 F tan(α/2)` is
/// `step_over_fraction · F`. `None` when the carve has no floor for the
/// V-bit to clear: no flat depth, no usable V-bit or step-over fraction,
/// or no region wide enough.
#[must_use]
pub fn floor_ridge_height_mm(project: &Project, op: &VCarveOp) -> Option<f64> {
    floor::ridge_height_mm(project, op)
}

/// A usable V-bit's slope and cone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VBit {
    /// `tan(α/2)`.
    pub slope: f64,
    /// Cone height, mm.
    pub cone_mm: f64,
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
pub(crate) fn checked_v_bit(op: &OperationId, tool: &Tool) -> Result<VBit, CamError> {
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
    let walls = Walls::new(regions, tolerance_mm);
    let tol = tolerance_mm.max(0.0) + EPS_MM;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() {
            continue;
        }
        let b = mv.to;
        if !matches!(mv.kind, MoveKind::Linear) {
            return Err(ConeViolation::NotStraight { at: b });
        }
        let vertical = a.xy().distance_to(b.xy()) <= EPS_MM;
        let low = if a.z <= b.z { a } else { b };
        if vertical && low.z > start_plane_z_mm + tol {
            continue;
        }
        let high = if a.z <= b.z { b } else { a };
        if !vertical && high.z > start_plane_z_mm + tol {
            return Err(ConeViolation::Wall {
                at: high,
                overlap_mm: high.z - start_plane_z_mm,
            });
        }
        let engaged_mm = engaged_from_z_mm - low.z;
        if engaged_mm > cone_mm + tol {
            return Err(ConeViolation::Shank {
                at: low,
                engaged_mm,
            });
        }
        let (tip_from, tip_to) = if vertical { (low, low) } else { (a, b) };
        // #463: the overlap is read where the tip is farthest outside, not
        // at the crossings, where it is 0.
        if let Some(out) = walls.excursion(tip_from.xy(), tip_to.xy(), tol) {
            let at = Point3::new(
                tip_from.x + (tip_to.x - tip_from.x) * out.t,
                tip_from.y + (tip_to.y - tip_from.y) * out.t,
                tip_from.z + (tip_to.z - tip_from.z) * out.t,
            );
            return Err(ConeViolation::Wall {
                at,
                overlap_mm: out.outside_mm + (start_plane_z_mm - at.z).max(0.0) * slope,
            });
        }
        let (room_mm, at) = walls.least_room(tip_from, tip_to, start_plane_z_mm, slope);
        if room_mm < -tol {
            return Err(ConeViolation::Wall {
                at,
                overlap_mm: -room_mm,
            });
        }
    }
    Ok(())
}

/// The cut blocks of `carve` for operation `op`, cut with `tool` (a
/// V-bit), in region order and, within a region, in the generator's cut
/// order (`docs/specs/phase-3/design.md`, "V-Carve generator", steps 3–6
/// and 8). Each block is one run of medial chains, or of offset rings,
/// the bit cuts without lifting, at pass 0, entered by a straight plunge
/// to its first point's Z; with a flat depth, a region's runs are followed
/// by its floor's rings and boundary trace (`floor::blocks`), and the
/// medial chains wholly on that floor are not cut.
///
/// # Errors
///
/// [`CamError::VCarveNeedsVBit`] or [`CamError::VBitAngleRange`] for an
/// unusable tool, a missing rate, [`CamError::UnknownOperation`],
/// [`CamError::MissingThickness`], [`CamError::VBitConeExceeded`] when a
/// region's deepest point (the offset strategy's deepest ring, or its
/// floor) engages the flank past the cone,
/// [`CamError::BelowMaterialBottom`] when it is below the stock bottom
/// less the allowance (S1), [`CamError::MissingStepOver`] or
/// [`CamError::InvalidStepOver`] when a floor is to be cut without a
/// usable `step_over_fraction`, and [`CamError::InvalidVCarveStepOver`]
/// for an offset step-over that is not positive or would need more than
/// [`crate::pocket::MAX_RINGS`] rings in a region.
///
/// With `carve.cleared_by` the V-bit's floor is what no clearing pass
/// reaches (design step 7): the boundary trace is the whole floor's, and the
/// rings clear each floor region less the passes' reach, from their radii.
pub fn blocks(
    project: &Project,
    op: &OperationId,
    tool: &Tool,
    carve: &Carve<'_>,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<CutBlock>, CamError> {
    let bit = checked_v_bit(op, tool)?;
    let rates = rates_of(tool)?;
    let operation = project
        .operations
        .iter()
        .find(|o| o.id() == op)
        .ok_or_else(|| CamError::UnknownOperation { op: op.clone() })?;
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let safe_z_mm = thickness_mm + operation.safe_z_mm();
    let floor_z_mm = -operation.through_cut_allowance_mm();
    if let VCarveStrategy::Offset { step_over_mm, .. } = *carve.strategy
        && !(step_over_mm.is_finite() && step_over_mm > 0.0)
    {
        return Err(CamError::InvalidVCarveStepOver {
            op: op.clone(),
            step_over_mm,
        });
    }
    let reached_by: Vec<Reference> = carve
        .cleared_by
        .iter()
        .map(|pass| clearing::reference(pass.radius_mm))
        .collect();
    let depth = Depth {
        start_plane_z_mm: carve.start_plane_z_mm,
        flat_depth_mm: carve.flat_depth_mm,
        slope: bit.slope,
    };
    let flat = Floor::of(carve, bit.slope);
    let tolerance_mm = project.flatten_tolerance_mm;
    // #448: the open edge, as the medial chains and the floor see it.
    let edge_walls = carve.open_edge.as_ref().map(|edge| OpenEdgeWalls {
        walls: Walls::new(&edge.band, tolerance_mm),
        top_z_mm: edge.top_z_mm,
        slope: bit.slope,
    });
    let open_floor = carve
        .open_edge
        .as_ref()
        .zip(flat.as_ref())
        .zip(edge_walls.as_ref())
        .map(|((edge, flat), walls)| OpenFloor {
            tip_room: floor_regions(
                &edge.band,
                (edge.top_z_mm - flat.z_mm) * bit.slope,
                tolerance_mm,
            ),
            band: walls.walls.clone(),
            design: Walls::new(&edge.design, tolerance_mm),
        });
    let is_band = |region: &Region| {
        carve
            .open_edge
            .as_ref()
            .is_some_and(|edge| !is_design_hole(region, &edge.design))
    };
    let mut budget_warned = false;
    let mut out = Vec::new();
    let deepest = |shape: &ShapeRef, deepest_z_mm: f64| {
        deepest_is_cut(
            op,
            shape,
            bit,
            carve,
            deepest_z_mm,
            floor_z_mm,
            thickness_mm,
        )
    };
    for (region, shape) in carve.regions.iter().zip(&carve.shapes) {
        if let VCarveStrategy::Offset {
            step_over_mm,
            direction,
        } = *carve.strategy
        {
            let levels =
                offset::levels(region, step_over_mm, &depth, tolerance_mm).ok_or_else(|| {
                    CamError::InvalidVCarveStepOver {
                        op: op.clone(),
                        step_over_mm,
                    }
                })?;
            // Step 8: the ring at the flat depth is the floor's boundary
            // trace, cut with the V-bit floor (step 6).
            let floors = flat
                .map(|f| f.regions(region, tolerance_mm))
                .unwrap_or_default();
            // Step 3: the offset strategy's deepest point is its deepest
            // ring, or its floor.
            let floor_z_mm = flat.filter(|_| !floors.is_empty()).map(|f| f.z_mm);
            if let Some(z_mm) = levels
                .last()
                .map(|l| l.z_mm)
                .into_iter()
                .chain(floor_z_mm)
                .reduce(f64::min)
            {
                deepest(shape, z_mm)?;
            }
            let walls = Walls::new(std::slice::from_ref(region), tolerance_mm);
            let runs = offset::runs(&levels, direction, &walls, &depth, tolerance_mm);
            let mut anchor = runs
                .last()
                .map(|run| run.steps.last().map_or(run.start, |s| s.to).xy());
            out.extend(
                runs.iter()
                    .map(|run| medial::run_block(run, shape, rates, safe_z_mm)),
            );
            if let Some(flat) = &flat {
                let cut = FloorCut {
                    op,
                    tool,
                    shape,
                    rates,
                    safe_z_mm,
                    tolerance_mm,
                };
                out.extend(floor::blocks(
                    flat,
                    &floors,
                    &reached_by,
                    &cut,
                    &mut anchor,
                )?);
            }
            continue;
        }
        let Some((graph, step_mm)) = medial::region_graph(
            region,
            tolerance_mm,
            carve.corner_reach(region, bit.slope),
            op,
            shape,
            &mut budget_warned,
            warnings,
        ) else {
            continue;
        };
        // Step 3: the deepest point, from the widest node, engages the
        // flank from `engaged_from`; below the cone the shank cuts.
        let widest_mm = graph
            .nodes
            .iter()
            .map(|n| n.clearance_mm)
            .fold(0.0, f64::max);
        deepest(shape, depth.z_mm(widest_mm))?;
        // Step 6: the floor, where the region is wider than the groove at
        // the flat depth; the chains it covers are dropped.
        // #448: a band region with an open edge floors out to the edge,
        // and its V-bit keeps its cone off the edge.
        let band = is_band(region);
        let floors = flat
            .map(|f| f.regions_of(carve, region, tolerance_mm))
            .unwrap_or_default();
        let walls = Walls::new(std::slice::from_ref(region), tolerance_mm);
        let runs = medial::runs_within(
            &graph,
            &depth,
            carve.tolerances(step_mm),
            &walls,
            &floors,
            edge_walls.as_ref().filter(|_| band),
        );
        let mut anchor = runs
            .last()
            .map(|run| run.steps.last().map_or(run.start, |s| s.to).xy());
        out.extend(
            runs.iter()
                .map(|run| medial::run_block(run, shape, rates, safe_z_mm)),
        );
        if let Some(flat) = &flat {
            let cut = FloorCut {
                op,
                tool,
                shape,
                rates,
                safe_z_mm,
                tolerance_mm,
            };
            out.extend(floor::blocks_open(
                flat,
                &floors,
                &reached_by,
                &cut,
                &mut anchor,
                open_floor.as_ref().filter(|_| band),
            )?);
        }
    }
    Ok(out)
}

/// Step 3 and S1 for one region of `carve` (operation `op`, V-bit `bit`):
/// its deepest point, at `deepest_z_mm`, must not engage the flank from
/// `engaged_from_z_mm` past the cone ([`CamError::VBitConeExceeded`]),
/// nor lie below `floor_z_mm`, the stock bottom less the allowance
/// ([`CamError::BelowMaterialBottom`]).
fn deepest_is_cut(
    op: &OperationId,
    shape: &ShapeRef,
    bit: VBit,
    carve: &Carve<'_>,
    deepest_z_mm: f64,
    floor_z_mm: f64,
    thickness_mm: f64,
) -> Result<(), CamError> {
    let engaged_mm = carve.engaged_from_z_mm - deepest_z_mm;
    if engaged_mm > bit.cone_mm + EPS_MM {
        return Err(CamError::VBitConeExceeded {
            op: op.clone(),
            shape: Box::new(shape.clone()),
            depth_mm: engaged_mm,
            cone_mm: bit.cone_mm,
        });
    }
    if deepest_z_mm < floor_z_mm - EPS_MM {
        return Err(CamError::BelowMaterialBottom {
            op: op.clone(),
            depth_mm: thickness_mm - deepest_z_mm,
            limit_mm: thickness_mm - floor_z_mm,
        });
    }
    Ok(())
}

/// Generates the V-Carve toolpaths for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness +
/// op.safe_z_mm`): one [`ToolRun`] per clearing pass that reaches some of
/// the floor, [`RunLabel::Clearing`] with the pass's end mill, in list
/// order (the `clearing` module), then [`RunLabel::Main`] with the
/// operation's V-bit.
///
/// The targets' closed outlines are nested even-odd into regions (an open
/// path warns [`Warning::VCarveTargetOpen`] and is skipped); each region
/// is carved along its medial lines, every point as deep below the start
/// plane as the bit's flanks touch both walls, `clearance / tan(α/2)`,
/// clamped at the flat depth, in one pass whatever the depth. With the
/// offset strategy each region is instead cut in rings stepping in by
/// `step_over_mm`, each `inset / tan(α/2)` deep, every ring run per
/// `direction` as a Pocket's rings, until the next would reach the flat
/// depth. Where a region is wider than the groove at the flat depth, its
/// floor is traced and cleared by the V-bit at that depth (step 6). Regions are
/// visited nearest-neighbour from the first in drawing order, a region
/// inside another's hole first; within a region, runs of chains or rings
/// in the generator's order, each entered by a straight plunge at the
/// plunge rate from the entry clearance above the stock top. Before
/// returning, the toolpath passes [`cone_in_material`].
///
/// # Errors
///
/// As [`blocks`], and: an unknown tool or instance, a missing rate or
/// thickness, a safe Z or flat depth that is not positive,
/// [`CamError::ClearingInvalid`] for a clearing pass that cannot run, what
/// the Pocket code refuses for a pass,
/// [`CamError::InvalidStartDepth`], [`CamError::StartDepthUncleared`]
/// (security review M2), crossing targets, [`CamError::OffStock`], and
/// [`CamError::ConeOffMaterial`] when the generator's own toolpath fails
/// the cone test.
#[must_use = "the toolpath is the whole point"]
pub fn generate_vcarve(
    project: &Project,
    op: &VCarveOp,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    checked_v_bit(&op.id, tool)?;
    rates_of(tool)?;
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    if let Some(flat_depth_mm) = op.flat_depth_mm
        && !(flat_depth_mm.is_finite() && flat_depth_mm > 0.0)
    {
        return Err(CamError::InvalidDepth {
            op: op.id.clone(),
            depth_mm: flat_depth_mm,
        });
    }
    let passes = clearing::Passes::of_v_carve(op);
    let clearing_tools = clearing::checked_tools(
        project,
        &passes,
        op.flat_depth_mm.is_some_and(|f| f.is_finite() && f > 0.0),
    )?;
    let (start_plane_z_mm, engaged_from_z_mm) = start_plane(project, op)?;
    let mut warnings = Vec::new();
    let Targets {
        regions,
        shapes,
        containment,
        ..
    } = carve_regions(project, &op.id, &op.targets, &mut warnings)?;
    let carve = Carve {
        regions,
        shapes,
        start_plane_z_mm,
        engaged_from_z_mm,
        flat_depth_mm: op.flat_depth_mm,
        strategy: &op.strategy,
        step_over_fraction: op.step_over_fraction.or(tool.step_over_fraction),
        cleared_by: clearing_tools
            .iter()
            .map(|t| Clearing {
                radius_mm: t.diameter_mm / 2.0,
            })
            .collect(),
        open_edge: None,
        design: None,
    };
    let bit = checked_v_bit(&op.id, tool)?;
    let mut runs = clearing::runs(
        project,
        &passes,
        &carve,
        &clearing_tools,
        bit.slope,
        &mut warnings,
    )?;
    let blocks = blocks(project, &op.id, tool, &carve, &mut warnings)?;
    let toolpath = finish(
        project,
        &op.id,
        tool,
        &carve,
        blocks,
        &containment,
        op.machining_order,
    )?;
    runs.push(ToolRun {
        label: RunLabel::Main,
        tool: tool.id.clone(),
        toolpath,
    });
    Ok((runs, warnings))
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
    let bit = checked_v_bit(op, tool)?;
    let rates = rates_of(tool)?;
    let operation = project
        .operations
        .iter()
        .find(|o| o.id() == op)
        .ok_or_else(|| CamError::UnknownOperation { op: op.clone() })?;
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let safe_z_mm = thickness_mm + operation.safe_z_mm();
    let input = SequenceInput {
        blocks,
        order,
        containment,
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        stock_top_mm: thickness_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        cut_before: &[],
    };
    let toolpath = sequence(&input)?;
    // #62: the cutter must stay on the stock.
    if let Some(overhang) = swept_bounds(&toolpath, tool.diameter_mm / 2.0)
        .and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.clone(),
            overhang,
        });
    }
    cone_in_material(
        &toolpath,
        &carve.regions,
        carve.start_plane_z_mm,
        carve.engaged_from_z_mm,
        bit.slope,
        bit.cone_mm,
        project.flatten_tolerance_mm,
    )
    .and_then(|()| {
        // #448: beyond an open band edge the board stands to the stock
        // top, so the cone is held to the edge there.
        carve.open_edge.as_ref().map_or(Ok(()), |edge| {
            cone_in_material(
                &toolpath,
                &edge.band,
                edge.top_z_mm,
                edge.top_z_mm,
                bit.slope,
                bit.cone_mm,
                project.flatten_tolerance_mm,
            )
        })
    })
    .map_err(|violation| {
        let at = match violation {
            ConeViolation::Wall { at, .. }
            | ConeViolation::Shank { at, .. }
            | ConeViolation::NotStraight { at } => at,
        };
        let shape = Box::new(shape_at(carve, at));
        match violation {
            ConeViolation::Shank { engaged_mm, .. } => CamError::VBitConeExceeded {
                op: op.clone(),
                shape,
                depth_mm: engaged_mm,
                cone_mm: bit.cone_mm,
            },
            ConeViolation::Wall { overlap_mm, .. } => CamError::ConeOffMaterial {
                op: op.clone(),
                shape,
                x_mm: at.x,
                y_mm: at.y,
                z_mm: at.z,
                overlap_mm,
            },
            ConeViolation::NotStraight { .. } => CamError::ConeArcMove {
                op: op.clone(),
                shape,
                x_mm: at.x,
                y_mm: at.y,
                z_mm: at.z,
            },
        }
    })?;
    Ok(toolpath)
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
/// stock top unless an earlier floor at the start depth holds the
/// targets, an enabled Pocket's or an enabled V-Carve's flat floor
/// ([`cleared_start_plane`], depth within [`EPS_MM`]), as validation
/// decides it.
///
/// # Errors
///
/// [`CamError::MissingThickness`], [`CamError::InvalidStartDepth`] for a
/// start depth that is not from 0 to the thickness, and
/// [`CamError::StartDepthUncleared`] for one below the stock top that no
/// earlier Pocket contains and the operation does not vouch for.
pub(crate) fn start_plane(project: &Project, op: &VCarveOp) -> Result<(f64, f64), CamError> {
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let start_depth_mm = op.start_depth_mm;
    if !(start_depth_mm.is_finite() && start_depth_mm >= 0.0 && start_depth_mm <= thickness_mm) {
        return Err(CamError::InvalidStartDepth {
            op: op.id.clone(),
            start_depth_mm,
        });
    }
    let start_plane_z_mm = thickness_mm - start_depth_mm;
    if start_depth_mm <= 0.0 {
        return Ok((start_plane_z_mm, start_plane_z_mm));
    }
    let cleared = cleared_start_plane(project, &Operation::VCarve(op.clone()));
    if cleared.is_none() && !op.start_plane_cleared_externally {
        return Err(CamError::StartDepthUncleared {
            op: op.id.clone(),
            start_depth_mm,
        });
    }
    let engaged_from_z_mm = if cleared.is_some_and(|plane| plane.is_at(start_depth_mm)) {
        start_plane_z_mm
    } else {
        thickness_mm
    };
    Ok((start_plane_z_mm, engaged_from_z_mm))
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
    carve_regions_then(project, op, targets, None, warnings)
}

/// [`carve_regions`] with every placed outline then moved by `then` (a
/// male Inlay's mirror, [`crate::inlay::male_mirror`]) before it is
/// nested; `None` moves nothing.
pub(crate) fn carve_regions_then(
    project: &Project,
    op: &OperationId,
    targets: &[InstanceId],
    then: Option<dc_geometry::Transform>,
    warnings: &mut Vec<Warning>,
) -> Result<Targets, CamError> {
    let mut shapes: Vec<(ShapeRef, Contour)> = Vec::new();
    for instance_id in targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op.clone(),
                instance: instance_id.clone(),
            })?;
        let definition = project
            .definitions
            .get(&instance.definition)
            .ok_or_else(|| CamError::UnknownDefinition {
                instance: instance.id.clone(),
                definition: instance.definition.clone(),
            })?;
        for (profile, polyline) in definition.profiles.iter().enumerate() {
            let shape = ShapeRef {
                instance: instance.id.clone(),
                instance_name: instance.name.clone(),
                profile,
            };
            if !polyline.is_closed() {
                warnings.push(Warning::VCarveTargetOpen {
                    op: op.clone(),
                    shape,
                });
                continue;
            }
            let contour = instance
                .transform
                .apply_polyline(polyline)
                .and_then(|placed| match then {
                    Some(then) => then.apply_polyline(&placed),
                    None => Ok(placed),
                })
                .and_then(Contour::new)
                .map_err(|source| CamError::InvalidTarget {
                    shape: shape.clone(),
                    source,
                })?;
            shapes.push((shape, contour));
        }
    }
    let contours: Vec<Contour> = shapes.iter().map(|(_, c)| c.clone()).collect();
    let (regions, nest_warnings) = nest(contours).map_err(|e| crossing_error(e, &shapes))?;
    for warning in nest_warnings {
        let GeometryWarning::DuplicateContourDropped {
            index,
            duplicate_of,
        } = warning;
        if let (Some((dropped, _)), Some((kept, _))) = (shapes.get(index), shapes.get(duplicate_of))
        {
            warnings.push(Warning::DuplicateShapeDropped {
                shape: dropped.clone(),
                duplicate_of: kept.clone(),
            });
        }
    }
    let region_shapes: Vec<ShapeRef> = regions
        .iter()
        .filter_map(|r| outer_shape(r, &shapes).cloned())
        .collect();
    let mut containment = Vec::new();
    for (region, inner) in regions.iter().zip(&region_shapes) {
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let Some(probe) = region.outer().polyline().vertices().first() else {
            continue;
        };
        let around = regions
            .iter()
            .zip(&region_shapes)
            .filter(|(r, _)| r.depth() == parent_depth)
            .find(|(r, _)| r.holes().iter().any(|h| hole_contains(h, probe.point())));
        if let Some((_, outer)) = around {
            containment.push((inner.clone(), outer.clone()));
        }
    }
    Ok(Targets {
        regions,
        shapes: region_shapes,
        containment,
        contours: shapes,
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
    /// Every closed outline placed, with its shape, in target then profile
    /// order, before nesting.
    pub contours: Vec<(ShapeRef, Contour)>,
}

/// True when `p` is inside hole `contour` (either winding).
fn hole_contains(contour: &Contour, p: dc_geometry::Point) -> bool {
    crate::profile::contour_contains(contour, p)
}
