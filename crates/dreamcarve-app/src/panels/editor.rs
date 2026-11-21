// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operation editor (`docs/specs/phase-4/functional.md`, capabilities
//! 66 and 67; mock-up `05-operation-profile.svg`; `experience.md`, section
//! 7): the operation the list selected, its rows in their groups, each
//! group a section whose open state is a preference (`gui.toml`), a group
//! with no row not shown. The shell walks `dc_app::editor::fields`'s table
//! with one renderer per field kind ([`render`]'s `match`, which the
//! compiler keeps exhaustive); nothing here is per operation kind. Every
//! field shows its unit, commits when left, and shows its help with its
//! `.dcv` field name on hover of its label. What is wrong at a row (the
//! findings located on it, the refusal of what was typed) takes no line:
//! its control carries the outline and the glyph of the worst
//! ([`crate::widgets::problem`]), and the details are in the glyph's
//! tooltip, the help line and the Warnings drawer.
//!
//! Setup's Targets row is one line (slice 27, #619; capability 67): the
//! `target` glyph, the count and the names ([`targets::row_summary`]), a
//! warning glyph when a target carries one, a pencil that enters target
//! editing on the drawing (so does a double-click on the row) and a `⋯`
//! menu with *Use selection* and *Add selection*. There is no list of
//! names here: the targets are marked on the Viewport and in the Design
//! Tree. While target editing is on the row reads the draft.
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
//! height under itself.
//!
//! Slice 26 (#618, the review redesign; capabilities 66, 66a, 66b): the
//! header holds the name, renamed in place ([`header`]: click it, double-
//! click the band or press F2; Enter commits one step, Escape reverts);
//! *enabled* is the list's checkbox. Setup comes first and never closes;
//! every other group starts closed with its values summarised in its
//! header ([`editor::summary`]), and opens by itself, once per showing,
//! when it holds a located error or warning. An unset value the file needs
//! shows *Required* in its field, the message its tooltip. `check`'s carve
//! summary is no longer drawn here (slice 28 makes it a note in the
//! Warnings drawer).

use dc_app::editor::{self, FieldKind, Group, Row, Slot, fields, summary, targets};
use dc_app::operations::Kind;
use dc_app::problems::Problems;
use dc_app::{App, Command, Selection, StrKey, Strings};
use dc_job::Tier;
use dc_project::{InstanceId, Operation, OperationId, Project, Tool, ToolKind};
use eframe::egui::{self, Id, Rect, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button};
use crate::widgets::checkbox::checkbox as check_box;
use crate::widgets::icon::Icon;
use crate::widgets::op_glyph::op_glyph;
use crate::widgets::problem::{self, Place};
use crate::widgets::row::{self, GAP_PX};
use crate::widgets::section::{Header, section_with};
use crate::widgets::segmented::segmented;
use crate::widgets::toggle::toggle;
use crate::widgets::unit_field::{HEIGHT_PX, UnitField, field as unit_field};

/// The panel's content margin, px (`experience.md`, section 3).
const MARGIN_PX: i8 = 16;
/// A segment's widest label, px (section 7, "Segmented control").
const SEGMENT_LABEL_PX: f32 = 56.0;
/// How far a feature's or a variant's own rows indent, px (section 7,
/// "Toggle").
const NESTED_PX: f32 = 16.0;
/// The header band's height, px.
const HEADER_PX: f32 = 48.0;
/// The gap between a row's trailing icon buttons, px.
const BUTTON_GAP_PX: f32 = 2.0;

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
    /// The app, for what a row reads beyond the project (the draft of a
    /// target edit).
    app: &'a App,
    pub(super) strings: &'a Strings,
    pub(super) project: &'a Project,
    pub(super) located: editor::Located<'a>,
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
        app,
        strings,
        project,
        located: editor::Located::of(app.warnings(), op),
        findings: app.warnings(),
        selected: app.selection().instances(),
        op,
        focus,
    };
    // The operation's one colour, its row's.
    let colour = dc_app::operations::colour(project, op.id()).unwrap_or(0);
    let selected = matches!(app.selection(), Selection::Operation(id) if id == op.id());
    header(ui, &cx, colour, selected, out);
    let kind = Kind::of(op);
    let opening = opens_now(ui, op, &cx.located);
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt(("operation_editor", op.id().as_str()))
            .auto_shrink(false),
        crate::widgets::scroll::Surface::Panel,
        |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: MARGIN_PX,
                    right: MARGIN_PX,
                    top: 0,
                    bottom: MARGIN_PX,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP_PX, 4.0);
                    for (group, rows) in fields::groups(op) {
                        let name = group.section(kind);
                        let open = summary::open(&state.prefs.sections, kind, group);
                        let wants = opening.contains(&group)
                            || focus.is_some_and(|key| rows.iter().any(|r| r.key == key));
                        if !open && wants {
                            out.push(Action::RevealSection(name.clone()));
                        }
                        let summary = if open {
                            String::new()
                        } else {
                            summary::group(project, strings, op, group)
                        };
                        let clicked = section_with(
                            ui,
                            Id::unique(("dc.editor.group", name.as_str())),
                            strings.get(group.title()),
                            Header {
                                open,
                                located: cx.located.group(group),
                                summary: &summary,
                                collapses: group.collapses(),
                            },
                            |ui| {
                                for row in &rows {
                                    render(ui, &cx, row, &rows, out);
                                }
                            },
                        );
                        if clicked {
                            out.push(Action::SetSection { name, open: !open });
                        }
                    }
                });
        },
    );
}

