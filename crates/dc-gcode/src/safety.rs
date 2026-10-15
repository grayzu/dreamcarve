// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The safety pass: parse the emitted program and check the phase 1 safety
//! rules on what the controller will read (`docs/specs/phase-1/design.md`,
//! "Emission", step 7; `docs/specs/phase-1/functional.md`, S1–S6). Every
//! violation is an [`ExportError`]; there are no safety warnings.
//!
//! Checks run in project space: each parsed point is mapped back through
//! the [`MachineTransform`] using the units the program itself selected, so
//! a wrong origin, a mirror, or a missing units command shows up as a
//! violation instead of being trusted.

use std::ops::Range;

use dc_cam::Toolpath;
use dc_geometry::{EPS_MM, Point3, approx_eq};
use dc_project::{PostProcessor, ProfileOp, Tool, Units};

use crate::ExportError;
use crate::format::decimal_places;
use crate::parse::{Block, MachineMove, MotionKind, parse_program};
use crate::transform::{MachineTransform, mm_per_unit};

/// The longest line, in characters, DreamCarve emits. grblHAL's line
/// buffer holds 256 bytes including the terminator.
pub const MAX_LINE_CHARS: usize = 255;

/// One emitted operation and the program lines its moves occupy.
#[derive(Debug, Clone)]
pub(crate) struct OpSpan<'a> {
    /// The operation.
    pub op: &'a ProfileOp,
    /// Its tool.
    pub tool: &'a Tool,
    /// Its toolpath, in project space.
    pub toolpath: &'a Toolpath,
    /// 1-based program lines, end exclusive, from the operation's first
    /// motion line to its last.
    pub lines: Range<usize>,
}

