// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the safety pass: each rule on hand-written programs and on
//! emitted ones, with the line and values a refusal reports. The rules
//! with the most cases have a submodule each.

use super::*;
use crate::emit::{emit, emit_with};
use std::f64::consts::{FRAC_PI_2, PI};

use dc_cam::{Move, MoveKind, generate_profile};
use dc_geometry::{Point, Point3};
use dc_project::test_support::sample_project;
use dc_project::{ArcFormat, Axis, Operation, Project, SpindleControl, ToolChangeMode};

/// The sample project (100 × 100 × 6 mm stock, safe Z 11 absolute,
/// feed 1000, plunge 300, 18000 rpm, GRBL), its operation, and its
/// tool as a point: the hand-written toolpaths below run along the
/// stock's left and front edges (X = 0, Y = 0), which S7 allows only
/// for a tool of no width. The S7 tests set a diameter.
fn fixture() -> (Project, dc_project::ProfileOp, Tool) {
    let project = sample_project();
    let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
        panic!("sample project has a profile operation");
    };
    let mut tool = project.tools[0].clone();
    tool.diameter_mm = 0.0;
    (project, op, tool)
}

/// [`fixture`] with a Profile of no targets: the hand-written cuts of
/// the S7, extents and S10 tests cross the sample rectangle, and with
/// no part to keep out of, S9 passes them, so each test sees only its
/// own rule.
fn partless_fixture() -> (Project, dc_project::ProfileOp, Tool) {
    let (project, mut op, tool) = fixture();
    op.targets.clear();
    (project, op, tool)
}

/// A cut from (`x0`, `y0`) to (`x1`, `y1`) at Z 3, then a clockwise
/// half circle about (`x1` + 10, `y1`), which bulges 10 mm past both
/// its ends in +Y.
fn cut_then_half_circle(x0: f64, y0: f64, x1: f64, y1: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(x0, y0, 11.0)),
            Move::linear(Point3::new(x0, y0, 3.0), 300.0),
            Move::linear(Point3::new(x1, y1, 3.0), 1000.0),
            Move {
                kind: MoveKind::ArcCw {
                    centre: Point::new(x1 + 10.0, y1),
                },
                to: Point3::new(x1 + 20.0, y1, 3.0),
                feed_mm_min: Some(1000.0),
            },
            Move::rapid(Point3::new(x1 + 20.0, y1, 11.0)),
        ],
    }
}

/// S7: the cutter's edge must stay on the stock. With a 6 mm tool a
/// cut 3 mm in from every edge passes; one along the left edge
/// overhangs it by the radius; a half circle whose ends are 7 mm in
/// from the back edge, with the radius, but which bulges 10 mm past
/// them fails at +Y by 3 mm.
#[test]
fn cut_off_stock_fails_s7() {
    let (project, op, mut tool) = partless_fixture();
    tool.diameter_mm = 6.0;
    emit_one(
        &project,
        &op,
        &tool,
        cut_then_half_circle(3.0, 3.0, 50.0, 3.0),
    )
    .unwrap();

    let err = emit_one(
        &project,
        &op,
        &tool,
        cut_then_half_circle(0.0, 10.0, 50.0, 10.0),
    )
    .unwrap_err();
    let ExportError::OffStock {
        op: Some(at),
        side,
        overhang_mm,
        ..
    } = &err
    else {
        panic!("expected OffStock, got {err:?}");
    };
    assert_eq!(at, &op.id);
    assert_eq!(*side, "-X");
    assert!((overhang_mm - 3.0).abs() < 1e-6, "{err}");

    let err = emit_one(
        &project,
        &op,
        &tool,
        cut_then_half_circle(20.0, 90.0, 50.0, 90.0),
    )
    .unwrap_err();
    assert!(
        matches!(&err, ExportError::OffStock { side: "+Y", overhang_mm, .. }
            if (overhang_mm - 3.0).abs() < 1e-3),
        "{err}"
    );
}

/// A program's extents are its machine coordinates' range, arc
/// bulges included. The sample project's origin is the stock's
/// bottom-left corner and top face, so machine X and Y are project X
/// and Y and machine Z is project Z minus 6.
#[test]
fn extents_include_arc_bulges() {
    let (project, op, mut tool) = partless_fixture();
    tool.diameter_mm = 6.0;
    let program = emit(
        &project,
        &[(
            Operation::Profile(op),
            tool,
            cut_then_half_circle(10.0, 20.0, 40.0, 20.0),
        )
            .into()],
        "2026-10-15",
    )
    .unwrap()
    .0;
    let extents = program.extents;
    assert_eq!(extents.units, Units::Mm);
    let (x, y, z) = (extents.x.unwrap(), extents.y.unwrap(), extents.z.unwrap());
    assert!(
        (x.min - 10.0).abs() < 1e-9 && (x.max - 60.0).abs() < 1e-9,
        "{x:?}"
    );
    // The arc's ends are at Y 20; it reaches Y 30.
    assert!(
        (y.min - 20.0).abs() < 1e-9 && (y.max - 30.0).abs() < 1e-9,
        "{y:?}"
    );
    // Deepest cut 3 - 6; safe Z 11 - 6.
    assert!(
        (z.min + 3.0).abs() < 1e-9 && (z.max - 5.0).abs() < 1e-9,
        "{z:?}"
    );
}

/// Rapid in, plunge to `z`, one cut, retract.
fn one_cut(z: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::linear(Point3::new(0.0, 0.0, z), 300.0),
            Move::linear(Point3::new(10.0, 0.0, z), 1000.0),
            Move::rapid(Point3::new(10.0, 0.0, 11.0)),
        ],
    }
}

/// The material of a span in a test of another rule on a hand-written
/// program. S9 is not run on such a span: the test calls [`check_rules`]
/// or [`check_but_material`], which read nothing of a material but S1's
/// floor, and every material but a cut-out's leaves that to the
/// operation.
static NO_MATERIAL: Material = Material::Unresolved;

/// The whole safety pass on `text` ([`check_parsed`]), for the program's
/// extents.
#[allow(
    clippy::too_many_arguments,
    reason = "the program, how it was written, and what it was written for, each needed"
)]
fn check(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock: &Stock,
    stock_thickness_mm: f64,
    tolerances: Tolerances,
    heights: ProgramHeights,
    spans: &[OpSpan<'_>],
) -> Result<Extents, ExportError> {
    check_parsed(
        text,
        post,
        transform,
        stock,
        stock_thickness_mm,
        tolerances,
        heights,
        spans,
    )
    .map(|(extents, _)| extents)
}

fn emit_one(
    project: &Project,
    op: &dc_project::ProfileOp,
    tool: &Tool,
    tp: Toolpath,
) -> Result<String, ExportError> {
    emit(
        project,
        &[(Operation::Profile(op.clone()), tool.clone(), tp).into()],
        "2026-10-15",
    )
    .map(|(p, _)| p.text)
}

/// Runs S1–S5 on hand-written `text` as if it were the whole of the
/// fixture's operation.
fn check_text(text: &str) -> Result<(), ExportError> {
    check_text_as(text, SpindleControl::Automatic)
}

/// [`check_text`] with spindle control set to `control`.
fn check_text_as(text: &str, control: SpindleControl) -> Result<(), ExportError> {
    let (project, _, _) = fixture();
    check_text_on(text, project.post_processor.resolve().unwrap(), control)
}

/// [`check_text_as`] with the post-processor `post` in place of the
/// fixture's.
fn check_text_on(
    text: &str,
    post: PostProcessor,
    control: SpindleControl,
) -> Result<(), ExportError> {
    check_text_lifting(text, post, control, None)
}

/// [`check_text_on`] for a project whose tool-change machine Z is
/// `machine_lift_mm` (S16).
fn check_text_lifting(
    text: &str,
    post: PostProcessor,
    control: SpindleControl,
    machine_lift_mm: Option<f64>,
) -> Result<(), ExportError> {
    let (project, op, tool) = fixture();
    let mut post = post;
    post.feed_speed.spindle_control = control;
    let transform = MachineTransform::new(&project, &post).unwrap();
    let toolpath = Toolpath::default();
    let lines = text.lines().count() + 1;
    let op = Operation::Profile(op);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath: &toolpath,
        material: &NO_MATERIAL,
        tabs: &[],
        lines: 1..lines,
    }];
    check_rules(
        text,
        &post,
        &transform,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: project.arc_fit_tolerance_mm,
        },
        ProgramHeights {
            machine_lift_mm,
            clearance_z_mm: None,
        },
        &spans,
    )
    .map(|_| ())
}

#[test]
fn no_z_below_bottom_minus_allowance() {
    let (mut project, mut op, tool) = fixture();
    // The generated program never goes below the stock bottom.
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    emit_one(&project, &op, &tool, toolpath).unwrap();
    // Exactly at the bottom is fine; 0.1 mm below is not.
    emit_one(&project, &op, &tool, one_cut(0.0)).unwrap();
    let err = emit_one(&project, &op, &tool, one_cut(-0.1)).unwrap_err();
    assert!(
        matches!(err, ExportError::BelowMaterialBottom { z_mm, floor_mm, .. }
            if (z_mm + 0.1).abs() < 1e-6 && floor_mm.abs() < 1e-6),
        "{err:?}"
    );
    // An explicit through-cut allowance permits exactly that much.
    op.through_cut_allowance_mm = 0.2;
    if let Some(Operation::Profile(p)) = project.operations.first_mut() {
        p.through_cut_allowance_mm = 0.2;
    }
    emit_one(&project, &op, &tool, one_cut(-0.2)).unwrap();
    let err = emit_one(&project, &op, &tool, one_cut(-0.3)).unwrap_err();
    assert!(
        matches!(err, ExportError::BelowMaterialBottom { .. }),
        "{err:?}"
    );
}

#[test]
fn rapids_only_at_or_above_safe_z() {
    let (project, op, tool) = fixture();
    // A rapid that ends below safe Z.
    let mut low_end = one_cut(0.0);
    low_end.moves[3].to.z = 10.0;
    let err = emit_one(&project, &op, &tool, low_end).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowSafeZ { .. }),
        "{err:?}"
    );
    // A rapid across the part at cutting depth.
    let mut across = one_cut(0.0);
    across
        .moves
        .insert(3, Move::rapid(Point3::new(20.0, 0.0, 0.0)));
    let err = emit_one(&project, &op, &tool, across).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowSafeZ { .. }),
        "{err:?}"
    );
    // Hand-written: an XY rapid before any Z is known.
    let err = check_text("G21 G90\nG54\nG0 X1 Y1\n").unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::RapidBelowSafeZ {
                line: 3,
                z_mm: None,
                ..
            }
        ),
        "{err:?}"
    );
}

/// A ramp at (20, 20) on the fixture's 6 mm stock (machine Z 0 at the
/// stock top, safe Z 5): rapid straight down to machine Z `first`,
/// ramp to Z −1 and back along X to (20, 20), lift, rapid straight
/// down to `second` at (`x`, 20), ramp to Z −2 at (45, 20), out.
fn entry_program(first: &str, x: &str, second: &str) -> String {
    format!(
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX20 Y20\nZ{first}\nG1 X30 Z-1 F1000\nX20\nG0 Z5\nX{x}\nZ{second}\nG1 X45 Z-2\nG0 Z5\nM5\n"
    )
}

/// S2: a ramp or helix starts 1 mm above the stock top, or 1 mm above
/// the floor the operation's pass before cut there, and a rapid
/// straight down from safe Z takes the tool there. A rapid in Z alone
/// may end at or above the stock top plus the entry clearance
/// anywhere, and at or above the entry clearance over the lowest Z a
/// cut of the same operation has reached through its XY.
#[test]
fn rapid_down_to_entry_start_above_stock_is_accepted() {
    // Z 1 is the stock top plus 1 mm; Z 0 is 1 mm above the Z −1 the
    // first ramp's return along X cut through (20, 20).
    check_text(&entry_program("1", "20", "0")).unwrap();
    // Higher is fine too.
    check_text(&entry_program("1.5", "20", "0.5")).unwrap();
    // Anywhere over the stock, Z 1 is in the air.
    check_text(&entry_program("1", "25", "1")).unwrap();
    // The return cut passes through (25, 20) at Z −1 as well.
    check_text(&entry_program("1", "25", "0")).unwrap();
}

/// S2 as changed in #214 lets a rapid in Z alone stop in the air only:
/// one that ends below the stock top plus the clearance where nothing
/// was cut, or less than the clearance above the floor a cut reached
/// there, or above a floor only another operation cut, is still
/// refused, and so is a rapid in XY below safe Z.
#[test]
fn rapid_into_the_stock_is_still_rejected() {
    let below_at = |text: &str, at: usize| {
        let err = check_text(text).unwrap_err();
        assert!(
            matches!(err, ExportError::RapidBelowSafeZ { line, .. } if line == at),
            "line {at}: {err:?}"
        );
    };
    // Less than 1 mm above the stock top, nothing cut there yet.
    below_at(&entry_program("0.5", "20", "0"), 7);
    below_at(&entry_program("-0.5", "20", "0"), 7);
    // Less than 1 mm above the floor cut there.
    below_at(&entry_program("1", "20", "-0.5"), 12);
    // Not over the cut: (35, 20) is past the ramp's far end.
    below_at(&entry_program("1", "35", "0"), 12);
    // In XY below safe Z, even above the stock.
    below_at(
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX20 Y20\nZ1\nX30\nG1 Z-1 F300\nG0 Z5\nM5\n",
        8,
    );

    // A floor another operation cut does not count: the same program
    // split after the first ramp, the second op's rapid at line 12
    // stops 1 mm above the first op's floor, below the stock top.
    let (project, op, tool) = fixture();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let toolpath = Toolpath::default();
    let first = Operation::Profile(op.clone());
    let mut second_op = op;
    second_op.id = dc_project::OperationId::new("o2");
    let second = Operation::Profile(second_op);
    let text = entry_program("1", "20", "0");
    let spans = [
        OpSpan {
            op: &first,
            tool: &tool,
            toolpath: &toolpath,
            material: &NO_MATERIAL,
            tabs: &[],
            lines: 1..10,
        },
        OpSpan {
            op: &second,
            tool: &tool,
            toolpath: &toolpath,
            material: &NO_MATERIAL,
            tabs: &[],
            lines: 10..text.lines().count() + 1,
        },
    ];
    let err = check_rules(
        &text,
        &post,
        &transform,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: project.arc_fit_tolerance_mm,
        },
        ProgramHeights::default(),
        &spans,
    )
    .unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowSafeZ { line: 12, .. }),
        "{err:?}"
    );
}

/// Straight plunges at (20, 20) on the fixture's stock: rapid straight
/// down to machine Z `first`, plunge to Z −1, cut to X 30 and back,
/// lift, rapid straight down to `second`, plunge to Z −2, cut, out.
fn plunge_program(first: &str, second: &str) -> String {
    format!(
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX20 Y20\nZ{first}\nG1 Z-1 F300\nX30 F1000\nX20\nG0 Z5\nZ{second}\nG1 Z-2 F300\nX30 F1000\nG0 Z5\nM5\n"
    )
}

/// S2: a straight plunge (`entry: plunge`, an Engrave pass, a
/// corner-relief notch) is reached by the same rapid straight down
/// from safe Z as a ramp or helix, stopping 1 mm above the stock top
/// or above the floor the operation cut there; one that stops lower
/// is still refused.
#[test]
fn rapid_down_to_a_plunge_below_the_clearance_fails() {
    check_text(&plunge_program("1", "0")).unwrap();
    let below_at = |text: &str, at: usize| {
        let err = check_text(text).unwrap_err();
        assert!(
            matches!(err, ExportError::RapidBelowSafeZ { line, .. } if line == at),
            "line {at}: {err:?}"
        );
    };
    // Into the stock top's clearance where nothing was cut.
    below_at(&plunge_program("0.9", "0"), 7);
    below_at(&plunge_program("0", "0"), 7);
    // Less than 1 mm above the Z −1 the first pass cut at (20, 20).
    below_at(&plunge_program("1", "-0.1"), 12);
    below_at(&plunge_program("1", "-1"), 12);
}

#[test]
fn every_cut_has_active_feed() {
    let ok = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM5\n";
    check_text(ok).unwrap();
    let no_feed = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1\nX10\nG0 Z5\nM5\n";
    let err = check_text(no_feed).unwrap_err();
    assert!(
        matches!(err, ExportError::CutWithoutFeed { line: 7 }),
        "{err:?}"
    );
    let invented =
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F2500\nG0 Z5\nM5\n";
    let err = check_text(invented).unwrap_err();
    assert!(
        matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
        "{err:?}"
    );
    let zero = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F0\nG0 Z5\nM5\n";
    let err = check_text(zero).unwrap_err();
    assert!(
        matches!(err, ExportError::CutWithoutFeed { line: 7 }),
        "{err:?}"
    );
}

/// S3 with S10's feed scaling: a ramp or helix move, one that falls
/// while it moves in XY, runs at the cutting feed scaled down so its
/// vertical rate stays under the plunge rate, which puts its feed
/// between the tool's plunge rate (1000) and feed (300) rather than on
/// one of them. It is still derived from the tool: above the feed or
/// below the plunge rate fails, and so does such a feed on a level
/// move.
#[test]
fn descent_feed_between_plunge_and_feed_is_from_tool() {
    let ramp = |f: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z0 F300\nX4 Z-1 F{f}\nX10 F1000\nG0 Z5\nM5\n"
        )
    };
    check_text(&ramp("650")).unwrap();
    check_text(&ramp("300")).unwrap();
    check_text(&ramp("1000")).unwrap();
    for f in ["1200", "250"] {
        let err = check_text(&ramp(f)).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
            "F{f}: {err:?}"
        );
    }
    let level = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F650\nG0 Z5\nM5\n";
    let err = check_text(level).unwrap_err();
    assert!(
        matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
        "{err:?}"
    );
}

/// [`check_text`] for the fixture's Profile after `edit`.
fn check_text_for(
    text: &str,
    edit: impl FnOnce(&mut dc_project::ProfileOp),
) -> Result<(), ExportError> {
    let (project, mut op, mut tool) = fixture();
    tool.diameter_mm = 6.35;
    edit(&mut op);
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let toolpath = Toolpath::default();
    let lines = text.lines().count() + 1;
    let op = Operation::Profile(op);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath: &toolpath,
        material: &NO_MATERIAL,
        tabs: &[],
        lines: 1..lines,
    }];
    check_rules(
        text,
        &post,
        &transform,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: project.arc_fit_tolerance_mm,
        },
        ProgramHeights {
            machine_lift_mm: None,
            clearance_z_mm: None,
        },
        &spans,
    )
    .map(|_| ())
}

/// #335 (S3 with a last pass): a Profile's last pass runs its inside
/// arcs at the feed scaled by `ρ / (ρ + r)`, never below
/// `feed · a / (a + r)`, 136.05 mm/min for the fixture's 1000 mm/min
/// 6.35 mm bit at 0.5 mm. With a last pass a level cut, an arc or its
/// chords, may run at that or any feed up to the tool's; slower, or
/// faster than the feed, fails, and without a last pass a slower level
/// cut fails as before. A descent keeps its rule.
#[test]
fn s3_accepts_a_last_pass_edge_feed() {
    let arc = |f: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG2 X10 Y0 I5 J0 F{f}\nG0 Z5\nM5\n"
        )
    };
    let line = |f: &str| {
        format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F{f}\nG0 Z5\nM5\n")
    };
    let with_last_pass = |op: &mut dc_project::ProfileOp| {
        op.last_pass = Some(dc_project::LastPass {
            stock_to_leave_mm: 0.5,
            thickness_mm: None,
            reverse: false,
        });
    };
    for f in ["206.25", "136.054", "650", "1000"] {
        check_text_for(&arc(f), with_last_pass).unwrap_or_else(|e| panic!("arc F{f}: {e}"));
        check_text_for(&line(f), with_last_pass).unwrap_or_else(|e| panic!("line F{f}: {e}"));
    }
    for f in ["135", "1100"] {
        let err = check_text_for(&arc(f), with_last_pass).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
            "F{f}: {err:?}"
        );
    }
    let err = check_text(&arc("206.25")).unwrap_err();
    assert!(
        matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
        "no last pass: {err:?}"
    );
}

#[test]
fn spindle_and_setup_rules_are_checked() {
    let no_units = "G54\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
    assert!(matches!(
        check_text(no_units),
        Err(ExportError::MissingSetup { .. })
    ));
    let no_wcs = "G21\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
    assert!(matches!(
        check_text(no_wcs),
        Err(ExportError::MissingSetup { .. })
    ));
    let off = "G21 G90\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
    assert!(matches!(
        check_text(off),
        Err(ExportError::SpindleNotRunning { line: 5 })
    ));
    let no_dwell = "G21 G90\nG54\nG0 Z5\nM3 S18000\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    assert!(matches!(
        check_text(no_dwell),
        Err(ExportError::SpindleNotUpToSpeed { line: 6 })
    ));
    let left_on = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
    assert!(matches!(
        check_text(left_on),
        Err(ExportError::SpindleNotStopped)
    ));
}

#[test]
fn forbidden_words_and_unsafe_characters_are_rejected() {
    for (text, word) in [
        ("G21 G90\nG54\nM4 S1000\n", "M4"),
        ("G21 G90\nG54\nM7\n", "M7"),
        ("G21 G90\nG54\nM6 T1\n", "M6"),
        ("G21 G90\nG54\nG53 G0 Z0\n", "G53"),
        ("G21 G90\nG54\nG91\n", "G91"),
    ] {
        let err = check_text(text).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
            "{text}: {err:?}"
        );
    }
    for text in [
        "G21 (stop!)\n",
        "G21 ; what?\n",
        "G21 (caf\u{e9})\n",
        "G21 (a~b)\n",
    ] {
        assert!(
            matches!(
                check_text(text),
                Err(ExportError::UnsafeLine { line: 1, .. })
            ),
            "{text}"
        );
    }
    let long = format!("G21 ({})\n", "a".repeat(MAX_LINE_CHARS));
    assert!(matches!(
        check_text(&long),
        Err(ExportError::UnsafeLine { line: 1, .. })
    ));
}

#[test]
fn only_codes_dreamcarve_emits_are_accepted() {
    // The safety pass models only these codes; anything else (an offset,
    // a probe, a tool-length or arc-mode change) could move the machine
    // in a way the check cannot see. A motion mode is already active, so
    // the parser reads axis words on the same line as a move.
    for (code, word) in [
        ("G92 Z0", "G92"),
        ("G92.1", "G92.1"),
        ("G10 L20 P1 Z0", "G10"),
        ("G43.1 Z5", "G43.1"),
        ("G38.2 Z-10 F100", "G38.2"),
        ("G90.1", "G90.1"),
        ("G41", "G41"),
        ("G43", "G43"),
        // A canned cycle with canned cycles off.
        ("G81", "G81"),
        ("M8", "M8"),
    ] {
        let text = format!("G21 G90\nG54\nG0 Z11\n{code}\n");
        let err = check_text(&text).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { line: 4, word: w } if w == word),
            "{code}: {err:?}"
        );
    }
    for text in [
        "G21 G17 G90 G94\nG54\nG0 Z5\nM0\nM5\nM30\n",
        "G20\nG59\nM2\n",
    ] {
        assert!(check_text(text).is_ok(), "{text}");
    }
}

/// The LinuxCNC and Mach3 headers cancel cutter compensation (`G40`),
/// tool length offset (`G49`) and any canned cycle (`G80`), and select
/// incremental arc centres (`G91.1`). Each only sets modal state, to
/// what DreamCarve already assumes, so they pass with canned cycles
/// off.
#[test]
fn header_cancel_codes_are_accepted() {
    for text in [
        "G90 G17 G40 G49 G80 G94\nG21\nG54\nG0 Z11\nM30\n",
        "G90 G94 G17 G40 G49 G80\nG91.1\nG21\nG54\nG0 Z11\nM30\n",
    ] {
        assert!(check_text(text).is_ok(), "{text}: {:?}", check_text(text));
    }
}

/// #312: `G50` (scale factors to 1) and `G69` (no rotation) are
/// accepted from a definition whose header has them, Mach3 v3 and
/// UCCNC v3, and from no other: LinuxCNC, GRBL and RS274/NGC do not
/// have them, grblHAL before v4 does not write them, and on other
/// dialects `G50` means something else. grblHAL v4 writes `G50` and,
/// with no rotation on grblHAL, no `G69` (#316). `G51` and `G68`, which
/// scale and rotate what the program writes, are refused everywhere:
/// the safety pass reads the program as written.
#[test]
fn g50_g69_accepted() {
    let post = |id: &str, version: u32| {
        dc_project::bundled_definition(id, version)
            .unwrap()
            .default_ref()
            .resolve()
            .unwrap()
    };
    let cancels = "G21 G90\nG54\nG50\nG69\nG0 Z11\nM30\n";
    let with_them = [
        ("mach3 v3", post("mach3", 3)),
        ("uccnc v3", post("uccnc", 3)),
    ];
    let without = [
        ("mach3 v2", post("mach3", 2)),
        ("uccnc v2", post("uccnc", 2)),
        ("linuxcnc v2", post("linuxcnc", 2)),
        ("grblhal v3", post("grblhal", 3)),
        ("grbl v3", post("grbl", 3)),
        ("generic v1", post("generic", 1)),
    ];
    for (case, p) in &with_them {
        let ok = check_text_on(cancels, p.clone(), SpindleControl::Automatic);
        assert!(ok.is_ok(), "{case}: {ok:?}");
    }
    for (case, p) in &without {
        for word in ["G50", "G69"] {
            let text = format!("G21 G90\nG54\n{word}\nG0 Z11\nM30\n");
            let err = check_text_on(&text, p.clone(), SpindleControl::Automatic).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
                "{case} {word}: {err:?}"
            );
        }
    }
    // grblHAL v4's header has `G50` and no `G69`; grblHAL has no
    // rotation.
    let grblhal_v4 = post("grblhal", 4);
    let ok = check_text_on(
        "G21 G90\nG54\nG50\nG0 Z11\nM30\n",
        grblhal_v4.clone(),
        SpindleControl::Automatic,
    );
    assert!(ok.is_ok(), "grblhal v4: {ok:?}");
    let err = check_text_on(
        "G21 G90\nG54\nG69\nG0 Z11\nM30\n",
        grblhal_v4.clone(),
        SpindleControl::Automatic,
    )
    .unwrap_err();
    assert!(
        matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == "G69"),
        "grblhal v4 G69: {err:?}"
    );
    let with_them = [
        with_them[0].clone(),
        with_them[1].clone(),
        ("grblhal v4", grblhal_v4),
    ];
    for (case, p) in with_them.iter().chain(&without) {
        for (code, word) in [("G51 Z-1", "G51"), ("G68", "G68")] {
            let text = format!("G21 G90\nG54\nG0 Z11\n{code}\nM30\n");
            let err = check_text_on(&text, p.clone(), SpindleControl::Automatic).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { line: 4, word: w } if w == word),
                "{case} {code}: {err:?}"
            );
        }
    }
}

