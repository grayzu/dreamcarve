// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the V-Carve generator, offset strategy (#407;
//! `docs/specs/phase-3/design.md`, "Test plan → dc-cam — vcarve
//! (offset)"; functional 43, acceptance 54).
//!
//! A ring is read back from the toolpath as a closed run of level cuts at
//! one Z ([`rings_of`]); a stay-down link between rings is a cut that
//! moves in XY and descends, or a level cut that is not part of a ring.

use super::*;
use crate::pocket::{RingWall, ring_is_ccw};
use crate::vcarve::cone::point_segment;
use dc_project::{CutDirection, VCarveStrategy};

/// The offset V-Carve of every instance with the V-bit `t2`, stepping
/// `step_over_mm` and running `direction`.
fn offset_op(project: &Project, step_over_mm: f64, direction: CutDirection) -> VCarveOp {
    VCarveOp {
        strategy: VCarveStrategy::Offset {
            step_over_mm,
            direction,
        },
        ..carve_op(project)
    }
}

/// `project` with its V-bit `t2` swapped for one of `angle_deg`.
fn with_bit(mut project: Project, angle_deg: f64) -> Project {
    project.tools.retain(|t| t.id != ToolId::new("t2"));
    project.tools.push(v_bit(angle_deg));
    project
}

/// One ring read back from a toolpath: a closed loop of level cuts.
#[derive(Debug, Clone)]
struct Loop {
    /// Its Z, mm.
    z: f64,
    /// Its points in cut order, the first not repeated at the end.
    points: Vec<Point>,
}

