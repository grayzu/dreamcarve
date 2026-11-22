// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Warnings drawer (functional spec, capability 76; mock-up
//! `10-warnings-panel.svg`; `experience.md`, section 2, "Warnings
//! drawer"; `redesign.md`, "The window"): a card over the bottom of the
//! Viewport's card, 240 px high, with a hairline and the one shadow,
//! 8 px in from the card's edges; never a window. The grab handle at its
//! top drags it taller or shorter, down to 120 px. The Viewport keeps its
//! size under it, and its read-out and Fit stand clear of it
//! ([`reserve`]). Its rows are `dc_app::warnings::panel`'s, `check` on the
//! project as it stands: grouped by operation with the project's rows
//! first, each with its tier glyph and word, its code in mono linking to
//! the code's entry in the Warnings chapter, the message with the
//! operation, tool or shape named where the engine's text gives its id
//! (`warnings::Row::shown`), and the field it is about. Clicking a row selects the operation or the shape it
//! names, and is named for assistive tools as the drawer words it
//! (`warnings::Row::name`: a hint is a hint, where `check`'s line for it
//! says `warning`); a row on an operation's field opens the operation in the editor
//! with that field focused (#539), and a row on a tool's, the stock's or
//! an output setting's field opens its step with that field focused.
//! *Copy* puts
//! `check`'s text on the clipboard, and its tooltip says that `check`
//! prints a hint as a warning. A click on a field's problem glyph opens
//! the drawer at the finding's row, the reverse trip: the row is scrolled
//! into view once and drawn selected (`accent.soft` with the accent bar,
//! as a list's selected row is) until a row is clicked or the drawer
//! closes (capability 66). Each group is named above its rows
//! only when there are two or more (#616). An operation's carve note is
//! the last row of its group, its facts laid out as labelled lines under
//! a board's sub-heading (#620). The artwork checks of the shapes
//! (`shapes_overlap`, `feature_below_size`; acceptance 106) are rows
//! like any warning's, and the overlap's selects the two shapes it names
//! on the Design step. The drawer has no Regenerate
//! button, since *Regenerate all* lives in the status bar after Stop
//! (capability 71).

use dc_app::StrKey;
use dc_app::status::counted;
use dc_app::warnings::{self, CarveNote, Counts, Group, Level, Row};
use eframe::egui::{self, Id, Rect, Sense, TextStyle, Ui, UiBuilder};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::card::floating;
use crate::widgets::chip::{Tone, chip};
use crate::widgets::icon::Icon;
use crate::widgets::row::INSET_PX;

/// The drawer's header, px: a control under the grab handle, and the gap
/// under it.
const HEADER_PX: f32 = 44.0;
/// The strip at the drawer's top that drags it taller or shorter, px.
const GRAB_PX: f32 = 14.0;
/// The grab handle's mark, px.
const GRAB_MARK: egui::Vec2 = egui::vec2(36.0, 4.0);
/// How far the drawer sits in from the Viewport card's edges, px.
const INSET_OVER_PX: f32 = theme::GAP_PX;
/// The least the drawer leaves of the Viewport above it, px: its
/// floating controls stay in view.
const ABOVE_PX: f32 = 96.0;
/// A group's header, px.
const GROUP_PX: f32 = theme::SECTION_HEADER_PX;
/// A row, px (`experience.md`, section 7, "Table rows").
const ROW_PX: f32 = theme::TABLE_ROW_PX;
/// A row's content margin, px.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;
/// A line of a carve note, px (the mock-up's 22-px fact lines).
const NOTE_LINE_PX: f32 = 22.0;
/// Where a carve note's facts start, from the row's left edge, px: past
/// the glyph and the word, where other rows' messages run.
const NOTE_FACTS_PX: f32 = 96.0;
/// A carve note's label column, px.
const NOTE_LABEL_PX: f32 = 140.0;
/// The drawer's height when first opened, px.
const DEFAULT_PX: f32 = 240.0;
/// Its least height, px.
const MIN_PX: f32 = 120.0;

