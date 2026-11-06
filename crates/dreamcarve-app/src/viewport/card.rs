// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's toolpath controls (functional spec, capabilities 73 and
//! 74; `experience.md`, section 9, "Visibility card", "Depth legend" and
//! "Program extents"; mock-up `09-preview-legend.svg`): the floating
//! *Toolpaths* card at the pane's top-right, collapsible, with the colour
//! mode, the layer toggles, *Only the selected operation*, the program
//! extents, and *Show all* / *Hide all*; in *By depth*, the depth legend
//! beside it; and the extents' caption pill. Everything here is view
//! state: the card emits actions the window turns into `dc_app` view
//! settings and `gui.toml` preferences, never commands.

use dc_app::view::{ColourMode, DepthLegend, ProgramExtents, Visibility, range_label};
use dc_app::{App, StrKey};
use eframe::egui::{self, Align2, Rect, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::viewport::renderer::gpu_f32;
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::section::section;
use crate::widgets::segmented::segmented;

/// The card's width, px.
const CARD_PX: f32 = 200.0;
/// Its inset from the pane's corner, px.
const INSET_PX: f32 = 12.0;
/// Its inner margin, px.
const MARGIN_PX: i8 = 8;
/// The `sections` key in `gui.toml` that keeps the card open or closed.
pub(crate) const SECTION: &str = "viewport.toolpaths";
/// The legend bar's size, px.
const LEGEND_BAR: egui::Vec2 = egui::vec2(12.0, 160.0);
/// Two legend labels closer than this stack side by side, px.
const LABEL_GAP_PX: f32 = 14.0;

/// Draws the card over `pane` and, in *By depth*, the legend left of it;
/// pushes what the user changed to `out`. Returns the card's rectangle.
pub(crate) fn show(
    ui: &Ui,
    pane: Rect,
    app: &App,
    legend: Option<&DepthLegend>,
    out: &mut Vec<Action>,
) -> Rect {
    let strings = app.strings();
    let view = app.view();
    let open = app.prefs().sections.get(SECTION).copied().unwrap_or(true);
    let palette = Palette::current(ui);
    let at = egui::pos2(pane.right() - INSET_PX - CARD_PX, pane.top() + INSET_PX);
    let area = egui::Area::new(egui::Id::unique("toolpaths_card"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(palette.surface_raised)
                .stroke(Stroke::new(1.0, palette.border_default))
                .corner_radius(8)
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(MARGIN_PX)
                .show(ui, |ui| {
                    ui.set_width(CARD_PX - 2.0 * f32::from(MARGIN_PX));
                    let flipped = section(
                        ui,
                        egui::Id::unique("toolpaths_card_section"),
                        strings.get(StrKey::AppToolpathsTitle),
                        open,
                        None,
                        |ui| body(ui, app, view.visibility, view.colour, out),
                    );
                    if flipped {
                        out.push(Action::SetSection {
                            name: SECTION.to_owned(),
                            open: !open,
                        });
                    }
                });
        });
    let card = area.response.rect;
    if let Some(legend) = legend.filter(|_| view.colour == ColourMode::ByDepth) {
        draw_legend(ui, pane, card, app, legend);
    }
    card
}

/// One of the card's toggles: the flag of [`Visibility`] it flips.
type Flag = fn(&mut Visibility) -> &mut bool;

/// The card's body: the colour mode, the toggles and the two buttons.
fn body(ui: &mut Ui, app: &App, visibility: Visibility, colour: ColourMode, out: &mut Vec<Action>) {
    let strings = app.strings();
    let modes = [
        strings.get(StrKey::AppToolpathsByOperation),
        strings.get(StrKey::AppToolpathsByDepth),
    ];
    let selected = usize::from(colour == ColourMode::ByDepth);
    let width = ui.available_width();
    if let Some(chosen) = segmented(
        ui,
        egui::Id::unique("toolpaths_colour"),
        &modes,
        selected,
        width,
    ) {
        out.push(Action::SetColour(if chosen == 1 {
            ColourMode::ByDepth
        } else {
            ColourMode::ByOperation
        }));
    }
    ui.add_space(4.0);
    let toggles: [(StrKey, Flag); 8] = [
        (StrKey::AppToolpathsCuts, |v| &mut v.cuts),
        (StrKey::AppToolpathsRapids, |v| &mut v.rapids),
        (StrKey::AppToolpathsPlunges, |v| &mut v.plunges),
        (StrKey::AppToolpathsArrows, |v| &mut v.arrows),
        (StrKey::AppToolpathsStartPoints, |v| &mut v.start_points),
        (StrKey::AppToolpathsTabMarks, |v| &mut v.tab_marks),
        (StrKey::AppToolpathsOnlySelected, |v| &mut v.only_selected),
        (StrKey::AppToolpathsExtents, |v| &mut v.extents),
    ];
    for (key, flag) in toggles {
        let mut next = visibility;
        let value = flag(&mut next);
        if ui.checkbox(value, strings.get(key)).changed() {
            out.push(Action::SetVisibility(next));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let show = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppToolpathsShowAll));
        if button(ui, show).clicked() {
            out.push(Action::ShowAllToolpaths);
        }
        let hide = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppToolpathsHideAll));
        if button(ui, hide).clicked() {
            out.push(Action::HideAllToolpaths);
        }
    });
}

