// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the Inlay generator (#436; `docs/specs/phase-3/design.md`,
//! "Algorithms → Inlay" and "Test plan → dc-cam — inlay"; functional
//! acceptance 56 and 57).
//!
//! The star is the inlay test part's: four points 60 mm tip to tip, inner
//! vertices 12 mm out on the diagonals, a 12 mm circle in its middle,
//! centred at (50, 50) on 100 × 100 × 12 mm stock. `t1` is the bench's
//! 60° 6.35 mm V-bit (cone 5.499 mm), `t2` a 3.175 mm end mill. The values
//! are the part's: `depth_mm` 4, `glue_gap_mm` 1, `plug_depth_mm` 2,
//! `margin_mm` 8, so the male's start plane is 3 mm below the stock top
//! and its floor 5 mm below it.

mod line_art;
mod seat;
mod spike;
mod traced;

use super::*;
use crate::heightfield::{Cutter, Heightfield};
use crate::run::RunLabel;
use crate::test_support::assert_run_safe;
use crate::toolpath::{MoveKind, Toolpath};
use crate::vcarve::generate_vcarve;
use dc_geometry::{
    Contour, Point, Point3, Polyline, Region, Vertex, floor_regions, nearest_point, nest, offset,
    rounded_band, surround, surround_floor_regions,
};
use dc_project::test_support::{sample_project, tool_6_35_upcut};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InlaySide, MachiningOrder, Operation, OperationId, Stock, Tool, ToolId,
    ToolKind, VCarveOp, VCarveStrategy,
};
use seat::{SeatCell, seat};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const THICKNESS_MM: f64 = 12.0;
/// The star's centre, mm.
const CENTRE: (f64, f64) = (50.0, 50.0);
const DEPTH_MM: f64 = 4.0;
const GLUE_GAP_MM: f64 = 1.0;
const PLUG_DEPTH_MM: f64 = 2.0;
const MARGIN_MM: f64 = 8.0;
/// The circle's radius, mm.
const CIRCLE_MM: f64 = 6.0;
/// The fit property's and the flank's tolerance (acceptance 57), mm.
const FIT_MM: f64 = 0.02;

/// A 6.35 mm V-bit `t1` of `angle_deg`: 1000 mm/min, plunge 300, step-down
/// 2 mm, floor step-over 0.4.
fn v_bit(angle_deg: f64) -> Tool {
    Tool {
        id: ToolId::new("t1"),
        name: format!("{angle_deg}° V-bit"),
        cutter: ToolKind::VBit {
            included_angle_deg: angle_deg,
        },
        ..tool_6_35_upcut()
    }
}

/// The 3.175 mm end mill `t2`: 1000 mm/min, plunge 300, step-down 2 mm,
/// step-over 0.4.
fn end_mill() -> Tool {
    Tool {
        id: ToolId::new("t2"),
        name: "3.175 mm end mill".into(),
        diameter_mm: 3.175,
        ..tool_6_35_upcut()
    }
}

fn slope() -> f64 {
    (30.0_f64).to_radians().tan()
}

fn polygon(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

/// The star's corners, centred at the origin: tips on the axes 30 mm out,
/// inner vertices on the diagonals 12 mm out.
fn star_points() -> Vec<(f64, f64)> {
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    vec![
        (30.0, 0.0),
        (inner, inner),
        (0.0, 30.0),
        (-inner, inner),
        (-30.0, 0.0),
        (-inner, -inner),
        (0.0, -30.0),
        (inner, -inner),
    ]
}

/// A `width` × `height` × [`THICKNESS_MM`] project with one instance `i1`
/// of `profiles` placed at `at`, the tools `t1` (a 60° V-bit) and `t2`,
/// and no operations.
fn project_of(profiles: Vec<Polyline>, at: (f64, f64), width_mm: f64, height_mm: f64) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm,
        height_mm,
        thickness_mm: Some(THICKNESS_MM),
    };
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Design".into(),
        profiles,
    };
    p.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: def.id.clone(),
        transform: Transform::translate(at.0, at.1),
        name: "Design".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    p.definitions = BTreeMap::from([(def.id.clone(), def)]);
    p.tools = vec![v_bit(60.0), end_mill()];
    p.operations.clear();
    p
}

/// The star with its circle on 100 × 100 mm stock `thickness_mm` thick.
fn star_project(thickness_mm: f64) -> Project {
    let mut p = project_of(
        vec![
            polygon(&star_points()),
            Polyline::circle(Point::new(0.0, 0.0), CIRCLE_MM),
        ],
        CENTRE,
        100.0,
        100.0,
    );
    p.stock.thickness_mm = Some(thickness_mm);
    p
}

/// The star part's Inlay `o1` of `i1` with `t1`, side `side`, no clearing.
fn inlay(side: InlaySide) -> InlayOp {
    InlayOp {
        id: OperationId::new("o1"),
        name: "Star".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(DEPTH_MM),
        glue_gap_mm: Some(GLUE_GAP_MM),
        plug_depth_mm: Some(PLUG_DEPTH_MM),
        margin_mm: Some(MARGIN_MM),
        outline_rounding_mm: None,
        clearing: Vec::new(),
        step_over_fraction: None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 10.0,
        cut_out: None,
    }
}

/// A clearing pass with `t2`, offset, climb, plunging.
fn clearing_pass() -> ClearingPass {
    ClearingPass {
        tool: ToolId::new("t2"),
        strategy: ClearanceStrategy::Offset,
        direction: CutDirection::Climb,
        entry: Entry::Plunge,
    }
}

/// `project` with `op` as its only operation.
fn with_op(project: &Project, op: Operation) -> Project {
    let mut p = project.clone();
    p.operations = vec![op];
    p
}

/// The generator's result for `op` in `project` (which gains `op`).
fn try_inlay(project: &Project, op: &InlayOp) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    generate_inlay(&with_op(project, Operation::Inlay(op.clone())), op)
}

/// [`try_inlay`], which must succeed; every run passes `assert_run_safe`,
/// the cone test included.
fn inlay_runs(project: &Project, op: &InlayOp) -> (Vec<ToolRun>, Vec<Warning>) {
    let p = with_op(project, Operation::Inlay(op.clone()));
    let (runs, warnings) =
        generate_inlay(&p, op).unwrap_or_else(|e| panic!("the inlay generates: {e}\n{e:?}"));
    for run in &runs {
        assert_run_safe(run, &p, &Operation::Inlay(op.clone()));
    }
    (runs, warnings)
}

/// The V-bit's own run of `runs`.
fn main_run(runs: &[ToolRun]) -> &Toolpath {
    &runs
        .iter()
        .find(|r| r.label == RunLabel::Main)
        .expect("a V-bit run")
        .toolpath
}

/// The star part's female and male, generated once for the tests that
/// only read them.
fn star_side(side: InlaySide) -> &'static (Vec<ToolRun>, Vec<Warning>) {
    static FEMALE: OnceLock<(Vec<ToolRun>, Vec<Warning>)> = OnceLock::new();
    static MALE: OnceLock<(Vec<ToolRun>, Vec<Warning>)> = OnceLock::new();
    let cell = match side {
        InlaySide::Female => &FEMALE,
        InlaySide::Male => &MALE,
    };
    cell.get_or_init(|| inlay_runs(&star_project(THICKNESS_MM), &inlay(side)))
}

/// Every point a cutting move of `toolpath` runs through at its ends, with
/// the move's start.
fn cut_points(toolpath: &Toolpath) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        if mv.is_cut()
            && let Some(a) = from
        {
            out.push(a);
            out.push(mv.to);
        }
        from = Some(mv.to);
    }
    out
}

/// The distance from `p` to the nearest outline of `contours`, mm.
fn distance_to(contours: &[Polyline], p: Point) -> f64 {
    contours
        .iter()
        .filter_map(|c| nearest_point(c, p))
        .map(|n| n.gap_mm)
        .fold(f64::INFINITY, f64::min)
}

/// Every outline of `regions`.
fn outlines(regions: &[Region]) -> Vec<Polyline> {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().clone())
        .collect()
}

/// Whether `p` is inside one of `regions` or within `slack_mm` of an
/// outline of one.
fn in_regions(regions: &[Region], p: Point, slack_mm: f64) -> bool {
    regions.iter().any(|r| r.contains_point(p)) || distance_to(&outlines(regions), p) <= slack_mm
}

/// The star placed, project space.
fn star_placed() -> Polyline {
    let points: Vec<(f64, f64)> = star_points()
        .iter()
        .map(|&(x, y)| (x + CENTRE.0, y + CENTRE.1))
        .collect();
    polygon(&points)
}

