// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/toolpath.ts (extractChains,
//   pruneRedundantLeafChains, simplifyChain, emitMedialToolpath)
//   src/engine/toolpaths/vcarveMedial/index.ts
//   (generateVCarveMedialToolpath: the per-region resolution and its
//   auto-refinement)

//! The medial strategy of a V-Carve (`docs/specs/phase-3/design.md`,
//! "Algorithms → V-Carve generator", step 5): a region's medial axis
//! becomes chains, the chains become cuts at the depth where the bit's
//! flanks touch both walls, and the cuts become [`CutBlock`]s.
//!
//! What changed in the port:
//!
//! - **Links (security review H2).** Upstream bridges two chain ends
//!   tool-down when the gap is within 0.9 of the disc the bit carves at
//!   the surface. A link at depth across that disc runs below the final V
//!   surface, so that rule is not ported: here a link stays down only when
//!   the straight move passes the exact cone test ([`Walls::least_room`])
//!   at its own depth along its whole length, with [`EPS_MM`] to spare;
//!   otherwise the tool retracts.
//! - **Blocks.** Upstream writes moves; here each run of chains the tool
//!   cuts without lifting (chains continued through a shared junction or
//!   joined by a link) is one block, its entry the run's first point at
//!   that point's Z (security review H1: a chain from a corner tip starts
//!   at the start plane, not at its deepest Z). `sequence` plunges to it
//!   and retracts between blocks, so it needs no cone test of its own.
//! - **The first chain.** Upstream starts a region at its first chain, or
//!   at the chain end nearest the previous region's. The sequencer orders
//!   regions here, so each region starts at its shallowest chain end (a
//!   corner tip where there is one), which is also the shortest plunge;
//!   then greedy nearest end, as upstream.
//! - **Depth fitting.** Douglas-Peucker on the chain (upstream's
//!   `simplifyChain`) and the straight edges between medial nodes can let
//!   the cone at a point between two kept points reach past a wall that
//!   bends there (a reflex corner, a flattened curve). Each such stretch
//!   is split at its worst point, set to that point's own depth, until the
//!   cone test passes ([`fit_chain`]). Upstream does not check this.
//! - Y-up needs no re-derivation: a medial axis, its chains and the
//!   greedy order are the same under reflection.

use crate::block::CutBlock;
use crate::error::ShapeRef;
use crate::tabs::LapRates;
use crate::toolpath::Move;
use crate::vcarve::cone::{OpenEdgeWalls, Walls};
use crate::warning::Warning;
use dc_geometry::medial::{MedialGraph, medial_axis, resolution};
use dc_geometry::{EPS_MM, Point3, Region};
use dc_project::OperationId;
use std::collections::BTreeSet;

/// Empty-result refinement: the step is halved at most this many times
/// (upstream's `MAX_AUTO_REFINEMENTS`).
pub(crate) const MAX_AUTO_REFINEMENTS: usize = 2;

/// Douglas-Peucker tolerance, in medial steps (upstream's
/// `simplifyTolerance: resolution * 0.25`).
pub(crate) const SIMPLIFY_STEPS: f64 = 0.25;

/// Redundant-leaf tolerance, in medial steps (upstream's
/// `redundancyTolerance: resolution * 0.75`).
pub(crate) const REDUNDANCY_STEPS: f64 = 0.75;

/// Passes of redundant-leaf pruning at most (upstream's four).
const MAX_PRUNE_PASSES: usize = 4;

/// Below this clearance, mm, a node is a corner tip (upstream's `1e-9`).
const TIP_CLEARANCE_MM: f64 = 1e-9;

/// How many times a stretch is halved at most when its cone is fitted to
/// the walls ([`fit_chain`]).
const MAX_FIT_LEVELS: usize = 16;

/// How far a stretch's cone may reach past a wall before it is split, mm:
/// a tenth of a micrometre, ten times finer than a program writes. Where a
/// wall bends between two points the overlap shrinks about fourfold per
/// split, so a few splits reach it.
const FIT_SLACK_MM: f64 = 1e-4;

/// How deep a carve cuts for a clearance: `min(clearance / slope, F)`
/// below the start plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Depth {
    /// The start plane, mm, project space.
    pub start_plane_z_mm: f64,
    /// The flat depth below the start plane, mm, if any.
    pub flat_depth_mm: Option<f64>,
    /// `tan(α/2)`: the cone's radius per mm of depth.
    pub slope: f64,
}

