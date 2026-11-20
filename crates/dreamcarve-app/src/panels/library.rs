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
//! *Delete*; rows already in the project are marked. The table's columns
//! and cells are the model's (`Column::ALL`, `row`; #685): the Ø column
//! is the cutting diameter for every type, a tapered ball nose's tip has
//! the column of its own, the Angle column says which angle it holds
//! (`90°`, `6.7°/side`), and a value a tool still needs is a red dash in
//! its cell, never a value. The right edits the
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
//! import review in place of the panes (slice 31, #623; capability 64b;
//! never a sheet above a sheet): the counts `tools import` reports, *Keep
//! mine* / *Replace with imported* (`--replace`) only when a tool is
//! *Changed*, and the file's tools with their status
//! (`model::review_status`; a skipped tool's says why, and a tapered ball
//! nose that comes in with values unset names them, *Still to set: Taper
//! per side, Flute length*). Nothing is written
//! until *Import*, which merges into the library as it is on disk then,
//! writes it through the staged write and returns to the table with a
//! banner whose *Details* reopens the review read-only. *Back* (or
//! Escape) leaves the table as it was.
//!
//! The model is `dc_app::library`; the sheet keeps the search, the ticks,
//! the selection and the session between frames. A library change never
//! alters the open project (ADR-0002): *Add to project* copies, under new
//! ids, as `tools add` does, and is one undo step; nothing else here is
//! in the project's history.

use std::collections::BTreeSet;

use dc_app::library::{self as model, Cell, Column, Filter, KindFilter, LibraryEdit};
use dc_app::prefs::SheetSize;
use dc_app::tools::{self, KindChoice, ToolField};
use dc_app::{App, Command, StrKey, Strings};
use dc_library::import::{Incoming, Outcome, Review, ReviewItem, Status};
use dc_library::{Library, LibraryToolId};
use dc_project::{Tool, ToolId, ToolKind, Units};
use dreamcarve_shell::Paths;
use eframe::egui::{self, Align, Id, Layout, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::panels::tools::{FieldEdit, ToolFields, tool_fields};
use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Dialogs};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring_round, text};
use crate::widgets::checkbox::paint_box;
use crate::widgets::help::{self, Scope};
use crate::widgets::icon::Icon;
use crate::widgets::scroll;
use crate::widgets::segmented::segmented;

/// The sheet's size among the preferences' `sizes` (capability 64): the
/// user drags it by its edges or its corner grip. Its default, px
/// (`experience.md`, section 3), is wide enough for the table's ten
/// columns, a type's name whole among them, to leave the name 120 px
/// beside the editing pane; the smallest it is dragged to holds the
/// columns at their minimums beside the pane, and six rows.
const SHEET: SheetSize = SheetSize {
    key: "library",
    default_px: (1100.0, 640.0),
    min_px: (920.0, 480.0),
};

/// `value` px as egui's `f32`. Sizes are a few thousand pixels at most,
/// far inside `f32`'s exact range.
#[allow(clippy::cast_possible_truncation, reason = "pixel sizes, exact in f32")]
fn px(value: f64) -> f32 {
    value as f32
}

/// The sheet's size in a window of `screen`, px: the one the user dragged
/// it to, else the default, held between its minimum and the window.
fn sheet_size(app: &App, screen: egui::Vec2) -> egui::Vec2 {
    let (width, height) = SHEET.of(app.prefs(), (f64::from(screen.x), f64::from(screen.y)));
    egui::vec2(px(width), px(height))
}
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The editing pane's width, px (section 3).
const PANE_PX: f32 = 320.0;
/// The gap between the panes, px.
const GAP_PX: f32 = 16.0;
/// A header, a table row, a vendor group's band and a sheet footer
/// button, px.
const HEADER_PX: f32 = 32.0;
const ROW_PX: f32 = theme::TABLE_ROW_PX;
const GROUP_PX: f32 = theme::SECTION_HEADER_PX;
const FOOTER_PX: f32 = theme::CONTROL_PX;
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
    /// The import being reviewed, or the last one made (its *Details*).
    review: Option<ImportReview>,
    /// Whether the review shows in place of the panes.
    reviewing: bool,
    /// The banner after an import: *Imported 13 tools from sample.tools.*
    banner: Option<String>,
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

/// An import's review (capability 64b): the file, its tools, both
/// imports and the user's choice between them.
#[derive(Debug, Clone, PartialEq)]
struct ImportReview {
    /// The file's name, as the title shows it.
    name: String,
    /// The file's tools, merged again into the library on disk at
    /// *Import*.
    incoming: Incoming,
    review: Review,
    /// *Replace with imported*; *Keep mine*, the default, is `false`.
    replace: bool,
    /// Imported: the review is read-only, what *Details* shows.
    done: bool,
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    /// *Import…*: choose a file and review it.
    Import,
    /// The review's *Import*: write the chosen import.
    ImportReviewed,
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
    /// The sheet dragged to this size, px, in a window of the second.
    Resize(egui::Vec2, egui::Vec2),
    /// A table column dragged to this width, px, in a table whose
    /// columns share the room given.
    ColumnWidth(Column, f32, f32),
}

