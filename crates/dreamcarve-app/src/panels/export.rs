// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The export sheet and its result (functional spec, capability 77 and
//! S20; mock-up `11-export-dialog.svg`; `docs/specs/phase-4/design.md`,
//! "Architecture → Export"; `experience.md`, sections 7 and 8, "Sheets").
//!
//! A 560-px sheet over a scrim: the destination folder and file name
//! (the folder remembered per project in `gui.toml`, beside the project
//! by default), the post-processor (changing it is the Project panel's
//! undoable switch), the program split as the project says or overridden
//! for this export, a tick per board with an Inlay, then *Program
//! Details*, a disclosure closed whenever the sheet opens, holding the
//! programs `dc_app::export::plan` built, one row each with the step
//! before it. Trimmed by the review (#624): no toolpath preview tick (the
//! Viewport is the preview), no warnings list (the status bar counts
//! them), no estimated total (the status bar shows it) and no note on how
//! files are written. Errors block *Export*: one line above the footer
//! counts them, with *Show* opening the Warnings drawer at them, and
//! *Export*'s tooltip gives the same reason. The plan is built on a
//! thread of its own once generation has finished, so the window keeps
//! answering; until then the sheet says what it waits for under a
//! progress bar.
//!
//! *Export* stages every program and moves them into place with
//! `dreamcarve_shell::commit_all`, every one or none, after the CLI's
//! check: never over the project file. The project file is never
//! written. The result (capability 77a, #625) replaces
//! the sheet's content in place: Status, Files and Run instructions
//! (`panels::result`), with *Save setup sheet…* (capability 77b) and
//! *Done*. The header's `x`, Escape and the scrim close it; no Cancel
//! (`experience.md`, section 13, item 9).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use dc_app::export::{Boards, ExportResult, Options, Ready};
use dc_app::project::{self as project_model, Offer};
use dc_app::{App, SnapshotId, StrKey, Strings};
use dc_job::{EstimateConfig, ProgramSplit, Tier};
use dc_project::ToolChangeMode;
use dreamcarve_shell::{StagedFile, commit_all};
use eframe::egui::{self, Align, Layout, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::section::section;
use dc_app::status::counted;

/// The sheet's width, px.
const WIDTH_PX: f32 = 560.0;
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The header, px.
const HEADER_PX: f32 = 32.0;
/// A sheet footer button, px.
const FOOTER_PX: f32 = 28.0;
/// How soon the window looks for the plan again while it is building.
const POLL: Duration = Duration::from_millis(50);

/// What the sheet asks the shell for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    /// Close the sheet.
    Close,
    /// Pick the destination folder.
    ChooseFolder,
    /// Write the files.
    Export,
    /// Open the folder the files went to.
    ShowInFolder,
    /// Close the sheet and open the Warnings drawer at the errors that
    /// block export.
    ShowErrors,
    /// The result's folder was copied: say so.
    Copied,
}

/// The plan being built off the UI thread, and what it was asked for.
struct Planning {
    key: (SnapshotId, Options),
    results: Receiver<Ready>,
}

/// What an export wrote: its result and its setup sheet.
#[derive(Debug, Clone, PartialEq)]
struct Done {
    dir: PathBuf,
    result: ExportResult,
    setup: SetupSheet,
    /// Whether *Run instructions* is open: closed when the result shows.
    instructions: bool,
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

/// The sheet's state between frames.
pub(crate) struct Sheet {
    /// The folder the files go to; `None` for a project never saved and
    /// no folder chosen.
    dir: Option<PathBuf>,
    /// What each file's name starts with.
    stem: String,
    split: ProgramSplit,
    boards: Boards,
    /// Whether *Program Details* is open: closed whenever the sheet
    /// opens.
    details: bool,
    /// The date the programs are headed with: today's, when the sheet
    /// opened.
    date: String,
    estimate: EstimateConfig,
    /// The plan of the current snapshot and options, once built.
    ready: Option<Box<Ready>>,
    planning: Option<Planning>,
    /// Why the last *Export* wrote nothing.
    failure: Option<String>,
    /// The result, once written.
    done: Option<Done>,
}

impl std::fmt::Debug for Sheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sheet")
            .field("dir", &self.dir)
            .field("stem", &self.stem)
            .field("split", &self.split)
            .field("planning", &self.planning.is_some())
            .finish_non_exhaustive()
    }
}

