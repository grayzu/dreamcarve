// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Import drawing sheet (functional spec, capabilities 51 and 55;
//! mock-up `02-import-dialog.svg`; `experience.md`, sections 7 and 8,
//! "Sheets"): a 560-px sheet over a scrim that names the file and the
//! shapes it holds (closed and open, the layers, the units read from the
//! file), and, for a DXF, the importer's options as `dreamcarve init`
//! takes them: the units (required when the file declares none: the
//! dropdown in the error state, the importer's message its glyph's
//! tooltip, and *Add* disabled), the join tolerance and a layer list to
//! tick, in which a layer of entities the
//! importer does not read is listed greyed. An SVG has no options. A
//! preview thumbnail draws the drawing alone, fitted to it through the
//! Viewport camera's one Y flip, with no stock outline, origin or
//! placement note (review 2026-11-10), and the importer's warnings are
//! listed as the Warnings panel will list them. The footer's *Add 14
//! shapes* (or *Create project*) counts what it adds.
//!
//! Under the importer's warnings the sheet lists the artwork checks that
//! fired (capability 55, acceptance 106): what `dreamcarve init` prints
//! for an SVG and what `dreamcarve check` reads from a DXF's shapes, each
//! with its tier's glyph, its code (a link to its entry in the Warnings
//! chapter, as a drawer row's is) and its message whole, and one quiet
//! line that says to run `dreamcarve image-prep`, with a link to the
//! *Preparing artwork* chapter, since the app does not clean a drawing.
//! The list is the model's, read with the file; it scrolls with the
//! options and the preview while the footer stays, never disables the
//! button, and each sheet opens scrolled to its top.
//!
//! The sheet resizes, as the Tool Library does (the smoke test's
//! leftover, 2026-11-20): by its edges and the grip at its corner, each
//! side it is dragged to kept in `gui.toml`. Until it is dragged it is
//! 560 px wide and as tall as its content. A wider sheet draws the
//! preview larger, and a taller one shows more of the layers and the
//! preview without scrolling.
//!
//! The model is `dc_app::import::DrawingImport`; the shell read the file.
//! The sheet changes the model's options in place, so every change reads
//! the bytes again, and asks the shell for *Import*, *Choose…* and close.
//! The footer has the one primary action and no Cancel: the header's `x`,
//! Escape and the scrim close the sheet (`experience.md`, section 13,
//! item 76).

use std::path::{Path, PathBuf};

