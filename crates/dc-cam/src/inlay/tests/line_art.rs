// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #463: a line-art design carves. Its regions are separated by ridges
//! 0.2 to 1 mm wide, some are small and enclosed, and lines end inside
//! regions as thin wedges of material, which a chain's straight stretch
//! between two medial nodes can cut across
//! (`dc_project::test_support::line_art_project`).

use super::*;
use dc_project::test_support::{line_art_inlay, line_art_project, line_art_v_carve};

/// `vcarve::line_art_regions_carve` (#463): the line-art design's female
/// and male Inlay, each with its clearing pass, and a plain V-Carve with a
/// flat floor all generate, every run passing `assert_run_safe` (S1–S3 and
/// the cone test, S17). Before #463 none did: "would cut 0.0000 mm past a
/// wall", a chain's stretch across the tip of a line's end at the stock
/// top. `dc-gcode`'s `generated_line_art_passes_the_safety_pass` holds the
/// same programs to S1–S19.
#[test]
fn line_art_regions_carve() {
    let project = line_art_project();
    let carve = line_art_v_carve();
    let with_carve = with_op(&project, Operation::VCarve(carve.clone()));
    let (runs, _) = generate_vcarve(&with_carve, &carve)
        .unwrap_or_else(|e| panic!("the V-Carve generates: {e}\n{e:?}"));
    for run in &runs {
        assert_run_safe(run, &with_carve, &Operation::VCarve(carve.clone()));
    }
    for side in [InlaySide::Female, InlaySide::Male] {
        let (runs, _) = inlay_runs(&project, &line_art_inlay(side));
        assert_eq!(
            runs.len(),
            2,
            "{side:?}: the V-bit's run and the end mill's"
        );
    }
}

/// #465: the seat check on the line art, as [`plug_seats_flush`] reads
/// its pairs (0.1 mm grid): the plug seats within [`SEAT_MM`]. Before,
/// beside the line ends in cells A and C and under E the female's groove
/// stopped short and the plug stood up to 0.191 mm proud: round a
/// wedge's tip the medial axis is the curve equidistant from the tip and
/// the wall beyond, both seen in one direction a little way out, and the
/// contact-spread filter dropped it (`medial::axis_wraps_round_a_line_end`).
/// Where the 0.5 mm ridges meet, the male stands 0.024 mm proud.
#[test]
fn line_art_plug_seats_flush() {
    let project = line_art_project();
    let female = line_art_inlay(InlaySide::Female);
    let male = line_art_inlay(InlaySide::Male);
    let fit = fit_of(&project, &female, &project, &male, 0.1);
    println!(
        "line art: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
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
