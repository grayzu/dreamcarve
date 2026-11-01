// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/vcarveMedial.test.ts (the generator
//   and emission tests)

//! Tests of the V-Carve generator, medial strategy (#404;
//! `docs/specs/phase-3/design.md`, "Test plan → dc-cam — vcarve (medial)").
//!
//! The ports keep their upstream test's name in their comment. Upstream
//! carves with a 6 mm V-bit and no cone check; these use the bench's
//! 6.35 mm bits (60°: cone 5.499 mm; 90°: 3.175 mm), so a shape whose
//! deepest point is past the cone is given a flat depth or scaled down,
//! and the comment says so. Depths are below the start plane, the stock
//! top unless a test sets a start depth; upstream's Z 0 is the top.

mod clearing_passes;
mod corners;
mod floor;

use super::*;
use crate::entry::ENTRY_CLEARANCE_MM;
use crate::sequence::{SequenceInput, sequence};
use crate::tabs::LapRates;
use crate::test_support::assert_safe;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::vcarve::cone::Walls;
use crate::vcarve::medial::{Depth, Run, Tolerances};
use crate::warning::WarningCode;
use dc_geometry::medial::{MedialGraph, MedialNode};
use dc_geometry::{Point, Point3, Polyline, Transform, Vertex, assert_approx_eq};
use dc_project::test_support::{sample_pocket, sample_project, sample_v_carve, tool_v_bit_90};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, MachiningOrder, Operation, PocketOp, Stock,
    ToolId,
};
use std::collections::BTreeMap;

mod offset;

const THICKNESS_MM: f64 = 12.0;
/// Where every fixture's drawing is placed, mm.
const AT: (f64, f64) = (20.0, 20.0);
const FEED_MM_MIN: f64 = 1000.0;
const PLUNGE_MM_MIN: f64 = 300.0;

/// A 6.35 mm V-bit of `angle_deg`, id `t2`, every rate set.
fn v_bit(angle_deg: f64) -> Tool {
    let mut tool = tool_v_bit_90();
    tool.cutter = ToolKind::VBit {
        included_angle_deg: angle_deg,
    };
    tool.name = format!("{angle_deg}° V-bit");
    tool
}

fn slope_of(angle_deg: f64) -> f64 {
    (angle_deg.to_radians() / 2.0).tan()
}

/// A 200 × 150 × 12 mm project with one definition of `profiles` per
/// entry of `drawings`, each placed once at its offset from [`AT`], as
/// instances `i1`, `i2`, … The tools are the sample end mill `t1` and a
/// 60° V-bit `t2`; no operations.
fn project_of(drawings: Vec<(Vec<Polyline>, (f64, f64))>) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 200.0,
        height_mm: 150.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    p.definitions = BTreeMap::new();
    p.instances = Vec::new();
    for (k, (profiles, (dx, dy))) in drawings.into_iter().enumerate() {
        let def = FeatureDefinition {
            id: DefinitionId::new(format!("d{}", k + 1)),
            name: format!("Drawing {}", k + 1),
            profiles,
        };
        p.instances.push(FeatureInstance {
            id: InstanceId::new(format!("i{}", k + 1)),
            definition: def.id.clone(),
            transform: Transform::translate(AT.0 + dx, AT.1 + dy),
            name: format!("Shape {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
        p.definitions.insert(def.id.clone(), def);
    }
    p.tools.push(v_bit(60.0));
    p.operations.clear();
    p
}

/// [`project_of`] with one drawing at [`AT`].
fn project(profiles: Vec<Polyline>) -> Project {
    project_of(vec![(profiles, (0.0, 0.0))])
}

fn rect(w: f64, h: f64) -> Polyline {
    Polyline::rect(Point::new(0.0, 0.0), w, h)
}

fn polygon(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

/// The V-Carve of every instance with the V-bit `t2`, medial, no flat
/// depth, at the stock top.
fn carve_op(project: &Project) -> VCarveOp {
    VCarveOp {
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t2"),
        ..sample_v_carve()
    }
}

/// `project` with `op` as its last operation (replacing one of its id).
fn with_op(project: &Project, op: &VCarveOp) -> Project {
    let mut p = project.clone();
    p.operations.retain(|o| o.id() != &op.id);
    p.operations.push(Operation::VCarve(op.clone()));
    p
}

/// The generator's result for `op` in `project` (which gains `op`).
fn try_carve(project: &Project, op: &VCarveOp) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let p = with_op(project, op);
    let (runs, warnings) = generate_vcarve(&p, op)?;
    assert_eq!(runs.len(), 1, "one tool run without clearing: {runs:?}");
    let run = runs.into_iter().next().expect("one run");
    assert_eq!(run.label, RunLabel::Main);
    assert_eq!(run.tool, op.tool);
    Ok((run.toolpath, warnings))
}

/// [`try_carve`], which must succeed, checked with [`assert_safe`] (S1–S3
/// and, for a V-Carve, the cone test S17).
fn carve(project: &Project, op: &VCarveOp) -> (Toolpath, Vec<Warning>) {
    let (toolpath, warnings) = try_carve(project, op).expect("the carve generates");
    assert_safe(
        &toolpath,
        &with_op(project, op),
        &Operation::VCarve(op.clone()),
    );
    (toolpath, warnings)
}

/// The start plane of the plain fixtures: the stock top.
const TOP_MM: f64 = THICKNESS_MM;

/// Each straight cutting move with its start: `(from, to)`.
fn cuts(toolpath: &Toolpath) -> Vec<(Point3, Point3)> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        if let (Some(a), true) = (from, mv.is_cut()) {
            out.push((a, mv.to));
        }
        from = Some(mv.to);
    }
    out
}

/// The cuts that move in XY: chain cuts and links, not plunges.
fn xy_cuts(toolpath: &Toolpath) -> Vec<(Point3, Point3)> {
    cuts(toolpath)
        .into_iter()
        .filter(|(a, b)| a.xy().distance_to(b.xy()) > EPS_MM)
        .collect()
}

/// The straight plunges after a rapid: one per block entered.
fn plunges(toolpath: &Toolpath) -> Vec<(Point3, Move)> {
    let mut out = Vec::new();
    for pair in toolpath.moves.windows(2) {
        let [before, mv] = pair else { continue };
        if before.is_rapid()
            && mv.kind == MoveKind::Linear
            && mv.to.xy().distance_to(before.to.xy()) <= EPS_MM
            && mv.to.z < before.to.z - EPS_MM
        {
            out.push((before.to, *mv));
        }
    }
    out
}

/// Every point a cut ends at: the cut chain points, links and plunge ends.
fn cut_points(toolpath: &Toolpath) -> Vec<Point3> {
    cuts(toolpath).into_iter().map(|(_, b)| b).collect()
}

