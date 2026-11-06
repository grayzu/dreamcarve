// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operation editor (`docs/specs/phase-4/functional.md`, capabilities
//! 66 and 67; mock-up `05-operation-profile.svg`; `experience.md`, section
//! 7): the operation the list selected, its rows in their groups, each
//! group a section whose open state is a preference (`gui.toml`), a group
//! with no row not shown. The shell walks `dc_app::editor::fields`'s table
//! with one renderer per field kind ([`render`]'s `match`, which the
//! compiler keeps exhaustive); nothing here is per operation kind. Every
//! field shows its unit, commits when left, shows the file's located
//! message under itself, and shows its help with its `.dcv` field name on
//! hover of its label.
//!
//! The Targets group lists the targets with a remove button each,
//! highlights them on the Viewport while it is hovered
//! ([`highlighted_targets`]), and has *Add selected*, *Replace with
//! selected* and *Pick…*, which arms the Viewport: each click on a shape
//! adds it until Escape ([`target_pick`]). Both are view state kept in
//! egui's memory, as the Project panel's park pick is, so the Viewport
//! reads them without the panel holding project data.
//!
//! Clicking a Warnings row that names a field opens its operation here and
//! focuses that field ([`request_focus`]).

use dc_app::editor::{self, FieldKind, Group, Row, fields, targets};
use dc_app::operations::Kind;
use dc_app::{Command, StrKey, Strings};
use dc_job::Tier;
use dc_project::{InstanceId, Operation, OperationId, Project, ToolKind};
use eframe::egui::{self, Id, Rect, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button};
use crate::widgets::icon::Icon;
use crate::widgets::op_glyph::op_glyph;
use crate::widgets::row::{self, GAP_PX};
use crate::widgets::section::section;
use crate::widgets::segmented::segmented;
use crate::widgets::toggle::toggle;
use crate::widgets::unit_field::{HEIGHT_PX, UnitField, field as unit_field};

/// The panel's content margin, px (`experience.md`, section 3).
const MARGIN_PX: i8 = 12;
/// A segment's widest label, px (section 7, "Segmented control").
const SEGMENT_LABEL_PX: f32 = 56.0;
/// How far a feature's or a variant's own rows indent, px (section 7,
/// "Toggle").
const NESTED_PX: f32 = 16.0;
/// The header band's height, px.
const HEADER_PX: f32 = 32.0;

/// Where the Viewport's target pick is armed, and for which operation.
fn pick_id() -> Id {
    Id::unique("dc.editor.target_pick")
}

/// The operation whose targets the next Viewport clicks add, while
/// *Pick…* is armed.
pub(crate) fn target_pick(ctx: &egui::Context) -> Option<OperationId> {
    ctx.data(|d| d.get_temp::<OperationId>(pick_id()))
}

/// Arms the Viewport's target pick for `op`, or disarms it.
pub(crate) fn arm_target_pick(ctx: &egui::Context, op: Option<OperationId>) {
    ctx.data_mut(|d| match op {
        Some(op) => {
            d.insert_temp(pick_id(), op);
        }
        None => d.remove::<OperationId>(pick_id()),
    });
}

/// Where this frame's hovered Targets group leaves its targets.
fn highlight_id() -> Id {
    Id::unique("dc.editor.highlight")
}

/// The targets of the Targets group the pointer is over this frame, for
/// the Viewport to draw in `shape.hover`; taken, so a group no longer
/// drawn highlights nothing.
pub(crate) fn highlighted_targets(ctx: &egui::Context) -> Vec<InstanceId> {
    ctx.data_mut(|d| d.remove_temp::<Vec<InstanceId>>(highlight_id()))
        .unwrap_or_default()
}

/// Where a Warnings row's field waits to be focused.
fn focus_id() -> Id {
    Id::unique("dc.editor.focus")
}

/// Asks the editor to scroll to and focus the row of `op` that `field`
/// (a finding's `field`) is located on, once it is drawn.
pub(crate) fn request_focus(ctx: &egui::Context, op: OperationId, field: String) {
    ctx.data_mut(|d| d.insert_temp(focus_id(), (op, field)));
}

