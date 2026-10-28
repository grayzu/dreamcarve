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
        let mut segments = Vec::new();
        let mut loops = Vec::new();
        for region in regions {
            for contour in std::iter::once(region.outer()).chain(region.holes()) {
                let flat = contour.polyline().with_arcs_flattened(tolerance_mm);
                let points = clean_loop(flat.vertices().iter().map(|v| v.point()));
                if points.len() < 2 {
                    continue;
                }
                let next = points.iter().cycle().skip(1);
                for (&a, &b) in points.iter().zip(next) {
                    segments.push((a, b));
                }
                loops.push(Polyline::new(
                    points.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
                    true,
                ));
            }
        }
        let (mut min, mut max) = (
            Point::new(f64::INFINITY, f64::INFINITY),
            Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for &(a, b) in &segments {
            for p in [a, b] {
                min = Point::new(min.x.min(p.x), min.y.min(p.y));
                max = Point::new(max.x.max(p.x), max.y.max(p.y));
            }
        }
        if segments.is_empty() {
            min = Point::new(0.0, 0.0);
            max = min;
        }
        let extent_mm = (max.x - min.x).max(max.y - min.y);
        let cell_mm = (extent_mm / MAX_CELLS).max(1e-3);
        let columns = count_of(((max.x - min.x) / cell_mm).floor()) + 1;
        let rows = count_of(((max.y - min.y) / cell_mm).floor()) + 1;
        let mut walls = Self {
            segments,
            loops,
            origin: min,
            cell_mm,
            columns,
            rows,
            buckets: vec![Vec::new(); columns * rows],
        };
        for (index, &(a, b)) in walls.segments.iter().enumerate() {
            let (c0, r0) = walls.cell_of(Point::new(a.x.min(b.x), a.y.min(b.y)));
            let (c1, r1) = walls.cell_of(Point::new(a.x.max(b.x), a.y.max(b.y)));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Some(bucket) = walls.buckets.get_mut(r * columns + c) {
                        bucket.push(index);
                    }
                }
            }
        }
        walls
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
        self.segments
            .iter()
            .map(|&(a, b)| point_segment(p, a, b))
            .fold(f64::INFINITY, f64::min)
    }

    /// Whether `p` lies inside the regions by the even-odd rule, or within
    /// `slack_mm` of a wall.
    pub(crate) fn holds(&self, p: Point, slack_mm: f64) -> bool {
        let loops: Vec<&Polyline> = self.loops.iter().collect();
        point_in_loops(p, &loops)
            || self
                .near(p, p, slack_mm)
                .into_iter()
                .filter_map(|i| self.segments.get(i))
                .any(|&(a, b)| point_segment(p, a, b) <= slack_mm)
    }

    /// Whether the straight tip path `a`–`b` stays inside the regions, or
    /// within `slack_mm` of them: the ends do, and so does the middle of
    /// every stretch between the points where the path meets a wall (or
    /// passes within `slack_mm` of a wall's end), so a path that leaves
    /// through a vertex is caught as well as one that crosses an edge.
    pub(crate) fn holds_path(&self, a: Point, b: Point, slack_mm: f64) -> bool {
        if !(self.holds(a, slack_mm) && self.holds(b, slack_mm)) {
            return false;
        }
        let ab = b - a;
        let length_sq = ab.dot(ab);
        if length_sq <= EPS_MM * EPS_MM {
            return true;
        }
        let lo = Point::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Point::new(a.x.max(b.x), a.y.max(b.y));
        let mut cuts = vec![0.0, 1.0];
        for &(c, d) in self
            .near(lo, hi, slack_mm)
            .iter()
            .filter_map(|&i| self.segments.get(i))
        {
            let cd = d - c;
            let denom = ab.cross(cd);
            if denom.abs() > 1e-12 {
                let t = (c - a).cross(cd) / denom;
                let u = (c - a).cross(ab) / denom;
                if (-1e-9..=1.0 + 1e-9).contains(&u) && (0.0..=1.0).contains(&t) {
                    cuts.push(t);
                }
            }
            for end in [c, d] {
                if point_segment(end, a, b) <= slack_mm {
                    cuts.push(((end - a).dot(ab) / length_sq).clamp(0.0, 1.0));
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.windows(2).all(|w| match w {
            [s, t] if t - s > 1e-12 => self.holds(a + ab * ((s + t) / 2.0), slack_mm),
            _ => true,
        })
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
        let radius = |z: f64| (start_plane_z_mm - z) * slope;
        let (a, b) = if a.xy().distance_to(b.xy()) <= EPS_MM {
            let low = if a.z <= b.z { a } else { b };
            (low, low)
        } else {
            (a, b)
        };
        let reach_mm = radius(a.z).max(radius(b.z)).max(0.0) + 2.0 * EPS_MM;
        let (pa, pb) = (a.xy(), b.xy());
        let lo = Point::new(pa.x.min(pb.x), pa.y.min(pb.y));
        let hi = Point::new(pa.x.max(pb.x), pa.y.max(pb.y));
        let at = |t: f64| {
            Point3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            )
        };
        let mut best = (reach_mm - radius(a.z).max(radius(b.z)), a);
        for &(c, d) in self
            .near(lo, hi, reach_mm)
            .iter()
            .filter_map(|&i| self.segments.get(i))
        {
            let room = |t: f64| {
                let p = at(t);
                point_segment(p.xy(), c, d) - radius(p.z)
            };
            let t = golden_minimum(&room);
            for t in [0.0, t, 1.0] {
                let g = room(t);
                if g < best.0 {
                    best = (g, at(t));
                }
            }
        }
        best
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
