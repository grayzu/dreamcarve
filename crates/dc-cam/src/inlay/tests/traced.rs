// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #465: a traced design's hole, with an island in it. A traced curve's
//! points lie a little off any one circle, and where such a convex curve
//! of radius about `F · tan(α/2)` meets a reflex corner, the male's floor
//! inset of the hole, which collapses the curve to about a point, came
//! back from `cavalier_contours` as open pieces, so the hole had no floor:
//! nothing cut its walls beyond `F · tan(α/2)` of the medial axis and the
//! plug stood up to a millimetre proud (the maintainer's eagle, 1.17 mm).

use super::*;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;

/// The hole's outline as the male carves it, design space,
/// counter-clockwise: a narrow channel and a chamber. The channel runs
/// down a straight edge and round a curve of eight chords about (1, 1) of
/// radius 1, whose points stand 0.002 mm in and out of the circle in turn
/// (a traced curve's), then turns at a reflex corner onto a long edge out
/// to a sharp far corner, and comes back along the line from there toward
/// its top. There it opens into a 10 × 6 mm chamber. Scaled so the curve's
/// radius is a little over the male's floor inset, `P · tan 30°` =
/// 1.155 mm, where the channel's inset closes, and centred on the origin.
fn traced_hole() -> Vec<(f64, f64)> {
    let scale = 2.0 * 30.0_f64.to_radians().tan() / 0.995;
    let mut points = vec![(-5.0, 6.0), (-0.2, 0.75)];
    for k in 0..=8 {
        let angle = (-145.0 + 35.0 * f64::from(k) / 8.0_f64).to_radians();
        let r = 1.0 + if k % 2 == 0 { 0.002 } else { -0.002 };
        points.push((1.0 + r * angle.cos(), 1.0 + r * angle.sin()));
    }
    points.extend([
        (8.0, -8.0),
        (2.0, 1.75),
        (5.0, 6.0),
        (5.0, 12.0),
        (-5.0, 12.0),
    ]);
    points
        .into_iter()
        .map(|(x, y)| ((x - 1.5) * scale, (y - 2.0) * scale))
        .collect()
}

/// The traced design on 100 × 100 mm stock at [`CENTRE`]: a 30 mm square
/// plate with [`traced_hole`] in it, mirrored so the male's hole is the
/// outline itself, and a 2 mm square island in the hole's chamber.
fn traced_project() -> Project {
    let mirrored: Vec<(f64, f64)> = traced_hole().into_iter().map(|(x, y)| (-x, y)).collect();
    let square = |x: f64, y: f64, side: f64| {
        polygon(&[(x, y), (x + side, y), (x + side, y + side), (x, y + side)])
    };
    project_of(
        vec![
            square(-15.0, -15.0, 30.0),
            polygon(&mirrored),
            square(0.5, 7.0, 2.0),
        ],
        CENTRE,
        100.0,
        100.0,
    )
}

/// The traced design's female or male, the V-bit's floor at step-over 0.1.
fn traced_inlay(side: InlaySide) -> InlayOp {
    InlayOp {
        step_over_fraction: Some(0.1),
        ..inlay(side)
    }
}

/// #465 (dc-geometry): the hole's outline inset by every distance from
/// 1.145 to 1.165 mm, round the curve's radius, is closed, and shrinks as
/// the inset grows. Before #465 `offset` dropped the open pieces
/// `cavalier_contours` gave for some of them and returned nothing.
#[test]
fn traced_hole_insets_close() {
    let hole = dc_geometry::Contour::new(polygon(&traced_hole())).expect("a contour");
    let mut last_mm2 = f64::INFINITY;
    for step in 0..=20 {
        let inset_mm = 1.145 + 0.001 * f64::from(step);
        let pieces = offset(&hole, -inset_mm);
        assert_eq!(pieces.len(), 1, "inset {inset_mm}: {} pieces", pieces.len());
        let area_mm2 = pieces[0].signed_area_mm2().abs();
        assert!(
            area_mm2 > 0.0 && area_mm2 < last_mm2,
            "inset {inset_mm}: {area_mm2} mm² after {last_mm2}"
        );
        last_mm2 = area_mm2;
    }
}

/// #465 (item 3): the male floors the traced hole. Its carve region (the
/// hole less the island) has a floor at `P · tan 30°`, and the pair seats
/// within [`SEAT_MM`] (0.1 mm grid). Before, the hole had no floor and
/// the plug stood proud of the female.
#[test]
fn traced_hole_plug_seats_flush() {
    let project = traced_project();
    let male = traced_inlay(InlaySide::Male);
    let side = side(
        &with_op(&project, Operation::Inlay(male.clone())),
        &male,
        &mut Vec::new(),
    )
    .expect("the male resolves");
    let inset_mm = side.flat_depth_mm * slope();
    let hole = side
        .targets
        .regions
        .iter()
        .find(|r| r.holes().len() == 1 && r.outer().signed_area_mm2() < 30.0 * 30.0)
        .expect("the male carves the traced hole");
    let floors = floor_regions(
        std::slice::from_ref(hole),
        inset_mm,
        FLATTEN_TOLERANCE_DEFAULT_MM,
    );
    assert!(!floors.is_empty(), "the traced hole has no floor");
    let fit = fit_of(
        &project,
        &traced_inlay(InlaySide::Female),
        &project,
        &male,
        0.1,
    );
    println!(
        "traced hole: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
        fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
    );
    assert!(
        fit.male_mm <= SEAT_MM,
        "the male stands {:.3} mm proud of the female's design at {:?}",
        fit.male_mm,
        fit.male_at
    );
    assert!(
        fit.seat_mm <= SEAT_MM,
        "the plug stands {:.3} mm proud, binding at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}
