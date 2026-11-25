// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/replay.test.ts

//! Tests of the cut. Two are ports of `replay.test.ts`
//! (`testKernelMatchesReferenceProfile`,
//! `testDirtyRegionCoversChangedCells`), on upstream's grid of 60 × 50
//! cells of 0.5 with the stock 6 thick; upstream's fixture has its top
//! at 0 and its bottom at −6, here the bottom is 0 and the top 6. The
//! rest are the phase 6 acceptance tests 132 to 136.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code reports failure by panicking"
)]

use super::*;
use crate::grid::{CellSpec, Grid};
use dc_geometry::assert_approx_eq;

fn stock(width_mm: f64, height_mm: f64, cell_mm: f64, top_mm: f64) -> Heightfield {
    let grid = Grid::of(width_mm, height_mm, CellSpec::CellMm(cell_mm)).unwrap();
    Heightfield::uncut(grid, top_mm).unwrap()
}

fn each_cell(field: &Heightfield) -> impl Iterator<Item = (u32, u32, Point, f64)> + '_ {
    let grid = *field.grid();
    (0..grid.ny()).flat_map(move |j| {
        (0..grid.nx()).map(move |i| (i, j, grid.centre(i, j), field.z_mm(i, j).unwrap()))
    })
}

/// The height `moves` leave over `q` by trying `samples` poses of each:
/// the reference the kernel is held to. Never above the stock's top or
/// below its bottom.
fn brute_mm(moves: &[SimMove], cutter: &Cutter, q: Point, top_mm: f64, samples: u32) -> f64 {
    let mut low = top_mm;
    for mv in moves.iter().filter(|m| m.cuts()) {
        for k in 0..=samples {
            let p = mv.point_at(f64::from(k) / f64::from(samples));
            if let Some(h) = cutter.height_mm(q.distance_to(p.xy())) {
                low = low.min(p.z + h);
            }
        }
    }
    low.max(0.0)
}

/// `cutter` with its radius changed by `by_mm`.
fn widened(cutter: &Cutter, by_mm: f64) -> Cutter {
    match *cutter {
        Cutter::Flat { radius_mm } => Cutter::Flat {
            radius_mm: radius_mm + by_mm,
        },
        Cutter::Vee { slope, radius_mm } => Cutter::Vee {
            slope,
            radius_mm: radius_mm + by_mm,
        },
        Cutter::Ball { .. } | Cutter::TaperedBall { .. } => {
            panic!("the rim test is for a flat end and a V")
        }
    }
}

/// Holds every cell of `field` to the brute force of `moves`, between
/// the heights a cutter `slack_mm` wider and narrower leaves, so that a
/// cell on the rim, which sampling cannot settle, passes either way.
fn assert_matches_brute(
    field: &Heightfield,
    moves: &[SimMove],
    cutter: &Cutter,
    slack_mm: f64,
    samples: u32,
) {
    let top_mm = field.top_z_mm();
    let mut cut = 0;
    for (i, j, q, z) in each_cell(field) {
        let low = brute_mm(moves, &widened(cutter, slack_mm), q, top_mm, samples);
        let high = brute_mm(moves, &widened(cutter, -slack_mm), q, top_mm, samples);
        assert!(
            z >= low - slack_mm && z <= high + slack_mm,
            "cell {i}, {j} at {q:?}: {z}, want {low} to {high}, {cutter:?}"
        );
        cut += usize::from(z < top_mm);
    }
    assert!(cut > 50, "only {cut} cells cut");
}

fn distance_to_segment_mm(q: Point, a: Point3, b: Point3) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    q.distance_to(Point::new(a.x + dx * t, a.y + dy * t))
}

