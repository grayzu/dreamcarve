// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/postprocessor.ts

//! Tests of emission: the text of the programs [`emit`] and
//! [`emit_programs`] write, the exports they refuse and the warnings they
//! return.

use super::*;
use crate::parse::{MotionKind, parse_motion, parse_program};
use crate::safety::MAX_LINE_CHARS;
use dc_cam::{FeedField, Move, MoveKind, generate_profile};
use dc_geometry::{Point, Point3, assert_approx_eq};
use dc_project::test_support::{sample_project, tool_6_35_upcut};
use dc_project::{
    Operation, OperationId, OriginZ, PostProcessorRef, ProfileOp, SpindleControl, ToolChangeMode,
    ToolId, Units,
};

const DATE: &str = "2026-10-15";

fn profile_op(project: &Project) -> ProfileOp {
    let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
        panic!("sample project has a profile operation");
    };
    op
}

/// The Profile in an emit entry, to edit.
fn profile_of(op: &mut Operation) -> &mut ProfileOp {
    match op {
        Operation::Profile(op) => op,
        other => panic!("a Profile operation, not {other:?}"),
    }
}

/// The sample project with the bundled post-processor `post_id`.
fn project_with(post_id: &str) -> Project {
    let mut project = sample_project();
    project.post_processor = PostProcessorRef::latest(post_id).unwrap();
    project
}

/// The effective post-processor of `project`.
fn post_of(project: &Project) -> PostProcessor {
    project.post_processor.resolve().unwrap()
}

/// The sample project's operation, tool, and generated toolpath.
fn ops_for(project: &Project) -> Vec<EmitOp> {
    let op = profile_op(project);
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .unwrap()
        .clone();
    let (toolpath, _) = generate_profile(project, &op).unwrap();
    dc_cam::test_support::assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
    vec![(Operation::Profile(op), tool, toolpath).into()]
}

fn emit_text(project: &Project) -> String {
    emit(project, &ops_for(project), DATE).unwrap().0.text
}

/// [`emit_text`] with the definition `post`, which no bundled version
/// need have, in place of the project's.
fn emit_text_with(project: &Project, post: &PostProcessor) -> String {
    emit_with(project, post, &ops_for(project), DATE)
        .unwrap()
        .0
        .text
}

/// Code words of a line: comments removed, upper-cased, split on
/// whitespace.
fn code(line: &str) -> String {
    let line = line.split(';').next().unwrap_or("");
    let mut out = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_uppercase()
}

fn has_word(line: &str, word: &str) -> bool {
    code(line).split_whitespace().any(|w| w == word)
}

fn first_index(lines: &[&str], pred: impl Fn(&str) -> bool) -> Option<usize> {
    lines.iter().position(|l| pred(l))
}

fn is_cut(line: &str) -> bool {
    ["G1", "G2", "G3"].iter().any(|w| has_word(line, w))
}

/// Review 1.7: the rates are read once, after they are checked, and
/// carried as plain numbers, so nothing later stands a default in for
/// one. A rate that is unset, 0, negative or not finite gives no
/// `Rates` at all.
#[test]
fn rates_carry_the_validated_feed_and_plunge() {
    let tool = Tool {
        feed_mm_min: Some(900.0),
        plunge_mm_min: Some(250.0),
        rpm: Some(18_000),
        ..sample_project().tools[0].clone()
    };
    let rates = Rates::of(&tool, SpindleControl::Automatic).unwrap();
    assert_eq!(
        rates,
        Rates {
            feed_mm_min: 900.0,
            plunge_mm_min: 250.0,
            rpm: Some(18_000),
        }
    );
    for bad in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        for field in [FeedField::Feed, FeedField::Plunge] {
            let mut tool = tool.clone();
            match field {
                FeedField::Feed => tool.feed_mm_min = bad,
                FeedField::Plunge => tool.plunge_mm_min = bad,
            }
            let err = Rates::of(&tool, SpindleControl::Automatic).unwrap_err();
            assert!(
                matches!(&err, ExportError::MissingFeed { field: f, .. } if *f == field),
                "{bad:?} {field:?}: {err:?}"
            );
        }
    }
}

#[test]
fn missing_feed_is_error_naming_tool_and_field() {
    let mut project = sample_project();
    let mut ops = ops_for(&project);
    project.tools[0].feed_mm_min = None;
    ops[0].tool.feed_mm_min = None;
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(&err, ExportError::MissingFeed { tool, field: FeedField::Feed, .. } if tool == "6.35 mm upcut"),
        "{err:?}"
    );
    let message = err.to_string();
    assert!(
        message.contains("6.35 mm upcut") && message.contains("feed_mm_min"),
        "{message}"
    );

    // A placeholder 0 is as missing as `None`.
    ops[0].tool.feed_mm_min = Some(1000.0);
    ops[0].tool.plunge_mm_min = Some(0.0);
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(
            &err,
            ExportError::MissingFeed {
                field: FeedField::Plunge,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("plunge_mm_min"));
}

/// #64: safe Z is the thickness plus the operation's safe Z, so an
/// unset thickness fails emission, whichever Z origin is in use; it is
/// never defaulted.
#[test]
fn missing_thickness_is_error() {
    for z in [OriginZ::StockTop, OriginZ::StockBottom] {
        let mut project = sample_project();
        project.origin.z = z;
        let ops = ops_for(&project);
        project.stock.thickness_mm = None;
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(err, ExportError::MissingThickness),
            "{z:?}: {err:?}"
        );
    }
}

#[test]
fn missing_rpm_is_error() {
    let project = sample_project();
    let mut ops = ops_for(&project);
    for rpm in [None, Some(0)] {
        ops[0].tool.rpm = rpm;
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(&err, ExportError::MissingRpm { tool, .. } if tool == "6.35 mm upcut"),
            "{err:?}"
        );
    }
}

/// #71: two tools may share a name, so an error about a tool's rates
/// carries its id as well as the name it prints.
#[test]
fn tool_errors_carry_the_tool_id() {
    let project = sample_project();
    let t1 = ToolId::new("t1");
    let mut ops = ops_for(&project);
    ops[0].tool.feed_mm_min = None;
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(&err, ExportError::MissingFeed { tool_id, .. } if *tool_id == t1),
        "{err:?}"
    );
    let mut ops = ops_for(&project);
    ops[0].tool.rpm = None;
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(&err, ExportError::MissingRpm { tool_id, .. } if *tool_id == t1),
        "{err:?}"
    );
}

#[test]
fn no_enabled_operation_is_an_error() {
    let project = sample_project();
    let mut ops = ops_for(&project);
    profile_of(&mut ops[0].op).enabled = false;
    assert!(matches!(
        emit(&project, &ops, DATE),
        Err(ExportError::NoEnabledOperations)
    ));
    assert!(matches!(
        emit(&project, &[], DATE),
        Err(ExportError::NoEnabledOperations)
    ));
}

#[test]
fn units_command_first_then_wcs() {
    for post in ["grblhal", "grbl"] {
        let text = emit_text(&project_with(post));
        let lines: Vec<&str> = text.lines().collect();
        let units = first_index(&lines, |l| has_word(l, "G21")).unwrap();
        let wcs = first_index(&lines, |l| has_word(l, "G54")).unwrap();
        let first_action = first_index(&lines, |l| {
            ["G0", "G1", "G2", "G3", "M3"]
                .iter()
                .any(|w| has_word(l, w))
        })
        .unwrap();
        assert!(units < wcs, "{post}: units before WCS");
        assert!(
            wcs < first_action,
            "{post}: WCS before any motion or spindle start"
        );
        // Nothing but comments and modal setup comes before the units.
        for line in &lines[..units] {
            let c = code(line);
            assert!(
                c.is_empty() || ["G90", "G17", "G94", "G80", "G40", "G50"].contains(&c.as_str()),
                "{post}: {line}"
            );
        }
    }
}

#[test]
fn spindle_on_with_rpm_before_first_cut() {
    for post in ["grblhal", "grbl"] {
        let text = emit_text(&project_with(post));
        let lines: Vec<&str> = text.lines().collect();
        let spindle = first_index(&lines, |l| code(l) == "M3 S18000").unwrap();
        let cut = first_index(&lines, is_cut).unwrap();
        assert!(spindle < cut, "{post}");
        // The footer stops it after the last move.
        let last_move = lines.iter().rposition(|l| {
            ["G0", "G1", "G2", "G3", "X", "Y", "Z"]
                .iter()
                .any(|w| code(l).starts_with(w))
        });
        let stop = lines.iter().rposition(|l| has_word(l, "M5")).unwrap();
        assert!(stop > last_move.unwrap(), "{post}");
    }
}

#[test]
fn arcs_emitted_as_g2_g3_when_supported() {
    // The outside offset of the sample rectangle has rounded corners;
    // climb on an outside wall runs clockwise, so they are G2.
    let project = project_with("grblhal");
    assert!(post_of(&project).motion.arc_interpolation);
    let text = emit_text(&project);
    let arcs = text.lines().filter(|l| has_word(l, "G2")).count();
    let moves = parse_motion(&text).unwrap();
    let cw = moves.iter().filter(|m| m.kind == MotionKind::ArcCw).count();
    assert!(arcs > 0);
    // Four corners per lap, three laps (6 mm in 2 mm passes).
    assert_eq!(cw, 12, "{text}");
    assert!(moves.iter().all(|m| m.kind != MotionKind::ArcCcw));
    assert!(
        moves
            .iter()
            .filter(|m| m.kind == MotionKind::ArcCw)
            .all(|m| m.arc.i.is_some() && m.arc.j.is_some())
    );
}

/// The feed lines of `text` with the motion lines they come before:
/// for each cut, whether the line just before it is an `F` line on
/// its own. Fails if any motion line carries an `F` word.
fn separate_feeds(text: &str) -> (Vec<String>, Vec<bool>) {
    let lines: Vec<String> = text.lines().map(code).collect();
    let is_feed = |l: &String| l.starts_with('F');
    for l in lines.iter().filter(|l| !is_feed(l)) {
        assert!(
            !l.split_whitespace().any(|w| w.starts_with('F')),
            "F on a motion line: {l}"
        );
    }
    let cuts = parse_motion(text)
        .unwrap()
        .iter()
        .filter(|m| m.kind.is_cut())
        .map(|m| {
            m.line
                .checked_sub(2)
                .and_then(|i| lines.get(i))
                .is_some_and(is_feed)
        })
        .collect();
    (lines.into_iter().filter(is_feed).collect(), cuts)
}

/// A definition that writes the feed on a line of its own
/// (`inline_with_motion: false`) and keeps it modal writes an `F` line
/// only when the rate changes, never the same rate twice running
/// (#71).
#[test]
fn separate_feed_line_is_not_repeated_when_modal() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.feed_speed.inline_with_motion = false;
    let (feeds, before_cut) = separate_feeds(&emit_text_with(&project, &post));
    assert!(!feeds.is_empty());
    for pair in feeds.windows(2) {
        assert_ne!(pair[0], pair[1], "feed repeated: {feeds:?}");
    }
    assert!(feeds.len() < before_cut.len(), "{feeds:?}");
}

/// With `modal_feed_speed: false` the feed is written before every
/// feed move, on a line of its own when the definition says so (#71).
/// It used to be repeated only when written inline.
#[test]
fn separate_feed_line_is_repeated_when_not_modal() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.feed_speed.inline_with_motion = false;
    post.feed_speed.modal_feed_speed = false;
    let (feeds, before_cut) = separate_feeds(&emit_text_with(&project, &post));
    assert_eq!(feeds.len(), before_cut.len(), "{feeds:?}");
    for (k, preceded) in before_cut.iter().enumerate() {
        assert!(preceded, "cut {k} has no F line before it");
    }
}

#[test]
fn mirroring_axis_map_reverses_arc_direction() {
    // With machine X inverted, the clockwise corners of a climb
    // outside cut are counter-clockwise on the machine.
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.coordinate_system.x_axis = dc_project::Axis::NegX;
    let moves = parse_motion(&emit_text_with(&project, &post)).unwrap();
    assert_eq!(
        moves
            .iter()
            .filter(|m| m.kind == MotionKind::ArcCcw)
            .count(),
        12
    );
    assert!(moves.iter().all(|m| m.kind != MotionKind::ArcCw));
    assert!(moves.iter().filter_map(|m| m.to.x).all(|x| x <= 0.0));
}

#[test]
fn arcs_emitted_with_r_when_the_definition_says_so() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.motion.arc_format = dc_project::ArcFormat::R;
    let text = emit_text_with(&project, &post);
    let moves = parse_motion(&text).unwrap();
    let arcs: Vec<_> = moves
        .iter()
        .filter(|m| m.kind == MotionKind::ArcCw)
        .collect();
    assert!(!arcs.is_empty());
    for arc in arcs {
        let r = arc.arc.r.unwrap();
        assert!(r > 0.0, "R is positive: no arc over 180 degrees");
        assert!(arc.arc.i.is_none() && arc.arc.j.is_none());
    }
}

#[test]
fn arcs_flattened_when_not() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.motion.arc_interpolation = false;
    let text = emit_text_with(&project, &post);
    assert!(
        !text.lines().any(|l| has_word(l, "G2") || has_word(l, "G3")),
        "{text}"
    );
    let moves = parse_motion(&text).unwrap();
    assert!(
        moves
            .iter()
            .all(|m| matches!(m.kind, MotionKind::Rapid | MotionKind::Linear))
    );

    // The rectangle spans [10, 60] x [10, 40]. A point beside it in both
    // X and Y lies on a flattened corner, which stays on the 3.175 mm
    // tool-centre arc about that corner, to within rounding.
    let mut on_corner_arcs = 0;
    for m in &moves {
        let (Some(x), Some(y)) = (m.to.x, m.to.y) else {
            continue;
        };
        let beside_x = !(10.0 - 1e-3..=60.0 + 1e-3).contains(&x);
        let beside_y = !(10.0 - 1e-3..=40.0 + 1e-3).contains(&y);
        if beside_x && beside_y {
            let r = (x - x.clamp(10.0, 60.0)).hypot(y - y.clamp(10.0, 40.0));
            assert_approx_eq!(r, 3.175, 1e-3);
            on_corner_arcs += 1;
        }
    }
    assert!(
        on_corner_arcs > 12,
        "flattening adds points along each corner"
    );
}

#[test]
fn inch_project_scales_and_emits_g20() {
    let mut project = project_with("grblhal");
    project.display_units = Units::Inch;
    let ops = ops_for(&project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.iter().any(|l| has_word(l, "G20")));
    assert!(!lines.iter().any(|l| has_word(l, "G21")));
    // Every coordinate is the project's millimetres / 25.4, origin
    // bottom-left, Z zero at stock top (6 mm).
    let moves = parse_motion(&text).unwrap();
    let max_x = moves.iter().filter_map(|m| m.to.x).fold(f64::MIN, f64::max);
    let max_x_mm = ops[0]
        .toolpath
        .moves
        .iter()
        .map(|m| m.to.x)
        .fold(f64::MIN, f64::max);
    assert_approx_eq!(max_x, max_x_mm / 25.4, 1e-4);
    let min_z = moves.iter().filter_map(|m| m.to.z).fold(f64::MAX, f64::min);
    assert_approx_eq!(min_z, -6.0 / 25.4, 1e-4);
    // Four decimals in inches, and feeds in inches per minute.
    assert!(text.contains(&format!("F{:.4}", 1000.0 / 25.4)), "{text}");
    assert!(text.contains(&format!("F{:.4}", 300.0 / 25.4)), "{text}");
    // Internal geometry is untouched: the toolpath is still mm.
    assert_approx_eq!(max_x_mm, 60.0 + 3.175, 1e-6);
}

/// Ramp angles, degrees, of [`ramp_sweep`].
const SWEEP_ANGLES_DEG: [f64; 19] = [
    1.0, 2.0, 3.5, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 30.0, 37.0, 45.0, 52.0, 60.0, 67.0, 75.0,
    80.0, 85.0, 89.0,
];
/// Segment runs, mm, of [`ramp_sweep`]: a helix's chords and a tab's
/// short slopes down to a long zigzag leg.
const SWEEP_RUNS_MM: [f64; 6] = [0.05, 0.13, 0.37, 1.1, 2.9, 7.0];

/// One ramp per angle and segment run, each in the waste right of the
/// sample part, from a start off the written grid: a rapid over it at
/// safe Z, a straight plunge to the stock top at `plunge_mm_min`, then
/// segments of that run falling at that angle, at most 1.5 mm in all
/// and 12 mm across, each at the feed `dc_cam::entry` derives for it
/// (`plunge_limited_feed_mm_min`: `min(feed, plunge × length / drop)`
/// from the unrounded ends), and a rapid out.
fn ramp_sweep(feed_mm_min: f64, plunge_mm_min: f64) -> Toolpath {
    let mut moves = Vec::new();
    let mut k = 0.0;
    for angle_deg in SWEEP_ANGLES_DEG {
        for run_mm in SWEEP_RUNS_MM {
            let heading = 0.2 * ((k % 5.0) - 2.0);
            let (dx, dy) = (run_mm * f64::cos(heading), run_mm * f64::sin(heading));
            let mut at = Point3::new(68.000_37 + 0.013 * k, 9.000_29 + 0.68 * k, 6.0);
            moves.push(Move::rapid(Point3::new(at.x, at.y, 11.0)));
            moves.push(Move::linear(at, plunge_mm_min));
            let (mut across_mm, floor_z) = (0.0, at.z - 1.5);
            while at.z > floor_z && across_mm + run_mm <= 12.0 {
                let drop_mm = (run_mm * angle_deg.to_radians().tan()).min(at.z - floor_z);
                let to = Point3::new(at.x + dx, at.y + dy, at.z - drop_mm);
                let length_mm = (run_mm * run_mm + drop_mm * drop_mm).sqrt();
                let feed = feed_mm_min.min(plunge_mm_min * length_mm / drop_mm);
                moves.push(Move::linear(to, feed));
                at = to;
                across_mm += run_mm;
            }
            moves.push(Move::rapid(Point3::new(at.x, at.y, 11.0)));
            k += 1.0;
        }
    }
    Toolpath { moves }
}

