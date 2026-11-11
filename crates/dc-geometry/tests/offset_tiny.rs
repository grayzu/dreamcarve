// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `offset` never panics on a tiny contour (#637; #611 was the first
//! case). An SVG file's specks reach it from `init`, `check` and
//! `image-prep`, so no contour of any scale may bring it down.
//!
//! `cavalier_contours` 0.9's `Polyline::remove_redundant` drops every
//! vertex it counts as collinear (a cross product under its 1e-5
//! tolerance) and then reads the second vertex of what is left. The cases
//! here sweep shapes of every scale from 1 nm to 1 mm, with the contour
//! built by the same rules as any other (`Contour::new`, which refuses
//! the ones that enclose no area), and offset each way. A panic anywhere
//! fails the test. The cases come from a fixed seed.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "test code reports failure by panicking"
)]

use dc_geometry::{Contour, Point, Polyline, Vertex, offset};
use std::f64::consts::TAU;

/// A xorshift generator, so the cases are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Offsets of `contour` by each distance in `deltas_mm`, both ways.
fn offset_all(contour: &Contour, deltas_mm: &[f64]) {
    for &delta_mm in deltas_mm {
        let _ = offset(contour, delta_mm);
        let _ = offset(contour, -delta_mm);
    }
}

/// Distances from a nanometre to a few millimetres, each way round.
fn deltas(scale_mm: f64) -> Vec<f64> {
    vec![
        scale_mm * 0.01,
        scale_mm * 0.3,
        scale_mm,
        scale_mm * 3.0,
        1e-6,
        1e-4,
        1e-3,
        0.05,
        0.08,
        1.0,
    ]
}

/// Scales from 1 nm to 1 mm, five to a decade.
fn scales_mm() -> Vec<f64> {
    (0..=30)
        .map(|k| 1e-6 * 10f64.powf(f64::from(k) / 5.0))
        .collect()
}

fn regular(centre: Point, radius_mm: f64, sides: usize, phase: f64) -> Polyline {
    Polyline::new(
        (0..sides)
            .map(|k| {
                let a = phase + TAU * k as f64 / sides as f64;
                Vertex::line(
                    centre.x + radius_mm * a.cos(),
                    centre.y + radius_mm * a.sin(),
                )
            })
            .collect(),
        true,
    )
}

/// The speck of #637: an octagon of radius 2 µm, 4 µm across, whose area
/// (1.13e-5 mm²) is over the area guard of #611.
#[test]
fn octagon_four_micrometres_across_is_offset_without_a_panic() {
    let octagon = Contour::new(regular(Point::new(10.0, 20.0), 0.002, 8, 0.0)).expect("an octagon");
    assert!(octagon.signed_area_mm2() > 1e-5);
    offset_all(&octagon, &deltas(0.002));
}

#[test]
fn regular_polygons_of_every_scale_are_offset_without_a_panic() {
    let mut rng = Rng(0x0637_0637_0637_0637);
    let mut built = 0;
    for scale_mm in scales_mm() {
        for sides in [3, 4, 5, 6, 8, 12, 16, 32, 64, 256] {
            for centre in [Point::new(0.0, 0.0), Point::new(137.25, 411.5)] {
                let phase = rng.next() * TAU;
                if let Ok(contour) = Contour::new(regular(centre, scale_mm, sides, phase)) {
                    built += 1;
                    offset_all(&contour, &deltas(scale_mm));
                }
            }
        }
    }
    assert!(built > 400, "only {built} contours were built");
}

#[test]
fn rectangles_and_slivers_of_every_scale_are_offset_without_a_panic() {
    let mut rng = Rng(0x0637_0001_0002_0003);
    let mut built = 0;
    for scale_mm in scales_mm() {
        for aspect in [1.0, 1.5, 3.0, 10.0, 100.0, 1e4, 1e6] {
            for flip in [false, true] {
                let (w, h) = if flip {
                    (scale_mm, scale_mm * aspect)
                } else {
                    (scale_mm * aspect, scale_mm)
                };
                // Slivers as long as 100 mm are the point; a sliver is
                // thin by the scale and long by the aspect.
                let min = Point::new(rng.next() * 300.0, rng.next() * 300.0);
                if let Ok(contour) = Contour::new(Polyline::rect(min, w, h)) {
                    built += 1;
                    offset_all(&contour, &deltas(scale_mm));
                }
            }
        }
    }
    assert!(built > 200, "only {built} contours were built");
}

#[test]
fn triangles_and_random_polygons_of_every_scale_are_offset_without_a_panic() {
    let mut rng = Rng(0x0637_a5a5_5a5a_1234);
    let mut built = 0;
    for scale_mm in scales_mm() {
        for case in 0..60 {
            let sides = 3 + case % 9;
            // Star-shaped about the centre, so simple: angles ascend and
            // the radius varies, down to a sliver.
            let squash = [1.0, 0.3, 0.01, 1e-3][case % 4];
            let centre = Point::new(rng.next() * 500.0 - 250.0, rng.next() * 500.0 - 250.0);
            let mut angles: Vec<f64> = (0..sides).map(|_| rng.next() * TAU).collect();
            angles.sort_by(f64::total_cmp);
            let vertices = angles
                .iter()
                .map(|&a| {
                    let r = scale_mm * (0.3 + 0.7 * rng.next());
                    Vertex::line(centre.x + r * a.cos(), centre.y + r * a.sin() * squash)
                })
                .collect();
            if let Ok(contour) = Contour::new(Polyline::new(vertices, true)) {
                built += 1;
                offset_all(&contour, &deltas(scale_mm));
            }
        }
    }
    assert!(built > 300, "only {built} contours were built");
}

#[test]
fn arcs_and_circles_of_every_scale_are_offset_without_a_panic() {
    let mut rng = Rng(0x0637_c1c1_e5e5_0042);
    for scale_mm in scales_mm() {
        let centre = Point::new(rng.next() * 100.0, rng.next() * 100.0);
        if let Ok(contour) = Contour::new(Polyline::circle(centre, scale_mm)) {
            offset_all(&contour, &deltas(scale_mm));
        }
        // A D: a half circle closed by its diameter, and a lens.
        let d = Polyline::new(
            vec![
                Vertex::new(centre.x - scale_mm, centre.y, 1.0),
                Vertex::line(centre.x + scale_mm, centre.y),
            ],
            true,
        );
        if let Ok(contour) = Contour::new(d) {
            offset_all(&contour, &deltas(scale_mm));
        }
        let lens = Polyline::new(
            vec![
                Vertex::new(centre.x - scale_mm, centre.y, 0.4),
                Vertex::new(centre.x + scale_mm, centre.y, 0.4),
            ],
            true,
        );
        if let Ok(contour) = Contour::new(lens) {
            offset_all(&contour, &deltas(scale_mm));
        }
    }
}

/// A traced shape has many vertices a few micrometres apart; the ones
/// small enough to look redundant at the tolerance must not panic either.
#[test]
fn finely_traced_shapes_are_offset_without_a_panic() {
    for radius_mm in [0.002, 0.004, 0.01, 0.05, 1.0, 20.0] {
        for sides in [100, 1000, 20_000] {
            // The finest of them cross themselves at the rounding of the
            // coordinates; `Contour::new` refuses those.
            if let Ok(contour) = Contour::new(regular(Point::new(5.0, 5.0), radius_mm, sides, 0.1))
            {
                offset_all(&contour, &deltas(radius_mm));
            }
        }
    }
}
