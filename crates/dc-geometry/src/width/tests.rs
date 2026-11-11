// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Width and gap tests (`docs/specs/phase-3/design.md`, "Test plan →
//! Sub-phase 3.4", `width::`; slice 21b, #595).

use super::*;
use crate::assert_approx_eq;
use crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;
use crate::medial::{medial_axis, resolution};
use crate::point::Point;
use crate::polyline::{Polyline, Vertex};
use crate::region::nest;
use proptest::prelude::*;
use std::f64::consts::TAU;

const TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

fn contour(points: &[(f64, f64)]) -> Contour {
    let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
    Contour::new(Polyline::new(vertices, true)).expect("a simple contour")
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
    Contour::new(Polyline::rect(Point::new(x, y), w, h)).expect("a rectangle")
}

fn regions_of(contours: Vec<Contour>) -> Vec<Region> {
    nest(contours).expect("the contours nest").0
}

fn region_of(contours: Vec<Contour>) -> Region {
    regions_of(contours).into_iter().next().expect("a region")
}

#[test]
fn widest_point_of_a_circle_is_its_diameter() {
    let circle = Contour::new(Polyline::circle(Point::new(3.0, -2.0), 5.0)).expect("a circle");
    let region = region_of(vec![circle]);
    assert!(is_narrower_than(&region, 10.1, TOL));
    assert!(!is_narrower_than(&region, 9.9, TOL));
}

#[test]
fn widest_point_of_a_bar_is_its_width() {
    let region = region_of(vec![rect(0.0, 0.0, 30.0, 4.0)]);
    assert!(is_narrower_than(&region, 4.1, TOL));
    assert!(!is_narrower_than(&region, 3.9, TOL));
    // At the width exactly: not narrower, so a piece "at or above" the
    // threshold is never dropped.
    assert!(!is_narrower_than(&region, 4.0, TOL));
}

#[test]
fn a_hairline_is_narrower_than_a_speck_threshold_whatever_its_length() {
    for length_mm in [1.0, 50.0, 400.0] {
        let region = region_of(vec![rect(0.0, 0.0, length_mm, 0.05)]);
        assert!(
            is_narrower_than(&region, 0.1, TOL),
            "a hairline {length_mm} mm long is under 0.1 mm wide"
        );
        assert!(!is_narrower_than(&region, 0.04, TOL));
    }
}

#[test]
fn nonsense_widths_are_never_narrower() {
    let region = region_of(vec![rect(0.0, 0.0, 30.0, 4.0)]);
    assert!(!is_narrower_than(&region, 0.0, TOL));
    assert!(!is_narrower_than(&region, -1.0, TOL));
    assert!(!is_narrower_than(&region, f64::NAN, TOL));
}

/// Twice the greatest clearance of a convex polygon's medial axis is the
/// diameter of its largest inscribed circle, which the erosion reads
/// exactly. Within 5 % either side of it the two agree (the axis is
/// sampled).
fn convex_polygon(a: f64, b: f64, jitter: &[f64]) -> Region {
    let n = jitter.len();
    let points: Vec<(f64, f64)> = jitter
        .iter()
        .enumerate()
        .map(|(k, j)| {
            let angle = TAU * (k as f64 + j) / n as f64;
            (a * angle.cos(), b * angle.sin())
        })
        .collect();
    region_of(vec![contour(&points)])
}

proptest! {
    #[test]
    fn erosion_test_agrees_with_the_medial_clearance(
        a in 6.0..15.0f64,
        b in 3.0..6.0f64,
        jitter in proptest::collection::vec(0.0..0.6f64, 5..10),
    ) {
        let region = convex_polygon(a, b, &jitter);
        let step_mm = resolution(&region).expect("a resolution").step_mm;
        let graph = medial_axis(&region, step_mm, TOL);
        let widest_mm = 2.0
            * graph
                .nodes
                .iter()
                .map(|n| n.clearance_mm)
                .fold(0.0, f64::max);
        prop_assert!(widest_mm > 0.0);
        prop_assert!(is_narrower_than(&region, widest_mm * 1.05, TOL));
        prop_assert!(!is_narrower_than(&region, widest_mm * 0.95, TOL));
    }
}

