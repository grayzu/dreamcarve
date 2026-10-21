// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Profile start-point control (#285; `docs/specs/phase-2/functional.md`,
//! "Phase 2 addendum", capability 36 and acceptance 47; `design.md`, "A9"):
//! `start: "first_vertex"` and `{ "near": … }` start each closed outline's
//! cut at the point of its bit path nearest the drawing's first vertex or
//! the given point; tabs a request lays count from there, a lead arrives
//! there or is dropped, and an entry with no room there falls back instead
//! of moving. Open paths ignore it.

use super::*;
use crate::test_support::assert_safe;
use crate::toolpath::{Move, MoveKind};
use crate::warning::WarningCode;
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Transform, Vertex, assert_approx_eq};
use dc_project::test_support::sample_project;
use dc_project::{
    CornerRelief, DefinitionId, Entry, FeatureDefinition, FeatureInstance, InstanceId, LastPass,
    Lead, MachiningOrder, Operation, OperationId, Side, StartAt, Stock, TabPlacement, TabShape,
    TabSpec, ToolId,
};
use std::collections::BTreeMap;
use std::f64::consts::FRAC_1_SQRT_2;

const TOL: f64 = 1e-6;
const THICKNESS_MM: f64 = 6.0;
const RADIUS_MM: f64 = 6.35 / 2.0;
/// Depth 6 in steps of 2 on 6 mm stock.
const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];
/// The tab top of [`tab_at`]: the passes at 2 and 0 rise over it.
const TAB_TOP_MM: f64 = 3.0;
const TAB_WIDTH_MM: f64 = 8.0;

const ORIGIN: StartAt = StartAt::Near {
    x_mm: 0.0,
    y_mm: 0.0,
};

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

/// A 100 × 50 rectangle drawn from its corner at `(x0, y0)` local.
fn rect_from(x0: f64, y0: f64) -> Polyline {
    let corners = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];
    let k = corners
        .iter()
        .position(|&c| c == (x0, y0))
        .expect("a corner of the rectangle");
    let vertices = (0..4)
        .map(|i| corners[(k + i) % 4])
        .map(|(x, y)| Vertex::line(x, y))
        .collect();
    Polyline::new(vertices, true)
}

/// The 100 × 50 rectangle at (20, 20).
fn rect_project() -> Project {
    project(
        vec![rect_from(0.0, 0.0)],
        &[("i1", Transform::translate(20.0, 20.0))],
    )
}

/// Profile of every instance in `project`, through the stock in 2 mm
/// steps, plunging, no tabs, starting at `start`.
fn profile_op(project: &Project, side: Side, start: StartAt) -> ProfileOp {
    ProfileOp {
        id: OperationId::new("o1"),
        name: "Profile".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(THICKNESS_MM),
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        tabs: None,
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start,
    }
}

/// Generates, and checks the result with [`assert_safe`].
fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
    let (toolpath, warnings) = generate_profile(project, op).unwrap();
    assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
    (toolpath, warnings)
}

fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
    warnings.iter().map(Warning::code).collect()
}

/// One `shape` tab 8 mm wide and 3 mm tall centred at each of
/// `distances_mm`, as a request that keeps its tabs where they are asked.
fn tab_at(distances_mm: Vec<f64>, shape: TabShape) -> TabSpec {
    TabSpec {
        placement: TabPlacement::AtDistances { distances_mm },
        width_mm: TAB_WIDTH_MM,
        height_mm: TAB_TOP_MM,
        shape,
        on_holes: false,
        avoid_corners: false,
    }
}

/// Every straight-down feed move that ends below the stock top, as the
/// XY it is at.
fn plunge_points(tp: &Toolpath) -> Vec<Point> {
    tp.moves
        .windows(2)
        .filter(|w| {
            let (from, mv) = (w[0].to, w[1]);
            !mv.is_rapid()
                && mv.to.z < from.z - EPS_MM
                && from.xy().distance_to(mv.to.xy()) <= EPS_MM
                && mv.to.z < THICKNESS_MM - EPS_MM
        })
        .map(|w| w[1].to.xy())
        .collect()
}

