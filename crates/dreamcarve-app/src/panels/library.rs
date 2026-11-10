// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Tool Library sheet (functional spec, capability 64 and acceptance
//! 71; mock-up `04-tool-library.svg`; `experience.md`, sections 7 and 8,
//! "Sheets"): a window over the library file in the config directory,
//! read and written through `dreamcarve_shell::library` as the CLI's
//! `tools` commands read and write it. A search field (name, vendor,
//! diameter), a type filter and a table whose rows are ticked for *Add to
//! project* or *Remove*; rows already in the project are marked. *Remove*
//! asks first, naming the ticked tools, in place of the table (#581).
//! *Import…* reads a Fusion `.tools` or `.json` or a DreamCarve library and
//! shows the per-tool report `tools import` prints in place of the table,
//! with *Back* (never a sheet above a sheet); *Replace existing* is
//! `--replace`. *Export…* writes the library as DreamCarve JSON.
//!
//! The model is `dc_app::library`; the sheet keeps the search, the type
//! filter and the ticks between frames. A library change never alters the
//! open project (ADR-0002): *Add to project* copies, under new ids, as
//! `tools add` does, and is one undo step. Library tools are not edited
//! in place here (phase 5).

use std::collections::BTreeSet;

use dc_app::library::{self as model, Filter, KindFilter};
use dc_app::tools::{self, KindChoice};
use dc_app::{App, Command, StrKey, Strings};
use dc_library::import::{ImportReport, Outcome};
use dc_library::{Library, LibraryToolId};
use dc_project::{ToolId, ToolKind, Units};
use dreamcarve_shell::Paths;
use eframe::egui::{self, Align, Layout, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Dialogs};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::icon::Icon;

/// The sheet's default size, px (`experience.md`, section 3).
const SIZE: egui::Vec2 = egui::vec2(880.0, 600.0);
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// A header, a table row and a sheet footer button, px.
const HEADER_PX: f32 = 32.0;
const ROW_PX: f32 = 28.0;
const FOOTER_PX: f32 = 28.0;
/// The tick column, px.
const TICK_PX: f32 = 32.0;

/// The sheet's state between frames.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sheet {
    /// The library as last read or written; the message when it cannot be
    /// read, and then nothing writes it.
    library: Result<Library, String>,
    /// The library file, as the header shows it.
    path: String,
    filter: Filter,
    ticked: BTreeSet<LibraryToolId>,
    replace: bool,
    /// The last import's file name and report, shown in place of the table.
    report: Option<(String, ImportReport)>,
    /// Whether *Remove* is asking first, in place of the table (#581).
    confirm_remove: bool,
    /// The last action's message, and whether it is an error.
    message: Option<(String, bool)>,
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    Import,
    Export,
    Remove,
    Add,
    Close,
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
        Self {
            library,
            path,
            filter: Filter::default(),
            ticked: BTreeSet::new(),
            replace: false,
            report: None,
            confirm_remove: false,
            message: None,
        }
    }

    /// The names of the ticked tools, in library order: what *Remove*'s
    /// question names.
    fn ticked_names(&self) -> Vec<&str> {
        self.library.as_ref().map_or_else(
            |_| Vec::new(),
            |library| {
                library
                    .tools
                    .iter()
                    .filter(|t| self.ticked.contains(&t.id))
                    .map(|t| t.tool.name.as_str())
                    .collect()
            },
        )
    }

    /// Takes `library` as the one on disk, dropping ticks on rows it no
    /// longer has.
    pub(crate) fn set_library(&mut self, library: Library) {
        self.ticked
            .retain(|id| library.tools.iter().any(|t| t.id == *id));
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
    let response = egui::Modal::new(egui::Id::unique("tool_library"))
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
    header(ui, strings, &sheet.path, out);
    ui.add_space(12.0);
    let readable = sheet.library.is_ok();
    let in_report = sheet.report.is_some();
    if !in_report && !sheet.confirm_remove {
        toolbar(ui, strings, sheet, readable, out);
        ui.add_space(8.0);
    }
    let footer_h = FOOTER_PX + 24.0;
    // The table takes what the header, toolbar and footer leave; an
    // `Area`'s available height is the screen's, so it is worked out.
    let used = ui.cursor().top() - top;
    let table_h = (height - used - footer_h).max(ROW_PX * 2.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), table_h), |ui| {
        ui.set_min_height(table_h);
        ui.set_max_height(table_h);
        if let Some((name, report)) = &sheet.report {
            report_view(ui, strings, name, report, &sheet.path);
        } else if sheet.confirm_remove {
            remove_question(ui, strings, &sheet.ticked_names());
        } else {
            match &sheet.library {
                Ok(library) => table(ui, app, library, &sheet.filter, &mut sheet.ticked),
                Err(message) => sentence(ui, message, palette.status_error.text),
            }
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

/// The 32-px header: the title with the library's path, and `x`.
fn header(ui: &mut Ui, strings: &Strings, path: &str, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
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
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                text(
                    ui,
                    &strings.fmt(StrKey::AppLibraryTitle, &[("path", path)]),
                    TextStyle::Name(theme::TITLE.into()),
                    palette.text_primary,
                );
            });
        },
    );
}

