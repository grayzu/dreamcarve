// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/arcFitting.ts
//   src/engine/toolpaths/arcReconstruction.ts
//   src/engine/gcode/arcFitting.test.ts

//! Tests of arc fitting: which runs [`fit`] replaces and with what
//! sub-arcs, how an emitted arc is judged once its numbers are rounded,
//! and the fallback to the original moves. The upstream tests ported here
//! are from `arcFitting.test.ts`.

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
    let moves = chords(10.0, 72, 0.0, TAU, 0.0);
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

/// A run that goes out along a circle and back (a Profile's ramp
/// returning to its entry point before the lap) lies on the circle
/// but is not one arc: every fitted run turns one way, so the moves
/// it replaces are the moves it follows.
#[test]
fn run_that_turns_back_is_not_one_arc() {
    let mut moves = chords(10.0, 18, 0.0, FRAC_PI_2, 0.0);
    moves.extend(chords(10.0, 18, FRAC_PI_2, 0.0, 0.0).into_iter().skip(1));
    moves.extend(chords(10.0, 36, 0.0, PI, 0.0).into_iter().skip(1));
    let fitted = fit(&moves, TOL);
    assert!(runs_of(&fitted) > 0, "the one-way stretches still fit");
    for f in &fitted {
        let Fitted::Arc(run) = f else { continue };
        let mut from = run.arcs[0].start;
        let mut sense = 0.0_f64;
        for mv in &run.fallback {
            let o = Point::new(0.0, 0.0);
            let turn = (from.xy() - o).cross(mv.to.xy() - o);
            assert!(sense * turn >= 0.0, "a run turns back: {run:?}");
            sense = turn;
            from = mv.to;
        }
    }
}

/// The angle `moves` turn about the origin from `from`, radians,
/// counter-clockwise positive.
fn turned_rad(from: Point3, moves: &[Move]) -> f64 {
    let mut at = from.xy().y.atan2(from.xy().x);
    let mut turned = 0.0;
    for mv in moves {
        let to = mv.to.y.atan2(mv.to.x);
        turned += (to - at + PI).rem_euclid(TAU) - PI;
        at = to;
    }
    turned
}

/// The angle `arc` turns as the controller reads it, radians,
/// counter-clockwise positive: at most a turn, and a whole turn where
/// it ends where it starts.
fn arc_turn_rad(arc: &SubArc) -> f64 {
    let a0 = (arc.start.y - arc.centre.y).atan2(arc.start.x - arc.centre.x);
    let a1 = (arc.end.y - arc.centre.y).atan2(arc.end.x - arc.centre.x);
    let turn = if arc.clockwise { a0 - a1 } else { a1 - a0 }.rem_euclid(TAU);
    let turn = if turn < 1e-9 { TAU } else { turn };
    if arc.clockwise { -turn } else { turn }
}

/// Fails unless every fitted run's sub-arcs turn as far as the moves
/// they replace.
fn assert_runs_turn_as_their_moves(moves: &[Move]) {
    let fitted = fit(moves, TOL);
    assert!(runs_of(&fitted) > 0, "{fitted:?}");
    for f in &fitted {
        let Fitted::Arc(run) = f else { continue };
        let arcs: f64 = run.arcs.iter().map(arc_turn_rad).sum();
        let moves = turned_rad(run.arcs[0].start, &run.fallback);
        assert!(
            (arcs - moves).abs() < 1e-6,
            "the arcs turn {:.1}°, their moves {:.1}°: {run:?}",
            arcs.to_degrees(),
            moves.to_degrees()
        );
    }
}

/// A run that goes on round its circle past a full turn (a male
/// Inlay's V-bit joins a ring of its band three chords before the
/// point it laps from, then cuts the whole ring) keeps every lap. A
/// sub-arc takes its sweep from its ends, modulo a turn, so one run
/// over all 80 chords was written as its last 34° alone and the lap
/// before them was never cut. Either way round.
#[test]
fn run_past_a_full_turn_keeps_its_lap() {
    let past = TAU + 0.6;
    assert_runs_turn_as_their_moves(&chords(10.0, 80, 0.0, past, 0.0));
    assert_runs_turn_as_their_moves(&chords(10.0, 80, 0.0, -past, 0.0));
}

/// A run that closes after two turns (80 chords of a 2 mm circle,
/// each within the tolerance of it) is two turns, not the one a
/// closed run's ends give.
#[test]
fn run_of_two_closed_turns_keeps_both() {
    assert_runs_turn_as_their_moves(&chords(2.0, 80, 0.0, 2.0 * TAU, 0.0));
}

/// A ring that closes a hair past its start (its last vertex 1e-7
/// mm on, within the closed run's 1e-6 · r) is one whole turn, not
/// the hair its ends give. Either way round.
#[test]
fn run_closing_a_hair_past_its_start_is_a_whole_turn() {
    let past = TAU + 1e-8;
    assert_runs_turn_as_their_moves(&chords(10.0, 72, 0.0, past, 0.0));
    assert_runs_turn_as_their_moves(&chords(10.0, 72, 0.0, -past, 0.0));
}

