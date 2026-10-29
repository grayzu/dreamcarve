// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts
//   src/engine/toolpaths/drillingCountersink.test.ts
//   src/engine/toolpaths/drillingRetract.test.ts
//   src/engine/toolpaths/toolpaths.test.ts (drilling ordering tests)
//   src/engine/toolpaths/camOperationSmoke.test.ts (drilling cases)

//! Drill toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Drill";
//! functional spec capability 14, safety rule S15).
//!
//! What changed in the port: a hole is any closed target whose outline is
//! a circle within the project's flatten tolerance
//! ([`dc_geometry::as_circle`]), not a `circle` feature kind; the
//! chip-break gap is the operation's `gap_mm`, not a constant in project
//! units; a dwell is kept as a [`crate::MoveKind::Dwell`] move when the cycle is
//! written as plain moves (upstream loses it); pecks count from the
//! retract plane, as a controller's `G83`/`G73` counts them from `R`,
//! not from the stock top (#153); holes are visited nearest
//! neighbour from the first circle in drawing order (D5), not from the
//! origin, by [`crate::sequence`](fn@crate::sequence); there is no countersink or
//! clamp check, and helical boring is its own operation ([`crate::bore`]).
//! Nothing here depends on Y-up or Y-down: a drill moves only
//! in Z at each centre, and distances are symmetric under the flip.

use crate::block::CutBlock;
use crate::error::{CamError, FeedField, ShapeRef};
use crate::passes::pass_z_mm;
use crate::sequence::{SequenceInput, clearance_z_mm, plan, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::toolpath::{Move, Toolpath};
use crate::warning::Warning;
use dc_geometry::{Point, Point3, as_circle};
use dc_project::{
    DrillCycle, DrillOp, InstanceId, MachiningOrder, OperationId, PeckRetract, Project, Tool,
};

/// How far above the depth already drilled a full-retract peck re-enters
/// the hole by rapid, mm. The tool re-enters only through air it has
/// already cut (S15); the next peck feeds from there.
pub const PECK_REENTRY_MM: f64 = 0.5;

/// The canned cycle a hole's plain moves are equivalent to, for the
/// emitter (`docs/specs/phase-2/design.md`, "Canned cycles").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannedKind {
    /// Simple, no dwell.
    G81,
    /// Simple with a dwell at the bottom.
    G82,
    /// Peck with a full retract, no dwell.
    G83,
    /// Peck with a chip-break retract, no dwell.
    G73,
    /// No canned cycle describes it (a peck with a dwell): always written
    /// as plain moves.
    Expanded,
}

/// One drilled hole, as the emitter may write it as a canned cycle. Z
/// values are absolute project Z, mm (stock bottom at 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrillHole {
    /// The hole centre, mm, project space.
    pub centre: Point,
    /// The hole top: the stock top.
    pub top_z_mm: f64,
    /// The hole bottom.
    pub bottom_z_mm: f64,
    /// The retract plane (`R`): `min(safe Z, top + retract_mm)`.
    pub retract_z_mm: f64,
    /// The equivalent canned cycle.
    pub cycle: CannedKind,
    /// The peck depth, mm, for a peck cycle.
    pub peck_mm: Option<f64>,
    /// The dwell at the bottom of every plunge, seconds, when set.
    pub dwell_s: Option<f64>,
}

/// Drill output: the toolpath, which is the authority (S11), and one
/// record per hole in the order the toolpath drills them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrillToolpath {
    /// The moves.
    pub toolpath: Toolpath,
    /// The holes, in cut order.
    pub holes: Vec<DrillHole>,
}

/// Generates the Drill toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Every closed profile of every target instance, placed by its
/// transform, whose outline is a circle within the project's flatten
/// tolerance is a hole; anything else warns
/// [`Warning::DrillTargetNotCircle`] and is skipped. A hole narrower than
/// the tool by more than the flatten tolerance is skipped with
/// [`Warning::DrillHoleTooSmall`], since a Drill never moves sideways and
/// the hole would come out the tool's size (maintainer decision
/// 2026-10-24); with `op.allow_tool_wider_than_hole` it is drilled anyway
/// and warns [`Warning::DrillToolWiderThanHole`]. With no hole left the
/// toolpath is empty and [`Warning::DrillNoHoles`] says so. Each hole is
/// drilled from the stock top to `depth_mm` (S1 with the operation's
/// through-cut allowance):
///
/// - the tool arrives at safe Z and rapids down to the retract plane
///   `min(safe Z, top + retract_mm)` (a negative `retract_mm` is raised to
///   the top with [`Warning::DrillRetractBelowStockTop`]);
/// - it plunges at the tool's plunge rate, in one plunge for a simple
///   cycle or in pecks of the peck depth (the operation's, else the tool's
///   step-down), waiting `dwell_s` at the bottom of every plunge as a
///   [`crate::MoveKind::Dwell`] move. Pecks count from the retract plane,
///   as a controller's `G83`/`G73` counts them from `R` (#153): peck `k`
///   ends at `max(bottom, retract plane − k·peck)`, so the first removes
///   the peck less the retract height (nothing, if the retract height is
///   the larger), the last may be shorter, and none cuts more than a peck
///   below the depth already drilled;
/// - between pecks it rapids up to the retract plane and back down to
///   [`PECK_REENTRY_MM`] above the depth reached (full retract), or up by
///   the chip-break gap (capped at the retract plane), and feeds on from
///   there; every rapid down ends in air already cut (S15);
/// - it ends with a rapid to safe Z, where the `G98` return of a canned
///   cycle puts it.
///
/// Holes are visited by [`crate::sequence`](fn@crate::sequence): nearest neighbour from the
/// first hole in drawing order, ties by drawing order, linked at safe Z.
/// A peck that reaches the bottom from the retract plane drills the hole
/// in one plunge, with [`Warning::DrillPeckDeeperThanHole`].
///
/// Fails without a toolpath when the plunge rate, the stock thickness, the
/// depth, or a peck's step-down is missing; when the depth passes the
/// stock bottom plus allowance; when the chip-break gap is not positive or
/// the dwell is negative; when an id does not resolve or a target cannot
/// be placed; or when the cutter would leave the stock
/// ([`CamError::OffStock`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_drill(
    project: &Project,
    op: &DrillOp,
) -> Result<(DrillToolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    let plunge_mm_min = tool
        .plunge_mm_min
        .filter(|r| r.is_finite() && *r > 0.0)
        .ok_or_else(|| CamError::MissingFeed {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            field: FeedField::Plunge,
        })?;
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
    if !(op.dwell_s.is_finite() && op.dwell_s >= 0.0) {
        return Err(CamError::InvalidDrillSetting {
            op: op.id.clone(),
            field: "dwell_s",
            value: op.dwell_s,
        });
    }
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let depth_mm = op
        .depth_mm
        .ok_or_else(|| CamError::MissingDepth { op: op.id.clone() })?;
    let cycle = Cycle::of(op, tool)?;
    // The hole bottom, with the depth checked against the stock bottom
    // plus the allowance (S1) as for every generator.
    let bottom_z_mm = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        depth_mm,
        op.through_cut_allowance_mm,
    )?
    .last()
    .copied()
    .unwrap_or(thickness_mm - depth_mm);
    let top_z_mm = thickness_mm;
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let mut warnings = Vec::new();
    if op.retract_mm < 0.0 {
        warnings.push(Warning::DrillRetractBelowStockTop {
            op: op.id.clone(),
            requested_mm: op.retract_mm,
            used_mm: 0.0,
        });
    }
    let retract_z_mm = (top_z_mm + op.retract_mm.max(0.0)).min(safe_z_mm);
    // Plunge levels, shallowest first; a simple cycle is one level. Pecks
    // count from the retract plane, as a controller's `G83`/`G73` counts
    // them from `R` (#153), so a canned cycle and these moves are the
    // same motion for any retract height. The pass rule is the one every
    // generator uses, from `R` instead of the stock top; `R` is at or
    // above the top, so no peck cuts deeper than a peck into material.
    let mut levels = match cycle.peck_mm {
        None => vec![bottom_z_mm],
        Some(peck_mm) => pass_z_mm(
            &op.id,
            retract_z_mm,
            retract_z_mm - bottom_z_mm,
            peck_mm,
            op.through_cut_allowance_mm,
        )?,
    };
    if let Some(last) = levels.last_mut() {
        *last = bottom_z_mm;
    }
    if let Some(peck_mm) = cycle.peck_mm
        && levels.len() == 1
    {
        warnings.push(Warning::DrillPeckDeeperThanHole {
            op: op.id.clone(),
            peck_mm,
            depth_mm,
        });
    }
    let dwell_s = (op.dwell_s > 0.0).then_some(op.dwell_s);

    let holes = resolve_holes(project, op, tool.diameter_mm, &mut warnings)?;
    let blocks: Vec<CutBlock> = holes
        .iter()
        .map(|(shape, centre)| {
            let at = |z: f64| Point3::new(centre.x, centre.y, z);
            let mut cuts = Vec::with_capacity(levels.len() * 4);
            for (k, &z) in levels.iter().enumerate() {
                cuts.push(Move::linear(at(z), plunge_mm_min));
                if let Some(seconds) = dwell_s {
                    cuts.push(Move::dwell(at(z), seconds));
                }
                if k + 1 == levels.len() {
                    break;
                }
                // The next level, if any, feeds on from where these leave
                // the tool, always at or above `z`: air already cut.
                match &cycle.retract {
                    PeckRetract::Full => {
                        cuts.push(Move::rapid(at(retract_z_mm)));
                        cuts.push(Move::rapid(at((z + PECK_REENTRY_MM).min(retract_z_mm))));
                    }
                    PeckRetract::ChipBreak { gap_mm } => {
                        cuts.push(Move::rapid(at((z + *gap_mm).min(retract_z_mm))));
                    }
                }
            }
            cuts.push(Move::rapid(at(safe_z_mm)));
            CutBlock {
                shape: shape.clone(),
                pass: 0,
                z_mm: levels.last().copied().unwrap_or(top_z_mm),
                descent_from: *centre,
                descent_from_z_mm: safe_z_mm,
                // The rapid down to the retract plane, above the stock:
                // the one rapid below safe Z the sequencer passes through
                // (design spec, "Algorithms → Sequence", step 5).
                descent: vec![Move::rapid(at(retract_z_mm))],
                entry: *centre,
                // The descent ends at the retract plane, where the first
                // feed down begins.
                entry_z_mm: retract_z_mm,
                cuts,
                exit: *centre,
                frees: false,
                cleared: None,
                group: 0,
            }
        })
        .collect();
    let input = SequenceInput {
        blocks,
        order: MachiningOrder::FullDepthFirst,
        containment: &[],
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        stock_top_mm: thickness_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        // Holes have no cleared area, so no stay-down link is ever cut at
        // this rate; the plunge rate is the only rate a Drill uses.
        feed_mm_min: plunge_mm_min,
        plunge_mm_min,
        cut_before: &[],
    };
    let records = plan(&input)?
        .into_iter()
        .filter_map(|i| input.blocks.get(i))
        .map(|block| DrillHole {
            centre: block.entry,
            top_z_mm,
            bottom_z_mm: block.z_mm,
            retract_z_mm,
            cycle: cycle.canned(dwell_s.is_some()),
            peck_mm: cycle.peck_mm,
            dwell_s,
        })
        .collect();
    let toolpath = sequence(&input)?;
    // #62: the cutter must stay on the stock.
    if let Some(overhang) = swept_bounds(&toolpath, tool.diameter_mm / 2.0)
        .and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.id.clone(),
            overhang,
        });
    }
    Ok((
        DrillToolpath {
            toolpath,
            holes: records,
        },
        warnings,
    ))
}

