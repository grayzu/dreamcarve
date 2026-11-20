// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `step_over_too_fine`: a stepover whose passes are closer than
//! [`MIN_PASS_SPACING_MM`] warns where the fraction in effect is set, by
//! the width each generator multiplies it by, and nothing else changes.

use super::super::*;
use super::only_located;
use crate::model::Operation;
use crate::test_support::{
    photo_project, sample_inlay, sample_pocket, sample_project, sample_v_carve, tapered_project,
    tool_tbn_fine, tool_v_bit_90,
};

/// The `step_over_too_fine` problems among `problems`.
fn too_fine(problems: &[Problem]) -> Vec<&Problem> {
    problems
        .iter()
        .filter(|p| p.code() == "step_over_too_fine")
        .collect()
}

/// The sample project with the 90° V-bit `t2` and a V-Carve `o6` of its
/// rectangle, flat 1.5 mm down, where the bit cuts 3 mm wide.
fn flat_v_carve_project() -> Project {
    let mut p = sample_project();
    p.tools.push(tool_v_bit_90());
    p.operations.push(Operation::VCarve(VCarveOp {
        flat_depth_mm: Some(1.5),
        ..sample_v_carve()
    }));
    p
}

/// The V-Carve of [`flat_v_carve_project`].
fn carve_mut(p: &mut Project) -> &mut VCarveOp {
    match p.operations.last_mut() {
        Some(Operation::VCarve(carve)) => carve,
        _ => panic!("the project ends with its V-Carve"),
    }
}

/// The bound is 0.02 mm, on the distance between passes: at it a stepover
/// is taken as meant, under it not.
#[test]
#[allow(clippy::float_cmp, reason = "the constant is this decimal")]
fn bound_is_two_hundredths_of_a_millimetre() {
    assert_eq!(MIN_PASS_SPACING_MM, 0.02);
    let mut p = sample_project();
    p.tools[0].diameter_mm = 10.0;
    p.tools[0].step_over_fraction = Some(0.002);
    assert_eq!(validate(&p), vec![], "0.02 mm between passes");
    p.tools[0].step_over_fraction = Some(0.0019);
    assert_eq!(too_fine(&validate(&p)).len(), 1, "0.019 mm");
}

/// The maintainer's case, 2026-11-20: 0.08 typed into a percentage field
/// for 8 % gave a 6.35 mm tool `step_over_fraction` 0.0008, 0.005 mm
/// between passes. A warning at the tool's field, naming the value as a
/// percentage, the distance and the percentage most likely meant.
#[test]
fn tool_step_over_of_a_few_microns_warns() {
    let mut p = sample_project();
    p.tools[0].step_over_fraction = Some(0.0008);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].code(), "step_over_too_fine");
    assert_eq!(problems[0].field().unwrap(), "tools[t1].step_over_fraction");
    assert_eq!(
        message,
        "tool `6.35 mm upcut` (`t1`) has a stepover of 0.08 % of its 6.35 mm diameter: 0.005 mm between passes, too fine to cut; 8 % is 0.508 mm"
    );
    assert_eq!(
        problems[0],
        Problem::StepOverTooFine {
            field: "tools[t1].step_over_fraction".into(),
            passes: FinePasses::Tool {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
            },
            fraction: 0.0008,
            width_mm: 6.35,
        }
    );
}

/// A vendor's stepovers are not caught: 0.0072 in on a 0.25 in bit
/// (0.0288, 0.183 mm) and the finest in a vendor's published library,
/// 0.0024 in on 0.25 in (0.0096, 0.061 mm).
#[test]
fn vendor_step_overs_do_not_warn() {
    for fraction in [0.0288, 0.0096] {
        let mut p = sample_project();
        p.tools[0].step_over_fraction = Some(fraction);
        assert_eq!(validate(&p), vec![], "{fraction}");
        // On a tapered ball nose too, whose V-Carve has no flat floor.
        let mut p = tapered_project();
        p.tools[1].step_over_fraction = Some(fraction);
        assert_eq!(validate(&p), vec![], "{fraction}");
    }
}