/// The depth legend left of `card`: the ramp as a 12 × 160 px bar, a tick
/// and a label at the stock top, each operation's depth and the deepest
/// cut, labels that would overlap stacked to the right; on a
/// `viewport.label.bg` pill so it reads over the toolpaths.
fn draw_legend(ui: &Ui, pane: Rect, card: Rect, app: &App, legend: &DepthLegend) {
    let palette = Palette::current(ui);
    let strings = app.strings();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("toolpaths_legend"),
    ));
    let small = TextStyle::Small.resolve(ui.style());
    let title = painter.layout_no_wrap(
        strings.get(StrKey::AppToolpathsLegend).to_owned(),
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    // Lay the labels out against a bar at x = 0, then place the whole.
    let title_h = title.size().y + 6.0;
    let mut rows = Vec::with_capacity(legend.ticks.len());
    let mut last: Option<(f32, f32)> = None;
    let mut width = LEGEND_BAR.x.max(title.size().x);
    for tick in &legend.ticks {
        let names = if tick.top {
            strings.get(StrKey::AppToolpathsStockTop).to_owned()
        } else {
            tick.operations.join(", ")
        };
        let galley = painter.layout_no_wrap(
            format!("{}  {names}", tick.label),
            small.clone(),
            palette.text_secondary,
        );
        let y = title_h + LEGEND_BAR.y * gpu_f32(tick.fraction);
        let mut x = LEGEND_BAR.x + 8.0;
        if let Some((last_y, last_right)) = last
            && (y - last_y).abs() < LABEL_GAP_PX
        {
            x = last_right + 8.0;
        }
        let right = x + galley.size().x;
        width = width.max(right);
        last = Some((y, right));
        rows.push((y, x, galley));
    }
    let pad = 8.0;
    let size = egui::vec2(width + 2.0 * pad, title_h + LEGEND_BAR.y + 2.0 * pad + 6.0);
    let left = (card.left() - INSET_PX - size.x).max(pane.left() + INSET_PX);
    let frame = Rect::from_min_size(egui::pos2(left, card.top()), size);
    painter.rect_filled(frame, 8.0, palette.viewport_label_bg);
    let origin = frame.min + egui::vec2(pad, pad);
    painter.galley(origin, title, palette.text_primary);
    let bar = Rect::from_min_size(origin + egui::vec2(0.0, title_h), LEGEND_BAR);
    let steps = 64_u8;
    for i in 0..steps {
        let (t0, t1) = (
            f32::from(i) / f32::from(steps),
            f32::from(i + 1) / f32::from(steps),
        );
        let slice = Rect::from_min_max(
            egui::pos2(bar.left(), bar.top() + bar.height() * t0),
            egui::pos2(bar.right(), bar.top() + bar.height() * t1),
        );
        painter.rect_filled(slice, 0.0, palette.ramp_at((t0 + t1) / 2.0));
    }
    painter.rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Outside,
    );
    for (y, x, galley) in rows {
        let y = origin.y + y;
        painter.line_segment(
            [egui::pos2(bar.right(), y), egui::pos2(bar.right() + 4.0, y)],
            Stroke::new(1.0, palette.text_secondary),
        );
        let rect = Align2::LEFT_CENTER.anchor_size(egui::pos2(origin.x + x, y), galley.size());
        painter.galley(rect.min, galley, palette.text_secondary);
    }
}

/// The extents' caption (`experience.md`, section 9: *Program extents
/// (bit edge, dashed)* and its ranges) in a `viewport.label.bg` pill at
/// the bit-edge box's top-left, `top_left` on the window.
pub(crate) fn extents_caption(
    ui: &Ui,
    painter: &egui::Painter,
    app: &App,
    extents: &ProgramExtents,
    top_left: egui::Pos2,
) {
    let palette = Palette::current(ui);
    let text = app.strings().fmt(
        StrKey::AppToolpathsExtentsCaption,
        &[
            ("x", &range_label(extents.edge_x, extents.units)),
            ("y", &range_label(extents.edge_y, extents.units)),
        ],
    );
    let galley = painter.layout_no_wrap(
        text,
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let rect = Rect::from_min_size(
        top_left + egui::vec2(0.0, -galley.size().y - 8.0),
        galley.size() + egui::vec2(12.0, 4.0),
    );
    painter.rect_filled(rect, 6.0, palette.viewport_label_bg);
    painter.galley(
        rect.min + egui::vec2(6.0, 2.0),
        galley,
        palette.text_secondary,
    );
}
