// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operation-kind glyphs (`docs/specs/phase-4/experience.md`, section
//! 6; section 12, `op_glyph`): DreamCarve's own, drawn with the painter on
//! Lucide's 24-unit grid with its round 2-unit stroke, so each takes its
//! operation's colour. They mark the list's rows, the Add menu, the
//! editor's title and the status bar's *Generating* cell, readable at
//! 16 px.

use dc_app::operations::Kind;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, StrokeKind};

/// The grid the glyphs are drawn on, units.
const GRID: f32 = 24.0;

/// Draws `kind`'s glyph filling `rect` (square) in `colour`.
pub(crate) fn op_glyph(painter: &Painter, rect: Rect, kind: Kind, colour: Color32) {
    let unit = rect.width().min(rect.height()) / GRID;
    let at = |x: f32, y: f32| -> Pos2 { rect.min + egui::vec2(x * unit, y * unit) };
    let stroke = Stroke::new((2.0 * unit).max(1.25), colour);
    let line = |points: &[(f32, f32)]| {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
        painter.add(egui::Shape::line(points, stroke));
    };
    let square = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            2.0 * unit,
            stroke,
            StrokeKind::Middle,
        );
    };
    match kind {
        // The outline, and the cut beside it on its outside right and
        // bottom.
        Kind::Profile => {
            square(3.0, 3.0, 16.0, 16.0);
            line(&[(20.0, 6.0), (20.0, 20.0), (6.0, 20.0)]);
        }
        // The outline filled with three raster lines.
        Kind::Pocket => {
            square(3.0, 3.0, 21.0, 21.0);
            for y in [8.0, 12.0, 16.0] {
                line(&[(7.0, y), (17.0, y)]);
            }
        }
        // A circle with a centred dot.
        Kind::Drill => {
            painter.circle_stroke(at(12.0, 12.0), 9.0 * unit, stroke);
            painter.circle_filled(at(12.0, 12.0), 2.0 * unit, colour);
        }
        // A circle, a smaller concentric one, and a short spiral tick.
        Kind::Bore => {
            painter.circle_stroke(at(12.0, 12.0), 9.0 * unit, stroke);
            painter.circle_stroke(at(12.0, 12.0), 4.5 * unit, stroke);
            line(&[(12.0, 7.5), (14.5, 5.5)]);
        }
        // One open wavy line.
        Kind::Engrave => {
            let wave: Vec<(f32, f32)> = (0_u8..=16)
                .map(|k| {
                    let x = 3.0 + f32::from(k) * (18.0 / 16.0);
                    let phase = (x - 3.0) / 18.0 * std::f32::consts::TAU * 1.5;
                    (x, 12.0 - 4.0 * phase.sin())
                })
                .collect();
            line(&wave);
        }
        // A V above a short baseline.
        Kind::VCarve => {
            line(&[(4.0, 4.0), (12.0, 16.0), (20.0, 4.0)]);
            line(&[(6.0, 20.0), (18.0, 20.0)]);
        }
        // The plug in its pocket: the pocket an open V, the plug a closed
        // V (its back the flat top) nested inside it with a glue gap. The
        // first drawing, a V over an inverted V, read as an X at 16 px
        // (#549).
        Kind::Inlay => {
            line(&[(3.0, 9.0), (12.0, 21.0), (21.0, 9.0)]);
            let plug = [(6.0, 4.0), (12.0, 13.0), (18.0, 4.0)];
            let plug: Vec<Pos2> = plug.iter().map(|(x, y)| at(*x, *y)).collect();
            painter.add(egui::Shape::closed_line(plug, stroke));
        }
        // A rounded rectangle with four short diagonal hatch strokes.
        Kind::PhotoVCarve => {
            square(3.0, 5.0, 21.0, 19.0);
            for x in [6.0, 10.0, 14.0, 18.0] {
                line(&[(x - 1.5, 15.0), (x + 1.5, 9.0)]);
            }
        }
    }
}