/// On a definition whose header writes them, `G50` and `G69` are
/// accepted only as `emit` writes them: alone on a line (a line number
/// aside), in the header or in the setup after a stop. With a value
/// word, `G50` is another command on some dialects (it would pass a
/// check that reads it as a cancel), and anywhere else it is not
/// DreamCarve's line, so both are refused with
/// [`ExportError::MisplacedSetupWord`]. The real headers pass.
#[test]
fn g50_g69_only_where_emit_writes_them() {
    let post = |id: &str| {
        dc_project::bundled_definition(id, 3)
            .unwrap()
            .default_ref()
            .resolve()
            .unwrap()
    };
    let mach3_header = "%\nN10 ( Project: x )\nN20 ( Generated by DreamCarve on 2026-10-15 )\n\
        N30 G90 G94 G17 G40 G80\nN40 G50\nN50 G69\nN60 G91.1\nN70 G21\nN80 G54\n\
        N90 G0 Z11\nN100 M30\n";
    let ok = check_text_on(mach3_header, post("mach3"), SpindleControl::Automatic);
    assert!(ok.is_ok(), "mach3: {ok:?}");
    let uccnc_header = "( Project: x )\n( Generated by DreamCarve on 2026-10-15 )\n\
        G90 G17 G40 G80 G94\nG50\nG69\nG54\nG0 Z11\nM30\n";
    let ok = check_text_on(uccnc_header, post("uccnc"), SpindleControl::Automatic);
    assert!(ok.is_ok(), "uccnc: {ok:?}");
    // Again in the setup after a stop.
    let after_stop = "G21 G90\nG54\nG50\nG69\nG0 Z11\nM0\nG50\nG69\nG21 G90\nG54\nG0 Z11\nM30\n";
    for id in ["mach3", "uccnc"] {
        let ok = check_text_on(after_stop, post(id), SpindleControl::Automatic);
        assert!(ok.is_ok(), "{id} after a stop: {ok:?}");
    }
    let misplaced = |id: &str, text: &str, line: usize, word: &str, why: &str| {
        let err = check_text_on(text, post(id), SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::MisplacedSetupWord { line: l, word: w, reason }
                    if *l == line && w == word && reason.contains(why)
            ),
            "{id}: {text}: {err:?}"
        );
    };
    for id in ["mach3", "uccnc"] {
        // With a value word, in the setup after a stop, where it would
        // pass alone. `G0` is still the motion mode there, so the
        // parser reads `G50 X0` as a move as well.
        for code in [
            "G50 X0",
            "G50 Z-1",
            "G50 F100",
            "G50 S1000",
            "G50 P1",
            "G69 X0",
            "G69 F100",
            "G50 G69",
        ] {
            let word = &code[..3];
            let text = format!("G21 G90\nG54\nG0 Z11\nM0\n{code}\nG21 G90\nG54\nG0 Z11\nM30\n");
            misplaced(id, &text, 5, word, "other words");
        }
        // In the header, the value word fails the same way where the
        // parser reads it.
        for code in ["G50 F100", "G50 S1000", "G50 P1", "G50 G69"] {
            let word = &code[..3];
            let text = format!("G21 G90\nG54\n{code}\nG0 Z11\nM30\n");
            misplaced(id, &text, 3, word, "other words");
        }
        // Alone, but after the first move, with no stop since.
        for word in ["G50", "G69"] {
            let text = format!("G21 G90\nG54\nG0 Z11\n{word}\nM30\n");
            misplaced(id, &text, 4, word, "outside");
            let spindle = format!("G21 G90\nG54\nM3 S18000\n{word}\nG0 Z11\nM5\nM30\n");
            misplaced(id, &spindle, 4, word, "outside");
            let resumed = format!("G21 G90\nG54\nG0 Z11\nM0\nG21 G90\nG54\nG0 Z11\n{word}\nM30\n");
            misplaced(id, &resumed, 8, word, "outside");
        }
    }
}

/// #316: grblHAL v4 writes `G50` in its header and the setup after a
/// stop, and the safety pass holds it to #312's rule: alone on its
/// line, only there. On grblHAL `G50` takes the axis-command slot of
/// its block (`gcode.c` 1767), so `G1 X5 G50` would drop the move and
/// `G50 X5` would drop the axis words; neither is DreamCarve's line.
/// The real header passes, and so does the setup after an `M6` in
/// mode `"m6"` and after an `M0`.
#[test]
fn grblhal_v4_g50_only_where_emit_writes_it() {
    let v4 = || {
        dc_project::bundled_definition("grblhal", 4)
            .unwrap()
            .default_ref()
            .resolve()
            .unwrap()
    };
    let header = "( Project: x )\n( Generated by DreamCarve on 2026-10-15 )\n\
        G90\nG17\nG94\nG80\nG40\nG50\nG21\nG54\nG0 Z11\nM30\n";
    let ok = check_text_on(header, v4(), SpindleControl::Automatic);
    assert!(ok.is_ok(), "header: {ok:?}");
    let after_m0 = "G21 G90\nG54\nG50\nG0 Z11\nM0\nG80\nG40\nG50\nG21 G90\nG54\nG0 Z11\nM30\n";
    let ok = check_text_on(after_m0, v4(), SpindleControl::Automatic);
    assert!(ok.is_ok(), "after M0: {ok:?}");
    let mut m6 = v4();
    m6.tool_change.mode = ToolChangeMode::M6;
    let after_m6 =
        "G21 G90\nG54\nG50\nM61 Q1\nG0 Z11\nM5\nT2 M6\nG80\nG40\nG50\nG21 G90\nG54\nG0 Z11\nM30\n";
    let ok = check_text_on(after_m6, m6.clone(), SpindleControl::Automatic);
    assert!(ok.is_ok(), "after M6: {ok:?}");
    let misplaced = |text: &str, line: usize, why: &str| {
        let err = check_text_on(text, v4(), SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::MisplacedSetupWord { line: l, word: w, reason }
                    if *l == line && w == "G50" && reason.contains(why)
            ),
            "{text}: {err:?}"
        );
    };
    for code in [
        "G50 X0",
        "G50 Z-1",
        "G50 F100",
        "G50 S1000",
        "G50 P1",
        "G1 X5 G50",
        "G0 G50",
    ] {
        let text = format!("G21 G90\nG54\nG0 Z11\nM0\n{code}\nG21 G90\nG54\nG0 Z11\nM30\n");
        misplaced(&text, 5, "other words");
    }
    for code in ["G50 F100", "G50 S1000", "G50 P1", "G90 G50", "G21 G50"] {
        let text = format!("G21 G90\nG54\n{code}\nG0 Z11\nM30\n");
        misplaced(&text, 3, "other words");
    }
    misplaced("G21 G90\nG54\nG0 Z11\nG50\nM30\n", 4, "outside");
    misplaced(
        "G21 G90\nG54\nM3 S18000\nG50\nG0 Z11\nM5\nM30\n",
        4,
        "outside",
    );
    misplaced(
        "G21 G90\nG54\nG0 Z11\nM0\nG21 G90\nG54\nG0 Z11\nG50\nM30\n",
        8,
        "outside",
    );
    // After an `M6`, but past the next move.
    let err = check_text_on(
        "G21 G90\nG54\nM61 Q1\nG0 Z11\nM5\nT2 M6\nG21 G90\nG54\nG0 Z11\nG50\nM30\n",
        m6,
        SpindleControl::Automatic,
    )
    .unwrap_err();
    assert!(
        matches!(&err, ExportError::MisplacedSetupWord { line: 10, .. }),
        "{err:?}"
    );
}

/// RS274/NGC (NIST IR 6556, 3.1) and Mach3 (Using Mach3Mill 1.84,
/// 10.5.5): a line holding only `%` delimits the program and is not
/// run.
#[test]
fn percent_line_is_a_delimiter() {
    let text = "%\nG21 G90\nG54\nG0 Z11\nM30\n";
    assert!(check_text(text).is_ok(), "{:?}", check_text(text));
    let err = check_text("G21 G90 %\nG54\n").unwrap_err();
    assert!(
        matches!(err, ExportError::Unparseable { line: 1, .. }),
        "{err:?}"
    );
}

/// UCCNC has no `G20`/`G21` and reads lengths in its profile's units
/// (the definitions README): on a definition without units commands,
/// S5 does not ask for one. On one with them it still does.
#[test]
fn units_word_not_required_where_the_controller_has_none() {
    let text = "G90 G17 G40 G49 G80 G94\nG54\nG0 Z11\nM30\n";
    let uccnc = dc_project::PostProcessorRef::latest("uccnc")
        .unwrap()
        .resolve()
        .unwrap();
    let ok = check_text_on(text, uccnc, SpindleControl::Automatic);
    assert!(ok.is_ok(), "{ok:?}");
    let err = check_text(text).unwrap_err();
    assert!(
        matches!(err, ExportError::MissingSetup { what } if what.contains("units")),
        "{err:?}"
    );
}

#[test]
fn violation_is_error_not_warning() {
    // A hand-built toolpath that plunges through the spoilboard: export
    // fails outright and yields no program to write.
    let (project, op, tool) = fixture();
    let result = emit(
        &project,
        &[(Operation::Profile(op), tool, one_cut(-1.0)).into()],
        "2026-10-15",
    );
    assert!(
        matches!(result, Err(ExportError::BelowMaterialBottom { .. })),
        "{result:?}"
    );
}

/// Runs the safety pass on `text` as [`check_text`] does, with the
/// router switched by hand.
fn check_manual(text: &str) -> Result<(), ExportError> {
    check_text_as(text, SpindleControl::Manual)
}

/// S4 (manual): a tool's first cut must follow an `M0` issued at safe
/// Z after the last cut, and the program must pause again at safe Z
/// after its last cut. Safe Z is machine Z5 in the fixture.
#[test]
fn manual_cut_without_on_pause_is_rejected() {
    let ok = "G21 G90\nG54\nG0 Z5\nM0 (MSG, Turn the router ON.)\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM0 (MSG, Turn the router OFF.)\n";
    check_manual(ok).unwrap();

    let no_pause = "G21 G90\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
    let err = check_manual(no_pause).unwrap_err();
    assert!(
        matches!(err, ExportError::RouterNotStarted { line: 5 }),
        "{err:?}"
    );

    // A tool change is not the pause to turn the router on, nor does
    // a pause before it count: the router needs its own pause after
    // the `M6`.
    let manual_grblhal = |text: &str| {
        let grblhal = grblhal_m6();
        check_text_on(text, grblhal, SpindleControl::Manual)
    };
    for text in [
        "G21 G90\nG54\nG0 Z5\nT1 M6\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n",
        "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nT1 M6\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n",
    ] {
        let line = text.lines().position(|l| l.starts_with("G1")).unwrap() + 1;
        let err = manual_grblhal(text).unwrap_err();
        assert!(
            matches!(err, ExportError::RouterNotStarted { line: l } if l == line),
            "{text:?}: {err:?}"
        );
    }
    manual_grblhal(
        "G21 G90\nG54\nG0 Z5\nT1 M6\nG21 G90 G54 G0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n",
    )
    .unwrap();

    let no_off = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
    let err = check_manual(no_off).unwrap_err();
    assert!(matches!(err, ExportError::RouterNotStopped), "{err:?}");

    // No spindle word reaches a router the user switches by hand.
    for (text, word) in [
        ("G21 G90\nG54\nM3\n", "M3"),
        ("G21 G90\nG54\nM5\n", "M5"),
        ("G21 G90\nG54\nS18000\n", "S18000"),
    ] {
        let err = check_manual(text).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
            "{text}: {err:?}"
        );
    }
}

/// S4: the automatic rules stand. The fixture is automatic, a
/// program that starts the spindle and dwells passes, and one that
/// relies on an `M0` instead of `M3` is rejected.
#[test]
fn automatic_rules_unchanged() {
    let (project, _, _) = fixture();
    assert_eq!(
        project.post_processor.spindle_control,
        SpindleControl::Automatic
    );
    let ok = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    check_text_as(ok, SpindleControl::Automatic).unwrap();
    let manual_style =
        "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
    let err = check_text_as(manual_style, SpindleControl::Automatic).unwrap_err();
    assert!(
        matches!(err, ExportError::SpindleNotRunning { line: 7 }),
        "{err:?}"
    );
    let no_dwell = "G21 G90\nG54\nG0 Z5\nM3 S18000\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    assert!(matches!(
        check_text_as(no_dwell, SpindleControl::Automatic),
        Err(ExportError::SpindleNotUpToSpeed { line: 6 })
    ));
}

/// S4: the user fits tools and switches the router at a pause, so
/// every `M0` comes where Z is known to be at or above safe Z (machine
/// Z5 in the fixture), in both spindle modes.
#[test]
fn pause_must_be_at_safe_z() {
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let below = match control {
            SpindleControl::Automatic => {
                "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nM0\n"
            }
            SpindleControl::Manual => {
                "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nM0\n"
            }
        };
        let below_line = below.lines().count();
        for (text, line) in [
            // Z never set: the program-start case.
            ("G21 G90\nG54\nM0\n", 3),
            // Below safe Z, after a cut.
            (below, below_line),
        ] {
            let err = check_text_as(text, control).unwrap_err();
            assert!(
                matches!(err, ExportError::PauseNotAtSafeZ { line: l } if l == line),
                "{control:?} {text:?}: {err:?}"
            );
        }
    }
    check_text_as(
        "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\n",
        SpindleControl::Automatic,
    )
    .unwrap();
}

/// S4: Z is not known after a pause, because the user may have fitted
/// a tool and set Z zero. The spindle starts, and the router-on pause
/// comes, only after a retract to safe Z.
#[test]
fn spindle_start_needs_a_retract_after_a_pause() {
    let no_lift = "G21 G90\nG54\nG0 Z5\nM5\nM0\nG21 G90 G54 M3 S18000\nG4 P3\nG0 Z5\nM5\n";
    let err = check_text_as(no_lift, SpindleControl::Automatic).unwrap_err();
    assert!(
        matches!(err, ExportError::SpindleStartNotAtSafeZ { line: 6 }),
        "{err:?}"
    );
    let lifted = "G21 G90\nG54\nG0 Z5\nM5\nM0\nG21 G90 G54 G0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    check_text_as(lifted, SpindleControl::Automatic).unwrap();

    let manual_no_lift = "G21 G90\nG54\nG0 Z5\nM0\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
    let err = check_manual(manual_no_lift).unwrap_err();
    assert!(
        matches!(err, ExportError::PauseNotAtSafeZ { line: 5 }),
        "{err:?}"
    );
    let manual_lifted = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
    check_manual(manual_lifted).unwrap();
}

/// S2, S4: Z is unknown after a pause, because the user may have
/// jogged or set Z zero, so an XY rapid needs a move to safe Z first.
/// This is the same "Z unknown" state the pause and spindle-start
/// checks use.
#[test]
fn xy_rapid_after_pause_needs_a_retract() {
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let no_lift = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54\nG0 X10 Y10\n";
        let err = check_text_as(no_lift, control).unwrap_err();
        assert!(
            matches!(
                err,
                ExportError::RapidBelowSafeZ {
                    line: 6,
                    z_mm: None,
                    ..
                }
            ),
            "{control:?}: {err:?}"
        );
        let lifted = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54\nG0 Z5\nG0 X10 Y10\n";
        check_text_as(lifted, control).unwrap();
    }
}

/// The same holds after an `M6` tool change, where the user sets Z
/// zero with the new bit.
#[test]
fn xy_rapid_after_m6_needs_a_retract() {
    let check = |text: &str| {
        let grblhal = grblhal_m6();
        check_text_on(text, grblhal, SpindleControl::Automatic)
    };
    let no_lift = "G21 G90\nG54\nG0 Z5\nT2 M6\nG21 G90 G54\nG0 X10 Y10\n";
    let err = check(no_lift).unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::RapidBelowSafeZ {
                line: 6,
                z_mm: None,
                ..
            }
        ),
        "{err:?}"
    );
    let lifted = "G21 G90\nG54\nG0 Z5\nT2 M6\nG21 G90 G54\nG0 Z5\nG0 X10 Y10\n";
    check(lifted).unwrap();
}

/// S5: the controller is in a known state before the program moves:
/// units, work coordinates, and absolute distance mode. A controller
/// left in `G91` by an earlier job reads every coordinate as an offset.
#[test]
fn absolute_mode_is_required_before_motion() {
    let err = check_text("G21\nG54\nG0 Z5\n").unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::MissingSetup {
                what: "absolute distance mode (G90)"
            }
        ),
        "{err:?}"
    );
    check_text("G21 G90\nG54\nG0 Z5\n").unwrap();
}

/// S5: at a pause the user may leave the controller in another state
/// (`G91` to jog, `G20`, another work coordinate system), so units,
/// work coordinates, and `G90` are set again after every pause before
/// the program moves.
#[test]
fn motion_after_a_pause_needs_setup_again() {
    let ok = "G21 G90\nG54\nG0 Z5\nM0\nG21\nG54\nG90\nG0 Z5\n";
    check_text(ok).unwrap();
    for (missing, what) in [
        ("G21\n", "units (G20 or G21)"),
        ("G54\n", "a work coordinate system (G54 to G59)"),
        ("G90\n", "absolute distance mode (G90)"),
    ] {
        let (before, after) = ok.split_once("M0\n").unwrap();
        let text = format!("{before}M0\n{}", after.replacen(missing, "", 1));
        let err = check_text(&text).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::MissingSetupAfterPause { line: 7, pause_line: 4, what: w } if *w == what
            ),
            "without {missing:?}: {err:?}"
        );
    }
}

/// The same rule holds for a spindle start after a pause.
#[test]
fn spindle_start_after_a_pause_needs_setup_again() {
    let text = "G21 G90\nG54\nG0 Z5\nM0\nG0 Z5\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
    let err = check_text(text).unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::MissingSetupAfterPause {
                line: 5,
                pause_line: 4,
                ..
            }
        ),
        "{err:?}"
    );
    let text = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54\nG0 Z5\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
    check_text(text).unwrap();
}

/// grblHAL's `M6` stops the program like `M0`, and the user fits the
/// bit and sets Z zero there. So it must come at safe Z, Z is unknown
/// after it, and the setup is written again before the program moves
/// or starts the spindle.
#[test]
fn m6_counts_as_a_pause() {
    let check = |text: &str| {
        let grblhal = grblhal_m6();
        check_text_on(text, grblhal, SpindleControl::Automatic)
    };
    let ok = "G21 G90\nG54\nG0 Z5\nM5\nT2 M6\nG21 G90 G54 G0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    check(ok).unwrap();

    // No retract after the change: Z is unknown.
    let no_lift = "G21 G90\nG54\nG0 Z5\nM5\nT2 M6\nG21 G90 G54 M3 S18000\nG4 P3\nG0 Z5\nM5\n";
    let err = check(no_lift).unwrap_err();
    assert!(
        matches!(err, ExportError::SpindleStartNotAtSafeZ { line: 6 }),
        "{err:?}"
    );

    // No setup after the change.
    let err = check("G21 G90\nG54\nG0 Z5\nM5\nT2 M6\nG0 Z5\n").unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::MissingSetupAfterPause {
                line: 6,
                pause_line: 5,
                ..
            }
        ),
        "{err:?}"
    );

    // Below safe Z after a cut, and at program start where Z is unknown.
    for (text, line) in [
        (
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nM5\nT2 M6\n",
            9,
        ),
        ("G21 G90\nG54\nT2 M6\n", 3),
    ] {
        let err = check(text).unwrap_err();
        assert!(
            matches!(err, ExportError::PauseNotAtSafeZ { line: l } if l == line),
            "{text:?}: {err:?}"
        );
    }

    // GRBL has no M6: the word is refused outright.
    let err = check_text(ok).unwrap_err();
    assert!(
        matches!(&err, ExportError::ForbiddenWord { line: 5, word } if word == "M6"),
        "{err:?}"
    );
}

/// A tool's set `number` is its `T` word, so `M61 Q<n>` and `T<n> M6`
/// carry any number, not only a place in the tool list. The `M6` and
/// `M61` checks are the same for them: accepted with `"m6"`, `M6`
/// still a pause that must be at safe Z, both refused in
/// `"separate_programs"` mode.
#[test]
fn tool_change_checks_hold_for_explicit_tool_numbers() {
    let ok = "G21 G90\nG54\nM61 Q7\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\nT12 M6\nG21 G90 G54 G0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
    check_text_on(ok, grblhal_m6(), SpindleControl::Automatic).unwrap();

    let low = "G21 G90\nG54\nM61 Q7\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nM5\nT12 M6\n";
    let err = check_text_on(low, grblhal_m6(), SpindleControl::Automatic).unwrap_err();
    assert!(
        matches!(err, ExportError::PauseNotAtSafeZ { line: 10 }),
        "{err:?}"
    );

    for (text, word) in [(ok, "M61"), ("G21 G90\nG54\nG0 Z5\nM5\nT12 M6\n", "M6")] {
        let err = check_text_on(text, bundled_grblhal(), SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { word: w, .. } if w == word),
            "{text:?}: {err:?}"
        );
    }
}

/// With `"separate_programs"`, the bundled default, each program has
/// one tool, so `M6` and `M61` are refused even on grblHAL.
#[test]
fn tool_change_words_need_m6_mode() {
    let grblhal = bundled_grblhal();
    assert_eq!(grblhal.tool_change.mode, ToolChangeMode::SeparatePrograms);
    for (text, word) in [
        ("G21 G90\nG54\nG0 Z5\nM5\nT2 M6\n", "M6"),
        ("G21 G90\nG54\nM61 Q1\n", "M61"),
    ] {
        let err = check_text_on(text, grblhal.clone(), SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { word: w, .. } if w == word),
            "{text:?}: {err:?}"
        );
    }
}

/// The latest bundled grblHAL definition with its default settings.
fn bundled_grblhal() -> PostProcessor {
    dc_project::latest_definition("grblhal")
        .unwrap()
        .post_processor()
        .clone()
}

/// The bundled grblHAL definition with tool-change mode `"m6"`: the
/// one program with `M6` tool changes.
fn grblhal_m6() -> PostProcessor {
    let mut grblhal = bundled_grblhal();
    grblhal.tool_change.mode = ToolChangeMode::M6;
    grblhal
}

/// Rapid in, plunge, a straight cut to (10, 0), a quarter circle
/// counter-clockwise about (10, 10) to (20, 10), retract. Project Z is
/// machine Z + 6 in the fixture.
fn line_then_arc() -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::linear(Point3::new(0.0, 0.0, 5.0), 300.0),
            Move::linear(Point3::new(10.0, 0.0, 5.0), 1000.0),
            Move {
                kind: MoveKind::ArcCcw {
                    centre: Point::new(10.0, 10.0),
                },
                to: Point3::new(20.0, 10.0, 5.0),
                feed_mm_min: Some(1000.0),
            },
            Move::rapid(Point3::new(20.0, 10.0, 11.0)),
        ],
    }
}

/// The program for [`line_then_arc`], with `arc` as the arc's lines.
fn line_then_arc_text(arc: &str) -> String {
    format!(
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F1000\n{arc}G0 Z5\nM5\n"
    )
}

/// Runs the whole safety pass, S6 included, on `text` as the fixture's
/// operation with `toolpath`, with the fixture's arc-fit tolerance.
fn check_against(text: &str, toolpath: &Toolpath) -> Result<(), ExportError> {
    let (project, _, _) = fixture();
    check_against_with(text, toolpath, project.arc_fit_tolerance_mm)
}

/// [`check_against`] with the arc-fit tolerance `arc_fit_mm`.
fn check_against_with(
    text: &str,
    toolpath: &Toolpath,
    arc_fit_mm: Option<f64>,
) -> Result<(), ExportError> {
    check_against_in(text, toolpath, arc_fit_mm, Units::Mm)
}

/// [`check_against_with`] for a program in `units`.
fn check_against_in(
    text: &str,
    toolpath: &Toolpath,
    arc_fit_mm: Option<f64>,
    units: Units,
) -> Result<(), ExportError> {
    let (mut project, op, tool) = fixture();
    project.display_units = units;
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let op = Operation::Profile(op);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath,
        material: &NO_MATERIAL,
        tabs: &[],
        lines: 1..text.lines().count() + 1,
    }];
    check_but_material(
        text,
        &post,
        &transform,
        &project.stock,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm,
        },
        ProgramHeights::default(),
        &spans,
    )
    .map(|_| ())
}

/// [`check_against`] with the operation `op` in place of the
/// fixture's Profile.
fn check_as(text: &str, toolpath: &Toolpath, op: &Operation) -> Result<(), ExportError> {
    let (project, _, tool) = fixture();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let material = holes_under(toolpath);
    let spans = [OpSpan {
        op,
        tool: &tool,
        toolpath,
        material: &material,
        tabs: &[],
        lines: 1..text.lines().count() + 1,
    }];
    check(
        text,
        &post,
        &transform,
        &project.stock,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: None,
        },
        ProgramHeights::default(),
        &spans,
    )
    .map(|_| ())
}

fn drill() -> Operation {
    Operation::Drill(dc_project::test_support::sample_drill())
}

/// For the hand-written Drill programs, whose hole positions the
/// fixture's rectangle does not draw: a 1 mm hole under every cutting
/// move of `toolpath`, so S9 holds the program to the toolpath's holes.
fn holes_under(toolpath: &Toolpath) -> Material {
    let mut holes: Vec<(Point, f64)> = Vec::new();
    for mv in toolpath.moves.iter().filter(|m| !m.is_rapid()) {
        let at = mv.to.xy();
        if !holes.iter().any(|(c, _)| c.distance_to(at) <= EPS_MM) {
            holes.push((at, 0.5));
        }
    }
    Material::Drill { holes }
}

/// A peck at (50, 50) on the 6 mm stock (stock-top origin, safe Z 5):
/// rapid down to the retract plane at Z 1, peck to −2, retract, rapid
/// back down to `reentry` (machine Z), peck to −3, out; `dwell` is
/// written after the last peck when given. With its toolpath.
fn peck_program(reentry: f64, dwell: Option<&str>) -> (String, Toolpath) {
    let dwell_line = dwell.map_or_else(String::new, |d| format!("{d}\n"));
    let text = format!(
        "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX50 Y50\nZ1\nG1 Z-2 F300\nG0 Z1\nZ{reentry}\nG1 Z-3\n{dwell_line}G0 Z5\nM5\n"
    );
    let mut moves = vec![
        Move::rapid(Point3::new(50.0, 50.0, 11.0)),
        Move::rapid(Point3::new(50.0, 50.0, 7.0)),
        Move::linear(Point3::new(50.0, 50.0, 4.0), 300.0),
        Move::rapid(Point3::new(50.0, 50.0, 7.0)),
        Move::rapid(Point3::new(50.0, 50.0, reentry + 6.0)),
        Move::linear(Point3::new(50.0, 50.0, 3.0), 300.0),
    ];
    if dwell.is_some() {
        moves.push(Move::dwell(Point3::new(50.0, 50.0, 3.0), 0.5));
    }
    moves.push(Move::rapid(Point3::new(50.0, 50.0, 11.0)));
    (text, Toolpath { moves })
}