/// What every row reads.
struct Cx<'a> {
    strings: &'a Strings,
    project: &'a Project,
    located: editor::Located<'a>,
    selected: &'a [InstanceId],
    op: &'a Operation,
    /// The row a Warnings click asked to focus, by key path.
    focus: Option<&'static str>,
}

/// Draws the editor of `op`.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, op: &Operation, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let focus = ui
        .data(|d| d.get_temp::<(OperationId, String)>(focus_id()))
        .filter(|(id, _)| id == op.id())
        .and_then(|(_, field)| fields::row_for(op, &field))
        .map(|r| r.key);
    let cx = Cx {
        strings,
        project,
        located: editor::Located::of(app.warnings(), op),
        selected: app.selection().instances(),
        op,
        focus,
    };
    let index = project
        .operations
        .iter()
        .position(|o| o.id() == op.id())
        .unwrap_or(0);
    header(ui, &cx, index);
    egui::ScrollArea::vertical()
        .id_salt(("operation_editor", op.id().as_str()))
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(MARGIN_PX, 4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, 4.0);
                    for (group, rows) in fields::groups(op) {
                        let name = group.section();
                        let open = state.prefs.sections.get(name).copied().unwrap_or(true);
                        if !open && focus.is_some_and(|key| rows.iter().any(|r| r.key == key)) {
                            out.push(Action::SetSection {
                                name: name.to_owned(),
                                open: true,
                            });
                        }
                        let located = cx.located.group(group);
                        let inner = ui.scope(|ui| {
                            section(
                                ui,
                                Id::unique(("dc.editor.group", name)),
                                strings.get(group.title()),
                                open,
                                located,
                                |ui| {
                                    for row in &rows {
                                        render(ui, &cx, row, out);
                                    }
                                },
                            )
                        });
                        if inner.inner {
                            out.push(Action::SetSection {
                                name: name.to_owned(),
                                open: !open,
                            });
                        }
                        if group == Group::Targets && inner.response.contains_pointer() {
                            let ids = op.targets().to_vec();
                            ui.data_mut(|d| d.insert_temp(highlight_id(), ids));
                        }
                    }
                });
        });
}

/// The 32-px title band: the kind's glyph in the operation's colour, then
/// `Name · Kind`.
fn header(ui: &mut Ui, cx: &Cx<'_>, index: usize) {
    let palette = Palette::current(ui);
    let kind = Kind::of(cx.op);
    let title = cx.strings.fmt(
        StrKey::EditorTitle,
        &[
            ("name", cx.op.name()),
            ("kind", cx.strings.get(kind.label())),
        ],
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &title));
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + 12.0 + 8.0, rect.center().y),
        egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
    );
    op_glyph(ui.painter(), glyph, kind, palette.operation(index));
    ui.painter().text(
        egui::pos2(glyph.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_default),
    );
}

/// The id of `row`'s control for `op`.
fn control_id(op: &Operation, row: &Row) -> Id {
    Id::unique(("dc.editor.field", op.id().as_str(), row.key))
}

/// Pushes `result`'s command, or keeps its refusal under the row.
fn commit(
    ui: &Ui,
    refused_id: Id,
    result: Result<Option<Command>, dc_app::CommandError>,
    out: &mut Vec<Action>,
) {
    match result {
        Ok(command) => {
            ui.data_mut(|d| d.remove::<String>(refused_id));
            out.extend(command.map(Action::run));
        }
        Err(e) => {
            ui.data_mut(|d| d.insert_temp(refused_id, e.to_string()));
        }
    }
}

