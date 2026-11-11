// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The export result (functional spec, capability 77a; mock-up
//! `13-export-result.svg`; `experience.md`, section 15, "Export result"):
//! the export sheet's content once the programs are written, in three
//! parts. **Status**, one 15-px SemiBold line with `circle-check` and the
//! CLI's own lines under it; **Files**, a table of 28-px rows (number,
//! file name in the monospace face, tool, time right-aligned) grouped by
//! board with an Inlay, the totals, then the folder as one mono line with
//! *Show in folder* and a `copy` button; **Run instructions**, a
//! disclosure closed whenever the result opens, whose steps are numbered
//! lines, whose controller notes are callouts, and which ends with each
//! program's extents, estimate and pauses and each operation's line, as
//! `export` prints them. The footer has *Save setup sheet…* on the left
//! and *Done* on the right. Every line of the CLI's report is in exactly
//! one part (`dc_app::export::ExportResult::parts`).

use dc_app::export::ExportResult;
use dc_app::{StrKey, Strings};
use eframe::egui::{self, Align, Layout, TextStyle, Ui};

use crate::panels::export::Request;
use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;
use crate::widgets::section::section;

/// A Files row, px.
const ROW_PX: f32 = 28.0;
/// A sheet footer button, px.
const FOOTER_PX: f32 = 28.0;

/// Shows `result`; `instructions` is whether *Run instructions* is open.
/// Returns whether its header was clicked, to flip it.
pub(crate) fn show(
    ui: &mut Ui,
    strings: &Strings,
    result: &ExportResult,
    instructions: bool,
    out: &mut Vec<Action>,
) -> bool {
    ui.add_space(8.0);
    let room = (ui.available_height() - FOOTER_PX - 24.0).max(160.0);
    let mut toggled = false;
    egui::ScrollArea::vertical()
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            status(ui, result);
            ui.add_space(12.0);
            files(ui, strings, result, out);
            ui.add_space(12.0);
            toggled = section(
                ui,
                egui::Id::unique("export.result.instructions"),
                strings.get(StrKey::ExportResultInstructions),
                instructions,
                None,
                |ui| run_instructions(ui, result),
            );
        });
    ui.add_space(12.0);
    footer(ui, strings, out);
    toggled
}

/// The status: the glyph and the headline, then the CLI's lines.
fn status(ui: &mut Ui, result: &ExportResult) {
    let palette = Palette::current(ui);
    let colours = palette.status_success;
    ui.horizontal(|ui| {
        glyph(ui, Icon::CircleCheck, colours.stroke);
        text(
            ui,
            &result.status.headline,
            TextStyle::Heading,
            palette.text_primary,
        );
    });
    for line in &result.status.lines {
        wrapped(ui, line, TextStyle::Small, palette.text_secondary);
    }
}

/// The Files table, the totals and the folder.
fn files(ui: &mut Ui, strings: &Strings, result: &ExportResult, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    text(
        ui,
        strings.get(StrKey::ExportResultFiles),
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(4.0);
    table_row(
        ui,
        [
            strings.get(StrKey::ExportResultColumnNumber),
            strings.get(StrKey::ExportResultColumnFile),
            strings.get(StrKey::ExportResultColumnTool),
            strings.get(StrKey::ExportResultColumnTime),
        ],
        None,
        true,
    );
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
            fill,
            false,
        );
    }
    for total in &result.files.totals {
        wrapped(ui, total, TextStyle::Small, palette.text_secondary);
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        text(
            ui,
            strings.get(StrKey::ExportResultFolder),
            TextStyle::Body,
            palette.text_secondary,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let copy = ButtonSpec::new(Kind::Quiet, "")
                .icon(Icon::Copy, theme::ICON_SMALL_PX)
                .tooltip(strings.get(StrKey::ExportResultCopyPath));
            if button(ui, copy).clicked() {
                ui.ctx().copy_text(result.files.folder.clone());
                out.push(Action::Export(Request::Copied));
            }
            let show = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportShowInFolder))
                .icon(Icon::FolderOpen, theme::ICON_SMALL_PX);
            if button(ui, show).clicked() {
                out.push(Action::Export(Request::ShowInFolder));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&result.files.folder)
                            .text_style(TextStyle::Monospace)
                            .color(palette.text_primary),
                    )
                    .truncate(),
                )
                .on_hover_text(&result.files.folder);
            });
        });
    });
}

/// One 28-px row of the Files table: number, file name in mono, tool,
/// and the time right-aligned; the header row in secondary text.
fn table_row(ui: &mut Ui, cells: [&str; 4], fill: Option<egui::Color32>, header: bool) {
    let palette = Palette::current(ui);
    let colour = if header {
        palette.text_secondary
    } else {
        palette.text_primary
    };
    let [number, name, tool, time] = cells;
    egui::Frame::NONE
        .fill(fill.unwrap_or(theme::NONE))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ROW_PX);
            ui.horizontal_centered(|ui| {
                let width = ui.available_width();
                cell(ui, number, TextStyle::Body, palette.text_secondary, 24.0);
                let file_style = if header {
                    TextStyle::Body
                } else {
                    TextStyle::Monospace
                };
                cell(ui, name, file_style, colour, (width - 24.0) * 0.4);
                cell(ui, tool, TextStyle::Body, colour, (width - 24.0) * 0.4);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    text(ui, time, TextStyle::Body, colour);
                });
            });
        });
}

/// `text` in a cell `width` px wide, truncated at the end.
fn cell(ui: &mut Ui, text: &str, style: TextStyle, colour: egui::Color32, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width.max(0.0), ROW_PX),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width.max(0.0));
            ui.add(
                egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
                    .truncate()
                    .selectable(false),
            );
        },
    );
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
            wrapped(ui, &step.before, TextStyle::Body, palette.text_primary);
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
/// panel with a 6-px radius.
fn callout(ui: &mut Ui, note: &str) {
    let palette = Palette::current(ui);
    egui::Frame::NONE
        .fill(palette.surface_stripe)
        .corner_radius(6)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                glyph(ui, Icon::Info, palette.text_secondary);
                wrapped(ui, note, TextStyle::Body, palette.text_primary);
            });
        });
}

/// The footer: *Save setup sheet…* left, *Done* right.
fn footer(ui: &mut Ui, strings: &Strings, out: &mut Vec<Action>) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), FOOTER_PX),
        Layout::left_to_right(Align::Center),
        |ui| {
            let save = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ExportSaveSetupSheet))
                .tooltip(strings.get(StrKey::ExportSaveSetupSheetHelp))
                .height(FOOTER_PX);
            if button(ui, save).clicked() {
                out.push(Action::SaveSetupSheet);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let done = ButtonSpec::new(Kind::Primary, strings.get(StrKey::ExportResultDone))
                    .height(FOOTER_PX);
                if button(ui, done).clicked() {
                    out.push(Action::Export(Request::Close));
                }
            });
        },
    );
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