use dc_app::import::{DrawingFormat, DrawingImport, Preview, Purpose};
use dc_app::prefs::{Prefs, SheetSize};
use dc_app::problems::Problems;
use dc_app::{Camera, StrKey, Strings};
use dc_geometry::{Point, Vec2 as MmVec2};
use dc_job::Tier;
use dc_project::{Project, Units};
use eframe::egui::{self, Align, Layout, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// The sheet's width until it is dragged, px.
const WIDTH_PX: f32 = 560.0;
/// The sheet's size among the preferences' `sizes`: each side the user
/// dragged it to. The default's height is not used: until its height is
/// dragged the sheet is as tall as its content. The smallest it goes
/// holds the file's row, an option and the footer.
const SHEET: SheetSize = SheetSize {
    key: "import",
    default_px: (560.0, 640.0),
    min_px: (440.0, 320.0),
};

/// `value` px as egui's `f32`. Sizes are a few thousand pixels at most,
/// far inside `f32`'s exact range.
#[allow(clippy::cast_possible_truncation, reason = "pixel sizes, exact in f32")]
fn px(value: f64) -> f32 {
    value as f32
}
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The header and a sheet footer button, px.
const HEADER_PX: f32 = 32.0;
const FOOTER_PX: f32 = theme::CONTROL_PX;
/// The preview thumbnail's height, px.
const PREVIEW_PX: f32 = 160.0;
/// The space left round the drawing in the thumbnail, px.
const PREVIEW_MARGIN_PX: f64 = 12.0;
/// How far a flattened arc may stray in the thumbnail, in the display's
/// own pixels: a tenth of one, under what a 1-px anti-aliased line shows,
/// so a circle reads as a circle and not as a polygon.
const PREVIEW_FLATTEN_DEVICE_PX: f64 = 0.1;

/// The tolerance the preview flattens arcs to, mm, drawn at `px_per_mm`
/// points to the millimetre on a display of `pixels_per_point`: the
/// preview's own pixel, so a drawing fitted small is not flattened finer
/// than it shows, and one on a 2× display is not left at half its
/// sharpness.
fn preview_tolerance_mm(px_per_mm: f64, pixels_per_point: f32) -> f64 {
    PREVIEW_FLATTEN_DEVICE_PX / (px_per_mm * f64::from(pixels_per_point)).max(f64::MIN_POSITIVE)
}
/// The tallest the layer list grows before it scrolls, px.
const LAYERS_MAX_PX: f32 = 168.0;

/// The sheet's state between frames: the model, and where the file is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sheet {
    model: DrawingImport,
    path: PathBuf,
    /// The join tolerance the user typed and the parser refused, with
    /// its reason.
    refused: Option<String>,
    /// Whether the sheet has yet to be drawn: its first frame puts its
    /// content at the top, wherever the last sheet was scrolled to.
    fresh: bool,
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Request {
    /// The sheet dragged by an edge or its grip: the size asked for, the
    /// size it had, and the window's, px.
    Resize {
        asked: egui::Vec2,
        was: egui::Vec2,
        screen: egui::Vec2,
    },
    /// Pick another file (*Choose…*).
    Choose,
    /// *Import*, or *Create project* for *New from drawing*.
    Confirm,
    /// Close the sheet, importing nothing.
    Close,
}

impl Sheet {
    /// The sheet over `bytes`, read from `path` as `format`, for `purpose`.
    pub(crate) fn new(
        path: &Path,
        name: &str,
        format: DrawingFormat,
        bytes: Vec<u8>,
        purpose: Purpose,
    ) -> Self {
        Self {
            model: DrawingImport::open(format, name, bytes, purpose),
            path: path.to_path_buf(),
            refused: None,
            fresh: true,
        }
    }

    /// The sheet over `model`, a drawing already read from `path` (a file
    /// dropped on the window, `dc_app::App::drop_file`).
    pub(crate) fn of(path: &Path, model: DrawingImport) -> Self {
        Self {
            model,
            path: path.to_path_buf(),
            refused: None,
            fresh: true,
        }
    }

    /// The model, for the shell to import from.
    pub(crate) fn model(&self) -> &DrawingImport {
        &self.model
    }
}

/// Keeps the sides of the sheet a drag changed (`asked` from `was`) in
/// `prefs`: a side that did not move stays as it was, so a sheet made
/// wider is still as tall as its content.
pub(crate) fn resize(prefs: &mut Prefs, asked: egui::Vec2, was: egui::Vec2, screen: egui::Vec2) {
    let window = (f64::from(screen.x), f64::from(screen.y));
    let (_, height) = SHEET.kept(prefs, window);
    let moved = |asked: f32, was: f32| (asked - was).abs() > 0.5;
    SHEET.keep(
        prefs,
        (
            moved(asked.x, was.x).then(|| f64::from(asked.x)),
            (moved(asked.y, was.y) || height.is_some()).then(|| f64::from(asked.y)),
        ),
        window,
    );
}

/// Shows the sheet over the open `project`, at the size `prefs` keeps for
/// it; what the user asked for goes to `out`.
pub(crate) fn show(
    ctx: &egui::Context,
    strings: &Strings,
    (project, prefs): (&Project, &Prefs),
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
    let (kept_width, kept_height) = SHEET.kept(prefs, (f64::from(screen.x), f64::from(screen.y)));
    let width = kept_width.map_or(WIDTH_PX.min(screen.x - 48.0), px);
    let height = kept_height.map(px);
    let frame = theme::sheet(palette);
    let response = egui::Modal::new(egui::Id::unique("import_drawing"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            let margin = f32::from(MARGIN_PX);
            let corner = ui.cursor().min - egui::vec2(margin, margin);
            ui.set_width(width - 2.0 * margin);
            match height {
                // Dragged: that tall, its options scrolling inside it.
                Some(height) => {
                    ui.set_min_height(height - 2.0 * margin);
                    ui.set_max_height(height - 2.0 * margin);
                }
                None => ui.set_max_height(screen.y - 96.0),
            }
            body(ui, strings, project, sheet, height.is_some(), out);
            // The sheet's frame, round its content: its edges and its
            // corner grip resize it.
            let tall = height.unwrap_or(ui.min_rect().height() + 2.0 * margin);
            let outer = egui::Rect::from_min_size(corner, egui::vec2(width, tall));
            if let Some(asked) = crate::widgets::sheet::resize_handles(
                ui,
                egui::Id::unique("import_drawing.resize"),
                outer,
                strings.get(StrKey::AppImportResize),
            ) {
                out.push(Action::ImportSheet(Request::Resize {
                    asked,
                    was: outer.size(),
                    screen,
                }));
            }
        });
    if response.should_close() {
        out.push(Action::ImportSheet(Request::Close));
    }
}

/// The sheet's content. `fixed` is whether the sheet's height is the
/// user's: the options then fill what the header and the footer leave,
/// and the help line takes its room from them, not from the sheet.
fn body(
    ui: &mut Ui,
    strings: &Strings,
    project: &Project,
    sheet: &mut Sheet,
    fixed: bool,
    out: &mut Vec<Action>,
) {
    header(ui, strings, sheet.model.purpose(), out);
    ui.add_space(8.0);
    file_row(ui, strings, sheet, out);
    ui.add_space(4.0);
    summary(ui, strings, &sheet.model);
    ui.add_space(12.0);
    // The options, the layers and the preview scroll when the window is
    // short; the header and footer stay.
    let footer_h = FOOTER_PX + 16.0;
    let foot_px = crate::widgets::help::foot_height(
        ui,
        crate::widgets::help::Scope::Sheet,
        ui.available_width() + 2.0 * f32::from(MARGIN_PX),
    );
    // A sheet of the user's height holds its footer, the gap egui puts
    // after the options, and the help line while it shows.
    let foot_room = match (fixed, foot_px > 0.0) {
        (true, true) => foot_px + 8.0 + 2.0 * ui.spacing().item_spacing.y,
        (true, false) => ui.spacing().item_spacing.y,
        (false, _) => 0.0,
    };
    let least = if fixed { 0.0 } else { 120.0 };
    let room = (ui.available_height() - footer_h - foot_room).max(least);
    let mut area = egui::ScrollArea::vertical()
        .max_height(room)
        .auto_shrink([false, !fixed]);
    // A list of checks can be longer than the sheet: the next drawing's
    // sheet starts at its top, not where this one was left.
    if std::mem::take(&mut sheet.fresh) {
        area = area.vertical_scroll_offset(0.0);
    }
    crate::widgets::scroll::show(ui, area, crate::widgets::scroll::Surface::Raised, |ui| {
        if sheet.model.format() == DrawingFormat::Dxf {
            options(ui, strings, project.display_units, sheet);
            ui.add_space(12.0);
            layers(ui, strings, &mut sheet.model);
            ui.add_space(12.0);
        }
        heading(ui, strings.get(StrKey::AppImportPreview));
        preview(ui, &sheet.model);
        notes(ui, strings, &sheet.model, out);
    });
    ui.add_space(16.0);
    footer(ui, strings, &sheet.model, out);
    // The help of the field being typed in, at the sheet's foot.
    crate::widgets::help::settle(ui, crate::widgets::help::Scope::Sheet);
    if foot_px > 0.0 {
        ui.add_space(8.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), foot_px),
            egui::Sense::hover(),
        );
        crate::widgets::help::foot(
            ui,
            crate::widgets::help::Scope::Sheet,
            rect.expand2(egui::vec2(f32::from(MARGIN_PX), 0.0)),
        );
    }
}