impl Depth {
    /// The depth below the start plane, mm, for `clearance_mm`, clamped
    /// at the flat depth.
    pub(crate) fn depth_mm(&self, clearance_mm: f64) -> f64 {
        let d = clearance_mm / self.slope;
        self.flat_depth_mm.map_or(d, |f| d.min(f))
    }

    /// The tip's Z, mm, for `clearance_mm`.
    pub(crate) fn z_mm(&self, clearance_mm: f64) -> f64 {
        self.start_plane_z_mm - self.depth_mm(clearance_mm)
    }
}

/// How far the medial pipeline may simplify, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tolerances {
    /// Douglas-Peucker tolerance; 0 keeps every node.
    pub simplify_mm: f64,
    /// Redundant-leaf slack, in clearance; 0 prunes nothing.
    pub redundancy_mm: f64,
}

impl Tolerances {
    /// Upstream's tolerances for a region sampled every `step_mm`.
    pub(crate) fn at_step(step_mm: f64) -> Self {
        Self {
            simplify_mm: SIMPLIFY_STEPS * step_mm,
            redundancy_mm: REDUNDANCY_STEPS * step_mm,
        }
    }
}

/// One stretch the tool cuts without lifting: from `start`, through each
/// step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    /// Where the tool plunges to: the first chain's first point.
    pub start: Point3,
    /// The points the tool cuts to, in order.
    pub steps: Vec<Step>,
}

/// One straight cut of a [`Run`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Step {
    /// Where the cut ends, mm, project space.
    pub to: Point3,
    /// True for a stay-down link between two chains, false for a cut
    /// along a chain (or a rise or fall in place at a shared node).
    pub link: bool,
}

/// The medial graph of `region`, at its automatic resolution, refined up
/// to [`MAX_AUTO_REFINEMENTS`] times while empty, with the step it was
/// built at. Warns [`Warning::VCarveSamplingBudget`] once per operation
/// (`budget_warned`) and [`Warning::VCarveNoMedialAxis`] when there is no
/// graph, then gives `None`.
pub(crate) fn region_graph(
    region: &Region,
    tolerance_mm: f64,
    op: &OperationId,
    shape: &ShapeRef,
    budget_warned: &mut bool,
    warnings: &mut Vec<Warning>,
) -> Option<(MedialGraph, f64)> {
    let no_axis = || Warning::VCarveNoMedialAxis {
        op: op.clone(),
        shape: shape.clone(),
    };
    let Some(resolved) = resolution(region) else {
        warnings.push(no_axis());
        return None;
    };
    let mut step_mm = resolved.step_mm;
    if resolved.budget_limited && !*budget_warned {
        *budget_warned = true;
        warnings.push(Warning::VCarveSamplingBudget {
            op: op.clone(),
            resolution_mm: step_mm,
        });
    }
    let mut graph = medial_axis(region, step_mm, tolerance_mm);
    // A region narrower than the step can miss every interior Voronoi
    // vertex: retry finer before giving up.
    for _ in 0..MAX_AUTO_REFINEMENTS {
        if !graph.nodes.is_empty() {
            break;
        }
        let refined_mm = resolved.budget_floor_mm.max(step_mm / 2.0);
        if refined_mm >= step_mm {
            break;
        }
        step_mm = refined_mm;
        graph = medial_axis(region, step_mm, tolerance_mm);
    }
    if graph.nodes.is_empty() {
        warnings.push(no_axis());
        return None;
    }
    Some((graph, step_mm))
}

/// [`runs_within`] with no open edge: every chain whole.
#[cfg(test)]
pub(crate) fn runs(
    graph: &MedialGraph,
    depth: &Depth,
    tolerances: Tolerances,
    walls: &Walls,
    floors: &[Region],
) -> Vec<Run> {
    runs_within(graph, depth, tolerances, walls, floors, None)
}

