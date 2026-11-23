// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A list row's surface (`docs/specs/phase-4/redesign.md`, "Components",
//! "List row"): no rule between rows; `surface.hover` under the pointer,
//! `accent.soft` with a 3-px `accent` bar at its left edge when selected,
//! on its own 8-radius surface. And an editor row's parts
//! (`docs/specs/phase-4/experience.md`, sections 3
//! and 7): the 112-px label column whose hover shows the field's help
//! with its `.dcv` field name in JetBrains Mono ("Help affordance"), and
//! the notes under a row, which are not problems: what a value comes to,
//! why an option is missing ([`hint`]), an Inlay's derived lines
//! ([`line()`], [`derived`]). A problem takes no line under its field: it
//! is the field's outline and glyph, with the details in the glyph's
//! tooltip, the help line and the Warnings drawer
//! ([`crate::widgets::problem`]; "Field problem"). Every state has an
//! outline and a glyph, never colour alone (section 13, items 53 and
//! 54).

use eframe::egui::{self, Rect, Response, RichText, Sense, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;
use crate::widgets::unit_field::HEIGHT_PX;

/// The label column's width, px (section 3).
pub(crate) const LABEL_PX: f32 = 112.0;
/// The gap between a label and its field, px.
pub(crate) const GAP_PX: f32 = 8.0;
/// How far a list row's surface sits in from its card's edges, px: with
/// the row's own 8 px of padding, its content starts at the card padding.
pub(crate) const INSET_PX: f32 = 8.0;

/// The rectangle of a list row's surface in the `slot` the list gave it:
/// inset from the card's edges, a hairline off the rows above and below.
pub(crate) fn surface_rect(slot: Rect) -> Rect {
    slot.shrink2(egui::vec2(INSET_PX, 1.0))
}

/// Paints a list row's surface in `rect` ([`surface_rect`]): `accent.soft`
/// and the accent bar when `selected`, else `surface.hover` when
/// `hovered`, else nothing.
pub(crate) fn surface(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    let palette = Palette::current(ui);
    if selected {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.accent_soft);
        // The bar clears the surface's rounded corners.
        let inset = (theme::CONTROL_RADIUS_PX - 1.0).min(rect.height() / 4.0);
        let bar = Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + inset),
            egui::pos2(rect.left() + theme::SELECTED_BAR_PX, rect.bottom() - inset),
        );
        ui.painter()
            .rect_filled(bar, theme::pill(theme::SELECTED_BAR_PX), palette.accent);
    } else if hovered {
        ui.painter()
            .rect_filled(rect, theme::CONTROL_RADIUS_PX, palette.surface_hover);
    }
}

/// A label laid out for the label column: the whole of `text`, wrapped at
/// its words to the column's 112 px. A label is never cut: in a tool
/// where the label is the instruction, "Through-cut all…" says nothing.
/// Two lines of the body face are a control tall, so a wrapped label
/// keeps its row's height and the rows stay aligned.
pub(crate) fn label_galley(
    ui: &Ui,
    text: &str,
    colour: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout(
        text.to_owned(),
        TextStyle::Body.resolve(ui.style()),
        colour,
        LABEL_PX,
    )
}

