// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Properties panel with instances selected (functional spec,
//! capability 58; mock-up `01-workspace.svg`, Properties;
//! `experience.md`, section 7): X and Y (the selection box's bottom-left),
//! W and H with the proportion link to their right (one chain spanning
//! both rows, linked or broken), and the operations targeting the
//! selection. Nothing else: rotation and mirroring are the Inspector's
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
//! With pictures selected (capabilities 56 and 58) it shows the one
//! picture's name, the selection's X and Y, W and H with the link always
//! closed (a picture is never stretched: [`closed_link`]), its size in
//! pixels, read-only, its lock and the Photo V-Carves that carve it
//! ([`pictures()`]). The values are `dc_app::pictures`'; X or Y moves the
//! selection and W or H resizes the one picture about its centre, each
//! one undo step.
//!
//! With the one shape selected keeping its sizes (phase 5, capability 84)
//! a **Shape** section stands above the placement fields
//! ([`shape_section`]): the sizes it was drawn with, as it is placed, and
//! its choices. A typed size or a choice is one `SetOutline`
//! (`dc_app::shape`), and a size the shape cannot have is refused in the
//! status bar.
//!
//! The link is view state, not project state: it lives in egui's memory,
//! and the Viewport's corner handles read it too ([`proportions_free`]).

use dc_app::pictures::{self, PictureProperties};
use dc_app::properties::{self, Field, Placement};
use dc_app::units;
use dc_app::viewport::tabs::TabEdit;
use dc_app::{CommandError, StrKey};
use dc_project::{BitmapId, InstanceId, TabId, TabShape};
use eframe::egui::{self, Id, Rect, Stroke, TextStyle, Ui, UiBuilder};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented;
use crate::widgets::toggle::toggle;
use crate::widgets::unit_field::{UnitField, field as unit_field, row};

/// The Inspector's content margin under its header: the card padding at
/// each side and none above, so the first row starts where a tool's and
/// an operation's do.
const INSPECTOR_MARGIN: egui::Margin = egui::Margin {
    left: 16,
    right: 16,
    top: 0,
    bottom: 8,
};

/// The proportion link's column right of the fields, px
/// (`experience.md`, section 7, "Proportion link").
const LINK_COLUMN_PX: f32 = 28.0;
/// The link's toggle, px square.
const LINK_TOGGLE_PX: f32 = 28.0;
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
    let (label, output) = row(ui, name, strings.get(field.help()), |ui| {
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
    painter.rect_filled(
        toggle.shrink(2.0),
        theme::CONTROL_RADIUS_PX,
        palette.surface_panel,
    );
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
        .height(LINK_TOGGLE_PX)
        .tooltip(strings.get(StrKey::PlacementKeepProportions))
        .enabled(enabled);
    // A child of its own, placed over the rows: it takes no room in the
    // column, so what follows the H row starts under it.
    let mut over = ui.new_child(UiBuilder::new().max_rect(toggle));
    let clicked = button(&mut over, spec).clicked();
    let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    ui.painter()
        .galley(toggle.center() - glyph.size() / 2.0, glyph, colour);
    if clicked {
        ui.data_mut(|d| d.insert_temp(link_id(), !free));
    }
}

/// The proportion link where it is not a choice (a picture's W and H, the
/// Add picture sheet's width and height): the bracket from the middle of
/// one row (`w_y`) to the middle of the other (`h_y`) with the `link`
/// glyph on it, closed, and `tip` on hover saying the other side follows.
/// Drawn in the 28-px column at the right of `ui`, over the rows; it
/// takes no room.
pub(crate) fn closed_link(ui: &mut Ui, (w_y, h_y): (f32, f32), tip: &str, enabled: bool) {
    let palette = Palette::current(ui);
    let x = ui.max_rect().right() - LINK_COLUMN_PX / 2.0;
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
        egui::pos2(x, f32::midpoint(w_y, h_y)),
        egui::vec2(LINK_TOGGLE_PX, LINK_TOGGLE_PX),
    );
    painter.rect_filled(
        toggle.shrink(2.0),
        theme::CONTROL_RADIUS_PX,
        ui.visuals().panel_fill,
    );
    let colour = if enabled {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    let glyph = Icon::Link.galley(ui, theme::ICON_SMALL_PX, colour);
    ui.painter()
        .galley(toggle.center() - glyph.size() / 2.0, glyph, colour);
    ui.interact(
        toggle,
        ui.scope_id().with("closed_proportion_link"),
        egui::Sense::hover(),
    )
    .on_hover_text(tip);
}

/// The locked note (capability 58): the padlock and the line saying what
/// a lock stops, with `unlock`'s button under them.
fn locked_lines(ui: &mut Ui, state: &FrameState<'_>, unlock: impl FnOnce(&mut Ui)) {
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
            unlock(ui);
        });
    });
    ui.add_space(8.0);
}