/// A 20 × 20 mm plate with a 10 × 10 mm hole: the hole holds a circle of
/// diameter 10 and no more; the ring round it is 5 mm wide, 5.86 mm at the
/// corners (centre `(r, r)`, `√2 (5 − r) = r`), so 6 mm is wider than the
/// piece and narrower than the hole.
#[test]
fn hole_test_reads_the_hole_not_the_piece() {
    let region = region_of(vec![rect(0.0, 0.0, 20.0, 20.0), rect(5.0, 5.0, 10.0, 10.0)]);
    let hole = region.holes().first().expect("a hole");
    assert!(!hole_is_narrower_than(hole, 9.9, TOL));
    assert!(hole_is_narrower_than(hole, 10.1, TOL));
    assert!(hole_is_narrower_than(hole, 11.0, TOL));
    assert!(
        is_narrower_than(&region, 6.0, TOL),
        "the ring is under 6 mm"
    );
    assert!(!hole_is_narrower_than(hole, 6.0, TOL), "the hole is not");
}

#[test]
fn local_widths_of_a_stroke_are_its_width() {
    let region = region_of(vec![rect(0.0, 0.0, 30.0, 2.0)]);
    let profile = local_widths(&region, TOL).expect("a profile");
    assert_approx_eq!(profile.median_mm(), 2.0, 0.02);
}

#[test]
fn a_tapering_tip_does_not_lower_the_median() {
    // A 2 mm stroke ending in a long point.
    let region = region_of(vec![contour(&[
        (0.0, 0.0),
        (30.0, 0.0),
        (40.0, 1.0),
        (30.0, 2.0),
        (0.0, 2.0),
    ])]);
    let profile = local_widths(&region, TOL).expect("a profile");
    assert_approx_eq!(profile.median_mm(), 2.0, 0.05);
    assert!(profile.min_mm() < profile.median_mm());
}

/// The #475 case: a 60 × 60 mm block (axis step 0.375 mm) with a 0.2 mm
/// hairline 40 mm long running out of it. At the shape step the hairline
/// is between two samples; the refined sampling finds it, so about 30 mm
/// of the profile is 0.2 mm wide.
#[test]
fn a_hairline_in_a_large_piece_is_measured() {
    let region = region_of(vec![contour(&[
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 29.9),
        (100.0, 29.9),
        (100.0, 30.1),
        (60.0, 30.1),
        (60.0, 60.0),
        (0.0, 60.0),
    ])]);
    let profile = local_widths(&region, TOL).expect("a profile");
    let hairline_mm: f64 = profile
        .samples()
        .iter()
        .filter(|&&(width, _)| (0.15..0.3).contains(&width))
        .map(|&(_, length)| length)
        .sum();
    assert!(
        hairline_mm > 30.0,
        "the hairline's axis is measured, found {hairline_mm} mm of it"
    );
}

#[test]
fn median_is_length_weighted() {
    // Two short narrow edges and one long wide one: the median is the
    // wide one, whatever the count.
    let profile = WidthProfile::from_samples(vec![(1.0, 1.0), (1.0, 1.0), (5.0, 10.0)]);
    assert_approx_eq!(profile.median_mm(), 5.0, 1e-12);
    assert_approx_eq!(profile.min_mm(), 1.0, 1e-12);
    let balanced = WidthProfile::from_samples(vec![(1.0, 6.0), (5.0, 4.0)]);
    assert_approx_eq!(balanced.median_mm(), 1.0, 1e-12);
}

#[test]
fn narrowest_gap_between_two_bars() {
    let regions = regions_of(vec![rect(0.0, 0.0, 20.0, 5.0), rect(0.0, 5.5, 20.0, 5.0)]);
    let gap = narrowest_gap(&regions, 3.0, TOL).expect("a gap");
    assert_approx_eq!(gap.width_mm, 0.5, 0.01);
    assert_approx_eq!(gap.y_mm, 5.25, 0.01);
}

#[test]
fn narrowest_gap_across_a_notch_of_one_piece() {
    // A 20 × 12 mm block with a 0.6 mm slot 10 mm deep cut in from the top.
    let region = region_of(vec![contour(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 12.0),
        (10.3, 12.0),
        (10.3, 2.0),
        (9.7, 2.0),
        (9.7, 12.0),
        (0.0, 12.0),
    ])]);
    let gap = narrowest_gap(&[region], 3.0, TOL).expect("a gap");
    assert_approx_eq!(gap.width_mm, 0.6, 0.01);
    assert_approx_eq!(gap.x_mm, 10.0, 0.31);
    assert!((2.0..=12.0).contains(&gap.y_mm), "along the slot");
}

