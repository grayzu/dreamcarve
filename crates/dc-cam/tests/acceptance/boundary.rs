// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 61l to 61p: the drawn boundary of an Inlay's male
//! (`docs/specs/phase-3/design.md`, "Drawn boundary"; functional 49f).
//!
//! The star is the inlay test part's: four points 60 mm tip to tip, inner
//! vertices 12 mm out on the diagonals, a 12 mm circle in its middle,
//! centred at (50, 50) on 100 × 100 × 12 mm stock, the male board the
//! stock's size. `t1` is the bench's 60° 6.35 mm V-bit, `t2` a 3.175 mm
//! end mill; `depth_mm` 4, `glue_gap_mm` 1, `plug_depth_mm` 2, so the
//! male's start plane is 3 mm below its board's top and its floor 5 mm.
//! An 80 mm square boundary centred on the star is 10 mm from each tip.

use super::*;
use dc_cam::inlay::male_mirror;
use dc_cam::{CamError, RunLabel, ToolRun, Warning, generate_inlay_board};
use dc_geometry::{Polyline, Transform, Vertex, nearest_point};
use dc_project::test_support::{sample_project, tool_6_35_upcut};
use dc_project::{
    Board, ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InlayCutOut, InlayOp, InstanceId, MachiningOrder, MaleBoard, Operation,
    OperationId, Problem, Project, Severity, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId,
    ToolKind,
};
use std::collections::BTreeMap;

const THICKNESS_MM: f64 = 12.0;
/// The star's centre, mm.
const CENTRE: (f64, f64) = (50.0, 50.0);
/// The star's circle's radius, mm.
const CIRCLE_MM: f64 = 6.0;
/// The 3.175 mm end mill's radius, mm.
const END_MILL_RADIUS_MM: f64 = 3.175 / 2.0;
/// The male's start plane, `D − G` below its board's top, mm.
const START_PLANE_Z_MM: f64 = THICKNESS_MM - 3.0;
/// The male's floor, `D − G + P` below its board's top, mm.
const FLOOR_Z_MM: f64 = THICKNESS_MM - 5.0;

fn slope() -> f64 {
    30.0_f64.to_radians().tan()
}

fn polygon(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

/// The star about `centre`: tips on the axes 30 mm out, inner vertices on
/// the diagonals 12 mm out.
fn star(centre: (f64, f64)) -> Polyline {
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    let points: Vec<(f64, f64)> = [
        (30.0, 0.0),
        (inner, inner),
        (0.0, 30.0),
        (-inner, inner),
        (-30.0, 0.0),
        (-inner, -inner),
        (0.0, -30.0),
        (inner, -inner),
    ]
    .iter()
    .map(|&(x, y)| (x + centre.0, y + centre.1))
    .collect();
    polygon(&points)
}

/// The star's circle about `centre`.
fn circle(centre: (f64, f64)) -> Polyline {
    Polyline::circle(Point::new(centre.0, centre.1), CIRCLE_MM)
}

/// A shape drawn in project space as instance `id`.
fn instance(id: &str, name: &str) -> FeatureInstance {
    FeatureInstance {
        id: InstanceId::new(id),
        definition: DefinitionId::new(format!("d-{id}")),
        transform: Transform::IDENTITY,
        name: name.into(),
        visible: true,
        locked: false,
        construction: false,
        group: None,
        z_top_mm: None,
        z_bottom_mm: None,
    }
}

/// `project` with `profiles`, drawn in project space, as instance `id`.
fn add(project: &mut Project, id: &str, name: &str, profiles: Vec<Polyline>) {
    let placed = instance(id, name);
    project.definitions.insert(
        placed.definition.clone(),
        FeatureDefinition {
            id: placed.definition.clone(),
            name: name.into(),
            shape: None,
            profiles,
        },
    );
    project.instances.push(placed);
}

/// A `width_mm` × 100 × 12 mm project with a star and its circle about
/// each of `centres` as instances `i1`, `i2`, …, the tools `t1` (the 60°
/// V-bit) and `t2` (the end mill), and no operations.
fn star_project(width_mm: f64, centres: &[(f64, f64)]) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm,
        height_mm: 100.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    p.definitions = BTreeMap::new();
    p.instances = Vec::new();
    for (k, &centre) in centres.iter().enumerate() {
        let id = format!("i{}", k + 1);
        add(&mut p, &id, "Star", vec![star(centre), circle(centre)]);
    }
    p.tools = vec![
        Tool {
            id: ToolId::new("t1"),
            name: "60° V-bit".into(),
            cutter: ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            ..tool_6_35_upcut()
        },
        Tool {
            id: ToolId::new("t2"),
            name: "3.175 mm end mill".into(),
            diameter_mm: 3.175,
            ..tool_6_35_upcut()
        },
    ];
    p.operations.clear();
    p
}

