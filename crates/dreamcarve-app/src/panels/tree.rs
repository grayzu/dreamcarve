// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Design Tree (functional spec, capability 57; `experience.md`,
//! sections 6 to 8; mock-up `01-workspace.svg`, left): the Shapes
//! section, one 26-px row per instance in file order with its eye, its
//! padlock (shown on hover unless locked), its name, the `open` and
//! `link-2` badges and the count of operations targeting it; then the
//! Tabs section, a row per outline holding placed tabs; then Pictures.
//! Each header's eye and padlock act on every row in it.
//!
//! The rows come from `dc_app::tree`; a click, the context menu, an eye,
//! a padlock or a rename only push actions, so every change is a command
//! and one undo step. The selection is `dc_app`'s, the one the Viewport
//! and the Properties panel show: click selects, Shift-click selects the
//! range from the last click, Ctrl/⌘-click toggles. A double-click turns
//! the name into a field in place; Enter or leaving it commits, Escape
//! reverts. What is being renamed and where the range starts live in
//! egui's memory, not the project.

use dc_app::tree::{MenuItem, ShapeRow, TabRow, Tree};
use dc_app::{ClickMode, Selection, StrKey, Strings};
use dc_project::{InstanceId, Project};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::icon::Icon;

/// A tree row's height, px (`experience.md`, section 3).
const ROW_PX: f32 = 26.0;
/// A section header's height, px.
const SECTION_PX: f32 = 28.0;
/// The panel's content margin, px.
const MARGIN_PX: f32 = 12.0;
/// An icon's hit square, px.
const ICON_BOX_PX: f32 = 20.0;

/// Where the next Shift-click's range starts: the last row clicked.
fn anchor_id() -> Id {
    Id::unique("design_tree_anchor")
}

/// The instance whose name is a field, and the text typed so far.
fn renaming_id() -> Id {
    Id::unique("design_tree_renaming")
}

/// The selection the tree last drew, to tell when it changed.
fn seen_id() -> Id {
    Id::unique("design_tree_seen_selection")
}

/// The click mode of `modifiers` in the tree: Ctrl/⌘ toggles, Shift
/// extends (a range here), else replaces.
fn mode(modifiers: egui::Modifiers) -> ClickMode {
    if modifiers.command {
        ClickMode::Toggle
    } else if modifiers.shift {
        ClickMode::Add
    } else {
        ClickMode::Replace
    }
}

/// A 16-px glyph centred in `rect`.
fn glyph(ui: &Ui, rect: Rect, icon: Icon, colour: egui::Color32) {
    let galley = icon.galley(ui, theme::ICON_SMALL_PX, colour);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
}

/// A clickable glyph at `rect`, named `name` for assistive tools; whether
/// it was clicked.
fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    icon: Icon,
    colour: egui::Color32,
    name: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    let palette = Palette::current(ui);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4.0, palette.surface_hover);
    }
    glyph(ui, rect, icon, colour);
    if enabled {
        response.on_hover_text(name)
    } else {
        response.on_disabled_hover_text(name)
    }
}

