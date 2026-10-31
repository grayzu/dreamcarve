// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S12 on fitted arcs whose circle is about as small as the arc-fit
//! allowance (#453).
//!
//! S12 places each toolpath vertex on a fitted chain's pieces in turn. A
//! vertex counts as on a piece up to the angle the allowance (the arc-fit
//! tolerance plus one unit in the last place) spans past its ends, so that
//! rounding at a joint does not fail it. On a circle whose radius is near
//! the allowance that slack is wider than the piece itself, and a walk
//! that keeps a vertex on the current piece while it still holds it never
//! reaches the last piece: it ran past the chain's end into the next
//! toolpath move and refused arcs that were within 0.002 mm of their
//! moves. The V wall of a male Inlay's rounded band edge, with no
//! clearing pass, has such runs.

use super::*;
use crate::arcfit::{self, Fitted};
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use dc_project::{InlayOp, Project};

/// The tolerance of these tests, and the sample project's, mm.
const ARC_FIT_MM: f64 = 0.01;

/// The radius of the small circle, mm: less than the 0.011 mm allowance,
/// so the angle slack (0.011 / 0.008 rad, 79°) is wider than a 45° piece.
const SMALL_R_MM: f64 = 0.008;

/// The point at `deg` degrees on the circle of `radius_mm` about
/// (50, 50), at Z 5.
fn on_small_circle(deg: f64, radius_mm: f64) -> Point3 {
    let a = deg.to_radians();
    Point3::new(50.0 + radius_mm * a.cos(), 50.0 + radius_mm * a.sin(), 5.0)
}

/// Rapid in over (50.008, 50), plunge to Z 5, chords of the small circle
/// counter-clockwise to each of `degrees` (vertex `bump`, 1-based, if any,
/// pushed 0.05 mm outwards), then a 1 mm straight cut to (49, 50.008),
/// which no fitted run takes, and retract.
fn small_circle_run(degrees: &[f64], bump: Option<usize>) -> Toolpath {
    let mut moves = vec![
        Move::rapid(Point3::new(50.008, 50.0, 11.0)),
        Move::linear(Point3::new(50.008, 50.0, 5.0), 300.0),
    ];
    for (k, &deg) in degrees.iter().enumerate() {
        let r = if bump == Some(k + 1) {
            SMALL_R_MM + 0.05
        } else {
            SMALL_R_MM
        };
        moves.push(Move::linear(on_small_circle(deg, r), 1000.0));
    }
    moves.push(Move::linear(Point3::new(49.0, 50.008, 5.0), 1000.0));
    moves.push(Move::rapid(Point3::new(49.0, 50.008, 11.0)));
    Toolpath { moves }
}

/// The program for [`small_circle_run`] with two `G3` pieces over the
/// quarter circle from 0° to 90°, meeting at 45° (rounded to
/// (50.006, 50.006)), as the emitter writes a fitted run: each piece's I
/// and J from its own written start. Line 8 is the first piece.
const SMALL_CIRCLE_TEXT: &str = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX50.008 Y50\nG1 Z-1 F300\n\
     G3 X50.006 Y50.006 I-0.008 J0 F1000\nX50 Y50.008 I-0.006 J-0.006\n\
     G1 X49 Y50.008\nG0 Z5\nM5\n";

/// #453, the failing case at the level of the check: four chords of a
/// circle of radius 0.008 mm, every vertex on it, written as two 45°
/// `G3` pieces. The chain lies within 0.0005 mm of every chord, far
/// inside the 0.01 mm tolerance. Every vertex is within the angle slack
/// (79°) of the first piece, so a walk that stays on a piece while it
/// holds the vertex never reached the second, missed the chain's end at
/// the 90° vertex, and refused the straight cut after it as off the
/// circle. A vertex past a piece's end that the next piece holds is on
/// the next piece.
#[test]
fn fitted_arc_on_a_circle_within_the_allowance_passes_s12() {
    let toolpath = small_circle_run(&[22.5, 45.0, 67.5, 90.0], None);
    check_against_with(SMALL_CIRCLE_TEXT, &toolpath, Some(ARC_FIT_MM)).unwrap();
}

/// #453: S12 still refuses a fitted arc that does not follow its moves,
/// on the small circle as on any other. With the 67.5° vertex 0.05 mm off
/// the circle, five times the tolerance, the same two pieces are refused.
/// So are they when the toolpath has no vertex at 90°, where the chain
/// ends: its last chord ends at 67.5° and the 1 mm cut goes on from
/// there. And on a 10 mm circle (as `arc_off_the_vertices_fails_s6`), one
/// vertex 0.05 mm off the arc is refused.
#[test]
fn deviation_beyond_tolerance_is_still_rejected() {
    let violation = |result: &Result<(), ExportError>| {
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. }))
    };
    let bumped = small_circle_run(&[22.5, 45.0, 67.5, 90.0], Some(3));
    let result = check_against_with(SMALL_CIRCLE_TEXT, &bumped, Some(ARC_FIT_MM));
    assert!(violation(&result), "vertex off the circle: {result:?}");

    let short = small_circle_run(&[22.5, 45.0, 67.5], None);
    let result = check_against_with(SMALL_CIRCLE_TEXT, &short, Some(ARC_FIT_MM));
    assert!(
        violation(&result),
        "chain ends between vertices: {result:?}"
    );

    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\n");
    let result = check_against_with(&text, &chord_run(32, FRAC_PI_2, Some(16)), Some(ARC_FIT_MM));
    assert!(violation(&result), "10 mm circle: {result:?}");
}

