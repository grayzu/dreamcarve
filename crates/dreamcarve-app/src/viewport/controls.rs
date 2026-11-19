// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's floating controls (`docs/specs/phase-4/redesign.md`,
//! "The Viewport's floating controls"; functional spec, capabilities 72,
//! 73 and 74; `experience.md`, section 9): three groups over the
//! Viewport's card, each a `surface.raised` pill with a hairline and the
//! one shadow, 12 px in from the card's edges.
//!
//! - **Top left**, the tools and the view: *Select* and the *Tab tool*
//!   (the one in use shows pressed), then *Fit*, *Zoom out*, the zoom as
//!   a percentage, and *Zoom in*. The percentage is of 96 pixels to the
//!   inch (`dc_app::viewport::camera::ACTUAL_PX_PER_MM`), about the real
//!   size on most displays, and a click on it sets the zoom to 100 %.
//! - **Top right**, once the project has a toolpath: the *By operation /
//!   By depth* control and *Layers*, which opens the toolpath view
//!   popover ([`crate::shell::toolpaths`]). The depth legend hangs under
//!   it while By depth is on ([`super::legend`]).
//! - **Bottom left**, the read-out (`dc_app::view::readout`): the
//!   cursor's position while the pointer is over the Viewport and, while
//!   the program extents are shown, the Z range.
//!
//! They fade to 60 % while a drag is under way on the Viewport, and they
//! never cover the stock at Fit: [`FIT_INSETS`] is the room Fit leaves
//! for them, and [`ways`] adds the two ways to stand clear of the legend.
//! Each takes the clicks on it, so none reaches the drawing under it. In
//! a pane too narrow for both top pills, *Layers* is its icon alone, and
//! in a narrower one still the zoom controls go ([`Fit::of`]): *Fit* and
//! the wheel remain.

use dc_app::StrKey;
use dc_app::view::ColourMode;
use dc_app::viewport::camera::{Camera, Insets};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use super::tab_tool;
use crate::keyboard::{self, Command as Shortcut};
use crate::shell::theme::{self, Palette};
use crate::shell::toolpaths;
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented_sized;

/// A floating pill's height, px: a 28-px control and the pill round it.
pub(super) const PILL_PX: f32 = 36.0;
/// A control in a pill, px high.
const CONTROL_PX: f32 = 28.0;
/// The read-out's pill, px high.
const READOUT_PX: f32 = 28.0;
/// The gap between a pill's controls, px.
const GAP_PX: f32 = 2.0;
/// The room a rule between two groups of a pill takes, px.
const RULE_PX: f32 = 9.0;
/// The gap between the colour mode and *Layers*, px.
const MODE_GAP_PX: f32 = 6.0;
/// The zoom percentage's width, px: room for four digits, so the pill
/// holds still while the zoom changes.
const ZOOM_PX: f32 = 56.0;
/// The colour mode control's width, px.
const MODE_PX: f32 = 184.0;
/// The least gap between the two top pills, px.
const BETWEEN_PX: f32 = 8.0;
/// The cursor's part of the read-out, px: room for `X 1234.5  Y 1234.5`,
/// so the pill holds still while the pointer moves.
const CURSOR_PX: f32 = 124.0;
/// One step of *Zoom in* and *Zoom out*.
const ZOOM_STEP: f64 = 1.25;
/// How much of themselves the controls show while a drag is under way.
const DRAG_OPACITY: f32 = 0.6;
/// How far a floating control sits in from the card's edge, px.
const INSET_PX: f32 = theme::FLOAT_INSET_PX;
/// The gap between the top-right pill and the legend under it, px.
pub(super) const LEGEND_GAP_PX: f32 = 8.0;

