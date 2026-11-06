// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport (functional spec, capabilities 72 and 75; `experience.md`,
//! section 9; `docs/specs/phase-4/design.md`, "The Viewport renderer"):
//! the stage, the stock sheet, and the wgpu line layers ([`renderer`])
//! for the grid, the stock's outline, the shapes, the hover and the
//! selection, with the overlay egui's painter draws on top (the origin
//! crosshair and its label, the park point, the padlocks, the marquee)
//! and the empty-state card over all while the project has no shapes.
//!
//! Navigation: a middle-button drag, a Space-drag or a two-finger scroll
//! pans; the wheel, Ctrl/⌘-scroll and a pinch zoom about the cursor
//! (open question 8). Selection: a click picks through
//! `dc_app::viewport::pick`, a second click in place cycles, Shift adds,
//! Ctrl/⌘ toggles, a drag from empty stock is a marquee, and the result is
//! `dc_app`'s one selection. Every position is the camera's: Y flips
//! once, in `dc_app::Camera::to_screen`, which the painter and the
//! pointer go through; the GPU never works in Y-down.

mod layers;
pub(crate) mod renderer;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dc_app::viewport::camera::{self, Camera, Pos};
use dc_app::viewport::grid::Grid;
use dc_app::viewport::pick::{self, ClickCycle, Hit, PICK_TOLERANCE_PX};
use dc_app::{App, ClickMode, StrKey};
use dc_geometry::{Point, Rect as MmRect};
use dc_project::InstanceId;
use eframe::egui::{
    self, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, Stroke,
    TextStyle, Ui,
};

use crate::panels::empty;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::icon::Icon;
use renderer::{Frame, Layer, LayerData, StyleId};

/// The gutter between the Viewport and its neighbours, px.
const GUTTER_PX: i8 = 4;
/// How far the pointer may move between press and release and still
/// click rather than drag, px (`experience.md`, section 8: a drag starts
/// at 4 px).
const CLICK_SLOP_PX: f32 = 4.0;
/// Scroll points per wheel line, as egui's native backends count them.
const POINTS_PER_LINE: f32 = 40.0;
/// Zoom per scrolled point: a wheel notch of 40 points zooms by e^0.16,
/// about 17 %, close to PureCutCNC's e^(0.0015 × 100) per browser notch.
const ZOOM_PER_POINT: f64 = 0.004;
/// The origin crosshair's arm and its gap at the centre, px.
const CROSSHAIR_ARM_PX: f32 = 12.0;
/// The gap at the crosshair's centre, px.
const CROSSHAIR_GAP_PX: f32 = 3.0;
/// The park point's radius, px.
const PARK_RADIUS_PX: f32 = 6.0;

/// What the pointer is doing on the pane.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Panning: the middle button, or the primary with Space held.
    Pan { last: Pos2 },
    /// The primary button pressed on nothing: a marquee once it moves.
    Marquee { start: Pos2 },
    /// The primary button pressed on a shape: a click on release.
    Press { start: Pos2 },
}

/// One layer's contents as last built, and what they were built from.
#[derive(Debug, Clone, Default)]
struct Cached {
    key: Option<u64>,
    version: u64,
    data: Arc<LayerData>,
}

/// The Viewport's state between frames: the pointer's gesture, the click
/// cycle, the hover, and each layer's last contents. Nothing of the
/// project's: that is the app's and the Scene's.
#[derive(Debug, Default)]
pub(crate) struct ViewportState {
    /// Fit the camera to the project at the next frame (a project was
    /// opened or made).
    fit_pending: bool,
    /// The camera is as Fit left it, so a resized pane fits again.
    fitted: bool,
    drag: Option<Drag>,
    clicks: ClickCycle,
    /// The pointer's project point, when it is over the pane.
    cursor_mm: Option<Point>,
    /// The shape under the pointer.
    hover: Option<InstanceId>,
    layers: [Cached; Layer::ALL.len()],
    styles: Option<(bool, u64, Arc<[renderer::Style; renderer::STYLE_COUNT]>)>,
    next_version: u64,
}

