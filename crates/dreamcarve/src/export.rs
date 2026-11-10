// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve export <dcv>`: loads the project, runs the pipeline of
//! [`dc_job::build()`] (every enabled operation's toolpath with `dc-cam`, the
//! programs with `dc-gcode`), and writes them. A project whose operations
//! use one tool is one program, `<stem>.<ext>`. With more than one tool,
//! the post-processor's `tool_change.mode` decides (#95):
//! `"separate_programs"` writes one program per tool run,
//! `<stem>-<k>-T<n>.<ext>`, and `"m6"` one program with `M6` tool changes;
//! `--programs-per-tool` and `--one-program` override it for one export
//! without saving it (functional spec, capability 19). Nothing is written
//! unless every step succeeds: every program (with `--preview`, the
//! toolpath picture of addendum A5, and with `--post`, the updated
//! project) is staged in a temporary file, and all are renamed into place
//! at the end. If one rename fails, the files already moved are put back
//! as they were.
//!
//! `check` runs the same pipeline, so `check` reports exactly what `export`
//! would. The pipeline validates the project it is given rather than
//! trusting the problems found at load, so the checks that depend on the
//! post-processor see the one `--post` swapped in.
//!
//! A finding about one value of the file is printed as
//! `coaster.dcv:9:21: error: …`, the form editors and terminals turn into a
//! link to that line and column (#76). Every warning carries its stable
//! `snake_case` code and prints as `warning: <code>: <message>` (functional
//! spec, capability 25); the Warnings chapter lists the codes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dc_job::report::{Written, export_report};
use dc_job::summary::by_board;
use dc_job::{Built, Estimates, Finding, Program, preview, program_suffixes};
use dc_project::{Board, PostProcessorRef, Project, ResetReason, SettingReset, SourceMap};
use dreamcarve_shell::{StagedFile, commit_all, is_preview};

use crate::cli::{ExportArgs, Failure, lookup_post, read_file, switch_post, today_utc};
use crate::config;
use crate::output::{errln, outln};

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
    /// What migrating the file changed that the user should hear about
    /// (#498), printed as `note:` lines.
    pub notes: Vec<String>,
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
        let message = match &finding.code {
            Some(code) => format!("{code}: {}", finding.message),
            None => finding.message.clone(),
        };
        match position {
            Some(at) => format!("{}:{at}: {label}: {message}", self.path.display()),
            None => format!("{label}: {message}"),
        }
    }
}

/// Loads the `.dcv` at `path`; a file that cannot become a project is an
/// error (exit 1). The problems found at load are not returned:
/// [`dc_job::build()`] validates the project again, after any `--post` swap,
/// so they cannot go stale. The file comes back to locate the findings in.
pub fn load_project(path: &Path) -> Result<(Project, ProjectFile<'_>), Failure> {
    let bytes = read_file(path)?;
    let loaded = dc_project::load(&bytes)
        .map_err(|e| Failure::error(format!("cannot load {}: {e}", path.display())))?;
    let map = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok());
    let file = ProjectFile {
        path,
        map,
        post_replaced: false,
        notes: loaded.notes.iter().map(ToString::to_string).collect(),
    };
    Ok((loaded.project, file))
}

