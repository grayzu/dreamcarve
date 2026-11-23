// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's floating controls (`docs/specs/phase-4/redesign.md`,
//! "The Viewport's floating controls"; functional spec, capabilities 72,
//! 73 and 74; `experience.md`, section 9): three groups over the
//! Viewport's card, each a `surface.raised` pill with a hairline and the
//! one shadow, 12 px in from the card's edges.
//!
//! - **Top left**, the tools and the view: *Select*, *Draw* and the *Tab
//!   tool* (the one in use shows pressed), then *Fit*, *Zoom out*, the
//!   zoom as a percentage, and *Zoom in*. *Draw* holds the drawing tool
//!   last used and arms it; the chevron beside it opens the drawing
//!   tools, each with its icon, name and key (phase 5, capability 81),
//!   and under them *Trim*. *Edit nodes* stands between *Select* and
//!   *Draw*. The percentage is of 96 pixels to the
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
use dc_app::tool::ToolGroup;
use dc_app::view::ColourMode;
use dc_app::viewport::camera::{Camera, Insets};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

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
/// The tool strip's buttons: two that select and edit, the three groups
/// and Text, three that act on what is drawn, and Snapping.
const STRIP_BUTTONS: f32 = 10.0;
/// The rules that divide the strip into its four segments.
const STRIP_RULES: f32 = 3.0;
/// The tool strip's height, px: its buttons, the gaps between what it
/// holds, its rules, and the pill round them.
const STRIP_PX: f32 = PILL_PX - CONTROL_PX
    + STRIP_BUTTONS * CONTROL_PX
    + STRIP_RULES * RULE_PX
    + (STRIP_BUTTONS + STRIP_RULES - 1.0) * GAP_PX;
/// What the tool strip takes of the pane's left, px: its inset, itself,
/// and the same inset again. The tool bar and Fit keep clear of it.
pub(super) const STRIP_CLEAR_PX: f32 = INSET_PX + PILL_PX + INSET_PX;
/// The drawing tools' popover, px wide.
const DRAW_MENU_PX: f32 = 200.0;
/// A row of the drawing tools' popover, px high.
const DRAW_ROW_PX: f32 = 32.0;
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

/// How far under and left of the origin its label starts, px.
pub(super) const ORIGIN_LABEL_OFFSET_PX: f32 = 4.0;

/// The room Fit leaves round the stock for the floating controls, px: the
/// pills along the top, the read-out along the bottom under the origin's
/// label, and at each side the origin's label and a locked shape's
/// padlock.
pub const FIT_INSETS: Insets = Insets {
    // The tool strip, with its inset at each side.
    left_px: 60.0,
    // The inset, a pill, the inset.
    top_px: 60.0,
    right_px: 40.0,
    // The origin's label, the read-out, the inset: the read-out floats
    // at the pane's bottom left, where the origin of a stock fitted to
    // the pane's height is, and it must not cover the `0,0`.
    bottom_px: 64.0,
};