#[test]
fn clockwise_is_g2() {
    let moves = chords(10.0, 72, 0.0, -TAU, 0.0);
    let fitted = fit(&moves, TOL);
    let arcs = arcs_of(&fitted);
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

/// The plaque's stay-down link into its wall ring, 0.006 mm, the
/// ring's shallow corner arc (1.825 mm radius, 2.7°) and its 40 mm
/// side all lie on a 154.75 mm circle, which bows 1.3 mm off the side
/// into the wall. One full chord step is not a run.
#[test]
fn long_side_after_a_fragment_is_not_an_arc() {
    let moves = vec![
        Move::rapid(pt(131.817, 56.915, 0.0)),
        Move::linear(pt(131.823, 56.915, 0.0), FEED),
        Move {
            kind: MoveKind::ArcCcw {
                centre: Point::new(130.0, 57.0),
            },
            to: pt(131.825, 57.0, 0.0),
            feed_mm_min: Some(FEED),
        },
        Move::linear(pt(131.825, 97.0, 0.0), FEED),
    ];
    let fitted = fit(&moves, TOL);
    assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
}

/// With the chord's ends unequally far from the centre, the arc bows
/// farthest from it square to the chord, not towards its middle. A 30°
/// arc of a 10 mm circle over a chord from (10, 0), on the circle, to
/// 9.5 mm out at 30°: the farthest point is at 20.47°, `r` less the
/// line's distance from the centre, 0.63121 mm. Towards the middle
/// (14.61°) and the ten samples it read 0.62843 mm, short by 0.0028 mm;
/// the fitter and S12 would have let such an arc stray that far past
/// the tolerance.
#[test]
fn arc_chord_deviation_is_found_square_to_the_chord() {
    let o = Point::new(0.0, 0.0);
    let r = 10.0;
    let sweep = 30.0_f64.to_radians();
    let a = Point::new(r, 0.0);
    let b = Point::new(9.5 * sweep.cos(), 9.5 * sweep.sin());
    let line_mm = (a - o).cross(b - o).abs() / a.distance_to(b);
    assert_approx_eq!(
        arc_chord_deviation_mm(o, r, 0.0, sweep, a, b),
        r - line_mm,
        1e-9
    );
    assert_approx_eq!(
        arc_chord_deviation_mm(o, r, sweep, -sweep, b, a),
        r - line_mm,
        1e-9
    );
}

/// A chord between two points on the circle is bowed off by the arc's
/// sagitta, `r(1 − cos(sweep/2))`, either way round; ends off the
/// circle add how far they are off.
#[test]
fn arc_chord_deviation_is_the_sagitta() {
    let o = Point::new(0.0, 0.0);
    let (r, sweep) = (10.0, FRAC_PI_2);
    let (a, b) = (Point::new(r, 0.0), Point::new(0.0, r));
    let sagitta = r * (1.0 - (sweep / 2.0).cos());
    assert_approx_eq!(
        arc_chord_deviation_mm(o, r, 0.0, sweep, a, b),
        sagitta,
        1e-9
    );
    assert_approx_eq!(
        arc_chord_deviation_mm(o, r, FRAC_PI_2, -sweep, b, a),
        sagitta,
        1e-9
    );
    let inside = Point::new(r - 0.5, 0.0);
    assert!(arc_chord_deviation_mm(o, r, 0.0, sweep, inside, b) >= 0.5 - 1e-9);
    // A 40 mm side under a 154.75 mm arc: 1.3 mm.
    let r = 154.75;
    let half = (20.0_f64 / r).asin();
    let (a, b) = (
        Point::new(r * half.cos(), -20.0),
        Point::new(r * half.cos(), 20.0),
    );
    let bow = arc_chord_deviation_mm(o, r, -half, 2.0 * half, a, b);
    assert_approx_eq!(bow, r * (1.0 - half.cos()), 1e-9);
    assert!(bow > 1.29, "{bow}");
}

/// A 7 mm square's four corners lie on one circle, with even 90° steps,
/// so every vertex gate passes; but each side is a chord that circle
/// bows 1.45 mm off. A run's every straight move must lie within the
/// tolerance of its arc, not only its ends.
#[test]
fn square_is_not_fitted_as_a_circle() {
    let moves = vec![
        Move::rapid(pt(30.0, 20.0, 0.0)),
        Move::linear(pt(37.0, 20.0, 0.0), FEED),
        Move::linear(pt(37.0, 27.0, 0.0), FEED),
        Move::linear(pt(30.0, 27.0, 0.0), FEED),
        Move::linear(pt(30.0, 20.0, 0.0), FEED),
    ];
    let fitted = fit(&moves, TOL);
    assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
    assert_eq!(lines_of(&fitted), 4);
}

/// An 8.45 mm straight move, a 0.03 mm corner arc and a 0.002 mm
/// straight move (the moves of the Pocket below) all lie on one 33 mm
/// circle, and the step gate leaves the corner arc out of its median.
/// The long chord's own deviation, 0.27 mm under that circle, refuses
/// the run, as does #213's gate of two full chord steps; the Pocket
/// below needs the deviation gate for its other runs.
#[test]
fn long_chord_with_tiny_corner_is_not_fitted() {
    let z = 5.0;
    let moves = vec![
        Move::rapid(pt(111.538_237_913_749_14, 79.670_914_550_191_23, z)),
        Move::linear(pt(114.736_761_571_381_44, 71.845_191_023_101_12, z), FEED),
        Move {
            kind: MoveKind::ArcCcw {
                centre: Point::new(118.204_085_241_192_94, 73.262_352_867_390_8),
            },
            to: pt(114.748_638_699_674_87, 71.816_472_401_080_93, z),
            feed_mm_min: Some(FEED),
        },
        Move::linear(pt(114.750_569_127_027_2, 71.817_280_159_837_1, z), FEED),
    ];
    let fitted = fit(&moves, TOL);
    assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
    assert_eq!(moves_of(&fitted).len(), 4);
}

/// #233's minimal case, from the Pocket property test: a raster Pocket
/// round a rectangular island, rotated, exports in mm and in inch with
/// arc fitting on. Before, a fitted arc bowed 0.27 mm off a straight
/// move, and the safety pass refused it (S9 in mm, S12 in inch).
#[test]
fn rotated_raster_pocket_with_island_exports_with_arc_fitting() {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 200.0,
        height_mm: 200.0,
        thickness_mm: Some(6.0),
    };
    project.arc_fit_tolerance_mm = Some(0.01);
    let (hx, hy) = (5.557_357_878_931_814, 4.211_528_476_684_152);
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Pocket".into(),
        shape: None,
        profiles: vec![
            Polyline::rect(
                Point::new(-16.293_109_341_906_767, -17.302_636_203_267_785),
                2.0 * 16.293_109_341_906_767,
                2.0 * 17.302_636_203_267_785,
            ),
            Polyline::rect(Point::new(-hx, -hy), 2.0 * hx, 2.0 * hy),
        ],
    };
    project.definitions = std::collections::BTreeMap::from([(def.id.clone(), def)]);
    project.instances = vec![FeatureInstance {
        transform: Transform::rotate(0.925_667_480_965_779_2_f64.atan2(-0.378_338_095_732_710_93))
            .then(Transform::translate(120.0, 80.0)),
        definition: DefinitionId::new("d1"),
        ..project.instances[0].clone()
    }];
    let tool = &mut project.tools[0];
    tool.diameter_mm = 7.491_510_161_281_357;
    tool.feed_mm_min = Some(3982.583);
    tool.plunge_mm_min = Some(596.066);
    let pocket = dc_project::PocketOp {
        targets: vec![project.instances[0].id.clone()],
        tool: project.tools[0].id.clone(),
        strategy: dc_project::ClearanceStrategy::Raster {
            angle_deg: 51.035_950_808_994_54,
        },
        step_over_fraction: Some(0.992_635_621_218_441_3),
        depth_mm: Some(1.0),
        ..dc_project::test_support::sample_pocket()
    };
    project.operations = vec![Operation::Pocket(pocket.clone())];
    for units in [dc_project::Units::Mm, dc_project::Units::Inch] {
        project.display_units = units;
        let (toolpath, _) = dc_cam::generate_pocket(&project, &pocket).unwrap();
        emit(
            &project,
            &[(
                Operation::Pocket(pocket.clone()),
                project.tools[0].clone(),
                toolpath,
            )
                .into()],
            "2026-10-21",
        )
        .unwrap_or_else(|e| panic!("{units:?}: {e:?}"));
    }
}

