// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Operations step (`docs/specs/phase-4/functional.md`, capability
//! 65; `experience.md`, sections 7 and 8; `redesign.md`, "The steps"):
//! its buttons (*Add ▾*, *Duplicate*, *Delete*) and the list in cut
//! order, in the step panel. The editor of the operation the list has
//! selected ([`crate::panels::editor`]) is the Inspector's. Each row is
//! 52 px on its own rounded surface: the drag grip, the
//! enable checkbox, the kind's glyph in the operation's colour, the name
//! over its kind and tool, the eye, and the status cell (the spinner
//! while it generates, its warning count, the red mark of an error).
//! Dragging a row's grip moves it, with an insertion line where it will
//! land, as one *Reorder operations* step on release. What each row says
//! and what each control does is `dc_app::operations`'s; the eye is view
//! state (`View::hidden_ops`), which the Viewport reads.

use dc_app::operations::{self, Kind, ListRow, Status};
use dc_app::{Command, StrKey, Strings};
use dc_project::{BitmapId, InstanceId, OperationId, Project};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use crate::panels;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button, focus_ring_round};
use crate::widgets::checkbox::paint_box;
use crate::widgets::icon::Icon;
use crate::widgets::op_glyph::op_glyph;
use crate::widgets::row::{INSET_PX, surface, surface_rect};
use crate::widgets::spinner::spinner;

/// A row's height, px (`experience.md`, section 3).
const ROW_PX: f32 = theme::LIST_ROW_PX;
/// The button row's height, px: a control and the gap under it.
const TOOLBAR_PX: f32 = theme::CONTROL_PX + theme::GAP_PX;
/// A glyph's box, px.
const ICON_PX: f32 = 16.0;
/// The status cell's width, px.
const STATUS_PX: f32 = 24.0;
/// The panel's content margin, px.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;
/// The Add menu's width, px: its longest reason on one line.
const MENU_PX: f32 = 260.0;

/// Where the row under the pointer leaves its operation for the shell.
fn hovered_id() -> Id {
    Id::unique("dc.operations.hovered")
}

/// The operation whose row the pointer was over in the frame just drawn:
/// its targets are marked as a preview until the pointer leaves
/// (capability 67). Taken, so a list no longer drawn previews nothing.
pub(crate) fn take_hovered(ctx: &egui::Context) -> Option<OperationId> {
    ctx.data_mut(|d| {
        let hovered = d.get_temp::<OperationId>(hovered_id());
        d.remove::<OperationId>(hovered_id());
        hovered
    })
}

/// The row being dragged, while its grip is held.
fn dragging_id() -> Id {
    Id::unique("dc.operations.dragging")
}

/// The step panel's content under its title: the buttons, then the list
/// in cut order, or the sentence that says how to add the first one.
pub(crate) fn list(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    toolbar(ui, state, out);
    let app = state.app;
    if app.project().operations.is_empty() {
        panels::side::empty_sentence(ui, app.strings().get(StrKey::AppPanelNoOperations));
        return;
    }
    rows(ui, state, out);
}

/// *Add ▾*, *Duplicate*, *Delete*. Only *Add* has an icon: the three fit
/// the step panel at its narrowest.
fn toolbar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let editing = state.app.editing().cloned();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), TOOLBAR_PX), Sense::hover());
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(MARGIN_PX, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = 8.0;
    let add = button(
        &mut bar,
        ButtonSpec::new(ButtonKind::Secondary, strings.get(StrKey::OperationsAdd))
            .icon(Icon::Plus, theme::ICON_SMALL_PX)
            .tooltip(strings.get(StrKey::OperationsAddHint)),
    );
    egui::Popup::menu(&add).show(|ui| {
        add_menu(
            ui,
            strings,
            state.app.project(),
            (
                state.app.selection().instances(),
                state.app.selected_pictures(),
            ),
            out,
        );
    });
    let duplicate = button(
        &mut bar,
        ButtonSpec::new(
            ButtonKind::Secondary,
            strings.get(StrKey::OperationsDuplicate),
        )
        .enabled(editing.is_some()),
    );
    if duplicate.clicked()
        && let Some(id) = editing.clone()
    {
        out.push(Action::run(Command::DuplicateOperation(id)));
    }
    let delete = button(
        &mut bar,
        ButtonSpec::new(
            ButtonKind::Destructive,
            strings.get(StrKey::OperationsDelete),
        )
        .enabled(editing.is_some()),
    );
    if delete.clicked()
        && let Some(id) = editing
    {
        out.push(Action::run(Command::DeleteOperation(id)));
    }
}

