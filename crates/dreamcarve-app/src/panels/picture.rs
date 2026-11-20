// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Add picture sheet (functional spec, capabilities 51 and 56;
//! `experience.md`, sections 7 and 8, "Sheets"; `redesign.md`, "Sheet"):
//! the Import drawing sheet's idiom for a picture. It names the file and
//! its size in pixels, shows the picture as the bitmap will store it (the
//! gray it was converted to), and asks the one thing the file does not
//! say: how big it is on the stock. *Width* or *Height* is typed in
//! project units and the other follows the pixels, the proportion link
//! between them always closed. What decoding converted (a colour picture
//! to grayscale, 16-bit samples, transparency) is listed as the Warnings
//! panel will list it. The footer's one primary button, *Add picture* or
//! *Create project*, is disabled with the reason as its tooltip until a
//! size is typed, and while the file or the size is refused; the reason
//! also stands under the size.
//!
//! The model is `dc_app::import::PictureImport`, which decoded the file
//! once when the shell opened the sheet; nothing here decodes. The
//! thumbnail is an egui texture made once from the model's pixels, thinned
//! to at most [`PREVIEW_MAX_PX`] a side so a camera's picture costs a
//! frame nothing. The sheet has no Cancel: the header's `x`, Escape and
//! the scrim close it (`experience.md`, section 13, item 76).

use std::path::{Path, PathBuf};

use dc_app::import::{PictureImport, PictureSide, Purpose};
use dc_app::{StrKey, Strings};
use dc_import::ImageFormat;
use dc_job::Tier;
use dc_project::{Grayscale, Units};
use eframe::egui::{self, Align, Layout, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::panels::properties::closed_link;
use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::unit_field::{UnitField, field as unit_field, row as field_row};

/// The sheet's width, px: the Import drawing sheet's.
const WIDTH_PX: f32 = 560.0;
/// Its margin, px.
const MARGIN_PX: i8 = 24;
/// The header and the footer button, px.
const HEADER_PX: f32 = 32.0;
const FOOTER_PX: f32 = theme::CONTROL_PX;
/// The thumbnail's height, px.
const PREVIEW_PX: f32 = 160.0;
/// The space left round the picture in the thumbnail, px.
const PREVIEW_MARGIN_PX: f32 = 12.0;
/// The longest side of the thumbnail's texture, pixels.
const PREVIEW_MAX_PX: u32 = 640;
/// The proportion link's column right of the two size fields, px.
const LINK_COLUMN_PX: f32 = 32.0;

/// The sheet's state between frames: the model, where the file is, and
/// the thumbnail.
pub(crate) struct Sheet {
    model: PictureImport,
    path: PathBuf,
    /// What was typed in a size field and is not a length, with the side
    /// it was typed in and why.
    refused: Option<(PictureSide, String)>,
    /// The thumbnail's texture, made when the sheet first draws.
    thumbnail: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for Sheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("picture::Sheet")
            .field("path", &self.path)
            .field("size_px", &self.model.size_px())
            .finish_non_exhaustive()
    }
}

/// What the sheet asks the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    /// Pick another file (*Choose…*).
    Choose,
    /// *Add picture*, or *Create project* for *New from picture*.
    Confirm,
    /// Close the sheet, adding nothing.
    Close,
}

impl Sheet {
    /// The sheet over `model`, the picture the shell read from `path`.
    pub(crate) fn new(path: &Path, model: PictureImport) -> Self {
        Self {
            model,
            path: path.to_path_buf(),
            refused: None,
            thumbnail: None,
        }
    }

    /// The model, for the shell to import from.
    pub(crate) fn model(&self) -> &PictureImport {
        &self.model
    }
}

