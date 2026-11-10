// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The export plan (`docs/specs/phase-4/design.md`, "Architecture →
//! Export"; functional spec, capability 77 and S20): what the export sheet
//! lists and writes.
//!
//! [`plan`] waits while any operation is pending, generating or stale
//! (the Viewport's drawing is then not the snapshot's), then builds the
//! current snapshot afresh with [`dc_job::build_board`], the call
//! `dreamcarve export` makes, rather than reusing the incremental results,
//! so the bytes the sheet writes are `build`'s. The program split
//! override and the board ticks are this export's only (`--one-program`,
//! `--programs-per-tool`, `--board`): they are arguments of the build and
//! never touch the project. Errors are the CLI's and block the export;
//! warnings are listed and do not. A project validation rejects is built
//! at once, since nothing generates for it, so its errors show without
//! waiting.
//!
//! The plan is pure. The shell runs [`Request::run`] off the UI thread,
//! names the files under the stem the user chose ([`Ready::files`]), and
//! writes them every one or none.

use dc_job::report::{Written, export_report};
use dc_job::summary::{by_board, describe_estimated_time};
use dc_job::{
    Built, EmitOp, EstimateConfig, Estimates, Findings, ProgramSplit, ProgramSummary, Tier,
};
use dc_project::{Board, Project};

use crate::app::App;
use crate::generation::SnapshotId;
use crate::strings::{StrKey, Strings};

/// Which boards of a project with an Inlay this export writes (#498): the
/// sheet's per-board ticks, for this export only. A project without an
/// enabled Inlay has one board and ignores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boards {
    /// The stock: every operation but an Inlay's male.
    pub female: bool,
    /// The Inlay's male board.
    pub male: bool,
}

impl Default for Boards {
    fn default() -> Self {
        Self {
            female: true,
            male: true,
        }
    }
}

impl Boards {
    /// The board `build_board` is asked for: `None` for both, `Some` for
    /// one; `Err(())` when neither is ticked.
    fn board(self) -> Result<Option<Board>, ()> {
        match (self.female, self.male) {
            (true, true) => Ok(None),
            (true, false) => Ok(Some(Board::Female)),
            (false, true) => Ok(Some(Board::Male)),
            (false, false) => Err(()),
        }
    }
}

/// What the export sheet asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The program split: as the project says, or overridden for this
    /// export.
    pub split: ProgramSplit,
    /// The boards ticked.
    pub boards: Boards,
    /// The date every program header is written with.
    pub date: String,
    /// The user's `[estimate]` settings, for the estimates.
    pub estimate: EstimateConfig,
}

impl Options {
    /// The program split as the project says and both boards, programs
    /// dated `date`.
    #[must_use]
    pub fn new(date: &str, estimate: EstimateConfig) -> Self {
        Self {
            split: ProgramSplit::ProjectDefault,
            boards: Boards::default(),
            date: date.to_owned(),
            estimate,
        }
    }
}

/// The export plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// An operation is pending, generating or stale: export waits for
    /// generation (S20).
    Waiting,
    /// Built.
    Ready(Box<Ready>),
}

/// A build of one snapshot, ready to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    snapshot: SnapshotId,
    project: Project,
    options: Options,
}

impl Request {
    /// The snapshot this request builds.
    #[must_use]
    pub fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    /// The options it builds with.
    #[must_use]
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Builds the snapshot as `dreamcarve export` does, and summarizes
    /// each program as the sheet lists it.
    #[must_use]
    pub fn run(self) -> Ready {
        let Self {
            snapshot,
            project,
            options,
        } = self;
        let grouped = by_board(&project);
        let board = if grouped {
            options.boards.board()
        } else {
            Ok(None)
        };
        let Ok(board) = board else {
            return Ready {
                snapshot,
                project,
                options,
                programs: Vec::new(),
                findings: Findings::default(),
                preview: None,
                by_board: grouped,
                total_line: None,
                generated: Vec::new(),
                no_board: true,
            };
        };
        let Built {
            programs,
            findings,
            generated,
        } = dc_job::build_board(&project, options.split, &options.date, board);
        // `build` generated every operation when it emitted programs.
        let preview = generated
            .as_deref()
            .filter(|_| !programs.is_empty())
            .map(|g| dc_job::preview::render_generated(&project, g));
        let generated = generated.unwrap_or_default();
        let (programs, total_line) = dc_job::summarize(
            &project,
            &generated,
            programs,
            options.split,
            &options.estimate,
        );
        Ready {
            snapshot,
            project,
            options,
            programs,
            findings,
            preview,
            by_board: grouped,
            total_line,
            generated,
            no_board: false,
        }
    }
}

/// What an export of one snapshot writes, and what it found.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    /// The snapshot built.
    pub snapshot: SnapshotId,
    project: Project,
    /// The options it was built with.
    pub options: Options,
    /// One row per program, in the order they are run, each with its
    /// program; empty when an error was found.
    pub programs: Vec<ProgramSummary>,
    /// The build's errors and warnings, as `export` prints them.
    pub findings: Findings,
    /// The toolpath preview, `--preview`'s picture of this build, when
    /// programs were emitted.
    pub preview: Option<String>,
    /// Whether the programs are listed and named by board (an enabled
    /// Inlay, #498).
    pub by_board: bool,
    /// The `Estimated total time: …` line, with several programs.
    pub total_line: Option<String>,
    generated: Vec<EmitOp>,
    /// Whether neither board was ticked.
    no_board: bool,
}