impl Loop {
    /// Twice the signed area (shoelace), mm²: positive counter-clockwise.
    fn signed_area_mm2(&self) -> f64 {
        let n = self.points.len();
        (0..n)
            .map(|i| {
                let a = self.points[i];
                let b = self.points[(i + 1) % n];
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            / 2.0
    }

    /// Its bounding box width and height, mm.
    fn size(&self) -> (f64, f64) {
        let xs = self.points.iter().map(|p| p.x);
        let ys = self.points.iter().map(|p| p.y);
        (
            xs.clone().fold(f64::NEG_INFINITY, f64::max) - xs.fold(f64::INFINITY, f64::min),
            ys.clone().fold(f64::NEG_INFINITY, f64::max) - ys.fold(f64::INFINITY, f64::min),
        )
    }
}

/// The closed loops of level cuts in `toolpath`: a run of cutting moves at
/// one Z that move in XY, split wherever it comes back to a point it has
/// passed (so a level link into a ring is not part of the ring).
fn rings_of(toolpath: &Toolpath) -> Vec<Loop> {
    let mut out = Vec::new();
    let mut run: Vec<Point3> = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let level = from.filter(|a| {
            mv.is_cut() && (a.z - mv.to.z).abs() <= 1e-9 && a.xy().distance_to(mv.to.xy()) > EPS_MM
        });
        match level {
            Some(a) => {
                if run.is_empty() {
                    run.push(a);
                }
                let closes = run
                    .iter()
                    .position(|q| q.xy().distance_to(mv.to.xy()) <= 1e-6);
                if let Some(start) = closes {
                    out.push(Loop {
                        z: mv.to.z,
                        points: run[start..].iter().map(|q| q.xy()).collect(),
                    });
                    run = vec![mv.to];
                } else {
                    run.push(mv.to);
                }
            }
            None => run.clear(),
        }
        from = Some(mv.to);
    }
    out
}

/// The distinct depths below [`TOP_MM`] of `rings`, shallowest first,
/// merged within 0.001 mm.
fn depths_of(rings: &[Loop]) -> Vec<f64> {
    let mut depths: Vec<f64> = rings.iter().map(|r| TOP_MM - r.z).collect();
    depths.sort_by(f64::total_cmp);
    depths.dedup_by(|a, b| (*a - *b).abs() <= 1e-3);
    depths
}

/// A 16 mm square with a centred 7.6 mm square hole: a 4.2 mm wide
/// corridor all round, so the rings at insets up to 2 mm go round both
/// walls, and nothing is left at 2.5 mm (the corridor's corners are
/// 4.2 · √2 / (1 + √2) = 2.46 mm from their walls).
fn annulus() -> Project {
    with_bit(
        project(vec![
            rect(16.0, 16.0),
            Polyline::rect(Point::new(4.2, 4.2), 7.6, 7.6),
        ]),
        60.0,
    )
}

/// Two 6 mm squares joined by a neck 2 mm long and 1.2 mm wide: the rings
/// at 0.5 mm go round both, from 1 mm each square has its own.
fn dumbbell() -> Project {
    with_bit(
        project(vec![polygon(&[
            (0.0, 0.0),
            (6.0, 0.0),
            (6.0, 2.4),
            (8.0, 2.4),
            (8.0, 0.0),
            (14.0, 0.0),
            (14.0, 6.0),
            (8.0, 6.0),
            (8.0, 3.6),
            (6.0, 3.6),
            (6.0, 6.0),
            (0.0, 6.0),
        ])]),
        90.0,
    )
}

/// Whether `p` is nearer the hole of [`annulus`] than its outer wall.
fn nearer_the_hole(p: Point) -> bool {
    let hole = [
        (at(4.2, 4.2), at(11.8, 4.2)),
        (at(11.8, 4.2), at(11.8, 11.8)),
        (at(11.8, 11.8), at(4.2, 11.8)),
        (at(4.2, 11.8), at(4.2, 4.2)),
    ];
    let outer = [
        (at(0.0, 0.0), at(16.0, 0.0)),
        (at(16.0, 0.0), at(16.0, 16.0)),
        (at(16.0, 16.0), at(0.0, 16.0)),
        (at(0.0, 16.0), at(0.0, 0.0)),
    ];
    let nearest = |walls: &[(Point, Point)]| {
        walls
            .iter()
            .map(|&(a, b)| point_segment(p, a, b))
            .fold(f64::INFINITY, f64::min)
    };
    nearest(&hole) < nearest(&outer)
}

/// Acceptance 54: the 6 mm square, `step_over_mm: 0.5`, 90° bit: one
/// ring at each inset 0.5, 1, … 2.5 mm from the outline (the square
/// `6 − 2·inset` wide), at Z −0.5, −1, … −2.5 below the stock top, each
/// ring's depth equal to its inset within 0.01 mm, and nothing deeper.
#[test]
fn offset_rings_are_inset_by_their_depth() {
    let p = with_bit(project(vec![rect(6.0, 6.0)]), 90.0);
    let op = offset_op(&p, 0.5, CutDirection::Climb);
    let (toolpath, _) = carve(&p, &op);
    let regions = regions_of(&p, &op);
    let rings = rings_of(&toolpath);
    let depths = depths_of(&rings);
    assert_eq!(depths.len(), 5, "{depths:?}");
    for (k, depth_mm) in depths.iter().enumerate() {
        assert_approx_eq!(*depth_mm, 0.5 * (k as f64 + 1.0), 0.01);
    }
    assert_eq!(rings.len(), 5, "one ring per level: {rings:?}");
    for ring in &rings {
        let depth_mm = TOP_MM - ring.z;
        let (w, h) = ring.size();
        assert_approx_eq!(w, 6.0 - 2.0 * depth_mm, 0.01);
        assert_approx_eq!(h, 6.0 - 2.0 * depth_mm, 0.01);
        for &q in &ring.points {
            assert_approx_eq!(wall_distance(&regions, q), depth_mm, 0.01);
        }
    }
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 2.5, 0.01);
}

/// Functional 43: `direction` sets the way round each ring. On the
/// annulus every ring, round the outer wall and round the hole, is cut
/// climb one way and conventional the other, over the same points.
#[test]
fn offset_climb_and_conventional_reverse_every_ring() {
    let p = annulus();
    let climb = rings_of(&carve(&p, &offset_op(&p, 0.5, CutDirection::Climb)).0);
    let conventional = rings_of(&carve(&p, &offset_op(&p, 0.5, CutDirection::Conventional)).0);
    assert_eq!(
        climb.len(),
        8,
        "an outer and a hole ring at 4 levels: {climb:?}"
    );
    assert_eq!(climb.len(), conventional.len());
    let key = |r: &Loop| (r.z, r.signed_area_mm2().abs());
    let sorted = |mut rings: Vec<Loop>| {
        rings.sort_by(|a, b| {
            let (az, aa) = key(a);
            let (bz, ba) = key(b);
            az.total_cmp(&bz).then(aa.total_cmp(&ba))
        });
        rings
    };
    for (a, b) in sorted(climb).iter().zip(&sorted(conventional)) {
        assert_approx_eq!(a.z, b.z, 1e-9);
        assert_approx_eq!(a.signed_area_mm2(), -b.signed_area_mm2(), 0.01);
        assert!(a.signed_area_mm2().abs() > 1.0, "{a:?}");
        for &q in &a.points {
            assert!(
                b.points.iter().any(|r| r.distance_to(q) <= 0.01),
                "{q:?} of the climb ring is not on the conventional one"
            );
        }
    }
}

