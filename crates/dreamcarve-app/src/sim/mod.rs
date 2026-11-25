// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Simulate step's Viewport (phase 6 functional spec, capabilities
//! 107 to 118; `docs/specs/phase-6/design.md`, "The renderer"): the
//! simulated stock in 3D, turned, slid and zoomed with the pointer, with
//! the transport that plays the job over it.
//!
//! [`SimView`] is the step's own state, the window's and never the
//! project's: the camera, the simulation being shown with its playback,
//! and the renderer. The simulation itself is `dc_app`'s
//! (`App::sim_built`); this follows it, so a newer one replaces what is
//! shown when it lands and the last one stays on screen until then,
//! dimmed. Nothing here is saved, and nothing here reaches a program
//! (S22).
//!
//! Without a wgpu device (egui's CPU test harness) there is no picture:
//! the card, the pills and the transport are drawn all the same, so the
//! window's tests read them.

mod controls;
pub(crate) mod renderer;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dc_app::App;
use dc_app::sim::{
    Blocked, Box3, Built, Hit, Layers, OrbitCamera, Shading, SimState, Speed, Transport, View3,
    probe, tool_mesh,
};
use dc_app::{StrKey, operations};
use dc_geometry::Point3;
use dc_job::sim::{CellSpec, Checkpoints, Grid, Heightfield, Playback, Quality};
use dc_project::{Board, OriginZ, Stock};
use eframe::egui::{self, MouseWheelUnit, Rect, Sense, Ui};
use eframe::egui_wgpu;

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::focus;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, shortcut_action};
use crate::viewport::{self, Margins};
use crate::widgets::card::mask_corners;
use renderer::{Extent, Line, Look, RenderError, Shot, StockRenderer};

/// Zoom per scrolled point: PureCutCNC's `exp(Δ · 0.0015)` (design,
/// "The camera and the read-out").
const ZOOM_PER_POINT: f64 = 0.0015;
/// How much of the last result shows while a newer one is made: it is
/// dimmed toward the Viewport's own colour.
const STALE_OPACITY: f32 = 0.55;
/// How much further back than a fit the view stands from places it was
/// asked to frame.
const FRAME_ROOM: f64 = 1.6;
/// The longest chord an arc of the toolpath is drawn with, mm, and the
/// most chords one arc gets.
const CHORD_MM: f64 = 1.0;
const MOST_CHORDS: u32 = 96;
/// The origin's axes, as a part of the board's longest side (upstream's
/// triad), and their least length, mm.
const AXIS_PART: f64 = 0.05;
const AXIS_MIN_MM: f64 = 5.0;

/// A hash of `value`: what a buffer on the GPU was made of.
fn key(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// A point as the renderer takes it: narrowed here, at its edge.
#[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
const fn narrowed(p: Point3) -> [f32; 3] {
    [p.x as f32, p.y as f32, p.z as f32]
}

/// The lines drawn over the stock (capability 117): the cutting moves of
/// the runs `shown` of `built` in their operation's colour and their
/// rapids dashed, as `layers` has them on, and the origin's three axes
/// from `origin`.
fn lines(
    built: Option<&Built>,
    shown: std::ops::Range<usize>,
    layers: Layers,
    palette: &Palette,
    origin: Option<(Point3, f64)>,
) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(built) = built.filter(|_| layers.toolpath || layers.rapids) {
        for (planned, run) in built
            .plan
            .runs
            .iter()
            .zip(built.runs.iter())
            .skip(shown.start)
            .take(shown.len())
        {
            let colour = palette.operation(operations::colour_at(planned.op_index));
            for mv in &run.moves {
                let cuts = mv.cuts();
                if !(if cuts { layers.toolpath } else { layers.rapids }) {
                    continue;
                }
                // An arc is drawn as chords; a straight move as itself.
                let chords = if mv.arc.is_some() {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a count of chords, held to a small range"
                    )]
                    let wanted = (mv.length_mm() / CHORD_MM).ceil().max(1.0) as u32;
                    wanted.min(MOST_CHORDS)
                } else {
                    1
                };
                for k in 0..chords {
                    let at = |k: u32| mv.point_at(f64::from(k) / f64::from(chords));
                    lines.push(Line {
                        a: narrowed(at(k)),
                        b: narrowed(at(k + 1)),
                        colour: if cuts { colour } else { palette.viewport_rapid },
                        dashed: !cuts,
                    });
                }
            }
        }
    }
    if let Some((zero, length_mm)) = origin.filter(|_| layers.origin) {
        for (dx, dy, dz) in [(1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)] {
            lines.push(Line {
                a: narrowed(zero),
                b: narrowed(Point3::new(
                    zero.x + dx * length_mm,
                    zero.y + dy * length_mm,
                    zero.z + dz * length_mm,
                )),
                colour: palette.viewport_axis,
                dashed: false,
            });
        }
    }
    lines
}

