// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17 in the safety pass (#408; `docs/specs/phase-3/design.md`,
//! "Safety pass", the S17 and clearing-run S9 bullets; functional S17;
//! security review H3, H4, M1 and M3).
//!
//! The hand-written programs carve the sample rectangle (X 10 to 60, Y 10
//! to 40, 6 mm stock, stock-top origin, so machine Z is project Z less 6)
//! with the 90° 6.35 mm V-bit `t2` (slope 1, cone 3.175 mm) unless a test
//! says otherwise. The generated ones cut every V-Carve shape the
//! generator's own tests use, with every strategy, floor and clearing
//! setting, through `emit` and so through the whole safety pass.

use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use crate::parse::{CycleModel, parse_program_with};
use dc_cam::RunLabel;
use dc_geometry::{Polyline, Transform, Vertex};
use dc_project::test_support::{sample_pocket, sample_v_carve, tool_6_35_upcut, tool_v_bit_90};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InstanceId, OperationId, PocketOp, Stock, ToolId, ToolKind, VCarveOp,
    VCarveStrategy,
};
use std::collections::BTreeMap;

const DATE: &str = "2026-10-28";

/// A flat end mill `id` of `diameter_mm`, every rate set, stepping down
/// 1 mm at a 0.4 step-over.
fn end_mill(id: &str, diameter_mm: f64) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: format!("{diameter_mm} mm end mill"),
        diameter_mm,
        step_down_mm: Some(1.0),
        step_over_fraction: Some(0.4),
        ..tool_6_35_upcut()
    }
}

/// A 6.35 mm V-bit `id` of `angle_deg`, every rate set.
fn v_bit(id: &str, angle_deg: f64) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: format!("{angle_deg}° V-bit"),
        cutter: ToolKind::VBit {
            included_angle_deg: angle_deg,
        },
        ..tool_v_bit_90()
    }
}

/// The sample project with the V-bit `t2` and the end mills `t3`
/// (3.175 mm) and `t4` (1.5 mm), and `ops` as its operations.
fn sample_with(ops: Vec<Operation>) -> Project {
    let mut project = sample_project();
    project.tools.push(tool_v_bit_90());
    project.tools.push(end_mill("t3", 3.175));
    project.tools.push(end_mill("t4", 1.5));
    project.operations = ops;
    project
}

/// The sample V-Carve `o6` of the rectangle, edited by `edit`.
fn carve_with(edit: impl FnOnce(&mut VCarveOp)) -> VCarveOp {
    let mut op = sample_v_carve();
    edit(&mut op);
    op
}

/// The project's tool `id`.
fn tool_of(project: &Project, id: &str) -> Tool {
    project
        .tools
        .iter()
        .find(|t| t.id.as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("no tool {id}"))
}

/// S9 and S17 alone on hand-written `text`, as the run `label` of `op`
/// cut with `tool`.
fn check_run(
    text: &str,
    project: &Project,
    op: &Operation,
    label: &RunLabel,
    tool: &Tool,
) -> Result<(), ExportError> {
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(project, &post).unwrap();
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let material = Material::of_run(project, op, label, tool);
    let tabs = material::tab_spines(project, op);
    let toolpath = Toolpath::default();
    let span = OpSpan {
        op,
        tool,
        toolpath: &toolpath,
        material: &material,
        tabs: &tabs,
        lines: 1..text.lines().count() + 1,
    };
    let tolerances = Tolerances {
        flatten_mm: project.flatten_tolerance_mm,
        arc_fit_mm: project.arc_fit_tolerance_mm,
    };
    material::check(&blocks, &post, &transform, tolerances, &span)
}

/// [`check_run`] for the V-bit's own run of carve `op` of `project`.
fn check_carve(text: &str, project: &Project, op: &VCarveOp) -> Result<(), ExportError> {
    let tool = tool_of(project, op.tool.as_str());
    check_run(
        text,
        project,
        &Operation::VCarve(op.clone()),
        &RunLabel::Main,
        &tool,
    )
}

/// A program that rapids to (`x`, `y`) at Z 5, down to Z 1 (the stock
/// top plus the entry clearance), runs `cuts` and lifts.
fn program_at(x: f64, y: f64, cuts: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG0 Z1\n{cuts}G0 Z5\n")
}

/// [`program_at`] from (20, 25).
fn program(cuts: &str) -> String {
    program_at(20.0, 25.0, cuts)
}

/// Asserts `result` is [`ExportError::ConeOffMaterial`] at `line`, with
/// an overlap within 0.01 mm of `overlap_mm` when one is given.
#[track_caller]
fn assert_cone_off(result: Result<(), ExportError>, line: usize, overlap_mm: Option<f64>) {
    match result {
        Err(ExportError::ConeOffMaterial {
            line: l,
            overlap_mm: o,
            ..
        }) if l == line && overlap_mm.is_none_or(|w| (o - w).abs() <= 0.01) => {}
        other => panic!("expected ConeOffMaterial at line {line} ({overlap_mm:?}): {other:?}"),
    }
}

