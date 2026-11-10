// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The checkbox (`docs/specs/phase-4/experience.md`, section 7,
//! "Checkbox"): a 16-px box with a 3-px radius and a `border.strong`
//! outline, filled `accent` with a white `check` when checked, its label
//! 8 px to the right, the whole 26-px row the hit target. egui's own
//! checkbox fills the box with the widget state's background and strokes
//! the tick in the text colour, so checked and unchecked differ only by
//! the tick; the review (#549) found it beside the painted one in the
//! Operations list and replaced it everywhere outside menus.

use std::fmt::Debug;
use std::hash::Hash;

use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::button::focus_ring;
use crate::widgets::icon::Icon;

/// The box's side, px.
pub(crate) const BOX_PX: f32 = 16.0;
/// The row's height, px.
const ROW_PX: f32 = 26.0;
/// The gap between the box and its label, px.
const GAP_PX: f32 = 8.0;
/// The narrowest hit target (section 11), px.
const MIN_HIT_PX: f32 = 24.0;

/// A checkbox labelled `label` over `checked`, its id `salt` under the
/// current scope; a click flips `checked` and marks the response changed.
/// The label wraps to the space left; an empty label leaves the box alone
/// (name it with `accessible_name`).
pub(crate) fn checkbox(
    ui: &mut Ui,
    salt: impl Hash + Debug,
    checked: &mut bool,
    label: &str,
) -> Response {
    let palette = Palette::current(ui);
    let enabled = ui.is_enabled();
    let colour = if enabled {
        palette.text_primary
    } else {
        palette.text_disabled
    };
    let galley = (!label.is_empty()).then(|| {
        let wrap = (ui.available_width() - BOX_PX - GAP_PX).max(0.0);
        ui.painter().layout(
            label.to_owned(),
            TextStyle::Body.resolve(ui.style()),
            colour,
            wrap,
        )
    });
    let text = galley.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let width = if galley.is_some() {
        BOX_PX + GAP_PX + text.x
    } else {
        MIN_HIT_PX
    };
    let size = egui::vec2(width, ROW_PX.max(text.y + 8.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut response = ui.interact(rect, ui.scope_id().with(("checkbox", salt)), Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let on = *checked;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    if ui.is_rect_visible(rect) {
        let left = if galley.is_some() {
            rect.left()
        } else {
            rect.center().x - BOX_PX / 2.0
        };
        let square = Rect::from_min_size(
            egui::pos2(left, rect.center().y - BOX_PX / 2.0),
            egui::Vec2::splat(BOX_PX),
        );
        paint_box(ui, square, on, response.hovered(), enabled);
        if let Some(galley) = galley {
            let at = egui::pos2(square.right() + GAP_PX, rect.center().y - text.y / 2.0);
            ui.painter().galley(at, galley, colour);
        }
    }
    focus_ring(ui, &response);
    response
}

/// The 16-px box in `rect`: `accent` with a white tick when `checked`,
/// else a `border.strong` outline (`text.secondary` on hover); disabled,
/// the outline is `border.default` and a checked box `text.disabled`.
pub(crate) fn paint_box(ui: &Ui, rect: Rect, checked: bool, hovered: bool, enabled: bool) {
    let palette = Palette::current(ui);
    if checked {
        let fill = if enabled {
            palette.accent
        } else {
            palette.text_disabled
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        let tick = Icon::Check.galley(ui, theme::ICON_SMALL_PX - 4.0, palette.text_on_accent);
        ui.painter().galley(
            rect.center() - tick.size() / 2.0,
            tick,
            palette.text_on_accent,
        );
    } else {
        let stroke = match (enabled, hovered) {
            (false, _) => palette.border_default,
            (true, true) => palette.text_secondary,
            (true, false) => palette.border_strong,
        };
        ui.painter()
            .rect_filled(rect.shrink(1.0), 3.0, palette.surface_sunken);
        ui.painter()
            .rect_stroke(rect, 3.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
}