/// The 32-px header: the title and `x`.
fn header(ui: &mut Ui, strings: &Strings, purpose: Purpose, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let title = match purpose {
        Purpose::Import => StrKey::AppImportTitle,
        Purpose::New => StrKey::AppImportTitleNew,
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), HEADER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let close = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::X, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::AppImportClose));
            if button(ui, close).clicked() {
                out.push(Action::ImportSheet(Request::Close));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                text(
                    ui,
                    strings.get(title),
                    TextStyle::Name(theme::DISPLAY.into()),
                    palette.text_primary,
                );
            });
        },
    );
}

/// The file's path and *Choose…*.
fn file_row(ui: &mut Ui, strings: &Strings, sheet: &Sheet, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        row::label(
            ui,
            strings.get(StrKey::AppImportFile),
            strings.get(StrKey::AppImportFileHelp),
            None,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let choose = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppImportChoose));
            if button(ui, choose).clicked() {
                out.push(Action::ImportSheet(Request::Choose));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // Cut at its start: the file's name is at its end.
                row::path(ui, &sheet.path.display().to_string());
            });
        });
    });
}

/// `DXF · 9 closed, 1 open, 7 layers · units from the file: mm`.
fn summary(ui: &mut Ui, strings: &Strings, model: &DrawingImport) {
    let palette = Palette::current(ui);
    let line = match (model.summary(), model.format()) {
        (None, _) => strings.fmt(
            StrKey::AppImportSummaryUnread,
            &[("format", format_name(strings, model.format()))],
        ),
        (Some(s), DrawingFormat::Svg) => strings.fmt(
            StrKey::AppImportSummarySvg,
            &[
                ("closed", &s.closed.to_string()),
                ("open", &s.open.to_string()),
            ],
        ),
        (Some(s), DrawingFormat::Dxf) => strings.fmt(
            StrKey::AppImportSummaryDxf,
            &[
                ("closed", &s.closed.to_string()),
                ("open", &s.open.to_string()),
                ("layers", &s.layers.to_string()),
                ("units", &units_read(strings, model)),
            ],
        ),
    };
    wrapped(ui, &line, TextStyle::Body, palette.text_secondary);
}

