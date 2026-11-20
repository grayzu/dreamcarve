// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/entry.ts
//   src/engine/toolpaths/edge.ts

//! Tests of the descents that follow a wall: the ramp along the path,
//! and the legs that take a ramp or a helix to and from its target.

use super::*;
use crate::entry::CutSide;
use dc_geometry::{Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Vertex, assert_approx_eq, nest};
use dc_project::CutDirection;

fn tool() -> EntryTool {
    EntryTool {
        diameter_mm: 4.0,
        feed_mm_min: 800.0,
        plunge_mm_min: 200.0,
        direction: CutDirection::Conventional,
        side: CutSide::Internal,
        quantum_mm: 0.001,
    }
}

fn square_domain() -> EntryDomain {
    let c = Contour::new(Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)).unwrap();
    let (regions, _) = nest(vec![c]).unwrap();
    EntryDomain::new(&regions, 4.0, FLATTEN_TOLERANCE_DEFAULT_MM)
}

/// A wall ramp starts on its target, on the domain's edge, and runs
/// into the domain at most 3·D, stopping the safety short of the far
/// side.
#[test]
fn wall_ramp_starts_at_the_target() {
    let target = Point::new(10.0, 0.0);
    let p = plan_at_wall(
        &square_domain(),
        &Entry::Ramp { angle_deg: 15.0 },
        target,
        &tool(),
    );
    let d = p.descend(2.0, 0.0);
    assert_eq!(d.used, EntryKind::Ramp);
    assert!(d.from.distance_to(target) <= EPS_MM);
    let ramp = p.ramp.unwrap();
    assert_approx_eq!(ramp.start.distance_to(ramp.end), 12.0, 1e-9);
    assert!(ramp.end.y > 0.0, "into the square: {ramp:?}");
    let last = d.moves.last().unwrap().to;
    assert!(last.xy().distance_to(target) <= EPS_MM);
    assert_approx_eq!(last.z, 0.0, EPS_MM);
}

/// The 20 mm square cut short at `height_mm`, as a domain.
fn low_domain(height_mm: f64) -> EntryDomain {
    let c = Contour::new(Polyline::rect(Point::new(0.0, 0.0), 20.0, height_mm)).unwrap();
    let (regions, _) = nest(vec![c]).unwrap();
    EntryDomain::new(&regions, 4.0, FLATTEN_TOLERANCE_DEFAULT_MM)
}

/// A plan follows another plan's ramp line from its target as
/// far as its own domain holds it: the whole line where the domain
/// holds it all, else stopped the boundary safety short of where the
/// domain ends, and not at all when that leaves no run. The follower
/// has no helix.
#[test]
fn following_keeps_to_the_line_as_far_as_the_domain_holds_it() {
    let target = Point::new(10.0, 0.0);
    let entry = Entry::Ramp { angle_deg: 15.0 };
    let plain = plan_at_wall(&square_domain(), &entry, target, &tool());
    let (start, end) = plain.ramp_line().unwrap();
    assert!(start.distance_to(target) <= EPS_MM);
    let whole = plain.following(&square_domain(), end).unwrap();
    assert_eq!(whole.ramp_line(), plain.ramp_line());
    let low = plan_at_wall(&low_domain(7.0), &entry, target, &tool());
    assert_ne!(low.ramp_line(), plain.ramp_line(), "another line");
    let follow = low.following(&low_domain(7.0), end).unwrap();
    let (s, e) = follow.ramp_line().unwrap();
    assert!(s.distance_to(target) <= EPS_MM);
    let along = (end - start) * (1.0 / start.distance_to(end));
    assert_approx_eq!(along.cross(e - s), 0.0, 1e-9);
    assert_approx_eq!(
        s.distance_to(e),
        7.0 / along.y - boundary_safety_mm(4.0),
        1e-9
    );
    assert!(follow.helix.is_none());
    assert!(low.following(&low_domain(0.4), end).is_none());
}

