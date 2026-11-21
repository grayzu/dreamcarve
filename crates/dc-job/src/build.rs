// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pipeline from a project to its programs, without I/O
//! (`docs/specs/phase-4/design.md`, "Data model → dc-job"): what
//! `dreamcarve export` and `check` compute, moved from
//! `crates/dreamcarve/src/export.rs` with its findings and bytes unchanged.
//!
//! [`build`] is the whole pipeline: it validates the project it is given
//! (rather than trusting the problems found at load, so the checks that
//! depend on the post-processor see the one a caller swapped in),
//! generates every enabled operation's toolpath with `dc-cam`, runs the
//! operation-order and placed-tab checks, and emits the programs with
//! `dc-gcode`, collecting every error and warning on the way. Nothing is
//! written: the caller does that.
//!
//! The app runs the same pipeline in two halves, so one edit regenerates
//! one operation: [`validation`], then [`generate_operation`] for each
//! operation, then [`finish`] once every operation has its runs. Their
//! findings and programs, in that order, equal [`build`]'s.
//!
//! Every warning carries its stable `snake_case` code (functional spec,
//! capability 25) and a [`Tier`]; errors carry a message alone. A finding
//! about one value of the file names its `field` as validation does, so a
//! shell can locate it (`coaster.dcv:9:21`, #76). After generation,
//! [`dc_cam::order::check`] adds the operation-order warnings and
//! [`dc_cam::placed_tabs::check`] the tabs no Profile cuts round (#105).

use dc_cam::{
    CamError, RunLabel, ShapeRef, ToolRun, Toolpath, Warning, generate_bore, generate_drill,
    generate_engrave, generate_inlay_board, generate_photo, generate_pocket, generate_profile,
    generate_vcarve, order, placed_tabs,
};
use dc_gcode::parse::{CycleModel, parse_program_with};
use dc_gcode::{EmitOp, Estimate, ExportWarning, Extents, Program, ProgramSplit, emit_programs};
use dc_project::{
    Board, Operation, OperationId, Project, Severity, ToolChangeMode, ToolId, validate,
};

use crate::names::program_names;
use crate::summary::{
    EstimateConfig, Estimates, before_you_start, by_board, describe_export_warning, edge_extents,
    largest_tool_radius_mm, tool_radius_mm,
};

/// How much a finding matters, as a shell presents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// No program can be written until it is fixed.
    Error,
    /// The program is safe, but the user should know.
    Warning,
    /// Advice about the job's setup, not about its cuts:
    /// `tool_change_avoidable` and `unused_tool`.
    Hint,
}

impl Tier {
    /// The tier of a warning with `code`: a hint for
    /// `tool_change_avoidable` and `unused_tool`, a warning otherwise.
    #[must_use]
    pub fn of_warning_code(code: &str) -> Self {
        match code {
            "tool_change_avoidable" | "unused_tool" => Self::Hint,
            _ => Self::Warning,
        }
    }
}

/// One error or warning.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// The field it is about, as validation names it
    /// (`tools[t1].feed_mm_min`), when it is about one.
    pub field: Option<String>,
    /// The finding's stable `snake_case` code, printed before the
    /// message. Every warning has one; errors print their message alone.
    pub code: Option<String>,
    /// What is wrong.
    pub message: String,
    /// How much it matters.
    pub tier: Tier,
    /// The operation it is about, when it is about one.
    pub op: Option<OperationId>,
    /// The shape it is about, when it is about one.
    pub shape: Option<ShapeRef>,
}

impl Finding {
    /// An error, with no code.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            field: None,
            code: None,
            message: message.into(),
            tier: Tier::Error,
            op: None,
            shape: None,
        }
    }

    /// A warning with code `code`, whose tier the code decides.
    #[must_use]
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            field: None,
            code: Some(code.to_owned()),
            message: message.into(),
            tier: Tier::of_warning_code(code),
            op: None,
            shape: None,
        }
    }

    /// The finding located at `field`.
    #[must_use]
    pub fn at_field(self, field: Option<String>) -> Self {
        Self { field, ..self }
    }

    /// The finding about operation `op`.
    #[must_use]
    pub fn for_op(self, op: Option<OperationId>) -> Self {
        Self { op, ..self }
    }

    /// The finding about `shape`.
    #[must_use]
    pub fn on_shape(self, shape: Option<ShapeRef>) -> Self {
        Self { shape, ..self }
    }
}

/// What the pipeline found, in the order found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Findings {
    /// Error-class findings; any one means no program.
    pub errors: Vec<Finding>,
    /// Findings that still allow a safe program, hints among them.
    pub warnings: Vec<Finding>,
}

impl Findings {
    /// Adds `finding` to the errors or the warnings, by its tier.
    pub fn push(&mut self, finding: Finding) {
        match finding.tier {
            Tier::Error => self.errors.push(finding),
            Tier::Warning | Tier::Hint => self.warnings.push(finding),
        }
    }

    /// Appends `other`'s findings after this one's.
    pub fn extend(&mut self, other: Self) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }

    /// Whether any finding is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// The pipeline's result: the programs when there were no errors.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The emitted programs, in the order they are run: one per tool run
    /// or one in all (#95). Empty when any error was found.
    pub programs: Vec<Program>,
    /// Errors and warnings.
    pub findings: Findings,
    /// Every enabled operation's generated toolpath, with its tool, when
    /// every one of them generated, even if emitting the programs then
    /// failed; `None` when one did not. What a preview draws.
    pub generated: Option<Vec<EmitOp>>,
}

/// Validates `project`, generates every enabled operation's toolpath, and
/// emits the programs, collecting every error and warning on the way.
/// `split` is how a multi-tool project is divided into programs (the
/// project's mode, or an `export` flag); `date` fills each program
/// header's `{date}`. Both boards are built (#498): [`build_board`] builds
/// one.
#[must_use]
pub fn build(project: &Project, split: ProgramSplit, date: &str) -> Built {
    build_board(project, split, date, None)
}

