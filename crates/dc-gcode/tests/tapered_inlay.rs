// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 61x on its own fixture (`docs/specs/phase-3/functional.md`;
//! `docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose → Inlay
//! fit"): the inlay test part's star with its points and inward corners
//! rounded 2 mm, cut with each of the bench's tapered ball nose bits at
//! `depth_mm` 4, `glue_gap_mm` 1.5, `plug_depth_mm` 2 and `margin_mm` 8.
//! The pair seats by heightfield and every program passes the safety
//! pass.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_cam::test_support::seat::{SEAT_MM, fit};
use dc_cam::{Warning, generate_inlay};
use dc_gcode::{EmitOp, ProgramSplit, emit_programs};
use dc_geometry::{Contour, Point, Polyline, Transform, Vertex, offset};
use dc_project::test_support::{sample_project, tool_6_35_upcut, tool_tbn_fine, tool_tbn_ultra};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InlayCutOut, InlayOp, InstanceId, MachiningOrder, MaleBoard, Operation,
    OperationId, Project, Stock, Tool, ToolId,
};

const DATE: &str = "2026-11-18";
const THICKNESS_MM: f64 = 12.0;
/// The rounding of the star's points and inward corners, mm.
const ROUNDING_MM: f64 = 2.0;

/// The one contour `contour` becomes when offset by `delta_mm`.
fn offset_one(contour: &Contour, delta_mm: f64) -> Contour {
    let mut pieces = offset(contour, delta_mm);
    assert_eq!(pieces.len(), 1, "the star stays one piece at {delta_mm} mm");
    pieces.remove(0)
}

/// The inlay test part's star about the origin (tips on the axes 30 mm
/// out, inner vertices on the diagonals 12 mm out) with its points and
/// its inward corners rounded [`ROUNDING_MM`]: opened by that radius,
/// which rounds the points, then closed by it, which rounds the inward
/// corners.
fn rounded_star() -> Polyline {
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    let sharp = Polyline::new(
        [
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
        .map(|&(x, y)| Vertex::line(x, y))
        .collect(),
        true,
    );
    let sharp = Contour::new(sharp).expect("the star is a contour");
    let opened = offset_one(&offset_one(&sharp, -ROUNDING_MM), ROUNDING_MM);
    let closed = offset_one(&offset_one(&opened, ROUNDING_MM), -ROUNDING_MM);
    // A point stands back `r / sin 21.5° − r` once rounded: half its angle
    // is 21.5°.
    let tip_x = closed.polyline().bounds().expect("bounds").max.x;
    let want = 30.0 - ROUNDING_MM * (1.0 / 21.5_f64.to_radians().sin() - 1.0);
    assert!((tip_x - want).abs() < 0.05, "the tip is at {tip_x}, {want}");
    closed.polyline().clone()
}

/// The rounded star with its 12 mm circle at (50, 50) on 100 × 100 × 12 mm
/// stock, `bit` as `t1` and a 3.175 mm end mill `t2`, and the Inlay of
/// acceptance 61x with its male board measured, the stock's size; with
/// `extras`, the end mill's clearing pass and a cut-out by it.
fn star_pair(bit: Tool, extras: bool) -> (Project, InlayOp) {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 100.0,
        height_mm: 100.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Star".into(),
        shape: None,
        profiles: vec![rounded_star(), Polyline::circle(Point::new(0.0, 0.0), 6.0)],
    };
    p.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: def.id.clone(),
        transform: Transform::translate(50.0, 50.0),
        name: "Star".into(),
        visible: true,
        locked: false,
        construction: false,
        group: None,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    p.definitions = std::collections::BTreeMap::from([(def.id.clone(), def)]);
    p.tools = vec![
        Tool {
            id: ToolId::new("t1"),
            step_over_fraction: Some(0.3),
            ..bit
        },
        Tool {
            id: ToolId::new("t2"),
            name: "3.175 mm end mill".into(),
            diameter_mm: 3.175,
            ..tool_6_35_upcut()
        },
    ];
    let op = InlayOp {
        id: OperationId::new("o1"),
        name: "Star".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        male_board: MaleBoard {
            width_mm: None,
            height_mm: None,
            thickness_mm: Some(THICKNESS_MM),
        },
        depth_mm: Some(4.0),
        glue_gap_mm: Some(1.5),
        plug_depth_mm: Some(2.0),
        margin_mm: Some(8.0),
        outline_rounding_mm: None,
        boundary: Vec::new(),
        clearing: if extras {
            vec![ClearingPass {
                tool: ToolId::new("t2"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }]
        } else {
            Vec::new()
        },
        step_over_fraction: Some(0.3),
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 10.0,
        cut_out: extras.then(|| InlayCutOut {
            tool: ToolId::new("t2"),
            tabs: None,
            through_cut_allowance_mm: 0.2,
        }),
    };
    p.operations = vec![Operation::Inlay(op.clone())];
    (p, op)
}

/// Acceptance 61x: the star rounded 2 mm, so that neither bench bit
/// rounds it further (no `inlay_rounded_for_bit`), at `glue_gap_mm` 1.5,
/// with the Ultra and the Fine bit, with and without a clearing pass and
/// a cut-out. The pair seats within 0.05 mm by heightfield, and both
/// boards' programs pass the whole safety pass.
#[test]
fn rounded_star_seats_at_a_glue_gap_of_1_5() {
    for (name, bit) in [("Ultra", tool_tbn_ultra()), ("Fine", tool_tbn_fine())] {
        for extras in [false, true] {
            let case = format!("{name}, clearing and cut-out {extras}");
            let (project, op) = star_pair(bit.clone(), extras);
            let (runs, warnings) = generate_inlay(&project, &op)
                .unwrap_or_else(|e| panic!("{case}: the pair generates: {e}"));
            assert!(
                !warnings
                    .iter()
                    .any(|w| matches!(w, Warning::InlayRoundedForBit { .. })),
                "{case}: {warnings:?}"
            );
            let seat = fit(&project, &op, &runs, 0.1);
            assert!(
                seat.male_mm <= SEAT_MM && seat.seat_mm <= SEAT_MM,
                "{case}: the plug stands {:.3} mm proud at {:?} (the male's share {:.3} mm at {:?})",
                seat.seat_mm,
                seat.seat_at,
                seat.male_mm,
                seat.male_at
            );

            let emitted: Vec<EmitOp> = runs
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
                    board: run.board,
                })
                .collect();
            let (programs, _) =
                emit_programs(&project, &emitted, ProgramSplit::ProgramPerTool, DATE)
                    .unwrap_or_else(|e| panic!("{case}: the safety pass refuses a program: {e}"));
            // The female's and the male's V-bit runs, and with the extras
            // the female's clearing run, the male's and the cut-out.
            assert_eq!(programs.len(), if extras { 5 } else { 2 }, "{case}");
        }
    }
}
