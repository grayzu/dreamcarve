// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The step panel and the Inspector (`docs/specs/phase-4/redesign.md`,
//! "The window" and "The steps"; functional spec, capability 50): the two
//! cards either side of the Viewport.
//!
//! The **step panel** (312 px, at least 280) shows the current step: its
//! title, one line saying what it holds ([`dc_app::steps::summary`]), then
//! its content. Design is *Add drawing* and *Add picture* over the Design
//! Tree ([`panels::tree`]); Stock is the units, the stock and the origin
//! ([`panels::project`]); Tools and Operations are their lists
//! ([`panels::tools`], [`panels::operations`]); Export is the whole
//! export, from its readiness card to its result
//! ([`panels::export_step`]), in a panel 440 px wide, since the step has
//! no Inspector and its programs' names are long. While an
//! operation's targets are edited it shows the Design Tree, the one part
//! of the window besides the Viewport that stays live (capability 67).
//!
//! The **Inspector** (328 px, at least 300) is there only while the step
//! has something selected ([`inspecting`]): on Design the Properties of
//! the selected shapes, tabs or pictures ([`panels::properties`]), the
//! shapes' with the Arrange group ([`panels::arrange`]) in its header (a
//! picture does not turn or flip), on Tools the tool's editor,
//! on Operations the operation's ([`panels::editor`]). Its close button
//! deselects, and the Viewport takes the room. In a window narrower than
//! 1280 px it lies over the Viewport's right side instead of beside it.
//!
//! Each panel resizes by its inner edge: the 8-px gap between two cards
//! is the handle, and shows a 2-px `accent` line while the pointer is on
//! it. The widths are remembered in `gui.toml`.

use dc_app::steps::Step;
use dc_app::{Selection, StrKey};
use dc_project::{BitmapId, InstanceId, Operation, TabId, Tool};
use eframe::egui::{self, Id, Rangef, Rect, Sense, Stroke, TextStyle, Ui, UiBuilder, Vec2};

use crate::panels;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::card::{card, floating};
use crate::widgets::icon::Icon;

/// A card's content margin, px.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;
/// The gap between regions, px.
const GAP_PX: f32 = theme::GAP_PX;
/// The gap, as a frame's margin.
const GAP: i8 = 8;
/// The margin under the cards, px: the status bar's pills stand 2 px in
/// from its top, which makes the gap.
const BOTTOM: i8 = crate::shell::status::BOTTOM_PX;
/// What a resizable panel and the Viewport each leave of the gap between
/// their cards, px: the 2 px between is where the resize line shows.
pub(crate) const HALF_GAP_PX: f32 = 3.0;
/// [`HALF_GAP_PX`] as a frame's margin.
const HALF_GAP: i8 = 3;
/// The resize line's room in the gap, px.
const LINE: i8 = 2;
/// An Inspector header's height, px: the operation editor's.
pub(crate) const HEADER_PX: f32 = 48.0;
/// The Inspector's close button, px square.
const CLOSE_PX: f32 = 28.0;
/// The room an Inspector header keeps clear at its right for the close
/// button, px.
pub(crate) const CLOSE_RESERVE_PX: f32 = 12.0 + CLOSE_PX + 4.0;
/// What the rail and the window's margins take of the window's width, px.
const RAIL_SPAN_PX: f32 = GAP_PX + theme::RAIL_PX;

/// A resizable panel as it came out this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    /// Its card.
    pub card: Rect,
    /// The card's width, px: the preference.
    pub width: f32,
    /// Whether it is the Export step's wider panel, whose width is its
    /// own and not the step panel's preference.
    pub export: bool,
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

/// Shows `panel` with the resize line egui draws on its inner edge as a
/// 2-px `accent` line; `body` draws in the app's own style.
fn resizable<R>(
    ui: &mut Ui,
    panel: egui::Panel,
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let palette = Palette::current(ui);
    let saved = ui.style().clone();
    let line = Stroke::new(2.0, palette.accent);
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.hovered.fg_stroke = line;
    widgets.active.fg_stroke = line;
    let style = saved.clone();
    let response = panel.show(ui, |ui| {
        ui.set_style(style);
        body(ui)
    });
    ui.set_style(saved);
    response
}

/// The widest the step panel may be in a window `window_px` wide, px:
/// what leaves the Viewport and the Inspector their minimum widths.
fn step_panel_max(window_px: f32) -> f32 {
    (window_px
        - RAIL_SPAN_PX
        - GAP_PX
        - GAP_PX
        - theme::VIEWPORT_MIN_PX
        - GAP_PX
        - theme::INSPECTOR_MIN_PX
        - GAP_PX)
        .max(theme::STEP_PANEL_MIN_PX)
}

