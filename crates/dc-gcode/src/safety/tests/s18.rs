// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S18 in the safety pass (#427; `docs/specs/phase-3/design.md`,
//! "Safety pass", the S18 bullet; functional S18 and acceptance 55).
//!
//! The hand-written programs countersink the 1/8 in circles `h1`–`h4`
//! inside the sample rectangle (X 10 to 60, Y 10 to 40) with the 90°
//! 6.35 mm V-bit `t2` (cone 3.175 mm) and a 6 mm mouth, so the plunge is
//! 3 mm below the start plane; the stock top is the origin, so machine Z
//! is project Z less the stock's thickness. The 7 mm circle `h5` is no
//! narrower than the mouth, so it is skipped. The retract plane is 1 mm
//! above the stock top, safe Z 5 mm. The generated ones run through
//! `emit` and so through the whole safety pass.

use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use crate::parse::{CycleModel, parse_program_with};
use dc_cam::RunLabel;
use dc_geometry::{Polyline, Transform};
use dc_project::test_support::{sample_drill, sample_pocket, tool_v_bit_90};
use dc_project::{
    DefinitionId, DrillCycle, DrillOp, FeatureDefinition, FeatureInstance, InstanceId, PocketOp,
    PostProcessorRef, ToolId, ToolKind,
};

const DATE: &str = "2026-10-29";

/// The 1/8 in holes of acceptance 55, centred inside the sample
/// rectangle, every tool-radius disc round them inside it too.
const HOLE_MM: f64 = 3.175;
const CENTRES: [(f64, f64); 4] = [(20.0, 20.0), (50.0, 20.0), (50.0, 30.0), (20.0, 30.0)];

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

