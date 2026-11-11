// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Warnings drawer (functional spec, capability 76; mock-up
//! `10-warnings-panel.svg`; `experience.md`, section 2, "Warnings
//! drawer"): above the status bar over the Viewport's bottom 240 px,
//! resizable down to 120, on `surface.panel` under a rule; never a
//! window. Its rows are `dc_app::warnings::panel`'s, `check` on the
//! project as it stands: grouped by operation with the project's rows
//! first, each with its tier glyph and word, its code in mono linking to
//! the code's entry in the Warnings chapter, the message, and the field
//! it is about. Clicking a row selects the operation or the shape it
//! names; a row on an operation's field opens the operation in the editor
//! with that field focused (#539). *Copy* puts
//! `check`'s text on the clipboard. Each group is named above its rows
//! only when there are two or more (#616). An operation's carve note is
//! the last row of its group, its facts laid out as labelled lines under
//! a board's sub-heading (#620). The drawer has no Regenerate
//! button, since *Regenerate all* lives in the status bar after Stop
//! (capability 71).

use dc_app::StrKey;
use dc_app::status::counted;
use dc_app::warnings::{self, CarveNote, Counts, Group, Level, Row};
use eframe::egui::{self, Rangef, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;

/// The drawer's header band, px.
const HEADER_PX: f32 = 32.0;
/// A group's header, px.
const GROUP_PX: f32 = 28.0;
/// A row, px (`experience.md`, section 7, "Table rows").
const ROW_PX: f32 = 28.0;
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

/// The tier's glyph, colour and word.
fn tier(level: Level, palette: &Palette) -> (Icon, egui::Color32, StrKey) {
    match level {
        Level::Error => (
            Icon::CircleX,
            palette.status_error.text,
            StrKey::AppWarningsLevelError,
        ),
        Level::Warning => (
            Icon::TriangleAlert,
            palette.status_warning.text,
            StrKey::AppWarningsLevelWarning,
        ),
        Level::Hint => (
            Icon::Info,
            palette.status_info.text,
            StrKey::AppWarningsLevelHint,
        ),
        Level::Note => (
            Icon::Info,
            palette.text_secondary,
            StrKey::AppWarningsLevelNote,
        ),
    }
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

/// Draws the drawer.
pub(crate) fn drawer(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let groups = warnings::panel(state.app);
    let counts = warnings::counts(state.app);
    egui::Panel::bottom("warnings_drawer")
        .resizable(true)
        .show_separator_line(false)
        .default_size(DEFAULT_PX)
        .size_range(Rangef::new(MIN_PX, f32::INFINITY))
        .frame(egui::Frame::NONE.fill(palette.surface_panel))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(
                full.x_range(),
                full.top(),
                egui::Stroke::new(1.0, palette.border_default),
            );
            header(ui, state, counts, out);
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
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
                });
        });
}

/// The 32-px header: the title left; *Copy* and the close button right.
fn header(ui: &mut Ui, state: &FrameState<'_>, counts: Counts, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::hover());
    let mut band = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(12.0, 0.0)))
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
            ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppWarningsCopy)),
        )
        .clicked()
        {
            out.push(Action::CopyWarnings);
        }
    });
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.border_default),
    );
}

/// A group's 28-px header band.
fn group_header(ui: &mut Ui, title: &str) {
    let palette = Palette::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), GROUP_PX), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.line));
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    } else if stripe {
        ui.painter().rect_filled(rect, 0.0, palette.surface_stripe);
    }
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
    let left = rect.left() + 12.0;
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

/// One row: tier glyph and word, the code as a link, the message, the
/// field; clicking it selects what it names.
fn draw_row(ui: &mut Ui, state: &FrameState<'_>, row: &Row, stripe: bool, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.line));
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    } else if stripe {
        ui.painter().rect_filled(rect, 0.0, palette.surface_stripe);
    }
    if response.clicked()
        && let Some(selection) = row.selection()
    {
        out.push(Action::Select(selection));
        // A row on an operation's field opens it in the editor there.
        if let (Some(op), Some(field)) = (&row.op, &row.field) {
            out.push(Action::FocusField {
                op: op.clone(),
                field: field.clone(),
            });
        }
    }
    response.on_hover_text(&row.message);

    let mut cells = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(12.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    cells.spacing_mut().item_spacing.x = 8.0;
    let (icon, colour, word) = tier(row.level, palette);
    let glyph = icon.galley(&cells, theme::ICON_SMALL_PX, colour);
    let (at, _) = cells.allocate_exact_size(glyph.size(), Sense::hover());
    cells.painter().galley(at.min, glyph, colour);
    text(&mut cells, strings.get(word), TextStyle::Small, colour);
    if let (Some(code), Some(url)) = (&row.code, row.chapter_url()) {
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
    let field_width = row.field.as_ref().map_or(0.0, |field| {
        cells
            .painter()
            .layout_no_wrap(
                field.clone(),
                TextStyle::Monospace.resolve(cells.style()),
                palette.text_secondary,
            )
            .size()
            .x
            + 8.0
    });
    let message_width = (cells.available_width() - field_width).max(0.0);
    cells.allocate_ui_with_layout(
        egui::vec2(message_width, ROW_PX),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(message_width);
            text(ui, &row.message, TextStyle::Body, palette.text_primary);
        },
    );
    if let Some(field) = &row.field {
        text(
            &mut cells,
            field,
            TextStyle::Monospace,
            palette.text_secondary,
        );
    }
}