/// Acceptance 132: one straight cut of a 6 mm end mill, 3 mm deep. Every
/// cell whose centre is within 3 mm of the move is 3 mm below the top,
/// every other cell is at the top, and the same holds at any angle.
#[test]
fn a_flat_end_mill_cuts_a_slot_at_any_angle() {
    let cutter = Cutter::Flat { radius_mm: 3.0 };
    for (a, b) in [
        (Point3::new(5.0, 15.0, 9.0), Point3::new(35.0, 15.0, 9.0)),
        (Point3::new(8.0, 6.0, 9.0), Point3::new(30.0, 24.0, 9.0)),
        (Point3::new(20.1, 4.0, 9.0), Point3::new(20.1, 26.0, 9.0)),
        (Point3::new(31.0, 5.0, 9.0), Point3::new(9.0, 25.3, 9.0)),
    ] {
        let mut field = stock(40.0, 30.0, 0.25, 12.0);
        let stamp = field.cut(&SimMove::cut(a, b, 1.0), (0.0, 1.0), &cutter, 0);
        assert!(stamp.changed.is_some());
        for (i, j, q, z) in each_cell(&field) {
            let d = distance_to_segment_mm(q, a, b);
            if d < 3.0 - 1e-6 {
                assert_approx_eq!(z, 9.0, 0.0);
                assert_eq!(field.cut_by(i, j), Some(0));
            } else if d > 3.0 + 1e-6 {
                assert_approx_eq!(z, 12.0, 0.0);
                assert_eq!(field.cut_by(i, j), None, "cell {i}, {j}, {d} mm off");
            }
        }
    }
}

/// Acceptance 133: the same cut with a 6 mm ball nose leaves the arc
/// `z = tip + r − √(r² − d²)` across the move at every cell.
#[test]
fn a_ball_nose_leaves_its_arc() {
    let cutter = Cutter::Ball { radius_mm: 3.0 };
    let (a, b) = (Point3::new(5.0, 15.1, 9.0), Point3::new(35.0, 15.1, 9.0));
    let mut field = stock(40.0, 30.0, 0.25, 12.0);
    field.cut(&SimMove::cut(a, b, 1.0), (0.0, 1.0), &cutter, 0);
    let mut seen = 0;
    for (_, _, q, z) in each_cell(&field) {
        let d = distance_to_segment_mm(q, a, b);
        if d < 3.0 - 1e-6 {
            assert_approx_eq!(z, 9.0 + 3.0 - (9.0 - d * d).sqrt(), 1e-9);
            seen += 1;
        } else if d > 3.0 + 1e-6 {
            assert_approx_eq!(z, 12.0, 0.0);
        }
    }
    assert!(seen > 1000);
}

/// Acceptance 134: a 90° V-bit 2 mm deep leaves `z = tip + d` out to
/// `d = 2 mm`, where its cone leaves the stock; one deeper than its cone
/// leaves a vertical wall at its diameter.
#[test]
fn a_v_bit_leaves_its_cone_and_a_wall_past_it() {
    let (a, b) = (Point3::new(5.0, 15.1, 10.0), Point3::new(35.0, 15.1, 10.0));
    let mut shallow = stock(40.0, 30.0, 0.25, 12.0);
    let wide = Cutter::Vee {
        slope: 1.0,
        radius_mm: 3.0,
    };
    shallow.cut(&SimMove::cut(a, b, 1.0), (0.0, 1.0), &wide, 0);
    for (_, _, q, z) in each_cell(&shallow) {
        let d = distance_to_segment_mm(q, a, b);
        assert_approx_eq!(z, (10.0 + d).min(12.0), 1e-9);
    }

    // A V-bit of 4 mm diameter, its tip 4 mm down: the cone is 2 mm high.
    let narrow = Cutter::Vee {
        slope: 1.0,
        radius_mm: 2.0,
    };
    let (a, b) = (Point3::new(5.0, 15.1, 8.0), Point3::new(35.0, 15.1, 8.0));
    let mut deep = stock(40.0, 30.0, 0.25, 12.0);
    deep.cut(&SimMove::cut(a, b, 1.0), (0.0, 1.0), &narrow, 0);
    for (_, _, q, z) in each_cell(&deep) {
        let d = distance_to_segment_mm(q, a, b);
        if d < 2.0 - 1e-6 {
            assert_approx_eq!(z, 8.0 + d, 1e-9);
        } else if d > 2.0 + 1e-6 {
            assert_approx_eq!(z, 12.0, 0.0);
        }
    }
    // The wall: the last cell inside is 2 mm below the first outside.
    let at = |y: f64| {
        let (i, j) = deep.grid().cell_at(Point::new(20.0, y)).unwrap();
        deep.z_mm(i, j).unwrap()
    };
    assert!(at(15.1 + 1.7) < 10.0 && at(15.1 + 2.2) > 11.99);
}

