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
//! - **Simplification on the carved edge (#457).** Upstream's
//!   Douglas-Peucker tolerance is measured on the axis, so a chord can run
//!   a quarter step off it and its cone stop up to twice that short of a
//!   wall. Here it is measured on the carved edge ([`simplify_chain`],
//!   [`EDGE_MM`]), and fitting also splits a stretch on the medial axis
//!   where it carves short of it between its points, a junction's above
//!   all (`Fit::off_axis`), so the groove reaches every wall within
//!   0.01 mm.
//! - **The tip stays in the region (#463).** A straight stretch between
//!   two medial nodes can cut across the tip of a thin wedge of material
//!   (a drawn line that ends inside a region), and at the start plane,
//!   where the cone is a point, the room test cannot see it. Fitting goes
//!   round the vertex such a stretch cuts across, by the medial axis
//!   point at it ([`Fit::around`]); a stretch that still leaves the
//!   region is not cut ([`within_walls`]). Upstream does not check this.
//! - **Every corner (#452, #456).** Every carve builds its axis to reach
//!   every convex corner of its walls (`CornerReach::Every`), where
//!   upstream's passes leave some with no branch.
//! - **Material above the start plane (#452).** A carve whose flank is
//!   engaged from above its start plane (a male Inlay) also fits each
//!   stretch down to its own depth (`Tolerances::deepen_mm`) and cuts its
//!   chains off where they run onto the floor ([`off_floor`]): there a
//!   corner the carve misses keeps wood up to the stock top.
//! - Y-up needs no re-derivation: a medial axis, its chains and the
//!   greedy order are the same under reflection.

use crate::block::CutBlock;
use crate::error::ShapeRef;
use crate::tabs::LapRates;
use crate::toolpath::Move;
use crate::vcarve::cone::{OpenEdgeWalls, Walls};
use crate::warning::Warning;
use dc_geometry::medial::{CornerReach, MedialGraph, medial_axis_with, resolution};
use dc_geometry::{EPS_MM, Point, Point3, Region};
use dc_project::OperationId;
use std::collections::BTreeSet;

/// Empty-result refinement: the step is halved at most this many times
/// (upstream's `MAX_AUTO_REFINEMENTS`).
pub(crate) const MAX_AUTO_REFINEMENTS: usize = 2;

/// Douglas-Peucker tolerance, in medial steps (upstream's
/// `simplifyTolerance: resolution * 0.25`), at most [`EDGE_MM`].
pub(crate) const SIMPLIFY_STEPS: f64 = 0.25;

/// How far short of the walls a simplified chain may carve, mm, measured
/// on the carved edge (#457; [`simplify_chain`]): half of the 0.01 mm the
/// groove must reach every wall within, which leaves the other half to the
/// medial axis's own nodes and to fitting.
pub(crate) const EDGE_MM: f64 = 0.005;

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

/// With [`Tolerances::axis_mm`], how far beyond a point's nearest wall
/// its medial axis point is sought, mm (#457): a stretch further off the
/// axis is split at the point that far out, which is then nearer it.
const AXIS_REACH_MM: f64 = 1.0;

/// With [`Tolerances::deepen_mm`], how far apart a stretch is read for
/// points whose own depth is deeper than the stretch there, mm (#452).
const DEEPEN_SAMPLE_MM: f64 = 0.05;

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
    /// How far, mm, a fitted stretch may run shallower than its points'
    /// own depth before it is split there too (#452); `None` splits only
    /// where the cone reaches past a wall, as upstream's tolerance allows.
    pub deepen_mm: Option<f64>,
    /// How far, mm, a fitted stretch may carve short of the medial axis
    /// between its points before it is split on the axis (#457); `None`
    /// fits to the walls alone.
    pub axis_mm: Option<f64>,
}

