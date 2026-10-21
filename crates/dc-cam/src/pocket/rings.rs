// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts

//! The offset ring tree of a pocket (`docs/specs/phase-2/design.md`,
//! "Algorithms → Pocket", steps 2 and 6): [`inset_region`] (upstream
//! `buildInsetRegions`) and [`RingTree`] (upstream `buildOffsetRegionTree`),
//! and the inner-first walk over it (upstream `cutOffsetRegionNode` with
//! `inner-first`).
//!
//! Joins: upstream offsets the outer with a miter join and islands with a
//! round one. `dc_geometry::offset` (cavalier_contours) has round joins
//! only. An inward offset of the outer leaves its convex corners sharp
//! whatever the join, and at a reflex corner the round join keeps the tool
//! centre exactly a radius from the corner, the closest it may come, so the
//! wall is reached as far as the tool allows either way. Islands are round
//! as upstream.
//!
//! Upstream builds the tree recursively; it is built and walked with an
//! explicit stack here, so a fine step-over on a large pocket cannot
//! exhaust the call stack.
//!
//! Above half the diameter the rings alone leave material standing (#258):
//! a ring sweeps `r` either side, so a point more than `r` from its node's
//! ring and more than `r` from every child is never cut, the middle of a
//! region too small for another step-over, or a sharp corner of one with a
//! child. [`RingTree::build`] gives each such node residual rings that
//! reach it, as upstream's trochoidal pocket does for its leaves
//! (`appendResidualCoreGuides`).

use dc_geometry::{
    Arc, Contour, EPS_MM, Point, Polyline, Region, Segment, Vertex, difference, nest, offset,
};
use std::f64::consts::TAU;

use super::rest::{grow_all, without_noise};
use crate::sequence::nearest_of;

/// `region` inset by `inset_mm`: its outer offset inward and its islands
/// outward by `inset_mm`, the islands then cut from the outer. An island
/// can split the result into several regions; nothing left is an empty
/// `Vec`. Outers come back counter-clockwise and holes clockwise (mm,
/// Y-up).
///
/// When the offset islands lie apart inside the offset outer, the result
/// is built from the offsets directly and keeps their arcs. When they
/// touch or cross, the islands are cut with [`dc_geometry::difference`],
/// which flattens arcs to chords within `tolerance_mm`.
#[must_use]
pub fn inset_region(region: &Region, inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outers = offset(region.outer(), -inset_mm);
    if outers.is_empty() {
        return Vec::new();
    }
    // `offset` keeps a hole's clockwise winding and grows it for a
    // positive delta.
    let grown: Vec<Contour> = region
        .holes()
        .iter()
        .flat_map(|hole| offset(hole, inset_mm))
        .collect();
    if grown.is_empty() {
        return outers.into_iter().filter_map(single_region).collect();
    }
    let mut all = outers.clone();
    all.extend(grown.iter().cloned());
    if let Ok((regions, warnings)) = nest(all) {
        let holes: usize = regions.iter().map(|r| r.holes().len()).sum();
        // A grown island that encloses the shrunk outer nests as the outer
        // of an inverted annulus; only the offset outers may be outers.
        let clean = warnings.is_empty()
            && regions.len() == outers.len()
            && holes == grown.len()
            && regions
                .iter()
                .all(|r| r.depth() == 0 && outers.contains(r.outer()));
        if clean {
            return regions;
        }
    }
    let subject: Vec<Region> = outers.into_iter().filter_map(single_region).collect();
    let clip: Vec<Region> = grown
        .iter()
        .filter_map(|c| single_region(c.reversed()))
        .collect();
    difference(&subject, &clip, tolerance_mm)
}

