// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/medialAxis.ts
//   src/engine/toolpaths/vcarveMedial/resolution.ts

//! The medial axis of a region (`docs/specs/phase-3/design.md`,
//! "Algorithms → Medial axis (dc-geometry::medial)"): the skeleton a V-bit
//! follows, each node carrying its exact distance to the boundary.
//!
//! The classic Delaunay-dual construction, as upstream: the boundary is
//! sampled at a step ([`resolution`]), the samples are triangulated
//! (`delaunator`, the Rust port of the library PureCutCNC uses), and the
//! circumcentres of the triangles inside the region converge to the axis.
//! Each node's clearance is the exact distance to the boundary segments,
//! so depth does not depend on the step. Nodes whose boundary contacts do
//! not spread at least 60° are dropped (the spokes curve flattening
//! leaves); then sub-step clusters are merged, short spurs pruned, small
//! fragments dropped, and sharp convex corners given zero-clearance tips so
//! a V-bit rises to the surface there.
//!
//! What changed in the port: the input is a [`Region`] whose arcs are
//! flattened once at the caller's tolerance, not a `Point[]` loop; the
//! `Map<string, …>` keys of rounded coordinates are integer pairs in
//! `BTreeMap`s and adjacency is a `BTreeSet`, so nothing depends on hash
//! order. No Y-up re-derivation is needed: a Voronoi skeleton is the same
//! under reflection, and [`convex_corners`] reads either winding through
//! `cross · area`, as upstream.

mod grid;
#[cfg(test)]
mod tests;

use crate::point::{Point, Vec2};
use crate::polyline::Polyline;
use crate::region::Region;
use grid::{SegmentDistanceGrid, count_of};
use std::collections::{BTreeMap, BTreeSet};

/// One node of a [`MedialGraph`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialNode {
    /// Position, mm, Y-up.
    pub p: Point,
    /// Exact distance from `p` to the nearest boundary segment of the
    /// flattened region, mm; 0 at a corner tip.
    pub clearance_mm: f64,
}

/// A region's medial axis as an undirected graph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MedialGraph {
    /// The nodes.
    pub nodes: Vec<MedialNode>,
    /// `adjacency[i]` lists the nodes joined to node `i`, ascending; the
    /// relation is symmetric. A graph of one node and no edges is a dot.
    pub adjacency: Vec<Vec<usize>>,
}

/// The automatic sampling step of a region ([`resolution`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolution {
    /// The step to sample the boundary at, mm.
    pub step_mm: f64,
    /// The sample-budget floor, mm: the summed loop lengths over
    /// [`MEDIAL_SAMPLE_BUDGET`].
    pub budget_floor_mm: f64,
    /// True when the budget floor, not the shape, set the step.
    pub budget_limited: bool,
}

/// A convex corner of a region ([`convex_corners`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// The corner vertex, mm.
    pub p: Point,
    /// How far the boundary turns there, radians (0 is straight on).
    pub turn_rad: f64,
    /// Unit vector along the corner's bisector, pointing into the region.
    pub bisector: Vec2,
}

/// Samples across the short side of the outer's bounding box, so similar
/// shapes keep the same sample density as they scale (upstream's
/// `MEDIAL_SAMPLES_ACROSS_SHORT_SPAN`).
pub const MEDIAL_SAMPLES_ACROSS_SHORT_SPAN: f64 = 160.0;

/// Boundary samples a region may have before its step is coarsened
/// (upstream's `MEDIAL_SAMPLE_BUDGET_PER_REGION`).
pub const MEDIAL_SAMPLE_BUDGET: f64 = 40_000.0;

/// The turn beyond which a convex corner gets a zero-clearance tip: 50°,
/// in radians. Shallower turns are the stair steps of flattened curves,
/// not corners a V-bit should surface in.
pub const CORNER_ANGLE_RAD: f64 = 50.0 * std::f64::consts::PI / 180.0;

/// The angle a node's boundary contacts must spread over to keep it: 60°.
const CONTACT_SPREAD_RAD: f64 = 60.0 * std::f64::consts::PI / 180.0;