/// What the status bar shows of the pointer: its project point and the
/// name of the shape under it, locked or not.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Pointer {
    pub cursor_mm: Option<Point>,
    pub hover: Option<(String, bool)>,
}

/// A hash of `value`, a layer's cache key.
fn key(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// `pos` on `pane` as the camera reads it.
fn pane_pos(pane: Rect, pos: Pos2) -> Pos {
    Pos::new(f64::from(pos.x - pane.min.x), f64::from(pos.y - pane.min.y))
}

/// `p` (mm) on the window, through the camera's one flip.
fn window_pos(pane: Rect, camera: &Camera, p: Point) -> Pos2 {
    let s = camera.to_screen(p);
    pane.min + egui::vec2(renderer::gpu_f32(s.x_px), renderer::gpu_f32(s.y_px))
}

impl ViewportState {
    /// Fits the camera to the project at the next frame, as after an open.
    pub(crate) fn fit_on_next_frame(&mut self) {
        self.fit_pending = true;
    }

    /// Records that the camera was just fitted (F), so a resize refits.
    pub(crate) fn mark_fitted(&mut self, fitted: bool) {
        self.fitted = fitted;
    }

    /// Forgets the click cycle (the selection changed elsewhere).
    pub(crate) fn reset_clicks(&mut self) {
        self.clicks.reset();
    }

    /// Reads where the pointer is over last frame's `pane`, for the status
    /// bar and this frame's hover.
    pub(crate) fn read_pointer(&mut self, ctx: &egui::Context, pane: Rect, app: &App) -> Pointer {
        let over = ctx.pointer_hover_pos().filter(|p| {
            pane.contains(*p)
                && ctx
                    .layer_id_at(*p)
                    .is_none_or(|layer| layer.order == egui::Order::Background)
        });
        let camera = app.view().camera;
        self.cursor_mm = over.map(|p| camera.to_world(pane_pos(pane, p)));
        self.hover = match (over, self.drag) {
            (Some(p), None | Some(Drag::Press { .. })) => {
                match pick::pick(
                    app.scene(),
                    &camera,
                    pane_pos(pane, p),
                    PICK_TOLERANCE_PX,
                    0,
                ) {
                    Hit::Instance(id) => Some(id),
                    Hit::Nothing => None,
                }
            }
            _ => None,
        };
        Pointer {
            cursor_mm: self.cursor_mm,
            hover: self.hover.as_ref().and_then(|id| {
                let shape = app.scene().shape(id)?;
                Some((shape.name.clone(), shape.locked))
            }),
        }
    }

    /// Layer `layer`'s contents, rebuilt by `build` when `key` differs
    /// from the last build's.
    fn layer(
        &mut self,
        layer: Layer,
        key: u64,
        build: impl FnOnce() -> Vec<renderer::Segment>,
    ) -> (u64, Arc<LayerData>) {
        let next = self.next_version.wrapping_add(1);
        let Some(cached) = self.layers.get_mut(layer.index()) else {
            return (0, Arc::default());
        };
        if cached.key != Some(key) {
            cached.key = Some(key);
            cached.version = next;
            cached.data = Arc::new(LayerData::new(&build()));
            self.next_version = next;
        }
        (cached.version, Arc::clone(&cached.data))
    }

    /// The style table for `palette`, rebuilt when the theme changes.
    fn styles(
        &mut self,
        palette: &Palette,
    ) -> (u64, Arc<[renderer::Style; renderer::STYLE_COUNT]>) {
        match &self.styles {
            Some((dark, version, table)) if *dark == palette.dark => (*version, Arc::clone(table)),
            _ => {
                let version = self.next_version.wrapping_add(1);
                self.next_version = version;
                let table = Arc::new(renderer::styles(palette));
                self.styles = Some((palette.dark, version, Arc::clone(&table)));
                (version, table)
            }
        }
    }
}

/// The keys the Viewport answers when no field has the keyboard
/// (functional spec, capability 54): F fits, Shift F zooms to the
/// selection, Escape clears it unless a menu or popup takes it.
pub(crate) fn keys(ctx: &egui::Context) -> Vec<Action> {
    if ctx.egui_wants_keyboard_input() {
        return Vec::new();
    }
    let popup_open = egui::Popup::is_any_open(ctx);
    let mut actions = Vec::new();
    ctx.input_mut(|input| {
        if input.consume_key(Modifiers::SHIFT, Key::F) {
            actions.push(Action::ZoomToSelection);
        }
        if input.consume_key(Modifiers::NONE, Key::F) {
            actions.push(Action::FitView);
        }
        if !popup_open && input.consume_key(Modifiers::NONE, Key::Escape) {
            actions.push(Action::ClearSelection);
        }
    });
    actions
}

/// The click mode of `modifiers`: Shift adds, Ctrl/⌘ toggles.
fn click_mode(modifiers: Modifiers) -> ClickMode {
    if modifiers.command {
        ClickMode::Toggle
    } else if modifiers.shift {
        ClickMode::Add
    } else {
        ClickMode::Replace
    }
}

/// Applies this frame's wheel, trackpad and pinch input over the pane to
/// `camera`, about the pointer at `at`.
fn scroll_and_pinch(ui: &Ui, pane: Rect, at: Pos, camera: &mut Camera) -> bool {
    let mut moved = false;
    ui.input(|input| {
        for event in &input.events {
            match *event {
                egui::Event::Zoom(factor) => {
                    camera.zoom_about(at, f64::from(factor));
                    moved = true;
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    source,
                    modifiers,
                    ..
                } => {
                    let points = match unit {
                        MouseWheelUnit::Point => delta,
                        MouseWheelUnit::Line => delta * POINTS_PER_LINE,
                        MouseWheelUnit::Page => delta * pane.height(),
                    };
                    let zooms = modifiers.command
                        || match source {
                            MouseWheelSource::Wheel => true,
                            MouseWheelSource::Trackpad | MouseWheelSource::Momentum => false,
                            MouseWheelSource::Unknown => unit != MouseWheelUnit::Point,
                        };
                    if zooms {
                        let notch = f64::from(points.y + points.x);
                        camera.zoom_about(at, (notch * ZOOM_PER_POINT).exp());
                    } else {
                        camera.pan(f64::from(points.x), f64::from(points.y));
                    }
                    moved = true;
                }
                _ => {}
            }
        }
    });
    moved
}

/// The pointer's gestures this frame: pans move `camera`, a click or a
/// marquee pushes a new selection. Returns whether the camera moved.
fn gestures(
    ui: &Ui,
    pane: Rect,
    response: &egui::Response,
    state: &FrameState<'_>,
    view: &mut ViewportState,
    camera: &mut Camera,
    out: &mut Vec<Action>,
) -> bool {
    let (pos, pressed, released, middle_pressed, middle_released, space, modifiers) =
        ui.input(|i| {
            (
                i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()),
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.button_released(PointerButton::Primary),
                i.pointer.button_pressed(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Middle),
                i.key_down(Key::Space),
                i.modifiers,
            )
        });
    let Some(pos) = pos else {
        view.drag = None;
        return false;
    };
    let mut moved = false;
    if (pressed || middle_pressed) && response.hovered() && view.drag.is_none() {
        view.drag = Some(if middle_pressed || space {
            Drag::Pan { last: pos }
        } else {
            match pick::pick(
                state.app.scene(),
                camera,
                pane_pos(pane, pos),
                PICK_TOLERANCE_PX,
                0,
            ) {
                Hit::Nothing => Drag::Marquee { start: pos },
                Hit::Instance(_) => Drag::Press { start: pos },
            }
        });
    }
    if let Some(Drag::Pan { last }) = view.drag {
        let delta = pos - last;
        if delta != egui::Vec2::ZERO {
            camera.pan(f64::from(delta.x), f64::from(delta.y));
            moved = true;
        }
        view.drag = Some(Drag::Pan { last: pos });
        if released || middle_released {
            view.drag = None;
        }
        return moved;
    }
    if !released {
        return moved;
    }
    let mode = click_mode(modifiers);
    let selection = state.app.selection();
    match view.drag.take() {
        Some(Drag::Marquee { start }) if start.distance(pos) >= CLICK_SLOP_PX => {
            let (a, b) = (
                camera.to_world(pane_pos(pane, start)),
                camera.to_world(pane_pos(pane, pos)),
            );
            let rect = MmRect {
                min: Point::new(a.x.min(b.x), a.y.min(b.y)),
                max: Point::new(a.x.max(b.x), a.y.max(b.y)),
            };
            let ids = pick::touching(state.app.scene(), rect);
            view.clicks.reset();
            out.push(Action::Select(selection.boxed(&ids, mode)));
        }
        // A drag that starts on a shape moves it, with the handles of
        // slice 5 (#534); until then it does nothing.
        Some(Drag::Press { start }) if start.distance(pos) >= CLICK_SLOP_PX => {}
        Some(Drag::Marquee { .. } | Drag::Press { .. }) => {
            let hit = view.clicks.click(
                state.app.scene(),
                camera,
                pane_pos(pane, pos),
                PICK_TOLERANCE_PX,
            );
            out.push(Action::Select(selection.clicked(&hit, mode)));
        }
        Some(Drag::Pan { .. }) | None => {}
    }
    moved
}

