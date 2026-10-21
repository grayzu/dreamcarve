// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/postprocessor.ts

//! G-code emission (`docs/specs/phase-1/design.md`, "Algorithms →
//! Emission"; `docs/specs/phase-1/functional.md`, S1–S7).
//!
//! Ported from `runPostProcessor`, without canned cycles, coolant, or arc
//! fitting (phase 1 has none of them). Re-derived for Y-up: toolpaths are
//! Y-up and so is the machine, so arc direction carries over unchanged
//! unless the definition's axis map mirrors the plane. Changes from
//! upstream, from the controller facts in `dc-project/src/definitions/README.md`
//! ("Verification"):
//!
//! - Missing feed, plunge, or RPM is an error; nothing is defaulted. With
//!   manual spindle control the RPM is optional: it is only a hint in the
//!   message of the pause before each tool's first cut.
//! - Placeholder text is reduced to printable ASCII without comment
//!   delimiters or real-time command characters, and capped in length.
//! - Coordinates are scaled to inches when the project's display units are
//!   inches (upstream stores projects in their display units).
//! - The spindle starts with `spindle_on_cw` only, followed by the spin-up
//!   dwell; coolant is never switched on. With manual spindle control there
//!   are no spindle words: the program retracts to safe Z and pauses with
//!   `M0` and a `(MSG, ...)` comment for the user to switch the router on
//!   before each tool's first cut, and off before a tool change and at the
//!   end. Z is unknown after the router-on pause, so the program retracts
//!   to safe Z again before it moves in XY (#68).
//! - There is no stop for the first tool: the user fits it and sets work
//!   zero with it before starting the program (#57). By default a program
//!   has one tool: [`emit_programs`] writes one program per tool run, which
//!   every sender and controller stops between (#95). Only with the
//!   post-processor's `tool_change.mode` `"m6"` and a definition whose tool
//!   change has `M6` (grblHAL) does one program change tools; otherwise
//!   [`emit`] refuses a second tool. The change retracts to safe Z and
//!   stops the spindle, then
//!   writes DreamCarve's own `T<n> M6 (MSG, Fit T<n> <name>. Set Z zero,
//!   then resume.)` in place of the definition's `M6` line, leaving out
//!   lines that only pause and `pause_after_change`: one stop per change.
//!   The user may jog or set Z zero there, so the position is then unknown
//!   and the program retracts to safe Z again before the spindle starts or
//!   the router-on pause (#43). A program that changes tools first sets the
//!   controller's current tool with `M61 Q<n>`, because grblHAL skips an
//!   `M6` for the tool it holds.
//! - Before the first move the program sets absolute distance mode (`G90`)
//!   as well as units and work coordinates, adding any the header leaves
//!   out. After every pause that more motion follows, it sets them again,
//!   with the header's other lines that only set modal state (`G17`,
//!   `G94`): the user may change any of them at the controller (#55).
//! - A rapid lowers Z only after its XY move.
//! - I and J are taken from the rounded start the controller holds; an arc
//!   the controller would reject, or read as a full circle, is flattened.
//! - The emitted text is parsed and checked by `safety` before return.

use std::f64::consts::{FRAC_PI_2, TAU};

use dc_cam::{FeedField, Move, MoveKind, Toolpath};
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3};
use dc_project::{
    ArcFormat, OperationId, PostProcessor, ProfileOp, Project, SpindleControl, Tool,
    ToolChangeMode, ToolId, Units, is_m6_line,
};

use crate::ExportError;
use crate::arcfit::{self, EmitOptions, Fitted, SubArc};
use crate::format::{decimal_places, format_fixed, format_number, read_back};
use crate::parse::parse_program;
use crate::safety::{self, Extents, OpSpan, Tolerances};
use crate::transform::MachineTransform;

/// An emitted G-code program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The program text, one block per line, `\n` line endings, ending in
    /// a newline.
    pub text: String,
    /// File extension without the dot, from the post-processor.
    pub extension: String,
    /// Every `M0` and `M6` in the program, in order. The controller may not
    /// show a pause's message (GRBL drops comments), so a caller lists these
    /// for the user.
    pub pauses: Vec<Pause>,
    /// The tool the program starts with. It does not stop for it: the user
    /// fits it and sets work zero with it before starting (#57).
    pub first_tool: FirstTool,
    /// Where the program takes the tool centre, in machine coordinates
    /// and the program's units, read back from [`Program::text`]. Users
    /// place clamps from it (#62).
    pub extents: Extents,
}

/// The tool a program starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstTool {
    /// Its number, `T<n>`: its 1-based place in the project's tool list.
    pub number: usize,
    /// Its name, as written in the program's comments.
    pub name: String,
}

/// A point where the program stops until the user resumes it: an `M0`, or
/// an `M6` tool change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pause {
    /// 1-based line number in [`Program::text`] (not an `N` number).
    pub line: usize,
    /// What the user should do, from the line's comment without a leading
    /// `MSG,`; empty if the line has no comment.
    pub message: String,
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
    /// Some runs of straight moves that fit an arc were written as the
    /// original moves because, once its numbers were rounded, the
    /// controller would have rejected one of the run's arcs
    /// (`arc_fit_fallback`).
    ArcFitFallback {
        /// The operation.
        op: OperationId,
        /// How many runs.
        runs: usize,
    },
}

/// Fewest decimal places accepted in millimetres. The safety pass tolerates
/// one unit in the last place, so this bounds that tolerance at 0.01 mm.
pub const MIN_DECIMAL_PLACES_MM: u8 = 2;
/// Fewest decimal places accepted in inches: one unit is 0.0254 mm.
pub const MIN_DECIMAL_PLACES_INCH: u8 = 3;

/// One operation to emit: the operation, its tool, and its toolpath.
type OpEntry = (ProfileOp, Tool, Toolpath);

/// Emits the programs `export` writes for the enabled operations in `ops`,
/// as [`emit`] does, split as the post-processor's `tool_change_mode` says
/// (#95):
///
/// - `"separate_programs"`: one program per tool run, a run being
///   consecutive operations with the same tool, in operation order.
///   Operations are never regrouped by tool, which would change the cut
///   order. Each program is exactly what [`emit`] writes for its run alone:
///   a one-tool program with no `M6` and no `M61`, checked by the safety
///   pass on its own, with its own extents. A one-tool project is one
///   program, the same as [`emit`]'s.
/// - `"m6"`: the one program [`emit`] writes, with `T<n> M6` at each tool
///   change.
///
/// Warnings are those of every program, disabled operations once.
///
/// # Errors
///
/// As for [`emit`]; the first program that fails fails the whole call, so
/// a caller never has some of the programs.
pub fn emit_programs(
    project: &Project,
    ops: &[OpEntry],
    date: &str,
) -> Result<(Vec<Program>, Vec<ExportWarning>), ExportError> {
    let post = project.post_processor.resolve()?;
    let (enabled, mut warnings) = enabled_ops(ops)?;
    let runs: Vec<&[&OpEntry]> = match post.tool_change.mode {
        ToolChangeMode::M6 => vec![enabled.as_slice()],
        ToolChangeMode::SeparatePrograms => enabled.chunk_by(|a, b| a.1.id == b.1.id).collect(),
    };
    let mut programs = Vec::with_capacity(runs.len());
    for run in runs {
        let (program, run_warnings) = emit_enabled(project, &post, run, date)?;
        warnings.extend(run_warnings);
        programs.push(program);
    }
    Ok((programs, warnings))
}

