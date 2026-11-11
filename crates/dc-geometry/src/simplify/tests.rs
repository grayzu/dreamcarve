// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Simplify tests (`docs/specs/phase-3/design.md`, "Test plan → Sub-phase
//! 3.4", `simplify::`; slice 21b, #595).

use super::*;
use crate::distance::point_segment_distance_mm;
use crate::point::Point;
use crate::polyline::{Polyline, Vertex};
use crate::region::nest;
use proptest::prelude::*;
use std::f64::consts::TAU;

fn contour(points: &[(f64, f64)]) -> Contour {
    let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
    Contour::new(Polyline::new(vertices, true)).expect("a simple contour")
}

fn points_of(contour: &Contour) -> Vec<(f64, f64)> {
    contour
        .polyline()
        .vertices()
        .iter()
        .map(|v| (v.x, v.y))
        .collect()
}

fn region_of(contours: Vec<Contour>) -> Vec<Region> {
    nest(contours).expect("the contours nest").0
}

#[test]
fn a_staircase_collapses_within_the_tolerance() {
    // A right triangle whose hypotenuse is a staircase of 0.1 mm steps:
    // each step corner is 0.0707 mm off the diagonal.
    let mut points = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
    for k in 0..100 {
        let x = 10.0 - 0.1 * f64::from(k);
        points.push((x - 0.1, 10.0 - 0.1 * f64::from(k)));
        points.push((x - 0.1, 10.0 - 0.1 * f64::from(k + 1)));
    }
    // The staircase ends back at (0, 0).
    points.pop();
    let stairs = contour(&points);
    assert!(stairs.polyline().vertices().len() > 150);
    let simple = simplify(&stairs, 0.1);
    assert_eq!(
        points_of(&simple),
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]
    );
}

#[test]
fn a_corner_is_kept() {
    // Every segment is longer than twice the tolerance, so all six
    // corners of this L stay.
    let l_shape = contour(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 4.0),
        (4.0, 4.0),
        (4.0, 10.0),
        (0.0, 10.0),
    ]);
    let simple = simplify(&l_shape, 0.5);
    assert_eq!(simple.polyline().vertices().len(), 6);
    let square = contour(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    assert_eq!(simplify(&square, 0.5).polyline().vertices().len(), 4);
}

/// A star-shaped polygon about the origin, `radii` at evenly spaced angles.
fn star(radii: &[f64]) -> Contour {
    let n = radii.len();
    let points: Vec<(f64, f64)> = radii
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let angle = TAU * k as f64 / n as f64;
            (r * angle.cos(), r * angle.sin())
        })
        .collect();
    contour(&points)
}

proptest! {
    #[test]
    fn every_dropped_vertex_is_within_the_tolerance(
        radii in proptest::collection::vec(8.0..10.0f64, 12..60),
        tolerance_mm in 0.01..0.5f64,
    ) {
        let input = star(&radii);
        let output = simplify(&input, tolerance_mm);
        let kept = points_of(&output);
        prop_assert!(kept.len() >= 3);
        let original = points_of(&input);
        // Every kept vertex is one of the input's.
        for k in &kept {
            prop_assert!(original.contains(k));
        }
        // Every input vertex is within the tolerance of the output.
        let segments: Vec<_> = output.polyline().segments().collect();
        for &(x, y) in &original {
            let nearest = segments
                .iter()
                .map(|s| point_segment_distance_mm(*s, Point::new(x, y)))
                .fold(f64::INFINITY, f64::min);
            prop_assert!(nearest <= tolerance_mm + 1e-9, "{nearest} > {tolerance_mm}");
        }
    }
}

/// A block whose bottom edge has a 0.09 mm dent into the material, and a
/// sliver beside it that sits in the dent, its tip 0.06 mm above the edge.
/// A tolerance of 0.1 mm drops the dent's apex, the edge runs straight
/// through the sliver's tip, and the two cross.
#[test]
fn a_crossing_result_is_returned_unsimplified_and_listed() {
    let block = contour(&[
        (0.0, 0.0),
        (4.0, 0.0),
        (5.0, 0.09),
        (6.0, 0.0),
        (10.0, 0.0),
        (10.0, 5.0),
        (0.0, 5.0),
    ]);
    let sliver = contour(&[(4.9, -0.2), (5.1, -0.2), (5.0, 0.06)]);
    let regions = region_of(vec![block, sliver]);
    assert_eq!(regions.len(), 2);

    let (out, skipped) = simplify_regions(&regions, 0.1);
    assert_eq!(skipped, vec![0], "the block is the one that crosses");
    assert_eq!(out, regions, "nothing was changed");

    // Far enough apart, the same block simplifies.
    let apart = region_of(vec![
        contour(&[
            (0.0, 0.0),
            (4.0, 0.0),
            (5.0, 0.09),
            (6.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (0.0, 5.0),
        ]),
        contour(&[(4.9, -3.2), (5.1, -3.2), (5.0, -3.0)]),
    ]);
    let (simplified, skipped) = simplify_regions(&apart, 0.1);
    assert!(skipped.is_empty());
    assert_eq!(simplified.len(), 2);
    assert_eq!(simplified[0].outer().polyline().vertices().len(), 4);
}

#[test]
fn a_contour_keeps_three_vertices() {
    // A flat sliver of a triangle, wholly within the tolerance of its long
    // side: it still comes back a triangle, not a segment.
    let sliver = contour(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.05)]);
    let simple = simplify(&sliver, 1.0);
    assert_eq!(simple.polyline().vertices().len(), 3);
}

#[test]
fn anchors_are_deterministic() {
    let radii: Vec<f64> = (0..60)
        .map(|k| 9.0 + 0.8 * f64::from((k * 7) % 5) / 4.0)
        .collect();
    let input = star(&radii);
    let first = simplify(&input, 0.3);
    // The same contour started at another vertex, and wound the other way.
    let vertices = input.polyline().vertices().to_vec();
    let mut rotated = vertices.clone();
    rotated.rotate_left(17);
    let rotated = Contour::new(Polyline::new(rotated, true)).expect("rotated");
    assert_eq!(simplify(&rotated, 0.3), first);
    let reversed = Contour::new(input.polyline().reversed()).expect("reversed");
    assert_eq!(simplify(&reversed, 0.3), first);
    // The first vertex is the lowest, then the leftmost.
    let lowest = points_of(&first)
        .into_iter()
        .fold((f64::INFINITY, f64::INFINITY), |best, p| {
            if p.1.total_cmp(&best.1).then(p.0.total_cmp(&best.0)).is_lt() {
                p
            } else {
                best
            }
        });
    assert_eq!(points_of(&first).first().copied(), Some(lowest));
}
