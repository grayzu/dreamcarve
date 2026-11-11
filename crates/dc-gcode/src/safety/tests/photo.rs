// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S19 in the safety pass (#646; `docs/specs/phase-3/design.md`, "Safety
//! pass", the S19 bullet; functional S19, S7 and acceptance 58).
//!
//! The hand-written programs carve the sample picture `b1` (X 15 to 55,
//! Y 15 to 35) of [`photo_project`] with its Photo V-Carve `o8`: the 90°
//! 6.35 mm V-bit `t2`, 1.5 mm deep at most, from the stock top, lifting
//! 1 mm between grooves. The stock top is the origin, so machine Z is
//! project Z less the 6 mm thickness: the grooves run from Z 0 down to
//! Z −1.5 and the links at Z 1; safe Z is 5. The generated ones run
//! through `emit_programs` and so through the whole safety pass.

use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use crate::parse::{CycleModel, parse_program_with};
use dc_cam::RunLabel;
use dc_geometry::{Polyline, Transform};
use dc_project::test_support::{
    gradient_bitmap, photo_project, sample_photo_v_carve, sample_pocket, tool_v_bit_90,
};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, PhotoStrategy, PhotoVCarveOp,
    PocketOp, ToolId, ToolKind,
};

const DATE: &str = "2026-11-11";

/// S9 and S19 alone on hand-written `text` as the Photo V-Carve of
/// `project`, cut with its own tool.
fn check_photo(text: &str, project: &Project) -> Result<(), ExportError> {
    let op = project
        .operations
        .iter()
        .find(|o| matches!(o, Operation::PhotoVCarve(_)))
        .cloned()
        .expect("the project has a Photo V-Carve");
    let tool = project
        .tools
        .iter()
        .find(|t| &t.id == op.tool())
        .cloned()
        .expect("its tool");
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(project, &post).unwrap();
    let material = Material::of_run(
        project,
        &op,
        &RunLabel::Main,
        &tool,
        dc_project::Board::Female,
    );
    let tabs = material::tab_spines(project, &op);
    assert!(tabs.is_empty());
    let toolpath = Toolpath::default();
    let tolerances = Tolerances {
        flatten_mm: project.flatten_tolerance_mm,
        arc_fit_mm: project.arc_fit_tolerance_mm,
    };
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
}

/// Two grooves and the link between them: a plunge at (20, 20) to 1 mm
/// (line 6), a groove to X `end_x` (7), the rise to the link height (8),
/// `across` (9), the plunge into the second groove at (30, 21) (10), the
/// groove back to X 20 (11), and the lift.
fn two_grooves(end_x: f64, depth: f64, across: &str) -> String {
    format!(
        "G21 G90\nG54\nG0 Z5\nX20 Y20\nG0 Z1\nG1 Z-{depth} F300\nX{end_x} F1000\nZ1\n{across}\nG1 Z-0.5 F300\nX20 F1000\nG0 Z5\n"
    )
}

/// What a Photo V-Carve's program holds passes: grooves inside the
/// picture, no deeper than its maximum depth, linked at the link height
/// by feed moves.
#[test]
fn hand_written_grooves_pass_s19() {
    let project = photo_project();
    check_photo(&two_grooves(30.0, 1.0, "X30 Y21"), &project).unwrap();
    // On the picture's edge, and at its maximum depth within rounding.
    check_photo(&two_grooves(55.0, 1.5, "X30 Y21"), &project).unwrap();
    check_photo(&two_grooves(55.0004, 1.5004, "X30 Y21"), &project).unwrap();
}

/// S19: a sample past the picture's footprint fails, naming the line and
/// the point in project space.
#[test]
fn groove_outside_the_picture_fails_s19() {
    let project = photo_project();
    let result = check_photo(&two_grooves(60.0, 1.0, "X30 Y21"), &project);
    assert!(
        matches!(result, Err(ExportError::GrooveOffPicture { line: 7, x_mm, y_mm, .. })
            if (x_mm - 60.0).abs() < 1e-9 && (y_mm - 20.0).abs() < 1e-9),
        "{result:?}"
    );
    // A plunge outside it fails at the plunge.
    let outside = "G21 G90\nG54\nG0 Z5\nX10 Y20\nG0 Z1\nG1 Z-1 F300\nX20 F1000\nG0 Z5\n";
    assert!(
        matches!(
            check_photo(outside, &project),
            Err(ExportError::GrooveOffPicture { line: 6, .. })
        ),
        "{:?}",
        check_photo(outside, &project)
    );
}