/// Sample points along the placed star's edges, fractions 0.25 to 0.75
/// of each edge. Within `(D − G) · tan 30° / tan 21.5°` =
/// 4.4 mm of a tip (21.5° is half the tip's angle) the arm is narrower
/// than the groove 3 mm down, so there the wall at that depth is not the
/// edge's own inset but the inset tip's, farther off; 0.25 of an edge is
/// 5.8 mm.
fn star_samples() -> Vec<Point> {
    let corners: Vec<(f64, f64)> = star_points()
        .iter()
        .map(|&(x, y)| (x + CENTRE.0, y + CENTRE.1))
        .collect();
    let mut out = Vec::new();
    for (k, &(ax, ay)) in corners.iter().enumerate() {
        let (bx, by) = corners[(k + 1) % corners.len()];
        for step in 0..=5 {
            let t = 0.25 + 0.1 * f64::from(step);
            out.push(Point::new(ax + (bx - ax) * t, ay + (by - ay) * t));
        }
    }
    out
}

/// Sample points on the placed circle, every 15°.
fn circle_samples() -> Vec<Point> {
    (0..24)
        .map(|k| {
            let a = f64::from(k) * 15.0_f64.to_radians();
            Point::new(
                CENTRE.0 + CIRCLE_MM * a.cos(),
                CENTRE.1 + CIRCLE_MM * a.sin(),
            )
        })
        .collect()
}

/// How far the V-bit's cones along `toolpath`, at height `z_mm`, stop
/// short of `q`, mm: the least over every point `p` of every straight
/// cutting move whose tip is at or below `z_mm` of `|q − p| − (z_mm − z_p)
/// · slope`, the distance from `q` to that cone's disc at `z_mm`.
/// Negative when a disc reaches past `q`, by that much. The function is
/// convex along a straight move, so its least is searched, not sampled.
fn disc_gap_mm(toolpath: &Toolpath, q: Point, z_mm: f64, slope: f64) -> f64 {
    let mut best = f64::INFINITY;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() || !matches!(mv.kind, MoveKind::Linear) {
            continue;
        }
        let b = mv.to;
        // The part of the move whose tip is at or below `z_mm`.
        let (lo, hi) = if (b.z - a.z).abs() < 1e-12 {
            if a.z > z_mm {
                continue;
            }
            (0.0, 1.0)
        } else {
            let t = (z_mm - a.z) / (b.z - a.z);
            if b.z > a.z {
                (0.0, t.min(1.0))
            } else {
                (t.max(0.0), 1.0)
            }
        };
        if lo > hi {
            continue;
        }
        let at = |t: f64| {
            let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let z = a.z + (b.z - a.z) * t;
            q.distance_to(p) - (z_mm - z) * slope
        };
        // A cheap bound: no point of the move is nearer `q` than its
        // segment, and no disc wider than the lowest end's.
        let seg = dc_geometry::Segment {
            start: a.xy(),
            end: b.xy(),
            bulge: 0.0,
        };
        let bound = dc_geometry::point_segment_distance_mm(seg, q) - (z_mm - a.z.min(b.z)) * slope;
        if bound >= best {
            continue;
        }
        let (mut x0, mut x1) = (lo, hi);
        for _ in 0..80 {
            let m0 = x0 + (x1 - x0) / 3.0;
            let m1 = x1 - (x1 - x0) / 3.0;
            if at(m0) <= at(m1) {
                x1 = m1;
            } else {
                x0 = m0;
            }
        }
        best = best.min(at(lo)).min(at(hi)).min(at((x0 + x1) / 2.0));
    }
    best
}

/// `toolpath` mirrored back about X = `axis_x_mm`.
fn mirrored_back(toolpath: &Toolpath, axis_x_mm: f64) -> Toolpath {
    let mut out = toolpath.clone();
    for mv in &mut out.moves {
        mv.to.x = 2.0 * axis_x_mm - mv.to.x;
    }
    out
}

/// The axis of the one `InlayMaleMirrored` among `warnings`.
fn mirror_axis(warnings: &[Warning]) -> f64 {
    let axes: Vec<f64> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayMaleMirrored { axis_x_mm, .. } => Some(*axis_x_mm),
            _ => None,
        })
        .collect();
    assert_eq!(axes.len(), 1, "one mirror note: {warnings:?}");
    axes[0]
}

/// Acceptance 56: the female is a V-Carve of its targets with
/// `flat_depth_mm: D` from the stock top, move for move, with and without
/// a clearing pass.
#[test]
fn female_equals_a_flat_depth_v_carve() {
    let project = star_project(THICKNESS_MM);
    for clearing in [Vec::new(), vec![clearing_pass()]] {
        let op = InlayOp {
            clearing: clearing.clone(),
            ..inlay(InlaySide::Female)
        };
        let carve = VCarveOp {
            id: op.id.clone(),
            name: op.name.clone(),
            enabled: true,
            targets: op.targets.clone(),
            tool: op.tool.clone(),
            strategy: VCarveStrategy::Medial,
            clearing,
            flat_depth_mm: Some(DEPTH_MM),
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false,
            step_over_fraction: op.step_over_fraction,
            machining_order: op.machining_order,
            through_cut_allowance_mm: op.through_cut_allowance_mm,
            safe_z_mm: op.safe_z_mm,
        };
        let (female, female_warnings) = inlay_runs(&project, &op);
        let (v_carve, v_carve_warnings) =
            generate_vcarve(&with_op(&project, Operation::VCarve(carve.clone())), &carve)
                .expect("the V-Carve generates");
        assert_eq!(female.len(), v_carve.len());
        for (a, b) in female.iter().zip(&v_carve) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.tool, b.tool);
            assert_eq!(a.toolpath, b.toolpath, "{:?}", a.label);
        }
        assert_eq!(female_warnings, v_carve_warnings);
        assert!(!female.is_empty());
    }
}

/// Acceptance 57: the male cuts the design mirrored about the vertical
/// line through the middle of its targets' joint bounding box, and says
/// so once. A triangle (X 10 to 40) and a square (X 70 to 76) mirror about
/// X 43: the male cuts round the triangle at X 46 to 76 and the square at
/// X 10 to 16, so it cuts inside the triangle as drawn and never inside
/// the mirrored triangle or square, the plug.
#[test]
fn male_mirrors_about_the_targets_centre_line() {
    let triangle = polygon(&[(10.0, 10.0), (40.0, 10.0), (10.0, 30.0)]);
    let project = two_shapes();
    let op = InlayOp {
        depth_mm: Some(2.0),
        glue_gap_mm: Some(0.5),
        plug_depth_mm: Some(1.0),
        margin_mm: Some(3.0),
        ..inlay(InlaySide::Male)
    };
    let mirror = male_mirror(&project, &op.targets).expect("the targets resolve");
    let expected = Transform::mirror_x_about(43.0).coeffs();
    for (got, want) in mirror.coeffs().iter().zip(expected) {
        assert!((got - want).abs() < 1e-9, "{:?}", mirror.coeffs());
    }
    let (runs, warnings) = inlay_runs(&project, &op);
    assert!((mirror_axis(&warnings) - 43.0).abs() < 1e-9);

    let mirrored: Vec<Contour> = project.definitions[&DefinitionId::new("d1")]
        .profiles
        .iter()
        .map(|p| Contour::new(mirror.apply_polyline(p).expect("mirrors")).expect("a contour"))
        .collect();
    let (design, _) = nest(mirrored).expect("nests");
    let band = surround(&design, 3.0, 0.0, project.flatten_tolerance_mm);
    let points = cut_points(main_run(&runs));
    assert!(!points.is_empty());
    for p in &points {
        assert!(
            in_regions(&band, p.xy(), 0.01),
            "{p:?} is outside the mirrored surround"
        );
        assert!(
            !design.iter().any(|r| r.contains_point(p.xy())),
            "{p:?} cuts the plug"
        );
    }
    let (drawn, _) = nest(vec![Contour::new(triangle).expect("a contour")]).expect("nests");
    assert!(
        points.iter().any(
            |p| drawn[0].contains_point(p.xy()) && distance_to(&outlines(&drawn), p.xy()) > 0.5
        ),
        "the male cuts where the triangle is drawn, round the mirrored square"
    );
}

/// Acceptance 57: every cutting point of the star's male lies in the band
/// from the star out to 8 mm beyond it, or inside the circle, which is a
/// hole of the design and so is carved; both are cut.
#[test]
fn male_carves_the_surround_and_the_holes() {
    let (runs, _) = star_side(InlaySide::Male);
    let star = star_placed();
    let (star_region, _) = nest(vec![Contour::new(star.clone()).expect("a star")]).expect("nests");
    let centre = Point::new(CENTRE.0, CENTRE.1);
    let mut in_band = 0;
    let mut in_circle = 0;
    for p in cut_points(main_run(runs)) {
        let xy = p.xy();
        let to_star = distance_to(std::slice::from_ref(&star), xy);
        if xy.distance_to(centre) <= CIRCLE_MM + 0.01 {
            in_circle += 1;
            continue;
        }
        assert!(
            !star_region[0].contains_point(xy) || to_star <= 0.01,
            "{p:?} cuts the plug"
        );
        assert!(to_star <= MARGIN_MM + 0.01, "{p:?} is beyond the margin");
        in_band += 1;
    }
    assert!(in_band > 0 && in_circle > 0, "{in_band} {in_circle}");
}

