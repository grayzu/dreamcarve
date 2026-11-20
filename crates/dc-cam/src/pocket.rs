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
use crate::context::{self, OpContext, check_diameter, check_safe_z, rates_of, tool_of};
use crate::entry::{self, CutSide, EntryDomain, EntryPlan, EntryTool, EntryWarnings};
use crate::error::{CamError, ShapeRef, at};
use crate::footprint::footprint_at;
use crate::passes::pass_z_mm;
use crate::profile::{contour_contains, crossing_error, region_outer, resolve_targets};
use crate::relief::{self, Site};
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
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
/// Each closed target profile is placed and nested: a region is a pocket,
/// its holes are islands. The pocket, inset by the tool radius plus
/// `op.stock_to_leave_mm`, is cleared in depth passes by offset rings or
/// raster lines per `op.strategy`, with the wall rings per `op.wall_pass`,
/// each reached by the operation's entry; [`sequence`] orders and links
/// them. With `op.rest_of` it cuts only what the referenced Pocket's tool
/// left, read from the project and never from a toolpath (S14). With
/// `op.corner_relief` it notches the corners the round tool cannot reach.
/// What cannot be cut as asked is skipped with a [`Warning`]. The rules
/// are in `docs/specs/phase-2/design.md`, "Algorithms → Pocket", "Entry"
/// and "Corner relief".
///
/// # Errors
///
/// No toolpath is returned when:
/// - an id does not resolve, a target cannot be placed, or targets cross;
/// - the cutter is not a flat end mill;
/// - a rate, the step-down, the step-over, the stock thickness or the
///   depth is unset, or the depth passes the stock bottom plus allowance;
/// - the stock to leave is negative, the raster angle is outside
///   [0, 180), or the entry is out of range;
/// - `op.rest_of` is not an enabled Pocket with the same targets listed
///   before `op` ([`CamError::RestOfInvalid`]);
/// - the cutter would leave the stock ([`CamError::OffStock`]) or its
///   footprint, descents included, the pocket's material (S9,
///   [`CamError::OffMaterial`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_pocket(
    project: &Project,
    op: &PocketOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    pocket(project, op).map_err(|e| e.for_op(&op.id))
}

/// [`generate_pocket`], before a defect it finds ([`CamError::Internal`])
/// names `op`.
fn pocket(project: &Project, op: &PocketOp) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let mut warnings = Vec::new();
    let setup = Setup::of(project, op, &mut warnings)?;
    let shapes = resolve_targets(project, &op.id, &op.targets, &mut warnings)?;
    let contours: Vec<Contour> = shapes.iter().map(|(_, c)| c.clone()).collect();
    let (regions, nest_warnings) = nest(contours).map_err(|e| crossing_error(e, &shapes))?;
    for warning in nest_warnings {
        let GeometryWarning::DuplicateContourDropped {
            index,
            duplicate_of,
        } = warning;
        // `nest` numbers the contours as it was given them, one a shape.
        let (dropped, _) = at(&shapes, index, "a duplicate contour is of no shape")?;
        let (kept, _) = at(&shapes, duplicate_of, "a duplicated contour is of no shape")?;
        warnings.push(Warning::DuplicateShapeDropped {
            shape: dropped.clone(),
            duplicate_of: kept.clone(),
        });
    }
    let toolpath = setup.cut(op, &regions, &shapes, &mut warnings)?;
    Ok((toolpath, warnings))
}