/// [`build`], of the programs of `board` alone when it is given (#498,
/// `export --board`): the stock's (every operation but an Inlay's male),
/// or the male board's (every Inlay's male). A board with no programs is
/// an error.
#[must_use]
pub fn build_board(
    project: &Project,
    split: ProgramSplit,
    date: &str,
    board: Option<Board>,
) -> Built {
    let mut findings = validation(project);
    if findings.has_errors() {
        // Generators must not see a project validation rejects.
        return Built {
            programs: Vec::new(),
            findings,
            generated: None,
        };
    }

    let mut ops = Vec::new();
    for operation in &project.operations {
        // Disabled operations are already reported by validation.
        if !operation.enabled() {
            continue;
        }
        let generated = generate_on(project, operation, board);
        findings.extend(generated.findings);
        ops.extend(generated.runs);
    }
    // The order check runs on whatever generated, so `check` reports it
    // even when another operation failed; it never needs a failed one.
    findings.extend(project_checks(project, &ops));
    if let Some(board) = board
        && !findings.has_errors()
        && ops.is_empty()
    {
        findings.push(Finding::error(format!(
            "nothing is cut on the {} board{}",
            board.as_str(),
            match board {
                Board::Female => "",
                Board::Male => ": no enabled Inlay cuts its male (set its male_board.thickness_mm)",
            }
        )));
    }
    if findings.has_errors() {
        // An operation did not generate.
        return Built {
            programs: Vec::new(),
            findings,
            generated: None,
        };
    }

    let programs = emit(project, &ops, split, date, &mut findings);
    Built {
        programs,
        findings,
        generated: Some(ops),
    }
}

/// Validation's problems as findings, in validation's order: the first
/// step of [`build`], which the app runs on every snapshot.
#[must_use]
pub fn validation(project: &Project) -> Findings {
    let mut findings = Findings::default();
    for problem in validate(project) {
        let field = problem.field();
        let op = field.as_deref().and_then(op_in_field);
        let finding = match problem.severity() {
            Severity::Error => Finding::error(problem.to_string()),
            Severity::Warning => Finding::warning(problem.code(), problem.to_string()),
        };
        findings.push(finding.at_field(field).for_op(op));
    }
    findings
}

/// The operation a field path names, `operations[<id>]…`.
fn op_in_field(field: &str) -> Option<OperationId> {
    let (id, _) = field.strip_prefix("operations[")?.split_once(']')?;
    Some(OperationId::new(id))
}

/// One operation's generated runs and what generating them found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Generated {
    /// The operation's tool runs, with their tools, in cut order: one for
    /// most operations, a carve's clearing passes before its own (#406), an
    /// Inlay's on each board it cuts (#498).
    pub runs: Vec<EmitOp>,
    /// What generating found: an error when the operation cannot be cut,
    /// else its warnings, each naming the operation as `check` prints it.
    pub findings: Findings,
}

/// The per-operation half of [`build`], for the app's incremental
/// generation: the runs of operation `op` of `project`, on both boards for
/// an Inlay, with what generating them found. Validation must have passed
/// first ([`validation`]): generators never see a project it rejects. An
/// operation the project does not have is an error.
#[must_use]
pub fn generate_operation(project: &Project, op: &OperationId) -> Generated {
    match project.operations.iter().find(|o| o.id() == op) {
        Some(operation) => generate_on(project, operation, None),
        None => {
            let mut findings = Findings::default();
            findings.push(
                Finding::error(CamError::UnknownOperation { op: op.clone() }.to_string())
                    .for_op(Some(op.clone())),
            );
            Generated {
                runs: Vec::new(),
                findings,
            }
        }
    }
}

/// [`generate_operation`], of the runs on `board` alone when it is given
/// (#498): an operation that is no Inlay cuts the stock only.
fn generate_on(project: &Project, operation: &Operation, board: Option<Board>) -> Generated {
    let mut out = Generated::default();
    let id = operation.id();
    let Some(tool) = project.tools.iter().find(|t| &t.id == operation.tool()) else {
        out.findings.push(
            Finding::error(format!(
                "operation `{id}` uses tool `{}`, which is not in the project",
                operation.tool()
            ))
            .for_op(Some(id.clone())),
        );
        return out;
    };
    // Format 22 (#436): an Inlay's board is a carve, so its runs are a
    // V-Carve's: its clearing passes', then its V-bit's; #498: both
    // boards, each generated and reported on its own.
    if let Operation::Inlay(op) = operation {
        inlay_boards(project, operation, op, board, &mut out);
        return out;
    }
    // Every other operation cuts the stock.
    if board == Some(Board::Male) {
        return out;
    }
    let plain =
        |toolpath: Toolpath| vec![EmitOp::from((operation.clone(), tool.clone(), toolpath))];
    let generated = match operation {
        Operation::Profile(op) => generate_profile(project, op).map(|(t, w)| (plain(t), w)),
        Operation::Pocket(op) => generate_pocket(project, op).map(|(t, w)| (plain(t), w)),
        Operation::Engrave(op) => generate_engrave(project, op).map(|(t, w)| (plain(t), w)),
        Operation::Bore(op) => generate_bore(project, op).map(|(t, w)| (plain(t), w)),
        // The hole records go to the emitter, which writes them as
        // canned cycles when the project asks and the controller's
        // motion for them is the toolpath; a countersink's always as
        // plain moves, which S18 holds to its derived depth (#427).
        Operation::Drill(op) => generate_drill(project, op).map(|(drilled, w)| {
            (
                vec![EmitOp::drill(operation.clone(), tool.clone(), drilled)],
                w,
            )
        }),
        // Format 20: one tool run per clearing pass, then the V-bit's
        // (`docs/specs/phase-3/design.md`, "Clearing passes", step 4);
        // each is written with its own tool.
        Operation::VCarve(op) => generate_vcarve(project, op).and_then(|(runs, w)| {
            runs.into_iter()
                .map(|run| run_op(project, operation, run))
                .collect::<Result<Vec<_>, _>>()
                .map(|runs| (runs, w))
        }),
        // Format 27 (#646): one run, its own, on the stock.
        Operation::PhotoVCarve(op) => generate_photo(project, op).map(|(t, w)| (plain(t), w)),
        // Generated above, board by board.
        Operation::Inlay(_) => return out,
    };
    match generated {
        Ok((runs, warnings)) => {
            // Validation reports `clearing_not_descending` (#406), as it
            // reports `start_depth_uncleared` (#409) and
            // `inlay_plug_base_thin` (#434): once.
            let warnings = warnings.iter().filter(|w| !reported_by_validation(w));
            for w in warnings {
                out.findings.push(
                    Finding::warning(w.code().as_str(), format!("operation `{id}`: {w}"))
                        .for_op(Some(id.clone()))
                        .on_shape(w.shape().cloned()),
                );
            }
            out.runs.extend(runs);
        }
        Err(e) => out
            .findings
            .push(Finding::error(e.to_string()).for_op(Some(id.clone()))),
    }
    out
}

