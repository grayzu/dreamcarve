// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/medialAxis.ts (SegmentDistanceGrid)

//! Exact distance from a point to the sampled boundary, through a uniform
//! grid over its short segments (`docs/specs/phase-3/design.md`,
//! "Algorithms → Medial axis", step 6). Upstream keeps the segments in a
//! flat `Float64Array` and the buckets in a sparse array; here they are a
//! `Vec` of point pairs and a dense `Vec` of buckets, visited in the same
//! order, so a query sees the segments in upstream's order.

use crate::point::Point;

/// The grid never has more cells than this on a side, whatever the cell
/// size asked for, so a tiny step on a large shape stays bounded.
const MAX_CELLS_PER_SIDE: f64 = 512.0;

/// Where the boundary comes near a point ([`SegmentDistanceGrid::contacts`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Contact {
    /// The segment's point nearest the query point, mm.
    pub q: Point,
    /// Its distance from the query point, mm.
    pub d: f64,
    /// The segment's index in the grid's input.
    pub segment: usize,
    /// Where `q` lies along the segment, 0 at its start to 1 at its end.
    pub t: f64,
}

/// A uniform grid over boundary segments, each at most a sampling step
/// long, so a segment spans few cells. Queries expand ring by ring from the
/// query point's cell and stop once no farther ring can do better.
pub(super) struct SegmentDistanceGrid {
    segments: Vec<(Point, Point)>,
    /// Segment indices per cell, row-major.
    cells: Vec<Vec<usize>>,
    cols: usize,
    rows: usize,
    cell_mm: f64,
    min: Point,
}

impl SegmentDistanceGrid {
    /// A grid over `segments` with cells `cell_mm` square, or larger when
    /// that would need more than [`MAX_CELLS_PER_SIDE`] a side.
    pub(super) fn new(segments: Vec<(Point, Point)>, cell_mm: f64) -> Self {
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &(a, b) in &segments {
            min_x = min_x.min(a.x).min(b.x);
            max_x = max_x.max(a.x).max(b.x);
            min_y = min_y.min(a.y).min(b.y);
            max_y = max_y.max(a.y).max(b.y);
        }
        if segments.is_empty() {
            (min_x, min_y, max_x, max_y) = (0.0, 0.0, 0.0, 0.0);
        }
        let span_x = (max_x - min_x).max(1e-9);
        let span_y = (max_y - min_y).max(1e-9);
        let size = cell_mm
            .max(span_x / MAX_CELLS_PER_SIDE)
            .max(span_y / MAX_CELLS_PER_SIDE);
        let cols = count_of((span_x / size).ceil()).max(1);
        let rows = count_of((span_y / size).ceil()).max(1);
        let mut grid = Self {
            segments: Vec::new(),
            cells: vec![Vec::new(); cols.saturating_mul(rows)],
            cols,
            rows,
            cell_mm: size,
            min: Point::new(min_x, min_y),
        };
        for (s, &(a, b)) in segments.iter().enumerate() {
            let (c0, c1) = (grid.col(a.x.min(b.x)), grid.col(a.x.max(b.x)));
            let (r0, r1) = (grid.row(a.y.min(b.y)), grid.row(a.y.max(b.y)));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Some(bucket) = grid.cells.get_mut(r * cols + c) {
                        bucket.push(s);
                    }
                }
            }
        }
        grid.segments = segments;
        grid
    }

    fn col(&self, x: f64) -> usize {
        clamped_index((x - self.min.x) / self.cell_mm, self.cols)
    }

    fn row(&self, y: f64) -> usize {
        clamped_index((y - self.min.y) / self.cell_mm, self.rows)
    }

    /// The segments in the cells of ring `ring` about cell (`cc`, `cr`):
    /// the cells whose Chebyshev distance from it is `ring`, row by row,
    /// left to right, as upstream walks them.
    fn ring(&self, cc: usize, cr: usize, ring: usize) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.ring_indexed(cc, cr, ring).map(|(_, a, b)| (a, b))
    }

    /// [`Self::ring`], each segment with its index in the grid's input.
    fn ring_indexed(
        &self,
        cc: usize,
        cr: usize,
        ring: usize,
    ) -> impl Iterator<Item = (usize, Point, Point)> + '_ {
        let rows = cr.saturating_sub(ring)..=(cr + ring).min(self.rows.saturating_sub(1));
        rows.flat_map(move |r| {
            let edge_row = r + ring == cr || r == cr + ring;
            let left = cc.checked_sub(ring);
            let right = cc + ring;
            let cols: Vec<usize> = if edge_row {
                (cc.saturating_sub(ring)..=right.min(self.cols.saturating_sub(1))).collect()
            } else {
                left.into_iter()
                    .chain((ring > 0 && right < self.cols).then_some(right))
                    .collect()
            };
            cols.into_iter()
                .filter_map(move |c| self.cells.get(r * self.cols + c))
                .flatten()
                .filter_map(|&s| self.segments.get(s).map(|&(a, b)| (s, a, b)))
        })
    }

    /// The distance from `p` to the nearest segment, mm; infinite when
    /// there are none. `p` should lie in the segments' bounding box.
    pub(super) fn distance_to(&self, p: Point) -> f64 {
        let (cc, cr) = (self.col(p.x), self.row(p.y));
        let max_ring = self.cols + self.rows;
        let mut best = f64::INFINITY;
        for ring in 0..=max_ring {
            // Every cell of ring `ring` is at least (ring − 1) cells from a
            // point in cell (cc, cr); once `best` beats that, no farther
            // ring can improve it.
            if best <= (ring as f64 - 1.0) * self.cell_mm {
                break;
            }
            for (a, b) in self.ring(cc, cr, ring) {
                best = best.min(closest_on_segment(p, a, b).distance_to(p));
            }
        }
        best
    }

    /// True when the boundary within `clearance_mm · (1 + tol_ratio)` of
    /// `p` spans at least `min_angle_rad` as seen from `p`: some contact
    /// lies that far round from the nearest one. Two walls, a corner wedge
    /// or an osculating arc pass; the spokes curve flattening leaves, which
    /// see the boundary in one narrow direction, do not.
    pub(super) fn has_contact_spread(
        &self,
        p: Point,
        clearance_mm: f64,
        min_angle_rad: f64,
        tol_ratio: f64,
    ) -> bool {
        let (contacts, nearest) = self.contacts(p, clearance_mm, tol_ratio);
        let Some(nearest) = nearest.and_then(|i| contacts.get(i)) else {
            return false;
        };
        let u = (nearest.q - p) * (1.0 / nearest.d);
        let max_cos = min_angle_rad.cos();
        contacts
            .iter()
            .filter(|c| c.d >= 1e-12)
            .any(|c| (c.q - p).dot(u) / c.d <= max_cos)
    }

    /// The boundary within `clearance_mm · (1 + tol_ratio)` of `p`, one
    /// [`Contact`] per segment that comes that near, in upstream's order,
    /// and the index of the nearest (the first of equals); `None` when no
    /// segment does.
    pub(super) fn contacts(
        &self,
        p: Point,
        clearance_mm: f64,
        tol_ratio: f64,
    ) -> (Vec<Contact>, Option<usize>) {
        let limit = clearance_mm * (1.0 + tol_ratio) + 1e-9;
        let (cc, cr) = (self.col(p.x), self.row(p.y));
        let max_ring = (self.cols + self.rows).min(count_of((limit / self.cell_mm).ceil()) + 1);

        let mut contacts: Vec<Contact> = Vec::new();
        let mut best: Option<(usize, f64)> = None;
        for ring in 0..=max_ring {
            for (segment, a, b) in self.ring_indexed(cc, cr, ring) {
                let (q, t) = closest_on_segment_at(p, a, b);
                let d = q.distance_to(p);
                if d > limit {
                    continue;
                }
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((contacts.len(), d));
                }
                contacts.push(Contact { q, d, segment, t });
            }
        }
        (contacts, best.map(|(i, _)| i))
    }
}

