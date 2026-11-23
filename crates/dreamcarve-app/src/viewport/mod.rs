// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport (functional spec, capabilities 72 and 75; `experience.md`,
//! section 9; `docs/specs/phase-4/design.md`, "The Viewport renderer"):
//! the stage, the stock sheet, and the wgpu line layers ([`renderer`])
//! for the grid, the stock's outline, the shapes, the hover and the
//! selection, with the overlay egui's painter draws on top (the origin
//! crosshair and its label, the park point, the padlocks, the marquee)
//! and the start screen in its place while the project has nothing to cut.
//!
//! Navigation: a middle-button drag, a Space-drag or a two-finger scroll
//! pans; the wheel, Ctrl/⌘-scroll and a pinch zoom about the cursor
//! (open question 8). Selection: a click picks through
//! `dc_app::viewport::pick`, a second click in place cycles, Shift adds,
//! Ctrl/⌘ toggles, a drag from empty stock is a marquee, and the result is
//! `dc_app`'s one selection. A click on a drawn cut away from any outline
//! opens its operation in the editor and leaves the selection (#576).
//! Transforms (#534): a drag from a shape moves
//! the selection, a corner handle scales it about its centre, the rotation
//! handle turns it; while the button is down the selection is drawn
//! through a preview `Transform` in the draw (no snapshot is made), and
//! the release commits one `TransformInstances`. The arrow keys nudge.
//! A locked selection's handles are grey and every transform is refused
//! with the reason in the status bar. Every position is the camera's: Y
//! flips once, in `dc_app::Camera::to_screen`, which the painter and the
//! pointer go through; the GPU never works in Y-down. While the Export
//! step's *Click on the view* is armed, a click sets the park point
//! instead of the selection and Escape disarms it (#536). While an
//! operation is selected, hovered in the list or having its targets
//! edited, its targets draw over the toolpaths in its colour and the
//! other shapes fade (slice 27, #619; capability 67); in target editing
//! a bar across the top of the pane says so ([`target_bar`]), a click
//! toggles the shape under it, a marquee adds what it touches and an
//! Alt-marquee removes, and nothing is selected or moved. While a point row's
//! *Pick on the view* is armed, the next click sets that point (a
//! Profile's start point) and disarms it, as the park point's does (#540).
//! Placed tabs (#546): a press on a tab (tabs win over the shape they sit
//! on) drags it along its outline, previewed live and committed as one
//! `MoveTab` on release; while the Tab tool is armed (its button, T) a
//! click on an outline adds a tab there, until Escape; Delete removes the
//! selected tabs ([`tab_tool`]).
//! Pictures ([`pictures`]; capabilities 56 to 58): each visible bitmap
//! draws as a textured quad under the shapes; a click inside one where
//! nothing above takes it selects it, a marquee that touches no shape
//! takes the pictures it touches, a drag moves the selected pictures, the
//! one selected picture's corner handles resize it with its proportions
//! kept, and the arrow keys nudge.
//!
//! The Viewport is a card. Its controls float over it ([`controls`];
//! `redesign.md`, "The Viewport's floating controls"): the tools and the
//! zoom at its top left, the colour mode and *Layers* at its top right
//! with the depth legend under them, the cursor's read-out at its bottom
//! left. Fit keeps the stock clear of them. While the project has nothing
//! to cut and the Design step is open, the card shows the start screen in
//! place of the stock ([`crate::panels::empty`]).

mod controls;
mod draft;
mod layers;
mod legend;
mod lod;
mod pictures;
pub(crate) mod renderer;
pub(crate) mod tab_tool;
pub(crate) mod tool_bar;

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dc_app::view::{ColourMode, DepthLegend, DepthRamp, ProgramExtents};
use dc_app::viewport::arrows::{ArrowCache, ArrowMove};
use dc_app::viewport::camera::{self, Camera, Insets, Pos};
use dc_app::viewport::grid::Grid;
use dc_app::viewport::handles::{self, Grab, Handles};
use dc_app::viewport::pick::{self, ClickCycle, Hit, PICK_TOLERANCE_PX};
use dc_app::{App, ClickMode, Command, Selection, StrKey, Tool};
use dc_geometry::{Point, Rect as MmRect, Transform};
use dc_project::{BitmapId, InstanceId, OperationId, TabId};
use eframe::egui::{
    self, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Rect, Stroke,
    TextStyle, Ui,
};

use crate::keyboard::{self, Command as Shortcut};
use crate::panels::{editor, empty, project, properties};
use crate::shell::focus;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, PasteAt, shortcut_action};
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button};
use crate::widgets::card::mask_corners;
use crate::widgets::icon::Icon;
pub use controls::FIT_INSETS;
pub(crate) use controls::held_drawing;
pub(crate) use draft::kind_name as snap_kind_name;
use renderer::{Frame, Layer, LayerData, StyleId, StyleTable, ToolpathData};

/// What the Viewport leaves of the gap to the card on each side, px: 8
/// at the window's edge or beside the rail, less beside a resizable
/// panel, which keeps the rest (`panels::side`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Margins {
    /// Left of the card.
    pub left: f32,
    /// Right of the card.
    pub right: f32,
    /// How much of the card's right side an Inspector lies over, px: in a
    /// window too narrow to hold it beside the Viewport. The floating
    /// controls and Fit keep clear of it.
    pub overlay_right: f32,
    /// How much of the card's bottom the Warnings drawer lies over, px,
    /// while it is open. The read-out and Fit keep clear of it.
    pub overlay_bottom: f32,
}

impl Margins {
    /// The margins as a frame's, over a `bottom` one.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a gap of at most 8 px, whole"
    )]
    pub(crate) fn frame(self, top: i8, bottom: i8) -> egui::Margin {
        egui::Margin {
            left: self.left as i8,
            right: self.right as i8,
            top,
            bottom,
        }
    }
}

/// The margin under the card, px: the status bar's pills stand 2 px in
/// from its top, which makes the gap.
const BOTTOM_PX: i8 = 6;
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
/// The male board's origin mark, smaller than the stock's: its arm and
/// its centre gap, px.
const MALE_CROSSHAIR_ARM_PX: f32 = 8.0;
const MALE_CROSSHAIR_GAP_PX: f32 = 2.0;
/// The target-editing bar's height, px (`experience.md`, section 3).
const TARGET_BAR_PX: f32 = 48.0;
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
    /// A placed tab: a click on release, a drag along its outline once
    /// moved.
    Tab(TabId),
    /// A picture: a click on release, a move once dragged.
    Picture(BitmapId),
    /// A corner handle of the one selected picture, by its place among
    /// its corners: a resize once dragged.
    PictureCorner(usize),
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
    /// Dragging placed tab `id` along its outline, previewed until release.
    Tab { id: TabId },
    /// Moving or resizing the pictures `ids` by `grip` from `start`,
    /// previewed until release.
    Picture {
        start: Pos2,
        ids: Vec<BitmapId>,
        grip: pictures::Grip,
    },
    /// The primary button down on the pane while a drawing tool is armed:
    /// the press went to the tool, and so will the release.
    Tool,
}

/// The selection drawn where the drag in progress will put it.
#[derive(Debug, Clone, PartialEq)]
struct Preview {
    ids: Vec<InstanceId>,
    /// Applied after each instance's own transform.
    placed: Transform,
    /// The change as the command will take it.
    delta: Transform,
    /// A move with Alt held: the release leaves linked copies there and
    /// the originals where they are (phase 5, capability 98), so both
    /// are drawn.
    copying: bool,
}

/// Where the Viewport's context menu was opened.
#[derive(Debug, Clone, PartialEq)]
struct MenuAt {
    /// The shape under the pointer then, if any.
    on: Option<InstanceId>,
    /// The pointer then, project mm: where the menu's Paste pastes.
    at_mm: Point,
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
    /// The ways Fit can stand clear of what floats over the pane, as the
    /// last frame laid it out ([`controls::ways`]).
    ways: Vec<Insets>,
    /// How much of the pane's bottom the Warnings drawer lay over in the
    /// last frame, px: the pointer there is the drawer's, not the
    /// drawing's.
    covered_bottom: f32,
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
    /// The placed tab under the pointer.
    hover_tab: Option<TabId>,
    /// The picture under the pointer, where nothing above it is.
    hover_picture: Option<BitmapId>,
    /// The pictures being dragged and where the release will put them.
    picture_preview: Option<pictures::Preview>,
    /// The pictures' mip chains, built once per stored picture.
    picture_mips: pictures::Mips,
    /// The tab being dragged and where the release will put it, in
    /// definition space.
    tab_preview: Option<(TabId, f64, f64)>,
    /// Where the context menu was last opened.
    menu: Option<MenuAt>,
    /// Where the armed tool was last told the pointer is: it is told
    /// again only when the pointer, or a key held with it, changes.
    tool_pointer: Option<dc_app::Pointer>,
    layers: [Cached; Layer::ALL.len()],
    /// Each operation's toolpath buffer as last built.
    toolpaths: BTreeMap<OperationId, Cached>,
    /// The direction arrows' placements, per run.
    arrows: ArrowCache,
    /// Each run's moves as the arrow pass reads them, by run key and
    /// zoom band.
    arrow_moves: BTreeMap<u64, (i32, Arc<Vec<ArrowMove>>)>,
    /// The depth ramp, its legend and the program extents of a Scene
    /// revision: scanning every move is not a per-frame cost.
    derived: Option<Derived>,
    styles: Option<(bool, u64, Arc<StyleTable>)>,
    next_version: u64,
    /// The operations whose toolpaths the last frame drew, bottom first.
    drawn: Vec<OperationId>,
}

