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
//! never touch the project. Errors are the CLI's and block the export,
//! counted in one line ([`Ready::blocking_line`]); warnings do not, and
//! the sheet leaves them to the status bar and the drawer. A project validation rejects is built
//! at once, since nothing generates for it, so its errors show without
//! waiting.
//!
//! The plan is pure. The shell runs [`Request::run`] off the UI thread,
//! names the files under the stem the user chose ([`Ready::files`]), and
//! writes them every one or none. The result ([`Ready::result`],
//! capability 77a) shows the CLI's report in three parts, and
//! [`Ready::setup_sheet`] renders the setup sheet (capability 77b).

use std::path::Path;

use dc_job::report::{Line, Part, Written, export_report, export_report_lines};
use dc_job::setup_sheet::{self, Label};
use dc_job::summary::{by_board, describe_estimated_time};
use dc_job::{Built, EmitOp, EstimateConfig, Estimates, Findings, ProgramSplit, ProgramSummary};
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
    /// order they are committed: the programs. The sheet writes no
    /// toolpath preview (capability 77): the Viewport is the preview, its
    /// SVG dump `--preview`'s file ([`crate::scene::Scene::preview_svg`]).
    #[must_use]
    pub fn files(&self, stem: &str) -> Vec<(String, &str)> {
        self.names(stem)
            .into_iter()
            .zip(&self.programs)
            .map(|(name, p)| (name, p.program.text.as_str()))
            .collect()
    }

    /// The one line above the sheet's footer when errors block export
    /// (capability 77): `1 error blocks export`, `3 errors block export`;
    /// `None` when the build found no error.
    #[must_use]
    pub fn blocking_line(&self, strings: &Strings) -> Option<String> {
        match self.findings.errors.len() {
            0 => None,
            1 => Some(strings.get(StrKey::ExportBlockedOne).to_owned()),
            n => Some(strings.fmt(StrKey::ExportBlockedMany, &[("count", &n.to_string())])),
        }
    }

    /// The report `export` prints once these programs are written to
    /// `outputs` (and, from the CLI, the preview to `preview`), line for
    /// line: the result sheet's text.
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

    /// The result of writing these programs as `names` into `folder`
    /// (capability 77a): the CLI's report, every line of it in one of
    /// three parts. Built from the report's tagged lines
    /// ([`dc_job::report::export_report_lines`]), the same lines
    /// [`Ready::report`] gives the CLI's text, so the parts hold the
    /// CLI's words.
    #[must_use]
    pub fn result(&self, names: &[String], folder: &Path, strings: &Strings) -> ExportResult {
        let programs: Vec<dc_job::Program> =
            self.programs.iter().map(|p| p.program.clone()).collect();
        let estimates = Estimates::new(&self.project, &programs, self.options.estimate);
        let written = Written {
            programs: &programs,
            outputs: names,
            split: self.options.split,
            generated: &self.generated,
            preview: None,
            switched: &[],
        };
        let lines = export_report_lines(&self.project, &written, &estimates);
        let count = self.programs.len();
        let headline = if count == 1 {
            strings.get(StrKey::ExportResultWrittenOne).to_owned()
        } else {
            strings.fmt(
                StrKey::ExportResultWrittenMany,
                &[("count", &count.to_string())],
            )
        };
        let mut status = Status {
            headline,
            lines: Vec::new(),
        };
        let mut steps: Vec<Step> = (0..count).map(|_| Step::default()).collect();
        let mut instructions = Instructions::default();
        let mut boards = Vec::new();
        let mut totals = Vec::new();
        for Line { part, text } in lines {
            let text = text.trim().to_owned();
            let program = match part {
                Part::Step(k)
                | Part::Program(k)
                | Part::Extents(k)
                | Part::Estimate(k)
                | Part::Pause(k) => steps.get_mut(k),
                _ => None,
            };
            match (part, program) {
                (Part::Status, _) => status.lines.push(text),
                (Part::Board, _) => boards.push(text),
                (Part::Total, _) => totals.push(text),
                (Part::Note, _) => instructions.notes.push(text),
                (Part::Step(_), Some(step)) => step.before = text,
                (Part::Program(_), Some(step)) => step.heading = Some(text),
                (Part::Extents(_), Some(step)) => step.extents.push(text),
                (Part::Estimate(_), Some(step)) => step.estimate = Some(text),
                (Part::Pause(_), Some(step)) => step.pauses.push(text),
                // Each operation and carve, and nothing else: every
                // program's lines have its step.
                _ => instructions.facts.push(text),
            }
        }
        instructions.steps = steps;
        let not_available = strings.get(StrKey::ExportNotAvailable);
        let mut board = None;
        let mut k_in_board = 0;
        let mut headings = boards.into_iter();
        let rows = self
            .programs
            .iter()
            .zip(names)
            .enumerate()
            .map(|(k, (summary, name))| {
                let program = &summary.program;
                let mut heading = None;
                if self.by_board && board != Some(program.board) {
                    board = Some(program.board);
                    k_in_board = 0;
                    heading = headings.next();
                }
                k_in_board += 1;
                FileRow {
                    number: if self.by_board { k_in_board } else { k + 1 },
                    board: heading,
                    name: name.clone(),
                    tool: strings.fmt(
                        StrKey::ExportTool,
                        &[
                            ("number", &program.first_tool.number.to_string()),
                            ("name", &program.first_tool.name),
                        ],
                    ),
                    time: self.estimate(k).unwrap_or_else(|| not_available.to_owned()),
                }
            })
            .collect();
        // The report heads each board twice, over its steps and over its
        // details; the Files part shows each heading once, over its rows,
        // so the rest are the same text.
        let repeated: Vec<String> = headings.collect();
        ExportResult {
            status,
            files: Files {
                rows,
                repeated_boards: repeated,
                totals,
                folder: folder.display().to_string(),
            },
            instructions,
        }
    }

    /// The setup sheet of these programs written as `names` (capability
    /// 77b): `dc_job::setup_sheet::render` with the export's date, rows,
    /// warnings and split, its labels from `strings`, its footer naming
    /// DreamCarve `version`.
    #[must_use]
    pub fn setup_sheet(&self, names: &[String], version: &str, strings: &Strings) -> String {
        dc_job::setup_sheet::render(
            &setup_sheet::Sheet {
                project: &self.project,
                programs: &self.programs,
                names,
                warnings: &self.findings.warnings,
                split: self.options.split,
                estimate: self.options.estimate,
                date: &self.options.date,
                version,
            },
            &|label| setup_sheet_text(strings, label),
        )
    }
}

