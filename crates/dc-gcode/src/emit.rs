// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/postprocessor.ts

//! G-code emission (`docs/specs/phase-1/design.md`, "Algorithms →
//! Emission"; `docs/specs/phase-1/functional.md`, S1–S7).
//!
//! Ported from `runPostProcessor`, without coolant. Arc fitting is
//! DreamCarve's (`arcfit`); canned cycles are written as upstream's block
//! does, but only when the controller's motion for them is the toolpath
//! (`docs/specs/phase-2/design.md`, "Canned cycles"; see [`EmitOp`] and
//! [`CannedExpansion`]). Re-derived for Y-up: toolpaths are
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
//!   post-processor's `tool_change.mode` `"m6"` (or `export --one-program`,
//!   [`ProgramSplit::OneProgram`]) and a definition whose tool change has
//!   `M6` (grblHAL) does one program change tools; otherwise
//!   [`emit`] refuses a second tool. The change retracts to safe Z and
//!   stops the spindle, then
//!   writes DreamCarve's own `T<n> M6 (MSG, Fit T<n> <name>. Set Z zero,
//!   then resume.)` in place of the definition's `M6` line, leaving out
//!   lines that only pause and `pause_after_change`: one stop per change.
//!   The user may jog or set Z zero there, so the position is then unknown
//!   and the program retracts to safe Z again before the spindle starts or
//!   the router-on pause (#43). A program that changes tools first sets the
//!   controller's current tool with `M61 Q<n>`, because grblHAL skips an
//!   `M6` for the tool it holds. `<n>`, here and in `{toolNumber}`, is the
//!   tool's `number` when set, otherwise its 1-based place in the
//!   project's tool list (#120); two tools of one program with the same
//!   number, or a number of 0, fail the export.
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

use dc_cam::{CannedKind, DrillHole, DrillToolpath, FeedField, Move, MoveKind, Toolpath};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3};
use dc_project::{
    ArcFormat, CannedCycles, DwellUnits, Operation, OperationId, PostProcessor, Project,
    RetractMode, SpindleControl, Tool, ToolChangeMode, ToolId, Units, is_m6_line,
};

use crate::ExportError;
use crate::arcfit::{self, EmitOptions, Fitted, SubArc};
use crate::ascii;
use crate::canned::{Cycle, CycleWords, Step, steps_at_hole};
use crate::format::{decimal_places, format_fixed, format_number, read_back};
use crate::material::Material;
use crate::parse::Block;
use crate::safety::{self, DWELL_TOLERANCE_S, Extents, OpSpan, Tolerances};
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
    /// Its id, to find it in the project's tool list.
    pub id: ToolId,
    /// Its number, `T<n>`, which also names a tool run's program file: the
    /// tool's `number` when set, otherwise its 1-based place in the
    /// project's tool list (#120, capability 21a). Not an index.
    pub number: u32,
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
    /// The project asks for canned cycles, but this Drill was written as
    /// plain moves, all of it (`canned_cycle_expanded`). The holes, their
    /// order and their depths are the same either way.
    CannedCycleExpanded {
        /// The operation.
        op: OperationId,
        /// Why.
        reason: CannedExpansion,
    },
    /// The controller has no units words (UCCNC), so the program has none:
    /// it is in `units`, and the controller must be set up in them
    /// (`units_set_by_controller`). One per program.
    UnitsSetByController {
        /// The post-processor's id.
        post: String,
        /// The program's units.
        units: Units,
    },
}

impl ExportWarning {
    /// The stable `snake_case` code of the warning, as `check` and
    /// `export` print it before the message.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DisabledOperationSkipped { .. } => "disabled_operation_skipped",
            Self::ArcsFlattened { .. } => "arcs_flattened",
            Self::ArcFitFallback { .. } => "arc_fit_fallback",
            Self::CannedCycleExpanded { .. } => "canned_cycle_expanded",
            Self::UnitsSetByController { .. } => "units_set_by_controller",
        }
    }
}

/// Why a Drill the project asked to write as canned cycles was written as
/// plain moves ([`ExportWarning::CannedCycleExpanded`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannedExpansion {
    /// The post-processor definition has no canned cycles.
    NoCannedCycles,
    /// The Drill came without its hole records.
    NoHoleRecords,
    /// A hole's cycle has no canned equivalent: a peck with a dwell.
    CycleNeedsMoves,
    /// The definition has no command for this cycle, or not its standard
    /// code, which is the only one the safety pass expands.
    NoCommand(Cycle),
    /// The definition writes something the safety pass cannot expand: a
    /// retract mode other than `G98`, a cancel other than `G80`, or a peck
    /// word other than `Q`. Names the field.
    UnsupportedDefinition(&'static str),
    /// A peck cycle, and the definition states no re-entry gap for it
    /// (`canned_cycles.reentry_gap_mm`).
    NoReentryGap,
    /// The motion the controller makes for the cycles, from the numbers as
    /// written, would not be the toolpath. A Drill from
    /// [`dc_cam::generate_drill`] pecks from the retract plane as the
    /// controller does (#153), so this means a toolpath or hole records
    /// from elsewhere, or numbers rounded too far apart.
    MotionDiffers,
}

impl CannedExpansion {
    /// The stable `snake_case` code of the reason.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoCannedCycles => "no_canned_cycles",
            Self::NoHoleRecords => "no_hole_records",
            Self::CycleNeedsMoves => "cycle_needs_moves",
            Self::NoCommand(_) => "no_command",
            Self::UnsupportedDefinition(_) => "unsupported_definition",
            Self::NoReentryGap => "no_reentry_gap",
            Self::MotionDiffers => "motion_differs",
        }
    }
}

