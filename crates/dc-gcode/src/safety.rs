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
//!
//! S11 (`docs/specs/phase-2/functional.md`): a canned-cycle line is checked
//! as the moves the controller makes for it. The parser expands each one
//! ([`crate::canned`]) into synthetic blocks that every rule here reads like
//! written moves, so S1, S2 (with S15's Drill allowance), S6 and S7 hold for
//! the motion the machine makes. A series of cycles must end with `G80`
//! before any other motion (#314): the parser models `G98`'s return level
//! only from one `G80` to the next.
//!
//! S16 (#283; `docs/specs/phase-2/functional.md`, "Safety (addendum)"):
//! the parser reads `G53 G0 Z` lines as machine-coordinate lifts and
//! refuses any other `G53`; a lift goes to the project's tool-change
//! machine Z, from at or above safe Z, on a definition that documents
//! `G53`, and only right before a stop or the end; work Z is unknown after
//! it.
//!
//! S10 (`docs/specs/phase-2/functional.md`): every descending cut falls no
//! faster than its tool's plunge rate, its slope recomputed from the
//! written numbers ([`check_descent_rate`]). S9: every cut stays in its
//! operation's own material, worked out from the project
//! ([`crate::material`]).

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
use crate::material::{self, Material};
use crate::parse::{
    Block, CycleModel, MachineMove, MachinePosition, MotionKind, Word, is_code, parse_program_with,
};
use crate::transform::{MachineTransform, mm_per_unit};

/// The longest line, in characters, DreamCarve emits. grblHAL's line
/// buffer holds 256 bytes including the terminator.
pub const MAX_LINE_CHARS: usize = 255;

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
    /// 1-based program lines, end exclusive, from the operation's first
    /// motion line to its last.
    pub lines: Range<usize>,
}

