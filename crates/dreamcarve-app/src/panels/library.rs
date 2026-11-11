// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Tool Library sheet (functional spec, capabilities 64 and 64a;
//! acceptance 71 and 101; mock-up `04-tool-library.svg`; `experience.md`,
//! sections 7, 8 and 15): a sheet over the library file in the config
//! directory, read and written through `dreamcarve_shell::library` as the
//! CLI's `tools` commands read and write it.
//!
//! Two panes (slice 30, #622). The left: a search field (name, vendor,
//! product, diameter), a type filter, *New tool*, *Duplicate* and
//! *Delete*, and the tools grouped by vendor (a collapsible header per
//! vendor, *No vendor* last), each row ticked for *Add to project* or
//! *Delete*; rows already in the project are marked. The right edits the
//! selected tool in place with the project tool editor's rows: selecting
//! is editing, with no Edit button. A field commits on leave and writes
//! the library at once (no undo; *Revert changes* puts the tool back as
//! the sheet opened it); a refused value or a failed write marks the
//! field with the reason. The header holds the library's own `mm` / `in`
//! (`Prefs::library_units`), *Import…* and *Export library…*.
//!
//! *Delete* asks first, naming the ticked tools (the selected one when
//! none is ticked), in place of the panes (#581). *Import…* reads a
//! Fusion `.tools` or `.json` or a DreamCarve library and shows the
//! per-tool report `tools import` prints in place of the panes, with
//! *Back* (never a sheet above a sheet); *Replace existing* is `--replace`
//! until the import review (#623) takes it.
//!
//! The model is `dc_app::library`; the sheet keeps the search, the ticks,
//! the selection and the session between frames. A library change never
//! alters the open project (ADR-0002): *Add to project* copies, under new
//! ids, as `tools add` does, and is one undo step; nothing else here is
//! in the project's history.

use std::collections::BTreeSet;

use dc_app::library::{self as model, Filter, KindFilter, LibraryEdit};
use dc_app::tools::{self, KindChoice, ToolField};
use dc_app::{App, Command, StrKey, Strings};
use dc_library::import::{ImportReport, Outcome};
use dc_library::{Library, LibraryToolId};
use dc_project::{Tool, ToolId, ToolKind, Units};
use dreamcarve_shell::Paths;
use eframe::egui::{self, Align, Id, Layout, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::panels::tools::{FieldEdit, ToolFields, tool_fields};
use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Dialogs};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::{checkbox, paint_box};
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented;

/// The sheet's default size, px (`experience.md`, section 3).
const SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The editing pane's width, px (section 3).
const PANE_PX: f32 = 320.0;
/// The gap between the panes, px.
const GAP_PX: f32 = 16.0;
/// A header, a table row, a vendor group's band and a sheet footer
/// button, px.
const HEADER_PX: f32 = 32.0;
const ROW_PX: f32 = 28.0;
const GROUP_PX: f32 = 28.0;
const FOOTER_PX: f32 = 28.0;
/// The tick column, px.
const TICK_PX: f32 = 32.0;
/// The units control's width, px.
const UNITS_PX: f32 = 96.0;

/// The sheet's state between frames.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sheet {
    /// The library as last read or written; the message when it cannot be
    /// read, and then nothing writes it.
    library: Result<Library, String>,
    /// The library file, as the title's tooltip shows it.
    path: String,
    /// The library as the sheet opened it, for *Revert changes*.
    session: LibraryEdit,
    filter: Filter,
    ticked: BTreeSet<LibraryToolId>,
    /// The row the editing pane edits.
    selected: Option<LibraryToolId>,
    /// The vendor groups closed, by vendor (`None` is *No vendor*).
    collapsed: BTreeSet<Option<String>>,
    replace: bool,
    /// The last import's file name and report, shown in place of the panes.
    report: Option<(String, ImportReport)>,
    /// Whether *Delete* is asking first, in place of the panes (#581).
    confirm_remove: bool,
    /// The tool whose switch to V-bit waits for its angle (#581).
    held: Option<LibraryToolId>,
    /// The tool whose name to focus, selected, when the pane next draws it
    /// (*New tool*).
    focus_name: Option<LibraryToolId>,
    /// A field marked with why its last change did not take.
    marked: Option<(LibraryToolId, ToolField, String)>,
    /// The last action's message, and whether it is an error.
    message: Option<(String, bool)>,
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Import,
    Export,
    Remove,
    Add,
    Close,
    /// *New tool* in the library.
    NewTool,
    /// *Duplicate* the selected tool.
    Duplicate,
    /// *Revert changes* to the selected tool.
    Revert,
    /// Library tool `.0` is now `.1`, from a change to field `.2`.
    Set(LibraryToolId, Box<Tool>, ToolField),
    /// The library's display units.
    Units(Units),
}

impl Sheet {
    /// The sheet over the library at `config`, read now. A missing file is
    /// an empty library; an unreadable one is its message.
    pub(crate) fn open(config: Option<&Paths>, strings: &Strings) -> Self {
        let (library, path) = match config {
            Some(paths) => (read(paths), paths.library_file.display().to_string()),
            None => (
                Err(strings.get(StrKey::AppLibraryUnavailable).to_owned()),
                String::new(),
            ),
        };
        let session = library
            .as_ref()
            .map_or_else(|_| LibraryEdit::default(), LibraryEdit::open);
        Self {
            library,
            path,
            session,
            filter: Filter::default(),
            ticked: BTreeSet::new(),
            selected: None,
            collapsed: BTreeSet::new(),
            replace: false,
            report: None,
            confirm_remove: false,
            held: None,
            focus_name: None,
            marked: None,
            message: None,
        }
    }

    /// The ids *Delete* acts on: the ticked tools, else the selected one.
    fn to_delete(&self) -> BTreeSet<LibraryToolId> {
        if self.ticked.is_empty() {
            self.selected.iter().cloned().collect()
        } else {
            self.ticked.clone()
        }
    }

    /// The names of the tools *Delete* acts on, in library order: what its
    /// question names.
    fn delete_names(&self) -> Vec<&str> {
        let ids = self.to_delete();
        self.library.as_ref().map_or_else(
            |_| Vec::new(),
            |library| {
                library
                    .tools
                    .iter()
                    .filter(|t| ids.contains(&t.id))
                    .map(|t| t.tool.name.as_str())
                    .collect()
            },
        )
    }

