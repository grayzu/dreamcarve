// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve export <dcv>`: loads the project, generates every enabled
//! operation's toolpath with `dc-cam`, emits the program with `dc-gcode`,
//! and writes it. Nothing is written unless every step succeeds: the
//! program (and, with `--post`, the updated project) is staged in a
//! temporary file and renamed into place at the end.
//!
//! [`build`] is the whole pipeline without writing; `check` runs it too, so
//! `check` reports exactly what `export` would. It validates the project it
//! is given rather than trusting the problems found at load, so the checks
//! that depend on the post-processor see the one `--post` swapped in.

use std::path::Path;
use std::process::ExitCode;

use dc_cam::generate_profile;
use dc_gcode::{ExportWarning, Program, emit};
use dc_project::{Operation, Project, Severity, validate};

use crate::cli::{ExportArgs, Failure, StagedFile, lookup_post, read_file, today_utc};

/// What the pipeline found, in the order found.
#[derive(Debug, Default)]
pub struct Findings {
    /// Error-class findings; any one means no program.
    pub errors: Vec<String>,
    /// Findings that still allow a safe program.
    pub warnings: Vec<String>,
}

/// The pipeline's result: the program when there were no errors.
pub struct Built {
    /// The emitted program, `None` when any error was found.
    pub program: Option<Program>,
    /// Errors and warnings.
    pub findings: Findings,
}

/// Validates `project`, generates every enabled operation's toolpath, and
/// emits the program, collecting every error and warning on the way.
/// `date` fills the program header's `{date}`.
pub fn build(project: &Project, date: &str) -> Built {
    let mut findings = Findings::default();
    for problem in validate(project) {
        match problem.severity() {
            Severity::Error => findings.errors.push(problem.to_string()),
            Severity::Warning => findings.warnings.push(problem.to_string()),
        }
    }
    if !findings.errors.is_empty() {
        // Generators must not see a project validation rejects.
        return Built {
            program: None,
            findings,
        };
    }

    let mut ops = Vec::new();
    for Operation::Profile(op) in &project.operations {
        // Disabled operations are already reported by validation.
        if !op.enabled {
            continue;
        }
        let Some(tool) = project.tools.iter().find(|t| t.id == op.tool) else {
            findings.errors.push(format!(
                "operation `{}` uses tool `{}`, which is not in the project",
                op.id, op.tool
            ));
            continue;
        };
        match generate_profile(project, op) {
            Ok((toolpath, warnings)) => {
                findings.warnings.extend(
                    warnings
                        .iter()
                        .map(|w| format!("operation `{}`: {w}", op.id)),
                );
                ops.push((op.clone(), tool.clone(), toolpath));
            }
            Err(e) => findings.errors.push(e.to_string()),
        }
    }
    if !findings.errors.is_empty() {
        return Built {
            program: None,
            findings,
        };
    }

    match emit(project, &ops, date) {
        Ok((program, warnings)) => {
            findings
                .warnings
                .extend(warnings.iter().map(describe_export_warning));
            Built {
                program: Some(program),
                findings,
            }
        }
        Err(e) => {
            findings.errors.push(e.to_string());
            Built {
                program: None,
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
    }
}

/// Loads the `.dcv` at `path`; a file that cannot become a project is an
/// error (exit 1). Only the project is returned: [`build`] validates it
/// again, after any `--post` swap, so problems found at load cannot go
/// stale.
pub fn load_project(path: &Path) -> Result<Project, Failure> {
    let bytes = read_file(path)?;
    dc_project::load(&bytes)
        .map(|loaded| loaded.project)
        .map_err(|e| Failure::error(format!("cannot load {}: {e}", path.display())))
}

/// Runs `export`.
pub fn run(args: &ExportArgs) -> Result<ExitCode, Failure> {
    let mut project = load_project(&args.dcv)?;
    if let Some(id) = &args.post {
        project.post_processor = lookup_post(id)?;
    }
    let date = args.date.clone().unwrap_or_else(today_utc);

    let Built { program, findings } = build(&project, &date);
    for warning in &findings.warnings {
        eprintln!("warning: {warning}");
    }
    for error in &findings.errors {
        eprintln!("error: {error}");
    }
    let Some(program) = program else {
        return Err(Failure::error(format!(
            "export of {} failed; nothing was written",
            args.dcv.display()
        )));
    };

    let output = match &args.output {
        Some(path) => path.clone(),
        // `build` validated the extension of the post-processor it emitted
        // with, so it is safe to append (`is_safe_file_extension`).
        None => args.dcv.with_extension(&program.extension),
    };
    if same_file(&output, &args.dcv) {
        return Err(Failure::error(format!(
            "refusing to write the program over the project file {}",
            args.dcv.display()
        )));
    }
    // Stage everything before moving anything into place.
    let staged_program = StagedFile::write(&output, program.text.as_bytes())?;
    let staged_project = match &args.post {
        Some(_) => {
            let text = dc_project::save(&project).map_err(Failure::error)?;
            Some(StagedFile::write(&args.dcv, text.as_bytes())?)
        }
        None => None,
    };
    staged_program.commit()?;
    if let Some(staged) = staged_project {
        staged.commit()?;
    }

    report(&project, &program, &output, args);
    Ok(ExitCode::SUCCESS)
}

/// True if `a` and `b` name the same existing file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Prints the export summary: the file written, one line per exported
/// operation, the tool to fit and set work zero with before starting (#57),
/// then every pause and tool change with its line number and message, because
/// the controller or sender may not show the message (design step 8).
fn report(project: &Project, program: &Program, output: &Path, args: &ExportArgs) {
    println!(
        "Wrote {} ({} lines, post-processor {}).",
        output.display(),
        program.text.lines().count(),
        project.post_processor.id
    );
    if args.post.is_some() {
        println!(
            "Updated {} to use post-processor {}.",
            args.dcv.display(),
            project.post_processor.id
        );
    }
    for Operation::Profile(op) in project.operations.iter().filter(|op| op.enabled()) {
        let tool = project
            .tools
            .iter()
            .find(|t| t.id == op.tool)
            .map_or("?", |t| t.name.as_str());
        println!(
            "  {} `{}`: tool `{}` ({tool}), {} target(s), {} mm deep",
            op.id,
            op.name,
            op.tool,
            op.targets.len(),
            op.depth_mm
        );
    }
    // #57: the program does not stop for the first tool.
    let first = &program.first_tool;
    let name = if first.name.is_empty() {
        String::new()
    } else {
        format!(" {}", first.name)
    };
    println!(
        "Before you start: fit T{}{name} and set work zero with it.",
        first.number
    );
    if !program.pauses.is_empty() {
        println!("Pauses and tool changes; resume each with cycle start:");
        for pause in &program.pauses {
            let message = if pause.message.is_empty() {
                "(no message)"
            } else {
                pause.message.as_str()
            };
            println!("  line {}: {message}", pause.line);
        }
    }
}