/// #325: every falling cut the program makes, read from its text
/// alone, falls no faster than the plunge rate: the ramp sweep's
/// derived feeds and its straight plunges, in mm and in inches, on
/// every bundled post-processor's number format, for a whole plunge
/// rate and one the written places cannot hold.
#[test]
fn derived_ramp_feed_never_exceeds_plunge() {
    let mut checked = 0;
    for definition in dc_project::latest_definitions().unwrap() {
        for units in [Units::Mm, Units::Inch] {
            for plunge_mm_min in [300.0, 437.123_456] {
                let mut project = project_with(definition.id);
                project.display_units = units;
                project.tools[0].plunge_mm_min = Some(plunge_mm_min);
                let mut ops = ops_for(&project);
                ops[0].tool.plunge_mm_min = Some(plunge_mm_min);
                ops[0].toolpath = ramp_sweep(1000.0, plunge_mm_min);
                let text = emit(&project, &ops, DATE)
                    .unwrap_or_else(|e| panic!("{} {units:?} {plunge_mm_min}: {e}", definition.id))
                    .0
                    .text;
                let mm_per_unit = match units {
                    Units::Mm => 1.0,
                    Units::Inch => 25.4,
                };
                for m in parse_motion(&text).unwrap() {
                    if m.kind != MotionKind::Linear {
                        continue;
                    }
                    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
                        (m.from.x, m.from.y, m.from.z, m.to.x, m.to.y, m.to.z)
                    else {
                        continue;
                    };
                    let (run, drop) = ((tx - fx).hypot(ty - fy), fz - tz);
                    if drop <= 0.0 {
                        continue;
                    }
                    let rate = m.feed.unwrap() * mm_per_unit * drop / run.hypot(drop);
                    assert!(
                        rate <= plunge_mm_min,
                        "{} {units:?}: line {} falls at {rate} mm/min, past {plunge_mm_min}",
                        definition.id,
                        m.line
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 10_000, "{checked} falling cuts");
}

/// #325 on arcs: for each of `angles_deg`, a quarter turn about
/// (80.000 3, 50.000 7), radius 2.3, falling at that angle from the
/// stock top, after a straight plunge at `plunge_mm_min`, at the feed
/// that holds the arc to the plunge rate, then a rapid out. Each starts
/// a quarter turn on from the last.
fn helix_arcs(angles_deg: &[f64], plunge_mm_min: f64) -> Toolpath {
    let centre = Point::new(80.000_3, 50.000_7);
    let radius_mm = 2.3;
    let at = |turn: f64, z: f64| {
        let angle = turn * FRAC_PI_2;
        Point3::new(
            centre.x + radius_mm * angle.cos(),
            centre.y + radius_mm * angle.sin(),
            z,
        )
    };
    let run_mm = radius_mm * FRAC_PI_2;
    let mut moves = Vec::new();
    for (k, angle_deg) in (0..).zip(angles_deg) {
        let turn = f64::from(k);
        let drop_mm = run_mm * angle_deg.to_radians().tan();
        let end = at(turn + 1.0, 6.0 - drop_mm);
        moves.push(Move::rapid(at(turn, 11.0)));
        moves.push(Move::linear(at(turn, 6.0), plunge_mm_min));
        moves.push(Move {
            kind: MoveKind::ArcCcw { centre },
            to: end,
            feed_mm_min: Some(1000.0_f64.min(plunge_mm_min * run_mm.hypot(drop_mm) / drop_mm)),
        });
        moves.push(Move::rapid(Point3::new(end.x, end.y, 11.0)));
    }
    Toolpath { moves }
}

/// #325: a falling arc held to the plunge rate exports on every bundled
/// post-processor, in mm and inches, whether written with I and J,
/// with R, or flattened, so S10, which allows nothing above the plunge
/// rate, passes each arc or chord as written; and where it is
/// flattened, every chord falls no faster, read from the text.
#[test]
fn derived_helix_arc_feed_never_exceeds_plunge() {
    let angles_deg = [2.0, 6.5, 11.0, 17.457_603_123_722_1, 19.0, 27.0, 33.0, 41.0];
    for definition in dc_project::latest_definitions().unwrap() {
        for units in [Units::Mm, Units::Inch] {
            for flatten in [false, true] {
                let mut project = project_with(definition.id);
                project.display_units = units;
                let mut post = post_of(&project);
                post.motion.arc_interpolation &= !flatten;
                let mut ops = ops_for(&project);
                ops[0].toolpath = helix_arcs(&angles_deg, 300.0);
                let text = emit_with(&project, &post, &ops, DATE)
                    .unwrap_or_else(|e| panic!("{} {units:?}: {e}", definition.id))
                    .0
                    .text;
                let moves = parse_motion(&text).unwrap();
                let arcs = moves
                    .iter()
                    .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
                    .count();
                assert_eq!(arcs == 0, flatten || !post.motion.arc_interpolation);
                let mm_per_unit = match units {
                    Units::Mm => 1.0,
                    Units::Inch => 25.4,
                };
                for m in moves.iter().filter(|m| m.kind == MotionKind::Linear) {
                    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
                        (m.from.x, m.from.y, m.from.z, m.to.x, m.to.y, m.to.z)
                    else {
                        continue;
                    };
                    let (run, drop) = ((tx - fx).hypot(ty - fy), fz - tz);
                    if drop > 0.0 {
                        let rate = m.feed.unwrap() * mm_per_unit * drop / run.hypot(drop);
                        assert!(rate <= 300.0, "line {} falls at {rate}", m.line);
                    }
                }
            }
        }
    }
}

/// Falls, mm, of [`sub_quantum_steps`]' first steps: every one less
/// than a unit in the last place of a millimetre program (0.001 mm)
/// or an inch one (0.0001 in, 0.00254 mm).
const SUB_QUANTUM_DROPS_MM: [f64; 4] = [0.0002, 0.0004, 0.0006, 0.0008];
/// Runs, mm, of [`sub_quantum_steps`]' first steps.
const SUB_QUANTUM_RUNS_MM: [f64; 3] = [0.0001, 0.0003, 0.0007];

/// #467: per fall and run, in the waste right of the sample part, a
/// V-Carve chain's start as `dc_cam`'s medial generator cuts it: a
/// rapid over it at safe Z, a straight plunge to 0.1 µm under the
/// stock top at `plunge_mm_min`, a step falling that little over that
/// run, then one falling 0.3 mm over 0.5 mm, each at the feed scaled to
/// the plunge rate (`min(feed, plunge × length / drop)`, from the
/// unrounded ends), a level cut at the feed, and a rapid out.
fn sub_quantum_steps(feed_mm_min: f64, plunge_mm_min: f64) -> Toolpath {
    let scaled = |from: Point3, to: Point3| {
        let (run_mm, drop_mm) = (from.xy().distance_to(to.xy()), from.z - to.z);
        feed_mm_min.min(plunge_mm_min * run_mm.hypot(drop_mm) / drop_mm)
    };
    let mut moves = Vec::new();
    let mut k = 0.0_f64;
    for drop_mm in SUB_QUANTUM_DROPS_MM {
        for run_mm in SUB_QUANTUM_RUNS_MM {
            let heading = 0.7 * k;
            let (cos, sin) = (heading.cos(), heading.sin());
            let start = Point3::new(68.000_37 + 0.4 * k, 9.000_29 + 1.1 * k, 6.0 - 0.000_1);
            let step = Point3::new(
                start.x + run_mm * cos,
                start.y + run_mm * sin,
                start.z - drop_mm,
            );
            let down = Point3::new(step.x + 0.5 * cos, step.y + 0.5 * sin, step.z - 0.3);
            let level = Point3::new(down.x + 2.0, down.y, down.z);
            moves.push(Move::rapid(Point3::new(start.x, start.y, 11.0)));
            moves.push(Move::linear(start, plunge_mm_min));
            moves.push(Move::linear(step, scaled(start, step)));
            moves.push(Move::linear(down, scaled(step, down)));
            moves.push(Move::linear(level, feed_mm_min));
            moves.push(Move::rapid(Point3::new(level.x, level.y, 11.0)));
            k += 1.0;
        }
    }
    Toolpath { moves }
}

/// #467: a cut that falls less than a unit in the last written place
/// at a feed scaled to the plunge rate exports on every bundled
/// post-processor, in mm and inches. Written level, it carries one of
/// its tool's rates (S3), and every falling cut, read from the text
/// alone, falls no faster than the plunge rate (S10).
#[test]
fn sub_quantum_descent_is_written_at_a_tool_rate() {
    let (feed_mm_min, plunge_mm_min) = (1000.0, 300.0);
    let mut written_level = 0;
    for definition in dc_project::latest_definitions().unwrap() {
        for units in [Units::Mm, Units::Inch] {
            let mut project = project_with(definition.id);
            project.display_units = units;
            let mut ops = ops_for(&project);
            ops[0].tool.feed_mm_min = Some(feed_mm_min);
            ops[0].tool.plunge_mm_min = Some(plunge_mm_min);
            ops[0].toolpath = sub_quantum_steps(feed_mm_min, plunge_mm_min);
            let text = emit(&project, &ops, DATE)
                .unwrap_or_else(|e| panic!("{} {units:?}: {e}", definition.id))
                .0
                .text;
            let mm_per_unit = match units {
                Units::Mm => 1.0,
                Units::Inch => 25.4,
            };
            for m in parse_motion(&text).unwrap() {
                if m.kind != MotionKind::Linear {
                    continue;
                }
                let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
                    (m.from.x, m.from.y, m.from.z, m.to.x, m.to.y, m.to.z)
                else {
                    continue;
                };
                let feed = m.feed.unwrap() * mm_per_unit;
                let (run, drop) = ((tx - fx).hypot(ty - fy) * mm_per_unit, fz - tz);
                if drop > 0.0 {
                    let rate = feed * (drop * mm_per_unit) / run.hypot(drop * mm_per_unit);
                    assert!(
                        rate <= plunge_mm_min,
                        "{} {units:?}: line {} falls at {rate} mm/min",
                        definition.id,
                        m.line
                    );
                } else if run > 0.0 {
                    assert!(
                        [feed_mm_min, plunge_mm_min]
                            .iter()
                            .any(|rate| (rate - feed).abs() <= 0.01),
                        "{} {units:?}: level line {} at {feed} mm/min",
                        definition.id,
                        m.line
                    );
                    if run < 0.01 {
                        written_level += 1;
                    }
                }
            }
        }
    }
    assert!(written_level > 0, "no step was written level");
}

/// UCCNC has no `G20`/`G21` (#164): an inch project's program has no
/// units word, is in inches, warns which, and passes the safety pass,
/// which reads it in inches. Read as millimetres, its safe Z of
/// 0.1969 in would be 0.2 mm, and the safety pass would reject it.
#[test]
fn uccnc_inch_program_has_no_units_word_and_is_read_in_inches() {
    let mut project = project_with("uccnc");
    project.display_units = Units::Inch;
    let (program, warnings) = emit(&project, &ops_for(&project), DATE).unwrap();
    let text = program.text;
    assert!(
        !text
            .lines()
            .any(|l| has_word(l, "G20") || has_word(l, "G21")),
        "{text}"
    );
    assert!(
        warnings.contains(&ExportWarning::UnitsSetByController {
            post: "uccnc".into(),
            units: Units::Inch,
        }),
        "{warnings:?}"
    );
    assert!(text.contains(&format!("Z{:.4}", -6.0 / 25.4)), "{text}");
}

/// The characters the controllers act on, and control characters, are
/// removed from every name. The non-ASCII character is one with no
/// transliteration (U+2603), so it is dropped; before #278 this was an
/// em dash, which is now written `-` (`comment_text_transliterates`).
#[test]
fn comment_text_is_sanitized() {
    let nasty = "Bad!~?(name);\nnext\u{2603}line\u{18}";
    for post in ["grblhal", "grbl"] {
        let mut project = project_with(post);
        project.name = nasty.into();
        project.tools[0].name = nasty.into();
        let mut ops = ops_for(&project);
        profile_of(&mut ops[0].op).name = nasty.into();
        ops[0].tool.name = nasty.into();
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        for line in text.lines() {
            assert!(
                line.chars().all(|c| (' '..='~').contains(&c)),
                "{post}: non-printable in {line:?}"
            );
            assert!(!line.contains(['!', '~', '?']), "{post}: {line:?}");
        }
        let definition = post_of(&project);
        let prefix = &definition.program.comment_prefix;
        let suffix = &definition.program.comment_suffix;
        let named: Vec<&str> = text.lines().filter(|l| l.contains("Bad")).collect();
        assert!(
            named.len() >= 3,
            "{post}: program, operation, and tool names appear"
        );
        for line in named {
            // The text inside the comment has no comment delimiters.
            // Pauses are always `M0 (MSG, ...)`, whatever the post's
            // comment style (#43).
            let (prefix, suffix) = if has_word(line, "M0") {
                ("(MSG,", ")")
            } else {
                (prefix.as_str(), suffix.as_str())
            };
            let start = line.find(prefix).unwrap() + prefix.len();
            let inner = if suffix.is_empty() {
                &line[start..]
            } else {
                &line[start..line.rfind(suffix).unwrap()]
            };
            assert!(!inner.contains(['(', ')', ';']), "{post}: {line:?}");
            assert!(inner.contains("Bad name next line"), "{post}: {line:?}");
        }
    }
}

/// Acceptance 40 (#278): a name with accents and symbols keeps its
/// letters, transliterated before the sanitizer runs, in each comment
/// style; the program stays printable ASCII. Tool and operation names
/// go through the same rule.
#[test]
fn comment_text_transliterates() {
    for (post, header) in [
        ("grblhal", "( Project: Cafe O6 1/2 )"),
        ("grbl", "; Project: Cafe O6 1/2"),
    ] {
        let mut project = project_with(post);
        project.name = "Café Ø6 ½".into();
        let mut ops = ops_for(&project);
        profile_of(&mut ops[0].op).name = "Découpe — Straße".into();
        ops[0].tool.name = "Ø6 mm ¼\u{2033}".into();
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        assert_eq!(text.lines().next(), Some(header), "{post}:\n{text}");
        assert!(text.contains("Decoupe - Strasse"), "{post}:\n{text}");
        assert!(text.contains("O6 mm 1/4"), "{post}:\n{text}");
        assert!(
            text.bytes()
                .all(|b| b == b'\n' || (b' '..=b'~').contains(&b)),
            "{post}: a byte that is not printable ASCII:\n{text}"
        );
    }
}

/// Acceptance 40: a name made only of characters with no
/// transliteration (CJK here) writes an empty name, as before #278,
/// and every byte is printable ASCII.
#[test]
fn comment_text_drops_unmapped() {
    for (post, header) in [("grblhal", "( Project:  )"), ("grbl", "; Project:")] {
        let mut project = project_with(post);
        project.name = "木工".into();
        let text = emit(&project, &ops_for(&project), DATE).unwrap().0.text;
        assert_eq!(text.lines().next(), Some(header), "{post}:\n{text}");
        assert!(
            text.bytes()
                .all(|b| b == b'\n' || (b' '..=b'~').contains(&b)),
            "{post}: a byte that is not printable ASCII:\n{text}"
        );
    }
}

#[test]
fn comment_text_is_truncated() {
    for post in ["grblhal", "grbl"] {
        let mut project = project_with(post);
        project.name = "P".repeat(1000);
        let mut ops = ops_for(&project);
        profile_of(&mut ops[0].op).name = "O".repeat(1000);
        ops[0].tool.name = "T".repeat(1000);
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        for line in text.lines() {
            assert!(line.len() <= MAX_LINE_CHARS, "{post}: {} chars", line.len());
        }
        assert!(text.contains("PPPP") && text.contains("OOOO") && text.contains("TTTT"));
    }
}

#[test]
fn never_emits_m4_or_m7() {
    // grblHAL's definition has M4 and M7; neither is used.
    let project = project_with("grblhal");
    assert_eq!(post_of(&project).feed_speed.spindle_on_ccw, "M4");
    let text = emit_text(&project);
    for line in text.lines() {
        for word in ["M4", "M7", "M6", "G53"] {
            assert!(!has_word(line, word), "{line}");
        }
    }
    assert!(text.lines().any(|l| has_word(l, "M3")));
}

#[test]
fn numbers_never_use_exponent_notation() {
    let mut project = project_with("grblhal");
    let mut ops = ops_for(&project);
    // A tiny coordinate and a huge-but-safe one, on stock wide enough
    // for it, cut by a point so the cut at X ≈ 0 stays on it (S7).
    project.stock.width_mm = 20_000_000.0;
    ops[0].tool.diameter_mm = 0.0;
    ops[0].toolpath = Toolpath {
        moves: vec![
            Move::rapid(Point3::new(1e-9, 1e-12, 11.0)),
            Move::linear(Point3::new(1e-9, 1e-12, 5.0), 300.0),
            Move::linear(Point3::new(12_345_678.9, 1e-12, 5.0), 1000.0),
            Move::rapid(Point3::new(12_345_678.9, 1e-12, 11.0)),
        ],
    };
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    for line in text.lines() {
        assert!(!code(line).contains('E'), "{line}");
    }
    assert!(text.contains("X0.000 Y0.000"), "{text}");
    assert!(text.contains("X12345678.900"), "{text}");
}

#[test]
fn arc_ij_is_relative_to_emitted_start() {
    // The start rounds from 70.0004 to 70.000. I taken from the
    // unrounded start would be 5.0004 -> "5.000"; from the emitted
    // start it is 5.0008 -> "5.001". The arc lies in the waste beside
    // the sample part, so S9 (#177) passes it.
    let project = project_with("grblhal");
    let mut ops = ops_for(&project);
    let start = Point3::new(70.0004, 60.0, 5.0);
    let centre = Point::new(75.0008, 60.0);
    let r = centre.x - start.x;
    ops[0].toolpath = Toolpath {
        moves: vec![
            Move::rapid(Point3::new(start.x, start.y, 11.0)),
            Move::linear(start, 300.0),
            Move {
                kind: MoveKind::ArcCw { centre },
                to: Point3::new(centre.x, centre.y + r, 5.0),
                feed_mm_min: Some(1000.0),
            },
            Move::rapid(Point3::new(centre.x, centre.y + r, 11.0)),
        ],
    };
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let arc_line = text.lines().find(|l| has_word(l, "G2")).unwrap();
    assert!(arc_line.contains("I5.001 J0.000"), "{arc_line}");

    // Re-derive the controller's radius check from the text alone.
    let mut checked = 0;
    for m in parse_motion(&text).unwrap() {
        if m.kind != MotionKind::ArcCw {
            continue;
        }
        let (sx, sy) = (m.from.x.unwrap(), m.from.y.unwrap());
        let (i, j) = (m.arc.i.unwrap(), m.arc.j.unwrap());
        let (x, y) = (m.to.x.unwrap(), m.to.y.unwrap());
        let start_r = i.hypot(j);
        let end_r = (x - (sx + i)).hypot(y - (sy + j));
        let delta = (end_r - start_r).abs();
        assert!(
            delta <= 0.005 || (delta <= 0.5 && delta <= 0.001 * start_r),
            "{delta}"
        );
        checked += 1;
    }
    assert_eq!(checked, 1);
}

fn second_tool() -> Tool {
    Tool {
        id: ToolId::new("t2"),
        name: "3 mm downcut".into(),
        diameter_mm: 3.0,
        rpm: Some(12000),
        ..tool_6_35_upcut()
    }
}

/// The sample operation, then the same with a second tool. The
/// project's tool-change mode becomes `"m6"`, the one program with a
/// tool change that [`emit`] writes (#95).
fn two_tool_ops(project: &mut Project) -> Vec<EmitOp> {
    project.post_processor.tool_change_mode = ToolChangeMode::M6;
    project.tools.push(second_tool());
    let mut second = profile_op(project);
    second.id = OperationId::new("o2");
    second.name = "Second".into();
    second.tool = ToolId::new("t2");
    project.operations.push(Operation::Profile(second.clone()));
    let mut ops = ops_for(project);
    let (toolpath, _) = generate_profile(project, &second).unwrap();
    ops.push((Operation::Profile(second), second_tool(), toolpath).into());
    ops
}

/// #406 (design "Clearing passes", step 4): a tool run labelled as a
/// carve's clearing pass is written with its own tool, as a program of
/// its own when each tool run is one, and its operation comment names it
/// `… (clearing t1)`, written `… clearing t1` since a comment cannot
/// hold parentheses; the operation's own run keeps its plain name. The
/// carve is the sample rectangle's V-Carve with the 90° V-bit `t2`, a
/// 2 mm flat depth and one clearing pass with the sample's end mill
/// `t1`; both runs pass the safety pass (S17, and S9 holding the
/// clearing run to the floor, #408).
#[test]
fn clearing_runs_become_programs_of_their_own_tools() {
    let mut project = project_with("grblhal");
    project
        .tools
        .push(dc_project::test_support::tool_v_bit_90());
    let carve = dc_project::VCarveOp {
        flat_depth_mm: Some(2.0),
        step_over_fraction: Some(0.4),
        clearing: vec![dc_project::ClearingPass {
            tool: ToolId::new("t1"),
            strategy: dc_project::ClearanceStrategy::Offset,
            direction: dc_project::CutDirection::Climb,
            entry: dc_project::Entry::Plunge,
        }],
        ..dc_project::test_support::sample_v_carve()
    };
    project.operations = vec![Operation::VCarve(carve.clone())];
    let (runs, _) = dc_cam::generate_vcarve(&project, &carve).unwrap();
    let ops: Vec<EmitOp> = runs
        .into_iter()
        .map(|run| EmitOp {
            op: Operation::VCarve(carve.clone()),
            tool: project
                .tools
                .iter()
                .find(|t| t.id == run.tool)
                .unwrap()
                .clone(),
            toolpath: run.toolpath,
            holes: None,
            label: run.label,
            board: run.board,
        })
        .collect();
    assert_eq!(ops[0].label, RunLabel::Clearing(ToolId::new("t1")));
    project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    assert_eq!(programs.len(), 2, "one program per tool run");
    let first_name = ops[0].op.name().to_owned();
    let comment = |text: &str, wanted: &str| {
        text.lines()
            .any(|l| l.contains("Operation 1:") && l.contains(wanted))
    };
    assert_eq!(programs[0].first_tool.number, 1);
    assert!(
        comment(&programs[0].text, &format!("{first_name} clearing t1")),
        "{}",
        programs[0].text
    );
    assert_eq!(programs[1].first_tool.number, 2);
    assert!(
        comment(&programs[1].text, &first_name),
        "{}",
        programs[1].text
    );
    assert!(
        !programs[1].text.contains("clearing"),
        "{}",
        programs[1].text
    );

    // In one program with M6, each run is an operation of its own.
    project.post_processor.tool_change_mode = ToolChangeMode::M6;
    let (one, _) = emit(&project, &ops, DATE).unwrap();
    assert!(
        one.text
            .lines()
            .any(|l| l.contains("clearing t1") && l.contains("Operation 1:")),
        "{}",
        one.text
    );
}

#[test]
fn dwell_follows_every_spindle_start() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines: Vec<&str> = text.lines().collect();
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| has_word(lines[i], "M3"))
        .collect();
    assert_eq!(starts.len(), 2, "program start and the tool change");
    assert_eq!(code(lines[starts[1]]), "M3 S12000");
    for &i in &starts {
        assert_eq!(lines[i + 1], "G4 P3.000");
        let next_cut = (i..lines.len()).find(|&k| is_cut(lines[k])).unwrap();
        assert!(next_cut > i + 1);
    }
    // The tool change stops the spindle, then stops the program, before
    // the second start.
    let stop = (starts[0]..starts[1]).find(|&k| has_word(lines[k], "M5"));
    let change = (starts[0]..starts[1]).find(|&k| has_word(lines[k], "M6"));
    assert!(matches!((stop, change), (Some(s), Some(c)) if s < c));
}

#[test]
fn zero_dwell_emits_nothing() {
    let mut project = project_with("grblhal");
    project.post_processor.spindle_on_dwell_s = 0.0;
    let ops = two_tool_ops(&mut project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    assert!(!text.lines().any(|l| has_word(l, "G4")), "{text}");
    assert_eq!(text.lines().filter(|l| has_word(l, "M3")).count(), 2);
}

#[test]
fn too_few_decimal_places_is_error() {
    // The safety pass tolerates one unit in the last place; with too few
    // places that tolerance would hide a real error (1 inch = 25.4 mm).
    for (mm, inch, units) in [(1, 4, Units::Mm), (3, 2, Units::Inch)] {
        let mut project = sample_project();
        project.display_units = units;
        project.post_processor.decimal_places.mm = mm;
        project.post_processor.decimal_places.inch = inch;
        let result = emit(&project, &ops_for(&project), DATE);
        assert!(
            matches!(result, Err(ExportError::TooFewDecimalPlaces { .. })),
            "{mm}/{inch} {units:?}: {result:?}"
        );
    }
}

#[test]
fn header_placeholders_are_filled() {
    let project = project_with("grblhal");
    let text = emit_text(&project);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "( Project: Sample )");
    assert_eq!(lines[1], "( Generated by DreamCarve on 2026-10-15 )");
    assert!(!text.contains('{'), "{text}");
    assert!(text.contains("( Operation 1: Cut out )"));
    assert!(text.contains("( Tool 1: 6.35 mm upcut )"));
    assert_eq!(lines.last(), Some(&"M30"));
    assert!(text.ends_with("M30\n"));
    parse_program(&text).unwrap();
}

#[test]
fn first_motion_raises_z_before_moving_in_xy() {
    let text = emit_text(&project_with("grblhal"));
    let moves = parse_motion(&text).unwrap();
    assert_eq!(moves[0].to.x, None, "Z first: {:?}", moves[0]);
    assert_eq!(moves[0].to.z, Some(5.0));
    assert_eq!(moves[1].from.z, Some(5.0));
}

#[test]
fn line_numbers_when_asked() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.program.line_numbers = true;
    let text = emit_text_with(&project, &post);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("N10 "), "{}", lines[0]);
    assert!(lines[1].starts_with("N20 "), "{}", lines[1]);
}