    /// Takes `library` as the one on disk, dropping ticks and the
    /// selection on rows it no longer has.
    pub(crate) fn set_library(&mut self, library: Library) {
        let has = |id: &LibraryToolId| library.tools.iter().any(|t| t.id == *id);
        self.ticked.retain(has);
        if !self.selected.as_ref().is_some_and(has) {
            self.selected = None;
        }
        self.library = Ok(library);
    }
}

/// The library file at `paths`, a missing one empty.
fn read(paths: &Paths) -> Result<Library, String> {
    dreamcarve_shell::library::read(paths)
        .map(Option::unwrap_or_default)
        .map_err(|e| e.to_string())
}

/// The type filter's label.
fn kind_filter_label(strings: &Strings, kind: KindFilter) -> &str {
    strings.get(match kind {
        KindFilter::All => StrKey::AppKindAll,
        KindFilter::EndMill => StrKey::AppKindEndMill,
        KindFilter::BallNose => StrKey::AppKindBallNose,
        KindFilter::VBit => StrKey::AppKindVBit,
        KindFilter::Engraver => StrKey::AppKindEngraver,
        KindFilter::Drill => StrKey::AppKindDrill,
    })
}

/// A cutter type's label.
pub(crate) fn kind_label(strings: &Strings, choice: KindChoice) -> &str {
    strings.get(match choice {
        KindChoice::EndMill => StrKey::AppKindEndMill,
        KindChoice::BallNose => StrKey::AppKindBallNose,
        KindChoice::VBit => StrKey::AppKindVBit,
        KindChoice::Engraver => StrKey::AppKindEngraver,
        KindChoice::Drill => StrKey::AppKindDrill,
    })
}

/// The units the sheet shows: the library's own once chosen, else the
/// project's (capability 64).
fn library_units(app: &App) -> Units {
    app.prefs()
        .library_units
        .unwrap_or(app.project().display_units)
}

/// Shows the sheet; what the user asked for goes to `out`.
pub(crate) fn show(
    ctx: &egui::Context,
    app: &App,
    project_name: &str,
    sheet: &mut Sheet,
    out: &mut Vec<Action>,
) {
    let style = ctx.global_style();
    let palette = if style.visuals.dark_mode {
        &theme::DARK
    } else {
        &theme::LIGHT
    };
    let screen = ctx.content_rect().size();
    let size = egui::vec2(SIZE.x.min(screen.x - 48.0), SIZE.y.min(screen.y - 48.0));
    let frame = egui::Frame::popup(&style)
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .inner_margin(MARGIN_PX);
    let response = egui::Modal::new(Id::unique("tool_library"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            let inner = size - egui::vec2(2.0, 2.0) * f32::from(MARGIN_PX);
            ui.set_min_size(inner);
            ui.set_max_size(inner);
            body(ui, inner.y, app, project_name, sheet, out);
        });
    if response.should_close() {
        if sheet.confirm_remove {
            // Escape, or a click on the scrim, answers the question first.
            sheet.confirm_remove = false;
        } else {
            out.push(Action::Library(Request::Close));
        }
    }
}

fn body(
    ui: &mut Ui,
    height: f32,
    app: &App,
    project_name: &str,
    sheet: &mut Sheet,
    out: &mut Vec<Action>,
) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let top = ui.cursor().top();
    header(ui, app, sheet, out);
    ui.add_space(12.0);
    let footer_h = FOOTER_PX + 24.0;
    // The panes take what the header and footer leave; an `Area`'s
    // available height is the screen's, so it is worked out.
    let used = ui.cursor().top() - top;
    let panes_h = (height - used - footer_h).max(ROW_PX * 4.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), panes_h), |ui| {
        ui.set_min_height(panes_h);
        ui.set_max_height(panes_h);
        if let Some((name, report)) = &sheet.report {
            report_view(ui, strings, name, report, &sheet.path);
        } else if sheet.confirm_remove {
            remove_question(ui, strings, &sheet.delete_names());
        } else if let Err(message) = &sheet.library {
            sentence(ui, message, palette.status_error.text);
        } else {
            panes(ui, app, sheet, out);
        }
    });
    if let Some((message, error)) = &sheet.message {
        let colour = if *error {
            palette.status_error.text
        } else {
            palette.text_secondary
        };
        text(ui, message, TextStyle::Small, colour);
    } else {
        ui.add_space(16.0);
    }
    ui.add_space(4.0);
    footer(ui, app, project_name, sheet, out);
}

/// The 32-px header: the title (the library file its tooltip), then at
/// the right the library's units, *Import…*, *Export library…* and `x`.
fn header(ui: &mut Ui, app: &App, sheet: &Sheet, out: &mut Vec<Action>) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let readable = sheet.library.is_ok();
    let in_place = sheet.report.is_some() || sheet.confirm_remove;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), HEADER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let close = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::X, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::AppLibraryClose));
            if button(ui, close).clicked() {
                out.push(Action::Library(Request::Close));
            }
            ui.add_space(8.0);
            let export = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryExport))
                .tooltip(strings.get(StrKey::AppLibraryExportTooltip))
                .enabled(readable && !in_place);
            if button(ui, export).clicked() {
                out.push(Action::Library(Request::Export));
            }
            let import = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryImport))
                .enabled(readable && !in_place);
            if button(ui, import).clicked() {
                out.push(Action::Library(Request::Import));
            }
            ui.add_space(8.0);
            units_control(ui, app, out);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let title = text(
                    ui,
                    strings.get(StrKey::AppLibraryTitle),
                    TextStyle::Name(theme::DISPLAY.into()),
                    palette.text_primary,
                );
                if !sheet.path.is_empty() {
                    title.on_hover_text(
                        strings.fmt(StrKey::AppLibraryTitleTooltip, &[("path", &sheet.path)]),
                    );
                }
            });
        },
    );
}