/// Acceptance 135: a tapered ball nose's cut is its ball, then its cone,
/// then the wall at its cutting diameter, at the heights phase 3's
/// profile gives.
#[test]
fn a_tapered_ball_nose_leaves_ball_cone_and_wall() {
    let cutter = Cutter::TaperedBall {
        tip_radius_mm: 0.381,
        slope: 6.7_f64.to_radians().tan(),
        radius_mm: 3.175,
    };
    let (a, b) = (Point3::new(3.0, 5.03, 2.0), Point3::new(17.0, 5.03, 2.0));
    let mut field = stock(20.0, 10.0, 0.05, 30.0);
    field.cut(&SimMove::cut(a, b, 1.0), (0.0, 1.0), &cutter, 0);
    let (mut in_ball, mut on_cone, mut outside) = (0, 0, 0);
    for (_, _, q, z) in each_cell(&field) {
        let d = distance_to_segment_mm(q, a, b);
        if d < 3.175 - 1e-6 {
            assert_approx_eq!(z, 2.0 + cutter.height_mm(d).unwrap(), 1e-9);
            if d < 0.37 {
                in_ball += 1;
            } else {
                on_cone += 1;
            }
        } else if d > 3.175 + 1e-6 {
            assert_approx_eq!(z, 30.0, 0.0);
            outside += 1;
        }
    }
    assert!(in_ball > 100 && on_cone > 1000 && outside > 1000);
}

/// Upstream's moves, in this frame: a diagonal ramp, an axis-aligned
/// cut, a plunge, a short lead and a cut below the stock's bottom.
fn test_moves() -> Vec<SimMove> {
    let cut = |a: (f64, f64, f64), b: (f64, f64, f64)| {
        SimMove::cut(Point3::new(a.0, a.1, a.2), Point3::new(b.0, b.1, b.2), 1.0)
    };
    vec![
        cut((4.0, 4.0, 6.0), (20.0, 14.0, 2.5)),
        cut((5.2, 18.1, 3.0), (24.7, 18.1, 3.0)),
        cut((24.3, 6.2, 7.0), (24.3, 6.2, 1.5)),
        cut((12.1, 21.3, 4.0), (13.4, 22.6, 4.0)),
        cut((6.3, 9.4, -1.0), (9.8, 12.2, -1.0)),
    ]
}

/// Ported from `testKernelMatchesReferenceProfile`: the kernel equals a
/// reference loop on the cutter's profile for a flat end mill of radius
/// 1.5, a ball of 2, a V-bit of 3 at 60° and at 90° and a drill of 1.25,
/// and cuts more than 50 cells. Upstream's reference reads the tip at
/// the pose nearest the cell, which is right for its level moves and its
/// plunge; here the reference tries every pose, so the ramp is held to
/// the lowest (acceptance 136). A cut below the bottom leaves the bottom.
#[test]
fn the_kernel_matches_the_reference_profile() {
    let cutters = [
        Cutter::Flat { radius_mm: 1.5 },
        Cutter::Ball { radius_mm: 2.0 },
        Cutter::Vee {
            slope: 30.0_f64.to_radians().tan(),
            radius_mm: 3.0,
        },
        Cutter::Vee {
            slope: 1.0,
            radius_mm: 3.0,
        },
        Cutter::Flat { radius_mm: 1.25 },
    ];
    let moves = test_moves();
    for cutter in cutters {
        let mut field = stock(30.0, 25.0, 0.5, 6.0);
        assert_eq!((field.grid().nx(), field.grid().ny()), (60, 50));
        for mv in &moves {
            field.cut(mv, (0.0, 1.0), &cutter, 0);
        }
        let mut cut = 0;
        for (i, j, q, z) in each_cell(&field) {
            let want = brute_mm(&moves, &cutter, q, 6.0, 2000);
            // The brute force is never below the truth, and above it by
            // no more than its step along a move changes a height (under
            // 0.01 mm a step, times the steepest surface's 1.9), unless
            // the cell is on a flat cutter's rim.
            let on_rim = matches!(cutter, Cutter::Flat { .. })
                && (brute_mm(&moves, &widened(&cutter, 0.02), q, 6.0, 2000)
                    - brute_mm(&moves, &widened(&cutter, -0.02), q, 6.0, 2000))
                .abs()
                    > 1e-9;
            assert!(z >= 0.0, "cell {i}, {j} is below the bottom: {z}");
            if !on_rim {
                assert!(
                    z <= want + 1e-9 && z >= want - 2e-2,
                    "cell {i}, {j} at {q:?}: {z}, want {want}, {cutter:?}"
                );
            }
            cut += usize::from(z < 6.0);
        }
        assert!(cut > 50, "{cutter:?} cut only {cut} cells");
        // The cut below the bottom went through.
        let (i, j) = field.grid().cell_at(Point::new(8.0, 10.8)).unwrap();
        assert!(field.is_through(i, j), "{cutter:?}");
    }
}

