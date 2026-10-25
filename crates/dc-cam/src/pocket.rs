// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts
//   src/engine/toolpaths/pocketPatterns.ts

//! Pocket toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Pocket",
//! steps 1–12): clear the inside of closed shapes with concentric
//! rings or raster lines, leaving nested shapes as islands and, with a
//! stock to leave, a band of material on every wall. The ring tree is in
//! [`rings`], the raster lines in [`raster`], rest machining's leftover in
//! [`rest`], each pass's ramp or helix in [`crate::entry`], and the corner
//! relief notches in the `relief` module.
//!
//! Re-derived for Y-up (ADR-0001) rather than copied: PureCutCNC's "conventional
//! is a no-op on Clipper output" holds because its positive-area outers are
//! machine-clockwise in Y-down screen space. Here a positive-area (CCW)
//! outer ring is the climb direction on a pocket wall, and a clockwise
//! island ring is climb round an island, so conventional reverses both
//! ([`ring_is_ccw`]).

pub mod raster;
pub mod rest;
pub mod rings;

use crate::block::CutBlock;
use crate::entry::{self, CutSide, EntryDomain, EntryPlan, EntryTool, EntryWarnings};
use crate::error::{CamError, ShapeRef};
use crate::footprint::footprint_at;
use crate::passes::pass_z_mm;
use crate::profile::{
    contour_contains, crossing_error, hole_shapes, outer_shape, rates_of, resolve_targets,
};
use crate::relief::{self, Site};
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::lap_cuts;
use crate::toolpath::{Move, Toolpath};
use crate::warning::{IslandNeighbour, Warning};
use dc_geometry::{
    Contour, EPS_MM, GeometryWarning, Point, Polyline, Region, Vertex, area_mm2, difference,
    distance_mm, nest, union,
};
use dc_project::{
    ClearanceStrategy, CutDirection, Entry, Operation, OperationId, PocketOp, Project, Tool,
    WallPass,
};
use raster::RasterSettings;
use rings::{Ring, RingKind, RingTree, WallRings, inset_region};

/// Most rings one tool-centre region may have. Bounds the work a tiny
/// step-over can cause (a 0.01 step-over of a 6 mm tool on a 1 m wide
/// pocket is still under it).
pub const MAX_RINGS: usize = 10_000;

/// Which wall a ring runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingWall {
    /// The pocket wall, or an interior ring stepping in from it: the tool
    /// runs inside the loop, as round a hole.
    Pocket,
    /// An island, or an interior ring stepping out from it: the tool runs
    /// outside the loop, as around a part.
    Island,
}

/// The direction rule (design step 8): true when a ring along `wall` is
/// traversed counter-clockwise (viewed from +Z, Y-up) for `direction`.
///
/// Climb milling keeps the uncut material on the right of travel (the
/// spindle turns clockwise from above). Along the pocket wall the material
/// is outside the loop, so climb is counter-clockwise; round an island the
/// material is inside the loop, so climb is clockwise. Conventional is the
/// reverse of climb. Interior rings follow the wall they step in from.
#[must_use]
pub fn ring_is_ccw(wall: RingWall, direction: CutDirection) -> bool {
    let climb_is_ccw = match wall {
        RingWall::Pocket => true,
        RingWall::Island => false,
    };
    match direction {
        CutDirection::Climb => climb_is_ccw,
        CutDirection::Conventional => !climb_is_ccw,
    }
}

/// Generates the Pocket toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Each target instance's closed profiles are placed and nested; each
/// region is a pocket and its holes are islands, and a region inside an
/// island is a pocket of its own, cut after the pocket around it. The
/// tool-centre region is the pocket inset by the tool radius plus
/// `op.stock_to_leave_mm` ([`rings::inset_region`]), so the footprint stays the
/// stock to leave off every wall and island; where it is empty the shape is
/// skipped with
/// [`Warning::PocketNoRoom`], and an island closer to the wall or
/// another island than the tool's diameter plus twice the stock to leave warns
/// [`Warning::PocketIslandTooClose`]. With the offset strategy each
/// tool-centre region's rings step in by `diameter × step_over_fraction`
/// ([`RingTree`]); per depth pass they are cut innermost first, with the
/// wall and island rings last, first, or not at all per `op.wall_pass`,
/// each ring starting at its point nearest the previous ring's end and
/// running per [`ring_is_ccw`]. With the raster strategy each tool-centre
/// region is crossed by lines at the raster angle, one step-over apart and
/// alternating in direction ([`raster::raster_lines`]), whatever
/// `op.direction`; the wall and island rings follow (or precede) them as
/// for offset. [`sequence`] orders and links the rings and lines
/// (stay-down links inside the cleared tool-centre area, else a retract to
/// safe Z).
///
/// With `op.rest_of` set the operation is rest machining: from the
/// referenced Pocket's tool radius and stock to leave, read from `project` and
/// never from its toolpath (S14), [`rest::left_by`] gives the material it
/// left, and the tool-centre region keeps only the centres within this
/// tool's radius of it ([`rest::rest_centre`]); rings or lines are then
/// built on that as on any tool-centre region, less the root rings that
/// would sweep only cleared area ([`rest::in_air`], #167). When nothing is
/// left that this tool reaches the operation warns
/// [`Warning::RestOfClearsNothing`] and has no cutting move. The
/// reference must be an enabled Pocket with the same targets listed before
/// `op` ([`CamError::RestOfInvalid`]).
///
/// Fails without a toolpath when a rate, the step-down or the step-over is
/// missing, when the stock to leave is negative ([`CamError::InvalidStockToLeave`]),
/// when the depth goes below the stock bottom plus allowance, when
/// the raster angle is outside [0, 180) ([`CamError::InvalidRasterAngle`]),
/// when targets cross, when an id does not resolve, when the cutter is not
/// a flat end mill, when the cutter would leave the stock
/// ([`CamError::OffStock`]) or its footprint, descents included, would
/// leave the pocket's material (S9, [`CamError::OffMaterial`]), and when
/// the entry's angle or helix fraction is out of range
/// ([`CamError::InvalidEntry`]).
///
/// Each ring's or line's pass starts with the operation's entry (design
/// step 9, [`entry`]): a helix or zigzag ramp placed once per ring or line
/// in the tool-centre region its pass cuts, from safe Z on the first pass
/// and from the previous pass Z after; where it does not fit the entry
/// falls back to a ramp, then a plunge ([`Warning::EntryFallback`],
/// [`Warning::HelixDiameterReduced`]). A ring or line reached by a
/// stay-down link skips its descent.
///
/// With `op.corner_relief` a dogbone or T-bone notch is cut in every
/// corner the round tool cannot reach, the pocket's corners and the
/// concave corners of its islands, sized to the wall the stock to leave
/// leaves, stepping down at the tool's step-down (the `relief` module):
/// from the wall or island ring of each pass as it turns the corner,
/// with no lift, or with stock to leave (or for a corner some pass has no
/// ring for) in a separate pass after the main passes; S9 then allows
/// each notch its corner's lobe.
/// A tool with no step-down skips it ([`Warning::ReliefSkipped`]);
/// corners it cannot relieve warn [`Warning::ReliefCornerTooTight`]
/// or [`Warning::ReliefCornerNotCut`].
#[must_use = "the toolpath is the whole point"]
pub fn generate_pocket(
    project: &Project,
    op: &PocketOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    // The tool-centre region is the pocket inset by half the diameter,
    // right only for a flat end mill (#67).
    if !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    let rates = rates_of(tool)?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    if let ClearanceStrategy::Raster { angle_deg } = op.strategy
        && !(angle_deg.is_finite() && (0.0..180.0).contains(&angle_deg))
    {
        return Err(CamError::InvalidRasterAngle {
            op: op.id.clone(),
            angle_deg,
        });
    }
    check_stock_to_leave(&op.id, op.stock_to_leave_mm)?;
    let reference = reference_of(project, op)?;
    entry::check(&op.id, &op.entry)?;
    let step_down_mm =
        op.step_down_mm
            .or(tool.step_down_mm)
            .ok_or_else(|| CamError::MissingStepDown {
                op: op.id.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
            })?;
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let depth_mm = op
        .depth_mm
        .ok_or_else(|| CamError::MissingDepth { op: op.id.clone() })?;
    let pass_z = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        step_down_mm,
        op.through_cut_allowance_mm,
    )?;
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let step_over_fraction = op
        .step_over_fraction
        .or(tool.step_over_fraction)
        .ok_or_else(|| CamError::MissingStepOver {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
        })?;
    let invalid_step_over = || CamError::InvalidStepOver {
        op: op.id.clone(),
        step_over_fraction,
    };
    if !(step_over_fraction.is_finite() && step_over_fraction > 0.0 && step_over_fraction <= 1.0) {
        return Err(invalid_step_over());
    }
    let stepover_mm = tool.diameter_mm * step_over_fraction;
    let radius_mm = tool.diameter_mm / 2.0;
    let tolerance_mm = project.flatten_tolerance_mm;

    let mut warnings = Vec::new();
    let relief = relief::Setup {
        op: &op.id,
        relief: op.corner_relief,
        tool,
        thickness_mm,
        depth_mm,
        through_cut_allowance_mm: op.through_cut_allowance_mm,
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        rates,
        tolerance_mm,
    }
    .plan(&mut warnings)?;
    let entry_tool = EntryTool {
        diameter_mm: tool.diameter_mm,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        direction: op.direction,
        side: CutSide::Internal,
        quantum_mm: project
            .post_processor
            .decimal_places
            .quantum_mm(project.display_units),
    };
    let shapes = resolve_targets(project, &op.id, &op.targets, &mut warnings)?;
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

    let wall = match op.wall_pass {
        WallPass::Last => WallRings::Last,
        WallPass::First => WallRings::First,
        WallPass::None => WallRings::Omitted,
    };
    // Every ring and raster line in cut order: its shape, its path (a ring
    // oriented and starting at its entry, a line from start to end), the
    // tool-centre area it has cleared, and its placed entry.
    let mut rings: Vec<(ShapeRef, Polyline, Option<Region>, Option<EntryPlan>)> = Vec::new();
    let mut anchor: Option<Point> = None;
    let mut root_index = 0;
    // Every tool-centre region, for the S9 check on descents.
    let mut centre_regions: Vec<Region> = Vec::new();
    // The material the operation may cut, for S9: each region less the
    // stock to leave.
    let mut allowed: Vec<Region> = Vec::new();
    // Whether a rest operation found anything to clear.
    let mut rest_found = false;
    let mut rest_had_room = false;
    // Each region's cleared loops and tool-centre rings, for the relief.
    let mut sites: Vec<Site> = Vec::new();
    for region in &regions {
        let Some(shape) = outer_shape(region, &shapes) else {
            continue;
        };
        warnings.extend(island_gap_warnings(op, tool, region, &shapes, shape));
        let cleared_from = allowed.len();
        if op.stock_to_leave_mm > 0.0 {
            allowed.extend(inset_region(region, op.stock_to_leave_mm, tolerance_mm));
        } else {
            allowed.push(region.clone());
        }
        // Design step 2: the tool centre stays `r + stock to leave` off every
        // wall and island.
        let tool_centre = inset_region(region, radius_mm + op.stock_to_leave_mm, tolerance_mm);
        if tool_centre.is_empty() {
            warnings.push(Warning::PocketNoRoom {
                op: op.id.clone(),
                shape: shape.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
                diameter_mm: tool.diameter_mm,
            });
            continue;
        }
        // Design step 3: a rest operation keeps only the tool centres that
        // touch what its reference left.
        let (tool_centre, left) = match reference {
            Some(reference) => {
                rest_had_room = true;
                let left = rest::left_by(region, op.stock_to_leave_mm, reference, tolerance_mm);
                let centre = rest::rest_centre(&tool_centre, &left, radius_mm, tolerance_mm);
                if centre.is_empty() {
                    continue;
                }
                rest_found = true;
                (centre, Some(left))
            }
            None => (tool_centre, None),
        };
        // #167: a rest operation skips the root rings that run along the
        // leftover's edge against the cleared area, cutting only air. The
        // tool-centre region itself is unchanged, as are the entry domain
        // and every ring's cleared area.
        let keep = |ring: &Ring| match &left {
            Some(left) => {
                ring.node != 0 || !rest::in_air(&ring.contour, left, radius_mm, tolerance_mm)
            }
            None => true,
        };
        centre_regions.extend(tool_centre.iter().cloned());
        if relief.is_some() {
            let cleared = allowed.get(cleared_from..).unwrap_or(&[]);
            sites.push(relief::pocket_site(cleared, &tool_centre, shape));
        }
        for root in tool_centre {
            if let ClearanceStrategy::Raster { angle_deg } = op.strategy {
                let settings = RasterSettings {
                    stepover_mm,
                    radius_mm,
                    angle_deg,
                    tolerance_mm,
                    wall,
                    direction: op.direction,
                };
                let paths = raster::region_paths(&root, root_index, &settings, &keep, &mut anchor)
                    .ok_or_else(invalid_step_over)?;
                // Raster lines reach the tool-centre region's edge at every
                // wall pass, so the whole root is cleared each pass.
                let domain = (op.entry != Entry::Plunge).then(|| {
                    EntryDomain::new(std::slice::from_ref(&root), tool.diameter_mm, tolerance_mm)
                });
                rings.extend(paths.into_iter().map(|(path, cleared)| {
                    let plan = domain
                        .as_ref()
                        .zip(path.vertices().first())
                        .map(|(d, v)| entry::plan(d, &op.entry, v.point(), &entry_tool));
                    (shape.clone(), path, cleared, plan)
                }));
                root_index += 1;
                continue;
            }
            let tree = RingTree::build(root, stepover_mm, radius_mm, tolerance_mm, MAX_RINGS)
                .ok_or_else(invalid_step_over)?;
            let domain = (op.entry != Entry::Plunge).then(|| {
                EntryDomain::new(&entry_regions(&tree, wall), tool.diameter_mm, tolerance_mm)
            });
            for ring in tree.walk_keeping(wall, &mut anchor, &keep) {
                let Some(node) = tree.nodes.get(ring.node) else {
                    continue;
                };
                let plan = domain
                    .as_ref()
                    .map(|d| entry::plan(d, &op.entry, ring.start, &entry_tool));
                let cleared = Some(node.region.clone());
                rings.push((shape.clone(), ring_path(ring, op.direction), cleared, plan));
            }
        }
    }

    if let Some(rest_of) = op.rest_of.as_ref().filter(|_| rest_had_room && !rest_found) {
        warnings.push(Warning::RestOfClearsNothing {
            op: op.id.clone(),
            rest_of: rest_of.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
        });
    }

    let mut blocks = Vec::with_capacity(rings.len() * pass_z.len());
    let mut entry_warnings = EntryWarnings::default();
    for (pass, &z_mm) in pass_z.iter().enumerate() {
        // #214: a ramp or helix starts the entry clearance above the stock
        // top, and on a later pass above the floor the pass before cleared
        // (the whole tool-centre region, every pass), not at safe Z.
        let above_mm = pass
            .checked_sub(1)
            .and_then(|k| pass_z.get(k).copied())
            .unwrap_or(thickness_mm);
        let descent_from_z_mm = entry::descent_start_z_mm(above_mm, safe_z_mm);
        for (shape, path, cleared, plan) in &rings {
            let Some(start) = path.vertices().first().map(|v| v.point()) else {
                continue;
            };
            // A ring ends where it starts; a raster line at its far end.
            let exit = if path.is_closed() {
                start
            } else {
                path.vertices().last().map_or(start, |v| v.point())
            };
            let Some(lap) = lap_cuts(path, z_mm, None, rates) else {
                continue;
            };
            let (descent_from, descent) = match plan {
                Some(plan) => {
                    let descent = plan.descend(descent_from_z_mm, z_mm);
                    entry_warnings.record(shape, &descent);
                    (descent.from, descent.moves)
                }
                None => (start, Vec::new()),
            };
            blocks.push(CutBlock {
                shape: shape.clone(),
                pass,
                z_mm,
                descent_from,
                descent_from_z_mm,
                descent,
                entry: start,
                cuts: lap.moves,
                exit,
                // A pocket never cuts a part free.
                frees: false,
                cleared: cleared.clone(),
                group: pass,
            });
        }
    }
    warnings.extend(entry_warnings.into_warnings(&op.id));
    // Design step 10 (maintainer decision 2026-10-24): each corner's
    // notch is cut from the wall or island ring of every pass that turns
    // it; stock to leave means no final wall, and a corner some pass has
    // no ring for is left for the separate pass after the main passes.
    let (inline, fallback) = match &relief {
        Some(relief) => relief.inline_pocket(
            &sites,
            &pass_z,
            op.stock_to_leave_mm <= 0.0,
            &mut blocks,
            &mut warnings,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let containment = nested_pockets(&regions, &shapes);
    let input = SequenceInput {
        blocks,
        order: op.machining_order,
        containment: &containment,
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        stock_top_mm: thickness_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        cut_before: &[],
    };
    let mut toolpath = sequence(&input)?;
    // Design step 10: the separate relief pass after all the passes, for
    // the corners not cut inline; S9 allows each notch the lobe its corner
    // may lose.
    if let Some(relief) = &relief {
        let mut cut = inline;
        cut.extend(relief.cut(fallback, &mut toolpath, &mut warnings)?);
        let lobes: Vec<Region> = cut
            .iter()
            .flat_map(|c| relief::lobe(c, radius_mm, tolerance_mm))
            .collect();
        if !lobes.is_empty() {
            allowed = union(&allowed, &lobes, tolerance_mm);
        }
    }

    // #62: the cutter must stay on the stock.
    if let Some(overhang) =
        swept_bounds(&toolpath, radius_mm).and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.id.clone(),
            overhang,
        });
    }
    check_in_material(
        op,
        &toolpath,
        radius_mm,
        tolerance_mm,
        &regions,
        &allowed,
        &centre_regions,
        &shapes,
    )?;
    Ok((toolpath, warnings))
}

/// The stock to leave validation requires, 0 or more, checked again: a
/// negative one would cut into the walls.
fn check_stock_to_leave(op: &OperationId, stock_to_leave_mm: f64) -> Result<(), CamError> {
    if stock_to_leave_mm.is_finite() && stock_to_leave_mm >= 0.0 {
        Ok(())
    } else {
        Err(CamError::InvalidStockToLeave {
            op: op.clone(),
            stock_to_leave_mm,
        })
    }
}

/// The Pocket `op` rests on, read from the project (S14: its tool and
/// stock to leave, never its toolpath), or `None` when `op.rest_of` is unset.
/// As validation requires, it must be an enabled Pocket with the same
/// targets listed before `op`; else [`CamError::RestOfInvalid`].
fn reference_of(project: &Project, op: &PocketOp) -> Result<Option<rest::Reference>, CamError> {
    let Some(rest_of) = &op.rest_of else {
        return Ok(None);
    };
    let invalid = || CamError::RestOfInvalid {
        op: op.id.clone(),
        rest_of: rest_of.clone(),
    };
    let index = project
        .operations
        .iter()
        .position(|o| o.id() == &op.id)
        .unwrap_or(project.operations.len());
    let earlier = project
        .operations
        .iter()
        .take(index)
        .find(|o| o.id() == rest_of);
    let Some(Operation::Pocket(rough)) = earlier else {
        return Err(invalid());
    };
    if !rough.enabled || rough.targets != op.targets {
        return Err(invalid());
    }
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == rough.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: rough.id.clone(),
            tool: rough.tool.clone(),
        })?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    check_stock_to_leave(&rough.id, rough.stock_to_leave_mm)?;
    Ok(Some(rest::Reference {
        radius_mm: tool.diameter_mm / 2.0,
        stock_to_leave_mm: rough.stock_to_leave_mm,
    }))
}