/// S19: a link between two grooves written as a rapid fails, and so does
/// one fed at another height than the link height.
#[test]
fn groove_link_as_a_rapid_fails_s19() {
    let project = photo_project();
    let result = check_photo(&two_grooves(30.0, 1.0, "G0 X30 Y21"), &project);
    assert!(
        matches!(result, Err(ExportError::GrooveLinkRapid { line: 9, .. })),
        "{result:?}"
    );
    let low = "G21 G90\nG54\nG0 Z5\nX20 Y20\nG0 Z1\nG1 Z-1 F300\nX30 F1000\nZ0.5\nX30 Y21\nG1 Z-0.5 F300\nX20 F1000\nG0 Z5\n";
    let result = check_photo(low, &project);
    assert!(
        matches!(result, Err(ExportError::GrooveLinkRapid { line: 9, .. })),
        "{result:?}"
    );
}

/// S19: a sample below the maximum depth fails as off the picture.
#[test]
fn groove_below_max_depth_fails_s19() {
    let project = photo_project();
    let result = check_photo(&two_grooves(30.0, 1.6, "X30 Y21"), &project);
    assert!(
        matches!(result, Err(ExportError::GrooveOffPicture { line: 6, .. })),
        "{result:?}"
    );
}

/// The S19 rule and its shank rule come from the project: a maximum
/// depth past the cone engaged from the stock top fails the shank test
/// whatever the toolpath says.
#[test]
fn photo_shank_is_held_to_the_cone() {
    let mut project = photo_project();
    if let Some(Operation::PhotoVCarve(photo)) = project.operations.last_mut() {
        photo.max_depth_mm = Some(3.5);
    }
    let result = check_photo(&two_grooves(30.0, 3.3, "X30 Y21"), &project);
    assert!(
        matches!(result, Err(ExportError::ConeHeightExceeded { line: 6, .. })),
        "{result:?}"
    );
}

/// A 6.35 mm V-bit `t2` of `angle_deg`.
fn v_bit(angle_deg: f64) -> Tool {
    Tool {
        id: ToolId::new("t2"),
        name: format!("{angle_deg}° V-bit"),
        cutter: ToolKind::VBit {
            included_angle_deg: angle_deg,
        },
        ..tool_v_bit_90()
    }
}

/// 60 × 40 × 6 mm stock with the end mill `t1` and the 30° V-bit `t2`,
/// the gradient picture 40 mm wide at (`x_mm`, 10) and `op`.
fn gradient_project(x_mm: f64, op: PhotoVCarveOp) -> Project {
    let mut project = sample_project();
    project.stock.width_mm = 60.0;
    project.stock.height_mm = 40.0;
    project.tools.push(v_bit(30.0));
    project.instances.clear();
    project.definitions.clear();
    project.bitmaps = vec![gradient_bitmap(x_mm, 10.0, 40.0)];
    project.operations = vec![Operation::PhotoVCarve(op)];
    project
}

/// The Photo V-Carve of acceptance 59: 2 mm deep at most with the 30°
/// bit.
fn acceptance_op() -> PhotoVCarveOp {
    PhotoVCarveOp {
        max_depth_mm: Some(2.0),
        ..sample_photo_v_carve()
    }
}

