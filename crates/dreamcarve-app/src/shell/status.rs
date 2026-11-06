// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The status bar (functional spec, capability 50; `experience.md`,
//! section 2, "Status bar"): 24 px on `surface.panel` under a rule; cells
//! of 12-px text 12 px apart, split by 1-px rules: units, stock size,
//! cursor in project units (`experience.md`, section 9: `X 64.2  Y 51.8`,
//! one decimal in mm, three in inches), generation state (*Generating k
//! of n…* and Stop), the counts of each tier with their glyphs (the button
//! that opens the Warnings drawer), the shape under
//! the pointer (section 8, "Hover"), and the last message.

use dc_app::{StrKey, warnings};
use dc_project::Units;
use eframe::egui::{self, TextStyle, Ui};

use crate::panels::warnings::counted;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring, text};
use crate::widgets::icon::Icon;
use crate::widgets::spinner::spinner;

/// The status bar's height, px.
const HEIGHT_PX: f32 = 24.0;

/// `value` to at most `decimals` places, trailing zeros dropped, as the
/// mock-up writes a stock (`Stock 160 × 100 × 12 mm`).
fn number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// A cell's rule on its right.
fn rule(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, HEIGHT_PX - 8.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette.border_default);
}

/// A count with its tier glyph, in the tier's colour (section 1,
/// principle 3: never colour without a glyph).
fn count(ui: &mut Ui, icon: Icon, colour: egui::Color32, label: &str) {
    let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    let (rect, _) = ui.allocate_exact_size(glyph.size(), egui::Sense::hover());
    ui.painter().galley(rect.min, glyph, colour);
    text(ui, label, TextStyle::Small, colour);
}

/// The generation cell (capability 71; `experience.md`, section 8,
/// "Progress"): *Generating k of n…* with the spinner and a quiet *Stop*
/// while an operation is pending, *Checking…* while only the project-level
/// pass runs, *Stopped* once Stop held anything back, else *Idle*.
fn generation(ui: &mut Ui, state: &FrameState<'_>, palette: &Palette, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let generation = state.app.generation();
    let secondary = palette.text_secondary;
    let running = match generation.progress() {
        Some(progress) => Some(strings.fmt(
            StrKey::AppGenerationGenerating,
            &[
                ("at", &progress.at.to_string()),
                ("of", &progress.of.to_string()),
            ],
        )),
        None if generation.busy() => Some(strings.get(StrKey::AppGenerationChecking).to_owned()),
        None => None,
    };
    match running {
        Some(label) => {
            spinner(ui, "");
            text(ui, &label, TextStyle::Small, palette.text_primary);
            let stop = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppGenerationStop))
                .height(HEIGHT_PX - 4.0);
            if button(ui, stop).clicked() {
                out.push(Action::StopGeneration);
            }
        }
        None if generation.stopped() => {
            text(
                ui,
                strings.get(StrKey::AppGenerationStopped),
                TextStyle::Small,
                secondary,
            );
        }
        None => {
            text(
                ui,
                strings.get(StrKey::AppStatusIdle),
                TextStyle::Small,
                secondary,
            );
        }
    }
}

/// The counts cell (`experience.md`, section 2, "Status bar"): each tier
/// with rows as its glyph and count in its colour, or *No warnings*; one
/// button that opens and closes the Warnings drawer.
fn counts(ui: &mut Ui, state: &FrameState<'_>, palette: &Palette, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let counts = warnings::counts(state.app);
    let cell = ui.horizontal(|ui| {
        if counts.errors == 0 && counts.warnings == 0 && counts.hints == 0 {
            text(
                ui,
                strings.get(StrKey::AppStatusNoWarnings),
                TextStyle::Small,
                palette.text_secondary,
            );
        }
        if counts.errors > 0 {
            let label = counted(
                strings,
                counts.errors,
                StrKey::AppStatusError,
                StrKey::AppStatusErrors,
            );
            count(ui, Icon::CircleX, palette.status_error.text, &label);
        }
        if counts.warnings > 0 {
            let label = counted(
                strings,
                counts.warnings,
                StrKey::AppStatusWarning,
                StrKey::AppStatusWarnings,
            );
            count(ui, Icon::TriangleAlert, palette.status_warning.text, &label);
        }
        if counts.hints > 0 {
            let label = counted(
                strings,
                counts.hints,
                StrKey::AppWarningsHint,
                StrKey::AppWarningsHints,
            );
            count(ui, Icon::Info, palette.status_info.text, &label);
        }
    });
    let response = ui.interact(
        cell.response.rect,
        ui.make_persistent_id("warnings_counts"),
        egui::Sense::click(),
    );
    let title = strings.get(StrKey::AppWarningsTitle);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    if response.clicked() {
        out.push(Action::ToggleWarnings);
    }
    focus_ring(ui, &response);
}