/// A slip so small that a hundred times it still does not cut gets no
/// guess at what was meant, and a distance under a micron is not printed
/// as 0.
#[test]
fn message_makes_no_guess_it_cannot_stand_behind() {
    let mut p = sample_project();
    p.tools[0].step_over_fraction = Some(0.000_01);
    let problems = validate(&p);
    assert_eq!(
        only_located(&p, &problems, Severity::Warning),
        "tool `6.35 mm upcut` (`t1`) has a stepover of 0.001 % of its 6.35 mm diameter: under 0.001 mm between passes, too fine to cut"
    );
    // 0.5 % of a 3 mm bit is 0.015 mm, and 50 % may well be meant.
    p.tools[0].diameter_mm = 3.0;
    p.tools[0].step_over_fraction = Some(0.005);
    let problems = validate(&p);
    assert_eq!(
        only_located(&p, &problems, Severity::Warning),
        "tool `6.35 mm upcut` (`t1`) has a stepover of 0.5 % of its 3 mm diameter: 0.015 mm between passes, too fine to cut; 50 % is 1.500 mm"
    );
    // 1.5 % of a 1 mm bit: a hundred times it is no fraction at all.
    p.tools[0].diameter_mm = 1.0;
    p.tools[0].step_down_mm = Some(1.0);
    p.tools[0].step_over_fraction = Some(0.015);
    let problems = validate(&p);
    assert_eq!(
        only_located(&p, &problems, Severity::Warning),
        "tool `6.35 mm upcut` (`t1`) has a stepover of 1.5 % of its 1 mm diameter: 0.015 mm between passes, too fine to cut"
    );
}

/// A sanity check, so of a used tool only: an unused tool is one warning
/// already, and a disabled operation's stepover cuts nothing.
#[test]
fn unused_tool_and_disabled_operation_are_not_judged() {
    let mut p = sample_project();
    p.tools.push(Tool {
        id: ToolId::new("t2"),
        name: "Spare".into(),
        step_over_fraction: Some(0.0008),
        ..crate::test_support::tool_6_35_upcut()
    });
    assert_eq!(
        validate(&p),
        vec![Problem::UnusedTool {
            tool: ToolId::new("t2"),
            name: "Spare".into(),
        }]
    );
    let mut p = sample_project();
    p.operations.push(Operation::Pocket(PocketOp {
        enabled: false,
        step_over_fraction: Some(0.0008),
        ..sample_pocket()
    }));
    assert_eq!(too_fine(&validate(&p)), Vec::<&Problem>::new());
}

/// A Pocket's own stepover is a fraction of its tool's diameter, and warns
/// at the Pocket's field; the tool's own, which is fine, is not named.
#[test]
fn pocket_own_step_over_warns_at_its_field() {
    let mut p = sample_project();
    p.operations.push(Operation::Pocket(PocketOp {
        step_over_fraction: Some(0.0008),
        ..sample_pocket()
    }));
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(
        problems[0].field().unwrap(),
        "operations[o2].step_over_fraction"
    );
    assert_eq!(
        message,
        "operation `Pocket` (`o2`) has a stepover of 0.08 % of the 6.35 mm diameter of tool `6.35 mm upcut`: 0.005 mm between passes, too fine to cut; 8 % is 0.508 mm"
    );
    // The Pocket's own value in range overrides a tool's that is not; the
    // tool's is still the tool's slip.
    let mut p = sample_project();
    p.tools[0].step_over_fraction = Some(0.0008);
    p.operations.push(Operation::Pocket(PocketOp {
        step_over_fraction: Some(0.4),
        ..sample_pocket()
    }));
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].field().unwrap(), "tools[t1].step_over_fraction");
}