/// The star part's Inlay `o1` of every star of `project` with `t1`, its
/// male board measured, the boundary `boundary` in place of a margin; with
/// `clearing`, the end mill's clearing pass (offset, climb, plunging), and
/// with `cut_out`, a cut-out by the end mill with four rect tabs 6 mm wide
/// and 3 mm tall, 0.2 mm through.
fn male(project: &Project, boundary: &[&str], clearing: bool, cut_out: bool) -> InlayOp {
    InlayOp {
        id: OperationId::new("o1"),
        name: "Star".into(),
        enabled: true,
        targets: project
            .instances
            .iter()
            .filter(|i| i.name == "Star")
            .map(|i| i.id.clone())
            .collect(),
        tool: ToolId::new("t1"),
        male_board: MaleBoard {
            width_mm: None,
            height_mm: None,
            thickness_mm: Some(THICKNESS_MM),
        },
        depth_mm: Some(4.0),
        glue_gap_mm: Some(1.0),
        plug_depth_mm: Some(2.0),
        margin_mm: None,
        outline_rounding_mm: None,
        boundary: boundary.iter().map(|id| InstanceId::new(*id)).collect(),
        clearing: if clearing {
            vec![ClearingPass {
                tool: ToolId::new("t2"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }]
        } else {
            Vec::new()
        },
        step_over_fraction: None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 10.0,
        cut_out: cut_out.then(|| InlayCutOut {
            tool: ToolId::new("t2"),
            tabs: Some(TabSpec {
                placement: TabPlacement::Count { count: 4 },
                width_mm: 6.0,
                height_mm: 3.0,
                shape: TabShape::Rect,
                on_holes: false,
                avoid_corners: true,
            }),
            through_cut_allowance_mm: 0.2,
        }),
    }
}

/// A square from `low_mm` to `high_mm` on both axes.
fn square(low_mm: f64, high_mm: f64) -> Polyline {
    Polyline::rect(
        Point::new(low_mm, low_mm),
        high_mm - low_mm,
        high_mm - low_mm,
    )
}

/// Acceptance 61l's project: the star with the 80 mm square `b1` as the
/// boundary of its male.
fn star_in_square(clearing: bool, cut_out: bool) -> (Project, InlayOp) {
    let mut project = star_project(100.0, &[CENTRE]);
    add(&mut project, "b1", "Boundary", vec![square(10.0, 90.0)]);
    let op = male(&project, &["b1"], clearing, cut_out);
    (project, op)
}

/// The male's runs and warnings for `op` in `project`, which gains it.
fn male_runs(project: &Project, op: &InlayOp) -> (Vec<ToolRun>, Vec<Warning>) {
    let mut p = project.clone();
    p.operations = vec![Operation::Inlay(op.clone())];
    generate_inlay_board(&p, op, Board::Male)
        .unwrap_or_else(|e| panic!("the male generates: {e}\n{e:?}"))
}

/// The run of `runs` that `wanted` picks.
fn run_of<'r>(runs: &'r [ToolRun], what: &str, wanted: impl Fn(&RunLabel) -> bool) -> &'r Toolpath {
    &runs
        .iter()
        .find(|r| wanted(&r.label))
        .unwrap_or_else(|| panic!("the male has {what}"))
        .toolpath
}

/// The Chebyshev distance of `p` from `centre`, mm: 40 on an 80 mm square
/// about it.
fn reach(p: Point, centre: (f64, f64)) -> f64 {
    (p.x - centre.0).abs().max((p.y - centre.1).abs())
}

