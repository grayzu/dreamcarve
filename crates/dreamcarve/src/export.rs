// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve export <dcv>`: loads the project, generates every enabled
//! operation's toolpath with `dc-cam`, emits the programs with `dc-gcode`,
//! and writes them. A project whose operations use one tool is one
//! program, `<stem>.<ext>`. With more than one tool, the post-processor's
//! `tool_change.mode` decides (#95): `"separate_programs"` writes one
//! program per tool run, `<stem>-<k>-T<n>.<ext>`, and `"m6"` one program
//! with `M6` tool changes; `--programs-per-tool` and `--one-program`
//! override it for one export without saving it (functional spec,
//! capability 19). Nothing is written unless every step succeeds:
//! every program (and, with `--post`, the updated project) is staged in a
//! temporary file, and all are renamed into place at the end.
//!
//! [`build`] is the whole pipeline without writing; `check` runs it too, so
//! `check` reports exactly what `export` would. It validates the project it
//! is given rather than trusting the problems found at load, so the checks
//! that depend on the post-processor see the one `--post` swapped in.
//!
//! A finding about one value of the file is printed as
//! `coaster.dcv:9:21: error: …`, the form editors and terminals turn into a
//! link to that line and column (#76).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dc_cam::generate_profile;
use dc_gcode::format::format_number;
use dc_gcode::transform::mm_per_unit;
use dc_gcode::{AxisRange, ExportWarning, Program, ProgramSplit, emit_programs};
use dc_project::{
    Operation, PostProcessorRef, Project, Severity, SourceMap, ToolChangeMode, Units, validate,
};

use crate::cli::{ExportArgs, Failure, StagedFile, lookup_post, read_file, today_utc};
use crate::config;

/// One error or warning.
#[derive(Debug)]
pub struct Finding {
    /// The field it is about, as validation names it
    /// (`tools[t1].feed_mm_min`), when it is about one.
    pub field: Option<String>,
    /// What is wrong.
    pub message: String,
}

impl Finding {
    /// A finding about no one field.
    fn plain(message: String) -> Self {
        Self {
            field: None,
            message,
        }
    }
}

/// What the pipeline found, in the order found.
#[derive(Debug, Default)]
pub struct Findings {
    /// Error-class findings; any one means no program.
    pub errors: Vec<Finding>,
    /// Findings that still allow a safe program.
    pub warnings: Vec<Finding>,
}

/// The `.dcv` file as read, to locate findings in.
pub struct ProjectFile<'a> {
    /// The path as the user gave it.
    pub path: &'a Path,
    /// Where each value is in the text, or `None` if it could not be
    /// scanned (then no finding gets a location).
    map: Option<SourceMap>,
    /// Whether the post-processor in the file was replaced (`--post`), so
    /// its fields' positions no longer match what was checked.
    post_replaced: bool,
}

impl ProjectFile<'_> {
    /// `finding` as one line: `path:line:column: label: message` when its
    /// field is in the file, `label: message` otherwise.
    pub fn describe(&self, label: &str, finding: &Finding) -> String {
        let position = finding
            .field
            .as_deref()
            .filter(|f| !(self.post_replaced && f.starts_with("post_processor")))
            .and_then(|f| self.map.as_ref()?.locate(f));
        match position {
            Some(at) => format!("{}:{at}: {label}: {}", self.path.display(), finding.message),
            None => format!("{label}: {}", finding.message),
        }
    }
}

/// The pipeline's result: the programs when there were no errors.
pub struct Built {
    /// The emitted programs, in the order they are run: one per tool run
    /// or one in all (#95). Empty when any error was found.
    pub programs: Vec<Program>,
    /// Errors and warnings.
    pub findings: Findings,
}