/// Whether validation already reports `warning`, so generation does not
/// repeat it: `clearing_not_descending` (#406), `inlay_plug_base_thin`
/// (#434), a tapered ball nose's `inlay_glue_gap_below_ball` and
/// `inlay_plug_depth_below_ball` (#681), the drawn boundary's `inlay_margin_ignored`,
/// `inlay_boundary_empty` and `inlay_boundary_hole_ignored` (#603), and
/// `photo_lines_overlap` (#646).
fn reported_by_validation(warning: &Warning) -> bool {
    matches!(
        warning,
        Warning::ClearingNotDescending { .. }
            | Warning::InlayPlugBaseThin { .. }
            | Warning::InlayGlueGapBelowBall { .. }
            | Warning::InlayPlugDepthBelowBall { .. }
            | Warning::InlayMarginIgnored { .. }
            | Warning::InlayBoundaryEmpty { .. }
            | Warning::InlayBoundaryHoleIgnored { .. }
            | Warning::PhotoLinesOverlap { .. }
    )
}

/// Generates Inlay `op` (the enabled `operation`) of `project` on each
/// board (#498): both, or `only` that one, female first, appending its
/// runs to `out` (each tagged with its board) and its findings. A male
/// board's warning names the board; its `inlay_parts_merged`,
/// `inlay_hole_filled` and `inlay_rounded_for_bit` (#681), the female's
/// design mirrored, and any warning the
/// female already gave, are left out, as [`dc_cam::generate_inlay`] leaves
/// them. `inlay_male_board_unset` is located at the male board's
/// thickness.
fn inlay_boards(
    project: &Project,
    operation: &Operation,
    op: &dc_project::InlayOp,
    only: Option<Board>,
    out: &mut Generated,
) {
    let id = operation.id();
    let boards = match only {
        Some(board) => vec![board],
        None => vec![Board::Female, Board::Male],
    };
    let mut female_warnings: Vec<Warning> = Vec::new();
    let female_cut = boards.contains(&Board::Female);
    for board in boards {
        let generated = generate_inlay_board(project, op, board).and_then(|(runs, w)| {
            runs.into_iter()
                .map(|run| run_op(project, operation, run))
                .collect::<Result<Vec<_>, _>>()
                .map(|runs| (runs, w))
        });
        let (runs, warnings) = match generated {
            Ok(generated) => generated,
            Err(e) => {
                let message = match board {
                    Board::Female => e.to_string(),
                    Board::Male => format!("{e} (on the male board)"),
                };
                out.findings
                    .push(Finding::error(message).for_op(Some(id.clone())));
                continue;
            }
        };
        for w in &warnings {
            let repeats = board == Board::Male
                && (female_warnings.contains(w)
                    || (female_cut
                        && matches!(
                            w,
                            Warning::InlayPartsMerged { .. }
                                | Warning::InlayHoleFilled { .. }
                                | Warning::InlayRoundedForBit { .. }
                        )));
            if reported_by_validation(w) || repeats {
                continue;
            }
            let on = match board {
                Board::Female => "",
                Board::Male => " (male board)",
            };
            out.findings.push(
                Finding::warning(w.code().as_str(), format!("operation `{id}`{on}: {w}"))
                    .at_field(w.field())
                    .for_op(Some(id.clone()))
                    .on_shape(w.shape().cloned()),
            );
        }
        if board == Board::Female {
            female_warnings = warnings;
        }
        out.runs.extend(runs);
    }
}

/// The field, as validation names it, that a project-wide warning is
/// about, so it is printed at that value's position: the start depth of
/// a `start_depth_mismatch`, where `start_depth_uncleared` is located too
/// (a countersink's is in its cycle, format 21).
fn warning_field(project: &Project, warning: &Warning) -> Option<String> {
    match warning {
        Warning::StartDepthMismatch { op, .. } => {
            let countersink = project.operations.iter().any(|o| {
                matches!(o, Operation::Drill(drill) if &drill.id == op && drill.cycle.is_countersink())
            });
            let field = if countersink {
                "cycle.countersink.start_depth_mm"
            } else {
                "start_depth_mm"
            };
            Some(format!("operations[{op}].{field}"))
        }
        _ => None,
    }
}