/// The point of segment `a`–`b` nearest `p` (upstream's
/// `closestPointOnSegment`, with its 1e-18 guard on a zero-length segment).
fn closest_on_segment(p: Point, a: Point, b: Point) -> Point {
    closest_on_segment_at(p, a, b).0
}

/// [`closest_on_segment`] and where it lies along the segment, 0 at `a` to
/// 1 at `b`.
fn closest_on_segment_at(p: Point, a: Point, b: Point) -> (Point, f64) {
    let ab = b - a;
    let len_sq = ab.dot(ab);
    let t = if len_sq > 1e-18 {
        ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t, t)
}

/// `v` floored and clamped to `0..len` (upstream's `clampCol`/`clampRow`).
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

/// A non-negative whole `v` as a count, saturating; 0 for NaN or below.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Rust's float-to-int `as` saturates; NaN and negatives are 0"
)]
pub(super) fn count_of(v: f64) -> usize {
    if v.is_nan() || v <= 0.0 {
        0
    } else {
        v as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_segments() -> Vec<(Point, Point)> {
        let c = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect()
    }

    /// The grid's distance is the exact distance to the nearest segment,
    /// wherever the point lies relative to the cells.
    #[test]
    fn grid_distance_is_the_exact_segment_distance() {
        let grid = SegmentDistanceGrid::new(square_segments(), 0.5);
        for (p, d) in [
            (Point::new(5.0, 5.0), 5.0),
            (Point::new(1.0, 7.0), 1.0),
            (Point::new(9.75, 9.5), 0.25),
            (Point::new(0.0, 3.0), 0.0),
        ] {
            let got = grid.distance_to(p);
            assert!(
                (got - d).abs() < 1e-12,
                "distance at {p:?} is {got}, expected {d}"
            );
        }
    }

    /// The centre of a square sees four walls a quarter turn apart; a point
    /// near one wall, with the clearance of that wall, sees only it.
    #[test]
    fn contact_spread_needs_a_second_wall() {
        let grid = SegmentDistanceGrid::new(square_segments(), 0.5);
        let min_angle = 60.0_f64.to_radians();
        assert!(grid.has_contact_spread(Point::new(5.0, 5.0), 5.0, min_angle, 0.003));
        assert!(!grid.has_contact_spread(Point::new(5.0, 2.0), 2.0, min_angle, 0.003));
    }
}