/// What the Viewport reads of one Scene revision besides its layers.
#[derive(Debug, Clone)]
struct Derived {
    revision: u64,
    ramp: Option<DepthRamp>,
    legend: Option<DepthLegend>,
    extents: Option<ProgramExtents>,
}

/// What the status bar shows of the pointer: the name of the shape under
/// it, locked or not, and the drag in progress. Its project point is the
/// Viewport's own read-out's ([`controls`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Pointer {
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

    /// The ways Fit can stand clear of what floats over the pane: the
    /// room the floating controls keep, and beside or under the depth
    /// legend while it shows. Fit takes the one that draws the stock
    /// largest (`Camera::fit_clear`).
    pub(crate) fn fit_ways(&self) -> Vec<Insets> {
        if self.ways.is_empty() {
            vec![FIT_INSETS]
        } else {
            self.ways.clone()
        }
    }

    /// Records that the camera was just fitted (F), so a resize refits.
    pub(crate) fn mark_fitted(&mut self, fitted: bool) {
        self.fitted = fitted;
    }

    /// Fits again at the next frame when the camera is as Fit left it:
    /// what was just added (a picture) may lie outside the view.
    pub(crate) fn refit_if_fitted(&mut self) {
        if self.fitted {
            self.fit_pending = true;
        }
    }

    /// Where the pointer is in the project, mm, while it is over the
    /// Viewport: where the Paste key puts what it pastes.
    pub(crate) const fn cursor_mm(&self) -> Option<Point> {
        self.cursor_mm
    }

    /// Forgets the click cycle (the selection changed elsewhere).
    pub(crate) fn reset_clicks(&mut self) {
        self.clicks.reset();
    }

    /// Reads where the pointer is over last frame's `pane`, for the status
    /// bar and this frame's hover.
    pub(crate) fn read_pointer(&mut self, ctx: &egui::Context, pane: Rect, app: &App) -> Pointer {
        let covered = pane.bottom() - self.covered_bottom;
        let over = ctx.pointer_hover_pos().filter(|p| {
            pane.contains(*p)
                && p.y < covered
                && ctx
                    .layer_id_at(*p)
                    .is_none_or(|layer| layer.order == egui::Order::Background)
        });
        let camera = app.view().camera;
        self.cursor_mm = over.map(|p| camera.to_world(pane_pos(pane, p)));
        let drawing = takes_pointer(app);
        let hit = match (over, &self.drag) {
            // A drawing tool's clicks are its own: nothing under the
            // pointer is picked, so nothing lights up.
            _ if drawing => Hit::Nothing,
            // Target editing toggles shapes: a cut under the pointer is
            // nothing to click.
            (Some(p), None) if app.target_editing().is_some() => pick::pick(
                app.scene(),
                &camera,
                pane_pos(pane, p),
                PICK_TOLERANCE_PX,
                0,
            ),
            (Some(p), None | Some(Drag::Press { .. })) => pick::pick_with_cuts(
                app.scene(),
                &camera,
                pane_pos(pane, p),
                PICK_TOLERANCE_PX,
                0,
                &pick::shown_cuts(app.scene(), app.view(), app.selection()),
            ),
            _ => Hit::Nothing,
        };
        // A male board shape is its Inlay's, not an instance: the status
        // bar names the board and the Inlay (`experience.md`, section 9).
        let male = match &hit {
            Hit::MaleBoardShape(op) => app.project().operations.iter().find(|o| o.id() == op),
            _ => None,
        };
        let hover_op;
        (self.hover, self.hover_tab, hover_op, self.hover_picture) = match hit {
            Hit::Instance(id) => (Some(id), None, None, None),
            Hit::Tab(id) => (None, Some(id), None, None),
            Hit::Toolpath(id) => (None, None, Some(id), None),
            Hit::Bitmap(id) => (None, None, None, Some(id)),
            Hit::MaleBoardShape(_) | Hit::Nothing => (None, None, None, None),
        };
        // A picture under the pointer is named as one, since a shape may
        // have its name.
        let hover_picture = self.hover_picture.as_ref().and_then(|id| {
            let bitmap = app.scene().bitmap(id)?;
            let name = app
                .strings()
                .fmt(StrKey::PlacementPictureHover, &[("name", &bitmap.name)]);
            Some((name, bitmap.locked))
        });
        // A cut under the pointer names its operation, as a shape does.
        let hover_op = hover_op.and_then(|id| {
            let op = app.scene().operation(&id)?;
            Some((op.name.clone(), false))
        });
        Pointer {
            hover: male.map_or_else(
                || {
                    self.hover
                        .as_ref()
                        .and_then(|id| {
                            let shape = app.scene().shape(id)?;
                            Some((shape.name.clone(), shape.locked))
                        })
                        .or(hover_op)
                        .or(hover_picture)
                },
                |op| {
                    let name = app
                        .strings()
                        .fmt(StrKey::AppViewportHoverMaleBoard, &[("name", op.name())]);
                    Some((name, false))
                },
            ),
            // The armed tool's prompt, where a drag's travel reads.
            drag: self
                .readout
                .clone()
                .or_else(|| app.tool_bar().map(|bar| bar.prompt)),
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

    /// The operations whose toolpaths the last frame drew, bottom first.
    pub(crate) fn drawn(&self) -> &[OperationId] {
        &self.drawn
    }

    /// The depth ramp, legend and extents of `app`'s Scene, computed once
    /// per Scene revision.
    fn derived(&mut self, app: &App) -> Derived {
        let revision = app.scene().revision();
        match &self.derived {
            Some(d) if d.revision == revision => d.clone(),
            _ => {
                let project = app.project();
                let scene = app.scene();
                let derived = Derived {
                    revision,
                    ramp: DepthRamp::of(project, scene),
                    legend: dc_app::view::depth_legend(project, scene),
                    extents: app
                        .generation()
                        .finished()
                        .and_then(|f| dc_app::view::program_extents(project, f)),
                };
                self.derived = Some(derived.clone());
                derived
            }
        }
    }

    /// Operation `op`'s toolpath buffer, rebuilt by `build` when `key`
    /// differs from the last build's.
    fn toolpath(
        &mut self,
        op: &OperationId,
        key: u64,
        build: impl FnOnce() -> Vec<renderer::Segment>,
    ) -> (u64, Arc<LayerData>) {
        let next = self.next_version.wrapping_add(1);
        let cached = self.toolpaths.entry(op.clone()).or_default();
        if cached.key != Some(key) {
            cached.key = Some(key);
            cached.version = next;
            cached.data = Arc::new(LayerData::new(&build()));
            self.next_version = next;
        }
        (cached.version, Arc::clone(&cached.data))
    }

    /// The style table for `palette`, rebuilt when the theme changes.
    fn styles(&mut self, palette: &Palette) -> (u64, Arc<StyleTable>) {
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

/// Whether the armed tool takes the pointer's clicks and moves as its own
/// events: a drawing tool, Edit nodes, Trim, Measure, or a Shape command
/// that is taking its value, outside target editing. Select's are phase 4's
/// gestures and the Tab tool's its one click.
fn takes_pointer(app: &App) -> bool {
    matches!(
        app.tool(),
        Tool::Draw(_) | Tool::Nodes | Tool::Trim | Tool::Measure | Tool::Adjust(_)
    ) && app.target_editing().is_none()
}

/// The keys the Viewport answers when no field has the keyboard
/// (functional spec, capability 54), read through the shortcut table
/// ([`keyboard::SHORTCUTS`]): F fits, Shift F zooms to the selection,
/// Ctrl/⌘ A selects every visible shape, Ctrl/⌘ D duplicates the
/// selection, H hides it, Shift H shows it, L locks or unlocks it, Delete
/// or Backspace deletes it, the arrow keys nudge it by 1 mm (10 with
/// Shift, 0.1 with Alt), one undo step each, T turns the Tab tool on or
/// off, and Escape clears the selection unless a menu or popup takes the
/// key, the Tab tool or a pick is armed, or the key just left a
/// focused control (a field's Escape restores the field and no more). The
/// arrows are the Viewport's only while no other control has the focus,
/// since they move between a list's rows.
pub(crate) fn keys(ctx: &egui::Context, app: &App) -> Vec<Action> {
    if ctx.text_edit_focused() {
        return Vec::new();
    }
    // A group's list of tools is no menu to dismiss: a tool's key still
    // arms it while one shows.
    let popup_open = egui::Popup::is_any_open(ctx) && !controls::flyout_open(ctx);
    // Escape with a group's list showing closes the list, and no more:
    // the tool stays in hand. (Read here: the context is locked while the
    // keys are read.)
    let closes_a_list = controls::flyout_open(ctx);
    let arrows_free = focus::keys_free(ctx);
    let escape_left_widget = focus::escape_left_widget(ctx);
    let selected = app.selection().instances().to_vec();
    let pictures = app.selected_pictures().to_vec();
    let park_picking = project::park_pick_armed(ctx);
    let point_picking = editor::point_pick(ctx).is_some();
    let tab_tool_armed = app.tool() == Tool::Tab;
    // A drawing tool reads Enter, Escape, Backspace and Delete itself
    // (phase 5, capabilities 81 and 87): they finish, step back and take
    // back a point, and delete nothing.
    let drawing = takes_pointer(app);
    let editing_nodes = app.tool() == Tool::Nodes && drawing;
    // Read before the input is: the plug-in's lock is the context's.
    let label_selected = ctx
        .plugin_opt::<egui::text_selection::LabelSelectionState>()
        .is_some_and(|labels| labels.lock().has_selection());
    let mut disarm = false;
    let mut pressed = Vec::new();
    let mut actions = Vec::new();
    ctx.input_mut(|input| {
        if drawing && !popup_open {
            for (key, told) in [
                (Key::Enter, dc_app::tool::Key::Enter),
                (Key::Backspace, dc_app::tool::Key::Backspace),
                (Key::Delete, dc_app::tool::Key::Delete),
            ] {
                if input.consume_key(Modifiers::NONE, key) {
                    actions.push(Action::Tool(dc_app::tool::Event::Key(told)));
                }
            }
        }
        // Cut, Copy and Paste reach a window as events of their own, not
        // as keys (egui-winit, which also reads the system's clipboard
        // for a paste). They are the shapes' unless text is selected in
        // a label, which they then copy as they always did.
        if !popup_open && !label_selected {
            input.events.retain(|event| {
                let command = match event {
                    egui::Event::Cut => Shortcut::Cut,
                    egui::Event::Copy => Shortcut::Copy,
                    egui::Event::Paste(_) | egui::Event::PasteImage(_) => Shortcut::Paste,
                    _ => return true,
                };
                pressed.push(command);
                false
            });
        }
        for command in [
            Shortcut::ZoomToSelection,
            Shortcut::FitView,
            Shortcut::SelectAll,
            Shortcut::Cut,
            Shortcut::Copy,
            Shortcut::Paste,
            Shortcut::Duplicate,
            Shortcut::Group,
            Shortcut::Ungroup,
            Shortcut::Show,
            Shortcut::Hide,
            Shortcut::Lock,
            Shortcut::Delete,
            Shortcut::Rename,
            Shortcut::FlipHorizontal,
            Shortcut::FlipVertical,
            Shortcut::RotateAnticlockwise,
            Shortcut::RotateClockwise,
            Shortcut::TabTool,
            Shortcut::SelectTool,
            Shortcut::ToggleSnapping,
            Shortcut::DrawLine,
            Shortcut::DrawArc,
            Shortcut::DrawSpline,
            Shortcut::DrawRectangle,
            Shortcut::DrawCircle,
            Shortcut::DrawEllipse,
            Shortcut::DrawPolygon,
            Shortcut::DrawSlot,
            Shortcut::DrawText,
            Shortcut::EditNodes,
            Shortcut::BreakPath,
            Shortcut::Measure,
            Shortcut::Offset,
            Shortcut::Join,
        ] {
            // Select All is the nodes' while they are edited (below).
            if editing_nodes && command == Shortcut::SelectAll {
                continue;
            }
            if !popup_open && keyboard::consume(input, command) > 0 {
                pressed.push(command);
            }
        }
        if editing_nodes && !popup_open {
            use dc_app::tool::{Event, NodeAction};
            if keyboard::consume(input, Shortcut::SelectAll) > 0 {
                actions.push(Action::Tool(Event::Node(NodeAction::SelectAll)));
            }
            // The arrows nudge the selected nodes as they nudge shapes.
            if arrows_free {
                for (command, step_mm) in [
                    (Shortcut::NudgeCoarse, NUDGE_SHIFT_MM),
                    (Shortcut::NudgeFine, NUDGE_ALT_MM),
                    (Shortcut::Nudge, NUDGE_MM),
                ] {
                    for (key, (dx, dy)) in [
                        (Key::ArrowLeft, (-1.0, 0.0)),
                        (Key::ArrowRight, (1.0, 0.0)),
                        (Key::ArrowDown, (0.0, -1.0)),
                        (Key::ArrowUp, (0.0, 1.0)),
                    ] {
                        for _ in 0..keyboard::consume_key(input, command, key) {
                            actions.push(Action::Tool(Event::Node(NodeAction::Nudge {
                                dx_mm: dx * step_mm,
                                dy_mm: dy * step_mm,
                            })));
                        }
                    }
                }
            }
        }
        if !popup_open
            && !closes_a_list
            && keyboard::consume(input, Shortcut::Escape) > 0
            && !escape_left_widget
        {
            if drawing {
                // Out of what the tool has begun, then out of the tool.
                actions.push(Action::Tool(dc_app::tool::Event::Key(
                    dc_app::tool::Key::Escape,
                )));
            } else if tab_tool_armed {
                actions.push(Action::Arm(Tool::Select));
            } else if park_picking || point_picking {
                // Disarmed below: the context is locked inside `input_mut`.
                disarm = true;
            } else {
                actions.push(Action::ClearSelection);
            }
        }
        if (selected.is_empty() && pictures.is_empty()) || popup_open || !arrows_free {
            return;
        }
        for (command, step_mm) in [
            (Shortcut::NudgeCoarse, NUDGE_SHIFT_MM),
            (Shortcut::NudgeFine, NUDGE_ALT_MM),
            (Shortcut::Nudge, NUDGE_MM),
        ] {
            for (key, (dx, dy)) in [
                (Key::ArrowLeft, (-1.0, 0.0)),
                (Key::ArrowRight, (1.0, 0.0)),
                (Key::ArrowDown, (0.0, -1.0)),
                (Key::ArrowUp, (0.0, 1.0)),
            ] {
                let presses = keyboard::consume_key(input, command, key);
                for _ in 0..presses {
                    if pictures.is_empty() {
                        actions.push(Action::Run(Box::new(Command::TransformInstances {
                            ids: selected.clone(),
                            delta: Transform::translate(dx * step_mm, dy * step_mm),
                        })));
                        continue;
                    }
                    // A picture moves like a shape; a locked one says so.
                    let nudge = dc_app::pictures::move_by(
                        app.project(),
                        &pictures,
                        dx * step_mm,
                        dy * step_mm,
                    );
                    actions.push(match nudge {
                        Ok(command) => Action::run(command),
                        Err(e) => Action::Refused(e.to_string()),
                    });
                }
            }
        }
    });
    actions.extend(
        pressed
            .into_iter()
            .filter_map(|command| shortcut_action(app, command)),
    );
    if disarm {
        if park_picking {
            project::arm_park_pick(ctx, false);
        } else {
            editor::arm_point_pick(ctx, None);
        }
    }
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
        && let Some(grab @ (Grab::Corner(_) | Grab::Side(_) | Grab::Rotate { .. })) =
            handles::grab_at(&h, camera, at)
    {
        return Some(Target::Handle(grab));
    }
    if let Some(corner) = pictures::corner_at(state, camera, at) {
        return Some(Target::PictureCorner(corner));
    }
    match pick::pick(app.scene(), camera, at, PICK_TOLERANCE_PX, 0) {
        Hit::Instance(id) => Some(Target::Shape(id)),
        Hit::Tab(id) => Some(Target::Tab(id)),
        Hit::Bitmap(id) => Some(Target::Picture(id)),
        // A male board shape is no instance to move: a click on it selects
        // its Inlay on release. A cut is picked on release too (#576).
        Hit::Toolpath(_) | Hit::MaleBoardShape(_) | Hit::Nothing => None,
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
        // A tab drags along its outline, selected first.
        Target::Tab(id) => {
            if !state.app.selection().tabs().contains(&id) {
                out.push(Action::Select(Selection::Tabs(vec![id.clone()])));
            }
            return Drag::Tab { id };
        }
        // A picture moves with the pictures selected; one outside the
        // selection is selected first and moves alone. A corner resizes
        // the one selected.
        Target::Picture(id) => {
            let chosen = state.app.selected_pictures();
            let ids = if chosen.contains(&id) {
                chosen.to_vec()
            } else {
                out.push(Action::Select(Selection::Bitmaps(vec![id.clone()])));
                vec![id]
            };
            return start_picture_drag(state, start, ids, pictures::Grip::Move, out);
        }
        Target::PictureCorner(corner) => {
            let ids = state.app.selected_pictures().to_vec();
            return start_picture_drag(state, start, ids, pictures::Grip::Corner(corner), out);
        }
        Target::Shape(id) if selected.contains(&id) => (selected.to_vec(), Grab::Move),
        // A shape of a group is its group (capability 99).
        Target::Shape(id) => {
            let ids = dc_app::tree::with_groups(project, std::slice::from_ref(&id));
            out.push(Action::Select(Selection::Instances(ids.clone())));
            (ids, Grab::Move)
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

/// A drag of `grip` on the pictures `ids` begins, or is refused with the
/// reason in the status bar when one of them is locked.
fn start_picture_drag(
    state: &FrameState<'_>,
    start: Pos2,
    ids: Vec<BitmapId>,
    grip: pictures::Grip,
    out: &mut Vec<Action>,
) -> Drag {
    // A move of no distance is refused for a lock before it is found to
    // be nothing: the lock's refusal, by the command's own check.
    match dc_app::pictures::move_by(state.app.project(), &ids, 0.0, 0.0) {
        Err(e @ dc_app::CommandError::PictureLocked { .. }) => {
            out.push(Action::Refused(e.to_string()));
            Drag::Refused
        }
        _ => Drag::Picture { start, ids, grip },
    }
}

/// `value_mm` for the status bar's readout: one decimal in mm, three in
/// inches, as the cursor's read-out.
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
        Grab::Corner(_) | Grab::Side(_) => strings.fmt(
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
/// previews a transform and commits it on release. In target editing
/// (capability 67) nothing is grabbed: a click toggles the shape under
/// it in the draft, a marquee adds what it touches and an Alt-marquee
/// removes, and the selection and the shapes stay as they are. Returns
/// whether the camera moved.
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
        view.picture_preview = None;
        view.readout = None;
        return false;
    };
    let mut moved = false;
    let editing_targets = state.app.target_editing().is_some();
    if (pressed || middle_pressed) && response.hovered() && view.drag.is_none() {
        view.drag = Some(if middle_pressed || space {
            Drag::Pan { last: pos }
        } else if editing_targets {
            // Nothing is grabbed, so nothing can move.
            Drag::Marquee { start: pos }
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
            copying: matches!(grab, Grab::Move) && modifiers.alt,
        });
    }

    if let Some(Drag::Picture { start, ids, grip }) = &view.drag {
        let from = camera.to_world(pane_pos(pane, *start));
        let to = camera.to_world(pane_pos(pane, pos));
        let preview = pictures::dragged(state.app.project(), ids, *grip, from, to);
        view.readout = Some(pictures::readout(state, &preview));
        view.picture_preview = Some(preview);
    }

    if let Some(Drag::Tab { id }) = &view.drag {
        let project = state.app.project();
        view.tab_preview = project.tabs.iter().find(|t| t.id == *id).map(|tab| {
            let to = camera.to_world(pane_pos(pane, pos));
            let (x_mm, y_mm) = dc_app::viewport::tabs::tab_drag(project, tab, to);
            (id.clone(), x_mm, y_mm)
        });
    }

    if !released {
        return moved;
    }
    let mode = click_mode(modifiers);
    let tab_preview = view.tab_preview.take();
    let selection = state.app.selection();
    let preview = view.preview.take();
    let picture_preview = view.picture_preview.take();
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
            if editing_targets {
                out.push(Action::BoxTargets {
                    ids,
                    remove: modifiers.alt,
                });
            } else if ids.is_empty() {
                // Pictures are selected on their own (capability 57): a
                // marquee takes them where it touches no shape.
                let pictures = pick::pictures_touching(state.app.scene(), rect);
                out.push(Action::Select(selection.boxed_pictures(&pictures, mode)));
            } else {
                out.push(Action::Select(selection.boxed_in(
                    state.app.project(),
                    &ids,
                    mode,
                )));
            }
        }
        // The release commits a picture's drag: one command, one step.
        Some(Drag::Picture { .. }) => {
            match picture_preview.map(|p| p.command(state.app.project())) {
                Some(Ok(command)) => out.push(Action::run(command)),
                Some(Err(dc_app::CommandError::NothingToDo)) | None => {}
                Some(Err(e)) => out.push(Action::Refused(e.to_string())),
            }
        }
        // The release commits the drag: one command, one undo step.
        Some(Drag::Transform { ids, .. }) => {
            if let Some(preview) = preview.filter(|p| !is_identity(p.delta)) {
                let command = if preview.copying {
                    Command::CopyInstances {
                        ids,
                        placements: vec![preview.delta],
                        folder: None,
                    }
                } else {
                    Command::TransformInstances {
                        ids,
                        delta: preview.delta,
                    }
                };
                out.push(Action::run(command));
            }
        }
        // In target editing a click toggles the shape under it; a click
        // on anything else is nothing.
        Some(Drag::Marquee { .. } | Drag::Press { .. }) if editing_targets => {
            view.clicks.reset();
            let at = pane_pos(pane, pos);
            if let Hit::Instance(id) =
                pick::pick(state.app.scene(), camera, at, PICK_TOLERANCE_PX, 0)
            {
                out.push(Action::ToggleTarget(id));
            }
        }
        // A double-click on a shape edits its nodes (phase 5, capability
        // 92): the shape alone selected, the Edit nodes tool armed.
        Some(Drag::Marquee { .. } | Drag::Press { .. })
            if ui.input(|i| i.pointer.button_double_clicked(PointerButton::Primary))
                && matches!(
                    pick::pick(
                        state.app.scene(),
                        camera,
                        pane_pos(pane, pos),
                        PICK_TOLERANCE_PX,
                        0
                    ),
                    Hit::Instance(_)
                ) =>
        {
            view.clicks.reset();
            if let Hit::Instance(id) = pick::pick(
                state.app.scene(),
                camera,
                pane_pos(pane, pos),
                PICK_TOLERANCE_PX,
                0,
            ) {
                // In a group the double-click reaches the one shape,
                // which the first click selected with its group.
                let alone =
                    dc_app::tree::with_groups(state.app.project(), std::slice::from_ref(&id)).len()
                        < 2;
                out.push(Action::Select(Selection::Instances(vec![id])));
                if alone {
                    out.push(Action::Arm(Tool::Nodes));
                }
            }
        }
        Some(Drag::Marquee { .. } | Drag::Press { .. }) => {
            let app = state.app;
            let hit = view.clicks.click_with_cuts(
                app.scene(),
                camera,
                pane_pos(pane, pos),
                PICK_TOLERANCE_PX,
                &pick::shown_cuts(app.scene(), app.view(), selection),
            );
            match hit {
                // A cut opens its operation; the shapes stay selected.
                Hit::Toolpath(id) => out.push(Action::ShowOperation(id)),
                hit => out.push(Action::Select(selection.clicked_in(
                    app.project(),
                    &hit,
                    mode,
                ))),
            }
        }
        // The release commits the tab's new point: one `MoveTab`.
        Some(Drag::Tab { id }) => {
            let moved_tab = state.app.project().tabs.iter().any(|t| {
                t.id == id
                    && tab_preview.as_ref().is_some_and(|(_, x, y)| {
                        (t.x_mm - x).abs() > NO_CHANGE || (t.y_mm - y).abs() > NO_CHANGE
                    })
            });
            if moved_tab && let Some((id, x_mm, y_mm)) = tab_preview {
                out.push(Action::Run(Box::new(Command::MoveTab { id, x_mm, y_mm })));
            }
        }
        Some(Drag::Pan { .. } | Drag::Refused | Drag::Tool) | None => {}
    }
    moved
}

/// The pointer while a drawing tool is armed (phase 5, capability 81):
/// the middle button and Space still pan; the primary button's press and
/// release, a double-click, and every move over the pane are the tool's,
/// told in project millimetres with the keys held. Nothing is selected,
/// moved or resized. Returns whether the camera moved.
fn tool_gestures(
    ui: &Ui,
    pane: Rect,
    response: &egui::Response,
    view: &mut ViewportState,
    camera: &mut Camera,
    out: &mut Vec<Action>,
) -> bool {
    use dc_app::tool::Event;
    let (pos, pressed, released, doubled, middle_pressed, middle_released, space, modifiers) = ui
        .input(|i| {
            (
                i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()),
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.button_released(PointerButton::Primary),
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_pressed(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Middle),
                i.key_down(Key::Space),
                i.modifiers,
            )
        });
    let Some(pos) = pos.filter(|p| pane.contains(*p) || view.drag.is_some()) else {
        // Off the pane: the tool shows no next point.
        view.drag = None;
        if view.tool_pointer.take().is_some() {
            out.push(Action::Tool(Event::Leave));
        }
        return false;
    };
    if (middle_pressed || (pressed && space)) && response.hovered() && view.drag.is_none() {
        view.drag = Some(Drag::Pan { last: pos });
    }
    if let Some(Drag::Pan { last }) = view.drag {
        let delta = pos - last;
        let moved = delta != egui::Vec2::ZERO;
        if moved {
            camera.pan(f64::from(delta.x), f64::from(delta.y));
        }
        view.drag = Some(Drag::Pan { last: pos });
        if released || middle_released {
            view.drag = None;
        }
        return moved;
    }
    let pointer = dc_app::Pointer {
        at_mm: camera.to_world(pane_pos(pane, pos)),
        shift: modifiers.shift,
        alt: modifiers.alt,
        command: modifiers.command,
    };
    // Over a pill or the tool bar the tool keeps the point it had: a size
    // is typed with the preview still where the pointer left it.
    let over = response.hovered() || view.drag.is_some();
    if over && view.tool_pointer != Some(pointer) {
        view.tool_pointer = Some(pointer);
        out.push(Action::Tool(Event::Move(pointer)));
    }
    // A click that closes a list or a menu is that list's, and takes no
    // point.
    let closes_a_popup = egui::Popup::is_any_open(ui.ctx()) && !controls::flyout_open(ui.ctx());
    if pressed && response.hovered() && view.drag.is_none() && !closes_a_popup {
        view.drag = Some(Drag::Tool);
        out.push(Action::Tool(Event::Press(pointer)));
    }
    if released && matches!(view.drag, Some(Drag::Tool)) {
        view.drag = None;
        out.push(Action::Tool(Event::Release(pointer)));
        if doubled {
            out.push(Action::Tool(Event::DoubleClick(pointer)));
        }
    }
    false
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
    let stock_layer = view.layer(
        Layer::Stock,
        key((stock, scene.shapes_revision(), band)),
        || layers::stock(project, scene, tolerance),
    );
    // A drag in progress draws its shapes where the release will put them,
    // through the preview transform, and not where the snapshot has them.
    let moving: Vec<InstanceId> = view
        .preview
        .as_ref()
        .map(|p| p.ids.clone())
        .unwrap_or_default();
    let placed = view.preview.as_ref().map(|p| p.placed);
    let placed_key = placed.map(|t| t.coeffs().map(f64::to_bits));
    // An Alt-drag leaves the originals, so they stay drawn where they are.
    let lifted: Vec<InstanceId> = if view.preview.as_ref().is_some_and(|p| p.copying) {
        Vec::new()
    } else {
        moving.clone()
    };
    // The marks are the Scene's, set when the marked operation or its
    // targets change: the two layers that show them rebuild then, and
    // not per frame (acceptance 87).
    let marks = scene.targets_revision();
    let shapes_layer = view.layer(
        Layer::Shapes,
        key((scene.shapes_revision(), band, &lifted, marks)),
        || layers::shapes(scene, tolerance, &lifted),
    );
    // A target that is also selected draws a halo under its selection.
    let haloed: Vec<InstanceId> = scene.targets().map_or_else(Vec::new, |m| {
        m.targets
            .iter()
            .filter(|id| selected.contains(id))
            .cloned()
            .collect()
    });
    let targets_layer = view.layer(
        Layer::Targets,
        key((scene.shapes_revision(), band, &lifted, marks, &haloed)),
        || layers::targets(scene, tolerance, &haloed, &lifted),
    );
    let hover_layer = view.layer(
        Layer::Hover,
        key((scene.shapes_revision(), band, &hover)),
        || {
            layers::highlighted(
                scene,
                &hover,
                tolerance,
                (StyleId::Hover, StyleId::HoverOpen),
                None,
            )
        },
    );
    let mut drawn = selected.clone();
    drawn.extend(moving.iter().filter(|id| !selected.contains(id)).cloned());
    let selection_layer = view.layer(
        Layer::Selection,
        key((scene.shapes_revision(), band, &drawn, &moving, placed_key)),
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
    // The pictures, under the shapes: at 60 % while a shape is selected,
    // so the selection reads over them (`viewport.picture`).
    let strength = if selected.is_empty() {
        1.0
    } else {
        theme::PICTURE_DIMMED
    };
    let picture_quads = pictures::quads(
        scene.bitmaps(),
        &view.picture_mips,
        view.picture_preview.as_ref(),
        strength,
    );
    let derived = view.derived(app);
    let toolpaths = toolpath_layers(state, view, band, &derived);
    let arrows_layer = arrow_layer(state, view, camera, band);
    let extents_on = app.view().visibility.extents;
    let extents = derived.extents.filter(|_| extents_on);
    let extents_layer = view.layer(Layer::Extents, key((derived.revision, extents_on)), || {
        extents.map_or_else(Vec::new, |e| layers::extents(e.centre_mm, e.edge_mm))
    });
    Frame {
        centre_mm: [
            renderer::gpu_f32(camera.centre_mm.x),
            renderer::gpu_f32(camera.centre_mm.y),
        ],
        px_per_mm: renderer::gpu_f32(camera.px_per_mm),
        minor_alpha: renderer::gpu_f32(grid.minor_alpha),
        by_depth: app.view().colour == ColourMode::ByDepth,
        styles: view.styles(palette),
        layers: [
            grid_layer,
            stock_layer,
            shapes_layer,
            arrows_layer,
            extents_layer,
            targets_layer,
            hover_layer,
            selection_layer,
        ],
        toolpaths,
        pictures: picture_quads,
    }
}

/// The operations whose toolpaths `app` draws, bottom first: every
/// operation of the Scene with runs whose eye is open (and, with *Only the
/// selected operation* on, the selected one), the selected operation last
/// so it draws on top (`experience.md`, section 9).
pub(crate) fn drawn_operations(app: &App) -> Vec<OperationId> {
    let view = app.view();
    let selection = app.selection();
    let mut ids: Vec<OperationId> = app
        .scene()
        .operations()
        .iter()
        .filter(|o| !o.runs.is_empty() && view.shows(&o.id, selection))
        .map(|o| o.id.clone())
        .collect();
    if let Selection::Operation(selected) = selection
        && let Some(at) = ids.iter().position(|id| id == selected)
    {
        let top = ids.remove(at);
        ids.push(top);
    }
    ids
}

/// One buffer per drawn operation (design: "cuts per operation (one
/// buffer each, so an operation's eye toggles a draw call)"), rebuilt only
/// when its result, its staleness, its colour, the zoom band, the depth
/// ramp or a layer toggle changes.
fn toolpath_layers(
    state: &FrameState<'_>,
    view: &mut ViewportState,
    band: i32,
    derived: &Derived,
) -> Vec<ToolpathData> {
    let app = state.app;
    let visibility = app.view().visibility;
    let drawn = drawn_operations(app);
    let ramp = derived.ramp;
    let ramp_key = ramp.map(|r| (r.top_z_mm.to_bits(), r.deepest_z_mm.to_bits()));
    let flags = (
        visibility.cuts,
        visibility.rapids,
        visibility.plunges,
        visibility.start_points,
        visibility.tab_marks,
    );
    let male_offset = app.scene().board_offset(dc_project::Board::Male);
    let look = layers::ToolpathLook {
        visibility: &visibility,
        ramp: ramp.as_ref(),
        tolerance_mm: layers::tolerance_mm(band),
        lod_mm: layers::lod_mm(band),
        male_offset,
    };
    let offset_key = male_offset.map(|o| (o.x.to_bits(), o.y.to_bits()));
    let mut out = Vec::with_capacity(drawn.len());
    for id in &drawn {
        let Some(op) = app.scene().operation(id) else {
            continue;
        };
        let layer_key = key((
            op.result_key(),
            op.stale,
            op.colour,
            band,
            ramp_key,
            flags,
            offset_key,
        ));
        let (version, data) = view.toolpath(id, layer_key, || layers::toolpath(op, look));
        out.push(ToolpathData {
            key: key(id),
            version,
            data,
        });
    }
    // Forget the buffers of operations no longer in the Scene.
    let present: Vec<&OperationId> = app.scene().operations().iter().map(|o| &o.id).collect();
    view.toolpaths.retain(|id, _| present.contains(&id));
    view.drawn = drawn;
    out
}

/// The direction arrows of every drawn operation, one layer, rebuilt with
/// the scale (their spacing is in pixels) and with what is drawn.
fn arrow_layer(
    state: &FrameState<'_>,
    view: &mut ViewportState,
    camera: &Camera,
    band: i32,
) -> (u64, Arc<LayerData>) {
    let app = state.app;
    let visibility = app.view().visibility;
    let scene = app.scene();
    // Each run's key, toolpath, tint and display offset.
    let mut runs: Vec<(u64, &dc_job::Toolpath, layers::Tint, dc_geometry::Vec2)> = Vec::new();
    if visibility.arrows {
        for id in &view.drawn {
            let Some(op) = scene.operation(id) else {
                continue;
            };
            for (k, run) in op.runs.iter().enumerate() {
                // A male board run's arrows go where its moves are drawn.
                if let Some(offset) = scene.board_offset(run.run.board) {
                    runs.push((
                        key((id, op.result_key(), k)),
                        &run.run.toolpath,
                        layers::Tint::of(op, run),
                        offset,
                    ));
                }
            }
        }
    }
    let layer_key = key((
        camera.px_per_mm.to_bits(),
        visibility.cuts,
        runs.iter()
            .map(|r| (r.0, r.2, r.3.x.to_bits(), r.3.y.to_bits()))
            .collect::<Vec<_>>(),
    ));
    let tolerance = layers::tolerance_mm(band);
    let keys: Vec<u64> = runs.iter().map(|r| r.0).collect();
    view.arrow_moves.retain(|k, _| keys.contains(k));
    view.arrows.retain(&keys);
    let mut moves = Vec::with_capacity(runs.len());
    for (run_key, toolpath, _, _) in &runs {
        let cached = view
            .arrow_moves
            .get(run_key)
            .filter(|(b, _)| *b == band)
            .map(|(_, m)| Arc::clone(m));
        let m = cached.unwrap_or_else(|| {
            let m = Arc::new(layers::arrow_moves(toolpath, tolerance));
            view.arrow_moves.insert(*run_key, (band, Arc::clone(&m)));
            m
        });
        moves.push(m);
    }
    let px_per_mm = camera.px_per_mm;
    let ViewportState {
        layers: cached_layers,
        arrows,
        next_version,
        ..
    } = view;
    let next = next_version.wrapping_add(1);
    let Some(cached) = cached_layers.get_mut(Layer::Arrows.index()) else {
        return (0, Arc::default());
    };
    if cached.key != Some(layer_key) {
        let arrow_runs: Vec<layers::ArrowRun<'_>> = runs
            .iter()
            .zip(&moves)
            .map(|((run_key, _, tint, offset), m)| layers::ArrowRun {
                key: *run_key,
                moves: m.as_slice(),
                tint: *tint,
                offset: *offset,
            })
            .collect();
        cached.key = Some(layer_key);
        cached.version = next;
        cached.data = Arc::new(LayerData::new(&layers::arrows(
            &arrow_runs,
            arrows,
            px_per_mm,
            &visibility,
        )));
        *next_version = next;
    }
    (cached.version, Arc::clone(&cached.data))
}

/// Whether the next click on the Viewport is a pick's: the park point's
/// or a point row's.
fn park_or_point(ctx: &egui::Context, editing_targets: bool, point: bool) -> bool {
    point || (project::park_pick_armed(ctx) && !editing_targets)
}

/// The Viewport's context menu (phase 5, capabilities 57, 92 and 98). On
/// a shape it is the shape's Design Tree row's, for the selection when
/// the shape is in it and for the shape alone, which it selects, when
/// not; on nothing it is *Paste* and *Select All*. In the Edit nodes tool
/// it is the tool bar's buttons for the node or the segment under the
/// pointer, which a right-click selects as a click does unless it is
/// selected already. *Paste* puts its shapes where the menu was opened.
fn context_menu(
    pane: Rect,
    response: &egui::Response,
    state: &FrameState<'_>,
    view: &mut ViewportState,
    camera: &Camera,
    out: &mut Vec<Action>,
) {
    use dc_app::tool::{Control, Event, Key as ToolKey, NodeAction};
    let app = state.app;
    let strings = app.strings();
    let nodes = app.tool() == Tool::Nodes;
    if response.secondary_clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let at_px = pane_pos(pane, at);
        let at_mm = camera.to_world(at_px);
        let mut on = None;
        if nodes {
            let click = match app.node_control_at(at_mm) {
                Some(Control::Node(node)) => !app.selected_nodes().contains(&node),
                Some(Control::Span { .. }) => true,
                _ => false,
            };
            if click {
                let pointer = dc_app::Pointer {
                    at_mm,
                    shift: false,
                    alt: false,
                    command: false,
                };
                out.push(Action::Tool(Event::Press(pointer)));
                out.push(Action::Tool(Event::Release(pointer)));
            }
        } else if let Hit::Instance(id) =
            pick::pick(app.scene(), camera, at_px, PICK_TOLERANCE_PX, 0)
        {
            if !app.selection().instances().contains(&id) {
                out.push(Action::Select(Selection::Instances(
                    dc_app::tree::with_groups(app.project(), std::slice::from_ref(&id)),
                )));
            }
            on = Some(id);
        }
        view.menu = Some(MenuAt { on, at_mm });
        view.clicks.reset();
    }
    let Some(menu) = view.menu.clone() else {
        return;
    };
    response.context_menu(|ui| {
        if nodes {
            let actions = app.tool_bar().map(|bar| bar.actions).unwrap_or_default();
            for action in &actions {
                let row = egui::Button::new(action.label.as_str());
                if ui.add_enabled(action.enabled, row).clicked() {
                    out.push(Action::Tool(Event::Node(action.action)));
                    ui.close();
                }
            }
            if !actions.is_empty() {
                ui.separator();
            }
            if ui.button(strings.get(StrKey::TreeDelete)).clicked() {
                out.push(Action::Tool(Event::Key(ToolKey::Delete)));
                ui.close();
            }
            if ui.button(strings.get(StrKey::AppMenuSelectAll)).clicked() {
                out.push(Action::Tool(Event::Node(NodeAction::SelectAll)));
                ui.close();
            }
        } else if let Some(on) = &menu.on {
            let selected = app.selection().instances();
            let alone = [on.clone()];
            let ids = if selected.contains(on) {
                selected
            } else {
                alone.as_slice()
            };
            crate::panels::tree::context_menu(
                ui,
                state,
                (on, ids),
                PasteAt::Point(menu.at_mm),
                out,
            );
        } else {
            let paste = egui::Button::new(strings.get(StrKey::AppMenuPaste));
            if ui.add_enabled(app.can_paste(), paste).clicked() {
                out.push(Action::Paste(PasteAt::Point(menu.at_mm)));
                ui.close();
            }
            let all = shortcut_action(app, Shortcut::SelectAll);
            let row = egui::Button::new(strings.get(StrKey::AppMenuSelectAll));
            if ui.add_enabled(all.is_some(), row).clicked()
                && let Some(all) = all
            {
                out.push(all);
                ui.close();
            }
        }
    });
}

/// The selection's transform handles (`experience.md`, section 8,
/// "Transform handles"; section 13, item 86): a 1-px accent box, four
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
    // Nothing moves in target editing, so there is nothing to grab.
    if app.target_editing().is_some() {
        return;
    }
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
    // A handle at the middle of each side stretches one way, where the
    // side is long enough to carry one.
    let middles: Vec<Pos2> = h
        .sides(camera)
        .into_iter()
        .map(|side| at(h.side(side)))
        .collect();
    for corner in corners.into_iter().chain(middles) {
        painter.rect(
            Rect::from_center_size(corner, egui::vec2(HANDLE_PX, HANDLE_PX)),
            0.0,
            palette.viewport_handle_fill,
            stroke,
            egui::StrokeKind::Middle,
        );
    }
    painter.circle(knob, HANDLE_PX / 2.0, palette.viewport_handle_fill, stroke);
}

/// The overlay over the GPU layers: the origin crosshair and its label,
/// the park point, a padlock by each locked shape, the selection's
/// handles, and the marquee.
fn overlay(ui: &Ui, pane: Rect, state: &FrameState<'_>, view: &ViewportState, camera: &Camera) {
    let palette = Palette::current(ui);
    let painter = ui.painter_at(pane);
    let project = state.app.project();

    // Where the two marks are is the model's (`dc_app::project::marks`),
    // so what acceptance 69 asserts of it is what is drawn here.
    let marks = dc_app::project::marks(project);
    let origin = window_pos(pane, camera, marks.origin_mm);
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
        origin
            + egui::vec2(
                -controls::ORIGIN_LABEL_OFFSET_PX,
                controls::ORIGIN_LABEL_OFFSET_PX,
            ),
        egui::Align2::RIGHT_TOP,
        state.app.strings().get(StrKey::AppViewportOrigin),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );

    if let Some(board) = state.app.scene().male_board() {
        male_board_marks(ui, &painter, pane, state, board, camera);
    }

    if let Some(park_mm) = marks.park_mm {
        let at = window_pos(pane, camera, park_mm);
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

    pictures::overlay(
        ui,
        &painter,
        pane,
        state,
        camera,
        pictures::Marks {
            hover: view.hover_picture.as_ref(),
            preview: view.picture_preview.as_ref(),
        },
    );
    tab_tool::paint(
        &painter,
        palette,
        pane,
        state.app,
        camera,
        &tab_tool::Marks {
            hover: view.hover_tab.as_ref(),
            dragged: view.tab_preview.as_ref(),
        },
    );
    // While a tool is armed the clicks are its own: there is nothing to
    // grab, so no handles are drawn (phase 5, capability 81).
    if state.app.tool() == Tool::Select {
        transform_handles(ui, &painter, pane, state, view, camera);
    }
    draft::paint(ui, &painter, pane, state.app, camera);
    // A selection dragged or resized reads its size beside it, as a
    // shape being drawn does (phase 5, capability 83a).
    let units = state.app.project().display_units;
    let strings = state.app.strings();
    let size = match (&view.drag, &view.preview, &view.picture_preview) {
        (Some(Drag::Transform { handles, grab, .. }), Some(preview), _) => {
            handles::size_label(strings, units, handles, *grab, preview.delta)
        }
        (_, _, Some(preview)) => pictures::size_label(state, preview),
        _ => None,
    };
    if let Some(size) = size {
        draft::size_label(ui, &painter, pane, camera, &size);
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

/// The male board's origin mark, a smaller crosshair than the stock's, and
/// its `Male board` caption in a label pill at its top-left, all in
/// `viewport.male-board` (`experience.md`, section 9, "Male inlay board").
fn male_board_marks(
    ui: &Ui,
    painter: &egui::Painter,
    pane: Rect,
    state: &FrameState<'_>,
    board: &dc_app::scene::MaleBoardView,
    camera: &Camera,
) {
    let palette = Palette::current(ui);
    let origin = window_pos(pane, camera, board.origin_mm);
    let stroke = Stroke::new(1.0, palette.viewport_male_board);
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let dir = egui::vec2(dx, dy);
        painter.line_segment(
            [
                origin + dir * MALE_CROSSHAIR_GAP_PX,
                origin + dir * MALE_CROSSHAIR_ARM_PX,
            ],
            stroke,
        );
    }
    let galley = painter.layout_no_wrap(
        state
            .app
            .strings()
            .get(StrKey::AppViewportMaleBoard)
            .to_owned(),
        TextStyle::Small.resolve(ui.style()),
        palette.viewport_male_board,
    );
    let corner = window_pos(
        pane,
        camera,
        Point::new(board.frame.min.x, board.frame.max.y),
    );
    let rect = Rect::from_min_size(
        corner + egui::vec2(0.0, -galley.size().y - 8.0),
        galley.size() + egui::vec2(16.0, 4.0),
    );
    painter.rect_filled(rect, theme::pill(rect.height()), palette.viewport_label_bg);
    painter.galley(
        rect.min + egui::vec2(8.0, 2.0),
        galley,
        palette.viewport_male_board,
    );
}

/// The target-editing bar (capability 67; `experience.md`, section 8,
/// "Target editing"): 32 px across the top of the pane while target
/// editing is on, `accent.soft` with a 1-px `accent` rule under it; the
/// `target` glyph and *Editing targets of "Panel": click shapes to add or
/// remove them* on the left, *Cancel* (Escape) and *Done* (Enter) on the
/// right. It lies over the pane and takes the clicks on it, so none
/// reaches the drawing under it.
fn target_bar(ui: &mut Ui, pane: Rect, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let Some(editing) = app.target_editing() else {
        return;
    };
    let palette = Palette::current(ui);
    let strings = app.strings();
    let name = app
        .project()
        .operations
        .iter()
        .find(|o| o.id() == editing.op())
        .map_or_else(|| editing.op().as_str().to_owned(), |o| o.name().to_owned());
    let rect = Rect::from_min_size(pane.min, egui::vec2(pane.width(), TARGET_BAR_PX));
    ui.interact(
        rect,
        egui::Id::unique("dc.viewport.target_bar"),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    // Opaque under the wash, so the drawing does not show through.
    painter.rect_filled(rect, 0.0, palette.surface_panel);
    painter.rect_filled(rect, 0.0, palette.accent_soft);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, palette.accent),
    );
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(16.0, 8.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = 8.0;
    let keys = |key: Key| crate::shell::window::shortcut_text(ui.ctx(), Modifiers::NONE, key);
    let done_hint = strings.fmt(
        StrKey::EditorTargetsDoneHint,
        &[("keys", &keys(Key::Enter))],
    );
    let cancel_hint = strings.fmt(
        StrKey::EditorTargetsCancelHint,
        &[("keys", &keys(Key::Escape))],
    );
    let done = ButtonSpec::new(ButtonKind::Primary, strings.get(StrKey::EditorTargetsDone))
        .tooltip(&done_hint);
    if button(&mut bar, done).clicked() {
        out.push(Action::CommitTargets);
    }
    let cancel = ButtonSpec::new(
        ButtonKind::Secondary,
        strings.get(StrKey::EditorTargetsCancel),
    )
    .tooltip(&cancel_hint);
    if button(&mut bar, cancel).clicked() {
        out.push(Action::CancelTargets);
    }
    bar.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        let (glyph_rect, _) = ui.allocate_exact_size(
            egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
            egui::Sense::hover(),
        );
        let glyph = Icon::Target.galley(ui, theme::ICON_SMALL_PX, palette.accent);
        ui.painter().galley(
            glyph_rect.center() - glyph.size() / 2.0,
            glyph,
            palette.accent,
        );
        // The bar says what to do, so its sentence is not cut where the
        // Viewport is narrow (an Inspector beside it): it wraps to the
        // two lines the bar is tall enough for, and only a third is cut.
        let text = strings.fmt(StrKey::EditorTargetsEditing, &[("name", &name)]);
        let mut job = egui::text::LayoutJob::simple(
            text.clone(),
            TextStyle::Body.resolve(ui.style()),
            palette.text_primary,
            ui.available_width(),
        );
        job.wrap.max_rows = 2;
        let galley = ui.painter().layout_job(job);
        let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        ui.painter().galley(rect.min, galley, palette.text_primary);
    });
}

