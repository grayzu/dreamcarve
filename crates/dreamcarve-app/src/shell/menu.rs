// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The menu bar (functional spec, capability 50: File, Edit, View,
//! Project, Help; `experience.md`, section 2, "Menu bar and toolbar"):
//! egui's own on every platform (design open question 6), 28 px, every
//! shortcut shown in its row (capability 54).

use dc_app::{StrKey, Strings};
use eframe::egui::{self, Button, Key, Modifiers, Ui};

use crate::shell::files;
use crate::shell::theme::Palette;
use crate::shell::window::{Action, Example, FrameState, shortcut_text};

/// The menu bar's height, px.
const HEIGHT_PX: f32 = 28.0;

/// A menu row reading `key` with `shortcut`, pushing `action` when chosen.
fn item(
    ui: &mut Ui,
    strings: &Strings,
    key: StrKey,
    shortcut: Option<(Modifiers, Key)>,
    action: Action,
    out: &mut Vec<Action>,
) {
    item_text(ui, strings.get(key), shortcut, true, action, out);
}

/// A menu row reading `text`.
fn item_text(
    ui: &mut Ui,
    text: &str,
    shortcut: Option<(Modifiers, Key)>,
    enabled: bool,
    action: Action,
    out: &mut Vec<Action>,
) {
    let mut button = Button::new(text);
    if let Some((modifiers, key)) = shortcut {
        button = button.shortcut_text(shortcut_text(ui.ctx(), modifiers, key));
    }
    if ui.add_enabled(enabled, button).clicked() {
        out.push(action);
        ui.close();
    }
}

/// A row for something a later slice brings, disabled with the reason.
fn later(ui: &mut Ui, strings: &Strings, key: StrKey, shortcut: Option<(Modifiers, Key)>) {
    let mut button = Button::new(strings.get(key));
    if let Some((modifiers, key)) = shortcut {
        button = button.shortcut_text(shortcut_text(ui.ctx(), modifiers, key));
    }
    ui.add_enabled(false, button)
        .on_disabled_hover_text(strings.get(StrKey::AppToolbarLater));
}

/// Draws the menu bar.
pub(crate) fn bar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    egui::Panel::top("menu_bar")
        .exact_size(HEIGHT_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_panel)
                .inner_margin(egui::Margin::symmetric(4, 0)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let strings = state.app.strings();
                ui.menu_button(strings.get(StrKey::AppMenuFile), |ui| file(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuEdit), |ui| edit(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuView), |ui| view(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuProject), |ui| {
                    item(
                        ui,
                        strings,
                        StrKey::AppMenuFitStock,
                        None,
                        Action::FitStock,
                        out,
                    );
                });
                ui.menu_button(strings.get(StrKey::AppMenuHelp), |ui| {
                    item(
                        ui,
                        strings,
                        StrKey::AppMenuUserGuide,
                        None,
                        Action::UserGuide,
                        out,
                    );
                    item(ui, strings, StrKey::AppMenuAbout, None, Action::About, out);
                });
            });
        });
}

fn file(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let command = Modifiers::COMMAND;
    let shift_command = Modifiers::COMMAND | Modifiers::SHIFT;
    item(
        ui,
        strings,
        StrKey::AppMenuNew,
        Some((command, Key::N)),
        Action::New,
        out,
    );
    item(
        ui,
        strings,
        StrKey::AppMenuOpen,
        Some((command, Key::O)),
        Action::Open,
        out,
    );
    ui.menu_button(strings.get(StrKey::AppMenuOpenRecent), |ui| {
        let listed: Vec<_> = state.prefs.recent.iter().filter(|p| p.is_file()).collect();
        if files::recent_has_missing(state.prefs) {
            out.push(Action::PruneRecent);
        }
        if listed.is_empty() {
            ui.add_enabled(false, Button::new(strings.get(StrKey::AppMenuNoRecent)));
        }
        for path in listed {
            let response = ui
                .add(Button::new(files::file_name(path)))
                .on_hover_text(path.display().to_string());
            if response.clicked() {
                out.push(Action::OpenRecent(path.clone()));
                ui.close();
            }
        }
    });
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuSave,
        Some((command, Key::S)),
        Action::Save,
        out,
    );
    item(
        ui,
        strings,
        StrKey::AppMenuSaveAs,
        Some((shift_command, Key::S)),
        Action::SaveAs,
        out,
    );
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuImportDrawing,
        Some((command, Key::I)),
        Action::ImportDrawing,
        out,
    );
    ui.add_enabled(
        false,
        Button::new(strings.get(StrKey::AppMenuImportPicture)),
    )
    .on_disabled_hover_text(strings.get(StrKey::AppEmptyPictureUnavailable));
    later(ui, strings, StrKey::AppMenuExport, Some((command, Key::E)));
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuQuit,
        Some((command, Key::Q)),
        Action::Quit,
        out,
    );
}

fn edit(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let undo = state.app.undo_label().map_or_else(
        || strings.get(StrKey::AppMenuUndo).to_owned(),
        |label| strings.fmt(StrKey::AppMenuUndoNamed, &[("label", label)]),
    );
    let redo = state.app.redo_label().map_or_else(
        || strings.get(StrKey::AppMenuRedo).to_owned(),
        |label| strings.fmt(StrKey::AppMenuRedoNamed, &[("label", label)]),
    );
    item_text(
        ui,
        &undo,
        Some((Modifiers::COMMAND, Key::Z)),
        state.app.can_undo(),
        Action::Undo,
        out,
    );
    item_text(
        ui,
        &redo,
        Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
        state.app.can_redo(),
        Action::Redo,
        out,
    );
}

fn view(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let mut left = state.prefs.panels.left_open;
    if ui
        .checkbox(&mut left, strings.get(StrKey::AppMenuLeftPanel))
        .clicked()
    {
        out.push(Action::ToggleLeft);
        ui.close();
    }
    let mut right = state.prefs.panels.right_open;
    if ui
        .checkbox(&mut right, strings.get(StrKey::AppMenuRightPanel))
        .clicked()
    {
        out.push(Action::ToggleRight);
        ui.close();
    }
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppViewportFitView,
        Some((Modifiers::NONE, Key::F)),
        Action::FitView,
        out,
    );
    item_text(
        ui,
        strings.get(StrKey::AppViewportZoomSelection),
        Some((Modifiers::SHIFT, Key::F)),
        !state.app.selection().instances().is_empty(),
        Action::ZoomToSelection,
        out,
    );
    ui.separator();
    ui.menu_button(strings.get(StrKey::AppMenuAppearance), |ui| {
        for (theme, key) in [
            (dc_app::Theme::System, StrKey::AppMenuThemeSystem),
            (dc_app::Theme::Light, StrKey::AppMenuThemeLight),
            (dc_app::Theme::Dark, StrKey::AppMenuThemeDark),
        ] {
            let chosen = state.prefs.theme == theme;
            if ui
                .add(Button::new(strings.get(key)).selected(chosen))
                .clicked()
            {
                out.push(Action::SetTheme(theme));
                ui.close();
            }
        }
    });
}

/// The examples, for the empty state.
pub(crate) const EXAMPLES: [(Example, StrKey); 3] = [
    (Example::Coaster, StrKey::AppEmptyCoaster),
    (Example::Plaque, StrKey::AppEmptyPlaque),
    (Example::Sign, StrKey::AppEmptySign),
];