/// The tier's glyph, colour and word (`warnings::level_word`: the word a
/// row's name starts with, so the chip and the name agree).
fn tier(level: Level, palette: &Palette) -> (Icon, egui::Color32, StrKey) {
    let (icon, colour) = match level {
        Level::Error => (Icon::CircleX, palette.status_error.text),
        Level::Warning => (Icon::TriangleAlert, palette.status_warning.text),
        Level::Hint => (Icon::Info, palette.status_info.text),
        Level::Note => (Icon::Info, palette.text_secondary),
    };
    (icon, colour, warnings::level_word(level))
}

/// The drawer's title: `Warnings · 1 error · 3 warnings · 1 hint`.
fn heading(strings: &dc_app::Strings, counts: Counts) -> String {
    let parts: Vec<String> = [
        (
            counts.errors,
            StrKey::AppStatusError,
            StrKey::AppStatusErrors,
        ),
        (
            counts.warnings,
            StrKey::AppStatusWarning,
            StrKey::AppStatusWarnings,
        ),
        (
            counts.hints,
            StrKey::AppWarningsHint,
            StrKey::AppWarningsHints,
        ),
        (
            counts.notes,
            StrKey::AppWarningsNote,
            StrKey::AppWarningsNotes,
        ),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| counted(strings, n, one, many))
    .collect();
    let counts = if parts.is_empty() {
        strings.get(StrKey::AppStatusNoWarnings).to_owned()
    } else {
        parts.join(strings.get(StrKey::AppWarningsSeparator))
    };
    strings.fmt(StrKey::AppWarningsHeading, &[("counts", &counts)])
}

/// The title of `group`: *Project*, or the operation's name and kind.
fn group_title(state: &FrameState<'_>, group: &Group) -> String {
    let strings = state.app.strings();
    let Some(id) = &group.op else {
        return strings.get(StrKey::AppWarningsProject).to_owned();
    };
    let project = state.app.project();
    let Some(op) = project.operations.iter().find(|o| o.id() == id) else {
        return id.to_string();
    };
    let kind = match op {
        dc_project::Operation::Profile(_) => StrKey::AppWarningsKindProfile,
        dc_project::Operation::Pocket(_) => StrKey::AppWarningsKindPocket,
        dc_project::Operation::Drill(_) => StrKey::AppWarningsKindDrill,
        dc_project::Operation::Bore(_) => StrKey::AppWarningsKindBore,
        dc_project::Operation::Engrave(_) => StrKey::AppWarningsKindEngrave,
        dc_project::Operation::VCarve(_) => StrKey::AppWarningsKindVCarve,
        dc_project::Operation::Inlay(_) => StrKey::AppWarningsKindInlay,
        // Format 27 (#644): the kind's own name; its rows are #544's.
        dc_project::Operation::PhotoVCarve(_) => StrKey::OperationsKindPhotoVCarve,
    };
    strings.fmt(
        StrKey::AppWarningsGroup,
        &[("name", op.name()), ("kind", strings.get(kind))],
    )
}

/// Where the drawer's height is kept between frames.
fn height_id() -> Id {
    Id::unique("dc.warnings.height")
}

/// The drawer's height, px, as the user left it.
fn height(ctx: &egui::Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(height_id()))
        .unwrap_or(DEFAULT_PX)
        .max(MIN_PX)
}

/// The room the open drawer takes of the Viewport card's bottom, px: its
/// height and its inset from the card's edge. What the Viewport's
/// read-out and Fit stand clear of.
pub(crate) fn reserve(ctx: &egui::Context) -> f32 {
    height(ctx) + INSET_OVER_PX
}

