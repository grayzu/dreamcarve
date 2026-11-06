// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The clearing-pass list (`docs/specs/phase-4/functional.md`,
//! capability 70; mock-up `06-operation-vcarve.svg`; `experience.md`,
//! section 7, "List row"; slice 13, #542): the block a
//! [`FieldKind::ClearingList`](dc_app::editor::FieldKind::ClearingList)
//! row draws under itself, for whatever list its row names (a V-Carve's
//! `clearing`, an Inlay's). Each pass is a card: its drag grip, `Pass n`
//! and a remove button, then the pass's rows ([`clearing::rows`]: tool,
//! clearance, its raster angle, direction, entry and the entry's own
//! settings) drawn by the editor's own renderers at the pass's key path,
//! so every control's egui id is the operation's id with that path
//! (`clearing[1].tool`), never its place in the layout. Dragging a grip
//! shows a 2-px accent insertion line where the pass will land and a
//! lifted copy at the cursor, and the release is one *Reorder* step.
//! *Add pass* appends the next smaller end mill's pass. What validation
//! finds in the list (`clearing_not_descending`) is drawn under it by the
//! row ([`crate::panels::editor`]).

use dc_app::StrKey;
use dc_app::editor::{Row, clearing};
use dc_job::Tier;
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use crate::panels::editor::{Cx, commit, control, control_id, tool_note};
use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button};
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::unit_field::HEIGHT_PX;

/// The gap between two cards, px (section 3's 4-pt scale).
const CARD_GAP_PX: f32 = 8.0;
/// The lifted copy's height, px.
const LIFTED_PX: f32 = 32.0;

/// Where the pass being dragged is kept while its grip is held: the
/// operation's and the list's, so two lists never share it.
fn dragging_id(cx: &Cx<'_>, list: &Row) -> Id {
    Id::unique(("dc.editor.clearing.dragging", cx.op.id().as_str(), list.key))
}

/// The list at `list`'s key path of the edited operation, its cards, the
/// drag that reorders them and *Add pass*.
pub(super) fn list(ui: &mut Ui, cx: &Cx<'_>, list: &'static Row, out: &mut Vec<Action>) {
    let passes = clearing::passes(cx.op, list);
    let drag_id = dragging_id(cx, list);
    let refused_id = control_id(cx.op, list.key).with("refused");
    let mut rects: Vec<Rect> = Vec::with_capacity(passes.len());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = CARD_GAP_PX;
        for index in 0..passes.len() {
            rects.push(card(ui, cx, list, index, drag_id, out));
        }
    });
    if passes.is_empty() {
        row::hint(ui, cx.strings.get(StrKey::EditorClearingEmpty));
    }
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    match dragging {
        Some(from) if from < passes.len() => drag(ui, cx, list, from, &rects, out),
        Some(_) => ui.data_mut(|d| d.remove::<usize>(drag_id)),
        None => {}
    }
    ui.horizontal(|ui| {
        let add = ButtonSpec::new(
            ButtonKind::Secondary,
            cx.strings.get(StrKey::EditorClearingAdd),
        )
        .icon(Icon::Plus, theme::ICON_SMALL_PX);
        if button(ui, add).clicked() {
            commit(ui, refused_id, clearing::add(cx.project, cx.op, list), out);
        }
        let palette = Palette::current(ui);
        ui.add(
            egui::Label::new(
                egui::RichText::new(cx.strings.get(StrKey::EditorClearingAddHint))
                    .text_style(TextStyle::Small)
                    .color(palette.text_secondary),
            )
            .selectable(false),
        );
    });
}