impl Sheet {
    /// Whether the last action did what it was asked: it left a message
    /// that is not an error.
    pub(crate) fn succeeded(&self) -> bool {
        self.message.as_ref().is_some_and(|(_, alert)| !alert)
    }

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
            review: None,
            reviewing: false,
            banner: None,
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

    /// *Back* from the review (or Escape): the table as it was. A review
    /// not imported is dropped; a finished one stays for *Details*.
    fn leave_review(&mut self) {
        self.reviewing = false;
        if self.review.as_ref().is_some_and(|r| !r.done) {
            self.review = None;
        }
    }
}

/// The library file at `paths`, a missing one empty.
fn read(paths: &Paths) -> Result<Library, String> {
    dreamcarve_shell::library::read(paths)
        .map(Option::unwrap_or_default)
        .map_err(|e| e.to_string())
}

/// A cutter type's label.
pub(crate) fn kind_label(strings: &Strings, choice: KindChoice) -> &str {
    strings.get(choice.label())
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
    let size = sheet_size(app, screen);
    let frame = theme::sheet(palette);
    let response = egui::Modal::new(Id::unique("tool_library"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            let margin = egui::vec2(1.0, 1.0) * f32::from(MARGIN_PX);
            let inner = size - 2.0 * margin;
            // The sheet's frame, round its content: its edges and its
            // corner grip resize it (capability 64).
            let outer = Rect::from_min_size(ui.cursor().min - margin, size);
            ui.set_min_size(inner);
            ui.set_max_size(inner);
            // The help of the field being typed in, at the sheet's foot.
            let foot_px = help::foot_height(ui, Scope::Sheet, size.x);
            body(ui, inner.y - foot_px, app, project_name, sheet, out);
            help::settle(ui, Scope::Sheet);
            if foot_px > 0.0 {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(inner.x, foot_px), Sense::hover());
                help::foot(ui, Scope::Sheet, rect.expand2(egui::vec2(margin.x, 0.0)));
            }
            let grip = app.strings().get(StrKey::AppLibraryResize);
            if let Some(asked) = crate::widgets::sheet::resize_handles(
                ui,
                Id::unique("tool_library.resize"),
                outer,
                grip,
            ) {
                out.push(Action::Library(Request::Resize(asked, screen)));
            }
        });
    if response.should_close() {
        if sheet.confirm_remove {
            // Escape, or a click on the scrim, answers the question first.
            sheet.confirm_remove = false;
        } else if sheet.reviewing {
            // And leaves the review, as *Back* does.
            sheet.leave_review();
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
    if !sheet.reviewing && !sheet.confirm_remove && sheet.banner.is_some() {
        banner(ui, strings, sheet);
        ui.add_space(8.0);
    }
    let footer_h = FOOTER_PX + 24.0;
    // The panes take what the header and footer leave; an `Area`'s
    // available height is the screen's, so it is worked out.
    let used = ui.cursor().top() - top;
    let panes_h = (height - used - footer_h).max(ROW_PX * 4.0);
    let units = library_units(app);
    ui.allocate_ui(egui::vec2(ui.available_width(), panes_h), |ui| {
        ui.set_min_height(panes_h);
        ui.set_max_height(panes_h);
        if sheet.reviewing
            && let Some(review) = &mut sheet.review
        {
            review_view(ui, strings, units, review);
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
    let in_place = sheet.reviewing || sheet.confirm_remove;
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

/// The library's `mm` / `in` (capability 64; section 7, "Tool Library
/// header"): a preference, never a library or project change.
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
                    table(ui, app, units, &groups, sheet, out);
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

/// *New tool*, the same command as the Tools step's (capability 63).
fn new_tool_button(ui: &mut Ui, strings: &Strings, out: &mut Vec<Action>) {
    let new = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryNewTool))
        .icon(Icon::Plus, theme::ICON_SMALL_PX);
    if button(ui, new).clicked() {
        out.push(Action::Library(Request::NewTool));
    }
}

/// The search field and the type filter, then *New tool*, *Duplicate*
/// and *Delete* over the list. *Replace* is the import review's choice
/// (64b), not a switch here.
fn toolbar(ui: &mut Ui, strings: &Strings, sheet: &mut Sheet, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let search_w = (ui.available_width() - 160.0).clamp(160.0, 320.0);
        search_field(ui, strings, &mut sheet.filter.search, search_w);
        ui.add_space(8.0);
        let filter = strings.get(StrKey::AppToolsKind);
        help::offer(ui, filter, strings.get(StrKey::AppLibraryFilterTip));
        let combo = egui::ComboBox::from_id_salt("library_kind")
            .icon(crate::widgets::icon::combo_chevron)
            .width(ui.available_width().min(170.0))
            .height(crate::panels::tools::KIND_MENU_PX)
            .selected_text(strings.get(sheet.filter.kind.label()))
            .show_ui(ui, |ui| {
                for kind in KindFilter::ALL {
                    ui.selectable_value(&mut sheet.filter.kind, kind, strings.get(kind.label()));
                }
            })
            .response
            .accessible_name(filter);
        let _ = help::control(ui, combo, filter);
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
    });
}

/// The search field (section 7, "Search field"): a leading `search`
/// glyph, a trailing `x` that clears it while it has text. It filters as
/// typed.
fn search_field(ui: &mut Ui, strings: &Strings, search: &mut String, width: f32) {
    let palette = Palette::current(ui);
    let (rect, hover) =
        ui.allocate_exact_size(egui::vec2(width, theme::CONTROL_PX), Sense::hover());
    // The field's surface, under its glyph and text; outlined below.
    ui.painter()
        .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_sunken);
    let glyph = Icon::Search.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, rect.center().y - glyph.size().y / 2.0),
        glyph,
        palette.text_secondary,
    );
    let clear_w = if search.is_empty() {
        0.0
    } else {
        theme::CONTROL_PX
    };
    let edit_rect = Rect::from_min_max(
        rect.min + egui::vec2(10.0 + theme::ICON_SMALL_PX + 8.0, 0.0),
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
    // Named for assistive tools by what it searches, as its hint says.
    let name = strings.get(StrKey::AppLibrarySearch);
    help::offer(ui, name, strings.get(StrKey::AppLibrarySearchTip));
    let response = help::control(ui, response.accessible_name(name), name);
    // The focus ring goes round the field's frame, not its text.
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    let stroke = if response.has_focus() {
        palette.focus_ring
    } else if hover.hovered() || response.hovered() {
        palette.border_strong
    } else {
        palette.border_default
    };
    ui.painter().rect_stroke(
        rect,
        theme::CONTROL_RADIUS_PX,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    if !search.is_empty() {
        let clear_rect = Rect::from_min_size(
            egui::pos2(rect.right() - theme::CONTROL_PX, rect.top()),
            egui::vec2(theme::CONTROL_PX, theme::CONTROL_PX),
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

/// The "in project" mark's width at the product cell's right end, px.
const MARK_PX: f32 = 18.0;
/// What a cell shows for a value its tool still needs (mock-up 04): a
/// dash, in the error colour.
const UNSET_MARK: &str = "—";
/// How wide the strip that takes a drag on a column's divider is, px.
const DIVIDER_PX: f32 = 9.0;

/// The widths the user dragged the columns to, by column
/// (`Prefs::sizes`).
type Sizes = std::collections::BTreeMap<String, f64>;

/// Each column's rect across `row` after the tick, in `Column::ALL`'s
/// order, at the widths kept in `sizes` (`model::column_widths`): each
/// column its own, the name what is left.
fn column_rects(row: Rect, sizes: &Sizes) -> Vec<Rect> {
    let mut x = row.left() + TICK_PX;
    model::column_widths(sizes, f64::from(row.width() - TICK_PX))
        .into_iter()
        .map(|width| {
            let width = px(width);
            let rect =
                Rect::from_min_size(egui::pos2(x, row.top()), egui::vec2(width, row.height()));
            x += width;
            rect
        })
        .collect()
}

/// The dividers of the header row `head`, whose columns are `rects`: one
/// at the left edge of every column but the name, a hairline that takes
/// the accent under the pointer. The columns stand against the table's
/// right edge and the name takes the slack, so a divider dragged sets
/// the width of the column at its right, and the columns at its left
/// follow it. A drag asks for the width through `out`.
fn column_dividers(ui: &Ui, strings: &Strings, head: Rect, rects: &[Rect], out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let room = head.width() - TICK_PX;
    for (column, rect) in Column::ALL.into_iter().zip(rects).skip(1) {
        let x = rect.left();
        let strip = Rect::from_center_size(
            egui::pos2(x, head.center().y),
            egui::vec2(DIVIDER_PX, head.height()),
        );
        let id = Id::unique(("library_divider", column.size_key()));
        let response = ui.interact(strip, id, Sense::drag());
        let name = strings.fmt(
            StrKey::AppLibraryColumnResize,
            &[("column", strings.get(column.heading()))],
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, &name));
        let live = response.hovered() || response.dragged();
        if live {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        // The width when the drag began and where the pointer was: the
        // divider then follows the pointer, however far past a limit it
        // went and came back.
        let start_id = id.with("start");
        if response.drag_started()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            ui.data_mut(|d| d.insert_temp(start_id, (rect.width(), pointer.x)));
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some((width, from)) = ui.data(|d| d.get_temp::<(f32, f32)>(start_id))
        {
            let asked = width - (pointer.x - from);
            out.push(Action::Library(Request::ColumnWidth(column, asked, room)));
        }
        let (stroke, inset) = if live {
            (Stroke::new(2.0, palette.accent), 4.0)
        } else {
            (Stroke::new(1.0, palette.border_default), 8.0)
        };
        ui.painter()
            .vline(x, (head.top() + inset)..=(head.bottom() - inset), stroke);
    }
}

/// Where a scrolled table's rows' width is kept between frames.
fn rows_width_id(table: &str) -> Id {
    Id::unique(("library_rows_px", table))
}

/// A table's header row `head` cut to the width its rows had in the last
/// frame: the rows scroll, and give the scroll bar its room when there is
/// one, so the headings are laid across the same width and each stands
/// over its cells.
fn header_over_rows(ui: &Ui, table: &str, head: Rect) -> Rect {
    let width = ui
        .data(|d| d.get_temp::<f32>(rows_width_id(table)))
        .map_or(head.width(), |rows| rows.min(head.width()));
    Rect::from_min_size(head.min, egui::vec2(width, head.height()))
}

/// Keeps the width `rows` of table `table`'s rows for the next frame's
/// header ([`header_over_rows`]).
fn remember_rows_width(ui: &Ui, table: &str, rows: f32) {
    ui.data_mut(|d| d.insert_temp(rows_width_id(table), rows));
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
    out: &mut Vec<Action>,
) {
    let strings = app.strings();
    let sizes = &app.prefs().sizes;
    let palette = Palette::current(ui);
    let (head, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let over_rows = header_over_rows(ui, "library", head);
    let head_rects = column_rects(over_rows, sizes);
    for (column, rect) in Column::ALL.into_iter().zip(head_rects.iter().copied()) {
        let heading = strings.get(column.heading());
        cell(
            ui,
            rect,
            heading,
            column.numeric(),
            palette.text_secondary,
            &TextStyle::Small,
        );
        // Named for assistive tools, which the painted text is not.
        ui.interact(
            rect,
            Id::unique(("library_column", heading)),
            Sense::hover(),
        )
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColumnHeader, true, heading));
    }
    // Over the headings, so a divider takes the drag.
    column_dividers(ui, strings, over_rows, &head_rects, out);
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
    let scrolled = egui::ScrollArea::vertical()
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
                    table_row(ui, strings, units, sizes, n, state, sheet);
                }
            }
            ui.available_width()
        });
    // The table stops above the sheet's footer, wherever its rows are:
    // the row its edge cuts fades out (row D5 of the worklog).
    scroll::fade(ui, &scrolled, palette.surface_raised);
    remember_rows_width(ui, "library", scrolled.inner);
}