/// What the transport and the camera can be asked from outside the
/// Viewport: a key, a menu row, a row of the step's panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SimAct {
    /// Play when paused, pause when playing.
    PlayPause,
    /// The stock as it is this many seconds into the job.
    Scrub(f64),
    /// The stock as this run of the plan leaves it.
    ToRunEnd(usize),
    /// To the end of the operation before the playback's place.
    Previous,
    /// To the end of the operation at the playback's place.
    Next,
    /// This many cutting moves on, or back.
    StepMoves(i64),
    /// Play at this speed.
    Speed(Speed),
    /// One speed up.
    Faster,
    /// One speed down.
    Slower,
    /// Take up a named view.
    View(View3),
    /// Frame the stock.
    Fit,
    /// Frame this box: the places of an operation's uncut material.
    Frame(Box3),
}

/// What the rest of the window reads of the playback this frame: the
/// step panel's rows and the transport.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct SimFrame {
    /// The time into the job the stock is shown at, seconds.
    pub time_s: f64,
    /// The job's length, seconds.
    pub total_s: f64,
    /// Whether it is playing.
    pub playing: bool,
    /// Whether the stock shown is the finished one.
    pub finished: bool,
}

/// What an uncut stock is made of, to know when to make it again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Blank {
    width_mm: f64,
    height_mm: f64,
    thickness_mm: f64,
    quality: Quality,
}

/// The stock on screen and its playback.
#[derive(Debug)]
struct Shown {
    /// The simulation shown; `None` for the uncut stock of a project that
    /// cannot be simulated.
    built: Option<Arc<Built>>,
    /// What the uncut stock was made of.
    blank: Option<Blank>,
    transport: Transport,
    shading: Shading,
    /// Whether every cell is on the GPU.
    uploaded: bool,
}

/// The Simulate step's view state.
#[derive(Default)]
pub(crate) struct SimView {
    /// The window's wgpu state; `None` under the CPU test harness.
    render_state: Option<egui_wgpu::RenderState>,
    renderer: Option<StockRenderer>,
    camera: Option<OrbitCamera>,
    /// The box the camera last framed, to fit again when it changes.
    framed: Option<Box3>,
    /// Whether the view is as Fit left it: it then follows the card's
    /// size and the stock's.
    fitted: bool,
    fit_pending: bool,
    shown: Option<Shown>,
    /// What the pointer is over.
    hover: Option<Hit>,
    /// Why the stock cannot be drawn, when it cannot.
    error: Option<RenderError>,
    /// Whether the stock is being dragged, for the floating controls.
    dragging: bool,
}

impl std::fmt::Debug for SimView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimView")
            .field("camera", &self.camera)
            .field("fitted", &self.fitted)
            .finish_non_exhaustive()
    }
}

impl SimView {
    /// The view of a window whose wgpu state is `render_state`.
    pub(crate) fn new(render_state: Option<&egui_wgpu::RenderState>) -> Self {
        Self {
            render_state: render_state.cloned(),
            fitted: true,
            ..Self::default()
        }
    }