/// Draws the drawer over the bottom of the Viewport's card `viewport`,
/// short of `right_px` of its right side (an Inspector lying over it in
/// a narrow window); the drawer's rectangle. It is drawn after the
/// Viewport, in its layer, and takes every click on it, so none reaches
/// the drawing under it; the Viewport leaves the pointer over it alone
/// (`viewport::Margins::overlay_bottom`). Not an area of its own: egui
/// lays a new area out unseen for a frame, and the drawer answers the
/// click that opens it in the next one (acceptance 87).
pub(crate) fn drawer(
    ui: &mut Ui,
    state: &FrameState<'_>,
    viewport: Rect,
    right_px: f32,
    out: &mut Vec<Action>,
) -> Rect {
    let groups = warnings::panel(state.app);
    let counts = warnings::counts(state.app);
    let tallest = (viewport.height() - ABOVE_PX - INSET_OVER_PX).max(MIN_PX);
    let tall = height(ui.ctx()).min(tallest);
    let rect = Rect::from_min_max(
        egui::pos2(
            viewport.left() + INSET_OVER_PX,
            viewport.bottom() - INSET_OVER_PX - tall,
        ),
        egui::pos2(
            (viewport.right() - INSET_OVER_PX - right_px).max(viewport.left() + INSET_OVER_PX),
            viewport.bottom() - INSET_OVER_PX,
        ),
    );
    ui.interact(
        rect,
        Id::unique("dc.warnings.drawer.surface"),
        Sense::click_and_drag(),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("dc.warnings.drawer")
            .max_rect(rect),
    );
    floating(&mut child, |ui| {
        if state.inert() {
            ui.disable();
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        grab(ui, tall, tallest);
        header(ui, state, counts, out);
        let palette = Palette::current(ui);
        crate::widgets::scroll::show(
            ui,
            egui::ScrollArea::vertical().auto_shrink(false),
            crate::widgets::scroll::Surface::Panel,
            |ui| {
                if groups.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        text(
                            ui,
                            state.app.strings().get(StrKey::AppWarningsNone),
                            TextStyle::Body,
                            palette.text_secondary,
                        );
                    });
                }
                let mut stripe = false;
                for group in &groups {
                    if group.heading {
                        group_header(ui, &group_title(state, group));
                    }
                    for row in &group.rows {
                        match &row.note {
                            Some(note) => draw_note(ui, state, row, note, stripe, out),
                            None => draw_row(ui, state, row, stripe, out),
                        }
                        stripe = !stripe;
                    }
                }
                ui.add_space(INSET_PX);
            },
        );
    });
    rect
}

/// The grab handle: a strip across the drawer's top with a small pill at
/// its centre. Dragging it makes the drawer taller or shorter, between
/// its least height and `tallest`; the pill takes the accent while the
/// pointer is on it.
fn grab(ui: &mut Ui, tall: f32, tallest: f32) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), GRAB_PX), Sense::hover());
    let response = ui.interact(rect, Id::unique("dc.warnings.grab"), Sense::drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.dragged() {
        let next = (tall - response.drag_delta().y).clamp(MIN_PX, tallest);
        ui.data_mut(|d| d.insert_temp(height_id(), next));
    }
    let colour = if response.hovered() || response.dragged() {
        palette.accent
    } else {
        palette.border_strong
    };
    let mark = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 4.0 + GRAB_MARK.y / 2.0),
        GRAB_MARK,
    );
    ui.painter()
        .rect_filled(mark, theme::pill(GRAB_MARK.y), colour);
}

/// The header: the title left; *Copy* and the close button right. No
/// band and no rule under it.
fn header(ui: &mut Ui, state: &FrameState<'_>, counts: Counts, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    let mut band = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(MARGIN_PX, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    text(
        &mut band,
        &heading(strings, counts),
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    band.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let close = strings.get(StrKey::AppWarningsClose);
        if button(
            ui,
            ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::X, theme::ICON_SMALL_PX)
                .tooltip(close),
        )
        .clicked()
        {
            out.push(Action::ToggleWarnings);
        }
        if button(
            ui,
            ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppWarningsCopy))
                .tooltip(strings.get(StrKey::AppWarningsCopyHelp)),
        )
        .clicked()
        {
            out.push(Action::CopyWarnings);
        }
    });
}

/// A group's 32-px header.
fn group_header(ui: &mut Ui, title: &str) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), GROUP_PX), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + MARGIN_PX, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
}

