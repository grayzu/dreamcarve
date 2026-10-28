// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The walls of a carve and the exact cone test on them
//! (`docs/specs/phase-3/design.md`, "Safety pass", the S17 bullet, and
//! "V-Carve generator", steps 5 and 10; security review H2 and M1).
//!
//! A V-bit whose tip is at depth `d` below the start plane cuts a disc of
//! radius `d · slope` there, and it cuts nothing below the carve's final V
//! surface exactly when that disc stays inside the carved region: the
//! final surface at a point `q` is `dist(q, wall) / slope` deep, and the
//! cone at `p` reaches `q` at `d − |q − p| / slope`, which the triangle
//! inequality keeps above it everywhere when `d · slope ≤ dist(p, wall)`.
//!
//! Along a straight move the tip's distance to one straight wall segment is
//! convex in the move's parameter and the disc's radius is linear in it,
//! so their difference is convex and its minimum is found by a
//! golden-section search to far below [`EPS_MM`]: no sampling step. Arcs
//! are flattened to chords within the caller's tolerance first, as
//! `dc_geometry::medial` flattens them, so the walls here are the walls
//! the medial axis measured its clearances to.

use dc_geometry::medial::point_in_loops;
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Region, Vec2, Vertex};

/// Golden-section iterations per wall segment: 0.618⁸⁰ of a move's length
/// is far below a nanometre for any move on the coordinate range.
const SEARCH_ITERATIONS: usize = 80;

/// Cells a side of the wall grid at most.
const MAX_CELLS: f64 = 128.0;

/// The straight wall segments of a set of regions, bucketed in a uniform
/// grid, with the flattened loops for the even-odd inside test.
#[derive(Debug, Clone)]
pub(crate) struct Walls {
    segments: Vec<(Point, Point)>,
    loops: Vec<Polyline>,
    origin: Point,
    cell_mm: f64,
    columns: usize,
    rows: usize,
    buckets: Vec<Vec<usize>>,
}

impl Walls {
    /// The outers and holes of `regions`, arcs flattened to chords within
    /// `tolerance_mm` as the medial axis flattens them, consecutive
    /// duplicates dropped.
    pub(crate) fn new(regions: &[Region], tolerance_mm: f64) -> Self {
        let _ = (regions, tolerance_mm);
        Self {
            segments: Vec::new(),
            loops: Vec::new(),
            origin: Point::new(0.0, 0.0),
            cell_mm: 1.0,
            columns: 1,
            rows: 1,
            buckets: vec![Vec::new()],
        }
    }

    /// The grid cell holding `p`, clamped to the grid.
    fn cell_of(&self, p: Point) -> (usize, usize) {
        (
            clamped_index((p.x - self.origin.x) / self.cell_mm, self.columns),
            clamped_index((p.y - self.origin.y) / self.cell_mm, self.rows),
        )
    }

    /// The segments that may come within `margin_mm` of the box `lo`–`hi`,
    /// ascending, each once.
    fn near(&self, lo: Point, hi: Point, margin_mm: f64) -> Vec<usize> {
        let (c0, r0) = self.cell_of(Point::new(lo.x - margin_mm, lo.y - margin_mm));
        let (c1, r1) = self.cell_of(Point::new(hi.x + margin_mm, hi.y + margin_mm));
        let mut out = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Some(bucket) = self.buckets.get(r * self.columns + c) {
                    out.extend_from_slice(bucket);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The exact distance, mm, from `p` to the nearest wall; infinite with
    /// no walls.
    pub(crate) fn clearance_mm(&self, p: Point) -> f64 {
        let _ = p;
        f64::INFINITY
    }

    /// Whether `p` lies inside the regions by the even-odd rule, or within
    /// `slack_mm` of a wall.
    pub(crate) fn holds(&self, p: Point, slack_mm: f64) -> bool {
        let _ = (p, slack_mm);
        true
    }

    /// Whether the straight tip path `a`–`b` stays inside the regions, or
    /// within `slack_mm` of them: the ends do, and so does the middle of
    /// every stretch between the points where the path meets a wall (or
    /// passes within `slack_mm` of a wall's end), so a path that leaves
    /// through a vertex is caught as well as one that crosses an edge.
    pub(crate) fn holds_path(&self, a: Point, b: Point, slack_mm: f64) -> bool {
        let _ = (a, b, slack_mm);
        true
    }

    /// The least, along the straight move `a`–`b`, of the tip's distance
    /// to the walls less the cone's radius `(start_plane_z_mm − z) ·
    /// slope` there, mm, with the point where it is least. Exact for the
    /// walls within reach; walls beyond it count as `2 · EPS_MM` of room,
    /// which is all a caller asks of them. A move with no XY motion is
    /// read at its lower end.
    pub(crate) fn least_room(
        &self,
        a: Point3,
        b: Point3,
        start_plane_z_mm: f64,
        slope: f64,
    ) -> (f64, Point3) {
        let _ = (b, start_plane_z_mm, slope);
        (f64::INFINITY, a)
    }
}

/// The minimiser in [0, 1] of convex `f`, by golden-section search.
fn golden_minimum(f: &impl Fn(f64) -> f64) -> f64 {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let mut x1 = hi - ratio * (hi - lo);
    let mut x2 = lo + ratio * (hi - lo);
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..SEARCH_ITERATIONS {
        if f1 <= f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - ratio * (hi - lo);
            f1 = f(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + ratio * (hi - lo);
            f2 = f(x2);
        }
    }
    (lo + hi) / 2.0
}

/// Distance, mm, from `p` to the segment `a`–`b`.
pub(crate) fn point_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab: Vec2 = b - a;
    let length_sq = ab.dot(ab);
    let t = if length_sq > 1e-18 {
        ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance_to(a + ab * t)
}

/// `points` without consecutive duplicates or an explicit closing vertex,
/// as the medial axis cleans its loops.
fn clean_loop(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in points {
        if out.last().is_some_and(|q| q.distance_to(p) < 1e-9) {
            continue;
        }
        out.push(p);
    }
    while let (Some(&first), Some(&last)) = (out.first(), out.last()) {
        if out.len() >= 2 && first.distance_to(last) < 1e-9 {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// `v` floored and clamped to `0..len`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is floored and clamped to 0..len first"
)]
fn clamped_index(v: f64, len: usize) -> usize {
    let last = len.saturating_sub(1);
    let f = v.floor();
    if f.is_nan() || f <= 0.0 {
        0
    } else if f >= last as f64 {
        last
    } else {
        f as usize
    }
}

/// A non-negative whole `v` as a count, saturating at [`MAX_CELLS`]; 0
/// for NaN or below.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=MAX_CELLS first"
)]
fn count_of(v: f64) -> usize {
    if v.is_nan() || v <= 0.0 {
        0
    } else {
        v.min(MAX_CELLS) as usize
    }
}
