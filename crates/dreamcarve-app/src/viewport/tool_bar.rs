// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The tool bar (functional spec, phase 5, capabilities 81 and 83): while
//! a drawing tool is armed, a raised bar floating under the Viewport's
//! top pills, centred, with the tool's name, one line saying what the
//! next click does, the tool's choices, the sizes of the step in hand as
//! fields, and **Done** where a shape can be finished early.
//!
//! Everything it shows is `dc_app::App::tool_bar`'s. A field holds what
//! the pointer gives until a value is typed in it; the typed value is
//! pinned and goes to the tool as [`Event::Field`]. `Enter` in a field
//! writes the field and then commits the step with the fields as they
//! read; `Escape` in a pinned field unpins it. `Tab` from the drawing
//! (the Viewport focused, or nothing) goes to the bar's first field.

use dc_app::problems::Problems;
use dc_app::tool::{Event, Key as ToolKey, Quantity, ToolBar};
use dc_app::{StrKey, Tool, units};
use eframe::egui::{self, Id, Key, Rect, Sense, Stroke, TextStyle, Ui, UiBuilder};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::segmented::segmented;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// The bar's height, px: a control and the padding round it.
const BAR_PX: f32 = 48.0;
/// The room one row of the bar takes under the pills, px: what Fit
/// leaves clear while a drawing tool is armed.
pub(super) const ROW_PX: f32 = ABOVE_PX + BAR_PX;
/// The bar's padding, px.
const PADDING_PX: f32 = 8.0;
/// The gap between the bar's parts, px.
const GAP_PX: f32 = 12.0;
/// The gap between a field's label and the field, px.
const LABEL_GAP_PX: f32 = 6.0;
/// A length field's width, px.
const FIELD_PX: f32 = 96.0;
/// A count's or an angle's field, px.
const SHORT_FIELD_PX: f32 = 72.0;
/// One option of a choice, px wide.
const OPTION_PX: f32 = 76.0;
/// The gap between the bar's rows when it wraps, px.
const ROW_GAP_PX: f32 = 6.0;
/// The gap between the bar and the pills above it, px.
const ABOVE_PX: f32 = 8.0;

/// Where what the bar measured is kept between frames: its size, to
/// centre it and to paint its surface under what it holds, and its
/// parts' widths, to break it into rows.
fn measured_id() -> Id {
    Id::unique("dc.viewport.tool_bar.measured")
}

/// The id of the bar's field for `field`.
fn field_id(field: dc_app::tool::SizeField) -> Id {
    Id::unique(("dc.viewport.tool_bar.field", field))
}

/// The unit a field of `quantity` shows after its value.
const fn unit(quantity: Quantity, units: dc_project::Units) -> &'static str {
    match quantity {
        Quantity::Length => units::suffix(units),
        Quantity::Angle => "°",
        Quantity::Count => "",
    }
}

/// The name of the tool in `bar`.
fn name(strings: &dc_app::Strings, bar: &ToolBar) -> String {
    match bar.tool {
        Tool::Draw(drawing) => strings.get(drawing.name()).to_owned(),
        _ => String::new(),
    }
}

/// One part of the bar, which stays whole when the bar wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The tool's name and what the next click does.
    Head,
    /// The choice at this index.
    Choice(usize),
    /// The field at this index, with its label.
    Field(usize),
    /// The note and **Done**.
    Tail,
}

/// What the bar measured in the last frame: its size, and each part's
/// width, by which this frame breaks it into rows.
#[derive(Debug, Clone, PartialEq, Default)]
struct Measured {
    size: egui::Vec2,
    parts: Vec<(Part, f32)>,
}

/// `parts` in rows no wider than `room_px`, by the widths measured in the
/// last frame; a part not measured yet goes on the row in hand.
fn rows(parts: &[Part], measured: &Measured, room_px: f32) -> Vec<Vec<Part>> {
    let mut rows: Vec<Vec<Part>> = vec![Vec::new()];
    let mut used = 0.0;
    for &part in parts {
        let width = measured
            .parts
            .iter()
            .find(|(measured, _)| *measured == part)
            .map_or(0.0, |(_, width)| *width);
        let fits = used + width <= room_px;
        if let Some(row) = rows.last_mut()
            && (fits || row.is_empty())
        {
            row.push(part);
            used += width + GAP_PX;
        } else {
            rows.push(vec![part]);
            used = width + GAP_PX;
        }
    }
    rows
}

