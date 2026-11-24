// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts
//   src/engine/toolpaths/geometry.ts

//! Tests of the Profile generator, [`generate_profile`]: the moves it
//! emits, the warnings beside them and the errors it refuses with.

use super::*;
use crate::error::{FeedField, ShapeRef};
use crate::test_support::assert_safe;
use crate::toolpath::{Move, MoveKind};
use crate::warning::WarningCode;
use dc_geometry::{EPS_MM, Point, Polyline, Transform, Vertex, assert_approx_eq};
use dc_project::test_support::sample_project;
use dc_project::{
    CornerRelief, DefinitionId, Entry, FeatureDefinition, FeatureInstance, InstanceId, LastPass,
    MachiningOrder, Operation, OperationId, Side, Stock, TabPlacement, TabShape, TabSpec, ToolId,
};
use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

const TOL: f64 = 1e-6;
const THICKNESS_MM: f64 = 6.0;
const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
const FEED_MM_MIN: f64 = 1000.0;
const PLUNGE_MM_MIN: f64 = 300.0;
/// Depth 6 in steps of 2 on 6 mm stock.
const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];

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
        shape: None,
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
            construction: false,
            group: None,
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

fn rect_project() -> Project {
    project(vec![rect_100x50()], &[("i1", at_20_20())])
}

/// The 100 × 50 rect with a 20 mm circular hole in its middle.
fn ring_project(hole_radius_mm: f64) -> Project {
    project(
        vec![
            rect_100x50(),
            Polyline::circle(Point::new(50.0, 25.0), hole_radius_mm),
        ],
        &[("i1", at_20_20())],
    )
}

/// Profile of every instance in `project`, through the stock in 2 mm
/// steps (the tool's step-down), no tabs.
fn profile_op(project: &Project, side: Side, direction: CutDirection) -> ProfileOp {
    ProfileOp {
        id: OperationId::new("o1"),
        name: "Profile".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(THICKNESS_MM),
        step_down_mm: None,
        direction,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        tabs: None,
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: dc_project::StartAt::Auto,
    }
}

/// Generates, and checks the result with [`assert_safe`].
fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
    let (toolpath, warnings) = generate_profile(project, op).unwrap();
    assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
    (toolpath, warnings)
}

/// One lap of a contour at one depth: the cutting moves between a
/// descent and the next non-cutting move.
#[derive(Debug)]
struct Lap {
    z_mm: f64,
    /// Start point, then the end of every move.
    points: Vec<Point>,
    /// Signed area enclosed, arcs included; negative is clockwise.
    area_mm2: f64,
    moves: Vec<Move>,
}

impl Lap {
    fn x_extent_mm(&self) -> f64 {
        let xs = self.points.iter().map(|p| p.x);
        xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
    }

    fn y_extent_mm(&self) -> f64 {
        let ys = self.points.iter().map(|p| p.y);
        ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min)
    }
}

/// Signed sweep, radians, of the arc from `a` to `b` about `c`.
fn sweep_rad(a: Point, b: Point, c: Point, ccw: bool) -> f64 {
    let (u, v) = (a - c, b - c);
    let raw = u.cross(v).atan2(u.dot(v));
    if ccw {
        if raw > 0.0 { raw } else { raw + TAU }
    } else if raw < 0.0 {
        raw
    } else {
        raw - TAU
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
    for mv in &toolpath.moves {
        let level = (mv.to.z - pos.z).abs() <= EPS_MM;
        let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
        let cutting = !mv.is_rapid() && level && xy;
        if cutting {
            let (a, b) = (pos.xy(), mv.to.xy());
            let lap = current.get_or_insert_with(|| Lap {
                z_mm: mv.to.z,
                points: vec![a],
                area_mm2: 0.0,
                moves: Vec::new(),
            });
            lap.area_mm2 += a.x * b.y - b.x * a.y;
            let arc = match mv.kind {
                MoveKind::ArcCcw { centre } => Some((centre, true)),
                MoveKind::ArcCw { centre } => Some((centre, false)),
                _ => None,
            };
            if let Some((c, ccw)) = arc {
                let r = a.distance_to(c);
                let theta = sweep_rad(a, b, c, ccw);
                // Twice the circular segment between chord and arc.
                lap.area_mm2 += r * r * (theta - theta.sin());
            }
            lap.points.push(b);
            lap.moves.push(*mv);
        } else if let Some(mut lap) = current.take() {
            lap.area_mm2 /= 2.0;
            laps.push(lap);
        }
        pos = mv.to;
    }
    if let Some(mut lap) = current.take() {
        lap.area_mm2 /= 2.0;
        laps.push(lap);
    }
    laps
}

fn laps_at(toolpath: &Toolpath, z_mm: f64) -> Vec<Lap> {
    laps(toolpath)
        .into_iter()
        .filter(|l| (l.z_mm - z_mm).abs() <= EPS_MM)
        .collect()
}

fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
    warnings.iter().map(|w| w.code()).collect()
}

#[test]
fn direction_rule() {
    use CutDirection::{Climb, Conventional};
    assert!(!traversal_is_ccw(Wall::Outside, Climb));
    assert!(traversal_is_ccw(Wall::Outside, Conventional));
    assert!(traversal_is_ccw(Wall::Inside, Climb));
    assert!(!traversal_is_ccw(Wall::Inside, Conventional));
}

#[test]
fn outside_rect_offsets_outward() {
    let p = rect_project();
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    let laps = laps(&tp);
    assert_eq!(laps.len(), PASS_Z_MM.len());
    for (lap, z) in laps.iter().zip(PASS_Z_MM) {
        assert_approx_eq!(lap.z_mm, z, TOL);
        assert_approx_eq!(lap.x_extent_mm(), 106.35, TOL);
        assert_approx_eq!(lap.y_extent_mm(), 56.35, TOL);
        let min_x = lap.points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        assert_approx_eq!(min_x, 20.0 - 3.175, TOL);
    }
}

/// #227, S2: with `entry: plunge` the tool rapids straight down from
/// safe Z to 1 mm above the stock top and plunges at the plunge rate
/// from there, not from safe Z through the air. A later pass after a
/// lift (the rect's next pass follows at its start, so none here) would
/// stop 1 mm above the floor already cut.
#[test]
fn plunge_entry_rapids_to_just_above_the_stock() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let first = tp.moves[0];
    assert!(first.is_rapid(), "{first:?}");
    assert_approx_eq!(first.to.z, SAFE_Z_MM, TOL);
    let (rapid, plunge) = (tp.moves[1], tp.moves[2]);
    assert_eq!(
        rapid,
        Move::rapid(Point3::new(first.to.x, first.to.y, THICKNESS_MM + 1.0)),
        "{:?}",
        &tp.moves[..4]
    );
    assert_eq!(
        plunge,
        Move::linear(
            Point3::new(first.to.x, first.to.y, PASS_Z_MM[0]),
            PLUNGE_MM_MIN
        )
    );
}

#[test]
fn inside_rect_offsets_inward() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
    let laps = laps(&tp);
    assert_eq!(laps.len(), PASS_Z_MM.len());
    for lap in &laps {
        assert_approx_eq!(lap.x_extent_mm(), 93.65, TOL);
        assert_approx_eq!(lap.y_extent_mm(), 43.65, TOL);
    }
}

#[test]
fn on_line_follows_the_shape() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
    for lap in &laps(&tp) {
        assert_approx_eq!(lap.x_extent_mm(), 100.0, TOL);
        assert_approx_eq!(lap.y_extent_mm(), 50.0, TOL);
        assert!(lap.area_mm2 < 0.0, "outer on the line, climb: clockwise");
    }
}

/// The outside cut of the rect at (20, 20) sweeps X 13.65..126.35
/// with the 6.35 mm tool. On 120 mm wide stock that is 6.35 mm past
/// the right edge; the inside cut, which sweeps to 120 exactly, fits.
#[test]
fn toolpath_off_stock_is_an_error() {
    let mut p = rect_project();
    p.stock.width_mm = 120.0;
    let err =
        generate_profile(&p, &profile_op(&p, Side::Outside, CutDirection::Climb)).unwrap_err();
    let CamError::OffStock { op, overhang } = &err else {
        panic!("expected OffStock, got {err:?}");
    };
    assert_eq!(op.as_str(), "o1");
    assert_approx_eq!(overhang.max_x_mm, 6.35, TOL);
    assert_approx_eq!(overhang.min_x_mm, 0.0, TOL);
    assert_approx_eq!(overhang.max_y_mm, 0.0, TOL);
    assert_eq!(
        err.to_string(),
        "operation `o1` cuts off the stock: 6.350 mm past the +X (right) edge"
    );
    generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
}

#[test]
fn ring_cuts_outer_outside_and_hole_inside() {
    let p = ring_project(10.0);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    for z in PASS_Z_MM {
        let laps = laps_at(&tp, z);
        assert_eq!(laps.len(), 2, "two contours at Z {z}");
        let hole = &laps[0];
        let outer = &laps[1];
        assert_approx_eq!(outer.x_extent_mm(), 106.35, TOL);
        assert_approx_eq!(outer.y_extent_mm(), 56.35, TOL);
        assert_approx_eq!(hole.x_extent_mm(), 20.0 - 6.35, TOL);
        assert_approx_eq!(hole.area_mm2.abs(), PI * 6.825 * 6.825, 1e-6);
    }
}

#[test]
fn holes_are_cut_before_their_outer() {
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let laps = laps(&tp);
    assert_eq!(laps.len(), 6);
    // Every pass of the hole, then every pass of the outer.
    for lap in &laps[..3] {
        assert!(lap.x_extent_mm() < 20.0, "hole first: {lap:?}");
    }
    for lap in &laps[3..] {
        assert!(lap.x_extent_mm() > 100.0, "outer last: {lap:?}");
    }
}

/// Names each lap, in cut order, by the `(name, x extent mm)` its
/// x extent is nearest to. Each contour is cut at every pass before the
/// next contour starts, so the result is the contour cut order with one
/// entry per pass.
fn lap_names(toolpath: &Toolpath, contours: &[(&'static str, f64)]) -> Vec<&'static str> {
    laps(toolpath)
        .iter()
        .map(|lap| {
            let off = |w: f64| (lap.x_extent_mm() - w).abs();
            let (name, width_mm) = contours
                .iter()
                .copied()
                .min_by(|a, b| off(a.1).total_cmp(&off(b.1)))
                .unwrap();
            assert!(off(width_mm) < TOL, "lap {lap:?} matches no contour");
            name
        })
        .collect()
}

/// Each name repeated once per pass, for comparing with [`lap_names`].
fn every_pass(names: &[&'static str]) -> Vec<&'static str> {
    names
        .iter()
        .flat_map(|&n| std::iter::repeat_n(n, PASS_Z_MM.len()))
        .collect()
}

/// O1 (`docs/research/toolpath-ordering.md`): a 200 × 150 frame (outline
/// A, 160 × 110 hole B) with a 60 mm coaster C inside the hole. Whatever
/// the drawing order, the coaster is cut first, then the hole that frees
/// the waste around it, then the frame's outline.
#[test]
fn island_is_cut_before_the_hole_around_it() {
    let a = Polyline::rect(Point::new(0.0, 0.0), 200.0, 150.0);
    let b = Polyline::rect(Point::new(20.0, 20.0), 160.0, 110.0);
    let c = Polyline::circle(Point::new(100.0, 75.0), 30.0);
    // Outside: A and C grow by the radius, the hole B shrinks.
    let widths = [("A", 206.35), ("B", 153.65), ("C", 66.35)];
    let orders = [
        vec![a.clone(), b.clone(), c.clone()],
        vec![c.clone(), b.clone(), a.clone()],
        vec![b, c, a],
    ];
    for profiles in orders {
        let p = project(profiles, &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(lap_names(&tp, &widths), every_pass(&["C", "B", "A"]));
    }
}

/// Four nested contours, drawn outermost first: outer, hole, island,
/// hole in the island. The innermost is cut first, and each hole still
/// comes before its own outline.
#[test]
fn four_levels_cut_innermost_first() {
    let p = project(
        vec![
            Polyline::rect(Point::new(0.0, 0.0), 200.0, 150.0),
            Polyline::rect(Point::new(20.0, 20.0), 160.0, 110.0),
            Polyline::rect(Point::new(40.0, 40.0), 120.0, 70.0),
            Polyline::rect(Point::new(60.0, 60.0), 80.0, 30.0),
        ],
        &[("i1", at_20_20())],
    );
    let widths = [
        ("outer", 206.35),
        ("hole", 153.65),
        ("island", 126.35),
        ("island hole", 73.65),
    ];
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        lap_names(&tp, &widths),
        every_pass(&["island hole", "island", "hole", "outer"])
    );
}

/// Two parts side by side, neither inside the other, are cut in the
/// order they were drawn, either way round.
#[test]
fn apart_regions_keep_drawing_order() {
    let small = Polyline::rect(Point::new(0.0, 0.0), 40.0, 30.0);
    let big = Polyline::rect(Point::new(100.0, 0.0), 60.0, 30.0);
    let widths = [("small", 46.35), ("big", 66.35)];
    for (profiles, expected) in [
        (vec![small.clone(), big.clone()], ["small", "big"]),
        (vec![big, small], ["big", "small"]),
    ] {
        let p = project(profiles, &[("i1", at_20_20())]);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert_eq!(lap_names(&tp, &widths), every_pass(&expected));
    }
}

#[test]
fn climb_outside_is_clockwise() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    for lap in &laps(&tp) {
        assert!(lap.area_mm2 < 0.0, "{lap:?}");
        assert_approx_eq!(
            lap.area_mm2,
            -(106.35 * 56.35 - (4.0 - PI) * 3.175 * 3.175),
            1e-6
        );
    }
}

#[test]
fn conventional_outside_is_counter_clockwise() {
    let p = rect_project();
    let (tp, _) = generate(
        &p,
        &profile_op(&p, Side::Outside, CutDirection::Conventional),
    );
    for lap in &laps(&tp) {
        assert!(lap.area_mm2 > 0.0, "{lap:?}");
    }
}

#[test]
fn climb_inside_is_counter_clockwise() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
    for lap in &laps(&tp) {
        assert!(lap.area_mm2 > 0.0, "{lap:?}");
    }
}

#[test]
fn climb_round_a_hole_is_counter_clockwise() {
    // Outside mode cuts the hole's wall from inside the hole.
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let laps = laps_at(&tp, PASS_Z_MM[0]);
    assert!(laps[0].area_mm2 > 0.0, "hole: {:?}", laps[0]);
    assert!(laps[1].area_mm2 < 0.0, "outer: {:?}", laps[1]);
}

#[test]
fn inside_mode_cuts_around_the_hole_clockwise_when_climbing() {
    // Inside mode cuts the hole's wall from outside the hole: an
    // outside wall, so climb is clockwise, and the hole grows by r.
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
    let laps = laps_at(&tp, PASS_Z_MM[0]);
    assert_eq!(laps.len(), 2);
    assert_approx_eq!(laps[0].x_extent_mm(), 20.0 + 6.35, TOL);
    assert!(laps[0].area_mm2 < 0.0, "hole: {:?}", laps[0]);
    assert!(laps[1].area_mm2 > 0.0, "outer: {:?}", laps[1]);
}

#[test]
fn arcs_turn_with_the_traversal() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    for lap in &laps(&tp) {
        let cw = lap
            .moves
            .iter()
            .filter(|m| matches!(m.kind, MoveKind::ArcCw { .. }))
            .count();
        let ccw = lap
            .moves
            .iter()
            .filter(|m| matches!(m.kind, MoveKind::ArcCcw { .. }))
            .count();
        assert_eq!((cw, ccw), (4, 0), "round corners of a clockwise lap");
    }
    let (tp, _) = generate(
        &p,
        &profile_op(&p, Side::Outside, CutDirection::Conventional),
    );
    let first = &laps(&tp)[0];
    assert!(
        first
            .moves
            .iter()
            .all(|m| !matches!(m.kind, MoveKind::ArcCw { .. }))
    );
}

#[test]
fn open_path_is_skipped_with_warning() {
    let open = Polyline::new(
        vec![Vertex::line(0.0, 60.0), Vertex::line(100.0, 60.0)],
        false,
    );
    let p = project(vec![rect_100x50(), open], &[("i1", at_20_20())]);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
    let Warning::OpenPathSkipped { shape, reason } = &warnings[0] else {
        panic!("{:?}", warnings[0]);
    };
    assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 1));
    assert_eq!(*reason, OpenPathReason::NoSide);
    // The message says what does cut it.
    let message = warnings[0].to_string();
    assert!(
        message.contains("On line") && message.contains("Engrave"),
        "{message}"
    );
    let laps = laps(&tp);
    assert_eq!(laps.len(), PASS_Z_MM.len(), "only the rect is cut");
    for lap in &laps {
        assert!(lap.points.iter().all(|p| p.y < 20.0 + 60.0 - 1.0));
    }
}

#[test]
fn tool_larger_than_hole_warns_and_skips() {
    let p = ring_project(2.0);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::ToolTooLarge]);
    let Warning::ToolTooLarge { shape, tool, .. } = &warnings[0] else {
        panic!("{:?}", warnings[0]);
    };
    assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 1));
    assert_eq!(tool, "6.35 mm upcut");
    for z in PASS_Z_MM {
        let laps = laps_at(&tp, z);
        assert_eq!(laps.len(), 1, "only the outer at Z {z}");
        assert_approx_eq!(laps[0].x_extent_mm(), 106.35, TOL);
    }
}

/// An Inside Profile of the 100 × 50 rect with a radius-22
/// island leaves a 3 mm band above and below the island, narrower than
/// the 6.35 mm bit. The island's lap would run 3.175 mm outside it, to
/// Y 19.825, past the rect's bottom edge at Y 20, and gouge the part
/// around the ring. No toolpath is produced: the error names the
/// operation, both shapes, the band and the tool.
#[test]
fn inside_cut_in_band_narrower_than_tool_is_refused() {
    let p = ring_project(22.0);
    let err = generate_profile(&p, &profile_op(&p, Side::Inside, CutDirection::Climb))
        .expect_err("a band narrower than the bit is refused");
    let CamError::BandNarrowerThanTool {
        op,
        shape,
        other,
        band_mm,
        diameter_mm,
    } = &err
    else {
        panic!("expected BandNarrowerThanTool, got {err:?}");
    };
    assert_eq!(op.as_str(), "o1");
    assert_eq!((shape.profile, other.profile), (0, 1), "outline first");
    assert_approx_eq!(*band_mm, 3.0, TOL);
    assert_approx_eq!(*diameter_mm, 6.35, TOL);
    let text = err.to_string();
    for needle in ["`o1`", "profile 0", "profile 1", "3.000 mm", "6.35 mm tool"] {
        assert!(text.contains(needle), "{needle:?} in {text}");
    }
}

/// A band exactly as wide as the bit is cut: each lap's cutter
/// just touches the other line. Outside, the same narrow ring is a
/// thin wall around the hole, not a gouge, and is cut too.
#[test]
fn inside_band_as_wide_as_the_tool_is_cut() {
    let p = ring_project(25.0 - 6.35);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
    for z in PASS_Z_MM {
        assert_eq!(laps_at(&tp, z).len(), 2, "both contours at Z {z}");
    }
    let p = ring_project(22.0);
    generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
}

/// #192 and `ToolTooLarge`: Inside, a 6 mm square outline is too small
/// for the 6.35 mm bit and is skipped, but the lap round the 2 mm
/// island in it would run outside the outline, into the part. That is
/// refused, not skipped: the island's path exists, and would gouge.
#[test]
fn inside_island_in_an_outline_too_small_for_the_tool_is_refused() {
    let p = project(
        vec![
            Polyline::rect(Point::new(0.0, 0.0), 6.0, 6.0),
            Polyline::rect(Point::new(2.0, 2.0), 2.0, 2.0),
        ],
        &[("i1", at_20_20())],
    );
    let err = generate_profile(&p, &profile_op(&p, Side::Inside, CutDirection::Climb))
        .expect_err("the island's lap would leave the outline");
    let text = err.to_string();
    assert!(
        text.contains("profile 0") && text.contains("profile 1"),
        "{text}"
    );
}

/// The band a refused Profile of `p` names: its two shapes' profiles
/// and its width.
fn refused_band(p: &Project, side: Side) -> (usize, usize, f64) {
    let err = generate_profile(p, &profile_op(p, side, CutDirection::Climb))
        .expect_err("a band narrower than the bit is refused");
    let CamError::BandNarrowerThanTool {
        op,
        shape,
        other,
        band_mm,
        diameter_mm,
    } = &err
    else {
        panic!("expected BandNarrowerThanTool, got {err:?}");
    };
    assert_eq!(op.as_str(), "o1");
    assert_approx_eq!(*diameter_mm, 6.35, TOL);
    (shape.profile, other.profile, *band_mm)
}

/// On line, the tool centre follows each line and the bit cuts
/// `r` of the ring on both sides of it, so the outline's cut and the
/// hole's together cut through a ring narrower than the bit: the part
/// loses material it keeps, as in #192's Inside band. A 3 mm ring
/// (closer than `r`, the issue's case) and a 4.5 mm one (wider than
/// `r`, narrower than the 6.35 mm bit) are both refused, and so is a
/// 2 mm web between two holes of one part.
#[test]
fn on_line_ring_narrower_than_tool_is_refused() {
    for band_mm in [3.0, 4.5] {
        let (shape, other, band) = refused_band(&ring_project(25.0 - band_mm), Side::OnLine);
        assert_eq!((shape, other), (0, 1), "outline first");
        assert_approx_eq!(band, band_mm, TOL);
    }
    let web = project(
        vec![
            rect_100x50(),
            Polyline::circle(Point::new(30.0, 25.0), 10.0),
            Polyline::circle(Point::new(52.0, 25.0), 10.0),
        ],
        &[("i1", at_20_20())],
    );
    let (shape, other, band) = refused_band(&web, Side::OnLine);
    assert_eq!((shape, other), (1, 2), "the two holes, in drawing order");
    assert_approx_eq!(band, 2.0, TOL);
}

/// An On line ring wider than the bit is cut as drawn, both
/// lines at every pass, with no finding.
#[test]
fn on_line_ring_wider_than_the_tool_is_cut() {
    let p = ring_project(18.0);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    for z in PASS_Z_MM {
        assert_eq!(laps_at(&tp, z).len(), 2, "both lines at Z {z}");
    }
}

#[test]
fn every_cut_move_has_feed_and_every_rapid_has_none() {
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert!(!tp.moves.is_empty());
    for mv in &tp.moves {
        match mv.kind {
            MoveKind::Rapid => assert_eq!(mv.feed_mm_min, None, "{mv:?}"),
            _ => {
                let feed = mv.feed_mm_min.unwrap_or_else(|| panic!("no feed: {mv:?}"));
                assert!(
                    [FEED_MM_MIN, PLUNGE_MM_MIN]
                        .iter()
                        .any(|f| (feed - f).abs() <= EPS_MM),
                    "{mv:?}"
                );
            }
        }
    }
    for lap in &laps(&tp) {
        for mv in &lap.moves {
            assert_approx_eq!(mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
        }
    }
}

#[test]
fn first_move_is_rapid_at_safe_z() {
    let p = rect_project();
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let first = tp.moves[0];
    assert!(first.is_rapid(), "{first:?}");
    assert_approx_eq!(first.to.z, SAFE_Z_MM, TOL);
    let lap_start = laps(&tp)[0].points[0];
    assert_approx_eq!(first.to.x, lap_start.x, TOL);
    assert_approx_eq!(first.to.y, lap_start.y, TOL);
    let last = tp.moves.last().unwrap();
    assert!(last.is_rapid(), "{last:?}");
    assert_approx_eq!(last.to.z, SAFE_Z_MM, TOL);
}

#[test]
fn descent_uses_plunge_feed() {
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let mut descents = 0;
    for pair in tp.moves.windows(2) {
        let (prev, mv) = (pair[0], pair[1]);
        if mv.is_rapid() && mv.to.z < prev.to.z - EPS_MM {
            // From safe Z straight down to 1 mm above the stock.
            assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
            assert_approx_eq!(mv.to.z, THICKNESS_MM + 1.0, TOL);
            assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM, "vertical");
        } else if mv.to.z < prev.to.z - EPS_MM {
            descents += 1;
            assert_eq!(mv.kind, MoveKind::Linear, "{mv:?}");
            assert_approx_eq!(mv.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
            assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM, "vertical");
        }
    }
    assert_eq!(descents, 2 * PASS_Z_MM.len(), "one per pass per contour");
}

#[test]
fn retracts_to_safe_z_between_contours() {
    let p = ring_project(10.0);
    let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let mut retracts = 0;
    for (i, pair) in tp.moves.windows(2).enumerate() {
        let (prev, mv) = (pair[0], pair[1]);
        if !mv.is_rapid() {
            continue;
        }
        if mv.to.z < prev.to.z - EPS_MM {
            // Down from safe Z to 1 mm above the stock, in Z
            // alone, before a plunge.
            assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
            assert_approx_eq!(mv.to.z, THICKNESS_MM + 1.0, TOL);
            assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM);
            continue;
        }
        assert_approx_eq!(mv.to.z, SAFE_Z_MM, TOL);
        if prev.to.z < SAFE_Z_MM - EPS_MM {
            retracts += 1;
            assert!(
                prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM,
                "retract {i} is vertical"
            );
        } else {
            assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
        }
    }
    assert_eq!(retracts, 2, "one retract after each contour");
    // Hole done, retract, rapid across at safe Z, outer starts; and a
    // rapid down before each contour's first plunge.
    let rapids: Vec<&Move> = tp.moves.iter().filter(|m| m.is_rapid()).collect();
    assert_eq!(rapids.len(), 4 + 2);
}