/// A regular octagon of 10 mm radius, its chords bowing 0.76 mm from
/// the circle through its corners, stays eight lines; the same circle
/// as 72 chords (0.0095 mm each) is fitted.
#[test]
fn regular_octagon_of_long_chords_is_not_fitted() {
    assert_eq!(runs_of(&fit(&chords(10.0, 8, 0.0, TAU, 0.0), TOL)), 0);
    assert_eq!(runs_of(&fit(&chords(10.0, 72, 0.0, TAU, 0.0), TOL)), 1);
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
    let mut moves = chords(10.0, 72, 0.0, TAU, 0.0);
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
    let mut moves = chords(10.0, 18, 0.0, FRAC_PI_2, 0.0);
    moves.extend(chords(10.0, 18, PI, PI + FRAC_PI_2, 0.0));
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
    let moves = chords(10.0, 64, 0.0, 3.0 * FRAC_PI_2, 0.0);
    let fitted = fit(&moves, TOL);
    let arcs = arcs_of(&fitted);
    assert_eq!(arcs.len(), 3, "{fitted:?}");
    for arc in arcs {
        assert!(sweep_deg(arc) <= 90.1, "{}", sweep_deg(arc));
    }
}

#[test]
fn split_arc_offsets_relative() {
    let moves = chords(10.0, 72, 0.0, TAU, 0.0);
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
    let moves = chords(5.0, 6, 0.0, PI / 6.0, 0.0);
    assert_eq!(arcs_of(&fit(&moves, TOL)).len(), 1);
}

