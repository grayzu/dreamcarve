// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The CAM panel's Tools tab (functional spec, capability 63; mock-ups
//! `01-workspace.svg` and `16-tools-tab.svg`; `experience.md`, section 7):
//! *New tool* and *Add from library…* over the project's tools, one 40-px
//! row each with its type glyph, name, diameter (and angle for a V-bit)
//! and number, a red dot when it lacks a feed, plunge rate or RPM and the
//! `unused_tool` hint when no operation uses it; *Duplicate* and *Delete*
//! under the list; then the selected tool's editor, every field with its
//! unit, and *Save to library* in its footer. Feed, plunge and RPM are
//! empty while the file holds `null`, never prefilled, and show the
//! required-field look with the file's message as the tooltip (S20; 66b).
//! Switching a tool to V-bit writes no default angle: the switch is held,
//! with an empty, required *Angle* field, until an angle is typed (#581).
//! *Delete* is disabled while an operation uses the tool, its tooltip
//! naming them. *New tool* (slice 29, #621) selects the tool it makes and
//! focuses its name with the text selected, so typing renames it. An
//! empty list says *No tools yet* with *New tool* and *Add from library…*
//! under it (capability 79).
//!
//! The tab holds no project data: it reads the snapshot and emits
//! commands. Which row is selected, a held switch and a name to focus are
//! view state, kept in egui's memory.

use dc_app::tools::{self, KindChoice, KindSwitch, ToolField};
use dc_app::{Command, CommandError, StrKey, Strings};
use dc_project::{Project, Tool, ToolId, ToolKind, Units};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use crate::panels::library::kind_label;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::unit_field::{Required, UnitField, field as unit_field, row, validation_line};

/// A list row's height, px (section 3).
const ROW_PX: f32 = 40.0;
/// The status cell's width, px.
const STATUS_PX: f32 = 24.0;
/// The editor footer's height, px: a 26-px button between 8-px gaps.
const FOOTER_PX: f32 = 42.0;
/// The panel's content margin, px.
const MARGIN_PX: f32 = 12.0;

/// The selected row, and how many tools there were, between frames.
#[derive(Debug, Clone, Default, PartialEq)]
struct Selected {
    tool: Option<ToolId>,
    count: usize,
}

/// The tool the tab shows in its editor: the one selected, the newest
/// when a tool was just added, else the first.
fn selected(ui: &Ui, project: &Project) -> Option<ToolId> {
    let id = Id::unique("tools_tab_selected");
    let mut state: Selected = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let count = project.tools.len();
    if count > state.count && state.count > 0 {
        state.tool = project.tools.last().map(|t| t.id.clone());
    }
    if !state
        .tool
        .as_ref()
        .is_some_and(|t| project.tools.iter().any(|tool| tool.id == *t))
    {
        state.tool = project.tools.first().map(|t| t.id.clone());
    }
    state.count = count;
    let tool = state.tool.clone();
    ui.data_mut(|d| d.insert_temp(id, state));
    tool
}

/// Selects tool `tool`.
fn select(ui: &Ui, tool: &ToolId, count: usize) {
    ui.data_mut(|d| {
        d.insert_temp(
            Id::unique("tools_tab_selected"),
            Selected {
                tool: Some(tool.clone()),
                count,
            },
        );
    });
}

/// The tool whose switch to V-bit is held until its included angle is
/// typed (#581): view state, in egui's memory. The project does not
/// change until then.
fn pending_v_bit_id() -> Id {
    Id::unique("tools_tab_pending_v_bit")
}

/// The tool held for an angle, when there is one.
fn pending_v_bit(ui: &Ui) -> Option<ToolId> {
    ui.data(|d| d.get_temp(pending_v_bit_id()))
}

/// Holds tool `tool`'s switch to V-bit, or (`None`) drops the held one.
fn set_pending_v_bit(ui: &Ui, tool: Option<&ToolId>) {
    ui.data_mut(|d| {
        if let Some(id) = tool {
            d.insert_temp(pending_v_bit_id(), id.clone());
        } else {
            d.remove::<ToolId>(pending_v_bit_id());
        }
    });
}

