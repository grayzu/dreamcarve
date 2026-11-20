// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A sheet that resizes (`docs/specs/phase-4/experience.md`, section 7,
//! "Sheet"; functional spec, capability 64): the Tool Library, whose
//! table and import review are worth more room than the default gives
//! them. The sheet is dragged by any edge or by the grip at its bottom
//! right corner. It stays centred in the window, so an edge moves with
//! the pointer and the opposite edge moves the other way. The size is the
//! caller's to keep (`dc_app::prefs::SheetSize`, in `gui.toml`); a small
//! sheet that asks one question keeps its fixed size and calls nothing
//! here.

use eframe::egui::{self, Id, Rect, Sense, Stroke, Ui};

use crate::shell::theme::Palette;

/// How wide the strip along an edge that takes the drag is, px.
const EDGE_PX: f32 = 8.0;
/// The corner grip's square, px.
const GRIP_PX: f32 = 18.0;

/// The handles of the sheet whose frame is `outer` in the window, drawn
/// in `ui`, the sheet's own: a strip along each edge and the grip at the
/// bottom right corner, named `grip` for assistive tools. The size the
/// drag asks for this frame, px, when one is under way; the caller holds
/// it between its limits.
pub(crate) fn resize_handles(ui: &Ui, id: Id, outer: Rect, grip: &str) -> Option<egui::Vec2> {
    let palette = Palette::current(ui);
    // The window's centre, where the sheet is centred: not the frame's
    // own, which lags a frame behind a change of size, so that a size
    // worked out from it would swing either side of the pointer.
    let centre = ui.ctx().content_rect().center();
    let size = outer.size();
    // Each handle: its strip, whether it sets the width and the height,
    // and its cursor.
    let corner = Rect::from_min_max(outer.max - egui::vec2(GRIP_PX, GRIP_PX), outer.max);
    let handles = [
        (
            "right",
            Rect::from_min_max(
                egui::pos2(outer.right() - EDGE_PX, outer.top() + GRIP_PX),
                egui::pos2(outer.right(), outer.bottom() - GRIP_PX),
            ),
            (true, false),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            "left",
            Rect::from_min_max(
                egui::pos2(outer.left(), outer.top() + GRIP_PX),
                egui::pos2(outer.left() + EDGE_PX, outer.bottom() - GRIP_PX),
            ),
            (true, false),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            "bottom",
            Rect::from_min_max(
                egui::pos2(outer.left() + GRIP_PX, outer.bottom() - EDGE_PX),
                egui::pos2(outer.right() - GRIP_PX, outer.bottom()),
            ),
            (false, true),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            "top",
            Rect::from_min_max(
                egui::pos2(outer.left() + GRIP_PX, outer.top()),
                egui::pos2(outer.right() - GRIP_PX, outer.top() + EDGE_PX),
            ),
            (false, true),
            egui::CursorIcon::ResizeVertical,
        ),
        ("corner", corner, (true, true), egui::CursorIcon::ResizeNwSe),
    ];
    let mut asked = None;
    let mut grip_lit = false;
    for (name, rect, (wide, tall), cursor) in handles {
        let response = ui.interact(rect, id.with(name), Sense::drag());
        if name == "corner" {
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, grip));
            grip_lit = response.hovered() || response.dragged();
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        // How far inside the frame's edge the pointer took hold, kept for
        // the drag, so the edge stays under the pointer.
        let hold_id = id.with((name, "hold"));
        if response.drag_started()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let hold = size / 2.0 - (pointer - centre).abs();
            ui.data_mut(|d| d.insert_temp(hold_id, hold));
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            // The sheet is centred: an edge is as far from the centre as
            // the pointer is, and the opposite edge as far the other way.
            let hold = ui
                .data(|d| d.get_temp::<egui::Vec2>(hold_id))
                .unwrap_or(egui::Vec2::ZERO);
            let from_centre = ((pointer - centre).abs() + hold) * 2.0;
            asked = Some(egui::vec2(
                if wide { from_centre.x } else { size.x },
                if tall { from_centre.y } else { size.y },
            ));
        }
    }
    // The grip: three short strokes across the corner.
    let colour = if grip_lit {
        palette.accent
    } else {
        palette.text_disabled
    };
    for k in 1..=3_u8 {
        let reach = 4.0 * f32::from(k);
        let at = corner.max - egui::vec2(5.0, 5.0);
        ui.painter().line_segment(
            [
                egui::pos2(at.x - reach, at.y),
                egui::pos2(at.x, at.y - reach),
            ],
            Stroke::new(1.0, colour),
        );
    }
    asked
}
