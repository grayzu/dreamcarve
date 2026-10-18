// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The safety pass: parse the emitted program and check the phase 1 safety
//! rules on what the controller will read (`docs/specs/phase-1/design.md`,
//! "Emission", step 7; `docs/specs/phase-1/functional.md`, S1–S7). Every
//! violation is an [`ExportError`]; there are no safety warnings.
//!
//! Checks run in project space: each parsed point is mapped back through
//! the [`MachineTransform`] using the units the program itself selected, so
//! a wrong origin, a mirror, or a missing units command shows up as a
//! violation instead of being trusted. S6 compares each operation's moves
//! with its toolpath move for move, arc direction and centre included; see
//! [`check_reproduces`] for exactly what it accepts.

use std::f64::consts::TAU;
use std::ops::Range;

use dc_cam::{MoveKind, Toolpath};
use dc_geometry::{Arc, EPS_MM, Point, Point3, approx_eq};
use dc_project::{PostProcessor, ProfileOp, SpindleControl, Stock, Tool, ToolId, Units};

use crate::ExportError;
use crate::emit::uses_m6;
use crate::format::decimal_places;
use crate::parse::{Block, MachineMove, MachinePosition, MotionKind, is_code, parse_program};
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
/// in `spans`, against S1–S7, and returns the program's extents.
/// `flatten_tolerance_mm` is the most a straight move written for an arc
/// may cut inside it.
///
/// # Errors
///
/// The first violation found, as the matching [`ExportError`].
pub(crate) fn check(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock: &Stock,
    stock_thickness_mm: f64,
    flatten_tolerance_mm: f64,
    spans: &[OpSpan<'_>],
) -> Result<Extents, ExportError> {
    let blocks = check_rules(text, post, transform, stock_thickness_mm, spans)?;
    // S6: reading the program back reproduces each toolpath.
    for span in spans {
        check_reproduces(span, &blocks, post, transform, flatten_tolerance_mm)?;
    }
    // S7: every cut stays on the stock.
    check_on_stock(&blocks, post, transform, stock, stock_thickness_mm, spans)?;
    Ok(extents(&blocks, transform.output_units()))
}

/// The lowest and highest value a program reaches on one machine axis,
/// in the program's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisRange {
    /// The lowest value.
    pub min: f64,
    /// The highest value.
    pub max: f64,
}

impl AxisRange {
    fn including(range: Option<Self>, value: f64) -> Option<Self> {
        Some(match range {
            Some(r) => Self {
                min: r.min.min(value),
                max: r.max.max(value),
            },
            None => Self {
                min: value,
                max: value,
            },
        })
    }
}

/// Where a program takes the tool centre, in machine coordinates: every
/// position it moves through, arcs' bulges included (#62). Z min is the
/// deepest cut. An axis the program never sets is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extents {
    /// The units of every range: the program's.
    pub units: Units,
    /// Machine X.
    pub x: Option<AxisRange>,
    /// Machine Y.
    pub y: Option<AxisRange>,
    /// Machine Z.
    pub z: Option<AxisRange>,
}

/// The extents of `blocks`, a program written in `units`.
fn extents(blocks: &[Block], units: Units) -> Extents {
    let mut extents = Extents {
        units,
        x: None,
        y: None,
        z: None,
    };
    for mv in blocks.iter().filter_map(|b| b.motion.as_ref()) {
        for p in [mv.from, mv.to] {
            if let Some(x) = p.x {
                extents.x = AxisRange::including(extents.x, x);
            }
            if let Some(y) = p.y {
                extents.y = AxisRange::including(extents.y, y);
            }
            if let Some(z) = p.z {
                extents.z = AxisRange::including(extents.z, z);
            }
        }
        for p in arc_extremes(mv) {
            extents.x = AxisRange::including(extents.x, p.x);
            extents.y = AxisRange::including(extents.y, p.y);
        }
    }
    extents
}