/// The quick start's "DC" male (`tests/fixtures/dc-male.dcv`, the 60°
/// V-bit, `plug_depth_mm` 2) with no clearing pass and no cut-out, so the
/// band's edge is a V wall the V-bit cuts round the rounded outline,
/// `margin_mm` 12 and `outline_rounding_mm` `rounding_mm`.
fn dc_male_v_wall(rounding_mm: f64) -> (Project, InlayOp) {
    let project = dc_project::load(include_bytes!("../../../../../tests/fixtures/dc-male.dcv"))
        .expect("the DC male loads")
        .project;
    let [Operation::Inlay(op)] = &project.operations[..] else {
        panic!("the DC male has one operation, an Inlay");
    };
    let op = InlayOp {
        clearing: Vec::new(),
        cut_out: None,
        margin_mm: Some(12.0),
        outline_rounding_mm: Some(rounding_mm),
        ..op.clone()
    };
    (project, op)
}

/// The farthest any point of `run`'s sub-arcs lies from the moves it
/// replaces (each a straight move from the vertex before, as the fitter
/// takes a shallow corner arc), mm: from points every 1/64 of each
/// sub-arc to the nearest move, exactly. This is the fitter's promise
/// (`arcfit::fit`) and what S12 measures, over each move's chord.
fn arc_off_moves_mm(run: &arcfit::ArcRun) -> f64 {
    const STEPS: u32 = 64;
    let Some(first) = run.arcs.first() else {
        return f64::INFINITY;
    };
    let mut polyline = vec![first.start.xy()];
    polyline.extend(run.fallback.iter().map(|m| m.to.xy()));
    // The distance from `p` to the straight move from `a` to `b`.
    let to_move = |p: Point, a: Point, b: Point| {
        let d = b - a;
        let length2 = d.dot(d);
        let t = if length2 > 0.0 {
            ((p - a).dot(d) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.distance_to(a + d * t)
    };
    run.arcs
        .iter()
        .flat_map(|arc| {
            let r = arc.start.xy().distance_to(arc.centre);
            let a0 = (arc.start.y - arc.centre.y).atan2(arc.start.x - arc.centre.x);
            let a1 = (arc.end.y - arc.centre.y).atan2(arc.end.x - arc.centre.x);
            let sweep = if arc.clockwise { a0 - a1 } else { a1 - a0 }.rem_euclid(TAU);
            let sign = if arc.clockwise { -1.0 } else { 1.0 };
            (0..=STEPS).map(move |k| {
                let a = a0 + sign * sweep * f64::from(k) / f64::from(STEPS);
                Point::new(arc.centre.x + r * a.cos(), arc.centre.y + r * a.sin())
            })
        })
        .map(|p| {
            polyline
                .windows(2)
                .map(|w| to_move(p, w[0], w[1]))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// The farthest any vertex of the moves `run` replaces lies from its
/// circle, mm: the fitter's other promise, which S12 checks too.
fn vertices_off_circle_mm(run: &arcfit::ArcRun) -> f64 {
    let Some(first) = run.arcs.first() else {
        return f64::INFINITY;
    };
    let r = first.start.xy().distance_to(first.centre);
    run.fallback
        .iter()
        .map(|m| (m.to.xy().distance_to(first.centre) - r).abs())
        .fold(0.0, f64::max)
}

/// #453: the "DC" male with no clearing pass, so the V-bit cuts the
/// band's rounded edge as a V wall, exports with the 0.01 mm arc-fit
/// tolerance and the quick start's 10 mm rounding (before the fix it
/// failed S12, as it did with 3 and 22 mm). Export runs the whole safety
/// pass, so S12 passing is the check that every arc as written follows
/// its moves. Independently, before rounding: every run the fitter makes
/// lies within the tolerance of the moves it replaces, every vertex of
/// them is within it of the run's circle, and some runs are fitted.
#[test]
fn rounded_band_edge_v_wall_exports() {
    let (mut project, op) = dc_male_v_wall(10.0);
    assert_eq!(project.arc_fit_tolerance_mm, Some(ARC_FIT_MM));
    project.operations = vec![Operation::Inlay(op.clone())];
    let (runs, _) = dc_cam::generate_inlay(&project, &op).expect("the inlay generates");

    let ops: Vec<EmitOp> = runs
        .iter()
        .map(|run| EmitOp {
            op: Operation::Inlay(op.clone()),
            tool: project
                .tools
                .iter()
                .find(|t| t.id == run.tool)
                .cloned()
                .expect("the run's tool"),
            toolpath: run.toolpath.clone(),
            holes: None,
            label: run.label.clone(),
        })
        .collect();
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, "2026-10-31")
        .unwrap_or_else(|e| panic!("{e}\n{e:?}"));
    assert_eq!(programs.len(), 1, "the V-bit only");

    let mut fitted = 0;
    for run in &runs {
        for item in arcfit::fit(&run.toolpath.moves, ARC_FIT_MM) {
            let Fitted::Arc(arc_run) = item else {
                continue;
            };
            fitted += 1;
            let off_mm = arc_off_moves_mm(&arc_run);
            assert!(
                off_mm <= ARC_FIT_MM + 1e-9,
                "an arc strays {off_mm} mm from its moves: {arc_run:?}"
            );
            let off_mm = vertices_off_circle_mm(&arc_run);
            assert!(
                off_mm <= ARC_FIT_MM + 1e-9,
                "a vertex is {off_mm} mm off its arc's circle: {arc_run:?}"
            );
        }
    }
    assert!(fitted > 0, "no run was fitted");
}
