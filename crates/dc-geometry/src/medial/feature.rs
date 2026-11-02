// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The medial axis round a feature (#465; `docs/specs/phase-3/design.md`,
//! "Algorithms → Medial axis", step 6). The contact-spread filter keeps a
//! node only when its boundary contacts lie at least 60° apart as seen
//! from it, which drops the spokes curve flattening leaves. It also drops
//! the axis round the tip of a thin wedge of material standing into a
//! region (a drawn line's end, an eagle's claw): there the axis is the
//! curve equidistant from the tip and the wall beyond it, both seen in one
//! direction once the axis is a little way out. That axis is the only one
//! whose discs reach the wall beside the wedge, so without it the groove
//! stops short of the wall.
//!
//! What tells the two apart is the boundary between the two contacts. A
//! spoke's contacts are on two chords of one gently turning curve, which
//! between them turns by no more than the spread itself. Round a wedge the
//! boundary runs from the wall up the wedge's flank and round its tip: it
//! turns by far more than the spread, and the tip stands out from the
//! wall by `r · (1 − cos θ)` for a node of clearance `r` and spread `θ`.

use super::grid::{Contact, SegmentDistanceGrid};
use super::{CONTACT_SPREAD_RAD, CORNER_ANGLE_RAD, REACH_SLACK_MM};
use crate::point::Point;

/// Where one sampled boundary segment lies: its loop, and the flattened
/// edge it is a piece of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegmentTag {
    /// The loop's index, outer first.
    pub loop_index: usize,
    /// The edge's index: it runs from the loop's vertex `edge` to the next.
    pub edge: usize,
    /// True for the edge's first piece, which starts at vertex `edge`.
    pub first: bool,
    /// True for the edge's last piece, which ends at the next vertex.
    pub last: bool,
}

/// The sampled boundary's loops as [`has_feature_contact`] reads them.
pub(super) struct Boundary {
    /// One per segment of the distance grid, in its order.
    tags: Vec<SegmentTag>,
    /// Per loop, how far the boundary turns at each vertex, radians, 0 to
    /// π either way.
    turns: Vec<Vec<f64>>,
    /// Per loop, `prefix[k]` is the sum of `turns` before vertex `k`;
    /// one more entry than the loop has vertices.
    prefix: Vec<Vec<f64>>,
}

/// Where a contact lies on its loop, as a position on a cycle of twice
/// the loop's vertices: vertex `i` is `2i`, the inside of edge `e` is
/// `2e + 1`.
type Position = usize;

impl Boundary {
    /// The boundary of `loops` (cleaned, outer first), sampled into
    /// segments tagged `tags`.
    pub(super) fn new(loops: &[Vec<Point>], tags: Vec<SegmentTag>) -> Self {
        let turns: Vec<Vec<f64>> = loops.iter().map(|l| vertex_turns(l)).collect();
        let prefix = turns
            .iter()
            .map(|t| {
                std::iter::once(0.0)
                    .chain(t.iter().scan(0.0, |sum, &turn| {
                        *sum += turn;
                        Some(*sum)
                    }))
                    .collect()
            })
            .collect();
        Self {
            tags,
            turns,
            prefix,
        }
    }

    /// The loop and position of `contact`.
    fn locate(&self, contact: &Contact) -> Option<(usize, Position)> {
        let tag = self.tags.get(contact.segment)?;
        let count = self.turns.get(tag.loop_index)?.len();
        let position = if tag.first && contact.t <= 0.0 {
            2 * tag.edge
        } else if tag.last && contact.t >= 1.0 {
            2 * ((tag.edge + 1) % count.max(1))
        } else {
            2 * tag.edge + 1
        };
        Some((tag.loop_index, position))
    }

    /// The turn at the vertex at `position` of loop `index`, radians; 0
    /// inside an edge.
    fn turn_at(&self, index: usize, position: Position) -> f64 {
        if position % 2 == 1 {
            return 0.0;
        }
        self.turns
            .get(index)
            .and_then(|t| t.get(position / 2))
            .copied()
            .unwrap_or(0.0)
    }