/// The region `contour` alone encloses.
fn single_region(contour: Contour) -> Option<Region> {
    nest(vec![contour])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

/// What the rings of a node of `region` whose children are `children`
/// leave uncut, as regions whose rings cut it (#258): the points more than
/// `r` from the node's rings and from every child, which the walk's rings
/// never reach when the step-over is above `r`. Each ring sweeps `r` either
/// side, and a child's subtree clears the child, so the rest of the node
/// is cut within `r` of its rings and of its children. Every point the
/// step-over inset leaves out lies under a step-over (at most `2r`) from
/// the node's rings, so under `r` from the edge of the node inset by `r`:
/// one ring round each piece of what is left, a leaf, reaches all of it.
///
/// Measured `tolerance_mm` further in on both sides, the node inset by
/// `r + tolerance_mm` less the children grown by `r + tolerance_mm`, so
/// at step-over `2r`, where the two edges meet along a straight wall, no
/// sliver is left between them; the pieces' rings still sweep the
/// tolerance-wide strips this leaves out. Pieces thinner than twice the
/// tolerance are noise, as rest machining's are. Empty when nothing is
/// left; at step-over `r` or less nothing ever is.
///
/// Upstream (`appendResidualCoreGuides`) adds one ring inset by the
/// shortfall to each leaf of a trochoidal pocket only, its contour pocket
/// having the same gap above half the diameter; the corners of a node with
/// children are left there.
#[must_use]
pub fn residual_regions(
    region: &Region,
    children: &[Region],
    radius_mm: f64,
    tolerance_mm: f64,
) -> Vec<Region> {
    let reach_mm = radius_mm + tolerance_mm;
    let deep = inset_region(region, reach_mm, tolerance_mm);
    if deep.is_empty() {
        return deep;
    }
    let left = if children.is_empty() {
        deep
    } else {
        difference(
            &deep,
            &grow_all(children, reach_mm, tolerance_mm),
            tolerance_mm,
        )
    };
    without_noise(left, tolerance_mm)
}

/// The rings of one tool-centre region: the region itself at the root,
/// and below each node the regions [`inset_region`] leaves of it at one
/// step-over, until nothing is left, with the residual nodes of
/// [`residual_regions`] beside them.
#[derive(Debug, Clone, PartialEq)]
pub struct RingTree {
    /// Every node; the root is node 0.
    pub nodes: Vec<RingNode>,
}

/// One node of a [`RingTree`].
#[derive(Debug, Clone, PartialEq)]
pub struct RingNode {
    /// The node's region; its outer and holes are its rings.
    pub region: Region,
    /// Indices of the node's children in [`RingTree::nodes`].
    pub children: Vec<usize>,
    /// A residual node (#258): a leaf that reaches what its parent's rings
    /// and its siblings leave, not an inset of its parent.
    pub residual: bool,
}

/// Which wall of its node a ring is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    /// The node's outer: the pocket wall, or a ring stepping in from it.
    Outer,
    /// One of the node's holes: an island, or a ring stepping out from it.
    Hole,
}

/// One ring in cut order: the contour as the node holds it (outer CCW,
/// hole CW), which wall it is, and the node it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    /// The ring, in its node's winding.
    pub contour: Contour,
    /// Outer or hole of its node.
    pub kind: RingKind,
    /// Index of the node in [`RingTree::nodes`].
    pub node: usize,
    /// The point the ring starts and ends at: its point nearest the end of
    /// the ring before it ([`nearest`]), a vertex or a point inside a
    /// segment, which [`split_at`] makes a vertex of the ring's path.
    pub start: Point,
}

/// Where a wall pass goes in a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallRings {
    /// The root's rings after every other ring.
    Last,
    /// The root's rings before every other ring.
    First,
    /// The root's rings are not cut.
    Omitted,
}