/// The sample project on the bundled post-processor `post_id`, with the
/// router switched on and off by hand.
fn manual_project(post_id: &str) -> Project {
    let mut project = project_with(post_id);
    project.post_processor.spindle_control = SpindleControl::Manual;
    project
}

/// Machine Z of safe Z in the sample project: origin at stock top, safe
/// Z 5 mm above it.
const SAFE_Z_MACHINE: f64 = 5.0;

/// The machine Z the program holds when it reaches 0-based line `index`.
fn z_before(text: &str, index: usize) -> Option<f64> {
    parse_motion(text)
        .unwrap()
        .iter()
        .rfind(|m| m.line <= index)
        .and_then(|m| m.to.z)
}

/// #34, S3 (manual): a router's speed is set on its dial, so a tool
/// with no RPM exports in manual mode. In automatic mode it is still an
/// error, and feed and plunge stay required in both.
#[test]
fn manual_spindle_allows_missing_rpm() {
    for post in ["grblhal", "grbl"] {
        let mut project = manual_project(post);
        let mut ops = ops_for(&project);
        project.tools[0].rpm = None;
        ops[0].tool.rpm = None;
        if let Err(err) = emit(&project, &ops, DATE) {
            panic!("{post}: manual export without RPM failed: {err:?}");
        }

        project.post_processor.spindle_control = SpindleControl::Automatic;
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(&err, ExportError::MissingRpm { .. }),
            "{post}: {err:?}"
        );

        project.post_processor.spindle_control = SpindleControl::Manual;
        ops[0].tool.plunge_mm_min = None;
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::MissingFeed {
                    field: FeedField::Plunge,
                    ..
                }
            ),
            "{post}: {err:?}"
        );
    }
}

/// #34, Behaviour 2 and 5: in manual mode the program sends no spindle
/// words and no spin-up dwell. Before the first cut it retracts to safe
/// Z and pauses with a message to turn the router on; resuming is the
/// user's confirmation that it is running. #57: the first tool has no
/// tool-change stop, so the router-on pause is the only one.
#[test]
fn manual_spindle_pauses_before_first_cut() {
    for post in ["grblhal", "grbl"] {
        let text = emit_text(&manual_project(post));
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines {
            for word in ["M3", "M4", "M5", "G4"] {
                assert!(!has_word(line, word), "{post}: {word} in {line:?}");
            }
            assert!(
                !code(line).split_whitespace().any(|w| w.starts_with('S')),
                "{post}: speed word in {line:?}"
            );
        }
        let cut = first_index(&lines, is_cut).unwrap();
        let pauses: Vec<usize> = (0..cut).filter(|&i| has_word(lines[i], "M0")).collect();
        let [on] = pauses[..] else {
            panic!("{post}: one M0 before the first cut, not {pauses:?}");
        };
        assert!(
            lines[on].contains("(MSG, Turn the router ON."),
            "{post}: {}",
            lines[on]
        );
        let z = z_before(&text, on).unwrap_or_else(|| panic!("{post}: Z unknown"));
        assert_approx_eq!(z, SAFE_Z_MACHINE, 1e-9);
    }
}

/// #34, Behaviour 1: the tool's speed is a hint in the message, and the
/// speed part is left out when the tool has none.
#[test]
fn manual_spindle_message_includes_rpm_hint() {
    let project = manual_project("grblhal");
    let mut ops = ops_for(&project);
    let on_line = |ops: &[EmitOp]| -> String {
        let text = emit(&project, ops, DATE).unwrap().0.text;
        text.lines()
            .find(|l| l.contains("Turn the router ON"))
            .unwrap()
            .to_owned()
    };
    let with_rpm = on_line(&ops);
    assert_eq!(
        with_rpm,
        "M0 (MSG, Turn the router ON. Speed 18000 RPM. Resume to start cutting.)"
    );
    ops[0].tool.rpm = None;
    let without = on_line(&ops);
    assert_eq!(
        without,
        "M0 (MSG, Turn the router ON. Resume to start cutting.)"
    );
}

/// #34, Behaviour 3 and 4: with two tools the program pauses to turn
/// the router off before each tool change and at the end, each time at
/// safe Z. #43, #57: each later tool has two stops before it cuts, the
/// `M6` to fit it and set Z zero and, after a retract, a pause to turn
/// the router on. GRBL cannot change tools within a program.
#[test]
fn manual_spindle_pauses_off_at_tool_change_and_end() {
    let mut project = manual_project("grblhal");
    let ops = two_tool_ops(&mut project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines: Vec<&str> = text.lines().collect();
    let mut events: Vec<&str> = lines
        .iter()
        .filter_map(|l| {
            if l.contains("Turn the router ON") {
                Some("on")
            } else if l.contains("Turn the router OFF") {
                Some("off")
            } else if l.contains("Set Z zero") {
                Some("fit")
            } else if is_cut(l) {
                Some("cut")
            } else if has_word(l, "M30") {
                Some("end")
            } else {
                None
            }
        })
        .collect();
    events.dedup();
    assert_eq!(
        events,
        ["on", "cut", "off", "fit", "on", "cut", "off", "end"]
    );
    for (index, line) in lines.iter().enumerate() {
        if line.contains("Turn the router") {
            let z = z_before(&text, index);
            assert!(
                z.is_some_and(|z| (z - SAFE_Z_MACHINE).abs() < 1e-9),
                "line {} pauses at Z {z:?}",
                index + 1
            );
        }
    }
}

/// Asserts that the first move after 0-based line `stop` of `text` is a
/// rapid in Z alone to safe Z: Z is unknown after a stop (#68).
fn assert_retract_after(text: &str, stop: usize, label: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let axis = |w: &str| ['X', 'Y', 'Z'].iter().any(|&a| w.starts_with(a));
    let next = (stop + 1..lines.len())
        .find(|&k| code(lines[k]).split_whitespace().any(axis))
        .unwrap_or_else(|| panic!("{label}: no move after line {}", stop + 1));
    let words = code(lines[next]);
    assert!(
        has_word(lines[next], "G0")
            && !words
                .split_whitespace()
                .any(|w| w.starts_with('X') || w.starts_with('Y')),
        "{label}: line {} after the stop is {:?}, not a retract",
        next + 1,
        lines[next]
    );
    // Where Z is after line `next`.
    let z = z_before(text, next + 1).unwrap_or_else(|| panic!("{label}: Z unknown"));
    assert_approx_eq!(z, SAFE_Z_MACHINE, 1e-9);
}

/// #68, S4: the user may jog or set Z zero at the router-on pause, so
/// the program lifts to safe Z before it moves in XY.
#[test]
fn manual_router_on_pause_is_followed_by_a_retract() {
    for post in ["grblhal", "grbl"] {
        let text = emit_text(&manual_project(post));
        let lines: Vec<&str> = text.lines().collect();
        let on = first_index(&lines, |l| l.contains("Turn the router ON"))
            .unwrap_or_else(|| panic!("{post}: no router-on pause"));
        assert_retract_after(&text, on, post);
    }
}

/// #68, #57: the same after an `M6` tool change, in both spindle modes.
#[test]
fn tool_change_is_followed_by_a_retract() {
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let mut project = project_with("grblhal");
        project.post_processor.spindle_control = control;
        let ops = two_tool_ops(&mut project);
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        let lines: Vec<&str> = text.lines().collect();
        let change = first_index(&lines, |l| has_word(l, "M6"))
            .unwrap_or_else(|| panic!("{control:?}: no tool change"));
        assert_retract_after(&text, change, &format!("{control:?}"));
    }
}

/// #34: the program lists every stop, `M0` or `M6`, with its line and
/// message, so the CLI can tell the user what each stop is for when
/// the controller does not show the message (GRBL drops comments).
#[test]
fn program_lists_every_pause_with_its_message() {
    let on_18000 = "Turn the router ON. Speed 18000 RPM. Resume to start cutting.";
    let on_12000 = "Turn the router ON. Speed 12000 RPM. Resume to start cutting.";
    let off = "Turn the router OFF.";
    let tc2 = "Fit T2 3 mm downcut. Set Z zero, then resume.";
    for (post, tools, control, expected) in [
        ("grblhal", 2, SpindleControl::Automatic, vec![tc2]),
        (
            "grblhal",
            2,
            SpindleControl::Manual,
            vec![on_18000, off, tc2, on_12000, off],
        ),
        ("grbl", 1, SpindleControl::Manual, vec![on_18000, off]),
        ("grbl", 1, SpindleControl::Automatic, vec![]),
    ] {
        let mut project = project_with(post);
        project.post_processor.spindle_control = control;
        let ops = if tools == 1 {
            ops_for(&project)
        } else {
            two_tool_ops(&mut project)
        };
        let program = emit(&project, &ops, DATE).unwrap().0;
        let messages: Vec<&str> = program.pauses.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(messages, expected, "{post} {control:?}");
        let lines: Vec<&str> = program.text.lines().collect();
        let stop_lines: Vec<usize> = (0..lines.len())
            .filter(|&i| has_word(lines[i], "M0") || has_word(lines[i], "M6"))
            .map(|i| i + 1)
            .collect();
        let pause_lines: Vec<usize> = program.pauses.iter().map(|p| p.line).collect();
        assert_eq!(pause_lines, stop_lines, "{post} {control:?}");
    }
}

/// #34: pause messages go through the comment sanitizer and fit the
/// controller's line buffer, even with the longest speed and line
/// number a project can hold. The message is not cut short.
#[test]
fn manual_messages_are_sanitized() {
    for post in ["grblhal", "grbl"] {
        let project = manual_project(post);
        let mut definition = post_of(&project);
        definition.program.line_numbers = true;
        definition.program.line_number_increment = 1_000_000_000;
        let mut ops = ops_for(&project);
        ops[0].tool.rpm = Some(u32::MAX);
        let program = emit_with(&project, &definition, &ops, DATE).unwrap().0;
        assert!(!program.pauses.is_empty(), "{post}");
        for pause in &program.pauses {
            assert_eq!(
                sanitize_comment(&pause.message, MAX_MESSAGE_CHARS),
                pause.message,
                "{post}: line {}",
                pause.line
            );
        }
        let on = program
            .pauses
            .iter()
            .find(|p| p.message.contains("Turn the router ON"))
            .unwrap();
        assert!(
            on.message
                .ends_with(&format!("Speed {} RPM. Resume to start cutting.", u32::MAX)),
            "{post}: {}",
            on.message
        );
        for line in program.text.lines() {
            assert!(line.len() <= MAX_LINE_CHARS, "{post}: {} chars", line.len());
        }
    }
}

/// Whether `line` stops the program: an `M0` pause or an `M6` tool
/// change.
fn is_stop(line: &str) -> bool {
    has_word(line, "M0") || has_word(line, "M6")
}

/// Every spindle mode, for each bundled post-processor, with one and
/// with two tools. GRBL cannot change tools within a program (#57).
fn every_program() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for post in ["grblhal", "grbl"] {
        for control in [SpindleControl::Automatic, SpindleControl::Manual] {
            for tools in [1, 2] {
                if post == "grbl" && tools == 2 {
                    continue;
                }
                let mut project = project_with(post);
                project.post_processor.spindle_control = control;
                let ops = if tools == 1 {
                    ops_for(&project)
                } else {
                    two_tool_ops(&mut project)
                };
                let text = emit(&project, &ops, DATE).unwrap().0.text;
                out.push((format!("{post} {control:?} {tools} tool(s)"), text));
            }
        }
    }
    out
}

/// S4, #43, #57: the user fits the tool and switches the router at a
/// stop, so every `M0` and `M6` comes at a known Z at or above safe Z.
/// At program start Z is unknown, so a `G0 Z` comes before the first.
#[test]
fn tool_change_pause_is_at_safe_z() {
    let mut checked = 0;
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        let pauses: Vec<usize> = (0..lines.len()).filter(|&i| is_stop(lines[i])).collect();
        checked += pauses.len();
        for i in pauses {
            let z = z_before(&text, i);
            assert!(
                z.is_some_and(|z| z >= SAFE_Z_MACHINE - 1e-9),
                "{case}: line {} pauses at Z {z:?}",
                i + 1
            );
        }
    }
    assert!(checked > 0);
}

/// #43, #57: the user may set Z zero at the tool change, so the
/// program lifts to safe Z again before the spindle starts (automatic)
/// or before the router-on pause (manual).
#[test]
fn retracts_after_tool_change_pause() {
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        let fits: Vec<usize> = (0..lines.len())
            .filter(|&i| lines[i].contains("Set Z zero"))
            .collect();
        assert_eq!(fits.is_empty(), case.contains("1 tool"), "{case}");
        for i in fits {
            let start = (i + 1..lines.len())
                .find(|&k| has_word(lines[k], "M3") || lines[k].contains("Turn the router ON"))
                .unwrap_or_else(|| panic!("{case}: nothing starts after line {}", i + 1));
            let lift =
                (i + 1..start).find(|&k| has_word(lines[k], "G0") && code(lines[k]).contains('Z'));
            assert!(
                lift.is_some(),
                "{case}: no retract between lines {} and {}",
                i + 1,
                start + 1
            );
            let z = z_before(&text, start);
            assert!(
                z.is_some_and(|z| z >= SAFE_Z_MACHINE - 1e-9),
                "{case}: line {} starts at Z {z:?}",
                start + 1
            );
        }
    }
}

/// #43, option C: every tool change asks for the tool and for Z zero
/// to be set with it. A long tool name is shortened so the message fits
/// [`MAX_MESSAGE_CHARS`]; the instruction is never cut. #57: the first
/// tool has no tool change.
#[test]
fn tool_change_message_asks_to_set_z_zero() {
    for (case, text) in every_program() {
        let changes = usize::from(case.contains("2 tool"));
        let fits: Vec<&str> = text.lines().filter(|l| l.contains("(MSG, Fit T")).collect();
        assert_eq!(fits.len(), changes, "{case}: {fits:?}");
        for line in fits {
            assert!(
                line.ends_with(". Set Z zero, then resume.)"),
                "{case}: {line}"
            );
        }
    }
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let mut project = project_with("grblhal");
        project.post_processor.spindle_control = control;
        let mut ops = two_tool_ops(&mut project);
        ops[1].tool.name = "a very long tool name ".repeat(6);
        let program = emit(&project, &ops, DATE).unwrap().0;
        let fit = &program
            .pauses
            .iter()
            .find(|p| p.message.starts_with("Fit T2"))
            .unwrap()
            .message;
        assert!(fit.starts_with("Fit T2 a very long"), "{control:?}: {fit}");
        assert!(
            fit.ends_with(". Set Z zero, then resume."),
            "{control:?}: {fit}"
        );
        assert!(
            fit.len() <= MAX_MESSAGE_CHARS,
            "{control:?}: {} chars",
            fit.len()
        );
    }
}

/// #43: at a pause the user may probe or move by hand to set Z zero,
/// which changes the controller's motion mode and feed. The first move
/// after every pause states its motion word, and the first cut its feed.
#[test]
fn moves_after_a_pause_restate_motion_and_feed() {
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        for i in (0..lines.len()).filter(|&i| is_stop(lines[i])) {
            let Some(next) = (i + 1..lines.len()).find(|&k| {
                let c = code(lines[k]);
                c.contains('X') || c.contains('Y') || c.contains('Z')
            }) else {
                continue;
            };
            assert!(
                ["G0", "G1", "G2", "G3"]
                    .iter()
                    .any(|w| has_word(lines[next], w)),
                "{case}: line {} after the pause on line {} has no motion word",
                next + 1,
                i + 1
            );
            if let Some(cut) = (i + 1..lines.len()).find(|&k| is_cut(lines[k])) {
                assert!(
                    code(lines[cut]).contains('F'),
                    "{case}: the first cut after line {}, line {}, has no feed",
                    i + 1,
                    cut + 1
                );
            }
        }
    }
}

/// For each stop (`M0` or `M6`) in `text` that is followed by motion
/// or a spindle start: the lines between the stop and that line,
/// inclusive.
fn after_pauses(text: &str) -> Vec<(usize, Vec<&str>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for i in (0..lines.len()).filter(|&i| is_stop(lines[i])) {
        let next = (i + 1..lines.len()).find(|&k| {
            let c = code(lines[k]);
            is_stop(lines[k])
                || has_word(lines[k], "M3")
                || c.contains('X')
                || c.contains('Y')
                || c.contains('Z')
        });
        if let Some(next) = next.filter(|&k| !is_stop(lines[k])) {
            out.push((i + 1, lines[i + 1..=next].to_vec()));
        }
    }
    out
}