impl Sheet {
    /// The sheet for `app`'s project: the folder remembered for it, else
    /// its own; the stem its file's, else its name.
    pub(crate) fn open(app: &App, date: String, estimate: EstimateConfig) -> Self {
        let source = app.source();
        let dir = source
            .and_then(|path| app.prefs().export_dirs.get(path).cloned())
            .or_else(|| source.and_then(Path::parent).map(Path::to_path_buf))
            .filter(|dir| !dir.as_os_str().is_empty());
        let stem = source.and_then(Path::file_stem).map_or_else(
            || app.project().name.clone(),
            |s| s.to_string_lossy().into_owned(),
        );
        Self {
            dir,
            stem,
            split: ProgramSplit::ProjectDefault,
            boards: Boards::default(),
            details: false,
            date,
            estimate,
            ready: None,
            planning: None,
            failure: None,
            done: None,
        }
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

    /// One frame's turn: the finished plan taken in, and a new one
    /// started once generation is over and the plan in hand is not the
    /// current snapshot's. Whether the window should look again soon.
    pub(crate) fn tick(&mut self, app: &App) -> bool {
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
        self.failure = None;
    }

    /// The folder the files go to.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The setup sheet of what this sheet wrote, once it has.
    pub(crate) fn setup_sheet(&self) -> Option<&SetupSheet> {
        self.done.as_ref().map(|done| &done.setup)
    }
}

/// Writes the plan's files for `app`, every one or none, and remembers
/// the folder for the project; the message for the status bar, or why
/// nothing was written, which the sheet shows.
pub(crate) fn export(sheet: &mut Sheet, app: &mut App) -> Option<String> {
    let strings = app.strings().clone();
    let written = {
        // S20: never while a toolpath is stale or generating, and only
        // the plan of the snapshot the app holds, unblocked.
        if dc_app::export::waits(app) {
            return None;
        }
        let ready = sheet.current(app)?;
        if ready.blocked().is_some() {
            return None;
        }
        write(sheet, ready, app.source(), &strings).map(|(dir, names)| {
            let result = ready.result(&names, &dir, &strings);
            let setup = SetupSheet {
                html: ready.setup_sheet(&names, env!("CARGO_PKG_VERSION"), &strings),
                dir: dir.clone(),
                file_name: dc_job::setup_sheet::file_name(sheet.stem.trim()),
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
            sheet.failure = None;
            sheet.done = Some(Done {
                dir,
                result,
                setup,
                instructions: false,
            });
            Some(message)
        }
        Err(reason) => {
            sheet.failure = Some(strings.fmt(StrKey::ExportFailed, &[("reason", &reason)]));
            None
        }
    }
}

/// Stages and commits `ready`'s files: the folder and the file names,
/// or why none was written.
fn write(
    sheet: &Sheet,
    ready: &Ready,
    source: Option<&Path>,
    strings: &Strings,
) -> Result<(PathBuf, Vec<String>), String> {
    let Some(dir) = sheet.dir.clone() else {
        return Err(strings.get(StrKey::ExportNoFolder).to_owned());
    };
    let stem = sheet.stem.trim();
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

/// Opens `dir` in the system's file manager.
pub(crate) fn show_in_folder(sheet: &Sheet) {
    let Some(done) = &sheet.done else {
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

/// Shows the sheet; what the user asked for goes to `out`.
pub(crate) fn show(
    ctx: &egui::Context,
    app: &App,
    display_name: &str,
    sheet: &mut Sheet,
    out: &mut Vec<Action>,
) {
    if sheet.tick(app) {
        ctx.request_repaint_after(POLL);
    }
    let style = ctx.global_style();
    let palette = if style.visuals.dark_mode {
        &theme::DARK
    } else {
        &theme::LIGHT
    };
    let screen = ctx.content_rect().size();
    let width = WIDTH_PX.min(screen.x - 48.0);
    let frame = egui::Frame::popup(&style)
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .inner_margin(MARGIN_PX);
    let response = egui::Modal::new(egui::Id::unique("export_sheet"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width - 2.0 * f32::from(MARGIN_PX));
            ui.set_max_height(screen.y - 96.0);
            body(ui, app, display_name, sheet, out);
        });
    if response.should_close() {
        out.push(Action::Export(Request::Close));
    }
}

fn body(ui: &mut Ui, app: &App, display_name: &str, sheet: &mut Sheet, out: &mut Vec<Action>) {
    let strings = app.strings();
    let title = if sheet.done.is_some() {
        strings.fmt(StrKey::ExportResultTitle, &[("name", display_name)])
    } else {
        strings.fmt(StrKey::ExportTitle, &[("name", display_name)])
    };
    header(ui, strings, &title, out);
    if let Some(done) = &mut sheet.done {
        if crate::panels::result::show(ui, strings, &done.result, done.instructions, out) {
            done.instructions = !done.instructions;
        }
        return;
    }
    let busy = dc_app::export::waits(app) || sheet.planning(app);
    if busy {
        // A 4-px bar under the header (`experience.md`, section 8,
        // "Progress"), moved by the frames the plan's polling asks for,
        // never by a repaint of its own.
        #[allow(clippy::cast_possible_truncation, reason = "a fraction in 0..1")]
        let phase = ui.input(|i| i.time).fract() as f32;
        ui.add(egui::ProgressBar::new(phase).desired_height(4.0));
    }
    ui.add_space(8.0);
    let footer_h = FOOTER_PX + 48.0;
    let room = (ui.available_height() - footer_h).max(160.0);
    let mut toggled = false;
    egui::ScrollArea::vertical()
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // What the stem names, as mock-up 11 shows it under the field.
            let names = sheet
                .current(app)
                .filter(|r| !r.programs.is_empty())
                .map(|r| r.names(sheet.stem.trim()).join(", "));
            destination(ui, strings, sheet, names.as_deref(), out);
            ui.add_space(8.0);
            post(ui, app, out);
            ui.add_space(8.0);
            split(ui, app, sheet);
            if dc_job::summary::by_board(app.project()) {
                ui.add_space(4.0);
                boards(ui, strings, sheet);
            }
            ui.add_space(12.0);
            match sheet.current(app).filter(|_| !busy) {
                Some(ready) => {
                    let stem = sheet.stem.trim();
                    toggled = section(
                        ui,
                        egui::Id::unique("export.details"),
                        strings.get(StrKey::ExportDetails),
                        sheet.details,
                        None,
                        |ui| programs(ui, app, ready, stem),
                    );
                    if !sheet.details && !ready.programs.is_empty() {
                        let summary = counted(
                            strings,
                            ready.programs.len(),
                            StrKey::ExportDetailsOne,
                            StrKey::ExportDetailsMany,
                        );
                        wrapped(
                            ui,
                            &summary,
                            TextStyle::Small,
                            Palette::current(ui).text_secondary,
                        );
                    }
                }
                None => {
                    let key = if dc_app::export::waits(app) {
                        StrKey::ExportWaiting
                    } else {
                        StrKey::ExportBuilding
                    };
                    wrapped(
                        ui,
                        strings.get(key),
                        TextStyle::Body,
                        Palette::current(ui).text_secondary,
                    );
                }
            }
        });
    if toggled {
        sheet.details = !sheet.details;
    }
    ui.add_space(12.0);
    footer(ui, app, sheet, busy, out);
}

/// The 32-px header: the title and `x`.
fn header(ui: &mut Ui, strings: &Strings, title: &str, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), HEADER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let close = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::X, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::ExportClose));
            if button(ui, close).clicked() {
                out.push(Action::Export(Request::Close));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                text(
                    ui,
                    title,
                    TextStyle::Name(theme::DISPLAY.into()),
                    palette.text_primary,
                );
            });
        },
    );
}