impl RingTree {
    /// The tree of `root` at step-over `stepover_mm` for a tool of radius
    /// `radius_mm`, or `None` when it would have more than `max_nodes`
    /// nodes. Each node's children are its [`inset_region`] by the
    /// step-over, then its [`residual_regions`] as residual leaves.
    #[must_use]
    pub fn build(
        root: Region,
        stepover_mm: f64,
        radius_mm: f64,
        tolerance_mm: f64,
        max_nodes: usize,
    ) -> Option<Self> {
        let mut nodes = vec![RingNode {
            region: root,
            children: Vec::new(),
            residual: false,
        }];
        let mut pending = vec![0usize];
        while let Some(index) = pending.pop() {
            let (children, residual) = match nodes.get(index) {
                Some(node) => {
                    let children = inset_region(&node.region, stepover_mm, tolerance_mm);
                    let residual = if stepover_mm > radius_mm + tolerance_mm {
                        residual_regions(&node.region, &children, radius_mm, tolerance_mm)
                    } else {
                        Vec::new()
                    };
                    (children, residual)
                }
                None => continue,
            };
            let mut ids = Vec::with_capacity(children.len() + residual.len());
            let tagged = children
                .into_iter()
                .map(|r| (r, false))
                .chain(residual.into_iter().map(|r| (r, true)));
            for (region, residual) in tagged {
                if nodes.len() >= max_nodes {
                    return None;
                }
                ids.push(nodes.len());
                if !residual {
                    pending.push(nodes.len());
                }
                nodes.push(RingNode {
                    region,
                    children: Vec::new(),
                    residual,
                });
            }
            if let Some(node) = nodes.get_mut(index) {
                node.children = ids;
            }
        }
        Some(Self { nodes })
    }

    /// Every ring, innermost first (upstream `inner-first`): a post-order
    /// walk that, at each node, visits the child whose outer comes nearest
    /// `anchor` (the end of the last ring, ties by child order), then cuts
    /// the node's own rings, nearest first. The root's rings go where
    /// `wall` says; with the root's rings omitted, so are its residual
    /// nodes, which reach only what those rings would leave. `anchor` is
    /// updated to each ring's point nearest it, which is where that ring
    /// will start and end.
    pub fn walk(&self, wall: WallRings, anchor: &mut Option<Point>) -> Vec<Ring> {
        self.walk_keeping(wall, anchor, &|_| true)
    }

    /// [`RingTree::walk`] over the rings `keep` accepts: a ring it refuses
    /// is not cut, and the next ring starts nearest the end of the last one
    /// kept. Rest machining refuses the rings that cut only air (#167).
    pub fn walk_keeping(
        &self,
        wall: WallRings,
        anchor: &mut Option<Point>,
        keep: &dyn Fn(&Ring) -> bool,
    ) -> Vec<Ring> {
        let mut out = Vec::new();
        if wall == WallRings::First {
            self.push_rings(0, anchor, keep, &mut out);
        }
        let mut first = self.children(0);
        if wall == WallRings::Omitted {
            first.retain(|&c| self.nodes.get(c).is_some_and(|n| !n.residual));
        }
        // Frames of (node, children not yet visited).
        let mut stack: Vec<(usize, Vec<usize>)> = vec![(0, first)];
        while let Some((node, remaining)) = stack.last_mut() {
            if remaining.is_empty() {
                let node = *node;
                stack.pop();
                if node != 0 || wall == WallRings::Last {
                    self.push_rings(node, anchor, keep, &mut out);
                }
                continue;
            }
            let pick = nearest_of(remaining.iter().enumerate().map(|(i, &n)| {
                let d = self
                    .nodes
                    .get(n)
                    .map_or(f64::INFINITY, |n| nearest(n.region.outer(), *anchor).1);
                (i, d)
            }));
            let Some(child) = pick.map(|i| remaining.remove(i)) else {
                break;
            };
            stack.push((child, self.children(child)));
        }
        out
    }

    fn children(&self, node: usize) -> Vec<usize> {
        self.nodes
            .get(node)
            .map_or_else(Vec::new, |n| n.children.clone())
    }

