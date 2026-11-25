// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The rail (`docs/specs/phase-4/redesign.md`, "The window" and "The
//! steps"; functional spec, capability 50): a 64-px card at the window's
//! left with the five step buttons in the workflow's order, each a 24-px
//! icon over an 11-px label, 48 × 52. The current step's button is filled
//! with `accent.soft` and its icon and label take `accent`. A step's
//! state ([`dc_app::steps::State`]) is the mark at its icon's top right:
//! nothing while it is empty, a check in `status.success` when complete,
//! a dot in `status.warning` or `status.error` when it needs attention.
//!
//! Pinned to the bottom: Warnings, with the count of the worst tier
//! found, which opens the Warnings drawer; Appearance (System, Light,
//! Dark); and Help (the Help menu's rows).

use dc_app::StrKey;
use dc_app::steps::{Severity, State, Step};
use dc_app::warnings::Level;
use eframe::egui::{self, Align, Layout, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::menu;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, shortcut_text};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring_at};
use crate::widgets::card::card;
use crate::widgets::icon::Icon;

/// The gap between the rail's buttons, px.
const GAP_PX: f32 = 4.0;
/// The rail's padding above its first button and under its last, px.
const PADDING_PX: f32 = 8.0;
/// A bottom button, px square.
const TOOL_PX: f32 = 40.0;
/// The state mark, px across.
const MARK_PX: f32 = 12.0;
/// The count badge's height, px.
const BADGE_PX: f32 = 16.0;

/// A step's icon.
const fn icon(step: Step) -> Icon {
    match step {
        Step::Design => Icon::Shapes,
        Step::Stock => Icon::Box,
        Step::Tools => Icon::Drill,
        Step::Operations => Icon::Route,
        Step::Simulate => Icon::Rotate3d,
        Step::Export => Icon::FileOutput,
    }
}

/// The shortcut that opens a step.
const fn shortcut(step: Step) -> Shortcut {
    match step {
        Step::Design => Shortcut::GoToDesign,
        Step::Stock => Shortcut::GoToStock,
        Step::Tools => Shortcut::GoToTools,
        Step::Operations => Shortcut::GoToOperations,
        Step::Simulate => Shortcut::GoToSimulate,
        Step::Export => Shortcut::GoToExport,
    }
}

/// Draws the rail; its card's rectangle.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) -> Rect {
    let palette = Palette::current(ui);
    #[allow(clippy::cast_possible_truncation, reason = "the window gap, 8 px")]
    let gap = theme::GAP_PX as i8;
    egui::Panel::left("rail")
        .exact_size(theme::RAIL_PX + theme::GAP_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(egui::Margin {
                    left: gap,
                    right: 0,
                    top: 0,
                    bottom: crate::shell::status::BOTTOM_PX,
                }),
        )
        .show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            card(ui, |ui| {
                if state.inert() {
                    ui.disable();
                }
                ui.spacing_mut().item_spacing.y = GAP_PX;
                ui.add_space(PADDING_PX);
                for step in Step::ALL {
                    step_button(ui, state, step, out);
                }
                // Pinned to the foot, and drawn from its top: Tab walks
                // the widgets in the order the frame makes them, and the
                // walk is Warnings, Appearance, Help (section 8,
                // "Keyboard focus").
                let room = ui.available_rect_before_wrap();
                let height = 3.0 * TOOL_PX + 2.0 * GAP_PX + PADDING_PX;
                let foot = Rect::from_min_max(
                    egui::pos2(room.left(), (room.bottom() - height).max(room.top())),
                    room.max,
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(foot)
                        .layout(Layout::top_down(Align::Center)),
                    |ui| {
                        warnings(ui, state, out);
                        appearance(ui, state, out);
                        help(ui, state, out);
                    },
                );
            });
            rect
        })
        .inner
}

/// What a step's tooltip says of its state.
fn state_line(state: &FrameState<'_>, step: Step) -> String {
    let strings = state.app.strings();
    let name = strings.get(step.label());
    let key = match state.states.of(step) {
        State::Empty => return name.to_owned(),
        State::Complete => StrKey::RailComplete,
        State::Attention(Severity::Warning) => StrKey::RailWarning,
        State::Attention(Severity::Error) => StrKey::RailError,
    };
    strings.fmt(key, &[("step", name)])
}