/// The Pocket toolpath of `op` on `regions`, already resolved, rather than
/// on its target instances (`docs/specs/phase-3/design.md`, "Clearing
/// passes", step 3): the same checks, rings, raster lines, entries,
/// sequencing and S9 check as [`generate_pocket`], so a carve's clearing
/// pass is cut by exactly the code a user's Pocket is. `op.targets` is not
/// read. `shapes` names the contours `regions` were nested from, by the
/// regions' indices (`Region::outer_index`, `Region::hole_indices`); a
/// hole with no entry there is an island no shape names, and is not
/// warned about. With `reached_by` the operation is rest machining on
/// those references, as a `rest_of` Pocket is on its one (#406: a carve's
/// earlier clearing passes), read from their tools and never from a
/// toolpath (S14); `op.rest_of` is then `None`.
///
/// # Errors
///
/// As [`generate_pocket`], but for resolving and nesting targets.
pub(crate) fn pocket_regions(
    project: &Project,
    op: &PocketOp,
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
    reached_by: &[rest::Reference],
    warnings: &mut Vec<Warning>,
) -> Result<Toolpath, CamError> {
    let mut setup = Setup::of(project, op, warnings)?;
    setup.references.extend_from_slice(reached_by);
    setup
        .cut(op, regions, shapes, warnings)
        .map_err(|e| e.for_op(&op.id))
}

/// What every region of a Pocket is cut with, checked before its targets
/// are resolved: the tool, its rates, the pass levels, the step-over, the
/// relief plan, the entry tool and the references of a rest operation.
struct Setup<'p> {
    ctx: OpContext<'p>,
    pass_z: Vec<f64>,
    step_over_fraction: f64,
    stepover_mm: f64,
    radius_mm: f64,
    tolerance_mm: f64,
    relief: Option<relief::Relief>,
    entry_tool: EntryTool,
    /// What the operation rests on: its `rest_of`, or a carve's earlier
    /// clearing passes. Empty for a plain Pocket.
    references: Vec<rest::Reference>,
}

impl<'p> Setup<'p> {
    /// Checks `op` in `project` and plans its relief, reporting problems
    /// in the order [`generate_pocket`] always has.
    fn of(
        project: &'p Project,
        op: &'p PocketOp,
        warnings: &mut Vec<Warning>,
    ) -> Result<Self, CamError> {
        let tool = tool_of(project, &op.id, &op.tool)?;
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
        check_diameter(tool)?;
        check_safe_z(&op.id, op.safe_z_mm)?;
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
        let step_down_mm = context::step_down_mm(&op.id, op.step_down_mm, tool)?;
        let thickness_mm = context::thickness_mm(project)?;
        let depth_mm = context::depth_mm(&op.id, op.depth_mm)?;
        let pass_z = pass_z_mm(
            &op.id,
            thickness_mm,
            depth_mm,
            step_down_mm,
            op.through_cut_allowance_mm,
        )?;
        let ctx = OpContext::new(project, &op.id, tool, rates, thickness_mm, op.safe_z_mm);
        let safe_z_mm = ctx.safe_z_mm;
        let step_over_fraction = op
            .step_over_fraction
            .or(tool.step_over_fraction)
            .ok_or_else(|| CamError::MissingStepOver {
                op: op.id.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
            })?;
        if !(step_over_fraction.is_finite()
            && step_over_fraction > 0.0
            && step_over_fraction <= 1.0)
        {
            return Err(CamError::InvalidStepOver {
                op: op.id.clone(),
                step_over_fraction,
            });
        }
        let tolerance_mm = project.flatten_tolerance_mm;
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
        .plan(warnings)?;
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
        Ok(Self {
            ctx,
            pass_z,
            step_over_fraction,
            stepover_mm: tool.diameter_mm * step_over_fraction,
            radius_mm: tool.diameter_mm / 2.0,
            tolerance_mm,
            relief,
            entry_tool,
            references: reference.into_iter().collect(),
        })
    }