/// The GPU layers of this frame.
fn frame(
    state: &FrameState<'_>,
    view: &mut ViewportState,
    camera: &Camera,
    palette: &Palette,
) -> Frame {
    let app = state.app;
    let project = app.project();
    let scene = app.scene();
    let units = project.display_units;
    let grid = Grid::at(camera, units);
    let show_grid = app.view().visibility.grid;
    let band = layers::band(camera.px_per_mm);
    let tolerance = layers::tolerance_mm(band);
    let stock = (
        project.stock.width_mm.to_bits(),
        project.stock.height_mm.to_bits(),
    );
    let origin = project.origin.xy.point_mm(&project.stock);
    let origin_key = (origin.x.to_bits(), origin.y.to_bits());
    let selected = app.selection().instances().to_vec();
    let hover: Vec<InstanceId> = view
        .hover
        .iter()
        .filter(|id| !selected.contains(id))
        .cloned()
        .collect();

    let grid_layer = view.layer(
        Layer::Grid,
        key((stock, origin_key, show_grid, grid.step_mm.to_bits())),
        || layers::grid(project, show_grid.then_some(grid)),
    );
    let stock_layer = view.layer(Layer::Stock, key(stock), || layers::stock(project));
    let shapes_layer = view.layer(Layer::Shapes, key((scene.revision(), band)), || {
        layers::shapes(scene, tolerance)
    });
    let hover_layer = view.layer(Layer::Hover, key((scene.revision(), band, &hover)), || {
        layers::highlighted(
            scene,
            &hover,
            tolerance,
            (StyleId::Hover, StyleId::HoverOpen),
        )
    });
    let selection_layer = view.layer(
        Layer::Selection,
        key((scene.revision(), band, &selected)),
        || {
            layers::highlighted(
                scene,
                &selected,
                tolerance,
                (StyleId::Selected, StyleId::SelectedOpen),
            )
        },
    );
    Frame {
        centre_mm: [
            renderer::gpu_f32(camera.centre_mm.x),
            renderer::gpu_f32(camera.centre_mm.y),
        ],
        px_per_mm: renderer::gpu_f32(camera.px_per_mm),
        minor_alpha: renderer::gpu_f32(grid.minor_alpha),
        styles: view.styles(palette),
        layers: [
            grid_layer,
            stock_layer,
            shapes_layer,
            hover_layer,
            selection_layer,
        ],
    }
}