/// The project's heights the program was written for (#283).
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
/// in `spans`, against S1–S7, S9, S10, S12 and S16, and returns the
/// program's extents. `heights` are the project's clearance height and
/// tool-change machine Z (S2, S16).
///
/// # Errors
///
/// The first violation found, as the matching [`ExportError`].
#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "the program, how it was written, and what it was written for, each needed"
)]
pub(crate) fn check(
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

/// [`check`], also returning the blocks the program parsed into, so the
/// emitter reads its pauses without parsing the program again (#71).
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
    // S9: every cut stays in its operation's own material.
    for span in spans {
        material::check(&blocks, post, transform, tolerances, span)?;
    }
    Ok((extents(&blocks, transform.output_units()), blocks))
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
pub(crate) fn r_centre(sx: f64, sy: f64, ex: f64, ey: f64, r: f64, machine_cw: bool) -> Point {
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
                    op: limits.op.map(|op| op.id().clone()),
                    line: mv.line,
                    side,
                    overhang_mm,
                });
            }
        }
    }
    Ok(())
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
    // it, because the user may change it at the controller (#55).
    let mut last_pause: Option<usize> = None;
    let mut spindle_on = false;
    let mut rpm: Option<f64> = None;
    let mut dwell_pending = false;
    // S15: the deepest a cut has reached at the XY of the last cut.
    let mut cut_to: Option<CutTo> = None;
    // S2 (#214): the cuts of the operation being read.
    let mut floors = Floors::new(stock_thickness_mm, tolerances);
    // S16: the line of the last machine-coordinate lift, while no motion
    // has followed it, and whether a stop has come since.
    let mut lifted: Option<(usize, bool)> = None;
    // S2 for #283: the span of the last move, and whether it has moved
    // across the work yet.
    let mut crossing: (Option<usize>, bool) = (None, false);
    for block in &blocks {
        let line = block.line;
        // S16 (#283): `G53 G0 Z<v>`, to the project's tool-change machine
        // Z, from a known Z at or above safe Z, where the bit already is.
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
        // S4, #43, #57: the user fits tools and switches the router at a
        // pause or an `M6` tool change, so it comes with the bit at safe Z.
        // Afterwards Z is unknown: the user may have jogged or set Z zero.
        let changes_tool = block.has('M', 6.0);
        if block.has('M', 0.0) || block.has('M', 1.0) || changes_tool {
            // #283: a stop is at the clearance height when it is set.
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
        // #68: Z after this move. A line without a Z word leaves Z where it
        // was, which after a pause is unknown, whatever Z the program last
        // wrote.
        let z_after = match (block.value('Z'), mv.to.z) {
            (Some(_), Some(z)) => Some((transform.project_z(z, mv.units), tolerance_mm)),
            _ => router.z_mm,
        };
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
            check_feed(mv, &limits, 2.0 * feed_quantum_mm_min(post, mv.units))?;
            check_descent_rate(mv, &limits, transform, tolerance_mm)?;
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
        // S2 as changed for #283: with a clearance height, an operation's
        // first move across the work and every one outside the operations
        // run at or above it. Links within an operation are held to the
        // toolpath by S6, whose sequencer links parts at it.
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
/// read this one state (#43, #68). A tool's first cut needs an `M0` or
/// `M1` after the last cut and the last `M6`: the user fits a tool at an
/// `M6`, so an `M6` never counts as the pause to turn the router on.
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
/// 10.7.20; UCCNC user's manual 1.0047, 4.1.1). Each only cancels a mode
/// DreamCarve never turns on, but it is accepted only from a definition
/// whose header writes it ([`header_cancel_codes`]): LinuxCNC, grblHAL,
/// GRBL and RS274/NGC do not have them, and in other dialects `G50` is a
/// different command. `G51` and `G68`, which scale and rotate every
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
/// The `M` codes of grblHAL's tool change, accepted when the definition's
/// tool change uses `M6` (#57) and its mode is `"m6"` (#95): `M6` itself, checked like a pause, and
/// `M61`, which sets the current tool number without a stop or a move.
/// GRBL 1.1 has neither.
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
    // #312: true in the header, before the first move or spindle start,
    // and again after each stop until the next one: the two places `emit`
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

/// #312: `word`, one of the definition's header cancels (`G50`, `G69`),
/// is accepted only as `emit` writes it: alone on its line, but for a line
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
/// `dc_cam::entry` keeps a helix's moves above (#265).
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
    /// operation, the strictest of them all.
    fn for_line(line: usize, spans: &[OpSpan<'a>], stock_thickness_mm: f64) -> Self {
        if let Some(span) = spans.iter().find(|s| s.lines.contains(&line)) {
            return Self {
                op: Some(span.op),
                tool: Some(span.tool),
                floor_mm: -span.op.through_cut_allowance_mm(),
                safe_z_mm: stock_thickness_mm + span.op.safe_z_mm(),
            };
        }
        let floor_mm = spans
            .iter()
            .map(|s| -s.op.through_cut_allowance_mm())
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
/// unknown: at program start and after a pause until Z is written (#68).
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

/// S2 as changed in #214: a rapid straight down from safe Z may stop below
/// it, over the start of a ramp or helix, where the tool is in the air: at
/// or above
/// the stock top plus [`ENTRY_CLEARANCE_MM`], or that far above the lowest
/// Z a cut of the same operation has reached through its XY. The cuts of
/// the operation being read, in project space, are kept to find that Z.
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
fn differs(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !approx_eq(a, b, EPS_MM),
        (None, None) => false,
        _ => true,
    }
}

/// S3: a cut has a positive feed, and it is one of its tool's rates. A
/// straight move that falls while it moves in XY (an entry ramp or helix)
/// may also run anywhere between the tool's plunge rate and its feed: its
/// feed is the cutting feed scaled so its vertical rate stays under the
/// plunge rate (S10's feed scale, `dc_cam::entry`), which lands between
/// the two. Whether that vertical rate is right is S10's check
/// ([`check_descent_rate`]).
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
        let on_a_rate = [tool.feed_mm_min, tool.plunge_mm_min]
            .into_iter()
            .flatten()
            .any(|rate| approx_eq(rate, feed_mm_min, tolerance_mm_min));
        let scaled_descent = is_descent(mv)
            && tool
                .plunge_mm_min
                .zip(tool.feed_mm_min)
                .is_some_and(|(plunge, cut)| {
                    feed_mm_min >= plunge - tolerance_mm_min
                        && feed_mm_min <= cut + tolerance_mm_min
                });
        let matches = on_a_rate || scaled_descent;
        if !matches {
            return Err(ExportError::FeedNotFromTool { line, feed_mm_min });
        }
    }
    Ok(())
}

/// True for a `G1` that falls while it moves in XY, every coordinate
/// known.
fn is_descent(mv: &MachineMove) -> bool {
    let (from, to) = (mv.from, mv.to);
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (from.x, from.y, from.z, to.x, to.y, to.z)
    else {
        return false;
    };
    mv.kind == MotionKind::Linear && tz < fz && (tx - fx).hypot(ty - fy) > 0.0
}

/// S10: a cutting move that descends falls no faster than its tool's
/// plunge rate: feed × sin(slope) ≤ plunge, with the slope recomputed from
/// the written numbers (`docs/specs/phase-2/functional.md`, S10). Every
/// descending cut is held to it, whatever made it: an entry ramp or helix,
/// a smooth tab's down slope (#157), a straight plunge (slope 90°, so its
/// feed must be at most the plunge rate) or an arc that descends. The
/// written move may differ from the toolpath's by the rounding of its
/// numbers, `quantum_mm` (one unit in the last written place, the same
/// for lengths and feeds): a feed scaled to the plunge rate exactly must
/// pass, so the check takes the slowest vertical rate those numbers allow,
/// the feed one unit lower, the drop one unit shorter and the length two
/// longer. A move whose start is not known is skipped; S2 and S4 keep a
/// program from cutting from an unknown position.
fn check_descent_rate(
    mv: &MachineMove,
    limits: &Limits<'_>,
    transform: &MachineTransform,
    quantum_mm: f64,
) -> Result<(), ExportError> {
    let (Some(op), Some(tool)) = (limits.op, limits.tool) else {
        return Ok(());
    };
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
    else {
        return Ok(());
    };
    let drop_mm = transform.project_z(fz, mv.units) - transform.project_z(tz, mv.units);
    if drop_mm <= 0.0 {
        return Ok(());
    }
    let scale = mm_per_unit(mv.units);
    let run_mm = match mv.kind {
        MotionKind::Rapid => return Ok(()),
        MotionKind::Linear => (tx - fx).hypot(ty - fy) * scale,
        MotionKind::ArcCw | MotionKind::ArcCcw => {
            let machine_cw = mv.kind == MotionKind::ArcCw;
            let centre = match mv.arc.r {
                Some(r) => r_centre(fx, fy, tx, ty, r, machine_cw),
                None => Point::new(fx + mv.arc.i.unwrap_or(0.0), fy + mv.arc.j.unwrap_or(0.0)),
            };
            let arc = Arc::from_ends(Point::new(fx, fy), Point::new(tx, ty), centre, !machine_cw);
            arc.radius_mm * arc.sweep_rad.abs() * scale
        }
    };
    let feed_mm_min = mv.feed.unwrap_or(0.0) * scale;
    let slowest_mm_min = (feed_mm_min - quantum_mm).max(0.0) * (drop_mm - quantum_mm).max(0.0)
        / (run_mm.hypot(drop_mm) + 2.0 * quantum_mm);
    let plunge_mm_min = tool.plunge_mm_min.unwrap_or(0.0);
    if slowest_mm_min > plunge_mm_min + EPS_MM {
        return Err(ExportError::EntryTooSteep {
            op: op.id().clone(),
            line: mv.line,
        });
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
///   it never travels sideways below either end); in a Drill written as
///   canned cycles, also by none, or by a rapid in XY alone over it, when
///   a rapid in Z alone passes through its end point and the toolpath
///   goes on the same way along that line (#305,
///   [`match_rapid_on_the_way`]);
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
    let mismatch = |index| ExportError::MotionMismatch {
        op: span.op.id().clone(),
        index,
    };
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
    // Where the last straight cut of the toolpath ended, in XY.
    let mut last_cut: Option<Point> = None;
    let mut next_dwell = 0;
    let mut cursor = motions
        .iter()
        .position(|m| !is_retract(m.mv))
        .unwrap_or(motions.len());
    let mut previous: Option<Point3> = None;
    let moves = &span.toolpath.moves;
    let mut index = 0;
    while let Some(mv) = moves.get(index) {
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
        // Inside a hole: in Z alone, where the last cut was.
        let in_hole = || {
            let at = |p: Point| p.distance_to(mv.to.xy()) <= tolerance_mm;
            previous.is_some_and(|p| at(p.xy())) && last_cut.is_some_and(at)
        };
        let consumed = match mv.kind {
            MoveKind::Rapid => match_rapid(rest, mv.to, already_there)
                .or_else(|| {
                    let gap_mm = reentry_gap_mm.filter(|_| in_hole())?;
                    match_in_hole_rapid(rest, mv.to, gap_mm)
                })
                .or_else(|| {
                    // #305: a cycle from the clearance height goes through
                    // safe Z without stopping.
                    canned
                        .then(|| match_rapid_on_the_way(rest, mv.to, moves.get(index + 1)))
                        .flatten()
                }),
            MoveKind::Dwell { seconds } => {
                // A `G4` with these seconds after the last motion matched
                // and before the next; it moves nothing.
                let after = cursor
                    .checked_sub(1)
                    .and_then(|i| motions.get(i))
                    .map(|m| m.block);
                let before = rest.first().map_or(usize::MAX, |m| m.block);
                let found = dwells
                    .iter()
                    .enumerate()
                    .skip(next_dwell)
                    .find(|(_, (block, s))| {
                        after.is_none_or(|a| *block > a)
                            && *block < before
                            && (s - seconds).abs() <= DWELL_TOLERANCE_S
                    })
                    .map(|(i, _)| i);
                found.map(|i| {
                    next_dwell = i + 1;
                    0
                })
            }
            MoveKind::Linear => match_linear(rest, mv.to, already_there),
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let clockwise = matches!(mv.kind, MoveKind::ArcCw { .. });
                match previous.map(|start| ToolpathArc::new(start, mv.to, centre, clockwise)) {
                    Some(Some(arc)) => match_arc(rest, &arc, transform, tolerances.flatten_mm),
                    // Too small to be an arc: `emit` writes a straight cut.
                    Some(None) => match_linear(rest, mv.to, already_there),
                    // `emit` cannot write an arc without a start.
                    None => None,
                }
            }
        };
        if let Some(consumed) = consumed {
            cursor += consumed;
            previous = Some(mv.to);
            if mv.kind == MoveKind::Linear {
                last_cut = Some(mv.to.xy());
            }
            index += 1;
            continue;
        }
        // S12: an arc where the toolpath has straight moves (or a shallow
        // corner arc a run may take as its chord).
        let fitted = (|| {
            let fitted = rest
                .first()
                .filter(|read| matches!(read.mv.kind, MotionKind::ArcCw | MotionKind::ArcCcw));
            let (Some(read), Some(arc_fit_mm)) = (fitted, tolerances.arc_fit_mm) else {
                return Err(mismatch(index));
            };
            if !run_eligible(previous, mv, arc_fit_mm) {
                return Err(mismatch(index));
            }
            match_fitted_arc(rest, moves, index, previous, transform, arc_fit_mm).ok_or_else(|| {
                ExportError::ArcFitViolation {
                    op: span.op.id().clone(),
                    line: read.mv.line,
                }
            })
        })();
        let (lines, covered) = match fitted {
            Ok(fitted) => fitted,
            // An arc of at most a half turn that ends where the controller
            // already is lies wholly within the rounding of it: `emit`
            // writes it as nothing, as it does such a straight move. Tried
            // last, so a fitted run that takes the arc is matched first.
            Err(_) if already_there && written_as_nothing(previous, mv) => (0, 1),
            Err(error) => return Err(error),
        };
        cursor += lines;
        index += covered;
        previous = moves.get(index.wrapping_sub(1)).map(|m| m.to);
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

/// S11 (#305): how many of `rest` a rapid to `target` takes when the
/// controller goes through `target` without stopping there: one rapid of
/// the controller's in Z alone, at the target's XY, passes through it on
/// the way to a Z beyond it, and the toolpath goes on the same way along
/// that line, by a rapid in Z alone (`next`), or the target is the
/// toolpath's last move and the controller's rapid goes up, the retract
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

/// S12: how many lines of `rest` and how many toolpath moves from `index`
/// a fitted run accounts for, if the lines may stand for the moves.
///
/// The lines are a chain of consecutive arc moves about one centre, one
/// way, each sweeping at most 90°, the first starting at `previous`, the
/// end of the toolpath move before, and the last ending at the end of a
/// toolpath move: a fitted run is split into sub-arcs whose ends between
/// are on the circle, and only the run's ends are vertices. The chain
/// covers at least [`MIN_RUN_CHORDS`] moves, all straight cuts or shallow
/// corner arcs (sagitta within `arc_fit_mm`) at the run's Z. Every vertex
/// between, and every shallow arc's midpoint, lies on the chain within
/// `arc_fit_mm` plus the lines' rounding, and the vertices turn the
/// chain's way about its centre. The chain also lies within that of every
/// move it covers along the move's whole length (#226): a straight move
/// between two points on the circle is bowed off by the arc's sagitta over
/// it, which the vertices alone never show. The chain may stop short of its last
/// line where the toolpath goes on with a move a run cannot take, if the
/// line before ends at the vertex there.
fn match_fitted_arc(
    rest: &[ReadMove<'_>],
    moves: &[Move],
    index: usize,
    previous: Option<Point3>,
    transform: &MachineTransform,
    arc_fit_mm: f64,
) -> Option<(usize, usize)> {
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
        // drifted 0.012 mm in five pieces, #137). Every vertex is still
        // checked against its own piece below.
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
    let done =
        |piece: usize, covered: usize, vertex: Point3, cross: f64| -> Option<(usize, usize)> {
            let current = pieces.get(piece)?;
            let turns_with = if clockwise { cross < 0.0 } else { cross > 0.0 };
            (near(vertex, current.end, tolerance_mm) && turns_with && covered >= MIN_RUN_CHORDS)
                .then_some((piece + 1, covered))
        };
    for mv in moves.get(index..)? {
        if !run_eligible(Some(vertex), mv, arc_fit_mm) || !approx_eq(mv.to.z, start.z, tolerance_mm)
        {
            // The toolpath goes on with a move no run takes: the chain
            // must end at the vertex before it.
            return done(piece, covered, vertex, cross);
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
                if current.turn_to(p, allowed_mm).is_some() {
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
        // #226: the chain stays within the allowance of the move along its
        // whole length, not only at its ends: over the part of every piece
        // between the move's ends, the farthest point from the move's
        // chord (for a straight move between points on the circle, its
        // sagitta). Each end is placed on each piece, not assumed to lie
        // before or past it (#262): a vertex up to the angle slack past a
        // piece's end counts as on that piece, and on a circle so small
        // that the slack is a whole chord it lies inside the next piece,
        // whose part before it the move does not span.
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
        if piece + 1 == pieces.len() && near(vertex, chain_end(&pieces)?, tolerance_mm) {
            return done(piece, covered, vertex, cross);
        }
    }
    done(piece, covered, vertex, cross)
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
    let (sx, sy, ex, ey) = (mv.from.x?, mv.from.y?, mv.to.x?, mv.to.y?);
    let machine_centre = match mv.arc.r {
        Some(r) => r_centre(sx, sy, ex, ey, r, machine_cw),
        None => Point::new(sx + mv.arc.i.unwrap_or(0.0), sy + mv.arc.j.unwrap_or(0.0)),
    };
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
    /// lies from the straight move `a` to `b` (#226).
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
mod tests {
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
    /// no part to keep out of, S9 (#177) passes them, so each test sees
    /// only its own rule.
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

    /// S7 (#62): the cutter's edge must stay on the stock. With a 6 mm
    /// tool a cut 3 mm in from every edge passes; one along the left edge
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

    /// #62: a program's extents are its machine coordinates' range, arc
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
    /// `machine_lift_mm` (S16, #283).
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
            material: &Material::Unchecked,
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

    /// A ramp at (20, 20) on the fixture's 6 mm stock (machine Z 0 at the
    /// stock top, safe Z 5): rapid straight down to machine Z `first`,
    /// ramp to Z −1 and back along X to (20, 20), lift, rapid straight
    /// down to `second` at (`x`, 20), ramp to Z −2 at (45, 20), out.
    fn entry_program(first: &str, x: &str, second: &str) -> String {
        format!(
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX20 Y20\nZ{first}\nG1 X30 Z-1 F1000\nX20\nG0 Z5\nX{x}\nZ{second}\nG1 X45 Z-2\nG0 Z5\nM5\n"
        )
    }

    /// S2 as changed in #214: a ramp or helix starts 1 mm above the stock
    /// top, or 1 mm above the floor the operation's pass before cut there,
    /// and a rapid straight down from safe Z takes the tool there. A rapid
    /// in Z alone may end at or above the stock top plus the entry
    /// clearance anywhere, and at or above the entry clearance over the
    /// lowest Z a cut of the same operation has reached through its XY.
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
                material: &Material::Unchecked,
                lines: 1..10,
            },
            OpSpan {
                op: &second,
                tool: &tool,
                toolpath: &toolpath,
                material: &Material::Unchecked,
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

    /// S2 as changed in #214 and #227: a straight plunge (`entry:
    /// plunge`, an Engrave pass, a corner-relief notch) is reached by the
    /// same rapid straight down from safe Z as a ramp or helix, stopping
    /// 1 mm above the stock top or above the floor the operation cut
    /// there; one that stops lower is still refused.
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

    /// S3 with S10's feed scaling (slice 9): a ramp or helix move, one that
    /// falls while it moves in XY, runs at the cutting feed scaled down so
    /// its vertical rate stays under the plunge rate, which puts its feed
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
        let level =
            "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F300\nX10 F650\nG0 Z5\nM5\n";
        let err = check_text(level).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
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
            ("G90.1", "G90.1"),
            ("G41", "G41"),
            ("G43", "G43"),
            // A canned cycle with canned cycles off (#154).
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

    /// #154: the LinuxCNC and Mach3 headers cancel cutter compensation
    /// (`G40`), tool length offset (`G49`) and any canned cycle (`G80`),
    /// and select incremental arc centres (`G91.1`). Each only sets modal
    /// state, to what DreamCarve already assumes, so they pass with canned
    /// cycles off.
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
    /// UCCNC v3, and from no other: LinuxCNC, grblHAL, GRBL and RS274/NGC
    /// do not have them, and on other dialects `G50` means something else.
    /// `G51` and `G68`, which scale and rotate what the program writes,
    /// are refused everywhere: the safety pass reads the program as
    /// written.
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

    /// #312: on a definition whose header writes them, `G50` and `G69` are
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
        let after_stop =
            "G21 G90\nG54\nG50\nG69\nG0 Z11\nM0\nG50\nG69\nG21 G90\nG54\nG0 Z11\nM30\n";
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
                let resumed =
                    format!("G21 G90\nG54\nG0 Z11\nM0\nG21 G90\nG54\nG0 Z11\n{word}\nM30\n");
                misplaced(id, &resumed, 8, word, "outside");
            }
        }
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

    /// S4 (manual), #34: a tool's first cut must follow an `M0` issued at
    /// safe Z after the last cut, and the program must pause again at safe
    /// Z after its last cut. Safe Z is machine Z5 in the fixture.
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

        // #68: a tool change is not the pause to turn the router on, nor
        // does a pause before it count: the router needs its own pause
        // after the `M6`.
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

    /// S4, #34: the automatic rules stand. The fixture is automatic, a
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
        let manual_lifted = "G21 G90\nG54\nG0 Z5\nM0\nG21 G90 G54 G0 Z5\nM0\nG21 G90 G54 G0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM0\n";
        check_manual(manual_lifted).unwrap();
    }

    /// S2, S4, #68: Z is unknown after a pause, because the user may have
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

    /// #68, #57: the same holds after an `M6` tool change, where the user
    /// sets Z zero with the new bit.
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

    /// #120: a tool's set `number` is its `T` word, so `M61 Q<n>` and
    /// `T<n> M6` carry any number, not only a place in the tool list. The
    /// `M6` and `M61` checks are the same for them: accepted with `"m6"`,
    /// `M6` still a pause that must be at safe Z, both refused in
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
            let err =
                check_text_on(text, bundled_grblhal(), SpindleControl::Automatic).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { word: w, .. } if w == word),
                "{text:?}: {err:?}"
            );
        }
    }

    /// #95: with `"separate_programs"`, the bundled default, each program
    /// has one tool, so `M6` and `M61` are refused even on grblHAL.
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
    /// one program with `M6` tool changes (#95).
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
            material: &Material::Unchecked,
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
    /// (line 7) is S2's rapid down to an entry (#214) and passes; the
    /// retract to it (line 9) does not start at safe Z, and fails.
    /// (changed in #214: line 7 failed.)
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

    /// S13 O7, S6 (#254): the post-processor preserves order, so a program
    /// whose shapes are reordered does not reproduce the toolpath. The two
    /// shapes of one operation, cut at Y 50 then Y 70, pass as written
    /// (and as the emitter writes them); with the two blocks swapped,
    /// each intact with its own rapids and retract, the program fails
    /// with `MotionMismatch` naming the operation, at the first move.
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

    /// S13 O7, S6 (#254): two operations' sections swapped. Each section
    /// is one shape block, so both are the same length and each
    /// operation's lines hold the other's moves; the first operation
    /// checked, `o1`, fails with `MotionMismatch`. In order, it passes.
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
                    material: &Material::Unchecked,
                    lines: 6..10,
                },
                OpSpan {
                    op: &second,
                    tool: &tool,
                    toolpath: &b,
                    material: &Material::Unchecked,
                    lines: 10..text.lines().count() + 1,
                },
            ];
            check(
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

    /// #226: an arc within the tolerance of every chord it replaces, its
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
            let text =
                chord_run_text("G3 X57.071 Y57.071 I-10 J0 F1000\nX50 Y60 I-7.071 J-7.071\n");
            check_against_with(&text, &toolpath, Some(0.01))
                .unwrap_or_else(|e| panic!("{n} chords, two arcs: {e:?}"));
        }
    }

    /// #262: a move that spans the end of one piece and the start of the
    /// next is held to the part of both pieces between its ends. A 20°
    /// chord from 40° to 60° under two `G3` pieces meeting at 50° bows
    /// 0.15 mm off its arc, so S12 refuses the chain; with that stretch in
    /// 2° chords too it passes. (Each end of a move is placed on each
    /// piece; the fix for #262 must not stop measuring across the joint.)
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

    /// #153: the Drill toolpath [`dc_cam::generate_drill`] makes for a
    /// 2 mm peck, 3 mm deep, with the retract plane 1 mm above the 6 mm
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
            check_drill_on(&text("2"), &toolpath, &post)
                .unwrap_or_else(|e| panic!("{word} Q2: {e:?}"));
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

    /// #305: S6 joins toolpath rapids along one vertical line only where
    /// they lie on one rapid of the controller's in the same direction,
    /// and only for a Drill written as canned cycles. A toolpath that goes
    /// up past safe Z and then back down is not the cycle's one rapid up;
    /// and a plain program with one `G0 Z` where the toolpath has two
    /// still fails, as before, while the program that writes both passes.
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

    // S9 and S10 (slice 16; `docs/specs/phase-2/design.md`, "Test plan").

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
            format!(
                "G21 G90\nG54\nG0 Z5\nM3 S18000\nG4 P3\nX0 Y0\nG1 Z-1 F{f}\nX10 F1000\nG0 Z5\nM5\n"
            )
        };
        check_text(&plunge("300")).unwrap();
        let err = check_text(&plunge("1000")).unwrap_err();
        assert!(
            matches!(err, ExportError::EntryTooSteep { line: 7, .. }),
            "{err:?}"
        );
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

    /// S10 holds on every descending move, a tab's down slope included
    /// (#157): one at the cutting feed that falls faster than the plunge
    /// rate fails; scaled, it passes. The up slope is not a descent.
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

    /// #157: the sample Profile with smooth tabs, generated, exports: each
    /// down slope (2 mm over 3 mm) runs at its plunge-limited feed.
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
            assert!(rate <= 300.0 * 1.01, "a down slope falls at {rate} mm/min");
        }
    }

    /// The slice 9 island Pocket (`dc_cam::pocket` tests): 300 × 200 × 6 mm
    /// stock, a 100 × 50 mm rectangle at (20, 20) with a 20 mm circle
    /// island at its centre (70, 45), an offset Pocket 4 mm deep in the
    /// 6.35 mm tool's 2 mm steps, 40 % step-over, entering by `entry`, with
    /// `allowance_mm`.
    fn island_pocket(
        entry: dc_project::Entry,
        allowance_mm: f64,
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
            allowance_mm,
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
            assert!(rate <= 300.0 * 1.01, "a helix chord falls at {rate} mm/min");
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
    /// wall, into its allowance, or onto an island fails with
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

        // A 0.5 mm allowance keeps the tool 0.5 mm off the wall.
        let left = dc_project::PocketOp {
            allowance_mm: 0.5,
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

    /// S9 on a raster Pocket round the 20 mm circle island (#202): with a
    /// wall pass the links between line ends against the island follow
    /// its tool-centre ring, and the program passes, with an allowance
    /// too, at angles that meet the island's curve everywhere.
    #[test]
    fn raster_edge_links_round_an_island_pass_s9() {
        for allowance_mm in [0.0, 0.5] {
            let (project, op) = island_pocket(dc_project::Entry::Plunge, allowance_mm);
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
                        "{allowance_mm} mm, {angle_deg}°, {wall_pass:?}: {text:?}"
                    );
                }
            }
        }
    }

    /// S9 on an offset Pocket at step-over 1.0 (#267): the corner clean-up
    /// rings are cut from inside the laps of the rings that leave them,
    /// out from a corner and back, and the program passes, on the rect with
    /// and without its island, with an allowance too, the wall pass last
    /// or first, plunging or by helix.
    #[test]
    fn offset_corner_cleanups_pass_s9() {
        for allowance_mm in [0.0, 0.5] {
            for entry in [dc_project::Entry::Plunge, helix()] {
                let (island, op) = island_pocket(entry, allowance_mm);
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
                            "{allowance_mm} mm, {:?}, {wall_pass:?}: {text:?}",
                            op.entry
                        );
                    }
                }
            }
        }
    }

    /// S9 on a raster Pocket whose wall curves inward (#218): an L, whose
    /// reflex corner rounds the tool-centre region by the tool radius, and
    /// a rect with a half-disc bay of radius 15 pushed in from its top.
    /// With a wall pass the links between line ends against those arcs
    /// follow the tool-centre ring, and the program passes, with an
    /// allowance too, at several angles.
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
            for allowance_mm in [0.0, 0.5] {
                let (mut project, op) = island_pocket(dc_project::Entry::Plunge, allowance_mm);
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
                            "{allowance_mm} mm, {angle_deg}°, {wall_pass:?}: {text:?}"
                        );
                    }
                }
            }
        }
    }

    /// S9 alone on hand-written `text`, as operation `op` of `project` with
    /// the project's tool.
    fn check_material_text(
        text: &str,
        project: &Project,
        op: &Operation,
    ) -> Result<(), ExportError> {
        let post = project.post_processor.resolve().unwrap();
        let transform = MachineTransform::new(project, &post).unwrap();
        let blocks = parse_program_with(text, &CycleModel::of(&post)).unwrap();
        let tool = project.tools[0].clone();
        let material = Material::of(project, op);
        let toolpath = Toolpath::default();
        let span = OpSpan {
            op,
            tool: &tool,
            toolpath: &toolpath,
            material: &material,
            lines: 1..text.lines().count() + 1,
        };
        let tolerances = Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm: project.arc_fit_tolerance_mm,
        };
        material::check(&blocks, &post, &transform, tolerances, &span)
    }

    /// S9 on a hand-written program for the sample rectangle's Pocket
    /// (X 10 to 60, Y 10 to 40, stock-top origin): a cut past the wall
    /// fails, so does an arc whose ends are inside but whose bulge takes
    /// the tool past it, and so does a plunge outside the region.
    #[test]
    fn hand_written_pocket_cut_off_material_fails_s9() {
        let project = sample_project();
        let op = Operation::Pocket(dc_project::test_support::sample_pocket());
        let program =
            |cuts: &str| format!("G21 G90\nG54\nG0 Z5\nX20 Y25\nG1 Z-3 F300\n{cuts}G0 Z5\n");
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

    /// S9's notch rule (slice 12, `crate::material`): with corner relief
    /// on, a Pocket cut may come nearer a wall than the tool radius in a
    /// corner's relief zone. At the sample rectangle's corner (10, 10)
    /// with the 6.35 mm tool, the dogbone from the miter point (13.175,
    /// 13.175) to one radius from the corner, (12.245, 12.245), and the
    /// T-bone to (13.175, 10) pass; with relief off the same dogbone
    /// fails at its cut.
    #[test]
    fn pocket_notch_inside_its_corner_zone_passes_s9() {
        use dc_project::CornerRelief;
        let project = sample_project();
        let program = |cuts: &str| {
            format!("G21 G90\nG54\nG0 Z5\nX13.175 Y13.175\nG1 Z-3 F300\n{cuts}G0 Z5\n")
        };
        let dogbone = program("X12.245 Y12.245 F1000\nX13.175 Y13.175\n");
        check_material_text(&dogbone, &project, &relief_pocket(CornerRelief::Dogbone)).unwrap();
        let t_bone = program("X13.175 Y10 F1000\nX13.175 Y13.175\n");
        check_material_text(&t_bone, &project, &relief_pocket(CornerRelief::TBone)).unwrap();
        let err = check_material_text(&dogbone, &project, &relief_pocket(CornerRelief::None))
            .unwrap_err();
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

    /// The sample project's outside Profile (#177).
    fn sample_profile_op() -> Operation {
        sample_project().operations[0].clone()
    }

    /// S9 for Profile (#177): the sample's outside Profile of the
    /// rectangle X 10 to 60, Y 10 to 40 with the 6.35 mm tool. A ramp
    /// that zigzags out into the waste and back to the tool-centre path
    /// (X 6.825), then a lap along it, passes; the same ramp swung into
    /// the part, its tool centre at X 12, fails with `OffMaterial` at the
    /// ramp's line.
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

    /// S9 for Profile (#177): a lead-in arc onto the rectangle's right-hand
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

    /// S9's lap rule (#177): with the second part 4 mm from the rectangle,
    /// closer than the 6.35 mm bit (`toolpaths_overlap`), the generated
    /// Profile still exports: each lap runs at the radius from its own
    /// outline, and the gouge into the neighbour is the one dc-cam warns
    /// about. A cut off the lap, to a tool centre in the middle of the
    /// gap, 2 mm from both parts, still fails.
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

    /// S9's notch rule on an outside Profile (#177): the generated dogbone
    /// in an L part's inside corner passes; the same program checked as a
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

    /// #231: the sample's outside Profile with T-bones of a 60 × 40 mm
    /// frame at (90, 60) with a 30 × 20 mm hole at (105, 70). The T-bone
    /// in the hole's corner (105, 70) runs from the miter point
    /// (108.175, 73.175) to (108.175, 70) on the hole's long wall. Its
    /// return leg, written from a start the rounding moved 0.0005 mm into
    /// the part, passes; one starting 0.1 mm inside the part fails: the
    /// slack S9 judges the side with does not open the part. The tool
    /// gets to each start by rapids, which S9 does not hold to the
    /// material, so only the return leg is judged.
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

    /// S9 for an inside Profile (#177): the tool centre stays inside the
    /// hole, a radius from its walls, as a Pocket's; a cut that crosses
    /// out of the hole fails.
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
    /// shapes in different regions (#192). An inside Profile of the sample
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

    /// #199: S9 refuses what `generate_profile` refuses. An On line
    /// Profile of the sample rectangle (X 10 to 60, Y 10 to 40) with a
    /// round hole at (35, 25): a lap along the hole's line passes when the
    /// ring around it is 10 mm wide, and fails at its first arc when it is
    /// 5 mm (narrower than the 6.35 mm bit, wider than its radius) or
    /// 2 mm, although it runs on a target line: the two lines' cuts would
    /// cut through the ring. A cut along the left edge, 15 mm from the
    /// 10 mm hole, still passes.
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

    /// #175: an On line Profile cuts its targets' open paths, so S9 holds
    /// it to them as an Engrave: the generated program of an open line
    /// (with the sample's tabs and a ramp along the line) passes, and a
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

    /// #207: a Left or Right Profile's cuts stay on the open path offset
    /// by the tool radius to the chosen side, and keep the radius from
    /// every line of the operation and from the strip one radius wide each
    /// line keeps on its other side. The sample with an open line from
    /// (10, 60) to (70, 60) cut Left, with its tabs and a ramp: the
    /// generated program passes, and so does a hand-written cut at Y 63.175
    /// (the 6.35 mm bit's radius to the left of the line). Cuts on the
    /// line, at the radius to its right, half a radius off the offset, or
    /// past the line's end along it fail at their plunge; so does the same
    /// cut when the line is cut Right. (The cut past the end plunges on
    /// the path's end and fails at the move beyond it.)
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
        let err =
            check_material_text(&cut((10.0, 63.175), (70.0, 63.175)), &right, &op).unwrap_err();
        assert!(
            matches!(&err, ExportError::OffMaterial { line: 5, .. }),
            "the Left cut under Right: {err:?}"
        );
    }

    /// #210: S9 refuses what `generate_profile` refuses. An On line
    /// Profile of the sample rectangle (X 10 to 60, Y 10 to 40) with an
    /// open line 2 mm above its bottom edge: the generator refuses it
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

    /// #221: S9 refuses what `generate_profile` refuses for a line closer
    /// than the bit to itself. On line, with the sample rectangle (X 10 to
    /// 60, Y 10 to 40, drawn at the origin): an open U in the part with
    /// legs 2 mm apart is refused by the generator, and a hand-written cut
    /// along it fails at its plunge, although it is on a target line: the
    /// cuts of the two legs would cut through the part between them. Legs
    /// 8 mm apart pass. An outline with a 3 mm neck fails the same way;
    /// one with a 3 mm notch passes, as the generator cuts it, because the
    /// notch between its stretches is waste. The generated programs of
    /// the wide U, a 60° V and the notched outline pass.
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

    /// #223: S9 refuses what `generate_profile` refuses for a part the bit
    /// removes whole. On line, a 3 mm square part (drawn at (20, 10), so
    /// X 30 to 33, Y 20 to 23) is refused by the generator
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
        // stays four lines (#229).
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

    /// S9 for Profile (#177) on generated programs: the sample's tabbed
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
    /// 0.5 mm last pass allowance, passes every rule of the safety pass
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
                        allowance_mm: 0.5,
                        thickness_mm: None,
                        reverse: false,
                    },
                    dc_project::LastPass {
                        allowance_mm: 0.5,
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

    /// #105: programs cut round placed tabs pass the safety pass (S1–S7,
    /// S9, S10, S12): the sample's tabs placed from its request, with a
    /// fifth, taller and smooth, on the right-hand edge, on each side, with
    /// a plunge and with a ramp, and Inside and Outside with a lead. Every
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
                    transform: dc_geometry::Transform::translate(
                        15.0 + 25.0 * col,
                        15.0 + 25.0 * row,
                    ),
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

    /// #177: S9 on a 20-part Profile with ramps, leads and tabs checks
    /// each move against the outlines near it, so it stays fast in a debug
    /// build.
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

    /// #221: S9 measures every On line cut against its own line's
    /// stretches, but only those near the cut, so an outline of 2000 chords
    /// cut On line in three passes stays fast in a debug build.
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

    /// #158: S9 on the helix Pocket checks each of its more than eight
    /// hundred descent chords as a segment against the region's outlines,
    /// not as the union of swept capsules (6.7 s in a debug build in slice
    /// 9), so it stays fast in a debug build. (changed in #214: more than a
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

    /// #142, #137: a long fitted run whose sub-arc centres drift by I/J
    /// snapping passes S12. The first and last of four centres sit 0.009 mm
    /// out along their bisectors and differ by 0.012 mm on each axis once
    /// rounded, more than the arc-fit tolerance plus one unit in the last
    /// place (0.011 mm); each differs from its neighbour by under 0.008 mm.
    /// The run's 102 chords put no vertex at the sub-arcs' ends, so the
    /// chain must not break there. (changed in #226: 51 chords, which bow
    /// 0.011 mm from the arc, past the tolerance.)
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

    /// #142: two abutting runs on different circles stay separate chains.
    /// A quarter of the radius 10 circle about (50, 50) from +X to
    /// (50, 60), then a quarter of the radius 15 circle about (50, 45) on
    /// to (35, 45): written as one arc each, about its own centre, the
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

    // S16 (#283): machine-coordinate moves.

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
    /// no machine-coordinate move (Mach3, UCCNC, generic; #306), whatever
    /// the project sets.
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
                let err = check_text_lifting(&text, post, SpindleControl::Automatic, Some(-5.0))
                    .unwrap_err();
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
            material: &Material::Unchecked,
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

    /// #304 review, S2 and S6 as changed for #283: with a clearance height
    /// (here work Z 20 over the fixture's 6 mm stock, absolute 26), an
    /// operation's first move across the work, every move across outside
    /// the operations (the park move), and every stop are at or above it;
    /// moves across inside an operation after its first stay held to safe
    /// Z (work Z 5). Without a clearance height the same low moves pass.
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
}