/// A vendor group's 32-px header (section 7, "Vendor group header"): the
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
        theme::NONE
    };
    paint_group_band(ui, rect, fill, vendor, group.rows.len(), TICK_PX);
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
    response.clicked()
}

/// A vendor group's band (section 7, "Vendor group header"): `fill`, then
/// the vendor in 13-px SemiBold and ` · count` in text.secondary, from
/// `indent` px.
fn paint_group_band(
    ui: &Ui,
    rect: Rect,
    fill: egui::Color32,
    vendor: &str,
    count: usize,
    indent: f32,
) {
    let palette = Palette::current(ui);
    ui.painter()
        .rect_filled(rect, theme::CONTROL_RADIUS_PX, fill);
    let name = ui.painter().layout_no_wrap(
        vendor.to_owned(),
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    let name_w = name.size().x;
    let x = rect.left() + indent;
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
}

/// What a row's cells say, for assistive tools (the cells are painted):
/// each column after the name with its heading, `Type Tapered ball nose,
/// Ø 6.35, Tip 0.762, Angle 6.7°/side, …`; an empty cell is left out, and
/// one whose value the tool still needs says so, `Feed not set`.
fn row_summary(strings: &Strings, cells: &[Cell]) -> String {
    let parts: Vec<String> = Column::ALL
        .into_iter()
        .zip(cells)
        .skip(1)
        .filter_map(|(column, cell)| {
            let heading = strings.get(column.heading());
            match cell {
                Cell::Text(value) => Some(strings.fmt(
                    StrKey::AppLibraryRowCell,
                    &[("column", heading), ("value", value)],
                )),
                Cell::Unset => {
                    Some(strings.fmt(StrKey::AppLibraryRowCellUnset, &[("column", heading)]))
                }
                Cell::Empty => None,
            }
        })
        .collect();
    parts.join(strings.get(StrKey::AppImportListSeparator))
}

/// One row: its tick (for *Add to project* and *Delete*) and the row
/// itself, which selects the tool for the editing pane. Its cells are the
/// model's (`model::row`).
fn table_row(
    ui: &mut Ui,
    strings: &Strings,
    units: Units,
    sizes: &Sizes,
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
    let cells = model::row(strings, tool, units);
    row.widget_info(|| {
        let mut info = egui::WidgetInfo::selected(egui::Role::ListItem, true, chosen, &tool.name);
        info.current_text_value = Some(row_summary(strings, &cells));
        info
    });
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
    if n % 2 == 1 && !chosen {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_stripe);
    }
    crate::widgets::row::surface(ui, rect, chosen, row.hovered() || tick.hovered());

    // The 16-px tick box.
    let tick_box = Rect::from_center_size(tick_rect.center(), egui::vec2(16.0, 16.0));
    paint_box(ui, tick_box, on, tick.hovered(), true);
    focus_ring_round(ui, &tick, tick_box, theme::MARK_RADIUS_PX);
    focus_ring_round(ui, &row, rect, theme::CONTROL_RADIUS_PX);

    let colour = if state.in_project {
        palette.text_secondary
    } else {
        palette.text_primary
    };
    let unset = palette.status_error.text;
    let mut rects = column_rects(rect, sizes);
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
    for ((value, column), r) in cells.iter().zip(Column::ALL).zip(&rects) {
        // A value the tool still needs is a red dash in its cell (mock-up
        // 04), never a value; a cell its type has no value for is empty.
        let (text, text_colour) = match value {
            Cell::Text(text) => (text.as_str(), colour),
            Cell::Unset => (UNSET_MARK, unset),
            Cell::Empty => continue,
        };
        cell(
            ui,
            *r,
            text,
            column.numeric(),
            text_colour,
            &TextStyle::Body,
        );
    }
    if row.hovered() {
        // The whole name and type (narrow columns cut them), then what it
        // lacks.
        let mut lines = vec![
            tool.name.clone(),
            kind_label(strings, KindChoice::of(&tool.cutter)).to_owned(),
        ];
        lines.extend(tools::unset_values(tool).into_iter().map(|(_, m)| m));
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
    let scrolled = egui::ScrollArea::vertical()
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
                    focus: None,
                    marked,
                    findings: &[],
                },
            )
        });
    scroll::fade(ui, &scrolled, palette.surface_raised);
    let output = scrolled.inner;
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
            if *next == *tool {
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
                    ..*next
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

/// The banner after an import (64b): `circle-check`, *Imported 13 tools
/// from sample.tools. 10 skipped.*, and *Details*, which reopens the
/// review read-only.
fn banner(ui: &mut Ui, strings: &Strings, sheet: &mut Sheet) {
    let Some(message) = sheet.banner.clone() else {
        return;
    };
    let palette = Palette::current(ui);
    egui::Frame::new()
        .fill(palette.surface_stripe)
        .corner_radius(theme::INNER_CARD_RADIUS_PX)
        .inner_margin(egui::Margin::symmetric(12, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(FOOTER_PX);
                let glyph =
                    Icon::CircleCheck.galley(ui, theme::ICON_SMALL_PX, palette.status_success.text);
                let (rect, _) = ui.allocate_exact_size(glyph.size(), Sense::hover());
                ui.painter()
                    .galley(rect.min, glyph, palette.status_success.text);
                ui.add_space(4.0);
                text(ui, &message, TextStyle::Body, palette.text_primary);
                let details = sheet.review.as_ref().is_some_and(|r| r.done);
                if details {
                    let spec = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppLibraryDetails))
                        .tooltip(strings.get(StrKey::AppLibraryDetailsTooltip));
                    if button(ui, spec).clicked() {
                        sheet.reviewing = true;
                    }
                }
            });
        });
}

