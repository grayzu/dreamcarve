// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The safety pass: parse the emitted program and check the safety rules
//! on what the controller will read (`docs/specs/phase-1/design.md`,
//! "Emission", step 7; `docs/specs/phase-1/functional.md`, S1–S7). Every
//! violation is an [`ExportError`]; there are no safety warnings.
//!
//! Checks run in project space: each parsed point is mapped back through
//! the [`MachineTransform`] using the units the program itself selected,
//! so a program that departs from the transform, or sets no units, shows
//! up as a violation instead of being trusted. S6 compares each
//! operation's moves with its toolpath move for move, arc direction and
//! centre included; see [`check_reproduces`] for exactly what it accepts.
//!
//! **What the pass shares with the emitter.** It reads the text, not the
//! emitter's state, but an error in a piece both use passes both:
//!
//! - the machine transform: the emitter builds one [`MachineTransform`]
//!   and hands it to the pass, so a wrong origin or axis map in the
//!   transform itself is not caught here;
//! - the canned-cycle model: the reader expands a cycle line with
//!   [`crate::canned`], by which the emitter decides that a cycle's
//!   motion is the toolpath;
//! - the arc helpers: [`crate::arcfit`]'s chord and shallow-arc measures,
//!   the centre an `R` arc is read with ([`crate::parse::r_centre`]), and
//!   [`descent_rate_mm_min`], by which the emitter writes the feeds S10
//!   then checks;
//! - how the definition is read: [`changes_tools_in_program`],
//!   [`controller_sets_units`], [`code_words`].
//!
//! Its own: the reader ([`crate::parse`], standard G-code whatever the
//! definition says), the modal rules (safe Z, depth, feeds, spindle,
//! setup), S6's matching, S7, and the material each operation may cut
//! ([`crate::material`], from the project). The simulator tools
//! (`tools/rs274-motion`, `tools/grbl-sim-motion`, `tools/linuxcnc-sim`)
//! are the check that shares none of this.
//!
//! Every cut belongs to an operation: S3's rates, S6, S7's tool radius,
//! S9 and S10 all hold a cut by the operation whose lines it is on, so a
//! `G1`, `G2` or `G3` on a line of no operation is
//! [`ExportError::CutOutsideOperation`]. DreamCarve writes none; only a
//! definition's header or footer line could be one.
//!
//! The pass fails closed ([`crate::material`], "Fail closed"): a lookup
//! into its own data that misses is [`ExportError::SafetyCheckDefect`],
//! never an empty answer that an `all(..)` would read as a pass.
//!
//! The later rules (`docs/specs/phase-2/functional.md`, S9 to S16;
//! `docs/specs/phase-3/functional.md`, S17 to S19):
//!
//! - S9, S17 to S19: every cut stays in its operation's own material,
//!   worked out from the project ([`crate::material`]).
//! - S10: every descending cut falls no faster than its tool's plunge
//!   rate, its slope recomputed from the written numbers, with nothing
//!   allowed for rounding ([`check_descent_rate`]).
//! - S11: a canned-cycle line is checked as the moves the controller
//!   makes for it, which the parser writes as synthetic blocks. A series
//!   of cycles must end with `G80` before any other motion: the parser
//!   models `G98`'s return level only from one `G80` to the next.
//! - S16: a `G53 G0 Z` line is a machine-coordinate lift, to the
//!   project's tool-change machine Z from at or above safe Z, right
//!   before a stop or the end; work Z is unknown after it.

use std::collections::HashSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::ops::Range;

use dc_cam::entry::{CLEARED_POINT_TOLERANCE_MM, ENTRY_CLEARANCE_MM};
use dc_cam::{Move, MoveKind, Toolpath};
use dc_geometry::{Arc, EPS_MM, Point, Point3, Segment, approx_eq, point_segment_distance_mm};
use dc_project::{
    DwellUnits, Operation, PostProcessor, SpindleControl, Stock, Tool, ToolId, Units,
};

use crate::ExportError;
use crate::arcfit::{MIN_RUN_CHORDS, arc_chord_deviation_mm, shallow_arc_midpoint};
use crate::emit::{changes_tools_in_program, code_words, controller_sets_units};
use crate::material::{self, Material, TabSpine};
use crate::parse::{
    Block, CycleModel, MachineMove, MachinePosition, MotionKind, Word, is_code, parse_program_with,
};
use crate::transform::{MachineTransform, mm_per_unit};

/// The longest line, in characters, DreamCarve emits. grblHAL's line
/// buffer holds 256 bytes including the terminator.
pub const MAX_LINE_CHARS: usize = 255;

/// The error for a lookup into the pass's own data that missed, naming
/// the `check` that could not be finished: see [`crate::material`]'s
/// module docs, "Fail closed".
pub(crate) fn defect(check: &'static str) -> ExportError {
    ExportError::SafetyCheckDefect { check }
}

/// One emitted operation and the program lines its moves occupy.
#[derive(Debug, Clone)]
pub(crate) struct OpSpan<'a> {
    /// The operation.
    pub op: &'a Operation,
    /// Its tool.
    pub tool: &'a Tool,
    /// Its toolpath, in project space.
    pub toolpath: &'a Toolpath,
    /// The material it may cut, from the project (S9).
    pub material: &'a Material,
    /// The middles of the tabs it must leave standing, from the project
    /// (S9).
    pub tabs: &'a [TabSpine],
    /// 1-based program lines, end exclusive, from the operation's first
    /// motion line to its last.
    pub lines: Range<usize>,
}

/// The project's heights the program was written for.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct ProgramHeights {
    /// `heights.tool_change_machine_z_mm`, the only machine Z a `G53` may
    /// lift to (S16); `None` refuses every `G53`.
    pub machine_lift_mm: Option<f64>,
    /// The clearance height, absolute project Z, mm, when set: each
    /// operation's first move across the work, every move across outside
    /// the operations (the park move) and every stop are at or above it.
    pub clearance_z_mm: Option<f64>,
}

/// How far the program may depart from the toolpath by design.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tolerances {
    /// The most a straight move written for an arc may cut inside it.
    pub flatten_mm: f64,
    /// The most a vertex of a run of straight moves may lie from the arc
    /// written for the run (S12); `None` when arc fitting is off, so no
    /// arc may stand for straight moves.
    pub arc_fit_mm: Option<f64>,
}

/// Checks `text`, emitted with `post` and `transform` for the operations
/// in `spans`, against the rules of the pass (module docs), and returns
/// the program's extents with the blocks it parsed into, so the emitter
/// reads its pauses without parsing the program again. `heights` are the
/// project's clearance height and tool-change machine Z (S2, S16).
///
/// # Errors
///
/// The first violation found, as the matching [`ExportError`].
#[allow(
    clippy::too_many_arguments,
    reason = "the program, how it was written, and what it was written for, each needed"
)]
pub(crate) fn check_parsed(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock: &Stock,
    stock_thickness_mm: f64,
    tolerances: Tolerances,
    heights: ProgramHeights,
    spans: &[OpSpan<'_>],
) -> Result<(Extents, Vec<Block>), ExportError> {
    let blocks = check_but_material(
        text,
        post,
        transform,
        stock,
        stock_thickness_mm,
        tolerances,
        heights,
        spans,
    )?;
    // S9: every cut stays in its operation's own material; S17 and S18
    // hold a V-bit's run and a countersink there too.
    for span in spans {
        material::check(&blocks, post, transform, tolerances, span)?;
    }
    Ok((extents(&blocks, transform.output_units()), blocks))
}

/// Every rule of [`check_parsed`] but those that hold a cut to its
/// operation's material ([`material::check`]). Nothing here reads a
/// span's `material` but for S1's floor. Returns the blocks `text` parsed
/// into.
#[allow(
    clippy::too_many_arguments,
    reason = "the program, how it was written, and what it was written for, each needed"
)]
fn check_but_material(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock: &Stock,
    stock_thickness_mm: f64,
    tolerances: Tolerances,
    heights: ProgramHeights,
    spans: &[OpSpan<'_>],
) -> Result<Vec<Block>, ExportError> {
    let blocks = check_rules(
        text,
        post,
        transform,
        stock_thickness_mm,
        tolerances,
        heights,
        spans,
    )?;
    // S6: reading the program back reproduces each toolpath.
    for span in spans {
        check_reproduces(span, &blocks, post, transform, tolerances)?;
    }
    // S7: every cut stays on the stock.
    check_on_stock(&blocks, post, transform, stock, stock_thickness_mm, spans)?;
    Ok(blocks)
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
/// position it moves through, arcs' bulges included. Z min is the
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
    let Some(centre) = mv.arc_centre() else {
        return Vec::new();
    };
    let mut arc = Arc::from_ends(Point::new(sx, sy), Point::new(ex, ey), centre, !machine_cw);
    // Fail-closed: an arc that ends where it starts is a full circle to
    // the controller, though `check_not_full_circle` rejects it first.
    if arc.sweep_rad.abs() < MIN_ARC_SWEEP_RAD {
        arc.sweep_rad = TAU;
    }
    arc.axis_extremes().collect()
}

/// S7: every cutting move, widened by its operation's tool radius, stays
/// inside the stock rectangle `[0, width] × [0, height]`, within one unit
/// in the last written place. Arcs count to their bulge. A cut outside
/// every operation has no tool radius to be widened by, and is refused
/// ([`ExportError::CutOutsideOperation`]; [`check_rules`] refuses it
/// first). A Photo V-Carve's cut is widened at each end by its groove's
/// half-width at the stock top there, `(thickness − z) · tan(α/2)`, from 0
/// above the stock to the tool's radius ([`groove_radius_mm`]): its
/// grooves may run past the picture's edge, and this is the rule that
/// keeps them on the stock.
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
        let Some(span) = spans.iter().find(|s| s.lines.contains(&mv.line)) else {
            return Err(ExportError::CutOutsideOperation { line: mv.line });
        };
        let (op, tool) = (span.op, span.tool);
        let full_mm = tool.cut_radius_mm();
        let radius_at = |z: Option<f64>| {
            groove_radius_mm(
                op,
                tool,
                stock_thickness_mm,
                z.map(|z| transform.project_z(z, mv.units)),
            )
            .unwrap_or(full_mm)
        };
        let tolerance_mm = quantum_mm(post, mv.units) + EPS_MM;
        let widest_mm = radius_at(mv.from.z).max(radius_at(mv.to.z));
        let ends = [mv.from, mv.to]
            .into_iter()
            .filter_map(|p| Some((Point::new(p.x?, p.y?), radius_at(p.z))));
        let extremes = arc_extremes(mv).into_iter().map(|m| (m, widest_mm));
        for (m, radius_mm) in ends.chain(extremes) {
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
                    op: Some(op.id().clone()),
                    line: mv.line,
                    side,
                    overhang_mm,
                });
            }
        }
    }
    Ok(())
}