/// S15 on the program (port of `drillingRetract.test.ts`
/// `assertPostedRetractSafe`): inside a Drill, a rapid in Z alone may
/// stop below safe Z above the stock or inside the hole already
/// drilled, never below the depth a `G1` has reached there.
#[test]
fn drill_rapid_into_uncut_fails() {
    for reentry in [-1.5, -2.0] {
        let (text, toolpath) = peck_program(reentry, None);
        check_as(&text, &toolpath, &drill())
            .unwrap_or_else(|e| panic!("re-entry to {reentry}: {e:?}"));
    }
    let (text, toolpath) = peck_program(-2.5, None);
    let err = check_as(&text, &toolpath, &drill()).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidIntoUncut { line: 10, .. }),
        "{err:?}"
    );
}

/// Outside a Drill, S15's rapids are S2's: the same rapids in a
/// Profile fail. The rapid from safe Z down to 1 mm above the stock
/// (line 7) is S2's rapid down to an entry and passes; the retract
/// to it (line 9) does not start at safe Z, and fails. (changed in
/// #214: line 7 failed.)
#[test]
fn drill_rapid_below_safe_z_on_profile_still_fails() {
    let (_, op, _) = fixture();
    let (text, toolpath) = peck_program(-1.5, None);
    let err = check_as(&text, &toolpath, &Operation::Profile(op)).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowSafeZ { line: 9, .. }),
        "{err:?}"
    );
}

/// S6: a dwell in the toolpath is matched by a `G4` with the same
/// seconds at that point of the program; a missing or different one is
/// a mismatch at the dwell.
#[test]
fn missing_dwell_fails_s6() {
    let (text, toolpath) = peck_program(-1.5, Some("G4 P0.5"));
    check_as(&text, &toolpath, &drill()).unwrap();
    let (text, _) = peck_program(-1.5, None);
    assert!(
        is_mismatch_at(&check_as(&text, &toolpath, &drill()), 6),
        "no G4"
    );
    let (text, _) = peck_program(-1.5, Some("G4 P2"));
    assert!(
        is_mismatch_at(&check_as(&text, &toolpath, &drill()), 6),
        "G4 P2"
    );
}

fn is_mismatch_at(result: &Result<(), ExportError>, at: usize) -> bool {
    matches!(result, Err(ExportError::MotionMismatch { index, .. }) if *index == at)
}

/// Security review of #62: a G2 that ends where it starts is a full
/// circle to the controller. This one, radius 10 about (105, 50), leaves
/// the 100 mm stock. It fails the check wherever it is, and the extents
/// count all of it, not zero sweep.
#[test]
fn full_circle_arc_fails_and_counts_whole() {
    let text = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX95 Y50\nG1 Z-1 F300\nG2 X95 Y50 I10 J0 F1000\nG0 Z5\nM5\n";
    let err = check_text(text).unwrap_err();
    assert!(
        matches!(err, ExportError::FullCircleArc { line: 8 }),
        "{err:?}"
    );
    // Within half a unit in the last place is still the same point.
    let near = text.replace("G2 X95 Y50", "G2 X95.0004 Y49.9996");
    assert!(matches!(
        check_text(&near),
        Err(ExportError::FullCircleArc { line: 8 })
    ));

    let blocks = crate::parse::parse_program(text).unwrap();
    let extents = extents(&blocks, Units::Mm);
    let x = extents.x.unwrap();
    let y = extents.y.unwrap();
    assert!((x.max - 115.0).abs() < 1e-9, "{x:?}");
    assert!(
        (y.min - 40.0).abs() < 1e-9 && (y.max - 60.0).abs() < 1e-9,
        "{y:?}"
    );
}

/// The arc, written as it should be, passes; the test programs below
/// differ from it in one line.
#[test]
fn arc_as_written_passes() {
    let text = line_then_arc_text("G3 X20 Y10 I0 J10\n");
    check_against(&text, &line_then_arc()).unwrap();
}

/// S6: G2 and G3 have the same radius check, so only S6 sees an arc
/// that runs the other way round.
#[test]
fn reversed_arc_fails_reproduction() {
    let text = line_then_arc_text("G2 X20 Y10 I0 J10\n");
    let result = check_against(&text, &line_then_arc());
    assert!(is_mismatch_at(&result, 3), "{result:?}");
}

/// S6: the same end points about a centre on the other side of the
/// chord.
#[test]
fn wrong_arc_centre_fails() {
    let text = line_then_arc_text("G3 X20 Y10 I10 J0\n");
    let result = check_against(&text, &line_then_arc());
    assert!(is_mismatch_at(&result, 3), "{result:?}");
}

/// S6: a move between two toolpath points that the toolpath does not
/// have.
#[test]
fn extra_move_between_points_fails() {
    let text =
        line_then_arc_text("G3 X20 Y10 I0 J10\n").replace("X10 F1000\n", "X5 Y3 F1000\nX10 Y0\n");
    let result = check_against(&text, &line_then_arc());
    assert!(is_mismatch_at(&result, 2), "{result:?}");
    // After the toolpath's last move, only a retract may follow.
    let text = line_then_arc_text("G3 X20 Y10 I0 J10\n").replace("M5\n", "X30\nM5\n");
    let result = check_against(&text, &line_then_arc());
    assert!(is_mismatch_at(&result, 5), "{result:?}");
}

/// One shape's block of toolpath: a rapid at safe Z to (10, `y`), a
/// plunge to Z 3, a cut to (40, `y`), and a retract.
fn shape_block(y: f64) -> Vec<Move> {
    vec![
        Move::rapid(Point3::new(10.0, y, 11.0)),
        Move::linear(Point3::new(10.0, y, 3.0), 300.0),
        Move::linear(Point3::new(40.0, y, 3.0), 1000.0),
        Move::rapid(Point3::new(40.0, y, 11.0)),
    ]
}

/// [`shape_block`] as written, every word explicit so a block reads
/// the same wherever it stands. Machine Z is project Z − 6.
fn shape_block_text(y: f64) -> String {
    format!("G0 X10 Y{y}\nG1 Z-3 F300\nG1 X40 F1000\nG0 Z5\n")
}

/// The fixture's setup lines, then `body`, then the spindle stop.
fn program_of(body: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\n{body}M5\n")
}

/// S13 O7, S6: the post-processor preserves order, so a program whose
/// shapes are reordered does not reproduce the toolpath. The two
/// shapes of one operation, cut at Y 50 then Y 70, pass as written
/// (and as the emitter writes them); with the two blocks swapped, each
/// intact with its own rapids and retract, the program fails with
/// `MotionMismatch` naming the operation, at the first move.
#[test]
fn reordered_shapes_fail_s6() {
    let (project, op, tool) = fixture();
    let toolpath = Toolpath {
        moves: [shape_block(50.0), shape_block(70.0)].concat(),
    };
    emit_one(&project, &op, &tool, toolpath.clone()).unwrap();
    let in_order = program_of(&(shape_block_text(50.0) + &shape_block_text(70.0)));
    check_against(&in_order, &toolpath).unwrap();
    let swapped = program_of(&(shape_block_text(70.0) + &shape_block_text(50.0)));
    let result = check_against(&swapped, &toolpath);
    assert!(
        matches!(
            &result,
            Err(ExportError::MotionMismatch { op: id, index: 0 }) if *id == op.id
        ),
        "{result:?}"
    );
}

/// S13 O7, S6: two operations' sections swapped. Each section is one
/// shape block, so both are the same length and each operation's
/// lines hold the other's moves; the first operation checked, `o1`,
/// fails with `MotionMismatch`. In order, it passes.
#[test]
fn reordered_operations_fail_s6() {
    let (project, first, tool) = fixture();
    let second = dc_project::ProfileOp {
        id: dc_project::OperationId::new("o2"),
        ..first.clone()
    };
    let (first, second) = (Operation::Profile(first), Operation::Profile(second));
    let (a, b) = (
        Toolpath {
            moves: shape_block(50.0),
        },
        Toolpath {
            moves: shape_block(70.0),
        },
    );
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    // Setup is lines 1–5; each section is 4 lines.
    let check_program = |text: &str| {
        let spans = [
            OpSpan {
                op: &first,
                tool: &tool,
                toolpath: &a,
                material: &NO_MATERIAL,
                tabs: &[],
                lines: 6..10,
            },
            OpSpan {
                op: &second,
                tool: &tool,
                toolpath: &b,
                material: &NO_MATERIAL,
                tabs: &[],
                lines: 10..text.lines().count() + 1,
            },
        ];
        check_but_material(
            text,
            &post,
            &transform,
            &project.stock,
            project.stock.thickness_mm.unwrap(),
            Tolerances {
                flatten_mm: project.flatten_tolerance_mm,
                arc_fit_mm: project.arc_fit_tolerance_mm,
            },
            ProgramHeights::default(),
            &spans,
        )
        .map(|_| ())
    };
    let in_order = program_of(&(shape_block_text(50.0) + &shape_block_text(70.0)));
    check_program(&in_order).unwrap();
    let swapped = program_of(&(shape_block_text(70.0) + &shape_block_text(50.0)));
    let result = check_program(&swapped);
    assert!(
        matches!(
            &result,
            Err(ExportError::MotionMismatch { op, .. }) if op.as_str() == "o1"
        ),
        "{result:?}"
    );
}

/// The arc of [`line_then_arc`] as `n` straight moves, counter-clockwise
/// (`ccw`) as in the toolpath, or the long way round clockwise.
fn flattened(n: u32, ccw: bool) -> String {
    let sweep = if ccw { FRAC_PI_2 } else { -3.0 * FRAC_PI_2 };
    let mut text = String::new();
    for k in 1..=n {
        let angle = -FRAC_PI_2 + sweep * f64::from(k) / f64::from(n);
        let (x, y) = (10.0 + 10.0 * angle.cos(), 10.0 + 10.0 * angle.sin());
        text.push_str(&format!("G1 X{x:.3} Y{y:.3}\n"));
    }
    text
}

/// S6: an arc written as short straight moves, as with
/// `arc_interpolation: false` or when the controller would reject the
/// rounded arc, still reproduces the toolpath. The same end point
/// reached the long way round does not.
#[test]
fn flattened_arc_passes() {
    let text = line_then_arc_text(&flattened(64, true));
    check_against(&text, &line_then_arc()).unwrap();
    let text = line_then_arc_text(&flattened(192, false));
    let result = check_against(&text, &line_then_arc());
    assert!(is_mismatch_at(&result, 3), "{result:?}");

    // Through the emitter: flattened, IJ, and R, mirrored or not.
    let (project, op, tool) = fixture();
    let mut post = project.post_processor.resolve().unwrap();
    for (interpolate, format, x_axis) in [
        (false, ArcFormat::Ij, Axis::X),
        (true, ArcFormat::Ij, Axis::X),
        (true, ArcFormat::R, Axis::X),
        (true, ArcFormat::Ij, Axis::NegX),
        (true, ArcFormat::R, Axis::NegX),
    ] {
        post.motion.arc_interpolation = interpolate;
        post.motion.arc_format = format;
        post.coordinate_system.x_axis = x_axis;
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        for tp in [toolpath, line_then_arc()] {
            emit_with(
                &project,
                &post,
                &[(Operation::Profile(op.clone()), tool.clone(), tp).into()],
                "2026-10-15",
            )
            .unwrap();
        }
    }
}

/// Mutation guard: an emitter that writes every arc the other way round
/// fails its own safety pass. The mutant is the emitter given a
/// definition with its two arc commands exchanged: the pass reads `G2`
/// and `G3` as the controller does, whatever the definition calls them.
#[test]
fn flipped_arcs_fail_emit() {
    let (project, op, tool) = fixture();
    let mut flipped = project.post_processor.resolve().unwrap();
    std::mem::swap(
        &mut flipped.motion.cw_arc_command,
        &mut flipped.motion.ccw_arc_command,
    );
    let result = emit_with(
        &project,
        &flipped,
        &[(
            Operation::Profile(op.clone()),
            tool.clone(),
            line_then_arc(),
        )
            .into()],
        "2026-10-15",
    )
    .map(|(p, _)| p.text);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { index: 3, .. })),
        "{result:?}"
    );
    emit_one(&project, &op, &tool, line_then_arc()).unwrap();
}

/// Rapid in, plunge to Z 5, then `n` chords of the circle of radius 10
/// about (50, 50) from +X through `sweep_rad` counter-clockwise, retract.
/// Vertex `bump` (1-based along the chords), if any, is pushed 0.05 mm
/// outwards, off the circle.
fn chord_run(n: u32, sweep_rad: f64, bump: Option<u32>) -> Toolpath {
    let mut moves = vec![
        Move::rapid(Point3::new(60.0, 50.0, 11.0)),
        Move::linear(Point3::new(60.0, 50.0, 5.0), 300.0),
    ];
    for k in 1..=n {
        let angle = sweep_rad * f64::from(k) / f64::from(n);
        let r = if bump == Some(k) { 10.05 } else { 10.0 };
        moves.push(Move::linear(
            Point3::new(50.0 + r * angle.cos(), 50.0 + r * angle.sin(), 5.0),
            1000.0,
        ));
    }
    let last = moves.last().unwrap().to;
    moves.push(Move::rapid(Point3::new(last.x, last.y, 11.0)));
    Toolpath { moves }
}

/// The program for [`chord_run`] with `arc` written for the chords.
fn chord_run_text(arc: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX60 Y50\nG1 Z-1 F300\n{arc}G0 Z5\nM5\n")
}

/// S12: one `G3` in place of 32 chords of a quarter circle passes
/// S6 when arc fitting is on; with the tolerance `null` the program
/// must have the chords. (changed in #226: of eight chords, which bow
/// 0.048 mm from the arc, more than the tolerance allows.)
#[test]
fn fitted_arcs_pass_s6() {
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\n");
    let toolpath = chord_run(32, FRAC_PI_2, None);
    check_against_with(&text, &toolpath, Some(0.01)).unwrap();
    let result = check_against_with(&text, &toolpath, None);
    assert!(is_mismatch_at(&result, 2), "{result:?}");
    // Two arcs over the run, meeting at a vertex, pass too.
    let text = chord_run_text("G3 X57.071 Y57.071 I-10 J0 F1000\nX50 Y60 I-7.071 J-7.071\n");
    check_against_with(&text, &toolpath, Some(0.01)).unwrap();
}

/// S12: a vertex between the arc's ends that is off the arc by more
/// than the tolerance fails. (changed in #226: of eight chords.)
#[test]
fn arc_off_the_vertices_fails_s6() {
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\n");
    let result = check_against_with(&text, &chord_run(32, FRAC_PI_2, Some(16)), Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
    // An arc that ends between vertices fails too.
    let text = chord_run_text("G3 X50.262 Y59.997 I-10 J0 F1000\n");
    let result = check_against_with(&text, &chord_run(32, FRAC_PI_2, None), Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
}

/// S12: one arc over 180° of chords is refused even though every vertex
/// is on it: a fitted arc sweeps at most 90°. (changed in #226: of 16
/// chords, which bow 0.19 mm from the arc.)
#[test]
fn arc_over_90_degrees_fails_s12() {
    let text = chord_run_text("G3 X40 Y50 I-10 J0 F1000\n");
    let result = check_against_with(&text, &chord_run(64, PI, None), Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
    // The same run as two 90° arcs passes.
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\nX40 Y50 I0 J-10\n");
    check_against_with(&text, &chord_run(64, PI, None), Some(0.01)).unwrap();
}

/// #226 (the #213 case): a 0.006 mm fragment, a short chord and a
/// 40 mm straight side, every vertex on one 154.75 mm circle, written
/// as one `G3`. The arc bows 1.3 mm off the side, so S12 refuses it,
/// though every vertex is on it. So does one `G3` for eight chords of
/// a 10 mm quarter circle, each bowing 0.048 mm, past the 0.01 mm
/// tolerance plus one unit in the last place.
#[test]
fn arc_bowing_off_a_long_straight_move_fails_s12() {
    let r = 154.75;
    let centre = Point::new(60.0 - r, 50.0);
    let side_rad = 2.0 * (20.0 / r).asin();
    let (fragment_rad, chord_rad) = (0.006 / r, 0.086 / r);
    let on_circle =
        |angle: f64| Point3::new(centre.x + r * angle.cos(), centre.y + r * angle.sin(), 5.0);
    let angles = [
        -side_rad / 2.0 - chord_rad - fragment_rad,
        -side_rad / 2.0 - chord_rad,
        -side_rad / 2.0,
        side_rad / 2.0,
    ];
    let (start, end) = (on_circle(angles[0]), on_circle(angles[3]));
    let mut moves = vec![
        Move::rapid(Point3::new(start.x, start.y, 11.0)),
        Move::linear(start, 300.0),
    ];
    moves.extend(
        angles[1..]
            .iter()
            .map(|&a| Move::linear(on_circle(a), 1000.0)),
    );
    moves.push(Move::rapid(Point3::new(end.x, end.y, 11.0)));
    let toolpath = Toolpath { moves };
    let program = |cut: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX{:.3} Y{:.3}\nG1 Z-1 F300\n{cut}G0 Z5\nM5\n",
            start.x, start.y,
        )
    };
    let arc = format!(
        "G3 X{:.3} Y{:.3} I{:.3} J{:.3} F1000\n",
        end.x,
        end.y,
        centre.x - start.x,
        centre.y - start.y,
    );
    let result = check_against_with(&program(&arc), &toolpath, Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
    // Written as its straight moves, the same toolpath passes.
    let lines: String = angles[1..]
        .iter()
        .map(|&a| on_circle(a))
        .map(|p| format!("G1 X{:.3} Y{:.3} F1000\n", p.x, p.y))
        .collect();
    check_against_with(&program(&lines), &toolpath, Some(0.01)).unwrap();

    // The same arc in inch (#233 failed S12 only in inch).
    let inch = |mm: f64| mm / 25.4;
    let text = format!(
        "G20 G90\nG54\nG0 Z{:.4}\nM3 S18000\nG4 P3\nX{:.4} Y{:.4}\nG1 Z{:.4} F{:.4}\n\
         G3 X{:.4} Y{:.4} I{:.4} J{:.4} F{:.4}\nG0 Z{:.4}\nM5\n",
        inch(5.0),
        inch(start.x),
        inch(start.y),
        inch(-1.0),
        inch(300.0),
        inch(end.x),
        inch(end.y),
        inch(centre.x - start.x),
        inch(centre.y - start.y),
        inch(1000.0),
        inch(5.0),
    );
    let result = check_against_in(&text, &toolpath, Some(0.01), Units::Inch);
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "inch: {result:?}"
    );

    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\n");
    let result = check_against_with(&text, &chord_run(8, FRAC_PI_2, None), Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
}

/// An arc within the tolerance of every chord it replaces, its
/// sagitta over each at most the tolerance plus one unit in the last
/// place, passes: 18 chords of a 10 mm quarter circle bow 0.0095 mm,
/// 32 chords 0.003 mm; as one arc and as two meeting at a vertex.
#[test]
fn arc_within_tolerance_of_every_chord_passes() {
    for n in [18, 32] {
        let toolpath = chord_run(n, FRAC_PI_2, None);
        let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\n");
        check_against_with(&text, &toolpath, Some(0.01))
            .unwrap_or_else(|e| panic!("{n} chords: {e:?}"));
        let text = chord_run_text("G3 X57.071 Y57.071 I-10 J0 F1000\nX50 Y60 I-7.071 J-7.071\n");
        check_against_with(&text, &toolpath, Some(0.01))
            .unwrap_or_else(|e| panic!("{n} chords, two arcs: {e:?}"));
    }
}

/// A move that spans the end of one piece and the start of the next is
/// held to the part of both pieces between its ends. A 20° chord from
/// 40° to 60° under two `G3` pieces meeting at 50° bows 0.15 mm off
/// its arc, so S12 refuses the chain; with that stretch in 2° chords
/// too it passes. (Each end of a move is placed on each piece; the fix
/// for #262 must not stop measuring across the joint.)
#[test]
fn chord_across_a_piece_end_is_measured_on_both_pieces() {
    let on_circle = |deg: f64| {
        let a = deg.to_radians();
        Point3::new(50.0 + 10.0 * a.cos(), 50.0 + 10.0 * a.sin(), 5.0)
    };
    let toolpath = |long_chord: bool| {
        let mut moves = vec![
            Move::rapid(Point3::new(60.0, 50.0, 11.0)),
            Move::linear(Point3::new(60.0, 50.0, 5.0), 300.0),
        ];
        let mut deg = 0_u32;
        while deg < 90 {
            deg += if long_chord && deg == 40 { 20 } else { 2 };
            moves.push(Move::linear(on_circle(f64::from(deg)), 1000.0));
        }
        moves.push(Move::rapid(Point3::new(50.0, 60.0, 11.0)));
        Toolpath { moves }
    };
    let joint = on_circle(50.0);
    let text = chord_run_text(&format!(
        "G3 X{:.3} Y{:.3} I-10 J0 F1000\nX50 Y60 I{:.3} J{:.3}\n",
        joint.x,
        joint.y,
        50.0 - joint.x,
        50.0 - joint.y,
    ));
    let result = check_against_with(&text, &toolpath(true), Some(0.01));
    assert!(
        matches!(result, Err(ExportError::ArcFitViolation { line: 8, .. })),
        "{result:?}"
    );
    check_against_with(&text, &toolpath(false), Some(0.01)).unwrap();
}

// S11: canned cycles (`docs/specs/phase-2/design.md`, "S11 expansion").
// No bundled definition has them yet, so these use a test-only one.

/// The fixture's post-processor with canned cycles, a re-entry gap of
/// `gap_mm`, and `use_canned_cycles` on.
fn canned(gap_mm: Option<f64>) -> PostProcessor {
    let (project, _, _) = fixture();
    let mut post = project.post_processor.resolve().unwrap();
    post.canned_cycles = Some(dc_project::CannedCycles {
        drill_command: Some("G81".into()),
        drill_with_dwell_command: Some("G82".into()),
        peck_drill_command: Some("G83".into()),
        chip_break_drill_command: Some("G73".into()),
        peck_step_word: "Q".into(),
        retract_mode: Some(dc_project::RetractMode::G98),
        cancel_command: "G80".into(),
        reentry_gap_mm: gap_mm,
    });
    post.use_canned_cycles = true;
    post
}

/// The whole safety pass on `text` as the fixture's Drill with
/// `toolpath`, written for `post`.
fn check_drill_on(
    text: &str,
    toolpath: &Toolpath,
    post: &PostProcessor,
) -> Result<(), ExportError> {
    let (project, _, tool) = fixture();
    let transform = MachineTransform::new(&project, post).unwrap();
    let op = drill();
    let material = holes_under(toolpath);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath,
        material: &material,
        tabs: &[],
        lines: 1..text.lines().count() + 1,
    }];
    check(
        text,
        post,
        &transform,
        &project.stock,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: None,
        },
        ProgramHeights::default(),
        &spans,
    )
    .map(|_| ())
}

/// A program on the fixture (stock-top origin, safe Z 5, spindle on)
/// that rapids over (50, 50) at safe Z, writes `G98`, then `cycles`.
fn cycle_program(cycles: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX50 Y50\nG98\n{cycles}G80\nM5\n")
}

/// The sample Drill's toolpath for simple holes at `xs` (Y 50): over
/// the hole at safe Z (11), down to the retract plane (7), plunge to
/// 3 mm, out to safe Z.
fn simple_holes(xs: &[f64]) -> Toolpath {
    let mut moves = Vec::new();
    for &x in xs {
        let at = |z: f64| Point3::new(x, 50.0, z);
        moves.extend([
            Move::rapid(at(11.0)),
            Move::rapid(at(7.0)),
            Move::linear(at(3.0), 300.0),
            Move::rapid(at(11.0)),
        ]);
    }
    Toolpath { moves }
}

/// A full-retract peck at (50, 50) from the stock top (retract 0):
/// pecks to 4 and 3 mm, re-entering 0.5 mm above 4.
fn peck_from_top() -> Toolpath {
    let at = |z: f64| Point3::new(50.0, 50.0, z);
    Toolpath {
        moves: vec![
            Move::rapid(at(11.0)),
            Move::rapid(at(6.0)),
            Move::linear(at(4.0), 300.0),
            Move::rapid(at(6.0)),
            Move::rapid(at(4.5)),
            Move::linear(at(3.0), 300.0),
            Move::rapid(at(11.0)),
        ],
    }
}

/// S11: the parser expands each `G81` and `G83` line into the motion
/// the controller makes, and S6 compares that with the Drill toolpath
/// move for move. The right program passes; a wrong `Z`, `R` or `Q`
/// does not. `G83` needs the definition's re-entry gap, and cycles
/// need the project to ask for them.
#[test]
fn canned_cycle_expansion_matches_drill_toolpath() {
    let post = canned(Some(0.5));
    let simple = simple_holes(&[50.0, 60.0]);
    let g81 = |z: &str, r: &str| {
        cycle_program(&format!("G81 X50 Y50 Z{z} R{r} F300\nX60 Y50 Z{z} R{r}\n"))
    };
    check_drill_on(&g81("-3", "1"), &simple, &post).unwrap();
    for (z, r) in [("-2.5", "1"), ("-3", "2")] {
        let result = check_drill_on(&g81(z, r), &simple, &post);
        assert!(
            matches!(result, Err(ExportError::MotionMismatch { .. })),
            "Z{z} R{r}: {result:?}"
        );
    }

    let g83 = |q: &str| cycle_program(&format!("G83 X50 Y50 Z-3 R0 Q{q} F300\n"));
    check_drill_on(&g83("2"), &peck_from_top(), &post).unwrap();
    let result = check_drill_on(&g83("1.5"), &peck_from_top(), &post);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { .. })),
        "Q1.5: {result:?}"
    );
    // A controller that re-enters 0.3 mm above the depth: within the
    // gap its definition states.
    check_drill_on(&g83("2"), &peck_from_top(), &canned(Some(0.3))).unwrap();
    // With no stated gap the G83 cannot be expanded.
    let result = check_drill_on(&g83("2"), &peck_from_top(), &canned(None));
    assert!(
        matches!(result, Err(ExportError::Unparseable { line: 8, .. })),
        "{result:?}"
    );
    // Cycles are refused where the project does not ask for them.
    let mut off = post;
    off.use_canned_cycles = false;
    let result = check_drill_on(&g81("-3", "1"), &simple, &off);
    assert!(
        matches!(&result, Err(ExportError::ForbiddenWord { line: 7, word }) if word == "G98"),
        "{result:?}"
    );
}

