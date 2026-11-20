// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Tools step (functional spec, capability 63; mock-up
//! `16-tools-tab.svg`; `experience.md`, section 7; `redesign.md`, "The
//! steps"). The step panel holds [`list`]: *New tool* and *Add from
//! library…* over the project's tools, one 52-px row each with its type
//! glyph, name, and under it its cutting diameter, a tapered ball nose's
//! tip, its angle labelled for what it is (`90°`, `6.7°/side`) and its
//! number (`tools::row_detail`), and a status cell
//! (`tools::row_status`): a red dot when it lacks a value an
//! operation needs (a feed, plunge rate or RPM, a tapered ball nose's
//! tip, taper or flute length), the mark an operation's row has when a
//! warning or an error names the tool, and the `unused_tool` hint
//! when no operation uses it; *Duplicate* and *Delete* under the list;
//! and *Tool Library…* at the card's foot. The Inspector holds the
//! selected tool's editor ([`editor_card`]), the rows of `tools::rows`,
//! every field with its unit: a type's own fields follow the type (a
//! V-bit's *Angle*; a tapered ball nose's *Tip diameter*, *Taper per
//! side* and *Flute length*, #685),
//! and *Save to library* in its footer. No tool is selected until a row
//! is clicked or a tool is added. Feed, plunge and RPM, and a tapered ball
//! nose's three values, are
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
//! The step holds no project data: it reads the snapshot and emits
//! commands. Which row is selected, a held switch, a name to focus and
//! the field a Warnings row asked for ([`open_field`]) are view state,
//! kept in egui's memory.

use dc_app::tools::{self, KindChoice, KindSwitch, RowMark, ToolField};
use dc_app::{Command, CommandError, StrKey, Strings};
use dc_project::{Project, Tool, ToolId, ToolKind, Units};
use eframe::egui::{self, Id, Rect, Sense, TextStyle, Ui};

use crate::panels::library::kind_label;
use crate::panels::side;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::row::{surface, surface_rect};
use crate::widgets::unit_field::{Required, UnitField, field as unit_field, row, validation_line};

/// A list row's height, px (section 3).
const ROW_PX: f32 = theme::LIST_ROW_PX;
/// The status cell's width, px.
const STATUS_PX: f32 = 24.0;
/// The editor footer's height, px: a control between the gap above it
/// and the card's padding under it.
const FOOTER_PX: f32 = theme::GAP_PX + theme::CONTROL_PX + theme::CARD_PADDING_PX;
/// The panel's content margin, px.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;
/// The tallest a type dropdown's menu grows, px: every type shows (six
/// in the editor, seven in the library's filter) with no scroll.
pub(crate) const KIND_MENU_PX: f32 = 320.0;
/// The room *Tool Library…* takes at the step panel's foot, px: the gap
/// above it, the button and the card's padding.
const FOOT_PX: f32 = theme::GAP_PX + theme::CONTROL_PX + theme::CARD_PADDING_PX;

/// The selected row, and how many tools there were, between frames.
#[derive(Debug, Clone, Default, PartialEq)]
struct Selected {
    tool: Option<ToolId>,
    /// The project's tool count when last looked at; `None` since the
    /// selection was last set, so the next look only records it.
    count: Option<usize>,
}

/// Where the selection lives in egui's memory.
fn selected_id() -> Id {
    Id::unique("tools_step_selected")
}

/// The tool the Tools step shows in the Inspector: the row last clicked,
/// or the newest when a tool has just been added (from the library, by
/// *Duplicate*); none until then, and none once that tool is deleted.
pub(crate) fn selected(ctx: &egui::Context, project: &Project) -> Option<ToolId> {
    let mut state: Selected = ctx.data(|d| d.get_temp(selected_id())).unwrap_or_default();
    let count = project.tools.len();
    if state.count.is_some_and(|was| count > was) {
        state.tool = project.tools.last().map(|t| t.id.clone());
    }
    if !state
        .tool
        .as_ref()
        .is_some_and(|t| project.tools.iter().any(|tool| tool.id == *t))
    {
        state.tool = None;
    }
    state.count = Some(count);
    let tool = state.tool.clone();
    ctx.data_mut(|d| d.insert_temp(selected_id(), state));
    tool
}