/// The export result (functional spec, capability 77a): the sheet's
/// content once the programs are written, in three parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    /// What was written.
    pub status: Status,
    /// The files in run order and their folder.
    pub files: Files,
    /// What to do before each program, the controller notes, and the
    /// rest of the CLI's facts, behind a disclosure.
    pub instructions: Instructions,
}

impl ExportResult {
    /// Every text each part shows, by part: `Status`, `Files`, `Run
    /// instructions`. Each line of the CLI's report is one of them, in
    /// one part only.
    #[must_use]
    pub fn parts(&self) -> [(&'static str, Vec<String>); 3] {
        let status = std::iter::once(self.status.headline.clone())
            .chain(self.status.lines.iter().cloned())
            .collect();
        let mut files = Vec::new();
        for row in &self.files.rows {
            files.extend(row.board.iter().cloned());
            files.extend([
                row.number.to_string(),
                row.name.clone(),
                row.tool.clone(),
                row.time.clone(),
            ]);
        }
        files.extend(self.files.repeated_boards.iter().cloned());
        files.extend(self.files.totals.iter().cloned());
        files.push(self.files.folder.clone());
        let mut instructions = Vec::new();
        for step in &self.instructions.steps {
            instructions.extend(step.heading.iter().cloned());
            instructions.push(step.before.clone());
            instructions.extend(step.extents.iter().cloned());
            instructions.extend(step.estimate.iter().cloned());
            instructions.extend(step.pauses.iter().cloned());
        }
        instructions.extend(self.instructions.notes.iter().cloned());
        instructions.extend(self.instructions.facts.iter().cloned());
        [
            ("Status", status),
            ("Files", files),
            ("Run instructions", instructions),
        ]
    }
}

/// The result's Status part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// `2 programs written`.
    pub headline: String,
    /// The CLI's own lines about what was written: `Wrote …` and
    /// `Post-processor …`.
    pub lines: Vec<String>,
}

/// One program's row in the Files part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// Its place in the run order, counted within its board with an
    /// Inlay.
    pub number: usize,
    /// The board heading over it, when it is its board's first (#498).
    pub board: Option<String>,
    /// Its file name.
    pub name: String,
    /// Its tool: `T2 1/8 end mill`.
    pub tool: String,
    /// Its estimated time as the CLI writes it, or a dash.
    pub time: String,
}

/// The result's Files part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    /// One row per program, in run order.
    pub rows: Vec<FileRow>,
    /// The board headings the report repeats over each board's details.
    pub repeated_boards: Vec<String>,
    /// The boards' and the export's estimated totals, as the CLI prints
    /// them.
    pub totals: Vec<String>,
    /// The folder the files went to.
    pub folder: String,
}