/// The distance from `p` to the nearest of `outlines`, following arcs, mm.
fn distance_to(outlines: &[Polyline], p: Point) -> f64 {
    outlines
        .iter()
        .filter_map(|o| nearest_point(o, p))
        .map(|n| n.gap_mm)
        .fold(f64::INFINITY, f64::min)
}

/// The cuts of `toolpath` that run level, as `(z, from, to)`.
fn level_cuts(toolpath: &Toolpath) -> Vec<(f64, Point, Point)> {
    cuts(toolpath)
        .into_iter()
        .filter(|(from, mv)| (from.z - mv.to.z).abs() < 1e-9)
        .map(|(from, mv)| (mv.to.z, from.xy(), mv.to.xy()))
        .collect()
}

/// Acceptance 61l, "its walls vertical": with `clearing: [t2]` the end
/// mill cuts the band's edge on the square upright. At every level it
/// cuts, down to the floor, its outermost path is the same one, the
/// square inset by its radius, so the wall it leaves stands on the square
/// from the board's top to the floor.
#[test]
fn boundary_edge_walls_are_vertical() {
    let (project, op) = star_in_square(true, false);
    let (runs, _) = male_runs(&project, &op);
    let clearing = run_of(&runs, "a clearing run", |l| {
        matches!(l, RunLabel::Clearing(_))
    });
    let mut levels: Vec<(f64, f64)> = Vec::new();
    for (z, a, b) in level_cuts(clearing) {
        let far = reach(a, CENTRE).max(reach(b, CENTRE));
        match levels
            .iter_mut()
            .find(|(level, _)| (*level - z).abs() < 1e-6)
        {
            Some((_, most)) => *most = most.max(far),
            None => levels.push((z, far)),
        }
    }
    assert!(levels.len() >= 2, "the pass steps down: {levels:?}");
    assert!(
        levels.iter().any(|(z, _)| (z - FLOOR_Z_MM).abs() < 1e-6),
        "the pass reaches the floor: {levels:?}"
    );
    for (z, far) in levels {
        assert!(
            (far - (40.0 - END_MILL_RADIUS_MM)).abs() < 0.01,
            "at Z {z} the end mill's outermost path is {far} mm out, not the square less its radius"
        );
    }
}

/// Acceptance 61l, "the square's corners are cut with that radius": the
/// end mill's centre runs into each corner of the square inset by its
/// radius, a sharp corner of its path, so the wall's corner is the round
/// of the end mill, 1.5875 mm.
#[test]
fn boundary_corners_are_cut_with_the_end_mills_radius() {
    let (project, op) = star_in_square(true, false);
    let (runs, _) = male_runs(&project, &op);
    let clearing = run_of(&runs, "a clearing run", |l| {
        matches!(l, RunLabel::Clearing(_))
    });
    let at_floor: Vec<Point> = cut_samples(clearing, 0.5)
        .into_iter()
        .filter(|p| (p.z - FLOOR_Z_MM).abs() < 1e-6)
        .map(|p| p.xy())
        .collect();
    let inset = 40.0 - END_MILL_RADIUS_MM;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = Point::new(CENTRE.0 + sx * inset, CENTRE.1 + sy * inset);
        let nearest = at_floor
            .iter()
            .map(|p| p.distance_to(corner))
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < 0.01,
            "the end mill's centre passes {nearest} mm from the inset corner {corner:?}"
        );
        // Its edge is then its radius from both sides of the square's
        // corner and `r(√2 − 1)` short of the corner itself.
        let square_corner = Point::new(CENTRE.0 + sx * 40.0, CENTRE.1 + sy * 40.0);
        let left = at_floor
            .iter()
            .map(|p| p.distance_to(square_corner) - END_MILL_RADIUS_MM)
            .fold(f64::INFINITY, f64::min);
        let want = END_MILL_RADIUS_MM * (std::f64::consts::SQRT_2 - 1.0);
        assert!(
            (left - want).abs() < 0.01,
            "{left} mm is left in the corner {square_corner:?}, not {want}"
        );
    }
}

