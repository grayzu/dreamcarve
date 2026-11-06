// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The segmented control (`docs/specs/phase-4/experience.md`, section 7,
//! "Segmented control"): two to four exclusive options in one
//! `border.strong` outline, 26 px; the chosen segment `surface.raised`
//! with text.primary Medium and its own 1-px outline, the rest
//! text.secondary. The app has no radio buttons.

use eframe::egui::{self, Align2, Id, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::button::focus_ring;
use crate::widgets::unit_field::HEIGHT_PX;

/// Draws `options` across `width_px` with `selected` chosen; the option
/// clicked this frame, if another.
pub(crate) fn segmented(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    width_px: f32,
) -> Option<usize> {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width_px, HEIGHT_PX), Sense::hover());
    let count = options.len().max(1);
    #[allow(
        clippy::cast_precision_loss,
        reason = "two to four segments, exact in f32"
    )]
    let segment_px = rect.width() / count as f32;
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        4.0,
        palette.surface_sunken,
        Stroke::new(1.0, palette.border_strong),
        StrokeKind::Inside,
    );
    let mut clicked = None;
    let mut left = rect.left();
    for (n, option) in options.iter().enumerate() {
        let segment = Rect::from_min_size(
            egui::pos2(left, rect.top()),
            egui::vec2(segment_px, HEIGHT_PX),
        );
        left += segment_px;
        let response = ui.interact(segment, id.with(n), Sense::click());
        let chosen = n == selected;
        let enabled = ui.is_enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, chosen, *option)
        });
        if response.clicked() && !chosen {
            clicked = Some(n);
        }
        let (style, colour) = if chosen {
            painter.rect(
                segment.shrink(1.0),
                3.0,
                palette.surface_raised,
                Stroke::new(1.0, palette.border_strong),
                StrokeKind::Inside,
            );
            (
                TextStyle::Name(theme::MEDIUM_STYLE.into()),
                palette.text_primary,
            )
        } else {
            if response.hovered() {
                painter.rect_filled(segment.shrink(1.0), 3.0, palette.surface_hover);
            }
            (TextStyle::Body, palette.text_secondary)
        };
        painter.text(
            segment.center(),
            Align2::CENTER_CENTER,
            option,
            style.resolve(ui.style()),
            colour,
        );
        focus_ring(ui, &response);
    }
    clicked
}