/// The clearance regions of a ring tree's entries: the material every
/// pass of the tree removes. That is the root tool-centre region, or with
/// the wall pass omitted its children (not its residual nodes, which are
/// then omitted too), so a descent from the previous pass
/// Z never starts above the uncut band the omitted wall ring leaves.
fn entry_regions(tree: &RingTree, wall: WallRings) -> Vec<Region> {
    let Some(root) = tree.nodes.first() else {
        return Vec::new();
    };
    match wall {
        WallRings::Omitted => root
            .children
            .iter()
            .filter_map(|&c| tree.nodes.get(c))
            .filter(|n| !n.residual)
            .map(|n| n.region.clone())
            .collect(),
        WallRings::First | WallRings::Last => vec![root.region.clone()],
    }
}

/// [`Warning::PocketIslandTooClose`] for every island of `region`
/// nearer the wall or a later island than the tool needs to pass between
/// them: its diameter plus twice `op.stock_to_leave_mm` (#168), the gap at which
/// the tool-centre region splits.
fn island_gap_warnings(
    op: &PocketOp,
    tool: &Tool,
    region: &Region,
    shapes: &[(ShapeRef, Contour)],
    shape: &ShapeRef,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let needed_mm = tool.diameter_mm + 2.0 * op.stock_to_leave_mm;
    let warn = |island: &ShapeRef, other, gap_mm| Warning::PocketIslandTooClose {
        op: op.id.clone(),
        shape: shape.clone(),
        island: island.profile,
        other,
        gap_mm,
        needed_mm,
        stock_to_leave_mm: op.stock_to_leave_mm,
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        diameter_mm: tool.diameter_mm,
    };
    let holes: Vec<(&Contour, &ShapeRef)> = hole_shapes(region, shapes).collect();
    for (i, &(hole, island)) in holes.iter().enumerate() {
        let gap_mm = distance_mm(hole.polyline(), region.outer().polyline());
        if gap_mm < needed_mm - EPS_MM {
            warnings.push(warn(island, IslandNeighbour::Wall, gap_mm));
        }
        for &(other, other_shape) in holes.iter().skip(i + 1) {
            let gap_mm = distance_mm(hole.polyline(), other.polyline());
            if gap_mm < needed_mm - EPS_MM {
                warnings.push(warn(
                    island,
                    IslandNeighbour::Island(other_shape.profile),
                    gap_mm,
                ));
            }
        }
    }
    warnings
}

/// `ring` as a path: wound by the direction rule ([`ring_is_ccw`]) for
/// `direction`, starting at its start point, with each of its clean-ups
/// spliced in where the lap reaches its point (#267): straight to the
/// residual ring's start, round it (wound by the same rule), and straight
/// back. A clean-up reached from the start is cut when the lap closes
/// there, after the ring has cleared round it.
fn ring_path(ring: Ring, direction: CutDirection) -> Polyline {
    let side = match ring.kind {
        RingKind::Outer => RingWall::Pocket,
        RingKind::Hole => RingWall::Island,
    };
    let path = if ring.contour.is_ccw() == ring_is_ccw(side, direction) {
        ring.contour.into_polyline()
    } else {
        ring.contour.reversed().into_polyline()
    };
    let lap = starting_at(&path, ring.start);
    if ring.cleanups.is_empty() {
        return lap;
    }
    let lap = ring
        .cleanups
        .iter()
        .fold(lap, |lap, c| rings::split_at(&lap, c.at));
    let vertices = lap.vertices();
    // The vertex of the lap each clean-up leaves from; the start counts as
    // the lap's end.
    let slot = |at: Point| {
        vertices
            .iter()
            .skip(1)
            .position(|v| v.point().distance_to(at) <= EPS_MM)
            .map_or(vertices.len(), |i| i + 1)
    };
    let mut excursions: Vec<(usize, Vec<Vertex>)> = ring
        .cleanups
        .into_iter()
        .map(|c| {
            let at = c.at;
            let start = c.ring.start;
            let mut out = vec![Vertex::new(at.x, at.y, 0.0)];
            out.extend_from_slice(ring_path(c.ring, direction).vertices());
            out.push(Vertex::new(start.x, start.y, 0.0));
            (slot(at), out)
        })
        .collect();
    // In lap order; a stable sort keeps the walk's order at one vertex.
    excursions.sort_by_key(|(slot, _)| *slot);
    let mut out = Vec::with_capacity(vertices.len() + excursions.len() * 8);
    let mut pending = excursions.into_iter().peekable();
    for (index, vertex) in vertices.iter().enumerate() {
        while let Some((_, excursion)) = pending.next_if(|(slot, _)| *slot == index) {
            out.extend(excursion);
        }
        out.push(*vertex);
    }
    for (_, excursion) in pending {
        out.extend(excursion);
    }
    Polyline::new(out, true)
}

/// `path` (closed) rotated to start at `start` (`rotateContourToNearestEntry`),
/// split there first when `start` lies inside a segment
/// ([`rings::split_at`], #196); unchanged when `start` is not on it.
fn starting_at(path: &Polyline, start: Point) -> Polyline {
    let mut vertices = rings::split_at(path, start).vertices().to_vec();
    let index = vertices
        .iter()
        .position(|v| v.point().distance_to(start) <= EPS_MM)
        .unwrap_or(0);
    // A vertex carries the bulge of the segment it starts, so rotating the
    // list keeps every arc.
    vertices.rotate_left(index);
    Polyline::new(vertices, true)
}

/// Containment pairs for `sequence`: a pocket inside an island (a region of
/// depth 2 or more) is cut after the pocket that holds the island, so each
/// pair is `(pocket around, pocket inside)`. Pockets free nothing, so this
/// orders the work only; it is the design's "after the island".
fn nested_pockets(regions: &[Region], shapes: &[(ShapeRef, Contour)]) -> Vec<(ShapeRef, ShapeRef)> {
    let mut pairs = Vec::new();
    for region in regions {
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let (Some(inner), Some(probe)) = (
            outer_shape(region, shapes),
            region.outer().polyline().vertices().first(),
        ) else {
            continue;
        };
        let around = regions
            .iter()
            .filter(|r| r.depth() == parent_depth)
            .find(|r| r.holes().iter().any(|h| contour_contains(h, probe.point())));
        if let Some(outer) = around.and_then(|r| outer_shape(r, shapes)) {
            pairs.push((outer.clone(), inner.clone()));
        }
    }
    pairs
}