/// Acceptance 57: the male's floor is `D − G + P` = 5 mm below the stock
/// top, and nothing is cut below it.
#[test]
fn male_floor_is_at_depth_minus_gap_plus_plug() {
    let (runs, _) = star_side(InlaySide::Male);
    let floor_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM + PLUG_DEPTH_MM);
    let lowest = cut_points(main_run(runs))
        .iter()
        .map(|p| p.z)
        .fold(f64::INFINITY, f64::min);
    assert!(lowest >= floor_z_mm - 1e-6, "{lowest}");
    assert!((lowest - floor_z_mm).abs() < 0.01, "{lowest}");
}

/// Acceptance 57: the bit's flank passes through every mirrored outline
/// point, the star's and the circle's, at the start plane, Z −3, within
/// 0.02 mm, and 3 mm above it, at the stock top, it stands the design's
/// inset of 3 · tan 30° = 1.732 mm into the plug.
#[test]
fn male_flank_passes_through_the_outline_at_the_start_plane() {
    let (runs, warnings) = star_side(InlaySide::Male);
    // The star is symmetric about its centre line, so its mirror is
    // itself.
    assert!((mirror_axis(warnings) - CENTRE.0).abs() < 1e-9);
    let toolpath = main_run(runs);
    let start_plane_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM);
    let top_inset_mm = (DEPTH_MM - GLUE_GAP_MM) * slope();
    assert!((top_inset_mm - 1.732).abs() < 0.001);
    for q in star_samples().into_iter().chain(circle_samples()) {
        let at_start = disc_gap_mm(toolpath, q, start_plane_z_mm, slope());
        assert!(at_start.abs() <= FIT_MM, "{q:?}: {at_start}");
        let at_top = -disc_gap_mm(toolpath, q, THICKNESS_MM, slope());
        assert!((at_top - top_inset_mm).abs() <= FIT_MM, "{q:?}: {at_top}");
    }
}

/// The fit property (design "Inlay" step 5; acceptance 57): mirrored
/// back, the plug's wall at height `h` above its start plane stands at
/// the same inset from the design's outline as the pocket's wall `h` below
/// the stock top, `h · tan 30°`, within 0.02 mm, for `h` from 0 to `D − G`
/// = 3 mm. Read from the two toolpaths' cones: the female's discs at
/// depth `h` stop that far short of each outline point, the male's reach
/// that far past it. The star is symmetric about its mirror line, so the
/// triangle and square of `male_mirrors_about_the_targets_centre_line`,
/// which the mirror moves, are checked too (`D − G` = 1.5 mm there).
#[test]
fn female_and_male_walls_mate() {
    let (female, _) = star_side(InlaySide::Female);
    let (male, warnings) = star_side(InlaySide::Male);
    let star = walls_mate(
        main_run(female),
        main_run(male),
        mirror_axis(warnings),
        &star_samples(),
        DEPTH_MM - GLUE_GAP_MM,
    );
    // #448: an open, rounded band edge changes nothing the walls mate by.
    let open = InlayOp {
        clearing: vec![clearing_pass()],
        outline_rounding_mm: Some(12.0),
        ..inlay(InlaySide::Male)
    };
    let (open_male, open_warnings) = inlay_runs(&star_project(THICKNESS_MM), &open);
    let rounded = walls_mate(
        main_run(female),
        main_run(&open_male),
        mirror_axis(&open_warnings),
        &star_samples(),
        DEPTH_MM - GLUE_GAP_MM,
    );

    let pair = two_shapes();
    let op = |side| InlayOp {
        depth_mm: Some(2.0),
        glue_gap_mm: Some(0.5),
        plug_depth_mm: Some(1.0),
        margin_mm: Some(3.0),
        ..inlay(side)
    };
    let (female, _) = inlay_runs(&pair, &op(InlaySide::Female));
    let (male, warnings) = inlay_runs(&pair, &op(InlaySide::Male));
    let corners = [
        vec![(10.0, 10.0), (40.0, 10.0), (10.0, 30.0)],
        vec![(70.0, 10.0), (76.0, 10.0), (76.0, 16.0), (70.0, 16.0)],
    ];
    let samples: Vec<Point> = corners
        .iter()
        .flat_map(|c| edge_samples(c, &[0.3, 0.4, 0.5, 0.6, 0.7]))
        .collect();
    let pair = walls_mate(
        main_run(&female),
        main_run(&male),
        mirror_axis(&warnings),
        &samples,
        1.5,
    );
    println!(
        "fit: worst mismatch {star:.5} mm on the star ({rounded:.5} mm open and rounded), {pair:.5} mm on the pair"
    );
}

/// The triangle (10, 10), (40, 10), (10, 30) and the 6 mm square at
/// (70, 10), on 100 × 50 mm stock: they mirror about X 43.
fn two_shapes() -> Project {
    project_of(
        vec![
            polygon(&[(10.0, 10.0), (40.0, 10.0), (10.0, 30.0)]),
            Polyline::rect(Point::new(70.0, 10.0), 6.0, 6.0),
        ],
        (0.0, 0.0),
        100.0,
        50.0,
    )
}

/// Points at `fractions` along each edge of the closed polygon `corners`.
fn edge_samples(corners: &[(f64, f64)], fractions: &[f64]) -> Vec<Point> {
    let mut out = Vec::new();
    for (k, &(ax, ay)) in corners.iter().enumerate() {
        let (bx, by) = corners[(k + 1) % corners.len()];
        for t in fractions {
            out.push(Point::new(ax + (bx - ax) * t, ay + (by - ay) * t));
        }
    }
    out
}

/// Asserts the fit property for the female's V-bit run `female` and the
/// male's `male` (mirrored about X `axis_x_mm`, on [`THICKNESS_MM`] stock,
/// start plane `rise_mm` below the stock top) at every outline point of
/// `samples`, for heights 0 to `rise_mm` in tenths; returns the worst
/// mismatch between the two walls, mm.
fn walls_mate(
    female: &Toolpath,
    male: &Toolpath,
    axis_x_mm: f64,
    samples: &[Point],
    rise_mm: f64,
) -> f64 {
    let male = mirrored_back(male, axis_x_mm);
    let start_plane_z_mm = THICKNESS_MM - rise_mm;
    let mut worst_mm: f64 = 0.0;
    for &q in samples {
        for step in 0..=10 {
            let h = rise_mm * f64::from(step) / 10.0;
            let inset = h * slope();
            let pocket = disc_gap_mm(female, q, THICKNESS_MM - h, slope());
            let plug = -disc_gap_mm(&male, q, start_plane_z_mm + h, slope());
            assert!(
                (pocket - inset).abs() <= FIT_MM,
                "{q:?} h {h}: pocket {pocket}, inset {inset}"
            );
            assert!(
                (plug - inset).abs() <= FIT_MM,
                "{q:?} h {h}: plug {plug}, inset {inset}"
            );
            assert!(
                (pocket - plug).abs() <= FIT_MM,
                "{q:?} h {h}: pocket {pocket}, plug {plug}"
            );
            worst_mm = worst_mm.max((pocket - plug).abs());
        }
    }
    worst_mm
}

/// Security review H4: the male's flank is engaged from the stock top,
/// `D − G + P`. Corbin's `2.5, 0.25, 1.5` engages the 90° bit 3.75 mm, past
/// its 3.175 mm cone; the female of the same values engages it 2.5 mm and
/// generates.
#[test]
fn male_engagement_beyond_the_cone_is_an_error() {
    let mut project = star_project(THICKNESS_MM);
    project.tools[0] = v_bit(90.0);
    let corbin = |side| InlayOp {
        depth_mm: Some(2.5),
        glue_gap_mm: Some(0.25),
        plug_depth_mm: Some(1.5),
        ..inlay(side)
    };
    match try_inlay(&project, &corbin(InlaySide::Male)) {
        Err(CamError::VBitConeExceeded {
            depth_mm, cone_mm, ..
        }) => {
            assert!((depth_mm - 3.75).abs() < 1e-9, "{depth_mm}");
            assert!((cone_mm - 3.175).abs() < 1e-9, "{cone_mm}");
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }
    inlay_runs(&project, &corbin(InlaySide::Female));
}

/// `0 <= G < D` on either side, else `InlayGlueGap`; a male's margin must
/// be above 0, else `InlayMargin` (acceptance 57: a `glue_gap_mm` of 4).
#[test]
fn glue_gap_at_or_above_depth_is_an_error() {
    let project = star_project(THICKNESS_MM);
    for side in [InlaySide::Female, InlaySide::Male] {
        for gap in [4.0, 5.0, -0.5] {
            let op = InlayOp {
                glue_gap_mm: Some(gap),
                ..inlay(side)
            };
            match try_inlay(&project, &op) {
                Err(CamError::InlayGlueGap {
                    glue_gap_mm,
                    depth_mm,
                    ..
                }) => {
                    assert!((glue_gap_mm - gap).abs() < 1e-12, "{glue_gap_mm}");
                    assert!((depth_mm - DEPTH_MM).abs() < 1e-12, "{depth_mm}");
                }
                other => panic!("{side:?} {gap}: expected InlayGlueGap, got {other:?}"),
            }
        }
    }
    let flat = InlayOp {
        margin_mm: Some(0.0),
        ..inlay(InlaySide::Male)
    };
    assert!(
        matches!(try_inlay(&project, &flat), Err(CamError::InlayMargin { margin_mm, .. }) if margin_mm.abs() < 1e-12)
    );
    let unset = InlayOp {
        plug_depth_mm: None,
        ..inlay(InlaySide::Female)
    };
    assert!(matches!(
        try_inlay(&project, &unset),
        Err(CamError::InlayValueUnset {
            field: "plug_depth_mm",
            ..
        })
    ));
}

/// Acceptance 57: on 10 mm stock the male leaves 10 − 5 = 5 mm under its
/// floor and warns `inlay_plug_base_thin`; on 12 mm it leaves 7 and does
/// not, nor does the female. A 20 × 10 rectangle keeps the test quick.
#[test]
fn thin_plug_base_warns_on_10_mm_stock_and_not_on_12() {
    let thin = |thickness_mm: f64, side| {
        let mut project = project_of(
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)],
            (30.0, 30.0),
            100.0,
            100.0,
        );
        project.stock.thickness_mm = Some(thickness_mm);
        let (_, warnings) = inlay_runs(&project, &inlay(side));
        warnings
            .iter()
            .filter_map(|w| match w {
                Warning::InlayPlugBaseThin { base_mm, .. } => Some(*base_mm),
                _ => None,
            })
            .collect::<Vec<f64>>()
    };
    let on_10 = thin(10.0, InlaySide::Male);
    assert_eq!(on_10.len(), 1, "{on_10:?}");
    assert!((on_10[0] - 5.0).abs() < 1e-9, "{on_10:?}");
    assert!(thin(12.0, InlaySide::Male).is_empty());
    assert!(thin(10.0, InlaySide::Female).is_empty());
}

