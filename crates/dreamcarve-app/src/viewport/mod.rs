// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport, as far as slice 3 takes it (`experience.md`, section 9):
//! the stage in `viewport.bg`, the stock as a `viewport.stock.fill` sheet
//! with its dashed outline, and the origin crosshair with its `0,0`
//! label, fitted to the pane. The camera, the wgpu line renderer, shapes,
//! toolpaths and the grid are slice 4's (#533); the empty-state card
//! floats over it until a shape arrives (functional spec, capability 79).
//!
//! Y flips exactly once, in [`Fit::to_screen`] and back in
//! [`Fit::to_world`] (`docs/rust-guidelines.md`, "GUI"). The park point
//! is drawn as its crossed circle and, while the Project panel's *Click on
//! the view* is armed, a click on the pane sets it (#536); TODO(#533): the
//! Viewport's camera replaces `Fit` for both.

use dc_app::StrKey;
use dc_geometry::Point;
use eframe::egui::{self, Pos2, Rect, Stroke, TextStyle, Ui};

use crate::panels::{empty, project};
use crate::shell::theme::Palette;
use crate::shell::window::{Action, FrameState};

/// The margin round the fitted stock, px.
const FIT_MARGIN_PX: f32 = 48.0;
/// The gutter between the Viewport and its neighbours, px.
const GUTTER_PX: i8 = 4;

/// Project millimetres (Y-up) to the pane's pixels (Y-down), fitted so
/// the stock fills the pane less a margin.
#[derive(Debug, Clone, Copy)]
struct Fit {
    /// The project point at the pane's centre, mm.
    centre_mm: Point,
    /// Pixels per millimetre.
    px_per_mm: f64,
    /// The pane's centre.
    pane_centre: Pos2,
}

impl Fit {
    fn new(pane: Rect, width_mm: f64, height_mm: f64) -> Self {
        let room = |px: f32| f64::from((px - 2.0 * FIT_MARGIN_PX).max(1.0));
        let px_per_mm =
            (room(pane.width()) / width_mm.max(1.0)).min(room(pane.height()) / height_mm.max(1.0));
        Self {
            centre_mm: Point::new(width_mm / 2.0, height_mm / 2.0),
            px_per_mm,
            pane_centre: pane.center(),
        }
    }

    /// The one Y flip: project Y grows up, screen Y grows down.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "screen coordinates of a fitted pane, exact enough in f32"
    )]
    fn to_screen(self, p: Point) -> Pos2 {
        let x = f64::from(self.pane_centre.x) + (p.x - self.centre_mm.x) * self.px_per_mm;
        let y = f64::from(self.pane_centre.y) - (p.y - self.centre_mm.y) * self.px_per_mm;
        egui::pos2(x as f32, y as f32)
    }

    /// [`Fit::to_screen`]'s inverse: a pane pixel as a project point, mm.
    fn to_world(self, s: Pos2) -> Point {
        Point::new(
            self.centre_mm.x + f64::from(s.x - self.pane_centre.x) / self.px_per_mm,
            self.centre_mm.y - f64::from(s.y - self.pane_centre.y) / self.px_per_mm,
        )
    }
}

/// The park mark's radius, px.
const PARK_RADIUS_PX: f32 = 6.0;

/// Draws the Viewport in the central panel; returns its rectangle.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) -> Rect {
    let palette = Palette::current(ui);
    egui::CentralPanel::no_frame()
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(GUTTER_PX),
        )
        .show(ui, |ui| {
            let picking = project::park_pick_armed(ui.ctx());
            let sense = if picking {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (pane, response) = ui.allocate_exact_size(ui.available_size(), sense);
            let painter = ui.painter_at(pane);
            painter.rect_filled(pane, 0.0, palette.viewport_bg);
            let project = state.app.project();
            let stock = &project.stock;
            let fit = Fit::new(pane, stock.width_mm, stock.height_mm);

            let corners = [
                Point::new(0.0, 0.0),
                Point::new(stock.width_mm, 0.0),
                Point::new(stock.width_mm, stock.height_mm),
                Point::new(0.0, stock.height_mm),
            ]
            .map(|p| fit.to_screen(p));
            let sheet = Rect::from_points(&corners);
            painter.rect_filled(sheet, 0.0, palette.viewport_stock_fill);
            let mut outline = corners.to_vec();
            outline.extend(corners.first());
            painter.extend(egui::Shape::dashed_line(
                &outline,
                Stroke::new(1.5, palette.viewport_stock),
                6.0,
                4.0,
            ));

            // The origin crosshair: 12-px arms, a 3-px gap, `0,0` below-left.
            let origin = fit.to_screen(project.origin.xy.point_mm(stock));
            let axis = Stroke::new(1.0, palette.viewport_axis);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let dir = egui::vec2(dx, dy);
                painter.line_segment([origin + dir * 3.0, origin + dir * 12.0], axis);
            }
            painter.text(
                origin + egui::vec2(-4.0, 4.0),
                egui::Align2::RIGHT_TOP,
                state.app.strings().get(StrKey::AppViewportOrigin),
                TextStyle::Small.resolve(ui.style()),
                palette.text_secondary,
            );

            // The park point: a crossed circle (`experience.md`, section 5).
            let marks = dc_app::project::marks(project);
            if let Some(park) = marks.park_mm {
                let at = fit.to_screen(park);
                let stroke = Stroke::new(1.5, palette.viewport_park);
                painter.circle_stroke(at, PARK_RADIUS_PX, stroke);
                let arm = PARK_RADIUS_PX * std::f32::consts::FRAC_1_SQRT_2;
                painter.line_segment(
                    [at + egui::vec2(-arm, -arm), at + egui::vec2(arm, arm)],
                    stroke,
                );
                painter.line_segment(
                    [at + egui::vec2(-arm, arm), at + egui::vec2(arm, -arm)],
                    stroke,
                );
            }

            if picking {
                response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    project::arm_park_pick(ui.ctx(), false);
                } else if response.clicked()
                    && let Some(at) = response.interact_pointer_pos()
                {
                    project::arm_park_pick(ui.ctx(), false);
                    out.extend(project::park_at(project, fit.to_world(at)).map(Action::run));
                }
            }

            if state.show_empty {
                empty::card(ui, pane, state, out);
            }
            pane
        })
        .inner
}
