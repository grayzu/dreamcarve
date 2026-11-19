// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 51: the clearing passes of a V-Carve
//! (`docs/specs/phase-3/design.md`, "Clearing passes"; functional 40).
//!
//! Acceptance 50's 30 × 10 mm rectangle at (20, 20) on 12 mm stock, the
//! 90° 6.35 mm V-bit `t2`, `flat_depth_mm` 2, so a 26 × 6 mm floor at
//! Z 10; `t3` is a 3.175 mm end mill.

use super::*;
use dc_cam::{RunLabel, generate_vcarve};
use dc_project::test_support::{sample_v_carve, tool_6_35_upcut, tool_v_bit_90};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, Entry, Operation, Tool, ToolId, VCarveOp,
};

const THICKNESS_MM: f64 = 12.0;
/// The floor's Z: 2 mm below the stock top.
const FLOOR_Z_MM: f64 = THICKNESS_MM - 2.0;
/// The length of the floor's boundary, 2 · (26 + 6) mm.
const TRACE_MM: f64 = 64.0;

/// The rectangle's V-Carve `o6` at `step_over_fraction`, with the end
/// mill's clearing pass (offset, climb, plunging) when `clearing`.
fn carve(step_over_fraction: f64, clearing: bool) -> (Project, VCarveOp) {
    let mut p = drawing_project(
        vec![Polyline::rect(Point::new(0.0, 0.0), 30.0, 10.0)],
        THICKNESS_MM,
    );
    p.tools.push(tool_v_bit_90());
    p.tools.push(Tool {
        id: ToolId::new("t3"),
        name: "3.175 mm end mill".into(),
        diameter_mm: 3.175,
        step_down_mm: Some(1.0),
        step_over_fraction: Some(0.4),
        ..tool_6_35_upcut()
    });
    let op = VCarveOp {
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t2"),
        flat_depth_mm: Some(2.0),
        step_over_fraction: Some(step_over_fraction),
        clearing: if clearing {
            vec![ClearingPass {
                tool: ToolId::new("t3"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }]
        } else {
            Vec::new()
        },
        ..sample_v_carve()
    };
    p.operations = vec![Operation::VCarve(op.clone())];
    (p, op)
}

/// What the V-bit's run of `op` cuts along the floor, mm: every cut level
/// at the floor's Z but the medial lines' stretch there, which runs from
/// each floor corner to the junction 5 mm in from the rectangle's end and
/// is not the floor's (acceptance 51). With it, how much of that runs
/// along the floor's boundary.
fn floor_cutting_mm(p: &Project, op: &VCarveOp) -> (f64, f64) {
    let (runs, _) = generate_vcarve(p, op).expect("the carve generates");
    let v_bit = &runs
        .iter()
        .find(|r| r.label == RunLabel::Main)
        .expect("the V-bit's run")
        .toolpath;
    let junctions = [
        Point::new(AT.0 + 5.0, AT.1 + 5.0),
        Point::new(AT.0 + 25.0, AT.1 + 5.0),
    ];
    let at_junction = |q: Point| junctions.iter().any(|j| j.distance_to(q) < 1e-3);
    let on_boundary = |a: Point, b: Point| {
        let on = |v: f64, lines: [f64; 2]| lines.iter().any(|l| (v - l).abs() < 1e-6);
        (on(a.x - AT.0, [2.0, 28.0]) && (a.x - b.x).abs() < 1e-6)
            || (on(a.y - AT.1, [2.0, 8.0]) && (a.y - b.y).abs() < 1e-6)
    };
    let (mut floor_mm, mut on_boundary_mm) = (0.0, 0.0);
    for (from, mv) in cuts(v_bit) {
        let level = (from.z - FLOOR_Z_MM).abs() < 1e-6 && (mv.to.z - FLOOR_Z_MM).abs() < 1e-6;
        let (a, b) = (from.xy(), mv.to.xy());
        if !level || at_junction(a) || at_junction(b) {
            continue;
        }
        assert!(
            matches!(mv.kind, MoveKind::Linear),
            "the floor of a rectangle is cut in straight moves: {mv:?}"
        );
        floor_mm += a.distance_to(b);
        if on_boundary(a, b) {
            on_boundary_mm += a.distance_to(b);
        }
    }
    (floor_mm, on_boundary_mm)
}

/// Acceptance 51's aside on why "under a third" is stated at the
/// recommended `step_over_fraction` 0.1: at the tool's 0.4 it cannot
/// hold, because the boundary trace, which the V-bit cuts whatever the end
/// mill cleared, is by itself over a third of the unassisted floor
/// cutting. The unassisted carve traces the 26 × 6 mm boundary once,
/// 64 mm; that is 36 % of what it cuts along the floor at 0.4 (the spec
/// said 40 %), so with the pass the V-bit's floor cutting stays above a
/// third there, and at 0.1 the same 64 mm is under a sixth.
#[test]
fn boundary_trace_alone_is_over_a_third_of_the_floor_cutting_at_0_4() {
    let (p, alone) = carve(0.4, false);
    let (alone_mm, traced_mm) = floor_cutting_mm(&p, &alone);
    assert!(
        (traced_mm - TRACE_MM).abs() < 0.01,
        "the unassisted carve traces the floor's boundary once: {traced_mm} mm"
    );
    let share = TRACE_MM / alone_mm;
    assert!(
        (share - 0.36).abs() < 0.02,
        "the trace is {share} of the {alone_mm} mm cut along the floor at 0.4"
    );
    assert!(share > 1.0 / 3.0, "{share}");

    let (p, with_pass) = carve(0.4, true);
    let (with_pass_mm, _) = floor_cutting_mm(&p, &with_pass);
    assert!(
        with_pass_mm >= TRACE_MM && with_pass_mm > alone_mm / 3.0,
        "{with_pass_mm} mm with the pass, {alone_mm} mm without"
    );

    let (p, fine) = carve(0.1, false);
    let (fine_mm, _) = floor_cutting_mm(&p, &fine);
    assert!(
        TRACE_MM / fine_mm < 1.0 / 6.0,
        "{fine_mm} mm at the floor at 0.1"
    );
}