/// Asserts `result` is [`ExportError::ConeHeightExceeded`] at `line` with
/// the engagement `engaged_mm` and the 90° bit's 3.175 mm cone.
#[track_caller]
fn assert_shank(result: Result<(), ExportError>, line: usize, engaged_mm: f64) {
    match result {
        Err(ExportError::ConeHeightExceeded {
            line: l,
            engaged_mm: e,
            cone_mm,
            ..
        }) if l == line && (e - engaged_mm).abs() <= 1e-6 && (cone_mm - 3.175).abs() <= 1e-9 => {}
        other => panic!("expected ConeHeightExceeded at line {line} ({engaged_mm}): {other:?}"),
    }
}

/// S17, the cone: a level cut 2 mm deep whose 2 mm disc touches the wall
/// at X 60 passes; one 1 mm nearer cuts 1 mm through it. A sloping cut
/// whose shallow end is far from the wall and whose deep end is past it
/// fails by its deep end's overlap, which the shallower-end reading of
/// #404 let through.
#[test]
fn cone_through_a_wall_fails_s17() {
    let op = sample_v_carve();
    let project = sample_with(vec![Operation::VCarve(op.clone())]);
    check_carve(&program("G1 Z-2 F300\nX58 F1000\n"), &project, &op).unwrap();
    assert_cone_off(
        check_carve(&program("G1 Z-2 F300\nX59 F1000\n"), &project, &op),
        7,
        Some(1.0),
    );
    check_carve(&program("G1 Z-0.5 F300\nX58 Z-2 F1000\n"), &project, &op).unwrap();
    assert_cone_off(
        check_carve(&program("G1 Z-0.5 F300\nX58.5 Z-2 F1000\n"), &project, &op),
        7,
        Some(0.5),
    );
}

/// S17, the depth range: with a 1 mm flat depth a cut at the floor
/// passes and one half a millimetre below it fails by that much.
#[test]
fn tip_below_the_flat_depth_fails_s17() {
    let op = carve_with(|op| {
        op.flat_depth_mm = Some(1.0);
        op.step_over_fraction = Some(0.4);
    });
    let project = sample_with(vec![Operation::VCarve(op.clone())]);
    check_carve(&program("G1 Z-1 F300\nX30 F1000\n"), &project, &op).unwrap();
    assert_cone_off(
        check_carve(&program("G1 Z-1 F300\nX30 Z-1.5 F1000\n"), &project, &op),
        7,
        Some(0.5),
    );
}

/// S17, the shank: in the middle of the rectangle, 15 mm from every wall,
/// a cut 3.17 mm deep fits the 90° bit's 3.175 mm cone and one 3.5 mm
/// deep does not: its shank would cut.
#[test]
fn tip_below_the_cone_height_fails_s17() {
    let op = sample_v_carve();
    let project = sample_with(vec![Operation::VCarve(op.clone())]);
    check_carve(&program("G1 Z-3.17 F300\nX40 F1000\n"), &project, &op).unwrap();
    assert_shank(
        check_carve(&program("G1 Z-3.17 F300\nX40 Z-3.5 F1000\n"), &project, &op),
        7,
        3.5,
    );
}

/// The sample V-Carve 2 mm below the stock top (start plane machine Z
/// −2), with the sample Pocket of the same rectangle 2 mm deep before it,
/// which clears the start plane.
fn cleared_at_2mm() -> (Project, VCarveOp) {
    let op = carve_with(|op| op.start_depth_mm = 2.0);
    let pocket = PocketOp {
        depth_mm: Some(2.0),
        ..sample_pocket()
    };
    let project = sample_with(vec![
        Operation::Pocket(pocket),
        Operation::VCarve(op.clone()),
    ]);
    (project, op)
}

/// H3: a straight descent from the stock top plus the entry clearance,
/// through the air above the start plane to a cut 1 mm below it, passes;
/// so does one that stops above the start plane over the carve. A descent
/// outside the rectangle, even one that stops above the start plane,
/// plunges into stock the Pocket left standing and fails; so does one
/// whose bottom puts the cone through a wall (the H1 plunge, deeper than
/// the cut it enters).
#[test]
fn vertical_descent_above_the_start_plane_passes_s17() {
    let (project, op) = cleared_at_2mm();
    check_carve(&program("G1 Z-3 F300\nX30 F1000\n"), &project, &op).unwrap();
    check_carve(&program("G1 Z-1.5 F300\n"), &project, &op).unwrap();
    assert_cone_off(
        check_carve(&program_at(5.0, 25.0, "G1 Z-1.5 F300\n"), &project, &op),
        6,
        None,
    );
    assert_cone_off(
        check_carve(
            &program_at(11.0, 25.0, "G1 Z-4 F300\nZ-2\nX20 Z-3 F1000\n"),
            &project,
            &op,
        ),
        6,
        Some(1.0),
    );
}

