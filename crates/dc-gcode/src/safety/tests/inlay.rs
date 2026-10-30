// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17 for an Inlay side (#436; `docs/specs/phase-3/design.md`, "Safety
//! pass", the S17 bullet; security review H4).
//!
//! The female is held as a V-Carve of its design with a flat depth `D`
//! from the stock top. The male is held to the surround of its targets
//! mirrored about their bounding box's centre line, re-derived here with
//! `dc_geometry` alone, from a start plane `D − G` below the stock top,
//! with its flank engaged from the stock top.
//!
//! The hand-written programs cut a male of a 30 × 10 mm rectangle (X 10
//! to 40, Y 20 to 30) and a 6 mm square (X 70 to 76, Y 20 to 26) on 12 mm
//! stock, stock-top origin (machine Z is project Z less 12), with the 90°
//! 6.35 mm V-bit `t2` (slope 1, cone 3.175 mm). The targets span X 10 to
//! 76, so the mirror line is X 43: the rectangle is mirrored to X 46 to 76
//! and the square to X 10 to 16.

use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use crate::parse::{CycleModel, parse_program_with};
use dc_cam::RunLabel;
use dc_geometry::{Polyline, Transform, Vertex};
use dc_project::test_support::{tool_6_35_upcut, tool_v_bit_90};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InlayOp, InlaySide, InstanceId, MachiningOrder, OperationId, Stock, ToolId,
    ToolKind,
};
use std::collections::BTreeMap;

const DATE: &str = "2026-10-30";

/// A `width` × `height` × 12 mm project with one instance `i1` of
/// `profiles`, the 90° V-bit `t2`, a 60° one `t5` and a 3.175 mm end mill
/// `t3`, and no operations.
fn drawn(profiles: Vec<Polyline>, width_mm: f64, height_mm: f64) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm,
        height_mm,
        thickness_mm: Some(12.0),
    };
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Design".into(),
        profiles,
    };
    p.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: def.id.clone(),
        transform: Transform::IDENTITY,
        name: "Design".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    p.definitions = BTreeMap::from([(def.id.clone(), def)]);
    p.tools.push(tool_v_bit_90());
    p.tools.push(Tool {
        id: ToolId::new("t5"),
        name: "60° V-bit".into(),
        cutter: ToolKind::VBit {
            included_angle_deg: 60.0,
        },
        ..tool_v_bit_90()
    });
    p.tools.push(Tool {
        id: ToolId::new("t3"),
        name: "3.175 mm end mill".into(),
        diameter_mm: 3.175,
        step_down_mm: Some(1.0),
        ..tool_6_35_upcut()
    });
    p.operations.clear();
    p
}

/// The rectangle and the square of the hand-written programs.
fn two_shapes() -> Project {
    drawn(
        vec![
            Polyline::rect(Point::new(10.0, 20.0), 30.0, 10.0),
            Polyline::rect(Point::new(70.0, 20.0), 6.0, 6.0),
        ],
        100.0,
        60.0,
    )
}

/// An Inlay `o1` of `i1` with `tool`: `side`, `depth_mm` 2, `glue_gap_mm`
/// 0.5 (start plane 1.5 mm below the stock top), `plug_depth_mm` `plug`,
/// `margin_mm` `margin`.
fn inlay(side: InlaySide, tool: &str, plug_mm: f64, margin_mm: f64) -> InlayOp {
    InlayOp {
        id: OperationId::new("o1"),
        name: "Inlay".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new(tool),
        side,
        depth_mm: Some(2.0),
        glue_gap_mm: Some(0.5),
        plug_depth_mm: Some(plug_mm),
        margin_mm: Some(margin_mm),
        clearing: Vec::new(),
        step_over_fraction: Some(0.4),
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        cut_out: None,
    }
}