/// The review's columns before its status, each with its width, px (0
/// is the rest): the library table's (`model::cell`), less those a look
/// at an import does not need. The type shows whole, and a tapered ball
/// nose its tip and its taper per side (#685).
const REVIEW_COLUMNS: [(Column, f32); 6] = [
    (Column::Name, 0.0),
    (Column::Kind, 124.0),
    (Column::Diameter, 56.0),
    (Column::Tip, 52.0),
    (Column::Angle, 76.0),
    (Column::Feed, 56.0),
];
/// The review's status column, px: room for what a tapered ball nose
/// still needs after its status.
const REVIEW_STATUS_PX: f32 = 460.0;

/// Each review column's rect across `row`: [`REVIEW_COLUMNS`]', then the
/// status'.
fn review_rects(row: Rect) -> Vec<Rect> {
    let widths = || {
        REVIEW_COLUMNS
            .iter()
            .map(|(_, w)| *w)
            .chain([REVIEW_STATUS_PX])
    };
    let fixed: f32 = widths().sum();
    let rest = (row.width() - 8.0 - fixed).max(120.0);
    let mut x = row.left() + 8.0;
    widths()
        .map(|w| {
            let width = if w > 0.0 { w } else { rest };
            let rect =
                Rect::from_min_size(egui::pos2(x, row.top()), egui::vec2(width, row.height()));
            x += width;
            rect
        })
        .collect()
}

