// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Export: the Export step's choices, its plan and the writing of the
//! programs (functional spec, capability 77 and S20;
//! `docs/specs/phase-4/redesign.md`, "The steps";
//! `docs/specs/phase-4/design.md`, "Architecture → Export").
//!
//! [`Export`] is the step's state, which the window keeps for the project
//! that is open: the destination folder (remembered per project in
//! `gui.toml`, beside the project by default) and file name, the program
//! split as the project says or overridden for this export, a tick per
//! board with an Inlay, the plan `dc_app::export::plan` built for the
//! current snapshot, and the result of the last export. The plan is
//! built on a thread of its own once generation has finished, and only
//! while the Export step shows, so the window keeps answering.
//!
//! [`plan`] draws the step's lower half: the program split (where the
//! project cuts with several tools), the boards, *Program Details* (a
//! disclosure, closed until the user opens it, holding one row per
//! program with the step before it), and the destination with *Choose…*
//! and the names it will write. The post-processor is the step's own
//! Post-processor group ([`crate::panels::project::output`]). The step
//! lists no warnings (the status bar counts them) and no estimated total
//! (its readiness card shows it).
//!
//! The header's *Export G-code* runs [`export`]: it stages every program
//! and moves them into place with `dreamcarve_shell::commit_all`, every
//! one or none, after the CLI's check: never over the project file. The
//! project file is never written. Until then the button is disabled with
//! [`Export::refusal`] as its tooltip. The result (capability 77a) takes
//! the plan's place ([`crate::panels::result`]) until *Done*, or until
//! the project changes.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use dc_app::export::{Boards, ExportResult, Options, Ready};
use dc_app::project::{self as project_model, Offer};
use dc_app::status::counted;
use dc_app::{App, SnapshotId, StrKey, Strings};
use dc_job::{EstimateConfig, ProgramSplit, Tier};
use dc_project::ToolChangeMode;
use dreamcarve_shell::{StagedFile, commit_all};
use eframe::egui::{self, Align, Id, Layout, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::row::{self, GAP_PX};
use crate::widgets::section::section;
use crate::widgets::spinner::spinner;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// How soon the window looks for the plan again while it is building.
pub(crate) const POLL: Duration = Duration::from_millis(50);
/// *Program Details*' name among `gui.toml`'s sections: closed until the
/// user opens it.
const DETAILS: &str = "export.details";
/// The gutter a program's number stands in, px.
const NUMBER_PX: f32 = 12.0;

/// What the Export step asks the shell for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    /// The result's *Done*: back to the plan.
    Done,
    /// Pick the destination folder.
    ChooseFolder,
    /// Write the files, asking for a folder first when none is chosen
    /// (the header's *Export G-code*).
    Export,
    /// Open the folder the files went to.
    ShowInFolder,
    /// Open the Warnings drawer at the errors that block export.
    ShowErrors,
    /// The result's folder was copied: say so.
    Copied,
    /// The file name, typed.
    SetStem(String),
    /// The program split for this export.
    SetSplit(ProgramSplit),
    /// The boards ticked for this export.
    SetBoards(Boards),
    /// The result's *Run instructions*, opened or closed.
    ToggleInstructions,
}

/// The plan being built off the UI thread, and what it was asked for.
struct Planning {
    key: (SnapshotId, Options),
    results: Receiver<Ready>,
}

/// What an export wrote: its result and its setup sheet.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Done {
    /// The snapshot written: the result shows while it is the app's.
    snapshot: SnapshotId,
    dir: PathBuf,
    /// The CLI's report, in three parts.
    pub result: ExportResult,
    setup: SetupSheet,
    /// Whether *Run instructions* is open: closed when the result shows.
    pub instructions: bool,
}

/// The setup sheet of the last export (capability 77b): the page, and
/// where *Save setup sheet…* offers to put it, beside the programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SetupSheet {
    /// The page.
    pub html: String,
    /// The folder the programs went to.
    pub dir: PathBuf,
    /// `<stem>-setup.html`.
    pub file_name: String,
}