/// What to do before one program, as the CLI prints it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Step {
    /// `plaque-1-T2.nc (1334 lines):`, with several programs.
    pub heading: Option<String>,
    /// The step-before line: the tool to fit and the zero to set.
    pub before: String,
    /// Its extents, tool centre then the cutter's edge.
    pub extents: Vec<String>,
    /// Its `Estimated time: …` line.
    pub estimate: Option<String>,
    /// Its pauses and tool changes.
    pub pauses: Vec<String>,
}

/// The result's Run instructions part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Instructions {
    /// One step per program, in run order.
    pub steps: Vec<Step>,
    /// The controller notes, each a callout.
    pub notes: Vec<String>,
    /// The CLI's other facts: each exported operation, each carve.
    pub facts: Vec<String>,
}

/// `label`'s text in `strings`: the setup sheet's labels are the string
/// table's `[setup_sheet]` entries (acceptance 91).
#[must_use]
pub fn setup_sheet_text(strings: &Strings, label: Label) -> String {
    let key = match label {
        Label::Title => StrKey::SetupSheetTitle,
        Label::Exported => StrKey::SetupSheetExported,
        Label::Post => StrKey::SetupSheetPost,
        Label::PostValue => StrKey::SetupSheetPostValue,
        Label::Units => StrKey::SetupSheetUnits,
        Label::UnitsMm => StrKey::SetupSheetUnitsMm,
        Label::UnitsIn => StrKey::SetupSheetUnitsIn,
        Label::RangeMm => StrKey::SetupSheetRangeMm,
        Label::RangeIn => StrKey::SetupSheetRangeIn,
        Label::Stock => StrKey::SetupSheetStock,
        Label::StockValue => StrKey::SetupSheetStockValue,
        Label::NotSet => StrKey::SetupSheetNotSet,
        Label::XyZero => StrKey::SetupSheetXyZero,
        Label::ZZero => StrKey::SetupSheetZZero,
        Label::XyBottomLeft => StrKey::SetupSheetXyBottomLeft,
        Label::XyBottomRight => StrKey::SetupSheetXyBottomRight,
        Label::XyTopLeft => StrKey::SetupSheetXyTopLeft,
        Label::XyTopRight => StrKey::SetupSheetXyTopRight,
        Label::XyCentre => StrKey::SetupSheetXyCentre,
        Label::XyCustom => StrKey::SetupSheetXyCustom,
        Label::ZStockTop => StrKey::SetupSheetZStockTop,
        Label::ZStockBottom => StrKey::SetupSheetZStockBottom,
        Label::Programs => StrKey::SetupSheetPrograms,
        Label::Program => StrKey::SetupSheetProgram,
        Label::Tool => StrKey::SetupSheetTool,
        Label::ToolValue => StrKey::SetupSheetToolValue,
        Label::ToolType => StrKey::SetupSheetToolType,
        Label::TypeEndMill => StrKey::SetupSheetTypeEndMill,
        Label::TypeBallNose => StrKey::SetupSheetTypeBallNose,
        Label::TypeVBit => StrKey::SetupSheetTypeVBit,
        Label::TypeEngraver => StrKey::SetupSheetTypeEngraver,
        Label::TypeDrill => StrKey::SetupSheetTypeDrill,
        Label::Diameter => StrKey::SetupSheetDiameter,
        Label::Angle => StrKey::SetupSheetAngle,
        Label::AngleValue => StrKey::SetupSheetAngleValue,
        Label::Vendor => StrKey::SetupSheetVendor,
        Label::Product => StrKey::SetupSheetProduct,
        Label::Operations => StrKey::SetupSheetOperations,
        Label::Before => StrKey::SetupSheetBefore,
        Label::Time => StrKey::SetupSheetTime,
        Label::ExtentsCentre => StrKey::SetupSheetExtentsCentre,
        Label::ExtentsEdge => StrKey::SetupSheetExtentsEdge,
        Label::ZRange => StrKey::SetupSheetZRange,
        Label::Pauses => StrKey::SetupSheetPauses,
        Label::Warnings => StrKey::SetupSheetWarnings,
        Label::Warning => StrKey::SetupSheetWarning,
        Label::Notes => StrKey::SetupSheetNotes,
        Label::Footer => StrKey::SetupSheetFooter,
    };
    strings.get(key).to_owned()
}

/// The file name `export --preview` gives the preview under `stem`:
/// `<stem>-preview.svg`.
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
