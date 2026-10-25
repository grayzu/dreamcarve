// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of [`super::generate_bore`] (`docs/specs/phase-2/design.md`,
//! "Bore operation (format version 18)", the dc-cam test list; functional
//! spec acceptance 23a).

use super::*;
use crate::test_support::assert_safe;
use crate::warning::WarningCode;
use dc_geometry::{Polyline, Transform, assert_approx_eq};
use dc_project::test_support::sample_project;
use dc_project::{
    CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
    OperationId, Stock, Tool, ToolId, ToolKind,
};
use std::collections::BTreeMap;

const TOL: f64 = 1e-9;
/// The plaque's measured plywood.
const THICKNESS_MM: f64 = 11.8;
const SAFE_Z_MM: f64 = THICKNESS_MM + 15.0;
/// The helix starts 1 mm above the stock top (#214).
const START_Z_MM: f64 = THICKNESS_MM + 1.0;
const BOTTOM_Z_MM: f64 = -0.2;
const FEED_MM_MIN: f64 = 1000.0;
const PLUNGE_MM_MIN: f64 = 300.0;
const STEP_DOWN_MM: f64 = 2.0;
/// 2.5 − 1.5875: a 5 mm hole less the 3.175 mm bit's radius.
const FIVE_MM_HELIX_RADIUS_MM: f64 = 0.9125;

/// The plaque's 3.175 mm two-flute end mill.
fn end_mill() -> Tool {
    Tool {
        id: ToolId::new("t1"),
        name: "1/8 end mill".into(),
        vendor: None,
        product_id: None,
        number: None,
        cutter: ToolKind::EndMill,
        diameter_mm: 3.175,
        flutes: 2,
        feed_mm_min: Some(FEED_MM_MIN),
        plunge_mm_min: Some(PLUNGE_MM_MIN),
        rpm: Some(18000),
        step_down_mm: Some(STEP_DOWN_MM),
        step_over_fraction: Some(0.4),
    }
}

/// A 200 × 100 × 11.8 mm project with one instance per `(id, outline)`,
/// each with its own definition, placed untransformed, in that drawing
/// order.
fn project(shapes: &[(&str, Polyline)]) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 200.0,
        height_mm: 100.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    p.tools = vec![end_mill()];
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

/// Circles of `diameter_mm` at each `(x, y)`, instance ids `c1`, `c2`, ….
fn circles(diameter_mm: f64, centres: &[(f64, f64)]) -> Project {
    let named: Vec<(String, Polyline)> = centres
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            (
                format!("c{}", i + 1),
                Polyline::circle(Point::new(x, y), diameter_mm / 2.0),
            )
        })
        .collect();
    let shapes: Vec<(&str, Polyline)> =
        named.iter().map(|(n, p)| (n.as_str(), p.clone())).collect();
    project(&shapes)
}

/// The plaque's Bore of every instance: through 11.8 mm with 0.2 mm
/// through-cut allowance, climb, pitch from the tool.
fn bore_op(project: &Project) -> BoreOp {
    BoreOp {
        id: OperationId::new("o1"),
        name: "Screw holes".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        depth_mm: Some(12.0),
        pitch_mm: None,
        direction: CutDirection::Climb,
        stock_to_leave_mm: 0.0,
        through_cut_allowance_mm: 0.2,
        safe_z_mm: 15.0,
    }
}

/// Generates, and checks the result with [`assert_safe`].
fn generate(project: &Project, op: &BoreOp) -> (Toolpath, Vec<Warning>) {
    let (toolpath, warnings) = generate_bore(project, op).unwrap();
    assert_safe(&toolpath, project, &Operation::Bore(op.clone()));
    (toolpath, warnings)
}

fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
    warnings.iter().map(Warning::code).collect()
}