/// The emitter's entry for tool run `run` of `operation`: written with the
/// run's own tool, as plain moves.
fn run_op(project: &Project, operation: &Operation, run: ToolRun) -> Result<EmitOp, CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == run.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: operation.id().clone(),
            tool: run.tool.clone(),
        })?;
    Ok(EmitOp {
        op: operation.clone(),
        tool: tool.clone(),
        toolpath: run.toolpath,
        holes: None,
        label: run.label,
        board: run.board,
    })
}

/// The project-level checks on every generated run: the operation-order
/// warnings ([`dc_cam::order::check`]) and the placed tabs no enabled
/// Profile cuts round ([`dc_cam::placed_tabs::check`], #105). Each takes
/// an operation's own run: a carve's clearing runs cut inside its targets
/// before it, as intended (design "Clearing passes", step 5), and the
/// tool-change hint counts them from the project. #498: they read the
/// stock; an Inlay's male is cut on its own board.
///
/// [`build`] runs them on whatever generated even when an operation
/// failed, and then emits nothing; [`finish`] runs them and emits. The
/// app calls them alone when an operation failed, so its Warnings panel
/// lists what `check` lists (acceptance 82).
#[must_use]
pub fn project_checks(project: &Project, ops: &[EmitOp]) -> Findings {
    let generated: Vec<(&Operation, &Toolpath)> = ops
        .iter()
        .filter(|entry| entry.label == RunLabel::Main && entry.board == Board::Female)
        .map(|entry| (&entry.op, &entry.toolpath))
        .collect();
    let mut findings = Findings::default();
    let warnings = order::check(project, &generated)
        .into_iter()
        .chain(placed_tabs::check(project, &generated));
    for w in warnings {
        let message = match w.op() {
            Some(op) => format!("operation `{op}`: {w}"),
            None => w.to_string(),
        };
        findings.push(
            Finding::warning(w.code().as_str(), message)
                .at_field(warning_field(project, &w))
                .for_op(w.op().cloned())
                .on_shape(w.shape().cloned()),
        );
    }
    findings
}

/// Emits the programs of `ops`, adding the export warnings, or the error,
/// to `findings`. Empty on failure.
fn emit(
    project: &Project,
    ops: &[EmitOp],
    split: ProgramSplit,
    date: &str,
    findings: &mut Findings,
) -> Vec<Program> {
    match emit_programs(project, ops, split, date) {
        Ok((programs, warnings)) => {
            for w in &warnings {
                findings.push(
                    Finding::warning(w.code(), describe_export_warning(w))
                        .for_op(export_warning_op(w)),
                );
            }
            programs
        }
        Err(e) => {
            findings.push(Finding::error(e.to_string()));
            Vec::new()
        }
    }
}

/// The operation an export warning is about.
fn export_warning_op(warning: &ExportWarning) -> Option<OperationId> {
    match warning {
        ExportWarning::DisabledOperationSkipped { op }
        | ExportWarning::ArcsFlattened { op, .. }
        | ExportWarning::CannedCycleExpanded { op, .. }
        | ExportWarning::ArcFitFallback { op, .. } => Some(op.clone()),
        ExportWarning::UnitsSetByController { .. } => None,
    }
}

/// What [`finish`] computes once every operation has its runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    /// The project-level warnings, then the export warnings, or the error
    /// emission failed with.
    pub findings: Findings,
    /// One row per program, in the order they are run; empty when any
    /// error was found.
    pub programs: Vec<ProgramSummary>,
    /// The `Estimated total time: …` line over every program, when there
    /// is more than one.
    pub total_line: Option<String>,
}

/// One program as the export summary describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramSummary {
    /// Its file name, as [`crate::program_names`] names it under the
    /// project's name.
    pub name: String,
    /// The program: its text, board, first tool and extents.
    pub program: Program,
    /// The operations it cuts, in cut order, each once.
    pub operations: Vec<OperationId>,
    /// Its tool runs, in the order they are cut (#95): one for a tool
    /// run's program, and one per tool fitted for one program with tool
    /// changes, which stops between them.
    pub runs: Vec<RunSummary>,
    /// Its extents widened in X and Y by the radius of its largest tool:
    /// where the cutter's edge reaches, which clamps must stay outside
    /// (#62).
    pub edge_extents: Extents,
    /// Its cycle-time estimate, when the post-processor could read it
    /// back (functional spec, capability 30).
    pub estimate: Option<Estimate>,
    /// The `Estimated time: …` line, with the user's `[estimate]` settings
    /// applied.
    pub estimate_line: String,
    /// What to do before starting it: which tool to fit and what to zero.
    pub before: String,
}

impl ProgramSummary {
    /// How many tools it cuts with: a tool that is fitted again later is
    /// counted once.
    #[must_use]
    pub fn tools(&self) -> usize {
        self.runs
            .iter()
            .enumerate()
            .filter(|(k, run)| !self.runs.iter().take(*k).any(|r| r.tool == run.tool))
            .count()
    }

    /// How many times it stops for the next tool: once between two tool
    /// runs.
    #[must_use]
    pub fn tool_changes(&self) -> usize {
        self.runs.len().saturating_sub(1)
    }
}

/// One tool run of a program (#95): what one tool cuts before the program
/// ends or stops for the next. A shell lists a program's runs so one
/// program with tool changes shows every tool it uses, not only the one it
/// starts with (functional spec, capability 77).
#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    /// The tool's id, to find it in the project's tool list.
    pub tool: ToolId,
    /// Its number, `T<n>`, as the program's tool change writes it: the
    /// tool's `number` when set, otherwise its 1-based place in the
    /// project's tool list (capability 21a).
    pub number: u32,
    /// Its name, as the project has it.
    pub name: String,
    /// The operations the run cuts, in cut order, each once. A carve
    /// whose clearing pass uses another tool is in both tools' runs.
    pub operations: Vec<OperationId>,
    /// The run's share of the program's cycle-time estimate, when the
    /// program has one (functional spec, capability 30): what the
    /// estimator reads between the tool change that starts the run and
    /// the next, so the runs' shares add up to the program's estimate.
    /// The tool change is the stop of the run it starts and has no time.
    pub estimate: Option<Estimate>,
}