/// The widest the Export step's panel may be in a window `window_px`
/// wide, px: the step has no Inspector, so only the Viewport's minimum
/// width is kept.
fn export_panel_max(window_px: f32) -> f32 {
    (window_px - RAIL_SPAN_PX - GAP_PX - GAP_PX - theme::VIEWPORT_MIN_PX - GAP_PX)
        .max(theme::STEP_PANEL_MIN_PX)
}

/// The step panel, in a window of `window`. The Export step's is wider
/// by default ([`theme::EXPORT_PANEL_PX`]), a panel of its own, so its
/// width and the other steps' do not follow each other.
pub(crate) fn step_panel(
    ui: &mut Ui,
    state: &FrameState<'_>,
    window: Vec2,
    out: &mut Vec<Action>,
) -> Shown {
    let palette = Palette::current(ui);
    let export = shown_step(state) == Step::Export;
    let (id, max, default) = if export {
        let max = export_panel_max(window.x);
        (
            "step_panel_export",
            max,
            theme::EXPORT_PANEL_PX.clamp(theme::STEP_PANEL_MIN_PX, max),
        )
    } else {
        let max = step_panel_max(window.x);
        (
            "step_panel",
            max,
            px(state.prefs.panels.left_width).clamp(theme::STEP_PANEL_MIN_PX, max),
        )
    };
    // What the frame adds to the card: the gap to the rail, and this
    // panel's share of the gap to the Viewport.
    let chrome = GAP_PX + HALF_GAP_PX + f32::from(LINE);
    let panel = egui::Panel::left(id)
        .resizable(true)
        .show_separator_line(false)
        .size_range(Rangef::new(theme::STEP_PANEL_MIN_PX + chrome, max + chrome))
        .default_size(default + chrome)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(egui::Margin {
                    left: GAP,
                    right: HALF_GAP,
                    top: 0,
                    bottom: BOTTOM,
                })
                .outer_margin(egui::Margin {
                    left: 0,
                    right: LINE,
                    top: 0,
                    bottom: 0,
                }),
        );
    let card_rect = resizable(ui, panel, |ui| {
        let rect = ui.available_rect_before_wrap();
        card(ui, |ui| step(ui, state, out));
        rect
    })
    .inner;
    Shown {
        card: card_rect,
        width: card_rect.width(),
        export,
    }
}

/// The step the panel shows: the current one, or Design while an
/// operation's targets are edited, for its tree.
fn shown_step(state: &FrameState<'_>) -> Step {
    if state.inert() {
        Step::Design
    } else {
        state.step
    }
}

/// The current step: its title and summary, then its content.
fn step(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let step = shown_step(state);
    title(ui, state, step);
    match step {
        Step::Design => design(ui, state, out),
        Step::Stock => panels::project::stock_step(ui, state, out),
        Step::Tools => panels::tools::list(ui, state, out),
        Step::Operations => panels::operations::list(ui, state, out),
        Step::Export => panels::export_step::show(ui, state, out),
    }
}

/// A step's title, 18-px SemiBold, over its one-line summary in
/// text.secondary.
fn title(ui: &mut Ui, state: &FrameState<'_>, step: Step) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let name = strings.get(step.label());
    let spacing = ui.spacing().item_spacing.y;
    ui.add_space(MARGIN_PX - 2.0);
    let style = TextStyle::Name(theme::DISPLAY.into());
    let height = ui.text_style_height(&style);
    let font = style.resolve(ui.style());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, name));
    ui.painter().text(
        egui::pos2(rect.left() + MARGIN_PX, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        palette.text_primary,
    );
    ui.add_space(2.0 - spacing);
    let summary = dc_app::steps::summary(state.app, &state.session, step);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(summary)
                        .text_style(TextStyle::Small)
                        .color(palette.text_secondary),
                )
                .wrap()
                .selectable(false),
            );
        });
    ui.add_space(12.0 - spacing);
}

/// One sentence, centred in the list area (section 8, "Empty states").
pub(crate) fn empty_sentence(ui: &mut Ui, sentence: &str) {
    let palette = Palette::current(ui);
    ui.add_space(24.0);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
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
        });
}