#[test]
fn crossing_targets_fail_naming_both_instances() {
    let p = project(
        vec![rect_100x50()],
        &[("i1", at_20_20()), ("i2", Transform::translate(70.0, 45.0))],
    );
    let err =
        generate_profile(&p, &profile_op(&p, Side::Outside, CutDirection::Climb)).unwrap_err();
    let CamError::TargetsCross { first, second } = &err else {
        panic!("expected TargetsCross, got {err:?}");
    };
    assert_eq!(
        first,
        &ShapeRef {
            instance: InstanceId::new("i1"),
            instance_name: "Shape i1".into(),
            profile: 0,
        }
    );
    assert_eq!(second.instance, InstanceId::new("i2"));
    let message = err.to_string();
    assert!(
        message.contains("i1") && message.contains("i2"),
        "{message}"
    );
}

#[test]
fn duplicate_target_is_cut_once_with_warning() {
    let p = project(
        vec![rect_100x50()],
        &[("i1", at_20_20()), ("i2", at_20_20())],
    );
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
    let Warning::DuplicateShapeDropped {
        shape,
        duplicate_of,
    } = &warnings[0]
    else {
        panic!("{:?}", warnings[0]);
    };
    assert_eq!(shape.instance.as_str(), "i2");
    assert_eq!(duplicate_of.instance.as_str(), "i1");
    for z in PASS_Z_MM {
        assert_eq!(laps_at(&tp, z).len(), 1, "one contour at Z {z}");
    }
}

#[test]
fn missing_feed_is_error_not_invented() {
    let mut p = rect_project();
    p.tools[0].feed_mm_min = None;
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingFeed {
            tool: "6.35 mm upcut".into(),
            tool_id: ToolId::new("t1"),
            field: FeedField::Feed
        }
    );

    let mut p = rect_project();
    p.tools[0].plunge_mm_min = None;
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingFeed {
            tool: "6.35 mm upcut".into(),
            tool_id: ToolId::new("t1"),
            field: FeedField::Plunge
        }
    );

    let mut p = rect_project();
    p.tools[0].feed_mm_min = Some(0.0);
    assert!(matches!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingFeed { .. }
    ));
}

/// Two tools may share a name, so an error about a tool carries
/// its id as well as the name it prints.
#[test]
fn tool_errors_carry_the_tool_id() {
    let tool_id = |err: CamError| match err {
        CamError::MissingFeed { tool_id, .. }
        | CamError::CutterNotFlat { tool_id, .. }
        | CamError::MissingStepDown { tool_id, .. }
        | CamError::InvalidToolDiameter { tool_id, .. } => tool_id,
        other => panic!("a tool error, not {other:?}"),
    };
    let t1 = ToolId::new("t1");
    let op = profile_op(&rect_project(), Side::Outside, CutDirection::Climb);
    let fails = |change: &dyn Fn(&mut Project)| {
        let mut p = rect_project();
        change(&mut p);
        assert_eq!(p.tools[0].id, t1);
        generate_profile(&p, &op).unwrap_err()
    };
    let errors = [
        fails(&|p| p.tools[0].feed_mm_min = None),
        fails(&|p| p.tools[0].cutter = dc_project::ToolKind::BallNose),
        fails(&|p| p.tools[0].diameter_mm = 0.0),
        fails(&|p| p.tools[0].step_down_mm = None),
    ];
    for err in errors {
        assert_eq!(tool_id(err.clone()), t1, "{err:?}");
    }
}

/// Profile offsets by half the diameter, right only for a flat end
/// mill. Validation reports it first; the generator refuses too, so no
/// caller gets a wrong-size part.
#[test]
fn v_bit_in_profile_is_an_error() {
    let mut p = rect_project();
    p.tools[0].cutter = dc_project::ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::CutterNotFlat {
            op: OperationId::new("o1"),
            tool: "6.35 mm upcut".into(),
            tool_id: ToolId::new("t1"),
            cutter: "a V-bit",
        }
    );
    for cutter in [
        dc_project::ToolKind::BallNose,
        dc_project::ToolKind::Engraver,
    ] {
        p.tools[0].cutter = cutter;
        for side in [Side::Outside, Side::Inside] {
            let op = profile_op(&p, side, CutDirection::Climb);
            assert!(matches!(
                generate_profile(&p, &op).unwrap_err(),
                CamError::CutterNotFlat { .. }
            ));
        }
    }
}

/// #67 (maintainer, PR #96): on the line the tool centre follows the
/// line, so any cutter cuts it as drawn.
#[test]
fn v_bit_on_line_is_allowed() {
    for cutter in [
        dc_project::ToolKind::VBit {
            included_angle_deg: 60.0,
        },
        dc_project::ToolKind::BallNose,
        dc_project::ToolKind::Engraver,
    ] {
        let mut p = rect_project();
        p.tools[0].cutter = cutter;
        let op = profile_op(&p, Side::OnLine, CutDirection::Climb);
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[test]
fn missing_step_down_is_error() {
    let mut p = rect_project();
    p.tools[0].step_down_mm = None;
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingStepDown {
            op: OperationId::new("o1"),
            tool: "6.35 mm upcut".into(),
            tool_id: ToolId::new("t1"),
        }
    );
}

#[test]
fn operation_step_down_overrides_tool() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.step_down_mm = Some(3.0);
    let (tp, _) = generate(&p, &op);
    let z: Vec<f64> = laps(&tp).iter().map(|l| l.z_mm).collect();
    assert_eq!(z.len(), 2);
    assert_approx_eq!(z[0], 3.0, TOL);
    assert_approx_eq!(z[1], 0.0, TOL);
}

#[test]
fn through_cut_goes_exactly_to_the_allowance() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.depth_mm = Some(THICKNESS_MM + 0.5);
    op.through_cut_allowance_mm = 0.5;
    let (tp, _) = generate(&p, &op);
    let lowest = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
    assert_approx_eq!(lowest, -0.5, TOL);

    op.through_cut_allowance_mm = 0.0;
    assert!(matches!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::BelowMaterialBottom { .. }
    ));
}

/// An unset thickness or depth fails generation; neither is
/// defaulted. Validation reports both first, so this is the generator
/// protecting itself.
#[test]
fn missing_thickness_or_depth_is_an_error() {
    let mut p = rect_project();
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    p.stock.thickness_mm = None;
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingThickness
    );

    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.depth_mm = None;
    assert_eq!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::MissingDepth {
            op: OperationId::new("o1")
        }
    );
}

#[test]
fn unknown_ids_are_errors() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.tool = ToolId::new("nope");
    assert!(matches!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::UnknownTool { .. }
    ));
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.targets.push(InstanceId::new("nope"));
    assert!(matches!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::UnknownInstance { .. }
    ));
}

#[test]
fn non_positive_safe_z_is_error() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    op.safe_z_mm = 0.0;
    assert!(matches!(
        generate_profile(&p, &op).unwrap_err(),
        CamError::InvalidSafeZ { .. }
    ));
}

#[test]
fn output_is_deterministic() {
    let p = ring_project(10.0);
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    assert_eq!(
        generate_profile(&p, &op).unwrap(),
        generate_profile(&p, &op).unwrap()
    );
}

/// O6 (acceptance test 31): a one-shape operation is the phase 1
/// recipe move for move, whatever the machining order: a rapid to the
/// start at safe Z, then per pass a plunge at the plunge rate and one
/// lap at the feed rate, then a rapid up. The expected lap is built here
/// from the offset contour directly, not through the sequencer.
/// (changed in #227: the first plunge starts from a rapid straight down
/// to 1 mm above the stock, not from safe Z.)
#[test]
fn one_shape_golden_is_byte_identical_after_sequence() {
    let p = rect_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    let (full, _) = generate(&p, &op);
    op.machining_order = MachiningOrder::PassFirst;
    let (pass, _) = generate(&p, &op);
    assert_eq!(full, pass);

    let placed = at_20_20().apply_polyline(&rect_100x50()).unwrap();
    let centre = offset(&Contour::new(placed).unwrap(), 3.175).remove(0);
    // Outside, climb: clockwise.
    let path = if centre.is_ccw() {
        centre.reversed().into_polyline()
    } else {
        centre.into_polyline()
    };
    let start = path.vertices()[0].point();
    let mut expected = vec![
        Move::rapid(Point3::new(start.x, start.y, SAFE_Z_MM)),
        Move::rapid(Point3::new(start.x, start.y, THICKNESS_MM + 1.0)),
    ];
    for z in PASS_Z_MM {
        expected.push(Move::linear(
            Point3::new(start.x, start.y, z),
            PLUNGE_MM_MIN,
        ));
        for segment in path.segments() {
            let kind = match segment.arc() {
                Some(arc) if arc.sweep_rad > 0.0 => MoveKind::ArcCcw { centre: arc.centre },
                Some(arc) => MoveKind::ArcCw { centre: arc.centre },
                None => MoveKind::Linear,
            };
            expected.push(Move {
                kind,
                to: Point3::new(segment.end.x, segment.end.y, z),
                feed_mm_min: Some(FEED_MM_MIN),
            });
        }
    }
    expected.push(Move::rapid(Point3::new(start.x, start.y, SAFE_Z_MM)));
    assert_eq!(full.moves, expected);
}

/// Three 20 mm squares drawn at X = 10, 50, 30, placed by three
/// instances.
fn three_squares_project() -> Project {
    project(
        vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)],
        &[
            ("i1", Transform::translate(10.0, 20.0)),
            ("i2", Transform::translate(50.0, 20.0)),
            ("i3", Transform::translate(30.0, 20.0)),
        ],
    )
}

/// The lowest X of each lap, in cut order.
fn lap_min_x(toolpath: &Toolpath) -> Vec<f64> {
    laps(toolpath)
        .iter()
        .map(|lap| lap.points.iter().map(|p| p.x).fold(f64::MAX, f64::min))
        .collect()
}

/// Acceptance test 31 (D5): squares drawn at X = 10, 50, 30 are cut at
/// 10, 30, 50 under full depth first. Under pass first each layer's
/// walk starts where the previous one ended, so the second layer runs
/// back 50, 30, 10.
#[test]
fn nearest_neighbour_visits_the_closest_part_next() {
    let p = three_squares_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    let r = 3.175;
    let at = |xs: &[f64]| -> Vec<f64> { xs.iter().map(|x| x - r).collect() };

    let (tp, _) = generate(&p, &op);
    let expected = at(&[10.0, 10.0, 10.0, 30.0, 30.0, 30.0, 50.0, 50.0, 50.0]);
    let got = lap_min_x(&tp);
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(&expected) {
        assert_approx_eq!(*g, *e, TOL);
    }

    op.machining_order = MachiningOrder::PassFirst;
    let (tp, _) = generate(&p, &op);
    let expected = at(&[10.0, 30.0, 50.0, 50.0, 30.0, 10.0, 10.0, 30.0, 50.0]);
    let got = lap_min_x(&tp);
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(&expected) {
        assert_approx_eq!(*g, *e, TOL);
    }
}

/// Acceptance test 30: three squares 6 mm deep in 2 mm passes. Under
/// pass first the Z of successive plunges is 4, 4, 4, 2, 2, 2, 0, 0, 0;
/// under full depth first it is 4, 2, 0 three times.
#[test]
fn pass_first_plunges_every_shape_before_stepping_down() {
    let p = three_squares_project();
    let plunge_zs = |tp: &Toolpath| -> Vec<f64> {
        tp.moves
            .windows(2)
            .filter(|pair| {
                let (prev, mv) = (pair[0], pair[1]);
                !mv.is_rapid()
                    && mv.to.z < prev.to.z - EPS_MM
                    && prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM
            })
            .map(|pair| pair[1].to.z)
            .collect()
    };
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    let (tp, _) = generate(&p, &op);
    let got = plunge_zs(&tp);
    let expected = [4.0, 2.0, 0.0, 4.0, 2.0, 0.0, 4.0, 2.0, 0.0];
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(expected) {
        assert_approx_eq!(*g, e, TOL);
    }

    op.machining_order = MachiningOrder::PassFirst;
    let (tp, _) = generate(&p, &op);
    let got = plunge_zs(&tp);
    let expected = [4.0, 4.0, 4.0, 2.0, 2.0, 2.0, 0.0, 0.0, 0.0];
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (g, e) in got.iter().zip(expected) {
        assert_approx_eq!(*g, e, TOL);
    }
}

/// A tab warning's `contour` counts contours in cut order, which
/// nearest neighbour sets: the square at X = 30 (instance `i3`) is cut
/// second, so it is contour 1.
#[test]
fn tab_warnings_index_contours_in_cut_order() {
    let p = three_squares_project();
    let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
    // Four 10 mm tabs plus the 6.35 mm diameter cover 65 mm of a
    // 100 mm path: more than half, so every square warns.
    op.tabs = Some(TabSpec {
        placement: TabPlacement::Count { count: 4 },
        width_mm: 10.0,
        height_mm: 3.0,
        shape: TabShape::Rect,
        on_holes: false,
        avoid_corners: false,
    });
    let (_, warnings) = generate(&p, &op);
    let covered: Vec<(usize, &str)> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::TabsCoverMostOfContour {
                contour,
                shape: Some(shape),
                ..
            } => Some((*contour, shape.instance.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(covered, [(0, "i1"), (1, "i3"), (2, "i2")]);
}

// ── #109: a part and its hole in one operation ──

const STAR_CENTRE: Point = Point::new(50.0, 50.0);
const STAR_HOLE_CENTRE: Point = Point::new(50.0, 70.0);

/// The maintainer's manual case (`tests/manual/star_with_hole`),
/// redrawn with straight edges: a five-pointed star (points 40 mm and
/// notches 22 mm from (50, 50), one point straight up) as instance
/// `i1`, and a 7 mm circle 20 mm up its top arm as instance `i2`,
/// leaving a 6.6 mm web. 100 × 100 × 12 mm stock.
fn star_with_hole_project() -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 100.0,
        height_mm: 100.0,
        thickness_mm: Some(12.0),
    };
    let star = (0..10u32)
        .map(|k| {
            let angle_rad = PI / 2.0 + f64::from(k) * PI / 5.0;
            let r_mm = if k % 2 == 0 { 40.0 } else { 22.0 };
            Vertex::line(
                STAR_CENTRE.x + r_mm * angle_rad.cos(),
                STAR_CENTRE.y + r_mm * angle_rad.sin(),
            )
        })
        .collect();
    let definitions = [
        ("d1", "Star", Polyline::new(star, true)),
        ("d2", "Hole", Polyline::circle(STAR_HOLE_CENTRE, 3.5)),
    ];
    p.definitions = definitions
        .iter()
        .map(|(id, name, profile)| {
            let def = FeatureDefinition {
                id: DefinitionId::new(*id),
                name: (*name).into(),
                shape: None,
                profiles: vec![profile.clone()],
            };
            (def.id.clone(), def)
        })
        .collect();
    p.instances = [("i1", "d1", "star"), ("i2", "d2", "hole")]
        .iter()
        .map(|(id, def, name)| FeatureInstance {
            id: InstanceId::new(*id),
            definition: DefinitionId::new(*def),
            transform: Transform::IDENTITY,
            name: (*name).into(),
            visible: true,
            locked: false,
            construction: false,
            group: None,
            z_top_mm: Some(12.0),
            z_bottom_mm: Some(0.0),
        })
        .collect();
    p.operations.clear();
    p
}

/// One Outside Profile of the star and its hole, with four
/// smooth 6 × 3 mm tabs and the 6.35 mm bit, 12 mm deep in 4 mm
/// passes, as the manual file set it. Containment cuts the hole first; the
/// hole, a slug of waste, gets no tabs and is cut free; only the
/// star's last pass rises over tabs, four times. Before #109 this
/// took two operations in the right order.
#[test]
fn star_and_its_hole_cut_in_one_operation() {
    let p = star_with_hole_project();
    let op = ProfileOp {
        targets: vec![InstanceId::new("i1"), InstanceId::new("i2")],
        depth_mm: Some(12.0),
        step_down_mm: Some(4.0),
        through_cut_allowance_mm: 0.01,
        tabs: Some(TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 6.0,
            height_mm: 3.0,
            shape: TabShape::Smooth,
            on_holes: false,
            avoid_corners: false,
        }),
        ..profile_op(&p, Side::Outside, CutDirection::Climb)
    };
    let (tp, warnings) = generate(&p, &op);
    assert!(
        !warnings.iter().any(|w| matches!(
            w.code(),
            WarningCode::TabsCoverMostOfContour | WarningCode::TabsOverlap
        )),
        "{warnings:?}"
    );

    // Every move along a path, with where it starts.
    let mut cuts: Vec<(Point3, Move)> = Vec::new();
    for pair in tp.moves.windows(2) {
        let (from, mv) = (pair[0].to, pair[1]);
        if !mv.is_rapid() && from.xy().distance_to(mv.to.xy()) > EPS_MM {
            cuts.push((from, mv));
        }
    }
    let in_hole = |(from, mv): &(Point3, Move)| {
        from.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
            && mv.to.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
    };
    let last_hole = cuts.iter().rposition(in_hole).expect("the hole is cut");
    let first_star = cuts
        .iter()
        .position(|c| !in_hole(c))
        .expect("the star is cut");
    assert!(last_hole < first_star, "the hole is cut before the star");

    // The depth is the stock: the last pass is at the stock bottom.
    let floor_mm = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
    assert_approx_eq!(floor_mm, 0.0, TOL);
    let on_last_pass = |(from, mv): &(Point3, Move)| from.z < 3.5 && mv.to.z < 3.5;
    let level = |(from, mv): &(Point3, Move)| {
        (from.z - floor_mm).abs() <= TOL && (mv.to.z - floor_mm).abs() <= TOL
    };

    let hole_last: Vec<_> = cuts[..=last_hole]
        .iter()
        .filter(|c| on_last_pass(c))
        .collect();
    assert!(!hole_last.is_empty(), "the hole's last pass is cut");
    for cut in &hole_last {
        assert!(level(cut), "the hole has no tabs: {cut:?}");
    }

    let star_last: Vec<_> = cuts[first_star..]
        .iter()
        .filter(|c| on_last_pass(c))
        .collect();
    let raised: Vec<bool> = star_last.iter().map(|c| !level(c)).collect();
    let mut tabs = raised.windows(2).filter(|w| !w[0] && w[1]).count();
    if raised.first() == Some(&true) && raised.last() != Some(&true) {
        tabs += 1;
    }
    assert_eq!(tabs, 4, "the star rises over its four tabs");
    let top_mm = star_last
        .iter()
        .map(|(from, mv)| from.z.max(mv.to.z))
        .fold(f64::MIN, f64::max);
    assert_approx_eq!(top_mm, 3.0, TOL);
}

/// #105, the issue's second test: one Outside Profile of the star and
/// its hole with no tab request, and four smooth 6 × 3 mm tabs placed
/// on the star only (`tabs place --at`, holes left out): the hole is
/// cut free and only the star's last pass rises, four times, whatever
/// `on_holes` would have said. A tab placed on the hole as well is cut,
/// so the hole's last pass rises too, and warns `tab_on_hole`.
#[test]
fn star_with_tabs_placed_only_on_the_star_lifts_only_on_the_star() {
    let mut p = star_with_hole_project();
    let op = ProfileOp {
        targets: vec![InstanceId::new("i1"), InstanceId::new("i2")],
        depth_mm: Some(12.0),
        step_down_mm: Some(4.0),
        through_cut_allowance_mm: 0.01,
        tabs: None,
        ..profile_op(&p, Side::Outside, CutDirection::Climb)
    };
    p.operations = vec![Operation::Profile(op.clone())];
    let star_mm = p.definitions[&DefinitionId::new("d1")].profiles[0].length_mm();
    let at: Vec<f64> = [0.5, 1.5, 2.5, 3.5].map(|k| star_mm * k / 4.0).to_vec();
    let size = crate::placed_tabs::TabSize {
        width_mm: 6.0,
        height_mm: 3.0,
        shape: TabShape::Smooth,
    };
    let placed = crate::placed_tabs::place_on_outlines(&p, &op.id, &at, size, false).unwrap();
    assert_eq!(placed.tabs.len(), 4, "{placed:?}");
    assert!(placed.tabs.iter().all(|t| t.instance.as_str() == "i1"));
    p.tabs = placed.tabs;

    let (tp, warnings) = generate(&p, &op);
    assert!(
        !codes(&warnings).contains(&WarningCode::TabOnHole),
        "{warnings:?}"
    );
    let (hole_rises, star_tabs, top_mm) = star_and_hole_rises(&tp);
    assert!(!hole_rises, "the hole has no tabs");
    assert_eq!(star_tabs, 4, "the star rises over its four tabs");
    assert_approx_eq!(top_mm, 3.0, TOL);

    let mut on_hole = p.tabs[0].clone();
    on_hole.id = dc_project::TabId::new("tab9");
    on_hole.instance = InstanceId::new("i2");
    on_hole.x_mm = STAR_HOLE_CENTRE.x + 3.5;
    on_hole.y_mm = STAR_HOLE_CENTRE.y;
    on_hole.width_mm = 1.0;
    p.tabs.push(on_hole);
    let (tp, warnings) = generate(&p, &op);
    assert!(
        codes(&warnings).contains(&WarningCode::TabOnHole),
        "{warnings:?}"
    );
    let (hole_rises, star_tabs, _) = star_and_hole_rises(&tp);
    assert!(hole_rises, "the hole's tab holds its slug");
    assert_eq!(star_tabs, 4);
}

/// The last pass of a star-and-hole toolpath: whether the hole's
/// leaves the floor, how many times the star's rises, and the highest
/// Z the star's reaches.
fn star_and_hole_rises(tp: &Toolpath) -> (bool, usize, f64) {
    let mut cuts: Vec<(Point3, Move)> = Vec::new();
    for pair in tp.moves.windows(2) {
        let (from, mv) = (pair[0].to, pair[1]);
        if !mv.is_rapid() && from.xy().distance_to(mv.to.xy()) > EPS_MM {
            cuts.push((from, mv));
        }
    }
    let in_hole = |(from, mv): &(Point3, Move)| {
        from.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
            && mv.to.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
    };
    let floor_mm = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
    let on_last_pass = |(from, mv): &(Point3, Move)| from.z < 3.5 && mv.to.z < 3.5;
    let level = |(from, mv): &(Point3, Move)| {
        (from.z - floor_mm).abs() <= TOL && (mv.to.z - floor_mm).abs() <= TOL
    };
    let hole_rises = cuts
        .iter()
        .filter(|c| in_hole(c) && on_last_pass(c))
        .any(|c| !level(c));
    let star_last: Vec<_> = cuts
        .iter()
        .filter(|c| !in_hole(c) && on_last_pass(c))
        .collect();
    let raised: Vec<bool> = star_last.iter().map(|c| !level(c)).collect();
    let mut tabs = raised.windows(2).filter(|w| !w[0] && w[1]).count();
    if raised.first() == Some(&true) && raised.last() != Some(&true) {
        tabs += 1;
    }
    let top_mm = star_last
        .iter()
        .map(|(from, mv)| from.z.max(mv.to.z))
        .fold(f64::MIN, f64::max);
    (hole_rises, tabs, top_mm)
}

// ── #176: the start moves off a tab ──

fn ramp_15() -> Entry {
    Entry::Ramp { angle_deg: 15.0 }
}

/// One `shape` tab 8 mm wide and 3 mm tall (so the passes at Z 2 and 0
/// rise over it), centred `at_mm` along the path.
fn tab_at(at_mm: f64, shape: TabShape) -> TabSpec {
    TabSpec {
        placement: TabPlacement::AtDistances {
            distances_mm: vec![at_mm],
        },
        width_mm: 8.0,
        height_mm: TAB_TOP_MM,
        shape,
        on_holes: false,
        avoid_corners: false,
    }
}

const TAB_TOP_MM: f64 = 3.0;
/// Length of path one 8 mm tab raises with the 6.35 mm tool.
const TAB_SPAN_MM: f64 = 8.0 + 6.35;

/// Every straight-down feed move that ends below the stock top, with
/// the point it starts from.
fn plunges_below_top(tp: &Toolpath) -> Vec<(Point3, Move)> {
    tp.moves
        .windows(2)
        .filter(|w| {
            let (from, mv) = (w[0].to, w[1]);
            !mv.is_rapid()
                && mv.to.z < from.z - EPS_MM
                && from.xy().distance_to(mv.to.xy()) <= EPS_MM
                && mv.to.z < THICKNESS_MM - EPS_MM
        })
        .map(|w| (w[0].to, w[1]))
        .collect()
}

/// Where the first move that falls while it moves in XY (the first
/// ramp leg) starts.
fn first_ramp_start(tp: &Toolpath) -> Point {
    tp.moves
        .windows(2)
        .find(|w| {
            let (from, mv) = (w[0].to, w[1]);
            !mv.is_rapid()
                && mv.to.z < from.z - EPS_MM
                && from.xy().distance_to(mv.to.xy()) > EPS_MM
        })
        .map(|w| w[0].to.xy())
        .expect("the toolpath ramps")
}

