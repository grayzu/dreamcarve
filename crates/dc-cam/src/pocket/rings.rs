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

use dc_geometry::{Contour, Point, Region, difference, nest, offset};

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

/// The rings of one tool-centre region: the region itself at the root,
/// and below each node the regions [`inset_region`] leaves of it at one
/// step-over, until nothing is left.
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
    /// The vertex the ring starts and ends at: the one nearest the end of
    /// the ring before it.
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
    /// The tree of `root` at step-over `stepover_mm`, or `None` when it
    /// would have more than `max_nodes` nodes.
    #[must_use]
    pub fn build(
        root: Region,
        stepover_mm: f64,
        tolerance_mm: f64,
        max_nodes: usize,
    ) -> Option<Self> {
        let mut nodes = vec![RingNode {
            region: root,
            children: Vec::new(),
        }];
        let mut pending = vec![0usize];
        while let Some(index) = pending.pop() {
            let children = match nodes.get(index) {
                Some(node) => inset_region(&node.region, stepover_mm, tolerance_mm),
                None => continue,
            };
            let mut ids = Vec::with_capacity(children.len());
            for region in children {
                if nodes.len() >= max_nodes {
                    return None;
                }
                ids.push(nodes.len());
                pending.push(nodes.len());
                nodes.push(RingNode {
                    region,
                    children: Vec::new(),
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
    /// `wall` says. `anchor` is updated to each ring's start vertex nearest
    /// it, which is where that ring will start and end.
    pub fn walk(&self, wall: WallRings, anchor: &mut Option<Point>) -> Vec<Ring> {
        let mut out = Vec::new();
        if wall == WallRings::First {
            self.push_rings(0, anchor, &mut out);
        }
        // Frames of (node, children not yet visited).
        let mut stack: Vec<(usize, Vec<usize>)> = vec![(0, self.children(0))];
        while let Some((node, remaining)) = stack.last_mut() {
            if remaining.is_empty() {
                let node = *node;
                stack.pop();
                if node != 0 || wall == WallRings::Last {
                    self.push_rings(node, anchor, &mut out);
                }
                continue;
            }
            let pick = remaining
                .iter()
                .enumerate()
                .min_by(|(ia, a), (ib, b)| {
                    let d = |n: usize| {
                        self.nodes
                            .get(n)
                            .map_or(f64::INFINITY, |n| nearest(n.region.outer(), *anchor).1)
                    };
                    d(**a).total_cmp(&d(**b)).then(ia.cmp(ib))
                })
                .map(|(i, _)| i);
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

    /// Appends `node`'s outer and holes, nearest `anchor` first.
    fn push_rings(&self, node: usize, anchor: &mut Option<Point>, out: &mut Vec<Ring>) {
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
            .collect();
        while !rings.is_empty() {
            let pick = rings
                .iter()
                .enumerate()
                .min_by(|(ia, a), (ib, b)| {
                    nearest(&a.contour, *anchor)
                        .1
                        .total_cmp(&nearest(&b.contour, *anchor).1)
                        .then(ia.cmp(ib))
                })
                .map_or(0, |(i, _)| i);
            let mut ring = rings.remove(pick);
            ring.start = nearest(&ring.contour, *anchor).0;
            *anchor = Some(ring.start);
            out.push(ring);
        }
    }
}

/// The vertex of `contour` nearest `anchor` (upstream
/// `contourNearestVertexIndex`, the first on a tie) and its distance; the
/// first vertex and 0 with no anchor.
#[must_use]
pub fn nearest(contour: &Contour, anchor: Option<Point>) -> (Point, f64) {
    let vertices = contour.polyline().vertices();
    let first = vertices.first().map_or(Point::new(0.0, 0.0), |v| v.point());
    let Some(anchor) = anchor else {
        return (first, 0.0);
    };
    vertices
        .iter()
        .map(|v| (v.point(), v.point().distance_to(anchor)))
        .fold((first, f64::INFINITY), |best, candidate| {
            if candidate.1 < best.1 {
                candidate
            } else {
                best
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Polyline, area_mm2, assert_approx_eq};

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
        let tree = RingTree::build(r, 1.2, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
        assert_eq!(tree.nodes.len(), 1);
    }

    #[test]
    fn tree_of_a_rect_is_a_chain() {
        let r = region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]);
        let tree = RingTree::build(r, 2.0, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
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
                FLATTEN_TOLERANCE_DEFAULT_MM,
                2
            )
            .is_none()
        );
    }
}