/// The library's `mm` / `in` (capability 64; section 15, "Library
/// units"): a preference, never a library or project change.
fn units_control(ui: &mut Ui, app: &App, out: &mut Vec<Action>) {
    const OPTIONS: [Units; 2] = [Units::Mm, Units::Inch];
    let strings = app.strings();
    let labels = [
        strings.get(StrKey::ProjectUnitsMm),
        strings.get(StrKey::ProjectUnitsIn),
    ];
    let units = library_units(app);
    let selected = OPTIONS.iter().position(|u| *u == units).unwrap_or(0);
    let shown = ui.allocate_ui_with_layout(
        egui::vec2(UNITS_PX, HEADER_PX),
        Layout::left_to_right(Align::Center),
        |ui| segmented(ui, Id::unique("library.units"), &labels, selected, UNITS_PX),
    );
    shown
        .response
        .on_hover_text(strings.get(StrKey::AppLibraryUnitsTooltip));
    if let Some(picked) = shown.inner.and_then(|n| OPTIONS.get(n)) {
        out.push(Action::Library(Request::Units(*picked)));
    }
}

/// The two panes: the grouped table on the left, the editing pane on the
/// right; an empty library's sentence and its two buttons instead of the
/// table.
fn panes(ui: &mut Ui, app: &App, sheet: &mut Sheet, out: &mut Vec<Action>) {
    let Ok(library) = sheet.library.clone() else {
        return;
    };
    let units = library_units(app);
    let states = model::annotate(&library, &app.project().tools);
    let groups = model::groups(&states, &sheet.filter, units);
    // The selection follows the table: the first shown row when the
    // selected one is gone.
    if !sheet
        .selected
        .as_ref()
        .is_some_and(|id| library.tools.iter().any(|t| t.id == *id))
    {
        sheet.selected = groups
            .first()
            .and_then(|g| g.rows.first())
            .map(|s| s.entry.id.clone());
    }
    if sheet.held.is_some() && sheet.held != sheet.selected {
        sheet.held = None;
    }
    let height = ui.available_height();
    let left_w = (ui.available_width() - PANE_PX - GAP_PX).max(240.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(left_w, height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_size(egui::vec2(left_w, height));
                ui.set_max_width(left_w);
                if library.tools.is_empty() {
                    empty_library(ui, app.strings(), out);
                } else {
                    toolbar(ui, app.strings(), sheet, out);
                    ui.add_space(8.0);
                    table(ui, app, units, &groups, sheet);
                }
            },
        );
        ui.add_space(GAP_PX / 2.0);
        let palette = Palette::current(ui);
        let (line, _) = ui.allocate_exact_size(egui::vec2(1.0, height), Sense::hover());
        ui.painter().vline(
            line.center().x,
            line.y_range(),
            Stroke::new(1.0, palette.border_default),
        );
        ui.add_space(GAP_PX / 2.0 - 1.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(height);
                editing_pane(ui, app, units, &library, sheet, out);
            },
        );
    });
}

/// An empty library (capability 64): its sentence, then the two ways to
/// fill it, *New tool* and *Import…*.
fn empty_library(ui: &mut Ui, strings: &Strings, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    sentence(
        ui,
        strings.get(StrKey::AppLibraryEmpty),
        palette.text_secondary,
    );
    ui.add_space(8.0);
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let pair_id = Id::unique("library_empty_pair");
        let pair = ui.data(|d| d.get_temp::<f32>(pair_id));
        ui.add_space(pair.map_or(0.0, |w| ((width - w) / 2.0).max(0.0)));
        let start = ui.cursor().left();
        new_tool_button(ui, strings, out);
        let import = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryImport));
        if button(ui, import).clicked() {
            out.push(Action::Library(Request::Import));
        }
        let used = ui.min_rect().right() - start;
        ui.data_mut(|d| d.insert_temp(pair_id, used));
    });
}

/// *New tool*, the same command as the Tools tab's (capability 63).
fn new_tool_button(ui: &mut Ui, strings: &Strings, out: &mut Vec<Action>) {
    let new = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryNewTool))
        .icon(Icon::Plus, theme::ICON_SMALL_PX);
    if button(ui, new).clicked() {
        out.push(Action::Library(Request::NewTool));
    }
}

/// The search field and the type filter, then *New tool*, *Duplicate*
/// and *Delete* over the list (and *Replace existing* for *Import…*).
fn toolbar(ui: &mut Ui, strings: &Strings, sheet: &mut Sheet, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let search_w = (ui.available_width() - 160.0).clamp(160.0, 320.0);
        search_field(ui, strings, &mut sheet.filter.search, search_w);
        ui.add_space(8.0);
        egui::ComboBox::from_id_salt("library_kind")
            .icon(crate::widgets::icon::combo_chevron)
            .width(ui.available_width().min(150.0))
            .selected_text(kind_filter_label(strings, sheet.filter.kind))
            .show_ui(ui, |ui| {
                for kind in KindFilter::ALL {
                    ui.selectable_value(
                        &mut sheet.filter.kind,
                        kind,
                        kind_filter_label(strings, kind),
                    );
                }
            })
            .response
            .on_hover_text(strings.get(StrKey::AppToolsKind));
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        new_tool_button(ui, strings, out);
        let duplicate = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryDuplicate))
            .icon(Icon::Copy, theme::ICON_SMALL_PX)
            .enabled(sheet.selected.is_some());
        if button(ui, duplicate).clicked() {
            out.push(Action::Library(Request::Duplicate));
        }
        let delete = ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppLibraryDelete))
            .icon(Icon::Trash2, theme::ICON_SMALL_PX)
            .enabled(!sheet.to_delete().is_empty());
        if button(ui, delete).clicked() {
            // The library is outside project history: ask first.
            sheet.confirm_remove = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            checkbox(
                ui,
                "library.replace",
                &mut sheet.replace,
                strings.get(StrKey::AppLibraryReplace),
            )
            .on_hover_text(strings.get(StrKey::AppLibraryReplaceTooltip));
        });
    });
}

