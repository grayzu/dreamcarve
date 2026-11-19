// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Arrange group (functional spec, capability 58a; slice 25, #617;
//! `redesign.md`, "The steps"): in the header of the Design step's
//! Inspector, beside the Properties of the selection it acts on.
//! Icon-only: Rotate as a split button (the button turns the selection
//! 90° clockwise; its chevron opens a 240-px popover with an *Angle*
//! field, positive anticlockwise, and a *Rotate* button, Enter applying
//! it), then Flip horizontal and Flip vertical. Disabled, with the reason
//! in the tooltip, when nothing can turn ([`dc_app::arrange::disabled`]).

use dc_app::StrKey;
use dc_app::arrange::{self, Action as Arrange};
use eframe::egui::{self, Id, Stroke, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::unit_field::{UnitField, field as unit_field, row};

/// A button of the group, px square.
const BUTTON_PX: f32 = 28.0;
/// The split button's chevron segment, px wide.
const CHEVRON_PX: f32 = 16.0;
/// The gap between the group's buttons, px.
const GAP_PX: f32 = 2.0;
/// The group's width, px: Rotate, its rule and chevron, and the two
/// flips.
pub(crate) const WIDTH_PX: f32 = 3.0 * BUTTON_PX + 1.0 + CHEVRON_PX + 2.0 * GAP_PX;
/// The Rotate popover's width, px.
const POPOVER_PX: f32 = 240.0;
/// Its inner margin, px.
const POPOVER_MARGIN_PX: i8 = 12;

/// The Rotate popover's id, which its chevron toggles and Arrange ▸
/// Rotate… opens.
fn rotate_popover_id() -> Id {
    Id::unique("arrange.rotate")
}

/// Where the popover keeps the angle last typed.
fn angle_id() -> Id {
    Id::unique("arrange.angle")
}

/// Opens the Rotate popover (Arrange ▸ Rotate…).
pub(crate) fn open_rotate(ctx: &egui::Context) {
    egui::Popup::open_id(ctx, rotate_popover_id());
}

/// The Arrange group: the Rotate split button with its popover, then the
/// two flips, laid out left to right in `ui`.
pub(crate) fn group(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let palette = Palette::current(ui);
    let selected = state.app.selection().instances();
    let disabled = arrange::disabled(state.app.project(), selected, 0);
    let reason = disabled.map(|why| strings.get(why.reason()));
    let enabled = disabled.is_none();
    let icon_button = |icon: Icon, px: f32, width: f32, key: StrKey| {
        let name = strings.get(key);
        ButtonSpec::new(Kind::Quiet, "")
            .icon(icon, px)
            .height(BUTTON_PX)
            .width(width)
            .named(name)
            .tooltip(reason.unwrap_or(name))
            .enabled(enabled)
    };
    let popover_open = egui::Popup::is_id_open(ui.ctx(), rotate_popover_id());
    let spacing = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;
    let rotate = icon_button(
        Icon::RotateCwSquare,
        theme::ICON_LARGE_PX,
        BUTTON_PX,
        StrKey::ArrangeRotateClockwise,
    );
    if button(ui, rotate).clicked() {
        out.push(Action::Arrange(Arrange::RotateClockwise));
    }
    let (rule, _) = ui.allocate_exact_size(egui::vec2(1.0, BUTTON_PX), egui::Sense::hover());
    ui.painter().vline(
        rule.center().x,
        rule.shrink2(egui::vec2(0.0, 5.0)).y_range(),
        Stroke::new(1.0, palette.border_default),
    );
    let chevron = icon_button(
        Icon::ChevronDown,
        theme::ICON_SMALL_PX,
        CHEVRON_PX,
        StrKey::ArrangeRotateAngle,
    )
    .pressed(popover_open);
    let chevron = button(ui, chevron);
    rotate_popover(ui, &chevron, state, out);
    for (icon, key, action) in [
        (
            Icon::FlipHorizontal,
            StrKey::ArrangeFlipHorizontal,
            Arrange::FlipHorizontal,
        ),
        (
            Icon::FlipVertical,
            StrKey::ArrangeFlipVertical,
            Arrange::FlipVertical,
        ),
    ] {
        ui.add_space(GAP_PX);
        let spec = icon_button(icon, theme::ICON_LARGE_PX, BUTTON_PX, key);
        if button(ui, spec).clicked() {
            out.push(Action::Arrange(action));
        }
    }
    ui.spacing_mut().item_spacing.x = spacing;
}

/// The Rotate popover under `chevron`: the *Angle* field and *Rotate*.
/// Enter in the field, or the button, turns the selection by the angle
/// and closes it; a typed text that is not an angle goes to the status
/// bar.
fn rotate_popover(
    ui: &Ui,
    chevron: &egui::Response,
    state: &FrameState<'_>,
    out: &mut Vec<Action>,
) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let enabled =
        arrange::disabled(state.app.project(), state.app.selection().instances(), 0).is_none();
    let popup = egui::Popup::from_toggle_button_response(chevron)
        .id(rotate_popover_id())
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(POPOVER_PX)
        .frame(theme::popover(palette, POPOVER_MARGIN_PX));
    let mut apply = None;
    popup.show(|ui| {
        ui.set_width(POPOVER_PX - 2.0 * f32::from(POPOVER_MARGIN_PX));
        let name = strings.get(StrKey::ArrangeAngle);
        let typed: String = ui.data(|d| d.get_temp(angle_id())).unwrap_or_default();
        let (label, output) = row(ui, name, |ui| {
            unit_field(
                ui,
                UnitField {
                    enabled,
                    ..UnitField::new(angle_id().with("field"), name, &typed, "°")
                },
            )
        });
        let entered = output.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let _ = output.response.labelled_by(label.id);
        let _ = label.on_hover_text(strings.get(StrKey::ArrangeAngleHelp));
        let mut text = typed;
        if let Some(committed) = output.committed {
            ui.data_mut(|d| d.insert_temp(angle_id(), committed.clone()));
            text = committed;
        }
        ui.add_space(8.0);
        let rotate =
            ButtonSpec::new(Kind::Secondary, strings.get(StrKey::ArrangeRotate)).enabled(enabled);
        if (button(ui, rotate).clicked() || entered) && enabled {
            apply = Some(text);
        }
    });
    if let Some(text) = apply {
        match arrange::parse_angle(&text) {
            Ok(angle_deg) => {
                out.push(Action::Arrange(Arrange::Rotate { angle_deg }));
                egui::Popup::close_id(ui.ctx(), rotate_popover_id());
            }
            Err(e) => out.push(Action::Refused(e.to_string())),
        }
    }
}
