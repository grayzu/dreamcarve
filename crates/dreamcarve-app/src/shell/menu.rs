// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The menus (functional spec, capability 50: File, Edit, Arrange, View,
//! Project, Help; `redesign.md`, "The window"): quiet text buttons at the
//! left of the header ([`crate::shell::header`]), egui's own on every
//! platform (design open question 6), every shortcut shown in its row
//! (capability 54). The Arrange menu (capability 58a, mock-up 14) holds
//! the Arrange group's commands with their shortcuts, *Rotate…* (the
//! group's angle popover) and *Centre on Stock* (61). The File, Edit,
//! View and Project menus are capability 79a's (mock-up 14): new-project
//! and add-to-project apart, *Export G-code…* in its own group, View ▸
//! *Go to* each of the five steps, and View ▸ Appearance a choice of
//! three with a check. The rail's Appearance and Help buttons open the
//! same rows ([`appearance_items`], [`help_items`]).

use dc_app::steps::Step;
use dc_app::{StrKey, Strings};
use eframe::egui::{self, Button, Key, Modifiers, Ui};

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, shortcut_action, shortcut_text};
use crate::widgets::icon::Icon;

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

/// Draws the menus at the left of `ui`, the header's left part; where
/// the last one ends, px.
pub(crate) fn menus(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) -> f32 {
    egui::MenuBar::new()
        .ui(ui, |ui| {
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
                help_items(ui, state, out);
            })
            .response
            .rect
            .right()
        })
        .inner
}

/// Help's rows: the User Guide and About. The Help menu's, and the
/// rail's Help button's.
pub(crate) fn help_items(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    item(
        ui,
        strings,
        StrKey::AppMenuUserGuide,
        None,
        Action::UserGuide,
        out,
    );
    item(ui, strings, StrKey::AppMenuAbout, None, Action::About, out);
}

/// The Arrange menu (capability 58a; mock-up 14): Rotate 90° either way,
/// *Rotate…*, the two flips, and *Centre on Stock*, each disabled with
/// the Arrange group's reason when nothing can turn.
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
    item(
        ui,
        strings,
        StrKey::AppMenuNewFromPicture,
        None,
        Action::NewFromPicture,
        out,
    );
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
    item(
        ui,
        strings,
        StrKey::AppMenuImportPicture,
        None,
        Action::ImportPicture,
        out,
    );
    ui.separator();
    item(
        ui,
        strings,
        StrKey::AppMenuExport,
        keyboard::binding(Shortcut::Export),
        Action::GoTo(Step::Export),
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
    let mut panel = state.prefs.panels.left_open;
    if ui
        .checkbox(&mut panel, strings.get(StrKey::AppMenuStepPanel))
        .clicked()
    {
        out.push(Action::ToggleStepPanel);
        ui.close();
    }
    ui.separator();
    // The five steps, each with its key (capability 54).
    for command in [
        Shortcut::GoToDesign,
        Shortcut::GoToStock,
        Shortcut::GoToTools,
        Shortcut::GoToOperations,
        Shortcut::GoToExport,
    ] {
        if let (Some((_, label)), Some(action)) = (
            keyboard::of(command).and_then(|s| s.menu),
            shortcut_action(state.app, command),
        ) {
            item(ui, strings, label, keyboard::binding(command), action, out);
        }
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
        appearance_items(ui, state, out);
    });
}

/// The name of appearance `theme`.
pub(crate) const fn theme_label(theme: dc_app::Theme) -> StrKey {
    match theme {
        dc_app::Theme::System => StrKey::AppMenuThemeSystem,
        dc_app::Theme::Light => StrKey::AppMenuThemeLight,
        dc_app::Theme::Dark => StrKey::AppMenuThemeDark,
    }
}

/// Appearance's three choices, the current one checked: View ▸
/// Appearance's rows, and the rail's Appearance button's.
pub(crate) fn appearance_items(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    for theme in [
        dc_app::Theme::System,
        dc_app::Theme::Light,
        dc_app::Theme::Dark,
    ] {
        let label = strings.get(theme_label(theme));
        if checked_item(ui, label, state.prefs.theme == theme).clicked() {
            out.push(Action::SetTheme(theme));
            ui.close();
        }
    }
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