/// The search field (section 7, "Search field"): a leading `search`
/// glyph, a trailing `x` that clears it while it has text. It filters as
/// typed.
fn search_field(ui: &mut Ui, strings: &Strings, search: &mut String, width: f32) {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::hover());
    let glyph = Icon::Search.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    ui.painter().galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - glyph.size().y / 2.0),
        glyph,
        palette.text_secondary,
    );
    let clear_w = if search.is_empty() { 0.0 } else { 26.0 };
    let edit_rect = Rect::from_min_max(
        rect.min + egui::vec2(8.0 + theme::ICON_SMALL_PX + 8.0, 0.0),
        rect.max - egui::vec2(4.0 + clear_w, 0.0),
    );
    let response = ui.put(
        edit_rect,
        egui::TextEdit::singleline(search)
            .id(Id::unique("library_search"))
            .frame(egui::Frame::NONE)
            .font(TextStyle::Body)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(
                egui::RichText::new(strings.get(StrKey::AppLibrarySearch))
                    .color(palette.text_disabled),
            )
            .desired_width(edit_rect.width()),
    );
    let stroke = if response.has_focus() {
        palette.accent
    } else {
        palette.border_strong
    };
    ui.painter()
        .rect_stroke(rect, 4.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    if !search.is_empty() {
        let clear_rect = Rect::from_min_size(
            egui::pos2(rect.right() - 26.0, rect.top()),
            egui::vec2(26.0, 26.0),
        );
        let clear = ButtonSpec::new(Kind::Quiet, "")
            .icon(Icon::X, theme::ICON_SMALL_PX)
            .tooltip(strings.get(StrKey::AppLibraryClearSearch));
        if ui
            .put(clear_rect, |ui: &mut Ui| button(ui, clear))
            .clicked()
        {
            search.clear();
        }
    }
}

/// The table's columns after the tick: label, width (0 is the rest) and
/// whether numbers right-align. Vendor is the group, not a column
/// (capability 64).
/// The product cell ends with the "in project" mark.
const COLUMNS: [(StrKey, f32, bool); 9] = [
    (StrKey::AppLibraryColumnName, 0.0, false),
    (StrKey::AppLibraryColumnKind, 66.0, false),
    (StrKey::AppLibraryColumnDiameter, 54.0, true),
    (StrKey::AppLibraryColumnAngle, 40.0, true),
    (StrKey::AppLibraryColumnFlutes, 44.0, true),
    (StrKey::AppLibraryColumnFeed, 48.0, true),
    (StrKey::AppLibraryColumnPlunge, 50.0, true),
    (StrKey::AppLibraryColumnRpm, 50.0, true),
    (StrKey::AppLibraryColumnProduct, 72.0, false),
];
/// The "in project" mark's width at the product cell's right end, px.
const MARK_PX: f32 = 18.0;

/// Each column's rect across `row` after the tick.
fn column_rects(row: Rect) -> Vec<Rect> {
    let fixed: f32 = COLUMNS.iter().map(|(_, w, _)| w).sum();
    let mut x = row.left() + TICK_PX;
    let rest = (row.width() - TICK_PX - fixed).max(80.0);
    COLUMNS
        .iter()
        .map(|(_, w, _)| {
            let width = if *w > 0.0 { *w } else { rest };
            let rect =
                Rect::from_min_size(egui::pos2(x, row.top()), egui::vec2(width, row.height()));
            x += width;
            rect
        })
        .collect()
}

/// `text` in `rect`, one line, an ellipsis when it does not fit.
fn cell(ui: &Ui, rect: Rect, text: &str, right: bool, colour: egui::Color32, style: &TextStyle) {
    let inner = rect.shrink2(egui::vec2(4.0, 0.0));
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        style.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: inner.width(),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let x = if right {
        inner.right() - galley.size().x
    } else {
        inner.left()
    };
    ui.painter().with_clip_rect(rect).galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
}

/// The table: a header row, then each vendor group's band and its rows,
/// striped within the group.
fn table(
    ui: &mut Ui,
    app: &App,
    units: Units,
    groups: &[model::VendorGroup<'_, '_>],
    sheet: &mut Sheet,
) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let (head, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    for ((key, _, right), rect) in COLUMNS.iter().zip(column_rects(head)) {
        cell(
            ui,
            rect,
            strings.get(*key),
            *right,
            palette.text_secondary,
            &TextStyle::Small,
        );
    }
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, palette.border_default),
    );
    if groups.is_empty() {
        sentence(
            ui,
            strings.get(StrKey::AppLibraryNoMatch),
            palette.text_secondary,
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("library_table")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for group in groups {
                let key = group.vendor.map(str::to_owned);
                let open = !sheet.collapsed.contains(&key);
                if group_header(ui, strings, group, open) {
                    if open {
                        sheet.collapsed.insert(key);
                    } else {
                        sheet.collapsed.remove(&key);
                    }
                }
                if !open {
                    continue;
                }
                for (n, state) in group.rows.iter().enumerate() {
                    table_row(ui, strings, units, n, state, sheet);
                }
            }
        });
}

/// A vendor group's 28-px band (section 7, "Vendor group header"): the
/// chevron, the vendor in 13-px SemiBold and its count in text.secondary.
/// Returns whether it was clicked, which opens or closes the group.
fn group_header(
    ui: &mut Ui,
    strings: &Strings,
    group: &model::VendorGroup<'_, '_>,
    open: bool,
) -> bool {
    let palette = Palette::current(ui);
    let vendor = group
        .vendor
        .unwrap_or_else(|| strings.get(StrKey::AppLibraryNoVendor));
    let count = group.rows.len().to_string();
    let label = strings.fmt(
        StrKey::AppLibraryGroup,
        &[("vendor", vendor), ("count", &count)],
    );
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), GROUP_PX), Sense::hover());
    let id = Id::unique(("library_group", group.vendor.unwrap_or_default()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, &label));
    let fill = if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface_panel
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let glyph = chevron.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    ui.painter().galley(
        egui::pos2(
            rect.left() + TICK_PX / 2.0 - glyph.size().x / 2.0,
            rect.center().y - glyph.size().y / 2.0,
        ),
        glyph,
        palette.text_secondary,
    );
    let name = ui.painter().layout_no_wrap(
        vendor.to_owned(),
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    let name_w = name.size().x;
    let x = rect.left() + TICK_PX;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name.size().y / 2.0),
        name,
        palette.text_primary,
    );
    let tail = ui.painter().layout_no_wrap(
        format!(" · {count}"),
        TextStyle::Body.resolve(ui.style()),
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(x + name_w, rect.center().y - tail.size().y / 2.0),
        tail,
        palette.text_secondary,
    );
    response.clicked()
}

