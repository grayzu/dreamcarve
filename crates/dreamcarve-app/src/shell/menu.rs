// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The menu bar (functional spec, capability 50: File, Edit, Arrange,
//! View, Project, Help; `experience.md`, section 2, "Menu bar and
//! toolbar"): egui's own on every platform (design open question 6),
//! 28 px, every shortcut shown in its row (capability 54). The Arrange
//! menu (capability 58a, mock-up 14) holds the toolbar's Arrange commands
//! with their shortcuts, *Rotate…* (the toolbar's angle popover) and
//! *Centre on Stock* (61). The File, Edit, View and Project menus are
//! capability 79a's (mock-up 14): new-project and add-to-project apart,
//! *Export G-code…* in its own group, and View ▸ Appearance a choice of
//! three with a check.

use dc_app::{StrKey, Strings};
use eframe::egui::{self, Button, Key, Modifiers, Ui};

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, shortcut_action, shortcut_text};
use crate::widgets::icon::Icon;

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
            if state.inert() {
                ui.disable();
            }
            egui::MenuBar::new().ui(ui, |ui| {
                let strings = state.app.strings();
                ui.menu_button(strings.get(StrKey::AppMenuFile), |ui| file(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuEdit), |ui| edit(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuArrange), |ui| {
                    arrange(ui, state, out);
                });
                ui.menu_button(strings.get(StrKey::AppMenuView), |ui| view(ui, state, out));
                ui.menu_button(strings.get(StrKey::AppMenuProject), |ui| {
                    item(
                        ui,
                        strings,
                        StrKey::AppMenuProjectSettings,
                        None,
                        Action::ShowProject,
                        out,
                    );
                    item(
                        ui,
                        strings,
                        StrKey::AppMenuToolLibrary,
                        None,
                        Action::OpenLibrary,
                        out,
                    );
                    placement_items(ui, state, out);
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

/// The Arrange menu (capability 58a; mock-up 14): Rotate 90° either way,
/// *Rotate…*, the two flips, and *Centre on Stock*, each disabled with
/// the toolbar group's reason when nothing can turn.
fn arrange(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    use dc_app::arrange::Action as Arrange;
    let strings = state.app.strings();
    let disabled =
        dc_app::arrange::disabled(state.app.project(), state.app.selection().instances(), 0);
    let mut row = |ui: &mut Ui, key: StrKey, shortcut: Option<Shortcut>, action: Action| {
        let mut button = Button::new(strings.get(key));
        if let Some((modifiers, key)) = shortcut.and_then(keyboard::binding) {
            button = button.shortcut_text(shortcut_text(ui.ctx(), modifiers, key));
        }
        let response = ui.add_enabled(disabled.is_none(), button);
        let response = match disabled {
            Some(why) => response.on_disabled_hover_text(strings.get(why.reason())),
            None => response,
        };
        if response.clicked() {
            out.push(action);
            ui.close();
        }
    };
    row(
        ui,
        StrKey::ArrangeMenuRotateClockwise,
        Some(Shortcut::RotateClockwise),
        Action::Arrange(Arrange::RotateClockwise),
    );
    row(
        ui,
        StrKey::ArrangeMenuRotateAnticlockwise,
        Some(Shortcut::RotateAnticlockwise),
        Action::Arrange(Arrange::RotateAnticlockwise),
    );
    row(ui, StrKey::ArrangeMenuRotate, None, Action::OpenRotate);
    ui.separator();
    row(
        ui,
        StrKey::ArrangeMenuFlipHorizontal,
        Some(Shortcut::FlipHorizontal),
        Action::Arrange(Arrange::FlipHorizontal),
    );
    row(
        ui,
        StrKey::ArrangeMenuFlipVertical,
        Some(Shortcut::FlipVertical),
        Action::Arrange(Arrange::FlipVertical),
    );
    ui.separator();
    row(
        ui,
        StrKey::ArrangeMenuCentreOnStock,
        None,
        Action::Arrange(Arrange::CentreOnStock),
    );
}

/// The Project menu's *Fit Stock to Shapes…* (functional spec, capability
/// 61): the submenu takes the margin in place (capability 50a: no
/// dialog), starting at `init`'s; the margin is not a project field, so it
/// is kept in egui's memory.
fn placement_items(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    ui.menu_button(strings.get(StrKey::AppMenuFitStock), |ui| {
        let units = state.app.project().display_units;
        let memory = egui::Id::unique("fit_stock_margin");
        let margin: String = ui.data(|d| d.get_temp(memory)).unwrap_or_else(|| {
            let decimals = match units {
                dc_project::Units::Mm => 0,
                dc_project::Units::Inch => 3,
            };
            dc_app::units::format(dc_job::project_from::STOCK_MARGIN_MM, units, decimals)
        });
        let mut typed = margin.clone();
        ui.horizontal(|ui| {
            ui.label(strings.get(StrKey::PlacementMargin));
            ui.add(egui::TextEdit::singleline(&mut typed).desired_width(80.0));
        });
        if typed != margin {
            ui.data_mut(|d| d.insert_temp(memory, typed.clone()));
        }
        if ui.button(strings.get(StrKey::PlacementFit)).clicked() {
            match dc_app::units::parse_length_mm(&typed, units) {
                Ok(margin_mm) => out.push(Action::FitStock { margin_mm }),
                Err(e) => out.push(Action::Refused(e.to_string())),
            }
            ui.close();
        }
    });
}

fn file(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    item(
        ui,
        strings,
        StrKey::AppMenuNew,
        keyboard::binding(Shortcut::New),
        Action::New,
        out,
    );
    item(
        ui,
        strings,
        StrKey::AppMenuNewFromDrawing,
        None,
        Action::NewFromDrawing,
        out,
    );
    ui.add_enabled(
        false,
        Button::new(strings.get(StrKey::AppMenuNewFromPicture)),
    )
    .on_disabled_hover_text(strings.get(StrKey::AppEmptyPictureUnavailable));
    item(
        ui,
        strings,
        StrKey::AppMenuOpen,
        keyboard::binding(Shortcut::Open),
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
        keyboard::binding(Shortcut::Save),
        Action::Save,
        out,
    );
    item(
        ui,
        strings,
        StrKey::AppMenuSaveAs,
        keyboard::binding(Shortcut::SaveAs),
        Action::SaveAs,
        out,
    );
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuImportDrawing,
        keyboard::binding(Shortcut::Import),
        Action::ImportDrawing,
        out,
    );
    ui.add_enabled(
        false,
        Button::new(strings.get(StrKey::AppMenuImportPicture)),
    )
    .on_disabled_hover_text(strings.get(StrKey::AppEmptyPictureUnavailable));
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuExport,
        keyboard::binding(Shortcut::Export),
        Action::OpenExport,
        out,
    );
    // Capability 77b: once this project has been exported.
    if state.setup_sheet {
        item(
            ui,
            strings,
            StrKey::AppMenuSaveSetupSheet,
            None,
            Action::SaveSetupSheet,
            out,
        );
    }
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuQuit,
        keyboard::binding(Shortcut::Quit),
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
        keyboard::binding(Shortcut::Undo),
        state.app.can_undo(),
        Action::Undo,
        out,
    );
    item_text(
        ui,
        &redo,
        keyboard::binding(Shortcut::Redo),
        state.app.can_redo(),
        Action::Redo,
        out,
    );
    // Capability 54: the selection's commands, each a row with its keys,
    // running what the key does (`window::shortcut_action`).
    ui.separator();
    let app = state.app;
    let selected = app.selection().instances();
    let all_locked = !selected.is_empty()
        && app
            .project()
            .instances
            .iter()
            .filter(|i| selected.contains(&i.id))
            .all(|i| i.locked);
    // Capability 79a: Duplicate, Delete, Rename | Select All; then the
    // visibility and lock rows, which carry shortcuts (54).
    for (command, key) in [
        (Shortcut::Duplicate, StrKey::AppMenuDuplicate),
        (Shortcut::Delete, StrKey::AppMenuDelete),
        (Shortcut::Rename, StrKey::AppMenuRename),
        (Shortcut::SelectAll, StrKey::AppMenuSelectAll),
        (Shortcut::Hide, StrKey::AppMenuHide),
        (Shortcut::Show, StrKey::AppMenuShow),
        (
            Shortcut::Lock,
            if all_locked {
                StrKey::AppMenuUnlock
            } else {
                StrKey::AppMenuLock
            },
        ),
    ] {
        if matches!(command, Shortcut::SelectAll | Shortcut::Hide) {
            ui.separator();
        }
        let mut button = Button::new(strings.get(key));
        if let Some((modifiers, key)) = keyboard::binding(command) {
            button = button.shortcut_text(shortcut_text(ui.ctx(), modifiers, key));
        }
        let action = shortcut_action(app, command);
        if ui.add_enabled(action.is_some(), button).clicked()
            && let Some(action) = action
        {
            out.push(action);
            ui.close();
        }
    }
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
    // Capability 73: the toolpath view, as the status bar's button opens
    // it; there is nothing to show until a toolpath exists.
    let toolpaths = state
        .app
        .scene()
        .operations()
        .iter()
        .any(|op| !op.runs.is_empty());
    item_text(
        ui,
        strings.get(StrKey::AppMenuToolpaths),
        None,
        toolpaths,
        Action::OpenToolpaths,
        out,
    );
    // Capability 76: the drawer. Regenerate all is the status bar's alone
    // (capability 71).
    let mut drawer = state.warnings_open;
    if ui
        .checkbox(&mut drawer, strings.get(StrKey::AppWarningsTitle))
        .clicked()
    {
        out.push(Action::ToggleWarnings);
        ui.close();
    }
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppViewportFitView,
        keyboard::binding(Shortcut::FitView),
        Action::FitView,
        out,
    );
    item_text(
        ui,
        strings.get(StrKey::AppViewportZoomSelection),
        keyboard::binding(Shortcut::ZoomToSelection),
        !state.app.selection().instances().is_empty(),
        Action::ZoomToSelection,
        out,
    );
    ui.separator();
    // Capability 79a: a choice of three is a submenu with a check, not a
    // checkbox, since a checkbox cannot say "follow the system".
    ui.menu_button(strings.get(StrKey::AppMenuAppearance), |ui| {
        for (theme, key) in [
            (dc_app::Theme::System, StrKey::AppMenuThemeSystem),
            (dc_app::Theme::Light, StrKey::AppMenuThemeLight),
            (dc_app::Theme::Dark, StrKey::AppMenuThemeDark),
        ] {
            if checked_item(ui, strings.get(key), state.prefs.theme == theme).clicked() {
                out.push(Action::SetTheme(theme));
                ui.close();
            }
        }
    });
}

/// A menu row with a check in front when `checked`: one of a choice
/// (mock-up 14). Screen readers hear it as a toggled button.
fn checked_item(ui: &mut Ui, text: &str, checked: bool) -> egui::Response {
    let palette = Palette::current(ui);
    let colour = if checked {
        palette.text_primary
    } else {
        theme::NONE
    };
    let check = egui::RichText::new(Icon::Check.codepoint().to_string())
        .font(egui::FontId::new(
            theme::ICON_SMALL_PX,
            egui::FontFamily::Name(theme::ICONS.into()),
        ))
        .color(colour);
    let response = ui.add(Button::new((check, text)));
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), checked, text)
    });
    response
}