/// A plan cut short keeps its ramp line from its target, ends
/// it `run_mm` along, and drops its helix; one no longer than that is
/// kept whole, and a run no longer than a ramp run may be short leaves
/// none.
#[test]
fn cut_short_keeps_to_the_line_from_the_target() {
    let target = Point::new(10.0, 0.0);
    let entry = Entry::Ramp { angle_deg: 15.0 };
    let plan = plan_at_wall(&square_domain(), &entry, target, &tool());
    let (start, end) = plan.ramp_line().unwrap();
    let length_mm = start.distance_to(end);
    assert_eq!(plan.cut_short(length_mm + 1.0), Some(plan.clone()));
    let short = plan.cut_short(length_mm / 2.0).unwrap();
    let (s, e) = short.ramp_line().unwrap();
    assert!(s.distance_to(start) <= EPS_MM);
    assert!(e.distance_to(start + (end - start) * 0.5) <= 1e-9);
    assert!(short.helix.is_none());
    assert!(plan.cut_short(0.0).is_none());
}

/// The helix rejoin leaves the target at the start Z for the helix
/// end and start, flat, then cuts the helix as placed.
#[test]
fn rejoin_reaches_the_helix_start_from_the_target() {
    let target = Point::new(10.0, 0.0);
    let p = plan_at_wall(
        &square_domain(),
        &Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        },
        target,
        &tool(),
    );
    let d = p.descend(2.0, 0.0);
    assert_eq!(d.used, EntryKind::Helix);
    let joined = p.rejoin(d.clone(), 2.0);
    assert!(joined.from.distance_to(target) <= EPS_MM);
    let extra = joined.moves.len() - d.moves.len();
    assert!((1..=2).contains(&extra), "{extra}");
    for mv in &joined.moves[..extra] {
        assert_approx_eq!(mv.to.z, 2.0, EPS_MM);
    }
    let to_start = joined.moves[extra - 1].to.xy();
    assert!(to_start.distance_to(d.from) <= EPS_MM);
    assert_eq!(&joined.moves[extra..], &d.moves[..]);
}

/// A closed path shorter than three diameters ramps round the whole
/// loop and back.
#[test]
fn along_a_short_loop_runs_the_whole_loop() {
    let square = Polyline::rect(Point::new(0.0, 0.0), 2.0, 2.0);
    let d = along_path(&square, 15.0, f64::INFINITY, 4.0, 0.0, &tool(), 0.01)
        .expect("a path with a start");
    assert_eq!(d.used, EntryKind::Ramp);
    // 8 mm loop: legs of 8 mm drop 2.14 mm, so two legs.
    let corners = d
        .moves
        .iter()
        .filter(|m| m.to.xy().distance_to(Point::new(2.0, 2.0)) <= EPS_MM)
        .count();
    assert!(corners >= 2, "out round the loop and back: {:?}", d.moves);
    let last = d.moves.last().unwrap().to;
    assert!(last.xy().distance_to(Point::new(0.0, 0.0)) <= EPS_MM);
}

/// `max_run_mm` shortens the run; under 0.02·D it falls back.
#[test]
fn along_path_stops_short_of_the_limit() {
    let line = Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(50.0, 0.0)], false);
    let d = along_path(&line, 15.0, 5.0, 2.0, 0.0, &tool(), 0.01).expect("a path with a start");
    assert!(
        d.moves.iter().all(|m| m.to.x <= 5.0 + 1e-9),
        "{:?}",
        d.moves
    );
    let d = along_path(&line, 15.0, 0.05, 2.0, 0.0, &tool(), 0.01).expect("a path with a start");
    assert_eq!(d.used, EntryKind::Plunge);
    assert_eq!(d.reason, Some(FallbackReason::NoRoom));
    assert!(d.moves.is_empty());
}

/// #234 for the On line ramp: over 4.0018 legs' depth (12 mm legs at
/// 5° along a 50 mm line), the fifth leg would fall 0.0019 mm over
/// 0.02 mm, a fragment the program writes level at the scaled feed.
/// Falling less than `RAMP_MIN_LAST_DROP_MM`, it shares the depth with
/// the fourth instead, each at 5° over 6.01 mm; no descending
/// move is shorter than a tenth of a leg or steeper than 5°, and the
/// drops sum to the depth.
#[test]
fn along_path_has_no_fragment_leg() {
    let line = Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(50.0, 0.0)], false);
    let tan = 5.0_f64.to_radians().tan();
    let depth_mm = 4.0018 * 12.0 * tan;
    let d = along_path(&line, 5.0, f64::INFINITY, depth_mm, 0.0, &tool(), 0.01)
        .expect("a path with a start");
    assert_eq!(d.used, EntryKind::Ramp);
    let mut from = Point3::new(d.from.x, d.from.y, depth_mm);
    let mut total = 0.0;
    for (i, mv) in d.moves.iter().enumerate() {
        let (run, drop) = (from.xy().distance_to(mv.to.xy()), from.z - mv.to.z);
        if drop > 0.0 {
            assert!(
                run >= 0.1 * 12.0 - 1e-9 && drop <= run * tan + 1e-9,
                "move {i} falls {drop} mm over {run} mm"
            );
            total += drop;
        }
        from = mv.to;
    }
    assert_approx_eq!(total, depth_mm, 1e-9);
}