/// Selects tool `tool`, or none.
pub(crate) fn select(ctx: &egui::Context, tool: Option<&ToolId>) {
    ctx.data_mut(|d| {
        d.insert_temp(
            selected_id(),
            Selected {
                tool: tool.cloned(),
                count: None,
            },
        );
    });
}

/// Where the field a Warnings row asked for waits to be focused.
fn focus_field_id() -> Id {
    Id::unique("tools_step_focus_field")
}

/// What a Warnings row about the field at `path` (`tools[t1].feed_mm_min`)
/// opens (capability 76): the tool it names selected, so the Inspector
/// shows it, and the field it is about focused and scrolled to when the
/// editor next draws it. A path that names no field of the editor
/// (`unused_tool`'s `tools[t1]`) selects the tool alone, and one that
/// names no tool selects none.
pub(crate) fn open_field(ctx: &egui::Context, project: &Project, path: &str) {
    let located = tools::located(project, path);
    select(ctx, located.as_ref().map(|(tool, _)| tool));
    ctx.data_mut(|d| {
        if let Some((tool, Some(field))) = located {
            d.insert_temp(focus_field_id(), (tool, field));
        } else {
            d.remove::<(ToolId, ToolField)>(focus_field_id());
        }
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

/// The step panel's content under its title: the buttons that add a
/// tool, the list, *Duplicate* and *Delete* under it, and *Tool
/// Library…* at the card's foot.
pub(crate) fn list(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let current = state.tool.as_ref();
    // Moving to another tool discards a held switch.
    if pending_v_bit(ui).is_some_and(|held| current != Some(&held)) {
        set_pending_v_bit(ui, None);
    }
    let full = ui.available_rect_before_wrap();
    let top = Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - FOOT_PX));
    let mut above = ui.new_child(egui::UiBuilder::new().max_rect(top));
    above.set_clip_rect(top.intersect(ui.clip_rect()));
    {
        let ui = &mut above;
        let gap = ui.spacing().item_spacing.y;
        if project.tools.is_empty() {
            empty(ui, strings, project, out);
        } else {
            ui.horizontal(|ui| {
                ui.add_space(MARGIN_PX);
                add_buttons(ui, strings, project, out);
            });
            ui.add_space(8.0 - gap);
            // The list takes what the buttons under it leave.
            let room = (ui.available_height() - theme::GAP_PX - theme::CONTROL_PX).max(ROW_PX);
            crate::widgets::scroll::show(
                ui,
                egui::ScrollArea::vertical()
                    .id_salt("tools_list")
                    .max_height(room)
                    .auto_shrink([false, true]),
                crate::widgets::scroll::Surface::Panel,
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for tool in &project.tools {
                        list_row(ui, state, tool, current == Some(&tool.id));
                    }
                },
            );
            ui.add_space(8.0 - gap);
            list_actions(ui, strings, project, current, out);
        }
    }
    // *Tool Library…* (capability 64), at the card's foot.
    let foot = Rect::from_min_max(
        egui::pos2(full.left() + MARGIN_PX, full.bottom() - FOOT_PX),
        egui::pos2(full.right() - MARGIN_PX, full.bottom() - MARGIN_PX),
    );
    let mut below = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(foot)
            .layout(egui::Layout::left_to_right(egui::Align::Max)),
    );
    let library = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppMenuToolLibrary))
        .icon(Icon::LibraryBig, theme::ICON_SMALL_PX);
    if button(&mut below, library).clicked() {
        out.push(Action::OpenLibrary);
    }
    ui.allocate_rect(full, Sense::hover());
}

/// The Inspector's content for `tool`: its type glyph and name, its
/// fields in a scroll area, and *Save to library* in a footer that stays
/// in view.
pub(crate) fn editor_card(ui: &mut Ui, state: &FrameState<'_>, tool: &Tool, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let units = state.app.project().display_units;
    header(ui, tool);
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("tool_editor")
            .auto_shrink(false)
            .max_height((ui.available_height() - FOOTER_PX).max(0.0)),
        crate::widgets::scroll::Surface::Panel,
        |ui| {
            ui.horizontal(|ui| {
                ui.add_space(MARGIN_PX);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - MARGIN_PX);
                    editor(ui, strings, units, tool, out);
                });
            });
        },
    );
    footer(ui, strings, tool, out);
}