/// S9: the tool's footprint along `toolpath` lies in the pockets'
/// material, `allowed` (each of `regions` less the stock to leave, plus the
/// lobe of every relief notch cut, [`relief::lobe`]), inside every region
/// and outside every island; `regions` name the pocket in
/// the error. A rest operation is held to the same area (S14). Flattening
/// moves both outlines by up to `tolerance_mm`, so area within that band
/// of the boundaries is allowed. Ramp and helix moves are checked on their
/// tool centres, which must stay in `centre_regions` (the tool-centre
/// regions, where the whole tool is in the material), and the rest of the
/// toolpath on its swept footprint.
#[expect(
    clippy::too_many_arguments,
    reason = "the check needs each of the generator's region sets"
)]
fn check_in_material(
    op: &PocketOp,
    toolpath: &Toolpath,
    radius_mm: f64,
    tolerance_mm: f64,
    regions: &[Region],
    allowed: &[Region],
    centre_regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
) -> Result<(), CamError> {
    let boundary_mm: f64 = regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().length_mm())
        .sum();
    let allowed_mm2 = (tolerance_mm + EPS_MM) * boundary_mm;
    let off_material = |swept: &[Region], probe: Option<Point>| {
        let outside = difference(swept, allowed, tolerance_mm);
        let area = area_mm2(&outside);
        if area <= allowed_mm2 {
            return Ok(());
        }
        // Name the pocket whose outline holds the stray area, else the first.
        let probe = probe.or_else(|| {
            outside
                .iter()
                .find_map(|r| r.outer().polyline().vertices().first().map(|v| v.point()))
        });
        let named = regions
            .iter()
            .find(|r| probe.is_some_and(|p| contour_contains(r.outer(), p)))
            .or_else(|| regions.first())
            .and_then(|r| outer_shape(r, shapes));
        match named {
            Some(shape) => Err(CamError::OffMaterial {
                op: op.id.clone(),
                shape: shape.clone(),
                area_mm2: area,
            }),
            None => Ok(()),
        }
    };
    if let Some(i) = entry::first_descent_outside(toolpath, centre_regions, tolerance_mm)
        && let (Some(from), Some(mv)) = (
            i.checked_sub(1).and_then(|k| toolpath.moves.get(k)),
            toolpath.moves.get(i),
        )
    {
        // The one stray move's own footprint, to measure and name.
        let stray = Toolpath {
            moves: vec![Move::rapid(from.to), *mv],
        };
        let swept = footprint_at(&stray, radius_mm, tolerance_mm);
        off_material(&swept, Some(mv.to.xy()))?;
    }
    let swept = footprint_at(&entry::without_descents(toolpath), radius_mm, tolerance_mm);
    if swept.is_empty() {
        return Ok(());
    }
    off_material(&swept, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryKind, without_descents};
    use crate::error::CamError;
    use crate::footprint::footprint;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind, Toolpath};
    use crate::warning::WarningCode;
    use dc_geometry::Vertex;
    use dc_geometry::{
        Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3, Polyline, Rect, Region,
        Transform, area_mm2, assert_approx_eq, difference, intersection, nest, offset,
    };
    use dc_project::test_support::sample_project;
    use dc_project::{
        ClearanceStrategy, CornerRelief, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, OperationId, Stock, ToolId, WallPass,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::{PI, TAU};

    const TOL: f64 = 1e-6;
    /// Rings and bounds are compared to this: the design's "± 0.01".
    const RING_TOL: f64 = 0.01;
    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    const DIAMETER_MM: f64 = 6.35;
    const RADIUS_MM: f64 = DIAMETER_MM / 2.0;
    /// 40 % of the diameter.
    const STEPOVER_MM: f64 = 2.54;
    /// Depth 4 in steps of 2 on 6 mm stock.
    const DEPTH_MM: f64 = 4.0;
    const PASS_Z_MM: [f64; 2] = [4.0, 2.0];
    const FLAT_TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

    /// The 100 × 50 rect every pocket test starts from, placed at (20, 20).
    fn rect_100x50() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)
    }

    fn at_20_20() -> Transform {
        Transform::translate(20.0, 20.0)
    }

    /// The placed rect's bounds.
    fn wall() -> Rect {
        Rect {
            min: Point::new(20.0, 20.0),
            max: Point::new(120.0, 70.0),
        }
    }

    /// `rect` shrunk by `inset_mm` on every side.
    fn inset(rect: Rect, inset_mm: f64) -> Rect {
        Rect {
            min: Point::new(rect.min.x + inset_mm, rect.min.y + inset_mm),
            max: Point::new(rect.max.x - inset_mm, rect.max.y - inset_mm),
        }
    }

    /// A 300 × 200 × 6 mm project with one definition made of `profiles`,
    /// placed once per `(instance id, transform)`.
    fn project(profiles: Vec<Polyline>, placements: &[(&str, Transform)]) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Shape".into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = placements
            .iter()
            .map(|&(id, transform)| FeatureInstance {
                id: InstanceId::new(id),
                definition: DefinitionId::new("d1"),
                transform,
                name: format!("Shape {id}"),
                visible: true,
                locked: false,
                z_top_mm: Some(THICKNESS_MM),
                z_bottom_mm: Some(0.0),
            })
            .collect();
        p.operations.clear();
        p
    }

    fn rect_project() -> Project {
        project(vec![rect_100x50()], &[("i1", at_20_20())])
    }

    /// The rect with a 20 mm circular island at its centre, (70, 45).
    fn island_project() -> Project {
        project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ],
            &[("i1", at_20_20())],
        )
    }

    /// An offset Pocket of every instance in `project`, 4 mm deep in the
    /// tool's 2 mm steps, 40 % step-over, wall pass last, plunging.
    fn pocket_op(project: &Project, direction: CutDirection) -> PocketOp {
        PocketOp {
            id: OperationId::new("o1"),
            name: "Pocket".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: Some(DEPTH_MM),
            step_down_mm: None,
            step_over_fraction: Some(0.4),
            direction,
            stock_to_leave_mm: 0.0,
            rest_of: None,
            entry: Entry::Plunge,
            corner_relief: CornerRelief::None,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &PocketOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_pocket(project, op).unwrap();
        assert_safe(
            &toolpath,
            project,
            &dc_project::Operation::Pocket(op.clone()),
        );
        (toolpath, warnings)
    }

    /// One closed ring of cutting moves at one Z, in cut order.
    #[derive(Debug, Clone)]
    struct Ring {
        z_mm: f64,
        /// Start point, then the end of every move.
        points: Vec<Point>,
        /// Signed area enclosed, arcs included; negative is clockwise.
        area_mm2: f64,
        bounds: Rect,
        /// The ring's cutting moves, from its start.
        moves: Vec<Move>,
    }

    impl Ring {
        fn start(&self) -> Point {
            self.points[0]
        }

        fn width_mm(&self) -> f64 {
            self.bounds.width_mm()
        }
    }

    /// Signed sweep, radians, of the arc from `a` to `b` about `c`.
    fn sweep_rad(a: Point, b: Point, c: Point, ccw: bool) -> f64 {
        let (u, v) = (a - c, b - c);
        let raw = u.cross(v).atan2(u.dot(v));
        if ccw {
            if raw > 0.0 { raw } else { raw + TAU }
        } else if raw < 0.0 {
            raw
        } else {
            raw - TAU
        }
    }

    /// The signed area contribution of the cutting move `mv` from `from`,
    /// arcs included, and the extra extreme points of an arc.
    fn move_area_and_extremes(from: Point, mv: &Move) -> (f64, Vec<Point>) {
        let to = mv.to.xy();
        let mut area = from.x * to.y - to.x * from.y;
        let mut extremes = Vec::new();
        let arc = match mv.kind {
            MoveKind::ArcCcw { centre } => Some((centre, true)),
            MoveKind::ArcCw { centre } => Some((centre, false)),
            _ => None,
        };
        if let Some((c, ccw)) = arc {
            let r = from.distance_to(c);
            let theta = sweep_rad(from, to, c, ccw);
            area += r * r * (theta - theta.sin());
            extremes.extend(dc_geometry::Arc::from_ends(from, to, c, ccw).axis_extremes());
        }
        (area, extremes)
    }

    /// True when the path from move `start` onward returns to `target`
    /// before it returns to `other` (or never returns to `other`), without
    /// leaving the Z level or hitting a rapid.
    fn returns_to_first(moves: &[Move], start: usize, target: Point, other: Point) -> bool {
        let z = moves[start].to.z;
        for mv in &moves[start..] {
            if mv.is_rapid() || (mv.to.z - z).abs() > EPS_MM {
                return false;
            }
            let p = mv.to.xy();
            if p.distance_to(target) <= EPS_MM {
                return true;
            }
            if p.distance_to(other) <= EPS_MM {
                return false;
            }
        }
        false
    }

    /// Splits the toolpath into closed rings: runs of XY cutting moves at
    /// one Z that return to their start. A stay-down link between two rings
    /// is a cutting move too; it is told from a ring's first segment by
    /// whether the path returns to the link's end before its start.
    fn rings(toolpath: &Toolpath) -> Vec<Ring> {
        let moves = &toolpath.moves;
        let mut rings = Vec::new();
        let mut pos = match moves.first() {
            Some(m) => m.to,
            None => return rings,
        };
        let mut current: Option<Ring> = None;
        let mut i = 0;
        while i < moves.len() {
            let mv = moves[i];
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
            if mv.is_rapid() || !level || !xy {
                assert!(current.is_none(), "ring left open at move {i}: {current:?}");
                pos = mv.to;
                i += 1;
                continue;
            }
            let from = pos.xy();
            let ring = match current.as_mut() {
                Some(ring) => ring,
                None => {
                    // Either this move starts a ring at `from`, or it is a
                    // link to the start of the next ring.
                    let to = mv.to.xy();
                    if i + 1 < moves.len() && returns_to_first(moves, i + 1, to, from) {
                        pos = mv.to;
                        i += 1;
                        continue;
                    }
                    current.insert(Ring {
                        z_mm: mv.to.z,
                        points: vec![from],
                        area_mm2: 0.0,
                        bounds: Rect::from_point(from),
                        moves: Vec::new(),
                    })
                }
            };
            let (area, extremes) = move_area_and_extremes(from, &mv);
            ring.area_mm2 += area;
            ring.points.push(mv.to.xy());
            ring.moves.push(mv);
            ring.bounds = ring.bounds.including(mv.to.xy());
            for p in extremes {
                ring.bounds = ring.bounds.including(p);
            }
            if mv.to.xy().distance_to(ring.start()) <= EPS_MM {
                let mut done = current.take().unwrap();
                done.area_mm2 /= 2.0;
                rings.push(done);
            }
            pos = mv.to;
            i += 1;
        }
        assert!(
            current.is_none(),
            "toolpath ends inside a ring: {current:?}"
        );
        rings
    }

    fn rings_at(toolpath: &Toolpath, z_mm: f64) -> Vec<Ring> {
        rings(toolpath)
            .into_iter()
            .filter(|r| (r.z_mm - z_mm).abs() <= EPS_MM)
            .collect()
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code()).collect()
    }

    fn assert_rect_eq(got: Rect, want: Rect, tol: f64, what: &str) {
        let close = (got.min.x - want.min.x).abs() <= tol
            && (got.min.y - want.min.y).abs() <= tol
            && (got.max.x - want.max.x).abs() <= tol
            && (got.max.y - want.max.y).abs() <= tol;
        assert!(close, "{what}: got {got:?}, want {want:?} within {tol}");
    }

    /// The region of one placed polyline.
    fn region_of(polylines: &[Polyline], placement: Transform) -> Region {
        let contours = polylines
            .iter()
            .map(|p| Contour::new(placement.apply_polyline(p).unwrap()).unwrap())
            .collect();
        nest(contours).unwrap().0.remove(0)
    }

    /// The direction rule itself (design step 8): climb is CCW on the
    /// pocket wall and CW round an island; conventional is the reverse.
    #[test]
    fn direction_rule() {
        use CutDirection::{Climb, Conventional};
        assert!(ring_is_ccw(RingWall::Pocket, Climb));
        assert!(!ring_is_ccw(RingWall::Pocket, Conventional));
        assert!(!ring_is_ccw(RingWall::Island, Climb));
        assert!(ring_is_ccw(RingWall::Island, Conventional));
    }

    /// Acceptance test 19: the 100 × 50 rect with the 6.35 mm tool at 40 %
    /// gives, per pass, concentric rings 2.54 mm apart whose outermost is
    /// the wall inset by the tool radius. The tool-centre rect is 93.65 ×
    /// 43.65; 8 insets of 2.54 fit in its 21.825 half-height, so 9 rings.
    #[test]
    fn offset_rings_are_concentric_and_spaced_by_stepover() {
        let p = rect_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            let mut rings = rings_at(&tp, z);
            assert_eq!(rings.len(), 9, "rings at Z {z}: {rings:?}");
            rings.sort_by(|a, b| a.area_mm2.abs().total_cmp(&b.area_mm2.abs()));
            let tool_centre = inset(wall(), RADIUS_MM);
            for (k, ring) in rings.iter().rev().enumerate() {
                let want = inset(tool_centre, k as f64 * STEPOVER_MM);
                assert_rect_eq(ring.bounds, want, RING_TOL, "ring");
                let centre_x = (ring.bounds.min.x + ring.bounds.max.x) / 2.0;
                let centre_y = (ring.bounds.min.y + ring.bounds.max.y) / 2.0;
                assert_approx_eq!(centre_x, 70.0, RING_TOL);
                assert_approx_eq!(centre_y, 45.0, RING_TOL);
            }
        }
    }

    /// Port of `testPocketOffsetCutsInnerFirst` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`, "offset cuts inner loops
    /// before wall-adjacent loops"): in cut order the first ring of a pass
    /// is the smallest and lies strictly inside the last, which reaches
    /// the wall inset; climb and conventional first rings wind opposite
    /// ways. Upstream's 20 × 20 pocket with a 4 mm tool at 50 % becomes
    /// the rect fixture; the assertions are the same.
    #[test]
    fn offset_cuts_inner_rings_before_the_wall() {
        let p = rect_project();
        let mut first_areas = Vec::new();
        for direction in [CutDirection::Conventional, CutDirection::Climb] {
            let (tp, warnings) = generate(&p, &pocket_op(&p, direction));
            assert!(warnings.is_empty(), "{warnings:?}");
            let rings = rings_at(&tp, PASS_Z_MM[0]);
            assert!(rings.len() >= 3, "{direction:?}: {}", rings.len());
            let first = &rings[0];
            let last = &rings[rings.len() - 1];
            assert!(
                first.area_mm2.abs() < last.area_mm2.abs(),
                "{direction:?}: first {first:?} is smaller than last {last:?}"
            );
            assert!(first.bounds.min.x > last.bounds.min.x, "{direction:?}");
            assert!(first.bounds.min.y > last.bounds.min.y, "{direction:?}");
            assert!(first.bounds.max.x < last.bounds.max.x, "{direction:?}");
            assert!(first.bounds.max.y < last.bounds.max.y, "{direction:?}");
            assert_rect_eq(last.bounds, inset(wall(), RADIUS_MM), RING_TOL, "wall ring");
            first_areas.push(first.area_mm2);
        }
        assert!(
            first_areas[0] * first_areas[1] < 0.0,
            "climb and conventional wind opposite ways: {first_areas:?}"
        );
    }

    /// `wall_pass`: `last` cuts the wall ring after the inner rings,
    /// `first` before them, and `none` not at all, so the pocket is
    /// undersize by one step-over.
    #[test]
    fn wall_pass_first_last_none() {
        let p = rect_project();
        let tool_centre = inset(wall(), RADIUS_MM);
        let is_wall = |r: &Ring| {
            (r.bounds.min.x - tool_centre.min.x).abs() <= RING_TOL
                && (r.bounds.max.y - tool_centre.max.y).abs() <= RING_TOL
        };
        for (wall_pass, position) in [(WallPass::Last, 8usize), (WallPass::First, 0)] {
            let mut op = pocket_op(&p, CutDirection::Climb);
            op.wall_pass = wall_pass;
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let rings = rings_at(&tp, z);
                assert_eq!(rings.len(), 9, "{wall_pass:?} at Z {z}");
                let walls: Vec<usize> = (0..rings.len()).filter(|&i| is_wall(&rings[i])).collect();
                assert_eq!(walls, [position], "{wall_pass:?} at Z {z}: {rings:?}");
            }
        }
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.wall_pass = WallPass::None;
        let (tp, _) = generate(&p, &op);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            assert_eq!(rings.len(), 8, "none at Z {z}");
            assert!(rings.iter().all(|r| !is_wall(r)), "no wall ring: {rings:?}");
            let outermost = rings
                .iter()
                .max_by(|a, b| a.area_mm2.abs().total_cmp(&b.area_mm2.abs()))
                .unwrap();
            assert_rect_eq(
                outermost.bounds,
                inset(tool_centre, STEPOVER_MM),
                RING_TOL,
                "outermost ring with no wall pass",
            );
        }
    }

    /// S9: the tool's footprint along every cutting move stays inside the
    /// rect. The footprint's area is the rect's less the four corner
    /// wedges a round tool cannot reach, `(4 − π) r²` in all.
    #[test]
    fn footprint_stays_inside_region() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let fp = footprint(&tp, RADIUS_MM);
        assert!(!fp.is_empty());
        let region = region_of(&[rect_100x50()], at_20_20());
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        assert!(
            area_mm2(&outside) <= 1e-3,
            "footprint outside the rect: {} mm²",
            area_mm2(&outside)
        );
        let corners_mm2 = (4.0 - std::f64::consts::PI) * RADIUS_MM * RADIUS_MM;
        assert_approx_eq!(area_mm2(&fp), 5000.0 - corners_mm2, 0.5);
    }

    /// S9, acceptance test 20: with the 20 mm circle island no cutting
    /// move's footprint enters the circle. Checked exactly on the tool
    /// centres (never nearer the island's centre than its radius plus the
    /// tool's) and on the area the footprint shares with the island.
    #[test]
    fn footprint_stays_outside_island() {
        let p = island_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let centre = Point::new(70.0, 45.0);
        let mut nearest = f64::MAX;
        for (i, mv) in tp.moves.iter().enumerate() {
            if !mv.is_rapid() {
                // A move's ends; a ring's arcs bulge away from the island.
                let d = mv.to.xy().distance_to(centre);
                assert!(
                    d >= 10.0 + RADIUS_MM - FLAT_TOL,
                    "move {i} brings the tool centre {d} mm from the island centre: {mv:?}"
                );
                nearest = nearest.min(d);
            }
        }
        assert!(
            nearest <= 10.0 + RADIUS_MM + RING_TOL,
            "the island ring wraps the island: nearest centre {nearest}"
        );
        let island = region_of(
            &[Polyline::circle(Point::new(50.0, 25.0), 10.0)],
            at_20_20(),
        );
        let fp = footprint(&tp, RADIUS_MM);
        let inside = intersection(&fp, std::slice::from_ref(&island), FLAT_TOL);
        // The flattened footprint and island each stray a tolerance from
        // the true curves; anything beyond that band is a real gouge.
        let band_mm2 = 2.0 * FLAT_TOL * TAU * 10.0;
        assert!(
            area_mm2(&inside) <= band_mm2,
            "footprint inside the island: {} mm²",
            area_mm2(&inside)
        );
    }

    /// Acceptance test 19, in the spirit of PureCutCNC
    /// `pocketClearance.test.ts` ("no stock left"): the union of the
    /// footprints covers the rect inset by the tool radius, the reachable
    /// area, within 0.05 mm (as area: 0.05 × its perimeter).
    #[test]
    fn offset_coverage_reaches_inset_region() {
        let p = rect_project();
        let outer = at_20_20().apply_polyline(&rect_100x50()).unwrap();
        for fraction in STEP_OVERS {
            let mut op = pocket_op(&p, CutDirection::Climb);
            op.step_over_fraction = Some(fraction);
            let (tp, _) = generate(&p, &op);
            let fp = footprint(&tp, RADIUS_MM);
            let reachable = reachable_of(&outer);
            assert_eq!(reachable.len(), 1);
            let uncut = difference(&reachable, &fp, FLAT_TOL);
            assert!(
                area_mm2(&uncut) <= coverage_tolerance_mm2(&reachable),
                "{fraction}: {} mm² of the reachable area is not cut, in {} pieces",
                area_mm2(&uncut),
                uncut.len()
            );
            // Coverage holds at every pass, not only in the union of passes.
            assert_covers_at_every_pass(&tp, &outer, &format!("{fraction}"));
        }
    }

    /// The step-overs the coverage tests run at: the fixtures' 40 % and
    /// three above half the diameter, where the ring tree alone left
    /// material standing (#258).
    const STEP_OVERS: [f64; 4] = [0.4, 0.6, 0.8, 1.0];

    /// The reachable area of a pocket whose (placed) wall is `outer`: the
    /// wall inset by the tool radius.
    fn reachable_of(outer: &Polyline) -> Vec<Region> {
        nest(offset(&Contour::new(outer.clone()).unwrap(), -RADIUS_MM))
            .unwrap()
            .0
    }

    /// The coverage tests' tolerance: 0.05 mm, as area, 0.05 × the
    /// reachable area's perimeter.
    fn coverage_tolerance_mm2(reachable: &[Region]) -> f64 {
        let perimeter_mm: f64 = reachable
            .iter()
            .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
            .map(|c| c.polyline().length_mm())
            .sum();
        0.05 * perimeter_mm
    }

    /// The footprint of the cutting moves of `tp` at `z_mm`.
    fn footprint_at_z(tp: &Toolpath, z_mm: f64) -> Vec<Region> {
        let at_z = Toolpath {
            moves: tp
                .moves
                .iter()
                .filter(|m| !m.is_rapid() && (m.to.z - z_mm).abs() <= EPS_MM)
                .copied()
                .collect(),
        };
        footprint(&at_z, RADIUS_MM)
    }

    /// The largest piece of the reachable area a coverage test lets stand,
    /// mm²: a 0.1 mm square.
    const PIN_MM2: f64 = 0.01;

    /// At every pass, the footprint of `tp` covers the reachable area of
    /// the pocket walled by `outer` within the coverage tolerance, and no
    /// piece it leaves is over [`PIN_MM2`].
    fn assert_covers_at_every_pass(tp: &Toolpath, outer: &Polyline, what: &str) {
        let reachable = reachable_of(outer);
        for z in PASS_Z_MM {
            let uncut = difference(&reachable, &footprint_at_z(tp, z), FLAT_TOL);
            // A pin is one piece; a sliver along a flattened arc is not.
            let largest_mm2 = uncut.iter().map(Region::area_mm2).fold(0.0, f64::max);
            assert!(
                largest_mm2 <= PIN_MM2,
                "{what} at Z {z}: a {largest_mm2} mm² piece is left standing"
            );
            assert!(
                area_mm2(&uncut) <= coverage_tolerance_mm2(&reachable),
                "{what} at Z {z}: {} mm² uncut, in {} pieces",
                area_mm2(&uncut),
                uncut.len()
            );
        }
    }

    /// #258: above half the diameter, the ring tree stops when a region
    /// cannot be inset by another step-over, so its innermost ring can lie
    /// up to a step-over from the middle while the tool reaches only its
    /// radius, and with step-over 1.0 each ring's sharp corners lie more
    /// than a radius from the ring inside it. Circles and rects whose
    /// innermost region is between `r` and `s` across, and the 100 × 50
    /// rect, are covered at every pass and their centre is cut.
    #[test]
    fn offset_above_half_step_over_leaves_no_centre_pin() {
        for fraction in [0.6, 0.8, 1.0] {
            for (name, outer) in pin_shapes() {
                let p = project(vec![outer.clone()], &[("i1", Transform::IDENTITY)]);
                // The residual rings take the entry like any other ring, and
                // go wherever the wall pass goes.
                let setups = [
                    (Entry::Plunge, WallPass::Last),
                    (helix_80(), WallPass::Last),
                    (Entry::Ramp { angle_deg: 3.0 }, WallPass::Last),
                    (Entry::Plunge, WallPass::First),
                ];
                for (entry, wall_pass) in setups {
                    let mut op = pocket_op(&p, CutDirection::Climb);
                    op.step_over_fraction = Some(fraction);
                    op.entry = entry;
                    op.wall_pass = wall_pass;
                    let (tp, warnings) = generate(&p, &op);
                    let what = format!("{name} at {fraction}, {:?}, {wall_pass:?}", op.entry);
                    assert!(warnings.is_empty(), "{what}: {:?}", codes(&warnings));
                    assert_covers_at_every_pass(&tp, &outer, &what);
                    for z in PASS_Z_MM {
                        assert!(
                            covers(&footprint_at_z(&tp, z), PIN_CENTRE),
                            "{what} at Z {z}: a pin is left at the centre"
                        );
                    }
                }
                // With no wall pass the root's residual rings go with its
                // rings; what is cut still passes S9 (`generate`).
                let mut op = pocket_op(&p, CutDirection::Climb);
                op.step_over_fraction = Some(fraction);
                op.wall_pass = WallPass::None;
                generate(&p, &op);
            }
        }
    }

    /// Raster has no such gap (#258): its lines are `s ≤ 2r` apart, and a
    /// point between two lines is within `s/2` of one of them or of the
    /// wall ring.
    #[test]
    fn raster_above_half_step_over_leaves_no_ridge() {
        for fraction in [0.6, 0.8, 1.0] {
            for (name, outer) in pin_shapes() {
                let p = project(vec![outer.clone()], &[("i1", Transform::IDENTITY)]);
                for angle_deg in [0.0, 45.0] {
                    let mut op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
                    op.step_over_fraction = Some(fraction);
                    let (tp, _) = generate(&p, &op);
                    let what = format!("{name} at {fraction}, {angle_deg}°");
                    assert_covers_at_every_pass(&tp, &outer, &what);
                }
            }
        }
    }

    /// The rapids up from a cut at `z_mm`: one per lift off that pass.
    fn lifts_from(tp: &Toolpath, z_mm: f64) -> usize {
        tp.moves
            .windows(2)
            .filter(|w| {
                w[1].is_rapid() && (w[0].to.z - z_mm).abs() <= EPS_MM && w[1].to.z > z_mm + EPS_MM
            })
            .count()
    }

    /// #267: above `(1 + √2/2)·r` step-over every sharp corner of a ring
    /// with a child has a residual ring (#258), and each one cut as a ring
    /// of its own lifted: 13 lifts per pass on the 100 × 50 rect at 1.0,
    /// where 0.8 lifts once. Each is now cut from the ring that leaves it,
    /// out from that ring's corner and back, at the pass Z: 1 lift per pass
    /// at 1.0 with the wall pass last and 2 with it first, the same as at
    /// 0.8. Coverage holds at every pass.
    #[test]
    fn corner_cleanup_rings_do_not_lift() {
        let p = rect_project();
        let outer = at_20_20().apply_polyline(&rect_100x50()).unwrap();
        for wall_pass in [WallPass::Last, WallPass::First] {
            for entry in [Entry::Plunge, helix_80()] {
                let lifts = |fraction| {
                    let mut op = pocket_op(&p, CutDirection::Climb);
                    op.step_over_fraction = Some(fraction);
                    op.wall_pass = wall_pass;
                    op.entry = entry.clone();
                    let (tp, warnings) = generate(&p, &op);
                    let what = format!("{fraction}, {wall_pass:?}, {entry:?}");
                    assert!(warnings.is_empty(), "{what}: {:?}", codes(&warnings));
                    assert_covers_at_every_pass(&tp, &outer, &what);
                    PASS_Z_MM.map(|z| lifts_from(&tp, z))
                };
                let (at_0_8, at_1_0) = (lifts(0.8), lifts(1.0));
                assert_eq!(at_1_0, at_0_8, "{wall_pass:?}, {entry:?}");
                // With the wall pass first, the lift from the wall ring to
                // the innermost ring is the second.
                let want = if wall_pass == WallPass::Last { 1 } else { 2 };
                assert_eq!(at_1_0, [want; 2], "{wall_pass:?}, {entry:?}");
            }
        }
    }

    /// The centre of every [`pin_shapes`] shape.
    const PIN_CENTRE: Point = Point::new(100.0, 80.0);

    /// Circles whose tool-centre radius (theirs less 3.175) falls between
    /// `r` and `s` at step-over 0.6, 0.8 and 1.0 or at a later ring, and
    /// rects whose tool-centre half-height does, with the 100 × 50 rect,
    /// all centred on [`PIN_CENTRE`].
    fn pin_shapes() -> Vec<(String, Polyline)> {
        let circles = [6.775, 7.475, 8.0, 8.175, 8.5, 9.0, 12.0, 20.0].map(|radius_mm| {
            (
                format!("circle r{radius_mm}"),
                Polyline::circle(PIN_CENTRE, radius_mm),
            )
        });
        let rects = [
            (100.0, 50.0),
            (40.0, 13.5),
            (40.0, 15.0),
            (40.0, 17.0),
            (40.0, 25.0),
        ]
        .map(|(w, h): (f64, f64)| {
            let corner = Point::new(PIN_CENTRE.x - w / 2.0, PIN_CENTRE.y - h / 2.0);
            (format!("rect {w}x{h}"), Polyline::rect(corner, w, h))
        });
        circles.into_iter().chain(rects).collect()
    }

    /// The wall ring (93.65 mm wide) and the island ring (the 20 mm circle
    /// grown by the tool radius: 26.35 mm wide) of the island fixture at
    /// `z_mm`.
    fn wall_and_island_rings(tp: &Toolpath, z_mm: f64) -> (Ring, Ring) {
        let rings = rings_at(tp, z_mm);
        let wall = rings
            .iter()
            .find(|r| (r.width_mm() - (100.0 - DIAMETER_MM)).abs() <= RING_TOL)
            .unwrap_or_else(|| panic!("no wall ring: {rings:?}"))
            .clone();
        let island = rings
            .iter()
            .find(|r| (r.width_mm() - (20.0 + DIAMETER_MM)).abs() <= RING_TOL)
            .unwrap_or_else(|| panic!("no island ring: {rings:?}"))
            .clone();
        (wall, island)
    }

    /// Port of PureCutCNC `pocketFinishIslandDirection.test.ts`: for one
    /// `direction` the island ring winds opposite to the pocket wall, so
    /// the cutter meets the material on the same side on both. Upstream's
    /// table reads CCW wall / CW island for conventional in Y-down; here
    /// the senses are mirrored by the Y flip (see the direction tests), but
    /// "opposite" is what this test pins, and that survives the flip.
    #[test]
    fn island_wall_runs_opposite_to_pocket_wall() {
        let p = island_project();
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let (tp, _) = generate(&p, &pocket_op(&p, direction));
            for z in PASS_Z_MM {
                let (wall, island) = wall_and_island_rings(&tp, z);
                assert!(
                    wall.area_mm2 * island.area_mm2 < 0.0,
                    "{direction:?} at Z {z}: wall {} island {}",
                    wall.area_mm2,
                    island.area_mm2
                );
            }
        }
    }

    /// Acceptance test 20 (#249): "the wall pass runs along the rectangle
    /// and around the circle". The offset wall pass is two rings, the
    /// outer wall (tool centre on the rect inset by r) and the island ring
    /// (tool centre on a circle of radius 10 + r about (70, 45)), cut as
    /// one pass: with `wall_pass: last` they are the last two rings of each
    /// Z, after every clearance ring; with `first`, the first two.
    #[test]
    fn offset_island_ring_is_in_the_wall_pass() {
        let p = island_project();
        let centre = Point::new(70.0, 45.0);
        let tool_centre = inset(wall(), RADIUS_MM);
        let is_wall = |r: &Ring| {
            let b = r.bounds;
            (b.min.x - tool_centre.min.x).abs() <= RING_TOL
                && (b.min.y - tool_centre.min.y).abs() <= RING_TOL
                && (b.max.x - tool_centre.max.x).abs() <= RING_TOL
                && (b.max.y - tool_centre.max.y).abs() <= RING_TOL
        };
        let is_island = |r: &Ring| {
            r.points
                .iter()
                .all(|q| (q.distance_to(centre) - (10.0 + RADIUS_MM)).abs() <= RING_TOL)
        };
        for wall_pass in [WallPass::Last, WallPass::First] {
            let mut op = pocket_op(&p, CutDirection::Climb);
            op.wall_pass = wall_pass;
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{wall_pass:?}: {warnings:?}");
            for z in PASS_Z_MM {
                let rings = rings_at(&tp, z);
                assert!(rings.len() > 2, "{wall_pass:?} at Z {z}: {rings:?}");
                let walls: Vec<usize> = (0..rings.len()).filter(|&i| is_wall(&rings[i])).collect();
                let islands: Vec<usize> =
                    (0..rings.len()).filter(|&i| is_island(&rings[i])).collect();
                assert_eq!(walls.len(), 1, "{wall_pass:?} at Z {z}: walls {walls:?}");
                assert_eq!(
                    islands.len(),
                    1,
                    "{wall_pass:?} at Z {z}: islands {islands:?}"
                );
                let mut pass = [walls[0], islands[0]];
                pass.sort_unstable();
                let want = match wall_pass {
                    WallPass::Last => [rings.len() - 2, rings.len() - 1],
                    _ => [0, 1],
                };
                assert_eq!(
                    pass,
                    want,
                    "{wall_pass:?} at Z {z}: wall ring {} and island ring {} of {}",
                    walls[0],
                    islands[0],
                    rings.len()
                );
            }
        }
    }

    /// Y-up re-derivation: climb on the pocket wall is counter-clockwise
    /// (phase 1's inside rule), and every interior ring follows it.
    #[test]
    fn climb_on_pocket_wall_is_counter_clockwise() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let rings = rings(&tp);
        assert!(!rings.is_empty());
        for ring in &rings {
            assert!(ring.area_mm2 > 0.0, "{ring:?}");
        }
    }

    /// Y-up re-derivation: climb round an island is clockwise, an outside
    /// wall, while the pocket's own rings stay counter-clockwise.
    #[test]
    fn climb_on_island_is_clockwise() {
        let p = island_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        for z in PASS_Z_MM {
            let (wall, island) = wall_and_island_rings(&tp, z);
            assert!(wall.area_mm2 > 0.0, "wall {wall:?}");
            assert!(island.area_mm2 < 0.0, "island {island:?}");
        }
    }

    /// Conventional is the reverse of climb on both walls.
    #[test]
    fn conventional_reverses_both() {
        let p = island_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Conventional));
        for z in PASS_Z_MM {
            let (wall, island) = wall_and_island_rings(&tp, z);
            assert!(wall.area_mm2 < 0.0, "wall {wall:?}");
            assert!(island.area_mm2 > 0.0, "island {island:?}");
        }
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Conventional));
        for ring in &rings(&tp) {
            assert!(ring.area_mm2 < 0.0, "{ring:?}");
        }
    }

    /// A 5 mm wide slot is narrower than the 6.35 mm tool: `PocketNoRoom`
    /// names it and it is not cut, while the rect beside it is.
    #[test]
    fn tool_wider_than_region_warns_and_skips() {
        let slot = Polyline::rect(Point::new(130.0, 0.0), 5.0, 40.0);
        let p = project(vec![rect_100x50(), slot], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::PocketNoRoom]);
        let Warning::PocketNoRoom {
            op, shape, tool, ..
        } = &warnings[0]
        else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 1));
        assert_eq!(tool, "6.35 mm upcut");
        assert_eq!(op.as_str(), "o1");
        for ring in &rings(&tp) {
            assert!(
                ring.bounds.max.x <= wall().max.x - RADIUS_MM + RING_TOL,
                "a ring in the slot: {ring:?}"
            );
        }
        assert_eq!(rings_at(&tp, PASS_Z_MM[0]).len(), 9, "the rect is cut");
    }

    /// A 10 mm bar island reaching to 1 mm of the top and bottom walls
    /// leaves no room for the tool past it, so the tool-centre region is
    /// two pieces, left and right of the bar, each with its own rings. The
    /// 1 mm gaps warn `PocketIslandTooClose`, and every move between the
    /// halves retracts to safe Z: no cutting move crosses the bar.
    #[test]
    fn island_splits_region_into_two_tool_centre_regions() {
        let bar = Polyline::rect(Point::new(45.0, 1.0), 10.0, 48.0);
        let p = project(vec![rect_100x50(), bar], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let close: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code() == WarningCode::PocketIslandTooClose)
            .collect();
        assert!(!close.is_empty(), "{warnings:?}");
        let Warning::PocketIslandTooClose {
            island,
            other,
            gap_mm,
            ..
        } = close[0]
        else {
            panic!("{:?}", close[0]);
        };
        assert_eq!((*island, *other), (1, IslandNeighbour::Wall));
        assert_approx_eq!(*gap_mm, 1.0, RING_TOL);
        assert!(
            warnings
                .iter()
                .all(|w| w.code() == WarningCode::PocketIslandTooClose),
            "{warnings:?}"
        );
        // The bar spans X 65..75; rings are wholly left or right of it.
        let (bar_min_x, bar_max_x) = (65.0, 75.0);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            let left = rings
                .iter()
                .filter(|r| r.bounds.max.x <= bar_min_x - RADIUS_MM + RING_TOL);
            let right = rings
                .iter()
                .filter(|r| r.bounds.min.x >= bar_max_x + RADIUS_MM - RING_TOL);
            assert_eq!(left.count() + right.count(), rings.len(), "{rings:?}");
            assert!(
                rings.iter().any(|r| r.bounds.max.x < bar_min_x),
                "a left ring"
            );
            assert!(
                rings.iter().any(|r| r.bounds.min.x > bar_max_x),
                "a right ring"
            );
            // The widest ring of each half hugs the bar: 45 − 6.35 wide.
            let widest = rings.iter().map(Ring::width_mm).fold(0.0, f64::max);
            assert_approx_eq!(widest, 45.0 - DIAMETER_MM, RING_TOL);
        }
        let mut pos = tp.moves[0].to;
        for (i, mv) in tp.moves.iter().enumerate() {
            if !mv.is_rapid() {
                let (a, b) = (pos.xy().x, mv.to.xy().x);
                assert!(
                    (a <= bar_min_x && b <= bar_min_x) || (a >= bar_max_x && b >= bar_max_x),
                    "move {i} crosses the bar: {pos:?} -> {mv:?}"
                );
            }
            pos = mv.to;
        }
    }

    /// #168: with stock to leave the tool needs `diameter + 2 × stock to leave`
    /// between an island and the wall. A 10 × 30 island 7 mm from the left
    /// wall leaves the 6.35 mm tool room at stock to leave 0, and none at 0.5
    /// (7.35 needed): the gap warns then, with `needed_mm` and
    /// `stock_to_leave_mm`.
    #[test]
    fn island_gap_warning_accounts_for_stock_to_leave() {
        let island = Polyline::rect(Point::new(7.0, 10.0), 10.0, 30.0);
        let p = project(vec![rect_100x50(), island], &[("i1", at_20_20())]);
        let mut op = pocket_op(&p, CutDirection::Climb);
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");

        op.stock_to_leave_mm = 0.5;
        let (_, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::PocketIslandTooClose],
            "{warnings:?}"
        );
        let w = &warnings[0];
        let Warning::PocketIslandTooClose {
            island,
            other,
            gap_mm,
            needed_mm,
            stock_to_leave_mm,
            diameter_mm,
            ..
        } = w
        else {
            panic!("{w:?}");
        };
        assert_eq!((*island, *other), (1, IslandNeighbour::Wall));
        assert_approx_eq!(*gap_mm, 7.0, RING_TOL);
        assert_approx_eq!(*needed_mm, DIAMETER_MM + 1.0, TOL);
        assert_approx_eq!(*stock_to_leave_mm, 0.5, TOL);
        assert_approx_eq!(*diameter_mm, DIAMETER_MM, TOL);
        assert_eq!(
            w.to_string(),
            "in the pocket of `Shape i1` (instance `i1`, profile 0), island profile 1 is 7 mm from the pocket wall, closer than tool `6.35 mm upcut` needs with 0.5 mm stock to leave (7.35 mm), so the material between them is left uncut"
        );
        // At stock to leave 0 the message is as before.
        let w = Warning::PocketIslandTooClose {
            op: "o1".into(),
            shape: ShapeRef {
                instance: "i1".into(),
                instance_name: "Shape i1".to_owned(),
                profile: 0,
            },
            island: 1,
            other: IslandNeighbour::Wall,
            gap_mm: 5.0,
            needed_mm: 6.35,
            stock_to_leave_mm: 0.0,
            tool: "t".to_owned(),
            tool_id: "t1".into(),
            diameter_mm: 6.35,
        };
        assert_eq!(
            w.to_string(),
            "in the pocket of `Shape i1` (instance `i1`, profile 0), island profile 1 is 5 mm from the pocket wall, closer than the 6.35 mm tool `t` is wide, so the material between them is left uncut"
        );
    }

    /// A 10 mm square inside a 30 mm circular island inside the rect is a
    /// pocket of its own (depth 2). Under full depth first every ring of
    /// it comes after every ring of the rect, which cuts the island's
    /// wall. The square's tool-centre square is 3.65 mm wide, less than
    /// two step-overs, so it is one ring per pass.
    #[test]
    fn pocket_inside_island_is_cut_after_island() {
        let p = project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 15.0),
                Polyline::rect(Point::new(45.0, 20.0), 10.0, 10.0),
            ],
            &[("i1", at_20_20())],
        );
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let rings = rings(&tp);
        let island_centre = Point::new(70.0, 45.0);
        let in_island = |r: &Ring| r.bounds.min.distance_to(island_centre) < 15.0;
        let inner: Vec<usize> = (0..rings.len()).filter(|&i| in_island(&rings[i])).collect();
        let outer: Vec<usize> = (0..rings.len())
            .filter(|&i| !in_island(&rings[i]))
            .collect();
        assert_eq!(inner.len(), PASS_Z_MM.len(), "one ring per pass: {rings:?}");
        assert!(outer.len() >= 2 * PASS_Z_MM.len(), "{rings:?}");
        assert!(
            outer.iter().max() < inner.iter().min(),
            "outer pocket first: outer {outer:?}, inner {inner:?}"
        );
        // The inner pocket's wall ring is 10 − 6.35 wide.
        let widest = inner
            .iter()
            .map(|&i| rings[i].width_mm())
            .fold(0.0, f64::max);
        assert_approx_eq!(widest, 10.0 - DIAMETER_MM, RING_TOL);
    }

    /// Step-down and step-over come from the operation, else the tool;
    /// neither set is an error, never a default.
    #[test]
    fn step_down_from_tool_when_operation_unset() {
        let p = rect_project();
        let op = pocket_op(&p, CutDirection::Climb);
        let (tp, _) = generate(&p, &op);
        let mut zs: Vec<f64> = rings(&tp).iter().map(|r| r.z_mm).collect();
        zs.dedup_by(|a, b| (*a - *b).abs() <= EPS_MM);
        assert_eq!(zs.len(), 2, "{zs:?}");
        assert_approx_eq!(zs[0], 4.0, TOL);
        assert_approx_eq!(zs[1], 2.0, TOL);

        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_down_mm = Some(4.0);
        let (tp, _) = generate(&p, &op);
        let rings_z: Vec<f64> = rings(&tp).iter().map(|r| r.z_mm).collect();
        assert!(
            rings_z.iter().all(|z| (z - 2.0).abs() <= TOL),
            "{rings_z:?}"
        );

        let mut p = rect_project();
        p.tools[0].step_down_mm = None;
        let op = pocket_op(&p, CutDirection::Climb);
        assert_eq!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::MissingStepDown {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
            }
        );

        // Step-over: the tool's 0.4 when the operation sets none; the
        // operation's 0.5 (3.175 mm) overrides it; neither is an error.
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_over_fraction = None;
        let (tp, _) = generate(&p, &op);
        assert_eq!(rings_at(&tp, PASS_Z_MM[0]).len(), 9);
        op.step_over_fraction = Some(0.5);
        let (tp, _) = generate(&p, &op);
        let mut rings = rings_at(&tp, PASS_Z_MM[0]);
        rings.sort_by(|a, b| b.area_mm2.abs().total_cmp(&a.area_mm2.abs()));
        assert_approx_eq!(
            rings[0].bounds.min.x + 3.175,
            rings[1].bounds.min.x,
            RING_TOL
        );
        let mut p = rect_project();
        p.tools[0].step_over_fraction = None;
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_over_fraction = None;
        assert_eq!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::MissingStepOver {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
            }
        );
        op.step_over_fraction = Some(1.5);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::InvalidStepOver { .. }
        ));
    }

    /// S1: a depth beyond the stock plus the allowance is an error.
    #[test]
    fn depth_below_bottom_is_error() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.depth_mm = Some(THICKNESS_MM + 0.5);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::BelowMaterialBottom { .. }
        ));
        op.through_cut_allowance_mm = 0.5;
        let (tp, _) = generate(&p, &op);
        let lowest = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        assert_approx_eq!(lowest, -0.5, TOL);
    }

    /// Port of `testPocketLevelTransitionsPlungeVerticallyFromSafeZ`
    /// (PureCutCNC `src/engine/toolpaths/toolpaths.test.ts`, "level
    /// transitions rapid at safe Z before plunging"): the first cut of the
    /// second pass is preceded by a vertical plunge at the plunge rate that
    /// ends where that cut starts, and the plunge comes down from a rapid
    /// at safe Z. Upstream's `-4` is the second pass Z of a 4 mm cut in 2
    /// mm steps; here that is Z 2 above the bottom of 6 mm stock. (changed
    /// in #227: the rapid from safe Z goes on straight down to 1 mm above
    /// the first pass's floor, Z 5, and the plunge starts there.)
    #[test]
    fn levels_rapid_at_safe_z_before_plunging() {
        let p = rect_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let m = &tp.moves;
        let first_deep = m
            .iter()
            .enumerate()
            .position(|(i, mv)| {
                !mv.is_rapid()
                    && (mv.to.z - PASS_Z_MM[1]).abs() <= EPS_MM
                    && i > 0
                    && m[i - 1].to.xy().distance_to(mv.to.xy()) > EPS_MM
            })
            .expect("a cut at the second pass");
        let plunge = m[first_deep - 1];
        let before = m[first_deep - 2];
        assert_eq!(plunge.kind, MoveKind::Linear, "{plunge:?}");
        assert_approx_eq!(plunge.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
        assert!(
            before.to.xy().distance_to(plunge.to.xy()) <= EPS_MM,
            "the plunge is vertical: {before:?} -> {plunge:?}"
        );
        assert_approx_eq!(plunge.to.z, PASS_Z_MM[1], TOL);
        assert!(before.is_rapid(), "{before:?}");
        assert_approx_eq!(before.to.z, PASS_Z_MM[0] + 1.0, TOL);
        let lifted = m[first_deep - 3];
        assert!(lifted.is_rapid(), "{lifted:?}");
        assert!(lifted.to.xy().distance_to(plunge.to.xy()) <= EPS_MM);
        assert_approx_eq!(lifted.to.z, SAFE_Z_MM, TOL);
        // And the ring continues from the plunge at the feed rate.
        assert_approx_eq!(m[first_deep].feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
    }

    /// Corner relief (slice 12) with a ramp entry: no warning, and the
    /// notches are cut from the wall rings (maintainer decision
    /// 2026-10-24): the Pocket has every move of the same Pocket without
    /// relief, in order, more moves, and no more rapids
    /// ([`crate::relief`] has the relief tests).
    #[test]
    fn corner_relief_is_cut_with_the_wall_rings() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = Entry::Ramp { angle_deg: 15.0 };
        let (plain, _) = generate(&p, &op);
        op.corner_relief = CornerRelief::Dogbone;
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut rest = tp.moves.iter();
        for mv in &plain.moves {
            assert!(rest.any(|m| m == mv), "{mv:?} is missing or out of order");
        }
        assert!(tp.moves.len() > plain.moves.len());
        let rapids = |t: &Toolpath| t.moves.iter().filter(|m| m.is_rapid()).count();
        assert_eq!(rapids(&tp), rapids(&plain));
    }

    /// The bounds of `regions`, outers only.
    fn regions_bounds(regions: &[Region]) -> Rect {
        regions
            .iter()
            .filter_map(|r| r.outer().polyline().bounds())
            .reduce(|a, b| a.including(b.min).including(b.max))
            .expect("some region")
    }

    /// The region of `rect`, a placed rectangle.
    fn rect_region(rect: Rect) -> Region {
        region_of(
            &[Polyline::rect(rect.min, rect.width_mm(), rect.height_mm())],
            Transform::IDENTITY,
        )
    }

    /// The area of `fp` outside `allowed`, which flattening alone keeps
    /// within a tolerance band along `allowed`'s boundary.
    fn area_outside(fp: &[Region], allowed: &Region) -> f64 {
        area_mm2(&difference(fp, std::slice::from_ref(allowed), FLAT_TOL))
    }

    /// Functional capability 12, design step 2 (S9 with stock to leave): at
    /// `stock_to_leave_mm` 0.5 the tool-centre region is the rect inset by
    /// `r + 0.5`, so the wall ring runs there and the footprint stays 0.5
    /// from every wall, with both strategies and with a helix entry. Round
    /// an island the footprint stays 0.5 off it too.
    #[test]
    fn stock_to_leave_stays_on_walls() {
        const STOCK_TO_LEAVE_MM: f64 = 0.5;
        let p = rect_project();
        let left = inset(wall(), STOCK_TO_LEAVE_MM);
        let allowed = rect_region(left);
        for strategy in [
            ClearanceStrategy::Offset,
            ClearanceStrategy::Raster { angle_deg: 0.0 },
        ] {
            for entry in [Entry::Plunge, helix_80()] {
                let mut op = pocket_op(&p, CutDirection::Climb);
                op.strategy = strategy.clone();
                op.entry = entry.clone();
                op.stock_to_leave_mm = STOCK_TO_LEAVE_MM;
                let what = format!("{strategy:?} {entry:?}");
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{what}: {warnings:?}");
                let fp = footprint(&tp, RADIUS_MM);
                assert!(
                    area_outside(&fp, &allowed) <= 1e-3,
                    "{what}: {} mm² cut into the stock to leave",
                    area_outside(&fp, &allowed)
                );
                assert_rect_eq(regions_bounds(&fp), left, RING_TOL, &what);
                if strategy != ClearanceStrategy::Offset {
                    continue;
                }
                // The wall ring is the tool-centre rect inset by the stock to leave.
                let wall_ring = rings_at(&tp, PASS_Z_MM[0])
                    .into_iter()
                    .max_by(|a, b| a.area_mm2.abs().total_cmp(&b.area_mm2.abs()))
                    .expect("a wall ring");
                assert_rect_eq(
                    wall_ring.bounds,
                    inset(wall(), RADIUS_MM + STOCK_TO_LEAVE_MM),
                    RING_TOL,
                    &what,
                );
            }
        }

        let p = island_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.stock_to_leave_mm = STOCK_TO_LEAVE_MM;
        let (tp, _) = generate(&p, &op);
        let centre = Point::new(70.0, 45.0);
        let nearest = tp
            .moves
            .iter()
            .filter(|m| !m.is_rapid())
            .map(|m| m.to.xy().distance_to(centre))
            .fold(f64::MAX, f64::min);
        assert!(
            nearest >= 10.0 + STOCK_TO_LEAVE_MM + RADIUS_MM - FLAT_TOL,
            "the tool centre comes {nearest} mm from the island centre"
        );
        assert!(nearest <= 10.0 + STOCK_TO_LEAVE_MM + RADIUS_MM + RING_TOL);
    }

    /// Rough tool of the rest tests: 12.7 mm.
    const ROUGH_MM: f64 = 12.7;
    /// Rest tool of the rest tests: 3.175 mm.
    const REST_MM: f64 = 3.175;
    /// The rough pocket's stock to leave in the rest tests.
    const ROUGH_STOCK_TO_LEAVE_MM: f64 = 0.5;

    /// Acceptance test 22's shape: the 100 × 50 rect with a 10 × 10 mm
    /// notch out of the middle of its top wall, placed at (20, 20): the
    /// notch spans X 65..75, Y 70..80.
    fn notched_rect() -> Polyline {
        let v = dc_geometry::Vertex::line;
        Polyline::new(
            vec![
                v(0.0, 0.0),
                v(100.0, 0.0),
                v(100.0, 50.0),
                v(55.0, 50.0),
                v(55.0, 60.0),
                v(45.0, 60.0),
                v(45.0, 50.0),
                v(0.0, 50.0),
            ],
            true,
        )
    }

    /// The placed notched rect's region.
    fn notched_region() -> Region {
        region_of(&[notched_rect()], at_20_20())
    }

    /// A tool like the sample tool, `diameter_mm` wide, with id `id`.
    fn tool_of(project: &Project, id: &str, diameter_mm: f64) -> Tool {
        let mut tool = project.tools[0].clone();
        tool.id = ToolId::new(id);
        tool.name = format!("{diameter_mm} mm end mill");
        tool.diameter_mm = diameter_mm;
        tool
    }

    /// The notched rect with a rough Pocket `o1` (tool `t1`, `rough_mm`,
    /// stock to leave `rough_stock_to_leave_mm`) and a rest Pocket `o2` of it (tool
    /// `t2`, `rest_mm`), both as [`pocket_op`]. The project holds both
    /// operations, in that order.
    fn rough_and_rest(
        rough_mm: f64,
        rough_stock_to_leave_mm: f64,
        rest_mm: f64,
    ) -> (Project, PocketOp, PocketOp) {
        let mut p = project(vec![notched_rect()], &[("i1", at_20_20())]);
        p.tools = vec![tool_of(&p, "t1", rough_mm), tool_of(&p, "t2", rest_mm)];
        let mut rough = pocket_op(&p, CutDirection::Climb);
        rough.stock_to_leave_mm = rough_stock_to_leave_mm;
        let mut rest = pocket_op(&p, CutDirection::Climb);
        rest.id = OperationId::new("o2");
        rest.name = "Rest".into();
        rest.tool = ToolId::new("t2");
        rest.rest_of = Some(rough.id.clone());
        p.operations = vec![
            dc_project::Operation::Pocket(rough.clone()),
            dc_project::Operation::Pocket(rest.clone()),
        ];
        (p, rough, rest)
    }

    /// The XY length of every cutting move, arcs along the arc, mm.
    fn cutting_length_mm(tp: &Toolpath) -> f64 {
        let mut pos = match tp.moves.first() {
            Some(m) => m.to.xy(),
            None => return 0.0,
        };
        let mut total = 0.0;
        for mv in &tp.moves {
            let to = mv.to.xy();
            total += match mv.kind {
                MoveKind::Linear => pos.distance_to(to),
                MoveKind::ArcCcw { centre } => {
                    pos.distance_to(centre) * sweep_rad(pos, to, centre, true).abs()
                }
                MoveKind::ArcCw { centre } => {
                    pos.distance_to(centre) * sweep_rad(pos, to, centre, false).abs()
                }
                _ => 0.0,
            };
            pos = to;
        }
        total
    }

    fn covers(fp: &[Region], p: Point) -> bool {
        fp.iter().any(|r| r.contains_point(p))
    }

    /// Design step 3: after a 12.7 mm rough at stock to leave 0.5, the 3.175 mm
    /// rest Pocket cuts the 0.5 mm band on every wall, the four corners and
    /// the notch, and nothing in the middle of the pocket. The rough left
    /// at most 2.36 mm from a wall (its corner wedges, `6.85 − 6.35/√2`)
    /// besides the notch, so no rest footprint is more than that plus the
    /// rest tool's diameter from a wall.
    #[test]
    fn rest_region_is_corners_and_wall_band() {
        let (p, _, rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        let (tp, warnings) = generate(&p, &rest);
        assert!(warnings.is_empty(), "{warnings:?}");
        let fp = footprint(&tp, REST_MM / 2.0);
        assert!(!fp.is_empty(), "the rest Pocket cuts");
        // The band, half way into it, on all four walls.
        for q in [
            Point::new(20.25, 45.0),
            Point::new(119.75, 45.0),
            Point::new(50.0, 20.25),
            Point::new(50.0, 69.75),
        ] {
            assert!(covers(&fp, q), "band at {q:?}");
        }
        // Each corner as close as the rest tool reaches.
        let near = REST_MM / 2.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2) + 0.05;
        for (x, y, dx, dy) in [
            (20.0, 20.0, 1.0, 1.0),
            (120.0, 20.0, -1.0, 1.0),
            (120.0, 70.0, -1.0, -1.0),
            (20.0, 70.0, 1.0, -1.0),
            (65.0, 80.0, 1.0, -1.0),
            (75.0, 80.0, -1.0, -1.0),
        ] {
            let q = Point::new(x + dx * near, y + dy * near);
            assert!(covers(&fp, q), "corner at {q:?}");
        }
        // The notch, across.
        for q in [Point::new(70.0, 76.0), Point::new(66.0, 78.0)] {
            assert!(covers(&fp, q), "notch at {q:?}");
        }
        // Nothing in the middle: the region inset by 2.36 + 3.175 + a margin
        // is untouched (the notch is gone from it, being 10 wide).
        let core = inset_region(&notched_region(), 2.36 + REST_MM + 0.1, FLAT_TOL);
        assert_eq!(core.len(), 1);
        let inside = intersection(&fp, &core, FLAT_TOL);
        assert!(
            area_mm2(&inside) <= 1e-3,
            "{} mm² of the middle recut",
            area_mm2(&inside)
        );
    }

    /// Acceptance test 22: the 12.7 mm rough at stock to leave 0.5 and the
    /// 3.175 mm rest together cover the notched region inset by 1.5875
    /// within 0.05 mm (as area: 0.05 × its perimeter), and the rest's
    /// cutting length is under a quarter of a full 3.175 mm pocket's.
    #[test]
    fn rest_coverage_with_rough_reaches_small_tool_inset() {
        let (p, rough, rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        let (rough_tp, rough_warnings) = generate(&p, &rough);
        assert!(rough_warnings.is_empty(), "{rough_warnings:?}");
        let (rest_tp, rest_warnings) = generate(&p, &rest);
        assert!(rest_warnings.is_empty(), "{rest_warnings:?}");
        let both = dc_geometry::union(
            &footprint(&rough_tp, ROUGH_MM / 2.0),
            &footprint(&rest_tp, REST_MM / 2.0),
            FLAT_TOL,
        );
        let reachable = inset_region(&notched_region(), REST_MM / 2.0, FLAT_TOL);
        assert_eq!(reachable.len(), 1);
        let perimeter_mm = reachable[0].outer().polyline().length_mm();
        let uncut = difference(&reachable, &both, FLAT_TOL);
        assert!(
            area_mm2(&uncut) <= 0.05 * perimeter_mm,
            "{} mm² the rest tool reaches is left: {uncut:?}",
            area_mm2(&uncut)
        );
        // Without the rest Pocket the band and the notch are left.
        let rough_only = difference(&reachable, &footprint(&rough_tp, ROUGH_MM / 2.0), FLAT_TOL);
        assert!(area_mm2(&rough_only) > 0.05 * perimeter_mm);

        let mut full = rest.clone();
        full.rest_of = None;
        let (full_tp, _) = generate(&p, &full);
        let (rest_mm, full_mm) = (cutting_length_mm(&rest_tp), cutting_length_mm(&full_tp));
        assert!(
            rest_mm < 0.25 * full_mm,
            "rest cuts {rest_mm} mm, a full pocket {full_mm} mm"
        );
    }

    /// The area `ring` sweeps with a tool of radius `radius_mm`.
    fn ring_footprint(ring: &Ring, radius_mm: f64) -> Vec<Region> {
        let start = ring.start();
        let mut moves = vec![Move::rapid(Point3::new(start.x, start.y, ring.z_mm))];
        moves.extend(ring.moves.iter().copied());
        footprint(&Toolpath { moves }, radius_mm)
    }

    /// The XY length of `ring`'s moves, mm.
    fn ring_length_mm(ring: &Ring) -> f64 {
        let start = ring.start();
        let mut moves = vec![Move::rapid(Point3::new(start.x, start.y, ring.z_mm))];
        moves.extend(ring.moves.iter().copied());
        cutting_length_mm(&Toolpath { moves })
    }

    /// The rest Pocket's cutting length on acceptance 22 before #167, mm:
    /// both passes, the ring along the band's inner edge included.
    const REST_BEFORE_167_MM: f64 = 1237.0;

    /// #167: with the offset strategy no rest ring runs wholly in the area
    /// the rough cleared. Each ring's footprint takes off more of what the
    /// rough left than a noise strip along it (`FLAT_TOL` × its length),
    /// so the ring along the wall band's inner edge, 276 mm a pass in air,
    /// is gone and the rest cuts at least 270 mm less on each of its two
    /// passes. Coverage is unchanged:
    /// `rest_coverage_with_rough_reaches_small_tool_inset`.
    #[test]
    fn rest_offset_cuts_no_ring_entirely_in_air() {
        let (p, _, rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        let (tp, warnings) = generate(&p, &rest);
        assert!(warnings.is_empty(), "{warnings:?}");
        let reference = rest::Reference {
            radius_mm: ROUGH_MM / 2.0,
            stock_to_leave_mm: ROUGH_STOCK_TO_LEAVE_MM,
        };
        let left = rest::left_by(&notched_region(), 0.0, reference, FLAT_TOL);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            assert!(!rings.is_empty(), "rings at Z {z}");
            for ring in &rings {
                let length_mm = ring_length_mm(ring);
                let cut = intersection(&ring_footprint(ring, REST_MM / 2.0), &left, FLAT_TOL);
                assert!(
                    area_mm2(&cut) > FLAT_TOL * length_mm,
                    "a {length_mm} mm ring at Z {z} cuts {} mm² of what the rough left: {:?}",
                    area_mm2(&cut),
                    ring.bounds
                );
            }
        }
        let rest_mm = cutting_length_mm(&tp);
        assert!(
            rest_mm <= REST_BEFORE_167_MM - 2.0 * 270.0,
            "the rest cuts {rest_mm} mm"
        );
    }

    /// The raster rest Pocket's cutting length on acceptance 22 before
    /// #167, mm: both passes, the wall pass round the band's inner edge
    /// included.
    const RASTER_REST_BEFORE_167_MM: f64 = 1602.0;

    /// #167: the raster strategy's wall pass skips the ring in air too;
    /// its lines already end at the rest tool-centre region's edge.
    #[test]
    fn rest_raster_wall_pass_skips_the_ring_in_air() {
        let (p, rough, mut rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        rest.strategy = ClearanceStrategy::Raster { angle_deg: 0.0 };
        let (tp, warnings) = generate(&p, &rest);
        assert!(warnings.is_empty(), "{warnings:?}");
        let rest_mm = cutting_length_mm(&tp);
        assert!(
            rest_mm <= RASTER_REST_BEFORE_167_MM - 2.0 * 270.0,
            "the rest cuts {rest_mm} mm"
        );
        // Still everything the rest tool reaches, with the rough.
        let (rough_tp, _) = generate(&p, &rough);
        let both = dc_geometry::union(
            &footprint(&rough_tp, ROUGH_MM / 2.0),
            &footprint(&tp, REST_MM / 2.0),
            FLAT_TOL,
        );
        let reachable = inset_region(&notched_region(), REST_MM / 2.0, FLAT_TOL);
        let perimeter_mm = reachable[0].outer().polyline().length_mm();
        let uncut = difference(&reachable, &both, FLAT_TOL);
        assert!(
            area_mm2(&uncut) <= 0.05 * perimeter_mm,
            "{} mm² the rest tool reaches is left",
            area_mm2(&uncut)
        );
    }

    /// Design step 3: a rest Pocket whose tool reaches nothing its
    /// reference left (a larger tool, or the same tool at the same
    /// stock to leave) warns `RestOfClearsNothing` and emits no cutting move.
    #[test]
    fn rest_of_that_clears_nothing_warns() {
        for (rough_mm, rough_stock_to_leave_mm, rest_mm, rest_stock_to_leave_mm) in [
            (REST_MM, 0.0, ROUGH_MM, 0.0),
            (DIAMETER_MM, 0.0, DIAMETER_MM, 0.0),
            (DIAMETER_MM, 0.5, DIAMETER_MM, 0.5),
        ] {
            let what = format!("rough {rough_mm} at {rough_stock_to_leave_mm}, rest {rest_mm}");
            let (mut p, _, mut rest) = rough_and_rest(rough_mm, rough_stock_to_leave_mm, rest_mm);
            // On the plain rect: the notch would leave room for a small tool.
            p.definitions
                .values_mut()
                .for_each(|d| d.profiles = vec![rect_100x50()]);
            rest.stock_to_leave_mm = rest_stock_to_leave_mm;
            let (tp, warnings) = generate(&p, &rest);
            assert_eq!(
                codes(&warnings),
                vec![WarningCode::RestOfClearsNothing],
                "{what}"
            );
            let Warning::RestOfClearsNothing {
                op, rest_of, tool, ..
            } = &warnings[0]
            else {
                panic!("{what}: {:?}", warnings[0]);
            };
            assert_eq!(op.as_str(), "o2", "{what}");
            assert_eq!(rest_of.as_str(), "o1", "{what}");
            assert_eq!(*tool, format!("{rest_mm} mm end mill"), "{what}");
            assert!(
                tp.moves.iter().all(Move::is_rapid),
                "{what}: no cutting move: {tp:?}"
            );
        }
        let w = Warning::RestOfClearsNothing {
            op: "o2".into(),
            rest_of: "o1".into(),
            tool: "12.7 mm end mill".to_owned(),
            tool_id: "t2".into(),
        };
        assert_eq!(
            w.to_string(),
            "operation `o1`, which this one rests on, already reaches everything tool `12.7 mm end mill` can, so this operation cuts nothing; use a smaller tool, or remove it"
        );
    }

    /// How much of a rest footprint may lie off its allowed area: the
    /// rest tool-centre region is a boolean result, flattened, so its
    /// footprint strays by rounding along the walls (0.01 mm² on a 320 mm
    /// boundary is 3e-5 mm of stray width).
    const REST_OUTSIDE_MM2: f64 = 0.01;

    /// S14: the rest footprint is bounded by the operation's own region,
    /// whatever the reference: inside the notched region always, and with a
    /// reference whose stock to leave gives it no room at all the rest Pocket
    /// is the whole pocket, still inside. A reference that is not an
    /// earlier enabled Pocket with the same targets is an error, as in
    /// validation.
    #[test]
    fn rest_footprint_stays_inside_region() {
        let region = notched_region();
        let (p, _, rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        let (tp, _) = generate(&p, &rest);
        let fp = footprint(&tp, REST_MM / 2.0);
        assert!(
            area_outside(&fp, &region) <= REST_OUTSIDE_MM2,
            "{}",
            area_outside(&fp, &region)
        );

        // A reference that reaches nothing: the rest is a full pocket.
        let (p, _, rest) = rough_and_rest(ROUGH_MM, 30.0, REST_MM);
        let (tp, _) = generate(&p, &rest);
        let fp = footprint(&tp, REST_MM / 2.0);
        assert!(
            area_outside(&fp, &region) <= REST_OUTSIDE_MM2,
            "{}",
            area_outside(&fp, &region)
        );
        let mut full = rest.clone();
        full.rest_of = None;
        let (full_tp, _) = generate(&p, &full);
        assert_approx_eq!(
            area_mm2(&fp),
            area_mm2(&footprint(&full_tp, REST_MM / 2.0)),
            1.0
        );

        // With its own stock to leave the rest stays that far off the walls.
        let (p, _, mut rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        rest.stock_to_leave_mm = 0.2;
        let (tp, _) = generate(&p, &rest);
        let fp = footprint(&tp, REST_MM / 2.0);
        let allowed = inset_region(&region, 0.2, FLAT_TOL);
        assert_eq!(allowed.len(), 1);
        assert!(area_outside(&fp, &allowed[0]) <= REST_OUTSIDE_MM2);
        assert!(!fp.is_empty());

        // Invalid references.
        let (p, rough, rest) = rough_and_rest(ROUGH_MM, ROUGH_STOCK_TO_LEAVE_MM, REST_MM);
        let invalid = |p: &Project, rest: &PocketOp| {
            assert_eq!(
                generate_pocket(p, rest).unwrap_err(),
                CamError::RestOfInvalid {
                    op: rest.id.clone(),
                    rest_of: rest.rest_of.clone().unwrap(),
                }
            );
        };
        let mut dangling = rest.clone();
        dangling.rest_of = Some(OperationId::new("nope"));
        invalid(&p, &dangling);
        let mut itself = rest.clone();
        itself.rest_of = Some(rest.id.clone());
        invalid(&p, &itself);
        // A later Pocket.
        let mut later = p.clone();
        later.operations.reverse();
        invalid(&later, &rest);
        // A disabled one.
        let mut disabled = p.clone();
        let mut off = rough.clone();
        off.enabled = false;
        disabled.operations[0] = dc_project::Operation::Pocket(off);
        invalid(&disabled, &rest);
        // One with other targets.
        let mut other = p.clone();
        let mut elsewhere = rough.clone();
        elsewhere.targets = vec![InstanceId::new("i2")];
        other.operations[0] = dc_project::Operation::Pocket(elsewhere);
        invalid(&other, &rest);
        // Not a Pocket.
        let mut profile = p.clone();
        let mut not_pocket = dc_project::test_support::sample_project().operations[0].clone();
        if let dc_project::Operation::Profile(op) = &mut not_pocket {
            op.id = rough.id.clone();
            op.targets = rough.targets.clone();
        }
        profile.operations[0] = not_pocket;
        invalid(&profile, &rest);
        let err = CamError::RestOfInvalid {
            op: OperationId::new("o2"),
            rest_of: OperationId::new("o1"),
        };
        assert_eq!(
            err.to_string(),
            "operation `o2` rests on `o1`, which is not an earlier enabled Pocket with the same targets; no toolpath was produced"
        );
    }

    fn helix_80() -> Entry {
        Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        }
    }

    fn assert_in_rect(q: Point, r: Rect, what: &str) {
        assert!(
            q.x >= r.min.x - TOL
                && q.x <= r.max.x + TOL
                && q.y >= r.min.y - TOL
                && q.y <= r.max.y + TOL,
            "{what} {q:?} leaves {r:?}"
        );
    }

    /// True when `mv` from `from` falls while it moves in XY: a ramp or
    /// helix move.
    fn sloped(from: Point3, mv: &Move) -> bool {
        !mv.is_rapid() && mv.to.z < from.z - EPS_MM && from.xy().distance_to(mv.to.xy()) > EPS_MM
    }

    /// Every sloped move with the point it starts from.
    fn sloped_moves(tp: &Toolpath) -> Vec<(Point3, Move)> {
        tp.moves
            .windows(2)
            .filter(|w| sloped(w[0].to, &w[1]))
            .map(|w| (w[0].to, w[1]))
            .collect()
    }

    /// The slope of `mv` from `from`, degrees below the horizontal.
    fn slope_deg(from: Point3, mv: &Move) -> f64 {
        (from.z - mv.to.z)
            .atan2(from.xy().distance_to(mv.to.xy()))
            .to_degrees()
    }

    fn entry_warnings(warnings: &[Warning]) -> Vec<&Warning> {
        warnings
            .iter()
            .filter(|w| {
                matches!(
                    w.code(),
                    WarningCode::EntryFallback | WarningCode::HelixDiameterReduced
                )
            })
            .collect()
    }

    /// Acceptance test 27, first part: a Pocket with `entry: helix` at 80 %
    /// and a 6.35 mm tool descends on a helix 5.08 mm across, then turns
    /// one flat revolution at the pass Z, all inside the region. The first
    /// pass's helix starts 1 mm above the stock top, reached by a rapid
    /// straight down from safe Z; later passes start 1 mm above the
    /// previous pass Z, never higher in the air above the floor already
    /// cut. (changed in #214: the first pass's helix started at safe Z, and
    /// later passes at the previous pass Z.)
    #[test]
    fn helix_80_percent_is_5_08_mm_for_6_35_tool() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = helix_80();
        let (tp, warnings) = generate(&p, &op);
        assert!(entry_warnings(&warnings).is_empty(), "{warnings:?}");
        let m = &tp.moves;
        let first = m
            .windows(2)
            .position(|w| sloped(w[0].to, &w[1]))
            .expect("a helix")
            + 1;
        assert!(m[first - 1].is_rapid(), "{:?}", m[first - 1]);
        assert_approx_eq!(m[first - 1].to.z, THICKNESS_MM + 1.0, TOL);
        assert!(m[first - 2].is_rapid(), "{:?}", m[first - 2]);
        assert_approx_eq!(m[first - 2].to.z, SAFE_Z_MM, TOL);
        assert!(m[first - 2].to.xy().distance_to(m[first - 1].to.xy()) <= TOL);
        let mut end = first;
        while end < m.len() && sloped(m[end - 1].to, &m[end]) {
            end += 1;
        }
        assert_approx_eq!(m[end - 1].to.z, PASS_Z_MM[0], TOL);
        let flat = &m[end..end + 48];
        for mv in flat {
            assert_approx_eq!(mv.to.z, PASS_Z_MM[0], TOL);
            assert_approx_eq!(mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
        }
        assert!(
            flat[47].to.xy().distance_to(m[end - 1].to.xy()) <= 1e-6,
            "the flat revolution closes"
        );
        let n = 48.0;
        let centre = Point::new(
            flat.iter().map(|mv| mv.to.x).sum::<f64>() / n,
            flat.iter().map(|mv| mv.to.y).sum::<f64>() / n,
        );
        for mv in m[first..end].iter().chain(flat) {
            let diameter = 2.0 * mv.to.xy().distance_to(centre);
            assert_approx_eq!(diameter, 5.08, 0.01);
        }
        let centre_area = inset(wall(), RADIUS_MM);
        for (from, mv) in sloped_moves(&tp) {
            if mv.to.z < PASS_Z_MM[0] - TOL {
                assert!(
                    from.z <= PASS_Z_MM[0] + 1.0 + TOL,
                    "a second-pass descent starts above the first floor: {from:?} -> {mv:?}"
                );
            }
            assert_in_rect(mv.to.xy(), centre_area, "helix point");
        }
        // The descents are in the tool-centre region; the rest by footprint.
        let region = region_of(&[rect_100x50()], at_20_20());
        let fp = footprint(&without_descents(&tp), RADIUS_MM);
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        assert!(area_mm2(&outside) <= 1e-3, "{}", area_mm2(&outside));
    }

    /// A 40 × 7 mm slot: its tool-centre region is 0.65 mm wide.
    fn slot_project() -> Project {
        project(
            vec![Polyline::rect(Point::new(0.0, 0.0), 40.0, 7.0)],
            &[("i1", at_20_20())],
        )
    }

    /// Acceptance test 27, second part: in a slot too narrow for a helix
    /// the entry falls back to a ramp, warns `entry_fallback` naming the
    /// operation and shape, and ramps inside the slot's tool-centre region
    /// at the entry's angle. The spec's "6 mm slot" is narrower than the
    /// 6.35 mm tool, which leaves no tool-centre region at all
    /// (`pocket_no_room`); 7 mm is the narrowest whole size the tool fits.
    #[test]
    fn helix_in_narrow_slot_falls_back_to_ramp() {
        let p = slot_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = helix_80();
        let (tp, warnings) = generate(&p, &op);
        let entry = entry_warnings(&warnings);
        assert_eq!(entry.len(), 1, "{warnings:?}");
        let w = entry[0];
        assert_eq!(w.code(), WarningCode::EntryFallback);
        let Warning::EntryFallback {
            op,
            shape,
            requested,
            used,
            ..
        } = w
        else {
            panic!("{w:?}");
        };
        assert_eq!(op.as_str(), "o1");
        assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 0));
        assert_eq!((*requested, *used), (EntryKind::Helix, EntryKind::Ramp));
        let s = sloped_moves(&tp);
        assert!(!s.is_empty(), "a ramp is cut");
        let centre_area = inset(
            Rect {
                min: Point::new(20.0, 20.0),
                max: Point::new(60.0, 27.0),
            },
            RADIUS_MM,
        );
        for (from, mv) in s {
            for q in [from.xy(), mv.to.xy()] {
                assert!(
                    q.x >= centre_area.min.x - TOL
                        && q.x <= centre_area.max.x + TOL
                        && q.y >= centre_area.min.y - TOL
                        && q.y <= centre_area.max.y + TOL,
                    "ramp point {q:?} leaves the slot's tool-centre region"
                );
            }
            assert_approx_eq!(slope_deg(from, &mv), 5.0, 0.1);
        }
    }

    /// Acceptance test 27, third part: in a hole barely wider than the
    /// tool neither a helix nor a ramp fits; the warning says `plunge` and
    /// every descent is straight down at the plunge rate. The spec's "2 mm
    /// hole" is smaller than the 6.35 mm tool (`pocket_no_room`); a 7 mm
    /// hole leaves a 0.65 mm tool-centre circle, as acceptance 26's hole.
    #[test]
    fn helix_in_small_hole_falls_back_to_plunge() {
        let p = project(
            vec![Polyline::circle(Point::new(10.0, 10.0), 3.5)],
            &[("i1", at_20_20())],
        );
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = helix_80();
        let (tp, warnings) = generate(&p, &op);
        let entry = entry_warnings(&warnings);
        assert_eq!(entry.len(), 1, "{warnings:?}");
        assert_eq!(entry[0].code(), WarningCode::EntryFallback);
        assert!(
            matches!(
                entry[0],
                Warning::EntryFallback {
                    requested: EntryKind::Helix,
                    used: EntryKind::Plunge,
                    ..
                }
            ),
            "{:?}",
            entry[0]
        );
        assert!(entry[0].to_string().contains("plunge"), "{}", entry[0]);
        assert!(sloped_moves(&tp).is_empty(), "no ramp or helix");
        let plunges = tp
            .moves
            .windows(2)
            .filter(|w| !w[1].is_rapid() && w[1].to.z < w[0].to.z - EPS_MM)
            .count();
        assert_eq!(plunges, PASS_Z_MM.len(), "one plunge per pass");
    }

    /// S9 with descents: on the rect with the 20 mm island, every helix
    /// stays in the pocket's tool-centre region and off the island, and the
    /// whole footprint stays inside the rect and out of the island.
    #[test]
    fn helix_entry_footprint_stays_in_region() {
        let p = island_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = helix_80();
        let (tp, warnings) = generate(&p, &op);
        assert!(entry_warnings(&warnings).is_empty(), "{warnings:?}");
        let s = sloped_moves(&tp);
        assert!(!s.is_empty(), "helices are cut");
        let island_centre = Point::new(70.0, 45.0);
        let centre_area = inset(wall(), RADIUS_MM);
        for (from, mv) in s {
            for q in [from.xy(), mv.to.xy()] {
                assert!(
                    q.distance_to(island_centre) >= 10.0 + RADIUS_MM - FLAT_TOL,
                    "helix point {q:?} brings the tool onto the island"
                );
                assert!(
                    q.x >= centre_area.min.x - TOL
                        && q.x <= centre_area.max.x + TOL
                        && q.y >= centre_area.min.y - TOL
                        && q.y <= centre_area.max.y + TOL,
                    "helix point {q:?} brings the tool through the wall"
                );
            }
        }
        let region = region_of(
            &[
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ],
            at_20_20(),
        );
        let fp = footprint(&without_descents(&tp), RADIUS_MM);
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        let band_mm2 = 2.0 * FLAT_TOL * TAU * 10.0;
        assert!(
            area_mm2(&outside) <= band_mm2,
            "footprint off the material: {} mm²",
            area_mm2(&outside)
        );
    }

    /// Capability 16: a 15° ramp descends at 15° (within 0.1°), at the
    /// feed rate (1000 × sin 15° = 259 mm/min, under the 300 plunge, so
    /// unscaled), at least 0.1 · D inside the tool-centre region.
    #[test]
    fn ramp_entry_descends_at_its_angle() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = Entry::Ramp { angle_deg: 15.0 };
        let (tp, warnings) = generate(&p, &op);
        assert!(entry_warnings(&warnings).is_empty(), "{warnings:?}");
        let s = sloped_moves(&tp);
        assert!(!s.is_empty(), "a ramp is cut");
        let clear = inset(
            wall(),
            RADIUS_MM + crate::entry::boundary_safety_mm(DIAMETER_MM) - TOL,
        );
        for (from, mv) in s {
            assert_approx_eq!(slope_deg(from, &mv), 15.0, 0.1);
            assert_approx_eq!(mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
            let q = mv.to.xy();
            assert!(
                q.x >= clear.min.x
                    && q.x <= clear.max.x
                    && q.y >= clear.min.y
                    && q.y <= clear.max.y,
                "ramp point {q:?} is within the boundary safety"
            );
        }
    }

    /// S10 on the toolpath: with a 50 mm/min plunge a 5° helix at feed
    /// 1000 would descend at 87 mm/min, so every helix move's feed is
    /// scaled until its vertical component is the plunge rate (the safety
    /// assertions check the exact value).
    #[test]
    fn helix_feed_is_plunge_limited() {
        let mut p = rect_project();
        p.tools[0].plunge_mm_min = Some(50.0);
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = helix_80();
        let (tp, _) = generate(&p, &op);
        let s = sloped_moves(&tp);
        assert!(!s.is_empty());
        for (from, mv) in s {
            let f = mv.feed_mm_min.unwrap();
            assert!(f < FEED_MM_MIN, "scaled: {f}");
            let length = ((mv.to.x - from.x).powi(2)
                + (mv.to.y - from.y).powi(2)
                + (mv.to.z - from.z).powi(2))
            .sqrt();
            let vertical = f * (from.z - mv.to.z) / length;
            assert!(vertical <= 50.0 + 1e-6, "{vertical} mm/min: {mv:?}");
        }
    }

    /// Design step 9 for raster: a raster line reached from safe Z starts
    /// with the entry too, placed at the line's start in the root's
    /// tool-centre region, off the island.
    #[test]
    fn raster_lines_take_the_entry() {
        let p = island_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.strategy = ClearanceStrategy::Raster { angle_deg: 0.0 };
        op.entry = helix_80();
        let (tp, warnings) = generate(&p, &op);
        assert!(entry_warnings(&warnings).is_empty(), "{warnings:?}");
        let s = sloped_moves(&tp);
        assert!(!s.is_empty(), "helices are cut");
        let island_centre = Point::new(70.0, 45.0);
        let centre_area = inset(wall(), RADIUS_MM);
        for (_, mv) in s {
            let q = mv.to.xy();
            assert_in_rect(q, centre_area, "helix point");
            assert!(
                q.distance_to(island_centre) >= 10.0 + RADIUS_MM - FLAT_TOL,
                "helix point {q:?} brings the tool onto the island"
            );
        }
    }

    /// S9 on descents: a ramp whose tool centre leaves the tool-centre
    /// region takes the tool through the wall, and the check refuses it;
    /// the same ramp kept inside passes.
    #[test]
    fn descent_through_the_wall_is_off_material() {
        let p = rect_project();
        let op = pocket_op(&p, CutDirection::Climb);
        let region = region_of(&[rect_100x50()], at_20_20());
        let centre = inset_region(&region, RADIUS_MM, FLAT_TOL);
        let shapes = vec![(
            ShapeRef {
                instance: InstanceId::new("i1"),
                instance_name: "Shape i1".into(),
                profile: 0,
            },
            region.outer().clone(),
        )];
        let ramp_to = |x: f64| Toolpath {
            moves: vec![
                Move::rapid(Point3::new(50.0, 40.0, SAFE_Z_MM)),
                Move::linear(Point3::new(50.0, 40.0, THICKNESS_MM), PLUNGE_MM_MIN),
                Move::linear(Point3::new(x, 40.0, PASS_Z_MM[0]), FEED_MM_MIN),
                Move::rapid(Point3::new(x, 40.0, SAFE_Z_MM)),
            ],
        };
        let check = |tp: &Toolpath| {
            check_in_material(
                &op,
                tp,
                RADIUS_MM,
                FLAT_TOL,
                std::slice::from_ref(&region),
                std::slice::from_ref(&region),
                &centre,
                &shapes,
            )
        };
        check(&ramp_to(30.0)).unwrap();
        let err = check(&ramp_to(21.0)).unwrap_err();
        assert!(
            matches!(err, CamError::OffMaterial { area_mm2, .. } if area_mm2 > 1.0),
            "{err:?}"
        );
    }

    /// Validation's ranges, enforced again by the generator: an entry angle
    /// outside (0, 45] or a helix fraction outside (0, 1] is an error.
    #[test]
    fn invalid_entry_is_an_error() {
        let p = rect_project();
        for entry in [
            Entry::Ramp { angle_deg: 0.0 },
            Entry::Ramp { angle_deg: 46.0 },
            Entry::Helix {
                diameter_fraction: 0.0,
                angle_deg: 5.0,
            },
            Entry::Helix {
                diameter_fraction: 1.5,
                angle_deg: 5.0,
            },
        ] {
            let mut op = pocket_op(&p, CutDirection::Climb);
            op.entry = entry.clone();
            assert!(
                matches!(generate_pocket(&p, &op), Err(CamError::InvalidEntry { .. })),
                "{entry:?}"
            );
        }
    }

    #[test]
    fn output_is_deterministic() {
        let p = island_project();
        let op = pocket_op(&p, CutDirection::Climb);
        assert_eq!(
            generate_pocket(&p, &op).unwrap(),
            generate_pocket(&p, &op).unwrap()
        );
    }

    /// Every ring starts at the point nearest the previous ring's end
    /// (`rotateContourToNearestEntry`): the stay-down link between two
    /// rings of one pass is never longer than one step-over and a bit.
    #[test]
    fn rings_start_nearest_the_previous_ring() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let rings = rings_at(&tp, PASS_Z_MM[0]);
        for pair in rings.windows(2) {
            let link_mm = pair[0].start().distance_to(pair[1].start());
            assert!(
                link_mm <= STEPOVER_MM * std::f64::consts::SQRT_2 + RING_TOL,
                "link of {link_mm} mm between {:?} and {:?}",
                pair[0].start(),
                pair[1].start()
            );
        }
    }

    /// #196: every ring starts at the point of its contour nearest the end
    /// of the ring before it, arcs included, not at its nearest vertex. A
    /// ring round the circular island is two arcs; its nearest vertex can
    /// be 7.6 mm from the previous ring where its nearest point is a
    /// step-over away (PureCutCNC's dense polygon ring starts there).
    #[test]
    fn ring_starts_at_nearest_point_not_nearest_vertex() {
        let p = island_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let centre = Point::new(70.0, 45.0);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            let mut circles = 0;
            for pair in rings.windows(2) {
                let (before, ring) = (&pair[0], &pair[1]);
                let link_mm = before.start().distance_to(ring.start());
                // A true circle: two arcs, three once split at its start.
                // (Rings the boolean difference flattens are polygons;
                // they start at their nearest vertex, as upstream's do.)
                let radius_mm = ring.start().distance_to(centre);
                let is_circle = ring.points.len() <= 4
                    && ring
                        .points
                        .iter()
                        .all(|q| (q.distance_to(centre) - radius_mm).abs() <= RING_TOL);
                if !is_circle {
                    continue;
                }
                // The nearest point of a circle lies on the line from its
                // centre: the link is the gap to the circle.
                let gap_mm = (before.start().distance_to(centre) - radius_mm).abs();
                assert_approx_eq!(link_mm, gap_mm, RING_TOL);
                // From the ring just inside it, that is a step-over.
                if link_mm <= STEPOVER_MM + RING_TOL {
                    circles += 1;
                }
            }
            assert!(
                circles >= 1,
                "Z {z}: no ring round the island a step-over from the one before"
            );
        }
    }

    #[test]
    fn unknown_tool_and_other_cutters_are_errors() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.tool = ToolId::new("nope");
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::UnknownTool { .. }
        ));
        let mut p = rect_project();
        p.tools[0].cutter = dc_project::ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let op = pocket_op(&p, CutDirection::Climb);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::CutterNotFlat { .. }
        ));
        let p = rect_project();
        let op = pocket_op(&p, CutDirection::Climb);
        let err = generate_pocket(
            &{
                let mut p = p.clone();
                p.tools[0].feed_mm_min = None;
                p
            },
            &op,
        )
        .unwrap_err();
        assert!(matches!(err, CamError::MissingFeed { .. }), "{err:?}");
    }

    // ---- Raster strategy (design step 7, acceptance 21) ----

    /// The rect's tool-centre rect.
    fn tool_centre() -> Rect {
        inset(wall(), RADIUS_MM)
    }

    /// The longest a stay-down link may be: one diameter plus a step-over.
    const LINK_MAX_MM: f64 = DIAMETER_MM + STEPOVER_MM;

    /// A raster Pocket at `angle_deg` with `wall_pass`, otherwise as
    /// [`pocket_op`].
    fn raster_op(
        project: &Project,
        direction: CutDirection,
        angle_deg: f64,
        wall_pass: WallPass,
    ) -> PocketOp {
        let mut op = pocket_op(project, direction);
        op.strategy = ClearanceStrategy::Raster { angle_deg };
        op.wall_pass = wall_pass;
        op
    }

    /// One XY cutting move at one Z: where it starts and ends, and its
    /// index in the toolpath.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Cut {
        index: usize,
        z_mm: f64,
        from: Point,
        to: Point,
    }

    impl Cut {
        fn length_mm(&self) -> f64 {
            self.from.distance_to(self.to)
        }

        fn unit(&self) -> dc_geometry::Vec2 {
            (self.to - self.from).normalized().unwrap()
        }

        fn mid(&self) -> Point {
            Point::new(
                (self.from.x + self.to.x) / 2.0,
                (self.from.y + self.to.y) / 2.0,
            )
        }
    }

    /// Every non-rapid move that travels in XY at a constant Z, in order.
    fn cuts(tp: &Toolpath) -> Vec<Cut> {
        let mut out = Vec::new();
        let mut pos = tp.moves[0].to;
        for (index, mv) in tp.moves.iter().enumerate() {
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            if !mv.is_rapid() && level && pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                out.push(Cut {
                    index,
                    z_mm: mv.to.z,
                    from: pos.xy(),
                    to: mv.to.xy(),
                });
            }
            pos = mv.to;
        }
        out
    }

    /// The cuts at `z_mm` longer than any stay-down link may be: the
    /// raster lines, and the wall ring's sides when there is a wall pass.
    fn long_cuts_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        cuts(tp)
            .into_iter()
            .filter(|c| (c.z_mm - z_mm).abs() <= EPS_MM && c.length_mm() > LINK_MAX_MM + RING_TOL)
            .collect()
    }

    /// True when both ends of `cut` lie on one side of `rect`.
    fn on_side(cut: &Cut, rect: Rect) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= RING_TOL;
        (near(cut.from.x, rect.min.x) && near(cut.to.x, rect.min.x))
            || (near(cut.from.x, rect.max.x) && near(cut.to.x, rect.max.x))
            || (near(cut.from.y, rect.min.y) && near(cut.to.y, rect.min.y))
            || (near(cut.from.y, rect.max.y) && near(cut.to.y, rect.max.y))
    }

    /// The raster lines of the rect fixture at `z_mm`, in cut order: the
    /// long cuts that are not sides of the wall ring.
    fn raster_lines_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        long_cuts_at(tp, z_mm)
            .into_iter()
            .filter(|c| !on_side(c, tool_centre()))
            .collect()
    }

    /// The wall ring's sides on the rect fixture at `z_mm`, in cut order.
    fn wall_sides_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        long_cuts_at(tp, z_mm)
            .into_iter()
            .filter(|c| on_side(c, tool_centre()))
            .collect()
    }

    /// The distance from `p` to the segment `a → b`.
    fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
        let ab = b - a;
        let len2 = ab.dot(ab);
        let t = if len2 <= 0.0 {
            0.0
        } else {
            ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
        };
        p.distance_to(a + ab * t)
    }

    /// Asserts consecutive values of `offsets` (sorted) are one step-over
    /// apart.
    fn assert_spaced_by_stepover(mut offsets: Vec<f64>, what: &str) {
        offsets.sort_by(f64::total_cmp);
        for pair in offsets.windows(2) {
            assert_approx_eq!(pair[1] - pair[0], STEPOVER_MM, RING_TOL);
        }
        assert!(offsets.len() >= 2, "{what}: {offsets:?}");
    }

    /// Design step 7 at 0°: every clearance line is parallel to X, the
    /// lines are one step-over apart, the first half a step-over below the
    /// top of the tool-centre rect (PureCutCNC's Y-down `minY`, #195), and
    /// each spans the rect. The tool-centre rect is 43.65 high: lines at
    /// 43.65 − 1.27 − 2.54 k for k = 0 to 16 lie above 1.27, so 17 lines.
    #[test]
    fn raster_lines_at_zero_degrees_are_horizontal_and_spaced() {
        let p = rect_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let tc = tool_centre();
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            assert_eq!(lines.len(), 17, "lines at Z {z}: {lines:?}");
            for line in &lines {
                assert_approx_eq!(line.from.y, line.to.y, TOL);
                assert_approx_eq!(line.from.x.min(line.to.x), tc.min.x, RING_TOL);
                assert_approx_eq!(line.from.x.max(line.to.x), tc.max.x, RING_TOL);
            }
            let ys: Vec<f64> = lines.iter().map(|l| l.from.y).collect();
            let highest = ys.iter().copied().fold(f64::MIN, f64::max);
            assert_approx_eq!(highest, tc.max.y - STEPOVER_MM / 2.0, RING_TOL);
            assert_spaced_by_stepover(ys, "0°");
        }
    }

    /// Design step 7 at 45°, Y-up: every line runs along (1, 1), up and to
    /// the right as drawn (PureCutCNC's −45°), the lines one step-over
    /// apart across their direction. 135° runs along (−1, 1).
    #[test]
    fn raster_lines_at_45_degrees() {
        let p = rect_project();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        for (angle_deg, along) in [
            (45.0, Point::new(half, half)),
            (135.0, Point::new(-half, half)),
        ] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            let along = along - Point::new(0.0, 0.0);
            let across = along.perp();
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert!(lines.len() > 20, "{angle_deg}° at Z {z}: {lines:?}");
                for line in &lines {
                    let d = line.unit();
                    assert_approx_eq!(d.cross(along), 0.0, 1e-6);
                }
                let offsets = lines
                    .iter()
                    .map(|l| (l.from - Point::new(0.0, 0.0)).dot(across))
                    .collect();
                assert_spaced_by_stepover(offsets, "45°");
            }
        }
    }

    /// Design step 7: consecutive lines in cut order run opposite ways.
    #[test]
    fn raster_lines_alternate_direction() {
        let p = rect_project();
        for angle_deg in [0.0, 45.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert!(lines.len() > 2);
                for pair in lines.windows(2) {
                    assert!(
                        pair[0].unit().dot(pair[1].unit()) < -1.0 + 1e-6,
                        "{angle_deg}°: {:?} then {:?}",
                        pair[0],
                        pair[1]
                    );
                }
            }
        }
    }

    /// Asserts that, at `z_mm`, everything from the first raster line to
    /// the last is cut at that Z with no rapid, and every link between
    /// lines is a straight feed move at most one diameter plus a step-over
    /// long with both ends inside the tool-centre rect.
    fn assert_lines_linked_without_retract(tp: &Toolpath, lines: &[Cut], z_mm: f64) {
        let (first, last) = (lines[0].index, lines[lines.len() - 1].index);
        for (i, mv) in tp.moves.iter().enumerate().take(last + 1).skip(first) {
            assert!(!mv.is_rapid(), "move {i} retracts between lines: {mv:?}");
            assert_approx_eq!(mv.to.z, z_mm, TOL);
        }
        let tc = tool_centre();
        for pair in lines.windows(2) {
            let links: Vec<Cut> = cuts(tp)
                .into_iter()
                .filter(|c| c.index > pair[0].index && c.index < pair[1].index)
                .collect();
            for link in links {
                assert!(link.length_mm() <= LINK_MAX_MM + EPS_MM, "{link:?}");
                assert_eq!(tp.moves[link.index].kind, MoveKind::Linear);
                assert_approx_eq!(
                    tp.moves[link.index].feed_mm_min.unwrap(),
                    FEED_MM_MIN,
                    EPS_MM
                );
                for p in [link.from, link.to] {
                    assert!(
                        p.x >= tc.min.x - RING_TOL
                            && p.x <= tc.max.x + RING_TOL
                            && p.y >= tc.min.y - RING_TOL
                            && p.y <= tc.max.y + RING_TOL,
                        "link leaves the tool-centre rect: {link:?}"
                    );
                }
            }
        }
    }

    /// Design step 7: on the rect, the link from one line to the next is a
    /// short cut inside the tool-centre region, never a retract.
    #[test]
    fn raster_link_stays_down_inside_region() {
        let p = rect_project();
        for angle_deg in [0.0, 45.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert_lines_linked_without_retract(&tp, &lines, z);
            }
        }
    }

    /// The tool centres along `tp` never come nearer the island fixture's
    /// circle than its radius plus the tool's, less the flattening: along
    /// every straight cut, and at the ends of every arc (the island ring's
    /// arcs bulge away from the island).
    fn assert_clear_of_island(tp: &Toolpath) {
        let centre = Point::new(70.0, 45.0);
        for cut in cuts(tp) {
            let d = if tp.moves[cut.index].kind == MoveKind::Linear {
                segment_distance(centre, cut.from, cut.to)
            } else {
                centre.distance_to(cut.from).min(centre.distance_to(cut.to))
            };
            assert!(
                d >= 10.0 + RADIUS_MM - FLAT_TOL,
                "{cut:?} brings the tool centre {d} mm from the island centre"
            );
        }
    }

    /// Design step 7: with the 20 mm circle island at 0°, the lines through
    /// it are split, and the move between the two halves of a line lifts
    /// to safe Z instead of cutting across the island.
    #[test]
    fn raster_retracts_across_island() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_clear_of_island(&tp);
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            let (first, last) = (lines[0].index, lines[lines.len() - 1].index);
            let retracts = tp.moves[first..=last]
                .iter()
                .filter(|m| m.is_rapid() && (m.to.z - SAFE_Z_MM).abs() <= TOL)
                .count();
            // Every line through the island's grown circle lifts once
            // between its halves; at least the lines near Y 45 do.
            assert!(retracts >= 2, "retracts at Z {z}: {retracts}");
            for pair in lines.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let same_line = (a.from.y - b.from.y).abs() <= TOL;
                if same_line {
                    let between = &tp.moves[a.index + 1..b.index];
                    assert!(
                        between
                            .iter()
                            .any(|m| m.is_rapid() && (m.to.z - SAFE_Z_MM).abs() <= TOL),
                        "no retract between {a:?} and {b:?}"
                    );
                }
            }
        }
    }

    /// The rect with a 20 mm square island at its centre, (60, 35) to
    /// (80, 55).
    fn square_island_project() -> Project {
        project(vec![rect_100x50(), square_island()], &[("i1", at_20_20())])
    }

    fn square_island() -> Polyline {
        Polyline::rect(Point::new(40.0, 15.0), 20.0, 20.0)
    }

    /// True when `q` is within the flattening tolerance of `region`'s edge.
    fn on_edge(region: &Region, q: Point) -> bool {
        std::iter::once(region.outer())
            .chain(region.holes())
            .flat_map(|c| c.polyline().segments())
            .any(|s| dc_geometry::point_segment_distance_mm(s, q) <= FLAT_TOL)
    }

    /// True when `q` is in `region` or on its edge.
    fn in_or_on(region: &Region, q: Point) -> bool {
        region.contains_point(q) || on_edge(region, q)
    }

    /// One link between two raster lines: each move with where it starts.
    type Link = Vec<(Point, Move)>;

    /// The XY path of `mv` from `from` as one segment, an arc as an arc.
    fn move_segment(from: Point, mv: &Move) -> dc_geometry::Segment {
        dc_geometry::Segment {
            start: from,
            end: mv.to.xy(),
            bulge: crate::footprint::move_bulge(from, mv),
        }
    }

    /// 41 points along `mv` from `from`, following an arc.
    fn samples_along(from: Point, mv: &Move) -> Vec<Point> {
        let segment = move_segment(from, mv);
        let length_mm = segment.length_mm();
        (0..=40)
            .map(|k| {
                segment
                    .point_at(length_mm * f64::from(k) / 40.0)
                    .map_or(from, |(p, _)| p)
            })
            .collect()
    }

    /// The length of `link` along its moves, arcs as arcs.
    fn link_length_mm(link: &Link) -> f64 {
        link.iter()
            .map(|(from, mv)| move_segment(*from, mv).length_mm())
            .sum()
    }

    /// At `z_mm`, the gaps between consecutive raster lines at
    /// `angle_deg`, in cut order: `None` for a retract to safe Z, else the
    /// moves of the stay-down between them, each checked to be at `z_mm`
    /// and at the cutting feed. Lines are the linear cuts along the raster
    /// angle longer than any link that do not run along an edge of `tc`
    /// (the wall and island rings' sides).
    fn raster_gaps_at(tp: &Toolpath, z_mm: f64, angle_deg: f64, tc: &Region) -> Vec<Option<Link>> {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        let along = dc_geometry::Vec2::new(cos, sin);
        let lines: Vec<Cut> = long_cuts_at(tp, z_mm)
            .into_iter()
            .filter(|c| {
                tp.moves[c.index].kind == MoveKind::Linear
                    && c.unit().cross(along).abs() <= 1e-6
                    && !on_edge(tc, c.mid())
            })
            .collect();
        assert!(lines.len() > 10, "Z {z_mm}: {lines:?}");
        let is_retract = |m: &Move| m.is_rapid() && (m.to.z - SAFE_Z_MM).abs() <= TOL;
        lines
            .windows(2)
            .map(|pair| {
                let between = &tp.moves[pair[0].index + 1..pair[1].index];
                if between.iter().any(is_retract) {
                    return None;
                }
                let mut from = tp.moves[pair[0].index].to.xy();
                let mut link = Vec::new();
                for mv in between {
                    assert!(!mv.is_rapid(), "{mv:?}");
                    assert_approx_eq!(mv.to.z, z_mm, TOL);
                    assert_approx_eq!(mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
                    link.push((from, *mv));
                    from = mv.to.xy();
                }
                Some(link)
            })
            .collect()
    }

    /// The retracts to safe Z at `z_mm` between the first 0° raster line
    /// and the last, on a rect fixture with an island whose profiles are
    /// `profiles`, and the edge links. Every link between two lines is
    /// checked: a retract; or a straight stay-down, linear feed moves at
    /// `z_mm` together no longer than one diameter plus a step-over,
    /// inside the tool-centre region (so clear of the island); or an edge
    /// link (#202), feed moves at `z_mm` no longer than that along their
    /// path, every point of it on the tool-centre region's edge.
    fn raster_lifts_at(tp: &Toolpath, z_mm: f64, profiles: &[Polyline]) -> (usize, Vec<Link>) {
        let region = region_of(profiles, at_20_20());
        let tc = inset_region(&region, RADIUS_MM, FLAT_TOL);
        assert_eq!(tc.len(), 1, "{tc:?}");
        let tc = &tc[0];
        let mut lifts = 0;
        let mut edges = Vec::new();
        for gap in raster_gaps_at(tp, z_mm, 0.0, tc) {
            let Some(link) = gap else {
                lifts += 1;
                continue;
            };
            assert!(link_length_mm(&link) <= LINK_MAX_MM + EPS_MM, "{link:?}");
            let straight = link.iter().all(|(from, mv)| {
                mv.kind == MoveKind::Linear
                    && samples_along(*from, mv).iter().all(|&q| in_or_on(tc, q))
            });
            if straight {
                continue;
            }
            for (from, mv) in &link {
                for q in samples_along(*from, mv) {
                    assert!(
                        on_edge(tc, q),
                        "edge link {link:?} leaves the tool-centre edge at {q:?}"
                    );
                }
            }
            edges.push(link);
        }
        (lifts, edges)
    }

    /// #150, #202: the pieces of lines split by an island are cut
    /// nearest-first (upstream `orderOpenSegmentsGreedy`), so the tool
    /// zigzags down one side of the island before it crosses to the other,
    /// as far as each link stays inside the region. In scan order every
    /// split line lifts once: 11 per pass round either island. Round the
    /// square island, straight links run along its sides and nearest-first
    /// lifts 4 times per pass. Round the circle every straight link
    /// between two line ends against it cuts into the island, so without
    /// an edge to follow any order lifts about once per split line: 11.
    /// With a wall pass the tool follows the island's tool-centre ring
    /// instead (#202), which cuts only what the island's wall ring cuts
    /// anyway: round the circle 1 lift per pass with the wall pass last
    /// and 2 with it first (the lines start from where the rings end),
    /// from 11; round the square, whose ring has round corners, 2, from 4.
    /// Without a wall pass that band is not the operation's to cut, so the
    /// counts stay 11 and 4. Every link between lines is a straight
    /// stay-down inside the region, an edge link along it, or a retract.
    #[test]
    fn raster_lifts_around_island_are_minimised() {
        let circle = Polyline::circle(Point::new(50.0, 25.0), 10.0);
        for (p, island, [none, last, first]) in [
            (island_project(), circle, [11, 1, 2]),
            (square_island_project(), square_island(), [4, 2, 2]),
        ] {
            let profiles = [rect_100x50(), island];
            for (wall_pass, want) in [
                (WallPass::None, none),
                (WallPass::Last, last),
                (WallPass::First, first),
            ] {
                let op = raster_op(&p, CutDirection::Climb, 0.0, wall_pass);
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{warnings:?}");
                for z in PASS_Z_MM {
                    let (lifts, _) = raster_lifts_at(&tp, z, &profiles);
                    assert_eq!(lifts, want, "{wall_pass:?} at Z {z}");
                }
            }
        }
    }

    /// #392: the plaque's Panel (100 × 50 with 5 mm corners and a 20 mm
    /// island, placed at (35, 52) as in the plaque) at 0° with a wall pass
    /// last and a helix entry, moved up by 1 to 50 µm, the size of the
    /// noise an SVG import leaves, plans the same toolpath moved by as
    /// much: the same moves of the same kinds in the same order, at the
    /// same Z (so the same safe Z and plunge depth), within 1e-6 mm in XY
    /// once moved back, at the same feeds. From a line end, the stay-down
    /// links one step-over up and one down are a tie, so float noise in
    /// the scan lines' Y cannot pick between them (it did at 5 µm).
    #[test]
    fn raster_plan_ignores_a_micrometre_shift_of_the_scan_lines() {
        // The plaque's panel: 100 × 50 with 5 mm corners, a 20 mm island.
        let b = (PI / 8.0).tan();
        let profiles = || {
            vec![
                Polyline::new(
                    vec![
                        Vertex::new(5.0, 0.0, 0.0),
                        Vertex::new(95.0, 0.0, b),
                        Vertex::new(100.0, 5.0, 0.0),
                        Vertex::new(100.0, 45.0, b),
                        Vertex::new(95.0, 50.0, 0.0),
                        Vertex::new(5.0, 50.0, b),
                        Vertex::new(0.0, 45.0, 0.0),
                        Vertex::new(0.0, 5.0, b),
                    ],
                    true,
                ),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ]
        };
        let plan = |shift_mm: f64| {
            let p = project(
                profiles(),
                &[("i1", Transform::translate(35.0, 52.0 + shift_mm))],
            );
            let mut op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::Last);
            op.entry = Entry::Helix {
                diameter_fraction: 0.8,
                angle_deg: 15.0,
            };
            generate(&p, &op).0
        };
        let base = plan(0.0);
        for k in 1..=20 {
            let shift_mm = f64::from(k) * 1e-6;
            let moved = plan(shift_mm);
            assert_eq!(
                moved.moves.len(),
                base.moves.len(),
                "shift {k} µm: move count"
            );
            for (i, (a, b)) in base.moves.iter().zip(&moved.moves).enumerate() {
                let same_kind = std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind);
                let back = Point::new(b.to.x, b.to.y - shift_mm);
                assert!(
                    same_kind
                        && (a.to.z - b.to.z).abs() <= 1e-9
                        && a.to.xy().distance_to(back) <= 1e-6
                        && match (a.feed_mm_min, b.feed_mm_min) {
                            (Some(f), Some(g)) => (f - g).abs() <= 1e-6 * f,
                            (None, None) => true,
                            _ => false,
                        },
                    "shift {k} µm, move {i}: {a:?} against {b:?}"
                );
            }
        }
    }

    /// The radius of the circle island's tool-centre ring: the island's
    /// 10 mm plus the tool radius.
    const ISLAND_RING_MM: f64 = 10.0 + RADIUS_MM;

    /// #202: on the 100 × 50 pocket with the 20 mm circle island at 0°,
    /// with a wall pass, the link between two line ends against the
    /// island follows the island's tool-centre ring, an arc about its
    /// centre, at pass Z and the cutting feed, instead of lifting: 1 lift
    /// per pass with the wall pass last and 2 with it first, from 11.
    /// Every point of every edge link lies on the ring, at most the
    /// flattening tolerance inside it (where a line ends on a chord of it)
    /// and never further in.
    #[test]
    fn raster_links_follow_a_round_island_edge() {
        let p = island_project();
        let centre = Point::new(70.0, 45.0);
        let profiles = [
            rect_100x50(),
            Polyline::circle(Point::new(50.0, 25.0), 10.0),
        ];
        for (wall_pass, want) in [(WallPass::Last, 1), (WallPass::First, 2)] {
            let op = raster_op(&p, CutDirection::Climb, 0.0, wall_pass);
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_clear_of_island(&tp);
            for z in PASS_Z_MM {
                let (lifts, edges) = raster_lifts_at(&tp, z, &profiles);
                assert_eq!(lifts, want, "{wall_pass:?} at Z {z}");
                assert!(edges.len() >= 10, "{wall_pass:?} at Z {z}: {edges:?}");
                for link in &edges {
                    assert!(
                        link.iter().any(|(_, mv)| matches!(
                            mv.kind,
                            MoveKind::ArcCw { centre: c } | MoveKind::ArcCcw { centre: c }
                                if c.distance_to(centre) <= EPS_MM
                        )),
                        "edge link without an arc on the ring: {link:?}"
                    );
                    for (from, mv) in link {
                        for q in samples_along(*from, mv) {
                            let r = q.distance_to(centre);
                            assert!(
                                (ISLAND_RING_MM - FLAT_TOL - EPS_MM..=ISLAND_RING_MM + EPS_MM)
                                    .contains(&r),
                                "edge link point {q:?} is {r} mm from the island centre"
                            );
                        }
                    }
                }
            }
        }
        // Without a wall pass the band along the island is not the
        // operation's to cut: no edge links, a lift per split line.
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, _) = generate(&p, &op);
        for z in PASS_Z_MM {
            let (lifts, edges) = raster_lifts_at(&tp, z, &profiles);
            assert_eq!(lifts, 11, "Z {z}");
            assert!(edges.is_empty(), "{edges:?}");
        }
    }

    /// #202, S9: no link between raster lines cuts into the circle island,
    /// at angles that meet its curve everywhere: the footprint of every
    /// stay-down between two lines overlaps the island by no more than the
    /// flattening band along it, and its tool centre never comes nearer
    /// the island centre than the ring, less the flattening.
    #[test]
    fn edge_link_never_enters_the_island() {
        let p = island_project();
        let centre = Point::new(70.0, 45.0);
        let circle = Polyline::circle(Point::new(50.0, 25.0), 10.0);
        let island = region_of(std::slice::from_ref(&circle), at_20_20());
        let tc = inset_region(
            &region_of(&[rect_100x50(), circle], at_20_20()),
            RADIUS_MM,
            FLAT_TOL,
        );
        assert_eq!(tc.len(), 1);
        let mut on_ring = 0;
        for angle_deg in [0.0, 30.0, 45.0, 90.0, 135.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{warnings:?}");
            for z in PASS_Z_MM {
                for link in raster_gaps_at(&tp, z, angle_deg, &tc[0])
                    .into_iter()
                    .flatten()
                {
                    let start = link[0].0;
                    let mut moves = vec![Move::rapid(Point3::new(start.x, start.y, z))];
                    moves.extend(link.iter().map(|(_, mv)| *mv));
                    let fp = footprint(&Toolpath { moves }, RADIUS_MM);
                    let into = intersection(&fp, std::slice::from_ref(&island), FLAT_TOL);
                    assert!(
                        area_mm2(&into) <= 2.0 * FLAT_TOL * link_length_mm(&link) + EPS_MM,
                        "{angle_deg}° at Z {z}: link cuts {} mm² of the island: {link:?}",
                        area_mm2(&into)
                    );
                    for (from, mv) in &link {
                        for q in samples_along(*from, mv) {
                            let r = q.distance_to(centre);
                            assert!(
                                r >= ISLAND_RING_MM - FLAT_TOL - EPS_MM,
                                "{angle_deg}°: link point {q:?} is {r} mm from the island centre"
                            );
                        }
                    }
                    if link.iter().any(|(_, mv)| mv.kind != MoveKind::Linear) {
                        on_ring += 1;
                    }
                }
            }
        }
        assert!(on_ring > 0, "no link followed the island's edge");
    }

    /// An L: the 100 × 50 rect less its top-right quarter, so its wall has
    /// one reflex corner, at (50, 25), (70, 45) placed.
    fn l_shape() -> Polyline {
        Polyline::new(
            [
                (0.0, 0.0),
                (100.0, 0.0),
                (100.0, 25.0),
                (50.0, 25.0),
                (50.0, 50.0),
                (0.0, 50.0),
            ]
            .into_iter()
            .map(|(x, y)| Vertex::line(x, y))
            .collect(),
            true,
        )
    }

    /// The 100 × 50 rect with a half-disc bay of radius 15 pushed into it
    /// from the middle of its top wall, centre (50, 50), (70, 70) placed:
    /// a wall that curves inward.
    fn bay_shape() -> Polyline {
        Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(100.0, 0.0),
                Vertex::line(100.0, 50.0),
                Vertex::new(65.0, 50.0, -1.0),
                Vertex::line(35.0, 50.0),
                Vertex::line(0.0, 50.0),
            ],
            true,
        )
    }

    /// The concave-wall fixtures of #218: the outline, the centre of the
    /// tool-centre arc a link along the wall follows (placed), that arc's
    /// radius, and the lifts per pass at 0° without a wall pass and with
    /// one (last or first).
    fn concave_walls() -> [(&'static str, Polyline, Point, f64, [usize; 2]); 2] {
        [
            ("L", l_shape(), Point::new(70.0, 45.0), RADIUS_MM, [1, 0]),
            (
                "bay",
                bay_shape(),
                Point::new(70.0, 70.0),
                15.0 + RADIUS_MM,
                [6, 1],
            ),
        ]
    }

    /// #218: where the pocket wall curves inward, the tool-centre region's
    /// outer ring is an arc bulging into it (radius `r` round the L's
    /// reflex corner, 15 + `r` round the bay), and a straight link between
    /// two line ends against it leaves the region. With a wall pass the
    /// link follows that ring, as round an island (#202), at pass Z and the
    /// cutting feed: on the L 0 lifts per pass from 1, on the bay 1 from 6.
    /// Every point of every edge link lies on the arc, at most the
    /// flattening tolerance off it. Without a wall pass the counts stay.
    #[test]
    fn raster_links_follow_a_concave_wall() {
        for (name, outline, centre, radius_mm, [none, with_wall]) in concave_walls() {
            let p = project(vec![outline.clone()], &[("i1", at_20_20())]);
            let profiles = std::slice::from_ref(&outline);
            for (wall_pass, want) in [
                (WallPass::None, none),
                (WallPass::Last, with_wall),
                (WallPass::First, with_wall),
            ] {
                let op = raster_op(&p, CutDirection::Climb, 0.0, wall_pass);
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{name}: {warnings:?}");
                for z in PASS_Z_MM {
                    let (lifts, edges) = raster_lifts_at(&tp, z, profiles);
                    assert_eq!(lifts, want, "{name}, {wall_pass:?} at Z {z}");
                    if wall_pass == WallPass::None {
                        assert!(edges.is_empty(), "{name}: {edges:?}");
                        continue;
                    }
                    assert!(
                        edges.len() >= none - with_wall,
                        "{name} at Z {z}: {edges:?}"
                    );
                    for link in &edges {
                        for (from, mv) in link {
                            for q in samples_along(*from, mv) {
                                let r = q.distance_to(centre);
                                assert!(
                                    (radius_mm - FLAT_TOL - EPS_MM..=radius_mm + FLAT_TOL + EPS_MM)
                                        .contains(&r),
                                    "{name}: edge link point {q:?} is {r} mm from {centre:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// #218, S9: no link between raster lines on the concave-wall
    /// fixtures cuts into the wall, at several angles: the footprint of
    /// every stay-down between two lines leaves the pocket by no more than
    /// the flattening band along it, and its tool centre stays in the
    /// tool-centre region or on its edge.
    #[test]
    fn edge_link_never_leaves_the_wall() {
        for (name, outline, ..) in concave_walls() {
            let p = project(vec![outline.clone()], &[("i1", at_20_20())]);
            let pocket = region_of(std::slice::from_ref(&outline), at_20_20());
            let tc = inset_region(&pocket, RADIUS_MM, FLAT_TOL);
            assert_eq!(tc.len(), 1, "{name}");
            let mut along = 0;
            for angle_deg in [0.0, 30.0, 45.0, 90.0, 135.0] {
                let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{name}: {warnings:?}");
                for z in PASS_Z_MM {
                    for link in raster_gaps_at(&tp, z, angle_deg, &tc[0])
                        .into_iter()
                        .flatten()
                    {
                        let start = link[0].0;
                        let mut moves = vec![Move::rapid(Point3::new(start.x, start.y, z))];
                        moves.extend(link.iter().map(|(_, mv)| *mv));
                        let fp = footprint(&Toolpath { moves }, RADIUS_MM);
                        let out = difference(&fp, std::slice::from_ref(&pocket), FLAT_TOL);
                        assert!(
                            area_mm2(&out) <= 2.0 * FLAT_TOL * link_length_mm(&link) + EPS_MM,
                            "{name}, {angle_deg}° at Z {z}: link cuts {} mm² of the wall: {link:?}",
                            area_mm2(&out)
                        );
                        for (from, mv) in &link {
                            for q in samples_along(*from, mv) {
                                assert!(
                                    in_or_on(&tc[0], q),
                                    "{name}, {angle_deg}°: link point {q:?} leaves the region"
                                );
                            }
                        }
                        if link.len() > 1 || link.iter().any(|(_, mv)| mv.kind != MoveKind::Linear)
                        {
                            along += 1;
                        }
                    }
                }
            }
            assert!(along > 0, "{name}: no link followed the wall");
        }
    }

    /// Design step 7: a line through the island's grown circle (radius
    /// 13.175 about (70, 45)) is two segments, ending and starting on that
    /// circle; a line clear of it is one segment across the rect.
    #[test]
    fn raster_segments_split_around_island() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, _) = generate(&p, &op);
        let grown = 10.0 + RADIUS_MM;
        let tc = tool_centre();
        let mut split = 0;
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            let mut ys: Vec<f64> = lines.iter().map(|l| l.from.y).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup_by(|a, b| (*a - *b).abs() <= TOL);
            assert_eq!(ys.len(), 17, "{ys:?}");
            for y in ys {
                let mut on_line: Vec<(f64, f64)> = lines
                    .iter()
                    .filter(|l| (l.from.y - y).abs() <= TOL)
                    .map(|l| (l.from.x.min(l.to.x), l.from.x.max(l.to.x)))
                    .collect();
                on_line.sort_by(|a, b| a.0.total_cmp(&b.0));
                let dy = (y - 45.0).abs();
                if dy < grown - RING_TOL {
                    assert_eq!(on_line.len(), 2, "Y {y}: {on_line:?}");
                    assert_approx_eq!(on_line[0].0, tc.min.x, RING_TOL);
                    assert_approx_eq!(on_line[1].1, tc.max.x, RING_TOL);
                    // The inner ends lie on the grown circle, flattened:
                    // on a chord at most the tolerance inside it.
                    for x in [on_line[0].1, on_line[1].0] {
                        let r = Point::new(x, y).distance_to(Point::new(70.0, 45.0));
                        assert!(
                            r >= grown - FLAT_TOL - EPS_MM && r <= grown + EPS_MM,
                            "Y {y}: end at X {x} is {r} mm from the island centre"
                        );
                    }
                    assert!(on_line[0].1 < 70.0 && on_line[1].0 > 70.0);
                    split += 1;
                } else {
                    assert_eq!(on_line.len(), 1, "Y {y}: {on_line:?}");
                }
            }
        }
        assert!(split >= 2 * PASS_Z_MM.len(), "{split}");
    }

    /// Design step 8: raster lines are the same for climb and conventional;
    /// the wall ring runs counter-clockwise for climb and clockwise for
    /// conventional (twice its signed area, from its four sides).
    #[test]
    fn raster_ignores_direction_wall_pass_honours_it() {
        let p = rect_project();
        let climb = generate(
            &p,
            &raster_op(&p, CutDirection::Climb, 30.0, WallPass::Last),
        )
        .0;
        let conventional = generate(
            &p,
            &raster_op(&p, CutDirection::Conventional, 30.0, WallPass::Last),
        )
        .0;
        for z in PASS_Z_MM {
            let ends = |tp: &Toolpath| -> Vec<(Point, Point)> {
                raster_lines_at(tp, z)
                    .iter()
                    .map(|l| (l.from, l.to))
                    .collect()
            };
            let lines = ends(&climb);
            assert!(lines.len() > 10, "{lines:?}");
            assert_eq!(lines, ends(&conventional), "lines at Z {z}");
            let twice_area = |tp: &Toolpath| -> f64 {
                let sides = wall_sides_at(tp, z);
                assert_eq!(sides.len(), 4, "{sides:?}");
                sides
                    .iter()
                    .map(|s| (s.from - Point::new(0.0, 0.0)).cross(s.to - Point::new(0.0, 0.0)))
                    .sum()
            };
            assert!(twice_area(&climb) > 0.0, "climb wall is CCW");
            assert!(twice_area(&conventional) < 0.0, "conventional wall is CW");
        }
    }

    /// S9 on the raster: the footprint stays inside the rect and out of
    /// the island, at an angle that meets the island's curve everywhere.
    #[test]
    fn raster_footprint_stays_inside_region() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 45.0, WallPass::Last);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_clear_of_island(&tp);
        let fp = footprint(&tp, RADIUS_MM);
        let region = region_of(
            &[
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ],
            at_20_20(),
        );
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        let boundary_mm = region.outer().polyline().length_mm() + TAU * 10.0;
        assert!(
            area_mm2(&outside) <= 2.0 * FLAT_TOL * boundary_mm,
            "footprint outside the material: {} mm²",
            area_mm2(&outside)
        );
    }

    /// Acceptance test 21, as the offset coverage test: at every pass the
    /// footprints cover the rect inset by the tool radius within 0.05 mm.
    #[test]
    fn raster_coverage_reaches_inset_region() {
        let p = rect_project();
        let outer = at_20_20().apply_polyline(&rect_100x50()).unwrap();
        // Raster lines `s` apart each clear `r` either side, and
        // `s ≤ 2r` at every allowed step-over, so no ridge is left between
        // them (#258).
        for fraction in STEP_OVERS {
            for angle_deg in [0.0, 45.0, 100.0] {
                let mut op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
                op.step_over_fraction = Some(fraction);
                let (tp, _) = generate(&p, &op);
                assert_covers_at_every_pass(&tp, &outer, &format!("{fraction} at {angle_deg}°"));
            }
        }
    }

    /// Acceptance test 21: the rect rastered at 0° has every clearance
    /// line parallel to X, 2.54 mm apart; at 45° every line is at 45°.
    /// Consecutive lines alternate and are joined by a cut inside the
    /// region, not a retract. The wall pass is last by default and first
    /// with `wall_pass: first`.
    #[test]
    fn raster_acceptance_21() {
        let p = rect_project();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::Last);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            let lines = raster_lines_at(&tp, z);
            assert!(lines.iter().all(|l| (l.from.y - l.to.y).abs() <= TOL));
            assert_spaced_by_stepover(lines.iter().map(|l| l.from.y).collect(), "0°");
        }
        for (wall_pass, wall_last) in [(WallPass::Last, true), (WallPass::First, false)] {
            for angle_deg in [0.0, 45.0] {
                let op = raster_op(&p, CutDirection::Climb, angle_deg, wall_pass);
                let (tp, _) = generate(&p, &op);
                for z in PASS_Z_MM {
                    let lines = raster_lines_at(&tp, z);
                    assert!(lines.len() > 10, "{lines:?}");
                    if angle_deg > 0.0 {
                        for line in &lines {
                            let d = line.unit();
                            assert_approx_eq!(d.x.abs(), half, 1e-6);
                            assert_approx_eq!(d.y.abs(), half, 1e-6);
                            assert!(d.x * d.y > 0.0, "{line:?} is at 45°");
                        }
                    }
                    for pair in lines.windows(2) {
                        assert!(pair[0].unit().dot(pair[1].unit()) < 0.0);
                    }
                    assert_lines_linked_without_retract(&tp, &lines, z);
                    let sides = wall_sides_at(&tp, z);
                    assert_eq!(sides.len(), 4, "{wall_pass:?} {angle_deg}°: {sides:?}");
                    let (first_line, last_line) = (lines[0].index, lines[lines.len() - 1].index);
                    if wall_last {
                        assert!(sides.iter().all(|s| s.index > last_line), "{wall_pass:?}");
                    } else {
                        assert!(sides.iter().all(|s| s.index < first_line), "{wall_pass:?}");
                    }
                    // Lines lie strictly inside the tool-centre rect.
                    let tc = tool_centre();
                    for line in &lines {
                        let m = line.mid();
                        assert!(
                            m.x > tc.min.x && m.x < tc.max.x && m.y > tc.min.y && m.y < tc.max.y
                        );
                    }
                }
            }
        }
    }

    /// The 100 × 50 rect with 5 mm round corners, the plaque's panel, and
    /// the 20 mm circle island at its centre, placed at (20, 20). The wall
    /// ring's tool-centre corners are arcs of 1.825 mm.
    fn rounded_island_project() -> Project {
        let b = (PI / 8.0).tan();
        let rounded = Polyline::new(
            vec![
                Vertex::new(5.0, 0.0, 0.0),
                Vertex::new(95.0, 0.0, b),
                Vertex::new(100.0, 5.0, 0.0),
                Vertex::new(100.0, 45.0, b),
                Vertex::new(95.0, 50.0, 0.0),
                Vertex::new(5.0, 50.0, b),
                Vertex::new(0.0, 45.0, 0.0),
                Vertex::new(0.0, 5.0, b),
            ],
            true,
        );
        project(
            vec![rounded, Polyline::circle(Point::new(50.0, 25.0), 10.0)],
            &[("i1", at_20_20())],
        )
    }

    /// #213: a raster Pocket with an island and a wall pass places the
    /// wall ring's helix or ramp, as the island ring's. The wall ring
    /// starts at its point nearest the last line's end, here on a round
    /// corner of the tool-centre region, which the entry domain holds as
    /// chords inside the arc: the start lay just outside the domain, no
    /// placement reached it, and the ring warned `entry_fallback` and
    /// plunged.
    #[test]
    fn raster_wall_ring_gets_its_entry() {
        let p = rounded_island_project();
        for entry in [
            helix_80(),
            Entry::Helix {
                diameter_fraction: 0.8,
                angle_deg: 15.0,
            },
            Entry::Ramp { angle_deg: 15.0 },
        ] {
            for wall_pass in [WallPass::Last, WallPass::First] {
                let mut op = raster_op(&p, CutDirection::Climb, 0.0, wall_pass);
                op.entry = entry.clone();
                let (_, warnings) = generate(&p, &op);
                assert!(
                    entry_warnings(&warnings).is_empty(),
                    "{entry:?}, {wall_pass:?}: {warnings:?}"
                );
            }
        }
    }

    /// #213: the step from the last raster line's end to the wall ring's
    /// start, inside material that line has just cut, is a stay-down link,
    /// not a lift: the ring's start on the corner arc lay a hair outside
    /// the cleared area as the sequencer flattens it.
    #[test]
    fn raster_wall_ring_follows_the_last_line_without_a_lift() {
        let p = rounded_island_project();
        let mut op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::Last);
        op.entry = helix_80();
        let (tp, _) = generate(&p, &op);
        for z in PASS_Z_MM {
            let sides = wall_sides_at(&tp, z);
            let first_side = sides.first().expect("the wall ring is cut").index;
            let last_line = raster_lines_at(&tp, z)
                .into_iter()
                .rfind(|c| c.index < first_side)
                .expect("lines come before the wall ring")
                .index;
            let between = &tp.moves[last_line + 1..first_side];
            assert!(
                between.iter().all(|m| !m.is_rapid()),
                "Z {z}: the tool lifts between the last line and the wall ring: {between:?}"
            );
        }
    }

    /// The raster angle is degrees from +X in [0, 180); outside it the
    /// generator refuses rather than guessing, as validation does.
    #[test]
    fn raster_angle_outside_range_is_error() {
        let p = rect_project();
        for angle_deg in [-1.0, 180.0, 270.0, f64::NAN] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
            let err = generate_pocket(&p, &op).unwrap_err();
            assert!(
                matches!(err, CamError::InvalidRasterAngle { ref op, .. } if op.as_str() == "o1"),
                "{angle_deg}: {err:?}"
            );
            assert!(
                err.to_string().contains("raster angle"),
                "{angle_deg}: {err}"
            );
        }
    }

    /// #232 (found by the dc-gcode property test): a raster Pocket of a
    /// 29.83 × 45.09 mm rectangle round a 15.65 × 14.49 mm rectangular
    /// island, with a 1.645 mm tool at 13.6 % step-over and 24.6°. A
    /// 0.34 mm stay-down link joined two raster ends on the round
    /// tool-centre boundary about an island corner, and passed 0.7985 mm
    /// from the corner, 0.024 mm inside the cutter's radius. No cutting
    /// move may bring the tool centre nearer the island than the radius,
    /// less the flatten tolerance the tool-centre outline is drawn to.
    #[test]
    fn raster_link_never_cuts_an_island_corner() {
        let placement = Transform::translate(120.0, 80.0);
        let island = Polyline::rect(
            Point::new(-7.825_474_403_184_983, -7.242_710_582_657_877),
            15.650_948_806_369_966,
            14.485_421_165_315_754,
        );
        let mut p = project(
            vec![
                Polyline::rect(
                    Point::new(-14.917_410_529_058_088, -22.545_952_942_869_242),
                    29.834_821_058_116_176,
                    45.091_905_885_738_484,
                ),
                island.clone(),
            ],
            &[("i1", placement)],
        );
        let diameter_mm = 1.644_546_628_706_222_7;
        p.tools[0].diameter_mm = diameter_mm;
        let op = PocketOp {
            depth_mm: Some(1.0),
            step_over_fraction: Some(0.135_681_152_562_577_37),
            ..raster_op(
                &p,
                CutDirection::Climb,
                24.628_005_484_201_154,
                WallPass::Last,
            )
        };
        let (tp, _) = generate(&p, &op);
        let island = placement.apply_polyline(&island).unwrap();
        let clear_mm = diameter_mm / 2.0 - FLAT_TOL;
        let mut at = tp.moves[0].to.xy();
        for (i, mv) in tp.moves.iter().enumerate() {
            if !mv.is_rapid() {
                let path = move_segment(at, mv);
                for edge in island.segments() {
                    let d = dc_geometry::segment_distance_mm(path, edge);
                    assert!(
                        d >= clear_mm,
                        "move {i} brings the tool centre {d} mm from the island: {mv:?}"
                    );
                }
            }
            at = mv.to.xy();
        }
    }
}