/// One row: its tick (for *Add to project* and *Delete*) and the row
/// itself, which selects the tool for the editing pane.
fn table_row(
    ui: &mut Ui,
    strings: &Strings,
    units: Units,
    n: usize,
    state: &model::EntryState<'_>,
    sheet: &mut Sheet,
) {
    let palette = Palette::current(ui);
    let entry = state.entry;
    let tool = &entry.tool;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let chosen = sheet.selected.as_ref() == Some(&entry.id);
    // The row first, the tick over its left end: egui gives a click to
    // the widget on top.
    let row = ui.interact(
        rect,
        Id::unique(("library_row", entry.id.as_str())),
        Sense::click(),
    );
    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, chosen, &tool.name));
    if row.clicked() && !chosen {
        sheet.selected = Some(entry.id.clone());
        sheet.held = None;
    }
    let tick_rect = Rect::from_min_size(rect.min, egui::vec2(TICK_PX, ROW_PX));
    let tick = ui.interact(
        tick_rect,
        Id::unique(("library_tick", entry.id.as_str())),
        Sense::click(),
    );
    let on = sheet.ticked.contains(&entry.id);
    tick.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &tool.name));
    if tick.clicked() {
        if on {
            sheet.ticked.remove(&entry.id);
        } else {
            sheet.ticked.insert(entry.id.clone());
        }
    }
    let on = sheet.ticked.contains(&entry.id);
    let chosen = sheet.selected.as_ref() == Some(&entry.id);
    let fill = if chosen {
        palette.accent_soft
    } else if row.hovered() || tick.hovered() {
        palette.surface_hover
    } else if n % 2 == 1 {
        palette.surface_stripe
    } else {
        theme::NONE
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    if chosen {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
            0.0,
            palette.accent,
        );
    }

    // The 16-px tick box.
    let tick_box = Rect::from_center_size(tick_rect.center(), egui::vec2(16.0, 16.0));
    paint_box(ui, tick_box, on, tick.hovered(), true);

    let colour = if state.in_project {
        palette.text_secondary
    } else {
        palette.text_primary
    };
    let unset = palette.status_error.text;
    // An unset rate is a red dash in its cell (mock-up 04), never a value.
    let rate = |v: Option<f64>| {
        v.map_or_else(
            || ("—".to_owned(), unset),
            |v| (model::rate_text(v, units), colour),
        )
    };
    let angle = match tool.cutter {
        ToolKind::VBit { included_angle_deg } => format!("{included_angle_deg}°"),
        _ => String::new(),
    };
    let cells = [
        (tool.name.clone(), colour),
        (
            kind_label(strings, KindChoice::of(&tool.cutter)).to_owned(),
            colour,
        ),
        (model::diameter_text(tool.diameter_mm, units), colour),
        (angle, colour),
        (tool.flutes.to_string(), colour),
        rate(tool.feed_mm_min),
        rate(tool.plunge_mm_min),
        tool.rpm
            .map_or_else(|| ("—".to_owned(), unset), |r| (r.to_string(), colour)),
        (tool.product_id.clone().unwrap_or_default(), colour),
    ];
    let mut rects = column_rects(rect);
    // The product cell gives its right end to the "in project" mark.
    if state.in_project
        && let Some(product) = rects.last_mut()
    {
        let mark_rect = Rect::from_min_max(
            egui::pos2(product.right() - MARK_PX, product.top()),
            product.max,
        );
        product.max.x -= MARK_PX;
        mark(ui, strings, mark_rect, &entry.id);
    }
    for (((value, value_colour), (_, _, right)), r) in cells.iter().zip(COLUMNS.iter()).zip(&rects)
    {
        cell(ui, *r, value, *right, *value_colour, &TextStyle::Body);
    }
    if row.hovered() {
        // The whole name (a narrow column cuts it), then what it lacks.
        let mut lines = vec![tool.name.clone()];
        lines.extend(tools::unset_rates(tool).into_iter().map(|(_, m)| m));
        row.on_hover_text(lines.join("\n"));
    }
}

