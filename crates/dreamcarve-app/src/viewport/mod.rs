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
//! `dc_app`'s one selection. Transforms (#534): a drag from a shape moves
//! the selection, a corner handle scales it about its centre, the rotation
//! handle turns it; while the button is down the selection is drawn
//! through a preview `Transform` in the draw (no snapshot is made), and
//! the release commits one `TransformInstances`. The arrow keys nudge.
//! A locked selection's handles are grey and every transform is refused
//! with the reason in the status bar. Every position is the camera's: Y
//! flips once, in `dc_app::Camera::to_screen`, which the painter and the
//! pointer go through; the GPU never works in Y-down. While the Project
//! panel's *Click on the view* is armed, a click sets the park point
//! instead of the selection and Escape disarms it (#536).

mod layers;
pub(crate) mod renderer;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dc_app::viewport::camera::{self, Camera, Pos};
use dc_app::viewport::grid::Grid;
use dc_app::viewport::handles::{self, Grab, Handles};
use dc_app::viewport::pick::{self, ClickCycle, Hit, PICK_TOLERANCE_PX};
use dc_app::{App, ClickMode, Command, Selection, StrKey};
use dc_geometry::{Point, Rect as MmRect, Transform};
use dc_project::InstanceId;
use eframe::egui::{
    self, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, Stroke,
    TextStyle, Ui,
};

use crate::panels::{empty, project, properties};
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
/// A transform handle's square and circle, px (`experience.md`, section 8).
const HANDLE_PX: f32 = 8.0;
/// The arrow keys' nudge, mm: plain, with Shift, with Alt (capability 54).
const NUDGE_MM: f64 = 1.0;
/// The nudge with Shift held, mm.
const NUDGE_SHIFT_MM: f64 = 10.0;
/// The nudge with Alt held, mm.
const NUDGE_ALT_MM: f64 = 0.1;
/// A drag whose change is within this of none commits nothing.
const NO_CHANGE: f64 = 1e-12;

/// What a press on the pane took hold of.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    /// A shape: a click on release, a move once dragged.
    Shape(InstanceId),
    /// A corner or the rotation handle of the selection.
    Handle(Grab),
}

/// What the pointer is doing on the pane.
#[derive(Debug, Clone, PartialEq)]
enum Drag {
    /// Panning: the middle button, or the primary with Space held.
    Pan { last: Pos2 },
    /// The primary button pressed on nothing: a marquee once it moves.
    Marquee { start: Pos2 },
    /// The primary button pressed on a shape or a handle: a click on
    /// release, a transform once it moves.
    Press { start: Pos2, target: Target },
    /// Transforming `ids` by `grab` from `start`, previewed until release.
    Transform {
        start: Pos2,
        ids: Vec<InstanceId>,
        handles: Handles,
        grab: Grab,
    },
    /// A transform the selection refused (a locked shape): nothing until
    /// the release.
    Refused,
}

