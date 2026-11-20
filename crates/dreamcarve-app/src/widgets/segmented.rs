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
    segmented_sized(
        ui,
        id,
        options,
        selected,
        egui::vec2(width_px, theme::CONTROL_PX),
    )
}

/// [`segmented`] at `size`: a control shorter than the 32-px one, for a
/// floating pill on the Viewport.
pub(crate) fn segmented_sized(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    size: egui::Vec2,
) -> Option<usize> {
    track(ui, id, options, selected, size, egui::Role::RadioButton)
}

/// The least room a segment keeps each side of its label, px.
const LABEL_PAD_PX: f32 = 6.0;

/// The widths of the segments whose labels are `labels` px wide, across
/// `room` px: equal, where every label fits an equal share with its
/// padding; else each its label's width and an equal share of what is
/// left, so a long label (*Plunge* beside *Ramp* and *Helix* in a narrow
/// card) is not cut while a short one has room to spare.
fn segment_widths(labels: &[f32], room: f32) -> Vec<f32> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "two to four segments, exact in f32"
    )]
    let count = labels.len().max(1) as f32;
    let equal = room / count;
    let widest = labels.iter().copied().fold(0.0, f32::max);
    if widest + 2.0 * LABEL_PAD_PX <= equal {
        return vec![equal; labels.len()];
    }
    let spare = (room - labels.iter().sum::<f32>()) / count;
    labels.iter().map(|label| label + spare).collect()
}

/// The track and its segments, each heard as `role`.
fn track(
    ui: &mut Ui,
    id: Id,
    options: &[&str],
    selected: usize,
    size: egui::Vec2,
    role: egui::Role,
) -> Option<usize> {
    let palette = Palette::current(ui);
    let height = size.y;
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter().clone();
    // Each label's width as the chosen segment draws it, the wider face.
    let chosen_font = TextStyle::Name(theme::MEDIUM_STYLE.into()).resolve(ui.style());
    let labels: Vec<f32> = options
        .iter()
        .map(|option| {
            painter
                .layout_no_wrap(
                    (*option).to_owned(),
                    chosen_font.clone(),
                    palette.text_primary,
                )
                .size()
                .x
        })
        .collect();
    let widths = segment_widths(&labels, rect.width() - 2.0 * INSET_PX);
    painter.rect_filled(rect, theme::pill(height), palette.surface_sunken);
    let enabled = ui.is_enabled();
    let mut clicked = None;
    let mut left = rect.left() + INSET_PX;
    for (n, option) in options.iter().enumerate() {
        let segment_px = widths.get(n).copied().unwrap_or_default();
        let segment = Rect::from_min_size(
            egui::pos2(left, rect.top() + INSET_PX),
            egui::vec2(segment_px, height - 2.0 * INSET_PX),
        );
        // The pill is drawn inset, but a click anywhere in the track's
        // height counts: a 28-px control's pill is 22 px high, under the
        // 24-px floor for a target.
        let target =
            Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(segment_px, height));
        left += segment_px;
        let response = ui.interact(target, id.with(n), Sense::click());
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
        job.wrap = egui::text::TextWrapping::truncate_at_width(segment.width() - 2.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Segments are equal while every label fits its share; a label too
    /// long for an equal share takes its own width and the spare room is
    /// split, so *Plunge / Ramp / Helix* in a clearing pass's 146-px
    /// track draws *Plunge* whole, where equal thirds cut it to "Plun…".
    #[test]
    fn segments_are_equal_until_a_label_would_be_cut() {
        let equal = segment_widths(&[23.0, 14.0], 154.0);
        assert_eq!(equal, [77.0, 77.0]);

        // Plunge, Ramp, Helix at 13-px Medium, about 44, 34 and 31 px.
        let labels = [44.0, 34.0, 31.0];
        let room = 146.0;
        let widths = segment_widths(&labels, room);
        assert!(
            (widths.iter().sum::<f32>() - room).abs() < 1e-3,
            "{widths:?}"
        );
        for (width, label) in widths.iter().zip(labels) {
            assert!(
                width - label >= 2.0 * LABEL_PAD_PX,
                "each label has its padding: {widths:?}"
            );
        }
        assert!(widths[0] > widths[1] && widths[1] > widths[2], "{widths:?}");
        assert!(
            room / 3.0 < labels[0] + 2.0 * LABEL_PAD_PX,
            "an equal third would have cut the first"
        );
    }
}
