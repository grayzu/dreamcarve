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
fn squares_inlay(side: Board) -> InlayOp {
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
    let fit = fit_of(&project, &squares_inlay(Board::Male), 0.05);
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// #479 (found by #464): two squares 0.001 mm apart seat within
/// [`SEAT_MM`] (0.05 mm grid). The plug stood its whole height, 3 mm,
/// proud on the gap: the female left the wall between the squares'
/// pockets, and the male cut no groove between the squares of its plug.
/// A tip path must keep `quantum / √2` (0.0007 mm) from the walls (#469),
/// which leaves no room in a gap this narrow; 0.0015 mm seated. The gap is
/// under the merge distance (`dc_project::INLAY_MERGE_MM`, 0.05 mm), so
/// both sides now cut the squares as one shape.
#[test]
fn male_of_parts_closer_than_the_written_precision_seats() {
    let project = squares_project(0.001);
    let fit = fit_of(&project, &squares_inlay(Board::Male), 0.05);
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// #479 (found by #464): a 3 mm circle 0.005 mm from a 10 mm square
/// seats within [`SEAT_MM`] (0.05 mm grid) with the 60° and 90° bits,
/// `D = min(cone / 2, 4)`, `G = D / 4`, `P = 0.4 D`. The plug stood
/// 2.062 mm (60°) and 1.191 mm (90°) proud at the pinch, where the male
/// cut no groove, with gaps up to 0.02 mm, twice the flatten tolerance;
/// from 0.0205 mm it seats (0.023 mm at 60°). The gap is under the merge
/// distance (`dc_project::INLAY_MERGE_MM`, 0.05 mm), so both sides now
/// cut the circle and the square as one shape.
#[test]
fn male_of_a_pinch_seats() {
    for angle_deg in [60.0, 90.0] {
        let fit = pinch_fit(0.005, angle_deg);
        assert!(
            fit.is_flush(),
            "{angle_deg}°: the plug stands {:.3} mm proud at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}

/// #479: the circle and the square of [`male_of_a_pinch_seats`], merged,
/// seat with the 60° bit at gaps across the merge distance. A gap's fill
/// ends where the two outlines part, in a corner (`dc_geometry`'s
/// `merging_ends_a_gap_in_a_corner`): ended by the closing's round arc,
/// with the merge distance at 0.0214 mm, at gaps of 0.009 to 0.013 and
/// 0.019 to 0.021 mm the male's V-bit left the narrow wedge beyond the
/// arc uncut for half a millimetre, and the plug stood 0.9 to 2 mm proud.
/// 0.04 mm was not merged then, and seated.
#[test]
fn male_of_a_merged_pinch_seats_at_every_gap() {
    for gap_mm in [0.01, 0.012, 0.02, 0.04] {
        let fit = pinch_fit(gap_mm, 60.0);
        assert!(
            fit.is_flush(),
            "{gap_mm} mm: the plug stands {:.3} mm proud at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}

/// #479: a 10 mm square whose corner points at a 3 mm circle `gap_mm`
/// off it, the square turned 0.5 rad, seats with the 60° bit. With the
/// merge distance at 0.0214 mm the plug stood 0.70 mm proud at 0.016 mm
/// (merged: the wedge the closing left was too narrow for the carve to
/// reach into) and 1.05 to 1.11 mm at 0.022 to 0.035 mm (not merged);
/// 0.045 mm seated.
#[test]
fn male_of_a_corner_beside_a_curve_seats() {
    for gap_mm in [0.016, 0.03] {
        let (c, s) = (0.5_f64.cos(), 0.5_f64.sin());
        let (x, y) = (gap_mm * c, gap_mm * s);
        let square = polygon(&[
            (x, y),
            (x + 10.0 * s, y - 10.0 * c),
            (x + 10.0 * s + 10.0 * c, y - 10.0 * c + 10.0 * s),
            (x + 10.0 * c, y + 10.0 * s),
        ]);
        let circle = Polyline::circle(Point::new(-3.0 * c, -3.0 * s), 3.0);
        let fit = pair_fit(vec![circle, square], 60.0);
        assert!(
            fit.is_flush(),
            "{gap_mm} mm: the plug stands {:.3} mm proud at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}

/// The fit of a 3 mm circle `gap_mm` from a 10 mm square with the
/// V-bit of `angle_deg` ([`pair_fit`]).
fn pinch_fit(gap_mm: f64, angle_deg: f64) -> Fit {
    let square = polygon(&[
        (gap_mm, -5.0),
        (10.0 + gap_mm, -5.0),
        (10.0 + gap_mm, 5.0),
        (gap_mm, 5.0),
    ]);
    pair_fit(
        vec![Polyline::circle(Point::new(-3.0, 0.0), 3.0), square],
        angle_deg,
    )
}

/// The fit of `profiles` placed at [`CENTRE`] with the V-bit of
/// `angle_deg`, `D = min(cone / 2, 4)`, `G = D / 4`, `P = 0.4 D`,
/// step-over 0.1, no clearing.
fn pair_fit(profiles: Vec<Polyline>, angle_deg: f64) -> Fit {
    let mut project = project_of(profiles, CENTRE, 100.0, 100.0);
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
    fit_of(&project, &side(Board::Male), 0.05)
}

/// The `inlay_parts_merged` warnings of `warnings`.
fn merged_warnings(warnings: &[Warning]) -> Vec<&Warning> {
    warnings
        .iter()
        .filter(|w| w.code().as_str() == "inlay_parts_merged")
        .collect()
}

/// The design side `op` of `project` carves: the female's regions, the
/// male's mirrored design.
fn design_of(project: &Project, op: &InlayOp) -> Vec<Region> {
    let resolved = side(project, op, board_of(op), &mut Vec::new()).expect("the side resolves");
    match board_of(op) {
        Board::Female => resolved.targets.regions,
        Board::Male => resolved.design,
    }
}

/// #479: parts of an Inlay's design closer than the merge distance are
/// one shape on both sides. The squares 0.001 mm apart resolve to one
/// region on each side, each side warns `inlay_parts_merged` once, and
/// the female's pocket runs across the gap: 5 mm along the line between
/// the squares, 5 mm from the pocket's walls, the V-bit carves the floor,
/// not the wall the two pockets left there.
#[test]
fn parts_closer_than_the_merge_distance_merge_and_warn() {
    let project = squares_project(0.001);
    for side in [Board::Female, Board::Male] {
        let op = squares_inlay(side);
        assert_eq!(
            design_of(&project, &op).len(),
            1,
            "{side:?}: the squares are one shape"
        );
        let (runs, warnings) = inlay_runs(&project, &op);
        assert_eq!(
            merged_warnings(&warnings).len(),
            1,
            "{side:?}: {warnings:?}"
        );
        if side == Board::Female {
            let z_mm = crate::heightfield::carved_z_mm(
                main_run(&runs),
                Point::new(CENTRE.0, CENTRE.1 + 5.0),
                slope(),
            );
            assert!(
                z_mm < THICKNESS_MM - 3.0,
                "the gap is carved to {z_mm:.3} mm"
            );
        }
    }
}

/// #479: parts 0.1 mm apart, wider than the merge distance, are not
/// merged: the female keeps the ridge between the two pockets standing
/// to the stock top, neither side warns `inlay_parts_merged`, and the
/// pair seats within [`SEAT_MM`] (0.05 mm grid).
#[test]
fn parts_a_tenth_apart_are_not_merged() {
    let project = squares_project(0.1);
    let female = squares_inlay(Board::Female);
    let male = squares_inlay(Board::Male);
    for op in [&female, &male] {
        assert_eq!(design_of(&project, op).len(), 2, "{:?}", board_of(op));
        let (runs, warnings) = inlay_runs(&project, op);
        assert!(
            merged_warnings(&warnings).is_empty(),
            "{:?}: {warnings:?}",
            board_of(op)
        );
        if board_of(op) == Board::Female {
            let z_mm = crate::heightfield::carved_z_mm(
                main_run(&runs),
                Point::new(CENTRE.0, CENTRE.1 + 5.0),
                slope(),
            );
            assert!(
                z_mm >= THICKNESS_MM - 1e-6,
                "the ridge is cut down to {z_mm:.4} mm"
            );
        }
    }
    let fit = fit_of(&project, &male, 0.05);
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}
