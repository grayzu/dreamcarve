// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Export step (`docs/specs/phase-4/redesign.md`, "The steps";
//! functional spec, capabilities 60, 62, 77 and 77a): the whole export,
//! top to bottom, in the step's panel. Under the step's title:
//!
//! - the **readiness card** (a card inside the card: the error, warning
//!   and hint counts as status chips, or *No problems*; the programs'
//!   estimated time as the 22-px summary number; and the tool centre's Z
//!   range once the programs are built);
//! - while an error blocks export, the **blocking findings**: a card in
//!   the error tint that counts them, with *Show* opening the Warnings
//!   drawer at them, and lists each one; a click on one opens the step
//!   that fixes it, as its row in the drawer does;
//! - the **output settings**, three groups closed until they are wanted,
//!   each with its values on its header
//!   ([`crate::panels::project::output`]);
//! - the **plan**: the program split, the boards, *Program Details* and
//!   the destination ([`crate::panels::export::plan`]); or, once the
//!   programs are written, the **result** in its place
//!   ([`crate::panels::result`]).
//!
//! The header's *Export G-code* is the step's one primary button: it
//! writes the plan, asking for a folder first when none is chosen. The
//! step has no Inspector, so its panel is wider than the other steps'
//! ([`crate::shell::theme::EXPORT_PANEL_PX`]): a program's name, its tool
//! and its time fit on one line.

use dc_app::StrKey;
use dc_app::warnings::{Level, Row};
use eframe::egui::{self, Rect, Sense, TextStyle, Ui};

use crate::panels;
use crate::panels::export::Request;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring};
use crate::widgets::card;
use crate::widgets::chip::{Chip, Tone};
use crate::widgets::icon::Icon;
use crate::widgets::row;

/// The step's content margin, px.
const MARGIN_PX: i8 = 16;
/// The readiness card's padding, px.
const PADDING_PX: i8 = 16;
/// The gap between the card's chips, px.
const CHIP_GAP_PX: f32 = 6.0;
/// The gap between the step's blocks, px.
const BLOCK_GAP_PX: f32 = 12.0;
/// The blocking card's padding, px.
const BLOCKING_PADDING_PX: f32 = 12.0;

/// Draws the step's content under its title, in a scroll area that fills
/// the card.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("export_step")
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
                    ui.spacing_mut().item_spacing.y = 0.0;
                    readiness(ui, state);
                    if let Some(failure) = state.export.failure() {
                        ui.add_space(BLOCK_GAP_PX);
                        row::line(ui, true, failure);
                    }
                    blocking(ui, state, out);
                    ui.add_space(BLOCK_GAP_PX);
                    panels::project::output(ui, state, out);
                    ui.add_space(BLOCK_GAP_PX - 4.0);
                    match state.export.done() {
                        Some(done) => {
                            panels::result::show(ui, strings, &done.result, done.instructions, out);
                        }
                        None => panels::export::plan(ui, state, out),
                    }
                });
        },
    );
}

/// The readiness card: the counts, the estimate and the Z range.
fn readiness(ui: &mut Ui, state: &FrameState<'_>) {
    let palette = Palette::current(ui);
    let app = state.app;
    let strings = app.strings();
    // What refuses export, which a blank project's status bar leaves out.
    let counts = &dc_app::status::export_counts(app);
    let said = counts.none.clone().unwrap_or_else(|| {
        counts
            .tiers
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    });
    let frame = card::inner(palette)
        .inner_margin(PADDING_PX)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The counts: a status chip per tier, or the all-clear.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CHIP_GAP_PX;
                if let Some(none) = &counts.none {
                    let glyph = Icon::CircleCheck.galley(
                        ui,
                        theme::ICON_SMALL_PX,
                        palette.status_success.stroke,
                    );
                    let text = ui.painter().layout_no_wrap(
                        none.clone(),
                        TextStyle::Body.resolve(ui.style()),
                        palette.text_primary,
                    );
                    let size = egui::vec2(glyph.size().x + 6.0 + text.size().x, theme::CHIP_PX);
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.painter().galley(
                        egui::pos2(rect.left(), rect.center().y - glyph.size().y / 2.0),
                        glyph.clone(),
                        palette.status_success.stroke,
                    );
                    ui.painter().galley(
                        egui::pos2(
                            rect.left() + glyph.size().x + 6.0,
                            rect.center().y - text.size().y / 2.0,
                        ),
                        text,
                        palette.text_primary,
                    );
                }
                for (level, label) in &counts.tiers {
                    let tone = match level {
                        Level::Error => Tone::Status(palette.status_error, Icon::CircleX),
                        Level::Warning => Tone::Status(palette.status_warning, Icon::TriangleAlert),
                        Level::Hint | Level::Note => Tone::Status(palette.status_info, Icon::Info),
                    };
                    let chip = Chip::new(ui, label, tone);
                    let (rect, _) = ui.allocate_exact_size(chip.size(), Sense::hover());
                    chip.paint(ui, rect);
                }
            });
            ui.add_space(14.0);
            // The estimate, the summary number, beside the Z range.
            let estimate = dc_app::export::total_time(app, state.estimate);
            let z_range = app.generation().finished().and_then(|finished| {
                let extents = dc_app::view::program_extents(app.project(), finished)?;
                let z = dc_app::view::range_label(extents.z?, extents.units);
                Some(format!("{z} {}", dc_app::units::suffix(extents.units)))
            });
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 28.0;
                fact(
                    ui,
                    strings.get(StrKey::StepsExportEstimate),
                    estimate
                        .as_deref()
                        .unwrap_or_else(|| strings.get(StrKey::ExportNotAvailable)),
                    TextStyle::Name(theme::SUMMARY.into()),
                );
                if let Some(z_range) = &z_range {
                    fact(
                        ui,
                        strings.get(StrKey::StepsExportZRange),
                        z_range,
                        TextStyle::Body,
                    );
                }
            });
        });
    // One node for assistive tools: the counts, as the status bar says
    // them.
    let name = strings.fmt(StrKey::StepsExportReadiness, &[("counts", &said)]);
    let rect: Rect = frame.response.rect;
    ui.interact(
        rect,
        egui::Id::unique("dc.export_step.readiness"),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
}

