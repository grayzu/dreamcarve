// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/arcFitting.ts
//   src/engine/toolpaths/arcReconstruction.ts
//   src/engine/gcode/arcFitting.test.ts

//! Arc fitting at export (`docs/specs/phase-2/design.md`, "Algorithms →
//! Arc fitting"; `docs/specs/phase-2/functional.md`, capability 24, S12).

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

    use dc_cam::{Move, MoveKind, generate_profile};
    use dc_geometry::{Point, Point3, Polyline, Transform, Vertex, assert_approx_eq};
    use dc_project::test_support::{profile_mut, sample_project};
    use dc_project::{
        ArcFormat, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
        PostProcessorRef, Project, Stock, TabPlacement, TabShape, TabSpec,
    };

    use crate::emit::{ExportWarning, emit};
    use crate::parse::{MotionKind, parse_motion};

    const TOL: f64 = 0.01;
    const FEED: f64 = 1000.0;

    fn pt(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    /// `n` chords of the circle of radius `r` about the origin from angle
    /// `from_rad` to `to_rad`, at `z`, after a rapid to the first point.
    fn chords(r: f64, n: usize, from_rad: f64, to_rad: f64, z: f64) -> Vec<Move> {
        let mut moves = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let angle = from_rad + (to_rad - from_rad) * k as f64 / n as f64;
            let p = pt(r * angle.cos(), r * angle.sin(), z);
            if k == 0 {
                moves.push(Move::rapid(p));
            } else {
                moves.push(Move::linear(p, FEED));
            }
        }
        moves
    }

    fn arcs_of(fitted: &[Fitted]) -> Vec<&SubArc> {
        fitted
            .iter()
            .filter_map(|f| match f {
                Fitted::Arc(run) => Some(run.arcs.iter()),
                Fitted::Move(_) => None,
            })
            .flatten()
            .collect()
    }

    fn moves_of(fitted: &[Fitted]) -> Vec<&Move> {
        fitted
            .iter()
            .filter_map(|f| match f {
                Fitted::Move(m) => Some(m),
                Fitted::Arc(_) => None,
            })
            .collect()
    }

    fn lines_of(fitted: &[Fitted]) -> usize {
        moves_of(fitted)
            .iter()
            .filter(|m| m.kind == MoveKind::Linear)
            .count()
    }

    fn runs_of(fitted: &[Fitted]) -> usize {
        fitted
            .iter()
            .filter(|f| matches!(f, Fitted::Arc(_)))
            .count()
    }

    fn sweep_deg(arc: &SubArc) -> f64 {
        let a0 = (arc.start.y - arc.centre.y).atan2(arc.start.x - arc.centre.x);
        let a1 = (arc.end.y - arc.centre.y).atan2(arc.end.x - arc.centre.x);
        let mut sweep = (a1 - a0).abs();
        if sweep > PI {
            sweep = TAU - sweep;
        }
        sweep.to_degrees()
    }

    #[test]
    fn accepted_circular_run_gives_four_arcs() {
        let moves = chords(10.0, 16, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4, "{fitted:?}");
        assert!(arcs.iter().all(|a| !a.clockwise), "counter-clockwise");
        let last = arcs.last().unwrap();
        assert_approx_eq!(last.end.x, 10.0, 1e-9);
        assert_approx_eq!(last.end.y, 0.0, 1e-9);
        // The run replaces every chord; the rapid passes through.
        assert_eq!(lines_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 1);
    }

    #[test]
    fn clockwise_is_g2() {
        let moves = chords(10.0, 8, 0.0, -TAU, 0.0);
        let arcs = arcs_of(&fit(&moves, TOL));
        assert!(!arcs.is_empty());
        assert!(arcs.iter().all(|a| a.clockwise), "clockwise in Y-up is G2");
    }

    #[test]
    fn rejects_straight_run() {
        let moves = vec![
            Move::rapid(pt(0.0, 0.0, 0.0)),
            Move::linear(pt(1.0, 0.0, 0.0), FEED),
            Move::linear(pt(2.0, 0.0, 0.0), FEED),
            Move::linear(pt(3.0, 0.0, 0.0), FEED),
            Move::linear(pt(4.0, 0.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(lines_of(&fitted), 4);
        assert_eq!(moves_of(&fitted).len(), 5);
    }

    #[test]
    fn rejects_z_changing_run() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(pt(7.07, 7.07, -0.5), FEED),
            Move::linear(pt(0.0, 10.0, -1.0), FEED),
            Move::linear(pt(-7.07, 7.07, -1.5), FEED),
            Move::linear(pt(-10.0, 0.0, -2.0), FEED),
        ];
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    #[test]
    fn rejects_mixed_feed_run() {
        let mut moves = chords(10.0, 8, 0.0, TAU, 0.0);
        for (k, m) in moves.iter_mut().skip(1).enumerate() {
            m.feed_mm_min = Some(if k % 2 == 0 { 500.0 } else { FEED });
        }
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    /// A toolpath has no gaps (each move starts where the last ended), so
    /// a discontinuity is a move the run cannot include: here a vertical
    /// plunge between two chords of one circle.
    #[test]
    fn rejects_discontinuous_run() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(
                pt(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 0.0),
                FEED,
            ),
            Move::linear(pt(0.0, 10.0, 0.0), FEED),
            Move::linear(pt(0.0, 10.0, -1.0), 300.0),
            Move::linear(
                pt(-10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), -1.0),
                FEED,
            ),
            Move::linear(pt(-10.0, 0.0, -1.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 6);
    }

    #[test]
    fn splits_around_rapids() {
        let mut moves = chords(10.0, 4, 0.0, FRAC_PI_2, 0.0);
        moves.extend(chords(10.0, 4, PI, PI + FRAC_PI_2, 0.0));
        let fitted = fit(&moves, TOL);
        assert_eq!(arcs_of(&fitted).len(), 2, "{fitted:?}");
        let rest = moves_of(&fitted);
        assert_eq!(rest.len(), 2);
        assert!(rest.iter().all(|m| m.kind == MoveKind::Rapid));
    }

    #[test]
    fn short_run_stays_linear() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(pt(7.0, 7.0, 0.0), FEED),
            Move::linear(pt(0.0, 10.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(lines_of(&fitted), 2);
    }

    #[test]
    fn rejects_ambiguous_direction() {
        let moves = vec![
            Move::rapid(pt(0.0, 0.0, 0.0)),
            Move::linear(pt(1.0, 1.0, 0.0), FEED),
            Move::linear(pt(2.0, 0.0, 0.0), FEED),
            Move::linear(pt(3.0, 1.0, 0.0), FEED),
            Move::linear(pt(4.0, 0.0, 0.0), FEED),
        ];
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    #[test]
    fn rejects_high_residual_ellipse() {
        let (a, b, n) = (10.0, 5.0, 8);
        let mut moves = Vec::new();
        for k in 0..=n {
            let angle = TAU * f64::from(k) / f64::from(n);
            let p = pt(a * angle.cos(), b * angle.sin(), 0.0);
            moves.push(if k == 0 {
                Move::rapid(p)
            } else {
                Move::linear(p, FEED)
            });
        }
        assert_eq!(runs_of(&fit(&moves, 0.01)), 0);
    }

    #[test]
    fn splits_large_arc_at_90_degrees() {
        let moves = chords(10.0, 32, 0.0, 3.0 * FRAC_PI_2, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 3, "{fitted:?}");
        for arc in arcs {
            assert!(sweep_deg(arc) <= 90.1, "{}", sweep_deg(arc));
        }
    }

    #[test]
    fn split_arc_offsets_relative() {
        let moves = chords(10.0, 16, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert!(arcs.len() >= 2);
        let c0 = arcs[0].centre;
        let mut distinct = 0;
        let (i0, j0) = (c0.x - arcs[0].start.x, c0.y - arcs[0].start.y);
        for arc in &arcs {
            assert_approx_eq!(arc.centre.x, c0.x, 1e-6);
            assert_approx_eq!(arc.centre.y, c0.y, 1e-6);
            let (i, j) = (arc.centre.x - arc.start.x, arc.centre.y - arc.start.y);
            if (i - i0).abs() > 1e-6 || (j - j0).abs() > 1e-6 {
                distinct += 1;
            }
        }
        assert!(distinct > 0, "I and J are per sub-arc");
    }

    #[test]
    fn rejects_near_collinear_run() {
        let (huge_r, n) = (100_000.0, 4);
        let shallow = PI / 720.0;
        let points: Vec<Point> = (0..=n)
            .map(|k| {
                let angle = shallow * (f64::from(k) / f64::from(n) - 0.5);
                Point::new(huge_r * angle.cos(), huge_r * angle.sin())
            })
            .collect();
        let (cx, cy) = (points[0].x, (points[0].y + points[n as usize].y) / 2.0);
        let mut moves = Vec::new();
        for (k, p) in points.iter().enumerate() {
            let p = pt(p.x - cx, p.y - cy, 0.0);
            moves.push(if k == 0 {
                Move::rapid(p)
            } else {
                Move::linear(p, FEED)
            });
        }
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 5);
    }

    #[test]
    fn rejects_long_straight_with_corner_fragment() {
        let moves = vec![
            Move::rapid(pt(2.625, 1.77, 0.0)),
            Move::linear(pt(1.375, 1.77, 0.0), FEED),
            Move::linear(
                pt(1.362_362_417_301_589_6, 1.769_448_231_223_303, 0.0),
                FEED,
            ),
            Move::linear(pt(1.349_821_014_238_295_1, 1.767_797_124_186_77, 0.0), FEED),
        ];
        let fitted = fit(&moves, 0.01 / 25.4);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 4);
    }

    #[test]
    fn accepts_ordinary_arc_after_collinear_gate() {
        let moves = chords(5.0, 4, 0.0, PI / 6.0, 0.0);
        assert_eq!(arcs_of(&fit(&moves, TOL)).len(), 1);
    }

    #[test]
    fn full_circle_loop_is_four_arcs() {
        let moves = chords(10.0, 16, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4);
        assert_eq!(lines_of(&fitted), 0);
        for arc in arcs {
            assert!(
                arc.start.xy().distance_to(arc.end.xy()) > 1.0,
                "no sub-arc ends where it starts: {arc:?}"
            );
        }
    }

    #[test]
    fn partial_arc_with_straight_leads() {
        let r = 10.0;
        let mut moves = vec![
            Move::rapid(pt(-15.0, 0.0, 0.0)),
            Move::linear(pt(-10.0, 0.0, 0.0), FEED),
            Move::linear(pt(-5.0, 0.0, 0.0), FEED),
            Move::linear(pt(r, 0.0, 0.0), FEED),
        ];
        moves.extend(chords(r, 8, 0.0, PI, 0.0).into_iter().skip(1));
        moves.extend([
            Move::linear(pt(-13.0, 0.0, 0.0), FEED),
            Move::linear(pt(-16.0, 0.0, 0.0), FEED),
            Move::linear(pt(-19.0, 0.0, 0.0), FEED),
        ]);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 2, "{fitted:?}");
        assert_eq!(lines_of(&fitted), 6);
        assert_approx_eq!(arcs[0].centre.x, 0.0, 1e-6);
        assert_approx_eq!(arcs[0].centre.y, 0.0, 1e-6);
        assert_approx_eq!(arcs[0].start.x, r, 1e-9);
        assert_approx_eq!(arcs[0].start.y, 0.0, 1e-9);
        assert_approx_eq!(arcs[1].end.x, -r, 1e-9);
        assert_approx_eq!(arcs[1].end.y, 0.0, 1e-9);
    }

    /// The 14 contiguous cut moves from PureCutCNC issue #447, after a
    /// rapid to the first point.
    fn issue_447_moves() -> Vec<Move> {
        let xy = [
            (122.394_176_793_950_8, 167.172_783_309_121_77),
            (122.375_566_801_907_44, 167.103_329_873_287_52),
            (122.369_300_000_024_96, 167.0317),
            (122.366_778_981_394_65, 166.960_070_126_712_48),
            (122.376_601_152_925_22, 166.890_616_690_878_23),
            (122.398_201_059_425_5, 166.825_450_000_000_05),
            (122.430_655_385_187_21, 166.766_550_111_004_34),
            (122.472_711_008_465_3, 166.715_706_667_213_45),
            (122.522_823_076_948_27, 166.674_464_520_938_95),
            (122.579_201_947_313_66, 166.644_076_793_925_7),
            (122.639_867_562_635_06, 166.625_466_801_882_57),
            (122.702_709_615_409_9, 166.619_200_000_000_1),
            (122.765_551_668_184_74, 166.625_466_801_882_57),
            (122.826_217_283_506_14, 166.644_076_793_925_7),
            (122.882_596_153_871_53, 166.674_464_520_938_95),
        ];
        xy.iter()
            .enumerate()
            .map(|(k, &(x, y))| {
                let p = pt(x, y, -10.5);
                if k == 0 {
                    Move::rapid(p)
                } else {
                    Move::linear(p, FEED)
                }
            })
            .collect()
    }

    #[test]
    fn adjacent_runs_share_endpoints_exactly() {
        let fitted = fit(&issue_447_moves(), TOL);
        assert!(runs_of(&fitted) >= 2, "{fitted:?}");
        let mut previous: Option<Point3> = None;
        for item in &fitted {
            match item {
                Fitted::Arc(run) => {
                    for arc in &run.arcs {
                        if let Some(p) = previous {
                            assert!(
                                p.x.to_bits() == arc.start.x.to_bits()
                                    && p.y.to_bits() == arc.start.y.to_bits(),
                                "arc starts exactly where the last item ended"
                            );
                        }
                        previous = Some(arc.end);
                    }
                }
                Fitted::Move(m) => previous = Some(m.to),
            }
        }
    }

    #[test]
    fn fitted_arcs_pass_through_source_endpoints() {
        let moves = issue_447_moves();
        let fitted = fit(&moves, TOL);
        let end = match fitted.last().unwrap() {
            Fitted::Arc(run) => run.arcs.last().unwrap().end,
            Fitted::Move(m) => m.to,
        };
        let target = moves.last().unwrap().to;
        assert!(end.xy().distance_to(target.xy()) < 1e-9);
    }

    /// Three-decimal millimetre formatting, as GRBL-family definitions use.
    fn mm3(value: f64) -> Option<f64> {
        Some((value * 1000.0).round() / 1000.0)
    }

    fn inch4(value: f64) -> Option<f64> {
        Some((value * 10_000.0).round() / 10_000.0)
    }

    fn mm3_opts(arc_format: ArcFormat) -> EmitOptions<'static> {
        EmitOptions {
            format: &mm3,
            arc_format,
            mm_per_unit: 1.0,
            quantum: 1e-3,
        }
    }

    fn inch4_opts() -> EmitOptions<'static> {
        EmitOptions {
            format: &inch4,
            arc_format: ArcFormat::Ij,
            mm_per_unit: 25.4,
            quantum: 1e-4,
        }
    }

    fn identity(p: Point) -> Point {
        p
    }

    /// The controller's own radius check on a formatted block.
    fn controller_delta(start: Point, target: Point, i: f64, j: f64) -> f64 {
        ((target.x - (start.x + i)).hypot(target.y - (start.y + j)) - i.hypot(j)).abs()
    }

    /// The worst radius disagreement any emitted arc presents, walking the
    /// formatted position from item to item as the emitter does.
    fn worst_emitted_delta(fitted: &[Fitted], opts: &EmitOptions<'_>) -> f64 {
        let f = |v: f64| (opts.format)(v).unwrap();
        let mut position: Option<Point> = None;
        let mut worst: f64 = 0.0;
        for item in fitted {
            match item {
                Fitted::Move(m) => position = Some(Point::new(f(m.to.x), f(m.to.y))),
                Fitted::Arc(run) => {
                    for arc in &run.arcs {
                        let start = position.unwrap_or(Point::new(f(arc.start.x), f(arc.start.y)));
                        let target = Point::new(f(arc.end.x), f(arc.end.y));
                        let emitted = resolve_emitted_arc(
                            arc.start.xy(),
                            arc.end.xy(),
                            arc.centre,
                            start,
                            opts,
                        );
                        worst =
                            worst.max(controller_delta(start, target, f(emitted.i), f(emitted.j)));
                        position = Some(target);
                    }
                }
            }
        }
        worst
    }

    #[test]
    fn ij_derived_from_previous_emitted_point() {
        // The exporter's un-rounded idea of the start is 10.0004; the
        // controller is at the formatted 10.000.
        let emitted = resolve_emitted_arc(
            Point::new(10.0004, 0.0),
            Point::new(0.0, 10.0),
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            &mm3_opts(ArcFormat::Ij),
        );
        assert_approx_eq!(emitted.i, -10.0, 1e-9);
        assert!(emitted.accepted);
    }

    /// A run the controller must reject: the end sits 0.02 mm off the
    /// circle its own I/J declares. Built by hand, so the test pins the
    /// fallback and not the fitter's choice of run boundaries.
    fn inconsistent_run() -> ArcRun {
        ArcRun {
            arcs: vec![SubArc {
                start: pt(10.0, 0.0, 0.0),
                end: pt(0.0, 10.02, 0.0),
                centre: Point::new(0.0, 0.0),
                clockwise: false,
            }],
            feed_mm_min: FEED,
            fallback: vec![
                Move::linear(pt(7.07, 7.07, 0.0), FEED),
                Move::linear(pt(0.0, 10.02, 0.0), FEED),
            ],
        }
    }

    #[test]
    fn invalid_run_falls_back_to_original_moves() {
        let fitted = vec![
            Fitted::Move(Move::rapid(pt(10.0, 0.0, 0.0))),
            Fitted::Arc(inconsistent_run()),
        ];
        let (out, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::Ij), None, &identity);
        assert_eq!(runs, 1);
        assert_eq!(runs_of(&out), 0);
        assert_eq!(out.len(), 3, "the run's original moves, one for one");
        let Fitted::Move(last) = out.last().unwrap() else {
            panic!("fallback output is moves");
        };
        assert_eq!(last.to, pt(0.0, 10.02, 0.0));
    }

    #[test]
    fn fallback_leaves_valid_runs_alone() {
        let mut moves = chords(10.0, 16, 0.0, TAU, -10.5);
        moves.extend(issue_447_moves());
        let fitted = fit(&moves, TOL);
        let good = arcs_of(&fitted).len();
        assert!(good > 0);
        let opts = mm3_opts(ArcFormat::Ij);
        let (clean, runs) = apply_fallback(fitted.clone(), &opts, None, &identity);
        assert_eq!(runs, 0, "well-formed spans do not fall back");
        assert!(worst_emitted_delta(&clean, &opts) <= ARC_RADIUS_CONSISTENCY_MM);

        let mut mixed = fitted;
        mixed.push(Fitted::Move(Move::rapid(pt(10.0, 0.0, 0.0))));
        mixed.push(Fitted::Arc(inconsistent_run()));
        let (out, runs) = apply_fallback(mixed, &opts, None, &identity);
        assert_eq!(runs, 1, "only the offending run falls back");
        assert_eq!(arcs_of(&out).len(), good);
    }

    #[test]
    fn r_format_rejects_unrepresentable_chord() {
        let bad = resolve_emitted_arc(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 0.0),
            &mm3_opts(ArcFormat::R),
        );
        assert!(!bad.accepted, "a chord longer than the diameter");
        let fitted = fit(&issue_447_moves(), TOL);
        let (_, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::R), None, &identity);
        assert_eq!(runs, 0, "the #447 span is representable in R format");
    }

    #[test]
    fn inch_output_uses_millimetre_budget() {
        // 0.004 in of radius disagreement is 0.1016 mm: legal if the budget
        // were applied in inches, far past GRBL's 0.005 mm.
        let as_inch = resolve_emitted_arc(
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.004),
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            &inch4_opts(),
        );
        assert!(!as_inch.accepted, "{}", as_inch.delta_r_mm);
    }

    #[test]
    fn snapping_never_worsens_agreement() {
        for (label, opts, scale) in [
            ("mm @3dp", mm3_opts(ArcFormat::Ij), 1.0),
            ("inch @4dp", inch4_opts(), 1.0 / 25.4),
        ] {
            let f = |v: f64| (opts.format)(v).unwrap();
            let moves: Vec<Move> = issue_447_moves()
                .into_iter()
                .map(|m| Move {
                    to: pt(m.to.x * scale, m.to.y * scale, m.to.z * scale),
                    ..m
                })
                .collect();
            let fitted = fit(&moves, TOL * scale);
            let mut position: Option<Point> = None;
            let mut improved = false;
            for item in &fitted {
                match item {
                    Fitted::Move(m) => position = Some(Point::new(f(m.to.x), f(m.to.y))),
                    Fitted::Arc(run) => {
                        for arc in &run.arcs {
                            let start =
                                position.unwrap_or(Point::new(f(arc.start.x), f(arc.start.y)));
                            let resolved = resolve_emitted_arc(
                                arc.start.xy(),
                                arc.end.xy(),
                                arc.centre,
                                start,
                                &opts,
                            );
                            let naive_i = f(arc.centre.x - start.x);
                            let naive_j = f(arc.centre.y - start.y);
                            let target = Point::new(f(arc.end.x), f(arc.end.y));
                            let naive = controller_delta(start, target, naive_i, naive_j)
                                * opts.mm_per_unit;
                            assert!(
                                resolved.delta_r_mm <= naive + 1e-12,
                                "{label}: snapped {} vs naive {naive}",
                                resolved.delta_r_mm
                            );
                            if resolved.delta_r_mm < naive - 1e-12 {
                                improved = true;
                            }
                            position = Some(target);
                        }
                    }
                }
            }
            assert!(improved, "{label}: snapping improves at least one arc");
        }
    }

    #[test]
    fn r_format_repairs_rounding_shortfall_only() {
        let opts = mm3_opts(ArcFormat::R);
        // A chord a hair longer than the rounded diameter: a rounding
        // artifact, repaired by one grid step.
        let repaired = resolve_emitted_arc(
            Point::new(0.0, 0.0),
            Point::new(1.0004, 0.0),
            Point::new(0.5002, 0.0),
            Point::new(0.0, 0.0),
            &opts,
        );
        assert!(repaired.accepted);
        assert!(repaired.radius >= 0.5, "{}", repaired.radius);
        assert!(repaired.radius < 0.51, "{}", repaired.radius);
        // Radius 1, chord 10: inconsistent by a factor of five, not by
        // rounding.
        let rejected = resolve_emitted_arc(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 0.0),
            &opts,
        );
        assert!(!rejected.accepted);
    }

    /// The quick start's coaster: a 90 mm circle imported as 160 facets on
    /// 110 × 110 × 11.8 mm stock, cut through by 0.2 mm in 4 mm passes with
    /// four 8 × 3 mm rect tabs, on grblHAL.
    fn coaster_project() -> Project {
        let mut project = sample_project();
        project.post_processor = PostProcessorRef::latest("grblhal").unwrap();
        project.stock = Stock {
            width_mm: 110.0,
            height_mm: 110.0,
            thickness_mm: Some(11.8),
        };
        let facets: Vec<Vertex> = (0..160)
            .map(|k| {
                let angle = TAU * f64::from(k) / 160.0;
                Vertex::line(45.0 * angle.cos(), 45.0 * angle.sin())
            })
            .collect();
        project.definitions.clear();
        project.definitions.insert(
            DefinitionId::new("d1"),
            FeatureDefinition {
                id: DefinitionId::new("d1"),
                name: "Circle".into(),
                profiles: vec![Polyline::new(facets, true)],
            },
        );
        project.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(55.0, 55.0),
            name: "Circle".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(11.8),
            z_bottom_mm: Some(0.0),
        }];
        let tool = &mut project.tools[0];
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.step_down_mm = Some(4.0);
        let op = profile_mut(&mut project);
        op.depth_mm = Some(12.0);
        op.through_cut_allowance_mm = 0.2;
        op.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 8.0,
            height_mm: 3.0,
            shape: TabShape::Rect,
        });
        project
    }

    /// Emits the coaster and returns the program text, its warnings, and
    /// the toolpath.
    fn export_coaster(project: &Project) -> (String, Vec<ExportWarning>, dc_cam::Toolpath) {
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("the coaster has a profile operation");
        };
        let tool = project.tools[0].clone();
        let (toolpath, warnings) = generate_profile(project, &op).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        dc_cam::test_support::assert_safe(&toolpath, project, &op);
        let (program, warnings) =
            emit(project, &[(op, tool, toolpath.clone())], "2026-10-15").unwrap();
        (program.text, warnings, toolpath)
    }

    /// Functional acceptance 37: the coaster exports as at most 8 arcs per
    /// lap, every original vertex within the tolerance of its arc (S6
    /// passes, or `emit` would have failed), with no fallback.
    #[test]
    fn coaster_circle_exports_as_at_most_eight_arcs_per_lap() {
        let project = coaster_project();
        assert_eq!(project.arc_fit_tolerance_mm, Some(0.01));
        let (text, warnings, _) = export_coaster(&project);
        assert!(warnings.is_empty(), "{warnings:?}");
        let moves = parse_motion(&text).unwrap();
        let arcs = moves
            .iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count();
        let lines = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .count();
        // Three laps (12 mm in 4 mm passes).
        assert!(arcs <= 3 * 8, "{arcs} arcs in {text}");
        assert!(arcs >= 3, "each lap is fitted: {text}");
        // The 160 facets and their corner arcs per lap are gone.
        assert!(lines < 3 * 160, "{lines} straight moves in {text}");
    }

    /// Functional acceptance 37: `arc_fit_tolerance_mm: null` gives the
    /// phase 1 program, move for move.
    #[test]
    fn null_tolerance_reproduces_phase_1_program() {
        let mut project = coaster_project();
        project.arc_fit_tolerance_mm = None;
        let (text, warnings, toolpath) = export_coaster(&project);
        assert!(warnings.is_empty(), "{warnings:?}");
        let moves = parse_motion(&text).unwrap();
        let program_arcs = moves
            .iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count();
        let program_lines = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .count();
        let toolpath_arcs = toolpath
            .moves
            .iter()
            .filter(|m| matches!(m.kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }))
            .count();
        let toolpath_lines = toolpath
            .moves
            .iter()
            .filter(|m| m.kind == MoveKind::Linear)
            .count();
        assert_eq!(program_arcs, toolpath_arcs, "{text}");
        assert_eq!(program_lines, toolpath_lines, "{text}");
        assert!(
            toolpath_arcs >= 3 * 160,
            "the corner arcs are written as arcs"
        );
    }

    /// A helix descent changes Z on every chord, so it stays lines; the
    /// flat revolution at the bottom is fitted.
    #[test]
    fn helix_descent_is_not_fitted_flat_revolution_is() {
        let n = 16;
        let mut moves = vec![Move::rapid(pt(10.0, 0.0, 5.0))];
        for k in 1..=n {
            let angle = TAU * f64::from(k) / f64::from(n);
            let z = 5.0 - 2.0 * f64::from(k) / f64::from(n);
            moves.push(Move::linear(
                pt(10.0 * angle.cos(), 10.0 * angle.sin(), z),
                300.0,
            ));
        }
        moves.extend(chords(10.0, n as usize, 0.0, TAU, 3.0).into_iter().skip(1));
        let fitted = fit(&moves, TOL);
        assert_eq!(lines_of(&fitted), n as usize, "the descent stays lines");
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4, "{fitted:?}");
        for arc in arcs {
            assert_approx_eq!(arc.start.z, 3.0, 1e-9);
            assert_approx_eq!(arc.end.z, 3.0, 1e-9);
        }
    }
}