/// The room Fit leaves round the stock for the floating controls, px: the
/// pills along the top, the read-out along the bottom, and at each side
/// the origin's label and a locked shape's padlock.
pub const FIT_INSETS: Insets = Insets {
    left_px: 40.0,
    // The inset, a pill, the inset.
    top_px: 60.0,
    right_px: 40.0,
    // The inset, the read-out, the inset.
    bottom_px: 52.0,
};

/// The ways Fit can stand clear of what floats over a pane whose right
/// `right_px` and bottom `bottom_px` are taken (an Inspector lying over
/// the Viewport, the Warnings drawer), with the depth legend `legend` px
/// in size hung under the top-right pill when it shows: the base insets
/// alone, or, with the legend, beside it and under it. Fit takes
/// whichever draws the stock largest (`Camera::fit_clear`). The
/// target-editing bar is not stood clear of: it lies over the room the
/// pills keep, and the drawing holds still when the mode starts.
pub(super) fn ways((right_px, bottom_px): (f32, f32), legend: Option<egui::Vec2>) -> Vec<Insets> {
    let base = Insets {
        right_px: FIT_INSETS.right_px + f64::from(right_px),
        bottom_px: FIT_INSETS.bottom_px + f64::from(bottom_px),
        ..FIT_INSETS
    };
    match legend {
        None => vec![base],
        Some(size) => vec![
            Insets {
                right_px: f64::from(right_px + INSET_PX + size.x + INSET_PX),
                ..base
            },
            Insets {
                top_px: base.top_px + f64::from(LEGEND_GAP_PX + size.y),
                ..base
            },
        ],
    }
}

/// How much of themselves the controls show this frame: all, easing to
/// [`DRAG_OPACITY`] while `dragging`.
pub(super) fn opacity(ui: &Ui, dragging: bool) -> f32 {
    let t = ui
        .ctx()
        .animate_bool(Id::unique("dc.viewport.controls.fade"), dragging);
    1.0 - (1.0 - DRAG_OPACITY) * t
}

/// A pill in `rect`, at `opacity`: it takes the clicks on it, and
/// `body` lays its controls out left to right, centred.
fn pill(ui: &mut Ui, rect: Rect, salt: &str, opacity: f32, body: impl FnOnce(&mut Ui)) {
    let palette = Palette::current(ui);
    ui.interact(
        rect,
        Id::unique(("dc.viewport.float", salt)),
        Sense::click_and_drag(),
    );
    let padding = (rect.height() - CONTROL_PX) / 2.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("dc.viewport.float.ui", salt))
            .max_rect(rect.shrink(padding))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_opacity(opacity);
    let radius = theme::pill(rect.height());
    child
        .painter()
        .add(theme::shadow(palette).as_shape(rect, radius));
    child.painter().rect(
        rect,
        radius,
        palette.surface_raised,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Inside,
    );
    child.spacing_mut().item_spacing.x = GAP_PX;
    body(&mut child);
}

/// A 1-px rule between two groups of a pill.
fn rule(ui: &mut Ui) {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(RULE_PX, CONTROL_PX), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range().shrink(6.0),
        Stroke::new(1.0, palette.border_default),
    );
}

/// An icon button of a pill, named `key`.
fn tool<'a>(strings: &'a dc_app::Strings, key: StrKey, icon: Icon) -> ButtonSpec<'a> {
    ButtonSpec::new(Kind::Quiet, "")
        .icon(icon, theme::ICON_SMALL_PX)
        .width(CONTROL_PX)
        .height(CONTROL_PX)
        .named(strings.get(key))
        .tooltip(strings.get(key))
}

/// `label` with the keys of `shortcut` after it, as a tooltip writes
/// them.
fn with_keys(ui: &Ui, label: &str, shortcut: Shortcut) -> String {
    keyboard::binding(shortcut).map_or_else(
        || label.to_owned(),
        |(modifiers, key)| {
            let keys = crate::shell::window::shortcut_text(ui.ctx(), modifiers, key);
            format!("{label} ({keys})")
        },
    )
}