/// A leaf edge shorter than this many steps that does not descend toward
/// the boundary is sampling noise.
const PRUNE_FACTOR: f64 = 1.5;

/// Passes of spur pruning at most.
const MAX_PRUNE_ITERATIONS: usize = 32;

/// Samples [`medial_axis`] takes at most, whatever step it is given: a
/// step far below [`resolution`]'s is raised to keep to this, which bounds
/// the memory a careless caller can ask for. [`resolution`]'s own steps
/// stay well under it.
const MAX_MEDIAL_SAMPLES: f64 = 1_000_000.0;

/// Coordinates are compared rounded to this many units per millimetre
/// when samples and circumcentres are deduplicated (upstream's `1e7`).
const KEY_SCALE: f64 = 1e7;

/// The sampling step for `region`'s medial axis, mm: the larger of the
/// short side of the outer's bounding box over
/// [`MEDIAL_SAMPLES_ACROSS_SHORT_SPAN`] and the length of every loop over
/// [`MEDIAL_SAMPLE_BUDGET`]. Scale-invariant: a shape twice the size gets
/// twice the step. Bounds and lengths are of the true curves. `None` when
/// the outer has no area to span.
#[must_use]
pub fn resolution(region: &Region) -> Option<Resolution> {
    let holes: Vec<&Polyline> = region.holes().iter().map(|h| h.polyline()).collect();
    resolve(region.outer().polyline(), &holes)
}

/// [`resolution`] on loops, which may be degenerate.
fn resolve(outer: &Polyline, holes: &[&Polyline]) -> Option<Resolution> {
    let bounds = outer.bounds()?;
    let (width_mm, height_mm) = (bounds.width_mm(), bounds.height_mm());
    if width_mm <= 0.0 || height_mm <= 0.0 {
        return None;
    }
    let shape_mm = width_mm.min(height_mm) / MEDIAL_SAMPLES_ACROSS_SHORT_SPAN;
    let perimeter_mm = outer.length_mm() + holes.iter().map(|h| h.length_mm()).sum::<f64>();
    let budget_floor_mm = perimeter_mm / MEDIAL_SAMPLE_BUDGET;
    let step_mm = shape_mm.max(budget_floor_mm);
    (step_mm.is_finite() && step_mm > 0.0).then_some(Resolution {
        step_mm,
        budget_floor_mm,
        budget_limited: budget_floor_mm > shape_mm,
    })
}

/// The medial axis of `region` (mm, Y-up; either winding), its boundary
/// sampled every `step_mm` (normally [`resolution`]'s) after arcs are
/// flattened to chords within `tolerance_mm`. Clearances are exact
/// distances to those chords. Convex corners turning more than
/// [`CORNER_ANGLE_RAD`] end in a node of clearance 0 at the corner.
/// Adjacency lists are sorted. Empty when the step is not positive or the
/// outer flattens to fewer than three distinct points; a region too small
/// for its step collapses to one node, its deepest.
///
/// Pure and deterministic: the same region and arguments give the same
/// graph, node for node.
#[must_use]
pub fn medial_axis(region: &Region, step_mm: f64, tolerance_mm: f64) -> MedialGraph {
    medial_axis_of_loops(&flattened_loops(region, tolerance_mm), step_mm)
}

/// The convex corners of `region` turning more than `min_turn_rad`, after
/// arcs are flattened within `tolerance_mm`: outer vertices where the
/// boundary turns into the region, and hole vertices the region wraps
/// round (a notch in an island). Loops in order, outer first, each from its
/// first vertex; mm.
#[must_use]
pub fn convex_corners(region: &Region, min_turn_rad: f64, tolerance_mm: f64) -> Vec<Corner> {
    corners_of_loops(&flattened_loops(region, tolerance_mm), min_turn_rad)
}

/// True when `p` is inside `loops` by the even-odd rule: inside an outer
/// and not inside a hole in it, whatever the windings. Arcs are read as
/// their chords; flatten first where that matters. A point exactly on an
/// edge may count either way.
#[must_use]
pub fn point_in_loops(p: Point, loops: &[&Polyline]) -> bool {
    let points: Vec<Vec<Point>> = loops
        .iter()
        .map(|l| l.vertices().iter().map(|v| v.point()).collect())
        .collect();
    even_odd(p, &points)
}

