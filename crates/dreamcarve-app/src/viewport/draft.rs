// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What an armed tool wants drawn over the Viewport
//! (`docs/specs/phase-5/design.md`, "Visual design"; functional
//! capabilities 82 and 83a): the shape being drawn as it would be made
//! now, the marker where the next point would be taken, shaped for what
//! it snapped to with the kind's name beside it, the dashed guide of an
//! on-outline or a perpendicular snap, and the shape's sizes.
//!
//! Everything here is `dc_app::App::draft`'s, in project millimetres;
//! this module only puts it on the screen, through the camera's one flip.

use dc_app::tool::Draft;
use dc_app::viewport::camera::Camera;
use dc_app::{App, SnapKind, StrKey};
use dc_geometry::Point;
use eframe::egui::{self, Pos2, Rect, Stroke, TextStyle, Ui};

use super::{layers, window_pos};
use crate::shell::theme::{self, Palette};

/// The stroke of the shape being drawn, px.
const OUTLINE_PX: f32 = 1.5;
/// Half a snap marker's size, px.
const MARKER_PX: f32 = 5.0;
/// A snap marker's stroke, px.
const MARKER_STROKE_PX: f32 = 1.5;
/// A dash and a gap of a snap's guide, px.
const GUIDE_DASH_PX: f32 = 4.0;
/// A node of the path being edited, px square (design, "Visual
/// design").
const NODE_PX: f32 = 9.0;
/// A curve's handle and an arc's middle, px across.
const HANDLE_PX: f32 = 7.0;
/// The stroke of the selected span and of the piece a trim would take,
/// px.
const HIGHLIGHT_PX: f32 = 3.0;
/// How far a label stands off what it names, px.
const LABEL_OFFSET_PX: f32 = 10.0;

/// The name of a snap kind, written beside its marker.
pub(crate) const fn kind_name(kind: SnapKind) -> StrKey {
    match kind {
        SnapKind::Endpoint => StrKey::SnapKindEndpoint,
        SnapKind::Midpoint => StrKey::SnapKindMidpoint,
        SnapKind::Centre => StrKey::SnapKindCentre,
        SnapKind::Intersection => StrKey::SnapKindIntersection,
        SnapKind::OnOutline => StrKey::SnapKindOnOutline,
        SnapKind::Perpendicular => StrKey::SnapKindPerpendicular,
        SnapKind::Grid => StrKey::SnapKindGrid,
    }
}

/// `text` in a label pill whose `anchor` corner or side is at `at`.
fn label(ui: &Ui, painter: &egui::Painter, at: Pos2, anchor: egui::Align2, text: &str) {
    let palette = Palette::current(ui);
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_primary,
    );
    let size = galley.size() + egui::vec2(12.0, 4.0);
    let rect = anchor.anchor_size(at, size);
    painter.rect_filled(rect, theme::pill(rect.height()), palette.viewport_label_bg);
    painter.galley(
        rect.min + egui::vec2(6.0, 2.0),
        galley,
        palette.text_primary,
    );
}

