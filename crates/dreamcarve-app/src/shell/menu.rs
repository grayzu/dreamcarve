// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The menus (functional spec, capability 50: File, Edit, Draw, Shape,
//! Arrange, View, Project, Help; `redesign.md`, "The window"): quiet text buttons at the
//! left of the header ([`crate::shell::header`]), egui's own on every
//! platform (design open question 6), every shortcut shown in its row
//! (capability 54). The Arrange menu (capability 58a, mock-up 14) holds
//! the Arrange group's commands with their shortcuts, *Rotate…* (the
//! group's angle popover) and *Centre on Stock* (61). The File, Edit,
//! View and Project menus are capability 79a's (mock-up 14): new-project
//! and add-to-project apart, *Export G-code…* in its own group, View ▸
//! *Go to* each of the five steps, and View ▸ Appearance a choice of
//! three with a check. The rail's Appearance and Help buttons open the
//! same rows ([`appearance_items`], [`help_items`]). Phase 5 adds
//! **Draw**, the drawing tools with their keys (capability 81), **Shape**
//! with *Convert to Path* (84), and under View the **Snapping** switch,
//! its kinds under **Snap To** (82) and the **Grid**'s spacing (82a).

use dc_app::steps::Step;
use dc_app::{StrKey, Strings};
use eframe::egui::{self, Button, Key, Modifiers, Ui};

use crate::keyboard::{self, Command as Shortcut};
use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, BookPage, FrameState, PasteAt, shortcut_action, shortcut_text};
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
            ui.menu_button(strings.get(StrKey::AppMenuDraw), |ui| draw(ui, state, out));
            ui.menu_button(strings.get(StrKey::AppMenuShape), |ui| {
                shape(ui, state, out)
            });
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