    /// Appends `node`'s outer and holes that `keep` accepts, nearest
    /// `anchor` first.
    fn push_rings(
        &self,
        node: usize,
        anchor: &mut Option<Point>,
        keep: &dyn Fn(&Ring) -> bool,
        out: &mut Vec<Ring>,
    ) {
        let Some(n) = self.nodes.get(node) else {
            return;
        };
        let ring = |contour: &Contour, kind| Ring {
            contour: contour.clone(),
            kind,
            node,
            start: Point::new(0.0, 0.0),
        };
        let mut rings: Vec<Ring> = std::iter::once(ring(n.region.outer(), RingKind::Outer))
            .chain(n.region.holes().iter().map(|h| ring(h, RingKind::Hole)))
            .filter(|r| keep(r))
            .collect();
        while !rings.is_empty() {
            let pick = nearest_of(
                rings
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (i, nearest(&r.contour, *anchor).1)),
            )
            .unwrap_or(0);
            let mut ring = rings.remove(pick);
            ring.start = nearest(&ring.contour, *anchor).0;
            *anchor = Some(ring.start);
            out.push(ring);
        }
    }
}

/// Where a ring starts: the point of `contour` nearest `anchor` among its
/// vertices and along its arcs, and its distance; the first vertex and 0
/// with no anchor. Upstream (`contourNearestVertexIndex`) takes the
/// nearest vertex of a Clipper polygon, whose arcs are sampled densely, so
/// on an arc its nearest vertex is in effect the nearest point, and on a
/// straight edge it is a corner. DreamCarve keeps arcs, so a circle has
/// two vertices and its nearest vertex can be far from its nearest point
/// (#196); the nearest point of an arc stands in for upstream's dense
/// vertices. A straight edge offers only its corners, as upstream's does
/// (the `pocket-offset-rect` golden case links its rings corner to
/// corner). A point within [`EPS_MM`] of an arc's end is that end, so no
/// sliver arc is made. Ties go to the earlier segment, points within
/// [`crate::sequence::NEAREST_TIE_MM`] of the nearest included (#215).
#[must_use]
pub fn nearest(contour: &Contour, anchor: Option<Point>) -> (Point, f64) {
    let polyline = contour.polyline();
    let first = polyline
        .vertices()
        .first()
        .map_or(Point::new(0.0, 0.0), |v| v.point());
    let Some(anchor) = anchor else {
        return (first, 0.0);
    };
    let candidates = std::iter::once(first).chain(polyline.segments().map(|segment| {
        let mut point = if segment.arc().is_some() {
            closest_on_segment(segment, anchor)
        } else {
            segment.start
        };
        for end in [segment.start, segment.end] {
            if point.distance_to(end) <= EPS_MM {
                point = end;
            }
        }
        point
    }));
    let best = nearest_of(candidates.map(|p| (p, p.distance_to(anchor)))).unwrap_or(first);
    (best, best.distance_to(anchor))
}

/// The point of `segment` nearest `p`, following an arc.
fn closest_on_segment(segment: Segment, p: Point) -> Point {
    let nearer_end = || {
        if p.distance_to(segment.start) <= p.distance_to(segment.end) {
            segment.start
        } else {
            segment.end
        }
    };
    match segment.arc() {
        Some(arc) => {
            let v = p - arc.centre;
            match v.normalized() {
                Some(unit) if sweep_to(arc, v.y.atan2(v.x)).is_some() => {
                    arc.centre + unit * arc.radius_mm
                }
                _ => nearer_end(),
            }
        }
        None => {
            let d = segment.end - segment.start;
            let len2 = d.dot(d);
            if len2 <= EPS_MM * EPS_MM {
                return segment.start;
            }
            let t = ((p - segment.start).dot(d) / len2).clamp(0.0, 1.0);
            segment.start + d * t
        }
    }
}

/// The signed sweep from `arc`'s start to the direction `angle_rad` from
/// its centre, when that direction lies within the arc.
fn sweep_to(arc: Arc, angle_rad: f64) -> Option<f64> {
    let from_start = if arc.sweep_rad > 0.0 {
        (angle_rad - arc.start_angle_rad).rem_euclid(TAU)
    } else {
        (arc.start_angle_rad - angle_rad).rem_euclid(TAU)
    };
    (from_start <= arc.sweep_rad.abs()).then(|| from_start.copysign(arc.sweep_rad))
}