/// Acceptance 57: with `clearing: [t2]` the male's first run is the end
/// mill's, which clears the surround's floor at Z −5 (#448: the band's
/// edge is open, so the floor runs from the plug's foot, `P · tan 30°`
/// out, to the band's edge, rounded by the end mill's radius; its centre
/// at least its radius inside), and the V-bit's floor cutting shrinks to
/// what the end mill cannot reach, as in acceptance 51.
#[test]
fn clearing_pass_clears_the_male_floor() {
    let project = star_project(THICKNESS_MM);
    let op = InlayOp {
        clearing: vec![clearing_pass()],
        ..inlay(InlaySide::Male)
    };
    let (runs, _) = inlay_runs(&project, &op);
    let labels: Vec<&RunLabel> = runs.iter().map(|r| &r.label).collect();
    assert_eq!(
        labels,
        [&RunLabel::Clearing(ToolId::new("t2")), &RunLabel::Main]
    );
    let floor_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM + PLUG_DEPTH_MM);
    let clearing = cut_points(&runs[0].toolpath);
    let lowest = clearing.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    assert!((lowest - floor_z_mm).abs() < 1e-6, "{lowest}");

    let design: Vec<Contour> = [
        star_placed(),
        Polyline::circle(Point::new(CENTRE.0, CENTRE.1), CIRCLE_MM),
    ]
    .into_iter()
    .map(|p| Contour::new(p).expect("a contour"))
    .collect();
    let (design, _) = nest(design).expect("nests");
    let tolerance_mm = project.flatten_tolerance_mm;
    let radius_mm = 3.175 / 2.0;
    let floors = surround_floor_regions(
        &surround(&design, MARGIN_MM, radius_mm, tolerance_mm),
        &design,
        PLUG_DEPTH_MM * slope(),
        true,
        tolerance_mm,
    );
    for p in &clearing {
        assert!(
            floors.iter().any(|r| r.contains_point(p.xy()))
                && distance_to(&outlines(&floors), p.xy()) >= radius_mm - 0.02,
            "{p:?} is not a floor centre"
        );
    }

    let floor_length = |toolpath: &Toolpath| {
        let mut from: Option<Point3> = None;
        let mut length = 0.0;
        for mv in &toolpath.moves {
            if let Some(a) = from
                && mv.is_cut()
                && (a.z - floor_z_mm).abs() < 1e-6
                && (mv.to.z - floor_z_mm).abs() < 1e-6
            {
                length += a.xy().distance_to(mv.to.xy());
            }
            from = Some(mv.to);
        }
        length
    };
    let (alone, _) = star_side(InlaySide::Male);
    let with = floor_length(&runs[1].toolpath);
    let without = floor_length(main_run(alone));
    println!("V-bit floor {with:.1} mm with clearing, {without:.1} mm without");
    assert!(with < 0.5 * without, "{with} {without}");
}

// Format 23 (#443): the male's cut-out.

/// The cut-out's through-cut allowance, mm.
const CUT_OUT_ALLOWANCE_MM: f64 = 0.2;
/// Its tabs' top above the stock bottom, mm.
const TAB_TOP_MM: f64 = 3.0;
/// Its tabs' width, mm.
const TAB_WIDTH_MM: f64 = 6.0;

/// The star part's male with a cut-out by the end mill `t2`: four rect
/// tabs 6 mm wide and 3 mm tall, 0.2 mm through.
fn cut_out_male() -> InlayOp {
    InlayOp {
        cut_out: Some(dc_project::InlayCutOut {
            tool: ToolId::new("t2"),
            tabs: Some(dc_project::TabSpec {
                placement: dc_project::TabPlacement::Count { count: 4 },
                width_mm: TAB_WIDTH_MM,
                height_mm: TAB_TOP_MM,
                shape: dc_project::TabShape::Rect,
                on_holes: false,
                avoid_corners: true,
            }),
            through_cut_allowance_mm: CUT_OUT_ALLOWANCE_MM,
        }),
        ..inlay(InlaySide::Male)
    }
}

/// The cut-out run of [`cut_out_male`] on the star, generated once.
fn cut_out_run() -> &'static ToolRun {
    static RUNS: OnceLock<(Vec<ToolRun>, Vec<Warning>)> = OnceLock::new();
    let (runs, _) = RUNS.get_or_init(|| inlay_runs(&star_project(THICKNESS_MM), &cut_out_male()));
    runs.iter()
        .find(|r| matches!(r.label, RunLabel::CutOut(_)))
        .expect("a cut-out run")
}

/// Points along every cutting move of `toolpath`: its ends and three
/// between, round the arc for an arc.
fn cut_samples(toolpath: &Toolpath) -> Vec<Point3> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() {
            continue;
        }
        let b = mv.to;
        out.push(a);
        for k in 1..4 {
            let t = f64::from(k) / 4.0;
            let z = a.z + (b.z - a.z) * t;
            let xy = match mv.kind {
                MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                    let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
                    let a0 = (a.y - centre.y).atan2(a.x - centre.x);
                    let a1 = (b.y - centre.y).atan2(b.x - centre.x);
                    let tau = std::f64::consts::TAU;
                    let mut sweep = a1 - a0;
                    if ccw && sweep <= 0.0 {
                        sweep += tau;
                    } else if !ccw && sweep >= 0.0 {
                        sweep -= tau;
                    }
                    let r = a.xy().distance_to(centre);
                    let angle = a0 + sweep * t;
                    Point::new(centre.x + r * angle.cos(), centre.y + r * angle.sin())
                }
                _ => Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
            };
            out.push(Point3::new(xy.x, xy.y, z));
        }
        out.push(b);
    }
    out
}

/// The design of the star project mirrored as the male mirrors it,
/// nested.
fn mirrored_design(project: &Project) -> Vec<Region> {
    let mirror = male_mirror(project, &[InstanceId::new("i1")]).expect("a mirror");
    let design: Vec<Contour> = [
        star_placed(),
        Polyline::circle(Point::new(CENTRE.0, CENTRE.1), CIRCLE_MM),
    ]
    .into_iter()
    .map(|p| Contour::new(mirror.apply_polyline(&p).expect("mirrored")).expect("a contour"))
    .collect();
    nest(design).expect("nests").0
}

/// #443: the cut-out is the male's last run, after the V-bit's, with its
/// own end mill; with a clearing pass the runs are T2 clearing, T1 V-bit,
/// T2 cut-out.
#[test]
fn cut_out_runs_last_with_its_own_tool() {
    let project = star_project(THICKNESS_MM);
    let (runs, _) = inlay_runs(&project, &cut_out_male());
    let labels: Vec<(&RunLabel, &ToolId)> = runs.iter().map(|r| (&r.label, &r.tool)).collect();
    assert_eq!(
        labels,
        [
            (&RunLabel::Main, &ToolId::new("t1")),
            (&RunLabel::CutOut(ToolId::new("t2")), &ToolId::new("t2")),
        ]
    );

    let op = InlayOp {
        clearing: vec![clearing_pass()],
        ..cut_out_male()
    };
    let (runs, _) = inlay_runs(&project, &op);
    let labels: Vec<(&RunLabel, &ToolId)> = runs.iter().map(|r| (&r.label, &r.tool)).collect();
    assert_eq!(
        labels,
        [
            (&RunLabel::Clearing(ToolId::new("t2")), &ToolId::new("t2")),
            (&RunLabel::Main, &ToolId::new("t1")),
            (&RunLabel::CutOut(ToolId::new("t2")), &ToolId::new("t2")),
        ]
    );
    // Without a cut-out the male is as before.
    let (runs, _) = star_side(InlaySide::Male);
    assert!(runs.iter().all(|r| !matches!(r.label, RunLabel::CutOut(_))));
}