/// Help's rows (capability 79): the book's quick start for the app, its
/// Keyboard chapter and its front page, each opened in the browser, and
/// About. The Help menu's, and the rail's Help button's.
pub(crate) fn help_items(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    for (label, page) in [
        (StrKey::AppMenuQuickStart, BookPage::QuickStart),
        (StrKey::AppMenuKeyboardShortcuts, BookPage::Keyboard),
        (StrKey::AppMenuUserGuide, BookPage::Front),
    ] {
        item(ui, strings, label, None, Action::OpenBook(page), out);
    }
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
    let row = |ui: &mut Ui,
               out: &mut Vec<Action>,
               key: StrKey,
               shortcut: Option<Shortcut>,
               action: Action| {
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
        out,
        StrKey::ArrangeMenuRotateClockwise,
        Some(Shortcut::RotateClockwise),
        Action::Arrange(Arrange::RotateClockwise),
    );
    row(
        ui,
        out,
        StrKey::ArrangeMenuRotateAnticlockwise,
        Some(Shortcut::RotateAnticlockwise),
        Action::Arrange(Arrange::RotateAnticlockwise),
    );
    row(ui, out, StrKey::ArrangeMenuRotate, None, Action::OpenRotate);
    ui.separator();
    row(
        ui,
        out,
        StrKey::ArrangeMenuFlipHorizontal,
        Some(Shortcut::FlipHorizontal),
        Action::Arrange(Arrange::FlipHorizontal),
    );
    row(
        ui,
        out,
        StrKey::ArrangeMenuFlipVertical,
        Some(Shortcut::FlipVertical),
        Action::Arrange(Arrange::FlipVertical),
    );
    ui.separator();
    // Align and Distribute (phase 5, capability 98a). One shape aligns
    // to the stock, as do several while To Stock is ticked.
    use dc_app::arrange::{Align, Axis, Spacing};
    let count = state.app.selection().instances().len();
    let to_stock = state.app.prefs().adjust.align_to_stock;
    ui.menu_button(strings.get(StrKey::ArrangeMenuAlign), |ui| {
        for (key, side) in [
            (StrKey::ArrangeMenuAlignLeft, Align::Left),
            (StrKey::ArrangeMenuAlignCentre, Align::Centre),
            (StrKey::ArrangeMenuAlignRight, Align::Right),
            (StrKey::ArrangeMenuAlignTop, Align::Top),
            (StrKey::ArrangeMenuAlignMiddle, Align::Middle),
            (StrKey::ArrangeMenuAlignBottom, Align::Bottom),
        ] {
            if side == Align::Top {
                ui.separator();
            }
            row(
                ui,
                out,
                key,
                None,
                Action::Arrange(Arrange::Align { side, to_stock }),
            );
        }
        ui.separator();
        let stock = checked_item(
            ui,
            strings.get(StrKey::ArrangeMenuToStock),
            to_stock || count == 1,
        );
        if stock.clicked() {
            out.push(Action::SetAlignToStock(!to_stock));
            ui.close();
        }
    });
    ui.menu_button(strings.get(StrKey::ArrangeMenuDistribute), |ui| {
        for (key, axis, spacing) in [
            (
                StrKey::ArrangeMenuDistributeHorizontal,
                Axis::Horizontal,
                Spacing::Gaps,
            ),
            (
                StrKey::ArrangeMenuDistributeVertical,
                Axis::Vertical,
                Spacing::Gaps,
            ),
            (
                StrKey::ArrangeMenuDistributeHorizontalCentres,
                Axis::Horizontal,
                Spacing::Centres,
            ),
            (
                StrKey::ArrangeMenuDistributeVerticalCentres,
                Axis::Vertical,
                Spacing::Centres,
            ),
        ] {
            if (axis, spacing) == (Axis::Horizontal, Spacing::Centres) {
                ui.separator();
            }
            row(
                ui,
                out,
                key,
                None,
                Action::Arrange(Arrange::Distribute { axis, spacing }),
            );
        }
    });
    // Array… takes its values in the tool bar (capability 98).
    let array = Button::new(strings.get(StrKey::ArrangeMenuArray));
    if ui.add_enabled(count > 0, array).clicked() {
        out.push(Action::Arm(dc_app::Tool::Adjust(
            dc_app::tool::Adjust::Array,
        )));
        ui.close();
    }
    ui.separator();
    // Group and Ungroup (capability 99): a group made here is one whose
    // shapes a click on the Viewport selects together.
    for (command, key) in [
        (Shortcut::Group, StrKey::AppMenuGroup),
        (Shortcut::Ungroup, StrKey::AppMenuUngroup),
    ] {
        let mut button = Button::new(strings.get(key));
        if let Some((modifiers, key)) = keyboard::binding(command) {
            button = button.shortcut_text(shortcut_text(ui.ctx(), modifiers, key));
        }
        let action = shortcut_action(state.app, command);
        if ui.add_enabled(action.is_some(), button).clicked()
            && let Some(action) = action
        {
            out.push(action);
            ui.close();
        }
    }
    ui.separator();
    row(
        ui,
        out,
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
        (Shortcut::Cut, StrKey::AppMenuCut),
        (Shortcut::Copy, StrKey::AppMenuCopy),
        (Shortcut::Paste, StrKey::AppMenuPaste),
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
        // The pointer is on the menu, so the menu's Paste is not placed
        // by it.
        let action = if command == Shortcut::Paste {
            app.can_paste().then_some(Action::Paste(PasteAt::Offset))
        } else {
            shortcut_action(app, command)
        };
        if ui.add_enabled(action.is_some(), button).clicked()
            && let Some(action) = action
        {
            out.push(action);
            ui.close();
        }
    }
}

/// Draw (capability 81): the drawing tools this build has, each with its
/// key, the one in use checked. Arming one opens the Design step.
fn draw(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let armed = match app.tool() {
        dc_app::Tool::Draw(drawing) => Some(drawing),
        _ => None,
    };
    // Target editing has the drawing to itself.
    let enabled = app.target_editing().is_none();
    for drawing in dc_app::Drawing::ALL
        .into_iter()
        .filter(|drawing| drawing.is_ready())
    {
        let binding = keyboard::command_of(drawing).and_then(keyboard::binding);
        let row = checked_item_with(
            ui,
            strings.get(drawing.name()),
            armed == Some(drawing),
            binding,
            enabled,
        );
        if row.clicked() {
            out.push(Action::Arm(dc_app::Tool::Draw(drawing)));
            ui.close();
        }
    }
}

