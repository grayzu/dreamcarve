// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The safety pass and a Photo V-Carve before S19 (#644;
//! `docs/specs/phase-3/design.md`, "Safety pass", the S19 bullet, which is
//! slice 24's, #646).
//!
//! Format 27 put the Photo V-Carve in the project file before its
//! generator and its safety rule. Until both land, the pass has no
//! material to hold such a run to, so it holds it to none: every cut of a
//! program written for a Photo V-Carve fails S9, wherever it is. The
//! post-processor is the last line and does not trust its callers, so
//! this holds whatever handed it the toolpath.

use super::*;
use crate::parse::{CycleModel, parse_program_with};
use dc_cam::RunLabel;
use dc_project::test_support::photo_project;

/// A hand-written program that carves a 10 mm groove 1 mm deep inside the
/// sample picture's footprint (X 15 to 55, Y 15 to 35), within its 1.5 mm
/// maximum depth: what a Photo V-Carve's program would hold. With no
/// rule to hold it to, the pass refuses its first cut, the plunge on
/// line 6, and so the program. A program that never cuts passes: there
/// is nothing to refuse.
#[test]
fn photo_v_carve_run_is_refused_until_s19() {
    let project = photo_project();
    let op = project.operations[1].clone();
    assert!(matches!(op, Operation::PhotoVCarve(_)), "{op:?}");
    let tool = project.tools[1].clone();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let material = Material::of_run(
        &project,
        &op,
        &RunLabel::Main,
        &tool,
        dc_project::Board::Female,
    );
    assert!(matches!(material, Material::Unresolved));
    assert!(matches!(Material::of(&project, &op), Material::Unresolved));
    let tabs = material::tab_spines(&project, &op);
    assert!(tabs.is_empty());
    let toolpath = Toolpath::default();
    let tolerances = Tolerances {
        flatten_mm: project.flatten_tolerance_mm,
        arc_fit_mm: project.arc_fit_tolerance_mm,
    };
    let check = |text: &str| {
        let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
        let span = OpSpan {
            op: &op,
            tool: &tool,
            toolpath: &toolpath,
            material: &material,
            tabs: &tabs,
            lines: 1..text.lines().count() + 1,
        };
        material::check(&blocks, &post, &transform, tolerances, &span)
    };

    let groove = "G21 G90\nG54\nG0 Z5\nX20 Y20\nG0 Z1\nG1 Z-1 F300\nX30 F1000\nG0 Z5\n";
    let result = check(groove);
    assert!(
        matches!(result, Err(ExportError::OffMaterial { line: 6, .. })),
        "{result:?}"
    );
    let no_cut = "G21 G90\nG54\nG0 Z5\nX20 Y20\nG0 Z5\n";
    check(no_cut).unwrap();
}