/// The half-width, mm, at the stock top of a Photo V-Carve's groove whose
/// tip is at project `z_mm` (S7): `(thickness − z) · tan(α/2)`, from 0
/// above the stock to the tool's radius. `None` for a line of any other
/// operation, a tool that is not a V-bit of a usable angle, or an unknown
/// Z, where the full radius holds.
fn groove_radius_mm(
    op: &Operation,
    tool: &Tool,
    stock_thickness_mm: f64,
    z_mm: Option<f64>,
) -> Option<f64> {
    let Operation::PhotoVCarve(_) = op else {
        return None;
    };
    // The Photo's groove follows a V-bit's angle (S19), so a tapered ball
    // nose, which validation refuses there, is unresolved too.
    let Some(dc_geometry::profile::BitProfile::VBit { slope }) = tool.carve_profile() else {
        return None;
    };
    let full_mm = tool.diameter_mm / 2.0;
    let z_mm = z_mm?;
    z_mm.is_finite()
        .then(|| ((stock_thickness_mm - z_mm) * slope).clamp(0.0, full_mm))
}

/// Checks `text` against S1–S5, S10 and S16 and returns its blocks.
fn check_rules(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock_thickness_mm: f64,
    tolerances: Tolerances,
    heights: ProgramHeights,
    spans: &[OpSpan<'_>],
) -> Result<Vec<Block>, ExportError> {
    check_characters(text)?;
    let machine_lift_mm = heights.machine_lift_mm;
    // S11: canned cycles are expanded into the moves the controller makes,
    // and every rule below checks those.
    // A controller without units words (UCCNC) reads the program in the
    // units it is set up in, which `emit` has warned must be the
    // program's.
    let controller_units = controller_sets_units(post);
    let model = CycleModel {
        units: transform.output_units(),
        ..CycleModel::of(post)
    };
    let blocks = parse_program_with(text, &model)?;
    // S16: `G53` only from a definition that documents it, and only when
    // the project sets the height it lifts to.
    let with_g53 = machine_lift_mm.is_some() && post.work_coordinates.machine_command.is_some();
    check_words(
        &blocks,
        changes_tools_in_program(post),
        writes_canned_cycles(post),
        with_g53,
        &header_cancel_codes(post),
    )?;

    let dwell_s = post.feed_speed.spindle_on_dwell.duration_s;
    let manual = post.feed_speed.spindle_control == SpindleControl::Manual;
    let mut router = RouterPauses::default();
    let mut units_set = controller_units;
    let mut wcs_set = false;
    let mut absolute_set = false;
    // The last pause: the controller's modal state must be set again after
    // it, because the user may change it at the controller.
    let mut last_pause: Option<usize> = None;
    let mut spindle_on = false;
    let mut rpm: Option<f64> = None;
    let mut dwell_pending = false;
    // S15: the deepest a cut has reached at the XY of the last cut.
    let mut cut_to: Option<CutTo> = None;
    // S2: the cuts of the operation being read.
    let mut floors = Floors::new(stock_thickness_mm, tolerances);
    // S16: the line of the last machine-coordinate lift, while no motion
    // has followed it, and whether a stop has come since.
    let mut lifted: Option<(usize, bool)> = None;
    // S2: the span of the last move, and whether it has moved across
    // the work yet.
    let mut crossing: (Option<usize>, bool) = (None, false);
    for block in &blocks {
        let line = block.line;
        // S16: `G53 G0 Z<v>`, to the project's tool-change machine Z,
        // from a known Z at or above safe Z, where the bit already is.
        // Work Z is unknown after it, as after a pause.
        if let Some(z_mm) = block.machine_lift_mm {
            let Some(expected_mm) = machine_lift_mm else {
                return Err(ExportError::ForbiddenWord {
                    line,
                    word: "G53".to_owned(),
                });
            };
            let tolerance_mm = quantum_mm(post, transform.output_units()) + EPS_MM;
            if z_mm >= 0.0 || (z_mm - expected_mm).abs() > tolerance_mm {
                return Err(ExportError::MachineLiftWrongZ {
                    line,
                    z_mm,
                    expected_mm,
                });
            }
            // From a known Z at or above safe Z, or again right after a stop
            // that followed a lift, with nothing moved since: the user may
            // have jogged at that stop (manual spindle control: the
            // router-off pause, then the tool change).
            let safe_z_mm = Limits::for_line(line, spans, stock_thickness_mm).safe_z_mm;
            let relift = matches!(lifted, Some((_, true)));
            if !(relift || router.at_or_above(safe_z_mm)) {
                return Err(ExportError::MachineLiftNotAtSafeZ { line });
            }
            // S5: the Z word is in the units in force, which a user may
            // have changed at a pause.
            if !units_set {
                return Err(match last_pause {
                    Some(pause_line) => ExportError::MissingSetupAfterPause {
                        line,
                        pause_line,
                        what: "units (G20 or G21)",
                    },
                    None => ExportError::MissingSetup {
                        what: "units (G20 or G21)",
                    },
                });
            }
            router.z_mm = None;
            lifted = Some((line, false));
            continue;
        }
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
        // S4: the user fits tools and switches the router at a pause or an
        // `M6` tool change, so it comes with the bit at safe Z. Afterwards
        // Z is unknown: the user may have jogged or set Z zero.
        let changes_tool = block.has('M', 6.0);
        if block.has('M', 0.0) || block.has('M', 1.0) || changes_tool {
            // A stop is at the clearance height when it is set.
            let safe_z_mm = Limits::for_line(line, spans, stock_thickness_mm)
                .safe_z_mm
                .max(heights.clearance_z_mm.unwrap_or(f64::NEG_INFINITY));
            // S16: or at the machine-coordinate lift, which nothing has
            // moved from since.
            if let Some((_, stopped)) = &mut lifted {
                *stopped = true;
            } else if !router.at_or_above(safe_z_mm) {
                return Err(ExportError::PauseNotAtSafeZ { line });
            }
            if manual {
                if changes_tool {
                    router.tool_changed();
                } else {
                    router.pause();
                }
            }
            router.z_mm = None;
            last_pause = Some(line);
            units_set = controller_units;
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
            // Z is known again only after a retract since the last
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
                .is_some_and(|p| dwell_seconds(post, p) >= dwell_s - DWELL_TOLERANCE_S)
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
        floors.enter(spans.iter().position(|s| s.lines.contains(&line)));
        let tolerance_mm = quantum_mm(post, mv.units);
        // Z after this move. A line without a Z word leaves Z where it was,
        // which after a pause is unknown, whatever Z the program last
        // wrote.
        let z_after = match (block.value('Z'), mv.to.z) {
            (Some(_), Some(z)) => Some((transform.project_z(z, mv.units), tolerance_mm)),
            _ => router.z_mm,
        };
        check_depth(mv, &limits, transform, tolerance_mm)?;
        if mv.kind.is_cut() {
            // Review 1.10: a cut is some operation's, or no rule below,
            // nor S6, S7 or S9, has a tool or a toolpath to hold it by.
            let Some((op, tool)) = limits.op.zip(limits.tool) else {
                return Err(ExportError::CutOutsideOperation { line });
            };
            if manual {
                router.cut(line, &tool.id)?;
            } else {
                if !spindle_on || rpm.is_none_or(|s| s <= 0.0) {
                    return Err(ExportError::SpindleNotRunning { line });
                }
                if dwell_pending {
                    return Err(ExportError::SpindleNotUpToSpeed { line });
                }
            }
            check_feed(mv, op, tool, 2.0 * feed_quantum_mm_min(post, mv.units))?;
            check_descent_rate(mv, op, tool, transform)?;
            check_arc(mv)?;
            check_not_full_circle(mv, post)?;
            floors.cut(mv, transform, router.z_mm, z_after);
            if let (Some(x), Some(y), Some((z_mm, _))) = (mv.to.x, mv.to.y, z_after) {
                let deepest_mm = cut_to
                    .filter(|cut| cut.is_at(x, y))
                    .map_or(z_mm, |cut| cut.z_mm.min(z_mm));
                cut_to = Some(CutTo {
                    x,
                    y,
                    z_mm: deepest_mm,
                });
            }
        } else if let Some(below_safe) = z_after.filter(|&(z_mm, tolerance_mm)| {
            matches!(limits.op, Some(Operation::Drill(_)))
                && !differs(mv.from.x, mv.to.x)
                && !differs(mv.from.y, mv.to.y)
                && z_mm < limits.safe_z_mm - tolerance_mm
        }) {
            check_drill_rapid(mv, below_safe, stock_thickness_mm, cut_to)?;
        } else if !floors.allow_approach(mv, transform, router.z_mm, z_after, limits.safe_z_mm) {
            check_rapid(mv, &limits, router.z_mm, z_after)?;
        }
        // S2: with a clearance height, an operation's first move across
        // the work and every one outside the operations run at or above
        // it. Links within an operation are held to the toolpath by S6,
        // whose sequencer links parts at it.
        let span = spans.iter().position(|s| s.lines.contains(&line));
        if crossing.0 != span || span.is_none() {
            crossing = (span, false);
        }
        let crosses = mv.kind == MotionKind::Rapid
            && (differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y));
        if let Some(clearance_z_mm) = heights.clearance_z_mm
            && crosses
            && !crossing.1
        {
            let below = |z: Option<(f64, f64)>| match z {
                Some((z_mm, tolerance_mm)) => {
                    (z_mm < clearance_z_mm - tolerance_mm).then_some(Some(z_mm))
                }
                None => Some(None),
            };
            if let Some(z_mm) = below(router.z_mm).or_else(|| below(z_after)) {
                return Err(ExportError::RapidBelowClearance {
                    line,
                    z_mm,
                    clearance_z_mm,
                });
            }
        }
        if crosses && span.is_some() {
            crossing.1 = true;
        }
        // S16: a lift comes only right before a stop or the end.
        if let Some((lift_line, stopped)) = lifted.take()
            && !stopped
        {
            return Err(ExportError::MachineLiftNotBeforeStop { line, lift_line });
        }
        router.moved(z_after);
    }
    if spindle_on {
        return Err(ExportError::SpindleNotStopped);
    }
    if manual && router.cut_since_pause {
        return Err(ExportError::RouterNotStopped);
    }
    Ok(blocks)
}

/// Where Z is, for the S2 and S4 checks in both spindle modes, and, with
/// manual spindle control, what the program has done since its last cut.
/// Z is unknown at program start and after every pause (`M0`, `M1`, `M6`)
/// until a move writes Z again; a rapid, a pause, and a spindle start all
/// read this one state. A tool's first cut needs an `M0` or `M1` after
/// the last cut and the last `M6`: the user fits a tool at an `M6`, so an
/// `M6` never counts as the pause to turn the router on.
#[derive(Debug, Default)]
struct RouterPauses<'a> {
    /// Z above stock bottom, mm, and its rounding tolerance, if known.
    z_mm: Option<(f64, f64)>,
    /// An `M0` or `M1` (at safe Z) has come after the last cut and the
    /// last `M6`.
    paused_at_safe_z: bool,
    /// A cut has come after the last `M0` or `M1`.
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

    /// An `M0` or `M1` at safe Z.
    fn pause(&mut self) {
        self.paused_at_safe_z = true;
        self.cut_since_pause = false;
    }

    /// An `M6` at safe Z: a pause before it does not count for the next
    /// tool's first cut.
    fn tool_changed(&mut self) {
        self.paused_at_safe_z = false;
    }

    /// A cut on `line` with `tool`. A tool's first cut needs a pause at
    /// safe Z since the previous cut, for the user to switch the router on.
    fn cut(&mut self, line: usize, tool: &'a ToolId) -> Result<(), ExportError> {
        let first_of_tool = self.tool != Some(tool);
        if first_of_tool && !self.paused_at_safe_z {
            return Err(ExportError::RouterNotStarted { line });
        }
        self.tool = Some(tool);
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
/// DreamCarve never sends to GRBL or grblHAL (`dc-project/src/definitions/README.md`,
/// "Verification"): `M4` needs a direction pin or laser mode, `M7` is
/// optional on both, and `G28` and `G30` move in machine coordinates.
/// `G53` is accepted only for S16's lift ([`check_words`]). `M6` is accepted only from a definition whose tool change
/// uses it and whose tool-change mode is `"m6"` ([`TOOL_CHANGE_M`]).
///
/// Four codes only cancel a mode DreamCarve never turns on, or select the
/// one it assumes, so they are accepted always (#154; LinuxCNC G-code
/// reference, "G40", "G49", "G80", "G90.1, G91.1"; Using Mach3Mill 1.84,
/// 10.7.13, 10.7.14, 10.7.23, 10.7.26): `G40` cutter compensation off,
/// `G49` tool length offset off, `G80` no canned cycle (the parser then
/// has no motion mode until the next `G0` to `G3`), and `G91.1`
/// incremental arc centres, which is how DreamCarve writes `I` and `J`.
/// `G90.1` stays out.
const ALLOWED_G: [f64; 20] = [
    0.0, 1.0, 2.0, 3.0, 4.0, 17.0, 20.0, 21.0, 40.0, 49.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0,
    80.0, 90.0, 91.1, 94.0,
];
/// See [`ALLOWED_G`].
const ALLOWED_M: [f64; 6] = [0.0, 1.0, 2.0, 3.0, 5.0, 30.0];
/// The drilling canned cycles and their retract modes, accepted when the
/// project asks for canned cycles and the definition has them
/// ([`writes_canned_cycles`]). The parser expands each cycle into the
/// moves the controller makes, which the rules then check (S11). Their
/// cancel, `G80`, is in [`ALLOWED_G`].
const CANNED_G: [f64; 6] = [73.0, 81.0, 82.0, 83.0, 98.0, 99.0];

/// Whether a program for `post` may hold canned cycles: the project asks
/// for them (`use_canned_cycles`) and the definition has them.
pub(crate) fn writes_canned_cycles(post: &PostProcessor) -> bool {
    post.use_canned_cycles && post.canned_cycles.is_some()
}
/// `G53`, accepted on a machine-coordinate lift only (S16).
const MACHINE_G: [f64; 1] = [53.0];
/// `G50`, every scale factor back to 1, and `G69`, no coordinate rotation,
/// as Mach3 and UCCNC define them (#312; Using Mach3Mill 1.84, 10.7.15,
/// 10.7.20; UCCNC user's manual 1.0047, 4.1.1), and `G50` as grblHAL
/// defines it (#316; `grblHAL/core` `gcode.c` 1766-1770, 2798-2799). Each
/// only cancels a mode DreamCarve never turns on, but it is accepted only
/// from a definition whose header writes it ([`header_cancel_codes`]):
/// LinuxCNC, GRBL and RS274/NGC do not have them, grblHAL has no `G69`,
/// and in other dialects `G50` is a different command. `G51` and `G68`, which scale and rotate every
/// coordinate the program writes, are never accepted: this pass reads the
/// program as written.
const HEADER_CANCEL_G: [(&str, f64); 2] = [("G50", 50.0), ("G69", 69.0)];

/// The codes of [`HEADER_CANCEL_G`] that `post`'s header writes.
fn header_cancel_codes(post: &PostProcessor) -> Vec<f64> {
    HEADER_CANCEL_G
        .iter()
        .filter(|(word, _)| {
            post.program
                .header
                .iter()
                .any(|line| code_words(line).iter().any(|w| w == word))
        })
        .map(|&(_, code)| code)
        .collect()
}
/// The `M` codes of grblHAL's tool change, accepted when the definition's tool
/// change uses `M6` and its mode is `"m6"`: `M6` itself, checked like a pause,
/// and `M61`, which sets the current tool number without a stop or a move. GRBL
/// 1.1 has neither.
const TOOL_CHANGE_M: [f64; 2] = [6.0, 61.0];

/// A dwell shorter than the configured one by less than this still counts:
/// it absorbs the rounding of the written seconds.
pub(crate) const DWELL_TOLERANCE_S: f64 = 0.0005;

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
/// The codes in [`TOOL_CHANGE_M`] are allowed too when the program may
/// change tools with `M6` (`with_m6`), those in [`CANNED_G`] when it
/// may hold canned cycles (`with_canned`), and `G53` on the lines the
/// parser read as a machine-coordinate lift when the definition documents
/// it and the project sets the height (`with_g53`, S16). The definition's
/// own header cancels, `header_g` ([`header_cancel_codes`]), are allowed
/// only where [`check_header_cancel`] places them.
fn check_words(
    blocks: &[Block],
    with_m6: bool,
    with_canned: bool,
    with_g53: bool,
    header_g: &[f64],
) -> Result<(), ExportError> {
    let tool_change: &[f64] = if with_m6 { &TOOL_CHANGE_M } else { &[] };
    let canned: &[f64] = if with_canned { &CANNED_G } else { &[] };
    // True in the header, before the first move or spindle start, and
    // again after each stop until the next one: the two places `emit`
    // writes the setup lines.
    let mut in_setup = true;
    for block in blocks {
        let lift: &[f64] = if with_g53 && block.machine_lift_mm.is_some() {
            &MACHINE_G
        } else {
            &[]
        };
        for word in &block.words {
            if word.letter == 'G' && header_g.iter().any(|&code| is_code(word.value, code)) {
                check_header_cancel(block, word, in_setup)?;
                continue;
            }
            let (allowed, extra): (&[f64], &[f64]) = match word.letter {
                'G' if !lift.is_empty() => (&ALLOWED_G, lift),
                'G' => (&ALLOWED_G, canned),
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
        if block.motion.is_some() || block.machine_lift_mm.is_some() || block.has('M', 3.0) {
            in_setup = false;
        }
        if block.has('M', 0.0) || block.has('M', 1.0) || (with_m6 && block.has('M', 6.0)) {
            in_setup = true;
        }
    }
    Ok(())
}

/// `word`, one of the definition's header cancels (`G50`, `G69`), is
/// accepted only as `emit` writes it: alone on its line, but for a line
/// number, so no axis, feed, speed or other value word can ride with it,
/// and only in the header or in the setup after a stop (`in_setup`). On a
/// dialect where `G50` takes axis words it is a different command, so
/// anything else is refused.
fn check_header_cancel(block: &Block, word: &Word, in_setup: bool) -> Result<(), ExportError> {
    let alone = block.words.iter().filter(|w| w.letter != 'N').count() == 1;
    let reason = if !alone {
        Some("it shares its line with other words")
    } else if !in_setup {
        Some("it is outside the header and the setup after a stop")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(ExportError::MisplacedSetupWord {
            line: block.line,
            word: format!("{}{}", word.letter, word.value),
            reason,
        }),
        None => Ok(()),
    }
}

/// One output unit in the last written decimal place of a feed, in
/// mm/min. A feed is written with the same number format as a length, so
/// this is [`quantum_mm`]'s number in rate units.
fn feed_quantum_mm_min(post: &PostProcessor, units: Units) -> f64 {
    quantum_mm(post, units)
}

/// One output unit in the last written decimal place, in mm: the one
/// `dc_cam::entry` keeps a helix's moves above.
pub(crate) fn quantum_mm(post: &PostProcessor, units: Units) -> f64 {
    post.number_format.decimal_places.quantum_mm(units)
}

/// The rules in force on one line, project space, mm.
struct Limits<'a> {
    op: Option<&'a Operation>,
    tool: Option<&'a Tool>,
    /// Lowest permitted Z: stock bottom minus through-cut allowance.
    floor_mm: f64,
    /// Safe Z.
    safe_z_mm: f64,
}

impl<'a> Limits<'a> {
    /// The limits of the operation that owns `line`; outside every
    /// operation, the strictest of them all, with no operation and no
    /// tool: only a rapid may be there
    /// ([`ExportError::CutOutsideOperation`]).
    fn for_line(line: usize, spans: &[OpSpan<'a>], stock_thickness_mm: f64) -> Self {
        if let Some(span) = spans.iter().find(|s| s.lines.contains(&line)) {
            return Self {
                op: Some(span.op),
                tool: Some(span.tool),
                floor_mm: span.material.floor_mm(span.op),
                safe_z_mm: stock_thickness_mm + span.op.safe_z_mm(),
            };
        }
        let floor_mm = spans
            .iter()
            .map(|s| s.material.floor_mm(s.op))
            .fold(0.0, f64::max);
        let safe_z_mm = spans
            .iter()
            .map(|s| stock_thickness_mm + s.op.safe_z_mm())
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
            op: limits.op.map(|op| op.id().clone()),
            line: mv.line,
            z_mm,
            floor_mm: limits.floor_mm,
        });
    }
    Ok(())
}

/// S2: a rapid ends at or above safe Z, and one that moves in XY also
/// starts there. `from_z_mm` and `to_z_mm` are Z above stock bottom and
/// its rounding tolerance before and after the move, `None` where Z is
/// unknown: at program start and after a pause until Z is written.
fn check_rapid(
    mv: &MachineMove,
    limits: &Limits<'_>,
    from_z_mm: Option<(f64, f64)>,
    to_z_mm: Option<(f64, f64)>,
) -> Result<(), ExportError> {
    let below = |z: Option<(f64, f64)>| -> Option<Option<f64>> {
        let Some((z_mm, tolerance_mm)) = z else {
            return Some(None);
        };
        (z_mm < limits.safe_z_mm - tolerance_mm).then_some(Some(z_mm))
    };
    let moves_xy = differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y);
    let offending = below(to_z_mm).or_else(|| if moves_xy { below(from_z_mm) } else { None });
    match offending {
        Some(z_mm) => Err(ExportError::RapidBelowSafeZ {
            line: mv.line,
            z_mm,
            safe_z_mm: limits.safe_z_mm,
        }),
        None => Ok(()),
    }
}

/// S2: a rapid straight down from safe Z may stop below it, over the start
/// of a ramp or helix, where the tool is in the air: at or above the stock
/// top plus [`ENTRY_CLEARANCE_MM`], or that far above the lowest Z a cut
/// of the same operation has reached through its XY. The cuts of the
/// operation being read, in project space, are kept to find that Z.
struct Floors {
    /// The stock top plus the entry clearance, mm above stock bottom.
    above_stock_mm: f64,
    /// How far a cut may pass from the rapid's XY and still count, less
    /// the program's rounding: dc-cam's own reach plus the arcs the
    /// program may write for straight moves and the reverse.
    reach_mm: f64,
    /// Which span the cuts belong to.
    span: Option<usize>,
    /// Each cut's tool-centre path in XY and its Z at start and end, mm.
    cuts: Vec<(Segment, f64, f64)>,
}

impl Floors {
    fn new(stock_thickness_mm: f64, tolerances: Tolerances) -> Self {
        Self {
            above_stock_mm: stock_thickness_mm + ENTRY_CLEARANCE_MM,
            reach_mm: CLEARED_POINT_TOLERANCE_MM
                + tolerances.flatten_mm
                + tolerances.arc_fit_mm.unwrap_or(0.0)
                + EPS_MM,
            span: None,
            cuts: Vec::new(),
        }
    }

    /// The line read is in `span`: a new operation starts with no cuts.
    fn enter(&mut self, span: Option<usize>) {
        if span != self.span {
            self.span = span;
            self.cuts.clear();
        }
    }

    /// Records cut `mv`, from `from_z` to `to_z` (Z above stock bottom
    /// and its rounding tolerance, as written; skipped where either or its
    /// XY is unknown).
    fn cut(
        &mut self,
        mv: &MachineMove,
        transform: &MachineTransform,
        from_z: Option<(f64, f64)>,
        to_z: Option<(f64, f64)>,
    ) {
        if let (Some(path), Some((from_mm, _)), Some((to_mm, _))) =
            (material::project_path(mv, transform), from_z, to_z)
        {
            self.cuts.push((path, from_mm, to_mm));
        }
    }

    /// Whether `mv`, a rapid, is one S2 allows below safe Z for an entry:
    /// straight down in Z alone from (`from_z`) at or above `safe_z_mm`,
    /// ending (`to_z`) at or above the stock top plus the entry clearance,
    /// or at or above the entry clearance over the lowest Z a cut of this
    /// operation reached within [`Self::reach_mm`] of its XY (plus the
    /// rounding of the numbers either side).
    fn allow_approach(
        &self,
        mv: &MachineMove,
        transform: &MachineTransform,
        from_z: Option<(f64, f64)>,
        to_z: Option<(f64, f64)>,
        safe_z_mm: f64,
    ) -> bool {
        let (Some((from_mm, from_tolerance_mm)), Some((z_mm, tolerance_mm))) = (from_z, to_z)
        else {
            return false;
        };
        if differs(mv.from.x, mv.to.x)
            || differs(mv.from.y, mv.to.y)
            || from_mm < safe_z_mm - from_tolerance_mm
        {
            return false;
        }
        if z_mm >= self.above_stock_mm - tolerance_mm {
            return true;
        }
        let (Some(x), Some(y)) = (mv.to.x, mv.to.y) else {
            return false;
        };
        let at = transform.to_project(Point3::new(x, y, 0.0), mv.units).xy();
        let reach_mm = self.reach_mm + 2.0 * tolerance_mm;
        self.cuts
            .iter()
            .filter_map(|&(path, from_mm, to_mm)| floor_near(path, from_mm, to_mm, at, reach_mm))
            .any(|floor_mm| z_mm >= floor_mm + ENTRY_CLEARANCE_MM - tolerance_mm)
    }
}

/// The Z, mm, at which the cut along `path` from Z `from_mm` to `to_mm`
/// passes within `reach_mm` of `at`, if it does: a straight move's Z where
/// it comes nearest, an arc's higher end Z, a move in Z alone its lower.
fn floor_near(path: Segment, from_mm: f64, to_mm: f64, at: Point, reach_mm: f64) -> Option<f64> {
    if point_segment_distance_mm(path, at) > reach_mm {
        return None;
    }
    if path.bulge != 0.0 {
        return Some(from_mm.max(to_mm));
    }
    let ab = path.end - path.start;
    let length_sq = ab.dot(ab);
    if length_sq <= EPS_MM * EPS_MM {
        return Some(from_mm.min(to_mm));
    }
    let t = ((at - path.start).dot(ab) / length_sq).clamp(0.0, 1.0);
    Some(from_mm + (to_mm - from_mm) * t)
}

/// The deepest Z a cut has reached at one XY: machine X and Y as written,
/// Z above stock bottom, mm (S15).
#[derive(Debug, Clone, Copy)]
struct CutTo {
    x: f64,
    y: f64,
    z_mm: f64,
}

impl CutTo {
    /// Whether this is the cut at machine (`x`, `y`). Both come from the
    /// same written numbers, so only rounding noise is tolerated.
    fn is_at(&self, x: f64, y: f64) -> bool {
        approx_eq(self.x, x, EPS_MM) && approx_eq(self.y, y, EPS_MM)
    }
}

/// S15: inside a Drill, a rapid in Z alone that stops below safe Z at
/// (`z_mm`, `tolerance_mm`) above stock bottom stops in air only: at or
/// above the stock top (the retract plane), or no deeper than a cut has
/// already reached at its XY (a peck re-entry). Port of PureCutCNC's
/// `assertPostedRetractSafe` (`drillingRetract.test.ts`).
fn check_drill_rapid(
    mv: &MachineMove,
    (z_mm, tolerance_mm): (f64, f64),
    stock_top_mm: f64,
    cut_to: Option<CutTo>,
) -> Result<(), ExportError> {
    if z_mm >= stock_top_mm - tolerance_mm {
        return Ok(());
    }
    let cut_mm = match (mv.to.x, mv.to.y, cut_to) {
        (Some(x), Some(y), Some(cut)) if cut.is_at(x, y) => Some(cut.z_mm),
        _ => None,
    };
    if cut_mm.is_some_and(|cut| z_mm >= cut - tolerance_mm) {
        Ok(())
    } else {
        Err(ExportError::RapidIntoUncut {
            line: mv.line,
            z_mm,
            cut_mm,
        })
    }
}

/// A `G4 P` value in seconds: the definition's dwell `units` say what `P`
/// is (seconds, or milliseconds on Mach3).
fn dwell_seconds(post: &PostProcessor, p: f64) -> f64 {
    match post.feed_speed.spindle_on_dwell.units {
        DwellUnits::Seconds => p,
        DwellUnits::Milliseconds => p / 1000.0,
    }
}

/// True unless both are known and equal.
pub(crate) fn differs(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !approx_eq(a, b, EPS_MM),
        (None, None) => false,
        _ => true,
    }
}