fn min_z(toolpath: &Toolpath) -> f64 {
    cut_points(toolpath)
        .iter()
        .map(|p| p.z)
        .fold(f64::INFINITY, f64::min)
}

/// The straight-line distance, mm, between `a` and `b`.
fn distance_3d(a: Point3, b: Point3) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
}

/// The exact distance, mm, from `p` to the walls of `regions` as drawn
/// (arcs as arcs): the test's own measure, not the generator's.
fn wall_distance(regions: &[Region], p: Point) -> f64 {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .flat_map(|c| c.polyline().segments().collect::<Vec<_>>())
        .map(|s| dc_geometry::point_segment_distance_mm(s, p))
        .fold(f64::INFINITY, f64::min)
}

/// The regions the generator carves for `op` in `project`.
fn regions_of(project: &Project, op: &VCarveOp) -> Vec<Region> {
    carve_regions(project, &op.id, &op.targets, &mut Vec::new())
        .expect("targets resolve")
        .regions
}

/// `(x, y)` offset by [`AT`].
fn at(x: f64, y: f64) -> Point {
    Point::new(AT.0 + x, AT.1 + y)
}

/// True when some cut point lies within `reach_mm` of `corner` in XY at
/// the surface (`top_mm`, within `depth_mm`).
fn surfaces_at(
    toolpath: &Toolpath,
    corner: Point,
    top_mm: f64,
    reach_mm: f64,
    depth_mm: f64,
) -> bool {
    cut_points(toolpath)
        .iter()
        .any(|p| p.xy().distance_to(corner) <= reach_mm && p.z >= top_mm - depth_mm)
}

// ---------------------------------------------------------------------------
// Emission on hand-built graphs (upstream `emitMedialToolpath` tests)
// ---------------------------------------------------------------------------

/// A bar `x0..x1` × `-half..half`, as the walls a hand-built graph lies in.
fn bar(x0: f64, x1: f64, half_mm: f64) -> Region {
    let contour = Contour::new(Polyline::rect(
        Point::new(x0, -half_mm),
        x1 - x0,
        2.0 * half_mm,
    ))
    .expect("a bar is a contour");
    let (mut regions, _) = nest(vec![contour]).expect("one region");
    regions.remove(0)
}

fn graph(nodes: &[(f64, f64, f64)], adjacency: &[&[usize]]) -> MedialGraph {
    MedialGraph {
        nodes: nodes
            .iter()
            .map(|&(x, y, c)| MedialNode {
                p: Point::new(x, y),
                clearance_mm: c,
            })
            .collect(),
        adjacency: adjacency.iter().map(|a| a.to_vec()).collect(),
    }
}

fn test_shape() -> ShapeRef {
    ShapeRef {
        instance: InstanceId::new("i1"),
        instance_name: "Shape 1".into(),
        profile: 0,
    }
}

/// The runs of `graph` lying in `region`, carved from a start plane at
/// Z 10 with `slope` and `flat_depth_mm`, no simplification or pruning.
fn runs_of(
    graph: &MedialGraph,
    region: &Region,
    slope: f64,
    flat_depth_mm: Option<f64>,
) -> Vec<Run> {
    let depth = Depth {
        start_plane_z_mm: 10.0,
        flat_depth_mm,
        slope,
    };
    let walls = Walls::new(std::slice::from_ref(region), 0.01);
    let no_simplifying = Tolerances {
        simplify_mm: 0.0,
        redundancy_mm: 0.0,
        deepen_mm: None,
    };
    medial::runs(graph, &depth, no_simplifying, &walls, &[])
}

/// [`runs_of`] as blocks, sequenced from safe Z 13 over a stock top at 10.
fn emit(graph: &MedialGraph, region: &Region, slope: f64, flat_depth_mm: Option<f64>) -> Toolpath {
    let rates = LapRates {
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
    };
    let blocks = runs_of(graph, region, slope, flat_depth_mm)
        .iter()
        .map(|run| medial::run_block(run, &test_shape(), rates, 13.0))
        .collect();
    sequence(&SequenceInput {
        blocks,
        order: MachiningOrder::FullDepthFirst,
        containment: &[],
        safe_z_mm: 13.0,
        clearance_z_mm: 13.0,
        stock_top_mm: 10.0,
        tool_diameter_mm: 6.35,
        stepover_mm: 0.0,
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
        cut_before: &[],
    })
    .expect("the blocks sequence")
}

/// Port of `testDotRegionCollapsesToSinglePlunge` (the emission half): a
/// 32-gon of radius 1 at step 0.35 is one node, carved as one plunge and
/// no cut.
#[test]
fn dot_region_is_one_plunge_and_no_cut() {
    let points: Vec<(f64, f64)> = (0..32)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 32.0;
            (5.0 + a.cos(), 5.0 + a.sin())
        })
        .collect();
    let contour = Contour::new(polygon(&points)).expect("a 32-gon");
    let (regions, _) = nest(vec![contour]).expect("one region");
    let dot = dc_geometry::medial::medial_axis(&regions[0], 0.35, 0.01);
    assert_eq!(dot.nodes.len(), 1, "a dot is one node: {dot:?}");
    let toolpath = emit(&dot, &regions[0], slope_of(60.0), Some(5.0));
    assert_eq!(plunges(&toolpath).len(), 1, "{toolpath:?}");
    assert!(xy_cuts(&toolpath).is_empty(), "{toolpath:?}");
}