/// The locked note of shapes `ids`: *Unlock* is one step unlocking the
/// selection's locked shapes.
fn locked_note(ui: &mut Ui, state: &FrameState<'_>, ids: &[InstanceId], out: &mut Vec<Action>) {
    let strings = state.app.strings();
    locked_lines(ui, state, |ui| {
        let unlock = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::PlacementUnlock));
        if button(ui, unlock).clicked()
            && let Some(command) = properties::unlock(state.app.project(), ids)
        {
            out.push(Action::run(command));
        }
    });
}

/// The panel's title for pictures `ids`: the one picture's name, or the
/// count.
pub(crate) fn pictures_title(state: &FrameState<'_>, ids: &[BitmapId]) -> String {
    pictures::properties(state.app.project(), ids)
        .map(|properties| pictures::title(state.app.strings(), &properties))
        .unwrap_or_default()
}

/// One of a picture selection's placement rows: X, Y, W or H, read-only
/// as [`pictures::read_only`] says; the row's vertical middle.
fn picture_field_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    shown: &PictureProperties,
    ids: &[BitmapId],
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
    let placement = &shown.placement;
    let value = properties::field_text(placement, field, units);
    let read_only = pictures::read_only(shown, field);
    // Why it cannot be typed: the lock, or that a size is one picture's.
    let reason = if placement.any_locked {
        StrKey::PlacementLockedNote
    } else {
        StrKey::PlacementPictureOneSize
    };
    let id = Id::unique(("picture_field", name));
    let name = strings.get(key);
    let (label, output) = row(ui, name, strings.get(field.picture_help()), |ui| {
        ui.set_max_width((ui.available_width() - LINK_GAP_PX - LINK_COLUMN_PX).max(0.0));
        unit_field(
            ui,
            UnitField {
                enabled: !read_only,
                disabled_reason: read_only.then(|| strings.get(reason)),
                ..UnitField::new(id, name, &value, properties::field_unit(field, units))
            },
        )
    });
    let middle = label.rect.center().y;
    let _ = output.response.labelled_by(label.id);
    if let Some(typed) = output.committed {
        // A picture keeps its proportions whatever the link shows.
        match properties::parse(placement, field, &typed, units, true)
            .and_then(|edit| pictures::edit(state.app.project(), ids, edit))
        {
            Ok(command) => out.push(Action::Run(Box::new(command))),
            Err(CommandError::NothingToDo) => {}
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
    middle
}

/// A text row of a picture's Properties: `label`, then a field holding
/// `value`; what was typed, when `editable` and left changed.
fn picture_text_row(
    ui: &mut Ui,
    (label, name, help): (&str, &str, &str),
    value: &str,
    editable: bool,
) -> Option<String> {
    let (label_response, output) = row(ui, label, help, |ui| {
        ui.set_max_width((ui.available_width() - LINK_GAP_PX - LINK_COLUMN_PX).max(0.0));
        unit_field(
            ui,
            UnitField {
                enabled: editable,
                ..UnitField::new(Id::unique(("picture_field", name)), label, value, "")
            },
        )
    });
    let _ = output.response.labelled_by(label_response.id);
    output.committed
}

/// What a picture traced over needs (phase 5, capability 101): the
/// pictures' **Opacity**, a percentage, which a lock does not stop; and,
/// for one picture, **Set scale…**, which arms the Viewport's mode that
/// scales it by two points (`dc_app::tool::Adjust::PictureScale`).
fn backdrop_rows(
    ui: &mut Ui,
    state: &FrameState<'_>,
    shown: &PictureProperties,
    ids: &[BitmapId],
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let label = strings.get(StrKey::PlacementPictureOpacity);
    let help = strings.get(StrKey::PlacementPictureOpacityHelp);
    let value = pictures::opacity_text(shown);
    let (label_response, output) = row(ui, label, help, |ui| {
        ui.set_max_width((ui.available_width() - LINK_GAP_PX - LINK_COLUMN_PX).max(0.0));
        let id = Id::unique(("picture_field", "opacity"));
        unit_field(ui, UnitField::new(id, label, &value, "%"))
    });
    let _ = output.response.labelled_by(label_response.id);
    if let Some(typed) = output.committed {
        match pictures::opacity(state.app.project(), strings, ids, &typed) {
            Ok(command) => out.push(Action::run(command)),
            Err(CommandError::NothingToDo) => {}
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
    if shown.placement.count == 1 {
        let name = strings.get(StrKey::PlacementPictureSetScale);
        let locked = shown.placement.any_locked;
        let help = if locked {
            strings.get(StrKey::PlacementLockedNote)
        } else {
            strings.get(StrKey::PlacementPictureSetScaleHelp)
        };
        ui.add_space(4.0);
        let spec = ButtonSpec::new(Kind::Secondary, name)
            .tooltip(help)
            .enabled(!locked);
        if button(ui, spec).clicked() {
            out.push(Action::Arm(dc_app::Tool::Adjust(
                dc_app::tool::Adjust::PictureScale,
            )));
        }
        ui.add_space(4.0);
    }
}

/// Draws the Properties of the pictures `ids` (capabilities 56 and 58):
/// the one picture's name; X and Y; W and H with the link closed; its
/// pixels, read-only; its opacity and *Set scale…* ([`backdrop_rows`]);
/// its lock; and the Photo V-Carves that carve it.
pub(crate) fn pictures(
    ui: &mut Ui,
    state: &FrameState<'_>,
    ids: &[BitmapId],
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let project = state.app.project();
    let palette = Palette::current(ui);
    let Some(shown) = pictures::properties(project, ids) else {
        return;
    };
    egui::Frame::NONE
        .inner_margin(INSPECTOR_MARGIN)
        .show(ui, |ui| {
            if let (Some(name), [id]) = (&shown.name, ids) {
                let label = strings.get(StrKey::PlacementPictureName);
                let help = strings.get(StrKey::PlacementPictureNameHelp);
                if let Some(typed) = picture_text_row(ui, (label, "name", help), name, true)
                    && let Some(command) = dc_app::tree::rename_bitmap(project, id, &typed)
                {
                    out.push(Action::run(command));
                }
            }
            let mut middles = [0.0; 4];
            for (field, middle) in properties::FIELDS.iter().zip(&mut middles) {
                *middle = picture_field_row(ui, state, &shown, ids, *field, out);
            }
            let [_, _, w_y, h_y] = middles;
            closed_link(
                ui,
                (w_y, h_y),
                strings.get(StrKey::PlacementPictureProportions),
                !pictures::read_only(&shown, Field::W),
            );
            if let Some(pixels) = pictures::pixels_text(strings, &shown) {
                let label = strings.get(pictures::PIXELS_LABEL);
                let help = strings.get(StrKey::PlacementPicturePixelsHelp);
                picture_text_row(ui, (label, "pixels", help), &pixels, false);
            }
            backdrop_rows(ui, state, &shown, ids, out);
            // The lock: a switch, and what it stops while it is on.
            let locked = shown.placement.locked == Some(true);
            let lock_label = strings.get(StrKey::PlacementPictureLocked);
            let lock_help = strings.get(StrKey::PlacementPictureLockedHelp);
            let (label_response, switched) = row(ui, lock_label, lock_help, |ui| {
                let switch = toggle(ui, locked, lock_label);
                crate::widgets::help::control(ui, switch, lock_label)
            });
            let switched = switched.labelled_by(label_response.id);
            if switched.clicked()
                && let Some(command) = dc_app::tree::toggle_bitmap_locked(project, ids)
            {
                out.push(Action::run(command));
            }
            if shown.placement.any_locked {
                locked_lines(ui, state, |ui| {
                    let unlock =
                        ButtonSpec::new(Kind::Secondary, strings.get(StrKey::PlacementUnlock));
                    if button(ui, unlock).clicked()
                        && let Some(command) = pictures::unlock(project, ids)
                    {
                        out.push(Action::run(command));
                    }
                });
            }
            ui.add_space(4.0);
            let carved = if shown.placement.targeted_by.is_empty() {
                strings.get(StrKey::PlacementPictureNotCarved).to_owned()
            } else {
                strings.fmt(
                    StrKey::PlacementPictureCarvedBy,
                    &[("names", &shown.placement.targeted_by.join(", "))],
                )
            };
            text(ui, &carved, TextStyle::Small, palette.text_secondary);
        });
}

/// The unit a size of `quantity` shows in its field.
const fn size_unit(quantity: dc_app::tool::Quantity, units: dc_project::Units) -> &'static str {
    match quantity {
        dc_app::tool::Quantity::Length => units::suffix(units),
        dc_app::tool::Quantity::Angle => "°",
        dc_app::tool::Quantity::Count => "",
        dc_app::tool::Quantity::Percent => "%",
    }
}

/// The Shape section of the one selected shape `id` (capability 84): its
/// kind as a heading, a row per size, a segmented control per choice,
/// and the note under them. Nothing for a shape that keeps no sizes.
fn shape_section(ui: &mut Ui, state: &FrameState<'_>, id: &InstanceId, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let palette = Palette::current(ui);
    // A text's sizes need the fonts on hand (capability 91).
    let Some(section) = dc_app::shape::section(project, strings, id)
        .or_else(|| dc_app::text::section(project, strings, app.fonts(), id))
    else {
        return;
    };
    let is_text = section.text.is_some();
    let units = project.display_units;
    text(
        ui,
        &section.title,
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(4.0);
    let push = |result: Result<dc_app::Command, CommandError>, out: &mut Vec<Action>| match result {
        Ok(command) => out.push(Action::Run(Box::new(command))),
        Err(CommandError::NothingToDo) => {}
        Err(e) => out.push(Action::Refused(e.to_string())),
    };
    if let Some(words) = &section.text {
        words_field(ui, id, words, section.read_only, out);
    }
    for choice in &section.choices {
        // A text's font: a list that drops down, with *Add font…*. The
        // list stays open to a text whose font is not on this computer,
        // since choosing another is how it is made a text again.
        if choice.list {
            use crate::viewport::tool_bar::{ListPick, list_choice};
            let label = crate::widgets::row::inline_label(ui, &choice.label, &choice.help);
            let (picked, response) = list_choice(ui, strings, choice, ui.available_width());
            let _ = response.labelled_by(label.id);
            ui.add_space(4.0);
            match picked {
                Some(ListPick::Option(dc_app::tool::ToolOption::Font(font))) => {
                    out.push(Action::EditText(dc_app::text::TextEdit::Font(font)));
                }
                Some(ListPick::AddFont) => out.push(Action::AddFont),
                _ => {}
            }
            continue;
        }
        let labels: Vec<&str> = choice.options.iter().map(|o| o.label.as_str()).collect();
        // The label over the control, which takes the panel's width: three
        // words as long as *Chamfered* do not fit beside a label.
        let label = crate::widgets::row::inline_label(ui, &choice.label, &choice.help);
        let shown = ui.scope(|ui| {
            if section.read_only {
                ui.disable();
            }
            segmented(
                ui,
                Id::unique(("shape_choice", choice.label.as_str())),
                &labels,
                choice.selected,
                ui.available_width(),
            )
        });
        let picked = shown.inner;
        let _ =
            crate::widgets::help::control(ui, shown.response, &choice.label).labelled_by(label.id);
        ui.add_space(4.0);
        if let Some(option) = picked.and_then(|i| choice.options.get(i)) {
            if let dc_app::tool::ToolOption::TextAlign(align) = option.option {
                out.push(Action::EditText(dc_app::text::TextEdit::Align(align)));
            } else {
                push(
                    dc_app::shape::set_option(project, strings, id, &option.option),
                    out,
                );
            }
        }
    }
    // Why the sizes cannot be typed: the shape is stretched, which the
    // note says, or it, or a linked copy of it, is locked.
    let stretched = project
        .instances
        .iter()
        .find(|instance| &instance.id == id)
        .is_some_and(|instance| dc_app::shape::even_scale(instance.transform).is_none());
    let read_only_reason = match &section.note {
        Some(note) if stretched => note.as_str(),
        _ => strings.get(StrKey::PlacementLockedNote),
    };
    for size in &section.rows {
        let field_id = Id::unique(("shape_field", size.field));
        let (label, output) = row(ui, &size.label, &size.help, |ui| {
            unit_field(
                ui,
                UnitField {
                    enabled: !section.read_only,
                    disabled_reason: section.read_only.then_some(read_only_reason),
                    ..UnitField::new(
                        field_id,
                        &size.label,
                        &size.text,
                        size_unit(size.quantity, units),
                    )
                },
            )
        });
        let _ = output.response.labelled_by(label.id);
        if let Some(typed) = output.committed {
            if is_text {
                out.push(Action::EditText(dc_app::text::TextEdit::Size(
                    size.field, typed,
                )));
            } else {
                push(
                    dc_app::shape::set_size(project, strings, id, size.field, &typed),
                    out,
                );
            }
        }
    }
    if let Some(note) = &section.note {
        if is_text {
            // A sentence, which wraps: why the text cannot be edited.
            crate::widgets::row::line(ui, false, note);
        } else {
            text(ui, note, TextStyle::Small, palette.text_secondary);
        }
    }
    ui.add_space(8.0);
    let (rule, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        Stroke::new(1.0, palette.border_default),
    );
    ui.add_space(8.0);
}

/// A text's words in Properties (capability 91): a field of several
/// lines. What is typed is kept beside the field until the field is
/// left, or `Ctrl`/`⌘ Enter` is pressed, and then written as one undo
/// step; `Escape` puts the words back.
fn words_field(
    ui: &mut Ui,
    id: &InstanceId,
    words: &dc_app::tool::TextRow,
    read_only: bool,
    out: &mut Vec<Action>,
) {
    let field_id = Id::unique(("shape_words", id.as_str()));
    let draft_id = field_id.with("draft");
    let label = crate::widgets::row::inline_label(ui, &words.label, &words.help);
    let mut text: String = ui
        .data(|d| d.get_temp(draft_id))
        .unwrap_or_else(|| words.text.clone());
    let focused = ui.memory(|m| m.has_focus(field_id));
    let commit_key =
        focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
    let reverted = focused && ui.input(|i| i.key_pressed(egui::Key::Escape));
    let edit = ui.add_enabled(
        !read_only,
        egui::TextEdit::multiline(&mut text)
            .id(field_id)
            .font(TextStyle::Body)
            .desired_width(ui.available_width())
            .desired_rows(2),
    );
    let edit = edit.accessible_name(&words.label).labelled_by(label.id);
    ui.add_space(4.0);
    if reverted {
        ui.data_mut(|d| d.remove::<String>(draft_id));
    } else if edit.lost_focus() || commit_key {
        ui.data_mut(|d| d.remove::<String>(draft_id));
        if text != words.text {
            out.push(Action::EditText(dc_app::text::TextEdit::Text(text)));
        }
        if commit_key {
            edit.surrender_focus();
        }
    } else if edit.has_focus() || edit.changed() {
        ui.data_mut(|d| d.insert_temp(draft_id, text));
    } else {
        // Not being typed in: it shows what the text holds.
        ui.data_mut(|d| d.remove::<String>(draft_id));
    }
}

/// The **Construction** switch for the selected shapes `ids` (phase 5,
/// capability 100): on while every one is construction geometry; turned
/// on, the shapes leave every operation's targets.
fn construction_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    ids: &[InstanceId],
    out: &mut Vec<Action>,
) {
    let app = state.app;
    let strings = app.strings();
    let on = !ids.is_empty()
        && app
            .project()
            .instances
            .iter()
            .filter(|instance| ids.contains(&instance.id))
            .all(|instance| instance.construction);
    let name = strings.get(StrKey::PlacementConstruction);
    let (label, switch) = row(
        ui,
        name,
        strings.get(StrKey::PlacementConstructionHelp),
        |ui| toggle(ui, on, name),
    );
    let switch = switch.labelled_by(label.id);
    if switch.clicked() {
        out.push(Action::run(dc_app::Command::SetConstruction {
            ids: ids.to_vec(),
            construction: !on,
        }));
    }
}

/// A button of the Align section: an icon, 28 px square.
const ALIGN_BUTTON_PX: f32 = 28.0;

/// **Align** and **Distribute** for the selected shapes `ids` (phase 5,
/// capability 98a), under the placement fields: six buttons that align
/// the shapes by a side or a middle of their boxes, the *To stock*
/// switch with two shapes or more (one shape always aligns to the
/// stock), and, with three or more, four that space them evenly. Each
/// label is over its buttons, which take the panel's width.
fn align_section(ui: &mut Ui, state: &FrameState<'_>, ids: &[InstanceId], out: &mut Vec<Action>) {
    use dc_app::arrange::{Action as Arrange, Align, Axis, Spacing};
    let app = state.app;
    let strings = app.strings();
    let disabled = dc_app::arrange::disabled(app.project(), ids, 0);
    let reason = disabled.map(|why| strings.get(why.reason()));
    let to_stock = app.prefs().adjust.align_to_stock;
    let icon_button = |ui: &mut Ui, icon: Icon, key: StrKey| {
        let name = strings.get(key);
        let spec = ButtonSpec::new(Kind::Quiet, "")
            .icon(icon, theme::ICON_LARGE_PX)
            .height(ALIGN_BUTTON_PX)
            .width(ALIGN_BUTTON_PX)
            .named(name)
            .tooltip(reason.unwrap_or(name))
            .enabled(disabled.is_none());
        button(ui, spec).clicked()
    };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        crate::widgets::row::inline_label(
            ui,
            strings.get(StrKey::ArrangeAlign),
            strings.get(StrKey::ArrangeAlignHelp),
        );
        if ids.len() > 1 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = strings.get(StrKey::ArrangeToStock);
                let switch = toggle(ui, to_stock, name)
                    .on_hover_text(strings.get(StrKey::ArrangeToStockHelp));
                if switch.clicked() {
                    out.push(Action::SetAlignToStock(!to_stock));
                }
                crate::widgets::row::inline_label(
                    ui,
                    name,
                    strings.get(StrKey::ArrangeToStockHelp),
                );
            });
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (icon, key, side) in [
            (
                Icon::AlignStartVertical,
                StrKey::ArrangeAlignLeft,
                Align::Left,
            ),
            (
                Icon::AlignCenterVertical,
                StrKey::ArrangeAlignCentre,
                Align::Centre,
            ),
            (
                Icon::AlignEndVertical,
                StrKey::ArrangeAlignRight,
                Align::Right,
            ),
            (
                Icon::AlignStartHorizontal,
                StrKey::ArrangeAlignTop,
                Align::Top,
            ),
            (
                Icon::AlignCenterHorizontal,
                StrKey::ArrangeAlignMiddle,
                Align::Middle,
            ),
            (
                Icon::AlignEndHorizontal,
                StrKey::ArrangeAlignBottom,
                Align::Bottom,
            ),
        ] {
            if side == Align::Top {
                ui.add_space(8.0);
            }
            if icon_button(ui, icon, key) {
                out.push(Action::Arrange(Arrange::Align { side, to_stock }));
            }
        }
    });
    if ids.len() < 3 {
        return;
    }
    ui.add_space(4.0);
    crate::widgets::row::inline_label(
        ui,
        strings.get(StrKey::ArrangeDistribute),
        strings.get(StrKey::ArrangeDistributeHelp),
    );
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (icon, key, axis, spacing) in [
            (
                Icon::AlignHorizontalSpaceBetween,
                StrKey::ArrangeDistributeHorizontal,
                Axis::Horizontal,
                Spacing::Gaps,
            ),
            (
                Icon::AlignVerticalSpaceBetween,
                StrKey::ArrangeDistributeVertical,
                Axis::Vertical,
                Spacing::Gaps,
            ),
            (
                Icon::AlignHorizontalDistributeCenter,
                StrKey::ArrangeDistributeHorizontalCentres,
                Axis::Horizontal,
                Spacing::Centres,
            ),
            (
                Icon::AlignVerticalDistributeCenter,
                StrKey::ArrangeDistributeVerticalCentres,
                Axis::Vertical,
                Spacing::Centres,
            ),
        ] {
            if (axis, spacing) == (Axis::Horizontal, Spacing::Centres) {
                ui.add_space(8.0);
            }
            if icon_button(ui, icon, key) {
                out.push(Action::Arrange(Arrange::Distribute { axis, spacing }));
            }
        }
    });
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
        .inner_margin(INSPECTOR_MARGIN)
        .show(ui, |ui| {
            // The sizes the one shape was drawn with, above where it is.
            if let [id] = ids {
                shape_section(ui, state, id, out);
            }
            let mut middles = [0.0; 4];
            for (field, middle) in properties::fields(&placement).iter().zip(&mut middles) {
                *middle = field_row(ui, state, &placement, ids, *field, out);
            }
            let [_, _, w_y, h_y] = middles;
            proportion_link(ui, state, (w_y, h_y), !properties::read_only(&placement));
            if properties::read_only(&placement) {
                locked_note(ui, state, ids, out);
            }
            construction_row(ui, state, ids, out);
            align_section(ui, state, ids, out);
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
    (key, help, name): (StrKey, StrKey, &str),
    value_mm: Option<f64>,
    edit: fn(f64) -> TabEdit,
    out: &mut Vec<Action>,
) {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    let value = value_mm.map_or_else(String::new, |v| units::field_text(v, units));
    let label = strings.get(key);
    let (label_response, output) = row(ui, label, strings.get(help), |ui| {
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
        .inner_margin(INSPECTOR_MARGIN)
        .show(ui, |ui| {
            tab_length_row(
                ui,
                state,
                ids,
                (
                    StrKey::PlacementTabWidth,
                    StrKey::PlacementTabWidthHelp,
                    "width",
                ),
                values.width_mm,
                TabEdit::Width,
                out,
            );
            tab_length_row(
                ui,
                state,
                ids,
                (
                    StrKey::PlacementTabHeight,
                    StrKey::PlacementTabHeightHelp,
                    "height",
                ),
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
            let shape_label = strings.get(StrKey::PlacementTabShape);
            let shape_help = strings.get(StrKey::PlacementTabShapeHelp);
            let (_, picked) = row(ui, shape_label, shape_help, |ui| {
                let shown = ui.scope(|ui| {
                    segmented(
                        ui,
                        Id::unique("tab_field_shape"),
                        &labels,
                        current,
                        ui.available_width(),
                    )
                });
                let _ = crate::widgets::help::control(ui, shown.response, shape_label);
                shown.inner
            });
            if let Some((shape, _)) = picked.and_then(|i| shapes.get(i)) {
                push_tab_edit(state, ids, TabEdit::Shape(*shape), out);
            }

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
                    units::summary_text(at.x, units),
                    units::summary_text(at.y, units)
                );
                let name = strings.get(StrKey::PlacementTabPosition);
                let help = strings.get(StrKey::PlacementTabPositionHelp);
                let (label_response, output) = row(ui, name, help, |ui| {
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
            let spec = ButtonSpec::new(Kind::Destructive, strings.get(StrKey::PlacementTabDelete))
                .icon(Icon::Trash2, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::PlacementTabDeleteHelp));
            if button(ui, spec).clicked() {
                out.push(Action::run(dc_app::Command::DeleteTabs(ids.to_vec())));
            }
        });
}
