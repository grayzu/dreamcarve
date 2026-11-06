// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The text field with a trailing unit (`docs/specs/phase-4/experience.md`,
//! section 7, "Text field with trailing unit"; section 12, `unit_field`):
//! 26 px, the value left in tabular figures, the unit right in
//! text.secondary inside the outline. It shows the project's value until
//! it has focus; while focused the typed text lives in egui's memory, not
//! the project (design, "The shell": widgets hold no project data). It
//! commits on leave (Tab, Enter, a click elsewhere) and Escape restores
//! the value, so one leave is at most one undo step (design, "Units").

use dc_job::Tier;
use eframe::egui::{
    self, Align, Align2, Id, Key, Layout, Margin, Rect, Sense, Stroke, StrokeKind, TextEdit,
    TextStyle, Ui, UiBuilder,
};

use crate::shell::theme::Palette;

/// A control's height, px (section 3).
pub(crate) const HEIGHT_PX: f32 = 26.0;
/// The room the unit takes at the field's right end, px (section 3).
const UNIT_PX: f32 = 32.0;
/// Inner padding, px.
const PADDING_PX: f32 = 8.0;

/// A field to draw.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UnitField<'a> {
    /// The field's id, stable across frames.
    pub id: Id,
    /// Its accessible name: the row's label.
    pub name: &'a str,
    /// The project's value as text, with no unit.
    pub value: &'a str,
    /// The unit drawn after the value; empty for none.
    pub unit: &'a str,
    /// Shown, dimmed, when the field is empty.
    pub placeholder: &'a str,
    /// Whether it can be typed into.
    pub enabled: bool,
    /// Why not, shown on hover when disabled.
    pub disabled_reason: Option<&'a str>,
    /// The validation state, which colours the outline.
    pub tier: Option<Tier>,
}

/// Draws `spec` across the available width; the text typed when the user
/// leaves the field changed, else `None`.
pub(crate) fn unit_field(ui: &mut Ui, spec: UnitField<'_>) -> Option<String> {
    let palette = Palette::current(ui);
    let (rect, frame_response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT_PX), Sense::hover());
    let buffer_id = spec.id.with("typed");
    let had_focus = ui.memory(|m| m.has_focus(spec.id));
    let mut text = if had_focus {
        ui.data(|d| d.get_temp::<String>(buffer_id))
            .unwrap_or_else(|| spec.value.to_owned())
    } else {
        spec.value.to_owned()
    };

    // The frame is painted under the text, into a slot taken before it.
    let background = ui.painter().add(egui::Shape::Noop);
    let unit_px = if spec.unit.is_empty() { 0.0 } else { UNIT_PX };
    // One line of text, centred in the 26-px control.
    let line_px = ui.text_style_height(&TextStyle::Body);
    let text_rect = Rect::from_min_max(
        egui::pos2(rect.left() + PADDING_PX, rect.center().y - line_px / 2.0),
        egui::pos2(
            rect.right() - PADDING_PX - unit_px,
            rect.center().y + line_px / 2.0,
        ),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if !spec.enabled {
        child.disable();
    }
    let placeholder = egui::RichText::new(spec.placeholder.to_owned()).color(palette.text_disabled);
    let response = child
        .add(
            TextEdit::singleline(&mut text)
                .id(spec.id)
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .font(TextStyle::Body)
                .hint_text(placeholder)
                .desired_width(text_rect.width()),
        )
        .accessible_name(spec.name);

    let escaped = ui.input(|i| i.key_pressed(Key::Escape));
    let mut committed = None;
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(buffer_id, text.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(buffer_id));
        if !escaped && text != spec.value {
            committed = Some(text);
        }
    }

    if ui.is_rect_visible(rect) {
        let hovered = spec.enabled && (frame_response.hovered() || response.hovered());
        let outline = match spec.tier {
            Some(Tier::Error) => palette.status_error.stroke,
            Some(Tier::Warning) => palette.status_warning.stroke,
            _ if !spec.enabled => palette.border_default,
            _ if response.has_focus() => palette.accent,
            _ if hovered => palette.text_secondary,
            _ => palette.border_strong,
        };
        let painter = ui.painter();
        painter.set(
            background,
            egui::epaint::RectShape::new(
                rect,
                4.0,
                palette.surface_sunken,
                Stroke::new(1.0, outline),
                StrokeKind::Inside,
            ),
        );
        if !spec.unit.is_empty() {
            let colour = if spec.enabled {
                palette.text_secondary
            } else {
                palette.text_disabled
            };
            painter.text(
                egui::pos2(rect.right() - PADDING_PX, rect.center().y),
                Align2::RIGHT_CENTER,
                spec.unit,
                TextStyle::Body.resolve(ui.style()),
                colour,
            );
        }
    }
    if let (false, Some(reason)) = (spec.enabled, spec.disabled_reason) {
        response.on_disabled_hover_text(reason);
        frame_response.on_hover_text(reason);
    }
    committed
}