/// The Drill toolpath [`dc_cam::generate_drill`] makes for a 2 mm
/// peck, 3 mm deep, with the retract plane 1 mm above the 6 mm
/// stock, at (50, 50): pecks from `R` (Z 7) to 5, then to 3.
fn generated_peck(retract: dc_project::PeckRetract) -> Toolpath {
    let (mut project, _, _) = fixture();
    let hole = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("hole"),
        name: "Hole".into(),
        profiles: vec![dc_geometry::Polyline::circle(Point::new(50.0, 50.0), 4.0)],
    };
    project.instances.push(dc_project::FeatureInstance {
        id: dc_project::InstanceId::new("h1"),
        definition: hole.id.clone(),
        transform: dc_geometry::Transform::translate(0.0, 0.0),
        name: "Hole".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
    project.definitions.insert(hole.id.clone(), hole);
    let op = dc_project::DrillOp {
        targets: vec![dc_project::InstanceId::new("h1")],
        cycle: dc_project::DrillCycle::Peck {
            peck_depth_mm: Some(2.0),
            retract,
        },
        depth_mm: Some(3.0),
        retract_mm: 1.0,
        ..dc_project::test_support::sample_drill()
    };
    dc_cam::generate_drill(&project, &op).unwrap().0.toolpath
}

/// #153, S11: with the retract plane 1 mm above the stock, the
/// controller's `G83`/`G73` pecks from `R1` (2 mm, then to the
/// bottom) are the generated Drill toolpath's, move for move, so the
/// written cycle passes the safety pass; a `Q` that pecks otherwise
/// does not.
#[test]
fn g83_expansion_matches_drill_toolpath_with_retract() {
    let post = canned(Some(0.5));
    for (retract, word) in [
        (dc_project::PeckRetract::Full, "G83"),
        (dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 }, "G73"),
    ] {
        let toolpath = generated_peck(retract);
        let text = |q: &str| cycle_program(&format!("{word} X50 Y50 Z-3 R1 Q{q} F300\n"));
        check_drill_on(&text("2"), &toolpath, &post).unwrap_or_else(|e| panic!("{word} Q2: {e:?}"));
        let result = check_drill_on(&text("3"), &toolpath, &post);
        assert!(
            matches!(result, Err(ExportError::MotionMismatch { .. })),
            "{word} Q3: {result:?}"
        );
    }
}

/// The fixture's clearance height for the #305 tests: work Z 10,
/// absolute 16, above safe Z (work 5, absolute 11).
const CLEARANCE_Z_MM: f64 = 16.0;

/// The whole safety pass on `text` as the fixture's Drill with
/// `toolpath`, written for `post`, with the clearance height
/// [`CLEARANCE_Z_MM`].
fn check_drill_at_clearance(
    text: &str,
    toolpath: &Toolpath,
    post: &PostProcessor,
) -> Result<(), ExportError> {
    let (project, _, tool) = fixture();
    let transform = MachineTransform::new(&project, post).unwrap();
    let op = drill();
    let material = holes_under(toolpath);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath,
        material: &material,
        tabs: &[],
        lines: 1..text.lines().count() + 1,
    }];
    check(
        text,
        post,
        &transform,
        &project.stock,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: None,
        },
        ProgramHeights {
            machine_lift_mm: None,
            clearance_z_mm: Some(CLEARANCE_Z_MM),
        },
        &spans,
    )
    .map(|_| ())
}

/// The sequencer's Drill toolpath for simple holes at `xs` (Y 50),
/// each a part of its own, with the clearance height
/// [`CLEARANCE_Z_MM`]: the first hole is reached at safe Z (11); each
/// hole goes down to the retract plane (7), plunges to 3 mm and goes
/// out to safe Z; between holes the tool rises to `link_z`, crosses
/// there and comes down to safe Z again.
fn holes_linked_at(xs: &[f64], link_z: f64) -> Toolpath {
    let mut moves = Vec::new();
    for (k, &x) in xs.iter().enumerate() {
        let at = |z: f64| Point3::new(x, 50.0, z);
        if k > 0 {
            moves.push(Move::rapid(at(link_z)));
        }
        moves.extend([
            Move::rapid(at(11.0)),
            Move::rapid(at(7.0)),
            Move::linear(at(3.0), 300.0),
            Move::rapid(at(11.0)),
        ]);
        if k + 1 < xs.len() && link_z > 11.0 {
            moves.push(Move::rapid(at(link_z)));
        }
    }
    Toolpath { moves }
}

/// A program on the fixture that rises to the clearance height (work
/// Z 10), rapids over (50, 50) there, writes `G98`, then `cycles`.
fn clearance_cycle_program(cycles: &str) -> String {
    format!("G21 G90\nG54\nG0 Z10\nM3 S18000\nG4 P3\nX50 Y50\nG98\n{cycles}G80\nM5\n")
}

/// #305, S11: cycles written from the clearance height. `G98` returns
/// each hole to the Z before the first cycle of the series (LinuxCNC
/// `interp_cycles.cc` 926-931 and 976; Mach3Mill 10.7.29), work Z 10,
/// so the controller rapids from there straight to `R` and from the
/// bottom straight back, where the toolpath has two rapids along the
/// same vertical line, through safe Z. S6 takes them as one: the
/// program passes. A cycle whose `R` or `Z` differs from the toolpath
/// still fails, and so does the same program against a toolpath whose
/// holes are linked at safe Z, which `G98` from the clearance height
/// does not follow.
#[test]
fn canned_cycles_from_clearance_match_the_toolpath() {
    let post = canned(Some(0.5));
    let linked = holes_linked_at(&[50.0, 60.0], CLEARANCE_Z_MM);
    let g81 = |z: &str, r: &str| {
        clearance_cycle_program(&format!("G81 X50 Y50 Z{z} R{r} F300\nX60 Y50 Z{z} R{r}\n"))
    };
    check_drill_at_clearance(&g81("-3", "1"), &linked, &post).unwrap();
    for (z, r) in [("-2.5", "1"), ("-3", "2"), ("-3", "0.5")] {
        let result = check_drill_at_clearance(&g81(z, r), &linked, &post);
        assert!(
            matches!(result, Err(ExportError::MotionMismatch { .. })),
            "Z{z} R{r}: {result:?}"
        );
    }
    let at_safe_z = holes_linked_at(&[50.0, 60.0], 11.0);
    let result = check_drill_at_clearance(&g81("-3", "1"), &at_safe_z, &post);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { .. })),
        "links at safe Z: {result:?}"
    );
}

/// S6 joins toolpath rapids along one vertical line only where they
/// lie on one rapid of the controller's in the same direction, and
/// only for a Drill written as canned cycles. A toolpath that goes up
/// past safe Z and then back down is not the cycle's one rapid up; and
/// a plain program with one `G0 Z` where the toolpath has two still
/// fails, as before, while the program that writes both passes.
#[test]
fn vertical_rapids_join_only_along_a_cycle_rapid() {
    let post = canned(Some(0.5));
    let at = |z: f64| Point3::new(50.0, 50.0, z);
    let back_down = Toolpath {
        moves: vec![
            Move::rapid(at(11.0)),
            Move::rapid(at(7.0)),
            Move::linear(at(3.0), 300.0),
            Move::rapid(at(13.0)),
            Move::rapid(at(11.0)),
        ],
    };
    let text = clearance_cycle_program("G81 X50 Y50 Z-3 R1 F300\n");
    let result = check_drill_at_clearance(&text, &back_down, &post);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { .. })),
        "up past safe Z, then down: {result:?}"
    );

    let linked = holes_linked_at(&[50.0, 60.0], CLEARANCE_Z_MM);
    let one_rapid = "G21 G90\nG54\nG0 Z10\nM3 S18000\nG4 P3\nX50 Y50\nZ1\nG1 Z-3 F300\nG0 Z10\nX60 Y50\nZ1\nG1 Z-3\nG0 Z5\nM5\n";
    let result = check_drill_at_clearance(one_rapid, &linked, &post);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { .. })),
        "plain program, one rapid for two: {result:?}"
    );
    let both = "G21 G90\nG54\nG0 Z10\nM3 S18000\nG4 P3\nX50 Y50\nZ5\nZ1\nG1 Z-3 F300\nG0 Z5\nZ10\nX60 Y50\nZ5\nZ1\nG1 Z-3\nG0 Z5\nM5\n";
    check_drill_at_clearance(both, &linked, &post).unwrap();
}

/// The sequencer's Drill toolpath for a hole at X 50, its own part,
/// then a part with holes at X 60 and X 70 (Y 50), with the clearance
/// height [`CLEARANCE_Z_MM`]: each hole goes down from safe Z (11) to
/// the retract plane (7), plunges to 3 mm and goes out to safe Z; the
/// tool rises to the clearance height between the parts, comes down
/// to safe Z over X 60, and crosses to X 70 at safe Z.
fn holes_of_two_parts() -> Toolpath {
    let at = |x: f64, z: f64| Point3::new(x, 50.0, z);
    let hole = |x: f64| {
        [
            Move::rapid(at(x, 7.0)),
            Move::linear(at(x, 3.0), 300.0),
            Move::rapid(at(x, 11.0)),
        ]
    };
    let mut moves = vec![Move::rapid(at(50.0, 11.0))];
    moves.extend(hole(50.0));
    moves.extend([
        Move::rapid(at(50.0, CLEARANCE_Z_MM)),
        Move::rapid(at(60.0, CLEARANCE_Z_MM)),
        Move::rapid(at(60.0, 11.0)),
    ]);
    moves.extend(hole(60.0));
    moves.push(Move::rapid(at(70.0, 11.0)));
    moves.extend(hole(70.0));
    Toolpath { moves }
}

/// The program `emit` writes for [`holes_of_two_parts`] on the
/// fixture (work Z: safe Z 5, clearance 10): a `G98` series per part,
/// each from safe Z over its first hole and closed by `G80`, the rise
/// to the clearance height between them. `second` is what comes
/// between the crossing to X 60 and the second series' first cycle,
/// `close` what closes the first series.
fn series_per_part_program(close: &str, second: &str) -> String {
    format!(
        "G21 G90\nG54\nG0 Z10\nM3 S18000\nG4 P3\nX50 Y50\nZ5\nG98\n\
         G81 X50 Y50 Z-3 R1 F300\n{close}G0 Z10\nX60\n{second}\
         G81 X60 Y50 Z-3 R1 F300\nX70 Y50 Z-3 R1\nG80\nG0 Z10\nM5\n"
    )
}

/// #314, S11: a Drill linked at both heights, written as one `G98`
/// series per part. `G80`, and any motion that is not a cycle, ends a
/// series, and the next cycle starts a new one from the Z it is at
/// (LinuxCNC `interp_convert.cc` 3818-3819, `interp_cycles.cc`
/// 926-931), here safe Z, so the second series returns both holes of
/// its part to safe Z and crosses between them there. The expansion is
/// the toolpath move for move: the program passes. It fails when the
/// first series lacks the `G80` before the rise (unparseable: S11
/// models no series ended by other motion), when the second series
/// starts from the clearance height (no `Z5` before its `G98`: `G98`
/// then returns to Z 10, and the pair is crossed there), and with
/// `G99`, which returns to `R`, below safe Z.
#[test]
fn series_per_part_match_the_toolpath() {
    let post = canned(Some(0.5));
    let toolpath = holes_of_two_parts();
    let text = series_per_part_program("G80\n", "Z5\nG98\n");
    check_drill_at_clearance(&text, &toolpath, &post).unwrap();

    let no_cancel = series_per_part_program("", "Z5\nG98\n");
    let result = check_drill_at_clearance(&no_cancel, &toolpath, &post);
    assert!(
        matches!(result, Err(ExportError::Unparseable { line: 10, .. })),
        "no G80 before the rise: {result:?}"
    );
    let from_clearance = series_per_part_program("G80\n", "G98\n");
    let result = check_drill_at_clearance(&from_clearance, &toolpath, &post);
    assert!(
        matches!(result, Err(ExportError::MotionMismatch { .. })),
        "second series from the clearance height: {result:?}"
    );
    let g99 = series_per_part_program("G80\n", "Z5\nG99\n");
    let result = check_drill_at_clearance(&g99, &toolpath, &post);
    assert!(result.is_err(), "G99: {result:?}");
}

/// S1 on the expansion: a `G81` whose bottom is below the stock bottom
/// (Z −6.5 on the 6 mm stock, no allowance) fails on its line.
#[test]
fn canned_cycle_below_floor_fails_s1() {
    let text = cycle_program("G81 X50 Y50 Z-6.5 R1 F300\n");
    let result = check_drill_on(&text, &simple_holes(&[50.0]), &canned(Some(0.5)));
    assert!(
        matches!(
            result,
            Err(ExportError::BelowMaterialBottom { line: 8, .. })
        ),
        "{result:?}"
    );
}

/// S15 and S2 on the expansion: an `R` plane inside the material makes
/// the controller rapid into uncut stock; `G99` returns to `R` between
/// holes, so the rapid to the next one is below safe Z.
#[test]
fn canned_cycle_rapid_into_uncut_fails() {
    let post = canned(Some(0.5));
    let text = cycle_program("G81 X50 Y50 Z-3 R-1 F300\n");
    let result = check_drill_on(&text, &simple_holes(&[50.0]), &post);
    assert!(
        matches!(result, Err(ExportError::RapidIntoUncut { line: 8, .. })),
        "{result:?}"
    );
    let text = cycle_program("G99\nG81 X50 Y50 Z-3 R1 F300\nX60 Y50 Z-3 R1\n");
    let result = check_drill_on(&text, &simple_holes(&[50.0, 60.0]), &post);
    assert!(
        matches!(result, Err(ExportError::RapidBelowSafeZ { line: 10, .. })),
        "{result:?}"
    );
}

// S9 and S10 (`docs/specs/phase-2/design.md`, "Test plan").

/// The 1-based line of `text` with a word that starts `word`.
fn line_with(text: &str, word: &str) -> usize {
    text.lines()
        .position(|l| l.split_whitespace().any(|w| w.starts_with(word)))
        .map(|i| i + 1)
        .unwrap_or_else(|| panic!("no {word} in\n{text}"))
}

/// The vertical rate, mm/min, of every `G1` in `text` (a millimetre
/// program) that falls while it moves in XY.
fn descent_rates(text: &str) -> Vec<f64> {
    crate::parse::parse_motion(text)
        .unwrap()
        .iter()
        .filter(|m| m.kind == MotionKind::Linear)
        .filter_map(|m| {
            let (fx, fy, fz) = (m.from.x?, m.from.y?, m.from.z?);
            let (tx, ty, tz) = (m.to.x?, m.to.y?, m.to.z?);
            let (drop, run) = (fz - tz, (tx - fx).hypot(ty - fy));
            (drop > 0.0 && run > 0.0).then(|| m.feed.unwrap_or(0.0) * drop / run.hypot(drop))
        })
        .collect()
}

/// Rapid over (10, 10), plunge to Z 6 (the stock top), ramp to
/// (14, 10) at Z 4 at `ramp_feed`, cut on to (30, 10), retract. The
/// ramp drops 2 mm over 4 mm, sin = 2/√20: at the 1000 feed it falls at
/// 447 mm/min, past the 300 plunge; at 300 × √20 / 2 = 670.8 mm/min it
/// falls at the plunge rate.
fn ramp(ramp_feed: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(10.0, 10.0, 11.0)),
            Move::linear(Point3::new(10.0, 10.0, 6.0), 300.0),
            Move::linear(Point3::new(14.0, 10.0, 4.0), ramp_feed),
            Move::linear(Point3::new(30.0, 10.0, 4.0), 1000.0),
            Move::rapid(Point3::new(30.0, 10.0, 11.0)),
        ],
    }
}

/// S10: a ramp at the cutting feed that falls faster than the plunge
/// rate fails, naming the operation and the line; at the scaled feed
/// it passes. The same on a hand-written program.
#[test]
fn ramp_steeper_than_plunge_fails_s10() {
    let (project, op, tool) = partless_fixture();
    let scaled = 300.0 * 20f64.sqrt() / 2.0;
    let text = emit_one(&project, &op, &tool, ramp(scaled)).unwrap();
    let ramp_line = line_with(&text, "X14");
    let err = emit_one(&project, &op, &tool, ramp(1000.0)).unwrap_err();
    assert!(
        matches!(&err, ExportError::EntryTooSteep { op: at, line }
            if at == &op.id && *line == ramp_line),
        "{err:?}"
    );

    let text = |f: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z0 F300\nX4 Z-2 F{f}\nX10 F1000\nG0 Z5\nM5\n"
        )
    };
    check_text(&text("670.82")).unwrap();
    let err = check_text(&text("1000")).unwrap_err();
    assert!(
        matches!(err, ExportError::EntryTooSteep { line: 8, .. }),
        "{err:?}"
    );
}

/// S10 on a straight plunge: its slope is 90°, so its feed is its
/// vertical rate and must not pass the plunge rate.
#[test]
fn vertical_plunge_at_cutting_feed_fails_s10() {
    let plunge = |f: &str| {
        format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F{f}\nX10 F1000\nG0 Z5\nM5\n")
    };
    check_text(&plunge("300")).unwrap();
    let err = check_text(&plunge("1000")).unwrap_err();
    assert!(
        matches!(err, ExportError::EntryTooSteep { line: 7, .. }),
        "{err:?}"
    );
}

/// S10 allows nothing above the plunge rate. A short ramp, 0.3 mm down
/// over 0.4 mm across (sin 0.6), at F501.5 falls at 300.9 mm/min against
/// the fixture's 300: refused, though a unit off each written number
/// would bring it under. Just under the plunge rate, at F499.999, it
/// passes, and so does line 7, a straight plunge at exactly F300.
#[test]
fn s10_refuses_vertical_rate_above_plunge() {
    let ramp = |f: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z0 F300\nX0.4 Z-0.3 F{f}\nX10 F1000\nG0 Z5\nM5\n"
        )
    };
    let err = check_text(&ramp("501.5")).unwrap_err();
    assert!(
        matches!(err, ExportError::EntryTooSteep { line: 8, .. }),
        "{err:?}"
    );
    check_text(&ramp("499.999")).unwrap();
    check_text(&ramp("400")).unwrap();
}

/// A lap at Z 3 along Y 10 with a tab: up 2 mm over 3 mm at the
/// cutting feed, a hold, then down 2 mm over 3 mm at `down_feed`. At
/// the 1000 feed the down slope falls at 1000 × 2/√13 = 555 mm/min,
/// past the 300 plunge; at 300 × √13 / 2 = 540.8 it falls at 300.
fn tab_lap(down_feed: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(10.0, 10.0, 11.0)),
            Move::linear(Point3::new(10.0, 10.0, 3.0), 300.0),
            Move::linear(Point3::new(20.0, 10.0, 3.0), 1000.0),
            Move::linear(Point3::new(23.0, 10.0, 5.0), 1000.0),
            Move::linear(Point3::new(26.0, 10.0, 5.0), 1000.0),
            Move::linear(Point3::new(29.0, 10.0, 3.0), down_feed),
            Move::linear(Point3::new(40.0, 10.0, 3.0), 1000.0),
            Move::rapid(Point3::new(40.0, 10.0, 11.0)),
        ],
    }
}

/// S10 holds on every descending move, a tab's down slope included:
/// one at the cutting feed that falls faster than the plunge rate
/// fails; scaled, it passes. The up slope is not a descent.
#[test]
fn tab_slope_steeper_than_plunge_fails_s10() {
    let (project, op, tool) = partless_fixture();
    let scaled = 300.0 * 13f64.sqrt() / 2.0;
    let text = emit_one(&project, &op, &tool, tab_lap(scaled)).unwrap();
    let down_line = line_with(&text, "X29");
    let err = emit_one(&project, &op, &tool, tab_lap(1000.0)).unwrap_err();
    assert!(
        matches!(&err, ExportError::EntryTooSteep { op: at, line }
            if at == &op.id && *line == down_line),
        "{err:?}"
    );
}

/// The sample Profile with smooth tabs, generated, exports: each down
/// slope (2 mm over 3 mm) runs at its plunge-limited feed.
#[test]
fn smooth_tab_profile_passes_s10() {
    let mut project = sample_project();
    let op = dc_project::test_support::profile_mut(&mut project);
    if let Some(tabs) = op.tabs.as_mut() {
        tabs.shape = dc_project::TabShape::Smooth;
    }
    let op = op.clone();
    let tool = project.tools[0].clone();
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    let text = emit_one(&project, &op, &tool, toolpath).unwrap();
    let rates = descent_rates(&text);
    assert!(!rates.is_empty(), "the tabs' down slopes descend");
    for rate in rates {
        assert!(rate <= 300.0, "a down slope falls at {rate} mm/min");
    }
}

/// The slice 9 island Pocket (`dc_cam::pocket` tests): 300 × 200 × 6 mm
/// stock, a 100 × 50 mm rectangle at (20, 20) with a 20 mm circle
/// island at its centre (70, 45), an offset Pocket 4 mm deep in the
/// 6.35 mm tool's 2 mm steps, 40 % step-over, entering by `entry`, with
/// `stock_to_leave_mm`.
fn island_pocket(
    entry: dc_project::Entry,
    stock_to_leave_mm: f64,
) -> (Project, dc_project::PocketOp) {
    let mut project = sample_project();
    project.stock = dc_project::Stock {
        width_mm: 300.0,
        height_mm: 200.0,
        thickness_mm: Some(6.0),
    };
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles = vec![
            dc_geometry::Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
            dc_geometry::Polyline::circle(Point::new(50.0, 25.0), 10.0),
        ];
    }
    project.instances[0].transform = dc_geometry::Transform::translate(20.0, 20.0);
    let op = dc_project::PocketOp {
        depth_mm: Some(4.0),
        step_over_fraction: Some(0.4),
        entry,
        stock_to_leave_mm,
        ..dc_project::test_support::sample_pocket()
    };
    project.operations = vec![Operation::Pocket(op.clone())];
    (project, op)
}

fn helix() -> dc_project::Entry {
    dc_project::Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    }
}

/// Emits Pocket `op` of `project` with `toolpath`, or its generated
/// toolpath when `None`.
fn emit_pocket(
    project: &Project,
    op: &dc_project::PocketOp,
    toolpath: Option<Toolpath>,
) -> Result<String, ExportError> {
    let toolpath = toolpath.unwrap_or_else(|| dc_cam::generate_pocket(project, op).unwrap().0);
    emit(
        project,
        &[(
            Operation::Pocket(op.clone()),
            project.tools[0].clone(),
            toolpath,
        )
            .into()],
        "2026-10-21",
    )
    .map(|(p, _)| p.text)
}

/// S10 on a generated helix: every chord's feed is scaled so it falls
/// no faster than the plunge rate, and the program passes.
#[test]
fn helix_feed_scaled_passes_s10() {
    let (project, op) = island_pocket(helix(), 0.0);
    let text = emit_pocket(&project, &op, None).unwrap();
    let rates = descent_rates(&text);
    assert!(rates.len() > 100, "helices are cut: {}", rates.len());
    // Within the rounding of 0.3 mm chords to three decimals.
    for rate in rates {
        assert!(rate <= 300.0, "a helix chord falls at {rate} mm/min");
    }
}

/// Rapid over (`x0`, `y`), plunge to Z 3, one cut to (`x1`, `y`) at the
/// feed, retract.
fn one_pass(x0: f64, x1: f64, y: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(x0, y, 11.0)),
            Move::linear(Point3::new(x0, y, 3.0), 300.0),
            Move::linear(Point3::new(x1, y, 3.0), 1000.0),
            Move::rapid(Point3::new(x1, y, 11.0)),
        ],
    }
}

/// S9: a Pocket cut that brings the 6.35 mm tool past the region's
/// wall, into its stock to leave, or onto an island fails with
/// `OffMaterial` at that line; the generated Pocket passes. The sample
/// rectangle spans X 10 to 60, so a tool centre at X 56.8 is 0.025 mm
/// inside the wall and one at 58 is 1.175 mm past it.
#[test]
fn pocket_cut_outside_region_fails_s9() {
    let mut project = sample_project();
    let op = dc_project::test_support::sample_pocket();
    project.operations = vec![Operation::Pocket(op.clone())];
    emit_pocket(&project, &op, None).unwrap();
    let text = emit_pocket(&project, &op, Some(one_pass(20.0, 56.8, 25.0))).unwrap();
    let cut_line = line_with(&text, "X56.8");
    let err = emit_pocket(&project, &op, Some(one_pass(20.0, 58.0, 25.0))).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line }
            if at == &op.id && *line == cut_line),
        "{err:?}"
    );

    // A 0.5 mm stock to leave keeps the tool 0.5 mm off the wall.
    let left = dc_project::PocketOp {
        stock_to_leave_mm: 0.5,
        ..op.clone()
    };
    let err = emit_pocket(&project, &left, Some(one_pass(20.0, 56.8, 25.0))).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line, .. } if line == cut_line),
        "{err:?}"
    );
    emit_pocket(&project, &left, Some(one_pass(20.0, 56.3, 25.0))).unwrap();

    // The island's edge is at X 60 on Y 45: the tool may come to 56.8.
    let (project, op) = island_pocket(dc_project::Entry::Plunge, 0.0);
    emit_pocket(&project, &op, None).unwrap();
    emit_pocket(&project, &op, Some(one_pass(30.0, 56.8, 45.0))).unwrap();
    let err = emit_pocket(&project, &op, Some(one_pass(30.0, 58.0, 45.0))).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line, .. } if line == cut_line),
        "{err:?}"
    );
}

/// S9 on a raster Pocket round the 20 mm circle island: with a wall
/// pass the links between line ends against the island follow its
/// tool-centre ring, and the program passes, with stock to leave too,
/// at angles that meet the island's curve everywhere.
#[test]
fn raster_edge_links_round_an_island_pass_s9() {
    for stock_to_leave_mm in [0.0, 0.5] {
        let (project, op) = island_pocket(dc_project::Entry::Plunge, stock_to_leave_mm);
        for angle_deg in [0.0, 45.0, 90.0] {
            for wall_pass in [dc_project::WallPass::Last, dc_project::WallPass::First] {
                let op = dc_project::PocketOp {
                    strategy: dc_project::ClearanceStrategy::Raster { angle_deg },
                    wall_pass,
                    ..op.clone()
                };
                let text = emit_pocket(&project, &op, None);
                assert!(
                    text.is_ok(),
                    "{stock_to_leave_mm} mm, {angle_deg}°, {wall_pass:?}: {text:?}"
                );
            }
        }
    }
}

/// S9 on an offset Pocket at step-over 1.0: the corner clean-up rings
/// are cut from inside the laps of the rings that leave them, out from
/// a corner and back, and the program passes, on the rect with and
/// without its island, with stock to leave too, the wall pass last or
/// first, plunging or by helix.
#[test]
fn offset_corner_cleanups_pass_s9() {
    for stock_to_leave_mm in [0.0, 0.5] {
        for entry in [dc_project::Entry::Plunge, helix()] {
            let (island, op) = island_pocket(entry, stock_to_leave_mm);
            let mut plain = island.clone();
            if let Some(def) = plain.definitions.values_mut().next() {
                def.profiles.truncate(1);
            }
            for project in [&island, &plain] {
                for wall_pass in [dc_project::WallPass::Last, dc_project::WallPass::First] {
                    let op = dc_project::PocketOp {
                        step_over_fraction: Some(1.0),
                        wall_pass,
                        ..op.clone()
                    };
                    let text = emit_pocket(project, &op, None);
                    assert!(
                        text.is_ok(),
                        "{stock_to_leave_mm} mm, {:?}, {wall_pass:?}: {text:?}",
                        op.entry
                    );
                }
            }
        }
    }
}