/// One row: its renderer, then the message under it (the refusal of what
/// was typed, else the file's located message).
fn render(ui: &mut Ui, cx: &Cx<'_>, row: &'static Row, out: &mut Vec<Action>) {
    let id = control_id(cx.op, row);
    let refused_id = id.with("refused");
    let refused: Option<String> = ui.data(|d| d.get_temp(refused_id));
    let status = cx.located.status(row);
    let tier = refused
        .as_ref()
        .map(|_| Tier::Error)
        .or(status.map(|s| s.tier));
    let focused = cx.focus == Some(row.key);
    let nested = row.key.contains('.');
    let response = ui
        .horizontal(|ui| {
            if nested {
                ui.add_space(NESTED_PX);
            }
            match row.kind {
                FieldKind::Text
                | FieldKind::Length
                | FieldKind::Rate
                | FieldKind::Angle
                | FieldKind::Percentage
                | FieldKind::Count
                | FieldKind::Seconds => typed(ui, cx, row, id, tier, focused, out),
                FieldKind::Choice(options) => choice(ui, cx, row, id, options, out),
                FieldKind::Checkbox => checkbox(ui, cx, row, out),
                FieldKind::Toggle(_) => switch(ui, cx, row, out),
                FieldKind::ToolPicker => tool_picker(ui, cx, row, id, out),
                FieldKind::InstanceList => {
                    // The Targets group's own block, under the row.
                    None
                }
                FieldKind::TabRequest
                | FieldKind::ClearingList
                | FieldKind::CutOut
                | FieldKind::MaleBoard => block(ui, cx, row),
            }
        })
        .inner;
    if matches!(row.kind, FieldKind::InstanceList) {
        target_list(ui, cx, out);
    }
    if focused {
        if let Some(response) = &response {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        ui.data_mut(|d| d.remove::<(OperationId, String)>(focus_id()));
    }
    match (refused, status) {
        (Some(reason), _) => row::message(ui, Tier::Error, &reason),
        (None, Some(status)) => row::message(ui, status.tier, status.message),
        (None, None) => {}
    }
}

/// The label with its help and the `.dcv` field name.
fn label(ui: &mut Ui, cx: &Cx<'_>, row: &Row) {
    row::label(
        ui,
        cx.strings.get(row.label),
        cx.strings.get(row.help),
        Some(row.key),
    );
}

/// A typed row: the unit field, its placeholder the tool's value while
/// `null`, and ⟲ while it has its own.
fn typed(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    tier: Option<Tier>,
    focused: bool,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, row);
    let value = editor::value_text(cx.project, cx.op, row);
    let placeholder = editor::placeholder(cx.project, cx.strings, cx.op, row).unwrap_or_default();
    let reset = editor::can_reset(cx.op, row).then(|| cx.strings.get(StrKey::EditorReset));
    let name = cx.strings.get(row.label);
    if focused {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let output = unit_field(
        ui,
        UnitField {
            placeholder: &placeholder,
            tier,
            reset,
            ..UnitField::new(
                id,
                name,
                &value,
                editor::unit_suffix(row, cx.project.display_units),
            )
        },
    );
    let refused_id = id.with("refused");
    if let Some(text) = output.committed {
        commit(
            ui,
            refused_id,
            editor::commit_text(cx.project, cx.op, row, &text),
            out,
        );
    }
    if output.reset {
        commit(ui, refused_id, editor::reset(cx.op, row), out);
    }
    Some(output.response)
}

/// A choice: segmented for two to four options whose labels fit 56 px
/// each, a dropdown otherwise (section 7).
fn choice(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    options: &[editor::ChoiceOption],
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, row);
    let labels: Vec<&str> = options.iter().map(|o| cx.strings.get(o.label)).collect();
    let current = editor::choice(cx.op, row);
    let fits = (2..=4).contains(&labels.len())
        && labels.iter().all(|l| {
            ui.painter()
                .layout_no_wrap(
                    (*l).to_owned(),
                    TextStyle::Body.resolve(ui.style()),
                    Palette::current(ui).text_primary,
                )
                .size()
                .x
                <= SEGMENT_LABEL_PX
        });
    let refused_id = id.with("refused");
    let mut picked = None;
    let response = if fits {
        picked = segmented(
            ui,
            id,
            &labels,
            current.unwrap_or(labels.len()),
            ui.available_width(),
        );
        None
    } else {
        let selected = current
            .and_then(|i| labels.get(i))
            .copied()
            .unwrap_or_default();
        let width = ui.available_width();
        let response = egui::ComboBox::from_id_salt(id)
            .truncate()
            .selected_text(selected)
            .width(width)
            .show_ui(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    if ui.selectable_label(current == Some(i), *label).clicked() {
                        picked = Some(i);
                    }
                }
            })
            .response
            .accessible_name(cx.strings.get(row.label));
        Some(response)
    };
    if let Some(index) = picked {
        commit(
            ui,
            refused_id,
            editor::choose(cx.project, cx.op, row, index),
            out,
        );
    }
    response
}