/// H3: the only cut above the start plane is a straight descent. A cut
/// that moves sideways 1 mm above the cleared start plane fails by that
/// height; so does one 0.5 mm above the stock top of a carve from the
/// top.
#[test]
fn a_sideways_cut_above_the_start_plane_fails_s17() {
    let (project, op) = cleared_at_2mm();
    assert_cone_off(
        check_carve(&program("G1 Z-1 F300\nX30 F1000\n"), &project, &op),
        7,
        Some(1.0),
    );
    let op = sample_v_carve();
    let project = sample_with(vec![Operation::VCarve(op.clone())]);
    assert_cone_off(
        check_carve(&program("G1 Z0.5 F300\nX30 F1000\n"), &project, &op),
        7,
        Some(0.5),
    );
}

/// The sample project with the rectangle's definition given `islands`
/// (definition space, the rectangle's corner at the origin), and `op`.
fn with_islands(islands: Vec<Polyline>, op: &VCarveOp) -> Project {
    let mut project = sample_with(vec![Operation::VCarve(op.clone())]);
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles.extend(islands);
    }
    project
}

fn polygon(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

/// M1: the cone test is exact, not sampled. A cut sloping from 0.5 mm
/// deep at (20, 20) to 2.5 mm at (40, 20) passes under a triangular
/// island's corner at (30.5, 21.5): read every 1 mm along the cut, the
/// least room is −0.019 mm at X 31, inside the slack (0.021 mm), but
/// between those samples it is −0.0575 mm at X 30.65, and the cut fails.
/// A level cut 0.1 mm deep that crosses a 0.5 mm island, its ends and
/// middle clear of it, fails too.
#[test]
fn cone_test_is_exact_between_vertices() {
    let op = sample_v_carve();
    let project = with_islands(
        vec![
            polygon(&[(20.5, 11.5), (23.0, 17.0), (18.0, 17.0)]),
            polygon(&[(23.0, 19.75), (23.5, 19.75), (23.5, 20.25), (23.0, 20.25)]),
        ],
        &op,
    );
    let sloping = program_at(20.0, 20.0, "G1 Z-0.5 F300\nX40 Z-2.5 F1000\n");
    assert_cone_off(check_carve(&sloping, &project, &op), 7, Some(0.0575));
    // The same cut 0.2 mm farther from the corner passes.
    let clear = program_at(20.0, 19.8, "G1 Z-0.5 F300\nX40 Z-2.5 F1000\n");
    check_carve(&clear, &project, &op).unwrap();
    let crossing = program_at(20.0, 30.0, "G1 Z-0.1 F300\nX40 F1000\n");
    assert_cone_off(check_carve(&crossing, &project, &op), 7, None);
}

/// M1: a cut at constant Z is measured segment against segment, arcs as
/// arcs. In a circle of radius 10 about (35, 25), a ring of radius 7 at
/// 3 mm deep, written as two G3 half circles, touches the wall with its
/// 3 mm disc and passes; the same ring as 36 chords lies inside it and
/// passes. A G3 arc about (36.5, 25), radius 6, from −90° to +30°, is
/// 7.34 mm from the circle's centre at its end and its middle, but its
/// bulge reaches 7.5 mm at 0°: it fails by 0.5 mm, its overlap between
/// those points.
#[test]
fn constant_z_rings_take_the_segment_path() {
    let op = carve_with(|op| op.targets = vec![InstanceId::new("i2")]);
    let mut project = sample_with(vec![Operation::VCarve(op.clone())]);
    let circle = FeatureDefinition {
        id: DefinitionId::new("d2"),
        name: "Circle".into(),
        profiles: vec![Polyline::circle(Point::new(0.0, 0.0), 10.0)],
    };
    project.definitions.insert(circle.id.clone(), circle);
    project.instances.push(FeatureInstance {
        id: InstanceId::new("i2"),
        definition: DefinitionId::new("d2"),
        transform: Transform::translate(35.0, 25.0),
        name: "Circle".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    let ring = program_at(
        42.0,
        25.0,
        "G1 Z-3 F300\nG3 X28 Y25 I-7 J0 F1000\nG3 X42 Y25 I7 J0\n",
    );
    check_carve(&ring, &project, &op).unwrap();
    let chords: String = (1..=36)
        .map(|k| {
            let a = f64::from(k) * std::f64::consts::TAU / 36.0;
            format!(
                "X{:.4} Y{:.4}\n",
                35.0 + 7.0 * a.cos(),
                25.0 + 7.0 * a.sin()
            )
        })
        .collect();
    let chord_ring = program_at(42.0, 25.0, &format!("G1 Z-3 F300\nF1000\n{chords}"));
    check_carve(&chord_ring, &project, &op).unwrap();
    let x = 36.5 + 6.0 * 30f64.to_radians().cos();
    let bulge = program_at(
        36.5,
        19.0,
        &format!("G1 Z-3 F300\nG3 X{x:.4} Y28 I0 J6 F1000\n"),
    );
    assert_cone_off(check_carve(&bulge, &project, &op), 7, Some(0.5));
}

/// The sample V-Carve 4 mm below the stock top (start plane machine Z
/// −4), `cleared_externally` as the user says.
fn deep_start(cleared_externally: bool) -> VCarveOp {
    carve_with(|op| {
        op.start_depth_mm = 4.0;
        op.start_plane_cleared_externally = cleared_externally;
    })
}

/// [`sample_with`] `ops` on 12 mm stock, so a cut 4 mm and more below the
/// stock top stays above its bottom.
fn thick_with(ops: Vec<Operation>) -> Project {
    let mut project = sample_with(ops);
    project.stock.thickness_mm = Some(12.0);
    project
}

/// H4: a carve 4 mm down that the user says another program cleared is
/// engaged from the stock top, so a cut 0.5 mm below its start plane
/// engages 4.5 mm of the 90° bit's 3.175 mm cone: the plunge to it fails
/// on the shank.
#[test]
fn shank_engagement_from_the_stock_top_fails_s17() {
    let op = deep_start(true);
    let project = thick_with(vec![Operation::VCarve(op.clone())]);
    assert_shank(
        check_carve(&program("G1 Z-4.5 F300\nX30 F1000\n"), &project, &op),
        6,
        4.5,
    );
}

/// H4: with an enabled earlier Pocket 4 mm deep over the rectangle, the
/// same carve is engaged from its start plane: the cut 0.5 mm below it
/// passes, and one 3.5 mm below it still fails on the shank.
#[test]
fn shank_engagement_from_a_cleared_start_plane_passes() {
    let op = deep_start(false);
    let pocket = PocketOp {
        depth_mm: Some(4.0),
        ..sample_pocket()
    };
    let project = thick_with(vec![
        Operation::Pocket(pocket),
        Operation::VCarve(op.clone()),
    ]);
    check_carve(&program("G1 Z-4.5 F300\nX30 F1000\n"), &project, &op).unwrap();
    assert_shank(
        check_carve(&program("G1 Z-4.5 F300\nX30 Z-7.5 F1000\n"), &project, &op),
        7,
        3.5,
    );
}

/// M3: a carve's clearing run is held to the floor re-derived from the
/// project. The sample rectangle with a 2 mm flat depth and the 90° bit
/// has the floor X 12 to 58, Y 12 to 38 at machine Z −2. The 3.175 mm pass
/// `t3` may run its centre to X 56.4125 and no farther, and not below the
/// floor. The 1.5 mm rest pass `t4` after it runs its footprint through
/// what `t3` cleared, as a rest pass does, and passes. A run labelled with
/// a tool the carve has no pass for, or written with another tool than its
/// label's, is refused.
#[test]
fn clearing_run_is_held_to_the_rederived_floor() {
    let pass = |tool: &str| ClearingPass {
        tool: ToolId::new(tool),
        strategy: ClearanceStrategy::Offset,
        direction: CutDirection::Climb,
        entry: Entry::Plunge,
    };
    let op = carve_with(|op| {
        op.flat_depth_mm = Some(2.0);
        op.step_over_fraction = Some(0.4);
        op.clearing = vec![pass("t3"), pass("t4")];
    });
    let project = sample_with(vec![Operation::VCarve(op.clone())]);
    let carve = Operation::VCarve(op.clone());
    let run = |text: &str, label: &str, tool: &str| {
        check_run(
            text,
            &project,
            &carve,
            &RunLabel::Clearing(ToolId::new(label)),
            &tool_of(&project, tool),
        )
    };
    let off = |result: Result<(), ExportError>, line: usize| {
        assert!(
            matches!(result, Err(ExportError::OffMaterial { line: l, .. }) if l == line),
            "line {line}: {result:?}"
        );
    };
    run(&program("G1 Z-2 F300\nX56.4 F1000\n"), "t3", "t3").unwrap();
    off(run(&program("G1 Z-2 F300\nX57 F1000\n"), "t3", "t3"), 7);
    off(run(&program("G1 Z-2.5 F300\nX30 F1000\n"), "t3", "t3"), 6);
    let rest = program_at(12.8, 25.0, "G1 Z-2 F300\nX57.2 F1000\n");
    run(&rest, "t4", "t4").unwrap();
    off(run(&rest, "t3", "t3"), 6);
    off(run(&program("G1 Z-2 F300\nX30 F1000\n"), "t1", "t1"), 6);
    off(run(&program("G1 Z-2 F300\nX30 F1000\n"), "t3", "t4"), 6);
}

/// One drawing of the generated fixtures: its profiles, definition
/// space, and where it is placed.
type Drawing = (Vec<Polyline>, (f64, f64));

/// A 200 × 150 × 12 mm project with one instance `i1`, `i2`, … per
/// drawing, the V-bits `t2` (90°) and `t5` (60°), the end mills `t1`,
/// `t3` and `t4`, and no operations.
fn drawn(drawings: &[Drawing]) -> Project {
    let mut p = sample_with(Vec::new());
    p.tools.push(v_bit("t5", 60.0));
    p.stock = Stock {
        width_mm: 200.0,
        height_mm: 150.0,
        thickness_mm: Some(12.0),
    };
    p.definitions = BTreeMap::new();
    p.instances = Vec::new();
    for (k, (profiles, (dx, dy))) in drawings.iter().enumerate() {
        let def = FeatureDefinition {
            id: DefinitionId::new(format!("d{}", k + 1)),
            name: format!("Drawing {}", k + 1),
            profiles: profiles.clone(),
        };
        p.instances.push(FeatureInstance {
            id: InstanceId::new(format!("i{}", k + 1)),
            definition: def.id.clone(),
            transform: Transform::translate(*dx, *dy),
            name: format!("Shape {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        p.definitions.insert(def.id.clone(), def);
    }
    p
}

/// A stadium `w` × `h` (`w` > `h`) with its corner at (`x`, `y`): two
/// straight sides and two half circles.
fn stadium(x: f64, y: f64, w: f64, h: f64) -> Polyline {
    let r = h / 2.0;
    Polyline::new(
        vec![
            Vertex::line(x + r, y),
            Vertex::new(x + w - r, y, 1.0),
            Vertex::line(x + w - r, y + h),
            Vertex::new(x + r, y + h, 1.0),
        ],
        true,
    )
}

/// The small shapes, every one narrow enough for the 90° bit's cone with
/// no flat depth: a 20 × 5 rectangle, a triangle, an L, a circle of
/// radius 3, a ring 3.5 mm wide, a rectangle round a square island, and a
/// stadium 6 mm wide.
fn small_shapes() -> Vec<Drawing> {
    vec![
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 5.0)],
            (10.0, 10.0),
        ),
        (
            vec![polygon(&[(0.0, 0.0), (10.0, 0.0), (4.0, 7.0)])],
            (40.0, 10.0),
        ),
        (
            vec![polygon(&[
                (0.0, 0.0),
                (16.0, 0.0),
                (16.0, 5.0),
                (5.0, 5.0),
                (5.0, 14.0),
                (0.0, 14.0),
            ])],
            (60.0, 10.0),
        ),
        (
            vec![Polyline::circle(Point::new(0.0, 0.0), 3.0)],
            (93.0, 15.0),
        ),
        (
            vec![
                Polyline::circle(Point::new(0.0, 0.0), 7.0),
                Polyline::circle(Point::new(0.0, 0.0), 3.5),
            ],
            (115.0, 18.0),
        ),
        (
            vec![
                Polyline::rect(Point::new(0.0, 0.0), 24.0, 6.0),
                Polyline::rect(Point::new(11.0, 2.0), 2.0, 2.0),
            ],
            (130.0, 10.0),
        ),
        (vec![stadium(0.0, 0.0, 20.0, 6.0)], (160.0, 10.0)),
    ]
}

/// The larger shapes, for floors and clearing: a 30 × 10 rectangle, a
/// circle of radius 8, and a 40 × 24 rectangle round a 20 × 8 stadium
/// island.
fn large_shapes() -> Vec<Drawing> {
    vec![
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 30.0, 10.0)],
            (10.0, 60.0),
        ),
        (
            vec![Polyline::circle(Point::new(0.0, 0.0), 8.0)],
            (60.0, 70.0),
        ),
        (
            vec![
                Polyline::rect(Point::new(0.0, 0.0), 40.0, 24.0),
                stadium(10.0, 8.0, 20.0, 8.0),
            ],
            (80.0, 60.0),
        ),
    ]
}