/// Checks `text`, emitted with `post` and `transform` for the operations
/// in `spans`, against S1–S6.
///
/// # Errors
///
/// The first violation found, as the matching [`ExportError`].
pub(crate) fn check(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock_thickness_mm: f64,
    spans: &[OpSpan<'_>],
) -> Result<(), ExportError> {
    check_characters(text)?;
    let blocks = parse_program(text)?;
    check_words(&blocks)?;

    let dwell_s = post.feed_speed.spindle_on_dwell.duration_s;
    let mut units_set = false;
    let mut wcs_set = false;
    let mut spindle_on = false;
    let mut rpm: Option<f64> = None;
    let mut dwell_pending = false;
    for block in &blocks {
        let line = block.line;
        if block.has('G', 20.0) || block.has('G', 21.0) {
            units_set = true;
        }
        if WCS_CODES.iter().any(|&code| block.has('G', code)) {
            wcs_set = true;
        }
        let starts_spindle = block.has('M', 3.0);
        // S5: units and work coordinates before anything happens.
        if block.motion.is_some() || starts_spindle {
            if !units_set {
                return Err(ExportError::MissingSetup {
                    what: "units (G20 or G21)",
                });
            }
            if !wcs_set {
                return Err(ExportError::MissingSetup {
                    what: "a work coordinate system (G54 to G59)",
                });
            }
        }
        // S4: spindle state. DreamCarve never puts a spindle word and a
        // move on one line, so the order within a line does not matter.
        if let Some(s) = block.value('S') {
            rpm = Some(s);
        }
        if starts_spindle {
            spindle_on = true;
            dwell_pending = dwell_s > 0.0;
        }
        if block.has('G', 4.0)
            && block
                .value('P')
                .is_some_and(|p| p >= dwell_s - DWELL_TOLERANCE_S)
        {
            dwell_pending = false;
        }
        if block.has('M', 5.0) {
            spindle_on = false;
        }

        let Some(mv) = &block.motion else {
            continue;
        };
        let limits = Limits::for_line(line, spans, stock_thickness_mm);
        let tolerance_mm = quantum_mm(post, mv.units);
        check_depth(mv, &limits, transform, tolerance_mm)?;
        if mv.kind.is_cut() {
            if !spindle_on || rpm.is_none_or(|s| s <= 0.0) {
                return Err(ExportError::SpindleNotRunning { line });
            }
            if dwell_pending {
                return Err(ExportError::SpindleNotUpToSpeed { line });
            }
            check_feed(mv, &limits, 2.0 * tolerance_mm)?;
            check_arc(mv)?;
        } else {
            check_rapid(mv, &limits, transform, tolerance_mm)?;
        }
    }
    if spindle_on {
        return Err(ExportError::SpindleNotStopped);
    }

    // S6: reading the program back reproduces each toolpath.
    for span in spans {
        check_reproduces(span, &blocks, post, transform)?;
    }
    Ok(())
}

/// `G54` to `G59`.
const WCS_CODES: [f64; 6] = [54.0, 55.0, 56.0, 57.0, 58.0, 59.0];

/// Words DreamCarve never sends to GRBL or grblHAL (`definitions/README.md`,
/// "Verification"): `M4` needs a direction pin or laser mode, `M6` needs a
/// tool-change protocol the sender may not speak, `M7` is optional on both,
/// `G53`, `G28`, and `G30` move in machine coordinates, and `G91` and `G93`
/// change what every later number means.
const FORBIDDEN: [(char, f64); 8] = [
    ('M', 4.0),
    ('M', 6.0),
    ('M', 7.0),
    ('G', 28.0),
    ('G', 30.0),
    ('G', 53.0),
    ('G', 91.0),
    ('G', 93.0),
];

/// A dwell shorter than the configured one by less than this still counts:
/// it absorbs the rounding of the written seconds.
const DWELL_TOLERANCE_S: f64 = 0.0005;

/// GRBL and grblHAL's arc radius check (`gcode.c`): start and end radii may
/// differ by 0.005 mm, or by 0.1 % of the radius up to 0.5 mm.
const ARC_RADIUS_ABSOLUTE_MM: f64 = 0.005;
/// See [`ARC_RADIUS_ABSOLUTE_MM`].
const ARC_RADIUS_RELATIVE: f64 = 0.001;
/// See [`ARC_RADIUS_ABSOLUTE_MM`].
const ARC_RADIUS_GROSS_MM: f64 = 0.5;

fn check_characters(text: &str) -> Result<(), ExportError> {
    for (index, content) in text.lines().enumerate() {
        let line = index + 1;
        let reason = if content.chars().count() > MAX_LINE_CHARS {
            Some("it is longer than the controller's 256-byte line buffer")
        } else if !content.chars().all(|c| (' '..='~').contains(&c)) {
            Some("it has a control or non-ASCII character, which the controller acts on at once")
        } else if content.contains(['!', '?', '~']) {
            Some("it has `!`, `?`, or `~`, which the controller acts on at once, even in a comment")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(ExportError::UnsafeLine { line, reason });
        }
    }
    Ok(())
}

fn check_words(blocks: &[Block]) -> Result<(), ExportError> {
    for block in blocks {
        for &(letter, code) in &FORBIDDEN {
            if block.has(letter, code) {
                return Err(ExportError::ForbiddenWord {
                    line: block.line,
                    word: format!("{letter}{code}"),
                });
            }
        }
    }
    Ok(())
}

/// One output unit in the last written decimal place, in mm.
fn quantum_mm(post: &PostProcessor, units: Units) -> f64 {
    let places = decimal_places(&post.number_format, units);
    10f64.powi(-i32::from(places)) * mm_per_unit(units)
}

/// The rules in force on one line, project space, mm.
struct Limits<'a> {
    op: Option<&'a ProfileOp>,
    tool: Option<&'a Tool>,
    /// Lowest permitted Z: stock bottom minus through-cut allowance.
    floor_mm: f64,
    /// Safe Z.
    safe_z_mm: f64,
}

impl<'a> Limits<'a> {
    /// The limits of the operation that owns `line`; outside every
    /// operation, the strictest of them all.
    fn for_line(line: usize, spans: &[OpSpan<'a>], stock_thickness_mm: f64) -> Self {
        if let Some(span) = spans.iter().find(|s| s.lines.contains(&line)) {
            return Self {
                op: Some(span.op),
                tool: Some(span.tool),
                floor_mm: -span.op.through_cut_allowance_mm,
                safe_z_mm: stock_thickness_mm + span.op.safe_z_mm,
            };
        }
        let floor_mm = spans
            .iter()
            .map(|s| -s.op.through_cut_allowance_mm)
            .fold(0.0, f64::max);
        let safe_z_mm = spans
            .iter()
            .map(|s| stock_thickness_mm + s.op.safe_z_mm)
            .fold(stock_thickness_mm, f64::max);
        Self {
            op: None,
            tool: None,
            floor_mm,
            safe_z_mm,
        }
    }
}

/// S1: no move ends below the stock bottom minus the allowance.
fn check_depth(
    mv: &MachineMove,
    limits: &Limits<'_>,
    transform: &MachineTransform,
    tolerance_mm: f64,
) -> Result<(), ExportError> {
    let Some(z) = mv.to.z else {
        return Ok(());
    };
    let z_mm = transform.project_z(z, mv.units);
    if z_mm < limits.floor_mm - tolerance_mm {
        return Err(ExportError::BelowMaterialBottom {
            op: limits.op.map(|op| op.id.clone()),
            line: mv.line,
            z_mm,
            floor_mm: limits.floor_mm,
        });
    }
    Ok(())
}

/// S2: a rapid ends at or above safe Z, and one that moves in XY also
/// starts there.
fn check_rapid(
    mv: &MachineMove,
    limits: &Limits<'_>,
    transform: &MachineTransform,
    tolerance_mm: f64,
) -> Result<(), ExportError> {
    let below = |z: Option<f64>| -> Option<Option<f64>> {
        let Some(z) = z else {
            return Some(None);
        };
        let z_mm = transform.project_z(z, mv.units);
        (z_mm < limits.safe_z_mm - tolerance_mm).then_some(Some(z_mm))
    };
    let moves_xy = differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y);
    let offending = below(mv.to.z).or_else(|| if moves_xy { below(mv.from.z) } else { None });
    match offending {
        Some(z_mm) => Err(ExportError::RapidBelowSafeZ {
            line: mv.line,
            z_mm,
            safe_z_mm: limits.safe_z_mm,
        }),
        None => Ok(()),
    }
}