/// A checkbox, its label to its right; hovering it shows the help.
fn checkbox(ui: &mut Ui, cx: &Cx<'_>, row: &Row, out: &mut Vec<Action>) -> Option<egui::Response> {
    let palette = Palette::current(ui);
    let mut on = editor::checked(cx.op, row);
    let response = ui.checkbox(&mut on, cx.strings.get(row.label));
    let help = cx.strings.get(row.help);
    let response = response.on_hover_ui(|ui| {
        ui.label(egui::RichText::new(help).text_style(TextStyle::Body));
        ui.label(
            egui::RichText::new(row.key)
                .text_style(TextStyle::Monospace)
                .color(palette.text_secondary),
        );
    });
    if response.changed() {
        commit(
            ui,
            control_id(cx.op, row).with("refused"),
            editor::set_checked(cx.op, row, on),
            out,
        );
    }
    Some(response)
}

/// A feature's switch; its settings are the rows under its key.
fn switch(ui: &mut Ui, cx: &Cx<'_>, row: &Row, out: &mut Vec<Action>) -> Option<egui::Response> {
    label(ui, cx, row);
    let on = editor::switched(cx.op, row);
    let response = toggle(ui, on, cx.strings.get(row.label));
    if response.clicked() {
        commit(
            ui,
            control_id(cx.op, row).with("refused"),
            editor::switch(cx.project, cx.op, row, !on),
            out,
        );
    }
    Some(response)
}

/// A tool type's glyph (section 6).
const fn tool_icon(kind: &ToolKind) -> Icon {
    match kind {
        ToolKind::EndMill => Icon::Cylinder,
        ToolKind::BallNose => Icon::CircleDot,
        ToolKind::VBit { .. } => Icon::Triangle,
        ToolKind::Engraver => Icon::PencilLine,
        ToolKind::Drill => Icon::MoveDown,
    }
}

/// `tool`'s name as the picker lists it: `name (T2)`.
fn tool_text(strings: &Strings, project: &Project, tool: &dc_project::Tool) -> String {
    let number = tool.number.map_or_else(
        || {
            project
                .tools
                .iter()
                .position(|t| t.id == tool.id)
                .map_or(0, |i| i + 1)
                .to_string()
        },
        |n| n.to_string(),
    );
    strings.fmt(
        StrKey::EditorToolNumbered,
        &[("name", &tool.name), ("number", &number)],
    )
}

/// The tool picker: the current tool, opening a popover of the project's
/// tools, its last row *Add from library…* (section 8: a popover, never
/// a sheet over a sheet).
fn tool_picker(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, row);
    let palette = Palette::current(ui);
    let current = cx.project.tools.iter().find(|t| t.id == *cx.op.tool());
    let text = current.map_or_else(
        || cx.strings.get(StrKey::EditorToolNone).to_owned(),
        |t| tool_text(cx.strings, cx.project, t),
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT_PX), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ComboBox, true, cx.strings.get(row.label))
    });
    let outline = if response.hovered() {
        palette.text_secondary
    } else {
        palette.border_strong
    };
    ui.painter().rect(
        rect,
        4.0,
        palette.surface_sunken,
        egui::Stroke::new(1.0, outline),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 8.0;
    if let Some(tool) = current {
        let glyph =
            tool_icon(&tool.cutter).galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
        ui.painter().galley(
            egui::pos2(x, rect.center().y - glyph.size().y / 2.0),
            glyph,
            palette.text_secondary,
        );
        x += theme::ICON_SMALL_PX + 6.0;
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &text,
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
    );
    let chevron = Icon::ChevronDown.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    ui.painter().galley(
        egui::pos2(
            rect.right() - 8.0 - chevron.size().x,
            rect.center().y - chevron.size().y / 2.0,
        ),
        chevron,
        palette.text_secondary,
    );
    let refused_id = id.with("refused");
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(rect.width());
        for tool in &cx.project.tools {
            let chosen = tool.id == *cx.op.tool();
            let label = tool_text(cx.strings, cx.project, tool);
            let item = ui.selectable_label(chosen, label);
            if item.clicked() {
                commit(ui, refused_id, editor::set_tool(cx.op, row, &tool.id), out);
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button(cx.strings.get(StrKey::EditorToolAddFromLibrary))
            .clicked()
        {
            out.push(Action::OpenLibrary);
            ui.close();
        }
    });
    Some(response)
}