impl std::fmt::Display for CannedExpansion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCannedCycles => write!(f, "the post-processor has no canned cycles"),
            Self::NoHoleRecords => write!(f, "the Drill came without its hole records"),
            Self::CycleNeedsMoves => write!(
                f,
                "a peck with a dwell has no canned cycle, so it is written as plain moves"
            ),
            Self::NoCommand(cycle) => write!(
                f,
                "the post-processor has no {} command for this cycle",
                cycle.code()
            ),
            Self::UnsupportedDefinition(field) => write!(
                f,
                "the post-processor's canned_cycles.{field} is not one DreamCarve can check"
            ),
            Self::NoReentryGap => write!(
                f,
                "the post-processor states no re-entry gap (canned_cycles.reentry_gap_mm) for peck cycles"
            ),
            Self::MotionDiffers => write!(
                f,
                "the controller's motion for the cycle would differ from the toolpath"
            ),
        }
    }
}

/// Fewest decimal places accepted in millimetres. The safety pass tolerates
/// one unit in the last place, so this bounds that tolerance at 0.01 mm.
pub const MIN_DECIMAL_PLACES_MM: u8 = 2;
/// Fewest decimal places accepted in inches: one unit is 0.0254 mm.
pub const MIN_DECIMAL_PLACES_INCH: u8 = 3;

/// One operation to emit: the operation, its tool, its toolpath, and for a
/// Drill the hole records [`dc_cam::generate_drill`] returned with it, from
/// which the emitter may write canned cycles (`docs/specs/phase-2/design.md`,
/// "Canned cycles"). The toolpath is the authority: the safety pass checks
/// that a canned cycle's expansion is the toolpath, move for move (S11).
#[derive(Debug, Clone, PartialEq)]
pub struct EmitOp {
    /// The operation.
    pub op: Operation,
    /// Its tool.
    pub tool: Tool,
    /// Its toolpath, project space, mm.
    pub toolpath: Toolpath,
    /// A Drill's holes in cut order; `None` writes plain moves.
    pub holes: Option<Vec<DrillHole>>,
}

impl EmitOp {
    /// A Drill operation with its generated toolpath and hole records.
    #[must_use]
    pub fn drill(op: Operation, tool: Tool, drilled: DrillToolpath) -> Self {
        Self {
            op,
            tool,
            toolpath: drilled.toolpath,
            holes: Some(drilled.holes),
        }
    }
}

impl From<(Operation, Tool, Toolpath)> for EmitOp {
    /// An operation written as plain moves.
    fn from((op, tool, toolpath): (Operation, Tool, Toolpath)) -> Self {
        Self {
            op,
            tool,
            toolpath,
            holes: None,
        }
    }
}

/// How [`emit_programs`] splits a project whose enabled operations use
/// more than one tool: as the project's post-processor `tool_change_mode`
/// says, or as an `export` flag overrides it for this one export
/// (`docs/specs/phase-2/functional.md`, capability 19). The project is
/// never changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgramSplit {
    /// As the project's `tool_change_mode` says (#95).
    #[default]
    ProjectDefault,
    /// One program with `T<n> M6` tool changes (`--one-program`), whatever
    /// the project's mode. Needs a definition whose tool change has `M6`;
    /// otherwise [`ExportError::OneProgramNeedsM6`].
    OneProgram,
    /// One program per tool run (`--programs-per-tool`), whatever the
    /// project's mode.
    ProgramPerTool,
}