/// The closed `path` with a vertex at `at`, a point on it: the segment
/// `at` lies inside (more than [`EPS_MM`] from both its ends) is split
/// there, an arc into two arcs of the same circle. Unchanged when `at` is
/// already a vertex or is on no segment.
#[must_use]
pub fn split_at(path: &Polyline, at: Point) -> Polyline {
    let vertices = path.vertices();
    if vertices.iter().any(|v| v.point().distance_to(at) <= EPS_MM) {
        return path.clone();
    }
    for (index, segment) in path.segments().enumerate() {
        if closest_on_segment(segment, at).distance_to(at) > EPS_MM {
            continue;
        }
        // The bulges of the two parts: tan(θ/4) of each part's sweep.
        let (before, after) = match segment.arc() {
            Some(arc) => {
                let v = at - arc.centre;
                let Some(sweep) = sweep_to(arc, v.y.atan2(v.x)) else {
                    continue;
                };
                ((sweep / 4.0).tan(), ((arc.sweep_rad - sweep) / 4.0).tan())
            }
            None => (0.0, 0.0),
        };
        let mut out = vertices.to_vec();
        if let Some(v) = out.get_mut(index) {
            v.bulge = before;
        }
        out.insert((index + 1).min(out.len()), Vertex::new(at.x, at.y, after));
        return Polyline::new(out, path.is_closed());
    }
    path.clone()
}

