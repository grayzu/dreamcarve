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
    /// For each segment, its loop's first segment and length: segment
    /// `k` of a loop runs from the loop's vertex `k` to its vertex `k + 1`.
    rings: Vec<(usize, usize)>,
    /// The tolerance the arcs were flattened within, mm.
    tolerance_mm: f64,
    loops: Vec<Polyline>,
    origin: Point,
    cell_mm: f64,
    columns: usize,
    rows: usize,
    buckets: Vec<Vec<usize>>,
}

/// Where a straight tip path leaves the regions ([`Walls::excursion`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Excursion {
    /// The point of the path farthest outside, as a fraction of the way
    /// from its start.
    pub t: f64,
    /// How far outside the walls it is there, mm.
    pub outside_mm: f64,
    /// The wall vertex the path cuts across, when it leaves and comes back
    /// through one loop ([`Walls::cut_corner`]).
    pub corner: Option<Corner>,
}

/// A wall vertex a tip path cuts across ([`Walls::cut_corner`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Corner {
    /// The vertex, mm.
    pub p: Point,
    /// The unit vector into the region that bisects the vertex.
    pub bisector: Vec2,
}

impl Walls {
    /// The outers and holes of `regions`, arcs flattened to chords within
    /// `tolerance_mm` as the medial axis flattens them, consecutive
    /// duplicates dropped.
    pub(crate) fn new(regions: &[Region], tolerance_mm: f64) -> Self {
        let mut segments = Vec::new();
        let mut rings = Vec::new();
        let mut loops = Vec::new();
        for region in regions {
            for contour in std::iter::once(region.outer()).chain(region.holes()) {
                let flat = contour.polyline().with_arcs_flattened(tolerance_mm);
                let points = clean_loop(flat.vertices().iter().map(|v| v.point()));
                if points.len() < 2 {
                    continue;
                }
                let first = segments.len();
                let next = points.iter().cycle().skip(1);
                for (&a, &b) in points.iter().zip(next) {
                    segments.push((a, b));
                    rings.push((first, points.len()));
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
            rings,
            tolerance_mm,
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

    /// How far a tip may stand outside these walls, mm: the flatten
    /// tolerance they were built at (a chord near a concave arc lies up
    /// to that far beyond the true wall) plus [`EPS_MM`]. The generator's
    /// own cone test ([`crate::vcarve::cone_in_material`]) allows this
    /// much, so fitting holds a chain's stretches to it too (#463).
    pub(crate) fn slack_mm(&self) -> f64 {
        self.tolerance_mm.max(0.0) + EPS_MM
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

    /// The wall point nearest `p` and its distance, mm; `None` with no
    /// walls. The grid is searched outward until the nearest point found
    /// is no farther than the cells searched reach, so the answer is exact.
    pub(crate) fn nearest(&self, p: Point) -> Option<(f64, Point)> {
        let foot = |a: Point, b: Point| {
            let ab: Vec2 = b - a;
            let length_sq = ab.dot(ab);
            let t = if length_sq > 1e-18 {
                ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            a + ab * t
        };
        let best_of = |indices: &[usize]| {
            indices
                .iter()
                .filter_map(|&i| self.segments.get(i))
                .map(|&(a, b)| {
                    let q = foot(a, b);
                    (p.distance_to(q), q)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
        };
        let mut margin_mm = self.cell_mm;
        let span_mm = self.cell_mm * (self.columns + self.rows) as f64;
        while margin_mm <= 2.0 * span_mm {
            let best = best_of(&self.near(p, p, margin_mm));
            if best.is_some_and(|(d, _)| d <= margin_mm) {
                return best;
            }
            margin_mm *= 2.0;
        }
        let all: Vec<usize> = (0..self.segments.len()).collect();
        best_of(&all)
    }

    /// The radius, mm, of the largest disc inside the walls that touches
    /// the wall point `w` with its centre along the unit vector `u` from
    /// it, at most `cap_mm` (the ball-shrinking construction of a medial
    /// axis point: for each wall segment the least radius whose disc,
    /// tangent at `w`, reaches it, `|q − w|² / (2 (q − w)·u)` over the
    /// segment's points `q` ahead of `w`, found in closed form). The discs
    /// tangent at `w` are nested, so only the walls within the largest,
    /// `cap_mm` about `w + u · cap_mm`, can bound it.
    pub(crate) fn ball_radius_mm(&self, w: Point, u: Vec2, cap_mm: f64) -> f64 {
        let centre = w + u * cap_mm;
        let mut radius_mm = cap_mm;
        for &(a, b) in self
            .near(centre, centre, cap_mm + EPS_MM)
            .iter()
            .filter_map(|&i| self.segments.get(i))
        {
            let d0: Vec2 = a - w;
            let e: Vec2 = b - a;
            let ratio = |t: f64| {
                let d = Vec2::new(d0.x + e.x * t, d0.y + e.y * t);
                let ahead = 2.0 * d.dot(u);
                // A point at or behind `w`'s tangent bounds no disc.
                (ahead > 1e-9 * (1.0 + d.length())).then(|| d.dot(d) / ahead)
            };
            // The stationary points of the ratio along the segment.
            let (eu, du) = (e.dot(u), d0.dot(u));
            let qa = e.dot(e) * eu;
            let qb = 2.0 * e.dot(e) * du;
            let qc = 2.0 * d0.dot(e) * du - d0.dot(d0) * eu;
            // The roots of `qa t² + qb t + qc`, in the form that keeps
            // its precision when `qa` is tiny (a wall nearly parallel to
            // the tangent).
            let mut ts = vec![0.0, 1.0];
            let disc = qb * qb - 4.0 * qa * qc;
            if disc >= 0.0 {
                let q = -0.5 * (qb + qb.signum() * disc.sqrt());
                if q.abs() > 1e-300 {
                    ts.push(qc / q);
                }
                if qa.abs() > 1e-300 {
                    ts.push(q / qa);
                }
            }
            for t in ts {
                if (0.0..=1.0).contains(&t)
                    && let Some(r) = ratio(t)
                {
                    radius_mm = radius_mm.min(r);
                }
            }
        }
        radius_mm
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
        self.excursion(a, b, slack_mm).is_none()
    }

    /// Where the straight tip path `a`–`b` first leaves the regions by
    /// more than `slack_mm` ([`Walls::holds_path`]'s test), or `None` when
    /// it stays in (#463). An end outside is the excursion itself. A
    /// stretch outside between two crossings is read every 1/32 of its
    /// length for the point farthest from the walls, so the distance is
    /// what the path leaves by, not the 0 of the walls it crosses; and
    /// when it leaves and comes back through one loop, the vertex it cuts
    /// across is named ([`Walls::cut_corner`]).
    pub(crate) fn excursion(&self, a: Point, b: Point, slack_mm: f64) -> Option<Excursion> {
        for (t, p) in [(0.0, a), (1.0, b)] {
            if !self.holds(p, slack_mm) {
                return Some(Excursion {
                    t,
                    outside_mm: self.clearance_mm(p),
                    corner: None,
                });
            }
        }
        let ab = b - a;
        let length_sq = ab.dot(ab);
        if length_sq <= EPS_MM * EPS_MM {
            return None;
        }
        let lo = Point::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Point::new(a.x.max(b.x), a.y.max(b.y));
        // Where the path meets a wall: its crossings, each with the
        // segment crossed, and the feet of the wall ends within the slack
        // of it.
        let mut cuts: Vec<(f64, Option<usize>)> = vec![(0.0, None), (1.0, None)];
        for index in self.near(lo, hi, slack_mm) {
            let Some(&(c, d)) = self.segments.get(index) else {
                continue;
            };
            let cd = d - c;
            let denom = ab.cross(cd);
            if denom.abs() > 1e-12 {
                let t = (c - a).cross(cd) / denom;
                let u = (c - a).cross(ab) / denom;
                if (-1e-9..=1.0 + 1e-9).contains(&u) && (0.0..=1.0).contains(&t) {
                    cuts.push((t, Some(index)));
                }
            }
            for end in [c, d] {
                if point_segment(end, a, b) <= slack_mm {
                    cuts.push((((end - a).dot(ab) / length_sq).clamp(0.0, 1.0), None));
                }
            }
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let at = |t: f64| a + ab * t;
        let mid = cuts.windows(2).find_map(|w| match w {
            [(s, _), (t, _)] if t - s > 1e-12 && !self.holds(at((s + t) / 2.0), slack_mm) => {
                Some((s + t) / 2.0)
            }
            _ => None,
        })?;
        // The crossings the path leaves by and comes back in by.
        let exit = cuts
            .iter()
            .rev()
            .find_map(|&(u, seg)| seg.filter(|_| u <= mid).map(|seg| (u, seg)));
        let entry = cuts
            .iter()
            .find_map(|&(u, seg)| seg.filter(|_| u >= mid).map(|seg| (u, seg)));
        let (t0, t1) = (exit.map_or(0.0, |e| e.0), entry.map_or(1.0, |e| e.0));
        let (t, outside_mm) = (0..=32)
            .map(|k| t0 + (t1 - t0) * f64::from(k) / 32.0)
            .chain([mid])
            .map(|u| (u, self.clearance_mm(at(u))))
            .fold((mid, f64::NEG_INFINITY), |best, x| {
                if x.1 > best.1 { x } else { best }
            });
        let corner = exit
            .zip(entry)
            .and_then(|((u0, i), (u1, j))| self.cut_corner(a, b, (at(u0), i), (at(u1), j)));
        Some(Excursion {
            t,
            outside_mm,
            corner,
        })
    }

    /// The wall vertex the path from `a` to `b` cuts across where it
    /// leaves the regions at `exit` (the point and the segment crossed)
    /// and comes back at `entry` through another segment of the same loop
    /// (#463): of the loop's two ways between them, the one that closes
    /// the smaller polygon with the path (outside an outer, the bay the
    /// path crosses, not the region round it; in a hole, either side), and
    /// on it the vertex farthest from the path's line, with the unit
    /// vector that bisects the region's side of it (away from both its
    /// walls; straight out from the path's line where they run straight
    /// on). `None` when the two are on different loops or the same
    /// segment.
    fn cut_corner(
        &self,
        a: Point,
        b: Point,
        (x0, i): (Point, usize),
        (x1, j): (Point, usize),
    ) -> Option<Corner> {
        let (&(first, len), &(first_j, _)) = (self.rings.get(i)?, self.rings.get(j)?);
        if first != first_j || i == j || len == 0 {
            return None;
        }
        let vertex = |k: usize| self.segments.get(first + k % len).map(|&(p, _)| p);
        // Segment `k` ends at vertex `k + 1`: the way on from the exit
        // visits vertices `i + 1 ..= j`, the way back `j + 1 ..= i`.
        let way = |from: usize, to: usize| -> Vec<usize> {
            let count = (to + len - from) % len;
            (1..=count).map(|k| (from + k) % len).collect()
        };
        let area = |start: Point, ks: &[usize], end: Point| {
            let ring: Vec<Point> = std::iter::once(start)
                .chain(ks.iter().filter_map(|&k| vertex(k)))
                .chain(std::iter::once(end))
                .collect();
            let next = ring.iter().cycle().skip(1);
            ring.iter()
                .zip(next)
                .map(|(p, q)| p.x * q.y - q.x * p.y)
                .sum::<f64>()
                .abs()
        };
        let (i, j) = (i - first, j - first);
        let on = way(i, j);
        let back = way(j, i);
        let chunk = if area(x0, &on, x1) <= area(x1, &back, x0) {
            on
        } else {
            back
        };
        let ab = b - a;
        let length = ab.length();
        if length <= EPS_MM {
            return None;
        }
        let (k, p) = chunk
            .into_iter()
            .filter_map(|k| vertex(k).map(|p| (k, p)))
            .max_by(|x, y| (ab.cross(x.1 - a).abs()).total_cmp(&ab.cross(y.1 - a).abs()))?;
        let (prev, next) = (vertex(k + len - 1)?, vertex(k + 1)?);
        let foot = a + ab * ((p - a).dot(ab) / (length * length));
        let out = (p - foot).normalized()?;
        // The path cuts across `p`, so the region is on the far side of it
        // from its two walls, which run back toward the path.
        let bisector = match ((prev - p).normalized(), (next - p).normalized()) {
            (Some(u), Some(v)) => Vec2::new(-u.x - v.x, -u.y - v.y).normalized(),
            _ => None,
        }
        .filter(|w| w.dot(out) > 0.0)
        .unwrap_or(out);
        Some(Corner { p, bisector })
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

/// A male Inlay's open band edge (#448; `docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 4): beyond these walls the board stands to
/// the stock top, so the V-bit's cone may not cross them below it. The
/// widest disc a cone cuts below the stock top is the one at the stock
/// top, `(top − z) · slope` across for a tip at `z`.
#[derive(Debug, Clone)]
pub(crate) struct OpenEdgeWalls {
    /// The band's edge, `B`.
    pub walls: Walls,
    /// The stock top, mm.
    pub top_z_mm: f64,
    /// The V-bit's `tan(α/2)`.
    pub slope: f64,
}

impl OpenEdgeWalls {
    /// Whether the straight move `a`–`b` keeps the cone's disc at the
    /// stock top within the walls all along it, exactly
    /// ([`Walls::least_room`]).
    pub(crate) fn allows(&self, a: Point3, b: Point3) -> bool {
        self.walls.least_room(a, b, self.top_z_mm, self.slope).0 >= 0.0
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