/// The Add menu's items for the shapes `selected` and the pictures
/// `pictures` selected (capability 65): the kinds they can take, a
/// divider, then the rest each with its reason, or with no shape selected
/// the sentence that says to select shapes. A Photo V-Carve carves the
/// selected picture, or the project's only one. Also the Design Tree's
/// *Add operation ▸*.
pub(crate) fn add_menu(
    ui: &mut Ui,
    strings: &Strings,
    project: &Project,
    (selected, pictures): (&[InstanceId], &[BitmapId]),
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let menu = operations::add_menu(project, strings, selected, pictures);
    ui.set_min_width(MENU_PX);
    ui.set_max_width(MENU_PX);
    if let Some(prompt) = &menu.prompt {
        ui.add(
            egui::Label::new(
                egui::RichText::new(prompt)
                    .text_style(TextStyle::Body)
                    .color(palette.text_secondary),
            )
            .selectable(false),
        );
    }
    for kind in &menu.available {
        if kind_item(ui, strings, *kind, true, None).clicked() {
            out.push(Action::AddOperation {
                kind: *kind,
                targets: selected.to_vec(),
                pictures: pictures.to_vec(),
            });
            ui.close();
        }
    }
    if !menu.available.is_empty() && !menu.unavailable.is_empty() {
        ui.separator();
    }
    for (kind, reason) in &menu.unavailable {
        kind_item(ui, strings, *kind, false, reason.as_deref());
    }
}

/// One kind in the Add menu: its glyph and name, and under it, when it is
/// unavailable, why.
fn kind_item(
    ui: &mut Ui,
    strings: &Strings,
    kind: Kind,
    enabled: bool,
    reason: Option<&str>,
) -> egui::Response {
    let palette = Palette::current(ui);
    let name = strings.get(kind.label());
    let height = if reason.is_some() { 44.0 } else { 32.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(MENU_PX, height),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuItem, enabled, name));
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_hover);
    }
    let colour = if enabled {
        palette.text_primary
    } else {
        palette.text_disabled
    };
    let glyph_rect = Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.top() + 8.0),
        egui::vec2(ICON_PX, ICON_PX),
    );
    op_glyph(ui.painter(), glyph_rect, kind, colour);
    ui.painter().text(
        egui::pos2(glyph_rect.right() + 8.0, glyph_rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        TextStyle::Body.resolve(ui.style()),
        colour,
    );
    if let Some(reason) = reason {
        ui.painter().text(
            egui::pos2(glyph_rect.right() + 8.0, rect.bottom() - 11.0),
            egui::Align2::LEFT_CENTER,
            reason,
            TextStyle::Small.resolve(ui.style()),
            palette.text_secondary,
        );
    }
    response
}

/// Where the list keeps the row it is to scroll into view.
fn reveal_id() -> Id {
    Id::unique("dc.operations.reveal")
}

/// Scrolls the list to operation `op`'s row at its next frame (a click
/// on the operation's cut, #576).
pub(crate) fn reveal(ctx: &egui::Context, op: OperationId) {
    ctx.data_mut(|d| d.insert_temp(reveal_id(), op));
}

/// The rows, with the drag that reorders them.
fn rows(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let rows = operations::rows(state.app);
    let dragging: Option<OperationId> = ui.data(|d| d.get_temp(dragging_id()));
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("operations_list")
            .auto_shrink(false),
        crate::widgets::scroll::Surface::Panel,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut rects: Vec<Rect> = Vec::with_capacity(rows.len());
            for row in &rows {
                rects.push(list_row(ui, state, row, out));
            }
            let Some(id) = dragging else {
                return;
            };
            let Some(from) = rows.iter().position(|r| r.id == id) else {
                ui.data_mut(|d| d.remove::<OperationId>(dragging_id()));
                return;
            };
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let slot = pointer.map_or(from, |p| {
                rects
                    .iter()
                    .position(|r| p.y < r.center().y)
                    .unwrap_or(rects.len())
            });
            // The insertion line between rows, with its dot at the left.
            if let Some(y) = rects
                .get(slot)
                .map(|r| r.top())
                .or_else(|| rects.last().map(|r| r.bottom()))
            {
                let left = ui.max_rect().left() + INSET_PX;
                let right = ui.max_rect().right() - INSET_PX;
                ui.painter()
                    .hline(left..=right, y, Stroke::new(2.0, palette.accent));
                ui.painter()
                    .circle_filled(egui::pos2(left + 3.0, y), 3.0, palette.accent);
            }
            // The lifted row follows the cursor.
            if let (Some(p), Some(row)) = (pointer, rows.get(from)) {
                lifted(ui, p, row);
            }
            if ui.input(|i| i.pointer.any_released()) {
                ui.data_mut(|d| d.remove::<OperationId>(dragging_id()));
                let to = if slot > from { slot - 1 } else { slot };
                out.extend(operations::reorder(state.app.project(), &id, to).map(Action::run));
            }
        },
    );
}

