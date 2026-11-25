// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Simulate step's panel (phase 6 functional spec, capabilities 107,
//! 112, 114, 116 to 118, 120 and 122), under its title and summary: the
//! board choice when the project has an Inlay, the operations in cut
//! order, **Result** and **Display**. The step has no Inspector.
//!
//! A row is an operation as the simulation cut it: a click shows the
//! stock as that operation leaves it, and the rows the playback has not
//! reached are dimmed. The panel reads the playback's place from the
//! frame's state (`FrameState::sim`) and asks for another through an
//! action: the playback itself is the Viewport's.

use dc_app::StrKey;
use dc_app::sim::{self, Blocked, Colouring, Layers, Reached, Row, SimPrefs, SimState};
use dc_job::sim::{CellSpec, Grid, Quality, UNCUT_RANGE_MM};
use dc_project::Board;
use eframe::egui::{self, Id, Rect, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::sim::SimAct;
use crate::widgets::button::{ButtonSpec, Kind as ButtonKind, button};
use crate::widgets::checkbox::checkbox;
use crate::widgets::op_glyph::op_glyph;
use crate::widgets::row::{self, surface, surface_rect};
use crate::widgets::section::section;
use crate::widgets::segmented::segmented;
use crate::widgets::unit_field::{UnitField, field as unit_field};

/// A row, px tall: the Operations list's.
const ROW_PX: f32 = theme::LIST_ROW_PX;
/// One run of an operation that has several, px tall.
const PART_PX: f32 = 28.0;
/// A row's glyph, px.
const ICON_PX: f32 = 16.0;
/// The card's padding, px.
const MARGIN: i8 = 16;
/// A row's progress line, px tall.
const PROGRESS_PX: f32 = 2.0;
/// The sections, by their `gui.toml` name.
const RESULT: &str = "simulate.result";
const DISPLAY: &str = "simulate.display";

/// `text` on one line, truncated with an ellipsis to `width`.
fn truncated(
    ui: &Ui,
    text: &str,
    style: &TextStyle,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        style.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// A small caption over a group of controls.
fn caption(ui: &mut Ui, text: &str) {
    let palette = Palette::current(ui);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    ui.painter()
        .galley(rect.min, galley, palette.text_secondary);
}

/// One operation's uncut material: its line, wrapped to the panel's
/// width, as a row to click. Whether it was clicked.
fn uncut_row(ui: &mut Ui, text: &str, help: &str) -> bool {
    const PAD: egui::Vec2 = egui::vec2(8.0, 6.0);
    let palette = Palette::current(ui);
    let width = ui.available_width();
    let galley = ui.painter().layout(
        text.to_owned(),
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
        (width - 2.0 * PAD.x).max(0.0),
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, galley.size().y + 2.0 * PAD.y),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    crate::widgets::button::focus_ring(ui, &response);
    ui.painter()
        .galley(rect.min + PAD, galley, palette.text_primary);
    response.on_hover_text(help).clicked()
}

/// The two-way choice of the board shown, with an Inlay in the project
/// (capability 118).
fn board_choice(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    if !state.app.has_male_board() {
        return;
    }
    let strings = state.app.strings();
    let options = [
        strings.get(StrKey::SimBoardStock),
        strings.get(StrKey::SimBoardMale),
    ];
    let selected = usize::from(state.app.sim_board() == Board::Male);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
            let width = ui.available_width();
            if let Some(chosen) =
                segmented(ui, Id::unique("dc.sim.board"), &options, selected, width)
            {
                out.push(Action::SetSimBoard(if chosen == 1 {
                    Board::Male
                } else {
                    Board::Female
                }));
            }
        });
    ui.add_space(8.0);
}

/// *Simulate again*, after Stop or a simulation that could not be made
/// (capability 123).
fn again(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    if !matches!(
        state.app.sim_state(),
        SimState::Stopped | SimState::Failed(_)
    ) || state.app.sim_blocked().is_some()
    {
        return;
    }
    let strings = state.app.strings();
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
            let spec = ButtonSpec::new(ButtonKind::Secondary, strings.get(StrKey::SimActionsAgain));
            if button(ui, spec).clicked() {
                out.push(Action::SimulateAgain);
            }
        });
    ui.add_space(8.0);
}

