// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The window (`docs/specs/phase-4/design.md`, "Architecture → The
//! shell"; functional spec, capabilities 50, 51 and 79): one [`Shell`]
//! implementing `eframe::App`, which owns the `dc_app::App` and every
//! file, dialog and preference write, and never holds project data of its
//! own. Each frame it follows the OS appearance, answers the window's
//! close button and dropped files, reads the keyboard shortcuts, draws the
//! menu bar, toolbar, status bar, side panels and Viewport, and then runs
//! what the widgets asked for.
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
use dc_app::{App, Command, Prefs, StrKey, Strings};
use dc_job::EstimateConfig;
use dreamcarve_shell::{GuiPrefs, Paths, config, gui_prefs};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, Vec2, ViewportCommand};

use crate::shell::worker::{self, Worker};
use crate::shell::{files, menu, status, theme, toolbar};
use crate::{panels, viewport};

/// The quick starts' example projects (functional spec, capability 79).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Example {
    /// The phase 1 coaster: `fixtures/coaster.svg` as `dreamcarve init`
    /// makes it, its feeds and depths unset as the quick start begins.
    Coaster,
    /// The phase 2 plaque, `tests/fixtures/plaque.dcv`.
    Plaque,
    /// The phase 3 sign, `tests/fixtures/sign.dcv`.
    Sign,
}

/// The coaster's drawing.
const COASTER_SVG: &[u8] = include_bytes!("../../../../fixtures/coaster.svg");
/// The plaque the phase 2 quick start builds.
const PLAQUE_DCV: &[u8] = include_bytes!("../../../../tests/fixtures/plaque.dcv");
/// The sign the phase 3 quick start builds.
const SIGN_DCV: &[u8] = include_bytes!("../../../../tests/fixtures/sign.dcv");

/// The book, for Help ▸ User Guide.
const USER_GUIDE_URL: &str = "https://grayzu.github.io/dreamcarve/";

/// The smallest window that holds both side panels at their minimum
/// (functional spec, capability 50; `experience.md`, section 3): below it
/// they collapse to strips.
pub const MIN_WINDOW: Vec2 = egui::vec2(1280.0, 768.0);
/// The left column's minimum width, px.
pub const LEFT_MIN_PX: f32 = 240.0;
/// The CAM panel's minimum width, px.
pub const RIGHT_MIN_PX: f32 = 300.0;
/// The Viewport's minimum width, px.
pub const VIEWPORT_MIN_PX: f32 = 560.0;
/// A collapsed panel's strip, px.
pub const STRIP_PX: f32 = 32.0;

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
}

/// The OS's open and save dialogs, through `rfd` (design open question 3).
#[derive(Debug, Clone)]
pub struct NativeDialogs {
    project_filter: String,
    drawing_filter: String,
    library_filter: String,
}

impl NativeDialogs {
    /// Dialogs whose filters are named from `strings`.
    #[must_use]
    pub fn new(strings: &Strings) -> Self {
        Self {
            project_filter: strings.get(StrKey::AppDialogProjectFilter).to_owned(),
            drawing_filter: strings.get(StrKey::AppDialogDrawingFilter).to_owned(),
            library_filter: strings.get(StrKey::AppDialogLibraryFilter).to_owned(),
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

/// How a side panel was shown in the last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelShown {
    /// Open, this wide, px.
    Open {
        /// The panel's width.
        width: f32,
    },
    /// Collapsed to its strip.
    Strip,
}

/// The last frame's layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The left column (Design Tree over Properties).
    pub left: PanelShown,
    /// The CAM panel.
    pub right: PanelShown,
    /// The Viewport's rectangle.
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
    ToggleLeft,
    ToggleRight,
    ShowLeft,
    ShowRight,
    DismissEmpty,
    PruneRecent,
    UserGuide,
    About,
    Answer(Answer),
    /// Project ▸ Stock and Origin…: the Project panel, shown with nothing
    /// selected.
    ShowProject,
    /// A panel section opened or closed, kept in `gui.toml`.
    SetSection {
        name: String,
        open: bool,
    },
    /// Run a command on the project; a refusal goes to the status bar.
    Run(Box<Command>),
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
    /// Stop generation (the status bar's *Stop*).
    StopGeneration,
    /// Regenerate every operation (View ▸ Regenerate All, the drawer).
    RegenerateAll,
    /// Open a page of the book (a warning code's chapter entry).
    OpenUrl(String),
}

impl Action {
    /// [`Action::Run`] of `command`.
    pub(crate) fn run(command: Command) -> Self {
        Self::Run(Box::new(command))
    }
}

/// The unsaved-changes sheet's answers (`experience.md`, section 8,
/// "Destructive actions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Save,
    Discard,
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

/// Which tab of the CAM panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CamTab {
    #[default]
    Operations,
    Tools,
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
    empty_dismissed: bool,
    /// The status bar's last message.
    message: Option<String>,
    layout: Layout,
    pub(crate) cam_tab: CamTab,
    /// The title last sent to the window.
    title: String,
    close_allowed: bool,
    /// The window's size in the last frame, to tell a resize.
    last_window: Option<Vec2>,
    /// Panels reopened from their strip in a window below the minimum.
    forced_open: (bool, bool),
    /// The Viewport's gestures and layer caches.
    viewport: viewport::ViewportState,
    /// The generation thread.
    worker: Worker,
    /// Whether the Warnings drawer is open.
    warnings_open: bool,
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
            empty_dismissed: false,
            message,
            layout: Layout {
                left: PanelShown::Strip,
                right: PanelShown::Strip,
                viewport: egui::Rect::NOTHING,
                window: Vec2::ZERO,
            },
            cam_tab: CamTab::default(),
            title: String::new(),
            close_allowed: false,
            last_window: None,
            forced_open: (false, false),
            viewport: viewport::ViewportState::default(),
            worker,
            warnings_open: false,
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