/// The descending chords: the straight cuts that fall.
fn helix_moves(toolpath: &Toolpath) -> Vec<(Point3, Move)> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for m in &toolpath.moves {
        if let Some(f) = from
            && m.kind == MoveKind::Linear
            && m.to.z < f.z - TOL
        {
            out.push((f, *m));
        }
        from = Some(m.to);
    }
    out
}

/// The signed angle swept from `a` to `b` about `c`, in (−π, π].
fn turn(c: Point, a: Point, b: Point) -> f64 {
    let (u, v) = (a - c, b - c);
    (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y)
}

/// Acceptance 23a in dc-cam: every move of a 5 mm hole's Bore with the
/// 3.175 mm bit, below safe Z and away from the rapids, stays within
/// 0.9125 mm of the centre, and every helix vertex is on that circle.
#[test]
fn five_mm_hole_helix_radius_is_0_9125() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert!(warnings.is_empty(), "{warnings:?}");
    let centre = Point::new(50.0, 50.0);
    let cuts: Vec<&Move> = toolpath.moves.iter().filter(|m| m.is_cut()).collect();
    assert!(cuts.len() > 100, "{} cuts", cuts.len());
    for m in &cuts {
        let r = m.to.xy().distance_to(centre);
        assert!(r <= FIVE_MM_HELIX_RADIUS_MM + TOL, "{m:?} is {r} mm out");
    }
    let helix = helix_moves(&toolpath);
    assert!(!helix.is_empty());
    for (_, m) in &helix {
        assert_approx_eq!(m.to.xy().distance_to(centre), FIVE_MM_HELIX_RADIUS_MM, TOL);
    }
    // Every move stays above the allowed bottom (S1).
    for m in &toolpath.moves {
        assert!(m.to.z >= BOTTOM_Z_MM - TOL, "{m:?}");
    }
}

/// The helix falls at most the pitch, the tool's 2 mm step-down here, in
/// each turn, and exactly that in every whole one.
#[test]
fn each_turn_falls_at_most_the_pitch() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let (toolpath, _) = generate(&p, &bore_op(&p));
    let centre = Point::new(50.0, 50.0);
    let helix = helix_moves(&toolpath);
    let (first, _) = helix[0];
    assert_approx_eq!(first.z, START_Z_MM, TOL);
    // (cumulative angle, Z) at every vertex.
    let mut swept = 0.0;
    let mut points = vec![(0.0, first.z)];
    for (from, m) in &helix {
        swept += turn(centre, from.xy(), m.to.xy()).abs();
        points.push((swept, m.to.z));
    }
    for (i, &(a, z)) in points.iter().enumerate() {
        for &(b, w) in &points[i + 1..] {
            if b - a <= TAU + 1e-9 {
                assert!(z - w <= STEP_DOWN_MM + 1e-9, "falls {} in one turn", z - w);
            }
        }
    }
    let total_turns = (START_Z_MM - BOTTOM_Z_MM) / STEP_DOWN_MM;
    assert_approx_eq!(swept / TAU, total_turns, 1e-9);
    assert_approx_eq!(points.last().unwrap().1, BOTTOM_Z_MM, TOL);
}

