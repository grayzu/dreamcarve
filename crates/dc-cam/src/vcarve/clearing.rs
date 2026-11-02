// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Clearing passes (`docs/specs/phase-3/design.md`, "Algorithms → Clearing
//! passes (dc-cam::vcarve::clearing)" and "V-Carve generator", step 7;
//! functional capability 40, acceptance 51). New in DreamCarve: PureCutCNC
//! has no clearing tools; VCarve's "Use Clearance Tools" and Corbin's
//! clearance toolpaths are what the behaviour follows.
//!
//! A carve with a flat depth and clearing passes cuts its floor with each
//! pass's flat end mill first, one [`ToolRun`] per pass in list order, and
//! then with its V-bit. Pass `i` is a Pocket of the carve's floor regions
//! (each region inset by `F · tan(α/2)`, [`Floor::regions`]) to the start
//! depth plus `F`, cut by the Pocket code itself
//! ([`pocket::pocket_regions`]): the pass's strategy, direction and entry;
//! step-down and step-over from its tool; wall pass last; no stock to
//! leave (the floor already stands the V-bit's inset off the outline) and
//! no corner relief. It rests on the passes before it as a `rest_of`
//! Pocket rests on its reference: from their tools' radii, never from a
//! toolpath (S14), its tool centres keep only those that reach what they
//! left ([`rest::left_by`], [`rest::rest_centre`]). The V-bit's own floor
//! is then what no pass reaches (`floor::blocks_after`), worked out the
//! same way.
//!
//! A pass that reaches nothing warns [`Warning::ClearingNoRoom`] and has
//! no run: its end mill does not fit the floor, or it is no narrower than
//! a pass before it (a disc reaches nothing a narrower disc cannot, so a
//! wider pass after a narrower one has nothing left to cut). A pass wider
//! than the one before it also warns [`Warning::ClearingNotDescending`].

use super::Carve;
use super::floor::Floor;
use crate::error::{CamError, ClearingProblem, ShapeRef};
use crate::pocket::rest::{self, Reference};
use crate::pocket::rings::inset_region;
use crate::pocket::{self};
use crate::run::{RunLabel, ToolRun};
use crate::warning::Warning;
use dc_geometry::{Contour, Region, nest};
use dc_project::{
    ClearingPass, CornerRelief, InlayOp, MachiningOrder, OperationId, PocketOp, Project, Tool,
    VCarveOp, WallPass,
};

/// What the clearing passes need of the operation that carves: a V-Carve
/// or an Inlay side (#436), whose passes are cut alike.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Passes<'a> {
    /// The carve.
    pub op: &'a OperationId,
    /// Its display name.
    pub name: &'a str,
    /// Its clearing passes, in cut order.
    pub clearing: &'a [ClearingPass],
    /// Its start plane's depth below the stock top, mm: the floor is the
    /// flat depth below it.
    pub start_depth_mm: f64,
    /// Its machining order.
    pub machining_order: MachiningOrder,
    /// How far below the stock bottom it may cut, mm.
    pub through_cut_allowance_mm: f64,
    /// Its safe Z above the stock top, mm.
    pub safe_z_mm: f64,
}

impl<'a> Passes<'a> {
    /// The passes of V-Carve `op`.
    pub(crate) fn of_v_carve(op: &'a VCarveOp) -> Self {
        Self {
            op: &op.id,
            name: &op.name,
            clearing: &op.clearing,
            start_depth_mm: op.start_depth_mm,
            machining_order: op.machining_order,
            through_cut_allowance_mm: op.through_cut_allowance_mm,
            safe_z_mm: op.safe_z_mm,
        }
    }

    /// The passes of Inlay side `op`, whose start plane is
    /// `start_depth_mm` below the stock top: 0 for the female, `depth_mm −
    /// glue_gap_mm` for the male.
    pub(crate) fn of_inlay(op: &'a InlayOp, start_depth_mm: f64) -> Self {
        Self {
            op: &op.id,
            name: &op.name,
            clearing: &op.clearing,
            start_depth_mm,
            machining_order: op.machining_order,
            through_cut_allowance_mm: op.through_cut_allowance_mm,
            safe_z_mm: op.safe_z_mm,
        }
    }
}

