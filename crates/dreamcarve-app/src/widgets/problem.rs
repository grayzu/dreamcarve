// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A field's problem mark (`docs/specs/phase-4/experience.md`, section 7,
//! "Field problem"; functional spec, capabilities 60, 63, 66 and 66b): no
//! message is laid out under a field. What is wrong at it
//! (`dc_app::problems::Problems`) shows as the field's outline in the
//! worst tier's stroke and that tier's glyph, inside the field at its
//! right or beside a control that has no inside ([`mark`]; the text field
//! draws its own with [`glyph`]). The details are:
//!
//! - the **tooltip** of the glyph and of the field ([`tooltip`]): every
//!   problem, most severe first, its tier's word, its message and its
//!   code in the mono face;
//! - the field's **accessible description** ([`set_description`]), the
//!   same words, and the glyph's own name;
//! - the **help line** at the foot of the field's panel while the field
//!   has the keyboard ([`crate::widgets::help::problems`]);
//! - the **Warnings drawer**: a click on the glyph opens it at the row of
//!   the most severe problem that is a finding ([`take_opened`]). A
//!   refused entry is not one, and its glyph takes no click.

use dc_app::problems::Problems;
use dc_app::warnings::RowKey;
use dc_job::Tier;
use eframe::egui::{
    self, Id, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Ui,
};

use crate::shell::theme::{self, Palette, StatusColours};
use crate::widgets::icon::Icon;

/// The glyph's size, px (`experience.md`, section 7).
pub(crate) const GLYPH_PX: f32 = theme::ICON_SMALL_PX;
/// The gap between a control and the glyph beside it, px.
const GAP_PX: f32 = 8.0;
/// The room a glyph beside a control takes at the row's end, px.
pub(crate) const BESIDE_PX: f32 = GLYPH_PX + GAP_PX;
/// The glyph's hit area, px (section 11, "Hit sizes").
const HIT_PX: f32 = 24.0;

/// `tier`'s glyph and colours (section 5, "Status").
pub(crate) fn look(palette: &Palette, tier: Tier) -> (Icon, StatusColours) {
    match tier {
        Tier::Error => (Icon::CircleX, palette.status_error),
        Tier::Warning => (Icon::TriangleAlert, palette.status_warning),
        Tier::Hint => (Icon::Info, palette.status_info),
    }
}

/// The stroke of a field's outline while `tier` is the worst at it.
pub(crate) fn stroke(palette: &Palette, tier: Tier) -> egui::Color32 {
    look(palette, tier).1.stroke
}

/// Where a glyph click waits for the window.
fn opened_id() -> Id {
    Id::unique("dc.problem.opened")
}

/// The drawer row a glyph click asked for this frame, taken once: the
/// window opens the Warnings drawer at it.
pub(crate) fn take_opened(ctx: &egui::Context) -> Option<RowKey> {
    let key = ctx.data(|d| d.get_temp::<RowKey>(opened_id()))?;
    ctx.data_mut(|d| d.remove::<RowKey>(opened_id()));
    Some(key)
}

/// The tooltip's content: each problem, most severe first, as its tier's
/// glyph and word in the tier's colour, its message, and its code in the
/// mono face; wrapped at the tooltip's width.
pub(crate) fn tooltip(ui: &mut Ui, problems: &Problems) {
    let palette = Palette::current(ui);
    ui.set_max_width(ui.spacing().tooltip_width);
    ui.spacing_mut().item_spacing.y = 2.0;
    for (n, problem) in problems.iter().enumerate() {
        if n > 0 {
            ui.add_space(6.0);
        }
        let (icon, colours) = look(palette, problem.tier);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let galley = icon.galley(ui, 12.0, colours.stroke);
            let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
            ui.painter().galley(rect.min, galley, colours.stroke);
            ui.label(
                RichText::new(&problem.word)
                    .text_style(TextStyle::Small)
                    .color(colours.text),
            );
        });
        ui.add(
            egui::Label::new(RichText::new(&problem.message).text_style(TextStyle::Body)).wrap(),
        );
        if let Some(code) = &problem.code {
            ui.label(
                RichText::new(code)
                    .text_style(TextStyle::Monospace)
                    .color(palette.text_secondary),
            );
        }
    }
}

