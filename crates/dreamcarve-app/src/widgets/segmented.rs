// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The segmented control and the pill tabs
//! (`docs/specs/phase-4/redesign.md`, "Components";
//! `docs/specs/phase-4/experience.md`, section 7, "Segmented control"):
//! two to four exclusive options on a `surface.sunken` track with fully
//! rounded ends, 32 px; the chosen segment a `surface.raised` pill with
//! text.primary Medium, the rest text.secondary. The app has no radio
//! buttons. [`tabs`] is the same control heard as a tab list.

use eframe::egui::{self, Align2, Id, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::button::focus_ring_at;

/// The gap between the track's edge and the chosen pill, px.
const INSET_PX: f32 = 3.0;

/// Draws `options` across `width_px` with `selected` chosen; the option
/// clicked this frame, if another.
pub(crate) fn segmented(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    width_px: f32,
) -> Option<usize> {
    track(ui, id, options, selected, width_px, egui::Role::RadioButton)
}

/// [`segmented`] as a tab list: each option is a tab to assistive tools.
pub(crate) fn tabs(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    width_px: f32,
) -> Option<usize> {
    track(ui, id, options, selected, width_px, egui::Role::Tab)
}

/// The track and its segments, each heard as `role`.
fn track(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    width_px: f32,
    role: egui::Role,
) -> Option<usize> {
    let palette = Palette::current(ui);
    let height = theme::CONTROL_PX;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width_px, height), Sense::hover());
    let count = options.len().max(1);
    #[allow(
        clippy::cast_precision_loss,
        reason = "two to four segments, exact in f32"
    )]
    let segment_px = (rect.width() - 2.0 * INSET_PX) / count as f32;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, theme::pill(height), palette.surface_sunken);
    let enabled = ui.is_enabled();
    let mut clicked = None;
    let mut left = rect.left() + INSET_PX;
    for (n, option) in options.iter().enumerate() {
        let segment = Rect::from_min_size(
            egui::pos2(left, rect.top() + INSET_PX),
            egui::vec2(segment_px, height - 2.0 * INSET_PX),
        );
        left += segment_px;
        let response = ui.interact(segment, id.with(n), Sense::click());
        let chosen = n == selected;
        response.widget_info(|| egui::WidgetInfo::selected(role, enabled, chosen, *option));
        if response.clicked() && !chosen {
            clicked = Some(n);
        }
        let radius = theme::pill(segment.height());
        let (style, colour) = if chosen {
            painter.rect(
                segment,
                radius,
                palette.surface_raised,
                Stroke::new(1.0, palette.border_default),
                StrokeKind::Inside,
            );
            (
                TextStyle::Name(theme::MEDIUM_STYLE.into()),
                if enabled {
                    palette.text_primary
                } else {
                    palette.text_disabled
                },
            )
        } else {
            if response.hovered() {
                painter.rect_filled(segment, radius, palette.surface_hover);
            }
            (
                TextStyle::Body,
                if enabled {
                    palette.text_secondary
                } else {
                    palette.text_disabled
                },
            )
        };
        // One line, an ellipsis when the segment is too narrow for it.
        let mut job = egui::text::LayoutJob::simple_singleline(
            (*option).to_owned(),
            style.resolve(ui.style()),
            colour,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(segment.width() - 8.0);
        let galley = painter.layout_job(job);
        painter.galley(
            Align2::CENTER_CENTER
                .anchor_size(segment.center(), galley.size())
                .min,
            galley,
            colour,
        );
        focus_ring_at(ui, &response, radius);
    }
    clicked
}