/// The Export step's state between frames.
pub(crate) struct Export {
    /// The folder the files go to; `None` for a project never saved and
    /// no folder chosen.
    dir: Option<PathBuf>,
    /// Whether the user chose the folder in this session: it then stays
    /// when the project is saved somewhere else.
    dir_chosen: bool,
    /// What each file's name starts with.
    stem: String,
    /// The project file the folder and the name were taken from.
    source: Option<PathBuf>,
    split: ProgramSplit,
    boards: Boards,
    /// The date the programs are headed with: today's.
    date: String,
    estimate: EstimateConfig,
    /// The plan of the current snapshot and options, once built.
    ready: Option<Box<Ready>>,
    planning: Option<Planning>,
    /// Why the last export wrote nothing.
    failure: Option<String>,
    /// The result, once written.
    done: Option<Done>,
}

impl std::fmt::Debug for Export {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Export")
            .field("dir", &self.dir)
            .field("stem", &self.stem)
            .field("split", &self.split)
            .field("planning", &self.planning.is_some())
            .finish_non_exhaustive()
    }
}

/// The folder `app`'s project exports to before the user chooses one: the
/// one remembered for it, else its own.
fn default_dir(app: &App) -> Option<PathBuf> {
    let source = app.source();
    source
        .and_then(|path| app.prefs().export_dirs.get(path).cloned())
        .or_else(|| source.and_then(Path::parent).map(Path::to_path_buf))
        .filter(|dir| !dir.as_os_str().is_empty())
}

/// What `app`'s programs are named after: its file's stem, else its name.
fn default_stem(app: &App) -> String {
    app.source().and_then(Path::file_stem).map_or_else(
        || app.project().name.clone(),
        |s| s.to_string_lossy().into_owned(),
    )
}

impl Export {
    /// The step's state for `app`'s project: the folder remembered for
    /// it, else its own; the name its file's, else the project's.
    pub(crate) fn new(app: &App, date: String, estimate: EstimateConfig) -> Self {
        Self {
            dir: default_dir(app),
            dir_chosen: false,
            stem: default_stem(app),
            source: app.source().map(Path::to_path_buf),
            split: ProgramSplit::ProjectDefault,
            boards: Boards::default(),
            date,
            estimate,
            ready: None,
            planning: None,
            failure: None,
            done: None,
        }
    }

    /// Follows the project to a new file (Save As, the first Save): the
    /// name is the new file's, and so is the folder unless the user chose
    /// one.
    pub(crate) fn follow(&mut self, app: &App) {
        if app.source() == self.source.as_deref() {
            return;
        }
        self.source = app.source().map(Path::to_path_buf);
        self.stem = default_stem(app);
        if !self.dir_chosen {
            self.dir = default_dir(app);
        }
        self.failure = None;
    }

    fn options(&self) -> Options {
        Options {
            split: self.split,
            boards: self.boards,
            date: self.date.clone(),
            estimate: self.estimate,
        }
    }

    /// The plan, when it answers `app`'s current snapshot and the options
    /// shown.
    fn current(&self, app: &App) -> Option<&Ready> {
        self.ready
            .as_deref()
            .filter(|r| r.snapshot == app.snapshot() && r.options == self.options())
    }

    /// Whether the plan of the current snapshot is being built or due.
    pub(crate) fn planning(&self, app: &App) -> bool {
        self.done.is_none()
            && (self.planning.is_some()
                || (!dc_app::export::waits(app) && self.current(app).is_none()))
    }