/// S5, #55: at a pause the user may leave the controller in another
/// state, so after every pause the program sets units, work
/// coordinates, distance mode, plane, and feed mode again before it
/// moves or starts the spindle.
#[test]
fn setup_is_resent_after_every_pause_before_motion() {
    let mut checked = 0;
    for (case, text) in every_program() {
        let pauses = after_pauses(&text);
        checked += pauses.len();
        for (line, between) in pauses {
            for word in ["G21", "G54", "G90", "G17", "G94"] {
                assert!(
                    between.iter().any(|l| has_word(l, word)),
                    "{case}: no {word} after the pause on line {line}: {between:?}"
                );
            }
        }
    }
    assert!(checked > 0);
}

/// #279, acceptance 41: grblHAL v3 and GRBL v3 cancel canned-cycle
/// motion and cutter compensation again in the setup written after
/// every stop, `G80` before `G40` as in the header, after an `M6` and
/// after a manual-spindle `M0`.
#[test]
fn setup_after_stop_repeats_g80_g40() {
    let (mut after_m6, mut after_m0) = (0, 0);
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        for (line, between) in after_pauses(&text) {
            if has_word(lines[line - 1], "M6") {
                after_m6 += 1;
            } else {
                after_m0 += 1;
            }
            let at = |word: &str| between.iter().position(|l| has_word(l, word));
            let (g80, g40) = (at("G80"), at("G40"));
            assert!(
                g80.is_some() && g40.is_some(),
                "{case}: no G80 or G40 after the stop on line {line}: {between:?}"
            );
            assert!(
                g80 < g40,
                "{case}: G40 before G80 after the stop on line {line}: {between:?}"
            );
        }
    }
    assert!(after_m6 > 0 && after_m0 > 0, "{after_m6} M6, {after_m0} M0");
}

/// #312: Mach3 v3 and UCCNC v3 cancel scaling (`G50`) and rotation
/// (`G69`) in the header, before the first move, and again in the
/// setup written after every stop that more motion follows, here the
/// manual-spindle `M0`s, since the user may set either at the
/// controller while it waits. Only those definitions write them.
#[test]
fn setup_after_stop_repeats_g50_g69() {
    for post in ["mach3", "uccnc"] {
        let mut project = project_with(post);
        project.post_processor.spindle_control = SpindleControl::Manual;
        let text = emit(&project, &ops_for(&project), DATE).unwrap().0.text;
        let lines: Vec<&str> = text.lines().collect();
        let moves = |l: &str| {
            let c = code(l);
            c.contains('X') || c.contains('Y') || c.contains('Z')
        };
        let first_move = first_index(&lines, moves).unwrap();
        for word in ["G50", "G69"] {
            assert!(
                lines[..first_move].iter().any(|l| has_word(l, word)),
                "{post}: no {word} before the first move:\n{text}"
            );
        }
        let pauses = after_pauses(&text);
        assert!(!pauses.is_empty(), "{post}: no pause:\n{text}");
        for (line, between) in pauses {
            for word in ["G50", "G69"] {
                assert!(
                    between.iter().any(|l| has_word(l, word)),
                    "{post}: no {word} after the stop on line {line}: {between:?}"
                );
            }
        }
    }
    // #316: grblHAL v4 cancels scaling (`G50`) the same way, on a line
    // of its own, in the header and after every stop: an `M6` and a
    // manual-spindle `M0`. It has no rotation, so no `G69`. GRBL has
    // neither, so its programs write none of the four.
    let (mut after_m6, mut after_m0) = (0, 0);
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        let grblhal = case.starts_with("grblhal ");
        let present: &[&str] = if grblhal { &["G50"] } else { &[] };
        for word in ["G50", "G51", "G68", "G69"] {
            assert_eq!(
                text.lines().any(|l| has_word(l, word)),
                present.contains(&word),
                "{case}: {word}:\n{text}"
            );
        }
        if !grblhal {
            continue;
        }
        for l in lines.iter().filter(|l| has_word(l, "G50")) {
            assert_eq!(code(l), "G50", "{case}: G50 shares its line: {l}");
        }
        let moves = |l: &str| {
            let c = code(l);
            c.contains('X') || c.contains('Y') || c.contains('Z')
        };
        let first_move = first_index(&lines, moves).unwrap();
        assert!(
            lines[..first_move].iter().any(|l| has_word(l, "G50")),
            "{case}: no G50 before the first move:\n{text}"
        );
        for (line, between) in after_pauses(&text) {
            if has_word(lines[line - 1], "M6") {
                after_m6 += 1;
            } else {
                after_m0 += 1;
            }
            let at = |word: &str| between.iter().position(|l| has_word(l, word));
            assert!(
                at("G50").is_some(),
                "{case}: no G50 after the stop on line {line}: {between:?}"
            );
            assert!(
                at("G40") < at("G50"),
                "{case}: G50 before G40 after the stop on line {line}: {between:?}"
            );
        }
    }
    assert!(after_m6 > 0 && after_m0 > 0, "{after_m6} M6, {after_m0} M0");
}

/// #279: in `G80` mode a line with axis words and no motion word is an
/// error on grblHAL and GRBL (`definitions/README.md`, "Header modal
/// state"). So the first line that moves after the header's `G80`, and
/// after each `G80` of a setup written again after a stop, carries
/// `G0` or `G1`.
#[test]
fn first_motion_after_g80_has_motion_word() {
    for (case, text) in every_program() {
        let lines: Vec<&str> = text.lines().collect();
        let moves = |l: &str| {
            let c = code(l);
            c.contains('X') || c.contains('Y') || c.contains('Z')
        };
        let cancels: Vec<usize> = (0..lines.len())
            .filter(|&i| has_word(lines[i], "G80"))
            .collect();
        let first_move = first_index(&lines, moves).unwrap();
        assert!(
            cancels.first().is_some_and(|&i| i < first_move),
            "{case}: no G80 before the first move: {text}"
        );
        let stops = after_pauses(&text).len();
        assert_eq!(cancels.len(), 1 + stops, "{case}: {text}");
        for i in cancels {
            let next = (i + 1..lines.len()).find(|&k| moves(lines[k])).unwrap();
            assert!(
                has_word(lines[next], "G0") || has_word(lines[next], "G1"),
                "{case}: line {} moves after the G80 on line {} without G0 or G1: {}",
                next + 1,
                i + 1,
                lines[next]
            );
        }
    }
}

/// #55: an inch project sets inches again after a pause, never
/// millimetres.
#[test]
fn inch_project_resends_g20() {
    for (post, control) in [
        ("grblhal", SpindleControl::Automatic),
        ("grbl", SpindleControl::Manual),
    ] {
        let mut project = project_with(post);
        project.display_units = Units::Inch;
        project.post_processor.spindle_control = control;
        let ops = if post == "grblhal" {
            two_tool_ops(&mut project)
        } else {
            ops_for(&project)
        };
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        let pauses = after_pauses(&text);
        assert!(!pauses.is_empty(), "{post}");
        for (line, between) in pauses {
            assert!(
                between.iter().any(|l| has_word(l, "G20")),
                "{post}: no G20 after the pause on line {line}"
            );
            assert!(
                !between.iter().any(|l| has_word(l, "G21")),
                "{post}: G21 after the pause on line {line}"
            );
        }
    }
}

/// #55: a definition whose header leaves out `G90` still gets it, at
/// the start and after every pause, because S5 requires it.
#[test]
fn header_without_g90_still_sets_absolute_mode() {
    for post in ["grblhal", "grbl"] {
        let project = project_with(post);
        let mut definition = post_of(&project);
        definition.program.header.retain(|l| l.trim() != "G90");
        let text = emit_text_with(&project, &definition);
        let lines: Vec<&str> = text.lines().collect();
        let first_move = (0..lines.len())
            .find(|&k| code(lines[k]).contains('Z'))
            .unwrap();
        assert!(
            lines[..first_move].iter().any(|l| has_word(l, "G90")),
            "{post}: no G90 before the first move"
        );
        for (line, between) in after_pauses(&text) {
            assert!(
                between.iter().any(|l| has_word(l, "G90")),
                "{post}: no G90 after the pause on line {line}"
            );
        }
    }
}

/// Review 1.10: a `G1`, `G2` or `G3` outside every operation's lines
/// has no toolpath to be read back against (S6), no material (S9) and
/// no tool whose rates and radius hold it (S3, S7, S10), so it is
/// refused wherever it is. Only a definition's header or footer can
/// write one. Here grblHAL's footer, which stops the spindle itself,
/// first plunges 1 mm into the stock at five times the tool's feed
/// where the Profile ended, then cuts across; with the spindle still
/// running, every other rule let both through.
#[test]
fn a_cut_outside_every_operation_is_refused() {
    let project = project_with("grblhal");
    let bundled = post_of(&project);
    assert_eq!(bundled.program.footer, ["M5"]);
    let ops = ops_for(&project);
    let lines = emit_with(&project, &bundled, &ops, DATE)
        .unwrap()
        .0
        .text
        .lines()
        .count();
    for cut in ["G1 Z-1 F5000", "G1 X50 F5000", "G2 X50 Y50 I10 J0 F5000"] {
        let mut definition = bundled.clone();
        definition.program.footer.insert(0, cut.to_owned());
        let err = emit_with(&project, &definition, &ops, DATE).unwrap_err();
        // The footer's first line: the bundled program's `M5`.
        assert!(
            matches!(err, ExportError::CutOutsideOperation { line } if line == lines - 1),
            "{cut}: {err:?}"
        );
    }
    // A rapid there is still held to S2 only.
    let mut definition = bundled;
    definition.program.footer.insert(0, "G0 Z20".to_owned());
    emit_with(&project, &definition, &ops, DATE).unwrap();
}

/// #57: the user fits the first tool and sets work zero before starting
/// the program, so nothing stops the program before its first cut but,
/// with manual spindle control, the pause to turn the router on.
#[test]
fn first_tool_has_no_tool_change_pause() {
    for (post, tools) in [("grblhal", 1), ("grblhal", 2), ("grbl", 1)] {
        for control in [SpindleControl::Automatic, SpindleControl::Manual] {
            let mut project = project_with(post);
            project.post_processor.spindle_control = control;
            let ops = if tools == 1 {
                ops_for(&project)
            } else {
                two_tool_ops(&mut project)
            };
            let text = emit(&project, &ops, DATE).unwrap().0.text;
            let lines: Vec<&str> = text.lines().collect();
            let cut = first_index(&lines, is_cut).unwrap();
            let stops: Vec<&str> = lines[..cut]
                .iter()
                .filter(|l| has_word(l, "M0") || has_word(l, "M6"))
                .copied()
                .collect();
            let expected: &[&str] = match control {
                SpindleControl::Automatic => &[],
                SpindleControl::Manual => {
                    &["M0 (MSG, Turn the router ON. Speed 18000 RPM. Resume to start cutting.)"]
                }
            };
            assert_eq!(stops, expected, "{post} {control:?} {tools} tool(s)");
            assert!(
                !text.contains("Fit T1"),
                "{post} {control:?} {tools} tool(s)"
            );
        }
    }
}

/// #57: on grblHAL a later tool is changed with `T<n> M6`, which stops
/// the program in a state where the user may jog to set Z zero. Before
/// it the program retracts to safe Z and stops the spindle; after it,
/// it writes the setup again, retracts from the new zero, and starts
/// the spindle with its dwell. There is no `M0` in the change.
#[test]
fn grblhal_later_tool_uses_m6() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines: Vec<&str> = text.lines().collect();
    let changes: Vec<usize> = (0..lines.len())
        .filter(|&i| has_word(lines[i], "M6"))
        .collect();
    let [change] = changes[..] else {
        panic!("one M6, not {changes:?}: {text}");
    };
    assert_eq!(code(lines[change]), "T2 M6", "{}", lines[change]);
    assert!(
        lines[change].contains("(MSG, Fit T2 3 mm downcut. Set Z zero, then resume.)"),
        "{}",
        lines[change]
    );

    let last_cut = (0..change).rev().find(|&k| is_cut(lines[k])).unwrap();
    let next_cut = (change..lines.len()).find(|&k| is_cut(lines[k])).unwrap();
    assert!(
        (last_cut..change).any(|k| has_word(lines[k], "M5")),
        "the spindle stops before the change: {text}"
    );
    let z = z_before(&text, change);
    assert!(
        z.is_some_and(|z| z >= SAFE_Z_MACHINE - 1e-9),
        "the change is at safe Z, not {z:?}"
    );
    assert!(
        !(last_cut..next_cut).any(|k| has_word(lines[k], "M0")),
        "no M0 in the change: {text}"
    );

    let start = (change..next_cut)
        .find(|&k| has_word(lines[k], "M3"))
        .unwrap();
    let after = &lines[change + 1..start];
    for word in ["G21", "G54", "G90", "G17", "G94"] {
        assert!(
            after.iter().any(|l| has_word(l, word)),
            "no {word} after the change: {after:?}"
        );
    }
    assert!(
        after
            .iter()
            .any(|l| has_word(l, "G0") && code(l).contains('Z')),
        "no retract after the change: {after:?}"
    );
    assert_eq!(code(lines[start]), "M3 S12000");
    assert_eq!(lines[start + 1], "G4 P3.000");
    let z = z_before(&text, start);
    assert!(z.is_some_and(|z| z >= SAFE_Z_MACHINE - 1e-9), "{z:?}");
}

/// #57: grblHAL skips `M6` for the tool it already holds, and that
/// number survives from the previous job. A program that changes tools
/// first tells grblHAL which tool the user fitted, with `M61`, so a
/// later `M6` is never skipped. A one-tool program has no `M6` and so
/// no `M61`.
#[test]
fn grblhal_sets_current_tool_before_a_tool_change() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines: Vec<&str> = text.lines().collect();
    let set = first_index(&lines, |l| has_word(l, "M61")).unwrap();
    assert_eq!(code(lines[set]), "M61 Q1");
    let first_move = first_index(&lines, |l| code(l).contains('Z')).unwrap();
    assert!(set < first_move, "{text}");
    assert_eq!(lines.iter().filter(|l| has_word(l, "M61")).count(), 1);

    let text = emit_text(&project_with("grblhal"));
    assert!(!text.lines().any(|l| has_word(l, "M61")), "{text}");
}

/// #57: GRBL has no `M6`, and in its `M0` feed hold it refuses to jog,
/// so the user cannot set Z zero for a new bit. A GRBL program that
/// needs a second tool is refused, naming the tools. Several operations
/// with one tool still export.
#[test]
fn grbl_multi_tool_export_fails() {
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let mut project = project_with("grbl");
        project.post_processor.spindle_control = control;
        let mut ops = two_tool_ops(&mut project);
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::ToolChangeNotSupported { post, tools }
                    if post == "grbl" && tools == &["6.35 mm upcut", "3 mm downcut"]
            ),
            "{control:?}: {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("6.35 mm upcut") && message.contains("3 mm downcut"),
            "{message}"
        );

        ops[1].tool = ops[0].tool.clone();
        profile_of(&mut ops[1].op).tool = ops[0].tool.id.clone();
        // Generated for the tool it now has: the 3 mm tool's path cut
        // with the 6.35 mm tool would cut into the part (S9, #177).
        let second = profile_of(&mut ops[1].op).clone();
        ops[1].toolpath = generate_profile(&project, &second).unwrap().0;
        emit(&project, &ops, DATE).unwrap();
    }
}

/// #95: with `"separate_programs"`, the bundled default, a two-tool
/// project is one program per tool run, on grblHAL and on GRBL. Each is
/// the program its run's operations alone emit: no `M6`, no `M61`, one
/// spindle start at its tool's speed, its own extents, and it passed
/// the safety pass on its own. Output is the same on every run.
#[test]
fn tool_run_program_passes_safety_alone() {
    for post in ["grblhal", "grbl"] {
        let mut project = project_with(post);
        let ops = two_tool_ops(&mut project);
        project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
        assert_eq!(programs.len(), 2, "{post}: one program per tool run");
        for (k, program) in programs.iter().enumerate() {
            let text = &program.text;
            for word in ["M6", "M61"] {
                assert!(
                    !text.lines().any(|l| has_word(l, word)),
                    "{post} program {k} has {word}: {text}"
                );
            }
            let starts: Vec<&str> = text.lines().filter(|l| has_word(l, "M3")).collect();
            let rpm = ops[k].tool.rpm.unwrap();
            assert_eq!(starts, [format!("M3 S{rpm}")], "{post} program {k}");
            assert_eq!(
                program.first_tool.number,
                u32::try_from(k + 1).unwrap(),
                "{post} program {k}"
            );
            let (alone, _) = emit(&project, &ops[k..=k], DATE).unwrap();
            assert_eq!(program, &alone, "{post} program {k} is its run alone");
        }
        let (again, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
        assert_eq!(again, programs, "{post}: deterministic");
    }
}

/// #95: with `"m6"` the programs are the one [`emit`] writes, with its
/// `T2 M6` tool change, byte for byte.
#[test]
fn m6_mode_emits_one_program_with_m6() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    let (single, _) = emit(&project, &ops, DATE).unwrap();
    assert_eq!(programs, [single]);
}

/// Sets the `number` of the tools of `project` and `ops`, in project
/// order: the project is the one the emitter reads numbers from, and
/// each operation carries a copy of its tool.
fn number_tools(project: &mut Project, ops: &mut [EmitOp], numbers: &[Option<u32>]) {
    for (tool, &number) in project.tools.iter_mut().zip(numbers) {
        tool.number = number;
        for entry in ops.iter_mut().filter(|e| e.tool.id == tool.id) {
            entry.tool.number = number;
        }
    }
}

/// #120, capability 21a: a tool's set `number` is its `T` number in
/// every program. One program per tool run: each says which tool it
/// starts with, by id and by number (the number names its file), and
/// writes the number in the definition's `{toolNumber}` comment.
#[test]
fn set_tool_number_is_written_as_t_word_and_file_name() {
    let mut project = project_with("grblhal");
    let mut ops = two_tool_ops(&mut project);
    project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    number_tools(&mut project, &mut ops, &[Some(7), Some(12)]);
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    let [first, second] = programs.as_slice() else {
        panic!("two programs, not {}", programs.len());
    };
    for (program, id, number, name) in [
        (first, "t1", 7, "6.35 mm upcut"),
        (second, "t2", 12, "3 mm downcut"),
    ] {
        assert_eq!(program.first_tool.id, ToolId::new(id));
        assert_eq!(program.first_tool.number, number);
        let comment = format!("( Tool {number}: {name} )");
        assert!(
            program.text.lines().any(|l| l == comment),
            "{comment}: {}",
            program.text
        );
    }
    for (program, absent) in [(first, "( Tool 1:"), (second, "( Tool 2:")] {
        assert!(!program.text.contains(absent), "{}", program.text);
    }
}