/// The "in project" mark: a `check` in text.secondary, named for screen
/// readers (section 7, "Table rows").
fn mark(ui: &mut Ui, strings: &Strings, rect: Rect, id: &LibraryToolId) {
    let palette = Palette::current(ui);
    let glyph = Icon::Check.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    ui.painter().galley(
        rect.center() - glyph.size() / 2.0,
        glyph,
        palette.text_secondary,
    );
    let label = strings.get(StrKey::AppLibraryInProject);
    let response = ui.interact(
        rect,
        Id::unique(("library_in_project", id.as_str())),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
    response.on_hover_text(label);
}

/// The editing pane (capability 64a): the selected tool's glyph, name and
/// vendor, its fields as the project tool editor shows them, in the
/// library's units, then the note and *Revert changes*.
fn editing_pane(
    ui: &mut Ui,
    app: &App,
    units: Units,
    library: &Library,
    sheet: &mut Sheet,
    out: &mut Vec<Action>,
) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let Some(entry) = sheet
        .selected
        .as_ref()
        .and_then(|id| library.tools.iter().find(|t| t.id == *id))
    else {
        if !library.tools.is_empty() {
            sentence(
                ui,
                strings.get(StrKey::AppLibrarySelectHint),
                palette.text_secondary,
            );
        }
        return;
    };
    let id = entry.id.clone();
    let tool = &entry.tool;
    // The pane's header: glyph, name, `· vendor`.
    ui.horizontal(|ui| {
        let (glyph, _) = ui.allocate_exact_size(egui::vec2(20.0, 26.0), Sense::hover());
        super::editor::paint_tool_icon(ui, glyph.center(), &tool.cutter, palette.text_secondary);
        text(
            ui,
            &tool.name,
            TextStyle::Name(theme::TITLE.into()),
            palette.text_primary,
        );
        if let Some(vendor) = tool.vendor.as_deref().filter(|v| !v.trim().is_empty()) {
            text(
                ui,
                &format!("· {vendor}"),
                TextStyle::Body,
                palette.text_secondary,
            );
        }
    });
    ui.add_space(8.0);
    let footer_h = 26.0 + 8.0 + 20.0;
    let held = sheet.held.as_ref() == Some(&id) && !matches!(tool.cutter, ToolKind::VBit { .. });
    let focus_name = sheet.focus_name.as_ref() == Some(&id);
    let marked = sheet
        .marked
        .as_ref()
        .filter(|(m, _, _)| *m == id)
        .map(|(_, field, reason)| (*field, reason.as_str()));
    let output = egui::ScrollArea::vertical()
        .id_salt(("library_pane", id.as_str()))
        .auto_shrink(false)
        .max_height((ui.available_height() - footer_h).max(0.0))
        .show(ui, |ui| {
            tool_fields(
                ui,
                &ToolFields {
                    salt: Id::unique(("library_tool_field", id.as_str())),
                    strings,
                    units,
                    tool,
                    held,
                    focus_name,
                    marked,
                },
            )
        })
        .inner;
    if focus_name {
        sheet.focus_name = None;
    }
    match output.hold {
        Some(true) => sheet.held = Some(id.clone()),
        Some(false) => sheet.held = None,
        None => {}
    }
    match output.edit {
        Some(FieldEdit::Set(next, field)) => {
            if next == *tool {
                if sheet
                    .marked
                    .as_ref()
                    .is_some_and(|(m, f, _)| *m == id && *f == field)
                {
                    sheet.marked = None;
                }
            } else {
                let next = Tool {
                    id: ToolId::new(id.as_str()),
                    ..next
                };
                out.push(Action::Library(Request::Set(
                    id.clone(),
                    Box::new(next),
                    field,
                )));
            }
        }
        Some(FieldEdit::Refused(field, reason)) => {
            sheet.marked = Some((id.clone(), field, reason));
        }
        None => {}
    }
    text(
        ui,
        strings.get(StrKey::AppLibrarySavedNote),
        TextStyle::Small,
        palette.text_secondary,
    );
    ui.add_space(8.0);
    let revert = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryRevert))
        .icon(Icon::RotateCcw, theme::ICON_SMALL_PX)
        .tooltip(strings.get(StrKey::AppLibraryRevertTooltip))
        .enabled(sheet.session.changed(library, &id));
    if button(ui, revert).clicked() {
        out.push(Action::Library(Request::Revert));
    }
}

/// One sentence in a pane (section 8, "Empty states").
fn sentence(ui: &mut Ui, sentence: &str, colour: egui::Color32) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(sentence)
                    .text_style(TextStyle::Body)
                    .color(colour),
            )
            .wrap()
            .selectable(false),
        );
    });
}

/// *Delete*'s question in place of the panes (`experience.md`, section 8,
/// "Destructive actions"; #581), naming the tools: the library is outside
/// project history, so a deletion cannot be undone. The footer holds the
/// answers.
fn remove_question(ui: &mut Ui, strings: &Strings, names: &[&str]) {
    let palette = Palette::current(ui);
    let names_text = names.join(", ");
    let count = names.len().to_string();
    let question = match names {
        [_] => strings.fmt(StrKey::AppLibraryDeleteAskOne, &[("names", &names_text)]),
        _ => strings.fmt(
            StrKey::AppLibraryDeleteAskMany,
            &[("count", &count), ("names", &names_text)],
        ),
    };
    sentence(ui, &question, palette.text_primary);
}

/// The import report in place of the panes: a line per tool as `tools
/// import` prints it, skipped items in the warning colour, tools left
/// with an unset value in the error colour, then the counts.
fn report_view(ui: &mut Ui, strings: &Strings, name: &str, report: &ImportReport, path: &str) {
    let palette = Palette::current(ui);
    text(
        ui,
        &strings.fmt(StrKey::AppLibraryReportTitle, &[("name", name)]),
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for entry in &report.entries {
                let colour = match entry.outcome {
                    Outcome::Skipped { .. } => palette.status_warning.text,
                    _ if !entry.notes.is_empty() => palette.status_error.text,
                    _ => palette.text_primary,
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(entry.to_string())
                            .text_style(TextStyle::Monospace)
                            .color(colour),
                    )
                    .selectable(true),
                );
            }
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(report.summary(path))
                        .text_style(TextStyle::Monospace)
                        .color(palette.text_secondary),
                )
                .wrap(),
            );
        });
}

/// The footer: the counts on the left and *Add to project* on the right;
/// *Back* on the left while the report shows; *Cancel* and *Delete* while
/// *Delete* asks.
fn footer(ui: &mut Ui, app: &App, project_name: &str, sheet: &mut Sheet, out: &mut Vec<Action>) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::left_to_right(Align::Center),
        |ui| {
            if sheet.report.is_some() {
                let back = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryBack))
                    .icon(Icon::ArrowLeft, theme::ICON_SMALL_PX)
                    .height(FOOTER_PX);
                if button(ui, back).clicked() {
                    sheet.report = None;
                }
                return;
            }
            if sheet.confirm_remove {
                // *Delete* (destructive) is the answer on the right, with
                // *Cancel* beside it.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let remove =
                        ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppLibraryDelete))
                            .height(FOOTER_PX);
                    if button(ui, remove).clicked() {
                        sheet.confirm_remove = false;
                        out.push(Action::Library(Request::Remove));
                    }
                    let cancel =
                        ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryCancel))
                            .height(FOOTER_PX);
                    if button(ui, cancel).clicked() {
                        sheet.confirm_remove = false;
                    }
                });
                return;
            }
            let Ok(library) = &sheet.library else {
                return;
            };
            let states = model::annotate(library, &app.project().tools);
            let selected = model::count_selected(&states, &sheet.ticked);
            let count = library.tools.len().to_string();
            let picked = selected.to_string();
            text(
                ui,
                &strings.fmt(
                    StrKey::AppLibraryFooter,
                    &[
                        ("count", &count),
                        ("selected", &picked),
                        ("name", project_name),
                    ],
                ),
                TextStyle::Small,
                palette.text_secondary,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = match selected {
                    0 => strings.get(StrKey::AppLibraryAdd).to_owned(),
                    1 => strings.get(StrKey::AppLibraryAddOne).to_owned(),
                    n => strings.fmt(StrKey::AppLibraryAddMany, &[("count", &n.to_string())]),
                };
                let add = ButtonSpec::new(Kind::Primary, &label)
                    .height(FOOTER_PX)
                    .enabled(selected > 0);
                if button(ui, add).clicked() {
                    out.push(Action::Library(Request::Add));
                }
            });
        },
    );
}