/// After the helix the bit cuts one full circle at the depth, as four
/// quarter arcs about the hole's centre, then moves to the centre and
/// lifts to safe Z.
#[test]
fn ends_with_full_circle_at_depth_then_centre() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let (toolpath, _) = generate(&p, &bore_op(&p));
    let centre = Point::new(50.0, 50.0);
    let moves = &toolpath.moves;
    let n = moves.len();
    let last = moves[n - 1];
    assert!(last.is_rapid());
    assert_approx_eq!(last.to.z, SAFE_Z_MM, TOL);
    assert!(last.to.xy().distance_to(centre) <= TOL);
    let to_centre = moves[n - 2];
    assert_eq!(to_centre.kind, MoveKind::Linear);
    assert!(to_centre.to.xy().distance_to(centre) <= TOL);
    assert_approx_eq!(to_centre.to.z, BOTTOM_Z_MM, TOL);
    assert_eq!(to_centre.feed_mm_min, Some(FEED_MM_MIN));
    let helix_end = moves[n - 7].to;
    assert_approx_eq!(helix_end.z, BOTTOM_Z_MM, TOL);
    let mut from = helix_end.xy();
    let mut swept = 0.0;
    for m in &moves[n - 6..n - 2] {
        let MoveKind::ArcCcw { centre: c } = m.kind else {
            panic!("a climb circle is counter-clockwise arcs: {m:?}");
        };
        assert!(c.distance_to(centre) <= TOL);
        assert_approx_eq!(m.to.z, BOTTOM_Z_MM, TOL);
        assert_approx_eq!(m.to.xy().distance_to(centre), FIVE_MM_HELIX_RADIUS_MM, TOL);
        assert_eq!(m.feed_mm_min, Some(FEED_MM_MIN));
        let quarter = turn(centre, from, m.to.xy());
        assert_approx_eq!(quarter, FRAC_PI_2, 1e-9);
        swept += quarter;
        from = m.to.xy();
    }
    assert_approx_eq!(swept, TAU, 1e-9);
    assert!(from.distance_to(helix_end.xy()) <= TOL, "the circle closes");
}

/// The bit reaches the helix start at safe Z, rapids straight down to
/// 1 mm above the stock top, and starts the helix there: it never
/// plunges into material.
#[test]
fn starts_one_mm_above_stock_top() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let (toolpath, _) = generate(&p, &bore_op(&p));
    let start = Point::new(50.0 + FIVE_MM_HELIX_RADIUS_MM, 50.0);
    let moves = &toolpath.moves;
    let first_cut = moves.iter().position(Move::is_cut).unwrap();
    let before = &moves[..first_cut];
    assert!(before.iter().all(Move::is_rapid), "{before:?}");
    let down = before.last().unwrap();
    assert!(down.to.xy().distance_to(start) <= TOL, "{down:?}");
    assert_approx_eq!(down.to.z, START_Z_MM, TOL);
    for m in before {
        assert!(m.to.z >= START_Z_MM - TOL, "{m:?}");
        if m.to.z < SAFE_Z_MM - TOL {
            assert!(m.to.xy().distance_to(start) <= TOL, "{m:?}");
        }
    }
}

/// Climb turns the helix and the circle counter-clockwise (Y-up, seen
/// from +Z), conventional clockwise.
#[test]
fn climb_is_ccw_conventional_is_cw() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let centre = Point::new(50.0, 50.0);
    for (direction, ccw) in [
        (CutDirection::Climb, true),
        (CutDirection::Conventional, false),
    ] {
        let mut op = bore_op(&p);
        op.direction = direction;
        let (toolpath, _) = generate(&p, &op);
        for (from, m) in helix_moves(&toolpath) {
            let t = turn(centre, from.xy(), m.to.xy());
            assert!(if ccw { t > 0.0 } else { t < 0.0 }, "{direction:?}: {t}");
        }
        let arcs: Vec<&Move> = toolpath
            .moves
            .iter()
            .filter(|m| matches!(m.kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }))
            .collect();
        assert_eq!(arcs.len(), 4);
        for m in arcs {
            assert_eq!(
                matches!(m.kind, MoveKind::ArcCcw { .. }),
                ccw,
                "{direction:?}: {m:?}"
            );
        }
    }
}

/// With `pitch_mm` unset the helix falls the tool's step-down per turn;
/// a set pitch is used as it is.
#[test]
fn pitch_defaults_to_tool_step_down() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let turns = |op: &BoreOp| {
        let (toolpath, _) = generate(&p, op);
        let centre = Point::new(50.0, 50.0);
        helix_moves(&toolpath)
            .iter()
            .map(|(f, m)| turn(centre, f.xy(), m.to.xy()).abs())
            .sum::<f64>()
            / TAU
    };
    let mut op = bore_op(&p);
    assert_approx_eq!(turns(&op), (START_Z_MM - BOTTOM_Z_MM) / STEP_DOWN_MM, 1e-9);
    op.pitch_mm = Some(0.5);
    assert_approx_eq!(turns(&op), (START_Z_MM - BOTTOM_Z_MM) / 0.5, 1e-9);
}

