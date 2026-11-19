// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The window (`docs/specs/phase-4/design.md`, "Architecture → The
//! shell"; `docs/specs/phase-4/redesign.md`, "The window"; functional
//! spec, capabilities 50, 51 and 79): one [`Shell`] implementing
//! `eframe::App`, which owns the `dc_app::App` and every file, dialog and
//! preference write, and never holds project data of its own. Each frame
//! it shows the appearance the preference names (dark unless chosen
//! otherwise), answers the window's close button and dropped files, reads
//! the keyboard shortcuts, draws the header, the status bar, the rail,
//! the current step's panel, the Inspector and the Viewport, and then
//! runs what the widgets asked for.
//!
//! The work is five steps ([`dc_app::steps`]): the shell holds which one
//! is current. A newly opened project starts on its first step that is
//! not complete; after that the step the user picks stays.
//!
//! Generation runs on the [`Worker`] thread (slice 9, #538): each frame
//! first hands the results it finished to `dc_app::App::accept`, and last
//! posts the next piece of work once the frame's edits have settled. The
//! UI thread never runs a generator.
//!
//! Files are read and written on the UI thread: a `.dcv` is small, and the
//! progress sheet the design gives slow disks arrives with export (slice
//! 18), the first job that can be large.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dc_app::import::{DrawingFormat, Purpose};
use dc_app::onboarding::Thumbnail;
use dc_app::steps::{self, Step};
use dc_app::{App, Command, Prefs, SnapshotId, StrKey, Strings};
use dc_job::EstimateConfig;
use dreamcarve_shell::{GuiPrefs, Paths, StagedFile, commit_all, config, gui_prefs};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, Rect, Vec2, ViewportCommand};

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::focus::{self, Region};
use crate::shell::worker::{self, Worker};
use crate::shell::{files, header, rail, status, theme};
use crate::{panels, viewport};

/// The quick starts' example projects (functional spec, capability 79).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Example {
    /// The phase 1 coaster: `fixtures/coaster.svg` as `dreamcarve init`
    /// makes it, its feeds and depths unset as the quick start begins.
    Coaster,
    /// The phase 2 plaque, `tests/fixtures/plaque.dcv`.
    Plaque,
    /// The phase 3 sign, `tests/fixtures/sign.dcv`.
    Sign,
}

impl Example {
    /// Every example, in the start screen's order.
    pub const ALL: [Self; 3] = [Self::Coaster, Self::Plaque, Self::Sign];

    /// Its name on its card.
    pub(crate) const fn name(self) -> StrKey {
        match self {
            Self::Coaster => StrKey::AppEmptyCoaster,
            Self::Plaque => StrKey::AppEmptyPlaque,
            Self::Sign => StrKey::AppEmptySign,
        }
    }

    /// The one line saying what it teaches.
    pub(crate) const fn teaches(self) -> StrKey {
        match self {
            Self::Coaster => StrKey::AppEmptyCoasterTeaches,
            Self::Plaque => StrKey::AppEmptyPlaqueTeaches,
            Self::Sign => StrKey::AppEmptySignTeaches,
        }
    }

    /// The saved project behind it; `None` for the coaster, which is made
    /// from its drawing ([`coaster`]).
    const fn dcv(self) -> Option<&'static [u8]> {
        match self {
            Self::Coaster => None,
            Self::Plaque => Some(PLAQUE_DCV),
            Self::Sign => Some(SIGN_DCV),
        }
    }

    /// Its thumbnail, drawn from the example's own shapes (capability
    /// 79). An example that cannot be read has an empty one; opening it
    /// says why.
    fn thumbnail(self) -> Thumbnail {
        let project = match self.dcv() {
            Some(bytes) => dc_project::load(bytes).map(|loaded| loaded.project).ok(),
            None => coaster().ok(),
        };
        project.map_or_else(Thumbnail::default, |project| {
            dc_app::onboarding::thumbnail(&project)
        })
    }
}

/// The coaster's drawing.
const COASTER_SVG: &[u8] = include_bytes!("../../../../fixtures/coaster.svg");
/// The plaque the phase 2 quick start builds.
const PLAQUE_DCV: &[u8] = include_bytes!("../../../../tests/fixtures/plaque.dcv");
/// The sign the phase 3 quick start builds.
const SIGN_DCV: &[u8] = include_bytes!("../../../../tests/fixtures/sign.dcv");

/// The book, for Help ▸ User Guide.
const USER_GUIDE_URL: &str = "https://grayzu.github.io/dreamcarve/";

/// The smallest window that holds the rail, the step panel, the Viewport
/// and the Inspector at their minimum widths (functional spec, capability
/// 50; `redesign.md`, "The window"): in a narrower one the Inspector lies
/// over the Viewport.
pub const MIN_WINDOW: Vec2 = egui::vec2(1280.0, 768.0);

/// The native file dialogs, or a test's stand-ins.
pub trait Dialogs {
    /// Asks for a project to open.
    fn open_project(&mut self, title: &str) -> Option<PathBuf>;
    /// Asks where to save the project, suggesting `file_name`.
    fn save_project(&mut self, title: &str, file_name: &str) -> Option<PathBuf>;
    /// Asks for a drawing to import.
    fn open_drawing(&mut self, title: &str) -> Option<PathBuf>;
    /// Asks for a tool library to import: a Fusion `.tools` or `.json`,
    /// or a DreamCarve library.
    fn open_library(&mut self, _title: &str) -> Option<PathBuf> {
        None
    }
    /// Asks where to export the tool library, suggesting `file_name`.
    fn save_library(&mut self, _title: &str, _file_name: &str) -> Option<PathBuf> {
        None
    }
    /// Asks for the folder to export G-code to, starting in `start`.
    fn export_folder(&mut self, _title: &str, _start: Option<&Path>) -> Option<PathBuf> {
        None
    }
    /// Asks where to save the setup sheet, suggesting `file_name` in
    /// `dir`, beside the programs (capability 77b).
    fn save_setup_sheet(&mut self, _title: &str, _dir: &Path, _file_name: &str) -> Option<PathBuf> {
        None
    }
}

/// The OS's open and save dialogs, through `rfd` (design open question 3).
#[derive(Debug, Clone)]
pub struct NativeDialogs {
    project_filter: String,
    drawing_filter: String,
    library_filter: String,
    setup_sheet_filter: String,
}

impl NativeDialogs {
    /// Dialogs whose filters are named from `strings`.
    #[must_use]
    pub fn new(strings: &Strings) -> Self {
        Self {
            project_filter: strings.get(StrKey::AppDialogProjectFilter).to_owned(),
            drawing_filter: strings.get(StrKey::AppDialogDrawingFilter).to_owned(),
            library_filter: strings.get(StrKey::AppDialogLibraryFilter).to_owned(),
            setup_sheet_filter: strings.get(StrKey::ExportSetupSheetFilter).to_owned(),
        }
    }
}

impl Dialogs for NativeDialogs {
    fn open_project(&mut self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.project_filter, &["dcv"])
            .pick_file()
    }

    fn save_project(&mut self, title: &str, file_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.project_filter, &["dcv"])
            .set_file_name(file_name)
            .save_file()
    }

    fn open_drawing(&mut self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.drawing_filter, &["svg", "dxf"])
            .pick_file()
    }

    fn open_library(&mut self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.library_filter, &["tools", "json"])
            .pick_file()
    }

    fn save_library(&mut self, title: &str, file_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.library_filter, &["json"])
            .set_file_name(file_name)
            .save_file()
    }

    fn export_folder(&mut self, title: &str, start: Option<&Path>) -> Option<PathBuf> {
        let dialog = rfd::FileDialog::new().set_title(title);
        match start {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
        .pick_folder()
    }

    fn save_setup_sheet(&mut self, title: &str, dir: &Path, file_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(&self.setup_sheet_filter, &["html"])
            .set_directory(dir)
            .set_file_name(file_name)
            .save_file()
    }
}

