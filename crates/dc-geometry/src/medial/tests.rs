// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/vcarveMedial.test.ts (geometry tests)
//   src/engine/toolpaths/vcarveMedial/resolution.test.ts
//   src/engine/toolpaths/vcarveMedial/noiseRegression.test.ts

//! The medial axis tests (`docs/specs/phase-3/design.md`, "Test plan →
//! dc-geometry — medial"). Upstream's tests run on Y-down point loops; these
//! run on Y-up regions. A Voronoi skeleton is the same under reflection, so
//! no expected value changes, except where a text fixture's topology sits on
//! the noise floor (`fresh_quarter_inch_glyph_rejects_grid_corner_noise`).
//! The outline-text regions come from `tests/fixtures/vcarve/`, captured
//! from PureCutCNC by `tools/parity/vcarve-fixtures.ts`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
    reason = "test keys and counts are small; index loops mirror upstream's"
)]

use super::*;
use crate::assert_approx_eq;
use crate::contour::Contour;
use crate::distance::point_segment_distance_mm;
use crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;
use crate::polyline::Vertex;
use crate::region::{nest, nest_unchecked};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

const TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

// ---------------------------------------------------------------------------
// Shape helpers
// ---------------------------------------------------------------------------

fn rect_loop(x: f64, y: f64, w: f64, h: f64) -> Vec<Point> {
    vec![
        Point::new(x, y),
        Point::new(x + w, y),
        Point::new(x + w, y + h),
        Point::new(x, y + h),
    ]
}