/// The level cutting moves at the tab top, as `(from, to)` in XY, and
/// their length along the path (arcs as arcs).
fn held_at_tab_top(tp: &Toolpath) -> (Vec<(Point, Point)>, f64) {
    let mut held = Vec::new();
    let mut length_mm = 0.0;
    for w in tp.moves.windows(2) {
        let (from, mv) = (w[0].to, w[1]);
        if mv.is_rapid()
            || (from.z - TAB_TOP_MM).abs() > EPS_MM
            || (mv.to.z - TAB_TOP_MM).abs() > EPS_MM
            || from.xy().distance_to(mv.to.xy()) <= EPS_MM
        {
            continue;
        }
        let (a, b) = (from.xy(), mv.to.xy());
        length_mm += match mv.kind {
            MoveKind::ArcCcw { centre } => a.distance_to(centre) * sweep_rad(a, b, centre, true),
            MoveKind::ArcCw { centre } => -a.distance_to(centre) * sweep_rad(a, b, centre, false),
            _ => a.distance_to(b),
        };
        held.push((a, b));
    }
    (held, length_mm)
}

/// With a tab centred on the path's start, a ramp below the tab
/// top would run through the tab, so slice 10 plunged onto it with
/// `entry_fallback`. The start moves to where the ramp fits clear of
/// the tab: no entry falls back and no straight plunge goes below the
/// stock top, on every side. A start the ramp already fits at stays.
#[test]
fn start_under_a_tab_moves_so_the_ramp_fits() {
    let p = rect_project();
    for side in [Side::Inside, Side::Outside, Side::OnLine] {
        let mut op = profile_op(&p, side, CutDirection::Climb);
        op.entry = ramp_15();
        let (untabbed, _) = generate(&p, &op);
        let start = first_ramp_start(&untabbed);

        op.tabs = Some(tab_at(0.0, TabShape::Smooth));
        let (tp, warnings) = generate(&p, &op);
        assert!(
            !codes(&warnings).contains(&WarningCode::EntryFallback),
            "{side:?}: {warnings:?}"
        );
        assert!(
            plunges_below_top(&tp).is_empty(),
            "{side:?}: {:?}",
            plunges_below_top(&tp)
        );
        assert!(
            first_ramp_start(&tp).distance_to(start) > TAB_SPAN_MM / 2.0,
            "{side:?}: the start left the tab"
        );

        // Two tabs, a quarter and three quarters round: the ramp fits
        // at the start, which is kept.
        op.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 2 },
            ..tab_at(0.0, TabShape::Smooth)
        });
        let (kept, warnings) = generate(&p, &op);
        assert!(
            !codes(&warnings).contains(&WarningCode::EntryFallback),
            "{side:?}: {warnings:?}"
        );
        assert!(
            first_ramp_start(&kept).distance_to(start) <= EPS_MM,
            "{side:?}: the start is kept"
        );
    }
}

/// Moving the start does not move the tabs. A tab is placed by
/// its distance from the path's own start, where the plunge entry
/// (which keeps the start) cuts it; with a ramp the start moves and
/// the tab top is held over exactly the same stretch.
#[test]
fn tab_positions_do_not_move_when_the_seam_moves() {
    let p = rect_project();
    for side in [Side::Inside, Side::Outside, Side::OnLine] {
        let mut op = profile_op(&p, side, CutDirection::Climb);
        op.tabs = Some(tab_at(0.0, TabShape::Rect));
        let (plunged, _) = generate(&p, &op);
        // The plunge entry starts at the path start, the tab's centre.
        let centre = plunged.moves[0].to.xy();
        op.entry = ramp_15();
        let (ramped, warnings) = generate(&p, &op);
        assert!(
            !codes(&warnings).contains(&WarningCode::EntryFallback),
            "{side:?}: {warnings:?}"
        );
        assert!(
            ramped.moves[0].to.xy().distance_to(centre) > TAB_SPAN_MM / 2.0,
            "{side:?}: the seam moved off the tab"
        );
        let ((_, before_mm), (after, after_mm)) =
            (held_at_tab_top(&plunged), held_at_tab_top(&ramped));
        // Two passes (Z 2 and 0) below the tab top, one span each.
        assert_approx_eq!(before_mm, 2.0 * TAB_SPAN_MM, 1e-6);
        assert_approx_eq!(after_mm, before_mm, 1e-6);
        for (a, b) in after {
            for q in [a, b] {
                assert!(
                    q.distance_to(centre) <= TAB_SPAN_MM / 2.0 + 1e-6,
                    "{side:?}: {q:?} is held off the tab at {centre:?}"
                );
            }
        }
    }
}

// ── #175: On line Profile of an open path ──

/// A 100 mm open line from (20, 20) to (120, 20).
fn line_project() -> Project {
    project(
        vec![Polyline::new(
            vec![Vertex::line(0.0, 0.0), Vertex::line(100.0, 0.0)],
            false,
        )],
        &[("i1", at_20_20())],
    )
}

/// Every move that cuts in XY, with the point it starts from.
fn xy_cuts(tp: &Toolpath) -> Vec<(Point3, Move)> {
    tp.moves
        .windows(2)
        .filter(|w| !w[1].is_rapid() && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM)
        .map(|w| (w[0].to, w[1]))
        .collect()
}

/// An On line Profile cuts an open path, as VCarve's Profile
/// "On" cuts an open vector. Each pass runs the line from its first
/// vertex to its last at the pass Z (conventional), then the tool
/// retracts and comes back to the first vertex for the next. (changed
/// in #242: climb ran first to last; VCarve starts conventional there.)
#[test]
fn on_line_cuts_an_open_path() {
    let p = line_project();
    let (tp, warnings) = generate(
        &p,
        &profile_op(&p, Side::OnLine, CutDirection::Conventional),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let laps = laps(&tp);
    assert_eq!(laps.len(), PASS_Z_MM.len(), "{laps:?}");
    for (lap, z) in laps.iter().zip(PASS_Z_MM) {
        assert_approx_eq!(lap.z_mm, z, TOL);
        assert_approx_eq!(lap.x_extent_mm(), 100.0, TOL);
        assert!(lap.points.iter().all(|q| (q.y - 20.0).abs() <= TOL));
        assert!(lap.points[0].distance_to(Point::new(20.0, 20.0)) <= TOL);
        let end = lap.points[lap.points.len() - 1];
        assert!(end.distance_to(Point::new(120.0, 20.0)) <= TOL);
    }
    for (from, mv) in xy_cuts(&tp) {
        assert!(mv.to.x > from.x, "every cut runs first to last: {mv:?}");
    }
    // Inside and Outside still skip it: an open path has no side.
    for side in [Side::Inside, Side::Outside] {
        let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
        assert!(tp.moves.is_empty(), "{side:?}: nothing is cut");
    }
}

/// An open path with no length has nothing to cut On line; it
/// is skipped with `open_path_skipped` ([`OpenPathReason::NoLength`]).
#[test]
fn on_line_open_path_with_no_length_is_skipped() {
    let dot = Polyline::new(vec![Vertex::line(5.0, 5.0), Vertex::line(5.0, 5.0)], false);
    let p = project(vec![rect_100x50(), dot], &[("i1", at_20_20())]);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
    let Warning::OpenPathSkipped { shape, reason } = &warnings[0] else {
        panic!("{:?}", warnings[0]);
    };
    assert_eq!(*reason, OpenPathReason::NoLength);
    assert_eq!(shape.profile, 1);
    assert!(warnings[0].to_string().contains("no length"));
    assert_eq!(laps(&tp).len(), PASS_Z_MM.len(), "only the rect is cut");
}

/// The direction picks the end an open path's cut starts at, as
/// VCarve's does ("Conventional starts cutting at the Start point
/// Climb cutting starts cutting at the end point"). Checks that every
/// pass of the line, cut `direction`, descends at `start` (the line's
/// X there), runs straight to `end` and ends there.
fn assert_open_line_runs(direction: CutDirection, start: f64, end: f64) {
    let p = line_project();
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, direction));
    assert!(warnings.is_empty(), "{direction:?}: {warnings:?}");
    let laps = laps(&tp);
    assert_eq!(laps.len(), PASS_Z_MM.len(), "{direction:?}: {laps:?}");
    for (lap, z) in laps.iter().zip(PASS_Z_MM) {
        assert_approx_eq!(lap.z_mm, z, TOL);
        assert!(lap.points[0].distance_to(Point::new(start, 20.0)) <= TOL);
        let last = lap.points[lap.points.len() - 1];
        assert!(last.distance_to(Point::new(end, 20.0)) <= TOL, "{last:?}");
    }
    let toward = (end - start).signum();
    for (from, mv) in xy_cuts(&tp) {
        assert!((mv.to.x - from.x) * toward > 0.0, "{direction:?}: {mv:?}");
    }
    let plunges = plunges_below_top(&tp);
    assert!(!plunges.is_empty());
    for (from, _) in plunges {
        assert!(from.xy().distance_to(Point::new(start, 20.0)) <= TOL);
    }
}

/// Conventional cuts an open path from its first vertex to its
/// last, as VCarve's "Conventional starts cutting at the Start point".
/// (Replaces #175's `open_path_direction_picks_the_end_it_starts_at`,
/// which pinned the opposite ends.)
#[test]
fn open_on_line_conventional_starts_at_first_vertex() {
    assert_open_line_runs(CutDirection::Conventional, 20.0, 120.0);
}

/// Climb cuts an open path from its last vertex to its first,
/// as VCarve's "Climb cutting starts cutting at the end point". A tab
/// at a distance stays at the same XY whichever end the cut starts at.
#[test]
fn open_on_line_climb_starts_at_last_vertex() {
    assert_open_line_runs(CutDirection::Climb, 120.0, 20.0);
    let p = line_project();
    let held_xs = |direction| {
        let mut op = profile_op(&p, Side::OnLine, direction);
        op.tabs = Some(tab_at(30.0, TabShape::Rect));
        let (tp, _) = generate(&p, &op);
        let mut xs: Vec<f64> = held_at_tab_top(&tp)
            .0
            .into_iter()
            .flat_map(|(a, b)| [a.x.min(b.x), a.x.max(b.x)])
            .collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    let (climb, conventional) = (
        held_xs(CutDirection::Climb),
        held_xs(CutDirection::Conventional),
    );
    assert!(!climb.is_empty());
    assert_eq!(climb.len(), conventional.len());
    for (a, b) in climb.iter().zip(&conventional) {
        assert_approx_eq!(*a, *b, TOL);
    }
    assert_approx_eq!(climb[0], 50.0 - TAB_SPAN_MM / 2.0, TOL);
    assert_approx_eq!(climb[climb.len() - 1], 50.0 + TAB_SPAN_MM / 2.0, TOL);
}

/// Acceptance 26: an On line Profile of a 100 mm line ramps
/// back and forth along the line itself, at most three diameters from
/// its first vertex, then cuts the whole line at the pass Z. A pass
/// after the first comes down the slot the pass above cut, to that
/// pass's Z, before it ramps. At 5° a leg falls 1.67 mm, less than
/// every pass's descent, so each turns back. (changed in #214: at 15°
/// the first pass, from safe Z, turned back; from 1 mm above the stock
/// one leg reaches it. changed in #242: cut climb, which now starts at
/// the last vertex; conventional starts at the first, as VCarve.)
#[test]
fn on_line_open_path_ramps_back_and_forth_along_the_line() {
    let p = line_project();
    let mut op = profile_op(&p, Side::OnLine, CutDirection::Conventional);
    op.entry = Entry::Ramp { angle_deg: 5.0 };
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let ramp: Vec<(Point3, Move)> = tp
        .moves
        .windows(2)
        .filter(|w| {
            !w[1].is_rapid()
                && w[1].to.z < w[0].to.z - EPS_MM
                && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM
        })
        .map(|w| (w[0].to, w[1]))
        .collect();
    assert!(!ramp.is_empty());
    let mut turned = false;
    for pair in ramp.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        turned |= (a.1.to.x - a.0.x) * (b.1.to.x - b.0.x) < 0.0;
    }
    assert!(turned, "back and forth: {ramp:?}");
    for (from, mv) in &ramp {
        assert_approx_eq!(mv.to.y, 20.0, TOL);
        assert!(mv.to.x >= 20.0 - TOL && mv.to.x <= 20.0 + 3.0 * 6.35 + TOL);
        let slope = (from.z - mv.to.z)
            .atan2(from.xy().distance_to(mv.to.xy()))
            .to_degrees();
        assert!((slope - 5.0).abs() <= 0.1, "slope {slope}");
    }
    for z in PASS_Z_MM {
        assert!(
            ramp.iter().any(|(_, mv)| (mv.to.z - z).abs() <= EPS_MM),
            "pass {z} is ramped into"
        );
        let full = laps_at(&tp, z)
            .iter()
            .any(|lap| (lap.points[lap.points.len() - 1].x - 120.0).abs() <= TOL);
        assert!(full, "pass {z} cuts to the line's end");
    }
    // Straight down only into the slot already cut: to the Z of the
    // pass above, at the first vertex.
    for (from, mv) in plunges_below_top(&tp) {
        assert!(from.xy().distance_to(Point::new(20.0, 20.0)) <= TOL);
        assert!(
            PASS_Z_MM.iter().any(|z| (mv.to.z - z).abs() <= EPS_MM)
                && mv.to.z > PASS_Z_MM[PASS_Z_MM.len() - 1] + EPS_MM,
            "{mv:?}"
        );
    }
}

/// Tabs on an open path are placed by distance from its first
/// vertex as drawn, whichever end the cut starts at.
#[test]
fn open_path_tabs_are_measured_from_its_first_vertex() {
    let p = line_project();
    for direction in [CutDirection::Climb, CutDirection::Conventional] {
        let mut op = profile_op(&p, Side::OnLine, direction);
        op.tabs = Some(tab_at(30.0, TabShape::Rect));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{direction:?}: {warnings:?}");
        let (held, length_mm) = held_at_tab_top(&tp);
        assert_approx_eq!(length_mm, 2.0 * TAB_SPAN_MM, 1e-6);
        for (a, b) in held {
            for q in [a, b] {
                assert!(
                    (q.x - 50.0).abs() <= TAB_SPAN_MM / 2.0 + TOL,
                    "{direction:?}: {q:?}"
                );
            }
        }
    }
}

/// The lowest a cut may go `along_mm` along a path from the centre of
/// one of [`tab_at`]'s smooth tabs and leave the tab standing: the
/// tab top within the tool radius of the centre, then down to the
/// stock bottom over half the tab's 8 mm (the last pass's slope; an
/// earlier pass's is higher).
fn smooth_tab_floor_mm(along_mm: f64) -> f64 {
    let f = ((along_mm.abs() - R_MM) / 4.0).clamp(0.0, 1.0);
    TAB_TOP_MM * (1.0 - f)
}

/// A smooth tab whose span an end of an open path cuts short is held
/// to that end: no room is left for its slope there, so the cut keeps
/// the tab top right to the end, as a rect tab's does and as a stock
/// path's stretch does. The 100 mm line, cut On line, Left and
/// Right, from either end, with one smooth tab 8 mm wide and 3 mm
/// tall centred 0, 2, 5, 96 and 99 mm along it: its span is 7.175 mm
/// either side of the centre, so each runs past an end. No feed move
/// goes below the tab anywhere along it.
///
/// Before, the slope was laid inside what was left of the span, from
/// the path's end: the lap started (or ended) at the pass depth at
/// the end of the path, the tab's centre for one at 0 mm, and the
/// plunge there went through the tab to the stock bottom.
#[test]
fn smooth_tab_cut_short_by_an_open_paths_end_is_not_cut_through() {
    let p = line_project();
    for side in [Side::OnLine, Side::Left, Side::Right] {
        for direction in [CutDirection::Conventional, CutDirection::Climb] {
            for at_mm in [0.0, 2.0, 5.0, 96.0, 99.0] {
                let case = format!("{side:?}, {direction:?}, a tab at {at_mm} mm");
                let mut op = profile_op(&p, side, direction);
                op.tabs = Some(tab_at(at_mm, TabShape::Smooth));
                let (tp, _) = generate(&p, &op);
                let mut over_tab = false;
                for w in tp.moves.windows(2) {
                    let (from, mv) = (w[0].to, w[1]);
                    if mv.is_rapid() {
                        continue;
                    }
                    // Every move here is straight: the path is a line.
                    assert_eq!(mv.kind, MoveKind::Linear, "{case}");
                    for k in 0..=16 {
                        let t = f64::from(k) / 16.0;
                        let x = from.x + (mv.to.x - from.x) * t;
                        let z = from.z + (mv.to.z - from.z) * t;
                        let floor = smooth_tab_floor_mm(x - 20.0 - at_mm);
                        over_tab |= floor > 0.0 && z < TAB_TOP_MM + TOL;
                        assert!(
                            z >= floor - TOL,
                            "{case}: a cut from {from:?} to {:?} is at Z {z} at X {x}, \
                                 below the tab at {floor}",
                            mv.to
                        );
                    }
                }
                assert!(over_tab, "{case}: no pass reaches the tab");
            }
        }
    }
}

/// The numbers of the case above that the safety pass refused (S9, a
/// plunge into a tab): a Right Profile of the line, conventional, so
/// cut from its first vertex at (20, 20 − r), with a smooth tab at
/// 0 mm. The passes at Z 2 and 0 are below the 3 mm tab top. Each
/// comes straight down at the path's start only to the tab top, holds
/// it for a tool radius, and slopes down over the next 4 mm.
#[test]
fn smooth_tab_at_an_open_paths_start_is_come_down_onto() {
    let p = line_project();
    let mut op = profile_op(&p, Side::Right, CutDirection::Conventional);
    op.tabs = Some(tab_at(0.0, TabShape::Smooth));
    let (tp, _) = generate(&p, &op);
    let start = Point::new(20.0, 20.0 - R_MM);
    let plunges = plunges_below_top(&tp);
    assert!(!plunges.is_empty());
    for (from, mv) in &plunges {
        assert!(from.xy().distance_to(start) <= TOL, "{from:?}");
        assert!(
            mv.to.z >= TAB_TOP_MM - TOL,
            "a plunge at the tab's centre ends at Z {}, below its top",
            mv.to.z
        );
    }
    let (held, length_mm) = held_at_tab_top(&tp);
    // Two passes, each holding the top from the start for a radius.
    assert_approx_eq!(length_mm, 2.0 * R_MM, 1e-6);
    for (a, b) in held {
        assert_approx_eq!(a.x.min(b.x), 20.0, TOL);
        assert_approx_eq!(a.x.max(b.x), 20.0 + R_MM, TOL);
    }
    // Down the slope to each pass's depth, 4 mm on.
    for z in [2.0, 0.0] {
        let foot = tp.moves.windows(2).any(|w| {
            (w[0].to.z - TAB_TOP_MM).abs() <= TOL
                && (w[1].to.z - z).abs() <= TOL
                && (w[1].to.x - (20.0 + R_MM + 4.0)).abs() <= TOL
        });
        assert!(foot, "the pass at Z {z} slopes down off the tab");
    }
}

// ── #207: Left and Right of an open path ──

/// The 6.35 mm tool's radius.
const R_MM: f64 = 6.35 / 2.0;

/// A Profile of `p` on `side`, `direction`, with `edit` applied.
fn sided_op(
    p: &Project,
    side: Side,
    direction: CutDirection,
    edit: impl FnOnce(&mut ProfileOp),
) -> ProfileOp {
    let mut op = profile_op(p, side, direction);
    edit(&mut op);
    op
}

/// Checks that every lap of `tp` runs at `fixed` across (Y when
/// `along_x`, else X), from `start` to `end` along the other axis.
fn assert_laps_run(tp: &Toolpath, along_x: bool, fixed: f64, (start, end): (f64, f64), case: &str) {
    let split = |q: Point| if along_x { (q.y, q.x) } else { (q.x, q.y) };
    let laps = laps(tp);
    assert_eq!(laps.len(), PASS_Z_MM.len(), "{case}: {laps:?}");
    for (lap, z) in laps.iter().zip(PASS_Z_MM) {
        assert_approx_eq!(lap.z_mm, z, TOL);
        for &q in &lap.points {
            assert!(
                (split(q).0 - fixed).abs() <= TOL,
                "{case}: {q:?} is off {fixed}"
            );
        }
        let first = lap.points[0];
        let last = lap.points[lap.points.len() - 1];
        assert!(
            (split(first).1 - start).abs() <= TOL,
            "{case}: starts at {first:?}"
        );
        assert!(
            (split(last).1 - end).abs() <= TOL,
            "{case}: ends at {last:?}"
        );
    }
}

/// Left puts the tool centre one radius to the left of the path,
/// looking from its first vertex to its last: in Y-up the normal
/// `(−dy, dx)`. The line from (20, 20) to (120, 20) is cut at Y 20 + r
/// over its whole length, the cut ending square on the normals through
/// its ends; drawn the other way, its left is −Y; drawn upward, −X.
/// Climb, so each pass runs from the first vertex.
#[test]
fn open_path_left_offsets_to_the_left() {
    let climb = CutDirection::Climb;
    let p = line_project();
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Left, climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_laps_run(&tp, true, 20.0 + R_MM, (20.0, 120.0), "left, drawn +X");

    let back = back_line_project();
    let (tp, _) = generate(&back, &profile_op(&back, Side::Left, climb));
    assert_laps_run(&tp, true, 20.0 - R_MM, (120.0, 20.0), "left, drawn −X");

    let up = project(
        vec![open(&[(0.0, 0.0), (0.0, 50.0)])],
        &[("i1", at_20_20())],
    );
    let (tp, _) = generate(&up, &profile_op(&up, Side::Left, climb));
    assert_laps_run(&tp, false, 20.0 - R_MM, (20.0, 70.0), "left, drawn +Y");
}

/// Right is the mirror of Left: the tool centre one radius to
/// the right of the path as drawn, `(dy, −dx)`. Conventional, so each
/// pass runs from the first vertex.
#[test]
fn open_path_right_offsets_to_the_right() {
    let conventional = CutDirection::Conventional;
    let p = line_project();
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Right, conventional));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_laps_run(&tp, true, 20.0 - R_MM, (20.0, 120.0), "right, drawn +X");

    let back = back_line_project();
    let (tp, _) = generate(&back, &profile_op(&back, Side::Right, conventional));
    assert_laps_run(&tp, true, 20.0 + R_MM, (120.0, 20.0), "right, drawn −X");

    let up = project(
        vec![open(&[(0.0, 0.0), (0.0, 50.0)])],
        &[("i1", at_20_20())],
    );
    let (tp, _) = generate(&up, &profile_op(&up, Side::Right, conventional));
    assert_laps_run(&tp, false, 20.0 + R_MM, (20.0, 70.0), "right, drawn +Y");
}

/// Left and Right are sides of an open path only. A closed
/// target is skipped with `side_needs_open_path` naming it, not
/// mapped to Inside or Outside by its winding, which the drawing does
/// not show; the open paths are cut. Inside and Outside still skip
/// open paths, and their warning now names Left and Right too.
#[test]
fn left_right_on_closed_target_is_skipped_with_warning() {
    let line = open(&[(0.0, 70.0), (100.0, 70.0)]);
    let p = project(vec![rect_100x50(), line], &[("i1", at_20_20())]);
    for side in [Side::Left, Side::Right] {
        let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::SideNeedsOpenPath],
            "{side:?}"
        );
        let Warning::SideNeedsOpenPath { shape } = &warnings[0] else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!(shape.profile, 0);
        let message = warnings[0].to_string();
        assert!(message.contains("closed"), "{message}");
        assert!(message.contains("profile 0"), "{message}");
        assert_eq!(laps(&tp).len(), PASS_Z_MM.len(), "{side:?}: only the line");
    }
    let rect = rect_project();
    let (tp, warnings) = generate(&rect, &profile_op(&rect, Side::Left, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::SideNeedsOpenPath]);
    assert!(tp.moves.is_empty(), "nothing is cut");

    let (_, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    let message = warnings[0].to_string();
    assert!(
        message.contains("Left") && message.contains("Right"),
        "{message}"
    );
}

/// The 100 mm line of [`line_project`] drawn the other way, from
/// (120, 20) to (20, 20).
fn back_line_project() -> Project {
    project(
        vec![open(&[(100.0, 0.0), (0.0, 0.0)])],
        &[("i1", at_20_20())],
    )
}

/// Checks that `p`'s line along Y 20, cut on `side` with `direction`,
/// runs every pass at `y` from X `start` to X `end`, plunging at
/// `start`, and that the line, with the part beyond it, lies on the
/// right of every cutting move when `part_on_right` and on its left
/// otherwise.
fn assert_side_cut(
    p: &Project,
    (side, direction): (Side, CutDirection),
    y: f64,
    (start, end): (f64, f64),
    part_on_right: bool,
) {
    let case = format!("{side:?} {direction:?}, {start} to {end}");
    let (tp, warnings) = generate(p, &profile_op(p, side, direction));
    assert!(warnings.is_empty(), "{case}: {warnings:?}");
    assert_laps_run(&tp, true, y, (start, end), &case);
    let cuts = xy_cuts(&tp);
    assert!(!cuts.is_empty(), "{case}");
    let toward = (end - start).signum();
    for (from, mv) in cuts {
        let (dx, dy) = (mv.to.x - from.x, mv.to.y - from.y);
        assert!(dx * toward > 0.0, "{case}: {mv:?}");
        // The cross product of travel `(dx, dy)` with the way to the
        // line, `(0, 20 − y)`, is negative when the line is on the
        // right of travel; the cut is level, so `dy` is zero.
        assert!(dy.abs() <= TOL, "{case}: {mv:?}");
        let cross = dx * (20.0 - from.y);
        assert_eq!(cross < 0.0, part_on_right, "{case}: {mv:?}");
    }
    let plunges = plunges_below_top(&tp);
    assert!(!plunges.is_empty(), "{case}");
    for (from, _) in plunges {
        assert!(
            from.xy().distance_to(Point::new(start, y)) <= TOL,
            "{case}: plunge at {from:?}"
        );
    }
}