/// Acceptance 61l: with the edge open, no V-bit cut comes within its
/// cone's radius at the male board's top of the square: `(D − G + P) ·
/// tan 30°` = 2.887 mm, so the V-bit's flank never reaches the wall the
/// end mill cut.
#[test]
fn boundary_open_edge_keeps_the_v_bit_its_cone_radius_inside() {
    let cone_radius_mm = 5.0 * slope();
    assert!((cone_radius_mm - 2.887).abs() < 0.001, "{cone_radius_mm}");
    let (project, op) = star_in_square(true, false);
    let (runs, _) = male_runs(&project, &op);
    let v_bit = run_of(&runs, "a V-bit run", |l| *l == RunLabel::Main);
    let samples = cut_samples(v_bit, 0.25);
    assert!(samples.len() > 100, "the V-bit cuts");
    for p in samples {
        let room = 40.0 - reach(p.xy(), CENTRE);
        assert!(
            room >= cone_radius_mm - 0.01,
            "the V-bit cuts at {p:?}, {room} mm from the square"
        );
    }
}

/// Acceptance 61m: the "DC" project with its 90 × 60 mm frame, moved 3 mm
/// right, as the boundary. As drawn the letters stand about 5.3 mm from
/// the frame on the left and 11.3 mm on the right; on the male, mirrored
/// about X = 55 with the frame, about 11.3 mm on the left and 5.3 mm on
/// the right. The band's edge is the one the generator and the safety
/// pass both read (`dc_project::band_edge`).
#[test]
fn boundary_rooms_swap_sides_on_the_male() {
    let mut project = dc_project::load(include_bytes!("../../../../tests/fixtures/dc.dcv"))
        .expect("the fixture loads")
        .project;
    let frame = project
        .instances
        .iter_mut()
        .find(|i| i.id.as_str() == "i3")
        .expect("the frame");
    frame.transform = frame.transform.then(Transform::translate(3.0, 0.0));
    let Some(Operation::Inlay(op)) = project.operations.first_mut() else {
        panic!("the fixture's one operation is its Inlay");
    };
    op.boundary = vec![InstanceId::new("i3")];
    op.margin_mm = None;
    op.outline_rounding_mm = None;
    let op = op.clone();

    let span = |outlines: &[Polyline]| {
        outlines
            .iter()
            .filter_map(Polyline::bounds)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| {
                (lo.min(b.min.x), hi.max(b.max.x))
            })
    };
    let placed = |id: &InstanceId| -> Vec<Polyline> {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == id)
            .expect("an instance");
        project.definitions[&instance.definition]
            .profiles
            .iter()
            .map(|p| instance.transform.apply_polyline(p).expect("placed"))
            .collect()
    };
    let letters: Vec<Polyline> = op.targets.iter().flat_map(&placed).collect();
    let (letters_min, letters_max) = span(&letters);
    let (frame_min, frame_max) = span(&placed(&InstanceId::new("i3")));
    let (left, right) = (letters_min - frame_min, frame_max - letters_max);
    assert!((left - 5.3).abs() < 0.1, "{left} mm on the left as drawn");
    assert!(
        (right - 11.3).abs() < 0.1,
        "{right} mm on the right as drawn"
    );

    let mirror = male_mirror(&project, &op.targets).expect("a mirror");
    let axis_x_mm = mirror.apply(Point::new(0.0, 0.0)).x / 2.0;
    assert!((axis_x_mm - 55.0).abs() < 0.01, "{axis_x_mm}");
    let edge: Vec<Polyline> = dc_project::band_edge(&project, &op, Some(mirror), &[])
        .expect("the band resolves")
        .iter()
        .map(|r| r.outer().polyline().clone())
        .collect();
    let (edge_min, edge_max) = span(&edge);
    let mirrored: Vec<Polyline> = letters
        .iter()
        .map(|l| mirror.apply_polyline(l).expect("mirrored"))
        .collect();
    let (mirrored_min, mirrored_max) = span(&mirrored);
    let (male_left, male_right) = (mirrored_min - edge_min, edge_max - mirrored_max);
    assert!(
        (male_left - right).abs() < 0.02 && (male_right - left).abs() < 0.02,
        "on the male {male_left} mm on the left and {male_right} on the right; drawn {left} and {right}"
    );
}