/// Emits one program for the enabled operations in `ops`, each with its
/// tool and its toolpath (project space, mm), using the project's
/// post-processor (its bundled definition with its settings, resolved by
/// [`dc_project::PostProcessorRef::resolve`]), machine origin, and display
/// units. `date` fills the
/// `{date}` placeholder; the caller supplies it so output is
/// deterministic. `export` calls [`emit_programs`], which splits a
/// multi-tool project as the post-processor's tool-change mode says.
///
/// The emitted text is parsed back and checked against safety rules S1–S7
/// before it is returned.
///
/// # Errors
///
/// A missing feed, plunge rate, or (with automatic spindle control)
/// spindle speed names the tool and field; more than one tool, unless the
/// tool-change mode is `"m6"` and the definition has `M6`, names the tools
/// ([`ExportError::ToolChangeNotSupported`]); any safety violation in the
/// emitted program is an error, never a warning. A post-processor that
/// names no bundled definition is [`ExportError::PostProcessor`]. See
/// [`ExportError`].
pub fn emit(
    project: &Project,
    ops: &[OpEntry],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    emit_with(project, &project.post_processor.resolve()?, ops, date)
}

/// [`emit`] with the effective post-processor `post` given. Tests use it
/// to try definitions that no bundled version has.
pub(crate) fn emit_with(
    project: &Project,
    post: &PostProcessor,
    ops: &[OpEntry],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    let (enabled, mut warnings) = enabled_ops(ops)?;
    let (program, emitted) = emit_enabled(project, post, &enabled, date)?;
    warnings.extend(emitted);
    Ok((program, warnings))
}

/// The enabled entries of `ops`, and a warning for each disabled one.
/// Fails when none is enabled.
fn enabled_ops(ops: &[OpEntry]) -> Result<(Vec<&OpEntry>, Vec<ExportWarning>), ExportError> {
    let mut warnings = Vec::new();
    let mut enabled = Vec::with_capacity(ops.len());
    for entry in ops {
        if entry.0.enabled {
            enabled.push(entry);
        } else {
            warnings.push(ExportWarning::DisabledOperationSkipped {
                op: entry.0.id.clone(),
            });
        }
    }
    if enabled.is_empty() {
        return Err(ExportError::NoEnabledOperations);
    }
    Ok((enabled, warnings))
}