/// Runs `export`.
pub fn run(args: &ExportArgs) -> Result<ExitCode, Failure> {
    let (mut project, mut file) = load_project(&args.dcv)?;
    // Read before anything is written, so a bad value stops the export
    // with nothing changed. It is used for the printed estimate only.
    let estimate_settings = config::estimate_settings()?;
    // `--post` is how a project changes post-processor or upgrades to a
    // newer version of its own (#106): an explicit action, reported below.
    let mut switched = None;
    if let Some(id) = &args.post {
        let (new, resets) = if args.reset_settings {
            // The definition's defaults with the user's configured ones on
            // top, as `init` writes them.
            let mut latest = lookup_post(id)?;
            config::seed_post(&mut latest)?;
            (latest, None)
        } else {
            // #277: the user's settings carry over; the config's
            // `tool_change_mode` applies only where the mode was reset
            // (functional spec, capability 27).
            let (mut new, resets) = switch_post(&project.post_processor, id)?;
            if resets.iter().any(|r| r.setting == "tool_change_mode") {
                config::seed_post(&mut new)?;
            }
            (new, Some(resets))
        };
        let old = std::mem::replace(&mut project.post_processor, new);
        switched = Some(Switched { old, resets });
        file.post_replaced = true;
    }
    let date = args.date.clone().unwrap_or_else(today_utc);

    if let Some(preview) = &args.preview {
        check_preview_path(preview, &args.dcv)?;
    }
    let Built {
        programs,
        findings,
        generated,
    } = dc_job::build_board(&project, args.split(), &date, args.board.map(Board::from));
    for note in &file.notes {
        errln!("{}: note: {note}", file.path.display());
    }
    for warning in &findings.warnings {
        errln!("{}", file.describe("warning", warning));
    }
    for error in &findings.errors {
        errln!("{}", file.describe("error", error));
    }
    if programs.is_empty() {
        return Err(Failure::error(format!(
            "export of {} failed; nothing was written",
            args.dcv.display()
        )));
    }

    let outputs = output_paths(args.output.as_deref(), &args.dcv, &project, &programs)?;
    if outputs.iter().any(|output| same_file(output, &args.dcv)) {
        return Err(Failure::error(format!(
            "refusing to write the program over the project file {}",
            args.dcv.display()
        )));
    }
    if let Some(preview) = &args.preview {
        check_preview_not_program(preview, &outputs)?;
    }
    // Stage everything before moving anything into place. A failed write
    // drops the files staged so far, which removes them.
    let staged_programs = outputs
        .iter()
        .zip(&programs)
        .map(|(output, program)| StagedFile::write(output, program.text.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    // The preview goes with the programs: all or none (addendum A5).
    // `build` generated every operation, since it emitted programs.
    let staged_preview = match (&args.preview, &generated) {
        (Some(path), Some(generated)) => Some(StagedFile::write(
            path,
            preview::render_generated(&project, generated).as_bytes(),
        )?),
        _ => None,
    };
    let staged_project = match &args.post {
        Some(_) => {
            let text = dc_project::save(&project).map_err(Failure::error)?;
            Some(StagedFile::write(&args.dcv, text.as_bytes())?)
        }
        None => None,
    };
    // All or nothing: a program in place beside a project that failed to
    // move, or the reverse, would not match what was reported.
    commit_all(
        staged_programs
            .into_iter()
            .chain(staged_preview)
            .chain(staged_project)
            .collect(),
    )?;

    // The programs are written; the estimate only reads them.
    let estimates = Estimates::new(&project, &programs, estimate_settings);
    let generated = generated.unwrap_or_default();
    let outputs: Vec<String> = outputs.iter().map(|o| o.display().to_string()).collect();
    let preview = args.preview.as_ref().map(|p| p.display().to_string());
    let switched = switched
        .as_ref()
        .map(|switched| switched_lines(&project, args, switched))
        .unwrap_or_default();
    let written = Written {
        programs: &programs,
        outputs: &outputs,
        split: args.split(),
        generated: &generated,
        preview: preview.as_deref(),
        switched: &switched,
    };
    for line in export_report(&project, &written, &estimates) {
        outln!("{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Refuses a `--preview` path that is not an `.svg` file, that is the
/// project file, or that is an existing file other than an earlier
/// preview, such as the drawing the project came from (addendum A5).
/// Runs before anything is built.
pub fn check_preview_path(preview: &Path, dcv: &Path) -> Result<(), Failure> {
    let svg = preview
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if !svg {
        return Err(Failure::error(format!(
            "--preview must name an .svg file, not {}",
            preview.display()
        )));
    }
    if same_path(preview, dcv) {
        return Err(Failure::error(format!(
            "--preview {} is the project file; refusing to write over it",
            preview.display()
        )));
    }
    if std::fs::symlink_metadata(preview).is_ok() && !is_preview(preview) {
        return Err(Failure::error(format!(
            "--preview {} exists and is not a DreamCarve preview (your drawing?); refusing to write over it",
            preview.display()
        )));
    }
    Ok(())
}

/// Refuses a `--preview` path that is one of the program paths `outputs`.
pub fn check_preview_not_program(preview: &Path, outputs: &[PathBuf]) -> Result<(), Failure> {
    match outputs.iter().find(|output| same_path(preview, output)) {
        Some(output) => Err(Failure::error(format!(
            "--preview {} is where the program {} goes; refusing to write over it",
            preview.display(),
            output.display()
        ))),
        None => Ok(()),
    }
}

/// True if `a` and `b` name one file, whether or not it exists yet: the
/// same path, the same existing file, or the same name in the same
/// directory.
fn same_path(a: &Path, b: &Path) -> bool {
    if a == b || same_file(a, b) {
        return true;
    }
    matches!((resolved(a), resolved(b)), (Some(a), Some(b)) if a == b)
}

/// `path` with its directory resolved, so `./x.svg` and `x.svg` compare
/// equal before either exists; `None` when the directory does not exist.
fn resolved(path: &Path) -> Option<PathBuf> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Some(std::fs::canonicalize(parent).ok()?.join(path.file_name()?))
}

/// Where each program of the project `dcv` goes. One program goes to
/// `output` (`-o`), or to the project's path with the post-processor's
/// extension. Programs of tool runs (#95) go beside it, named
/// `<stem>-<k>-T<n>.<ext>`: `<k>` the run's place, `<n>` its tool's
/// number, `<stem>` and `<ext>` those of that one path. With an enabled
/// Inlay (#498, [`by_board`]) every program is named after its board,
/// `<stem>-female-<k>-T<n>.<ext>` and `<stem>-male-<k>-T<n>.<ext>`, `<k>`
/// counting from 1 within each board, however many there are
/// ([`dc_job::program_suffixes`]).
pub fn output_paths(
    output: Option<&Path>,
    dcv: &Path,
    project: &Project,
    programs: &[Program],
) -> Result<Vec<PathBuf>, Failure> {
    let single = match (output, programs.first()) {
        (Some(path), _) => path.to_path_buf(),
        // `build` validated the extension of the post-processor it emitted
        // with, so it is safe to append (`is_safe_file_extension`).
        (None, Some(program)) => dcv.with_extension(&program.extension),
        (None, None) => dcv.to_path_buf(),
    };
    if programs.len() < 2 && !by_board(project) {
        return Ok(vec![single]);
    }
    let stem = single
        .file_stem()
        .ok_or_else(|| Failure::error(format!("{} is not a file path", single.display())))?;
    Ok(program_suffixes(project, programs)
        .into_iter()
        .map(|suffix| {
            let mut name = OsString::from(stem);
            name.push(suffix);
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

/// What `--post` replaced: the old reference, and the settings the switch
/// reset (`None` with `--reset-settings`, which resets them all).
pub struct Switched {
    /// The reference the project had.
    old: PostProcessorRef,
    /// The settings the new definition could not use (#277).
    resets: Option<Vec<SettingReset>>,
}

/// That `--post` updated the project, from which post-processor version
/// to which (`grblhal v1 → v2`, #106), whether it kept the user's
/// settings, and each one it reset and why (#277): the lines the report
/// prints before the operations.
fn switched_lines(project: &Project, args: &ExportArgs, switched: &Switched) -> Vec<String> {
    let Switched { old, resets } = switched;
    let new = &project.post_processor;
    let to = if new.id == old.id {
        format!("v{}", new.definition_version)
    } else {
        format!("{} v{}", new.id, new.definition_version)
    };
    let settings = match resets {
        Some(_) => "keeping your settings",
        None => "with its default settings",
    };
    let mut lines = vec![format!(
        "Updated {}: post-processor {} v{} → {to}, {settings}.",
        args.dcv.display(),
        old.id,
        old.definition_version
    )];
    for reset in resets.iter().flatten() {
        let why = match reset.reason {
            ResetReason::NoM6 => "has no M6 tool change",
            ResetReason::NoCannedCycles => "has no canned cycles",
        };
        lines.push(format!(
            "  Reset {} {} → {}: post-processor {} {why}.",
            reset.setting, reset.from, reset.to, new.id
        ));
    }
    lines
}