    /// What the playback shows this frame.
    pub(crate) fn frame_info(&self) -> SimFrame {
        self.shown.as_ref().map_or_else(SimFrame::default, |shown| {
            let transport = &shown.transport;
            SimFrame {
                time_s: transport.shown_s(),
                total_s: transport.total_s(),
                playing: transport.playing(),
                finished: transport.playback().is_finished(),
            }
        })
    }

    /// The camera, once a stock has been shown.
    pub(crate) const fn camera(&self) -> Option<OrbitCamera> {
        self.camera
    }

    /// The speed the transport plays at, when there is a job.
    pub(crate) fn speed(&self) -> Option<Speed> {
        self.shown.as_ref().map(|shown| shown.transport.speed())
    }

    /// Follows `app`'s simulation: a newer one of the board shown takes
    /// the place of what is on screen, at its end; a project that cannot
    /// be simulated shows its stock uncut (capability 108).
    pub(crate) fn follow(&mut self, app: &App) {
        let refused = matches!(
            app.sim_blocked(),
            Some(Blocked::NoOperations | Blocked::Errors(_))
        );
        if let Some(built) = app.sim_built().filter(|_| !refused) {
            let same = self
                .shown
                .as_ref()
                .and_then(|shown| shown.built.as_ref())
                .is_some_and(|shown| Arc::ptr_eq(shown, built));
            if !same {
                let playback = built.playback();
                let speed = app
                    .prefs()
                    .sim
                    .speed
                    .unwrap_or_else(|| Speed::opening(playback.timeline().total_s()));
                self.shown = Some(Shown {
                    built: Some(Arc::clone(built)),
                    blank: None,
                    transport: Transport::new(playback, speed),
                    shading: Shading::default(),
                    uploaded: false,
                });
            }
            return;
        }
        // The stock as it lies, uncut: only the project's own board has
        // a size before a simulation says the male board's.
        let stock = &app.project().stock;
        let blank = stock
            .thickness_mm
            .filter(|_| app.sim_board() == Board::Female)
            .map(|thickness_mm| Blank {
                width_mm: stock.width_mm,
                height_mm: stock.height_mm,
                thickness_mm,
                quality: app.prefs().sim.quality,
            });
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.built.is_none())
            && self.shown.as_ref().and_then(|shown| shown.blank) == blank
        {
            return;
        }
        self.shown = blank.and_then(|blank| {
            let grid = Grid::of(
                blank.width_mm,
                blank.height_mm,
                CellSpec::Quality(blank.quality),
            )
            .ok()?;
            let field = Heightfield::uncut(grid, blank.thickness_mm).ok()?;
            let playback = Playback::at_end(
                Arc::from(Vec::new()),
                Checkpoints::default(),
                Arc::new(field),
            );
            Some(Shown {
                built: None,
                blank: Some(blank),
                transport: Transport::new(playback, Speed::default()),
                shading: Shading::default(),
                uploaded: false,
            })
        });
    }

    /// The box of the stock shown, from the bed to its top.
    fn bounds(&self) -> Option<Box3> {
        let field = self.shown.as_ref()?.transport.field();
        let grid = field.grid();
        Some(Box3 {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(grid.width_mm(), grid.height_mm(), field.top_z_mm()),
        })
    }

    /// The runs of the plan that end an operation: where Previous and
    /// Next stop.
    fn operation_ends(&self) -> Vec<usize> {
        let Some(built) = self.shown.as_ref().and_then(|shown| shown.built.as_ref()) else {
            return Vec::new();
        };
        let runs = &built.plan.runs;
        runs.iter()
            .enumerate()
            .filter(|(k, run)| runs.get(k + 1).is_none_or(|next| next.op != run.op))
            .map(|(k, _)| k)
            .collect()
    }

    /// Does what a key, a menu row or the step's panel asked.
    pub(crate) fn act(&mut self, act: SimAct) {
        let ends = self.operation_ends();
        let bounds = self.bounds();
        match act {
            SimAct::View(view) => {
                if let (Some(camera), Some(bounds)) = (self.camera.as_mut(), bounds) {
                    camera.preset(view);
                    camera.fit(bounds);
                    self.fitted = true;
                }
                return;
            }
            SimAct::Fit => {
                self.fit_pending = true;
                return;
            }
            SimAct::Frame(around) => {
                if let Some(camera) = self.camera.as_mut() {
                    camera.fit(around);
                    // Room round the places, for what they are part of
                    // and for the pills at the card's edges.
                    camera.distance_mm *= FRAME_ROOM;
                    self.fitted = false;
                }
                return;
            }
            _ => {}
        }
        let Some(shown) = self.shown.as_mut() else {
            return;
        };
        let transport = &mut shown.transport;
        match act {
            SimAct::PlayPause => transport.toggle(),
            SimAct::Scrub(t_s) => transport.scrub(t_s),
            SimAct::ToRunEnd(run) => transport.to_run_end(run),
            SimAct::StepMoves(n) => transport.step_moves(n),
            SimAct::Speed(speed) => transport.set_speed(speed),
            SimAct::Faster => transport.set_speed(transport.speed().faster()),
            SimAct::Slower => transport.set_speed(transport.speed().slower()),
            SimAct::Previous | SimAct::Next => {
                // The run being cut, or the one about to be: the runs
                // before it are whole.
                let timeline = transport.playback().timeline();
                let now = transport.shown_s();
                let at = (0..timeline.runs())
                    .find(|run| {
                        timeline
                            .run_span_s(*run)
                            .is_some_and(|(_, end)| end > now + 1e-9)
                    })
                    .unwrap_or_else(|| timeline.runs());
                if act == SimAct::Next {
                    if let Some(end) = ends.iter().find(|end| **end >= at) {
                        transport.to_run_end(*end);
                    }
                } else if let Some(end) = ends.iter().rev().find(|end| **end + 1 < at) {
                    transport.to_run_end(*end);
                } else {
                    transport.scrub(0.0);
                }
            }
            SimAct::View(_) | SimAct::Fit | SimAct::Frame(_) => {}
        }
    }

    /// One frame of the stock in `pane`: the playback moved on, the
    /// changed cells sent to the GPU, the pointer's gestures taken, and
    /// the picture drawn.
    fn stock(&mut self, ui: &Ui, pane: Rect, response: &egui::Response, state: &FrameState<'_>) {
        let palette = Palette::current(ui);
        let prefs = state.prefs.sim;
        let Some(bounds) = self.bounds() else {
            return;
        };
        if self.renderer.is_none()
            && let Some(render_state) = &self.render_state
        {
            self.renderer = Some(StockRenderer::new(render_state));
        }
        let Some(shown) = self.shown.as_mut() else {
            return;
        };
        // The playback, by the time since the last frame.
        let dt_s = f64::from(ui.input(|i| i.stable_dt));
        if shown.transport.frame(dt_s) {
            ui.ctx().request_repaint();
        }
        let dirty = shown.transport.take_dirty();
        if let Some(renderer) = self.renderer.as_mut() {
            let flags = shown
                .built
                .as_ref()
                .map(|built| built.comparison.flags.as_slice());
            let field = shown.transport.field();
            let sent = if !shown.uploaded {
                shown.shading = Shading::of(field);
                shown.uploaded = true;
                renderer.upload(field, &shown.shading, flags, None)
            } else if let Some(changed) = dirty {
                let around = shown.shading.update(field, changed);
                renderer.upload(field, &shown.shading, flags, Some(around))
            } else {
                Ok(())
            };
            self.error = sent.err();
        }

        // The camera: the card's size, Fit, then the pointer.
        let viewport_px = (f64::from(pane.width()), f64::from(pane.height()));
        let camera = self
            .camera
            .get_or_insert_with(|| OrbitCamera::framing(bounds, viewport_px));
        let resized = (camera.viewport_px.0 - viewport_px.0).abs() > 0.5
            || (camera.viewport_px.1 - viewport_px.1).abs() > 0.5;
        camera.viewport_px = viewport_px;
        if self.fit_pending || (self.fitted && (resized || self.framed != Some(bounds))) {
            camera.fit(bounds);
            self.fit_pending = false;
            self.fitted = true;
        }
        self.framed = Some(bounds);
        let at = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| pane.contains(*p) && response.contains_pointer())
            .map(|p| (f64::from(p.x - pane.left()), f64::from(p.y - pane.top())));
        let mut moved = false;
        if response.dragged() {
            let delta = response.drag_delta();
            let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
            // Capability 111: the right or middle button, or Shift,
            // slides; a plain drag turns.
            let slides = response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || ui.input(|i| i.modifiers.shift);
            if slides {
                camera.pan(dx, dy);
            } else {
                camera.orbit(dx, dy);
            }
            moved = dx.abs() > 0.0 || dy.abs() > 0.0;
        }
        self.dragging = response.dragged();
        if let Some(at) = at {
            ui.input(|input| {
                for event in &input.events {
                    match *event {
                        // A pinch's factor is over 1 to come closer.
                        egui::Event::Zoom(factor) if factor > 0.0 => {
                            camera.zoom_about(at, 1.0 / f64::from(factor));
                            moved = true;
                        }
                        egui::Event::MouseWheel { unit, delta, .. } => {
                            let points = match unit {
                                MouseWheelUnit::Point => delta,
                                MouseWheelUnit::Line => delta * viewport::POINTS_PER_LINE,
                                MouseWheelUnit::Page => delta * pane.height(),
                            };
                            let notch = f64::from(points.y + points.x);
                            camera.zoom_about(at, (-notch * ZOOM_PER_POINT).exp());
                            moved = true;
                        }
                        _ => {}
                    }
                }
            });
        }
        if moved {
            self.fitted = false;
        }
        let camera = *camera;

        // What the pointer is over, for the read-out.
        let field = shown.transport.field();
        self.hover = at
            .filter(|_| !response.dragged())
            .and_then(|at| probe(field, camera.ray(at)));

        // The picture.
        let Some(renderer) = self.renderer.as_mut().filter(|_| self.error.is_none()) else {
            return;
        };
        let Some(render_state) = &self.render_state else {
            return;
        };
        let grid = field.grid();
        let finished = shown.transport.playback().is_finished();
        // The tool, where the playback has it (capability 115).
        let pose = shown.transport.pose().filter(|_| prefs.layers.tool);
        let tool_tip = pose.zip(shown.built.as_ref()).and_then(|(pose, built)| {
            let cutter = built.runs.get(pose.run)?.cutter.cutter;
            renderer.set_tool(key((Arc::as_ptr(built) as usize, pose.run)), || {
                tool_mesh(&cutter)
            });
            Some(narrowed(pose.tip))
        });
        // The toolpath of the operation being shown, and the origin.
        let project = state.app.project();
        let board = Stock {
            width_mm: grid.width_mm(),
            height_mm: grid.height_mm(),
            thickness_mm: Some(field.top_z_mm()),
        };
        let zero = project.origin.xy.point_mm(&board);
        let zero = Point3::new(
            zero.x,
            zero.y,
            match project.origin.z {
                OriginZ::StockTop => field.top_z_mm(),
                OriginZ::StockBottom => 0.0,
            },
        );
        let axis_mm = (grid.width_mm().max(grid.height_mm()) * AXIS_PART).max(AXIS_MIN_MM);
        let run_shown = shown.built.as_ref().and_then(|built| {
            let runs = &built.plan.runs;
            let last = runs.len().checked_sub(1)?;
            // The run being cut; at the moment between two, the one just
            // cut, whose path lies on the stock it left.
            let cursor = shown.transport.playback().cursor();
            let between = cursor.mv == 0 && cursor.fraction <= 0.0;
            let at = if finished {
                last
            } else if between {
                cursor.run.saturating_sub(1).min(last)
            } else {
                cursor.run.min(last)
            };
            let op = &runs.get(at)?.op;
            let first = runs.iter().position(|run| run.op == *op)?;
            let count = runs
                .iter()
                .skip(first)
                .take_while(|run| run.op == *op)
                .count();
            Some(first..first + count)
        });
        let layers = prefs.layers;
        let built = shown.built.as_deref();
        let run_range = run_shown.clone().unwrap_or(0..0);
        let lines_key = key((
            shown
                .built
                .as_ref()
                .map(|built| Arc::as_ptr(built) as usize),
            run_range.start,
            (layers.toolpath, layers.rapids, layers.origin, palette.dark),
            (zero.x.to_bits(), zero.y.to_bits(), zero.z.to_bits()),
        ));
        renderer.set_lines(lines_key, || {
            lines(built, run_range, layers, palette, Some((zero, axis_mm)))
        });
        let look = Look {
            background: palette.viewport_bg,
            stock: palette.sim_stock,
            bed: prefs.layers.bed.then_some(palette.sim_bed),
            colouring: prefs.colouring,
            runs: shown.built.as_ref().map_or_else(Vec::new, |built| {
                built
                    .plan
                    .runs
                    .iter()
                    .map(|run| palette.operation(operations::colour_at(run.op_index)))
                    .collect()
            }),
            ramp: palette.depth_ramp,
            // The check is of the finished stock (capability 120).
            uncut: (prefs.uncut_shown && finished && shown.built.is_some())
                .then_some(palette.status_warning.stroke),
            tool: (palette.sim_tool, palette.sim_tool_shank),
        };
        // The bed reaches past the board: the far plane must hold it.
        let margin = (grid.width_mm().max(grid.height_mm()) * 0.05).max(10.0);
        let scene = Box3 {
            min: Point3::new(-margin, -margin, 0.0),
            max: Point3::new(bounds.max.x + margin, bounds.max.y + margin, bounds.max.z),
        };
        let pixels_per_point = ui.ctx().pixels_per_point();
        // Whole pixels, a card being a few thousand at most.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a card's size in pixels, never negative"
        )]
        let size_px = [
            (pane.width() * pixels_per_point).round().max(0.0) as u32,
            (pane.height() * pixels_per_point).round().max(0.0) as u32,
        ];
        // Where the depth ramp ends: on the stock, where phase 4's legend
        // ends, the deepest toolpath of the project; on the male board,
        // which that legend leaves out, its own deepest cut.
        let ramp = dc_app::view::DepthRamp::of(project, state.app.scene())
            .filter(|_| state.app.sim_board() == Board::Female);
        let deepest_mm = ramp.map_or_else(
            || {
                shown
                    .built
                    .as_ref()
                    .map_or(0.0, |built| built.facts.deepest_mm)
            },
            |ramp| ramp.top_z_mm - ramp.deepest_z_mm,
        );
        // The renderer's edge: lengths are narrowed here.
        #[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
        let extent = Extent {
            cell_mm: grid.cell_mm() as f32,
            top_mm: field.top_z_mm() as f32,
            width_mm: grid.width_mm() as f32,
            height_mm: grid.height_mm() as f32,
            deepest_mm: deepest_mm as f32,
        };
        let shot = Shot {
            view_projection: camera.view_projection(scene).to_columns_f32(),
            size_px,
            tool_tip,
            pixels_per_point,
        };
        if let Some(texture) = renderer.draw(render_state, shot, &look, extent) {
            ui.painter_at(pane).image(
                texture,
                pane,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                theme::IMAGE_TINT,
            );
        }
    }
}

