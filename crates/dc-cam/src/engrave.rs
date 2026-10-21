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

use crate::error::CamError;
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_project::{EngraveOp, Project};

/// Generates the Engrave toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
#[must_use = "the toolpath is the whole point"]
pub fn generate_engrave(
    _project: &Project,
    _op: &EngraveOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    Ok((Toolpath::default(), Vec::new()))
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
    /// first vertex: a retract to safe Z, a rapid back, and a plunge.
    #[test]
    fn no_alternation_retracts_between_passes() {
        let p = l_project();
        let (tp, _) = generate(&p, &engrave_op(&p, 0.5));
        let laps = laps(&tp);
        assert_eq!(laps.len(), 2, "{laps:?}");
        assert_points(&laps[0].points, &L_FORWARD);
        assert_points(&laps[1].points, &L_FORWARD);
        let between = &tp.moves[laps[0].last + 1..laps[1].first];
        assert_eq!(between.len(), 3, "{between:?}");
        assert!(between[0].is_rapid(), "{between:?}");
        assert_approx_eq!(between[0].to.x, 30.0, TOL);
        assert_approx_eq!(between[0].to.y, 30.0, TOL);
        assert_approx_eq!(between[0].to.z, SAFE_Z_MM, TOL);
        assert!(between[1].is_rapid(), "{between:?}");
        assert_approx_eq!(between[1].to.x, 20.0, TOL);
        assert_approx_eq!(between[1].to.y, 20.0, TOL);
        assert_approx_eq!(between[1].to.z, SAFE_Z_MM, TOL);
        assert_eq!(between[2].kind, MoveKind::Linear);
        assert_approx_eq!(between[2].to.z, PASS_Z_MM[1], TOL);
        assert_approx_eq!(between[2].feed_mm_min.unwrap(), PLUNGE_MM_MIN, TOL);
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
            .filter(|w| w.code == WarningCode::EngravePathDegenerate)
            .collect();
        assert_eq!(degenerate.len(), 2, "{warnings:?}");
        for (w, profile) in degenerate.iter().zip(["1", "2"]) {
            assert_eq!(w.param("op"), Some("o1"), "{w:?}");
            assert_eq!(w.param("instance"), Some("i1"), "{w:?}");
            assert_eq!(w.param("instance_name"), Some("Shape i1"), "{w:?}");
            assert_eq!(w.param("profile"), Some(profile), "{w:?}");
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