/// The V-Carve `o6` of every instance of `project` with `tool`, edited
/// by `edit`.
fn carve_of(project: &Project, tool: &str, edit: impl FnOnce(&mut VCarveOp)) -> VCarveOp {
    carve_with(|op| {
        op.targets = project.instances.iter().map(|i| i.id.clone()).collect();
        op.tool = ToolId::new(tool);
        edit(op);
    })
}

/// Generates `op` in `project` (which gains it as its last operation) and
/// emits every run of it, one program per tool, through the whole safety
/// pass.
fn export_carve(project: &Project, op: &VCarveOp) -> Result<Vec<String>, ExportError> {
    let mut project = project.clone();
    project.operations.retain(|o| o.id() != &op.id);
    project.operations.push(Operation::VCarve(op.clone()));
    let (runs, _) = dc_cam::generate_vcarve(&project, op)
        .unwrap_or_else(|e| panic!("the carve generates: {e}\n{e:?}"));
    let ops: Vec<EmitOp> = runs
        .into_iter()
        .map(|run| EmitOp {
            op: Operation::VCarve(op.clone()),
            tool: tool_of(&project, run.tool.as_str()),
            toolpath: run.toolpath,
            holes: None,
            label: run.label,
        })
        .collect();
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)?;
    Ok(programs.into_iter().map(|p| p.text).collect())
}