/// S3: a cut has a positive feed, and it is one of its tool's rates. A
/// move that falls while it moves in XY (an entry ramp or helix, a smooth
/// tab's down slope, or an arc that falls) may also run anywhere between
/// the tool's plunge rate and its feed: its feed is the cutting feed
/// scaled so its vertical rate stays under the plunge rate (S10's feed
/// scale, `dc_cam::entry`), or a tool rate the emitter rounded down for
/// the same reason, which lands between the two. Whether that vertical
/// rate is right is S10's check ([`check_descent_rate`]). A level cut of a
/// Profile with a last pass may also run anywhere from
/// [`last_pass_floor_mm_min`] up to the feed: the last pass's inside arcs,
/// or their chords, slowed so the cutting edge moves at the feed.
fn check_feed(
    mv: &MachineMove,
    op: &Operation,
    tool: &Tool,
    tolerance_mm_min: f64,
) -> Result<(), ExportError> {
    let line = mv.line;
    let Some(feed) = mv.feed.filter(|f| *f > 0.0) else {
        return Err(ExportError::CutWithoutFeed { line });
    };
    let feed_mm_min = feed * mm_per_unit(mv.units);
    let on_a_rate = [tool.feed_mm_min, tool.plunge_mm_min]
        .into_iter()
        .flatten()
        .any(|rate| approx_eq(rate, feed_mm_min, tolerance_mm_min));
    let scaled_descent = is_descent(mv)
        && tool
            .plunge_mm_min
            .zip(tool.feed_mm_min)
            .is_some_and(|(plunge, cut)| {
                feed_mm_min >= plunge - tolerance_mm_min && feed_mm_min <= cut + tolerance_mm_min
            });
    // A Profile's last pass runs its inside arcs at the feed scaled
    // by `ρ / (ρ + r)`, so its cutting edge moves at the feed, and
    // never slower than the edge needs in the smallest hole the last
    // pass cuts, `a / (a + r)`; flattened, their chords too.
    let edge_scaled = is_level_cut(mv)
        && last_pass_floor_mm_min(op, tool)
            .zip(tool.feed_mm_min)
            .is_some_and(|(floor, cut)| {
                feed_mm_min >= floor - tolerance_mm_min && feed_mm_min <= cut + tolerance_mm_min
            });
    let matches = on_a_rate || scaled_descent || edge_scaled;
    if !matches {
        return Err(ExportError::FeedNotFromTool { line, feed_mm_min });
    }
    Ok(())
}

