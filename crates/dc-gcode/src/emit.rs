// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/postprocessor.ts

//! G-code emission (`docs/specs/phase-1/design.md`, "Algorithms →
//! Emission").

use dc_cam::Toolpath;
use dc_project::{OperationId, ProfileOp, Project, Tool};

use crate::ExportError;

/// An emitted G-code program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// The program text, one block per line, `\n` line endings, ending in
    /// a newline.
    pub text: String,
    /// File extension without the dot, from the post-processor.
    pub extension: String,
}

/// Something the user should know about an export that still yields a safe
/// program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportWarning {
    /// A disabled operation was left out of the program.
    DisabledOperationSkipped {
        /// The operation.
        op: OperationId,
    },
    /// Some arcs were written as short straight moves because, once their
    /// numbers were rounded, the controller would have rejected them.
    ArcsFlattened {
        /// The operation.
        op: OperationId,
        /// How many arcs.
        count: usize,
    },
}

/// Emits a program for the enabled operations in `ops`, each with its tool
/// and its toolpath (project space, mm), using the project's embedded
/// post-processor, machine origin, and display units. `date` fills the
/// `{date}` placeholder; the caller supplies it so output is
/// deterministic.
///
/// The emitted text is parsed back and checked against safety rules S1–S6
/// before it is returned.
///
/// # Errors
///
/// A missing feed, plunge rate, or spindle speed names the tool and field;
/// any safety violation in the emitted program is an error, never a
/// warning. See [`ExportError`].
pub fn emit(
    project: &Project,
    ops: &[(ProfileOp, Tool, Toolpath)],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    let _ = (project, ops, date);
    Err(ExportError::NoEnabledOperations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{MotionKind, parse_motion, parse_program};
    use crate::post::lookup;
    use crate::safety::MAX_LINE_CHARS;
    use dc_cam::{FeedField, Move, MoveKind, generate_profile};
    use dc_geometry::{Point, Point3, assert_approx_eq};
    use dc_project::test_support::{sample_project, tool_6_35_upcut};
    use dc_project::{Operation, OperationId, ToolId, Units};

    const DATE: &str = "2026-10-15";

    fn profile_op(project: &Project) -> ProfileOp {
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        op
    }

    /// The sample project with the bundled post-processor `post_id`.
    fn project_with(post_id: &str) -> Project {
        let mut project = sample_project();
        project.post_processor = lookup(post_id).unwrap();
        project
    }

    /// The sample project's operation, tool, and generated toolpath.
    fn ops_for(project: &Project) -> Vec<(ProfileOp, Tool, Toolpath)> {
        let op = profile_op(project);
        let tool = project
            .tools
            .iter()
            .find(|t| t.id == op.tool)
            .unwrap()
            .clone();
        let (toolpath, _) = generate_profile(project, &op).unwrap();
        dc_cam::test_support::assert_safe(&toolpath, project, &op);
        vec![(op, tool, toolpath)]
    }

    fn emit_text(project: &Project) -> String {
        emit(project, &ops_for(project), DATE).unwrap().0.text
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

    #[test]
    fn missing_feed_is_error_naming_tool_and_field() {
        let mut project = sample_project();
        let mut ops = ops_for(&project);
        project.tools[0].feed_mm_min = None;
        ops[0].1.feed_mm_min = None;
        let err = emit(&project, &ops, DATE).unwrap_err();
        assert!(
            matches!(&err, ExportError::MissingFeed { tool, field: FeedField::Feed } if tool == "6.35 mm upcut"),
            "{err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("6.35 mm upcut") && message.contains("feed_mm_min"),
            "{message}"
        );

        // A placeholder 0 is as missing as `None`.
        ops[0].1.feed_mm_min = Some(1000.0);
        ops[0].1.plunge_mm_min = Some(0.0);
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

    #[test]
    fn missing_rpm_is_error() {
        let project = sample_project();
        let mut ops = ops_for(&project);
        for rpm in [None, Some(0)] {
            ops[0].1.rpm = rpm;
            let err = emit(&project, &ops, DATE).unwrap_err();
            assert!(
                matches!(&err, ExportError::MissingRpm { tool } if tool == "6.35 mm upcut"),
                "{err:?}"
            );
        }
    }

    #[test]
    fn no_enabled_operation_is_an_error() {
        let project = sample_project();
        let mut ops = ops_for(&project);
        ops[0].0.enabled = false;
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
                    c.is_empty() || ["G90", "G17", "G94"].contains(&c.as_str()),
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
        assert!(project.post_processor.motion.arc_interpolation);
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

    #[test]
    fn arcs_emitted_with_r_when_the_definition_says_so() {
        let mut project = project_with("grblhal");
        project.post_processor.motion.arc_format = dc_project::ArcFormat::R;
        let text = emit_text(&project);
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
        let mut project = project_with("grblhal");
        project.post_processor.motion.arc_interpolation = false;
        let text = emit_text(&project);
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
            .2
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

    #[test]
    fn comment_text_is_sanitized() {
        let nasty = "Bad!~?(name);\nnext\u{2014}line\u{18}";
        for post in ["grblhal", "grbl"] {
            let mut project = project_with(post);
            project.name = nasty.into();
            project.tools[0].name = nasty.into();
            let mut ops = ops_for(&project);
            ops[0].0.name = nasty.into();
            ops[0].1.name = nasty.into();
            let text = emit(&project, &ops, DATE).unwrap().0.text;
            for line in text.lines() {
                assert!(
                    line.chars().all(|c| (' '..='~').contains(&c)),
                    "{post}: non-printable in {line:?}"
                );
                assert!(!line.contains(['!', '~', '?']), "{post}: {line:?}");
            }
            let prefix = &project.post_processor.program.comment_prefix;
            let suffix = &project.post_processor.program.comment_suffix;
            let named: Vec<&str> = text.lines().filter(|l| l.contains("Bad")).collect();
            assert!(
                named.len() >= 3,
                "{post}: program, operation, and tool names appear"
            );
            for line in named {
                // The text inside the comment has no comment delimiters.
                let start = line.find(prefix.as_str()).unwrap() + prefix.len();
                let inner = if suffix.is_empty() {
                    &line[start..]
                } else {
                    &line[start..line.rfind(suffix.as_str()).unwrap()]
                };
                assert!(!inner.contains(['(', ')', ';']), "{post}: {line:?}");
                assert!(inner.contains("Bad name next line"), "{post}: {line:?}");
            }
        }
    }

    #[test]
    fn comment_text_is_truncated() {
        for post in ["grblhal", "grbl"] {
            let mut project = project_with(post);
            project.name = "P".repeat(1000);
            let mut ops = ops_for(&project);
            ops[0].0.name = "O".repeat(1000);
            ops[0].1.name = "T".repeat(1000);
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
        assert_eq!(project.post_processor.feed_speed.spindle_on_ccw, "M4");
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
        let project = project_with("grblhal");
        let mut ops = ops_for(&project);
        // A tiny coordinate and a huge-but-safe one.
        ops[0].2 = Toolpath {
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
        // The start rounds from 10.0004 to 10.000. I taken from the
        // unrounded start would be 5.0004 -> "5.000"; from the emitted
        // start it is 5.0008 -> "5.001".
        let project = project_with("grblhal");
        let mut ops = ops_for(&project);
        let start = Point3::new(10.0004, 20.0, 5.0);
        let centre = Point::new(15.0008, 20.0);
        let r = centre.x - start.x;
        ops[0].2 = Toolpath {
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

    /// The sample operation, then the same with a second tool.
    fn two_tool_ops(project: &mut Project) -> Vec<(ProfileOp, Tool, Toolpath)> {
        project.tools.push(second_tool());
        let mut second = profile_op(project);
        second.id = OperationId::new("o2");
        second.name = "Second".into();
        second.tool = ToolId::new("t2");
        project.operations.push(Operation::Profile(second.clone()));
        let mut ops = ops_for(project);
        let (toolpath, _) = generate_profile(project, &second).unwrap();
        ops.push((second, second_tool(), toolpath));
        ops
    }

    #[test]
    fn dwell_follows_every_spindle_start() {
        for post in ["grblhal", "grbl"] {
            let mut project = project_with(post);
            let ops = two_tool_ops(&mut project);
            let text = emit(&project, &ops, DATE).unwrap().0.text;
            let lines: Vec<&str> = text.lines().collect();
            let starts: Vec<usize> = (0..lines.len())
                .filter(|&i| has_word(lines[i], "M3"))
                .collect();
            assert_eq!(starts.len(), 2, "{post}: program start and the tool change");
            assert_eq!(code(lines[starts[1]]), "M3 S12000");
            for &i in &starts {
                assert_eq!(lines[i + 1], "G4 P3.000", "{post}");
                let next_cut = (i..lines.len()).find(|&k| is_cut(lines[k])).unwrap();
                assert!(next_cut > i + 1, "{post}");
            }
            // The tool change stops the spindle and pauses before the
            // second start.
            let stop = (starts[0]..starts[1]).find(|&k| has_word(lines[k], "M5"));
            let pause = (starts[0]..starts[1]).find(|&k| has_word(lines[k], "M0"));
            assert!(
                matches!((stop, pause), (Some(s), Some(p)) if s < p),
                "{post}"
            );
        }
    }

    #[test]
    fn zero_dwell_emits_nothing() {
        let mut project = project_with("grblhal");
        project
            .post_processor
            .feed_speed
            .spindle_on_dwell
            .duration_s = 0.0;
        let ops = two_tool_ops(&mut project);
        let text = emit(&project, &ops, DATE).unwrap().0.text;
        assert!(!text.lines().any(|l| has_word(l, "G4")), "{text}");
        assert_eq!(text.lines().filter(|l| has_word(l, "M3")).count(), 2);
    }

    #[test]
    fn header_placeholders_are_filled() {
        let project = project_with("grblhal");
        let text = emit_text(&project);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "( Sample )");
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
        let mut project = project_with("grblhal");
        project.post_processor.program.line_numbers = true;
        let text = emit_text(&project);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("N10 "), "{}", lines[0]);
        assert!(lines[1].starts_with("N20 "), "{}", lines[1]);
    }
}
