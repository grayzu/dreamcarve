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
    assert_cone_off, check_male, edge_pass, export_inlay, inlay, program_at, small_star, two_shapes,
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

/// The 6 mm test part's pair (functional spec, "Tapered ball nose carve
/// and inlay", item 2): the "DC" letters of `tests/fixtures/dc.dcv` as an
/// Inlay with the Ultra-High Detail bit, `depth_mm` 6, `glue_gap_mm`
/// 0.85, `plug_depth_mm` 2, `margin_mm` 8, `outline_rounding_mm` 10, the
/// 3.175 mm end mill's clearing pass and cut-out with four tabs, the male
/// board 19 mm thick. As generated: the female's floor at Z 6, inset
/// `w(6)` = 1.044 mm; the male's floor 7.15 mm below the male board's top
/// (`D − G` = 5.15 mm to its start plane, against 19.05 mm of flute),
/// with 11.85 mm of board under it, so no `inlay_plug_base_thin`; the
/// points rounded to `R_min` = 0.983 mm; the band's foot `w(2)` =
/// 0.574 mm and flank `w(7.15)` = 1.179 mm, its edge open; the seat line
/// 0.55 mm per 0.5°. Every program passes S1 to S19 and the pair seats
/// within 0.05 mm by heightfield.
#[test]
fn six_mm_tapered_pair_exports_and_seats() {
    let mut project = dc_project::load(include_bytes!("../../../../../tests/fixtures/dc.dcv"))
        .expect("the DC pair loads")
        .project;
    let ultra = project
        .tools
        .iter_mut()
        .find(|t| t.id.as_str() == "t3")
        .expect("the fixture's V-bit");
    *ultra = Tool {
        id: ToolId::new("t3"),
        ..tool_tbn_ultra()
    };
    let Some(Operation::Inlay(mut inlay)) = project.operations.first().cloned() else {
        panic!("the fixture's Inlay");
    };
    inlay.depth_mm = Some(6.0);
    inlay.glue_gap_mm = Some(0.85);
    inlay.plug_depth_mm = Some(2.0);
    inlay.male_board.thickness_mm = Some(19.0);
    project.operations = vec![Operation::Inlay(inlay.clone())];
    let problems: Vec<_> = dc_project::validate(&project)
        .into_iter()
        .filter(|p| p.code() != "unused_tool")
        .collect();
    assert_eq!(problems, vec![], "{problems:?}");

    let (runs, warnings) = dc_cam::generate_inlay(&project, &inlay)
        .unwrap_or_else(|e| panic!("the pair generates: {e}\n{e:?}"));
    let lowest = |board: Board, label: &RunLabel| {
        runs.iter()
            .filter(|r| r.board == board && &r.label == label)
            .flat_map(|r| r.toolpath.moves.iter())
            .filter(|m| m.is_cut())
            .map(|m| m.to.z)
            .fold(f64::INFINITY, f64::min)
    };
    let female_floor_z = lowest(Board::Female, &RunLabel::Main);
    let male_floor_z = lowest(Board::Male, &RunLabel::Main);
    println!("female floor Z {female_floor_z:.4}, male floor Z {male_floor_z:.4}");
    assert!((female_floor_z - 6.0).abs() < 0.01, "{female_floor_z}");
    assert!(
        (male_floor_z - (19.0 - 7.15)).abs() < 0.01,
        "{male_floor_z}"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.code() == dc_cam::WarningCode::InlayPlugBaseThin),
        "{warnings:?}"
    );
    let rounded: Vec<(f64, f64, f64, f64)> = warnings
        .iter()
        .filter_map(|w| match w {
            dc_cam::Warning::InlayRoundedForBit {
                x_mm,
                y_mm,
                change_mm,
                radius_mm,
                ..
            } => Some((*x_mm, *y_mm, *change_mm, *radius_mm)),
            _ => None,
        })
        .collect();
    println!("rounded for the bit (x, y, change, radius): {rounded:?}");
    assert!(
        rounded.iter().any(|w| (w.3 - 0.983).abs() < 5e-4),
        "{rounded:?}"
    );
    let profile = project.tools[2].carve_profile().expect("the bit's profile");
    let (foot, flank) = (profile.half_width_mm(2.0), profile.half_width_mm(7.15));
    let floor_inset = profile.half_width_mm(6.0);
    println!("floor inset {floor_inset:.4}, foot {foot:.4}, flank {flank:.4}");
    assert!((floor_inset - 1.044).abs() < 5e-4);
    assert!((foot - 0.574).abs() < 5e-4);
    assert!((flank - 1.179).abs() < 5e-4);
    let band = dc_project::male_band(&project, &inlay).expect("the band");
    assert!(band.is_open(), "{band:?}");
    assert!((band.foot_mm - foot).abs() < 1e-9);
    assert!((band.flank_mm - (foot + flank)).abs() < 1e-9);
    let seat = dc_project::inlay_seat_sensitivity(&project, &inlay).expect("a seat line");
    println!("seat: {seat:?}");
    assert!((seat.taper_mm - 0.55).abs() < 5e-3, "{seat:?}");
    assert!((seat.tip_mm - 0.76).abs() < 5e-3, "{seat:?}");

    let ops: Vec<EmitOp> = runs
        .iter()
        .map(|run| EmitOp {
            op: Operation::Inlay(inlay.clone()),
            tool: project
                .tools
                .iter()
                .find(|t| t.id == run.tool)
                .cloned()
                .expect("the run's tool"),
            toolpath: run.toolpath.clone(),
            holes: None,
            label: run.label.clone(),
            board: run.board,
        })
        .collect();
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)
        .unwrap_or_else(|e| panic!("S1 to S19: {e}\n{e:?}"));
    println!("{} programs", programs.len());
    let fit = dc_cam::test_support::seat::fit(&project, &inlay, &runs, 0.2);
    println!(
        "seat {:.3} mm at {:?}, the male's share {:.3}",
        fit.seat_mm, fit.seat_at, fit.male_mm
    );
    assert!(fit.is_flush(), "{fit:?}");
}