/// The ways Fit can stand clear of what floats over a pane whose right
/// `right_px` and bottom `bottom_px` are taken (an Inspector lying over
/// the Viewport, the Warnings drawer), with the depth legend `legend` px
/// in size hung under the top-right pill when it shows: the base insets
/// alone, or, with the legend, beside it and under it. Fit takes
/// whichever draws the stock largest (`Camera::fit_clear`). The
/// target-editing bar is not stood clear of: it lies over the room the
/// pills keep, and the drawing holds still when the mode starts.
///
/// A drawing tool's bar is stood clear of too (`tool_bar_px`, one row of
/// it): what is drawn must not start under it.
pub(super) fn ways(
    (right_px, bottom_px): (f32, f32),
    legend: Option<egui::Vec2>,
    tool_bar_px: f32,
) -> Vec<Insets> {
    let base = Insets {
        right_px: FIT_INSETS.right_px + f64::from(right_px),
        bottom_px: FIT_INSETS.bottom_px + f64::from(bottom_px),
        // The armed tool's bar hangs under the pills.
        top_px: FIT_INSETS.top_px + f64::from(tool_bar_px),
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
    // It takes the pointer and not the focus: a pill is no Tab stop, its
    // controls are.
    ui.interact(
        rect,
        Id::unique(("dc.viewport.float", salt)),
        Sense::CLICK | Sense::DRAG,
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
    /// Whether the zoom percentage shows between them: it goes before
    /// they do.
    level: bool,
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
            level: true,
            layers_px,
            label: true,
        };
        let fits = |fit: &Self| {
            let view_px = if view {
                BETWEEN_PX + fit.view_px()
            } else {
                0.0
            };
            fit.tools_px() + view_px <= room_px
        };
        // What goes, in order: the label, the percentage, the two zoom
        // buttons.
        if view && !fits(&fit) {
            fit.label = false;
        }
        if !fits(&fit) {
            fit.level = false;
        }
        if !fits(&fit) {
            fit.zoom = false;
        }
        fit
    }

    /// The top-left pill's width, px: Fit, and with the zoom controls a
    /// rule and them. The tools are in the strip under it.
    fn tools_px(&self) -> f32 {
        let padding = PILL_PX - CONTROL_PX;
        let zoom = RULE_PX + 2.0 * CONTROL_PX + 3.0 * GAP_PX;
        match (self.zoom, self.level) {
            (true, true) => padding + CONTROL_PX + zoom + self.zoom_px + GAP_PX,
            (true, false) => padding + CONTROL_PX + zoom,
            (false, _) => padding + CONTROL_PX,
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

/// The icon of drawing tool `drawing`.
pub(crate) const fn drawing_icon(drawing: dc_app::Drawing) -> Icon {
    use dc_app::Drawing;
    match drawing {
        Drawing::Line => Icon::Slash,
        Drawing::Arc => Icon::Radius,
        Drawing::Spline => Icon::Spline,
        Drawing::Rectangle => Icon::Square,
        Drawing::Circle => Icon::Circle,
        Drawing::Ellipse => Icon::Ellipse,
        Drawing::Polygon => Icon::Hexagon,
        Drawing::Slot => Icon::Pill,
        Drawing::Gear => Icon::Cog,
        Drawing::Text => Icon::Type,
    }
}

/// The key that arms `drawing`, as a menu writes it; empty for a tool
/// with none.
pub(crate) fn drawing_keys(ctx: &egui::Context, drawing: dc_app::Drawing) -> String {
    keyboard::command_of(drawing)
        .and_then(keyboard::binding)
        .map_or_else(String::new, |(modifiers, key)| {
            crate::shell::window::shortcut_text(ctx, modifiers, key)
        })
}

/// The drawing tool the Draw button holds: the one last used, or the
/// Rectangle where this build does not have that one.
pub(crate) fn held_drawing(app: &dc_app::App) -> dc_app::Drawing {
    let held = app.prefs().drawing_tool;
    if held.is_ready() {
        held
    } else {
        dc_app::Drawing::Rectangle
    }
}

/// The id of group `group`'s list of tools, which its button toggles.
fn flyout_id(group: ToolGroup) -> Id {
    Id::unique(("dc.viewport.tools.flyout", group))
}

/// Whether a group's list of tools is open. It is no menu to dismiss: a
/// press on the Viewport while it shows is the tool's, and a tool's key
/// still arms it.
pub(crate) fn flyout_open(ctx: &egui::Context) -> bool {
    ToolGroup::ALL
        .into_iter()
        .any(|group| egui::Popup::is_id_open(ctx, flyout_id(group)))
}

/// One drawing tool's row in a group's list: its icon and its name at the
/// left, its key at the right, the tool in use on the selected surface.
fn draw_row(ui: &mut Ui, icon: Icon, name: &str, keys: &str, chosen: bool) -> egui::Response {
    let palette = Palette::current(ui);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), DRAW_ROW_PX),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, chosen, name));
    let painter = ui.painter();
    let fill = if chosen {
        palette.accent_soft
    } else if response.hovered() || response.has_focus() {
        palette.surface_hover
    } else {
        theme::NONE
    };
    painter.rect_filled(rect, theme::CONTROL_RADIUS_PX, fill);
    let glyph = icon.galley(ui, theme::ICON_SMALL_PX, palette.text_primary);
    let middle = rect.center().y;
    painter.galley(
        egui::pos2(rect.left() + 8.0, middle - glyph.size().y / 2.0),
        glyph,
        palette.text_primary,
    );
    let body = TextStyle::Body.resolve(ui.style());
    let label = painter.layout_no_wrap(name.to_owned(), body.clone(), palette.text_primary);
    painter.galley(
        egui::pos2(
            rect.left() + 8.0 + theme::ICON_SMALL_PX + 8.0,
            middle - label.size().y / 2.0,
        ),
        label,
        palette.text_primary,
    );
    let key = painter.layout_no_wrap(keys.to_owned(), body, palette.text_secondary);
    painter.galley(
        egui::pos2(
            rect.right() - 8.0 - key.size().x,
            middle - key.size().y / 2.0,
        ),
        key,
        palette.text_secondary,
    );
    crate::widgets::button::focus_ring(ui, &response);
    response
}