/// The folder, *Choose…*, and the file name.
fn destination(
    ui: &mut Ui,
    strings: &Strings,
    sheet: &mut Sheet,
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
                match &sheet.dir {
                    Some(dir) => {
                        let path = dir.display().to_string();
                        text(ui, &path, TextStyle::Monospace, palette.text_primary)
                            .on_hover_text(&path);
                    }
                    None => {
                        text(
                            ui,
                            strings.get(StrKey::ExportNoFolder),
                            TextStyle::Small,
                            palette.status_error.text,
                        );
                    }
                }
            });
        });
    });
    ui.horizontal(|ui| {
        let label = row::label(
            ui,
            strings.get(StrKey::ExportStem),
            strings.get(StrKey::ExportStemHelp),
            None,
        );
        let edit = ui.add(
            egui::TextEdit::singleline(&mut sheet.stem)
                .font(TextStyle::Monospace)
                .desired_width(ui.available_width()),
        );
        let _ = edit.labelled_by(label.id);
    });
    if !valid_stem(sheet.stem.trim()) {
        row::message(ui, Tier::Error, strings.get(StrKey::ExportStemEmpty));
    } else if let Some(names) = names {
        row::hint(ui, &strings.fmt(StrKey::ExportNames, &[("names", names)]));
    }
}

