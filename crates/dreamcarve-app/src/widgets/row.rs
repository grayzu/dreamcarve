// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! An editor row's parts (`docs/specs/phase-4/experience.md`, sections 3
//! and 7): the 112-px label column whose hover shows the field's help
//! with its `.dcv` field name in JetBrains Mono ("Help affordance"), and
//! the inline validation message under the field: glyph then the file's
//! message, 12 px, in the status's text colour ("Inline validation
//! message"). Every state has an outline, a glyph and a message, never
//! colour alone (section 13, item 7).

use dc_job::Tier;
use eframe::egui::{self, Response, RichText, Sense, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;
use crate::widgets::unit_field::HEIGHT_PX;

/// The label column's width, px (section 3).
pub(crate) const LABEL_PX: f32 = 112.0;
/// The gap between a label and its field, px.
pub(crate) const GAP_PX: f32 = 8.0;

/// A field's label, `text`, 112 px wide and a control tall; hovering it
/// shows `help` and, on a second line in the mono face, `path`, the
/// `.dcv` field it writes. Its dotted underline appears on hover to say it
/// has help.
pub(crate) fn label(ui: &mut Ui, text: &str, help: &str, path: Option<&str>) -> Response {
    let palette = Palette::current(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(LABEL_PX, HEIGHT_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    let colour = if ui.is_enabled() {
        palette.text_primary
    } else {
        palette.text_disabled
    };
    let painter = ui.painter_at(rect);
    // One line, an ellipsis when it does not fit (section 12, "Text
    // truncation"); the tooltip's help names the field in full.
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        TextStyle::Body.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 4.0);
    let galley = painter.layout_job(job);
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x.min(rect.width());
    painter.galley(at, galley, colour);
    if response.hovered() {
        let y = rect.center().y + 8.0;
        painter.extend(egui::Shape::dashed_line(
            &[
                egui::pos2(rect.left(), y),
                egui::pos2(rect.left() + width, y),
            ],
            Stroke::new(1.0, palette.text_disabled),
            1.0,
            2.0,
        ));
    }
    response.on_hover_ui(|ui| {
        ui.label(RichText::new(help).text_style(TextStyle::Body));
        if let Some(path) = path {
            ui.label(
                RichText::new(path)
                    .text_style(TextStyle::Monospace)
                    .color(palette.text_secondary),
            );
        }
    })
}

/// The validation message under a field at the label column's right,
/// `tier`'s glyph then `text`, wrapped to the field's width.
pub(crate) fn message(ui: &mut Ui, tier: Tier, text: &str) {
    let palette = Palette::current(ui);
    let (icon, colours) = match tier {
        Tier::Error => (Icon::CircleX, palette.status_error),
        Tier::Warning => (Icon::TriangleAlert, palette.status_warning),
        Tier::Hint => (Icon::Info, palette.status_info),
    };
    note(ui, icon, colours.stroke, colours.text, text);
}

/// A note under a row in text.secondary: what a switch kept, why an
/// option is missing.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    let palette = Palette::current(ui);
    note(
        ui,
        Icon::Info,
        palette.text_secondary,
        palette.text_secondary,
        text,
    );
}

/// `icon` in `glyph` and `text` in `colour`, indented to the field column.
fn note(ui: &mut Ui, icon: Icon, glyph: egui::Color32, colour: egui::Color32, text: &str) {
    ui.horizontal_top(|ui| {
        ui.add_space(LABEL_PX + GAP_PX);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
            Sense::hover(),
        );
        let galley = icon.galley(ui, theme::ICON_SMALL_PX, glyph);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, glyph);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .text_style(TextStyle::Small)
                    .color(colour),
            )
            .wrap()
            .selectable(false),
        );
    });
}