fn offset(step_over_mm: f64, direction: CutDirection) -> VCarveStrategy {
    VCarveStrategy::Offset {
        step_over_mm,
        direction,
    }
}

/// Every generated V-Carve passes S17 and the clearing-run S9: medial and
/// offset, with both bench angles, with flat floors, with clearing passes
/// (offset and raster, plunging and ramping), from a start plane a Pocket
/// cleared and from one the user vouches for. Arc fitting is on (the
/// sample's 0.01 mm), so rings are written as arcs where they are round.
#[test]
fn generated_v_carves_pass_s17() {
    let small = drawn(&small_shapes());
    let mut both = small_shapes();
    both.extend(large_shapes());
    let both = drawn(&both);
    let large = drawn(&large_shapes());
    let pass = |tool: &str, strategy: ClearanceStrategy, entry: Entry| ClearingPass {
        tool: ToolId::new(tool),
        strategy,
        direction: CutDirection::Climb,
        entry,
    };
    let floor = |f: f64| {
        move |op: &mut VCarveOp| {
            op.flat_depth_mm = Some(f);
            op.step_over_fraction = Some(0.4);
        }
    };
    let mut cases: Vec<(&str, &Project, VCarveOp)> = vec![
        ("medial 90°", &small, carve_of(&small, "t2", |_| {})),
        ("medial 60°", &small, carve_of(&small, "t5", |_| {})),
        (
            "offset 90° climb",
            &small,
            carve_of(&small, "t2", |op| {
                op.strategy = offset(0.5, CutDirection::Climb);
            }),
        ),
        (
            "offset 60° conventional",
            &small,
            carve_of(&small, "t5", |op| {
                op.strategy = offset(0.4, CutDirection::Conventional);
            }),
        ),
        ("medial 90° floor", &both, carve_of(&both, "t2", floor(1.5))),
        (
            "offset 90° floor",
            &both,
            carve_of(&both, "t2", |op| {
                floor(1.5)(op);
                op.strategy = offset(0.5, CutDirection::Climb);
            }),
        ),
        (
            "medial 90° clearing",
            &large,
            carve_of(&large, "t2", |op| {
                floor(2.0)(op);
                op.clearing = vec![
                    pass("t3", ClearanceStrategy::Offset, Entry::Plunge),
                    pass("t4", ClearanceStrategy::Offset, Entry::Plunge),
                ];
            }),
        ),
        (
            "offset 90° clearing",
            &large,
            carve_of(&large, "t2", |op| {
                floor(2.0)(op);
                op.strategy = offset(0.5, CutDirection::Conventional);
                op.clearing = vec![pass(
                    "t3",
                    ClearanceStrategy::Raster { angle_deg: 0.0 },
                    Entry::Ramp { angle_deg: 10.0 },
                )];
            }),
        ),
        (
            "medial 60° vouched start plane",
            &small,
            carve_of(&small, "t5", |op| {
                floor(2.0)(op);
                op.start_depth_mm = 1.5;
                op.start_plane_cleared_externally = true;
            }),
        ),
    ];
    // A start plane 2 mm down that a Pocket of a rectangle round every
    // small shape clears first.
    let mut pocketed = small.clone();
    let around = FeatureDefinition {
        id: DefinitionId::new("dp"),
        name: "Around".into(),
        profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 190.0, 30.0)],
    };
    pocketed.definitions.insert(around.id.clone(), around);
    pocketed.instances.push(FeatureInstance {
        id: InstanceId::new("ip"),
        definition: DefinitionId::new("dp"),
        transform: Transform::translate(5.0, 5.0),
        name: "Around".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    pocketed.operations = vec![Operation::Pocket(PocketOp {
        id: OperationId::new("o2"),
        targets: vec![InstanceId::new("ip")],
        depth_mm: Some(2.0),
        ..sample_pocket()
    })];
    let mut cleared = carve_of(&small, "t2", |op| op.start_depth_mm = 2.0);
    cleared.targets.retain(|t| t.as_str() != "ip");
    cases.push(("medial 90° in a Pocket", &pocketed, cleared));
    for (name, project, op) in cases {
        if let Err(e) = export_carve(project, &op) {
            panic!("{name}: the safety pass refuses a generated carve: {e}\n{e:?}");
        }
    }
}

/// S17 imports no generator (`docs/rust-guidelines.md`, "Crate
/// boundaries"; security review M3): the `use dc_cam::` lines of the
/// crate's code outside its tests name only toolpath data types and the
/// entry constants, and no other path into `dc_cam` appears there but
/// phase 2's two documented rules (`last_pass_step_reach_mm`, the
/// last-pass step's length, and `placed_tabs::place`, the tab layout
/// rule). A V-Carve's regions, floor and engagement come from
/// `dc_geometry` and `dc_project`.
#[test]
fn s17_uses_no_generator() {
    const DATA_TYPES: [&str; 10] = [
        "CannedKind",
        "DrillHole",
        "DrillToolpath",
        "FeedField",
        "Move",
        "MoveKind",
        "RunLabel",
        "Toolpath",
        "entry::CLEARED_POINT_TOLERANCE_MM",
        "entry::ENTRY_CLEARANCE_MM",
    ];
    const RULES: [&str; 2] = [
        "dc_cam::last_pass_step_reach_mm",
        "dc_cam::placed_tabs::place",
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    assert!(
        files.iter().any(|f| f.ends_with("material.rs")),
        "{files:?}"
    );
    // Modules `lib.rs` declares for tests only (`post`) are test code.
    let lib = std::fs::read_to_string(dir.join("lib.rs")).unwrap();
    let test_only: Vec<String> = lib
        .lines()
        .zip(lib.lines().skip(1))
        .filter(|(attribute, _)| attribute.trim() == "#[cfg(test)]")
        .filter_map(|(_, item)| {
            let name = item.trim().strip_prefix("mod ")?.strip_suffix(';')?;
            Some(format!("{name}.rs"))
        })
        .collect();
    assert!(test_only.contains(&"post.rs".to_owned()), "{test_only:?}");
    files.retain(|f| !test_only.iter().any(|t| f.ends_with(t)));
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let code = outside_tests(&text);
        for statement in use_statements(&code, "use dc_cam::") {
            for name in imported_names(&statement) {
                assert!(
                    DATA_TYPES.contains(&name.as_str()),
                    "{}: `{statement}` imports `{name}`, not a toolpath data type",
                    file.display()
                );
            }
        }
        for line in code.lines() {
            let line = line.trim();
            if line.starts_with("//") || line.starts_with("use dc_cam::") {
                continue;
            }
            for (at, _) in line.match_indices("dc_cam::") {
                let path: String = line[at..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                    .collect();
                assert!(
                    RULES.contains(&path.as_str())
                        || DATA_TYPES.iter().any(|t| path == format!("dc_cam::{t}")),
                    "{}: `{line}` calls `{path}`",
                    file.display()
                );
            }
        }
    }
}

/// `text` without its `#[cfg(test)]` items: each attribute and the item
/// after it, to the end of its braces (or its line, for an item with
/// none).
fn outside_tests(text: &str) -> String {
    let mut out = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "#[cfg(test)]" {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut depth = 0i64;
        let mut opened = false;
        for item in lines.by_ref() {
            let code = item.split("//").next().unwrap_or_default();
            for c in code.chars() {
                match c {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            let attribute = item.trim_start().starts_with("#[");
            let ended = code.trim_end().ends_with(';') || code.trim_end().ends_with(',');
            if !attribute && depth <= 0 && (opened || ended) {
                break;
            }
        }
    }
    out
}

/// Every statement of `code` that starts with `prefix`, up to its `;`,
/// on one line.
fn use_statements(code: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open: Option<String> = None;
    for line in code.lines() {
        let line = line.trim();
        match open.as_mut() {
            Some(statement) => statement.push_str(line),
            None if line.starts_with(prefix) => open = Some(line.to_owned()),
            None => continue,
        }
        if line.ends_with(';')
            && let Some(statement) = open.take()
        {
            out.push(statement);
        }
    }
    out
}

/// The paths `statement` (`use dc_cam::…;`) imports, relative to
/// `dc_cam`: `use dc_cam::entry::{A, B};` imports `entry::A` and
/// `entry::B`.
fn imported_names(statement: &str) -> Vec<String> {
    let body = statement
        .trim_start_matches("use dc_cam::")
        .trim_end_matches(';')
        .replace(' ', "");
    match body.split_once('{') {
        Some((stem, rest)) => rest
            .trim_end_matches('}')
            .split(',')
            .filter(|n| !n.is_empty())
            .map(|n| format!("{stem}{n}"))
            .collect(),
        None => vec![body],
    }
}

/// A sign like the one slice 11 (#411) draws: a 150 × 70 mm board whose
/// border, round a 110 × 40 mm stadium, is carved with the 90° bit to a
/// 2.5 mm flat depth, and five letters (I, L, T, O and D, 16 mm tall,
/// strokes 3 mm wide) carved in the stadium with the 60° bit, medial.
fn sign() -> (Project, VCarveOp, VCarveOp) {
    let letters: Vec<Drawing> = vec![
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 3.0, 16.0)],
            (60.0, 52.0),
        ),
        (
            vec![polygon(&[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 3.0),
                (3.0, 3.0),
                (3.0, 16.0),
                (0.0, 16.0),
            ])],
            (67.0, 52.0),
        ),
        (
            vec![polygon(&[
                (4.5, 0.0),
                (7.5, 0.0),
                (7.5, 13.0),
                (12.0, 13.0),
                (12.0, 16.0),
                (0.0, 16.0),
                (0.0, 13.0),
                (4.5, 13.0),
            ])],
            (81.0, 52.0),
        ),
        (
            vec![
                Polyline::circle(Point::new(0.0, 0.0), 8.0),
                Polyline::circle(Point::new(0.0, 0.0), 5.0),
            ],
            (105.0, 60.0),
        ),
        (
            vec![
                Polyline::new(
                    vec![
                        Vertex::line(0.0, 0.0),
                        Vertex::new(5.0, 0.0, 1.0),
                        Vertex::line(5.0, 16.0),
                        Vertex::line(0.0, 16.0),
                    ],
                    true,
                ),
                Polyline::new(
                    vec![
                        Vertex::line(3.0, 3.0),
                        Vertex::new(5.0, 3.0, 1.0),
                        Vertex::line(5.0, 13.0),
                        Vertex::line(3.0, 13.0),
                    ],
                    true,
                ),
            ],
            (117.0, 52.0),
        ),
    ];
    let mut drawings = vec![(
        vec![
            Polyline::rect(Point::new(0.0, 0.0), 150.0, 70.0),
            stadium(20.0, 15.0, 110.0, 40.0),
        ],
        (20.0, 25.0),
    )];
    drawings.extend(letters);
    let project = drawn(&drawings);
    let border = carve_with(|op| {
        op.name = "Border".into();
        op.targets = vec![InstanceId::new("i1")];
        op.flat_depth_mm = Some(2.5);
        op.step_over_fraction = Some(0.4);
    });
    let text = carve_with(|op| {
        op.id = OperationId::new("o7");
        op.name = "Letters".into();
        op.targets = (2..=6).map(|k| InstanceId::new(format!("i{k}"))).collect();
        op.tool = ToolId::new("t5");
    });
    (project, border, text)
}