/// Port of `testEmissionLinksNearbyChainEnds`, rewritten for security
/// review H2. Two chains, (0, 0)–(2, 0) and (2.2, 0.05)–(4, 0), every node
/// at clearance 1, slope 1, so the bit runs 1 mm deep. Upstream bridges
/// the 0.2 mm gap tool-down because it is within 0.9 of the 1 mm disc
/// carved at the surface. In the bar of half-width 1 that gives the nodes
/// their clearance, that link at 1 mm deep ends 0.95 mm from the wall
/// with a 1 mm cone: it would cut 0.05 mm below the final V surface, so
/// DreamCarve retracts there (two plunges, two cuts). In a bar of
/// half-width 1.5 the same link keeps its cone 0.45 mm inside the walls
/// all along: it runs within the carved disc and stays down (one plunge,
/// three cuts: upstream's result, for the reason H2 gives). Upstream's
/// second case, clearance 0.05, retracts here too: in a bar of half-width
/// 0.1 the link's end is exactly a cone's radius from the wall, with
/// nothing to spare.
#[test]
fn nearby_chain_ends_link_tool_down_within_the_carved_disc() {
    let nodes = [
        (0.0, 0.0, 1.0),
        (2.0, 0.0, 1.0),
        (2.2, 0.05, 1.0),
        (4.0, 0.0, 1.0),
    ];
    let adjacency: [&[usize]; 4] = [&[1], &[0], &[3], &[2]];
    let chains = graph(&nodes, &adjacency);

    let tight = emit(&chains, &bar(-1.0, 5.0, 1.0), 1.0, Some(5.0));
    assert_eq!(plunges(&tight).len(), 2, "H2 retracts: {tight:?}");
    assert_eq!(xy_cuts(&tight).len(), 2, "no link cut: {tight:?}");

    let roomy = emit(&chains, &bar(-1.0, 5.0, 1.5), 1.0, Some(5.0));
    assert_eq!(plunges(&roomy).len(), 1, "the link stays down: {roomy:?}");
    assert_eq!(
        xy_cuts(&roomy).len(),
        3,
        "two chain cuts and a link: {roomy:?}"
    );

    let shallow_nodes: Vec<(f64, f64, f64)> = nodes.iter().map(|&(x, y, _)| (x, y, 0.05)).collect();
    let shallow = graph(&shallow_nodes, &adjacency);
    let thin = emit(&shallow, &bar(-1.0, 5.0, 0.1), 1.0, Some(5.0));
    assert_eq!(
        plunges(&thin).len(),
        2,
        "retract at the cone's edge: {thin:?}"
    );
}

/// Port of `testLinkRespectsClampedDepth`, restated for security review
/// H2. Chains (0, 0)–(2, 0) and (2.8, 0)–(4, 0) at clearance 1 in a bar of
/// half-width 1, slope 1, clamped at a 0.1 mm flat depth. Upstream retracts
/// because the 0.8 mm link crosses floor its clamp leaves uncut; here the
/// cone test decides, at the depth the link actually runs: the clamped
/// 0.1 mm, whose 0.1 mm cone stays far inside the walls, so the link stays
/// down and runs at Z 10 − 0.1. The floor between the chains is the
/// carve's own (the V-bit floor of #405 clears it). Judged at the raw
/// clearance's depth (1 mm, its cone touching both walls) the same link
/// retracts, which the unclamped carve shows.
#[test]
fn links_use_the_clamped_disc() {
    let nodes = [
        (0.0, 0.0, 1.0),
        (2.0, 0.0, 1.0),
        (2.8, 0.0, 1.0),
        (4.0, 0.0, 1.0),
    ];
    let adjacency: [&[usize]; 4] = [&[1], &[0], &[3], &[2]];
    let chains = graph(&nodes, &adjacency);
    let walls = bar(-1.0, 5.0, 1.0);

    let clamped = emit(&chains, &walls, 1.0, Some(0.1));
    assert_eq!(plunges(&clamped).len(), 1, "{clamped:?}");
    let bridge = xy_cuts(&clamped)
        .into_iter()
        .find(|(a, b)| (a.x - 2.0).abs() < 1e-6 && (b.x - 2.8).abs() < 1e-6)
        .expect("a link from x 2 to x 2.8");
    assert_approx_eq!(bridge.0.z, 9.9, 1e-9);
    assert_approx_eq!(bridge.1.z, 9.9, 1e-9);

    let full = emit(&chains, &walls, 1.0, Some(5.0));
    assert_eq!(plunges(&full).len(), 2, "{full:?}");
    assert!(
        !xy_cuts(&full)
            .iter()
            .any(|(a, b)| (a.x - 2.0).abs() < 1e-6 && (b.x - 2.8).abs() < 1e-6),
        "a link bridged the gap at full depth: {full:?}"
    );
}