/// `pixels` thinned to at most [`PREVIEW_MAX_PX`] a side, as an egui
/// image: every `step`-th sample of every `step`-th row. A thumbnail, so
/// nearest samples are enough; the Viewport's texture is the whole
/// picture.
fn thumbnail(pixels: &Grayscale) -> egui::ColorImage {
    let (width, height) = (pixels.width_px.max(1), pixels.height_px.max(1));
    let step = width.max(height).div_ceil(PREVIEW_MAX_PX).max(1);
    let size = [width.div_ceil(step), height.div_ceil(step)];
    let mut gray = Vec::with_capacity(size[0] as usize * size[1] as usize);
    for row in (0..height).step_by(step as usize) {
        for column in (0..width).step_by(step as usize) {
            let at = row as usize * width as usize + column as usize;
            gray.push(pixels.bytes.get(at).copied().unwrap_or(0));
        }
    }
    egui::ColorImage::from_gray([size[0] as usize, size[1] as usize], &gray)
}

/// Shows the sheet over a project shown in `units`; what the user asked
/// for goes to `out`.
pub(crate) fn show(
    ctx: &egui::Context,
    strings: &Strings,
    units: Units,
    sheet: &mut Sheet,
    out: &mut Vec<Action>,
) {
    let style = ctx.global_style();
    let palette = if style.visuals.dark_mode {
        &theme::DARK
    } else {
        &theme::LIGHT
    };
    if sheet.thumbnail.is_none()
        && let Some(pixels) = sheet.model.pixels()
    {
        sheet.thumbnail = Some(ctx.load_texture(
            "add_picture_thumbnail",
            thumbnail(pixels),
            egui::TextureOptions::LINEAR,
        ));
    }
    let screen = ctx.content_rect().size();
    let width = WIDTH_PX.min(screen.x - 48.0);
    let response = egui::Modal::new(egui::Id::unique("add_picture"))
        .backdrop_color(palette.scrim)
        .frame(theme::sheet(palette))
        .show(ctx, |ui| {
            ui.set_width(width - 2.0 * f32::from(MARGIN_PX));
            ui.set_max_height(screen.y - 96.0);
            body(ui, strings, units, sheet, out);
        });
    if response.should_close() {
        out.push(Action::PictureSheet(Request::Close));
    }
}

fn body(ui: &mut Ui, strings: &Strings, units: Units, sheet: &mut Sheet, out: &mut Vec<Action>) {
    header(ui, strings, sheet.model.purpose(), out);
    ui.add_space(8.0);
    file_row(ui, strings, sheet, out);
    ui.add_space(4.0);
    summary(ui, strings, &sheet.model);
    ui.add_space(12.0);
    // The preview, the size and the notes scroll when the window is
    // short; the header and footer stay.
    let footer_h = FOOTER_PX + 16.0;
    let room = (ui.available_height() - footer_h).max(120.0);
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .max_height(room)
            .auto_shrink([false, true]),
        crate::widgets::scroll::Surface::Raised,
        |ui| {
            heading(ui, strings.get(StrKey::AppImportPreview));
            preview(ui, sheet);
            ui.add_space(12.0);
            heading(ui, strings.get(StrKey::AppPictureSize));
            size(ui, strings, units, sheet);
            notes(ui, strings, &sheet.model);
        },
    );
    ui.add_space(16.0);
    footer(ui, strings, &sheet.model, out);
}

/// The 32-px header: the title and `x`.
fn header(ui: &mut Ui, strings: &Strings, purpose: Purpose, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let title = match purpose {
        Purpose::Import => StrKey::AppPictureTitle,
        Purpose::New => StrKey::AppPictureTitleNew,
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), HEADER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let close = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::X, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::AppImportClose));
            if button(ui, close).clicked() {
                out.push(Action::PictureSheet(Request::Close));
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
            strings.get(StrKey::AppPictureFileHelp),
            None,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let choose = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppImportChoose));
            if button(ui, choose).clicked() {
                out.push(Action::PictureSheet(Request::Choose));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // Cut at its start: the file's name is at its end.
                row::path(ui, &sheet.path.display().to_string());
            });
        });
    });
}