/// The glyph of `problems`' worst tier, centred on `centre`, as the
/// widget `id`: named for assistive tools with every problem
/// (`Problems::text`), showing them as its tooltip, and, where one is a
/// finding of the project, a button that opens the Warnings drawer at
/// its row. It does not take the keyboard: Tab goes from field to field.
pub(crate) fn glyph(ui: &Ui, id: Id, centre: Pos2, problems: &Problems) -> Option<Response> {
    let tier = problems.worst()?;
    let palette = Palette::current(ui);
    let (icon, colours) = look(palette, tier);
    let hit = Rect::from_center_size(centre, egui::vec2(HIT_PX, HIT_PX));
    let row = problems.row().cloned();
    let (sense, role) = if row.is_some() {
        (Sense::CLICK, egui::Role::Button)
    } else {
        (Sense::hover(), egui::Role::Image)
    };
    let response = ui.interact(hit, id, sense);
    let name = problems.text();
    response.widget_info(|| egui::WidgetInfo::labeled(role, true, &name));
    if row.is_some() && response.hovered() {
        ui.painter()
            .rect_filled(hit, theme::pill(HIT_PX), colours.tint);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let galley = icon.galley(ui, GLYPH_PX, colours.stroke);
    ui.painter()
        .galley(centre - galley.size() / 2.0, galley, colours.stroke);
    if let (true, Some(row)) = (response.clicked(), row) {
        ui.data_mut(|d| d.insert_temp(opened_id(), row));
        ui.ctx().request_repaint();
    }
    Some(response.on_hover_ui(|ui| tooltip(ui, problems)))
}

/// Sets the accessible description of the widget `id` to `problems`'
/// words (`Problems::text`), then `help` on its own line: what the
/// tooltips say, for a reader who cannot hover.
pub(crate) fn set_description(ui: &Ui, id: Id, problems: &Problems, help: Option<&str>) {
    if problems.is_empty() || !ui.ctx().has_accesskit_node(id) {
        return;
    }
    let text = match help {
        Some(help) if !help.is_empty() => format!("{}\n{help}", problems.text()),
        _ => problems.text(),
    };
    ui.ctx()
        .accesskit_node_builder(id, |node| node.set_description(text));
}

/// Gives `response` its tooltips: `problems` first, then `help`, each a
/// tooltip of its own; a disabled control shows the problems too.
pub(crate) fn hover(response: Response, problems: &Problems, help: Option<&str>) -> Response {
    let response = if problems.is_empty() {
        response
    } else if response.enabled() {
        response.on_hover_ui(|ui| tooltip(ui, problems))
    } else {
        response.on_disabled_hover_ui(|ui| tooltip(ui, problems))
    };
    match help {
        Some(help) if !help.is_empty() => response.on_hover_text(help),
        _ => response,
    }
}

/// Where a control's glyph goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Place {
    /// Inside the control, its centre this far from the control's right
    /// edge, on a patch of the control's own fill, so nothing of the
    /// control shows through it (a dropdown's chosen text, cut before
    /// its chevron).
    Inside {
        /// The glyph's centre, px from the control's right edge.
        from_right_px: f32,
        /// The control's fill there.
        fill: egui::Color32,
    },
    /// Beside the control, at its right: a control with no inside (a
    /// checkbox, a switch, a pill control). The row leaves
    /// [`BESIDE_PX`] for it ([`with_room`]).
    Beside,
}

/// The room a dropdown's glyph takes before its chevron, px.
const IN_DROPDOWN_PX: f32 = GLYPH_PX + 8.0;

