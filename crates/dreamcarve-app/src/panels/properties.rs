// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Properties panel with instances selected (functional spec,
//! capability 58; mock-up `01-workspace.svg`, Properties;
//! `experience.md`, section 7): X and Y (the selection box's bottom-left),
//! W and H with the proportion link to their right (one chain spanning
//! both rows, linked or broken), and the operations targeting the
//! selection. Nothing else: rotation and mirroring are the toolbar's
//! Arrange actions (58a; slice 25, #617). Fields are
//! `dc_app::properties`'; a field commits on leave as one
//! `TransformInstances` about the box's centre, and a refusal (a value
//! that is not a length) goes to the status bar. A locked selection's
//! fields are read-only under the line *Locked: cannot be moved or
//! resized. Operations still cut it.* and an *Unlock* button.
//!
//! With placed tabs selected (#546) it shows their width, height and
//! shape, the one tab's position and the Profiles that cross it, and
//! *Delete* ([`tabs`]).
//!
//! The link is view state, not project state: it lives in egui's memory,
//! and the Viewport's corner handles read it too ([`proportions_free`]).

use dc_app::properties::{self, Field, Placement};
use dc_app::units;
use dc_app::viewport::tabs::TabEdit;
use dc_app::{CommandError, StrKey};
use dc_project::{InstanceId, TabId, TabShape};
use eframe::egui::{self, Id, Rect, Stroke, TextStyle, Ui, UiBuilder};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented;
use crate::widgets::unit_field::{UnitField, field as unit_field, row};

/// The proportion link's column right of the fields, px
/// (`experience.md`, section 7, "Proportion link").
const LINK_COLUMN_PX: f32 = 26.0;
/// The link's toggle, px square.
const LINK_TOGGLE_PX: f32 = 26.0;
/// The gap between a field and the link's column, px.
const LINK_GAP_PX: f32 = 4.0;
/// The bracket's ticks toward the fields, px.
const BRACKET_TICK_PX: f32 = 6.0;

/// Where the W/H proportion link's state lives.
fn link_id() -> Id {
    Id::unique("placement_proportions_free")
}

/// Whether the W/H proportion link is broken, so W and H (and a corner
/// drag) scale each axis on its own.
pub(crate) fn proportions_free(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(link_id())).unwrap_or(false)
}

/// The panel's title for `ids`: the one shape's name, or the count.
pub(crate) fn title(state: &FrameState<'_>, ids: &[InstanceId]) -> String {
    let strings = state.app.strings();
    let project = state.app.project();
    match ids {
        [one] => {
            let name = project
                .instances
                .iter()
                .find(|i| i.id == *one)
                .map_or_else(|| one.as_str().to_owned(), |i| i.name.clone());
            strings.fmt(StrKey::PlacementTitle, &[("name", &name)])
        }
        _ => strings.fmt(
            StrKey::PlacementTitleMany,
            &[("count", &ids.len().to_string())],
        ),
    }
}