/// On a Left cut only the bit's flank toward the line meets kept
/// material, so `direction` is the milling mode, as for closed shapes:
/// the spindle turns clockwise from above, so climb keeps the part on
/// the right of travel. The tool runs left of the line and the part is
/// beyond the line, so climb runs from the end beside the first vertex
/// to the end beside the last. The line from (20, 20) to (120, 20) is
/// cut at Y 20 + r moving +X, the part (below) on its right; drawn the
/// other way, at Y 20 − r moving −X, the part (above) on its right.
#[test]
fn left_climb_keeps_the_part_on_the_right_of_travel() {
    let climb = (Side::Left, CutDirection::Climb);
    assert_side_cut(&line_project(), climb, 20.0 + R_MM, (20.0, 120.0), true);
    let back = back_line_project();
    assert_side_cut(&back, climb, 20.0 - R_MM, (120.0, 20.0), true);
}

/// On a Right cut the part is on the line's left, so climb, the
/// part on the right of travel, runs from the end beside the last
/// vertex to the end beside the first: at Y 20 − r moving −X, the part
/// (above) on its right; drawn the other way, at Y 20 + r moving +X.
#[test]
fn right_climb_keeps_the_part_on_the_right_of_travel() {
    let climb = (Side::Right, CutDirection::Climb);
    assert_side_cut(&line_project(), climb, 20.0 - R_MM, (120.0, 20.0), true);
    let back = back_line_project();
    assert_side_cut(&back, climb, 20.0 + R_MM, (20.0, 120.0), true);
}

/// Conventional is the reverse of climb, the part on the left
/// of travel: Left conventional runs from the end beside the last
/// vertex, Right conventional from the end beside the first.
#[test]
fn left_conventional_is_the_reverse() {
    let p = line_project();
    let left = (Side::Left, CutDirection::Conventional);
    assert_side_cut(&p, left, 20.0 + R_MM, (120.0, 20.0), false);
    let back = back_line_project();
    assert_side_cut(&back, left, 20.0 - R_MM, (20.0, 120.0), false);
    let right = (Side::Right, CutDirection::Conventional);
    assert_side_cut(&p, right, 20.0 - R_MM, (20.0, 120.0), false);
    assert_side_cut(&back, right, 20.0 + R_MM, (120.0, 20.0), false);
}

/// VCarve allows ramps and tabs on open profile toolpaths, and
/// no leads. A Left or Right ramp runs back and forth along the offset
/// path, as On line's runs along the line (acceptance 26), within
/// three diameters of its start; tabs are measured along the offset
/// path from the end beside the first vertex as drawn, whichever end
/// the cut starts at. A lead warns `lead_dropped` and corner relief
/// `relief_skipped`, both for an open path, and a helix, which needs a
/// waste area, is refused.
#[test]
fn left_right_ramp_and_tabs() {
    let p = line_project();
    // The direction that starts beside the first vertex.
    for (side, y, forward) in [
        (Side::Left, 20.0 + R_MM, CutDirection::Climb),
        (Side::Right, 20.0 - R_MM, CutDirection::Conventional),
    ] {
        let op = sided_op(&p, side, forward, |op| {
            op.entry = Entry::Ramp { angle_deg: 5.0 };
        });
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{side:?}: {warnings:?}");
        let ramp: Vec<(Point3, Move)> = tp
            .moves
            .windows(2)
            .filter(|w| {
                !w[1].is_rapid()
                    && w[1].to.z < w[0].to.z - EPS_MM
                    && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM
            })
            .map(|w| (w[0].to, w[1]))
            .collect();
        assert!(!ramp.is_empty(), "{side:?}");
        let mut turned = false;
        for pair in ramp.windows(2) {
            turned |= (pair[0].1.to.x - pair[0].0.x) * (pair[1].1.to.x - pair[1].0.x) < 0.0;
        }
        assert!(turned, "{side:?}: back and forth: {ramp:?}");
        for (_, mv) in &ramp {
            assert_approx_eq!(mv.to.y, y, TOL);
            assert!(
                mv.to.x >= 20.0 - TOL && mv.to.x <= 20.0 + 3.0 * 6.35 + TOL,
                "{side:?}: {mv:?}"
            );
        }

        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let op = sided_op(&p, side, direction, |op| {
                op.tabs = Some(tab_at(30.0, TabShape::Rect));
            });
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{side:?} {direction:?}: {warnings:?}");
            let (held, length_mm) = held_at_tab_top(&tp);
            assert_approx_eq!(length_mm, 2.0 * TAB_SPAN_MM, 1e-6);
            for (a, b) in held {
                for q in [a, b] {
                    assert!(
                        (q.x - 50.0).abs() <= TAB_SPAN_MM / 2.0 + TOL && (q.y - y).abs() <= TOL,
                        "{side:?} {direction:?}: {q:?}"
                    );
                }
            }
        }

        let op = sided_op(&p, side, CutDirection::Climb, |op| {
            op.lead = Some(Lead {
                radius_mm: 4.0,
                sweep_deg: 90.0,
                lead_out: true,
                overcut_mm: 0.0,
            });
            op.corner_relief = CornerRelief::Dogbone;
        });
        let (_, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::LeadDropped, WarningCode::ReliefSkipped],
            "{side:?}"
        );
        assert!(matches!(
            warnings[0],
            Warning::LeadDropped {
                reason: crate::warning::LeadDroppedReason::OpenPath,
                ..
            }
        ));
        assert!(matches!(
            warnings[1],
            Warning::ReliefSkipped {
                reason: crate::warning::ReliefSkippedReason::OpenPath,
                ..
            }
        ));

        let op = sided_op(&p, side, CutDirection::Climb, |op| {
            op.entry = Entry::Helix {
                diameter_fraction: 0.8,
                angle_deg: 5.0,
            };
        });
        let err = generate_profile(&p, &op).unwrap_err();
        assert!(
            matches!(err, CamError::HelixNeedsWasteSide { .. }),
            "{side:?}: {err:?}"
        );
    }
}

/// The side a Left or Right cut runs on is waste, the other side
/// is the part. A cut that comes within the tool radius of another
/// line of the operation crosses into what that line keeps; one that
/// comes within the radius of the strip one radius wide that a line
/// keeps beside it (its kept edge) cuts into it too. Both are refused
/// with `CutsIntoKeptSide`, the shapes in drawing order: two lines
/// 4 mm apart, two lines 5 mm apart whose cut sides face each other,
/// a line drawn twice in opposite directions (each copy keeps what the
/// other cuts), and a square of four separate lines cut on its
/// inside, where each line's square end runs onto the next. The same
/// square cut on its outside, lines 20 mm apart and the line drawn
/// twice the same way (dropped as a duplicate) are cut.
#[test]
fn left_right_cut_into_the_kept_side_is_refused() {
    let square = |clockwise: bool| {
        let mut corners = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)];
        if clockwise {
            corners.reverse();
        }
        (0..4)
            .map(|k| open(&[corners[k], corners[(k + 1) % 4]]))
            .collect::<Vec<_>>()
    };
    let line = |from: (f64, f64), to: (f64, f64)| open(&[from, to]);
    let refused: Vec<(&str, Vec<Polyline>, Side)> = vec![
        (
            "4 mm apart",
            vec![
                line((0.0, 0.0), (100.0, 0.0)),
                line((0.0, 4.0), (100.0, 4.0)),
            ],
            Side::Left,
        ),
        (
            "facing cuts 5 mm apart",
            vec![
                line((0.0, 0.0), (100.0, 0.0)),
                line((100.0, 5.0), (0.0, 5.0)),
            ],
            Side::Left,
        ),
        (
            "drawn twice, opposite ways",
            vec![
                line((0.0, 0.0), (100.0, 0.0)),
                line((100.0, 0.0), (0.0, 0.0)),
            ],
            Side::Right,
        ),
        ("a square of lines, inside", square(false), Side::Left),
    ];
    for (case, profiles, side) in refused {
        let p = project(profiles, &[("i1", at_20_20())]);
        let err = generate_profile(&p, &profile_op(&p, side, CutDirection::Climb)).expect_err(case);
        let CamError::CutsIntoKeptSide {
            shape,
            other,
            diameter_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected CutsIntoKeptSide, got {err:?}");
        };
        assert!(
            shape.profile < other.profile,
            "{case}: drawing order {err:?}"
        );
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
    }
    let cut: Vec<(&str, Vec<Polyline>, Side, usize)> = vec![
        ("a square of lines, outside", square(false), Side::Right, 4),
        (
            "a square of lines drawn clockwise, outside",
            square(true),
            Side::Left,
            4,
        ),
        (
            "20 mm apart",
            vec![
                line((0.0, 0.0), (100.0, 0.0)),
                line((0.0, 20.0), (100.0, 20.0)),
            ],
            Side::Left,
            2,
        ),
    ];
    for (case, profiles, side, lines) in cut {
        let p = project(profiles, &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
        assert!(warnings.is_empty(), "{case}: {warnings:?}");
        assert_eq!(laps(&tp).len(), lines * PASS_Z_MM.len(), "{case}");
    }
    let twice = project(
        vec![
            line((0.0, 0.0), (100.0, 0.0)),
            line((0.0, 0.0), (100.0, 0.0)),
        ],
        &[("i1", at_20_20())],
    );
    let (tp, warnings) = generate(&twice, &profile_op(&twice, Side::Left, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
    assert_eq!(laps(&tp).len(), PASS_Z_MM.len());
}

/// An offset that vanishes leaves nothing to cut: a half circle
/// of radius 2 mm cut on the side its centre lies on, with a 6.35 mm
/// bit, warns `tool_too_large` and is not cut. Cut on its other side
/// it is.
#[test]
fn left_right_tool_too_large_for_a_tight_arc_warns() {
    let arc = Polyline::new(
        vec![Vertex::new(52.0, 25.0, 1.0), Vertex::line(48.0, 25.0)],
        false,
    );
    let p = project(vec![arc], &[("i1", at_20_20())]);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Left, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::ToolTooLarge]);
    assert!(tp.moves.is_empty());
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::Right, CutDirection::Climb));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(laps(&tp).len(), PASS_Z_MM.len());
}

// ── #210: an open path in a part, beside the part's own lines ──

/// An open polyline through `points`.
fn open(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        false,
    )
}

/// The 100 × 50 rect at (20, 20) with `extra` profiles drawn in it,
/// in the rect's own coordinates.
fn part_with(extra: Vec<Polyline>) -> Project {
    let mut profiles = vec![rect_100x50()];
    profiles.extend(extra);
    project(profiles, &[("i1", at_20_20())])
}

/// Generates an On line Profile of `p` and checks every line is cut
/// at every pass, with no finding.
fn every_line_cut(case: &str, p: &Project) {
    let lines = p
        .definitions
        .values()
        .map(|d| d.profiles.len())
        .sum::<usize>();
    let (tp, warnings) = generate(p, &profile_op(p, Side::OnLine, CutDirection::Climb));
    assert!(warnings.is_empty(), "{case}: {warnings:?}");
    assert_eq!(
        laps(&tp).len(),
        lines * PASS_Z_MM.len(),
        "{case}: every line at every pass"
    );
}

/// An open path that runs alongside a line of its own part,
/// closer than the 6.35 mm bit, cuts through the wall between them
/// together with that line's cut, as #199's narrow ring: refused with
/// `BandNarrowerThanTool`, the two shapes in drawing order and the band
/// the narrowest gap away from where the lines meet. Meeting the
/// outline excuses only the stretch near the meeting point (within
/// `√2·d` of it, so a slot meeting the outline at 45° or more is
/// cut): a slot from the bottom edge that runs 2 mm beside the left
/// edge, or turns to run 2 mm above the bottom edge, or leaves it at
/// 10°, still cuts through a wall.
#[test]
fn open_path_alongside_its_part_wall_is_refused() {
    let shallow_rad = 10_f64.to_radians();
    let reach_mm = std::f64::consts::SQRT_2 * 6.35;
    // The case, the profiles drawn in the part, the two profiles named
    // and the band.
    type Case = (&'static str, Vec<Polyline>, (usize, usize), f64);
    let cases: Vec<Case> = vec![
        (
            "1 mm inside the left edge",
            vec![open(&[(1.0, 10.0), (1.0, 40.0)])],
            (0, 1),
            1.0,
        ),
        (
            "from the bottom edge, 2 mm beside the left edge",
            vec![open(&[(2.0, 0.0), (2.0, 30.0)])],
            (0, 1),
            2.0,
        ),
        (
            "from the bottom edge, then along it",
            vec![open(&[(50.0, 0.0), (50.0, 2.0), (90.0, 2.0)])],
            (0, 1),
            2.0,
        ),
        (
            "from the bottom edge at 10°",
            vec![open(&[
                (50.0, 0.0),
                (50.0 + 40.0 * shallow_rad.cos(), 40.0 * shallow_rad.sin()),
            ])],
            (0, 1),
            reach_mm * shallow_rad.sin(),
        ),
        (
            "3 mm from a hole",
            vec![
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
                open(&[(63.0, 15.0), (63.0, 35.0)]),
            ],
            (1, 2),
            3.0,
        ),
        (
            "2 mm from another line",
            vec![
                open(&[(30.0, 10.0), (30.0, 40.0)]),
                open(&[(32.0, 10.0), (32.0, 40.0)]),
            ],
            (1, 2),
            2.0,
        ),
    ];
    for (case, extra, pair, band_mm) in cases {
        let p = part_with(extra);
        let err = generate_profile(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb))
            .expect_err(case);
        let CamError::BandNarrowerThanTool {
            shape,
            other,
            band_mm: band,
            diameter_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected BandNarrowerThanTool, got {err:?}");
        };
        assert_eq!((shape.profile, other.profile), pair, "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        assert!((band - band_mm).abs() <= TOL, "{case}: band {band}");
    }
}

/// An open path that meets a line of its part, a kerf slot from
/// the edge or a saw cut across the part, opens into that line's cut
/// as the user drew it: cut, with no finding. So are a slot meeting
/// the edge at 60°, two slots joined at a corner, two lines crossing,
/// and a slot from a hole's edge.
#[test]
fn open_slot_from_the_edge_is_cut() {
    let steep_rad = 60_f64.to_radians();
    let cases: Vec<(&str, Vec<Polyline>)> = vec![
        (
            "square to the edge",
            vec![open(&[(50.0, 0.0), (50.0, 20.0)])],
        ),
        (
            "a saw cut edge to edge",
            vec![open(&[(50.0, 0.0), (50.0, 50.0)])],
        ),
        (
            "at 60° to the edge",
            vec![open(&[
                (50.0, 0.0),
                (50.0 + 20.0 * steep_rad.cos(), 20.0 * steep_rad.sin()),
            ])],
        ),
        (
            "two slots joined at a corner",
            vec![
                open(&[(50.0, 0.0), (50.0, 20.0)]),
                open(&[(50.0, 20.0), (70.0, 20.0)]),
            ],
        ),
        (
            "two lines crossing",
            vec![
                open(&[(30.0, 25.0), (70.0, 25.0)]),
                open(&[(50.0, 10.0), (50.0, 40.0)]),
            ],
        ),
        (
            "from a hole's edge",
            vec![
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
                open(&[(60.0, 25.0), (80.0, 25.0)]),
            ],
        ),
    ];
    for (case, extra) in cases {
        every_line_cut(case, &part_with(extra));
    }
}

/// An open path at least the bit's diameter from every other
/// line of its part is cut; a band exactly `d` wide is cut, as #199's
/// ring is.
#[test]
fn open_path_far_from_walls_is_cut() {
    let cases: Vec<(&str, Vec<Polyline>)> = vec![
        ("in the middle", vec![open(&[(20.0, 25.0), (80.0, 25.0)])]),
        (
            "a tool width inside the left edge",
            vec![open(&[(6.35, 10.0), (6.35, 40.0)])],
        ),
        (
            "a tool width from a hole and from another line",
            vec![
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
                open(&[(66.35, 15.0), (66.35, 35.0)]),
                open(&[(72.7, 15.0), (72.7, 35.0)]),
            ],
        ),
    ];
    for (case, extra) in cases {
        every_line_cut(case, &part_with(extra));
    }
}

// ── #221: a line closer than the bit to itself ──

/// Generates an On line Profile of `p`, expects `BandNarrowerThanTool`
/// naming profile `profile` of `i1` as both shapes, and returns the
/// band.
fn refused_self_band(case: &str, p: &Project, profile: usize) -> f64 {
    let err =
        generate_profile(p, &profile_op(p, Side::OnLine, CutDirection::Climb)).expect_err(case);
    let CamError::BandNarrowerThanTool {
        shape,
        other,
        band_mm,
        diameter_mm,
        ..
    } = &err
    else {
        panic!("{case}: expected BandNarrowerThanTool, got {err:?}");
    };
    assert_eq!(shape, &**other, "{case}: the shape is named twice");
    assert_eq!(shape.profile, profile, "{case}");
    assert_approx_eq!(*diameter_mm, 6.35, TOL);
    *band_mm
}

/// The point `length_mm` from `from` at `angle_deg` from +X.
fn towards(from: (f64, f64), angle_deg: f64, length_mm: f64) -> (f64, f64) {
    let a = angle_deg.to_radians();
    (from.0 + length_mm * a.cos(), from.1 + length_mm * a.sin())
}

/// An open V with its tip at `tip`, pointing up, its two 30 mm arms
/// `angle_deg` apart.
fn v_turn(tip: (f64, f64), angle_deg: f64) -> Polyline {
    let half = angle_deg / 2.0;
    open(&[
        towards(tip, -90.0 - half, 30.0),
        tip,
        towards(tip, -90.0 + half, 30.0),
    ])
}

/// Two stretches of one line, more than `√2·d` apart along it,
/// that come closer than the 6.35 mm bit cut through the part between
/// them, as two lines do: refused with
/// `BandNarrowerThanTool` naming the shape twice. A U with square
/// corners and legs 2 mm apart, one with a round end and legs 3 mm
/// apart, and a closed outline with a 3 mm neck (a dumbbell). A sharp
/// corner is where two lines meet: its arms are set aside
/// within `√2·d` of the tip, so a 10° spike is refused with the band
/// `√2·d·sin 10°`, as a slot meeting the edge at 10° is.
#[test]
fn on_line_hairpin_narrower_than_tool_is_refused() {
    let reach_mm = std::f64::consts::SQRT_2 * 6.35;
    let round_end = Polyline::new(
        vec![
            Vertex::line(50.0, 10.0),
            Vertex::new(50.0, 38.0, -1.0),
            Vertex::line(53.0, 38.0),
            Vertex::line(53.0, 10.0),
        ],
        false,
    );
    let cases: Vec<(&str, Polyline, f64)> = vec![
        (
            "a U with legs 2 mm apart",
            open(&[(30.0, 10.0), (30.0, 40.0), (32.0, 40.0), (32.0, 10.0)]),
            2.0,
        ),
        ("a U with a round end, legs 3 mm apart", round_end, 3.0),
        (
            "a 10° spike",
            v_turn((80.0, 40.0), 10.0),
            reach_mm * 10_f64.to_radians().sin(),
        ),
    ];
    for (case, line, band_mm) in cases {
        let band = refused_self_band(case, &part_with(vec![line]), 1);
        assert!((band - band_mm).abs() <= TOL, "{case}: band {band}");
    }
    let dumbbell = Polyline::new(
        [
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 13.5),
            (50.0, 13.5),
            (50.0, 0.0),
            (80.0, 0.0),
            (80.0, 30.0),
            (50.0, 30.0),
            (50.0, 16.5),
            (30.0, 16.5),
            (30.0, 30.0),
            (0.0, 30.0),
        ]
        .into_iter()
        .map(|(x, y)| Vertex::line(x, y))
        .collect(),
        true,
    );
    let p = project(vec![dumbbell], &[("i1", at_20_20())]);
    let band = refused_self_band("a dumbbell", &p, 0);
    assert_approx_eq!(band, 3.0, TOL);
}

/// An open spiral in a part whose turns are 4 mm apart, closer
/// than the 6.35 mm bit, cuts through the part between its turns:
/// refused naming the spiral twice, drawn as fine chords or as half
/// circles about two centres.
#[test]
fn on_line_spiral_with_tight_turns_is_refused() {
    // r = 4 + 4·θ/2π about (50, 25), three turns in 5° chords.
    let chords: Vec<(f64, f64)> = (0..=216)
        .map(|k| {
            let theta = f64::from(k) * 5_f64.to_radians();
            let r = 4.0 + 4.0 * theta / TAU;
            (50.0 + r * theta.cos(), 25.0 + r * theta.sin())
        })
        .collect();
    let band = refused_self_band("fine chords", &part_with(vec![open(&chords)]), 1);
    assert!(band > 3.5 && band < 4.0, "fine chords: band {band}");
    // Half turns about (50, 25) above the axis and (52, 25) below it,
    // each 2 mm wider: turns 4 mm apart.
    let halves = Polyline::new(
        [52.0, 48.0, 56.0, 44.0, 60.0, 40.0, 64.0, 36.0]
            .into_iter()
            .map(|x| Vertex::new(x, 25.0, 1.0))
            .collect(),
        false,
    );
    let band = refused_self_band("half circles", &part_with(vec![halves]), 1);
    assert_approx_eq!(band, 4.0, TOL);
}

/// Neighbouring stretches of one line are not compared with each
/// other, only stretches more than `√2·d` apart along it, so a smooth
/// curve, a corner of 90° or more, and a round hole are cut as drawn.
/// A sharp corner sets its arms aside within `√2·d` of the tip, as
/// where two lines meet, so a V of 60°, or of exactly 45°, is
/// cut too.
#[test]
fn on_line_smooth_curve_is_not_compared_with_itself() {
    let wave: Vec<(f64, f64)> = (10..=90)
        .map(|x| {
            let x = f64::from(x);
            (x, 25.0 + 8.0 * (x / 10.0).sin())
        })
        .collect();
    let half_circle: Vec<(f64, f64)> = (0..=90)
        .map(|k| {
            let a = f64::from(180 - 2 * k).to_radians();
            (50.0 + 15.0 * a.cos(), 20.0 + 15.0 * a.sin())
        })
        .collect();
    let cases: Vec<(&str, Vec<Polyline>)> = vec![
        ("a wave of fine chords", vec![open(&wave)]),
        ("a half circle of fine chords", vec![open(&half_circle)]),
        (
            "an L",
            vec![open(&[(30.0, 10.0), (30.0, 40.0), (60.0, 40.0)])],
        ),
        (
            "a square corner turned back",
            vec![open(&[
                (30.0, 10.0),
                (30.0, 40.0),
                (60.0, 40.0),
                (60.0, 10.0),
            ])],
        ),
        (
            "a round hole",
            vec![Polyline::circle(Point::new(50.0, 25.0), 10.0)],
        ),
        ("a V of 60°", vec![v_turn((50.0, 40.0), 60.0)]),
        ("a V of 45°", vec![v_turn((50.0, 40.0), 45.0)]),
    ];
    for (case, extra) in cases {
        every_line_cut(case, &part_with(extra));
    }
}

/// Only the part between two stretches of one line is kept. A
/// 3 mm notch cut into an outline, and a hole with a 3 mm neck, have
/// waste between their stretches, which the cut may clear: both are
/// cut as drawn.
#[test]
fn on_line_narrow_notch_in_an_outline_is_cut() {
    let closed = |points: &[(f64, f64)]| {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            true,
        )
    };
    let notched = closed(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (51.5, 50.0),
        (51.5, 25.0),
        (48.5, 25.0),
        (48.5, 50.0),
        (0.0, 50.0),
    ]);
    every_line_cut("a notch", &project(vec![notched], &[("i1", at_20_20())]));
    let necked_hole = closed(&[
        (20.0, 15.0),
        (40.0, 15.0),
        (40.0, 23.5),
        (60.0, 23.5),
        (60.0, 15.0),
        (80.0, 15.0),
        (80.0, 35.0),
        (60.0, 35.0),
        (60.0, 26.5),
        (40.0, 26.5),
        (40.0, 35.0),
        (20.0, 35.0),
    ]);
    every_line_cut("a necked hole", &part_with(vec![necked_hole]));
}