/// #461: the star's band edge `B`, the mirrored star's outline grown by
/// the margin and closed by `rounding_mm` (design "Inlay", step 4), built
/// here with `dc_geometry`, and its outlines.
fn band_edge(project: &Project, rounding_mm: f64) -> (Vec<Region>, Vec<Polyline>) {
    let band = rounded_band(
        &mirrored_design(project),
        MARGIN_MM,
        rounding_mm,
        project.flatten_tolerance_mm,
    );
    let edge = outlines(&band);
    (band, edge)
}

/// The cut-out run of `op` on the star.
fn cut_out_of(project: &Project, op: &InlayOp) -> Toolpath {
    let (runs, _) = inlay_runs(project, op);
    runs.into_iter()
        .find(|r| matches!(r.label, RunLabel::CutOut(_)))
        .expect("a cut-out run")
        .toolpath
}

/// #461: with an open edge (the end mill's clearing pass fits the floor
/// ring) the floor reaches the band's edge `B`, so the cut-out's centre
/// runs on `B` inset by its radius: every point of every cut is the
/// radius from `B`, within the flatten tolerance, inside `B`. Above the
/// floor (its plunges) its swept disc lies inside `B`, which the edge
/// pass cut down to the floor, so it cuts nothing the carve left. Plain
/// (`B` closed by the edge pass's radius) and rounded by 6 mm. Where the
/// band is plain it runs `M − r` = 6.41 mm from the star, where the old
/// rule ran it in the middle of the ring, `(M + P·tan 30°) / 2` = 4.58 mm.
#[test]
fn cut_out_hugs_the_band_edge() {
    let project = star_project(THICKNESS_MM);
    let tolerance_mm = project.flatten_tolerance_mm;
    let star: Vec<Polyline> = mirrored_design(&project)
        .iter()
        .map(|r| r.outer().polyline().clone())
        .collect();
    for rounding_mm in [None, Some(6.0)] {
        let op = InlayOp {
            clearing: vec![clearing_pass()],
            outline_rounding_mm: rounding_mm,
            ..cut_out_male()
        };
        let toolpath = cut_out_of(&project, &op);
        let edge_rounding_mm = rounding_mm.unwrap_or(0.0).max(END_MILL_RADIUS_MM);
        let (band, edge) = band_edge(&project, edge_rounding_mm);
        let samples = cut_samples(&toolpath);
        assert!(samples.len() > 100, "{}", samples.len());
        let mut worst_mm: f64 = 0.0;
        let mut farthest_mm: f64 = 0.0;
        for p in &samples {
            assert!(
                in_regions(&band, p.xy(), 1e-6),
                "{rounding_mm:?}: {p:?} is outside the band"
            );
            let gap_mm = distance_to(&edge, p.xy());
            assert!(
                (gap_mm - END_MILL_RADIUS_MM).abs() <= tolerance_mm + 1e-6,
                "{rounding_mm:?}: {p:?} is {gap_mm:.4} mm from the band's edge, not its radius"
            );
            if p.z > male_floor_z_mm() + 1e-9 {
                assert!(
                    gap_mm >= END_MILL_RADIUS_MM - tolerance_mm - 1e-6,
                    "{rounding_mm:?}: above the floor at {p:?} the end mill reaches past the band"
                );
            }
            worst_mm = worst_mm.max((gap_mm - END_MILL_RADIUS_MM).abs());
            farthest_mm = farthest_mm.max(distance_to(&star, p.xy()));
        }
        println!(
            "{rounding_mm:?}: the cut-out runs its radius inside the band's edge within {worst_mm:.4} mm, up to {farthest_mm:.3} mm from the star"
        );
        assert!(
            farthest_mm >= MARGIN_MM - END_MILL_RADIUS_MM - tolerance_mm,
            "{rounding_mm:?}: {farthest_mm}"
        );
    }
}

/// #461: with a V wall (no clearing pass) the floor ends at the slope's
/// foot, `P·tan 30°` = 1.155 mm inside the band's edge `B`, so the
/// cut-out's centre runs on `B` inset by `P·tan 30° + r` = 2.742 mm:
/// every point of every cut within the flatten tolerance of that, inside
/// `B`. Its edge stays on the floor and never reaches the slope, so the
/// freed plug has no knife edge. Plain and rounded by 6 mm (`B` closed by
/// `R` alone).
#[test]
fn cut_out_hugs_the_slope_foot_with_a_v_wall() {
    let project = star_project(THICKNESS_MM);
    let tolerance_mm = project.flatten_tolerance_mm;
    let inset_mm = PLUG_DEPTH_MM * slope() + END_MILL_RADIUS_MM;
    for rounding_mm in [None, Some(6.0)] {
        let op = InlayOp {
            outline_rounding_mm: rounding_mm,
            ..cut_out_male()
        };
        let toolpath = cut_out_of(&project, &op);
        let (band, edge) = band_edge(&project, rounding_mm.unwrap_or(0.0));
        let samples = cut_samples(&toolpath);
        assert!(samples.len() > 100, "{}", samples.len());
        for p in &samples {
            assert!(
                in_regions(&band, p.xy(), 1e-6),
                "{rounding_mm:?}: {p:?} is outside the band"
            );
            let gap_mm = distance_to(&edge, p.xy());
            assert!(
                (gap_mm - inset_mm).abs() <= tolerance_mm + 1e-6,
                "{rounding_mm:?}: {p:?} is {gap_mm:.4} mm from the band's edge, not {inset_mm:.4}"
            );
        }
    }
}

/// #443: the band's flat floor lies between the mirrored design grown by
/// `P·tan 30°` = 1.155 mm and by `M − P·tan 30°` = 6.845 mm; the end
/// mill's footprint stays at least its radius inside both edges of that
/// ring, as `dc_geometry` derives it (the V-bit's floor).
#[test]
fn cut_out_stays_inside_the_floor_ring() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let tolerance_mm = project.flatten_tolerance_mm;
    let floors = floor_regions(
        &surround(&design, MARGIN_MM, 0.0, tolerance_mm),
        PLUG_DEPTH_MM * slope(),
        tolerance_mm,
    );
    // The ring is the floor round the star, not the circle's.
    let ring: Vec<Region> = floors
        .into_iter()
        .filter(|r| !r.contains_point(Point::new(CENTRE.0, CENTRE.1)) && !r.holes().is_empty())
        .collect();
    assert_eq!(ring.len(), 1, "one floor ring round the star");
    let radius_mm = 3.175 / 2.0;
    let mut least_mm = f64::INFINITY;
    for p in cut_samples(&cut_out_run().toolpath) {
        assert!(
            ring[0].contains_point(p.xy()),
            "{p:?} is off the floor ring"
        );
        let gap_mm = distance_to(&outlines(&ring), p.xy());
        least_mm = least_mm.min(gap_mm);
        assert!(
            gap_mm >= radius_mm - 1e-6,
            "{p:?}: the end mill reaches {:.4} mm past the floor ring",
            radius_mm - gap_mm
        );
    }
    println!(
        "the end mill stays {:.3} mm inside the ring",
        least_mm - radius_mm
    );
}

/// #443: no pass cuts the air the carve removed: the highest lap runs no
/// higher than the floor, `D − G + P` = 5 mm below the top, and within a
/// step-down of it; the last goes through the board to the allowance.
#[test]
fn cut_out_starts_at_the_floor_and_goes_through() {
    let floor_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM + PLUG_DEPTH_MM);
    let toolpath = &cut_out_run().toolpath;
    let mut highest_lap = f64::NEG_INFINITY;
    let mut lowest = f64::INFINITY;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() {
            continue;
        }
        lowest = lowest.min(mv.to.z);
        if a.xy().distance_to(mv.to.xy()) > 1e-9 {
            highest_lap = highest_lap.max(a.z.max(mv.to.z));
        }
    }
    assert!(highest_lap <= floor_z_mm + 1e-9, "{highest_lap}");
    assert!(highest_lap >= floor_z_mm - 2.0 - 1e-9, "{highest_lap}");
    assert!((lowest + CUT_OUT_ALLOWANCE_MM).abs() < 1e-9, "{lowest}");
}

/// #443: only the outer outline is cut out: nothing of the cut-out runs
/// inside the star, so the circle's hole stays in the plug.
#[test]
fn cut_out_skips_design_holes() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    for p in cut_samples(&cut_out_run().toolpath) {
        assert!(
            design.iter().all(|r| !r.contains_point(p.xy())),
            "{p:?} is inside the design"
        );
        assert!(
            p.xy().distance_to(Point::new(CENTRE.0, CENTRE.1)) > 12.0 + 4.0,
            "{p:?} is near the circle"
        );
    }
}