/// What an item's Status cell shows under the choice `replace`
/// (capability 64b): its status as the model words it
/// (`model::review_status`: a skipped item's says why), and, for a
/// tapered ball nose that comes in with values unset, what is still to
/// set (`model::review_unset`), *New · Still to set: Taper per side,
/// Flute length*.
fn status_text(strings: &Strings, item: &ReviewItem, replace: bool) -> String {
    let status = model::review_status(strings, item, replace);
    match model::review_unset(strings, item) {
        Some(unset) => strings.fmt(
            StrKey::AppLibraryStatusUnset,
            &[("status", &status), ("unset", &unset)],
        ),
        None => status,
    }
}

/// The import review in place of the panes (capability 64b; mock-up 04,
/// below): the file's name, the counts, the choice when a tool changed,
/// then the file's items grouped by vendor with their status.
fn review_view(ui: &mut Ui, strings: &Strings, units: Units, pending: &mut ImportReview) {
    let palette = Palette::current(ui);
    text(
        ui,
        &strings.fmt(StrKey::AppLibraryReviewTitle, &[("name", &pending.name)]),
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(12.0);
    let review = &pending.review;
    let mut counts = vec![
        (StrKey::AppLibraryReviewNew, review.count(Status::New)),
        (
            StrKey::AppLibraryReviewPresent,
            review.count(Status::Present),
        ),
        (
            StrKey::AppLibraryReviewChanged,
            review.count(Status::Changed),
        ),
    ];
    // A DreamCarve library has nothing to skip, and `tools import`
    // prints no skipped count for it.
    if review.keep.report.skipped.is_some() {
        counts.push((
            StrKey::AppLibraryReviewSkipped,
            review.count(Status::Skipped),
        ));
    }
    // Each skipped tool with why, as `tools import` prints it: the
    // Skipped count's tooltip (a row's status may be cut short).
    let skipped: Vec<String> = model::skipped(review)
        .into_iter()
        .map(|(name, reason)| {
            strings.fmt(
                StrKey::AppLibraryReviewSkippedItem,
                &[("name", name), ("reason", reason)],
            )
        })
        .collect();
    ui.horizontal(|ui| {
        for (key, count) in counts {
            let tip = (key == StrKey::AppLibraryReviewSkipped && !skipped.is_empty())
                .then(|| skipped.join("\n"));
            count_tile(ui, strings, key, count, tip.as_deref());
            ui.add_space(8.0);
        }
    });
    ui.add_space(12.0);
    let changed = review.count(Status::Changed);
    let choosing = changed > 0 && !pending.done;
    if choosing {
        ui.horizontal(|ui| {
            let question = if changed == 1 {
                strings.get(StrKey::AppLibraryReviewChangedOne).to_owned()
            } else {
                strings.fmt(
                    StrKey::AppLibraryReviewChangedMany,
                    &[("count", &changed.to_string())],
                )
            };
            text(ui, &question, TextStyle::Body, palette.text_primary);
            ui.add_space(8.0);
            let labels = [
                strings.get(StrKey::AppLibraryKeepMine),
                strings.get(StrKey::AppLibraryReplaceImported),
            ];
            let picked = segmented(
                ui,
                Id::unique("library.review.choice"),
                &labels,
                usize::from(pending.replace),
                320.0,
            );
            if let Some(n) = picked {
                pending.replace = n == 1;
            }
        });
        ui.add_space(12.0);
    }
    let note_h = if choosing { 28.0 } else { 0.0 };
    let table_h = (ui.available_height() - note_h).max(ROW_PX * 3.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
        ui.set_min_height(table_h);
        ui.set_max_height(table_h);
        review_table(ui, strings, units, pending);
    });
    if choosing {
        let kept: Vec<&str> = pending
            .review
            .items
            .iter()
            .filter(|i| i.status == Status::Changed)
            .filter_map(|i| match &i.keep {
                Outcome::Imported { stored_name, .. } => Some(stored_name.as_str()),
                _ => None,
            })
            .collect();
        let note = match kept.as_slice() {
            [one] => strings.fmt(StrKey::AppLibraryChoiceNoteOne, &[("name", one)]),
            _ => strings.get(StrKey::AppLibraryChoiceNoteMany).to_owned(),
        };
        ui.add_space(8.0);
        text(ui, &note, TextStyle::Small, palette.text_secondary);
    }
}