/// `project` with a 44 × 24 mm Pocket `o2` round the picture, `depth_mm`
/// deep, before its operations.
fn with_pocket(mut project: Project, depth_mm: f64) -> Project {
    let rect = FeatureDefinition {
        id: DefinitionId::new("d9"),
        name: "Clearing".into(),
        profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 44.0, 24.0)],
    };
    project.definitions.insert(rect.id.clone(), rect);
    project.instances.push(FeatureInstance {
        id: InstanceId::new("i9"),
        definition: DefinitionId::new("d9"),
        transform: Transform::translate(8.0, 8.0),
        name: "Clearing".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    project.operations.insert(
        0,
        Operation::Pocket(PocketOp {
            targets: vec![InstanceId::new("i9")],
            depth_mm: Some(depth_mm),
            ..sample_pocket()
        }),
    );
    project
}

/// The Photo V-Carve `photo`'s toolpath, generated on `project`.
fn generated(project: &Project, photo: &PhotoVCarveOp) -> Toolpath {
    dc_cam::generate_photo(project, photo)
        .expect("it generates")
        .0
}

/// Every operation of `project` exported, its Pocket generated and its
/// Photo V-Carve's toolpath `photo_toolpath` gives.
fn export(
    project: &Project,
    photo_toolpath: impl Fn(&Project, &PhotoVCarveOp) -> Toolpath,
) -> Result<Vec<String>, ExportError> {
    let ops: Vec<EmitOp> = project
        .operations
        .iter()
        .map(|op| {
            let tool = project
                .tools
                .iter()
                .find(|t| &t.id == op.tool())
                .cloned()
                .expect("its tool");
            let toolpath = match op {
                Operation::PhotoVCarve(photo) => photo_toolpath(project, photo),
                Operation::Pocket(pocket) => {
                    dc_cam::generate_pocket(project, pocket)
                        .expect("it generates")
                        .0
                }
                _ => unreachable!("only a Pocket and a Photo V-Carve"),
            };
            EmitOp::from((op.clone(), tool, toolpath))
        })
        .collect();
    let (programs, _) = emit_programs(project, &ops, ProgramSplit::ProgramPerTool, DATE)?;
    Ok(programs.into_iter().map(|p| p.text).collect())
}

/// Every generated Photo V-Carve passes the whole safety pass, S19
/// included: raster and hatch, `invert` on and off, at 0°, 22.5°, 90°
/// and 135°, from the stock top, from a Pocket's floor and from a start
/// plane the user vouches for, with arc fitting on (it never fits a
/// groove) and off, in millimetres and in inches.
#[test]
fn generated_photos_pass_s19() {
    let mut cases: Vec<(String, Project)> = Vec::new();
    for strategy in [PhotoStrategy::Raster, PhotoStrategy::Hatch] {
        for invert in [false, true] {
            for angle in [0.0, 22.5, 90.0, 135.0] {
                let op = PhotoVCarveOp {
                    strategy,
                    invert,
                    line_angle_deg: angle,
                    ..acceptance_op()
                };
                cases.push((
                    format!("{strategy:?} invert {invert} {angle}°"),
                    gradient_project(10.0, op),
                ));
            }
        }
    }
    let pocketed = with_pocket(
        gradient_project(
            10.0,
            PhotoVCarveOp {
                start_depth_mm: 1.0,
                ..acceptance_op()
            },
        ),
        1.0,
    );
    cases.push(("from a Pocket's floor".into(), pocketed));
    cases.push((
        "vouched".into(),
        gradient_project(
            10.0,
            PhotoVCarveOp {
                start_depth_mm: 2.0,
                start_plane_cleared_externally: true,
                ..acceptance_op()
            },
        ),
    ));
    let mut no_fit = gradient_project(10.0, acceptance_op());
    no_fit.arc_fit_tolerance_mm = None;
    cases.push(("arc fitting off".into(), no_fit));
    let mut inches = gradient_project(10.0, acceptance_op());
    inches.display_units = dc_project::Units::Inch;
    cases.push(("inches".into(), inches));

    for (name, project) in &cases {
        let programs = export(project, generated).unwrap_or_else(|e| panic!("{name}: {e}"));
        let text = programs.join("\n");
        assert!(
            !text.contains("G2 ") && !text.contains("G3 "),
            "{name}: an arc"
        );
    }
}

/// S7 holds a groove's width, not S19: a full-depth groove at the
/// picture's edge reaches half its width past the footprint. On the
/// all-black left edge of the gradient, the grooves at 90° along the
/// picture's left edge are 2 mm deep, 0.536 mm wide each side with the
/// 30° bit. With the picture 0.6 mm in from the stock's edge they pass
/// every rule; at 0.3 mm they pass S19 and fail S7 by 0.236 mm.
#[test]
fn edge_groove_width_is_s7s_business() {
    let op = PhotoVCarveOp {
        line_angle_deg: 90.0,
        ..acceptance_op()
    };
    let shift = |by_mm: f64| {
        move |toolpath: Toolpath| Toolpath {
            moves: toolpath
                .moves
                .into_iter()
                .map(|mut m| {
                    m.to.x -= by_mm;
                    m
                })
                .collect(),
        }
    };
    // Generated at X 1, then moved with the picture to X 0.6 and 0.3.
    let at_one = gradient_project(1.0, op.clone());
    let moved = |by_mm: f64| {
        let at_one = at_one.clone();
        move |_: &Project, photo: &PhotoVCarveOp| shift(by_mm)(generated(&at_one, photo))
    };
    let mut inside = gradient_project(0.6, op);
    export(&inside, moved(0.4)).unwrap();
    inside.bitmaps[0].x_mm = 0.3;
    let result = export(&inside, moved(0.7));
    assert!(
        matches!(&result, Err(ExportError::OffStock { side: "-X", overhang_mm, .. })
            if (overhang_mm - (2.0 * 15f64.to_radians().tan() - 0.3)).abs() < 0.002),
        "{result:?}"
    );
    // S19 alone passes a full-depth groove along that edge: it is in the
    // picture.
    let edge = "G21 G90\nG54\nG0 Z5\nX0.3 Y30\nG0 Z1\nG1 Z-2 F300\nY10 F1000\nG0 Z5\n";
    check_photo(edge, &inside).unwrap();
}
