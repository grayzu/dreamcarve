// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The side panels (functional spec, capability 50; `experience.md`,
//! sections 2 and 3): the left column, and the CAM panel with its Tools
//! and Operations tabs, each resizable by its divider and collapsing to a
//! 32-px strip, dragged below its minimum or when the window is under
//! 1280 × 768. The left column holds the Design Tree ([`panels::tree`])
//! over a scrolling area with two collapsible panels (#615): Project
//! Settings ([`panels::project`]), always there, on top, and Properties
//! ([`panels::properties`]), there while shapes or tabs are selected and
//! scrolled into view when a selection appears. The CAM panel has the
//! title *CAM* over its tab strip; the Operations tab holds the list and
//! the operation editor ([`panels::operations`]), the Tools tab the
//! project's tools ([`panels::tools`]).

use dc_app::{Selection, StrKey};
use eframe::egui::{self, Align, Id, Rangef, Sense, TextStyle, Ui, Vec2};

use crate::panels;
use crate::shell::focus::{self, Region};
use crate::shell::theme::{self, Palette};
use crate::shell::window::{
    Action, CamTab, FrameState, LEFT_MIN_PX, RIGHT_MIN_PX, STRIP_PX, VIEWPORT_MIN_PX,
};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;

/// A panel header's height, px.
const HEADER_PX: f32 = 32.0;
/// Project Settings' open state in `gui.toml`'s `sections`.
pub(crate) const SETTINGS_SECTION: &str = "panel.project_settings";
/// Properties' open state in `gui.toml`'s `sections`.
pub(crate) const PROPERTIES_SECTION: &str = "panel.properties";

/// Where Project ▸ Project Settings… asks the left column to scroll the
/// panel into view on its next frame.
fn reveal_id() -> Id {
    Id::unique("dc.side.reveal_settings")
}

/// Scrolls Project Settings into view on the next frame (Project ▸
/// Project Settings…).
pub(crate) fn reveal_settings(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(reveal_id(), true));
}

/// The selection the left column last drew, to tell when one appears.
fn shown_selection_id() -> Id {
    Id::unique("dc.side.shown_selection")
}
/// The gutter between regions, px.
const GUTTER_PX: f32 = 4.0;

/// How a side panel came out this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    /// Its width when open; `None` when shown as a strip.
    pub width: Option<f32>,
    /// The user dragged it below its minimum, closing it.
    pub dragged_closed: bool,
    /// The left column's Properties panel, where it was drawn: the
    /// Design Tree is the column above it (#548, for Tab's walk).
    pub properties: Option<egui::Rect>,
}

/// `value` px as egui's `f32`. Panel widths are a few thousand pixels at
/// most, far inside `f32`'s exact range.
#[allow(
    clippy::cast_possible_truncation,
    reason = "pixel widths, exact in f32"
)]
fn px(value: f64) -> f32 {
    value as f32
}

/// The panel's frame: `surface.panel`, no border of its own (section 2,
/// "Panel chrome").
fn frame(palette: &Palette) -> egui::Frame {
    egui::Frame::NONE.fill(palette.surface_panel)
}

/// A 32-px header band with `title` and the one rule under it.
fn header(ui: &mut Ui, title: &str) {
    let palette = Palette::current(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, title));
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_default),
    );
}

/// A collapsible panel (`experience.md`, section 2, "Panel chrome"): its
/// 32-px title band with the chevron at its left, at panel-title weight
/// on `surface.panel` with the one rule under it, then, while `open`, its
/// body. Whether it is open is a preference passed in; the band's
/// response, clicked to flip it and scrolled to.
fn collapsible(
    ui: &mut Ui,
    name: &str,
    title: &str,
    open: bool,
    body: impl FnOnce(&mut Ui),
) -> egui::Response {
    let palette = Palette::current(ui);
    let id = Id::unique(name);
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    state.set_open(open);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, title));
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    }
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let glyph = chevron.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    let glyph_at = egui::pos2(rect.left() + 8.0, rect.center().y - glyph.size().y / 2.0);
    ui.painter().galley(glyph_at, glyph, palette.text_secondary);
    ui.painter().text(
        egui::pos2(
            rect.left() + 8.0 + theme::ICON_SMALL_PX + 6.0,
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_default),
    );
    crate::widgets::button::focus_ring(ui, &response);
    state.show_body_unindented(ui, body);
    response
}

