// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Import drawing sheet (functional spec, capabilities 51 and 55;
//! mock-up `02-import-dialog.svg`; `experience.md`, sections 7 and 8,
//! "Sheets"): a 560-px sheet over a scrim that names the file and the
//! shapes it holds (closed and open, the layers, the units read from the
//! file), and, for a DXF, the importer's options as `dreamcarve init`
//! takes them: the units (required when the file declares none, with the
//! importer's message under the dropdown and *Import* disabled), the join
//! tolerance and a layer list to tick, in which a layer of entities the
//! importer does not read is listed greyed. An SVG has no options. A
//! preview thumbnail draws the outlines where they will land, through the
//! Viewport camera's one Y flip, and the importer's warnings are listed
//! as the Warnings panel will list them.
//!
//! The model is `dc_app::import::DrawingImport`; the shell read the file.
//! The sheet changes the model's options in place, so every change reads
//! the bytes again, and asks the shell for *Import*, *Choose…* and close.
//! The footer has the one primary action and no Cancel: the header's `x`,
//! Escape and the scrim close the sheet (`experience.md`, section 13,
//! item 9).

use std::path::{Path, PathBuf};

use dc_app::import::{DrawingFormat, DrawingImport, Purpose};
use dc_app::{Camera, StrKey, Strings};
use dc_geometry::{Point, Polyline, Rect as MmRect, Transform, Vec2 as MmVec2};
use dc_job::Tier;
use dc_project::{Project, Units};
use eframe::egui::{self, Align, Layout, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// The sheet's width, px.
const WIDTH_PX: f32 = 560.0;
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The header and a sheet footer button, px.
const HEADER_PX: f32 = 32.0;
const FOOTER_PX: f32 = 28.0;
/// The preview thumbnail's height, px.
const PREVIEW_PX: f32 = 160.0;
/// The space left round the drawing in the thumbnail, px.
const PREVIEW_MARGIN_PX: f64 = 12.0;
/// How far a flattened arc may stray in the thumbnail, px.
const PREVIEW_FLATTEN_PX: f64 = 0.5;
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
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
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
        }
    }

    /// The model, for the shell to import from.
    pub(crate) fn model(&self) -> &DrawingImport {
        &self.model
    }
}

/// Shows the sheet over the open `project`; what the user asked for goes
/// to `out`.
pub(crate) fn show(
    ctx: &egui::Context,
    strings: &Strings,
    project: &Project,
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
    let width = WIDTH_PX.min(screen.x - 48.0);
    let frame = egui::Frame::popup(&style)
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .inner_margin(MARGIN_PX);
    let response = egui::Modal::new(egui::Id::unique("import_drawing"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width - 2.0 * f32::from(MARGIN_PX));
            ui.set_max_height(screen.y - 96.0);
            body(ui, strings, project, sheet, out);
        });
    if response.should_close() {
        out.push(Action::ImportSheet(Request::Close));
    }
}

fn body(
    ui: &mut Ui,
    strings: &Strings,
    project: &Project,
    sheet: &mut Sheet,
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
    let room = (ui.available_height() - footer_h).max(120.0);
    egui::ScrollArea::vertical()
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if sheet.model.format() == DrawingFormat::Dxf {
                options(ui, strings, project.display_units, sheet);
                ui.add_space(12.0);
                layers(ui, strings, &mut sheet.model);
                ui.add_space(12.0);
            }
            heading(ui, strings.get(StrKey::AppImportPreview));
            preview(ui, project, &sheet.model);
            ui.add_space(8.0);
            placement(ui, strings, &sheet.model);
            notes(ui, strings, &sheet.model);
        });
    ui.add_space(16.0);
    footer(ui, strings, &sheet.model, out);
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
    let palette = Palette::current(ui);
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
                let path = sheet.path.display().to_string();
                text(ui, &path, TextStyle::Monospace, palette.text_primary).on_hover_text(&path);
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
    // Required and unset: the error state, outline and glyph and message
    // (`experience.md`, section 7).
    let missing = model.units_required() && model.units().is_none();
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
        let dropdown = egui::ComboBox::from_id_salt("import_units")
            .width(ui.available_width())
            .selected_text(selected)
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
        if missing {
            let palette = Palette::current(ui);
            ui.painter().rect_stroke(
                dropdown.rect,
                4.0,
                Stroke::new(1.0, palette.status_error.stroke),
                StrokeKind::Inside,
            );
        }
    });
    if chosen != model.units() {
        model.set_units(chosen);
    }
    if let (true, Some(reason)) = (missing, model.blocked()) {
        row::message(ui, Tier::Error, &reason);
    }
    ui.add_space(4.0);

    let tolerance_label = strings.get(StrKey::AppImportJoinTolerance);
    let value = dc_app::units::field_text(model.join_tolerance_mm(), display_units);
    let refused_by_importer = (model.join_tolerance_mm() < 0.0
        || !model.join_tolerance_mm().is_finite())
    .then(|| model.blocked())
    .flatten();
    let message = sheet.refused.clone().or(refused_by_importer);
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
                tier: message.as_ref().map(|_| Tier::Error),
                reset: None,
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
    if let Some(message) = message {
        row::message(ui, Tier::Error, &message);
    }
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
    egui::ScrollArea::vertical()
        .id_salt("import_layers")
        .max_height(LAYERS_MAX_PX)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (name, label, readable) in rows {
                let mut ticked = readable && model.is_ticked(&name);
                let response = ui.add_enabled(readable, egui::Checkbox::new(&mut ticked, label));
                if !readable {
                    response.on_disabled_hover_text(strings.get(StrKey::AppImportLayerNotRead));
                } else if response.changed() {
                    model.set_layer(&name, ticked);
                }
            }
        });
}