/// Draws the Viewport in the central panel, `margins` from the cards
/// beside it; returns its rectangle.
pub(crate) fn show(
    ui: &mut Ui,
    state: &FrameState<'_>,
    view: &mut ViewportState,
    margins: Margins,
    out: &mut Vec<Action>,
) -> Rect {
    let palette = Palette::current(ui);
    egui::CentralPanel::no_frame()
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(margins.frame(0, BOTTOM_PX)),
        )
        .show(ui, |ui| {
            let (pane, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            // Tab from the drawing (the Viewport focused, or nothing)
            // goes to the armed tool's first field.
            let tabbed = focus::tab_from(ui.ctx())
                .is_some_and(|from| from.is_none_or(|id| id == response.id));
            // Tab stops on the Viewport (#548): named for screen readers.
            let viewport_name = state.app.strings().get(StrKey::AppViewportName);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, viewport_name));
            // Its focus ring is a card's.
            focus::outline(ui.ctx(), response.id, pane, theme::CARD_RADIUS_PX);
            let app = state.app;
            let before = app.view().camera;
            let mut camera = before;
            let size = dc_geometry::Vec2::new(f64::from(pane.width()), f64::from(pane.height()));
            let resized = (camera.viewport_px.x - size.x).abs() > 0.5
                || (camera.viewport_px.y - size.y).abs() > 0.5;
            camera.viewport_px = size;
            // What floats over the pane besides its pills: an Inspector
            // over its right in a narrow window, the Warnings drawer over
            // its bottom, and the depth legend while By depth is on. Fit
            // stands clear of them, and a fitted view refits when one
            // comes or goes. The target-editing bar only moves the pills
            // down.
            let bar_px = if app.target_editing().is_some() {
                TARGET_BAR_PX
            } else {
                0.0
            };
            let legend = view
                .derived(app)
                .legend
                .filter(|_| app.view().colour == ColourMode::ByDepth)
                .filter(|_| !state.show_empty);
            let legend_size = legend.as_ref().map(|legend| legend::size(ui, app, legend));
            let over = (margins.overlay_right, margins.overlay_bottom);
            // The armed tool's bar, two rows of it: Fit stands clear of
            // it, and a fitted view refits as the tool is taken up or
            // put down.
            let tool_bar_px = if takes_pointer(app) {
                tool_bar::ROOM_PX
            } else {
                0.0
            };
            let ways = controls::ways(over, legend_size, tool_bar_px);
            let ways_changed = view.ways != ways;
            view.ways = ways;
            if view.fit_pending || (view.fitted && (resized || ways_changed)) {
                camera.fit_clear(camera::fit_bounds(app.project(), app.scene()), &view.ways);
                view.fit_pending = false;
                view.fitted = true;
            }

            // The pointer over the Warnings drawer is the drawer's: its
            // wheel scrolls the rows and does not zoom the drawing.
            view.covered_bottom = margins.overlay_bottom;
            let clear = pane.bottom() - margins.overlay_bottom;
            let at = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| pane.contains(*p) && p.y < clear && response.contains_pointer());
            let mut moved = false;
            if let Some(at) = at {
                moved |= scroll_and_pinch(ui, pane, pane_pos(pane, at), &mut camera);
            }
            let editing_targets = app.target_editing().is_some();
            let point = editor::point_pick(ui.ctx()).filter(|_| !editing_targets);
            let picking = park_or_point(ui.ctx(), editing_targets, point.is_some());
            let adding_tabs = app.tool() == Tool::Tab && !editing_targets;
            let drawing = takes_pointer(app);
            let park = project::park_pick_armed(ui.ctx()) && !editing_targets;
            if (park || point.is_some() || adding_tabs || drawing) && response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if park
                && response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                // The armed click is the park point's, not a selection.
                project::arm_park_pick(ui.ctx(), false);
                view.drag = None;
                view.clicks.reset();
                let at_mm = camera.to_world(pane_pos(pane, at));
                out.extend(project::park_at(app.project(), at_mm).map(Action::run));
            } else if let Some(pick) = point
                && response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                // An armed click is the point row's (a Profile's start
                // point), one click, not a selection.
                editor::arm_point_pick(ui.ctx(), None);
                view.drag = None;
                view.clicks.reset();
                let at_mm = camera.to_world(pane_pos(pane, at));
                out.extend(editor::point_at(app.project(), &pick, at_mm));
            } else if adding_tabs
                && response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                // The Tab tool's click adds a tab on the outline under it,
                // and the tool stays armed until Escape.
                view.drag = None;
                view.clicks.reset();
                out.extend(tab_tool::click(app, &camera, pane_pos(pane, at)).map(Action::run));
            } else if drawing {
                moved |= tool_gestures(ui, pane, &response, view, &mut camera, out);
            } else {
                moved |= gestures(ui, pane, &response, state, view, &mut camera, out);
            }
            if !drawing {
                view.tool_pointer = None;
            }
            if !(editing_targets || picking || adding_tabs || state.show_empty)
                && matches!(app.tool(), Tool::Select | Tool::Nodes)
            {
                context_menu(pane, &response, state, view, &camera, out);
            }
            if moved {
                view.fitted = false;
            }
            if camera != before {
                out.push(Action::SetCamera(camera));
            }
            match view.drag {
                Some(Drag::Refused) => ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed),
                Some(Drag::Transform { .. } | Drag::Tab { .. } | Drag::Picture { .. }) => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                // The pointing hand over a shape a click would toggle.
                None if editing_targets => {
                    if view.hover.is_some() && response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
                None if view.hover_tab.is_some() && !adding_tabs => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                _ => {}
            }

            let painter = ui.painter_at(pane);
            painter.rect_filled(pane, 0.0, palette.viewport_bg);
            if state.show_empty {
                // The start screen, in place of the empty stock.
                empty::screen(ui, pane, state, out);
                mask_corners(&painter, pane, theme::CARD_RADIUS_PX, palette);
                return pane;
            }
            let stock = &app.project().stock;
            let sheet = Rect::from_two_pos(
                window_pos(pane, &camera, Point::new(0.0, 0.0)),
                window_pos(pane, &camera, Point::new(stock.width_mm, stock.height_mm)),
            );
            painter.rect_filled(sheet, 0.0, palette.viewport_stock_fill);
            if let Some(board) = app.scene().male_board() {
                let male = Rect::from_two_pos(
                    window_pos(pane, &camera, board.frame.min),
                    window_pos(pane, &camera, board.frame.max),
                );
                painter.rect_filled(male, 0.0, palette.viewport_stock_fill);
            }
            // The pictures' mip chains: a large one's is built on its own
            // thread, and the frame looks again until it lands.
            if view.picture_mips.follow(app.scene().bitmaps()) {
                ui.ctx().request_repaint_after(crate::shell::worker::POLL);
            }
            let frame = frame(state, view, &camera, palette);
            painter.add(renderer::callback(pane, frame));
            overlay(ui, pane, state, view, &camera);
            target_bar(ui, pane, state, out);
            // The floating controls, at 60 % while a drag is under way.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let dragging = match &view.drag {
                Some(
                    Drag::Pan { .. }
                    | Drag::Transform { .. }
                    | Drag::Tab { .. }
                    | Drag::Picture { .. },
                ) => true,
                Some(Drag::Marquee { start } | Drag::Press { start, .. }) => {
                    pointer.is_some_and(|at| start.distance(at) >= CLICK_SLOP_PX)
                }
                Some(Drag::Refused | Drag::Tool) | None => false,
            };
            let opacity = controls::opacity(ui, dragging);
            let place = controls::Place {
                pane,
                top: pane.top() + bar_px,
                right: pane.right() - margins.overlay_right,
                bottom: pane.bottom() - margins.overlay_bottom,
                opacity,
            };
            let floats = controls::show(ui, place, state, &camera, view.cursor_mm, out);
            // The armed tool's bar, under the pills.
            let bar_top = place.top + theme::FLOAT_INSET_PX + controls::PILL_PX;
            let tool_bar = tool_bar::show(ui, pane, bar_top, tabbed, state, out);
            if let Some(zoomed) = floats.camera {
                // A zoom control's camera, shown from the next frame on.
                out.push(Action::SetCamera(zoomed));
                view.fitted = false;
            }
            // Capability 73a: the depth legend while By depth is on,
            // whether or not the toolpath view is open, hung under the
            // top-right controls.
            let mut covers = floats.rects();
            covers.extend(tool_bar);
            if let Some(legend) = &legend {
                let mut fading = painter.clone();
                fading.set_opacity(opacity);
                let hung = floats.view.map_or(
                    egui::pos2(place.right - theme::FLOAT_INSET_PX, place.top),
                    |pill| egui::pos2(pill.right(), pill.bottom()),
                ) + egui::vec2(0.0, controls::LEGEND_GAP_PX);
                covers.push(legend::draw_legend(ui, &fading, hung, app, legend));
            }
            // After the legend, so the caption can step out from under
            // it and the controls.
            if let Some(extents) = view
                .derived
                .as_ref()
                .and_then(|d| d.extents)
                .filter(|_| app.view().visibility.extents)
            {
                let edge = Rect::from_two_pos(
                    window_pos(pane, &camera, extents.edge_mm.min),
                    window_pos(pane, &camera, extents.edge_mm.max),
                );
                legend::extents_caption(ui, &painter, app, &extents, edge, &covers);
            }
            // The Viewport is a card: its corners, which the wgpu callback
            // and the overlay fill square, go back to the backdrop.
            mask_corners(&painter, pane, theme::CARD_RADIUS_PX, palette);
            pane
        })
        .inner
}