/// True for a `G1`, `G2` or `G3` that moves in XY at one Z, every
/// coordinate known.
fn is_level_cut(mv: &MachineMove) -> bool {
    let (from, to) = (mv.from, mv.to);
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (from.x, from.y, from.z, to.x, to.y, to.z)
    else {
        return false;
    };
    mv.kind.is_cut() && (tz - fz).abs() <= EPS_MM && (tx - fx).hypot(ty - fy) > 0.0
}

/// The least feed, mm/min, a level cut of a Profile with a last pass may
/// carry: its tool's feed times `a / (a + r)`, the stock to leave over the
/// stock to leave plus the tool radius, as slow as the generator runs the
/// last pass's tightest inside arc. `None` for any other operation.
fn last_pass_floor_mm_min(op: &Operation, tool: &Tool) -> Option<f64> {
    let Operation::Profile(op) = op else {
        return None;
    };
    let stock_to_leave_mm = op.last_pass.as_ref()?.stock_to_leave_mm;
    let feed_mm_min = tool.feed_mm_min?;
    Some(feed_mm_min * stock_to_leave_mm / (stock_to_leave_mm + tool.diameter_mm / 2.0))
}

/// True for a `G1`, `G2` or `G3` that falls while it moves in XY, every
/// coordinate known.
fn is_descent(mv: &MachineMove) -> bool {
    let (from, to) = (mv.from, mv.to);
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (from.x, from.y, from.z, to.x, to.y, to.z)
    else {
        return false;
    };
    mv.kind.is_cut() && tz < fz && (tx - fx).hypot(ty - fy) > 0.0
}