    /// One frame's turn while the Export step shows, on `today`'s date:
    /// a result the project has moved on from is let go, the finished
    /// plan taken in, and a new one started once generation is over and
    /// the plan in hand is not the current snapshot's. Whether the window
    /// should look again soon.
    pub(crate) fn tick(&mut self, app: &App, today: String) -> bool {
        if self.date != today {
            self.date = today;
        }
        if self
            .done
            .as_ref()
            .is_some_and(|done| done.snapshot != app.snapshot())
        {
            self.done = None;
        }
        if self.done.is_some() {
            return false;
        }
        if let Some(planning) = &self.planning {
            match planning.results.try_recv() {
                Ok(ready) => {
                    self.ready = Some(Box::new(ready));
                    self.planning = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.planning = None,
            }
        }
        let Some(request) = dc_app::export::request(app, &self.options()) else {
            return false;
        };
        let key = (request.snapshot(), request.options().clone());
        if self.current(app).is_some() {
            return false;
        }
        if self.planning.as_ref().is_some_and(|p| p.key == key) {
            return true;
        }
        // Another plan is due, so what the last export said of the old
        // one no longer stands.
        self.failure = None;
        let (send, results) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("export-plan".into())
            .spawn(move || {
                let _ = send.send(request.run());
            });
        match started {
            Ok(_) => self.planning = Some(Planning { key, results }),
            // No thread: build here, once; the window waits this frame.
            Err(_) => {
                if let Some(request) = dc_app::export::request(app, &self.options()) {
                    self.ready = Some(Box::new(request.run()));
                }
                self.planning = None;
            }
        }
        true
    }

    /// The folder chosen in the folder dialog.
    pub(crate) fn set_dir(&mut self, dir: PathBuf) {
        self.dir = Some(dir);
        self.dir_chosen = true;
        self.failure = None;
    }

    /// The folder the files go to.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The file name, typed.
    pub(crate) fn set_stem(&mut self, stem: String) {
        self.stem = stem;
        self.failure = None;
    }

    /// The program split for this export.
    pub(crate) fn set_split(&mut self, split: ProgramSplit) {
        self.split = split;
    }

    /// The boards ticked for this export.
    pub(crate) fn set_boards(&mut self, boards: Boards) {
        self.boards = boards;
    }

    /// The result of the last export, while it shows.
    pub(crate) fn done(&self) -> Option<&Done> {
        self.done.as_ref()
    }

    /// Lets the result go: the plan shows again.
    pub(crate) fn dismiss(&mut self) {
        self.done = None;
        self.failure = None;
    }

    /// Opens or closes the result's *Run instructions*.
    pub(crate) fn toggle_instructions(&mut self) {
        if let Some(done) = &mut self.done {
            done.instructions = !done.instructions;
        }
    }

    /// Why the last export wrote nothing, when it did not.
    pub(crate) fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// The setup sheet of what the last export wrote, while its result
    /// shows.
    pub(crate) fn setup_sheet(&self) -> Option<&SetupSheet> {
        self.done.as_ref().map(|done| &done.setup)
    }

    /// Whether the plan is not yet there to read: generation is under
    /// way, or the programs are being built.
    fn busy(&self, app: &App) -> bool {
        dc_app::export::waits(app) || self.planning(app)
    }

    /// Why *Export G-code* cannot run now, for its tooltip: an error in
    /// the project, as the CLI refuses it; else that the toolpaths or the
    /// programs are not ready; else what the plan found; else a file name
    /// that names no file. `None` when it can. A folder not yet chosen is
    /// no refusal: the command asks for one.
    pub(crate) fn refusal(&self, app: &App) -> Option<String> {
        let strings = app.strings();
        if let Some(finding) = project_model::export_refusal(app) {
            return Some(finding.message.clone());
        }
        if dc_app::export::waits(app) {
            return Some(strings.get(StrKey::ExportWaiting).to_owned());
        }
        let Some(ready) = self.current(app) else {
            return Some(strings.get(StrKey::ExportBuilding).to_owned());
        };
        ready
            .blocking_line(strings)
            .or_else(|| ready.blocked())
            .or_else(|| {
                (!valid_stem(self.stem.trim()))
                    .then(|| strings.get(StrKey::ExportStemEmpty).to_owned())
            })
    }
}

/// Writes the plan's files for `app`, every one or none, and remembers
/// the folder for the project; the message for the status bar, or why
/// nothing was written, which the step shows.
pub(crate) fn export(state: &mut Export, app: &mut App) -> Option<String> {
    let strings = app.strings().clone();
    let written = {
        // S20: never while a toolpath is stale or generating, and only
        // the plan of the snapshot the app holds, unblocked.
        if dc_app::export::waits(app) {
            return None;
        }
        let ready = state.current(app)?;
        if ready.blocked().is_some() {
            return None;
        }
        write(state, ready, app.source(), &strings).map(|(dir, names)| {
            let result = ready.result(&names, &dir, &strings);
            let setup = SetupSheet {
                html: ready.setup_sheet(&names, env!("CARGO_PKG_VERSION"), &strings),
                dir: dir.clone(),
                file_name: dc_job::setup_sheet::file_name(state.stem.trim()),
            };
            (dir, result, setup, names.len())
        })
    };
    match written {
        Ok((dir, result, setup, count)) => {
            if let Some(source) = app.source().map(Path::to_path_buf) {
                app.prefs_mut().export_dirs.insert(source, dir.clone());
            }
            let message = strings.fmt(
                StrKey::ExportWritten,
                &[
                    ("count", &count.to_string()),
                    ("dir", &dir.display().to_string()),
                ],
            );
            state.failure = None;
            state.done = Some(Done {
                snapshot: app.snapshot(),
                dir,
                result,
                setup,
                instructions: false,
            });
            Some(message)
        }
        Err(reason) => {
            state.failure = Some(strings.fmt(StrKey::ExportFailed, &[("reason", &reason)]));
            None
        }
    }
}

/// Stages and commits `ready`'s files: the folder and the file names,
/// or why none was written.
fn write(
    state: &Export,
    ready: &Ready,
    source: Option<&Path>,
    strings: &Strings,
) -> Result<(PathBuf, Vec<String>), String> {
    let Some(dir) = state.dir.clone() else {
        return Err(strings.get(StrKey::ExportNoFolder).to_owned());
    };
    let stem = state.stem.trim();
    if !valid_stem(stem) {
        return Err(strings.get(StrKey::ExportStemEmpty).to_owned());
    }
    let files = ready.files(stem);
    let paths: Vec<PathBuf> = files.iter().map(|(name, _)| dir.join(name)).collect();
    // The CLI's check (`export`): never over the project file.
    if let Some(source) = source
        && let Some(path) = paths.iter().find(|p| same_file(p, source))
    {
        return Err(strings.fmt(
            StrKey::ExportOverProject,
            &[("name", &path.display().to_string())],
        ));
    }
    let staged = paths
        .iter()
        .zip(&files)
        .map(|(path, (_, text))| StagedFile::write(path, text.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    commit_all(staged).map_err(|e| e.to_string())?;
    // The result names each file as the CLI run in that folder does; the
    // folder is its own line.
    let names = files.into_iter().map(|(name, _)| name).collect();
    Ok((dir, names))
}

/// A stem that names a file in the folder: not empty, no separator.
fn valid_stem(stem: &str) -> bool {
    !stem.is_empty() && !stem.contains(['/', '\\']) && stem != "." && stem != ".."
}

/// Whether `a` and `b` are the same existing file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Opens the folder the last export wrote to in the system's file
/// manager.
pub(crate) fn show_in_folder(state: &Export) {
    let Some(done) = &state.done else {
        return;
    };
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(&done.dir).spawn();
}

/// The plan, under the step's output settings: the program split and the
/// boards where they apply, *Program Details*, then the destination.
pub(crate) fn plan(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let export = state.export;
    let busy = export.busy(app);
    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, 4.0);
    if dc_app::export::several_tools(app.project()) {
        split(ui, app, export, out);
    }
    if dc_job::summary::by_board(app.project()) {
        boards(ui, strings, export, out);
    }
    let ready = export.current(app).filter(|_| !busy);
    match ready {
        // A plan an error blocks has no program to detail.
        Some(ready) if ready.programs.is_empty() => {}
        Some(ready) => details(ui, state, ready, out),
        None => waiting(ui, app),
    }
    // What the name names, under its field.
    let names = ready
        .filter(|r| !r.programs.is_empty())
        .map(|r| r.names(export.stem.trim()).join(", "));
    destination(ui, strings, export, names.as_deref(), out);
}

/// *Program Details*: the disclosure and, while it is closed, how many
/// programs it holds.
fn details(ui: &mut Ui, state: &FrameState<'_>, ready: &Ready, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let open = state.prefs.sections.get(DETAILS).copied().unwrap_or(false);
    let stem = state.export.stem.trim();
    let toggled = section(
        ui,
        Id::unique(DETAILS),
        strings.get(StrKey::ExportDetails),
        open,
        None,
        |ui| programs(ui, app, ready, stem),
    );
    if toggled {
        out.push(Action::SetSection {
            name: DETAILS.to_owned(),
            open: !open,
        });
    }
    if !open {
        let summary = counted(
            strings,
            ready.programs.len(),
            StrKey::ExportDetailsOne,
            StrKey::ExportDetailsMany,
        );
        ui.horizontal(|ui| {
            // Under the header's title, past its chevron.
            ui.add_space(theme::ICON_SMALL_PX + 8.0 - GAP_PX);
            wrapped(
                ui,
                &summary,
                TextStyle::Small,
                Palette::current(ui).text_secondary,
            );
        });
        ui.add_space(theme::SECTION_GAP_PX - 8.0);
    }
}

/// What the plan waits for, with the spinner: generation, or the build.
fn waiting(ui: &mut Ui, app: &App) {
    let strings = app.strings();
    let key = if dc_app::export::waits(app) {
        StrKey::ExportWaiting
    } else {
        StrKey::ExportBuilding
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::SECTION_HEADER_PX),
        Sense::hover(),
    );
    let mut line = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    line.add_space(4.0);
    spinner(&mut line, "");
    text(
        &mut line,
        strings.get(key),
        TextStyle::Body,
        Palette::current(ui).text_secondary,
    );
    ui.add_space(theme::SECTION_GAP_PX - 8.0);
}

/// The folder with *Choose…*, and the file name with what it names.
fn destination(
    ui: &mut Ui,
    strings: &Strings,
    export: &Export,
    names: Option<&str>,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    ui.horizontal(|ui| {
        row::label(
            ui,
            strings.get(StrKey::ExportDestination),
            strings.get(StrKey::ExportDestinationHelp),
            None,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let choose = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportChoose));
            if button(ui, choose).clicked() {
                out.push(Action::Export(Request::ChooseFolder));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                match &export.dir {
                    Some(dir) => row::path(ui, &dir.display().to_string()),
                    None => {
                        text(
                            ui,
                            strings.get(StrKey::ExportNoFolderYet),
                            TextStyle::Body,
                            palette.text_secondary,
                        )
                        .on_hover_text(strings.get(StrKey::ExportNoFolder));
                    }
                }
            });
        });
    });
    let stem = strings.get(StrKey::ExportStem);
    let valid = valid_stem(export.stem.trim());
    ui.horizontal(|ui| {
        row::label(ui, stem, strings.get(StrKey::ExportStemHelp), None);
        let typed = unit_field(
            ui,
            UnitField {
                tier: (!valid).then_some(Tier::Error),
                ..UnitField::new(Id::unique("export.stem"), stem, &export.stem, "")
            },
        );
        if let Some(stem) = typed.committed {
            out.push(Action::Export(Request::SetStem(stem)));
        }
    });
    if !valid {
        row::message(ui, Tier::Error, strings.get(StrKey::ExportStemEmpty));
    } else if let Some(names) = names {
        row::hint(ui, &strings.fmt(StrKey::ExportNames, &[("names", names)]));
    }
}