/// The machine XY points, program units, where arc move `mv` reaches its
/// extreme X or Y between its ends; none for a straight move or an arc
/// whose ends are not known.
fn arc_extremes(mv: &MachineMove) -> Vec<Point> {
    let machine_cw = match mv.kind {
        MotionKind::ArcCw => true,
        MotionKind::ArcCcw => false,
        MotionKind::Rapid | MotionKind::Linear => return Vec::new(),
    };
    let (Some(sx), Some(sy), Some(ex), Some(ey)) = (mv.from.x, mv.from.y, mv.to.x, mv.to.y) else {
        return Vec::new();
    };
    let centre = match mv.arc.r {
        Some(r) => r_centre(sx, sy, ex, ey, r, machine_cw),
        None => Point::new(sx + mv.arc.i.unwrap_or(0.0), sy + mv.arc.j.unwrap_or(0.0)),
    };
    let mut arc = Arc::from_ends(Point::new(sx, sy), Point::new(ex, ey), centre, !machine_cw);
    // Fail-closed: an arc that ends where it starts is a full circle to
    // the controller, though `check_not_full_circle` rejects it first.
    if arc.sweep_rad.abs() < MIN_ARC_SWEEP_RAD {
        arc.sweep_rad = TAU;
    }
    arc.axis_extremes().collect()
}

/// The centre GRBL and grblHAL compute for an R-format arc from
/// (`sx`, `sy`) to (`ex`, `ey`) (`gcode.c`, radius format): on the right
/// of the chord for G2 and the left for G3, the other side for a negative
/// R.
fn r_centre(sx: f64, sy: f64, ex: f64, ey: f64, r: f64, machine_cw: bool) -> Point {
    let (dx, dy) = (ex - sx, ey - sy);
    let chord = dx.hypot(dy);
    if chord <= 0.0 {
        return Point::new(sx, sy);
    }
    let h = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    let side = if machine_cw { 1.0 } else { -1.0 } * r.signum();
    Point::new(
        (sx + ex) / 2.0 + side * h * dy / chord,
        (sy + ey) / 2.0 - side * h * dx / chord,
    )
}

