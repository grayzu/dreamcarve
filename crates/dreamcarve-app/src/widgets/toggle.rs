// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The switch (`docs/specs/phase-4/experience.md`, section 7, "Toggle";
//! section 12, `toggle`): 28 × 16, `border.strong` track, `accent` when
//! on, for an on/off feature whose sub-fields dim when it is off, such as
//! arc fitting's "off" (`null`). egui has no switch, so it is painted.

use eframe::egui::{self, Response, Sense, Stroke, StrokeKind, Ui};

use crate::shell::theme::Palette;
use crate::widgets::button::focus_ring;

/// The switch's size, px.
const SIZE: egui::Vec2 = egui::vec2(28.0, 16.0);

/// Draws a switch that is `on`, named `name`; `clicked` is the flip.
pub(crate) fn toggle(ui: &mut Ui, on: bool, name: &str) -> Response {
    let palette = Palette::current(ui);
    // Centred in a 26-px row.
    let (row, _) = ui.allocate_exact_size(egui::vec2(SIZE.x, 26.0), Sense::hover());
    let rect = egui::Rect::from_center_size(row.center(), SIZE);
    let response = ui.interact(rect, ui.scope_id().with(("toggle", name)), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, on, name));
    let painter = ui.painter();
    let (fill, stroke) = if on {
        (palette.accent, palette.accent)
    } else {
        (palette.surface_sunken, palette.border_strong)
    };
    painter.rect(
        rect,
        SIZE.y / 2.0,
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    let radius = SIZE.y / 2.0 - 3.0;
    let x = if on {
        rect.right() - SIZE.y / 2.0
    } else {
        rect.left() + SIZE.y / 2.0
    };
    let knob = if on {
        palette.text_on_accent
    } else {
        palette.text_secondary
    };
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, knob);
    focus_ring(ui, &response);
    response
}