/// [`emit`] for operations already known to be enabled.
fn emit_enabled(
    project: &Project,
    post: &PostProcessor,
    enabled: &[&OpEntry],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    let control = post.feed_speed.spindle_control;
    let manual = control == SpindleControl::Manual;
    let mut warnings = Vec::new();

    // Step 1: preconditions, all checked before a line is written. The
    // post-processor is the last line and trusts no caller.
    let mut planned = Vec::with_capacity(enabled.len());
    for &(op, tool, toolpath) in enabled {
        let rates = Rates::of(tool, control)?;
        let number = project
            .tools
            .iter()
            .position(|t| t.id == tool.id)
            .map(|i| i + 1)
            .ok_or_else(|| ExportError::UnknownTool {
                op: op.id.clone(),
                tool: tool.id.clone(),
            })?;
        planned.push((op, tool, toolpath, rates, number));
    }
    // #57: the program stops for every tool after the first, and only `M6`
    // stops where the user may jog to set Z zero. #95: and only when the
    // user chose one program with `M6`; otherwise each tool run is a
    // program of its own (`emit_programs`).
    let changes_tools = planned
        .iter()
        .zip(planned.iter().skip(1))
        .any(|(a, b)| a.1.id != b.1.id);
    if changes_tools && !changes_tools_in_program(post) {
        let mut tools: Vec<String> = Vec::new();
        for &(_, tool, ..) in &planned {
            if !tools.contains(&tool.name) {
                tools.push(tool.name.clone());
            }
        }
        return Err(ExportError::ToolChangeNotSupported {
            post: post.id.clone(),
            tools,
        });
    }
    let first_tool = planned
        .first()
        .map(|&(_, tool, _, _, number)| FirstTool {
            number,
            name: comment_text(&tool.name),
        })
        .ok_or(ExportError::NoEnabledOperations)?;
    // Safe Z is measured from the stock bottom, so it needs the thickness
    // whatever the Z origin (#64).
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(ExportError::MissingThickness)?;
    // Steps 2 and 3: machine transform and units.
    let transform = MachineTransform::new(project, post)?;
    let (units_command, units_field) = match transform.output_units() {
        Units::Mm => (post.units.mm_command.as_deref(), "units.mm_command"),
        Units::Inch => (post.units.inch_command.as_deref(), "units.inch_command"),
    };
    let units_command = required(units_command, &post.id, units_field)?;
    let wcs_command = required(
        post.work_coordinates.select_command.as_deref(),
        &post.id,
        "work_coordinates.select_command",
    )?;
    let (units_name, min_places) = match transform.output_units() {
        Units::Mm => ("mm", MIN_DECIMAL_PLACES_MM),
        Units::Inch => ("inch", MIN_DECIMAL_PLACES_INCH),
    };
    let places = decimal_places(&post.number_format, transform.output_units());
    if places < min_places {
        return Err(ExportError::TooFewDecimalPlaces {
            post: post.id.clone(),
            units: units_name,
            places,
            min: min_places,
        });
    }
    let dwell_s = post.feed_speed.spindle_on_dwell.duration_s;
    if !(dwell_s.is_finite() && dwell_s >= 0.0) {
        return Err(ExportError::InvalidDwell {
            post: post.id.clone(),
            duration_s: dwell_s,
        });
    }

    let mut w = Writer::new(project, post, transform);

    // Step 4: header, then G90, units and WCS if the header does not place
    // them. The same setup is written again after every pause (#55).
    let common = vec![
        ("programName", comment_text(&project.name)),
        ("date", comment_text(date)),
        ("unitsCommand", units_command.to_owned()),
        ("wcsCommand", wcs_command.to_owned()),
    ];
    for line in &post.program.header {
        w.template(line, &common);
    }
    let mut setup: Vec<String> = post
        .program
        .header
        .iter()
        .flat_map(|line| {
            substitute(line, &common)
                .lines()
                .map(str::trim)
                .filter(|l| only_sets_modal_state(l))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    if !setup
        .iter()
        .any(|l| code_words(l).iter().any(|w| w == "G90"))
    {
        w.line(ABSOLUTE_COMMAND);
        setup.push(ABSOLUTE_COMMAND.to_owned());
    }
    let header_has = |token: &str| post.program.header.iter().any(|l| l.contains(token));
    if !header_has("{unitsCommand}") {
        w.line(units_command);
        setup.push(units_command.to_owned());
    }
    if !header_has("{wcsCommand}") {
        w.line(wcs_command);
        setup.push(wcs_command.to_owned());
    }
    w.setup = setup;
    if changes_tools {
        // #57: grblHAL skips an `M6` for the tool it holds, and that number
        // survives from the previous job. Tell it which tool the user
        // fitted, so no later `M6` is skipped. `M61` sets the number only.
        w.line(&format!("{SET_TOOL_COMMAND} Q{}", first_tool.number));
    }

    // Step 5: operations.
    let mut lines = Vec::with_capacity(planned.len());
    let mut router_on = false;
    for (index, &(op, tool, toolpath, rates, number)) in planned.iter().enumerate() {
        let mut context = common.clone();
        context.extend([
            ("operationIndex", (index + 1).to_string()),
            ("operationName", comment_text(&op.name)),
            ("toolNumber", number.to_string()),
            ("toolName", comment_text(&tool.name)),
        ]);
        if post.program.operation_header.is_empty() {
            let comment = format!(
                "{} Operation {}: {} {}",
                post.program.comment_prefix,
                index + 1,
                comment_text(&op.name),
                post.program.comment_suffix
            );
            w.line(comment.trim_end());
        } else {
            for line in &post.program.operation_header {
                w.template(line, &context);
            }
        }

        let spindle_off = post.feed_speed.spindle_off.trim();
        let safe_z_mm = thickness_mm + op.safe_z_mm;
        // The tool change belongs to the operation, so the safety pass
        // checks its stop against this operation's safe Z.
        let first_line = w.next_line();
        if w.tool.is_none() {
            // S4, #57: the user fitted the first tool and set work zero
            // with it before starting, so there is no stop. Z is wherever
            // the user left it, so the program lifts to safe Z first.
            w.forget_position();
            w.retract(safe_z_mm)?;
            w.tool = Some(tool.id.clone());
        } else if w.tool.as_ref() != Some(&tool.id) {
            // S4, #57: the user fits the tool with the bit clear of the
            // work. After a router-off pause the user may have jogged, so
            // the retract is always written.
            w.forget_position();
            w.retract(safe_z_mm)?;
            if post.tool_change.stop_spindle_first && w.spindle_rpm.is_some() {
                w.line(spindle_off);
                w.spindle_rpm = None;
            }
            // The definition's `M6` line becomes DreamCarve's own, with the
            // tool number and the message. Lines that only pause are left
            // out (one stop per change), and so are lines that only stop a
            // spindle that is already off, or that the user switches (S4).
            let fit = tool_change_message(number, &tool.name);
            let mut changed = false;
            for line in &post.tool_change.commands {
                if is_m6_line(line) {
                    if !changed {
                        w.change_tool(number, &fit);
                        changed = true;
                    }
                } else if !(only_pauses(line)
                    || (w.spindle_rpm.is_none() && only_stops_spindle(line, spindle_off)))
                {
                    w.template(line, &context);
                }
            }
            w.tool = Some(tool.id.clone());
            // The user may have jogged and set Z zero, so where the tool is
            // is no longer known. Lift to safe Z, relative to the new zero,
            // before the spindle starts.
            w.forget_position();
            w.retract(safe_z_mm)?;
        }

        if manual {
            // S4 (manual): retract, then pause for the user to switch the
            // router on. Resuming is the confirmation, so there is no
            // dwell, and the speed is only a hint. The user may jog or set
            // Z zero at the pause, so Z is unknown after it: lift to safe
            // Z again before moving in XY (#68).
            if !router_on {
                w.retract(safe_z_mm)?;
                w.pause(&router_on_message(rates.rpm), true);
                w.forget_position();
                w.retract(safe_z_mm)?;
                router_on = true;
            }
        } else if let Some(rpm) = rates.rpm
            && w.spindle_rpm != Some(rpm)
        {
            // S4: start the spindle clockwise at the tool's speed, then
            // dwell while it spins up. Never M4.
            let fs = &post.feed_speed;
            w.line(&format!("{} {}{rpm}", fs.spindle_on_cw, fs.rpm_command));
            w.spindle_rpm = Some(rpm);
            if dwell_s > 0.0 {
                let seconds = format_fixed(dwell_s, DWELL_DECIMAL_PLACES, &post.number_format)?;
                w.line(&fs.spindle_on_dwell.command.replace("{seconds}", &seconds));
            }
        }

        let written = w.toolpath(op, toolpath)?;
        if written.flattened > 0 {
            warnings.push(ExportWarning::ArcsFlattened {
                op: op.id.clone(),
                count: written.flattened,
            });
        }
        if written.fallback_runs > 0 {
            warnings.push(ExportWarning::ArcFitFallback {
                op: op.id.clone(),
                runs: written.fallback_runs,
            });
        }

        // Stop the spindle before a tool change, and after the last
        // operation unless the footer does it (S4). With manual control,
        // retract and pause for the user to switch the router off.
        let tool_changes_next = planned.get(index + 1).map(|next| next.1.id != tool.id);
        if manual {
            if tool_changes_next.unwrap_or(true) {
                w.retract(safe_z_mm)?;
                // After the last tool the program only ends.
                w.pause(ROUTER_OFF_MESSAGE, index + 1 < planned.len());
                router_on = false;
            }
        } else {
            let footer_stops = post.program.footer.iter().any(|l| l.trim() == spindle_off);
            if tool_changes_next.unwrap_or(!footer_stops) {
                w.line(spindle_off);
                w.spindle_rpm = None;
            }
        }
        lines.push(first_line..w.next_line());
    }

    // Step 6: footer and program end.
    let spindle_off = post.feed_speed.spindle_off.trim();
    for line in &post.program.footer {
        if !(manual && only_stops_spindle(line, spindle_off)) {
            w.template(line, &common);
        }
    }
    w.line(&post.stop.program_end_command);
    let tolerances = Tolerances {
        flatten_mm: w.flatten_tolerance_mm,
        arc_fit_mm: w.arc_fit_tolerance_mm,
    };
    let text = w.finish();

    // Step 7: the safety pass, on the text.
    let spans: Vec<OpSpan<'_>> = planned
        .iter()
        .zip(lines)
        .map(|(&(op, tool, toolpath, _, _), lines)| OpSpan {
            op,
            tool,
            toolpath,
            lines,
        })
        .collect();
    let extents = safety::check(
        &text,
        post,
        &transform,
        &project.stock,
        thickness_mm,
        tolerances,
        &spans,
    )?;
    let pauses = pauses(&text)?;

    Ok((
        Program {
            text,
            extension: post.file_extension.clone(),
            pauses,
            first_tool,
            extents,
        },
        warnings,
    ))
}

/// The longest text one placeholder puts into a line, in characters. With
/// the bundled templates every line stays well under
/// [`crate::MAX_LINE_CHARS`]; the safety pass rejects any that does not.
pub const MAX_PLACEHOLDER_CHARS: usize = 64;

/// The longest pause message, in characters. The longest DreamCarve writes
/// is the router-on message with a ten-digit speed, 67 characters.
pub const MAX_MESSAGE_CHARS: usize = 96;

/// Dwell seconds are written to the millisecond.
const DWELL_DECIMAL_PLACES: u8 = 3;

#[cfg(test)]
thread_local! {
    /// Test-only mutation: when set, every arc is written the other way
    /// round, so a test can show the safety pass rejects it (#60).
    pub(crate) static FLIP_ARCS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Most straight moves one flattened arc becomes.
const MAX_ARC_SEGMENTS: f64 = 100_000.0;

/// Arcs are written with at most this sweep in R format, where a
/// half-circle leaves the controller unable to place the centre.
const MAX_R_ARC_SWEEP_RAD: f64 = FRAC_PI_2;

/// An arc whose sweep or radius is below these is drawn as a straight move.
const MIN_ARC_SWEEP_RAD: f64 = 1e-9;

/// GRBL and grblHAL (`gcode.c`, arc radius check) accept an I/J arc when
/// its start and end radii differ by at most 0.005 mm, or by at most
/// 0.1 % of the radius and 0.5 mm. The controllers compute in 32-bit
/// floats, so DreamCarve keeps a fifth of each budget in reserve.
const ARC_RADIUS_ABSOLUTE_MM: f64 = 0.004;
/// See [`ARC_RADIUS_ABSOLUTE_MM`].
const ARC_RADIUS_RELATIVE: f64 = 0.0008;
/// See [`ARC_RADIUS_ABSOLUTE_MM`].
const ARC_RADIUS_GROSS_MM: f64 = 0.4;

/// A tool whose feed and plunge rate the user has set, and its spindle
/// speed: always set with automatic spindle control, optional with manual.
/// The moves carry the feeds; the safety pass checks them against the
/// tool's.
#[derive(Debug, Clone, Copy)]
struct Rates {
    rpm: Option<u32>,
}

impl Rates {
    /// The tool's rates, or the first one that is missing. A placeholder 0
    /// counts as missing.
    fn of(tool: &Tool, control: SpindleControl) -> Result<Self, ExportError> {
        for (rate, field) in [
            (tool.feed_mm_min, FeedField::Feed),
            (tool.plunge_mm_min, FeedField::Plunge),
        ] {
            if !rate.is_some_and(|r| r.is_finite() && r > 0.0) {
                return Err(ExportError::MissingFeed {
                    tool: tool.name.clone(),
                    field,
                });
            }
        }
        let rpm = tool.rpm.filter(|r| *r > 0);
        if rpm.is_none() && control == SpindleControl::Automatic {
            return Err(ExportError::MissingRpm {
                tool: tool.name.clone(),
            });
        }
        Ok(Self { rpm })
    }
}

/// The pause DreamCarve writes for the router. Always `M0`, never the
/// definition's `pause_command`: `M1` is skipped unless the controller's
/// optional-stop switch is on, and then the bit would plunge without
/// spinning. GRBL and grblHAL sync the planner on `M0`, enter feed hold,
/// and resume only on cycle start (`gcode.c`, "[21. Program flow]").
const PAUSE_COMMAND: &str = "M0";

/// grblHAL's tool change (`dc-project/src/definitions/README.md`, "Tool changes"). With
/// `$341` at 0 and no automatic tool changer it syncs the planner, reports
/// the line's `(MSG, ...)`, and waits in its tool-change state, where the
/// user may jog, until cycle start.
const TOOL_CHANGE_COMMAND: &str = "M6";

/// grblHAL's "set current tool number" (`M61 Q<n>`): no stop, no motion.
const SET_TOOL_COMMAND: &str = "M61";

/// Whether a program for `post` may change tools: the user chose one
/// program with `M6` (`tool_change.mode` `"m6"`, #95) and its tool change
/// has an `M6` line, which GRBL 1.1 lacks (#57).
pub(crate) fn changes_tools_in_program(post: &PostProcessor) -> bool {
    post.tool_change.mode == ToolChangeMode::M6 && post.tool_change.has_m6()
}

/// The message of the pause before a tool's last cut is followed by a
/// tool change or the end of the program.
const ROUTER_OFF_MESSAGE: &str = "Turn the router OFF.";

/// The message of the pause before a tool's first cut, with the tool's
/// speed as a hint when it has one.
fn router_on_message(rpm: Option<u32>) -> String {
    match rpm {
        Some(rpm) => format!("Turn the router ON. Speed {rpm} RPM. Resume to start cutting."),
        None => "Turn the router ON. Resume to start cutting.".to_owned(),
    }
}

/// What the user does at a tool change, after the tool's number and
/// name (#43): a new tool has a new length, so Z zero is set with it.
const SET_Z_ZERO: &str = "Set Z zero, then resume.";

/// The message of the tool change for tool `number` named `name`.
/// The name is shortened so the instruction always fits
/// [`MAX_MESSAGE_CHARS`] whole.
fn tool_change_message(number: usize, name: &str) -> String {
    let head = format!("Fit T{number}");
    let room = MAX_MESSAGE_CHARS.saturating_sub(head.len() + SET_Z_ZERO.len() + 3);
    let name = sanitize_comment(name, room);
    if name.is_empty() {
        format!("{head}. {SET_Z_ZERO}")
    } else {
        format!("{head} {name}. {SET_Z_ZERO}")
    }
}

/// Absolute distance mode. S5 requires it before the first move and after
/// every pause.
const ABSOLUTE_COMMAND: &str = "G90";

/// The codes a header line may hold and still be written again after a
/// pause: they set modal state and nothing else (S5).
const MODAL_SETUP_CODES: [&str; 11] = [
    "G17", "G20", "G21", "G54", "G55", "G56", "G57", "G58", "G59", "G90", "G94",
];

/// Whether `line` sets modal state and does nothing else: it has code
/// words, and each is in [`MODAL_SETUP_CODES`].
fn only_sets_modal_state(line: &str) -> bool {
    let words = code_words(line);
    !words.is_empty()
        && words
            .iter()
            .all(|w| MODAL_SETUP_CODES.contains(&w.as_str()))
}

/// Whether template `line` does nothing but pause: its only code word is
/// `M0` or `M1` (in any spelling the parser accepts).
fn only_pauses(line: &str) -> bool {
    match code_words(line).as_slice() {
        [word] => matches!(word.as_str(), "M0" | "M00" | "M1" | "M01"),
        _ => false,
    }
}

/// The code words of `line`, outside `( )` and `;` comments, upper-cased.
fn code_words(line: &str) -> Vec<String> {
    let mut code = String::new();
    let mut in_comment = false;
    for c in line.chars() {
        match c {
            ';' if !in_comment => break,
            '(' => in_comment = true,
            ')' => in_comment = false,
            c if !in_comment => code.push(c),
            _ => {}
        }
    }
    code.split_whitespace().map(str::to_uppercase).collect()
}

/// Whether template `line` does nothing but stop the spindle: its code
/// words are exactly `spindle_off`.
fn only_stops_spindle(line: &str, spindle_off: &str) -> bool {
    let off = code_words(spindle_off);
    !off.is_empty() && code_words(line) == off
}

/// The text of `line`'s comment, trimmed and without a leading `MSG,`; empty
/// if it has none.
fn comment_of(line: &str) -> String {
    let paren = line
        .split_once('(')
        .map(|(_, rest)| rest.split_once(')').map_or(rest, |(inner, _)| inner));
    let text = paren
        .or_else(|| line.split_once(';').map(|(_, rest)| rest))
        .unwrap_or("")
        .trim();
    let text = match text.get(..4) {
        Some(head) if head.eq_ignore_ascii_case("MSG,") => text.get(4..).unwrap_or(""),
        _ => text,
    };
    text.trim().to_owned()
}

/// Every `M0` and `M6` line of `text`, with its message.
fn pauses(text: &str) -> Result<Vec<Pause>, ExportError> {
    let blocks = parse_program(text)?;
    let mut raw = text.lines().enumerate();
    let mut out = Vec::new();
    for block in blocks.iter().filter(|b| b.has('M', 0.0) || b.has('M', 6.0)) {
        let message = raw
            .find(|(index, _)| index + 1 == block.line)
            .map(|(_, line)| comment_of(line))
            .unwrap_or_default();
        out.push(Pause {
            line: block.line,
            message,
        });
    }
    Ok(out)
}

/// A non-empty command from the post-processor, or the error naming it.
fn required<'a>(
    command: Option<&'a str>,
    post: &str,
    field: &'static str,
) -> Result<&'a str, ExportError> {
    command
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| ExportError::MissingCommand {
            post: post.to_owned(),
            field,
        })
}