/// Pass `index`'s card: its header (grip, `Pass n`, remove), then its
/// rows; its rectangle, for the drag's slots.
fn card(
    ui: &mut Ui,
    cx: &Cx<'_>,
    list: &'static Row,
    index: usize,
    drag_id: Id,
    out: &mut Vec<Action>,
) -> Rect {
    let palette = Palette::current(ui);
    let n = (index + 1).to_string();
    let tools = clearing::tools(cx.project);
    egui::Frame::NONE
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                let (grip_rect, _) = ui.allocate_exact_size(
                    egui::vec2(theme::ICON_SMALL_PX, HEIGHT_PX),
                    Sense::hover(),
                );
                // The grip's id is the operation's, the list's and the
                // pass's: never its place in the layout.
                let grip = ui.interact(
                    grip_rect.expand(4.0),
                    Id::unique((
                        "dc.editor.clearing.grip",
                        cx.op.id().as_str(),
                        list.key,
                        index,
                    )),
                    Sense::drag(),
                );
                let grip_name = cx.strings.fmt(StrKey::EditorClearingGrip, &[("n", &n)]);
                grip.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, &grip_name)
                });
                if grip.drag_started() {
                    ui.data_mut(|d| d.insert_temp(drag_id, index));
                }
                if grip.hovered() || grip.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                let colour = if grip.hovered() || grip.dragged() {
                    palette.text_secondary
                } else {
                    palette.text_disabled
                };
                let glyph = Icon::GripVertical.galley(ui, theme::ICON_SMALL_PX, colour);
                ui.painter()
                    .galley(grip_rect.center() - glyph.size() / 2.0, glyph, colour);
                let title = cx.strings.fmt(StrKey::EditorClearingPass, &[("n", &n)]);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(title)
                            .text_style(TextStyle::Body)
                            .color(palette.text_primary),
                    )
                    .selectable(false),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let remove = cx.strings.fmt(StrKey::EditorClearingRemove, &[("n", &n)]);
                    let spec = ButtonSpec::new(ButtonKind::Quiet, "")
                        .icon(Icon::X, theme::ICON_SMALL_PX)
                        .tooltip(&remove)
                        .height(22.0);
                    if button(ui, spec).clicked() {
                        commit(
                            ui,
                            control_id(cx.op, list.key).with("refused"),
                            clearing::remove(cx.op, list, index),
                            out,
                        );
                    }
                });
            });
            for slot in clearing::rows(cx.op, list, index) {
                let refused_id = control_id(cx.op, slot.key()).with("refused");
                let refused: Option<String> = ui.data(|d| d.get_temp(refused_id));
                let tier = refused.as_ref().map(|_| Tier::Error);
                ui.horizontal(|ui| control(ui, cx, &slot, &tools, tier, false, out));
                if matches!(slot.row.kind, dc_app::editor::FieldKind::ToolPicker) {
                    tool_note(ui, cx, &slot);
                }
                if let Some(reason) = refused {
                    row::message(ui, Tier::Error, &reason);
                }
            }
        })
        .response
        .rect
}

/// The drag of pass `from`: the insertion line where it would land, a
/// lifted copy at the cursor, and on release one reorder.
fn drag(
    ui: &mut Ui,
    cx: &Cx<'_>,
    list: &'static Row,
    from: usize,
    rects: &[Rect],
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let slot = pointer.map_or(from, |p| {
        rects
            .iter()
            .position(|r| p.y < r.center().y)
            .unwrap_or(rects.len())
    });
    // Halfway into the gap above the card it lands before, or under the
    // last.
    let y = rects.get(slot).map_or_else(
        || rects.last().map(|r| r.bottom() + CARD_GAP_PX / 2.0),
        |r| Some(r.top() - CARD_GAP_PX / 2.0),
    );
    if let Some(y) = y {
        let left = ui.max_rect().left();
        let right = ui.max_rect().right();
        ui.painter()
            .hline(left..=right, y, Stroke::new(2.0, palette.accent));
        ui.painter()
            .circle_filled(egui::pos2(left + 3.0, y), 3.0, palette.accent);
    }
    if let Some(at) = pointer {
        lifted(ui, cx, at, from);
    }
    if ui.input(|i| i.pointer.any_released()) {
        ui.data_mut(|d| d.remove::<usize>(dragging_id(cx, list)));
        let to = if slot > from { slot - 1 } else { slot };
        commit(
            ui,
            control_id(cx.op, list.key).with("refused"),
            clearing::reorder(cx.op, list, from, to),
            out,
        );
    }
}

/// Pass `index`, lifted and following the cursor at 96 % alpha.
fn lifted(ui: &Ui, cx: &Cx<'_>, at: egui::Pos2, index: usize) {
    let palette = Palette::current(ui);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        Id::unique("dc.editor.clearing.lifted"),
    ));
    let rect = Rect::from_min_size(
        egui::pos2(ui.max_rect().left(), at.y - LIFTED_PX / 2.0),
        egui::vec2(ui.max_rect().width(), LIFTED_PX),
    );
    painter.rect_filled(rect.translate(egui::vec2(0.0, 2.0)), 4.0, palette.shadow);
    painter.rect_filled(rect, 4.0, palette.surface_raised.gamma_multiply(0.96));
    let title = cx.strings.fmt(
        StrKey::EditorClearingPass,
        &[("n", &(index + 1).to_string())],
    );
    painter.text(
        egui::pos2(
            rect.left() + 8.0 + theme::ICON_SMALL_PX + 8.0,
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
    );
}