/// Shape (capabilities 84 and 93 to 97): *Convert to Path*; *Fillet
/// Corners…*, *Offset…* and *Join…*, which take their value in the tool
/// bar; and *Weld*, *Subtract* and *Intersect*, which act at once on two
/// or more selected shapes. A command that cannot act says why in the
/// status bar.
fn shape(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    use dc_app::edit::Boolean;
    use dc_app::tool::Adjust;
    let app = state.app;
    let strings = app.strings();
    let selected = app.selection().instances();
    let sized = selected
        .iter()
        .any(|id| dc_app::shape::section(app.project(), strings, id).is_some());
    item_text(
        ui,
        strings.get(StrKey::AppMenuConvertToPath),
        None,
        sized,
        Action::run(dc_app::Command::ConvertToPath(selected.to_vec())),
        out,
    );
    let items = dc_app::tree::menu(app.project(), selected, None, false);
    let linked = items.contains(&dc_app::tree::MenuItem::MakeUnique);
    // Construction (capability 100): ticked while every selected shape
    // is construction geometry.
    let marked = items.contains(&dc_app::tree::MenuItem::NotConstruction);
    let construction = checked_item_with(
        ui,
        strings.get(StrKey::AppMenuConstruction),
        marked,
        None,
        !selected.is_empty(),
    );
    if construction.clicked() {
        out.push(Action::run(dc_app::Command::SetConstruction {
            ids: selected.to_vec(),
            construction: !marked,
        }));
        ui.close();
    }
    ui.separator();
    item_text(
        ui,
        strings.get(StrKey::AppMenuMakeUnique),
        None,
        linked,
        Action::run(dc_app::Command::MakeUnique(selected.to_vec())),
        out,
    );
    item_text(
        ui,
        strings.get(StrKey::AppMenuFilletCorners),
        None,
        selected.len() == 1,
        Action::Arm(dc_app::Tool::Adjust(Adjust::FilletCorners)),
        out,
    );
    ui.separator();
    item_text(
        ui,
        strings.get(StrKey::AppMenuOffset),
        keyboard::binding(Shortcut::Offset),
        !selected.is_empty(),
        Action::Arm(dc_app::Tool::Adjust(Adjust::Offset)),
        out,
    );
    ui.separator();
    for (label, op) in [
        (StrKey::AppMenuWeld, Boolean::Weld),
        (StrKey::AppMenuSubtract, Boolean::Subtract),
        (StrKey::AppMenuIntersect, Boolean::Intersect),
    ] {
        let action = match dc_app::edit::boolean(app.project(), strings, selected, op) {
            Ok(command) => Action::run(command),
            Err(e) => Action::Refused(e.to_string()),
        };
        item_text(
            ui,
            strings.get(label),
            None,
            selected.len() > 1,
            action,
            out,
        );
    }
    ui.separator();
    item_text(
        ui,
        strings.get(StrKey::AppMenuJoin),
        keyboard::binding(Shortcut::Join),
        selected.len() > 1,
        Action::Arm(dc_app::Tool::Adjust(Adjust::Join)),
        out,
    );
    // The Tab tool, ticked while it is in hand: a click on an outline
    // adds a tab. It is here and on its key, not in the tool strip,
    // since tabs are mostly placed by a Profile's own request.
    ui.separator();
    let tab_armed = app.tool() == dc_app::Tool::Tab;
    let tab = checked_item_with(
        ui,
        strings.get(StrKey::AppMenuTabTool),
        tab_armed,
        keyboard::binding(Shortcut::TabTool),
        app.target_editing().is_none(),
    );
    if tab.clicked() {
        out.push(Action::Arm(if tab_armed {
            dc_app::Tool::Select
        } else {
            dc_app::Tool::Tab
        }));
        ui.close();
    }
}

/// View ▸ Snap To (capability 82): a tick per kind of snap. The master
/// switch is View ▸ Snapping, a row of the View menu with its key.
pub(crate) fn snap_to(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let snap = &state.prefs.snap;
    for kind in dc_app::SnapKind::ALL {
        let on = snap.kinds.contains(&kind);
        let name = strings.get(crate::viewport::snap_kind_name(kind));
        if checked_item_with(ui, name, on, None, snap.enabled).clicked() {
            let mut next = snap.clone();
            next.set(kind, !on);
            out.push(Action::SetSnap(next));
            ui.close();
        }
    }
}