/// [`sanitize_comment`] capped at [`MAX_PLACEHOLDER_CHARS`], for text
/// substituted into a template.
fn comment_text(text: &str) -> String {
    sanitize_comment(text, MAX_PLACEHOLDER_CHARS)
}

/// `text` made safe for a comment on GRBL and grblHAL: printable ASCII,
/// without `( ) ;` (which end a comment early), `! ? ~` (real-time
/// commands even inside a comment), `{ }` (placeholder syntax) or `%`
/// (a program delimiter to some senders), whitespace collapsed, and at
/// most `max_chars` long.
fn sanitize_comment(text: &str, max_chars: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            let printable = (' '..='~').contains(&c);
            let special = matches!(c, '(' | ')' | ';' | '!' | '?' | '~' | '{' | '}' | '%');
            if printable && !special { c } else { ' ' }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(max_chars).collect();
    truncated.trim_end().to_owned()
}

/// `template` with each `{name}` in `context` replaced, in one pass, so
/// substituted text is never read as a placeholder. Unknown names are
/// left as written.
fn substitute(template: &str, context: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('{') {
        out.push_str(before);
        if let Some((name, tail)) = after.split_once('}')
            && let Some((_, value)) = context.iter().find(|(key, _)| *key == name)
        {
            out.push_str(value);
            rest = tail;
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// A word value as written and as the controller reads it back.
#[derive(Debug, Clone)]
struct Emitted {
    text: String,
    value: f64,
}

/// An arc in project space, mm, with a positive sweep.
#[derive(Debug, Clone, Copy)]
struct ArcGeometry {
    start: Point3,
    end: Point3,
    centre: Point,
    radius_mm: f64,
    start_angle_rad: f64,
    sweep_rad: f64,
    clockwise: bool,
}

impl ArcGeometry {
    /// The arc from `start` to `end` about `centre`, or `None` if it is
    /// too small to be an arc.
    fn new(start: Point3, end: Point3, centre: Point, clockwise: bool) -> Option<Self> {
        let radius_mm = start.xy().distance_to(centre);
        let start_angle_rad = (start.y - centre.y).atan2(start.x - centre.x);
        let end_angle_rad = (end.y - centre.y).atan2(end.x - centre.x);
        let turn = if clockwise {
            start_angle_rad - end_angle_rad
        } else {
            end_angle_rad - start_angle_rad
        };
        let sweep_rad = turn.rem_euclid(TAU);
        (radius_mm > MIN_ARC_SWEEP_RAD && sweep_rad > MIN_ARC_SWEEP_RAD).then_some(Self {
            start,
            end,
            centre,
            radius_mm,
            start_angle_rad,
            sweep_rad,
            clockwise,
        })
    }

    /// The point `fraction` of the way along the arc; Z changes linearly.
    fn point_at(&self, fraction: f64) -> Point3 {
        if fraction >= 1.0 {
            return self.end;
        }
        let signed = if self.clockwise { -1.0 } else { 1.0 };
        let angle = self.start_angle_rad + signed * fraction * self.sweep_rad;
        Point3::new(
            self.centre.x + self.radius_mm * angle.cos(),
            self.centre.y + self.radius_mm * angle.sin(),
            self.start.z + fraction * (self.end.z - self.start.z),
        )
    }

    /// Part `k` (0-based) of the arc split into `n` equal parts.
    fn part(&self, k: usize, n: usize) -> Self {
        #[allow(
            clippy::cast_precision_loss,
            reason = "k < n <= MAX_ARC_SEGMENTS, exact in f64"
        )]
        let (a, b) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
        Self {
            start: self.point_at(a),
            end: self.point_at(b),
            start_angle_rad: self.start_angle_rad
                + if self.clockwise { -1.0 } else { 1.0 } * a * self.sweep_rad,
            sweep_rad: (b - a) * self.sweep_rad,
            ..*self
        }
    }
}

/// `count` rounded up and clamped to `1..=MAX_ARC_SEGMENTS`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 1..=MAX_ARC_SEGMENTS, which usize holds exactly"
)]
fn segment_count(count: f64) -> usize {
    if count.is_nan() {
        return 1;
    }
    count.ceil().clamp(1.0, MAX_ARC_SEGMENTS) as usize
}