// ---------------------------------------------------------------------------
// Boundary preparation
// ---------------------------------------------------------------------------

/// The outer then each hole of `region`, arcs flattened, cleaned.
fn flattened_loops(region: &Region, tolerance_mm: f64) -> Vec<Vec<Point>> {
    std::iter::once(region.outer())
        .chain(region.holes())
        .map(|c| {
            let flat = c.polyline().with_arcs_flattened(tolerance_mm);
            clean_loop(flat.vertices().iter().map(|v| v.point()))
        })
        .collect()
}

/// `points` without consecutive duplicates or an explicit closing vertex
/// (upstream's `cleanLoop`).
fn clean_loop(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in points {
        if out.last().is_some_and(|q| q.distance_to(p) < 1e-9) {
            continue;
        }
        out.push(p);
    }
    while out.len() >= 2 {
        match (out.first(), out.last()) {
            (Some(&first), Some(&last)) if first.distance_to(last) < 1e-9 => {
                out.pop();
            }
            _ => break,
        }
    }
    out
}

/// The edges of closed `points`, each with the vertex before it first
/// (upstream's `j = i − 1` walk): (last, first), (first, second), …
fn edges(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .last()
        .copied()
        .into_iter()
        .chain(points.iter().copied())
        .zip(points.iter().copied())
}

/// Every edge of closed `points` split into `ceil(len / step)` equal
/// pieces, original vertices kept so corners stay sharp (upstream's
/// `resampleLoop`).
fn resample_loop(points: &[Point], step_mm: f64) -> Vec<Point> {
    let mut out = Vec::new();
    let next = points.iter().cycle().skip(1);
    for (&a, &b) in points.iter().zip(next) {
        let pieces = count_of((a.distance_to(b) / step_mm).ceil()).max(1);
        for k in 0..pieces {
            let t = k as f64 / pieces as f64;
            out.push(a + (b - a) * t);
        }
    }
    out
}

/// Even-odd point-in-polygon over every loop (upstream's
/// `pointInRegionLoops`).
fn even_odd(p: Point, loops: &[Vec<Point>]) -> bool {
    let mut inside = false;
    for l in loops {
        for (pj, pi) in edges(l) {
            if (pi.y > p.y) != (pj.y > p.y)
                && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x
            {
                inside = !inside;
            }
        }
    }
    inside
}