/// Runs `request` on the open sheet: the file dialogs, the library file,
/// the preference and the project command. Returns the status bar's
/// message.
pub(crate) fn perform(
    sheet: &mut Sheet,
    request: Request,
    app: &mut App,
    config: Option<&Paths>,
    dialogs: &mut dyn Dialogs,
) -> Option<String> {
    let message = match request {
        Request::Close => return None,
        Request::Units(units) => {
            app.prefs_mut().library_units = Some(units);
            return None;
        }
        Request::Import => import(sheet, app.strings(), config, dialogs),
        Request::Export => export(sheet, app.strings(), dialogs),
        Request::Remove => remove(sheet, app.strings(), config),
        Request::Add => add(sheet, app),
        Request::NewTool => new_tool(sheet, app.strings(), config),
        Request::Duplicate => duplicate(sheet, app.strings(), config),
        Request::Revert => revert(sheet, app.strings(), config),
        Request::Set(id, tool, field) => {
            set(sheet, app.strings(), config, &id, *tool, field);
            return None;
        }
    };
    sheet.message.clone_from(&message);
    message.map(|(text, _)| text)
}

/// Why a library change could not be written.
fn write_failed(strings: &Strings, reason: &str) -> String {
    strings.fmt(StrKey::AppLibraryWriteFailed, &[("reason", reason)])
}

/// Applies `change` to the library as it is on disk now (as `tools`
/// reads it, so a change made elsewhere is kept) and writes the result
/// through the staged write, a failure leaving the old file. On success
/// the sheet takes the library written and `change`'s value is returned.
fn change_library<T>(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
    change: impl FnOnce(&Library) -> Result<(Library, T), String>,
) -> Result<T, String> {
    let paths = config.ok_or_else(|| strings.get(StrKey::AppLibraryUnavailable).to_owned())?;
    let current = read(paths)?;
    let (next, value) = change(&current)?;
    if next != current {
        dreamcarve_shell::library::write(paths, &next)
            .map_err(|e| write_failed(strings, &e.to_string()))?;
    }
    sheet.set_library(next);
    Ok(value)
}

/// A field of the editing pane, left changed: tool `id` set to `tool`
/// and the library written at once (capability 64a). A refusal or a
/// failed write marks the field with the reason; the file is unchanged.
fn set(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
    id: &LibraryToolId,
    tool: Tool,
    field: ToolField,
) {
    let result = change_library(sheet, strings, config, |lib| {
        model::set_tool(lib, id, tool)
            .map(|next| (next, ()))
            .map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => {
            if sheet.marked.as_ref().is_some_and(|(m, _, _)| m == id) {
                sheet.marked = None;
            }
        }
        Err(reason) => sheet.marked = Some((id.clone(), field, reason)),
    }
}

/// *New tool*: the `tool<n>` placeholder added to the library and
/// written, selected with its name focused.
fn new_tool(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
) -> Option<(String, bool)> {
    let made = change_library(sheet, strings, config, |lib| {
        let (next, id) = model::new_tool(lib);
        Ok((next, id))
    });
    Some(match made {
        Ok(id) => {
            sheet.filter = Filter::default();
            sheet.selected = Some(id.clone());
            sheet.focus_name = Some(id.clone());
            let name = tool_name(sheet, &id);
            (
                strings.fmt(
                    StrKey::AppLibraryMade,
                    &[("name", &name), ("id", id.as_str())],
                ),
                false,
            )
        }
        Err(reason) => (reason, true),
    })
}

/// The name of tool `id` in the sheet's library.
fn tool_name(sheet: &Sheet, id: &LibraryToolId) -> String {
    sheet
        .library
        .as_ref()
        .ok()
        .and_then(|lib| lib.tools.iter().find(|t| t.id == *id))
        .map(|t| t.tool.name.clone())
        .unwrap_or_default()
}

/// *Duplicate*: the selected tool copied below itself and selected.
fn duplicate(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
) -> Option<(String, bool)> {
    let id = sheet.selected.clone()?;
    let from = tool_name(sheet, &id);
    let copied = change_library(sheet, strings, config, |lib| {
        model::duplicate(lib, &id).map_err(|e| e.to_string())
    });
    Some(match copied {
        Ok(copy) => {
            sheet.selected = Some(copy.clone());
            let name = tool_name(sheet, &copy);
            (
                strings.fmt(
                    StrKey::AppLibraryDuplicated,
                    &[("from", &from), ("name", &name), ("id", copy.as_str())],
                ),
                false,
            )
        }
        Err(reason) => (reason, true),
    })
}

/// *Revert changes*: the selected tool as the sheet opened it, written.
fn revert(sheet: &mut Sheet, strings: &Strings, config: Option<&Paths>) -> Option<(String, bool)> {
    let id = sheet.selected.clone()?;
    let session = sheet.session.clone();
    let reverted = change_library(sheet, strings, config, |lib| {
        session
            .revert(lib, &id)
            .map(|next| (next, ()))
            .map_err(|e| e.to_string())
    });
    Some(match reverted {
        Ok(()) => {
            sheet.held = None;
            if sheet.marked.as_ref().is_some_and(|(m, _, _)| *m == id) {
                sheet.marked = None;
            }
            let name = tool_name(sheet, &id);
            (
                strings.fmt(StrKey::AppLibraryReverted, &[("name", &name)]),
                false,
            )
        }
        Err(reason) => (reason, true),
    })
}

/// *Import…*: asks for the file, merges it into the library as it is on
/// disk now (as `tools import` reads it), writes the library when it
/// changed, and shows the report.
fn import(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
    dialogs: &mut dyn Dialogs,
) -> Option<(String, bool)> {
    let paths = config?;
    let path = dialogs.open_library(strings.get(StrKey::AppDialogLibraryImportTitle))?;
    let name = files::file_name(&path);
    let failed = |reason: String| {
        Some((
            strings.fmt(
                StrKey::AppLibraryImportFailed,
                &[("name", &name), ("reason", &reason)],
            ),
            true,
        ))
    };
    let bytes = match files::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => return failed(e.to_string()),
    };
    let current = match read(paths) {
        Ok(library) => library,
        Err(reason) => return failed(reason),
    };
    let merged = match model::import(&current, &bytes, sheet.replace) {
        Ok(merged) => merged,
        Err(e) => return failed(e.to_string()),
    };
    if merged.library != current
        && let Err(e) = dreamcarve_shell::library::write(paths, &merged.library)
    {
        return Some((write_failed(strings, &e.to_string()), true));
    }
    sheet.set_library(merged.library);
    sheet.report = Some((name, merged.report));
    None
}

