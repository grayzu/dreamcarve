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
//!
//! Slice 11 (#540): a point row (a Profile's start point near a point)
//! arms the Viewport for one click ([`point_pick`]), as the park point's
//! pick does; `rest_of` is a dropdown of the earlier enabled Pockets; a
//! tab request is a switch with *Place tabs* beside it, disabled with its
//! reason until slice 17 (#546), and its settings are ordinary rows.
//!
//! Slice 12 (#541): a countersink's depth is a read-only field, derived
//! (S18), or validation's message where there is nothing to derive it
//! from; the Targets group shows the generator's warnings about a target
//! the kind cannot cut.
//!
//! Slice 13 (#542): every control draws a [`Slot`], a row at its key
//! path, so a clearing pass's rows (`clearing[1].tool`) are drawn by the
//! same renderers ([`control`]) inside the clearing list's block
//! ([`crate::panels::clearing`]); a flat depth is a switch and a field; a
//! tool picker lists only the tools that fit and shows a V-bit's cone
//! height under itself; and the editor's foot is `check`'s carve summary
//! for the operation ([`status_line`]).

use dc_app::editor::{self, FieldKind, Group, Row, Slot, fields, targets};
use dc_app::operations::Kind;
use dc_app::{Command, StrKey, Strings};
use dc_job::Tier;
use dc_project::{InstanceId, Operation, OperationId, Project, Tool, ToolKind};
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

/// Where the Viewport's point pick is armed, for which operation and row.
fn point_pick_id() -> Id {
    Id::unique("dc.editor.point_pick")
}

/// The operation and the point row (by key path) the next Viewport click
/// sets, while a point row's pick is armed (a Profile's *Start on the
/// view*).
pub(crate) fn point_pick(ctx: &egui::Context) -> Option<(OperationId, String)> {
    ctx.data(|d| d.get_temp::<(OperationId, String)>(point_pick_id()))
}

/// Arms the Viewport's point pick for `op`'s row at `key`, or disarms it.
pub(crate) fn arm_point_pick(ctx: &egui::Context, pick: Option<(OperationId, String)>) {
    ctx.data_mut(|d| match pick {
        Some(pick) => {
            d.insert_temp(point_pick_id(), pick);
        }
        None => d.remove::<(OperationId, String)>(point_pick_id()),
    });
}

/// The command a Viewport click at `at_mm` makes while `pick` is armed:
/// the point row's point there.
pub(crate) fn point_at(
    project: &Project,
    pick: &(OperationId, String),
    at_mm: dc_geometry::Point,
) -> Option<Action> {
    let (id, key) = pick;
    let op = project.operations.iter().find(|o| o.id() == id)?;
    let row = fields::rows(op).into_iter().find(|r| r.key == key)?;
    match editor::pick_point(op, row, at_mm) {
        Ok(command) => command.map(Action::run),
        Err(e) => Some(Action::Refused(e.to_string())),
    }
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
pub(super) struct Cx<'a> {
    pub(super) strings: &'a Strings,
    pub(super) project: &'a Project,
    located: editor::Located<'a>,
    /// Every finding, for a derived row's reason.
    findings: &'a [dc_job::Finding],
    selected: &'a [InstanceId],
    pub(super) op: &'a Operation,
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
        findings: app.warnings(),
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
    status_line(ui, &cx, &editor::carve_summary(app, op.id()));
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

/// The editor's status line, at its foot (slice 13, #542; mock-up 06):
/// `check`'s carve summary for the operation, as `check` prints it, from
/// the result the app holds ([`editor::carve_summary`]). Nothing for a
/// kind `check` prints no such line for, or while the operation has no
/// current result.
fn status_line(ui: &mut Ui, cx: &Cx<'_>, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let palette = Palette::current(ui);
    egui::Panel::bottom(Id::unique(("dc.editor.status", cx.op.id().as_str())))
        .resizable(false)
        .show_separator_line(true)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_panel)
                .inner_margin(egui::Margin::symmetric(MARGIN_PX, 6)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(cx.strings.get(StrKey::EditorCarveSummary))
                        .text_style(TextStyle::Small)
                        .color(palette.text_secondary),
                )
                .selectable(false),
            );
            for line in lines {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(line)
                            .text_style(TextStyle::Small)
                            .color(palette.text_primary),
                    )
                    .wrap(),
                );
            }
        });
}

/// The id of the control at `key` (a row's key path, a clearing pass's
/// `clearing[1].tool`) for `op`: stable however the layout moves.
pub(super) fn control_id(op: &Operation, key: &str) -> Id {
    Id::unique(("dc.editor.field", op.id().as_str(), key))
}

