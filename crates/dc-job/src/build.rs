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
    generate_engrave, generate_inlay_board, generate_pocket, generate_profile, generate_vcarve,
    order, placed_tabs,
};
use dc_gcode::{EmitOp, Estimate, ExportWarning, Extents, Program, ProgramSplit, emit_programs};
use dc_project::{Board, Operation, OperationId, Project, Severity, ToolChangeMode, validate};

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
/// (#434) and the drawn boundary's `inlay_margin_ignored`,
/// `inlay_boundary_empty` and `inlay_boundary_hole_ignored` (#603).
fn reported_by_validation(warning: &Warning) -> bool {
    matches!(
        warning,
        Warning::ClearingNotDescending { .. }
            | Warning::InlayPlugBaseThin { .. }
            | Warning::InlayMarginIgnored { .. }
            | Warning::InlayBoundaryEmpty { .. }
            | Warning::InlayBoundaryHoleIgnored { .. }
    )
}

/// Generates Inlay `op` (the enabled `operation`) of `project` on each
/// board (#498): both, or `only` that one, female first, appending its
/// runs to `out` (each tagged with its board) and its findings. A male
/// board's warning names the board; its `inlay_parts_merged` and
/// `inlay_hole_filled`, the female's design mirrored, and any warning the
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
                            Warning::InlayPartsMerged { .. } | Warning::InlayHoleFilled { .. }
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

/// The project-level half of [`build`], once every enabled operation has
/// its runs (`generated`, in operation order): the order and tab checks,
/// then emission for the programs and their export warnings, then each
/// program's name, operations, extents, edge extents and estimate, with
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
/// under the project's name, operations, edge extents, estimate with the
/// user's `estimate` settings and the step before it, as [`finish`] lists
/// them; and the `Estimated total time: …` line when there are several.
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
    let groups = program_operations(project, generated, split);
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
            let summary = ProgramSummary {
                name: names.get(k).cloned().unwrap_or_default(),
                operations: groups.get(k).cloned().unwrap_or_default(),
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

/// The operations of each program, as [`dc_gcode::emit_programs`] divides
/// the runs: each board's runs in order (#498), as one program with `M6`
/// tool changes or one per run of consecutive entries with one tool
/// (#95).
fn program_operations(
    project: &Project,
    generated: &[EmitOp],
    split: ProgramSplit,
) -> Vec<Vec<OperationId>> {
    let mode = match split {
        ProgramSplit::ProjectDefault => project.post_processor.tool_change_mode,
        ProgramSplit::OneProgram => ToolChangeMode::M6,
        ProgramSplit::ProgramPerTool => ToolChangeMode::SeparatePrograms,
    };
    let mut groups = Vec::new();
    for board in [Board::Female, Board::Male] {
        let on_board: Vec<&EmitOp> = generated.iter().filter(|e| e.board == board).collect();
        if on_board.is_empty() {
            continue;
        }
        let runs: Vec<&[&EmitOp]> = match mode {
            ToolChangeMode::M6 => vec![on_board.as_slice()],
            ToolChangeMode::SeparatePrograms => {
                on_board.chunk_by(|a, b| a.tool.id == b.tool.id).collect()
            }
        };
        for run in runs {
            let mut ops: Vec<OperationId> = Vec::new();
            for entry in run {
                if !ops.contains(entry.op.id()) {
                    ops.push(entry.op.id().clone());
                }
            }
            groups.push(ops);
        }
    }
    groups
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

    /// #644: a Photo V-Carve is in the project file (format 27) before
    /// its generator is built (#646). Until then a project with an
    /// enabled one validates, and building it is one error that names the
    /// operation: no program, and no toolpath for a preview, so nothing
    /// that cuts nothing is ever written. The project's other operations
    /// export once it is disabled.
    #[test]
    fn photo_v_carve_is_not_generated_yet() {
        let project = dc_project::test_support::photo_project();
        assert!(!validation(&project).has_errors());
        let built = build(&project, ProgramSplit::ProjectDefault, "2026-01-01");
        assert!(built.programs.is_empty(), "{:?}", built.programs.len());
        assert_eq!(built.generated, None);
        let expected = dc_cam::CamError::PhotoNotBuilt {
            op: OperationId::new("o8"),
        }
        .to_string();
        let errors: Vec<(&str, Option<&str>)> = built
            .findings
            .errors
            .iter()
            .map(|f| (f.message.as_str(), f.op.as_ref().map(OperationId::as_str)))
            .collect();
        assert_eq!(errors, [(expected.as_str(), Some("o8"))]);

        let generated = generate_operation(&project, &OperationId::new("o8"));
        assert!(generated.runs.is_empty());
        assert_eq!(generated.findings.errors.len(), 1);

        let mut disabled = project;
        if let Some(Operation::PhotoVCarve(photo)) = disabled.operations.last_mut() {
            photo.enabled = false;
        }
        let built = build(&disabled, ProgramSplit::ProjectDefault, "2026-01-01");
        assert!(!built.findings.has_errors(), "{:?}", built.findings);
        assert_eq!(built.programs.len(), 1, "the Profile's program");
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