#[test]
fn full_circle_loop_is_four_arcs() {
    let moves = chords(10.0, 72, 0.0, TAU, 0.0);
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
    moves.extend(chords(r, 36, 0.0, PI, 0.0).into_iter().skip(1));
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
                    let emitted = resolve_emitted_arc(arc.end.xy(), arc.centre, start, opts);
                    worst = worst.max(controller_delta(start, target, f(emitted.i), f(emitted.j)));
                    position = Some(target);
                }
            }
        }
    }
    worst
}

/// #506 (line art `0x54`'s V-Carve from a cleared start depth, 15°
/// bit): an arc whose radius is a few units in the last place is not
/// written, so its run falls back to its moves. A V-Carve chain round
/// a tip a few thousandths across was fitted as `G3` pieces of radius
/// 0.002 mm, `I0.002 J0.000`; centre, start and end all round to the
/// same units, and the safety pass could not match the written arc to
/// its moves (`ArcFitViolation`). A quarter arc of radius 2 units is
/// refused, one of 10 units still written.
#[test]
fn arc_a_few_units_across_is_not_written() {
    let opts = mm3_opts(ArcFormat::Ij);
    let tiny = resolve_emitted_arc(
        Point::new(10.002, 10.002),
        Point::new(10.002, 10.0),
        Point::new(10.0, 10.0),
        &opts,
    );
    assert!(!tiny.accepted, "{tiny:?}");
    let small = resolve_emitted_arc(
        Point::new(10.010, 10.010),
        Point::new(10.010, 10.0),
        Point::new(10.0, 10.0),
        &opts,
    );
    assert!(small.accepted, "{small:?}");
}

#[test]
fn ij_derived_from_previous_emitted_point() {
    // The exporter's un-rounded idea of the start is 10.0004; the
    // controller is at the formatted 10.000.
    let emitted = resolve_emitted_arc(
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
    let (out, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::Ij), None, &identity, TOL);
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
    let mut moves = chords(10.0, 72, 0.0, TAU, -10.5);
    moves.extend(issue_447_moves());
    let fitted = fit(&moves, TOL);
    let good = arcs_of(&fitted).len();
    assert!(good > 0);
    let opts = mm3_opts(ArcFormat::Ij);
    let (clean, runs) = apply_fallback(fitted.clone(), &opts, None, &identity, TOL);
    assert_eq!(runs, 0, "well-formed spans do not fall back");
    assert!(worst_emitted_delta(&clean, &opts) <= ARC_RADIUS_CONSISTENCY_MM);

    let mut mixed = fitted;
    mixed.push(Fitted::Move(Move::rapid(pt(10.0, 0.0, 0.0))));
    mixed.push(Fitted::Arc(inconsistent_run()));
    let (out, runs) = apply_fallback(mixed, &opts, None, &identity, TOL);
    assert_eq!(runs, 1, "only the offending run falls back");
    assert_eq!(arcs_of(&out).len(), good);
}

#[test]
fn r_format_rejects_unrepresentable_chord() {
    let bad = resolve_emitted_arc(
        Point::new(10.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.0, 0.0),
        &mm3_opts(ArcFormat::R),
    );
    assert!(!bad.accepted, "a chord longer than the diameter");
    let fitted = fit(&issue_447_moves(), TOL);
    let (_, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::R), None, &identity, TOL);
    assert_eq!(runs, 0, "the #447 span is representable in R format");
}

/// #355's run: 15 chords of a 9.2417 mm ring round a circular island,
/// each bowed 0.0099 mm by the arc through its ends, written in inch
/// from the work origin at (192.539, 119.461) mm.
fn island_ring_moves() -> Vec<Move> {
    let z = 1.101_992_404_702_241_6;
    let mut moves = vec![Move::rapid(pt(
        198.036_678_761_243_82,
        49.030_722_826_719_284,
        z,
    ))];
    moves.extend(
        [
            (197.211_803_972_721_1, 48.811_046_928_167_34),
            (196.410_717_070_102_7, 48.516_198_277_473_45),
            (195.640_252_679_586_4, 48.148_692_429_065_704),
            (194.906_984_120_607_38, 47.711_664_825_677_87),
            (194.217_167_347_669_6, 47.208_844_006_061_554),
            (193.576_687_693_595_9, 46.644_519_865_512_85),
            (192.991_009_473_800_66, 46.023_507_028_818_13),
            (192.465_129_464_864_73, 45.351_103_752_851_486),
            (192.003_534_346_818_92, 44.633_046_746_253_97),
            (191.610_162_228_345_87, 43.875_462_204_217_91),
            (191.288_369_268_178_94, 43.084_813_594_818_115),
            (191.040_900_856_256_48, 42.267_846_465_110_78),
            (190.869_868_308_305_74, 41.431_530_833_244_324),
            (190.776_730_835_437_77, 40.583_001_911_640_17),
            (190.762_283_056_974_4, 39.729_499_012_231_83),
        ]
        .map(|(x, y)| Move::linear(pt(x, y, z), FEED)),
    );
    moves
}

fn island_ring_to_machine(p: Point) -> Point {
    Point::new(
        (p.x - 192.539_444_272_137_16) / 25.4,
        (p.y - 119.461_040_582_034_09) / 25.4,
    )
}

fn exact(value: f64) -> Option<f64> {
    Some(value)
}

