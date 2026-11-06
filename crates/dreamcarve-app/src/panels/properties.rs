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
//! The padlock is view state, not project state: it lives in egui's
//! memory, and the Viewport's corner handles read it too
//! ([`proportions_free`]).

use dc_app::properties::{self, Edit, Field, Placement};
use dc_app::{CommandError, StrKey};
use dc_project::InstanceId;
use eframe::egui::{self, Id, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
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
            let response = ui.checkbox(&mut checked, label);
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
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
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
                        label.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Label, true, mirror)
                        });
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
        });
}

/// The sentence the panel shows with placed tabs selected (their size and
/// shape are slice 17's).
pub(crate) fn tabs_sentence(state: &FrameState<'_>, count: usize) -> String {
    state.app.strings().fmt(
        StrKey::PlacementTabsSelected,
        &[("count", &count.to_string())],
    )
}