/// #443: the four tabs stand: every pass below their top, at Z 1 and Z
/// −0.2, rises over each of them, so the laps run at the tab top in
/// eight stretches, each at least a tab wide.
#[test]
fn cut_out_leaves_the_tabs() {
    let toolpath = &cut_out_run().toolpath;
    let mut stretches: Vec<f64> = Vec::new();
    let mut on_top = false;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        let level_on_top = mv.is_cut()
            && (a.z - TAB_TOP_MM).abs() < 1e-9
            && (mv.to.z - TAB_TOP_MM).abs() < 1e-9
            && a.xy().distance_to(mv.to.xy()) > 1e-9;
        if level_on_top {
            let length = a.xy().distance_to(mv.to.xy());
            match stretches.last_mut() {
                Some(last) if on_top => *last += length,
                _ => stretches.push(length),
            }
        }
        on_top = level_on_top;
    }
    // The pass at the tab top itself is one long level lap.
    let tabs: Vec<f64> = stretches.iter().copied().filter(|&l| l < 50.0).collect();
    assert_eq!(tabs.len(), 8, "{stretches:?}");
    for l in &tabs {
        assert!(*l >= TAB_WIDTH_MM - 1e-6, "{tabs:?}");
    }
}

/// #443: the generator refuses what validation refuses: a cut-out on a
/// female, and one whose end mill does not fit the floor ring (the star
/// with a 4 mm margin: 4 − 2.309 < 3.175).
#[test]
fn cut_out_refuses_a_female_and_a_narrow_floor() {
    let project = star_project(THICKNESS_MM);
    let female = InlayOp {
        side: InlaySide::Female,
        ..cut_out_male()
    };
    let result = try_inlay(&project, &female);
    assert!(
        matches!(result, Err(CamError::InlayCutOutNeedsMale { .. })),
        "{result:?}"
    );
    let narrow = InlayOp {
        margin_mm: Some(4.0),
        ..cut_out_male()
    };
    match try_inlay(&project, &narrow) {
        Err(CamError::InlayCutOutNoRoom {
            diameter_mm,
            needed_mm,
            ..
        }) => {
            assert!((diameter_mm - 3.175).abs() < 1e-12);
            assert!((needed_mm - (2.0 * PLUG_DEPTH_MM * slope() + 3.175)).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

// #448: the male's band with an open, rounded outer edge.

/// The 3.175 mm end mill's radius, mm.
const END_MILL_RADIUS_MM: f64 = 3.175 / 2.0;

/// The male's floor, mm: `D − G + P` below the stock top.
fn male_floor_z_mm() -> f64 {
    THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM + PLUG_DEPTH_MM)
}

/// The star part's male with the end mill's clearing pass, its outline
/// rounded by `rounding_mm`.
fn open_male(rounding_mm: Option<f64>) -> InlayOp {
    InlayOp {
        clearing: vec![clearing_pass()],
        outline_rounding_mm: rounding_mm,
        ..inlay(InlaySide::Male)
    }
}

/// The `InlayBandEdgeVWall` margins among `warnings`.
fn v_wall_warnings(warnings: &[Warning]) -> Vec<Option<f64>> {
    warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayBandEdgeVWall {
                margin_needed_mm, ..
            } => Some(Some(*margin_needed_mm)),
            _ => None,
        })
        .collect()
}

/// How many of `points` lie on the floor `P · tan 30°` = 1.155 mm inside
/// the outlines `edge`, within 0.02 mm: where the V-bit's tip runs to cut
/// a V wall through `edge` at the start plane.
fn on_v_wall(points: &[Point3], edge: &[Polyline]) -> usize {
    let foot_mm = PLUG_DEPTH_MM * slope();
    points
        .iter()
        .filter(|p| {
            (p.z - male_floor_z_mm()).abs() < 1e-6
                && (distance_to(edge, p.xy()) - foot_mm).abs() <= 0.02
        })
        .count()
}

/// #448: with a clearing pass that fits the floor ring the band's outer
/// edge is the end mill's: no V-bit cut comes within 1 mm of the band's
/// edge `B` (the star's outline grown by the margin and closed by the end
/// mill's radius), nor within its cone's radius at the stock top, so the
/// V-bit never cuts beyond `B` below the stock top; nothing comes nearer
/// than the floor ring's width, `M − P · tan 30°`. The same male without
/// clearing cuts the band's outer V wall: its tip runs 1.155 mm inside
/// the plain band's edge at the floor.
#[test]
fn male_with_clearing_has_no_v_wall_at_the_band_edge() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let tolerance_mm = project.flatten_tolerance_mm;
    let (runs, warnings) = inlay_runs(&project, &open_male(None));
    assert!(v_wall_warnings(&warnings).is_empty(), "{warnings:?}");
    let band = rounded_band(&design, MARGIN_MM, END_MILL_RADIUS_MM, tolerance_mm);
    let edge = outlines(&band);
    let points = cut_points(main_run(&runs));
    assert!(!points.is_empty());
    let mut nearest_mm = f64::INFINITY;
    for p in &points {
        assert!(in_regions(&band, p.xy(), 1e-6), "{p:?} is outside the band");
        let gap_mm = distance_to(&edge, p.xy());
        let cone_at_top_mm = (THICKNESS_MM - p.z) * slope();
        assert!(
            gap_mm >= cone_at_top_mm - tolerance_mm,
            "{p:?}: the cone is {cone_at_top_mm:.3} mm wide at the stock top, {gap_mm:.3} mm from the band's edge"
        );
        assert!(
            gap_mm >= 1.0,
            "{p:?} is {gap_mm:.3} mm from the band's edge"
        );
        nearest_mm = nearest_mm.min(gap_mm);
    }
    println!("the V-bit stays {nearest_mm:.3} mm inside the band's edge");
    assert!(
        nearest_mm >= MARGIN_MM - PLUG_DEPTH_MM * slope() - 2.0 * tolerance_mm,
        "{nearest_mm}"
    );

    let (alone, _) = star_side(InlaySide::Male);
    let plain = outlines(&rounded_band(&design, MARGIN_MM, 0.0, tolerance_mm));
    let wall = on_v_wall(&cut_points(main_run(alone)), &plain);
    assert!(wall > 20, "{wall} V-bit points on the outer V wall's foot");
}

/// #448: the edge pass (the end mill's, the first that fits) clears the
/// band out to its edge: its centre never comes nearer the edge than its
/// radius, and every vertex of the band's outer edge (the star's outline
/// grown by the margin and closed by `max(R, r)` = 5 mm) is the radius
/// from its path at the floor, within the flatten tolerance: the outermost
/// centre path is the edge inset by the radius, so the edge is the end
/// mill's vertical wall.
#[test]
fn edge_pass_sweeps_the_band_to_its_outer_edge() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let tolerance_mm = project.flatten_tolerance_mm;
    let (runs, _) = inlay_runs(&project, &open_male(Some(5.0)));
    let band = rounded_band(&design, MARGIN_MM, 5.0, tolerance_mm);
    let edge = outlines(&band);
    let clearing = &runs
        .iter()
        .find(|r| r.label == RunLabel::Clearing(ToolId::new("t2")))
        .expect("a clearing run")
        .toolpath;
    let mut floor: Vec<dc_geometry::Segment> = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &clearing.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() {
            continue;
        }
        for p in [a, mv.to] {
            assert!(in_regions(&band, p.xy(), 1e-6), "{p:?} is outside the band");
            let gap_mm = distance_to(&edge, p.xy());
            assert!(
                gap_mm >= END_MILL_RADIUS_MM - tolerance_mm,
                "{p:?}: the end mill reaches {:.4} mm past the band's edge",
                END_MILL_RADIUS_MM - gap_mm
            );
        }
        if (a.z - male_floor_z_mm()).abs() < 1e-6 && (mv.to.z - male_floor_z_mm()).abs() < 1e-6 {
            floor.push(dc_geometry::Segment {
                start: a.xy(),
                end: mv.to.xy(),
                bulge: 0.0,
            });
        }
    }
    assert!(!floor.is_empty(), "the edge pass cuts at the floor");
    let outer = band
        .iter()
        .flat_map(|r| r.outer().polyline().vertices().iter().map(|v| v.point()));
    let mut worst_mm: f64 = 0.0;
    for q in outer {
        let to_path_mm = floor
            .iter()
            .map(|s| dc_geometry::point_segment_distance_mm(*s, q))
            .fold(f64::INFINITY, f64::min);
        assert!(
            (to_path_mm - END_MILL_RADIUS_MM).abs() <= tolerance_mm + 0.005,
            "{q:?} on the band's edge is {to_path_mm:.4} mm from the edge pass's path"
        );
        worst_mm = worst_mm.max((to_path_mm - END_MILL_RADIUS_MM).abs());
    }
    println!("the edge pass runs its radius from the band's edge within {worst_mm:.4} mm");
}

