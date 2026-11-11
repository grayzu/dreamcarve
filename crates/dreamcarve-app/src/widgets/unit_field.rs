// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The text field with a trailing unit (`docs/specs/phase-4/experience.md`,
//! section 7, "Text field with trailing unit", "Tool-derived placeholder",
//! "Numeric entry", "Inline validation message"; section 12,
//! `unit_field`): 26 px, the value left in tabular figures, the unit
//! right-aligned in text.secondary inside the outline. The text the user
//! types lives in egui's memory while the field has focus; it commits on
//! leaving the field (Tab, Enter, a click elsewhere) and Escape restores
//! the value, so an edit is one undo step. The outline takes the
//! validation tier's colour; the message under the field is the caller's
//! ([`validation_line`] in a field column, [`crate::widgets::row::message`]
//! beside a label). A tool-derived field shows the tool's value as grey
//! placeholder text while empty, and a ⟲ inside its right end while it
//! holds a value of its own.
//!
//! One form for every panel: the Project panel (#536), the Tools tab
//! (#537), the placement fields (#534) and the operation editor (#539).
//!
//! An unset value the file needs before export (functional spec, 66b;
//! section 7, "required (unset)"; slice 26, #618) is [`UnitField::required`]:
//! the error outline, `circle-x` inside the field before the unit,
//! *Required* as the placeholder, and the file's message as the tooltip,
//! with no message line under it.

use dc_job::Tier;
use eframe::egui::{
    self, Align, Align2, Id, Key, Layout, Margin, Rect, Response, Sense, Stroke, StrokeKind,
    TextEdit, TextStyle, Ui, UiBuilder,
};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// A control's height, px (section 3).
pub(crate) const HEIGHT_PX: f32 = 26.0;
/// The unit's share of the field's right end, px (section 3).
const UNIT_PX: f32 = 32.0;
/// The reset button's square, px.
const RESET_PX: f32 = 20.0;
/// Inner padding, px.
const PADDING_PX: f32 = 8.0;
/// The label column of an editor row, px (section 3).
pub const LABEL_PX: f32 = 112.0;

/// A field to draw.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UnitField<'a> {
    /// The field's id, stable across frames.
    pub id: Id,
    /// Its accessible name: the row's label.
    pub name: &'a str,
    /// The project's value as text, with no unit; empty for `null`.
    pub value: &'a str,
    /// The unit drawn after the value; empty for none.
    pub unit: &'a str,
    /// Shown in text.disabled while the field is empty (`tool's: 3 mm`).
    pub placeholder: &'a str,
    /// Whether it can be typed into.
    pub enabled: bool,
    /// Why not, shown on hover when disabled.
    pub disabled_reason: Option<&'a str>,
    /// The validation state, which colours the outline.
    pub tier: Option<Tier>,
    /// The ⟲ button's tooltip, when the field shows one (a tool-derived
    /// field holding a value of its own).
    pub reset: Option<&'a str>,
    /// The required-field look (functional 66b), while the value is unset
    /// and the file needs it: the placeholder (*Required*) and the file's
    /// message, its tooltip. The caller draws no message under the field.
    pub required: Option<Required<'a>>,
}

/// An unset required value's placeholder and message.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Required<'a> {
    /// *Required*, from the string table.
    pub placeholder: &'a str,
    /// The file's message (`tools[t1].feed_mm_min is not set…`).
    pub message: &'a str,
}

impl<'a> UnitField<'a> {
    /// An enabled field `id` named `name`, showing `value` in `unit`, with
    /// no placeholder, tier or reset.
    #[must_use]
    pub(crate) const fn new(id: Id, name: &'a str, value: &'a str, unit: &'a str) -> Self {
        Self {
            id,
            name,
            value,
            unit,
            placeholder: "",
            enabled: true,
            disabled_reason: None,
            tier: None,
            reset: None,
            required: None,
        }
    }
}

/// What a field did this frame.
#[derive(Debug)]
pub(crate) struct FieldOutput {
    /// The text edit's response.
    pub response: Response,
    /// The typed text, once, when the user leaves a field they changed.
    pub committed: Option<String>,
    /// Whether ⟲ was clicked.
    pub reset: bool,
}

/// Draws `spec` across the available width.
pub(crate) fn field(ui: &mut Ui, spec: UnitField<'_>) -> FieldOutput {
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
    let reset_px = if spec.reset.is_some() { RESET_PX } else { 0.0 };
    // The required look's glyph, before the unit (section 7).
    let glyph_px = if spec.required.is_some() {
        theme::ICON_SMALL_PX + 4.0
    } else {
        0.0
    };
    let tier = spec.required.map_or(spec.tier, |_| Some(Tier::Error));
    let placeholder_text = spec.required.map_or(spec.placeholder, |r| r.placeholder);
    // One line of text, centred in the 26-px control.
    let line_px = ui.text_style_height(&TextStyle::Body);
    let text_rect = Rect::from_min_max(
        egui::pos2(rect.left() + PADDING_PX, rect.center().y - line_px / 2.0),
        egui::pos2(
            rect.right() - PADDING_PX - unit_px - reset_px - glyph_px,
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
    let placeholder = egui::RichText::new(placeholder_text.to_owned()).color(palette.text_disabled);
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

    let mut reset = false;
    if let Some(tooltip) = spec.reset {
        let at = Rect::from_center_size(
            egui::pos2(
                rect.right() - PADDING_PX - unit_px - RESET_PX / 2.0,
                rect.center().y,
            ),
            egui::vec2(RESET_PX, RESET_PX),
        );
        let button = ui.interact(at, spec.id.with("reset"), Sense::click());
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
        if button.hovered() {
            ui.painter().rect_filled(at, 3.0, palette.surface_hover);
        }
        let glyph = Icon::RotateCcw.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
        ui.painter().galley(
            at.center() - glyph.size() / 2.0,
            glyph,
            palette.text_secondary,
        );
        reset = button.on_hover_text(tooltip).clicked();
    }

    if ui.is_rect_visible(rect) {
        let hovered = spec.enabled && (frame_response.hovered() || response.hovered());
        let outline = match tier {
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
        if spec.required.is_some() {
            let glyph = Icon::CircleX.galley(ui, theme::ICON_SMALL_PX, palette.status_error.stroke);
            let right = rect.right() - PADDING_PX - unit_px - reset_px;
            ui.painter().galley(
                egui::pos2(
                    right - glyph.size().x - 2.0,
                    rect.center().y - glyph.size().y / 2.0,
                ),
                glyph,
                palette.status_error.stroke,
            );
        }
    }
    let response = match (spec.enabled, spec.disabled_reason, spec.required) {
        (false, Some(reason), _) => {
            frame_response.on_hover_text(reason);
            response.on_disabled_hover_text(reason)
        }
        (true, _, Some(required)) => {
            frame_response.on_hover_text(required.message);
            response.on_hover_text(required.message)
        }
        _ => response,
    };
    FieldOutput {
        response,
        committed,
        reset,
    }
}

/// The 12-px message under a field in its column, in the error state:
/// `circle-x`, then the file's message verbatim, wrapped to the field.
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