/// An open path drawn twice, the same vertices in either
/// direction, is cut once and the copies are dropped with
/// `duplicate_shape_dropped`, as a closed shape's are, before the band
/// check (which would see a band of 0 between them).
#[test]
fn duplicate_open_paths_are_dropped_with_a_warning() {
    let line = open(&[(30.0, 10.0), (30.0, 40.0), (60.0, 40.0)]);
    let p = part_with(vec![line.clone(), line.clone(), line.reversed()]);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
    assert_eq!(
        codes(&warnings),
        vec![
            WarningCode::DuplicateShapeDropped,
            WarningCode::DuplicateShapeDropped
        ]
    );
    for (warning, profile) in warnings.iter().zip([2, 3]) {
        let Warning::DuplicateShapeDropped {
            shape,
            duplicate_of,
        } = warning
        else {
            panic!("{warning:?}");
        };
        assert_eq!(shape.profile, profile);
        assert_eq!(
            (duplicate_of.instance.as_str(), duplicate_of.profile),
            ("i1", 1)
        );
    }
    assert_eq!(
        laps(&tp).len(),
        2 * PASS_Z_MM.len(),
        "the outline and one copy of the line"
    );
    // Outside a part too: two copies in the waste are cut once, with
    // no `toolpaths_overlap` between them.
    let waste = open(&[(150.0, 10.0), (150.0, 40.0)]);
    let p = part_with(vec![waste.clone(), waste]);
    let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
    assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
    assert_eq!(laps(&tp).len(), 2 * PASS_Z_MM.len());
}

// ── #223: a part the bit removes whole ──

/// A part of one closed outline, `outline`, placed at (20, 20).
fn part_of(outline: Polyline) -> Project {
    project(vec![outline], &[("i1", at_20_20())])
}

/// A part whose outline cut On line lies everywhere within the
/// tool radius of the line (the part inset by `r` is empty) is removed
/// whole by the cut: refused with `PartNarrowerThanTool` naming the
/// outline. That holds for a part too short round for the self-band
/// check to compare any two of its stretches (a 2.5 mm radius circle,
/// a 3 mm square, under `2·√2·d` round), and for longer ones it would
/// otherwise refuse as a band (a 2.9 mm radius circle, a 3 × 50 mm
/// strip), so the answer no longer depends on the perimeter.
#[test]
fn on_line_part_smaller_than_the_bit_is_refused() {
    let cases = [
        (
            "a 2.5 mm radius circle",
            Polyline::circle(Point::new(10.0, 10.0), 2.5),
        ),
        (
            "a 2.9 mm radius circle",
            Polyline::circle(Point::new(10.0, 10.0), 2.9),
        ),
        (
            "a 3 mm square",
            Polyline::rect(Point::new(10.0, 10.0), 3.0, 3.0),
        ),
        (
            "a 3 × 50 mm strip",
            Polyline::rect(Point::new(10.0, 10.0), 50.0, 3.0),
        ),
        (
            "a 6 mm circle drawn the other way",
            Polyline::circle(Point::new(10.0, 10.0), 3.0).reversed(),
        ),
    ];
    for (case, outline) in cases {
        let p = part_of(outline);
        let err = generate_profile(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb))
            .expect_err(case);
        let CamError::PartNarrowerThanTool {
            op,
            shape,
            diameter_mm,
        } = &err
        else {
            panic!("{case}: expected PartNarrowerThanTool, got {err:?}");
        };
        assert_eq!(op.as_str(), "o1", "{case}");
        assert_eq!(shape.instance.as_str(), "i1", "{case}");
        assert_eq!(shape.profile, 0, "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        let text = err.to_string();
        assert!(text.contains("whole part"), "{case}: {text}");
    }
    // A small hole's inside is waste, and a small island in a hole is a
    // part of its own.
    let island = project(
        vec![
            rect_100x50(),
            Polyline::circle(Point::new(50.0, 25.0), 15.0),
            Polyline::circle(Point::new(50.0, 25.0), 2.5),
        ],
        &[("i1", at_20_20())],
    );
    let err = generate_profile(
        &island,
        &profile_op(&island, Side::OnLine, CutDirection::Climb),
    )
    .expect_err("the island");
    assert!(
        matches!(&err, CamError::PartNarrowerThanTool { shape, .. } if shape.profile == 2),
        "{err:?}"
    );
}

/// A part a little wider than the bit everywhere (a 3.3 mm
/// radius circle, a 7 mm square) keeps material inside the cut and is
/// cut as drawn, as is a 2.5 mm radius hole.
#[test]
fn on_line_part_just_larger_than_the_bit_is_cut() {
    let cases = [
        (
            "a 3.3 mm radius circle",
            Polyline::circle(Point::new(10.0, 10.0), 3.3),
        ),
        (
            "a 7 mm square",
            Polyline::rect(Point::new(10.0, 10.0), 7.0, 7.0),
        ),
    ];
    for (case, outline) in cases {
        every_line_cut(case, &part_of(outline));
    }
    every_line_cut(
        "a 2.5 mm radius hole",
        &part_with(vec![Polyline::circle(Point::new(50.0, 25.0), 2.5)]),
    );
}

// ── #284: the last pass stock to leave ──

const STOCK_TO_LEAVE_MM: f64 = 0.5;
/// Acceptance 46: 12 mm stock cut through in 4 mm passes.
const LAST_STOCK_MM: f64 = 12.0;
const LAST_PASS_Z_MM: [f64; 3] = [8.0, 4.0, 0.0];

fn rect_60x40() -> Polyline {
    Polyline::rect(Point::new(0.0, 0.0), 60.0, 40.0)
}

/// Acceptance 46's part, `profiles` (the 60 × 40 mm rectangle first)
/// placed at (20, 20) on 12 mm stock.
fn last_pass_project_of(profiles: Vec<Polyline>) -> Project {
    let mut p = project(profiles, &[("i1", at_20_20())]);
    p.stock.thickness_mm = Some(LAST_STOCK_MM);
    for instance in &mut p.instances {
        instance.z_top_mm = Some(LAST_STOCK_MM);
    }
    p
}

fn last_pass_project() -> Project {
    last_pass_project_of(vec![rect_60x40()])
}

fn stock_to_leave(stock_to_leave_mm: f64) -> LastPass {
    LastPass {
        stock_to_leave_mm,
        thickness_mm: None,
        reverse: false,
    }
}

/// Acceptance 46's Profile: 12 mm deep in 4 mm passes, climb, with a
/// 0.5 mm last pass stock to leave.
fn last_pass_op(p: &Project, side: Side) -> ProfileOp {
    ProfileOp {
        depth_mm: Some(LAST_STOCK_MM),
        step_down_mm: Some(4.0),
        last_pass: Some(stock_to_leave(STOCK_TO_LEAVE_MM)),
        ..profile_op(p, side, CutDirection::Climb)
    }
}

/// The placed outline of the 60 × 40 mm rectangle.
fn placed_rect() -> Polyline {
    at_20_20().apply_polyline(&rect_60x40()).unwrap()
}

/// Distance from `p` to `line`.
fn gap_mm(line: &Polyline, p: Point) -> f64 {
    nearest_point(line, p).unwrap().gap_mm
}

/// The middle of `mv`, from `from`, in XY (arcs along the arc).
fn middle(from: Point, mv: &Move) -> Point {
    let arc = match mv.kind {
        MoveKind::ArcCcw { centre } => Some((centre, true)),
        MoveKind::ArcCw { centre } => Some((centre, false)),
        _ => None,
    };
    match arc {
        Some((centre, ccw)) => {
            let sweep = sweep_rad(from, mv.to.xy(), centre, ccw);
            let radius = from.distance_to(centre);
            let angle = (from.y - centre.y).atan2(from.x - centre.x) + sweep / 2.0;
            Point::new(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
            )
        }
        None => from + (mv.to.xy() - from) * 0.5,
    }
}

/// The nearest and farthest any cutting move of `moves` (after the
/// move at `from`) comes to `line`, sampled at each move's middle and
/// end. `None` when no move cuts.
fn gap_range_mm(line: &Polyline, from: Point3, moves: &[Move]) -> Option<(f64, f64)> {
    let mut at = from;
    let mut range: Option<(f64, f64)> = None;
    for mv in moves {
        if !mv.is_rapid() {
            for p in [middle(at.xy(), mv), mv.to.xy()] {
                let gap = gap_mm(line, p);
                range = Some(range.map_or((gap, gap), |(lo, hi)| (lo.min(gap), hi.max(gap))));
            }
        }
        at = mv.to;
    }
    range
}

/// Where the last pass leaves its stock path in a one-shape,
/// full-depth-first toolpath: the first move, after the first
/// one below `roughing_floor_mm` (the deepest pass before the last),
/// that ends nearer `outline` than the stock path, `r +
/// stock_to_leave_mm`. The last pass goes down on that path, as one more
/// earlier pass would, so the moves before this one keep `r + a`; this
/// is its step across the stock to leave onto its own path, or its lead-in
/// arc. (Changed in #339: the rise to safe Z before the last pass,
/// which no longer retracts.)
fn last_pass_start(
    tp: &Toolpath,
    outline: &Polyline,
    roughing_floor_mm: f64,
    stock_to_leave_mm: f64,
) -> usize {
    let below = tp
        .moves
        .iter()
        .position(|m| m.to.z < roughing_floor_mm - EPS_MM)
        .expect("the last pass goes deeper");
    (below..tp.moves.len())
        .find(|&k| {
            let m = &tp.moves[k];
            !m.is_rapid() && gap_mm(outline, m.to.xy()) < R_MM + stock_to_leave_mm - 1e-3
        })
        .expect("the last pass reaches its path")
}

/// Acceptance 46: the first two passes run 0.5 mm farther from the
/// part than the third, which is the path the Profile cuts with no
/// last pass, move for move; every cut of the first two keeps
/// `r + 0.5` from the part, and the third cuts the wall at `r`, at
/// full depth, so it takes an even 0.5 mm skin off the whole wall.
#[test]
fn last_pass_stock_to_leave_offsets_earlier_passes() {
    let p = last_pass_project();
    let op = last_pass_op(&p, Side::Outside);
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (plain, _) = generate(
        &p,
        &ProfileOp {
            last_pass: None,
            ..op.clone()
        },
    );
    let laps = last_pass_laps(&tp);
    let plain_laps = laps_at(&plain, 0.0);
    assert_eq!(laps.len(), 3);
    for (lap, z) in laps.iter().zip(LAST_PASS_Z_MM) {
        assert_approx_eq!(lap.z_mm, z, TOL);
    }
    let grown = 2.0 * (R_MM + STOCK_TO_LEAVE_MM);
    for lap in &laps[..2] {
        assert_approx_eq!(lap.x_extent_mm(), 60.0 + grown, TOL);
        assert_approx_eq!(lap.y_extent_mm(), 40.0 + grown, TOL);
    }
    assert_approx_eq!(laps[2].x_extent_mm(), 66.35, TOL);
    assert_approx_eq!(laps[2].y_extent_mm(), 46.35, TOL);
    assert_eq!(laps[2].moves, plain_laps[0].moves, "the phase 1 path");

    let outline = placed_rect();
    let split = last_pass_start(&tp, &outline, LAST_PASS_Z_MM[1], STOCK_TO_LEAVE_MM);
    let (near, far) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split]).unwrap();
    assert_approx_eq!(near, R_MM + STOCK_TO_LEAVE_MM, TOL);
    assert_approx_eq!(far, R_MM + STOCK_TO_LEAVE_MM, TOL);
    let (near, far) = gap_range_mm(&outline, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
    assert_approx_eq!(near, R_MM, TOL);
    assert_approx_eq!(far, R_MM, TOL);
    // The last pass plunges where the pass before ended, on the
    // stock path, one step-down to the full depth, then steps the
    // stock to leave across onto its own path at the feed.
    let step = tp.moves[split];
    let plunge = tp.moves[split - 1];
    let before = tp.moves[split - 2].to;
    assert_eq!(plunge.feed_mm_min, Some(300.0));
    assert!(before.xy().distance_to(plunge.to.xy()) <= TOL);
    assert_approx_eq!(before.z, LAST_PASS_Z_MM[1], TOL);
    assert_approx_eq!(plunge.to.z, 0.0, TOL);
    assert_eq!(step.kind, MoveKind::Linear);
    assert_eq!(step.feed_mm_min, Some(1000.0));
    assert_approx_eq!(step.to.z, 0.0, TOL);
    assert_approx_eq!(
        plunge.to.xy().distance_to(step.to.xy()),
        STOCK_TO_LEAVE_MM,
        1e-9
    );
}

/// [`laps`], without the step across the stock to leave a last pass's lap
/// starts with: a lap that starts with a straight move no
/// longer than a third of the bit and does not end where it starts.
/// (The tests' open paths start with a longer move.)
fn last_pass_laps(tp: &Toolpath) -> Vec<Lap> {
    laps(tp)
        .into_iter()
        .map(|mut lap| {
            // Closed, the lap ends where the step ends; open (Left,
            // Right) it never closes, and its first real move is long.
            let last = lap.points[lap.points.len() - 1];
            let stepped = lap.moves.len() > 1
                && lap.moves[0].kind == MoveKind::Linear
                && lap.points[0].distance_to(lap.points[1]) <= 6.35 / 3.0 + EPS_MM
                && lap.points[0].distance_to(last) > EPS_MM;
            if stepped {
                let (a, b) = (lap.points[0], lap.points[1]);
                lap.area_mm2 -= (a.x * b.y - b.x * a.y) / 2.0;
                lap.points.remove(0);
                lap.moves.remove(0);
            }
            lap
        })
        .collect()
}

/// Acceptance 46: `thickness_mm: 1` cuts 4, 8 and 11 mm deep on the
/// stock path and only the 12 mm pass on the part's path.
#[test]
fn last_pass_thickness_adds_final_depth() {
    let p = last_pass_project();
    let op = ProfileOp {
        last_pass: Some(LastPass {
            thickness_mm: Some(1.0),
            ..stock_to_leave(STOCK_TO_LEAVE_MM)
        }),
        ..last_pass_op(&p, Side::Outside)
    };
    let (tp, _) = generate(&p, &op);
    let laps = last_pass_laps(&tp);
    let zs: Vec<f64> = laps.iter().map(|l| l.z_mm).collect();
    assert_eq!(laps.len(), 4, "{zs:?}");
    for (z, expected) in zs.iter().zip([8.0, 4.0, 1.0, 0.0]) {
        assert_approx_eq!(*z, expected, TOL);
    }
    for lap in &laps[..3] {
        assert_approx_eq!(lap.x_extent_mm(), 67.35, TOL);
    }
    assert_approx_eq!(laps[3].x_extent_mm(), 66.35, TOL);

    // As thick as a step-down: the usual depths.
    let op = ProfileOp {
        last_pass: Some(LastPass {
            thickness_mm: Some(4.0),
            ..stock_to_leave(STOCK_TO_LEAVE_MM)
        }),
        ..last_pass_op(&p, Side::Outside)
    };
    let usual: Vec<f64> = last_pass_laps(&generate(&p, &op).0)
        .iter()
        .map(|l| l.z_mm)
        .collect();
    assert_eq!(usual.len(), 3, "{usual:?}");
    for (z, expected) in usual.iter().zip(LAST_PASS_Z_MM) {
        assert_approx_eq!(*z, expected, TOL);
    }

    // The coordinator's rule: the last pass is never
    // thicker than the step-down, so 10 mm, or all 12 mm, of a cut in
    // 4 mm steps is refused, not cut in one go.
    for thickness_mm in [10.0, LAST_STOCK_MM] {
        let op = ProfileOp {
            last_pass: Some(LastPass {
                thickness_mm: Some(thickness_mm),
                ..stock_to_leave(STOCK_TO_LEAVE_MM)
            }),
            ..last_pass_op(&p, Side::Outside)
        };
        let err = generate_profile(&p, &op).expect_err("thicker than the step-down");
        assert!(
            matches!(&err, CamError::InvalidLastPass { field: "thickness_mm", value, .. }
                if (*value - thickness_mm).abs() <= TOL),
            "{err:?}"
        );
        assert!(err.to_string().contains("4 mm step-down"), "{err}");
    }
}

/// Acceptance 46: with `reverse` the last pass runs the other way
/// round the part (conventional where the others climb); the earlier
/// passes are unchanged.
#[test]
fn last_pass_reverse_flips_direction() {
    let p = last_pass_project();
    let forward = last_pass_op(&p, Side::Outside);
    let reverse = ProfileOp {
        last_pass: Some(LastPass {
            reverse: true,
            ..stock_to_leave(STOCK_TO_LEAVE_MM)
        }),
        ..forward.clone()
    };
    let forward = last_pass_laps(&generate(&p, &forward).0);
    let reverse = last_pass_laps(&generate(&p, &reverse).0);
    assert_eq!(reverse.len(), 3);
    for (a, b) in forward.iter().zip(&reverse).take(2) {
        assert_eq!(a.moves, b.moves, "the stock passes are unchanged");
        assert!(b.area_mm2 < 0.0, "climb outside is clockwise");
    }
    assert!(
        reverse[2].area_mm2 > 0.0,
        "the last pass runs counter-clockwise"
    );
    assert_approx_eq!(reverse[2].area_mm2, -forward[2].area_mm2, 1e-6);
    assert_approx_eq!(reverse[2].x_extent_mm(), 66.35, TOL);
    // It starts where the forward last pass starts.
    assert_eq!(reverse[2].points[0], forward[2].points[0]);
}

/// The `[from, to]` X ranges of the level cuts at `top_mm` on the
/// horizontal line `y_mm`, in toolpath order.
fn held_on_line(tp: &Toolpath, y_mm: f64, top_mm: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in tp.moves.windows(2) {
        let (from, mv) = (w[0].to, w[1]);
        let on_line = (from.y - y_mm).abs() <= 1e-6 && (mv.to.y - y_mm).abs() <= 1e-6;
        let held = (from.z - top_mm).abs() <= EPS_MM && (mv.to.z - top_mm).abs() <= EPS_MM;
        if mv.is_rapid() || !on_line || !held || (from.x - mv.to.x).abs() <= EPS_MM {
            continue;
        }
        let (a, b) = (from.x.min(mv.to.x), from.x.max(mv.to.x));
        match out.last_mut() {
            Some(last) if (a - last.1).abs() <= 1e-6 || (b - last.0).abs() <= 1e-6 => {
                *last = (last.0.min(a), last.1.max(b));
            }
            _ => out.push((a, b)),
        }
    }
    out
}

/// #284 (design A8, step 3): tabs are laid on the part's path, and on
/// the stock path each raised stretch is the part path's,
/// projected and widened by the stock to leave each way, so every pass
/// below the tab top keeps off the tab the last pass leaves. Placed
/// tabs and a request, rect and smooth, with and without
/// `thickness_mm`: on the bottom edge each stock pass below the
/// 5 mm tab top holds the tab top over the last pass's held stretch
/// and as far beyond it as its cutter could reach the tab (smooth:
/// over at least the same stretch), and the last pass rises over the
/// tab too. (changed in the #310 review: the stretch is found from the
/// tab's material, not widened by `a`;
/// [`last_pass_tabs_keep_off_the_tab_material`] checks it anywhere.)
#[test]
fn last_pass_tabs_cover_same_stretch() {
    const TOP_MM: f64 = 5.0;
    let final_y = 20.0 - R_MM;
    let stock_to_leave_y = final_y - STOCK_TO_LEAVE_MM;
    let p = last_pass_project();
    // The request's distance along the part's path to the middle of
    // the bottom edge, measured from the path's start.
    let path = {
        let centre = offset(&Contour::new(placed_rect()).unwrap(), R_MM).remove(0);
        if centre.is_ccw() {
            centre.reversed().into_polyline()
        } else {
            centre.into_polyline()
        }
    };
    let bottom_mm = nearest_point(&path, Point::new(50.0, final_y))
        .unwrap()
        .along_mm;
    for shape in [TabShape::Rect, TabShape::Smooth] {
        for thickness_mm in [None, Some(1.0)] {
            let requested = ProfileOp {
                tabs: Some(TabSpec {
                    placement: TabPlacement::AtDistances {
                        distances_mm: vec![bottom_mm],
                    },
                    width_mm: 8.0,
                    height_mm: TOP_MM,
                    shape,
                    on_holes: false,
                    avoid_corners: false,
                }),
                last_pass: Some(LastPass {
                    thickness_mm,
                    ..stock_to_leave(STOCK_TO_LEAVE_MM)
                }),
                ..last_pass_op(&p, Side::Outside)
            };
            let mut placed = p.clone();
            placed.tabs = vec![dc_project::PlacedTab {
                id: dc_project::TabId::new("tab1"),
                instance: InstanceId::new("i1"),
                profile: 0,
                x_mm: 30.0,
                y_mm: 0.0,
                width_mm: 8.0,
                height_mm: TOP_MM,
                shape,
                holds_slug: Some(false),
            }];
            let placed_op = ProfileOp {
                tabs: None,
                ..requested.clone()
            };
            for (case, project, op) in [
                ("requested", &p, &requested),
                ("placed", &placed, &placed_op),
            ] {
                let case = format!("{case} {shape:?} {thickness_mm:?}");
                let (tp, _) = generate(project, op);
                let last = held_on_line(&tp, final_y, TOP_MM);
                assert_eq!(last.len(), 1, "{case}: the last pass rises: {last:?}");
                let (fx1, fx2) = last[0];
                let expected_mm = match shape {
                    TabShape::Rect => 8.0 + 6.35,
                    TabShape::Smooth => 6.35,
                };
                assert_approx_eq!(fx2 - fx1, expected_mm, 1e-6);
                assert_approx_eq!((fx1 + fx2) / 2.0, 50.0, 1e-6);
                let rough = held_on_line(&tp, stock_to_leave_y, TOP_MM);
                // Passes at Z 4 (and 1) are below the tab top; Z 8 is not.
                let below = if thickness_mm.is_some() { 2 } else { 1 };
                assert_eq!(rough.len(), below, "{case}: {rough:?}");
                for (ax1, ax2) in rough {
                    match shape {
                        // The stock cutter reaches the tab's
                        // corner where it meets the waste
                        // √(2ar − a²) = 1.71 mm before the stretch,
                        // so it rises that soon, less the sliver of
                        // the corner thinner than a sampling step
                        // (0.26 mm), and not much sooner.
                        TabShape::Rect => {
                            let reach_mm = (2.0 * STOCK_TO_LEAVE_MM * R_MM
                                - STOCK_TO_LEAVE_MM * STOCK_TO_LEAVE_MM)
                                .sqrt();
                            for (rise_mm, end) in [(fx1 - ax1, "before"), (ax2 - fx2, "after")] {
                                assert!(
                                    rise_mm >= reach_mm - 0.26 && rise_mm <= reach_mm + 1.0,
                                    "{case}: raised {rise_mm} mm {end} the stretch"
                                );
                            }
                        }
                        TabShape::Smooth => {
                            assert!(ax1 <= fx1 + 1e-6 && ax2 >= fx2 - 1e-6, "{case}");
                        }
                    }
                }
            }
        }
    }
}

/// A 5-point star of `outer_mm` and `inner_mm` radii centred at
/// (40, 40), counter-clockwise from a tip on +X.
fn star(outer_mm: f64, inner_mm: f64) -> Polyline {
    let points: Vec<Vertex> = (0..10)
        .map(|k| {
            let radius = if k % 2 == 0 { outer_mm } else { inner_mm };
            let angle = f64::from(k) * PI / 5.0;
            Vertex::line(40.0 + radius * angle.cos(), 40.0 + radius * angle.sin())
        })
        .collect();
    Polyline::new(points, true)
}

/// A closed outline through `points`.
fn closed(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

/// An equilateral triangle of 60 mm sides: three 60° corners.
fn triangle_60() -> Polyline {
    closed(&[(0.0, 0.0), (60.0, 0.0), (30.0, 30.0 * 3f64.sqrt())])
}

/// A right isosceles triangle of 60 mm legs: two 45° corners.
fn triangle_45() -> Polyline {
    closed(&[(0.0, 0.0), (60.0, 0.0), (0.0, 60.0)])
}

/// The 60 × 40 mm rectangle with a V notch of `angle_deg`, 20 mm deep,
/// in the middle of its top edge.
fn vee_notched(angle_deg: f64) -> Polyline {
    let half = (angle_deg / 2.0).to_radians().tan() * 20.0;
    closed(&[
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 40.0),
        (30.0 + half, 40.0),
        (30.0, 20.0),
        (30.0 - half, 40.0),
        (0.0, 40.0),
    ])
}

/// Points along every cutting move of `moves`, from `from`, at most
/// `step_mm` apart, arcs along the arc, Z interpolated.
fn cut_points(from: Point3, moves: &[Move], step_mm: f64) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut at = from;
    for mv in moves {
        if !mv.is_rapid() {
            let arc = match mv.kind {
                MoveKind::ArcCcw { centre } => Some((centre, true)),
                MoveKind::ArcCw { centre } => Some((centre, false)),
                _ => None,
            };
            let (length_mm, point_at): (f64, Box<dyn Fn(f64) -> Point>) = match arc {
                Some((centre, ccw)) => {
                    let sweep = sweep_rad(at.xy(), mv.to.xy(), centre, ccw);
                    let radius = at.xy().distance_to(centre);
                    let start = (at.y - centre.y).atan2(at.x - centre.x);
                    (
                        radius * sweep.abs(),
                        Box::new(move |f: f64| {
                            let angle = start + sweep * f;
                            Point::new(
                                centre.x + radius * angle.cos(),
                                centre.y + radius * angle.sin(),
                            )
                        }),
                    )
                }
                None => {
                    let (a, b) = (at.xy(), mv.to.xy());
                    (a.distance_to(b), Box::new(move |f: f64| a + (b - a) * f))
                }
            };
            let steps = (length_mm / step_mm).ceil().max(1.0);
            let mut k = 0.0;
            while k <= steps {
                let f = k / steps;
                let p = point_at(f);
                out.push(Point3::new(p.x, p.y, at.z + (mv.to.z - at.z) * f));
                k += 1.0;
            }
        }
        at = mv.to;
    }
    out
}

