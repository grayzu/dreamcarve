// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The card (`docs/specs/phase-4/redesign.md`, principle 2 and
//! "Components"; "egui notes"): a region of the window is a
//! `surface.panel` rectangle with a 14-px radius and no border, on the
//! `surface.window` backdrop. egui clips to rectangles, so what a card
//! holds (a list row's wash, a scroll area, the Viewport's wgpu callback)
//! would square its corners off; [`mask_corners`] paints the four corners
//! back in the backdrop's colour after the content.
//!
//! A card inside a card (an example card, a callout) is [`inner`]:
//! `surface.raised` at the smaller radius.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder};

use crate::shell::theme::{self, Palette};

/// How many segments draw a corner's quarter circle.
const ARC_STEPS: u8 = 12;

/// Draws `body` in a card that fills the space `ui` has left, and masks
/// its corners afterwards. The card has no padding of its own: rows run
/// to its edges, and each panel keeps its content 16 px in.
pub(crate) fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = Palette::current(ui);
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(rect, theme::CARD_RADIUS_PX, palette.surface_panel);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let out = body(&mut child);
    ui.allocate_rect(rect, Sense::hover());
    mask_corners(ui.painter(), rect, theme::CARD_RADIUS_PX, palette);
    out
}

/// Draws `body` in a card as tall as what it holds, across the width `ui`
/// has left, and masks its corners afterwards.
pub(crate) fn fitted<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = Palette::current(ui);
    let background = ui.painter().add(Shape::Noop);
    let inner = ui.scope(|ui| {
        ui.set_width(ui.available_width());
        body(ui)
    });
    let rect = inner.response.rect;
    ui.painter().set(
        background,
        egui::epaint::RectShape::filled(rect, theme::CARD_RADIUS_PX, palette.surface_panel),
    );
    mask_corners(ui.painter(), rect, theme::CARD_RADIUS_PX, palette);
    inner.inner
}

/// The frame of a card inside a card: `surface.raised` at the inner
/// radius, with a hairline so it reads on a light card of the same white.
pub(crate) fn inner(palette: &Palette) -> egui::Frame {
    egui::Frame::NONE
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(theme::INNER_CARD_RADIUS_PX)
}

/// Paints the four corners of `rect` outside a `radius_px` rounding in
/// the backdrop's colour, over whatever was drawn there: the rounded
/// clipping egui does not have. Nothing is painted outside `rect`.
pub(crate) fn mask_corners(painter: &egui::Painter, rect: Rect, radius_px: f32, palette: &Palette) {
    let radius = radius_px.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if radius <= 0.0 {
        return;
    }
    for (corner, sx, sy) in [
        (rect.left_top(), -1.0, -1.0),
        (rect.right_top(), 1.0, -1.0),
        (rect.right_bottom(), 1.0, 1.0),
        (rect.left_bottom(), -1.0, 1.0),
    ] {
        painter.add(corner_mask(corner, sx, sy, radius, palette.surface_window));
    }
}

/// One corner's mask: the square corner at `corner` less the quarter disc
/// inside it. `sx` and `sy` say which way the corner points. The outline
/// starts at the corner, which sees the whole arc, so the tessellator's
/// fan from the first point fills it though it is not convex.
fn corner_mask(corner: Pos2, sx: f32, sy: f32, radius: f32, colour: Color32) -> Shape {
    let centre = corner - egui::vec2(sx, sy) * radius;
    let mut arc: Vec<Pos2> = (0..=ARC_STEPS)
        .map(|k| {
            let angle = f32::from(k) / f32::from(ARC_STEPS) * std::f32::consts::FRAC_PI_2;
            centre + radius * egui::vec2(sx * angle.sin(), sy * angle.cos())
        })
        .collect();
    // Clockwise on screen, the winding the tessellator keeps as given: a
    // path it has to reverse would no longer start at the corner.
    if sx * sy < 0.0 {
        arc.reverse();
    }
    let mut points = Vec::with_capacity(arc.len() + 1);
    points.push(corner);
    points.extend(arc);
    Shape::convex_polygon(points, colour, Stroke::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tessellator's own measure of winding: positive is clockwise on
    /// screen (y down), the order it keeps.
    fn signed_area(points: &[Pos2]) -> f32 {
        let mut area = 0.0;
        let mut previous = points[points.len() - 1];
        for p in points {
            area += previous.x * p.y - p.x * previous.y;
            previous = *p;
        }
        area
    }

    /// Every corner's mask starts at its corner and winds clockwise, so
    /// the fill fans from the corner; and it stays inside the corner's
    /// square.
    #[test]
    fn corner_masks_start_at_the_corner_and_wind_clockwise() {
        let rect = Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(110.0, 90.0));
        let radius = 14.0;
        for (corner, sx, sy) in [
            (rect.left_top(), -1.0, -1.0),
            (rect.right_top(), 1.0, -1.0),
            (rect.right_bottom(), 1.0, 1.0),
            (rect.left_bottom(), -1.0, 1.0),
        ] {
            let Shape::Path(path) = corner_mask(corner, sx, sy, radius, theme::NONE) else {
                panic!("a path");
            };
            assert_eq!(path.points[0], corner);
            assert!(signed_area(&path.points) > 0.0, "{sx} {sy}");
            let square = Rect::from_two_pos(corner, corner - egui::vec2(sx, sy) * radius);
            for p in &path.points {
                assert!(square.expand(1e-3).contains(*p), "{p:?} in {square:?}");
            }
        }
    }
}