/// Ported from `testDirtyRegionCoversChangedCells`: every cell a move
/// changes is inside the rectangle it reports, and the rectangle is no
/// larger than the changed cells need.
#[test]
fn the_changed_rectangle_holds_every_changed_cell() {
    let cutter = Cutter::Ball { radius_mm: 2.0 };
    let mut field = stock(30.0, 25.0, 0.5, 6.0);
    for mv in test_moves() {
        let before = field.clone();
        let stamp = field.cut(&mv, (0.0, 1.0), &cutter, 0);
        let changed: Vec<(u32, u32)> = each_cell(&field)
            .filter(|&(i, j, _, z)| z < before.z_mm(i, j).unwrap())
            .map(|(i, j, _, _)| (i, j))
            .collect();
        let rect = stamp.changed.expect("every test move cuts something");
        assert!(!changed.is_empty());
        for &(i, j) in &changed {
            assert!(rect.contains(i, j), "cell {i}, {j} outside {rect:?}");
        }
        let tight = CellRect {
            i0: changed.iter().map(|c| c.0).min().unwrap(),
            i1: changed.iter().map(|c| c.0).max().unwrap() + 1,
            j0: changed.iter().map(|c| c.1).min().unwrap(),
            j1: changed.iter().map(|c| c.1).max().unwrap() + 1,
        };
        assert_eq!(rect, tight);
        assert!(stamp.tested >= changed.len() as u64);
    }
    // A second pass over the same moves changes nothing.
    for mv in test_moves() {
        assert_eq!(field.cut(&mv, (0.0, 1.0), &cutter, 1).changed, None);
    }
    assert!(field.owners().iter().all(|&o| o != 1));
}

/// A move that misses the grid, runs at or above the stock's top, or
/// does not cut, reads no cell. Upstream scans the clamped border cells
/// of a move off the grid (the survey, section 5.2).
#[test]
fn a_move_off_the_grid_or_above_the_stock_reads_no_cell() {
    let cutter = Cutter::Flat { radius_mm: 3.0 };
    let mut field = stock(30.0, 25.0, 0.5, 6.0);
    let p = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
    for mv in [
        SimMove::cut(p(-20.0, 5.0, 2.0), p(-10.0, 5.0, 2.0), 1.0),
        SimMove::cut(p(5.0, 40.0, 2.0), p(25.0, 40.0, 2.0), 1.0),
        SimMove::cut(p(5.0, 5.0, 6.0), p(25.0, 5.0, 6.0), 1.0),
        SimMove::cut(p(5.0, 5.0, 9.0), p(25.0, 5.0, 7.0), 1.0),
        SimMove::travel(p(5.0, 5.0, 2.0), p(25.0, 5.0, 2.0), 1.0),
        SimMove::cut(p(5.0, 5.0, f64::NAN), p(25.0, 5.0, 2.0), 1.0),
    ] {
        let stamp = field.cut(&mv, (0.0, 1.0), &cutter, 0);
        assert_eq!(stamp, Stamp::default(), "{mv:?}");
    }
    assert!(field.heights().iter().all(|&z| z >= 6.0));
    // A V-bit above the stock by less than its cone's height still
    // reaches no cell: its reach is the cone's radius at the top.
    let v = Cutter::Vee {
        slope: 1.0,
        radius_mm: 3.0,
    };
    let stamp = field.cut(
        &SimMove::cut(p(5.0, 5.0, 6.5), p(25.0, 5.0, 6.5), 1.0),
        (0.0, 1.0),
        &v,
        0,
    );
    assert_eq!(stamp, Stamp::default());
}