/// The Design step: *Add drawing* and *Add picture*, the two ways
/// geometry comes into the project (capabilities 55 and 56), over the
/// Design Tree.
fn design(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let spacing = ui.spacing().item_spacing.y;
    // Target editing leaves the tree live and nothing else (capability
    // 67).
    ui.add_enabled_ui(!state.inert(), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP_PX;
            ui.add_space(MARGIN_PX);
            let width = ((ui.available_width() - MARGIN_PX - GAP_PX) / 2.0).max(0.0);
            let add = |key: StrKey, icon: Icon| {
                ButtonSpec::new(Kind::Secondary, strings.get(key))
                    .icon(icon, theme::ICON_SMALL_PX)
                    .width(width)
            };
            if button(ui, add(StrKey::StepsDesignAddDrawing, Icon::FileInput)).clicked() {
                out.push(Action::ImportDrawing);
            }
            if button(ui, add(StrKey::StepsDesignAddPicture, Icon::ImagePlus)).clicked() {
                out.push(Action::ImportPicture);
            }
        });
    });
    ui.add_space(GAP_PX - spacing);
    let project = state.app.project();
    if project.instances.is_empty() && project.bitmaps.is_empty() {
        empty_sentence(ui, strings.get(StrKey::AppPanelNoShapes));
    } else {
        panels::tree::show(ui, state, out);
    }
}

/// What the Inspector edits.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Inspecting<'a> {
    /// The placement of the selected shapes.
    Shapes(&'a [InstanceId]),
    /// The size and shape of the selected placed tabs.
    Tabs(&'a [TabId]),
    /// The placement, the pixels and the lock of the selected pictures.
    Pictures(&'a [BitmapId]),
    /// A project tool.
    Tool(&'a Tool),
    /// An operation.
    Operation(&'a Operation),
}

/// What the current step has selected that has an editor: on Design the
/// selected shapes, tabs or pictures, on Tools the selected tool, on Operations
/// the operation in the editor. While an operation's targets are edited,
/// that operation.
pub(crate) fn inspecting<'a>(state: &FrameState<'a>) -> Option<Inspecting<'a>> {
    let app = state.app;
    let project = app.project();
    let operation = |id: &dc_project::OperationId| {
        project
            .operations
            .iter()
            .find(|o| o.id() == id)
            .map(Inspecting::Operation)
    };
    if let Some(editing) = app.target_editing() {
        return operation(editing.op());
    }
    match state.step {
        Step::Design => match app.selection() {
            Selection::Instances(ids) if !ids.is_empty() => Some(Inspecting::Shapes(ids)),
            Selection::Tabs(ids) if !ids.is_empty() => Some(Inspecting::Tabs(ids)),
            Selection::Bitmaps(ids) if !ids.is_empty() => Some(Inspecting::Pictures(ids)),
            _ => None,
        },
        Step::Tools => {
            let id = state.tool.as_ref()?;
            project
                .tools
                .iter()
                .find(|t| t.id == *id)
                .map(Inspecting::Tool)
        }
        Step::Operations => app.editing().and_then(operation),
        Step::Stock | Step::Export => None,
    }
}

/// The Inspector as a panel beside the Viewport, in a window of `window`
/// whose step panel is `step_px` wide (when it shows).
pub(crate) fn inspector(
    ui: &mut Ui,
    state: &FrameState<'_>,
    window: Vec2,
    step_px: Option<f32>,
    out: &mut Vec<Action>,
) -> Shown {
    let palette = Palette::current(ui);
    let step_span = step_px.map_or(0.0, |w| GAP_PX + w);
    let max =
        (window.x - RAIL_SPAN_PX - step_span - GAP_PX - theme::VIEWPORT_MIN_PX - GAP_PX - GAP_PX)
            .max(theme::INSPECTOR_MIN_PX);
    let default = px(state.prefs.panels.right_width).clamp(theme::INSPECTOR_MIN_PX, max);
    let chrome = GAP_PX + HALF_GAP_PX + f32::from(LINE);
    let panel = egui::Panel::right("inspector")
        .resizable(true)
        .show_separator_line(false)
        .size_range(Rangef::new(theme::INSPECTOR_MIN_PX + chrome, max + chrome))
        .default_size(default + chrome)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(egui::Margin {
                    left: HALF_GAP,
                    right: GAP,
                    top: 0,
                    bottom: BOTTOM,
                })
                .outer_margin(egui::Margin {
                    left: LINE,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }),
        );
    let card_rect = resizable(ui, panel, |ui| {
        let rect = ui.available_rect_before_wrap();
        card(ui, |ui| body(ui, state, out));
        rect
    })
    .inner;
    Shown {
        card: card_rect,
        width: card_rect.width(),
        export: false,
    }
}

/// The Inspector over the right side of the Viewport's card `viewport`,
/// for a window too narrow to hold it beside the Viewport; its
/// rectangle. It takes every click on it, so none reaches the drawing
/// under it.
pub(crate) fn inspector_over(
    ui: &Ui,
    state: &FrameState<'_>,
    viewport: Rect,
    out: &mut Vec<Action>,
) -> Rect {
    let width = px(state.prefs.panels.right_width)
        .max(theme::INSPECTOR_MIN_PX)
        .min(viewport.width());
    let rect = Rect::from_min_max(
        egui::pos2(viewport.right() - width, viewport.top()),
        viewport.max,
    );
    egui::Area::new(Id::unique("dc.inspector.over"))
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.interact(
                rect,
                Id::unique("dc.inspector.over.surface"),
                Sense::click_and_drag(),
            );
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
            floating(&mut child, |ui| body(ui, state, out));
            ui.allocate_rect(rect, Sense::hover());
        });
    rect
}