/// Validates `project`, generates every enabled operation's toolpath, and
/// emits the programs, collecting every error and warning on the way.
/// `split` is how a multi-tool project is divided into programs (the
/// project's mode, or an `export` flag); `date` fills each program
/// header's `{date}`.
pub fn build(project: &Project, split: ProgramSplit, date: &str) -> Built {
    let mut findings = Findings::default();
    for problem in validate(project) {
        let finding = Finding {
            field: problem.field(),
            message: problem.to_string(),
        };
        match problem.severity() {
            Severity::Error => findings.errors.push(finding),
            Severity::Warning => findings.warnings.push(finding),
        }
    }
    if !findings.errors.is_empty() {
        // Generators must not see a project validation rejects.
        return Built {
            programs: Vec::new(),
            findings,
        };
    }

    let mut ops = Vec::new();
    for operation in &project.operations {
        // Disabled operations are already reported by validation.
        if !operation.enabled() {
            continue;
        }
        // Pocket, Drill and Engrave load and validate from format version
        // 8 on; their generators land in their own slices (#117). Until
        // then an enabled one is refused, never skipped.
        let Operation::Profile(op) = operation else {
            findings.errors.push(Finding::plain(format!(
                "operation `{}` (`{}`) is a {} operation, which is not supported by this build \
                 yet; disable it to export the rest",
                operation.id(),
                operation.name(),
                operation.kind()
            )));
            continue;
        };
        let Some(tool) = project.tools.iter().find(|t| t.id == op.tool) else {
            findings.errors.push(Finding::plain(format!(
                "operation `{}` uses tool `{}`, which is not in the project",
                op.id, op.tool
            )));
            continue;
        };
        match generate_profile(project, op) {
            Ok((toolpath, warnings)) => {
                findings.warnings.extend(
                    warnings
                        .iter()
                        .map(|w| Finding::plain(format!("operation `{}`: {w}", op.id))),
                );
                ops.push((op.clone(), tool.clone(), toolpath));
            }
            Err(e) => findings.errors.push(Finding::plain(e.to_string())),
        }
    }
    if !findings.errors.is_empty() {
        return Built {
            programs: Vec::new(),
            findings,
        };
    }

    match emit_programs(project, &ops, split, date) {
        Ok((programs, warnings)) => {
            findings.warnings.extend(
                warnings
                    .iter()
                    .map(|w| Finding::plain(describe_export_warning(w))),
            );
            Built { programs, findings }
        }
        Err(e) => {
            findings.errors.push(Finding::plain(e.to_string()));
            Built {
                programs: Vec::new(),
                findings,
            }
        }
    }
}

fn describe_export_warning(warning: &ExportWarning) -> String {
    match warning {
        ExportWarning::DisabledOperationSkipped { op } => {
            format!("operation `{op}` is disabled and was left out")
        }
        ExportWarning::ArcsFlattened { op, count } => format!(
            "operation `{op}`: {count} arc(s) written as short straight moves because the controller would reject them once rounded"
        ),
        ExportWarning::ArcFitFallback { op, runs } => format!(
            "arc_fit_fallback: operation `{op}`: {runs} run(s) of straight moves kept as written because the controller would reject the arc fitted to them once rounded"
        ),
    }
}

/// Loads the `.dcv` at `path`; a file that cannot become a project is an
/// error (exit 1). The problems found at load are not returned: [`build`]
/// validates the project again, after any `--post` swap, so they cannot go
/// stale. The file comes back to locate [`build`]'s findings in.
pub fn load_project(path: &Path) -> Result<(Project, ProjectFile<'_>), Failure> {
    let bytes = read_file(path)?;
    let project = dc_project::load(&bytes)
        .map(|loaded| loaded.project)
        .map_err(|e| Failure::error(format!("cannot load {}: {e}", path.display())))?;
    let map = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok());
    let file = ProjectFile {
        path,
        map,
        post_replaced: false,
    };
    Ok((project, file))
}