/// The row being dragged, raised and following the cursor at 96 % alpha.
fn lifted(ui: &Ui, at: egui::Pos2, row: &ListRow) {
    let palette = Palette::current(ui);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        Id::unique("dc.operations.lifted"),
    ));
    let rect = Rect::from_min_size(
        egui::pos2(ui.max_rect().left() + INSET_PX, at.y - ROW_PX / 2.0),
        egui::vec2(ui.max_rect().width() - 2.0 * INSET_PX, ROW_PX),
    );
    painter.add(theme::shadow(palette).as_shape(rect, theme::CONTROL_RADIUS_PX));
    painter.rect_filled(
        rect,
        theme::CONTROL_RADIUS_PX,
        palette.surface_raised.gamma_multiply(0.96),
    );
    let glyph = Rect::from_min_size(
        egui::pos2(rect.left() + 54.0, rect.center().y - ICON_PX / 2.0),
        egui::vec2(ICON_PX, ICON_PX),
    );
    op_glyph(&painter, glyph, row.kind, palette.operation(row.colour));
    painter.text(
        egui::pos2(glyph.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &row.name,
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
    );
}

/// One row; its rectangle, for the drag's slots.
fn list_row(ui: &mut Ui, state: &FrameState<'_>, row: &ListRow, out: &mut Vec<Action>) -> Rect {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let (slot, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    // The row's own surface, inset from the card's edges.
    let rect = surface_rect(slot);
    // The operation's own id, not the layout's: a status cell that comes
    // or goes (the spinner's child) must not move a row's id between the
    // frame a click is aimed and the frame it lands.
    let response = ui.interact(
        rect,
        Id::unique(("dc.operations.row", row.id.as_str())),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::ListItem, true, row.editing, &row.name)
    });
    surface(ui, rect, row.editing, response.hovered());
    if response.clicked() {
        out.push(Action::EditOperation(row.id.clone()));
    }
    // The row under the pointer previews its targets on the Viewport and
    // in the Design Tree (capability 67), whichever of its cells it is on.
    if ui.is_enabled() && ui.rect_contains_pointer(rect) {
        ui.data_mut(|d| d.insert_temp(hovered_id(), row.id.clone()));
    }
    if ui.data(|d| d.get_temp::<OperationId>(reveal_id())).as_ref() == Some(&row.id) {
        ui.data_mut(|d| d.remove::<OperationId>(reveal_id()));
        response.scroll_to_me(None);
    }
    // Capability 78: the estimated time of the program it is cut in, the
    // CLI's number, on hover.
    let times = dc_app::export::operation_times(state.app, &row.id, state.estimate);
    let response = if times.is_empty() {
        response
    } else {
        let lines: Vec<String> = times
            .iter()
            .map(|(name, time)| {
                strings.fmt(
                    StrKey::ExportOperationEstimate,
                    &[("name", name), ("time", time)],
                )
            })
            .collect();
        response.on_hover_text(lines.join("\n"))
    };
    response.context_menu(|ui| {
        if ui
            .button(strings.get(StrKey::OperationsDuplicate))
            .clicked()
        {
            out.push(Action::run(Command::DuplicateOperation(row.id.clone())));
            ui.close();
        }
        if ui.button(strings.get(StrKey::OperationsDelete)).clicked() {
            out.push(Action::run(Command::DeleteOperation(row.id.clone())));
            ui.close();
        }
        // A Profile's tab request, placed as `tabs place` places it (#546).
        let requested = state.app.project().operations.iter().any(|o| {
            o.id() == &row.id && matches!(o, dc_project::Operation::Profile(p) if p.tabs.is_some())
        });
        if requested
            && ui
                .button(strings.get(StrKey::OperationsPlaceTabs))
                .clicked()
        {
            out.push(Action::PlaceTabs(row.id.clone()));
            ui.close();
        }
    });

    let cell = |x: f32| {
        Rect::from_center_size(
            egui::pos2(x + ICON_PX / 2.0, rect.center().y),
            egui::vec2(ICON_PX, ICON_PX),
        )
    };
    // The grip: a drag handle, the row's only one.
    let grip_rect = cell(rect.left() + 8.0);
    let grip = ui.interact(
        grip_rect.expand(4.0),
        Id::unique(("dc.operations.grip", row.id.as_str())),
        Sense::drag(),
    );
    let grip_name = strings.get(StrKey::OperationsGrip);
    grip.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, grip_name));
    if grip.drag_started() {
        ui.data_mut(|d| d.insert_temp(dragging_id(), row.id.clone()));
    }
    if grip.hovered() || grip.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let grip_colour = if response.hovered() || grip.hovered() {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    glyph(ui, grip_rect, Icon::GripVertical, grip_colour);

    // The enable checkbox.
    let check_rect = cell(grip_rect.right() + 6.0);
    let check = ui.interact(
        check_rect.expand(4.0),
        Id::unique(("dc.operations.enable", row.id.as_str())),
        Sense::click(),
    );
    let check_name = strings.fmt(StrKey::OperationsEnable, &[("name", &row.name)]);
    check.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, true, row.enabled, &check_name)
    });
    paint_box(ui, check_rect, row.enabled, check.hovered(), true);
    focus_ring_round(ui, &check, check_rect, theme::MARK_RADIUS_PX);
    if check.clicked() {
        out.push(Action::run(Command::SetOperationEnabled {
            id: row.id.clone(),
            enabled: !row.enabled,
        }));
    }

    // The kind's glyph in the operation's colour.
    let glyph_rect = cell(check_rect.right() + 8.0);
    op_glyph(
        ui.painter(),
        glyph_rect,
        row.kind,
        palette.operation(row.colour),
    );

    // The status cell and the eye, from the right.
    let status_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 4.0 - STATUS_PX / 2.0, rect.center().y),
        egui::vec2(STATUS_PX, rect.height()),
    );
    status_cell(ui, state, status_rect, row);
    let eye_rect = cell(status_rect.left() - 4.0 - ICON_PX);
    let eye = ui.interact(
        eye_rect.expand(4.0),
        Id::unique(("dc.operations.eye", row.id.as_str())),
        Sense::click(),
    );
    let eye_name = strings.fmt(
        if row.hidden {
            StrKey::OperationsShowToolpath
        } else {
            StrKey::OperationsHideToolpath
        },
        &[("name", &row.name)],
    );
    eye.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &eye_name));
    let eye_colour = if eye.hovered() {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    glyph(
        ui,
        eye_rect,
        if row.hidden { Icon::EyeOff } else { Icon::Eye },
        eye_colour,
    );
    if eye.clicked() {
        out.push(Action::ToggleOperationHidden(row.id.clone()));
    }
    let _ = eye.on_hover_text(eye_name);

    // Name over kind and tool; a disabled operation's in text.secondary.
    let left = glyph_rect.right() + 8.0;
    let width = (eye_rect.left() - 8.0 - left).max(0.0);
    let name_colour = if row.enabled {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let name = truncated(ui, &row.name, &TextStyle::Body, name_colour, width);
    let detail = truncated(
        ui,
        &row.detail,
        &TextStyle::Small,
        palette.text_secondary,
        width,
    );
    // Two lines, 2 px apart, centred in the row.
    let top = rect.center().y - (name.size().y + 2.0 + detail.size().y) / 2.0;
    let detail_top = top + name.size().y + 2.0;
    ui.painter()
        .galley(egui::pos2(left, top), name, name_colour);
    ui.painter()
        .galley(egui::pos2(left, detail_top), detail, palette.text_secondary);
    slot
}

/// The status cell (section 7): the spinner, a warning count, the error
/// mark, or nothing (`widgets::status`, a tool's row's marks too).
/// Hovering it names what it shows.
fn status_cell(ui: &mut Ui, state: &FrameState<'_>, rect: Rect, row: &ListRow) {
    let strings = state.app.strings();
    match row.status {
        Status::Clean => {}
        Status::Generating => {
            let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            ));
            spinner(&mut inside, strings.get(StrKey::AppGenerationRegenerating));
        }
        Status::Warnings(count) => {
            let text = if count == 1 {
                strings.get(StrKey::OperationsWarningsOne).to_owned()
            } else {
                strings.fmt(
                    StrKey::OperationsWarningsMany,
                    &[("count", &count.to_string())],
                )
            };
            let response = ui.interact(
                rect,
                Id::unique(("dc.operations.status", row.id.as_str())),
                Sense::hover(),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
            crate::widgets::status::warnings(ui, rect, count);
            let _ = response.on_hover_text(hover_text(&text, row));
        }
        Status::Error => {
            let text = strings.get(StrKey::OperationsError);
            let response = ui.interact(
                rect,
                Id::unique(("dc.operations.status", row.id.as_str())),
                Sense::hover(),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
            crate::widgets::status::error(ui, rect);
            let _ = response.on_hover_text(hover_text(text, row));
        }
    }
}

/// The status cell's hover: what it shows, then the messages behind it.
fn hover_text(summary: &str, row: &ListRow) -> String {
    std::iter::once(summary)
        .chain(row.messages.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `icon` centred in `rect`.
fn glyph(ui: &Ui, rect: Rect, icon: Icon, colour: egui::Color32) {
    let galley = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
}

/// `text` on one line, truncated with an ellipsis to `width`.
fn truncated(
    ui: &Ui,
    text: &str,
    style: &TextStyle,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        style.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}