/// One step's button: the icon over the label, the state mark, and the
/// current step's fill. Heard as a tab, selected when it is the current
/// step.
fn step_button(ui: &mut Ui, state: &FrameState<'_>, step: Step, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let label = strings.get(step.label());
    let current = state.step == step;
    let (slot, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::RAIL_BUTTON_HEIGHT_PX),
        Sense::hover(),
    );
    let rect = Rect::from_center_size(
        slot.center(),
        egui::vec2(theme::RAIL_BUTTON_WIDTH_PX, theme::RAIL_BUTTON_HEIGHT_PX),
    );
    let enabled = ui.is_enabled();
    let response = ui.interact(
        rect,
        egui::Id::unique(("dc.rail.step", step.index())),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, enabled, current, label));
    if response.clicked() {
        out.push(Action::GoTo(step));
    }
    let radius = theme::INNER_CARD_RADIUS_PX;
    let colour = if !enabled {
        palette.text_disabled
    } else if current {
        ui.painter().rect_filled(rect, radius, palette.accent_soft);
        palette.accent
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius, palette.surface_hover);
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let glyph = icon(step).galley(ui, theme::ICON_RAIL_PX, colour);
    let glyph_rect = Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + 7.0 + theme::ICON_RAIL_PX / 2.0,
        ),
        egui::vec2(theme::ICON_RAIL_PX, theme::ICON_RAIL_PX),
    );
    ui.painter()
        .galley(glyph_rect.center() - glyph.size() / 2.0, glyph, colour);
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        TextStyle::Name(theme::RAIL_LABEL.into()).resolve(ui.style()),
        colour,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - text.size().x / 2.0,
            rect.bottom() - 6.0 - text.size().y,
        ),
        text,
        colour,
    );
    mark(ui, glyph_rect, state.states.of(step), current);
    focus_ring_at(ui, &response, radius);
    let keys = keyboard::binding(shortcut(step))
        .map(|(modifiers, key)| shortcut_text(ui.ctx(), modifiers, key));
    let line = state_line(state, step);
    let _ = response.on_hover_ui(|ui| {
        ui.label(line);
        if let Some(keys) = keys {
            ui.label(
                egui::RichText::new(keys)
                    .text_style(TextStyle::Small)
                    .color(palette.text_secondary),
            );
        }
    });
}

/// The state mark at the top right of the icon in `glyph`: a check when
/// complete, a dot when the step needs attention, on a disc of the
/// surface under it so it stands clear of the icon.
fn mark(ui: &Ui, glyph: Rect, state: State, current: bool) {
    let palette = Palette::current(ui);
    let centre = egui::pos2(glyph.right() + 1.0, glyph.top() + 1.0);
    let colour = match state {
        State::Empty => return,
        State::Complete => palette.status_success.stroke,
        State::Attention(Severity::Warning) => palette.status_warning.stroke,
        State::Attention(Severity::Error) => palette.status_error.stroke,
    };
    // The card's colour, or the card's under the current step's fill.
    ui.painter()
        .circle_filled(centre, MARK_PX / 2.0 + 1.5, palette.surface_panel);
    if current {
        ui.painter()
            .circle_filled(centre, MARK_PX / 2.0 + 1.5, palette.accent_soft);
    }
    if state == State::Complete {
        let check = Icon::Check.galley(ui, MARK_PX, colour);
        ui.painter()
            .galley(centre - check.size() / 2.0, check, colour);
    } else {
        ui.painter().circle_filled(centre, 4.0, colour);
    }
}

/// A bottom button: an icon alone, named `name`, with `name` as its
/// tooltip.
fn tool(ui: &mut Ui, icon: Icon, name: &str, pressed: bool) -> egui::Response {
    button(
        ui,
        ButtonSpec::new(Kind::Quiet, "")
            .icon(icon, theme::ICON_LARGE_PX)
            .width(TOOL_PX)
            .height(TOOL_PX)
            .tooltip(name)
            .pressed(pressed),
    )
}

/// Warnings: opens and closes the drawer. Its badge is the count of the
/// worst tier found, in that tier's colour, and its name says every
/// count, as the status bar's cell shows them.
fn warnings(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let cell = &state.cells.counts;
    let said = cell.none.clone().unwrap_or_else(|| {
        cell.tiers
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    });
    let name = strings.fmt(StrKey::RailWarnings, &[("counts", &said)]);
    let response = tool(ui, Icon::TriangleAlert, &name, state.warnings_open);
    if response.clicked() {
        out.push(Action::ToggleWarnings);
    }
    let counts = cell.counts;
    let (count, colours) = match cell.tiers.first() {
        Some((Level::Error, _)) => (counts.errors, palette.status_error),
        Some((Level::Warning, _)) => (counts.warnings, palette.status_warning),
        Some((Level::Hint | Level::Note, _)) => (counts.hints, palette.status_info),
        None => return,
    };
    let text = ui.painter().layout_no_wrap(
        count.to_string(),
        TextStyle::Name(theme::RAIL_LABEL.into()).resolve(ui.style()),
        palette.surface_panel,
    );
    let size = egui::vec2((text.size().x + 8.0).max(BADGE_PX), BADGE_PX);
    let badge = Rect::from_center_size(
        egui::pos2(response.rect.right() - 9.0, response.rect.top() + 10.0),
        size,
    );
    ui.painter().rect(
        badge,
        theme::pill(BADGE_PX),
        colours.stroke,
        Stroke::new(1.5, palette.surface_panel),
        StrokeKind::Outside,
    );
    ui.painter().galley(
        badge.center() - text.size() / 2.0,
        text,
        palette.surface_panel,
    );
}

/// Appearance: a menu of the three choices, the current one checked
/// (View ▸ Appearance's rows).
fn appearance(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let current = strings.get(menu::theme_label(state.prefs.theme));
    let name = strings.fmt(StrKey::RailAppearance, &[("theme", current)]);
    let response = tool(ui, Icon::SunMoon, &name, false);
    egui::Popup::menu(&response)
        .align(egui::RectAlign::RIGHT_END)
        .show(|ui| menu::appearance_items(ui, state, out));
}

/// Help: a menu of the Help menu's rows.
fn help(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let name = state.app.strings().get(StrKey::RailHelp);
    let response = tool(ui, Icon::CircleHelp, name, false);
    egui::Popup::menu(&response)
        .align(egui::RectAlign::RIGHT_END)
        .show(|ui| menu::help_items(ui, state, out));
}