/// The groups of `op` to open this frame: those holding a located error
/// or warning ([`summary::opened`]) that have not opened by themselves
/// since the editor began showing `op`, so each opens once and the user
/// can close it again (capability 66).
fn opens_now(ui: &Ui, op: &Operation, located: &editor::Located<'_>) -> Vec<Group> {
    let id = Id::unique("dc.editor.auto_opened");
    let (shown, mut done): (Option<OperationId>, Vec<Group>) = ui
        .data(|d| d.get_temp::<(OperationId, Vec<Group>)>(id))
        .map_or((None, Vec::new()), |(shown, done)| (Some(shown), done));
    if shown.as_ref() != Some(op.id()) {
        done.clear();
    }
    let now: Vec<Group> = summary::opened(op, located)
        .into_iter()
        .filter(|g| !done.contains(g))
        .collect();
    done.extend(now.iter().copied());
    ui.data_mut(|d| d.insert_temp(id, (op.id().clone(), done)));
    now
}

/// Where the header's rename field keeps its text, and whether it has
/// just opened (to select the name and take the focus).
fn rename_id(op: &Operation) -> Id {
    Id::unique(("dc.editor.rename", op.id().as_str()))
}

/// The card's header, with no band and no rule (capability 66; section 7,
/// "Editor header"): the
/// kind's glyph in the operation's colour, the name, editable in place,
/// `· Kind` in text.secondary, and the operation's worst located tier's
/// glyph at the right. Clicking the name, double-clicking the band, or F2
/// while the operation is the selection makes the name a field with the
/// name selected; Enter or leaving it commits one undo step
/// ([`editor::rename`]), Escape reverts. No enabled checkbox: that is the
/// list row's.
fn header(ui: &mut Ui, cx: &Cx<'_>, colour: usize, selected: bool, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let kind = Kind::of(cx.op);
    let name = cx.op.name();
    let title = cx.strings.fmt(
        StrKey::EditorTitle,
        &[("name", name), ("kind", cx.strings.get(kind.label()))],
    );
    // The band takes a double click and not the focus: the name is the
    // Tab stop.
    let (rect, band) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::CLICK);
    band.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &title));
    // Centred a little low, in the room the card's top padding leaves.
    let rect = rect.translate(egui::vec2(0.0, 2.0));
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + f32::from(MARGIN_PX) + 8.0, rect.center().y),
        egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
    );
    op_glyph(ui.painter(), glyph, kind, palette.operation(colour));

    // The worst located finding about the operation, at the right end.
    let worst = [Tier::Error, Tier::Warning, Tier::Hint]
        .into_iter()
        .find(|tier| {
            cx.findings
                .iter()
                .any(|f| f.op.as_ref() == Some(cx.op.id()) && f.tier == *tier)
        });
    // The Inspector's close button is at the header's right end.
    let mut right = rect.right() - crate::panels::side::CLOSE_RESERVE_PX;
    if let Some(tier) = worst {
        let (icon, colours) = match tier {
            Tier::Error => (Icon::CircleX, palette.status_error),
            Tier::Warning => (Icon::TriangleAlert, palette.status_warning),
            Tier::Hint => (Icon::Info, palette.status_info),
        };
        let galley = icon.galley(ui, theme::ICON_SMALL_PX, colours.stroke);
        right -= galley.size().x;
        ui.painter().galley(
            egui::pos2(right, rect.center().y - galley.size().y / 2.0),
            galley,
            colours.stroke,
        );
        right -= 8.0;
    }

    let font = TextStyle::Name(theme::TITLE.into()).resolve(ui.style());
    let kind_text = cx.strings.fmt(
        StrKey::EditorHeaderKind,
        &[("kind", cx.strings.get(kind.label()))],
    );
    let kind_galley = ui.painter().layout_no_wrap(
        kind_text,
        TextStyle::Body.resolve(ui.style()),
        palette.text_secondary,
    );
    let left = glyph.right() + 8.0;
    let name_room = (right - left - kind_galley.size().x - 8.0).max(24.0);
    let name_rect = Rect::from_min_max(
        egui::pos2(left - 8.0, rect.top() + 8.0),
        egui::pos2(left + name_room, rect.bottom() - 8.0),
    );

    let field_id = rename_id(cx.op);
    let renaming: Option<(String, bool)> = ui.data(|d| d.get_temp(field_id));
    let name_right = if let Some((mut text, fresh)) = renaming {
        let edit = ui.put(
            name_rect,
            egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .font(font)
                .desired_width(name_rect.width()),
        );
        let edit = edit.accessible_name(cx.strings.fmt(StrKey::EditorRename, &[("name", name)]));
        if fresh {
            edit.request_focus();
            if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), field_id) {
                s.cursor
                    .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
                s.store(ui.ctx(), field_id);
            }
        }
        // Left by any means, the name is written (capability 52).
        let left = edit.lost_focus() || crate::widgets::unit_field::left_with_focus(ui);
        if left && !fresh {
            ui.data_mut(|d| d.remove::<(String, bool)>(field_id));
            // The Viewport takes the Escape key before the editor draws,
            // and the focus router says when it left a field.
            let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape))
                || crate::shell::focus::escape_left_widget(ui.ctx());
            if !escaped {
                // A name the file refuses is said in the status bar, as a
                // shape's is: the header has no row for it.
                match editor::rename(cx.op, &text) {
                    Ok(command) => out.extend(command.map(Action::run)),
                    Err(e) => out.push(Action::Refused(e.to_string())),
                }
            }
        } else {
            ui.data_mut(|d| d.insert_temp(field_id, (text, false)));
        }
        name_rect.right()
    } else {
        let mut job =
            egui::text::LayoutJob::simple_singleline(name.to_owned(), font, palette.text_primary);
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_room);
        let galley = ui.painter().layout_job(job);
        let at = Rect::from_min_size(
            egui::pos2(left - 8.0, rect.top() + 8.0),
            egui::vec2(galley.size().x + 16.0, rect.height() - 16.0),
        );
        let response = ui
            .interact(at, field_id.with("name"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Text);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        if response.hovered() {
            ui.painter().rect_stroke(
                at,
                theme::CONTROL_RADIUS_PX,
                egui::Stroke::new(1.0, palette.border_strong),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text_primary,
        );
        let f2 = selected
            && ui.memory(|m| m.focused().is_none())
            && ui.input(|i| i.key_pressed(egui::Key::F2));
        if response.clicked() || band.double_clicked() || f2 {
            ui.data_mut(|d| d.insert_temp(field_id, (name.to_owned(), true)));
            ui.ctx().request_repaint();
        }
        at.right()
    };
    ui.painter().galley(
        egui::pos2(
            name_right + 4.0,
            rect.center().y - kind_galley.size().y / 2.0,
        ),
        kind_galley,
        palette.text_secondary,
    );
}

