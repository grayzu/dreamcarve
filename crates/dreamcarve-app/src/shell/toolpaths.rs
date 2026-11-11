// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The toolpath view (functional spec, capability 73; `experience.md`,
//! section 9, "Toolpath view popover"; mock-up `09-preview-legend.svg`;
//! slice 24, #616): a popover above the status bar's *Toolpaths* button,
//! 240 px wide on `surface.raised`, closed by a click elsewhere or
//! Escape. It holds the colour mode with its key under it (each
//! operation's swatch and name by operation, a line on the ramp by
//! depth), the eight layer toggles, and *Show all* / *Hide all*. It takes
//! no room on the Viewport. Everything here is view state: the popover
//! emits actions the window turns into `dc_app` view settings and
//! `gui.toml` preferences, never commands.

use dc_app::view::{ColourMode, Visibility, operation_key};
use dc_app::{App, StrKey};
use eframe::egui::{self, Id, Response, Sense, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::Action;
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::checkbox::checkbox;
use crate::widgets::segmented::segmented;

/// The popover's width, px.
const WIDTH_PX: f32 = 240.0;
/// Its inner margin, px.
const MARGIN_PX: i8 = 12;
/// A key row, px.
const KEY_ROW_PX: f32 = 24.0;
/// A key swatch's side, px.
const SWATCH_PX: f32 = 10.0;
/// The key lists this many operations, then *and n more*.
const KEY_MAX: usize = 8;

/// The popover's id, which its button toggles.
pub(crate) fn popover_id() -> Id {
    Id::unique("status.toolpaths.popover")
}

/// Whether the popover is open.
pub(crate) fn is_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_id_open(ctx, popover_id())
}

/// The popover above `button` (drawn in `ui`), toggled by it.
pub(crate) fn popover(ui: &Ui, button: &Response, app: &App, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    egui::Popup::from_toggle_button_response(button)
        .id(popover_id())
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(WIDTH_PX)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_raised)
                .stroke(Stroke::new(1.0, palette.border_default))
                .corner_radius(8)
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(MARGIN_PX),
        )
        .show(|ui| body(ui, app, out));
}

/// One of the toggles: the flag of [`Visibility`] it flips.
type Flag = fn(&mut Visibility) -> &mut bool;

/// The popover's content.
fn body(ui: &mut Ui, app: &App, out: &mut Vec<Action>) {
    let strings = app.strings();
    let view = app.view();
    let palette = Palette::current(ui);
    ui.set_width(WIDTH_PX - 2.0 * f32::from(MARGIN_PX));
    ui.spacing_mut().item_spacing.y = 4.0;
    text(
        ui,
        strings.get(StrKey::AppToolpathsTitle),
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    let modes = [
        strings.get(StrKey::AppToolpathsByOperation),
        strings.get(StrKey::AppToolpathsByDepth),
    ];
    let selected = usize::from(view.colour == ColourMode::ByDepth);
    let width = ui.available_width();
    if let Some(chosen) = segmented(ui, Id::unique("toolpaths.colour"), &modes, selected, width) {
        out.push(Action::SetColour(if chosen == 1 {
            ColourMode::ByDepth
        } else {
            ColourMode::ByOperation
        }));
    }
    match view.colour {
        ColourMode::ByOperation => operation_rows(ui, app),
        ColourMode::ByDepth => {
            text(
                ui,
                strings.get(StrKey::AppToolpathsKeyDepth),
                TextStyle::Small,
                palette.text_secondary,
            );
        }
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
        let mut next = view.visibility;
        let value = flag(&mut next);
        if checkbox(ui, key, value, strings.get(key)).changed() {
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

/// The by-operation key: one 24-px row per operation, a 10-px swatch in
/// the colour the Viewport draws it and its name, at most eight, then
/// *and n more*.
fn operation_rows(ui: &mut Ui, app: &App) {
    let palette = Palette::current(ui);
    let key = operation_key(app.scene());
    for entry in key.iter().take(KEY_MAX) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), KEY_ROW_PX), Sense::hover());
        let swatch = egui::Rect::from_center_size(
            egui::pos2(rect.left() + SWATCH_PX / 2.0, rect.center().y),
            egui::vec2(SWATCH_PX, SWATCH_PX),
        );
        ui.painter()
            .rect_filled(swatch, 2.0, palette.operation(entry.colour));
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("toolpaths.key", entry.id.as_str()))
                .max_rect(rect.with_min_x(swatch.right() + 8.0))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        text(&mut row, &entry.name, TextStyle::Body, palette.text_primary);
    }
    if key.len() > KEY_MAX {
        let more = app.strings().fmt(
            StrKey::AppToolpathsKeyMore,
            &[("count", &(key.len() - KEY_MAX).to_string())],
        );
        text(ui, &more, TextStyle::Small, palette.text_secondary);
    }
}