/// One count of the review: the number large, its label under it, named
/// `13 New` for screen readers; `tip`, when there is one, on hover.
fn count_tile(ui: &mut Ui, strings: &Strings, key: StrKey, count: usize, tip: Option<&str>) {
    let palette = Palette::current(ui);
    let label = strings.get(key);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 56.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::INNER_CARD_RADIUS_PX, palette.surface_stripe);
    let number = ui.painter().layout_no_wrap(
        count.to_string(),
        TextStyle::Name(theme::DISPLAY.into()).resolve(ui.style()),
        palette.text_primary,
    );
    ui.painter().galley(
        rect.min + egui::vec2(12.0, 8.0),
        number,
        palette.text_primary,
    );
    let caption = ui.painter().layout_no_wrap(
        label.to_owned(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 8.0 - caption.size().y),
        caption,
        palette.text_secondary,
    );
    let name = strings.fmt(
        StrKey::AppLibraryReviewCount,
        &[("count", &count.to_string()), ("label", label)],
    );
    let response = ui.interact(
        rect,
        Id::unique(("library.review.count", label)),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    if let Some(tip) = tip {
        response.on_hover_text(tip);
    }
}

/// The review's table: a header row, then each vendor's band and its
/// items, striped within the group.
fn review_table(ui: &mut Ui, strings: &Strings, units: Units, pending: &ImportReview) {
    let palette = Palette::current(ui);
    let (head, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let headings = REVIEW_COLUMNS
        .iter()
        .map(|(column, _)| (column.heading(), column.numeric()))
        .chain([(StrKey::AppLibraryColumnStatus, false)]);
    let over_rows = header_over_rows(ui, "review", head);
    for ((key, right), rect) in headings.zip(review_rects(over_rows)) {
        cell(
            ui,
            rect,
            strings.get(key),
            right,
            palette.text_secondary,
            &TextStyle::Small,
        );
    }
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, palette.border_default),
    );
    let scrolled = egui::ScrollArea::vertical()
        .id_salt("library_review_table")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            for group in model::review_groups(&pending.review) {
                let vendor = group
                    .vendor
                    .unwrap_or_else(|| strings.get(StrKey::AppLibraryNoVendor));
                let (band, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), GROUP_PX),
                    Sense::hover(),
                );
                paint_group_band(
                    ui,
                    band,
                    // No band, as the library's own table has none.
                    theme::NONE,
                    vendor,
                    group.items.len(),
                    8.0,
                );
                for (n, (index, item)) in group.items.iter().enumerate() {
                    review_row(ui, strings, units, (n, *index), item, pending.replace);
                }
            }
            width
        });
    scroll::fade(ui, &scrolled, palette.surface_raised);
    remember_rows_width(ui, "review", scrolled.inner);
}