/// S10: a cutting move that descends falls no faster than its tool's
/// plunge rate: feed × sin(slope) ≤ plunge, with the slope recomputed from
/// the written numbers (`docs/specs/phase-2/functional.md`, S10). Every
/// descending cut is held to it, whatever made it: an entry ramp or helix,
/// a smooth tab's down slope, a straight plunge (slope 90°, so its feed
/// must be at most the plunge rate) or an arc that descends. Nothing above
/// the plunge rate is allowed for rounding: the emitter writes every feed
/// the toolpath holds to the plunge rate so that the rate
/// [`descent_rate_mm_min`] reads from the written numbers stays at or
/// under it. A move whose start is not known is skipped; S2 and S4 keep a
/// program from cutting from an unknown position.
fn check_descent_rate(
    mv: &MachineMove,
    op: &Operation,
    tool: &Tool,
    transform: &MachineTransform,
) -> Result<(), ExportError> {
    let Some(rate_mm_min) = descent_rate_mm_min(mv, transform) else {
        return Ok(());
    };
    if rate_mm_min > tool.plunge_mm_min.unwrap_or(0.0) {
        return Err(ExportError::EntryTooSteep {
            op: op.id().clone(),
            line: mv.line,
        });
    }
    Ok(())
}

/// The vertical rate, mm/min, at which the controller runs cutting move
/// `mv`, from its written numbers alone: its feed times the sine of its
/// slope, `feed × (drop / length)`, with an arc's length along the arc
/// its centre words describe. A straight plunge's rate is its feed
/// exactly, and infinite for a cut with no feed (S3 refuses that cut;
/// S10 must not pass it). `None` for a rapid, a move that does not fall,
/// or one whose start is not known. S10 judges every program by it, and
/// the emitter writes feeds by it.
pub(crate) fn descent_rate_mm_min(mv: &MachineMove, transform: &MachineTransform) -> Option<f64> {
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
    else {
        return None;
    };
    let drop_mm = transform.project_z(fz, mv.units) - transform.project_z(tz, mv.units);
    if drop_mm <= 0.0 {
        return None;
    }
    let scale = mm_per_unit(mv.units);
    let run_mm = match mv.kind {
        MotionKind::Rapid => return None,
        MotionKind::Linear => (tx - fx).hypot(ty - fy) * scale,
        MotionKind::ArcCw | MotionKind::ArcCcw => {
            let machine_cw = mv.kind == MotionKind::ArcCw;
            let centre = mv.arc_centre()?;
            let arc = Arc::from_ends(Point::new(fx, fy), Point::new(tx, ty), centre, !machine_cw);
            arc.radius_mm * arc.sweep_rad.abs() * scale
        }
    };
    // A falling cut with no feed has no rate S10 could pass, so it falls
    // faster than any plunge rate. S3 refuses it first.
    let feed_mm_min = mv.feed.unwrap_or(f64::INFINITY) * scale;
    // `drop / length` first: exactly 1 on a straight plunge.
    Some(feed_mm_min * (drop_mm / run_mm.hypot(drop_mm)))
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
/// chord. This judges the words as written, so it reads them itself and
/// not through [`MachineMove::arc_centre`], which places a centre for any
/// words.
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
///   it never travels sideways below either end); in a Drill written as
///   canned cycles, also by none, or by a rapid in XY alone over it, when
///   a rapid in Z alone passes through its end point and the toolpath goes
///   on the same way along that line ([`match_rapid_on_the_way`]);
/// - a straight cut by one `G1` that ends at its end point;
/// - a dwell by a `G4` with the same seconds (within a millisecond, in
///   the definition's dwell units) between the motion lines around it;
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
///
/// S12: with arc fitting on (`tolerances.arc_fit_mm`), a `G2`/`G3` may
/// stand for a run of straight moves (and the shallow corner arcs among
/// them) when it starts and ends at toolpath vertices, covers at least
/// [`crate::arcfit::MIN_RUN_CHORDS`] moves, every vertex between (and
/// every shallow arc's midpoint) lies within the arc-fit tolerance plus
/// one unit in the last place of it, it sweeps at most 90°, and it turns
/// the way the vertices do; see [`match_fitted_arc`]. Any other arc in
/// place of straight moves is [`ExportError::ArcFitViolation`].
///
/// The program reproduces the toolpath when some matching of its lines to
/// the moves by these rules takes every move and every line. Within one
/// unit in the last place a line may end at more than one move:
/// sub-quantum steps that round to one point, or a fitted arc's end near
/// several vertices. The first matching tried takes, at each move, the
/// first way the rules allow; where that fails, the others are tried in
/// turn ([`Matching::search`]). The error is that of the matching that
/// went furthest.
fn check_reproduces(
    span: &OpSpan<'_>,
    blocks: &[Block],
    post: &PostProcessor,
    transform: &MachineTransform,
    tolerances: Tolerances,
) -> Result<(), ExportError> {
    let in_span = |b: &Block| span.lines.contains(&b.line);
    let motions: Vec<ReadMove<'_>> = blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| in_span(b))
        .filter_map(|(index, b)| Some((index, b.motion.as_ref()?)))
        .map(|(index, mv)| ReadMove::new(mv, index, post, transform))
        .collect();
    // The span's dwells, a canned cycle's included: block index and
    // seconds. Blocks, not lines: a cycle's moves and dwell share its line.
    let dwells: Vec<(usize, f64)> = blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| in_span(b) && b.motion.is_none() && b.has('G', 4.0))
        .filter_map(|(index, b)| Some((index, dwell_seconds(post, b.value('P')?))))
        .collect();
    // S11: where a Drill is written as canned cycles, how far the
    // controller backs off for a re-entry or a chip-break retract is its
    // own (the definition's `reentry_gap_mm`), not the toolpath's, so a
    // rapid in Z alone inside a hole matches within that gap. Every other
    // move matches as for plain moves.
    let canned =
        matches!(span.op, Operation::Drill(_)) && blocks.iter().any(|b| b.synthetic && in_span(b));
    let reentry_gap_mm = canned
        .then(|| CycleModel::of(post).reentry_gap_mm)
        .flatten();
    Matching {
        op: span.op,
        moves: &span.toolpath.moves,
        motions: &motions,
        dwells: &dwells,
        canned,
        reentry_gap_mm,
        transform,
        tolerances,
    }
    .search()
}

/// S6's matching of one span's toolpath moves to its motion lines
/// ([`check_reproduces`]).
struct Matching<'m, 'a> {
    op: &'m Operation,
    moves: &'m [Move],
    motions: &'m [ReadMove<'a>],
    /// The span's dwells: block index and seconds.
    dwells: &'m [(usize, f64)],
    /// A Drill written as canned cycles.
    canned: bool,
    /// S11: how far a canned cycle backs off for a re-entry.
    reentry_gap_mm: Option<f64>,
    transform: &'m MachineTransform,
    tolerances: Tolerances,
}