/// `PNG · 64 × 32 px`, or that the file could not be read.
fn summary(ui: &mut Ui, strings: &Strings, model: &PictureImport) {
    let palette = Palette::current(ui);
    let format = strings.get(match model.format() {
        ImageFormat::Png => StrKey::AppPicturePng,
        ImageFormat::Jpeg => StrKey::AppPictureJpeg,
    });
    let line = match model.size_px() {
        Some((width_px, height_px)) => strings.fmt(
            StrKey::AppPictureSummary,
            &[
                ("format", format),
                ("width", &width_px.to_string()),
                ("height", &height_px.to_string()),
            ],
        ),
        None => strings.fmt(StrKey::AppImportSummaryUnread, &[("format", format)]),
    };
    wrapped(ui, &line, TextStyle::Body, palette.text_secondary);
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

/// The picture as the bitmap will store it, fitted to the thumbnail's
/// well: top row at the top, as on screen and as the Viewport draws it.
fn preview(ui: &mut Ui, sheet: &Sheet) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), PREVIEW_PX), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::INNER_CARD_RADIUS_PX, palette.viewport_bg);
    painter.rect_stroke(
        rect,
        theme::INNER_CARD_RADIUS_PX,
        Stroke::new(1.0, palette.border_default),
        StrokeKind::Inside,
    );
    let (Some(texture), Some((width_px, height_px))) = (&sheet.thumbnail, sheet.model.size_px())
    else {
        return;
    };
    let room = rect.shrink(PREVIEW_MARGIN_PX);
    #[allow(
        clippy::cast_precision_loss,
        reason = "a picture's side, at most 8192 px"
    )]
    let (w, h) = (width_px.max(1) as f32, height_px.max(1) as f32);
    let scale = (room.width() / w).min(room.height() / h);
    let fitted = egui::Rect::from_center_size(room.center(), egui::vec2(w * scale, h * scale));
    painter.image(
        texture.id(),
        fitted,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        theme::IMAGE_TINT,
    );
}

/// The size: *Width* and *Height* in project units. The one typed is the
/// model's; the other shows what follows from the pixels. Under them, why
/// the picture cannot be added yet.
fn size(ui: &mut Ui, strings: &Strings, units: Units, sheet: &mut Sheet) {
    let footprint = sheet.model.footprint_mm();
    let readable = sheet.model.pixels().is_some();
    let mut middles = [0.0_f32; 2];
    for (side, middle) in [PictureSide::Width, PictureSide::Height]
        .into_iter()
        .zip(&mut middles)
    {
        let (key, name) = match side {
            PictureSide::Width => (StrKey::AppPictureWidth, "width"),
            PictureSide::Height => (StrKey::AppPictureHeight, "height"),
        };
        // The footprint's side once there is one; else what was typed in
        // this field, so a refused size stays in view.
        let value_mm = footprint
            .map(|(width_mm, height_mm)| match side {
                PictureSide::Width => width_mm,
                PictureSide::Height => height_mm,
            })
            .or_else(|| {
                (sheet.model.side() == side)
                    .then(|| sheet.model.size())
                    .flatten()
                    .map(|size| match size {
                        dc_job::picture::PictureSize::Width { width_mm } => width_mm,
                        dc_job::picture::PictureSize::Height { height_mm } => height_mm,
                    })
            });
        let value = value_mm.map_or_else(String::new, |mm| dc_app::units::field_text(mm, units));
        let refused = sheet
            .refused
            .as_ref()
            .filter(|(at, _)| *at == side)
            .map(|(_, reason)| reason.clone());
        let label = strings.get(key);
        let (label_response, output) = field_row(ui, label, |ui| {
            ui.set_max_width((ui.available_width() - LINK_COLUMN_PX).max(0.0));
            let output = unit_field(
                ui,
                UnitField {
                    enabled: readable,
                    tier: refused.as_ref().map(|_| Tier::Error),
                    ..UnitField::new(
                        egui::Id::unique(("add_picture_size", name)),
                        label,
                        &value,
                        dc_app::units::suffix(units),
                    )
                },
            );
            if let Some(reason) = &refused {
                crate::widgets::unit_field::validation_line(ui, reason);
            }
            output
        });
        *middle = label_response.rect.center().y;
        let _ = output.response.labelled_by(label_response.id);
        if let Some(typed) = output.committed {
            commit(sheet, side, &typed, units);
            // The other side follows in the next frame: draw it now.
            ui.ctx().request_repaint();
        }
        ui.add_space(4.0);
    }
    let [w_y, h_y] = middles;
    closed_link(
        ui,
        (w_y, h_y),
        strings.get(StrKey::AppPictureProportions),
        readable,
    );
    // Why *Add picture* is disabled: a hint until a size is typed, an
    // error for a file or a size that is refused.
    if let Some(reason) = sheet.model.blocked(strings) {
        if readable && sheet.model.size().is_none() {
            row::hint(ui, &reason);
        } else if sheet.refused.is_none() {
            row::message(ui, Tier::Error, &reason);
        }
    }
}

