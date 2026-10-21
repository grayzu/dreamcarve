// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts
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
//! written as plain moves (upstream loses it); holes are visited nearest
//! neighbour from the first circle in drawing order (D5), not from the
//! origin, by [`crate::sequence`]; there is no helical bore, countersink or
//! clamp check. Nothing here depends on Y-up or Y-down: a drill moves only
//! in Z at each centre, and distances are symmetric under the flip.

use crate::block::CutBlock;
use crate::error::{CamError, FeedField, ShapeRef};
use crate::passes::pass_z_mm;
use crate::sequence::{SequenceInput, plan, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::toolpath::{Move, Toolpath};
use crate::warning::{Warning, WarningCode};
use dc_geometry::{EPS_MM, Point, Point3, as_circle};
use dc_project::{DrillCycle, DrillOp, MachiningOrder, PeckRetract, Project, Tool};

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
/// [`WarningCode::DrillTargetNotCircle`] and is skipped, and a tool wider
/// than a hole warns [`WarningCode::DrillToolWiderThanHole`]. Each hole is
/// drilled from the stock top to `depth_mm` (S1 with the operation's
/// through-cut allowance):
///
/// - the tool arrives at safe Z and rapids down to the retract plane
///   `min(safe Z, top + retract_mm)` (a negative `retract_mm` is raised to
///   the top with [`WarningCode::DrillRetractBelowStockTop`]);
/// - it plunges at the tool's plunge rate, in one plunge for a simple
///   cycle or in pecks of the peck depth (the operation's, else the tool's
///   step-down; only the last is shorter), waiting `dwell_s` at the bottom
///   of every plunge as a [`crate::MoveKind::Dwell`] move;
/// - between pecks it rapids up to the retract plane and back down to
///   [`PECK_REENTRY_MM`] above the depth reached (full retract), or up by
///   the chip-break gap (capped at the retract plane), and feeds on from
///   there; every rapid down ends in air already cut (S15);
/// - it ends with a rapid to safe Z, where the `G98` return of a canned
///   cycle puts it.
///
/// Holes are visited by [`crate::sequence`]: nearest neighbour from the
/// first hole in drawing order, ties by drawing order, linked at safe Z.
/// A peck at least as deep as the hole drills it in one plunge with
/// [`WarningCode::DrillPeckDeeperThanHole`].
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
            field: FeedField::Plunge,
        })?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
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
    // Peck levels, shallowest first; a simple cycle is one level. The pass
    // rule is the one every generator uses: only the last is shorter.
    let step_mm = cycle.peck_mm.unwrap_or(depth_mm);
    let levels = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        step_mm,
        op.through_cut_allowance_mm,
    )?;
    let top_z_mm = thickness_mm;
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let mut warnings = Vec::new();
    if op.retract_mm < 0.0 {
        warnings.push(
            Warning::new(WarningCode::DrillRetractBelowStockTop)
                .with("op", &op.id)
                .with("requested_mm", op.retract_mm)
                .with("used_mm", 0.0),
        );
    }
    let retract_z_mm = (top_z_mm + op.retract_mm.max(0.0)).min(safe_z_mm);
    if let Some(peck_mm) = cycle.peck_mm
        && peck_mm >= depth_mm - EPS_MM
    {
        warnings.push(
            Warning::new(WarningCode::DrillPeckDeeperThanHole)
                .with("op", &op.id)
                .with("peck_mm", peck_mm)
                .with("depth_mm", depth_mm),
        );
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
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        // Holes have no cleared area, so no stay-down link is ever cut at
        // this rate; the plunge rate is the only rate a Drill uses.
        feed_mm_min: plunge_mm_min,
        plunge_mm_min,
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

/// The holes of `op`, in drawing order (target order, then profile order):
/// each circular closed profile, placed by its instance's transform, with
/// its centre. Other profiles warn and are skipped; a tool wider than a
/// hole warns.
fn resolve_holes(
    project: &Project,
    op: &DrillOp,
    tool_diameter_mm: f64,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Point)>, CamError> {
    let mut holes = Vec::new();
    for instance_id in &op.targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op.id.clone(),
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
            let shape_warning = |code| {
                Warning::new(code)
                    .with("op", &op.id)
                    .with("instance", &shape.instance)
                    .with("instance_name", &shape.instance_name)
                    .with("profile", shape.profile)
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
            let Some((centre, radius_mm)) = circle else {
                warnings.push(shape_warning(WarningCode::DrillTargetNotCircle));
                continue;
            };
            let hole_mm = 2.0 * radius_mm;
            if tool_diameter_mm > hole_mm + EPS_MM {
                warnings.push(
                    shape_warning(WarningCode::DrillToolWiderThanHole)
                        .with("hole_mm", hole_mm)
                        .with("tool_mm", tool_diameter_mm),
                );
            }
            holes.push((shape, centre));
        }
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
        warnings.iter().map(|w| w.code).collect()
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
                ('L', 6.0),
                ('D', 6.0),
                ('R', RETRACT_Z_MM),
                ('R', 6.0 + PECK_REENTRY_MM),
                ('L', 0.0),
                ('D', 0.0),
                ('R', SAFE_Z_MM),
            ],
        );
    }

    /// Port of `camOperationSmoke.test.ts` "drilling peck: multiple plunges
    /// with full retracts", with acceptance 23's numbers: 12 mm in 4 mm
    /// pecks is three plunges, each but the last retracting to the retract
    /// plane above the hole top and re-entering 0.5 mm above the depth
    /// reached.
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
                ('L', 8.0),
                ('R', RETRACT_Z_MM),
                ('R', 8.5),
                ('L', 4.0),
                ('R', RETRACT_Z_MM),
                ('R', 4.5),
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
                ('L', 8.0),
                ('R', 8.3),
                ('L', 4.0),
                ('R', 4.3),
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
        assert_xs(&zs, &[8.0, 4.0, 0.0]);

        p.tools[0].step_down_mm = None;
        let err = generate_drill(&p, &op).unwrap_err();
        assert!(
            matches!(err, CamError::MissingStepDown { ref op, .. } if op.as_str() == "o1"),
            "{err:?}"
        );
    }

    /// 12 mm in 5 mm pecks: 5, 5, then 2. Only the last peck is short.
    #[test]
    fn last_peck_is_shallower_never_deeper() {
        let p = circles(&[(50.0, 50.0)]);
        let mut op = drill_op(&p);
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
        assert_eq!(warnings[0].param("op"), Some("o1"));
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
        assert_eq!(warnings[0].param("instance"), Some("r"));
        assert_eq!(warnings[0].param("profile"), Some("0"));
        assert_eq!(warnings[1].param("instance"), Some("open"));
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
        assert_eq!(warnings[0].param("instance"), Some("slot"));
    }

    /// A tool wider than the hole still drills it, with a warning giving
    /// both sizes: the hole comes out the tool's size.
    #[test]
    fn tool_wider_than_hole_warns() {
        let mut p = circles(&[(50.0, 50.0)]);
        p.tools[0].diameter_mm = 6.35;
        let (drilled, warnings) = generate(&p, &drill_op(&p));
        assert_eq!(plunges(&drilled.toolpath).len(), 1);
        assert_eq!(codes(&warnings), [WarningCode::DrillToolWiderThanHole]);
        let number = |key: &str| -> f64 { warnings[0].param(key).unwrap().parse().unwrap() };
        assert_approx_eq!(number("hole_mm"), 5.0, 1e-6);
        assert_approx_eq!(number("tool_mm"), 6.35, 1e-9);
        assert_eq!(warnings[0].param("instance"), Some("c1"));
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
        let number = |key: &str| -> f64 { warnings[0].param(key).unwrap().parse().unwrap() };
        assert_approx_eq!(number("requested_mm"), -2.0, TOL);
        assert_approx_eq!(number("used_mm"), 0.0, TOL);
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
}
