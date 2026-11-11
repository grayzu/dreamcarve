// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Properties panel with instances selected (functional spec,
//! capability 58; mock-up `01-workspace.svg`, Properties;
//! `experience.md`, section 7): X and Y (the selection box's bottom-left),
//! W and H with the padlock between them, Rotation, the two Mirror
//! buttons, Visible and Locked, and the operations targeting the
//! selection. Fields are `dc_app::properties`'; a field commits on leave
//! as one `TransformInstances` about the box's centre, and a refusal (a
//! locked shape, a value that is not a length) goes to the status bar.
//! A multi-selection's rotation, Visible and Locked show *Mixed* when the
//! shapes differ.
//!
//! With placed tabs selected (#546) it shows their width, height and
//! shape, the one tab's position and the Profiles that cross it, and
//! *Delete* ([`tabs`]).
//!
//! The padlock is view state, not project state: it lives in egui's
//! memory, and the Viewport's corner handles read it too
//! ([`proportions_free`]).

use dc_app::properties::{self, Edit, Field, Placement};
use dc_app::units;
use dc_app::viewport::tabs::TabEdit;
use dc_app::{CommandError, StrKey};
use dc_project::{InstanceId, TabId, TabShape};
use eframe::egui::{self, Id, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented;
use crate::widgets::unit_field::{LABEL_PX, UnitField, field as unit_field, row};

/// Where the W/H padlock's state lives.
fn padlock_id() -> Id {
    Id::unique("placement_proportions_free")
}

/// Whether the W/H padlock is open, so W and H (and a corner drag) scale
/// each axis on its own.
pub(crate) fn proportions_free(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(padlock_id()))
        .unwrap_or(false)
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

/// Runs what `edit` makes, or says why not; an unchanged value is no step.
fn push_edit(state: &FrameState<'_>, ids: &[InstanceId], edit: Edit, out: &mut Vec<Action>) {
    match properties::edit(state.app.project(), ids, edit) {
        Ok(command) => out.push(Action::Run(Box::new(command))),
        Err(CommandError::NothingToDo) => {}
        Err(e) => out.push(Action::Refused(e.to_string())),
    }
}

/// One placement field's row.
fn field_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    placement: &Placement,
    ids: &[InstanceId],
    field: Field,
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    let (key, name) = match field {
        Field::X => (StrKey::PlacementX, "x"),
        Field::Y => (StrKey::PlacementY, "y"),
        Field::W => (StrKey::PlacementW, "w"),
        Field::H => (StrKey::PlacementH, "h"),
        Field::Rotation => (StrKey::PlacementRotation, "rotation"),
    };
    let value = properties::field_text(placement, field, units);
    let hint = if value.is_empty() {
        strings.get(StrKey::PlacementMixed)
    } else {
        ""
    };
    let id = Id::unique(("placement_field", name));
    let name = strings.get(key);
    let (label, output) = row(ui, name, |ui| {
        unit_field(
            ui,
            UnitField {
                placeholder: hint,
                ..UnitField::new(id, name, &value, properties::field_unit(field, units))
            },
        )
    });
    let _ = output.response.labelled_by(label.id);
    if let Some(typed) = output.committed {
        let uniform = !proportions_free(ui.ctx());
        match properties::parse(placement, field, &typed, units, uniform) {
            Ok(edit) => push_edit(state, ids, edit, out),
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
    ui.add_space(4.0);
}

/// The padlock between W and H: closed keeps proportions.
fn padlock(ui: &mut Ui, state: &FrameState<'_>) {
    let strings = state.app.strings();
    let free = proportions_free(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(LABEL_PX + 8.0);
        let (icon, tip) = if free {
            (
                Icon::LockOpen,
                strings.get(StrKey::PlacementProportionsFree),
            )
        } else {
            (Icon::Lock, strings.get(StrKey::PlacementProportionsKept))
        };
        let spec = ButtonSpec::new(Kind::Quiet, "")
            .icon(icon, theme::ICON_SMALL_PX)
            .tooltip(tip);
        if button(ui, spec).clicked() {
            ui.data_mut(|d| d.insert_temp(padlock_id(), !free));
        }
    });
    ui.add_space(4.0);
}

/// A checkbox row for a flag, *Mixed* beside it when the shapes differ;
/// the new value when clicked.
fn flag_row(ui: &mut Ui, label: &str, value: Option<bool>, mixed: &str) -> Option<bool> {
    let palette = Palette::current(ui);
    let mut checked = value.unwrap_or(false);
    let changed = ui
        .horizontal(|ui| {
            ui.add_space(LABEL_PX + 8.0);
            let response = checkbox(ui, label, &mut checked, label);
            if value.is_none() {
                text(ui, mixed, TextStyle::Small, palette.text_secondary);
            }
            response.clicked()
        })
        .inner;
    changed.then_some(match value {
        // A mixed flag set by a click: every shape takes it.
        None => true,
        Some(v) => !v,
    })
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
            field_row(ui, state, &placement, ids, Field::X, out);
            field_row(ui, state, &placement, ids, Field::Y, out);
            field_row(ui, state, &placement, ids, Field::W, out);
            padlock(ui, state);
            field_row(ui, state, &placement, ids, Field::H, out);
            field_row(ui, state, &placement, ids, Field::Rotation, out);

            ui.horizontal(|ui| {
                let (rect, label) =
                    ui.allocate_exact_size(egui::vec2(LABEL_PX, 26.0), Sense::hover());
                let mirror = strings.get(StrKey::PlacementMirror);
                label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, mirror));
                ui.painter().text(
                    egui::pos2(rect.left(), rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    mirror,
                    TextStyle::Body.resolve(ui.style()),
                    palette.text_secondary,
                );
                for (icon, key, edit) in [
                    (
                        Icon::MirrorLeftRight,
                        StrKey::PlacementMirrorHorizontal,
                        Edit::MirrorHorizontal,
                    ),
                    (
                        Icon::MirrorTopBottom,
                        StrKey::PlacementMirrorVertical,
                        Edit::MirrorVertical,
                    ),
                ] {
                    let spec = ButtonSpec::new(Kind::Secondary, "")
                        .icon(icon, theme::ICON_SMALL_PX)
                        .tooltip(strings.get(key));
                    if button(ui, spec).clicked() {
                        push_edit(state, ids, edit, out);
                    }
                }
            });
            ui.add_space(8.0);

            let mixed = strings.get(StrKey::PlacementMixed);
            if let Some(visible) = flag_row(
                ui,
                strings.get(StrKey::PlacementVisible),
                placement.visible,
                mixed,
            ) {
                out.push(Action::Run(Box::new(dc_app::Command::SetInstanceFlags {
                    ids: ids.to_vec(),
                    visible: Some(visible),
                    locked: None,
                })));
            }
            if let Some(locked) = flag_row(
                ui,
                strings.get(StrKey::PlacementLocked),
                placement.locked,
                mixed,
            ) {
                out.push(Action::Run(Box::new(dc_app::Command::SetInstanceFlags {
                    ids: ids.to_vec(),
                    visible: None,
                    locked: Some(locked),
                })));
            }
            ui.add_space(8.0);
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