/// S9 on a raster Pocket whose wall curves inward: an L, whose reflex
/// corner rounds the tool-centre region by the tool radius, and a rect
/// with a half-disc bay of radius 15 pushed in from its top. With a
/// wall pass the links between line ends against those arcs follow the
/// tool-centre ring, and the program passes, with a stock to leave
/// too, at several angles.
#[test]
fn raster_edge_links_along_a_concave_wall_pass_s9() {
    use dc_geometry::{Polyline, Vertex};
    let l_shape = Polyline::new(
        [
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 25.0),
            (50.0, 25.0),
            (50.0, 50.0),
            (0.0, 50.0),
        ]
        .into_iter()
        .map(|(x, y)| Vertex::line(x, y))
        .collect(),
        true,
    );
    let bay = Polyline::new(
        vec![
            Vertex::line(0.0, 0.0),
            Vertex::line(100.0, 0.0),
            Vertex::line(100.0, 50.0),
            Vertex::new(65.0, 50.0, -1.0),
            Vertex::line(35.0, 50.0),
            Vertex::line(0.0, 50.0),
        ],
        true,
    );
    for outline in [l_shape, bay] {
        for stock_to_leave_mm in [0.0, 0.5] {
            let (mut project, op) = island_pocket(dc_project::Entry::Plunge, stock_to_leave_mm);
            if let Some(def) = project.definitions.values_mut().next() {
                def.profiles = vec![outline.clone()];
            }
            for angle_deg in [0.0, 45.0, 90.0] {
                for wall_pass in [dc_project::WallPass::Last, dc_project::WallPass::First] {
                    let op = dc_project::PocketOp {
                        strategy: dc_project::ClearanceStrategy::Raster { angle_deg },
                        wall_pass,
                        ..op.clone()
                    };
                    project.operations = vec![Operation::Pocket(op.clone())];
                    let text = emit_pocket(&project, &op, None);
                    assert!(
                        text.is_ok(),
                        "{stock_to_leave_mm} mm, {angle_deg}°, {wall_pass:?}: {text:?}"
                    );
                }
            }
        }
    }
}

/// S9 alone on hand-written `text`, as operation `op` of `project` with
/// the project's tool.
fn check_material_text(text: &str, project: &Project, op: &Operation) -> Result<(), ExportError> {
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(project, &post).unwrap();
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let tool = project.tools[0].clone();
    // As `Material::held_run`: tabs that do not resolve leave no
    // material.
    let (material, tabs) = match material::tab_spines(project, op) {
        Some(tabs) => (Material::of(project, op), tabs),
        None => (Material::Unresolved, Vec::new()),
    };
    let toolpath = Toolpath::default();
    let span = OpSpan {
        op,
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

/// S17 for a V-Carve (exact since #408; `crate::material`'s module
/// docs), on a hand-written program for a V-Carve of the sample
/// rectangle (X 10 to 60, Y 10 to 40, stock-top origin) with the 90°
/// 6.35 mm V-bit (cone 3.175 mm): a cut 2 mm deep well inside passes,
/// after a plunge from above; a cut whose tip leaves the rectangle
/// fails, so does one whose 2 mm cone reaches past the wall and one
/// sideways above the start plane (`ConeOffMaterial`), and one deeper
/// than the cone (`ConeHeightExceeded`).
#[test]
fn hand_written_v_carve_off_its_region_fails_s9() {
    let mut project = sample_project();
    project
        .tools
        .insert(0, dc_project::test_support::tool_v_bit_90());
    let op = Operation::VCarve(dc_project::test_support::sample_v_carve());
    let program = |cuts: &str| format!("G21 G90\nG54\nG0 Z5\nX20 Y25\nG0 Z1\n{cuts}G0 Z5\n");
    let ok = program("G1 Z-2 F300\nX50 F1000\n");
    check_material_text(&ok, &project, &op).unwrap();
    for (what, cuts) in [
        ("tip off the rectangle", "G1 Z-2 F300\nX61 F1000\n"),
        ("cone past the wall", "G1 Z-2 F300\nX59 F1000\n"),
        (
            "sideways above the start plane",
            "G1 Z0.5 F300\nX30 F1000\n",
        ),
    ] {
        let err = check_material_text(&program(cuts), &project, &op);
        assert!(
            matches!(err, Err(ExportError::ConeOffMaterial { line: 7, .. })),
            "{what}: {err:?}"
        );
    }
    let err = check_material_text(&program("G1 Z-3.5 F300\nX30 F1000\n"), &project, &op);
    assert!(
        matches!(err, Err(ExportError::ConeHeightExceeded { line: 6, .. })),
        "below the cone: {err:?}"
    );
}

/// The sample project with a D-shaped hole in its rectangle: an arc of
/// `radius_mm` about (35, 25), 270° round through its bottom (bulge tan
/// 67.5°, past 1, a half turn), closed by a chord across its top.
fn sample_project_with_d_hole(radius_mm: f64) -> Project {
    use dc_geometry::{Polyline, Vertex};
    let mut project = sample_project();
    let at = |deg: f64| {
        let rad = f64::to_radians(deg);
        (25.0 + radius_mm * rad.cos(), 15.0 + radius_mm * rad.sin())
    };
    let ((sx, sy), (ex, ey)) = (at(135.0), at(45.0));
    let hole = Polyline::new(
        vec![
            Vertex::new(sx, sy, (f64::to_radians(270.0) / 4.0).tan()),
            Vertex::line(ex, ey),
        ],
        true,
    );
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles.push(hole);
    }
    project
}

/// S17 refuses a level V-Carve cut across a wall arc that sweeps past
/// a half turn. With a D hole of radius 3, a cut 0.2 mm deep along Y
/// 24.5 from X 20 to X 40 has its ends and middle in the material and
/// crosses the hole through the arc twice, so it is off its region; it
/// used to read about 2 mm from the arc and pass. The same cut along Y
/// 21.5, 0.5 mm below the arc's bottom, still passes.
#[test]
fn level_v_carve_cut_across_an_arc_past_a_half_turn_fails_s17() {
    let mut project = sample_project_with_d_hole(3.0);
    project
        .tools
        .insert(0, dc_project::test_support::tool_v_bit_90());
    let op = Operation::VCarve(dc_project::test_support::sample_v_carve());
    let program =
        |y: f64| format!("G21 G90\nG54\nG0 Z5\nX20 Y{y}\nG0 Z1\nG1 Z-0.2 F300\nX40 F1000\nG0 Z5\n");
    check_material_text(&program(21.5), &project, &op).unwrap();
    let err = check_material_text(&program(24.5), &project, &op);
    assert!(
        matches!(err, Err(ExportError::ConeOffMaterial { line: 7, .. })),
        "{err:?}"
    );
}

/// S9 refuses a Pocket cut across an island arc that sweeps past a
/// half turn. With a D island of radius 8 and the 6.35 mm tool, a cut
/// along Y 24.5 from X 15 to X 55 starts and ends in the pocket and
/// crosses the island through its arc twice; it used to read over 6 mm
/// from the island (the arc's ends) and pass. The same cut along Y
/// 13.5, 3.5 mm below the arc's bottom, still passes.
#[test]
fn pocket_cut_across_an_island_arc_past_a_half_turn_fails_s9() {
    let project = sample_project_with_d_hole(8.0);
    let op = Operation::Pocket(dc_project::test_support::sample_pocket());
    let program =
        |y: f64| format!("G21 G90\nG54\nG0 Z5\nX15 Y{y}\nG1 Z-3 F300\nX55 F1000\nG0 Z5\n");
    check_material_text(&program(13.5), &project, &op).unwrap();
    let err = check_material_text(&program(24.5), &project, &op);
    assert!(
        matches!(err, Err(ExportError::OffMaterial { line: 6, .. })),
        "{err:?}"
    );
}

/// S9 on a hand-written program for the sample rectangle's Pocket
/// (X 10 to 60, Y 10 to 40, stock-top origin): a cut past the wall
/// fails, so does an arc whose ends are inside but whose bulge takes
/// the tool past it, and so does a plunge outside the region.
#[test]
fn hand_written_pocket_cut_off_material_fails_s9() {
    let project = sample_project();
    let op = Operation::Pocket(dc_project::test_support::sample_pocket());
    let program = |cuts: &str| format!("G21 G90\nG54\nG0 Z5\nX20 Y25\nG1 Z-3 F300\n{cuts}G0 Z5\n");
    let ok = program("X56.8 F1000\nG3 X43.2 Y25 I-6.8 J0\n");
    check_material_text(&ok, &project, &op).unwrap();
    let past_wall = program("X58 F1000\n");
    let err = check_material_text(&past_wall, &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
    // Centre (56.8, 30), radius 5, from the bottom round through X 61.8.
    let bulge = program("X56.8 F1000\nG3 X56.8 Y35 I0 J5\n");
    let err = check_material_text(&bulge, &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 7, .. }),
        "{err:?}"
    );
    let outside = "G21 G90\nG54\nG0 Z5\nX5 Y25\nG1 Z-3 F300\nG0 Z5\n";
    let err = check_material_text(outside, &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 5, .. }),
        "{err:?}"
    );
}

/// The sample rectangle's Pocket with `relief`.
fn relief_pocket(relief: dc_project::CornerRelief) -> Operation {
    Operation::Pocket(dc_project::PocketOp {
        corner_relief: relief,
        ..dc_project::test_support::sample_pocket()
    })
}

/// S9's notch rule (`crate::material`): with corner relief on, a
/// Pocket cut may come nearer a wall than the tool radius in a
/// corner's relief zone. At the sample rectangle's corner (10, 10)
/// with the 6.35 mm tool, the dogbone from the miter point (13.175,
/// 13.175) to one radius from the corner, (12.245, 12.245), and the
/// T-bone to (13.175, 10) pass; with relief off the same dogbone
/// fails at its cut.
#[test]
fn pocket_notch_inside_its_corner_zone_passes_s9() {
    use dc_project::CornerRelief;
    let project = sample_project();
    let program =
        |cuts: &str| format!("G21 G90\nG54\nG0 Z5\nX13.175 Y13.175\nG1 Z-3 F300\n{cuts}G0 Z5\n");
    let dogbone = program("X12.245 Y12.245 F1000\nX13.175 Y13.175\n");
    check_material_text(&dogbone, &project, &relief_pocket(CornerRelief::Dogbone)).unwrap();
    let t_bone = program("X13.175 Y10 F1000\nX13.175 Y13.175\n");
    check_material_text(&t_bone, &project, &relief_pocket(CornerRelief::TBone)).unwrap();
    let err =
        check_material_text(&dogbone, &project, &relief_pocket(CornerRelief::None)).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
}

/// S9's notch rule holds the notch to its corner: a cut that drives
/// the tool centre to the corner itself fails, and so does a cut past
/// a wall away from every corner.
#[test]
fn pocket_notch_past_the_corner_fails_s9() {
    use dc_project::CornerRelief;
    let project = sample_project();
    let op = relief_pocket(CornerRelief::Dogbone);
    let to_corner = "G21 G90\nG54\nG0 Z5\nX13.175 Y13.175\nG1 Z-3 F300\nX10 Y10 F1000\nG0 Z5\n";
    let err = check_material_text(to_corner, &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
    let mid_wall = "G21 G90\nG54\nG0 Z5\nX30 Y13.175\nG1 Z-3 F300\nY11 F1000\nG0 Z5\n";
    let err = check_material_text(mid_wall, &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
}

/// S9's notch rule on an arc (2026-11-18, `crate::material`): arc
/// fitting may write the handoff from an entry and the notch after it
/// as one arc, which runs from clear stock into the corner's relief
/// zone. At the sample
/// rectangle's corner (10, 10) with the 6.35 mm tool, an arc of radius
/// 14.835 from (15, 14.6) through the miter point (13.175, 13.175) to
/// the dogbone's end (12.245, 12.245) passes with relief on and fails
/// with it off.
#[test]
fn arc_from_clear_stock_into_a_notch_passes_s9() {
    use dc_project::CornerRelief;
    let project = sample_project();
    let arc = "G21 G90\nG54\nG0 Z5\nX15 Y14.6\nG1 Z-3 F300\n\
               G3 X12.245 Y12.245 I8.19 J-12.37 F1000\nG0 Z5\n";
    check_material_text(arc, &project, &relief_pocket(CornerRelief::Dogbone)).unwrap();
    let err = check_material_text(arc, &project, &relief_pocket(CornerRelief::None)).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
}

/// S9's notch rule holds an arc to clear stock and the relief zone,
/// point by point. From the same start through the same
/// miter point, an arc on to the corner itself fails, and so does one
/// that leaves the zone through a wall, to (12.6, 11.2), 1.2 mm from
/// the bottom wall and 2.86 mm from the corner. So does an arc that
/// reaches the dogbone's end from (20, 13.175) by way of (16, 11),
/// 1 mm from the bottom wall and 6.1 mm from the corner, outside the
/// zone.
#[test]
fn arc_into_a_notch_off_its_zone_fails_s9() {
    use dc_project::CornerRelief;
    let project = sample_project();
    let op = relief_pocket(CornerRelief::Dogbone);
    let program = |start: &str, arc: &str| {
        format!("G21 G90\nG54\nG0 Z5\n{start}\nG1 Z-3 F300\n{arc} F1000\nG0 Z5\n")
    };
    for (start, arc) in [
        ("X15 Y14.6", "G3 X10 Y10 I16.187 J-22.612"),
        ("X15 Y14.6", "G3 X12.6 Y11.2 I1.158 J-3.365"),
        ("X20 Y13.175", "G2 X12.245 Y12.245 I-4.313 J3.166"),
    ] {
        let err = check_material_text(&program(start, arc), &project, &op).unwrap_err();
        assert!(
            matches!(err, ExportError::OffMaterial { line: 6, .. }),
            "{arc}: {err:?}"
        );
    }
}

/// The sample project's outside Profile.
fn sample_profile_op() -> Operation {
    sample_project().operations[0].clone()
}

/// S9 refuses a straight plunge into a tab below its top. The sample
/// rectangle (X 10 to 60, Y 10 to 40) cut outside with a placed 6 mm
/// tab in the middle of its bottom edge, its top 2 mm above the stock
/// bottom (program Z -4). Plunging to Z -5 on the bit's path (Y 6.825)
/// at the tab's centre, or 2 mm along from it, is refused at the
/// plunge; the same plunge to Z -3.5, above the tab, passes, and so
/// does one past the tab's stretch, where a rect tab steps down, and
/// one far from it. A plunge at the centre of a tab a request places is
/// refused too.
#[test]
fn s9_refuses_a_descent_into_a_tab() {
    let mut project = sample_project();
    dc_project::test_support::profile_mut(&mut project).tabs = None;
    project.tabs = vec![dc_project::PlacedTab {
        height_mm: 2.0,
        ..dc_project::test_support::sample_tab("tab1", 25.0, 0.0)
    }];
    let op = sample_profile_op_of(&project);
    let program =
        |x: f64, y: f64, z: f64| format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG1 Z{z} F300\nG0 Z5\n");
    for (x, z) in [(50.0, -5.0), (35.0, -3.5), (41.175, -5.0)] {
        check_material_text(&program(x, 6.825, z), &project, &op)
            .unwrap_or_else(|e| panic!("X{x} Z{z}: {e:?}"));
    }
    for x in [35.0, 37.0] {
        let err = check_material_text(&program(x, 6.825, -5.0), &project, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
            "X{x}: {err:?}"
        );
    }

    // The sample's own request, four 6 × 2 mm tabs: plunge at one.
    let mut project = sample_project();
    let op = sample_profile_op_of(&project);
    let spec = dc_project::test_support::profile_mut(&mut project)
        .tabs
        .clone()
        .unwrap();
    let placed = dc_cam::placed_tabs::place(&project, op.id(), &spec).unwrap();
    let tab = &placed.tabs[0];
    let at = project.instances[0]
        .transform
        .apply(Point::new(tab.x_mm, tab.y_mm));
    let err = check_material_text(&program(at.x, at.y, -5.5), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { line: 5, .. }),
        "{err:?}"
    );
}

/// The sample project cut by its Profile on `side` with a 1 mm last
/// pass stock to leave, no tab request, and one placed 2 × 2 mm rect tab at
/// (`x_mm`, `y_mm`) on its outline, drawn `width_mm` wide: the 6.35 mm
/// tool's tab spans 4.175 mm either way along the lap.
fn last_pass_tab_project(side: dc_project::Side, width_mm: f64, at: (f64, f64)) -> Project {
    let mut project = sample_project();
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![dc_geometry::Polyline::rect(
            Point::new(0.0, 0.0),
            width_mm,
            30.0,
        )];
    }
    let profile = dc_project::test_support::profile_mut(&mut project);
    profile.side = side;
    profile.tabs = None;
    profile.last_pass = Some(dc_project::LastPass {
        stock_to_leave_mm: 1.0,
        thickness_mm: None,
        reverse: false,
    });
    project.tabs = vec![dc_project::PlacedTab {
        width_mm: 2.0,
        ..dc_project::test_support::sample_tab("tab1", at.0, at.1)
    }];
    project
}

/// A plunge from Z 5 to Z -5 at (`x`, `y`), below a 2 mm tab's top
/// (program Z -4).
fn plunge_at(x: f64, y: f64) -> String {
    format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG1 Z-5 F300\nG0 Z5\n")
}

/// S9 holds a stock pass off a tab only as far as the tab material the
/// last pass's lap leaves standing, not the lap's span. An inside
/// Profile of an 8.5 mm wide slot (X 10 to 18.5, Y 10 to 40) with a
/// tab on its left wall at Y 25: the lap on the right wall (X 15.325)
/// cuts the far side of the left lap's kerf down to its floor (X 12.15
/// on), so the stock cutter on the left stock path (X 14.175) clears
/// the tab the left lap leaves `4.175 − √(r² − 1.025²) + √(r² −
/// 2.025²)` = 3.62 mm along from the tab. A plunge there 3.75 mm along
/// either way, inside the lap's span less a tenth of the radius (3.86
/// mm), cuts none of it and passes.
#[test]
fn stock_to_leave_pass_leaves_a_tab_at_its_stretch_end() {
    let project = last_pass_tab_project(dc_project::Side::Inside, 8.5, (0.0, 15.0));
    let op = sample_profile_op_of(&project);
    check_material_text(&plunge_at(14.175, 28.75), &project, &op).unwrap();
    check_material_text(&plunge_at(14.175, 21.25), &project, &op).unwrap();
}

/// A stock pass that comes down onto the tab material the lap leaves still
/// fails S9. In the slot of
/// [`stock_to_leave_pass_leaves_a_tab_at_its_stretch_end`], a plunge 3.3 or
/// 3.55 mm along the left stock path from the tab, short of the 3.62 mm
/// where its cutter clears the tab, or at the tab, is refused.
#[test]
fn stock_to_leave_pass_dropping_onto_a_tab_fails_s9() {
    let project = last_pass_tab_project(dc_project::Side::Inside, 8.5, (0.0, 15.0));
    let op = sample_profile_op_of(&project);
    for y in [28.3, 21.7, 28.55, 21.45, 25.0] {
        let err = check_material_text(&plunge_at(14.175, y), &project, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { line: 5, .. }),
            "Y{y}: {err:?}"
        );
    }
}

/// On a straight wall the lap's kerf keeps its far side, where the tab
/// stands the lap's whole span (4.175 mm) either side of the tab, and
/// the stock cutter reaches it until it is `√(2ar − a²)` = 2.31 mm
/// farther along: 6.49 mm in all, the last half millimetre of it the
/// tab's tip, thinner than the slack. The sample rectangle cut outside
/// with a tab on its bottom edge at X 35: a plunge on the stock path
/// (Y 5.825) 5 or 5.8 mm along, past the lap's span less a tenth of
/// the radius (3.86 mm), is refused; one 6.7 mm along passes.
#[test]
fn stock_to_leave_pass_past_the_span_onto_a_tab_fails_s9() {
    let project = last_pass_tab_project(dc_project::Side::Outside, 50.0, (25.0, 0.0));
    let op = sample_profile_op_of(&project);
    for x in [40.0, 30.0, 40.8, 29.2] {
        let err = check_material_text(&plunge_at(x, 5.825), &project, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { line: 5, .. }),
            "X{x}: {err:?}"
        );
    }
    for x in [41.7, 28.3] {
        check_material_text(&plunge_at(x, 5.825), &project, &op)
            .unwrap_or_else(|e| panic!("X{x}: {e:?}"));
    }
}

/// S9 refuses a ramp into stock no earlier cut cleared. The sample
/// rectangle (X 10 to 60, Y 10 to 40) cut outside, no tabs: the first
/// pass ramps from the stock top out 18.8 mm into the waste from the
/// right edge's tool-centre path (X 63.175) and back, laps, and the
/// second ramps from its floor along the same line, which passes. The
/// second pass ramping from the same point 19 mm along a line no cut
/// ran on, into full-height stock, is refused at that line; so is one
/// that starts on the floor after a straight plunge from above the
/// stock. The first pass's ramp, from the stock top, is its own bore.
#[test]
fn s9_refuses_a_ramp_into_uncut_stock() {
    let mut project = sample_project();
    dc_project::test_support::profile_mut(&mut project).tabs = None;
    let op = sample_profile_op_of(&project);
    let program = |ramp: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nX63.175 Y25\nG1 Z0 F300\nX82 Z-1 F500\nX63.175 Z-2\n\
             Y40 F1000\nY25\n{ramp}\nX63.175 Y25 Z-4\nY40 F1000\nG0 Z5\n"
        )
    };
    check_material_text(&program("X82 Z-3 F500"), &project, &op).unwrap();
    let err = check_material_text(&program("X81.175 Y31 Z-3 F500"), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 10 } if at == op.id()),
        "{err:?}"
    );
    // A plunge from above the stock does not make the ramp after it
    // one that starts above the stock.
    let plunged = "G21 G90\nG54\nG0 Z5\nX63.175 Y25\nG1 Z0 F300\nX82 Z-1 F500\n\
                   X63.175 Z-2\nY40 F1000\nY25\nG0 Z5\nG0 Z1\nG1 Z-2 F300\n\
                   X81.175 Y31 Z-3 F500\nG0 Z5\n";
    let err = check_material_text(plunged, &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { line: 13, .. }),
        "{err:?}"
    );
}

/// The Profile of `project`.
fn sample_profile_op_of(project: &Project) -> Operation {
    project.operations[0].clone()
}

/// S9 for Profile: the sample's outside Profile of the rectangle X 10
/// to 60, Y 10 to 40 with the 6.35 mm tool. A ramp that zigzags out
/// into the waste and back to the tool-centre path (X 6.825), then a
/// lap along it, passes; the same ramp swung into the part, its tool
/// centre at X 12, fails with `OffMaterial` at the ramp's line.
#[test]
fn profile_ramp_into_part_fails_s9() {
    let project = sample_project();
    let op = sample_profile_op();
    let program = |ramp: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nX6.825 Y25\nG1 Z0 F300\n{ramp}\nX6.825 Y25 Z-2\n\
             Y40 F1000\nG2 X10 Y43.175 I3.175 J0\nG0 Z5\n"
        )
    };
    check_material_text(&program("X2 Y30 Z-1 F500"), &project, &op).unwrap();
    let err = check_material_text(&program("X12 Y30 Z-1 F500"), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 6 } if at == op.id()),
        "{err:?}"
    );
}

/// The sample project with a second part, 20 × 30 mm at (`x`, 10), in
/// the Profile's targets: `x − 60` mm right of the rectangle.
fn two_part_profile(x: f64) -> (Project, Operation) {
    let mut project = sample_project();
    let narrow = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d2"),
        name: "Narrow".into(),
        profiles: vec![dc_geometry::Polyline::rect(
            Point::new(0.0, 0.0),
            20.0,
            30.0,
        )],
    };
    project.definitions.insert(narrow.id.clone(), narrow);
    let mut instance = project.instances[0].clone();
    instance.id = dc_project::InstanceId::new("i2");
    instance.definition = dc_project::DefinitionId::new("d2");
    instance.transform = dc_geometry::Transform::translate(x, 10.0);
    project.instances.push(instance);
    let op = dc_project::test_support::profile_mut(&mut project);
    op.targets.push(dc_project::InstanceId::new("i2"));
    let op = Operation::Profile(op.clone());
    (project, op)
}

/// S9 for Profile: a lead-in arc onto the rectangle's right-hand
/// tool-centre path (X 63.175) that stays in the 6.825 mm gap before
/// the second part passes; one that swings out to X 75.675, into the
/// second part, fails with `OffMaterial` at the arc's line.
#[test]
fn profile_lead_arc_into_neighbouring_part_fails_s9() {
    let (project, op) = two_part_profile(70.0);
    let program = |start_y: f64, arc: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nX63.175 Y{start_y}\nG1 Z-1 F300\n{arc}\n\
             Y10 F1000\nG0 Z5\n"
        )
    };
    let small = program(31.0, "G2 X63.175 Y25 I0 J-3 F1000");
    check_material_text(&small, &project, &op).unwrap();
    let wide = program(50.0, "G2 X63.175 Y25 I0 J-12.5 F1000");
    let err = check_material_text(&wide, &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 6 } if at == op.id()),
        "{err:?}"
    );
}

/// S9's lap rule: with the second part 4 mm from the rectangle, closer
/// than the 6.35 mm bit (`toolpaths_overlap`), the generated Profile
/// still exports: each lap runs at the radius from its own outline,
/// and the gouge into the neighbour is the one dc-cam warns about. A
/// cut off the lap, to a tool centre in the middle of the gap, 2 mm
/// from both parts, still fails.
#[test]
fn lap_beside_a_close_part_passes_s9_and_a_cut_off_it_fails() {
    let (project, op) = two_part_profile(64.0);
    let Operation::Profile(profile) = &op else {
        unreachable!("a Profile");
    };
    let (toolpath, warnings) = generate_profile(&project, profile).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.code() == dc_cam::WarningCode::ToolpathsOverlap),
        "{warnings:?}"
    );
    emit(
        &project,
        &[(op.clone(), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .unwrap();
    let program =
        |cut: &str| format!("G21 G90\nG54\nG0 Z5\nX63.175 Y20\nG1 Z-1 F300\n{cut}\nG0 Z5\n");
    check_material_text(&program("Y30 F1000"), &project, &op).unwrap();
    let err = check_material_text(&program("X62 Y25 F1000"), &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
}

/// S9's notch rule on an outside Profile: the generated dogbone in an
/// L part's inside corner passes; the same program checked as a
/// Profile without relief fails at the notch.
#[test]
fn outside_notch_passes_s9_only_with_relief() {
    let mut project = sample_project();
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles = vec![dc_geometry::Polyline::new(
            [
                (0.0, 0.0),
                (40.0, 0.0),
                (40.0, 40.0),
                (20.0, 40.0),
                (20.0, 20.0),
                (0.0, 20.0),
            ]
            .into_iter()
            .map(|(x, y)| dc_geometry::Vertex::line(x, y))
            .collect(),
            true,
        )];
    }
    let op = dc_project::test_support::profile_mut(&mut project);
    op.tabs = None;
    op.corner_relief = dc_project::CornerRelief::Dogbone;
    let op = op.clone();
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    let text = emit(
        &project,
        &[(
            Operation::Profile(op.clone()),
            project.tools[0].clone(),
            toolpath,
        )
            .into()],
        "2026-10-21",
    )
    .map(|(p, _)| p.text)
    .unwrap();
    let without = Operation::Profile(dc_project::ProfileOp {
        corner_relief: dc_project::CornerRelief::None,
        ..op
    });
    let err = check_material_text(&text, &project, &without).unwrap_err();
    assert!(matches!(err, ExportError::OffMaterial { .. }), "{err:?}");
}

/// The sample's outside Profile with T-bones of a 60 × 40 mm frame at
/// (90, 60) with a 30 × 20 mm hole at (105, 70). The T-bone in the
/// hole's corner (105, 70) runs from the miter point (108.175,
/// 73.175) to (108.175, 70) on the hole's long wall. Its return leg,
/// written from a start the rounding moved 0.0005 mm into the part,
/// passes; one starting 0.1 mm inside the part fails: the slack S9
/// judges the side with does not open the part. The tool gets to each
/// start by rapids, which S9 does not hold to the material, so only
/// the return leg is judged.
#[test]
fn outside_cut_starting_inside_the_part_is_rejected() {
    let mut project = sample_project();
    project.stock = dc_project::Stock {
        width_mm: 240.0,
        height_mm: 160.0,
        thickness_mm: Some(6.0),
    };
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles = vec![
            dc_geometry::Polyline::rect(Point::new(0.0, 0.0), 60.0, 40.0),
            dc_geometry::Polyline::rect(Point::new(15.0, 10.0), 30.0, 20.0),
        ];
    }
    project.instances[0].transform = dc_geometry::Transform::translate(90.0, 60.0);
    let op = dc_project::test_support::profile_mut(&mut project);
    op.tabs = None;
    op.corner_relief = dc_project::CornerRelief::TBone;
    let op = Operation::Profile(op.clone());
    let program =
        |y: &str| format!("G21 G90\nG54\nG0 Z5\nX108.175 Y{y}\nZ-1\nG1 Y73.175 F1000\nG0 Z5\n");
    check_material_text(&program("69.9995"), &project, &op).unwrap();
    let err = check_material_text(&program("69.9"), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 6 } if at == op.id()),
        "{err:?}"
    );
}