/// The point of `path` nearest `p`, along its lines and arcs, and its
/// distance; `None` for a path with no segment. Ties go to the earlier
/// segment.
#[must_use]
pub fn closest_point(path: &Polyline, p: Point) -> Option<(Point, f64)> {
    path.segments()
        .map(|segment| {
            let q = closest_on_segment(segment, p);
            (q, q.distance_to(p))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The part of the closed `ring` from `from` to `to`, two points on it,
/// the shorter way round, as an open path keeping the ring's arcs (#202).
/// Ties go the ring's own way. `None` when either point is not on it.
#[must_use]
pub fn shorter_way(ring: &Polyline, from: Point, to: Point) -> Option<Polyline> {
    let forward = way_along(ring, from, to)?;
    let backward = way_along(&ring.reversed(), from, to)?;
    Some(if backward.length_mm() < forward.length_mm() - EPS_MM {
        backward
    } else {
        forward
    })
}

/// The part of the closed `ring` from `from` to `to` in its own direction.
fn way_along(ring: &Polyline, from: Point, to: Point) -> Option<Polyline> {
    let split = split_at(&split_at(ring, from), to);
    let vertices = split.vertices();
    let at = |p: Point| {
        vertices
            .iter()
            .position(|v| v.point().distance_to(p) <= EPS_MM)
    };
    let (first, last) = (at(from)?, at(to)?);
    let mut out = Vec::new();
    let mut index = first;
    for _ in 0..vertices.len() {
        out.push(*vertices.get(index)?);
        if index == last {
            break;
        }
        index = (index + 1) % vertices.len();
    }
    if let Some(end) = out.last_mut() {
        end.bulge = 0.0;
    }
    Some(Polyline::new(out, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pocket::ring_path;
    use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, area_mm2, assert_approx_eq};
    use dc_project::CutDirection;

    fn region(polylines: Vec<Polyline>) -> Region {
        let contours = polylines
            .into_iter()
            .map(|p| Contour::new(p).unwrap())
            .collect();
        nest(contours).unwrap().0.remove(0)
    }

    #[test]
    fn inset_keeps_arcs_when_island_is_clear() {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
            Polyline::circle(Point::new(50.0, 25.0), 10.0),
        ]);
        let out = inset_region(&r, 3.0, FLATTEN_TOLERANCE_DEFAULT_MM);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes().len(), 1);
        let hole = out[0].holes()[0].polyline();
        assert!(
            hole.segments().any(|s| !s.is_line()),
            "the island stays round"
        );
        assert_approx_eq!(
            area_mm2(&out),
            94.0 * 44.0 - std::f64::consts::PI * 13.0 * 13.0,
            1e-6
        );
    }

    #[test]
    fn inset_splits_around_a_bar() {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
            Polyline::rect(Point::new(45.0, 1.0), 10.0, 48.0),
        ]);
        let out = inset_region(&r, 3.0, FLATTEN_TOLERANCE_DEFAULT_MM);
        assert_eq!(out.len(), 2, "{out:?}");
    }

    /// A band thinner than twice the inset (what a rough Pocket's
    /// allowance leaves for rest machining) insets to nothing. The grown
    /// island then encloses the shrunk outer, and must not be read back
    /// as the outer of an inverted annulus, which grows on every inset.
    #[test]
    fn inset_of_a_thin_band_is_empty() {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
            Polyline::rect(Point::new(1.0, 1.0), 38.0, 38.0),
        ]);
        let out = inset_region(&r, 1.2, FLATTEN_TOLERANCE_DEFAULT_MM);
        assert!(out.is_empty(), "{out:?}");
        let tree = RingTree::build(r, 1.2, 1.5, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
        assert_eq!(tree.nodes.len(), 1);
    }

    /// #196: a ring starts at its point nearest the anchor, on an arc too,
    /// not at its nearest vertex. A circle of radius 10 about the origin
    /// has vertices at (±10, 0); from (12, 1) its nearest point is at
    /// θ = atan2(1, 12), and the ring's contour gains a vertex there.
    #[test]
    fn ring_starts_at_nearest_point_of_an_arc() {
        let r = region(vec![Polyline::circle(Point::new(0.0, 0.0), 10.0)]);
        let tree = RingTree::build(r, 50.0, 25.0, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
        let mut anchor = Some(Point::new(12.0, 1.0));
        let rings = tree.walk(WallRings::Last, &mut anchor);
        assert_eq!(rings.len(), 1);
        let theta = 1.0_f64.atan2(12.0);
        let want = Point::new(10.0 * theta.cos(), 10.0 * theta.sin());
        assert!(
            rings[0].start.distance_to(want) <= EPS_MM,
            "starts at {:?}, want {want:?}",
            rings[0].start
        );
        assert_eq!(anchor, Some(rings[0].start));
        // The ring's path starts there, split into arcs of the same
        // circle: the same length and area.
        let start = rings[0].start;
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let path = ring_path(rings[0].clone(), direction);
            let first = path.vertices().first().unwrap().point();
            assert!(first.distance_to(start) <= EPS_MM, "{path:?}");
            assert_eq!(path.vertices().len(), 3, "{path:?}");
            assert_approx_eq!(path.length_mm(), TAU * 10.0, 1e-9);
            assert_approx_eq!(
                path.signed_area_mm2().abs(),
                std::f64::consts::PI * 100.0,
                1e-9
            );
        }
    }

    /// Splitting a line keeps both halves straight; a point that is a
    /// vertex, or on no segment, changes nothing.
    #[test]
    fn split_at_a_line_and_at_a_vertex() {
        let rect = Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0);
        let split = split_at(&rect, Point::new(5.0, 0.0));
        assert_eq!(split.vertices().len(), 5);
        assert!(split.segments().all(|s| s.is_line()));
        assert_approx_eq!(split.length_mm(), 60.0, 1e-9);
        assert_eq!(split_at(&rect, Point::new(20.0, 0.0)), rect);
        assert_eq!(split_at(&rect, Point::new(5.0, 5.0)), rect);
        // A straight edge offers only its corners, as upstream's polygon
        // does: from inside the rect, its nearest corner.
        let r = region(vec![rect]);
        let (p, d) = nearest(r.outer(), Some(Point::new(5.0, 2.0)));
        assert!(p.distance_to(Point::new(0.0, 0.0)) <= EPS_MM, "{p:?}");
        assert_approx_eq!(d, 29.0_f64.sqrt(), 1e-9);
    }

    /// #258: a 6.35 mm tool at step-over 1.0 (`s = 6.35`, `r = 3.175`). A
    /// tool-centre circle of radius 5.325 has no room for another ring, and
    /// its ring reaches only 3.175 in; it gets one residual ring, the circle
    /// inset by `r` and the tolerance, which reaches the centre. At radius
    /// 3.0 the ring reaches the centre and none is added; nor is one at
    /// step-over 0.5, whatever the radius.
    #[test]
    fn leaf_gets_a_residual_ring_only_when_needed() {
        let tol = FLATTEN_TOLERANCE_DEFAULT_MM;
        let circle = |radius_mm| region(vec![Polyline::circle(Point::new(0.0, 0.0), radius_mm)]);
        let tree = RingTree::build(circle(5.325), 6.35, 3.175, tol, 100).unwrap();
        assert_eq!(tree.nodes.len(), 2, "{tree:?}");
        assert_eq!(tree.nodes[0].children, [1]);
        let residual = &tree.nodes[1];
        assert!(residual.residual && residual.children.is_empty());
        assert_approx_eq!(
            residual.region.area_mm2(),
            std::f64::consts::PI * (5.325 - 3.175 - tol).powi(2),
            1e-6
        );
        let rings = tree.walk(WallRings::Last, &mut None);
        assert_eq!(rings.iter().map(|r| r.node).collect::<Vec<_>>(), [1, 0]);
        // With the wall pass omitted the residual ring goes with it.
        assert!(tree.walk(WallRings::Omitted, &mut None).is_empty());

        let tree = RingTree::build(circle(3.0), 6.35, 3.175, tol, 100).unwrap();
        assert_eq!(tree.nodes.len(), 1);
        let tree = RingTree::build(circle(5.325), 3.175, 3.175, tol, 100).unwrap();
        assert!(tree.nodes.iter().all(|n| !n.residual), "{tree:?}");
    }

    /// #258: at step-over 1.0 the child of a rect has its corners `2r` in
    /// from each side, so a point `1.1r` from both sides of a corner is
    /// more than `r` from the ring and from the child (`0.9·√2·r`). The
    /// 100 × 50 pocket's tool-centre rect gets one residual ring in each
    /// corner beside its child. At 0.8 the child's corner is near enough
    /// (a right angle needs `s ≤ (1 + √2/2)·r`) and none is added.
    #[test]
    fn square_corners_get_residual_rings_at_full_step_over() {
        let tol = FLATTEN_TOLERANCE_DEFAULT_MM;
        let rect = || region(vec![Polyline::rect(Point::new(0.0, 0.0), 93.65, 43.65)]);
        let tree = RingTree::build(rect(), 6.35, 3.175, tol, 1000).unwrap();
        let root = &tree.nodes[0];
        let residual: Vec<&RingNode> = root
            .children
            .iter()
            .map(|&c| &tree.nodes[c])
            .filter(|n| n.residual)
            .collect();
        assert_eq!(root.children.len(), 5, "{root:?}");
        assert_eq!(residual.len(), 4);
        for corner in [(0.0, 0.0), (93.65, 0.0), (93.65, 43.65), (0.0, 43.65)] {
            let corner = Point::new(corner.0, corner.1);
            assert!(
                residual.iter().any(|n| {
                    n.region
                        .outer()
                        .polyline()
                        .vertices()
                        .iter()
                        .all(|v| v.point().distance_to(corner) < 2.5 * 3.175)
                }),
                "no residual ring at {corner:?}"
            );
        }
        let tree = RingTree::build(rect(), 0.8 * 6.35, 3.175, tol, 1000).unwrap();
        assert!(tree.nodes.iter().all(|n| !n.residual));
    }

    #[test]
    fn tree_of_a_rect_is_a_chain() {
        let r = region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]);
        let tree = RingTree::build(r, 2.0, 2.5, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
        // Half-height 5: insets of 2 and 4 remain, 6 does not.
        assert_eq!(tree.nodes.len(), 3);
        let mut anchor = None;
        let rings = tree.walk(WallRings::Last, &mut anchor);
        let nodes: Vec<usize> = rings.iter().map(|r| r.node).collect();
        assert_eq!(nodes, [2, 1, 0]);
        assert!(
            RingTree::build(
                region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]),
                2.0,
                2.5,
                FLATTEN_TOLERANCE_DEFAULT_MM,
                2
            )
            .is_none()
        );
    }
}