fn format_name(strings: &Strings, format: DrawingFormat) -> &str {
    strings.get(match format {
        DrawingFormat::Svg => StrKey::AppImportSvg,
        DrawingFormat::Dxf => StrKey::AppImportDxf,
    })
}

/// Where the units come from: the file, the dropdown, or nowhere yet.
fn units_read(strings: &Strings, model: &DrawingImport) -> String {
    match (model.units(), model.declared_units()) {
        (Some(units), _) => strings.fmt(
            StrKey::AppImportUnitsChosen,
            &[("units", unit_name(strings, units))],
        ),
        (None, Some(declared)) => strings.fmt(
            StrKey::AppImportUnitsDeclared,
            &[("units", &declared.to_string())],
        ),
        (None, None) => strings.get(StrKey::AppImportUnitsMissing).to_owned(),
    }
}

fn unit_name(strings: &Strings, units: Units) -> &str {
    strings.get(match units {
        Units::Mm => StrKey::AppImportUnitsMm,
        Units::Inch => StrKey::AppImportUnitsInch,
    })
}

/// A 13-px SemiBold group heading.
fn heading(ui: &mut Ui, title: &str) {
    let palette = Palette::current(ui);
    text(
        ui,
        title,
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(4.0);
}

/// The DXF options: units and join tolerance.
fn options(ui: &mut Ui, strings: &Strings, display_units: Units, sheet: &mut Sheet) {
    heading(ui, strings.get(StrKey::AppImportOptions));
    let model = &mut sheet.model;
    let units_label = strings.get(StrKey::AppImportUnits);
    let mut chosen = model.units();
    // Required and unset: the error state, the outline and the glyph
    // whose tooltip is the importer's message (`experience.md`, section
    // 7, "Field problem").
    let missing = model.units_required() && model.units().is_none();
    let unset = match (missing, model.blocked()) {
        (true, Some(reason)) => Problems::said(strings, Tier::Error, &reason),
        _ => Problems::none(),
    };
    ui.horizontal(|ui| {
        let label = row::label(
            ui,
            units_label,
            strings.get(StrKey::AppImportUnitsHelp),
            None,
        );
        let selected = match (chosen, model.declared_units()) {
            (Some(units), _) => unit_name(strings, units).to_owned(),
            (None, Some(declared)) => strings.fmt(
                StrKey::AppImportUnitsFromFile,
                &[("units", &declared.to_string())],
            ),
            (None, None) => strings.get(StrKey::AppImportUnitsChoose).to_owned(),
        };
        let width = ui.available_width();
        let dropdown = egui::ComboBox::from_id_salt("import_units")
            .icon(crate::widgets::icon::combo_chevron)
            .width(width)
            .selected_text(crate::widgets::problem::dropdown_text(
                ui, &selected, width, &unset,
            ))
            .show_ui(ui, |ui| {
                if let Some(declared) = model.declared_units() {
                    let from_file = strings.fmt(
                        StrKey::AppImportUnitsFromFile,
                        &[("units", &declared.to_string())],
                    );
                    ui.selectable_value(&mut chosen, None, from_file);
                }
                for units in [Units::Mm, Units::Inch] {
                    ui.selectable_value(&mut chosen, Some(units), unit_name(strings, units));
                }
            })
            .response
            .labelled_by(label.id);
        let shape = (dropdown.rect, theme::CONTROL_RADIUS_PX);
        let place = crate::widgets::problem::in_dropdown(ui);
        let marked =
            crate::widgets::problem::mark(ui, dropdown, units_label, &unset, Some(shape), place);
        let _ = crate::widgets::problem::dropdown_whole(ui, marked, &selected);
    });
    if chosen != model.units() {
        model.set_units(chosen);
    }
    ui.add_space(4.0);

    let tolerance_label = strings.get(StrKey::AppImportJoinTolerance);
    let value = dc_app::units::field_text(model.join_tolerance_mm(), display_units);
    let refused_by_importer = (model.join_tolerance_mm() < 0.0
        || !model.join_tolerance_mm().is_finite())
    .then(|| model.blocked())
    .flatten();
    // What the field refused, or the importer refuses of the tolerance.
    let problems = match (&sheet.refused, refused_by_importer) {
        (Some(reason), _) => Problems::refused(strings, reason),
        (None, Some(reason)) => Problems::said(strings, Tier::Error, &reason),
        (None, None) => Problems::none(),
    };
    ui.horizontal(|ui| {
        row::label(
            ui,
            tolerance_label,
            strings.get(StrKey::AppImportJoinToleranceHelp),
            None,
        );
        let typed = unit_field(
            ui,
            UnitField {
                id: egui::Id::unique("import_join_tolerance"),
                name: tolerance_label,
                value: &value,
                unit: dc_app::units::suffix(display_units),
                placeholder: "",
                enabled: true,
                disabled_reason: None,
                problems: Some(&problems),
                reset: None,
                required: None,
            },
        );
        if let Some(text) = typed.committed {
            match dc_app::units::parse_length_mm(&text, display_units) {
                Ok(tolerance_mm) => {
                    sheet.refused = None;
                    sheet.model.set_join_tolerance_mm(tolerance_mm);
                }
                Err(e) => sheet.refused = Some(e.to_string()),
            }
        }
    });
}

/// The layers to tick, each with what it holds; a layer of nothing the
/// importer reads is greyed.
fn layers(ui: &mut Ui, strings: &Strings, model: &mut DrawingImport) {
    heading(ui, strings.get(StrKey::AppImportLayers));
    let palette = Palette::current(ui);
    if model.layers().is_empty() {
        wrapped(
            ui,
            strings.get(StrKey::AppImportLayersUnread),
            TextStyle::Body,
            palette.text_secondary,
        );
        return;
    }
    let rows: Vec<(String, String, bool)> = model
        .layers()
        .iter()
        .map(|layer| {
            let shapes = layer.closed + layer.open;
            let unread: Vec<String> = layer
                .skipped
                .iter()
                .map(|(kind, count)| {
                    strings.fmt(
                        StrKey::AppImportUnreadKind,
                        &[("kind", kind), ("count", &count.to_string())],
                    )
                })
                .collect();
            let unread = unread.join(strings.get(StrKey::AppImportListSeparator));
            let closed = layer.closed.to_string();
            let open = layer.open.to_string();
            let args = [
                ("name", layer.name.as_str()),
                ("closed", closed.as_str()),
                ("open", open.as_str()),
                ("unread", unread.as_str()),
            ];
            let label = match (shapes, unread.is_empty()) {
                (0, false) => strings.fmt(StrKey::AppImportLayerUnread, &args),
                (_, true) => strings.fmt(StrKey::AppImportLayer, &args),
                (_, false) => strings.fmt(StrKey::AppImportLayerPartlyUnread, &args),
            };
            (layer.name.clone(), label, shapes > 0)
        })
        .collect();
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("import_layers")
            .max_height(LAYERS_MAX_PX)
            .auto_shrink([false, true]),
        crate::widgets::scroll::Surface::Raised,
        |ui| {
            for (name, label, readable) in rows {
                let mut ticked = readable && model.is_ticked(&name);
                let response = ui
                    .add_enabled_ui(readable, |ui| checkbox(ui, &name, &mut ticked, &label))
                    .inner;
                if !readable {
                    response.on_disabled_hover_text(strings.get(StrKey::AppImportLayerNotRead));
                } else if response.changed() {
                    model.set_layer(&name, ticked);
                }
            }
        },
    );
}

