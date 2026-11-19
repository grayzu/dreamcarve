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
//! [`CannedExpansion`]). Re-derived for Y-up: toolpaths are Y-up and so
//! is the machine, so arc direction carries over unchanged unless the
//! definition's axis map mirrors the plane. Changes from upstream, from
//! the controller facts in `dc-project/src/definitions/README.md`
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
//!   to safe Z again before it moves in XY.
//! - There is no stop for the first tool: the user fits it and sets work
//!   zero with it before starting. By default a program has one tool:
//!   [`emit_programs`] writes one program per tool run, which every sender
//!   and controller stops between. Only with the post-processor's
//!   `tool_change.mode` `"m6"` (or [`ProgramSplit::OneProgram`]) and a
//!   definition whose tool change has `M6` (grblHAL) does one program change
//!   tools; otherwise [`emit`] refuses a second tool. The change retracts to
//!   safe Z and stops the spindle, then writes DreamCarve's own `T<n> M6
//!   (MSG, Fit T<n> <name>. Set Z zero, then resume.)` in place of the
//!   definition's `M6` line, leaving out lines that only pause and
//!   `pause_after_change`: one stop per change. The user may jog or set Z
//!   zero there, so the position is then unknown and the program retracts to
//!   safe Z again before the spindle starts or the router-on pause. A
//!   program that changes tools first sets the controller's current tool
//!   with `M61 Q<n>`, because grblHAL skips an `M6` for the tool it holds.
//!   `<n>`, here and in `{toolNumber}`, is the tool's `number` when set,
//!   otherwise its place in the project's tool list, from 1; two tools of
//!   one program with the same number, or a number of 0, fail the export.
//! - Before the first move the program sets absolute distance mode (`G90`)
//!   as well as units and work coordinates, adding any the header leaves
//!   out. After every pause that more motion follows, it sets them again,
//!   with the header's other lines that only set modal state (`G17`,
//!   `G94`): the user may change any of them at the controller.
//! - The project's heights (`docs/specs/phase-2/design.md`, A7): with a
//!   clearance height, each operation starts and ends at it, and a tool
//!   change rises to it; with a tool-change machine Z, the program lifts
//!   to it with `G53 G0 Z` before each tool-change stop and before the
//!   footer (S16); with a park point, it rapids there at the clearance
//!   height before that last lift. Unset, the program is as before.
//! - A rapid lowers Z only after its XY move.
//! - I and J are taken from the rounded start the controller holds; an arc
//!   the controller would reject, or read as a full circle, is flattened.
//! - The emitted text is parsed and checked by `safety` before return.

use std::f64::consts::{FRAC_PI_2, TAU};

use dc_cam::{CannedKind, DrillHole, DrillToolpath, FeedField, Move, MoveKind, RunLabel, Toolpath};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, MAX_COORDINATE_MM, Point, Point3};
use dc_project::{
    ArcFormat, Board, CannedCycles, DwellUnits, Operation, OperationId, ParkXy, PostProcessor,
    Project, RetractMode, SpindleControl, Tool, ToolChangeMode, ToolId, Units, is_m6_line,
};

