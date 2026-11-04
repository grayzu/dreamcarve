// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve export <dcv>`: loads the project, runs the pipeline of
//! [`dc_job::build`] (every enabled operation's toolpath with `dc-cam`, the
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

use dc_job::summary::{
    before_you_start, board_heading, by_board, changes_tools, describe_carves, describe_extents,
    describe_operations, largest_tool_radius_mm, one_program, tool_radius_mm,
};
use dc_job::{Built, EmitOp, Estimates, Finding, Program, preview, program_suffixes};
use dc_project::{Board, PostProcessorRef, Project, ResetReason, SettingReset, SourceMap};
use dreamcarve_shell::{StagedFile, commit_all};

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
/// [`dc_job::build`] validates the project again, after any `--post` swap,
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

    let boards = by_board(&project);
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
    match (programs.as_slice(), outputs.as_slice()) {
        ([program], [output]) if !boards => report(
            &project,
            program,
            output,
            args,
            switched.as_ref(),
            &estimates,
            &generated,
        )?,
        _ => report_runs(
            &project,
            &programs,
            &outputs,
            args,
            switched.as_ref(),
            &estimates,
            &generated,
        )?,
    }
    for line in describe_carves(&project, &generated) {
        outln!("{line}")?;
    }
    if let Some(preview) = &args.preview {
        outln!("Wrote preview {}.", preview.display())?;
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

/// Whether the file at `path` starts as [`preview::render`] writes it:
/// its first kilobyte holds [`preview::MARKER`].
fn is_preview(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(1024).read_to_end(&mut head).is_err() {
        return false;
    }
    String::from_utf8_lossy(&head).contains(preview::MARKER)
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

/// Prints the export summary of one program: the file written, one line
/// per exported operation, the program's extents (#62), its estimated run
/// time (capability 30), the tool to fit and set work zero with before
/// starting (#57), then every pause and tool change with its line number
/// and message, because the controller or sender may not show the message
/// (design step 8).
#[allow(
    clippy::too_many_arguments,
    reason = "what was written, for what, and what generated it, each needed"
)]
fn report(
    project: &Project,
    program: &Program,
    output: &Path,
    args: &ExportArgs,
    switched: Option<&Switched>,
    estimates: &Estimates,
    generated: &[EmitOp],
) -> Result<(), Failure> {
    outln!(
        "Wrote {} ({} lines, post-processor {}).",
        output.display(),
        program.text.lines().count(),
        project.post_processor.id
    )?;
    report_post_and_operations(project, args, switched, generated)?;
    for line in describe_extents(project, program, largest_tool_radius_mm(project)) {
        outln!("{line}")?;
    }
    outln!("{}", estimates.program_line(0))?;
    // #57: the program does not stop for the first tool.
    outln!("Before you start: {}", before_you_start(program, 0, true))?;
    // #283: `G53` moves to a machine position, which is right only on a
    // homed machine; the program cannot check that.
    if let Some(z) = &program.machine_lift {
        outln!(
            "Before you start: home the machine; this program moves to machine Z {z} (G53).{}",
            limits_note(project)
        )?;
    }
    report_tool_length_offset(project)?;
    report_scaling(project)?;
    report_pauses(program, "")?;
    // #95: the user chose one program with M6, in the project or with
    // `--one-program`; it stops only if the sender lets M6 through or
    // stops for it itself.
    if one_program(project, args.split()) && changes_tools(project) {
        outln!(
            "This program changes tools with M6, so your sender must stop at M6; see \"One program with M6\" in the post-processors chapter."
        )?;
    }
    Ok(())
}

/// #308: the post-processor's condition for a `G53` lift outside the
/// travel to stop before it moves, with a leading space, or nothing.
fn limits_note(project: &Project) -> String {
    dc_gcode::machine_lift_limits_note(&project.post_processor.id)
        .map(|note| format!(" {note}"))
        .unwrap_or_default()
}

/// Prints the export summary of the programs of tool runs (#95): the files
/// in the order to run them, with what to do before each, then one line
/// per exported operation, then each program's extents (#62), estimated
/// run time and pauses, then the total run time (capability 30).
#[allow(
    clippy::too_many_arguments,
    reason = "what was written, for what, and what generated it, each needed"
)]
fn report_runs(
    project: &Project,
    programs: &[Program],
    outputs: &[PathBuf],
    args: &ExportArgs,
    switched: Option<&Switched>,
    estimates: &Estimates,
    generated: &[EmitOp],
) -> Result<(), Failure> {
    let boards = by_board(project);
    if programs.len() == 1 {
        outln!("Wrote 1 program:")?;
    } else {
        outln!("Wrote {} programs; run them in this order:", programs.len())?;
    }
    let mut k = 0;
    let mut board = None;
    for (program, output) in programs.iter().zip(outputs) {
        // #498: each board is its own setup, its programs counted afresh.
        if boards && board != Some(program.board) {
            board = Some(program.board);
            k = 0;
            outln!("{}", board_heading(project, program.board))?;
        }
        // The first program sets work zero; a later one keeps X and Y and
        // sets Z zero with its new bit, whose length differs (#43).
        let before = before_you_start(program, k, false);
        k += 1;
        outln!("  {k}. {}  Before you start: {before}", output.display())?;
    }
    // #283: every program of the export ends with the same lift.
    if let Some(z) = programs.iter().find_map(|p| p.machine_lift.as_ref()) {
        outln!(
            "Before you start: home the machine; each program moves to machine Z {z} (G53).{}",
            limits_note(project)
        )?;
    }
    report_tool_length_offset(project)?;
    report_scaling(project)?;
    outln!("Post-processor {}.", project.post_processor.id)?;
    report_post_and_operations(project, args, switched, generated)?;
    let mut board = None;
    for (k, (program, output)) in programs.iter().zip(outputs).enumerate() {
        // #498: each board's programs together, with the board's total.
        if boards && board != Some(program.board) {
            if let Some(done) = board {
                report_board_total(programs, estimates, done)?;
            }
            board = Some(program.board);
            outln!("{}", board_heading(project, program.board))?;
        }
        outln!(
            "{} ({} lines):",
            output.display(),
            program.text.lines().count()
        )?;
        for line in describe_extents(project, program, tool_radius_mm(project, program)) {
            outln!("  {line}")?;
        }
        outln!("  {}", estimates.program_line(k))?;
        report_pauses(program, "  ")?;
    }
    if let Some(done) = board {
        report_board_total(programs, estimates, done)?;
    }
    if let Some(total) = estimates.total_line() {
        outln!("{total}")?;
    }
    Ok(())
}