/// How far a [`Matching`] has gone.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// The next toolpath move to match.
    index: usize,
    /// The next motion line, in the span's motions.
    cursor: usize,
    /// The first of the span's dwells a toolpath dwell may still take.
    next_dwell: usize,
    /// Where the last straight cut matched ended, in XY.
    last_cut: Option<Point>,
}

impl Place {
    /// The place as a set key: equal for places the matching goes on from
    /// alike.
    fn key(&self) -> (usize, usize, usize, Option<(u64, u64)>) {
        (
            self.index,
            self.cursor,
            self.next_dwell,
            self.last_cut.map(|p| (p.x.to_bits(), p.y.to_bits())),
        )
    }
}

/// One way to match toolpath moves from a [`Place`]: `lines` motion lines
/// for `covered` moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Step {
    lines: usize,
    covered: usize,
    /// For a toolpath dwell, the index after the dwell it takes.
    next_dwell: Option<usize>,
}

impl Step {
    /// `lines` lines for one move.
    fn one(lines: usize) -> Self {
        Self {
            lines,
            covered: 1,
            next_dwell: None,
        }
    }

    /// `lines` lines for a fitted run of `covered` moves.
    fn run((lines, covered): (usize, usize)) -> Self {
        Self {
            lines,
            covered,
            next_dwell: None,
        }
    }
}

/// A place a [`Matching`] went on from and the toolpath move it matched
/// there, with the step it took and, once it has come back, the steps it
/// has still to try, last first.
struct Choice<'m> {
    at: Place,
    mv: &'m Move,
    taken: Step,
    others: Option<Vec<Step>>,
}

/// What a [`Matching`] sees at a place.
struct View<'r, 'a> {
    /// The motion lines already matched.
    taken: &'r [ReadMove<'a>],
    /// The motion lines not yet matched.
    rest: &'r [ReadMove<'a>],
    /// One unit in the last written place, mm.
    tolerance_mm: f64,
    /// The controller is at the move's end already.
    already_there: bool,
    /// The end of the toolpath move before.
    previous: Option<Point3>,
}

impl<'m, 'a> Matching<'m, 'a> {
    fn mismatch(&self, index: usize) -> ExportError {
        ExportError::MotionMismatch {
            op: self.op.id().clone(),
            index,
        }
    }

    /// The motion lines a matching at `at` has taken, and those it has
    /// not.
    ///
    /// # Errors
    ///
    /// [`ExportError::SafetyCheckDefect`] for a place past the last
    /// line: such a matching has claimed lines the program does not
    /// have, and reading that as "no line is left" would pass it.
    fn lines(&self, at: &Place) -> Result<(&'m [ReadMove<'a>], &'m [ReadMove<'a>]), ExportError> {
        self.motions
            .split_at_checked(at.cursor)
            .ok_or(defect("S6: a matching is past the last motion line"))
    }

    /// What the matching sees at `at`, about to match `mv`.
    ///
    /// # Errors
    ///
    /// [`ExportError::SafetyCheckDefect`] for a place past the last line
    /// or the last move ([`Self::lines`]).
    fn view(&self, at: &Place, mv: &Move) -> Result<View<'m, 'a>, ExportError> {
        let (taken, rest) = self.lines(at)?;
        let (matched, _) = self
            .moves
            .split_at_checked(at.index)
            .ok_or(defect("S6: a matching is past the last toolpath move"))?;
        // Where the controller is before `rest`.
        let here = rest
            .first()
            .map_or_else(|| taken.last().and_then(|m| m.to), |m| m.from);
        let tolerance_mm = rest
            .first()
            .or(taken.last())
            .map_or(EPS_MM, |m| m.tolerance_mm);
        Ok(View {
            taken,
            rest,
            tolerance_mm,
            already_there: here.is_some_and(|h| near(h, mv.to, tolerance_mm)),
            previous: matched.last().map(|m| m.to),
        })
    }

    /// `Ok` if some matching of the lines to the moves takes them all;
    /// else the error of the matching that went furthest along the
    /// toolpath (of those, the first tried).
    ///
    /// A depth-first search over [`Place`]s: from each it takes the first
    /// step [`Self::first_step`] allows; where none is allowed, it goes
    /// back to the last place it went on from and tries the next of
    /// [`Self::other_steps`] there. Every step takes a move, so no place
    /// comes twice on one path, and a place met again on another path has
    /// failed already: each place is tried once. A program the first
    /// matching takes is checked as it was before #483.
    ///
    /// # Errors
    ///
    /// The furthest matching's, as above; or
    /// [`ExportError::SafetyCheckDefect`] at once, whatever is left to
    /// try, when a step leads past the last move or line.
    fn search(&self) -> Result<(), ExportError> {
        let mut at = Place {
            index: 0,
            cursor: self
                .motions
                .iter()
                .position(|m| !is_retract(m.mv))
                .unwrap_or(self.motions.len()),
            next_dwell: 0,
            last_cut: None,
        };
        let mut choices: Vec<Choice<'m>> = Vec::new();
        let mut tried = HashSet::new();
        // The furthest failure: the move, and why.
        let mut furthest: Option<(usize, ExportError)> = None;
        loop {
            let failed = if !tried.insert(at.key()) {
                // Failed from here already, on another path.
                None
            } else if let Some(mv) = self.moves.get(at.index) {
                match self.first_step(&at, mv, self.view(&at, mv)?) {
                    Ok(step) => {
                        choices.push(Choice {
                            at,
                            mv,
                            taken: step,
                            others: None,
                        });
                        at = self.advance(&at, mv, step)?;
                        continue;
                    }
                    Err(error) => Some(error),
                }
            } else if self.finished(&at)? {
                return Ok(());
            } else {
                Some(self.mismatch(self.moves.len()))
            };
            if let Some(error) = failed
                && furthest.as_ref().is_none_or(|(index, _)| at.index > *index)
            {
                furthest = Some((at.index, error));
            }
            // Back to the last place with a step still to try.
            at = loop {
                let Some(choice) = choices.last_mut() else {
                    return Err(furthest.map_or_else(|| self.mismatch(at.index), |(_, e)| e));
                };
                let others = match &mut choice.others {
                    Some(others) => others,
                    untried => {
                        let view = self.view(&choice.at, choice.mv)?;
                        let mut others =
                            self.other_steps(&choice.at, choice.mv, choice.taken, view);
                        others.reverse();
                        untried.insert(others)
                    }
                };
                if let Some(step) = others.pop() {
                    break self.advance(&choice.at, choice.mv, step)?;
                }
                choices.pop();
            };
        }
    }

    /// Whether every line after `at`, which has taken every move, is a
    /// retract.
    ///
    /// # Errors
    ///
    /// [`ExportError::SafetyCheckDefect`] for a place past the last line
    /// ([`Self::lines`]): it is not "finished".
    fn finished(&self, at: &Place) -> Result<bool, ExportError> {
        let (_, rest) = self.lines(at)?;
        Ok(rest.iter().all(|m| is_retract(m.mv)))
    }

    /// Where `step`, which matched `mv` at `at`, leads.
    ///
    /// # Errors
    ///
    /// [`ExportError::SafetyCheckDefect`] for a step that takes more
    /// moves than the toolpath has left, or more lines than the span has
    /// left: past the last move a place reads as "every move matched",
    /// and past the last line as "no line left".
    fn advance(&self, at: &Place, mv: &Move, step: Step) -> Result<Place, ExportError> {
        let index = at.index + step.covered;
        let cursor = at.cursor + step.lines;
        if index > self.moves.len() || cursor > self.motions.len() {
            return Err(defect(
                "S6: a step takes more moves or motion lines than are left",
            ));
        }
        let straight = step.covered == 1 && mv.kind == MoveKind::Linear;
        Ok(Place {
            index,
            cursor,
            next_dwell: step.next_dwell.unwrap_or(at.next_dwell),
            last_cut: if straight {
                Some(mv.to.xy())
            } else {
                at.last_cut
            },
        })
    }

    /// The first way the rules allow to match `mv`, the move at `at`
    /// (`view` what the matching sees there), and what follows it; else
    /// why there is none.
    fn first_step(&self, at: &Place, mv: &Move, view: View<'_, 'a>) -> Result<Step, ExportError> {
        let View {
            taken,
            rest,
            tolerance_mm,
            already_there,
            previous,
        } = view;
        let index = at.index;
        // Inside a hole: in Z alone, where the last cut was.
        let in_hole = || {
            let on = |p: Point| p.distance_to(mv.to.xy()) <= tolerance_mm;
            previous.is_some_and(|p| on(p.xy())) && at.last_cut.is_some_and(on)
        };
        let consumed = match mv.kind {
            MoveKind::Rapid => match_rapid(rest, mv.to, already_there)
                .or_else(|| {
                    let gap_mm = self.reentry_gap_mm.filter(|_| in_hole())?;
                    match_in_hole_rapid(rest, mv.to, gap_mm)
                })
                .or_else(|| {
                    // A cycle from the clearance height goes through safe
                    // Z without stopping.
                    self.canned
                        .then(|| match_rapid_on_the_way(rest, mv.to, self.moves.get(index + 1)))
                        .flatten()
                })
                .map(Step::one),
            MoveKind::Dwell { seconds } => {
                // A `G4` with these seconds after the last motion matched
                // and before the next; it moves nothing.
                let after = taken.last().map(|m| m.block);
                let before = rest.first().map_or(usize::MAX, |m| m.block);
                self.dwells
                    .iter()
                    .enumerate()
                    .skip(at.next_dwell)
                    .find(|(_, (block, s))| {
                        after.is_none_or(|a| *block > a)
                            && *block < before
                            && (s - seconds).abs() <= DWELL_TOLERANCE_S
                    })
                    .map(|(i, _)| Step {
                        next_dwell: Some(i + 1),
                        ..Step::one(0)
                    })
            }
            MoveKind::Linear => match_linear(rest, mv.to, already_there).map(Step::one),
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let clockwise = matches!(mv.kind, MoveKind::ArcCw { .. });
                match previous.map(|start| ToolpathArc::new(start, mv.to, centre, clockwise)) {
                    Some(Some(arc)) => {
                        match_arc(rest, &arc, self.transform, self.tolerances.flatten_mm)
                    }
                    // Too small to be an arc: `emit` writes a straight cut.
                    Some(None) => match_linear(rest, mv.to, already_there),
                    // `emit` cannot write an arc without a start.
                    None => None,
                }
                .map(Step::one)
            }
        };
        if let Some(step) = consumed {
            return Ok(step);
        }
        // S12: an arc where the toolpath has straight moves (or a shallow
        // corner arc a run may take as its chord).
        let fitted = (|| {
            let fitted = rest
                .first()
                .filter(|read| matches!(read.mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw));
            let (Some(read), Some(arc_fit_mm)) = (fitted, self.tolerances.arc_fit_mm) else {
                return Err(self.mismatch(index));
            };
            if !run_eligible(previous, mv, arc_fit_mm) {
                return Err(self.mismatch(index));
            }
            let ends = match_fitted_arc(
                rest,
                self.moves,
                index,
                previous,
                self.transform,
                arc_fit_mm,
                FittedEnds::First,
            );
            ends.first()
                .map(|&end| Step::run(end))
                .ok_or_else(|| ExportError::ArcFitViolation {
                    op: self.op.id().clone(),
                    line: read.mv.line,
                })
        })();
        match fitted {
            Ok(step) => Ok(step),
            // An arc of at most a half turn that ends where the controller
            // already is lies wholly within the rounding of it: `emit`
            // writes it as nothing, as it does such a straight move. Tried
            // last, so a fitted run that takes the arc is matched first.
            Err(_) if already_there && written_as_nothing(previous, mv) => Ok(Step::one(0)),
            Err(error) => Err(error),
        }
    }

    /// The ways other than `taken` the rules allow to match `mv`, the move
    /// at `at`, and what follows it, in the order to try them: by no
    /// lines, where the controller is already at its end and `emit` writes
    /// nothing for such a move; then a fitted arc here, to each vertex it
    /// may end at, nearest first. `view` is what the matching sees at
    /// `at`.
    fn other_steps(&self, at: &Place, mv: &Move, taken: Step, view: View<'_, 'a>) -> Vec<Step> {
        let View {
            rest,
            already_there,
            previous,
            ..
        } = view;
        let mut steps = Vec::new();
        let nothing = already_there
            && match mv.kind {
                MoveKind::Rapid | MoveKind::Linear => true,
                MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                    let clockwise = matches!(mv.kind, MoveKind::ArcCw { .. });
                    // Too small to be an arc, so a straight cut, or within
                    // the rounding of where the controller is.
                    previous.is_some_and(|start| {
                        ToolpathArc::new(start, mv.to, centre, clockwise).is_none()
                    }) || written_as_nothing(previous, mv)
                }
                MoveKind::Dwell { .. } => false,
            };
        if nothing {
            steps.push(Step::one(0));
        }
        let arc_next = rest
            .first()
            .is_some_and(|read| matches!(read.mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw));
        if let Some(arc_fit_mm) = self.tolerances.arc_fit_mm
            && arc_next
            && run_eligible(previous, mv, arc_fit_mm)
        {
            let ends = match_fitted_arc(
                rest,
                self.moves,
                at.index,
                previous,
                self.transform,
                arc_fit_mm,
                FittedEnds::All,
            );
            steps.extend(ends.into_iter().map(Step::run));
        }
        steps.retain(|step| *step != taken);
        steps
    }
}

