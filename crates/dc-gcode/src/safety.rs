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
use dc_project::{PostProcessor, ProfileOp, SpindleControl, Tool, ToolId, Units};

use crate::ExportError;
use crate::format::decimal_places;
use crate::parse::{Block, MachineMove, MotionKind, is_code, parse_program};
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
    let manual = post.feed_speed.spindle_control == SpindleControl::Manual;
    let mut router = RouterPauses::default();
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
        if manual {
            // S4 (manual): the program never touches the spindle.
            if let Some(word) = block.words.iter().find(|w| {
                w.letter == 'S'
                    || (w.letter == 'M' && (is_code(w.value, 3.0) || is_code(w.value, 5.0)))
            }) {
                return Err(ExportError::ForbiddenWord {
                    line,
                    word: format!("{}{}", word.letter, word.value),
                });
            }
        }
        // S4, #43: the user fits tools and switches the router at a pause,
        // so it comes with the bit at safe Z. Afterwards Z is unknown: the
        // user may have jogged or set Z zero.
        if block.has('M', 0.0) || block.has('M', 1.0) {
            let safe_z_mm = Limits::for_line(line, spans, stock_thickness_mm).safe_z_mm;
            if !router.at_or_above(safe_z_mm) {
                return Err(ExportError::PauseNotAtSafeZ { line });
            }
            if manual {
                router.pause(safe_z_mm);
            }
            router.z_mm = None;
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
            // #43: Z is known again only after a retract since the last
            // pause, where the user may have set Z zero.
            let safe_z_mm = Limits::for_line(line, spans, stock_thickness_mm).safe_z_mm;
            if !router.at_or_above(safe_z_mm) {
                return Err(ExportError::SpindleStartNotAtSafeZ { line });
            }
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
            if manual {
                router.cut(line, limits.tool.map(|t| &t.id))?;
            } else {
                if !spindle_on || rpm.is_none_or(|s| s <= 0.0) {
                    return Err(ExportError::SpindleNotRunning { line });
                }
                if dwell_pending {
                    return Err(ExportError::SpindleNotUpToSpeed { line });
                }
            }
            check_feed(mv, &limits, 2.0 * tolerance_mm)?;
            check_arc(mv)?;
        } else {
            check_rapid(mv, &limits, transform, tolerance_mm)?;
        }
        if manual && is_retract(mv) {
            router.retracted();
        }
        router.moved(
            mv.to
                .z
                .map(|z| (transform.project_z(z, mv.units), tolerance_mm)),
        );
    }
    if spindle_on {
        return Err(ExportError::SpindleNotStopped);
    }
    if manual && router.cut_since_pause {
        return Err(ExportError::RouterNotStopped);
    }

    // S6: reading the program back reproduces each toolpath.
    for span in spans {
        check_reproduces(span, &blocks, post, transform)?;
    }
    Ok(())
}

/// Where Z is, for the S4 checks in both spindle modes, and, with manual
/// spindle control, what the program has done since its last cut. A tool's
/// first cut needs a pause after the last retract, so a tool-change pause
/// followed by the retract to safe Z does not count as the pause to turn
/// the router on.
#[derive(Debug, Default)]
struct RouterPauses<'a> {
    /// Z above stock bottom, mm, and its rounding tolerance, if known.
    z_mm: Option<(f64, f64)>,
    /// An `M0` at safe Z has come after the last cut and the last retract.
    paused_at_safe_z: bool,
    /// A cut has come after the last `M0` at safe Z.
    cut_since_pause: bool,
    /// The tool of the last cut.
    tool: Option<&'a ToolId>,
}

impl<'a> RouterPauses<'a> {
    /// The tool is now at `z_mm`, if known.
    fn moved(&mut self, z_mm: Option<(f64, f64)>) {
        if z_mm.is_some() {
            self.z_mm = z_mm;
        }
    }

    /// Whether Z is known to be at or above `safe_z_mm`.
    fn at_or_above(&self, safe_z_mm: f64) -> bool {
        self.z_mm
            .is_some_and(|(z, tolerance)| z >= safe_z_mm - tolerance)
    }

    /// A retract: a pause before it does not count for the next cut.
    fn retracted(&mut self) {
        self.paused_at_safe_z = false;
    }

    /// An `M0` where safe Z is `safe_z_mm`.
    fn pause(&mut self, safe_z_mm: f64) {
        if self.at_or_above(safe_z_mm) {
            self.paused_at_safe_z = true;
            self.cut_since_pause = false;
        }
    }