use crate::ExportError;
use crate::arcfit::{self, EmitOptions, Fitted, SubArc};
use crate::ascii;
use crate::canned::{Cycle, CycleWords, Step, steps_at_hole};
use crate::format::{decimal_places, format_fixed, format_number, read_back};
use crate::material::{Material, TabSpine};
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
    /// The board the program cuts (#498): the stock, or an Inlay's male
    /// board. Its origin, extents and safety checks are that board's.
    pub board: Board,
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
    /// The board the run cuts (#498): [`Board::Female`], the stock, for
    /// every run but an Inlay male's, which is on the Inlay's male board.
    /// Its toolpath is in that board's project space, and the safety pass
    /// holds it to that board.
    pub board: Board,
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
            board: Board::Female,
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
            board: Board::Female,
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
/// Each board is a setup of its own (#498): the entries on the stock
/// ([`Board::Female`]) are emitted first, as above, then those on the
/// male board ([`Board::Male`], an Inlay's male), as above again, each
/// male program emitted and checked by the safety pass against the
/// project on the male board ([`Project::on_male_board`]): its origin,
/// its thickness (S1), its rectangle (S7) and its top (S17). Every
/// [`Program`] says its board.
///
/// Warnings are those of every program, disabled operations once.
///
/// # Errors
///
/// [`ExportError::OneProgramNeedsM6`] for [`ProgramSplit::OneProgram`] on
/// a definition whose tool change has no `M6`, before anything is emitted.
/// [`ExportError::NotOnMaleBoard`] or [`ExportError::MaleBoardsDiffer`] for
/// male board entries with no one male board to hold them to.
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
    let mut programs = Vec::new();
    // #498: each board is its own setup, so its own run of programs: the
    // stock's (every operation's runs but an Inlay male's) first, then the
    // male board's, each in the order given.
    for board in [Board::Female, Board::Male] {
        let on_board: Vec<&EmitOp> = enabled
            .iter()
            .copied()
            .filter(|e| e.board == board)
            .collect();
        if on_board.is_empty() {
            continue;
        }
        let board_project = board_project(project, &on_board)?;
        let runs: Vec<&[&EmitOp]> = match mode {
            ToolChangeMode::M6 => vec![on_board.as_slice()],
            ToolChangeMode::SeparatePrograms => {
                on_board.chunk_by(|a, b| a.tool.id == b.tool.id).collect()
            }
        };
        for run in runs {
            let (program, run_warnings) = emit_enabled(&board_project, &post, run, date)?;
            // Every program of the export has the same units: say so once.
            for warning in run_warnings {
                let about_units = matches!(warning, ExportWarning::UnitsSetByController { .. });
                if !(about_units && warnings.contains(&warning)) {
                    warnings.push(warning);
                }
            }
            programs.push(program);
        }
    }
    Ok((programs, warnings))
}