/// The id of the control at `key` (a row's key path, a clearing pass's
/// `clearing[1].tool`) for `op`: stable however the layout moves.
pub(super) fn control_id(op: &Operation, key: &str) -> Id {
    Id::unique(("dc.editor.field", op.id().as_str(), key))
}

/// Pushes `result`'s command, or keeps its refusal for the row's control
/// to mark itself with ([`refusal`]).
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

/// What is wrong at the control whose refusals are kept at `refused_id`:
/// the reason it refused what was typed, then `located`, the findings on
/// it.
pub(super) fn refusal(ui: &Ui, cx: &Cx<'_>, refused_id: Id, located: Problems) -> Problems {
    match ui.data(|d| d.get_temp::<String>(refused_id)) {
        Some(reason) => Problems::refused(cx.strings, &reason).with(located),
        None => located,
    }
}

/// One row among `siblings`, its group's rows: its renderer, marked with
/// what is wrong at it (the refusal of what was typed, the findings
/// located on it), then its notes. A problem draws no line, so the row
/// is as tall with one as without.
fn render(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &'static Row,
    siblings: &[&'static Row],
    out: &mut Vec<Action>,
) {
    let slot = Slot::of(row);
    let indent = row.indented_among(siblings.iter().copied());
    let refused_id = control_id(cx.op, slot.key()).with("refused");
    let refused = ui.data(|d| d.get_temp::<String>(refused_id)).is_some();
    let status = cx.located.status(row);
    // A block marks what is located on it and not on one of the fields
    // inside it, which mark themselves (a clearing pass's tool).
    let located = if row.kind.is_block() {
        let keys = crate::panels::clearing::keys(cx, row);
        cx.located.problems_but(cx.strings, cx.op, row, &keys)
    } else {
        cx.located.problems(cx.strings, row)
    };
    let mut problems = refusal(ui, cx, refused_id, located);
    // Nothing to derive a countersink's depth from: validation's reason
    // is the read-only field's problem.
    if matches!(row.kind, FieldKind::CountersinkDepth)
        && let Some(Err(why)) = editor::derived(cx.project, cx.strings, cx.findings, cx.op, row)
    {
        problems = problems.with(Problems::said(cx.strings, Tier::Error, &why));
    }
    let focused = cx.focus == Some(row.key);
    let offered = editor::offered_for(cx.project, cx.op, row);
    // An unset value with an error or a warning on it, and no default of
    // the tool's or the stock's, is required (capability 66b): *Required*
    // in the field, the message among its problems.
    let required = status.is_some_and(|s| {
        !refused
            && s.tier != Tier::Hint
            && typed_kind(row.kind)
            && slot.value_text(cx.project, cx.op).is_empty()
            && slot.placeholder(cx.project, cx.strings, cx.op).is_none()
    });
    let response = ui
        .horizontal(|ui| {
            nest(ui, indent);
            control(ui, cx, &slot, &offered, &problems, required, focused, out)
        })
        .inner;
    if matches!(row.kind, FieldKind::ClearingList) {
        crate::panels::clearing::list(ui, cx, row, out);
    }
    // The lines under it (`editor::notes`): under a tool picker a V-bit's
    // cone height or a tapered ball nose's cutting height (#685), the
    // board under an Inlay's band, and an Inlay's derived lines, the last
    // of which is the seat line when a tapered ball nose cuts it; an
    // alert in red with its glyph. The derived row has no field, so its
    // lines, which are sentences, take the row's whole width.
    // A stepover's distance between passes, from what its field shows.
    let live: Option<String> = ui.data(|d| d.get_temp(control_id(cx.op, slot.key()).with("live")));
    if let Some(note) =
        live.and_then(|typed| editor::step_over_note(cx.project, cx.strings, cx.op, row, &typed))
    {
        row::hint(ui, &note);
    }
    let whole_width = matches!(row.kind, FieldKind::InlayDerived);
    for note in editor::notes(cx.project, cx.strings, cx.op, &slot) {
        if whole_width {
            row::line(ui, note.alert, &note.text);
        } else {
            row::derived(ui, note.alert, &note.text);
        }
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
}

/// Whether a row of `kind` is a typed field, which can show the
/// required-field look.
const fn typed_kind(kind: FieldKind) -> bool {
    matches!(
        kind,
        FieldKind::Length
            | FieldKind::Rate
            | FieldKind::Angle
            | FieldKind::Percentage
            | FieldKind::Count
            | FieldKind::Seconds
            | FieldKind::LengthList
    )
}

/// The room a row drawn under its feature or variant starts with, when
/// `indent` (`Row::indented_among`: its parent is drawn with it).
pub(super) fn nest(ui: &mut Ui, indent: bool) {
    if indent {
        ui.add_space(NESTED_PX);
    }
}

/// The control of `slot`, by its kind: what a row of the operation and a
/// row of a clearing pass both draw, after [`nest`]. `offered` is what a
/// tool picker lists; `problems`, what is wrong at the control, which
/// each kind marks itself with (the outline and the glyph: inside a
/// field or a dropdown, beside a control that has no inside); `required`,
/// whether a typed field is an unset required value. Each control takes
/// the help its label offered: its tooltip, and the panel's help line
/// while it has the keyboard.
#[allow(
    clippy::too_many_arguments,
    reason = "one renderer's inputs, each read by some kind of row"
)]
pub(super) fn control(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    offered: &[&Tool],
    problems: &Problems,
    required: bool,
    focused: bool,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let id = control_id(cx.op, slot.key());
    // A row another replaces (an Inlay's Band margin under a drawn
    // boundary) is disabled, and says why where its field would be.
    if let Some(why) = editor::replaced(cx.op, slot.row) {
        label(ui, cx, slot);
        return Some(replaced(ui, cx, slot, id, why, problems));
    }
    let name = cx.strings.get(slot.row.label);
    match slot.row.kind {
        FieldKind::Length
        | FieldKind::Rate
        | FieldKind::Angle
        | FieldKind::Percentage
        | FieldKind::Count
        | FieldKind::Seconds
        | FieldKind::LengthList => {
            label(ui, cx, slot);
            typed(ui, cx, slot, id, problems, required, focused, out)
        }
        FieldKind::SwitchedLength => switched_length(ui, cx, slot, id, problems, focused, out),
        FieldKind::Choice(options) => choice(ui, cx, slot, id, options, problems, out),
        FieldKind::Checkbox => checkbox(ui, cx, slot, problems, out),
        FieldKind::Toggle(_) => switch(ui, cx, slot, problems, out),
        FieldKind::ToolPicker => tool_picker(ui, cx, slot, id, offered, problems, out),
        FieldKind::PicturePicker => picture_picker(ui, cx, slot.row, id, problems, out),
        FieldKind::BoundaryPicker => boundary_picker(ui, cx, slot.row, id, problems, out),
        FieldKind::InstanceList => {
            // Setup's Targets row: one line (capability 67).
            label(ui, cx, slot);
            Some(targets_row(ui, cx, slot, problems, out))
        }
        FieldKind::ClearingList => {
            // The list itself is under the row.
            label(ui, cx, slot);
            let palette = Palette::current(ui);
            let intro = problem::with_room(ui, problems, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(cx.strings.get(StrKey::EditorClearingIntro))
                            .text_style(TextStyle::Small)
                            .color(palette.text_secondary),
                    )
                    .wrap()
                    .selectable(false),
                )
            });
            Some(problem::mark(
                ui,
                intro,
                name,
                problems,
                None,
                Place::Beside,
            ))
        }
        FieldKind::TabRequest => tab_request(ui, cx, slot.row, problems, out),
        FieldKind::EarlierPocket => earlier_pocket(ui, cx, slot.row, id, problems, out),
        FieldKind::PointPick => point_pick_button(ui, cx, slot.row, problems),
        FieldKind::CountersinkDepth => {
            label(ui, cx, slot);
            Some(derived(ui, cx, slot.row, id, problems))
        }
        // The label; the derived lines are drawn under it. What is
        // located on the row marks the label.
        FieldKind::InlayDerived => {
            let label = label(ui, cx, slot);
            Some(problem::mark(
                ui,
                label,
                name,
                problems,
                None,
                Place::Beside,
            ))
        }
    }
}