/// The program split: as the project says, or overridden for this export.
fn split(ui: &mut Ui, app: &App, export: &Export, out: &mut Vec<Action>) {
    let strings = app.strings();
    let project = app.project();
    let mode = match project.post_processor.tool_change_mode {
        ToolChangeMode::SeparatePrograms => strings.get(StrKey::ExportModeSeparate),
        ToolChangeMode::M6 => strings.get(StrKey::ExportModeM6),
    };
    let as_project = strings.fmt(StrKey::ExportSplitProject, &[("mode", mode)]);
    let label_of = |split: ProgramSplit| match split {
        ProgramSplit::ProjectDefault => as_project.clone(),
        ProgramSplit::ProgramPerTool => strings.get(StrKey::ExportSplitPerTool).to_owned(),
        ProgramSplit::OneProgram => strings.get(StrKey::ExportSplitM6).to_owned(),
    };
    let m6 = project_model::offers(project).m6;
    // The choices are sentences, so the list takes the row's whole width,
    // under its label.
    let label = row::caption(
        ui,
        strings.get(StrKey::ExportSplit),
        strings.get(StrKey::ExportSplitHelp),
    );
    egui::ComboBox::from_id_salt("export.split")
        .icon(crate::widgets::icon::combo_chevron)
        .truncate()
        .selected_text(label_of(export.split))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for choice in [
                ProgramSplit::ProjectDefault,
                ProgramSplit::ProgramPerTool,
                ProgramSplit::OneProgram,
            ] {
                let refused = match (&m6, choice) {
                    (Offer::Refused { reason }, ProgramSplit::OneProgram) => Some(reason),
                    _ => None,
                };
                let response = ui.add_enabled(
                    refused.is_none(),
                    egui::Button::selectable(export.split == choice, label_of(choice)),
                );
                if let Some(reason) = refused {
                    let _ = response.on_disabled_hover_text(reason);
                } else if response.clicked() && export.split != choice {
                    out.push(Action::Export(Request::SetSplit(choice)));
                }
            }
        })
        .response
        .labelled_by(label.id);
    ui.add_space(4.0);
}