/// Draws the tab under the CAM panel's tab strip.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let current = selected(ui, project);
    // Moving to another tool discards a held switch.
    if pending_v_bit(ui).is_some_and(|held| current.as_ref() != Some(&held)) {
        set_pending_v_bit(ui, None);
    }
    if project.tools.is_empty() {
        empty(ui, strings, project, out);
        return;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(MARGIN_PX - 8.0);
        add_buttons(ui, strings, project, out);
    });
    ui.add_space(8.0);
    // Six rows show before the list scrolls.
    let rows = u8::try_from(project.tools.len().min(6)).unwrap_or(6);
    let list_h = ROW_PX * f32::from(rows);
    egui::ScrollArea::vertical()
        .id_salt("tools_list")
        .max_height(list_h)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for tool in &project.tools {
                list_row(ui, state, tool, current.as_ref() == Some(&tool.id));
            }
        });
    ui.add_space(8.0);
    list_actions(ui, strings, project, current.as_ref(), out);
    let palette = Palette::current(ui);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        line.x_range(),
        line.center().y,
        Stroke::new(1.0, palette.border_default),
    );
    if let Some(tool) = current
        .as_ref()
        .and_then(|id| project.tools.iter().find(|t| t.id == *id))
    {
        // The editor scrolls above its footer, which stays in view.

        egui::ScrollArea::vertical()
            .id_salt("tool_editor")
            .auto_shrink(false)
            .max_height((ui.available_height() - FOOTER_PX).max(0.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(MARGIN_PX);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - MARGIN_PX);
                        editor(ui, strings, project.display_units, tool, out);
                    });
                });
            });
        footer(ui, strings, tool, out);
    }
}

/// The name field to focus, with its text selected, when the editor next
/// draws that tool: set by *New tool*, view state in egui's memory.
fn focus_name_id() -> Id {
    Id::unique("tools_tab_focus_name")
}

/// *New tool* and *Add from library…*, the ways to add a tool, in that
/// order (capability 63). *New tool* selects the tool it makes and has
/// its name focused when the editor draws it.
fn add_buttons(ui: &mut Ui, strings: &Strings, project: &Project, out: &mut Vec<Action>) {
    let new = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppToolsNewTool))
        .icon(Icon::Plus, theme::ICON_SMALL_PX);
    if button(ui, new).clicked() {
        let command = tools::new_tool(project);
        if let Command::NewTool(tool) = &command {
            select(ui, &tool.id, project.tools.len().saturating_add(1));
            ui.data_mut(|d| d.insert_temp(focus_name_id(), tool.id.clone()));
        }
        out.push(Action::Run(Box::new(command)));
    }
    let add = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppToolsAddFromLibrary));
    if button(ui, add).clicked() {
        out.push(Action::OpenLibrary);
    }
}

/// The editor's footer: *Save to library*, which copies the project tool
/// into the library (capability 63).
fn footer(ui: &mut Ui, strings: &Strings, tool: &Tool, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        line.x_range(),
        line.center().y,
        Stroke::new(1.0, palette.border_default),
    );
    ui.add_space(7.0);
    ui.horizontal(|ui| {
        ui.add_space(MARGIN_PX);
        let save = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppToolsSaveToLibrary))
            .icon(Icon::BookPlus, theme::ICON_SMALL_PX);
        if button(ui, save).clicked() {
            out.push(Action::SaveToLibrary(tool.id.clone()));
        }
    });
}