/// The label with its help and the `.dcv` field name (a clearing pass's
/// with its index, `clearing[1].tool`).
fn label(ui: &mut Ui, cx: &Cx<'_>, slot: &Slot<'_>) -> egui::Response {
    row::label(
        ui,
        cx.strings.get(slot.row.label),
        cx.strings.get(slot.row.help),
        Some(slot.key()),
    )
}

/// A typed field: the unit field, its placeholder the tool's value while
/// `null`, and ⟲ while it has its own; while `required`, the
/// required-field look. `problems` are the field's own to show.
#[allow(
    clippy::too_many_arguments,
    reason = "one renderer's inputs, each read by some kind of row"
)]
fn typed(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    problems: &Problems,
    required: bool,
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
        .then(|| cx.strings.get(editor::reset_label(row)));
    let name = cx.strings.get(row.label);
    if focused {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let output = unit_field(
        ui,
        UnitField {
            placeholder: &placeholder,
            problems: Some(problems),
            reset,
            required: required.then(|| cx.strings.get(StrKey::EditorRequired)),
            ..UnitField::new(
                id,
                name,
                &value,
                editor::unit_suffix(row, cx.project.display_units),
            )
        },
    );
    // What the field shows now, for a note that follows the typing (a
    // stepover's distance between passes), which the row draws under it.
    ui.data_mut(|d| d.insert_temp(id.with("live"), output.text.clone()));
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

/// A row that is not in use (`editor::replaced`; capability 68a): a
/// disabled field with the reason in its place (*Set by the boundary*)
/// and the whole of it as the tooltip. It writes nothing. A value a
/// hand-edited file still holds shows greyed, marked with validation's
/// message as any other field is.
fn replaced(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    why: editor::Replaced,
    problems: &Problems,
) -> egui::Response {
    let value = slot.value_text(cx.project, cx.op);
    unit_field(
        ui,
        UnitField {
            enabled: false,
            disabled_reason: Some(cx.strings.get(why.help)),
            placeholder: cx.strings.get(why.note),
            problems: Some(problems),
            ..UnitField::new(
                id,
                cx.strings.get(slot.row.label),
                &value,
                if value.is_empty() {
                    ""
                } else {
                    editor::unit_suffix(slot.row, cx.project.display_units)
                },
            )
        },
    )
    .response
}

/// A derived value (S18; slice 12, #541): a read-only field showing
/// `derived: 5.5` in its unit, never typed. When there is nothing to
/// derive it from (the tool is not a V-bit), the field is empty and
/// marked with validation's reason, which [`render`] put among
/// `problems`.
fn derived(ui: &mut Ui, cx: &Cx<'_>, row: &Row, id: Id, problems: &Problems) -> egui::Response {
    let text = editor::derived(cx.project, cx.strings, cx.findings, cx.op, row)
        .and_then(Result::ok)
        .unwrap_or_default();
    unit_field(
        ui,
        UnitField {
            enabled: false,
            disabled_reason: Some(cx.strings.get(StrKey::EditorDerivedReadOnly)),
            problems: Some(problems),
            ..UnitField::new(
                id,
                cx.strings.get(row.label),
                &text,
                editor::unit_suffix(row, cx.project.display_units),
            )
        },
    )
    .response
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
    problems: &Problems,
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
        // Off, the switch is all there is to mark.
        let shape = crate::widgets::toggle::pill_of(&switch);
        let name = cx.strings.get(slot.row.label);
        return Some(problem::mark(
            ui,
            switch,
            name,
            problems,
            Some(shape),
            Place::Beside,
        ));
    }
    ui.add_space(GAP_PX);
    typed(ui, cx, slot, id, problems, false, focused, out)
}

/// A choice: segmented for two to four options whose labels fit 56 px
/// each, a dropdown otherwise (section 7).
fn choice(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    id: Id,
    options: &[editor::ChoiceOption],
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let row = slot.row;
    let name = cx.strings.get(row.label);
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
        // A pill control has no inside: its glyph stands beside it.
        let shown = problem::with_room(ui, problems, |ui| {
            ui.scope(|ui| {
                segmented(
                    ui,
                    id,
                    &labels,
                    current.unwrap_or(labels.len()),
                    ui.available_width(),
                )
            })
        });
        picked = shown.inner;
        let shape = (shown.response.rect, theme::pill(HEIGHT_PX));
        Some(problem::mark(
            ui,
            shown.response,
            name,
            problems,
            Some(shape),
            Place::Beside,
        ))
    } else {
        let selected = current
            .and_then(|i| labels.get(i))
            .copied()
            .unwrap_or_default();
        let width = ui.available_width();
        let response = egui::ComboBox::from_id_salt(id)
            .icon(crate::widgets::icon::combo_chevron)
            .truncate()
            .selected_text(problem::dropdown_text(ui, selected, width, problems))
            .width(width)
            .show_ui(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    if ui.selectable_label(current == Some(i), *label).clicked() {
                        picked = Some(i);
                    }
                }
            })
            .response
            .accessible_name(name);
        Some(dropdown_mark(ui, response, (name, selected), problems))
    };
    if let Some(index) = picked {
        commit(ui, refused_id, slot.choose(cx.project, cx.op, index), out);
    }
    response
}