/// A dropdown's glyph place: inside it, before its chevron. The
/// dropdown's chosen text stops short of it ([`dropdown_text`]), so the
/// glyph needs no patch under it: a dropdown's fill under the pointer is
/// a wash, which a patch of the same wash would double.
pub(crate) fn in_dropdown(ui: &Ui) -> Place {
    Place::Inside {
        // Past the chevron's square and the dropdown's own padding.
        from_right_px: ui.spacing().button_padding.x
            + ui.spacing().icon_width
            + IN_DROPDOWN_PX / 2.0,
        fill: theme::NONE,
    }
}

/// `text` as a dropdown `width_px` wide shows it while `problems` has
/// any: cut with an ellipsis where the problem's glyph stands, before
/// the chevron, so the two never overlap. With no problem it is `text`,
/// which the dropdown cuts at its chevron itself.
pub(crate) fn dropdown_text(ui: &Ui, text: &str, width_px: f32, problems: &Problems) -> String {
    if problems.is_empty() {
        return text.to_owned();
    }
    let spacing = ui.spacing();
    let room = width_px
        - 2.0 * spacing.button_padding.x
        - spacing.icon_width
        - spacing.item_spacing.x
        - IN_DROPDOWN_PX;
    let font = TextStyle::Button.resolve(ui.style());
    let width_of = |text: String| {
        ui.painter()
            .layout_no_wrap(text, font.clone(), theme::NONE)
            .size()
            .x
    };
    if width_of(text.to_owned()) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut kept = chars.len();
    while kept > 0 {
        kept -= 1;
        let cut: String = chars.iter().take(kept).collect();
        let cut = format!("{}…", cut.trim_end());
        if width_of(cut.clone()) <= room {
            return cut;
        }
    }
    String::new()
}

/// Finishes a marked dropdown whose chosen text is `whole`: where the
/// glyph's room cut it ([`dropdown_text`]), assistive tools still read
/// the whole of it as the dropdown's value, and it is the dropdown's
/// last tooltip, as for any text the window cuts.
pub(crate) fn dropdown_whole(ui: &Ui, dropdown: Response, whole: &str) -> Response {
    let cut = ui
        .ctx()
        .accesskit_node_builder(dropdown.id, |node| {
            let cut = node.value().is_some_and(|shown| shown != whole);
            if cut {
                node.set_value(whole);
            }
            cut
        })
        .unwrap_or(false);
    if cut {
        dropdown.on_hover_text(whole)
    } else {
        dropdown
    }
}

/// Draws `add`'s control in what `ui`'s row has left, less the glyph's
/// room while `problems` has any, so a control that fills the row ends
/// before the glyph drawn beside it ([`Place::Beside`]). The row's height
/// does not change, and with no problem the control has the whole row.
/// The control draws in a child of its own either way, so its id, and
/// with it the focus, does not change as a problem comes and goes.
pub(crate) fn with_room<R>(ui: &mut Ui, problems: &Problems, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut room = ui.available_rect_before_wrap();
    if !problems.is_empty() {
        room.max.x = (room.max.x - BESIDE_PX).max(room.min.x);
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt("dc.problem.room")
            .max_rect(room),
        add,
    )
    .inner
}