/// Acceptance 61n: a boundary shape with a hole round the star's circle
/// warns `inlay_boundary_hole_ignored` and its hole is not read: the male
/// is cut move for move as with the plain square, so the band runs
/// through the hole, and the inside of the star's circle, which the hole
/// would have taken out of the band, is still carved.
#[test]
fn band_is_cut_through_an_ignored_boundary_hole() {
    let (plain, plain_op) = star_in_square(true, false);
    let (want, plain_warnings) = male_runs(&plain, &plain_op);
    assert!(
        !plain_warnings
            .iter()
            .any(|w| matches!(w, Warning::InlayBoundaryHoleIgnored { .. })),
        "{plain_warnings:?}"
    );

    let mut holed = star_project(100.0, &[CENTRE]);
    add(
        &mut holed,
        "b1",
        "Boundary",
        vec![
            square(10.0, 90.0),
            Polyline::circle(Point::new(CENTRE.0, CENTRE.1), 8.0),
        ],
    );
    let op = male(&holed, &["b1"], true, false);
    let (runs, warnings) = male_runs(&holed, &op);
    let ignored = warnings
        .iter()
        .filter(|w| matches!(w, Warning::InlayBoundaryHoleIgnored { .. }))
        .count();
    assert_eq!(ignored, 1, "{warnings:?}");
    assert_eq!(runs, want, "the hole changes no move");
    let v_bit = run_of(&runs, "a V-bit run", |l| *l == RunLabel::Main);
    let centre = Point::new(CENTRE.0, CENTRE.1);
    assert!(
        cut_samples(v_bit, 0.25)
            .iter()
            .any(|p| p.xy().distance_to(centre) < CIRCLE_MM && p.z < START_PLANE_Z_MM),
        "the star's circle is carved inside the ignored hole"
    );
}

/// Acceptance 61n: a boundary smaller than the star, a 40 mm square
/// centred on it, is `inlay_boundary_crosses_design`, an error at the
/// boundary list. Its message names where the design lies farthest past
/// the boundary, a tip of the star, 10 mm beyond the square's side, and
/// the generator refuses the male for the same reason.
#[test]
fn boundary_smaller_than_the_star_crosses_the_design() {
    let mut project = star_project(100.0, &[CENTRE]);
    add(&mut project, "b1", "Boundary", vec![square(30.0, 70.0)]);
    let op = male(&project, &["b1"], false, false);
    project.operations = vec![Operation::Inlay(op.clone())];

    let crossing: Vec<Problem> = dc_project::validate(&project)
        .into_iter()
        .filter(|p| p.code().starts_with("inlay_boundary"))
        .collect();
    let [
        problem @ Problem::InlayBoundaryCrossesDesign {
            x_mm,
            y_mm,
            distance_mm,
            ..
        },
    ] = crossing.as_slice()
    else {
        panic!("one boundary problem, the crossing: {crossing:?}");
    };
    assert_eq!(problem.code(), "inlay_boundary_crosses_design");
    assert_eq!(problem.severity(), Severity::Error);
    assert_eq!(problem.field().as_deref(), Some("operations[o1].boundary"));
    assert!((distance_mm - 10.0).abs() < 0.01, "{distance_mm}");
    let tip = Point::new(*x_mm, *y_mm);
    assert!(
        [(30.0, 0.0), (0.0, 30.0), (-30.0, 0.0), (0.0, -30.0)]
            .iter()
            .any(|&(dx, dy)| tip.distance_to(Point::new(CENTRE.0 + dx, CENTRE.1 + dy)) < 0.01),
        "{tip:?} is no tip of the star"
    );
    assert!(problem.to_string().contains("10.000"), "{problem}");

    let refused = generate_inlay_board(&project, &op, Board::Male);
    assert!(
        matches!(
            refused,
            Err(CamError::InlayBoundary {
                problem: dc_project::BoundaryProblem::CrossesDesign { .. },
                ..
            })
        ),
        "{refused:?}"
    );
}