/// A dropdown's finish: its help, and the mark of `problems` (the
/// outline, and the glyph inside it before the chevron, where `chosen`,
/// its text, was cut for it).
fn dropdown_mark(
    ui: &Ui,
    dropdown: egui::Response,
    (name, chosen): (&str, &str),
    problems: &Problems,
) -> egui::Response {
    let shape = (dropdown.rect, theme::CONTROL_RADIUS_PX);
    let place = problem::in_dropdown(ui);
    let marked = problem::mark(ui, dropdown, name, problems, Some(shape), place);
    problem::dropdown_whole(ui, marked, chosen)
}

/// A checkbox, its label to its right; hovering it shows the help. Its
/// problem's glyph stands beside it, and the outline goes round its box.
fn checkbox(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let palette = Palette::current(ui);
    let row = slot.row;
    let mut on = slot.checked(cx.op);
    let response = problem::with_room(ui, problems, |ui| {
        check_box(ui, slot.key(), &mut on, cx.strings.get(row.label))
    });
    let help = cx.strings.get(row.help);
    if response.has_focus() {
        let _ = crate::widgets::help::focused(ui, cx.strings.get(row.label), help);
    }
    let shape = crate::widgets::checkbox::box_of(&response, true);
    let response = problem::mark(
        ui,
        response,
        cx.strings.get(row.label),
        problems,
        Some(shape),
        Place::Beside,
    );
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

/// A feature's switch; its settings are the rows under its key. Its
/// problem's glyph stands beside it.
fn switch(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let on = slot.switched(cx.op);
    let name = cx.strings.get(slot.row.label);
    let response = toggle(ui, on, name);
    if response.clicked() {
        commit(
            ui,
            control_id(cx.op, slot.key()).with("refused"),
            slot.switch(cx.project, cx.op, !on),
            out,
        );
    }
    let shape = crate::widgets::toggle::pill_of(&response);
    Some(problem::mark(
        ui,
        response,
        name,
        problems,
        Some(shape),
        Place::Beside,
    ))
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

/// A tab request's switch, with *Place tabs* beside it (capability 69):
/// it turns the request into placed tabs, as `tabs place` does, and is
/// disabled with its reason while the request is off (#546).
fn tab_request(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    let response = switch(ui, cx, &Slot::of(row), problems, out);
    let requested = matches!(cx.op, Operation::Profile(p) if p.tabs.is_some());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let spec = ButtonSpec::new(
            ButtonKind::Secondary,
            cx.strings.get(StrKey::EditorPlaceTabs),
        )
        .tooltip(cx.strings.get(if requested {
            StrKey::EditorPlaceTabsHelp
        } else {
            StrKey::EditorPlaceTabsOff
        }))
        .enabled(requested);
        if button(ui, spec).clicked() {
            out.push(Action::PlaceTabs(cx.op.id().clone()));
        }
    });
    response
}

/// `rest_of`: none, or one of the earlier enabled Pockets, in a dropdown.
/// A file's `rest_of` that is not one of them (a later Pocket, written by
/// hand) shows as chosen, by name, marked with validation's message.
fn earlier_pocket(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    problems: &Problems,
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
        .icon(crate::widgets::icon::combo_chevron)
        .truncate()
        .selected_text(problem::dropdown_text(ui, &selected, width, problems))
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
    Some(dropdown_mark(
        ui,
        response,
        (cx.strings.get(row.label), &selected),
        problems,
    ))
}