/// The Simulate step's keys (capability 126), read while no field is
/// typed in and no menu is open: the transport's, the views' and Fit.
/// They are taken before the window's controls see them, so Space plays
/// and does not also press the button a click left the focus on; a
/// control reached with Tab keeps Space for itself.
pub(crate) fn keys(ctx: &egui::Context, app: &App) -> Vec<Action> {
    if ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) {
        return Vec::new();
    }
    let space_free = focus::space_free(ctx);
    let mut pressed = Vec::new();
    ctx.input_mut(|input| {
        for command in [
            Shortcut::SimPlayPause,
            Shortcut::SimPrevious,
            Shortcut::SimNext,
            Shortcut::SimStepBackTen,
            Shortcut::SimStepForwardTen,
            Shortcut::SimStepBack,
            Shortcut::SimStepForward,
            Shortcut::SimSlower,
            Shortcut::SimFaster,
            Shortcut::ViewHome,
            Shortcut::ViewTop,
            Shortcut::ViewFront,
            Shortcut::ViewRight,
            Shortcut::FitView,
        ] {
            if command == Shortcut::SimPlayPause && !space_free {
                continue;
            }
            for _ in 0..keyboard::consume(input, command) {
                pressed.push(command);
            }
        }
    });
    pressed
        .into_iter()
        .filter_map(|command| shortcut_action(app, command))
        .collect()
}