/// The project-level half of [`build`], once every enabled operation has
/// its runs (`generated`, in operation order): the order and tab checks,
/// then emission for the programs and their export warnings, then each
/// program's name, operations, tool runs, extents, edge extents and
/// estimate, with
/// `estimate` (the user's `[estimate]` settings) applied to the printed
/// lines. Pure: `date` is the caller's.
#[must_use]
pub fn finish(
    project: &Project,
    generated: &[EmitOp],
    split: ProgramSplit,
    date: &str,
    estimate: &EstimateConfig,
) -> Finished {
    let mut findings = project_checks(project, generated);
    let programs = emit(project, generated, split, date, &mut findings);
    let (programs, total_line) = summarize(project, generated, programs, split, estimate);
    Finished {
        findings,
        programs,
        total_line,
    }
}

/// The rows of `programs`, emitted from `generated` (every enabled
/// operation's runs, or one board's) with `split`: each program's name
/// under the project's name, operations, tool runs with each one's share
/// of its estimate, edge extents, estimate with the user's `estimate`
/// settings and the step before it, as [`finish`] lists them; and the `Estimated total time: …` line when there are several.
/// The second half of [`finish`], for a shell that built the programs
/// with [`build`] itself (the app's export, which builds afresh).
#[must_use]
pub fn summarize(
    project: &Project,
    generated: &[EmitOp],
    programs: Vec<Program>,
    split: ProgramSplit,
    estimate: &EstimateConfig,
) -> (Vec<ProgramSummary>, Option<String>) {
    let estimates = Estimates::new(project, &programs, *estimate);
    let names = program_names(project, &project.name, &programs);
    let mut groups = program_runs(project, generated, split).into_iter();
    let boards = by_board(project);
    let single = programs.len() == 1 && !boards;
    let mut board = None;
    let mut k_in_board = 0;
    let programs = programs
        .into_iter()
        .enumerate()
        .map(|(k, program)| {
            // #498: each board is its own setup, its programs counted afresh.
            if board != Some(program.board) {
                board = Some(program.board);
                k_in_board = 0;
            }
            let radius_mm = if single {
                largest_tool_radius_mm(project)
            } else {
                tool_radius_mm(project, &program)
            };
            let mut runs = groups.next().unwrap_or_default();
            let shares = run_estimates(project, &program, estimates.estimate(k), runs.len());
            for (run, share) in runs.iter_mut().zip(shares) {
                run.estimate = share;
            }
            let mut operations: Vec<OperationId> = Vec::new();
            for id in runs.iter().flat_map(|run| &run.operations) {
                if !operations.contains(id) {
                    operations.push(id.clone());
                }
            }
            let summary = ProgramSummary {
                name: names.get(k).cloned().unwrap_or_default(),
                operations,
                runs,
                edge_extents: edge_extents(&program.extents, radius_mm),
                estimate: estimates.estimate(k),
                estimate_line: estimates.program_line(k),
                before: before_you_start(&program, k_in_board, single),
                program,
            };
            k_in_board += 1;
            summary
        })
        .collect();
    (programs, estimates.total_line())
}

/// The tool runs of each program, without their estimates, as
/// [`dc_gcode::emit_programs`] divides the runs: each board's entries in
/// order (#498), a tool run being the consecutive entries with one tool,
/// as one program with `M6` tool changes or one program per tool run
/// (#95).
fn program_runs(
    project: &Project,
    generated: &[EmitOp],
    split: ProgramSplit,
) -> Vec<Vec<RunSummary>> {
    let mode = match split {
        ProgramSplit::ProjectDefault => project.post_processor.tool_change_mode,
        ProgramSplit::OneProgram => ToolChangeMode::M6,
        ProgramSplit::ProgramPerTool => ToolChangeMode::SeparatePrograms,
    };
    let mut programs = Vec::new();
    for board in [Board::Female, Board::Male] {
        let on_board: Vec<&EmitOp> = generated.iter().filter(|e| e.board == board).collect();
        if on_board.is_empty() {
            continue;
        }
        let runs: Vec<RunSummary> = on_board
            .chunk_by(|a, b| a.tool.id == b.tool.id)
            .filter_map(|entries| {
                let tool = &entries.first()?.tool;
                let mut operations: Vec<OperationId> = Vec::new();
                for entry in entries {
                    if !operations.contains(entry.op.id()) {
                        operations.push(entry.op.id().clone());
                    }
                }
                Some(RunSummary {
                    tool: tool.id.clone(),
                    number: tool_number(project, tool),
                    name: tool.name.clone(),
                    operations,
                    estimate: None,
                })
            })
            .collect();
        match mode {
            ToolChangeMode::M6 => programs.push(runs),
            ToolChangeMode::SeparatePrograms => {
                programs.extend(runs.into_iter().map(|run| vec![run]));
            }
        }
    }
    programs
}

/// `tool`'s `T` number as a program writes it (capability 21a): its
/// `number` when set, otherwise its 1-based place in `project`'s tool
/// list; 0 for a tool the project does not have, which no program is
/// emitted with.
fn tool_number(project: &Project, tool: &dc_project::Tool) -> u32 {
    tool.number.unwrap_or_else(|| {
        project
            .tools
            .iter()
            .position(|t| t.id == tool.id)
            .and_then(|index| u32::try_from(index + 1).ok())
            .unwrap_or(0)
    })
}