/// A Photo V-Carve's Picture row (capability 67): a dropdown of the
/// project's pictures, the one carved shown as chosen. A picture the
/// project no longer has shows by the id the file still names, marked
/// with validation's message, so the row never reads as empty.
fn picture_picker(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, &Slot::of(row));
    let selected = editor::picture_text(cx.project, cx.strings, cx.op).unwrap_or_default();
    let current = editor::picture(cx.project, cx.op).map(|bitmap| &bitmap.id);
    let offered = editor::offered_pictures(cx.project);
    let mut picked: Option<dc_project::BitmapId> = None;
    let width = ui.available_width();
    let response = egui::ComboBox::from_id_salt(id)
        .icon(crate::widgets::icon::combo_chevron)
        .truncate()
        .selected_text(problem::dropdown_text(ui, &selected, width, problems))
        .width(width)
        .show_ui(ui, |ui| {
            for (picture, name) in &offered {
                if ui
                    .selectable_label(current == Some(picture), name)
                    .clicked()
                {
                    picked = Some(picture.clone());
                }
            }
        })
        .response
        .accessible_name(cx.strings.get(row.label));
    if let Some(picture) = picked {
        commit(
            ui,
            id.with("refused"),
            editor::set_picture(cx.op, row, &picture),
            out,
        );
    }
    Some(dropdown_mark(
        ui,
        response,
        (cx.strings.get(row.label), &selected),
        problems,
    ))
}

/// An Inlay's Boundary row (capability 68; phase 3's capability 49f): a
/// dropdown of *None* (the band grown from the design by the margin) and
/// the shapes validation takes as a drawn boundary, what the file lists
/// shown as chosen. Choosing a shape makes it the boundary alone; with
/// two or more such shapes selected on the drawing, *Selected shapes*
/// takes them all, for a band drawn by several. A listed shape the
/// project no longer has shows by its id, marked with validation's
/// message. The shapes are asked for only while the list is open: the
/// question nests each shape's outlines.
fn boundary_picker(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    id: Id,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, &Slot::of(row));
    let selected = editor::boundary_text(cx.project, cx.strings, cx.op).unwrap_or_default();
    let current = editor::boundary(cx.op);
    let mut picked: Option<Vec<InstanceId>> = None;
    let width = ui.available_width();
    let response = egui::ComboBox::from_id_salt(id)
        .icon(crate::widgets::icon::combo_chevron)
        .truncate()
        .selected_text(problem::dropdown_text(ui, &selected, width, problems))
        .width(width)
        .show_ui(ui, |ui| {
            let none = cx.strings.get(StrKey::EditorBoundaryNone);
            if ui.selectable_label(current.is_empty(), none).clicked() {
                picked = Some(Vec::new());
            }
            for (shape, name) in editor::offered_boundaries(cx.project, cx.op) {
                if ui
                    .selectable_label(current == std::slice::from_ref(&shape), name)
                    .clicked()
                {
                    picked = Some(vec![shape]);
                }
            }
            let several = editor::boundary_of_selection(cx.project, cx.op, cx.selected);
            if several.len() > 1 {
                ui.separator();
                let text = cx.strings.fmt(
                    StrKey::EditorBoundarySelection,
                    &[("count", &several.len().to_string())],
                );
                if ui.selectable_label(current == several, text).clicked() {
                    picked = Some(several);
                }
            }
        })
        .response
        .accessible_name(cx.strings.get(row.label));
    if let Some(shapes) = picked {
        commit(
            ui,
            id.with("refused"),
            // The boundary, and the margin and the rounding it replaces
            // cleared with it: one undo step.
            editor::choose_boundary(cx.op, row, &shapes),
            out,
        );
    }
    Some(dropdown_mark(
        ui,
        response,
        (cx.strings.get(row.label), &selected),
        problems,
    ))
}

/// A point row's *Start on the view*: arms the Viewport, whose next click
/// sets the point (one click, as the park point's pick); clicked again,
/// or Escape, disarms it.
fn point_pick_button(
    ui: &mut Ui,
    cx: &Cx<'_>,
    row: &Row,
    problems: &Problems,
) -> Option<egui::Response> {
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
    let shape = (response.rect, theme::CONTROL_RADIUS_PX);
    Some(problem::mark(
        ui,
        response,
        cx.strings.get(row.label),
        problems,
        Some(shape),
        Place::Beside,
    ))
}

/// A tool type's glyph (section 6).
pub(super) const fn tool_icon(kind: &ToolKind) -> Icon {
    match kind {
        ToolKind::EndMill => Icon::Cylinder,
        ToolKind::BallNose => Icon::CircleDot,
        // A tapered ball nose takes the V-bit's glyph until #685 gives it one.
        ToolKind::VBit { .. } | ToolKind::TaperedBallNose { .. } => Icon::Triangle,
        ToolKind::Engraver => Icon::PencilLine,
        ToolKind::Drill => Icon::MoveDown,
    }
}