/// A level arc is cut at its true distance: cells under a flat cutter
/// are at the arc's depth, cells clear of it are untouched, and a V
/// leaves its cone about the arc; clockwise and anticlockwise, short and
/// long ways round.
#[test]
fn a_level_arc_is_cut_at_its_true_distance() {
    let centre = Point::new(15.0, 12.0);
    let on = |deg: f64, z: f64| {
        let a = deg.to_radians();
        Point3::new(centre.x + 7.0 * a.cos(), centre.y + 7.0 * a.sin(), z)
    };
    for (from_deg, to_deg, turn) in [
        (10.0, 100.0, Turn::Ccw),
        (10.0, 100.0, Turn::Cw),
        (200.0, 40.0, Turn::Ccw),
        (-30.0, 150.0, Turn::Cw),
    ] {
        let mv = SimMove::cut(on(from_deg, 3.0), on(to_deg, 3.0), 1.0).along(centre, turn);
        for cutter in [
            Cutter::Flat { radius_mm: 1.5 },
            Cutter::Vee {
                slope: 1.0,
                radius_mm: 2.5,
            },
        ] {
            let mut field = stock(30.0, 25.0, 0.5, 6.0);
            field.cut(&mv, (0.0, 1.0), &cutter, 0);
            assert_matches_brute(&field, &[mv], &cutter, 6e-3, 2000);
        }
    }
}

/// Acceptance 136: an arc whose Z changes is cut within a micrometre of
/// the arc, as chords; two half turns that fall make a round hole.
#[test]
fn a_helix_is_cut_within_a_micron_and_leaves_a_round_hole() {
    let centre = Point::new(10.0, 10.0);
    let (east, west) = (Point::new(11.5, 10.0), Point::new(8.5, 10.0));
    let at = |p: Point, z: f64| Point3::new(p.x, p.y, z);
    let moves = [
        SimMove::cut(at(east, 6.0), at(west, 4.5), 1.0).along(centre, Turn::Ccw),
        SimMove::cut(at(west, 4.5), at(east, 3.0), 1.0).along(centre, Turn::Ccw),
        // A level turn at the bottom, as a Bore ends.
        SimMove::cut(at(east, 3.0), at(west, 3.0), 1.0).along(centre, Turn::Ccw),
        SimMove::cut(at(west, 3.0), at(east, 3.0), 1.0).along(centre, Turn::Ccw),
    ];
    let cutter = Cutter::Flat { radius_mm: 2.0 };
    let mut field = stock(20.0, 20.0, 0.1, 6.0);
    for mv in &moves {
        field.cut(mv, (0.0, 1.0), &cutter, 0);
    }
    // The hole is round: 3.5 mm from the centre, at the helix's floor.
    for (_, _, q, z) in each_cell(&field) {
        let d = q.distance_to(centre);
        if d < 3.5 - 1e-3 {
            assert_approx_eq!(z, 3.0, 1e-9);
        } else if d > 3.5 + 1e-3 {
            assert_approx_eq!(z, 6.0, 0.0);
        }
    }
    // The falling turns alone, against the arc itself.
    let mut ramp = stock(20.0, 20.0, 0.25, 6.0);
    for mv in moves.iter().take(2) {
        ramp.cut(mv, (0.0, 1.0), &cutter, 0);
    }
    assert_matches_brute(&ramp, moves.get(..2).unwrap(), &cutter, 4e-3, 3000);
    // An arc that ends where it starts sweeps nothing, as the engine
    // reads one, and with no change in Z it cuts only where it stands.
    let mut point = stock(20.0, 20.0, 0.1, 6.0);
    let still = SimMove::cut(at(east, 3.0), at(east, 3.0), 1.0).along(centre, Turn::Cw);
    point.cut(&still, (0.0, 1.0), &cutter, 0);
    for (_, _, q, z) in each_cell(&point) {
        let d = q.distance_to(east);
        assert!(
            (d < 2.0 - 1e-6 && z < 3.0 + 1e-9)
                || (d > 2.0 + 1e-6 && z >= 6.0)
                || (d - 2.0).abs() <= 1e-6,
            "{q:?}: {z}"
        );
    }
}