/// The line under a row that is being cut: how far through it.
fn progress(ui: &Ui, rect: Rect, part: f64) {
    let palette = Palette::current(ui);
    // Narrowed to a length on the screen.
    #[expect(clippy::cast_possible_truncation, reason = "a fraction of a row, px")]
    let done = rect.width() * part.clamp(0.0, 1.0) as f32;
    let line = Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - PROGRESS_PX),
        egui::vec2(done, PROGRESS_PX),
    );
    ui.painter()
        .rect_filled(line, PROGRESS_PX / 2.0, palette.accent);
}

/// One operation's row: its glyph in its colour, its name over its tool
/// and its time; under it, its runs when it has several.
fn list_row(ui: &mut Ui, row: &Row, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let (slot, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let rect = surface_rect(slot);
    let response = ui.interact(
        rect,
        Id::unique(("dc.sim.row", row.op.as_str())),
        Sense::click(),
    );
    let ahead = row.reached == Reached::Ahead;
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, !ahead, &row.name));
    surface(ui, rect, false, response.hovered());
    if response.clicked()
        && let Some(last) = row.runs.end.checked_sub(1)
    {
        out.push(Action::Sim(SimAct::ToRunEnd(last)));
    }
    // A row the playback has not reached is dimmed (capability 114).
    let (name_colour, detail_colour, glyph_colour) = if ahead {
        (
            palette.text_disabled,
            palette.text_disabled,
            palette.text_disabled,
        )
    } else {
        (
            palette.text_primary,
            palette.text_secondary,
            palette.operation(row.colour),
        )
    };
    let glyph_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_PX / 2.0, rect.center().y),
        egui::vec2(ICON_PX, ICON_PX),
    );
    if let Some(kind) = row.kind {
        op_glyph(ui.painter(), glyph_rect, kind, glyph_colour);
    }
    let time = truncated(ui, &row.time, &TextStyle::Small, detail_colour, 96.0);
    let time_left = rect.right() - 8.0 - time.size().x;
    let left = glyph_rect.right() + 8.0;
    let width = (time_left - 8.0 - left).max(0.0);
    let name = truncated(ui, &row.name, &TextStyle::Body, name_colour, width);
    let tool = truncated(ui, &row.tool, &TextStyle::Small, detail_colour, width);
    let top = rect.center().y - (name.size().y + 2.0 + tool.size().y) / 2.0;
    let tool_top = top + name.size().y + 2.0;
    ui.painter()
        .galley(egui::pos2(left, top), name, name_colour);
    ui.painter()
        .galley(egui::pos2(left, tool_top), tool, detail_colour);
    ui.painter().galley(
        egui::pos2(time_left, rect.center().y - time.size().y / 2.0),
        time,
        detail_colour,
    );
    if let Reached::Cutting(part) = row.reached {
        progress(ui, rect.shrink2(egui::vec2(8.0, 0.0)), part);
    }

    for part in &row.parts {
        let (slot, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), PART_PX), Sense::hover());
        let rect = surface_rect(slot);
        let response = ui.interact(
            rect,
            Id::unique(("dc.sim.run", row.op.as_str(), part.run)),
            Sense::click(),
        );
        let ahead = part.reached == Reached::Ahead;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::ListItem, true, !ahead, &part.label)
        });
        surface(ui, rect, false, response.hovered());
        if response.clicked() {
            out.push(Action::Sim(SimAct::ToRunEnd(part.run)));
        }
        let colour = if ahead {
            palette.text_disabled
        } else {
            palette.text_secondary
        };
        let time = truncated(ui, &part.time, &TextStyle::Small, colour, 96.0);
        let time_left = rect.right() - 8.0 - time.size().x;
        let label = truncated(
            ui,
            &part.label,
            &TextStyle::Small,
            colour,
            (time_left - 8.0 - left).max(0.0),
        );
        ui.painter().galley(
            egui::pos2(left, rect.center().y - label.size().y / 2.0),
            label,
            colour,
        );
        ui.painter().galley(
            egui::pos2(time_left, rect.center().y - time.size().y / 2.0),
            time,
            colour,
        );
        if let Reached::Cutting(done) = part.reached {
            progress(ui, rect.shrink2(egui::vec2(8.0, 0.0)), done);
        }
    }
}

