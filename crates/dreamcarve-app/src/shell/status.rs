// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The status bar (functional spec, capability 50b; `experience.md`,
//! section 2, "Status bar"; slice 24, #616): 24 px on `surface.panel`
//! under a rule; cells of 12-px text 12 px apart, split by 1-px rules.
//! The cells are `dc_app::status`'s, drawn left to right: the units
//! button and its menu, the cursor's fixed cell, the *Toolpaths* button
//! and its popover (`shell::toolpaths`), generation (*Generating k of
//! n…* and Stop, or *Stopped* and *Regenerate all*), the counts (the
//! button that opens the Warnings drawer), the estimate, the Z range, a
//! drag, the shape under the pointer and the last message. A cell with
//! nothing to say is not drawn and takes no rule. No stock size: Project
//! Settings shows it.

use dc_app::status::{self, CountsCell, GenerationAction, GenerationCell, Inputs};
use dc_app::warnings::Level;
use dc_app::{Command, StrKey};
use dc_project::Units;
use eframe::egui::{self, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::toolpaths;
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring, text};
use crate::widgets::icon::Icon;
use crate::widgets::spinner::spinner;

/// The status bar's height, px.
const HEIGHT_PX: f32 = 24.0;
/// The cursor cell's fixed width, px, so the cells after it hold still.
const CURSOR_PX: f32 = 120.0;

/// A cell's rule on its right.
fn rule(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, HEIGHT_PX - 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette.border_default);
}

/// A glyph in `colour` at the small icon size.
fn glyph(ui: &mut Ui, icon: Icon, colour: egui::Color32) {
    let galley = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, colour);
}

/// The units cell: a quiet button reading `mm` or `in` with a
/// `chevron-down`, whose two-row menu switches the project's display
/// units (capability 53) as Project Settings' switch does: one undo step.
fn units(ui: &mut Ui, state: &FrameState<'_>, label: &str, current: Units, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let small = TextStyle::Small.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), small, palette.text_secondary);
    let chevron = Icon::ChevronDown.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    let size = egui::vec2(
        galley.size().x + 4.0 + chevron.size().x + 12.0,
        HEIGHT_PX - 4.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let response = response.on_hover_text(strings.get(StrKey::AppStatusUnitsHint));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    if response.hovered() || open {
        let wash = if open {
            palette.surface_active
        } else {
            palette.surface_hover
        };
        ui.painter().rect_filled(rect, 4.0, wash);
    }
    let x = rect.left() + 6.0;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(
            x + galley.size().x + 4.0,
            rect.center().y - chevron.size().y / 2.0,
        ),
        chevron,
        palette.text_secondary,
    );
    focus_ring(ui, &response);
    egui::Popup::menu(&response)
        .align(egui::RectAlign::TOP_START)
        .show(|ui| {
            for (units, key) in [
                (Units::Mm, StrKey::AppStatusUnitsMm),
                (Units::Inch, StrKey::AppStatusUnitsInch),
            ] {
                let row = ui.add(egui::Button::new(strings.get(key)).selected(units == current));
                if row.clicked() {
                    if units != current {
                        out.push(Action::run(Command::SetDisplayUnits(units)));
                    }
                    ui.close();
                }
            }
        });
}