/// The preview: the outlines where *Import* puts them, on the stock they
/// land on, fitted to the thumbnail.
fn preview(ui: &mut Ui, project: &Project, model: &DrawingImport) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), PREVIEW_PX), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, palette.viewport_bg);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, palette.border_default),
        StrokeKind::Inside,
    );
    let Some((stock, outlines)) = placed(project, model) else {
        return;
    };
    let bounds = outlines
        .iter()
        .filter_map(Polyline::bounds)
        .fold(stock, |acc, b| acc.including(b.min).including(b.max));
    let size_px = MmVec2::new(f64::from(rect.width()), f64::from(rect.height()));
    let room = |px: f64| (px - 2.0 * PREVIEW_MARGIN_PX).max(1.0);
    let px_per_mm = (room(size_px.x) / bounds.width_mm().max(1.0))
        .min(room(size_px.y) / bounds.height_mm().max(1.0));
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
    let corners = [
        Point::new(stock.min.x, stock.min.y),
        Point::new(stock.max.x, stock.min.y),
        Point::new(stock.max.x, stock.max.y),
        Point::new(stock.min.x, stock.max.y),
        Point::new(stock.min.x, stock.min.y),
    ];
    let stock_px: Vec<egui::Pos2> = corners.into_iter().map(on_screen).collect();
    painter.add(Shape::convex_polygon(
        stock_px.get(..4).unwrap_or_default().to_vec(),
        palette.viewport_stock_fill,
        Stroke::NONE,
    ));
    painter.extend(Shape::dashed_line(
        &stock_px,
        Stroke::new(1.0, palette.viewport_stock),
        4.0,
        3.0,
    ));
    let tolerance_mm = PREVIEW_FLATTEN_PX / px_per_mm;
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

/// The stock the shapes land on and their outlines there: the open
/// project's stock with the shapes as drawn, or, for a new project, the
/// stock `init` sizes round them with the shapes moved onto it.
fn placed(project: &Project, model: &DrawingImport) -> Option<(MmRect, Vec<Polyline>)> {
    let outlines = model.outlines();
    let bounds = model.bounds()?;
    let (width_mm, height_mm, shift) = match model.purpose() {
        Purpose::Import => (
            project.stock.width_mm,
            project.stock.height_mm,
            Transform::IDENTITY,
        ),
        Purpose::New => {
            let fit =
                dc_job::project_from::fit_stock(bounds, dc_job::project_from::STOCK_MARGIN_MM);
            (fit.width_mm, fit.height_mm, fit.shift)
        }
    };
    let stock = MmRect {
        min: Point::new(0.0, 0.0),
        max: Point::new(width_mm, height_mm),
    };
    let outlines = outlines
        .iter()
        .filter_map(|p| shift.apply_polyline(p).ok())
        .collect();
    Some((stock, outlines))
}

/// Where the shapes go: as drawn and selected, or on a new stock.
fn placement(ui: &mut Ui, strings: &Strings, model: &DrawingImport) {
    let palette = Palette::current(ui);
    let line = match model.purpose() {
        Purpose::Import => strings.get(StrKey::AppImportPlacedAsDrawn).to_owned(),
        Purpose::New => strings.fmt(
            StrKey::AppImportPlacedNew,
            &[(
                "margin",
                &dc_app::units::field_text(dc_job::project_from::STOCK_MARGIN_MM, Units::Mm),
            )],
        ),
    };
    wrapped(ui, &line, TextStyle::Small, palette.text_secondary);
}

/// The importer's warnings, as the Warnings panel will list them.
fn notes(ui: &mut Ui, strings: &Strings, model: &DrawingImport) {
    let warnings = model.warnings();
    if warnings.is_empty() {
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
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let glyph = Icon::TriangleAlert.galley(ui, 12.0, palette.status_warning.stroke);
            let (rect, _) = ui.allocate_exact_size(glyph.size(), Sense::hover());
            ui.painter()
                .galley(rect.min, glyph, palette.status_warning.stroke);
            wrapped(
                ui,
                &warning.to_string(),
                TextStyle::Small,
                palette.status_warning.text,
            );
        });
    }
}

/// The footer: the one primary action, disabled with the reason when the
/// options leave nothing to import.
fn footer(ui: &mut Ui, strings: &Strings, model: &DrawingImport, out: &mut Vec<Action>) {
    let label = match model.purpose() {
        Purpose::Import => StrKey::AppImportImport,
        Purpose::New => StrKey::AppImportCreate,
    };
    let reason = model.blocked();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let mut spec = ButtonSpec::new(Kind::Primary, strings.get(label))
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