/// The zoom as the read-out writes it: a whole percentage.
fn zoom_text(strings: &dc_app::Strings, camera: &Camera) -> String {
    strings.fmt(
        StrKey::AppViewportZoom,
        &[("percent", &format!("{:.0}", camera.zoom_percent()))],
    )
}

/// What the two top pills hold in the room they have.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    /// The zoom percentage's width, px.
    zoom_px: f32,
    /// Whether the zoom controls show: not in the narrowest panes.
    zoom: bool,
    /// *Layers*' width with its label, px.
    layers_px: f32,
    /// Whether *Layers* shows its label: not in a narrow pane.
    label: bool,
}

impl Fit {
    /// What fits in `room_px` between the pane's insets, with a zoom
    /// percentage `zoom_px` wide and a labelled *Layers* `layers_px` wide,
    /// and the top-right pill there or not (`view`).
    fn of(room_px: f32, zoom_px: f32, layers_px: f32, view: bool) -> Self {
        let mut fit = Self {
            zoom_px,
            zoom: true,
            layers_px,
            label: true,
        };
        if !view {
            fit.zoom = fit.tools_px() <= room_px;
            return fit;
        }
        let fits = |fit: &Self| fit.tools_px() + BETWEEN_PX + fit.view_px() <= room_px;
        if !fits(&fit) {
            fit.label = false;
        }
        if !fits(&fit) {
            fit.zoom = false;
        }
        fit
    }

    /// The top-left pill's width, px.
    fn tools_px(&self) -> f32 {
        let padding = PILL_PX - CONTROL_PX;
        let tools = 3.0 * CONTROL_PX + RULE_PX + 3.0 * GAP_PX;
        if self.zoom {
            padding + tools + 2.0 * CONTROL_PX + self.zoom_px + 3.0 * GAP_PX
        } else {
            padding + tools
        }
    }

    /// The top-right pill's width, px.
    fn view_px(&self) -> f32 {
        let layers = if self.label {
            self.layers_px
        } else {
            CONTROL_PX
        };
        PILL_PX - CONTROL_PX + MODE_PX + MODE_GAP_PX + layers
    }
}

/// The top-left pill: *Select* and the *Tab tool*, then *Fit* and, where
/// there is room (`fit`), *Zoom out*, the zoom and *Zoom in*. `top` is
/// the pane's top edge less whatever lies across it. Returns the camera
/// a zoom control set.
fn tools(
    ui: &mut Ui,
    (pane, top): (Rect, f32),
    fit: Fit,
    state: &FrameState<'_>,
    camera: &Camera,
    opacity: f32,
    out: &mut Vec<Action>,
) -> (Rect, Option<Camera>) {
    let strings = state.app.strings();
    let editing_targets = state.app.target_editing().is_some();
    let zoom = zoom_text(strings, camera);
    let zoom_px = fit.zoom_px;
    let rect = Rect::from_min_size(
        egui::pos2(pane.left() + INSET_PX, top + INSET_PX),
        egui::vec2(fit.tools_px(), PILL_PX),
    );
    let mut zoomed = None;
    pill(ui, rect, "tools", opacity, |ui| {
        // The tools (#546): the one in use shows as pressed. Target
        // editing has the drawing to itself.
        let armed = tab_tool::armed(ui.ctx());
        let select = tool(strings, StrKey::AppViewportSelect, Icon::MousePointer)
            .tooltip(strings.get(StrKey::AppViewportSelectHelp))
            .pressed(!armed && !editing_targets)
            .enabled(!editing_targets);
        if button(ui, select).clicked() {
            tab_tool::arm(ui.ctx(), false);
        }
        let tab = tool(strings, StrKey::AppViewportTabTool, Icon::BookmarkPlus)
            .tooltip(strings.get(StrKey::AppViewportTabToolHelp))
            .pressed(armed)
            .enabled(!editing_targets);
        if button(ui, tab).clicked() {
            tab_tool::arm(ui.ctx(), !armed);
        }
        rule(ui);
        let fit_tip = with_keys(ui, strings.get(StrKey::AppViewportFit), Shortcut::FitView);
        let fit_button = tool(strings, StrKey::AppViewportFit, Icon::Maximize).tooltip(&fit_tip);
        if button(ui, fit_button).clicked() {
            out.push(Action::FitView);
        }
        if !fit.zoom {
            return;
        }
        let stepped = |factor: f64| {
            let mut next = *camera;
            next.zoom_by(factor);
            next
        };
        if button(ui, tool(strings, StrKey::AppViewportZoomOut, Icon::ZoomOut)).clicked() {
            zoomed = Some(stepped(1.0 / ZOOM_STEP));
        }
        let name = strings.fmt(StrKey::AppViewportZoomLevel, &[("zoom", &zoom)]);
        let level = ButtonSpec::new(Kind::Quiet, &zoom)
            .width(zoom_px)
            .height(CONTROL_PX)
            .named(&name)
            .tooltip(strings.get(StrKey::AppViewportZoomReset));
        if button(ui, level).clicked() {
            let mut next = *camera;
            next.set_zoom_percent(100.0);
            zoomed = Some(next);
        }
        if button(ui, tool(strings, StrKey::AppViewportZoomIn, Icon::ZoomIn)).clicked() {
            zoomed = Some(stepped(ZOOM_STEP));
        }
    });
    (rect, zoomed)
}

