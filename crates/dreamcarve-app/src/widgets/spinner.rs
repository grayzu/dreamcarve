// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The spinner (`docs/specs/phase-4/experience.md`, sections 7, "Status
//! cell", and 10): a 14-px arc of 270° in the accent colour, one turn per
//! 800 ms, the app's only continuous animation. Painted here rather than
//! with egui's `Spinner`, which asks for the next frame at once: this one
//! asks for it a frame-time later, so the window idles at 30 frames a
//! second while a job runs and a test harness settles between steps.

use std::f32::consts::TAU;
use std::time::Duration;

use eframe::egui::{self, Response, Sense, Stroke, Ui};

use crate::shell::theme::Palette;

/// The spinner's size, px.
const SIZE_PX: f32 = 14.0;
/// One turn, s.
const TURN_S: f64 = 0.8;
/// How soon the next frame is asked for while it spins.
const FRAME: Duration = Duration::from_millis(33);

/// The fraction of a turn at `time_s`. A fraction of 1 is exact in
/// `f32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a fraction in [0, 1), exact enough in f32"
)]
fn phase(time_s: f64) -> f32 {
    ((time_s % TURN_S) / TURN_S) as f32
}

/// Draws the spinner, named `label` for the accessibility tree.
pub fn spinner(ui: &mut Ui, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE_PX, SIZE_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, label));
    let palette = Palette::current(ui);
    let start = phase(ui.input(|i| i.time)) * TAU;
    let radius = SIZE_PX / 2.0 - 1.5;
    let steps = 24_u8;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|k| {
            let angle = start + f32::from(k) / f32::from(steps) * TAU * 0.75;
            rect.center() + radius * egui::vec2(angle.cos(), angle.sin())
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(2.0, palette.accent)));
    ui.ctx().request_repaint_after(FRAME);
    response
}
