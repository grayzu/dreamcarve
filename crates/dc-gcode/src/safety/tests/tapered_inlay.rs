// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17 and S9 for an Inlay cut with a tapered ball nose (#681, slice 27e;
//! `docs/specs/phase-3/design.md`, "Safety pass", "A tapered ball nose",
//! and "Test plan → Sub-phase 3.6", the Inlay group).
//!
//! The safety pass re-derives each side's design rounded for the bit
//! (`dc_project::rounded_for_bit`), the band on the bit's profile and the
//! open edge at the stock top, from the project alone. The projects are
//! `inlay.rs`'s: the rectangle and the square (or the small star) on 12 mm
//! stock, `D` 2, `G` 0.5 (the start plane 1.5 mm down), with the bench's
//! Ultra-High Detail bit (tip 0.762 mm, 6.7° per side) as `t2`, or the
//! Fine Detail (tip 2.286 mm).

use super::inlay::{
    assert_cone_off, check_male, edge_pass, export_inlay, inlay, program_at, small_star,
    two_shapes,
};
use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use dc_cam::RunLabel;
use dc_project::test_support::{tool_tbn_fine, tool_tbn_ultra};
use dc_project::{Board, InlayCutOut, InlayOp, ToolId, VCarveOp, VCarveStrategy};

const DATE: &str = "2026-11-16";

/// `project` with its 90° V-bit `t2` replaced by `tool`.
fn with_tapered(mut project: Project, tool: Tool) -> Project {
    let t2 = project
        .tools
        .iter_mut()
        .find(|t| t.id.as_str() == "t2")
        .expect("the V-bit t2");
    *t2 = Tool {
        id: ToolId::new("t2"),
        ..tool
    };
    project
}

/// `w(d)` of the Ultra bit on its cone, by hand: `(d + δ)·tan 6.7°`.
fn ultra_cone_mm(d: f64) -> f64 {
    let (sin, tan) = (6.7_f64.to_radians().sin(), 6.7_f64.to_radians().tan());
    (d + 0.381 * (1.0 / sin - 1.0)) * tan
}

/// The male's open band edge is held at the stock top with the bit's
/// profile. The two shapes' male (margin 6, `P` 1) with the end mill's
/// pass has an open edge `B` 6 mm below the mirrored rectangle, at Y 14.
/// A cut at the floor, Z −2.5, has a disc `w(2.5)` = 0.633 mm wide at the
/// stock top: 0.7 mm from `B` it passes, 0.5 mm from it it crosses `B`
/// and fails S17, though its disc at the start plane, `w(1)` = 0.456 mm,
/// is inside the band. With a V wall the edge is held at the start plane
/// only, and the cut 0.5 mm from `B` passes.
#[test]
fn tapered_male_open_edge_is_held_at_the_stock_top() {
    let project = with_tapered(two_shapes(), tool_tbn_ultra());
    assert!((ultra_cone_mm(2.5) - 0.633).abs() < 5e-4);
    assert!((ultra_cone_mm(1.0) - 0.456).abs() < 5e-4);
    let open = InlayOp {
        clearing: vec![edge_pass()],
        ..inlay(Board::Male, "t2", 1.0, 6.0)
    };
    let band = dc_project::male_band(&project, &open).expect("the band resolves");
    assert!(band.is_open(), "{band:?}");
    let at = |y: f64| program_at(56.0, y, "G1 Z-2.5 F300\nX57 F1000\n");
    check_male(&at(14.7), &project, &open).unwrap();
    assert_cone_off(check_male(&at(14.5), &project, &open), 6);
    let v_wall = inlay(Board::Male, "t2", 1.0, 6.0);
    check_male(&at(14.5), &project, &v_wall).unwrap();
}