/// The independent tab check the #310 review asked for, on the
/// emitted moves: over every point of the stock the last pass leaves
/// standing above its floor (a tab's material: within the radius of
/// its cuts, none of which within the radius is that low), no cut of
/// an earlier pass comes within the radius below that height. Points
/// are on a 0.15 mm lattice; the radius is held 0.03 mm short and the
/// height 0.01 mm, for the lattice and the moves' sampling. Returns
/// the worst cut below a standing height, mm, and how many lattice
/// points stand.
#[allow(
    clippy::cast_possible_truncation,
    reason = "lattice indices of coordinates within the stock"
)]
fn earlier_cuts_into_tabs(tp: &Toolpath, split: usize, floor_mm: f64) -> (f64, usize) {
    const LATTICE_MM: f64 = 0.15;
    let last = cut_points(tp.moves[split].to, &tp.moves[split + 1..], 0.05);
    let earlier = cut_points(tp.moves[0].to, &tp.moves[..split], 0.05);
    let cell = R_MM;
    let key = |p: Point| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut buckets: BTreeMap<(i64, i64), Vec<Point3>> = BTreeMap::new();
    for p in &last {
        buckets.entry(key(p.xy())).or_default().push(*p);
    }
    let mut earlier_buckets: BTreeMap<(i64, i64), Vec<Point3>> = BTreeMap::new();
    for p in &earlier {
        earlier_buckets.entry(key(p.xy())).or_default().push(*p);
    }
    let near = |map: &BTreeMap<(i64, i64), Vec<Point3>>, p: Point, reach: f64| {
        let (i, j) = key(p);
        let mut found: Vec<Point3> = Vec::new();
        for di in -1..=1 {
            for dj in -1..=1 {
                if let Some(points) = map.get(&(i + di, j + dj)) {
                    found.extend(points.iter().filter(|q| q.xy().distance_to(p) < reach));
                }
            }
        }
        found
    };
    // Lattice points round every raised cut of the last pass.
    let mut lattice: std::collections::BTreeSet<(i64, i64)> = Default::default();
    let steps = (R_MM / LATTICE_MM).ceil() as i64;
    for p in last.iter().filter(|p| p.z > floor_mm + 1e-6) {
        let (ci, cj) = (
            (p.x / LATTICE_MM).round() as i64,
            (p.y / LATTICE_MM).round() as i64,
        );
        for di in -steps..=steps {
            for dj in -steps..=steps {
                lattice.insert((ci + di, cj + dj));
            }
        }
    }
    let mut worst: f64 = 0.0;
    let mut standing = 0;
    for (i, j) in lattice {
        let p = Point::new(i as f64 * LATTICE_MM, j as f64 * LATTICE_MM);
        let cuts = near(&buckets, p, R_MM);
        let Some(height) = cuts.iter().map(|q| q.z).reduce(f64::min) else {
            continue;
        };
        if height <= floor_mm + 1e-6 {
            continue;
        }
        standing += 1;
        for q in near(&earlier_buckets, p, R_MM - 0.03) {
            worst = worst.max(height - 0.01 - q.z);
        }
    }
    (worst, standing)
}

/// The #310 review's repro: four 8 × 6 mm tabs, 12 mm deep in 2 mm
/// steps with 0.5 mm stock to leave, so the passes at Z 4 and 2 are
/// earlier passes below the tab top. The star of radii 35 and 15
/// Outside and Inside, whose 42° tips the last pass cuts since #324,
/// and the 60° and 45° triangles both ways. Rect and smooth, requested
/// and placed: no earlier pass cuts below what the last pass leaves
/// standing, anywhere over a tab.
#[test]
fn last_pass_tabs_keep_off_the_tab_material() {
    for (side, outline) in [
        (Side::Outside, star(35.0, 15.0)),
        (Side::Inside, star(35.0, 15.0)),
        (Side::Inside, triangle_60()),
        (Side::Outside, triangle_60()),
        (Side::Inside, triangle_45()),
        (Side::Outside, triangle_45()),
    ] {
        let part = at_20_20().apply_polyline(&outline).unwrap();
        let mut p = last_pass_project_of(vec![outline]);
        p.stock.width_mm = 300.0;
        for shape in [TabShape::Rect, TabShape::Smooth] {
            let op = ProfileOp {
                step_down_mm: Some(2.0),
                tabs: Some(TabSpec {
                    placement: TabPlacement::Count { count: 4 },
                    width_mm: 8.0,
                    height_mm: 6.0,
                    shape,
                    on_holes: false,
                    avoid_corners: false,
                }),
                ..last_pass_op(&p, side)
            };
            let placed = crate::placed_tabs::place_requests(&{
                let mut q = p.clone();
                q.operations = vec![Operation::Profile(op.clone())];
                q
            })
            .unwrap()
            .0;
            let placed_op = match &placed.operations[0] {
                Operation::Profile(op) => op.clone(),
                _ => unreachable!("a Profile"),
            };
            for (case, project, op) in [("requested", &p, &op), ("placed", &placed, &placed_op)] {
                let case = format!("{side:?} {shape:?} {case}");
                let (tp, _) = generate(project, op);
                let split = last_pass_start(&tp, &part, 2.0, STOCK_TO_LEAVE_MM);
                let (worst_mm, standing) = earlier_cuts_into_tabs(&tp, split, 0.0);
                assert!(
                    standing > 100,
                    "{case}: the last pass leaves tabs: {standing}"
                );
                assert!(
                    worst_mm <= 0.0,
                    "{case}: an earlier pass cuts {worst_mm} mm into a tab"
                );
            }
        }
    }
}

/// The #310 review, kept by #324: a notch or tip the earlier passes
/// skip, which the last pass would cut whole at full depth, is
/// refused: Outside, a 7 × 3 mm notch with 0.5 mm (the last pass
/// would take 1.0 mm off, as thin as a 60° corner's, but the earlier
/// passes never enter the notch) and a 10 × 4 mm notch with 2.1 mm;
/// Inside, a tip of 10° half-angle with 0.5 mm, which the last pass
/// would cut 2.9 mm thick, more than a third of the bit. Square
/// corners are cut at any stock to leave up to a third of the bit: a
/// rectangle Inside and Outside and an L Outside at 2.1 mm.
#[test]
fn last_pass_refuses_a_last_pass_thicker_than_the_stock_to_leave() {
    let points = |pts: &[(f64, f64)]| {
        Polyline::new(pts.iter().map(|&(x, y)| Vertex::line(x, y)).collect(), true)
    };
    let notched = |width: f64, depth: f64| {
        let (l, r) = (30.0 - width / 2.0, 30.0 + width / 2.0);
        points(&[
            (0.0, 0.0),
            (60.0, 0.0),
            (60.0, 40.0),
            (r, 40.0),
            (r, 40.0 - depth),
            (l, 40.0 - depth),
            (l, 40.0),
            (0.0, 40.0),
        ])
    };
    let half = 10.0_f64.to_radians().tan() * 30.0;
    let tipped = points(&[
        (0.0, 20.0),
        (30.0, 20.0 - half),
        (30.0, 0.0),
        (70.0, 0.0),
        (70.0, 40.0),
        (30.0, 40.0),
        (30.0, 20.0 + half),
    ]);
    let refused = [
        ("7 × 3 notch", Side::Outside, notched(7.0, 3.0), 0.5),
        ("10 × 4 notch", Side::Outside, notched(10.0, 4.0), 2.1),
        ("10° tip", Side::Inside, tipped.clone(), 0.5),
    ];
    for (case, side, outline, stock_to_leave_mm) in refused {
        let p = last_pass_project_of(vec![outline]);
        let op = ProfileOp {
            last_pass: Some(stock_to_leave(stock_to_leave_mm)),
            ..last_pass_op(&p, side)
        };
        let err = generate_profile(&p, &op).expect_err(case);
        assert!(
            matches!(err, CamError::ShapeNarrowerThanStockToLeave { .. }),
            "{case}: {err:?}"
        );
        generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op
            },
        );
    }
    let ell = points(&[
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 20.0),
        (30.0, 20.0),
        (30.0, 40.0),
        (0.0, 40.0),
    ]);
    for (case, side, outline) in [
        ("rectangle", Side::Inside, rect_60x40()),
        ("rectangle", Side::Outside, rect_60x40()),
        ("L", Side::Outside, ell),
    ] {
        let p = last_pass_project_of(vec![outline]);
        let op = ProfileOp {
            last_pass: Some(stock_to_leave(2.1)),
            ..last_pass_op(&p, side)
        };
        generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case} {side:?}: {e}"));
    }
    // With 0.2 mm the tip's wedge is 1.15 mm, within a third of
    // the bit, and the notch's cut no more than a square corner's.
    for (case, side, outline) in [
        ("10° tip", Side::Inside, tipped),
        ("7 × 3 notch", Side::Outside, notched(7.0, 3.0)),
    ] {
        let p = last_pass_project_of(vec![outline]);
        let op = ProfileOp {
            last_pass: Some(stock_to_leave(0.2)),
            ..last_pass_op(&p, side)
        };
        generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case} at 0.2: {e}"));
    }
}

/// One squared distance transform along a line of the lattice
/// (Felzenszwalb and Huttenlocher's lower envelope of parabolas):
/// `f` is 0 where a point is not removed and huge where it is.
#[allow(
    clippy::cast_precision_loss,
    reason = "lattice indices of coordinates within the stock"
)]
fn squared_distances(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    let at = |q: usize| q as f64;
    let mut v = vec![0usize; n];
    let mut z = vec![0.0; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let meet = |q: usize, p: usize| {
        ((f[q] + at(q) * at(q)) - (f[p] + at(p) * at(p))) / (2.0 * (at(q) - at(p)))
    };
    for q in 1..n {
        let mut s = meet(q, v[k]);
        // z[0] is −∞, so this stops at k = 0.
        while s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, slot) in out.iter_mut().enumerate() {
        while z[k + 1] < at(q) {
            k += 1;
        }
        let p = v[k];
        *slot = (at(q) - at(p)).powi(2) + f[p];
    }
    out
}

/// The #324 swept-area check, on the emitted moves, independent of
/// the generator's own measure: the stock at `layer_z_mm`, on a
/// 0.05 mm lattice, cut by every earlier cut through that layer (the
/// moves before `split`), then by the last pass's (the moves after
/// it). Returns the diameter of the widest disc in what the last pass
/// removes there, less one lattice step for the lattice (the thickest
/// it cuts, anywhere, mm), and the area it removes, mm².
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "lattice indices of coordinates within the stock"
)]
fn last_pass_removal(tp: &Toolpath, split: usize, layer_z_mm: f64) -> (f64, f64) {
    const LATTICE_MM: f64 = 0.05;
    let through = |from: Point3, moves: &[Move], step_mm: f64| -> Vec<Point> {
        cut_points(from, moves, step_mm)
            .into_iter()
            .filter(|p| p.z <= layer_z_mm)
            .map(|p| p.xy())
            .collect()
    };
    let earlier = through(tp.moves[0].to, &tp.moves[..split], 0.1);
    let last = through(tp.moves[split].to, &tp.moves[split + 1..], 0.05);
    let margin = R_MM + 2.0 * LATTICE_MM;
    let (mut lo, mut hi) = (
        Point::new(f64::MAX, f64::MAX),
        Point::new(f64::MIN, f64::MIN),
    );
    for p in &last {
        lo = Point::new(lo.x.min(p.x - margin), lo.y.min(p.y - margin));
        hi = Point::new(hi.x.max(p.x + margin), hi.y.max(p.y + margin));
    }
    let nx = ((hi.x - lo.x) / LATTICE_MM).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / LATTICE_MM).ceil() as usize + 1;
    // 0: stock; 1: cut before the last pass; 2: cut by the last pass.
    let mut cells = vec![0u8; nx * ny];
    let stamp = |cells: &mut [u8], c: Point, set: u8| {
        let j0 = ((c.y - R_MM - lo.y) / LATTICE_MM).floor().max(0.0) as usize;
        let j1 = (((c.y + R_MM - lo.y) / LATTICE_MM).ceil() as usize).min(ny - 1);
        for j in j0..=j1 {
            let dy = lo.y + j as f64 * LATTICE_MM - c.y;
            let reach = R_MM * R_MM - dy * dy;
            if reach < 0.0 {
                continue;
            }
            let half = reach.sqrt();
            let i0 = ((c.x - half - lo.x) / LATTICE_MM).ceil().max(0.0) as usize;
            let i1 = (((c.x + half - lo.x) / LATTICE_MM).floor() as usize).min(nx - 1);
            for cell in &mut cells[j * nx + i0..=j * nx + i1] {
                if *cell == 0 {
                    *cell = set;
                }
            }
        }
    };
    for p in &earlier {
        stamp(&mut cells, *p, 1);
    }
    for p in &last {
        stamp(&mut cells, *p, 2);
    }
    let removed = cells.iter().filter(|&&c| c == 2).count();
    // More than any squared distance on the lattice.
    let far = ((nx + ny) as f64).powi(2);
    let mut f: Vec<f64> = cells
        .iter()
        .map(|&c| if c == 2 { far } else { 0.0 })
        .collect();
    for row in f.chunks_mut(nx) {
        let d = squared_distances(row);
        row.copy_from_slice(&d);
    }
    for i in 0..nx {
        let column: Vec<f64> = (0..ny).map(|j| f[j * nx + i]).collect();
        for (j, d) in squared_distances(&column).into_iter().enumerate() {
            f[j * nx + i] = d;
        }
    }
    let widest = f.iter().copied().fold(0.0, f64::max).sqrt();
    (
        (2.0 * widest - 1.0).max(0.0) * LATTICE_MM,
        removed as f64 * LATTICE_MM * LATTICE_MM,
    )
}

/// Sharp corners are cut, not refused. Inside the 60° and 45°
/// triangles and the star of radii 35 and 15 (42° tips), Outside that
/// star and 80° and 60° V notches, at 0.2 and 0.5 mm with the 6.35 mm
/// bit: each is generated (and passes `assert_safe`), the earlier
/// passes keep `r + a` from the part and the last pass exactly `r`.
/// The material check ([`last_pass_removal`]) finds the last pass
/// cutting the corner's wedge, `a / sin(θ/2)` thick, and nowhere more
/// than the bound: a third of the bit, or `√2 · a + 0.05` where that
/// is more.
#[test]
fn last_pass_allows_sharp_corners() {
    let bound_mm = |a: f64| (6.35 / 3.0_f64).max(2f64.sqrt() * a + 0.05);
    let star_tip_deg = 2.0
        * (15.0 * (0.2 * PI).sin())
            .atan2(35.0 - 15.0 * (0.2 * PI).cos())
            .to_degrees();
    for (case, side, outline, corner_deg) in [
        ("60° triangle", Side::Inside, triangle_60(), 60.0),
        ("45° triangle", Side::Inside, triangle_45(), 45.0),
        ("star", Side::Inside, star(35.0, 15.0), star_tip_deg),
        ("star", Side::Outside, star(35.0, 15.0), 114.2),
        ("80° V notch", Side::Outside, vee_notched(80.0), 80.0),
        ("60° V notch", Side::Outside, vee_notched(60.0), 60.0),
    ] {
        let part = at_20_20().apply_polyline(&outline).unwrap();
        for a in [0.2, 0.5] {
            let case = format!("{case} {side:?} at {a} mm");
            let p = last_pass_project_of(vec![outline.clone()]);
            let op = ProfileOp {
                last_pass: Some(stock_to_leave(a)),
                ..last_pass_op(&p, side)
            };
            let (tp, _) = generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case}: {e}"));
            assert_safe(&tp, &p, &Operation::Profile(op.clone()));
            let split = last_pass_start(&tp, &part, LAST_PASS_Z_MM[1], a);
            let (near, _) = gap_range_mm(&part, tp.moves[0].to, &tp.moves[..split]).unwrap();
            assert!(near >= R_MM + a - 1e-3, "{case}: an earlier pass at {near}");
            let (near, _) =
                gap_range_mm(&part, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
            assert!(
                (near - R_MM).abs() <= 1e-3,
                "{case}: the last pass at {near}"
            );
            let (widest_mm, removed_mm2) = last_pass_removal(&tp, split, LAST_PASS_Z_MM[1] + 0.01);
            let wedge_mm = a / (corner_deg / 2.0_f64).to_radians().sin();
            assert!(removed_mm2 > 0.0, "{case}");
            assert!(
                widest_mm >= wedge_mm.max(a) - 0.15,
                "{case}: the last pass cuts {widest_mm} mm, not the {wedge_mm} mm wedge"
            );
            assert!(
                widest_mm <= bound_mm(a) + 0.1,
                "{case}: the last pass cuts {widest_mm} mm thick, over {}",
                bound_mm(a)
            );
        }
    }
}

/// A round hole `diameter_mm` across, centred on the origin: two half
/// arcs, counter-clockwise.
fn round_hole(diameter_mm: f64) -> Polyline {
    let r = diameter_mm / 2.0;
    Polyline::new(
        vec![Vertex::new(r, 0.0, 1.0), Vertex::new(-r, 0.0, 1.0)],
        true,
    )
}

/// The tool-centre arcs of the last pass of a one-shape toolpath:
/// every level arc after [`last_pass_start`], with its radius
/// and feed.
fn last_pass_arcs(tp: &Toolpath, outline: &Polyline, stock_to_leave_mm: f64) -> Vec<(f64, f64)> {
    let split = last_pass_start(tp, outline, LAST_PASS_Z_MM[1], stock_to_leave_mm);
    let mut at = tp.moves[split].to;
    let mut arcs = Vec::new();
    for mv in &tp.moves[split + 1..] {
        if let MoveKind::ArcCcw { centre } | MoveKind::ArcCw { centre } = mv.kind
            && (mv.to.z - at.z).abs() <= EPS_MM
        {
            arcs.push((mv.to.xy().distance_to(centre), mv.feed_mm_min.unwrap()));
        }
        at = mv.to;
    }
    arcs
}

/// In a round hole cut Inside, the tool centre runs a circle of
/// `ρ − r` round a wall of `ρ`, so its cutting edge moves `ρ / (ρ − r)`
/// times as fast as the centre. The last pass runs those arcs at
/// `feed · (ρ − r) / ρ` (the spec's `k` is 1), so its edge moves at the
/// feed: the 8, 10 and 15 mm holes with a 6.35 mm bit, at 0.2, 0.5 and
/// 2 mm where they fit, climb and conventional, with no warning. The
/// earlier passes keep the feed. A 60 × 40 mm rectangle's last pass,
/// Inside and Outside, is unchanged: every level cut at the feed.
#[test]
fn last_pass_small_hole_edge_feed() {
    const FEED_MM_MIN: f64 = 1000.0;
    for diameter_mm in [8.0, 10.0, 15.0] {
        let p = last_pass_project_of(vec![round_hole(diameter_mm)]);
        let hole = at_20_20().apply_polyline(&round_hole(diameter_mm)).unwrap();
        for a in [0.2, 0.5, 2.0] {
            if diameter_mm < 6.35 + 2.0 * a {
                continue;
            }
            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                let case = format!("{diameter_mm} mm hole at {a} mm {direction:?}");
                let op = ProfileOp {
                    direction,
                    last_pass: Some(stock_to_leave(a)),
                    ..last_pass_op(&p, Side::Inside)
                };
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{case}: {warnings:?}");
                let rho = diameter_mm / 2.0;
                let expected = FEED_MM_MIN * (rho - R_MM) / rho;
                let arcs = last_pass_arcs(&tp, &hole, a);
                assert!(arcs.len() >= 2, "{case}: {arcs:?}");
                for (radius_mm, feed) in arcs {
                    assert_approx_eq!(radius_mm, rho - R_MM, 1e-6);
                    assert_approx_eq!(feed, expected, 1e-9);
                }
                let split = last_pass_start(&tp, &hole, LAST_PASS_Z_MM[1], a);
                assert!(
                    tp.moves[..split]
                        .iter()
                        .all(|m| m.is_rapid() || m.feed_mm_min.unwrap() >= 300.0 - 1e-9),
                    "{case}: an earlier pass slows down"
                );
            }
        }
    }
    let outline = placed_rect();
    for side in [Side::Inside, Side::Outside] {
        let p = last_pass_project();
        let (tp, _) = generate(&p, &last_pass_op(&p, side));
        let split = last_pass_start(&tp, &outline, LAST_PASS_Z_MM[1], STOCK_TO_LEAVE_MM);
        let mut at = tp.moves[split].to;
        for mv in &tp.moves[split + 1..] {
            if !mv.is_rapid() && (mv.to.z - at.z).abs() <= EPS_MM {
                assert_eq!(mv.feed_mm_min, Some(FEED_MM_MIN), "{side:?}: {mv:?}");
            }
            at = mv.to;
        }
    }
}

/// Where the last pass's inside arc is tighter than the
/// stock to leave (`ρ − r < a`), its feed is lowered only as far as the
/// smallest hole the last pass cuts needs, `feed · a / (a + r)`, the
/// least the safety pass's S3 accepts, and the edge there moves faster
/// than the feed: `last_pass_edge_faster` says so, naming the shape
/// and the arc. A 40 × 30 mm hole with 3.5 mm corner radii, cut Inside
/// with a 6.35 mm bit at 0.5 mm: its last pass turns its corners on
/// arcs of 0.325 mm.
#[test]
fn last_pass_tight_inside_arc_warns() {
    let b = (PI / 8.0).tan();
    let (w, h, f) = (40.0, 30.0, 3.5);
    let rounded = Polyline::new(
        vec![
            Vertex::line(f, 0.0),
            Vertex::new(w - f, 0.0, b),
            Vertex::line(w, f),
            Vertex::new(w, h - f, b),
            Vertex::line(w - f, h),
            Vertex::new(f, h, b),
            Vertex::line(0.0, h - f),
            Vertex::new(0.0, f, b),
        ],
        true,
    );
    let p = last_pass_project_of(vec![rounded.clone()]);
    let op = last_pass_op(&p, Side::Inside);
    let (tp, warnings) = generate(&p, &op);
    let edge: Vec<&Warning> = warnings
        .iter()
        .filter(|w| w.code().as_str() == "last_pass_edge_faster")
        .collect();
    assert_eq!(edge.len(), 1, "{warnings:?}");
    let message = edge[0].to_string();
    assert!(message.contains("0.325 mm"), "{message}");
    let outline = at_20_20().apply_polyline(&rounded).unwrap();
    let floor = 1000.0 * STOCK_TO_LEAVE_MM / (STOCK_TO_LEAVE_MM + R_MM);
    for (radius_mm, feed) in last_pass_arcs(&tp, &outline, STOCK_TO_LEAVE_MM) {
        assert_approx_eq!(radius_mm, f - R_MM, 1e-6);
        assert_approx_eq!(feed, floor, 1e-9);
    }
}

/// An ellipse `width_mm` × `height_mm` centred on the origin, flattened
/// counter-clockwise to straight segments no farther than
/// `tolerance_mm` from the curve, as an importer flattens a DXF
/// `ELLIPSE` or SVG Bézier: each chord's sagitta is
/// `tolerance_mm` at the curve's local radius of curvature.
fn flattened_ellipse(width_mm: f64, height_mm: f64, tolerance_mm: f64) -> Polyline {
    let (a, b) = (width_mm / 2.0, height_mm / 2.0);
    let mut vertices = Vec::new();
    let mut t: f64 = 0.0;
    while t < TAU - 1e-9 {
        vertices.push(Vertex::line(a * t.cos(), b * t.sin()));
        let speed = (a * a * t.sin().powi(2) + b * b * t.cos().powi(2)).sqrt();
        let curvature_radius_mm = speed.powi(3) / (a * b);
        let chord_mm = (8.0 * tolerance_mm * curvature_radius_mm).sqrt();
        t += chord_mm / speed;
    }
    Polyline::new(vertices, true)
}

/// The `last_pass_flattened_curve` warnings of `warnings`.
fn flattened_curve_warnings(warnings: &[Warning]) -> Vec<&Warning> {
    warnings
        .iter()
        .filter(|w| w.code().as_str() == "last_pass_flattened_curve")
        .collect()
}