/// A segment's rule across the tool strip: what separates the tools that
/// select and edit, those that draw, those that act on what is drawn,
/// and snapping.
fn strip_rule(ui: &mut Ui) {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(CONTROL_PX, RULE_PX), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.5, palette.border_strong),
    );
}

/// One group of drawing tools in the strip: a button that shows the tool
/// of the group last used and arms it, a mark at its corner that it
/// holds more, and beside it the list of the group's tools, each with
/// its icon, name and key. A click arms the tool and opens the list; a
/// row arms that tool and closes it.
fn group_button(
    ui: &mut Ui,
    group: ToolGroup,
    state: &FrameState<'_>,
    enabled: bool,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let held = state.app.held(group);
    let in_use = match state.app.tool() {
        dc_app::Tool::Draw(drawing) if drawing.group() == Some(group) => Some(drawing),
        _ => None,
    };
    let name = strings.get(group.name());
    let held_name = strings.get(held.name());
    let keys = drawing_keys(ui.ctx(), held);
    let others: Vec<&str> = group
        .tools()
        .iter()
        .filter(|tool| **tool != held)
        .map(|tool| strings.get(tool.name()))
        .collect();
    let help = strings.fmt(
        StrKey::AppViewportGroupHelp,
        &[("tool", held_name), ("others", &others.join(", "))],
    );
    let tip = if keys.is_empty() {
        help
    } else {
        format!("{help} ({keys})")
    };
    let spec = ButtonSpec::new(Kind::Quiet, "")
        .icon(drawing_icon(held), theme::ICON_SMALL_PX)
        .width(CONTROL_PX)
        .height(CONTROL_PX)
        .named(name)
        .tooltip(&tip)
        .pressed(in_use.is_some())
        .enabled(enabled);
    let response = button(ui, spec);
    // The mark that the button holds more than one tool.
    let corner = response.rect.right_bottom() - egui::vec2(3.0, 3.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            corner,
            corner - egui::vec2(5.0, 0.0),
            corner - egui::vec2(0.0, 5.0),
        ],
        palette.text_secondary,
        Stroke::NONE,
    ));
    if response.clicked() {
        out.push(Action::Arm(dc_app::Tool::Draw(held)));
    }
    egui::Popup::from_toggle_button_response(&response)
        .id(flyout_id(group))
        .align(egui::RectAlign::RIGHT_START)
        .gap(10.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .width(DRAW_MENU_PX)
        .frame(theme::popover(palette, 6))
        .show(|ui| {
            ui.set_width(DRAW_MENU_PX - 12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            for drawing in group.tools().iter().copied() {
                let keys = drawing_keys(ui.ctx(), drawing);
                let name = strings.get(drawing.name());
                let chosen = in_use == Some(drawing);
                if draw_row(ui, drawing_icon(drawing), name, &keys, chosen).clicked() {
                    out.push(Action::Arm(dc_app::Tool::Draw(drawing)));
                }
            }
        });
}

/// The snapping button's list: the master switch, then a tick per kind,
/// as View ▸ Snapping and View ▸ Snap To have them.
fn snap_menu(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let snap = &state.prefs.snap;
    let all = crate::shell::menu::checked_item(
        ui,
        strings.get(StrKey::AppViewportSnapping),
        snap.enabled,
    );
    if all.clicked() {
        let mut next = snap.clone();
        next.enabled = !snap.enabled;
        out.push(Action::SetSnap(next));
        ui.close();
    }
    ui.separator();
    crate::shell::menu::snap_to(ui, state, out);
}

/// The tool strip, down the Viewport's left edge under the top-left
/// pill (review feedback, 2026-11-23; Affinity's tools are laid out so):
/// the tool in use shows pressed, and rules divide it into what selects
/// and edits (*Select*, *Edit nodes*), what draws (the three groups and
/// *Text*), what acts on what is drawn (*Trim*, *Measure*, the *Tab
/// tool*), and *Snapping*. Returns the strip's rectangle.
fn strip(
    ui: &mut Ui,
    (pane, top): (Rect, f32),
    state: &FrameState<'_>,
    opacity: f32,
    out: &mut Vec<Action>,
) -> Rect {
    let strings = state.app.strings();
    let editing_targets = state.app.target_editing().is_some();
    let rect = Rect::from_min_size(
        egui::pos2(
            pane.left() + INSET_PX,
            top + INSET_PX + PILL_PX + BETWEEN_PX,
        ),
        egui::vec2(PILL_PX, STRIP_PX),
    );
    let palette = Palette::current(ui);
    // It takes the pointer and not the focus, as a pill does.
    ui.interact(
        rect,
        Id::unique(("dc.viewport.float", "strip")),
        Sense::CLICK | Sense::DRAG,
    );
    let padding = (PILL_PX - CONTROL_PX) / 2.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("dc.viewport.float.ui", "strip"))
            .max_rect(rect.shrink(padding))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.set_opacity(opacity);
    let radius = theme::pill(rect.width());
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
    child.spacing_mut().item_spacing.y = GAP_PX;
    let ui = &mut child;
    let in_use = state.app.tool();
    let arm = |ui: &mut Ui,
               (key, help, icon): (StrKey, StrKey, Icon),
               shortcut: Option<Shortcut>,
               tool: dc_app::Tool,
               out: &mut Vec<Action>| {
        let tip = shortcut.map_or_else(
            || strings.get(help).to_owned(),
            |shortcut| with_keys(ui, strings.get(help), shortcut),
        );
        let pressed = in_use == tool && !editing_targets;
        let spec = self::tool(strings, key, icon)
            .tooltip(&tip)
            .pressed(pressed)
            .enabled(!editing_targets);
        if button(ui, spec).clicked() {
            out.push(Action::Arm(tool));
        }
    };
    // What selects and edits.
    arm(
        ui,
        (
            StrKey::AppViewportSelect,
            StrKey::AppViewportSelectHelp,
            Icon::MousePointer,
        ),
        Some(Shortcut::SelectTool),
        dc_app::Tool::Select,
        out,
    );
    arm(
        ui,
        (
            StrKey::AppViewportNodes,
            StrKey::AppViewportNodesHelp,
            Icon::Waypoints,
        ),
        Some(Shortcut::EditNodes),
        dc_app::Tool::Nodes,
        out,
    );
    strip_rule(ui);
    // What draws: the three groups, and Text.
    for group in ToolGroup::ALL {
        group_button(ui, group, state, !editing_targets, out);
    }
    arm(
        ui,
        (
            StrKey::DrawNameText,
            StrKey::AppViewportTextHelp,
            Icon::Type,
        ),
        keyboard::command_of(dc_app::Drawing::Text),
        dc_app::Tool::Draw(dc_app::Drawing::Text),
        out,
    );
    strip_rule(ui);
    // What acts on what is drawn.
    arm(
        ui,
        (
            StrKey::AppViewportTrim,
            StrKey::AppViewportTrimHelp,
            Icon::Scissors,
        ),
        None,
        dc_app::Tool::Trim,
        out,
    );
    arm(
        ui,
        (
            StrKey::AppViewportMeasure,
            StrKey::AppViewportMeasureHelp,
            Icon::Ruler,
        ),
        Some(Shortcut::Measure),
        dc_app::Tool::Measure,
        out,
    );
    let tab_armed = in_use == dc_app::Tool::Tab;
    let tab = tool(strings, StrKey::AppViewportTabTool, Icon::BookmarkPlus)
        .tooltip(strings.get(StrKey::AppViewportTabToolHelp))
        .pressed(tab_armed)
        .enabled(!editing_targets);
    if button(ui, tab).clicked() {
        out.push(Action::Arm(if tab_armed {
            dc_app::Tool::Select
        } else {
            dc_app::Tool::Tab
        }));
    }
    strip_rule(ui);
    // Snapping: pressed while it is on; its list has the kinds.
    let snap_tip = with_keys(
        ui,
        strings.get(StrKey::AppViewportSnappingHelp),
        Shortcut::ToggleSnapping,
    );
    let snap = tool(strings, StrKey::AppViewportSnapping, Icon::Magnet)
        .tooltip(&snap_tip)
        .pressed(state.prefs.snap.enabled);
    let snap = button(ui, snap);
    egui::Popup::menu(&snap)
        .align(egui::RectAlign::RIGHT_START)
        .gap(10.0)
        .show(|ui| snap_menu(ui, state, out));
    rect
}