/// #120: with no `number`, a tool's `T` number is its 1-based place in
/// the project's tool list, as in phase 1. Numbers that equal the
/// places write the same bytes as no numbers, in both modes.
#[test]
fn unset_tool_number_falls_back_to_position() {
    for mode in [ToolChangeMode::M6, ToolChangeMode::SeparatePrograms] {
        let mut project = project_with("grblhal");
        let mut ops = two_tool_ops(&mut project);
        project.post_processor.tool_change_mode = mode;
        let (unset, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
        let numbers: Vec<u32> = unset.iter().map(|p| p.first_tool.number).collect();
        let expected: &[u32] = match mode {
            ToolChangeMode::M6 => &[1],
            ToolChangeMode::SeparatePrograms => &[1, 2],
        };
        assert_eq!(numbers, expected, "{mode:?}");
        if mode == ToolChangeMode::M6 {
            let text = &unset[0].text;
            assert!(text.lines().any(|l| l == "M61 Q1"), "{text}");
            assert!(text.lines().any(|l| l.starts_with("T2 M6 ")), "{text}");
        }

        number_tools(&mut project, &mut ops, &[Some(1), Some(2)]);
        let (set, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
        assert_eq!(set, unset, "{mode:?}");
    }
}

/// #120: in one program with `M6`, the set numbers are the `M61 Q<n>`
/// of the first tool and the `T<n> M6` and `Fit T<n>` message of the
/// later one, and the pause list says the same. The program still
/// passes the safety pass, which `emit` runs.
#[test]
fn m6_mode_uses_tool_number_in_m61_and_m6() {
    let mut project = project_with("grblhal");
    let mut ops = two_tool_ops(&mut project);
    number_tools(&mut project, &mut ops, &[Some(7), Some(12)]);
    let program = emit(&project, &ops, DATE).unwrap().0;
    let text = &program.text;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(program.first_tool.number, 7);
    let sets: Vec<&str> = lines
        .iter()
        .filter(|l| has_word(l, "M61"))
        .copied()
        .collect();
    assert_eq!(sets, ["M61 Q7"], "{text}");
    let changes: Vec<&str> = lines
        .iter()
        .filter(|l| has_word(l, "M6"))
        .copied()
        .collect();
    assert_eq!(
        changes,
        ["T12 M6 (MSG, Fit T12 3 mm downcut. Set Z zero, then resume.)"],
        "{text}"
    );
    assert!(lines.contains(&"( Tool 7: 6.35 mm upcut )"), "{text}");
    assert!(lines.contains(&"( Tool 12: 3 mm downcut )"), "{text}");
    let messages: Vec<&str> = program.pauses.iter().map(|p| p.message.as_str()).collect();
    assert_eq!(
        messages,
        ["Fit T12 3 mm downcut. Set Z zero, then resume."],
        "{text}"
    );
    for position in ["T1", "T2", "Q1", "Q2"] {
        assert!(
            !lines.iter().any(|l| has_word(l, position)),
            "{position}: {text}"
        );
    }
}

/// #120: the post-processor trusts no caller. Two tools of one program
/// that resolve to the same `T` number, set or by position, or a set
/// number of 0, fail the export, which validation would have refused.
#[test]
fn clashing_or_zero_tool_number_fails_export() {
    for (numbers, number) in [
        (&[Some(5), Some(5)][..], 5),
        (&[None, Some(1)][..], 1),
        (&[Some(2), None][..], 2),
    ] {
        let mut project = project_with("grblhal");
        let mut ops = two_tool_ops(&mut project);
        number_tools(&mut project, &mut ops, numbers);
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(
                &err,
                ExportError::DuplicateToolNumber { tool, other, number: n }
                    if tool == &ToolId::new("t2") && other == &ToolId::new("t1") && *n == number
            ),
            "{numbers:?}: {err:?}"
        );
    }
    let mut project = project_with("grblhal");
    let mut ops = ops_for(&project);
    number_tools(&mut project, &mut ops, &[Some(0)]);
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(&err, ExportError::ToolNumberZero { tool } if tool == &ToolId::new("t1")),
        "{err:?}"
    );
}

/// #95: [`emit`] writes one program, so with `"separate_programs"` it
/// refuses a second tool rather than write an `M6` the user did not
/// choose.
#[test]
fn emit_refuses_tool_change_in_separate_programs_mode() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    let err = emit(&project, &ops, DATE).unwrap_err();
    assert!(
        matches!(err, ExportError::ToolChangeNotSupported { .. }),
        "{err:?}"
    );
}

/// Acceptance 33: `--one-program` ([`ProgramSplit::OneProgram`]) on
/// grblHAL writes the one program `"m6"` mode writes, `T2 M6` included,
/// while the project's own mode stays `"separate_programs"`.
#[test]
fn one_program_flag_writes_m6_on_grblhal() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::OneProgram, DATE).unwrap();
    let [program] = programs.as_slice() else {
        panic!("one program, not {}", programs.len());
    };
    assert!(
        program.text.lines().any(|l| l.starts_with("T2 M6")),
        "{}",
        program.text
    );
    let mut m6 = project.clone();
    m6.post_processor.tool_change_mode = ToolChangeMode::M6;
    let (expected, _) = emit(&m6, &ops, DATE).unwrap();
    assert_eq!(*program, expected, "the program `\"m6\"` mode writes");
}

/// Acceptance 33: GRBL has no `M6`, so one program with tool changes is
/// refused naming the post-processor, whatever the project's mode says.
#[test]
fn one_program_flag_on_grbl_is_error() {
    for mode in [ToolChangeMode::SeparatePrograms, ToolChangeMode::M6] {
        let mut project = project_with("grbl");
        let ops = two_tool_ops(&mut project);
        project.post_processor.tool_change_mode = mode;
        let err = emit_programs(&project, &ops, ProgramSplit::OneProgram, DATE).unwrap_err();
        assert!(
            matches!(&err, ExportError::OneProgramNeedsM6 { post } if post == "grbl"),
            "{mode:?}: {err:?}"
        );
        assert!(err.to_string().contains("`grbl`"), "{err}");
    }
}

/// Acceptance 33: `--programs-per-tool` ([`ProgramSplit::ProgramPerTool`])
/// on a project in `"m6"` mode writes one program per tool run, exactly
/// what `"separate_programs"` writes, and leaves the project as it was.
#[test]
fn programs_per_tool_flag_overrides_m6_mode_without_saving() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    assert_eq!(project.post_processor.tool_change_mode, ToolChangeMode::M6);
    let before = project.clone();
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE).unwrap();
    assert_eq!(programs.len(), 2, "one program per tool run");
    for (k, program) in programs.iter().enumerate() {
        for word in ["M6", "M61"] {
            assert!(
                !program.text.lines().any(|l| has_word(l, word)),
                "program {k} has {word}: {}",
                program.text
            );
        }
    }
    let mut separate = project.clone();
    separate.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    let (expected, _) = emit_programs(&separate, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    assert_eq!(programs, expected, "what `\"separate_programs\"` writes");
    assert_eq!(project, before, "the project is not changed");
}

/// Without a flag ([`ProgramSplit::ProjectDefault`]) the project's mode
/// decides, as before (#95): `"m6"` is the one program [`emit`] writes,
/// `"separate_programs"` one per tool run.
#[test]
fn project_default_split_follows_the_project_mode() {
    let mut project = project_with("grblhal");
    let ops = two_tool_ops(&mut project);
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    let (single, _) = emit(&project, &ops, DATE).unwrap();
    assert_eq!(programs, [single], "m6: one program");
    project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
    let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
    assert_eq!(programs.len(), 2, "separate_programs: one per tool run");
}

/// A one-hole Drill at (50, 50), 3 mm deep, of an 8 mm circle added to
/// `project`: in at safe Z (11), down to the retract plane 1 mm above
/// the 6 mm stock, plunge, dwell 0.5 s, back to safe Z. Hand-written so
/// the emitter is tested apart from the generator.
fn drill_ops(project: &mut Project) -> Vec<EmitOp> {
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
    let tool = project.tools[0].clone();
    let toolpath = Toolpath {
        moves: vec![
            Move::rapid(Point3::new(50.0, 50.0, 11.0)),
            Move::rapid(Point3::new(50.0, 50.0, 7.0)),
            Move::linear(Point3::new(50.0, 50.0, 3.0), 300.0),
            Move::dwell(Point3::new(50.0, 50.0, 3.0), 0.5),
            Move::rapid(Point3::new(50.0, 50.0, 11.0)),
        ],
    };
    vec![(Operation::Drill(op), tool, toolpath).into()]
}

/// The line after the plunge to the hole bottom (Z −3 on a stock-top
/// origin) in `text`.
fn line_after_plunge(text: &str) -> &str {
    let lines: Vec<&str> = text.lines().collect();
    let plunge = lines
        .iter()
        .position(|l| has_word(l, "G1") && has_word(l, "Z-3.000"))
        .unwrap_or_else(|| panic!("no plunge to Z-3.000 in\n{text}"));
    lines[plunge + 1]
}

/// Design spec, "Algorithms → Drill" step 5: a dwell is written as the
/// post-processor's dwell command, `G4 P<seconds>` on grblHAL, where P
/// is seconds (grblHAL `gcode.c`, G4), at the bottom of the hole, and
/// the program passes the safety pass (S6 matches the dwell, S15 the
/// rapid down to the retract plane).
#[test]
fn dwell_writes_g4() {
    let mut project = project_with("grblhal");
    let ops = drill_ops(&mut project);
    let (program, _) = emit(&project, &ops, DATE).unwrap();
    assert_eq!(line_after_plunge(&program.text), "G4 P0.500");
}

/// Some controllers take `G4 P` in milliseconds (UCCNC; design spec,
/// "Post-processor definitions"); a definition whose dwell `units` are
/// milliseconds gets the same dwell as `P500`.
#[test]
fn dwell_in_milliseconds_when_the_post_says_so() {
    let mut project = project_with("grblhal");
    let mut post = post_of(&project);
    post.feed_speed.spindle_on_dwell.units = dc_project::DwellUnits::Milliseconds;
    let ops = drill_ops(&mut project);
    let (program, _) = emit_with(&project, &post, &ops, DATE).unwrap();
    assert_eq!(line_after_plunge(&program.text), "G4 P500");
}

/// UCCNC's `G4 P` is milliseconds by default (UCCNC user's manual,
/// "Dwell : G4", and General settings, "Measure dwell in seconds
/// instead of msec"), so the 3 s spin-up is `G4 P3000` and a Drill's
/// 0.5 s dwell `G4 P500`. The design spec expected this of Mach3, whose
/// manual documents seconds; see the definitions README.
#[test]
fn uccnc_dwell_is_milliseconds() {
    let mut project = project_with("uccnc");
    let text = emit_text(&project);
    assert!(text.lines().any(|l| code(l) == "G4 P3000"), "{text}");
    let ops = drill_ops(&mut project);
    let (program, _) = emit(&project, &ops, DATE).unwrap();
    assert_eq!(line_after_plunge(&program.text), "G4 P500");
    let mach3 = emit_text(&project_with("mach3"));
    assert!(
        mach3.lines().any(|l| code(l).ends_with("G4 P3.000")),
        "{mach3}"
    );
}