/// A row's surface in the `slot` the list gave it: inset from the card's
/// edges at the control radius, `surface.hover` under the pointer, else
/// `surface.stripe` on every other row.
fn row_surface(ui: &Ui, slot: egui::Rect, hovered: bool, stripe: bool) {
    let palette = Palette::current(ui);
    let fill = if hovered {
        palette.surface_hover
    } else if stripe {
        palette.surface_stripe
    } else {
        return;
    };
    ui.painter().rect_filled(
        slot.shrink2(egui::vec2(INSET_PX, 0.0)),
        theme::CONTROL_RADIUS_PX,
        fill,
    );
}

/// A carve note (capability 76a; mock-up `10-warnings-panel.svg`): the
/// quiet `info` glyph and *note* in secondary text, then its facts one per
/// line, each label in secondary text before its value, under a board's
/// sub-heading when there are two. Clicking it selects its operation; the
/// status bar's message cell opens the drawer scrolled to it.
fn draw_note(
    ui: &mut Ui,
    state: &FrameState<'_>,
    row: &Row,
    note: &CarveNote,
    stripe: bool,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let lines: usize = note
        .sections
        .iter()
        .map(|s| s.facts.len() + usize::from(s.heading.is_some()))
        .sum();
    #[allow(clippy::cast_precision_loss, reason = "a note has a handful of lines")]
    let height = (NOTE_LINE_PX * lines as f32 + 12.0).max(ROW_PX);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.name));
    row_surface(ui, rect, response.hovered(), stripe);
    if state.note_target.is_some() && state.note_target == row.op.as_ref() {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    if response.clicked()
        && let Some(selection) = row.selection()
    {
        out.push(Action::Select(selection));
    }

    let (icon, colour, word) = tier(row.level, palette);
    let painter = ui.painter_at(rect);
    let left = rect.left() + MARGIN_PX;
    let first_y = rect.top() + 6.0 + NOTE_LINE_PX / 2.0;
    let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    painter.galley(
        egui::pos2(left, first_y - glyph.size().y / 2.0),
        glyph,
        colour,
    );
    let small = TextStyle::Small.resolve(ui.style());
    let body = TextStyle::Body.resolve(ui.style());
    let title = TextStyle::Name(theme::TITLE.into()).resolve(ui.style());
    painter.text(
        egui::pos2(left + theme::ICON_SMALL_PX + 8.0, first_y),
        egui::Align2::LEFT_CENTER,
        strings.get(word),
        small,
        colour,
    );
    let label_x = left + NOTE_FACTS_PX;
    let value_x = label_x + NOTE_LABEL_PX;
    let mut y = first_y;
    for section in &note.sections {
        if let Some(heading) = &section.heading {
            painter.text(
                egui::pos2(label_x, y),
                egui::Align2::LEFT_CENTER,
                heading,
                title.clone(),
                palette.text_primary,
            );
            y += NOTE_LINE_PX;
        }
        for fact in &section.facts {
            painter.text(
                egui::pos2(label_x, y),
                egui::Align2::LEFT_CENTER,
                &fact.label,
                body.clone(),
                palette.text_secondary,
            );
            painter.text(
                egui::pos2(value_x, y),
                egui::Align2::LEFT_CENTER,
                &fact.value,
                body.clone(),
                palette.text_primary,
            );
            y += NOTE_LINE_PX;
        }
    }
}