/// True unless both are known and equal.
fn differs(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !approx_eq(a, b, EPS_MM),
        (None, None) => false,
        _ => true,
    }
}

/// S3: a cut has a positive feed, and it is one of its tool's rates.
fn check_feed(
    mv: &MachineMove,
    limits: &Limits<'_>,
    tolerance_mm_min: f64,
) -> Result<(), ExportError> {
    let line = mv.line;
    let Some(feed) = mv.feed.filter(|f| *f > 0.0) else {
        return Err(ExportError::CutWithoutFeed { line });
    };
    let feed_mm_min = feed * mm_per_unit(mv.units);
    if let Some(tool) = limits.tool {
        let matches = [tool.feed_mm_min, tool.plunge_mm_min]
            .into_iter()
            .flatten()
            .any(|rate| approx_eq(rate, feed_mm_min, tolerance_mm_min));
        if !matches {
            return Err(ExportError::FeedNotFromTool { line, feed_mm_min });
        }
    }
    Ok(())
}

/// An arc the controller will accept: I/J radii agree, or R spans the
/// chord.
fn check_arc(mv: &MachineMove) -> Result<(), ExportError> {
    if !matches!(mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw) {
        return Ok(());
    }
    let line = mv.line;
    let scale = mm_per_unit(mv.units);
    let (Some(sx), Some(sy), Some(x), Some(y)) = (mv.from.x, mv.from.y, mv.to.x, mv.to.y) else {
        return Err(ExportError::ArcRejected {
            line,
            delta_mm: f64::INFINITY,
        });
    };
    let delta_mm = if let Some(r) = mv.arc.r {
        let half_chord = (x - sx).hypot(y - sy) / 2.0;
        ((half_chord - r.abs()) * scale).max(0.0)
    } else {
        let (i, j) = (mv.arc.i.unwrap_or(0.0), mv.arc.j.unwrap_or(0.0));
        let start_r = i.hypot(j) * scale;
        let end_r = (x - (sx + i)).hypot(y - (sy + j)) * scale;
        let delta_mm = (end_r - start_r).abs();
        let accepted = delta_mm <= ARC_RADIUS_ABSOLUTE_MM
            || (delta_mm <= ARC_RADIUS_GROSS_MM && delta_mm <= ARC_RADIUS_RELATIVE * start_r);
        if accepted { 0.0 } else { delta_mm }
    };
    if delta_mm > 0.0 {
        return Err(ExportError::ArcRejected { line, delta_mm });
    }
    Ok(())
}