/// The operations of the simulation shown, in cut order, then the line
/// about those an error left out (capabilities 108 and 114).
fn rows(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let refused = matches!(
        app.sim_blocked(),
        Some(Blocked::NoOperations | Blocked::Errors(_))
    );
    if let Some(built) = app.sim_built().filter(|_| !refused) {
        let rows = sim::rows(app.project(), strings, built, state.sim.time_s);
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in &rows.rows {
            list_row(ui, row, out);
        }
    }
    let left_out = app.sim_left_out().len();
    if left_out > 0 {
        ui.add_space(8.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(16, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    row::line(
                        ui,
                        true,
                        &strings.fmt(StrKey::SimRowsLeftOut, &[("count", &left_out.to_string())]),
                    );
                    let spec = ButtonSpec::new(ButtonKind::Quiet, strings.get(StrKey::SimRowsShow));
                    if button(ui, spec).clicked() {
                        out.push(Action::OpenWarnings);
                    }
                });
            });
    }
}

/// Whether section `name` is open: as last left, else open.
fn open(state: &FrameState<'_>, name: &str) -> bool {
    state.prefs.sections.get(name).copied().unwrap_or(true)
}

/// **Result**: the facts of the finished stock, then Uncut material
/// (capabilities 120 and 122).
fn result(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let prefs = state.prefs.sim;
    let is_open = open(state, RESULT);
    let clicked = section(
        ui,
        Id::unique("dc.sim.result"),
        strings.get(StrKey::SimResultTitle),
        is_open,
        None,
        |ui| {
            let Some(built) = app.sim_built() else {
                row::hint(ui, strings.get(StrKey::SimResultNone));
                return;
            };
            // The report's lines; its backticks are the terminal's.
            for line in sim::facts_lines(app.project(), built) {
                row::line(ui, false, &line.replace('`', ""));
            }
            ui.add_space(8.0);
            let mut shown = prefs.uncut_shown;
            if checkbox(
                ui,
                "dc.sim.uncut",
                &mut shown,
                strings.get(StrKey::SimUncutShow),
            )
            .changed()
            {
                out.push(Action::SetSimPrefs(SimPrefs {
                    uncut_shown: shown,
                    ..prefs
                }));
            }
            if !prefs.uncut_shown {
                return;
            }
            // The threshold, in the project's units.
            let units = app.project().display_units;
            let value = dc_app::units::summary_text(prefs.uncut_mm(), units);
            let label = strings.get(StrKey::SimUncutOver);
            ui.horizontal(|ui| {
                row::label(ui, label, strings.get(StrKey::SimUncutOverHelp), None);
                let typed = unit_field(
                    ui,
                    UnitField::new(
                        Id::unique("dc.sim.uncut.over"),
                        label,
                        &value,
                        dc_app::units::suffix(units),
                    ),
                );
                if let Some(text) = typed.committed
                    && let Ok(mm) = dc_app::units::parse_length_mm(&text, units)
                {
                    let (least, most) = UNCUT_RANGE_MM;
                    out.push(Action::SetSimPrefs(SimPrefs {
                        uncut_mm: mm.clamp(least, most),
                        ..prefs
                    }));
                }
            });
            let found = sim::uncut_rows(app.project(), strings, built);
            if found.is_empty() {
                row::hint(ui, strings.get(StrKey::SimUncutNone));
            }
            for found in &found {
                if uncut_row(ui, &found.text, strings.get(StrKey::SimUncutFrameHelp))
                    && let Some(around) = sim::uncut_bounds(built, found.op)
                {
                    out.push(Action::Sim(SimAct::View(sim::View3::Top)));
                    out.push(Action::Sim(SimAct::Frame(around)));
                }
            }
            // What the check cannot find, with the cell that sets it.
            let cell = dc_job::summary::describe_length(
                app.project(),
                2.0 * built.finished.grid().cell_mm(),
            );
            row::hint(
                ui,
                &strings.fmt(StrKey::SimUncutNarrow, &[("width", &cell)]),
            );
        },
    );
    if clicked {
        out.push(Action::SetSection {
            name: RESULT.to_owned(),
            open: !is_open,
        });
    }
}

