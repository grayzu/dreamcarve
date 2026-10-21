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
//! [`crate::sequence`] writes the links, so consecutive passes of a closed
//! path, and of an open path under `alternate_direction`, plunge straight
//! from the previous pass (functional spec, capability 15; decision D9).
//! Depth is measured from the stock top and bounded by S1 with the
//! allowance, where upstream clamps to absolute 0. Nothing here depends on
//! Y-up or Y-down: the tool follows the line as drawn.

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::passes::pass_z_mm;
use crate::profile::{containment_pairs, rates_of};
use crate::sequence::{SequenceInput, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::segment_kind;
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
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
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

    let mut warnings = Vec::new();
    let paths = resolve_targets(project, op, &mut warnings)?;
    let containment = closed_containment(&paths);

    let mut blocks = Vec::with_capacity(paths.len() * pass_z.len());
    for (shape, path) in &paths {
        let closed = path.is_closed();
        let backwards = path.reversed();
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            let reverse = !closed && op.alternate_direction && pass % 2 == 1;
            let traced = if reverse { &backwards } else { path };
            let Some(entry) = traced.vertices().first().map(|v| v.point()) else {
                continue;
            };
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
            let descent_from_z_mm = pass
                .checked_sub(1)
                .filter(|_| stays_down)
                .and_then(|k| pass_z.get(k).copied())
                .unwrap_or(safe_z_mm);
            blocks.push(CutBlock {
                shape: shape.clone(),
                pass,
                z_mm,
                descent_from: entry,
                descent_from_z_mm,
                descent: Vec::new(),
                entry,
                cuts,
                exit,
                frees: closed && pass + 1 == pass_z.len() && z_mm <= EPS_MM,
                cleared: None,
                group: 0,
            });
        }
    }
    let input = SequenceInput {
        blocks,
        order: op.machining_order,
        containment: &containment,
        safe_z_mm,
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
            op: op.id.clone(),
            overhang,
        });
    }
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
            let placed = instance
                .transform
                .apply_polyline(polyline)
                .map_err(|source| CamError::InvalidTarget {
                    shape: shape.clone(),
                    source,
                })?;
            if !has_two_distinct_points(&placed) {
                warnings.push(Warning::EngravePathDegenerate {
                    op: op.id.clone(),
                    shape,
                });
                continue;
            }
            paths.push((shape, placed));
        }
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
/// order by, so none is applied.
fn closed_containment(paths: &[(ShapeRef, Polyline)]) -> Vec<(ShapeRef, ShapeRef)> {
    let closed: Vec<(ShapeRef, Contour)> = paths
        .iter()
        .filter(|(_, p)| p.is_closed())
        .filter_map(|(shape, p)| Contour::new(p.clone()).ok().map(|c| (shape.clone(), c)))
        .collect();
    let contours: Vec<Contour> = closed.iter().map(|(_, c)| c.clone()).collect();
    match nest(contours) {
        Ok((regions, _)) => containment_pairs(&regions, &closed),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FeedField;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point, Polyline, Transform, Vertex, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, MachiningOrder, Operation,
        OperationId, Stock, ToolId, ToolKind,
    };
    use std::collections::BTreeMap;

    const TOL: f64 = 1e-6;
    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    /// 1 mm deep in 0.5 mm steps on 6 mm stock.
    const PASS_Z_MM: [f64; 2] = [5.5, 5.0];

    /// An open L: (0, 0) → (10, 0) → (10, 10).
    fn open_l() -> Polyline {
        Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(10.0, 0.0),
                Vertex::line(10.0, 10.0),
            ],
            false,
        )
    }

    fn rect_100x50() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)
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

    fn at_20_20() -> Transform {
        Transform::translate(20.0, 20.0)
    }

    fn l_project() -> Project {
        project(vec![open_l()], &[("i1", at_20_20())])
    }

    fn rect_project() -> Project {
        project(vec![rect_100x50()], &[("i1", at_20_20())])
    }

    /// Engrave of every instance in `project`, 1 mm deep in `step_down_mm`
    /// steps, full depth first, no alternation.
    fn engrave_op(project: &Project, step_down_mm: f64) -> EngraveOp {
        EngraveOp {
            id: OperationId::new("o1"),
            name: "Engrave".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            depth_mm: Some(1.0),
            step_down_mm: Some(step_down_mm),
            alternate_direction: false,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &EngraveOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_engrave(project, op).unwrap();
        assert_safe(&toolpath, project, &Operation::Engrave(op.clone()));
        (toolpath, warnings)
    }

    /// One run of XY cutting moves at one Z.
    #[derive(Debug)]
    struct Lap {
        z_mm: f64,
        /// Start point, then the end of every move.
        points: Vec<Point>,
        /// Index in the toolpath of the lap's first and last move.
        first: usize,
        last: usize,
    }

    impl Lap {
        fn x_extent_mm(&self) -> f64 {
            let xs = self.points.iter().map(|p| p.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        }
    }

    /// Splits the toolpath into laps: runs of XY cutting moves at one Z.
    fn laps(toolpath: &Toolpath) -> Vec<Lap> {
        let mut laps: Vec<Lap> = Vec::new();
        let mut current: Option<Lap> = None;
        let mut pos = match toolpath.moves.first() {
            Some(m) => m.to,
            None => return laps,
        };
        for (index, mv) in toolpath.moves.iter().enumerate() {
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
            if !mv.is_rapid() && level && xy {
                let lap = current.get_or_insert_with(|| Lap {
                    z_mm: mv.to.z,
                    points: vec![pos.xy()],
                    first: index,
                    last: index,
                });
                lap.points.push(mv.to.xy());
                lap.last = index;
            } else if let Some(lap) = current.take() {
                laps.push(lap);
            }
            pos = mv.to;
        }
        laps.extend(current);
        laps
    }

    fn assert_points(actual: &[Point], expected: &[(f64, f64)]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, &(x, y)) in actual.iter().zip(expected) {
            assert_approx_eq!(a.x, x, TOL);
            assert_approx_eq!(a.y, y, TOL);
        }
    }

    /// The Z of each non-rapid move, with repeats collapsed.
    fn cut_z_transitions(toolpath: &Toolpath) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for mv in toolpath.moves.iter().filter(|m| !m.is_rapid()) {
            if out.last().is_none_or(|z| (z - mv.to.z).abs() > EPS_MM) {
                out.push(mv.to.z);
            }
        }
        out
    }

    fn assert_z(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() <= EPS_MM, "{actual:?} vs {expected:?}");
        }
    }

    fn cuts(toolpath: &Toolpath) -> Vec<Move> {
        toolpath
            .moves
            .iter()
            .copied()
            .filter(|m| !m.is_rapid())
            .collect()
    }

    const L_FORWARD: [(f64, f64); 3] = [(20.0, 20.0), (30.0, 20.0), (30.0, 30.0)];
    const L_BACKWARD: [(f64, f64); 3] = [(30.0, 30.0), (30.0, 20.0), (20.0, 20.0)];

    /// Spec step 1: the tool centre follows the line as drawn, so the
    /// cutting moves visit the placed vertices in order, with no offset.
    #[test]
    fn open_path_is_traced_on_the_line() {
        let p = l_project();
        let (tp, warnings) = generate(&p, &engrave_op(&p, 1.0));
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = laps(&tp);
        assert_eq!(laps.len(), 1, "{laps:?}");
        assert_approx_eq!(laps[0].z_mm, THICKNESS_MM - 1.0, TOL);
        assert_points(&laps[0].points, &L_FORWARD);
    }

    /// Spec step 3: a closed path's block ends at its start.
    #[test]
    fn closed_path_returns_to_start() {
        let p = rect_project();
        let (tp, _) = generate(&p, &engrave_op(&p, 1.0));
        let laps = laps(&tp);
        assert_eq!(laps.len(), 1, "{laps:?}");
        assert_points(
            &laps[0].points,
            &[
                (20.0, 20.0),
                (120.0, 20.0),
                (120.0, 70.0),
                (20.0, 70.0),
                (20.0, 20.0),
            ],
        );
    }

    /// Spec step 2: passes as Profile's. The operation's step-down wins
    /// over the tool's 2 mm.
    #[test]
    fn passes_step_down_to_depth() {
        let p = rect_project();
        let (tp, _) = generate(&p, &engrave_op(&p, 0.5));
        let zs: Vec<f64> = laps(&tp).iter().map(|l| l.z_mm).collect();
        assert_z(&zs, &PASS_Z_MM);

        let mut op = engrave_op(&p, 0.5);
        op.step_down_mm = None;
        let (tp, _) = generate(&p, &op);
        let zs: Vec<f64> = laps(&tp).iter().map(|l| l.z_mm).collect();
        assert_z(&zs, &[THICKNESS_MM - 1.0]);
    }

    /// Spec step 3, D9: under `alternate_direction` the second pass of an
    /// open path runs backwards from where the first ended, and the only
    /// move between the passes is the plunge to the next Z.
    #[test]
    fn alternate_direction_reverses_odd_passes_without_retract() {
        let p = l_project();
        let mut op = engrave_op(&p, 0.5);
        op.alternate_direction = true;
        let (tp, _) = generate(&p, &op);
        let laps = laps(&tp);
        assert_eq!(laps.len(), 2, "{laps:?}");
        assert_points(&laps[0].points, &L_FORWARD);
        assert_points(&laps[1].points, &L_BACKWARD);
        let between = &tp.moves[laps[0].last + 1..laps[1].first];
        assert_eq!(
            between.len(),
            1,
            "one plunge between the passes: {between:?}"
        );
        let plunge = between[0];
        assert_eq!(plunge.kind, MoveKind::Linear);
        assert_approx_eq!(plunge.to.x, 30.0, TOL);
        assert_approx_eq!(plunge.to.y, 30.0, TOL);
        assert_approx_eq!(plunge.to.z, PASS_Z_MM[1], TOL);
        assert_approx_eq!(plunge.feed_mm_min.unwrap(), PLUNGE_MM_MIN, TOL);
    }

    /// Spec step 3: without alternation every pass starts at the path's
    /// first vertex: a retract to safe Z, a rapid back, a rapid down to
    /// 1 mm above the floor just cut, and a plunge (changed in #227: the
    /// plunge started at safe Z).
    #[test]
    fn no_alternation_retracts_between_passes() {
        let p = l_project();
        let (tp, _) = generate(&p, &engrave_op(&p, 0.5));
        let laps = laps(&tp);
        assert_eq!(laps.len(), 2, "{laps:?}");
        assert_points(&laps[0].points, &L_FORWARD);
        assert_points(&laps[1].points, &L_FORWARD);
        let between = &tp.moves[laps[0].last + 1..laps[1].first];
        assert_eq!(between.len(), 4, "{between:?}");
        assert!(between[0].is_rapid(), "{between:?}");
        assert_approx_eq!(between[0].to.x, 30.0, TOL);
        assert_approx_eq!(between[0].to.y, 30.0, TOL);
        assert_approx_eq!(between[0].to.z, SAFE_Z_MM, TOL);
        assert!(between[1].is_rapid(), "{between:?}");
        assert_approx_eq!(between[1].to.x, 20.0, TOL);
        assert_approx_eq!(between[1].to.y, 20.0, TOL);
        assert_approx_eq!(between[1].to.z, SAFE_Z_MM, TOL);
        assert!(between[2].is_rapid(), "{between:?}");
        assert_approx_eq!(between[2].to.z, PASS_Z_MM[0] + 1.0, TOL);
        assert_eq!(between[3].kind, MoveKind::Linear);
        assert_approx_eq!(between[3].to.z, PASS_Z_MM[1], TOL);
        assert_approx_eq!(between[3].feed_mm_min.unwrap(), PLUNGE_MM_MIN, TOL);
    }

    /// #227, S2: every pass reaches its plunge by a rapid straight down
    /// from safe Z, and plunges at the plunge rate only from 1 mm above
    /// the stock top (the first pass) or 1 mm above the floor the pass
    /// before cut at the path's start (a later pass after a lift).
    #[test]
    fn later_pass_rapids_to_just_above_its_floor() {
        let p = l_project();
        let (tp, _) = generate(&p, &engrave_op(&p, 0.5));
        let downs: Vec<(usize, Move)> = tp
            .moves
            .iter()
            .copied()
            .enumerate()
            .filter(|(i, m)| m.is_rapid() && *i > 0 && m.to.z < tp.moves[i - 1].to.z - EPS_MM)
            .collect();
        let expected = [
            (THICKNESS_MM + 1.0, PASS_Z_MM[0]),
            (PASS_Z_MM[0] + 1.0, PASS_Z_MM[1]),
        ];
        assert_eq!(downs.len(), expected.len(), "{:?}", tp.moves);
        for (&(i, rapid), (rapid_z, pass_z)) in downs.iter().zip(expected) {
            assert_eq!(
                tp.moves[i - 1],
                Move::rapid(Point3::new(20.0, 20.0, SAFE_Z_MM))
            );
            assert_eq!(rapid, Move::rapid(Point3::new(20.0, 20.0, rapid_z)));
            assert_eq!(
                tp.moves[i + 1],
                Move::linear(Point3::new(20.0, 20.0, pass_z), PLUNGE_MM_MIN)
            );
        }
    }

    /// Capability 15: any cutter, as with Profile's On line, and the cutter
    /// changes nothing about the moves.
    #[test]
    fn any_cutter_is_allowed() {
        let p = l_project();
        let (reference, _) = generate(&p, &engrave_op(&p, 0.5));
        assert!(!cuts(&reference).is_empty(), "the end mill cuts");
        for cutter in [
            ToolKind::BallNose,
            ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            ToolKind::Engraver,
            ToolKind::Drill,
        ] {
            let mut p = l_project();
            p.tools[0].cutter = cutter.clone();
            let (tp, warnings) = generate(&p, &engrave_op(&p, 0.5));
            assert!(warnings.is_empty(), "{cutter:?}: {warnings:?}");
            assert_eq!(cuts(&tp), cuts(&reference), "{cutter:?}");
        }
    }

    /// Spec step 1: a path of fewer than two distinct points warns and is
    /// skipped; the others are still cut.
    #[test]
    fn degenerate_path_warns() {
        let p = project(
            vec![
                Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(10.0, 0.0)], false),
                Polyline::new(vec![Vertex::line(5.0, 5.0)], false),
                Polyline::new(vec![Vertex::line(7.0, 7.0), Vertex::line(7.0, 7.0)], false),
            ],
            &[("i1", at_20_20())],
        );
        let (tp, warnings) = generate(&p, &engrave_op(&p, 1.0));
        let degenerate: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code() == WarningCode::EngravePathDegenerate)
            .collect();
        assert_eq!(degenerate.len(), 2, "{warnings:?}");
        for (w, profile) in degenerate.iter().zip([1, 2]) {
            let Warning::EngravePathDegenerate { op, shape } = w else {
                panic!("{w:?}");
            };
            assert_eq!(op.as_str(), "o1", "{w:?}");
            assert_eq!(shape.instance.as_str(), "i1", "{w:?}");
            assert_eq!(shape.instance_name, "Shape i1", "{w:?}");
            assert_eq!(shape.profile, profile, "{w:?}");
        }
        let laps = laps(&tp);
        assert_eq!(laps.len(), 1, "{laps:?}");
        assert_points(&laps[0].points, &[(20.0, 20.0), (30.0, 20.0)]);
    }

    /// S1: the depth may reach the stock bottom plus the allowance and no
    /// further.
    #[test]
    fn depth_obeys_material_bottom() {
        let p = l_project();
        let mut op = engrave_op(&p, 2.0);
        op.depth_mm = Some(THICKNESS_MM + 0.5);
        let err = generate_engrave(&p, &op).unwrap_err();
        assert!(
            matches!(err, CamError::BelowMaterialBottom { .. }),
            "{err:?}"
        );

        op.through_cut_allowance_mm = 0.5;
        let (tp, _) = generate(&p, &op);
        let lowest = cuts(&tp)
            .iter()
            .map(|m| m.to.z)
            .fold(f64::INFINITY, f64::min);
        assert_approx_eq!(lowest, -0.5, TOL);
    }

    /// Port of `testDirectModeRegression` (PureCutCNC `carving.test.ts`):
    /// a 20 mm line with a 4 mm flat end mill 1 mm deep. Upstream compares
    /// an unset carve strategy with an explicit `direct`; DreamCarve has
    /// only direct mode, so the port asserts the same thing it did, that
    /// two generations are move-for-move identical, and that every cut
    /// stays on the guide centreline. The line sits at Y = 20 rather than
    /// upstream's Y = 0 so the cutter's edge stays on the stock (S7).
    #[test]
    fn direct_mode_regression() {
        let mut p = project(
            vec![Polyline::new(
                vec![Vertex::line(0.0, 0.0), Vertex::line(20.0, 0.0)],
                false,
            )],
            &[("line1", at_20_20())],
        );
        p.tools[0].diameter_mm = 4.0;
        let op = engrave_op(&p, 2.0);
        let (first, _) = generate(&p, &op);
        let (second, _) = generate(&p, &op);
        assert!(!first.moves.is_empty(), "direct mode must produce moves");
        assert_eq!(first, second, "same moves, same order");
        let cuts = cuts(&first);
        assert!(!cuts.is_empty(), "direct mode must have cut moves");
        for (i, mv) in cuts.iter().enumerate() {
            assert!(
                (mv.to.y - 20.0).abs() <= 1e-6,
                "cut {i} must stay on the guide centreline, got y={}",
                mv.to.y
            );
        }
    }

    /// Spec step 4: containment pairs still apply to closed paths, so an
    /// outline inside another is cut first under both orders.
    #[test]
    fn closed_path_inside_another_is_cut_first() {
        let p = project(
            vec![
                rect_100x50(),
                Polyline::rect(Point::new(30.0, 10.0), 20.0, 20.0),
            ],
            &[("i1", at_20_20())],
        );
        for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
            let mut op = engrave_op(&p, 0.5);
            op.machining_order = order;
            let (tp, _) = generate(&p, &op);
            let laps = laps(&tp);
            assert_eq!(laps.len(), 4, "{order:?}: {laps:?}");
            assert_approx_eq!(laps[0].x_extent_mm(), 20.0, TOL);
        }
    }

    /// Spec step 4: open paths are their own units with no containment,
    /// so an open path inside an outline drawn first does not jump ahead
    /// of it.
    #[test]
    fn open_path_inside_an_outline_is_not_forced_first() {
        let p = project(
            vec![
                rect_100x50(),
                Polyline::new(
                    vec![Vertex::line(30.0, 10.0), Vertex::line(50.0, 10.0)],
                    false,
                ),
            ],
            &[("i1", at_20_20())],
        );
        let (tp, _) = generate(&p, &engrave_op(&p, 1.0));
        let laps = laps(&tp);
        assert_eq!(laps.len(), 2, "{laps:?}");
        assert_approx_eq!(laps[0].x_extent_mm(), 100.0, TOL);
        assert_approx_eq!(laps[1].x_extent_mm(), 20.0, TOL);
    }

    /// #206: Engrave takes no part in D6. It cuts every line as drawn at
    /// one depth, so lines that cross or touch (lettering, a hatch, a
    /// border meeting its corners) are the design, not two shapes cut
    /// into each other: crossing lines and a line touching an outline
    /// give no `toolpaths_overlap`.
    #[test]
    fn crossing_lines_do_not_warn_toolpaths_overlap() {
        let p = project(
            vec![
                rect_100x50(),
                Polyline::new(
                    vec![Vertex::line(50.0, 0.0), Vertex::line(50.0, 50.0)],
                    false,
                ),
                Polyline::new(
                    vec![Vertex::line(10.0, 25.0), Vertex::line(90.0, 25.0)],
                    false,
                ),
                Polyline::new(
                    vec![Vertex::line(20.0, 50.0), Vertex::line(20.0, 70.0)],
                    false,
                ),
            ],
            &[("i1", at_20_20())],
        );
        let (_, warnings) = generate(&p, &engrave_op(&p, 1.0));
        assert!(
            warnings
                .iter()
                .all(|w| w.code() != WarningCode::ToolpathsOverlap),
            "{warnings:?}"
        );
    }

    /// Capability 11 on Engrave: pass first cuts every path at one depth
    /// before stepping down; full depth first finishes each path.
    #[test]
    fn pass_first_cuts_every_path_before_stepping_down() {
        let p = project(
            vec![
                Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(10.0, 0.0)], false),
                Polyline::new(
                    vec![Vertex::line(50.0, 0.0), Vertex::line(60.0, 0.0)],
                    false,
                ),
            ],
            &[("i1", at_20_20())],
        );
        let mut op = engrave_op(&p, 0.5);
        op.machining_order = MachiningOrder::PassFirst;
        let (tp, _) = generate(&p, &op);
        assert_z(&cut_z_transitions(&tp), &[5.5, 5.0]);
        op.machining_order = MachiningOrder::FullDepthFirst;
        let (tp, _) = generate(&p, &op);
        assert_z(&cut_z_transitions(&tp), &[5.5, 5.0, 5.5, 5.0]);
    }

    /// Safety: a feed is never invented.
    #[test]
    fn missing_feed_is_error_not_invented() {
        let mut p = l_project();
        p.tools[0].feed_mm_min = None;
        let err = generate_engrave(&p, &engrave_op(&p, 1.0)).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::MissingFeed {
                    field: FeedField::Feed,
                    ..
                }
            ),
            "{err:?}"
        );
        let mut p = l_project();
        p.tools[0].plunge_mm_min = None;
        let err = generate_engrave(&p, &engrave_op(&p, 1.0)).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::MissingFeed {
                    field: FeedField::Plunge,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let p = l_project();
        let mut op = engrave_op(&p, 1.0);
        op.tool = ToolId::new("nope");
        let err = generate_engrave(&p, &op).unwrap_err();
        assert!(matches!(err, CamError::UnknownTool { .. }), "{err:?}");
    }

    /// S7 (#62): the cutter's edge must stay on the stock. A line starting
    /// 1 mm inside the left edge overhangs it by the radius less 1 mm.
    #[test]
    fn toolpath_off_stock_is_an_error() {
        let p = project(vec![open_l()], &[("i1", Transform::translate(1.0, 20.0))]);
        let err = generate_engrave(&p, &engrave_op(&p, 1.0)).unwrap_err();
        assert!(matches!(err, CamError::OffStock { .. }), "{err:?}");
    }
}