fn regular_polygon(cx: f64, cy: f64, radius: f64, sides: usize) -> Vec<Point> {
    (0..sides)
        .map(|i| {
            let angle = TAU * i as f64 / sides as f64;
            Point::new(cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect()
}

fn l_shape() -> Vec<Point> {
    vec![
        Point::new(0.0, 0.0),
        Point::new(30.0, 0.0),
        Point::new(30.0, 10.0),
        Point::new(10.0, 10.0),
        Point::new(10.0, 30.0),
        Point::new(0.0, 30.0),
    ]
}

fn closed(points: &[Point]) -> Polyline {
    Polyline::new(
        points.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
        true,
    )
}

/// The region with outer `outer` and holes `holes`, whatever their winding.
fn region(outer: &[Point], holes: &[Vec<Point>]) -> Region {
    let contours: Vec<Contour> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .map(|l| Contour::new(closed(l)).expect("fixture loop is a simple polygon"))
        .collect();
    let (mut regions, warnings) = nest(contours).expect("fixture loops nest");
    assert!(
        warnings.is_empty(),
        "fixture loops nest cleanly: {warnings:?}"
    );
    assert_eq!(regions.len(), 1, "fixture loops make one region");
    assert_eq!(
        regions[0].holes().len(),
        holes.len(),
        "every hole is a hole"
    );
    regions.remove(0)
}

fn loops_of(region: &Region) -> Vec<Vec<Point>> {
    std::iter::once(region.outer())
        .chain(region.holes())
        .map(|c| c.polyline().vertices().iter().map(|v| v.point()).collect())
        .collect()
}

/// Exact distance from `p` to the nearest segment of `region`.
fn distance_to_boundary(region: &Region, p: Point) -> f64 {
    std::iter::once(region.outer())
        .chain(region.holes())
        .flat_map(|c| c.polyline().segments().collect::<Vec<_>>())
        .map(|s| point_segment_distance_mm(s, p))
        .fold(f64::INFINITY, f64::min)
}

fn component_count(graph: &MedialGraph) -> usize {
    let mut seen = vec![false; graph.nodes.len()];
    let mut components = 0;
    for start in 0..graph.nodes.len() {
        if seen[start] {
            continue;
        }
        components += 1;
        seen[start] = true;
        let mut queue = vec![start];
        while let Some(node) = queue.pop() {
            for &n in &graph.adjacency[node] {
                if !seen[n] {
                    seen[n] = true;
                    queue.push(n);
                }
            }
        }
    }
    components
}

fn edge_count(graph: &MedialGraph) -> usize {
    graph.adjacency.iter().map(Vec::len).sum::<usize>() / 2
}

fn zero_clearance_nodes_near(graph: &MedialGraph, p: Point, radius: f64) -> usize {
    graph
        .nodes
        .iter()
        .filter(|n| n.clearance_mm < 1e-9 && n.p.distance_to(p) <= radius)
        .count()
}

fn leaf_count(graph: &MedialGraph) -> usize {
    graph.adjacency.iter().filter(|n| n.len() == 1).count()
}

/// Leaves that are not zero-clearance corner tips (`noiseRegression.test.ts`).
fn non_corner_leaves(graph: &MedialGraph) -> usize {
    let tips = graph
        .nodes
        .iter()
        .zip(&graph.adjacency)
        .filter(|(n, adj)| n.clearance_mm < 1e-9 && adj.len() == 1)
        .count();
    leaf_count(graph) - tips
}

/// How many chains PureCutCNC's `extractChains` (`vcarveMedial/toolpath.ts`)
/// splits `graph` into: runs between nodes of degree other than 2, then
/// pure cycles, then isolated nodes. The chains themselves are slice 4's
/// (`dc-cam::vcarve::medial`); the noise tests only count them.
fn chain_count(graph: &MedialGraph) -> usize {
    let n = graph.nodes.len();
    let degree: Vec<usize> = graph.adjacency.iter().map(Vec::len).collect();
    let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
    let mut visited = BTreeSet::new();
    let walk = |start: usize, next: usize, visited: &mut BTreeSet<(usize, usize)>| {
        let (mut prev, mut curr) = (start, next);
        visited.insert(key(start, next));
        while degree[curr] == 2 && curr != start {
            let (a, b) = (graph.adjacency[curr][0], graph.adjacency[curr][1]);
            let following = if a == prev { b } else { a };
            if !visited.insert(key(curr, following)) {
                break;
            }
            prev = curr;
            curr = following;
        }
    };
    let mut chains = 0;
    for pass_degree_two in [false, true] {
        for node in 0..n {
            let wanted = if pass_degree_two {
                degree[node] == 2
            } else {
                degree[node] != 2 && degree[node] != 0
            };
            if !wanted {
                continue;
            }
            for &neighbour in &graph.adjacency[node] {
                if visited.contains(&key(node, neighbour)) {
                    continue;
                }
                chains += 1;
                walk(node, neighbour, &mut visited);
            }
        }
    }
    chains + degree.iter().filter(|&&d| d == 0).count()
}

fn graph_of(region: &Region) -> (Resolution, MedialGraph) {
    let res = resolution(region).expect("region has a resolution");
    (res, medial_axis(region, res.step_mm, TOL))
}

// ---------------------------------------------------------------------------
// Text fixtures (tests/fixtures/vcarve/*.json)
// ---------------------------------------------------------------------------

/// PureCutCNC's numbers for one fixture region, from its README table.
#[derive(Debug, Clone, Copy)]
struct Stats {
    /// In PureCutCNC's project units.
    resolution: f64,
    nodes: usize,
    edges: usize,
    components: usize,
    corners: usize,
    non_corner_leaves: usize,
    chains: usize,
}

struct FixtureRegion {
    region: Region,
    /// PureCutCNC on its own (Y-down, project-unit) input.
    upstream: Stats,
    /// PureCutCNC on the written (Y-up, mm) coordinates.
    y_up: Stats,
}

struct FixtureCase {
    /// mm per PureCutCNC project unit.
    scale: f64,
    regions: Vec<FixtureRegion>,
}

fn stats(value: &serde_json::Value) -> Stats {
    let count = |k: &str| value[k].as_u64().expect("count") as usize;
    Stats {
        resolution: value["resolution"].as_f64().expect("resolution"),
        nodes: count("nodes"),
        edges: count("edges"),
        components: count("components"),
        corners: count("corners"),
        non_corner_leaves: count("non_corner_leaves"),
        chains: count("chains"),
    }
}

fn points(value: &serde_json::Value) -> Vec<Point> {
    value
        .as_array()
        .expect("loop")
        .iter()
        .map(|p| Point::new(p[0].as_f64().expect("x"), p[1].as_f64().expect("y")))
        .collect()
}

fn fixture(json: &str) -> Vec<FixtureCase> {
    let root: serde_json::Value = serde_json::from_str(json).expect("fixture is JSON");
    root["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| FixtureCase {
            scale: case["scale"].as_f64().expect("scale"),
            regions: case["regions"]
                .as_array()
                .expect("regions")
                .iter()
                .map(|r| {
                    let holes: Vec<Vec<Point>> = r["holes"]
                        .as_array()
                        .expect("holes")
                        .iter()
                        .map(points)
                        .collect();
                    FixtureRegion {
                        region: region(&points(&r["outer"]), &holes),
                        upstream: stats(&r["purecutcnc"]),
                        y_up: stats(&r["purecutcnc_y_up"]),
                    }
                })
                .collect(),
        })
        .collect()
}

const RAG: &str = include_str!("../../../../tests/fixtures/vcarve/rag.json");
const G: &str = include_str!("../../../../tests/fixtures/vcarve/g.json");
const GA_LARGE: &str = include_str!("../../../../tests/fixtures/vcarve/ga-large.json");
const GA_SMALL: &str = include_str!("../../../../tests/fixtures/vcarve/ga-small.json");

/// `shape` scaled by `by` about the origin. Upstream scales the point loops
/// and checks nothing; so does this. At 0.2 × the glyphs' near-touching
/// vertices come within `Contour::new`'s self-intersection tolerance, which
/// says nothing about the medial axis, so the region is built without it.
fn scaled(shape: &Region, by: f64) -> Region {
    let contours: Vec<Contour> = loops_of(shape)
        .iter()
        .map(|l| {
            let small: Vec<Point> = l.iter().map(|p| Point::new(p.x * by, p.y * by)).collect();
            Contour::oriented(closed(&small)).expect("a scaled loop keeps its area")
        })
        .collect();
    let mut regions = nest_unchecked(contours);
    assert_eq!(regions.len(), 1, "the scaled loops make one region");
    regions.remove(0)
}

// ---------------------------------------------------------------------------
// Medial axis geometry (vcarveMedial.test.ts)
// ---------------------------------------------------------------------------

/// Port of `testRectangleSpineAndCorners`: a 40 × 10 rectangle at step 0.5
/// is one component; every clearance is the exact boundary distance; the
/// spine near (20, 5) has clearance 5; each corner has exactly one
/// zero-clearance tip.
#[test]
fn rectangle_spine_has_exact_clearances_and_corner_tips() {
    let outer = rect_loop(0.0, 0.0, 40.0, 10.0);
    let r = region(&outer, &[]);
    let graph = medial_axis(&r, 0.5, TOL);

    assert!(
        !graph.nodes.is_empty(),
        "expected medial nodes for the rectangle"
    );
    assert_eq!(component_count(&graph), 1);
    for n in &graph.nodes {
        let exact = distance_to_boundary(&r, n.p);
        assert!(
            (n.clearance_mm - exact).abs() < 1e-6,
            "clearance {} != exact boundary distance {exact} at {:?}",
            n.clearance_mm,
            n.p
        );
    }
    let spine = graph
        .nodes
        .iter()
        .find(|n| (n.p.x - 20.0).abs() < 3.0 && (n.p.y - 5.0).abs() < 0.05)
        .expect("a spine node near (20, 5)");
    assert!(
        (spine.clearance_mm - 5.0).abs() < 0.02,
        "spine clearance {}",
        spine.clearance_mm
    );
    for corner in &outer {
        assert_eq!(
            zero_clearance_nodes_near(&graph, *corner, 1e-6),
            1,
            "zero-clearance tip at corner {corner:?}"
        );
    }
}

/// Port of `testSquareCenterJunction`: a 20 × 20 square's deepest node is
/// within 0.3 of the centre with clearance 10 ± 0.05; connected; four tips.
#[test]
fn square_diagonals_meet_at_centre() {
    let outer = rect_loop(0.0, 0.0, 20.0, 20.0);
    let graph = medial_axis(&region(&outer, &[]), 0.5, TOL);

    let centre = graph
        .nodes
        .iter()
        .copied()
        .reduce(|best, n| {
            if n.clearance_mm > best.clearance_mm {
                n
            } else {
                best
            }
        })
        .expect("nodes");
    assert!(
        centre.p.distance_to(Point::new(10.0, 10.0)) < 0.3,
        "deepest node {:?}",
        centre.p
    );
    assert!(
        (centre.clearance_mm - 10.0).abs() < 0.05,
        "centre clearance {}",
        centre.clearance_mm
    );
    assert_eq!(component_count(&graph), 1);
    for corner in &outer {
        assert_eq!(
            zero_clearance_nodes_near(&graph, *corner, 1e-6),
            1,
            "tip at {corner:?}"
        );
    }
}

/// Port of `testCircleCollapsesToCenter`: a 64-gon of radius 8 at step 0.4
/// keeps no flattening spokes: every node is within 1 of the centre with
/// clearance above 6, and the deepest equals the apothem ± 0.05.
#[test]
fn circle_collapses_to_centre() {
    let outer = regular_polygon(10.0, 10.0, 8.0, 64);
    let graph = medial_axis(&region(&outer, &[]), 0.4, TOL);

    assert!(
        !graph.nodes.is_empty(),
        "expected at least one node for the circle"
    );
    for n in &graph.nodes {
        let off = n.p.distance_to(Point::new(10.0, 10.0));
        assert!(
            off < 1.0,
            "node {:?} is {off} from the centre: a spoke",
            n.p
        );
        assert!(
            n.clearance_mm > 6.0,
            "node clearance {} too small",
            n.clearance_mm
        );
    }
    let apothem = 8.0 * (PI / 64.0).cos();
    let deepest = graph
        .nodes
        .iter()
        .map(|n| n.clearance_mm)
        .fold(0.0, f64::max);
    assert!(
        (deepest - apothem).abs() < 0.05,
        "deepest {deepest} != apothem {apothem}"
    );
}

/// Port of `testRingWithIsland`: a 40 mm square with a 20 mm island is one
/// component with a cycle (edges ≥ nodes); no clearance exceeds the corner
/// pocket's 10·(2−√2); the corridor at (20, 5) has clearance 5; the outer
/// corners have tips and the island's do not; every edge stays inside.
#[test]
fn ring_with_island_is_a_cycle_without_island_tips() {
    let outer = rect_loop(0.0, 0.0, 40.0, 40.0);
    let island = rect_loop(10.0, 10.0, 20.0, 20.0);
    let r = region(&outer, std::slice::from_ref(&island));
    let graph = medial_axis(&r, 0.5, TOL);

    assert!(!graph.nodes.is_empty(), "expected nodes for the ring");
    assert_eq!(component_count(&graph), 1);
    assert!(
        edge_count(&graph) >= graph.nodes.len(),
        "the ring's axis has a cycle"
    );
    let corner_pocket_max = 10.0 * (2.0 - 2.0_f64.sqrt());
    for n in &graph.nodes {
        let exact = distance_to_boundary(&r, n.p);
        assert!(
            (n.clearance_mm - exact).abs() < 1e-6,
            "clearance mismatch at {:?}",
            n.p
        );
        assert!(
            n.clearance_mm <= corner_pocket_max + 1e-6,
            "clearance {} beyond the corner pocket's",
            n.clearance_mm
        );
    }
    let corridor = graph
        .nodes
        .iter()
        .find(|n| (n.p.x - 20.0).abs() < 2.0 && (n.p.y - 5.0).abs() < 0.05)
        .expect("a corridor node near (20, 5)");
    assert!(
        (corridor.clearance_mm - 5.0).abs() < 0.02,
        "corridor {}",
        corridor.clearance_mm
    );
    for corner in &outer {
        assert_eq!(
            zero_clearance_nodes_near(&graph, *corner, 1e-6),
            1,
            "tip at {corner:?}"
        );
    }
    for corner in &island {
        assert_eq!(
            zero_clearance_nodes_near(&graph, *corner, 0.5),
            0,
            "tip at island {corner:?}"
        );
    }
    let outer_line = closed(&outer);
    let island_line = closed(&island);
    for (i, adj) in graph.adjacency.iter().enumerate() {
        for &j in adj.iter().filter(|&&j| j > i) {
            let (a, b) = (graph.nodes[i].p, graph.nodes[j].p);
            let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            assert!(
                point_in_loops(mid, &[&outer_line, &island_line]),
                "edge midpoint {mid:?} leaves the ring"
            );
        }
    }
}

/// Port of `testLShapeJunctionAndReflexCorner`: the L's axis is connected
/// with a junction of degree ≥ 3, a tip at each of the five convex corners
/// and none within 0.5 of the reflex corner (10, 10).
#[test]
fn l_shape_has_a_junction_and_no_reflex_tip() {
    let outer = l_shape();
    let graph = medial_axis(&region(&outer, &[]), 0.5, TOL);

    assert_eq!(component_count(&graph), 1);
    let max_degree = graph.adjacency.iter().map(Vec::len).max().unwrap_or(0);
    assert!(
        max_degree >= 3,
        "expected a junction, max degree {max_degree}"
    );
    for corner in outer
        .iter()
        .filter(|p| p.distance_to(Point::new(10.0, 10.0)) > 1e-9)
    {
        assert_eq!(
            zero_clearance_nodes_near(&graph, *corner, 1e-6),
            1,
            "tip at {corner:?}"
        );
    }
    assert_eq!(
        zero_clearance_nodes_near(&graph, Point::new(10.0, 10.0), 0.5),
        0
    );
}

/// Port of `testRegionConvexCorners`: the L has 5 convex corners, the ring
/// only its 4 outer ones, the 64-gon none, and an island's notch apex at
/// (20, 20) is a corner of the region around it.
#[test]
fn convex_corners_are_classified() {
    let corners = |r: &Region| convex_corners(r, CORNER_ANGLE_RAD, TOL);
    assert_eq!(corners(&region(&l_shape(), &[])).len(), 5);

    let ring = region(
        &rect_loop(0.0, 0.0, 40.0, 40.0),
        &[rect_loop(10.0, 10.0, 20.0, 20.0)],
    );
    assert_eq!(corners(&ring).len(), 4);

    let circle = region(&regular_polygon(0.0, 0.0, 8.0, 64), &[]);
    assert_eq!(corners(&circle).len(), 0);

    let notched_island = vec![
        Point::new(10.0, 10.0),
        Point::new(30.0, 10.0),
        Point::new(30.0, 30.0),
        Point::new(21.0, 30.0),
        Point::new(20.0, 20.0),
        Point::new(19.0, 30.0),
        Point::new(10.0, 30.0),
    ];
    let notched = region(&rect_loop(0.0, 0.0, 40.0, 40.0), &[notched_island]);
    assert!(
        corners(&notched)
            .iter()
            .any(|c| c.p.distance_to(Point::new(20.0, 20.0)) < 1e-9),
        "the island's notch apex is a corner of the region"
    );
}

/// Port of `testAcuteCornerTipsSurviveContactFiltering`: a 20° isosceles
/// triangle at step 0.2 keeps three zero-clearance tips of degree 1.
#[test]
fn acute_corner_tips_survive_contact_filtering() {
    let apex_rad = 20.0_f64.to_radians();
    let half_base = 20.0 * (apex_rad / 2.0).tan();
    let triangle = vec![
        Point::new(0.0, 0.0),
        Point::new(half_base, 20.0),
        Point::new(-half_base, 20.0),
    ];
    let graph = medial_axis(&region(&triangle, &[]), 0.2, TOL);
    let tips: Vec<usize> = (0..graph.nodes.len())
        .filter(|&i| graph.nodes[i].clearance_mm < 1e-9)
        .collect();
    assert_eq!(tips.len(), 3, "the 20° triangle keeps 3 corner tips");
    for &i in &tips {
        assert_eq!(graph.adjacency[i].len(), 1, "tip {i} joins once");
    }
}

/// The 40 mm square with the notched island of
/// `convex_corners_are_classified`: the notch's apex at (20, 20), its
/// walls to (19, 30) and (21, 30), 11.4° apart, is a convex corner of the
/// region round the island.
fn notched_island_region() -> Region {
    let island = vec![
        Point::new(10.0, 10.0),
        Point::new(30.0, 10.0),
        Point::new(30.0, 30.0),
        Point::new(21.0, 30.0),
        Point::new(20.0, 20.0),
        Point::new(19.0, 30.0),
        Point::new(10.0, 30.0),
    ];
    region(&rect_loop(0.0, 0.0, 40.0, 40.0), &[island])
}

/// [`CornerReach::Every`] on every loop, with no floor.
const EVERY: CornerReach = CornerReach::Every {
    outer: true,
    floor_inset_mm: None,
};

/// #452: an island's convex corner's bisector points into the region
/// round it, as [`Corner::bisector`] says: the notch apex's points up the
/// notch, +Y. (Upstream's `regionConvexCorners` negates an island's,
/// which points into the island; [`CornerReach::Upstream`] keeps that.)
#[test]
fn island_corner_bisector_points_into_the_region() {
    let corners = convex_corners(&notched_island_region(), CORNER_ANGLE_RAD, TOL);
    let apex = corners
        .iter()
        .find(|c| c.p.distance_to(Point::new(20.0, 20.0)) < 1e-9)
        .expect("the apex is a corner");
    assert_approx_eq!(apex.bisector.x, 0.0, 1e-9);
    assert_approx_eq!(apex.bisector.y, 1.0, 1e-9);
}

/// #452: with [`CornerReach::Every`] the axis reaches the island's 11.4°
/// notch: a zero-clearance tip at the apex, joined once, and the notch's
/// branch, nodes up its bisector at their exact clearances. Upstream's
/// passes leave the notch with no node at all: its clearance falls
/// `sin 5.7°` = 0.1 per mm toward the apex, under the 0.25 spur pruning
/// asks of a corner's branch, so pruning unzips it, and the apex's tip
/// seeks its joint along a bisector into the island.
#[test]
fn every_corner_reaches_an_island_notch() {
    let r = notched_island_region();
    let step_mm = resolution(&r).expect("a resolution").step_mm;
    let apex = Point::new(20.0, 20.0);
    let in_notch = |graph: &MedialGraph| -> Vec<MedialNode> {
        graph
            .nodes
            .iter()
            .filter(|n| n.p.y > 20.0 && n.p.y < 29.0 && (n.p.x - 20.0).abs() < 1.0)
            .copied()
            .collect()
    };

    let upstream = medial_axis(&r, step_mm, TOL);
    assert!(
        in_notch(&upstream).is_empty(),
        "upstream: {:?}",
        in_notch(&upstream)
    );

    let every = medial_axis_with(&r, step_mm, TOL, EVERY);
    let tip = every
        .nodes
        .iter()
        .position(|n| n.p.distance_to(apex) < 1e-9 && n.clearance_mm < 1e-9)
        .expect("a tip at the apex");
    assert_eq!(every.adjacency[tip].len(), 1, "the tip joins once");
    let branch = in_notch(&every);
    assert!(
        branch.len() as f64 >= 8.0 / step_mm / 2.0,
        "{} nodes up the 9 mm notch at step {step_mm}",
        branch.len()
    );
    for n in &branch {
        assert_approx_eq!(n.clearance_mm, distance_to_boundary(&r, n.p), 1e-9);
        assert!((n.p.x - 20.0).abs() < step_mm, "{n:?} is off the bisector");
    }
}

/// #452: the 30 × 10 mm bar `blunt` ends in a point at (31.82, 5) whose
/// walls turn 40° there, under [`CORNER_ANGLE_RAD`], and the
/// contact-spread filter drops the branch into it (its contacts are 40°
/// apart, under 60°), so upstream gives it no tip. With
/// [`CornerReach::Every`] it gets one, joined to the axis by nodes at most
/// a step apart at their exact clearances, unless a floor `d` in from the
/// walls reaches it within 0.01 mm, as a floor of 0.1 mm does (`d (1 / cos
/// 20° − 1)` = 0.006 mm) and one of 1 mm does not (0.064 mm).
#[test]
fn every_corner_reaches_an_obtuse_corner() {
    let point = Point::new(30.0 + 5.0 * 20.0_f64.to_radians().tan(), 5.0);
    let blunt = vec![
        Point::new(0.0, 0.0),
        Point::new(30.0, 0.0),
        point,
        Point::new(30.0, 10.0),
        Point::new(0.0, 10.0),
    ];
    let r = region(&blunt, &[]);
    let step_mm = resolution(&r).expect("a resolution").step_mm;
    let tipped = |graph: &MedialGraph| zero_clearance_nodes_near(graph, point, 1e-9) == 1;
    assert!(!tipped(&medial_axis(&r, step_mm, TOL)), "upstream");
    let every = medial_axis_with(&r, step_mm, TOL, EVERY);
    assert!(tipped(&every), "every corner");
    for (i, n) in every.nodes.iter().enumerate() {
        assert_approx_eq!(n.clearance_mm, distance_to_boundary(&r, n.p), 1e-9);
        for &j in &every.adjacency[i] {
            let m = every.nodes[j];
            if m.p.distance_to(point) < 1e-9 || n.p.distance_to(point) < 1e-9 {
                let edge_mm = n.p.distance_to(m.p);
                assert!(edge_mm <= step_mm + 1e-9, "tip edge {edge_mm} mm long");
            }
        }
    }
    let floored = |inset_mm| {
        medial_axis_with(
            &r,
            step_mm,
            TOL,
            CornerReach::Every {
                outer: true,
                floor_inset_mm: Some(inset_mm),
            },
        )
    };
    assert!(!tipped(&floored(0.1)), "a 0.1 mm floor reaches the point");
    assert!(tipped(&floored(1.0)), "a 1 mm floor does not");
}

/// A region with an outer corner whose walls meet at `angle_deg`: the
/// apex at the origin, its walls 20 mm long either side of +X, closed by
/// a 10 mm deep box beyond their ends.
fn outer_corner_region(angle_deg: f64) -> Region {
    let half = (angle_deg / 2.0).to_radians();
    let (x, y) = (20.0 * half.cos(), 20.0 * half.sin());
    region(
        &[
            Point::new(0.0, 0.0),
            Point::new(x, -y),
            Point::new(x + 10.0, -y),
            Point::new(x + 10.0, y),
            Point::new(x, y),
        ],
        &[],
    )
}

/// A 60 mm square round a 30 mm square island whose top edge has a V
/// notch whose walls meet at `angle_deg`, the notch's apex at (30, 45 −
/// depth): the apex is a convex corner of the region round the island,
/// as the notch of a serif M is in a male Inlay's surround.
fn hole_corner_region(angle_deg: f64) -> Region {
    let half = (angle_deg / 2.0).to_radians();
    let depth_mm = (12.0 / half.tan()).min(10.0);
    let width_mm = depth_mm * half.tan();
    let island = vec![
        Point::new(15.0, 15.0),
        Point::new(45.0, 15.0),
        Point::new(45.0, 45.0),
        Point::new(30.0 + width_mm, 45.0),
        Point::new(30.0, 45.0 - depth_mm),
        Point::new(30.0 - width_mm, 45.0),
        Point::new(15.0, 45.0),
    ];
    region(&rect_loop(0.0, 0.0, 60.0, 60.0), &[island])
}

/// #456: [`CornerReach::Every`] reaches every convex corner, an outer's
/// and a hole's, whose walls meet at 10°, 23° (the serif M's notch of
/// #452), 90° or 133° (the star's inner vertex): for every convex corner
/// of each region some medial disc, a node's clearance about it, comes
/// within 0.01 mm of the corner. That disc is the carve's cone at the
/// start plane, so the carve cuts the corner crisp. Upstream's passes miss
/// some of these: spur pruning takes a branch into a corner under about
/// 29°, a hole's corner seeks its joint through the hole, and a corner
/// wider than 130° gets no tip (#452).
#[test]
fn every_convex_corner_gets_a_tip() {
    for angle_deg in [10.0, 23.0, 90.0, 133.0] {
        for (kind, r) in [
            ("outer", outer_corner_region(angle_deg)),
            ("hole", hole_corner_region(angle_deg)),
        ] {
            let step_mm = resolution(&r).expect("a resolution").step_mm;
            let graph = medial_axis_with(&r, step_mm, TOL, EVERY);
            let corners = convex_corners(&r, 0.0, TOL);
            assert!(!corners.is_empty(), "{kind} {angle_deg}°: no corners");
            for corner in corners {
                let gap_mm = graph
                    .nodes
                    .iter()
                    .map(|n| n.p.distance_to(corner.p) - n.clearance_mm)
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    gap_mm <= 0.01,
                    "{kind} {angle_deg}°: the corner at {:?} is {gap_mm:.4} mm from every medial disc",
                    corner.p
                );
            }
        }
    }
}

/// Port of `testAutomaticResolutionKeepsScaledTopologyStable`: a 40 × 10
/// and a 4 × 1 rectangle at their automatic steps (ratio 10) have equal
/// node, edge and component counts.
#[test]
fn scaled_rectangles_have_identical_topology() {
    let large = region(&rect_loop(0.0, 0.0, 40.0, 10.0), &[]);
    let small = region(&rect_loop(0.0, 0.0, 4.0, 1.0), &[]);
    let (large_res, large_graph) = graph_of(&large);
    let (small_res, small_graph) = graph_of(&small);
    assert!((large_res.step_mm / small_res.step_mm - 10.0).abs() < 1e-6);
    assert_eq!(large_graph.nodes.len(), small_graph.nodes.len());
    assert_eq!(edge_count(&large_graph), edge_count(&small_graph));
    assert_eq!(component_count(&large_graph), component_count(&small_graph));
}

/// Port of `testScaledOutlineTextTopologyStaysStable` on `rag.json`: each
/// region of "Rag" at 10 mm and at 0.2 × keeps its node, edge and
/// component counts, at steps in the same ratio. Also checks the 10 mm
/// counts against PureCutCNC's on the same coordinates (the fixture
/// README's Y-up column).
#[test]
fn scaled_outline_text_keeps_topology() {
    let cases = fixture(RAG);
    assert!(
        !cases[0].regions.is_empty(),
        "expected outline-text regions"
    );
    for (index, f) in cases[0].regions.iter().enumerate() {
        let small = scaled(&f.region, 0.2);
        let (large_res, large_graph) = graph_of(&f.region);
        let (small_res, small_graph) = graph_of(&small);
        assert!(
            (large_res.step_mm * 0.2 - small_res.step_mm).abs() < 1e-9,
            "region {index}: the step did not follow the scale"
        );
        assert_eq!(
            large_graph.nodes.len(),
            small_graph.nodes.len(),
            "region {index} gained or lost nodes when scaled"
        );
        assert_eq!(
            edge_count(&large_graph),
            edge_count(&small_graph),
            "region {index} edges"
        );
        assert_eq!(
            component_count(&large_graph),
            component_count(&small_graph),
            "region {index}"
        );
        assert_eq!(
            large_graph.nodes.len(),
            f.y_up.nodes,
            "region {index}: PureCutCNC's nodes"
        );
        assert_eq!(
            edge_count(&large_graph),
            f.y_up.edges,
            "region {index}: PureCutCNC's edges"
        );
        assert_eq!(
            component_count(&large_graph),
            f.y_up.components,
            "region {index}"
        );
    }
}

/// Port of `testOutlineGRejectsFlatteningSpokesAcrossScales` on `g.json`:
/// "g" at 10 and 30 mm resolves to one region each, the steps are in ratio
/// 3, each keeps at most 3 non-corner leaves and 18 chains, and both sizes
/// have the same number of chains.
#[test]
fn lowercase_g_rejects_flattening_spokes_at_both_sizes() {
    let cases = fixture(G);
    let stats: Vec<(f64, usize, usize)> = cases
        .iter()
        .map(|c| {
            assert_eq!(c.regions.len(), 1, "one lowercase-g region");
            let (res, graph) = graph_of(&c.regions[0].region);
            let tips = graph.nodes.iter().filter(|n| n.clearance_mm < 1e-9).count();
            (res.step_mm, leaf_count(&graph) - tips, chain_count(&graph))
        })
        .collect();
    assert!(
        (stats[1].0 / stats[0].0 - 3.0).abs() < 1e-9,
        "the step did not follow the size"
    );
    for (size, &(_, leaves, chains)) in [10, 30].iter().zip(&stats) {
        assert!(
            leaves <= 3,
            "g at {size} mm kept {leaves} non-corner leaves"
        );
        assert!(chains <= 18, "g at {size} mm kept {chains} chains");
    }
    assert_eq!(stats[0].2, stats[1].2, "g topology changed across sizes");
}

/// The geometry half of `testDotRegionCollapsesToSinglePlunge`: a 32-gon of
/// radius 1 at step 0.35 is a dot, one node.
#[test]
fn dot_region_is_one_node() {
    let graph = medial_axis(&region(&regular_polygon(5.0, 5.0, 1.0, 32), &[]), 0.35, TOL);
    assert_eq!(graph.nodes.len(), 1, "a dot is one node");
    assert!(graph.adjacency[0].is_empty());
}

/// Upstream reads either winding (`convex_corners` uses `cross · area`);
/// DreamCarve's regions are normalised, but the loops the axis is built on
/// may come either way. Clockwise loops give the same nodes and edges as
/// counter-clockwise ones (DreamCarve's Y-up note: a Y flip reverses
/// winding).
#[test]
fn winding_does_not_change_the_graph() {
    let canonical = |g: &MedialGraph| {
        let key = |p: Point| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
        let nodes: BTreeSet<_> = g
            .nodes
            .iter()
            .map(|n| (key(n.p), (n.clearance_mm * 1e6).round() as i64))
            .collect();
        let edges: BTreeSet<_> = g
            .adjacency
            .iter()
            .enumerate()
            .flat_map(|(i, adj)| {
                adj.iter().map(move |&j| {
                    let (a, b) = (key(g.nodes[i].p), key(g.nodes[j].p));
                    if a < b { (a, b) } else { (b, a) }
                })
            })
            .collect();
        (nodes, edges)
    };
    let shapes = [
        vec![l_shape()],
        vec![
            rect_loop(0.0, 0.0, 40.0, 40.0),
            rect_loop(10.0, 10.0, 20.0, 20.0),
        ],
        vec![regular_polygon(0.0, 0.0, 8.0, 64)],
    ];
    for loops in shapes {
        let ccw = medial_axis_of_loops(&loops, 0.5, CornerReach::Upstream);
        let reversed: Vec<Vec<Point>> = loops
            .iter()
            .map(|l| l.iter().rev().copied().collect())
            .collect();
        let cw = medial_axis_of_loops(&reversed, 0.5, CornerReach::Upstream);
        assert!(!ccw.nodes.is_empty(), "the shape has an axis");
        assert_eq!(
            canonical(&ccw),
            canonical(&cw),
            "winding changed the axis of {loops:?}"
        );
    }
}

/// Determinism (`docs/rust-guidelines.md`, "Floating point and
/// determinism"): the same region twice gives the identical graph.
#[test]
fn same_region_twice_is_identical() {
    for f in &fixture(RAG)[0].regions {
        let (_, first) = graph_of(&f.region);
        let (_, second) = graph_of(&f.region);
        assert!(!first.nodes.is_empty());
        assert_eq!(first, second);
    }
}

// ---------------------------------------------------------------------------
// Resolution (resolution.test.ts)
// ---------------------------------------------------------------------------

fn rect_res(w: f64, h: f64) -> Resolution {
    resolution(&region(&rect_loop(0.0, 0.0, w, h), &[])).expect("a resolution")
}

/// Port of `testResolutionScalesWithShortSpan`: 20 × 10 samples at 0.0625,
/// 2 × 1 at 0.00625.
#[test]
fn resolution_scales_with_short_span() {
    let (large, small) = (rect_res(20.0, 10.0), rect_res(2.0, 1.0));
    assert!((large.step_mm - 0.0625).abs() < 1e-9, "{}", large.step_mm);
    assert!((small.step_mm - 0.00625).abs() < 1e-9, "{}", small.step_mm);
    assert!((large.step_mm / small.step_mm - 10.0).abs() < 1e-9);
}

/// Port of `testLargeShapeKeepsScaleRelativeResolution`: a 100 mm square
/// samples at 0.625 and is not budget-limited.
#[test]
fn large_shape_keeps_scale_relative_resolution() {
    let res = rect_res(100.0, 100.0);
    assert!((res.step_mm - 0.625).abs() < 1e-9, "{}", res.step_mm);
    assert!(!res.budget_limited);
}

/// Port of `testResolutionIsUnitEquivalent`: 25.4 × 12.7 mm samples 25.4
/// times as finely spaced as 1 × 0.5, the same shape in inches.
#[test]
fn resolution_is_unit_equivalent() {
    let (mm, inch) = (rect_res(25.4, 12.7), rect_res(1.0, 0.5));
    assert!((mm.step_mm - inch.step_mm * 25.4).abs() < 1e-9);
}

/// Port of `testDegenerateRegionIsRejected`: collinear points have no
/// resolution. A `Region` cannot be collinear, so this runs on the loops.
#[test]
fn degenerate_region_is_rejected() {
    let line = closed(&[
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(2.0, 0.0),
    ]);
    assert_eq!(resolve(&line, &[]), None);
}

/// Port of `testSampleBudgetOverridesShapeResolution`: a 100 000 × 1 strip
/// takes the budget floor, perimeter / 40 000, and says so.
#[test]
fn sample_budget_overrides_shape_resolution() {
    let res = rect_res(100_000.0, 1.0);
    let floor = 200_002.0 / MEDIAL_SAMPLE_BUDGET;
    assert!((res.budget_floor_mm - floor).abs() < 1e-9);
    assert!((res.step_mm - floor).abs() < 1e-9);
    assert!(res.budget_limited);
}

// ---------------------------------------------------------------------------
// Noise regression (noiseRegression.test.ts)
// ---------------------------------------------------------------------------

/// Resolution (mm), corners, non-corner leaves and chains of a fixture
/// region, as `regionStats` computes them.
fn region_stats(r: &Region) -> (f64, usize, usize, usize) {
    let (res, graph) = graph_of(r);
    (
        res.step_mm,
        convex_corners(r, CORNER_ANGLE_RAD, TOL).len(),
        non_corner_leaves(&graph),
        chain_count(&graph),
    )
}

/// Port of `testSavedGlyphProjectRejectsNoiseAcrossLinkedScales` on
/// `ga-large.json`: both operations of the saved "gA" project resolve "g"
/// and "A" separately, at the upstream resolutions (inches there, × 25.4
/// here) within the proven clean range (≤ 0.005 in), with the upstream
/// topology: "g" 1 corner, 2 non-corner leaves, 9 chains; "A" 10, 0, 20.
/// PureCutCNC gives the same counts on the Y-up coordinates.
#[test]
fn saved_glyph_project_rejects_noise_across_linked_scales() {
    let cases = fixture(GA_LARGE);
    assert_eq!(cases.len(), 2, "two medial operations");
    let expected_resolutions = [[0.00241, 0.00248], [0.004403125, 0.004828125]];
    let expected_topology = [(1, 2, 9), (10, 0, 20)];
    for (op, case) in cases.iter().enumerate() {
        assert_eq!(
            case.regions.len(),
            2,
            "operation {op} resolves g and A separately"
        );
        for (index, f) in case.regions.iter().enumerate() {
            let (step_mm, corners, leaves, chains) = region_stats(&f.region);
            let expected_mm = expected_resolutions[op][index] * case.scale;
            assert!(
                (step_mm - expected_mm).abs() < 1e-9,
                "operation {op} region {index}: step {step_mm} mm, expected {expected_mm}"
            );
            assert!(
                step_mm <= 0.005 * case.scale,
                "outside the proven clean range"
            );
            assert!((f.upstream.resolution * case.scale - expected_mm).abs() < 1e-9);
            let (c, l, n) = expected_topology[index];
            assert_eq!(
                (corners, leaves, chains),
                (c, l, n),
                "operation {op} region {index}: corners, non-corner leaves, chains"
            );
            assert_eq!(
                (f.y_up.corners, f.y_up.non_corner_leaves, f.y_up.chains),
                (c, l, n)
            );
        }
    }
}

/// Port of `testFreshQuarterInchGlyphRejectsGridCornerNoise` on
/// `ga-small.json`: "gA" regenerated at 0.25 in resolves at 0.001506875 and
/// 0.00155 in, keeps 1 and 10 corners, and keeps "g" to at most 2
/// non-corner leaves and 15 chains. For "A" upstream asserts at most 2 and
/// 24; PureCutCNC itself keeps 3 and 26 on the same glyph in millimetres,
/// Y-up (the fixture README's Y-up column: the frame change alone keeps
/// one more spur), so the bound here is PureCutCNC's own count on the
/// input this test gives.
#[test]
fn fresh_quarter_inch_glyph_rejects_grid_corner_noise() {
    let cases = fixture(GA_SMALL);
    let case = &cases[0];
    assert_eq!(
        case.regions.len(),
        2,
        "the quarter-inch operation resolves g and A"
    );
    let g = region_stats(&case.regions[0].region);
    let a = region_stats(&case.regions[1].region);
    assert!(
        (g.0 - 0.001506875 * case.scale).abs() < 1e-9,
        "g step {}",
        g.0
    );
    assert!((a.0 - 0.00155 * case.scale).abs() < 1e-9, "A step {}", a.0);
    assert_eq!(g.1, 1, "g corner tips");
    assert_eq!(a.1, 10, "A corner tips");
    assert!(g.2 <= 2, "g kept {} non-corner leaves", g.2);
    assert!(g.3 <= 15, "g kept {} chains", g.3);
    let a_y_up = case.regions[1].y_up;
    assert!(
        a.2 <= a_y_up.non_corner_leaves.max(2),
        "A kept {} non-corner leaves",
        a.2
    );
    assert!(a.3 <= a_y_up.chains.max(24), "A kept {} chains", a.3);
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// The seed every run starts from (as `tests/properties.rs`).
const SEED: u64 = 0x0402_d0ca_4e00_0402;

fn config(cases: u32) -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(cases);
    config.rng_seed = RngSeed::Fixed(SEED);
    config.failure_persistence = None;
    config
}

/// A convex polygon: 3 to 9 vertices on an ellipse with semi-axes 5–50 mm
/// about a centre in ±100 mm, at angles a positive gap apart.
fn convex_polygon() -> impl Strategy<Value = Vec<Point>> {
    (
        -100.0..100.0_f64,
        -100.0..100.0_f64,
        5.0..50.0_f64,
        5.0..50.0_f64,
        prop::collection::vec(0.2..1.0_f64, 3..=9),
    )
        .prop_map(|(cx, cy, a, b, gaps)| {
            let total: f64 = gaps.iter().sum();
            let mut angle = 0.0_f64;
            gaps.iter()
                .map(|g| {
                    let p = Point::new(cx + a * angle.cos(), cy + b * angle.sin());
                    angle += TAU * g / total;
                    p
                })
                .collect()
        })
}

proptest! {
    #![proptest_config(config(12))]

    /// Every node lies in the region (a corner tip on its boundary).
    #[test]
    fn every_node_is_inside_its_region(outer in convex_polygon()) {
        let r = region(&outer, &[]);
        let (_, graph) = graph_of(&r);
        prop_assert!(!graph.nodes.is_empty());
        for n in &graph.nodes {
            prop_assert!(r.contains_point(n.p), "node {:?} is outside", n.p);
        }
    }

    /// Every node's clearance is its exact distance to the boundary.
    #[test]
    fn every_clearance_equals_the_exact_distance(outer in convex_polygon()) {
        let r = region(&outer, &[]);
        let (_, graph) = graph_of(&r);
        for n in &graph.nodes {
            let exact = distance_to_boundary(&r, n.p);
            prop_assert!(
                (n.clearance_mm - exact).abs() < 1e-9,
                "clearance {} != {exact} at {:?}", n.clearance_mm, n.p
            );
        }
    }
}

/// #463: no corner's tip is joined to another's. A 70 mm square whose top
/// is a wave, `amp` high and `wave_mm` long, flattened every `every_mm`, as
/// a traced drawing's curve is: near each inflection the vertices turn a
/// few degrees into the region and stand a few hundredths beyond the axis,
/// so each gets a tip (`CornerReach::Every`). Upstream's joint scores could
/// join the next vertex's tip to the one before (a tip "aligned" with its
/// bisector, at distance 0.12, scores better than the axis a step or more
/// away), and the branch ran along the wall: a V-bit's tip on the outline
/// all the way, which S17 refuses. On the maintainer's line-art eagle that
/// was the next failure after the first. Every convex corner is still
/// reached within 0.01 mm (#456).
#[test]
fn no_corner_tip_is_joined_to_another() {
    for (amp_mm, wave_mm, every_mm) in [(1.0, 10.0, 0.15), (0.5, 5.0, 0.12), (1.0, 4.0, 0.12)] {
        let mut outer = vec![Point::new(0.0, 0.0), Point::new(70.0, 0.0)];
        let pieces = (70.0 / every_mm) as usize;
        for k in (0..=pieces).rev() {
            let x = k as f64 * every_mm;
            outer.push(Point::new(x, 70.0 + amp_mm * (TAU * x / wave_mm).sin()));
        }
        let r = region(&outer, &[]);
        let step_mm = resolution(&r).expect("a resolution").step_mm;
        let graph = medial_axis_with(&r, step_mm, TOL, EVERY);
        let case = format!("wave {amp_mm} × {wave_mm} mm every {every_mm} mm");
        for (i, n) in graph.nodes.iter().enumerate() {
            for &j in &graph.adjacency[i] {
                let other = graph.nodes[j];
                assert!(
                    n.clearance_mm > 0.0 || other.clearance_mm > 0.0,
                    "{case}: the tips at {:?} and {:?} are joined",
                    n.p,
                    other.p
                );
            }
        }
        for corner in convex_corners(&r, 0.0, TOL) {
            let gap_mm = graph
                .nodes
                .iter()
                .map(|n| n.p.distance_to(corner.p) - n.clearance_mm)
                .fold(f64::INFINITY, f64::min);
            assert!(
                gap_mm <= 0.01,
                "{case}: the corner at {:?} is {gap_mm:.4} mm from every medial disc",
                corner.p
            );
        }
    }
}

/// A thin wedge of material `length_mm` long, `half_deg` either side of
/// its axis, standing out of a wall at `base` toward `heading_deg` (a
/// drawn line's end): its foot's two points and its tip, as `[left, tip,
/// right]` looking along the heading.
fn wedge(base: Point, heading_deg: f64, length_mm: f64, half_deg: f64) -> [Point; 3] {
    let (sin, cos) = heading_deg.to_radians().sin_cos();
    let half_mm = length_mm * half_deg.to_radians().tan();
    let side = Vec2::new(-sin * half_mm, cos * half_mm);
    [
        base + side,
        base + Vec2::new(cos, sin) * length_mm,
        base + side * -1.0,
    ]
}

/// How far `p` lies outside every medial disc of `graph`, mm.
fn reach_gap_of(graph: &MedialGraph, p: Point) -> f64 {
    graph
        .nodes
        .iter()
        .map(|n| n.p.distance_to(p) - n.clearance_mm)
        .fold(f64::INFINITY, f64::min)
}

/// #465: with [`CornerReach::Every`] the axis wraps round a line's end. A
/// 12 mm square has a wedge 0.375 mm long, 6° either side, standing down
/// from its top edge at 73° (the line art's cell A). Round the wedge's tip
/// the axis is the curve equidistant from the tip and the top edge, both
/// seen in one direction once it is a little way out, and the
/// contact-spread filter dropped it from about 0.6 mm clearance on, so
/// the top edge beside the wedge stood up to 0.073 mm beyond every medial
/// disc. The top edge, sampled every 0.05 mm from 0.2 mm off the wedge's
/// feet to 0.5 mm off the square's corners, is now reached within 0.02 mm
/// by the nodes themselves (0.012 at worst; the carve's fitting puts its
/// cuts on the axis between them). Upstream's reach ([`medial_axis`]) is
/// unchanged.
#[test]
fn axis_wraps_round_a_line_end() {
    let [left, tip, right] = wedge(Point::new(5.0, 12.0), -73.0, 0.375, 6.0);
    let outer = vec![
        Point::new(0.0, 0.0),
        Point::new(12.0, 0.0),
        Point::new(12.0, 12.0),
        left,
        tip,
        right,
        Point::new(0.0, 12.0),
    ];
    let r = region(&outer, &[]);
    let step_mm = resolution(&r).expect("a resolution").step_mm;
    let graph = medial_axis_with(&r, step_mm, TOL, EVERY);
    let feet = left.x.min(right.x) - 0.2..=left.x.max(right.x) + 0.2;
    let mut worst = (f64::NEG_INFINITY, Point::new(0.0, 0.0));
    for k in 0..=220 {
        let x = 0.5 + 0.05 * f64::from(k);
        if x > 11.5 || feet.contains(&x) {
            continue;
        }
        let p = Point::new(x, 12.0);
        let gap_mm = reach_gap_of(&graph, p);
        if gap_mm > worst.0 {
            worst = (gap_mm, p);
        }
    }
    assert!(
        worst.0 <= 0.02,
        "the top edge at {:?} is {:.4} mm beyond every medial disc",
        worst.1,
        worst.0
    );
    assert_eq!(
        medial_axis(&r, step_mm, TOL),
        medial_axis_with(&r, step_mm, TOL, CornerReach::Upstream),
        "upstream's reach is upstream's"
    );
}

/// #465: the axis round a feature does not bring back the spokes curve
/// flattening leaves. The lowercase "g" at 10 and 30 mm (as
/// `lowercase_g_rejects_flattening_spokes_at_both_sizes`) with
/// [`CornerReach::Every`] keeps at most 3 leaves that are not corner tips:
/// a spoke would end in one. The glyph's curves turn one way between the
/// contacts of a spoke's nodes, by less than the spread, so the feature
/// rule keeps none of them.
#[test]
fn feature_contacts_keep_no_flattening_spokes() {
    for (size, case) in [10, 30].iter().zip(fixture(G)) {
        let shape = &case.regions[0].region;
        let step_mm = resolution(shape).expect("a resolution").step_mm;
        let graph = medial_axis_with(shape, step_mm, TOL, EVERY);
        let tips = graph.nodes.iter().filter(|n| n.clearance_mm < 1e-9).count();
        let leaves = leaf_count(&graph) - tips;
        assert!(
            leaves <= 3,
            "g at {size} mm kept {leaves} non-corner leaves"
        );
    }
}

// ---------------------------------------------------------------------------
// Bottlenecks (#475)
// ---------------------------------------------------------------------------

/// A `side_mm` square centred on the origin with a hole in it: a spike
/// 6 mm long, 0.6 mm wide at its foot and closed by a point 2 mm beyond
/// it, its tip `gap_mm` from the square's left wall and its axis
/// `tilt_deg` from the X axis (the inlay's `spike_project`).
fn spike_region(side_mm: f64, gap_mm: f64, tilt_deg: f64) -> (Region, Point) {
    let half = side_mm / 2.0;
    let (sin, cos) = tilt_deg.to_radians().sin_cos();
    let tip = Point::new(-half + gap_mm, 0.0);
    let along = Vec2::new(cos, sin);
    let foot = tip + along * 6.0;
    let across = Vec2::new(-sin, cos) * 0.3;
    let spike = vec![tip, foot + across * -1.0, foot + along * 2.0, foot + across];
    let square = rect_loop(-half, -half, side_mm, side_mm);
    (region(&square, &[spike]), tip)
}

/// #475: a 50 mm square's step, 0.3125 mm, is coarser than the 0.2 mm
/// between a spike's tip and the wall, and the axis round the tip is a
/// few nodes off it: the wall beside the tip stands 0.085 mm beyond every
/// medial disc. The graph has a bottleneck narrower than the step, so the
/// step is refined, to the bottleneck's clearance (no finer than a
/// quarter of the step), and there the nodes reach that wall within
/// 0.02 mm.
#[test]
fn refined_step_resolves_a_spike_beside_a_wall() {
    let (r, tip) = spike_region(50.0, 0.2, 60.0);
    let resolved = resolution(&r).expect("a resolution");
    assert!((resolved.step_mm - 0.3125).abs() < 1e-9);
    let coarse = medial_axis_with(&r, resolved.step_mm, TOL, EVERY);
    let narrowest_mm = bottleneck_mm(&coarse).expect("a bottleneck");
    assert!(
        narrowest_mm < resolved.step_mm,
        "the bottleneck {narrowest_mm:.4} mm is narrower than the step"
    );
    let refined_mm = refined_step_mm(&resolved, &coarse).expect("a finer step");
    assert!(
        refined_mm >= resolved.step_mm / MAX_FEATURE_REFINEMENT - 1e-12
            && refined_mm <= narrowest_mm.max(resolved.step_mm / MAX_FEATURE_REFINEMENT) + 1e-12,
        "refined to {refined_mm:.4} mm"
    );
    let wall = Point::new(tip.x - 0.2, tip.y);
    let coarse_gap_mm = reach_gap_of(&coarse, wall);
    assert!(
        coarse_gap_mm > 0.05,
        "at the shape's step the wall stands {coarse_gap_mm:.4} mm beyond every disc"
    );
    let fine = medial_axis_with(&r, refined_mm, TOL, EVERY);
    let fine_gap_mm = reach_gap_of(&fine, wall);
    println!(
        "step {:.4} mm: the wall stands {coarse_gap_mm:.4} mm beyond; bottleneck {narrowest_mm:.4} mm, step {refined_mm:.4} mm: {fine_gap_mm:.4} mm",
        resolved.step_mm
    );
    assert!(
        fine_gap_mm <= 0.02,
        "at {refined_mm:.4} mm the wall stands {fine_gap_mm:.4} mm beyond every disc"
    );
}

/// #475: a shape whose narrowest part is many steps wide keeps its step:
/// a rectangle's spine and a ring's cycle have no bottleneck narrower
/// than the step, and a 50 mm square with the spike's tip 5 mm from the
/// wall neither.
#[test]
fn refined_step_keeps_the_step_of_a_wide_shape() {
    let ring = region(
        &regular_polygon(0.0, 0.0, 20.0, 64),
        &[regular_polygon(0.0, 0.0, 10.0, 64)],
    );
    let (far, _) = spike_region(50.0, 5.0, 60.0);
    for (name, r) in [
        ("rectangle", region(&rect_loop(0.0, 0.0, 20.0, 10.0), &[])),
        ("ring", ring),
        ("far spike", far),
    ] {
        let resolved = resolution(&r).expect("a resolution");
        let graph = medial_axis_with(&r, resolved.step_mm, TOL, EVERY);
        assert_eq!(refined_step_mm(&resolved, &graph), None, "{name}");
    }
}

/// #475: however narrow the bottleneck, the step is refined no further
/// than a quarter of the shape step: the spike's tip 0.01 mm from the
/// wall gets 0.3125 / 4 mm. A budget-limited step is not refined.
#[test]
fn refined_step_is_bounded() {
    let (r, _) = spike_region(50.0, 0.01, 60.0);
    let resolved = resolution(&r).expect("a resolution");
    let graph = medial_axis_with(&r, resolved.step_mm, TOL, EVERY);
    let refined_mm = refined_step_mm(&resolved, &graph).expect("a finer step");
    assert!(
        (refined_mm - resolved.step_mm / MAX_FEATURE_REFINEMENT).abs() < 1e-12,
        "refined to {refined_mm:.4} mm"
    );
    let limited = Resolution {
        budget_floor_mm: resolved.step_mm,
        budget_limited: true,
        ..resolved
    };
    assert_eq!(refined_step_mm(&limited, &graph), None);
}