/// The Inspector's content for what the step has selected, and its close
/// button.
fn body(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let Some(what) = inspecting(state) else {
        return;
    };
    // Target editing leaves the Viewport and the Design Tree live and
    // nothing else (capability 67).
    if state.inert() {
        ui.disable();
    }
    let card = ui.max_rect();
    match what {
        Inspecting::Shapes(ids) => {
            let title = panels::properties::title(state, ids);
            header(ui, &title, panels::arrange::WIDTH_PX, |ui| {
                panels::arrange::group(ui, state, out);
            });
            scrolled(ui, "inspector_properties", |ui| {
                panels::properties::show(ui, state, ids, out);
            });
        }
        Inspecting::Tabs(ids) => {
            header(ui, &panels::properties::tabs_title(state, ids), 0.0, |_| {});
            scrolled(ui, "inspector_tabs", |ui| {
                panels::properties::tabs(ui, state, ids, out);
            });
        }
        Inspecting::Pictures(ids) => {
            let title = panels::properties::pictures_title(state, ids);
            header(ui, &title, 0.0, |_| {});
            scrolled(ui, "inspector_pictures", |ui| {
                panels::properties::pictures(ui, state, ids, out);
            });
        }
        Inspecting::Tool(tool) => panels::tools::editor_card(ui, state, tool, out),
        Inspecting::Operation(op) => panels::editor::show(ui, state, op, out),
    }
    let close = Rect::from_min_size(
        egui::pos2(
            card.right() - 12.0 - CLOSE_PX,
            card.top() + (HEADER_PX - CLOSE_PX) / 2.0 + 2.0,
        ),
        egui::vec2(CLOSE_PX, CLOSE_PX),
    );
    let mut over = ui.new_child(UiBuilder::new().max_rect(close));
    let spec = ButtonSpec::new(Kind::Quiet, "")
        .icon(Icon::X, theme::ICON_SMALL_PX)
        .width(CLOSE_PX)
        .height(CLOSE_PX)
        .tooltip(state.app.strings().get(StrKey::InspectorClose));
    if button(&mut over, spec).clicked() {
        out.push(Action::CloseInspector);
    }
}

/// `body` in a scroll area that fills what is left of the card.
fn scrolled(ui: &mut Ui, salt: &str, body: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink(false)
        .show(ui, body);
}

/// An Inspector header (the operation editor's has its own): `title` at
/// panel-title weight, cut with an ellipsis, and `trailing`'s controls,
/// `trailing_px` wide, before the room kept for the close button.
pub(crate) fn header(ui: &mut Ui, title: &str, trailing_px: f32, trailing: impl FnOnce(&mut Ui)) {
    let palette = Palette::current(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, title));
    // Centred a little low, in the room the card's top padding leaves.
    let rect = rect.translate(egui::vec2(0.0, 2.0));
    let right = rect.right() - CLOSE_RESERVE_PX;
    let mut controls = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                egui::pos2(right - trailing_px, rect.top()),
                egui::pos2(right, rect.bottom()),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    trailing(&mut controls);
    let title_right = if trailing_px > 0.0 {
        right - trailing_px - 8.0
    } else {
        right
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (title_right - rect.left() - MARGIN_PX).max(0.0),
    );
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(
            rect.left() + MARGIN_PX,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_primary,
    );
}