/// The search field, the type filter, *Replace existing*, and the
/// library's actions.
fn toolbar(
    ui: &mut Ui,
    strings: &Strings,
    sheet: &mut Sheet,
    readable: bool,
    out: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        search_field(ui, strings, &mut sheet.filter.search);
        ui.add_space(8.0);
        let palette = Palette::current(ui);
        let label = text(
            ui,
            strings.get(StrKey::AppToolsKind),
            TextStyle::Body,
            palette.text_secondary,
        );
        egui::ComboBox::from_id_salt("library_kind")
            .icon(crate::widgets::icon::combo_chevron)
            .width(150.0)
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
            .labelled_by(label.id);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let remove = ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppLibraryRemove))
                .enabled(readable && !sheet.ticked.is_empty());
            if button(ui, remove).clicked() {
                // The library is outside project history: ask first.
                sheet.confirm_remove = true;
            }
            let export = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryExport))
                .enabled(readable);
            if button(ui, export).clicked() {
                out.push(Action::Library(Request::Export));
            }
            let import = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppLibraryImport))
                .enabled(readable);
            if button(ui, import).clicked() {
                out.push(Action::Library(Request::Import));
            }
        });
    });
    ui.add_space(4.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 26.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            checkbox(
                ui,
                "library.replace",
                &mut sheet.replace,
                strings.get(StrKey::AppLibraryReplace),
            );
        },
    );
}

/// The search field (section 7, "Search field"): a leading `search`
/// glyph, a trailing `x` that clears it while it has text. It filters as
/// typed.
fn search_field(ui: &mut Ui, strings: &Strings, search: &mut String) {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(280.0, 26.0), Sense::hover());
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
            .id(egui::Id::unique("library_search"))
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
/// whether numbers right-align.
const COLUMNS: [(StrKey, f32, bool); 11] = [
    (StrKey::AppLibraryColumnName, 0.0, false),
    (StrKey::AppLibraryColumnKind, 72.0, false),
    (StrKey::AppLibraryColumnDiameter, 56.0, true),
    (StrKey::AppLibraryColumnAngle, 48.0, true),
    (StrKey::AppLibraryColumnFlutes, 52.0, true),
    (StrKey::AppLibraryColumnFeed, 56.0, true),
    (StrKey::AppLibraryColumnPlunge, 56.0, true),
    (StrKey::AppLibraryColumnRpm, 56.0, true),
    (StrKey::AppLibraryColumnVendor, 88.0, false),
    (StrKey::AppLibraryColumnProduct, 84.0, false),
    // The mark: "in project", or a rate the tool lacks; no header.
    (StrKey::AppLibraryInProject, 80.0, false),
];

/// Each column's left edge and width across `width` px after the tick.
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
    let inner = rect.shrink2(egui::vec2(6.0, 0.0));
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