/// The fitted arc lies within 0.0099 mm of every chord, but in inch
/// the I/J snap moves its centre two quanta each way (I 0.0773 →
/// 0.0771, J −0.3556 → −0.3554, 0.0072 mm) to agree on the radius to
/// 0.0002 mm instead of an already accepted 0.0013 mm. The arc as
/// written then bows 0.0126 mm off a chord, past the 0.0125 mm S12
/// allows, so the run is written as its moves. Unrounded, it is kept.
#[test]
fn snapped_arc_off_its_chords_falls_back() {
    let fitted = fit(&island_ring_moves(), TOL);
    assert_eq!(runs_of(&fitted), 1, "{fitted:?}");
    let (out, runs) = apply_fallback(
        fitted.clone(),
        &inch4_opts(),
        None,
        &island_ring_to_machine,
        TOL,
    );
    assert_eq!(runs, 1, "{out:?}");
    assert_eq!(lines_of(&out), 15);
    let unrounded = EmitOptions {
        format: &exact,
        arc_format: ArcFormat::Ij,
        mm_per_unit: 25.4,
        quantum: 0.0,
    };
    let (out, runs) = apply_fallback(fitted, &unrounded, None, &island_ring_to_machine, TOL);
    assert_eq!(runs, 0, "{out:?}");
    assert_eq!(runs_of(&out), 1);
}

/// A point at `deg` on a 2 mm circle about the origin.
fn on_2_mm_circle(deg: f64) -> Point3 {
    let t = deg.to_radians();
    pt(2.0 * t.cos(), 2.0 * t.sin(), 0.0)
}

/// The degrees 0.0002 mm, too short to write, turns on that circle.
fn sliver_deg() -> f64 {
    (0.0002_f64 / 2.0).to_degrees()
}

/// `moves` fit one run, which falls back to its three moves.
fn three_moves_fall_back(moves: &[Move]) {
    let fitted = fit(moves, TOL);
    assert_eq!(runs_of(&fitted), 1, "{fitted:?}");
    let (out, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::Ij), None, &identity, TOL);
    assert_eq!(runs, 1, "{out:?}");
    assert_eq!(lines_of(&out), 3);
}

/// Two 10° chords of a 2 mm circle and a third 0.0002 mm long, too
/// short to write. The fit takes all three, but S12 counts only the
/// moves up to the first vertex at the run's written end, two, so the
/// run is written as its moves.
#[test]
fn run_ending_in_a_sliver_falls_back() {
    three_moves_fall_back(&[
        Move::rapid(on_2_mm_circle(0.0)),
        Move::linear(on_2_mm_circle(10.0), FEED),
        Move::linear(on_2_mm_circle(20.0), FEED),
        Move::linear(on_2_mm_circle(20.0 + sliver_deg()), FEED),
    ]);
}

/// The sliver first. S12 reads it as a straight move written as
/// nothing, before the arcs, and counts two moves under them.
#[test]
fn run_starting_with_a_sliver_falls_back() {
    three_moves_fall_back(&[
        Move::rapid(on_2_mm_circle(0.0)),
        Move::linear(on_2_mm_circle(sliver_deg()), FEED),
        Move::linear(on_2_mm_circle(10.0), FEED),
        Move::linear(on_2_mm_circle(20.0), FEED),
    ]);
}

