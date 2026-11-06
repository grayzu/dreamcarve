// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The side panels (functional spec, capability 50; `experience.md`,
//! sections 2 and 3): the left column, Design Tree over Properties, and
//! the CAM panel with its Operations and Tools tabs, each resizable by its
//! divider and collapsing to a 32-px strip, dragged below its minimum or
//! when the window is under 1280 × 768. Their contents are this slice's
//! read-only lists and empty sentences (capability 79); the tree, the
//! editors and the lists' rows arrive with slices 5, 8 and 10.

use dc_app::StrKey;
use eframe::egui::{self, Rangef, Sense, TextStyle, Ui, Vec2};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{
    Action, CamTab, FrameState, LEFT_MIN_PX, RIGHT_MIN_PX, STRIP_PX, VIEWPORT_MIN_PX,
};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;

/// A panel header's height, px.
const HEADER_PX: f32 = 32.0;
/// A list row's height, px.
const ROW_PX: f32 = 26.0;
/// The gutter between regions, px.
const GUTTER_PX: f32 = 4.0;

/// How a side panel came out this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    /// Its width when open; `None` when shown as a strip.
    pub width: Option<f32>,
    /// The user dragged it below its minimum, closing it.
    pub dragged_closed: bool,
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
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
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

/// Rows reading `names`, 26 px each, truncated to the panel.
fn rows<'a>(ui: &mut Ui, names: impl Iterator<Item = &'a str>) {
    let palette = Palette::current(ui);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for name in names {
                ui.allocate_ui(egui::vec2(ui.available_width(), ROW_PX), |ui| {
                    ui.set_min_height(ROW_PX);
                    ui.horizontal_centered(|ui| {
                        ui.add_space(12.0);
                        text(ui, name, TextStyle::Body, palette.text_primary);
                    });
                });
            }
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
        };
    }
    let other = if right_open { RIGHT_MIN_PX } else { STRIP_PX };
    let max = (window.x - other - VIEWPORT_MIN_PX - 2.0 * GUTTER_PX).max(LEFT_MIN_PX);
    let default = px(state.prefs.panels.left_width).clamp(LEFT_MIN_PX, max);
    let palette = Palette::current(ui);
    let mut expanded = true;
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
            egui::Panel::bottom("properties")
                .resizable(true)
                .show_separator_line(false)
                .default_size(properties_height)
                .size_range(Rangef::new(HEADER_PX + 40.0, f32::INFINITY))
                .frame(frame(palette))
                .show(ui, |ui| {
                    // Fill the panel, or it shrinks to its content.
                    ui.set_min_height(ui.available_height());
                    header(ui, strings.get(StrKey::AppPanelProperties));
                    empty_sentence(ui, strings.get(StrKey::AppPanelNoSelection));
                });
            header(ui, strings.get(StrKey::AppPanelDesignTree));
            let instances = &state.app.project().instances;
            if instances.is_empty() {
                empty_sentence(ui, strings.get(StrKey::AppPanelNoShapes));
            } else {
                rows(ui, instances.iter().map(|i| i.name.as_str()));
            }
        });
    Shown {
        width: response.map(|r| r.response.rect.width()),
        dragged_closed: !expanded,
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
            let project = state.app.project();
            tabs(ui, state, tab);
            match tab {
                CamTab::Operations if project.operations.is_empty() => {
                    empty_sentence(ui, strings.get(StrKey::AppPanelNoOperations));
                }
                CamTab::Operations => rows(ui, project.operations.iter().map(|op| op.name())),
                CamTab::Tools if project.tools.is_empty() => {
                    empty_sentence(ui, strings.get(StrKey::AppPanelNoTools));
                }
                CamTab::Tools => rows(ui, project.tools.iter().map(|t| t.name.as_str())),
            }
        });
    Shown {
        width: response.map(|r| r.response.rect.width()),
        dragged_closed: !expanded,
    }
}

/// The CAM panel's 32-px tab strip (section 7, "Tabs"): two equal
/// segments, the active one SemiBold with a 2-px accent underline.
fn tabs(ui: &mut Ui, state: &FrameState<'_>, tab: &mut CamTab) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let project = state.app.project();
    let labels = [
        (
            CamTab::Operations,
            strings.fmt(
                StrKey::AppPanelOperations,
                &[("count", &project.operations.len().to_string())],
            ),
        ),
        (
            CamTab::Tools,
            strings.fmt(
                StrKey::AppPanelTools,
                &[("count", &project.tools.len().to_string())],
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