/// Runs `export`.
pub fn run(args: &ExportArgs) -> Result<ExitCode, Failure> {
    let (mut project, mut file) = load_project(&args.dcv)?;
    // `--post` is how a project changes post-processor or upgrades to a
    // newer version of its own (#106): an explicit action, reported below.
    let mut switched_from = None;
    if let Some(id) = &args.post {
        let mut latest = lookup_post(id)?;
        // The new settings are the definition's defaults with the user's
        // configured ones on top, as `init` writes them.
        config::seed_post(&mut latest)?;
        switched_from = Some(std::mem::replace(&mut project.post_processor, latest));
        file.post_replaced = true;
    }
    let date = args.date.clone().unwrap_or_else(today_utc);

    let Built { programs, findings } = build(&project, args.split(), &date);
    for warning in &findings.warnings {
        eprintln!("{}", file.describe("warning", warning));
    }
    for error in &findings.errors {
        eprintln!("{}", file.describe("error", error));
    }
    if programs.is_empty() {
        return Err(Failure::error(format!(
            "export of {} failed; nothing was written",
            args.dcv.display()
        )));
    }

    let outputs = output_paths(args, &programs)?;
    if outputs.iter().any(|output| same_file(output, &args.dcv)) {
        return Err(Failure::error(format!(
            "refusing to write the program over the project file {}",
            args.dcv.display()
        )));
    }
    // Stage everything before moving anything into place. A failed write
    // drops the files staged so far, which removes them.
    let staged_programs = outputs
        .iter()
        .zip(&programs)
        .map(|(output, program)| StagedFile::write(output, program.text.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    let staged_project = match &args.post {
        Some(_) => {
            let text = dc_project::save(&project).map_err(Failure::error)?;
            Some(StagedFile::write(&args.dcv, text.as_bytes())?)
        }
        None => None,
    };
    for staged in staged_programs {
        staged.commit()?;
    }
    if let Some(staged) = staged_project {
        staged.commit()?;
    }

    match (programs.as_slice(), outputs.as_slice()) {
        ([program], [output]) => report(&project, program, output, args, switched_from.as_ref()),
        _ => report_runs(&project, &programs, &outputs, args, switched_from.as_ref()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Where each program goes. One program goes to `-o`, or to the project's
/// path with the post-processor's extension. Programs of tool runs (#95)
/// go beside it, named `<stem>-<k>-T<n>.<ext>`: `<k>` the run's place,
/// `<n>` its tool's number, `<stem>` and `<ext>` those of that one path.
fn output_paths(args: &ExportArgs, programs: &[Program]) -> Result<Vec<PathBuf>, Failure> {
    let single = match (&args.output, programs.first()) {
        (Some(path), _) => path.clone(),
        // `build` validated the extension of the post-processor it emitted
        // with, so it is safe to append (`is_safe_file_extension`).
        (None, Some(program)) => args.dcv.with_extension(&program.extension),
        (None, None) => args.dcv.clone(),
    };
    if programs.len() < 2 {
        return Ok(vec![single]);
    }
    let stem = single
        .file_stem()
        .ok_or_else(|| Failure::error(format!("{} is not a file path", single.display())))?;
    Ok(programs
        .iter()
        .enumerate()
        .map(|(k, program)| {
            let mut name = OsString::from(stem);
            name.push(format!("-{}-T{}", k + 1, program.first_tool.number));
            if let Some(extension) = single.extension() {
                name.push(".");
                name.push(extension);
            }
            single.with_file_name(name)
        })
        .collect())
}

/// True if `a` and `b` name the same existing file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Prints the export summary of one program: the file written, one line
/// per exported operation, the program's extents (#62), the tool to fit and
/// set work zero with before starting (#57), then every pause and tool
/// change with its line number and message, because the controller or
/// sender may not show the message (design step 8).
fn report(
    project: &Project,
    program: &Program,
    output: &Path,
    args: &ExportArgs,
    switched_from: Option<&PostProcessorRef>,
) {
    println!(
        "Wrote {} ({} lines, post-processor {}).",
        output.display(),
        program.text.lines().count(),
        project.post_processor.id
    );
    report_post_and_operations(project, args, switched_from);
    for line in describe_extents(project, program, largest_tool_radius_mm(project)) {
        println!("{line}");
    }
    // #57: the program does not stop for the first tool.
    println!(
        "Before you start: fit T{}{} and set work zero with it.",
        program.first_tool.number,
        name_suffix(&program.first_tool.name)
    );
    report_pauses(program, "");
    // #95: the user chose one program with M6, in the project or with
    // `--one-program`; it stops only if the sender lets M6 through or
    // stops for it itself.
    let one_program = match args.split() {
        ProgramSplit::OneProgram => true,
        ProgramSplit::ProgramPerTool => false,
        ProgramSplit::ProjectDefault => {
            project.post_processor.tool_change_mode == ToolChangeMode::M6
        }
    };
    if one_program && changes_tools(project) {
        println!(
            "This program changes tools with M6, so your sender must stop at M6; see \"One program with M6\" in the post-processors chapter."
        );
    }
}

/// Prints the export summary of the programs of tool runs (#95): the files
/// in the order to run them, with what to do before each, then one line
/// per exported operation, then each program's extents (#62) and pauses.
fn report_runs(
    project: &Project,
    programs: &[Program],
    outputs: &[PathBuf],
    args: &ExportArgs,
    switched_from: Option<&PostProcessorRef>,
) {
    println!("Wrote {} programs; run them in this order:", programs.len());
    for (k, (program, output)) in programs.iter().zip(outputs).enumerate() {
        let tool = &program.first_tool;
        let fit = format!("fit T{}{}", tool.number, name_suffix(&tool.name));
        // The first program sets work zero; a later one keeps X and Y and
        // sets Z zero with its new bit, whose length differs (#43).
        let before = if k == 0 {
            format!("{fit} and set work zero (X, Y and Z) with it.")
        } else {
            format!("{fit} and set Z zero with it. Keep X and Y zero.")
        };
        println!(
            "  {}. {}  Before you start: {before}",
            k + 1,
            output.display()
        );
    }
    println!("Post-processor {}.", project.post_processor.id);
    report_post_and_operations(project, args, switched_from);
    for (program, output) in programs.iter().zip(outputs) {
        println!(
            "{} ({} lines):",
            output.display(),
            program.text.lines().count()
        );
        for line in describe_extents(project, program, tool_radius_mm(project, program)) {
            println!("  {line}");
        }
        report_pauses(program, "  ");
    }
}

/// Prints that `--post` updated the project, from which post-processor
/// version to which (`grblhal v1 → v2`, #106), then one line per exported
/// operation.
fn report_post_and_operations(
    project: &Project,
    args: &ExportArgs,
    switched_from: Option<&PostProcessorRef>,
) {
    if let Some(old) = switched_from {
        let new = &project.post_processor;
        let to = if new.id == old.id {
            format!("v{}", new.definition_version)
        } else {
            format!("{} v{}", new.id, new.definition_version)
        };
        println!(
            "Updated {}: post-processor {} v{} → {to}, with its default settings.",
            args.dcv.display(),
            old.id,
            old.definition_version
        );
    }
    for op in project.operations.iter().filter(|op| op.enabled()) {
        let tool = project
            .tools
            .iter()
            .find(|t| &t.id == op.tool())
            .map_or("?", |t| t.name.as_str());
        // Validation refuses an unset depth, so an exported one is set.
        let depth = op
            .depth_mm()
            .map_or_else(|| "?".to_owned(), |d| d.to_string());
        println!(
            "  {} `{}`: tool `{}` ({tool}), {} target(s), {depth} mm deep",
            op.id(),
            op.name(),
            op.tool(),
            op.targets().len(),
        );
    }
}

/// Prints every pause and tool change of `program`, each line after
/// `indent`.
fn report_pauses(program: &Program, indent: &str) {
    if program.pauses.is_empty() {
        return;
    }
    println!("{indent}Pauses and tool changes; resume each with cycle start:");
    for pause in &program.pauses {
        let message = if pause.message.is_empty() {
            "(no message)"
        } else {
            pause.message.as_str()
        };
        println!("{indent}  line {}: {message}", pause.line);
    }
}

/// ` <name>`, or nothing for an empty name.
fn name_suffix(name: &str) -> String {
    if name.is_empty() {
        String::new()
    } else {
        format!(" {name}")
    }
}

/// Whether consecutive enabled operations of `project` use different tools.
fn changes_tools(project: &Project) -> bool {
    let tools: Vec<_> = project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .map(Operation::tool)
        .collect();
    tools.windows(2).any(|pair| pair.first() != pair.get(1))
}

/// The radius of the largest tool an enabled operation uses, mm: how far
/// the cutter's edge reaches past the tool centre in a program that uses
/// them all.
pub fn largest_tool_radius_mm(project: &Project) -> f64 {
    project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .filter_map(|op| project.tools.iter().find(|t| &t.id == op.tool()))
        .map(|t| t.diameter_mm / 2.0)
        .fold(0.0, f64::max)
}

/// The radius of the one tool of `program`, a tool run's program (#95), mm.
pub fn tool_radius_mm(project: &Project, program: &Program) -> f64 {
    program
        .first_tool
        .number
        .checked_sub(1)
        .and_then(|i| project.tools.get(i))
        .map_or(0.0, |t| t.diameter_mm / 2.0)
}

/// The program's extents in machine coordinates (#62), written as the
/// program writes numbers: first the tool centre's, every axis, then in X
/// and Y widened by `radius_mm`, the largest radius of the program's tools,
/// which is where the cutter's edge reaches. Users place clamps from them.
/// Empty for a program that never moves.
pub fn describe_extents(project: &Project, program: &Program, radius_mm: f64) -> Vec<String> {
    let extents = &program.extents;
    let units = extents.units;
    // The program was emitted with this post-processor, so it resolves.
    let post = project.post_processor.resolve().ok();
    let number = |value: f64| {
        post.as_ref()
            .and_then(|post| format_number(value, &post.number_format, units).ok())
            .unwrap_or_else(|| value.to_string())
    };
    let ranges = |widen: f64, axes: &[(&str, Option<AxisRange>)]| -> Vec<String> {
        axes.iter()
            .filter_map(|(axis, range)| {
                range.map(|r| {
                    format!(
                        "{axis} {}..{}",
                        number(r.min - widen),
                        number(r.max + widen)
                    )
                })
            })
            .collect()
    };
    let centre = ranges(0.0, &[("X", extents.x), ("Y", extents.y), ("Z", extents.z)]);
    if centre.is_empty() {
        return Vec::new();
    }
    let unit = match units {
        Units::Mm => "mm",
        Units::Inch => "in",
    };
    let mut lines = vec![format!(
        "Extents (machine coordinates, {unit}, tool centre): {}",
        centre.join(", ")
    )];
    let edge = ranges(
        radius_mm / mm_per_unit(units),
        &[("X", extents.x), ("Y", extents.y)],
    );
    if radius_mm > 0.0 && !edge.is_empty() {
        lines.push(format!(
            "  The cutter's edge reaches {}; keep clamps outside that.",
            edge.join(", ")
        ));
    }
    lines
}