/// A curve drawn as straight segments gets none of #335's
/// slowdown, so round its inside bends the last pass's cutting edge
/// moves faster than the feed. The last pass's feeds stay as they are,
/// and the result warns `last_pass_flattened_curve`, naming the shape:
/// a 12 × 8 mm ellipse flattened to 0.01 mm, cut Inside with a 6.35 mm
/// bit at 0.5 mm, climb and conventional. The 60 × 40 mm rectangle,
/// Inside and Outside, and a true-arc 10 mm hole carry no such warning.
#[test]
fn last_pass_flattened_hole_edge_feed() {
    let ellipse = flattened_ellipse(12.0, 8.0, 0.01);
    let outline = at_20_20().apply_polyline(&ellipse).unwrap();
    let p = last_pass_project_of(vec![ellipse]);
    for direction in [CutDirection::Climb, CutDirection::Conventional] {
        let op = ProfileOp {
            direction,
            ..last_pass_op(&p, Side::Inside)
        };
        let (tp, warnings) = generate(&p, &op);
        assert_safe(&tp, &p, &Operation::Profile(op.clone()));
        let flagged = flattened_curve_warnings(&warnings);
        assert_eq!(flagged.len(), 1, "{direction:?}: {warnings:?}");
        let message = flagged[0].to_string();
        assert!(message.contains("`Shape i1`"), "{message}");
        assert!(message.contains("times the feed"), "{message}");
        assert_eq!(flagged[0].op(), Some(&op.id));
        // Feeds unchanged: every level cut of the last pass runs at
        // the feed.
        let split = last_pass_start(&tp, &outline, LAST_PASS_Z_MM[1], STOCK_TO_LEAVE_MM);
        let mut at = tp.moves[split].to;
        for mv in &tp.moves[split + 1..] {
            if !mv.is_rapid() && (mv.to.z - at.z).abs() <= EPS_MM {
                assert_eq!(mv.feed_mm_min, Some(FEED_MM_MIN), "{direction:?}: {mv:?}");
            }
            at = mv.to;
        }
    }
    for side in [Side::Inside, Side::Outside] {
        let p = last_pass_project();
        let (_, warnings) = generate(&p, &last_pass_op(&p, side));
        assert!(
            flattened_curve_warnings(&warnings).is_empty(),
            "rectangle {side:?}: {warnings:?}"
        );
    }
    let p = last_pass_project_of(vec![round_hole(10.0)]);
    let (_, warnings) = generate(&p, &last_pass_op(&p, Side::Inside));
    assert!(
        flattened_curve_warnings(&warnings).is_empty(),
        "true-arc hole: {warnings:?}"
    );
}

/// A cut of one pass has no pass before the last to
/// leave the stock to leave, so it cuts as without a last pass and warns
/// `last_pass_unused`.
#[test]
fn last_pass_on_a_one_pass_cut_warns_unused() {
    let p = rect_project();
    let op = ProfileOp {
        depth_mm: Some(2.0),
        ..profile_op(&p, Side::Outside, CutDirection::Climb)
    };
    let (plain, plain_warnings) = generate(&p, &op);
    assert!(plain_warnings.is_empty(), "{plain_warnings:?}");
    let (with, warnings) = generate(
        &p,
        &ProfileOp {
            last_pass: Some(stock_to_leave(STOCK_TO_LEAVE_MM)),
            ..op
        },
    );
    assert_eq!(with, plain);
    assert_eq!(codes(&warnings), vec![WarningCode::LastPassUnused]);
    assert!(
        warnings[0].to_string().contains("one pass"),
        "{}",
        warnings[0]
    );
}

/// Inside, an island 7 mm from the outline leaves room for the
/// 6.35 mm bit but not for the bit plus a 0.5 mm stock to leave on both
/// lines (7.35 mm): refused, naming the stock to leave, where the plain
/// cut and a 0.3 mm stock to leave are cut.
#[test]
fn last_pass_band_too_narrow_for_stock_to_leave() {
    let mut p = ring_project(25.0 - 7.0);
    p.stock.thickness_mm = Some(LAST_STOCK_MM);
    let op = |stock_to_leave_mm: Option<f64>| ProfileOp {
        last_pass: stock_to_leave_mm.map(stock_to_leave),
        ..last_pass_op(&p, Side::Inside)
    };
    generate(&p, &op(None));
    generate(&p, &op(Some(0.3)));
    let err = generate_profile(&p, &op(Some(STOCK_TO_LEAVE_MM)))
        .expect_err("a band narrower than the bit and stock to leave is refused");
    let CamError::BandNarrowerThanStockToLeave {
        op: id,
        shape,
        other,
        band_mm,
        diameter_mm,
        stock_to_leave_mm,
        needed_mm,
    } = &err
    else {
        panic!("expected BandNarrowerThanStockToLeave, got {err:?}");
    };
    assert_eq!(id.as_str(), "o1");
    assert_eq!((shape.profile, other.profile), (0, 1), "outline first");
    assert_approx_eq!(*band_mm, 7.0, TOL);
    assert_approx_eq!(*diameter_mm, 6.35, TOL);
    assert_approx_eq!(*stock_to_leave_mm, STOCK_TO_LEAVE_MM, TOL);
    assert_approx_eq!(*needed_mm, 7.35, TOL);
    let text = err.to_string();
    for needle in [
        "`o1`",
        "7.000 mm",
        "7.350 mm",
        "6.35 mm",
        "0.5 mm",
        "stock to leave",
    ] {
        assert!(text.contains(needle), "{needle:?} in {text}");
    }
}

/// Inside a shape whose neck, or whose slot, is wider than the
/// bit but narrower than the bit plus the stock to leave on both sides,
/// the stock passes would skip it and leave the last pass a full
/// slot at full depth there; a hole that only the bit fits leaves the
/// stock passes no path at all. Each is refused naming the shape
/// and the stock to leave; a 0.2 mm stock to leave fits all three.
#[test]
fn last_pass_neck_slot_or_hole_too_narrow_for_stock_to_leave() {
    let points = |pts: &[(f64, f64)]| {
        Polyline::new(pts.iter().map(|&(x, y)| Vertex::line(x, y)).collect(), true)
    };
    // Two 30 mm squares joined by a 7 mm wide neck.
    let dumbbell = points(&[
        (0.0, 0.0),
        (30.0, 0.0),
        (30.0, 11.5),
        (40.0, 11.5),
        (40.0, 0.0),
        (70.0, 0.0),
        (70.0, 30.0),
        (40.0, 30.0),
        (40.0, 18.5),
        (30.0, 18.5),
        (30.0, 30.0),
        (0.0, 30.0),
    ]);
    // A 60 × 40 mm rectangle with a 7 mm wide, 20 mm long slot.
    let slotted = points(&[
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 40.0),
        (33.5, 40.0),
        (33.5, 60.0),
        (26.5, 60.0),
        (26.5, 40.0),
        (0.0, 40.0),
    ]);
    let small_hole = Polyline::rect(Point::new(0.0, 0.0), 7.0, 7.0);
    for (case, outline) in [("neck", dumbbell), ("slot", slotted), ("hole", small_hole)] {
        let p = last_pass_project_of(vec![outline]);
        let op = last_pass_op(&p, Side::Inside);
        let err = generate_profile(&p, &op).expect_err(case);
        let CamError::ShapeNarrowerThanStockToLeave {
            shape,
            diameter_mm,
            stock_to_leave_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected ShapeNarrowerThanStockToLeave, got {err:?}");
        };
        assert_eq!(shape.profile, 0, "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        assert_approx_eq!(*stock_to_leave_mm, STOCK_TO_LEAVE_MM, TOL);
        assert!(err.to_string().contains("stock to leave"), "{case}: {err}");
        generate(
            &p,
            &ProfileOp {
                last_pass: Some(stock_to_leave(0.2)),
                ..op.clone()
            },
        );
        generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op
            },
        );
    }
}

/// Outside, a part inside another part's 40 mm hole, 6.6 mm
/// from its walls: the plain cut keeps the radius from every line,
/// but the part's stock passes, 0.5 mm farther out, would cut
/// 0.25 mm into the frame round the hole, which S9 rejects. Refused,
/// naming the hole and the part; a 0.2 mm stock to leave fits.
#[test]
fn last_pass_stock_to_leave_into_a_neighbour_is_refused() {
    let p = last_pass_project_of(vec![
        Polyline::rect(Point::new(0.0, 0.0), 80.0, 80.0),
        Polyline::rect(Point::new(20.0, 20.0), 40.0, 40.0),
        Polyline::rect(Point::new(26.6, 26.6), 26.8, 26.8),
    ]);
    let op = last_pass_op(&p, Side::Outside);
    let (_, warnings) = generate(
        &p,
        &ProfileOp {
            last_pass: None,
            ..op.clone()
        },
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    generate(
        &p,
        &ProfileOp {
            last_pass: Some(stock_to_leave(0.2)),
            ..op.clone()
        },
    );
    let err = generate_profile(&p, &op).expect_err("the stock passes would gouge");
    let CamError::BandNarrowerThanStockToLeave {
        shape,
        other,
        band_mm,
        ..
    } = &err
    else {
        panic!("expected BandNarrowerThanStockToLeave, got {err:?}");
    };
    assert_eq!((shape.profile, other.profile), (1, 2), "{err}");
    assert_approx_eq!(*band_mm, 6.6, 1e-9);

    // The #310 review's property case: 0.3 mm from the hole's wall,
    // the part's laps already cut into each other (D6 warns), and its
    // stock passes run wholly inside the frame, and the hole's
    // inside the part, more than a radius past either line.
    let p = last_pass_project_of(vec![
        Polyline::rect(Point::new(0.0, 0.0), 80.0, 80.0),
        Polyline::rect(Point::new(20.0, 20.0), 40.0, 40.0),
        Polyline::rect(Point::new(20.3, 20.3), 39.4, 39.4),
    ]);
    let op = last_pass_op(&p, Side::Outside);
    let (_, warnings) = generate(
        &p,
        &ProfileOp {
            last_pass: None,
            ..op.clone()
        },
    );
    assert!(
        codes(&warnings).contains(&WarningCode::ToolpathsOverlap),
        "{warnings:?}"
    );
    let err = generate_profile(&p, &op).expect_err("the stock passes would cut a part");
    assert!(
        matches!(err, CamError::BandNarrowerThanStockToLeave { .. }),
        "{err:?}"
    );
}

/// Acceptance 46 Inside: the first two passes run 0.5 mm farther
/// into the waste than the third, which is the plain Inside path.
#[test]
fn last_pass_inside_rectangle() {
    let p = last_pass_project();
    let op = last_pass_op(&p, Side::Inside);
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (plain, _) = generate(
        &p,
        &ProfileOp {
            last_pass: None,
            ..op.clone()
        },
    );
    let laps = last_pass_laps(&tp);
    assert_eq!(laps.len(), 3);
    let shrunk = 2.0 * (R_MM + STOCK_TO_LEAVE_MM);
    for lap in &laps[..2] {
        assert_approx_eq!(lap.x_extent_mm(), 60.0 - shrunk, TOL);
        assert_approx_eq!(lap.y_extent_mm(), 40.0 - shrunk, TOL);
        assert!(lap.area_mm2 > 0.0, "climb inside is counter-clockwise");
    }
    assert_approx_eq!(laps[2].x_extent_mm(), 60.0 - 6.35, TOL);
    assert_eq!(laps[2].moves, laps_at(&plain, 0.0)[0].moves);
    let outline = placed_rect();
    let split = last_pass_start(&tp, &outline, LAST_PASS_Z_MM[1], STOCK_TO_LEAVE_MM);
    let (near, _) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split]).unwrap();
    assert_approx_eq!(near, R_MM + STOCK_TO_LEAVE_MM, TOL);
    let (near, far) = gap_range_mm(&outline, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
    assert_approx_eq!(near, R_MM, TOL);
    assert_approx_eq!(far, R_MM, TOL);
}

/// With `last_pass: null` the toolpath is the one the Profile
/// cut before: move for move the phase 1 recipe of
/// [`one_shape_golden_is_byte_identical_after_sequence`], and the same
/// as with a last pass whose cut has only one pass, which the last
/// pass cuts on the part's path. (`dreamcarve`'s migration corpus
/// holds whole programs to the bytes the build before #284 wrote.)
#[test]
fn no_last_pass_is_byte_identical() {
    let p = rect_project();
    let op = profile_op(&p, Side::Outside, CutDirection::Climb);
    assert_eq!(op.last_pass, None);
    let (plain, _) = generate(&p, &op);
    let placed = at_20_20().apply_polyline(&rect_100x50()).unwrap();
    let centre = offset(&Contour::new(placed).unwrap(), R_MM).remove(0);
    let path = if centre.is_ccw() {
        centre.reversed().into_polyline()
    } else {
        centre.into_polyline()
    };
    let start = path.vertices()[0].point();
    let laps = laps(&plain);
    assert_eq!(laps.len(), PASS_Z_MM.len());
    for lap in &laps {
        assert_eq!(lap.points[0], start);
        assert_eq!(lap.moves.len(), path.segments().count());
    }
    // One pass in all: the last pass is the cut, as without one.
    let deep = ProfileOp {
        depth_mm: Some(2.0),
        ..op.clone()
    };
    let (one, _) = generate(&p, &deep);
    let (with, _) = generate(
        &p,
        &ProfileOp {
            last_pass: Some(stock_to_leave(STOCK_TO_LEAVE_MM)),
            ..deep
        },
    );
    assert_eq!(one, with);
}

/// #284 with leads, ramps, helixes and corner relief, Inside and
/// Outside, climb and conventional: every cut of the
/// stock passes, their descents and lead arcs included, keeps
/// `r + a` from the part (within the waste domain's slack); the last
/// pass visits every vertex of the part's path at full depth; relief
/// notches are cut from the last pass, never into the stock to leave
/// skin; and the whole toolpath is safe (S1–S3: depth, rapids, every
/// feed and plunge rate the tool's).
#[test]
fn last_pass_with_leads_ramps_and_relief() {
    let lead = Lead {
        radius_mm: 4.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    };
    let helix = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    let outline = placed_rect();
    let p = last_pass_project();
    for side in [Side::Outside, Side::Inside] {
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            for (entry, lead, relief) in [
                (ramp_15(), None, CornerRelief::None),
                (ramp_15(), Some(lead.clone()), CornerRelief::None),
                (helix.clone(), Some(lead.clone()), CornerRelief::Dogbone),
                (Entry::Plunge, None, CornerRelief::TBone),
            ] {
                let case = format!("{side:?} {direction:?} {entry:?} {lead:?} {relief:?}");
                let op = ProfileOp {
                    direction,
                    entry,
                    lead,
                    corner_relief: relief,
                    ..last_pass_op(&p, side)
                };
                let (tp, _) = generate(&p, &op);
                let split = last_pass_start(&tp, &outline, LAST_PASS_Z_MM[1], STOCK_TO_LEAVE_MM);
                let (near, _) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split])
                    .unwrap_or_else(|| panic!("{case}: the stock passes cut"));
                assert!(
                    near >= R_MM + STOCK_TO_LEAVE_MM - 1e-3,
                    "{case}: a stock pass comes {near} mm from the part"
                );
                let centre = match side {
                    Side::Inside => -R_MM,
                    _ => R_MM,
                };
                let path = offset(&Contour::new(outline.clone()).unwrap(), centre).remove(0);
                let last = &tp.moves[split..];
                for v in path.polyline().vertices() {
                    assert!(
                        last.iter().any(|m| {
                            !m.is_rapid()
                                && m.to.z.abs() <= TOL
                                && m.to.xy().distance_to(v.point()) <= 1e-6
                        }) || last.windows(2).any(|w| {
                            !w[1].is_rapid()
                                && w[0].to.z.abs() <= TOL
                                && w[1].to.z.abs() <= TOL
                                && dc_geometry::point_segment_distance_mm(
                                    dc_geometry::Segment {
                                        start: w[0].to.xy(),
                                        end: w[1].to.xy(),
                                        bulge: 0.0,
                                    },
                                    v.point(),
                                ) <= 1e-6
                        }),
                        "{case}: the last pass misses {:?} at full depth",
                        v.point()
                    );
                }
                if relief != CornerRelief::None && side == Side::Inside {
                    // A notch reaches past the part's path towards each
                    // corner, at full depth, after the stock passes.
                    let corners = outline.vertices().len();
                    let notched = outline
                        .vertices()
                        .iter()
                        .filter(|c| {
                            last.iter().any(|m| {
                                !m.is_rapid()
                                    && m.to.z.abs() <= TOL
                                    && m.to.xy().distance_to(c.point()) < R_MM * 2f64.sqrt() - 0.1
                            })
                        })
                        .count();
                    assert_eq!(notched, corners, "{case}");
                }
            }
        }
    }
}

// ── #311: the last pass on Left and Right Profiles ──

/// The 60 mm open line of #311's tests, placed from (20, 20) to
/// (80, 20).
fn line_60() -> Polyline {
    open(&[(0.0, 0.0), (60.0, 0.0)])
}

/// An open L: 60 mm along +X, then 40 mm up, a left turn. Left cuts
/// inside its corner, Right round it.
fn ell_path() -> Polyline {
    open(&[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0)])
}

/// An open V: 50 mm along +X, then 50 mm back at `angle_deg` to the
/// first arm, a left turn. Left cuts inside its tip, Right round it.
fn vee_path(angle_deg: f64) -> Polyline {
    let angle = angle_deg.to_radians();
    open(&[
        (0.0, 0.0),
        (50.0, 0.0),
        (50.0 - 50.0 * angle.cos(), 50.0 * angle.sin()),
    ])
}

/// Which way `side` puts the tool from a line drawn along +X: +1 for
/// Left (+Y), −1 for Right.
fn side_sign(side: Side) -> f64 {
    if side == Side::Left { 1.0 } else { -1.0 }
}

/// The direction that cuts `side` from the end beside the first
/// vertex: Left climb, Right conventional.
fn from_first_vertex(side: Side) -> CutDirection {
    if side == Side::Left {
        CutDirection::Climb
    } else {
        CutDirection::Conventional
    }
}

/// Acceptance 46's Profile (12 mm deep in 4 mm passes, a 0.5 mm last
/// pass stock to leave) cut on `side` with `direction`.
fn side_last_pass_op(p: &Project, side: Side, direction: CutDirection) -> ProfileOp {
    ProfileOp {
        direction,
        ..last_pass_op(p, side)
    }
}

/// A 60 mm open line cut Left, 12 mm deep in 4 mm passes with a
/// 0.5 mm stock to leave: passes 1–2 run `r + 0.5` to the left, pass 3 is
/// the plain Left path, move for move. Every pass starts and ends on
/// the normals through the line's ends (X 20 and 80): the stock to leave
/// passes are neither shorter nor longer than the last. Right is the
/// mirror, and the other direction runs each pass from the other end.
#[test]
fn last_pass_left_offsets_earlier_passes() {
    let p = last_pass_project_of(vec![line_60()]);
    for side in [Side::Left, Side::Right] {
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let case = format!("{side:?} {direction:?}");
            let op = side_last_pass_op(&p, side, direction);
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{case}: {warnings:?}");
            let (plain, _) = generate(
                &p,
                &ProfileOp {
                    last_pass: None,
                    ..op.clone()
                },
            );
            let (start, end) = if direction == from_first_vertex(side) {
                (20.0, 80.0)
            } else {
                (80.0, 20.0)
            };
            let laps = last_pass_laps(&tp);
            assert_eq!(laps.len(), 3, "{case}: {laps:?}");
            for (k, (lap, z)) in laps.iter().zip(LAST_PASS_Z_MM).enumerate() {
                assert_approx_eq!(lap.z_mm, z, TOL);
                let away_mm = if k < 2 {
                    R_MM + STOCK_TO_LEAVE_MM
                } else {
                    R_MM
                };
                let y = 20.0 + side_sign(side) * away_mm;
                for q in &lap.points {
                    assert!(
                        (q.y - y).abs() <= TOL,
                        "{case}: pass {k} at {q:?}, not Y {y}"
                    );
                }
                let (first, last) = (lap.points[0], lap.points[lap.points.len() - 1]);
                assert!(
                    (first.x - start).abs() <= TOL && (last.x - end).abs() <= TOL,
                    "{case}: pass {k} runs {first:?} to {last:?}, not X {start} to {end}"
                );
            }
            assert_eq!(
                laps[2].moves,
                laps_at(&plain, 0.0)[0].moves,
                "{case}: the plain path"
            );
        }
    }
}

/// With a ramp entry the last pass ramps. The rectangle Outside
/// and Inside and the 60 mm line Left and Right, `reverse` off and on:
/// no straight move down goes farther than the 1 mm a descent starts
/// above the floor it reaches (so none runs through the stock to leave's
/// uncut strip), the last pass ramps below the floor the earlier passes
/// cut, and every ramp move below that floor runs in their kerf, its
/// tool centre on the stock path.
#[test]
fn last_pass_uses_the_configured_entry() {
    let floor_mm = LAST_PASS_Z_MM[1];
    let slack_mm = dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM + 1e-6;
    for (side, drawn) in [
        (Side::Outside, rect_60x40()),
        (Side::Inside, rect_60x40()),
        (Side::Left, line_60()),
        (Side::Right, line_60()),
    ] {
        for reverse in [false, true] {
            let case = format!("{side:?} reverse {reverse}");
            let p = last_pass_project_of(vec![drawn.clone()]);
            let line = at_20_20().apply_polyline(&drawn).unwrap();
            let op = ProfileOp {
                entry: Entry::Ramp { angle_deg: 10.0 },
                last_pass: Some(LastPass {
                    reverse,
                    ..stock_to_leave(STOCK_TO_LEAVE_MM)
                }),
                ..last_pass_op(&p, side)
            };
            let (tp, _) = generate(&p, &op);
            let mut at = tp.moves[0].to;
            let mut ramped = false;
            for mv in &tp.moves[1..] {
                let straight_down =
                    mv.to.z < at.z - EPS_MM && at.xy().distance_to(mv.to.xy()) <= EPS_MM;
                if !mv.is_rapid() && straight_down {
                    assert!(
                        at.z - mv.to.z <= crate::entry::ENTRY_CLEARANCE_MM + TOL,
                        "{case}: a plunge from Z {} to {} at {:?}",
                        at.z,
                        mv.to.z,
                        mv.to.xy()
                    );
                }
                let ramp = !mv.is_rapid() && !straight_down && mv.to.z < at.z - EPS_MM;
                if ramp && mv.to.z < floor_mm - EPS_MM {
                    ramped = true;
                    for q in [middle(at.xy(), mv), mv.to.xy()] {
                        let gap = gap_mm(&line, q);
                        assert!(
                            (gap - (R_MM + STOCK_TO_LEAVE_MM)).abs() <= slack_mm,
                            "{case}: the ramp runs {gap} mm from the part at {q:?}"
                        );
                    }
                }
                at = mv.to;
            }
            assert!(ramped, "{case}: the last pass ramps below Z {floor_mm}");
        }
    }
}

/// #339 review, from the property test: the last pass's step is held
/// to the operation's lines, not to the entries' waste domain, which
/// leaves out every other region's kerf. A 28.9 mm disc cut Outside
/// with an 8.4 × 10 mm hole and a 2.4 × 2.3 mm part inside it, a
/// 2.1 mm bit, a ramp and a 0.25 mm stock to leave: the small part's
/// domain lies wholly beyond its own path, and its step was refused as
/// leaving the waste. It is cut, and safe.
#[test]
fn last_pass_steps_beside_a_neighbours_kerf() {
    let (a, b) = (4.212_446_961_402_92, 4.982_479_348_614_591);
    let (c, d) = (1.192_218_418_059_746, 1.166_751_264_510_846_2);
    let square = |a: f64, b: f64| {
        closed(&[
            (-a + 40.0, -b + 40.0),
            (a + 40.0, -b + 40.0),
            (a + 40.0, b + 40.0),
            (-a + 40.0, b + 40.0),
        ])
    };
    let disc = Polyline::new(
        vec![
            Vertex::new(40.0 - 14.453_060_993_366_394, 40.0, -1.0),
            Vertex::new(40.0 + 14.453_060_993_366_394, 40.0, -1.0),
        ],
        true,
    );
    let mut p = last_pass_project_of(vec![disc, square(a, b), square(c, d)]);
    p.tools[0].diameter_mm = 2.103_135_519_226_597_2;
    let op = ProfileOp {
        entry: Entry::Ramp { angle_deg: 14.0 },
        step_down_mm: Some(2.27),
        last_pass: Some(stock_to_leave(0.247_470_115_596_678_34)),
        ..last_pass_op(&p, Side::Outside)
    };
    let (tp, _) = generate(&p, &op);
    assert!(!tp.moves.is_empty());
}

/// The last pass's step across the stock to leave runs above
/// the tab material its lap leaves, and only there. A 60 mm path along
/// X raised to Z 3 from 20 to 34 mm (a rect tab over its bit's span),
/// floor 0, a 6.35 mm bit: a step across the stock to leave at X 27, over
/// the tab, must run at the tab's top; one at X 40 and one at X 37.5,
/// beside the tab's end, where every point in the cutter's reach is
/// cut to the floor by the lap, at the floor.
#[test]
fn last_pass_step_runs_over_tab_material_only() {
    let path = open(&[(0.0, 0.0), (60.0, 0.0)]);
    let layout = PathTabs::from_spans(
        60.0,
        1,
        &[crate::tabs::RaisedSpan {
            start_mm: 20.0,
            length_mm: 14.0,
            top_z_mm: 3.0,
            shape: TabShape::Rect,
            ramp_in_mm: 0.0,
            ramp_out_mm: 0.0,
        }],
        false,
    );
    let step = |x: f64| {
        last_pass::tab_clear_z_mm(
            &path,
            &layout,
            0.0,
            R_MM,
            (Point::new(x, R_MM + 0.5), Point::new(x, R_MM)),
        )
    };
    assert_approx_eq!(step(27.0), 3.0, TOL);
    assert_approx_eq!(step(40.0), 0.0, TOL);
    assert_approx_eq!(step(37.5), 0.0, TOL);
    let untabbed = PathTabs::from_spans(60.0, 0, &[], false);
    assert_approx_eq!(
        last_pass::tab_clear_z_mm(
            &path,
            &untabbed,
            0.0,
            R_MM,
            (Point::new(27.0, R_MM + 0.5), Point::new(27.0, R_MM))
        ),
        0.0,
        TOL
    );
}