/// The top-left pill: *Fit* and, where there is room (`fit`), *Zoom
/// out*, the zoom and *Zoom in*. `top` is the pane's top edge less
/// whatever lies across it. Returns the pill's rectangle and the camera
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
    let zoom = zoom_text(strings, camera);
    let zoom_px = fit.zoom_px;
    let rect = Rect::from_min_size(
        egui::pos2(pane.left() + INSET_PX, top + INSET_PX),
        egui::vec2(fit.tools_px(), PILL_PX),
    );
    let mut zoomed = None;
    pill(ui, rect, "tools", opacity, |ui| {
        let fit_tip = with_keys(ui, strings.get(StrKey::AppViewportFit), Shortcut::FitView);
        let fit_button = tool(strings, StrKey::AppViewportFit, Icon::Maximize).tooltip(&fit_tip);
        if button(ui, fit_button).clicked() {
            out.push(Action::FitView);
        }
        if !fit.zoom {
            return;
        }
        rule(ui);
        let stepped = |factor: f64| {
            let mut next = *camera;
            next.zoom_by(factor);
            next
        };
        if button(ui, tool(strings, StrKey::AppViewportZoomOut, Icon::ZoomOut)).clicked() {
            zoomed = Some(stepped(1.0 / ZOOM_STEP));
        }
        if fit.level {
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
        // Target editing has the drawing to itself: the view holds.
        if state.inert() {
            ui.disable();
        }
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
    /// The tool strip under it.
    pub strip: Rect,
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
        [self.tools, self.strip]
            .into_iter()
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
    let strip = strip(ui, (pane, top), state, opacity, out);
    let view = with_view.then(|| view(ui, (top, right), fit, state, opacity, out));
    let readout = readout(ui, (pane.left(), bottom), state, cursor_mm, opacity);
    Shown {
        tools,
        strip,
        view,
        readout,
        camera: zoomed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fit's insets are the floating controls' room: a pill and its two
    /// insets along the top; the origin's label, the read-out and its inset
    /// along the bottom.
    #[test]
    fn fit_insets_hold_the_pills() {
        // The room under the stock for the origin's `0,0`: the label
        // hangs 4 px under the origin, a 12-px line is 16 px, and 4 px
        // stand between it and the read-out.
        let origin_label_px = ORIGIN_LABEL_OFFSET_PX + 16.0 + 4.0;
        assert!((FIT_INSETS.top_px - f64::from(INSET_PX + PILL_PX + INSET_PX)).abs() < 1e-9);
        assert!(
            (FIT_INSETS.bottom_px - f64::from(origin_label_px + READOUT_PX + INSET_PX)).abs()
                < 1e-9
        );
    }

    /// With the tools in the strip, the top-left pill is Fit and the
    /// zoom, and both top pills show whole down to the Viewport's least
    /// width, 520 px. Narrower than the Viewport ever is, *Layers* is its
    /// icon alone, then the zoom percentage goes, then the two zoom
    /// buttons; and the pills never overlap while they fit at all.
    #[test]
    fn narrow_panes_drop_the_label_then_the_zoom() {
        let (zoom_px, layers_px) = (ZOOM_PX, 88.0);
        let wide = Fit::of(900.0 - 24.0, zoom_px, layers_px, true);
        assert!(wide.zoom && wide.level && wide.label);
        let least = Fit::of(theme::VIEWPORT_MIN_PX - 24.0, zoom_px, layers_px, true);
        assert!(least.zoom && least.level && least.label, "{least:?}");
        assert!(least.tools_px() + BETWEEN_PX + least.view_px() <= theme::VIEWPORT_MIN_PX - 24.0);
        let narrower = Fit::of(440.0, zoom_px, layers_px, true);
        assert!(
            narrower.zoom && narrower.level && !narrower.label,
            "{narrower:?}"
        );
        let narrow = Fit::of(380.0, zoom_px, layers_px, true);
        assert!(narrow.zoom && !narrow.level && !narrow.label, "{narrow:?}");
        let narrowest = Fit::of(330.0, zoom_px, layers_px, true);
        assert!(!narrowest.zoom && !narrowest.label, "{narrowest:?}");
        assert!(narrowest.tools_px() + BETWEEN_PX + narrowest.view_px() <= 330.0);
        // With no toolpath there is one pill, which keeps its zoom.
        let alone = Fit::of(200.0, zoom_px, layers_px, false);
        assert!(alone.zoom && alone.level);
    }

    /// The tool strip holds its ten buttons and three rules, and Fit
    /// leaves its width clear at the pane's left.
    #[test]
    fn the_strip_has_room_for_its_tools() {
        let inside = STRIP_PX - (PILL_PX - CONTROL_PX);
        let held = STRIP_BUTTONS * CONTROL_PX + STRIP_RULES * RULE_PX;
        let gaps = (STRIP_BUTTONS + STRIP_RULES - 1.0) * GAP_PX;
        assert!((inside - held - gaps).abs() < 1e-3);
        assert!(FIT_INSETS.left_px >= f64::from(STRIP_CLEAR_PX));
    }

    /// With the legend hung at the top right, Fit may stand beside it or
    /// under it; without it, there is the one way.
    #[test]
    fn ways_stand_beside_the_legend_or_under_it() {
        assert_eq!(ways((0.0, 0.0), None, 0.0), [FIT_INSETS]);
        // A drawing tool's bar takes its row off the top.
        let drawing = ways((0.0, 0.0), None, 56.0);
        assert!((drawing[0].top_px - 116.0).abs() < 1e-9, "{:?}", drawing[0]);
        let with = ways((0.0, 0.0), Some(egui::vec2(200.0, 280.0)), 0.0);
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
        let narrow = ways((300.0, 248.0), Some(egui::vec2(200.0, 280.0)), 0.0);
        assert!((narrow[0].right_px - 524.0).abs() < 1e-9, "{:?}", narrow[0]);
        assert!((narrow[1].right_px - 340.0).abs() < 1e-9, "{:?}", narrow[1]);
        assert!(
            narrow
                .iter()
                .all(|way| (way.bottom_px - 312.0).abs() < 1e-9)
        );
    }
}
