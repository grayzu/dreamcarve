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
//! every program (with `--preview`, the toolpath picture of addendum A5,
//! and with `--post`, the updated project) is staged in a
//! temporary file, and all are renamed into place at the end. If one
//! rename fails, the files already moved are put back as they were.
//!
//! [`build`] is the whole pipeline without writing; `check` runs it too, so
//! `check` reports exactly what `export` would. It validates the project it
//! is given rather than trusting the problems found at load, so the checks
//! that depend on the post-processor see the one `--post` swapped in.
//!
//! A finding about one value of the file is printed as
//! `coaster.dcv:9:21: error: …`, the form editors and terminals turn into a
//! link to that line and column (#76). Every warning carries its stable
//! `snake_case` code and prints as `warning: <code>: <message>` (functional
//! spec, capability 25); the Warnings chapter lists the codes. After
//! generation, `dc_cam::order::check` adds the operation-order warnings,
//! and `dc_cam::placed_tabs::check` the tabs no Profile cuts round (#105).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dc_cam::{
    Toolpath, generate_drill, generate_engrave, generate_pocket, generate_profile, order,
    placed_tabs,
};
use dc_gcode::format::format_number;
use dc_gcode::transform::mm_per_unit;
use dc_gcode::{AxisRange, EmitOp, Estimate, ExportWarning, Program, ProgramSplit, emit_programs};
use dc_project::{
    Operation, PostProcessorRef, Project, ResetReason, SettingReset, Severity, SourceMap, Tool,
    ToolChangeMode, Units, validate,
};

use crate::cli::{
    ExportArgs, Failure, StagedFile, commit_all, lookup_post, read_file, switch_post, today_utc,
};
use crate::config::{self, EstimateConfig};
use crate::output::{errln, outln};
use crate::preview;

/// One error or warning.
#[derive(Debug)]
pub struct Finding {
    /// The field it is about, as validation names it
    /// (`tools[t1].feed_mm_min`), when it is about one.
    pub field: Option<String>,
    /// The finding's stable `snake_case` code, printed before the
    /// message. Every warning has one; errors print their message alone.
    pub code: Option<String>,
    /// What is wrong.
    pub message: String,
}

impl Finding {
    /// A finding about no one field, with no code.
    fn plain(message: String) -> Self {
        Self {
            field: None,
            code: None,
            message,
        }
    }