/// The post-processor and its version; picking another is the Project
/// panel's switch, an undoable project change.
fn post(ui: &mut Ui, app: &App, out: &mut Vec<Action>) {
    let strings = app.strings();
    let project = app.project();
    let posts = project_model::posts().unwrap_or_default();
    let current = project.post_processor.id.as_str();
    ui.horizontal(|ui| {
        let label = row::label(
            ui,
            strings.get(StrKey::ExportPost),
            strings.get(StrKey::ExportPostHelp),
            None,
        );
        let mut picked = None;
        egui::ComboBox::from_id_salt("export.post")
            .icon(crate::widgets::icon::combo_chevron)
            .truncate()
            .selected_text(project_model::post_label(strings, project))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for choice in &posts {
                    let label = strings.fmt(
                        StrKey::ProjectPostVersionLatest,
                        &[
                            ("name", &choice.name),
                            ("version", &choice.version.to_string()),
                        ],
                    );
                    if ui.selectable_label(choice.id == current, label).clicked() {
                        picked = Some(choice.id);
                    }
                }
            })
            .response
            .labelled_by(label.id);
        if let Some(id) = picked {
            out.extend(project_model::switch_post(project, id).map(Action::run));
        }
    });
    row::hint(ui, strings.get(StrKey::ExportPostNote));
}

/// The program split: as the project says, or overridden for this export.
fn split(ui: &mut Ui, app: &App, sheet: &mut Sheet) {
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
    ui.horizontal(|ui| {
        let label = row::label(
            ui,
            strings.get(StrKey::ExportSplit),
            strings.get(StrKey::ExportSplitHelp),
            None,
        );
        egui::ComboBox::from_id_salt("export.split")
            .icon(crate::widgets::icon::combo_chevron)
            .truncate()
            .selected_text(label_of(sheet.split))
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
                        egui::Button::selectable(sheet.split == choice, label_of(choice)),
                    );
                    if let Some(reason) = refused {
                        let _ = response.on_disabled_hover_text(reason);
                    } else if response.clicked() {
                        sheet.split = choice;
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
}

/// A tick per board, with an Inlay (#498): this export only.
fn boards(ui: &mut Ui, strings: &Strings, sheet: &mut Sheet) {
    ui.horizontal(|ui| {
        row::label(
            ui,
            strings.get(StrKey::ExportBoards),
            strings.get(StrKey::ExportBoardsHelp),
            None,
        );
        checkbox(
            ui,
            "export.female",
            &mut sheet.boards.female,
            strings.get(StrKey::ExportFemale),
        );
        checkbox(
            ui,
            "export.male",
            &mut sheet.boards.male,
            strings.get(StrKey::ExportMale),
        );
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
            |z| dc_app::view::range_label(z, program.extents.units),
        );
        let fill = if k % 2 == 0 {
            palette.surface_stripe
        } else {
            palette.surface_raised
        };
        egui::Frame::NONE
            .fill(fill)
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    text(
                        ui,
                        &number.to_string(),
                        TextStyle::Body,
                        palette.text_secondary,
                    );
                    text(ui, name, TextStyle::Monospace, palette.text_primary);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        text(ui, &z, TextStyle::Body, palette.text_secondary)
                            .on_hover_text(strings.get(StrKey::ExportColumnZ));
                        text(ui, &estimate, TextStyle::Body, palette.text_primary)
                            .on_hover_text(&summary.estimate_line);
                    });
                });
                let what = strings.fmt(
                    StrKey::ExportUses,
                    &[("tool", &tool), ("operations", &operations.join(", "))],
                );
                wrapped(ui, &what, TextStyle::Body, palette.text_primary);
                let before = strings.fmt(StrKey::ExportBefore, &[("before", &summary.before)]);
                wrapped(ui, &before, TextStyle::Small, palette.text_secondary);
            });
    }
}