/// The top-right pill, once the project has a toolpath: the *By operation
/// / By depth* control and *Layers*, which opens the toolpath view
/// popover. `right` is the pane's right edge less whatever lies over it.
/// Returns the pill's rectangle.
fn view(
    ui: &mut Ui,
    (top, right): (f32, f32),
    fit: Fit,
    state: &FrameState<'_>,
    opacity: f32,
    out: &mut Vec<Action>,
) -> Rect {
    let strings = state.app.strings();
    let layers = strings.get(StrKey::AppViewportLayers);
    let width = fit.view_px();
    let rect = Rect::from_min_size(
        egui::pos2(right - INSET_PX - width, top + INSET_PX),
        egui::vec2(width, PILL_PX),
    );
    pill(ui, rect, "view", opacity, |ui| {
        ui.spacing_mut().item_spacing.x = MODE_GAP_PX;
        let modes = [
            strings.get(StrKey::AppToolpathsByOperation),
            strings.get(StrKey::AppToolpathsByDepth),
        ];
        let selected = usize::from(state.app.view().colour == ColourMode::ByDepth);
        let size = egui::vec2(MODE_PX, CONTROL_PX);
        if let Some(chosen) =
            segmented_sized(ui, Id::unique("dc.viewport.colour"), &modes, selected, size)
        {
            out.push(Action::SetColour(if chosen == 1 {
                ColourMode::ByDepth
            } else {
                ColourMode::ByOperation
            }));
        }
        // Its icon alone where the pane is too narrow for the label.
        let (label, width) = if fit.label {
            (layers, fit.layers_px)
        } else {
            ("", CONTROL_PX)
        };
        let spec = ButtonSpec::new(Kind::Quiet, label)
            .icon(Icon::Layers, theme::ICON_SMALL_PX)
            .width(width)
            .height(CONTROL_PX)
            .named(layers)
            .pressed(toolpaths::is_open(ui.ctx()))
            .tooltip(strings.get(StrKey::AppViewportLayersHelp));
        let response = button(ui, spec);
        toolpaths::popover(ui, &response, state.app, out);
    });
    rect
}