/// Each tool run's share of `program`'s estimate `total`, for its `runs`
/// tool runs in order. A program of one run has the whole of it. One
/// program with tool changes is read by the estimator up to each `M6`
/// line in turn, the way the whole program was read for `total`, and a
/// run's share is what the reading gained since the tool change before
/// it: the shares add up to `total`, and the `M6` stop is counted in the
/// run it starts. `None` for every run when the program has no estimate,
/// or does not stop for a tool exactly once between its runs.
fn run_estimates(
    project: &Project,
    program: &Program,
    total: Option<Estimate>,
    runs: usize,
) -> Vec<Option<Estimate>> {
    if runs < 2 {
        return vec![total; runs];
    }
    shares(project, program, total, runs).unwrap_or_else(|| vec![None; runs])
}

/// [`run_estimates`] for a program of two runs or more, when it can be
/// divided.
fn shares(
    project: &Project,
    program: &Program,
    total: Option<Estimate>,
    runs: usize,
) -> Option<Vec<Option<Estimate>>> {
    let total = total?;
    let post = project.post_processor.resolve().ok()?;
    // As the estimator reads the program.
    let model = CycleModel {
        units: program.extents.units,
        ..CycleModel::of(&post)
    };
    let changes: Vec<usize> = parse_program_with(&program.text, &model)
        .ok()?
        .iter()
        .filter(|block| block.has('M', TOOL_CHANGE_CODE))
        .map(|block| block.line)
        .collect();
    if changes.len() + 1 != runs {
        return None;
    }
    let mut out = Vec::with_capacity(runs);
    let mut before = Estimate::default();
    for line in changes {
        // The program up to the tool change, which starts the next run.
        let read = Program {
            text: program
                .text
                .split_inclusive('\n')
                .take(line.saturating_sub(1))
                .collect(),
            extension: program.extension.clone(),
            pauses: Vec::new(),
            first_tool: program.first_tool.clone(),
            extents: program.extents,
            machine_lift: program.machine_lift.clone(),
            board: program.board,
        };
        let upto = dc_gcode::estimate(&read, &post).ok()?;
        out.push(Some(gained(&upto, &before)));
        before = upto;
    }
    out.push(Some(gained(&total, &before)));
    Some(out)
}

/// The `M` code of a tool change, `M6`.
const TOOL_CHANGE_CODE: f64 = 6.0;

/// What the estimate `after` has that `before`, an estimate of the start
/// of the same program, has not.
fn gained(after: &Estimate, before: &Estimate) -> Estimate {
    Estimate {
        feed_s: after.feed_s - before.feed_s,
        dwell_s: after.dwell_s - before.dwell_s,
        feed_mm: after.feed_mm - before.feed_mm,
        rapid_mm: after.rapid_mm - before.rapid_mm,
        stops: after.stops.saturating_sub(before.stops),
    }
}

#[cfg(test)]
mod tests {
    use dc_project::test_support::{profile_mut, sample_project};
    use dc_project::{OperationId, ToolId};

    use super::*;

    /// The sample project with a second tool `t2` (3 mm) and a second
    /// Profile that cuts with it, the tools numbered `numbers`.
    fn two_tool_project(numbers: [Option<u32>; 2]) -> Project {
        let mut project = sample_project();
        let mut second = project.tools[0].clone();
        second.id = ToolId::new("t2");
        second.name = "3 mm".into();
        second.diameter_mm = 3.0;
        project.tools.push(second);
        let mut op = profile_mut(&mut project).clone();
        op.id = OperationId::new("o2");
        op.tool = ToolId::new("t2");
        project.operations.push(Operation::Profile(op));
        for (tool, number) in project.tools.iter_mut().zip(numbers) {
            tool.number = number;
        }
        project
    }

    /// #120: a tool run's program finds its tool by id, not by its `T`
    /// number, which once set is no place in the tool list.
    #[test]
    fn first_tool_radius_is_found_by_id_not_number() {
        for numbers in [[None, None], [Some(9), Some(4)], [Some(2), Some(1)]] {
            let project = two_tool_project(numbers);
            let Built {
                programs, findings, ..
            } = build(&project, ProgramSplit::ProgramPerTool, "2026-01-01");
            assert!(findings.errors.is_empty(), "{numbers:?}: {findings:?}");
            let radii: Vec<f64> = programs
                .iter()
                .map(|p| tool_radius_mm(&project, p))
                .collect();
            assert_eq!(radii, [3.175, 1.5], "{numbers:?}");
        }
    }

    /// `project`'s programs with `split`, as rows.
    fn rows(project: &Project, split: ProgramSplit) -> Vec<ProgramSummary> {
        let Built {
            programs,
            findings,
            generated,
        } = build(project, split, "2026-01-01");
        assert!(findings.errors.is_empty(), "{split:?}: {findings:?}");
        let generated = generated.expect("every operation generated");
        summarize(
            project,
            &generated,
            programs,
            split,
            &EstimateConfig::default(),
        )
        .0
    }