/// A V-Carve's floor steps over a fraction of the width its bit cuts at
/// the flat depth (`dc-cam`'s `Floor::stepover`), not of the bit's
/// diameter: 0.5 % of the 3 mm a 90° V-bit cuts 1.5 mm down is 0.015 mm,
/// though 0.5 % of its 6.35 mm diameter would pass.
#[test]
fn carve_floor_step_over_is_judged_by_the_width_at_the_flat_depth() {
    let mut p = flat_v_carve_project();
    assert_eq!(validate(&p), vec![], "no stepover anywhere");
    carve_mut(&mut p).step_over_fraction = Some(0.005);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(
        problems[0].field().unwrap(),
        "operations[o6].step_over_fraction"
    );
    assert_eq!(
        message,
        "operation `V-Carve` (`o6`) has a floor stepover of 0.5 % of the 3 mm its bit cuts at the flat depth, 1.5 mm: 0.015 mm between passes, too fine to cut; 50 % is 1.500 mm"
    );
    // 1 % is 0.03 mm: fine.
    carve_mut(&mut p).step_over_fraction = Some(0.01);
    assert_eq!(validate(&p), vec![]);
    // With no flat depth there is no floor, so the fraction spaces nothing.
    carve_mut(&mut p).step_over_fraction = Some(0.0001);
    carve_mut(&mut p).flat_depth_mm = None;
    assert_eq!(validate(&p), vec![]);
}

/// A carve with no stepover of its own takes its tool's. One that is fine
/// of the tool's diameter can still be too fine of the floor's width: the
/// warning is the operation's, at the tool's field, where the value is.
/// One too fine of the diameter itself is the tool's warning alone.
#[test]
fn carve_floor_inherits_its_tools_step_over() {
    let mut p = flat_v_carve_project();
    p.tools[1].step_over_fraction = Some(0.005);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "tools[t2].step_over_fraction");
    assert_eq!(
        message,
        "operation `V-Carve` (`o6`) takes tool `90° V-bit`'s stepover for its floor, 0.5 % of the 3 mm its bit cuts at the flat depth, 1.5 mm: 0.015 mm between passes, too fine to cut; give the operation a step_over_fraction of its own"
    );
    // A vendor's finest, 0.96 %, on a floor 1 mm wide. Neither is a slip
    // to guess at: the tool's value cuts at the tool's diameter, so the
    // advice is to give the operation its own.
    carve_mut(&mut p).flat_depth_mm = Some(0.5);
    p.tools[1].step_over_fraction = Some(0.0096);
    let problems = validate(&p);
    assert_eq!(
        only_located(&p, &problems, Severity::Warning),
        "operation `V-Carve` (`o6`) takes tool `90° V-bit`'s stepover for its floor, 0.96 % of the 1 mm its bit cuts at the flat depth, 0.5 mm: 0.010 mm between passes, too fine to cut; give the operation a step_over_fraction of its own"
    );
    // The operation's own value answers it.
    carve_mut(&mut p).step_over_fraction = Some(0.1);
    assert_eq!(validate(&p), vec![]);
    // The tool's slip is reported once, at the tool.
    carve_mut(&mut p).step_over_fraction = None;
    p.tools[1].step_over_fraction = Some(0.0008);
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        matches!(
            &problems[0],
            Problem::StepOverTooFine {
                passes: FinePasses::Tool { .. },
                ..
            }
        ),
        "{problems:?}"
    );
}