/// *Duplicate* and *Delete* under the list, acting on the selected tool.
/// *Delete* is disabled while an operation uses it, and says which.
fn list_actions(
    ui: &mut Ui,
    strings: &Strings,
    project: &Project,
    current: Option<&ToolId>,
    out: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.add_space(MARGIN_PX - 8.0);
        let duplicate = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppToolsDuplicate))
            .enabled(current.is_some());
        if button(ui, duplicate).clicked()
            && let Some(id) = current
        {
            out.push(match tools::duplicate(project, id) {
                Ok(command) => Action::Run(Box::new(command)),
                Err(e) => Action::Refused(e.to_string()),
            });
        }
        let users = current
            .map(|id| tools::users(project, id))
            .unwrap_or_default();
        let refusal = current.filter(|_| !users.is_empty()).map(|id| {
            let tool = project
                .tools
                .iter()
                .find(|t| t.id == *id)
                .map_or_else(|| id.to_string(), |t| t.name.clone());
            CommandError::ToolInUse {
                tool,
                operations: users.clone(),
            }
            .to_string()
        });
        let mut delete = ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppToolsDelete))
            .enabled(current.is_some() && refusal.is_none());
        if let Some(reason) = &refusal {
            delete = delete.tooltip(reason);
        }
        if button(ui, delete).clicked()
            && let Some(id) = current
        {
            out.push(Action::Run(Box::new(Command::DeleteTool(id.clone()))));
        }
    });
}

/// The empty list (capability 79; section 8, "Empty states"): *No tools
/// yet*, with *New tool* and *Add from library…* under it.
fn empty(ui: &mut Ui, strings: &Strings, project: &Project, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(strings.get(StrKey::AppPanelNoTools))
                    .text_style(TextStyle::Body)
                    .color(palette.text_secondary),
            )
            .wrap()
            .selectable(false),
        );
    });
    ui.add_space(8.0);
    // The two buttons, centred as a pair.
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let pair = ui.data(|d| d.get_temp::<f32>(Id::unique("tools_tab_empty_pair")));
        ui.add_space(pair.map_or(MARGIN_PX, |w| ((width - w) / 2.0).max(0.0)));
        let start = ui.cursor().left();
        add_buttons(ui, strings, project, out);
        let used = ui.min_rect().right() - start;
        ui.data_mut(|d| d.insert_temp(Id::unique("tools_tab_empty_pair"), used));
    });
}

/// The row's second line: the diameter, a V-bit's angle, the number.
fn detail(strings: &Strings, tool: &Tool, units: Units) -> String {
    let mut parts = vec![format!(
        "{} {}",
        tools::field_text(tool, ToolField::Diameter, units),
        tools::field_unit(ToolField::Diameter, units)
    )];
    if let ToolKind::VBit { .. } = tool.cutter {
        parts.push(format!(
            "{}°",
            tools::field_text(tool, ToolField::Angle, units)
        ));
    }
    if let Some(number) = tool.number {
        parts.push(strings.fmt(StrKey::AppToolsNumber, &[("number", &number.to_string())]));
    }
    parts.join(" · ")
}