    /// The sum of the turns at the vertices strictly between `from` and
    /// `to`, going forward round loop `index`; `None` when there is no
    /// vertex between them (the two lie on one edge, or on two edges that
    /// meet at one of them).
    fn turn_between(&self, index: usize, from: Position, to: Position) -> Option<f64> {
        let prefix = self.prefix.get(index)?;
        let count = prefix.len().checked_sub(1).filter(|&n| n > 0)?;
        let total = prefix.last().copied().unwrap_or(0.0);
        let to = if to <= from { to + 2 * count } else { to };
        // Vertices `first..=last`, counted on from vertex 0.
        let first = from / 2 + 1;
        let last = (to - 1) / 2;
        if last < first {
            return None;
        }
        let before =
            |k: usize| (k / count) as f64 * total + prefix.get(k % count).copied().unwrap_or(0.0);
        Some(before(last + 1) - before(first))
    }

    /// How far the boundary turns between contacts `a` and `b` (#465): at
    /// each vertex between them, the shorter way round, and at either
    /// contact that is itself a vertex. `None` when they lie on one edge or
    /// on two edges that meet: the two sides of a convex corner, or a
    /// corner and the edges into it. Infinite when they are on two loops.
    fn turn(&self, a: &Contact, b: &Contact) -> Option<f64> {
        let (loop_a, at_a) = self.locate(a)?;
        let (loop_b, at_b) = self.locate(b)?;
        if loop_a != loop_b {
            return Some(f64::INFINITY);
        }
        if at_a == at_b {
            return None;
        }
        let forward = self.turn_between(loop_a, at_a, at_b)?;
        let back = self.turn_between(loop_a, at_b, at_a)?;
        Some(forward.min(back) + self.turn_at(loop_a, at_a) + self.turn_at(loop_b, at_b))
    }

    /// True when `contact` is a vertex the boundary turns at by more than
    /// [`CORNER_ANGLE_RAD`]: a point of the boundary, not a wall.
    fn is_corner(&self, contact: &Contact) -> bool {
        self.locate(contact)
            .is_some_and(|(index, at)| self.turn_at(index, at) > CORNER_ANGLE_RAD)
    }
}

/// How far the boundary turns at each vertex of closed `points`, radians.
fn vertex_turns(points: &[Point]) -> Vec<f64> {
    let n = points.len();
    if n < 3 {
        return vec![0.0; n];
    }
    let prevs = points.iter().cycle().skip(n - 1);
    let nexts = points.iter().cycle().skip(1);
    prevs
        .zip(points)
        .zip(nexts)
        .map(|((&prev, &curr), &next)| {
            let (d1, d2) = (curr - prev, next - curr);
            d1.cross(d2).atan2(d1.dot(d2)).abs()
        })
        .collect()
}

/// True when the node at `p`, of clearance `clearance_mm`, is on the axis
/// round a feature (#465): it has a second contact, within the tolerance
/// band of [`SegmentDistanceGrid::contacts`], such that the boundary
/// between it and the nearest turns by at least [`CONTACT_SPREAD_RAD`]
/// ([`Boundary::turn`]), the nearer of the two stands out from the
/// other's wall by more than [`REACH_SLACK_MM`] (`r · (1 − cos θ)` for
/// spread `θ`), and the two are not both corners. Two corners are reached
/// by every node round each; a wall point beside a feature only by the
/// node this keeps.
pub(super) fn has_feature_contact(
    grid: &SegmentDistanceGrid,
    boundary: &Boundary,
    p: Point,
    clearance_mm: f64,
    tol_ratio: f64,
) -> bool {
    let (contacts, nearest) = grid.contacts(p, clearance_mm, tol_ratio);
    let Some(nearest) = nearest.and_then(|i| contacts.get(i)) else {
        return false;
    };
    if nearest.d < 1e-12 {
        return false;
    }
    let u = (nearest.q - p) * (1.0 / nearest.d);
    let nearest_corner = boundary.is_corner(nearest);
    contacts.iter().filter(|c| c.d >= 1e-12).any(|c| {
        let cos = ((c.q - p).dot(u) / c.d).clamp(-1.0, 1.0);
        clearance_mm * (1.0 - cos) > REACH_SLACK_MM
            && !(nearest_corner && boundary.is_corner(c))
            && boundary
                .turn(nearest, c)
                .is_some_and(|turn| turn >= CONTACT_SPREAD_RAD)
    })
}
