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
//! - The project's heights (#283; `docs/specs/phase-2/design.md`, "Phase 2
//!   addendum (#73)", A7): with a clearance height, each operation starts
//!   and ends at it, and a tool change rises to it; with a tool-change
//!   machine Z, the program lifts to it with `G53 G0 Z` before each
//!   tool-change stop and before the footer (S16); with a park point, it
//!   rapids there at the clearance height before that last lift. Unset,
//!   the program is as before.
//! - A rapid lowers Z only after its XY move.
//! - I and J are taken from the rounded start the controller holds; an arc
//!   the controller would reject, or read as a full circle, is flattened.
//! - The emitted text is parsed and checked by `safety` before return.

use std::f64::consts::{FRAC_PI_2, TAU};

use dc_cam::{CannedKind, DrillHole, DrillToolpath, FeedField, Move, MoveKind, RunLabel, Toolpath};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, MAX_COORDINATE_MM, Point, Point3};
use dc_project::{
    ArcFormat, CannedCycles, DwellUnits, Operation, OperationId, ParkXy, PostProcessor, Project,
    RetractMode, SpindleControl, Tool, ToolChangeMode, ToolId, Units, is_m6_line,
};

use crate::ExportError;
use crate::arcfit::{self, EmitOptions, Fitted, SubArc};
use crate::ascii;
use crate::canned::{Cycle, CycleWords, Step, steps_at_hole};
use crate::format::{decimal_places, format_fixed, format_number, read_back};
use crate::material::{self, Material, TabSpine};
use crate::parse::{ArcWords, Block, MachineMove, MachinePosition, MotionKind};
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
    /// The machine Z of the program's `G53` lifts, as written, with its
    /// units (`-5.000 mm`), when it has any (#283). They need a homed
    /// machine, which a caller tells the user.
    pub machine_lift: Option<String>,
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
    /// controller would have rejected one of the run's arcs, or the arcs
    /// would have strayed more than the tolerance plus one unit in the
    /// last place from the moves (#355) (`arc_fit_fallback`).
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
    /// The toolpath links some holes above safe Z, and not as the cycles
    /// can follow: a series of cycles' `G98` returns every hole to the Z
    /// before its first cycle. Holes linked all at the clearance height
    /// (#305), all at safe Z, or at the clearance height between parts and
    /// safe Z within one (one series per part, #314) keep their cycles, so
    /// a Drill from [`dc_cam::generate_drill`] never has this reason.
    ClearanceHeight,
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
            Self::ClearanceHeight => "clearance_height",
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
            Self::ClearanceHeight => write!(
                f,
                "the holes are linked above safe Z at a height no series of canned cycles returns to"
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
    /// Which of the operation's tool runs this is
    /// (`docs/specs/phase-3/design.md`, "dc-gcode"): [`RunLabel::Main`]
    /// for every operation but a carve's clearing pass (#406).
    pub label: RunLabel,
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
            label: RunLabel::Main,
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
            label: RunLabel::Main,
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
    // #283: the clearance, tool-change and park heights, checked again
    // here whatever validation found.
    let heights = Lifts::of(
        project,
        post,
        planned.iter().map(|p| p.0),
        thickness_mm,
        transform.output_units(),
    )?;

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
    // #283, #304 review: say in the program itself that it moves in
    // machine coordinates, which need a homed machine. Only with the
    // height set, so no other program's bytes change.
    if heights.machine_z_mm.is_some() {
        let comment = format!(
            "{} {} {}",
            post.program.comment_prefix,
            machine_lift_comment(&post.id),
            post.program.comment_suffix
        );
        w.line(comment.trim_end());
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
        // #406: a carve's clearing run is named after its pass's tool.
        let name = enabled
            .get(index)
            .map_or_else(|| op.name().to_owned(), |e| e.label.naming(op.name()));
        let mut context = common.clone();
        context.extend([
            ("operationIndex", (index + 1).to_string()),
            ("operationName", comment_text(&name)),
            ("toolNumber", number.to_string()),
            ("toolName", comment_text(&tool.name)),
        ]);
        if post.program.operation_header.is_empty() {
            let comment = format!(
                "{} Operation {}: {} {}",
                post.program.comment_prefix,
                index + 1,
                comment_text(&name),
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
        // #283: the operation starts at the clearance height, and a tool
        // change rises to it; safe Z when it is unset.
        let rise_z_mm = heights.rise_z_mm(safe_z_mm);
        // The tool change belongs to the operation, so the safety pass
        // checks its stop against this operation's safe Z.
        let first_line = w.next_line();
        if w.tool.is_none() {
            // S4, #57: the user fitted the first tool and set work zero
            // with it before starting, so there is no stop. Z is wherever
            // the user left it, so the program lifts to safe Z first.
            w.forget_position();
            w.retract(rise_z_mm)?;
            w.tool = Some(tool.id.clone());
        } else if w.tool.as_ref() != Some(&tool.id) {
            // S4, #57: the user fits the tool with the bit clear of the
            // work. After a router-off pause the user may have jogged, so
            // the retract is always written, unless the program has just
            // lifted to the tool-change machine Z (S16, #283): a retract in
            // work coordinates there could be a move down.
            if !w.lifted {
                w.forget_position();
                w.retract(rise_z_mm)?;
            } else if w.stopped_since_lift
                && let Some(machine_z_mm) = heights.machine_z_mm
            {
                // #304 review: the user may have jogged at the router-off
                // pause after the lift, so lift again before the change.
                w.machine_lift(machine_z_mm)?;
            }
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
            w.retract(rise_z_mm)?;
        }

        if manual {
            // S4 (manual): retract, then pause for the user to switch the
            // router on. Resuming is the confirmation, so there is no
            // dwell, and the speed is only a hint. The user may jog or set
            // Z zero at the pause, so Z is unknown after it: lift to safe
            // Z again before moving in XY (#68).
            if !router_on {
                w.retract(rise_z_mm)?;
                w.pause(&router_on_message(rates.rpm), true);
                w.forget_position();
                w.retract(rise_z_mm)?;
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
        let written = match canned_plan(
            &w,
            op,
            tool,
            toolpath,
            holes,
            safe_z_mm,
            heights.clearance_z_mm,
        )? {
            CannedPlan::NotAsked => w.toolpath(op, tool, toolpath)?,
            CannedPlan::Expand(reason) => {
                warnings.push(ExportWarning::CannedCycleExpanded {
                    op: op.id().clone(),
                    reason,
                });
                w.toolpath(op, tool, toolpath)?
            }
            CannedPlan::Cycles(lines) => {
                w.cycles(&lines)?;
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

        // #283: the operation ends at the clearance height; S6 accepts the
        // rise as a retract after its last move.
        if let Some(clearance_z_mm) = heights.clearance_z_mm {
            w.retract(clearance_z_mm)?;
        }
        let tool_changes_next = planned.get(index + 1).map(|next| next.1.id != tool.id);
        let last = index + 1 == planned.len();
        // #283: the end sequence is no operation's motion. Outside every
        // span, S2 holds it to the highest safe Z of the program.
        let end_sequence = last && (heights.park.is_some() || heights.machine_z_mm.is_some());
        let span_end = end_sequence.then(|| w.next_line());
        if last && let Some(park) = heights.park {
            // At the clearance height (or the highest safe Z), across to
            // the park point, before the machine-coordinate lift, so the
            // safety pass can check the move in work coordinates (S2, S16).
            w.retract(heights.end_z_mm)?;
            w.rapid(Point3::new(park.x_mm, park.y_mm, heights.end_z_mm))?;
        }
        if let Some(machine_z_mm) = heights.machine_z_mm
            && (last || tool_changes_next == Some(true))
        {
            // S16: before the first stop of a tool change and before the
            // footer, from at least safe Z, where the bit already is.
            w.retract(if last { heights.end_z_mm } else { rise_z_mm })?;
            w.machine_lift(machine_z_mm)?;
        }

        // Stop the spindle before a tool change, and after the last
        // operation unless the footer does it (S4). With manual control,
        // retract and pause for the user to switch the router off.
        if manual {
            if tool_changes_next.unwrap_or(true) {
                // Not from the machine-coordinate lift (S16), which is
                // higher.
                if !w.lifted {
                    w.retract(rise_z_mm)?;
                }
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
        lines.push(first_line..span_end.unwrap_or_else(|| w.next_line()));
    }

    // Step 6: footer and program end.
    let spindle_off = post.feed_speed.spindle_off.trim();
    for line in &post.program.footer {
        if !(manual && only_stops_spindle(line, spindle_off)) {
            w.template(line, &common);
        }
    }
    w.line(&post.stop.program_end_command);
    let machine_lift = heights
        .machine_z_mm
        .map(|z| w.number(z / w.transform.mm_per_unit()))
        .transpose()?
        .map(|z| format!("{z} {units_name}"));
    let tolerances = Tolerances {
        flatten_mm: w.flatten_tolerance_mm,
        arc_fit_mm: w.arc_fit_tolerance_mm,
    };
    let text = w.finish();

    // Step 7: the safety pass, on the text. The material each operation
    // may cut (S9, S17) comes from the project, not from the generator,
    // and per tool run: a carve's clearing run is held to its floor
    // (#406, #408).
    let materials: Vec<Material> = planned
        .iter()
        .zip(enabled)
        .map(|(&(op, tool, ..), entry)| Material::of_run(project, op, &entry.label, tool))
        .collect();
    // #337: and the tabs each must leave standing.
    let tabs: Vec<Vec<TabSpine>> = planned
        .iter()
        .map(|&(op, ..)| material::tab_spines(project, op))
        .collect();
    let spans: Vec<OpSpan<'_>> = planned
        .iter()
        .zip(lines)
        .zip(materials.iter().zip(&tabs))
        .map(
            |((&(op, tool, toolpath, ..), lines), (material, tabs))| OpSpan {
                op,
                tool,
                toolpath,
                material,
                tabs,
                lines,
            },
        )
        .collect();
    let (extents, blocks) = safety::check_parsed(
        &text,
        post,
        &transform,
        &project.stock,
        thickness_mm,
        tolerances,
        safety::ProgramHeights {
            machine_lift_mm: heights.machine_z_mm,
            clearance_z_mm: heights.clearance_z_mm,
        },
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
            machine_lift,
        },
        warnings,
    ))
}

/// The project's heights as one program writes them (#283; design
/// addendum A7), checked: the post-processor is the last line and trusts
/// no caller.
#[derive(Debug, Clone, Copy)]
struct Lifts {
    /// Absolute clearance Z, mm, project space, when the project sets
    /// one: at least every operation's safe Z.
    clearance_z_mm: Option<f64>,
    /// Where the program ends before the park move and the lift: the
    /// clearance Z, or the highest safe Z of the program's operations,
    /// which S2 holds motion outside every operation to.
    end_z_mm: f64,
    /// The tool-change machine Z, mm, when set: below 0, on a definition
    /// that documents `G53`.
    machine_z_mm: Option<f64>,
    /// The park point, project space.
    park: Option<ParkXy>,
}

impl Lifts {
    /// `project`'s heights for a program of `ops` on `post`.
    ///
    /// # Errors
    ///
    /// [`ExportError::ClearanceBelowSafeZ`],
    /// [`ExportError::MachineZNeedsG53`],
    /// [`ExportError::MachineZNotBelowHome`] or
    /// [`ExportError::InvalidPark`], as validation reports them first.
    fn of<'a>(
        project: &Project,
        post: &PostProcessor,
        ops: impl Iterator<Item = &'a Operation> + Clone,
        thickness_mm: f64,
        units: Units,
    ) -> Result<Self, ExportError> {
        let heights = &project.heights;
        let highest_safe_z_mm = ops
            .clone()
            .map(|op| thickness_mm + op.safe_z_mm())
            .fold(thickness_mm, f64::max);
        let clearance_z_mm = match heights.clearance_mm {
            None => None,
            Some(clearance_mm) => {
                for op in ops {
                    let safe_z_mm = op.safe_z_mm();
                    if !(clearance_mm.is_finite() && clearance_mm >= safe_z_mm) {
                        return Err(ExportError::ClearanceBelowSafeZ {
                            op: op.id().clone(),
                            clearance_mm,
                            safe_z_mm,
                        });
                    }
                }
                Some(thickness_mm + clearance_mm)
            }
        };
        if let Some(machine_z_mm) = heights.tool_change_machine_z_mm {
            if post.work_coordinates.machine_command.is_none() {
                return Err(ExportError::MachineZNeedsG53 {
                    post: post.id.clone(),
                });
            }
            // Below 0 as written: not one that rounds to 0 in the
            // program's decimal places (#304 review).
            let half_quantum_mm = post.number_format.decimal_places.quantum_mm(units) / 2.0;
            if !(machine_z_mm.is_finite() && machine_z_mm < -half_quantum_mm) {
                return Err(ExportError::MachineZNotBelowHome { machine_z_mm });
            }
        }
        if let Some(park) = heights.park {
            let in_reach = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
            if !(in_reach(park.x_mm) && in_reach(park.y_mm)) {
                return Err(ExportError::InvalidPark {
                    x_mm: park.x_mm,
                    y_mm: park.y_mm,
                });
            }
        }
        Ok(Self {
            clearance_z_mm,
            end_z_mm: clearance_z_mm.unwrap_or(highest_safe_z_mm),
            machine_z_mm: heights.tool_change_machine_z_mm,
            park: heights.park,
        })
    }

    /// The height an operation with absolute safe Z `safe_z_mm` starts,
    /// changes tools and pauses at: the clearance Z, or its safe Z.
    fn rise_z_mm(&self, safe_z_mm: f64) -> f64 {
        self.clearance_z_mm.unwrap_or(safe_z_mm)
    }
}

/// The start of the header comment of a program that lifts with `G53`
/// (#283).
const MACHINE_LIFT_COMMENT: &str = "Lifts to machine Z with G53: home the machine before running.";

/// The header comment of a program that lifts with `G53` on the
/// definition `post_id` (#283): home the machine, then what keeps a lift
/// outside the travel from moving (#308; definitions README,
/// "Machine-coordinate moves"). On grblHAL and GRBL, where a machine that
/// homes Z downward has every negative machine Z below the travel, Z
/// homing upward or soft limits on (`$20=1`). On LinuxCNC, whose soft
/// limits always apply once homed, homing that is not skipped with
/// `NO_FORCE_HOMING`. No `( ) ;` inside, which would end the comment
/// early; `$` starts a GRBL system command only at the start of a line.
fn machine_lift_comment(post_id: &str) -> String {
    let limits = match post_id {
        "grblhal" | "grbl" => Some("Needs Z to home upward, or soft limits on with $20=1"),
        "linuxcnc" => Some("Soft limits apply only once homed: leave NO_FORCE_HOMING unset"),
        _ => None,
    };
    limits.map_or_else(
        || MACHINE_LIFT_COMMENT.to_owned(),
        |limits| format!("{MACHINE_LIFT_COMMENT} {limits}"),
    )
}

/// What the export note of a program that lifts with `G53` on the
/// definition `post_id` adds after "home the machine" (#308): the
/// controller's own condition for a lift outside the travel to stop before
/// it moves, as in `machine_lift_comment`. `None` for a definition that
/// has no such check documented.
#[must_use]
pub fn machine_lift_limits_note(post_id: &str) -> Option<&'static str> {
    match post_id {
        "grblhal" | "grbl" => Some("Z must home upward, or soft limits must be on ($20=1)."),
        "linuxcnc" => Some(
            "LinuxCNC applies soft limits only once homed: leave [TRAJ] NO_FORCE_HOMING unset.",
        ),
        _ => None,
    }
}

/// The export note of a grblHAL program whose header writes `G50` (#320).
/// grblHAL's `G50` does not clear a scale that leaves X at 1: `set_scaling`
/// assigns its `changed` flag on each pass of the axis loop instead of
/// OR-ing it, so only X's factor decides `scale_changed`, and the machine's
/// state is updated only when it is set (`grblHAL/core` `4ec1ae44`,
/// `gcode.c` 226, 2799, 4475-4482). A soft reset clears every factor
/// (`gc_init`, 822), and `$G` ends with `G50` once none is left, or with
/// `G51:` and the scaled axes (`report.c` 816-821). See
/// `dc-project/src/definitions/README.md`, "Scaling on grblHAL".
const GRBLHAL_SCALING_NOTE: &str = "If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.";

/// What `export` tells the user about axis scaling before a program for
/// `post` runs (#320): `GRBLHAL_SCALING_NOTE` for a grblHAL definition
/// whose header writes `G50` (version 4 and later), so a user who trusts
/// that `G50` still soft-resets after a `G51`. `None` for every other
/// definition: GRBL and LinuxCNC have no scaling, Mach3's and UCCNC's
/// `G50` clears every factor, and an older grblHAL header cancels none.
#[must_use]
pub fn scaling_note(post: &PostProcessor) -> Option<&'static str> {
    let writes_g50 = post
        .program
        .header
        .iter()
        .any(|line| code_words(line).iter().any(|w| w == "G50"));
    (post.id == "grblhal" && writes_g50).then_some(GRBLHAL_SCALING_NOTE)
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
    /// The project does not ask for canned cycles, or this is not a Drill,
    /// or it is a countersink, which no canned cycle describes.
    NotAsked,
    /// Canned cycles were asked for, but the Drill is written as moves.
    Expand(CannedExpansion),
    /// As these canned-cycle lines.
    Cycles(CycleLines),
}

/// A Drill as canned-cycle lines: one or more series, each a rapid to its
/// first hole at the initial Z, the retract mode, one line per hole, the
/// cancel; between series, a rise to the clearance height (#314).
#[derive(Debug)]
struct CycleLines {
    /// In cut order. One, unless the toolpath links its holes at the
    /// clearance height between parts and at safe Z within one: then one
    /// per run of holes linked at safe Z (#314).
    series: Vec<Series>,
    /// With more than one series, the clearance height, project space,
    /// which the tool rises to after each series but the last, and crosses
    /// at to the next.
    rise_z_mm: Option<f64>,
    /// `G98`.
    retract_mode: &'static str,
    /// `G80`.
    cancel: String,
    /// The plunge rate as the `F` number is written, never above the
    /// tool's (#325).
    feed: String,
}

/// One series of canned cycles: `G98` returns each of its holes to the Z
/// the tool is at before its first cycle, the initial Z.
#[derive(Debug)]
struct Series {
    /// The first hole's centre at the initial Z, project space: safe Z, or
    /// the clearance height when every link between holes is at it (#305).
    first: Point3,
    /// The last hole's centre at the initial Z, where the series leaves the
    /// tool.
    last: Point3,
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
/// as canned cycles when it is a Drill but no countersink, the project's `use_canned_cycles`
/// is on, the definition has canned cycles with a command for every hole's
/// cycle, and the motion the controller makes for those lines
/// ([`crate::canned`]) is the toolpath's, move for move, within the
/// definition's re-entry gap for a peck's re-entry or chip-break retract.
/// Anything else is written as moves, with the reason when cycles were
/// asked for. The safety pass checks the written cycles again (S11).
///
/// #305: `G98` returns each hole to the Z the tool was at before the
/// first cycle of the series, so the cycles start from the height the
/// toolpath links its holes at: the clearance height `clearance_z_mm`
/// when every link is at it (holes that are separate parts, or one hole),
/// otherwise safe Z.
///
/// #314: when the links are at both heights, the clearance height between
/// parts and safe Z between the holes of one part, each run of holes
/// linked at safe Z is a series of its own from safe Z, and between series
/// the tool rises to the clearance height, crosses, and comes down to safe
/// Z over the next run's first hole. Links at any other height fail the
/// comparison and are written as moves (`CannedExpansion::ClearanceHeight`).
fn canned_plan(
    w: &Writer<'_>,
    op: &Operation,
    tool: &Tool,
    toolpath: &Toolpath,
    holes: Option<&[DrillHole]>,
    safe_z_mm: f64,
    clearance_z_mm: Option<f64>,
) -> Result<CannedPlan, ExportError> {
    use CannedExpansion as Why;
    let post = w.post;
    // A countersink (format 21) is never a canned cycle: no `G8x` can say
    // its depth is derived, and S18 refuses one. It is plain moves by
    // design, as a Pocket is, so there is nothing to warn about (#427).
    let drill = matches!(op, Operation::Drill(d) if !d.cycle.is_countersink());
    if !(post.use_canned_cycles && drill) {
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
    let links: Vec<f64> = link_heights_mm(toolpath).collect();
    let at = |z_mm: f64, height_mm: f64| (z_mm - height_mm).abs() <= EPS_MM;
    let (initial_z_mm, split_at) = match clearance_z_mm {
        // #305: one series from the clearance height.
        Some(clearance_mm) if links.iter().all(|&z| at(z, clearance_mm)) => (clearance_mm, None),
        // #314: one series per run of holes linked at safe Z, from it.
        Some(clearance_mm)
            if links.len() + 1 == holes.len()
                && links
                    .iter()
                    .all(|&z| at(z, clearance_mm) || at(z, safe_z_mm)) =>
        {
            (safe_z_mm, Some(clearance_mm))
        }
        _ => (safe_z_mm, None),
    };
    // Whether the series follow every link: all at the initial Z, or, with
    // a series per part, at safe Z or the clearance height.
    let links_followed = split_at.is_some() || links.iter().all(|&z| at(z, initial_z_mm));
    let at_initial = |hole: &DrillHole| Point3::new(hole.centre.x, hole.centre.y, initial_z_mm);
    let mut series: Vec<Series> = Vec::new();
    for (k, (hole, line)) in holes.iter().zip(lines).enumerate() {
        // Linked to the hole before at the clearance height: a new series.
        let crosses = k > 0
            && split_at
                .is_some_and(|clearance_mm| links.get(k - 1).is_some_and(|&z| at(z, clearance_mm)));
        match series.last_mut() {
            Some(current) if !crosses => {
                current.last = at_initial(hole);
                current.holes.push(line);
            }
            _ => series.push(Series {
                first: at_initial(hole),
                last: at_initial(hole),
                holes: vec![line],
            }),
        }
    }
    if series.is_empty() {
        return expand(Why::NoHoleRecords);
    }
    let plan = CycleLines {
        rise_z_mm: split_at.filter(|_| series.len() > 1),
        series,
        retract_mode: "G98",
        cancel: cycles.cancel_command.trim().to_owned(),
        // Straight down, at a rate that is the feed as written: rounded
        // down where rounding to nearest would pass the plunge rate (#325).
        feed: w.number(
            w.feed_at_most_plunge(plunge_mm_min, plunge_mm_min, |f| {
                Some(f * transform.mm_per_unit())
            })? / transform.mm_per_unit(),
        )?,
    };
    if expansion_is_toolpath(w, &plan, toolpath, plunge_mm_min, gap_mm, clearance_z_mm)? {
        Ok(CannedPlan::Cycles(plan))
    } else if !links_followed {
        // #283, #314: links at a height no series of cycles returns to.
        expand(Why::ClearanceHeight)
    } else {
        expand(Why::MotionDiffers)
    }
}

/// The Z of every move of `toolpath` across the work but its first: the
/// heights its holes are linked at, mm, project space.
fn link_heights_mm(toolpath: &Toolpath) -> impl Iterator<Item = f64> + '_ {
    toolpath.moves.windows(2).filter_map(|pair| match pair {
        [from, to] if from.to.xy().distance_to(to.to.xy()) > EPS_MM => Some(to.to.z),
        _ => None,
    })
}

/// `steps`, from Z `start_z`, with each run of rapids that keep going the
/// same way along the hole's vertical line joined into one: the
/// controller does not stop between them, and the motion is the same
/// (#305).
fn join_rapids(start_z: f64, steps: &[HoleStep]) -> Vec<HoleStep> {
    let mut joined: Vec<HoleStep> = Vec::with_capacity(steps.len());
    // The Z before the last step in `joined`, and after it.
    let (mut before, mut z) = (start_z, start_z);
    for &step in steps {
        // A rapid to where the tool already is moves nothing: the clearance
        // height equal to safe Z.
        if matches!(step, HoleStep::Rapid(to) if (to - z).abs() <= EPS_MM) {
            continue;
        }
        if let (HoleStep::Rapid(to), Some(HoleStep::Rapid(last))) = (step, joined.last_mut()) {
            let same_way = (*last - before) * (to - *last) > 0.0;
            if same_way {
                *last = to;
                z = to;
                continue;
            }
        }
        match step {
            HoleStep::Rapid(to) | HoleStep::Feed(to) => {
                before = z;
                z = to;
            }
            HoleStep::Dwell(_) => before = z,
        }
        joined.push(step);
    }
    joined
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
/// at the initial Z, then its moves in Z and dwells, in the same order,
/// each Z within one unit in the last place, a rapid inside a hole
/// already cut within the re-entry gap `gap_mm`, every cut at the plunge
/// rate. Rapids along the hole's vertical line that keep going the same
/// way are one rapid ([`join_rapids`], #305): with the cycles from the
/// clearance height the controller goes straight from it to `R` and from
/// the bottom back to it, where the toolpath stops at safe Z. The first
/// hole is reached at safe Z below the initial Z, which the tool is at
/// (the operation starts at the clearance height), and after the last the
/// operation rises to the clearance height `clearance_z_mm`. With a
/// series per part (#314) the initial Z is safe Z, each series' last hole
/// is followed by the rise to the clearance height, and the next series'
/// first hole is reached at it, then at safe Z, as the toolpath does.
fn expansion_is_toolpath(
    w: &Writer<'_>,
    plan: &CycleLines,
    toolpath: &Toolpath,
    plunge_mm_min: f64,
    gap_mm: Option<f64>,
    clearance_z_mm: Option<f64>,
) -> Result<bool, ExportError> {
    let transform = w.transform;
    let units = transform.output_units();
    let tolerance_mm = w.quantum() * transform.mm_per_unit() + EPS_MM;
    let Some(first) = plan.series.first().map(|s| s.first) else {
        return Ok(false);
    };
    let initial_mm = first.z;
    let read = |text: &str| read_back(text).ok_or(ExportError::NonFiniteNumber { value: f64::NAN });
    let initial = read(&w.number(transform.to_machine(first).z)?)?;
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
    // Each hole's line, whether the tool comes to it from the clearance
    // height after the series before (#314: it goes down to the initial Z
    // over it first), and whether it rises to the clearance height after it,
    // the last hole of a series but the last.
    let count = plan.series.len();
    let lines: Vec<(&CycleLine, bool, bool)> = plan
        .series
        .iter()
        .enumerate()
        .flat_map(|(k, series)| {
            let n = series.holes.len();
            series
                .holes
                .iter()
                .enumerate()
                .map(move |(j, line)| (line, k > 0 && j == 0, k + 1 < count && j + 1 == n))
        })
        .collect();
    if groups.len() != lines.len() {
        return Ok(false);
    }
    // #305: after the last hole the operation rises to the clearance
    // height, which is where the cycles return to when they start from it.
    if let (Some(clearance_mm), Some((_, last))) = (clearance_z_mm, groups.last_mut())
        && (clearance_mm - initial_mm).abs() <= EPS_MM
    {
        last.push(HoleStep::Rapid(clearance_mm));
    }

    for ((arrival, actual), &(line, crossed, rises)) in groups.iter().zip(&lines) {
        let centre = transform.to_project(Point3::new(line.x.value, line.y.value, 0.0), units);
        // Where the tool is over the hole before its cycle's first move:
        // the clearance height the series before rose to (#314), or the
        // initial Z.
        let (before_mm, crossed) = match plan.rise_z_mm {
            Some(rise_mm) if crossed => (rise_mm, true),
            _ => (initial_mm, false),
        };
        let arrives = if crossed {
            (arrival.z - before_mm).abs() <= tolerance_mm
        } else {
            arrival.z <= initial_mm + tolerance_mm
        };
        if centre.xy().distance_to(arrival.xy()) > tolerance_mm || !arrives {
            return Ok(false);
        }
        // Reached below the initial Z (the first hole, at safe Z): the
        // tool is over it at the initial Z, then goes down to there.
        let mut from_initial = Vec::with_capacity(actual.len() + 1);
        if !crossed && arrival.z < initial_mm - tolerance_mm {
            from_initial.push(HoleStep::Rapid(arrival.z));
        }
        from_initial.extend_from_slice(actual);
        let actual = join_rapids(before_mm, &from_initial);
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
        // already is: from the clearance height down to the initial Z, the
        // `G0` the writer puts before the series (#314); down to R (it
        // arrives at the initial Z), the cycle; then, after the series'
        // `G80`, the rise to the clearance height (#314).
        let mut expected = Vec::with_capacity(steps.len() + 3);
        if crossed {
            expected.push(HoleStep::Rapid(initial_mm));
        }
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
        if let Some(rise_mm) = plan.rise_z_mm.filter(|_| rises) {
            expected.push(HoleStep::Rapid(rise_mm));
        }
        let expected = join_rapids(before_mm, &expected);
        if expected.len() != actual.len() {
            return Ok(false);
        }
        let mut cut = false;
        for (e, a) in expected.iter().zip(&actual) {
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

/// A toolpath cut holds to its tool's plunge rate when it falls no faster
/// than that rate times one plus this: the floating-point error of a feed
/// a generator derived from the move's own ends (`dc_cam::entry`,
/// `plunge_limited_feed_mm_min`), far below one unit in any written place.
const HELD_TO_PLUNGE_RELATIVE: f64 = 1e-9;

/// How many feeds, from the highest down one unit in the last written place
/// at a time, the emitter tries for a cut it holds to the plunge rate
/// (#325). The first is enough unless the floating-point sum of
/// [`safety::descent_rate_mm_min`] lands a hair above the plunge rate.
const PLUNGE_FEED_TRIES: u32 = 4;

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
/// DreamCarve assumes (#154; LinuxCNC and Mach3 headers). `G50` and `G69`
/// set every scale factor to 1 and cancel coordinate rotation on Mach3 and
/// UCCNC (#312), which only their version 3 headers write. grblHAL's
/// version 4 header writes `G50` too, with the same meaning (#316).
const MODAL_SETUP_CODES: [&str; 17] = [
    "G17", "G20", "G21", "G40", "G49", "G50", "G54", "G55", "G56", "G57", "G58", "G59", "G69",
    "G80", "G90", "G91.1", "G94",
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
pub(crate) fn code_words(line: &str) -> Vec<String> {
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
    /// The plunge rate, mm/min, of the tool whose toolpath is being
    /// written: no cut the toolpath holds to it may fall faster, as
    /// written (S10, #325).
    plunge_mm_min: Option<f64>,
    /// The cutting feed, mm/min, of that tool: a level cut slower than
    /// it, a last pass's inside arc (#335), is written rounded down, so
    /// its cutting edge moves no faster than the generator held it to.
    cut_feed_mm_min: Option<f64>,
    /// The lines that set the controller's modal state, written again after
    /// a pause.
    setup: Vec<String>,
    /// The last motion written was the machine-coordinate lift (S16,
    /// #283): work Z is unknown, and no retract may follow before the stop.
    lifted: bool,
    /// Z, project space, after a retract from an unknown position: XY is
    /// not known, but the next rapid moves in XY at this Z before it lowers
    /// (#283: from the clearance height to an operation's safe Z).
    retracted_z_mm: Option<f64>,
    /// A stop has come since the machine-coordinate lift (#283): the user
    /// may have jogged, so a tool change lifts again.
    stopped_since_lift: bool,
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
            plunge_mm_min: None,
            cut_feed_mm_min: None,
            setup: Vec::new(),
            lifted: false,
            retracted_z_mm: None,
            stopped_since_lift: false,
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
        self.stopped_since_lift = true;
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
        match &mut self.position {
            Some(position) => position.z = to.z,
            None => self.retracted_z_mm = Some(to.z),
        }
        Ok(())
    }

    /// S16 (#283): `G53 G0 Z<machine_z_mm>` in program units, the
    /// definition's machine-coordinate command, alone on its line, at the
    /// XY where the bit is. Motion is modal `G0` after it, and work Z is
    /// unknown, so the next move writes Z in work coordinates first.
    fn machine_lift(&mut self, machine_z_mm: f64) -> Result<(), ExportError> {
        let post = self.post;
        let command = post
            .work_coordinates
            .machine_command
            .as_deref()
            .ok_or_else(|| ExportError::MachineZNeedsG53 {
                post: post.id.clone(),
            })?;
        let rapid = post.motion.rapid_command.as_str();
        let z = self.number(machine_z_mm / self.transform.mm_per_unit())?;
        self.line(&format!("{command} {rapid} Z{z}"));
        self.motion = Some(rapid.to_owned());
        self.position = None;
        self.retracted_z_mm = None;
        self.axes[2] = None;
        self.lifted = true;
        self.stopped_since_lift = false;
        Ok(())
    }

    fn forget_position(&mut self) {
        self.position = None;
        self.retracted_z_mm = None;
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
    /// reject any of its arcs. Every cut the toolpath holds to `tool`'s
    /// plunge rate is written so it falls no faster, its numbers rounded
    /// (#325).
    fn toolpath(
        &mut self,
        op: &Operation,
        tool: &Tool,
        toolpath: &Toolpath,
    ) -> Result<Written, ExportError> {
        self.plunge_mm_min = tool.plunge_mm_min;
        self.cut_feed_mm_min = tool.feed_mm_min;
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
                arcfit::apply_fallback(fitted, &opts, start, &to_machine, tolerance_mm)
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
    /// "Canned cycles", ported from `runPostProcessor`), one series at a
    /// time: a rapid to the series' first hole at the plan's initial Z
    /// (safe Z, or the clearance height, #305), so `G98`'s initial Z is
    /// that height; the retract mode; one line per hole with the command
    /// word (on the series' first line, then when it changes, or always
    /// unless motion is modal), `X Y`, `Z` (bottom), `R` (retract plane),
    /// `Q` (peck) or `P` (dwell), and `F` (plunge) on the first; then the
    /// cancel, which leaves no motion mode. Unlike upstream every line
    /// carries its `Z`, `R`, `Q` and `P`: controllers differ in which of
    /// them they keep, and the safety pass expands each line from its own
    /// words. #314: between series, after the cancel, a rapid up to the
    /// clearance height, across at it and down to the initial Z over the
    /// next series' first hole. Each series states every word again, as
    /// after `G80` LinuxCNC requires `Z` and `R` (`interp_cycles.cc`
    /// 729-734, 920-923) and Mach3 needs a sticky number "the first time"
    /// (Mach3Mill 10.7.24).
    fn cycles(&mut self, plan: &CycleLines) -> Result<(), ExportError> {
        let mut previous: Option<Point3> = None;
        for series in &plan.series {
            if let (Some(last), Some(rise_z_mm)) = (previous, plan.rise_z_mm) {
                self.rapid(Point3::new(last.x, last.y, rise_z_mm))?;
            }
            self.series(plan, series)?;
            previous = Some(series.last);
        }
        Ok(())
    }

    /// Writes one series of [`Self::cycles`], from the rapid over its first
    /// hole to its cancel.
    fn series(&mut self, plan: &CycleLines, series: &Series) -> Result<(), ExportError> {
        self.rapid(series.first)?;
        self.line(plan.retract_mode);
        let fs = &self.post.feed_speed;
        let feed_word = format!("{}{}", fs.feed_command, plan.feed);
        let inline = fs.inline_with_motion;
        let modal = self.post.motion.modal_motion;
        if !inline {
            self.line(&feed_word);
        }
        let mut command: Option<Cycle> = None;
        for (k, hole) in series.holes.iter().enumerate() {
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
        // the initial Z, which the rapid above wrote.
        self.motion = None;
        self.feed = Some(plan.feed.clone());
        if let Some(last) = series.holes.last() {
            self.axes[0] = Some(last.x.clone());
            self.axes[1] = Some(last.y.clone());
        }
        self.position = Some(series.last);
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
    /// either end. With the position unknown, Z goes first, unless a
    /// retract has set Z and `to` is lower.
    fn rapid(&mut self, to: Point3) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let from_z = self.position.map(|p| p.z).or(self.retracted_z_mm);
        let rising = from_z.is_none_or(|z| to.z >= z);
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

    /// A straight cut to `to`, held to the plunge rate when the toolpath
    /// holds it there ([`Self::holds_to_plunge`]).
    fn linear(&mut self, to: Point3, feed_mm_min: f64) -> Result<(), ExportError> {
        let held = self.position.is_some_and(|from| {
            self.holds_to_plunge(feed_mm_min, from.xy().distance_to(to.xy()), from.z - to.z)
        });
        self.linear_held(to, feed_mm_min, held)
    }

    /// A straight cut to `to`; with `held`, its feed is lowered as far as
    /// the rounding of its numbers needs to keep it from falling faster
    /// than the plunge rate ([`Self::written_feed_mm_min`]).
    fn linear_held(&mut self, to: Point3, feed_mm_min: f64, held: bool) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let post = self.post;
        let level = self
            .position
            .is_some_and(|from| (from.z - to.z).abs() <= EPS_MM);
        let feed_mm_min = if held {
            self.written_feed_mm_min(MotionKind::Linear, m, ArcWords::default(), feed_mm_min)?
        } else if level {
            self.level_feed_mm_min(feed_mm_min)?
        } else {
            feed_mm_min
        };
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

    /// Whether the toolpath holds a cut at `feed_mm_min`, `run_mm` across
    /// as it falls `drop_mm`, to the tool's plunge rate: it falls, and no
    /// faster than that rate (S10; within [`HELD_TO_PLUNGE_RELATIVE`]). A
    /// generator derives such a cut's feed from its unrounded ends
    /// (#157), and plunges straight down at the plunge rate. A cut that
    /// falls faster is not held, and S10 refuses it.
    fn holds_to_plunge(&self, feed_mm_min: f64, run_mm: f64, drop_mm: f64) -> bool {
        self.plunge_mm_min.is_some_and(|plunge_mm_min| {
            drop_mm > 0.0
                && feed_mm_min * (drop_mm / run_mm.hypot(drop_mm))
                    <= plunge_mm_min * (1.0 + HELD_TO_PLUNGE_RELATIVE)
        })
    }

    /// Whether the toolpath holds `arc` at `feed_mm_min` to the plunge
    /// rate ([`Self::holds_to_plunge`]), its length along the arc. Its
    /// chords, should it be flattened, are held with it: the emitter, not
    /// the toolpath, made them steeper than the arc.
    fn holds_arc_to_plunge(&self, arc: &ArcGeometry, feed_mm_min: f64) -> bool {
        self.holds_to_plunge(
            feed_mm_min,
            arc.radius_mm * arc.sweep_rad,
            arc.start.z - arc.end.z,
        )
    }

    /// The feed, mm/min, to write for a cut held to the plunge rate from
    /// where the controller is to machine point `to` (an arc's with its
    /// centre words `arc`): `feed_mm_min`, unless at it, as written, the
    /// cut would fall faster than the plunge rate, by the sum S10 makes
    /// of the written numbers ([`safety::descent_rate_mm_min`]). Then the
    /// fastest feed in the last written place, rounded down, that does
    /// not (#325). Rounded coordinates can make a short cut steeper than
    /// the toolpath's, and a rounded feed faster; only here are both
    /// known. With the start not known, S10 has nothing to judge.
    fn written_feed_mm_min(
        &self,
        kind: MotionKind,
        to: Point3,
        arc: ArcWords,
        feed_mm_min: f64,
    ) -> Result<f64, ExportError> {
        let (Some(plunge_mm_min), [Some(x), Some(y), Some(z)]) = (
            self.plunge_mm_min,
            self.axes.each_ref().map(|a| a.as_ref().map(|e| e.value)),
        ) else {
            return Ok(feed_mm_min);
        };
        let from = MachinePosition {
            x: Some(x),
            y: Some(y),
            z: Some(z),
        };
        let to = MachinePosition {
            x: Some(self.written(to.x)?),
            y: Some(self.written(to.y)?),
            z: Some(self.written(to.z)?),
        };
        let units = self.transform.output_units();
        let rate = |feed: f64| {
            let mv = MachineMove {
                line: 0,
                kind,
                from,
                to,
                feed: Some(feed),
                units,
                arc,
            };
            safety::descent_rate_mm_min(&mv, &self.transform)
        };
        self.feed_at_most_plunge(feed_mm_min, plunge_mm_min, rate)
    }

    /// `feed_mm_min` if a cut at it as written falls no faster than
    /// `plunge_mm_min`; otherwise the fastest feed in the last written
    /// place, rounded down, that does, in mm/min. `rate` is the cut's
    /// vertical rate, mm/min, at a written feed in output units per
    /// minute, `None` if it does not fall; it is proportional to the
    /// feed. `feed_mm_min` if no feed one unit or more in the last place
    /// is slow enough, which S10 refuses.
    fn feed_at_most_plunge(
        &self,
        feed_mm_min: f64,
        plunge_mm_min: f64,
        rate: impl Fn(f64) -> Option<f64>,
    ) -> Result<f64, ExportError> {
        let mm_per_unit = self.transform.mm_per_unit();
        let written = self.written(feed_mm_min / mm_per_unit)?;
        let Some(rate_mm_min) = rate(written).filter(|r| *r > plunge_mm_min) else {
            return Ok(feed_mm_min);
        };
        let quantum = self.quantum();
        let mut steps = (plunge_mm_min * written / rate_mm_min / quantum).floor();
        for _ in 0..PLUNGE_FEED_TRIES {
            if steps < 1.0 {
                break;
            }
            let feed = self.written(steps * quantum)?;
            if rate(feed).is_some_and(|r| r <= plunge_mm_min) {
                return Ok(feed * mm_per_unit);
            }
            steps -= 1.0;
        }
        Ok(feed_mm_min)
    }

    /// `feed_mm_min`, mm/min, as written for a level cut: rounded down to
    /// the last written place when it is slower than the tool's cutting
    /// feed and is not its plunge rate (#335: a last pass's inside arc,
    /// whose cutting edge must move no faster than the feed), so the
    /// controller never runs it faster than the toolpath holds it;
    /// otherwise as it is, rounded as every feed is. A toolpath without
    /// such cuts is written as before.
    fn level_feed_mm_min(&self, feed_mm_min: f64) -> Result<f64, ExportError> {
        let slower = self
            .cut_feed_mm_min
            .is_some_and(|cut| feed_mm_min < cut * (1.0 - HELD_TO_PLUNGE_RELATIVE));
        let plunge = self
            .plunge_mm_min
            .is_some_and(|p| (feed_mm_min - p).abs() <= p * HELD_TO_PLUNGE_RELATIVE);
        if !slower || plunge {
            return Ok(feed_mm_min);
        }
        let mm_per_unit = self.transform.mm_per_unit();
        let units = feed_mm_min / mm_per_unit;
        let written = self.written(units)?;
        if written <= units {
            return Ok(written * mm_per_unit);
        }
        let quantum = self.quantum();
        Ok(self.written((units / quantum).floor() * quantum)? * mm_per_unit)
    }

    /// `value`, output units, as the controller reads it once written.
    fn written(&self, value: f64) -> Result<f64, ExportError> {
        read_back(&self.number(value)?).ok_or(ExportError::NonFiniteNumber { value })
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
        let words = ArcWords {
            i: Some(i),
            j: Some(j),
            r: None,
        };
        let feed_mm_min = self.arc_feed_mm_min(arc, command, end, words, feed_mm_min)?;
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
        let words = ArcWords {
            i: None,
            j: None,
            r: Some(r),
        };
        let feed_mm_min = self.arc_feed_mm_min(arc, command, end, words, feed_mm_min)?;
        self.motion_line(
            command,
            [Some(end.x), Some(end.y), Some(end.z)],
            &[format!("R{r_text}")],
            Some(feed_mm_min),
            true,
        )?;
        Ok(true)
    }

    /// The feed, mm/min, to write for `arc`, written with `command` to
    /// machine point `end` and centre words `words`: lowered, if the
    /// toolpath holds it to the plunge rate, as [`Self::written_feed_mm_min`]
    /// finds.
    fn arc_feed_mm_min(
        &self,
        arc: &ArcGeometry,
        command: &str,
        end: Point3,
        words: ArcWords,
        feed_mm_min: f64,
    ) -> Result<f64, ExportError> {
        if !self.holds_arc_to_plunge(arc, feed_mm_min) {
            return if (arc.start.z - arc.end.z).abs() <= EPS_MM {
                self.level_feed_mm_min(feed_mm_min)
            } else {
                Ok(feed_mm_min)
            };
        }
        let kind = if command == self.post.motion.cw_arc_command {
            MotionKind::ArcCw
        } else {
            MotionKind::ArcCcw
        };
        self.written_feed_mm_min(kind, end, words, feed_mm_min)
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
        let held = self.holds_arc_to_plunge(arc, feed_mm_min);
        for k in 1..=n {
            #[allow(
                clippy::cast_precision_loss,
                reason = "k <= n <= MAX_ARC_SEGMENTS, exact in f64"
            )]
            let fraction = k as f64 / n as f64;
            self.linear_held(arc.point_at(fraction), feed_mm_min, held)?;
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
        // A move in work coordinates: the machine-coordinate lift is behind.
        self.lifted = false;
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
                    c.is_empty()
                        || ["G90", "G17", "G94", "G80", "G40", "G50"].contains(&c.as_str()),
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
                        .unwrap_or_else(|e| {
                            panic!("{} {units:?} {plunge_mm_min}: {e}", definition.id)
                        })
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
            })
            .collect();
        assert_eq!(ops[0].label, RunLabel::Clearing(ToolId::new("t1")));
        project.post_processor.tool_change_mode = ToolChangeMode::SeparatePrograms;
        let (programs, _) =
            emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE).unwrap();
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
        dc_project::load(include_bytes!("../../../tests/fixtures/plaque.dcv"))
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
            "../../dreamcarve/tests/fixtures/heights-migration/plaque.dcv"
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
            include_str!("../../dreamcarve/tests/expected/tabs-migration/plaque-1-T1.nc"),
            include_str!("../../dreamcarve/tests/expected/tabs-migration/plaque-2-T2.nc"),
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
                let (programs, _) =
                    emit_programs(&project, &ops, ProgramSplit::ProjectDefault, DATE)
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
                    let (mut project, op) = drill_project_with_two_hole_part_as(
                        dc_project::DrillCycle::Simple,
                        dwell_s,
                    );
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
                    lines[g98 - 1].starts_with('X')
                        && lines[..g98].iter().any(|l| l.ends_with(z20)),
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
}