/// The safety pass holds an Inlay cut with a tapered ball nose to its
/// design rounded for the bit, re-derived from the project. The two
/// shapes' sharp corners are rounded by `R_min = 1.5·tan 6.7° + w_t` =
/// 0.555 mm on both boards, so a female program carved for the design as
/// drawn, which reaches each corner at the surface (a V-Carve of the
/// targets at a flat depth of `D`, the female's unrounded twin), fails
/// S17, while the Inlay's own female passes.
#[test]
fn tapered_inlay_regions_are_the_rounded_design() {
    let project = with_tapered(two_shapes(), tool_tbn_ultra());
    let female = inlay(Board::Female, "t2", 1.0, 6.0);
    export_inlay(&project, &female).expect("the rounded female passes");
    let unrounded = VCarveOp {
        id: female.id.clone(),
        name: female.name.clone(),
        enabled: true,
        targets: female.targets.clone(),
        tool: female.tool.clone(),
        strategy: VCarveStrategy::Medial,
        clearing: Vec::new(),
        flat_depth_mm: female.depth_mm,
        start_depth_mm: 0.0,
        start_plane_cleared_externally: false,
        step_over_fraction: female.step_over_fraction,
        machining_order: female.machining_order,
        through_cut_allowance_mm: female.through_cut_allowance_mm,
        safe_z_mm: female.safe_z_mm,
    };
    let mut carve_project = project.clone();
    carve_project.operations = vec![Operation::VCarve(unrounded.clone())];
    let (runs, _) = dc_cam::generate_vcarve(&carve_project, &unrounded)
        .unwrap_or_else(|e| panic!("the V-Carve generates: {e}"));
    let main = runs
        .into_iter()
        .find(|r| r.label == RunLabel::Main)
        .expect("a V run");
    let mut inlay_project = project.clone();
    inlay_project.operations = vec![Operation::Inlay(female.clone())];
    let ops = vec![EmitOp {
        op: Operation::Inlay(female.clone()),
        tool: inlay_project
            .tools
            .iter()
            .find(|t| t.id.as_str() == "t2")
            .cloned()
            .expect("the bit"),
        toolpath: main.toolpath,
        holes: None,
        label: RunLabel::Main,
        board: Board::Female,
    }];
    match emit_programs(&inlay_project, &ops, ProgramSplit::ProgramPerTool, DATE) {
        Err(ExportError::ConeOffMaterial { overlap_mm, .. }) => {
            assert!(overlap_mm > 0.01, "{overlap_mm}");
        }
        other => panic!("expected ConeOffMaterial: {other:?}"),
    }
}

/// Every generated Inlay cut with a tapered ball nose passes S17 and S9
/// through the whole safety pass, S1 to S19: the small star's female and
/// male with each bench bit, with and without a clearing pass, the male
/// with a cut-out, and the two shapes' male, whose mirror moves them.
#[test]
fn generated_tapered_inlays_pass_s17_and_s9() {
    for tool in [tool_tbn_ultra(), tool_tbn_fine()] {
        let name = tool.name.clone();
        let star = with_tapered(small_star(), tool.clone());
        for side in [Board::Female, Board::Male] {
            for clearing in [Vec::new(), vec![edge_pass()]] {
                let op = InlayOp {
                    clearing,
                    ..inlay(side, "t2", 1.0, 6.0)
                };
                export_inlay(&star, &op)
                    .unwrap_or_else(|e| panic!("{name} star {side:?}: {e}\n{e:?}"));
            }
        }
        let cut_out = InlayOp {
            clearing: vec![edge_pass()],
            cut_out: Some(InlayCutOut {
                tool: ToolId::new("t3"),
                tabs: None,
                through_cut_allowance_mm: 0.2,
            }),
            ..inlay(Board::Male, "t2", 1.0, 6.0)
        };
        export_inlay(&star, &cut_out)
            .unwrap_or_else(|e| panic!("{name} star with a cut-out: {e}\n{e:?}"));
        let shapes = with_tapered(two_shapes(), tool);
        export_inlay(&shapes, &inlay(Board::Male, "t2", 1.0, 6.0))
            .unwrap_or_else(|e| panic!("{name} two shapes: {e}\n{e:?}"));
    }
}