/// The table: a header row, then the rows the filter passes, striped;
/// each row's tick is its whole width.
fn table(
    ui: &mut Ui,
    app: &App,
    library: &Library,
    filter: &Filter,
    ticked: &mut BTreeSet<LibraryToolId>,
) {
    let strings = app.strings();
    let palette = Palette::current(ui);
    let units = app.project().display_units;
    let states = model::annotate(library, &app.project().tools);
    let (head, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    for ((key, _, right), rect) in COLUMNS
        .iter()
        .zip(column_rects(head))
        .take(COLUMNS.len() - 1)
    {
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
    if library.tools.is_empty() {
        sentence(
            ui,
            strings.get(StrKey::AppLibraryEmpty),
            palette.text_secondary,
        );
        return;
    }
    let shown = model::filter_states(&states, filter, units);
    if shown.is_empty() {
        sentence(
            ui,
            strings.get(StrKey::AppLibraryNoMatch),
            palette.text_secondary,
        );
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (n, state) in shown.iter().enumerate() {
                table_row(ui, strings, units, n, state, ticked);
            }
        });
}

/// One row of the table.
fn table_row(
    ui: &mut Ui,
    strings: &Strings,
    units: Units,
    n: usize,
    state: &model::EntryState<'_>,
    ticked: &mut BTreeSet<LibraryToolId>,
) {
    let palette = Palette::current(ui);
    let entry = state.entry;
    let tool = &entry.tool;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let on = ticked.contains(&entry.id);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &tool.name));
    if response.clicked() {
        if on {
            ticked.remove(&entry.id);
        } else {
            ticked.insert(entry.id.clone());
        }
    }
    let on = ticked.contains(&entry.id);
    let fill = if on {
        palette.accent_soft
    } else if response.hovered() {
        palette.surface_hover
    } else if n % 2 == 1 {
        palette.surface_stripe
    } else {
        theme::NONE
    };
    ui.painter().rect_filled(rect, 0.0, fill);

    // The 16-px tick box.
    let tick = Rect::from_center_size(
        egui::pos2(rect.left() + TICK_PX / 2.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    if on {
        ui.painter().rect_filled(tick, 3.0, palette.accent_fill);
        let glyph = Icon::Check.galley(ui, 12.0, palette.text_on_accent);
        ui.painter().galley(
            tick.center() - glyph.size() / 2.0,
            glyph,
            palette.text_on_accent,
        );
    } else {
        ui.painter().rect_stroke(
            tick,
            3.0,
            Stroke::new(1.0, palette.border_strong),
            StrokeKind::Inside,
        );
    }

    let colour = if state.in_project {
        palette.text_secondary
    } else {
        palette.text_primary
    };
    let rate = |v: Option<f64>| v.map_or_else(String::new, |v| model::diameter_text(v, units));
    let angle = match tool.cutter {
        ToolKind::VBit { included_angle_deg } => format!("{included_angle_deg}°"),
        _ => String::new(),
    };
    let cells = [
        tool.name.clone(),
        kind_label(strings, KindChoice::of(&tool.cutter)).to_owned(),
        model::diameter_text(tool.diameter_mm, units),
        angle,
        tool.flutes.to_string(),
        rate(tool.feed_mm_min),
        rate(tool.plunge_mm_min),
        tool.rpm.map_or_else(String::new, |r| r.to_string()),
        tool.vendor.clone().unwrap_or_default(),
        tool.product_id.clone().unwrap_or_default(),
    ];
    let rects = column_rects(rect);
    for ((value, (_, _, right)), r) in cells.iter().zip(COLUMNS.iter()).zip(&rects) {
        cell(ui, *r, value, *right, colour, &TextStyle::Body);
    }
    if let Some(mark_rect) = rects.last() {
        mark(ui, strings, *mark_rect, state);
    }
    if response.hovered() {
        let unset = tools::unset_rates(tool);
        if !unset.is_empty() {
            let lines: Vec<String> = unset.into_iter().map(|(_, m)| m).collect();
            response.on_hover_text(lines.join("\n"));
        }
    }
}

/// The row's mark: "in project" with a `check` in text.secondary, else
/// the rate the tool lacks in status.error, else nothing.
fn mark(ui: &mut Ui, strings: &Strings, rect: Rect, state: &model::EntryState<'_>) {
    let palette = Palette::current(ui);
    let tool = &state.entry.tool;
    let (key, colour) = if state.in_project {
        (StrKey::AppLibraryInProject, palette.text_secondary)
    } else if tools::lacks_rate(tool) {
        (StrKey::AppLibraryRateUnset, palette.status_error.text)
    } else {
        return;
    };
    let pill = Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 8.0, 18.0));
    let stroke = if state.in_project {
        palette.border_strong
    } else {
        palette.status_error.stroke
    };
    ui.painter()
        .rect_stroke(pill, 9.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    ui.put(
        pill,
        egui::Label::new(
            egui::RichText::new(strings.get(key))
                .text_style(TextStyle::Small)
                .color(colour),
        )
        .selectable(false)
        .truncate(),
    );
}

