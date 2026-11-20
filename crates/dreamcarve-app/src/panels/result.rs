// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The export result (functional spec, capability 77a; mock-up
//! `13-export-result.svg`; `experience.md`, section 15, "Export result"):
//! what the Export step shows in the plan's place once the programs are
//! written. A card inside the step's card holds **Status**, one 15-px
//! SemiBold line with `circle-check`, *Done* at its right (which brings
//! the plan back) and the CLI's own lines under it; **Files**, a table
//! of 30-px rows (number, file name in the monospace face, tool, time
//! right-aligned) grouped by board with an Inlay, and the totals; and the
//! **Folder**, one mono line cut at its start, with a `copy` button, over
//! *Show in folder* and *Save setup sheet…*. Under the card, **Run
//! instructions**, a disclosure closed whenever the result shows, whose
//! steps are numbered lines, whose controller notes are callouts, and
//! which ends with each program's extents, estimate and pauses and each
//! operation's line, as `export` prints them. Every line of the CLI's
//! report is in exactly one part
//! (`dc_app::export::ExportResult::parts`).

use dc_app::export::ExportResult;
use dc_app::{StrKey, Strings};
use eframe::egui::{self, Align, Layout, TextStyle, Ui};

use crate::panels::export::Request;
use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::card;
use crate::widgets::icon::Icon;
use crate::widgets::row;
use crate::widgets::section::section;

/// A Files row, px.
const ROW_PX: f32 = 30.0;
/// The Files table's header row, px.
const HEADER_ROW_PX: f32 = 24.0;
/// The result card's padding, px.
const PADDING_PX: i8 = 16;
/// The number column, px.
const NUMBER_PX: f32 = 16.0;
/// The gap between the result's parts, px.
const PART_GAP_PX: f32 = 12.0;

/// Shows `result`; `instructions` is whether *Run instructions* is open.
pub(crate) fn show(
    ui: &mut Ui,
    strings: &Strings,
    result: &ExportResult,
    instructions: bool,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    ui.spacing_mut().item_spacing = egui::vec2(theme::GAP_PX, 4.0);
    card::inner(palette)
        .inner_margin(PADDING_PX)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            status(ui, strings, result, out);
            ui.add_space(PART_GAP_PX - 4.0);
            files(ui, strings, result);
            ui.add_space(PART_GAP_PX - 4.0);
            folder(ui, strings, result, out);
        });
    let toggled = section(
        ui,
        egui::Id::unique("export.result.instructions"),
        strings.get(StrKey::ExportResultInstructions),
        instructions,
        None,
        |ui| run_instructions(ui, result),
    );
    if toggled {
        out.push(Action::Export(Request::ToggleInstructions));
    }
}

/// The status: the glyph, the headline and *Done*, then the CLI's lines.
fn status(ui: &mut Ui, strings: &Strings, result: &ExportResult, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let colours = palette.status_success;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), theme::CONTROL_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            // Secondary: the header's *Export G-code* is the view's one
            // primary button.
            let done = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportResultDone))
                .tooltip(strings.get(StrKey::ExportResultDoneHelp));
            if button(ui, done).clicked() {
                out.push(Action::Export(Request::Done));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                glyph(ui, Icon::CircleCheck, colours.stroke);
                text(
                    ui,
                    &result.status.headline,
                    TextStyle::Heading,
                    palette.text_primary,
                );
            });
        },
    );
    for line in &result.status.lines {
        wrapped(ui, line, TextStyle::Small, palette.text_secondary);
    }
}

/// The widths of the Files table's columns after the number: the file's
/// and the tool's, which share what the time's leaves, and the time's.
#[derive(Debug, Clone, Copy)]
struct Columns {
    shared_px: f32,
    time_px: f32,
}