/// The project the entries `on_board`, all of one board, are cut in
/// (#498): `project` itself for the stock, and for the male board
/// `project` on the male board of their Inlays
/// ([`Project::on_male_board`]), which must be one board. The emitter does
/// not trust its caller: a male board entry of an operation that is no
/// Inlay, or of an Inlay whose male board has no thickness, is refused,
/// and so are two Inlays whose male boards differ.
///
/// # Errors
///
/// [`ExportError::NotOnMaleBoard`] and [`ExportError::MaleBoardsDiffer`].
fn board_project<'p>(
    project: &'p Project,
    on_board: &[&EmitOp],
) -> Result<std::borrow::Cow<'p, Project>, ExportError> {
    let Some(first) = on_board.first() else {
        return Ok(std::borrow::Cow::Borrowed(project));
    };
    if first.board == Board::Female {
        return Ok(std::borrow::Cow::Borrowed(project));
    }
    let mut male: Option<(&OperationId, Project)> = None;
    for entry in on_board {
        let Operation::Inlay(inlay) = &entry.op else {
            return Err(ExportError::NotOnMaleBoard {
                op: entry.op.id().clone(),
            });
        };
        if !inlay.cuts_male() {
            return Err(ExportError::NotOnMaleBoard {
                op: inlay.id.clone(),
            });
        }
        match &male {
            None => male = Some((&inlay.id, project.on_male_board(inlay))),
            Some((first, board)) => {
                if inlay.male_board.stock(&project.stock) != board.stock {
                    return Err(ExportError::MaleBoardsDiffer {
                        op: inlay.id.clone(),
                        first: (*first).clone(),
                    });
                }
            }
        }
    }
    Ok(male.map_or(std::borrow::Cow::Borrowed(project), |(_, p)| {
        std::borrow::Cow::Owned(p)
    }))
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
    // #498: one program is cut on one board.
    if let Some(first) = enabled.first()
        && let Some(other) = enabled.iter().find(|e| e.board != first.board)
    {
        return Err(ExportError::BoardsMixed {
            op: other.op.id().clone(),
        });
    }
    let board_project = board_project(project, &enabled)?;
    let (program, emitted) = emit_enabled(&board_project, post, &enabled, date)?;
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
            rates,
            toolpath,
            holes,
            safe_z_mm,
            heights.clearance_z_mm,
        )? {
            CannedPlan::NotAsked => w.toolpath(op, tool, rates, toolpath)?,
            CannedPlan::Expand(reason) => {
                warnings.push(ExportWarning::CannedCycleExpanded {
                    op: op.id().clone(),
                    reason,
                });
                w.toolpath(op, tool, rates, toolpath)?
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
    // (#406, #408), a male Inlay's cut-out to its floor ring (#443).
    // #337: and the tabs each must leave standing, a cut-out's too. A
    // run whose tabs do not resolve has no material (#711).
    let held: Vec<(Material, Vec<TabSpine>)> = planned
        .iter()
        .zip(enabled)
        .map(|(&(op, tool, ..), entry)| {
            Material::held_run(project, op, &entry.label, tool, entry.board)
        })
        .collect();
    let spans: Vec<OpSpan<'_>> = planned
        .iter()
        .zip(lines)
        .zip(&held)
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
            board: enabled.first().map_or(Board::Female, |e| e.board),
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
    rates: Rates,
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
    let plunge_mm_min = rates.plunge_mm_min;

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

/// A tool's rates as the user set them, read once, here, after they are
/// checked (review 1.7): no emitter code reads a tool's optional rate
/// again, so none can stand a default in for one. "Never invent a feed"
/// holds by this type, not by the order of the checks. The spindle speed
/// is always set with automatic spindle control, optional with manual.
/// The moves carry the feeds; the safety pass checks them against the
/// tool's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rates {
    /// The cutting feed, mm/min: finite and above 0.
    feed_mm_min: f64,
    /// The plunge rate, mm/min: finite and above 0.
    plunge_mm_min: f64,
    rpm: Option<u32>,
}

impl Rates {
    /// The tool's rates, or the first one that is missing. A placeholder 0
    /// counts as missing.
    fn of(tool: &Tool, control: SpindleControl) -> Result<Self, ExportError> {
        let set = |rate: Option<f64>, field| {
            rate.filter(|r| r.is_finite() && *r > 0.0)
                .ok_or_else(|| ExportError::MissingFeed {
                    tool: tool.name.clone(),
                    tool_id: tool.id.clone(),
                    field,
                })
        };
        let feed_mm_min = set(tool.feed_mm_min, FeedField::Feed)?;
        let plunge_mm_min = set(tool.plunge_mm_min, FeedField::Plunge)?;
        let rpm = tool.rpm.filter(|r| *r > 0);
        if rpm.is_none() && control == SpindleControl::Automatic {
            return Err(ExportError::MissingRpm {
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
            });
        }
        Ok(Self {
            feed_mm_min,
            plunge_mm_min,
            rpm,
        })
    }

    /// Whether the toolpath holds a cut at `feed_mm_min`, `run_mm` across
    /// as it falls `drop_mm`, to the tool's plunge rate: it falls, and no
    /// faster than that rate (S10; within [`HELD_TO_PLUNGE_RELATIVE`]). A
    /// generator derives such a cut's feed from its unrounded ends
    /// (#157), and plunges straight down at the plunge rate. A cut that
    /// falls faster is not held, and S10 refuses it.
    fn holds_to_plunge(self, feed_mm_min: f64, run_mm: f64, drop_mm: f64) -> bool {
        drop_mm > 0.0
            && feed_mm_min * (drop_mm / run_mm.hypot(drop_mm))
                <= self.plunge_mm_min * (1.0 + HELD_TO_PLUNGE_RELATIVE)
    }

    /// Whether the toolpath holds `arc` at `feed_mm_min` to the plunge
    /// rate ([`Self::holds_to_plunge`]), its length along the arc. Its
    /// chords, should it be flattened, are held with it: the emitter, not
    /// the toolpath, made them steeper than the arc.
    fn holds_arc_to_plunge(self, arc: &ArcGeometry, feed_mm_min: f64) -> bool {
        self.holds_to_plunge(
            feed_mm_min,
            arc.radius_mm * arc.sweep_rad,
            arc.start.z - arc.end.z,
        )
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
    /// plunge rate, `rates.plunge_mm_min`, is written so it falls no
    /// faster, its numbers rounded (S10, #325), and a level cut slower
    /// than its cutting feed, `rates.feed_mm_min`, a last pass's inside
    /// arc, is written rounded down, so its cutting edge moves no faster
    /// than the generator held it to (#335).
    fn toolpath(
        &mut self,
        op: &Operation,
        tool: &Tool,
        rates: Rates,
        toolpath: &Toolpath,
    ) -> Result<Written, ExportError> {
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
        // A Photo V-Carve's grooves are never fitted (#646): their points
        // are not on arcs, and S19 holds every cut to a straight one. Nor
        // is a tapered ball nose's carve (#681): near its tip the ball cuts
        // nothing a written unit deep, so its chains run level at the
        // start plane within micrometres of the walls, where an arc fitted
        // within the tolerance would take the tip across one, which S17
        // refuses however small.
        let fitting = self
            .arc_fit_tolerance_mm
            .filter(|_| self.post.motion.arc_interpolation)
            .filter(|_| !matches!(op, Operation::PhotoVCarve(_)))
            .filter(|_| !matches!(tool.cutter, dc_project::ToolKind::TaperedBallNose { .. }));
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
                    written.flattened += self.write_move(op, rates, index, &mv)?;
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
        rates: Rates,
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
                self.linear(mv.to, feed, rates)?;
                return Ok(0);
            }
            MoveKind::ArcCw { centre } => (centre, true),
            MoveKind::ArcCcw { centre } => (centre, false),
        };
        let start = self.position.ok_or_else(|| ExportError::ArcWithoutStart {
            op: op.id().clone(),
            index,
        })?;
        self.arc(start, mv.to, centre, clockwise, feed, rates)
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
    /// holds it there ([`Rates::holds_to_plunge`]).
    fn linear(&mut self, to: Point3, feed_mm_min: f64, rates: Rates) -> Result<(), ExportError> {
        let held = self.position.is_some_and(|from| {
            rates.holds_to_plunge(feed_mm_min, from.xy().distance_to(to.xy()), from.z - to.z)
        });
        self.linear_held(to, feed_mm_min, rates, held)
    }

    /// A straight cut to `to`; with `held`, its feed is lowered as far as
    /// the rounding of its numbers needs to keep it from falling faster
    /// than the plunge rate ([`Self::written_feed_mm_min`]).
    fn linear_held(
        &mut self,
        to: Point3,
        feed_mm_min: f64,
        rates: Rates,
        held: bool,
    ) -> Result<(), ExportError> {
        let m = self.transform.to_machine(to);
        let post = self.post;
        let level = self
            .position
            .is_some_and(|from| (from.z - to.z).abs() <= EPS_MM);
        let feed_mm_min = if held {
            self.written_feed_mm_min(
                MotionKind::Linear,
                m,
                ArcWords::default(),
                feed_mm_min,
                rates,
            )?
        } else if level {
            self.level_feed_mm_min(feed_mm_min, rates)?
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

    /// The feed, mm/min, to write for a cut held to the plunge rate from
    /// where the controller is to machine point `to` (an arc's with its
    /// centre words `arc`): `feed_mm_min`, unless at it, as written, the
    /// cut would fall faster than the plunge rate, by the sum S10 makes
    /// of the written numbers ([`safety::descent_rate_mm_min`]). Then the
    /// fastest feed in the last written place, rounded down, that does
    /// not (#325). Rounded coordinates can make a short cut steeper than
    /// the toolpath's, and a rounded feed faster; only here are both
    /// known. A cut that falls less than a unit in the last place may be
    /// written at the Z the controller holds, so it does not fall; it is
    /// then a level cut to S3, and is written as one
    /// ([`Self::written_level_feed_mm_min`], #467). With the start not
    /// known, S10 has nothing to judge.
    fn written_feed_mm_min(
        &self,
        kind: MotionKind,
        to: Point3,
        arc: ArcWords,
        feed_mm_min: f64,
        rates: Rates,
    ) -> Result<f64, ExportError> {
        let plunge_mm_min = rates.plunge_mm_min;
        let [Some(x), Some(y), Some(z)] = self.axes.each_ref().map(|a| a.as_ref().map(|e| e.value))
        else {
            return Ok(feed_mm_min);
        };
        // The line writes no Z word, as `motion_line` decides it.
        let z_text = self.number(to.z)?;
        if self.axes[2].as_ref().is_some_and(|e| e.text == z_text) {
            return self.written_level_feed_mm_min(feed_mm_min, rates);
        }
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
    fn level_feed_mm_min(&self, feed_mm_min: f64, rates: Rates) -> Result<f64, ExportError> {
        let (cut, p) = (rates.feed_mm_min, rates.plunge_mm_min);
        let slower = feed_mm_min < cut * (1.0 - HELD_TO_PLUNGE_RELATIVE);
        let plunge = (feed_mm_min - p).abs() <= p * HELD_TO_PLUNGE_RELATIVE;
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

    /// The feed, mm/min, of a cut the toolpath holds to the plunge rate
    /// `rates.plunge_mm_min` but that is written at the Z the controller holds
    /// (#467: a V-Carve chain's step falling 0.0007 mm, written to three
    /// decimals). As written it does not fall, so S3 takes it as a level
    /// cut, and S10 does not judge it; the next falling cut starts from
    /// the written Z and is judged with this one's fall. Its feed, scaled
    /// between the plunge rate and the cutting feed for a fall the program
    /// does not make, is not a rate of its tool, so it is written at the
    /// plunge rate: the fastest of the tool's rates no faster than the
    /// toolpath holds it. A cut at the cutting feed, or slower than the
    /// plunge rate, is written as any level cut
    /// ([`Self::level_feed_mm_min`]).
    fn written_level_feed_mm_min(
        &self,
        feed_mm_min: f64,
        rates: Rates,
    ) -> Result<f64, ExportError> {
        let plunge_mm_min = rates.plunge_mm_min;
        let scaled = feed_mm_min > plunge_mm_min * (1.0 + HELD_TO_PLUNGE_RELATIVE)
            && feed_mm_min < rates.feed_mm_min * (1.0 - HELD_TO_PLUNGE_RELATIVE);
        if scaled {
            return Ok(plunge_mm_min);
        }
        self.level_feed_mm_min(feed_mm_min, rates)
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
        rates: Rates,
    ) -> Result<usize, ExportError> {
        let Some(arc) = ArcGeometry::new(start, to, centre, clockwise) else {
            self.linear(to, feed_mm_min, rates)?;
            return Ok(0);
        };
        if !self.post.motion.arc_interpolation {
            self.flatten(&arc, feed_mm_min, rates)?;
            return Ok(0);
        }
        let mut flattened = 0;
        match self.post.motion.arc_format {
            ArcFormat::Ij => {
                if !self.arc_ij(&arc, feed_mm_min, rates)? {
                    self.flatten(&arc, feed_mm_min, rates)?;
                    flattened += 1;
                }
            }
            ArcFormat::R => {
                let parts = segment_count(arc.sweep_rad / MAX_R_ARC_SWEEP_RAD);
                for k in 0..parts {
                    let part = arc.part(k, parts);
                    if !self.arc_r(&part, feed_mm_min, rates)? {
                        self.flatten(&part, feed_mm_min, rates)?;
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
        if clockwise != self.transform.mirrors_xy() {
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
    fn arc_ij(
        &mut self,
        arc: &ArcGeometry,
        feed_mm_min: f64,
        rates: Rates,
    ) -> Result<bool, ExportError> {
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
        let feed_mm_min = self.arc_feed_mm_min(arc, command, end, words, feed_mm_min, rates)?;
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
    fn arc_r(
        &mut self,
        arc: &ArcGeometry,
        feed_mm_min: f64,
        rates: Rates,
    ) -> Result<bool, ExportError> {
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
        let feed_mm_min = self.arc_feed_mm_min(arc, command, end, words, feed_mm_min, rates)?;
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
        rates: Rates,
    ) -> Result<f64, ExportError> {
        if !rates.holds_arc_to_plunge(arc, feed_mm_min) {
            return if (arc.start.z - arc.end.z).abs() <= EPS_MM {
                self.level_feed_mm_min(feed_mm_min, rates)
            } else {
                Ok(feed_mm_min)
            };
        }
        let kind = if command == self.post.motion.cw_arc_command {
            MotionKind::ArcCw
        } else {
            MotionKind::ArcCcw
        };
        self.written_feed_mm_min(kind, end, words, feed_mm_min, rates)
    }

    /// `arc` as straight moves within the project's flatten tolerance.
    fn flatten(
        &mut self,
        arc: &ArcGeometry,
        feed_mm_min: f64,
        rates: Rates,
    ) -> Result<(), ExportError> {
        let tolerance = self.flatten_tolerance_mm;
        let max_step_rad = if tolerance >= arc.radius_mm {
            FRAC_PI_2
        } else {
            (2.0 * (1.0 - tolerance / arc.radius_mm).acos()).min(FRAC_PI_2)
        };
        let n = segment_count(arc.sweep_rad / max_step_rad);
        let held = rates.holds_arc_to_plunge(arc, feed_mm_min);
        for k in 1..=n {
            #[allow(
                clippy::cast_precision_loss,
                reason = "k <= n <= MAX_ARC_SEGMENTS, exact in f64"
            )]
            let fraction = k as f64 / n as f64;
            self.linear_held(arc.point_at(fraction), feed_mm_min, rates, held)?;
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
mod tests;