/// The marker of a point snapped to `kind`, centred on `at`: a square
/// for an endpoint, a triangle for a midpoint, a circle with a cross for
/// a centre, a cross for an intersection, a circle for a point on an
/// outline, a right-angle mark for a perpendicular's foot, a small plus
/// for the grid.
fn marker(painter: &egui::Painter, at: Pos2, kind: SnapKind, stroke: Stroke) {
    let r = MARKER_PX;
    let line = |a: egui::Vec2, b: egui::Vec2| painter.line_segment([at + a, at + b], stroke);
    match kind {
        SnapKind::Endpoint => {
            painter.rect_stroke(
                Rect::from_center_size(at, egui::vec2(2.0 * r, 2.0 * r)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        SnapKind::Midpoint => {
            painter.add(egui::Shape::closed_line(
                vec![
                    at + egui::vec2(0.0, -r),
                    at + egui::vec2(r, r),
                    at + egui::vec2(-r, r),
                ],
                stroke,
            ));
        }
        SnapKind::Centre => {
            painter.circle_stroke(at, r, stroke);
            line(egui::vec2(-r, 0.0), egui::vec2(r, 0.0));
            line(egui::vec2(0.0, -r), egui::vec2(0.0, r));
        }
        SnapKind::Intersection => {
            line(egui::vec2(-r, -r), egui::vec2(r, r));
            line(egui::vec2(-r, r), egui::vec2(r, -r));
        }
        SnapKind::OnOutline => {
            painter.circle_stroke(at, r, stroke);
        }
        SnapKind::Perpendicular => {
            line(egui::vec2(-r, -r), egui::vec2(-r, r));
            line(egui::vec2(-r, r), egui::vec2(r, r));
            line(egui::vec2(-r, 0.0), egui::vec2(0.0, 0.0));
            line(egui::vec2(0.0, 0.0), egui::vec2(0.0, r));
        }
        SnapKind::Grid => {
            let r = r * 0.6;
            line(egui::vec2(-r, 0.0), egui::vec2(r, 0.0));
            line(egui::vec2(0.0, -r), egui::vec2(0.0, r));
        }
    }
}

/// Paints `app`'s draft over `pane`.
pub(super) fn paint(ui: &Ui, painter: &egui::Painter, pane: Rect, app: &App, camera: &Camera) {
    let palette = Palette::current(ui);
    let Draft {
        outlines,
        snap,
        labels,
        nodes,
        handles,
        highlights,
        marquee,
    } = app.draft();
    let at = |p: Point| window_pos(pane, camera, p);
    let tolerance_mm = layers::tolerance_mm(layers::band(camera.px_per_mm));
    let stroke = Stroke::new(OUTLINE_PX, palette.accent);
    for outline in outlines {
        let flat = outline.with_arcs_flattened(tolerance_mm);
        let points: Vec<Pos2> = flat.vertices().iter().map(|v| at(v.point())).collect();
        if flat.is_closed() {
            painter.add(egui::Shape::closed_line(points, stroke));
        } else {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
    // The selected span, or the piece a trim would take: over the outline.
    let highlight = Stroke::new(HIGHLIGHT_PX, palette.viewport_shape_hover);
    for outline in highlights {
        let flat = outline.with_arcs_flattened(tolerance_mm);
        let points: Vec<Pos2> = flat.vertices().iter().map(|v| at(v.point())).collect();
        painter.add(egui::Shape::line(points, highlight));
    }
    // The handles of the path being edited: a curve's on a stem from its
    // node, an arc's middle alone.
    let mark = Stroke::new(1.0, palette.accent);
    for handle in handles {
        let centre = at(handle.at_mm);
        if let Some(from) = handle.from_mm {
            painter.line_segment([at(from), centre], mark);
        }
        painter.circle(centre, HANDLE_PX / 2.0, palette.viewport_handle_fill, mark);
    }
    // Its nodes: a corner square, a smooth node round; a selected one
    // filled.
    for node in nodes {
        let centre = at(node.at_mm);
        let fill = if node.selected {
            palette.accent
        } else {
            palette.viewport_handle_fill
        };
        if node.smooth {
            painter.circle(centre, NODE_PX / 2.0, fill, mark);
        } else {
            painter.rect(
                Rect::from_center_size(centre, egui::vec2(NODE_PX, NODE_PX)),
                0.0,
                fill,
                mark,
                egui::StrokeKind::Middle,
            );
        }
    }
    if let Some((a, b)) = marquee {
        painter.rect(
            Rect::from_two_pos(at(*a), at(*b)),
            0.0,
            palette.accent_soft,
            Stroke::new(1.0, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    for size in labels {
        label(
            ui,
            painter,
            at(size.at_mm) + egui::vec2(0.0, LABEL_OFFSET_PX),
            egui::Align2::CENTER_TOP,
            &size.text,
        );
    }
    let Some(snap) = snap else {
        return;
    };
    let mark = Stroke::new(MARKER_STROKE_PX, palette.viewport_shape_hover);
    if let Some((from, to)) = snap.guide {
        painter.extend(egui::Shape::dashed_line(
            &[at(from), at(to)],
            Stroke::new(1.0, palette.viewport_shape_hover),
            GUIDE_DASH_PX,
            GUIDE_DASH_PX,
        ));
    }
    let point = at(snap.at_mm);
    match snap.kind {
        Some(kind) => {
            marker(painter, point, kind, mark);
            label(
                ui,
                painter,
                point + egui::vec2(LABEL_OFFSET_PX, -LABEL_OFFSET_PX),
                egui::Align2::LEFT_BOTTOM,
                app.strings().get(kind_name(kind)),
            );
        }
        // The pointer's own point: a dot, so where a click lands is seen.
        None => {
            painter.circle_filled(point, 2.0, palette.viewport_shape_hover);
        }
    }
}