/// One unit in the last place of an inch program with 4 decimals, mm.
const INCH_QUANTUM_MM: f64 = 0.00254;

/// #273 (the Profile property, `PROPTEST_RNG_SEED=500`): an On line
/// ramp in an inch program wrote a chord level at the plunge-limited
/// feed, which S3 refuses, because a leg ended 0.0055 mm past a chord
/// vertex and its last piece fell 0.0014 mm, under the program's last
/// place (0.00254 mm). Three such legs: cut short 0.0055 mm past a
/// vertex of a line; turning 0.002 mm past one; and on that issue's
/// 14.33 mm circle by a 1.614 mm bit at 14.3°, a depth one leg and
/// 0.004 mm deep, whose last leg run whole and shallower fell
/// under 0.001 mm a chord. Every descending move falls at least a unit
/// and runs at least √2 units in XY, so its ends round apart (as the
/// helix's); none leaves the path by more than the flatten
/// tolerance and a unit; the drops sum to the depth.
#[test]
fn along_path_writes_no_move_too_short_for_the_program() {
    let line = |x: f64| {
        Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(x, 0.0),
                Vertex::line(50.0, 0.0),
            ],
            false,
        )
    };
    let (bit, circle_bit) = (
        EntryTool {
            quantum_mm: INCH_QUANTUM_MM,
            ..tool()
        },
        EntryTool {
            diameter_mm: 1.614_062_019_946_190_8,
            quantum_mm: INCH_QUANTUM_MM,
            ..tool()
        },
    );
    let circle_r_mm = 14.332_509_386_700_824;
    let tan15 = 15.0_f64.to_radians().tan();
    let tan14 = 14.301_902_402_989_514_f64.to_radians().tan();
    let cases = [
        (line(3.0), 15.0, 3.0055 * tan15, &bit),
        (line(11.998), 15.0, 18.0 * tan15, &bit),
        (
            Polyline::circle(Point::new(0.0, 0.0), circle_r_mm),
            14.301_902_402_989_514,
            3.0 * circle_bit.diameter_mm * tan14 + 0.004,
            &circle_bit,
        ),
    ];
    for (k, (path, angle_deg, depth_mm, t)) in cases.iter().enumerate() {
        let d = along_path(path, *angle_deg, f64::INFINITY, *depth_mm, 0.0, t, 0.01)
            .expect("a path with a start");
        assert_eq!(d.used, EntryKind::Ramp, "case {k}");
        let mut from = Point3::new(d.from.x, d.from.y, *depth_mm);
        let mut total = 0.0;
        for (i, mv) in d.moves.iter().enumerate() {
            let (run, drop) = (from.xy().distance_to(mv.to.xy()), from.z - mv.to.z);
            if drop > EPS_MM {
                assert!(
                    drop >= INCH_QUANTUM_MM && run >= std::f64::consts::SQRT_2 * INCH_QUANTUM_MM,
                    "case {k}: move {i} falls {drop} mm over {run} mm"
                );
                total += drop;
            }
            if k == 2 {
                let middle = from.xy() + (mv.to.xy() - from.xy()) * 0.5;
                for p in [middle, mv.to.xy()] {
                    let off = (p.distance_to(Point::new(0.0, 0.0)) - circle_r_mm).abs();
                    assert!(
                        off <= 0.01 + INCH_QUANTUM_MM,
                        "case {k}: move {i} is {off} mm off the circle"
                    );
                }
            }
            from = mv.to;
        }
        assert_approx_eq!(total, *depth_mm, 1e-9);
        assert!(from.xy().distance_to(d.from) <= EPS_MM, "case {k}");
    }
}