/// The preview: the drawing alone, as drawn, fitted to the thumbnail
/// (capability 55; mock-up 02): no stock outline, no origin and no
/// placement note, since the selected shapes on the Viewport show where
/// they landed.
fn preview(ui: &mut Ui, model: &DrawingImport) {
    let palette = Palette::current(ui);
    // As tall for its width as in the sheet's own size: a sheet dragged
    // wider draws the drawing larger.
    let own = WIDTH_PX - 2.0 * f32::from(MARGIN_PX);
    let height = PREVIEW_PX * (ui.available_width() / own).max(1.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::INNER_CARD_RADIUS_PX, palette.viewport_bg);
    painter.rect_stroke(
        rect,
        theme::INNER_CARD_RADIUS_PX,
        Stroke::new(1.0, palette.border_default),
        StrokeKind::Inside,
    );
    let Some(Preview { outlines, bounds }) = model.preview() else {
        return;
    };
    let size_px = MmVec2::new(f64::from(rect.width()), f64::from(rect.height()));
    let room = |px: f64| (px - 2.0 * PREVIEW_MARGIN_PX).max(1.0);
    let px_per_mm = (room(size_px.x) / bounds.width_mm().max(1e-6))
        .min(room(size_px.y) / bounds.height_mm().max(1e-6));
    // The Viewport's camera, so the thumbnail flips Y where the Viewport
    // does, once, in `Camera::to_screen`.
    let camera = Camera {
        centre_mm: Point::new(
            (bounds.min.x + bounds.max.x) / 2.0,
            (bounds.min.y + bounds.max.y) / 2.0,
        ),
        px_per_mm,
        viewport_px: size_px,
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "screen coordinates, exact enough in f32"
    )]
    let on_screen = |p: Point| {
        let s = camera.to_screen(p);
        rect.min + egui::vec2(s.x_px as f32, s.y_px as f32)
    };
    let tolerance_mm = preview_tolerance_mm(px_per_mm, ui.pixels_per_point());
    for outline in &outlines {
        let flat = outline.with_arcs_flattened(tolerance_mm);
        let points: Vec<egui::Pos2> = flat
            .vertices()
            .iter()
            .map(|v| on_screen(v.point()))
            .collect();
        let stroke = Stroke::new(1.0, palette.viewport_shape);
        if flat.is_closed() {
            painter.add(Shape::closed_line(points, stroke));
        } else {
            painter.add(Shape::line(points, stroke));
        }
    }
}

