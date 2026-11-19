// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Export step (`docs/specs/phase-4/redesign.md`, "The steps";
//! functional spec, capabilities 60, 62 and 77): under the step's title, a
//! **readiness card** (a card inside the card: the error, warning and
//! hint counts as status chips, or *No problems*; the programs' estimated
//! time as the 22-px summary number; and the tool centre's Z range once
//! the programs are built), *Review programs…*, which opens the export
//! sheet ([`crate::panels::export`]) whatever the counts say, and the
//! output settings: the Heights, Tolerances and Post-processor sections
//! ([`crate::panels::project::output`]). The header's *Export G-code* is
//! the step's primary button.

use dc_app::StrKey;
use dc_app::warnings::Level;
use eframe::egui::{self, Rect, Sense, TextStyle, Ui};

use crate::panels;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::card;
use crate::widgets::chip::{Chip, Tone};
use crate::widgets::icon::Icon;

/// The step's content margin, px.
const MARGIN_PX: i8 = 16;
/// The readiness card's padding, px.
const PADDING_PX: i8 = 16;
/// The gap between the card's chips, px.
const CHIP_GAP_PX: f32 = 6.0;

/// Draws the step's content under its title, in a scroll area that fills
/// the card.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    egui::ScrollArea::vertical()
        .id_salt("export_step")
        .auto_shrink(false)
        .show(ui, |ui| {
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
                    ui.add_space(12.0);
                    let review =
                        ButtonSpec::new(Kind::Secondary, strings.get(StrKey::StepsExportReview))
                            .width(ui.available_width())
                            .tooltip(strings.get(StrKey::StepsExportReviewHelp));
                    if button(ui, review).clicked() {
                        out.push(Action::OpenExport);
                    }
                    ui.add_space(theme::SECTION_GAP_PX - 12.0);
                    panels::project::output(ui, state, out);
                });
        });
}

/// The readiness card: the counts, the estimate and the Z range.
fn readiness(ui: &mut Ui, state: &FrameState<'_>) {
    let palette = Palette::current(ui);
    let app = state.app;
    let strings = app.strings();
    let counts = &state.cells.counts;
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
                ui.spacing_mut().item_spacing.x = 20.0;
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