/// The operation's cycle, resolved: the peck depth in effect and how the
/// tool retracts between pecks.
#[derive(Debug, Clone)]
struct Cycle {
    /// `None` for a simple cycle.
    peck_mm: Option<f64>,
    /// How a peck retracts; unused for a simple cycle.
    retract: PeckRetract,
}

impl Cycle {
    fn of(op: &DrillOp, tool: &Tool) -> Result<Self, CamError> {
        match &op.cycle {
            DrillCycle::Simple => Ok(Self {
                peck_mm: None,
                retract: PeckRetract::Full,
            }),
            DrillCycle::Countersink { .. } => Err(CamError::NotSupportedYet {
                op: op.id.clone(),
                feature: "a countersink cycle".into(),
            }),
            DrillCycle::Peck {
                peck_depth_mm,
                retract,
            } => {
                if let PeckRetract::ChipBreak { gap_mm } = retract
                    && !(gap_mm.is_finite() && *gap_mm > 0.0)
                {
                    return Err(CamError::InvalidDrillSetting {
                        op: op.id.clone(),
                        field: "gap_mm",
                        value: *gap_mm,
                    });
                }
                let peck_mm = peck_depth_mm.or(tool.step_down_mm).ok_or_else(|| {
                    CamError::MissingStepDown {
                        op: op.id.clone(),
                        tool: tool.name.clone(),
                        tool_id: tool.id.clone(),
                    }
                })?;
                Ok(Self {
                    peck_mm: Some(peck_mm),
                    retract: retract.clone(),
                })
            }
        }
    }

    /// The canned cycle these moves are equivalent to (design spec,
    /// "Algorithms → Drill", step 6).
    fn canned(&self, dwells: bool) -> CannedKind {
        match (self.peck_mm, &self.retract, dwells) {
            (None, _, false) => CannedKind::G81,
            (None, _, true) => CannedKind::G82,
            (Some(_), PeckRetract::Full, false) => CannedKind::G83,
            (Some(_), PeckRetract::ChipBreak { .. }, false) => CannedKind::G73,
            (Some(_), _, true) => CannedKind::Expanded,
        }
    }
}

/// A target profile and its circle, centre and radius, if it is one.
pub(crate) type TargetCircle = (ShapeRef, Option<(Point, f64)>);

/// Every profile of every target of `op_id`, in drawing order (target
/// order, then profile order), with its circle: the centre and radius of
/// a closed profile whose outline, placed by its instance's transform, is
/// a circle within the project's flatten tolerance, and `None` for any
/// other profile. Drill and Bore find their holes this way.
pub(crate) fn circles_of(
    project: &Project,
    op_id: &OperationId,
    targets: &[InstanceId],
) -> Result<Vec<TargetCircle>, CamError> {
    let mut out = Vec::new();
    for instance_id in targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op_id.clone(),
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
            let circle = if polyline.is_closed() {
                let placed = instance
                    .transform
                    .apply_polyline(polyline)
                    .map_err(|source| CamError::InvalidTarget {
                        shape: shape.clone(),
                        source,
                    })?;
                as_circle(&placed, project.flatten_tolerance_mm)
            } else {
                None
            };
            out.push((shape, circle));
        }
    }
    Ok(out)
}

