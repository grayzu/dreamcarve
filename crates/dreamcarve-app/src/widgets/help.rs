// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A field's help (`docs/specs/phase-4/experience.md`, section 7, "Help
//! affordance"; functional spec, capability 66): every field of the app
//! says what it does, its unit, the values it takes and what leaving it
//! empty means. The help shows in two places:
//!
//! - as a **tooltip**, after the usual delay, on the field's label (with
//!   the `.dcv` field name under it) and on the field itself;
//! - on the **help line** at the foot of the panel that holds the field,
//!   while the field has the keyboard: the step panel's, the Inspector's
//!   or a sheet's ([`with_foot`], [`foot`]). It is the one place the eye
//!   finds the help of what is being typed, without the pointer.
//!
//! A row's label offers its help ([`offer`], which `row::label` calls);
//! the control drawn after it under the same name takes it ([`take`] for
//! a text field, [`control`] for any other), so a panel names a field's
//! help once, at its label.

use eframe::egui::{self, Id, Rect, Response, Sense, Stroke, TextStyle, Ui, UiBuilder};

use crate::shell::focus::{self, Region};
use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// The help line's padding above and below its text, px.
const PAD_PX: f32 = 10.0;
/// The help line's side margin, px: the card's padding.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;

/// Which panel a help line is the foot of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The step panel.
    Step,
    /// The Inspector.
    Inspector,
    /// A sheet, drawn over the window.
    Sheet,
}

/// The help the last label offered, for the control under the same name.
#[derive(Debug, Clone)]
struct Offered {
    label: String,
    help: String,
}

/// The help of the control that has the keyboard, as its panel's foot
/// shows it.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    scope: Scope,
    label: String,
    help: String,
    frame_nr: u64,
}

fn offered_id() -> Id {
    Id::unique("dc.help.offered")
}

fn shown_id() -> Id {
    Id::unique("dc.help.shown")
}

/// The panel being drawn: the region the focus router is in, and a sheet
/// when it is in none (sheets draw after the regions).
fn scope(ui: &Ui) -> Scope {
    match focus::drawing(ui.ctx()) {
        Some(Region::Step) => Scope::Step,
        Some(Region::Inspector) => Scope::Inspector,
        _ => Scope::Sheet,
    }
}

/// A label named `label` offers `help` to the control drawn after it.
pub(crate) fn offer(ui: &Ui, label: &str, help: &str) {
    ui.data_mut(|d| {
        d.insert_temp(
            offered_id(),
            Offered {
                label: label.to_owned(),
                help: help.to_owned(),
            },
        );
    });
}

/// The help the label `name` offered, taken once: `None` when the last
/// label was another's, or offered none.
pub(crate) fn take(ui: &Ui, name: &str) -> Option<String> {
    let offered = ui.data(|d| d.get_temp::<Offered>(offered_id()))?;
    if offered.label != name || offered.help.is_empty() {
        return None;
    }
    ui.data_mut(|d| d.remove::<Offered>(offered_id()));
    Some(offered.help)
}

/// Says the control named `label` has the keyboard this frame: its
/// panel's foot shows `help`.
pub(crate) fn focused(ui: &Ui, label: &str, help: &str) {
    let shown = Shown {
        scope: scope(ui),
        label: label.to_owned(),
        help: help.to_owned(),
        frame_nr: ui.ctx().cumulative_frame_nr(),
    };
    // The foot is laid out before the fields draw, so a line that is new
    // shows in the next frame: ask for one.
    let was = ui.data(|d| d.get_temp::<Shown>(shown_id()));
    let new = was.is_none_or(|was| {
        was.label != shown.label || was.scope != shown.scope || was.frame_nr + 1 < shown.frame_nr
    });
    if new {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(shown_id(), shown));
}

/// Gives the control `response`, named `name`, the help its label
/// offered: its tooltip, and the foot's line while it has the keyboard.
/// A control whose label offered none is returned as it came.
pub(crate) fn control(ui: &Ui, response: Response, name: &str) -> Response {
    let Some(help) = take(ui, name) else {
        return response;
    };
    if response.has_focus() {
        focused(ui, name, &help);
    }
    response.on_hover_text(help)
}

