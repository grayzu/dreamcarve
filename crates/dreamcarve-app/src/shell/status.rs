// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The status bar (functional spec, capability 50b; `experience.md`,
//! section 2, "Status bar"; slice 24, #616; `redesign.md`, "The window"):
//! 28 px on the `surface.window` backdrop, each cell a quiet 24-px pill
//! of 12-px text, 6 px apart, with no rules between them. The cells are
//! `dc_app::status`'s, in two groups, so that none moves when another
//! comes or goes. From the left: the units button and its menu, the
//! counts (the button that opens the Warnings drawer), and the last
//! message, which takes the room that is left. From the right: one cell
//! for the job, generation while it runs (*Generating k of n…* and Stop,
//! or *Stopped* and *Regenerate all*) and the estimate when it is done;
//! and, left of it, one cell for the pointer, a drag's travel or else the
//! shape under it. A cell with nothing to say is not drawn. The cursor's
//! position, the Z range and the toolpath view are the Viewport's
//! floating controls (`viewport::controls`), and there is no stock size:
//! the Stock step shows it.

use dc_app::status::{CountsCell, GenerationAction, GenerationCell};
use dc_app::warnings::Level;
use dc_app::{Command, StrKey};
use dc_project::Units;
use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring_at, text};
use crate::widgets::icon::Icon;
use crate::widgets::spinner::spinner;

/// The status bar's height, px.
const HEIGHT_PX: f32 = theme::STATUS_BAR_PX;
/// The margin under the bar and under the cards above it, px: with the
/// 2 px a pill stands off the bar's edge, the gap round the cards.
pub(crate) const BOTTOM_PX: i8 = 6;
/// A cell's pill, px high.
const CELL_PX: f32 = theme::CHIP_PX;
/// The gap between cells, px.
const CELL_GAP_PX: f32 = 6.0;
/// The space left and right of a cell's content, px.
const CELL_PADDING_PX: f32 = 10.0;

/// Where the right-hand group's width is kept between frames: the room
/// the message, the last of the left-hand cells, stops short of.
fn right_width_id() -> egui::Id {
    egui::Id::unique("dc.status.right_width")
}

/// The pill of a cell in `rect`, with `wash` over it (a button cell's
/// hover or pressed look).
fn pill(palette: &Palette, rect: Rect, wash: egui::Color32) -> egui::Shape {
    let radius = theme::pill(rect.height());
    egui::Shape::Vec(vec![
        egui::epaint::RectShape::new(
            rect,
            radius,
            palette.surface_panel,
            Stroke::new(1.0, palette.border_default),
            StrokeKind::Inside,
        )
        .into(),
        egui::epaint::RectShape::filled(rect, radius, wash).into(),
    ])
}

/// Lays `body` out in a cell and returns the cell's rectangle and a slot
/// under the content for its pill, which the caller paints.
fn cell_body<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> (Rect, egui::layers::ShapeIdx, R) {
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), CELL_PX),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // The closing padding stays inside the bar. Set before
            // anything is laid out: narrowing a row rewinds its cursor.
            ui.set_max_width((ui.available_width() - CELL_PADDING_PX).max(0.0));
            ui.add_space(CELL_PADDING_PX);
            let out = body(ui);
            ui.add_space(CELL_PADDING_PX);
            out
        },
    );
    let content = inner.response.rect;
    let rect = Rect::from_center_size(content.center(), egui::vec2(content.width(), CELL_PX));
    (rect, slot, inner.inner)
}

/// A cell that only shows: `body` on a quiet pill.
fn cell<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = Palette::current(ui);
    let (rect, slot, out) = cell_body(ui, body);
    ui.painter().set(slot, pill(palette, rect, theme::NONE));
    out
}

/// [`cell`] in the right-hand group, which is laid out from the bar's
/// right end: `body` adds its parts last first, and the cell ends where
/// the layout stands, whatever its width.
fn cell_from_right<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let palette = Palette::current(ui);
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), CELL_PX),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(CELL_PADDING_PX);
            let out = body(ui);
            ui.add_space(CELL_PADDING_PX);
            out
        },
    );
    let content = inner.response.rect;
    let rect = Rect::from_center_size(content.center(), egui::vec2(content.width(), CELL_PX));
    ui.painter().set(slot, pill(palette, rect, theme::NONE));
    inner.inner
}