/// One item of the review, `index` in the file, row `n` of its group:
/// its cells under [`REVIEW_COLUMNS`] (a value its tool still needs a red
/// dash, as in the library's table) and its status. Its tooltip is its
/// whole status and what the file leaves unset.
fn review_row(
    ui: &mut Ui,
    strings: &Strings,
    units: Units,
    (n, index): (usize, usize),
    item: &ReviewItem,
    replace: bool,
) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let status = status_text(strings, item, replace);
    let label = strings.fmt(
        StrKey::AppLibraryReviewRow,
        &[("name", &item.name), ("status", &status)],
    );
    // The id is the item's place in the file: its identity there,
    // whatever group or row it is drawn in.
    let response = ui.interact(
        rect,
        Id::unique(("library.review.row", index)),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Row, true, &label));
    if n % 2 == 1 {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_stripe);
    }
    let colour = match item.status {
        Status::Present | Status::Skipped => palette.text_secondary,
        Status::New | Status::Changed => palette.text_primary,
    };
    let status_colour = match item.status {
        Status::New => palette.status_success.text,
        Status::Changed => palette.status_warning.text,
        Status::Present | Status::Skipped => palette.text_secondary,
    };
    let unset = palette.status_error.text;
    let rects = review_rects(rect);
    for ((column, _), r) in REVIEW_COLUMNS.iter().zip(&rects) {
        // A skipped item has no tool: its name, a dash for its type.
        let value = match (&item.tool, column) {
            (_, Column::Name) => Cell::Text(item.name.clone()),
            (Some(tool), _) => model::cell(strings, tool, *column, units),
            (None, Column::Kind) => Cell::Text(UNSET_MARK.to_owned()),
            (None, _) => Cell::Empty,
        };
        let (text, text_colour) = match &value {
            Cell::Text(text) => (text.as_str(), colour),
            Cell::Unset => (UNSET_MARK, unset),
            Cell::Empty => continue,
        };
        cell(
            ui,
            *r,
            text,
            column.numeric(),
            text_colour,
            &TextStyle::Body,
        );
    }
    if let Some(r) = rects.last() {
        cell(ui, *r, &status, false, status_colour, &TextStyle::Body);
    }
    if response.hovered() {
        let mut lines = vec![item.name.clone(), status];
        lines.extend(item.notes.iter().cloned());
        response.on_hover_text(lines.join("\n"));
    }
}

/// The review's primary action: *Import 5 tools*, its count following
/// the choice; *Nothing to import* (disabled) when it would change
/// nothing.
fn import_button(ui: &mut Ui, strings: &Strings, pending: &ImportReview, out: &mut Vec<Action>) {
    let writes = pending.review.writes(pending.replace);
    let label = match writes {
        0 => strings.get(StrKey::AppLibraryImportNothing).to_owned(),
        1 => strings.get(StrKey::AppLibraryImportOne).to_owned(),
        n => strings.fmt(StrKey::AppLibraryImportMany, &[("count", &n.to_string())]),
    };
    let mut spec = ButtonSpec::new(Kind::Primary, &label)
        .height(FOOTER_PX)
        .enabled(writes > 0);
    if writes == 0 {
        spec = spec.tooltip(strings.get(StrKey::AppLibraryImportNothingTooltip));
    }
    if button(ui, spec).clicked() {
        out.push(Action::Library(Request::ImportReviewed));
    }
}

/// The footer: the counts on the left and *Add to project* on the right;
/// *Back* on the left and *Import* on the right while the review shows
/// (*Back* alone when it is read-only); *Cancel* and *Delete* while
/// *Delete* asks.
fn footer(ui: &mut Ui, app: &App, project_name: &str, sheet: &mut Sheet, out: &mut Vec<Action>) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::left_to_right(Align::Center),
        |ui| {
            if sheet.reviewing {
                let back = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryBack))
                    .icon(Icon::ArrowLeft, theme::ICON_SMALL_PX)
                    .height(FOOTER_PX);
                if button(ui, back).clicked() {
                    sheet.leave_review();
                    return;
                }
                if let Some(pending) = sheet.review.as_ref().filter(|r| !r.done) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        import_button(ui, strings, pending, out);
                    });
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
        Request::Resize(size, screen) => {
            SHEET.set(
                app.prefs_mut(),
                (f64::from(size.x), f64::from(size.y)),
                (f64::from(screen.x), f64::from(screen.y)),
            );
            return None;
        }
        Request::ColumnWidth(column, width_px, room_px) => {
            model::set_column_width(
                &mut app.prefs_mut().sizes,
                column,
                f64::from(width_px),
                f64::from(room_px),
            );
            return None;
        }
        Request::Import => import(sheet, app.strings(), config, dialogs),
        Request::ImportReviewed => import_reviewed(sheet, app.strings(), config),
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