/// The selection drawn where the drag in progress will put it.
#[derive(Debug, Clone, PartialEq)]
struct Preview {
    ids: Vec<InstanceId>,
    /// Applied after each instance's own transform.
    placed: Transform,
    /// The change as the command will take it.
    delta: Transform,
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
    /// The transform being dragged, drawn but not yet a command.
    preview: Option<Preview>,
    /// The status bar's readout of the drag: ΔX ΔY, the scale, the angle.
    readout: Option<String>,
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
    /// The drag in progress, as the status bar reads it.
    pub drag: Option<String>,
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
        self.hover = match (over, &self.drag) {
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
            drag: self.readout.clone(),
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
/// selection, Escape clears it unless a menu or popup takes it, the arrow
/// keys nudge the selected shapes by 1 mm (10 with Shift, 0.1 with Alt),
/// one undo step each, and Delete or Backspace deletes them.
pub(crate) fn keys(ctx: &egui::Context, app: &App) -> Vec<Action> {
    if ctx.egui_wants_keyboard_input() {
        return Vec::new();
    }
    let popup_open = egui::Popup::is_any_open(ctx);
    let selected = app.selection().instances().to_vec();
    let park_picking = project::park_pick_armed(ctx);
    let mut actions = Vec::new();
    ctx.input_mut(|input| {
        if input.consume_key(Modifiers::SHIFT, Key::F) {
            actions.push(Action::ZoomToSelection);
        }
        if input.consume_key(Modifiers::NONE, Key::F) {
            actions.push(Action::FitView);
        }
        if !popup_open && input.consume_key(Modifiers::NONE, Key::Escape) {
            if park_picking {
                project::arm_park_pick(ctx, false);
            } else {
                actions.push(Action::ClearSelection);
            }
        }
        if selected.is_empty() || popup_open {
            return;
        }
        // Shift and Alt first: `consume_key` ignores extra Shift and Alt.
        for (key, (dx, dy)) in [
            (Key::ArrowLeft, (-1.0, 0.0)),
            (Key::ArrowRight, (1.0, 0.0)),
            (Key::ArrowDown, (0.0, -1.0)),
            (Key::ArrowUp, (0.0, 1.0)),
        ] {
            for (modifiers, step_mm) in [
                (Modifiers::SHIFT, NUDGE_SHIFT_MM),
                (Modifiers::ALT, NUDGE_ALT_MM),
                (Modifiers::NONE, NUDGE_MM),
            ] {
                while input.consume_key(modifiers, key) {
                    actions.push(Action::Run(Box::new(Command::TransformInstances {
                        ids: selected.clone(),
                        delta: Transform::translate(dx * step_mm, dy * step_mm),
                    })));
                }
            }
        }
        if input.consume_key(Modifiers::NONE, Key::Delete)
            || input.consume_key(Modifiers::NONE, Key::Backspace)
        {
            actions.push(Action::Run(Box::new(Command::DeleteInstances(
                selected.clone(),
            ))));
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

/// What a primary press at window point `pos` takes hold of: a handle of
/// the selection, else the shape under it, else nothing (a marquee).
fn press_target(state: &FrameState<'_>, camera: &Camera, pane: Rect, pos: Pos2) -> Option<Target> {
    let app = state.app;
    let at = pane_pos(pane, pos);
    let selected = app.selection().instances();
    if let Some(h) = handles::handles(app.project(), selected, camera)
        && let Some(grab @ (Grab::Corner(_) | Grab::Rotate { .. })) =
            handles::grab_at(&h, camera, at)
    {
        return Some(Target::Handle(grab));
    }
    match pick::pick(app.scene(), camera, at, PICK_TOLERANCE_PX, 0) {
        Hit::Instance(id) => Some(Target::Shape(id)),
        Hit::Nothing => None,
    }
}

/// A press on `target` has moved past the slop: the transform it starts,
/// or a refusal with the reason in the status bar. A press on a shape
/// outside the selection selects it first and moves it alone.
fn start_transform(
    state: &FrameState<'_>,
    camera: &Camera,
    start: Pos2,
    target: Target,
    out: &mut Vec<Action>,
) -> Drag {
    let project = state.app.project();
    let selected = state.app.selection().instances();
    let (ids, grab) = match target {
        Target::Shape(id) if selected.contains(&id) => (selected.to_vec(), Grab::Move),
        Target::Shape(id) => {
            out.push(Action::Select(Selection::Instances(vec![id.clone()])));
            (vec![id], Grab::Move)
        }
        Target::Handle(grab) => (selected.to_vec(), grab),
    };
    if let Some(refusal) = handles::refusal(project, &ids) {
        out.push(Action::Refused(refusal.to_string()));
        return Drag::Refused;
    }
    match handles::handles(project, &ids, camera) {
        Some(handles) => Drag::Transform {
            start,
            ids,
            handles,
            grab,
        },
        None => Drag::Refused,
    }
}

/// `value_mm` for the status bar's readout: one decimal in mm, three in
/// inches, as the cursor cell.
fn readout_length(value_mm: f64, units: dc_project::Units) -> String {
    let decimals = match units {
        dc_project::Units::Mm => 1,
        dc_project::Units::Inch => 3,
    };
    let text = format!("{:.decimals$}", dc_app::units::to_display(value_mm, units));
    if text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        text.trim_start_matches('-').to_owned()
    } else {
        text
    }
}

/// The status bar's text for a drag of `grab` giving `delta`.
fn readout(state: &FrameState<'_>, grab: Grab, delta: Transform) -> String {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    let [a, b, c, d, e, f] = delta.coeffs();
    match grab {
        Grab::Move => strings.fmt(
            StrKey::PlacementMove,
            &[
                ("dx", &readout_length(e, units)),
                ("dy", &readout_length(f, units)),
            ],
        ),
        Grab::Corner(_) => strings.fmt(
            StrKey::PlacementScale,
            &[
                ("x", &format!("{:.0}", a.hypot(b) * 100.0)),
                ("y", &format!("{:.0}", c.hypot(d) * 100.0)),
            ],
        ),
        Grab::Rotate { .. } => strings.fmt(
            StrKey::PlacementRotate,
            &[(
                "angle",
                &format!("{:.1}", dc_app::properties::rotation_deg(delta)),
            )],
        ),
    }
}

/// Whether `t` changes nothing.
fn is_identity(t: Transform) -> bool {
    t.coeffs()
        .iter()
        .zip(Transform::IDENTITY.coeffs())
        .all(|(a, b)| (a - b).abs() <= NO_CHANGE)
}

/// The pointer's gestures this frame: pans move `camera`, a click or a
/// marquee pushes a new selection, a drag from a shape or a handle
/// previews a transform and commits it on release. Returns whether the
/// camera moved.
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
        view.preview = None;
        view.readout = None;
        return false;
    };
    let mut moved = false;
    if (pressed || middle_pressed) && response.hovered() && view.drag.is_none() {
        view.drag = Some(if middle_pressed || space {
            Drag::Pan { last: pos }
        } else {
            match press_target(state, camera, pane, pos) {
                None => Drag::Marquee { start: pos },
                Some(target) => Drag::Press { start: pos, target },
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

    // A press that has moved 4 px starts its transform.
    if let Some(Drag::Press { start, target }) = &view.drag
        && start.distance(pos) >= CLICK_SLOP_PX
    {
        let (start, target) = (*start, target.clone());
        view.drag = Some(start_transform(state, camera, start, target, out));
        view.clicks.reset();
    }
    if let Some(Drag::Transform {
        start,
        ids,
        handles,
        grab,
    }) = &view.drag
    {
        let grab = match grab {
            Grab::Rotate { .. } => Grab::Rotate {
                snap: modifiers.shift,
            },
            other => *other,
        };
        let uniform = !properties::proportions_free(ui.ctx()) && !modifiers.alt;
        let from = camera.to_world(pane_pos(pane, *start));
        let to = camera.to_world(pane_pos(pane, pos));
        let delta = handles::drag_delta(handles, grab, from, to, uniform);
        view.readout = Some(readout(state, grab, delta));
        view.preview = Some(Preview {
            ids: ids.clone(),
            placed: handles::about_centre(handles, delta),
            delta,
        });
    }

    if !released {
        return moved;
    }
    let mode = click_mode(modifiers);
    let selection = state.app.selection();
    let preview = view.preview.take();
    view.readout = None;
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
        // The release commits the drag: one command, one undo step.
        Some(Drag::Transform { ids, .. }) => {
            if let Some(preview) = preview.filter(|p| !is_identity(p.delta)) {
                out.push(Action::Run(Box::new(Command::TransformInstances {
                    ids,
                    delta: preview.delta,
                })));
            }
        }
        Some(Drag::Marquee { .. } | Drag::Press { .. }) => {
            let hit = view.clicks.click(
                state.app.scene(),
                camera,
                pane_pos(pane, pos),
                PICK_TOLERANCE_PX,
            );
            out.push(Action::Select(selection.clicked(&hit, mode)));
        }
        Some(Drag::Pan { .. } | Drag::Refused) | None => {}
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
    // A drag in progress draws its shapes where the release will put them,
    // through the preview transform, and not where the snapshot has them.
    let moving: Vec<InstanceId> = view
        .preview
        .as_ref()
        .map(|p| p.ids.clone())
        .unwrap_or_default();
    let placed = view.preview.as_ref().map(|p| p.placed);
    let placed_key = placed.map(|t| t.coeffs().map(f64::to_bits));
    let shapes_layer = view.layer(
        Layer::Shapes,
        key((scene.revision(), band, &moving)),
        || layers::shapes(scene, tolerance, &moving),
    );
    let hover_layer = view.layer(Layer::Hover, key((scene.revision(), band, &hover)), || {
        layers::highlighted(
            scene,
            &hover,
            tolerance,
            (StyleId::Hover, StyleId::HoverOpen),
            None,
        )
    });
    let mut drawn = selected.clone();
    drawn.extend(moving.iter().filter(|id| !selected.contains(id)).cloned());
    let selection_layer = view.layer(
        Layer::Selection,
        key((scene.revision(), band, &drawn, &moving, placed_key)),
        || {
            layers::highlighted(
                scene,
                &drawn,
                tolerance,
                (StyleId::Selected, StyleId::SelectedOpen),
                placed.map(|t| (moving.as_slice(), t)),
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

/// The selection's transform handles (`experience.md`, section 8,
/// "Transform handles"; section 13, item 17): a 1-px accent box, four
/// 8-px corner squares and an 8-px rotation circle on its stem above the
/// top's centre, all in `shape.locked` grey when a shape is locked. While
/// a drag is in progress they follow its preview.
fn transform_handles(
    ui: &Ui,
    painter: &egui::Painter,
    pane: Rect,
    state: &FrameState<'_>,
    view: &ViewportState,
    camera: &Camera,
) {
    let palette = Palette::current(ui);
    let app = state.app;
    let ids = view
        .preview
        .as_ref()
        .map_or_else(|| app.selection().instances(), |p| p.ids.as_slice());
    if !ids.iter().any(|id| app.scene().shape(id).is_some()) {
        return;
    }
    let Some(h) = handles::handles(app.project(), ids, camera) else {
        return;
    };
    let placed = view
        .preview
        .as_ref()
        .map_or(Transform::IDENTITY, |p| p.placed);
    let at = |p: Point| window_pos(pane, camera, placed.apply(p));
    let colour = if h.locked {
        palette.viewport_shape_locked
    } else {
        palette.accent
    };
    let stroke = Stroke::new(1.0, colour);
    let corners: Vec<Pos2> = h.corners.iter().map(|c| at(*c)).collect();
    painter.add(egui::Shape::closed_line(corners.clone(), stroke));
    let top = Point::new((h.bounds.min.x + h.bounds.max.x) / 2.0, h.bounds.max.y);
    let (stem, knob) = (at(top), at(h.rotation));
    painter.line_segment([stem, knob], stroke);
    for corner in corners {
        painter.rect(
            Rect::from_center_size(corner, egui::vec2(HANDLE_PX, HANDLE_PX)),
            0.0,
            palette.surface_raised,
            stroke,
            egui::StrokeKind::Middle,
        );
    }
    painter.circle(knob, HANDLE_PX / 2.0, palette.surface_raised, stroke);
}

/// The overlay over the GPU layers: the origin crosshair and its label,
/// the park point, a padlock by each locked shape, the selection's
/// handles, and the marquee.
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

    transform_handles(ui, &painter, pane, state, view, camera);

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
            if project::park_pick_armed(ui.ctx()) && response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if project::park_pick_armed(ui.ctx())
                && response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                // The armed click is the park point's, not a selection.
                project::arm_park_pick(ui.ctx(), false);
                view.drag = None;
                view.clicks.reset();
                let at_mm = camera.to_world(pane_pos(pane, at));
                out.extend(project::park_at(app.project(), at_mm).map(Action::run));
            } else {
                moved |= gestures(ui, pane, &response, state, view, &mut camera, out);
            }
            if moved {
                view.fitted = false;
            }
            if camera != before {
                out.push(Action::SetCamera(camera));
            }
            match view.drag {
                Some(Drag::Refused) => ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed),
                Some(Drag::Transform { .. }) => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                _ => {}
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