/// Finishes the control `response`, named `name`: the help its label
/// offered (its tooltip, and the foot's line while it has the keyboard,
/// as [`crate::widgets::help::control`] gives it), and, while `problems`
/// has any, the mark: the outline `shape` (the control's frame and its
/// corner radius) in the worst tier's stroke, the glyph at `place`, the
/// problems as the control's first tooltip and at the head of its
/// accessible description, and on the help line before the help.
pub(crate) fn mark(
    ui: &Ui,
    response: Response,
    name: &str,
    problems: &Problems,
    shape: Option<(Rect, f32)>,
    place: Place,
) -> Response {
    let help = crate::widgets::help::take(ui, name);
    let focused = response.has_focus();
    if focused && let Some(help) = &help {
        let _ = crate::widgets::help::focused(ui, name, help);
    }
    let Some(tier) = problems.worst() else {
        return hover(response, problems, help.as_deref());
    };
    let palette = Palette::current(ui);
    let frame = shape.map_or(response.rect, |(rect, _)| rect);
    if let Some((rect, radius)) = shape {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, stroke(palette, tier)),
            StrokeKind::Inside,
        );
    }
    let centre = match place {
        Place::Inside {
            from_right_px,
            fill,
        } => {
            let centre = egui::pos2(frame.right() - from_right_px, frame.center().y);
            let patch = Rect::from_center_size(
                centre,
                egui::vec2(GLYPH_PX + 8.0, (frame.height() - 4.0).max(GLYPH_PX)),
            );
            ui.painter().rect_filled(patch, 0.0, fill);
            centre
        }
        // Beside the whole control (a checkbox's label with its box),
        // on its first line.
        Place::Beside => egui::pos2(
            response.rect.right() + GAP_PX + GLYPH_PX / 2.0,
            response
                .rect
                .center()
                .y
                .min(response.rect.top() + theme::CONTROL_PX / 2.0),
        ),
    };
    let _ = glyph(ui, response.id.with("problem"), centre, problems);
    if focused {
        let _ = crate::widgets::help::problems(ui, name, problems);
    }
    // The help's tooltip names the control where it has no name yet, and
    // would describe it: the problems' words go first.
    let response = hover(response, problems, help.as_deref());
    set_description(ui, response.id, problems, help.as_deref());
    response
}

#[cfg(test)]
mod tests {
    use eframe::egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    use super::*;
    use crate::widgets::checkbox::{box_of, checkbox};
    use crate::widgets::row;
    use crate::widgets::segmented::segmented;
    use crate::widgets::toggle::{pill_of, toggle};
    use crate::widgets::unit_field::{UnitField, field};

    /// The gallery's width, px: an Inspector's content.
    const WIDTH_PX: f32 = 296.0;

    /// One row of each kind of control, each marked with `problems`: a
    /// text field with a unit, a dropdown, a pill control, a checkbox and
    /// a switch.
    fn gallery(ui: &mut Ui, problems: &Problems) {
        // The app's styles are the context's from the frame after they
        // are applied.
        let styled = ui
            .style()
            .text_styles
            .contains_key(&TextStyle::Name(theme::MEDIUM_STYLE.into()));
        if !styled {
            theme::apply(ui.ctx(), dc_app::Theme::Dark);
            ui.ctx().request_repaint();
            return;
        }
        ui.set_width(WIDTH_PX);
        ui.horizontal(|ui| {
            row::label(ui, "Depth", "How deep.", None);
            field(
                ui,
                UnitField {
                    problems: Some(problems),
                    ..UnitField::new(Id::unique("t.depth"), "Depth", "12", "mm")
                },
            );
        });
        ui.horizontal(|ui| {
            row::label(ui, "Side", "Which side.", None);
            let width = ui.available_width();
            let chosen = "A long choice that is cut before the chevron";
            let dropdown = egui::ComboBox::from_id_salt("t.side")
                .icon(crate::widgets::icon::combo_chevron)
                .truncate()
                .selected_text(dropdown_text(ui, chosen, width, problems))
                .width(width)
                .show_ui(ui, |_| {})
                .response
                .accessible_name("Side");
            let shape = (dropdown.rect, theme::CONTROL_RADIUS_PX);
            let place = in_dropdown(ui);
            mark(ui, dropdown, "Side", problems, Some(shape), place);
        });
        ui.horizontal(|ui| {
            row::label(ui, "Direction", "Which way.", None);
            let shown = with_room(ui, problems, |ui| {
                ui.scope(|ui| {
                    segmented(
                        ui,
                        Id::unique("t.direction"),
                        &["Climb", "Conventional"],
                        0,
                        ui.available_width(),
                    )
                })
            });
            let track = ui.interact(shown.response.rect, Id::unique("t.track"), Sense::hover());
            track.widget_info(|| egui::WidgetInfo::labeled(Role::Group, true, "Direction"));
            let shape = (track.rect, theme::pill(theme::CONTROL_PX));
            mark(ui, track, "Direction", problems, Some(shape), Place::Beside);
        });
        ui.horizontal(|ui| {
            let mut on = true;
            let ticked = with_room(ui, problems, |ui| {
                checkbox(ui, "t.finish", &mut on, "Finish pass")
            });
            let shape = box_of(&ticked, true);
            mark(
                ui,
                ticked,
                "Finish pass",
                problems,
                Some(shape),
                Place::Beside,
            );
        });
        ui.horizontal(|ui| {
            row::label(ui, "Lead", "A lead in.", None);
            let switch = toggle(ui, true, "Lead");
            let shape = pill_of(&switch);
            mark(ui, switch, "Lead", problems, Some(shape), Place::Beside);
        });
    }

