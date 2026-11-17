// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The rounding, the seat line and the ball's costs of an Inlay cut with
//! a tapered ball nose (#681). The expected numbers are the design spec's
//! ("Inlay fit", "The taper's ±0.5°") and the tests' own, from the bench
//! bits' formulas.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use super::*;
use dc_geometry::{Polyline, Vertex};

/// The bench's 6.7° per side, as the profile's slope.
fn t() -> f64 {
    6.7_f64.to_radians().tan()
}

/// A bench bit of tip diameter `tip_mm`, capped at 3.175 mm.
fn bench(tip_mm: f64) -> BitProfile {
    BitProfile::TaperedBall {
        tip_radius_mm: tip_mm / 2.0,
        slope: t(),
        cap_radius_mm: 3.175,
    }
}

fn ultra() -> BitProfile {
    bench(0.762)
}

fn fine() -> BitProfile {
    bench(2.286)
}

/// `points` as one region.
fn region(points: &[(f64, f64)]) -> Vec<Region> {
    let polyline = Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    );
    nest(vec![Contour::new(polyline).unwrap()]).unwrap().0
}

/// A star of `n` points, tips `outer` and notches `inner` from (50, 50).
fn star(n: u32, outer: f64, inner: f64, turn: f64) -> Vec<(f64, f64)> {
    (0..2 * n)
        .map(|k| {
            let a = turn + std::f64::consts::PI * f64::from(k) / f64::from(n);
            let r = if k % 2 == 0 { outer } else { inner };
            (50.0 + r * a.cos(), 50.0 + r * a.sin())
        })
        .collect()
}