    /// Capability 77 (the maintainer's smoke test, 2026-11-20): one
    /// program for two tools lists both tool runs, in the order they are
    /// cut, each with its tool, its operations and its share of the
    /// program's estimate. The shares come from the estimator reading the
    /// program up to each tool change, so they add up to the program's
    /// own estimate, and each tool's cutting and dwell times are those of
    /// its own program when the project is written one program per tool.
    /// The tool change itself has no time: it is the second run's stop.
    #[test]
    fn one_program_lists_its_tool_runs_and_their_times_sum_to_its_own() {
        use dc_geometry::assert_approx_eq;

        for numbers in [[None, None], [Some(9), Some(4)]] {
            let mut project = two_tool_project(numbers);
            project.post_processor =
                dc_project::PostProcessorRef::latest("grblhal").expect("grblHAL is bundled");
            let one = rows(&project, ProgramSplit::OneProgram);
            let [program] = one.as_slice() else {
                panic!("{numbers:?}: one program, not {}", one.len());
            };
            let tools: Vec<(&str, u32, &str)> = program
                .runs
                .iter()
                .map(|run| (run.tool.as_str(), run.number, run.name.as_str()))
                .collect();
            let [first, second] = numbers.map(|n| n.unwrap_or(0));
            assert_eq!(
                tools,
                [
                    ("t1", first.max(1), "6.35 mm upcut"),
                    ("t2", if second == 0 { 2 } else { second }, "3 mm"),
                ],
                "{numbers:?}"
            );
            assert_eq!(
                program.runs[0].number, program.program.first_tool.number,
                "{numbers:?}: the first run's tool is the one the program starts with"
            );
            assert!(
                program
                    .program
                    .text
                    .lines()
                    .any(|line| line.starts_with(&format!("T{} M6", program.runs[1].number))),
                "{numbers:?}: the second run starts at its tool change"
            );
            let operations: Vec<Vec<&str>> = program
                .runs
                .iter()
                .map(|run| run.operations.iter().map(OperationId::as_str).collect())
                .collect();
            assert_eq!(operations, [["o1"], ["o2"]], "{numbers:?}");
            assert_eq!(
                program.operations,
                [OperationId::new("o1"), OperationId::new("o2")]
            );

            let total = program.estimate.expect("the program estimates");
            let shares: Vec<Estimate> = program
                .runs
                .iter()
                .map(|run| run.estimate.expect("each run estimates"))
                .collect();
            let sum = shares
                .iter()
                .fold(Estimate::default(), |sum, share| sum + *share);
            assert_approx_eq!(sum.feed_s, total.feed_s, 1e-9);
            assert_approx_eq!(sum.dwell_s, total.dwell_s, 1e-9);
            assert_approx_eq!(sum.feed_mm, total.feed_mm, 1e-9);
            assert_approx_eq!(sum.rapid_mm, total.rapid_mm, 1e-9);
            assert_eq!(sum.stops, total.stops, "{numbers:?}");
            assert_eq!(
                (shares[0].stops, shares[1].stops),
                (0, 1),
                "{numbers:?}: the tool change is the second run's stop"
            );

            // The same estimator, the same cuts: one program per tool.
            let per_tool = rows(&project, ProgramSplit::ProgramPerTool);
            assert_eq!(per_tool.len(), 2, "{numbers:?}");
            for (k, (share, own)) in shares.iter().zip(&per_tool).enumerate() {
                let own_estimate = own.estimate.expect("each program estimates");
                assert!(share.feed_s > 0.0, "{numbers:?}: run {k} cuts");
                assert_approx_eq!(share.feed_s, own_estimate.feed_s, 1e-6);
                assert_approx_eq!(share.dwell_s, own_estimate.dwell_s, 1e-6);
                // A tool run's program is one run: its own.
                let [run] = own.runs.as_slice() else {
                    panic!("{numbers:?}: program {k} has {} runs", own.runs.len());
                };
                assert_eq!(run.tool, own.program.first_tool.id);
                assert_eq!(run.number, own.program.first_tool.number);
                assert_eq!(run.operations, own.operations);
                assert_eq!(run.estimate, own.estimate);
                assert_eq!((own.tools(), own.tool_changes()), (1, 0));
            }
            assert_eq!((program.tools(), program.tool_changes()), (2, 1));
        }
    }

    /// A tool that is needed again later is a tool run of its own, and
    /// one more tool change, but not one more tool: a third Profile with
    /// the first tool makes three runs of two tools with two changes, and
    /// the three shares still add up to the program's estimate.
    #[test]
    fn a_tool_fitted_again_is_another_run_of_the_same_tool() {
        use dc_geometry::assert_approx_eq;

        let mut project = two_tool_project([None, None]);
        project.post_processor =
            dc_project::PostProcessorRef::latest("grblhal").expect("grblHAL is bundled");
        let mut again = profile_mut(&mut project).clone();
        again.id = OperationId::new("o3");
        project.operations.push(Operation::Profile(again));
        let one = rows(&project, ProgramSplit::OneProgram);
        let [program] = one.as_slice() else {
            panic!("one program, not {}", one.len());
        };
        let tools: Vec<&str> = program.runs.iter().map(|run| run.tool.as_str()).collect();
        assert_eq!(tools, ["t1", "t2", "t1"]);
        assert_eq!((program.tools(), program.tool_changes()), (2, 2));
        let total = program.estimate.expect("the program estimates");
        let sum = program
            .runs
            .iter()
            .map(|run| run.estimate.expect("each run estimates"))
            .fold(Estimate::default(), |sum, share| sum + share);
        assert_approx_eq!(sum.feed_s, total.feed_s, 1e-9);
        assert_approx_eq!(sum.dwell_s, total.dwell_s, 1e-9);
        assert_eq!(sum.stops, total.stops);
        assert_eq!(total.stops, 2, "the two tool changes");
    }

    /// The sample project with a V-Carve `o6` of its 50 x 30 mm rectangle
    /// with the 90 degree V-bit `t2` at a 2 mm flat depth, clearing with
    /// `passes` (format 20, #406), and a 3 mm end mill `t3`.
    fn carve_project(passes: &[&str]) -> Project {
        let mut project = sample_project();
        project
            .tools
            .push(dc_project::test_support::tool_v_bit_90());
        let mut small = project.tools[0].clone();
        small.id = ToolId::new("t3");
        small.name = "3 mm end mill".into();
        small.diameter_mm = 3.0;
        project.tools.push(small);
        let carve = dc_project::VCarveOp {
            flat_depth_mm: Some(2.0),
            clearing: passes
                .iter()
                .map(|tool| dc_project::ClearingPass {
                    tool: ToolId::new(*tool),
                    strategy: dc_project::ClearanceStrategy::Offset,
                    direction: dc_project::CutDirection::Climb,
                    entry: dc_project::Entry::Plunge,
                })
                .collect(),
            ..dc_project::test_support::sample_v_carve()
        };
        project.operations = vec![Operation::VCarve(carve)];
        project
    }