/// The ring direction is Pocket's ([`ring_is_ccw`], re-derived for Y-up
/// in phase 2): a ring along the outer wall runs as a Pocket's wall ring
/// and a ring round the hole as its island ring. In Y-up, climb runs the
/// outer wall counter-clockwise (the uncut material on the right of
/// travel) and the hole clockwise.
#[test]
fn offset_ring_direction_matches_pocket_rings() {
    let p = annulus();
    for direction in [CutDirection::Climb, CutDirection::Conventional] {
        let rings = rings_of(&carve(&p, &offset_op(&p, 0.5, direction)).0);
        let (round_hole, round_wall): (Vec<&Loop>, Vec<&Loop>) = rings
            .iter()
            .partition(|r| r.points.iter().all(|&q| nearer_the_hole(q)));
        assert_eq!(round_hole.len(), 4, "{direction:?}: {rings:?}");
        assert_eq!(round_wall.len(), 4, "{direction:?}: {rings:?}");
        for (wall, rings) in [
            (RingWall::Pocket, round_wall),
            (RingWall::Island, round_hole),
        ] {
            for ring in rings {
                assert_eq!(
                    ring.signed_area_mm2() > 0.0,
                    ring_is_ccw(wall, direction),
                    "{direction:?} {wall:?}: {ring:?}"
                );
            }
        }
    }
    assert!(ring_is_ccw(RingWall::Pocket, CutDirection::Climb));
    assert!(!ring_is_ccw(RingWall::Island, CutDirection::Climb));
}

/// Design step 8 with a flat depth: a 20 mm square, 90° bit,
/// `step_over_mm: 0.5`, flat depth 1.8 mm. The rings step in at 0.5, 1
/// and 1.5 mm and the last one is at the flat depth, inset 1.8 mm at Z
/// −1.8; nothing is cut below it; and the floor, the square inset 1.8 mm,
/// is cleared at −1.8 (#405): every point of it is within the V-bit's
/// 1.8 mm radius there (plus 0.05) of a cut at that Z.
#[test]
fn offset_stops_at_the_flat_depth_and_clears_the_floor() {
    let p = with_bit(project(vec![rect(20.0, 20.0)]), 90.0);
    let op = VCarveOp {
        flat_depth_mm: Some(1.8),
        step_over_fraction: Some(0.4),
        ..offset_op(&p, 0.5, CutDirection::Climb)
    };
    let (toolpath, _) = carve(&p, &op);
    let regions = regions_of(&p, &op);
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 1.8, 1e-6);
    let rings = rings_of(&toolpath);
    let depths = depths_of(&rings);
    assert_eq!(depths.len(), 4, "{depths:?}");
    for (depth_mm, expected_mm) in depths.iter().zip([0.5, 1.0, 1.5, 1.8]) {
        assert_approx_eq!(*depth_mm, expected_mm, 0.01);
    }
    // The ring at the flat depth runs on the floor's boundary.
    let boundary = rings
        .iter()
        .filter(|r| (TOP_MM - r.z - 1.8).abs() <= 1e-6)
        .find(|r| {
            r.points
                .iter()
                .all(|&q| (wall_distance(&regions, q) - 1.8).abs() <= 0.01)
        });
    assert!(
        boundary.is_some(),
        "no ring on the floor boundary: {rings:?}"
    );
    // The floor is cleared.
    let floor_cuts: Vec<(Point, Point)> = cuts(&toolpath)
        .into_iter()
        .filter(|(a, b)| (TOP_MM - a.z - 1.8).abs() <= 1e-6 && (TOP_MM - b.z - 1.8).abs() <= 1e-6)
        .map(|(a, b)| (a.xy(), b.xy()))
        .collect();
    for i in 0..=65 {
        for j in 0..=65 {
            let q = at(
                1.8 + 16.4 * f64::from(i) / 65.0,
                1.8 + 16.4 * f64::from(j) / 65.0,
            );
            let reach_mm = floor_cuts
                .iter()
                .map(|&(a, b)| point_segment(q, a, b))
                .fold(f64::INFINITY, f64::min);
            assert!(
                reach_mm <= 1.8 + 0.05,
                "floor point {q:?} is {reach_mm} mm from every cut at the flat depth"
            );
        }
    }
}