/// The footer: the last failure, the one line saying errors block
/// export with *Show*, and *Export*, disabled with the reason in its
/// tooltip while it waits, or while anything blocks it.
fn footer(ui: &mut Ui, app: &App, sheet: &Sheet, busy: bool, out: &mut Vec<Action>) {
    let strings = app.strings();
    if let Some(failure) = &sheet.failure {
        row::message(ui, Tier::Error, failure);
    }
    let ready = sheet.current(app).filter(|_| !busy);
    let blocking = ready.and_then(|r| r.blocking_line(strings));
    if let Some(line) = &blocking {
        blocking_line(ui, strings, line, out);
    }
    ui.add_space(8.0);
    let reason = if busy {
        Some(strings.get(StrKey::ExportWaiting).to_owned())
    } else {
        match ready {
            // The blocking line's reason, as the line says it.
            Some(ready) => blocking.clone().or_else(|| ready.blocked()),
            None => Some(strings.get(StrKey::ExportBuilding).to_owned()),
        }
    }
    .or_else(|| {
        sheet
            .dir
            .is_none()
            .then(|| strings.get(StrKey::ExportNoFolder).to_owned())
    })
    .or_else(|| {
        (!valid_stem(sheet.stem.trim())).then(|| strings.get(StrKey::ExportStemEmpty).to_owned())
    });
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let mut spec = ButtonSpec::new(Kind::Primary, strings.get(StrKey::ExportExport))
                .icon(Icon::Download, theme::ICON_SMALL_PX)
                .height(FOOTER_PX)
                .enabled(reason.is_none());
            if let Some(reason) = reason.as_deref() {
                spec = spec.tooltip(reason);
            }
            if button(ui, spec).clicked() {
                out.push(Action::Export(Request::Export));
            }
        },
    );
}

/// The one line above the footer when errors block export: the error
/// glyph, `1 error blocks export`, and *Show*, which opens the Warnings
/// drawer at them.
fn blocking_line(ui: &mut Ui, strings: &Strings, line: &str, out: &mut Vec<Action>) {
    let colours = Palette::current(ui).status_error;
    ui.horizontal(|ui| {
        let glyph = Icon::CircleX.galley(ui, theme::ICON_SMALL_PX, colours.stroke);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
            egui::Sense::hover(),
        );
        ui.painter()
            .galley(rect.center() - glyph.size() / 2.0, glyph, colours.stroke);
        text(ui, line, TextStyle::Body, colours.text);
        let show = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::ExportShow))
            .tooltip(strings.get(StrKey::ExportShowHelp));
        if button(ui, show).clicked() {
            out.push(Action::Export(Request::ShowErrors));
        }
    });
}

/// `text` wrapped to the width.
fn wrapped(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
            .wrap()
            .selectable(false),
    );
}
