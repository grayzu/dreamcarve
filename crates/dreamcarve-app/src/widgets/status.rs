// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The marks of a list row's status cell (`experience.md`, section 7,
//! "Status cell"): `triangle-alert` and a count in status.warning, and
//! the filled status.error disc. One painter for each, so an operation's
//! row and a tool's row carry the same mark for the same thing.

use eframe::egui::{self, Rect, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// The radius of the error mark's disc, px.
const ERROR_DISC_PX: f32 = 7.0;
/// The size of the cross cut out of the disc, px.
const ERROR_CROSS_PX: f32 = 12.0;

/// The warning mark, centred in `rect`: the warning glyph and `count`.
pub(crate) fn warnings(ui: &Ui, rect: Rect, count: usize) {
    let palette = Palette::current(ui);
    let glyph = Icon::TriangleAlert.galley(ui, theme::ICON_SMALL_PX, palette.status_warning.stroke);
    let number = ui.painter().layout_no_wrap(
        count.to_string(),
        TextStyle::Small.resolve(ui.style()),
        palette.status_warning.text,
    );
    let total = glyph.size().x + 2.0 + number.size().x;
    let x = rect.center().x - total / 2.0;
    let glyph_width = glyph.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - glyph.size().y / 2.0),
        glyph,
        palette.status_warning.stroke,
    );
    ui.painter().galley(
        egui::pos2(
            x + glyph_width + 2.0,
            rect.center().y - number.size().y / 2.0,
        ),
        number,
        palette.status_warning.text,
    );
}

/// The error mark, centred in `rect`: a filled disc with the cross cut
/// out of it in the card's own colour.
pub(crate) fn error(ui: &Ui, rect: Rect) {
    let palette = Palette::current(ui);
    ui.painter()
        .circle_filled(rect.center(), ERROR_DISC_PX, palette.status_error.stroke);
    let cross = Icon::X.galley(ui, ERROR_CROSS_PX, palette.surface_panel);
    ui.painter().galley(
        rect.center() - cross.size() / 2.0,
        cross,
        palette.surface_panel,
    );
}