    /// A cut on `line` with `tool`. A tool's first cut needs a pause at
    /// safe Z since the previous cut, for the user to switch the router on.
    fn cut(&mut self, line: usize, tool: Option<&'a ToolId>) -> Result<(), ExportError> {
        let first_of_tool = self.tool.is_none() || self.tool != tool;
        if first_of_tool && !self.paused_at_safe_z {
            return Err(ExportError::RouterNotStarted { line });
        }
        self.tool = tool.or(self.tool);
        self.paused_at_safe_z = false;
        self.cut_since_pause = true;
        Ok(())
    }
}

/// A retract as `emit` writes one: a rapid in Z alone that does not
/// descend.
fn is_retract(mv: &MachineMove) -> bool {
    let z_only = !differs(mv.from.x, mv.to.x) && !differs(mv.from.y, mv.to.y);
    let rises = match (mv.from.z, mv.to.z) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(from), Some(to)) => to >= from - EPS_MM,
    };
    mv.kind == MotionKind::Rapid && z_only && rises
}

/// `G54` to `G59`.
const WCS_CODES: [f64; 6] = [54.0, 55.0, 56.0, 57.0, 58.0, 59.0];

/// The only `G` and `M` codes the safety pass accepts: the ones DreamCarve
/// emits and this module models. Anything else, such as `G92` or `G10`
/// offsets, `G43.1` tool length, `G38` probing, `G91` or `G93` modes, or
/// `M8` coolant, could change what the controller does in a way the check
/// cannot follow, so it is rejected. This also keeps out the words
/// DreamCarve never sends to GRBL or grblHAL (`definitions/README.md`,
/// "Verification"): `M4` needs a direction pin or laser mode, `M6` needs a
/// tool-change protocol the sender may not speak, `M7` is optional on both,
/// and `G28`, `G30`, and `G53` move in machine coordinates.
const ALLOWED_G: [f64; 16] = [
    0.0, 1.0, 2.0, 3.0, 4.0, 17.0, 20.0, 21.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0, 90.0, 94.0,
];
/// See [`ALLOWED_G`].
const ALLOWED_M: [f64; 6] = [0.0, 1.0, 2.0, 3.0, 5.0, 30.0];

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
        for word in &block.words {
            let allowed: &[f64] = match word.letter {
                'G' => &ALLOWED_G,
                'M' => &ALLOWED_M,
                _ => continue,
            };
            if !allowed.iter().any(|&code| is_code(word.value, code)) {
                return Err(ExportError::ForbiddenWord {
                    line: block.line,
                    word: format!("{}{}", word.letter, word.value),
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
    use dc_project::{Operation, Project, SpindleControl};

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
        check_text_as(text, SpindleControl::Automatic)
    }

    /// [`check_text`] with spindle control set to `control`.
    fn check_text_as(text: &str, control: SpindleControl) -> Result<(), ExportError> {
        let (mut project, op, tool) = fixture();
        project.post_processor.feed_speed.spindle_control = control;
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
        let ok = "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM5\n";
        check_text(ok).unwrap();
        let no_feed = "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1\nX10\nG0 Z5\nM5\n";
        let err = check_text(no_feed).unwrap_err();
        assert!(
            matches!(err, ExportError::CutWithoutFeed { line: 7 }),
            "{err:?}"
        );
        let invented =
            "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F2500\nG0 Z5\nM5\n";
        let err = check_text(invented).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
            "{err:?}"
        );
        let zero = "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F0\nG0 Z5\nM5\n";
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
        let no_dwell = "G21\nG54\nG0 Z5\nM3 S18000\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text(no_dwell),
            Err(ExportError::SpindleNotUpToSpeed { line: 6 })
        ));
        let left_on = "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
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
        // in a way the check cannot see. A motion mode is already active, so
        // the parser reads axis words on the same line as a move.
        for (code, word) in [
            ("G92 Z0", "G92"),
            ("G92.1", "G92.1"),
            ("G10 L20 P1 Z0", "G10"),
            ("G43.1 Z5", "G43.1"),
            ("G38.2 Z-10 F100", "G38.2"),
            ("G91.1", "G91.1"),
            ("M8", "M8"),
        ] {
            let text = format!("G21\nG54\nG0 Z11\n{code}\n");
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

    /// Runs the safety pass on `text` as [`check_text`] does, with the
    /// router switched by hand.
    fn check_manual(text: &str) -> Result<(), ExportError> {
        check_text_as(text, SpindleControl::Manual)
    }

    /// S4 (manual), #34: a tool's first cut must follow an `M0` issued at
    /// safe Z after the last cut, and the program must pause again at safe
    /// Z after its last cut. Safe Z is machine Z5 in the fixture.
    #[test]
    fn manual_cut_without_on_pause_is_rejected() {
        let ok = "G21\nG54\nG0 Z5\nM0 (MSG, Turn the router ON.)\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM0 (MSG, Turn the router OFF.)\n";
        check_manual(ok).unwrap();

        let no_pause = "G21\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(no_pause).unwrap_err();
        assert!(
            matches!(err, ExportError::RouterNotStarted { line: 5 }),
            "{err:?}"
        );

        // A tool-change pause, then the retract that must follow it: the
        // router still needs its own pause after the retract.
        let no_on_after_retract = "G21\nG54\nG0 Z5\nM0\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(no_on_after_retract).unwrap_err();
        assert!(
            matches!(err, ExportError::RouterNotStarted { line: 7 }),
            "{err:?}"
        );

        let no_off = "G21\nG54\nG0 Z5\nM0\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
        let err = check_manual(no_off).unwrap_err();
        assert!(matches!(err, ExportError::RouterNotStopped), "{err:?}");

        // No spindle word reaches a router the user switches by hand.
        for (text, word) in [
            ("G21\nG54\nM3\n", "M3"),
            ("G21\nG54\nM5\n", "M5"),
            ("G21\nG54\nS18000\n", "S18000"),
        ] {
            let err = check_manual(text).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
                "{text}: {err:?}"
            );
        }
    }

    /// S4, #34: the automatic rules stand. The fixture is automatic, a
    /// program that starts the spindle and dwells passes, and one that
    /// relies on an `M0` instead of `M3` is rejected.
    #[test]
    fn automatic_rules_unchanged() {
        let (project, _, _) = fixture();
        assert_eq!(
            project.post_processor.feed_speed.spindle_control,
            SpindleControl::Automatic
        );
        let ok = "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        check_text_as(ok, SpindleControl::Automatic).unwrap();
        let manual_style = "G21\nG54\nG0 Z5\nM0\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_text_as(manual_style, SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(err, ExportError::SpindleNotRunning { line: 6 }),
            "{err:?}"
        );
        let no_dwell = "G21\nG54\nG0 Z5\nM3 S18000\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text_as(no_dwell, SpindleControl::Automatic),
            Err(ExportError::SpindleNotUpToSpeed { line: 6 })
        ));
    }

    /// S4, #43: the user fits tools and switches the router at a pause, so
    /// every `M0` comes where Z is known to be at or above safe Z (machine
    /// Z5 in the fixture), in both spindle modes.
    #[test]
    fn pause_must_be_at_safe_z() {
        for control in [SpindleControl::Automatic, SpindleControl::Manual] {
            let below = match control {
                SpindleControl::Automatic => {
                    "G21\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nM0\n"
                }
                SpindleControl::Manual => "G21\nG54\nG0 Z5\nM0\nX0 Y0\nG1 Z-1 F300\nM0\n",
            };
            let below_line = below.lines().count();
            for (text, line) in [
                // Z never set: the program-start case.
                ("G21\nG54\nM0\n", 3),
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
        check_text_as("G21\nG54\nG0 Z5\nM0\nG0 Z5\n", SpindleControl::Automatic).unwrap();
    }

    /// S4, #43: Z is not known after a pause, because the user may have
    /// fitted a tool and set Z zero. The spindle starts, and the router-on
    /// pause comes, only after a retract to safe Z.
    #[test]
    fn spindle_start_needs_a_retract_after_a_pause() {
        let no_lift = "G21\nG54\nG0 Z5\nM5\nM0\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
        let err = check_text_as(no_lift, SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(err, ExportError::SpindleStartNotAtSafeZ { line: 6 }),
            "{err:?}"
        );
        let lifted =
            "G21\nG54\nG0 Z5\nM5\nM0\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        check_text_as(lifted, SpindleControl::Automatic).unwrap();

        let manual_no_lift = "G21\nG54\nG0 Z5\nM0\nM0\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(manual_no_lift).unwrap_err();
        assert!(
            matches!(err, ExportError::PauseNotAtSafeZ { line: 5 }),
            "{err:?}"
        );
        let manual_lifted = "G21\nG54\nG0 Z5\nM0\nG0 Z5\nM0\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        check_manual(manual_lifted).unwrap();
    }
}