/// S6: every point of the span's toolpath is reached, in order, by the
/// program's moves read back into project space. A wrong origin, units, or
/// mirror cannot pass.
fn check_reproduces(
    span: &OpSpan<'_>,
    blocks: &[Block],
    post: &PostProcessor,
    transform: &MachineTransform,
) -> Result<(), ExportError> {
    let points: Vec<(Point3, f64)> = blocks
        .iter()
        .filter(|b| span.lines.contains(&b.line))
        .filter_map(|b| b.motion.as_ref())
        .filter_map(|m| {
            let machine = Point3::new(m.to.x?, m.to.y?, m.to.z?);
            Some((
                transform.to_project(machine, m.units),
                quantum_mm(post, m.units),
            ))
        })
        .collect();
    let mut cursor = 0;
    for (index, mv) in span.toolpath.moves.iter().enumerate() {
        let target = mv.to;
        let found = points.get(cursor..).and_then(|rest| {
            rest.iter().position(|(p, quantum)| {
                let tolerance = *quantum + EPS_MM;
                approx_eq(p.x, target.x, tolerance)
                    && approx_eq(p.y, target.y, tolerance)
                    && approx_eq(p.z, target.z, tolerance)
            })
        });
        let Some(offset) = found else {
            return Err(ExportError::MotionMismatch {
                op: span.op.id.clone(),
                index,
            });
        };
        cursor += offset;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::emit;
    use dc_cam::{Move, generate_profile};
    use dc_geometry::Point3;
    use dc_project::test_support::sample_project;
    use dc_project::{Operation, Project};

    /// The sample project (6 mm stock, safe Z 11 absolute, feed 1000,
    /// plunge 300, 18000 rpm, GRBL), its operation, and its tool.
    fn fixture() -> (Project, ProfileOp, Tool) {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        let tool = project.tools[0].clone();
        (project, op, tool)
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

    fn emit_one(
        project: &Project,
        op: &ProfileOp,
        tool: &Tool,
        tp: Toolpath,
    ) -> Result<String, ExportError> {
        emit(project, &[(op.clone(), tool.clone(), tp)], "2026-10-15").map(|(p, _)| p.text)
    }

    /// Runs the safety pass on hand-written `text` as if it were the whole
    /// of the fixture's operation.
    fn check_text(text: &str) -> Result<(), ExportError> {
        let (project, op, tool) = fixture();
        let transform = MachineTransform::new(&project).unwrap();
        let toolpath = Toolpath::default();
        let lines = text.lines().count() + 1;
        let spans = [OpSpan {
            op: &op,
            tool: &tool,
            toolpath: &toolpath,
            lines: 1..lines,
        }];
        check(
            text,
            &project.post_processor,
            &transform,
            project.stock.thickness_mm,
            &spans,
        )
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
        // An explicit allowance permits exactly that much.
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
        let err = check_text("G21\nG54\nG0 X1 Y1\n").unwrap_err();
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

    #[test]
    fn every_cut_has_active_feed() {
        let ok = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM5\n";
        check_text(ok).unwrap();
        let no_feed = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1\nX10\nG0 Z5\nM5\n";
        let err = check_text(no_feed).unwrap_err();
        assert!(
            matches!(err, ExportError::CutWithoutFeed { line: 7 }),
            "{err:?}"
        );
        let invented =
            "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nX10 F2500\nG0 Z5\nM5\n";
        let err = check_text(invented).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
            "{err:?}"
        );
        let zero = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F0\nG0 Z5\nM5\n";
        let err = check_text(zero).unwrap_err();
        assert!(
            matches!(err, ExportError::CutWithoutFeed { line: 7 }),
            "{err:?}"
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
        let off = "G21\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
        assert!(matches!(
            check_text(off),
            Err(ExportError::SpindleNotRunning { line: 5 })
        ));
        let no_dwell = "G21\nG54\nM3 S18000\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text(no_dwell),
            Err(ExportError::SpindleNotUpToSpeed { line: 6 })
        ));
        let left_on = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
        assert!(matches!(
            check_text(left_on),
            Err(ExportError::SpindleNotStopped)
        ));
    }

    #[test]
    fn forbidden_words_and_unsafe_characters_are_rejected() {
        for (text, word) in [
            ("G21\nG54\nM4 S1000\n", "M4"),
            ("G21\nG54\nM7\n", "M7"),
            ("G21\nG54\nM6 T1\n", "M6"),
            ("G21\nG54\nG53 G0 Z0\n", "G53"),
            ("G21\nG54\nG91\n", "G91"),
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
        // in a way the check cannot see.
        for (text, word) in [
            ("G21\nG54\nG92 Z0\n", "G92"),
            ("G21\nG54\nG10 L20 P1 Z0\n", "G10"),
            ("G21\nG54\nG43.1 Z5\n", "G43.1"),
            ("G21\nG54\nG38.2 Z-10 F100\n", "G38.2"),
            ("G21\nG54\nG91.1\n", "G91.1"),
            ("G21\nG54\nM8\n", "M8"),
        ] {
            let err = check_text(text).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
                "{text}: {err:?}"
            );
        }
        for text in ["G21 G17 G90 G94\nG54\nM0\nM5\nM30\n", "G20\nG59\nM2\n"] {
            assert!(check_text(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn violation_is_error_not_warning() {
        // A hand-built toolpath that plunges through the spoilboard: export
        // fails outright and yields no program to write.
        let (project, op, tool) = fixture();
        let result = emit(&project, &[(op, tool, one_cut(-1.0))], "2026-10-15");
        assert!(
            matches!(result, Err(ExportError::BelowMaterialBottom { .. })),
            "{result:?}"
        );
    }
}