/// One row: the tier as a status chip (glyph and word), the code as a
/// link, the message, the field; clicking it selects what it names. The
/// message is read here, so it is never cut: it wraps in the room after
/// the code, and the row grows by its lines.
fn draw_row(ui: &mut Ui, state: &FrameState<'_>, row: &Row, stripe: bool, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let (icon, _, word) = tier(row.level, palette);
    let tone = match row.level {
        Level::Error => Tone::Status(palette.status_error, icon),
        Level::Warning => Tone::Status(palette.status_warning, icon),
        Level::Hint => Tone::Status(palette.status_info, icon),
        Level::Note => Tone::Neutral,
    };
    // What stands before the message and after it, to know its room.
    let gap = 8.0;
    let mono = TextStyle::Monospace.resolve(ui.style());
    let width_of = |text: &str, colour: egui::Color32| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), mono.clone(), colour)
            .size()
            .x
    };
    let chip_px = crate::widgets::chip::Chip::new(ui, strings.get(word), tone)
        .size()
        .x;
    let link = row.code.as_ref().zip(row.chapter_url());
    let code_px = link
        .as_ref()
        .map_or(0.0, |(code, _)| width_of(code, palette.accent) + gap);
    let field_px = row
        .field
        .as_ref()
        .map_or(0.0, |field| width_of(field, palette.text_secondary) + gap);
    let inner = ui.available_width() - 2.0 * MARGIN_PX;
    let body = TextStyle::Body.resolve(ui.style());
    // Where the message and its field fit one line, the field stands at
    // the row's right end; where they do not, the message has the whole
    // width after the code and the field goes under it, as the Export
    // step lists a finding.
    let room = (inner - chip_px - gap - code_px).max(120.0);
    let natural = ui
        .painter()
        .layout_no_wrap(row.shown.clone(), body.clone(), palette.text_primary)
        .size()
        .x;
    let field_beside = natural + field_px <= room;
    let message = ui
        .painter()
        .layout(row.shown.clone(), body, palette.text_primary, room);
    let line_px = ui.text_style_height(&TextStyle::Body);
    let field_line_px = ui.text_style_height(&TextStyle::Monospace) + 2.0;
    let under_px = if row.field.is_some() && !field_beside {
        field_line_px
    } else {
        0.0
    };
    let height = ROW_PX.max(message.size().y + under_px + (ROW_PX - line_px));
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    // The row a field's glyph opened the drawer at: selected, and brought
    // into view the once.
    let selected = state.finding.is_some_and(|key| *key == row.key());
    if selected {
        response
            .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, true, &row.name));
        crate::widgets::row::surface(
            ui,
            rect.shrink2(egui::vec2(INSET_PX, 0.0)),
            true,
            response.hovered(),
        );
        if state.finding_scroll {
            response.scroll_to_me(Some(egui::Align::Center));
        }
    } else {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.name));
        row_surface(ui, rect, response.hovered(), stripe);
    }
    // What the row names is selected, and the step that edits it opens:
    // an operation's field in its editor, a shape on Design, a tool or a
    // setting in its own step.
    if response.clicked() {
        out.push(Action::OpenRow {
            selection: row.selection(),
            op: row.op.clone(),
            field: row.field.clone(),
        });
    }

    // The chip, the code and the field stand on the message's first
    // line: a strip one row tall at the row's top.
    let first = Rect::from_min_size(rect.min, egui::vec2(rect.width(), ROW_PX));
    let mut cells = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(first.shrink2(egui::vec2(MARGIN_PX, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    cells.spacing_mut().item_spacing.x = gap;
    // The tier: a status chip, its glyph and word on its tint.
    chip(&mut cells, strings.get(word), tone);
    if let Some((code, url)) = link {
        let link = cells
            .add(
                egui::Label::new(
                    egui::RichText::new(code)
                        .text_style(TextStyle::Monospace)
                        .color(palette.accent),
                )
                .selectable(false)
                .sense(Sense::click()),
            )
            .on_hover_text(strings.fmt(StrKey::AppWarningsChapter, &[("code", code)]));
        if link.clicked() {
            out.push(Action::OpenUrl(url));
        }
    }
    // The message, whole, from the first line down.
    let at = egui::pos2(cells.cursor().left(), rect.top() + (ROW_PX - line_px) / 2.0);
    let place = Rect::from_min_size(at, message.size());
    ui.interact(place, response.id.with("message"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &row.shown));
    ui.painter().galley(at, message, palette.text_primary);
    if let Some(field) = &row.field {
        let (anchor, align) = if field_beside {
            (
                egui::pos2(rect.right() - MARGIN_PX, first.center().y),
                egui::Align2::RIGHT_CENTER,
            )
        } else {
            (
                egui::pos2(at.x, place.bottom() + field_line_px / 2.0),
                egui::Align2::LEFT_CENTER,
            )
        };
        let place = ui
            .painter()
            .text(anchor, align, field, mono, palette.text_secondary);
        ui.interact(place, response.id.with("field"), Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, field));
    }
}