/// Acceptance 140's ground: a part of a move is cut to where the tip
/// is, and the parts together leave what the whole move leaves.
#[test]
fn a_part_of_a_move_is_cut_to_where_the_tip_is() {
    let cutter = Cutter::Flat { radius_mm: 2.0 };
    let (a, b) = (Point3::new(5.0, 10.1, 3.0), Point3::new(25.0, 10.1, 3.0));
    let mv = SimMove::cut(a, b, 1.0);
    let mut part = stock(30.0, 20.0, 0.25, 6.0);
    part.cut(&mv, (0.0, 0.4), &cutter, 0);
    let tip = mv.point_at(0.4);
    assert_approx_eq!(tip.x, 13.0, 1e-12);
    for (_, _, q, z) in each_cell(&part) {
        let d = distance_to_segment_mm(q, a, tip);
        if d < 2.0 - 1e-6 {
            assert_approx_eq!(z, 3.0, 0.0);
        } else if d > 2.0 + 1e-6 {
            assert_approx_eq!(z, 6.0, 0.0);
        }
    }
    part.cut(&mv, (0.4, 0.75), &cutter, 0);
    part.cut(&mv, (0.75, 1.0), &cutter, 0);
    let mut whole = stock(30.0, 20.0, 0.25, 6.0);
    whole.cut(&mv, (0.0, 1.0), &cutter, 0);
    assert_eq!(part.heights(), whole.heights());
    // A span that runs backwards, or holds nothing, cuts nothing.
    let mut none = stock(30.0, 20.0, 0.25, 6.0);
    assert_eq!(none.cut(&mv, (0.6, 0.2), &cutter, 0), Stamp::default());
}

/// A long diagonal is cut in pieces, so it tests far fewer cells than
/// its box holds, and leaves what one cut of it leaves.
#[test]
fn a_long_diagonal_is_cut_in_pieces() {
    let cutter = Cutter::Ball { radius_mm: 1.0 };
    let mv = SimMove::cut(
        Point3::new(2.0, 2.0, 5.0),
        Point3::new(98.0, 98.0, 3.0),
        1.0,
    );
    let mut field = stock(100.0, 100.0, 0.25, 6.0);
    let stamp = field.cut(&mv, (0.0, 1.0), &cutter, 0);
    let box_cells = 400 * 400;
    assert!(
        stamp.tested < box_cells / 8,
        "{} cells tested of a box of {box_cells}",
        stamp.tested
    );
    // Against the closed form on the whole move, cell by cell.
    for (i, j, q, z) in each_cell(&field) {
        let want = cutter
            .low_over_mm(mv.from, mv.to, q)
            .map_or(6.0, |z| z.min(6.0));
        assert_approx_eq!(z, want, 1e-9);
        assert_eq!(field.cut_by(i, j).is_some(), want < 6.0);
    }
}

#[test]
fn a_moves_length_and_points() {
    let line = SimMove::cut(Point3::new(0.0, 0.0, 4.0), Point3::new(3.0, 4.0, 4.0), 1.0);
    assert_approx_eq!(line.length_mm(), 5.0, 1e-12);
    let mid = line.point_at(0.5);
    assert_approx_eq!(mid.x, 1.5, 1e-12);
    assert_approx_eq!(mid.y, 2.0, 1e-12);
    // A half turn of radius 2 that falls 3: a helix √((2π)² + 3²) long.
    let centre = Point::new(5.0, 5.0);
    let arc = SimMove::cut(Point3::new(7.0, 5.0, 6.0), Point3::new(3.0, 5.0, 3.0), 1.0)
        .along(centre, Turn::Ccw);
    assert_approx_eq!(arc.length_mm(), (TAU * TAU + 9.0).sqrt(), 1e-9);
    // Half way round an anticlockwise half turn from the east is north.
    let top = arc.point_at(0.5);
    assert_approx_eq!(top.x, 5.0, 1e-9);
    assert_approx_eq!(top.y, 7.0, 1e-9);
    assert_approx_eq!(top.z, 4.5, 1e-12);
    // Clockwise it is south.
    let bottom = SimMove {
        arc: Some(SimArc {
            centre,
            turn: Turn::Cw,
        }),
        ..arc
    }
    .point_at(0.5);
    assert_approx_eq!(bottom.y, 3.0, 1e-9);
}