/// What writing a toolpath had to change for the controller's sake.
#[derive(Debug, Clone, Copy, Default)]
struct Written {
    /// Arcs written as straight moves.
    flattened: usize,
    /// Fitted runs written as their original moves.
    fallback_runs: usize,
}

/// Writes lines and tracks the controller's modal state and position.
struct Writer<'a> {
    post: &'a PostProcessor,
    transform: MachineTransform,
    flatten_tolerance_mm: f64,
    /// The project's arc-fit tolerance; `None` turns fitting off.
    arc_fit_tolerance_mm: Option<f64>,
    lines: Vec<String>,
    line_number: u32,
    /// The last motion word written.
    motion: Option<String>,
    /// The last feed word value written.
    feed: Option<String>,
    /// The last X, Y, Z written, as the controller holds them.
    axes: [Option<Emitted>; 3],
    /// Where the tool is, project space, if known.
    position: Option<Point3>,
    spindle_rpm: Option<u32>,
    tool: Option<ToolId>,
    /// The lines that set the controller's modal state, written again after
    /// a pause.
    setup: Vec<String>,
}

impl<'a> Writer<'a> {
    fn new(project: &Project, post: &'a PostProcessor, transform: MachineTransform) -> Self {
        let tolerance = project.flatten_tolerance_mm;
        Self {
            post,
            transform,
            flatten_tolerance_mm: if tolerance.is_finite() && tolerance > 0.0 {
                tolerance
            } else {
                FLATTEN_TOLERANCE_DEFAULT_MM
            },
            arc_fit_tolerance_mm: project
                .arc_fit_tolerance_mm
                .filter(|t| t.is_finite() && *t > 0.0),
            lines: Vec::new(),
            line_number: post.program.line_number_increment,
            motion: None,
            feed: None,
            axes: [None, None, None],
            position: None,
            spindle_rpm: None,
            tool: None,
            setup: Vec::new(),
        }
    }

    /// The 1-based number of the next line to be written.
    fn next_line(&self) -> usize {
        self.lines.len() + 1
    }

    fn line(&mut self, content: &str) {
        if self.post.program.line_numbers {
            self.lines.push(format!("N{} {content}", self.line_number));
            self.line_number = self
                .line_number
                .saturating_add(self.post.program.line_number_increment);
        } else {
            self.lines.push(content.to_owned());
        }
    }