/// A cell that is a button named `name`, its id from `salt`: the pill
/// takes the hover wash, and the pressed one while `pressed` or held.
fn button_cell(
    ui: &mut Ui,
    salt: &str,
    name: &str,
    pressed: bool,
    body: impl FnOnce(&mut Ui),
) -> Response {
    let palette = Palette::current(ui);
    let (rect, slot, ()) = cell_body(ui, body);
    let response = ui.interact(rect, ui.make_persistent_id(salt), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    let wash = if pressed || response.is_pointer_button_down_on() {
        palette.surface_active
    } else if response.hovered() {
        palette.surface_hover
    } else {
        theme::NONE
    };
    ui.painter().set(slot, pill(palette, rect, wash));
    focus_ring_at(ui, &response, theme::pill(rect.height()));
    response
}

/// `label` in `colour` at the small size, cut with an ellipsis at the
/// room left: a button cell's own name, painted, so it is not a second
/// node of that name beside the button.
fn painted(ui: &mut Ui, label: &str, colour: egui::Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        TextStyle::Small.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(ui.available_width().max(0.0));
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, colour);
}

/// A glyph in `colour` at the small icon size.
fn glyph(ui: &mut Ui, icon: Icon, colour: egui::Color32) {
    let galley = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, colour);
}

/// The units cell: a button reading `mm` or `in` with a `chevron-down`,
/// whose two-row menu switches the project's display units (capability
/// 53) as the Stock step's switch does: one undo step.
fn units(ui: &mut Ui, state: &FrameState<'_>, label: &str, current: Units, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let id = "status_units";
    // The id `Popup::menu` gives the menu of the response made below.
    let open = egui::Popup::is_id_open(ui.ctx(), ui.make_persistent_id(id).with("popup"));
    let response = button_cell(ui, id, label, open, |ui| {
        painted(ui, label, palette.text_secondary);
        glyph(ui, Icon::ChevronDown, palette.text_secondary);
    })
    .on_hover_text(strings.get(StrKey::AppStatusUnitsHint));
    egui::Popup::menu(&response)
        .align(egui::RectAlign::TOP_START)
        .show(|ui| {
            for (units, key) in [
                (Units::Mm, StrKey::AppStatusUnitsMm),
                (Units::Inch, StrKey::AppStatusUnitsInch),
            ] {
                let row = ui.add(egui::Button::new(strings.get(key)).selected(units == current));
                if row.clicked() {
                    if units != current {
                        out.push(Action::run(Command::SetDisplayUnits(units)));
                    }
                    ui.close();
                }
            }
        });
}