    /// A warning with code `code`, about no one field.
    fn coded(code: &str, message: String) -> Self {
        Self {
            field: None,
            code: Some(code.to_owned()),
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

/// The pipeline's result: the programs when there were no errors.
pub struct Built {
    /// The emitted programs, in the order they are run: one per tool run
    /// or one in all (#95). Empty when any error was found.
    pub programs: Vec<Program>,
    /// Errors and warnings.
    pub findings: Findings,
    /// Every enabled operation's generated toolpath, with its tool, when
    /// every one of them generated, even if emitting the programs then
    /// failed; `None` when one did not. What `--preview` draws.
    pub generated: Option<Vec<EmitOp>>,
}

/// Validates `project`, generates every enabled operation's toolpath, and
/// emits the programs, collecting every error and warning on the way.
/// `split` is how a multi-tool project is divided into programs (the
/// project's mode, or an `export` flag); `date` fills each program
/// header's `{date}`.
pub fn build(project: &Project, split: ProgramSplit, date: &str) -> Built {
    let mut findings = Findings::default();
    for problem in validate(project) {
        let severity = problem.severity();
        let finding = Finding {
            field: problem.field(),
            code: (severity == Severity::Warning).then(|| problem.code().to_owned()),
            message: problem.to_string(),
        };
        match severity {
            Severity::Error => findings.errors.push(finding),
            Severity::Warning => findings.warnings.push(finding),
        }
    }
    if !findings.errors.is_empty() {
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
        let Some(tool) = project.tools.iter().find(|t| &t.id == operation.tool()) else {
            findings.errors.push(Finding::plain(format!(
                "operation `{}` uses tool `{}`, which is not in the project",
                operation.id(),
                operation.tool()
            )));
            continue;
        };
        let plain = |toolpath: Toolpath| (toolpath, None);
        let generated = match operation {
            Operation::Profile(op) => generate_profile(project, op).map(|(t, w)| (plain(t), w)),
            Operation::Pocket(op) => generate_pocket(project, op).map(|(t, w)| (plain(t), w)),
            Operation::Engrave(op) => generate_engrave(project, op).map(|(t, w)| (plain(t), w)),
            // The hole records go to the emitter, which writes them as
            // canned cycles when the project asks and the controller's
            // motion for them is the toolpath.
            Operation::Drill(op) => generate_drill(project, op)
                .map(|(drilled, w)| ((drilled.toolpath, Some(drilled.holes)), w)),
        };
        match generated {
            Ok(((toolpath, holes), warnings)) => {
                findings.warnings.extend(warnings.iter().map(|w| {
                    Finding::coded(
                        w.code().as_str(),
                        format!("operation `{}`: {w}", operation.id()),
                    )
                }));
                ops.push(EmitOp {
                    op: operation.clone(),
                    tool: tool.clone(),
                    toolpath,
                    holes,
                });
            }
            Err(e) => findings.errors.push(Finding::plain(e.to_string())),
        }
    }
    // The order check runs on whatever generated, so `check` reports it
    // even when another operation failed; it never needs a failed one.
    let generated: Vec<(&Operation, &Toolpath)> = ops
        .iter()
        .map(|entry| (&entry.op, &entry.toolpath))
        .collect();
    // #105: a placed tab no enabled Profile cuts round holds nothing.
    let project_warnings = order::check(project, &generated)
        .into_iter()
        .chain(placed_tabs::check(project, &generated));
    findings.warnings.extend(project_warnings.map(|w| {
        let message = match w.op() {
            Some(op) => format!("operation `{op}`: {w}"),
            None => w.to_string(),
        };
        Finding::coded(w.code().as_str(), message)
    }));
    if !findings.errors.is_empty() {
        // An operation did not generate.
        return Built {
            programs: Vec::new(),
            findings,
            generated: None,
        };
    }

    match emit_programs(project, &ops, split, date) {
        Ok((programs, warnings)) => {
            findings.warnings.extend(
                warnings
                    .iter()
                    .map(|w| Finding::coded(w.code(), describe_export_warning(w))),
            );
            Built {
                programs,
                findings,
                generated: Some(ops),
            }
        }
        Err(e) => {
            findings.errors.push(Finding::plain(e.to_string()));
            Built {
                programs: Vec::new(),
                findings,
                generated: Some(ops),
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
        ExportWarning::CannedCycleExpanded { op, reason } => {
            format!("operation `{op}`: written as plain moves, not canned cycles, because {reason}")
        }
        ExportWarning::UnitsSetByController { post, units } => {
            let units = match units {
                Units::Mm => "millimetres",
                Units::Inch => "inches",
            };
            format!(
                "post-processor `{post}` has no units words (G20/G21), so the program has none; it is in {units}, so run it only on a machine set up in {units}"
            )
        }
        ExportWarning::ArcFitFallback { op, runs } => format!(
            "operation `{op}`: {runs} run(s) of straight moves kept as written because the controller would reject the arc fitted to them once rounded"
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
    } = build(&project, args.split(), &date);
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

    let outputs = output_paths(args.output.as_deref(), &args.dcv, &programs)?;
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
            preview_svg(&project, generated).as_bytes(),
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
    match (programs.as_slice(), outputs.as_slice()) {
        ([program], [output]) => report(
            &project,
            program,
            output,
            args,
            switched.as_ref(),
            &estimates,
        )?,
        _ => report_runs(
            &project,
            &programs,
            &outputs,
            args,
            switched.as_ref(),
            &estimates,
        )?,
    }
    if let Some(preview) = &args.preview {
        outln!("Wrote preview {}.", preview.display())?;
    }
    Ok(ExitCode::SUCCESS)
}

/// The preview SVG of the generated operations (addendum A5).
pub fn preview_svg(project: &Project, generated: &[EmitOp]) -> String {
    let ops: Vec<(&Operation, &Tool, &Toolpath)> = generated
        .iter()
        .map(|entry| (&entry.op, &entry.tool, &entry.toolpath))
        .collect();
    preview::render(project, &ops)
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

/// The cycle-time estimate of each program (functional spec, capability
/// 30), with the user's `[estimate]` settings to print them with. It reads
/// the programs and changes nothing.
pub struct Estimates {
    /// Each program's estimate, in program order, or why there is none.
    per_program: Vec<Result<Estimate, String>>,
    settings: EstimateConfig,
}

impl Estimates {
    /// The estimates of `programs`, emitted from `project`.
    pub fn new(project: &Project, programs: &[Program], settings: EstimateConfig) -> Self {
        let post = project.post_processor.resolve().map_err(|e| e.to_string());
        let per_program = programs
            .iter()
            .map(|program| {
                let post = post.as_ref().map_err(Clone::clone)?;
                dc_gcode::estimate(program, post).map_err(|e| e.to_string())
            })
            .collect();
        Self {
            per_program,
            settings,
        }
    }

    /// The `Estimated time: …` line of program `k`.
    pub fn program_line(&self, k: usize) -> String {
        match self.per_program.get(k) {
            Some(Ok(estimate)) => describe_estimate("Estimated time", estimate, &self.settings),
            Some(Err(reason)) => format!("Estimated time: not available ({reason})."),
            None => "Estimated time: not available.".to_owned(),
        }
    }

    /// The `Estimated total time: …` line over every program, when there
    /// is more than one.
    pub fn total_line(&self) -> Option<String> {
        if self.per_program.len() < 2 {
            return None;
        }
        let total = self
            .per_program
            .iter()
            .try_fold(Estimate::default(), |sum, e| {
                e.as_ref().ok().map(|e| sum + *e)
            });
        Some(match total {
            Some(total) => describe_estimate("Estimated total time", &total, &self.settings),
            None => "Estimated total time: not available.".to_owned(),
        })
    }
}

/// `estimate` as one line, as capability 30 shows it: the total, then the
/// cutting and dwell times, the rapids' time when the user has given a
/// rapid rate or else their distance and how to include it, the scale
/// factor when it is not 1, and what is left out: acceleration, and the
/// time at each pause and tool change, counted. Every time is multiplied
/// by the scale factor.
fn describe_estimate(label: &str, estimate: &Estimate, settings: &EstimateConfig) -> String {
    let scale = settings.scale_factor;
    let cutting_s = estimate.feed_s * scale;
    let dwells_s = estimate.dwell_s * scale;
    let rapids = settings
        .rapid_mm_min
        .map(|rate| (estimate.rapid_mm / rate * SECONDS_PER_MINUTE * scale, rate));
    let total_s = cutting_s + dwells_s + rapids.map_or(0.0, |(s, _)| s);
    let mut parts = format!(
        "cutting {}, dwells {}",
        duration(cutting_s),
        duration(dwells_s)
    );
    if let Some((rapids_s, rate)) = rapids {
        parts.push_str(&format!(", rapids {} at {rate} mm/min", duration(rapids_s)));
    }
    if (scale - 1.0).abs() > f64::EPSILON {
        parts.push_str(&format!("; scale factor {scale}"));
    }
    let mut line = format!("{label}: {} ({parts})", duration(total_s));
    if rapids.is_none() && estimate.rapid_mm > 0.0 {
        line.push_str(&format!(
            "; {} of rapids not included: set [estimate] rapid_mm_min in config.toml",
            distance(estimate.rapid_mm)
        ));
    }
    line.push_str(". Excludes acceleration");
    match estimate.stops {
        0 => {}
        1 => line.push_str(" and 1 pause"),
        n => line.push_str(&format!(" and {n} pauses")),
    }
    line.push('.');
    line
}

const SECONDS_PER_MINUTE: f64 = 60.0;

/// `seconds`, rounded to the second, as `1 h 2 min 3 s`, `14 min 5 s` or
/// `13 s`.
fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round();
    let hours = (total / 3600.0).floor();
    let minutes = ((total - hours * 3600.0) / 60.0).floor();
    let secs = total - hours * 3600.0 - minutes * 60.0;
    if hours > 0.0 {
        format!("{hours:.0} h {minutes:.0} min {secs:.0} s")
    } else if minutes > 0.0 {
        format!("{minutes:.0} min {secs:.0} s")
    } else {
        format!("{secs:.0} s")
    }
}

/// `mm` as `50 mm`, or from a metre up as `4.3 m`.
fn distance(mm: f64) -> String {
    if mm < 1000.0 {
        format!("{mm:.0} mm")
    } else {
        format!("{:.1} m", mm / 1000.0)
    }
}

/// Where each program of the project `dcv` goes. One program goes to
/// `output` (`-o`), or to the project's path with the post-processor's
/// extension. Programs of tool runs (#95) go beside it, named
/// `<stem>-<k>-T<n>.<ext>`: `<k>` the run's place, `<n>` its tool's
/// number, `<stem>` and `<ext>` those of that one path.
pub fn output_paths(
    output: Option<&Path>,
    dcv: &Path,
    programs: &[Program],
) -> Result<Vec<PathBuf>, Failure> {
    let single = match (output, programs.first()) {
        (Some(path), _) => path.to_path_buf(),
        // `build` validated the extension of the post-processor it emitted
        // with, so it is safe to append (`is_safe_file_extension`).
        (None, Some(program)) => dcv.with_extension(&program.extension),
        (None, None) => dcv.to_path_buf(),
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
/// per exported operation, the program's extents (#62), its estimated run
/// time (capability 30), the tool to fit and set work zero with before
/// starting (#57), then every pause and tool change with its line number
/// and message, because the controller or sender may not show the message
/// (design step 8).
fn report(
    project: &Project,
    program: &Program,
    output: &Path,
    args: &ExportArgs,
    switched: Option<&Switched>,
    estimates: &Estimates,
) -> Result<(), Failure> {
    outln!(
        "Wrote {} ({} lines, post-processor {}).",
        output.display(),
        program.text.lines().count(),
        project.post_processor.id
    )?;
    report_post_and_operations(project, args, switched)?;
    for line in describe_extents(project, program, largest_tool_radius_mm(project)) {
        outln!("{line}")?;
    }
    outln!("{}", estimates.program_line(0))?;
    // #57: the program does not stop for the first tool.
    outln!(
        "Before you start: fit T{}{} and set work zero with it.",
        program.first_tool.number,
        name_suffix(&program.first_tool.name)
    )?;
    report_tool_length_offset(project)?;
    report_pauses(program, "")?;
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
        outln!(
            "This program changes tools with M6, so your sender must stop at M6; see \"One program with M6\" in the post-processors chapter."
        )?;
    }
    Ok(())
}

/// Prints the export summary of the programs of tool runs (#95): the files
/// in the order to run them, with what to do before each, then one line
/// per exported operation, then each program's extents (#62), estimated
/// run time and pauses, then the total run time (capability 30).
fn report_runs(
    project: &Project,
    programs: &[Program],
    outputs: &[PathBuf],
    args: &ExportArgs,
    switched: Option<&Switched>,
    estimates: &Estimates,
) -> Result<(), Failure> {
    outln!("Wrote {} programs; run them in this order:", programs.len())?;
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
        outln!(
            "  {}. {}  Before you start: {before}",
            k + 1,
            output.display()
        )?;
    }
    report_tool_length_offset(project)?;
    outln!("Post-processor {}.", project.post_processor.id)?;
    report_post_and_operations(project, args, switched)?;
    for (k, (program, output)) in programs.iter().zip(outputs).enumerate() {
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
    if let Some(total) = estimates.total_line() {
        outln!("{total}")?;
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
        outln!(
            "  {} `{}`: tool `{}` ({tool}), {} target(s), {depth} mm deep",
            op.id(),
            op.name(),
            op.tool(),
            op.targets().len(),
        )?;
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
/// The tool is found by id: its `T` number, once set, is no place in the
/// tool list (#120).
pub fn tool_radius_mm(project: &Project, program: &Program) -> f64 {
    project
        .tools
        .iter()
        .find(|t| t.id == program.first_tool.id)
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
}