/// A tick per board, with an Inlay (#498): this export only.
fn boards(ui: &mut Ui, strings: &Strings, export: &Export, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        row::label(
            ui,
            strings.get(StrKey::ExportBoards),
            strings.get(StrKey::ExportBoardsHelp),
            None,
        );
        let mut ticked = export.boards;
        checkbox(
            ui,
            "export.female",
            &mut ticked.female,
            strings.get(StrKey::ExportFemale),
        );
        checkbox(
            ui,
            "export.male",
            &mut ticked.male,
            strings.get(StrKey::ExportMale),
        );
        if ticked != export.boards {
            out.push(Action::Export(Request::SetBoards(ticked)));
        }
    });
}

/// The programs: one striped row per program, grouped by board with an
/// Inlay: its number and file, its estimate and Z range on the right;
/// its tool and operations under them; then the step before it as
/// `export` prints it: *Program Details*' body.
fn programs(ui: &mut Ui, app: &App, ready: &Ready, stem: &str) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let project = ready.project();
    let names = ready.names(stem);
    let not_available = strings.get(StrKey::ExportNotAvailable);
    let mut board = None;
    let mut k_in_board = 0;
    for (k, (summary, name)) in ready.programs.iter().zip(&names).enumerate() {
        let program = &summary.program;
        if ready.by_board && board != Some(program.board) {
            board = Some(program.board);
            k_in_board = 0;
            ui.add_space(4.0);
            wrapped(
                ui,
                &dc_job::summary::board_heading(project, program.board),
                TextStyle::Name(theme::TITLE.into()),
                palette.text_primary,
            );
        }
        k_in_board += 1;
        let number = if ready.by_board { k_in_board } else { k + 1 };
        let tool = strings.fmt(
            StrKey::ExportTool,
            &[
                ("number", &program.first_tool.number.to_string()),
                ("name", &program.first_tool.name),
            ],
        );
        let operations: Vec<&str> = summary
            .operations
            .iter()
            .filter_map(|id| project.operations.iter().find(|o| o.id() == id))
            .map(dc_project::Operation::name)
            .collect();
        let estimate = ready
            .estimate(k)
            .unwrap_or_else(|| not_available.to_owned());
        let z = program.extents.z.map_or_else(
            || not_available.to_owned(),
            |z| {
                let range = dc_app::view::range_label(z, program.extents.units);
                strings.fmt(StrKey::AppToolpathsZRange, &[("z", &range)])
            },
        );
        let fill = if k % 2 == 0 {
            palette.surface_stripe
        } else {
            theme::NONE
        };
        egui::Frame::NONE
            .fill(fill)
            .corner_radius(theme::CONTROL_RADIUS_PX)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // One text line high, so the three lines sit evenly.
                let line = ui.text_style_height(&TextStyle::Body);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, 4.0);
                    // The number in a gutter, the program's lines beside it.
                    ui.allocate_ui_with_layout(
                        egui::vec2(NUMBER_PX, line),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            text(
                                ui,
                                &number.to_string(),
                                TextStyle::Body,
                                palette.text_secondary,
                            );
                        },
                    );
                    ui.vertical(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), line),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                text(ui, &z, TextStyle::Body, palette.text_secondary)
                                    .on_hover_text(strings.get(StrKey::ExportColumnZ));
                                text(ui, &estimate, TextStyle::Body, palette.text_primary)
                                    .on_hover_text(&summary.estimate_line);
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    text(ui, name, TextStyle::Monospace, palette.text_primary)
                                        .on_hover_text(name);
                                });
                            },
                        );
                        let what = strings.fmt(
                            StrKey::ExportUses,
                            &[("tool", &tool), ("operations", &operations.join(", "))],
                        );
                        wrapped(ui, &what, TextStyle::Body, palette.text_primary);
                        let before =
                            strings.fmt(StrKey::ExportBefore, &[("before", &summary.before)]);
                        wrapped(ui, &before, TextStyle::Small, palette.text_secondary);
                    });
                });
            });
    }
}

/// `text` wrapped to the width.
fn wrapped(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
            .wrap()
            .selectable(false),
    );
}