/// One 40-px row (section 7, "List row"): the type glyph, the name over
/// its detail, and the status cell.
fn list_row(ui: &mut Ui, state: &FrameState<'_>, tool: &Tool, chosen: bool) {
    let palette = Palette::current(ui);
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, chosen, &tool.name));
    if response.clicked() {
        select(ui, &tool.id, project.tools.len());
    }
    if chosen {
        ui.painter().rect_filled(rect, 0.0, palette.accent_soft);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
            0.0,
            palette.accent,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    }
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + MARGIN_PX + 8.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    super::editor::paint_tool_icon(ui, glyph.center(), &tool.cutter, palette.text_secondary);
    let text_left = glyph.right() + 8.0;
    let text_w = rect.right() - STATUS_PX - 8.0 - text_left;
    let line = |s: &str, style: TextStyle, colour, y: f32| {
        let mut job = egui::text::LayoutJob::simple_singleline(
            s.to_owned(),
            style.resolve(ui.style()),
            colour,
        );
        job.wrap = egui::text::TextWrapping {
            max_width: text_w,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(text_left, y - galley.size().y / 2.0),
            galley,
            colour,
        );
    };
    line(
        &tool.name,
        TextStyle::Body,
        palette.text_primary,
        rect.top() + 13.0,
    );
    line(
        &detail(strings, tool, project.display_units),
        TextStyle::Small,
        palette.text_secondary,
        rect.top() + 28.0,
    );

    // The status cell: the red dot for a missing rate, else the hint.
    let cell = Rect::from_center_size(
        egui::pos2(rect.right() - 8.0 - STATUS_PX / 2.0, rect.center().y),
        egui::vec2(STATUS_PX, STATUS_PX),
    );
    let unset = tools::unset_rates(tool);
    let hint = tools::unused_hint(app.warnings(), &tool.id);
    let messages: Vec<String> = if unset.is_empty() {
        hint.iter().map(|h| h.message.clone()).collect()
    } else {
        unset.iter().map(|(_, m)| m.clone()).collect()
    };
    if !unset.is_empty() {
        ui.painter()
            .circle_filled(cell.center(), 4.0, palette.status_error.stroke);
    } else if hint.is_some() {
        let glyph = Icon::Info.galley(ui, theme::ICON_SMALL_PX, palette.status_info.text);
        ui.painter().galley(
            cell.center() - glyph.size() / 2.0,
            glyph,
            palette.status_info.text,
        );
    }
    if !messages.is_empty() {
        let id = ui.make_persistent_id(("tool_status", tool.id.as_str()));
        ui.interact(cell, id, Sense::hover())
            .on_hover_text(messages.join("\n"));
    }
}

/// The selected project tool's fields (its footer is [`footer`]): the
/// shared rows of [`tool_fields`], each change one command.
fn editor(ui: &mut Ui, strings: &Strings, units: Units, tool: &Tool, out: &mut Vec<Action>) {
    let held = pending_v_bit(ui).as_ref() == Some(&tool.id)
        && !matches!(tool.cutter, ToolKind::VBit { .. });
    let focus_name = ui.data(|d| d.get_temp::<ToolId>(focus_name_id())).as_ref() == Some(&tool.id);
    let output = tool_fields(
        ui,
        &ToolFields {
            salt: Id::unique(("tool_field", tool.id.as_str())),
            strings,
            units,
            tool,
            held,
            focus_name,
            marked: None,
        },
    );
    if focus_name {
        ui.data_mut(|d| d.remove::<ToolId>(focus_name_id()));
    }
    match output.hold {
        Some(true) => set_pending_v_bit(ui, Some(&tool.id)),
        Some(false) => set_pending_v_bit(ui, None),
        None => {}
    }
    match output.edit {
        Some(FieldEdit::Set(next, _)) if next != *tool => {
            out.push(Action::Run(Box::new(Command::UpdateTool(next))));
        }
        Some(FieldEdit::Refused(_, reason)) => out.push(Action::Refused(reason)),
        _ => {}
    }
}

/// What the tool editor's rows show (capability 63; the library's
/// editing pane, 64a, shows the same rows).
pub(crate) struct ToolFields<'a> {
    /// The fields' ids derive from it: one per tool, never a position.
    pub salt: Id,
    pub strings: &'a Strings,
    /// The units lengths and rates show and are typed in.
    pub units: Units,
    pub tool: &'a Tool,
    /// Whether the tool's switch to V-bit is held for its angle (#581).
    pub held: bool,
    /// Whether to focus the name with its text selected this frame (*New
    /// tool*).
    pub focus_name: bool,
    /// A field marked with why its last change did not take, shown under
    /// it in the error state (a library write that failed, 64a).
    pub marked: Option<(ToolField, &'a str)>,
}

/// A change the rows made this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldEdit {
    /// The tool with `ToolField` changed from what was typed.
    Set(Tool, ToolField),
    /// What was typed in the field is not a value of its kind: why.
    Refused(ToolField, String),
}

/// What [`tool_fields`] did this frame.
#[derive(Debug, Default)]
pub(crate) struct FieldsOutput {
    /// The change, when a field was left changed.
    pub edit: Option<FieldEdit>,
    /// `Some(true)` holds the switch to V-bit, `Some(false)` drops it.
    pub hold: Option<bool>,
}