#[test]
fn a_gap_beyond_the_reach_is_none() {
    let regions = regions_of(vec![rect(0.0, 0.0, 20.0, 5.0), rect(0.0, 5.5, 20.0, 5.0)]);
    assert!(narrowest_gap(&regions, 0.3, TOL).is_none());
    // One bar by itself has no gap, however far the reach.
    let alone = regions_of(vec![rect(0.0, 0.0, 20.0, 5.0)]);
    assert!(narrowest_gap(&alone, 50.0, TOL).is_none());
}

/// A disc (the space inside a ring) and an annulus round it (the space
/// outside), 2 mm apart: the ridge between pockets in `Regions` mode. The
/// disc is radius 10, the annulus runs from radius 12 to 15.
#[test]
fn narrowest_gap_is_the_ridge_between_pockets() {
    let disc = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 10.0)).expect("a disc");
    let outer = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 15.0)).expect("an outer");
    let inner = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 12.0)).expect("an inner");
    let regions = regions_of(vec![outer, inner, disc]);
    assert_eq!(regions.len(), 2, "the annulus and the disc inside it");
    let gap = narrowest_gap(&regions, 5.0, TOL).expect("a gap");
    assert_approx_eq!(gap.width_mm, 2.0, 0.02);
    let radius_mm = gap.x_mm.hypot(gap.y_mm);
    assert_approx_eq!(radius_mm, 11.0, 0.05);
}

/// A numerical sliver of a stroked outline union: about 6 µm by 4 nm, area
/// near 3e-11 mm² (#611).
fn sliver() -> Region {
    region_of(vec![rect(10.0, 20.0, 0.006, 0.000_004)])
}

#[test]
fn degenerate_sliver_is_narrower_than_any_width() {
    let region = sliver();
    for width_mm in [1e-6, 0.01, 0.1, 5.0] {
        assert!(
            is_narrower_than(&region, width_mm, TOL),
            "a 4 nm sliver is under {width_mm} mm wide"
        );
    }
    let hole = sliver().outer().reversed();
    assert!(hole_is_narrower_than(&hole, 0.1, TOL));
}

#[test]
fn functions_do_not_panic_on_slivers() {
    for (w, h) in [(0.006, 4e-6), (4e-6, 0.006), (0.5, 1e-8)] {
        let region = region_of(vec![rect(3.0, -7.0, w, h)]);
        let _ = is_narrower_than(&region, 0.1, TOL);
        let _ = hole_is_narrower_than(&region.outer().reversed(), 0.1, TOL);
        assert!(local_widths(&region, TOL).is_none_or(|p| p.min_mm() >= 0.0));
        let other = region_of(vec![rect(3.0, -6.9, 0.5, 0.5)]);
        let _ = narrowest_gap(&[region.clone(), other], 1.0, TOL);
        let _ = narrowest_gap(std::slice::from_ref(&region), 1.0, TOL);
    }
    // The sign fixture's sliver (#611): 6 µm long, a few nm wide, whose
    // offset made cavalier_contours panic.
    let triangle = Contour::oriented(Polyline::new(
        vec![
            Vertex::line(68.469_385_257_611_78, 33.602_204_291_770_875),
            Vertex::line(68.469_446_292_768_03, 33.602_208_106_468_14),
            Vertex::line(68.469_416_490_445_64, 33.602_207_152_793_824),
        ],
        true,
    ))
    .expect("a sliver triangle");
    let region = region_of(vec![triangle.clone()]);
    for width_mm in [1e-6, 0.01, 0.1] {
        assert!(is_narrower_than(&region, width_mm, TOL));
        assert!(hole_is_narrower_than(&triangle, width_mm, TOL));
    }
    assert!(local_widths(&region, TOL).is_none());
    assert!(narrowest_gap(&[region], 1.0, TOL).is_none());
    assert!(local_widths(&sliver(), TOL).is_none());
    let big = region_of(vec![rect(0.0, 0.0, 10.0, 10.0)]);
    assert!(narrowest_gap(&[big, sliver()], 1.0, TOL).is_none());
}