/// The bottom-left pill, over `bottom`: the cursor's position and the Z
/// range, each while it has something to say. Returns the pill's
/// rectangle, when it shows.
fn readout(
    ui: &mut Ui,
    (left, bottom): (f32, f32),
    state: &FrameState<'_>,
    cursor_mm: Option<dc_geometry::Point>,
    opacity: f32,
) -> Option<Rect> {
    let palette = Palette::current(ui);
    let said = dc_app::view::readout(state.app, cursor_mm);
    if said.cursor.is_none() && said.z_range.is_none() {
        return None;
    }
    let font = TextStyle::Small.resolve(ui.style());
    let z = said.z_range.as_ref().map(|z| {
        ui.painter()
            .layout_no_wrap(z.clone(), font.clone(), palette.text_secondary)
    });
    let padding = 12.0;
    let gap = 12.0;
    let cursor_px = if said.cursor.is_some() {
        CURSOR_PX
    } else {
        0.0
    };
    let z_px = z.as_ref().map_or(0.0, |z| z.size().x);
    let between = if said.cursor.is_some() && z.is_some() {
        gap
    } else {
        0.0
    };
    let rect = Rect::from_min_size(
        egui::pos2(left + INSET_PX, bottom - INSET_PX - READOUT_PX),
        egui::vec2(2.0 * padding + cursor_px + between + z_px, READOUT_PX),
    );
    let response = ui.interact(
        rect,
        Id::unique("dc.viewport.float.readout"),
        Sense::hover(),
    );
    let name = [said.cursor.as_deref(), said.z_range.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    let mut painter = ui.painter_at(rect.expand(32.0));
    painter.set_opacity(opacity);
    let radius = theme::pill(rect.height());
    painter.add(theme::shadow(palette).as_shape(rect, radius));
    painter.rect(
        rect,
        radius,
        palette.surface_raised,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + padding;
    if let Some(cursor) = said.cursor {
        let galley = painter.layout_no_wrap(cursor, font, palette.text_primary);
        painter.galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text_primary,
        );
        x += cursor_px + between;
    }
    if let Some(z) = z {
        painter.galley(
            egui::pos2(x, rect.center().y - z.size().y / 2.0),
            z,
            palette.text_secondary,
        );
    }
    Some(rect)
}

/// Where the floating controls are this frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shown {
    /// The top-left pill.
    pub tools: Rect,
    /// The top-right pill, when the project has a toolpath: the legend
    /// hangs under it.
    pub view: Option<Rect>,
    /// The read-out, while it has something to say.
    pub readout: Option<Rect>,
    /// The camera a zoom control set.
    pub camera: Option<Camera>,
}

impl Shown {
    /// The rectangles the controls cover.
    pub(super) fn rects(&self) -> Vec<Rect> {
        std::iter::once(self.tools)
            .chain(self.view)
            .chain(self.readout)
            .collect()
    }
}

/// What the controls are laid out against.
#[derive(Debug, Clone, Copy)]
pub(super) struct Place {
    /// The Viewport's card.
    pub pane: Rect,
    /// The card's top edge less whatever lies across it (the
    /// target-editing bar).
    pub top: f32,
    /// The card's right edge less whatever lies over it (an Inspector in
    /// a narrow window).
    pub right: f32,
    /// The card's bottom edge less whatever lies over it (the Warnings
    /// drawer).
    pub bottom: f32,
    /// How much of themselves they show ([`opacity`]).
    pub opacity: f32,
}