/// Whether toolpath move `mv`, from `previous`, is an arc of at most a
/// half turn: every point of it lies no further from its start than its
/// end does, so it is wholly within the rounding when its end is.
fn written_as_nothing(previous: Option<Point3>, mv: &Move) -> bool {
    let (MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre }) = mv.kind else {
        return false;
    };
    let clockwise = matches!(mv.kind, MoveKind::ArcCw { .. });
    previous
        .and_then(|start| ToolpathArc::new(start, mv.to, centre, clockwise))
        .is_some_and(|arc| arc.sweep_rad <= PI)
}

/// A parsed move with its ends in project space, mm.
struct ReadMove<'a> {
    mv: &'a MachineMove,
    /// Its block's index in the parsed program.
    block: usize,
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
    fn new(
        mv: &'a MachineMove,
        block: usize,
        post: &PostProcessor,
        transform: &MachineTransform,
    ) -> Self {
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
            block,
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

/// S11: how many of `rest` a rapid in Z alone inside a hole to `target`
/// takes, if they match it within the controller's re-entry gap `gap_mm`:
/// one rapid in Z alone at the target's XY whose Z is within the gap. S1
/// and S15 still check where that rapid ends.
fn match_in_hole_rapid(rest: &[ReadMove<'_>], target: Point3, gap_mm: f64) -> Option<usize> {
    let first = rest.first().filter(|m| m.mv.kind == MotionKind::Rapid)?;
    let tolerance = first.tolerance_mm;
    let (from, to) = (&first.mv.from, &first.mv.to);
    let z_only = !differs(from.x, to.x) && !differs(from.y, to.y);
    let at_xy = first.to_xy.is_some_and(|p| {
        approx_eq(p.x, target.x, tolerance) && approx_eq(p.y, target.y, tolerance)
    });
    let within = first
        .to_z
        .is_some_and(|z| approx_eq(z, target.z, gap_mm + tolerance));
    (z_only && at_xy && within).then_some(1)
}

/// S11: how many of `rest` a rapid to `target` takes when the controller
/// goes through `target` without stopping there: one rapid of the
/// controller's in Z alone, at the target's XY, passes through it on the
/// way to a Z beyond it, and the toolpath goes on the same way along that
/// line, by a rapid in Z alone (`next`), or the target is the toolpath's
/// last move and the controller's rapid goes up, the retract
/// [`check_reproduces`] accepts after the last move. Coming down, a rapid
/// in XY alone to over the target may come first: the emitter's split of
/// the move over the first hole, made at the clearance height. The lines
/// taken are that one, or none; the rapid in Z stays for the next move.
/// Only for a Drill written as canned cycles, whose `G98` returns each
/// hole to the clearance height where the toolpath stops at safe Z.
fn match_rapid_on_the_way(
    rest: &[ReadMove<'_>],
    target: Point3,
    next: Option<&Move>,
) -> Option<usize> {
    let rapid = |m: &&ReadMove<'_>| m.mv.kind == MotionKind::Rapid;
    let first = rest.first().filter(rapid)?;
    let tolerance = first.tolerance_mm;
    let over =
        |p: Point| approx_eq(p.x, target.x, tolerance) && approx_eq(p.y, target.y, tolerance);
    let xy_only = !differs(first.mv.from.z, first.mv.to.z) && first.to_xy.is_some_and(over);
    let (taken, along) = if xy_only {
        (1, rest.get(1).filter(rapid)?)
    } else {
        (0, first)
    };
    let (from, to) = (along.from?, along.to?);
    if !(over(from.xy()) && over(to.xy())) {
        return None;
    }
    let up = to.z > from.z;
    let through = if up {
        from.z + tolerance < target.z && target.z < to.z - tolerance
    } else {
        to.z + tolerance < target.z && target.z < from.z - tolerance
    };
    let goes_on = match next {
        Some(n) => {
            n.kind == MoveKind::Rapid
                && over(n.to.xy())
                && if up {
                    n.to.z > target.z + tolerance
                } else {
                    n.to.z < target.z - tolerance
                }
        }
        None => up && taken == 0,
    };
    (through && goes_on && !(xy_only && up)).then_some(taken)
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

/// Whether toolpath move `mv`, from `previous`, is one a fitted run may
/// take: a straight cut, or an arc whose sagitta is within `arc_fit_mm`.
/// With `previous` unknown nothing is.
fn run_eligible(previous: Option<Point3>, mv: &Move, arc_fit_mm: f64) -> bool {
    match mv.kind {
        MoveKind::Rapid | MoveKind::Dwell { .. } => false,
        MoveKind::Linear => true,
        MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
            let cw = matches!(mv.kind, MoveKind::ArcCw { .. });
            previous.is_some_and(|p| {
                shallow_arc_midpoint(p.xy(), mv.to.xy(), centre, cw, arc_fit_mm).is_some()
            })
        }
    }
}

/// Which ends of a fitted run [`match_fitted_arc`] finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FittedEnds {
    /// The nearest only.
    First,
    /// Every one, nearest first.
    All,
}

/// S12: how many lines of `rest` and how many toolpath moves from `index`
/// a fitted run accounts for, if the lines may stand for the moves: for
/// each vertex the run may end at, nearest first (`ends`), or none.
///
/// The lines are a chain of consecutive arc moves about one centre, one way,
/// each sweeping at most 90°, the first starting at `previous`, the end of the
/// toolpath move before, and the last ending at the end of a toolpath move: a
/// fitted run is split into sub-arcs whose ends between are on the circle, and
/// only the run's ends are vertices. The chain covers at least
/// [`MIN_RUN_CHORDS`] moves, all straight cuts or shallow corner arcs (sagitta
/// within `arc_fit_mm`) at the run's Z. Every vertex between, and every shallow
/// arc's midpoint, lies on the chain within `arc_fit_mm` plus the lines'
/// rounding, and the vertices turn the chain's way about its centre. The chain
/// also lies within that of every move it covers along the move's whole length:
/// a straight move between two points on the circle is bowed off by the arc's
/// sagitta over it, which the vertices alone never show. The chain may stop
/// short of its last line where the toolpath goes on with a move a run cannot
/// take, if the line before ends at the vertex there.
///
/// More than one vertex may end the chain: every vertex within a unit in
/// the last place of its end, the walk going on past each (the fitter's
/// run ended at a vertex after one that rounds to its end too).
fn match_fitted_arc(
    rest: &[ReadMove<'_>],
    moves: &[Move],
    index: usize,
    previous: Option<Point3>,
    transform: &MachineTransform,
    arc_fit_mm: f64,
    ends: FittedEnds,
) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    walk_fitted_arc(
        rest, moves, index, previous, transform, arc_fit_mm, ends, &mut found,
    );
    found
}

/// [`match_fitted_arc`]'s walk: adds each end it finds to `found`, and
/// stops at the first when `ends` asks for it, or where the chain cannot
/// go on (`None`).
#[allow(
    clippy::too_many_arguments,
    reason = "match_fitted_arc's arguments and its result"
)]
fn walk_fitted_arc(
    rest: &[ReadMove<'_>],
    moves: &[Move],
    index: usize,
    previous: Option<Point3>,
    transform: &MachineTransform,
    arc_fit_mm: f64,
    ends: FittedEnds,
    found: &mut Vec<(usize, usize)>,
) -> Option<()> {
    let start = previous?;
    let first = rest.first()?;
    let tolerance_mm = first.tolerance_mm;
    let allowed_mm = arc_fit_mm + tolerance_mm;

    // The chain of sub-arcs.
    let mut pieces: Vec<ToolpathArc> = Vec::new();
    let mut expect_from = start;
    for read in rest {
        if !matches!(read.mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw) {
            break;
        }
        let (Some(from), Some(to)) = (read.from, read.to) else {
            break;
        };
        let Some((centre, clockwise)) = read_arc_centre(read, transform) else {
            break;
        };
        // Each sub-arc's I and J are snapped within a few quanta to agree
        // with its own rounded ends, so the pieces' centres differ by
        // more than one unit in the last place, within the allowance.
        // Compared with the piece before, not the first: over a long run
        // the snaps add up past the allowance (a 31.7 mm pocket ring
        // drifted 0.012 mm in five pieces). Every vertex is still checked
        // against its own piece below.
        let same_circle = pieces.last().is_none_or(|p: &ToolpathArc| {
            p.clockwise == clockwise
                && approx_eq(p.centre.x, centre.x, allowed_mm)
                && approx_eq(p.centre.y, centre.y, allowed_mm)
        });
        if !same_circle || !near(from, expect_from, tolerance_mm) {
            break;
        }
        let arc = ToolpathArc::new(from, to, centre, clockwise)?;
        if arc.sweep_rad > FRAC_PI_2 + arc.angle_tolerance_rad(allowed_mm) {
            return None;
        }
        pieces.push(arc);
        expect_from = to;
    }
    let chain = pieces.first()?;
    let (centre, clockwise) = (chain.centre, chain.clockwise);

    // Walk the vertices along the chain.
    let mut piece = 0;
    let mut vertex = start;
    let mut cross = 0.0;
    let mut covered = 0;
    // Adds the end at `vertex`, if the chain may end there; `None` once
    // the walk is to stop.
    let mut end_here = |piece: usize, covered: usize, vertex: Point3, cross: f64| -> Option<()> {
        let current = pieces.get(piece)?;
        let turns_with = if clockwise { cross < 0.0 } else { cross > 0.0 };
        let end = (piece + 1, covered);
        if near(vertex, current.end, tolerance_mm)
            && turns_with
            && covered >= MIN_RUN_CHORDS
            && found.last() != Some(&end)
        {
            found.push(end);
            if ends == FittedEnds::First {
                return None;
            }
        }
        Some(())
    };
    for mv in moves.get(index..)? {
        if !run_eligible(Some(vertex), mv, arc_fit_mm) || !approx_eq(mv.to.z, start.z, tolerance_mm)
        {
            // The toolpath goes on with a move no run takes: the chain
            // must end at the vertex before it.
            end_here(piece, covered, vertex, cross);
            return None;
        }
        let midpoint = match mv.kind {
            MoveKind::ArcCw { centre: c } | MoveKind::ArcCcw { centre: c } => {
                let cw = matches!(mv.kind, MoveKind::ArcCw { .. });
                shallow_arc_midpoint(vertex.xy(), mv.to.xy(), c, cw, arc_fit_mm)
            }
            MoveKind::Rapid | MoveKind::Linear | MoveKind::Dwell { .. } => None,
        };
        let first_piece = piece;
        for p in midpoint.into_iter().chain([mv.to.xy()]) {
            // On the current piece, or past its end and on the next.
            loop {
                let current = pieces.get(piece)?;
                if let Some(turn_rad) = current.turn_to(p, allowed_mm) {
                    // Past the piece's end, away from the joint, and on
                    // the next piece, `p` is on the next. On a circle
                    // whose radius is near the allowance the angle slack
                    // is wider than a piece, so a vertex well into the
                    // next piece is still within this one's slack, and the
                    // walk would never reach the last piece.
                    let at_joint = approx_eq(p.x, current.end.x, tolerance_mm)
                        && approx_eq(p.y, current.end.y, tolerance_mm);
                    let on_next = pieces
                        .get(piece + 1)
                        .is_some_and(|next| next.turn_to(p, allowed_mm).is_some());
                    if turn_rad > current.sweep_rad && !at_joint && on_next {
                        piece += 1;
                        continue;
                    }
                    break;
                }
                let on_circle =
                    (p.distance_to(current.centre) - current.radius_mm).abs() <= allowed_mm;
                if !on_circle || piece + 1 >= pieces.len() {
                    return None;
                }
                piece += 1;
            }
        }
        // The chain stays within the allowance of the move along its whole
        // length, not only at its ends: over the part of every piece
        // between the move's ends, the farthest point from the move's
        // chord (for a straight move between points on the circle, its
        // sagitta). Each end is placed on each piece, not assumed to lie
        // before or past it: a vertex up to the angle slack past a piece's
        // end counts as on that piece, and on a circle so small that the
        // slack is a whole chord it lies inside the next piece, whose part
        // before it the move does not span.
        for k in first_piece..=piece {
            let current = pieces.get(k)?;
            let from_rad = current.clamped_turn(vertex.xy());
            let to_rad = current.clamped_turn(mv.to.xy());
            if to_rad > from_rad
                && current.deviation_mm(from_rad, to_rad, vertex.xy(), mv.to.xy()) > allowed_mm
            {
                return None;
            }
        }
        cross += (vertex.xy() - centre).cross(mv.to.xy() - centre);
        covered += 1;
        vertex = mv.to;
        // On the last piece; or, on a circle so small that its last pieces
        // are within a unit in the last place (a V-Carve's tip fitted on a
        // circle 0.003 mm across), where every piece after this one starts
        // and ends within a unit of the vertex. #453 keeps a vertex at a
        // joint on the piece before it, so such a vertex never reached the
        // last piece. Those pieces lie within the allowance of the vertex:
        // each sweeps at most 90°, so no point of one is further from its
        // start than its chord, a unit or two.
        let last = pieces.len().saturating_sub(1);
        let at_last = piece == last
            || pieces.get(piece + 1..).is_some_and(|after| {
                after.iter().all(|p| {
                    near(p.start, vertex, tolerance_mm) && near(p.end, vertex, tolerance_mm)
                })
            });
        if at_last && near(vertex, chain_end(&pieces)?, tolerance_mm) {
            end_here(last, covered, vertex, cross)?;
        }
    }
    end_here(piece, covered, vertex, cross)
}

/// Where the last piece of `pieces` ends.
fn chain_end(pieces: &[ToolpathArc]) -> Option<Point3> {
    pieces.last().map(|p| p.end)
}

/// The centre of arc move `read` in project space, and whether it turns
/// clockwise there: from I and J, or from R as the controller derives it,
/// with a mirroring axis map swapping the direction.
fn read_arc_centre(read: &ReadMove<'_>, transform: &MachineTransform) -> Option<(Point, bool)> {
    let mv = read.mv;
    let machine_cw = mv.kind == MotionKind::ArcCw;
    let machine_centre = mv.arc_centre()?;
    let centre = transform
        .to_project(
            Point3::new(machine_centre.x, machine_centre.y, 0.0),
            mv.units,
        )
        .xy();
    Some((centre, machine_cw != transform.mirrors_xy()))
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
    let Some(Point { x: cx, y: cy }) = mv.arc_centre() else {
        return false;
    };
    // I and J say where the centre is, so it is compared; R says only how
    // far away, so the arc's midpoint is.
    if mv.arc.r.is_none() {
        let centre = to_project(cx, cy);
        return approx_eq(centre.x, arc.centre.x, tolerance)
            && approx_eq(centre.y, arc.centre.y, tolerance);
    }
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

/// An arc whose sweep is below this is written as a straight move
/// (`emit`, `ArcGeometry::new`).
const MIN_ARC_SWEEP_RAD: f64 = 1e-9;

/// An arc whose radius is below this is written as a straight move
/// (`emit`, `ArcGeometry::new`).
const MIN_ARC_RADIUS_MM: f64 = 1e-9;

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
        (radius_mm > MIN_ARC_RADIUS_MM && sweep_rad > MIN_ARC_SWEEP_RAD).then_some(Self {
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

    /// How far along the arc the point of it nearest `p` in angle is, as
    /// the angle turned from the start: the arc's start or end for `p`
    /// beyond them.
    fn clamped_turn(&self, p: Point) -> f64 {
        let angle = (p.y - self.centre.y).atan2(p.x - self.centre.x);
        let turn = if self.clockwise {
            self.start_rad - angle
        } else {
            angle - self.start_rad
        }
        .rem_euclid(TAU);
        if turn <= self.sweep_rad {
            turn
        } else if turn - self.sweep_rad < TAU - turn {
            self.sweep_rad
        } else {
            0.0
        }
    }

    /// The farthest, mm, the arc between turns `from_rad` and `to_rad`
    /// lies from the straight move `a` to `b`.
    fn deviation_mm(&self, from_rad: f64, to_rad: f64, a: Point, b: Point) -> f64 {
        let sign = if self.clockwise { -1.0 } else { 1.0 };
        arc_chord_deviation_mm(
            self.centre,
            self.radius_mm,
            self.start_rad + sign * from_rad,
            sign * (to_rad - from_rad),
            a,
            b,
        )
    }
}

#[cfg(test)]
mod tests;
