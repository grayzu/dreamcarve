// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A scrolling list's edges (`docs/specs/phase-4/experience.md`, section
//! 7, "Scrolling lists"): where a list goes on past the edge of its area,
//! its last 20 px fade into the surface under it, so a row cut by the
//! edge reads as *more below* and never as a row sliced in two. The fade
//! is at the bottom while there is more below and at the top while there
//! is more above; a list that fits has none.
//!
//! egui has no gradient fill, but a mesh takes a colour at each corner:
//! the fade is one quad from the surface's colour at the edge to nothing.

use eframe::egui::{self, Color32, Rect, Ui, scroll_area::ScrollAreaOutput};

use crate::shell::theme;

/// The fade's height, px.
pub(crate) const FADE_PX: f32 = 20.0;
/// How far a list must run past an edge before that edge fades, px: a
/// rounding error is not *more below*.
const SLACK_PX: f32 = 1.0;

/// Which edges of a scrolled list fade: `(top, bottom)`, for a list
/// `content_px` high scrolled `offset_px` down in a `viewport_px` window.
pub(crate) fn fades(offset_px: f32, viewport_px: f32, content_px: f32) -> (bool, bool) {
    (
        offset_px > SLACK_PX,
        offset_px + viewport_px < content_px - SLACK_PX,
    )
}

/// Paints the fades of the scroll area that gave `output`, into `surface`,
/// the colour under the list.
pub(crate) fn fade<R>(ui: &Ui, output: &ScrollAreaOutput<R>, surface: Color32) {
    let window = output.inner_rect;
    let (top, bottom) = fades(
        output.state.clamped_offset().y,
        window.height(),
        output.content_size.y,
    );
    let height = FADE_PX.min(window.height() / 2.0);
    let painter = ui.painter().with_clip_rect(window);
    if top {
        let band = Rect::from_min_max(
            window.min,
            egui::pos2(window.right(), window.top() + height),
        );
        painter.add(gradient(band, surface, theme::NONE));
    }
    if bottom {
        let band = Rect::from_min_max(
            egui::pos2(window.left(), window.bottom() - height),
            window.max,
        );
        painter.add(gradient(band, theme::NONE, surface));
    }
}

/// `rect` filled from `top` along its top edge to `bottom` along its
/// bottom edge.
fn gradient(rect: Rect, top: Color32, bottom: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list that fits fades nowhere; one longer than its window fades
    /// at the bottom from its top, at both edges in its middle, and at
    /// the top alone at its end.
    #[test]
    fn a_list_fades_where_it_goes_on() {
        assert_eq!(fades(0.0, 300.0, 200.0), (false, false), "it fits");
        assert_eq!(fades(0.0, 300.0, 300.4), (false, false), "to a rounding");
        assert_eq!(fades(0.0, 300.0, 734.0), (false, true), "from its top");
        assert_eq!(fades(120.0, 300.0, 734.0), (true, true), "in its middle");
        assert_eq!(fades(434.0, 300.0, 734.0), (true, false), "at its end");
    }
}
