// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The switch (`docs/specs/phase-4/experience.md`, section 7, "Toggle";
//! section 12, `toggle`): a 36 × 20 pill, `surface.sunken` with a
//! `border.strong` outline when off, `accent.fill` when on, for an on/off
//! feature whose sub-fields dim when it is off, such as arc fitting's
//! "off" (`null`). egui has no switch, so it is painted.

use eframe::egui::{self, Response, Sense, Stroke, StrokeKind, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::button::focus_ring_at;

/// The switch's size, px.
const SIZE: egui::Vec2 = egui::vec2(36.0, 20.0);
/// The gap between the track's edge and the knob, px.
const KNOB_INSET_PX: f32 = 3.0;

/// Draws a switch that is `on`, named `name`; `clicked` is the flip.
pub(crate) fn toggle(ui: &mut Ui, on: bool, name: &str) -> Response {
    let palette = Palette::current(ui);
    // Centred in a control-high row.
    let (row, _) = ui.allocate_exact_size(egui::vec2(SIZE.x, theme::CONTROL_PX), Sense::hover());
    let rect = egui::Rect::from_center_size(row.center(), SIZE);
    let response = ui.interact(rect, ui.scope_id().with(("toggle", name)), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, on, name));
    let painter = ui.painter();
    let radius = theme::pill(SIZE.y);
    let (fill, stroke, knob) = match (on, enabled) {
        (true, true) if response.hovered() => (
            palette.accent_hover,
            palette.accent_hover,
            palette.text_on_accent,
        ),
        (true, true) => (
            palette.accent_fill,
            palette.accent_fill,
            palette.text_on_accent,
        ),
        (true, false) => (
            palette.text_disabled,
            palette.text_disabled,
            palette.surface_panel,
        ),
        (false, true) => (
            palette.surface_sunken,
            palette.border_strong,
            palette.text_secondary,
        ),
        (false, false) => (
            palette.surface_sunken,
            palette.border_default,
            palette.text_disabled,
        ),
    };
    painter.rect(
        rect,
        radius,
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    let knob_radius = radius - KNOB_INSET_PX;
    let x = if on {
        rect.right() - radius
    } else {
        rect.left() + radius
    };
    painter.circle_filled(egui::pos2(x, rect.center().y), knob_radius, knob);
    focus_ring_at(ui, &response, radius);
    response
}