/// The editor's header: the tool's type glyph and its name at
/// panel-title weight, cut with an ellipsis before the Inspector's close
/// button. Painted only: the Name field under it carries the name for
/// assistive tools.
fn header(ui: &mut Ui, tool: &Tool) {
    let palette = Palette::current(ui);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), side::HEADER_PX),
        Sense::hover(),
    );
    let rect = rect.translate(egui::vec2(0.0, 2.0));
    let glyph = egui::pos2(rect.left() + MARGIN_PX + 8.0, rect.center().y);
    super::editor::paint_tool_icon(ui, glyph, &tool.cutter, palette.text_secondary);
    let left = glyph.x + 8.0 + 8.0;
    let mut job = egui::text::LayoutJob::simple_singleline(
        tool.name.clone(),
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (rect.right() - side::CLOSE_RESERVE_PX - left).max(0.0),
    );
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text_primary,
    );
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
            select(ui.ctx(), Some(&tool.id));
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
    ui.add_space(theme::GAP_PX - ui.spacing().item_spacing.y);
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
        ui.add_space(MARGIN_PX);
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
    side::empty_sentence(ui, strings.get(StrKey::AppPanelNoTools));
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

/// One 52-px row on its own rounded surface (section 7, "List row"): the
/// type glyph, the name over its detail (`tools::row_detail`: the cutting
/// diameter, a tapered ball nose's tip, the angle labelled, the number),
/// and the status cell. Assistive tools read the detail as the row's
/// value.
fn list_row(ui: &mut Ui, state: &FrameState<'_>, tool: &Tool, chosen: bool) {
    let palette = Palette::current(ui);
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let detail = tools::row_detail(strings, tool, project.display_units);
    let (slot, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::selected(egui::Role::ListItem, true, chosen, &tool.name);
        info.current_text_value = Some(detail.clone());
        info
    });
    if response.clicked() {
        select(ui.ctx(), Some(&tool.id));
    }
    let rect = surface_rect(slot);
    surface(ui, rect, chosen, response.hovered());
    crate::widgets::button::focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + 12.0 + 8.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    super::editor::paint_tool_icon(ui, glyph.center(), &tool.cutter, palette.text_secondary);
    let text_left = glyph.right() + 10.0;
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
        rect.center().y - 9.0,
    );
    line(
        &detail,
        TextStyle::Small,
        palette.text_secondary,
        rect.center().y + 9.0,
    );

    // The status cell (`tools::row_status`): the error mark or the
    // warning mark and count of an operation's row when a finding names
    // the tool; the red dot for a value it still needs (a rate, a tapered
    // ball nose's tip, taper or flute length); else the hint. Hovering it
    // lists the messages behind it.
    let cell = Rect::from_center_size(
        egui::pos2(rect.right() - 8.0 - STATUS_PX / 2.0, rect.center().y),
        egui::vec2(STATUS_PX, STATUS_PX),
    );
    let status = tools::row_status(tool, app.warnings());
    // The marks have no words: assistive tools read what each means.
    let name = match status.mark {
        RowMark::Clean => return,
        RowMark::Error => {
            crate::widgets::status::error(ui, cell);
            strings.get(StrKey::AppToolsRowError).to_owned()
        }
        RowMark::LacksValue => {
            ui.painter()
                .circle_filled(cell.center(), 4.0, palette.status_error.stroke);
            strings.get(StrKey::AppToolsLacksValue).to_owned()
        }
        RowMark::Warnings(count) => {
            crate::widgets::status::warnings(ui, cell, count);
            dc_app::status::counted(
                strings,
                count,
                StrKey::AppStatusWarning,
                StrKey::AppStatusWarnings,
            )
        }
        RowMark::Hint => {
            let glyph = Icon::Info.galley(ui, theme::ICON_SMALL_PX, palette.status_info.text);
            ui.painter().galley(
                cell.center() - glyph.size() / 2.0,
                glyph,
                palette.status_info.text,
            );
            strings.get(StrKey::AppToolsRowHint).to_owned()
        }
    };
    let id = ui.make_persistent_id(("tool_status", tool.id.as_str()));
    let response = ui.interact(cell, id, Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    response.on_hover_text(status.messages.join("\n"));
}