/// `points` with each point within [`TOL`] of an earlier one dropped.
fn distinct(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in points {
        if !out.iter().any(|q| q.distance_to(p) <= TOL) {
            out.push(p);
        }
    }
    out
}

/// Asserts that `points` and `expected` hold the same points, in any
/// order, within [`TOL`].
fn assert_same_points(points: &[Point], expected: &[Point], case: &str) {
    assert_eq!(points.len(), expected.len(), "{case}: {points:?}");
    for e in expected {
        assert!(
            points.iter().any(|p| p.distance_to(*e) <= TOL),
            "{case}: no start at {e:?} in {points:?}"
        );
    }
}

/// The point of an outside path round a convex corner at `corner` nearest
/// `target`, a radius off the corner towards it.
fn round_corner(corner: Point, target: Point, radius_mm: f64) -> Point {
    let v = (target - corner).normalized().unwrap();
    corner + v * radius_mm
}

/// One move that changes XY, with where it starts and its length (arcs
/// as arcs).
#[derive(Debug, Clone, Copy)]
struct XyMove {
    from: Point3,
    mv: Move,
    length_mm: f64,
}

/// The XY moves of the pass at `z_mm`, from the first move that arrives
/// at that Z: every non-rapid move that changes XY and stays below the
/// pass above, until the next rapid.
fn pass_moves(tp: &Toolpath, z_mm: f64, above_mm: f64) -> Vec<XyMove> {
    let start = tp
        .moves
        .iter()
        .position(|m| !m.is_rapid() && (m.to.z - z_mm).abs() <= EPS_MM)
        .expect("the pass is cut");
    let mut out = Vec::new();
    let mut pos = tp.moves[start].to;
    for mv in &tp.moves[start + 1..] {
        if mv.is_rapid() {
            break;
        }
        let (a, b) = (pos.xy(), mv.to.xy());
        if a.distance_to(b) > EPS_MM && pos.z.max(mv.to.z) < above_mm - EPS_MM {
            let length_mm = match mv.kind {
                MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                    let (u, v) = (a - centre, b - centre);
                    let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
                    let mut sweep = u.cross(v).atan2(u.dot(v));
                    if ccw && sweep < 0.0 {
                        sweep += std::f64::consts::TAU;
                    } else if !ccw && sweep > 0.0 {
                        sweep -= std::f64::consts::TAU;
                    }
                    a.distance_to(centre) * sweep.abs()
                }
                _ => a.distance_to(b),
            };
            out.push(XyMove {
                from: pos,
                mv: *mv,
                length_mm,
            });
        }
        pos = mv.to;
    }
    out
}

/// The centre of each stretch of `lap` held at the tab top, mm along the
/// lap from its first move.
fn held_centres(lap: &[XyMove]) -> Vec<f64> {
    let mut centres = Vec::new();
    let mut walked = 0.0;
    let mut run: Option<(f64, f64)> = None;
    let held =
        |m: &XyMove| (m.from.z - TAB_TOP_MM).abs() <= TOL && (m.mv.to.z - TAB_TOP_MM).abs() <= TOL;
    for m in lap {
        if held(m) {
            let (start, length) = run.get_or_insert((walked, 0.0));
            let _ = start;
            *length += m.length_mm;
        } else if let Some((start, length)) = run.take() {
            centres.push(start + length / 2.0);
        }
        walked += m.length_mm;
    }
    if let Some((start, length)) = run {
        centres.push(start + length / 2.0);
    }
    centres
}