/// Acceptance 61o: two stars 120 mm apart, each in its own 80 mm square.
/// The male's band is the two squares less the stars: every V-bit cut's
/// cone at the start plane lies inside one of the squares and outside
/// both stars but for their circles, both squares are cut, and nothing is
/// cut between them. (The mirror is about X = 150, half way between the
/// stars, so the mirrored squares and stars are the drawn ones.)
#[test]
fn two_boundaries_band_is_the_two_squares_less_the_stars() {
    let centres = [(90.0, 50.0), (210.0, 50.0)];
    let mut project = star_project(300.0, &centres);
    add(
        &mut project,
        "b1",
        "Boundary 1",
        vec![Polyline::rect(Point::new(50.0, 10.0), 80.0, 80.0)],
    );
    add(
        &mut project,
        "b2",
        "Boundary 2",
        vec![Polyline::rect(Point::new(170.0, 10.0), 80.0, 80.0)],
    );
    let op = male(&project, &["b1", "b2"], false, false);
    let mirror = male_mirror(&project, &op.targets).expect("a mirror");
    assert!((mirror.apply(Point::new(0.0, 0.0)).x / 2.0 - 150.0).abs() < 0.01);

    let (runs, _) = male_runs(&project, &op);
    let v_bit = run_of(&runs, "a V-bit run", |l| *l == RunLabel::Main);
    let walls: Vec<Polyline> = centres.iter().flat_map(|&c| [star(c), circle(c)]).collect();
    let mut cut_in = [0_usize; 2];
    for p in cut_samples(v_bit, 0.25) {
        assert!(p.z >= FLOOR_Z_MM - 1e-6, "{p:?} is below the floor");
        let r = (START_PLANE_Z_MM - p.z).max(0.0) * slope();
        let (k, centre) = if p.x < 150.0 {
            (0, centres[0])
        } else {
            (1, centres[1])
        };
        cut_in[k] += 1;
        assert!(
            reach(p.xy(), centre) + r <= 40.0 + 0.03,
            "a cone at {p:?} reaches past its square"
        );
        assert!(
            distance_to(&walls, p.xy()) >= r - 0.03,
            "a cone at {p:?} reaches into a star"
        );
    }
    assert!(
        cut_in[0] > 100 && cut_in[1] > 100,
        "both squares are cut: {cut_in:?}"
    );
}

/// Acceptance 61p: with the square boundary, `clearing: [t2]` and a
/// cut-out by t2 with four tabs, the freed plug's outline is the square
/// inset by the end mill's diameter, 3.175 mm: every point of that
/// outline, its corners too, is the end mill's radius from the cut-out's
/// tool-centre path, within 0.01 mm, so the cutter's edge runs along it
/// and nowhere cuts inside it.
#[test]
fn boundary_cut_out_frees_the_square_inset_by_the_end_mills_diameter() {
    let (project, op) = star_in_square(true, true);
    let (runs, _) = male_runs(&project, &op);
    let cut_out = run_of(&runs, "a cut-out run", |l| matches!(l, RunLabel::CutOut(_)));
    let path: Vec<Point> = cut_samples(cut_out, 0.02)
        .into_iter()
        .map(|p| p.xy())
        .collect();
    assert!(path.len() > 1000, "the cut-out cuts");
    let half = 40.0 - 2.0 * END_MILL_RADIUS_MM;
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    for (k, &(ax, ay)) in corners.iter().enumerate() {
        let (bx, by) = corners[(k + 1) % 4];
        // From one corner of the plug's outline to the next, 0.5 mm apart.
        for step in 0..=147 {
            let t = f64::from(step) / 147.0;
            let q = Point::new(
                CENTRE.0 + half * (ax + (bx - ax) * t),
                CENTRE.1 + half * (ay + (by - ay) * t),
            );
            let nearest = path
                .iter()
                .map(|p| p.distance_to(q))
                .fold(f64::INFINITY, f64::min);
            assert!(
                (nearest - END_MILL_RADIUS_MM).abs() < 0.01,
                "the plug's outline at {q:?} is {nearest} mm from the cut-out's path"
            );
        }
    }
}