/// How the shell starts.
pub struct Options {
    /// The config directory holding `gui.toml`; `None` when it cannot be
    /// found, and then preferences are not saved.
    pub config: Option<Paths>,
    /// A project to open at once (`--open`, or the OS's file association).
    pub open: Option<PathBuf>,
    /// The file dialogs.
    pub dialogs: Box<dyn Dialogs>,
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("config", &self.config)
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

/// The last frame's layout: each region's card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The rail.
    pub rail: egui::Rect,
    /// The step panel; `None` while View ▸ Step Panel is off.
    pub step_panel: Option<egui::Rect>,
    /// The Inspector; `None` while the step has nothing selected. In a
    /// window narrower than [`MIN_WINDOW`] it lies over the Viewport.
    pub inspector: Option<egui::Rect>,
    /// The Viewport.
    pub viewport: egui::Rect,
    /// The window's size.
    pub window: Vec2,
}

/// Something a widget or shortcut asked for this frame, run after the
/// frame is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    New,
    Open,
    OpenRecent(PathBuf),
    OpenExample(Example),
    Save,
    SaveAs,
    ImportDrawing,
    /// File ▸ New from drawing… (capability 51).
    NewFromDrawing,
    /// Something the Import drawing sheet asked for.
    ImportSheet(panels::import::Request),
    Undo,
    Redo,
    Quit,
    /// Fit the stock to the visible shapes with this margin, mm.
    FitStock {
        margin_mm: f64,
    },
    SetTheme(dc_app::Theme),
    /// Open this step (a rail button, View ▸ Go to, Ctrl 1 to 5).
    GoTo(Step),
    /// The header's *Continue*: the next step that is not complete.
    Continue,
    /// View ▸ Step Panel: the step panel shown or hidden.
    ToggleStepPanel,
    /// The Inspector's close button: what it edits is no longer selected.
    CloseInspector,
    PruneRecent,
    UserGuide,
    About,
    Answer(Answer),
    /// Project ▸ Project Settings…: the Stock step, where the project's
    /// units, stock and origin are (capability 60).
    ShowProject,
    /// A panel section opened or closed, kept in `gui.toml`.
    SetSection {
        name: String,
        open: bool,
    },
    /// Run a command on the project; a refusal goes to the status bar.
    Run(Box<Command>),
    /// An Arrange action on the selection (the Inspector's group, the
    /// Arrange menu, R, Shift R, Ctrl Shift H, Ctrl Shift V; #617): its
    /// command runs on the unlocked shapes, and the status bar names the
    /// locked ones, or says why nothing could turn.
    Arrange(dc_app::arrange::Action),
    /// Rename a shape in its Design Tree row (F2, the context menu's
    /// *Rename*; #617).
    Rename(dc_project::InstanceId),
    /// Arrange ▸ Rotate…: the Arrange group's Rotate popover opened, on
    /// the Design step where the group is (#617).
    OpenRotate,
    /// Show a refusal in the status bar.
    Refused(String),
    /// Open the Tool Library sheet.
    OpenLibrary,
    /// Something the Tool Library sheet asked for.
    Library(panels::library::Request),
    /// Copy a project tool into the library.
    SaveToLibrary(dc_project::ToolId),
    /// The Viewport's camera after this frame's pan or zoom.
    SetCamera(dc_app::Camera),
    /// A click or marquee on the Viewport chose this selection.
    Select(dc_app::Selection),
    /// Fit the Viewport to the stock and shapes (F).
    FitView,
    /// Fit the Viewport to the selection (Shift F).
    ZoomToSelection,
    /// Clear the selection (Escape).
    ClearSelection,
    /// Open or close the Warnings drawer (the status bar's counts, View ▸
    /// Warnings).
    ToggleWarnings,
    /// Put `check`'s text on the clipboard (the drawer's *Copy*).
    CopyWarnings,
    /// Open the Warnings drawer at this operation's carve note (the
    /// status bar's message cell, capability 76a).
    ShowNote(dc_project::OperationId),
    /// Stop generation (the status bar's *Stop*).
    StopGeneration,
    /// Regenerate every operation (the status bar's *Regenerate all*,
    /// offered after Stop).
    RegenerateAll,
    /// Open a page of the book (a warning code's chapter entry).
    OpenUrl(String),
    /// What the Viewport draws of a toolpath (the toolpath view's
    /// toggles): view state, kept in `gui.toml`.
    SetVisibility(dc_app::Visibility),
    /// How the Viewport colours toolpaths (the toolpath view's colour
    /// mode).
    SetColour(dc_app::ColourMode),
    /// Open every operation's eye (the toolpath view's *Show all*).
    ShowAllToolpaths,
    /// Close every operation's eye (the toolpath view's *Hide all*).
    HideAllToolpaths,
    /// Show an operation in the editor (its row clicked).
    EditOperation(dc_project::OperationId),
    /// *Add ▸ kind* on these shapes, from the Operations step or the Design
    /// Tree.
    AddOperation {
        kind: dc_app::operations::Kind,
        targets: Vec<dc_project::InstanceId>,
    },
    /// An operation row's eye: its toolpath hidden or shown (view state).
    ToggleOperationHidden(dc_project::OperationId),
    /// A Warnings row was clicked: what it names selected, and the step
    /// that edits it opened.
    OpenRow {
        selection: Option<dc_app::Selection>,
        op: Option<dc_project::OperationId>,
        field: Option<String>,
    },
    /// *Place tabs* on a Profile (the editor's button, the operation's
    /// menu): its request placed, what placing found in the Warnings
    /// panel (#546).
    PlaceTabs(dc_project::OperationId),
    /// A click on an operation's cut in the Viewport (#576): the
    /// operation in the editor, the Operations step open, its row
    /// scrolled into view. The shape selection stays.
    ShowOperation(dc_project::OperationId),
    /// The Targets row's *Edit*: target editing on this operation (#619;
    /// capability 67).
    EditTargets(dc_project::OperationId),
    /// A click on a shape or its Design Tree row in target editing: in
    /// the draft or out of it.
    ToggleTarget(dc_project::InstanceId),
    /// A marquee in target editing: these shapes added to the draft, or
    /// with Alt held taken out of it.
    BoxTargets {
        ids: Vec<dc_project::InstanceId>,
        remove: bool,
    },
    /// The target-editing bar's *Done* (Enter): the draft as one step.
    CommitTargets,
    /// The target-editing bar's *Cancel* (Escape).
    CancelTargets,
    /// Open the export sheet (File ▸ Export G-code…, the header's
    /// *Export G-code*, the Export step, ⌘E; #547).
    OpenExport,
    /// Open the toolpath view's popover (View ▸ Toolpaths…, capability
    /// 79a), as the status bar's *Toolpaths* button does.
    OpenToolpaths,
    /// Something the export sheet asked for.
    Export(panels::export::Request),
    /// Save the last export's setup sheet (the result's footer, File ▸
    /// Save Setup Sheet…; capability 77b).
    SaveSetupSheet,
}

impl Action {
    /// [`Action::Run`] of `command`.
    pub(crate) fn run(command: Command) -> Self {
        Self::Run(Box::new(command))
    }

    /// Whether the action still runs in target editing (capability 67:
    /// "nothing else can be selected or moved"): the mode's own and the
    /// camera's. Every other is dropped, whatever raised it.
    const fn in_target_editing(&self) -> bool {
        matches!(
            self,
            Self::ToggleTarget(_)
                | Self::BoxTargets { .. }
                | Self::CommitTargets
                | Self::CancelTargets
                | Self::SetCamera(_)
                | Self::FitView
                | Self::Refused(_)
        )
    }
}

/// The unsaved-changes sheet's answers (`experience.md`, section 8,
/// "Destructive actions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Save,
    /// *Don't Save*: go ahead, losing the changes.
    DontSave,
    Cancel,
}

/// What waits on the unsaved-changes sheet.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    Close,
    New,
    Open,
    OpenPath(PathBuf),
    Example(Example),
    NewFromDrawing,
}