/// The importer's warnings, as the Warnings panel will list them, then
/// the artwork checks that fired (capability 55): each check with its
/// tier's glyph, its code, which opens its entry in the Warnings
/// chapter as a drawer row's does, and its message as the CLI words it,
/// whole. Under the list, when a check fired, one quiet line says what
/// to do, since the app does not clean a drawing: run `dreamcarve
/// image-prep`, with a link to the *Preparing artwork* chapter. The list
/// is the model's, read with the file; nothing is checked here.
fn notes(ui: &mut Ui, strings: &Strings, model: &DrawingImport, out: &mut Vec<Action>) {
    let warnings = model.warnings();
    let checks = model.checks();
    if warnings.is_empty() && checks.is_empty() {
        return;
    }
    let palette = Palette::current(ui);
    ui.add_space(8.0);
    wrapped(
        ui,
        strings.get(StrKey::AppImportNotesHeading),
        TextStyle::Small,
        palette.text_secondary,
    );
    for warning in warnings {
        note(ui, Tier::Warning, |ui| {
            wrapped(
                ui,
                &warning.to_string(),
                TextStyle::Small,
                palette.status_warning.text,
            );
        });
    }
    for check in checks {
        let tier = check.tier();
        let (_, colours) = crate::widgets::problem::look(palette, tier);
        note(ui, tier, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let code = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(check.code)
                            .text_style(TextStyle::Monospace)
                            .color(palette.accent),
                    )
                    .selectable(false)
                    .sense(Sense::click()),
                )
                .on_hover_text(strings.fmt(StrKey::AppImportCheckChapter, &[("code", check.code)]));
            if code.clicked() {
                out.push(Action::OpenUrl(format!(
                    "{}#{}",
                    dc_app::warnings::CHAPTER_URL,
                    check.code
                )));
            }
            wrapped(ui, &check.message, TextStyle::Small, colours.text);
        });
    }
    if model.advises_prep() {
        ui.add_space(8.0);
        wrapped(
            ui,
            strings.get(StrKey::AppImportPrepAdvice),
            TextStyle::Small,
            palette.text_secondary,
        );
        let chapter = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(strings.get(StrKey::AppImportPrepChapter))
                        .text_style(TextStyle::Small)
                        .color(palette.accent),
                )
                .selectable(false)
                .sense(Sense::click()),
            )
            .on_hover_text(strings.get(StrKey::AppImportPrepChapterHelp));
        if chapter.clicked() {
            out.push(Action::OpenUrl(
                dc_app::import::PREPARING_ARTWORK_URL.to_owned(),
            ));
        }
    }
}