/// What was typed in the `side` field: a length sets that side, and the
/// other follows; an emptied field forgets the size when it was this
/// side's; anything else is refused under the field.
fn commit(sheet: &mut Sheet, side: PictureSide, typed: &str, units: Units) {
    if typed.trim().is_empty() {
        if sheet.model.side() == side {
            sheet.model.clear_size();
        }
        sheet.refused = None;
        return;
    }
    match dc_app::units::parse_length_mm(typed, units) {
        Ok(mm) => {
            sheet.refused = None;
            match side {
                PictureSide::Width => sheet.model.set_width_mm(mm),
                PictureSide::Height => sheet.model.set_height_mm(mm),
            }
        }
        Err(e) => sheet.refused = Some((side, e.to_string())),
    }
}

/// What decoding converted, as the Warnings panel will list it.
fn notes(ui: &mut Ui, strings: &Strings, model: &PictureImport) {
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
            let glyph =
                Icon::TriangleAlert.galley(ui, theme::ICON_SMALL_PX, palette.status_warning.stroke);
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

/// The footer: the one primary action, a pill, disabled with the reason
/// until the picture can be added.
fn footer(ui: &mut Ui, strings: &Strings, model: &PictureImport, out: &mut Vec<Action>) {
    let label = strings.get(match model.purpose() {
        Purpose::Import => StrKey::AppPictureAdd,
        Purpose::New => StrKey::AppImportCreate,
    });
    let reason = model.blocked(strings);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let mut spec = ButtonSpec::new(Kind::Primary, label)
                .height(FOOTER_PX)
                .enabled(reason.is_none());
            if let Some(reason) = reason.as_deref() {
                spec = spec.tooltip(reason);
            }
            if button(ui, spec).clicked() {
                out.push(Action::PictureSheet(Request::Confirm));
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

    /// The thumbnail keeps the picture's shape and its rows' order: the
    /// top row first, every `step`-th sample, no side above the limit.
    #[test]
    fn thumbnail_thins_a_large_picture_and_keeps_its_rows_in_order() {
        let small = Grayscale {
            width_px: 3,
            height_px: 2,
            bytes: vec![0, 10, 20, 30, 40, 50],
        };
        let image = thumbnail(&small);
        assert_eq!(image.size, [3, 2]);
        let gray: Vec<u8> = image.pixels.iter().map(|p| p.r()).collect();
        assert_eq!(gray, [0, 10, 20, 30, 40, 50], "top row first, as stored");

        let (width_px, height_px) = (2000_u32, 1000_u32);
        let large = Grayscale {
            width_px,
            height_px,
            bytes: (0..width_px * height_px)
                .map(|k| u8::try_from((k / width_px) % 251).unwrap_or(0))
                .collect(),
        };
        let image = thumbnail(&large);
        assert_eq!(image.size, [500, 250], "every fourth sample");
        assert_eq!(image.pixels.first().map(|p| p.r()), Some(0));
        assert_eq!(
            image.pixels.get(500).map(|p| p.r()),
            Some(4),
            "the second row kept is the picture's fifth"
        );
    }
}
