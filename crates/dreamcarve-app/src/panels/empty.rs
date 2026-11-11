// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The empty state (functional spec, capability 79; mock-up
//! `12-empty-state.svg`; `experience.md`, section 8, "Empty states"): a
//! 480-px raised card centred on the Viewport while the project has no
//! shapes, with Import a drawing, Import a picture and Open a project as
//! 40-px rows, the three quick starts' examples, and a close button. The
//! card goes when a shape arrives or on its close button.
//!
//! *Import a picture* waits for phase 3.5's picture import and says so;
//! *Import a drawing* opens the Import drawing sheet on the chosen file.

use dc_app::StrKey;
use eframe::egui::{self, Key, Modifiers, Rect, TextStyle, Ui};

use crate::shell::menu::EXAMPLES;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, shortcut_text};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;

/// The card's width, px.
const WIDTH_PX: f32 = 480.0;
/// Its padding, px.
const PADDING_PX: f32 = 24.0;
/// The height the card takes, px, to centre it before it is laid out.
const HEIGHT_PX: f32 = 380.0;
/// A row's height, px.
const ROW_PX: f32 = 40.0;

/// Draws the card centred on `pane`.
pub(crate) fn card(ui: &mut Ui, pane: Rect, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let size = egui::vec2(WIDTH_PX.min(pane.width() - 16.0), HEIGHT_PX);
    let rect = Rect::from_center_size(pane.center(), size).intersect(pane);
    let mut card_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut card_ui;
    egui::Frame::NONE
        .fill(palette.surface_raised)
        .stroke(egui::Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .shadow(ui.visuals().window_shadow)
        .inner_margin(PADDING_PX)
        .show(ui, |ui| {
            ui.set_width(rect.width() - 2.0 * PADDING_PX);
            let width = ui.available_width();
            ui.horizontal(|ui| {
                text(
                    ui,
                    strings.get(StrKey::AppEmptyHeading),
                    TextStyle::Name(theme::DISPLAY.into()),
                    palette.text_primary,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let close = ButtonSpec::new(Kind::Quiet, "")
                        .icon(Icon::X, theme::ICON_SMALL_PX)
                        .tooltip(strings.get(StrKey::AppEmptyClose));
                    if button(ui, close).clicked() {
                        out.push(Action::DismissEmpty);
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(strings.get(StrKey::AppEmptyIntro))
                        .color(palette.text_secondary),
                )
                .wrap()
                .selectable(false),
            );
            ui.add_space(16.0);
            let command = Modifiers::COMMAND;
            let import_hint = strings.fmt(
                StrKey::AppEmptyImportDrawingHint,
                &[("shortcut", &shortcut_text(ui.ctx(), command, Key::I))],
            );
            let row = |label: StrKey, icon: Icon| {
                ButtonSpec::new(Kind::Secondary, strings.get(label))
                    .icon(icon, theme::ICON_LARGE_PX)
                    .height(ROW_PX)
                    .width(width)
            };
            if button(
                ui,
                row(StrKey::AppEmptyImportDrawing, Icon::FileInput).hint(&import_hint),
            )
            .clicked()
            {
                out.push(Action::ImportDrawing);
            }
            button(
                ui,
                row(StrKey::AppEmptyImportPicture, Icon::ImagePlus)
                    .hint(strings.get(StrKey::AppEmptyImportPictureHint))
                    .tooltip(strings.get(StrKey::AppEmptyPictureUnavailable))
                    .enabled(false),
            );
            let open_hint = strings.fmt(
                StrKey::AppEmptyOpenProjectHint,
                &[("shortcut", &shortcut_text(ui.ctx(), command, Key::O))],
            );
            if button(
                ui,
                row(StrKey::AppEmptyOpenProject, Icon::FolderOpen).hint(&open_hint),
            )
            .clicked()
            {
                out.push(Action::Open);
            }
            ui.add_space(16.0);
            text(
                ui,
                strings.get(StrKey::AppEmptyExamples),
                TextStyle::Name(theme::TITLE.into()),
                palette.text_primary,
            );
            ui.horizontal(|ui| {
                for (example, key) in EXAMPLES {
                    let spec = ButtonSpec::new(Kind::Quiet, strings.get(key)).width(96.0);
                    if button(ui, spec).clicked() {
                        out.push(Action::OpenExample(example));
                    }
                }
            });
            text(
                ui,
                strings.get(StrKey::AppEmptyExamplesNote),
                TextStyle::Small,
                palette.text_secondary,
            );
        });
}