/// The help to show at the foot of `scope`'s panel: that of the control
/// that had the keyboard in the last frame or this one.
fn shown(ui: &Ui, scope: Scope) -> Option<Shown> {
    let frame_nr = ui.ctx().cumulative_frame_nr();
    ui.data(|d| d.get_temp::<Shown>(shown_id()))
        .filter(|shown| shown.scope == scope && shown.frame_nr + 1 >= frame_nr)
}

/// Called once `scope`'s panel has drawn its fields: a line shown on the
/// word of the last frame that no field asked for again in this one is
/// taken away by the next frame, which this asks for.
pub(crate) fn settle(ui: &Ui, scope: Scope) {
    let frame_nr = ui.ctx().cumulative_frame_nr();
    let stale = ui
        .data(|d| d.get_temp::<Shown>(shown_id()))
        .is_some_and(|shown| shown.scope == scope && shown.frame_nr + 1 == frame_nr);
    if stale {
        ui.ctx().request_repaint();
    }
}

/// The line as laid out for a foot `width` px wide: the label at medium
/// weight, then the help in text.secondary, wrapped.
fn galley(ui: &Ui, shown: &Shown, width: f32) -> std::sync::Arc<egui::Galley> {
    let palette = Palette::current(ui);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (width - 2.0 * MARGIN_PX - theme::ICON_SMALL_PX - 8.0).max(40.0);
    job.append(
        &shown.label,
        0.0,
        egui::TextFormat {
            font_id: TextStyle::Name(theme::MEDIUM_STYLE.into()).resolve(ui.style()),
            color: palette.text_primary,
            ..egui::TextFormat::default()
        },
    );
    job.append(
        &shown.help,
        8.0,
        egui::TextFormat {
            font_id: TextStyle::Small.resolve(ui.style()),
            color: palette.text_secondary,
            ..egui::TextFormat::default()
        },
    );
    ui.painter().layout_job(job)
}

/// How tall `scope`'s help line is in a panel `width` px wide; 0 while no
/// field of the panel has the keyboard.
pub(crate) fn foot_height(ui: &Ui, scope: Scope, width: f32) -> f32 {
    shown(ui, scope).map_or(0.0, |shown| {
        galley(ui, &shown, width).size().y + 2.0 * PAD_PX
    })
}

/// Paints `scope`'s help line in `rect`, the foot [`foot_height`] gave
/// room for: a hairline over it, the `info` glyph, the field's label and
/// its help. One node for assistive tools, named with both.
pub(crate) fn foot(ui: &Ui, scope: Scope, rect: Rect) {
    let Some(shown) = shown(ui, scope) else {
        return;
    };
    let palette = Palette::current(ui);
    let galley = galley(ui, &shown, rect.width());
    ui.painter().hline(
        (rect.left() + MARGIN_PX)..=(rect.right() - MARGIN_PX),
        rect.top() + 0.5,
        Stroke::new(1.0, palette.border_default),
    );
    let glyph = Icon::Info.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
    let top = rect.top() + PAD_PX;
    ui.painter().galley(
        egui::pos2(rect.left() + MARGIN_PX, top),
        glyph,
        palette.text_secondary,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + MARGIN_PX + theme::ICON_SMALL_PX + 8.0, top),
        galley,
        palette.text_secondary,
    );
    let name = format!("{}: {}", shown.label, shown.help);
    ui.interact(
        rect,
        Id::unique(("dc.help.foot", scope as u8)),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
}

/// `body` in what `ui` has left, less the help line at its foot while a
/// field of `scope`'s panel has the keyboard. The line is pinned: the
/// panel's content scrolls above it.
pub(crate) fn with_foot<R>(ui: &mut Ui, scope: Scope, body: impl FnOnce(&mut Ui) -> R) -> R {
    let full = ui.available_rect_before_wrap();
    let foot_px = foot_height(ui, scope, full.width()).min(full.height() / 2.0);
    let above = Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - foot_px));
    // The body draws in a child of its own whether or not the line shows,
    // so a widget's id, and with it the focus, does not change as the
    // line comes and goes.
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("dc.help.body")
            .max_rect(above)
            .layout(*ui.layout()),
    );
    child.set_clip_rect(above.intersect(ui.clip_rect()));
    let inner = body(&mut child);
    settle(ui, scope);
    if foot_px > 0.0 {
        foot(
            ui,
            scope,
            Rect::from_min_max(egui::pos2(full.left(), above.bottom()), full.max),
        );
    }
    ui.allocate_rect(full, Sense::hover());
    inner
}