/// #448: without a clearing pass the band's outer edge keeps its V wall:
/// the V-bit's tip runs 1.155 mm inside the edge at the floor, and its
/// cone passes through every vertex of the edge at the start plane, the
/// edge rounded by `R` alone (none, or 6 mm). There is nothing to warn of.
#[test]
fn male_without_clearing_keeps_the_v_wall() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let tolerance_mm = project.flatten_tolerance_mm;
    let start_plane_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM);
    for rounding_mm in [None, Some(6.0)] {
        let op = InlayOp {
            outline_rounding_mm: rounding_mm,
            ..inlay(InlaySide::Male)
        };
        let (runs, warnings) = inlay_runs(&project, &op);
        assert!(v_wall_warnings(&warnings).is_empty(), "{warnings:?}");
        let band = rounded_band(&design, MARGIN_MM, rounding_mm.unwrap_or(0.0), tolerance_mm);
        let toolpath = main_run(&runs);
        let wall = on_v_wall(&cut_points(toolpath), &outlines(&band));
        assert!(wall > 20, "{rounding_mm:?}: {wall}");
        for region in &band {
            for (k, v) in region.outer().polyline().vertices().iter().enumerate() {
                if k % 4 != 0 {
                    continue;
                }
                let gap_mm = disc_gap_mm(toolpath, v.point(), start_plane_z_mm, slope());
                assert!(
                    gap_mm.abs() <= FIT_MM,
                    "{rounding_mm:?}: the cone stops {gap_mm:.4} mm short of {v:?}"
                );
            }
        }
    }
}

/// #448: a clearing pass whose end mill does not fit the floor ring (the
/// star with a 4 mm margin: 4 − 1.155 < 3.175) leaves the edge a V wall
/// and warns `inlay_band_edge_v_wall`, naming the margin that would fit
/// it, `P · tan 30° + 3.175` = 4.33 mm.
#[test]
fn clearing_that_does_not_fit_keeps_the_v_wall_and_warns() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let tolerance_mm = project.flatten_tolerance_mm;
    let op = InlayOp {
        margin_mm: Some(4.0),
        ..open_male(None)
    };
    let (runs, warnings) = inlay_runs(&project, &op);
    let needed = v_wall_warnings(&warnings);
    assert_eq!(needed.len(), 1, "{warnings:?}");
    let needed_mm = needed[0].expect("a margin");
    assert!(
        (needed_mm - (PLUG_DEPTH_MM * slope() + 3.175)).abs() < 1e-9,
        "{needed_mm}"
    );
    let band = rounded_band(&design, 4.0, 0.0, tolerance_mm);
    let wall = on_v_wall(&cut_points(main_run(&runs)), &outlines(&band));
    assert!(wall > 20, "{wall}");
}

/// #448: with an open edge the floor ring runs from the plug's foot to
/// the band's edge, so a cut-out needs `M − P · tan 30° ≥ d`; with a V
/// wall (no clearing pass) the old `M − 2 · P · tan 30° ≥ d`. A 6.35 mm
/// cut-out in the star's band with a 6 mm margin fits neither, and names
/// `P · tan 30° + 6.35` = 7.505 mm with the end mill's clearing pass and
/// `2 · P · tan 30° + 6.35` = 8.66 mm without.
#[test]
fn cut_out_no_room() {
    let mut project = star_project(THICKNESS_MM);
    project.tools.push(Tool {
        id: ToolId::new("t3"),
        name: "6.35 mm end mill".into(),
        ..tool_6_35_upcut()
    });
    let foot_mm = PLUG_DEPTH_MM * slope();
    for (clearing, want_mm) in [
        (vec![clearing_pass()], foot_mm + 6.35),
        (Vec::new(), 2.0 * foot_mm + 6.35),
    ] {
        let op = InlayOp {
            clearing: clearing.clone(),
            margin_mm: Some(6.0),
            cut_out: Some(dc_project::InlayCutOut {
                tool: ToolId::new("t3"),
                tabs: None,
                through_cut_allowance_mm: CUT_OUT_ALLOWANCE_MM,
            }),
            ..inlay(InlaySide::Male)
        };
        match try_inlay(&project, &op) {
            Err(CamError::InlayCutOutNoRoom {
                diameter_mm,
                needed_mm,
                ..
            }) => {
                assert!((diameter_mm - 6.35).abs() < 1e-12);
                assert!(
                    (needed_mm - want_mm).abs() < 1e-9,
                    "{clearing:?}: {needed_mm} not {want_mm}"
                );
            }
            other => panic!("{clearing:?}: {other:?}"),
        }
    }
}

/// #466: a male of several curved shapes apart, its 16 mm margin rounded
/// by 25 mm so the band closes over the gaps between them, with the end
/// mill's clearing pass ramping in. Its clearing run is a Pocket of the
/// band's floor round the plugs, whose rings step out from the plugs and
/// in from the band's edge until they meet; they gather many short
/// round-join arcs, which `dc_geometry::offset` (cavalier_contours) once
/// lost: the island a ring grows vanished, so a ring ran over the plugs,
/// or the loop the footprint is swept from had no inside, so S9 counted
/// the plugs as cut. Every run generates and passes `assert_run_safe`, and
/// below the start plane the end mill's centre stays outside the plugs,
/// its radius beyond the floor's edge (the plugs' outlines grown by `P ·
/// tan 30°`), within the flatten tolerance.
#[test]
fn rounded_male_over_wide_gaps_clears_only_the_band() {
    let project = dc_project::test_support::blobs_project();
    let op = dc_project::test_support::blobs_male_inlay();
    let (runs, _) = inlay_runs(&project, &op);
    let clearing = runs
        .iter()
        .find(|r| r.label == RunLabel::Clearing(ToolId::new("t2")))
        .expect("a clearing run");
    let p = with_op(&project, Operation::Inlay(op.clone()));
    let design = side(&p, &op, &mut Vec::new())
        .expect("the male resolves")
        .design;
    let edges = outlines(&design);
    let start_plane_z_mm = THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM);
    let clear_mm = END_MILL_RADIUS_MM + PLUG_DEPTH_MM * slope() - p.flatten_tolerance_mm;
    let mut cuts = 0;
    for q in cut_points(&clearing.toolpath) {
        if q.z < start_plane_z_mm - 1e-6 {
            cuts += 1;
            let gap_mm = distance_to(&edges, q.xy());
            assert!(
                !in_regions(&design, q.xy(), 0.0) && gap_mm >= clear_mm,
                "{q:?} is {gap_mm:.3} mm from a plug, not {clear_mm:.3}"
            );
        }
    }
    assert!(cuts > 0, "the clearing run cuts the band's floor");
}

/// #452: the acute-notch design's outline, centred at the origin: a Λ 20
/// mm wide and 40 mm tall whose legs are split by a notch from (0, 12)
/// down to the foot, 2.8 mm wide there, so its walls meet at 5.0°. In the
/// male's surround the notch is a convex corner of a hole, which the
/// medial axis must reach.
fn notch_points() -> Vec<(f64, f64)> {
    vec![
        (-10.0, -20.0),
        (-1.4, -20.0),
        (0.0, 12.0),
        (1.4, -20.0),
        (10.0, -20.0),
        (0.0, 20.0),
    ]
}

/// The notch tip, project space.
fn notch_tip() -> Point {
    Point::new(CENTRE.0, CENTRE.1 + 12.0)
}

/// The acute-notch design on 100 × 100 mm stock [`THICKNESS_MM`] thick.
fn notch_project() -> Project {
    project_of(vec![polygon(&notch_points())], CENTRE, 100.0, 100.0)
}

/// The cutter a run of `project` cuts with, as the heightfield models it.
fn cutter_of(project: &Project, run: &ToolRun) -> Cutter {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == run.tool)
        .expect("the run's tool is in the project");
    match tool.cutter {
        ToolKind::VBit { included_angle_deg } => Cutter::VBit {
            slope: (included_angle_deg.to_radians() / 2.0).tan(),
        },
        _ => Cutter::Flat {
            radius_mm: tool.diameter_mm / 2.0,
        },
    }
}

/// The signed distance from `p` into `design`, mm: positive inside.
fn inset_mm(design: &[Region], p: Point) -> f64 {
    let gap_mm = distance_to(&outlines(design), p);
    if design.iter().any(|r| r.contains_point(p)) {
        gap_mm
    } else {
        -gap_mm
    }
}

