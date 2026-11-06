// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The unsaved-changes sheet (functional spec, capability 51 and
//! acceptance 88; `experience.md`, section 8, "Destructive actions" and
//! "Sheets"): 420 px on `surface.raised` over a scrim, the file's name,
//! *Discard* on the left and *Cancel* and *Save* on the right. Enter is
//! Save; Escape, or a click on the scrim, is Cancel. The one confirmation
//! in the app.

use dc_app::{StrKey, Strings};
use eframe::egui::{self, Align, Key, Layout, Modifiers, TextStyle};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Answer};
use crate::widgets::button::{ButtonSpec, Kind, button};

/// The sheet's width, px.
const WIDTH_PX: f32 = 420.0;
/// Its padding, px.
const PADDING_PX: i8 = 24;
/// A footer button's height, px.
const FOOTER_PX: f32 = 28.0;

/// Shows the sheet asking about `file`; pushes the answer when given.
pub(crate) fn sheet(ctx: &egui::Context, strings: &Strings, file: &str, out: &mut Vec<Action>) {
    let style = ctx.global_style();
    let palette = if style.visuals.dark_mode {
        &theme::DARK
    } else {
        &theme::LIGHT
    };
    let frame = egui::Frame::popup(&style)
        .fill(palette.surface_raised)
        .stroke(egui::Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .inner_margin(PADDING_PX);
    let response = egui::Modal::new(egui::Id::unique("unsaved_changes"))
        .backdrop_color(palette.scrim)
        .frame(frame)
        .show(ctx, |ui| {
            let palette = Palette::current(ui);
            ui.set_width(WIDTH_PX - 2.0 * f32::from(PADDING_PX));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(strings.fmt(StrKey::AppUnsavedTitle, &[("name", file)]))
                        .text_style(TextStyle::Name(theme::DISPLAY.into()))
                        .color(palette.text_primary),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(strings.get(StrKey::AppUnsavedBody))
                    .color(palette.text_secondary),
            );
            ui.add_space(24.0);
            let mut answer = None;
            ui.horizontal(|ui| {
                let discard =
                    ButtonSpec::new(Kind::Destructive, strings.get(StrKey::AppUnsavedDiscard))
                        .height(FOOTER_PX);
                if button(ui, discard).clicked() {
                    answer = Some(Answer::Discard);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let save = ButtonSpec::new(Kind::Primary, strings.get(StrKey::AppUnsavedSave))
                        .height(FOOTER_PX);
                    if button(ui, save).clicked() {
                        answer = Some(Answer::Save);
                    }
                    let cancel =
                        ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppUnsavedCancel))
                            .height(FOOTER_PX);
                    if button(ui, cancel).clicked() {
                        answer = Some(Answer::Cancel);
                    }
                });
            });
            answer
        });
    let answer = response.inner.or_else(|| {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            Some(Answer::Save)
        } else if response.should_close() {
            Some(Answer::Cancel)
        } else {
            None
        }
    });
    if let Some(answer) = answer {
        out.push(Action::Answer(answer));
    }
}
