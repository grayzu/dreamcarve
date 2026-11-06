// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The toolbar (functional spec, capability 50; `experience.md`, section
//! 2, "Menu bar and toolbar"): 40 px on `surface.panel`, quiet buttons with
//! a 20-px icon and a label in groups 16 px apart, the file name and the
//! unsaved dot right of centre, and Export as the one primary button at
//! the right end. What later slices bring (Picture, Fit, the Tab tool,
//! Export) is shown disabled with the reason.

use dc_app::StrKey;
use eframe::egui::{self, Align, Layout, Sense, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button, text};
use crate::widgets::icon::Icon;

/// The toolbar's height, px.
const HEIGHT_PX: f32 = 40.0;
/// A toolbar button's height, px.
const BUTTON_PX: f32 = 28.0;
/// The gap between groups, px.
const GROUP_GAP_PX: f32 = 16.0;

/// Draws the toolbar.
pub(crate) fn bar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    egui::Panel::top("toolbar")
        .exact_size(HEIGHT_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_panel)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .stroke(egui::Stroke::NONE),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let strings = state.app.strings();
                let later = strings.get(StrKey::AppToolbarLater);
                let quiet = |key: StrKey, icon: Icon| {
                    ButtonSpec::new(Kind::Quiet, strings.get(key))
                        .icon(icon, theme::ICON_LARGE_PX)
                        .height(BUTTON_PX)
                };
                if button(ui, quiet(StrKey::AppToolbarOpen, Icon::FolderOpen)).clicked() {
                    out.push(Action::Open);
                }
                if button(ui, quiet(StrKey::AppToolbarSave, Icon::Save)).clicked() {
                    out.push(Action::Save);
                }
                ui.add_space(GROUP_GAP_PX);
                let undo = quiet(StrKey::AppToolbarUndo, Icon::Undo).enabled(state.app.can_undo());
                let undo = match state.app.undo_label() {
                    Some(label) => undo.tooltip(label),
                    None => undo,
                };
                if button(ui, undo).clicked() {
                    out.push(Action::Undo);
                }
                let redo = quiet(StrKey::AppToolbarRedo, Icon::Redo).enabled(state.app.can_redo());
                let redo = match state.app.redo_label() {
                    Some(label) => redo.tooltip(label),
                    None => redo,
                };
                if button(ui, redo).clicked() {
                    out.push(Action::Redo);
                }
                ui.add_space(GROUP_GAP_PX);
                if button(ui, quiet(StrKey::AppToolbarImport, Icon::FileInput)).clicked() {
                    out.push(Action::ImportDrawing);
                }
                button(
                    ui,
                    quiet(StrKey::AppToolbarPicture, Icon::ImagePlus)
                        .enabled(false)
                        .tooltip(strings.get(StrKey::AppEmptyPictureUnavailable)),
                );
                ui.add_space(GROUP_GAP_PX);
                button(
                    ui,
                    quiet(StrKey::AppToolbarFit, Icon::Maximize)
                        .enabled(false)
                        .tooltip(later),
                );
                button(
                    ui,
                    quiet(StrKey::AppToolbarTabTool, Icon::BookmarkPlus)
                        .enabled(false)
                        .tooltip(later),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    button(
                        ui,
                        ButtonSpec::new(Kind::Primary, strings.get(StrKey::AppToolbarExport))
                            .icon(Icon::Download, theme::ICON_LARGE_PX)
                            .height(BUTTON_PX)
                            .enabled(false)
                            .tooltip(later),
                    );
                    ui.add_space(ui.available_width() * 0.35);
                    if state.app.is_dirty() {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 3.0, palette.text_secondary);
                        response.on_hover_text(strings.get(StrKey::AppToolbarUnsaved));
                    }
                    text(
                        ui,
                        &state.display_name,
                        TextStyle::Body,
                        palette.text_secondary,
                    );
                });
            });
        });
}