/// The left column's lower half (#615): Project Settings over
/// Properties in one scrolling area.
fn lower_half(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let sections = &state.prefs.sections;
    let open = |name: &str| sections.get(name).copied().unwrap_or(true);
    let selection = state.app.selection();
    let appeared = ui.ctx().data_mut(|d| {
        let last = d.get_temp::<Selection>(shown_selection_id());
        d.insert_temp(shown_selection_id(), selection.clone());
        last.as_ref() != Some(selection)
    });
    let reveal = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<bool>(reveal_id()))
        .unwrap_or(false);
    // The bands stack with no gap; each body keeps the panel's spacing.
    let spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .id_salt("left_lower")
        .auto_shrink(false)
        .show(ui, |ui| {
            let settings_open = state.settings_forced || open(SETTINGS_SECTION);
            let band = collapsible(
                ui,
                SETTINGS_SECTION,
                strings.get(StrKey::AppPanelProjectSettings),
                settings_open,
                |ui| {
                    ui.spacing_mut().item_spacing = spacing;
                    panels::project::show(ui, state, out);
                },
            );
            if band.clicked() {
                out.push(Action::SetSection {
                    name: SETTINGS_SECTION.to_owned(),
                    open: !settings_open,
                });
            }
            if reveal {
                band.scroll_to_me(Some(Align::TOP));
            }
            let title = match selection {
                Selection::Instances(ids) => panels::properties::title(state, ids),
                Selection::Tabs(ids) => panels::properties::tabs_title(state, ids),
                Selection::Nothing | Selection::Operation(_) => return,
            };
            let properties_open = open(PROPERTIES_SECTION);
            let band = collapsible(ui, PROPERTIES_SECTION, &title, properties_open, |ui| {
                ui.spacing_mut().item_spacing = spacing;
                match selection {
                    // Instances: their placement (#534).
                    Selection::Instances(ids) => panels::properties::show(ui, state, ids, out),
                    // Placed tabs: their size and shape (#546).
                    Selection::Tabs(ids) => panels::properties::tabs(ui, state, ids, out),
                    Selection::Nothing | Selection::Operation(_) => {}
                }
            });
            if band.clicked() {
                out.push(Action::SetSection {
                    name: PROPERTIES_SECTION.to_owned(),
                    open: !properties_open,
                });
            }
            // A selection that appears brings Properties to the top, so it
            // is never below the fold under an open Project Settings.
            if appeared && !reveal {
                band.scroll_to_me(Some(Align::TOP));
            }
        });
}

/// One sentence, centred in the list area (section 8, "Empty states").
fn empty_sentence(ui: &mut Ui, sentence: &str) {
    let palette = Palette::current(ui);
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(sentence)
                    .text_style(TextStyle::Body)
                    .color(palette.text_secondary),
            )
            .wrap()
            .selectable(false),
        );
    });
}