/// Pushes `result`'s command, or keeps its refusal under the row.
pub(super) fn commit(
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
    let slot = Slot::of(row);
    let refused_id = control_id(cx.op, slot.key()).with("refused");
    let refused: Option<String> = ui.data(|d| d.get_temp(refused_id));
    let status = cx.located.status(row);
    let tier = refused
        .as_ref()
        .map(|_| Tier::Error)
        .or(status.map(|s| s.tier));
    let focused = cx.focus == Some(row.key);
    let offered = editor::offered_tools(cx.project, cx.op);
    let response = ui
        .horizontal(|ui| control(ui, cx, &slot, &offered, tier, focused, out))
        .inner;
    match row.kind {
        FieldKind::InstanceList => target_list(ui, cx, out),
        FieldKind::ToolPicker => tool_note(ui, cx, &slot),
        FieldKind::ClearingList => crate::panels::clearing::list(ui, cx, row, out),
        // Nothing to derive it from: validation's message in place of the
        // number.
        FieldKind::CountersinkDepth => {
            if let Some(Err(why)) = editor::derived(cx.project, cx.strings, cx.findings, cx.op, row)
            {
                row::message(ui, Tier::Error, &why);
            }
        }
        _ => {}
    }
    if matches!(row.kind, FieldKind::PointPick)
        && point_pick(ui.ctx()).is_some_and(|(op, key)| op == *cx.op.id() && key == row.key)
    {
        row::hint(ui, cx.strings.get(StrKey::EditorPickingPoint));
    }
    if focused {
        if let Some(response) = &response {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        ui.data_mut(|d| d.remove::<(OperationId, String)>(focus_id()));
    }
    match refused {
        Some(reason) => row::message(ui, Tier::Error, &reason),
        // A block shows every message located on it, its items' among
        // them (a clearing list's `clearing_not_descending`).
        None if row.kind.is_block() => {
            for status in cx.located.all(row) {
                row::message(ui, status.tier, status.message);
            }
        }
        None => {
            if let Some(status) = status {
                row::message(ui, status.tier, status.message);
            }
        }
    }
}

/// The control of `slot`, by its kind, indented under its feature or
/// variant when its key is nested: what a row of the operation and a row
/// of a clearing pass both draw. `offered` is what a tool picker lists.
pub(super) fn control(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    offered: &[&Tool],
    tier: Option<Tier>,
    focused: bool,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let id = control_id(cx.op, slot.key());
    // A derived value stands where the value it replaces would.
    if slot.row.key.contains('.') && !matches!(slot.row.kind, FieldKind::CountersinkDepth) {
        ui.add_space(NESTED_PX);
    }
    match slot.row.kind {
        FieldKind::Text
        | FieldKind::Length
        | FieldKind::Rate
        | FieldKind::Angle
        | FieldKind::Percentage
        | FieldKind::Count
        | FieldKind::Seconds
        | FieldKind::LengthList => {
            label(ui, cx, slot);
            typed(ui, cx, slot, id, tier, focused, out)
        }
        FieldKind::SwitchedLength => switched_length(ui, cx, slot, id, tier, focused, out),
        FieldKind::Choice(options) => choice(ui, cx, slot, id, options, out),
        FieldKind::Checkbox => checkbox(ui, cx, slot, out),
        FieldKind::Toggle(_) => switch(ui, cx, slot, out),
        FieldKind::ToolPicker => tool_picker(ui, cx, slot, id, offered, out),
        FieldKind::InstanceList => {
            // The Targets group's own block, under the row.
            None
        }
        FieldKind::ClearingList => {
            // The list itself is under the row.
            label(ui, cx, slot);
            let palette = Palette::current(ui);
            Some(
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(cx.strings.get(StrKey::EditorClearingIntro))
                            .text_style(TextStyle::Small)
                            .color(palette.text_secondary),
                    )
                    .wrap()
                    .selectable(false),
                ),
            )
        }
        FieldKind::TabRequest => tab_request(ui, cx, slot.row, out),
        FieldKind::EarlierPocket => earlier_pocket(ui, cx, slot.row, id, out),
        FieldKind::PointPick => point_pick_button(ui, cx, slot.row),
        FieldKind::CountersinkDepth => {
            label(ui, cx, slot);
            derived(ui, cx, slot.row, id)
        }
        FieldKind::CutOut | FieldKind::MaleBoard => block(ui, cx, slot.row),
    }
}