/// One sentence in the table's area (section 8, "Empty states").
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

/// *Remove*'s question in place of the table (`experience.md`, section 8,
/// "Destructive actions"; #581), naming the ticked tools: the library is
/// outside project history, so a removal cannot be undone. The footer
/// holds the answers.
fn remove_question(ui: &mut Ui, strings: &Strings, names: &[&str]) {
    let palette = Palette::current(ui);
    let names_text = names.join(", ");
    let count = names.len().to_string();
    let question = match names {
        [_] => strings.fmt(StrKey::AppLibraryRemoveAskOne, &[("names", &names_text)]),
        _ => strings.fmt(
            StrKey::AppLibraryRemoveAskMany,
            &[("count", &count), ("names", &names_text)],
        ),
    };
    sentence(ui, &question, palette.text_primary);
}

/// The import report in place of the table: a line per tool as `tools
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
/// *Back* on the left while the report shows.
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
                // *Remove* (destructive) is the answer on the right, with
                // *Cancel* beside it.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let remove =
                        ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppLibraryRemove))
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
/// and the project command. Returns the status bar's message.
pub(crate) fn perform(
    sheet: &mut Sheet,
    request: Request,
    app: &mut App,
    config: Option<&Paths>,
    dialogs: &mut dyn Dialogs,
) -> Option<String> {
    let message = match request {
        Request::Close => return None,
        Request::Import => import(sheet, app.strings(), config, dialogs),
        Request::Export => export(sheet, app.strings(), dialogs),
        Request::Remove => remove(sheet, app.strings(), config),
        Request::Add => add(sheet, app),
    };
    sheet.message.clone_from(&message);
    message.map(|(text, _)| text)
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
        let reason = e.to_string();
        return Some((
            strings.fmt(StrKey::AppLibraryWriteFailed, &[("reason", &reason)]),
            true,
        ));
    }
    sheet.set_library(merged.library);
    sheet.report = Some((name, merged.report));
    None
}

/// *Export…*: the library as DreamCarve JSON wherever the user says.
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

/// *Remove*, answered: the ticked tools out of the library, written at once.
/// Projects keep their copies.
fn remove(sheet: &mut Sheet, strings: &Strings, config: Option<&Paths>) -> Option<(String, bool)> {
    let paths = config?;
    let Ok(library) = &sheet.library else {
        return None;
    };
    let mut next = library.clone();
    let mut removed = Vec::new();
    for id in &sheet.ticked {
        if let Ok(tool) = dc_library::remove(&mut next, id.as_str()) {
            removed.push(tool);
        }
    }
    if removed.is_empty() {
        return None;
    }
    if let Err(e) = dreamcarve_shell::library::write(paths, &next) {
        return Some((
            strings.fmt(StrKey::AppLibraryWriteFailed, &[("reason", &e.to_string())]),
            true,
        ));
    }
    sheet.set_library(next);
    let text = match removed.as_slice() {
        [one] => strings.fmt(
            StrKey::AppLibraryRemovedOne,
            &[("name", &one.tool.name), ("id", one.id.as_str())],
        ),
        many => strings.fmt(
            StrKey::AppLibraryRemovedMany,
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

    /// At the sheet's default width (880 px less its margins) the fixed
    /// columns leave the name at least 140 px, and the columns end inside
    /// the row.
    #[test]
    fn columns_fit_the_default_sheet() {
        let row = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.x - 48.0, ROW_PX));
        let rects = column_rects(row);
        assert_eq!(rects.len(), COLUMNS.len());
        assert!(rects[0].width() >= 140.0, "{:?}", rects[0]);
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