/// S7 (#62): every cutting move, widened by its operation's tool radius,
/// stays inside the stock rectangle `[0, width] × [0, height]`, within one
/// unit in the last written place. Arcs count to their bulge. A cut
/// outside every operation is checked as a point.
fn check_on_stock(
    blocks: &[Block],
    post: &PostProcessor,
    transform: &MachineTransform,
    stock: &Stock,
    stock_thickness_mm: f64,
    spans: &[OpSpan<'_>],
) -> Result<(), ExportError> {
    for mv in blocks.iter().filter_map(|b| b.motion.as_ref()) {
        if !mv.kind.is_cut() {
            continue;
        }
        let limits = Limits::for_line(mv.line, spans, stock_thickness_mm);
        let radius_mm = limits.tool.map_or(0.0, |t| t.diameter_mm / 2.0);
        let tolerance_mm = quantum_mm(post, mv.units) + EPS_MM;
        let ends = [mv.from, mv.to]
            .into_iter()
            .filter_map(|p| Some(Point::new(p.x?, p.y?)));
        for m in ends.chain(arc_extremes(mv)) {
            let p = transform
                .to_project(Point3::new(m.x, m.y, 0.0), mv.units)
                .xy();
            let sides = [
                ("-X", -(p.x - radius_mm)),
                ("+X", p.x + radius_mm - stock.width_mm),
                ("-Y", -(p.y - radius_mm)),
                ("+Y", p.y + radius_mm - stock.height_mm),
            ];
            if let Some((side, overhang_mm)) = sides.into_iter().find(|(_, o)| *o > tolerance_mm) {
                return Err(ExportError::OffStock {
                    op: limits.op.map(|op| op.id.clone()),
                    line: mv.line,
                    side,
                    overhang_mm,
                });
            }
        }
    }
    Ok(())
}

/// Checks `text` against S1–S5 and returns its blocks.
fn check_rules(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock_thickness_mm: f64,
    spans: &[OpSpan<'_>],
) -> Result<Vec<Block>, ExportError> {
    check_characters(text)?;
    let blocks = parse_program(text)?;
    check_words(&blocks, uses_m6(post))?;

    let dwell_s = post.feed_speed.spindle_on_dwell.duration_s;
    let manual = post.feed_speed.spindle_control == SpindleControl::Manual;
    let mut router = RouterPauses::default();
    let mut units_set = false;
    let mut wcs_set = false;
    let mut absolute_set = false;
    // The last pause: the controller's modal state must be set again after
    // it, because the user may change it at the controller (#55).
    let mut last_pause: Option<usize> = None;
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
        if block.has('G', 90.0) {
            absolute_set = true;
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
        // S4, #43, #57: the user fits tools and switches the router at a
        // pause or an `M6` tool change, so it comes with the bit at safe Z.
        // Afterwards Z is unknown: the user may have jogged or set Z zero.
        if block.has('M', 0.0) || block.has('M', 1.0) || block.has('M', 6.0) {
            let safe_z_mm = Limits::for_line(line, spans, stock_thickness_mm).safe_z_mm;
            if !router.at_or_above(safe_z_mm) {
                return Err(ExportError::PauseNotAtSafeZ { line });
            }
            if manual {
                router.pause(safe_z_mm);
            }
            router.z_mm = None;
            last_pause = Some(line);
            units_set = false;
            wcs_set = false;
            absolute_set = false;
        }
        let starts_spindle = block.has('M', 3.0);
        // S5: units, work coordinates, and absolute distance mode before
        // anything happens, and again after every pause.
        if block.motion.is_some() || starts_spindle {
            let missing = [
                (units_set, "units (G20 or G21)"),
                (wcs_set, "a work coordinate system (G54 to G59)"),
                (absolute_set, "absolute distance mode (G90)"),
            ]
            .into_iter()
            .find_map(|(set, what)| (!set).then_some(what));
            if let Some(what) = missing {
                return Err(match last_pause {
                    Some(pause_line) => ExportError::MissingSetupAfterPause {
                        line,
                        pause_line,
                        what,
                    },
                    None => ExportError::MissingSetup { what },
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
            check_not_full_circle(mv, post)?;
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
    Ok(blocks)
}

/// Where Z is, for the S4 checks in both spindle modes, and, with manual
/// spindle control, what the program has done since its last cut. A tool's
/// first cut needs a pause after the last retract, so a tool-change stop
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
/// "Verification"): `M4` needs a direction pin or laser mode, `M7` is
/// optional on both, and `G28`, `G30`, and `G53` move in machine
/// coordinates. `M6` is accepted only from a definition whose tool change
/// uses it ([`TOOL_CHANGE_M`]).
const ALLOWED_G: [f64; 16] = [
    0.0, 1.0, 2.0, 3.0, 4.0, 17.0, 20.0, 21.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0, 90.0, 94.0,
];
/// See [`ALLOWED_G`].
const ALLOWED_M: [f64; 6] = [0.0, 1.0, 2.0, 3.0, 5.0, 30.0];
/// The `M` codes of grblHAL's tool change, accepted when the definition's
/// tool change uses `M6` (#57): `M6` itself, checked like a pause, and
/// `M61`, which sets the current tool number without a stop or a move.
/// GRBL 1.1 has neither.
const TOOL_CHANGE_M: [f64; 2] = [6.0, 61.0];

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

/// Rejects every `G` and `M` code not in [`ALLOWED_G`] or [`ALLOWED_M`].
/// The codes in [`TOOL_CHANGE_M`] are allowed too when the definition
/// changes tools with `M6` (`with_m6`).
fn check_words(blocks: &[Block], with_m6: bool) -> Result<(), ExportError> {
    let tool_change: &[f64] = if with_m6 { &TOOL_CHANGE_M } else { &[] };
    for block in blocks {
        for word in &block.words {
            let (allowed, extra): (&[f64], &[f64]) = match word.letter {
                'G' => (&ALLOWED_G, &[]),
                'M' => (&ALLOWED_M, tool_change),
                _ => continue,
            };
            if !allowed
                .iter()
                .chain(extra)
                .any(|&code| is_code(word.value, code))
            {
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

/// An arc move whose end is its start within half a unit in the last
/// written place is a full circle to the controller. Rejected anywhere in
/// the program, fail-closed: S6 rejects it inside an operation, and this
/// covers every other line, so S7 and the extents never see one.
fn check_not_full_circle(mv: &MachineMove, post: &PostProcessor) -> Result<(), ExportError> {
    if is_full_circle(mv, post) {
        return Err(ExportError::FullCircleArc { line: mv.line });
    }
    Ok(())
}

/// Whether arc move `mv` ends where it starts, as the controller reads it.
fn is_full_circle(mv: &MachineMove, post: &PostProcessor) -> bool {
    if !matches!(mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw) {
        return false;
    }
    let (Some(sx), Some(sy), Some(ex), Some(ey)) = (mv.from.x, mv.from.y, mv.to.x, mv.to.y) else {
        return false;
    };
    let half_quantum = quantum_mm(post, mv.units) / mm_per_unit(mv.units) / 2.0;
    (ex - sx).abs() < half_quantum && (ey - sy).abs() < half_quantum
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

/// S6: reading the program back reproduces the span's toolpath, move for
/// move, in project space.
///
/// Each toolpath move must be matched by the next motion lines, and by
/// nothing else:
///
/// - a rapid by one rapid that ends at its end point, or two whose first
///   moves in Z alone or in XY alone towards it (`emit` splits a rapid so
///   it never travels sideways below either end);
/// - a straight cut by one `G1` that ends at its end point;
/// - an arc by `G2`/`G3` moves and straight moves that each end on the
///   arc, further along it than the last, the last one at the arc's end.
///   A `G2`/`G3` must turn the same way as the toolpath once the axis
///   mapping is applied (a mirror swaps them), and its centre, from I and
///   J, must be the toolpath's; with R, where the centre is
///   ill-conditioned for short chords, the controller's arc midpoint must
///   lie on the toolpath's arc instead. A straight move may cut inside the
///   arc by no more than the flatten tolerance. An arc too small to be one
///   is matched as a straight cut, as `emit` writes it.
///
/// A move whose end point the program is already at may be matched by no
/// lines, since `emit` writes nothing for it. The only other motion
/// allowed in the span is a retract (a rapid up in Z alone) before the
/// first toolpath move or after the last: the tool-change and router
/// retracts. Ends compare within one unit in the last written place.
fn check_reproduces(
    span: &OpSpan<'_>,
    blocks: &[Block],
    post: &PostProcessor,
    transform: &MachineTransform,
    flatten_tolerance_mm: f64,
) -> Result<(), ExportError> {
    let motions: Vec<ReadMove<'_>> = blocks
        .iter()
        .filter(|b| span.lines.contains(&b.line))
        .filter_map(|b| b.motion.as_ref())
        .map(|mv| ReadMove::new(mv, post, transform))
        .collect();
    let mismatch = |index| ExportError::MotionMismatch {
        op: span.op.id.clone(),
        index,
    };
    let mut cursor = motions
        .iter()
        .position(|m| !is_retract(m.mv))
        .unwrap_or(motions.len());
    let mut previous: Option<Point3> = None;
    for (index, mv) in span.toolpath.moves.iter().enumerate() {
        let rest = motions.get(cursor..).unwrap_or_default();
        // Where the controller is before `rest`.
        let here = rest
            .first()
            .map_or_else(|| motions.last().and_then(|m| m.to), |m| m.from);
        let tolerance_mm = rest
            .first()
            .or(motions.last())
            .map_or(EPS_MM, |m| m.tolerance_mm);
        let already_there = here.is_some_and(|h| near(h, mv.to, tolerance_mm));
        let consumed = match mv.kind {
            MoveKind::Rapid => match_rapid(rest, mv.to, already_there),
            MoveKind::Linear => match_linear(rest, mv.to, already_there),
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let clockwise = matches!(mv.kind, MoveKind::ArcCw { .. });
                match previous.map(|start| ToolpathArc::new(start, mv.to, centre, clockwise)) {
                    Some(Some(arc)) => match_arc(rest, &arc, transform, flatten_tolerance_mm),
                    // Too small to be an arc: `emit` writes a straight cut.
                    Some(None) => match_linear(rest, mv.to, already_there),
                    // `emit` cannot write an arc without a start.
                    None => None,
                }
            }
        };
        cursor += consumed.ok_or_else(|| mismatch(index))?;
        previous = Some(mv.to);
    }
    if motions
        .get(cursor..)
        .unwrap_or_default()
        .iter()
        .all(|m| is_retract(m.mv))
    {
        Ok(())
    } else {
        Err(mismatch(span.toolpath.moves.len()))
    }
}

/// A parsed move with its ends in project space, mm.
struct ReadMove<'a> {
    mv: &'a MachineMove,
    /// The start, if every axis is known.
    from: Option<Point3>,
    /// The end, if every axis is known.
    to: Option<Point3>,
    /// The end in XY, if known.
    to_xy: Option<Point>,
    /// The end in Z, if known.
    to_z: Option<f64>,
    /// One unit in the last written place, mm, plus [`EPS_MM`].
    tolerance_mm: f64,
}

impl<'a> ReadMove<'a> {
    fn new(mv: &'a MachineMove, post: &PostProcessor, transform: &MachineTransform) -> Self {
        let xy = |p: &MachinePosition| {
            let m = Point3::new(p.x?, p.y?, 0.0);
            Some(transform.to_project(m, mv.units).xy())
        };
        let z = |p: &MachinePosition| p.z.map(|z| transform.project_z(z, mv.units));
        let full = |p: &MachinePosition| {
            let (xy, z) = (xy(p)?, z(p)?);
            Some(Point3::new(xy.x, xy.y, z))
        };
        Self {
            mv,
            from: full(&mv.from),
            to: full(&mv.to),
            to_xy: xy(&mv.to),
            to_z: z(&mv.to),
            tolerance_mm: quantum_mm(post, mv.units) + EPS_MM,
        }
    }

    /// Whether the move ends at `target`.
    fn reaches(&self, target: Point3) -> bool {
        self.to
            .is_some_and(|to| near(to, target, self.tolerance_mm))
    }
}

/// Whether `a` and `b` agree on every axis within `tolerance_mm`.
fn near(a: Point3, b: Point3, tolerance_mm: f64) -> bool {
    approx_eq(a.x, b.x, tolerance_mm)
        && approx_eq(a.y, b.y, tolerance_mm)
        && approx_eq(a.z, b.z, tolerance_mm)
}

/// How many of `rest` a rapid to `target` takes, if they match it.
fn match_rapid(rest: &[ReadMove<'_>], target: Point3, already_there: bool) -> Option<usize> {
    let rapid = |m: &&ReadMove<'_>| m.mv.kind == MotionKind::Rapid;
    if let Some(first) = rest.first().filter(rapid) {
        if first.reaches(target) {
            return Some(1);
        }
        let (from, to) = (&first.mv.from, &first.mv.to);
        let tolerance = first.tolerance_mm;
        let z_only = !differs(from.x, to.x)
            && !differs(from.y, to.y)
            && first
                .to_z
                .is_some_and(|z| approx_eq(z, target.z, tolerance));
        let xy_only = !differs(from.z, to.z)
            && first.to_xy.is_some_and(|p| {
                approx_eq(p.x, target.x, tolerance) && approx_eq(p.y, target.y, tolerance)
            });
        if (z_only || xy_only) && rest.get(1).filter(rapid).is_some_and(|m| m.reaches(target)) {
            return Some(2);
        }
    }
    already_there.then_some(0)
}

/// How many of `rest` a straight cut to `target` takes, if they match it.
fn match_linear(rest: &[ReadMove<'_>], target: Point3, already_there: bool) -> Option<usize> {
    if rest
        .first()
        .is_some_and(|m| m.mv.kind == MotionKind::Linear && m.reaches(target))
    {
        return Some(1);
    }
    already_there.then_some(0)
}

/// How many of `rest` `arc` takes, if they match it: see
/// [`check_reproduces`].
fn match_arc(
    rest: &[ReadMove<'_>],
    arc: &ToolpathArc,
    transform: &MachineTransform,
    flatten_tolerance_mm: f64,
) -> Option<usize> {
    let mut turned_rad = 0.0;
    for (count, read) in rest.iter().enumerate() {
        let to = read.to?;
        let tolerance = read.tolerance_mm;
        let turn_rad = arc.turn_to(to.xy(), tolerance)?;
        let slack_rad = arc.angle_tolerance_rad(tolerance);
        let (low_z, high_z) = (arc.start.z.min(arc.end.z), arc.start.z.max(arc.end.z));
        if turn_rad < turned_rad - slack_rad
            || to.z < low_z - tolerance
            || to.z > high_z + tolerance
        {
            return None;
        }
        match read.mv.kind {
            MotionKind::Rapid => return None,
            MotionKind::Linear => {
                // A chord cuts inside the arc by its sagitta.
                let step_rad = (turn_rad - turned_rad).max(0.0);
                let sagitta_mm = arc.radius_mm * (1.0 - (step_rad / 2.0).cos());
                if sagitta_mm > flatten_tolerance_mm + tolerance {
                    return None;
                }
            }
            MotionKind::ArcCw | MotionKind::ArcCcw => {
                if !arc_follows(read, arc, transform, turned_rad, turn_rad) {
                    return None;
                }
            }
        }
        if near(to, arc.end, tolerance) {
            return Some(count + 1);
        }
        turned_rad = turn_rad;
    }
    None
}

/// Whether arc move `read`, which ends `turn_rad` along `arc`, runs along
/// `arc` from `turned_rad`: same direction, and the same centre (I/J) or
/// a midpoint on the arc (R).
fn arc_follows(
    read: &ReadMove<'_>,
    arc: &ToolpathArc,
    transform: &MachineTransform,
    turned_rad: f64,
    turn_rad: f64,
) -> bool {
    let mv = read.mv;
    let machine_cw = mv.kind == MotionKind::ArcCw;
    if (machine_cw != transform.mirrors_xy()) != arc.clockwise {
        return false;
    }
    let (Some(sx), Some(sy), Some(ex), Some(ey)) = (mv.from.x, mv.from.y, mv.to.x, mv.to.y) else {
        return false;
    };
    let to_project = |x: f64, y: f64| transform.to_project(Point3::new(x, y, 0.0), mv.units).xy();
    let tolerance = read.tolerance_mm;
    // The same start and end is a full circle to the controller.
    let (start, end) = (to_project(sx, sy), to_project(ex, ey));
    if (end.x - start.x).abs() < tolerance / 2.0 && (end.y - start.y).abs() < tolerance / 2.0 {
        return false;
    }
    let Some(r) = mv.arc.r else {
        let (i, j) = (mv.arc.i.unwrap_or(0.0), mv.arc.j.unwrap_or(0.0));
        let centre = to_project(sx + i, sy + j);
        return approx_eq(centre.x, arc.centre.x, tolerance)
            && approx_eq(centre.y, arc.centre.y, tolerance);
    };
    let Point { x: cx, y: cy } = r_centre(sx, sy, ex, ey, r, machine_cw);
    let start_rad = (sy - cy).atan2(sx - cx);
    let end_rad = (ey - cy).atan2(ex - cx);
    let sweep_rad = if machine_cw {
        start_rad - end_rad
    } else {
        end_rad - start_rad
    }
    .rem_euclid(TAU);
    let mid_rad = start_rad + if machine_cw { -1.0 } else { 1.0 } * sweep_rad / 2.0;
    let radius = (sx - cx).hypot(sy - cy);
    let mid = to_project(cx + radius * mid_rad.cos(), cy + radius * mid_rad.sin());
    let slack_rad = arc.angle_tolerance_rad(tolerance);
    arc.turn_to(mid, tolerance)
        .is_some_and(|t| t >= turned_rad - slack_rad && t <= turn_rad + slack_rad)
}

/// An arc move of a toolpath in project space, mm.
struct ToolpathArc {
    start: Point3,
    end: Point3,
    centre: Point,
    radius_mm: f64,
    start_rad: f64,
    sweep_rad: f64,
    clockwise: bool,
}

/// An arc whose radius or sweep is below this is written as a straight
/// move (`emit`, `ArcGeometry::new`).
const MIN_ARC_SWEEP_RAD: f64 = 1e-9;

impl ToolpathArc {
    /// The arc from `start` to `end` about `centre`, or `None` if `emit`
    /// writes it as a straight move.
    fn new(start: Point3, end: Point3, centre: Point, clockwise: bool) -> Option<Self> {
        let radius_mm = start.xy().distance_to(centre);
        let start_rad = (start.y - centre.y).atan2(start.x - centre.x);
        let end_rad = (end.y - centre.y).atan2(end.x - centre.x);
        let turn = if clockwise {
            start_rad - end_rad
        } else {
            end_rad - start_rad
        };
        let sweep_rad = turn.rem_euclid(TAU);
        (radius_mm > MIN_ARC_SWEEP_RAD && sweep_rad > MIN_ARC_SWEEP_RAD).then_some(Self {
            start,
            end,
            centre,
            radius_mm,
            start_rad,
            sweep_rad,
            clockwise,
        })
    }

    /// The angle `tolerance_mm` spans on the arc.
    fn angle_tolerance_rad(&self, tolerance_mm: f64) -> f64 {
        tolerance_mm / self.radius_mm
    }

    /// How far along the arc `p` is, as the angle turned from the start in
    /// the arc's direction, or `None` if `p` is not on the arc within
    /// `tolerance_mm`.
    fn turn_to(&self, p: Point, tolerance_mm: f64) -> Option<f64> {
        if (p.distance_to(self.centre) - self.radius_mm).abs() > tolerance_mm {
            return None;
        }
        let slack_rad = self.angle_tolerance_rad(tolerance_mm);
        let angle = (p.y - self.centre.y).atan2(p.x - self.centre.x);
        let turn = if self.clockwise {
            self.start_rad - angle
        } else {
            angle - self.start_rad
        };
        let mut turn = turn.rem_euclid(TAU);
        // Just short of the start, within rounding.
        if turn > self.sweep_rad + slack_rad && turn > TAU - slack_rad {
            turn -= TAU;
        }
        (turn <= self.sweep_rad + slack_rad).then_some(turn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::emit;
    use std::f64::consts::FRAC_PI_2;

    use dc_cam::{Move, MoveKind, generate_profile};
    use dc_geometry::{Point, Point3};
    use dc_project::test_support::sample_project;
    use dc_project::{ArcFormat, Axis, Operation, Project, SpindleControl};

    /// The sample project (100 × 100 × 6 mm stock, safe Z 11 absolute,
    /// feed 1000, plunge 300, 18000 rpm, GRBL), its operation, and its
    /// tool as a point: the hand-written toolpaths below run along the
    /// stock's left and front edges (X = 0, Y = 0), which S7 allows only
    /// for a tool of no width. The S7 tests set a diameter.
    fn fixture() -> (Project, ProfileOp, Tool) {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        let mut tool = project.tools[0].clone();
        tool.diameter_mm = 0.0;
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

    /// S7 (#62): the cutter's edge must stay on the stock. With a 6 mm
    /// tool a cut 3 mm in from every edge passes; one along the left edge
    /// overhangs it by the radius; a half circle whose ends are 7 mm in
    /// from the back edge, with the radius, but which bulges 10 mm past
    /// them fails at +Y by 3 mm.
    #[test]
    fn cut_off_stock_fails_s7() {
        let (project, op, mut tool) = fixture();
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

    /// #62: a program's extents are its machine coordinates' range, arc
    /// bulges included. The sample project's origin is the stock's
    /// bottom-left corner and top face, so machine X and Y are project X
    /// and Y and machine Z is project Z minus 6.
    #[test]
    fn extents_include_arc_bulges() {
        let (project, op, mut tool) = fixture();
        tool.diameter_mm = 6.0;
        let program = emit(
            &project,
            &[(op, tool, cut_then_half_circle(10.0, 20.0, 40.0, 20.0))],
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

    fn emit_one(
        project: &Project,
        op: &ProfileOp,
        tool: &Tool,
        tp: Toolpath,
    ) -> Result<String, ExportError> {
        emit(project, &[(op.clone(), tool.clone(), tp)], "2026-10-15").map(|(p, _)| p.text)
    }

    /// Runs S1–S5 on hand-written `text` as if it were the whole of the
    /// fixture's operation.
    fn check_text(text: &str) -> Result<(), ExportError> {
        check_text_as(text, SpindleControl::Automatic)
    }

    /// [`check_text`] with spindle control set to `control`.
    fn check_text_as(text: &str, control: SpindleControl) -> Result<(), ExportError> {
        let (project, _, _) = fixture();
        check_text_on(text, project.post_processor, control)
    }

    /// [`check_text_as`] with the post-processor `post` in place of the
    /// fixture's.
    fn check_text_on(
        text: &str,
        post: PostProcessor,
        control: SpindleControl,
    ) -> Result<(), ExportError> {
        let (mut project, op, tool) = fixture();
        project.post_processor = post;
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
        check_rules(
            text,
            &project.post_processor,
            &transform,
            project.stock.thickness_mm.unwrap(),
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

    #[test]
    fn every_cut_has_active_feed() {
        let ok =
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM5\n";
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
            ("G91.1", "G91.1"),
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
        let ok = "G21 G90\nG54\nG0 Z5\nM0 (MSG, Turn the router ON.)\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM0 (MSG, Turn the router OFF.)\n";
        check_manual(ok).unwrap();

        let no_pause = "G21 G90\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(no_pause).unwrap_err();
        assert!(
            matches!(err, ExportError::RouterNotStarted { line: 5 }),
            "{err:?}"
        );

        // A tool-change pause, then the retract that must follow it: the
        // router still needs its own pause after the retract.
        let no_on_after_retract =
            "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(no_on_after_retract).unwrap_err();
        assert!(
            matches!(err, ExportError::RouterNotStarted { line: 7 }),
            "{err:?}"
        );

        let no_off = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nG0 Z5\n";
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
        let ok = "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        check_text_as(ok, SpindleControl::Automatic).unwrap();
        let manual_style = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_text_as(manual_style, SpindleControl::Automatic).unwrap_err();
        assert!(
            matches!(err, ExportError::SpindleNotRunning { line: 6 }),
            "{err:?}"
        );
        let no_dwell = "G21 G90\nG54\nG0 Z5\nM3 S18000\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
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
                    "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nM0\n"
                }
                SpindleControl::Manual => {
                    "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nM0\n"
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

    /// S4, #43: Z is not known after a pause, because the user may have
    /// fitted a tool and set Z zero. The spindle starts, and the router-on
    /// pause comes, only after a retract to safe Z.
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

        let manual_no_lift =
            "G21 G90\nG54\nG0 Z5\nM0\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        let err = check_manual(manual_no_lift).unwrap_err();
        assert!(
            matches!(err, ExportError::PauseNotAtSafeZ { line: 5 }),
            "{err:?}"
        );
        let manual_lifted = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nM0\nG21 G90 G54 X0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        check_manual(manual_lifted).unwrap();
    }

    /// S5, #55: the controller is in a known state before the program
    /// moves: units, work coordinates, and absolute distance mode. A
    /// controller left in `G91` by an earlier job reads every coordinate as
    /// an offset.
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

    /// S5, #55: at a pause the user may leave the controller in another
    /// state (`G91` to jog, `G20`, another work coordinate system), so
    /// units, work coordinates, and `G90` are set again after every pause
    /// before the program moves.
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

    /// #55: the same rule holds for a spindle start after a pause.
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

    /// #57: grblHAL's `M6` stops the program like `M0`, and the user fits
    /// the bit and sets Z zero there. So it must come at safe Z, Z is
    /// unknown after it, and the setup is written again before the program
    /// moves or starts the spindle.
    #[test]
    fn m6_counts_as_a_pause() {
        let check = |text: &str| {
            let grblhal = crate::post::lookup("grblhal").unwrap();
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
    /// operation with `toolpath`.
    fn check_against(text: &str, toolpath: &Toolpath) -> Result<(), ExportError> {
        let (project, op, tool) = fixture();
        let transform = MachineTransform::new(&project).unwrap();
        let spans = [OpSpan {
            op: &op,
            tool: &tool,
            toolpath,
            lines: 1..text.lines().count() + 1,
        }];
        check(
            text,
            &project.post_processor,
            &transform,
            &project.stock,
            project.stock.thickness_mm.unwrap(),
            project.flatten_tolerance_mm,
            &spans,
        )
        .map(|_| ())
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

        let blocks = parse_program(text).unwrap();
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

    /// S6, #60: G2 and G3 have the same radius check, so only S6 sees an
    /// arc that runs the other way round.
    #[test]
    fn reversed_arc_fails_reproduction() {
        let text = line_then_arc_text("G2 X20 Y10 I0 J10\n");
        let result = check_against(&text, &line_then_arc());
        assert!(is_mismatch_at(&result, 3), "{result:?}");
    }

    /// S6, #60: the same end points about a centre on the other side of the
    /// chord.
    #[test]
    fn wrong_arc_centre_fails() {
        let text = line_then_arc_text("G3 X20 Y10 I10 J0\n");
        let result = check_against(&text, &line_then_arc());
        assert!(is_mismatch_at(&result, 3), "{result:?}");
    }

    /// S6, #60: a move between two toolpath points that the toolpath does
    /// not have.
    #[test]
    fn extra_move_between_points_fails() {
        let text = line_then_arc_text("G3 X20 Y10 I0 J10\n")
            .replace("X10 F1000\n", "X5 Y3 F1000\nX10 Y0\n");
        let result = check_against(&text, &line_then_arc());
        assert!(is_mismatch_at(&result, 2), "{result:?}");
        // After the toolpath's last move, only a retract may follow.
        let text = line_then_arc_text("G3 X20 Y10 I0 J10\n").replace("M5\n", "X30\nM5\n");
        let result = check_against(&text, &line_then_arc());
        assert!(is_mismatch_at(&result, 5), "{result:?}");
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

    /// S6, #60: an arc written as short straight moves, as with
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
        let (mut project, op, tool) = fixture();
        for (interpolate, format, x_axis) in [
            (false, ArcFormat::Ij, Axis::X),
            (true, ArcFormat::Ij, Axis::X),
            (true, ArcFormat::R, Axis::X),
            (true, ArcFormat::Ij, Axis::NegX),
            (true, ArcFormat::R, Axis::NegX),
        ] {
            let motion = &mut project.post_processor.motion;
            motion.arc_interpolation = interpolate;
            motion.arc_format = format;
            project.post_processor.coordinate_system.x_axis = x_axis;
            let (toolpath, _) = generate_profile(&project, &op).unwrap();
            emit_one(&project, &op, &tool, toolpath).unwrap();
            emit_one(&project, &op, &tool, line_then_arc()).unwrap();
        }
    }

    /// Mutation guard, #60: an emitter that writes every arc the other way
    /// round fails its own safety pass.
    #[test]
    fn flipped_arcs_fail_emit() {
        let (project, op, tool) = fixture();
        crate::emit::FLIP_ARCS.set(true);
        let result = emit_one(&project, &op, &tool, line_then_arc());
        crate::emit::FLIP_ARCS.set(false);
        assert!(
            matches!(result, Err(ExportError::MotionMismatch { index: 3, .. })),
            "{result:?}"
        );
        emit_one(&project, &op, &tool, line_then_arc()).unwrap();
    }
}