/// #452 (the fit property on the programs): the male's cuts reach the tip
/// of a 5° notch. For each male, with and without a clearing pass, the
/// heightfield its runs leave round the notch (0.05 mm grid) is at or
/// below `start_plane + h` wherever the mirrored design inset by `h · s`
/// excludes the point, for `h` from 0 to `D − G`: at most `start_plane +
/// max(0, e / s)` where `e` is the point's inset into the design, within
/// 0.05 mm. Before #452 spur pruning took the notch's whole medial branch
/// and left a fin of full-height wood in it.
#[test]
fn male_carves_to_the_tip_of_an_acute_notch() {
    const STEP_MM: f64 = 0.05;
    const TOLERANCE_MM: f64 = 0.05;
    let project = notch_project();
    let tip = notch_tip();
    let rise_mm = DEPTH_MM - GLUE_GAP_MM;
    // The notch from 2 mm above its tip to 16 mm below, and the legs.
    let window = Rect {
        min: Point::new(tip.x - 4.0, tip.y - 16.0),
        max: Point::new(tip.x + 4.0, tip.y + 2.0),
    };
    for clearing in [Vec::new(), vec![clearing_pass()]] {
        let op = InlayOp {
            clearing: clearing.clone(),
            ..inlay(InlaySide::Male)
        };
        let (runs, _) = inlay_runs(&project, &op);
        let male = side(
            &with_op(&project, Operation::Inlay(op.clone())),
            &op,
            &mut Vec::new(),
        )
        .expect("the male resolves");
        let mut field = Heightfield::new(window, STEP_MM, THICKNESS_MM, None);
        field.cut_runs(&runs, |run| cutter_of(&project, run));
        let (nx, ny) = field.size();
        let mut worst = (f64::NEG_INFINITY, Point::new(0.0, 0.0));
        for j in 0..ny {
            for i in 0..nx {
                let p = field.cell(i, j);
                let height_mm = (inset_mm(&male.design, p) / slope()).max(0.0);
                if height_mm >= rise_mm {
                    continue;
                }
                let over_mm = field.z_mm(i, j) - (male.start_plane_z_mm + height_mm);
                if over_mm > worst.0 {
                    worst = (over_mm, p);
                }
            }
        }
        assert!(
            worst.0 <= TOLERANCE_MM,
            "clearing {}: the male leaves {:.3} mm too much at {:?}",
            !clearing.is_empty(),
            worst.0,
            worst.1
        );
    }
}

/// How an inlay pair seats (#452), from its two sides' programs.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    /// How far the plug's start plane stops above the female's surface,
    /// mm, and the cell (female's frame) that stops it.
    seat_mm: f64,
    seat_at: Point,
    /// The male's share of it, mm: the most, over the cells that hold the
    /// plug up more than [`SEAT_MM`], by which the plug stands higher
    /// above its start plane than the pocket the female's design describes
    /// is deep there, `min(e / s, D)` for a point `e` inside the design;
    /// and that cell. Where the female's own V-Carve falls short of its
    /// design, this is what the male adds to it.
    male_mm: f64,
    male_at: Point,
}

/// How far the plug may stand proud of its female, mm: from the male's
/// own cuts (#452), and in all (#457).
const SEAT_MM: f64 = 0.05;

/// The fit of `female` (in `female_project`) and `male` (in
/// `male_project`): both sides' runs simulated as heightfields (`seat`),
/// cells `step_mm` apart, over the design's box grown by the margin and 2
/// mm, the male mirrored as it seats.
fn fit_of(
    female_project: &Project,
    female: &InlayOp,
    male_project: &Project,
    male: &InlayOp,
    step_mm: f64,
) -> Fit {
    let (female_runs, _) = inlay_runs(female_project, female);
    let (male_runs, _) = inlay_runs(male_project, male);
    let resolve = |project: &Project, op: &InlayOp| {
        side(
            &with_op(project, Operation::Inlay(op.clone())),
            op,
            &mut Vec::new(),
        )
        .expect("the side resolves")
    };
    let female_side = resolve(female_project, female);
    let male_side = resolve(male_project, male);
    // The mirror is about the middle of the design's box, so the mirrored
    // design's box is the design's.
    let mut bounds: Option<Rect> = None;
    for outline in outlines(&male_side.design) {
        let b = outline.bounds().expect("an outline has a box");
        bounds = Some(bounds.map_or(b, |all| all.including(b.min).including(b.max)));
    }
    let grow_mm = male.margin_mm.expect("a male has a margin") + 2.0;
    let b = bounds.expect("the design has an outline");
    let bounds = Rect {
        min: Point::new(b.min.x - grow_mm, b.min.y - grow_mm),
        max: Point::new(b.max.x + grow_mm, b.max.y + grow_mm),
    };
    let top_mm = |p: &Project| p.stock.thickness_mm.expect("stock has a thickness");
    let mut female_field = Heightfield::new(bounds, step_mm, top_mm(female_project), None);
    female_field.cut_runs(&female_runs, |run| cutter_of(female_project, run));
    let mut male_field =
        Heightfield::new(bounds, step_mm, top_mm(male_project), male_side.mirror_x_mm);
    male_field.cut_runs(&male_runs, |run| cutter_of(male_project, run));
    let cells = seat(&female_field, &male_field, male_side.start_plane_z_mm);
    let worst = cells
        .iter()
        .max_by(|a, b| a.need_mm.total_cmp(&b.need_mm))
        .expect("the plug has cells");
    let Cutter::VBit { slope } = cutter_of(female_project, main_run_of(&female_runs)) else {
        panic!("the female's main run is a V-bit's");
    };
    let depth_mm = female.depth_mm.expect("a depth");
    let share = |c: &SeatCell| {
        let pocket_mm = (inset_mm(&female_side.targets.regions, c.at) / slope).clamp(0.0, depth_mm);
        (c.plug_mm - pocket_mm, c.at)
    };
    // Where the plug seats within `SEAT_MM` everywhere, the male's share
    // at the cell that holds it highest.
    let male = cells
        .iter()
        .filter(|c| c.need_mm > SEAT_MM)
        .map(share)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or_else(|| share(worst));
    Fit {
        seat_mm: worst.need_mm,
        seat_at: worst.at,
        male_mm: male.0,
        male_at: male.1,
    }
}

/// The V-bit's run of `runs`.
fn main_run_of(runs: &[ToolRun]) -> &ToolRun {
    runs.iter()
        .find(|r| r.label == RunLabel::Main)
        .expect("a V-bit run")
}

/// The project and the Inlay of a tutorial fixture
/// (`tests/fixtures/dc-female.dcv`, `dc-male.dcv`; #446).
fn fixture(text: &str) -> (Project, InlayOp) {
    let project = dc_project::load(text.as_bytes())
        .expect("the fixture loads")
        .project;
    let op = project
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::Inlay(op) => Some(op.clone()),
            _ => None,
        })
        .expect("the fixture has an Inlay");
    (project, op)
}

/// #452 (the regression test): the plug seats flush. Each pair's female
/// and male are generated and simulated as heightfields (V-bit cone,
/// end-mill disc, arcs cut as chords), the male flipped about its mirror
/// line and its start plane laid on the female's surface. Where a cell
/// holds the plug up more than [`SEAT_MM`], the male may stand no more
/// than that above the pocket the female's design describes, and the
/// whole seat, the female's own V-Carve included, is within [`SEAT_MM`]
/// too (#457: before, the female's Douglas-Peucker chords ran up to a
/// quarter step off the medial axis and its groove stopped up to 0.11 mm
/// short of a wall, which held the "DC" plug 0.185 mm up). The pairs: the tutorial's "DC" (clearing, open
/// rounded band, cut-out; 0.2 mm grid), the star (no clearing, a V wall,
/// the V-bit's floor rings at the step-over the user guide asks for, 0.1;
/// 0.2 mm grid) and the acute notch with a clearing pass (0.1 mm grid).
/// Before #452 the notch's male left a fin of full-height wood in the
/// notch and stood 3 mm proud.
#[test]
fn plug_seats_flush() {
    let (dc_female, dc_female_op) =
        fixture(include_str!("../../../../tests/fixtures/dc-female.dcv"));
    let (dc_male, dc_male_op) = fixture(include_str!("../../../../tests/fixtures/dc-male.dcv"));
    let star = star_project(THICKNESS_MM);
    let fine = |side| InlayOp {
        step_over_fraction: Some(0.1),
        ..inlay(side)
    };
    let notch = notch_project();
    let with_clearing = |side| InlayOp {
        clearing: vec![clearing_pass()],
        ..inlay(side)
    };
    let pairs = [
        (
            "DC",
            fit_of(&dc_female, &dc_female_op, &dc_male, &dc_male_op, 0.2),
        ),
        (
            "star",
            fit_of(
                &star,
                &fine(InlaySide::Female),
                &star,
                &fine(InlaySide::Male),
                0.2,
            ),
        ),
        (
            "notch",
            fit_of(
                &notch,
                &with_clearing(InlaySide::Female),
                &notch,
                &with_clearing(InlaySide::Male),
                0.1,
            ),
        ),
    ];
    for (name, fit) in pairs {
        println!(
            "{name}: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
            fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
        );
        assert!(
            fit.male_mm <= SEAT_MM,
            "{name}: the male stands {:.3} mm proud of the female's design at {:?}",
            fit.male_mm,
            fit.male_at
        );
        assert!(
            fit.seat_mm <= SEAT_MM,
            "{name}: the plug stands {:.3} mm proud, binding at {:?}",
            fit.seat_mm,
            fit.seat_at
        );
    }
}