/// A pitch above the tool's step-down, or not positive, is refused
/// (validation reports it first).
#[test]
fn pitch_over_step_down_is_error() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    for pitch_mm in [2.5, 0.0, -1.0, f64::NAN] {
        let mut op = bore_op(&p);
        op.pitch_mm = Some(pitch_mm);
        let err = generate_bore(&p, &op).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::InvalidBoreSetting {
                    field: "pitch_mm",
                    ..
                }
            ),
            "{pitch_mm}: {err:?}"
        );
    }
    // With no pitch and no step-down there is no pitch at all.
    let mut p = p;
    p.tools[0].step_down_mm = None;
    let err = generate_bore(&p, &bore_op(&p)).unwrap_err();
    assert!(matches!(err, CamError::MissingStepDown { .. }), "{err:?}");
    // A pitch so fine a chord falls under the last place is refused too.
    let mut op = bore_op(&p);
    op.pitch_mm = Some(0.01);
    let err = generate_bore(&p, &op).unwrap_err();
    assert!(
        matches!(
            err,
            CamError::InvalidBoreSetting {
                field: "pitch_mm",
                ..
            }
        ),
        "{err:?}"
    );
}

/// The stock to leave stays on the wall: the helix radius is the
/// hole's less the tool's and the stock to leave. A negative one is refused.
#[test]
fn stock_to_leave_shrinks_helix() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let mut op = bore_op(&p);
    op.stock_to_leave_mm = 0.1;
    let (toolpath, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let centre = Point::new(50.0, 50.0);
    for m in toolpath.moves.iter().filter(|m| m.is_cut()) {
        assert!(m.to.xy().distance_to(centre) <= 0.8125 + TOL, "{m:?}");
    }
    for (_, m) in helix_moves(&toolpath) {
        assert_approx_eq!(m.to.xy().distance_to(centre), 0.8125, TOL);
    }
    op.stock_to_leave_mm = -0.1;
    let err = generate_bore(&p, &op).unwrap_err();
    assert!(
        matches!(
            err,
            CamError::InvalidBoreSetting {
                field: "stock_to_leave_mm",
                ..
            }
        ),
        "{err:?}"
    );
}

/// S10: every chord falls no faster than the plunge rate, at the feed
/// scaled down only as far as that needs.
#[test]
fn descent_feed_is_plunge_limited() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let (toolpath, _) = generate(&p, &bore_op(&p));
    for (from, m) in helix_moves(&toolpath) {
        let feed = m.feed_mm_min.unwrap();
        let (dx, dy, dz) = (m.to.x - from.x, m.to.y - from.y, from.z - m.to.z);
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        assert!(feed <= FEED_MM_MIN + TOL, "{m:?}");
        assert!(feed * dz / length <= PLUNGE_MM_MIN + 1e-6, "{m:?}");
        assert_approx_eq!(feed, FEED_MM_MIN.min(PLUNGE_MM_MIN * length / dz), 1e-9);
    }
}

/// A hole the bit does not fit is skipped with `bore_no_room`, saying a
/// smaller end mill is needed, and nothing is cut.
#[test]
fn hole_too_small_is_skipped_no_room() {
    let p = circles(3.0, &[(50.0, 50.0)]);
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert!(toolpath.moves.iter().all(|m| !m.is_cut()), "{toolpath:?}");
    assert_eq!(
        codes(&warnings),
        [WarningCode::BoreNoRoom, WarningCode::BoreNoHoles]
    );
    let Warning::BoreNoRoom {
        hole_mm,
        tool_mm,
        drill_fits,
        shape,
        ..
    } = &warnings[0]
    else {
        panic!("{warnings:?}");
    };
    assert_approx_eq!(*hole_mm, 3.0, 1e-6);
    assert_approx_eq!(*tool_mm, 3.175, TOL);
    assert!(!drill_fits);
    assert_eq!(shape.instance.as_str(), "c1");
}