/// The one sentence the card shows in place of a stock.
fn sentence(ui: &Ui, pane: Rect, text: &str) {
    let palette = Palette::current(ui);
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        palette.text_secondary,
        (pane.width() - 2.0 * theme::CARD_PADDING_PX).max(0.0),
    );
    let at = pane.center() - galley.size() / 2.0;
    ui.painter_at(pane)
        .galley(at, galley, palette.text_secondary);
}

/// Draws the Simulate step's Viewport in the central panel, `margins`
/// from the cards beside it; returns its rectangle.
pub(crate) fn show(
    ui: &mut Ui,
    state: &FrameState<'_>,
    view: &mut SimView,
    margins: Margins,
    out: &mut Vec<Action>,
) -> Rect {
    let palette = Palette::current(ui);
    egui::CentralPanel::no_frame()
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(margins.frame(0, viewport::BOTTOM_PX)),
        )
        .show(ui, |ui| {
            let strings = state.app.strings();
            let (pane, response) =
                ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
            let name = strings.get(StrKey::SimViewName);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, name));
            focus::outline(ui.ctx(), response.id, pane, theme::CARD_RADIUS_PX);
            let painter = ui.painter_at(pane);
            painter.rect_filled(pane, 0.0, palette.viewport_bg);
            view.stock(ui, pane, &response, state);
            if view.shown.is_none() {
                sentence(ui, pane, strings.get(StrKey::SimViewNoStock));
            } else if view.error.is_some() {
                sentence(ui, pane, strings.get(StrKey::SimViewTooFine));
            } else if !matches!(state.app.sim_state(), SimState::Current)
                && view
                    .shown
                    .as_ref()
                    .is_some_and(|shown| shown.built.is_some())
            {
                // The last result, dimmed, while a newer one is made.
                painter.rect_filled(
                    pane,
                    0.0,
                    palette.viewport_bg.gamma_multiply(1.0 - STALE_OPACITY),
                );
            }
            let opacity = viewport::controls::opacity(ui, view.dragging);
            let place = controls::Place {
                pane,
                right: pane.right() - margins.overlay_right,
                bottom: pane.bottom() - margins.overlay_bottom,
                opacity,
            };
            controls::show(ui, place, state, view, out);
            // Capability 116: the depth legend while By depth is on, hung
            // under Colour as the 2D Viewport hangs it. It is the
            // stock's: the male board is cut from its own top.
            let has_job = view
                .shown
                .as_ref()
                .is_some_and(|shown| shown.built.is_some());
            if has_job
                && state.prefs.sim.colouring == dc_app::sim::Colouring::Depth
                && state.app.sim_board() == Board::Female
                && let Some(legend) =
                    dc_app::view::depth_legend(state.app.project(), state.app.scene())
            {
                let mut fading = painter.clone();
                fading.set_opacity(opacity);
                let hung = egui::pos2(
                    place.right - theme::FLOAT_INSET_PX,
                    pane.top()
                        + theme::FLOAT_INSET_PX
                        + viewport::controls::PILL_PX
                        + viewport::controls::LEGEND_GAP_PX,
                );
                viewport::legend::draw_legend(ui, &fading, hung, state.app, &legend);
            }
            // The Viewport is a card: its corners go back to the
            // backdrop.
            mask_corners(&painter, pane, theme::CARD_RADIUS_PX, palette);
            pane
        })
        .inner
}
