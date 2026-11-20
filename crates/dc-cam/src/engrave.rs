// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/carving.ts (generateFollowLineToolpath, direct
//   mode; buildCarveLevels)

//! Engrave toolpaths (`docs/specs/phase-2/design.md`, "Algorithms →
//! Engrave"): every target path, open or closed, traced with the tool
//! centre on the line, no offset, with any cutter.
//!
//! Upstream's direct mode retracts to safe Z after every level of every
//! fragment. Here each path at each pass is one [`CutBlock`] and
//! [`crate::sequence`](fn@crate::sequence) writes the links, so consecutive passes of a closed
//! path, and of an open path under `alternate_direction`, plunge straight
//! from the previous pass (functional spec, capability 15; decision D9).
//! Depth is measured from the stock top and bounded by S1 with the
//! allowance, where upstream clamps to absolute 0. Nothing here depends on
//! Y-up or Y-down: the tool follows the line as drawn.

use crate::block::CutBlock;
use crate::context::{self, OpContext, check_diameter, check_safe_z, rates_of, tool_of};
use crate::error::{CamError, ShapeRef};
use crate::passes::pass_z_mm;
use crate::profile::containment_pairs;
use crate::sequence::{SequenceInput, sequence};
use crate::tabs::segment_kind;
use crate::targets::target_profiles;
use crate::toolpath::{Move, Toolpath};
use crate::warning::Warning;
use dc_geometry::{Contour, EPS_MM, Point3, Polyline, nest};
use dc_project::{EngraveOp, Project};

/// Generates the Engrave toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Every profile of every target instance, open or closed, is placed by
/// its transform and traced with the tool centre on the line: no offset,
/// so any cutter is allowed. Each path becomes one [`CutBlock`] per depth
/// pass (a straight plunge at the plunge rate, then the path's moves at the
/// feed rate; arc segments stay arcs), and [`sequence`] orders and links
/// the blocks by `op.machining_order`. A closed path ends at its start, so
/// its next pass plunges straight from the previous one. An open path does
/// too under `op.alternate_direction`, where odd passes run the path
/// backwards; without it every pass starts at the path's first vertex, so
/// the tool retracts to safe Z, rapids back, and plunges. Closed paths keep
/// Profile's containment order (an outline inside another is cut first);
/// open paths have none.
///
/// A path of fewer than two distinct points is skipped with
/// [`Warning::EngravePathDegenerate`]. Fails without a toolpath when a
/// feed, plunge rate, or step-down is missing, when the depth goes below the
/// stock bottom plus allowance (S1), when an id does not resolve, or when
/// the cutter would leave the stock ([`CamError::OffStock`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_engrave(
    project: &Project,
    op: &EngraveOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    engrave(project, op).map_err(|e| e.for_op(&op.id))
}

/// [`generate_engrave`], before a defect it finds
/// ([`CamError::Internal`]) names `op`.
fn engrave(project: &Project, op: &EngraveOp) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = tool_of(project, &op.id, &op.tool)?;
    let rates = rates_of(tool)?;
    check_diameter(tool)?;
    check_safe_z(&op.id, op.safe_z_mm)?;
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

    let mut warnings = Vec::new();
    let paths = resolve_targets(project, op, &mut warnings)?;
    let containment = closed_containment(&paths)?;

    let mut blocks = Vec::with_capacity(paths.len() * pass_z.len());
    for (shape, path) in &paths {
        let closed = path.is_closed();
        let backwards = path.reversed();
        // The Z of the pass above, where the tool is when a pass begins.
        let mut above_z_mm: Option<f64> = None;
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            let reverse = !closed && op.alternate_direction && pass % 2 == 1;
            let traced = if reverse { &backwards } else { path };
            // `resolve_targets` kept only paths of two distinct points.
            let entry = traced
                .vertices()
                .first()
                .map(|v| v.point())
                .ok_or_else(|| CamError::internal("an engraved path has no vertex to start at"))?;
            let cuts: Vec<Move> = traced
                .segments()
                .filter(|s| s.start.distance_to(s.end) > EPS_MM)
                .map(|s| Move {
                    kind: segment_kind(&s),
                    to: Point3::new(s.end.x, s.end.y, z_mm),
                    feed_mm_min: Some(rates.feed_mm_min),
                })
                .collect();
            let exit = cuts.last().map_or(entry, |m| m.to.xy());
            // The tool is already at this pass's entry when the previous
            // pass ended there: always for a closed path, and for an open
            // path cut back and forth. Then the descent starts at the
            // previous pass Z; otherwise the sequencer retracts first.
            let stays_down = closed || op.alternate_direction;
            let descent_from_z_mm = above_z_mm.filter(|_| stays_down).unwrap_or(safe_z_mm);
            above_z_mm = Some(z_mm);
            blocks.push(CutBlock {
                shape: shape.clone(),
                pass,
                z_mm,
                descent_from: entry,
                descent_from_z_mm,
                descent: Vec::new(),
                entry,
                entry_z_mm: z_mm,
                cuts,
                exit,
                frees: closed && pass + 1 == pass_z.len() && z_mm <= EPS_MM,
                cleared: None,
                group: 0,
            });
        }
    }
    let input = SequenceInput {
        order: op.machining_order,
        containment: &containment,
        ..SequenceInput::new(&ctx, blocks)
    };
    let toolpath = sequence(&input)?;
    // The cutter must stay on the stock.
    ctx.check_on_stock(&toolpath, tool.cut_radius_mm())?;
    Ok((toolpath, warnings))
}

/// Places every profile of every target instance, in target then profile
/// order. A path of fewer than two distinct points is skipped with a
/// warning.
fn resolve_targets(
    project: &Project,
    op: &EngraveOp,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Polyline)>, CamError> {
    let mut paths = Vec::new();
    for target in target_profiles(project, &op.id, &op.targets) {
        let target = target?;
        let placed = target.placed()?;
        if !has_two_distinct_points(&placed) {
            warnings.push(Warning::EngravePathDegenerate {
                op: op.id.clone(),
                shape: target.shape,
            });
            continue;
        }
        paths.push((target.shape, placed));
    }
    Ok(paths)
}

/// True when some vertex lies more than [`EPS_MM`] from the first.
fn has_two_distinct_points(path: &Polyline) -> bool {
    let Some(first) = path.vertices().first().map(|v| v.point()) else {
        return false;
    };
    path.vertices()
        .iter()
        .any(|v| v.point().distance_to(first) > EPS_MM)
}

/// The `(inner, encloser)` pairs among the closed paths, as Profile
/// derives them from `nest`. Open paths take no part. A closed path that
/// is not a valid contour (it crosses itself, or encloses no area) is
/// engraved as drawn but takes no part either; and when the closed paths
/// cross one another, which engraving allows, there is no nesting to
/// order by, so none is applied. Fails as [`containment_pairs`] does.
fn closed_containment(
    paths: &[(ShapeRef, Polyline)],
) -> Result<Vec<(ShapeRef, ShapeRef)>, CamError> {
    let closed: Vec<(ShapeRef, Contour)> = paths
        .iter()
        .filter(|(_, p)| p.is_closed())
        .filter_map(|(shape, p)| Contour::new(p.clone()).ok().map(|c| (shape.clone(), c)))
        .collect();
    let contours: Vec<Contour> = closed.iter().map(|(_, c)| c.clone()).collect();
    match nest(contours) {
        Ok((regions, _)) => containment_pairs(&regions, &closed),
        Err(_) => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests;
