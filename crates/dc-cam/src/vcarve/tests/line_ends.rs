// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests that the generator's own cone test (step 10) refuses what cuts
//! past a wall, and only that, and that a carve round the end of a drawn
//! line (a thin wedge of material standing into a region) passes it
//! (#463; `docs/specs/phase-3/design.md`, "V-Carve generator", step 5,
//! "Fit", and step 10).

use super::*;
use crate::vcarve::medial::Step;

/// The 90° V-bit's carve of `regions` from the stock top, with `runs` as
/// its blocks, through [`finish`] (steps 9 and 10).
fn finish_runs(regions: Vec<Region>, runs: &[Run]) -> Result<Toolpath, CamError> {
    let mut p = project(vec![rect(10.0, 6.0)]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = carve_op(&p);
    let p = with_op(&p, &op);
    let tool = p
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the V-bit")
        .clone();
    let rates = LapRates {
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
    };
    let blocks: Vec<CutBlock> = runs
        .iter()
        .map(|run| medial::run_block(run, &test_shape(), rates, TOP_MM + 5.0))
        .collect();
    let carve = Carve {
        regions,
        shapes: vec![test_shape()],
        start_plane_z_mm: TOP_MM,
        engaged_from_z_mm: TOP_MM,
        flat_depth_mm: None,
        strategy: &VCarveStrategy::Medial,
        step_over_fraction: None,
        cleared_by: Vec::new(),
        open_edge: None,
        design: None,
    };
    let toolpath = finish(
        &p,
        &op.id,
        &tool,
        &carve,
        blocks,
        &[],
        MachiningOrder::FullDepthFirst,
    )?;
    assert_safe(&toolpath, &p, &Operation::VCarve(op));
    Ok(toolpath)
}

/// A run from `start` through `steps`, each a cut along a chain.
fn run(start: (f64, f64, f64), steps: &[(f64, f64, f64)]) -> Run {
    let at = |(x, y, z): (f64, f64, f64)| Point3::new(AT.0 + x, AT.1 + y, z);
    Run {
        start: at(start),
        steps: steps
            .iter()
            .map(|&p| Step {
                to: at(p),
                link: false,
            })
            .collect(),
    }
}

/// The 10 × 6 mm rectangle at [`AT`] as one region, with `holes`
/// (drawing space, each a rectangle `x0, y0, x1, y1`).
fn rectangle_with(holes: &[(f64, f64, f64, f64)]) -> Vec<Region> {
    let contours = std::iter::once(Polyline::rect(at(0.0, 0.0), 10.0, 6.0))
        .chain(
            holes
                .iter()
                .map(|&(x0, y0, x1, y1)| Polyline::rect(at(x0, y0), x1 - x0, y1 - y0)),
        )
        .map(|p| Contour::new(p).expect("a rectangle"))
        .collect();
    nest(contours).expect("the rectangles nest").0
}

/// `vcarve::touching_a_wall_is_not_a_violation` (#463): a cone that meets
/// a wall exactly is no violation, at the surface or at depth. In the
/// 10 × 6 mm rectangle with the 90° bit (slope 1), a hand-built carve
/// starts at a corner, on both walls at the stock top, runs down the
/// corner's bisector to (3, 3) 3 mm deep, its cone touching both walls all
/// the way, along the centre line, touching the long walls, and up the far
/// corner's bisector to that corner at the stock top; a second run at the
/// stock top ends on the top wall. Both are returned, and pass
/// [`assert_safe`]'s cone test.
#[test]
fn touching_a_wall_is_not_a_violation() {
    let top = TOP_MM;
    let runs = [
        run(
            (0.0, 0.0, top),
            &[
                (3.0, 3.0, top - 3.0),
                (7.0, 3.0, top - 3.0),
                (10.0, 6.0, top),
            ],
        ),
        run((5.0, 4.0, top), &[(5.0, 6.0, top)]),
    ];
    let toolpath = finish_runs(rectangle_with(&[]), &runs)
        .unwrap_or_else(|e| panic!("a cone that touches a wall passes: {e}"));
    assert!(!cuts(&toolpath).is_empty());
}

/// `vcarve::real_wall_violation_is_still_refused` (#463): a move that
/// cuts 0.05 mm past a wall is refused, and the message names the
/// distance where the tip is farthest out. The rectangle has a 0.1 mm bar
/// of material from (4.95, 2) to (5.05, 4) in it, a ridge's end: a move at
/// the stock top across it, from (3, 3) to (7, 3), is 0.05 mm past its
/// walls at (5, 3), its middle, though both its ends are inside. Before
/// #463 the overlap was read at an end ("1.9500 mm" here; at a crossing
/// it read "0.0000 mm"). A move that ends 0.05 mm beyond the top wall is
/// refused with that distance too.
#[test]
fn real_wall_violation_is_still_refused() {
    let top = TOP_MM;
    let regions = || rectangle_with(&[(4.95, 2.0, 5.05, 4.0)]);
    for (name, runs, x, y) in [
        (
            "across the bar",
            run((3.0, 3.0, top), &[(7.0, 3.0, top)]),
            5.0,
            3.0,
        ),
        (
            "past the top wall",
            run((7.0, 4.0, top), &[(7.0, 6.05, top)]),
            7.0,
            6.05,
        ),
    ] {
        match finish_runs(regions(), &[runs]) {
            Err(e @ CamError::ConeOffMaterial { overlap_mm, .. }) => {
                assert_approx_eq!(overlap_mm, 0.05, 1e-6);
                let CamError::ConeOffMaterial { x_mm, y_mm, .. } = e else {
                    unreachable!("matched above");
                };
                assert_approx_eq!(x_mm, AT.0 + x, 1e-3);
                assert_approx_eq!(y_mm, AT.1 + y, 1e-3);
                let message = e.to_string();
                assert!(
                    message.contains("would cut 0.0500 mm past a wall"),
                    "{name}: {message}"
                );
            }
            other => panic!("{name}: expected ConeOffMaterial, got {other:?}"),
        }
    }
}

/// A `width` × `height` mm rectangle with a thin wedge of material hanging
/// from its top edge at `x0`: a drawn line that ends inside the region,
/// `length` long, `half_deg` either side of its axis, its axis
/// `tilt_deg` off straight down.
fn line_end(
    width: f64,
    height: f64,
    x0: f64,
    length: f64,
    half_deg: f64,
    tilt_deg: f64,
) -> Polyline {
    let heading = (270.0 + tilt_deg).to_radians();
    let tip = (x0 + length * heading.cos(), height + length * heading.sin());
    let half = length * half_deg.to_radians().tan();
    polygon(&[
        (0.0, 0.0),
        (width, 0.0),
        (width, height),
        (x0 + half, height),
        tip,
        (x0 - half, height),
        (0.0, height),
    ])
}

/// `vcarve::line_ends_carve` (#463): a region a line ends in carves. Each
/// case is a `2h` × `h` rectangle with a wedge from its top edge, its
/// length in medial steps (`h / 160`), half-angle and tilt as given, cut
/// with the 60° bit to a flat depth: the cases of a sweep that main
/// refused, six of the 33 in 576. Before #463 the first four stopped at
/// "would cut 0.0000 mm past a wall", the last two at 0.022 and 0.043 mm:
/// a chain's straight stretch between two medial nodes cut across the
/// wedge's tip at or just below the stock top. Each now generates and
/// passes [`assert_safe`], the cone test included.
#[test]
fn line_ends_carve() {
    for (h, steps, half_deg, tilt_deg, flat_mm) in [
        (20.0, 0.5, 20.0, 0.0, 0.5),
        (20.0, 1.5, 20.0, 33.0, 0.5),
        (20.0, 5.0, 11.0, 17.0, 0.5),
        (20.0, 2.0, 3.0, 17.0, 3.0),
        (10.0, 1.5, 20.0, 52.0, 0.5),
        (20.0, 2.0, 11.0, 52.0, 3.0),
    ] {
        let wedge = line_end(2.0 * h, h, 0.77 * h, steps * h / 160.0, half_deg, tilt_deg);
        let p = project(vec![wedge]);
        let op = VCarveOp {
            flat_depth_mm: Some(flat_mm),
            step_over_fraction: Some(0.1),
            ..carve_op(&p)
        };
        let result = try_carve(&p, &op);
        assert!(
            result.is_ok(),
            "h {h}, {steps} steps, {half_deg}°, tilted {tilt_deg}°: {result:?}"
        );
        carve(&p, &op);
    }
}