/// The runs that carve one region from its medial `graph`: chains
/// ([`extract_chains`]) less the redundant leaves, each node at its depth,
/// simplified and fitted to `walls`, each cut to the stretches whose moves
/// keep the cone inside `edge` at the stock top (#448: a male Inlay's open
/// band edge, beyond which the board stands full height; `None` keeps
/// every chain whole), less the chains wholly on the cleared `floors`
/// (step 6, #405; empty without a floor), then ordered from the shallowest
/// chain end by nearest end, continued through shared nodes, linked
/// tool-down where the link passes the cone test, and split by a retract
/// elsewhere. With an edge only the chains a V wall at the band's edge
/// needs fail it, and chains on the floor far out, which the floor's
/// cutting covers.
pub(crate) fn runs_within(
    graph: &MedialGraph,
    depth: &Depth,
    tolerances: Tolerances,
    walls: &Walls,
    floors: &[Region],
    edge: Option<&OpenEdgeWalls>,
) -> Vec<Run> {
    let chains: Vec<Vec<Point3>> =
        prune_redundant_leaf_chains(graph, extract_chains(graph), tolerances.redundancy_mm)
            .iter()
            .flat_map(|chain| {
                let points: Vec<Point3> = chain
                    .iter()
                    .filter_map(|&i| graph.nodes.get(i))
                    .map(|n| Point3::new(n.p.x, n.p.y, depth.z_mm(n.clearance_mm)))
                    .collect();
                let simple = simplify_chain(&points, tolerances.simplify_mm, depth.slope);
                within_edge(fit_chain(&simple, walls, depth), edge)
            })
            .filter(|points| !points.is_empty() && !inside_floor(points, depth, floors))
            .collect();

    let mut remaining: Vec<usize> = (0..chains.len()).collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;
    let mut position: Option<Point3> = None;
    while !remaining.is_empty() {
        let picked = match position {
            None => shallowest_end(&chains, &remaining),
            Some(p) => nearest_end(&chains, &remaining, p),
        };
        let Some((slot, reversed)) = picked else {
            break;
        };
        let index = remaining.remove(slot);
        let Some(chain) = chains.get(index) else {
            continue;
        };
        let oriented: Vec<Point3> = if reversed {
            chain.iter().rev().copied().collect()
        } else {
            chain.clone()
        };
        let Some((&entry, rest)) = oriented.split_first() else {
            continue;
        };
        let from = position.filter(|_| current.is_some());
        // Continuing through a shared node: the tool is already there.
        let continues = from.is_some_and(|p| p.xy().distance_to(entry.xy()) <= EPS_MM);
        let links = !continues && from.is_some_and(|p| links_tool_down(walls, depth, p, entry));
        if !(continues || links) {
            runs.extend(current.take());
            current = Some(Run {
                start: entry,
                steps: Vec::new(),
            });
        }
        let Some(run) = current.as_mut() else {
            continue;
        };
        if let Some(p) = from {
            // At a shared node only a difference in Z is cut.
            if links || (continues && (p.z - entry.z).abs() > EPS_MM) {
                run.steps.push(Step {
                    to: entry,
                    link: links,
                });
            }
        }
        run.steps
            .extend(rest.iter().map(|&to| Step { to, link: false }));
        position = oriented.last().copied();
    }
    runs.extend(current);
    runs
}