    /// Where each control of the gallery is, by name.
    fn controls(h: &Harness<'_>) -> Vec<(&'static str, Rect)> {
        [
            (Role::TextInput, "Depth"),
            (Role::ComboBox, "Side"),
            (Role::Group, "Direction"),
            (Role::CheckBox, "Finish pass"),
            (Role::Switch, "Lead"),
        ]
        .into_iter()
        .map(|(role, name)| (name, h.get_by_role_and_label(role, name).rect()))
        .collect()
    }

    /// "Field problem": every kind of control shows the mark, and no row
    /// moves or grows for it. With a warning on each, every control is
    /// where it was and as tall as it was; each has one glyph, named with
    /// the problem, on its own line; a field's and a dropdown's is inside
    /// it (the dropdown's clear of its chevron), and a pill control's, a
    /// checkbox's and a switch's is beside it, inside the row's width.
    #[test]
    fn every_kind_of_control_is_marked_and_no_row_grows() {
        let strings = dc_app::Strings::english();
        let warned = Problems::said(&strings, Tier::Warning, "too fast");
        let mut plain = Harness::new_ui(|ui| gallery(ui, &Problems::none()));
        plain.run();
        let mut marked = Harness::new_ui(move |ui| gallery(ui, &warned));
        marked.run();
        let before = controls(&plain);
        let after = controls(&marked);
        let glyphs: Vec<Rect> = marked
            .query_all_by_label("warning: too fast")
            .map(|node| node.rect())
            .collect();
        assert_eq!(glyphs.len(), before.len(), "one glyph a control");
        let left = before[0].1.left() - row::LABEL_PX - row::GAP_PX;
        let right = left + WIDTH_PX;
        for (((name, was), (_, is)), glyph) in before.iter().zip(&after).zip(&glyphs) {
            assert!(
                (was.top() - is.top()).abs() < 0.01 && (was.height() - is.height()).abs() < 0.01,
                "{name} moved or grew: {was:?} to {is:?}"
            );
            assert!(
                is.y_range().contains(glyph.center().y),
                "{name}: the glyph {glyph:?} is on its line {is:?}"
            );
            let centre = glyph.center().x;
            let half = GLYPH_PX / 2.0;
            match *name {
                // The field's node is its text: the glyph follows it,
                // before the unit at the frame's right end.
                "Depth" => assert!(
                    centre - half >= is.right() - 1.0 && centre + half <= right - 28.0,
                    "{name}: {glyph:?} after the text {is:?}, before the unit"
                ),
                "Side" => assert!(
                    is.contains(glyph.center()) && centre + half <= is.right() - 20.0,
                    "{name}: {glyph:?} inside {is:?}, clear of the chevron"
                ),
                _ => assert!(
                    centre - half >= is.right() && centre + half <= right + 0.5,
                    "{name}: {glyph:?} beside {is:?}, inside the row ending at {right}"
                ),
            }
        }
        // The pill control gave the glyph its room, and the field its
        // text's; nothing else changed width.
        for ((name, was), (_, is)) in before.iter().zip(&after) {
            let narrower = was.width() - is.width();
            match *name {
                "Direction" => assert!(
                    (narrower - BESIDE_PX).abs() < 0.5,
                    "{name} is {narrower} px narrower"
                ),
                "Depth" => assert!(narrower > 0.0, "{name}'s text makes room"),
                _ => assert!(narrower.abs() < 0.5, "{name} is {narrower} px narrower"),
            }
        }
    }
}