/// `text` laid out in `style` and `colour`, cut with an ellipsis at
/// `width` (`experience.md`, section 12, "Text truncation").
fn truncated(
    ui: &Ui,
    text: &str,
    style: &TextStyle,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        style.resolve(ui.style()),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// A section header (`experience.md`, section 7, "Disclosure section
/// header"): the title, then its eye and padlock at the right end, each
/// acting on every row; `visible` and `locked` as `dc_app::tree` reads
/// the rows, `None` for mixed. Returns which was clicked.
fn section(
    ui: &mut Ui,
    strings: &Strings,
    title: &str,
    state: Option<dc_app::tree::SectionState>,
    disabled_reason: Option<&str>,
) -> (bool, bool) {
    let palette = Palette::current(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), SECTION_PX), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, title));
    ui.painter().text(
        egui::pos2(rect.left() + MARGIN_PX, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    let lock_rect = Rect::from_center_size(
        egui::pos2(
            rect.right() - MARGIN_PX - ICON_BOX_PX / 2.0,
            rect.center().y,
        ),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    let eye_rect = lock_rect.translate(egui::vec2(-ICON_BOX_PX - 4.0, 0.0));
    let enabled = state.is_some() && disabled_reason.is_none();
    let visible = state.and_then(|s| s.visible);
    let locked = state.and_then(|s| s.locked);
    let colour = if enabled {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    let (eye_icon, eye_name) = match visible {
        Some(true) => (Icon::Eye, strings.get(StrKey::TreeHideAll)),
        Some(false) => (Icon::EyeOff, strings.get(StrKey::TreeShowAll)),
        None => (Icon::Eye, strings.get(StrKey::TreeShowAll)),
    };
    let (lock_icon, lock_name) = match locked {
        Some(true) => (Icon::Lock, strings.get(StrKey::TreeUnlockAll)),
        _ => (Icon::LockOpen, strings.get(StrKey::TreeLockAll)),
    };
    let reason = disabled_reason.unwrap_or_default();
    let eye = icon_button(
        ui,
        eye_rect,
        ui.make_persistent_id((title, "eye")),
        eye_icon,
        colour,
        if enabled { eye_name } else { reason },
        enabled,
    );
    let lock = icon_button(
        ui,
        lock_rect,
        ui.make_persistent_id((title, "lock")),
        lock_icon,
        colour,
        if enabled { lock_name } else { reason },
        enabled,
    );
    (eye.clicked(), lock.clicked())
}

/// Every instance id of `project`.
fn every_instance(project: &Project) -> Vec<InstanceId> {
    project.instances.iter().map(|i| i.id.clone()).collect()
}

/// Draws the Design Tree under its header.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let tree = Tree::build(project);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let (eye, lock) = section(
                ui,
                strings,
                strings.get(StrKey::TreeShapes),
                (!tree.shapes.is_empty()).then(|| tree.shapes_header()),
                None,
            );
            let all = every_instance(project);
            if eye && let Some(command) = dc_app::tree::toggle_visible(project, &all) {
                out.push(Action::Run(Box::new(command)));
            }
            if lock && let Some(command) = dc_app::tree::toggle_locked(project, &all) {
                out.push(Action::Run(Box::new(command)));
            }
            // The tree scrolls to the first selected row when the
            // selection changes (`experience.md`, section 8).
            let current = app.selection().instances().to_vec();
            let seen: Option<Vec<InstanceId>> = ui.data(|d| d.get_temp(seen_id()));
            let reveal = (seen.as_ref() != Some(&current))
                .then(|| current.first().cloned())
                .flatten();
            ui.data_mut(|d| d.insert_temp(seen_id(), current));
            for row in &tree.shapes {
                // A row out of view keeps its place and is not laid out,
                // so 500 shapes cost a frame what a screenful does
                // (acceptance 87); the row to reveal is always drawn.
                let next =
                    Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), ROW_PX));
                if !ui.is_rect_visible(next) && reveal.as_ref() != Some(&row.id) {
                    ui.allocate_exact_size(next.size(), Sense::hover());
                    continue;
                }
                let response = shape_row(ui, state, &tree, row, out);
                if reveal.as_ref() == Some(&row.id) {
                    response.scroll_to_me(None);
                }
            }

            section(
                ui,
                strings,
                strings.get(StrKey::TreeTabs),
                None,
                Some(strings.get(StrKey::TreeTabsNoFlags)),
            );
            for row in &tree.tabs {
                tab_row(ui, state, row, out);
            }

            section(
                ui,
                strings,
                strings.get(StrKey::TreePictures),
                None,
                Some(strings.get(StrKey::TreePicturesNoFlags)),
            );
            if tree.pictures.is_empty() {
                let palette = Palette::current(ui);
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
                ui.painter().text(
                    egui::pos2(rect.left() + MARGIN_PX, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    strings.get(StrKey::TreeNoPictures),
                    TextStyle::Body.resolve(ui.style()),
                    palette.text_secondary,
                );
            }
        });
}