/// An Inlay's floors: the female's is `depth_mm` down and the male's,
/// once its board is measured, `plug_depth_mm`; the shallower is the
/// narrower. The maintainer's Inlay, cut with a tapered ball nose whose
/// stepover was 0.0008, is the tool's warning.
#[test]
fn inlay_floor_step_over_is_judged_on_its_shallower_floor() {
    let mut p = sample_project();
    p.tools.push(tool_v_bit_90());
    p.operations.push(Operation::Inlay(InlayOp {
        step_over_fraction: Some(0.001),
        ..sample_inlay()
    }));
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(
        problems[0].field().unwrap(),
        "operations[o7].step_over_fraction"
    );
    assert_eq!(
        message,
        "operation `Inlay` (`o7`) has a floor stepover of 0.1 % of the 5 mm its bit cuts at the flat depth, 2.5 mm: 0.005 mm between passes, too fine to cut; 10 % is 0.500 mm"
    );
    // 0.5 % clears the female's 5 mm floor (0.025 mm) but not the male's
    // 3 mm one (0.015 mm), which is cut once the male board is measured.
    let inlay = |p: &mut Project| match p.operations.last_mut() {
        Some(Operation::Inlay(inlay)) => {
            inlay.step_over_fraction = Some(0.005);
            inlay.male_board.thickness_mm = Some(12.0);
        }
        _ => panic!("the project ends with its Inlay"),
    };
    let mut female = p.clone();
    match female.operations.last_mut() {
        Some(Operation::Inlay(inlay)) => inlay.step_over_fraction = Some(0.005),
        _ => panic!("the project ends with its Inlay"),
    }
    assert_eq!(too_fine(&validate(&female)), Vec::<&Problem>::new());
    inlay(&mut p);
    let problems = validate(&p);
    let found = too_fine(&problems);
    assert_eq!(found.len(), 1, "{problems:?}");
    assert!(
        matches!(
            found[0],
            Problem::StepOverTooFine {
                passes: FinePasses::Floor { depth_mm, from_tool: None, .. },
                width_mm,
                ..
            } if (*depth_mm - 1.5).abs() < 1e-12 && (*width_mm - 3.0).abs() < 1e-9
        ),
        "{found:?}"
    );

    // The maintainer's: a tapered ball nose at 0.0008 in an Inlay.
    let mut p = sample_project();
    p.tools.push(Tool {
        id: ToolId::new("t2"),
        step_over_fraction: Some(0.0008),
        ..tool_tbn_fine()
    });
    p.operations.push(Operation::Inlay(sample_inlay()));
    let problems = validate(&p);
    let found = too_fine(&problems);
    assert_eq!(found.len(), 1, "{problems:?}");
    assert_eq!(
        found[0].to_string(),
        "tool `IDC Fine Detail` (`t2`) has a stepover of 0.08 % of its 6.35 mm diameter: 0.005 mm between passes, too fine to cut; 8 % is 0.508 mm"
    );
    assert_eq!(found[0].severity(), Severity::Warning);
}

/// A Photo V-Carve's grooves are a fraction of a groove's width at
/// `max_depth_mm` apart: 0.5 % of the 3 mm a 90° V-bit cuts 1.5 mm down
/// warns at `line_spacing_fraction`, beside the overlap warning any
/// spacing below 1 has.
#[test]
fn photo_line_spacing_is_judged_by_the_groove_width() {
    let mut p = photo_project();
    assert_eq!(validate(&p), vec![]);
    match p.operations.last_mut() {
        Some(Operation::PhotoVCarve(photo)) => photo.line_spacing_fraction = 0.005,
        _ => panic!("the project ends with its Photo V-Carve"),
    }
    let problems = validate(&p);
    let codes: Vec<&str> = problems.iter().map(Problem::code).collect();
    assert_eq!(codes, ["photo_lines_overlap", "step_over_too_fine"]);
    let found = too_fine(&problems);
    assert_eq!(
        found[0].field().unwrap(),
        "operations[o8].line_spacing_fraction"
    );
    assert_eq!(
        found[0].to_string(),
        "operation `Photo V-Carve` (`o8`) has a line spacing of 0.5 % of the 3 mm a groove is wide at its max_depth_mm, 1.5 mm: 0.015 mm between grooves, too fine to cut; 50 % is 1.500 mm"
    );
}

/// A project with no such value is unchanged: the fixtures every other
/// test starts from have no `step_over_too_fine`, with their stepovers of
/// 0.4 and 0.1 and their carves 1.5 to 4 mm deep.
#[test]
fn projects_with_ordinary_step_overs_are_unchanged() {
    use crate::model::Board;
    use crate::test_support::{
        blobs_male_inlay, blobs_project, line_art_inlay, line_art_project, line_art_v_carve,
    };
    assert_eq!(validate(&sample_project()), vec![]);
    assert_eq!(validate(&tapered_project()), vec![]);
    assert_eq!(validate(&photo_project()), vec![]);
    let mut pocket = sample_project();
    pocket.operations.push(Operation::Pocket(sample_pocket()));
    assert_eq!(validate(&pocket), vec![]);
    let mut line_art = line_art_project();
    line_art.operations = vec![
        Operation::Inlay(line_art_inlay(Board::Male)),
        Operation::VCarve(line_art_v_carve()),
    ];
    let mut blobs = blobs_project();
    blobs.operations = vec![Operation::Inlay(blobs_male_inlay())];
    for p in [line_art, blobs] {
        assert_eq!(too_fine(&validate(&p)), Vec::<&Problem>::new());
    }
}
