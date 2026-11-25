// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What floats over the simulated stock (phase 6 functional spec,
//! capabilities 111, 113 and 116; `docs/specs/phase-6/design.md`,
//! "Visual design"): the views at the top left, Colour at the top right,
//! the read-out at the bottom left and the transport at the bottom
//! centre. They are the 2D Viewport's pills (`viewport::controls`), 36 px
//! tall and 12 px in from the card's edges, and ease to 60 % while the
//! stock is dragged as those do.

use dc_app::StrKey;
use dc_app::sim::{Colouring, SimPrefs, Speed, View3, clock, readout};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use super::{SimAct, SimView};
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::viewport::controls::{CONTROL_PX, PILL_PX, pill};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::segmented::segmented_sized;

/// How far a pill stands in from the card's edges, px.
const INSET_PX: f32 = theme::FLOAT_INSET_PX;
/// The gap between a pill's controls, px, as the 2D pills have it.
const GAP_PX: f32 = 2.0;
/// A view's button, px wide.
const VIEW_PX: f32 = 52.0;
/// Colour's three-way control, px wide.
const COLOUR_PX: f32 = 264.0;
/// The widest the transport is, px.
const TRANSPORT_MAX_PX: f32 = 620.0;
/// The scrubber's least width, px (design, "Visual design").
const SCRUBBER_MIN_PX: f32 = 240.0;
/// The two times, px wide: room for `1:03:10 / 1:14:20`.
const TIME_PX: f32 = 116.0;
/// Speed's button, px wide.
const SPEED_PX: f32 = 52.0;
/// The scrubber's track, px tall, and its knob's radius.
const TRACK_PX: f32 = 4.0;
const KNOB_PX: f32 = 6.0;
/// The read-out's height and its padding, px.
const READOUT_PX: f32 = 28.0;
const READOUT_PAD_PX: f32 = 12.0;

/// Where the controls go this frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct Place {
    /// The card.
    pub pane: Rect,
    /// The card's right edge, less what an Inspector lies over.
    pub right: f32,
    /// The card's bottom edge, less what the Warnings drawer lies over.
    pub bottom: f32,
    /// How much of themselves the controls show.
    pub opacity: f32,
}

/// The top-left pill: Home, Top, Front, Right and Fit.
fn views(ui: &mut Ui, place: Place, state: &FrameState<'_>, view: &mut SimView) {
    let strings = state.app.strings();
    let padding = (PILL_PX - CONTROL_PX) / 2.0;
    let width = 2.0 * padding + 4.0 * VIEW_PX + CONTROL_PX + 4.0 * GAP_PX;
    let rect = Rect::from_min_size(
        egui::pos2(place.pane.left() + INSET_PX, place.pane.top() + INSET_PX),
        egui::vec2(width, PILL_PX),
    );
    let current = view
        .camera
        .as_ref()
        .and_then(dc_app::sim::OrbitCamera::view);
    let mut asked = None;
    pill(ui, rect, "sim.views", place.opacity, |ui| {
        for (preset, label, help) in [
            (View3::Home, StrKey::SimViewHome, StrKey::SimViewHomeHelp),
            (View3::Top, StrKey::SimViewTop, StrKey::SimViewTopHelp),
            (View3::Front, StrKey::SimViewFront, StrKey::SimViewFrontHelp),
            (View3::Right, StrKey::SimViewRight, StrKey::SimViewRightHelp),
        ] {
            let spec = ButtonSpec::new(Kind::Quiet, strings.get(label))
                .width(VIEW_PX)
                .height(CONTROL_PX)
                .pressed(current == Some(preset))
                .tooltip(strings.get(help));
            if button(ui, spec).clicked() {
                asked = Some(SimAct::View(preset));
            }
        }
        let fit = strings.get(StrKey::AppViewportFit);
        let spec = ButtonSpec::new(Kind::Quiet, "")
            .icon(Icon::Maximize, theme::ICON_SMALL_PX)
            .width(CONTROL_PX)
            .height(CONTROL_PX)
            .named(fit)
            .tooltip(fit);
        if button(ui, spec).clicked() {
            asked = Some(SimAct::Fit);
        }
    });
    if let Some(act) = asked {
        view.act(act);
    }
}