/// S9 for an inside Profile: the tool centre stays inside the hole,
/// a radius from its walls, as a Pocket's; a cut that crosses out of
/// the hole fails.
#[test]
fn inside_profile_cut_outside_hole_fails_s9() {
    let mut project = sample_project();
    dc_project::test_support::profile_mut(&mut project).side = dc_project::Side::Inside;
    let op = project.operations[0].clone();
    let program = |to_x: &str| {
        format!("G21 G90\nG54\nG0 Z5\nX13.175 Y25\nG1 Z-1 F300\nX{to_x} F1000\nG0 Z5\n")
    };
    check_material_text(&program("56.825"), &project, &op).unwrap();
    let err = check_material_text(&program("58"), &project, &op).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line: 6, .. }),
        "{err:?}"
    );
}

/// S9's lap rule covers only what `toolpaths_overlap` warns about,
/// shapes in different regions. An inside Profile of the sample
/// rectangle (X 10 to 60, Y 10 to 40) with a round island at (35, 25)
/// leaves a band of 15 mm less the island's radius above and below it.
/// With a radius-5 island (a 10 mm band) a lap round the island at the
/// tool radius passes; with a radius-13 island (2 mm, narrower than
/// the 6.35 mm bit) the same lap, at 16.175 mm from the centre, runs
/// past the rectangle's top edge and fails at its first arc, although
/// it runs along the island at the radius on its waste side.
#[test]
fn inside_profile_lap_in_a_band_narrower_than_the_tool_fails_s9() {
    let ring = |island_mm: f64| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles.push(dc_geometry::Polyline::circle(
                Point::new(25.0, 15.0),
                island_mm,
            ));
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = dc_project::Side::Inside;
        op.tabs = None;
        let op = Operation::Profile(op.clone());
        (project, op)
    };
    let lap = |radius_mm: f64| {
        let (east, west) = (35.0 + radius_mm, 35.0 - radius_mm);
        format!(
            "G21 G90\nG54\nG0 Z5\nX{east} Y25\nG1 Z-1 F300\n\
             G3 X{west} Y25 I-{radius_mm} J0 F1000\nG3 X{east} Y25 I{radius_mm} J0\nG0 Z5\n"
        )
    };
    let (wide, op) = ring(5.0);
    check_material_text(&lap(8.175), &wide, &op).unwrap();
    let (narrow, op) = ring(13.0);
    let err = check_material_text(&lap(16.175), &narrow, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 6 } if at == op.id()),
        "{err:?}"
    );
}

/// S9 refuses what `generate_profile` refuses. An On line Profile of
/// the sample rectangle (X 10 to 60, Y 10 to 40) with a round hole at
/// (35, 25): a lap along the hole's line passes when the ring around
/// it is 10 mm wide, and fails at its first arc when it is 5 mm
/// (narrower than the 6.35 mm bit, wider than its radius) or 2 mm,
/// although it runs on a target line: the two lines' cuts would cut
/// through the ring. A cut along the left edge, 15 mm from the 10 mm
/// hole, still passes.
#[test]
fn on_line_profile_in_a_ring_narrower_than_the_tool_fails_s9() {
    let ring = |hole_mm: f64| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles.push(dc_geometry::Polyline::circle(
                Point::new(25.0, 15.0),
                hole_mm,
            ));
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = dc_project::Side::OnLine;
        op.tabs = None;
        let op = Operation::Profile(op.clone());
        (project, op)
    };
    let lap = |radius_mm: f64| {
        let (east, west) = (35.0 + radius_mm, 35.0 - radius_mm);
        format!(
            "G21 G90\nG54\nG0 Z5\nX{east} Y25\nG1 Z-1 F300\n\
             G3 X{west} Y25 I-{radius_mm} J0 F1000\nG3 X{east} Y25 I{radius_mm} J0\nG0 Z5\n"
        )
    };
    let (wide, op) = ring(5.0);
    check_material_text(&lap(5.0), &wide, &op).unwrap();
    for hole_mm in [10.0, 13.0] {
        let (narrow, op) = ring(hole_mm);
        let err = check_material_text(&lap(hole_mm), &narrow, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { op: at, line: 6 } if at == op.id()),
            "hole {hole_mm}: {err:?}"
        );
    }
    let (narrow, op) = ring(10.0);
    let left_edge = "G21 G90\nG54\nG0 Z5\nX10 Y15\nG1 Z-1 F300\nY35 F1000\nG0 Z5\n";
    check_material_text(left_edge, &narrow, &op).unwrap();
}

/// An On line Profile cuts its targets' open paths, so S9 holds it to
/// them as an Engrave: the generated program of an open line (with
/// the sample's tabs and a ramp along the line) passes, and a
/// hand-written cut beside the line fails.
#[test]
fn on_line_profile_of_an_open_path_passes_s9() {
    let mut project = sample_project();
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles.push(dc_geometry::Polyline::new(
            vec![
                dc_geometry::Vertex::line(0.0, 50.0),
                dc_geometry::Vertex::line(60.0, 50.0),
            ],
            false,
        ));
    }
    let op = dc_project::test_support::profile_mut(&mut project);
    op.side = dc_project::Side::OnLine;
    op.entry = dc_project::Entry::Ramp { angle_deg: 15.0 };
    let op = op.clone();
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    let op = Operation::Profile(op);
    emit(
        &project,
        &[(op.clone(), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .unwrap();
    let along = "G21 G90\nG54\nG0 Z5\nX10 Y60\nG1 Z-1 F300\nX70 F1000\nG0 Z5\n";
    check_material_text(along, &project, &op).unwrap();
    let beside = "G21 G90\nG54\nG0 Z5\nX10 Y62\nG1 Z-1 F300\nX70 F1000\nG0 Z5\n";
    let err = check_material_text(beside, &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
        "the plunge beside the line: {err:?}"
    );
}

/// A Left or Right Profile's cuts stay on the open path offset by the
/// tool radius to the chosen side, and keep the radius from every line
/// of the operation and from the strip one radius wide each line keeps
/// on its other side. The sample with an open line from (10, 60) to
/// (70, 60) cut Left, with its tabs and a ramp: the generated program
/// passes, and so does a hand-written cut at Y 63.175 (the 6.35 mm
/// bit's radius to the left of the line). Cuts on the line, at the
/// radius to its right, half a radius off the offset, or past the
/// line's end along it fail at their plunge; so does the same cut when
/// the line is cut Right. (The cut past the end plunges on the path's
/// end and fails at the move beyond it.)
#[test]
fn left_right_cut_off_its_path_fails_s9() {
    let with_line = |side: dc_project::Side| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles.push(dc_geometry::Polyline::new(
                vec![
                    dc_geometry::Vertex::line(0.0, 50.0),
                    dc_geometry::Vertex::line(60.0, 50.0),
                ],
                false,
            ));
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = side;
        op.entry = dc_project::Entry::Ramp { angle_deg: 15.0 };
        let op = op.clone();
        (project, op)
    };
    let (project, op) = with_line(dc_project::Side::Left);
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    let op = Operation::Profile(op);
    emit(
        &project,
        &[(op.clone(), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .unwrap();
    let cut = |from: (f64, f64), to: (f64, f64)| {
        format!(
            "G21 G90\nG54\nG0 Z5\nX{} Y{}\nG1 Z-1 F300\nX{} Y{} F1000\nG0 Z5\n",
            from.0, from.1, to.0, to.1
        )
    };
    check_material_text(&cut((10.0, 63.175), (70.0, 63.175)), &project, &op).unwrap();
    for (case, from, to, at_line) in [
        ("on the line", (10.0, 60.0), (70.0, 60.0), 5),
        ("to its right", (10.0, 56.825), (70.0, 56.825), 5),
        ("half a radius off", (10.0, 64.8), (70.0, 64.8), 5),
        ("past its end", (70.0, 63.175), (80.0, 63.175), 6),
    ] {
        let err = check_material_text(&cut(from, to), &project, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { op: at, line } if at == op.id() && *line == at_line),
            "{case}: {err:?}"
        );
    }
    let (right, op) = with_line(dc_project::Side::Right);
    let op = Operation::Profile(op);
    check_material_text(&cut((10.0, 56.825), (70.0, 56.825)), &right, &op).unwrap();
    let err = check_material_text(&cut((10.0, 63.175), (70.0, 63.175)), &right, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { line: 5, .. }),
        "the Left cut under Right: {err:?}"
    );
}

/// S9 holds a plunge off a tab on an open path of a Left or Right
/// Profile, which it did not: a spine was built for a tab on a closed
/// outline and on an open path cut On line only. The sample with an
/// open line from (10, 60) to (70, 60), its 6.35 mm bit, and one tab
/// 6 mm wide whose top is 2 mm above the stock bottom (Z −4) asked
/// for 30 mm along the line, or at its first vertex. The bit runs
/// 3.175 mm to the cut side of the line, and the tab is centred on
/// that path: at (40, 63.175) Left, (40, 56.825) Right. Its stretch
/// is 6.175 mm either side of the centre.
///
/// The generated program passes. A hand-written plunge on the bit's
/// path passes at the tab's centre down to its top and fails 1 mm
/// below it; 5 mm along it fails too, to the stock bottom over a rect
/// tab and 0.1 mm above it on a smooth tab's slope, where it may go
/// to 0.571 mm above the bottom and passes 1 mm above it; 8 mm along
/// it is clear and passes to the stock bottom. The same holds at a
/// tab asked for at the line's first vertex, where the path starts.
#[test]
fn left_right_plunge_into_a_tab_on_an_open_path_fails_s9() {
    let with_tab = |side: dc_project::Side, shape: dc_project::TabShape, at_mm: f64| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles = vec![dc_geometry::Polyline::new(
                vec![
                    dc_geometry::Vertex::line(0.0, 50.0),
                    dc_geometry::Vertex::line(60.0, 50.0),
                ],
                false,
            )];
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = side;
        if let Some(tabs) = &mut op.tabs {
            tabs.placement = dc_project::TabPlacement::AtDistances {
                distances_mm: vec![at_mm],
            };
            tabs.shape = shape;
        }
        let op = op.clone();
        (project, op)
    };
    let plunge =
        |x: f64, y: f64, z: f64| format!("G21 G90\nG54\nG0 Z5\nX{x} Y{y}\nG1 Z{z} F300\nG0 Z5\n");
    for (side, y) in [
        (dc_project::Side::Left, 63.175),
        (dc_project::Side::Right, 56.825),
    ] {
        for shape in [dc_project::TabShape::Rect, dc_project::TabShape::Smooth] {
            for at_mm in [30.0, 0.0] {
                let case = format!("{side:?}, a {shape:?} tab at {at_mm} mm");
                let (project, op) = with_tab(side, shape, at_mm);
                let (toolpath, _) = generate_profile(&project, &op).unwrap();
                let op = Operation::Profile(op);
                emit(
                    &project,
                    &[(op.clone(), project.tools[0].clone(), toolpath).into()],
                    "2026-11-18",
                )
                .unwrap_or_else(|e| panic!("{case}: {e}"));
                let passes = |x: f64, z: f64| {
                    check_material_text(&plunge(x, y, z), &project, &op)
                        .unwrap_or_else(|e| panic!("{case}: a plunge at X{x} to Z{z}: {e}"));
                };
                let fails = |x: f64, z: f64| {
                    let err = check_material_text(&plunge(x, y, z), &project, &op).unwrap_err();
                    assert!(
                        matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
                        "{case}: a plunge at X{x} to Z{z}: {err:?}"
                    );
                };
                let centre = 10.0 + at_mm;
                passes(centre, -4.0);
                fails(centre, -5.0);
                match shape {
                    dc_project::TabShape::Rect => fails(centre + 5.0, -6.0),
                    dc_project::TabShape::Smooth => {
                        fails(centre + 5.0, -5.9);
                        passes(centre + 5.0, -5.0);
                    }
                }
                passes(centre + 8.0, -6.0);
            }
        }
    }
}

/// S9 holds a Left or Right cut out of the strip a line keeps,
/// bounded by the normals through its end vertices, whatever the
/// offset does there. The sample with a 30° V notch, tip at (50, 50),
/// down a 20 mm arm and up a 2 mm one, cut Left (inside it): its
/// offset by the radius (`offset_open`) is one piece beside the first
/// arm, stopped only by the circle round the end vertex, past the
/// normal there. A hand-written cut along that piece stays on the
/// offset and the radius from the line and from its kept edge, which
/// S9 alone accepted before #342; but at its end the cutter crosses
/// the end normal into the strip the 2 mm arm keeps, so it fails at
/// the move that gets there, and its first stretch, more than the
/// radius from that strip, passes. Cut Right (round the V), the
/// generated program passes.
#[test]
fn s9_refuses_a_cut_past_an_open_path_end() {
    let (sin, cos) = 15f64.to_radians().sin_cos();
    let v = [
        (40.0 - 20.0 * sin, 40.0 + 20.0 * cos),
        (40.0, 40.0),
        (40.0 + 2.0 * sin, 40.0 + 2.0 * cos),
    ];
    let with_notch = |side: dc_project::Side| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles.push(dc_geometry::Polyline::new(
                v.iter()
                    .map(|&(x, y)| dc_geometry::Vertex::line(x, y))
                    .collect(),
                false,
            ));
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = side;
        op.tabs = None;
        let op = op.clone();
        (project, op)
    };
    let placed = dc_geometry::Polyline::new(
        v.iter()
            .map(|&(x, y)| dc_geometry::Vertex::line(x + 10.0, y + 10.0))
            .collect(),
        false,
    );
    let pieces = dc_geometry::offset_open(&placed, 6.35 / 2.0);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    let ends: Vec<dc_geometry::Point> = pieces[0].vertices().iter().map(|v| v.point()).collect();
    let (start, end) = (ends[0], ends[ends.len() - 1]);
    let tip = dc_geometry::Point::new(v[2].0 + 10.0, v[2].1 + 10.0);
    let along = (tip - dc_geometry::Point::new(50.0, 50.0)) * 0.5;
    assert!(
        (end - tip).dot(along) > 0.0 && (end.distance_to(tip) - 6.35 / 2.0).abs() < 1e-6,
        "the piece ends past the end normal, on the circle round the end: {end:?}"
    );
    // 2 mm back from its end along the piece.
    let back = end + (start - end) * (2.0 / start.distance_to(end));
    let cut = |points: &[dc_geometry::Point]| {
        let mut text = format!(
            "G21 G90\nG54\nG0 Z5\nX{:.4} Y{:.4}\nG1 Z-1 F300\n",
            points[0].x, points[0].y
        );
        for p in &points[1..] {
            text.push_str(&format!("X{:.4} Y{:.4} F1000\n", p.x, p.y));
        }
        text.push_str("G0 Z5\n");
        text
    };
    let (project, op) = with_notch(dc_project::Side::Left);
    let op = Operation::Profile(op);
    check_material_text(&cut(&[start, back]), &project, &op).unwrap();
    let err = check_material_text(&cut(&[start, back, end]), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 7 } if at == op.id()),
        "the cut past the end normal: {err:?}"
    );
    let (project, op) = with_notch(dc_project::Side::Right);
    let (toolpath, _) = generate_profile(&project, &op).unwrap();
    emit(
        &project,
        &[(Operation::Profile(op), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .unwrap();
}

/// S9 refuses what `generate_profile` refuses. An On line Profile of
/// the sample rectangle (X 10 to 60, Y 10 to 40) with an open line 2
/// mm above its bottom edge: the generator refuses it
/// (`BandNarrowerThanTool`), and a hand-written cut along the line
/// fails at its plunge, although it is on a target line: with the
/// outline's cut it would cut through the wall between them. The same
/// line a tool width (6.35 mm) above the edge passes, and so does a
/// slot drawn from the bottom edge into the part, the generated
/// program included: there the cut opens into the outline's.
#[test]
fn on_line_open_path_cutting_through_a_wall_fails_s9() {
    type Line = ((f64, f64), (f64, f64));
    let with_lines = |lines: &[Line]| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles.extend(lines.iter().map(|&(from, to)| {
                dc_geometry::Polyline::new(
                    vec![
                        dc_geometry::Vertex::line(from.0, from.1),
                        dc_geometry::Vertex::line(to.0, to.1),
                    ],
                    false,
                )
            }));
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = dc_project::Side::OnLine;
        op.tabs = None;
        let op = op.clone();
        (project, op)
    };
    // The rectangle sits at (10, 10): project = drawn + 10.
    let cut = |from: (f64, f64), to: (f64, f64)| {
        format!(
            "G21 G90\nG54\nG0 Z5\nX{} Y{}\nG1 Z-1 F300\nX{} Y{} F1000\nG0 Z5\n",
            from.0 + 10.0,
            from.1 + 10.0,
            to.0 + 10.0,
            to.1 + 10.0
        )
    };

    let (near, op) = with_lines(&[((10.0, 2.0), (40.0, 2.0))]);
    let refused = generate_profile(&near, &op);
    assert!(
        matches!(refused, Err(dc_cam::CamError::BandNarrowerThanTool { .. })),
        "{refused:?}"
    );
    let op = Operation::Profile(op);
    let err = check_material_text(&cut((10.0, 2.0), (40.0, 2.0)), &near, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
        "the plunge 2 mm from the edge: {err:?}"
    );

    let (clear, op) = with_lines(&[((10.0, 6.35), (40.0, 6.35))]);
    let op = Operation::Profile(op);
    check_material_text(&cut((10.0, 6.35), (40.0, 6.35)), &clear, &op).unwrap();

    let (slot, op) = with_lines(&[((25.0, 0.0), (25.0, 15.0))]);
    check_material_text(
        &cut((25.0, 0.0), (25.0, 15.0)),
        &slot,
        &Operation::Profile(op),
    )
    .unwrap();

    // What the generator cuts, S9 passes: a slot square to the edge, a
    // saw cut, a slot at 60°, two slots joined at a corner, and two
    // lines crossing, with a ramp along each line.
    let steep = (
        25.0 + 15.0 * 60_f64.to_radians().cos(),
        15.0 * 60_f64.to_radians().sin(),
    );
    let drawings: [&[Line]; 5] = [
        &[((25.0, 0.0), (25.0, 15.0))],
        &[((25.0, 0.0), (25.0, 30.0))],
        &[((25.0, 0.0), steep)],
        &[((25.0, 0.0), (25.0, 15.0)), ((25.0, 15.0), (40.0, 15.0))],
        &[((10.0, 15.0), (40.0, 15.0)), ((25.0, 8.0), (25.0, 22.0))],
    ];
    for lines in drawings {
        let (project, mut op) = with_lines(lines);
        op.entry = dc_project::Entry::Ramp { angle_deg: 15.0 };
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        let op = Operation::Profile(op);
        emit(
            &project,
            &[(op, project.tools[0].clone(), toolpath).into()],
            "2026-10-21",
        )
        .unwrap_or_else(|e| panic!("{lines:?}: {e}"));
    }
}

/// S9 refuses what `generate_profile` refuses for a line closer than
/// the bit to itself. On line, with the sample rectangle (X 10 to 60,
/// Y 10 to 40, drawn at the origin): an open U in the part with legs 2
/// mm apart is refused by the generator, and a hand-written cut along
/// it fails at its plunge, although it is on a target line: the cuts
/// of the two legs would cut through the part between them. Legs 8 mm
/// apart pass. An outline with a 3 mm neck fails the same way; one
/// with a 3 mm notch passes, as the generator cuts it, because the
/// notch between its stretches is waste. The generated programs of the
/// wide U, a 60° V and the notched outline pass.
#[test]
fn on_line_hairpin_fails_s9() {
    use dc_geometry::{Polyline, Vertex};
    let polyline = |points: &[(f64, f64)], closed: bool| {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            closed,
        )
    };
    // The sample's definition with `profiles`, the rectangle kept when
    // `in_rect`.
    let with = |profiles: Vec<Polyline>, in_rect: bool| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            if !in_rect {
                def.profiles.clear();
            }
            def.profiles.extend(profiles);
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = dc_project::Side::OnLine;
        op.tabs = None;
        let op = op.clone();
        (project, op)
    };
    // A cut through `points`, drawn: project = drawn + 10.
    let cut = |points: &[(f64, f64)]| {
        let mut text = format!(
            "G21 G90\nG54\nG0 Z5\nX{} Y{}\nG1 Z-1 F300\n",
            points[0].0 + 10.0,
            points[0].1 + 10.0
        );
        for (x, y) in &points[1..] {
            text.push_str(&format!("X{} Y{} F1000\n", x + 10.0, y + 10.0));
        }
        text.push_str("G0 Z5\n");
        text
    };
    let fails_at_plunge = |text: &str, project: &Project, op: &Operation, case: &str| {
        let err = check_material_text(text, project, op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
            "{case}: {err:?}"
        );
    };

    let narrow_u = [(15.0, 8.0), (15.0, 22.0), (17.0, 22.0), (17.0, 8.0)];
    let (project, op) = with(vec![polyline(&narrow_u, false)], true);
    let refused = generate_profile(&project, &op);
    assert!(
        matches!(
            &refused,
            Err(dc_cam::CamError::BandNarrowerThanTool { shape, other, .. })
                if shape == &**other
        ),
        "{refused:?}"
    );
    fails_at_plunge(&cut(&narrow_u), &project, &Operation::Profile(op), "U");

    let wide_u = [(15.0, 8.0), (15.0, 22.0), (23.0, 22.0), (23.0, 8.0)];
    let (project, op) = with(vec![polyline(&wide_u, false)], true);
    check_material_text(&cut(&wide_u), &project, &Operation::Profile(op)).unwrap();

    let dumbbell = [
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 13.5),
        (30.0, 13.5),
        (30.0, 0.0),
        (50.0, 0.0),
        (50.0, 30.0),
        (30.0, 30.0),
        (30.0, 16.5),
        (20.0, 16.5),
        (20.0, 30.0),
        (0.0, 30.0),
    ];
    let (project, op) = with(vec![polyline(&dumbbell, true)], false);
    let op = Operation::Profile(op);
    fails_at_plunge(&cut(&[(20.0, 13.5), (30.0, 13.5)]), &project, &op, "neck");

    let notched = [
        (0.0, 0.0),
        (50.0, 0.0),
        (50.0, 30.0),
        (26.5, 30.0),
        (26.5, 15.0),
        (23.5, 15.0),
        (23.5, 30.0),
        (0.0, 30.0),
    ];
    let (project, op) = with(vec![polyline(&notched, true)], false);
    let along_notch = cut(&[(26.5, 30.0), (26.5, 15.0), (23.5, 15.0), (23.5, 30.0)]);
    check_material_text(&along_notch, &project, &Operation::Profile(op)).unwrap();

    let v = [
        (25.0 - 15.0 * 30_f64.to_radians().tan(), 7.0),
        (25.0, 22.0),
        (25.0 + 15.0 * 30_f64.to_radians().tan(), 7.0),
    ];
    let drawings = [
        (vec![polyline(&wide_u, false)], true),
        (vec![polyline(&v, false)], true),
        (vec![polyline(&notched, true)], false),
    ];
    for (profiles, in_rect) in drawings {
        let (project, mut op) = with(profiles, in_rect);
        op.entry = dc_project::Entry::Ramp { angle_deg: 15.0 };
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        let op = Operation::Profile(op);
        emit(
            &project,
            &[(op, project.tools[0].clone(), toolpath).into()],
            "2026-10-21",
        )
        .unwrap_or_else(|e| panic!("{project:?}: {e}"));
    }
}