/// The end mills of the clearing passes `passes`, in list order, checked
/// as validation checks them (design "Clearing passes", step 1): each
/// names a tool of `project`, a flat end mill with a usable diameter, and
/// the carve has a flat depth (`has_floor`). Empty without passes.
///
/// # Errors
///
/// [`CamError::ClearingInvalid`], naming the first pass that cannot run,
/// and [`CamError::InvalidToolDiameter`].
pub(crate) fn checked_tools<'p>(
    project: &'p Project,
    passes: &Passes<'_>,
    has_floor: bool,
) -> Result<Vec<&'p Tool>, CamError> {
    let invalid = |tool: &dc_project::ToolId, reason| CamError::ClearingInvalid {
        op: passes.op.clone(),
        tool: tool.clone(),
        reason,
    };
    let mut tools = Vec::with_capacity(passes.clearing.len());
    for pass in passes.clearing {
        if !has_floor {
            return Err(invalid(&pass.tool, ClearingProblem::NoFlatDepth));
        }
        let tool = project
            .tools
            .iter()
            .find(|t| t.id == pass.tool)
            .ok_or_else(|| invalid(&pass.tool, ClearingProblem::UnknownTool))?;
        if !tool.cutter.is_flat() {
            return Err(invalid(
                &pass.tool,
                ClearingProblem::NotEndMill {
                    cutter: tool.cutter.describe(),
                },
            ));
        }
        if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
            return Err(CamError::InvalidToolDiameter {
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
                diameter_mm: tool.diameter_mm,
            });
        }
        tools.push(tool);
    }
    Ok(tools)
}

/// What a clearing pass reaches, as rest machining reads a reference: its
/// end mill's radius, with no stock to leave.
pub(crate) fn reference(radius_mm: f64) -> Reference {
    Reference {
        radius_mm,
        stock_to_leave_mm: 0.0,
    }
}