/// Security review H2: every cut of the offset carve, rings and the links
/// between them, keeps its cone inside the walls as drawn at its own
/// depth, measured by the test at 64 points along it (less the 0.01 mm the
/// outlines are flattened to). Links stay down where they pass: the 6 mm
/// square's rings are joined along the corner bisectors, so it is one
/// plunge. Where they would not, the tool retracts: the dumbbell's two
/// squares, apart from 1 mm in, are each entered by a plunge of their
/// own, never joined across the 1.2 mm neck.
#[test]
fn offset_ring_links_pass_the_cone_test_or_retract() {
    let fixtures = [
        ("square", with_bit(project(vec![rect(6.0, 6.0)]), 90.0)),
        ("annulus", annulus()),
        ("dumbbell", dumbbell()),
        (
            "circle",
            project(vec![Polyline::circle(Point::new(3.0, 3.0), 3.0)]),
        ),
    ];
    let mut links = 0;
    for (name, p) in &fixtures {
        let op = offset_op(p, 0.5, CutDirection::Climb);
        let (toolpath, _) = carve(p, &op);
        let regions = regions_of(p, &op);
        let tool = p.tools.iter().find(|t| t.id == op.tool).expect("the bit");
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            panic!("a V-bit");
        };
        let slope = slope_of(included_angle_deg);
        for (a, b) in xy_cuts(&toolpath) {
            if (a.z - b.z).abs() > EPS_MM {
                links += 1;
            }
            for k in 0..=64 {
                let t = f64::from(k) / 64.0;
                let q = Point3::new(
                    a.x + (b.x - a.x) * t,
                    a.y + (b.y - a.y) * t,
                    a.z + (b.z - a.z) * t,
                );
                let room_mm = wall_distance(&regions, q.xy()) - (TOP_MM - q.z) * slope;
                assert!(
                    room_mm >= -0.01,
                    "{name}: cut {a:?} -> {b:?} runs {} mm below the V surface at {q:?}",
                    -room_mm
                );
            }
        }
        match *name {
            "square" => assert_eq!(plunges(&toolpath).len(), 1, "{toolpath:?}"),
            "dumbbell" => {
                let entered: Vec<Point> =
                    plunges(&toolpath).iter().map(|(_, m)| m.to.xy()).collect();
                assert!(
                    entered.iter().any(|q| q.x < at(6.0, 0.0).x)
                        && entered.iter().any(|q| q.x > at(8.0, 0.0).x),
                    "each square is entered by a plunge of its own: {entered:?}"
                );
                // No cut below 0.6 mm crosses the neck.
                for (a, b) in xy_cuts(&toolpath) {
                    let deep = TOP_MM - a.z.max(b.z) > 0.6 + 0.01;
                    let crosses = (a.x - at(7.0, 0.0).x) * (b.x - at(7.0, 0.0).x) < 0.0;
                    assert!(!(deep && crosses), "{a:?} -> {b:?} crosses the neck");
                }
            }
            _ => {}
        }
    }
    assert!(links > 0, "no ring is linked tool-down");
}

/// Design step 8: `step_over_mm` must be positive and leave at most
/// `MAX_RINGS` rings a region; the deepest ring is held to the cone
/// (step 3), so a 12 mm square's 4.5 mm deep ring with the 90° bit
/// (cone 3.175 mm) is `v_bit_cone_exceeded`.
#[test]
fn offset_refuses_a_bad_step_over_and_a_ring_past_the_cone() {
    let p = with_bit(project(vec![rect(6.0, 6.0)]), 90.0);
    for step_over_mm in [0.0, -0.5, f64::NAN, f64::INFINITY, 1e-7] {
        let op = offset_op(&p, step_over_mm, CutDirection::Climb);
        match try_carve(&p, &op) {
            Err(CamError::InvalidVCarveStepOver {
                step_over_mm: s, ..
            }) => {
                assert!(
                    s.is_nan() == step_over_mm.is_nan(),
                    "{s} for {step_over_mm}"
                );
            }
            other => {
                panic!("step-over {step_over_mm}: expected InvalidVCarveStepOver, got {other:?}")
            }
        }
    }
    let big = with_bit(project(vec![rect(12.0, 12.0)]), 90.0);
    match try_carve(&big, &offset_op(&big, 0.5, CutDirection::Climb)) {
        Err(CamError::VBitConeExceeded {
            depth_mm, cone_mm, ..
        }) => {
            assert_approx_eq!(depth_mm, 5.5, 0.01);
            assert_approx_eq!(cone_mm, 3.175, 1e-9);
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }
}