/// One line of the sheet's notes: the glyph of `tier` in its colour,
/// then what `body` draws, top-aligned with it and as wide as the room
/// left.
fn note(ui: &mut Ui, tier: Tier, body: impl FnOnce(&mut Ui)) {
    let palette = Palette::current(ui);
    let (icon, colours) = crate::widgets::problem::look(palette, tier);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colours.stroke);
        let (rect, _) = ui.allocate_exact_size(glyph.size(), Sense::hover());
        ui.painter().galley(rect.min, glyph, colours.stroke);
        ui.vertical(body);
    });
}

/// The footer: the one primary action, disabled with the reason when the
/// options leave nothing to import.
fn footer(ui: &mut Ui, strings: &Strings, model: &DrawingImport, out: &mut Vec<Action>) {
    // Mock-up 02: *Add 14 shapes*, the count the summary gives.
    let shapes = model.summary().map(|s| s.closed + s.open);
    let label = match (model.purpose(), shapes) {
        (Purpose::Import, Some(1)) => strings.get(StrKey::AppImportAddOne).to_owned(),
        (Purpose::Import, Some(count)) if count > 1 => {
            strings.fmt(StrKey::AppImportAddMany, &[("count", &count.to_string())])
        }
        (Purpose::Import, _) => strings.get(StrKey::AppImportAdd).to_owned(),
        (Purpose::New, _) => strings.get(StrKey::AppImportCreate).to_owned(),
    };
    let reason = model.blocked();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let mut spec = ButtonSpec::new(Kind::Primary, &label)
                .height(FOOTER_PX)
                .enabled(reason.is_none());
            if let Some(reason) = reason.as_deref() {
                spec = spec.tooltip(reason);
            }
            if button(ui, spec).clicked() {
                out.push(Action::ImportSheet(Request::Confirm));
            }
        },
    );
}