#[test]
fn inch_output_uses_millimetre_budget() {
    // 0.004 in of radius disagreement is 0.1016 mm: legal if the budget
    // were applied in inches, far past GRBL's 0.005 mm.
    let as_inch = resolve_emitted_arc(
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
                        let start = position.unwrap_or(Point::new(f(arc.start.x), f(arc.start.y)));
                        let resolved = resolve_emitted_arc(arc.end.xy(), arc.centre, start, &opts);
                        let naive_i = f(arc.centre.x - start.x);
                        let naive_j = f(arc.centre.y - start.y);
                        let target = Point::new(f(arc.end.x), f(arc.end.y));
                        let naive =
                            controller_delta(start, target, naive_i, naive_j) * opts.mm_per_unit;
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
            shape: None,
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
        construction: false,
        folder: None,
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
        on_holes: false,
        avoid_corners: false,
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
    dc_cam::test_support::assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
    let (program, warnings) = emit(
        project,
        &[(Operation::Profile(op), tool, toolpath.clone()).into()],
        "2026-10-15",
    )
    .unwrap();
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

/// The coaster's program moves split into laps: a lap starts with the
/// first move that cuts deeper than any before it (the plunge to its
/// pass) and runs to the next such move or the retract. Rapids are
/// left out.
fn laps(text: &str) -> Vec<Vec<crate::parse::MachineMove>> {
    let mut laps: Vec<Vec<crate::parse::MachineMove>> = Vec::new();
    let mut deepest = f64::INFINITY;
    for m in parse_motion(text).unwrap() {
        if m.kind == MotionKind::Rapid {
            continue;
        }
        let z = m.to.z.unwrap();
        if z < deepest - 1e-6 {
            deepest = z;
            laps.push(Vec::new());
        }
        laps.last_mut()
            .expect("a lap starts at the first cut")
            .push(m);
    }
    laps
}

/// Functional acceptance 37, per lap: the coaster's three laps (12 mm
/// in 4 mm passes) are two full untabbed laps of at most 8 `G2`/`G3`
/// each, and the final pass, whose four tabs (3 mm high, so their
/// tops at Z −8.8 lie above it) split the lap into 5 runs: one run
/// per segment between tabs, so at least 9 arcs.
#[test]
fn coaster_arcs_counted_per_lap() {
    let project = coaster_project();
    let (text, warnings, _) = export_coaster(&project);
    assert!(warnings.is_empty(), "{warnings:?}");
    let laps = laps(&text);
    let floors: Vec<f64> = laps.iter().map(|lap| lap[0].to.z.unwrap()).collect();
    assert_eq!(floors.len(), 3, "three laps: {floors:?}\n{text}");
    for (k, z) in [-4.0, -8.0, -12.0].into_iter().enumerate() {
        assert_approx_eq!(floors[k], z, 1e-6);
    }
    let arcs = |lap: &[crate::parse::MachineMove]| {
        lap.iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count()
    };
    for (k, lap) in laps.iter().take(2).enumerate() {
        let tab_moves = lap
            .iter()
            .filter(|m| (m.to.z.unwrap() - floors[k]).abs() > 1e-6)
            .count();
        assert_eq!(tab_moves, 0, "lap {k} at Z {} has no tabs", floors[k]);
        let n = arcs(lap);
        assert!(
            (1..=8).contains(&n),
            "lap {k} at Z {} has {n} arcs, want 1..=8",
            floors[k]
        );
    }
    let tabbed = &laps[2];
    let on_tabs = tabbed
        .iter()
        .filter(|m| (m.to.z.unwrap() - (-8.8)).abs() <= 1e-6)
        .count();
    assert!(on_tabs >= 4, "the final lap rises onto 4 tabs: {on_tabs}");
    let n = arcs(tabbed);
    assert!(n >= 9, "the tabbed lap has {n} arcs, want at least 9");
}

/// Functional acceptance 37: "the same number of `G1` moves as before
/// on a post-processor without arc support". With `arc_interpolation`
/// off, `arc_fit_tolerance_mm: 0.01` writes as many `G1` moves as
/// `null`, and neither writes a `G2` or `G3`.
#[test]
fn no_arc_support_keeps_the_g1_count() {
    let mut counts = Vec::new();
    for tolerance in [Some(0.01), None] {
        let mut project = coaster_project();
        project.arc_fit_tolerance_mm = tolerance;
        let mut post = project.post_processor.resolve().unwrap();
        post.motion.arc_interpolation = false;
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("the coaster has a profile operation");
        };
        let tool = project.tools[0].clone();
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        let (program, _) = crate::emit::emit_with(
            &project,
            &post,
            &[(Operation::Profile(op), tool, toolpath).into()],
            "2026-10-15",
        )
        .unwrap();
        let moves = parse_motion(&program.text).unwrap();
        let arcs = moves
            .iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count();
        assert_eq!(arcs, 0, "{tolerance:?}: no G2/G3\n{}", program.text);
        let lines = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .count();
        counts.push(lines);
    }
    assert_eq!(
        counts[0], counts[1],
        "G1 moves with tolerance 0.01 vs null: {counts:?}"
    );
    assert!(counts[0] >= 3 * 160, "every facet is a G1: {counts:?}");
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
    let n = 72;
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

/// One turn of a tiny helix ending at `end_angle_rad` round `centre`
/// at `floor_z_mm`, each of its 48 chords falling `drop_mm`, then its
/// flat revolution back to where it ended, as `dc_cam::entry` writes
/// them; plunged into at the helix start from `safe_z_mm` and left by
/// a rapid up to it.
struct TinyHelix {
    centre: Point,
    radius_mm: f64,
    end_angle_rad: f64,
    ccw: bool,
    floor_z_mm: f64,
    drop_mm: f64,
}

impl TinyHelix {
    fn toolpath(&self, feed_mm_min: f64, plunge_mm_min: f64, safe_z_mm: f64) -> dc_cam::Toolpath {
        let n = dc_cam::entry::HELIX_SEGMENTS_PER_REV;
        let sense = if self.ccw { 1.0 } else { -1.0 };
        let at = |angle: f64| {
            self.centre + dc_geometry::Vec2::new(angle.cos(), angle.sin()) * self.radius_mm
        };
        let endpoint = at(self.end_angle_rad);
        let start_angle = self.end_angle_rad - sense * TAU;
        let start_z = self.floor_z_mm + self.drop_mm * n as f64;
        let from = at(start_angle);
        let mut moves = vec![
            Move::rapid(pt(from.x, from.y, safe_z_mm)),
            Move::linear(pt(from.x, from.y, start_z), plunge_mm_min),
        ];
        let mut current = pt(from.x, from.y, start_z);
        for k in 1..=n {
            let next = if k == n {
                pt(endpoint.x, endpoint.y, self.floor_z_mm)
            } else {
                let t = k as f64 / n as f64;
                let p = at(start_angle + sense * TAU * t);
                pt(p.x, p.y, start_z - self.drop_mm * n as f64 * t)
            };
            let (dx, dy, dz) = (next.x - current.x, next.y - current.y, current.z - next.z);
            let length = (dx * dx + dy * dy + dz * dz).sqrt();
            moves.push(Move::linear(
                next,
                feed_mm_min.min(plunge_mm_min * length / dz),
            ));
            current = next;
        }
        for k in 1..=n {
            let p = if k == n {
                endpoint
            } else {
                at(self.end_angle_rad + sense * TAU * k as f64 / n as f64)
            };
            moves.push(Move::linear(pt(p.x, p.y, self.floor_z_mm), feed_mm_min));
        }
        moves.push(Move::rapid(pt(endpoint.x, endpoint.y, safe_z_mm)));
        dc_cam::Toolpath { moves }
    }
}

/// The sample project on 240 × 160 × `thickness_mm` stock with
/// `origin`, written for `post` in `units` with arc fitting at
/// 0.01 mm, its tool cutting at `feed` and plunging at `plunge`; its
/// one operation the sample Pocket of a 220 × 140 mm rectangle at
/// (10, 10), so a helix anywhere near (40, 40) is in its material.
fn tiny_helix_project(
    thickness_mm: f64,
    origin: dc_project::MachineOrigin,
    post: &str,
    units: dc_project::Units,
    (feed, plunge): (f64, f64),
) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 240.0,
        height_mm: 160.0,
        thickness_mm: Some(thickness_mm),
    };
    project.origin = origin;
    project.display_units = units;
    project.arc_fit_tolerance_mm = Some(0.01);
    project.post_processor = PostProcessorRef::latest(post).unwrap();
    project.tools[0].feed_mm_min = Some(feed);
    project.tools[0].plunge_mm_min = Some(plunge);
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![Polyline::rect(Point::new(0.0, 0.0), 220.0, 140.0)];
    }
    for instance in &mut project.instances {
        instance.z_top_mm = Some(thickness_mm);
    }
    project.operations = vec![Operation::Pocket(dc_project::test_support::sample_pocket())];
    project
}