/// The top-right pill: Colour's three choices (capability 116).
fn colour(ui: &mut Ui, place: Place, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let prefs = state.prefs.sim;
    let padding = (PILL_PX - CONTROL_PX) / 2.0;
    let width = (COLOUR_PX + 2.0 * padding).min(place.pane.width() - 2.0 * INSET_PX);
    let rect = Rect::from_min_size(
        egui::pos2(place.right - INSET_PX - width, place.pane.top() + INSET_PX),
        egui::vec2(width, PILL_PX),
    );
    pill(ui, rect, "sim.colour", place.opacity, |ui| {
        let options = [
            strings.get(StrKey::SimColourMaterial),
            strings.get(StrKey::SimColourOperation),
            strings.get(StrKey::SimColourDepth),
        ];
        let selected = Colouring::ALL
            .iter()
            .position(|c| *c == prefs.colouring)
            .unwrap_or(0);
        let size = egui::vec2(width - 2.0 * padding, CONTROL_PX);
        if let Some(chosen) =
            segmented_sized(ui, Id::unique("dc.sim.colour"), &options, selected, size)
            && let Some(colouring) = Colouring::ALL.get(chosen)
        {
            out.push(Action::SetSimPrefs(SimPrefs {
                colouring: *colouring,
                ..prefs
            }));
        }
    });
}

/// The scrubber: the whole job as a track, the time shown as its filled
/// part, an operation's end as a tick. A click or a drag asks for the
/// stock as it is at that moment.
fn scrubber(ui: &mut Ui, width: f32, state: &FrameState<'_>, view: &mut SimView) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let frame = view.frame_info();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, CONTROL_PX), Sense::click_and_drag());
    let total = frame.total_s.max(f64::MIN_POSITIVE);
    let name = strings.get(StrKey::SimTransportScrubber);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, name);
        info.value = Some(frame.time_s);
        info
    });
    let track = Rect::from_center_size(
        rect.center(),
        egui::vec2(rect.width() - 2.0 * KNOB_PX, TRACK_PX),
    );
    if (response.dragged() || response.clicked())
        && let Some(at) = response.interact_pointer_pos()
    {
        let part = f64::from(((at.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0));
        view.act(SimAct::Scrub(part * frame.total_s));
    }
    // Narrowed to place a mark on the screen.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a fraction of the track, px"
    )]
    let x_of = |t_s: f64| track.left() + track.width() * (t_s / total).clamp(0.0, 1.0) as f32;
    let painter = ui.painter();
    painter.rect_filled(track, TRACK_PX / 2.0, palette.border_default);
    let done = Rect::from_min_max(track.min, egui::pos2(x_of(frame.time_s), track.max.y));
    painter.rect_filled(done, TRACK_PX / 2.0, palette.accent);
    // An operation's end: a tick across the track.
    let ends = view.operation_ends();
    if let Some(shown) = view.shown.as_ref() {
        let timeline = shown.transport.playback().timeline();
        for end in ends.iter().filter_map(|run| timeline.run_span_s(*run)) {
            let x = x_of(end.1);
            if x < track.right() - 1.0 {
                painter.vline(
                    x,
                    (track.top() - 3.0)..=(track.bottom() + 3.0),
                    Stroke::new(1.0, palette.border_strong),
                );
            }
        }
    }
    let knob = egui::pos2(x_of(frame.time_s), track.center().y);
    painter.circle(
        knob,
        KNOB_PX,
        palette.surface_raised,
        Stroke::new(1.5, palette.accent),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// The bottom-centre pill: Previous, Play or Pause, Next, the scrubber,
/// the two times and Speed (capability 113).
fn transport(
    ui: &mut Ui,
    place: Place,
    state: &FrameState<'_>,
    view: &mut SimView,
    out: &mut Vec<Action>,
) {
    let has_job = view
        .shown
        .as_ref()
        .is_some_and(|shown| shown.built.is_some());
    if !has_job {
        return;
    }
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let frame = view.frame_info();
    let padding = (PILL_PX - CONTROL_PX) / 2.0;
    let room = place.right - place.pane.left() - 2.0 * INSET_PX;
    let fixed = 2.0 * padding + 3.0 * CONTROL_PX + TIME_PX + SPEED_PX + 6.0 * GAP_PX + 16.0;
    let width = room.clamp(0.0, TRANSPORT_MAX_PX);
    // A card too narrow for the scrubber keeps the buttons.
    let scrubber_px = width - fixed;
    let rect = Rect::from_min_size(
        egui::pos2(
            (place.pane.left() + place.right - width) / 2.0,
            place.bottom - INSET_PX - PILL_PX,
        ),
        egui::vec2(width, PILL_PX),
    );
    let mut asked = None;
    let speed = view.speed().unwrap_or_default();
    pill(ui, rect, "sim.transport", place.opacity, |ui| {
        let icon_button = |ui: &mut Ui, icon: Icon, name: StrKey| {
            let name = strings.get(name);
            let spec = ButtonSpec::new(Kind::Quiet, "")
                .icon(icon, theme::ICON_SMALL_PX)
                .width(CONTROL_PX)
                .height(CONTROL_PX)
                .named(name)
                .tooltip(name);
            button(ui, spec).clicked()
        };
        if icon_button(ui, Icon::SkipBack, StrKey::SimTransportPrevious) {
            asked = Some(SimAct::Previous);
        }
        let (icon, name) = if frame.playing {
            (Icon::Pause, StrKey::SimTransportPause)
        } else {
            (Icon::Play, StrKey::SimTransportPlay)
        };
        if icon_button(ui, icon, name) {
            asked = Some(SimAct::PlayPause);
        }
        if icon_button(ui, Icon::SkipForward, StrKey::SimTransportNext) {
            asked = Some(SimAct::Next);
        }
        ui.add_space(8.0);
        if scrubber_px >= SCRUBBER_MIN_PX / 2.0 {
            scrubber(ui, scrubber_px, state, view);
        }
        ui.add_space(8.0);
        // The two times, in tabular figures so they do not shift.
        let times = strings.fmt(
            StrKey::SimTransportTime,
            &[
                ("now", &clock(frame.time_s)),
                ("total", &clock(frame.total_s)),
            ],
        );
        let (cell, response) =
            ui.allocate_exact_size(egui::vec2(TIME_PX, CONTROL_PX), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &times));
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            &times,
            TextStyle::Small.resolve(ui.style()),
            palette.text_secondary,
        );
        let help = strings.get(StrKey::SimTransportSpeedHelp);
        let spec = ButtonSpec::new(Kind::Quiet, speed.label())
            .width(SPEED_PX)
            .height(CONTROL_PX)
            .named(strings.get(StrKey::SimTransportSpeed))
            .tooltip(help);
        let response = button(ui, spec);
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP_END)
            .show(|ui| {
                for choice in Speed::ALL {
                    let row = ui.add(egui::Button::new(choice.label()).selected(choice == speed));
                    if row.clicked() {
                        asked = Some(SimAct::Speed(choice));
                        ui.close();
                    }
                }
            });
    });
    match asked {
        // The speed is a preference too: the window keeps it.
        Some(act @ SimAct::Speed(_)) => out.push(Action::Sim(act)),
        Some(act) => view.act(act),
        None => {}
    }
}