/// One placement field's row, narrowed to leave the link's column free;
/// the row's vertical middle, where the link's bracket meets it.
fn field_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    placement: &Placement,
    ids: &[InstanceId],
    field: Field,
    out: &mut Vec<Action>,
) -> f32 {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    let (key, name) = match field {
        Field::X => (StrKey::PlacementX, "x"),
        Field::Y => (StrKey::PlacementY, "y"),
        Field::W => (StrKey::PlacementW, "w"),
        Field::H => (StrKey::PlacementH, "h"),
    };
    let value = properties::field_text(placement, field, units);
    let read_only = properties::read_only(placement);
    let id = Id::unique(("placement_field", name));
    let name = strings.get(key);
    let (label, output) = row(ui, name, |ui| {
        ui.set_max_width((ui.available_width() - LINK_GAP_PX - LINK_COLUMN_PX).max(0.0));
        unit_field(
            ui,
            UnitField {
                placeholder: if value.is_empty() {
                    strings.get(StrKey::PlacementMixed)
                } else {
                    ""
                },
                enabled: !read_only,
                disabled_reason: read_only.then(|| strings.get(StrKey::PlacementLockedNote)),
                ..UnitField::new(id, name, &value, properties::field_unit(field, units))
            },
        )
    });
    let middle = label.rect.center().y;
    let _ = output.response.labelled_by(label.id);
    if let Some(typed) = output.committed {
        let uniform = !proportions_free(ui.ctx());
        match properties::parse(placement, field, &typed, units, uniform)
            .and_then(|edit| properties::edit(state.app.project(), ids, edit))
        {
            Ok(command) => out.push(Action::Run(Box::new(command))),
            Err(CommandError::NothingToDo) => {}
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
    ui.add_space(4.0);
    middle
}

/// The proportion link right of W and H (`experience.md`, section 7): a
/// 1-px bracket from the middle of W's row to the middle of H's, with a
/// quiet toggle on it holding `link` (linked) or `unlink` (broken).
fn proportion_link(ui: &mut Ui, state: &FrameState<'_>, (w_y, h_y): (f32, f32), enabled: bool) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let free = proportions_free(ui.ctx());
    let column = Rect::from_x_y_ranges(
        (ui.max_rect().right() - LINK_COLUMN_PX)..=ui.max_rect().right(),
        w_y..=h_y,
    );
    let x = column.center().x;
    let stroke = Stroke::new(1.0, palette.border_strong);
    let painter = ui.painter();
    painter.line_segment(
        [egui::pos2(x - BRACKET_TICK_PX, w_y), egui::pos2(x, w_y)],
        stroke,
    );
    painter.vline(x, w_y..=h_y, stroke);
    painter.line_segment(
        [egui::pos2(x - BRACKET_TICK_PX, h_y), egui::pos2(x, h_y)],
        stroke,
    );
    let toggle = Rect::from_center_size(
        egui::pos2(x, column.center().y),
        egui::vec2(LINK_TOGGLE_PX, LINK_TOGGLE_PX),
    );
    painter.rect_filled(toggle.shrink(2.0), 4.0, palette.surface_panel);
    let (icon, colour) = match (free, enabled) {
        (_, false) => (
            if free { Icon::Unlink } else { Icon::Link },
            palette.text_disabled,
        ),
        (true, true) => (Icon::Unlink, palette.text_secondary),
        (false, true) => (Icon::Link, palette.text_primary),
    };
    // The glyph is painted here, in the link's own colour; the quiet
    // button gives the hover, the hit area and the name.
    let spec = ButtonSpec::new(Kind::Quiet, "")
        .width(LINK_TOGGLE_PX)
        .tooltip(strings.get(StrKey::PlacementKeepProportions))
        .enabled(enabled);
    let clicked = ui
        .scope_builder(UiBuilder::new().max_rect(toggle), |ui| {
            let response = button(ui, spec);
            let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colour);
            ui.painter()
                .galley(toggle.center() - glyph.size() / 2.0, glyph, colour);
            response.clicked()
        })
        .inner;
    if clicked {
        ui.data_mut(|d| d.insert_temp(link_id(), !free));
    }
}

/// The locked note (capability 58): the padlock, the line saying what a
/// lock stops, and *Unlock*, one step unlocking the selection's locked
/// shapes.
fn locked_note(ui: &mut Ui, state: &FrameState<'_>, ids: &[InstanceId], out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let glyph = Icon::Lock.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
        let (rect, _) = ui.allocate_exact_size(glyph.size(), egui::Sense::hover());
        ui.painter().galley(rect.min, glyph, palette.text_secondary);
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(strings.get(StrKey::PlacementLockedNote))
                        .text_style(TextStyle::Small)
                        .color(palette.text_secondary),
                )
                .wrap()
                .selectable(false),
            );
            ui.add_space(8.0);
            let unlock = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::PlacementUnlock));
            if button(ui, unlock).clicked()
                && let Some(command) = properties::unlock(state.app.project(), ids)
            {
                out.push(Action::run(command));
            }
        });
    });
    ui.add_space(8.0);
}

/// Draws the placement of `ids`.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, ids: &[InstanceId], out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let project = state.app.project();
    let palette = Palette::current(ui);
    let Some(placement) = properties::placement(project, ids) else {
        return;
    };
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            let mut middles = [0.0; 4];
            for (field, middle) in properties::fields(&placement).iter().zip(&mut middles) {
                *middle = field_row(ui, state, &placement, ids, *field, out);
            }
            let [_, _, w_y, h_y] = middles;
            proportion_link(ui, state, (w_y, h_y), !properties::read_only(&placement));
            if properties::read_only(&placement) {
                locked_note(ui, state, ids, out);
            }
            ui.add_space(4.0);
            let targeted = if placement.targeted_by.is_empty() {
                strings.get(StrKey::PlacementNotTargeted).to_owned()
            } else {
                strings.fmt(
                    StrKey::PlacementTargetedBy,
                    &[("names", &placement.targeted_by.join(", "))],
                )
            };
            text(ui, &targeted, TextStyle::Small, palette.text_secondary);
        });
}

/// The panel's title for placed tabs `ids`: the one tab's id, or the
/// count.
pub(crate) fn tabs_title(state: &FrameState<'_>, ids: &[TabId]) -> String {
    let strings = state.app.strings();
    match ids {
        [one] => strings.fmt(StrKey::PlacementTitle, &[("name", one.as_str())]),
        _ => strings.fmt(
            StrKey::PlacementTabTitleMany,
            &[("count", &ids.len().to_string())],
        ),
    }
}

/// Runs what `edit` makes of the selected tabs, or says why not; an
/// unchanged value is no step.
fn push_tab_edit(state: &FrameState<'_>, ids: &[TabId], edit: TabEdit, out: &mut Vec<Action>) {
    match dc_app::viewport::tabs::edit(state.app.project(), ids, edit) {
        Ok(command) => out.push(Action::Run(Box::new(command))),
        Err(CommandError::NothingToDo) => {}
        Err(e) => out.push(Action::Refused(e.to_string())),
    }
}