/// An open path's tabs are laid on its last pass's path and
/// carried onto the stock path as on a closed shape
/// (`last_pass::twin_tabs`, the #310 review's rule: the issue's
/// "widened by a" was #284's, which it replaced). The 60 mm line with
/// a 5 mm tab 8 mm wide at its middle (X 50), requested and placed,
/// rect and smooth, Left and Right, cut from either end: the last pass
/// holds the tab top over the tab plus the bit (smooth: the bit),
/// centred on X 50, and the stock pass at Z 4, the only earlier
/// pass below the tab top, over that stretch and, rect, as far beyond
/// it as its cutter reaches the tab's corner, `√(2ar − a²)`.
#[test]
fn last_pass_left_right_tabs_cover_same_stretch() {
    const TOP_MM: f64 = 5.0;
    let p = last_pass_project_of(vec![line_60()]);
    let reach_mm = (2.0 * STOCK_TO_LEAVE_MM * R_MM - STOCK_TO_LEAVE_MM * STOCK_TO_LEAVE_MM).sqrt();
    for side in [Side::Left, Side::Right] {
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            for shape in [TabShape::Rect, TabShape::Smooth] {
                let requested = ProfileOp {
                    tabs: Some(TabSpec {
                        placement: TabPlacement::AtDistances {
                            distances_mm: vec![30.0],
                        },
                        width_mm: 8.0,
                        height_mm: TOP_MM,
                        shape,
                        on_holes: false,
                        avoid_corners: false,
                    }),
                    ..side_last_pass_op(&p, side, direction)
                };
                let mut placed = p.clone();
                placed.tabs = vec![dc_project::PlacedTab {
                    id: dc_project::TabId::new("tab1"),
                    instance: InstanceId::new("i1"),
                    profile: 0,
                    x_mm: 30.0,
                    y_mm: 0.0,
                    width_mm: 8.0,
                    height_mm: TOP_MM,
                    shape,
                    holds_slug: Some(false),
                }];
                let placed_op = ProfileOp {
                    tabs: None,
                    ..requested.clone()
                };
                for (how, project, op) in [
                    ("requested", &p, &requested),
                    ("placed", &placed, &placed_op),
                ] {
                    let case = format!("{side:?} {direction:?} {shape:?} {how}");
                    let (tp, _) = generate(project, op);
                    let final_y = 20.0 + side_sign(side) * R_MM;
                    let stock_to_leave_y = 20.0 + side_sign(side) * (R_MM + STOCK_TO_LEAVE_MM);
                    let last = held_on_line(&tp, final_y, TOP_MM);
                    assert_eq!(last.len(), 1, "{case}: the last pass rises: {last:?}");
                    let (fx1, fx2) = last[0];
                    let expected_mm = match shape {
                        TabShape::Rect => 8.0 + 6.35,
                        TabShape::Smooth => 6.35,
                    };
                    assert_approx_eq!(fx2 - fx1, expected_mm, 1e-6);
                    assert_approx_eq!((fx1 + fx2) / 2.0, 50.0, 1e-6);
                    let rough = held_on_line(&tp, stock_to_leave_y, TOP_MM);
                    assert_eq!(rough.len(), 1, "{case}: {rough:?}");
                    let (ax1, ax2) = rough[0];
                    match shape {
                        TabShape::Rect => {
                            for (rise_mm, end) in [(fx1 - ax1, "before"), (ax2 - fx2, "after")] {
                                assert!(
                                    rise_mm >= reach_mm - 0.26 && rise_mm <= reach_mm + 1.0,
                                    "{case}: raised {rise_mm} mm {end} the stretch"
                                );
                            }
                        }
                        TabShape::Smooth => {
                            assert!(ax1 <= fx1 + 1e-6 && ax2 >= fx2 - 1e-6, "{case}");
                        }
                    }
                }
            }
        }
    }
}

/// #311, the #310 review's tab check on open paths: three 8 × 6 mm
/// tabs on an L and on a 60° V, cut Left (inside the corner, where
/// the stock path runs past the corner beside the other arm's
/// tabs) and Right (round it), 12 mm deep in 2 mm steps, rect and
/// smooth, requested and placed: no earlier pass cuts below what the
/// last pass leaves standing, anywhere over a tab.
#[test]
fn last_pass_left_right_tabs_keep_off_the_tab_material() {
    for (name, path) in [("L", ell_path()), ("60° V", vee_path(60.0))] {
        let line = at_20_20().apply_polyline(&path).unwrap();
        let p = last_pass_project_of(vec![path]);
        for side in [Side::Left, Side::Right] {
            for shape in [TabShape::Rect, TabShape::Smooth] {
                let op = ProfileOp {
                    step_down_mm: Some(2.0),
                    tabs: Some(TabSpec {
                        placement: TabPlacement::Count { count: 3 },
                        width_mm: 8.0,
                        height_mm: 6.0,
                        shape,
                        on_holes: false,
                        avoid_corners: false,
                    }),
                    ..side_last_pass_op(&p, side, CutDirection::Climb)
                };
                let placed = crate::placed_tabs::place_requests(&{
                    let mut q = p.clone();
                    q.operations = vec![Operation::Profile(op.clone())];
                    q
                })
                .unwrap()
                .0;
                let placed_op = match &placed.operations[0] {
                    Operation::Profile(op) => op.clone(),
                    _ => unreachable!("a Profile"),
                };
                for (how, project, op) in [("requested", &p, &op), ("placed", &placed, &placed_op)]
                {
                    let case = format!("{name} {side:?} {shape:?} {how}");
                    let (tp, _) = generate(project, op);
                    let split = last_pass_start(&tp, &line, 2.0, STOCK_TO_LEAVE_MM);
                    let (worst_mm, standing) = earlier_cuts_into_tabs(&tp, split, 0.0);
                    assert!(
                        standing > 100,
                        "{case}: the last pass leaves tabs: {standing}"
                    );
                    assert!(
                        worst_mm <= 0.0,
                        "{case}: an earlier pass cuts {worst_mm} mm into a tab"
                    );
                }
            }
        }
    }
}

/// With `reverse` an open path's last pass runs from its other
/// end, so it mills the other way, as a closed shape's does: from X 80
/// to X 20 where the earlier passes run from X 20 to X 80. The
/// earlier passes are unchanged, and a tab 20 mm from the first
/// vertex (X 40) is held over the same stretch either way.
#[test]
fn last_pass_left_right_reverse_runs_from_the_other_end() {
    const TOP_MM: f64 = 5.0;
    let p = last_pass_project_of(vec![line_60()]);
    for side in [Side::Left, Side::Right] {
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let case = format!("{side:?} {direction:?}");
            let forward = side_last_pass_op(&p, side, direction);
            let reverse = ProfileOp {
                last_pass: Some(LastPass {
                    reverse: true,
                    ..stock_to_leave(STOCK_TO_LEAVE_MM)
                }),
                ..forward.clone()
            };
            let f = last_pass_laps(&generate(&p, &forward).0);
            let r = last_pass_laps(&generate(&p, &reverse).0);
            assert_eq!(r.len(), 3, "{case}");
            for (a, b) in f.iter().zip(&r).take(2) {
                assert_eq!(a.moves, b.moves, "{case}: the earlier passes");
            }
            let (fa, fb) = (f[2].points[0], f[2].points[f[2].points.len() - 1]);
            let (ra, rb) = (r[2].points[0], r[2].points[r[2].points.len() - 1]);
            assert!(
                ra.distance_to(fb) <= TOL && rb.distance_to(fa) <= TOL,
                "{case}: {fa:?}–{fb:?} reversed is {ra:?}–{rb:?}"
            );
            // It starts where the earlier passes end.
            let earlier_end = r[1].points[r[1].points.len() - 1];
            assert!(
                (ra.x - earlier_end.x).abs() <= TOL,
                "{case}: {ra:?} {earlier_end:?}"
            );

            let tabbed = |op: &ProfileOp| ProfileOp {
                tabs: Some(TabSpec {
                    placement: TabPlacement::AtDistances {
                        distances_mm: vec![20.0],
                    },
                    width_mm: 8.0,
                    height_mm: TOP_MM,
                    shape: TabShape::Rect,
                    on_holes: false,
                    avoid_corners: false,
                }),
                ..op.clone()
            };
            let final_y = 20.0 + side_sign(side) * R_MM;
            let held = |op: &ProfileOp| held_on_line(&generate(&p, op).0, final_y, TOP_MM);
            let (f, r) = (held(&tabbed(&forward)), held(&tabbed(&reverse)));
            assert_eq!(f.len(), 1, "{case}: {f:?}");
            assert_eq!(r.len(), 1, "{case}: {r:?}");
            assert_approx_eq!(f[0].0, r[0].0, 1e-6);
            assert_approx_eq!(f[0].1, r[0].1, 1e-6);
            assert_approx_eq!((f[0].0 + f[0].1) / 2.0, 40.0, 1e-6);
        }
    }
}

/// Every #284 refusal that applies to an open path cut on its
/// side. Each case is cut without a last pass and with 0.2 mm, and
/// refused at 0.5 mm:
///
/// - narrowness: the inside of a U 7 mm wide fits the bit but not the
///   bit and the stock to leave on both arms (7.35 mm), so the stock to leave
///   offset vanishes (`ShapeNarrowerThanStockToLeave`);
/// - corners only: a 7 × 3 mm notch the stock passes bridge, which
///   the last pass would cut whole (`ShapeNarrowerThanStockToLeave`);
/// - the thickness cap: the tip of a 20° V, whose wedge is 2.88 mm at
///   0.5 mm, more than a third of the bit (`ShapeNarrowerThanStockToLeave`);
/// - neighbours: two lines 6.85 mm apart cut towards each other, where
///   each stock pass would take the other line's skin, and a line
///   10.3 mm below another, cut towards its kept side, where the
///   stock pass would come within `r + a` of its kept strip
///   (`StockToLeaveCutsIntoKeptSide`, the lines in drawing order).
///
/// A square of four lines cut on its outside, whose lines meet at its
/// corners, is cut at 0.5 mm.
#[test]
fn last_pass_left_right_refuses_what_the_earlier_passes_cannot_follow() {
    let u = open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 7.0), (0.0, 7.0)]);
    let notch = open(&[
        (0.0, 0.0),
        (26.5, 0.0),
        (26.5, -3.0),
        (33.5, -3.0),
        (33.5, 0.0),
        (60.0, 0.0),
    ]);
    let with = |p: &Project, stock_to_leave_mm: Option<f64>| ProfileOp {
        last_pass: stock_to_leave_mm.map(stock_to_leave),
        ..side_last_pass_op(p, Side::Left, CutDirection::Climb)
    };
    for (case, path) in [("U", u), ("notch", notch), ("20° V", vee_path(20.0))] {
        let mut p = last_pass_project_of(vec![path]);
        p.instances[0].transform = Transform::translate(20.0, 40.0);
        generate(&p, &with(&p, None));
        generate(&p, &with(&p, Some(0.2)));
        let err = generate_profile(&p, &with(&p, Some(STOCK_TO_LEAVE_MM))).expect_err(case);
        let CamError::ShapeNarrowerThanStockToLeave {
            shape,
            diameter_mm,
            stock_to_leave_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected ShapeNarrowerThanStockToLeave, got {err:?}");
        };
        assert_eq!(shape.profile, 0, "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        assert_approx_eq!(*stock_to_leave_mm, STOCK_TO_LEAVE_MM, TOL);
    }
    let facing = vec![line_60(), open(&[(60.0, 6.85), (0.0, 6.85)])];
    let below_kept = vec![line_60(), open(&[(0.0, 10.3), (60.0, 10.3)])];
    for (case, lines) in [("facing", facing), ("below a kept strip", below_kept)] {
        let p = last_pass_project_of(lines);
        generate(&p, &with(&p, None));
        generate(&p, &with(&p, Some(0.2)));
        let err = generate_profile(&p, &with(&p, Some(STOCK_TO_LEAVE_MM))).expect_err(case);
        let CamError::StockToLeaveCutsIntoKeptSide {
            shape,
            other,
            diameter_mm,
            stock_to_leave_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected StockToLeaveCutsIntoKeptSide, got {err:?}");
        };
        assert_eq!((shape.profile, other.profile), (0, 1), "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        assert_approx_eq!(*stock_to_leave_mm, STOCK_TO_LEAVE_MM, TOL);
        let text = err.to_string();
        for needle in ["`o1`", "0.5 mm", "stock to leave", "6.35 mm"] {
            assert!(text.contains(needle), "{case}: {needle:?} in {text}");
        }
    }
    let corners = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)];
    let square: Vec<Polyline> = (0..4)
        .map(|k| open(&[corners[k], corners[(k + 1) % 4]]))
        .collect();
    let p = last_pass_project_of(square);
    let op = side_last_pass_op(&p, Side::Right, CutDirection::Climb);
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(last_pass_laps(&tp).len(), 4 * 3);
}

/// #311 (#324's rule on an open path): a sharp corner is cut. Inside
/// a 60° V and an L (cut Left) and round them (cut Right), at 0.2 and
/// 0.5 mm: each is generated and safe, the earlier passes keep
/// `r + a` from the line and the last pass exactly `r`, and the
/// material check ([`last_pass_removal`]) finds the last pass
/// cutting the corner's wedge (`a / sin(θ/2)` inside, `a` round it)
/// and nowhere more than the bound, at the ends included.
#[test]
fn last_pass_left_right_allows_sharp_corners() {
    let bound_mm = |a: f64| (6.35 / 3.0_f64).max(2f64.sqrt() * a + 0.05);
    for (case, path, side, corner_deg) in [
        ("60° V", vee_path(60.0), Side::Left, 60.0),
        ("60° V", vee_path(60.0), Side::Right, 180.0),
        ("L", ell_path(), Side::Left, 90.0),
        ("L", ell_path(), Side::Right, 180.0),
    ] {
        let line = at_20_20().apply_polyline(&path).unwrap();
        for a in [0.2, 0.5] {
            let case = format!("{case} {side:?} at {a} mm");
            let p = last_pass_project_of(vec![path.clone()]);
            let op = ProfileOp {
                last_pass: Some(stock_to_leave(a)),
                ..side_last_pass_op(&p, side, CutDirection::Climb)
            };
            let (tp, _) = generate(&p, &op);
            let split = last_pass_start(&tp, &line, LAST_PASS_Z_MM[1], a);
            let (near, far) = gap_range_mm(&line, tp.moves[0].to, &tp.moves[..split]).unwrap();
            assert!(
                (near - (R_MM + a)).abs() <= 1e-3 && (far - (R_MM + a)).abs() <= 1e-3,
                "{case}: an earlier pass from {near} to {far}"
            );
            let (near, far) =
                gap_range_mm(&line, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
            assert!(
                (near - R_MM).abs() <= 1e-3 && (far - R_MM).abs() <= 1e-3,
                "{case}: the last pass from {near} to {far}"
            );
            let (widest_mm, removed_mm2) = last_pass_removal(&tp, split, LAST_PASS_Z_MM[1] + 0.01);
            let wedge_mm = a / (corner_deg / 2.0_f64).to_radians().sin();
            assert!(removed_mm2 > 0.0, "{case}");
            assert!(
                widest_mm >= wedge_mm - 0.15,
                "{case}: the last pass cuts {widest_mm} mm, not the {wedge_mm} mm wedge"
            );
            assert!(
                widest_mm <= bound_mm(a) + 0.1,
                "{case}: the last pass cuts {widest_mm} mm thick, over {}",
                bound_mm(a)
            );
        }
    }
}

// ── #342: square ends of a Left or Right cut ──

/// An open path whose end segment is too short for the bit on
/// the inside of the turn before it is refused at that end, rather
/// than cut past the end normal (A8, "Ends"). A 30° V drawn down a
/// 20 mm arm and up a 2 mm one, cut inside (Left): a cut beside the
/// 2 mm arm would start `r·cot 15°` (11.8 mm) along it, so the offset
/// is the first arm's alone, and it runs to the circle round the end
/// vertex, past the normal there: `EndSegmentHasNoRoom` at vertex 2,
/// the segment 2 mm long, with no stock to leave. Drawn the other way it is
/// the first segment, refused at vertex 0 when cut inside (Right).
/// Round the V it is cut. A 30 × 3.4 mm L cut inside its corner has
/// room for the bit (`r·cot 45°` = 3.175 mm) and for the bit plus a
/// 0.2 mm stock to leave, and every lap ends square on the end normal, but
/// not for the bit plus 0.5 mm: refused with that stock to leave.
#[test]
fn left_right_end_segment_with_no_room_is_refused() {
    let climb = CutDirection::Climb;
    let (sin, cos) = 15f64.to_radians().sin_cos();
    let v = open(&[
        (-20.0 * sin, 20.0 * cos),
        (0.0, 0.0),
        (2.0 * sin, 2.0 * cos),
    ]);
    let op = |p: &Project, side: Side, stock_to_leave_mm: Option<f64>| ProfileOp {
        last_pass: stock_to_leave_mm.map(stock_to_leave),
        ..side_last_pass_op(p, side, climb)
    };
    let refused = |p: &Project, op: &ProfileOp| {
        let err = generate_profile(p, op).expect_err("no room");
        let message = err.to_string();
        let CamError::EndSegmentHasNoRoom {
            shape,
            vertex,
            length_mm,
            diameter_mm,
            stock_to_leave_mm,
            ..
        } = err
        else {
            panic!("expected EndSegmentHasNoRoom, got {err:?}");
        };
        assert_eq!(shape.profile, 0);
        assert_approx_eq!(diameter_mm, 6.35, TOL);
        (vertex, length_mm, stock_to_leave_mm, message)
    };
    for a in [None, Some(STOCK_TO_LEAVE_MM)] {
        let p = last_pass_project_of(vec![v.clone()]);
        let (vertex, length_mm, stock_to_leave_mm, message) = refused(&p, &op(&p, Side::Left, a));
        assert_eq!((vertex, stock_to_leave_mm), (2, None), "{a:?}");
        assert_approx_eq!(length_mm, 2.0, TOL);
        assert!(
            message.contains("its vertex 2 is 2.000 mm long"),
            "{message}"
        );
        generate(&p, &op(&p, Side::Right, a));
        let back = last_pass_project_of(vec![v.reversed()]);
        let (vertex, length_mm, _, _) = refused(&back, &op(&back, Side::Right, a));
        assert_eq!(vertex, 0, "{a:?}");
        assert_approx_eq!(length_mm, 2.0, TOL);
        generate(&back, &op(&back, Side::Left, a));
    }
    let ell = last_pass_project_of(vec![open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 3.4)])]);
    let (tp, _) = generate(&ell, &op(&ell, Side::Left, None));
    let square = Point::new(50.0 - R_MM, 23.4);
    for lap in laps(&tp) {
        let end = lap.points[lap.points.len() - 1];
        assert!(end.distance_to(square) <= TOL, "{end:?}");
    }
    generate(&ell, &op(&ell, Side::Left, Some(0.2)));
    let (vertex, length_mm, stock_to_leave_mm, message) =
        refused(&ell, &op(&ell, Side::Left, Some(STOCK_TO_LEAVE_MM)));
    assert_eq!((vertex, stock_to_leave_mm), (2, Some(STOCK_TO_LEAVE_MM)));
    assert_approx_eq!(length_mm, 3.4, TOL);
    assert!(
        message.contains("and its 0.5 mm last pass stock to leave"),
        "{message}"
    );
}

/// The strip a line keeps is closed at each end by the normal
/// through the end vertex, so a cut that reaches into it past the end
/// is refused even where it keeps the radius from the line and from
/// the kept edge. Placed at (20, 20), a 20 mm line from (0, 0) cut Left keeps the strip
/// below it; a 0.8 mm line at X 26.19, cut Left (toward −X), runs its
/// cut 0.95 r past the end from Y −2 to −1.2: more than `r` from the
/// end vertex and from the kept edge's end at (20, −r), yet its bit
/// reaches 0.16 mm into the strip. `CutsIntoKeptSide`, the lines in
/// drawing order. Moved 0.2 mm further off, it is cut.
#[test]
fn left_right_cut_into_the_strip_past_an_end_is_refused() {
    let lines = |x: f64| {
        vec![
            open(&[(0.0, 0.0), (20.0, 0.0)]),
            open(&[(x, -2.0), (x, -1.2)]),
        ]
    };
    let x = 20.0 + 0.95 * R_MM + R_MM;
    let p = project(lines(x), &[("i1", at_20_20())]);
    let err = generate_profile(&p, &profile_op(&p, Side::Left, CutDirection::Climb))
        .expect_err("into the strip");
    let CamError::CutsIntoKeptSide { shape, other, .. } = &err else {
        panic!("expected CutsIntoKeptSide, got {err:?}");
    };
    assert_eq!((shape.profile, other.profile), (0, 1), "{err:?}");
    let p = project(lines(x + 0.2), &[("i1", at_20_20())]);
    generate(&p, &profile_op(&p, Side::Left, CutDirection::Climb));
}

// ── #356: a later pass's ramp keeps to the trench cut below the top ──

/// The farthest point from `start` of the part of `legs` (each from
/// its first point to its second) below `top_mm`, mm.
fn reach_below_mm(start: Point, legs: &[(Point3, Point3)], top_mm: f64) -> f64 {
    let at = |p: Point| start.distance_to(p);
    legs.iter()
        .filter_map(|&(a, b)| match (a.z < top_mm, b.z < top_mm) {
            (true, true) => Some(at(a.xy()).max(at(b.xy()))),
            (false, false) => None,
            (a_below, _) => {
                let t = (top_mm - a.z) / (b.z - a.z);
                let cross = a.xy() + (b.xy() - a.xy()) * t;
                let below = if a_below { a } else { b };
                Some(at(below.xy()).max(at(cross)))
            }
        })
        .fold(0.0, f64::max)
}

/// The Profile property's `e577fed0` regression: a 1 mm deep
/// Outside Profile of a 33.77 mm circle with a small hole, ramped at
/// 1.33° by a 7.27 mm tool stepping down 0.415 mm. The first pass
/// starts its ramp 1 mm above the stock, so it cuts only 17.1 mm of
/// its 21.8 mm line below the stock top, and the second's ramp needs
/// 17.8 mm. No ramp leg of a pass runs farther from the target than
/// the part of the pass above's ramp below the stock top.
#[test]
fn later_ramp_keeps_to_the_trench_the_pass_above_cut_below_the_top() {
    let r = 16.883_035_482_589_705;
    let (hx, hy) = (3.422_910_926_112_843, 3.733_229_249_787_881);
    let top_mm = 20.195_642_689_564_75;
    let hole = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
        .into_iter()
        .map(|(x, y)| Vertex::new(x, y, 0.0))
        .collect();
    let mut p = project(
        vec![
            Polyline::new(
                vec![Vertex::new(r, 0.0, 1.0), Vertex::new(-r, 0.0, 1.0)],
                true,
            ),
            Polyline::new(hole, true),
        ],
        &[(
            "i1",
            Transform::from([
                0.737_167_684_917_281_9,
                0.675_709_852_165_628_2,
                -0.675_709_852_165_628_2,
                0.737_167_684_917_281_9,
                40.0,
                40.0,
            ]),
        )],
    );
    p.stock.thickness_mm = Some(top_mm);
    p.instances[0].z_top_mm = Some(top_mm);
    let tool = &mut p.tools[0];
    tool.diameter_mm = 7.269_778_836_713_369;
    tool.feed_mm_min = Some(3_192.045_316_616_497);
    tool.plunge_mm_min = Some(2_633.305_020_598_756_5);
    tool.step_down_mm = Some(0.415_478_759_066_539_8);
    let op = ProfileOp {
        depth_mm: Some(1.009_782_134_478_237_6),
        safe_z_mm: 1.0,
        entry: Entry::Ramp {
            angle_deg: 1.334_494_957_255_164,
        },
        ..profile_op(&p, Side::Outside, CutDirection::Climb)
    };
    let (tp, _) = generate(&p, &op);
    // Each pass's ramp: its start and its falling legs.
    let mut ramps: Vec<(Point, Vec<(Point3, Point3)>)> = Vec::new();
    let mut in_ramp = false;
    for w in tp.moves.windows(2) {
        let (from, mv) = (w[0].to, w[1]);
        let leg = !mv.is_rapid()
            && mv.to.z < from.z - EPS_MM
            && from.xy().distance_to(mv.to.xy()) > EPS_MM;
        if leg {
            if !in_ramp {
                ramps.push((from.xy(), Vec::new()));
            }
            if let Some((_, legs)) = ramps.last_mut() {
                legs.push((from, mv.to));
            }
        }
        in_ramp = leg;
    }
    assert!(ramps.len() >= 3, "a ramp per pass: {ramps:?}");
    for pair in ramps.windows(2) {
        let ((above_start, above), (start, legs)) = (&pair[0], &pair[1]);
        assert!(above_start.distance_to(*start) <= EPS_MM, "one target");
        let reach_mm = reach_below_mm(*start, above, top_mm);
        for &(a, b) in legs {
            for q in [a, b] {
                let d = start.distance_to(q.xy());
                assert!(
                    d <= reach_mm + 1e-6,
                    "a leg runs {d} mm out, past the {reach_mm} mm the pass above cut \
                         below the top"
                );
            }
        }
    }
}