/// Mach3 skips a line that starts with `%` (Using Mach3Mill 1.84,
/// 10.5.5), so the opening `%` gets no line number; the next line is
/// `N10`.
#[test]
fn percent_line_has_no_line_number() {
    let text = emit_text(&project_with("mach3"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "%", "{text}");
    assert!(lines[1].starts_with("N10 "), "{text}");
}

/// RS274/NGC (NIST IR 6556, 3.3.1) and Mach3 (Using Mach3Mill 1.84,
/// 10.5.1) take line numbers of at most five digits, and Mach3 says
/// they "may be repeated or used out of order": after `N99999` the
/// numbers start again from the increment.
#[test]
fn line_numbers_wrap_after_five_digits() {
    let project = project_with("grblhal");
    let mut post = post_of(&project);
    post.program.line_numbers = true;
    post.program.line_number_increment = 40_000;
    let text = emit_text_with(&project, &post);
    let numbers: Vec<&str> = text
        .lines()
        .take(4)
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(numbers, ["N40000", "N80000", "N40000", "N80000"], "{text}");
}

// Canned cycles (`docs/specs/phase-2/design.md`, "Canned cycles";
// ported from PureCutCNC `postprocessor.test.ts`). No bundled
// definition has canned cycles yet, so these use a test-only one.

/// The holes the canned tests drill: 8 mm circles at these centres, in
/// drawing order.
const HOLES: [(f64, f64); 3] = [(20.0, 20.0), (80.0, 30.0), (50.0, 70.0)];

/// The sample project on grblHAL with the 8 mm circles of [`HOLES`] and
/// one Drill of them, 5 mm deep into the 6 mm stock: `cycle`,
/// `dwell_s`, `retract_mm`.
fn drill_project(
    cycle: dc_project::DrillCycle,
    dwell_s: f64,
    retract_mm: f64,
) -> (Project, dc_project::DrillOp) {
    let mut project = project_with("grblhal");
    let hole = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d2"),
        name: "Hole".into(),
        profiles: vec![dc_geometry::Polyline::circle(Point::new(0.0, 0.0), 4.0)],
    };
    project.definitions.insert(hole.id.clone(), hole);
    let mut targets = Vec::new();
    for (k, (x, y)) in HOLES.into_iter().enumerate() {
        let id = dc_project::InstanceId::new(format!("h{}", k + 1));
        project.instances.push(dc_project::FeatureInstance {
            id: id.clone(),
            definition: dc_project::DefinitionId::new("d2"),
            transform: dc_geometry::Transform::translate(x, y),
            name: format!("Hole {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        targets.push(id);
    }
    let op = dc_project::DrillOp {
        targets,
        cycle,
        dwell_s,
        depth_mm: Some(5.0),
        retract_mm,
        ..dc_project::test_support::sample_drill()
    };
    project.operations = vec![Operation::Drill(op.clone())];
    (project, op)
}

/// The generated Drill of `op`, with its hole records.
fn drill_entry(project: &Project, op: &dc_project::DrillOp) -> Vec<EmitOp> {
    let (drilled, _) = dc_cam::generate_drill(project, op).unwrap();
    dc_cam::test_support::assert_safe(&drilled.toolpath, project, &Operation::Drill(op.clone()));
    vec![EmitOp::drill(
        Operation::Drill(op.clone()),
        project.tools[0].clone(),
        drilled,
    )]
}

/// `project`'s post-processor with canned cycles `G81`, `G82`, `G83`,
/// `G73`, `G98`, `G80`, a 0.5 mm re-entry gap, and `use_canned_cycles`
/// on.
fn canned_post(project: &Project) -> PostProcessor {
    let mut post = post_of(project);
    post.canned_cycles = Some(CannedCycles {
        drill_command: Some("G81".into()),
        drill_with_dwell_command: Some("G82".into()),
        peck_drill_command: Some("G83".into()),
        chip_break_drill_command: Some("G73".into()),
        peck_step_word: "Q".into(),
        retract_mode: Some(RetractMode::G98),
        cancel_command: "G80".into(),
        reentry_gap_mm: Some(0.5),
    });
    post.use_canned_cycles = true;
    post
}

/// The code of every line of `text`.
fn codes(text: &str) -> Vec<String> {
    text.lines().map(code).collect()
}

/// Whether `text` has any canned-cycle word. `G80` alone is no cycle:
/// the grblHAL and GRBL v3 headers write it to cancel one (#279).
fn has_cycle_word(text: &str) -> bool {
    ["G73", "G81", "G82", "G83", "G98", "G99"]
        .iter()
        .any(|w| text.lines().any(|l| has_word(l, w)))
}

/// The lines between the one `G98` and the one `G80` after it of
/// `text`; also asserts the line before `G98` is the rapid over the
/// first hole. A `G80` before the `G98` is the header's (#279).
fn cycle_lines(text: &str, first: (f64, f64)) -> Vec<String> {
    let codes = codes(text);
    let all = |word: &str, from: usize| -> Vec<usize> {
        (from..codes.len())
            .filter(|&i| codes[i].split_whitespace().any(|w| w == word))
            .collect()
    };
    let [g98] = all("G98", 0)[..] else {
        panic!("one G98 in\n{text}");
    };
    let [g80] = all("G80", g98)[..] else {
        panic!("one G80 after the G98 in\n{text}");
    };
    assert_eq!(codes[g98], "G98", "{text}");
    assert_eq!(codes[g80], "G80", "{text}");
    assert_eq!(
        codes[g98 - 1],
        format!("X{:.3} Y{:.3}", first.0, first.1),
        "the rapid over the first hole at safe Z comes before G98\n{text}"
    );
    let last_z = (0..g98)
        .rev()
        .find(|&i| codes[i].split_whitespace().any(|w| w.starts_with('Z')))
        .unwrap();
    assert_eq!(codes[last_z], "G0 Z5.000", "at safe Z\n{text}");
    codes[g98 + 1..g80].to_vec()
}

/// The holes' centres in cut order: nearest neighbour from the first.
fn cut_order(entry: &EmitOp) -> Vec<(f64, f64)> {
    entry
        .holes
        .as_ref()
        .unwrap()
        .iter()
        .map(|h| (h.centre.x, h.centre.y))
        .collect()
}

/// Format 21 (#425, #427): a countersink is never a canned cycle, so
/// it is written as its plain moves whether or not the project asks
/// for cycles, with no `canned_cycle_expanded` warning (nothing was
/// expanded that could have been a cycle), and S18 passes it: one
/// plunge per hole to the derived depth, 10 / (2 · tan 45°) = 5 mm.
#[test]
fn countersink_is_written_as_moves_without_a_warning() {
    let (mut project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    project.tools[0] = dc_project::Tool {
        cutter: dc_project::ToolKind::VBit {
            included_angle_deg: 90.0,
        },
        diameter_mm: 12.7,
        ..project.tools[0].clone()
    };
    let op = dc_project::DrillOp {
        cycle: dc_project::DrillCycle::Countersink {
            diameter_mm: 10.0,
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false,
        },
        depth_mm: None,
        ..op
    };
    project.operations = vec![Operation::Drill(op.clone())];
    let ops = drill_entry(&project, &op);
    let holes = ops[0].holes.as_deref().unwrap_or_default();
    assert_eq!(holes.len(), HOLES.len());
    assert!(
        holes
            .iter()
            .all(|h| h.cycle == dc_cam::CannedKind::Expanded)
    );
    for post in [post_of(&project), canned_post(&project)] {
        let (program, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!has_cycle_word(&program.text), "{}", program.text);
        let plunges = codes(&program.text)
            .iter()
            .filter(|c| c.starts_with("G1 Z-5.000"))
            .count();
        assert_eq!(plunges, HOLES.len(), "{}", program.text);
    }
}

/// Port of `testCannedSimpleG81`: with canned cycles on, a simple Drill
/// is a rapid over the first hole at safe Z, `G98` once, one `G81` line
/// per hole with its `X Y Z R` (`Z` the bottom 5 mm below the stock-top
/// origin, `R` the retract plane 1 mm above it), `F` once, then `G80`,
/// and nothing else: no `G1`. The program passes the safety pass, which
/// expands each line (S11).
#[test]
fn drill_simple_writes_g81_per_hole_after_g98_and_ends_g80() {
    let (project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    let ops = drill_entry(&project, &op);
    let order = cut_order(&ops[0]);
    let (program, warnings) = emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let lines = cycle_lines(&program.text, order[0]);
    let expected: Vec<String> = order
        .iter()
        .enumerate()
        .map(|(k, (x, y))| {
            let head = if k == 0 { "G81 " } else { "" };
            let feed = if k == 0 { " F300.000" } else { "" };
            format!("{head}X{x:.3} Y{y:.3} Z-5.000 R1.000{feed}")
        })
        .collect();
    assert_eq!(lines, expected, "{}", program.text);
    assert!(
        !program.text.lines().any(|l| has_word(l, "G1")),
        "no plain cuts\n{}",
        program.text
    );
}

/// Port of `testCannedDwellG82`: a simple cycle with a dwell writes
/// `G82` with the dwell as `P`, in the definition's dwell units:
/// seconds on grblHAL, milliseconds where the definition says so.
#[test]
fn dwell_writes_g82_with_p() {
    let (project, op) = drill_project(dc_project::DrillCycle::Simple, 0.5, 1.0);
    let ops = drill_entry(&project, &op);
    let order = cut_order(&ops[0]);
    let post = canned_post(&project);
    let (program, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let lines = cycle_lines(&program.text, order[0]);
    let (x, y) = order[0];
    assert_eq!(
        lines[0],
        format!("G82 X{x:.3} Y{y:.3} Z-5.000 R1.000 P0.500 F300.000")
    );
    assert!(
        lines[1..].iter().all(|l| l.ends_with("P0.500")),
        "{lines:?}"
    );
    assert!(
        !program
            .text
            .lines()
            .any(|l| has_word(l, "G4") && has_word(l, "P0.500"))
    );

    let mut ms = post;
    ms.feed_speed.spindle_on_dwell.units = DwellUnits::Milliseconds;
    let (program, _) = emit_with(&project, &ms, &ops, DATE).unwrap();
    let lines = cycle_lines(&program.text, order[0]);
    assert!(lines.iter().all(|l| l.contains(" P500")), "{lines:?}");
}

/// Port of `testCannedPeckG83`: a peck with a full retract writes `G83`
/// with the peck as `Q`, here with `R` on the stock top (retract 0).
/// [`g83_written_with_nonzero_retract_when_definition_has_it`] covers
/// a retract plane above it (#153).
#[test]
fn peck_writes_g83_with_q() {
    let peck = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::Full,
    };
    let (project, op) = drill_project(peck, 0.0, 0.0);
    let ops = drill_entry(&project, &op);
    let order = cut_order(&ops[0]);
    let (program, warnings) = emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let lines = cycle_lines(&program.text, order[0]);
    let (x, y) = order[0];
    assert_eq!(
        lines[0],
        format!("G83 X{x:.3} Y{y:.3} Z-5.000 R0.000 Q2.000 F300.000")
    );
    assert_eq!(lines.len(), HOLES.len());
}

/// #153: the Drill toolpath pecks from the retract plane, as the
/// controller does, so a definition with peck cycles writes `G83` and
/// `G73` with any retract height: here `R1.000` with no warning, and
/// the program passes the safety pass, which expands each line (S11).
#[test]
fn g83_written_with_nonzero_retract_when_definition_has_it() {
    for (retract, word) in [
        (dc_project::PeckRetract::Full, "G83"),
        (dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 }, "G73"),
    ] {
        let peck = dc_project::DrillCycle::Peck {
            peck_depth_mm: Some(2.0),
            retract,
        };
        let (project, op) = drill_project(peck, 0.0, 1.0);
        let ops = drill_entry(&project, &op);
        let order = cut_order(&ops[0]);
        let (program, warnings) = emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
        assert!(warnings.is_empty(), "{word}: {warnings:?}");
        let lines = cycle_lines(&program.text, order[0]);
        let expected: Vec<String> = order
            .iter()
            .enumerate()
            .map(|(k, (x, y))| {
                let head = if k == 0 {
                    format!("{word} ")
                } else {
                    String::new()
                };
                let feed = if k == 0 { " F300.000" } else { "" };
                format!("{head}X{x:.3} Y{y:.3} Z-5.000 R1.000 Q2.000{feed}")
            })
            .collect();
        assert_eq!(lines, expected, "{}", program.text);
    }
}

/// Port of `testCannedChipBreakingG73`: a chip-break peck writes `G73`
/// with the peck as `Q`; the controller's short retract is the
/// definition's re-entry gap, the same 0.5 mm as the toolpath's gap.
#[test]
fn chip_break_writes_g73_with_q() {
    let chip = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 },
    };
    let (project, op) = drill_project(chip, 0.0, 0.0);
    let ops = drill_entry(&project, &op);
    let order = cut_order(&ops[0]);
    let (program, warnings) = emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let lines = cycle_lines(&program.text, order[0]);
    let (x, y) = order[0];
    assert_eq!(
        lines[0],
        format!("G73 X{x:.3} Y{y:.3} Z-5.000 R0.000 Q2.000 F300.000")
    );
}

/// Port of `testRegressionGrblNoCannedCycles`: GRBL and grblHAL have no
/// canned cycles, so a Drill is always `G0`/`G1` moves ending with
/// `M30`, with `use_canned_cycles` off (the default) or, in a project
/// built in code that validation would refuse, on; then with the
/// warning saying why.
#[test]
fn grbl_never_writes_canned_cycles() {
    for id in ["grbl", "grblhal"] {
        let (mut project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
        project.post_processor = PostProcessorRef::latest(id).unwrap();
        let ops = drill_entry(&project, &op);
        for on in [false, true] {
            project.post_processor.use_canned_cycles = on;
            let (program, warnings) = emit(&project, &ops, DATE).unwrap();
            let text = &program.text;
            assert!(!has_cycle_word(text), "{id} {on}\n{text}");
            assert!(text.lines().any(|l| has_word(l, "G0")), "{id}");
            assert!(text.lines().any(|l| has_word(l, "G1")), "{id}");
            assert_eq!(text.lines().last(), Some("M30"), "{id}");
            let expected = if on {
                vec![ExportWarning::CannedCycleExpanded {
                    op: op.id.clone(),
                    reason: CannedExpansion::NoCannedCycles,
                }]
            } else {
                Vec::new()
            };
            assert_eq!(warnings, expected, "{id} {on}");
        }
    }
}

/// Functional decision 2: canned cycles are off by default. With a
/// definition that has them and `use_canned_cycles` off, the program
/// is the plain one, with no warning; a new project reference has it
/// off.
#[test]
fn canned_cycles_off_by_default() {
    let (project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    assert!(!project.post_processor.use_canned_cycles);
    assert!(
        !PostProcessorRef::latest("grblhal")
            .unwrap()
            .use_canned_cycles
    );
    let ops = drill_entry(&project, &op);
    let mut off = canned_post(&project);
    off.use_canned_cycles = false;
    let (program, warnings) = emit_with(&project, &off, &ops, DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!has_cycle_word(&program.text), "{}", program.text);
    let (plain, _) = emit(&project, &ops, DATE).unwrap();
    assert_eq!(program.text, plain.text);
}

/// A Drill is written as cycles only if every hole can be: one hole
/// whose cycle the definition has no command for, or that has no
/// canned equivalent, writes the whole operation as moves, with one
/// warning naming the operation and the reason.
#[test]
fn mixed_cycles_expand_whole_operation_with_warning() {
    let (project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    let plain = emit(&project, &drill_entry(&project, &op), DATE)
        .unwrap()
        .0
        .text;
    let mut post = canned_post(&project);
    if let Some(cycles) = &mut post.canned_cycles {
        cycles.chip_break_drill_command = None;
    }
    for (kind, reason) in [
        (CannedKind::G73, CannedExpansion::NoCommand(Cycle::G73)),
        (CannedKind::Expanded, CannedExpansion::CycleNeedsMoves),
    ] {
        let mut ops = drill_entry(&project, &op);
        if let Some(holes) = &mut ops[0].holes {
            holes[1].cycle = kind;
        }
        let (program, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
        assert!(!has_cycle_word(&program.text), "{kind:?}\n{}", program.text);
        assert_eq!(program.text, plain, "{kind:?}: the plain program");
        assert_eq!(
            warnings,
            [ExportWarning::CannedCycleExpanded {
                op: op.id.clone(),
                reason,
            }],
            "{kind:?}"
        );
        assert_eq!(warnings[0].code(), "canned_cycle_expanded");
    }
}

/// A peck with a dwell has no canned cycle (`CannedKind::Expanded`): even
/// with canned cycles on it is written as moves, with `G4 P0.500` at the
/// bottom of every peck.
#[test]
fn expanded_dwell_writes_g4() {
    let peck = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::Full,
    };
    let (project, op) = drill_project(peck, 0.5, 0.0);
    let ops = drill_entry(&project, &op);
    let (program, warnings) = emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
    assert!(!has_cycle_word(&program.text), "{}", program.text);
    assert_eq!(
        warnings,
        [ExportWarning::CannedCycleExpanded {
            op: op.id.clone(),
            reason: CannedExpansion::CycleNeedsMoves,
        }]
    );
    // Three pecks (2, 2, 1 mm) per hole, each followed by its dwell.
    let moves = parse_motion(&program.text).unwrap();
    let blocks = parse_program(&program.text).unwrap();
    let dwells: Vec<usize> = blocks
        .iter()
        .filter(|b| code(program.text.lines().nth(b.line - 1).unwrap()) == "G4 P0.500")
        .map(|b| b.line)
        .collect();
    assert_eq!(dwells.len(), 3 * HOLES.len(), "{}", program.text);
    for line in dwells {
        let plunge = moves.iter().rev().find(|m| m.line < line).unwrap();
        assert_eq!(plunge.kind, MotionKind::Linear, "line {line}");
        assert!(
            plunge.to.z.unwrap() < plunge.from.z.unwrap(),
            "line {line}: a dwell follows a plunge"
        );
    }
}

/// Acceptance 24: the canned program's parsed motion, each cycle
/// expanded as the controller performs it, is the plain-moves
/// program's parsed motion, move for move: same kinds, same ends,
/// same feeds. Simple, a full-retract peck from the stock top, and
/// (#153) a full-retract and a chip-break peck with the retract plane
/// 1 mm above it.
#[test]
fn canned_program_motion_equals_plain_program() {
    let peck = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::Full,
    };
    let chip = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 },
    };
    for (cycle, retract_mm, word) in [
        (dc_project::DrillCycle::Simple, 1.0, "G81"),
        (peck.clone(), 0.0, "G83"),
        (peck, 1.0, "G83"),
        (chip, 1.0, "G73"),
    ] {
        let (project, op) = drill_project(cycle, 0.0, retract_mm);
        let ops = drill_entry(&project, &op);
        let post = canned_post(&project);
        let canned = emit_with(&project, &post, &ops, DATE).unwrap().0.text;
        assert!(canned.lines().any(|l| has_word(l, word)), "{canned}");
        let plain = emit(&project, &ops, DATE).unwrap().0.text;
        let model = crate::parse::CycleModel::of(&post);
        let expanded = crate::parse::parse_motion_with(&canned, &model).unwrap();
        let moves = parse_motion(&plain).unwrap();
        let shape = |m: &crate::MachineMove| {
            let feed = m.kind.is_cut().then_some(m.feed).flatten();
            (m.kind, m.to.x, m.to.y, m.to.z, feed)
        };
        let expanded: Vec<_> = expanded.iter().map(shape).collect();
        let moves: Vec<_> = moves.iter().map(shape).collect();
        assert_eq!(expanded, moves, "{word}\n{canned}\n{plain}");
    }
}

/// #325: a plunge rate the written places cannot hold, 1000 mm/min in
/// inches (39.370 08 in/min), is written rounded down, `F39.3700`, not
/// to the nearest, `F39.3701` (1000.0005 mm/min), in a canned cycle
/// and as plain moves alike; both pass S10, which allows nothing above
/// the plunge rate.
#[test]
fn plunge_feed_is_rounded_down_to_the_plunge_rate() {
    let (mut project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    project.display_units = Units::Inch;
    project.tools[0].plunge_mm_min = Some(1000.0);
    let ops = drill_entry(&project, &op);
    let canned = emit_with(&project, &canned_post(&project), &ops, DATE)
        .unwrap()
        .0
        .text;
    let plain = emit(&project, &ops, DATE).unwrap().0.text;
    for text in [canned, plain] {
        assert!(text.contains("F39.3700"), "{text}");
        assert!(!text.contains("F39.3701"), "{text}");
    }
}

/// #335: a last pass's inside arcs run at `feed · ρ / (ρ + r)`, which
/// in an 8 mm hole with the 6.35 mm bit is 206.25 mm/min, 8.120 08
/// in/min. It is written rounded down, `F8.1200`, not to the nearest,
/// `F8.1201` (206.2525 mm/min), so the cutting edge moves no faster
/// than the feed; the program passes S3 with it.
#[test]
fn last_pass_edge_feed_is_written_rounded_down() {
    let mut project = sample_project();
    project.display_units = Units::Inch;
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![dc_geometry::Polyline::new(
            vec![
                dc_geometry::Vertex::new(29.0, 15.0, 1.0),
                dc_geometry::Vertex::new(21.0, 15.0, 1.0),
            ],
            true,
        )];
    }
    let op = dc_project::test_support::profile_mut(&mut project);
    op.side = dc_project::Side::Inside;
    op.tabs = None;
    op.last_pass = Some(dc_project::LastPass {
        stock_to_leave_mm: 0.5,
        thickness_mm: None,
        reverse: false,
    });
    let ops = generated_ops(&project);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    assert!(text.contains("F8.1200"), "{text}");
    assert!(!text.contains("F8.1201"), "{text}");
}

// #283: the clearance, tool-change and park heights (design addendum
// A7, acceptance 45).

/// The repository's plaque: four operations with a 1/4" end mill, then
/// an Engrave with a 1/8" one, every safe Z 15 mm, on grblHAL.
fn plaque() -> Project {
    dc_project::load(include_bytes!("../../../../tests/fixtures/plaque.dcv"))
        .unwrap()
        .project
}

/// Every enabled operation of `project`, generated.
fn generated_ops(project: &Project) -> Vec<EmitOp> {
    project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .map(|op| {
            let tool = project
                .tools
                .iter()
                .find(|t| &t.id == op.tool())
                .unwrap()
                .clone();
            let op_owned = op.clone();
            match op {
                Operation::Profile(p) => {
                    (op_owned, tool, generate_profile(project, p).unwrap().0).into()
                }
                Operation::Pocket(p) => (
                    op_owned,
                    tool,
                    dc_cam::generate_pocket(project, p).unwrap().0,
                )
                    .into(),
                Operation::Engrave(p) => (
                    op_owned,
                    tool,
                    dc_cam::generate_engrave(project, p).unwrap().0,
                )
                    .into(),
                Operation::Drill(p) => EmitOp::drill(
                    op_owned,
                    tool,
                    dc_cam::generate_drill(project, p).unwrap().0,
                ),
                Operation::Bore(p) => {
                    (op_owned, tool, dc_cam::generate_bore(project, p).unwrap().0).into()
                }
                Operation::VCarve(_) => panic!("these tests use no V-Carve operations"),
                Operation::PhotoVCarve(_) => {
                    panic!("these tests use no Photo V-Carve operations")
                }
                // #436: an Inlay side's V-bit run.
                Operation::Inlay(p) => {
                    let (runs, _) = dc_cam::generate_inlay(project, p).unwrap();
                    let toolpath = runs
                        .into_iter()
                        .find(|r| r.label == RunLabel::Main)
                        .unwrap()
                        .toolpath;
                    (op_owned, tool, toolpath).into()
                }
            }
        })
        .collect()
}

/// Acceptance 45's heights: clearance 20 mm above the stock, machine Z
/// −5 mm, park at the project origin.
fn acceptance_45_heights() -> dc_project::Heights {
    dc_project::Heights {
        clearance_mm: Some(20.0),
        tool_change_machine_z_mm: Some(-5.0),
        park: Some(dc_project::ParkXy {
            x_mm: 0.0,
            y_mm: 0.0,
        }),
    }
}

/// The plaque with acceptance 45's heights, in one program with `M6`.
fn heights_plaque() -> String {
    let mut project = plaque();
    project.post_processor.tool_change_mode = ToolChangeMode::M6;
    project.heights = acceptance_45_heights();
    emit(&project, &generated_ops(&project), DATE)
        .unwrap()
        .0
        .text
}

/// The code of every line that has any, in order.
fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(code)
        .filter(|line| !line.is_empty())
        .collect()
}

/// Acceptance 45, design addendum A7: before the tool-change stop the
/// program rises to the clearance height in work coordinates, lifts to
/// the machine Z with `G53 G0` (grblHAL `gcode.c` 1543-1545, 3317-3325),
/// stops the spindle and changes the tool, with nothing between: no
/// drop to the next operation's safe Z (the #283 problem). After the
/// stop, Z is set again in work coordinates before any move in XY.
#[test]
fn tool_change_lifts_to_machine_z_before_m6() {
    let text = heights_plaque();
    let lines = code_lines(&text);
    // The plaque cuts with T2 first since its Bore (maintainer
    // decision 2026-10-24), so the one change is to T1.
    let change = lines.iter().position(|l| l.starts_with("T1 M6")).unwrap();
    let before: Vec<&str> = lines[change - 3..=change]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(before[1..], ["G53 G0 Z-5.000", "M5", "T1 M6"], "{text}");
    assert!(
        ["Z20.000", "G0 Z20.000"].contains(&before[0]),
        "the rise to the clearance height comes first: {before:?}"
    );
    let after = lines[change + 1..]
        .iter()
        .find(|l| {
            ["X", "Y", "Z"]
                .iter()
                .any(|a| l.split_whitespace().any(|w| w.starts_with(a)))
        })
        .unwrap();
    assert_eq!(after, "G0 Z20.000", "{text}");
    // One lift per stop, and one at the end.
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("G53")).count(),
        2,
        "{text}"
    );
    // S1 and S2 on the whole program: nothing below the material
    // bottom less the allowance, every XY rapid at or above safe Z.
    let moves = parse_motion(&text).unwrap();
    for mv in &moves {
        if let Some(z) = mv.to.z {
            assert!(z >= -12.0 - 0.2 - 1e-9, "line {}: {mv:?}", mv.line);
        }
    }
}

/// Acceptance 45, design addendum A7: after the last operation the bit
/// rises to the clearance height, rapids there to the park point
/// (0, 0), then lifts to the machine Z, then the footer: XY moves
/// before the lift, at a work Z the safety pass can check (S16).
#[test]
fn program_ends_at_park_then_machine_z() {
    let text = heights_plaque();
    let lines = code_lines(&text);
    let tail: Vec<&str> = lines[lines.len() - 4..]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        ["X0.000 Y0.000", "G53 G0 Z-5.000", "M5", "M30"],
        "{text}"
    );
    let moves = parse_motion(&text).unwrap();
    let park = moves.last().unwrap();
    assert_eq!(park.kind, MotionKind::Rapid);
    assert_approx_eq!(park.to.x.unwrap(), 0.0, 1e-9);
    assert_approx_eq!(park.to.y.unwrap(), 0.0, 1e-9);
    // At the clearance height.
    assert_approx_eq!(park.from.z.unwrap(), 20.0, 1e-9);
    assert_approx_eq!(park.to.z.unwrap(), 20.0, 1e-9);
}

/// Design addendum A7: the machine Z is written in the program's
/// units, as `G53` reads it (grblHAL `gcode.c` 2831 converts every
/// axis word of a G20 block to mm): −5 mm is −0.1969 in.
#[test]
fn machine_z_written_in_inch_units() {
    let mut project = sample_project();
    project.display_units = Units::Inch;
    project.heights.tool_change_machine_z_mm = Some(-5.0);
    let text = emit_text(&project);
    assert!(text.lines().any(|l| l == "G20"), "{text}");
    assert!(text.lines().any(|l| l == "G53 G0 Z-0.1969"), "{text}");
    project.display_units = Units::Mm;
    let text = emit_text(&project);
    assert!(text.lines().any(|l| l == "G53 G0 Z-5.000"), "{text}");
}

/// Design addendum A7: with all three heights unset, as every older
/// project migrates, the version 11 plaque exports the programs it did
/// before #283, byte for byte. The expected programs are the tabs-migration
/// corpus's, exported by the build before #105 and checked unchanged
/// since by `cli::version_9_tab_settings_export_byte_identical_programs`,
/// the T1 program since without the plaque's four holes, which its Drill
/// now skips (#377).
#[test]
fn unset_heights_are_byte_identical() {
    // The plaque as the build before #283 saved it, at version 11 on
    // grblHAL v2, from the heights-migration corpus.
    let project = dc_project::load(include_bytes!(
        "../../../dreamcarve/tests/fixtures/heights-migration/plaque.dcv"
    ))
    .unwrap()
    .project;
    assert_eq!(project.heights, dc_project::Heights::unset());
    let (programs, _) = emit_programs(
        &project,
        &generated_ops(&project),
        ProgramSplit::ProjectDefault,
        DATE,
    )
    .unwrap();
    let expected = [
        include_str!("../../../dreamcarve/tests/expected/tabs-migration/plaque-1-T1.nc"),
        include_str!("../../../dreamcarve/tests/expected/tabs-migration/plaque-2-T2.nc"),
    ];
    assert_eq!(programs.len(), expected.len());
    for (program, expected) in programs.iter().zip(expected) {
        assert!(program.text == expected, "{}", program.text);
        assert!(!program.text.contains("G53"));
    }
}

/// Design addendum A7: a program per tool run ends each program with
/// the park and the lift, which is the tool-change height on GRBL; a
/// definition without `G53` (Mach3, UCCNC, generic; #306) refuses the
/// machine Z, while the clearance height and the park need none.
#[test]
fn every_program_ends_with_the_lift_and_unchecked_definitions_refuse_it() {
    let mut project = plaque();
    project.heights = acceptance_45_heights();
    let (programs, _) = emit_programs(
        &project,
        &generated_ops(&project),
        ProgramSplit::ProjectDefault,
        DATE,
    )
    .unwrap();
    assert_eq!(programs.len(), 2);
    for program in &programs {
        let lines = code_lines(&program.text);
        let tail: Vec<&str> = lines[lines.len() - 4..]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(tail, ["X0.000 Y0.000", "G53 G0 Z-5.000", "M5", "M30"]);
    }
    for id in ["mach3", "uccnc", "generic"] {
        let mut project = project.clone();
        project.post_processor = PostProcessorRef::latest(id).unwrap();
        let err = emit_programs(
            &project,
            &generated_ops(&project),
            ProgramSplit::ProjectDefault,
            DATE,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ExportError::MachineZNeedsG53 { post } if post == id),
            "{id}: {err:?}"
        );
        project.heights.tool_change_machine_z_mm = None;
        let (programs, _) = emit_programs(
            &project,
            &generated_ops(&project),
            ProgramSplit::ProjectDefault,
            DATE,
        )
        .unwrap();
        assert!(programs.iter().all(|p| !p.text.contains("G53")), "{id}");
    }
}