/// The selection a click on `row` with `modifiers` makes.
fn clicked(ui: &Ui, state: &FrameState<'_>, tree: &Tree, row: &ShapeRow) -> Selection {
    let modifiers = ui.input(|i| i.modifiers);
    let selection = state.app.selection();
    match mode(modifiers) {
        ClickMode::Add => {
            let anchor: Option<InstanceId> = ui.data(|d| d.get_temp(anchor_id()));
            let range = anchor.map_or_else(|| vec![row.id.clone()], |a| tree.range(&a, &row.id));
            let mut ids = selection.instances().to_vec();
            for id in range {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Selection::Instances(ids)
        }
        mode => selection.clicked(&dc_app::Hit::Instance(row.id.clone()), mode),
    }
}

/// One instance's row.
fn shape_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    tree: &Tree,
    row: &ShapeRow,
    out: &mut Vec<Action>,
) -> egui::Response {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let project = state.app.project();
    let selected = state.app.selection().instances().contains(&row.id);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let name = row.name.clone();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &name));

    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, 0.0, palette.accent_soft);
        painter.rect_filled(
            Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
            0.0,
            palette.accent,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, palette.surface_hover);
    }

    // The eye and the padlock, left.
    let eye_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    let lock_rect = eye_rect.translate(egui::vec2(ICON_BOX_PX + 2.0, 0.0));
    let (eye_icon, eye_name) = if row.visible {
        (Icon::Eye, strings.get(StrKey::TreeHide))
    } else {
        (Icon::EyeOff, strings.get(StrKey::TreeShow))
    };
    let eye_colour = if row.visible {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    let ids = std::slice::from_ref(&row.id);
    if icon_button(
        ui,
        eye_rect,
        ui.make_persistent_id(("tree_eye", row.id.as_str())),
        eye_icon,
        eye_colour,
        eye_name,
        true,
    )
    .clicked()
        && let Some(command) = dc_app::tree::toggle_visible(project, ids)
    {
        out.push(Action::Run(Box::new(command)));
    }
    let row_hovered = ui.rect_contains_pointer(rect);
    let (lock_icon, lock_name, lock_colour) = if row.locked {
        (
            Icon::Lock,
            strings.get(StrKey::TreeUnlock),
            palette.viewport_shape_locked,
        )
    } else if row_hovered {
        (
            Icon::LockOpen,
            strings.get(StrKey::TreeLock),
            palette.text_disabled,
        )
    } else {
        (Icon::LockOpen, strings.get(StrKey::TreeLock), theme::NONE)
    };
    if icon_button(
        ui,
        lock_rect,
        ui.make_persistent_id(("tree_lock", row.id.as_str())),
        lock_icon,
        lock_colour,
        lock_name,
        true,
    )
    .clicked()
        && let Some(command) = dc_app::tree::toggle_locked(project, ids)
    {
        out.push(Action::Run(Box::new(command)));
    }

    // The count, right; the badges before it.
    let small = TextStyle::Small;
    let count = row.operations.len().to_string();
    let count_galley =
        ui.painter()
            .layout_no_wrap(count, small.resolve(ui.style()), palette.text_secondary);
    let count_rect = Rect::from_min_max(
        egui::pos2(
            rect.right() - MARGIN_PX - count_galley.size().x.max(ICON_BOX_PX),
            rect.top(),
        ),
        egui::pos2(rect.right() - MARGIN_PX, rect.bottom()),
    );
    painter.galley(
        egui::pos2(
            count_rect.right() - count_galley.size().x,
            rect.center().y - count_galley.size().y / 2.0,
        ),
        count_galley,
        palette.text_secondary,
    );
    let names: Vec<&str> = row.operations.iter().map(|o| o.name.as_str()).collect();
    let hover = if names.is_empty() {
        strings.get(StrKey::TreeNotTargeted).to_owned()
    } else {
        strings.fmt(StrKey::TreeTargetedBy, &[("names", &names.join(", "))])
    };
    ui.interact(
        count_rect,
        ui.make_persistent_id(("tree_count", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(hover);

    let mut right = count_rect.left() - 8.0;
    if row.linked {
        let r = Rect::from_center_size(
            egui::pos2(right - ICON_BOX_PX / 2.0, rect.center().y),
            egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
        );
        glyph(ui, r, Icon::Link2, palette.text_secondary);
        ui.interact(
            r,
            ui.make_persistent_id(("tree_linked", row.id.as_str())),
            Sense::hover(),
        )
        .on_hover_text(strings.get(StrKey::TreeLinked));
        right = r.left() - 4.0;
    }
    if row.open {
        let label = strings.get(StrKey::TreeOpen);
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            small.resolve(ui.style()),
            palette.text_secondary,
        );
        let pill = Rect::from_center_size(
            egui::pos2(right - galley.size().x / 2.0 - 6.0, rect.center().y),
            egui::vec2(galley.size().x + 12.0, 16.0),
        );
        painter.rect(
            pill,
            8.0,
            palette.surface_stripe,
            Stroke::new(1.0, palette.border_default),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            palette.text_secondary,
        );
        ui.interact(
            pill,
            ui.make_persistent_id(("tree_open", row.id.as_str())),
            Sense::hover(),
        )
        .on_hover_text(strings.get(StrKey::TreeOpenHelp));
        right = pill.left() - 4.0;
    }

    // The name, or the rename field.
    let name_rect = Rect::from_min_max(
        egui::pos2(lock_rect.right() + 8.0, rect.top()),
        egui::pos2(right, rect.bottom()),
    );
    let renaming: Option<(InstanceId, String, bool)> = ui.data(|d| d.get_temp(renaming_id()));
    match renaming {
        Some((id, mut text, fresh)) if id == row.id => {
            let field_id = ui.make_persistent_id(("tree_rename", row.id.as_str()));
            let edit = ui.put(
                name_rect.shrink2(egui::vec2(0.0, 3.0)),
                egui::TextEdit::singleline(&mut text)
                    .id(field_id)
                    .font(TextStyle::Body)
                    .desired_width(name_rect.width()),
            );
            let edit =
                edit.accessible_name(strings.fmt(StrKey::TreeRename, &[("name", &row.name)]));
            if fresh {
                edit.request_focus();
                if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), field_id) {
                    s.cursor
                        .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
                    s.store(ui.ctx(), field_id);
                }
            }
            if edit.lost_focus() && !fresh {
                ui.data_mut(|d| d.remove::<(InstanceId, String, bool)>(renaming_id()));
                let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if !escaped && let Some(command) = dc_app::tree::rename(project, &row.id, &text) {
                    out.push(Action::Run(Box::new(command)));
                }
            } else {
                ui.data_mut(|d| d.insert_temp(renaming_id(), (id, text, false)));
            }
        }
        _ => {
            let colour = if row.visible {
                palette.text_primary
            } else {
                palette.text_secondary
            };
            let galley = truncated(ui, &row.name, &TextStyle::Body, colour, name_rect.width());
            painter.galley(
                egui::pos2(name_rect.left(), rect.center().y - galley.size().y / 2.0),
                galley,
                colour,
            );
        }
    }

    if response.double_clicked() {
        ui.data_mut(|d| d.insert_temp(renaming_id(), (row.id.clone(), row.name.clone(), true)));
    } else if response.clicked() {
        let next = clicked(ui, state, tree, row);
        ui.data_mut(|d| d.insert_temp(anchor_id(), row.id.clone()));
        out.push(Action::Select(next));
    }
    if response.secondary_clicked() && !selected {
        out.push(Action::Select(Selection::Instances(vec![row.id.clone()])));
    }
    response.context_menu(|ui| {
        let chosen: Vec<InstanceId> = if selected {
            state.app.selection().instances().to_vec()
        } else {
            vec![row.id.clone()]
        };
        context_menu(ui, strings, project, &chosen, out);
    });
    response
}