/// The clearing runs of carve `carve` (design "Clearing passes", steps 2
/// and 3), one per pass of `passes.clearing` whose end mill
/// (`tools`, from [`checked_tools`]) reaches some of the floor the passes
/// before it left, in list order: labelled [`RunLabel::Clearing`], with the
/// pass's tool. `slope` is the V-bit's `tan(α/2)`.
///
/// # Errors
///
/// Whatever the Pocket code refuses for the pass ([`pocket::pocket_regions`]):
/// a missing rate, step-down or step-over of its tool, an entry out of
/// range, a depth below the stock bottom, and S9.
pub(crate) fn runs(
    project: &Project,
    passes: &Passes<'_>,
    carve: &Carve<'_>,
    tools: &[&Tool],
    slope: f64,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<ToolRun>, CamError> {
    let Some(flat) = Floor::of(carve, slope) else {
        return Ok(Vec::new());
    };
    let tolerance_mm = project.flatten_tolerance_mm;
    // The floor regions of every carved region, each with its region's
    // shape: the same `floor_regions` call the V-bit's floor makes.
    let floors: Vec<(Region, &ShapeRef)> = carve
        .regions
        .iter()
        .zip(&carve.shapes)
        .flat_map(|(region, shape)| {
            // #448: out to an open band edge.
            flat.regions_of(carve, region, tolerance_mm)
                .into_iter()
                .map(move |f| (f, shape))
        })
        .collect();
    let mut out = Vec::new();
    let mut earlier: Vec<Reference> = Vec::new();
    let mut before: Option<&Tool> = None;
    for (pass, &tool) in passes.clearing.iter().zip(tools) {
        if let Some(previous) = before
            && tool.diameter_mm > previous.diameter_mm
        {
            warnings.push(Warning::ClearingNotDescending {
                op: passes.op.clone(),
                tool: tool.id.clone(),
                diameter_mm: tool.diameter_mm,
                earlier_tool: previous.id.clone(),
                earlier_diameter_mm: previous.diameter_mm,
            });
        }
        before = Some(tool);
        let radius_mm = tool.diameter_mm / 2.0;
        let reached: Vec<&(Region, &ShapeRef)> = floors
            .iter()
            .filter(|(f, _)| reaches(f, radius_mm, &earlier, tolerance_mm))
            .collect();
        if reached.is_empty() {
            warnings.push(Warning::ClearingNoRoom {
                op: passes.op.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
                diameter_mm: tool.diameter_mm,
            });
        } else {
            let (regions, shapes) = renest(&reached)?;
            let pocket = PocketOp {
                id: passes.op.clone(),
                name: passes.name.to_owned(),
                enabled: true,
                targets: Vec::new(),
                tool: tool.id.clone(),
                strategy: pass.strategy.clone(),
                wall_pass: WallPass::Last,
                depth_mm: Some(passes.start_depth_mm + flat_depth_of(carve)),
                step_down_mm: None,
                step_over_fraction: None,
                direction: pass.direction,
                stock_to_leave_mm: 0.0,
                rest_of: None,
                entry: pass.entry.clone(),
                corner_relief: CornerRelief::None,
                machining_order: passes.machining_order,
                through_cut_allowance_mm: passes.through_cut_allowance_mm,
                safe_z_mm: passes.safe_z_mm,
            };
            let toolpath =
                pocket::pocket_regions(project, &pocket, &regions, &shapes, &earlier, warnings)?;
            out.push(ToolRun {
                label: RunLabel::Clearing(tool.id.clone()),
                tool: tool.id.clone(),
                toolpath,
            });
        }
        earlier.push(reference(radius_mm));
    }
    Ok(out)
}

/// The carve's flat depth, mm; [`runs`] is only reached with one.
fn flat_depth_of(carve: &Carve<'_>) -> f64 {
    carve.flat_depth_mm.unwrap_or(0.0)
}

/// Whether an end mill of `radius_mm` reaches some of `floor` that the
/// passes `earlier` left: it fits the floor somewhere, and some of its tool
/// centres there touch the leftover, as the Pocket code decides it for a
/// rest operation with no stock to leave.
fn reaches(floor: &Region, radius_mm: f64, earlier: &[Reference], tolerance_mm: f64) -> bool {
    let centre = inset_region(floor, radius_mm, tolerance_mm);
    if centre.is_empty() {
        return false;
    }
    if earlier.is_empty() {
        return true;
    }
    let left = rest::left_by(floor, 0.0, earlier, tolerance_mm);
    !rest::rest_centre(&centre, &left, radius_mm, tolerance_mm).is_empty()
}

/// Regions as the Pocket code takes them, with the shapes that name their
/// contours by index.
type Resolved = (Vec<Region>, Vec<(ShapeRef, Contour)>);

/// `floors` as the Pocket code takes resolved regions: nested again from
/// their outers and holes, so a floor inside another floor's hole is a
/// pocket inside an island, each outer named by its carved region's shape.
/// The holes are islands no shape names: the floor grows them by the
/// V-bit's inset, so they are not the user's outlines.
///
/// One carved region's floor can be several pieces, one inside another's
/// island (#482: the inset closes a bay's mouth, and the bay is a piece in
/// the island the closed mouth makes). Its pieces share its shape, so
/// `sequence` cuts them at a pass as one unit, in the order given here,
/// and no containment pair orders them (`pocket::nested_pockets`): each
/// shape's pieces come at its first one, shallowest first, so a piece is
/// cut after the piece round its island as two shapes would be. Pieces of
/// different shapes keep `nest`'s order.
fn renest(floors: &[&(Region, &ShapeRef)]) -> Result<Resolved, CamError> {
    let shapes: Vec<(ShapeRef, Contour)> = floors
        .iter()
        .map(|(f, shape)| ((*shape).clone(), f.outer().clone()))
        .collect();
    let contours: Vec<Contour> = shapes
        .iter()
        .map(|(_, c)| c.clone())
        .chain(floors.iter().flat_map(|(f, _)| f.holes().iter().cloned()))
        .collect();
    let (regions, _) = nest(contours)?;
    let shape_of = |r: &Region| shapes.get(r.outer_index()).map(|(s, _)| s);
    let keys: Vec<(usize, usize)> = regions
        .iter()
        .map(|r| {
            let first = regions
                .iter()
                .position(|o| shape_of(o) == shape_of(r))
                .unwrap_or(0);
            (first, r.depth())
        })
        .collect();
    let mut keyed: Vec<((usize, usize), Region)> = keys.into_iter().zip(regions).collect();
    keyed.sort_by_key(|(key, _)| *key);
    Ok((keyed.into_iter().map(|(_, r)| r).collect(), shapes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{Point, Polyline};
    use dc_project::InstanceId;

    fn shape(profile: usize) -> ShapeRef {
        ShapeRef {
            instance: InstanceId::new("i1"),
            instance_name: "Design".to_owned(),
            profile,
        }
    }

    fn square(at_mm: f64, side_mm: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(at_mm, at_mm), side_mm, side_mm)).expect("a square")
    }

    /// #482: a bay in a ring's island, both floor pieces of one shape and
    /// the bay given first, comes back after the ring, so the Pocket code
    /// cuts the ring first; pieces of two shapes keep `nest`'s order, the
    /// bay first, and their containment pair orders them.
    #[test]
    fn renest_puts_a_piece_after_the_piece_round_its_island() {
        let (ring, _) = nest(vec![square(0.0, 40.0), square(10.0, 20.0)]).expect("a ring");
        let (bay, _) = nest(vec![square(14.0, 12.0)]).expect("a bay");
        let (ring, bay) = (ring[0].clone(), bay[0].clone());
        let depths = |floors: &[&(Region, &ShapeRef)]| -> Vec<usize> {
            let (regions, _) = renest(floors).expect("nested");
            regions.iter().map(Region::depth).collect()
        };
        let (one, other) = (shape(0), shape(1));
        assert_eq!(
            depths(&[&(bay.clone(), &one), &(ring.clone(), &one)]),
            [0, 2]
        );
        assert_eq!(depths(&[&(bay, &other), &(ring, &one)]), [2, 0]);
    }
}
