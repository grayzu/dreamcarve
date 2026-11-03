// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #498: the safety pass holds each program to its own board
//! (`docs/specs/phase-3/design.md`, "Safety pass", "Boards"). One Inlay
//! cuts the female on the stock and the male on its male board; every
//! check that reads the stock (S1 with the cut-out's allowance, S7, S17's
//! stock-top plane, the cut-out's floor ring) reads the board the program
//! is for.
//!
//! The project is the small star of the Inlay tests (30 mm tip to tip, a
//! 6 mm circle, centred at (30, 30)) on 60 × 60 × 12 mm stock, with the
//! 60° V-bit `t5` and the 3.175 mm end mill `t3`: `D 2, G 0.5, P 1`,
//! margin 4, the end mill's clearing pass and a cut-out with it, 0.2 mm
//! through.

use super::inlay::{inlay, small_star};
use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use dc_cam::RunLabel;
use dc_project::{
    Board, ClearanceStrategy, ClearingPass, CutDirection, Entry, InlayCutOut, InlayOp, MaleBoard,
    OriginXy, ToolId,
};

const DATE: &str = "2026-11-03";

/// The small star's Inlay, its male board `board`: the end mill's
/// clearing pass and a cut-out with it, untabbed, 0.2 mm through.
fn star_inlay(board: MaleBoard) -> InlayOp {
    InlayOp {
        clearing: vec![ClearingPass {
            tool: ToolId::new("t3"),
            strategy: ClearanceStrategy::Offset,
            direction: CutDirection::Climb,
            entry: Entry::Plunge,
        }],
        cut_out: Some(InlayCutOut {
            tool: ToolId::new("t3"),
            tabs: None,
            through_cut_allowance_mm: 0.2,
        }),
        male_board: board,
        ..inlay(Board::Female, "t5", 1.0, 4.0)
    }
}

/// A male board `thickness_mm` thick, `width_mm` × `height_mm` when set,
/// the stock's otherwise.
fn board(width_mm: Option<f64>, height_mm: Option<f64>, thickness_mm: f64) -> MaleBoard {
    MaleBoard {
        width_mm,
        height_mm,
        thickness_mm: Some(thickness_mm),
    }
}

/// `op` generated in `project` (which gains it), both boards, as emitter
/// entries tagged with their boards.
fn entries(project: &Project, op: &InlayOp) -> (Project, Vec<EmitOp>) {
    let mut project = project.clone();
    project.operations = vec![Operation::Inlay(op.clone())];
    let (runs, _) = dc_cam::generate_inlay(&project, op)
        .unwrap_or_else(|e| panic!("the inlay generates: {e}\n{e:?}"));
    let ops = runs
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
    (project, ops)
}

/// `entries` with every move of the male's cut-out run moved `dz_mm` in Z.
fn with_cut_out_moved(entries: &[EmitOp], dz_mm: f64) -> Vec<EmitOp> {
    entries
        .iter()
        .cloned()
        .map(|mut e| {
            if matches!(e.label, RunLabel::CutOut(_)) {
                for m in &mut e.toolpath.moves {
                    if m.is_cut() {
                        m.to.z += dz_mm;
                    }
                }
            }
            e
        })
        .collect()
}

/// #498: S1 holds a male program to the male board's bottom. On 12 mm
/// stock with an 8 mm male board, the cut-out runs through the 8 mm board
/// to 0.2 mm below it and passes; the same cut-out run 4 mm deeper, as
/// it would be cut through the 12 mm stock from the male board's top (to
/// 12.2 mm below it), fails S1 below the male board's bottom, though it
/// would be within the stock's.
#[test]
fn male_program_is_held_to_the_male_boards_bottom() {
    let (project, ops) = entries(&small_star(), &star_inlay(board(None, None, 8.0)));
    let lowest = ops
        .iter()
        .filter(|e| e.board == Board::Male)
        .flat_map(|e| e.toolpath.moves.iter())
        .map(|m| m.to.z)
        .fold(f64::INFINITY, f64::min);
    dc_geometry::assert_approx_eq!(lowest, -0.2, 1e-9);
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)
        .unwrap_or_else(|e| panic!("through 8.2 mm: {e}"));
    let boards: Vec<Board> = programs.iter().map(|p| p.board).collect();
    assert_eq!(
        boards,
        [
            Board::Female,
            Board::Female,
            Board::Male,
            Board::Male,
            Board::Male
        ]
    );
    let deeper = with_cut_out_moved(&ops, -4.0);
    let result = emit_programs(&project, &deeper, ProgramSplit::ProgramPerTool, DATE);
    assert!(
        matches!(
            result,
            Err(ExportError::BelowMaterialBottom { floor_mm, .. }) if (floor_mm + 0.2).abs() < 1e-9
        ),
        "through 12.2 mm fails S1 at the male board's bottom: {result:?}"
    );
}