/// `v` rounded half up, as JavaScript's `Math.round`, as a key.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a key only needs equal inputs to give equal keys; `as` saturates"
)]
fn round_key(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

fn point_key(p: Point, scale: f64) -> (i64, i64) {
    (round_key(p.x * scale), round_key(p.y * scale))
}

// ---------------------------------------------------------------------------
// Convex corners
// ---------------------------------------------------------------------------

/// Upstream's `regionConvexCorners` on cleaned loops, outer first.
fn corners_of_loops(loops: &[Vec<Point>], min_turn_rad: f64) -> Vec<Corner> {
    let mut corners = Vec::new();
    for (index, pts) in loops.iter().enumerate() {
        let island = index > 0;
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let area: f64 = edges(pts).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
        if area.abs() < 1e-12 {
            continue;
        }
        let prevs = pts.iter().cycle().skip(n - 1);
        let nexts = pts.iter().cycle().skip(1);
        for ((&prev, &curr), &next) in prevs.zip(pts).zip(nexts) {
            let (d1, d2) = (curr - prev, next - curr);
            let (len1, len2) = (d1.length(), d2.length());
            if len1 < 1e-12 || len2 < 1e-12 {
                continue;
            }
            let cross = d1.cross(d2);
            let convex_into_region = if island {
                cross * area < 0.0
            } else {
                cross * area > 0.0
            };
            if !convex_into_region {
                continue;
            }
            let turn_rad = (d1.dot(d2) / (len1 * len2)).clamp(-1.0, 1.0).acos();
            if turn_rad <= min_turn_rad {
                continue;
            }
            let mut bisector = Vec2::new(d2.x / len2 - d1.x / len1, d2.y / len2 - d1.y / len1);
            if island {
                bisector = -bisector;
            }
            let length = bisector.length();
            if length < 1e-12 {
                continue;
            }
            let bisector = bisector * (1.0 / length);
            corners.push(Corner {
                p: curr,
                turn_rad,
                bisector,
            });
        }
    }
    corners
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------

/// The circumcentre of triangle `a b c`, or `None` when it is degenerate
/// (`|d| <= 1e-9 · max(|ab|², |ac|²)`, upstream's `circumcenter`).
fn circumcentre(a: Point, b: Point, c: Point) -> Option<Point> {
    let (d, e) = (b - a, c - a);
    let (bl, cl) = (d.dot(d), e.dot(e));
    let det = d.cross(e);
    if det.abs() <= 1e-9 * bl.max(cl) {
        return None;
    }
    let half = 0.5 / det;
    Some(Point::new(
        a.x + (e.y * bl - d.y * cl) * half,
        a.y + (d.x * cl - e.x * bl) * half,
    ))
}

/// The medial axis of `loops` (outer first, then holes; any winding)
/// sampled every `step_mm` (upstream's `computeMedialAxis`).
fn medial_axis_of_loops(loops: &[Vec<Point>], step_mm: f64) -> MedialGraph {
    if !(step_mm.is_finite() && step_mm > 0.0) {
        return MedialGraph::default();
    }
    let mut cleaned = loops.iter().map(|l| clean_loop(l.iter().copied()));
    let outer = cleaned.next().unwrap_or_default();
    if outer.len() < 3 {
        return MedialGraph::default();
    }
    let loops: Vec<Vec<Point>> = std::iter::once(outer)
        .chain(cleaned.filter(|l| l.len() >= 3))
        .collect();
    let perimeter_mm: f64 = loops
        .iter()
        .flat_map(|l| edges(l))
        .map(|(a, b)| a.distance_to(b))
        .sum();
    let step_mm = step_mm.max(perimeter_mm / MAX_MEDIAL_SAMPLES);

    // 1. Sample the boundary; keep every piece as a segment, each distinct
    //    sample once.
    let mut samples: Vec<Point> = Vec::new();
    let mut segments: Vec<(Point, Point)> = Vec::new();
    let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
    for l in &loops {
        let resampled = resample_loop(l, step_mm);
        let next = resampled.iter().cycle().skip(1);
        for (&a, &b) in resampled.iter().zip(next) {
            segments.push((a, b));
            if seen.insert(point_key(a, KEY_SCALE)) {
                samples.push(a);
            }
        }
    }
    if samples.len() < 3 {
        return MedialGraph::default();
    }
    let grid = SegmentDistanceGrid::new(segments, step_mm * 2.0);

    // 2. Delaunay triangulation.
    let coords: Vec<delaunator::Point> = samples
        .iter()
        .map(|p| delaunator::Point { x: p.x, y: p.y })
        .collect();
    let delaunay = delaunator::triangulate(&coords);
    if delaunay.triangles.is_empty() {
        return MedialGraph::default();
    }

    // 3. Circumcentres inside the region become raw nodes, deduplicated.
    let mut raw: Vec<Point> = Vec::new();
    let mut by_key: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let (triangles, _) = delaunay.triangles.as_chunks::<3>();
    let mut node_of_triangle: Vec<Option<usize>> = triangles
        .iter()
        .map(|&[i, j, k]| {
            let corner = |s: usize| samples.get(s).copied();
            let cc = circumcentre(corner(i)?, corner(j)?, corner(k)?)?;
            if !even_odd(cc, &loops) {
                return None;
            }
            Some(*by_key.entry(point_key(cc, KEY_SCALE)).or_insert_with(|| {
                raw.push(cc);
                raw.len() - 1
            }))
        })
        .collect();

    // 4. Keep nodes whose boundary contacts spread (a λ-medial axis on the
    //    exact boundary). The tolerance covers the node error from chord
    //    sagitta, plus a small floor.
    let mut kept: Vec<Option<usize>> = vec![None; raw.len()];
    let mut nodes: Vec<(Point, f64)> = Vec::new();
    for (slot, &p) in kept.iter_mut().zip(&raw) {
        let clearance = grid.distance_to(p);
        if clearance < step_mm * 1e-3 {
            continue;
        }
        let tol_ratio = 0.003 + (step_mm / clearance).powi(2) / 8.0;
        if !grid.has_contact_spread(p, clearance, CONTACT_SPREAD_RAD, tol_ratio) {
            continue;
        }
        *slot = Some(nodes.len());
        nodes.push((p, clearance));
    }
    for node in &mut node_of_triangle {
        *node = node.and_then(|n| kept.get(n).copied().flatten());
    }

    // 5. Edges between adjacent kept triangles, unless the edge cuts across
    //    a reflex corner (its midpoint outside).
    let mut graph_edges: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (e, &h) in delaunay.halfedges.iter().enumerate() {
        if h == delaunator::EMPTY || h <= e {
            continue;
        }
        let triangle_node = |half: usize| node_of_triangle.get(half / 3).copied().flatten();
        let (Some(a), Some(b)) = (triangle_node(e), triangle_node(h)) else {
            continue;
        };
        if a == b {
            continue;
        }
        let (Some(&(pa, _)), Some(&(pb, _))) = (nodes.get(a), nodes.get(b)) else {
            continue;
        };
        let mid = Point::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0);
        if even_odd(mid, &loops) {
            graph_edges.insert((a.min(b), a.max(b)));
        }
    }

    let mut graph = contract_micro_clusters(&nodes, &graph_edges, step_mm * 0.75);
    prune_spurs(&mut graph, step_mm * PRUNE_FACTOR);
    drop_small_components(&mut graph, step_mm * 4.0);
    extend_into_corners(
        &mut graph,
        &corners_of_loops(&loops, CORNER_ANGLE_RAD),
        &loops,
        step_mm,
    );
    finalize(graph, step_mm)
}

// ---------------------------------------------------------------------------
// Post-processing passes
// ---------------------------------------------------------------------------

/// The graph the passes edit in place.
struct Graph {
    nodes: Vec<MedialNode>,
    adjacency: Vec<BTreeSet<usize>>,
}

impl Graph {
    fn node(&self, i: usize) -> Option<MedialNode> {
        self.nodes.get(i).copied()
    }

    fn degree(&self, i: usize) -> usize {
        self.adjacency.get(i).map_or(0, BTreeSet::len)
    }

    fn unlink(&mut self, a: usize, b: usize) {
        if let Some(set) = self.adjacency.get_mut(a) {
            set.remove(&b);
        }
        if let Some(set) = self.adjacency.get_mut(b) {
            set.remove(&a);
        }
    }

    fn link(&mut self, a: usize, b: usize) {
        if let Some(set) = self.adjacency.get_mut(a) {
            set.insert(b);
        }
        if let Some(set) = self.adjacency.get_mut(b) {
            set.insert(a);
        }
    }
}

/// Merges nodes in the same `cell_mm` grid cell into the member with the
/// largest clearance (upstream's `contractMicroClusters`). Braided chains
/// and micro-rings below the step collapse; a long chain of short edges
/// cannot collapse transitively, since the cell bounds a merge.
fn contract_micro_clusters(
    nodes: &[(Point, f64)],
    edges: &BTreeSet<(usize, usize)>,
    cell_mm: f64,
) -> Graph {
    let mut cluster_of: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let mut representative: Vec<(Point, f64)> = Vec::new();
    let mut mapped: Vec<usize> = Vec::with_capacity(nodes.len());
    for &(p, clearance) in nodes {
        let key = (round_key(p.x / cell_mm), round_key(p.y / cell_mm));
        let index = *cluster_of.entry(key).or_insert_with(|| {
            representative.push((p, clearance));
            representative.len() - 1
        });
        if let Some(rep) = representative.get_mut(index)
            && clearance > rep.1
        {
            *rep = (p, clearance);
        }
        mapped.push(index);
    }
    let mut graph = Graph {
        nodes: representative
            .iter()
            .map(|&(p, clearance_mm)| MedialNode { p, clearance_mm })
            .collect(),
        adjacency: vec![BTreeSet::new(); representative.len()],
    };
    for &(a, b) in edges {
        if let (Some(&ma), Some(&mb)) = (mapped.get(a), mapped.get(b))
            && ma != mb
        {
            graph.link(ma, mb);
        }
    }
    graph
}

/// Removes short leaves that do not descend toward the boundary (upstream's
/// `pruneSpurs`): a corner branch loses clearance toward its tip, a
/// sampling spur keeps its junction's. Leaves are visited in index order
/// and removed at once, so a junction left a leaf can go in the same pass.
fn prune_spurs(graph: &mut Graph, max_len_mm: f64) {
    for _ in 0..MAX_PRUNE_ITERATIONS {
        let mut removed = 0;
        for leaf in 0..graph.adjacency.len() {
            let junction = match graph.adjacency.get(leaf) {
                Some(set) if set.len() == 1 => set.first().copied(),
                _ => None,
            };
            let (Some(junction), Some(l)) = (junction, graph.node(leaf)) else {
                continue;
            };
            let Some(j) = graph.node(junction) else {
                continue;
            };
            let len = l.p.distance_to(j.p);
            if len >= max_len_mm || l.clearance_mm < j.clearance_mm - 0.25 * len {
                continue;
            }
            graph.unlink(leaf, junction);
            removed += 1;
        }
        if removed == 0 {
            break;
        }
    }
}

/// Drops components shorter in total than `min_total_mm`, the remnants of
/// filtered spokes; when none is longer, keeps the one holding the deepest
/// node (upstream's `dropSmallComponents`).
fn drop_small_components(graph: &mut Graph, min_total_mm: f64) {
    let count = graph.nodes.len();
    let mut component_of: Vec<Option<usize>> = vec![None; count];
    let mut lengths: Vec<f64> = Vec::new();
    for start in 0..count {
        if component_of.get(start).copied().flatten().is_some() || graph.degree(start) == 0 {
            continue;
        }
        let component = lengths.len();
        let mut length = 0.0;
        if let Some(slot) = component_of.get_mut(start) {
            *slot = Some(component);
        }
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            let (Some(n), Some(adjacent)) = (graph.node(node), graph.adjacency.get(node)) else {
                continue;
            };
            for &neighbour in adjacent {
                if let Some(m) = graph.node(neighbour) {
                    length += n.p.distance_to(m.p) / 2.0;
                }
                if let Some(slot) = component_of.get_mut(neighbour)
                    && slot.is_none()
                {
                    *slot = Some(component);
                    stack.push(neighbour);
                }
            }
        }
        lengths.push(length);
    }
    if lengths.is_empty() {
        return;
    }
    let mut keep: Vec<bool> = lengths.iter().map(|&l| l >= min_total_mm).collect();
    if !keep.iter().any(|&k| k) {
        let deepest = (0..count)
            .filter(|&i| component_of.get(i).copied().flatten().is_some())
            .reduce(|best, i| {
                let deeper = graph.node(i).map_or(0.0, |n| n.clearance_mm)
                    > graph.node(best).map_or(0.0, |n| n.clearance_mm);
                if deeper { i } else { best }
            });
        if let Some(slot) = deepest
            .and_then(|d| component_of.get(d).copied().flatten())
            .and_then(|c| keep.get_mut(c))
        {
            *slot = true;
        }
    }
    for (component, set) in component_of.iter().zip(graph.adjacency.iter_mut()) {
        if component.is_some_and(|c| !keep.get(c).copied().unwrap_or(true)) {
            set.clear();
        }
    }
}