/// A block a later slice edits in place: its label and a note.
fn block(ui: &mut Ui, cx: &Cx<'_>, row: &Row) -> Option<egui::Response> {
    let palette = Palette::current(ui);
    label(ui, cx, row);
    let response = ui.add(
        egui::Label::new(
            egui::RichText::new(cx.strings.get(StrKey::EditorBlockLater))
                .text_style(TextStyle::Small)
                .color(palette.text_secondary),
        )
        .wrap()
        .selectable(false),
    );
    Some(response)
}

/// The Targets group's list and buttons (capability 67).
fn target_list(ui: &mut Ui, cx: &Cx<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = cx.strings;
    let op = cx.op;
    let listed = targets::list(cx.project, op);
    let refused_id = Id::unique(("dc.editor.targets", op.id().as_str())).with("refused");
    egui::Frame::NONE
        .fill(palette.surface_sunken)
        .stroke(egui::Stroke::new(1.0, palette.border_strong))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for target in &listed {
                ui.horizontal(|ui| {
                    ui.set_min_height(22.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&target.name).text_style(TextStyle::Body),
                        )
                        .truncate()
                        .selectable(false),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let remove =
                            strings.fmt(StrKey::EditorTargetsRemove, &[("name", &target.name)]);
                        let spec = ButtonSpec::new(ButtonKind::Quiet, "")
                            .icon(Icon::X, theme::ICON_SMALL_PX)
                            .tooltip(&remove)
                            .height(22.0);
                        if button(ui, spec).clicked() {
                            commit(ui, refused_id, targets::remove(op, &target.id), out);
                        }
                    });
                });
            }
            let summary = match listed.len() {
                0 => strings.get(StrKey::EditorTargetsNone).to_owned(),
                1 => strings.get(StrKey::EditorTargetsOne).to_owned(),
                n => strings.fmt(StrKey::EditorTargetsMany, &[("count", &n.to_string())]),
            };
            ui.label(
                egui::RichText::new(summary)
                    .text_style(TextStyle::Small)
                    .color(palette.text_secondary),
            );
        });
    let picking = target_pick(ui.ctx()).as_ref() == Some(op.id());
    ui.horizontal_wrapped(|ui| {
        let has = !cx.selected.is_empty();
        let add = ButtonSpec::new(
            ButtonKind::Secondary,
            strings.get(StrKey::EditorTargetsAddSelected),
        )
        .enabled(has);
        if button(ui, add).clicked() {
            commit(ui, refused_id, targets::add(op, cx.selected), out);
        }
        let replace = ButtonSpec::new(
            ButtonKind::Secondary,
            strings.get(StrKey::EditorTargetsReplace),
        )
        .enabled(has);
        if button(ui, replace).clicked() {
            commit(ui, refused_id, targets::replace(op, cx.selected), out);
        }
        let pick_kind = if picking {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        };
        let pick = ButtonSpec::new(pick_kind, strings.get(StrKey::EditorTargetsPick))
            .icon(Icon::Crosshair, theme::ICON_SMALL_PX);
        if button(ui, pick).clicked() {
            let armed = (!picking).then(|| op.id().clone());
            arm_target_pick(ui.ctx(), armed);
        }
    });
    if picking {
        row::hint(ui, strings.get(StrKey::EditorTargetsPicking));
    }
    if let Some(reason) = ui.data(|d| d.get_temp::<String>(refused_id)) {
        row::message(ui, Tier::Error, &reason);
    }
}