/// Selects all of `text` in the field `id`: a size is typed over what the
/// pointer gave, so the field's text is selected as it takes the
/// keyboard.
fn select_all(ctx: &egui::Context, id: Id, text: &str) {
    let mut edit = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    edit.cursor
        .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
    edit.store(ctx, id);
}

/// Draws one field of the bar, with its label; what it tells the tool.
fn field_part(
    ui: &mut Ui,
    state: &FrameState<'_>,
    row: &dc_app::tool::SizeRow,
    (entered, escaped): (bool, bool),
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let id = field_id(row.field);
    let wide = match row.quantity {
        Quantity::Length => FIELD_PX,
        Quantity::Angle | Quantity::Count => SHORT_FIELD_PX,
    };
    let problems = row
        .problem
        .as_deref()
        .map(|why| Problems::refused(strings, why));
    ui.spacing_mut().item_spacing.x = LABEL_GAP_PX;
    let label = crate::widgets::row::inline_label(ui, &row.label, &row.help);
    let output = ui
        .allocate_ui_with_layout(
            egui::vec2(wide, theme::CONTROL_PX),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(wide);
                unit_field(
                    ui,
                    UnitField {
                        problems: problems.as_ref(),
                        ..UnitField::new(
                            id,
                            &row.label,
                            &row.text,
                            unit(row.quantity, state.app.project().display_units),
                        )
                    },
                )
            },
        )
        .inner;
    let left = output.response.lost_focus();
    if output.response.gained_focus() {
        select_all(ui.ctx(), id, &output.text);
    }
    let _ = output.response.labelled_by(label.id);
    if let Some(typed) = output.committed {
        out.push(Action::Tool(Event::Field {
            field: row.field,
            text: typed,
        }));
    } else if left && escaped && row.pinned {
        // Escape in a pinned field gives it back to the pointer.
        out.push(Action::Tool(Event::Field {
            field: row.field,
            text: String::new(),
        }));
    }
    // Enter writes the field, then commits the step as it reads.
    if left && entered {
        out.push(Action::Tool(Event::Key(ToolKey::Enter)));
    }
}

/// Draws one choice of the bar, with its label.
fn choice_part(ui: &mut Ui, choice: &dc_app::tool::Choice, out: &mut Vec<Action>) {
    let labels: Vec<&str> = choice.options.iter().map(|o| o.label.as_str()).collect();
    #[allow(
        clippy::cast_precision_loss,
        reason = "a choice has two or three options"
    )]
    let wide = OPTION_PX * labels.len() as f32;
    ui.spacing_mut().item_spacing.x = LABEL_GAP_PX;
    let label = crate::widgets::row::inline_label(ui, &choice.label, &choice.help);
    let shown = ui.scope(|ui| {
        segmented(
            ui,
            Id::unique(("dc.viewport.tool_bar.choice", choice.label.as_str())),
            &labels,
            choice.selected,
            wide,
        )
    });
    let _ = shown.response.labelled_by(label.id);
    if let Some(option) = shown.inner.and_then(|i| choice.options.get(i)) {
        out.push(Action::Tool(Event::Option(option.option.clone())));
    }
}