/// Step 10: the generator refuses a toolpath its own cone test fails. A
/// hand-built chain at clearance 1 along a 10 × 2 mm bar's centreline is
/// sound in that bar; handed to [`finish`] with a carve whose region is
/// the bar narrowed to 1.5 mm, its 1 mm cone (90° bit) crosses each wall
/// by 0.25 mm, and the result is [`CamError::ConeOffMaterial`].
#[test]
fn generator_refuses_its_own_toolpath_on_a_cone_violation() {
    let mut p = project(vec![rect(10.0, 2.0)]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = carve_op(&p);
    let p = with_op(&p, &op);
    let tool = p
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the V-bit")
        .clone();

    let chain = graph(
        &[(AT.0 + 1.0, AT.1 + 1.0, 1.0), (AT.0 + 9.0, AT.1 + 1.0, 1.0)],
        &[&[1], &[0]],
    );
    let narrow = {
        let contour = Contour::new(Polyline::rect(at(0.0, 0.25), 10.0, 1.5)).expect("a bar");
        nest(vec![contour]).expect("one region").0
    };
    let depth = Depth {
        start_plane_z_mm: TOP_MM,
        flat_depth_mm: None,
        slope: 1.0,
    };
    let rates = LapRates {
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
    };
    let walls = Walls::new(&narrow, 0.01);
    let no_simplifying = Tolerances {
        simplify_mm: 0.0,
        redundancy_mm: 0.0,
        deepen_mm: None,
    };
    let blocks: Vec<CutBlock> = medial::runs(&chain, &depth, no_simplifying, &walls, &[])
        .iter()
        .map(|run| medial::run_block(run, &test_shape(), rates, TOP_MM + 5.0))
        .collect();
    let carve = Carve {
        regions: narrow,
        shapes: vec![test_shape()],
        start_plane_z_mm: TOP_MM,
        engaged_from_z_mm: TOP_MM,
        flat_depth_mm: None,
        strategy: &VCarveStrategy::Medial,
        step_over_fraction: None,
        cleared_by: Vec::new(),
        open_edge: None,
        design: None,
    };
    let result = finish(
        &p,
        &op.id,
        &tool,
        &carve,
        blocks,
        &[],
        MachiningOrder::FullDepthFirst,
    );
    match result {
        Err(CamError::ConeOffMaterial { overlap_mm, .. }) => {
            assert_approx_eq!(overlap_mm, 0.25, 1e-6);
        }
        other => panic!("expected ConeOffMaterial, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The generator (upstream `generateVCarveMedialToolpath` tests)
// ---------------------------------------------------------------------------

/// Port of `testGeneratorSquare`: a 20 × 20 mm square, 60° bit, flat
/// depth 2: every cut inside the square, the deepest at −2 (the centre's
/// 10 mm clearance would be 17.3 mm deep), the corner tips at the
/// surface, no warnings.
#[test]
fn square_clamps_at_flat_depth_and_surfaces_at_corners() {
    let p = project(vec![rect(20.0, 20.0)]);
    let op = VCarveOp {
        flat_depth_mm: Some(2.0),
        ..carve_op(&p)
    };
    let (toolpath, warnings) = carve(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let points = cut_points(&toolpath);
    assert!(!xy_cuts(&toolpath).is_empty());
    for q in &points {
        let inside = (AT.0 - 1e-6..=AT.0 + 20.0 + 1e-6).contains(&q.x)
            && (AT.1 - 1e-6..=AT.1 + 20.0 + 1e-6).contains(&q.y);
        assert!(inside, "cut point {q:?} outside the square");
    }
    assert_approx_eq!(min_z(&toolpath), TOP_MM - 2.0, 1e-6);
    let max_z = points.iter().map(|q| q.z).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_z >= TOP_MM - 1e-6,
        "no corner tip at the surface: {max_z}"
    );
}

/// Port of `testGeneratorDepthMatchesVBitGeometry`: a 30 × 5 mm bar, half
/// width 2.5, is cut 2.5 / tan 30° = 4.330 mm deep with the 60° bit and
/// 2.5 / tan 45° = 2.5 mm with the 90° one.
#[test]
fn depth_follows_the_bit_angle() {
    for (angle_deg, expected_mm) in [(60.0, 2.5 / slope_of(60.0)), (90.0, 2.5)] {
        let mut p = project(vec![rect(30.0, 5.0)]);
        p.tools.retain(|t| t.id != ToolId::new("t2"));
        p.tools.push(v_bit(angle_deg));
        let (toolpath, _) = carve(&p, &carve_op(&p));
        assert_approx_eq!(TOP_MM - min_z(&toolpath), expected_mm, 0.02);
    }
}

/// Port of `testGeneratorRequiresVBit`: an end mill is refused
/// (`v_carve_needs_v_bit`), with no toolpath.
#[test]
fn end_mill_is_refused() {
    let p = project(vec![rect(20.0, 20.0)]);
    let op = VCarveOp {
        tool: ToolId::new("t1"),
        ..carve_op(&p)
    };
    match try_carve(&p, &op) {
        Err(CamError::VCarveNeedsVBit { tool_id, .. }) => assert_eq!(tool_id, ToolId::new("t1")),
        other => panic!("expected VCarveNeedsVBit, got {other:?}"),
    }
}

/// Port of `testGeneratorDeterminism`: the same input gives the same
/// toolpath, move for move.
#[test]
fn same_input_twice_is_identical() {
    let p = project(vec![rect(30.0, 5.0)]);
    let op = carve_op(&p);
    assert_eq!(carve(&p, &op).0, carve(&p, &op).0);
}

/// Port of `testGeneratorMultipleFeatures`: two 10 × 4 mm bars 10 mm
/// apart; no cut has both ends in the gap between them.
#[test]
fn two_bars_are_never_bridged() {
    let p = project_of(vec![
        (vec![rect(10.0, 4.0)], (0.0, 0.0)),
        (vec![rect(10.0, 4.0)], (20.0, 0.0)),
    ]);
    let (toolpath, _) = carve(&p, &carve_op(&p));
    let cuts = xy_cuts(&toolpath);
    assert!(!cuts.is_empty());
    let in_gap = |q: &Point3| q.x > AT.0 + 10.0 + 1e-6 && q.x < AT.0 + 20.0 - 1e-6;
    for (a, b) in &cuts {
        assert!(
            !(in_gap(a) && in_gap(b)),
            "cut crosses the gap: {a:?} -> {b:?}"
        );
    }
}

/// Port of `testGeneratorVisitsNearestResolvedRegionFirst`, restated for
/// DreamCarve's order (capability 38, as capability 11): shapes are
/// visited nearest-neighbour from the first in drawing order, where
/// upstream starts from the origin. Three 10 mm squares at x 0, 60 and
/// 20, drawn in that order, are cut 0, 20, 60: after the first, the
/// nearest, not the next drawn.
#[test]
fn nearest_region_is_cut_first() {
    let p = project_of(vec![
        (vec![rect(10.0, 10.0)], (0.0, 0.0)),
        (vec![rect(10.0, 10.0)], (60.0, 0.0)),
        (vec![rect(10.0, 10.0)], (20.0, 0.0)),
    ]);
    // Clamped at 2 mm: a 10 mm square's centre would be 8.66 mm deep,
    // past the 60° bit's cone.
    let op = VCarveOp {
        flat_depth_mm: Some(2.0),
        ..carve_op(&p)
    };
    let (toolpath, _) = carve(&p, &op);
    let mut visited: Vec<usize> = Vec::new();
    for (_, b) in xy_cuts(&toolpath) {
        let square = ((b.x - AT.0) / 20.0).floor();
        let square = if square >= 2.0 {
            2
        } else if square >= 1.0 {
            1
        } else {
            0
        };
        if visited.last() != Some(&square) {
            visited.push(square);
        }
    }
    assert_eq!(
        visited,
        vec![0, 1, 2],
        "squares at x 0, 20, 60 in that order"
    );
}

/// The fixture "Rag" (`tests/fixtures/vcarve/rag.json`, PureCutCNC's
/// outline text at 10 mm, Y-up): each region's outer then holes.
fn rag() -> Vec<Polyline> {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/vcarve/rag.json"))
            .expect("the fixture is JSON");
    let loop_of = |value: &serde_json::Value| -> Polyline {
        let points: Vec<(f64, f64)> = value
            .as_array()
            .expect("a loop")
            .iter()
            .map(|p| (p[0].as_f64().expect("x"), p[1].as_f64().expect("y")))
            .collect();
        polygon(&points)
    };
    let mut profiles = Vec::new();
    for region in json["cases"][0]["regions"].as_array().expect("regions") {
        profiles.push(loop_of(&region["outer"]));
        for hole in region["holes"].as_array().expect("holes") {
            profiles.push(loop_of(hole));
        }
    }
    profiles
}

/// [`rag`] on the stock: its glyphs lie at Y −14..0, so it is raised.
fn rag_project() -> Project {
    project_of(vec![(rag(), (0.0, 20.0))])
}

/// Port of `testGeneratorTextFeatureTarget`: the outline text "Rag" is
/// carved (and passes the cone test, which upstream does not have).
#[test]
fn outline_text_target_carves() {
    let p = rag_project();
    let (toolpath, _) = carve(&p, &carve_op(&p));
    assert!(!xy_cuts(&toolpath).is_empty());
}

/// Port of `testGeneratorTriangle`, scaled by 0.4 so the incentre's depth
/// (inradius 2.57 mm, 4.45 mm deep) is within the 60° bit's 5.5 mm cone
/// (upstream's 24 × 18 triangle would be 11.1 mm deep): the three arms
/// reach the surface at the three corners.
#[test]
fn triangle_arms_surface_at_three_corners() {
    let p = project(vec![triangle()]);
    let (toolpath, _) = carve(&p, &carve_op(&p));
    for (x, y) in TRIANGLE {
        assert!(
            surfaces_at(&toolpath, at(x, y), TOP_MM, 0.01, 1e-6),
            "no cut reaches the surface at corner ({x}, {y})"
        );
    }
}

/// Upstream's triangle (0, 0), (24, 0), (12, 18), scaled by 0.4.
const TRIANGLE: [(f64, f64); 3] = [(0.0, 0.0), (9.6, 0.0), (4.8, 7.2)];

fn triangle() -> Polyline {
    polygon(&TRIANGLE)
}

// ---------------------------------------------------------------------------
// Acceptance 48, 49, 52, 53
// ---------------------------------------------------------------------------

/// Acceptance 48: a 20 × 5 mm rectangle with the 60° 6.35 mm bit is cut
/// 2.5 / tan 30° = 4.330 mm deep along its centreline (within 0.01);
/// every cutting point's depth is its distance to the nearest wall over
/// tan 30° (within 0.02); and each corner is reached at the surface
/// (within 0.01).
#[test]
fn rectangle_centreline_depth_is_half_width_over_tan() {
    let p = project(vec![rect(20.0, 5.0)]);
    let op = carve_op(&p);
    let (toolpath, _) = carve(&p, &op);
    let regions = regions_of(&p, &op);
    let slope = slope_of(60.0);
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 2.5 / slope, 0.01);
    for q in cut_points(&toolpath) {
        let expected_mm = wall_distance(&regions, q.xy()) / slope;
        assert!(
            (TOP_MM - q.z - expected_mm).abs() <= 0.02,
            "cut point {q:?} is {} deep, its walls say {expected_mm}",
            TOP_MM - q.z
        );
    }
    for (x, y) in [(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (0.0, 5.0)] {
        assert!(
            surfaces_at(&toolpath, at(x, y), TOP_MM, 0.01, 0.01),
            "corner ({x}, {y}) is not reached at the surface"
        );
    }
}

/// Acceptance 48: a 6 mm square with the 90° bit carves four diagonals
/// meeting at the centre 3 mm deep, each from a corner at the surface.
#[test]
fn six_mm_square_with_90_degree_bit_meets_at_three_mm() {
    let mut p = project(vec![rect(6.0, 6.0)]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let (toolpath, _) = carve(&p, &carve_op(&p));
    let deepest = cut_points(&toolpath)
        .into_iter()
        .min_by(|a, b| a.z.total_cmp(&b.z))
        .expect("cuts");
    assert_approx_eq!(TOP_MM - deepest.z, 3.0, 0.01);
    assert!(
        deepest.xy().distance_to(at(3.0, 3.0)) <= 0.05,
        "{deepest:?}"
    );
    for (x, y) in [(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)] {
        assert!(
            surfaces_at(&toolpath, at(x, y), TOP_MM, 0.01, 0.01),
            "corner ({x}, {y})"
        );
    }
}

/// Acceptance 48: a 6 mm circle with the 60° bit collapses to one plunge
/// at its centre, 3 / tan 30° = 5.196 mm deep. The circle is drawn as
/// arcs and flattened within the project's 0.01 mm, which can lower the
/// clearance by up to that much (0.017 mm of depth), hence 0.02.
#[test]
fn six_mm_circle_is_one_plunge_at_5_196() {
    let p = project(vec![Polyline::circle(Point::new(3.0, 3.0), 3.0)]);
    let (toolpath, _) = carve(&p, &carve_op(&p));
    let plunged = plunges(&toolpath);
    assert_eq!(plunged.len(), 1, "{toolpath:?}");
    assert!(xy_cuts(&toolpath).is_empty(), "{toolpath:?}");
    let (_, plunge) = plunged[0];
    assert_approx_eq!(TOP_MM - plunge.to.z, 5.196, 0.02);
    assert!(
        plunge.to.xy().distance_to(at(3.0, 3.0)) <= 0.05,
        "{plunge:?}"
    );
}

/// Acceptance 49: a 24 mm square with a 12 mm square hole, 90° bit. The
/// ring's corridors are 6 mm wide, so the carve runs along their
/// centreline 3 mm from each wall and 3 mm deep. Where a corridor turns,
/// the disc touching both outer walls and the hole's corner is 6(2 − √2)
/// = 3.51 mm, which would be 3.51 mm deep, past the 90° bit's 3.175 mm
/// cone, so the ring carries a 3 mm flat depth (recorded on #404): the
/// corridors are exactly that deep. The carve rises to the surface at the
/// four outer corners, and nowhere at the hole's corners.
#[test]
fn ring_runs_along_its_centreline_and_rises_at_outer_corners() {
    let hole = Polyline::rect(Point::new(6.0, 6.0), 12.0, 12.0);
    let mut p = project(vec![rect(24.0, 24.0), hole]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = VCarveOp {
        flat_depth_mm: Some(3.0),
        ..carve_op(&p)
    };
    let (toolpath, _) = carve(&p, &op);
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 3.0, 1e-6);
    // Each corridor's middle is cut 3 mm deep on its centreline.
    for (x, y) in [(12.0, 3.0), (21.0, 12.0), (12.0, 21.0), (3.0, 12.0)] {
        let middle = at(x, y);
        let on_line = xy_cuts(&toolpath).iter().any(|(a, b)| {
            let (pa, pb) = (a.xy(), b.xy());
            let ab = pb - pa;
            let t = ((middle - pa).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
            let z = a.z + (b.z - a.z) * t;
            (pa + ab * t).distance_to(middle) <= 0.01 && (TOP_MM - z - 3.0).abs() <= 0.01
        });
        assert!(on_line, "corridor middle ({x}, {y}) not cut 3 mm deep");
    }
    for (x, y) in [(0.0, 0.0), (24.0, 0.0), (24.0, 24.0), (0.0, 24.0)] {
        assert!(
            surfaces_at(&toolpath, at(x, y), TOP_MM, 0.01, 0.01),
            "outer corner ({x}, {y})"
        );
    }
    for (x, y) in [(6.0, 6.0), (18.0, 6.0), (18.0, 18.0), (6.0, 18.0)] {
        assert!(
            !surfaces_at(&toolpath, at(x, y), TOP_MM, 0.5, 0.5),
            "a tip at the hole's corner ({x}, {y})"
        );
    }
}

/// The fixtures every cone property below runs on: the rectangle, the
/// 6 mm square and circle, the ring (with its flat depth), the triangle,
/// an L, the two bars, a clamped square and "Rag" (also clamped at
/// 0.5 mm, where its chain ends have room to link), each with its V-bit
/// angle.
fn fixtures() -> Vec<(&'static str, Project, VCarveOp)> {
    let with_bit = |mut p: Project, angle_deg: f64| {
        p.tools.retain(|t| t.id != ToolId::new("t2"));
        p.tools.push(v_bit(angle_deg));
        p
    };
    let mut out = Vec::new();
    let mut add = |name: &'static str, p: Project, flat_depth_mm: Option<f64>| {
        let op = VCarveOp {
            flat_depth_mm,
            ..carve_op(&p)
        };
        out.push((name, p, op));
    };
    add("rectangle", project(vec![rect(20.0, 5.0)]), None);
    add(
        "square",
        with_bit(project(vec![rect(6.0, 6.0)]), 90.0),
        None,
    );
    add(
        "circle",
        project(vec![Polyline::circle(Point::new(3.0, 3.0), 3.0)]),
        None,
    );
    add(
        "ring",
        with_bit(
            project(vec![
                rect(24.0, 24.0),
                Polyline::rect(Point::new(6.0, 6.0), 12.0, 12.0),
            ]),
            90.0,
        ),
        Some(3.0),
    );
    add("triangle", project(vec![triangle()]), None);
    add(
        "L",
        project(vec![polygon(&[
            (0.0, 0.0),
            (12.0, 0.0),
            (12.0, 4.0),
            (4.0, 4.0),
            (4.0, 12.0),
            (0.0, 12.0),
        ])]),
        None,
    );
    add(
        "bars",
        project_of(vec![
            (vec![rect(10.0, 4.0)], (0.0, 0.0)),
            (vec![rect(10.0, 4.0)], (20.0, 0.0)),
        ]),
        None,
    );
    add("square, flat 2", project(vec![rect(20.0, 20.0)]), Some(2.0));
    add("Rag", rag_project(), None);
    add("Rag, flat 0.5", rag_project(), Some(0.5));
    out
}

/// Acceptance 49, S17 on every fixture, measured by the test itself: at
/// points along every cut, 64 or every 0.05 mm, whichever are fewer (the
/// floor's short ring moves, #405), the distance to the nearest wall as
/// drawn is at least the depth there times tan(α/2), less the 0.01 mm the
/// outlines are flattened to.
#[test]
fn no_cut_is_nearer_a_wall_than_its_depth_allows() {
    for (name, p, op) in fixtures() {
        let (toolpath, _) = carve(&p, &op);
        let regions = regions_of(&p, &op);
        let tool = p.tools.iter().find(|t| t.id == op.tool).expect("the bit");
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            panic!("a V-bit");
        };
        let slope = slope_of(included_angle_deg);
        for (a, b) in cuts(&toolpath) {
            let length_mm = distance_3d(a, b);
            let samples = (1..=64)
                .find(|&n| length_mm / f64::from(n) <= 0.05)
                .unwrap_or(64);
            for k in 0..=samples {
                let t = f64::from(k) / f64::from(samples);
                let q = Point3::new(
                    a.x + (b.x - a.x) * t,
                    a.y + (b.y - a.y) * t,
                    a.z + (b.z - a.z) * t,
                );
                let depth_mm = TOP_MM - q.z;
                if depth_mm < 0.0 {
                    continue;
                }
                let room_mm = wall_distance(&regions, q.xy()) - depth_mm * slope;
                assert!(
                    room_mm >= -0.01 - 1e-9,
                    "{name}: the cone at {q:?} reaches {} mm past a wall",
                    -room_mm
                );
            }
        }
    }
}

/// Acceptance 49: the triangle's carve meets at its incentre, inradius /
/// tan 30° deep (the 0.4-scaled triangle: inradius 2.569 mm).
#[test]
fn triangle_meets_at_the_incentre() {
    let p = project(vec![triangle()]);
    let (toolpath, _) = carve(&p, &carve_op(&p));
    let [a, b, c] = TRIANGLE.map(|(x, y)| Point::new(x, y));
    let (la, lb, lc) = (b.distance_to(c), c.distance_to(a), a.distance_to(b));
    let perimeter = la + lb + lc;
    let incentre = Point::new(
        (la * a.x + lb * b.x + lc * c.x) / perimeter,
        (la * a.y + lb * b.y + lc * c.y) / perimeter,
    );
    let area = ((b - a).cross(c - a) / 2.0).abs();
    let inradius = area / (perimeter / 2.0);
    let deepest = cut_points(&toolpath)
        .into_iter()
        .min_by(|p, q| p.z.total_cmp(&q.z))
        .expect("cuts");
    assert!(
        deepest.xy().distance_to(at(incentre.x, incentre.y)) <= 0.05,
        "deepest {deepest:?}, incentre {incentre:?}"
    );
    assert_approx_eq!(TOP_MM - deepest.z, inradius / slope_of(60.0), 0.02);
}

/// Acceptance 52: an open polyline among the targets is skipped with
/// `vcarve_target_open`, and the closed targets are carved.
#[test]
fn open_path_is_skipped_with_warning() {
    let open = Polyline::new(
        vec![Vertex::line(30.0, 0.0), Vertex::line(40.0, 5.0)],
        false,
    );
    let p = project(vec![rect(20.0, 5.0), open]);
    let (toolpath, warnings) = carve(&p, &carve_op(&p));
    let codes: Vec<WarningCode> = warnings.iter().map(Warning::code).collect();
    assert_eq!(codes, vec![WarningCode::VCarveTargetOpen], "{warnings:?}");
    assert!(matches!(
        &warnings[0],
        Warning::VCarveTargetOpen { shape, .. } if shape.profile == 1
    ));
    assert!(!xy_cuts(&toolpath).is_empty());
}

/// Acceptance 52: a 20 mm square with the 90° bit and no flat depth would
/// be cut 10 mm deep at its centre, past the 3.175 mm cone:
/// `v_bit_cone_exceeded`, naming the shape and 10 mm.
#[test]
fn cone_exceeded_is_an_error_naming_the_depth() {
    let mut p = project(vec![rect(20.0, 20.0)]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    match try_carve(&p, &carve_op(&p)) {
        Err(CamError::VBitConeExceeded {
            shape,
            depth_mm,
            cone_mm,
            ..
        }) => {
            assert_eq!(shape.instance, InstanceId::new("i1"));
            assert_eq!(shape.profile, 0);
            assert_approx_eq!(depth_mm, 10.0, 0.05);
            assert_approx_eq!(cone_mm, 3.175, 1e-9);
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }
}

/// Acceptance 52: the same square with `flat_depth_mm: 3`, within the
/// cone, exports, clamped at 3 mm.
#[test]
fn flat_depth_within_the_cone_exports() {
    let mut p = project(vec![rect(20.0, 20.0)]);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = VCarveOp {
        flat_depth_mm: Some(3.0),
        ..carve_op(&p)
    };
    let (toolpath, _) = carve(&p, &op);
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 3.0, 1e-6);
}

/// The 20 × 5 mm rectangle with a 60 × 30 mm Pocket around it (instance
/// `i2`, operation `o2`, `pocket_depth_mm` deep) listed first.
fn rectangle_in_a_pocket(pocket_depth_mm: f64) -> (Project, PocketOp) {
    let p = project_of(vec![
        (vec![rect(20.0, 5.0)], (0.0, 0.0)),
        (vec![rect(60.0, 30.0)], (-20.0, -12.5)),
    ]);
    let pocket = PocketOp {
        targets: vec![InstanceId::new("i2")],
        depth_mm: Some(pocket_depth_mm),
        ..sample_pocket()
    };
    let mut p = p;
    p.operations.push(Operation::Pocket(pocket.clone()));
    (p, pocket)
}

/// The V-Carve of the rectangle alone, `i1`.
fn rectangle_carve(p: &Project) -> VCarveOp {
    VCarveOp {
        targets: vec![InstanceId::new("i1")],
        ..carve_op(p)
    }
}

/// Acceptance 53 (the generator's half): the rectangle with
/// `start_depth_mm: 4` inside a 4 mm Pocket that runs before it is cut
/// exactly 4 mm lower than at the stock top, move for move; and its first
/// cut is reached by a rapid down to the entry clearance above the stock
/// top (not the start plane: S2 unchanged), then a plunge at the plunge
/// rate.
#[test]
fn start_depth_lowers_every_cut_and_plunges_from_the_stock_top() {
    let (p, _) = rectangle_in_a_pocket(4.0);
    let at_top = rectangle_carve(&p);
    let lowered = VCarveOp {
        start_depth_mm: 4.0,
        ..rectangle_carve(&p)
    };
    let (top_path, _) = carve(&p, &at_top);
    let (low_path, _) = carve(&p, &lowered);
    let (top_cuts, low_cuts) = (cuts(&top_path), cuts(&low_path));
    assert_eq!(top_cuts.len(), low_cuts.len());
    for ((a, b), (c, d)) in top_cuts.iter().zip(&low_cuts) {
        assert!(a.xy().distance_to(c.xy()) <= 1e-9 && b.xy().distance_to(d.xy()) <= 1e-9);
        assert_approx_eq!(b.z - d.z, 4.0, 1e-9);
    }
    let (from, plunge) = plunges(&low_path)[0];
    assert_approx_eq!(from.z, THICKNESS_MM + ENTRY_CLEARANCE_MM, 1e-9);
    assert_eq!(plunge.feed_mm_min, Some(PLUNGE_MM_MIN));
    assert_approx_eq!(plunge.to.z, TOP_MM - 4.0, 0.01);
}

/// Capability 42: one pass whatever the depth. The tool's step-down,
/// which a Pocket would follow, is ignored: the rectangle carved to full
/// depth (4.33 mm) or clamped at 1 or 3 mm is cut move for move the same
/// with a 0.5 mm step-down as with none, and the medial carve is entered
/// as often at any depth. (Restated in #405: with a flat depth the floor's
/// rings are entered too, so the plunge count is no longer equal across
/// depths.)
#[test]
fn one_pass_whatever_the_depth() {
    let p = project(vec![rect(20.0, 5.0)]);
    let mut stepped = p.clone();
    for tool in &mut stepped.tools {
        tool.step_down_mm = Some(0.5);
    }
    let mut unstepped = p.clone();
    for tool in &mut unstepped.tools {
        tool.step_down_mm = None;
    }
    for flat_depth_mm in [None, Some(1.0), Some(3.0)] {
        let op = VCarveOp {
            flat_depth_mm,
            ..carve_op(&p)
        };
        assert_eq!(
            carve(&stepped, &op).0,
            carve(&unstepped, &op).0,
            "flat depth {flat_depth_mm:?}"
        );
    }
    let medial_entries: Vec<usize> = [None, Some(1.0), Some(3.0)]
        .into_iter()
        .map(|flat_depth_mm| {
            let op = VCarveOp {
                flat_depth_mm,
                ..carve_op(&p)
            };
            plunges(&carve(&stepped, &op).0).len()
        })
        .collect();
    assert!(medial_entries.iter().all(|&n| n < 9), "{medial_entries:?}");
}

/// Steps 5 and 9: the blocks are runs of chains, each at pass 0 with its
/// entry at its first point's Z, its pass Z its deepest, no descent of
/// its own and nothing cleared; `sequence` cuts one region's blocks in
/// the generator's order, each after a plunge to its entry.
#[test]
fn chains_become_blocks_that_sequence_in_generator_order() {
    let p = project(vec![rect(20.0, 5.0)]);
    let op = carve_op(&p);
    let p = with_op(&p, &op);
    let tool = p
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the bit")
        .clone();
    let targets = carve_regions(&p, &op.id, &op.targets, &mut Vec::new()).expect("targets");
    let rectangle = Carve {
        regions: targets.regions,
        shapes: targets.shapes,
        start_plane_z_mm: TOP_MM,
        engaged_from_z_mm: TOP_MM,
        flat_depth_mm: None,
        strategy: &VCarveStrategy::Medial,
        step_over_fraction: None,
        cleared_by: Vec::new(),
        open_edge: None,
        design: None,
    };
    let blocks = blocks(&p, &op.id, &tool, &rectangle, &mut Vec::new()).expect("blocks");
    assert!(!blocks.is_empty());
    for block in &blocks {
        assert_eq!(block.pass, 0);
        assert_eq!(block.shape.instance, InstanceId::new("i1"));
        assert!(block.descent.is_empty() && block.cleared.is_none() && !block.frees);
        assert_eq!(block.descent_from, block.entry);
        let deepest = block
            .cuts
            .iter()
            .map(|m| m.to.z)
            .fold(block.entry_z_mm, f64::min);
        assert_approx_eq!(block.z_mm, deepest, 1e-12);
    }
    let (toolpath, _) = carve(&p, &op);
    let plunged = plunges(&toolpath);
    assert_eq!(plunged.len(), blocks.len());
    for ((_, plunge), block) in plunged.iter().zip(&blocks) {
        assert!(plunge.to.xy().distance_to(block.entry) <= EPS_MM);
        assert_approx_eq!(plunge.to.z, block.entry_z_mm, 1e-12);
    }
    let written: Vec<Point3> = xy_cuts(&toolpath).into_iter().map(|(_, b)| b).collect();
    let generated_xy: Vec<Point3> = {
        let mut out = Vec::new();
        for block in &blocks {
            let mut from = Point3::new(block.entry.x, block.entry.y, block.entry_z_mm);
            for m in &block.cuts {
                if from.xy().distance_to(m.to.xy()) > EPS_MM {
                    out.push(m.to);
                }
                from = m.to;
            }
        }
        out
    };
    assert_eq!(written, generated_xy);
}

/// Capability 42 with `machining_order`: one pass, so full depth first
/// and pass first cut the same, on the three regions of "Rag" and on the
/// two bars.
#[test]
fn both_machining_orders_cut_the_same() {
    for p in [
        rag_project(),
        project_of(vec![
            (vec![rect(10.0, 4.0)], (0.0, 0.0)),
            (vec![rect(10.0, 4.0)], (20.0, 0.0)),
        ]),
    ] {
        let full = VCarveOp {
            machining_order: MachiningOrder::FullDepthFirst,
            ..carve_op(&p)
        };
        let pass = VCarveOp {
            machining_order: MachiningOrder::PassFirst,
            ..carve_op(&p)
        };
        assert_eq!(carve(&p, &full).0, carve(&p, &pass).0);
    }
}

// ---------------------------------------------------------------------------
// Security review
// ---------------------------------------------------------------------------

/// Security review H1: the 20 × 5 mm rectangle's first plunge ends at a
/// corner at the start plane, where its chain starts, not at the
/// centreline's 4.33 mm.
#[test]
fn corner_tip_chain_plunges_to_the_surface_not_the_centreline() {
    let p = project(vec![rect(20.0, 5.0)]);
    let (toolpath, _) = carve(&p, &carve_op(&p));
    let (_, first) = plunges(&toolpath)[0];
    assert_approx_eq!(first.to.z, TOP_MM, 0.01);
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (0.0, 5.0)];
    assert!(
        corners
            .iter()
            .any(|&(x, y)| first.to.xy().distance_to(at(x, y)) <= 0.01),
        "first plunge at {first:?}"
    );
}

/// Security review H2: every stay-down link of every fixture region keeps
/// the cone inside the walls all along it: at 64 points, the clearance is
/// at least the interpolated depth times tan(α/2). Some fixture links, so
/// the property is not empty. The hand-built graph of
/// [`nearby_chain_ends_link_tool_down_within_the_carved_disc`] at
/// clearance 1 retracts where upstream bridged (upstream's 0.9-of-the-disc
/// rule let that link cut 0.05 mm below the V surface).
#[test]
fn links_never_run_below_the_final_v_surface() {
    let mut links = 0;
    for (name, p, op) in fixtures() {
        let tool = p.tools.iter().find(|t| t.id == op.tool).expect("the bit");
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            panic!("a V-bit");
        };
        let slope = slope_of(included_angle_deg);
        let targets = carve_regions(&p, &op.id, &op.targets, &mut Vec::new()).expect("targets");
        for (region, shape) in targets.regions.iter().zip(&targets.shapes) {
            let Some((graph, step_mm)) = medial::region_graph(
                region,
                p.flatten_tolerance_mm,
                CornerReach::Upstream,
                &op.id,
                shape,
                &mut false,
                &mut Vec::new(),
            ) else {
                continue;
            };
            let depth = Depth {
                start_plane_z_mm: TOP_MM,
                flat_depth_mm: op.flat_depth_mm,
                slope,
            };
            let walls = Walls::new(std::slice::from_ref(region), p.flatten_tolerance_mm);
            for run in medial::runs(&graph, &depth, Tolerances::at_step(step_mm), &walls, &[]) {
                let mut from = run.start;
                for step in &run.steps {
                    if step.link {
                        links += 1;
                        for k in 0..=64 {
                            let t = f64::from(k) / 64.0;
                            let q = Point3::new(
                                from.x + (step.to.x - from.x) * t,
                                from.y + (step.to.y - from.y) * t,
                                from.z + (step.to.z - from.z) * t,
                            );
                            let room_mm = wall_distance(std::slice::from_ref(region), q.xy())
                                - (TOP_MM - q.z) * slope;
                            assert!(
                                room_mm >= -0.01,
                                "{name}: link {from:?} -> {:?} runs {} mm below the V surface at {q:?}",
                                step.to,
                                -room_mm
                            );
                        }
                    }
                    from = step.to;
                }
            }
        }
    }
    assert!(links > 0, "no fixture links tool-down");

    let nodes = [
        (0.0, 0.0, 1.0),
        (2.0, 0.0, 1.0),
        (2.2, 0.05, 1.0),
        (4.0, 0.0, 1.0),
    ];
    let chains = graph(&nodes, &[&[1], &[0], &[3], &[2]]);
    let upstream_bridge = runs_of(&chains, &bar(-1.0, 5.0, 1.0), 1.0, Some(5.0));
    assert!(
        upstream_bridge
            .iter()
            .all(|r| r.steps.iter().all(|s| !s.link)),
        "{upstream_bridge:?}"
    );
}

/// Security review H4 and M2: `start_depth_mm: 4` over stock no Pocket
/// clears is refused (`start_depth_uncleared`); vouched for with
/// `start_plane_cleared_externally`, the flank engages from the stock
/// top, 4 + 1 = 5 mm, past the 90° bit's 3.175 mm cone, though the carve
/// itself is clamped at 1 mm (`v_bit_cone_exceeded`); inside a 4 mm
/// Pocket that runs first it engages 1 mm only and exports.
#[test]
fn uncleared_start_depth_is_refused_and_acknowledged_counts_toward_the_cone() {
    let (mut p, _) = rectangle_in_a_pocket(4.0);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = VCarveOp {
        start_depth_mm: 4.0,
        flat_depth_mm: Some(1.0),
        ..rectangle_carve(&p)
    };

    let mut no_pocket = p.clone();
    no_pocket
        .operations
        .retain(|o| !matches!(o, Operation::Pocket(_)));
    match try_carve(&no_pocket, &op) {
        Err(CamError::StartDepthUncleared { start_depth_mm, .. }) => {
            assert_approx_eq!(start_depth_mm, 4.0, 1e-12);
        }
        other => panic!("expected StartDepthUncleared, got {other:?}"),
    }

    let vouched = VCarveOp {
        start_plane_cleared_externally: true,
        ..op.clone()
    };
    match try_carve(&no_pocket, &vouched) {
        Err(CamError::VBitConeExceeded {
            depth_mm, cone_mm, ..
        }) => {
            assert_approx_eq!(depth_mm, 5.0, 1e-9);
            assert_approx_eq!(cone_mm, 3.175, 1e-9);
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }

    let (toolpath, _) = carve(&p, &op);
    assert_approx_eq!(TOP_MM - min_z(&toolpath), 5.0, 1e-6);
}