/// Paints `kind`'s glyph (section 6) at 16 px centred on `centre` in
/// `colour`. Lucide's `triangle` points up, so the V-bit's is turned to
/// point down, as the spec draws it.
pub(super) fn paint_tool_icon(ui: &Ui, centre: egui::Pos2, kind: &ToolKind, colour: egui::Color32) {
    let galley = tool_icon(kind).galley(ui, theme::ICON_SMALL_PX, colour);
    let half = galley.size() / 2.0;
    if matches!(
        kind,
        ToolKind::VBit { .. } | ToolKind::TaperedBallNose { .. }
    ) {
        // A text shape turns about its top-left corner.
        let shape = egui::epaint::TextShape::new(centre + half, galley, colour)
            .with_angle(std::f32::consts::PI);
        ui.painter().add(shape);
    } else {
        ui.painter().galley(centre - half, galley, colour);
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

/// A picker row's padding at each end, px.
const PICKER_PAD_PX: f32 = 8.0;
/// The widest the picker's popover grows for a long name, px; a longer
/// name is cut, with the whole of it in the row's tooltip.
const PICKER_MAX_PX: f32 = 360.0;

/// `tool`'s size as a picker row shows it: its diameter, a typed value,
/// as its field reads (`3.175 mm`).
fn picker_size(tool: &Tool, units: dc_project::Units) -> String {
    format!(
        "{} {}",
        dc_app::units::field_text(tool.diameter_mm, units),
        dc_app::units::suffix(units)
    )
}

/// One row of the tool picker's popover (section 7, "Dropdown": "the tool
/// dropdown shows the tool-type glyph, name, size and `(T2)`"): `tool`'s
/// type glyph, `label` (its name and number), and its diameter at the
/// right in text.secondary; the current one on `accent.soft`, as a
/// dropdown's current row is. Heard as a button named `label`.
fn picker_row(ui: &mut Ui, cx: &Cx<'_>, tool: &Tool, chosen: bool, label: &str) -> egui::Response {
    let palette = Palette::current(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT_PX), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, chosen, label));
    let fill = if chosen {
        palette.accent_soft
    } else if response.hovered() {
        palette.surface_hover
    } else {
        theme::NONE
    };
    ui.painter()
        .rect_filled(rect, theme::CONTROL_RADIUS_PX, fill);
    let colour = if chosen {
        palette.accent
    } else {
        palette.text_primary
    };
    let pad = PICKER_PAD_PX;
    paint_tool_icon(
        ui,
        egui::pos2(
            rect.left() + pad + theme::ICON_SMALL_PX / 2.0,
            rect.center().y,
        ),
        &tool.cutter,
        palette.text_secondary,
    );
    let size = ui.painter().layout_no_wrap(
        picker_size(tool, cx.project.display_units),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let left = rect.left() + pad + theme::ICON_SMALL_PX + 6.0;
    let right = rect.right() - pad - size.size().x;
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        TextStyle::Body.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - 8.0 - left).max(0.0));
    let name = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - name.size().y / 2.0),
        name,
        colour,
    );
    ui.painter().galley(
        egui::pos2(right, rect.center().y - size.size().y / 2.0),
        size,
        palette.text_secondary,
    );
    crate::widgets::button::focus_ring(ui, &response);
    response.on_hover_text(label)
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
    problems: &Problems,
    out: &mut Vec<Action>,
) -> Option<egui::Response> {
    label(ui, cx, slot);
    let row = slot.row;
    let palette = Palette::current(ui);
    let current = slot.tool(cx.project, cx.op);
    // The room a problem's glyph takes before the chevron.
    let glyph_px = if problems.is_empty() {
        0.0
    } else {
        problem::GLYPH_PX + 6.0
    };
    let text = current.map_or_else(
        || cx.strings.get(StrKey::EditorToolNone).to_owned(),
        |t| tool_text(cx.strings, cx.project, t),
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT_PX), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ComboBox, true, cx.strings.get(row.label))
    });
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let outline = if open {
        palette.focus_ring
    } else if response.hovered() {
        palette.border_strong
    } else {
        palette.border_default
    };
    ui.painter().rect(
        rect,
        theme::CONTROL_RADIUS_PX,
        palette.surface_sunken,
        egui::Stroke::new(1.0, outline),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 10.0;
    if let Some(tool) = current {
        paint_tool_icon(
            ui,
            egui::pos2(x + theme::ICON_SMALL_PX / 2.0, rect.center().y),
            &tool.cutter,
            palette.text_secondary,
        );
        x += theme::ICON_SMALL_PX + 6.0;
    }
    let chevron = Icon::ChevronDown.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    // One line, an ellipsis before the chevron when the name is long.
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.clone(),
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (rect.right() - 10.0 - chevron.size().x - 6.0 - glyph_px - x).max(0.0),
    );
    let name = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name.size().y / 2.0),
        name,
        palette.text_primary,
    );
    ui.painter().galley(
        egui::pos2(
            rect.right() - 10.0 - chevron.size().x,
            rect.center().y - chevron.size().y / 2.0,
        ),
        chevron,
        palette.text_secondary,
    );
    let refused_id = id.with("refused");
    egui::Popup::menu(&response).show(|ui| {
        // As wide as its field (the popover's margin and outline are its
        // own), and wider where a tool's name and size need it: a picker
        // that cuts its names is no picker.
        let field = rect.width() - 2.0 * (ui.spacing().menu_margin.leftf() + 1.0);
        let body = TextStyle::Body.resolve(ui.style());
        let small = TextStyle::Small.resolve(ui.style());
        let units = cx.project.display_units;
        let widest = offered
            .iter()
            .map(|tool| {
                let name = tool_text(cx.strings, cx.project, tool);
                let name_px = ui
                    .painter()
                    .layout_no_wrap(name, body.clone(), palette.text_primary)
                    .size()
                    .x;
                let size_px = ui
                    .painter()
                    .layout_no_wrap(
                        picker_size(tool, units),
                        small.clone(),
                        palette.text_secondary,
                    )
                    .size()
                    .x;
                PICKER_PAD_PX
                    + theme::ICON_SMALL_PX
                    + 6.0
                    + name_px
                    + 16.0
                    + size_px
                    + PICKER_PAD_PX
            })
            .fold(field, f32::max);
        let inner = widest.min(PICKER_MAX_PX).ceil();
        ui.set_min_width(inner);
        ui.set_max_width(inner);
        for tool in offered {
            let chosen = current.is_some_and(|c| c.id == tool.id);
            let label = tool_text(cx.strings, cx.project, tool);
            let item = picker_row(ui, cx, tool, chosen, &label);
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
    let place = Place::Inside {
        from_right_px: 10.0 + theme::ICON_SMALL_PX + 6.0 + problem::GLYPH_PX / 2.0,
        fill: palette.surface_sunken,
    };
    Some(problem::mark(
        ui,
        response,
        cx.strings.get(row.label),
        problems,
        Some((rect, theme::CONTROL_RADIUS_PX)),
        place,
    ))
}

/// Setup's Targets row (capability 67; `experience.md`, section 7,
/// "Targets row"): `target`, then `2 shapes · pocket, island` cut at the
/// row's end with every name in its tooltip, a `triangle-alert` when a
/// target carries a warning, and at the right end the pencil (*Edit
/// targets*) and `⋯` (*Use selection*, *Add selection*). A double-click
/// on the summary enters target editing too. What is located on the row
/// (a kind's `drill_target_not_circle`, the refusal of a selection)
/// outlines the summary and puts its glyph where the triangle stands.
fn targets_row(
    ui: &mut Ui,
    cx: &Cx<'_>,
    slot: &Slot<'_>,
    problems: &Problems,
    out: &mut Vec<Action>,
) -> egui::Response {
    let palette = Palette::current(ui);
    let strings = cx.strings;
    let op = cx.op;
    let summary = targets::row_summary(cx.app, op);
    let refused_id = control_id(op, slot.key()).with("refused");
    // The two icon buttons sit together at the row's end, so the names
    // keep what room there is.
    ui.spacing_mut().item_spacing.x = BUTTON_GAP_PX;
    let buttons = 2.0 * (HEIGHT_PX + BUTTON_GAP_PX);
    let width = (ui.available_width() - buttons).max(0.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT_PX), Sense::hover());
    // A double click on the summary edits the targets, as the pencil
    // beside it does; the pencil is the Tab stop.
    let response = ui.interact(rect, control_id(op, slot.key()), Sense::CLICK);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &summary.text));
    let colour = if ui.is_enabled() {
        palette.text_primary
    } else {
        palette.text_disabled
    };
    let glyph = Icon::Target.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    let mut left = rect.left();
    ui.painter().galley(
        egui::pos2(left, rect.center().y - glyph.size().y / 2.0),
        glyph,
        palette.text_secondary,
    );
    left += theme::ICON_SMALL_PX + 6.0;
    let mut right = rect.right();
    if !problems.is_empty() {
        // The row's own glyph, drawn by its mark, takes the place.
        right -= problem::GLYPH_PX + 4.0;
    } else if summary.warning {
        let alert =
            Icon::TriangleAlert.galley(ui, theme::ICON_SMALL_PX, palette.status_warning.stroke);
        right -= theme::ICON_SMALL_PX;
        ui.painter().galley(
            egui::pos2(right, rect.center().y - alert.size().y / 2.0),
            alert,
            palette.status_warning.stroke,
        );
        right -= 6.0;
    }
    // The count and the names where they fit whole; else the count
    // alone, and the names in the tooltip (`RowSummary::short`).
    let font = TextStyle::Body.resolve(ui.style());
    let room = (right - left).max(0.0);
    let whole = ui
        .painter()
        .layout_no_wrap(summary.text.clone(), font.clone(), colour);
    let shown = if whole.size().x <= room {
        &summary.text
    } else {
        &summary.short
    };
    let mut job = egui::text::LayoutJob::simple_singleline(shown.clone(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    if response.double_clicked() {
        out.push(Action::EditTargets(op.id().clone()));
    }
    let response = problem::mark(
        ui,
        response,
        cx.strings.get(slot.row.label),
        problems,
        // The summary has no frame of its own: the outline stands a
        // little off its glyph and its text, short of the pencil.
        Some((
            Rect::from_min_max(
                egui::pos2(rect.left() - 6.0, rect.top()),
                egui::pos2(rect.right() + 2.0, rect.bottom()),
            ),
            theme::CONTROL_RADIUS_PX,
        )),
        Place::Inside {
            from_right_px: 2.0 + problem::GLYPH_PX / 2.0,
            fill: theme::NONE,
        },
    );
    let response = if summary.names.is_empty() {
        response
    } else {
        response.on_hover_text(&summary.names)
    };

    let edit_name = strings.get(StrKey::EditorTargetsEdit);
    let edit = ButtonSpec::new(ButtonKind::Quiet, "")
        .icon(Icon::Pencil, theme::ICON_SMALL_PX)
        .tooltip(edit_name)
        .width(HEIGHT_PX);
    if button(ui, edit).clicked() {
        out.push(Action::EditTargets(op.id().clone()));
    }
    let menu_name = strings.get(StrKey::EditorTargetsMenu);
    let more = button(
        ui,
        ButtonSpec::new(ButtonKind::Quiet, "")
            .icon(Icon::Ellipsis, theme::ICON_SMALL_PX)
            .tooltip(menu_name)
            .width(HEIGHT_PX),
    );
    egui::Popup::menu(&more).show(|ui| {
        let has = !cx.selected.is_empty();
        let reason = strings.get(StrKey::EditorTargetsSelectFirst);
        let item = |ui: &mut Ui, key: StrKey| {
            ui.add_enabled(has, egui::Button::new(strings.get(key)))
                .on_disabled_hover_text(reason)
                .clicked()
        };
        if item(ui, StrKey::EditorTargetsUseSelection) {
            commit(ui, refused_id, targets::replace(op, cx.selected), out);
            ui.close();
        }
        if item(ui, StrKey::EditorTargetsAddSelection) {
            commit(ui, refused_id, targets::add(op, cx.selected), out);
            ui.close();
        }
    });
    response
}
