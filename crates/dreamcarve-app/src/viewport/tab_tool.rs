// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs on the Viewport (functional spec, capability 69;
//! `experience.md`, sections 8 and 9, "Tab dragging" and the
//! `viewport.tab` token; mock-up 08; #546): the Tab tool's armed state and
//! its click, and the tab marks the overlay paints over the GPU layers.
//!
//! The toolpaths' own tab marks (`dc_job::preview::marks`, slice 16) show
//! each crossed tab across each run that rises over it. What this module
//! paints is what they cannot show: a tab no enabled Profile crosses,
//! hollow (a 1.5-px outline the tab's width long); the selected tabs in
//! `shape.selected`; the hovered tab 2 px wider; and the tab being
//! dragged, where the release will put it. Every position comes from the
//! Scene (computed per snapshot) or, for the drag, from
//! `dc_app::viewport::tabs`; nothing is computed per frame but the pixels.

use dc_app::viewport::camera::{Camera, Pos};
use dc_app::viewport::pick::PICK_TOLERANCE_PX;
use dc_app::viewport::tabs::{self, OutlinePoint};
use dc_app::{App, Command};
use dc_geometry::{Point, Vec2};
use dc_project::TabId;
use eframe::egui::{self, Id, Pos2, Rect, Stroke};

use crate::shell::theme::Palette;

/// A tab mark's width across the outline, px (`experience.md`, section 9:
/// "tab mark 4 px wide").
const MARK_PX: f32 = 4.0;
/// A hovered mark is this much wider, px (section 8: "hover widens it
/// 2 px").
const HOVER_EXTRA_PX: f32 = 2.0;
/// A hollow tab's outline, px (section 5: "the same stroke at 1.5 px with
/// no fill").
const HOLLOW_PX: f32 = 1.5;

/// Where the Tab tool's armed state lives.
fn armed_id() -> Id {
    Id::unique("dc.viewport.tab_tool")
}

/// Whether the Tab tool is armed: the next click on an outline adds a
/// tab.
pub(crate) fn armed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(armed_id()))
        .unwrap_or(false)
}

/// Arms or disarms the Tab tool.
pub(crate) fn arm(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(armed_id(), on));
}

/// The command an armed click at pane point `at` makes: a tab at the
/// outline point under it with the last-used size (`Prefs::tab`), or
/// nothing away from every outline.
pub(crate) fn click(app: &App, camera: &Camera, at: Pos) -> Option<Command> {
    let point: OutlinePoint =
        tabs::outline_at(app.scene(), app.project(), camera, at, PICK_TOLERANCE_PX)?;
    Some(tabs::add_tab(app.project(), &point, app.prefs().tab))
}

/// What the overlay paints of the tabs this frame.
pub(crate) struct Marks<'a> {
    /// The tab under the pointer.
    pub hover: Option<&'a TabId>,
    /// The tab being dragged and its definition-space point.
    pub dragged: Option<&'a (TabId, f64, f64)>,
}

/// The corners of the band `width_mm` long along `tangent` and `across_px`
/// wide, centred on `at_mm`, on the window.
fn band(
    pane: Rect,
    camera: &Camera,
    at_mm: Point,
    tangent: Vec2,
    width_mm: f64,
    across_px: f32,
) -> [Pos2; 4] {
    let screen = |p: Point| {
        let s = camera.to_screen(p);
        pane.min
            + egui::vec2(
                super::renderer::gpu_f32(s.x_px),
                super::renderer::gpu_f32(s.y_px),
            )
    };
    let half = width_mm / 2.0;
    let a = screen(Point::new(
        at_mm.x - tangent.x * half,
        at_mm.y - tangent.y * half,
    ));
    let b = screen(Point::new(
        at_mm.x + tangent.x * half,
        at_mm.y + tangent.y * half,
    ));
    let along = (b - a).normalized();
    let normal = if along == egui::Vec2::ZERO {
        egui::vec2(0.0, 1.0)
    } else {
        egui::vec2(-along.y, along.x)
    } * (across_px / 2.0);
    [a + normal, b + normal, b - normal, a - normal]
}

/// Paints the tab marks the toolpaths do not (module docs).
pub(crate) fn paint(
    painter: &egui::Painter,
    palette: &Palette,
    pane: Rect,
    app: &App,
    camera: &Camera,
    marks: &Marks<'_>,
) {
    let scene = app.scene();
    let selected = app.selection().tabs();
    for mark in scene
        .tabs()
        .iter()
        .filter(|m| scene.shape(&m.instance).is_some())
    {
        let dragged = marks.dragged.filter(|(id, ..)| *id == mark.id);
        let hovered = marks.hover == Some(&mark.id);
        let is_selected = selected.contains(&mark.id);
        let (at_mm, tangent) = match dragged {
            Some((_, x_mm, y_mm)) => app
                .project()
                .tabs
                .iter()
                .find(|t| t.id == mark.id)
                .and_then(|t| {
                    tabs::tab_frame(
                        app.project(),
                        &dc_project::PlacedTab {
                            x_mm: *x_mm,
                            y_mm: *y_mm,
                            ..t.clone()
                        },
                    )
                })
                .unwrap_or((mark.at_mm, mark.tangent)),
            None => (mark.at_mm, mark.tangent),
        };
        let across = MARK_PX + if hovered { HOVER_EXTRA_PX } else { 0.0 };
        let corners = band(pane, camera, at_mm, tangent, mark.width_mm, across);
        if dragged.is_some() || is_selected {
            painter.add(egui::Shape::convex_polygon(
                corners.to_vec(),
                palette.viewport_shape_selected,
                Stroke::NONE,
            ));
        } else if mark.hollow() {
            painter.add(egui::Shape::closed_line(
                corners.to_vec(),
                Stroke::new(HOLLOW_PX, palette.viewport_tab),
            ));
        } else if hovered {
            painter.add(egui::Shape::convex_polygon(
                corners.to_vec(),
                palette.viewport_tab,
                Stroke::NONE,
            ));
        }
    }
}