    /// The toolpath of `op` on `regions`, nested from the contours of
    /// `shapes`: design steps 2 to 12.
    #[expect(
        clippy::too_many_lines,
        reason = "steps 2 to 12 of the Pocket generator, kept in one place"
    )]
    fn cut(
        &self,
        op: &PocketOp,
        regions: &[Region],
        shapes: &[(ShapeRef, Contour)],
        warnings: &mut Vec<Warning>,
    ) -> Result<Toolpath, CamError> {
        let Self {
            ctx,
            ref pass_z,
            step_over_fraction,
            stepover_mm,
            radius_mm,
            tolerance_mm,
            ref relief,
            entry_tool,
            ref references,
        } = *self;
        let OpContext {
            tool,
            rates,
            thickness_mm,
            safe_z_mm,
            ..
        } = ctx;
        let invalid_step_over = || CamError::InvalidStepOver {
            op: op.id.clone(),
            step_over_fraction,
        };
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
        for region in regions {
            let shape = region_outer(region, shapes)?;
            warnings.extend(island_gap_warnings(op, tool, region, shapes, shape));
            // The material this region's cuts may reach.
            let region_allowed = if op.stock_to_leave_mm > 0.0 {
                inset_region(region, op.stock_to_leave_mm, tolerance_mm)
            } else {
                vec![region.clone()]
            };
            allowed.extend(region_allowed.iter().cloned());
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
            // touch what its references left.
            let (tool_centre, left) = if references.is_empty() {
                (tool_centre, None)
            } else {
                rest_had_room = true;
                let left = rest::left_by(region, op.stock_to_leave_mm, references, tolerance_mm);
                let centre = rest::rest_centre(&tool_centre, &left, radius_mm, tolerance_mm);
                if centre.is_empty() {
                    continue;
                }
                rest_found = true;
                (centre, Some(left))
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
                sites.push(relief::pocket_site(&region_allowed, &tool_centre, shape));
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
                    let paths =
                        raster::region_paths(&root, root_index, &settings, &keep, &mut anchor)?
                            .ok_or_else(invalid_step_over)?;
                    // Raster lines reach the tool-centre region's edge at every
                    // wall pass, so the whole root is cleared each pass.
                    let domain = (op.entry != Entry::Plunge).then(|| {
                        EntryDomain::new(
                            std::slice::from_ref(&root),
                            tool.diameter_mm,
                            tolerance_mm,
                        )
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
                let domain = if op.entry == Entry::Plunge {
                    None
                } else {
                    let regions = entry_regions(&tree, wall)?;
                    Some(EntryDomain::new(&regions, tool.diameter_mm, tolerance_mm))
                };
                for ring in tree.walk_keeping(wall, &mut anchor, &keep)? {
                    let node = at(&tree.nodes, ring.node, "a ring is of no node of its tree")?;
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
        // The surface a pass starts above: the stock top, then the floor
        // of the pass before.
        let mut above_mm = thickness_mm;
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            // #214: a ramp or helix starts the entry clearance above the stock
            // top, and on a later pass above the floor the pass before cleared
            // (the whole tool-centre region, every pass), not at safe Z.
            let descent_from_z_mm = entry::descent_start_z_mm(above_mm, safe_z_mm);
            above_mm = z_mm;
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
                    entry_z_mm: z_mm,
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
        let (inline, fallback) = match relief {
            Some(relief) => relief.inline_pocket(
                &sites,
                pass_z,
                op.stock_to_leave_mm <= 0.0,
                &mut blocks,
                warnings,
            ),
            None => (Vec::new(), Vec::new()),
        };
        let containment = nested_pockets(regions, shapes)?;
        let input = SequenceInput {
            order: op.machining_order,
            containment: &containment,
            stepover_mm,
            ..SequenceInput::new(&ctx, blocks)
        };
        let mut toolpath = sequence(&input)?;
        // Design step 10: the separate relief pass after all the passes, for
        // the corners not cut inline; S9 allows each notch the lobe its corner
        // may lose.
        if let Some(relief) = relief {
            let mut cut = inline;
            cut.extend(relief.cut(fallback, &mut toolpath, warnings)?);
            let lobes: Vec<Region> = cut
                .iter()
                .flat_map(|c| relief::lobe(c, radius_mm, tolerance_mm))
                .collect();
            if !lobes.is_empty() {
                allowed = union(&allowed, &lobes, tolerance_mm);
            }
        }

        // #62: the cutter must stay on the stock.
        ctx.check_on_stock(&toolpath, radius_mm)?;
        check_in_material(
            op,
            &toolpath,
            radius_mm,
            tolerance_mm,
            regions,
            &allowed,
            &centre_regions,
            shapes,
        )?;
        Ok(toolpath)
    }
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
    let tool = tool_of(project, &rough.id, &rough.tool)?;
    check_diameter(tool)?;
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
/// A tree has its root, and a node's children are nodes of the tree:
/// [`CamError::Internal`] otherwise, where the entries would get a
/// smaller domain or none, and plunge.
fn entry_regions(tree: &RingTree, wall: WallRings) -> Result<Vec<Region>, CamError> {
    let root = at(&tree.nodes, 0, "a ring tree has no root")?;
    match wall {
        WallRings::Omitted => {
            let mut regions = Vec::with_capacity(root.children.len());
            for &child in &root.children {
                let node = at(
                    &tree.nodes,
                    child,
                    "a ring tree's root has a child it lacks",
                )?;
                if !node.residual {
                    regions.push(node.region.clone());
                }
            }
            Ok(regions)
        }
        WallRings::First | WallRings::Last => Ok(vec![root.region.clone()]),
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
    // An island no shape names (a clearing pass's, `pocket_regions`) is
    // not warned about.
    let holes: Vec<(&Contour, &ShapeRef)> = region
        .holes()
        .iter()
        .zip(region.hole_indices())
        .filter_map(|(hole, &i)| shapes.get(i).map(|(s, _)| (hole, s)))
        .collect();
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
pub(crate) fn ring_path(ring: Ring, direction: CutDirection) -> Polyline {
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
///
/// Two pockets of one shape are never paired (#482): `sequence` cuts a
/// shape's blocks at a pass as one unit, in generator order, and would
/// read the pair as the shape enclosing itself, a containment cycle. A
/// user's Pocket names every region by its own outline, so this happens
/// only for a clearing pass, whose floor pieces of one carved region
/// share its shape (`vcarve::clearing` puts a piece after the
/// piece round its island). Every region's outline is one of `shapes`
/// and has a vertex: [`CamError::Internal`] otherwise, where a pair left
/// out would let the pocket inside be cut first.
fn nested_pockets(
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
) -> Result<Vec<(ShapeRef, ShapeRef)>, CamError> {
    let mut pairs = Vec::new();
    for region in regions {
        // A region at depth 0 or 1 is inside no island.
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let inner = region_outer(region, shapes)?;
        let probe = region
            .outer()
            .polyline()
            .vertices()
            .first()
            .ok_or_else(|| CamError::internal("a region's outline has no vertex"))?
            .point();
        let around = regions
            .iter()
            .filter(|r| r.depth() == parent_depth)
            .find(|r| r.holes().iter().any(|h| contour_contains(h, probe)));
        // No island of that depth holds it: nothing to cut it after.
        let Some(around) = around else {
            continue;
        };
        let outer = region_outer(around, shapes)?;
        if outer != inner {
            pairs.push((outer.clone(), inner.clone()));
        }
    }
    Ok(pairs)
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
        // A cut outside the material is refused whether or not a pocket
        // can be named for it; with none to name, the defect is said so.
        let named = regions
            .iter()
            .find(|r| probe.is_some_and(|p| contour_contains(r.outer(), p)))
            .or_else(|| regions.first())
            .ok_or_else(|| {
                CamError::internal("a Pocket with no region cuts outside its material")
            })?;
        Err(CamError::OffMaterial {
            op: op.id.clone(),
            shape: region_outer(named, shapes)?.clone(),
            area_mm2: area,
        })
    };
    if let Some(i) = entry::first_descent_outside(toolpath, centre_regions, tolerance_mm) {
        // The one stray move's own footprint, to measure and name. A
        // descent starts where the move before it ends.
        let mv = at(
            &toolpath.moves,
            i,
            "the descent found outside the material is not in the toolpath",
        )?;
        let before = i
            .checked_sub(1)
            .ok_or_else(|| CamError::internal("a toolpath starts with a descent, from nowhere"))?;
        let from = at(
            &toolpath.moves,
            before,
            "the descent found outside the material has no move before it",
        )?;
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
mod tests;