/// The id of field `field` among the rows of `salt`.
fn field_id(salt: Id, field: ToolField) -> Id {
    salt.with(field.file_name())
}

/// The tool editor's rows, in order: name, type (then a V-bit's angle),
/// diameter, flutes, feed, plunge, RPM, step-down, stepover, number,
/// vendor and product id, every field with its unit. Feed, plunge and RPM
/// are empty while unset, never prefilled, with the required-field look
/// (66b). While the tool's switch to V-bit is held, its *Angle* field
/// shows empty, required and in the error state; a typed angle makes the
/// switch, and leaving it empty discards it (#581).
pub(crate) fn tool_fields(ui: &mut Ui, spec: &ToolFields<'_>) -> FieldsOutput {
    let ToolFields {
        salt,
        strings,
        units,
        tool,
        held,
        focus_name,
        marked,
    } = *spec;
    let mut result = FieldsOutput::default();
    let unset = tools::unset_rates(tool);
    let angle_required = strings.get(StrKey::AppToolsAngleRequired);
    let error_for = |field: ToolField| {
        if let Some((marked_field, reason)) = marked
            && marked_field == field
        {
            return Some(reason);
        }
        if held && field == ToolField::Angle {
            return Some(angle_required);
        }
        unset
            .iter()
            .find(|(f, _)| *f == field.file_name())
            .map(|(_, m)| m.as_str())
    };
    let fields: Vec<(ToolField, StrKey)> = [
        (ToolField::Name, StrKey::AppToolsName),
        (ToolField::Angle, StrKey::AppToolsAngle),
        (ToolField::Diameter, StrKey::AppToolsDiameter),
        (ToolField::Flutes, StrKey::AppToolsFlutes),
        (ToolField::Feed, StrKey::AppToolsFeed),
        (ToolField::Plunge, StrKey::AppToolsPlunge),
        (ToolField::Rpm, StrKey::AppToolsRpm),
        (ToolField::StepDown, StrKey::AppToolsStepDown),
        (ToolField::StepOver, StrKey::AppToolsStepOver),
        (ToolField::Number, StrKey::AppToolsToolNumber),
        (ToolField::Vendor, StrKey::AppToolsVendor),
        (ToolField::ProductId, StrKey::AppToolsProductId),
    ]
    .into_iter()
    .filter(|(field, _)| {
        *field != ToolField::Angle || held || matches!(tool.cutter, ToolKind::VBit { .. })
    })
    .collect();
    for (field, key) in fields {
        let value = tools::field_text(tool, field, units);
        let hint = if field == ToolField::Number {
            strings.get(StrKey::AppToolsNumberUnset)
        } else {
            ""
        };
        let id = field_id(salt, field);
        let name = strings.get(key);
        let error = error_for(field);
        let is_marked = marked.is_some_and(|(f, _)| f == field);
        // An unset rate is required (functional 66b; slice 26, #618):
        // *Required* in the field and the file's message its tooltip, not a
        // line under it. The held V-bit's angle keeps its line, which says
        // what to type, and so does a marked field.
        let required = error
            .filter(|_| value.is_empty() && field != ToolField::Angle && !is_marked)
            .map(|message| Required {
                placeholder: strings.get(StrKey::EditorRequired),
                message,
            });
        let (label, output) = row(ui, name, |ui| {
            let output = unit_field(
                ui,
                UnitField {
                    placeholder: hint,
                    tier: error.map(|_| dc_job::Tier::Error),
                    required,
                    ..UnitField::new(id, name, &value, tools::field_unit(field, units))
                },
            );
            if let (Some(message), None) = (error, required) {
                validation_line(ui, message);
            }
            output
        });
        let response = output.response.labelled_by(label.id);
        let left = response.lost_focus();
        if field == ToolField::Name && focus_name {
            focus_with_text_selected(ui, id, &value);
        }
        if required.is_none() {
            response.on_hover_text(field.file_name());
        }
        if held && field == ToolField::Angle {
            // The held switch: a typed angle makes it, one undo step; the
            // field left empty (or Escape) discards it; a refusal keeps it.
            match output
                .committed
                .map(|typed| tools::finish_v_bit(tool, &typed))
            {
                Some(Ok(Some(Command::UpdateTool(next)))) => {
                    result.hold = Some(false);
                    result.edit = Some(FieldEdit::Set(next, field));
                }
                Some(Ok(_)) => result.hold = Some(false),
                Some(Err(e)) => result.edit = Some(FieldEdit::Refused(field, e.to_string())),
                None if left => result.hold = Some(false),
                None => {}
            }
        } else if let Some(typed) = output.committed {
            result.edit = Some(match tools::set_field(tool, field, &typed, units) {
                Ok(next) => FieldEdit::Set(next, field),
                Err(e) => FieldEdit::Refused(field, e.to_string()),
            });
        }
        ui.add_space(4.0);
        if field == ToolField::Name {
            // The type, then (for a V-bit) the angle it enables.
            kind_row(ui, salt, strings, tool, held, &mut result);
        }
    }
    ui.add_space(12.0);
    result
}