    /// Writes `template` filled from `context`, skipping lines left empty
    /// (a `{wcsCommand}` with nothing to select, for example).
    fn template(&mut self, template: &str, context: &[(&str, String)]) {
        let filled = substitute(template, context);
        for line in filled.lines() {
            let line = line.trim_end();
            if !line.trim().is_empty() {
                self.line(line);
            }
        }
    }

    /// Writes an `M0` with `message`, sanitized, as a `(MSG, ...)` comment.
    /// grblHAL reports it to the sender as `[MSG:...]`; GRBL ignores the
    /// comment (`dc-project/src/definitions/README.md`, "Manual spindle control").
    ///
    /// The user may send commands while the program is paused, such as a
    /// probe (`G38.2`) or a `G1` with its own feed to set Z zero, so the
    /// next move writes its motion and feed words again instead of relying
    /// on the modal ones (#43).
    ///
    /// For the same reason, when the program moves again after the pause
    /// (`moves_after`), the setup is written again: units, work coordinates,
    /// `G90` and the header's other modal lines (#55).
    fn pause(&mut self, message: &str, moves_after: bool) {
        let message = sanitize_comment(message, MAX_MESSAGE_CHARS);
        self.line(&format!("{PAUSE_COMMAND} (MSG, {message})"));
        self.stopped(moves_after);
    }

    /// Writes `T<number> M6` with `message` as a `(MSG, ...)` comment on
    /// the same line, and sets up again after it as after a pause: the user
    /// jogs and sets Z zero at the stop (#57). On its own line the message
    /// would be reported when the controller reads the line, which may be
    /// long before it stops; on the `M6` line grblHAL reports it after the
    /// planner has synced, as it stops (`dc-project/src/definitions/README.md`).
    fn change_tool(&mut self, number: usize, message: &str) {
        let message = sanitize_comment(message, MAX_MESSAGE_CHARS);
        self.line(&format!("T{number} {TOOL_CHANGE_COMMAND} (MSG, {message})"));
        self.stopped(true);
    }

    /// After a stop: the modal state the user may have changed is written
    /// again, and the setup lines too when the program `moves_after`.
    fn stopped(&mut self, moves_after: bool) {
        self.motion = None;
        self.feed = None;
        if moves_after {
            for line in self.setup.clone() {
                self.line(&line);
            }
        }
    }

    /// Raises the tool to `safe_z_mm` (project space) unless it is known to
    /// be there or higher. XY does not change.
    fn retract(&mut self, safe_z_mm: f64) -> Result<(), ExportError> {
        let to = self.position.map_or(Point3::new(0.0, 0.0, safe_z_mm), |p| {
            Point3::new(p.x, p.y, p.z.max(safe_z_mm))
        });
        let z = self.transform.to_machine(to).z;
        let command = self.post.motion.rapid_command.clone();
        self.motion_line(&command, [None, None, Some(z)], &[], None, false)?;
        if let Some(position) = &mut self.position {
            position.z = to.z;
        }
        Ok(())
    }

    fn forget_position(&mut self) {
        self.position = None;
        self.axes = [None, None, None];
    }

    fn finish(self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    fn number(&self, value: f64) -> Result<String, ExportError> {
        format_number(
            value,
            &self.post.number_format,
            self.transform.output_units(),
        )
    }

    /// Writes `toolpath`, with runs of straight moves fitted to arcs when
    /// the project has an arc-fit tolerance and the post-processor writes
    /// arcs (`docs/specs/phase-2/design.md`, "Arc fitting"). Fitting runs
    /// in project space; each fitted run is then judged on its rounded
    /// machine numbers and reverts to its moves if the controller would
    /// reject any of its arcs.
    fn toolpath(&mut self, op: &ProfileOp, toolpath: &Toolpath) -> Result<Written, ExportError> {
        // Every cut carries a feed, checked before fitting so the error
        // names the move whatever the fitting does.
        for (index, mv) in toolpath.moves.iter().enumerate() {
            if !mv.is_rapid() && !mv.feed_mm_min.is_some_and(|f| f.is_finite() && f > 0.0) {
                return Err(ExportError::MoveWithoutFeed {
                    op: op.id.clone(),
                    index,
                });
            }
        }
        let fitting = self
            .arc_fit_tolerance_mm
            .filter(|_| self.post.motion.arc_interpolation);
        let (fitted, fallback_runs) = match fitting {
            Some(tolerance_mm) => {
                let fitted = arcfit::fit(&toolpath.moves, tolerance_mm);
                let format = self.post.number_format;
                let units = self.transform.output_units();
                let read = move |v: f64| read_back(&format_number(v, &format, units).ok()?);
                let opts = EmitOptions {
                    format: &read,
                    arc_format: self.post.motion.arc_format,
                    mm_per_unit: self.transform.mm_per_unit(),
                    quantum: self.quantum(),
                };
                let transform = self.transform;
                let to_machine =
                    move |p: Point| transform.to_machine(Point3::new(p.x, p.y, 0.0)).xy();
                let start = self.emitted_xy().map(|(x, y)| Point::new(x, y));
                arcfit::apply_fallback(fitted, &opts, start, &to_machine)
            }
            None => (
                toolpath.moves.iter().copied().map(Fitted::Move).collect(),
                0,
            ),
        };

        let mut written = Written {
            flattened: 0,
            fallback_runs,
        };
        let mut index = 0;
        for item in fitted {
            match item {
                Fitted::Move(mv) => {
                    written.flattened += self.write_move(op, index, &mv)?;
                    index += 1;
                }
                Fitted::Arc(run) => {
                    for arc in &run.arcs {
                        self.fitted_arc(op, index, arc, run.feed_mm_min)?;
                    }
                    if let Some(last) = run.arcs.last() {
                        self.position = Some(last.end);
                    }
                    index += run.fallback.len();
                }
            }
        }
        Ok(written)
    }

    /// Writes toolpath move `index` as it is; returns how many arcs had to
    /// be flattened (0 or 1).
    fn write_move(
        &mut self,
        op: &ProfileOp,
        index: usize,
        mv: &Move,
    ) -> Result<usize, ExportError> {
        if mv.is_rapid() {
            self.rapid(mv.to)?;
            return Ok(0);
        }
        let feed = mv
            .feed_mm_min
            .filter(|f| f.is_finite() && *f > 0.0)
            .ok_or_else(|| ExportError::MoveWithoutFeed {
                op: op.id.clone(),
                index,
            })?;
        let (centre, clockwise) = match mv.kind {
            MoveKind::Rapid | MoveKind::Linear => {
                self.linear(mv.to, feed)?;
                return Ok(0);
            }
            MoveKind::ArcCw { centre } => (centre, true),
            MoveKind::ArcCcw { centre } => (centre, false),
        };
        let start = self.position.ok_or_else(|| ExportError::ArcWithoutStart {
            op: op.id.clone(),
            index,
        })?;
        self.arc(start, mv.to, centre, clockwise, feed)
    }

    /// Writes fitted sub-arc `arc` with the words [`arcfit::resolve_emitted_arc`]
    /// chose for it from the position the controller holds: I and J
    /// snapped to agree, or R repaired for rounding. `apply_fallback` has
    /// already accepted the whole run from that same position.
    fn fitted_arc(
        &mut self,
        op: &ProfileOp,
        index: usize,
        arc: &SubArc,
        feed_mm_min: f64,
    ) -> Result<(), ExportError> {
        let (sx, sy) = self
            .emitted_xy()
            .ok_or_else(|| ExportError::ArcWithoutStart {
                op: op.id.clone(),
                index,
            })?;
        let end = self.transform.to_machine(arc.end);
        let centre = self
            .transform
            .to_machine(Point3::new(arc.centre.x, arc.centre.y, arc.end.z))
            .xy();
        let resolved = {
            let format = self.post.number_format;
            let units = self.transform.output_units();
            let read = move |v: f64| read_back(&format_number(v, &format, units).ok()?);
            let opts = EmitOptions {
                format: &read,
                arc_format: self.post.motion.arc_format,
                mm_per_unit: self.transform.mm_per_unit(),
                quantum: self.quantum(),
            };
            arcfit::resolve_emitted_arc(end.xy(), centre, Point::new(sx, sy), &opts)
        };
        let extra = match self.post.motion.arc_format {
            ArcFormat::Ij => vec![
                format!("I{}", self.number(resolved.i)?),
                format!("J{}", self.number(resolved.j)?),
            ],
            ArcFormat::R => vec![format!("R{}", self.number(resolved.radius)?)],
        };
        let command = self.arc_command_for(arc.clockwise);
        self.motion_line(
            &command,
            [Some(end.x), Some(end.y), Some(end.z)],
            &extra,
            Some(feed_mm_min),
            true,
        )
    }

    /// A rapid to `to`. Raises Z before moving in XY, and moves in XY
    /// before lowering Z, so a rapid never travels sideways lower than
    /// either end. With the position unknown, Z goes first.
    fn rapid(&mut self, to: Point3) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let rising = self.position.is_none_or(|p| to.z >= p.z);
        let command = self.post.motion.rapid_command.clone();
        let z_only = [None, None, Some(m.z)];
        let xy_only = [Some(m.x), Some(m.y), None];
        let (first, second) = if rising {
            (z_only, xy_only)
        } else {
            (xy_only, z_only)
        };
        self.motion_line(&command, first, &[], None, false)?;
        self.motion_line(&command, second, &[], None, false)?;
        self.position = Some(to);
        Ok(())
    }