/// Draws the bar for the armed tool under `top`, centred on `pane`;
/// its rectangle, when there is one. A bar with more than fits across
/// the pane (the Gear tool's sizes) breaks into rows, a label and its
/// control staying together.
pub(super) fn show(
    ui: &mut Ui,
    pane: Rect,
    top: f32,
    to_first_field: bool,
    state: &FrameState<'_>,
    out: &mut Vec<Action>,
) -> Option<Rect> {
    let app = state.app;
    let bar = app.tool_bar()?;
    let palette = Palette::current(ui);
    let strings = app.strings();
    let room = pane.width() - 2.0 * theme::FLOAT_INSET_PX;
    let inner_room = room - 2.0 * PADDING_PX;
    // The bar is as large as what it held in the last frame.
    let last = ui
        .data(|d| d.get_temp::<Measured>(measured_id()))
        .unwrap_or_else(|| Measured {
            size: egui::vec2(room.min(480.0), BAR_PX),
            parts: Vec::new(),
        });
    let size = egui::vec2(last.size.x.min(room), last.size.y);
    let rect = Rect::from_min_size(
        egui::pos2(pane.center().x - size.x / 2.0, top + ABOVE_PX),
        size,
    );
    // It takes the clicks on it, so none reaches the drawing under it.
    ui.interact(
        rect,
        Id::unique("dc.viewport.tool_bar"),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect.expand(32.0).intersect(pane));
    painter.add(theme::shadow(palette).as_shape(rect, theme::CARD_RADIUS_PX));
    painter.rect(
        rect,
        theme::CARD_RADIUS_PX,
        palette.surface_raised,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Inside,
    );

    let mut parts = vec![Part::Head];
    parts.extend((0..bar.choices.len()).map(Part::Choice));
    parts.extend((0..bar.fields.len()).map(Part::Field));
    if bar.note.is_some() || bar.can_finish {
        parts.push(Part::Tail);
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("dc.viewport.tool_bar.ui")
            .max_rect(rect.shrink(PADDING_PX))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(pane);
    child.spacing_mut().item_spacing = egui::vec2(GAP_PX, ROW_GAP_PX);
    let keys = child.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    let mut widths: Vec<(Part, f32)> = Vec::with_capacity(parts.len());
    let mut widest: f32 = 0.0;
    for row in rows(&parts, &last, inner_room) {
        let drawn = child.horizontal(|ui| {
            ui.set_min_height(theme::CONTROL_PX);
            for part in row {
                let shown = ui.horizontal(|ui| {
                    ui.set_min_height(theme::CONTROL_PX);
                    match part {
                        Part::Head => {
                            ui.add_space(4.0);
                            text(
                                ui,
                                &name(strings, &bar),
                                TextStyle::Button,
                                palette.text_primary,
                            );
                            let prompt =
                                text(ui, &bar.prompt, TextStyle::Body, palette.text_secondary);
                            prompt.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Label, true, &bar.prompt)
                            });
                        }
                        Part::Choice(index) => {
                            if let Some(choice) = bar.choices.get(index) {
                                choice_part(ui, choice, out);
                            }
                        }
                        Part::Field(index) => {
                            if let Some(row) = bar.fields.get(index) {
                                field_part(ui, state, row, keys, out);
                            }
                        }
                        Part::Tail => {
                            if let Some(note) = &bar.note {
                                text(ui, note, TextStyle::Small, palette.text_secondary);
                            }
                            if bar.can_finish {
                                let done =
                                    ButtonSpec::new(Kind::Primary, strings.get(StrKey::DrawDone))
                                        .height(theme::CONTROL_PX)
                                        .tooltip(strings.get(StrKey::DrawDoneHint));
                                if button(ui, done).clicked() {
                                    out.push(Action::Tool(Event::Done));
                                }
                            }
                        }
                    }
                });
                widths.push((part, shown.response.rect.width()));
            }
        });
        widest = widest.max(drawn.response.rect.width());
    }

    // Tab from the drawing goes to the first field, whatever egui's own
    // walk gave the focus to on its way here.
    if to_first_field && let Some(first) = bar.fields.first() {
        let id = field_id(first.field);
        ui.memory_mut(|m| m.request_focus(id));
        select_all(ui.ctx(), id, &first.text);
        crate::shell::focus::tab_taken(ui.ctx());
    }

    // The bar is as large as what it holds, from the next frame on.
    let measured = Measured {
        size: egui::vec2(
            widest + 2.0 * PADDING_PX + 4.0,
            child.min_rect().height() + 2.0 * PADDING_PX,
        ),
        parts: widths,
    };
    let settled = (measured.size - last.size).abs().max_elem() <= 0.5
        && measured.parts.len() == last.parts.len()
        && measured
            .parts
            .iter()
            .zip(&last.parts)
            .all(|(now, was)| now.0 == was.0 && (now.1 - was.1).abs() <= 0.5);
    if !settled {
        ui.data_mut(|d| d.insert_temp(measured_id(), measured));
        ui.ctx().request_repaint();
    }
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bar that fits is one row; one that does not breaks where the
    /// row is full, and a part wider than the room has a row to itself.
    #[test]
    fn a_bar_breaks_into_rows_where_it_is_full() {
        let parts = [Part::Head, Part::Field(0), Part::Field(1), Part::Tail];
        let measured = Measured {
            size: egui::Vec2::ZERO,
            parts: vec![
                (Part::Head, 200.0),
                (Part::Field(0), 120.0),
                (Part::Field(1), 120.0),
                (Part::Tail, 60.0),
            ],
        };
        assert_eq!(rows(&parts, &measured, 600.0), [parts.to_vec()]);
        assert_eq!(
            rows(&parts, &measured, 340.0),
            [
                vec![Part::Head, Part::Field(0)],
                vec![Part::Field(1), Part::Tail]
            ]
        );
        assert_eq!(rows(&parts, &measured, 100.0).len(), 4);
        // Not measured yet: all on one row, to be measured.
        assert_eq!(rows(&parts, &Measured::default(), 100.0), [parts.to_vec()]);
    }
}