/// The sign's two carves pass S17, and S17 (with S9) checks them in under
/// 2 s in a debug build: the time is the material check alone, on the
/// parsed programs, generation and the rest of the safety pass aside.
#[test]
fn s17_check_is_fast_on_the_sign() {
    let (project, border, text) = sign();
    let mut project = project;
    project.operations = vec![
        Operation::VCarve(border.clone()),
        Operation::VCarve(text.clone()),
    ];
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let tolerances = Tolerances {
        flatten_mm: project.flatten_tolerance_mm,
        arc_fit_mm: project.arc_fit_tolerance_mm,
    };
    let mut elapsed = std::time::Duration::ZERO;
    let mut moves = 0;
    for op in [border, text] {
        let programs = export_carve(&project, &op).unwrap();
        assert_eq!(programs.len(), 1, "one V-bit run");
        let text = &programs[0];
        let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
        moves += blocks.iter().filter(|b| b.motion.is_some()).count();
        let operation = Operation::VCarve(op.clone());
        let tool = tool_of(&project, op.tool.as_str());
        let material = Material::of_run(&project, &operation, &RunLabel::Main, &tool);
        let toolpath = Toolpath::default();
        let span = OpSpan {
            op: &operation,
            tool: &tool,
            toolpath: &toolpath,
            material: &material,
            tabs: &[],
            lines: 1..text.lines().count() + 1,
        };
        let start = std::time::Instant::now();
        material::check(&blocks, &post, &transform, tolerances, &span).unwrap();
        elapsed += start.elapsed();
    }
    assert!(moves > 1000, "the sign has {moves} moves");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "S17 took {elapsed:?} for {moves} moves"
    );
}