/// The selected project tool's fields (its footer is [`footer`]): the
/// shared rows of [`tool_fields`], each change one command.
fn editor(ui: &mut Ui, strings: &Strings, units: Units, tool: &Tool, out: &mut Vec<Action>) {
    let held = pending_v_bit(ui).as_ref() == Some(&tool.id)
        && !matches!(tool.cutter, ToolKind::VBit { .. });
    let focus_name = ui.data(|d| d.get_temp::<ToolId>(focus_name_id())).as_ref() == Some(&tool.id);
    // The field a Warnings row asked for: asked once, so dropped whether
    // or not this tool's rows hold it.
    let asked = ui.data(|d| d.get_temp::<(ToolId, ToolField)>(focus_field_id()));
    if asked.is_some() {
        ui.data_mut(|d| d.remove::<(ToolId, ToolField)>(focus_field_id()));
    }
    let focus = asked
        .filter(|(id, _)| *id == tool.id)
        .map(|(_, field)| field);
    let output = tool_fields(
        ui,
        &ToolFields {
            salt: Id::unique(("tool_field", tool.id.as_str())),
            strings,
            units,
            tool,
            held,
            focus_name,
            focus,
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
        Some(FieldEdit::Set(next, _)) if *next != *tool => {
            out.push(Action::Run(Box::new(Command::UpdateTool(*next))));
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
    /// The field to focus and scroll to this frame: the one a Warnings
    /// row is about (capability 76).
    pub focus: Option<ToolField>,
    /// A field marked with why its last change did not take, shown under
    /// it in the error state (a library write that failed, 64a).
    pub marked: Option<(ToolField, &'a str)>,
}

/// A change the rows made this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldEdit {
    /// The tool with `ToolField` changed from what was typed.
    Set(Box<Tool>, ToolField),
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

/// The tool editor's rows (`tools::rows`), in the order of
/// `tools::fields_for` the tool's type: name, type, then the type's own
/// values (a V-bit's angle; a tapered ball nose's tip diameter, taper per
/// side and flute length, #685), then diameter, flutes, feed, plunge,
/// RPM, step-down, stepover, number, vendor and product id, every field
/// with its unit. A value the tool still needs (`FieldRow::required`: a
/// rate, a tapered ball nose's tip, taper or flute length) is empty,
/// never prefilled, with the required-field look and the file's message
/// as its tooltip (66b). While the tool's switch to V-bit is held, a
/// V-bit's rows are drawn and its *Angle* field shows empty and in the
/// error state, with the line that says what to type; a typed angle makes
/// the switch, and leaving it empty discards it (#581).
pub(crate) fn tool_fields(ui: &mut Ui, spec: &ToolFields<'_>) -> FieldsOutput {
    let ToolFields {
        salt,
        strings,
        units,
        tool,
        held,
        focus_name,
        focus,
        marked,
    } = *spec;
    let mut result = FieldsOutput::default();
    let angle_required = strings.get(StrKey::AppToolsAngleRequired);
    // The held switch shows the type it is switching to.
    let rows = if held {
        tools::rows_for(tool, KindChoice::VBit, units)
    } else {
        tools::rows(tool, units)
    };
    for row_spec in &rows {
        let field = row_spec.field;
        let value = row_spec.text.as_str();
        let hint = if field == ToolField::Number {
            strings.get(StrKey::AppToolsNumberUnset)
        } else {
            ""
        };
        let id = field_id(salt, field);
        let name = strings.get(row_spec.label);
        let is_marked = marked.is_some_and(|(f, _)| f == field);
        let holding = held && field == ToolField::Angle;
        // A marked field says why its last change did not take, and the
        // held V-bit's angle what to type: each a line under the field.
        let line = match marked {
            Some((_, reason)) if is_marked => Some(reason),
            _ if holding => Some(angle_required),
            _ => None,
        };
        // A value the tool still needs is required (functional 66b; slice
        // 26, #618; #685): *Required* in the field and the file's message
        // its tooltip, not a line under it.
        let required = row_spec
            .required
            .as_deref()
            .filter(|_| line.is_none() && value.is_empty())
            .map(|message| Required {
                placeholder: strings.get(StrKey::EditorRequired),
                message,
            });
        let error = line.or(row_spec.required.as_deref());
        if focus == Some(field) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let (label, output) = row(ui, name, |ui| {
            let output = unit_field(
                ui,
                UnitField {
                    placeholder: hint,
                    tier: error.map(|_| dc_job::Tier::Error),
                    required,
                    ..UnitField::new(id, name, value, row_spec.unit)
                },
            );
            if let (Some(message), None) = (error, required) {
                validation_line(ui, message);
            }
            output
        });
        let response = output.response.labelled_by(label.id);
        if focus == Some(field) {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        let left = response.lost_focus();
        if field == ToolField::Name && focus_name {
            focus_with_text_selected(ui, id, value);
        }
        if required.is_none() {
            response.on_hover_text(field.file_name());
        }
        if holding {
            // The held switch: a typed angle makes it, one undo step; the
            // field left empty (or Escape) discards it; a refusal keeps it.
            match output
                .committed
                .map(|typed| tools::finish_v_bit(tool, &typed))
            {
                Some(Ok(Some(Command::UpdateTool(next)))) => {
                    result.hold = Some(false);
                    result.edit = Some(FieldEdit::Set(Box::new(next), field));
                }
                Some(Ok(_)) => result.hold = Some(false),
                Some(Err(e)) => result.edit = Some(FieldEdit::Refused(field, e.to_string())),
                None if left => result.hold = Some(false),
                None => {}
            }
        } else if let Some(typed) = output.committed {
            result.edit = Some(match tools::set_field(tool, field, &typed, units) {
                Ok(next) => FieldEdit::Set(Box::new(next), field),
                Err(e) => FieldEdit::Refused(field, e.to_string()),
            });
        }
        if field == ToolField::Name {
            // The type, then the values the type has of its own.
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

/// The type dropdown: the six cutters (`KindChoice::ALL`); a V-bit shows
/// its angle field, a tapered ball nose its tip diameter, taper per side
/// and flute length. Choosing *V-bit* for another type holds the switch
/// (`held` from then on) and moves the focus to the empty *Angle* field;
/// choosing *Tapered ball nose* writes the cutter with none of its three
/// values set, one undo step, and each shows as required.
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
            .height(KIND_MENU_PX)
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
                result.edit = Some(FieldEdit::Set(next, ToolField::Angle));
            }
            KindSwitch::HoldForAngle => {
                result.hold = Some(true);
                ui.memory_mut(|m| m.request_focus(field_id(salt, ToolField::Angle)));
                ui.ctx().request_repaint();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The row's second line (`tools::row_detail`, which the list draws)
    /// reads as the spec's rows do: the cutting diameter, a tapered ball
    /// nose's tip, the angle labelled for what it is, the number
    /// (capability 63).
    #[test]
    fn detail_names_size_angle_and_number() {
        let strings = Strings::english();
        let detail = |tool: &Tool, units| tools::row_detail(&strings, tool, units);
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
        assert_eq!(detail(&tool, Units::Mm), "6.35 mm · T2");
        tool.cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        tool.number = None;
        assert_eq!(detail(&tool, Units::Inch), "0.25 in · 60°");
        tool.cutter = ToolKind::TaperedBallNose {
            tip_diameter_mm: Some(0.762),
            taper_angle_deg: Some(6.7),
            flute_length_mm: Some(19.05),
        };
        assert_eq!(
            detail(&tool, Units::Mm),
            "6.35 mm · tip 0.762 mm · 6.7°/side"
        );
    }
}