/// #285: `near (0, 0)` starts each closed outline's cut at the point of
/// its bit path nearest the origin, on every pass: the outside path's
/// arc round a corner of each of two rectangles, and the inside path of
/// a round hole. Without it the cuts start where they did.
#[test]
fn start_near_point_seams_nearest() {
    let p = project(
        vec![
            rect_from(0.0, 0.0),
            Polyline::circle(Point::new(50.0, 25.0), 10.0),
        ],
        &[
            ("i1", Transform::translate(20.0, 20.0)),
            ("i2", Transform::translate(150.0, 120.0)),
        ],
    );
    let origin = Point::new(0.0, 0.0);
    let (auto, _) = generate(&p, &profile_op(&p, Side::Outside, StartAt::Auto));
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, ORIGIN));
    assert!(warnings.is_empty(), "{warnings:?}");
    let hole_start = |centre: Point| {
        let path_radius_mm = 10.0 - RADIUS_MM;
        centre + (origin - centre).normalized().unwrap() * path_radius_mm
    };
    let expected = [
        round_corner(Point::new(20.0, 20.0), origin, RADIUS_MM),
        round_corner(Point::new(150.0, 120.0), origin, RADIUS_MM),
        hole_start(Point::new(70.0, 45.0)),
        hole_start(Point::new(200.0, 145.0)),
    ];
    let starts = distinct(plunge_points(&tp));
    assert_same_points(&starts, &expected, "near the origin");
    assert_eq!(plunge_points(&tp).len(), 4 * PASS_Z_MM.len());
    assert!(
        distinct(plunge_points(&auto))
            .iter()
            .any(|a| expected.iter().all(|e| e.distance_to(*a) > TOL)),
        "auto starts elsewhere"
    );
}

/// #285: `first_vertex` starts each closed outline's cut at the point of
/// its bit path nearest the drawn first vertex, placed by the instance's
/// transform: a rectangle drawn from its top-right corner starts there,
/// on the middle of the arc round it outside, at the path's corner inside
/// and at the corner itself on the line.
#[test]
fn start_first_vertex_seams_at_vertex() {
    let p = project(
        vec![rect_from(100.0, 50.0)],
        &[("i1", Transform::translate(20.0, 20.0))],
    );
    let corner = Point::new(120.0, 70.0);
    let off = RADIUS_MM * FRAC_1_SQRT_2;
    for (side, expected) in [
        (Side::Outside, Point::new(120.0 + off, 70.0 + off)),
        (
            Side::Inside,
            Point::new(120.0 - RADIUS_MM, 70.0 - RADIUS_MM),
        ),
        (Side::OnLine, corner),
    ] {
        let (tp, warnings) = generate(&p, &profile_op(&p, side, StartAt::FirstVertex));
        assert!(warnings.is_empty(), "{side:?}: {warnings:?}");
        assert_same_points(
            &distinct(plunge_points(&tp)),
            &[expected],
            &format!("{side:?}"),
        );
    }
    // A rotated instance: the vertex goes with it.
    let turned =
        Transform::rotate(std::f64::consts::FRAC_PI_2).then(Transform::translate(200.0, 20.0));
    let p = project(vec![rect_from(100.0, 50.0)], &[("i1", turned)]);
    let vertex = turned.apply(Point::new(100.0, 50.0));
    let (tp, _) = generate(&p, &profile_op(&p, Side::OnLine, StartAt::FirstVertex));
    assert_same_points(&distinct(plunge_points(&tp)), &[vertex], "turned");
}