/// The largest distance from a point of the outlines of `a` to the
/// nearest outline of `b`, mm.
fn departure(a: &[Region], b: &[Region]) -> f64 {
    let walls: Vec<&Polyline> = b
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(Contour::polyline)
        .collect();
    a.iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .flat_map(|c| {
            c.polyline()
                .with_arcs_flattened(0.001)
                .vertices()
                .iter()
                .map(|v| v.point())
                .collect::<Vec<_>>()
        })
        .map(|p| {
            walls
                .iter()
                .filter_map(|w| nearest_point(w, p))
                .map(|n| n.gap_mm)
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// The design spec's rounding radii: `R_min = (D − G)·t + w_t`, 0.731 mm
/// with the Ultra bit at `D − G` 3 (`w_t` 0.378) and 1.488 mm with the
/// Fine (`w_t` 1.135); a V-bit has none.
#[test]
fn rounding_radii_are_the_spec_s() {
    let ultra = BitRounding::of(&ultra(), 3.0).unwrap();
    assert!((ultra.open_mm - 0.731).abs() < 5e-4, "{}", ultra.open_mm);
    assert!((ultra.close_mm - 0.378).abs() < 5e-4, "{}", ultra.close_mm);
    let fine = BitRounding::of(&fine(), 3.0).unwrap();
    assert!((fine.open_mm - 1.488).abs() < 5e-4, "{}", fine.open_mm);
    assert!((fine.close_mm - 1.135).abs() < 5e-4, "{}", fine.close_mm);
    assert_eq!(BitRounding::of(&BitProfile::VBit { slope: 0.5 }, 3.0), None);
}

/// Acceptance 61y, the rounding's property, over stars of 3 to 8 points
/// and a range of sharpness, and their unions with a bar: the rounded
/// design, rounded again, moves by no more than the flatten tolerance
/// (opening it by `R_min` and closing it by `w_t` changes nothing), with
/// both bench bits.
#[test]
fn rounded_design_is_open_and_closed() {
    let tolerance_mm = 0.01;
    for profile in [ultra(), fine()] {
        for n in 3..=8 {
            for (k, inner) in [6.0, 10.0, 16.0].into_iter().enumerate() {
                let turn = 0.37 * f64::from(n) + 0.11 * f64::from(u32::try_from(k).unwrap());
                let design = region(&star(n, 25.0, inner, turn));
                let first = rounded_for_bit(&design, &profile, 3.0, tolerance_mm)
                    .unwrap_or_else(|e| panic!("{n} points, inner {inner}: {e:?}"));
                let again = rounded_for_bit(&first.regions, &profile, 3.0, tolerance_mm)
                    .unwrap_or_else(|e| panic!("again: {n} points, inner {inner}: {e:?}"));
                assert!(
                    again.changes.is_empty(),
                    "{n} points, inner {inner}: {:?}",
                    again.changes
                );
                let apart = departure(&again.regions, &first.regions)
                    .max(departure(&first.regions, &again.regions));
                assert!(
                    apart <= tolerance_mm,
                    "{n} points, inner {inner}: moved {apart} mm"
                );
            }
        }
    }
}

/// A part or gap the rounding cannot keep is refused, with its width and
/// the width needed: a 1 mm bar (Ultra, `D − G` 3) is 1.000 mm against
/// 1.462; a ring whose wall is 1 mm thick (Fine) is opened, a part; two
/// bars 0.5 mm apart (Ultra) are a gap 0.500 against 0.757.
#[test]
fn topology_changes_are_refused() {
    let bar = region(&[(0.0, 0.0), (20.0, 0.0), (20.0, 1.0), (0.0, 1.0)]);
    let narrow = rounded_for_bit(&bar, &ultra(), 3.0, 0.01).unwrap_err();
    assert_eq!(narrow.kind, CornerKind::Point);
    assert!((narrow.width_mm - 1.0).abs() < 5e-4, "{}", narrow.width_mm);
    assert!(
        (narrow.needed_mm - 1.462).abs() < 5e-4,
        "{}",
        narrow.needed_mm
    );

    let two = [
        region(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        region(&[(10.5, 0.0), (20.5, 0.0), (20.5, 10.0), (10.5, 10.0)]),
    ]
    .concat();
    let gap = rounded_for_bit(&two, &ultra(), 3.0, 0.01).unwrap_err();
    assert_eq!(gap.kind, CornerKind::Notch);
    assert!((gap.width_mm - 0.5).abs() < 5e-3, "{}", gap.width_mm);
    assert!((gap.needed_mm - 0.757).abs() < 5e-4, "{}", gap.needed_mm);

    // A hole narrower than `2 w_t` (Fine, 2.27 mm): filled.
    let outer = Contour::new(Polyline::new(
        [(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)]
            .iter()
            .map(|&(x, y)| Vertex::line(x, y))
            .collect(),
        true,
    ))
    .unwrap();
    let slot = Contour::new(Polyline::new(
        [(10.0, 14.0), (10.0, 16.0), (20.0, 16.0), (20.0, 14.0)]
            .iter()
            .map(|&(x, y)| Vertex::line(x, y))
            .collect(),
        true,
    ))
    .unwrap();
    let holed = nest(vec![outer, slot]).unwrap().0;
    let filled = rounded_for_bit(&holed, &fine(), 3.0, 0.01).unwrap_err();
    assert_eq!(filled.kind, CornerKind::Notch);
    assert!((filled.width_mm - 2.0).abs() < 5e-3, "{}", filled.width_mm);
}

/// A V-bit's design is never rounded: it comes back as it went in.
#[test]
fn v_bit_design_comes_back_as_it_is() {
    let design = region(&star(5, 25.0, 6.0, 0.3));
    let rounded = rounded_for_bit(&design, &BitProfile::VBit { slope: 0.577 }, 3.0, 0.01).unwrap();
    assert_eq!(rounded.regions, design);
    assert!(rounded.changes.is_empty());
}

/// The seat line (design spec, "The taper's ±0.5°"): at `D` 4, `G` 1, `P`
/// 2 the Ultra bit's seat moves 0.40 mm per 0.5° of taper and the Fine's
/// 0.30, both 0.76 mm per 0.05 mm of tip radius; at the 6 mm test part's
/// `D` 6, `P` 2, the Ultra's 0.55. A V-bit has no seat line.
#[test]
fn seat_line_reports_the_sensitivity() {
    let ultra_seat = seat_sensitivity(&ultra(), 4.0, 2.0).unwrap();
    assert!((ultra_seat.taper_mm - 0.40).abs() < 5e-3, "{ultra_seat:?}");
    assert!((ultra_seat.tip_mm - 0.76).abs() < 5e-3, "{ultra_seat:?}");
    let fine_seat = seat_sensitivity(&fine(), 4.0, 2.0).unwrap();
    assert!((fine_seat.taper_mm - 0.30).abs() < 5e-3, "{fine_seat:?}");
    assert!((fine_seat.tip_mm - 0.76).abs() < 5e-3, "{fine_seat:?}");
    let part = seat_sensitivity(&ultra(), 6.0, 2.0).unwrap();
    assert!((part.taper_mm - 0.55).abs() < 5e-3, "{part:?}");
    assert_eq!(
        seat_sensitivity(&BitProfile::VBit { slope: 0.5 }, 4.0, 2.0),
        None
    );
}

/// The ball's costs (design spec, "Inlay fit"): the Fine bit with `G`
/// 0.25 stands 0.432 mm proud with a 0.051 mm wall gap (`h_t` 1.010); the
/// Ultra's `h_t` is 0.337 mm, so 0.25 costs about 0.03 and 0.004 mm. The
/// Fine with `P` 0.5 leaves 0.130 mm at the wall. At or above `h_t`,
/// nothing.
#[test]
fn the_ball_s_costs_are_the_spec_s() {
    let fine_gap = glue_gap_below_ball(&fine(), 0.25).unwrap();
    assert!((fine_gap.tangent_height_mm - 1.010).abs() < 5e-4);
    assert!((fine_gap.lift_mm - 0.432).abs() < 5e-4, "{fine_gap:?}");
    assert!((fine_gap.wall_gap_mm - 0.051).abs() < 5e-4, "{fine_gap:?}");
    let ultra_gap = glue_gap_below_ball(&ultra(), 0.25).unwrap();
    assert!((ultra_gap.lift_mm - 0.03).abs() < 5e-3, "{ultra_gap:?}");
    assert!(
        (ultra_gap.wall_gap_mm - 0.004).abs() < 5e-4,
        "{ultra_gap:?}"
    );
    let plug = plug_depth_below_ball(&fine(), 0.5).unwrap();
    assert!((plug.wall_gap_mm - 0.130).abs() < 5e-4, "{plug:?}");
    assert_eq!(glue_gap_below_ball(&fine(), 1.1), None);
    assert_eq!(plug_depth_below_ball(&fine(), 1.1), None);
    assert_eq!(
        glue_gap_below_ball(&BitProfile::VBit { slope: 1.0 }, 0.0),
        None
    );
}