/// The Files table and the totals.
fn files(ui: &mut Ui, strings: &Strings, result: &ExportResult) {
    let palette = Palette::current(ui);
    let header = [
        strings.get(StrKey::ExportResultColumnNumber),
        strings.get(StrKey::ExportResultColumnFile),
        strings.get(StrKey::ExportResultColumnTool),
        strings.get(StrKey::ExportResultColumnTime),
    ];
    // One set of columns for every row: the time's is its widest text.
    let body = TextStyle::Body.resolve(ui.style());
    let time_px = result
        .files
        .rows
        .iter()
        .map(|row| row.time.as_str())
        .chain([header[3]])
        .map(|time| {
            ui.painter()
                .layout_no_wrap(time.to_owned(), body.clone(), palette.text_primary)
                .size()
                .x
        })
        .fold(0.0, f32::max)
        .ceil();
    let inner = ui.available_width() - 16.0;
    let columns = Columns {
        shared_px: ((inner - NUMBER_PX - time_px - 3.0 * theme::GAP_PX) / 2.0).max(0.0),
        time_px,
    };
    table_row(ui, header, columns, None, true);
    for (k, row) in result.files.rows.iter().enumerate() {
        if let Some(board) = &row.board {
            ui.add_space(4.0);
            wrapped(
                ui,
                board,
                TextStyle::Name(theme::MEDIUM_STYLE.into()),
                palette.text_primary,
            );
        }
        let fill = (k % 2 == 0).then_some(palette.surface_stripe);
        table_row(
            ui,
            [&row.number.to_string(), &row.name, &row.tool, &row.time],
            columns,
            fill,
            false,
        );
    }
    ui.add_space(4.0);
    for total in &result.files.totals {
        wrapped(ui, total, TextStyle::Small, palette.text_secondary);
    }
}

/// The folder the files went to and its copy button, over *Show in
/// folder* and *Save setup sheet…*.
fn folder(ui: &mut Ui, strings: &Strings, result: &ExportResult, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), theme::CHIP_PX),
        Layout::right_to_left(Align::Center),
        |ui| {
            let copy = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::Copy, theme::ICON_SMALL_PX)
                .height(theme::CHIP_PX)
                .width(theme::CHIP_PX)
                .tooltip(strings.get(StrKey::ExportResultCopyPath));
            if button(ui, copy).clicked() {
                ui.ctx().copy_text(result.files.folder.clone());
                out.push(Action::Export(Request::Copied));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                text(
                    ui,
                    strings.get(StrKey::ExportResultFolder),
                    TextStyle::Body,
                    palette.text_secondary,
                );
                row::path(ui, &result.files.folder);
            });
        },
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let show = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportShowInFolder))
            .icon(Icon::FolderOpen, theme::ICON_SMALL_PX);
        if button(ui, show).clicked() {
            out.push(Action::Export(Request::ShowInFolder));
        }
        let save = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportSaveSetupSheet))
            .tooltip(strings.get(StrKey::ExportSaveSetupSheetHelp));
        if button(ui, save).clicked() {
            out.push(Action::SaveSetupSheet);
        }
    });
}

/// One row of the Files table: number, file name in mono, tool, and the
/// time right-aligned; the header row in secondary text.
fn table_row(
    ui: &mut Ui,
    cells: [&str; 4],
    columns: Columns,
    fill: Option<egui::Color32>,
    header: bool,
) {
    let palette = Palette::current(ui);
    let (colour, style, height) = if header {
        (palette.text_secondary, TextStyle::Small, HEADER_ROW_PX)
    } else {
        (palette.text_primary, TextStyle::Body, ROW_PX)
    };
    let [number, name, tool, time] = cells;
    egui::Frame::NONE
        .fill(fill.unwrap_or(theme::NONE))
        .corner_radius(theme::CONTROL_RADIUS_PX)
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = theme::GAP_PX;
                    let secondary = palette.text_secondary;
                    cell(ui, number, style.clone(), secondary, NUMBER_PX, height);
                    let file_style = if header {
                        style.clone()
                    } else {
                        TextStyle::Monospace
                    };
                    cell(ui, name, file_style, colour, columns.shared_px, height);
                    cell(ui, tool, style.clone(), colour, columns.shared_px, height);
                    ui.allocate_ui_with_layout(
                        egui::vec2(columns.time_px, height),
                        Layout::right_to_left(Align::Center),
                        |ui| text(ui, time, style, colour),
                    );
                },
            );
        });
}