/// Draws the status bar.
pub(crate) fn bar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    egui::Panel::bottom("status_bar")
        .exact_size(HEIGHT_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_panel)
                .inner_margin(egui::Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(
                full.x_range(),
                full.top(),
                egui::Stroke::new(1.0, palette.border_default),
            );
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let strings = state.app.strings();
                let project = state.app.project();
                let units = project.display_units;
                let secondary = palette.text_secondary;
                let unit_key = match units {
                    Units::Mm => StrKey::AppStatusMm,
                    Units::Inch => StrKey::AppStatusInch,
                };
                text(ui, strings.get(unit_key), TextStyle::Small, secondary);
                rule(ui, palette);
                let decimals = match units {
                    Units::Mm => 1,
                    Units::Inch => 3,
                };
                let length = |mm: f64| number(dc_app::units::to_display(mm, units), decimals);
                let thickness = project
                    .stock
                    .thickness_mm
                    .map_or_else(|| strings.get(StrKey::AppStatusUnset).to_owned(), length);
                let stock = strings.fmt(
                    StrKey::AppStatusStock,
                    &[
                        ("width", &length(project.stock.width_mm)),
                        ("height", &length(project.stock.height_mm)),
                        ("thickness", &thickness),
                        ("unit", strings.get(unit_key)),
                    ],
                );
                text(ui, &stock, TextStyle::Small, secondary);
                rule(ui, palette);
                let cursor = state.pointer.cursor_mm.map_or_else(
                    || strings.get(StrKey::AppStatusCursorNone).to_owned(),
                    |p| {
                        let coordinate = |mm: f64| {
                            let text =
                                format!("{:.decimals$}", dc_app::units::to_display(mm, units));
                            // No "-0.0" at the origin.
                            if text
                                .trim_start_matches('-')
                                .chars()
                                .all(|c| c == '0' || c == '.')
                            {
                                text.trim_start_matches('-').to_owned()
                            } else {
                                text
                            }
                        };
                        strings.fmt(
                            StrKey::AppViewportCursor,
                            &[("x", &coordinate(p.x)), ("y", &coordinate(p.y))],
                        )
                    },
                );
                text(ui, &cursor, TextStyle::Small, secondary);
                // A drag's travel, scale or angle (#534).
                if let Some(drag) = &state.pointer.drag {
                    rule(ui, palette);
                    text(ui, drag, TextStyle::Small, palette.text_primary);
                }
                rule(ui, palette);
                generation(ui, state, palette, out);
                rule(ui, palette);
                counts(ui, state, palette, out);
                if let Some((name, locked)) = &state.pointer.hover {
                    rule(ui, palette);
                    let label = if *locked {
                        strings.fmt(StrKey::AppViewportHoverLocked, &[("name", name)])
                    } else {
                        name.clone()
                    };
                    text(ui, &label, TextStyle::Small, palette.text_primary);
                }
                if let Some(message) = state.message {
                    rule(ui, palette);
                    text(ui, message, TextStyle::Small, secondary);
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn stock_numbers_drop_trailing_zeros() {
        assert_eq!(number(160.0, 1), "160");
        assert_eq!(number(11.8, 1), "11.8");
        assert_eq!(number(4.724_409, 3), "4.724");
        assert_eq!(number(3.5, 3), "3.5");
    }
}