/// Prints the estimated time of `board`'s programs, when it has more than
/// one and they are not all of the export (#498).
fn report_board_total(
    programs: &[Program],
    estimates: &Estimates,
    board: Board,
) -> Result<(), Failure> {
    let ks: Vec<usize> = programs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.board == board)
        .map(|(k, _)| k)
        .collect();
    if ks.len() > 1
        && ks.len() < programs.len()
        && let Some(line) =
            estimates.total_of(&ks, &format!("Estimated {} board time", board.as_str()))
    {
        outln!("{line}")?;
    }
    Ok(())
}

/// What `--post` replaced: the old reference, and the settings the switch
/// reset (`None` with `--reset-settings`, which resets them all).
pub struct Switched {
    /// The reference the project had.
    old: PostProcessorRef,
    /// The settings the new definition could not use (#277).
    resets: Option<Vec<SettingReset>>,
}

/// Prints that `--post` updated the project, from which post-processor
/// version to which (`grblhal v1 → v2`, #106), whether it kept the user's
/// settings, and each one it reset and why (#277); then one line per
/// exported operation.
fn report_post_and_operations(
    project: &Project,
    args: &ExportArgs,
    switched: Option<&Switched>,
    generated: &[EmitOp],
) -> Result<(), Failure> {
    if let Some(Switched { old, resets }) = switched {
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
        outln!(
            "Updated {}: post-processor {} v{} → {to}, {settings}.",
            args.dcv.display(),
            old.id,
            old.definition_version
        )?;
        for reset in resets.iter().flatten() {
            let why = match reset.reason {
                ResetReason::NoM6 => "has no M6 tool change",
                ResetReason::NoCannedCycles => "has no canned cycles",
            };
            outln!(
                "  Reset {} {} → {}: post-processor {} {why}.",
                reset.setting,
                reset.from,
                reset.to,
                new.id
            )?;
        }
    }
    for line in describe_operations(project, generated) {
        outln!("  {line}")?;
    }
    Ok(())
}

/// The post-processors whose programs leave the tool length offset
/// (`G43.1`) alone: grblHAL and GRBL fold an active offset into the work
/// zero the user sets (`G10 L20`), so a `G49` after the user zeroes Z
/// would move every Z by it (#279; `dc-project/src/definitions/README.md`,
/// "Header modal state").
const KEEPS_TOOL_LENGTH_OFFSET: [&str; 2] = ["grblhal", "grbl"];

/// Prints, for a program that leaves the tool length offset alone, when
/// to set one (functional spec, capability 29).
fn report_tool_length_offset(project: &Project) -> Result<(), Failure> {
    if KEEPS_TOOL_LENGTH_OFFSET.contains(&project.post_processor.id.as_str()) {
        outln!(
            "This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero."
        )?;
    }
    Ok(())
}

/// Prints, for a grblHAL program whose header cancels scaling with `G50`,
/// that grblHAL's `G50` misses a scale that leaves X at 1, so to soft-reset
/// after any `G51` and check `$G` (#320; [`dc_gcode::scaling_note`]). It is
/// printed, not written into the program.
fn report_scaling(project: &Project) -> Result<(), Failure> {
    // The project was exported with this post-processor, so it resolves.
    let note = project
        .post_processor
        .resolve()
        .ok()
        .and_then(|post| dc_gcode::scaling_note(&post));
    if let Some(note) = note {
        outln!("{note}")?;
    }
    Ok(())
}

/// Prints every pause and tool change of `program`, each line after
/// `indent`.
fn report_pauses(program: &Program, indent: &str) -> Result<(), Failure> {
    if program.pauses.is_empty() {
        return Ok(());
    }
    outln!("{indent}Pauses and tool changes; resume each with cycle start:")?;
    for pause in &program.pauses {
        let message = if pause.message.is_empty() {
            "(no message)"
        } else {
            pause.message.as_str()
        };
        outln!("{indent}  line {}: {message}", pause.line)?;
    }
    Ok(())
}