    /// #646: a Photo V-Carve generates one run, its own, on the stock
    /// with its V-bit, and exports a program of its own; its summary line
    /// gives the picture's footprint, the groove count and the full-depth
    /// width. A spacing below 1 is reported once, by validation.
    #[test]
    fn photo_v_carve_generates_one_run() {
        let project = dc_project::test_support::photo_project();
        assert!(!validation(&project).has_errors());
        let built = build(&project, ProgramSplit::ProjectDefault, "2026-01-01");
        assert!(!built.findings.has_errors(), "{:?}", built.findings);
        assert_eq!(built.programs.len(), 2, "the Profile's and the Photo's");
        let generated = generate_operation(&project, &OperationId::new("o8"));
        assert!(!generated.findings.has_errors(), "{:?}", generated.findings);
        let [run] = generated.runs.as_slice() else {
            panic!("one run: {:?}", generated.runs.len());
        };
        assert_eq!(run.label, RunLabel::Main);
        assert_eq!(run.board, Board::Female);
        assert_eq!(run.tool.id.as_str(), "t2");

        let lines = crate::summary::describe_carves(
            &project,
            built
                .generated
                .as_deref()
                .expect("every operation generated"),
        );
        assert_eq!(
            lines,
            [
                "Photo V-Carve o8 `Photo V-Carve`: picture `ramp` (b1), 40.000 mm by 20.000 mm at (15.000 mm, 15.000 mm), 11 grooves 3.000 mm wide at the full depth of 1.500 mm."
            ]
        );
        // #648: export's operation line names the picture, not "0
        // target(s)", and the full depth.
        let lines = crate::summary::describe_operations(
            &project,
            built
                .generated
                .as_deref()
                .expect("every operation generated"),
        );
        let photo = lines
            .iter()
            .find(|l| l.starts_with("o8 "))
            .unwrap_or_else(|| panic!("a line for o8: {lines:?}"));
        assert!(
            photo.starts_with("o8 `Photo V-Carve`: tool `t2` (")
                && photo.ends_with("), picture `ramp` (b1), 1.500 mm deep at most"),
            "{photo}"
        );

        let mut overlapping = project;
        if let Some(Operation::PhotoVCarve(photo)) = overlapping.operations.last_mut() {
            photo.line_spacing_fraction = 0.9;
        }
        let built = build(&overlapping, ProgramSplit::ProjectDefault, "2026-01-01");
        assert!(!built.findings.has_errors(), "{:?}", built.findings);
        let overlaps = built
            .findings
            .warnings
            .iter()
            .filter(|f| f.code.as_deref() == Some("photo_lines_overlap"))
            .count();
        assert_eq!(overlaps, 1, "{:?}", built.findings.warnings);
    }

    /// #406: the summary names a carve's clearing run `o6 (clearing t1)`
    /// with its own tool and depth, before the carve's own line.
    #[test]
    fn summary_names_clearing_runs() {
        let project = carve_project(&["t1"]);
        let Built { generated, .. } = build(&project, ProgramSplit::ProjectDefault, "2026-01-01");
        let generated = generated.expect("every operation generated");
        let lines = crate::summary::describe_operations(&project, &generated);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with(
                "o6 (clearing t1) `V-Carve`: tool `t1` (6.35 mm upcut), 1 target(s), 2."
            ) && lines[0].ends_with(" mm deep"),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("o6 `V-Carve`: tool `t2` (90° V-bit), 1 target(s), cut 2."),
            "{lines:?}"
        );
    }

    /// #406: a wider clearing pass after a narrower one is reported once,
    /// by validation, not again by the generator.
    #[test]
    fn clearing_not_descending_is_reported_once() {
        let project = carve_project(&["t3", "t1"]);
        let Built { findings, .. } = build(&project, ProgramSplit::ProjectDefault, "2026-01-01");
        let count = findings
            .warnings
            .iter()
            .filter(|w| w.code.as_deref() == Some("clearing_not_descending"))
            .count();
        assert_eq!(count, 1, "{findings:?}");
    }

    /// Format 21: `start_depth_mismatch` is printed at the start depth,
    /// which for a countersink is in its cycle.
    #[test]
    fn start_depth_mismatch_is_located_at_the_operations_start_depth() {
        let mut project = sample_project();
        let mut sink = dc_project::test_support::sample_drill();
        sink.cycle = dc_project::DrillCycle::Countersink {
            diameter_mm: 6.0,
            start_depth_mm: 2.0,
            start_plane_cleared_externally: false,
        };
        project.operations.push(Operation::Drill(sink));
        let mismatch = |op: &str| Warning::StartDepthMismatch {
            op: OperationId::new(op),
            cleared_by: OperationId::new("o2"),
            floor: dc_cam::FloorKind::Pocket,
            footprint: dc_project::StartFootprint::Targets,
            start_depth_mm: 2.0,
            floor_depth_mm: Some(1.0),
        };
        assert_eq!(
            warning_field(&project, &mismatch("o3")).as_deref(),
            Some("operations[o3].cycle.countersink.start_depth_mm")
        );
        assert_eq!(
            warning_field(&project, &mismatch("o6")).as_deref(),
            Some("operations[o6].start_depth_mm")
        );
    }

    /// The operation a validation field names: `operations[o1].depth_mm`
    /// is about `o1`; a tool's or the stock's field is about no operation.
    #[test]
    fn validation_findings_name_their_operation() {
        assert_eq!(
            op_in_field("operations[o1].depth_mm"),
            Some(OperationId::new("o1"))
        );
        assert_eq!(op_in_field("tools[t1].feed_mm_min"), None);
        assert_eq!(op_in_field("stock.thickness_mm"), None);
    }
}