/// #306: LinuxCNC documents `G53` (`interp_find.cc` 170-191: the
/// work, `G92` and tool length offsets are taken out of the Z word;
/// `interp_check.cc` 103-108: `G0` or `G1`, absolute), so on both of
/// its versions each program of a run per tool lifts to the machine Z
/// in the program's units, after the park, right before the footer
/// `M5`, `M2`, and nowhere else. `emit` runs the safety pass (S16), so
/// a program returned is one it accepted.
#[test]
fn machine_lift_on_linuxcnc() {
    for version in [1, 2] {
        for (units, park, lift, note) in [
            (Units::Mm, "X0.000 Y0.000", "G53 G0 Z-5.000", "-5.000 mm"),
            (
                Units::Inch,
                "X0.0000 Y0.0000",
                "G53 G0 Z-0.1969",
                "-0.1969 inch",
            ),
        ] {
            let mut project = project_with("linuxcnc");
            project.post_processor.definition_version = version;
            project.display_units = units;
            let ops = two_tool_ops(&mut project);
            // No `M6` on LinuxCNC: one program per tool run.
            project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
            project.heights = dc_project::Heights {
                clearance_mm: Some(10.0),
                tool_change_machine_z_mm: Some(-5.0),
                park: Some(dc_project::ParkXy {
                    x_mm: 0.0,
                    y_mm: 0.0,
                }),
            };
            let (programs, _) = emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE)
                .unwrap_or_else(|e| panic!("v{version} {units:?}: {e:?}"));
            assert_eq!(programs.len(), 2, "v{version} {units:?}");
            for program in &programs {
                let lines = code_lines(&program.text);
                let tail: Vec<&str> = lines[lines.len() - 4..]
                    .iter()
                    .map(String::as_str)
                    .collect();
                assert_eq!(
                    tail,
                    [park, lift, "M5", "M2"],
                    "v{version} {units:?}\n{}",
                    program.text
                );
                assert_eq!(
                    lines.iter().filter(|l| l.starts_with("G53")).count(),
                    1,
                    "{}",
                    program.text
                );
                assert_eq!(program.machine_lift.as_deref(), Some(note));
                assert!(
                    program
                        .text
                        .lines()
                        .any(|l| l == format!("( {} )", machine_lift_comment("linuxcnc"))),
                    "{}",
                    program.text
                );
            }
        }
    }
}

/// #306 with manual spindle control on LinuxCNC: the lift comes right
/// before the router-off pause `M0` at the end of each program, with
/// no retract in work coordinates between them, and the footer has no
/// `M5`.
#[test]
fn machine_lift_before_router_off_on_linuxcnc() {
    for units in [Units::Mm, Units::Inch] {
        let mut project = manual_project("linuxcnc");
        project.display_units = units;
        project.heights.tool_change_machine_z_mm = Some(-5.0);
        let text = emit_text(&project);
        let lines = code_lines(&text);
        let lift = lines
            .iter()
            .position(|l| l.starts_with("G53 G0 Z-"))
            .unwrap_or_else(|| panic!("{units:?}\n{text}"));
        assert_eq!(lines[lift + 1..], ["M0", "M2"], "{units:?}\n{text}");
    }
}

/// #306: LinuxCNC with canned cycles: the series of `G81` cycles is
/// cancelled with `G80` before the lift (`G53` needs `G0` or `G1`,
/// `interp_check.cc` 104-105, and the lift's `G0` sets it), and the
/// safety pass accepts the lift from the `G98` return height.
#[test]
fn machine_lift_after_canned_cycles_on_linuxcnc() {
    let (mut project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    project.post_processor = PostProcessorRef::latest("linuxcnc").unwrap();
    project.post_processor.use_canned_cycles = true;
    project.heights.tool_change_machine_z_mm = Some(-5.0);
    let (program, warnings) = emit(&project, &drill_entry(&project, &op), DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let lines = code_lines(&program.text);
    assert!(has_cycle_word(&program.text), "{}", program.text);
    let lift = lines.iter().position(|l| l == "G53 G0 Z-5.000").unwrap();
    let cancel = lines.iter().rposition(|l| l == "G80").unwrap();
    assert!(cancel < lift, "{}", program.text);
    assert_eq!(lines[lift + 1..], ["M5", "M2"], "{}", program.text);
}

/// #306, ADR-0004: a definition's `machine_command` writes nothing
/// unless the project sets the machine Z, so enabling it in place on
/// a released version changes no program that version could export
/// before. Every bundled version exports the sample project, in mm and
/// inch, with the same bytes whether its `machine_command` is set or
/// not.
#[test]
fn machine_command_writes_nothing_without_the_height() {
    for definition in dc_project::bundled_definitions().unwrap() {
        for units in [Units::Mm, Units::Inch] {
            let mut project = sample_project();
            project.post_processor = definition.default_ref();
            project.display_units = units;
            assert_eq!(project.heights, dc_project::Heights::unset());
            let post = post_of(&project);
            let mut without = post.clone();
            without.work_coordinates.machine_command = None;
            let text = emit_text(&project);
            assert!(!text.contains("G53"), "{}", definition.path);
            assert!(
                text == emit_text_with(&project, &without),
                "{} {units:?}",
                definition.path
            );
        }
    }
}

/// Design addendum A7, validation repeated at export: a clearance
/// below an operation's safe Z and a machine Z of 0 or more fail.
#[test]
fn export_refuses_bad_heights() {
    let mut project = sample_project();
    project.heights.clearance_mm = Some(3.0);
    let err = emit(&project, &ops_for(&project), DATE).unwrap_err();
    assert!(
        matches!(err, ExportError::ClearanceBelowSafeZ { .. }),
        "{err:?}"
    );
    // 0, and values that the program would write as 0: under half a
    // unit of its last place (0.0005 mm; 0.00127 mm in inches).
    for (units, machine_z_mm) in [
        (Units::Mm, 0.0),
        (Units::Mm, -0.0004),
        (Units::Inch, -0.001),
    ] {
        let mut project = sample_project();
        project.display_units = units;
        project.heights.tool_change_machine_z_mm = Some(machine_z_mm);
        let err = emit(&project, &ops_for(&project), DATE).unwrap_err();
        assert!(
            matches!(err, ExportError::MachineZNotBelowHome { .. }),
            "{units:?} {machine_z_mm}: {err:?}"
        );
    }
    for (units, machine_z_mm, written) in [
        (Units::Mm, -0.001, "G53 G0 Z-0.001"),
        (Units::Inch, -0.0013, "G53 G0 Z-0.0001"),
    ] {
        let mut project = sample_project();
        project.display_units = units;
        project.heights.tool_change_machine_z_mm = Some(machine_z_mm);
        let text = emit_text(&project);
        assert!(text.lines().any(|l| l == written), "{units:?}\n{text}");
    }
}

/// Design addendum A7 with manual spindle control: the lift comes
/// before the router-off pause, with no retract in work coordinates
/// between them. The user may jog at that pause, so the program lifts
/// again, after the setup lines, right before `T2 M6` (#304 review);
/// at the end the one lift is before the last router-off pause.
#[test]
fn manual_router_off_pause_follows_the_lift() {
    let mut project = project_with("grblhal");
    project.post_processor.spindle_control = SpindleControl::Manual;
    let ops = two_tool_ops(&mut project);
    project.heights.clearance_mm = Some(10.0);
    project.heights.tool_change_machine_z_mm = Some(-5.0);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines = code_lines(&text);
    let lifts: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with("G53"))
        .collect();
    assert_eq!(lifts.len(), 3, "{text}");
    for &i in [lifts[0], lifts[2]].iter() {
        assert_eq!(lines[i - 1], "Z10.000", "{text}");
        assert_eq!(lines[i + 1], "M0", "{text}");
    }
    let change = lines.iter().position(|l| l.starts_with("T2 M6")).unwrap();
    assert_eq!(
        lifts[1] + 1,
        change,
        "the second lift is right before the change\n{text}"
    );
    assert!(
        lines[lifts[0]..change]
            .iter()
            .all(|l| !l.contains('X') && !l.contains('Y') && !l.contains('Z')
                || l.starts_with("G53")),
        "{text}"
    );
}

/// #304 review: with manual spindle control, every tool change lifts
/// again after the router-off pause, right before its `T<n> M6`: three
/// operations with tools 1, 2, 1 change tools twice, and each change
/// has the lift before the pause and the one before the change.
#[test]
fn manual_relift_before_every_tool_change() {
    let mut project = project_with("grblhal");
    project.post_processor.spindle_control = SpindleControl::Manual;
    let mut ops = two_tool_ops(&mut project);
    let mut third = profile_op(&project);
    third.id = OperationId::new("o3");
    third.name = "Third".into();
    project.operations.push(Operation::Profile(third.clone()));
    let tool = project.tools[0].clone();
    let (toolpath, _) = generate_profile(&project, &third).unwrap();
    ops.push((Operation::Profile(third), tool, toolpath).into());
    project.heights.tool_change_machine_z_mm = Some(-5.0);
    let text = emit(&project, &ops, DATE).unwrap().0.text;
    let lines = code_lines(&text);
    let changes: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains(" M6"))
        .collect();
    assert_eq!(changes.len(), 2, "{text}");
    for &change in &changes {
        assert_eq!(lines[change - 1], "G53 G0 Z-5.000", "{text}");
        let pause = (0..change)
            .rev()
            .find(|&i| lines[i] == "M0")
            .unwrap_or_else(|| panic!("{text}"));
        assert_eq!(lines[pause - 1], "G53 G0 Z-5.000", "{text}");
    }
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("G53")).count(),
        5,
        "two per change and one at the end\n{text}"
    );
}

/// #308 on LinuxCNC (#306): `$20` is a GRBL setting, so the header names
/// LinuxCNC's own condition instead. Its soft limits always apply once
/// the machine is homed ("The axis must be homed before MIN_LIMIT is in
/// force", INI document; `motion/command.c` 1011-1025), and a program
/// does not run unhomed unless `[TRAJ] NO_FORCE_HOMING` is set
/// (`emctaskmain.cc` 121, 2302-2303; definitions README,
/// "Machine-coordinate moves"). In mm and inches, and no `$20` anywhere.
#[test]
fn machine_z_header_comment_on_linuxcnc() {
    let comment = "( Lifts to machine Z with G53: home the machine before running. \
                   Soft limits apply only once homed: leave NO_FORCE_HOMING unset )";
    for units in [Units::Mm, Units::Inch] {
        let mut project = project_with("linuxcnc");
        project.display_units = units;
        project.heights.tool_change_machine_z_mm = Some(-5.0);
        let text = emit_text(&project);
        assert_eq!(
            text.lines().filter(|l| *l == comment).count(),
            1,
            "{units:?}\n{text}"
        );
        assert!(!text.contains("$20"), "{units:?}\n{text}");
    }
}

/// #304 review, #308: with a tool-change machine Z, the header says
/// the program uses machine coordinates and needs a homed machine, and
/// names what keeps the lift from driving Z down on a machine that
/// homes Z downward: soft limits (`$20=1`), which alarm before the move
/// (GRBL `motion_control.c` 34-38, grblHAL `motion_control.c` 96-99;
/// definitions README, "Machine-coordinate moves"). In the definition's
/// comment syntax: `( )` on grblHAL, `;` on GRBL v3. Without the height
/// the header is as before (the goldens and the heights-migration
/// corpus prove the bytes).
#[test]
fn machine_z_header_comment() {
    let mut project = project_with("grblhal");
    let comment = "( Lifts to machine Z with G53: home the machine before running. \
                   Needs Z to home upward, or soft limits on with $20=1 )";
    let text = emit_text(&project);
    assert!(!text.contains("G53"), "{text}");
    assert!(!text.contains("home the machine"), "{text}");
    assert!(!text.contains("soft limits"), "{text}");
    project.heights.tool_change_machine_z_mm = Some(-5.0);
    let text = emit_text(&project);
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|l| *l == comment)
        .unwrap_or_else(|| panic!("{text}"));
    let first_motion = lines.iter().position(|l| l.starts_with("G0")).unwrap();
    assert!(at < first_motion, "in the header\n{text}");
    // In inches on GRBL v3, in mm on grblHAL, and in every program of a
    // run per tool.
    for (post, units, expected) in [
        (
            "grbl",
            Units::Inch,
            "; Lifts to machine Z with G53: home the machine before running. \
             Needs Z to home upward, or soft limits on with $20=1",
        ),
        ("grblhal", Units::Mm, comment),
    ] {
        let mut project = project_with(post);
        project.display_units = units;
        let ops = two_tool_ops(&mut project);
        project.heights.tool_change_machine_z_mm = Some(-5.0);
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE).unwrap();
        assert_eq!(programs.len(), 2);
        for program in &programs {
            assert!(
                program.text.lines().any(|l| l == expected),
                "{post} {units:?}\n{}",
                program.text
            );
            let named = program
                .text
                .lines()
                .filter(|l| l.contains("soft limits") && l.contains("$20=1"))
                .count();
            assert_eq!(named, 1, "{post} {units:?}\n{}", program.text);
        }
    }
    // The clearance height and the park point need no homing.
    project.heights.tool_change_machine_z_mm = None;
    project.heights.clearance_mm = Some(10.0);
    project.heights.park = Some(dc_project::ParkXy {
        x_mm: 0.0,
        y_mm: 0.0,
    });
    assert!(!emit_text(&project).contains(comment));
}

/// [`drill_project`] with a fourth target, `h4`: one part with two
/// 8 mm holes, at (20, 40) and (20, 50). Nearest neighbour from the
/// first hole, (20, 20), the Drill goes to (20, 40), then to (20, 50)
/// in the same part, so with a clearance height one link between holes
/// is at safe Z and the others are at the clearance height.
fn drill_project_with_two_hole_part() -> (Project, dc_project::DrillOp) {
    drill_project_with_two_hole_part_as(dc_project::DrillCycle::Simple, 0.0)
}