/// The generation cell (capability 71; `experience.md`, section 8,
/// "Progress"): the spinner while anything is pending, the text, and Stop
/// or, after Stop, *Regenerate all*: the only place that command lives.
/// It is one of the right-hand cells, so its parts are added last first.
fn generation(ui: &mut Ui, cell: &GenerationCell, palette: &Palette, out: &mut Vec<Action>) {
    if let Some(action) = &cell.action {
        let spec = ButtonSpec::new(Kind::Quiet, &action.label).height(CELL_PX - 4.0);
        if button(ui, spec).clicked() {
            out.push(match action.action {
                GenerationAction::Stop => Action::StopGeneration,
                GenerationAction::RegenerateAll => Action::RegenerateAll,
            });
        }
    }
    let colour = if cell.spinner {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    text(ui, &cell.text, TextStyle::Small, colour);
    if cell.spinner {
        spinner(ui, "");
    }
}

/// The counts cell (`experience.md`, section 2, "Status bar"): each tier
/// with rows as its glyph and count in its colour, or `circle-check` and
/// *No problems*; one button that opens and closes the Warnings drawer.
fn counts(ui: &mut Ui, state: &FrameState<'_>, cell: &CountsCell, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let title = state.app.strings().get(StrKey::AppWarningsTitle);
    let response = button_cell(ui, "warnings_counts", title, state.warnings_open, |ui| {
        if let Some(none) = &cell.none {
            glyph(ui, Icon::CircleCheck, palette.text_secondary);
            text(ui, none, TextStyle::Small, palette.text_secondary);
        }
        for (level, label) in &cell.tiers {
            let (icon, colour) = match level {
                Level::Error => (Icon::CircleX, palette.status_error.text),
                Level::Warning => (Icon::TriangleAlert, palette.status_warning.text),
                Level::Hint | Level::Note => (Icon::Info, palette.status_info.text),
            };
            glyph(ui, icon, colour);
            text(ui, label, TextStyle::Small, colour);
        }
    });
    if response.clicked() {
        out.push(Action::ToggleWarnings);
    }
}

/// Draws the status bar; its rectangle.
pub(crate) fn bar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) -> Rect {
    let palette = Palette::current(ui);
    #[allow(clippy::cast_possible_truncation, reason = "the window gap, 8 px")]
    let side = theme::GAP_PX as i8;
    egui::Panel::bottom("status_bar")
        .exact_size(HEIGHT_PX + f32::from(BOTTOM_PX))
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(egui::Margin {
                    left: side,
                    right: side,
                    top: 0,
                    bottom: BOTTOM_PX,
                }),
        )
        .show(ui, |ui| {
            if state.inert() {
                ui.disable();
            }
            let cells = &state.cells;
            let full = ui.max_rect();
            // The message stops short of the right-hand group as the last
            // frame drew it.
            let width: f32 = ui.data(|d| d.get_temp(right_width_id())).unwrap_or(0.0);
            let gap = if width > 0.0 { CELL_GAP_PX } else { 0.0 };
            let left_rect = full.with_max_x((full.right() - width - gap).max(full.left()));
            let row = |ui: &mut Ui, rect: Rect, layout: egui::Layout| {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
                child.spacing_mut().item_spacing.x = CELL_GAP_PX;
                child
            };
            let centred = egui::Align::Center;

            // From the left: what holds still.
            let mut left = row(ui, left_rect, egui::Layout::left_to_right(centred));
            units(&mut left, state, &cells.units_label, cells.units, out);
            counts(&mut left, state, &cells.counts, out);
            if let Some(label) = &cells.message {
                message(&mut left, state, label, cells.message_op.as_ref(), out);
            }

            // From the right: what comes and goes. First the job:
            // generation while it runs, its estimate after (capability
            // 78: every program's estimated time).
            let mut right = row(ui, full, egui::Layout::right_to_left(centred));
            if let Some(generating) = &cells.generation {
                cell_from_right(&mut right, |ui| generation(ui, generating, palette, out));
            } else if let Some(estimate) = &cells.estimate {
                let help = state.app.strings().get(StrKey::ExportStatusTotalHelp);
                cell_from_right(&mut right, |ui| {
                    text(ui, estimate, TextStyle::Small, palette.text_secondary)
                })
                .on_hover_text(help);
            }
            // Then the pointer: a drag's travel, scale or angle (#534),
            // else the shape under it.
            if let Some(label) = cells.drag.as_ref().or(cells.hover.as_ref()) {
                cell_from_right(&mut right, |ui| {
                    text(ui, label, TextStyle::Small, palette.text_primary)
                });
            }
            let used = right.min_rect().width();
            if (used - width).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(right_width_id(), used));
                ui.ctx().request_repaint();
            }
            ui.allocate_rect(full, Sense::hover());
        })
        .response
        .rect
}

/// The message cell: the last message, or the selected operation's carve
/// summary (capability 76a), which opens the Warnings drawer at its note.
fn message(
    ui: &mut Ui,
    state: &FrameState<'_>,
    label: &str,
    op: Option<&dc_project::OperationId>,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let Some(op) = op else {
        cell(ui, |ui| {
            text(ui, label, TextStyle::Small, palette.text_secondary)
        });
        return;
    };
    let salt = format!("status_note.{}", op.as_str());
    let response = button_cell(ui, &salt, label, false, |ui| {
        text(ui, label, TextStyle::Small, palette.text_secondary);
    })
    .on_hover_text(state.app.strings().get(StrKey::AppCarveOpenNote));
    if response.clicked() {
        out.push(Action::ShowNote(op.clone()));
    }
}
