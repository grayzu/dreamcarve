// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #498: one Inlay cuts both boards, the female on the stock and the male
//! on the Inlay's male board (`docs/specs/phase-3/design.md`, "Algorithms
//! → Inlay", "Boards"). The star part's values throughout ([`inlay`]).

use super::*;
use dc_project::{Board, MaleBoard};

/// The star part's Inlay with a clearing pass and a cut-out, its male
/// board `board`.
fn star_on(board: MaleBoard) -> InlayOp {
    InlayOp {
        clearing: vec![clearing_pass()],
        cut_out: cut_out_male().cut_out,
        male_board: board,
        ..inlay(Board::Female)
    }
}

/// A male board of `thickness_mm`, the stock's size.
fn measured(thickness_mm: f64) -> MaleBoard {
    MaleBoard {
        width_mm: None,
        height_mm: None,
        thickness_mm: Some(thickness_mm),
    }
}

/// The runs `generate_inlay` gives `op` in `project` (which gains it),
/// with their labels and boards, in order.
fn labels_and_boards(runs: &[ToolRun]) -> Vec<(RunLabel, Board)> {
    runs.iter().map(|r| (r.label.clone(), r.board)).collect()
}

/// #498: `generate_inlay` gives both boards' runs from the one operation:
/// the female's first (its clearing pass, then its V-bit), each tagged
/// [`Board::Female`], then the male's (its clearing pass, its V-bit, its
/// cut-out), each tagged [`Board::Male`]. Each board's runs are those
/// [`generate_inlay_board`] gives that board alone, and every run passes
/// `assert_run_safe` on its own board.
#[test]
fn both_boards_from_one_operation() {
    let project = star_project(THICKNESS_MM);
    let op = star_on(measured(THICKNESS_MM));
    let p = with_op(&project, Operation::Inlay(op.clone()));
    let (runs, warnings) = generate_inlay(&p, &op).expect("the inlay generates");
    let t2 = || RunLabel::Clearing(ToolId::new("t2"));
    assert_eq!(
        labels_and_boards(&runs),
        [
            (t2(), Board::Female),
            (RunLabel::Main, Board::Female),
            (t2(), Board::Male),
            (RunLabel::Main, Board::Male),
            (RunLabel::CutOut(ToolId::new("t2")), Board::Male),
        ]
    );
    for run in &runs {
        assert_run_safe(run, &p, &Operation::Inlay(op.clone()));
    }
    let (female, _) = generate_inlay_board(&p, &op, Board::Female).expect("the female");
    let (male, male_warnings) = generate_inlay_board(&p, &op, Board::Male).expect("the male");
    assert_eq!(runs[..2], female[..]);
    assert_eq!(runs[2..], male[..]);
    // The male notes its mirror once.
    assert_eq!(
        warnings
            .iter()
            .filter(|w| matches!(w, Warning::InlayMaleMirrored { .. }))
            .count(),
        1,
        "{warnings:?}"
    );
    assert!(
        male_warnings
            .iter()
            .any(|w| matches!(w, Warning::InlayMaleMirrored { .. })),
        "{male_warnings:?}"
    );
}

/// #498: with the male board's thickness unset, the male is not cut: the
/// runs are the female's alone, and the generator warns
/// `inlay_male_board_unset`, located at `operations[o1].
/// male_board.thickness_mm`, the value to measure. No mirror is noted.
#[test]
fn male_runs_wait_for_the_board_thickness() {
    let project = star_project(THICKNESS_MM);
    let op = star_on(MaleBoard::default());
    let p = with_op(&project, Operation::Inlay(op.clone()));
    let (runs, warnings) = generate_inlay(&p, &op).expect("the inlay generates");
    assert!(
        runs.iter().all(|r| r.board == Board::Female),
        "{:?}",
        labels_and_boards(&runs)
    );
    assert_eq!(runs.len(), 2, "{:?}", labels_and_boards(&runs));
    let unset: Vec<&Warning> = warnings
        .iter()
        .filter(|w| matches!(w, Warning::InlayMaleBoardUnset { .. }))
        .collect();
    assert_eq!(unset.len(), 1, "{warnings:?}");
    assert_eq!(unset[0].code().as_str(), "inlay_male_board_unset");
    assert_eq!(
        unset[0].field().as_deref(),
        Some("operations[o1].male_board.thickness_mm")
    );
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, Warning::InlayMaleMirrored { .. })),
        "{warnings:?}"
    );
    let (male, male_warnings) = generate_inlay_board(&p, &op, Board::Male).expect("the male waits");
    assert!(male.is_empty());
    assert_eq!(
        male_warnings,
        unset.into_iter().cloned().collect::<Vec<_>>()
    );
}