/// The label with its help and the `.dcv` field name (a clearing pass's
/// with its index, `clearing[1].tool`).
fn label(ui: &mut Ui, cx: &Cx<'_>, slot: &Slot<'_>) {
    row::label(
        ui,
        cx.strings.get(slot.row.label),
        cx.strings.get(slot.row.help),
        Some(slot.key()),
    );
}

/// A typed field: the unit field, its placeholder the tool's value while
/// `null`, and ⟲ while it has its own.
fn typed(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    tier: Option<Tier>,
    focused: bool,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let row = slot.row;
    let value = slot.value_text(cx.project, cx.op);
    let placeholder = slot
        .placeholder(cx.project, cx.strings, cx.op)
        .unwrap_or_default();
    let reset = slot
        .can_reset(cx.op)
        .then(|| cx.strings.get(StrKey::EditorReset));
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
            slot.commit_text(cx.project, cx.op, &text),
            out,
        );
    }
    if output.reset {
        commit(ui, refused_id, slot.reset(cx.op), out);
    }
    Some(output.response)
}

/// A derived value (S18; slice 12, #541): a read-only field showing
/// `derived: 5.5` in its unit, never typed. When there is nothing to
/// derive it from (the tool is not a V-bit), nothing: [`render`] draws
/// validation's message under the label in place of the number.
fn derived(ui: &mut Ui, cx: &Cx<'_>, row: &Row, id: Id) -> Option<egui::Response> {
    let text = editor::derived(cx.project, cx.strings, cx.findings, cx.op, row)?.ok()?;
    let output = unit_field(
        ui,
        UnitField {
            enabled: false,
            disabled_reason: Some(cx.strings.get(StrKey::EditorDerivedReadOnly)),
            ..UnitField::new(
                id,
                cx.strings.get(row.label),
                &text,
                editor::unit_suffix(row, cx.project.display_units),
            )
        },
    );
    Some(output.response)
}

/// A length with an off switch for `null` (a V-Carve's flat depth): the
/// switch, then, while it is on, the field. Turning it on opens the field
/// empty and writes nothing (S20: the length typed is the value); the
/// open state is view state, kept in egui's memory under the row's id
/// until the switch is turned off, which writes `null`.
fn switched_length(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    tier: Option<Tier>,
    focused: bool,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let opened_id = id.with("opened");
    let opened = ui.data(|d| d.get_temp::<bool>(opened_id)).unwrap_or(false);
    let on = slot.switched(cx.op) || opened || focused;
    let switch = toggle(ui, on, cx.strings.get(slot.row.label));
    if switch.clicked() {
        if on {
            ui.data_mut(|d| d.remove::<bool>(opened_id));
            commit(
                ui,
                id.with("refused"),
                slot.switch(cx.project, cx.op, false),
                out,
            );
        } else {
            ui.data_mut(|d| d.insert_temp(opened_id, true));
            ui.memory_mut(|m| m.request_focus(id));
        }
    }
    if !on {
        return Some(switch);
    }
    ui.add_space(GAP_PX);
    typed(ui, cx, slot, id, tier, focused, out)
}

/// A choice: segmented for two to four options whose labels fit 56 px
/// each, a dropdown otherwise (section 7).
fn choice(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    options: &[editor::ChoiceOption],
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let row = slot.row;
    let labels: Vec<&str> = options.iter().map(|o| cx.strings.get(o.label)).collect();
    let current = slot.choice(cx.op);
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
        commit(ui, refused_id, slot.choose(cx.project, cx.op, index), out);
    }
    response
}

/// A checkbox, its label to its right; hovering it shows the help.
fn checkbox(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let palette = Palette::current(ui);
    let row = slot.row;
    let mut on = slot.checked(cx.op);
    let response = ui.checkbox(&mut on, cx.strings.get(row.label));
    let help = cx.strings.get(row.help);
    let response = response.on_hover_ui(|ui| {
        ui.label(egui::RichText::new(help).text_style(TextStyle::Body));
        ui.label(
            egui::RichText::new(slot.key())
                .text_style(TextStyle::Monospace)
                .color(palette.text_secondary),
        );
    });
    if response.changed() {
        commit(
            ui,
            control_id(cx.op, slot.key()).with("refused"),
            slot.set_checked(cx.op, on),
            out,
        );
    }
    Some(response)
}

/// A feature's switch; its settings are the rows under its key.
fn switch(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let on = slot.switched(cx.op);
    let response = toggle(ui, on, cx.strings.get(slot.row.label));
    if response.clicked() {
        commit(
            ui,
            control_id(cx.op, slot.key()).with("refused"),
            slot.switch(cx.project, cx.op, !on),
            out,
        );
    }
    Some(response)
}