/// The bottom-left pill: where the pointer is on the stock, and the
/// operation that cut there (capability 111).
fn read_out(ui: &mut Ui, place: Place, state: &FrameState<'_>, view: &SimView) {
    let Some(hit) = view.hover.as_ref() else {
        return;
    };
    let palette = Palette::current(ui);
    let built = view.shown.as_ref().and_then(|shown| shown.built.as_deref());
    let said = readout(state.app, built, hit);
    let galley = ui.painter().layout_no_wrap(
        said.clone(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_primary,
    );
    // Above the transport, which has the bottom edge.
    let bottom = place.bottom - INSET_PX - PILL_PX - 8.0;
    let rect = Rect::from_min_size(
        egui::pos2(place.pane.left() + INSET_PX, bottom - READOUT_PX),
        egui::vec2(galley.size().x + 2.0 * READOUT_PAD_PX, READOUT_PX),
    );
    let response = ui.interact(rect, Id::unique("dc.sim.readout"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &said));
    let mut painter = ui.painter_at(rect.expand(32.0));
    painter.set_opacity(place.opacity);
    let radius = theme::pill(rect.height());
    painter.add(theme::shadow(palette).as_shape(rect, radius));
    painter.rect(
        rect,
        radius,
        palette.surface_raised,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(
            rect.left() + READOUT_PAD_PX,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_primary,
    );
}

/// Draws the floating controls over the card.
pub(super) fn show(
    ui: &mut Ui,
    place: Place,
    state: &FrameState<'_>,
    view: &mut SimView,
    out: &mut Vec<Action>,
) {
    if view.shown.is_none() {
        return;
    }
    views(ui, place, state, view);
    colour(ui, place, state, out);
    read_out(ui, place, state, view);
    transport(ui, place, state, view, out);
}