/// The context menu for `ids` (capability 57).
fn context_menu(
    ui: &mut Ui,
    strings: &Strings,
    project: &Project,
    ids: &[InstanceId],
    out: &mut Vec<Action>,
) {
    for item in dc_app::tree::menu(project, ids) {
        let key = match item {
            MenuItem::Hide => StrKey::TreeHide,
            MenuItem::Show => StrKey::TreeShow,
            MenuItem::Lock => StrKey::TreeLock,
            MenuItem::Unlock => StrKey::TreeUnlock,
            MenuItem::Duplicate => StrKey::TreeDuplicate,
            MenuItem::Delete => StrKey::TreeDelete,
            MenuItem::ZoomTo => StrKey::TreeZoomTo,
            MenuItem::AddOperation => StrKey::TreeAddOperation,
        };
        if item == MenuItem::AddOperation {
            ui.separator();
            // Slice 10 (#539) lists the kinds the selection can take.
            ui.add_enabled_ui(false, |ui| ui.menu_button(strings.get(key), |_| {}))
                .inner
                .response
                .on_disabled_hover_text(strings.get(StrKey::TreeAddOperationLater));
            continue;
        }
        if item == MenuItem::Delete {
            ui.separator();
        }
        if ui.button(strings.get(key)).clicked() {
            match item {
                MenuItem::ZoomTo => out.push(Action::ZoomToSelection),
                _ => {
                    if let Some(command) = dc_app::tree::menu_command(project, item, ids) {
                        out.push(Action::Run(Box::new(command)));
                    }
                }
            }
            ui.close();
        }
    }
}

/// One outline's placed tabs (capability 57): a click selects them.
fn tab_row(ui: &mut Ui, state: &FrameState<'_>, row: &TabRow, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let selected = match state.app.selection() {
        Selection::Tabs(ids) => !ids.is_empty() && row.tabs.iter().all(|t| ids.contains(t)),
        _ => false,
    };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let label = if row.tabs.len() == 1 {
        strings.fmt(StrKey::TreeTabRowOne, &[("name", &row.name)])
    } else {
        strings.fmt(
            StrKey::TreeTabRow,
            &[("name", &row.name), ("count", &row.tabs.len().to_string())],
        )
    };
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &label));
    if selected {
        ui.painter().rect_filled(rect, 0.0, palette.accent_soft);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, palette.surface_hover);
    }
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    glyph(ui, icon_rect, Icon::Bookmark, palette.text_secondary);
    let width = rect.right() - MARGIN_PX - icon_rect.right() - 8.0;
    let galley = truncated(ui, &label, &TextStyle::Body, palette.text_primary, width);
    ui.painter().galley(
        egui::pos2(
            icon_rect.right() + 8.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text_primary,
    );
    if response.clicked() {
        out.push(Action::Select(Selection::Tabs(row.tabs.clone())));
    }
}