/// View ▸ Grid (capability 82a): the grid shown or hidden, and its
/// spacing, *Automatic* or a typed length in the project's units.
fn grid(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let units = app.project().display_units;
    let visibility = app.view().visibility;
    if checked_item(ui, strings.get(StrKey::AppMenuGridShow), visibility.grid).clicked() {
        out.push(Action::SetVisibility(dc_app::Visibility {
            grid: !visibility.grid,
            ..visibility
        }));
        ui.close();
    }
    ui.separator();
    let pinned = state.prefs.grid_spacing_mm;
    if checked_item(
        ui,
        strings.get(StrKey::AppMenuGridAutomatic),
        pinned.is_none(),
    )
    .clicked()
    {
        out.push(Action::SetGridSpacing(None));
        ui.close();
    }
    // The spacing, typed: what is pinned, or the automatic one as the
    // placeholder.
    let value = pinned.map_or_else(String::new, |mm| dc_app::units::field_text(mm, units));
    let automatic = dc_app::units::field_text(app.grid_step_mm(), units);
    let label = strings.get(StrKey::AppMenuGridSpacing);
    let typed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.allocate_ui(egui::vec2(GRID_FIELD_PX, theme::CONTROL_PX), |ui| {
                ui.set_width(GRID_FIELD_PX);
                crate::widgets::unit_field::field(
                    ui,
                    crate::widgets::unit_field::UnitField {
                        placeholder: &automatic,
                        ..crate::widgets::unit_field::UnitField::new(
                            egui::Id::unique("dc.menu.grid.spacing"),
                            label,
                            &value,
                            dc_app::units::suffix(units),
                        )
                    },
                )
                .committed
            })
            .inner
        })
        .inner;
    if let Some(typed) = typed {
        // Emptied, the spacing is automatic again.
        if typed.trim().is_empty() {
            out.push(Action::SetGridSpacing(None));
        } else {
            match dc_app::units::parse_length_mm(&typed, units) {
                Ok(mm) if mm > 0.0 => out.push(Action::SetGridSpacing(Some(mm))),
                Ok(_) => out.push(Action::Refused(
                    strings.get(StrKey::DrawProblemNotPositive).to_owned(),
                )),
                Err(e) => out.push(Action::Refused(e.to_string())),
            }
        }
    }
}

/// The grid spacing field in View ▸ Grid, px wide.
const GRID_FIELD_PX: f32 = 96.0;

fn view(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    // A row that is on or off is ticked or not, as a choice's rows are
    // ([`checked_item`]): one look for a check in every menu.
    let panel = state.prefs.panels.left_open;
    if checked_item(ui, strings.get(StrKey::AppMenuStepPanel), panel).clicked() {
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
    let drawer = state.warnings_open;
    if checked_item(ui, strings.get(StrKey::AppWarningsTitle), drawer).clicked() {
        out.push(Action::ToggleWarnings);
        ui.close();
    }
    ui.separator();
    // Phase 5: snapping and the snap grid (capabilities 82 and 82a).
    let snapping = checked_item_with(
        ui,
        strings.get(StrKey::AppMenuSnap),
        state.prefs.snap.enabled,
        keyboard::binding(Shortcut::ToggleSnapping),
        true,
    );
    if snapping.clicked() {
        if let Some(action) = shortcut_action(state.app, Shortcut::ToggleSnapping) {
            out.push(action);
        }
        ui.close();
    }
    ui.menu_button(strings.get(StrKey::AppMenuSnapping), |ui| {
        snap_to(ui, state, out);
    });
    ui.menu_button(strings.get(StrKey::AppMenuGrid), |ui| grid(ui, state, out));
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
pub(crate) fn checked_item(ui: &mut Ui, text: &str, checked: bool) -> egui::Response {
    checked_item_with(ui, text, checked, None, true)
}

/// [`checked_item`] with its `shortcut` shown, and greyed when not
/// `enabled`.
fn checked_item_with(
    ui: &mut Ui,
    text: &str,
    checked: bool,
    shortcut: Option<(Modifiers, Key)>,
    enabled: bool,
) -> egui::Response {
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
    let mut button = Button::new((check, text));
    // The row's name ends with its keys, as a plain row's does.
    let mut name = text.to_owned();
    if let Some((modifiers, key)) = shortcut {
        let keys = shortcut_text(ui.ctx(), modifiers, key);
        name = format!("{text} {keys}");
        button = button.shortcut_text(keys);
    }
    let response = ui.add_enabled(enabled, button);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            enabled && ui.is_enabled(),
            checked,
            &name,
        )
    });
    response
}
