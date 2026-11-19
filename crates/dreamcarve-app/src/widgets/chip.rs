// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The chip (`docs/specs/phase-4/redesign.md`, "Components"): a 24-px
//! pill with 12-px text, for a count, an operation's kind, a finding's
//! tier. A neutral chip is `surface.raised` with a hairline, so it reads
//! on a card of the same colour; a status chip adds its glyph and takes
//! its status tint as fill. A chip is a label, never a button.

use std::sync::Arc;

use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette, StatusColours};
use crate::widgets::icon::Icon;

/// The space left and right of a chip's content, px.
const PADDING_PX: f32 = 10.0;
/// A chip's glyph, px.
const GLYPH_PX: f32 = 14.0;
/// The gap between a chip's glyph and its text, px.
const GLYPH_GAP_PX: f32 = 4.0;

/// What a chip says with its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// `surface.raised`, text.secondary.
    Neutral,
    /// The status's tint, its glyph, and its text colour.
    Status(StatusColours, Icon),
}

/// A chip laid out and not yet placed.
#[derive(Debug, Clone)]
pub(crate) struct Chip {
    text: Arc<egui::Galley>,
    tone: Tone,
}

impl Chip {
    /// A chip reading `text` in `tone`.
    #[must_use]
    pub(crate) fn new(ui: &Ui, text: &str, tone: Tone) -> Self {
        let palette = Palette::current(ui);
        let colour = match tone {
            Tone::Neutral => palette.text_secondary,
            Tone::Status(status, _) => status.text,
        };
        Self {
            text: ui.painter().layout_no_wrap(
                text.to_owned(),
                TextStyle::Small.resolve(ui.style()),
                colour,
            ),
            tone,
        }
    }

    /// The room its glyph takes, gap included, px.
    const fn glyph_px(&self) -> f32 {
        match self.tone {
            Tone::Neutral => 0.0,
            Tone::Status(..) => GLYPH_PX + GLYPH_GAP_PX,
        }
    }

    /// Its size, px.
    #[must_use]
    pub(crate) fn size(&self) -> egui::Vec2 {
        egui::vec2(
            self.text.size().x + self.glyph_px() + 2.0 * PADDING_PX,
            theme::CHIP_PX,
        )
    }

    /// Paints it in `rect`, a rectangle of its [`Chip::size`].
    pub(crate) fn paint(self, ui: &Ui, rect: Rect) {
        let palette = Palette::current(ui);
        let radius = theme::pill(rect.height());
        let mut x = rect.left() + PADDING_PX;
        let colour = match self.tone {
            Tone::Neutral => {
                ui.painter().rect(
                    rect,
                    radius,
                    palette.surface_raised,
                    Stroke::new(1.0, palette.border_default),
                    StrokeKind::Inside,
                );
                palette.text_secondary
            }
            Tone::Status(status, icon) => {
                ui.painter().rect_filled(rect, radius, status.tint);
                let mark = icon.galley(ui, GLYPH_PX, status.stroke);
                ui.painter().galley(
                    egui::pos2(x, rect.center().y - mark.size().y / 2.0),
                    mark,
                    status.stroke,
                );
                x += self.glyph_px();
                status.text
            }
        };
        ui.painter().galley(
            egui::pos2(x, rect.center().y - self.text.size().y / 2.0),
            self.text,
            colour,
        );
    }
}

/// Draws a chip reading `text` where the layout puts it.
pub(crate) fn chip(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    let chip = Chip::new(ui, text, tone);
    let (rect, response) = ui.allocate_exact_size(chip.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    if ui.is_rect_visible(rect) {
        chip.paint(ui, rect);
    }
    response
}