/// A hole the bit's size, or within 0.1 mm of it, leaves no room to
/// spiral: skipped with `bore_no_room`, pointing to Drill.
#[test]
fn hole_bit_size_is_skipped_pointing_to_drill() {
    for diameter_mm in [3.175, 3.17, 3.27] {
        let p = circles(diameter_mm, &[(50.0, 50.0)]);
        let (toolpath, warnings) = generate(&p, &bore_op(&p));
        assert!(toolpath.moves.iter().all(|m| !m.is_cut()), "{diameter_mm}");
        let Some(Warning::BoreNoRoom { drill_fits, .. }) = warnings.first() else {
            panic!("{diameter_mm}: {warnings:?}");
        };
        assert!(drill_fits, "{diameter_mm}");
    }
    // Just over the limit it is bored.
    let p = circles(
        3.175 + 2.0 * BORE_MIN_HELIX_RADIUS_MM + 0.001,
        &[(50.0, 50.0)],
    );
    let (_, warnings) = generate(&p, &bore_op(&p));
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A hole twice the bit's diameter is bored: the bit's edge passes the
/// centre, so no core is left.
#[test]
fn hole_twice_tool_is_bored() {
    let p = circles(6.35, &[(50.0, 50.0)]);
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert!(warnings.is_empty(), "{warnings:?}");
    let centre = Point::new(50.0, 50.0);
    for (_, m) in helix_moves(&toolpath) {
        assert_approx_eq!(m.to.xy().distance_to(centre), 1.5875, TOL);
    }
}

/// A hole more than twice the bit's diameter would leave a core standing,
/// loose in a through hole: it is skipped with `bore_leaves_core`, naming
/// the core, and the other holes are cut.
#[test]
fn hole_over_twice_tool_is_skipped_leaves_core() {
    let mut p = project(&[
        ("big", Polyline::circle(Point::new(30.0, 50.0), 5.0)),
        ("small", Polyline::circle(Point::new(70.0, 50.0), 2.5)),
    ]);
    p.operations.clear();
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert_eq!(codes(&warnings), [WarningCode::BoreLeavesCore]);
    let Warning::BoreLeavesCore {
        core_mm,
        hole_mm,
        shape,
        ..
    } = &warnings[0]
    else {
        panic!("{warnings:?}");
    };
    assert_approx_eq!(*core_mm, 3.65, 1e-6);
    assert_approx_eq!(*hole_mm, 10.0, 1e-6);
    assert_eq!(shape.instance.as_str(), "big");
    for m in toolpath.moves.iter().filter(|m| m.is_cut()) {
        assert!(
            m.to.xy().distance_to(Point::new(70.0, 50.0)) <= FIVE_MM_HELIX_RADIUS_MM + TOL,
            "only the small hole is cut: {m:?}"
        );
    }
    // Within the flatten tolerance of twice the bit it is bored.
    let p = circles(6.35 + 2.0 * 0.009, &[(50.0, 50.0)]);
    let (_, warnings) = generate(&p, &bore_op(&p));
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A target that is not a circle is skipped with
/// `bore_target_not_circle`; the circles are still cut.
#[test]
fn non_circle_is_skipped() {
    let p = project(&[
        ("sq", Polyline::rect(Point::new(10.0, 10.0), 5.0, 5.0)),
        ("c1", Polyline::circle(Point::new(50.0, 50.0), 2.5)),
    ]);
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert_eq!(codes(&warnings), [WarningCode::BoreTargetNotCircle]);
    assert!(toolpath.moves.iter().any(Move::is_cut));
}

/// With every target skipped the Bore cuts nothing and says so once.
#[test]
fn every_hole_skipped_warns_no_holes() {
    let p = project(&[
        ("sq", Polyline::rect(Point::new(10.0, 10.0), 5.0, 5.0)),
        ("c1", Polyline::circle(Point::new(50.0, 50.0), 1.0)),
        ("c2", Polyline::circle(Point::new(80.0, 50.0), 10.0)),
    ]);
    let (toolpath, warnings) = generate(&p, &bore_op(&p));
    assert_eq!(
        codes(&warnings),
        [
            WarningCode::BoreTargetNotCircle,
            WarningCode::BoreNoRoom,
            WarningCode::BoreLeavesCore,
            WarningCode::BoreNoHoles
        ]
    );
    assert!(toolpath.moves.iter().all(|m| !m.is_cut()));
}

/// Holes are cut nearest first from the first in drawing order, and every
/// move between them is a rapid at safe Z.
#[test]
fn holes_nearest_first_linked_at_safe_z() {
    let p = circles(5.0, &[(10.0, 10.0), (50.0, 10.0), (30.0, 10.0)]);
    let (toolpath, _) = generate(&p, &bore_op(&p));
    let moves = &toolpath.moves;
    // Centres in cut order: where each hole's move to its centre ends.
    let mut order = Vec::new();
    for (i, pair) in moves.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if b.is_rapid() && a.kind == MoveKind::Linear && a.to.z <= BOTTOM_Z_MM + TOL {
            order.push(a.to.x);
        }
        if b.is_rapid() && a.to.xy().distance_to(b.to.xy()) > EPS_MM {
            assert_approx_eq!(a.to.z, SAFE_Z_MM, TOL);
            assert_approx_eq!(b.to.z, SAFE_Z_MM, TOL);
        }
        if b.is_cut() {
            assert!(a.to.z <= START_Z_MM + TOL, "move {} cuts from {a:?}", i + 1);
        }
    }
    assert_eq!(order.len(), 3);
    for (a, e) in order.iter().zip([10.0, 30.0, 50.0]) {
        assert_approx_eq!(*a, e, TOL);
    }
}

/// S1: the depth may pass the stock bottom only by the operation's own
/// through-cut allowance.
#[test]
fn depth_beyond_bottom_needs_allowance() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    let mut op = bore_op(&p);
    op.through_cut_allowance_mm = 0.0;
    let err = generate_bore(&p, &op).unwrap_err();
    assert!(
        matches!(err, CamError::BelowMaterialBottom { .. }),
        "{err:?}"
    );
    op.depth_mm = Some(THICKNESS_MM);
    let (toolpath, _) = generate(&p, &op);
    let lowest = toolpath
        .moves
        .iter()
        .map(|m| m.to.z)
        .fold(f64::INFINITY, f64::min);
    assert_approx_eq!(lowest, 0.0, TOL);
}

/// A Bore needs the tool's feed and plunge rate, never invented ones,
/// and a flat end mill.
#[test]
fn missing_feed_is_error() {
    let p = circles(5.0, &[(50.0, 50.0)]);
    for clear in [
        (|t: &mut Tool| t.feed_mm_min = None) as fn(&mut Tool),
        |t: &mut Tool| t.plunge_mm_min = None,
    ] {
        let mut p = p.clone();
        clear(&mut p.tools[0]);
        let err = generate_bore(&p, &bore_op(&p)).unwrap_err();
        assert!(matches!(err, CamError::MissingFeed { .. }), "{err:?}");
    }
    let mut p = p;
    p.tools[0].cutter = ToolKind::Drill;
    let err = generate_bore(&p, &bore_op(&p)).unwrap_err();
    assert!(matches!(err, CamError::CutterNotFlat { .. }), "{err:?}");
}