/// The generation cell (capability 71; `experience.md`, section 8,
/// "Progress"): the spinner while anything is pending, the text, and Stop
/// or, after Stop, *Regenerate all*: the only place that command lives.
fn generation(ui: &mut Ui, cell: &GenerationCell, palette: &Palette, out: &mut Vec<Action>) {
    if cell.spinner {
        spinner(ui, "");
    }
    let colour = if cell.spinner {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    text(ui, &cell.text, TextStyle::Small, colour);
    if let Some(action) = &cell.action {
        let spec = ButtonSpec::new(Kind::Quiet, &action.label).height(HEIGHT_PX - 4.0);
        if button(ui, spec).clicked() {
            out.push(match action.action {
                GenerationAction::Stop => Action::StopGeneration,
                GenerationAction::RegenerateAll => Action::RegenerateAll,
            });
        }
    }
}

/// The counts cell (`experience.md`, section 2, "Status bar"): each tier
/// with rows as its glyph and count in its colour, or `circle-check` and
/// *No problems*; one button that opens and closes the Warnings drawer.
fn counts(ui: &mut Ui, state: &FrameState<'_>, cell: &CountsCell, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let inner = ui.horizontal(|ui| {
        if let Some(none) = &cell.none {
            glyph(ui, Icon::CircleCheck, palette.text_secondary);
            text(ui, none, TextStyle::Small, palette.text_secondary);
        }
        for (level, label) in &cell.tiers {
            let (icon, colour) = match level {
                Level::Error => (Icon::CircleX, palette.status_error.text),
                Level::Warning => (Icon::TriangleAlert, palette.status_warning.text),
                Level::Hint | Level::Note => (Icon::Info, palette.status_info.text),
            };
            glyph(ui, icon, colour);
            text(ui, label, TextStyle::Small, colour);
        }
    });
    let response = ui.interact(
        inner.response.rect,
        ui.make_persistent_id("warnings_counts"),
        Sense::click(),
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
            if state.inert() {
                ui.disable();
            }
            let full = ui.max_rect();
            ui.painter().hline(
                full.x_range(),
                full.top(),
                egui::Stroke::new(1.0, palette.border_default),
            );
            let inputs = Inputs {
                cursor_mm: state.pointer.cursor_mm,
                drag: state.pointer.drag.as_deref(),
                hover: state
                    .pointer
                    .hover
                    .as_ref()
                    .map(|(name, locked)| (name.as_str(), *locked)),
                message: state.message,
                estimate: state.estimate,
            };
            let cells = status::cells(state.app, &inputs);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let secondary = palette.text_secondary;
                units(ui, state, &cells.units_label, cells.units, out);
                rule(ui, palette);
                // The cursor's fixed cell, blank off the Viewport.
                ui.allocate_ui_with_layout(
                    egui::vec2(CURSOR_PX, HEIGHT_PX),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(CURSOR_PX);
                        if !cells.cursor.is_empty() {
                            text(ui, &cells.cursor, TextStyle::Small, secondary);
                        }
                    },
                );
                if cells.toolpaths {
                    rule(ui, palette);
                    let open = toolpaths::is_open(ui.ctx());
                    let spec = ButtonSpec::new(
                        Kind::Quiet,
                        state.app.strings().get(StrKey::AppToolpathsTitle),
                    )
                    .icon(Icon::Layers, theme::ICON_SMALL_PX)
                    .height(HEIGHT_PX - 4.0)
                    .pressed(open);
                    let response = button(ui, spec);
                    toolpaths::popover(ui, &response, state.app, out);
                }
                if let Some(cell) = &cells.generation {
                    rule(ui, palette);
                    generation(ui, cell, palette, out);
                }
                rule(ui, palette);
                counts(ui, state, &cells.counts, out);
                for (cell, colour) in [
                    // Capability 78: every program's estimated time.
                    (&cells.estimate, secondary),
                    // Capability 74: the program's Z range beside its boxes.
                    (&cells.z_range, secondary),
                    // A drag's travel, scale or angle (#534).
                    (&cells.drag, palette.text_primary),
                    (&cells.hover, palette.text_primary),
                    (&cells.message, secondary),
                ] {
                    if let Some(label) = cell {
                        rule(ui, palette);
                        let response = text(ui, label, TextStyle::Small, colour);
                        if std::ptr::eq(cell, &cells.estimate) {
                            response.on_hover_text(
                                state.app.strings().get(StrKey::ExportStatusTotalHelp),
                            );
                        }
                    }
                }
            });
        });
}