/// Emits the programs `export` writes for the enabled operations in `ops`,
/// as [`emit`] does, split as `split` says: by default as the
/// post-processor's `tool_change_mode` (#95):
///
/// - `"separate_programs"` (or [`ProgramSplit::ProgramPerTool`]): one
///   program per tool run, a run being
///   consecutive operations with the same tool, in operation order.
///   Operations are never regrouped by tool, which would change the cut
///   order. Each program is exactly what [`emit`] writes for its run alone:
///   a one-tool program with no `M6` and no `M61`, checked by the safety
///   pass on its own, with its own extents. A one-tool project is one
///   program, the same as [`emit`]'s.
/// - `"m6"` (or [`ProgramSplit::OneProgram`]): the one program [`emit`]
///   writes, with `T<n> M6` at each tool change.
///
/// Warnings are those of every program, disabled operations once.
///
/// # Errors
///
/// [`ExportError::OneProgramNeedsM6`] for [`ProgramSplit::OneProgram`] on
/// a definition whose tool change has no `M6`, before anything is emitted.
/// Otherwise as for [`emit`]; the first program that fails fails the whole
/// call, so a caller never has some of the programs.
pub fn emit_programs(
    project: &Project,
    ops: &[EmitOp],
    split: ProgramSplit,
    date: &str,
) -> Result<(Vec<Program>, Vec<ExportWarning>), ExportError> {
    let mut post = project.post_processor.resolve()?;
    let mode = match split {
        ProgramSplit::ProjectDefault => post.tool_change.mode,
        ProgramSplit::OneProgram => {
            // GRBL 1.1 has no `M6`, so no program of its own can stop for a
            // new bit (#57); the flag asks for exactly that.
            if !post.tool_change.has_m6() {
                return Err(ExportError::OneProgramNeedsM6 {
                    post: post.id.clone(),
                });
            }
            ToolChangeMode::M6
        }
        ProgramSplit::ProgramPerTool => ToolChangeMode::SeparatePrograms,
    };
    // The effective post-processor for this export only; `project` is
    // borrowed and stays as it is, so the flag is never saved.
    post.tool_change.mode = mode;
    let (enabled, mut warnings) = enabled_ops(ops)?;
    let runs: Vec<&[&EmitOp]> = match mode {
        ToolChangeMode::M6 => vec![enabled.as_slice()],
        ToolChangeMode::SeparatePrograms => {
            enabled.chunk_by(|a, b| a.tool.id == b.tool.id).collect()
        }
    };
    let mut programs = Vec::with_capacity(runs.len());
    for run in runs {
        let (program, run_warnings) = emit_enabled(project, &post, run, date)?;
        // Every program of the export has the same units: say so once.
        for warning in run_warnings {
            let about_units = matches!(warning, ExportWarning::UnitsSetByController { .. });
            if !(about_units && warnings.contains(&warning)) {
                warnings.push(warning);
            }
        }
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
/// and phase 2's S9–S12 before it is returned. For S9 the material each
/// operation may cut is worked out from `project`, not taken from the
/// toolpath.
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
    ops: &[EmitOp],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    emit_with(project, &project.post_processor.resolve()?, ops, date)
}

/// [`emit`] with the effective post-processor `post` given. Tests use it
/// to try definitions that no bundled version has.
pub(crate) fn emit_with(
    project: &Project,
    post: &PostProcessor,
    ops: &[EmitOp],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    let (enabled, mut warnings) = enabled_ops(ops)?;
    let (program, emitted) = emit_enabled(project, post, &enabled, date)?;
    warnings.extend(emitted);
    Ok((program, warnings))
}

/// The enabled entries of `ops`, and a warning for each disabled one.
/// Fails when none is enabled.
fn enabled_ops(ops: &[EmitOp]) -> Result<(Vec<&EmitOp>, Vec<ExportWarning>), ExportError> {
    let mut warnings = Vec::new();
    let mut enabled = Vec::with_capacity(ops.len());
    for entry in ops {
        if entry.op.enabled() {
            enabled.push(entry);
        } else {
            warnings.push(ExportWarning::DisabledOperationSkipped {
                op: entry.op.id().clone(),
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
    enabled: &[&EmitOp],
    date: &str,
) -> Result<(Program, Vec<ExportWarning>), ExportError> {
    let control = post.feed_speed.spindle_control;
    let manual = control == SpindleControl::Manual;
    let mut warnings = Vec::new();

    // Step 1: preconditions, all checked before a line is written. The
    // post-processor is the last line and trusts no caller.
    let mut planned = Vec::with_capacity(enabled.len());
    for &entry in enabled {
        let (op, tool, toolpath) = (&entry.op, &entry.tool, &entry.toolpath);
        let rates = Rates::of(tool, control)?;
        let number = tool_number(project, &tool.id).ok_or_else(|| ExportError::UnknownTool {
            op: op.id().clone(),
            tool: tool.id.clone(),
        })?;
        planned.push((op, tool, toolpath, rates, number, entry.holes.as_deref()));
    }
    check_tool_numbers(
        planned
            .iter()
            .map(|&(_, tool, _, _, number, _)| (tool, number)),
    )?;
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
        .map(|&(_, tool, _, _, number, _)| FirstTool {
            id: tool.id.clone(),
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
    // A controller with no units words (UCCNC) reads every length in the
    // units it is set up in, which the program cannot check; the export
    // says which units the program is in. A definition with one units
    // command but not the other is incomplete.
    let units_command = if controller_sets_units(post) {
        warnings.push(ExportWarning::UnitsSetByController {
            post: post.id.clone(),
            units: transform.output_units(),
        });
        None
    } else {
        Some(required(units_command, &post.id, units_field)?)
    };
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
        ("unitsCommand", units_command.unwrap_or_default().to_owned()),
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
    if let Some(units_command) = units_command
        && !header_has("{unitsCommand}")
    {
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
    for (index, &(op, tool, toolpath, rates, number, holes)) in planned.iter().enumerate() {
        let mut context = common.clone();
        context.extend([
            ("operationIndex", (index + 1).to_string()),
            ("operationName", comment_text(op.name())),
            ("toolNumber", number.to_string()),
            ("toolName", comment_text(&tool.name)),
        ]);
        if post.program.operation_header.is_empty() {
            let comment = format!(
                "{} Operation {}: {} {}",
                post.program.comment_prefix,
                index + 1,
                comment_text(op.name()),
                post.program.comment_suffix
            );
            w.line(comment.trim_end());
        } else {
            for line in &post.program.operation_header {
                w.template(line, &context);
            }
        }

        let spindle_off = post.feed_speed.spindle_off.trim();
        let safe_z_mm = thickness_mm + op.safe_z_mm();
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
                w.line(&dwell_line(post, dwell_s)?);
            }
        }

        // A Drill is written as canned cycles when the project asks and
        // the controller's motion for them is the toolpath; otherwise as
        // its moves, with a warning when cycles were asked for. Never
        // both for one operation.
        let written = match canned_plan(&w, op, tool, toolpath, holes, safe_z_mm)? {
            CannedPlan::NotAsked => w.toolpath(op, toolpath)?,
            CannedPlan::Expand(reason) => {
                warnings.push(ExportWarning::CannedCycleExpanded {
                    op: op.id().clone(),
                    reason,
                });
                w.toolpath(op, toolpath)?
            }
            CannedPlan::Cycles(lines) => {
                w.cycles(&lines, safe_z_mm)?;
                Written::default()
            }
        };
        if written.flattened > 0 {
            warnings.push(ExportWarning::ArcsFlattened {
                op: op.id().clone(),
                count: written.flattened,
            });
        }
        if written.fallback_runs > 0 {
            warnings.push(ExportWarning::ArcFitFallback {
                op: op.id().clone(),
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

    // Step 7: the safety pass, on the text. The material each operation
    // may cut (S9) comes from the project, not from the generator.
    let materials: Vec<Material> = planned
        .iter()
        .map(|&(op, ..)| Material::of(project, op))
        .collect();
    let spans: Vec<OpSpan<'_>> = planned
        .iter()
        .zip(lines)
        .zip(&materials)
        .map(|((&(op, tool, toolpath, ..), lines), material)| OpSpan {
            op,
            tool,
            toolpath,
            material,
            lines,
        })
        .collect();
    let (extents, blocks) = safety::check_parsed(
        &text,
        post,
        &transform,
        &project.stock,
        thickness_mm,
        tolerances,
        &spans,
    )?;
    let pauses = pauses(&text, &blocks);

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

/// The definition's dwell command for `seconds`: `{seconds}` filled with
/// the number its `units` say `P` takes, seconds to the millisecond or
/// whole milliseconds (Mach3; design spec, "Post-processor definitions").
/// Used for the spindle spin-up and for a Drill's dwell alike.
fn dwell_line(post: &PostProcessor, seconds: f64) -> Result<String, ExportError> {
    let value = dwell_value(post, seconds)?;
    Ok(post
        .feed_speed
        .spindle_on_dwell
        .command
        .replace("{seconds}", &value))
}

/// `seconds` as the number a dwell's `P` takes on `post`: seconds to the
/// millisecond, or whole milliseconds. A `G82`'s `P` is the same.
fn dwell_value(post: &PostProcessor, seconds: f64) -> Result<String, ExportError> {
    match post.feed_speed.spindle_on_dwell.units {
        DwellUnits::Seconds => format_fixed(seconds, DWELL_DECIMAL_PLACES, &post.number_format),
        DwellUnits::Milliseconds => format_fixed(seconds * 1000.0, 0, &post.number_format),
    }
}

/// How a Drill is written ([`canned_plan`]).
#[derive(Debug)]
enum CannedPlan {
    /// The project does not ask for canned cycles, or this is not a Drill.
    NotAsked,
    /// Canned cycles were asked for, but the Drill is written as moves.
    Expand(CannedExpansion),
    /// As these canned-cycle lines.
    Cycles(CycleLines),
}

/// A Drill as canned-cycle lines: rapid to the first hole at safe Z, the
/// retract mode, one line per hole, the cancel.
#[derive(Debug)]
struct CycleLines {
    /// The first hole's centre at safe Z, project space.
    first: Point3,
    /// The last hole's centre at safe Z, where the cycles leave the tool.
    last: Point3,
    /// `G98`.
    retract_mode: &'static str,
    /// `G80`.
    cancel: String,
    /// The plunge rate, as the `F` number is written.
    feed: String,
    /// One per hole, in cut order.
    holes: Vec<CycleLine>,
}

/// The words of one hole's cycle line, as written.
#[derive(Debug)]
struct CycleLine {
    cycle: Cycle,
    x: Emitted,
    y: Emitted,
    z: String,
    r: String,
    /// The peck number, for `G83` and `G73`.
    q: Option<String>,
    /// The dwell number, for `G82`.
    p: Option<String>,
}

/// The canned cycle a hole record names, if it has one.
const fn cycle_of(kind: CannedKind) -> Option<Cycle> {
    match kind {
        CannedKind::G81 => Some(Cycle::G81),
        CannedKind::G82 => Some(Cycle::G82),
        CannedKind::G83 => Some(Cycle::G83),
        CannedKind::G73 => Some(Cycle::G73),
        CannedKind::Expanded => None,
    }
}

/// The definition's command for `cycle`.
fn command_of(cycles: &CannedCycles, cycle: Cycle) -> Option<&str> {
    match cycle {
        Cycle::G81 => cycles.drill_command.as_deref(),
        Cycle::G82 => cycles.drill_with_dwell_command.as_deref(),
        Cycle::G83 => cycles.peck_drill_command.as_deref(),
        Cycle::G73 => cycles.chip_break_drill_command.as_deref(),
    }
}

/// How `op` is written (`docs/specs/phase-2/design.md`, "Canned cycles"):
/// as canned cycles when it is a Drill, the project's `use_canned_cycles`
/// is on, the definition has canned cycles with a command for every hole's
/// cycle, and the motion the controller makes for those lines
/// ([`crate::canned`]) is the toolpath's, move for move, within the
/// definition's re-entry gap for a peck's re-entry or chip-break retract.
/// Anything else is written as moves, with the reason when cycles were
/// asked for. The safety pass checks the written cycles again (S11).
fn canned_plan(
    w: &Writer<'_>,
    op: &Operation,
    tool: &Tool,
    toolpath: &Toolpath,
    holes: Option<&[DrillHole]>,
    safe_z_mm: f64,
) -> Result<CannedPlan, ExportError> {
    use CannedExpansion as Why;
    let post = w.post;
    if !(post.use_canned_cycles && matches!(op, Operation::Drill(_))) {
        return Ok(CannedPlan::NotAsked);
    }
    let expand = |why| Ok(CannedPlan::Expand(why));
    let Some(cycles) = &post.canned_cycles else {
        return expand(Why::NoCannedCycles);
    };
    let Some(holes) = holes.filter(|h| !h.is_empty()) else {
        return expand(Why::NoHoleRecords);
    };
    if cycles.retract_mode != Some(RetractMode::G98) {
        return expand(Why::UnsupportedDefinition("retract_mode"));
    }
    if !cycles.cancel_command.trim().eq_ignore_ascii_case("G80") {
        return expand(Why::UnsupportedDefinition("cancel_command"));
    }
    let mut kinds = Vec::with_capacity(holes.len());
    for hole in holes {
        let Some(cycle) = cycle_of(hole.cycle) else {
            return expand(Why::CycleNeedsMoves);
        };
        let standard = command_of(cycles, cycle).is_some_and(|c| c.trim() == cycle.code());
        if !standard {
            return expand(Why::NoCommand(cycle));
        }
        kinds.push(cycle);
    }
    let pecks = kinds.iter().any(|c| c.pecks());
    if pecks && !cycles.peck_step_word.trim().eq_ignore_ascii_case("Q") {
        return expand(Why::UnsupportedDefinition("peck_step_word"));
    }
    let gap_mm = cycles.reentry_gap_mm.filter(|g| g.is_finite() && *g > 0.0);
    if pecks && gap_mm.is_none() {
        return expand(Why::NoReentryGap);
    }
    // Validated by `Rates::of` before anything was written.
    let plunge_mm_min = tool.plunge_mm_min.unwrap_or_default();

    let transform = w.transform;
    let mut lines = Vec::with_capacity(holes.len());
    for (hole, &cycle) in holes.iter().zip(&kinds) {
        let centre = transform.to_machine(Point3::new(hole.centre.x, hole.centre.y, 0.0));
        let emitted = |value: f64| -> Result<Emitted, ExportError> {
            let text = w.number(value)?;
            let value = read_back(&text).ok_or(ExportError::NonFiniteNumber { value })?;
            Ok(Emitted { text, value })
        };
        let z_of = |z_mm: f64| transform.to_machine(Point3::new(0.0, 0.0, z_mm)).z;
        let q = match (cycle.pecks(), hole.peck_mm) {
            (true, Some(peck_mm)) => Some(w.number(peck_mm / transform.mm_per_unit())?),
            (true, None) => return expand(Why::MotionDiffers),
            (false, _) => None,
        };
        let p = match (cycle, hole.dwell_s) {
            (Cycle::G82, Some(seconds)) => Some(dwell_value(post, seconds)?),
            (Cycle::G82, None) => return expand(Why::MotionDiffers),
            _ => None,
        };
        lines.push(CycleLine {
            cycle,
            x: emitted(centre.x)?,
            y: emitted(centre.y)?,
            z: w.number(z_of(hole.bottom_z_mm))?,
            r: w.number(z_of(hole.retract_z_mm))?,
            q,
            p,
        });
    }
    let at_safe = |hole: &DrillHole| Point3::new(hole.centre.x, hole.centre.y, safe_z_mm);
    let (Some(first), Some(last)) = (holes.first(), holes.last()) else {
        return expand(Why::NoHoleRecords);
    };
    let plan = CycleLines {
        first: at_safe(first),
        last: at_safe(last),
        retract_mode: "G98",
        cancel: cycles.cancel_command.trim().to_owned(),
        feed: w.number(plunge_mm_min / transform.mm_per_unit())?,
        holes: lines,
    };
    if expansion_is_toolpath(w, &plan, toolpath, plunge_mm_min, gap_mm)? {
        Ok(CannedPlan::Cycles(plan))
    } else {
        expand(Why::MotionDiffers)
    }
}

/// One thing the tool does at a hole, project Z, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HoleStep {
    Rapid(f64),
    Feed(f64),
    /// Seconds.
    Dwell(f64),
}

/// Whether the motion the controller makes for `plan`'s lines, from the
/// numbers as written, is `toolpath`: each hole's arrival over its centre
/// at safe Z, then its moves in Z and dwells, in the same order, each Z
/// within one unit in the last place, a rapid inside a hole already cut
/// within the re-entry gap `gap_mm`, every cut at the plunge rate.
fn expansion_is_toolpath(
    w: &Writer<'_>,
    plan: &CycleLines,
    toolpath: &Toolpath,
    plunge_mm_min: f64,
    gap_mm: Option<f64>,
) -> Result<bool, ExportError> {
    let transform = w.transform;
    let units = transform.output_units();
    let tolerance_mm = w.quantum() * transform.mm_per_unit() + EPS_MM;
    let initial_mm = plan.first.z;
    let read = |text: &str| read_back(text).ok_or(ExportError::NonFiniteNumber { value: f64::NAN });
    let initial = read(&w.number(transform.to_machine(plan.first).z)?)?;
    let gap = gap_mm.map(|g| g / transform.mm_per_unit());
    let dwell_s = |p: f64| match w.post.feed_speed.spindle_on_dwell.units {
        DwellUnits::Seconds => p,
        DwellUnits::Milliseconds => p / 1000.0,
    };

    // The toolpath, one group per hole: the rapid that arrives over it,
    // then what the tool does there.
    let mut groups: Vec<(Point3, Vec<HoleStep>)> = Vec::new();
    let mut at: Option<Point> = None;
    for mv in &toolpath.moves {
        let arrives = at.is_none_or(|p| p.distance_to(mv.to.xy()) > tolerance_mm);
        at = Some(mv.to.xy());
        if arrives {
            if !mv.is_rapid() {
                return Ok(false);
            }
            groups.push((mv.to, Vec::new()));
            continue;
        }
        let Some((_, steps)) = groups.last_mut() else {
            return Ok(false);
        };
        steps.push(match mv.kind {
            MoveKind::Rapid => HoleStep::Rapid(mv.to.z),
            MoveKind::Linear
                if mv
                    .feed_mm_min
                    .is_some_and(|f| (f - plunge_mm_min).abs() <= EPS_MM) =>
            {
                HoleStep::Feed(mv.to.z)
            }
            MoveKind::Dwell { seconds } => HoleStep::Dwell(seconds),
            _ => return Ok(false),
        });
    }
    if groups.len() != plan.holes.len() {
        return Ok(false);
    }

    for ((arrival, actual), line) in groups.iter().zip(&plan.holes) {
        let centre = transform.to_project(Point3::new(line.x.value, line.y.value, 0.0), units);
        if centre.xy().distance_to(arrival.xy()) > tolerance_mm
            || (arrival.z - initial_mm).abs() > tolerance_mm
        {
            return Ok(false);
        }
        let words = CycleWords {
            cycle: line.cycle,
            z: read(&line.z)?,
            r: read(&line.r)?,
            q: line.q.as_deref().map(read).transpose()?,
            p: line.p.as_deref().map(read).transpose()?,
        };
        let Ok(steps) = steps_at_hole(&words, gap, initial.max(words.r)) else {
            return Ok(false);
        };
        // The controller's moves at the hole, without those to where it
        // already is: down to R (it arrives at the initial Z), the cycle.
        let mut expected = Vec::with_capacity(steps.len() + 1);
        let mut z = initial;
        for step in [Step::Rapid(words.r)].into_iter().chain(steps) {
            match step {
                Step::Rapid(to) | Step::Feed(to) if (to - z).abs() <= f64::EPSILON => {}
                Step::Rapid(to) => {
                    expected.push(HoleStep::Rapid(transform.project_z(to, units)));
                    z = to;
                }
                Step::Feed(to) => {
                    expected.push(HoleStep::Feed(transform.project_z(to, units)));
                    z = to;
                }
                Step::Dwell(p) => expected.push(HoleStep::Dwell(dwell_s(p))),
            }
        }
        if expected.len() != actual.len() {
            return Ok(false);
        }
        let mut cut = false;
        for (e, a) in expected.iter().zip(actual) {
            let same = match (*e, *a) {
                (HoleStep::Rapid(e), HoleStep::Rapid(a)) => {
                    let slack = if cut { gap_mm.unwrap_or(0.0) } else { 0.0 };
                    (e - a).abs() <= tolerance_mm + slack
                }
                (HoleStep::Feed(e), HoleStep::Feed(a)) => {
                    cut = true;
                    (e - a).abs() <= tolerance_mm
                }
                (HoleStep::Dwell(e), HoleStep::Dwell(a)) => (e - a).abs() <= DWELL_TOLERANCE_S,
                _ => false,
            };
            if !same {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
thread_local! {
    /// Test-only mutation: when set, every arc is written the other way
    /// round, so a test can show the safety pass rejects it (#60).
    pub(crate) static FLIP_ARCS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Most straight moves one flattened arc becomes.
const MAX_ARC_SEGMENTS: u32 = 100_000;

/// Arcs are written with at most this sweep in R format, where a
/// half-circle leaves the controller unable to place the centre.
const MAX_R_ARC_SWEEP_RAD: f64 = FRAC_PI_2;

/// An arc whose sweep is below this is drawn as a straight move.
const MIN_ARC_SWEEP_RAD: f64 = 1e-9;

/// An arc whose radius is below this is drawn as a straight move.
const MIN_ARC_RADIUS_MM: f64 = 1e-9;

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
                    tool_id: tool.id.clone(),
                    field,
                });
            }
        }
        let rpm = tool.rpm.filter(|r| *r > 0);
        if rpm.is_none() && control == SpindleControl::Automatic {
            return Err(ExportError::MissingRpm {
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
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

/// The `T` number of the project's tool `id` (#120, capability 21a): its
/// `number` when set, otherwise its 1-based place in the project's tool
/// list. `None` when the project has no tool `id`.
fn tool_number(project: &Project, id: &ToolId) -> Option<u32> {
    let (index, tool) = project
        .tools
        .iter()
        .enumerate()
        .find(|(_, t)| &t.id == id)?;
    // A place in the list is far below u32::MAX.
    tool.number.or_else(|| u32::try_from(index + 1).ok())
}

/// Fails unless every tool of a program has a `T` number of 1 or more
/// that no other tool of it has: the program would fit the wrong bit.
/// Validation refuses both first; the post-processor trusts no caller.
fn check_tool_numbers<'a>(
    tools: impl IntoIterator<Item = (&'a Tool, u32)>,
) -> Result<(), ExportError> {
    let mut numbered: Vec<(u32, &ToolId)> = Vec::new();
    for (tool, number) in tools {
        if number == 0 {
            return Err(ExportError::ToolNumberZero {
                tool: tool.id.clone(),
            });
        }
        match numbered.iter().find(|&&(n, _)| n == number) {
            Some(&(_, other)) if other != &tool.id => {
                return Err(ExportError::DuplicateToolNumber {
                    tool: tool.id.clone(),
                    other: other.clone(),
                    number,
                });
            }
            Some(_) => {}
            None => numbered.push((number, &tool.id)),
        }
    }
    Ok(())
}

/// Whether a program for `post` may change tools: the user chose one
/// program with `M6` (`tool_change.mode` `"m6"`, #95) and its tool change
/// has an `M6` line, which GRBL 1.1 lacks (#57).
pub(crate) fn changes_tools_in_program(post: &PostProcessor) -> bool {
    post.tool_change.mode == ToolChangeMode::M6 && post.tool_change.has_m6()
}

/// Whether `post`'s controller has no units words, so it reads every
/// length in the units it is set up in: the definition has neither
/// `units.mm_command` nor `units.inch_command`. UCCNC has no `G20`/`G21`;
/// its units are the profile's (`dc-project/src/definitions/README.md`).
pub(crate) fn controller_sets_units(post: &PostProcessor) -> bool {
    post.units.mm_command.is_none() && post.units.inch_command.is_none()
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
fn tool_change_message(number: u32, name: &str) -> String {
    let head = format!("Fit T{number}");
    let room = MAX_MESSAGE_CHARS.saturating_sub(head.len() + SET_Z_ZERO.len() + 3);
    let name = sanitize_comment(name, room);
    if name.is_empty() {
        format!("{head}. {SET_Z_ZERO}")
    } else {
        format!("{head} {name}. {SET_Z_ZERO}")
    }
}

/// A line that delimits a program, on its own (RS274/NGC, NIST IR 6556,
/// 3.1). It is not run.
pub(crate) const PROGRAM_DELIMITER: &str = "%";

/// The highest line number, `N99999`: five digits (NIST IR 6556, 3.3.1).
const MAX_LINE_NUMBER: u32 = 99_999;

/// Absolute distance mode. S5 requires it before the first move and after
/// every pause.
const ABSOLUTE_COMMAND: &str = "G90";

/// The codes a header line may hold and still be written again after a
/// pause: they set modal state and nothing else (S5). `G40`, `G49` and
/// `G80` cancel cutter compensation, tool length offset and canned-cycle
/// motion, and `G91.1` selects incremental arc centres, each the state
/// DreamCarve assumes (#154; LinuxCNC and Mach3 headers).
const MODAL_SETUP_CODES: [&str; 15] = [
    "G17", "G20", "G21", "G40", "G49", "G54", "G55", "G56", "G57", "G58", "G59", "G80", "G90",
    "G91.1", "G94",
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

/// Every `M0` and `M6` line of `text`, with its message; `blocks` are
/// those `text` parses into.
fn pauses(text: &str, blocks: &[Block]) -> Vec<Pause> {
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
    out
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

/// `text` made safe for a comment on GRBL and grblHAL: non-ASCII
/// characters transliterated where [`ascii::transliterate`] has an entry
/// (#278), then printable ASCII only, without `( ) ;` (which end a comment
/// early), `! ? ~` (real-time commands even inside a comment), `{ }`
/// (placeholder syntax) or `%` (a program delimiter to some senders),
/// whitespace collapsed, and at most `max_chars` long, counted after
/// transliteration.
fn sanitize_comment(text: &str, max_chars: usize) -> String {
    let mut ascii = String::with_capacity(text.len());
    for c in text.chars() {
        match ascii::transliterate(c) {
            Some(spelled) => ascii.push_str(spelled),
            // ASCII is kept or replaced below; an unmapped character is
            // replaced by the filter, as before #278.
            None => ascii.push(c),
        }
    }
    let cleaned: String = ascii
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
        (radius_mm > MIN_ARC_RADIUS_MM && sweep_rad > MIN_ARC_SWEEP_RAD).then_some(Self {
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
    count.ceil().clamp(1.0, f64::from(MAX_ARC_SEGMENTS)) as usize
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

    /// Writes `content` as a line, with the next line number if the
    /// definition numbers lines. A `%` line is a program delimiter, which
    /// a controller recognises only at the start of the line (RS274/NGC,
    /// NIST IR 6556, 3.1; Using Mach3Mill 1.84, 10.5.5), so it gets none.
    /// Line numbers have at most five digits (NIST IR 6556, 3.3.1; Using
    /// Mach3Mill 10.5.1, where they "may be repeated or used out of
    /// order"), so after `N99999` they start again from the increment.
    fn line(&mut self, content: &str) {
        if self.post.program.line_numbers && content.trim() != PROGRAM_DELIMITER {
            self.lines.push(format!("N{} {content}", self.line_number));
            let increment = self.post.program.line_number_increment;
            let next = self.line_number.saturating_add(increment);
            self.line_number = if next > MAX_LINE_NUMBER {
                increment
            } else {
                next
            };
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
    fn change_tool(&mut self, number: u32, message: &str) {
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
            let setup = std::mem::take(&mut self.setup);
            for line in &setup {
                self.line(line);
            }
            self.setup = setup;
        }
    }

    /// Raises the tool to `safe_z_mm` (project space) unless it is known to
    /// be there or higher. XY does not change.
    fn retract(&mut self, safe_z_mm: f64) -> Result<(), ExportError> {
        let to = self.position.map_or(Point3::new(0.0, 0.0, safe_z_mm), |p| {
            Point3::new(p.x, p.y, p.z.max(safe_z_mm))
        });
        let z = self.transform.to_machine(to).z;
        let post = self.post;
        self.motion_line(
            &post.motion.rapid_command,
            [None, None, Some(z)],
            &[],
            None,
            false,
        )?;
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
    fn toolpath(&mut self, op: &Operation, toolpath: &Toolpath) -> Result<Written, ExportError> {
        // Every cut carries a feed, checked before fitting so the error
        // names the move whatever the fitting does.
        for (index, mv) in toolpath.moves.iter().enumerate() {
            if !mv.is_rapid()
                && !mv.is_dwell()
                && !mv.feed_mm_min.is_some_and(|f| f.is_finite() && f > 0.0)
            {
                return Err(ExportError::MoveWithoutFeed {
                    op: op.id().clone(),
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

    /// Writes a Drill as canned cycles (`docs/specs/phase-2/design.md`,
    /// "Canned cycles", ported from `runPostProcessor`): a rapid to the
    /// first hole at safe Z, Z first, so `G98`'s initial Z is safe Z; the
    /// retract mode once; one line per hole with the command word (when it
    /// changes, or always unless motion is modal), `X Y`, `Z` (bottom), `R`
    /// (retract plane), `Q` (peck) or `P` (dwell), and `F` (plunge) on the
    /// first; then the cancel, which leaves no motion mode. Unlike upstream
    /// every line carries its `Z`, `R`, `Q` and `P`: controllers differ in
    /// which of them they keep, and the safety pass expands each line from
    /// its own words.
    fn cycles(&mut self, plan: &CycleLines, safe_z_mm: f64) -> Result<(), ExportError> {
        self.rapid(plan.first)?;
        self.line(plan.retract_mode);
        let fs = &self.post.feed_speed;
        let feed_word = format!("{}{}", fs.feed_command, plan.feed);
        let inline = fs.inline_with_motion;
        let modal = self.post.motion.modal_motion;
        if !inline {
            self.line(&feed_word);
        }
        let mut command: Option<Cycle> = None;
        for (k, hole) in plan.holes.iter().enumerate() {
            let mut words = Vec::with_capacity(8);
            if !modal || command != Some(hole.cycle) {
                words.push(hole.cycle.code().to_owned());
                command = Some(hole.cycle);
            }
            words.push(format!("X{}", hole.x.text));
            words.push(format!("Y{}", hole.y.text));
            words.push(format!("Z{}", hole.z));
            words.push(format!("R{}", hole.r));
            if let Some(q) = &hole.q {
                words.push(format!("Q{q}"));
            }
            if let Some(p) = &hole.p {
                words.push(format!("P{p}"));
            }
            if k == 0 && inline {
                words.push(feed_word.clone());
            }
            self.line(&words.join(" "));
        }
        self.line(&plan.cancel);
        // `G80` leaves no motion mode. The tool is over the last hole at
        // the initial Z, safe Z, which the rapid above wrote.
        self.motion = None;
        self.feed = Some(plan.feed.clone());
        if let Some(last) = plan.holes.last() {
            self.axes[0] = Some(last.x.clone());
            self.axes[1] = Some(last.y.clone());
        }
        self.position = Some(Point3::new(plan.last.x, plan.last.y, safe_z_mm));
        Ok(())
    }

    /// Writes toolpath move `index` as it is; returns how many arcs had to
    /// be flattened (0 or 1).
    fn write_move(
        &mut self,
        op: &Operation,
        index: usize,
        mv: &Move,
    ) -> Result<usize, ExportError> {
        if mv.is_rapid() {
            self.rapid(mv.to)?;
            return Ok(0);
        }
        if let MoveKind::Dwell { seconds } = mv.kind {
            // The tool waits where it is: no motion, no feed.
            if !(seconds.is_finite() && seconds > 0.0) {
                return Err(ExportError::InvalidDwell {
                    post: self.post.id.clone(),
                    duration_s: seconds,
                });
            }
            let line = dwell_line(self.post, seconds)?;
            self.line(&line);
            return Ok(0);
        }
        let feed = mv
            .feed_mm_min
            .filter(|f| f.is_finite() && *f > 0.0)
            .ok_or_else(|| ExportError::MoveWithoutFeed {
                op: op.id().clone(),
                index,
            })?;
        let (centre, clockwise) = match mv.kind {
            // Rapids and dwells are written above.
            MoveKind::Rapid | MoveKind::Linear | MoveKind::Dwell { .. } => {
                self.linear(mv.to, feed)?;
                return Ok(0);
            }
            MoveKind::ArcCw { centre } => (centre, true),
            MoveKind::ArcCcw { centre } => (centre, false),
        };
        let start = self.position.ok_or_else(|| ExportError::ArcWithoutStart {
            op: op.id().clone(),
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
        op: &Operation,
        index: usize,
        arc: &SubArc,
        feed_mm_min: f64,
    ) -> Result<(), ExportError> {
        let (sx, sy) = self
            .emitted_xy()
            .ok_or_else(|| ExportError::ArcWithoutStart {
                op: op.id().clone(),
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
            command,
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
        let post = self.post;
        let command = post.motion.rapid_command.as_str();
        let z_only = [None, None, Some(m.z)];
        let xy_only = [Some(m.x), Some(m.y), None];
        let (first, second) = if rising {
            (z_only, xy_only)
        } else {
            (xy_only, z_only)
        };
        self.motion_line(command, first, &[], None, false)?;
        self.motion_line(command, second, &[], None, false)?;
        self.position = Some(to);
        Ok(())
    }

    fn linear(&mut self, to: Point3, feed_mm_min: f64) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let post = self.post;
        self.motion_line(
            &post.motion.linear_command,
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
    fn arc_command(&self, arc: &ArcGeometry) -> &'a str {
        self.arc_command_for(arc.clockwise)
    }

    /// The arc command for an arc that is `clockwise` in project space.
    fn arc_command_for(&self, clockwise: bool) -> &'a str {
        let post: &'a PostProcessor = self.post;
        let motion = &post.motion;
        let clockwise = clockwise != self.transform.mirrors_xy();
        #[cfg(test)]
        let clockwise = clockwise != FLIP_ARCS.get();
        if clockwise {
            &motion.cw_arc_command
        } else {
            &motion.ccw_arc_command
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
            command,
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
            command,
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
        // The axes this line changes; the rest keep what was written.
        let mut changed_axes: [Option<Emitted>; 3] = [None, None, None];
        for ((((slot, written), letter), value), force) in changed_axes
            .iter_mut()
            .zip(&self.axes)
            .zip(['X', 'Y', 'Z'])
            .zip(axes)
            .zip([force_xy, force_xy, false])
        {
            let Some(value) = value else {
                continue;
            };
            let text = format_number(value, &format, units)?;
            if !force && written.as_ref().is_some_and(|e| e.text == text) {
                continue;
            }
            let read = read_back(&text).ok_or(ExportError::NonFiniteNumber { value })?;
            axis_words.push(format!("{letter}{text}"));
            *slot = Some(Emitted { text, value: read });
        }
        if axis_words.is_empty() && extra.is_empty() {
            return Ok(());
        }
        for (slot, changed) in self.axes.iter_mut().zip(changed_axes) {
            if changed.is_some() {
                *slot = changed;
            }
        }

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
                } else {
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
        Operation, OperationId, OriginZ, PostProcessorRef, ProfileOp, SpindleControl,
        ToolChangeMode, ToolId, Units,
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
            let (again, _) =
                emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
            assert_eq!(again, programs, "{post}: deterministic");
        }
    }

    /// #95: with `"m6"` the programs are the one [`emit`] writes, with its
    /// `T2 M6` tool change, byte for byte.
    #[test]
    fn m6_mode_emits_one_program_with_m6() {
        let mut project = project_with("grblhal");
        let ops = two_tool_ops(&mut project);
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
            let (unset, _) =
                emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
            let (set, _) =
                emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProgramPerTool, DATE).unwrap();
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
        let (expected, _) =
            emit_programs(&separate, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
        let (single, _) = emit(&project, &ops, DATE).unwrap();
        assert_eq!(programs, [single], "m6: one program");
        project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
        dc_cam::test_support::assert_safe(
            &drilled.toolpath,
            project,
            &Operation::Drill(op.clone()),
        );
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

    /// Whether `text` has any canned-cycle word.
    fn has_cycle_word(text: &str) -> bool {
        ["G73", "G80", "G81", "G82", "G83", "G98", "G99"]
            .iter()
            .any(|w| text.lines().any(|l| has_word(l, w)))
    }

    /// The lines between the one `G98` and the one `G80` of `text`; also
    /// asserts the line before `G98` is the rapid over the first hole.
    fn cycle_lines(text: &str, first: (f64, f64)) -> Vec<String> {
        let codes = codes(text);
        let at = |word: &str| -> usize {
            let found: Vec<usize> = (0..codes.len())
                .filter(|&i| codes[i].split_whitespace().any(|w| w == word))
                .collect();
            assert_eq!(found.len(), 1, "one {word} in\n{text}");
            found[0]
        };
        let (g98, g80) = (at("G98"), at("G80"));
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
            let (program, warnings) =
                emit_with(&project, &canned_post(&project), &ops, DATE).unwrap();
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
}