/// *Export library…*: the library as DreamCarve JSON wherever the user
/// says.
fn export(sheet: &Sheet, strings: &Strings, dialogs: &mut dyn Dialogs) -> Option<(String, bool)> {
    let Ok(library) = &sheet.library else {
        return None;
    };
    let path = dialogs.save_library(
        strings.get(StrKey::AppDialogLibraryExportTitle),
        dreamcarve_shell::config_dir::LIBRARY_FILE_NAME,
    )?;
    let name = files::file_name(&path);
    Some(match dreamcarve_shell::library::export(library, &path) {
        Ok(()) => (
            strings.fmt(
                StrKey::AppLibraryExported,
                &[("count", &library.tools.len().to_string()), ("name", &name)],
            ),
            false,
        ),
        Err(e) => (
            strings.fmt(
                StrKey::AppLibraryExportFailed,
                &[("reason", &e.to_string())],
            ),
            true,
        ),
    })
}

/// *Delete*, answered: the ticked tools (else the selected one) out of the
/// library, written at once. Projects keep their copies.
fn remove(sheet: &mut Sheet, strings: &Strings, config: Option<&Paths>) -> Option<(String, bool)> {
    let paths = config?;
    let Ok(library) = &sheet.library else {
        return None;
    };
    let mut next = library.clone();
    let mut removed = Vec::new();
    for id in &sheet.to_delete() {
        if let Ok(tool) = dc_library::remove(&mut next, id.as_str()) {
            removed.push(tool);
        }
    }
    if removed.is_empty() {
        return None;
    }
    if let Err(e) = dreamcarve_shell::library::write(paths, &next) {
        return Some((write_failed(strings, &e.to_string()), true));
    }
    sheet.set_library(next);
    let text = match removed.as_slice() {
        [one] => strings.fmt(
            StrKey::AppLibraryDeletedOne,
            &[("name", &one.tool.name), ("id", one.id.as_str())],
        ),
        many => strings.fmt(
            StrKey::AppLibraryDeletedMany,
            &[("count", &many.len().to_string())],
        ),
    };
    Some((text, false))
}

/// *Add to project*: the ticked rows copied into the project as one
/// command, the ticks cleared.
fn add(sheet: &mut Sheet, app: &mut App) -> Option<(String, bool)> {
    let Ok(library) = &sheet.library else {
        return None;
    };
    let command = tools::add_from_library(app.project(), library, &sheet.ticked)?;
    let count = match &command {
        Command::AddTools(tools) => tools.len(),
        _ => 0,
    };
    Some(match app.run(command) {
        Ok(_) => {
            sheet.ticked.clear();
            (
                app.strings()
                    .fmt(StrKey::AppLibraryAdded, &[("count", &count.to_string())]),
                false,
            )
        }
        Err(e) => (e.to_string(), true),
    })
}

/// *Save to library* from the Tools tab: tool `id` of the project into
/// the library file, as `dc_library::add` stores it. Updates the open
/// sheet's table. Returns the status bar's message.
pub(crate) fn save_to_library(
    app: &App,
    config: Option<&Paths>,
    id: &ToolId,
    sheet: Option<&mut Sheet>,
) -> Option<String> {
    let strings = app.strings();
    let tool = app.project().tools.iter().find(|t| t.id == *id)?;
    let failed = |reason: String| {
        strings.fmt(
            StrKey::AppToolsSaveFailed,
            &[("name", &tool.name), ("reason", &reason)],
        )
    };
    let Some(paths) = config else {
        return Some(failed(
            strings.get(StrKey::AppLibraryUnavailable).to_owned(),
        ));
    };
    let library = match read(paths) {
        Ok(library) => library,
        Err(reason) => return Some(failed(reason)),
    };
    let (next, outcome) = match tools::save_to_library(&library, tool) {
        Ok(saved) => saved,
        Err(e) => return Some(failed(e.to_string())),
    };
    let message = match &outcome {
        dc_library::AddOutcome::Added { id } => {
            if let Err(e) = dreamcarve_shell::library::write(paths, &next) {
                return Some(failed(e.to_string()));
            }
            let stored = dc_library::find(&next, id.as_str())
                .map_or_else(|_| tool.name.clone(), |t| t.tool.name.clone());
            strings.fmt(
                StrKey::AppToolsSaved,
                &[("name", &stored), ("id", id.as_str())],
            )
        }
        dc_library::AddOutcome::AlreadyPresent { id } | dc_library::AddOutcome::Replaced { id } => {
            strings.fmt(
                StrKey::AppToolsAlreadySaved,
                &[("name", &tool.name), ("id", id.as_str())],
            )
        }
    };
    if let Some(sheet) = sheet {
        sheet.set_library(next);
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the sheet's default width (960 px less its margins, the editing
    /// pane and the gap) the fixed columns leave the name at least 120
    /// px, and the columns end inside the row.
    #[test]
    fn columns_fit_the_default_sheet() {
        let left = SIZE.x - 48.0 - PANE_PX - GAP_PX;
        let row = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(left, ROW_PX));
        let rects = column_rects(row);
        assert_eq!(rects.len(), COLUMNS.len());
        assert!(rects[0].width() >= 120.0, "{:?}", rects[0]);
        let end = rects.last().map_or(0.0, |r| r.right());
        assert!(end <= row.right() + 0.01, "{end} past {}", row.right());
    }

    #[test]
    fn a_missing_config_is_the_unavailable_message() {
        let sheet = Sheet::open(None, &Strings::english());
        assert_eq!(
            sheet.library,
            Err(Strings::english()
                .get(StrKey::AppLibraryUnavailable)
                .to_owned())
        );
    }
}
