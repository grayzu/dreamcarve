// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The text field with a trailing unit (`docs/specs/phase-4/experience.md`,
//! section 7, "Text field with trailing unit", "Numeric entry", "Inline
//! validation message"; section 12, `unit_field`): 26 px, the value left
//! in tabular figures, the unit right-aligned in text.secondary inside
//! the outline. The text the user types lives in egui's memory while the
//! field has focus; it commits on leaving the field (Tab, Enter, a click
//! elsewhere) and Escape restores the value, so an edit is one undo step.
//! In the error state the outline turns status.error and the file's
//! message sits under the field with `circle-x`.

use eframe::egui::{self, Id, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::Palette;
use crate::widgets::icon::Icon;

/// A field's height, px.
const HEIGHT_PX: f32 = 26.0;
/// The unit's share of the field's right end, px.
const UNIT_PX: f32 = 32.0;
/// The label column of an editor row, px (section 3).
pub const LABEL_PX: f32 = 112.0;

/// A field to draw.
#[derive(Debug, Clone, Copy)]
pub struct FieldSpec<'a> {
    /// The field's identity, stable across frames.
    pub id: Id,
    /// The value as the project holds it, formatted; empty for `null`.
    pub text: &'a str,
    /// The unit after the value; empty for none.
    pub unit: &'a str,
    /// Shown in text.disabled while the field is empty.
    pub hint: &'a str,
    /// The file's message when the field blocks export.
    pub error: Option<&'a str>,
}

/// What a field did this frame.
#[derive(Debug)]
pub struct FieldOutput {
    /// The text edit's response.
    pub response: Response,
    /// The typed text, once, when the user leaves a field they changed.
    pub committed: Option<String>,
}

/// Draws `spec` at the available width.
pub fn unit_field(ui: &mut Ui, spec: FieldSpec<'_>) -> FieldOutput {
    let palette = Palette::current(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT_PX), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(spec.id));
    let mut buffer = if focused {
        ui.data(|d| d.get_temp::<String>(spec.id))
            .unwrap_or_else(|| spec.text.to_owned())
    } else {
        spec.text.to_owned()
    };
    let unit_w = if spec.unit.is_empty() { 0.0 } else { UNIT_PX };
    let inner = egui::Rect::from_min_max(
        rect.min + egui::vec2(8.0, 0.0),
        rect.max - egui::vec2(8.0 + unit_w, 0.0),
    );
    let response = ui.put(
        inner,
        egui::TextEdit::singleline(&mut buffer)
            .id(spec.id)
            .frame(egui::Frame::NONE)
            .font(TextStyle::Body)
            .hint_text(
                egui::RichText::new(spec.hint)
                    .text_style(TextStyle::Body)
                    .color(palette.text_disabled),
            )
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(inner.width()),
    );
    let mut committed = None;
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(spec.id, buffer.clone()));
    } else if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(spec.id));
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if !escaped && buffer != spec.text {
            committed = Some(buffer);
        }
    }

    let stroke = if spec.error.is_some() {
        palette.status_error.stroke
    } else if response.has_focus() {
        palette.accent
    } else if response.hovered() {
        palette.text_secondary
    } else {
        palette.border_strong
    };
    let painter = ui.painter();
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    if !spec.unit.is_empty() {
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            spec.unit,
            TextStyle::Body.resolve(ui.style()),
            palette.text_secondary,
        );
    }
    if let Some(message) = spec.error {
        validation_line(ui, message);
    }
    FieldOutput {
        response,
        committed,
    }
}

/// The 12-px message under a field in the error state: `circle-x`, then
/// the file's message verbatim, wrapped to the field.
pub fn validation_line(ui: &mut Ui, message: &str) {
    let palette = Palette::current(ui);
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let glyph = Icon::CircleX.galley(ui, 12.0, palette.status_error.text);
        let (rect, _) = ui.allocate_exact_size(glyph.size(), Sense::hover());
        ui.painter()
            .galley(rect.min, glyph, palette.status_error.text);
        ui.add(
            egui::Label::new(
                egui::RichText::new(message)
                    .text_style(TextStyle::Small)
                    .color(palette.status_error.text),
            )
            .wrap()
            .selectable(false),
        );
    });
    ui.add_space(4.0);
}

/// An editor row: the label in its 112-px column, then `field` at the
/// rest of the width. The label names the field for assistive tools.
pub fn row<R>(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    let palette = Palette::current(ui);
    ui.horizontal_top(|ui| {
        let (rect, label_response) =
            ui.allocate_exact_size(egui::vec2(LABEL_PX, HEIGHT_PX), Sense::hover());
        let galley = ui.painter().layout(
            label.to_owned(),
            TextStyle::Body.resolve(ui.style()),
            palette.text_secondary,
            LABEL_PX,
        );
        ui.painter().galley(
            egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text_secondary,
        );
        label_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
        let inner = ui.vertical(|ui| field(ui)).inner;
        (label_response, inner)
    })
    .inner
}