/// Draws a field's label in its column, in text.secondary
/// (`experience.md`, section 4, "Field label"): `text` whole, on one line
/// or wrapped to two ([`label_galley`]), centred on the control beside
/// it. The slot is a control tall, and taller only for a label of three
/// lines. `text` is its accessible name. `underlined` draws the dotted
/// rule under each line that says the label has help.
pub(crate) fn label_slot(ui: &mut Ui, text: &str, help: bool) -> Response {
    let palette = Palette::current(ui);
    let colour = if ui.is_enabled() {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    let galley = label_galley(ui, text, colour);
    let height = HEIGHT_PX.max(galley.size().y);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(LABEL_PX, height), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    if help && response.hovered() {
        for line in &galley.rows {
            let line = line.rect().translate(at.to_vec2());
            ui.painter().extend(egui::Shape::dashed_line(
                &[
                    egui::pos2(line.left(), line.bottom()),
                    egui::pos2(line.right(), line.bottom()),
                ],
                Stroke::new(1.0, palette.text_disabled),
                1.0,
                2.0,
            ));
        }
    }
    ui.painter().galley(at, galley, colour);
    response
}

/// A field's label, `text`, in the 112-px column ([`label_slot`]);
/// hovering it shows `help` and, on a second line in the mono face,
/// `path`, the `.dcv` field it writes. Its dotted underline appears on
/// hover to say it has help.
pub(crate) fn label(ui: &mut Ui, text: &str, help: &str, path: Option<&str>) -> Response {
    let palette = Palette::current(ui);
    // The control drawn next under this name takes the help: its tooltip,
    // and the panel's help line while it has the keyboard.
    crate::widgets::help::offer(ui, text, help);
    let response = label_slot(ui, text, true);
    response.on_hover_ui(|ui| {
        ui.label(RichText::new(help).text_style(TextStyle::Body));
        if let Some(path) = path {
            ui.label(
                RichText::new(path)
                    .text_style(TextStyle::Monospace)
                    .color(palette.text_secondary),
            );
        }
    })
}

/// A field's label on a bar, where there is no label column: `text` at
/// its own width in text.secondary, with `help` on hover, which the
/// control drawn after it takes as [`label`]'s does.
pub(crate) fn inline_label(ui: &mut Ui, text: &str, help: &str) -> Response {
    let palette = Palette::current(ui);
    crate::widgets::help::offer(ui, text, help);
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .text_style(TextStyle::Body)
                .color(palette.text_secondary),
        )
        .selectable(false)
        .sense(egui::Sense::hover()),
    )
    .on_hover_text(help)
}

/// A file path on one line in the mono face, cut at its start with an
/// ellipsis when it is longer than the room, since a path's end says the
/// most. Its accessible name and its tooltip are the whole path.
pub(crate) fn path(ui: &mut Ui, path: &str) {
    let palette = Palette::current(ui);
    let font = TextStyle::Monospace.resolve(ui.style());
    let room = ui.available_width();
    let layout = |text: String| {
        ui.painter()
            .layout_no_wrap(text, font.clone(), palette.text_primary)
    };
    let mut galley = layout(path.to_owned());
    let mut cut = 0;
    let chars: Vec<char> = path.chars().collect();
    while galley.size().x > room && cut + 1 < chars.len() {
        cut += 1;
        let tail: String = chars.iter().skip(cut).collect();
        galley = layout(format!("…{tail}"));
    }
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x.min(room), galley.size().y),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, path));
    ui.painter_at(rect.expand(1.0))
        .galley(rect.min, galley, palette.text_primary);
    let _ = response.on_hover_text(path);
}

/// A derived note under a field (an Inlay's board left under the band):
/// the line [`hint`] draws, in the error colour with its glyph while
/// `alert`, since what it derives is out of range. The line is there
/// either way, so it moves no row; a problem takes none.
pub(crate) fn derived(ui: &mut Ui, alert: bool, text: &str) {
    if !alert {
        hint(ui, text);
        return;
    }
    let palette = Palette::current(ui);
    note(
        ui,
        FIELD_COLUMN_PX,
        Icon::CircleX,
        palette.status_error.stroke,
        palette.status_error.text,
        text,
    );
}

/// A note under a row in text.secondary: what a switch kept, why an
/// option is missing.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    let palette = Palette::current(ui);
    note(
        ui,
        FIELD_COLUMN_PX,
        Icon::Info,
        palette.text_secondary,
        palette.text_secondary,
        text,
    );
}

/// A line of a row that has no field, across the row's whole width under
/// its label (an Inlay's derived lines, which are sentences): in
/// text.secondary with the `info` glyph, or as an `alert` in the error
/// colour with its glyph.
pub(crate) fn line(ui: &mut Ui, alert: bool, text: &str) {
    let palette = Palette::current(ui);
    let (icon, glyph, colour) = if alert {
        (
            Icon::CircleX,
            palette.status_error.stroke,
            palette.status_error.text,
        )
    } else {
        (Icon::Info, palette.text_secondary, palette.text_secondary)
    };
    note(ui, 0.0, icon, glyph, colour, text);
}