/// #410's "Rag" golden: PureCutCNC's depth-aware Douglas-Peucker runs the
/// R's stem-to-bowl chain straight from (14.799, 19.404) 1.518 mm deep to
/// (12.464, 19.404) 4.268 mm deep, and its cone reaches 0.256 mm past the
/// R's wall on the way (`tests/golden/vcarve-medial-text-rag/README.md`,
/// `ChainDepthFitted`). Neither end of that chord is past the V surface by
/// more than the slack; only the exact test between them finds the
/// gouge, and S17 refuses the upstream program's cut. DreamCarve's own
/// carve of the twin passes.
#[test]
fn upstream_rag_chord_through_the_r_fails_s17() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/vcarve-medial-text-rag/dreamcarve.dcv");
    let project = dc_project::load(&std::fs::read(path).unwrap())
        .unwrap()
        .project;
    let Some(Operation::VCarve(op)) = project.operations.first().cloned() else {
        panic!("the Rag twin is one V-Carve");
    };
    let upstream = program_at(
        16.693,
        19.409,
        "G1 Z-1.525 F300\nX14.799 Y19.404 Z-1.518 F1000\nX12.464 Y19.404 Z-4.268\n",
    );
    assert_cone_off(check_carve(&upstream, &project, &op), 8, Some(0.256));
    let before = program_at(
        16.693,
        19.409,
        "G1 Z-1.525 F300\nX14.799 Y19.404 Z-1.518 F1000\n",
    );
    check_carve(&before, &project, &op).unwrap();
    export_carve(&project, &op).unwrap();
}