/// [`drill_project_with_two_hole_part`] drilled with `cycle` and
/// `dwell_s`.
fn drill_project_with_two_hole_part_as(
    cycle: dc_project::DrillCycle,
    dwell_s: f64,
) -> (Project, dc_project::DrillOp) {
    let (mut project, mut op) = drill_project(cycle, dwell_s, 1.0);
    let pair = dc_project::FeatureDefinition {
        id: dc_project::DefinitionId::new("d3"),
        name: "Pair".into(),
        profiles: vec![
            dc_geometry::Polyline::circle(Point::new(0.0, 0.0), 4.0),
            dc_geometry::Polyline::circle(Point::new(0.0, 10.0), 4.0),
        ],
    };
    project.definitions.insert(pair.id.clone(), pair);
    let id = dc_project::InstanceId::new("h4");
    project.instances.push(dc_project::FeatureInstance {
        id: id.clone(),
        definition: dc_project::DefinitionId::new("d3"),
        transform: dc_geometry::Transform::translate(20.0, 40.0),
        name: "Pair".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
    op.targets.push(id);
    project.operations = vec![Operation::Drill(op.clone())];
    (project, op)
}

/// #304 review, #305, #314: a Drill linked at a height that is neither
/// safe Z nor the clearance height follows no series of cycles, and is
/// written as moves with the `clearance_height` reason. The sequencer
/// never links holes so; here the first crossing of
/// [`drill_project_with_two_hole_part`] is moved down to Z 16, between
/// safe Z (11) and the clearance height (26). Without a clearance
/// height every link is at safe Z, and the Drill is written as cycles.
#[test]
fn clearance_height_expands_canned_cycles() {
    let (mut project, op) = drill_project_with_two_hole_part();
    let post = canned_post(&project);
    let (program, warnings) =
        emit_with(&project, &post, &drill_entry(&project, &op), DATE).unwrap();
    assert!(has_cycle_word(&program.text), "{}", program.text);
    assert!(warnings.is_empty(), "{warnings:?}");
    project.heights.clearance_mm = Some(20.0);
    let mut ops = drill_entry(&project, &op);
    let moves = &mut ops[0].toolpath.moves;
    let first_crossing = moves
        .windows(2)
        .position(|pair| pair[0].to.xy() != pair[1].to.xy())
        .unwrap();
    assert_approx_eq!(moves[first_crossing].to.z, 26.0, 1e-9);
    moves[first_crossing].to.z = 16.0;
    moves[first_crossing + 1].to.z = 16.0;
    let (program, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
    assert!(!has_cycle_word(&program.text), "{}", program.text);
    assert_eq!(
        warnings,
        [ExportWarning::CannedCycleExpanded {
            op: op.id.clone(),
            reason: CannedExpansion::ClearanceHeight,
        }]
    );
    assert_eq!(CannedExpansion::ClearanceHeight.code(), "clearance_height");
}

/// The parsed motion of `text`, canned cycles expanded with `post`'s
/// model, move for move, with nothing joined.
fn exact_motion(text: &str, post: &PostProcessor) -> Vec<MoveShape> {
    let model = crate::parse::CycleModel::of(post);
    crate::parse::parse_motion_with(text, &model)
        .unwrap()
        .iter()
        .map(|m| {
            let feed = m.kind.is_cut().then_some(m.feed).flatten();
            (m.kind, m.to.x, m.to.y, m.to.z, feed)
        })
        .collect()
}

/// The code lines of each canned-cycle series in `text`, from the line
/// after its `G98` to the line before its `G80`, with the code line
/// before the `G98` and the one after the `G80`. A `G80` before the
/// first `G98` is the header's (#279).
fn series_of(text: &str) -> Vec<(String, Vec<String>, Option<String>)> {
    // Without line numbers (Mach3).
    let lines: Vec<String> = code_lines(text)
        .into_iter()
        .map(|line| {
            let mut words = line.split_whitespace().peekable();
            words.next_if(|w| w.starts_with('N'));
            words.collect::<Vec<_>>().join(" ")
        })
        .collect();
    let mut series = Vec::new();
    let mut at = 0;
    while let Some(g98) = lines[at..].iter().position(|l| l == "G98") {
        let g98 = at + g98;
        let g80 = g98
            + lines[g98..]
                .iter()
                .position(|l| l == "G80")
                .unwrap_or_else(|| panic!("a G80 after line {g98}\n{text}"));
        series.push((
            lines[g98 - 1].clone(),
            lines[g98 + 1..g80].to_vec(),
            lines.get(g80 + 1).cloned(),
        ));
        at = g80 + 1;
    }
    series
}

/// #314: [`drill_project_with_two_hole_part`] with `clearance_mm: 20`
/// crosses between parts at the clearance height (Z 20) and between
/// the two holes of the pair at safe Z (Z 5). On the test definition,
/// with every cycle it has (`G81`, `G82`, `G83`, `G73`), it is written
/// with no warning as one `G98` series per part, each closed by `G80`:
/// the holes in cut order are (20, 20), the pair (20, 40) and
/// (20, 50), then (50, 70) and (80, 30), so four series of 1, 2, 1
/// and 1 holes. Each series starts after `Z5.000` over its first hole
/// (`G98` returns every hole of it to safe Z), and each `G80` but the
/// last is followed by the rise to the clearance height, `G0 Z20.000`,
/// on which `G80` makes `G0` necessary. The expansion (S11) crosses
/// between parts at Z 20 and between the pair's holes at Z 5; pecks
/// at the same depths as the plain program (Z 1, −3, −5 per hole);
/// nothing goes below the hole bottom, Z −5, or the material bottom,
/// Z −6; and the motion is the plain program's move for move, so also
/// once collinear rapids are joined.
#[test]
fn canned_series_per_part_at_the_clearance_height() {
    let peck = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::Full,
    };
    let chip = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 },
    };
    let order = [
        (20.0, 20.0),
        (20.0, 40.0),
        (20.0, 50.0),
        (50.0, 70.0),
        (80.0, 30.0),
    ];
    for (cycle, dwell_s, word) in [
        (dc_project::DrillCycle::Simple, 0.0, "G81"),
        (dc_project::DrillCycle::Simple, 0.5, "G82"),
        (peck, 0.0, "G83"),
        (chip, 0.0, "G73"),
    ] {
        let (mut project, op) = drill_project_with_two_hole_part_as(cycle, dwell_s);
        project.heights.clearance_mm = Some(20.0);
        let ops = drill_entry(&project, &op);
        assert_eq!(cut_order(&ops[0]), order, "{word}");
        let post = canned_post(&project);
        let (canned, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
        let text = &canned.text;
        assert!(warnings.is_empty(), "{word}: {warnings:?}\n{text}");

        let series = series_of(text);
        let sizes: Vec<usize> = series.iter().map(|s| s.1.len()).collect();
        assert_eq!(sizes, [1, 2, 1, 1], "{word}\n{text}");
        let mut hole = 0;
        for (k, (before, lines, after)) in series.iter().enumerate() {
            assert_eq!(before, "Z5.000", "{word}: series {k} from safe Z\n{text}");
            assert!(lines[0].starts_with(word), "{word}: series {k}\n{text}");
            for line in lines {
                let (x, y) = order[hole];
                assert!(
                    line.contains(&format!("X{x:.3} Y{y:.3} Z-5.000 R1.000")),
                    "{word}: {line}"
                );
                hole += 1;
            }
            let last = k + 1 == series.len();
            if !last {
                assert_eq!(after.as_deref(), Some("G0 Z20.000"), "{word}\n{text}");
            }
        }
        assert_eq!(hole, order.len());

        let model = crate::parse::CycleModel::of(&post);
        let moves = crate::parse::parse_motion_with(text, &model).unwrap();
        // The pair is the only part with two holes.
        let in_pair = |p: (f64, f64)| (p.0 - 20.0).abs() < 1e-9 && p.1 > 30.0;
        for (i, mv) in moves.iter().enumerate() {
            let z = mv.to.z.unwrap();
            assert!(
                z >= -5.0 - 1e-9,
                "{word}: move {i} below the bottom: {mv:?}"
            );
            assert!(z >= -6.0, "{word}: move {i} below the material: {mv:?}");
            if mv.from.x == mv.to.x && mv.from.y == mv.to.y {
                continue;
            }
            assert_eq!(mv.kind, MotionKind::Rapid, "{word}: move {i}: {mv:?}");
            let (Some(from_x), Some(from_y)) = (mv.from.x, mv.from.y) else {
                // Over the first hole, from where the user left the
                // bit: at the clearance height.
                assert_eq!((mv.from.z, mv.to.z), (Some(20.0), Some(20.0)), "{mv:?}");
                continue;
            };
            let from = (from_x, from_y);
            let to = (mv.to.x.unwrap(), mv.to.y.unwrap());
            let link_z = if in_pair(from) && in_pair(to) {
                5.0
            } else {
                20.0
            };
            assert_eq!(
                (mv.from.z, mv.to.z),
                (Some(link_z), Some(link_z)),
                "{word}: move {i} from {from:?} to {to:?}\n{text}"
            );
        }
        let feeds: Vec<f64> = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .filter_map(|m| m.to.z)
            .collect();
        let per_hole: &[f64] = if word == "G83" || word == "G73" {
            &[-1.0, -3.0, -5.0]
        } else {
            &[-5.0]
        };
        assert_eq!(feeds, per_hole.repeat(order.len()), "{word}\n{text}");

        let plain = emit(&project, &ops, DATE).unwrap().0.text;
        let plain_post = post_of(&project);
        assert!(!has_cycle_word(&plain), "{plain}");
        assert_eq!(
            exact_motion(text, &post),
            exact_motion(&plain, &plain_post),
            "{word}\n{text}\n{plain}"
        );
        assert_eq!(
            joined_motion(text, &post),
            joined_motion(&plain, &plain_post),
            "{word}\n{text}\n{plain}"
        );
    }
}

/// #314 on the bundled definitions with canned cycles, LinuxCNC and
/// Mach3, in mm and inch, simple and with a dwell: the Drill of
/// [`drill_project_with_two_hole_part`] with `clearance_mm: 20` is
/// four `G98` series with no warning, and `emit` ran the safety pass
/// on it. Between series, `G80` then `G0` up to the clearance height.
/// On LinuxCNC also with acceptance 45's lift and park.
#[test]
fn canned_series_per_part_on_linuxcnc_and_mach3() {
    for post_id in ["linuxcnc", "mach3"] {
        for (units, rise) in [(Units::Mm, "G0 Z20.000"), (Units::Inch, "G0 Z0.7874")] {
            for (dwell_s, word) in [(0.0, "G81"), (0.5, "G82")] {
                let (mut project, op) =
                    drill_project_with_two_hole_part_as(dc_project::DrillCycle::Simple, dwell_s);
                project.post_processor = PostProcessorRef::latest(post_id).unwrap();
                project.post_processor.use_canned_cycles = true;
                project.display_units = units;
                project.heights.clearance_mm = Some(20.0);
                let ops = drill_entry(&project, &op);
                let label = format!("{post_id} {units:?} {word}");
                let (program, warnings) =
                    emit(&project, &ops, DATE).unwrap_or_else(|e| panic!("{label}: {e:?}"));
                let text = &program.text;
                assert!(warnings.is_empty(), "{label}: {warnings:?}\n{text}");
                let series = series_of(text);
                let sizes: Vec<usize> = series.iter().map(|s| s.1.len()).collect();
                assert_eq!(sizes, [1, 2, 1, 1], "{label}\n{text}");
                for (_, lines, after) in &series[..series.len() - 1] {
                    assert!(lines[0].starts_with(word), "{label}\n{text}");
                    assert_eq!(after.as_deref(), Some(rise), "{label}\n{text}");
                }
                if post_id == "linuxcnc" {
                    // With #306's lift and the park, the program still
                    // passes, and lifts once, after the last `G80`.
                    project.heights = acceptance_45_heights();
                    let (program, warnings) = emit(&project, &ops, DATE)
                        .unwrap_or_else(|e| panic!("{label} lift: {e:?}"));
                    let text = &program.text;
                    assert!(warnings.is_empty(), "{label}: {warnings:?}\n{text}");
                    assert_eq!(series_of(text).len(), 4, "{label}\n{text}");
                    let lines = code_lines(text);
                    let lift = lines.iter().position(|l| l.starts_with("G53")).unwrap();
                    let cancel = lines.iter().rposition(|l| l == "G80").unwrap();
                    assert!(cancel < lift, "{label}\n{text}");
                    let lifts = lines.iter().filter(|l| l.starts_with("G53")).count();
                    assert_eq!(lifts, 1, "{label}\n{text}");
                }
            }
        }
    }
}

/// A parsed move's kind, end (X, Y, Z) and, for a cut, feed.
type MoveShape = (
    MotionKind,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// The parsed motion of `text`, canned cycles expanded with `post`'s
/// model, with each run of rapids in Z alone along one vertical line in
/// one direction joined into one rapid: the motion is the same, the
/// controller only does not stop between them (#305).
fn joined_motion(text: &str, post: &PostProcessor) -> Vec<MoveShape> {
    let model = crate::parse::CycleModel::of(post);
    let moves = crate::parse::parse_motion_with(text, &model).unwrap();
    let vertical = |m: &crate::MachineMove| {
        m.kind == MotionKind::Rapid && m.from.x == m.to.x && m.from.y == m.to.y
    };
    let rise = |m: &crate::MachineMove| match (m.from.z, m.to.z) {
        (Some(from), Some(to)) => Some(to > from),
        _ => None,
    };
    let mut joined: Vec<crate::MachineMove> = Vec::new();
    for mv in moves {
        if let Some(last) = joined.last_mut()
            && vertical(last)
            && vertical(&mv)
            && rise(last).is_some()
            && rise(last) == rise(&mv)
        {
            last.to = mv.to;
            continue;
        }
        joined.push(mv);
    }
    joined
        .iter()
        .map(|m| {
            let feed = m.kind.is_cut().then_some(m.feed).flatten();
            (m.kind, m.to.x, m.to.y, m.to.z, feed)
        })
        .collect()
}

/// The plaque's screw holes drilled, as the plaque drilled them before
/// its Bore (maintainer decision 2026-10-24): a Drill of the four 5 mm
/// circles with the 6.35 mm bit at the bit's size, alone, on LinuxCNC
/// with canned cycles on and a 20 mm clearance height, as a simple
/// cycle with `dwell_s`.
fn plaque_drill_on_linuxcnc(dwell_s: f64) -> (Project, Vec<EmitOp>) {
    let mut project = plaque();
    project.post_processor = PostProcessorRef::latest("linuxcnc").unwrap();
    project.post_processor.use_canned_cycles = true;
    project.heights.clearance_mm = Some(20.0);
    for op in &mut project.operations {
        if let Operation::Bore(bore) = op {
            *op = Operation::Drill(dc_project::DrillOp {
                id: bore.id.clone(),
                name: bore.name.clone(),
                enabled: true,
                targets: bore.targets.clone(),
                tool: dc_project::ToolId::new("t2"),
                cycle: dc_project::DrillCycle::Simple,
                dwell_s,
                depth_mm: bore.depth_mm,
                retract_mm: 1.0,
                through_cut_allowance_mm: bore.through_cut_allowance_mm,
                safe_z_mm: bore.safe_z_mm,
                allow_tool_wider_than_hole: true,
            });
        }
    }
    let ops = generated_ops(&project)
        .into_iter()
        .filter(|entry| matches!(entry.op, Operation::Drill(_)))
        .collect();
    (project, ops)
}

/// #305: the plaque's four screw holes are four parts, linked at the
/// clearance height. On LinuxCNC with `clearance_mm: 20` and canned
/// cycles on, the Drill is a rapid over the first hole at the
/// clearance height, `G98` once, one `G81` (or `G82 … P`) line per
/// hole, `G80`, and no warning. `G98` returns each hole to the Z
/// before the first cycle of the series ([G] `#gcode:g98-g99`;
/// `interp_cycles.cc` 926-931, 976), here Z 20, so the controller
/// crosses between holes at the clearance height. The expansion
/// (S11) plunges each hole from `R` to the bottom (Z −12, the stock
/// bottom less the 0.2 mm through-cut allowance), goes back to Z 20 after each,
/// never lower, and is the plain program's motion once its rapids
/// along one vertical line are joined.
#[test]
fn canned_cycles_at_the_clearance_height() {
    for (dwell_s, word) in [(0.0, "G81"), (0.5, "G82")] {
        let (project, ops) = plaque_drill_on_linuxcnc(dwell_s);
        let post = post_of(&project);
        let (program, warnings) = emit(&project, &ops, DATE).unwrap();
        let text = &program.text;
        assert!(warnings.is_empty(), "{word}: {warnings:?}\n{text}");
        let codes = code_lines(text);
        let g98 = codes
            .iter()
            .position(|l| l == "G98")
            .unwrap_or_else(|| panic!("{word}: no G98\n{text}"));
        assert_eq!(codes.iter().filter(|l| has_word(l, "G98")).count(), 1);
        let before_z = codes[..g98]
            .iter()
            .rev()
            .find(|l| l.split_whitespace().any(|w| w.starts_with('Z')))
            .unwrap();
        assert_eq!(before_z, "G0 Z20.000", "{word}: the initial Z\n{text}");
        let holes: Vec<&String> = codes[g98 + 1..]
            .iter()
            .take_while(|l| *l != "G80")
            .collect();
        assert_eq!(holes.len(), 4, "{word}\n{text}");
        assert!(holes[0].starts_with(word), "{word}\n{text}");
        for hole in &holes {
            assert!(hole.contains(" Z-12.000 R1.000"), "{word}: {hole}");
            assert_eq!(hole.contains(" P0.500"), dwell_s > 0.0, "{word}: {hole}");
        }

        let model = crate::parse::CycleModel::of(&post);
        let moves = crate::parse::parse_motion_with(text, &model).unwrap();
        let bottom = -12.0;
        let mut returns = 0;
        for (i, mv) in moves.iter().enumerate() {
            let z = mv.to.z.unwrap();
            assert!(
                z >= bottom - 1e-9,
                "{word}: move {i} below the bottom: {mv:?}"
            );
            let across = mv.from.x != mv.to.x || mv.from.y != mv.to.y;
            if across {
                assert_eq!(mv.kind, MotionKind::Rapid, "{word}: move {i}: {mv:?}");
                assert!(
                    (mv.from.z.unwrap() - 20.0).abs() < 1e-9 && (z - 20.0).abs() < 1e-9,
                    "{word}: move {i} crosses below the clearance height: {mv:?}"
                );
            }
            if mv.kind == MotionKind::Linear {
                assert!((z - bottom).abs() < 1e-9, "{word}: move {i}: {mv:?}");
                assert_eq!(mv.feed, ops[0].tool.plunge_mm_min, "{word}: {mv:?}");
                let out = &moves[i + 1];
                assert_eq!(out.kind, MotionKind::Rapid, "{word}: {out:?}");
                assert_eq!(out.to.z, Some(20.0), "{word}: back to Z 20: {out:?}");
                returns += 1;
            }
        }
        assert_eq!(returns, 4, "{word}\n{text}");

        let mut plain_post = post.clone();
        plain_post.use_canned_cycles = false;
        let (plain, _) = emit_with(&project, &plain_post, &ops, DATE).unwrap();
        assert_eq!(
            joined_motion(text, &post),
            joined_motion(&plain.text, &plain_post),
            "{word}\n{text}\n{}",
            plain.text
        );
    }
}

/// #305 with #306: on LinuxCNC, cycles from the clearance height and
/// the `G53` lift in one program. With acceptance 45's heights (machine
/// Z −5, park at the origin), automatic and manual spindle control, mm
/// and inch, the plaque's Drill is still written as cycles with no
/// warning, the program passes the safety pass (`emit` runs it), and
/// after `G80` it parks at the clearance height, lifts once and ends:
/// `G0 X0 Y0` (with its `G0`: `G80` leaves no motion mode),
/// `G53 G0 Z…`, then `M5`, `M2` (or the router-off `M0`). No
/// move comes between the cycles' return to the clearance height and
/// the park, so the lift is from at least safe Z (S16).
#[test]
fn canned_cycles_from_the_clearance_height_with_the_lift_on_linuxcnc() {
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        for (units, z20, lift, park) in [
            (Units::Mm, "Z20.000", "G53 G0 Z-5.000", "G0 X0.000 Y0.000"),
            (
                Units::Inch,
                "Z0.7874",
                "G53 G0 Z-0.1969",
                "G0 X0.0000 Y0.0000",
            ),
        ] {
            let (mut project, ops) = plaque_drill_on_linuxcnc(0.0);
            project.post_processor.spindle_control = control;
            project.display_units = units;
            project.heights = acceptance_45_heights();
            let (program, warnings) = emit(&project, &ops, DATE)
                .unwrap_or_else(|e| panic!("{control:?} {units:?}: {e:?}"));
            let text = &program.text;
            assert!(
                warnings.is_empty(),
                "{control:?} {units:?}: {warnings:?}\n{text}"
            );
            let lines = code_lines(text);
            let g98 = lines.iter().position(|l| l == "G98").unwrap();
            assert!(
                lines[g98 - 1].starts_with('X') && lines[..g98].iter().any(|l| l.ends_with(z20)),
                "{control:?} {units:?}: over the first hole at the clearance height\n{text}"
            );
            let cancel = lines.iter().position(|l| l == "G80").unwrap();
            assert_eq!(lines[g98 + 1..cancel].len(), 4, "{text}");
            let at = lines.iter().position(|l| l == lift).unwrap();
            assert_eq!(
                lines.iter().filter(|l| l.starts_with("G53")).count(),
                1,
                "{text}"
            );
            assert_eq!(
                lines[cancel + 1..at],
                [park],
                "{control:?} {units:?}\n{text}"
            );
            let end: &[&str] = match control {
                SpindleControl::Automatic => &["M5", "M2"],
                SpindleControl::Manual => &["M2"],
            };
            let tail: Vec<&str> = lines[at + 1..]
                .iter()
                .map(String::as_str)
                .filter(|l| !l.starts_with("M0"))
                .collect();
            assert_eq!(tail, end, "{control:?} {units:?}\n{text}");
        }
    }
}

/// #305: a clearance height equal to the Drill's safe Z (validation
/// allows it) links the parts at safe Z. The cycles start from it, as
/// without a clearance height, with no warning, and the program is the
/// one without the clearance height.
#[test]
fn canned_cycles_with_the_clearance_height_at_safe_z() {
    let (mut project, op) = drill_project(dc_project::DrillCycle::Simple, 0.0, 1.0);
    let post = canned_post(&project);
    let without = emit_with(&project, &post, &drill_entry(&project, &op), DATE)
        .unwrap()
        .0
        .text;
    project.heights.clearance_mm = Some(op.safe_z_mm);
    let (program, warnings) =
        emit_with(&project, &post, &drill_entry(&project, &op), DATE).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}\n{}", program.text);
    assert!(has_cycle_word(&program.text), "{}", program.text);
    assert_eq!(program.text, without);
}

/// #305, acceptance 24 with a clearance height: holes that are separate
/// parts, with every cycle the test definition has (`G81`, `G82`,
/// `G83`, `G73`), are written as cycles from the clearance height with
/// no warning, and the expansion is the plain program's motion once
/// its rapids along one vertical line are joined: the same pecks from
/// `R` (Z 1, then 2 mm a peck to Z −5), the same re-entries, the same
/// dwell. Every move across the work is at the clearance height, Z 20,
/// and nothing goes below the hole bottom, Z −5.
#[test]
fn canned_motion_from_the_clearance_height_equals_plain_program() {
    let peck = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::Full,
    };
    let chip = dc_project::DrillCycle::Peck {
        peck_depth_mm: Some(2.0),
        retract: dc_project::PeckRetract::ChipBreak { gap_mm: 0.5 },
    };
    for (cycle, dwell_s, word) in [
        (dc_project::DrillCycle::Simple, 0.0, "G81"),
        (dc_project::DrillCycle::Simple, 0.5, "G82"),
        (peck, 0.0, "G83"),
        (chip, 0.0, "G73"),
    ] {
        let (mut project, op) = drill_project(cycle, dwell_s, 1.0);
        project.heights.clearance_mm = Some(20.0);
        let ops = drill_entry(&project, &op);
        let post = canned_post(&project);
        let (canned, warnings) = emit_with(&project, &post, &ops, DATE).unwrap();
        let canned = canned.text;
        assert!(warnings.is_empty(), "{word}: {warnings:?}\n{canned}");
        assert!(canned.lines().any(|l| has_word(l, word)), "{canned}");
        let plain = emit(&project, &ops, DATE).unwrap().0.text;
        let expanded = joined_motion(&canned, &post);
        assert_eq!(
            expanded,
            joined_motion(&plain, &post_of(&project)),
            "{word}\n{canned}\n{plain}"
        );
        let feeds: Vec<f64> = expanded
            .iter()
            .filter(|m| m.0 == MotionKind::Linear)
            .filter_map(|m| m.3)
            .collect();
        let per_hole: &[f64] = if word == "G83" || word == "G73" {
            &[-1.0, -3.0, -5.0]
        } else {
            &[-5.0]
        };
        assert_eq!(feeds, per_hole.repeat(HOLES.len()), "{word}\n{canned}");
        let model = crate::parse::CycleModel::of(&post);
        let moves = crate::parse::parse_motion_with(&canned, &model).unwrap();
        for (i, mv) in moves.iter().enumerate() {
            assert!(mv.to.z.unwrap() >= -5.0 - 1e-9, "{word}: move {i}: {mv:?}");
            if mv.from.x != mv.to.x || mv.from.y != mv.to.y {
                assert_eq!(mv.to.z, Some(20.0), "{word}: move {i}: {mv:?}");
            }
        }
    }
}