/// *Import…*: asks for the file, reads it and the library as it is on
/// disk now, and shows the review (capability 64b). Nothing is written.
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
        Err(reason) => return failed(reason),
    };
    let current = match read(paths) {
        Ok(library) => library,
        Err(reason) => return failed(reason),
    };
    let (incoming, review) = match model::review(&current, &bytes) {
        Ok(read) => read,
        Err(e) => return failed(e.to_string()),
    };
    sheet.set_library(current);
    sheet.review = Some(ImportReview {
        name,
        incoming,
        review,
        replace: false,
        done: false,
    });
    sheet.reviewing = true;
    sheet.banner = None;
    sheet.message = None;
    None
}

/// The review's *Import*: the file merged as chosen into the library as
/// it is on disk now (as `tools import` would read it then; the review is
/// made again should the file have changed since), written through the
/// staged write. Back on the table with the banner; the review stays for
/// *Details*. A failure keeps the review open, with its reason.
fn import_reviewed(
    sheet: &mut Sheet,
    strings: &Strings,
    config: Option<&Paths>,
) -> Option<(String, bool)> {
    let pending = sheet.review.clone().filter(|r| !r.done)?;
    let replace = pending.replace;
    let incoming = pending.incoming.clone();
    let made = change_library(sheet, strings, config, |current| {
        let review = dc_library::import::review(current, incoming).map_err(|e| e.to_string())?;
        Ok((review.merged(replace).library.clone(), review))
    });
    let review = match made {
        Ok(review) => review,
        Err(reason) => {
            return Some((
                strings.fmt(
                    StrKey::AppLibraryImportFailed,
                    &[("name", &pending.name), ("reason", &reason)],
                ),
                true,
            ));
        }
    };
    let writes = review.writes(replace);
    let mut banner = match writes {
        1 => strings.fmt(StrKey::AppLibraryImportedOne, &[("name", &pending.name)]),
        n => strings.fmt(
            StrKey::AppLibraryImportedMany,
            &[("count", &n.to_string()), ("name", &pending.name)],
        ),
    };
    if let Some(skipped) = review.merged(replace).report.skipped.filter(|n| *n > 0) {
        banner.push(' ');
        banner.push_str(&strings.fmt(
            StrKey::AppLibraryImportedSkipped,
            &[("count", &skipped.to_string())],
        ));
    }
    sheet.review = Some(ImportReview {
        review,
        done: true,
        ..pending
    });
    sheet.reviewing = false;
    sheet.banner = Some(banner);
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

/// *Save to library* from the tool's editor: tool `id` of the project into
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

    /// At the sheet's default width (1100 px less its margins, the
    /// editing pane and the gap) the table's ten columns (`Column::ALL`,
    /// the Tip column among them, #685) leave the name at least 120 px,
    /// and the columns end inside the row. The name alone takes the rest.
    #[test]
    fn columns_fit_the_default_sheet() {
        let left = px(SHEET.default_px.0) - 48.0 - PANE_PX - GAP_PX;
        let row = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(left, ROW_PX));
        let rects = column_rects(row, &Sizes::new());
        assert_eq!(rects.len(), Column::ALL.len());
        assert!(rects[0].width() >= 120.0, "{:?}", rects[0]);
        // And at the smallest the sheet is dragged to, with every column
        // at its default: the name keeps its minimum, the others give up
        // what they can.
        let least = px(SHEET.min_px.0) - 48.0 - PANE_PX - GAP_PX;
        let narrow = column_rects(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(least, ROW_PX)),
            &Sizes::new(),
        );
        assert!(narrow[0].width() >= 119.99, "{:?}", narrow[0]);
        assert!(
            narrow.last().map_or(0.0, |r| r.right()) <= least + 0.01,
            "the columns end inside the narrowest table"
        );
        let end = rects.last().map_or(0.0, |r| r.right());
        assert!(end <= row.right() + 0.01, "{end} past {}", row.right());
        let rest: Vec<Column> = Column::ALL
            .into_iter()
            .filter(|c| c.default_width_px() <= 0.0)
            .collect();
        assert_eq!(rest, [Column::Name]);
    }

    /// The review's columns and its status fit the sheet's width with at
    /// least 160 px left for the name.
    #[test]
    fn review_columns_fit_the_default_sheet() {
        let row = Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(px(SHEET.default_px.0) - 48.0, ROW_PX),
        );
        let rects = review_rects(row);
        assert_eq!(rects.len(), REVIEW_COLUMNS.len() + 1);
        assert!(rects[0].width() >= 160.0, "{:?}", rects[0]);
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