/// The app's window.
pub struct Shell {
    app: App,
    config: Option<Paths>,
    dialogs: Box<dyn Dialogs>,
    /// `gui.toml` as last read or written.
    written: GuiPrefs,
    /// The action the unsaved-changes sheet is asking about.
    sheet: Option<Pending>,
    /// The Tool Library sheet, while open.
    library: Option<panels::library::Sheet>,
    /// The Import drawing sheet, while open.
    import: Option<panels::import::Sheet>,
    /// The export sheet, while open (#547).
    export: Option<panels::export::Sheet>,
    /// The setup sheet of this project's last export, for File ▸ Save
    /// Setup Sheet… (capability 77b); gone when another project opens.
    setup_sheet: Option<panels::export::SetupSheet>,
    /// `config.toml`'s `[estimate]`: the estimates' settings, as the CLI
    /// reads them (capability 78).
    estimate: EstimateConfig,
    /// The start screen's example thumbnails, built when it first shows
    /// and kept: never per frame.
    thumbnails: Vec<(Example, Thumbnail)>,
    /// The recent files the start screen lists: the preference's list as
    /// last looked at, and those of it that exist. Looked at when the
    /// list changes, never per frame.
    recent: (Vec<PathBuf>, Vec<PathBuf>),
    /// The status bar's last message.
    message: Option<String>,
    layout: Layout,
    /// The step whose panel shows (capability 50).
    step: Step,
    /// The steps' states as the rail marks them: the project's, with a
    /// mark held while toolpaths regenerate ([`steps::settle`]).
    states: steps::States,
    /// How many of the notes in the Warnings panel are an importer's
    /// warnings about a drawing: the Design step's mark.
    import_warnings: usize,
    /// The snapshot the last export wrote: the Export step is complete
    /// while it is the current one.
    exported: Option<SnapshotId>,
    /// [`App::replaced`] as last seen, to tell a newly opened project.
    seen_project: Option<u64>,
    /// The title last sent to the window.
    title: String,
    close_allowed: bool,
    /// The Viewport's gestures and layer caches.
    viewport: viewport::ViewportState,
    /// The generation thread.
    worker: Worker,
    /// Whether the Warnings drawer is open.
    warnings_open: bool,
    /// The carve note the drawer scrolls to in the next frame it draws.
    note_target: Option<dc_project::OperationId>,
    /// Where the keyboard focus was after the last frame (#548).
    focus: focus::Shown,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("source", &self.app.source())
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// The shell on a new project, with the preferences in
    /// `options.config` and the project `options.open` opened. A
    /// `gui.toml` that cannot be read is the defaults and a note in the
    /// status bar, never a refusal to start (design, "Preferences").
    ///
    /// # Errors
    ///
    /// [`dc_app::CommandError::PostProcessor`] when the bundled grblHAL
    /// definition a new project names cannot be read, which no release
    /// ships.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        options: Options,
    ) -> Result<Self, dc_app::CommandError> {
        let mut message = None;
        let written = match &options.config {
            Some(paths) => {
                let loaded = gui_prefs::load(paths);
                message = loaded.notes.first().cloned();
                loaded.prefs
            }
            None => GuiPrefs::default(),
        };
        let prefs = files::prefs_from_file(&written);
        // `config.toml`'s `[estimate]`, as the CLI reads it for the
        // printed estimates (capability 78); unreadable is the defaults
        // and a note.
        let estimate = match options.config.as_ref().map(config::load_from) {
            Some(Ok(config)) => config.estimate,
            Some(Err(e)) => {
                message = Some(e.to_string());
                EstimateConfig::default()
            }
            None => EstimateConfig::default(),
        };
        let (worker, failed) = Worker::spawn(estimate);
        if let Some(e) = failed {
            message = Some(e.to_string());
        }
        theme::apply(&cc.egui_ctx, prefs.theme);
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            viewport::renderer::install(render_state);
        }
        let app = App::new(App::new_project()?, Strings::english(), prefs);
        let mut shell = Self {
            app,
            config: options.config,
            dialogs: options.dialogs,
            written,
            sheet: None,
            library: None,
            import: None,
            export: None,
            setup_sheet: None,
            estimate,
            thumbnails: Vec::new(),
            recent: (Vec::new(), Vec::new()),
            message,
            layout: Layout {
                rail: egui::Rect::NOTHING,
                step_panel: None,
                inspector: None,
                viewport: egui::Rect::NOTHING,
                window: Vec2::ZERO,
            },
            step: Step::Design,
            states: steps::States::default(),
            import_warnings: 0,
            exported: None,
            seen_project: None,
            title: String::new(),
            close_allowed: false,
            viewport: viewport::ViewportState::default(),
            worker,
            warnings_open: false,
            note_target: None,
            focus: focus::Shown::default(),
        };
        shell.viewport.fit_on_next_frame();
        if let Some(path) = options.open {
            shell.open_path(&path);
        }
        Ok(shell)
    }

    /// The application state.
    #[must_use]
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The operations whose toolpaths the Viewport drew in the last
    /// frame, bottom first: what an operation's eye, *Show all*, *Hide
    /// all* and *Only the selected operation* change.
    #[must_use]
    pub fn drawn_toolpaths(&self) -> &[dc_project::OperationId] {
        self.viewport.drawn()
    }

    /// The step whose panel shows.
    #[must_use]
    pub fn step(&self) -> Step {
        self.step
    }

    /// The steps' states as the rail marks them.
    #[must_use]
    pub fn step_states(&self) -> steps::States {
        self.states
    }

    /// The start screen's example cards' thumbnails, in the cards' order:
    /// empty until the screen first shows.
    #[must_use]
    pub fn example_thumbnails(&self) -> &[(Example, Thumbnail)] {
        &self.thumbnails
    }

    /// The application state, to change.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Whether the generation thread is running a piece of work, kept or
    /// not.
    #[must_use]
    pub fn worker_busy(&self) -> bool {
        self.worker.busy()
    }

    /// Whether the export sheet is building its plan, or will once
    /// generation is over (#547).
    #[must_use]
    pub fn export_planning(&self) -> bool {
        self.export
            .as_ref()
            .is_some_and(|sheet| sheet.planning(&self.app))
    }

    /// Whether the Warnings drawer is open.
    #[must_use]
    pub fn warnings_open(&self) -> bool {
        self.warnings_open
    }

    /// The last frame's layout.
    #[must_use]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The window's title as last sent.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The part of the window holding the keyboard focus after the last
    /// frame, or `None` when nothing has it.
    #[must_use]
    pub fn focus(&self) -> Option<crate::FocusRegion> {
        self.focus.region
    }

    /// Where the last frame drew the keyboard focus ring: `None` when
    /// nothing has the focus, or it came from a click (`experience.md`,
    /// section 8, "Keyboard focus").
    #[must_use]
    pub fn focus_ring(&self) -> Option<egui::Rect> {
        self.focus.ring
    }

    /// The status bar's message.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Whether the shell has let the window close: no unsaved changes, or
    /// the user answered the sheet.
    #[must_use]
    pub fn close_allowed(&self) -> bool {
        self.close_allowed
    }

    fn strings(&self) -> &Strings {
        self.app.strings()
    }

    /// The name the window title and the sheets use (capability 50): the
    /// file's name without its extension, or *Untitled* before the
    /// project has a file.
    pub(crate) fn display_name(&self) -> String {
        self.app.source().map_or_else(
            || self.strings().get(StrKey::AppUntitled).to_owned(),
            files::stem,
        )
    }

    /// What the steps read that the project does not hold.
    fn session(&self) -> steps::Session {
        steps::Session {
            // Saving clears the notes, and the importer's among them.
            import_warnings: self.import_warnings.min(self.app.notes().len()),
            exported: self.exported,
        }
    }

    /// The steps' states for this frame, and what a newly opened project
    /// starts with, once per project: its first step that is not complete
    /// (capability 50), and no tool in the Inspector.
    fn follow_project(&mut self, ctx: &egui::Context) {
        let now = steps::states(&self.app, &self.session());
        let replaced = self.app.replaced();
        if self.seen_project == Some(replaced) {
            let generating = self.app.generation().busy();
            self.states = steps::settle(&self.states, now, generating);
        } else {
            self.seen_project = Some(replaced);
            self.states = now;
            self.step = steps::opening(&now);
            panels::tools::select(ctx, None);
            // The last export, and its setup sheet, are the old project's.
            self.setup_sheet = None;
            self.exported = None;
        }
    }

    /// Opens `step`, with its panel showing.
    fn go_to(&mut self, step: Step) {
        self.step = step;
        self.app.prefs_mut().panels.left_open = true;
    }

    /// Disarms a pick on the Viewport whose button is no longer in view:
    /// the park point's is the Export step's, a point row's its
    /// operation's editor's.
    fn disarm_hidden_picks(&self, ctx: &egui::Context) {
        if self.step != Step::Export {
            panels::project::arm_park_pick(ctx, false);
        }
        if let Some((picking, _)) = panels::editor::point_pick(ctx)
            && (self.step != Step::Operations || self.app.editing() != Some(&picking))
        {
            panels::editor::arm_point_pick(ctx, None);
        }
    }

    /// Asks for `pending`, through the unsaved-changes sheet when the
    /// project has unsaved changes.
    fn request(&mut self, ctx: &egui::Context, pending: Pending) {
        if self.app.is_dirty() {
            self.sheet = Some(pending);
        } else {
            self.perform(ctx, pending);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::Close => {
                self.close_allowed = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Pending::New => match App::new_project() {
                Ok(project) => {
                    self.app.reset(project);
                    self.import_warnings = 0;
                    self.viewport.fit_on_next_frame();
                }
                Err(e) => self.message = Some(e.to_string()),
            },
            Pending::Open => {
                let title = self.strings().get(StrKey::AppDialogOpenTitle).to_owned();
                if let Some(path) = self.dialogs.open_project(&title) {
                    self.open_path(&path);
                }
            }
            Pending::OpenPath(path) => self.open_path(&path),
            Pending::Example(example) => self.open_example(example),
            Pending::NewFromDrawing => {
                let title = self
                    .strings()
                    .get(StrKey::AppDialogNewFromDrawingTitle)
                    .to_owned();
                if let Some(path) = self.dialogs.open_drawing(&title) {
                    self.open_import(&path, Purpose::New);
                }
            }
        }
    }

    /// Opens the project at `path`, or says why not.
    fn open_path(&mut self, path: &Path) {
        let name = files::file_name(path);
        let result = files::read(path).and_then(|bytes| {
            self.app
                .open(&bytes, Some(path.to_path_buf()))
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(opened) => {
                self.app.prefs_mut().push_recent(path.to_path_buf());
                self.import_warnings = 0;
                self.viewport.fit_on_next_frame();
                let count = opened.notes.len().to_string();
                // The project's name, as the title has it (capability
                // 50b: *Saved plaque*).
                let name = files::stem(path);
                self.message = Some(if opened.notes.is_empty() {
                    self.strings()
                        .fmt(StrKey::AppMessageOpened, &[("name", &name)])
                } else {
                    self.strings().fmt(
                        StrKey::AppMessageOpenedNotes,
                        &[("name", &name), ("count", &count)],
                    )
                });
            }
            Err(reason) => {
                self.message = Some(self.strings().fmt(
                    StrKey::AppMessageOpenFailed,
                    &[("name", &name), ("reason", &reason)],
                ));
            }
        }
    }

    /// Opens `example` as an untitled copy: no file behind it, so Save
    /// asks where.
    fn open_example(&mut self, example: Example) {
        let result = match example.dcv() {
            Some(bytes) => self
                .app
                .open(bytes, None)
                .map(drop)
                .map_err(|e| e.to_string()),
            None => coaster().map(|project| self.app.reset(project)),
        };
        self.import_warnings = 0;
        self.viewport.fit_on_next_frame();
        if let Err(reason) = result {
            self.message = Some(reason);
        }
    }

    /// Runs Arrange `action` on the selection (capability 58a): one step
    /// over the unlocked shapes, the status bar naming the locked ones; a
    /// disabled group's reason, or a refusal, in the status bar instead. A
    /// turn that changes nothing (a typed 0° or 360°) is no step and says
    /// nothing.
    fn arrange(&mut self, action: dc_app::arrange::Action) {
        let selected = self.app.selection().instances().to_vec();
        match dc_app::arrange::arrange(self.app.project(), &selected, 0, action) {
            Ok(arranged) => match self.app.run(arranged.command) {
                Ok(_) => {
                    if let Some(line) =
                        dc_app::arrange::locked_message(self.app.strings(), &arranged.locked)
                    {
                        self.message = Some(line);
                    }
                }
                Err(dc_app::CommandError::NothingToDo) => {}
                Err(e) => self.message = Some(e.to_string()),
            },
            Err(why) => self.message = Some(self.strings().get(why.reason()).to_owned()),
        }
    }

    /// Saves to the project's file, or asks where when `as_new` or when
    /// it has none. Whether it was written.
    fn save(&mut self, as_new: bool) -> bool {
        let path = match self.app.source() {
            Some(path) if !as_new => path.to_path_buf(),
            source => {
                let file_name = source.map_or_else(
                    || format!("{}.dcv", self.app.project().name),
                    files::file_name,
                );
                let title = self.strings().get(StrKey::AppDialogSaveTitle).to_owned();
                match self.dialogs.save_project(&title, &file_name) {
                    Some(path) => path,
                    None => return false,
                }
            }
        };
        let name = files::file_name(&path);
        let written = self
            .app
            .serialize()
            .map_err(|e| e.to_string())
            .and_then(|text| {
                files::write(&path, text.as_bytes())
                    .map(|()| text)
                    .map_err(|e| e.to_string())
            });
        match written {
            Ok(text) => {
                self.app.mark_saved(text.as_bytes(), Some(path.clone()));
                let name = files::stem(&path);
                self.app.prefs_mut().push_recent(path);
                self.message = Some(
                    self.strings()
                        .fmt(StrKey::AppMessageSaved, &[("name", &name)]),
                );
                true
            }
            Err(reason) => {
                self.message = Some(self.strings().fmt(
                    StrKey::AppMessageSaveFailed,
                    &[("name", &name), ("reason", &reason)],
                ));
                false
            }
        }
    }

    /// Opens the Import drawing sheet on the drawing at `path`, to import
    /// into the project or, for `Purpose::New`, to make a new project
    /// from; or says why not. The file is read here and parsed by the
    /// sheet's model.
    fn open_import(&mut self, path: &Path, purpose: Purpose) {
        let name = files::file_name(path);
        let format = match files::kind(path) {
            Some(files::FileKind::Svg) => DrawingFormat::Svg,
            Some(files::FileKind::Dxf) => DrawingFormat::Dxf,
            Some(files::FileKind::Picture) => {
                self.message = Some(
                    self.strings()
                        .get(StrKey::AppEmptyPictureUnavailable)
                        .to_owned(),
                );
                return;
            }
            _ => {
                self.message = Some(
                    self.strings()
                        .fmt(StrKey::AppMessageNotADrawing, &[("name", &name)]),
                );
                return;
            }
        };
        match files::read(path) {
            Ok(bytes) => {
                self.import = Some(panels::import::Sheet::new(
                    path, &name, format, bytes, purpose,
                ));
            }
            Err(reason) => {
                self.message = Some(self.strings().fmt(
                    StrKey::AppMessageImportFailed,
                    &[("name", &name), ("reason", &reason)],
                ));
            }
        }
    }

    /// What the Import drawing sheet asked for.
    fn import_request(&mut self, request: panels::import::Request) {
        let Some(sheet) = self.import.take() else {
            return;
        };
        match request {
            panels::import::Request::Close => {}
            panels::import::Request::Choose => {
                let title = self.strings().get(StrKey::AppDialogDrawingTitle).to_owned();
                let purpose = sheet.model().purpose();
                match self.dialogs.open_drawing(&title) {
                    Some(path) => self.open_import(&path, purpose),
                    None => self.import = Some(sheet),
                }
            }
            panels::import::Request::Confirm => {
                let model = sheet.model();
                let name = model.name().to_owned();
                // The importer's warnings become notes: the Design step
                // marks them until the file is saved.
                let noted = self.app.notes().len();
                let result = match model.purpose() {
                    Purpose::Import => self.app.import_drawing(model).map(|_| {
                        self.import_warnings += self.app.notes().len().saturating_sub(noted);
                        let count = self.app.selection().instances().len().to_string();
                        self.strings().fmt(
                            StrKey::AppMessageImported,
                            &[("count", &count), ("name", &name)],
                        )
                    }),
                    Purpose::New => self.app.new_from_drawing(model).map(|()| {
                        self.import_warnings = self.app.notes().len();
                        self.viewport.fit_on_next_frame();
                        self.strings()
                            .fmt(StrKey::AppMessageNewFromDrawing, &[("name", &name)])
                    }),
                };
                match result {
                    Ok(message) => self.message = Some(message),
                    Err(e) => {
                        let reason = e.to_string();
                        self.message = Some(self.strings().fmt(
                            StrKey::AppMessageImportFailed,
                            &[("name", &name), ("reason", &reason)],
                        ));
                        self.import = Some(sheet);
                    }
                }
            }
        }
    }

    /// What the window's close button, dropped files and the keyboard ask
    /// for, before the frame is drawn.
    fn read_input(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // The window's close button and a dropped file open a sheet, and
        // a sheet's buttons do not run under target editing: either one
        // is Cancel first (capability 67).
        if ctx.input(|i| i.viewport().close_requested() || !i.raw.dropped_files.is_empty()) {
            self.app.cancel_targets();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.close_allowed {
            if self.app.is_dirty() {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.sheet = Some(Pending::Close);
            } else {
                self.close_allowed = true;
            }
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        // A drop never opens a sheet above another: a project replaces the
        // Import drawing sheet, and nothing is taken while the
        // unsaved-changes sheet or the Tool Library is open.
        let other_sheet = self.sheet.is_some() || self.library.is_some() || self.export.is_some();
        for path in dropped {
            match files::kind(&path) {
                _ if other_sheet => {}
                Some(files::FileKind::Project) => {
                    self.import = None;
                    self.request(ctx, Pending::OpenPath(path));
                }
                _ => self.open_import(&path, Purpose::Import),
            }
        }
        let sheet_open = self.sheet.is_some()
            || self.library.is_some()
            || self.import.is_some()
            || self.export.is_some();
        focus::begin(ctx, sheet_open);
        if self.app.target_editing().is_some() {
            // Target editing has two keys, Enter for Done and Escape for
            // Cancel; no other shortcut runs under it (capability 67).
            if !sheet_open {
                let (done, cancel) = ctx.input_mut(|input| {
                    (
                        input.consume_key(Modifiers::NONE, Key::Enter),
                        input.consume_key(Modifiers::NONE, Key::Escape),
                    )
                });
                if cancel {
                    actions.push(Action::CancelTargets);
                } else if done {
                    actions.push(Action::CommitTargets);
                }
            }
        } else if !sheet_open {
            actions.extend(shortcuts(ctx, &self.app));
            actions.extend(viewport::keys(ctx, &self.app));
        }
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::New => self.request(ctx, Pending::New),
            Action::Open => self.request(ctx, Pending::Open),
            Action::OpenRecent(path) => self.request(ctx, Pending::OpenPath(path)),
            Action::OpenExample(example) => self.request(ctx, Pending::Example(example)),
            Action::Save => {
                self.save(false);
            }
            Action::SaveAs => {
                self.save(true);
            }
            Action::ImportDrawing => {
                let title = self.strings().get(StrKey::AppDialogDrawingTitle).to_owned();
                if let Some(path) = self.dialogs.open_drawing(&title) {
                    self.open_import(&path, Purpose::Import);
                }
            }
            Action::NewFromDrawing => self.request(ctx, Pending::NewFromDrawing),
            Action::ImportSheet(request) => self.import_request(request),
            Action::Undo => {
                self.app.undo();
            }
            Action::Redo => {
                self.app.redo();
            }
            Action::Quit => self.request(ctx, Pending::Close),
            Action::FitStock { margin_mm } => {
                if let Err(e) = self.app.run(Command::FitStockToShapes { margin_mm }) {
                    self.message = Some(e.to_string());
                }
            }
            Action::SetTheme(theme) => self.app.prefs_mut().theme = theme,
            Action::GoTo(step) => self.go_to(step),
            Action::Continue => self.go_to(steps::next(&self.states, self.step)),
            Action::ToggleStepPanel => {
                let panels = &mut self.app.prefs_mut().panels;
                panels.left_open = !panels.left_open;
            }
            Action::CloseInspector => match self.step {
                Step::Design => self.app.select(dc_app::Selection::Nothing),
                Step::Tools => panels::tools::select(ctx, None),
                Step::Operations => self.app.edit(None),
                Step::Stock | Step::Export => {}
            },
            Action::PruneRecent => {
                files::recent(self.app.prefs_mut());
            }
            Action::UserGuide => ctx.open_url(egui::OpenUrl::new_tab(USER_GUIDE_URL)),
            Action::About => {
                self.message = Some(self.strings().fmt(
                    StrKey::AppMessageAbout,
                    &[("version", env!("CARGO_PKG_VERSION"))],
                ));
            }
            Action::Answer(answer) => self.answer(ctx, answer),
            Action::Run(command) => {
                if let Err(e) = self.app.run(*command) {
                    self.message = Some(e.to_string());
                }
            }
            Action::Arrange(action) => self.arrange(action),
            Action::OpenRotate => {
                // The popover hangs from the Arrange group, which is the
                // Design step's.
                self.go_to(Step::Design);
                panels::arrange::open_rotate(ctx);
            }
            Action::Rename(id) => {
                // The name is edited in its Design Tree row, so the tree
                // shows.
                self.go_to(Step::Design);
                panels::tree::start_rename(ctx, self.app.project(), &id);
            }
            Action::ShowProject => self.go_to(Step::Stock),
            Action::SetSection { name, open } => {
                self.app.prefs_mut().sections.insert(name, open);
            }
            Action::Refused(reason) => self.message = Some(reason),
            Action::OpenLibrary => {
                self.library = Some(panels::library::Sheet::open(
                    self.config.as_ref(),
                    self.app.strings(),
                ));
            }
            Action::Library(panels::library::Request::Close) => self.library = None,
            Action::Library(request) => {
                if let Some(sheet) = self.library.as_mut()
                    && let Some(message) = panels::library::perform(
                        sheet,
                        request,
                        &mut self.app,
                        self.config.as_ref(),
                        self.dialogs.as_mut(),
                    )
                {
                    self.message = Some(message);
                }
            }
            Action::SaveToLibrary(id) => {
                self.message = panels::library::save_to_library(
                    &self.app,
                    self.config.as_ref(),
                    &id,
                    self.library.as_mut(),
                )
                .or(self.message.take());
            }
            Action::SetCamera(camera) => self.app.view_mut().camera = camera,
            Action::SetVisibility(visibility) => self.app.set_visibility(visibility),
            Action::SetColour(colour) => self.app.set_colour(colour),
            Action::OpenToolpaths => {
                egui::Popup::open_id(ctx, crate::shell::toolpaths::popover_id());
            }
            Action::ShowAllToolpaths => self.app.view_mut().show_all(),
            Action::HideAllToolpaths => {
                let scene = self.app.scene().clone();
                self.app.view_mut().hide_all(&scene);
            }
            Action::Select(selection) => self.app.select(selection),
            Action::FitView => {
                let app = &self.app;
                let bounds = dc_app::viewport::camera::fit_bounds(app.project(), app.scene());
                let reserve = self.viewport.fit_reserve_px();
                self.app.view_mut().camera.fit_beside(bounds, reserve);
                self.viewport.mark_fitted(true);
                ctx.request_repaint();
            }
            Action::ZoomToSelection => {
                let selected = self.app.selection().instances();
                if let Some(bounds) = self.app.scene().bounds_of(selected) {
                    self.app.view_mut().camera.fit(bounds);
                    self.viewport.mark_fitted(false);
                    ctx.request_repaint();
                }
            }
            Action::ClearSelection => {
                self.app.select(dc_app::Selection::Nothing);
                self.viewport.reset_clicks();
                ctx.request_repaint();
            }
            Action::OpenRow {
                selection,
                op,
                field,
            } => {
                let owner = selection.as_ref().and_then(steps::owner);
                if let Some(selection) = selection {
                    self.app.select(selection);
                }
                match (owner, op, field) {
                    // An operation's field: the editor, at that field.
                    (_, Some(op), Some(field)) => {
                        self.app.edit(Some(op.clone()));
                        panels::editor::request_focus(ctx, op, field);
                        self.go_to(Step::Operations);
                    }
                    (Some(step), _, _) => self.go_to(step),
                    // A field of no operation and no shape: the step that
                    // edits it, and a tool's shown in the Inspector.
                    (None, _, Some(field)) => {
                        let step = steps::step_of_field(&field);
                        if step == Step::Tools {
                            let tool = panels::tools::tool_in_field(self.app.project(), &field);
                            panels::tools::select(ctx, tool.as_ref());
                        }
                        self.go_to(step);
                    }
                    (None, _, None) => {}
                }
            }
            Action::ToggleWarnings => self.warnings_open = !self.warnings_open,
            Action::ShowNote(op) => {
                self.warnings_open = true;
                self.note_target = Some(op);
            }
            // The OS clipboard needs eframe's `clipboard` feature, off
            // until `deny.toml` admits its BSL-1.0 Windows crates (#532);
            // egui keeps the text until then.
            Action::CopyWarnings => ctx.copy_text(dc_app::warnings::copy_text(&self.app)),
            Action::StopGeneration => self.app.stop(),
            Action::RegenerateAll => self.app.regenerate_all(),
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Action::EditOperation(id) => {
                self.app.edit(Some(id));
                self.go_to(Step::Operations);
            }
            Action::AddOperation { kind, targets } => {
                let app = &self.app;
                match dc_app::operations::add(app.project(), app.strings(), kind, &targets) {
                    Some(command) => {
                        if let Err(e) = self.app.run(command) {
                            self.message = Some(e.to_string());
                        }
                        self.go_to(Step::Operations);
                    }
                    None => {
                        self.message = Some(
                            self.strings()
                                .get(StrKey::OperationsAddSelectShapes)
                                .to_owned(),
                        );
                    }
                }
            }
            Action::ToggleOperationHidden(id) => self.app.toggle_hidden(&id),
            Action::ShowOperation(op) => {
                self.app.edit(Some(op.clone()));
                panels::operations::reveal(ctx, op);
                self.go_to(Step::Operations);
            }
            Action::EditTargets(op) => {
                // The mode is on the drawing: no pick stays armed under it.
                viewport::tab_tool::arm(ctx, false);
                panels::project::arm_park_pick(ctx, false);
                panels::editor::arm_point_pick(ctx, None);
                self.app.edit_targets(&op);
                self.viewport.reset_clicks();
                ctx.request_repaint();
            }
            Action::ToggleTarget(id) => self.app.toggle_target(&id),
            Action::BoxTargets { ids, remove } => self.app.box_targets(&ids, remove),
            Action::CommitTargets => {
                if let Err(e) = self.app.commit_targets() {
                    self.message = Some(e.to_string());
                }
                ctx.request_repaint();
            }
            Action::CancelTargets => {
                self.app.cancel_targets();
                ctx.request_repaint();
            }
            Action::OpenExport => {
                self.export = Some(panels::export::Sheet::open(
                    &self.app,
                    dreamcarve_shell::clock::today_utc(),
                    self.estimate,
                ));
                ctx.request_repaint();
            }
            Action::Export(request) => self.export_request(request),
            Action::SaveSetupSheet => self.save_setup_sheet(),
            Action::PlaceTabs(op) => {
                let notes = self.app.notes().len();
                match self.app.place_tabs(&op) {
                    // What placing found is in the drawer: open it.
                    Ok(_) if self.app.notes().len() > notes => self.warnings_open = true,
                    Ok(_) => {}
                    Err(e) => self.message = Some(e.to_string()),
                }
            }
        }
    }

    /// Writes the last export's setup sheet where the user picks, beside
    /// the programs by default, through the staged write (capability
    /// 77b). Writing it is not an export: no program, no project change.
    fn save_setup_sheet(&mut self) {
        let Some(setup) = self.setup_sheet.clone() else {
            return;
        };
        let strings = self.strings().clone();
        let title = strings.get(StrKey::ExportSetupSheetTitle);
        let Some(path) = self
            .dialogs
            .save_setup_sheet(title, &setup.dir, &setup.file_name)
        else {
            return;
        };
        let written = StagedFile::write(&path, setup.html.as_bytes())
            .and_then(|staged| commit_all(vec![staged]));
        self.message = Some(match written {
            Ok(()) => strings.fmt(
                StrKey::ExportSetupSheetSaved,
                &[("path", &path.display().to_string())],
            ),
            Err(e) => strings.fmt(
                StrKey::ExportSetupSheetFailed,
                &[("reason", &e.to_string())],
            ),
        });
    }

    /// What the export sheet asked for.
    fn export_request(&mut self, request: panels::export::Request) {
        use panels::export::Request;
        match request {
            Request::Close => self.export = None,
            Request::ChooseFolder => {
                let title = self.strings().get(StrKey::ExportChooseTitle).to_owned();
                let start = self
                    .export
                    .as_ref()
                    .and_then(|s| s.dir().map(Path::to_path_buf));
                if let Some(dir) = self.dialogs.export_folder(&title, start.as_deref())
                    && let Some(sheet) = self.export.as_mut()
                {
                    sheet.set_dir(dir);
                }
            }
            Request::Export => {
                if let Some(sheet) = self.export.as_mut()
                    && let Some(message) = panels::export::export(sheet, &mut self.app)
                {
                    self.message = Some(message);
                    self.setup_sheet = sheet.setup_sheet().cloned();
                    self.exported = Some(self.app.snapshot());
                }
            }
            Request::Copied => {
                let folder = self
                    .setup_sheet
                    .as_ref()
                    .map(|s| s.dir.display().to_string());
                if let Some(path) = folder {
                    let message = self
                        .strings()
                        .fmt(StrKey::ExportResultCopied, &[("path", &path)]);
                    self.message = Some(message);
                }
            }
            Request::ShowInFolder => {
                if let Some(sheet) = &self.export {
                    panels::export::show_in_folder(sheet);
                }
            }
            // The drawer at the first error: open, and what that row
            // names selected, as clicking the row would.
            Request::ShowErrors => {
                self.export = None;
                self.warnings_open = true;
                let first = dc_app::warnings::panel(&self.app)
                    .into_iter()
                    .flat_map(|g| g.rows)
                    .find(|r| r.level == dc_app::warnings::Level::Error);
                if let Some(selection) = first.and_then(|r| r.selection()) {
                    if let Some(step) = steps::owner(&selection) {
                        self.go_to(step);
                    }
                    self.app.select(selection);
                }
            }
        }
    }

    fn answer(&mut self, ctx: &egui::Context, answer: Answer) {
        let Some(pending) = self.sheet.take() else {
            return;
        };
        match answer {
            Answer::Cancel => {}
            Answer::DontSave => self.perform(ctx, pending),
            Answer::Save => {
                if self.save(false) {
                    self.perform(ctx, pending);
                }
            }
        }
    }

    /// Sends the title when it changed: the project's name and `*` when
    /// unsaved (capabilities 50 and 51), as the header shows it.
    fn update_title(&mut self, ctx: &egui::Context) {
        let key = if self.app.is_dirty() {
            StrKey::AppTitleUnsaved
        } else {
            StrKey::AppTitle
        };
        let title = self.strings().fmt(key, &[("name", &self.display_name())]);
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Remembers the window's place for the next session.
    fn record_window(&mut self, ctx: &egui::Context) {
        let (outer, maximized) = ctx.input(|i| {
            (
                i.viewport().outer_rect,
                i.viewport().maximized.unwrap_or(false),
            )
        });
        let inner = ctx.input(|i| i.viewport().inner_rect);
        if let (Some(outer), Some(inner)) = (outer, inner) {
            self.app.prefs_mut().window = Some(dc_app::WindowGeometry {
                x: f64::from(outer.min.x),
                y: f64::from(outer.min.y),
                width: f64::from(inner.width()),
                height: f64::from(inner.height()),
                maximized,
            });
        }
    }

    /// Writes `gui.toml` when a preference changed. The window's place is
    /// written on exit only, since it changes with every move.
    fn persist(&mut self, on_exit: bool) {
        let mut file = files::prefs_to_file(self.app.prefs());
        if !on_exit {
            file.window = self.written.window;
        }
        if file == self.written {
            return;
        }
        if let Some(paths) = &self.config
            && let Err(e) = gui_prefs::save(paths, &file)
        {
            let reason = e.to_string();
            self.message = Some(
                self.strings()
                    .fmt(StrKey::AppMessagePrefsFailed, &[("reason", &reason)]),
            );
        }
        self.written = file;
    }
}

/// The coaster as `dreamcarve init coaster.svg` writes it: the stock round
/// the drawing, the placeholder tool and the Profile, every feed and depth
/// unset (the quick start fills them in; nothing is invented here).
fn coaster() -> Result<dc_project::Project, String> {
    let drawing = files::read_drawing(files::FileKind::Svg, COASTER_SVG)?;
    let post = dc_project::latest_definition("grblhal")
        .map_err(|e| e.to_string())?
        .default_ref();
    dc_job::project_from::drawing("coaster", drawing, post, dc_project::Units::Mm, true)
        .map_err(|e| e.to_string())
}

/// The shortcuts read anywhere outside a sheet, even in a field: the
/// file and history commands of [`keyboard::SHORTCUTS`] (capability 54).
/// The selection's are the Viewport's (`viewport::keys`). ⌘ on macOS, Ctrl
/// elsewhere.
fn shortcuts(ctx: &egui::Context, app: &App) -> Vec<Action> {
    // Most modifiers first, so Shift Ctrl S is never Ctrl S as well.
    let mut table: Vec<&keyboard::Shortcut> = keyboard::SHORTCUTS
        .iter()
        .filter(|s| s.scope == keyboard::Scope::Anywhere)
        .collect();
    table.sort_by_key(|s| std::cmp::Reverse(modifier_count(s.modifiers)));
    let mut pressed = Vec::new();
    ctx.input_mut(|input| {
        for shortcut in table {
            if keyboard::consume(input, shortcut.command) > 0 {
                pressed.push(shortcut.command);
            }
        }
    });
    pressed
        .into_iter()
        .filter_map(|command| shortcut_action(app, command))
        .collect()
}

/// How many modifiers `modifiers` holds.
fn modifier_count(modifiers: Modifiers) -> usize {
    [modifiers.command, modifiers.shift, modifiers.alt]
        .into_iter()
        .filter(|held| *held)
        .count()
}

/// What `command` asks of the app now, as its key and its menu row both
/// run it; `None` when it has nothing to act on (no selection, nothing
/// visible). The nudges and Escape are the Viewport's (`viewport::keys`).
pub(crate) fn shortcut_action(app: &App, command: Shortcut) -> Option<Action> {
    let project = app.project();
    let selected = app.selection().instances();
    let on_selection = |item: dc_app::tree::MenuItem| {
        dc_app::tree::menu_command(project, item, selected).map(Action::run)
    };
    // The Arrange group's own action on a selection; with every shape
    // locked it runs to say so in the status bar (acceptance 66).
    let arrange = |action| (!selected.is_empty()).then_some(Action::Arrange(action));
    match command {
        Shortcut::New => Some(Action::New),
        Shortcut::Open => Some(Action::Open),
        Shortcut::Save => Some(Action::Save),
        Shortcut::SaveAs => Some(Action::SaveAs),
        Shortcut::Import => Some(Action::ImportDrawing),
        Shortcut::Export => Some(Action::OpenExport),
        Shortcut::Quit => Some(Action::Quit),
        Shortcut::Undo => app.can_undo().then_some(Action::Undo),
        Shortcut::Redo => app.can_redo().then_some(Action::Redo),
        Shortcut::SelectAll => {
            let visible: Vec<_> = project
                .instances
                .iter()
                .filter(|i| i.visible)
                .map(|i| i.id.clone())
                .collect();
            (!visible.is_empty()).then_some(Action::Select(dc_app::Selection::Instances(visible)))
        }
        Shortcut::Duplicate => on_selection(dc_app::tree::MenuItem::Duplicate),
        Shortcut::Delete => {
            // Selected tabs (#546) delete as one step, like shapes.
            let tabs = app.selection().tabs();
            if tabs.is_empty() {
                on_selection(dc_app::tree::MenuItem::Delete)
            } else {
                Some(Action::run(dc_app::Command::DeleteTabs(tabs.to_vec())))
            }
        }
        Shortcut::Hide => on_selection(dc_app::tree::MenuItem::Hide),
        Shortcut::Show => on_selection(dc_app::tree::MenuItem::Show),
        Shortcut::Lock => dc_app::tree::toggle_locked(project, selected).map(Action::run),
        Shortcut::Rename => selected.first().cloned().map(Action::Rename),
        Shortcut::RotateClockwise => arrange(dc_app::arrange::Action::RotateClockwise),
        Shortcut::RotateAnticlockwise => arrange(dc_app::arrange::Action::RotateAnticlockwise),
        Shortcut::FlipHorizontal => arrange(dc_app::arrange::Action::FlipHorizontal),
        Shortcut::FlipVertical => arrange(dc_app::arrange::Action::FlipVertical),
        Shortcut::FitView => Some(Action::FitView),
        Shortcut::ZoomToSelection => (!selected.is_empty()).then_some(Action::ZoomToSelection),
        Shortcut::GoToDesign => Some(Action::GoTo(Step::Design)),
        Shortcut::GoToStock => Some(Action::GoTo(Step::Stock)),
        Shortcut::GoToTools => Some(Action::GoTo(Step::Tools)),
        Shortcut::GoToOperations => Some(Action::GoTo(Step::Operations)),
        Shortcut::GoToExport => Some(Action::GoTo(Step::Export)),
        Shortcut::TabTool
        | Shortcut::Nudge
        | Shortcut::NudgeCoarse
        | Shortcut::NudgeFine
        | Shortcut::Escape => None,
    }
}

/// The shortcut text a menu row or hint shows for `action`'s binding.
pub(crate) fn shortcut_text(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(modifiers, key))
}

/// What the frame's widgets read: the app and the shell's view state.
pub(crate) struct FrameState<'a> {
    pub app: &'a App,
    pub prefs: &'a Prefs,
    pub display_name: String,
    /// Whether the Viewport shows the start screen: the project has
    /// nothing to cut and the Design step is open.
    pub show_empty: bool,
    /// The start screen's example cards, each with its thumbnail.
    pub examples: &'a [(Example, Thumbnail)],
    /// The recent files that still exist, for the start screen.
    pub recent: &'a [PathBuf],
    /// The step whose panel shows.
    pub step: Step,
    /// The steps' states, as the rail marks them.
    pub states: steps::States,
    /// What the steps read that the project does not hold.
    pub session: steps::Session,
    /// The status bar's cells, which the header's generation chip and
    /// the rail's count read too.
    pub cells: dc_app::status::StatusBar,
    /// The tool the Tools step shows in the Inspector.
    pub tool: Option<dc_project::ToolId>,
    /// Whether the Warnings drawer is open, for View ▸ Warnings.
    pub warnings_open: bool,
    /// The operation whose carve note the drawer scrolls to this frame.
    pub note_target: Option<&'a dc_project::OperationId>,
    /// The estimates' settings, for the status bar's total and an
    /// operation row's hover (capability 78).
    pub estimate: &'a EstimateConfig,
    /// Whether this project has been exported, so File ▸ Save Setup
    /// Sheet… has a sheet to save.
    pub setup_sheet: bool,
}

impl FrameState<'_> {
    /// Whether target editing is on (capability 67; `experience.md`,
    /// section 8, "Target editing"): every part of the window but the
    /// Viewport and the Design Tree, which the step panel then shows,
    /// draws dimmed and takes no clicks.
    pub(crate) fn inert(&self) -> bool {
        self.app.target_editing().is_some()
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::follow(&ctx, self.app.prefs().theme);
        // The backdrop, under every panel: the gaps the panels leave
        // between their frames show it.
        ui.painter().rect_filled(
            ui.max_rect(),
            0.0,
            theme::Palette::current(ui).surface_window,
        );
        // What the worker finished since the last frame, before anything
        // reads the results.
        self.worker.tick(&mut self.app, Instant::now());
        self.follow_project(&ctx);
        self.disarm_hidden_picks(&ctx);
        let mut actions = Vec::new();
        self.read_input(&ctx, &mut actions);

        let size = ctx.content_rect().size();
        let pointer = self
            .viewport
            .read_pointer(&ctx, self.layout.viewport, &self.app);
        // Each part of the window's rectangle, for Tab's walk
        // (`shell::focus`).
        let mut regions: Vec<(Region, Rect)> = Vec::new();
        let show_empty = steps::blank(self.app.project()) && self.step == Step::Design;
        if show_empty && self.thumbnails.is_empty() {
            // Once, when the start screen first shows: never per frame.
            self.thumbnails = Example::ALL
                .into_iter()
                .map(|example| (example, example.thumbnail()))
                .collect();
        }
        if show_empty && self.recent.0 != self.app.prefs().recent {
            let listed = self.app.prefs().recent.clone();
            let existing = listed.iter().filter(|p| p.is_file()).cloned().collect();
            self.recent = (listed, existing);
        }
        // Asked for by the last frame's message cell: scrolled to once.
        let note_target = if self.warnings_open {
            self.note_target.take()
        } else {
            None
        };
        let cells = dc_app::status::cells(
            &self.app,
            &dc_app::status::Inputs {
                cursor_mm: pointer.cursor_mm,
                drag: pointer.drag.as_deref(),
                hover: pointer
                    .hover
                    .as_ref()
                    .map(|(name, locked)| (name.as_str(), *locked)),
                message: self.message.as_deref(),
                estimate: &self.estimate,
            },
        );
        let state = FrameState {
            app: &self.app,
            prefs: self.app.prefs(),
            display_name: self.display_name(),
            show_empty,
            examples: &self.thumbnails,
            recent: &self.recent.1,
            step: self.step,
            states: self.states,
            session: self.session(),
            cells,
            tool: panels::tools::selected(&ctx, self.app.project()),
            warnings_open: self.warnings_open,
            note_target: note_target.as_ref(),
            estimate: &self.estimate,
            setup_sheet: self.setup_sheet.is_some(),
        };
        focus::mark(ui, Region::Header);
        regions.push((Region::Header, header::bar(ui, &state, &mut actions)));
        focus::mark(ui, Region::Status);
        regions.push((Region::Status, status::bar(ui, &state, &mut actions)));
        focus::mark(ui, Region::Rail);
        let rail = rail::show(ui, &state, &mut actions);
        regions.push((Region::Rail, rail));
        let step_panel = state.prefs.panels.left_open.then(|| {
            focus::mark(ui, Region::Step);
            panels::side::step_panel(ui, &state, size, &mut actions)
        });
        regions.extend(step_panel.map(|shown| (Region::Step, shown.card)));
        // In a window too narrow for all four regions the Inspector lies
        // over the Viewport, drawn after it.
        let inspecting = panels::side::inspecting(&state).is_some();
        let overlaid = size.x < MIN_WINDOW.x;
        let docked = (inspecting && !overlaid).then(|| {
            focus::mark(ui, Region::Inspector);
            panels::side::inspector(ui, &state, size, step_panel.map(|s| s.width), &mut actions)
        });
        let margins = viewport::Margins {
            left: step_panel.map_or(theme::GAP_PX, |_| panels::side::HALF_GAP_PX),
            right: docked.map_or(theme::GAP_PX, |_| panels::side::HALF_GAP_PX),
        };
        // Between the step panel and the Inspector, over the Viewport's
        // bottom.
        if self.warnings_open {
            focus::mark(ui, Region::Drawer);
            let drawer = panels::warnings::drawer(ui, &state, margins, &mut actions);
            regions.push((Region::Drawer, drawer));
        }
        focus::mark(ui, Region::Viewport);
        let viewport = viewport::show(ui, &state, &mut self.viewport, margins, &mut actions);
        regions.push((Region::Viewport, viewport));
        let floating = (inspecting && overlaid).then(|| {
            focus::mark(ui, Region::Inspector);
            panels::side::inspector_over(ui, &state, viewport, &mut actions)
        });
        let inspector = docked.map(|shown| shown.card).or(floating);
        regions.extend(inspector.map(|card| (Region::Inspector, card)));
        focus::mark_end(ui);
        if self.sheet.is_some() {
            panels::unsaved::sheet(&ctx, self.app.strings(), &state.display_name, &mut actions);
        } else if let Some(sheet) = self.library.as_mut() {
            panels::library::show(&ctx, &self.app, &state.display_name, sheet, &mut actions);
        } else if let Some(sheet) = self.import.as_mut() {
            panels::import::show(
                &ctx,
                self.app.strings(),
                self.app.project(),
                sheet,
                &mut actions,
            );
        } else if let Some(sheet) = self.export.as_mut() {
            panels::export::show(&ctx, &self.app, &state.display_name, sheet, &mut actions);
        }
        self.focus = focus::finish(ui, regions);

        // The widths the user dragged the panels to are preferences.
        if let Some(shown) = step_panel {
            self.app.prefs_mut().panels.left_width = f64::from(shown.width);
        }
        if let Some(shown) = docked {
            self.app.prefs_mut().panels.right_width = f64::from(shown.width);
        }
        self.layout = Layout {
            rail,
            step_panel: step_panel.map(|shown| shown.card),
            inspector,
            viewport,
            window: size,
        };

        let selected = dc_app::status::selected_operation(&self.app).cloned();
        let said = self.message.clone();
        // In target editing only the mode's own actions run, whatever a
        // panel or a key raised (capability 67).
        let editing_targets = self.app.target_editing().is_some();
        for action in actions {
            if !editing_targets || action.in_target_editing() {
                self.run(&ctx, action);
            }
        }
        // The operation row under the pointer previews its targets.
        let hovered = panels::operations::take_hovered(&ctx);
        if self.app.previewed() != hovered.as_ref() {
            self.app.preview_targets(hovered);
            ctx.request_repaint();
        }
        // Capability 50b: choosing an operation is newer than the last
        // message, so the message cell shows its carve summary; a message
        // said in the same frame stays.
        let now = dc_app::status::selected_operation(&self.app);
        if now.is_some() && now != selected.as_ref() && self.message == said {
            self.message = None;
        }
        // Post the next work once the frame's edits have settled; look
        // again soon while any is out or waiting.
        if self.worker.tick(&mut self.app, Instant::now()) {
            ctx.request_repaint_after(worker::POLL);
        }
        self.update_title(&ctx);
        self.record_window(&ctx);
        // Once a drag is over: a panel divider writes once, not per frame.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.persist(false);
        }
    }

    fn on_exit(&mut self) {
        self.persist(true);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let palette = if visuals.dark_mode {
            &theme::DARK
        } else {
            &theme::LIGHT
        };
        palette.surface_window.to_normalized_gamma_f32()
    }
}
