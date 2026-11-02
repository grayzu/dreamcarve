// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #475: a thin spike of material standing a fraction of a millimetre from
//! the wall of a large region. The medial step follows the region's size
//! (`short_span / 160`, 0.3125 mm for a 50 mm plate), coarser than the gap
//! between the spike's tip and the wall, so the axis round the tip was a
//! few coarse nodes and the female's groove stopped short there.

use super::*;

/// A `side_mm` square plate centred on the origin with a hole in it: a
/// spike 6 mm long, 0.6 mm wide at its foot and closed by a point 2 mm
/// beyond the foot, its tip `gap_mm` from the plate's left wall and its
/// axis `tilt_deg` from the X axis. Placed at [`CENTRE`] on 100 × 100 mm
/// stock.
fn spike_project(side_mm: f64, gap_mm: f64, tilt_deg: f64) -> Project {
    let half = side_mm / 2.0;
    let tilt = tilt_deg.to_radians();
    let tip = (-half + gap_mm, 0.0);
    let along = (tilt.cos(), tilt.sin());
    let foot = (tip.0 + along.0 * 6.0, tip.1 + along.1 * 6.0);
    let across = (-along.1 * 0.3, along.0 * 0.3);
    let spike = polygon(&[
        tip,
        (foot.0 - across.0, foot.1 - across.1),
        (foot.0 + along.0 * 2.0, foot.1 + along.1 * 2.0),
        (foot.0 + across.0, foot.1 + across.1),
    ]);
    let plate = polygon(&[(-half, -half), (half, -half), (half, half), (-half, half)]);
    project_of(vec![plate, spike], CENTRE, 100.0, 100.0)
}

/// The spike's female or male, the V-bit's floor at step-over 0.1 and one
/// clearing pass.
fn spike_inlay(side: InlaySide) -> InlayOp {
    InlayOp {
        step_over_fraction: Some(0.1),
        clearing: vec![clearing_pass()],
        ..inlay(side)
    }
}

/// #475 (the regression test): on a 50 mm plate the spike's pair seats
/// within [`SEAT_MM`] on a 0.1 mm grid, its tip 0.2 mm from the wall
/// tilted 60°, and 0.5 mm from it square on. Before #475 the female
/// stopped short at the spike's tip and the plug stood 0.087 mm proud.
#[test]
fn spike_in_a_large_region_seats_flush() {
    for (gap_mm, tilt_deg) in [(0.2, 60.0), (0.5, 0.0)] {
        let project = spike_project(50.0, gap_mm, tilt_deg);
        let fit = fit_of(
            &project,
            &spike_inlay(InlaySide::Female),
            &project,
            &spike_inlay(InlaySide::Male),
            0.1,
        );
        println!(
            "gap {gap_mm}, tilt {tilt_deg}°: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
            fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
        );
        assert!(
            fit.male_mm <= SEAT_MM,
            "gap {gap_mm}, tilt {tilt_deg}°: the male stands {:.3} mm proud of the female's design at {:?}",
            fit.male_mm,
            fit.male_at
        );
        assert!(
            fit.seat_mm <= SEAT_MM,
            "gap {gap_mm}, tilt {tilt_deg}°: the plug stands {:.3} mm proud, binding at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}