/// S9 refuses what `generate_profile` refuses for a part the bit
/// removes whole. On line, a 3 mm square part (drawn at (20, 10), so X
/// 30 to 33, Y 20 to 23) is refused by the generator
/// (`PartNarrowerThanTool`), and a hand-written cut round it fails at
/// its plunge, although it is on the part's line. A 7 mm square passes,
/// hand-written and generated.
#[test]
fn on_line_part_smaller_than_the_bit_fails_s9() {
    let square = |size_mm: f64| {
        let mut project = sample_project();
        if let Some(def) = project.definitions.values_mut().next() {
            def.profiles = vec![dc_geometry::Polyline::rect(
                dc_geometry::Point::new(20.0, 10.0),
                size_mm,
                size_mm,
            )];
        }
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = dc_project::Side::OnLine;
        op.tabs = None;
        let op = op.clone();
        (project, op)
    };
    // Round the square from its lower-left corner, project space.
    let cut = |size_mm: f64| {
        let (x0, y0) = (30.0, 20.0);
        let (x1, y1) = (x0 + size_mm, y0 + size_mm);
        format!(
            "G21 G90\nG54\nG0 Z5\nX{x0} Y{y0}\nG1 Z-1 F300\nX{x1} F1000\nY{y1}\nX{x0}\nY{y0}\nG0 Z5\n"
        )
    };

    let (small, op) = square(3.0);
    let refused = generate_profile(&small, &op);
    assert!(
        matches!(refused, Err(dc_cam::CamError::PartNarrowerThanTool { .. })),
        "{refused:?}"
    );
    let op = Operation::Profile(op);
    let err = check_material_text(&cut(3.0), &small, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line: 5 } if at == op.id()),
        "the plunge on the 3 mm square: {err:?}"
    );

    let (large, op) = square(7.0);
    check_material_text(&cut(7.0), &large, &Operation::Profile(op.clone())).unwrap();
    // With arc fitting on, as the sample has it: the square's lap
    // stays four lines.
    assert!(large.arc_fit_tolerance_mm.is_some());
    let (toolpath, _) = generate_profile(&large, &op).unwrap();
    emit(
        &large,
        &[(Operation::Profile(op), large.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .unwrap();
}

/// The generated sample Profile with `edit` applied, emitted: tabs and
/// whatever entry, lead and relief `edit` sets.
fn emit_profile(edit: impl FnOnce(&mut dc_project::ProfileOp)) -> Result<String, ExportError> {
    let mut project = sample_project();
    edit(dc_project::test_support::profile_mut(&mut project));
    let op = project.operations[0].clone();
    let Operation::Profile(profile) = &op else {
        unreachable!("the sample's operation is a Profile");
    };
    let (toolpath, _) = generate_profile(&project, profile).unwrap();
    emit(
        &project,
        &[(op, project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .map(|(p, _)| p.text)
}

/// The sample's Profile starting under a 3 mm tall tab (its cut
/// starts nearest (35, 0), mid-way along the bottom edge, where a tab
/// at 0 mm sits) exports with every entry and both tab shapes: the
/// passes below the tab top come down onto it, not into it, which
/// S9's tab rule would refuse.
#[test]
fn start_under_a_tab_passes_s9() {
    for entry in [
        dc_project::Entry::Plunge,
        dc_project::Entry::Ramp { angle_deg: 15.0 },
        dc_project::Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        },
    ] {
        for shape in [dc_project::TabShape::Rect, dc_project::TabShape::Smooth] {
            emit_profile(|op| {
                op.entry = entry.clone();
                op.start = dc_project::StartAt::Near {
                    x_mm: 35.0,
                    y_mm: 0.0,
                };
                if let Some(tabs) = &mut op.tabs {
                    tabs.placement = dc_project::TabPlacement::AtDistances {
                        distances_mm: vec![0.0],
                    };
                    tabs.height_mm = 3.0;
                    tabs.shape = shape;
                }
            })
            .unwrap_or_else(|e| panic!("{entry:?} {shape:?}: {e}"));
        }
    }
}

/// S9 for Profile on generated programs: the sample's tabbed
/// outside Profile, with a ramp, a helix and a lead in and out, and
/// inside and on the line, all pass the safety pass.
#[test]
fn generated_profiles_pass_s9() {
    let ramp = dc_project::Entry::Ramp { angle_deg: 5.0 };
    let lead = dc_project::Lead {
        radius_mm: 4.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    };
    let helix = dc_project::Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    for side in [
        dc_project::Side::Outside,
        dc_project::Side::Inside,
        dc_project::Side::OnLine,
    ] {
        emit_profile(|op| op.side = side).unwrap();
        emit_profile(|op| {
            op.side = side;
            op.entry = ramp.clone();
        })
        .unwrap();
        if side != dc_project::Side::OnLine {
            emit_profile(|op| {
                op.side = side;
                op.entry = helix.clone();
                op.lead = Some(lead.clone());
                op.corner_relief = dc_project::CornerRelief::Dogbone;
            })
            .unwrap();
        }
    }
}

/// #284 (acceptance 46, S9 with a last pass): the sample made a 60 × 40
/// mm rectangle on 12 mm stock, cut 12 mm deep in 4 mm passes with a
/// 0.5 mm last pass stock to leave, passes every rule of the safety pass
/// (S1–S16), Inside and Outside; with a plunge, a ramp and lead, and a
/// helix, lead and dogbones; with its tab request and with those tabs
/// placed; with `thickness_mm: 1` and with `reverse`. Every program
/// cuts to the full depth (Z-12 below the stock top) and rises over
/// the 5 mm tabs (Z-7) on the last pass too.
#[test]
fn last_pass_program_passes_s9() {
    let lead = dc_project::Lead {
        radius_mm: 4.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    };
    let mut base = sample_project();
    base.stock.thickness_mm = Some(12.0);
    base.instances[0].z_top_mm = Some(12.0);
    if let Some(def) = base.definitions.values_mut().next() {
        def.profiles = vec![dc_geometry::Polyline::rect(
            Point::new(0.0, 0.0),
            60.0,
            40.0,
        )];
    }
    for side in [dc_project::Side::Outside, dc_project::Side::Inside] {
        for (entry, lead, relief) in [
            (
                dc_project::Entry::Plunge,
                None,
                dc_project::CornerRelief::None,
            ),
            (
                dc_project::Entry::Ramp { angle_deg: 10.0 },
                Some(lead.clone()),
                dc_project::CornerRelief::None,
            ),
            (
                dc_project::Entry::Helix {
                    diameter_fraction: 0.8,
                    angle_deg: 5.0,
                },
                Some(lead.clone()),
                dc_project::CornerRelief::Dogbone,
            ),
        ] {
            for last_pass in [
                dc_project::LastPass {
                    stock_to_leave_mm: 0.5,
                    thickness_mm: None,
                    reverse: false,
                },
                dc_project::LastPass {
                    stock_to_leave_mm: 0.5,
                    thickness_mm: Some(1.0),
                    reverse: true,
                },
            ] {
                for placed in [false, true] {
                    let mut project = base.clone();
                    let op = dc_project::test_support::profile_mut(&mut project);
                    op.side = side;
                    op.depth_mm = Some(12.0);
                    op.step_down_mm = Some(4.0);
                    op.entry = entry.clone();
                    op.lead = lead.clone();
                    op.corner_relief = relief;
                    op.last_pass = Some(last_pass.clone());
                    if let Some(tabs) = &mut op.tabs {
                        tabs.height_mm = 5.0;
                    }
                    if placed {
                        project = dc_cam::placed_tabs::place_requests(&project).unwrap().0;
                        assert_eq!(project.tabs.len(), 4);
                    }
                    let case = format!("{side:?} {entry:?} {lead:?} {last_pass:?} {placed}");
                    let op = project.operations[0].clone();
                    let Operation::Profile(profile) = &op else {
                        unreachable!("the sample's operation is a Profile");
                    };
                    let (toolpath, _) = generate_profile(&project, profile)
                        .unwrap_or_else(|e| panic!("{case}: {e}"));
                    dc_cam::test_support::assert_safe(&toolpath, &project, &op);
                    let (program, _) = emit(
                        &project,
                        &[(op, project.tools[0].clone(), toolpath).into()],
                        "2026-10-21",
                    )
                    .unwrap_or_else(|e| panic!("{case}: {e}"));
                    for z in ["Z-12.000", "Z-7.000"] {
                        assert!(program.text.contains(z), "{case}: no {z}");
                    }
                }
            }
        }
    }
}

/// #311 (S9 with a last pass on an open path): the sample with an open
/// line from (10, 60) to (70, 60) and an open L from (10, 80) to
/// (70, 80) to (70, 95), cut Left and Right, climb and conventional,
/// with a plunge and a ramp, its tab request and those tabs placed,
/// and a 0.5 mm last pass, also with `thickness_mm: 1` and `reverse`:
/// every program passes the safety pass (S1–S16). S9 holds the
/// stock passes to the open paths offset by `r + a`, and every
/// pass to the radius from every line and kept strip; a hand-written
/// pass half the stock to leave off the path fails it.
#[test]
fn left_right_last_pass_passes_s9() {
    let mut base = sample_project();
    if let Some(def) = base.definitions.values_mut().next() {
        def.profiles = vec![
            dc_geometry::Polyline::new(
                vec![
                    dc_geometry::Vertex::line(0.0, 50.0),
                    dc_geometry::Vertex::line(60.0, 50.0),
                ],
                false,
            ),
            dc_geometry::Polyline::new(
                vec![
                    dc_geometry::Vertex::line(0.0, 70.0),
                    dc_geometry::Vertex::line(60.0, 70.0),
                    dc_geometry::Vertex::line(60.0, 85.0),
                ],
                false,
            ),
        ];
    }
    let mut programs = 0;
    for side in [dc_project::Side::Left, dc_project::Side::Right] {
        for direction in [
            dc_project::CutDirection::Climb,
            dc_project::CutDirection::Conventional,
        ] {
            for entry in [
                dc_project::Entry::Plunge,
                dc_project::Entry::Ramp { angle_deg: 10.0 },
            ] {
                for last_pass in [
                    dc_project::LastPass {
                        stock_to_leave_mm: 0.5,
                        thickness_mm: None,
                        reverse: false,
                    },
                    dc_project::LastPass {
                        stock_to_leave_mm: 0.5,
                        thickness_mm: Some(1.0),
                        reverse: true,
                    },
                ] {
                    for placed in [false, true] {
                        let mut project = base.clone();
                        let op = dc_project::test_support::profile_mut(&mut project);
                        op.side = side;
                        op.direction = direction;
                        op.entry = entry.clone();
                        op.last_pass = Some(last_pass.clone());
                        if placed {
                            project = dc_cam::placed_tabs::place_requests(&project).unwrap().0;
                            assert!(!project.tabs.is_empty());
                        }
                        let case =
                            format!("{side:?} {direction:?} {entry:?} {last_pass:?} {placed}");
                        let op = project.operations[0].clone();
                        let Operation::Profile(profile) = &op else {
                            unreachable!("the sample's operation is a Profile");
                        };
                        let (toolpath, _) = generate_profile(&project, profile)
                            .unwrap_or_else(|e| panic!("{case}: {e}"));
                        dc_cam::test_support::assert_safe(&toolpath, &project, &op);
                        let (program, _) = emit(
                            &project,
                            &[(op, project.tools[0].clone(), toolpath).into()],
                            "2026-10-21",
                        )
                        .unwrap_or_else(|e| panic!("{case}: {e}"));
                        assert!(program.text.contains("Z-6.000"), "{case}: full depth");
                        programs += 1;
                    }
                }
            }
        }
    }
    assert_eq!(programs, 32);

    // By hand: Left of the line, at the stock path (Y 63.675) and
    // at half the stock to leave off it.
    let mut project = base.clone();
    let op = dc_project::test_support::profile_mut(&mut project);
    op.side = dc_project::Side::Left;
    op.last_pass = Some(dc_project::LastPass {
        stock_to_leave_mm: 0.5,
        thickness_mm: None,
        reverse: false,
    });
    let op = Operation::Profile(op.clone());
    let cut = |y: f64| format!("G21 G90\nG54\nG0 Z5\nX10 Y{y}\nG1 Z-1 F300\nX70 F1000\nG0 Z5\n");
    check_material_text(&cut(63.175), &project, &op).unwrap();
    check_material_text(&cut(63.675), &project, &op).unwrap();
    let err = check_material_text(&cut(63.425), &project, &op).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { line: 5, .. }),
        "half the stock to leave off: {err:?}"
    );
    // The last pass's step across the stock to leave, from the earlier
    // passes' path to its own, level, passes; the same step in two
    // halves, or falling as it crosses, fails at the move that starts
    // or falls inside the band.
    let step = |moves: &str| {
        format!("G21 G90\nG54\nG0 Z5\nX10 Y63.675\nG1 Z-1 F300\n{moves}X70 F1000\nG0 Z5\n")
    };
    check_material_text(&step("Y63.175 F1000\n"), &project, &op).unwrap();
    for (case, moves, line) in [
        ("in halves", "Y63.425 F1000\nY63.175\n", 6),
        ("falling", "G1 Z-0.5 F300\nY63.175 Z-1\n", 7),
    ] {
        let err = check_material_text(&step(moves), &project, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { line: l, .. } if *l == line),
            "{case}: {err:?}"
        );
    }
}

/// Programs cut round placed tabs pass the safety pass (S1–S7, S9, S10,
/// S12): the sample's tabs placed from its request, with a fifth,
/// taller and smooth, on the right-hand edge, on each side, with a
/// plunge and with a ramp, and Inside and Outside with a lead. Every
/// pass below a tab rises over it: the program holds each tab top.
#[test]
fn placed_tab_programs_pass_s1_to_s9() {
    let lead = dc_project::Lead {
        radius_mm: 4.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    };
    for side in [
        dc_project::Side::Outside,
        dc_project::Side::Inside,
        dc_project::Side::OnLine,
    ] {
        for entry in [
            dc_project::Entry::Plunge,
            dc_project::Entry::Ramp { angle_deg: 10.0 },
        ] {
            let mut project = sample_project();
            let op = dc_project::test_support::profile_mut(&mut project);
            op.side = side;
            op.entry = entry.clone();
            if side != dc_project::Side::OnLine {
                op.lead = Some(lead.clone());
            }
            let (mut project, _) = dc_cam::placed_tabs::place_requests(&project).unwrap();
            assert_eq!(project.tabs.len(), 4, "{side:?}");
            project.tabs.push(dc_project::PlacedTab {
                height_mm: 3.5,
                shape: dc_project::TabShape::Smooth,
                ..dc_project::test_support::sample_tab("tall", 50.0, 15.0)
            });
            let op = project.operations[0].clone();
            let Operation::Profile(profile) = &op else {
                unreachable!("the sample's operation is a Profile");
            };
            let (toolpath, _) = generate_profile(&project, profile).unwrap();
            dc_cam::test_support::assert_safe(&toolpath, &project, &op);
            let (program, _) = emit(
                &project,
                &[(op, project.tools[0].clone(), toolpath).into()],
                "2026-10-21",
            )
            .unwrap_or_else(|e| panic!("{side:?} {entry:?}: {e}"));
            // Machine Z zero is the stock top: tab tops at 2 and 3.5 mm
            // above the 6 mm stock's bottom are Z-4 and Z-2.5.
            for top in ["Z-4.000", "Z-2.500"] {
                assert!(program.text.contains(top), "{side:?} {entry:?}: no {top}");
            }
        }
    }
}

/// 20 parts, 15 × 15 mm, 10 mm apart in a 5 × 4 grid, cut outside
/// with ramps, leads, tabs and dogbones (#177's cost bound).
fn twenty_part_profile() -> (Project, Operation) {
    let mut project = sample_project();
    project.stock = dc_project::Stock {
        width_mm: 160.0,
        height_mm: 130.0,
        thickness_mm: Some(6.0),
    };
    let square = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d1"),
        name: "Square".into(),
        profiles: vec![dc_geometry::Polyline::rect(
            Point::new(0.0, 0.0),
            15.0,
            15.0,
        )],
    };
    project.definitions = std::collections::BTreeMap::from([(square.id.clone(), square)]);
    let template = project.instances[0].clone();
    project.instances = (0..20)
        .map(|k| {
            let (col, row) = (f64::from(k % 5), f64::from(k / 5));
            dc_project::FeatureInstance {
                id: dc_project::InstanceId::new(format!("i{k}")),
                name: format!("Square {k}"),
                transform: dc_geometry::Transform::translate(15.0 + 25.0 * col, 15.0 + 25.0 * row),
                ..template.clone()
            }
        })
        .collect();
    let targets = project.instances.iter().map(|i| i.id.clone()).collect();
    let op = dc_project::test_support::profile_mut(&mut project);
    op.targets = targets;
    op.entry = dc_project::Entry::Ramp { angle_deg: 5.0 };
    op.lead = Some(dc_project::Lead {
        radius_mm: 3.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    });
    op.corner_relief = dc_project::CornerRelief::Dogbone;
    let op = Operation::Profile(op.clone());
    (project, op)
}

/// S9 on a 20-part Profile with ramps, leads and tabs checks each move
/// against the outlines near it, so it stays fast in a debug build.
#[test]
fn s9_check_is_fast_on_twenty_profile_parts() {
    let (project, op) = twenty_part_profile();
    let Operation::Profile(profile) = &op else {
        unreachable!("a Profile");
    };
    let (toolpath, _) = generate_profile(&project, profile).unwrap();
    let text = emit(
        &project,
        &[(op.clone(), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .map(|(p, _)| p.text)
    .unwrap();
    let start = std::time::Instant::now();
    check_material_text(&text, &project, &op).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "S9 took {elapsed:?} on {} lines",
        text.lines().count()
    );
}

/// S9 measures every On line cut against its own line's stretches, but
/// only those near the cut, so an outline of 2000 chords cut On line in
/// three passes stays fast in a debug build.
#[test]
fn s9_check_is_fast_on_an_on_line_outline_of_many_chords() {
    let mut project = sample_project();
    let chords: Vec<dc_geometry::Vertex> = (0..2000)
        .map(|k| {
            let a = f64::from(k) * TAU / 2000.0;
            dc_geometry::Vertex::line(25.0 + 14.0 * a.cos(), 15.0 + 14.0 * a.sin())
        })
        .collect();
    if let Some(def) = project.definitions.values_mut().next() {
        def.profiles = vec![dc_geometry::Polyline::new(chords, true)];
    }
    let profile = dc_project::test_support::profile_mut(&mut project);
    profile.side = dc_project::Side::OnLine;
    profile.tabs = None;
    let profile = profile.clone();
    let (toolpath, _) = generate_profile(&project, &profile).unwrap();
    let op = Operation::Profile(profile);
    let text = emit(
        &project,
        &[(op.clone(), project.tools[0].clone(), toolpath).into()],
        "2026-10-21",
    )
    .map(|(p, _)| p.text)
    .unwrap();
    let start = std::time::Instant::now();
    check_material_text(&text, &project, &op).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "S9 took {elapsed:?} on {} lines",
        text.lines().count()
    );
}

/// S9 on the helix Pocket checks each of its more than eight hundred
/// descent chords as a segment against the region's outlines, not as
/// the union of swept capsules (6.7 s in a debug build in slice 9), so
/// it stays fast in a debug build. (changed in #214: more than a
/// thousand while the first pass's helices started at safe Z.)
#[test]
fn s9_check_is_fast_on_helix_pocket() {
    let (project, op) = island_pocket(helix(), 0.0);
    let text = emit_pocket(&project, &op, None).unwrap();
    let descents = descent_rates(&text).len();
    assert!(descents > 800, "{descents} descent chords");
    let op = Operation::Pocket(op);
    let start = std::time::Instant::now();
    check_material_text(&text, &project, &op).unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "S9 took {elapsed:?} on {descents} descent chords"
    );
}

/// The sample project with a Drill of one 8 mm hole at (50, 50).
fn drill_hole_project() -> (Project, dc_project::DrillOp) {
    let mut project = sample_project();
    let hole = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d2"),
        name: "Hole".into(),
        profiles: vec![dc_geometry::Polyline::circle(Point::new(0.0, 0.0), 4.0)],
    };
    project.definitions.insert(hole.id.clone(), hole);
    let mut instance = project.instances[0].clone();
    instance.id = dc_project::InstanceId::new("h1");
    instance.definition = dc_project::DefinitionId::new("d2");
    instance.transform = dc_geometry::Transform::translate(50.0, 50.0);
    project.instances.push(instance);
    let op = dc_project::DrillOp {
        targets: vec![dc_project::InstanceId::new("h1")],
        ..dc_project::test_support::sample_drill()
    };
    project.operations = vec![Operation::Drill(op.clone())];
    (project, op)
}

/// A simple hole at (`x`, 50): to the retract plane, plunge to Z 3,
/// out.
fn hole_at(x: f64) -> Toolpath {
    Toolpath {
        moves: vec![
            Move::rapid(Point3::new(x, 50.0, 11.0)),
            Move::rapid(Point3::new(x, 50.0, 7.0)),
            Move::linear(Point3::new(x, 50.0, 3.0), 300.0),
            Move::rapid(Point3::new(x, 50.0, 11.0)),
        ],
    }
}

/// S9 for a Drill: every cut stays inside one of its holes. A plunge
/// 5 mm from the 4 mm hole's centre is off it.
#[test]
fn drill_off_its_hole_fails_s9() {
    let (project, op) = drill_hole_project();
    let emit_drill = |tp: Toolpath| {
        emit(
            &project,
            &[(Operation::Drill(op.clone()), project.tools[0].clone(), tp).into()],
            "2026-10-21",
        )
        .map(|(p, _)| p.text)
    };
    let text = emit_drill(hole_at(50.0)).unwrap();
    let plunge_line = line_with(&text, "Z-3");
    let err = emit_drill(hole_at(55.0)).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line }
            if at == &op.id && *line == plunge_line),
        "{err:?}"
    );
}

/// The sample project with a Bore of one 10 mm hole at (50, 50), its
/// 6.35 mm tool's helix 1.825 mm round the centre (format 18).
fn bore_hole_project() -> (Project, dc_project::BoreOp) {
    let mut project = sample_project();
    let hole = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d2"),
        name: "Hole".into(),
        profiles: vec![dc_geometry::Polyline::circle(Point::new(0.0, 0.0), 5.0)],
    };
    project.definitions.insert(hole.id.clone(), hole);
    let mut instance = project.instances[0].clone();
    instance.id = dc_project::InstanceId::new("h1");
    instance.definition = dc_project::DefinitionId::new("d2");
    instance.transform = dc_geometry::Transform::translate(50.0, 50.0);
    project.instances.push(instance);
    let op = dc_project::BoreOp {
        targets: vec![dc_project::InstanceId::new("h1")],
        ..dc_project::test_support::sample_bore()
    };
    project.operations = vec![Operation::Bore(op.clone())];
    (project, op)
}

/// A hand-written bore of the hole at (50, 50): down at `radius_mm`
/// east of the centre, one circle there at Z 3 in four quarter arcs,
/// to the centre, out.
fn bore_circle_at(radius_mm: f64) -> Toolpath {
    let at = |angle: f64, z: f64| {
        Point3::new(
            50.0 + radius_mm * angle.cos(),
            50.0 + radius_mm * angle.sin(),
            z,
        )
    };
    let mut moves = vec![
        Move::rapid(at(0.0, 11.0)),
        Move::rapid(at(0.0, 7.0)),
        Move::linear(at(0.0, 3.0), 300.0),
    ];
    for quarter in 1..=4 {
        moves.push(Move {
            kind: MoveKind::ArcCcw {
                centre: Point::new(50.0, 50.0),
            },
            to: at(FRAC_PI_2 * f64::from(quarter), 3.0),
            feed_mm_min: Some(1000.0),
        });
    }
    moves.push(Move::linear(Point3::new(50.0, 50.0, 3.0), 1000.0));
    moves.push(Move::rapid(Point3::new(50.0, 50.0, 11.0)));
    Toolpath { moves }
}

/// S9 for a Bore (format 18): the generated helix and circle, within
/// the hole's radius less the tool's of its centre, pass, and so does
/// a hand-written circle on that radius.
#[test]
fn bore_inside_hole_passes_s9() {
    let (project, op) = bore_hole_project();
    let (toolpath, warnings) = dc_cam::generate_bore(&project, &op).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let operation = Operation::Bore(op);
    for tp in [toolpath, bore_circle_at(1.825)] {
        let text = emit(
            &project,
            &[(operation.clone(), project.tools[0].clone(), tp).into()],
            "2026-10-21",
        )
        .map(|(p, _)| p.text)
        .unwrap();
        check_material_text(&text, &project, &operation).unwrap();
    }
}

/// S9 for a Bore refuses a doctored program whose circle runs 0.1 mm
/// wider than the helix radius, at its first cut there: that would cut
/// the hole 0.2 mm oversize. With a 0.1 mm stock to leave even the helix
/// radius of the hole as drawn is too wide.
#[test]
fn bore_doctored_outside_hole_fails_s9() {
    let (mut project, mut op) = bore_hole_project();
    let emit_bore = |project: &Project, op: &dc_project::BoreOp, tp: Toolpath| {
        emit(
            project,
            &[(Operation::Bore(op.clone()), project.tools[0].clone(), tp).into()],
            "2026-10-21",
        )
        .map(|(p, _)| p.text)
    };
    let text = emit_bore(&project, &op, bore_circle_at(1.825)).unwrap();
    let plunge_line = line_with(&text, "Z-3");
    let err = emit_bore(&project, &op, bore_circle_at(1.925)).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line }
            if at == &op.id && *line == plunge_line),
        "{err:?}"
    );
    op.stock_to_leave_mm = 0.1;
    project.operations = vec![Operation::Bore(op.clone())];
    let err = emit_bore(&project, &op, bore_circle_at(1.825)).unwrap_err();
    assert!(matches!(&err, ExportError::OffMaterial { .. }), "{err:?}");
}

/// S9 for an Engrave: every cut stays on its lines. The sample
/// rectangle's bottom and right edges pass; an end 2 mm off the corner
/// fails, and so does a diagonal between two corners, whose ends are on
/// the line but whose middle is not.
#[test]
fn engrave_off_its_line_fails_s9() {
    let project = sample_project();
    let op = dc_project::test_support::sample_engrave();
    let path = |points: &[(f64, f64)]| {
        let mut moves = vec![
            Move::rapid(Point3::new(10.0, 10.0, 11.0)),
            Move::linear(Point3::new(10.0, 10.0, 5.0), 300.0),
        ];
        for &(x, y) in points {
            moves.push(Move::linear(Point3::new(x, y, 5.0), 1000.0));
        }
        let last = moves.last().unwrap().to;
        moves.push(Move::rapid(Point3::new(last.x, last.y, 11.0)));
        Toolpath { moves }
    };
    let emit_engrave = |tp: Toolpath| {
        emit(
            &project,
            &[(Operation::Engrave(op.clone()), project.tools[0].clone(), tp).into()],
            "2026-10-21",
        )
        .map(|(p, _)| p.text)
    };
    let text = emit_engrave(path(&[(60.0, 10.0), (60.0, 40.0)])).unwrap();
    let corner_line = line_with(&text, "X60");
    let err = emit_engrave(path(&[(60.0, 12.0), (60.0, 40.0)])).unwrap_err();
    assert!(
        matches!(&err, ExportError::OffMaterial { op: at, line }
            if at == &op.id && *line == corner_line),
        "{err:?}"
    );
    let err = emit_engrave(path(&[(60.0, 40.0)])).unwrap_err();
    assert!(
        matches!(err, ExportError::OffMaterial { line, .. } if line == corner_line),
        "{err:?}"
    );
}

/// The program for a [`chord_run`] over 270° written as four sub-arcs
/// of 67.5°, sub-arc `k`'s centre moved `shift[k]` mm along its
/// chord's perpendicular bisector (which keeps its start and end radii
/// equal), I and J rounded to three decimals from the written start, as
/// the emitter's I/J snapping moves a centre.
fn drifting_run_text(shift: [f64; 4]) -> String {
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let on = |a: f64| Point::new(round(50.0 + 10.0 * a.cos()), round(50.0 + 10.0 * a.sin()));
    let mut arcs = String::new();
    for (k, delta) in shift.into_iter().enumerate() {
        let a0 = 3.0 * PI / 8.0 * f64::from(u8::try_from(k).unwrap());
        let a1 = a0 + 3.0 * PI / 8.0;
        let (start, end) = (on(a0), on(a1));
        let mid = (a0 + a1) / 2.0;
        let centre = Point::new(50.0 + delta * mid.cos(), 50.0 + delta * mid.sin());
        let (i, j) = (round(centre.x - start.x), round(centre.y - start.y));
        let feed = if k == 0 { " F1000" } else { "" };
        arcs.push_str(&format!(
            "G3 X{:.3} Y{:.3} I{i:.3} J{j:.3}{feed}\n",
            end.x, end.y
        ));
    }
    chord_run_text(&arcs)
}