/// #498: a male board's width and height default to the stock's, its
/// frame shares the stock's bottom-left, and its thickness is its own.
/// So an 8 mm male board on 100 × 100 × 12 mm stock is the stock's 100 ×
/// 100 at 8 mm (`Project::on_male_board`), and its male runs are those of
/// the same male on 100 × 100 × 8 mm stock with the male board given in
/// full; the female's runs are cut on the 12 mm stock.
#[test]
fn male_board_defaults_to_the_stock_in_xy() {
    let project = star_project(THICKNESS_MM);
    let op = star_on(measured(8.0));
    let p = with_op(&project, Operation::Inlay(op.clone()));
    assert_eq!(
        p.on_male_board(&op).stock,
        Stock {
            width_mm: 100.0,
            height_mm: 100.0,
            thickness_mm: Some(8.0),
        }
    );
    let (male, _) = generate_inlay_board(&p, &op, Board::Male).expect("the male");

    let thin = star_project(8.0);
    let given = star_on(MaleBoard {
        width_mm: Some(100.0),
        height_mm: Some(100.0),
        thickness_mm: Some(8.0),
    });
    let q = with_op(&thin, Operation::Inlay(given.clone()));
    let (same, _) = generate_inlay_board(&q, &given, Board::Male).expect("the male");
    assert_eq!(male, same);

    let (female, _) = generate_inlay_board(&p, &op, Board::Female).expect("the female");
    let top_mm = female
        .iter()
        .flat_map(|r| r.toolpath.moves.iter())
        .filter(|m| m.is_cut())
        .map(|m| m.to.z)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        top_mm <= THICKNESS_MM + 1e-9 && top_mm > 8.0,
        "the female cuts from the 12 mm stock's top: {top_mm}"
    );
    let male_top_mm = male
        .iter()
        .flat_map(|r| r.toolpath.moves.iter())
        .filter(|m| m.is_cut())
        .map(|m| m.to.z)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(male_top_mm <= 8.0 + 1e-9, "{male_top_mm}");
}

/// #498: `inlay_plug_base_thin` reads the male board, not the stock: on
/// 18 mm stock with an 8 mm male board, `D 4, G 1, P 2` floors the band
/// 5 mm down and leaves 3 mm, so both the generator and validation warn
/// (validation at the plug depth, naming the 8 mm board). With the male
/// board 12 mm thick it leaves 7 mm and neither warns, and with it unset
/// there is no male to warn about.
#[test]
fn plug_base_thin_reads_the_male_board() {
    let project = project_of(
        vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)],
        (30.0, 30.0),
        100.0,
        100.0,
    );
    let thin = |board: MaleBoard| {
        let mut p = project.clone();
        p.stock.thickness_mm = Some(18.0);
        let op = InlayOp {
            male_board: board,
            ..inlay(Board::Female)
        };
        let p = with_op(&p, Operation::Inlay(op.clone()));
        let (_, warnings) = generate_inlay(&p, &op).expect("the inlay generates");
        let generated: Vec<f64> = warnings
            .iter()
            .filter_map(|w| match w {
                Warning::InlayPlugBaseThin { base_mm, .. } => Some(*base_mm),
                _ => None,
            })
            .collect();
        let validated: Vec<(f64, f64)> = dc_project::validate(&p)
            .into_iter()
            .filter_map(|problem| match problem {
                dc_project::Problem::InlayPlugBaseThin {
                    thickness_mm,
                    base_mm,
                    ..
                } => Some((thickness_mm, base_mm)),
                _ => None,
            })
            .collect();
        (generated, validated)
    };
    let (generated, validated) = thin(measured(8.0));
    assert_eq!(generated.len(), 1, "{generated:?}");
    assert!((generated[0] - 3.0).abs() < 1e-9, "{generated:?}");
    assert_eq!(validated.len(), 1, "{validated:?}");
    assert!((validated[0].0 - 8.0).abs() < 1e-9, "{validated:?}");
    assert!((validated[0].1 - 3.0).abs() < 1e-9, "{validated:?}");
    assert_eq!(thin(measured(12.0)), (vec![], vec![]));
    assert_eq!(thin(MaleBoard::default()), (vec![], vec![]));
}