/// `text` in a cell `width` px wide, truncated at the end, the whole of
/// it its tooltip.
fn cell(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32, width: f32, height: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width.max(0.0), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width.max(0.0));
            ui.add(
                egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
                    .truncate()
                    .selectable(false),
            )
            .on_hover_text(text);
        },
    );
}

/// Step `number`'s line without the number the report gave it: with
/// several programs `export` prints `1. plaque-1-T2.nc  Before you
/// start: …`, and the window numbers its steps in a gutter of its own,
/// so the line read "1. 1. plaque-1-T2.nc".
fn unnumbered(before: &str, number: usize) -> &str {
    before
        .strip_prefix(&format!("{number}."))
        .map_or(before, str::trim_start)
}

/// The steps, numbered; the notes as callouts; then each program's
/// details and each operation's line.
fn run_instructions(ui: &mut Ui, result: &ExportResult) {
    let palette = Palette::current(ui);
    let instructions = &result.instructions;
    for (k, step) in instructions.steps.iter().enumerate() {
        ui.horizontal_top(|ui| {
            text(
                ui,
                &format!("{}.", k + 1),
                TextStyle::Body,
                palette.text_secondary,
            );
            wrapped(
                ui,
                unnumbered(&step.before, k + 1),
                TextStyle::Body,
                palette.text_primary,
            );
        });
    }
    for note in &instructions.notes {
        ui.add_space(4.0);
        callout(ui, note);
    }
    for step in &instructions.steps {
        ui.add_space(8.0);
        if let Some(heading) = &step.heading {
            wrapped(
                ui,
                heading,
                TextStyle::Name(theme::MEDIUM_STYLE.into()),
                palette.text_primary,
            );
        }
        for line in step
            .extents
            .iter()
            .chain(&step.estimate)
            .chain(&step.pauses)
        {
            wrapped(ui, line, TextStyle::Small, palette.text_secondary);
        }
    }
    if !instructions.facts.is_empty() {
        ui.add_space(8.0);
        for line in &instructions.facts {
            wrapped(ui, line, TextStyle::Small, palette.text_secondary);
        }
    }
}

/// A controller note: the `info` glyph and the text on a stripe-filled
/// panel at the inner-card radius.
fn callout(ui: &mut Ui, note: &str) {
    let palette = Palette::current(ui);
    egui::Frame::NONE
        .fill(palette.surface_stripe)
        .corner_radius(theme::INNER_CARD_RADIUS_PX)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                glyph(ui, Icon::Info, palette.text_secondary);
                wrapped(ui, note, TextStyle::Body, palette.text_primary);
            });
        });
}

/// `icon` at 16 px in `colour`, in its own slot.
fn glyph(ui: &mut Ui, icon: Icon, colour: egui::Color32) {
    let galley = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
        egui::Sense::hover(),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
}

/// `text` wrapped to the width.
fn wrapped(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
            .wrap()
            .selectable(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step is numbered once: the report's own number is taken off the
    /// line the window numbers, and a line with none (one program) or
    /// with another step's number is left as it is.
    #[test]
    fn a_step_is_numbered_once() {
        assert_eq!(
            unnumbered("1. plaque-1-T2.nc  Before you start: fit T2", 1),
            "plaque-1-T2.nc  Before you start: fit T2"
        );
        assert_eq!(
            unnumbered("Before you start: fit T1", 1),
            "Before you start: fit T1"
        );
        assert_eq!(unnumbered("2. b.nc  Before: …", 1), "2. b.nc  Before: …");
        assert_eq!(unnumbered("12. l.nc", 12), "l.nc");
    }
}