/// A long fitted run whose sub-arc centres drift by I/J snapping passes
/// S12. The first and last of four centres sit 0.009 mm out along their
/// bisectors and differ by 0.012 mm on each axis once rounded, more
/// than the arc-fit tolerance plus one unit in the last place (0.011
/// mm); each differs from its neighbour by under 0.008 mm. The run's
/// 102 chords put no vertex at the sub-arcs' ends, so the chain must
/// not break there. (changed in #226: 51 chords, which bow 0.011 mm
/// from the arc, past the tolerance.)
#[test]
fn fitted_run_with_drifting_snapped_centres_passes_s12() {
    let toolpath = chord_run(102, 3.0 * FRAC_PI_2, None);
    let text = drifting_run_text([0.009, 0.0, 0.0, 0.009]);
    check_against_with(&text, &toolpath, Some(0.01)).unwrap();
}

/// Found by the Pocket property test once #232, #234 and #235 were
/// fixed: an offset ring started 0.0017 mm before the end of a round
/// corner, leaving an arc piece shorter than a unit of a 4-place inch
/// program. `emit` writes such an arc as nothing (its ends round to one
/// point, and a G2 there would be a full circle), so S6 must take a
/// toolpath arc of at most a half turn that ends where the controller
/// already is as reproduced, as it takes such a straight move. Here a
/// 0.001 mm arc of radius 0.685 mm, in inches; the same arc in mm is
/// written and matched as usual.
#[test]
fn arc_shorter_than_a_quantum_is_written_as_nothing() {
    for units in [Units::Inch, Units::Mm] {
        let (mut project, op, tool) = partless_fixture();
        project.display_units = units;
        let centre = Point::new(20.0, 10.685);
        let turn: f64 = 0.001 / 0.685;
        let end = Point::new(20.0 + 0.685 * turn.sin(), 10.685 - 0.685 * turn.cos());
        let toolpath = Toolpath {
            moves: vec![
                Move::rapid(Point3::new(10.0, 10.0, 11.0)),
                Move::linear(Point3::new(10.0, 10.0, 3.0), 300.0),
                Move::linear(Point3::new(20.0, 10.0, 3.0), 1000.0),
                Move {
                    kind: MoveKind::ArcCcw { centre },
                    to: Point3::new(end.x, end.y, 3.0),
                    feed_mm_min: Some(1000.0),
                },
                Move::linear(Point3::new(30.0, 10.0, 3.0), 1000.0),
                Move::rapid(Point3::new(30.0, 10.0, 11.0)),
            ],
        };
        let result = emit(
            &project,
            &[(Operation::Profile(op), tool, toolpath).into()],
            "2026-10-21",
        );
        assert!(result.is_ok(), "{units:?}: {result:?}");
    }
}

/// #527 (found by the stress sweep): a V-bit floor ring's lap leaves
/// for a leftover piece a few hundredths across, cuts round it and
/// comes back, then goes on along the ring. Arc fitting fitted the
/// lap's return and the ring after it as two runs on circles about
/// 0.08 mm across whose centres lay within the allowance, which S12
/// read as one chain, and refused (`ArcFitViolation`). A circle under
/// six tolerances in radius is no longer fitted, and the program
/// passes.
#[test]
fn fitted_runs_on_two_small_near_circles_pass_s12() {
    let (project, op, tool) = partless_fixture();
    let points = [
        (47.359_478_040, 37.122_857_426),
        (47.390_692_026, 37.071_576_469),
        (47.433_445_736, 37.097_600_041),
        (47.443_435_102, 37.082_003_376),
        (47.449_253_439, 37.083_092_993),
        (47.460_127_740, 37.085_087_246),
        (47.446_078_150, 37.086_724_570),
        (47.433_445_736, 37.097_600_041),
        (47.390_692_026, 37.071_576_469),
        (47.404_748_104, 37.048_483_962),
        (47.417_076_491, 37.024_424_700),
        (47.437_610_822, 37.029_420_362),
        (47.458_397_438, 37.033_232_450),
    ];
    let at = |(x, y): (f64, f64), z: f64| Point3::new(x - 27.4, y - 27.0, z);
    let mut moves = vec![
        Move::rapid(at(points[0], 11.0)),
        Move::linear(at(points[0], 3.0), 300.0),
    ];
    moves.extend(
        points[1..]
            .iter()
            .map(|&p| Move::linear(at(p, 3.0), 1000.0)),
    );
    moves.push(Move::rapid(at(points[points.len() - 1], 11.0)));
    let mut project = project;
    project.arc_fit_tolerance_mm = Some(0.01);
    let result = emit(
        &project,
        &[(Operation::Profile(op), tool, Toolpath { moves }).into()],
        "2026-11-16",
    );
    assert!(result.is_ok(), "{result:?}");
}

/// A cut along Y 10 at Z 3 to X 20, then `steps` straight cuts at
/// those X, then one on to X 30, and retract.
fn sub_quantum_steps_at(steps: &[f64]) -> Toolpath {
    let mut moves = vec![
        Move::rapid(Point3::new(10.0, 10.0, 11.0)),
        Move::linear(Point3::new(10.0, 10.0, 3.0), 300.0),
        Move::linear(Point3::new(20.0, 10.0, 3.0), 1000.0),
    ];
    for &x in steps {
        moves.push(Move::linear(Point3::new(x, 10.0, 3.0), 1000.0));
    }
    moves.push(Move::linear(Point3::new(30.0, 10.0, 3.0), 1000.0));
    moves.push(Move::rapid(Point3::new(30.0, 10.0, 11.0)));
    Toolpath { moves }
}

/// The steps of #483 on a line: from X 20 to 20.0004 and back to
/// 19.9996, both written as nothing (they round to X20), then to
/// 20.0007, written `X20.001`. That line ends within one unit in the
/// last place of 20.0004 as well, so S6, taking the first move it
/// reaches, matched it to the step to 20.0004; from there 19.9996 is
/// 0.0014 mm off and no line follows for it.
const SUB_QUANTUM_STEPS: [f64; 3] = [20.0004, 19.9996, 20.0007];

/// S6 matches a line to the toolpath move it was written for, not
/// the first move it ends within a unit in the last place of. A
/// V-Carve's sub-quantum steps ([`SUB_QUANTUM_STEPS`]) are written
/// as `X20.000` (the steps that round to it written as nothing) and
/// `X20.001`; the program reproduces its toolpath, and exports.
#[test]
fn line_after_sub_quantum_steps_matches_the_move_it_was_written_for() {
    let (mut project, op, tool) = partless_fixture();
    project.arc_fit_tolerance_mm = None;
    let text = emit_one(
        &project,
        &op,
        &tool,
        sub_quantum_steps_at(&SUB_QUANTUM_STEPS),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        text.contains("\nX20.000 F1000.000\nX20.001\nX30.000\n"),
        "the steps are written as X20.001 alone:\n{text}"
    );
}

/// S6 still refuses a program that does not follow its toolpath,
/// however it matches lines to moves. With `X20.002` in place of
/// `X20.001`, the step to 20.0007 is 0.0013 mm off it and 0.0007 mm
/// from X20, so no line can stand for the cut on to X 30: a
/// mismatch at that move, as before.
#[test]
fn line_off_sub_quantum_steps_still_fails_s6() {
    let program = |line: &str| {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX10 Y10\nG1 Z-3 F300\nX20 F1000\n{line}X30\nG0 Z5\nM5\n"
        )
    };
    let toolpath = sub_quantum_steps_at(&SUB_QUANTUM_STEPS);
    check_against_with(&program("X20.001\n"), &toolpath, None).unwrap();
    let result = check_against_with(&program("X20.002\n"), &toolpath, None);
    assert!(is_mismatch_at(&result, 6), "{result:?}");
    let result = check_against_with(&program(""), &sub_quantum_steps_at(&[20.002]), None);
    assert!(is_mismatch_at(&result, 3), "{result:?}");
}

/// A fitted arc that ends at a toolpath vertex the arc's end is
/// within a unit in the last place of, where an earlier vertex is
/// too. [`chord_run`]'s quarter circle, its last vertex 0.0006 mm
/// short of (50, 60), then back along the circle to 0.002 mm short,
/// then at (50, 60), then a cut to (40, 60): one `G3` to (50, 60) for
/// all of them and a `G1` on. S6 ended the chain at the first vertex
/// near its end and found no line for the vertex 0.002 mm back; the
/// chain ends at the last.
#[test]
fn fitted_arc_ends_at_the_vertex_it_was_written_for() {
    let mut toolpath = chord_run(32, FRAC_PI_2, None);
    let retract = toolpath.moves.pop().unwrap();
    let on = |short_mm: f64| {
        let angle = FRAC_PI_2 - short_mm / 10.0;
        Point3::new(50.0 + 10.0 * angle.cos(), 50.0 + 10.0 * angle.sin(), 5.0)
    };
    let last = toolpath.moves.len() - 1;
    toolpath.moves[last].to = on(0.0006);
    for to in [
        on(0.002),
        Point3::new(50.0, 60.0, 5.0),
        Point3::new(40.0, 60.0, 5.0),
    ] {
        toolpath.moves.push(Move::linear(to, 1000.0));
    }
    toolpath
        .moves
        .push(Move::rapid(Point3::new(40.0, 60.0, retract.to.z)));
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\nG1 X40\n");
    check_against_with(&text, &toolpath, Some(0.01)).unwrap();
    // The same arc ending 0.003 mm short of its last vertex, at the
    // vertex 0.002 mm back, is not that vertex's arc: refused.
    let text = chord_run_text("G3 X50.003 Y60 I-10 J0 F1000\nG1 X40\n");
    let result = check_against_with(&text, &toolpath, Some(0.01));
    assert!(result.is_err(), "{result:?}");
}

/// Two abutting runs on different circles stay separate chains. A
/// quarter of the radius 10 circle about (50, 50) from +X to (50,
/// 60), then a quarter of the radius 15 circle about (50, 45) on to
/// (35, 45): written as one arc each, about its own centre, the
/// program passes; with the second arc written about the first
/// circle's centre, its vertices are off it and it fails. (changed in
/// #226: of eight chords each, which bow more than the tolerance.)
#[test]
fn chain_does_not_join_two_different_circles() {
    let mut toolpath = chord_run(32, FRAC_PI_2, None);
    let retract = toolpath.moves.pop().unwrap();
    for k in 1..=32 {
        let angle = FRAC_PI_2 + FRAC_PI_2 * f64::from(k) / 32.0;
        toolpath.moves.push(Move::linear(
            Point3::new(50.0 + 15.0 * angle.cos(), 45.0 + 15.0 * angle.sin(), 5.0),
            1000.0,
        ));
    }
    let last = toolpath.moves.last().unwrap().to;
    toolpath
        .moves
        .push(Move::rapid(Point3::new(last.x, last.y, retract.to.z)));
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\nX35 Y45 I0 J-15\n");
    check_against_with(&text, &toolpath, Some(0.01)).unwrap();
    let text = chord_run_text("G3 X50 Y60 I-10 J0 F1000\nX40 Y50 I0 J-10\n");
    let result = check_against_with(&text, &toolpath, Some(0.01));
    assert!(result.is_err(), "{result:?}");
}

// S16: machine-coordinate moves.

/// A program that cuts at the fixture's safe Z (work Z 5), then has
/// `tail` after its last retract.
fn lift_program(tail: &str) -> String {
    format!("G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n{tail}")
}

/// [`check_text_lifting`] on the fixture's GRBL definition, which
/// documents `G53` (grbl `gcode.c` 154-156, 635-640), with `lift`.
fn check_lift(text: &str, lift: Option<f64>) -> Result<(), ExportError> {
    let (project, _, _) = fixture();
    let post = project.post_processor.resolve().unwrap();
    assert!(post.work_coordinates.machine_command.is_some());
    check_text_lifting(text, post, SpindleControl::Automatic, lift)
}

/// S16: the one machine-coordinate move is `G53 G0 Z<v>`, alone on its
/// line. A `G53` with an X word, with `G1`, without `G0`, or with
/// another word is refused as G-code the check does not model; the
/// lift itself, before the end, is accepted.
#[test]
fn g53_only_z_alone() {
    check_lift(&lift_program("G53 G0 Z-5\nM5\n"), Some(-5.0)).unwrap();
    for line in [
        "G53 G0 X0",
        "G53 G1 Z-5",
        "G53 Z-5",
        "G53 G0 Z-5 X0",
        "G53 G0 Z-5 F100",
        "G53 G53 G0 Z-5",
        "G0 G53 G0 Z-5",
    ] {
        let err = check_lift(&lift_program(&format!("{line}\nM5\n")), Some(-5.0)).unwrap_err();
        assert!(
            matches!(&err, ExportError::Unparseable { line: 9, reason } if reason.contains("S16")),
            "{line}: {err:?}"
        );
    }
}

/// S16: work Z is unknown after the lift, as after a pause (S4), so an
/// XY rapid right after it is refused (S2), and so is any move before
/// the stop or the end that the lift is for. After a stop, a retract
/// in work coordinates makes the XY rapid safe again.
#[test]
fn xy_rapid_after_g53_needs_retract() {
    let err = check_lift(&lift_program("G53 G0 Z-5\nX10 Y10\nM5\n"), Some(-5.0)).unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::RapidBelowSafeZ {
                line: 10,
                z_mm: None,
                ..
            }
        ),
        "{err:?}"
    );
    let err = check_lift(&lift_program("G53 G0 Z-5\nG0 Z5\nM5\n"), Some(-5.0)).unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::MachineLiftNotBeforeStop {
                line: 10,
                lift_line: 9
            }
        ),
        "{err:?}"
    );
    let stopped = lift_program("G53 G0 Z-5\nM5\nM0\nG21 G90 G54\nG0 Z5\nX10 Y10\n");
    check_lift(&stopped, Some(-5.0)).unwrap();
    let unretracted = lift_program("G53 G0 Z-5\nM5\nM0\nG21 G90 G54\nX10 Y10\n");
    let err = check_lift(&unretracted, Some(-5.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowSafeZ { z_mm: None, .. }),
        "{err:?}"
    );
}

/// S16: the lift starts from a known Z at or above safe Z, and goes to
/// the project's tool-change machine Z, below 0; a stop right after it
/// is accepted though work Z is unknown there.
#[test]
fn g53_lifts_from_safe_z_to_the_project_height() {
    let low = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG53 G0 Z-5\nM5\n";
    let err = check_lift(low, Some(-5.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::MachineLiftNotAtSafeZ { line: 8 }),
        "{err:?}"
    );
    let err = check_lift(&lift_program("G53 G0 Z-4\nM5\n"), Some(-5.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::MachineLiftWrongZ { line: 9, .. }),
        "{err:?}"
    );
    let err = check_lift(&lift_program("G53 G0 Z0\nM5\n"), Some(0.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::MachineLiftWrongZ { line: 9, .. }),
        "{err:?}"
    );
    check_lift(&lift_program("G53 G0 Z-5\nM5\nM0\n"), Some(-5.0)).unwrap();
}

/// S16: a `G53` is refused, as before #283, in a program whose project
/// sets no tool-change machine Z, and on a definition that documents
/// no machine-coordinate move (Mach3, UCCNC, generic), whatever the
/// project sets.
#[test]
fn g53_refused_without_height() {
    let text = lift_program("G53 G0 Z-5\nM5\n");
    let err = check_lift(&text, None).unwrap_err();
    assert!(
        matches!(&err, ExportError::ForbiddenWord { line: 9, word } if word == "G53"),
        "{err:?}"
    );
    for id in ["mach3", "uccnc", "generic"] {
        for definition in dc_project::bundled_definitions()
            .unwrap()
            .iter()
            .filter(|d| d.id == id)
        {
            let post = definition.post_processor().clone();
            assert!(
                post.work_coordinates.machine_command.is_none(),
                "{}",
                definition.path
            );
            let err =
                check_text_lifting(&text, post, SpindleControl::Automatic, Some(-5.0)).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { word, .. } if word == "G53"),
                "{}: {err:?}",
                definition.path
            );
        }
    }
}

/// #306, S16 on LinuxCNC, with the strictness of grblHAL and GRBL:
/// both versions accept the lift to the project's machine Z, and
/// refuse it without the project's height, to another Z, at or above
/// machine Z 0, below safe Z, and in any other form. (The inch lift
/// is checked through `emit::machine_lift_on_linuxcnc`, whose programs
/// pass this pass.)
#[test]
fn g53_on_linuxcnc() {
    for definition in dc_project::bundled_definitions()
        .unwrap()
        .iter()
        .filter(|d| d.id == "linuxcnc")
    {
        let post = || definition.post_processor().clone();
        let check = |text: &str, lift: Option<f64>| {
            check_text_lifting(text, post(), SpindleControl::Automatic, lift)
        };
        let path = definition.path;
        check(&lift_program("G53 G0 Z-5\nM5\n"), Some(-5.0))
            .unwrap_or_else(|e| panic!("{path}: {e:?}"));
        let err = check(&lift_program("G53 G0 Z-5\nM5\n"), None).unwrap_err();
        assert!(
            matches!(&err, ExportError::ForbiddenWord { word, .. } if word == "G53"),
            "{path}: {err:?}"
        );
        for (tail, lift) in [("G53 G0 Z-4\nM5\n", -5.0), ("G53 G0 Z0\nM5\n", 0.0)] {
            let err = check(&lift_program(tail), Some(lift)).unwrap_err();
            assert!(
                matches!(err, ExportError::MachineLiftWrongZ { .. }),
                "{path} {tail}: {err:?}"
            );
        }
        let low = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\n\
                   G53 G0 Z-5\nM5\n";
        let err = check(low, Some(-5.0)).unwrap_err();
        assert!(
            matches!(err, ExportError::MachineLiftNotAtSafeZ { .. }),
            "{path}: {err:?}"
        );
        for line in ["G53 G0 X0", "G53 G1 Z-5", "G53 Z-5", "G53 G0 Z-5 X0"] {
            let err = check(&lift_program(&format!("{line}\nM5\n")), Some(-5.0)).unwrap_err();
            assert!(
                matches!(&err, ExportError::Unparseable { reason, .. } if reason.contains("S16")),
                "{path} {line}: {err:?}"
            );
        }
    }
}

/// #304 review, S16: with manual spindle control the user may jog at
/// the router-off pause after a lift, so the program lifts again before
/// `T<n> M6`. A second lift is accepted right after a stop that came
/// after a lift, with nothing moved between, once the units are set
/// again; a lift after a stop with no lift before it is still refused,
/// since Z is unknown there.
#[test]
fn g53_again_after_a_stop() {
    let again = lift_program("G53 G0 Z-5\nM5\nM0\nG21 G90 G54\nG53 G0 Z-5\nM0\n");
    check_lift(&again, Some(-5.0)).unwrap();
    let no_units = lift_program("G53 G0 Z-5\nM5\nM0\nG53 G0 Z-5\nM0\n");
    let err = check_lift(&no_units, Some(-5.0)).unwrap_err();
    assert!(
        matches!(
            err,
            ExportError::MissingSetupAfterPause {
                line: 12,
                pause_line: 11,
                ..
            }
        ),
        "{err:?}"
    );
    let unlifted = lift_program("M5\nM0\nG21 G90 G54\nG53 G0 Z-5\nM0\n");
    let err = check_lift(&unlifted, Some(-5.0)).unwrap_err();
    assert!(
        matches!(err, ExportError::MachineLiftNotAtSafeZ { line: 12 }),
        "{err:?}"
    );
}

/// [`check_rules`] on the fixture's GRBL definition with `heights`, the
/// operation owning lines `op_lines` and every other line outside it.
fn check_heights(
    text: &str,
    op_lines: Range<usize>,
    heights: ProgramHeights,
) -> Result<(), ExportError> {
    let (project, op, tool) = fixture();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let toolpath = Toolpath::default();
    let op = Operation::Profile(op);
    let spans = [OpSpan {
        op: &op,
        tool: &tool,
        toolpath: &toolpath,
        material: &NO_MATERIAL,
        tabs: &[],
        lines: op_lines,
    }];
    check_rules(
        text,
        &post,
        &transform,
        project.stock.thickness_mm.unwrap(),
        Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: project.arc_fit_tolerance_mm,
        },
        heights,
        &spans,
    )
    .map(|_| ())
}

/// #304 review, S2 and S6: with a clearance height (here work Z 20
/// over the fixture's 6 mm stock, absolute 26), an operation's first
/// move across the work, every move across outside the operations (the
/// park move), and every stop are at or above it; moves across inside
/// an operation after its first stay held to safe Z (work Z 5).
/// Without a clearance height the same low moves pass.
#[test]
fn rapids_held_to_the_clearance_height() {
    let heights = ProgramHeights {
        machine_lift_mm: None,
        clearance_z_mm: Some(26.0),
    };
    let program = |entry_z: &str, park_z: &str| {
        format!(
            "G21 G90\nG54\nG0 Z20\nM3 S18000\nG4 P3\nZ{entry_z}\nX0 Y0\nZ5\nG1 Z-1 F300\nG0 Z5\nX10 Y0\nG1 Z-1\nG0 Z20\nZ{park_z}\nX50 Y50\nM5\n"
        )
    };
    // The operation is lines 1 to 13; the park is lines 14 and 15.
    check_heights(&program("20", "20"), 1..14, heights).unwrap();
    let err = check_heights(&program("5", "20"), 1..14, heights).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowClearance { line: 7, .. }),
        "{err:?}"
    );
    let err = check_heights(&program("20", "10"), 1..14, heights).unwrap_err();
    assert!(
        matches!(err, ExportError::RapidBelowClearance { line: 15, .. }),
        "{err:?}"
    );
    check_heights(&program("5", "10"), 1..14, ProgramHeights::default()).unwrap();
    // A stop below the clearance height.
    let stop = "G21 G90\nG54\nG0 Z20\nM5\nZ5\nM0\n";
    let err = check_heights(stop, 1..7, heights).unwrap_err();
    assert!(
        matches!(err, ExportError::PauseNotAtSafeZ { line: 6 }),
        "{err:?}"
    );
    check_heights(stop, 1..7, ProgramHeights::default()).unwrap();
}

/// Review 1.1: a matching whose place is past the last motion line
/// has not taken every line: it has claimed lines the program does
/// not have. It is not finished: the pass stops with the
/// internal-defect error.
#[test]
fn matching_is_not_finished_past_the_end() {
    let (project, op, _) = fixture();
    let op = Operation::Profile(op);
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let matching = Matching {
        op: &op,
        moves: &[],
        motions: &[],
        dwells: &[],
        canned: false,
        reentry_gap_mm: None,
        transform: &transform,
        tolerances: Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: None,
        },
    };
    let place = |cursor: usize| Place {
        index: 0,
        cursor,
        next_dwell: 0,
        last_cut: None,
    };
    assert!(matches!(matching.finished(&place(0)), Ok(true)));
    let past = matching.finished(&place(1));
    assert!(
        matches!(past, Err(ExportError::SafetyCheckDefect { .. })),
        "{past:?}"
    );
}

/// A [`Matching`] of `moves` to no motion line.
fn matching_of<'m>(
    op: &'m Operation,
    moves: &'m [Move],
    transform: &'m MachineTransform,
) -> Matching<'m, 'static> {
    Matching {
        op,
        moves,
        motions: &[],
        dwells: &[],
        canned: false,
        reentry_gap_mm: None,
        transform,
        tolerances: Tolerances {
            flatten_mm: 0.01,
            arc_fit_mm: None,
        },
    }
}

/// A place past the last motion line, or past the last move, has
/// nothing true to show: no line "left", no move "before". The
/// view is the internal-defect error, where it was an empty rest.
#[test]
fn view_fails_closed_on_a_place_past_the_end() {
    let (project, op, _) = fixture();
    let op = Operation::Profile(op);
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let moves = [Move::rapid(Point3::new(0.0, 0.0, 11.0))];
    let matching = matching_of(&op, &moves, &transform);
    let place = |index: usize, cursor: usize| Place {
        index,
        cursor,
        next_dwell: 0,
        last_cut: None,
    };
    assert!(matching.view(&place(0, 0), &moves[0]).is_ok());
    assert!(matching.view(&place(1, 0), &moves[0]).is_ok());
    for (index, cursor) in [(0, 1), (2, 0)] {
        let view = matching.view(&place(index, cursor), &moves[0]);
        assert!(
            matches!(view, Err(ExportError::SafetyCheckDefect { .. })),
            "move {index}, line {cursor}"
        );
    }
}

/// A step that takes more motion lines than the span has left, or
/// more moves than the toolpath has left, leads nowhere. Past the
/// last move a place read as "every move matched", and past the last
/// line as "no line left": together, a pass.
#[test]
fn advance_fails_closed_on_a_step_past_the_end() {
    let (project, op, _) = fixture();
    let op = Operation::Profile(op);
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let moves = [Move::rapid(Point3::new(0.0, 0.0, 11.0))];
    let matching = matching_of(&op, &moves, &transform);
    let start = Place {
        index: 0,
        cursor: 0,
        next_dwell: 0,
        last_cut: None,
    };
    let end = matching.advance(&start, &moves[0], Step::one(0)).unwrap();
    assert_eq!((end.index, end.cursor), (1, 0));
    for step in [Step::one(1), Step::run((0, 2))] {
        let past = matching.advance(&start, &moves[0], step);
        assert!(
            matches!(past, Err(ExportError::SafetyCheckDefect { .. })),
            "{step:?}: {past:?}"
        );
    }
}

/// S10 reads a falling cut's vertical rate from its feed. With no
/// feed the rate was 0, under every plunge rate; it is now above
/// every one. S3 refuses such a cut first, and S10 no longer leans on
/// that.
#[test]
fn check_descent_rate_fails_closed_on_a_cut_without_feed() {
    let (project, op, tool) = fixture();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let at = |z: f64| MachinePosition {
        x: Some(10.0),
        y: Some(10.0),
        z: Some(z),
    };
    let plunge = |feed: Option<f64>| MachineMove {
        line: 7,
        kind: MotionKind::Linear,
        from: at(5.0),
        to: at(-1.0),
        feed,
        units: Units::Mm,
        arc: crate::parse::ArcWords::default(),
    };
    let op = Operation::Profile(op);
    check_descent_rate(&plunge(Some(300.0)), &op, &tool, &transform).unwrap();
    assert_eq!(
        descent_rate_mm_min(&plunge(None), &transform),
        Some(f64::INFINITY)
    );
    let err = check_descent_rate(&plunge(None), &op, &tool, &transform).unwrap_err();
    assert!(
        matches!(err, ExportError::EntryTooSteep { line: 7, .. }),
        "{err:?}"
    );
}

/// Review 1.10: S7 widens a cut by its operation's tool radius. A cut
/// on a line of no operation was checked as a point; it is refused,
/// here too and not only in the modal rules before it.
#[test]
fn check_on_stock_fails_closed_on_a_cut_outside_every_span() {
    let (project, _, _) = fixture();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let text = "G21 G90\nG54\nG0 Z5\nG0 X50 Y50\nG1 Z-1 F300\n";
    let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
    let err = check_on_stock(&blocks, &post, &transform, &project.stock, 6.0, &[]).unwrap_err();
    assert!(
        matches!(err, ExportError::CutOutsideOperation { line: 5 }),
        "{err:?}"
    );
}

mod arc_fit;
mod boards;
mod boundary;
mod inlay;
mod photo;
mod s17;
mod s18;
mod tapered;
mod tapered_inlay;