/// The sample project on `thickness_mm` stock with the V-bits `t2` (90°)
/// and `t6` (82°), the circles `h1`–`h4` ([`CENTRES`], 3.175 mm) and `h5`
/// (7 mm at (35, 25)), and `ops` as its operations.
fn holes_with(thickness_mm: f64, ops: Vec<Operation>) -> Project {
    let mut project = sample_project();
    project.stock.thickness_mm = Some(thickness_mm);
    project.tools.push(tool_v_bit_90());
    project.tools.push(v_bit("t6", 82.0));
    let circle = |id: &str, diameter_mm: f64| FeatureDefinition {
        id: DefinitionId::new(id),
        name: format!("{diameter_mm} mm hole"),
        profiles: vec![Polyline::circle(Point::new(0.0, 0.0), diameter_mm / 2.0)],
    };
    for definition in [circle("dh", HOLE_MM), circle("dw", 7.0)] {
        project
            .definitions
            .insert(definition.id.clone(), definition);
    }
    let mut place = |id: &str, definition: &str, (x, y): (f64, f64)| {
        project.instances.push(FeatureInstance {
            id: InstanceId::new(id),
            definition: DefinitionId::new(definition),
            transform: Transform::translate(x, y),
            name: id.to_uppercase(),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
    };
    for (k, &centre) in CENTRES.iter().enumerate() {
        place(&format!("h{}", k + 1), "dh", centre);
    }
    place("h5", "dw", (35.0, 25.0));
    project.operations = ops;
    project
}

/// A countersink `o3` of `h1`–`h5` with `t2`, a `mouth_mm` mouth from
/// `start_depth_mm` below the stock top, retract 1 mm, safe Z 5 mm.
fn sink(mouth_mm: f64, start_depth_mm: f64, cleared_externally: bool) -> DrillOp {
    DrillOp {
        targets: (1..=5).map(|k| InstanceId::new(format!("h{k}"))).collect(),
        tool: ToolId::new("t2"),
        cycle: DrillCycle::Countersink {
            diameter_mm: mouth_mm,
            start_depth_mm,
            start_plane_cleared_externally: cleared_externally,
        },
        depth_mm: None,
        ..sample_drill()
    }
}

/// The sample Pocket `o2` of the rectangle, `depth_mm` deep.
fn pocket(depth_mm: f64) -> PocketOp {
    PocketOp {
        depth_mm: Some(depth_mm),
        ..sample_pocket()
    }
}

/// `project`'s tool `id`.
fn tool_of(project: &Project, id: &ToolId) -> Tool {
    project
        .tools
        .iter()
        .find(|t| &t.id == id)
        .cloned()
        .unwrap_or_else(|| panic!("no tool {id}"))
}

/// S9 and S18 alone on hand-written `text` as Drill `op` of `project`,
/// cut with its own tool, read as `project`'s post-processor reads it.
fn check_sink(text: &str, project: &Project, op: &DrillOp) -> Result<(), ExportError> {
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(project, &post).unwrap();
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let operation = Operation::Drill(op.clone());
    let tool = tool_of(project, &op.tool);
    let material = Material::of_run(project, &operation, &RunLabel::Main, &tool);
    let tabs = material::tab_spines(project, &operation);
    let toolpath = Toolpath::default();
    let span = OpSpan {
        op: &operation,
        tool: &tool,
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

/// A program that rapids to (`x`, `y`) at Z 5, down to Z 1 (the retract
/// plane), runs `cuts` from line 6 and lifts.
fn program_at(x: f64, y: f64, cuts: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG0 Z1\n{cuts}G0 Z5\n")
}

/// Asserts `result` is [`ExportError::CountersinkDepth`] at `line`, with
/// the derived bottom and the Z the line ends at (project space) within
/// 1e-3 mm.
#[track_caller]
fn assert_depth(result: Result<(), ExportError>, line: usize, expected_mm: f64, found_mm: f64) {
    match result {
        Err(ExportError::CountersinkDepth {
            line: l,
            expected_mm: e,
            found_mm: f,
            ..
        }) if l == line && (e - expected_mm).abs() <= 1e-3 && (f - found_mm).abs() <= 1e-3 => {}
        other => panic!(
            "expected CountersinkDepth at line {line} ({expected_mm} vs {found_mm}): {other:?}"
        ),
    }
}

/// Asserts `result` is [`ExportError::CountersinkDepth`] at `line`,
/// whatever its numbers.
#[track_caller]
fn assert_refused(result: Result<(), ExportError>, line: usize) {
    assert!(
        matches!(result, Err(ExportError::CountersinkDepth { line: l, .. }) if l == line),
        "expected CountersinkDepth at line {line}: {result:?}"
    );
}

/// S18, the depth: on 6 mm stock the 6 mm mouth of the 90° bit plunges
/// to machine Z −3 (project Z 3) and the 82° bit's to −3.451, within the
/// last written place; a plunge that stops short or goes past, or a cut
/// that ends anywhere but there, fails. The plunge starts no higher than
/// the retract plane, and no rapid ends below it.
#[test]
fn countersink_at_the_wrong_depth_fails_s18() {
    let op = sink(6.0, 0.0, false);
    let project = holes_with(6.0, vec![Operation::Drill(op.clone())]);
    let at = |cuts: &str| check_sink(&program_at(20.0, 20.0, cuts), &project, &op);
    at("G1 Z-3 F300\n").unwrap();
    at("G1 Z-3.0004 F300\n").unwrap();
    at("G1 Z-3 F300\nG4 P0.5\n").unwrap();
    assert_depth(at("G1 Z-2.9 F300\n"), 6, 3.0, 3.1);
    assert_depth(at("G1 Z-3.1 F300\n"), 6, 3.0, 2.9);
    assert_depth(at("G1 Z-3.002 F300\n"), 6, 3.0, 2.998);
    // Every cut ends at the depth: two steps down are refused at the first.
    assert_depth(at("G1 Z-2 F300\nZ-3\n"), 6, 3.0, 4.0);
    // A plunge from above the retract plane (7 in project Z).
    assert_refused(
        check_sink(
            "G21 G90\nG54\nG0 Z5\nX20 Y20\nG1 Z-3 F300\nG0 Z5\n",
            &project,
            &op,
        ),
        5,
    );
    // A rapid that ends below the retract plane; the error names the
    // derived bottom and where the line ends.
    assert_depth(
        check_sink(
            "G21 G90\nG54\nG0 Z5\nX20 Y20\nG0 Z0.5\nG1 Z-3 F300\nG0 Z5\n",
            &project,
            &op,
        ),
        5,
        3.0,
        6.5,
    );
    // The 82° bit's mouth: 6 / (2 · tan 41°) = 3.4511 mm.
    let op82 = DrillOp {
        tool: ToolId::new("t6"),
        ..op.clone()
    };
    let project82 = holes_with(6.0, vec![Operation::Drill(op82.clone())]);
    check_sink(
        &program_at(20.0, 20.0, "G1 Z-3.451 F300\n"),
        &project82,
        &op82,
    )
    .unwrap();
    assert_depth(
        check_sink(&program_at(20.0, 20.0, "G1 Z-3 F300\n"), &project82, &op82),
        6,
        6.0 - 6.0 / (2.0 * 41f64.to_radians().tan()),
        3.0,
    );
}

/// S18, no cycle word: on LinuxCNC with canned cycles on, a `G81` at a
/// hole's centre to the right depth from the retract plane makes the
/// same motion as the plain moves, which pass, and still fails at its
/// line; so does a `G82` with a dwell.
#[test]
fn countersink_written_as_g81_fails_s18() {
    let op = sink(6.0, 0.0, false);
    let mut project = holes_with(6.0, vec![Operation::Drill(op.clone())]);
    project.post_processor = PostProcessorRef::latest("linuxcnc").unwrap();
    project.post_processor.use_canned_cycles = true;
    check_sink(&program_at(20.0, 20.0, "G1 Z-3 F300\n"), &project, &op).unwrap();
    let cycle =
        |line: &str| format!("G21 G90\nG54\nG0 Z5\nX20 Y20\nG98\n{line}\nX50 Y20\nG80\nG0 Z5\n");
    assert_refused(
        check_sink(&cycle("G81 X20 Y20 Z-3 R1 F300"), &project, &op),
        6,
    );
    assert_refused(
        check_sink(&cycle("G82 X20 Y20 Z-3 R1 P0.5 F300"), &project, &op),
        6,
    );
}

/// S18, the centre: a plunge half a millimetre off a hole's centre, at a
/// point that is no hole's, or at the 7 mm circle the mouth rule skips
/// fails; one off by less than the last written place passes. Every cut
/// is vertical: a sideways cut at the bottom, a slanted plunge that ends
/// at the centre and an arc that comes back to it fail.
#[test]
fn countersink_off_centre_fails_s18() {
    let op = sink(6.0, 0.0, false);
    let project = holes_with(6.0, vec![Operation::Drill(op.clone())]);
    let plunge = |x: f64, y: f64| check_sink(&program_at(x, y, "G1 Z-3 F300\n"), &project, &op);
    for (x, y) in CENTRES {
        plunge(x, y).unwrap();
    }
    plunge(20.0004, 30.0).unwrap();
    assert_refused(plunge(20.5, 20.0), 6);
    assert_refused(plunge(30.0, 25.0), 6);
    assert_refused(plunge(35.0, 25.0), 6);
    let at = |cuts: &str| check_sink(&program_at(20.0, 20.0, cuts), &project, &op);
    assert_refused(at("G1 Z-3 F300\nX20.2\n"), 7);
    assert_refused(
        check_sink(&program_at(20.5, 20.0, "G1 X20 Z-3 F300\n"), &project, &op),
        6,
    );
    assert_refused(at("G1 Z-3 F300\nG2 X20 Y20 I0.1 J0\n"), 7);
}

/// H4: a countersink 2 mm down that the user vouches for is engaged from
/// the stock top: its 3 mm plunge engages 5 mm of the 90° bit's 3.175 mm
/// cone and fails on the shank. An enabled earlier Pocket 2 mm deep over
/// the holes clears its start plane, and it passes; a Pocket at another
/// depth, a disabled one or a later one does not. A shallow countersink
/// within the cone from the stock top passes uncleared, and a mouth wider
/// than the bit engages past its cone wherever it starts.
#[test]
fn uncleared_countersink_engagement_fails_s18() {
    let op = sink(6.0, 2.0, true);
    let deep = |ops: Vec<Operation>| {
        let project = holes_with(12.0, ops);
        check_sink(&program_at(20.0, 20.0, "G1 Z-5 F300\n"), &project, &op)
    };
    let shank = |result: Result<(), ExportError>, engaged_mm: f64| match result {
        Err(ExportError::ConeHeightExceeded {
            line: 6,
            engaged_mm: e,
            cone_mm,
            ..
        }) if (e - engaged_mm).abs() <= 1e-6 && (cone_mm - 3.175).abs() <= 1e-9 => {}
        other => panic!("expected ConeHeightExceeded ({engaged_mm}): {other:?}"),
    };
    let drill = Operation::Drill(op.clone());
    shank(deep(vec![drill.clone()]), 5.0);
    deep(vec![Operation::Pocket(pocket(2.0)), drill.clone()]).unwrap();
    shank(
        deep(vec![Operation::Pocket(pocket(3.0)), drill.clone()]),
        5.0,
    );
    let disabled = PocketOp {
        enabled: false,
        ..pocket(2.0)
    };
    shank(deep(vec![Operation::Pocket(disabled), drill.clone()]), 5.0);
    shank(deep(vec![drill, Operation::Pocket(pocket(2.0))]), 5.0);

    let shallow = sink(4.0, 1.0, true);
    let project = holes_with(12.0, vec![Operation::Drill(shallow.clone())]);
    check_sink(&program_at(20.0, 20.0, "G1 Z-3 F300\n"), &project, &shallow).unwrap();

    let wide = sink(7.0, 0.0, false);
    let project = holes_with(12.0, vec![Operation::Drill(wide.clone())]);
    shank(
        check_sink(&program_at(20.0, 20.0, "G1 Z-3.5 F300\n"), &project, &wide),
        3.5,
    );
}

/// Generates Drill `op` in `project` (which gains it as its last
/// operation) and emits it through the whole safety pass. Returns the
/// program and the warnings.
fn export_sink(project: &Project, op: &DrillOp) -> Result<(String, Vec<String>), ExportError> {
    let mut project = project.clone();
    project.operations.retain(|o| o.id() != &op.id);
    project.operations.push(Operation::Drill(op.clone()));
    let (drilled, _) = dc_cam::generate_drill(&project, op)
        .unwrap_or_else(|e| panic!("the countersink generates: {e}\n{e:?}"));
    let ops = [EmitOp::drill(
        Operation::Drill(op.clone()),
        tool_of(&project, &op.tool),
        drilled,
    )];
    let (programs, warnings) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)?;
    let text = programs.into_iter().map(|p| p.text).collect();
    Ok((text, warnings.iter().map(|w| w.code().to_owned()).collect()))
}

/// Whether `line` has a drilling canned-cycle word, outside comments.
fn has_cycle_word(line: &str) -> bool {
    let code = line.split(['(', ';']).next().unwrap_or_default();
    code.split_whitespace().any(|w| {
        matches!(
            w.to_ascii_uppercase().as_str(),
            "G73" | "G81" | "G82" | "G83"
        )
    })
}

/// Every generated countersink passes S18: from the stock top with the
/// 90° and 82° bits, with a dwell, with a mouth as wide as the bit, from
/// a start plane an earlier Pocket cleared and from one the user vouches
/// for. Each is exported on the sample's grblHAL and on LinuxCNC with
/// canned cycles on (acceptance 55, the safety half): there the program
/// has no cycle word and no `canned_cycle_expanded` warning, since a
/// countersink is never a canned cycle.
#[test]
fn generated_countersinks_pass_s18() {
    let thin = holes_with(6.0, Vec::new());
    let thick = holes_with(12.0, Vec::new());
    let pocketed = holes_with(12.0, vec![Operation::Pocket(pocket(4.0))]);
    let cases: Vec<(&str, &Project, DrillOp)> = vec![
        ("90° from the stock top", &thin, sink(6.0, 0.0, false)),
        (
            "82° from the stock top",
            &thin,
            DrillOp {
                tool: ToolId::new("t6"),
                ..sink(6.0, 0.0, false)
            },
        ),
        (
            "90° with a dwell",
            &thin,
            DrillOp {
                dwell_s: 0.5,
                ..sink(6.0, 0.0, false)
            },
        ),
        ("a mouth as wide as the bit", &thin, sink(6.35, 0.0, false)),
        ("on a Pocket's floor", &pocketed, sink(6.0, 4.0, false)),
        ("on a vouched start plane", &thick, sink(4.0, 1.0, true)),
    ];
    for (name, project, op) in cases {
        for post in ["grblhal", "linuxcnc"] {
            let mut project = project.clone();
            project.post_processor = PostProcessorRef::latest(post).unwrap();
            project.post_processor.use_canned_cycles = post == "linuxcnc";
            let (text, warnings) = export_sink(&project, &op).unwrap_or_else(|e| {
                panic!(
                    "{name} on {post}: the safety pass refuses a generated countersink: {e}\n{e:?}"
                )
            });
            assert!(
                !text.lines().any(has_cycle_word),
                "{name} on {post}: a cycle word\n{text}"
            );
            assert!(
                !warnings.iter().any(|w| w == "canned_cycle_expanded"),
                "{name} on {post}: {warnings:?}"
            );
        }
    }
}