/// The holes of `op`, in drawing order (target order, then profile order):
/// each circular closed profile, placed by its instance's transform, with
/// its centre. Other profiles warn and are skipped. A hole narrower than
/// the tool by more than the flatten tolerance warns and is skipped
/// (maintainer decision 2026-10-24), unless the operation allows it, when
/// it is drilled at the tool's size and warns. No hole left warns once.
fn resolve_holes(
    project: &Project,
    op: &DrillOp,
    tool_diameter_mm: f64,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Point)>, CamError> {
    let mut holes = Vec::new();
    for (shape, circle) in circles_of(project, &op.id, &op.targets)? {
        let Some((centre, radius_mm)) = circle else {
            warnings.push(Warning::DrillTargetNotCircle {
                op: op.id.clone(),
                shape,
            });
            continue;
        };
        let hole_mm = 2.0 * radius_mm;
        // The outline was taken for a circle within the flatten
        // tolerance, so its diameter is known no closer than that: a
        // hole drawn at the tool's size is drilled.
        if tool_diameter_mm > hole_mm + project.flatten_tolerance_mm {
            if !op.allow_tool_wider_than_hole {
                warnings.push(Warning::DrillHoleTooSmall {
                    op: op.id.clone(),
                    shape,
                    hole_mm,
                    tool_mm: tool_diameter_mm,
                });
                continue;
            }
            warnings.push(Warning::DrillToolWiderThanHole {
                op: op.id.clone(),
                shape: shape.clone(),
                hole_mm,
                tool_mm: tool_diameter_mm,
            });
        }
        holes.push((shape, centre));
    }
    if holes.is_empty() {
        warnings.push(Warning::DrillNoHoles { op: op.id.clone() });
    }
    Ok(holes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point3, Polyline, Transform, Vertex, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        DefinitionId, DrillCycle, FeatureDefinition, FeatureInstance, InstanceId, OperationId,
        PeckRetract, Stock, Tool, ToolId, ToolKind,
    };
    use std::collections::BTreeMap;

    const TOL: f64 = 1e-6;
    const THICKNESS_MM: f64 = 12.0;
    /// 5 mm above the 12 mm stock.
    const SAFE_Z_MM: f64 = 17.0;
    /// 1 mm above the stock top.
    const RETRACT_Z_MM: f64 = 13.0;
    const PLUNGE_MM_MIN: f64 = 300.0;

    /// A 3 mm twist drill with a 4 mm step-down.
    fn drill_tool() -> Tool {
        Tool {
            id: ToolId::new("t1"),
            name: "3 mm drill".into(),
            vendor: None,
            product_id: None,
            number: None,
            cutter: ToolKind::Drill,
            diameter_mm: 3.0,
            flutes: 2,
            feed_mm_min: Some(1000.0),
            plunge_mm_min: Some(PLUNGE_MM_MIN),
            rpm: Some(18000),
            step_down_mm: Some(4.0),
            step_over_fraction: None,
        }
    }

    /// A 200 × 100 × 12 mm project with one instance per `(id, outline)`,
    /// each with its own definition, placed untransformed, in that drawing
    /// order.
    fn project(shapes: &[(&str, Polyline)]) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 200.0,
            height_mm: 100.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        p.tools = vec![drill_tool()];
        p.definitions = BTreeMap::new();
        p.instances = Vec::new();
        for (id, outline) in shapes {
            let definition = FeatureDefinition {
                id: DefinitionId::new(format!("d_{id}")),
                name: format!("Shape {id}"),
                profiles: vec![outline.clone()],
            };
            p.instances.push(FeatureInstance {
                id: InstanceId::new(*id),
                definition: definition.id.clone(),
                transform: Transform::translate(0.0, 0.0),
                name: format!("Shape {id}"),
                visible: true,
                locked: false,
                z_top_mm: Some(THICKNESS_MM),
                z_bottom_mm: Some(0.0),
            });
            p.definitions.insert(definition.id.clone(), definition);
        }
        p.operations.clear();
        p
    }

    /// A circle of diameter `diameter_mm` centred at (`x`, `y`).
    fn circle(x: f64, y: f64, diameter_mm: f64) -> Polyline {
        Polyline::circle(Point::new(x, y), diameter_mm / 2.0)
    }

    /// 5 mm circles at each `(x, y)`, instance ids `c1`, `c2`, ….
    fn circles(centres: &[(f64, f64)]) -> Project {
        let named: Vec<(String, Polyline)> = centres
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| (format!("c{}", i + 1), circle(x, y, 5.0)))
            .collect();
        let shapes: Vec<(&str, Polyline)> = named
            .iter()
            .map(|(id, outline)| (id.as_str(), outline.clone()))
            .collect();
        project(&shapes)
    }

    /// A simple drill of every instance, through the 12 mm stock, retract
    /// 1 mm, safe Z 5 mm.
    fn drill_op(project: &Project) -> DrillOp {
        DrillOp {
            id: OperationId::new("o1"),
            name: "Drill".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            cycle: DrillCycle::Simple,
            dwell_s: 0.0,
            depth_mm: Some(THICKNESS_MM),
            retract_mm: 1.0,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            allow_tool_wider_than_hole: false,
        }
    }

    fn peck(peck_depth_mm: Option<f64>, retract: PeckRetract) -> DrillCycle {
        DrillCycle::Peck {
            peck_depth_mm,
            retract,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &DrillOp) -> (DrillToolpath, Vec<Warning>) {
        let (drilled, warnings) = generate_drill(project, op).unwrap();
        assert_safe(
            &drilled.toolpath,
            project,
            &dc_project::Operation::Drill(op.clone()),
        );
        (drilled, warnings)
    }

    /// The moves of a one-hole toolpath as (kind, Z): `R` rapid, `L`
    /// straight cut, `D` dwell. Every move must be at the hole's centre
    /// once the first rapid has arrived.
    fn profile_of(toolpath: &Toolpath) -> Vec<(char, f64)> {
        let centre = toolpath.moves.first().map(|m| m.to.xy()).unwrap();
        toolpath
            .moves
            .iter()
            .map(|m| {
                assert!(
                    m.to.xy().distance_to(centre) <= EPS_MM,
                    "a one-hole drill moved off the centre: {m:?}"
                );
                let kind = match m.kind {
                    MoveKind::Rapid => 'R',
                    MoveKind::Linear => 'L',
                    MoveKind::Dwell { .. } => 'D',
                    MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. } => 'A',
                };
                (kind, m.to.z)
            })
            .collect()
    }

    fn assert_profile(actual: &[(char, f64)], expected: &[(char, f64)]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "moves {actual:?}, expected {expected:?}"
        );
        for (i, ((ka, za), (ke, ze))) in actual.iter().zip(expected).enumerate() {
            assert_eq!(ka, ke, "move {i}: {actual:?}, expected {expected:?}");
            assert_approx_eq!(*za, *ze, TOL);
        }
    }

    /// The straight cuts: every one is a plunge.
    fn plunges(toolpath: &Toolpath) -> Vec<Move> {
        toolpath
            .moves
            .iter()
            .filter(|m| m.kind == MoveKind::Linear)
            .copied()
            .collect()
    }

    /// X of the first plunge at each hole, in drilled order.
    fn hole_xs(toolpath: &Toolpath) -> Vec<f64> {
        let mut xs: Vec<f64> = Vec::new();
        let mut last: Option<Point> = None;
        for m in plunges(toolpath) {
            if last.is_none_or(|p| p.distance_to(m.to.xy()) > EPS_MM) {
                xs.push(m.to.x);
            }
            last = Some(m.to.xy());
        }
        xs
    }

    fn assert_xs(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert_approx_eq!(*a, *e, TOL);
        }
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code()).collect()
    }

    /// The instance and profile a drill shape warning names.
    fn shape_of(warning: &Warning) -> (&str, usize) {
        let (Warning::DrillTargetNotCircle { shape, .. }
        | Warning::DrillHoleTooSmall { shape, .. }
        | Warning::DrillToolWiderThanHole { shape, .. }) = warning
        else {
            panic!("{warning:?}");
        };
        (shape.instance.as_str(), shape.profile)
    }

    /// Port of `camOperationSmoke.test.ts` "drilling simple: single
    /// plunge + rapid retract": rapid in at safe Z, rapid down to the retract
    /// plane 1 mm above the stock, one plunge at the plunge rate to the
    /// bottom, one rapid back to safe Z (acceptance 23; the `G98` return
    /// to the initial Z a canned `G81` makes).
    #[test]
    fn simple_is_one_plunge_and_retract() {
        let p = circles(&[(50.0, 50.0)]);
        let op = drill_op(&p);
        let (drilled, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
        let plunge = plunges(&drilled.toolpath);
        assert_eq!(plunge[0].feed_mm_min, Some(PLUNGE_MM_MIN));
        assert_approx_eq!(plunge[0].to.x, 50.0, TOL);
        assert_approx_eq!(plunge[0].to.y, 50.0, TOL);
    }

    /// Capability 14: `dwell_s` waits at the bottom of every plunge, simple
    /// or peck. Upstream drops the dwell when it expands a cycle.
    #[test]
    fn dwell_adds_dwell_move_at_bottom() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.dwell_s = 0.5;
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 0.0),
                ('D', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
        let dwell = drilled.toolpath.moves[3];
        assert_eq!(dwell.kind, MoveKind::Dwell { seconds: 0.5 });
        assert_eq!(dwell.feed_mm_min, None);

        op.cycle = peck(Some(6.0), PeckRetract::Full);
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 7.0),
                ('D', 7.0),
                ('R', RETRACT_Z_MM),
                ('R', 7.0 + PECK_REENTRY_MM),
                ('L', 1.0),
                ('D', 1.0),
                ('R', RETRACT_Z_MM),
                ('R', 1.0 + PECK_REENTRY_MM),
                ('L', 0.0),
                ('D', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
    }

    /// Port of `camOperationSmoke.test.ts` "drilling peck: multiple plunges
    /// with full retracts", with acceptance 23's numbers: 12 mm in 4 mm
    /// pecks from the retract plane 1 mm above the hole top is four
    /// plunges (to 9, 5, 1 and 0; #153), each but the last retracting to
    /// the retract plane and re-entering 0.5 mm above the depth reached.
    #[test]
    fn peck_full_retracts_above_hole_top() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(Some(4.0), PeckRetract::Full);
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 9.0),
                ('R', RETRACT_Z_MM),
                ('R', 9.5),
                ('L', 5.0),
                ('R', RETRACT_Z_MM),
                ('R', 5.5),
                ('L', 1.0),
                ('R', RETRACT_Z_MM),
                ('R', 1.5),
                ('L', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
    }

    /// Port of `camOperationSmoke.test.ts` "drilling chip_breaking":
    /// between pecks the tool lifts by the gap only (upstream's fixed 0.5
    /// project units is the operation's `gap_mm` here), and the hole ends
    /// with the retract to safe Z.
    #[test]
    fn chip_break_retracts_by_gap() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 0.3 });
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 9.0),
                ('R', 9.3),
                ('L', 5.0),
                ('R', 5.3),
                ('L', 1.0),
                ('R', 1.3),
                ('L', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
    }

    /// Capability 14: an unset peck depth is the tool's step-down; with
    /// neither set there is no peck to drill (validation reports it first).
    #[test]
    fn peck_depth_from_tool_step_down() {
        let mut p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(None, PeckRetract::Full);
        let (drilled, _) = generate(&p, &op);
        let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
        assert_xs(&zs, &[9.0, 5.0, 1.0, 0.0]);

        p.tools[0].step_down_mm = None;
        let err = generate_drill(&p, &op).unwrap_err();
        assert!(
            matches!(err, CamError::MissingStepDown { ref op, .. } if op.as_str() == "o1"),
            "{err:?}"
        );
    }

    /// 12 mm in 5 mm pecks from the stock top (retract 0): 5, 5, then 2.
    /// With the retract plane on the top, only the last peck is short.
    #[test]
    fn last_peck_is_shallower_never_deeper() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.retract_mm = 0.0;
        op.cycle = peck(Some(5.0), PeckRetract::ChipBreak { gap_mm: 0.5 });
        let (drilled, _) = generate(&p, &op);
        let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
        assert_xs(&zs, &[7.0, 2.0, 0.0]);
    }

    /// #153: peck levels count from the retract plane `R`, as a
    /// controller's `G83`/`G73` does (RS274/NGC 3.5.16; LinuxCNC `G83`):
    /// `z_k = max(bottom, R − k·peck)`. With `R` 1 mm above the 12 mm
    /// stock (Z 13) and 4 mm pecks the plunges end at 9, 5, 1 and 0; the
    /// first removes 3 mm of material, the middle ones a full peck, the
    /// last 1 mm. With `R` on the stock top the levels are the old ones.
    #[test]
    fn peck_increments_count_from_retract_plane() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(Some(4.0), PeckRetract::Full);
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 9.0),
                ('R', RETRACT_Z_MM),
                ('R', 9.5),
                ('L', 5.0),
                ('R', RETRACT_Z_MM),
                ('R', 5.5),
                ('L', 1.0),
                ('R', RETRACT_Z_MM),
                ('R', 1.5),
                ('L', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );

        op.cycle = peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 0.5 });
        let (drilled, _) = generate(&p, &op);
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', 9.0),
                ('R', 9.5),
                ('L', 5.0),
                ('R', 5.5),
                ('L', 1.0),
                ('R', 1.5),
                ('L', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );

        for (retract_mm, expected) in [
            (0.0, vec![8.0, 4.0, 0.0]),
            (2.5, vec![10.5, 6.5, 2.5, 0.0]),
            // R is capped at safe Z (17).
            (50.0, vec![13.0, 9.0, 5.0, 1.0, 0.0]),
        ] {
            op.retract_mm = retract_mm;
            op.cycle = peck(Some(4.0), PeckRetract::Full);
            let (drilled, _) = generate(&p, &op);
            let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
            assert_xs(&zs, &expected);
        }

        // A 12.5 mm peck into the 12 mm hole from R at 13 is two plunges,
        // so it is not drilled in one and does not warn.
        op.retract_mm = 1.0;
        op.cycle = peck(Some(12.5), PeckRetract::Full);
        let (drilled, warnings) = generate(&p, &op);
        let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
        assert_xs(&zs, &[0.5, 0.0]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// #153: measuring from `R` never makes a peck cut deeper into the
    /// material than the peck depth. `R` is at or above the stock top, so
    /// the first plunge ends at or above `top − peck`, and every plunge
    /// removes at most one peck below the depth already drilled (or the
    /// stock top). A retract larger than the peck makes the first pecks
    /// air cuts, as the controller's do. S15 and the bottom still hold.
    #[test]
    fn first_peck_never_exceeds_peck_depth_below_stock_top() {
        let p = circles(&[(50.0, 50.0)]);
        for retract_mm in [0.0, 0.5, 1.0, 3.9, 4.0, 4.5, 50.0] {
            for peck_mm in [0.3, 4.0, 5.0, 11.0] {
                for retract in [PeckRetract::Full, PeckRetract::ChipBreak { gap_mm: 0.5 }] {
                    let mut op = drill_op(&p);
                    op.retract_mm = retract_mm;
                    op.cycle = peck(Some(peck_mm), retract.clone());
                    let label = format!("retract {retract_mm}, peck {peck_mm}, {retract:?}");
                    let (drilled, _) = generate(&p, &op);
                    let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
                    let first = zs[0];
                    assert!(
                        first >= THICKNESS_MM - peck_mm - TOL,
                        "{label}: first peck to {first}"
                    );
                    let mut drilled_to = THICKNESS_MM;
                    for &z in &zs {
                        assert!(
                            drilled_to - z <= peck_mm + TOL,
                            "{label}: a peck from {drilled_to} to {z}"
                        );
                        drilled_to = drilled_to.min(z);
                    }
                    assert_approx_eq!(*zs.last().unwrap(), 0.0, TOL);
                    assert_retract_safe(&drilled.toolpath, &label);
                }
            }
        }
    }

    /// A peck at least as deep as the hole drills it in one plunge, with a
    /// warning: the user asked for pecking and gets none.
    #[test]
    fn peck_deeper_than_hole_warns_and_plunges_once() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(Some(20.0), PeckRetract::Full);
        let (drilled, warnings) = generate(&p, &op);
        assert_eq!(plunges(&drilled.toolpath).len(), 1);
        assert_eq!(codes(&warnings), [WarningCode::DrillPeckDeeperThanHole]);
        assert_eq!(warnings[0].op().map(OperationId::as_str), Some("o1"));
    }

    /// Port of `toolpaths.test.ts` `testDrillingOrdersByNearestNeighbor`:
    /// circles at X 10, 50, 30 are drilled at 10, 30, 50. Upstream starts
    /// from the origin; here the walk starts at the first circle drawn,
    /// which is the same circle in this layout.
    #[test]
    fn orders_by_nearest_neighbour() {
        let p = circles(&[(10.0, 10.0), (50.0, 10.0), (30.0, 10.0)]);
        let (drilled, _) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[10.0, 30.0, 50.0]);
    }

    /// Port of `toolpaths.test.ts` `testDrillingTieBreaksByOriginalOrder`:
    /// c1 and c2 at one point and c3 farther away, targeted c1, c3, c2:
    /// the second hole is c2 (distance 0), not c3. Then two holes
    /// equidistant from the first are taken in drawing order (D5).
    #[test]
    fn tie_breaks_by_drawing_order() {
        let p = project(&[
            ("c1", circle(20.0, 20.0, 5.0)),
            ("c3", circle(60.0, 10.0, 5.0)),
            ("c2", circle(20.0, 20.0, 5.0)),
        ]);
        let (drilled, _) = generate(&p, &drill_op(&p));
        let xs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.x).collect();
        assert_xs(&xs, &[20.0, 20.0, 60.0]);

        let p = circles(&[(50.0, 50.0), (70.0, 50.0), (30.0, 50.0)]);
        let (drilled, _) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[50.0, 70.0, 30.0]);
    }

    /// D5: the walk starts at the first circle in drawing order. Drawn at
    /// X 80 then 10, the hole at 80 comes first, where upstream's start at
    /// the origin would take 10 first.
    #[test]
    fn starts_from_first_in_drawing_order() {
        let p = circles(&[(80.0, 10.0), (10.0, 10.0)]);
        let (drilled, _) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[80.0, 10.0]);
    }

    /// Capability 14: a target that is not a circle, or not closed, is
    /// skipped with a warning naming it; the circles are still drilled.
    #[test]
    fn non_circle_target_warns_and_is_skipped() {
        let open = Polyline::new(
            vec![Vertex::line(100.0, 10.0), Vertex::line(120.0, 10.0)],
            false,
        );
        let p = project(&[
            ("c1", circle(10.0, 10.0, 5.0)),
            ("r", Polyline::rect(Point::new(40.0, 40.0), 5.0, 5.0)),
            ("c2", circle(30.0, 10.0, 5.0)),
            ("open", open),
        ]);
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[10.0, 30.0]);
        assert_eq!(
            codes(&warnings),
            [
                WarningCode::DrillTargetNotCircle,
                WarningCode::DrillTargetNotCircle
            ]
        );
        assert_eq!(shape_of(&warnings[0]), ("r", 0));
        assert_eq!(shape_of(&warnings[1]).0, "open");
    }

    /// A circle drawn as four quarter arcs (a DXF or upstream four-arc
    /// circle) and one flattened to a polygon at the project's tolerance
    /// (an SVG import) are both holes, drilled at their centres.
    #[test]
    fn four_arc_circle_and_facet_circle_both_detected() {
        let quarter = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
        let four_arcs = Polyline::new(
            vec![
                Vertex::new(32.5, 20.0, quarter),
                Vertex::new(30.0, 22.5, quarter),
                Vertex::new(27.5, 20.0, quarter),
                Vertex::new(30.0, 17.5, quarter),
            ],
            true,
        );
        let tolerance_mm = sample_project().flatten_tolerance_mm;
        let facets = circle(70.0, 20.0, 5.0).with_arcs_flattened(tolerance_mm);
        assert!(facets.vertices().len() > 4, "{facets:?}");
        let p = project(&[("arcs", four_arcs), ("facets", facets)]);
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(drilled.holes.len(), 2);
        assert_approx_eq!(drilled.holes[0].centre.x, 30.0, 1e-3);
        assert_approx_eq!(drilled.holes[0].centre.y, 20.0, 1e-3);
        assert_approx_eq!(drilled.holes[1].centre.x, 70.0, 1e-3);
        assert_approx_eq!(drilled.holes[1].centre.y, 20.0, 1e-3);
    }

    /// Acceptance 23: a 5 × 8 mm slot with two 5 mm circles drills two
    /// holes and warns about the slot.
    #[test]
    fn slot_is_not_a_circle() {
        let slot = Polyline::new(
            vec![
                Vertex::line(52.5, 10.0),
                Vertex::new(55.5, 10.0, 1.0),
                Vertex::line(55.5, 15.0),
                Vertex::new(52.5, 15.0, 1.0),
            ],
            true,
        );
        let p = project(&[
            ("c1", circle(10.0, 10.0, 5.0)),
            ("slot", slot),
            ("c2", circle(30.0, 10.0, 5.0)),
        ]);
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert_eq!(drilled.holes.len(), 2);
        assert_eq!(codes(&warnings), [WarningCode::DrillTargetNotCircle]);
        assert_eq!(shape_of(&warnings[0]).0, "slot");
    }

    /// Maintainer decision 2026-10-24 (format 18): the warning for a hole
    /// narrower than the drill tells the user a Bore cuts holes bigger
    /// than its bit.
    #[test]
    fn hole_too_small_message_points_to_bore() {
        let mut p = project(&[("small", circle(30.0, 50.0, 5.0))]);
        p.tools[0].diameter_mm = 6.35;
        let (_, warnings) = generate(&p, &drill_op(&p));
        let text = warnings[0].to_string();
        assert_eq!(warnings[0].code(), WarningCode::DrillHoleTooSmall);
        assert!(
            text.contains("cut it to size with a Bore and an end mill smaller than the hole"),
            "{text}"
        );
        assert!(
            text.contains("a Bore cuts holes bigger than its bit"),
            "{text}"
        );
    }

    /// Maintainer decision 2026-10-24: a 5 mm hole with a 6.35 mm tool
    /// is skipped, with a warning giving the hole and both sizes, and no
    /// plunge at its centre; the 10 mm hole beside it is drilled.
    #[test]
    fn hole_narrower_than_tool_is_skipped() {
        let mut p = project(&[
            ("small", circle(30.0, 50.0, 5.0)),
            ("large", circle(70.0, 50.0, 10.0)),
        ]);
        p.tools[0].diameter_mm = 6.35;
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[70.0]);
        assert!(
            drilled
                .toolpath
                .moves
                .iter()
                .filter(|m| m.to.z < THICKNESS_MM)
                .all(|m| m.to.xy().distance_to(Point::new(30.0, 50.0)) > 1.0),
            "a move goes into the stock at the skipped hole"
        );
        assert_eq!(drilled.holes.len(), 1);
        assert_eq!(codes(&warnings), [WarningCode::DrillHoleTooSmall]);
        let Warning::DrillHoleTooSmall {
            hole_mm, tool_mm, ..
        } = warnings[0]
        else {
            panic!("{:?}", warnings[0]);
        };
        assert_approx_eq!(hole_mm, 5.0, 1e-6);
        assert_approx_eq!(tool_mm, 6.35, 1e-9);
        assert_eq!(shape_of(&warnings[0]), ("small", 0));
    }

    /// A hole the tool's size is drilled: within the project's flatten
    /// tolerance (0.01 mm), the tolerance its outline was accepted as a
    /// circle at, a hole is as wide as the tool. 6.3 mm is not.
    #[test]
    fn hole_equal_to_tool_within_tolerance_is_drilled() {
        let mut p = project(&[
            ("exact", circle(20.0, 50.0, 6.35)),
            ("near", circle(50.0, 50.0, 6.345)),
            ("under", circle(80.0, 50.0, 6.3)),
        ]);
        p.tools[0].diameter_mm = 6.35;
        assert_approx_eq!(p.flatten_tolerance_mm, 0.01, 1e-12);
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert_xs(&hole_xs(&drilled.toolpath), &[20.0, 50.0]);
        assert_eq!(codes(&warnings), [WarningCode::DrillHoleTooSmall]);
        assert_eq!(shape_of(&warnings[0]).0, "under");
    }

    /// With `allow_tool_wider_than_hole` the 5 mm hole is drilled at the
    /// tool's size, warning as before the decision: the hole comes out
    /// the tool's size.
    #[test]
    fn allow_tool_wider_than_hole_drills_and_warns() {
        let mut p = circles(&[(50.0, 50.0)]);
        p.tools[0].diameter_mm = 6.35;
        let mut op = drill_op(&p);
        op.allow_tool_wider_than_hole = true;
        let (drilled, warnings) = generate(&p, &op);
        assert_eq!(plunges(&drilled.toolpath).len(), 1);
        assert_eq!(codes(&warnings), [WarningCode::DrillToolWiderThanHole]);
        let Warning::DrillToolWiderThanHole {
            hole_mm, tool_mm, ..
        } = warnings[0]
        else {
            panic!("{:?}", warnings[0]);
        };
        assert_approx_eq!(hole_mm, 5.0, 1e-6);
        assert_approx_eq!(tool_mm, 6.35, 1e-9);
        assert_eq!(shape_of(&warnings[0]).0, "c1");
    }

    /// Every hole skipped: no toolpath motion, a warning per hole, and
    /// one saying the operation drills nothing.
    #[test]
    fn every_hole_skipped_warns_no_holes() {
        let mut p = circles(&[(30.0, 50.0), (60.0, 50.0)]);
        p.tools[0].diameter_mm = 6.35;
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert!(drilled.toolpath.moves.is_empty(), "{:?}", drilled.toolpath);
        assert!(drilled.holes.is_empty());
        assert_eq!(
            codes(&warnings),
            [
                WarningCode::DrillHoleTooSmall,
                WarningCode::DrillHoleTooSmall,
                WarningCode::DrillNoHoles,
            ]
        );
    }

    /// Port of `drillingRetract.test.ts` "retract height above the
    /// clearance plane is still capped at safe Z": no warning, and every
    /// hole's retract plane is safe Z.
    #[test]
    fn retract_above_safe_z_is_clamped() {
        let p = circles(&[(10.0, 10.0), (30.0, 10.0)]);
        let mut op = drill_op(&p);
        op.retract_mm = 50.0;
        op.cycle = peck(Some(4.0), PeckRetract::Full);
        let (drilled, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(drilled.holes.len(), 2);
        for hole in &drilled.holes {
            assert_approx_eq!(hole.retract_z_mm, SAFE_Z_MM, TOL);
        }
        let max_z = drilled
            .toolpath
            .moves
            .iter()
            .map(|m| m.to.z)
            .fold(f64::MIN, f64::max);
        assert_approx_eq!(max_z, SAFE_Z_MM, TOL);
    }

    /// Port of `drillingRetract.test.ts` "a negative retract distance is
    /// raised to the surface and warned" (and the peck variant): the
    /// retract plane is the stock top, the warning reports the distance as
    /// entered, and every rapid still obeys S15. Validation rejects a
    /// negative `retract_mm`; the generator does not trust it.
    #[test]
    fn retract_below_surface_is_raised_and_warned() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.retract_mm = -2.0;
        op.cycle = peck(Some(4.0), PeckRetract::Full);
        let (drilled, warnings) = generate(&p, &op);
        assert_approx_eq!(drilled.holes[0].retract_z_mm, THICKNESS_MM, TOL);
        assert_eq!(codes(&warnings), [WarningCode::DrillRetractBelowStockTop]);
        let Warning::DrillRetractBelowStockTop {
            requested_mm,
            used_mm,
            ..
        } = warnings[0]
        else {
            panic!("{:?}", warnings[0]);
        };
        assert_approx_eq!(requested_mm, -2.0, TOL);
        assert_approx_eq!(used_mm, 0.0, TOL);
        assert_retract_safe(&drilled.toolpath, "negative retract");
        let below_surface = drilled
            .toolpath
            .moves
            .iter()
            .filter(|m| m.is_rapid() && m.to.z < THICKNESS_MM - EPS_MM)
            .count();
        assert!(below_surface > 0, "pecks still re-enter below the surface");
    }

    /// Port of `drillingRetract.test.ts` `assertRetractSafe` (S15): every
    /// rapid end below the stock top is at an XY a cutting move has
    /// already opened to at least that depth. Cutting moves count both
    /// their ends.
    fn assert_retract_safe(toolpath: &Toolpath, label: &str) {
        let mut opened: Vec<(Point, f64)> = Vec::new();
        let mut from: Option<Point3> = None;
        for (i, m) in toolpath.moves.iter().enumerate() {
            let ends = [from, Some(m.to)];
            if m.is_cut() {
                for p in ends.into_iter().flatten() {
                    match opened
                        .iter_mut()
                        .find(|(at, _)| at.distance_to(p.xy()) <= EPS_MM)
                    {
                        Some((_, z)) => *z = z.min(p.z),
                        None => opened.push((p.xy(), p.z)),
                    }
                }
            } else if m.is_rapid() {
                for p in ends.into_iter().flatten() {
                    if p.z >= THICKNESS_MM - EPS_MM {
                        continue;
                    }
                    let cut = opened
                        .iter()
                        .find(|(at, _)| at.distance_to(p.xy()) <= EPS_MM)
                        .map(|(_, z)| *z);
                    assert!(
                        cut.is_some_and(|z| z <= p.z + EPS_MM),
                        "{label}: rapid {i} reaches {p:?} below the surface where no cut has opened (deepest {cut:?})"
                    );
                }
            }
            from = Some(m.to);
        }
    }

    /// S15 over every cycle, three holes each.
    #[test]
    fn rapids_descend_only_into_cut_depth() {
        let p = circles(&[(10.0, 10.0), (50.0, 10.0), (30.0, 10.0)]);
        let cycles = [
            ("simple", DrillCycle::Simple, 0.0, 1.0),
            ("dwell", DrillCycle::Simple, 0.5, 1.0),
            ("peck full", peck(Some(4.0), PeckRetract::Full), 0.0, 1.0),
            ("peck tiny", peck(Some(0.3), PeckRetract::Full), 0.0, 0.0),
            (
                "chip break",
                peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 0.5 }),
                0.0,
                1.0,
            ),
            (
                "chip break, gap past retract",
                peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 30.0 }),
                0.0,
                1.0,
            ),
            (
                "peck, negative retract",
                peck(Some(4.0), PeckRetract::Full),
                0.2,
                -2.0,
            ),
        ];
        for (label, cycle, dwell_s, retract_mm) in cycles {
            let mut op = drill_op(&p);
            op.cycle = cycle;
            op.dwell_s = dwell_s;
            op.retract_mm = retract_mm;
            let (drilled, _) = generate(&p, &op);
            assert_eq!(drilled.holes.len(), 3, "{label}");
            assert_retract_safe(&drilled.toolpath, label);
        }
    }

    /// Design spec, "Algorithms → Drill" step 6: the canned cycle each
    /// combination is equivalent to.
    #[test]
    fn holes_record_canned_kind() {
        let p = circles(&[(50.0, 50.0)]);
        let cases = [
            (DrillCycle::Simple, 0.0, CannedKind::G81, None, None),
            (DrillCycle::Simple, 0.5, CannedKind::G82, None, Some(0.5)),
            (
                peck(Some(4.0), PeckRetract::Full),
                0.0,
                CannedKind::G83,
                Some(4.0),
                None,
            ),
            (
                peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 0.5 }),
                0.0,
                CannedKind::G73,
                Some(4.0),
                None,
            ),
            (
                peck(Some(4.0), PeckRetract::Full),
                0.5,
                CannedKind::Expanded,
                Some(4.0),
                Some(0.5),
            ),
            (
                peck(None, PeckRetract::ChipBreak { gap_mm: 0.5 }),
                1.0,
                CannedKind::Expanded,
                Some(4.0),
                Some(1.0),
            ),
        ];
        for (cycle, dwell_s, kind, peck_mm, dwell) in cases {
            let mut op = drill_op(&p);
            op.cycle = cycle.clone();
            op.dwell_s = dwell_s;
            let (drilled, _) = generate(&p, &op);
            let hole = drilled.holes[0];
            assert_eq!(hole.cycle, kind, "{cycle:?} dwell {dwell_s}");
            assert_eq!(hole.peck_mm, peck_mm, "{cycle:?} dwell {dwell_s}");
            assert_eq!(hole.dwell_s, dwell, "{cycle:?} dwell {dwell_s}");
            assert_approx_eq!(hole.top_z_mm, THICKNESS_MM, TOL);
            assert_approx_eq!(hole.bottom_z_mm, 0.0, TOL);
            assert_approx_eq!(hole.retract_z_mm, RETRACT_Z_MM, TOL);
        }
    }

    /// S1, S15: the depth may pass the stock bottom only by the
    /// operation's own through-cut allowance.
    #[test]
    fn depth_beyond_bottom_needs_allowance() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.depth_mm = Some(12.2);
        let err = generate_drill(&p, &op).unwrap_err();
        assert!(
            matches!(err, CamError::BelowMaterialBottom { .. }),
            "{err:?}"
        );
        op.through_cut_allowance_mm = 0.2;
        let (drilled, _) = generate(&p, &op);
        let bottom = plunges(&drilled.toolpath)[0].to.z;
        assert_approx_eq!(bottom, -0.2, TOL);
        assert_approx_eq!(drilled.holes[0].bottom_z_mm, -0.2, TOL);
    }

    /// The hole records follow the drilled order, and every link between
    /// holes is a rapid at safe Z.
    #[test]
    fn holes_are_in_cut_order_and_links_are_at_safe_z() {
        let p = circles(&[(10.0, 10.0), (50.0, 10.0), (30.0, 10.0)]);
        let (drilled, _) = generate(&p, &drill_op(&p));
        let xs: Vec<f64> = drilled.holes.iter().map(|h| h.centre.x).collect();
        assert_xs(&xs, &[10.0, 30.0, 50.0]);
        let moves = &drilled.toolpath.moves;
        for (i, pair) in moves.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            if a.to.xy().distance_to(b.to.xy()) > EPS_MM {
                assert!(b.is_rapid(), "move {} changes XY: {b:?}", i + 1);
                assert_approx_eq!(a.to.z, SAFE_Z_MM, TOL);
                assert_approx_eq!(b.to.z, SAFE_Z_MM, TOL);
            }
        }
    }

    /// A chip-break gap that is not positive, or a negative dwell, cannot
    /// be drilled (validation reports both first).
    #[test]
    fn invalid_gap_or_dwell_is_error() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
        op.cycle = peck(Some(4.0), PeckRetract::ChipBreak { gap_mm: 0.0 });
        let err = generate_drill(&p, &op).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::InvalidDrillSetting {
                    field: "gap_mm",
                    ..
                }
            ),
            "{err:?}"
        );
        op.cycle = DrillCycle::Simple;
        op.dwell_s = -1.0;
        let err = generate_drill(&p, &op).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::InvalidDrillSetting {
                    field: "dwell_s",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    /// A Drill needs the tool's plunge rate, never an invented one.
    #[test]
    fn missing_plunge_rate_is_error() {
        let mut p = circles(&[(50.0, 50.0)]);
        p.tools[0].plunge_mm_min = None;
        let err = generate_drill(&p, &drill_op(&p)).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::MissingFeed {
                    field: crate::error::FeedField::Plunge,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    // Countersink (format 21; `docs/specs/phase-3/design.md`, "Algorithms
    // → Countersink"; functional spec capability 44, acceptance 55). The
    // ported tests are from `drillingCountersink.test.ts`.

    /// The 1/8 in holes of acceptance 55.
    const HOLE_MM: f64 = 3.175;

    /// A V-bit `t1` of `angle_deg`, `diameter_mm` wide, rates as the drill.
    fn v_bit(angle_deg: f64, diameter_mm: f64) -> Tool {
        Tool {
            name: format!("{angle_deg}° V-bit"),
            cutter: ToolKind::VBit {
                included_angle_deg: angle_deg,
            },
            diameter_mm,
            step_down_mm: None,
            ..drill_tool()
        }
    }

    /// Circles of `diameter_mm` at each `(x, y)`, instance ids `h1`, `h2`,
    /// …, in the 12 mm stock, cut with `tool`.
    fn holes(centres: &[(f64, f64)], diameter_mm: f64, tool: Tool) -> Project {
        let named: Vec<(String, Polyline)> = centres
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| (format!("h{}", i + 1), circle(x, y, diameter_mm)))
            .collect();
        let shapes: Vec<(&str, Polyline)> = named
            .iter()
            .map(|(id, outline)| (id.as_str(), outline.clone()))
            .collect();
        let mut p = project(&shapes);
        p.tools = vec![tool];
        p
    }

    /// A countersink of every instance of `project`, `mouth_mm` wide,
    /// from the stock top, retract 1 mm, safe Z 5 mm: [`drill_op`] with
    /// the cycle and no `depth_mm`.
    fn countersink_op(project: &Project, mouth_mm: f64) -> DrillOp {
        DrillOp {
            cycle: DrillCycle::Countersink {
                diameter_mm: mouth_mm,
                start_depth_mm: 0.0,
                start_plane_cleared_externally: false,
            },
            depth_mm: None,
            ..drill_op(project)
        }
    }

    /// The countersink's start depth and its "cleared externally" flag.
    fn start_of(op: &mut DrillOp) -> (&mut f64, &mut bool) {
        match &mut op.cycle {
            DrillCycle::Countersink {
                start_depth_mm,
                start_plane_cleared_externally,
                ..
            } => (start_depth_mm, start_plane_cleared_externally),
            cycle => panic!("not a countersink: {cycle:?}"),
        }
    }

    /// `project` with a 40 × 40 mm square `sq` from (30, 30), not among
    /// the countersink's targets, and before every operation a Pocket
    /// `p0` of it `depth_mm` deep with a 6.35 mm end mill `t9`.
    fn with_pocket(mut project: Project, depth_mm: f64) -> Project {
        let definition = FeatureDefinition {
            id: DefinitionId::new("d_sq"),
            name: "Pocket".into(),
            profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0)],
        };
        project.instances.push(FeatureInstance {
            id: InstanceId::new("sq"),
            definition: definition.id.clone(),
            transform: Transform::translate(30.0, 30.0),
            name: "Pocket".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
        project
            .definitions
            .insert(definition.id.clone(), definition);
        project.tools.push(Tool {
            id: ToolId::new("t9"),
            ..dc_project::test_support::tool_6_35_upcut()
        });
        project.operations.insert(
            0,
            dc_project::Operation::Pocket(dc_project::PocketOp {
                id: OperationId::new("p0"),
                targets: vec![InstanceId::new("sq")],
                tool: ToolId::new("t9"),
                depth_mm: Some(depth_mm),
                ..dc_project::test_support::sample_pocket()
            }),
        );
        project
    }

    /// Generates countersink `op` as the last operation of `project`, as
    /// a project holds it (the start plane is found from the operations
    /// before it), and checks the result with [`assert_safe`] and S15.
    fn countersink(project: &Project, op: &DrillOp) -> (DrillToolpath, Vec<Warning>) {
        let placed = placed(project, op);
        let (drilled, warnings) = generate(&placed, op);
        assert_retract_safe(&drilled.toolpath, "countersink");
        (drilled, warnings)
    }

    /// `project` with `op` appended to its operations.
    fn placed(project: &Project, op: &DrillOp) -> Project {
        let mut placed = project.clone();
        placed
            .operations
            .push(dc_project::Operation::Drill(op.clone()));
        placed
    }

    /// The countersink's error for `op` as the last operation of
    /// `project`; no toolpath is returned.
    fn refused(project: &Project, op: &DrillOp) -> CamError {
        match generate_drill(&placed(project, op), op) {
            Ok((drilled, warnings)) => panic!("cut: {drilled:?} {warnings:?}"),
            Err(err) => err,
        }
    }

    /// Port of "90° V-bit: a 6 mm mouth plunges 3 mm" and "82° V-bit:
    /// depth follows D / (2·tan(θ/2)), not the 90° shortcut", with
    /// acceptance 55's numbers: a 6 mm mouth is 3 mm below the stock top
    /// with a 90° bit, 3.451 mm with an 82° one.
    #[test]
    fn countersink_depth_is_mouth_over_two_tan_half_angle() {
        for (angle_deg, depth_mm) in [(90.0, 3.0), (82.0, 6.0 / (2.0 * 41f64.to_radians().tan()))] {
            let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(angle_deg, 6.35));
            let (drilled, warnings) = countersink(&p, &countersink_op(&p, 6.0));
            assert!(warnings.is_empty(), "{angle_deg}: {warnings:?}");
            let cut = plunges(&drilled.toolpath);
            assert_eq!(cut.len(), 1, "{angle_deg}: {cut:?}");
            assert_approx_eq!(cut[0].to.z, THICKNESS_MM - depth_mm, 1e-9);
        }
        assert_approx_eq!(6.0 / (2.0 * 41f64.to_radians().tan()), 3.451, 5e-4);
    }

    /// Port of "depth is measured from each target's own top face",
    /// restated for DreamCarve's start plane (capability 44; acceptance
    /// 55): on the floor of a 4 mm Pocket that runs before it, with
    /// `start_depth_mm: 4`, a 6 mm mouth plunges to 7 mm below the stock
    /// top (Z 5 in the 12 mm stock), its mouth at the floor.
    #[test]
    fn countersink_depth_is_measured_from_the_start_plane() {
        let p = with_pocket(
            holes(&[(40.0, 40.0), (60.0, 60.0)], HOLE_MM, v_bit(90.0, 6.35)),
            4.0,
        );
        let mut op = countersink_op(&p, 6.0);
        op.targets = vec![InstanceId::new("h1"), InstanceId::new("h2")];
        *start_of(&mut op).0 = 4.0;
        let (drilled, warnings) = countersink(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let zs: Vec<f64> = plunges(&drilled.toolpath).iter().map(|m| m.to.z).collect();
        assert_xs(&zs, &[THICKNESS_MM - 7.0, THICKNESS_MM - 7.0]);
        for hole in &drilled.holes {
            assert_approx_eq!(hole.top_z_mm, THICKNESS_MM - 4.0, TOL);
            assert_approx_eq!(hole.bottom_z_mm, THICKNESS_MM - 7.0, TOL);
        }
    }

    /// Port of "motion is centre-plunge and retract: rapid in at safe Z,
    /// feed down, rapid out": in at safe Z, a rapid down to the retract
    /// plane, one plunge at the plunge rate at the centre, a rapid to safe
    /// Z, and no rapid below the stock top.
    #[test]
    fn countersink_is_a_centre_plunge_and_retract() {
        let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
        let (drilled, _) = countersink(&p, &countersink_op(&p, 6.0));
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', THICKNESS_MM - 3.0),
                ('R', SAFE_Z_MM),
            ],
        );
        let cut = plunges(&drilled.toolpath);
        assert_eq!(cut[0].feed_mm_min, Some(PLUNGE_MM_MIN));
        assert_approx_eq!(cut[0].to.x, 20.0, TOL);
        assert_approx_eq!(cut[0].to.y, 20.0, TOL);
        for m in drilled.toolpath.moves.iter().filter(|m| m.is_rapid()) {
            assert!(
                m.to.z >= THICKNESS_MM - EPS_MM,
                "rapid into the stock: {m:?}"
            );
        }
    }

    /// Port of "no V-bit assigned: a drill bit produces no countersink":
    /// the depth follows a V-bit's angle, so any other cutter is refused
    /// and nothing is cut.
    #[test]
    fn countersink_needs_a_v_bit() {
        for cutter in [ToolKind::Drill, ToolKind::EndMill, ToolKind::BallNose] {
            let mut p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
            p.tools[0].cutter = cutter.clone();
            let err = refused(&p, &countersink_op(&p, 6.0));
            assert!(
                matches!(&err, CamError::CountersinkNeedsVBit { op, cutter: c, .. }
                    if op.as_str() == "o1" && *c == cutter.describe()),
                "{err:?}"
            );
        }
    }

    /// Port of "unusable V-bit angle produces no countersink".
    #[test]
    fn countersink_rejects_an_unusable_angle() {
        for angle_deg in [0.0, 180.0, -10.0, f64::NAN] {
            let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(angle_deg, 6.35));
            let err = refused(&p, &countersink_op(&p, 6.0));
            assert!(
                matches!(&err, CamError::VBitAngleRange { tool_id, .. } if tool_id.as_str() == "t1"),
                "{angle_deg}: {err:?}"
            );
        }
    }

    /// Port of "missing or zero diameter produces no countersink": the
    /// field cannot be missing from a file (every field is written), so
    /// zero, negative and not-a-number stand for it.
    #[test]
    fn countersink_rejects_a_missing_or_zero_diameter() {
        for mouth_mm in [0.0, -6.0, f64::NAN, f64::INFINITY] {
            let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
            let err = refused(&p, &countersink_op(&p, mouth_mm));
            assert!(
                matches!(&err, CamError::CountersinkDiameterPositive { op, .. } if op.as_str() == "o1"),
                "{mouth_mm}: {err:?}"
            );
        }
    }

    /// Port of "mouth wider than the cutter produces no countersink": a
    /// 12 mm mouth with a 10 mm bit is refused; a mouth exactly the bit's
    /// diameter cuts (acceptance 55: `diameter_mm: 7` with the 6.35 mm
    /// bit is refused).
    #[test]
    fn countersink_mouth_wider_than_the_cutter_is_refused() {
        let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 10.0));
        let err = refused(&p, &countersink_op(&p, 12.0));
        assert!(
            matches!(err, CamError::CountersinkMouthExceedsTool { mouth_mm, tool_mm, .. }
                if mouth_mm == 12.0 && tool_mm == 10.0),
            "{err:?}"
        );
        let (drilled, _) = countersink(&p, &countersink_op(&p, 10.0));
        assert_eq!(plunges(&drilled.toolpath).len(), 1);
        let err = refused(&p, &countersink_op(&p, 10.0 + 1e-6));
        assert!(
            matches!(err, CamError::CountersinkMouthExceedsTool { .. }),
            "{err:?}"
        );
    }

    /// Port of "a tool limited to exactly its cone height still cuts a
    /// full-diameter countersink", for DreamCarve's cone-height check
    /// (tools have no max cut depth): a 12 mm 90° bit's cone is 6 mm, and
    /// its full 12 mm mouth derives 6 mm a ulp high in floating point.
    /// It cuts, to 6 mm within 1e-9; so does a 6 mm mouth 3 mm down a
    /// start plane vouched for, whose engagement is the whole cone. A
    /// tenth of a millimetre more is refused.
    #[test]
    fn full_diameter_countersink_is_cut_within_one_ulp() {
        let derived_mm = 12.0 / (2.0 * std::f64::consts::FRAC_PI_4.tan());
        assert!(
            derived_mm > 6.0,
            "the derivation must land above 6: {derived_mm}"
        );
        let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 12.0));
        let (drilled, _) = countersink(&p, &countersink_op(&p, 12.0));
        let cut = plunges(&drilled.toolpath);
        assert_eq!(cut.len(), 1);
        assert_approx_eq!(cut[0].to.z, THICKNESS_MM - 6.0, 1e-9);

        let mut op = countersink_op(&p, 6.0);
        let (start_depth_mm, cleared_externally) = start_of(&mut op);
        *start_depth_mm = 3.0;
        *cleared_externally = true;
        let (drilled, _) = countersink(&p, &op);
        assert_approx_eq!(plunges(&drilled.toolpath)[0].to.z, THICKNESS_MM - 6.0, 1e-9);

        *start_of(&mut op).0 = 3.1;
        let err = refused(&p, &op);
        assert!(
            matches!(err, CamError::CountersinkConeExceeded { engaged_mm, cone_mm, .. }
                if (engaged_mm - 6.1).abs() < 1e-9 && (cone_mm - 6.0).abs() < 1e-9),
            "{err:?}"
        );
    }

    /// Port of "mouth no wider than the hole skips that target and leaves
    /// the others cut": an 8 mm hole, and a 6 mm one, cannot seat a 6 mm
    /// mouth, so each is skipped with a warning naming it and both sizes,
    /// and nothing descends at their centres; the 3.175 mm hole is cut.
    /// With every hole skipped the operation cuts nothing (acceptance 55:
    /// a 3 mm mouth skips every 3.175 mm hole).
    #[test]
    fn mouth_no_wider_than_the_hole_skips_that_target() {
        let shapes = [
            ("h1", circle(20.0, 20.0, 8.0)),
            ("h2", circle(60.0, 20.0, HOLE_MM)),
            ("h3", circle(100.0, 20.0, 6.0)),
        ];
        let mut p = project(&shapes);
        p.tools = vec![v_bit(90.0, 6.35)];
        let (drilled, warnings) = countersink(&p, &countersink_op(&p, 6.0));
        let cut = plunges(&drilled.toolpath);
        assert_eq!(cut.len(), 1, "{cut:?}");
        assert_approx_eq!(cut[0].to.x, 60.0, TOL);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::CountersinkMouthNotWiderThanHole; 2]
        );
        let skipped: Vec<(&str, f64, f64)> = warnings
            .iter()
            .filter_map(|w| match w {
                Warning::CountersinkMouthNotWiderThanHole {
                    shape,
                    mouth_mm,
                    hole_mm,
                    ..
                } => Some((shape.instance.as_str(), *mouth_mm, *hole_mm)),
                _ => None,
            })
            .collect();
        assert_eq!(skipped.len(), 2);
        assert_eq!((skipped[0].0, skipped[1].0), ("h1", "h3"));
        assert_approx_eq!(skipped[0].1, 6.0, TOL);
        assert_approx_eq!(skipped[0].2, 8.0, 1e-6);
        assert_approx_eq!(skipped[1].2, 6.0, 1e-6);
        assert!(
            !drilled
                .toolpath
                .moves
                .iter()
                .any(|m| { (m.to.x - 20.0).abs() < TOL || (m.to.x - 100.0).abs() < TOL }),
            "motion at a skipped hole"
        );

        let p = holes(&[(20.0, 20.0), (40.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
        let (drilled, warnings) = countersink(&p, &countersink_op(&p, 3.0));
        assert!(drilled.toolpath.moves.is_empty(), "{:?}", drilled.toolpath);
        assert!(drilled.holes.is_empty());
        assert_eq!(
            codes(&warnings),
            vec![
                WarningCode::CountersinkMouthNotWiderThanHole,
                WarningCode::CountersinkMouthNotWiderThanHole,
                WarningCode::DrillNoHoles,
            ]
        );
    }

    /// Port of "non-circular targets are skipped, not countersunk".
    #[test]
    fn non_circular_targets_are_skipped() {
        let shapes = [
            ("h1", circle(20.0, 20.0, HOLE_MM)),
            ("r1", Polyline::rect(Point::new(60.0, 10.0), 20.0, 20.0)),
        ];
        let mut p = project(&shapes);
        p.tools = vec![v_bit(90.0, 6.35)];
        let (drilled, warnings) = countersink(&p, &countersink_op(&p, 6.0));
        assert_eq!(plunges(&drilled.toolpath).len(), 1);
        assert_eq!(codes(&warnings), vec![WarningCode::DrillTargetNotCircle]);
        assert_eq!(shape_of(&warnings[0]), ("r1", 0));
    }

    /// Port of "countersink posts as linear motion even on a canned-cycle
    /// machine", the generator half (design step 4; S18): every hole is
    /// recorded, in cut order, as [`CannedKind::Expanded`], which the
    /// emitter always writes as its moves, so no `G8x` is written
    /// whatever `use_canned_cycles` says. The same holes drilled simple
    /// record `G81`, so the fixture would bite.
    #[test]
    fn countersink_records_no_canned_cycle() {
        let p = holes(&[(20.0, 20.0), (60.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
        for dwell_s in [0.0, 0.5] {
            let mut op = countersink_op(&p, 6.0);
            op.dwell_s = dwell_s;
            let (drilled, _) = countersink(&p, &op);
            assert_eq!(drilled.holes.len(), 2);
            for hole in &drilled.holes {
                assert_eq!(hole.cycle, CannedKind::Expanded, "{hole:?}");
                assert_eq!(hole.peck_mm, None);
                assert_approx_eq!(hole.top_z_mm, THICKNESS_MM, TOL);
                assert_approx_eq!(hole.bottom_z_mm, THICKNESS_MM - 3.0, TOL);
                assert_approx_eq!(hole.retract_z_mm, RETRACT_Z_MM, TOL);
            }
            assert_xs(
                &drilled.holes.iter().map(|h| h.centre.x).collect::<Vec<_>>(),
                &hole_xs(&drilled.toolpath),
            );
        }
        let mut p = p;
        p.tools = vec![drill_tool()];
        let (drilled, _) = generate(&p, &drill_op(&p));
        assert!(drilled.holes.iter().all(|h| h.cycle == CannedKind::G81));
    }

    /// Port of "legacy drilling operations are untouched by the new
    /// field": the Rust enum has no stray countersink field to set, so
    /// the port is that a countersink in the project changes nothing a
    /// simple, peck or chip-break Drill of the same holes generates.
    #[test]
    fn other_cycles_are_unchanged_by_the_new_variant() {
        let p = circles(&[(10.0, 10.0), (50.0, 10.0)]);
        let mut with_countersink = p.clone();
        let mut bit = v_bit(90.0, 6.35);
        bit.id = ToolId::new("t2");
        with_countersink.tools.push(bit);
        let mut sink = countersink_op(&p, 6.0);
        sink.id = OperationId::new("o2");
        sink.tool = ToolId::new("t2");
        with_countersink
            .operations
            .push(dc_project::Operation::Drill(sink));
        for (cycle, dwell_s) in [
            (DrillCycle::Simple, 0.0),
            (DrillCycle::Simple, 0.5),
            (peck(Some(3.0), PeckRetract::Full), 0.0),
            (peck(Some(3.0), PeckRetract::ChipBreak { gap_mm: 0.5 }), 0.0),
        ] {
            let mut op = drill_op(&p);
            op.cycle = cycle.clone();
            op.dwell_s = dwell_s;
            let (alone, _) = generate(&p, &op);
            let (beside, _) = generate(&with_countersink, &op);
            assert_eq!(alone, beside, "{cycle:?}");
            assert_eq!(alone.holes.len(), 2, "{cycle:?}");
        }
    }

    /// Capability 44: `dwell_s` applies as for a simple cycle, a wait at
    /// the bottom of the plunge.
    #[test]
    fn countersink_dwell_is_at_the_bottom() {
        let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
        let mut op = countersink_op(&p, 6.0);
        op.dwell_s = 0.5;
        let (drilled, _) = countersink(&p, &op);
        let bottom = THICKNESS_MM - 3.0;
        assert_profile(
            &profile_of(&drilled.toolpath),
            &[
                ('R', SAFE_Z_MM),
                ('R', RETRACT_Z_MM),
                ('L', bottom),
                ('D', bottom),
                ('R', SAFE_Z_MM),
            ],
        );
        assert_eq!(
            drilled.toolpath.moves[3].kind,
            MoveKind::Dwell { seconds: 0.5 }
        );
        assert_eq!(drilled.holes[0].dwell_s, Some(0.5));
    }

    /// Security review H3: the retract plane is measured from the stock
    /// top, never the start plane, so with `start_depth_mm: 4` and
    /// `retract_mm: 1` the rapid down stops at the stock top plus 1 mm
    /// and the plunge covers the Pocket's air at the plunge rate. A
    /// retract above safe Z is capped there; a negative one is raised to
    /// the stock top with a warning.
    #[test]
    fn countersink_rapid_stops_above_the_stock_top_whatever_the_start_depth() {
        let p = with_pocket(holes(&[(50.0, 50.0)], HOLE_MM, v_bit(90.0, 6.35)), 4.0);
        let mut op = countersink_op(&p, 6.0);
        op.targets = vec![InstanceId::new("h1")];
        *start_of(&mut op).0 = 4.0;
        for (retract_mm, retract_z_mm, warns) in [
            (1.0, RETRACT_Z_MM, false),
            (20.0, SAFE_Z_MM, false),
            (-2.0, THICKNESS_MM, true),
        ] {
            op.retract_mm = retract_mm;
            let (drilled, warnings) = countersink(&p, &op);
            assert_profile(
                &profile_of(&drilled.toolpath),
                &[
                    ('R', SAFE_Z_MM),
                    ('R', retract_z_mm),
                    ('L', THICKNESS_MM - 7.0),
                    ('R', SAFE_Z_MM),
                ],
            );
            assert_approx_eq!(drilled.holes[0].retract_z_mm, retract_z_mm, TOL);
            assert_eq!(
                codes(&warnings),
                if warns {
                    vec![WarningCode::DrillRetractBelowStockTop]
                } else {
                    vec![]
                },
                "{retract_mm}"
            );
        }
    }

    /// Security review M2, H4: below the stock top with no enabled earlier
    /// Pocket holding the bit, and no word that the plane is cleared, the
    /// countersink is refused (validation says so first). Vouched for, the
    /// flanks engage from the stock top: 2 mm plus the 3 mm depth is past
    /// the 90° 6.35 mm bit's 3.175 mm cone. A Pocket at another depth
    /// does not clear the plane either; a disabled one does not count.
    #[test]
    fn uncleared_countersink_start_depth_is_refused() {
        let p = holes(&[(50.0, 50.0)], HOLE_MM, v_bit(90.0, 6.35));
        let mut op = countersink_op(&p, 6.0);
        *start_of(&mut op).0 = 2.0;
        let err = refused(&p, &op);
        assert!(
            matches!(&err, CamError::StartDepthUncleared { op, start_depth_mm }
                if op.as_str() == "o1" && *start_depth_mm == 2.0),
            "{err:?}"
        );

        *start_of(&mut op).1 = true;
        let err = refused(&p, &op);
        assert!(
            matches!(err, CamError::CountersinkConeExceeded { engaged_mm, cone_mm, .. }
                if (engaged_mm - 5.0).abs() < 1e-9 && (cone_mm - 3.175).abs() < 1e-9),
            "{err:?}"
        );
        // Vouched for, a start depth within what the cone has left cuts.
        *start_of(&mut op).0 = 0.1;
        let (drilled, _) = countersink(&p, &op);
        assert_approx_eq!(plunges(&drilled.toolpath)[0].to.z, THICKNESS_MM - 3.1, TOL);

        // A 1 mm Pocket holds the bit, but its floor is not the 2 mm plane.
        let shallower = with_pocket(p.clone(), 1.0);
        let mut op = countersink_op(&p, 6.0);
        *start_of(&mut op).0 = 2.0;
        let err = refused(&shallower, &op);
        assert!(
            matches!(err, CamError::CountersinkConeExceeded { .. }),
            "{err:?}"
        );
        // A 2 mm one is the plane: 3 mm of flank engaged, which fits.
        let (drilled, _) = countersink(&with_pocket(p.clone(), 2.0), &op);
        assert_approx_eq!(plunges(&drilled.toolpath)[0].to.z, THICKNESS_MM - 5.0, TOL);
        let mut disabled = with_pocket(p, 2.0);
        if let Some(dc_project::Operation::Pocket(pocket)) = disabled.operations.first_mut() {
            pocket.enabled = false;
        }
        let err = refused(&disabled, &op);
        assert!(
            matches!(err, CamError::StartDepthUncleared { .. }),
            "{err:?}"
        );
    }

    /// S1: the derived depth below the start plane may pass the stock
    /// bottom only by the through-cut allowance.
    #[test]
    fn countersink_below_the_stock_bottom_is_refused() {
        let mut p = with_pocket(holes(&[(50.0, 50.0)], HOLE_MM, v_bit(90.0, 12.0)), 10.0);
        p.stock.thickness_mm = Some(12.0);
        let mut op = countersink_op(&p, 6.0);
        op.targets = vec![InstanceId::new("h1")];
        *start_of(&mut op).0 = 10.0;
        let err = refused(&p, &op);
        assert!(
            matches!(err, CamError::BelowMaterialBottom { .. }),
            "{err:?}"
        );
        op.through_cut_allowance_mm = 1.0;
        let (drilled, _) = countersink(&p, &op);
        assert_approx_eq!(plunges(&drilled.toolpath)[0].to.z, -1.0, TOL);
    }

    /// Design spec, "Countersink" step 2 (spec sync 1): Drill's rule that
    /// skips a hole narrower than the tool does not apply, since a
    /// countersink's bit is wider than its hole by design; the 3.175 mm
    /// hole is cut with the 6.35 mm bit, with no warning, whatever
    /// `allow_tool_wider_than_hole` says.
    #[test]
    fn countersink_ignores_the_narrower_hole_rule() {
        let p = holes(&[(20.0, 20.0)], HOLE_MM, v_bit(90.0, 6.35));
        let op = countersink_op(&p, 6.0);
        assert!(!op.allow_tool_wider_than_hole);
        let (narrow, warnings) = countersink(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(plunges(&narrow.toolpath).len(), 1);
        let allowed = DrillOp {
            allow_tool_wider_than_hole: true,
            ..op
        };
        let (wide, warnings) = countersink(&p, &allowed);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(narrow, wide);
    }

    /// Acceptance 55, the generator half: four 3.175 mm holes, a 90°
    /// 6.35 mm bit and a 6 mm mouth plunge once each at the centre to 3 mm
    /// below the stock top at the plunge rate, with every hole recorded as
    /// plain moves (no `G81` with canned cycles on); `diameter_mm: 7` is
    /// refused, and `3` skips every hole.
    #[test]
    fn acceptance_55_countersinks_four_holes() {
        let centres = [(40.0, 40.0), (60.0, 40.0), (40.0, 60.0), (60.0, 60.0)];
        let p = holes(&centres, HOLE_MM, v_bit(90.0, 6.35));
        let (drilled, warnings) = countersink(&p, &countersink_op(&p, 6.0));
        assert!(warnings.is_empty(), "{warnings:?}");
        let cut = plunges(&drilled.toolpath);
        assert_eq!(cut.len(), 4);
        for m in &cut {
            assert_approx_eq!(m.to.z, THICKNESS_MM - 3.0, TOL);
            assert_eq!(m.feed_mm_min, Some(PLUNGE_MM_MIN));
        }
        assert_eq!(drilled.holes.len(), 4);
        assert!(
            drilled
                .holes
                .iter()
                .all(|h| h.cycle == CannedKind::Expanded)
        );

        let err = refused(&p, &countersink_op(&p, 7.0));
        assert!(
            matches!(err, CamError::CountersinkMouthExceedsTool { .. }),
            "{err:?}"
        );
        let (drilled, warnings) = countersink(&p, &countersink_op(&p, 3.0));
        assert!(drilled.toolpath.moves.is_empty());
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.code() == WarningCode::CountersinkMouthNotWiderThanHole)
                .count(),
            4
        );
    }
}