impl Tolerances {
    /// Upstream's tolerances for a region sampled every `step_mm`.
    pub(crate) fn at_step(step_mm: f64) -> Self {
        Self {
            simplify_mm: (SIMPLIFY_STEPS * step_mm).min(EDGE_MM),
            redundancy_mm: REDUNDANCY_STEPS * step_mm,
            deepen_mm: None,
            axis_mm: Some(EDGE_MM),
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
/// built at, carried into the region's sharp corners as `reach` says
/// (#452). Warns [`Warning::VCarveSamplingBudget`] once per operation
/// (`budget_warned`) and [`Warning::VCarveNoMedialAxis`] when there is no
/// graph, then gives `None`.
pub(crate) fn region_graph(
    region: &Region,
    tolerance_mm: f64,
    reach: CornerReach,
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
    let mut graph = medial_axis_with(region, step_mm, tolerance_mm, reach);
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
        graph = medial_axis_with(region, step_mm, tolerance_mm, reach);
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
                let simple = simplify_chain(&points, tolerances.simplify_mm, walls, depth);
                within_walls(fit_chain(&simple, walls, depth, &tolerances), walls)
                    .into_iter()
                    .flat_map(|points| within_edge(points, edge))
                    .collect::<Vec<_>>()
            })
            .map(|points| {
                // #452: a chain deepened to its corners is cut off where
                // it runs out onto the floor, which the floor's cutting
                // covers.
                if tolerances.deepen_mm.is_some() {
                    off_floor(points, depth, floors)
                } else {
                    points
                }
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

/// The fitted chain `points` cut where a stretch's tip path still leaves
/// the region between its ends (#463: [`Fit::around`] found no way round
/// in its splits), in order. A tip outside the region cuts what the carve
/// must keep, so the tool lifts over it instead; a piece left with one
/// point is dropped, since a plunge at a fitted chain's point carves no
/// more than the stretches through it. A chain that stays in is whole.
fn within_walls(points: Vec<Point3>, walls: &Walls) -> Vec<Vec<Point3>> {
    let leaves = |a: &Point3, b: &Point3| !walls.holds_path(a.xy(), b.xy(), walls.slack_mm());
    if !points
        .windows(2)
        .any(|w| matches!(w, [a, b] if leaves(a, b)))
    {
        return vec![points];
    }
    let mut out = Vec::new();
    let mut current: Vec<Point3> = Vec::new();
    for pair in points.windows(2) {
        let [a, b] = pair else {
            continue;
        };
        if leaves(a, b) {
            if current.len() > 1 {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
        } else {
            if current.is_empty() {
                current.push(*a);
            }
            current.push(*b);
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
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

/// The fitted chain `points` less the stretches at either end that lie on
/// the cleared `floors` (#452): from each end, every stretch whose two
/// points are both on a floor at the flat depth ([`inside_floor`]'s test,
/// which holds the stretch between them to the floor too). A chain wholly
/// on the floor keeps one point, which [`inside_floor`] then drops.
fn off_floor(points: Vec<Point3>, depth: &Depth, floors: &[Region]) -> Vec<Point3> {
    let on_floor = |p: Option<&Point3>| p.is_some_and(|&p| inside_floor(&[p], depth, floors));
    let mut lo = 0;
    while lo + 1 < points.len() && on_floor(points.get(lo)) && on_floor(points.get(lo + 1)) {
        lo += 1;
    }
    let mut hi = points.len();
    while hi > lo + 1 && on_floor(points.get(hi - 1)) && on_floor(points.get(hi - 2)) {
        hi -= 1;
    }
    points
        .get(lo..hi)
        .map(<[Point3]>::to_vec)
        .unwrap_or_default()
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

/// Douglas-Peucker on a chain's points, measured on the carved edge (#457;
/// upstream's `simplifyChain` measures on the axis). A stretch from `a`
/// to `b` stands for the points between when, for each point `p` of
/// them, the stretch's cone carves within `tolerance_mm` of the walls
/// `p`'s own cone touches: `slope · (low − z_p)`, where `low` is the
/// lowest the cone reaches over `p` along the stretch
/// ([`cone_low_mm`]), is at most the tolerance. Since the carved surface
/// rises no faster than `1 / slope` per mm, that bounds the shortfall
/// over the whole of `p`'s cone. And the stretch's cone may not reach
/// past a wall (`walls`, at most [`FIT_SLACK_MM`]). Else the stretch is
/// split at its worst point, or where its cone reaches past a wall at the
/// point nearest that. The kept points are the chain's own, each on the
/// axis at its own depth; the ends are always kept.
///
/// Upstream's tolerance on the axis lets a chord run a quarter step off
/// it; its cone, fitted to its own depth, then touches the nearer wall
/// and stops up to twice that short of the farther (a female Inlay's
/// groove 0.11 mm short on the "DC" letters, #452).
pub(crate) fn simplify_chain(
    points: &[Point3],
    tolerance_mm: f64,
    walls: &Walls,
    depth: &Depth,
) -> Vec<Point3> {
    if tolerance_mm <= 0.0 || points.len() <= 2 {
        return points.to_vec();
    }
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
        let inner = || points.iter().enumerate().take(hi).skip(lo + 1);
        let mut worst: Option<usize> = None;
        let mut worst_mm = tolerance_mm;
        for (i, &p) in inner() {
            let short_mm = depth.slope * (cone_low_mm(a, b, p.xy(), depth.slope) - p.z);
            if short_mm > worst_mm {
                worst_mm = short_mm;
                worst = Some(i);
            }
        }
        if worst.is_none() {
            let (room_mm, at) = walls.least_room(a, b, depth.start_plane_z_mm, depth.slope);
            if room_mm < -FIT_SLACK_MM {
                worst = inner()
                    .min_by(|x, y| {
                        x.1.xy()
                            .distance_to(at.xy())
                            .total_cmp(&y.1.xy().distance_to(at.xy()))
                    })
                    .map(|(i, _)| i);
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

/// The lowest a V-bit cone of `slope` reaches over `q`, mm, while its tip
/// runs straight from `a` to `b`: the least over `t` of `z(t) + |q −
/// p(t)| / slope`, which is convex in `t`, so its stationary point
/// clamped to the move is its least.
fn cone_low_mm(a: Point3, b: Point3, q: Point, slope: f64) -> f64 {
    let at = |t: f64| {
        let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        a.z + (b.z - a.z) * t + q.distance_to(p) / slope
    };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length <= EPS_MM {
        return at(0.0).min(at(1.0));
    }
    let (ux, uy) = (dx / length, dy / length);
    let (qx, qy) = (q.x - a.x, q.y - a.y);
    let along = qx * ux + qy * uy;
    let off = (qx * uy - qy * ux).abs();
    // `f'(t) = dz + L·w / (slope·√(off² + w²))` with `w = L(t − u)`: a
    // move that falls faster than its cone widens is least at an end.
    let k = -(b.z - a.z) * slope / length;
    if k.abs() >= 1.0 {
        return at(0.0).min(at(1.0));
    }
    let w = k * off / (1.0 - k * k).sqrt();
    at(((along + w) / length).clamp(0.0, 1.0))
}

/// `points` with every stretch whose cone reaches past `walls` split at
/// its worst point, that point set to its own depth, until each stretch
/// passes or [`MAX_FIT_LEVELS`] halvings are spent. A point's own depth is
/// never deeper than the stretch it splits, so fitting only ever raises
/// the carve toward its true V surface.
///
/// With `deepen_mm` (#452), a stretch that passes is also read every
/// [`DEEPEN_SAMPLE_MM`], and where it runs more than `deepen_mm` shallower
/// than a point's own depth it is split there, at that depth, which
/// touches the nearer wall and so passes. A straight fall from a corner's
/// tip to the floor, or Douglas-Peucker's chord across a bend in depth,
/// would otherwise leave the cone short of the walls, which only loses air
/// where the carve starts at the material's surface.
fn fit_chain(
    points: &[Point3],
    walls: &Walls,
    depth: &Depth,
    tolerances: &Tolerances,
) -> Vec<Point3> {
    let Some((&first, rest)) = points.split_first() else {
        return Vec::new();
    };
    let fit = Fit {
        walls,
        depth,
        deepen_mm: tolerances.deepen_mm,
        axis_mm: tolerances.axis_mm,
    };
    let mut out = vec![first];
    let mut from = first;
    for &to in rest {
        fit.stretch(from, to, (MAX_FIT_LEVELS, MAX_FIT_LEVELS), &mut out);
        from = to;
    }
    out
}

/// What [`fit_chain`] fits a chain to.
struct Fit<'a> {
    walls: &'a Walls,
    depth: &'a Depth,
    deepen_mm: Option<f64>,
    axis_mm: Option<f64>,
}

impl Fit<'_> {
    /// [`fit_chain`] for the stretch `a`–`b`, `a` already written: writes
    /// the points after `a`, up to and including `b`. `levels` counts the
    /// splits left for a cone past a wall or a stretch short of its depth,
    /// then for one short of the axis, each its own, so splits on the axis
    /// never use up the gouge test's.
    fn stretch(&self, a: Point3, b: Point3, levels: (usize, usize), out: &mut Vec<Point3>) {
        let (walls, axis) = levels;
        if a.xy().distance_to(b.xy()) > EPS_MM {
            let fitted = (walls > 0)
                .then(|| {
                    self.around(a, b)
                        .or_else(|| self.overcut(a, b))
                        .or_else(|| self.undercut(a, b))
                })
                .flatten();
            let split = fitted.map(|p| (p, (walls - 1, axis))).or_else(|| {
                (axis > 0)
                    .then(|| self.off_axis(a, b))
                    .flatten()
                    .map(|p| (p, (walls, axis - 1)))
            });
            if let Some((split, left)) = split {
                self.stretch(a, split, left, out);
                self.stretch(split, b, left, out);
                return;
            }
        }
        out.push(b);
    }

    /// Where the tip path `a`–`b` leaves the region between its ends (by
    /// more than [`Walls::slack_mm`], the cone test's own allowance), the
    /// medial axis point it should have gone round by, at its own depth
    /// (#463). A chain's straight stretch can cut across a wall that bends
    /// between its points, the tip of a line's end (a thin wedge of
    /// material standing into the region) most of all: the Delaunay
    /// dual's edges join circumcentres a step apart, and a wedge narrower
    /// than the step passes between them. The room test cannot see that
    /// where the cone is a point: at the start plane the room at a
    /// crossing is 0, not less, so [`Fit::overcut`] would split the
    /// stretch onto the walls either side of the wedge and keep the move
    /// across it. Here the vertex the stretch cuts across
    /// ([`Walls::excursion`]) is gone round: the largest disc inside the
    /// walls tangent at it, its centre along the vertex's bisector into
    /// the region ([`Walls::ball_radius_mm`]), has its centre on the axis,
    /// with the disc's radius as its clearance.
    fn around(&self, a: Point3, b: Point3) -> Option<Point3> {
        let corner = self
            .walls
            .excursion(a.xy(), b.xy(), self.walls.slack_mm())?
            .corner?;
        let reach_mm = a.xy().distance_to(b.xy())
            + self
                .walls
                .clearance_mm(a.xy())
                .max(self.walls.clearance_mm(b.xy()));
        let radius_mm = self
            .walls
            .ball_radius_mm(corner.p, corner.bisector, reach_mm);
        let m = corner.p + corner.bisector * radius_mm;
        (m.distance_to(a.xy()) > EPS_MM && m.distance_to(b.xy()) > EPS_MM)
            .then(|| Point3::new(m.x, m.y, self.depth.z_mm(radius_mm)))
    }

    /// Where the cone along `a`–`b` reaches furthest past a wall, set to
    /// its own depth, when it does by more than [`FIT_SLACK_MM`] away from
    /// the ends.
    fn overcut(&self, a: Point3, b: Point3) -> Option<Point3> {
        let (room_mm, worst) =
            self.walls
                .least_room(a, b, self.depth.start_plane_z_mm, self.depth.slope);
        let inside =
            worst.xy().distance_to(a.xy()) > EPS_MM && worst.xy().distance_to(b.xy()) > EPS_MM;
        (room_mm < -FIT_SLACK_MM && inside).then(|| {
            let own_z = self.depth.z_mm(self.walls.clearance_mm(worst.xy()));
            Point3::new(worst.x, worst.y, own_z.max(worst.z))
        })
    }

    /// With `axis_mm` (#457), the medial axis point the stretch `a`–`b`
    /// carves shortest of, on the axis at its own depth, when the
    /// stretch's cone stops more than `axis_mm` short of the walls it
    /// touches. The stretch is read every [`DEEPEN_SAMPLE_MM`] short of
    /// its ends; each point's axis point is found by shrinking a disc
    /// tangent at its nearest wall point, along the line through it
    /// (`Walls::ball_radius_mm`), and the stretch carves within the
    /// tolerance of that disc's walls exactly when its cone reaches `slope
    /// · (low − z)` of the disc's own depth `z`, `low` the lowest its cone
    /// comes over the disc's centre ([`cone_low_mm`]). Points at the flat
    /// depth are left to the floor. A chain's points are medial nodes, a
    /// step apart and merged where they crowd, so the straight cut between
    /// two of them, and above all into a junction, can run off the axis
    /// even unsimplified.
    fn off_axis(&self, a: Point3, b: Point3) -> Option<Point3> {
        let tolerance_mm = self.axis_mm?;
        let slope = self.depth.slope;
        let floor_mm = self.depth.flat_depth_mm.map(|f| f * slope);
        let pieces = (a.xy().distance_to(b.xy()) / DEEPEN_SAMPLE_MM).ceil();
        let mut worst: Option<(f64, Point3)> = None;
        let mut k = 1.0;
        while k < pieces {
            let t = k / pieces;
            k += 1.0;
            let x = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let (near_mm, w) = self.walls.nearest(x)?;
            // #463: a point the stretch runs outside the walls at (within
            // their slack) has its nearest wall behind it, and the disc
            // from there would grow into the material.
            if near_mm <= EPS_MM
                || floor_mm.is_some_and(|f| near_mm >= f)
                || !self.walls.holds(x, 0.0)
            {
                continue;
            }
            let u = (x - w) * (1.0 / near_mm);
            let radius_mm = self.walls.ball_radius_mm(w, u, near_mm + AXIS_REACH_MM);
            let m = w + u * radius_mm;
            let own_z = self.depth.z_mm(radius_mm);
            let short_mm = slope * (cone_low_mm(a, b, m, slope) - own_z);
            if short_mm > tolerance_mm && worst.is_none_or(|(s, _)| short_mm > s) {
                worst = Some((short_mm, Point3::new(m.x, m.y, own_z)));
            }
        }
        let (_, m) = worst?;
        (m.xy().distance_to(a.xy()) > EPS_MM && m.xy().distance_to(b.xy()) > EPS_MM).then_some(m)
    }

    /// With `deepen_mm`, the point of `a`–`b`, read every
    /// [`DEEPEN_SAMPLE_MM`] short of its ends, whose own depth is deepest
    /// below the stretch, at that depth, when it is more than `deepen_mm`
    /// below.
    fn undercut(&self, a: Point3, b: Point3) -> Option<Point3> {
        let slack_mm = self.deepen_mm?;
        let pieces = (a.xy().distance_to(b.xy()) / DEEPEN_SAMPLE_MM).ceil();
        let mut worst: Option<(f64, Point3)> = None;
        let mut k = 1.0;
        while k < pieces {
            let t = k / pieces;
            let p = Point3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            );
            let own_z = self.depth.z_mm(self.walls.clearance_mm(p.xy()));
            let below_mm = p.z - own_z;
            if below_mm > slack_mm && worst.is_none_or(|(w, _)| below_mm > w) {
                worst = Some((below_mm, Point3::new(p.x, p.y, own_z)));
            }
            k += 1.0;
        }
        worst.map(|(_, p)| p)
    }
}