/// Under a tool picker: the chosen tool's note, a V-bit's cone height
/// (mock-up 06, `cone height 3.175 mm`).
pub(super) fn tool_note(ui: &mut Ui, cx: &Cx<'_>, slot: &Slot<'_>) {
    if let Some(note) = slot
        .tool(cx.project, cx.op)
        .and_then(|tool| editor::tool_note(cx.project, cx.strings, tool))
    {
        row::hint(ui, &note);
    }
}

/// A tab request's switch, with *Place tabs* beside it: shown, and
/// disabled with its reason, until slice 17 (#546) places requests from
/// the editor (capability 69).
fn tab_request(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let response = switch(ui, cx, &Slot::of(row), out);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_enabled_ui(false, |ui| {
            let spec = ButtonSpec::new(
                ButtonKind::Secondary,
                cx.strings.get(StrKey::EditorPlaceTabs),
            )
            .tooltip(cx.strings.get(StrKey::EditorPlaceTabsLater))
            .enabled(false);
            button(ui, spec);
        });
    });
    response
}

/// `rest_of`: none, or one of the earlier enabled Pockets, in a dropdown.
/// A file's `rest_of` that is not one of them (a later Pocket, written by
/// hand) shows as chosen, by name, with validation's message under it.
fn earlier_pocket(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, &Slot::of(row));
    let none = cx.strings.get(StrKey::EditorRestOfNone);
    let current = editor::value_text(cx.project, cx.op, row);
    let selected = if current.is_empty() {
        none.to_owned()
    } else {
        cx.project
            .operations
            .iter()
            .find(|o| o.id().as_str() == current)
            .map_or(current.clone(), |o| o.name().to_owned())
    };
    let offered = editor::earlier_pockets(cx.project, cx.op);
    let mut picked: Option<Option<OperationId>> = None;
    let width = ui.available_width();
    let response = egui::ComboBox::from_id_salt(id)
        .truncate()
        .selected_text(selected)
        .width(width)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_empty(), none).clicked() {
                picked = Some(None);
            }
            for (pocket, name) in &offered {
                if ui
                    .selectable_label(pocket.as_str() == current, name)
                    .clicked()
                {
                    picked = Some(Some(pocket.clone()));
                }
            }
        })
        .response
        .accessible_name(cx.strings.get(row.label));
    if let Some(pocket) = picked {
        commit(
            ui,
            id.with("refused"),
            editor::set_earlier_pocket(cx.op, row, pocket.as_ref()),
            out,
        );
    }
    Some(response)
}

/// A point row's *Start on the view*: arms the Viewport, whose next click
/// sets the point (one click, as the park point's pick); clicked again,
/// or Escape, disarms it.
fn point_pick_button(ui: &mut Ui, cx: &Cx<'_>, row: &Row) -> Option<egui::Response> {
    label(ui, cx, &Slot::of(row));
    let mine = (cx.op.id().clone(), row.key.to_owned());
    let armed = point_pick(ui.ctx()).as_ref() == Some(&mine);
    let kind = if armed {
        ButtonKind::Primary
    } else {
        ButtonKind::Secondary
    };
    let spec = ButtonSpec::new(kind, cx.strings.get(StrKey::EditorPickPoint))
        .icon(Icon::Crosshair, theme::ICON_SMALL_PX);
    let response = button(ui, spec);
    if response.clicked() {
        arm_point_pick(ui.ctx(), (!armed).then_some(mine));
    }
    Some(response)
}

/// A tool type's glyph (section 6).
pub(super) const fn tool_icon(kind: &ToolKind) -> Icon {
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

/// The tool picker: the current tool, opening a popover of `offered` (the
/// project's tools that fit: V-bits for a carve, end mills for a clearing
/// pass), its last row *Add from library…* (section 8: a popover, never
/// a sheet over a sheet).
fn tool_picker(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    offered: &[&Tool],
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let row = slot.row;
    let palette = Palette::current(ui);
    let current = slot.tool(cx.project, cx.op);
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
        for tool in offered {
            let chosen = current.is_some_and(|c| c.id == tool.id);
            let label = tool_text(cx.strings, cx.project, tool);
            let item = ui.selectable_label(chosen, label);
            if item.clicked() {
                commit(ui, refused_id, slot.set_tool(cx.op, &tool.id), out);
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
    label(ui, cx, &Slot::of(row));
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