/// `text` wrapped to the width.
fn wrapped(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
            .wrap()
            .selectable(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Row D5, defect 4: the coaster's circle in the preview is a circle.
    /// Fitted to the 160-px thumbnail it is 136 points across, and the
    /// preview flattened it to within half a point: 26 chords of 16
    /// points, a polygon. Flattened at the preview's own pixel no chord
    /// leaves the arc by more than a tenth of a device pixel, at 1× and
    /// at 2×, and a drawing fitted smaller is flattened no finer than it
    /// shows.
    #[test]
    fn preview_flattens_a_circle_to_a_tenth_of_a_device_pixel() {
        let radius_mm = 45.0;
        let circle = dc_geometry::Polyline::circle(Point::new(0.0, 0.0), radius_mm);
        // The thumbnail's room for the drawing, in points.
        let across_pt = f64::from(PREVIEW_PX) - 2.0 * PREVIEW_MARGIN_PX;
        let mut chords = Vec::new();
        for (px_per_mm, scale) in [
            (across_pt / (2.0 * radius_mm), 1.0_f32),
            (across_pt / (2.0 * radius_mm), 2.0),
            (0.2 * across_pt / (2.0 * radius_mm), 2.0),
        ] {
            let flat = circle.with_arcs_flattened(preview_tolerance_mm(px_per_mm, scale));
            let points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
            let device_px_per_mm = px_per_mm * f64::from(scale);
            let mut worst_px: f64 = 0.0;
            for (n, a) in points.iter().enumerate() {
                let b = points[(n + 1) % points.len()];
                let middle = Point::new(f64::midpoint(a.x, b.x), f64::midpoint(a.y, b.y));
                let off_mm = radius_mm - middle.x.hypot(middle.y);
                worst_px = worst_px.max(off_mm * device_px_per_mm);
            }
            assert!(
                worst_px <= PREVIEW_FLATTEN_DEVICE_PX + 1e-9,
                "{px_per_mm} pt/mm at {scale}×: a chord is {worst_px} device px off the arc"
            );
            chords.push(points.len());
        }
        assert!(
            chords[0] >= 56,
            "136 points across at 1×: {} chords",
            chords[0]
        );
        assert!(chords[1] > chords[0], "finer on a 2× display: {chords:?}");
        assert!(
            chords[2] < chords[0],
            "coarser when fitted small: {chords:?}"
        );
    }
}