/// The stretches of chain `points` whose every move `edge` allows
/// (#448), in order; `points` whole when there is no edge. A lone point is
/// kept when its plunge is allowed.
fn within_edge(points: Vec<Point3>, edge: Option<&OpenEdgeWalls>) -> Vec<Vec<Point3>> {
    let Some(edge) = edge else {
        return vec![points];
    };
    if let [p] = points[..] {
        return if edge.allows(p, p) {
            vec![points]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    let mut current: Vec<Point3> = Vec::new();
    for pair in points.windows(2) {
        let &[a, b] = pair else {
            continue;
        };
        if edge.allows(a, b) {
            if current.is_empty() {
                current.push(a);
            }
            current.push(b);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Step 6: true when the fitted chain `points` lies wholly on the cleared
/// floor, which cuts it: every point at the flat depth (fitting has held
/// each stretch between them to the cone test there, so the clearance all
/// along it is at least `F · slope`) and inside one of `floors`. A chain
/// that reaches a corner or a narrow part rises above the floor there and
/// is kept, as is every chain when there is no floor.
fn inside_floor(points: &[Point3], depth: &Depth, floors: &[Region]) -> bool {
    let Some(flat_depth_mm) = depth.flat_depth_mm else {
        return false;
    };
    let floor_z_mm = depth.start_plane_z_mm - flat_depth_mm;
    !floors.is_empty()
        && points.iter().all(|p| {
            (p.z - floor_z_mm).abs() <= EPS_MM && floors.iter().any(|f| f.contains_point(p.xy()))
        })
}

/// Security review H2: the straight link from `from` to `to`, each at its
/// own Z, stays down only when the cone along it is inside the walls with
/// [`EPS_MM`] to spare everywhere: at both ends and wherever the room is
/// least. A link from or to a corner tip (no room at all) never does.
pub(crate) fn links_tool_down(walls: &Walls, depth: &Depth, from: Point3, to: Point3) -> bool {
    let top_mm = depth.start_plane_z_mm + EPS_MM;
    from.z <= top_mm
        && to.z <= top_mm
        && walls
            .least_room(from, to, depth.start_plane_z_mm, depth.slope)
            .0
            >= EPS_MM
}

/// The block that cuts `run` for `shape`: a straight plunge to the run's
/// start at its own Z (`entry_z_mm`), then its steps at the feed, a
/// descending step at the feed scaled to the plunge rate (S10), a fall in
/// place at the plunge rate. `z_mm` is the run's deepest Z; nothing is
/// cleared for `sequence` to link through.
pub(crate) fn run_block(run: &Run, shape: &ShapeRef, rates: LapRates, safe_z_mm: f64) -> CutBlock {
    let mut from = run.start;
    let mut z_mm = run.start.z;
    let mut cuts = Vec::with_capacity(run.steps.len());
    for step in &run.steps {
        cuts.push(Move::linear(step.to, feed_between(from, step.to, rates)));
        z_mm = z_mm.min(step.to.z);
        from = step.to;
    }
    CutBlock {
        shape: shape.clone(),
        pass: 0,
        z_mm,
        descent_from: run.start.xy(),
        descent_from_z_mm: safe_z_mm,
        descent: Vec::new(),
        entry: run.start.xy(),
        entry_z_mm: run.start.z,
        cuts,
        exit: from.xy(),
        frees: false,
        cleared: None,
        group: 0,
    }
}

/// The feed of a straight cut from `from` to `to`, mm/min: the plunge rate
/// straight down, the feed scaled so the move falls no faster than the
/// plunge rate when it falls while moving in XY (S10,
/// `min(feed, plunge · length / drop)`), the feed otherwise.
fn feed_between(from: Point3, to: Point3, rates: LapRates) -> f64 {
    let drop_mm = from.z - to.z;
    let run_mm = from.xy().distance_to(to.xy());
    if drop_mm <= EPS_MM {
        rates.feed_mm_min
    } else if run_mm <= EPS_MM {
        rates.plunge_mm_min
    } else {
        let length_mm = run_mm.hypot(drop_mm);
        rates
            .feed_mm_min
            .min(rates.plunge_mm_min * length_mm / drop_mm)
    }
}

/// The remaining chain end with the highest Z, as `(slot in remaining,
/// reversed)`; ties go to the earlier chain, then its first end.
fn shallowest_end(chains: &[Vec<Point3>], remaining: &[usize]) -> Option<(usize, bool)> {
    let mut best: Option<(usize, bool, f64)> = None;
    for (slot, &index) in remaining.iter().enumerate() {
        let Some(chain) = chains.get(index) else {
            continue;
        };
        let ends = [(false, chain.first()), (true, chain.last())];
        for (reversed, end) in ends {
            if let Some(end) = end
                && best.is_none_or(|(_, _, z)| end.z > z + EPS_MM)
            {
                best = Some((slot, reversed, end.z));
            }
        }
    }
    best.map(|(slot, reversed, _)| (slot, reversed))
}

/// The remaining chain whose nearer end is nearest `p` in XY, as `(slot in
/// remaining, reversed)` (upstream's greedy pick: a strictly nearer end
/// wins, the last end only when strictly nearer than the first, and an
/// end at `p` ends the search).
fn nearest_end(chains: &[Vec<Point3>], remaining: &[usize], p: Point3) -> Option<(usize, bool)> {
    let mut best: Option<(usize, bool)> = None;
    let mut best_cost = f64::INFINITY;
    for (slot, &index) in remaining.iter().enumerate() {
        let (Some(first), Some(last)) = chains
            .get(index)
            .map_or((None, None), |c| (c.first(), c.last()))
        else {
            continue;
        };
        let forward = first.xy().distance_to(p.xy());
        let backward = last.xy().distance_to(p.xy());
        let reversed = backward < forward;
        let cost = forward.min(backward);
        if cost < best_cost {
            best_cost = cost;
            best = Some((slot, reversed));
        }
        if cost < EPS_MM {
            break;
        }
    }
    best
}

/// The graph as node-index chains (upstream's `extractChains`): every
/// maximal path whose interior nodes have degree 2, then the pure cycles
/// left (first node repeated at the end), then each isolated node alone
/// (a plunge point for a dot).
pub(crate) fn extract_chains(graph: &MedialGraph) -> Vec<Vec<usize>> {
    let degree: Vec<usize> = graph.adjacency.iter().map(Vec::len).collect();
    let degree_of = |n: usize| degree.get(n).copied().unwrap_or(0);
    let key = |a: usize, b: usize| (a.min(b), a.max(b));
    let mut visited: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut chains: Vec<Vec<usize>> = Vec::new();

    let walk = |start: usize, next: usize, visited: &mut BTreeSet<(usize, usize)>| {
        let mut chain = vec![start];
        let mut prev = start;
        let mut curr = next;
        visited.insert(key(start, next));
        while degree_of(curr) == 2 && curr != start {
            chain.push(curr);
            let following = match graph.adjacency.get(curr).map(Vec::as_slice) {
                Some(&[a, b]) => {
                    if a == prev {
                        b
                    } else {
                        a
                    }
                }
                _ => break,
            };
            if !visited.insert(key(curr, following)) {
                break;
            }
            prev = curr;
            curr = following;
        }
        chain.push(curr);
        chain
    };

    let n = graph.nodes.len().min(graph.adjacency.len());
    for node in 0..n {
        if matches!(degree_of(node), 0 | 2) {
            continue;
        }
        for &neighbour in graph.adjacency.get(node).into_iter().flatten() {
            if visited.contains(&key(node, neighbour)) {
                continue;
            }
            let chain = walk(node, neighbour, &mut visited);
            chains.push(chain);
        }
    }
    // Edges not visited yet belong to pure degree-2 cycles.
    for node in 0..n {
        if degree_of(node) != 2 {
            continue;
        }
        for &neighbour in graph.adjacency.get(node).into_iter().flatten() {
            if visited.contains(&key(node, neighbour)) {
                continue;
            }
            let chain = walk(node, neighbour, &mut visited);
            chains.push(chain);
        }
    }
    for node in 0..graph.nodes.len() {
        if degree_of(node) == 0 {
            chains.push(vec![node]);
        }
    }
    chains
}

/// Drops leaf chains that deepen the carve nowhere by more than
/// `tolerance_mm` of clearance (upstream's `pruneRedundantLeafChains`): a
/// groove at `q` covers the groove tip at `p` when `|p − q| ≤ c_q − c_p`,
/// so with the slack a leaf branch whose every body node is so covered by
/// a node still in the skeleton (not its own body) is dropped. Corner tips
/// (clearance 0) are kept; at most [`MAX_PRUNE_PASSES`] passes; one chain
/// always stays.
fn prune_redundant_leaf_chains(
    graph: &MedialGraph,
    chains: Vec<Vec<usize>>,
    tolerance_mm: f64,
) -> Vec<Vec<usize>> {
    if chains.len() <= 1 || tolerance_mm <= 0.0 {
        return chains;
    }
    let degree = |n: usize| graph.adjacency.get(n).map_or(0, Vec::len);
    let mut removed: BTreeSet<usize> = BTreeSet::new();
    let mut alive = vec![true; chains.len()];

    let contained = |p: usize, body: &BTreeSet<usize>, removed: &BTreeSet<usize>| -> bool {
        let Some(node) = graph.nodes.get(p) else {
            return false;
        };
        graph.nodes.iter().enumerate().any(|(q, other)| {
            if q == p || removed.contains(&q) || body.contains(&q) {
                return false;
            }
            let slack = other.clearance_mm - node.clearance_mm + tolerance_mm;
            slack > 0.0 && node.p.distance_to(other.p) <= slack
        })
    };

    for _ in 0..MAX_PRUNE_PASSES {
        let mut changed = false;
        for (index, chain) in chains.iter().enumerate() {
            if !alive.get(index).copied().unwrap_or(false) {
                continue;
            }
            if alive.iter().filter(|a| **a).count() <= 1 {
                break;
            }
            let (Some(&first), Some(&last)) = (chain.first(), chain.last()) else {
                continue;
            };
            let first_is_leaf = degree(first) == 1;
            let last_is_leaf = degree(last) == 1;
            // An interior chain, or a floating segment.
            if first_is_leaf == last_is_leaf {
                continue;
            }
            let leaf = if first_is_leaf { first } else { last };
            if graph
                .nodes
                .get(leaf)
                .is_none_or(|n| n.clearance_mm < TIP_CLEARANCE_MM)
            {
                continue;
            }
            // The junction stays in the skeleton; only the chain's own body
            // may not vouch for itself.
            let junction = if first_is_leaf { last } else { first };
            let body: BTreeSet<usize> = chain.iter().copied().filter(|&n| n != junction).collect();
            if chain
                .iter()
                .all(|&n| n == junction || contained(n, &body, &removed))
            {
                if let Some(a) = alive.get_mut(index) {
                    *a = false;
                }
                removed.extend(body);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    chains
        .into_iter()
        .zip(alive)
        .filter_map(|(chain, alive)| alive.then_some(chain))
        .collect()
}

/// Douglas-Peucker on a chain's points (upstream's `simplifyChain`), where
/// a dropped point's error is the larger of its XY distance to the chord
/// and its Z error times `slope` (a depth error `dz` is as visible on the
/// carved wall as an XY error of `dz · slope`). The ends are always kept.
pub(crate) fn simplify_chain(points: &[Point3], tolerance_mm: f64, slope: f64) -> Vec<Point3> {
    if tolerance_mm <= 0.0 || points.len() <= 2 {
        return points.to_vec();
    }
    let deviation = |p: Point3, a: Point3, b: Point3| -> f64 {
        let (abx, aby) = (b.x - a.x, b.y - a.y);
        let length_sq = abx * abx + aby * aby;
        let t = if length_sq > 1e-18 {
            (((p.x - a.x) * abx + (p.y - a.y) * aby) / length_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let xy = (p.x - (a.x + abx * t)).hypot(p.y - (a.y + aby * t));
        let z = (p.z - (a.z + (b.z - a.z) * t)).abs() * slope;
        xy.max(z)
    };
    let mut keep = vec![false; points.len()];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((lo, hi)) = stack.pop() {
        if hi - lo < 2 {
            continue;
        }
        let (Some(&a), Some(&b)) = (points.get(lo), points.get(hi)) else {
            continue;
        };
        let mut worst: Option<usize> = None;
        let mut worst_deviation = tolerance_mm;
        for (i, &p) in points.iter().enumerate().take(hi).skip(lo + 1) {
            let d = deviation(p, a, b);
            if d > worst_deviation {
                worst_deviation = d;
                worst = Some(i);
            }
        }
        if let Some(w) = worst {
            if let Some(k) = keep.get_mut(w) {
                *k = true;
            }
            stack.push((lo, w));
            stack.push((w, hi));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, k)| k.then_some(p))
        .collect()
}

/// `points` with every stretch whose cone reaches past `walls` split at
/// its worst point, that point set to its own depth, until each stretch
/// passes or [`MAX_FIT_LEVELS`] halvings are spent. A point's own depth is
/// never deeper than the stretch it splits, so fitting only ever raises
/// the carve toward its true V surface.
fn fit_chain(points: &[Point3], walls: &Walls, depth: &Depth) -> Vec<Point3> {
    let Some((&first, rest)) = points.split_first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut from = first;
    for &to in rest {
        fit_stretch(from, to, walls, depth, MAX_FIT_LEVELS, &mut out);
        from = to;
    }
    out
}

/// [`fit_chain`] for the stretch `a`–`b`, `a` already written: writes the
/// points after `a`, up to and including `b`.
fn fit_stretch(
    a: Point3,
    b: Point3,
    walls: &Walls,
    depth: &Depth,
    levels: usize,
    out: &mut Vec<Point3>,
) {
    if levels > 0 && a.xy().distance_to(b.xy()) > EPS_MM {
        let (room_mm, worst) = walls.least_room(a, b, depth.start_plane_z_mm, depth.slope);
        let inside =
            worst.xy().distance_to(a.xy()) > EPS_MM && worst.xy().distance_to(b.xy()) > EPS_MM;
        if room_mm < -FIT_SLACK_MM && inside {
            let own_z = depth.z_mm(walls.clearance_mm(worst.xy()));
            let split = Point3::new(worst.x, worst.y, own_z.max(worst.z));
            fit_stretch(a, split, walls, depth, levels - 1, out);
            fit_stretch(split, b, walls, depth, levels - 1, out);
            return;
        }
    }
    out.push(b);
}