/// A warning that is about the whole of what the panel shows, not one
/// field of it (an operation that cuts nothing): `title` in the warning
/// colour with its glyph, on the warning's tint, and each of `reasons`
/// under it. It is one thing to a screen reader, an alert.
pub(crate) fn warning_banner(ui: &mut Ui, title: &str, reasons: &[String]) {
    let palette = Palette::current(ui);
    let colours = palette.status_warning;
    let shown = egui::Frame::NONE
        .fill(colours.tint)
        .stroke(egui::Stroke::new(1.0, colours.stroke))
        .corner_radius(theme::INNER_CARD_RADIUS_PX)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            note(
                ui,
                0.0,
                Icon::TriangleAlert,
                colours.stroke,
                colours.text,
                title,
            );
            for reason in reasons {
                ui.add(
                    egui::Label::new(
                        RichText::new(reason)
                            .text_style(TextStyle::Small)
                            .color(palette.text_primary),
                    )
                    .wrap(),
                );
            }
        });
    let whole = std::iter::once(title)
        .chain(reasons.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    shown
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Alert, true, &whole));
}

/// Where the field column starts, px from the row's left: after the label
/// and its gap.
const FIELD_COLUMN_PX: f32 = LABEL_PX + GAP_PX;

/// `icon` in `glyph` and `text` in `colour`, `indent` px in from the row's
/// left ([`FIELD_COLUMN_PX`] puts it under the field).
fn note(
    ui: &mut Ui,
    indent: f32,
    icon: Icon,
    glyph: egui::Color32,
    colour: egui::Color32,
    text: &str,
) {
    ui.horizontal_top(|ui| {
        ui.add_space(indent);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(theme::ICON_SMALL_PX, theme::ICON_SMALL_PX),
            Sense::hover(),
        );
        let galley = icon.galley(ui, theme::ICON_SMALL_PX, glyph);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, glyph);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .text_style(TextStyle::Small)
                    .color(colour),
            )
            .wrap()
            .selectable(false),
        );
    });
}

#[cfg(test)]
mod tests {
    use dc_app::StrKey;

    use super::*;

    /// Every field label of the string table (an entry with a `_help`
    /// beside it) is drawn whole in the 112-px label column: on one line
    /// or two, which is a control's height, so no row grows and none is
    /// cut. *Through-cut allowance*, *Tool-change machine Z* and *Canned
    /// drill cycles* were cut to "Through-cut all…", "Tool-change m…" and
    /// "Canned drill cy…".
    #[test]
    fn every_field_label_fits_its_column_whole() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, dc_app::Theme::Dark);
        let labels: Vec<&str> = StrKey::ALL
            .iter()
            .filter(|key| {
                let help = format!("{}_help", key.key());
                StrKey::ALL.iter().any(|k| k.key() == help)
            })
            .map(|key| key.english())
            .collect();
        for long in [
            "Through-cut allowance",
            "Tool-change machine Z",
            "Canned drill cycles",
        ] {
            assert!(labels.contains(&long), "{long} is a field label");
        }
        let mut wrapped = 0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for label in &labels {
                let galley = label_galley(ui, label, theme::NONE);
                let drawn: String = galley
                    .rows
                    .iter()
                    .map(|row| row.text())
                    .collect::<Vec<_>>()
                    .join(" ");
                let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(!galley.elided, "{label} is cut");
                assert_eq!(words(&drawn), words(label), "{label} is drawn whole");
                assert!(galley.rows.len() <= 2, "{label} takes more than two lines");
                assert!(
                    galley.size().x <= LABEL_PX && galley.size().y <= HEIGHT_PX,
                    "{label} is {:?}, outside its {LABEL_PX} × {HEIGHT_PX} slot",
                    galley.size()
                );
                wrapped += usize::from(galley.rows.len() == 2);
            }
        });
        output.textures_delta.clear();
        assert!(wrapped >= 3, "the long labels wrap: {wrapped} did");
    }
}