impl Ready {
    /// Why *Export* is disabled: the first error, as the CLI says it, or
    /// that no board is ticked; `None` when the programs can be written.
    #[must_use]
    pub fn blocked(&self) -> Option<String> {
        if self.no_board {
            return Some(Strings::english().get(StrKey::ExportNoBoard).to_owned());
        }
        if let Some(error) = self.findings.errors.first() {
            return Some(error.message.clone());
        }
        // `build` names an error whenever it emits nothing.
        self.programs
            .is_empty()
            .then(|| Strings::english().get(StrKey::ExportNothing).to_owned())
    }

    /// The project snapshot built.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Each program's file name under `stem`, as `export` names them
    /// beside a project of that stem: `<stem>.<ext>`, `<stem>-<k>-T<n>.<ext>`,
    /// or by board with an Inlay.
    #[must_use]
    pub fn names(&self, stem: &str) -> Vec<String> {
        let programs: Vec<dc_job::Program> =
            self.programs.iter().map(|p| p.program.clone()).collect();
        dc_job::program_names(&self.project, stem, &programs)
    }

    /// The files to write under `stem`, each name with its text, in the
    /// order they are committed: the programs, then the preview when
    /// `preview` is asked for and there is one.
    #[must_use]
    pub fn files(&self, stem: &str, preview: bool) -> Vec<(String, &str)> {
        let mut files: Vec<(String, &str)> = self
            .names(stem)
            .into_iter()
            .zip(&self.programs)
            .map(|(name, p)| (name, p.program.text.as_str()))
            .collect();
        if preview && let Some(svg) = &self.preview {
            files.push((preview_name(stem), svg.as_str()));
        }
        files
    }

    /// The report `export` prints once these programs are written to
    /// `outputs` (and the preview to `preview`), line for line: the
    /// result sheet's text.
    #[must_use]
    pub fn report(&self, outputs: &[String], preview: Option<&str>) -> Vec<String> {
        let programs: Vec<dc_job::Program> =
            self.programs.iter().map(|p| p.program.clone()).collect();
        let estimates = Estimates::new(&self.project, &programs, self.options.estimate);
        let written = Written {
            programs: &programs,
            outputs,
            split: self.options.split,
            generated: &self.generated,
            preview,
            switched: &[],
        };
        export_report(&self.project, &written, &estimates)
    }

    /// Program `k`'s estimated time as the CLI writes it (`9 min 40 s`),
    /// when the post-processor could read the program back.
    #[must_use]
    pub fn estimate(&self, k: usize) -> Option<String> {
        let estimate = self.programs.get(k)?.estimate?;
        Some(describe_estimated_time(&estimate, &self.options.estimate))
    }

    /// The findings to list, errors first, each with its tier.
    #[must_use]
    pub fn listed(&self) -> Vec<&dc_job::Finding> {
        self.findings
            .errors
            .iter()
            .chain(&self.findings.warnings)
            .collect()
    }

    /// Whether any listed finding is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.listed().iter().any(|f| f.tier == Tier::Error)
    }
}

/// The preview's file name under `stem`: `<stem>-preview.svg`.
#[must_use]
pub fn preview_name(stem: &str) -> String {
    format!("{stem}-preview.svg")
}

/// Whether export must wait for `app`'s generation: validation passed and
/// an operation is pending, generating or stale, or the project-level
/// pass is due. A project validation rejects never generates, so it
/// waits for nothing.
#[must_use]
pub fn waits(app: &App) -> bool {
    if app.validation().has_errors() {
        return false;
    }
    let generation = app.generation();
    generation.busy()
        || app
            .project()
            .operations
            .iter()
            .filter(|op| op.enabled())
            .any(|op| generation.is_stale(op.id()))
}

/// The build request for `app`'s current snapshot with `options`; `None`
/// while export waits ([`waits`]).
#[must_use]
pub fn request(app: &App, options: &Options) -> Option<Request> {
    (!waits(app)).then(|| Request {
        snapshot: app.snapshot(),
        project: app.project().clone(),
        options: options.clone(),
    })
}

/// The export plan for `app`'s current snapshot ([`request`], run here).
#[must_use]
pub fn plan(app: &App, options: &Options) -> Plan {
    match request(app, options) {
        Some(request) => Plan::Ready(Box::new(request.run())),
        None => Plan::Waiting,
    }
}

/// The estimated time of every program of `app`'s current snapshot, as
/// the CLI writes a time (`13 min 35 s`), with the user's `settings`:
/// what the status bar shows (capability 78). `None` until the
/// project-level pass is in, or when a program has no estimate.
#[must_use]
pub fn total_time(app: &App, settings: &EstimateConfig) -> Option<String> {
    let finished = app.generation().finished()?;
    if finished.programs.is_empty() {
        return None;
    }
    let total = finished
        .programs
        .iter()
        .try_fold(dc_job::Estimate::default(), |sum, p| {
            Some(sum + p.estimate?)
        })?;
    Some(describe_estimated_time(&total, settings))
}

/// The programs of `app`'s current snapshot that cut operation `op`, each
/// with its estimated time as the CLI writes it: an operation row's hover
/// (capability 78). The CLI estimates whole programs, so an operation
/// shows the time of the program it is cut in.
#[must_use]
pub fn operation_times(
    app: &App,
    op: &dc_project::OperationId,
    settings: &EstimateConfig,
) -> Vec<(String, String)> {
    let Some(finished) = app.generation().finished() else {
        return Vec::new();
    };
    finished
        .programs
        .iter()
        .filter(|p| p.operations.contains(op))
        .filter_map(|p| {
            let estimate = p.estimate?;
            Some((p.name.clone(), describe_estimated_time(&estimate, settings)))
        })
        .collect()
}