/// A captioned value: `caption` in 12-px text.secondary over `value` in
/// `style`, with the value's baseline at the same height whatever its
/// size.
fn fact(ui: &mut Ui, caption: &str, value: &str, style: TextStyle) {
    let palette = Palette::current(ui);
    let caption_galley = ui.painter().layout_no_wrap(
        caption.to_owned(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let value_galley = ui.painter().layout_no_wrap(
        value.to_owned(),
        style.resolve(ui.style()),
        palette.text_primary,
    );
    // The room the summary number takes, so a smaller value sits on its
    // baseline.
    let tall = ui.text_style_height(&TextStyle::Name(theme::SUMMARY.into()));
    let size = egui::vec2(
        caption_galley.size().x.max(value_galley.size().x),
        caption_galley.size().y + 4.0 + tall,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .galley(rect.min, caption_galley, palette.text_secondary);
    ui.painter().galley(
        egui::pos2(rect.left(), rect.bottom() - 3.0 - value_galley.size().y),
        value_galley,
        palette.text_primary,
    );
}

/// The findings that block export, while any does: a card in the error
/// tint, headed by their count and *Show*, then one row each.
fn blocking(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let rows = dc_app::warnings::blocking(state.app);
    if rows.is_empty() {
        return;
    }
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let colours = palette.status_error;
    ui.add_space(BLOCK_GAP_PX);
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(0, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The head: the glyph, the count, and Show at the right.
            let (head, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), theme::CONTROL_PX),
                Sense::hover(),
            );
            let mut line = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(head.shrink2(egui::vec2(BLOCKING_PADDING_PX, 0.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            let show = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::ExportShow))
                .tooltip(strings.get(StrKey::ExportShowHelp));
            if button(&mut line, show).clicked() {
                out.push(Action::Export(Request::ShowErrors));
            }
            line.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let glyph = Icon::CircleX.galley(ui, theme::ICON_SMALL_PX, colours.stroke);
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
                    Sense::hover(),
                );
                ui.painter()
                    .galley(rect.center() - glyph.size() / 2.0, glyph, colours.stroke);
                let count = dc_app::status::counted(
                    strings,
                    rows.len(),
                    StrKey::ExportBlockedOne,
                    StrKey::ExportBlockedMany,
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(count)
                            .text_style(TextStyle::Name(theme::MEDIUM_STYLE.into()))
                            .color(colours.text),
                    )
                    .truncate()
                    .selectable(false),
                );
            });
            for found in &rows {
                finding(ui, found, out);
            }
        });
    ui.painter().set(
        slot,
        egui::epaint::RectShape::filled(
            inner.response.rect,
            theme::INNER_CARD_RADIUS_PX,
            colours.tint,
        ),
    );
}

/// One blocking finding: its message, wrapped, over the field it is
/// about in the mono face. The whole row is a button named with `check`'s
/// line, as the finding's row in the Warnings drawer is: a click selects
/// what it names and opens the step that edits it.
fn finding(ui: &mut Ui, row: &Row, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let pad = egui::vec2(BLOCKING_PADDING_PX, 6.0);
    let room = (ui.available_width() - 2.0 * pad.x).max(0.0);
    // Painted, so the row is one node for assistive tools: the button.
    let message = ui.painter().layout(
        row.message.clone(),
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
        room,
    );
    let field = row.field.as_ref().map(|field| {
        let mut job = egui::text::LayoutJob::simple_singleline(
            field.clone(),
            TextStyle::Monospace.resolve(ui.style()),
            palette.text_secondary,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        ui.painter().layout_job(job)
    });
    let height = message.size().y + field.as_ref().map_or(0.0, |f| 2.0 + f.size().y);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height + 2.0 * pad.y),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.line));
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(4.0, 0.0)),
            theme::CONTROL_RADIUS_PX,
            palette.surface_hover,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let at = rect.min + pad;
    let below = at.y + message.size().y + 2.0;
    ui.painter().galley(at, message, palette.text_primary);
    if let Some(field) = field {
        ui.painter()
            .galley(egui::pos2(at.x, below), field, palette.text_secondary);
    }
    focus_ring(ui, &response);
    if response.clicked() {
        out.push(Action::OpenRow {
            selection: row.selection(),
            op: row.op.clone(),
            field: row.field.clone(),
        });
    }
}