/// #262, from the Pocket property's committed seed (`9cea68b9…`) and
/// `PROPTEST_RNG_SEED=105`: a helix 0.17 mm across and its flat
/// revolution. The flat revolution is fitted as a closed run of four
/// 90° arcs, which S12 refused: its chords are 7.5° and its pieces'
/// angle slack (the tolerance over the radius) as much, so a vertex
/// one chord past a piece's end counted as on that piece, and the
/// next chord's deviation was measured from the start of the next
/// piece, not from that vertex: over arc the chord does not span,
/// 0.011023 mm off it against 0.011001 allowed. Both cases export in
/// inch and in mm.
#[test]
fn small_helix_flat_revolution_in_inches_exports() {
    use dc_project::{MachineOrigin, OriginXy, OriginZ, Units};
    // The seed `9cea68b9…`: LinuxCNC, top left and stock top, inch;
    // its moves 23720 to 23816.
    let inch_case = (
        TinyHelix {
            centre: Point::new(40.206_913_519_649_284, 39.550_644_954_268_115),
            radius_mm: 40.293_501_470_256 - 40.206_913_519_649_284,
            end_angle_rad: 0.0,
            ccw: true,
            floor_z_mm: 1.252_841_074_242_111_2,
            drop_mm: 0.001_689_111_677_942_3,
        },
        5.872_515_626_754_48,
        MachineOrigin {
            xy: OriginXy::TopLeft,
            z: OriginZ::StockTop,
        },
        "linuxcnc",
        (1_787.111_112_547_554_7, 2_136.427_614_162_320_4),
    );
    // `PROPTEST_RNG_SEED=105`: Mach3, bottom left and stock bottom, mm.
    let mm_case = (
        TinyHelix {
            centre: Point::new(43.157_111_168_893_31, 47.102_805_434_202_09),
            radius_mm: 43.240_669_225_108_84 - 43.157_111_168_893_31,
            end_angle_rad: 0.0,
            ccw: false,
            floor_z_mm: 3.145_858_777_472_146_7,
            drop_mm: 0.008_047_045_301_816,
        },
        3.980_585_106_286_197_7,
        MachineOrigin {
            xy: OriginXy::BottomLeft,
            z: OriginZ::StockBottom,
        },
        "mach3",
        (942.682_590_680_295_1, 2_284.956_191_250_548_7),
    );
    let mut failures = Vec::new();
    for (helix, thickness_mm, origin, post, rates) in [inch_case, mm_case] {
        for units in [Units::Inch, Units::Mm] {
            let project = tiny_helix_project(thickness_mm, origin.clone(), post, units, rates);
            let op = project.operations[0].clone();
            let toolpath = helix.toolpath(rates.0, rates.1, thickness_mm + op.safe_z_mm());
            let tool = project.tools[0].clone();
            let result = emit(&project, &[(op, tool, toolpath).into()], "2026-10-21");
            let program = match result {
                Ok((program, _)) => program,
                Err(e) => {
                    failures.push(format!("{post} in {units:?}: {e:?}"));
                    continue;
                }
            };
            let arcs = parse_motion(&program.text)
                .unwrap()
                .iter()
                .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
                .count();
            assert!(
                arcs > 0,
                "{post} in {units:?}: the flat revolution is fitted"
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A level run like the floor rings of a V-bit's long level stretch:
/// `rings` concentric rings, each an ellipse, walked by moves whose
/// lengths are drawn log-uniform from `shortest_mm` to `longest_mm` by
/// a fixed generator from `seed`, each ring joined to the next by a
/// short step in. From 0.002 to 0.4 mm the lengths vary as the stress
/// case's do (line art `0x4640000`), so almost every window fails the
/// step uniformity gate though its points lie on a circle.
fn dense_level_rings(
    rings: usize,
    outer_mm: f64,
    shortest_mm: f64,
    longest_mm: f64,
    seed: u64,
) -> Vec<Point> {
    let mut state = seed;
    let mut uniform = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1_u64 << 53) as f64
    };
    let mut points = Vec::new();
    for ring in 0..rings {
        let a = outer_mm - 0.3 * ring as f64;
        let b = 0.7 * a;
        let mut angle = 0.0;
        while angle < TAU {
            points.push(Point::new(a * angle.cos(), b * angle.sin()));
            let length_mm = shortest_mm * (longest_mm / shortest_mm).powf(uniform());
            angle += length_mm / b;
        }
    }
    points
}

/// The search as it was before #480: every window from each start, a
/// circle fit and every gate on each. The reference the search must
/// agree with exactly.
fn find_arc_runs_exhaustive(
    points: &[Point],
    mids: &[Option<Point>],
    tolerance_mm: f64,
) -> Vec<FoundArc> {
    let n = points.len();
    let mut found = Vec::new();
    let mut i = 0;
    while i + MIN_RUN_POINTS <= n {
        let max_end = (n - 1).min(i + MAX_CANDIDATE_POINTS - 1);
        let mut best = None;
        for end in (i + MIN_RUN_POINTS - 1..=max_end).rev() {
            let sub = &points[i..=end];
            let Some((centre, radius_mm)) = fit_circle_kasa(sub) else {
                continue;
            };
            let (centre, radius_mm) =
                constrain_to_endpoints(centre, radius_mm, sub[0], sub[sub.len() - 1])
                    .unwrap_or((centre, radius_mm));
            if gates_pass(sub, &mids[i..end], centre, radius_mm, tolerance_mm) {
                best = Some(FoundArc {
                    start_index: i,
                    end_index: end,
                    centre,
                    clockwise: cross_sum(sub, centre) < 0.0,
                });
                break;
            }
        }
        match best {
            Some(arc) => {
                i = arc.end_index;
                found.push(arc);
            }
            None => i += 1,
        }
    }
    found
}

/// A long dense level run cost about `n · 128` circle fits, each
/// judged by every gate over up to 128 points, and the stress case's
/// V-bit run (49 445 moves) took 41 s to fit in a release build. The
/// search now fits a few windows a point, and finds exactly the arcs
/// the exhaustive search finds.
#[test]
fn fit_of_a_long_dense_level_run_is_bounded() {
    let points = dense_level_rings(8, 8.0, 0.002, 0.4, 0x0480);
    assert!(points.len() > 3000, "{} points", points.len());
    let mids = vec![None; points.len() - 1];
    let mut work = SearchWork::default();
    let found = find_arc_runs_counted(&points, &mids, TOL, &mut work);
    assert!(
        work.windows <= 4 * points.len(),
        "{} windows fitted for {} points",
        work.windows,
        points.len()
    );
    assert_eq!(found, find_arc_runs_exhaustive(&points, &mids, TOL));
}

/// Skipping the windows whose moves are too uneven for any circle never
/// changes what the search finds: on runs whose lengths vary by less
/// than, about, and more than the step uniformity gate's ratio of four,
/// with and without shallow corner arcs, at two tolerances, the arcs
/// are the exhaustive search's, bit for bit.
#[test]
fn uneven_window_skip_finds_the_exhaustive_arcs() {
    let ranges = [(0.05, 0.1), (0.05, 0.2), (0.05, 0.25), (0.01, 1.0)];
    for (seed, &(shortest_mm, longest_mm)) in (1_u64..).zip(&ranges) {
        for tolerance_mm in [TOL, 0.001] {
            for corners in [false, true] {
                let points = dense_level_rings(2, 4.0, shortest_mm, longest_mm, seed);
                // Every third move a shallow corner arc: its midpoint
                // is on the ring, and the gates leave its step out of
                // the median.
                let mids: Vec<Option<Point>> = points
                    .windows(2)
                    .enumerate()
                    .map(|(k, pair)| {
                        (corners && k % 3 == 2).then(|| {
                            Point::new((pair[0].x + pair[1].x) / 2.0, (pair[0].y + pair[1].y) / 2.0)
                        })
                    })
                    .collect();
                assert_eq!(
                    find_arc_runs(&points, &mids, tolerance_mm),
                    find_arc_runs_exhaustive(&points, &mids, tolerance_mm),
                    "lengths {shortest_mm} to {longest_mm} mm, tolerance {tolerance_mm}, corners {corners}"
                );
            }
        }
    }
}