/// Focuses field `id` with all of `text` selected, so typing replaces it
/// (*New tool*: typing renames the new tool).
fn focus_with_text_selected(ui: &Ui, id: Id, text: &str) {
    ui.memory_mut(|m| m.request_focus(id));
    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
        state.store(ui.ctx(), id);
    }
}

/// The type dropdown: the five cutters; a V-bit shows its angle field.
/// Choosing *V-bit* for another type holds the switch (`held` from then
/// on) and moves the focus to the empty *Angle* field.
fn kind_row(
    ui: &mut Ui,
    salt: Id,
    strings: &Strings,
    tool: &Tool,
    held: bool,
    result: &mut FieldsOutput,
) {
    let current = if held {
        KindChoice::VBit
    } else {
        KindChoice::of(&tool.cutter)
    };
    let mut chosen = current;
    let (label, combo) = row(ui, strings.get(StrKey::AppToolsKind), |ui| {
        egui::ComboBox::from_id_salt(salt.with("kind"))
            .icon(crate::widgets::icon::combo_chevron)
            .width(ui.available_width())
            .selected_text(kind_label(strings, current))
            .show_ui(ui, |ui| {
                for choice in KindChoice::ALL {
                    ui.selectable_value(&mut chosen, choice, kind_label(strings, choice));
                }
            })
            .response
    });
    combo.labelled_by(label.id);
    if chosen != current {
        result.hold = Some(false);
        match tools::set_kind(tool, chosen) {
            KindSwitch::Unchanged => {}
            KindSwitch::Update(next) => {
                result.edit = Some(FieldEdit::Set(*next, ToolField::Angle));
            }
            KindSwitch::HoldForAngle => {
                result.hold = Some(true);
                ui.memory_mut(|m| m.request_focus(field_id(salt, ToolField::Angle)));
                ui.ctx().request_repaint();
            }
        }
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The second line reads as the spec's rows do: size, angle, type,
    /// number.
    #[test]
    fn detail_names_size_angle_and_number() {
        let strings = Strings::english();
        let mut tool = Tool {
            id: ToolId::new("t1"),
            name: "1/4 upcut".into(),
            vendor: None,
            product_id: None,
            number: Some(2),
            cutter: ToolKind::EndMill,
            diameter_mm: 6.35,
            flutes: 2,
            feed_mm_min: None,
            plunge_mm_min: None,
            rpm: None,
            step_down_mm: None,
            step_over_fraction: None,
        };
        assert_eq!(detail(&strings, &tool, Units::Mm), "6.35 mm · T2");
        tool.cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        tool.number = None;
        assert_eq!(detail(&strings, &tool, Units::Inch), "0.25 in · 60°");
    }
}
