// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test shapes for #694. Each one holds an arc past a half turn
//! (|bulge| > 1) and comes with the same shape where that arc is drawn as
//! its two halves. The tests check that cavalier_contours gives the same
//! offsets, booleans and self-crossings for both. The halves are built
//! here from angles, not by the code under test.

use crate::distance::distance_to_segment;
use crate::point::Point;
use crate::polyline::{Polyline, Vertex};
use std::f64::consts::{FRAC_PI_2, PI};

/// The sweeps past a half turn the tests cover, degrees.
pub(crate) const SWEEPS_DEG: [f64; 3] = [200.0, 270.0, 350.0];

/// One shape, drawn with its big arc whole and as two halves.
pub(crate) struct Case {
    /// What the shape is, for failure messages.
    pub(crate) name: String,
    /// With the arc past a half turn as one bulge.
    pub(crate) whole: Polyline,
    /// With that arc as its two halves.
    pub(crate) split: Polyline,
}

/// The point at `angle_rad` on the circle of `radius_mm` about `centre`,
/// as a vertex with `bulge`.
fn on_circle(centre: Point, radius_mm: f64, angle_rad: f64, bulge: f64) -> Vertex {
    Vertex::new(
        centre.x + radius_mm * angle_rad.cos(),
        centre.y + radius_mm * angle_rad.sin(),
        bulge,
    )
}

/// The vertices that start an arc of `radius_mm` about `centre`, from
/// `from_rad` sweeping `sweep_rad` (negative is clockwise). That is one
/// vertex when the arc is whole, or the start and the middle when it is
/// `split`. The vertex at the arc's end is the caller's.
fn arc(centre: Point, radius_mm: f64, from_rad: f64, sweep_rad: f64, split: bool) -> Vec<Vertex> {
    if split {
        let half = (sweep_rad / 8.0).tan();
        vec![
            on_circle(centre, radius_mm, from_rad, half),
            on_circle(centre, radius_mm, from_rad + sweep_rad / 2.0, half),
        ]
    } else {
        vec![on_circle(
            centre,
            radius_mm,
            from_rad,
            (sweep_rad / 4.0).tan(),
        )]
    }
}

/// A sector of radius 10 about the origin: a slice of pie, or past a half
/// turn a pac-man.
fn sector(sweep_rad: f64, split: bool) -> Polyline {
    let (centre, r, from) = (Point::new(0.0, 0.0), 10.0, 0.3);
    let mut v = vec![Vertex::line(0.0, 0.0)];
    v.extend(arc(centre, r, from, sweep_rad, split));
    v.push(on_circle(centre, r, from + sweep_rad, 0.0));
    Polyline::new(v, true)
}

/// A 40 × 30 rectangle whose top edge has a circle of radius 5 drawn on
/// it, centred at x = 20. The circle is an `outward` lobe, or else a
/// notch cut into the rectangle. The arc of `sweep_rad` meets the edge
/// where the rest of the circle would cross it.
fn on_rectangle(sweep_rad: f64, outward: bool, split: bool) -> Polyline {
    let r = 5.0;
    // Half the angle the arc leaves out, which faces the rectangle for a
    // lobe and away from it for a notch.
    let h = PI - sweep_rad / 2.0;
    let (centre, from, sweep) = if outward {
        (
            Point::new(20.0, 30.0 + r * h.cos()),
            -FRAC_PI_2 + h,
            sweep_rad,
        )
    } else {
        (
            Point::new(20.0, 30.0 - r * h.cos()),
            FRAC_PI_2 - h,
            -sweep_rad,
        )
    };
    let mut v = vec![
        Vertex::line(0.0, 0.0),
        Vertex::line(40.0, 0.0),
        Vertex::line(40.0, 30.0),
    ];
    v.extend(arc(centre, r, from, sweep, split));
    v.push(Vertex::line(20.0 - r * h.sin(), 30.0));
    v.push(Vertex::line(0.0, 30.0));
    Polyline::new(v, true)
}

/// An open arc of radius 8 about (50, 50), counter-clockwise.
pub(crate) fn open_arc(sweep_rad: f64, split: bool) -> Polyline {
    let (centre, r, from) = (Point::new(50.0, 50.0), 8.0, -0.4);
    let mut v = arc(centre, r, from, sweep_rad, split);
    v.push(on_circle(centre, r, from + sweep_rad, 0.0));
    Polyline::new(v, false)
}

/// A shape drawn with an arc of the given sweep, radians, whole or
/// `split`.
type Shape = fn(f64, bool) -> Polyline;

/// Every closed shape at every sweep in [`SWEEPS_DEG`].
pub(crate) fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for deg in SWEEPS_DEG {
        let s = deg.to_radians();
        let shapes: [(&str, Shape); 3] = [
            ("sector", sector),
            ("lobe", |s, split| on_rectangle(s, true, split)),
            ("notch", |s, split| on_rectangle(s, false, split)),
        ];
        for (name, shape) in shapes {
            out.push(Case {
                name: format!("{name} of {deg}°"),
                whole: shape(s, false),
                split: shape(s, true),
            });
        }
    }
    out
}

/// The vertices of `polyline` and five points along each of its
/// segments, following arcs.
fn samples(polyline: &Polyline) -> Vec<Point> {
    let mut points: Vec<Point> = polyline.vertices().iter().map(|v| v.point()).collect();
    for seg in polyline.segments() {
        let length_mm = seg.length_mm();
        for k in 1..6 {
            if let Some((p, _)) = seg.point_at(length_mm * f64::from(k) / 6.0) {
                points.push(p);
            }
        }
    }
    points
}

/// The farthest a sampled point of any of `a` lies from all of `b`, or a
/// point of `b` from `a`, mm (a sampled Hausdorff distance). Infinite
/// when only one side is empty.
pub(crate) fn deviation_mm(a: &[&Polyline], b: &[&Polyline]) -> f64 {
    let one_way = |from: &[&Polyline], to: &[&Polyline]| {
        from.iter()
            .flat_map(|p| samples(p))
            .map(|p| {
                to.iter()
                    .flat_map(|t| t.segments())
                    .map(|seg| distance_to_segment(seg, p))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    };
    one_way(a, b).max(one_way(b, a))
}