/// #285, acceptance 47: tabs a request lays count from the explicit
/// start. `at_distances: [10]` with `near (0, 0)` centres the tab 10 mm
/// along the bit's path from where the cut starts, on every pass below
/// the tab top, and a `count` of 4 centres them an eighth of the path
/// apart from it.
#[test]
fn at_distances_count_from_explicit_start() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, ORIGIN);
    op.tabs = Some(tab_at(vec![10.0], TabShape::Rect));
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let start = round_corner(Point::new(20.0, 20.0), Point::new(0.0, 0.0), RADIUS_MM);
    assert_same_points(
        &distinct(plunge_points(&tp))
            .into_iter()
            .take(1)
            .collect::<Vec<_>>(),
        &[start],
        "start",
    );
    for (k, &z) in PASS_Z_MM.iter().enumerate().skip(1) {
        let lap = pass_moves(&tp, z, PASS_Z_MM[k - 1]);
        assert!(
            lap[0].from.xy().distance_to(start) <= TOL,
            "{z}: {:?}",
            lap[0]
        );
        let centres = held_centres(&lap);
        assert_eq!(centres.len(), 1, "{z}: {centres:?}");
        assert_approx_eq!(centres[0], 10.0, 1e-6);
    }

    op.tabs = Some(TabSpec {
        placement: TabPlacement::Count { count: 4 },
        ..tab_at(Vec::new(), TabShape::Rect)
    });
    let (tp, _) = generate(&p, &op);
    let lap = pass_moves(&tp, 0.0, PASS_Z_MM[1]);
    let l: f64 = lap.iter().map(|m| m.length_mm).sum();
    let centres = held_centres(&lap);
    assert_eq!(centres.len(), 4, "{centres:?}");
    for (k, c) in centres.iter().enumerate() {
        assert_approx_eq!(*c, l * (2.0 * k as f64 + 1.0) / 8.0, 1e-6);
    }
}

/// #285: an explicit start wins over the entry. A smooth tab centred on
/// it puts the start under the tab, so a ramp below the tab top has no
/// room there; the start stays (it moves with `auto`, #176), those passes
/// come straight down onto the tab top there, with `entry_fallback`, and
/// nothing plunges anywhere else.
#[test]
fn explicit_start_entry_falls_back_not_moves() {
    let p = rect_project();
    let start = round_corner(Point::new(20.0, 20.0), Point::new(0.0, 0.0), RADIUS_MM);
    let mut op = profile_op(&p, Side::Outside, ORIGIN);
    op.entry = Entry::Ramp { angle_deg: 15.0 };
    op.tabs = Some(tab_at(vec![0.0], TabShape::Smooth));
    let (tp, warnings) = generate(&p, &op);
    assert!(
        codes(&warnings).contains(&WarningCode::EntryFallback),
        "{warnings:?}"
    );
    let plunges = distinct(plunge_points(&tp));
    assert_same_points(&plunges, &[start], "plunges only at the start");
    // The first pass, above the tab, ramps down to the start.
    let lap = pass_moves(&tp, PASS_Z_MM[0], THICKNESS_MM);
    assert!(lap[0].from.xy().distance_to(start) <= TOL, "{:?}", lap[0]);
    // With `auto` the start moves off the tab, so the ramp fits.
    op.start = StartAt::Auto;
    let (moved, warnings) = generate(&p, &op);
    assert!(
        !codes(&warnings).contains(&WarningCode::EntryFallback),
        "{warnings:?}"
    );
    assert!(
        plunge_points(&moved).is_empty(),
        "{:?}",
        plunge_points(&moved)
    );
}

