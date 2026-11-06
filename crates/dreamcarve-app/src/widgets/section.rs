// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The disclosure section (`docs/specs/phase-4/experience.md`, section 7,
//! "Disclosure section header"; section 12, `section_header`): a 28-px
//! full-width header, 13-px SemiBold, the chevron left, and, while the
//! section is closed, the glyph and count of the located messages inside
//! it at the right end so nothing hides. Whether it is open is a
//! preference (`gui.toml`'s `sections`), passed in and reported back, not
//! held here; egui's `CollapsingState` only animates the body.

use dc_job::Tier;
use eframe::egui::{self, Align2, Sense, TextStyle, Ui, collapsing_header::CollapsingState};

use crate::shell::theme::{self, Palette};
use crate::widgets::button::focus_ring;
use crate::widgets::icon::Icon;

/// The header's height, px (section 3).
const HEADER_PX: f32 = 28.0;

/// Draws a section titled `title`, open when `open`, whose body holds
/// `located` messages (the worst tier and how many); `body` draws it.
/// Returns whether the header was clicked, to flip the preference.
pub(crate) fn section(
    ui: &mut Ui,
    id: egui::Id,
    title: &str,
    open: bool,
    located: Option<(Tier, usize)>,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let palette = Palette::current(ui);
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, open);
    state.set_open(open);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_PX), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, title));
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    }
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let glyph = chevron.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    let glyph_at = egui::pos2(rect.left() + 4.0, rect.center().y - glyph.size().y / 2.0);
    ui.painter().galley(glyph_at, glyph, palette.text_secondary);
    ui.painter().text(
        egui::pos2(
            rect.left() + 4.0 + theme::ICON_SMALL_PX + 4.0,
            rect.center().y,
        ),
        Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    if let (false, Some((tier, count))) = (open, located) {
        let (icon, colours) = match tier {
            Tier::Error => (Icon::CircleX, palette.status_error),
            Tier::Warning => (Icon::TriangleAlert, palette.status_warning),
            Tier::Hint => (Icon::Info, palette.status_info),
        };
        let count = ui.painter().layout_no_wrap(
            count.to_string(),
            TextStyle::Small.resolve(ui.style()),
            colours.text,
        );
        let count_at = egui::pos2(
            rect.right() - 8.0 - count.size().x,
            rect.center().y - count.size().y / 2.0,
        );
        let glyph = icon.galley(ui, theme::ICON_SMALL_PX, colours.stroke);
        let glyph_at = egui::pos2(
            count_at.x - 2.0 - glyph.size().x,
            rect.center().y - glyph.size().y / 2.0,
        );
        ui.painter().galley(count_at, count, colours.text);
        ui.painter().galley(glyph_at, glyph, colours.stroke);
    }
    focus_ring(ui, &response);
    state.show_body_unindented(ui, body);
    response.clicked()
}