/// **Display**: Quality with its cell size, Colour, and Layers
/// (capabilities 112, 116 and 117).
fn display(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let prefs = state.prefs.sim;
    let is_open = open(state, DISPLAY);
    let clicked = section(
        ui,
        Id::unique("dc.sim.display"),
        strings.get(StrKey::SimDisplayTitle),
        is_open,
        None,
        |ui| {
            const QUALITIES: [Quality; 3] = [Quality::Draft, Quality::Standard, Quality::Fine];
            ui.horizontal(|ui| {
                row::label(
                    ui,
                    strings.get(StrKey::SimQualityLabel),
                    strings.get(StrKey::SimQualityHelp),
                    None,
                );
                let labels = [
                    strings.get(StrKey::SimQualityDraft),
                    strings.get(StrKey::SimQualityStandard),
                    strings.get(StrKey::SimQualityFine),
                ];
                let selected = QUALITIES
                    .iter()
                    .position(|q| *q == prefs.quality)
                    .unwrap_or(1);
                let width = ui.available_width();
                if let Some(chosen) =
                    segmented(ui, Id::unique("dc.sim.quality"), &labels, selected, width)
                    && let Some(quality) = QUALITIES.get(chosen)
                {
                    out.push(Action::SetSimPrefs(SimPrefs {
                        quality: *quality,
                        ..prefs
                    }));
                }
            });
            // The cell the quality gives this board (*0.21 mm cells*).
            let stock = &app.project().stock;
            let (width_mm, height_mm) = app
                .sim_built()
                .map_or((stock.width_mm, stock.height_mm), |built| {
                    (built.plan.width_mm, built.plan.height_mm)
                });
            if let Ok(grid) = Grid::of(width_mm, height_mm, CellSpec::Quality(prefs.quality)) {
                let cell = dc_job::summary::describe_length(app.project(), grid.cell_mm());
                row::hint(
                    ui,
                    &strings.fmt(StrKey::SimQualityCells, &[("cell", &cell)]),
                );
            }
            ui.add_space(8.0);
            // Colour's three choices need the panel's whole width.
            caption(ui, strings.get(StrKey::SimColourLabel));
            let labels = [
                strings.get(StrKey::SimColourMaterial),
                strings.get(StrKey::SimColourOperation),
                strings.get(StrKey::SimColourDepth),
            ];
            let selected = Colouring::ALL
                .iter()
                .position(|c| *c == prefs.colouring)
                .unwrap_or(0);
            let width = ui.available_width();
            let chosen = ui
                .scope(|ui| {
                    segmented(
                        ui,
                        Id::unique("dc.sim.colour.panel"),
                        &labels,
                        selected,
                        width,
                    )
                })
                .inner;
            if let Some(colouring) = chosen.and_then(|n| Colouring::ALL.get(n)) {
                out.push(Action::SetSimPrefs(SimPrefs {
                    colouring: *colouring,
                    ..prefs
                }));
            }
            ui.add_space(8.0);
            caption(ui, strings.get(StrKey::SimLayersLabel));
            let layers = prefs.layers;
            let mut set = layers;
            for (salt, on, key) in [
                ("dc.sim.layer.tool", &mut set.tool, StrKey::SimLayersTool),
                (
                    "dc.sim.layer.toolpath",
                    &mut set.toolpath,
                    StrKey::SimLayersToolpath,
                ),
                (
                    "dc.sim.layer.rapids",
                    &mut set.rapids,
                    StrKey::SimLayersRapids,
                ),
                ("dc.sim.layer.bed", &mut set.bed, StrKey::SimLayersBed),
                (
                    "dc.sim.layer.origin",
                    &mut set.origin,
                    StrKey::SimLayersOrigin,
                ),
            ] {
                let _ = checkbox(ui, salt, on, strings.get(key));
            }
            if set != layers {
                out.push(Action::SetSimPrefs(SimPrefs {
                    layers: Layers { ..set },
                    ..prefs
                }));
            }
        },
    );
    if clicked {
        out.push(Action::SetSection {
            name: DISPLAY.to_owned(),
            open: !is_open,
        });
    }
}

/// The Simulate step's content, under its title, in a scroll area that
/// fills the card.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("simulate_step")
            .auto_shrink(false),
        crate::widgets::scroll::Surface::Panel,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            board_choice(ui, state, out);
            again(ui, state, out);
            rows(ui, state, out);
            ui.add_space(8.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: MARGIN,
                    right: MARGIN,
                    top: 0,
                    bottom: MARGIN,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(row::GAP_PX, 4.0);
                    result(ui, state, out);
                    display(ui, state, out);
                });
        },
    );
}