/// A tab's length field: its width or height, *Mixed* when the selected
/// tabs differ; a typed length commits on leave.
fn tab_length_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    ids: &[TabId],
    (key, name): (StrKey, &str),
    value_mm: Option<f64>,
    edit: fn(f64) -> TabEdit,
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    let value = value_mm.map_or_else(String::new, |v| units::field_text(v, units));
    let label = strings.get(key);
    let (label_response, output) = row(ui, label, |ui| {
        unit_field(
            ui,
            UnitField {
                placeholder: if value_mm.is_none() {
                    strings.get(StrKey::PlacementMixed)
                } else {
                    ""
                },
                ..UnitField::new(
                    Id::unique(("tab_field", name)),
                    label,
                    &value,
                    units::suffix(units),
                )
            },
        )
    });
    let _ = output.response.labelled_by(label_response.id);
    if let Some(typed) = output.committed {
        match units::parse_length_mm(&typed, units) {
            Ok(mm) => push_tab_edit(state, ids, edit(mm), out),
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
    ui.add_space(4.0);
}

/// Draws the properties of placed tabs `ids` (functional spec, capability
/// 69; mock-up 08): width, height and shape, each one undo step over
/// every selected tab and the size the Tab tool adds next; the one tab's
/// position in project space, read-only; the Profiles that cross it; and
/// *Delete*.
pub(crate) fn tabs(ui: &mut Ui, state: &FrameState<'_>, ids: &[TabId], out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let project = state.app.project();
    let palette = Palette::current(ui);
    let Some(values) = dc_app::viewport::tabs::values(project, ids) else {
        return;
    };
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            tab_length_row(
                ui,
                state,
                ids,
                (StrKey::PlacementTabWidth, "width"),
                values.width_mm,
                TabEdit::Width,
                out,
            );
            tab_length_row(
                ui,
                state,
                ids,
                (StrKey::PlacementTabHeight, "height"),
                values.height_mm,
                TabEdit::Height,
                out,
            );
            let shapes = [
                (TabShape::Rect, strings.get(StrKey::EditorChoiceRect)),
                (TabShape::Smooth, strings.get(StrKey::EditorChoiceSmooth)),
            ];
            let labels: Vec<&str> = shapes.iter().map(|(_, l)| *l).collect();
            let current = values
                .shape
                .and_then(|s| shapes.iter().position(|(shape, _)| *shape == s))
                .unwrap_or(labels.len());
            let (_, picked) = row(ui, strings.get(StrKey::PlacementTabShape), |ui| {
                segmented(
                    ui,
                    Id::unique("tab_field_shape"),
                    &labels,
                    current,
                    ui.available_width(),
                )
            });
            if let Some((shape, _)) = picked.and_then(|i| shapes.get(i)) {
                push_tab_edit(state, ids, TabEdit::Shape(*shape), out);
            }
            ui.add_space(4.0);

            let units = project.display_units;
            let one = match ids {
                [one] => project.tabs.iter().find(|t| t.id == *one),
                _ => None,
            };
            if let Some(tab) = one
                && let Some(at) = dc_job::tabs::tab_point(project, tab)
            {
                let text = format!(
                    "{}, {}",
                    units::field_text(at.x, units),
                    units::field_text(at.y, units)
                );
                let name = strings.get(StrKey::PlacementTabPosition);
                let (label_response, output) = row(ui, name, |ui| {
                    unit_field(
                        ui,
                        UnitField {
                            enabled: false,
                            ..UnitField::new(
                                Id::unique("tab_field_position"),
                                name,
                                &text,
                                units::suffix(units),
                            )
                        },
                    )
                });
                let _ = output.response.labelled_by(label_response.id);
                ui.add_space(4.0);
                let crossing: Vec<&str> = state
                    .app
                    .scene()
                    .tab(&tab.id)
                    .map(|m| {
                        m.crossed_by
                            .iter()
                            .filter_map(|op| {
                                project
                                    .operations
                                    .iter()
                                    .find(|o| o.id() == op)
                                    .map(dc_project::Operation::name)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let sentence = if crossing.is_empty() {
                    strings.get(StrKey::PlacementTabNotCrossed).to_owned()
                } else {
                    strings.fmt(
                        StrKey::PlacementTabCrossedBy,
                        &[("names", &crossing.join(", "))],
                    )
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(sentence)
                            .text_style(TextStyle::Small)
                            .color(palette.text_secondary),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
            }
            let spec = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::PlacementTabDelete))
                .icon(Icon::Trash2, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::PlacementTabDeleteHelp));
            if button(ui, spec).clicked() {
                out.push(Action::run(dc_app::Command::DeleteTabs(ids.to_vec())));
            }
        });
}