    fn linear(&mut self, to: Point3, feed_mm_min: f64) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let command = self.post.motion.linear_command.clone();
        self.motion_line(
            &command,
            [Some(m.x), Some(m.y), Some(m.z)],
            &[],
            Some(feed_mm_min),
            false,
        )?;
        self.position = Some(to);
        Ok(())
    }

    /// An arc from `start` to `to` about `centre`: as G2/G3 if the
    /// post-processor allows and the controller would accept the rounded
    /// numbers, otherwise flattened. Returns how many arcs were flattened
    /// for the controller's sake.
    fn arc(
        &mut self,
        start: Point3,
        to: Point3,
        centre: Point,
        clockwise: bool,
        feed_mm_min: f64,
    ) -> Result<usize, ExportError> {
        let Some(arc) = ArcGeometry::new(start, to, centre, clockwise) else {
            self.linear(to, feed_mm_min)?;
            return Ok(0);
        };
        if !self.post.motion.arc_interpolation {
            self.flatten(&arc, feed_mm_min)?;
            return Ok(0);
        }
        let mut flattened = 0;
        match self.post.motion.arc_format {
            ArcFormat::Ij => {
                if !self.arc_ij(&arc, feed_mm_min)? {
                    self.flatten(&arc, feed_mm_min)?;
                    flattened += 1;
                }
            }
            ArcFormat::R => {
                let parts = segment_count(arc.sweep_rad / MAX_R_ARC_SWEEP_RAD);
                for k in 0..parts {
                    let part = arc.part(k, parts);
                    if !self.arc_r(&part, feed_mm_min)? {
                        self.flatten(&part, feed_mm_min)?;
                        flattened += 1;
                    }
                }
            }
        }
        self.position = Some(to);
        Ok(flattened)
    }

    /// The arc command for `arc` in machine space: a mirroring axis map
    /// turns clockwise into counter-clockwise.
    fn arc_command(&self, arc: &ArcGeometry) -> String {
        self.arc_command_for(arc.clockwise)
    }

    /// The arc command for an arc that is `clockwise` in project space.
    fn arc_command_for(&self, clockwise: bool) -> String {
        let motion = &self.post.motion;
        let clockwise = clockwise != self.transform.mirrors_xy();
        #[cfg(test)]
        let clockwise = clockwise != FLIP_ARCS.get();
        if clockwise {
            motion.cw_arc_command.clone()
        } else {
            motion.ccw_arc_command.clone()
        }
    }

    /// The X and Y the controller holds, if both are known.
    fn emitted_xy(&self) -> Option<(f64, f64)> {
        let [x, y, _] = &self.axes;
        Some((x.as_ref()?.value, y.as_ref()?.value))
    }

    /// The end point of `arc` as written and as read back, machine units.
    fn emitted_end(&self, arc: &ArcGeometry) -> Result<Option<(Point3, f64, f64)>, ExportError> {
        let end = self.transform.to_machine(arc.end);
        let (Some(x), Some(y)) = (
            read_back(&self.number(end.x)?),
            read_back(&self.number(end.y)?),
        ) else {
            return Ok(None);
        };
        Ok(Some((end, x, y)))
    }

    /// Writes `arc` with I and J taken from the start the controller holds.
    /// Returns false, writing nothing, if the controller would reject the
    /// rounded arc or read it as a full circle.
    fn arc_ij(&mut self, arc: &ArcGeometry, feed_mm_min: f64) -> Result<bool, ExportError> {
        let Some((sx, sy)) = self.emitted_xy() else {
            return Ok(false);
        };
        let Some((end, ex, ey)) = self.emitted_end(arc)? else {
            return Ok(false);
        };
        let centre = self
            .transform
            .to_machine(Point3::new(arc.centre.x, arc.centre.y, arc.end.z));
        let (i_text, j_text) = (self.number(centre.x - sx)?, self.number(centre.y - sy)?);
        let (Some(i), Some(j)) = (read_back(&i_text), read_back(&j_text)) else {
            return Ok(false);
        };
        let quantum = self.quantum();
        // Same start and end is a full circle to the controller.
        if (ex - sx).abs() < quantum / 2.0 && (ey - sy).abs() < quantum / 2.0 {
            return Ok(false);
        }
        let scale = self.transform.mm_per_unit();
        let start_r = i.hypot(j);
        let delta_mm = ((ex - (sx + i)).hypot(ey - (sy + j)) - start_r).abs() * scale;
        let accepted = delta_mm <= ARC_RADIUS_ABSOLUTE_MM
            || (delta_mm <= ARC_RADIUS_GROSS_MM
                && delta_mm <= ARC_RADIUS_RELATIVE * start_r * scale);
        if !accepted {
            return Ok(false);
        }
        let command = self.arc_command(arc);
        self.motion_line(
            &command,
            [Some(end.x), Some(end.y), Some(end.z)],
            &[format!("I{i_text}"), format!("J{j_text}")],
            Some(feed_mm_min),
            true,
        )?;
        Ok(true)
    }

    /// Writes `arc`, at most a quarter turn, with R. Returns false, writing
    /// nothing, if the rounded radius cannot span the rounded chord.
    fn arc_r(&mut self, arc: &ArcGeometry, feed_mm_min: f64) -> Result<bool, ExportError> {
        let Some((sx, sy)) = self.emitted_xy() else {
            return Ok(false);
        };
        let Some((end, ex, ey)) = self.emitted_end(arc)? else {
            return Ok(false);
        };
        let r_text = self.number(arc.radius_mm / self.transform.mm_per_unit())?;
        let Some(r) = read_back(&r_text) else {
            return Ok(false);
        };
        let chord = (ex - sx).hypot(ey - sy);
        if chord < self.quantum() / 2.0 || chord > 2.0 * r {
            return Ok(false);
        }
        let command = self.arc_command(arc);
        self.motion_line(
            &command,
            [Some(end.x), Some(end.y), Some(end.z)],
            &[format!("R{r_text}")],
            Some(feed_mm_min),
            true,
        )?;
        Ok(true)
    }

    /// `arc` as straight moves within the project's flatten tolerance.
    fn flatten(&mut self, arc: &ArcGeometry, feed_mm_min: f64) -> Result<(), ExportError> {
        let tolerance = self.flatten_tolerance_mm;
        let max_step_rad = if tolerance >= arc.radius_mm {
            FRAC_PI_2
        } else {
            (2.0 * (1.0 - tolerance / arc.radius_mm).acos()).min(FRAC_PI_2)
        };
        let n = segment_count(arc.sweep_rad / max_step_rad);
        for k in 1..=n {
            #[allow(
                clippy::cast_precision_loss,
                reason = "k <= n <= MAX_ARC_SEGMENTS, exact in f64"
            )]
            let fraction = k as f64 / n as f64;
            self.linear(arc.point_at(fraction), feed_mm_min)?;
        }
        Ok(())
    }

    /// One output unit in the last decimal place.
    fn quantum(&self) -> f64 {
        let places = decimal_places(&self.post.number_format, self.transform.output_units());
        10f64.powi(-i32::from(places))
    }

    /// Writes one motion line: the command word unless modal and
    /// unchanged, the axis words whose written value changed (X and Y
    /// always when `force_xy`), `extra` words, and the feed when it
    /// changed. Writes nothing if no axis changed and there are no extra
    /// words.
    fn motion_line(
        &mut self,
        command: &str,
        axes: [Option<f64>; 3],
        extra: &[String],
        feed_mm_min: Option<f64>,
        force_xy: bool,
    ) -> Result<(), ExportError> {
        let format = self.post.number_format;
        let units = self.transform.output_units();
        let mut axis_words = Vec::with_capacity(3);
        let mut updated = self.axes.clone();
        for (((slot, letter), value), force) in updated
            .iter_mut()
            .zip(['X', 'Y', 'Z'])
            .zip(axes)
            .zip([force_xy, force_xy, false])
        {
            let Some(value) = value else {
                continue;
            };
            let text = format_number(value, &format, units)?;
            if !force && slot.as_ref().is_some_and(|e| e.text == text) {
                continue;
            }
            let read = read_back(&text).ok_or(ExportError::NonFiniteNumber { value })?;
            axis_words.push(format!("{letter}{text}"));
            *slot = Some(Emitted { text, value: read });
        }
        if axis_words.is_empty() && extra.is_empty() {
            return Ok(());
        }
        self.axes = updated;

        let mut words = Vec::with_capacity(axis_words.len() + extra.len() + 2);
        if !self.post.motion.modal_motion || self.motion.as_deref() != Some(command) {
            words.push(command.to_owned());
            self.motion = Some(command.to_owned());
        }
        words.extend(axis_words);
        words.extend(extra.iter().cloned());
        if let Some(feed) = feed_mm_min {
            let fs = &self.post.feed_speed;
            let text = format_number(feed / self.transform.mm_per_unit(), &format, units)?;
            let changed = self.feed.as_deref() != Some(text.as_str());
            if changed || !fs.modal_feed_speed {
                let word = format!("{}{text}", fs.feed_command);
                if fs.inline_with_motion {
                    words.push(word);
                } else if changed {
                    self.line(&word);
                }
                self.feed = Some(text);
            }
        }
        self.line(&words.join(" "));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{MotionKind, parse_motion, parse_program};
    use crate::safety::MAX_LINE_CHARS;
    use dc_cam::{FeedField, Move, MoveKind, generate_profile};
    use dc_geometry::{Point, Point3, assert_approx_eq};
    use dc_project::test_support::{sample_project, tool_6_35_upcut};
    use dc_project::{
        Operation, OperationId, OriginZ, PostProcessorRef, SpindleControl, ToolChangeMode, ToolId,
        Units,
    };

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
        project.post_processor = PostProcessorRef::latest(post_id).unwrap();
        project
    }

    /// The effective post-processor of `project`.
    fn post_of(project: &Project) -> PostProcessor {
        project.post_processor.resolve().unwrap()
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
        ops[0].1.diameter_mm = 0.0;
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

    /// The sample operation, then the same with a second tool. The
    /// project's tool-change mode becomes `"m6"`, the one program with a
    /// tool change that [`emit`] writes (#95).
    fn two_tool_ops(project: &mut Project) -> Vec<(ProfileOp, Tool, Toolpath)> {
        project.post_processor.tool_change_mode = ToolChangeMode::M6;
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
            ops[0].1.rpm = None;
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
            ops[0].1.plunge_mm_min = None;
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
        let on_line = |ops: &[(ProfileOp, Tool, Toolpath)]| -> String {
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
        ops[0].1.rpm = None;
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
            ops[0].1.rpm = Some(u32::MAX);
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
                let lift = (i + 1..start)
                    .find(|&k| has_word(lines[k], "G0") && code(lines[k]).contains('Z'));
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
            ops[1].1.name = "a very long tool name ".repeat(6);
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

            ops[1].1 = ops[0].1.clone();
            ops[1].0.tool = ops[0].1.id.clone();
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
            let (programs, _) = emit_programs(&project, &ops, DATE).unwrap();
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
                let rpm = ops[k].1.rpm.unwrap();
                assert_eq!(starts, [format!("M3 S{rpm}")], "{post} program {k}");
                assert_eq!(program.first_tool.number, k + 1, "{post} program {k}");
                let (alone, _) = emit(&project, &ops[k..=k], DATE).unwrap();
                assert_eq!(program, &alone, "{post} program {k} is its run alone");
            }
            let (again, _) = emit_programs(&project, &ops, DATE).unwrap();
            assert_eq!(again, programs, "{post}: deterministic");
        }
    }

    /// #95: with `"m6"` the programs are the one [`emit`] writes, with its
    /// `T2 M6` tool change, byte for byte.
    #[test]
    fn m6_mode_emits_one_program_with_m6() {
        let mut project = project_with("grblhal");
        let ops = two_tool_ops(&mut project);
        let (programs, _) = emit_programs(&project, &ops, DATE).unwrap();
        let (single, _) = emit(&project, &ops, DATE).unwrap();
        assert_eq!(programs, [single]);
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
}