/// Joins each sharp convex corner to the graph with a zero-clearance tip
/// node (upstream's `extendIntoCorners`). The sampled branch stops about a
/// step short of the corner; the tip continues it so a V-bit surfaces at
/// the corner. The joint is the best node aligned with the bisector, else
/// one whose medial disc reaches the corner, and only one whose segment to
/// the corner stays inside the region.
fn extend_into_corners(graph: &mut Graph, corners: &[Corner], loops: &[Vec<Point>], step_mm: f64) {
    for corner in corners {
        let count = graph.nodes.len();
        let mut candidates: Vec<(usize, bool, f64)> = Vec::new();
        for (i, node) in graph.nodes.iter().enumerate() {
            if graph.degree(i) == 0 && count > 1 {
                continue;
            }
            let d = node.p - corner.p;
            let distance = d.length();
            let projection = d.dot(corner.bisector);
            let off_axis = d.cross(corner.bisector).abs();
            let aligned = projection > 0.0 && off_axis <= (1.5 * step_mm).max(projection * 0.1);
            let touches_disc = distance <= node.clearance_mm + 1.5 * step_mm;
            if !aligned && !touches_disc {
                continue;
            }
            let score = if aligned {
                projection + 4.0 * off_axis
            } else {
                distance
            };
            candidates.push((i, aligned, score));
        }
        // Aligned first, then by score; stable, as upstream's sort.
        candidates.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        });
        let best = candidates.iter().map(|c| c.0).find(|&i| {
            graph
                .node(i)
                .is_some_and(|n| segment_inside(corner.p, n.p, loops, step_mm))
        });
        let Some(best) = best else {
            continue;
        };
        let tip = graph.nodes.len();
        graph.nodes.push(MedialNode {
            p: corner.p,
            clearance_mm: 0.0,
        });
        graph.adjacency.push(BTreeSet::new());
        graph.link(tip, best);
    }
}

