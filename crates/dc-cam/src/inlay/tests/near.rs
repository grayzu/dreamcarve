// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #464: design parts that nearly touch, as the stress tests
//! (`dc-gcode/tests/stress`) draw them: two 10 mm squares side by side, a
//! hair apart. The male's surround runs between them as a slot as wide as
//! the gap.

use super::*;

/// Two 10 mm squares `gap_mm` apart, side by side about [`CENTRE`], on
/// 100 × 100 mm stock.
fn squares_project(gap_mm: f64) -> Project {
    let square = |x: f64| polygon(&[(x, 0.0), (x + 10.0, 0.0), (x + 10.0, 10.0), (x, 10.0)]);
    project_of(
        vec![square(-10.0 - gap_mm / 2.0), square(gap_mm / 2.0)],
        CENTRE,
        100.0,
        100.0,
    )
}

/// The star part's Inlay of the squares, side `side`, the V-bit's floor
/// at step-over 0.1.
fn squares_inlay(side: InlaySide) -> InlayOp {
    InlayOp {
        step_over_fraction: Some(0.1),
        ..inlay(side)
    }
}

/// #464: the male of two squares 0.003 mm apart generates, every run
/// passing `assert_run_safe`, and the pair seats within [`SEAT_MM`] (0.05
/// mm grid). Before, it did not generate: "would cut 0.4389 mm past a
/// wall" at the floor, inside a square. The squares grown by the floor's
/// inset, `P · tan 30°` = 1.155 mm, overlap by 2.3 mm, but `nest` judged
/// them apart, so the floor kept both as holes and its rings ran between
/// them (`dc_geometry`'s `floor_of_holes_a_hair_apart_merges_them`).
#[test]
fn male_of_parts_a_hair_apart_seats() {
    let project = squares_project(0.003);
    let fit = fit_of(
        &project,
        &squares_inlay(InlaySide::Female),
        &project,
        &squares_inlay(InlaySide::Male),
        0.05,
    );
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// #479 (found by #464): two squares 0.001 mm apart seat within
/// [`SEAT_MM`] (0.05 mm grid). The plug stands its whole height, 3 mm,
/// proud on the gap: the female leaves the wall between the squares'
/// pockets, and the male cuts no groove between the squares of its plug.
/// A tip path must keep `quantum / √2` (0.0007 mm) from the walls (#469),
/// which leaves no room in a gap this narrow; 0.0015 mm seats (0.048).
#[test]
#[ignore = "#479: the male leaves near touches uncut"]
fn male_of_parts_closer_than_the_written_precision_seats() {
    let project = squares_project(0.001);
    let fit = fit_of(
        &project,
        &squares_inlay(InlaySide::Female),
        &project,
        &squares_inlay(InlaySide::Male),
        0.05,
    );
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// #479 (found by #464): a 3 mm circle 0.005 mm from a 10 mm square
/// seats within [`SEAT_MM`] (0.05 mm grid) with the 60° and 90° bits,
/// `D = min(cone / 2, 4)`, `G = D / 4`, `P = 0.4 D`. The plug stands
/// 2.062 mm (60°) and 1.191 mm (90°) proud at the pinch, where the male
/// cuts no groove; 0.0158 mm and more seat (0.012 to 0.022 mm). The
/// pinch is seven times the written-precision margin, so the medial
/// sampling may miss it too.
#[test]
#[ignore = "#479: the male leaves near touches uncut"]
fn male_of_a_pinch_seats() {
    for angle_deg in [60.0, 90.0] {
        let square = polygon(&[(0.005, -5.0), (10.005, -5.0), (10.005, 5.0), (0.005, 5.0)]);
        let mut project = project_of(
            vec![Polyline::circle(Point::new(-3.0, 0.0), 3.0), square],
            CENTRE,
            100.0,
            100.0,
        );
        project.tools[0] = v_bit(angle_deg);
        let cone_mm = 3.175 / (angle_deg / 2.0_f64).to_radians().tan();
        let depth_mm = (cone_mm / 2.0).min(4.0);
        let side = |side| InlayOp {
            step_over_fraction: Some(0.1),
            depth_mm: Some(depth_mm),
            glue_gap_mm: Some(depth_mm / 4.0),
            plug_depth_mm: Some(0.4 * depth_mm),
            ..inlay(side)
        };
        let fit = fit_of(
            &project,
            &side(InlaySide::Female),
            &project,
            &side(InlaySide::Male),
            0.05,
        );
        assert!(
            fit.is_flush(),
            "{angle_deg}°: the plug stands {:.3} mm proud at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}