/// Draws the three groups over the pane.
pub(super) fn show(
    ui: &mut Ui,
    place: Place,
    state: &FrameState<'_>,
    camera: &Camera,
    cursor_mm: Option<dc_geometry::Point>,
    out: &mut Vec<Action>,
) -> Shown {
    let Place {
        pane,
        top,
        right,
        bottom,
        opacity,
    } = place;
    let strings = state.app.strings();
    let body = TextStyle::Body.resolve(ui.style());
    let measured = |text: String| {
        ui.painter()
            .layout_no_wrap(text, body.clone(), theme::PAINT_TIME)
            .size()
            .x
    };
    let zoom_px = (measured(zoom_text(strings, camera)) + 16.0).max(ZOOM_PX);
    let layers_px = measured(strings.get(StrKey::AppViewportLayers).to_owned())
        + theme::ICON_SMALL_PX
        + theme::GAP_PX
        + 20.0;
    let with_view = dc_app::view::has_toolpaths(state.app);
    let room = right - pane.left() - 2.0 * INSET_PX;
    let fit = Fit::of(room, zoom_px, layers_px, with_view);
    let (tools, zoomed) = tools(ui, (pane, top), fit, state, camera, opacity, out);
    let view = with_view.then(|| view(ui, (top, right), fit, state, opacity, out));
    let readout = readout(ui, (pane.left(), bottom), state, cursor_mm, opacity);
    Shown {
        tools,
        view,
        readout,
        camera: zoomed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fit's insets are the floating controls' room: a pill and its two
    /// insets along the top, the read-out and its two along the bottom.
    #[test]
    fn fit_insets_hold_the_pills() {
        assert!((FIT_INSETS.top_px - f64::from(INSET_PX + PILL_PX + INSET_PX)).abs() < 1e-9);
        assert!((FIT_INSETS.bottom_px - f64::from(INSET_PX + READOUT_PX + INSET_PX)).abs() < 1e-9);
    }

    /// In a wide pane both top pills show whole; at the Viewport's least
    /// width, 520 px, *Layers* is its icon alone; narrower, the zoom
    /// controls go too; and the pills never overlap while they fit at
    /// all.
    #[test]
    fn narrow_panes_drop_the_label_then_the_zoom() {
        let (zoom_px, layers_px) = (ZOOM_PX, 88.0);
        let wide = Fit::of(900.0 - 24.0, zoom_px, layers_px, true);
        assert!(wide.zoom && wide.label);
        let least = Fit::of(theme::VIEWPORT_MIN_PX - 24.0, zoom_px, layers_px, true);
        assert!(least.zoom && !least.label, "{least:?}");
        assert!(least.tools_px() + BETWEEN_PX + least.view_px() <= theme::VIEWPORT_MIN_PX - 24.0);
        let narrow = Fit::of(400.0, zoom_px, layers_px, true);
        assert!(!narrow.zoom && !narrow.label, "{narrow:?}");
        assert!(narrow.tools_px() + BETWEEN_PX + narrow.view_px() <= 400.0);
        // With no toolpath there is one pill, which keeps its zoom.
        let alone = Fit::of(400.0, zoom_px, layers_px, false);
        assert!(alone.zoom);
    }

    /// With the legend hung at the top right, Fit may stand beside it or
    /// under it; without it, there is the one way.
    #[test]
    fn ways_stand_beside_the_legend_or_under_it() {
        assert_eq!(ways((0.0, 0.0), None), [FIT_INSETS]);
        let with = ways((0.0, 0.0), Some(egui::vec2(200.0, 280.0)));
        assert_eq!(with.len(), 2);
        assert!(
            (with[0].right_px - 224.0).abs() < 1e-9,
            "beside: {:?}",
            with[0]
        );
        assert!((with[0].top_px - 60.0).abs() < 1e-9);
        assert!(
            (with[1].top_px - 348.0).abs() < 1e-9,
            "under: {:?}",
            with[1]
        );
        assert!((with[1].right_px - 40.0).abs() < 1e-9);
        // An Inspector over the pane's right moves both ways clear of it,
        // and the Warnings drawer over its bottom lifts them.
        let narrow = ways((300.0, 248.0), Some(egui::vec2(200.0, 280.0)));
        assert!((narrow[0].right_px - 524.0).abs() < 1e-9, "{:?}", narrow[0]);
        assert!((narrow[1].right_px - 340.0).abs() < 1e-9, "{:?}", narrow[1]);
        assert!(
            narrow
                .iter()
                .all(|way| (way.bottom_px - 300.0).abs() < 1e-9)
        );
    }
}