/// True when the open segment from `from` to `to`, sampled at up to 64
/// points about a step apart, stays inside `loops`.
fn segment_inside(from: Point, to: Point, loops: &[Vec<Point>], step_mm: f64) -> bool {
    let d = to - from;
    let samples = count_of((d.length() / step_mm).ceil()).clamp(2, 64);
    (1..samples).all(|s| even_odd(from + d * (s as f64 / samples as f64), loops))
}

/// Drops nodes with no edges (keeping the deepest alone when nothing has
/// an edge), collapses a graph no wider than three steps to its deepest
/// node (a dot), and renumbers (upstream's `finalizeGraph`).
fn finalize(mut graph: Graph, step_mm: f64) -> MedialGraph {
    let deepest_of = |graph: &Graph, indices: &[usize]| {
        indices.iter().copied().reduce(|best, i| {
            let deeper = graph.node(i).map_or(0.0, |n| n.clearance_mm)
                > graph.node(best).map_or(0.0, |n| n.clearance_mm);
            if deeper { i } else { best }
        })
    };
    let mut keep: Vec<usize> = (0..graph.nodes.len())
        .filter(|&i| graph.degree(i) > 0)
        .collect();
    if keep.is_empty() {
        let all: Vec<usize> = (0..graph.nodes.len()).collect();
        keep = deepest_of(&graph, &all).into_iter().collect();
    }

    if keep.len() > 1 {
        let points: Vec<Point> = keep
            .iter()
            .filter_map(|&i| graph.node(i))
            .map(|n| n.p)
            .collect();
        let (mut min, mut max) = (
            Point::new(f64::INFINITY, f64::INFINITY),
            Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in &points {
            min = Point::new(min.x.min(p.x), min.y.min(p.y));
            max = Point::new(max.x.max(p.x), max.y.max(p.y));
        }
        if (max.x - min.x).max(max.y - min.y) <= step_mm * 3.0
            && let Some(deepest) = deepest_of(&graph, &keep)
        {
            if let Some(set) = graph.adjacency.get_mut(deepest) {
                set.clear();
            }
            keep = vec![deepest];
        }
    }

    let mut remap: Vec<Option<usize>> = vec![None; graph.nodes.len()];
    for (new, &old) in keep.iter().enumerate() {
        if let Some(slot) = remap.get_mut(old) {
            *slot = Some(new);
        }
    }
    let nodes = keep.iter().filter_map(|&old| graph.node(old)).collect();
    let adjacency = keep
        .iter()
        .map(|&old| {
            let mut list: Vec<usize> = graph
                .adjacency
                .get(old)
                .into_iter()
                .flatten()
                .filter_map(|&n| remap.get(n).copied().flatten())
                .collect();
            list.sort_unstable();
            list
        })
        .collect();
    MedialGraph { nodes, adjacency }
}