/// A collapsed panel: a 32-px strip with the reopen button and the
/// panel's title set vertically (section 2, "Dividers").
fn strip(ui: &mut Ui, left: bool, label: &str, title: &str, action: Action, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let panel = if left {
        egui::Panel::left("left_strip")
    } else {
        egui::Panel::right("right_strip")
    };
    panel
        .exact_size(STRIP_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(frame(palette))
        .show(ui, |ui| {
            ui.add_space(3.0);
            let icon = if left {
                Icon::PanelLeftOpen
            } else {
                Icon::PanelRightOpen
            };
            ui.vertical_centered(|ui| {
                let spec = ButtonSpec::new(Kind::Quiet, "")
                    .icon(icon, theme::ICON_SMALL_PX)
                    .tooltip(label);
                if button(ui, spec).clicked() {
                    out.push(action);
                }
            });
            let galley = ui.painter().layout_no_wrap(
                title.to_owned(),
                TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
                palette.text_secondary,
            );
            let top = ui.cursor().top() + 8.0;
            let x = ui.max_rect().center().x - galley.size().y / 2.0;
            let anchor = egui::pos2(x, top + galley.size().x);
            ui.painter().add(
                egui::epaint::TextShape::new(anchor, galley, palette.text_secondary)
                    .with_angle(-std::f32::consts::FRAC_PI_2),
            );
        });
}

/// The left column; `open` as laid out for a window of `window` with the
/// CAM panel `right_open`.
pub(crate) fn left(
    ui: &mut Ui,
    state: &FrameState<'_>,
    open: bool,
    window: Vec2,
    right_open: bool,
    out: &mut Vec<Action>,
) -> Shown {
    let strings = state.app.strings();
    if !open {
        focus::mark(ui, Region::Tree);
        strip(
            ui,
            true,
            strings.get(StrKey::AppPanelShowLeft),
            strings.get(StrKey::AppPanelDesignTree),
            Action::ShowLeft,
            out,
        );
        return Shown {
            width: None,
            dragged_closed: false,
            properties: None,
        };
    }
    let other = if right_open { RIGHT_MIN_PX } else { STRIP_PX };
    let max = (window.x - other - VIEWPORT_MIN_PX - 2.0 * GUTTER_PX).max(LEFT_MIN_PX);
    let default = px(state.prefs.panels.left_width).clamp(LEFT_MIN_PX, max);
    let palette = Palette::current(ui);
    let mut expanded = true;
    let mut properties = None;
    let response = egui::Panel::left("left_panel")
        .resizable(true)
        .drag_to_open(false)
        .show_separator_line(false)
        .size_range(Rangef::new(LEFT_MIN_PX, max))
        .default_size(default)
        .frame(frame(palette))
        .show_collapsible(ui, &mut expanded, |ui| {
            // From the window, not `available_height`, which a panel's
            // first frame does not know yet: the menu bar, toolbar and
            // status bar take 92 px.
            let properties_height = ((window.y - 92.0) * 0.45).max(120.0);
            let shown = egui::Panel::bottom("properties")
                .resizable(true)
                .show_separator_line(false)
                .default_size(properties_height)
                .size_range(Rangef::new(HEADER_PX + 40.0, f32::INFINITY))
                .frame(frame(palette))
                .show(ui, |ui| {
                    focus::mark(ui, Region::Properties);
                    // Target editing leaves the Design Tree live and
                    // nothing else (capability 67).
                    if state.inert() {
                        ui.disable();
                    }
                    // Fill the panel, or it shrinks to its content.
                    ui.set_min_height(ui.available_height());
                    lower_half(ui, state, out);
                });
            properties = Some(shown.response.rect);
            // Properties is drawn first, to take the column's bottom; Tab
            // still walks the Design Tree first (`shell::focus`).
            focus::mark(ui, Region::Tree);
            header(ui, strings.get(StrKey::AppPanelDesignTree));
            let instances = &state.app.project().instances;
            if instances.is_empty() {
                empty_sentence(ui, strings.get(StrKey::AppPanelNoShapes));
            } else {
                panels::tree::show(ui, state, out);
            }
        });
    Shown {
        width: response.map(|r| r.response.rect.width()),
        dragged_closed: !expanded,
        properties,
    }
}

/// The CAM panel; `open` as laid out for a window of `window`.
pub(crate) fn right(
    ui: &mut Ui,
    state: &FrameState<'_>,
    open: bool,
    window: Vec2,
    tab: &mut CamTab,
    out: &mut Vec<Action>,
) -> Shown {
    let strings = state.app.strings();
    if !open {
        strip(
            ui,
            false,
            strings.get(StrKey::AppPanelShowRight),
            strings.get(StrKey::AppMenuRightPanel),
            Action::ShowRight,
            out,
        );
        return Shown {
            width: None,
            dragged_closed: false,
            properties: None,
        };
    }
    let max = (window.x - LEFT_MIN_PX - VIEWPORT_MIN_PX - 2.0 * GUTTER_PX).max(RIGHT_MIN_PX);
    let default = px(state.prefs.panels.right_width).clamp(RIGHT_MIN_PX, max);
    let palette = Palette::current(ui);
    let mut expanded = true;
    let response = egui::Panel::right("right_panel")
        .resizable(true)
        .drag_to_open(false)
        .show_separator_line(false)
        .size_range(Rangef::new(RIGHT_MIN_PX, max))
        .default_size(default)
        .frame(frame(palette))
        .show_collapsible(ui, &mut expanded, |ui| {
            if state.inert() {
                ui.disable();
            }
            header(ui, strings.get(StrKey::AppPanelCam));
            tabs(ui, state, tab);
            match tab {
                CamTab::Operations => panels::operations::show(ui, state, out),
                CamTab::Tools => crate::panels::tools::show(ui, state, out),
            }
        });
    Shown {
        width: response.map(|r| r.response.rect.width()),
        dragged_closed: !expanded,
        properties: None,
    }
}

/// The CAM panel's 32-px tab strip (section 7, "Tabs"): two equal
/// segments in workflow order, Tools then Operations, the active one
/// SemiBold with a 2-px accent underline.
fn tabs(ui: &mut Ui, state: &FrameState<'_>, tab: &mut CamTab) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let project = state.app.project();
    let labels = [
        (
            CamTab::Tools,
            strings.fmt(
                StrKey::AppPanelTools,
                &[("count", &project.tools.len().to_string())],
            ),
        ),
        (
            CamTab::Operations,
            strings.fmt(
                StrKey::AppPanelOperations,
                &[("count", &project.operations.len().to_string())],
            ),
        ),
    ];
    let (strip, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    let width = strip.width() / 2.0;
    for (n, (which, label)) in labels.into_iter().enumerate() {
        let offset = if n == 0 { 0.0 } else { width };
        let rect = egui::Rect::from_min_size(
            strip.min + egui::vec2(offset, 0.0),
            egui::vec2(width, HEADER_PX),
        );
        let response = ui.interact(rect, ui.make_persistent_id(("cam_tab", n)), Sense::click());
        let active = *tab == which;
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, active, &label));
        if response.clicked() {
            *tab = which;
        }
        if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
        }
        let (style, colour) = if active {
            (TextStyle::Name(theme::TITLE.into()), palette.text_primary)
        } else {
            (TextStyle::Body, palette.text_secondary)
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &label,
            style.resolve(ui.style()),
            colour,
        );
        if active {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.bottom() - 2.0),
                    rect.right_bottom(),
                ),
                0.0,
                palette.accent,
            );
        }
    }
    ui.painter().hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_default),
    );
}