/// A run whose every leg falls less than a unit (0.05 mm at 15°
/// falls 0.0134 mm, under the 0.0254 mm of a 3-decimal inch program)
/// cannot be written as a ramp; it falls back to a plunge, reason
/// `too_small`, as a helix that small does.
#[test]
fn along_path_too_small_to_write_falls_back_to_a_plunge() {
    let line = Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(50.0, 0.0)], false);
    let t = EntryTool {
        diameter_mm: 1.0,
        quantum_mm: 0.0254,
        ..tool()
    };
    let d = along_path(&line, 15.0, 0.05, 2.0, 0.0, &t, 0.01).expect("a path with a start");
    assert_eq!(d.used, EntryKind::Plunge);
    assert_eq!(d.reason, Some(FallbackReason::TooSmall));
    assert!(d.moves.is_empty());
}

/// #321 (the Profile property, `PROPTEST_RNG_SEED=72`): the first pass
/// of an On line ramp at 43.4° by a 3.455 mm bit round a 12.42 mm
/// circle, clockwise from (10.802, 6.135), 1.845 mm deep in a 3-place
/// mm program. Its one leg ends 0.00015 mm past a chord vertex; the
/// descent merges that piece, but the way back went to the
/// vertex first, a level move of 0.00015 mm the program writes as
/// nothing, which arc fitting counted as one of a fitted arc's three
/// chords and S12 did not. Every move of the ramp, down and back, runs
/// at least √2 units in XY; each stays within the flatten tolerance
/// and a unit of the circle, none goes below the pass Z, the level
/// moves run at the tool's feed and the descending ones at no more,
/// and the ramp ends at its start.
#[test]
fn along_path_returns_with_no_move_too_short_for_the_program() {
    let quantum_mm = 0.001;
    let t = EntryTool {
        diameter_mm: 3.454_973_032_185_739_3,
        feed_mm_min: 2_327.326_110_062_505,
        plunge_mm_min: 2_862.833_030_368_147,
        quantum_mm,
        ..tool()
    };
    let (sx, sy): (f64, f64) = (10.802_141_070_690_34, 6.135_438_106_759_34);
    let r_mm = sx.hypot(sy);
    let circle = Polyline::new(
        vec![Vertex::new(sx, sy, -1.0), Vertex::new(-sx, -sy, -1.0)],
        true,
    );
    let (start_z_mm, z_mm) = (10.999_327_355_218_911, 9.154_636_095_983_91);
    let d = along_path(
        &circle,
        43.395_007_100_651_696,
        f64::INFINITY,
        start_z_mm,
        z_mm,
        &t,
        0.01,
    )
    .expect("a path with a start");
    assert_eq!(d.used, EntryKind::Ramp);
    let mut from = Point3::new(d.from.x, d.from.y, start_z_mm);
    for (i, mv) in d.moves.iter().enumerate() {
        let (run, drop) = (from.xy().distance_to(mv.to.xy()), from.z - mv.to.z);
        assert!(
            run >= std::f64::consts::SQRT_2 * quantum_mm,
            "move {i} runs {run} mm: {:?}",
            d.moves
        );
        let middle = from.xy() + (mv.to.xy() - from.xy()) * 0.5;
        for p in [middle, mv.to.xy()] {
            let off = (p.distance_to(Point::new(0.0, 0.0)) - r_mm).abs();
            assert!(
                off <= 0.01 + quantum_mm,
                "move {i} is {off} mm off the circle"
            );
        }
        assert!(mv.to.z >= z_mm - EPS_MM, "move {i} goes below the pass");
        let feed = mv.feed_mm_min.unwrap();
        if drop > EPS_MM {
            assert!(feed <= t.feed_mm_min + EPS_MM, "move {i}");
        } else {
            assert_approx_eq!(feed, t.feed_mm_min, EPS_MM);
        }
        from = mv.to;
    }
    assert!(from.xy().distance_to(d.from) <= EPS_MM);
    assert_approx_eq!(from.z, z_mm, EPS_MM);
}

/// Code review finding 1.2: a path with no vertex has no On line ramp,
/// where one was once started at the origin.
#[test]
fn along_path_of_no_vertex_is_none() {
    let empty = Polyline::new(Vec::new(), false);
    let d = along_path(&empty, 15.0, f64::INFINITY, 4.0, 0.0, &tool(), 0.01);
    assert!(d.is_none(), "{d:?}");
}