/// The overlay over the GPU layers: the origin crosshair and its label,
/// the park point, a padlock by each locked shape, and the marquee.
fn overlay(ui: &Ui, pane: Rect, state: &FrameState<'_>, view: &ViewportState, camera: &Camera) {
    let palette = Palette::current(ui);
    let painter = ui.painter_at(pane);
    let project = state.app.project();

    let origin = window_pos(pane, camera, project.origin.xy.point_mm(&project.stock));
    let axis = Stroke::new(1.0, palette.viewport_axis);
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let dir = egui::vec2(dx, dy);
        painter.line_segment(
            [
                origin + dir * CROSSHAIR_GAP_PX,
                origin + dir * CROSSHAIR_ARM_PX,
            ],
            axis,
        );
    }
    painter.text(
        origin + egui::vec2(-4.0, 4.0),
        egui::Align2::RIGHT_TOP,
        state.app.strings().get(StrKey::AppViewportOrigin),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );

    if let Some(park) = project.heights.park {
        let at = window_pos(pane, camera, Point::new(park.x_mm, park.y_mm));
        let stroke = Stroke::new(1.25, palette.viewport_park);
        painter.circle_stroke(at, PARK_RADIUS_PX, stroke);
        let d = PARK_RADIUS_PX * std::f32::consts::FRAC_1_SQRT_2;
        painter.line_segment([at + egui::vec2(-d, -d), at + egui::vec2(d, d)], stroke);
        painter.line_segment([at + egui::vec2(-d, d), at + egui::vec2(d, -d)], stroke);
    }

    for shape in state.app.scene().shapes().iter().filter(|s| s.locked) {
        let Some(bounds) = shape.bounds else {
            continue;
        };
        let corner = window_pos(pane, camera, bounds.max);
        let glyph = Icon::Lock.galley(ui, theme::ICON_SMALL_PX, palette.viewport_shape_locked);
        painter.galley(
            corner + egui::vec2(2.0, -glyph.size().y - 2.0),
            glyph,
            palette.viewport_shape_locked,
        );
    }

    if let Some(Drag::Marquee { start }) = view.drag
        && let Some(now) = ui.input(|i| i.pointer.interact_pos())
        && start.distance(now) >= CLICK_SLOP_PX
    {
        let rect = Rect::from_two_pos(start, now);
        painter.rect(
            rect,
            0.0,
            palette.accent_soft,
            Stroke::new(1.0, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
}

/// Draws the Viewport in the central panel; returns its rectangle.
pub(crate) fn show(
    ui: &mut Ui,
    state: &FrameState<'_>,
    view: &mut ViewportState,
    out: &mut Vec<Action>,
) -> Rect {
    let palette = Palette::current(ui);
    egui::CentralPanel::no_frame()
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(GUTTER_PX),
        )
        .show(ui, |ui| {
            let (pane, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            let app = state.app;
            let before = app.view().camera;
            let mut camera = before;
            let size = dc_geometry::Vec2::new(f64::from(pane.width()), f64::from(pane.height()));
            let resized = (camera.viewport_px.x - size.x).abs() > 0.5
                || (camera.viewport_px.y - size.y).abs() > 0.5;
            camera.viewport_px = size;
            if view.fit_pending || (view.fitted && resized) {
                camera.fit(camera::fit_bounds(app.project(), app.scene()));
                view.fit_pending = false;
                view.fitted = true;
            }

            let at = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| pane.contains(*p) && response.contains_pointer());
            let mut moved = false;
            if let Some(at) = at {
                moved |= scroll_and_pinch(ui, pane, pane_pos(pane, at), &mut camera);
            }
            moved |= gestures(ui, pane, &response, state, view, &mut camera, out);
            if moved {
                view.fitted = false;
            }
            if camera != before {
                out.push(Action::SetCamera(camera));
            }

            let painter = ui.painter_at(pane);
            painter.rect_filled(pane, 0.0, palette.viewport_bg);
            let stock = &app.project().stock;
            let sheet = Rect::from_two_pos(
                window_pos(pane, &camera, Point::new(0.0, 0.0)),
                window_pos(pane, &camera, Point::new(stock.width_mm, stock.height_mm)),
            );
            painter.rect_filled(sheet, 0.0, palette.viewport_stock_fill);
            let frame = frame(state, view, &camera, palette);
            painter.add(renderer::callback(pane, frame));
            overlay(ui, pane, state, view, &camera);

            if state.show_empty {
                empty::card(ui, pane, state, out);
            }
            pane
        })
        .inner
}