/// #285: an explicit start wins over the lead. A rectangle 6.5 mm from
/// the stock's left edge, cut outside, has a path 3.33 mm from it, where
/// the cutter must keep 3.175 mm: no lead arc fits there. With its cut
/// starting nearest a point to its left, the lead is dropped with
/// `lead_dropped` (`no_room`) and each pass plunges at the start; with
/// `auto` the lead arrives elsewhere, where it fits. A start with room
/// keeps its lead, which arrives there.
#[test]
fn explicit_start_lead_dropped_when_no_room() {
    let p = project(
        vec![rect_from(0.0, 0.0)],
        &[("i1", Transform::translate(6.5, 20.0))],
    );
    let lead = Lead {
        radius_mm: 5.0,
        sweep_deg: 90.0,
        lead_out: false,
        overcut_mm: 0.0,
    };
    let left = StartAt::Near {
        x_mm: 0.0,
        y_mm: 45.0,
    };
    let mut op = profile_op(&p, Side::Outside, StartAt::Auto);
    op.lead = Some(lead.clone());
    let (_, warnings) = generate(&p, &op);
    assert!(
        !codes(&warnings).contains(&WarningCode::LeadDropped),
        "{warnings:?}"
    );

    op.start = left;
    let (tp, warnings) = generate(&p, &op);
    let dropped: Vec<&Warning> = warnings
        .iter()
        .filter(|w| w.code() == WarningCode::LeadDropped)
        .collect();
    assert_eq!(dropped.len(), 1, "{warnings:?}");
    assert!(
        matches!(
            dropped[0],
            Warning::LeadDropped {
                reason: crate::warning::LeadDroppedReason::NoRoom,
                ..
            }
        ),
        "{dropped:?}"
    );
    let start = Point::new(6.5 - RADIUS_MM, 45.0);
    assert_same_points(&distinct(plunge_points(&tp)), &[start], "at the start");

    // Nearest (50, 0), on the bottom side, the lead fits: every pass
    // comes in on its arc and joins the path there.
    op.start = StartAt::Near {
        x_mm: 50.0,
        y_mm: 0.0,
    };
    let (tp, warnings) = generate(&p, &op);
    assert!(
        !codes(&warnings).contains(&WarningCode::LeadDropped),
        "{warnings:?}"
    );
    let arrival = Point::new(50.0, 20.0 - RADIUS_MM);
    for (k, &z) in PASS_Z_MM.iter().enumerate() {
        let above = if k == 0 {
            THICKNESS_MM + 5.0
        } else {
            PASS_Z_MM[k - 1]
        };
        let lap = pass_moves(&tp, z, above);
        assert!(
            !matches!(lap[0].mv.kind, MoveKind::Linear),
            "{z}: a lead arc first: {:?}",
            lap[0]
        );
        assert!(
            lap[0].mv.to.xy().distance_to(arrival) <= TOL,
            "{z}: arrives at the start: {:?}",
            lap[0]
        );
    }
}

/// #285: an open path ignores `start`: its cut starts at the end the cut
/// direction sets (#242), whatever `start` says.
#[test]
fn open_path_ignores_start() {
    let p = project(
        vec![Polyline::new(
            vec![Vertex::line(0.0, 0.0), Vertex::line(100.0, 0.0)],
            false,
        )],
        &[("i1", Transform::translate(20.0, 20.0))],
    );
    for direction in [CutDirection::Climb, CutDirection::Conventional] {
        let mut op = profile_op(&p, Side::OnLine, StartAt::Auto);
        op.direction = direction;
        op.tabs = Some(tab_at(vec![30.0], TabShape::Rect));
        let (auto, _) = generate(&p, &op);
        for start in [
            StartAt::FirstVertex,
            StartAt::Near {
                x_mm: 70.0,
                y_mm: 20.0,
            },
        ] {
            op.start = start;
            let (tp, _) = generate(&p, &op);
            assert_eq!(tp, auto, "{direction:?} {start:?}");
        }
    }
}

/// #285 with #284: with a last pass every pass starts nearest the point,
/// the allowance passes on their own path a little farther out, and tabs
/// laid off corners keep their material on every pass (the safety pass's
/// S9, which `dreamcarve`'s export runs, is checked at the CLI). A
/// reversed last pass starts at the same point.
#[test]
fn explicit_start_with_last_pass_starts_every_pass_there() {
    let p = rect_project();
    let corner = Point::new(20.0, 20.0);
    let origin = Point::new(0.0, 0.0);
    for reverse in [false, true] {
        let mut op = profile_op(&p, Side::Outside, ORIGIN);
        op.last_pass = Some(LastPass {
            allowance_mm: 0.5,
            thickness_mm: None,
            reverse,
        });
        op.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 4 },
            avoid_corners: true,
            ..tab_at(Vec::new(), TabShape::Smooth)
        });
        let (tp, warnings) = generate(&p, &op);
        assert!(
            !codes(&warnings).contains(&WarningCode::TabOnCorner),
            "{warnings:?}"
        );
        let starts = distinct(plunge_points(&tp));
        assert_same_points(
            &starts,
            &[
                round_corner(corner, origin, RADIUS_MM + 0.5),
                round_corner(corner, origin, RADIUS_MM),
            ],
            &format!("reverse {reverse}"),
        );
    }
}