/// S9 and S17 alone on hand-written `text`, as the V-bit's run of `op` in
/// `project` (which gains `op`).
fn check_male(text: &str, project: &Project, op: &InlayOp) -> Result<(), ExportError> {
    let mut project = project.clone();
    project.operations = vec![Operation::Inlay(op.clone())];
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .cloned()
        .expect("the tool");
    let op = Operation::Inlay(op.clone());
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let material = Material::of_run(&project, &op, &RunLabel::Main, &tool);
    let tabs = material::tab_spines(&project, &op);
    let toolpath = Toolpath::default();
    let span = OpSpan {
        op: &op,
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

/// A program that rapids to (`x`, `y`) at Z 5, down to Z 1 (the stock
/// top plus the entry clearance), runs `cuts` and lifts.
fn program_at(x: f64, y: f64, cuts: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG0 Z1\n{cuts}G0 Z5\n")
}

#[track_caller]
fn assert_cone_off(result: Result<(), ExportError>, line: usize) {
    assert!(
        matches!(result, Err(ExportError::ConeOffMaterial { line: l, .. }) if l == line),
        "expected ConeOffMaterial at line {line}: {result:?}"
    );
}

/// Generates `op` in `project` (which gains it) and emits every run of
/// it, one program per tool, through the whole safety pass.
fn export_inlay(project: &Project, op: &InlayOp) -> Result<Vec<String>, ExportError> {
    let mut project = project.clone();
    project.operations = vec![Operation::Inlay(op.clone())];
    let (runs, _) = dc_cam::generate_inlay(&project, op)
        .unwrap_or_else(|e| panic!("the inlay generates: {e}\n{e:?}"));
    let ops: Vec<EmitOp> = runs
        .into_iter()
        .map(|run| EmitOp {
            op: Operation::Inlay(op.clone()),
            tool: project
                .tools
                .iter()
                .find(|t| t.id == run.tool)
                .cloned()
                .expect("the run's tool"),
            toolpath: run.toolpath,
            holes: None,
            label: run.label,
        })
        .collect();
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)?;
    Ok(programs.into_iter().map(|p| p.text).collect())
}

/// The inlay test part's star at half size, 30 mm tip to tip, with a
/// 6 mm circle in its middle, centred at (30, 30) on 60 × 60 mm stock.
fn small_star() -> Project {
    let inner = 6.0 * std::f64::consts::FRAC_1_SQRT_2;
    let points = [
        (15.0, 0.0),
        (inner, inner),
        (0.0, 15.0),
        (-inner, inner),
        (-15.0, 0.0),
        (-inner, -inner),
        (0.0, -15.0),
        (inner, -inner),
    ];
    let star = Polyline::new(
        points
            .iter()
            .map(|&(x, y)| Vertex::line(x + 30.0, y + 30.0))
            .collect(),
        true,
    );
    drawn(
        vec![star, Polyline::circle(Point::new(30.0, 30.0), 3.0)],
        60.0,
        60.0,
    )
}

/// Every generated Inlay passes S17 and the clearing-run S9 through the
/// whole safety pass: the small star's female and male with the 60° bit,
/// each with and without a clearing pass, and the two shapes' male,
/// whose mirror moves them, with the 90° bit.
#[test]
fn generated_inlays_pass_s17() {
    let star = small_star();
    let pass = ClearingPass {
        tool: ToolId::new("t3"),
        strategy: ClearanceStrategy::Offset,
        direction: CutDirection::Climb,
        entry: Entry::Plunge,
    };
    for side in [InlaySide::Female, InlaySide::Male] {
        for clearing in [Vec::new(), vec![pass.clone()]] {
            let op = InlayOp {
                clearing: clearing.clone(),
                ..inlay(side, "t5", 1.0, 4.0)
            };
            let programs =
                export_inlay(&star, &op).unwrap_or_else(|e| panic!("{side:?} {clearing:?}: {e}"));
            assert_eq!(programs.len(), 1 + clearing.len(), "{side:?}");
        }
    }
    let male = inlay(InlaySide::Male, "t2", 1.0, 3.0);
    export_inlay(&two_shapes(), &male).unwrap_or_else(|e| panic!("two shapes: {e}"));
}

/// The male's surround is the targets' mirrored about X 43, not as drawn:
/// a cut 1.5 mm below the rectangle as drawn, at X 30, is off its
/// material, and the same cut below the mirrored rectangle, at X 56, is
/// in it. Both are 0.5 mm below the start plane, a 0.5 mm disc 1.5 mm
/// from the rectangle and from the surround's edge.
#[test]
fn male_surround_is_derived_from_the_mirrored_targets() {
    let project = two_shapes();
    let op = inlay(InlaySide::Male, "t2", 1.0, 3.0);
    let cut = "G1 Z-2 F300\nX{x} F1000\n";
    let at = |x: f64| program_at(x, 18.5, &cut.replace("{x}", &format!("{}", x + 1.0)));
    check_male(&at(56.0), &project, &op).unwrap();
    assert_cone_off(check_male(&at(30.0), &project, &op), 6);
}

/// Security review H4: the male's flank is engaged from the stock top,
/// not from its start plane, 1.5 mm below it. With a 2.5 mm plug depth
/// and a 6 mm margin, a cut 2 mm below the start plane (machine Z −3.5)
/// fits the cone's disc but engages 3.5 mm of the 3.175 mm cone and
/// fails; one at Z −3 engages 3 mm and passes. From the start plane both
/// would be within it.
#[test]
fn male_shank_check_counts_from_the_stock_top() {
    let project = two_shapes();
    let op = inlay(InlaySide::Male, "t2", 2.5, 6.0);
    let at = |z: f64| program_at(56.0, 17.0, &format!("G1 Z{z} F300\nX57 F1000\n"));
    check_male(&at(-3.0), &project, &op).unwrap();
    match check_male(&at(-3.5), &project, &op) {
        Err(ExportError::ConeHeightExceeded {
            line: 6,
            engaged_mm,
            cone_mm,
            ..
        }) => {
            assert!((engaged_mm - 3.5).abs() < 1e-6, "{engaged_mm}");
            assert!((cone_mm - 3.175).abs() < 1e-9, "{cone_mm}");
        }
        other => panic!("expected ConeHeightExceeded at line 6: {other:?}"),
    }
}

/// The plug is the mirrored design: a cut inside the mirrored rectangle
/// (X 60, Y 25) fails S17, as does a cut in its surround that runs
/// sideways above the male's start plane, which is not the stock top.
#[test]
fn male_cut_inside_the_plug_fails_s17() {
    let project = two_shapes();
    let op = inlay(InlaySide::Male, "t2", 1.0, 3.0);
    assert_cone_off(
        check_male(
            &program_at(60.0, 25.0, "G1 Z-2 F300\nX61 F1000\n"),
            &project,
            &op,
        ),
        6,
    );
    assert_cone_off(
        check_male(
            &program_at(56.0, 18.5, "G1 Z-1 F300\nX57 F1000\n"),
            &project,
            &op,
        ),
        7,
    );
}

// Format 23 (#443): a male's cut-out run is held to the floor ring.

/// A cut-out with the 3.175 mm end mill `t3` and `tabs`, 0.2 mm
/// through.
fn cut_out(tabs: Option<dc_project::TabSpec>) -> dc_project::InlayCutOut {
    dc_project::InlayCutOut {
        tool: ToolId::new("t3"),
        tabs,
        through_cut_allowance_mm: 0.2,
    }
}

/// Four rect tabs 4 mm wide and 2 mm tall.
fn four_tabs() -> dc_project::TabSpec {
    dc_project::TabSpec {
        placement: dc_project::TabPlacement::Count { count: 4 },
        width_mm: 4.0,
        height_mm: 2.0,
        shape: dc_project::TabShape::Rect,
        on_holes: false,
        avoid_corners: true,
    }
}

/// S9 alone on hand-written `text`, as run `label` of `op` cut with
/// `tool` in `project` (which gains `op`).
fn check_run(
    text: &str,
    project: &Project,
    op: &InlayOp,
    label: &RunLabel,
    tool: &str,
) -> Result<(), ExportError> {
    let mut project = project.clone();
    project.operations = vec![Operation::Inlay(op.clone())];
    let tool = project
        .tools
        .iter()
        .find(|t| t.id.as_str() == tool)
        .cloned()
        .expect("the tool");
    let op = Operation::Inlay(op.clone());
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let material = Material::of_run(&project, &op, label, &tool);
    let tabs = material::run_tab_spines(&project, &op, label);
    let toolpath = Toolpath::default();
    let span = OpSpan {
        op: &op,
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

#[track_caller]
fn assert_off(result: Result<(), ExportError>, line: usize) {
    assert!(
        matches!(result, Err(ExportError::OffMaterial { line: l, .. }) if l == line),
        "expected OffMaterial at line {line}: {result:?}"
    );
}

/// #443: the two shapes' male with the 90° bit, `P` 1 and a 6 mm
/// margin: its floor ring runs from 1 mm to 5 mm off the mirrored
/// rectangle (X 46 to 76, Y 20 to 30), so the 3.175 mm end mill's centre
/// may run from 2.59 to 3.41 mm off it. A cut-out 3 mm below it, through
/// the board, passes; 1.5 mm or 4.5 mm below it, its footprint crosses
/// the plug's wall or the band's outer wall, and below the rectangle as
/// drawn (X 20) it is off the ring: each fails S9 at the cut. A cut-out
/// run with another tool than the cut-out's, or of an Inlay with no
/// cut-out, is unresolved.
#[test]
fn cut_out_off_the_floor_ring_fails() {
    let project = two_shapes();
    let op = InlayOp {
        cut_out: Some(cut_out(None)),
        ..inlay(InlaySide::Male, "t2", 1.0, 6.0)
    };
    let label = RunLabel::CutOut(ToolId::new("t3"));
    let at = |x: f64, y: f64| program_at(x, y, &format!("G1 Z-12.2 F300\nX{} F1000\n", x + 5.0));
    check_run(&at(55.0, 17.0), &project, &op, &label, "t3").unwrap();
    check_run(&at(55.0, 17.4), &project, &op, &label, "t3").unwrap();
    for (x, y) in [(55.0, 18.5), (55.0, 15.5), (20.0, 17.0)] {
        assert_off(check_run(&at(x, y), &project, &op, &label, "t3"), 6);
    }
    // Below the ring the cut is held by S1, not S9: one deeper is still in
    // the ring.
    let deeper = program_at(55.0, 17.0, "G1 Z-13 F300\nX60 F1000\n");
    check_run(&deeper, &project, &op, &label, "t3").unwrap();
    // The wrong tool, or no cut-out.
    assert_off(check_run(&at(55.0, 17.0), &project, &op, &label, "t2"), 6);
    let without = inlay(InlaySide::Male, "t2", 1.0, 6.0);
    assert_off(
        check_run(&at(55.0, 17.0), &project, &without, &label, "t3"),
        6,
    );
    let female = InlayOp {
        side: InlaySide::Female,
        ..op.clone()
    };
    assert_off(
        check_run(&at(55.0, 17.0), &project, &female, &label, "t3"),
        6,
    );
}

/// #443: a cut-out's S1 floor is its own through-cut allowance, not the
/// Inlay's: 0.2 mm below the stock bottom passes, 0.3 mm fails.
#[test]
fn cut_out_is_held_to_its_own_allowance() {
    let project = two_shapes();
    let op = InlayOp {
        cut_out: Some(cut_out(Some(four_tabs()))),
        ..inlay(InlaySide::Male, "t2", 1.0, 6.0)
    };
    let mut programs = export_inlay(&project, &op).expect("the male exports");
    let cut_out = programs.pop().expect("a cut-out program");
    assert!(cut_out.contains("Z-12.200"), "{cut_out}");
    assert!(!cut_out.contains("Z-12.201"), "{cut_out}");
}

/// #443: every generated cut-out passes the whole safety pass, S1 to
/// S18 with S9's floor ring and tab rules: the small star's male with the
/// 60° bit and a 6 mm margin, with and without tabs and a clearing pass,
/// and the two shapes' male, whose mirror moves them, with the 90° bit.
/// The cut-out is the last program, with its own tool.
#[test]
fn generated_cut_outs_pass() {
    let star = small_star();
    let pass = ClearingPass {
        tool: ToolId::new("t3"),
        strategy: ClearanceStrategy::Offset,
        direction: CutDirection::Climb,
        entry: Entry::Plunge,
    };
    for clearing in [Vec::new(), vec![pass.clone()]] {
        for tabs in [None, Some(four_tabs())] {
            let op = InlayOp {
                clearing: clearing.clone(),
                cut_out: Some(cut_out(tabs.clone())),
                ..inlay(InlaySide::Male, "t5", 1.0, 6.0)
            };
            let programs =
                export_inlay(&star, &op).unwrap_or_else(|e| panic!("{clearing:?} {tabs:?}: {e}"));
            assert_eq!(programs.len(), 2 + clearing.len());
            let last = programs.last().expect("a cut-out program");
            assert!(last.contains("cut-out t3"), "{last}");
            assert!(last.contains("Z-12.200"), "{last}");
        }
    }
    let male = InlayOp {
        cut_out: Some(cut_out(Some(four_tabs()))),
        ..inlay(InlaySide::Male, "t2", 1.0, 6.0)
    };
    export_inlay(&two_shapes(), &male).unwrap_or_else(|e| panic!("two shapes: {e}"));
}