    /// The name the title, the toolbar and the sheet use: the file's name,
    /// or the project's before it has one.
    pub(crate) fn display_name(&self) -> String {
        self.app
            .source()
            .map_or_else(|| self.app.project().name.clone(), files::file_name)
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
                    self.empty_dismissed = false;
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
        let result = files::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                self.app
                    .open(&bytes, Some(path.to_path_buf()))
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(opened) => {
                self.app.prefs_mut().push_recent(path.to_path_buf());
                self.empty_dismissed = false;
                self.viewport.fit_on_next_frame();
                let count = opened.notes.len().to_string();
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

    /// Opens `example` as an unsaved copy: no file behind it, so Save asks
    /// where.
    fn open_example(&mut self, example: Example) {
        let result = match example {
            Example::Plaque => self
                .app
                .open(PLAQUE_DCV, None)
                .map(drop)
                .map_err(|e| e.to_string()),
            Example::Sign => self
                .app
                .open(SIGN_DCV, None)
                .map(drop)
                .map_err(|e| e.to_string()),
            Example::Coaster => coaster().map(|project| self.app.reset(project)),
        };
        self.empty_dismissed = false;
        self.viewport.fit_on_next_frame();
        if let Err(reason) = result {
            self.message = Some(reason);
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
            Err(e) => {
                let reason = e.to_string();
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
                let result = match model.purpose() {
                    Purpose::Import => self.app.import_drawing(model).map(|_| {
                        let count = self.app.selection().instances().len().to_string();
                        self.strings().fmt(
                            StrKey::AppMessageImported,
                            &[("count", &count), ("name", &name)],
                        )
                    }),
                    Purpose::New => self.app.new_from_drawing(model).map(|()| {
                        self.empty_dismissed = false;
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
        let other_sheet = self.sheet.is_some() || self.library.is_some();
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
        if self.sheet.is_none() && self.library.is_none() && self.import.is_none() {
            actions.extend(shortcuts(ctx));
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
            Action::ToggleLeft => {
                let panels = &mut self.app.prefs_mut().panels;
                panels.left_open = !panels.left_open;
                self.forced_open.0 = panels.left_open;
            }
            Action::ToggleRight => {
                let panels = &mut self.app.prefs_mut().panels;
                panels.right_open = !panels.right_open;
                self.forced_open.1 = panels.right_open;
            }
            Action::ShowLeft => {
                self.app.prefs_mut().panels.left_open = true;
                self.forced_open.0 = true;
            }
            Action::ShowRight => {
                self.app.prefs_mut().panels.right_open = true;
                self.forced_open.1 = true;
            }
            Action::DismissEmpty => self.empty_dismissed = true,
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
            Action::ShowProject => {
                self.app.select(dc_app::Selection::Nothing);
                self.app.prefs_mut().panels.left_open = true;
                self.forced_open.0 = true;
            }
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
            Action::Select(selection) => self.app.select(selection),
            Action::FitView => {
                let app = &self.app;
                let bounds = dc_app::viewport::camera::fit_bounds(app.project(), app.scene());
                self.app.view_mut().camera.fit(bounds);
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
            Action::ToggleWarnings => self.warnings_open = !self.warnings_open,
            // The OS clipboard needs eframe's `clipboard` feature, off
            // until `deny.toml` admits its BSL-1.0 Windows crates (#532);
            // egui keeps the text until then.
            Action::CopyWarnings => ctx.copy_text(dc_app::warnings::copy_text(&self.app)),
            Action::StopGeneration => self.app.stop(),
            Action::RegenerateAll => self.app.regenerate_all(),
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        }
    }

    fn answer(&mut self, ctx: &egui::Context, answer: Answer) {
        let Some(pending) = self.sheet.take() else {
            return;
        };
        match answer {
            Answer::Cancel => {}
            Answer::Discard => self.perform(ctx, pending),
            Answer::Save => {
                if self.save(false) {
                    self.perform(ctx, pending);
                }
            }
        }
    }

    /// Sends the title when it changed: the file's name and a marker when
    /// unsaved (capability 51).
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

    /// Lays out the side panels for a window of `size`: below the minimum
    /// they show as strips unless reopened from one since the last resize.
    fn panels_open(&mut self, size: Vec2) -> (bool, bool) {
        if self.last_window != Some(size) {
            self.forced_open = (false, false);
            self.last_window = Some(size);
        }
        let fits = size.x >= MIN_WINDOW.x && size.y >= MIN_WINDOW.y;
        let panels = self.app.prefs().panels;
        (
            panels.left_open && (fits || self.forced_open.0),
            panels.right_open && (fits || self.forced_open.1),
        )
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

/// The file and edit shortcuts (functional spec, capability 54; the rest
/// arrive with slice 19). ⌘ on macOS, Ctrl elsewhere.
fn shortcuts(ctx: &egui::Context) -> Vec<Action> {
    let shift_command = Modifiers::COMMAND | Modifiers::SHIFT;
    let bindings = [
        (KeyboardShortcut::new(shift_command, Key::S), Action::SaveAs),
        (KeyboardShortcut::new(shift_command, Key::Z), Action::Redo),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
            Action::Save,
        ),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
            Action::Undo,
        ),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            Action::Open,
        ),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::N),
            Action::New,
        ),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::I),
            Action::ImportDrawing,
        ),
        (
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Q),
            Action::Quit,
        ),
    ];
    let mut actions = Vec::new();
    ctx.input_mut(|input| {
        for (shortcut, action) in bindings {
            if input.consume_shortcut(&shortcut) {
                actions.push(action);
            }
        }
    });
    actions
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
    pub message: Option<&'a str>,
    pub show_empty: bool,
    /// The pointer over the Viewport, for the status bar.
    pub pointer: viewport::Pointer,
    /// Whether the Warnings drawer is open, for View ▸ Warnings.
    pub warnings_open: bool,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::follow(&ctx, self.app.prefs().theme);
        // What the worker finished since the last frame, before anything
        // reads the results.
        self.worker.tick(&mut self.app, Instant::now());
        let mut actions = Vec::new();
        self.read_input(&ctx, &mut actions);

        let size = ctx.content_rect().size();
        let (left_open, right_open) = self.panels_open(size);
        let pointer = self
            .viewport
            .read_pointer(&ctx, self.layout.viewport, &self.app);
        let state = FrameState {
            app: &self.app,
            prefs: self.app.prefs(),
            display_name: self.display_name(),
            message: self.message.as_deref(),
            show_empty: self.app.project().instances.is_empty() && !self.empty_dismissed,
            pointer,
            warnings_open: self.warnings_open,
        };
        menu::bar(ui, &state, &mut actions);
        toolbar::bar(ui, &state, &mut actions);
        status::bar(ui, &state, &mut actions);
        let left = panels::side::left(ui, &state, left_open, size, right_open, &mut actions);
        let mut cam_tab = self.cam_tab;
        let right = panels::side::right(ui, &state, right_open, size, &mut cam_tab, &mut actions);
        // Between the side panels, over the Viewport's bottom.
        if self.warnings_open {
            panels::warnings::drawer(ui, &state, &mut actions);
        }
        let viewport = viewport::show(ui, &state, &mut self.viewport, &mut actions);
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
        }
        self.cam_tab = cam_tab;

        // A panel the user dragged closed is a preference; one closed for
        // the window's size is not.
        let fits = size.x >= MIN_WINDOW.x && size.y >= MIN_WINDOW.y;
        if let panels::side::Shown {
            width: Some(w),
            dragged_closed,
        } = left
        {
            self.app.prefs_mut().panels.left_width = f64::from(w);
            if dragged_closed && fits {
                self.app.prefs_mut().panels.left_open = false;
            }
        }
        if let panels::side::Shown {
            width: Some(w),
            dragged_closed,
        } = right
        {
            self.app.prefs_mut().panels.right_width = f64::from(w);
            if dragged_closed && fits {
                self.app.prefs_mut().panels.right_open = false;
            }
        }
        self.layout = Layout {
            left: left
                .width
                .map_or(PanelShown::Strip, |width| PanelShown::Open { width }),
            right: right
                .width
                .map_or(PanelShown::Strip, |width| PanelShown::Open { width }),
            viewport,
            window: size,
        };

        for action in actions {
            self.run(&ctx, action);
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