/// #498: S7 holds a male program to the male board's XY, and S7 runs
/// before S17. The small star's male V-bit run, its 4 mm band reaching X
/// 11 to 49, passes on a male board of the 60 mm stock's size. Moved
/// 20 mm in X it reaches past X 60: on that board it is off the +X edge
/// and fails S7; on a male board 100 mm wide (the stock still 60 mm) it
/// is on the board, so S7 passes it and S17 refuses it, off its carve.
#[test]
fn male_program_stays_on_the_male_board() {
    let shifted = |ops: &[EmitOp]| -> Vec<EmitOp> {
        ops.iter()
            .filter(|e| e.board == Board::Male && e.label == RunLabel::Main)
            .cloned()
            .map(|mut e| {
                for m in &mut e.toolpath.moves {
                    m.to.x += 20.0;
                }
                e
            })
            .collect()
    };
    let (project, ops) = entries(&small_star(), &star_inlay(board(None, None, 12.0)));
    emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)
        .unwrap_or_else(|e| panic!("on its board: {e}"));
    let result = emit_programs(&project, &shifted(&ops), ProgramSplit::ProgramPerTool, DATE);
    assert!(
        matches!(result, Err(ExportError::OffStock { side: "+X", .. })),
        "off the 60 mm male board: {result:?}"
    );

    let wide = board(Some(100.0), None, 12.0);
    let (wide_project, wide_ops) = entries(&small_star(), &star_inlay(wide));
    emit_programs(&wide_project, &wide_ops, ProgramSplit::ProgramPerTool, DATE)
        .unwrap_or_else(|e| panic!("on the wide board: {e}"));
    let result = emit_programs(
        &wide_project,
        &shifted(&wide_ops),
        ProgramSplit::ProgramPerTool,
        DATE,
    );
    assert!(
        matches!(result, Err(ExportError::ConeOffMaterial { .. })),
        "on the 100 mm male board S7 passes and S17 refuses: {result:?}"
    );
}

/// #498: every generated Inlay passes S1–S19 on both boards: the small
/// star's two boards from one operation, with a male board thinner than
/// the stock, of the stock's size, and narrower and lower than it but
/// still holding the band; with and without clearing and cut-out.
#[test]
fn generated_inlays_pass_on_both_boards() {
    for male_board in [
        board(None, None, 12.0),
        board(None, None, 9.0),
        board(Some(56.0), Some(57.0), 10.0),
    ] {
        for (clearing, cut_out) in [(false, false), (true, false), (true, true)] {
            let mut op = star_inlay(male_board.clone());
            if !clearing {
                op.clearing.clear();
            }
            if !cut_out {
                op.cut_out = None;
            }
            let (project, ops) = entries(&small_star(), &op);
            let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)
                .unwrap_or_else(|e| panic!("{male_board:?} {clearing} {cut_out}: {e}"));
            let female = programs.iter().filter(|p| p.board == Board::Female).count();
            let male = programs.iter().filter(|p| p.board == Board::Male).count();
            let runs = 1 + usize::from(clearing);
            assert_eq!(female, runs, "{male_board:?}");
            assert_eq!(male, runs + usize::from(cut_out), "{male_board:?}");
        }
    }
}

/// #498: the project's origin preset applies to the male board as to the
/// stock. With the XY origin at the centre, on 60 × 60 mm stock and a
/// 54 × 56 mm male board, the female's program is written about the
/// stock's centre (30, 30) and the male's about the male board's (27,
/// 28), though both cut the star (mirrored about its own centre line,
/// its symmetry axis) at (30, 30) in project space: the female's extents
/// are centred on 0, the male's on (3, 2).
#[test]
fn origin_preset_applies_to_the_male_board() {
    let mut project = small_star();
    project.origin.xy = OriginXy::Centre;
    let op = InlayOp {
        clearing: Vec::new(),
        cut_out: None,
        ..star_inlay(board(Some(54.0), Some(56.0), 12.0))
    };
    let (project, ops) = entries(&project, &op);
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE)
        .unwrap_or_else(|e| panic!("the inlay emits: {e}"));
    let [female, male] = programs.as_slice() else {
        panic!("one program per board: {}", programs.len());
    };
    assert_eq!((female.board, male.board), (Board::Female, Board::Male));
    let centre = |p: &crate::emit::Program| {
        let (x, y) = (
            p.extents.x.expect("the program moves in X"),
            p.extents.y.expect("the program moves in Y"),
        );
        ((x.min + x.max) / 2.0, (y.min + y.max) / 2.0)
    };
    let (fx, fy) = centre(female);
    let (mx, my) = centre(male);
    dc_geometry::assert_approx_eq!(fx, 0.0, 0.1);
    dc_geometry::assert_approx_eq!(fy, 0.0, 0.1);
    dc_geometry::assert_approx_eq!(mx, 30.0 - 27.0, 0.1);
    dc_geometry::assert_approx_eq!(my, 30.0 - 28.0, 0.1);
}
