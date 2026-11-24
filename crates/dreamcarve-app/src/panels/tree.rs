// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Design Tree (functional spec, capability 57; `experience.md`,
//! sections 6 to 8; mock-up `01-workspace.svg`, left): the Shapes
//! section, one 30-px row per instance in file order with its eye, its
//! padlock (shown on hover unless locked; its tooltip says what a lock
//! does), the shape glyph (`square`, `circle` or `spline`: never the word
//! "open"), its name, the `component` badge of a linked copy and the count
//! of operations targeting it; then the Tabs section, a row per outline
//! holding placed tabs with a row per tab under it (#546); then Pictures,
//! a row per bitmap with its eye, its padlock, the `image` glyph, its
//! name and the count of Photo V-Carves that carve it (capabilities 56
//! and 57). An empty section is its header alone (slice 25, #617). The
//! Shapes and Pictures headers' eye and padlock act on every row in the
//! section.
//!
//! The rows come from `dc_app::tree`; a click, the context menu, an eye,
//! a padlock or a rename only push actions, so every change is a command
//! and one undo step. The selection is `dc_app`'s, the one the Viewport
//! and the Properties panel show: click selects, Shift-click selects the
//! range from the last click, Ctrl/⌘-click toggles. A double-click, F2 on
//! the selected shape or *Rename* in the context menu turns the name into
//! a field in place with the name selected ([`start_rename`]); Enter or
//! leaving it commits, Escape reverts. What is being renamed and where
//! the range starts live in egui's memory, not the project. A picture's
//! row answers the same way among the pictures, which are selected on
//! their own ([`start_rename_picture`], `dc_app::tree::bitmap_menu`).

use dc_app::tree::{
    BitmapRow, GroupRow, MenuItem, Row, Section, ShapeGlyph, ShapeRow, TabRow, Tree,
};
use dc_app::{ClickMode, Selection, StrKey, Strings};
use dc_project::{BitmapId, GroupId, InstanceId, Project};
use eframe::egui::{self, Id, Rect, Sense, Stroke, TextStyle, Ui};

use crate::panels;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState, PasteAt};
use crate::widgets::button::focus_ring_round;
use crate::widgets::checkbox::{BOX_PX, paint_box};
use crate::widgets::icon::Icon;
use crate::widgets::row::{surface, surface_rect};

/// A tree row's height, px (`experience.md`, section 3).
const ROW_PX: f32 = theme::TREE_ROW_PX;
/// A section header's height, px.
const SECTION_PX: f32 = theme::SECTION_HEADER_PX;
/// The panel's content margin, px.
const MARGIN_PX: f32 = theme::CARD_PADDING_PX;
/// The space between a row's surface and its content at each end, px.
const ROW_PADDING_PX: f32 = 8.0;
/// An icon's hit square, px.
const ICON_BOX_PX: f32 = 20.0;
/// The least a clickable glyph's hit area is, px square.
const MIN_HIT_PX: f32 = 24.0;
/// The bar on a target row's left edge, px (`experience.md`, section 7,
/// "Design Tree row").
const TARGET_BAR_PX: f32 = 3.0;

/// Where the next Shift-click's range starts: the last row clicked.
fn anchor_id() -> Id {
    Id::unique("design_tree_anchor")
}

/// The instance whose name is a field, and the text typed so far.
fn renaming_id() -> Id {
    Id::unique("design_tree_renaming")
}

/// Turns instance `id`'s name into a field in place, the name selected,
/// on the tree's next frame (F2, the context menu's *Rename*).
pub(crate) fn start_rename(ctx: &egui::Context, project: &Project, id: &InstanceId) {
    if let Some(instance) = project.instances.iter().find(|i| i.id == *id) {
        ctx.data_mut(|d| {
            d.insert_temp(renaming_id(), (id.clone(), instance.name.clone(), true));
        });
    }
}

/// Where the next Shift-click's range starts among the pictures.
fn picture_anchor_id() -> Id {
    Id::unique("design_tree_picture_anchor")
}

/// The picture whose name is a field, and the text typed so far.
fn renaming_picture_id() -> Id {
    Id::unique("design_tree_renaming_picture")
}

/// Turns picture `id`'s name into a field in place, the name selected,
/// on the tree's next frame (F2, its menu's *Rename*).
pub(crate) fn start_rename_picture(ctx: &egui::Context, project: &Project, id: &BitmapId) {
    if let Some(bitmap) = project.bitmaps.iter().find(|b| b.id == *id) {
        ctx.data_mut(|d| {
            d.insert_temp(
                renaming_picture_id(),
                (id.clone(), bitmap.name.clone(), true),
            );
        });
    }
}

/// The group whose name is a field, and the text typed so far.
fn renaming_group_id() -> Id {
    Id::unique("design_tree_renaming_group")
}

/// Turns group `id`'s name into a field in place, the name selected, on
/// the tree's next frame (its menu's *Rename*).
pub(crate) fn start_rename_group(ctx: &egui::Context, project: &Project, id: &GroupId) {
    if let Some(group) = project.groups.iter().find(|f| f.id == *id) {
        ctx.data_mut(|d| {
            d.insert_temp(renaming_group_id(), (id.clone(), group.name.clone(), true));
        });
    }
}

/// The groups whose rows are folded away: view state, kept while the
/// app runs.
fn collapsed_id() -> Id {
    Id::unique("design_tree_collapsed")
}

/// The rows being dragged, while a drag is under way.
fn dragging_id() -> Id {
    Id::unique("design_tree_dragging")
}

/// How far a row filed in a group is set in from the rows that are not.
const INDENT_PX: f32 = 16.0;

/// Where the rows being dragged would go if they were let go now
/// (capability 99), and where the line that says so is drawn.
#[derive(Debug, Clone, PartialEq)]
struct DropAt {
    /// The row they go before.
    before: Option<InstanceId>,
    /// The group they are filed in.
    group: Option<GroupId>,
    /// The line: its height in the panel, and how far it is set in.
    line: (f32, f32),
}

/// Which half of `rect` the pointer is in, while rows are dragged over
/// it: `Some(true)` for the upper.
fn drop_half(ui: &Ui, dragging: bool, rect: Rect) -> Option<bool> {
    if !dragging {
        return None;
    }
    let at = ui.input(|i| i.pointer.interact_pos())?;
    rect.contains(at).then(|| at.y < rect.center().y)
}

/// The selection the tree last drew, to tell when it changed.
fn seen_id() -> Id {
    Id::unique("design_tree_seen_selection")
}

/// The pictures the tree last drew selected.
fn seen_pictures_id() -> Id {
    Id::unique("design_tree_seen_pictures")
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

/// A clickable glyph at `rect`, named `name` for assistive tools and
/// showing `name` on hover; whether it was clicked.
fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    icon: Icon,
    colour: egui::Color32,
    name: &str,
    enabled: bool,
) -> egui::Response {
    tipped_button(ui, rect, id, (icon, colour), name, name, enabled)
}

/// [`icon_button`] whose hover says `tip` rather than its name (the
/// padlock's, which says what a lock does).
fn tipped_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    (icon, colour): (Icon, egui::Color32),
    name: &str,
    tip: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    // The glyph's box is 20 px; its hit area is the 24 px the
    // accessibility rule asks (section 11, "Hit sizes").
    let rect = rect.expand((MIN_HIT_PX - rect.width()).max(0.0) / 2.0);
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    let palette = Palette::current(ui);
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::pill(rect.height()), palette.surface_hover);
    }
    glyph(ui, rect, icon, colour);
    if enabled {
        response.on_hover_text(tip)
    } else {
        response.on_disabled_hover_text(tip)
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
/// the rows, `None` for mixed. `names` are what the two buttons are
/// called: hide all, show all, unlock all, lock all. Returns which was
/// clicked.
fn section(
    ui: &mut Ui,
    strings: &Strings,
    title: &str,
    state: Option<dc_app::tree::SectionState>,
    (names, disabled_reason): ([StrKey; 4], Option<&str>),
    extra: Option<(Icon, &str)>,
) -> (bool, bool, bool) {
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
    let [hide_all, show_all, unlock_all, lock_all] = names;
    let (eye_icon, eye_name) = match visible {
        Some(true) => (Icon::Eye, strings.get(hide_all)),
        Some(false) => (Icon::EyeOff, strings.get(show_all)),
        None => (Icon::Eye, strings.get(show_all)),
    };
    let (lock_icon, lock_name) = match locked {
        Some(true) => (Icon::Lock, strings.get(unlock_all)),
        _ => (Icon::LockOpen, strings.get(lock_all)),
    };
    // A section with no rows has nothing to act on: its buttons keep
    // their names and are disabled; one that never has flags says why.
    let eye = icon_button(
        ui,
        eye_rect,
        ui.make_persistent_id((title, "eye")),
        eye_icon,
        colour,
        disabled_reason.unwrap_or(eye_name),
        enabled,
    );
    let lock = icon_button(
        ui,
        lock_rect,
        ui.make_persistent_id((title, "lock")),
        lock_icon,
        colour,
        disabled_reason.unwrap_or(lock_name),
        enabled,
    );
    // A button of the section's own, before the eye: the Shapes
    // section's New Group.
    let extra = extra.is_some_and(|(icon, name)| {
        icon_button(
            ui,
            eye_rect.translate(egui::vec2(-ICON_BOX_PX - 4.0, 0.0)),
            ui.make_persistent_id((title, "extra")),
            icon,
            palette.text_secondary,
            name,
            true,
        )
        .clicked()
    });
    (eye.clicked(), lock.clicked(), extra)
}

/// The Shapes header's buttons: hide all, show all, unlock all, lock all.
const SHAPE_FLAGS: [StrKey; 4] = [
    StrKey::TreeHideAll,
    StrKey::TreeShowAll,
    StrKey::TreeUnlockAll,
    StrKey::TreeLockAll,
];
/// The Pictures header's, named apart from the Shapes header's.
const PICTURE_FLAGS: [StrKey; 4] = [
    StrKey::TreeHideAllPictures,
    StrKey::TreeShowAllPictures,
    StrKey::TreeUnlockAllPictures,
    StrKey::TreeLockAllPictures,
];

/// Every instance id of `project`.
fn every_instance(project: &Project) -> Vec<InstanceId> {
    project.instances.iter().map(|i| i.id.clone()).collect()
}

/// Draws the Design Tree under its header.
pub(crate) fn show(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    // The marked operation's targets, as the Viewport marks them.
    let tree = Tree::build(project).with_targets(app.scene().targets());
    crate::widgets::scroll::show(
        ui,
        egui::ScrollArea::vertical().auto_shrink(false),
        crate::widgets::scroll::Surface::Panel,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // The tree scrolls to the first selected row when the
            // selection changes (`experience.md`, section 8).
            let current = app.selection().instances().to_vec();
            let seen: Option<Vec<InstanceId>> = ui.data(|d| d.get_temp(seen_id()));
            let reveal = (seen.as_ref() != Some(&current))
                .then(|| current.first().cloned())
                .flatten();
            ui.data_mut(|d| d.insert_temp(seen_id(), current));
            let pictures = app.selected_pictures().to_vec();
            let seen: Option<Vec<BitmapId>> = ui.data(|d| d.get_temp(seen_pictures_id()));
            let reveal_picture = (seen.as_ref() != Some(&pictures))
                .then(|| pictures.first().cloned())
                .flatten();
            ui.data_mut(|d| d.insert_temp(seen_pictures_id(), pictures));
            // Rows dragged within the tree (capability 99): where they
            // would go is read row by row, and a line drawn there.
            let collapsed: Vec<GroupId> =
                ui.data(|d| d.get_temp(collapsed_id())).unwrap_or_default();
            let dragging: Option<Vec<InstanceId>> = ui.data(|d| d.get_temp(dragging_id()));
            let mut drop: Option<DropAt> = None;
            // An empty section is its header alone (capability 57).
            for line in tree.rows() {
                match line {
                    Row::Header(Section::Shapes) => {
                        let (eye, lock, new_group) = section(
                            ui,
                            strings,
                            strings.get(StrKey::TreeShapes),
                            (!tree.shapes.is_empty()).then(|| tree.shapes_header()),
                            (SHAPE_FLAGS, None),
                            Some((Icon::Group, strings.get(StrKey::TreeNewGroupHelp))),
                        );
                        // New Group holds the selection, or nothing.
                        if new_group && app.target_editing().is_none() {
                            out.push(Action::run(dc_app::Command::NewGroup {
                                name: dc_app::tree::next_group_name(project),
                                ids: app.selection().instances().to_vec(),
                                together: true,
                            }));
                        }
                        let all = every_instance(project);
                        if eye && let Some(command) = dc_app::tree::toggle_visible(project, &all) {
                            out.push(Action::Run(Box::new(command)));
                        }
                        if lock && let Some(command) = dc_app::tree::toggle_locked(project, &all) {
                            out.push(Action::Run(Box::new(command)));
                        }
                    }
                    Row::Group(row) => {
                        let folded = collapsed.contains(&row.id);
                        let response = group_row(ui, state, row, folded, out);
                        let rect = surface_rect(response.rect);
                        // Dropped on its upper half, rows go before the
                        // group; on its lower, in it, first. A group
                        // folded away or empty takes them last.
                        match drop_half(ui, dragging.is_some(), rect) {
                            Some(_) if folded || row.rows.is_empty() => {
                                drop = Some(DropAt {
                                    before: None,
                                    group: Some(row.id.clone()),
                                    line: (rect.bottom(), INDENT_PX),
                                });
                            }
                            Some(upper) => {
                                drop = Some(DropAt {
                                    before: row.rows.first().cloned(),
                                    group: (!upper).then(|| row.id.clone()),
                                    line: if upper {
                                        (rect.top(), 0.0)
                                    } else {
                                        (rect.bottom(), INDENT_PX)
                                    },
                                });
                            }
                            None => {}
                        }
                    }
                    Row::Shape(row) => {
                        // A folded group's rows are not listed, unless
                        // one is to be revealed.
                        if row
                            .group
                            .as_ref()
                            .is_some_and(|group| collapsed.contains(group))
                            && reveal.as_ref() != Some(&row.id)
                        {
                            continue;
                        }
                        // A row out of view keeps its place and is not
                        // laid out, so 500 shapes cost a frame what a
                        // screenful does (acceptance 87); the row to
                        // reveal is always drawn.
                        let next = Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), ROW_PX),
                        );
                        if !ui.is_rect_visible(next) && reveal.as_ref() != Some(&row.id) {
                            ui.allocate_exact_size(next.size(), Sense::hover());
                            continue;
                        }
                        let response = shape_row(ui, state, &tree, row, out);
                        if reveal.as_ref() == Some(&row.id) {
                            response.scroll_to_me(None);
                        }
                        let rect = surface_rect(response.rect);
                        let indent = if row.group.is_some() { INDENT_PX } else { 0.0 };
                        match drop_half(ui, dragging.is_some(), rect) {
                            Some(true) => {
                                drop = Some(DropAt {
                                    before: Some(row.id.clone()),
                                    group: row.group.clone(),
                                    line: (rect.top(), indent),
                                });
                            }
                            // After this row, in the group it is in:
                            // before the next row of that group, or as
                            // its last. A row in no group is followed
                            // by whatever row comes next.
                            Some(false) => {
                                let next = tree
                                    .shapes
                                    .iter()
                                    .skip_while(|other| other.id != row.id)
                                    .nth(1);
                                let beside = next
                                    .filter(|next| next.group == row.group || row.group.is_none());
                                drop = Some(DropAt {
                                    before: beside.map(|next| next.id.clone()),
                                    group: row.group.clone(),
                                    line: (rect.bottom(), indent),
                                });
                            }
                            None => {}
                        }
                    }
                    Row::Header(Section::Tabs) => {
                        let top = ui.cursor().min.y;
                        section(
                            ui,
                            strings,
                            strings.get(StrKey::TreeTabs),
                            None,
                            (SHAPE_FLAGS, Some(strings.get(StrKey::TreeTabsNoFlags))),
                            None,
                        );
                        // Past the last shape: the end, in no group.
                        let header = Rect::from_min_max(
                            egui::pos2(ui.min_rect().left(), top),
                            egui::pos2(ui.min_rect().right(), top + SECTION_PX),
                        );
                        if drop_half(ui, dragging.is_some(), header).is_some() {
                            drop = Some(DropAt {
                                before: None,
                                group: None,
                                line: (top, 0.0),
                            });
                        }
                    }
                    Row::Tab(row) => tab_row(ui, state, row, out),
                    Row::Header(Section::Pictures) => {
                        let (eye, lock, _) = section(
                            ui,
                            strings,
                            strings.get(StrKey::TreePictures),
                            (!tree.bitmaps.is_empty()).then(|| tree.bitmaps_header()),
                            (PICTURE_FLAGS, None),
                            None,
                        );
                        let all: Vec<BitmapId> =
                            tree.bitmaps.iter().map(|row| row.id.clone()).collect();
                        if eye
                            && let Some(command) =
                                dc_app::tree::toggle_bitmap_visible(project, &all)
                        {
                            out.push(Action::run(command));
                        }
                        if lock
                            && let Some(command) = dc_app::tree::toggle_bitmap_locked(project, &all)
                        {
                            out.push(Action::run(command));
                        }
                    }
                    Row::Bitmap(row) => {
                        let response = bitmap_row(ui, state, &tree, row, out);
                        if reveal_picture.as_ref() == Some(&row.id) {
                            response.scroll_to_me(None);
                        }
                    }
                }
            }
            if let Some(ids) = dragging {
                let (released, down) =
                    ui.input(|i| (i.pointer.any_released(), i.pointer.any_down()));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(at) = &drop {
                    let (y, indent) = at.line;
                    let inner = surface_rect(ui.min_rect());
                    ui.painter().hline(
                        (inner.left() + indent)..=inner.right(),
                        y,
                        Stroke::new(2.0, Palette::current(ui).accent),
                    );
                }
                if released || !down {
                    ui.data_mut(|d| d.remove::<Vec<InstanceId>>(dragging_id()));
                    if let (true, Some(at)) = (released, drop) {
                        let command = dc_app::Command::MoveRows {
                            ids,
                            before: at.before,
                            group: at.group,
                        };
                        // Rows let go where they were change nothing,
                        // and say nothing.
                        if command.apply(project).is_ok() {
                            out.push(Action::run(command));
                        }
                    }
                }
            }
        },
    );
}

/// What a row's rename field did this frame.
enum Renamed {
    /// Still typing: the text so far.
    Typing(String),
    /// Escape: the name stays as it was.
    Reverted,
    /// Enter or leaving the field: the name typed.
    Committed(String),
}

/// A row's name as a field in place, at `name_rect`, holding `text`: on
/// its first frame (`fresh`) it takes the focus with the text selected.
/// Enter or leaving it commits and Escape reverts (`experience.md`,
/// section 8).
fn rename_field(
    ui: &mut Ui,
    name_rect: Rect,
    field_id: Id,
    label: &str,
    mut text: String,
    fresh: bool,
) -> Renamed {
    // The field fits the row: a stock field is a control tall.
    let spacing = ui.spacing().clone();
    ui.spacing_mut().interact_size.y = name_rect.height() - 4.0;
    ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
    let edit = ui.put(
        name_rect.shrink2(egui::vec2(0.0, 2.0)),
        egui::TextEdit::singleline(&mut text)
            .id(field_id)
            .font(TextStyle::Body)
            .desired_width(name_rect.width()),
    );
    *ui.spacing_mut() = spacing;
    let edit = edit.accessible_name(label);
    if fresh {
        edit.request_focus();
        if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), field_id) {
            s.cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
            s.store(ui.ctx(), field_id);
        }
    }
    // Escape reverts. egui drops the focus as the frame begins and the
    // Viewport's keys take the press, so the focus router says whether
    // this frame's Escape left a widget.
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape))
        || crate::shell::focus::escape_left_widget(ui.ctx());
    if escaped && !fresh {
        Renamed::Reverted
    } else if (edit.lost_focus() || crate::widgets::unit_field::left_with_focus(ui)) && !fresh {
        Renamed::Committed(text)
    } else {
        Renamed::Typing(text)
    }
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
    // Target editing (capability 67): the row toggles the shape in the
    // draft, its eye is a checkbox, and nothing else of it answers.
    let editing = state.app.target_editing();
    // A row is dragged to reorder it, or to file it (capability 99).
    let (slot, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_PX),
        Sense::click_and_drag(),
    );
    let name = row.name.clone();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &name));
    if response.drag_started() && editing.is_none() {
        // The selection goes with a selected row; another row goes
        // alone.
        let ids = if selected {
            state.app.selection().instances().to_vec()
        } else {
            vec![row.id.clone()]
        };
        ui.data_mut(|d| d.insert_temp(dragging_id(), ids));
    }

    let painter = ui.painter().clone();
    // The row's own surface, inset from the card's edges.
    let rect = surface_rect(slot);
    surface(ui, rect, selected, response.hovered());
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    // A target of the marked operation: a 3-px bar in its colour on the
    // surface's left edge, where a selected row's accent bar is.
    if let Some(colour) = row.target {
        let inset = rect.height() / 4.0;
        painter.rect_filled(
            Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + inset),
                egui::pos2(rect.left() + TARGET_BAR_PX, rect.bottom() - inset),
            ),
            theme::pill(TARGET_BAR_PX),
            palette.operation(colour),
        );
    }

    // The eye and the padlock, left.
    let eye_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    let lock_rect = eye_rect.translate(egui::vec2(ICON_BOX_PX + 4.0, 0.0));
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
    if let Some(editing) = editing {
        // The eye's place holds a checkbox: in the draft or not.
        let on = editing.targets().contains(&row.id);
        let key = if on {
            StrKey::TreeTargetOn
        } else {
            StrKey::TreeTargetOff
        };
        let check_name = strings.fmt(key, &[("name", &row.name)]);
        let check = ui.interact(
            eye_rect,
            ui.make_persistent_id(("tree_target", row.id.as_str())),
            Sense::click(),
        );
        check.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &check_name)
        });
        let square = Rect::from_center_size(eye_rect.center(), egui::Vec2::splat(BOX_PX));
        paint_box(ui, square, on, check.hovered(), true);
        focus_ring_round(ui, &check, square, theme::MARK_RADIUS_PX);
        if check.clicked() {
            out.push(Action::ToggleTarget(row.id.clone()));
        }
    } else if icon_button(
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
    let row_hovered = ui.rect_contains_pointer(rect) && editing.is_none();
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
    // The padlock's hover says what a lock does: it stops transforms only.
    if tipped_button(
        ui,
        lock_rect,
        ui.make_persistent_id(("tree_lock", row.id.as_str())),
        (lock_icon, lock_colour),
        lock_name,
        strings.get(StrKey::TreeLockHelp),
        editing.is_none(),
    )
    .clicked()
        && let Some(command) = dc_app::tree::toggle_locked(project, ids)
    {
        out.push(Action::Run(Box::new(command)));
    }

    // The shape glyph: what the shape is, never a word. A row filed in a
    // group is set in under the group's.
    let indent = if row.group.is_some() { INDENT_PX } else { 0.0 };
    let glyph_rect = lock_rect.translate(egui::vec2(ICON_BOX_PX + 2.0 + indent, 0.0));
    let (shape_icon, shape_tip) = match row.glyph {
        // Construction geometry says so before it says what it is.
        _ if row.construction => (Icon::SquareDashed, StrKey::TreeGlyphConstruction),
        ShapeGlyph::Closed => (Icon::Square, StrKey::TreeGlyphClosed),
        ShapeGlyph::Circle => (Icon::Circle, StrKey::TreeGlyphCircle),
        ShapeGlyph::Open => (Icon::Spline, StrKey::TreeGlyphOpen),
    };
    glyph(ui, glyph_rect, shape_icon, palette.text_secondary);
    ui.interact(
        glyph_rect,
        ui.make_persistent_id(("tree_glyph", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(strings.get(shape_tip));

    // The count, right; the badges before it.
    let small = TextStyle::Small;
    let count = row.operations.len().to_string();
    let count_galley =
        ui.painter()
            .layout_no_wrap(count, small.resolve(ui.style()), palette.text_secondary);
    let count_rect = Rect::from_min_max(
        egui::pos2(
            rect.right() - ROW_PADDING_PX - count_galley.size().x.max(ICON_BOX_PX),
            rect.top(),
        ),
        egui::pos2(rect.right() - ROW_PADDING_PX, rect.bottom()),
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
    // The `target` glyph before the count, in the operation's colour.
    if let (Some(colour), Some(marks)) = (row.target, state.app.scene().targets()) {
        let r = Rect::from_center_size(
            egui::pos2(right - ICON_BOX_PX / 2.0, rect.center().y),
            egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
        );
        glyph(ui, r, Icon::Target, palette.operation(colour));
        let op_name = row
            .operations
            .iter()
            .find(|o| o.id == marks.op)
            .map(|o| o.name.clone())
            .or_else(|| {
                let op = project.operations.iter().find(|o| *o.id() == marks.op)?;
                Some(op.name().to_owned())
            })
            .unwrap_or_default();
        ui.interact(
            r,
            ui.make_persistent_id(("tree_target_mark", row.id.as_str())),
            Sense::hover(),
        )
        .on_hover_text(strings.fmt(StrKey::TreeTargetOf, &[("name", &op_name)]));
        right = r.left() - 4.0;
    }
    if row.linked {
        let r = Rect::from_center_size(
            egui::pos2(right - ICON_BOX_PX / 2.0, rect.center().y),
            egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
        );
        glyph(ui, r, Icon::Component, palette.text_secondary);
        ui.interact(
            r,
            ui.make_persistent_id(("tree_linked", row.id.as_str())),
            Sense::hover(),
        )
        .on_hover_text(strings.get(StrKey::TreeLinked));
        right = r.left() - 4.0;
    }

    // The name, or the rename field.
    let name_rect = Rect::from_min_max(
        egui::pos2(glyph_rect.right() + 8.0, rect.top()),
        egui::pos2(right, rect.bottom()),
    );
    let renaming: Option<(InstanceId, String, bool)> = ui.data(|d| d.get_temp(renaming_id()));
    match renaming {
        Some((id, text, fresh)) if id == row.id => {
            let label = strings.fmt(StrKey::TreeRename, &[("name", &row.name)]);
            let field_id = ui.make_persistent_id(("tree_rename", row.id.as_str()));
            match rename_field(ui, name_rect, field_id, &label, text, fresh) {
                Renamed::Typing(text) => {
                    ui.data_mut(|d| d.insert_temp(renaming_id(), (id, text, false)));
                }
                Renamed::Reverted => {
                    ui.data_mut(|d| d.remove::<(InstanceId, String, bool)>(renaming_id()));
                }
                Renamed::Committed(text) => {
                    ui.data_mut(|d| d.remove::<(InstanceId, String, bool)>(renaming_id()));
                    if let Some(command) = dc_app::tree::rename(project, &row.id, &text) {
                        out.push(Action::Run(Box::new(command)));
                    }
                }
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

    if editing.is_some() {
        if response.clicked() {
            out.push(Action::ToggleTarget(row.id.clone()));
        }
        return response;
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
        context_menu(ui, state, (&row.id, &chosen), PasteAt::Offset, out);
    });
    response
}

/// A group's row (capability 99): its eye and padlock, which act on
/// every row filed in it; the chevron that folds its rows away; its
/// glyph; its name, a field while it is
/// renamed; and how many shapes it holds. A click selects its shapes, a
/// double-click renames it, and its menu is *Rename*, *Ungroup* (the
/// group goes, its shapes stay) and *Delete* (its shapes go with it).
fn group_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    row: &GroupRow,
    folded: bool,
    out: &mut Vec<Action>,
) -> egui::Response {
    let palette = Palette::current(ui);
    let app = state.app;
    let strings = app.strings();
    let project = app.project();
    let editing = app.target_editing().is_some();
    let held = app.selection().instances();
    let selected = !row.rows.is_empty() && row.rows.iter().all(|id| held.contains(id));
    let (slot, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let name = row.name.clone();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &name));
    let painter = ui.painter().clone();
    let rect = surface_rect(slot);
    surface(ui, rect, false, response.hovered());
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);

    // The eye and the padlock, for every row of the group.
    let eye_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    let lock_rect = eye_rect.translate(egui::vec2(ICON_BOX_PX + 4.0, 0.0));
    let has_rows = !row.rows.is_empty() && !editing;
    let colour = if has_rows {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    let (eye_icon, eye_key) = match row.visible {
        Some(false) => (Icon::EyeOff, StrKey::TreeShowAll),
        Some(true) => (Icon::Eye, StrKey::TreeHideAll),
        None => (Icon::Eye, StrKey::TreeShowAll),
    };
    let (lock_icon, lock_key) = match row.locked {
        Some(true) => (Icon::Lock, StrKey::TreeUnlockAll),
        _ => (Icon::LockOpen, StrKey::TreeLockAll),
    };
    let eye = icon_button(
        ui,
        eye_rect,
        ui.make_persistent_id(("tree_group_eye", row.id.as_str())),
        eye_icon,
        colour,
        strings.get(eye_key),
        has_rows,
    );
    if eye.clicked()
        && let Some(command) = dc_app::tree::toggle_visible(project, &row.rows)
    {
        out.push(Action::run(command));
    }
    let lock = icon_button(
        ui,
        lock_rect,
        ui.make_persistent_id(("tree_group_lock", row.id.as_str())),
        lock_icon,
        colour,
        strings.get(lock_key),
        has_rows,
    );
    if lock.clicked()
        && let Some(command) = dc_app::tree::toggle_locked(project, &row.rows)
    {
        out.push(Action::run(command));
    }

    // The chevron folds the rows away; the glyph says it is a group.
    let chevron_rect = lock_rect.translate(egui::vec2(ICON_BOX_PX + 2.0, 0.0));
    let (chevron_icon, chevron_key) = if folded {
        (Icon::ChevronRight, StrKey::TreeExpand)
    } else {
        (Icon::ChevronDown, StrKey::TreeCollapse)
    };
    let chevron = icon_button(
        ui,
        chevron_rect,
        ui.make_persistent_id(("tree_group_chevron", row.id.as_str())),
        chevron_icon,
        palette.text_secondary,
        &strings.fmt(chevron_key, &[("name", &row.name)]),
        true,
    );
    if chevron.clicked() {
        let mut collapsed: Vec<GroupId> =
            ui.data(|d| d.get_temp(collapsed_id())).unwrap_or_default();
        if folded {
            collapsed.retain(|id| *id != row.id);
        } else {
            collapsed.push(row.id.clone());
        }
        ui.data_mut(|d| d.insert_temp(collapsed_id(), collapsed));
    }
    let glyph_rect = chevron_rect.translate(egui::vec2(ICON_BOX_PX, 0.0));
    // One glyph for a group; its tooltip says how its shapes are
    // selected.
    let group_tip = if row.together {
        StrKey::TreeGroup
    } else {
        StrKey::TreeGroupApart
    };
    glyph(ui, glyph_rect, Icon::Group, palette.text_secondary);
    ui.interact(
        glyph_rect,
        ui.make_persistent_id(("tree_group_glyph", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(strings.get(group_tip));

    // How many shapes it holds, right.
    let count = row.rows.len().to_string();
    let count_galley = ui.painter().layout_no_wrap(
        count.clone(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let count_rect = Rect::from_min_max(
        egui::pos2(
            rect.right() - ROW_PADDING_PX - count_galley.size().x.max(ICON_BOX_PX),
            rect.top(),
        ),
        egui::pos2(rect.right() - ROW_PADDING_PX, rect.bottom()),
    );
    painter.galley(
        egui::pos2(
            count_rect.right() - count_galley.size().x,
            rect.center().y - count_galley.size().y / 2.0,
        ),
        count_galley,
        palette.text_secondary,
    );
    ui.interact(
        count_rect,
        ui.make_persistent_id(("tree_group_count", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(strings.fmt(StrKey::TreeGroupRows, &[("count", &count)]));

    // The name, or the rename field.
    let name_rect = Rect::from_min_max(
        egui::pos2(glyph_rect.right() + 6.0, rect.top()),
        egui::pos2(count_rect.left() - 8.0, rect.bottom()),
    );
    let renaming: Option<(GroupId, String, bool)> = ui.data(|d| d.get_temp(renaming_group_id()));
    match renaming {
        Some((id, text, fresh)) if id == row.id => {
            let label = strings.fmt(StrKey::TreeRenameGroup, &[("name", &row.name)]);
            let field_id = ui.make_persistent_id(("tree_rename_group", row.id.as_str()));
            match rename_field(ui, name_rect, field_id, &label, text, fresh) {
                Renamed::Typing(text) => {
                    ui.data_mut(|d| d.insert_temp(renaming_group_id(), (id, text, false)));
                }
                Renamed::Reverted => {
                    ui.data_mut(|d| d.remove::<(GroupId, String, bool)>(renaming_group_id()));
                }
                Renamed::Committed(text) => {
                    ui.data_mut(|d| d.remove::<(GroupId, String, bool)>(renaming_group_id()));
                    let command = dc_app::Command::RenameGroup {
                        id: row.id.clone(),
                        name: text,
                    };
                    // The name it had, or none, changes nothing.
                    if command.apply(project).is_ok() {
                        out.push(Action::run(command));
                    }
                }
            }
        }
        _ => {
            let galley = truncated(
                ui,
                &row.name,
                &TextStyle::Body,
                palette.text_primary,
                name_rect.width(),
            );
            painter.galley(
                egui::pos2(name_rect.left(), rect.center().y - galley.size().y / 2.0),
                galley,
                palette.text_primary,
            );
        }
    }

    if editing {
        return response;
    }
    if response.double_clicked() {
        ui.data_mut(|d| {
            d.insert_temp(
                renaming_group_id(),
                (row.id.clone(), row.name.clone(), true),
            );
        });
    } else if response.clicked() && !row.rows.is_empty() {
        out.push(Action::Select(Selection::Instances(row.rows.clone())));
    }
    response.context_menu(|ui| {
        if ui.button(strings.get(StrKey::TreeRenameItem)).clicked() {
            out.push(Action::RenameGroup(row.id.clone()));
            ui.close();
        }
        // Select Together: a tick, on for a group as it is made.
        let together = strings.get(StrKey::TreeSelectTogether);
        if crate::shell::menu::checked_item(ui, together, row.together).clicked() {
            out.push(Action::run(dc_app::Command::SetGroupTogether {
                id: row.id.clone(),
                together: !row.together,
            }));
            ui.close();
        }
        if ui.button(strings.get(StrKey::AppMenuUngroup)).clicked() {
            out.push(Action::run(dc_app::Command::Ungroup(vec![row.id.clone()])));
            ui.close();
        }
        ui.separator();
        if ui.button(strings.get(StrKey::TreeDelete)).clicked() {
            out.push(Action::run(dc_app::Command::DeleteGroup(row.id.clone())));
            ui.close();
        }
    });
    response
}

/// The context menu of row `on` for `ids`, the selection it acts on
/// (capabilities 57 and 98): a Design Tree row's, and a shape's on the
/// Viewport. *Rename* renames the row itself; *Paste* puts its shapes
/// where `paste` says.
pub(crate) fn context_menu(
    ui: &mut Ui,
    state: &FrameState<'_>,
    (on, ids): (&InstanceId, &[InstanceId]),
    paste: PasteAt,
    out: &mut Vec<Action>,
) {
    let app = state.app;
    let (strings, project) = (app.strings(), app.project());
    // The selected operation: the one the editor shows (capability 57).
    let op = app
        .editing()
        .and_then(|id| project.operations.iter().find(|o| o.id() == id));
    let items = dc_app::tree::menu(project, ids, op, app.can_paste());
    for item in items.iter().copied() {
        // *Add to "Panel"* and *Remove from "Panel"*, under their own rule.
        if let (MenuItem::AddTo | MenuItem::RemoveFrom, Some(op)) = (item, op) {
            if item == MenuItem::AddTo || !ids.iter().any(|id| !op.targets().contains(id)) {
                ui.separator();
            }
            let key = if item == MenuItem::AddTo {
                StrKey::TreeAddTo
            } else {
                StrKey::TreeRemoveFrom
            };
            if ui
                .button(strings.fmt(key, &[("name", op.name())]))
                .clicked()
            {
                match dc_app::tree::target_command(project, op, item, ids) {
                    Ok(command) => out.extend(command.map(Action::run)),
                    Err(e) => out.push(Action::Refused(e.to_string())),
                }
                ui.close();
            }
            continue;
        }
        let key = match item {
            MenuItem::Rename => StrKey::TreeRenameItem,
            MenuItem::Hide => StrKey::TreeHide,
            MenuItem::Show => StrKey::TreeShow,
            MenuItem::Lock => StrKey::TreeLock,
            MenuItem::Unlock => StrKey::TreeUnlock,
            MenuItem::Cut => StrKey::AppMenuCut,
            MenuItem::Copy => StrKey::AppMenuCopy,
            MenuItem::Paste => StrKey::AppMenuPaste,
            MenuItem::Duplicate => StrKey::TreeDuplicate,
            MenuItem::Array => StrKey::ArrangeMenuArray,
            MenuItem::Delete => StrKey::TreeDelete,
            MenuItem::Group => StrKey::AppMenuGroup,
            MenuItem::Ungroup => StrKey::AppMenuUngroup,
            MenuItem::MakeUnique => StrKey::AppMenuMakeUnique,
            MenuItem::ConvertToPath => StrKey::AppMenuConvertToPath,
            MenuItem::Construction | MenuItem::NotConstruction => StrKey::AppMenuConstruction,
            MenuItem::ZoomTo => StrKey::TreeZoomTo,
            MenuItem::AddOperation => StrKey::TreeAddOperation,
            MenuItem::AddTo | MenuItem::RemoveFrom => continue,
        };
        if item == MenuItem::AddOperation {
            ui.separator();
            // The Operations step's Add menu, for these shapes (#539).
            ui.menu_button(strings.get(key), |ui| {
                panels::operations::add_menu(ui, strings, project, (ids, &[]), out);
            });
            continue;
        }
        // Rename | Hide, Lock | Cut, Copy, Paste, Duplicate, Delete |
        // Group, Ungroup | Make Unique, Convert to Path |
        // Zoom to.
        let first_of_its_group = match item {
            MenuItem::Hide
            | MenuItem::Show
            | MenuItem::Cut
            | MenuItem::ZoomTo
            | MenuItem::MakeUnique => true,
            MenuItem::ConvertToPath => !items.contains(&MenuItem::MakeUnique),
            MenuItem::Construction | MenuItem::NotConstruction => {
                !items.contains(&MenuItem::MakeUnique) && !items.contains(&MenuItem::ConvertToPath)
            }
            _ => false,
        };
        if first_of_its_group {
            ui.separator();
        }
        // Construction is a tick: on while every row is construction.
        if matches!(item, MenuItem::Construction | MenuItem::NotConstruction) {
            let ticked = item == MenuItem::NotConstruction;
            if crate::shell::menu::checked_item(ui, strings.get(key), ticked).clicked() {
                if let Some(command) = dc_app::tree::menu_command(project, item, ids) {
                    out.push(Action::run(command));
                }
                ui.close();
            }
            continue;
        }
        if ui.button(strings.get(key)).clicked() {
            match item {
                MenuItem::Rename => out.push(Action::Rename(on.clone())),
                MenuItem::ZoomTo => out.push(Action::ZoomToSelection),
                // The clipboard's are the selection's, which the menu's
                // shapes are: a row not selected was selected as the
                // menu opened.
                MenuItem::Cut => out.push(Action::Cut),
                MenuItem::Copy => out.push(Action::Copy),
                MenuItem::Paste => out.push(Action::Paste(paste)),
                // Array… takes its values in the tool bar.
                MenuItem::Array => out.push(Action::Arm(dc_app::Tool::Adjust(
                    dc_app::tool::Adjust::Array,
                ))),
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

/// The selection a click on picture `row` makes: it alone, the range
/// from the last picture clicked with Shift, or it in or out with Ctrl/⌘.
fn picture_clicked(ui: &Ui, state: &FrameState<'_>, tree: &Tree, row: &BitmapRow) -> Selection {
    let modifiers = ui.input(|i| i.modifiers);
    let selection = state.app.selection();
    match mode(modifiers) {
        ClickMode::Add => {
            let anchor: Option<BitmapId> = ui.data(|d| d.get_temp(picture_anchor_id()));
            let range =
                anchor.map_or_else(|| vec![row.id.clone()], |a| tree.bitmap_range(&a, &row.id));
            let mut ids = selection.bitmaps().to_vec();
            for id in range {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Selection::Bitmaps(ids)
        }
        mode => selection.clicked(&dc_app::Hit::Bitmap(row.id.clone()), mode),
    }
}

/// One picture's row (capabilities 56 and 57): its eye, its padlock, the
/// `image` glyph, its name (a field while it is renamed) and the count of
/// Photo V-Carves that carve it. A click selects it among the pictures, a
/// double-click renames it, and its menu is `dc_app::tree::bitmap_menu`.
fn bitmap_row(
    ui: &mut Ui,
    state: &FrameState<'_>,
    tree: &Tree,
    row: &BitmapRow,
    out: &mut Vec<Action>,
) -> egui::Response {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let project = state.app.project();
    let selected = state.app.selected_pictures().contains(&row.id);
    // Target editing toggles shapes: a picture's row answers nothing.
    let editing = state.app.target_editing().is_some();
    let (slot, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let name = row.name.clone();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &name));
    let painter = ui.painter().clone();
    let rect = surface_rect(slot);
    surface(ui, rect, selected, response.hovered() && !editing);
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    // The picture the marked Photo V-Carve carves: its bar, as a target's.
    if let Some(colour) = row.target {
        let inset = rect.height() / 4.0;
        painter.rect_filled(
            Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + inset),
                egui::pos2(rect.left() + TARGET_BAR_PX, rect.bottom() - inset),
            ),
            theme::pill(TARGET_BAR_PX),
            palette.operation(colour),
        );
    }

    let eye_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    let lock_rect = eye_rect.translate(egui::vec2(ICON_BOX_PX + 4.0, 0.0));
    let (eye_icon, eye_name, eye_colour) = if row.visible {
        (
            Icon::Eye,
            strings.get(StrKey::TreeHide),
            palette.text_secondary,
        )
    } else {
        (
            Icon::EyeOff,
            strings.get(StrKey::TreeShow),
            palette.text_disabled,
        )
    };
    let ids = std::slice::from_ref(&row.id);
    if icon_button(
        ui,
        eye_rect,
        ui.make_persistent_id(("tree_picture_eye", row.id.as_str())),
        eye_icon,
        eye_colour,
        eye_name,
        !editing,
    )
    .clicked()
        && let Some(command) = dc_app::tree::toggle_bitmap_visible(project, ids)
    {
        out.push(Action::run(command));
    }
    let row_hovered = ui.rect_contains_pointer(rect) && !editing;
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
    if tipped_button(
        ui,
        lock_rect,
        ui.make_persistent_id(("tree_picture_lock", row.id.as_str())),
        (lock_icon, lock_colour),
        lock_name,
        strings.get(StrKey::TreeLockHelp),
        !editing,
    )
    .clicked()
        && let Some(command) = dc_app::tree::toggle_bitmap_locked(project, ids)
    {
        out.push(Action::run(command));
    }

    let glyph_rect = lock_rect.translate(egui::vec2(ICON_BOX_PX + 2.0, 0.0));
    glyph(ui, glyph_rect, Icon::Image, palette.text_secondary);
    ui.interact(
        glyph_rect,
        ui.make_persistent_id(("tree_picture_glyph", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(strings.get(StrKey::TreeGlyphPicture));

    // The count of Photo V-Carves that carve it, right.
    let count = row.operations.len().to_string();
    let count_galley = ui.painter().layout_no_wrap(
        count,
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let count_rect = Rect::from_min_max(
        egui::pos2(
            rect.right() - ROW_PADDING_PX - count_galley.size().x.max(ICON_BOX_PX),
            rect.top(),
        ),
        egui::pos2(rect.right() - ROW_PADDING_PX, rect.bottom()),
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
        strings.get(StrKey::TreeNotCarved).to_owned()
    } else {
        strings.fmt(StrKey::TreeCarvedBy, &[("names", &names.join(", "))])
    };
    ui.interact(
        count_rect,
        ui.make_persistent_id(("tree_picture_count", row.id.as_str())),
        Sense::hover(),
    )
    .on_hover_text(hover);

    // The name, or the rename field.
    let name_rect = Rect::from_min_max(
        egui::pos2(glyph_rect.right() + 8.0, rect.top()),
        egui::pos2(count_rect.left() - 8.0, rect.bottom()),
    );
    let renaming: Option<(BitmapId, String, bool)> = ui.data(|d| d.get_temp(renaming_picture_id()));
    match renaming {
        Some((id, text, fresh)) if id == row.id => {
            let label = strings.fmt(StrKey::TreeRename, &[("name", &row.name)]);
            let field_id = ui.make_persistent_id(("tree_picture_rename", row.id.as_str()));
            match rename_field(ui, name_rect, field_id, &label, text, fresh) {
                Renamed::Typing(text) => {
                    ui.data_mut(|d| d.insert_temp(renaming_picture_id(), (id, text, false)));
                }
                Renamed::Reverted => {
                    ui.data_mut(|d| d.remove::<(BitmapId, String, bool)>(renaming_picture_id()));
                }
                Renamed::Committed(text) => {
                    ui.data_mut(|d| d.remove::<(BitmapId, String, bool)>(renaming_picture_id()));
                    if let Some(command) = dc_app::tree::rename_bitmap(project, &row.id, &text) {
                        out.push(Action::run(command));
                    }
                }
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

    if editing {
        return response;
    }
    if response.double_clicked() {
        ui.data_mut(|d| {
            d.insert_temp(
                renaming_picture_id(),
                (row.id.clone(), row.name.clone(), true),
            );
        });
    } else if response.clicked() {
        let next = picture_clicked(ui, state, tree, row);
        ui.data_mut(|d| d.insert_temp(picture_anchor_id(), row.id.clone()));
        out.push(Action::Select(next));
    }
    if response.secondary_clicked() && !selected {
        out.push(Action::Select(Selection::Bitmaps(vec![row.id.clone()])));
    }
    response.context_menu(|ui| {
        let chosen: Vec<BitmapId> = if selected {
            state.app.selected_pictures().to_vec()
        } else {
            vec![row.id.clone()]
        };
        picture_menu(ui, state, (&row.id, &chosen), out);
    });
    response
}

/// The menu of picture row `on` for `ids`, the pictures it acts on
/// (`dc_app::tree::bitmap_menu`): Rename, Hide or Show, Lock or Unlock,
/// Delete, Zoom to and *Add operation ▸*, whose menu offers a Photo
/// V-Carve on the picture.
fn picture_menu(
    ui: &mut Ui,
    state: &FrameState<'_>,
    (on, ids): (&BitmapId, &[BitmapId]),
    out: &mut Vec<Action>,
) {
    let (strings, project) = (state.app.strings(), state.app.project());
    for item in dc_app::tree::bitmap_menu(project, ids) {
        let key = match item {
            MenuItem::Rename => StrKey::TreeRenameItem,
            MenuItem::Hide => StrKey::TreeHide,
            MenuItem::Show => StrKey::TreeShow,
            MenuItem::Lock => StrKey::TreeLock,
            MenuItem::Unlock => StrKey::TreeUnlock,
            MenuItem::Delete => StrKey::TreeDelete,
            MenuItem::ZoomTo => StrKey::TreeZoomTo,
            MenuItem::AddOperation => StrKey::TreeAddOperation,
            // A picture's menu has none of these.
            MenuItem::Duplicate
            | MenuItem::Array
            | MenuItem::Construction
            | MenuItem::NotConstruction
            | MenuItem::Group
            | MenuItem::Ungroup
            | MenuItem::Cut
            | MenuItem::Copy
            | MenuItem::Paste
            | MenuItem::MakeUnique
            | MenuItem::ConvertToPath
            | MenuItem::AddTo
            | MenuItem::RemoveFrom => continue,
        };
        if item == MenuItem::AddOperation {
            ui.separator();
            ui.menu_button(strings.get(key), |ui| {
                panels::operations::add_menu(ui, strings, project, (&[], ids), out);
            });
            continue;
        }
        if matches!(item, MenuItem::Delete | MenuItem::Hide | MenuItem::Show) {
            ui.separator();
        }
        if ui.button(strings.get(key)).clicked() {
            match item {
                MenuItem::Rename => out.push(Action::RenamePicture(on.clone())),
                MenuItem::ZoomTo => out.push(Action::ZoomToSelection),
                _ => {
                    if let Some(command) = dc_app::tree::bitmap_menu_command(project, item, ids) {
                        out.push(Action::run(command));
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
    let (slot, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::click());
    let rect = surface_rect(slot);
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
    surface(ui, rect, selected, response.hovered());
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + ICON_BOX_PX / 2.0, rect.center().y),
        egui::vec2(ICON_BOX_PX, ICON_BOX_PX),
    );
    glyph(ui, icon_rect, Icon::Bookmark, palette.text_secondary);
    let width = rect.right() - ROW_PADDING_PX - icon_rect.right() - 8.0;
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
    for tab in &row.tabs {
        tab_item(ui, state, tab, out);
    }
}

/// One placed tab under its outline's row (mock-up 08: its id and its
/// point in project space): a click selects it, Shift adds it, Ctrl/⌘
/// toggles it, and the Viewport selects the same tab.
fn tab_item(ui: &mut Ui, state: &FrameState<'_>, id: &dc_project::TabId, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let project = state.app.project();
    let selected = state.app.selection().tabs().contains(id);
    let units = project.display_units;
    let (x, y) = project
        .tabs
        .iter()
        .find(|t| t.id == *id)
        .and_then(|t| dc_job::tabs::tab_point(project, t))
        .map_or_else(Default::default, |p| {
            (
                dc_app::units::summary_text(p.x, units),
                dc_app::units::summary_text(p.y, units),
            )
        });
    let label = strings.fmt(
        StrKey::TreeTabItem,
        &[("name", id.as_str()), ("x", &x), ("y", &y)],
    );
    let (slot, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_PX), Sense::hover());
    let response = ui.interact(
        slot,
        Id::unique(("dc.tree.tab", id.as_str())),
        Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::TreeItem, true, selected, &label));
    let rect = surface_rect(slot);
    surface(ui, rect, selected, response.hovered());
    focus_ring_round(ui, &response, rect, theme::CONTROL_RADIUS_PX);
    let left = rect.left() + 8.0 + ICON_BOX_PX + 8.0;
    let width = rect.right() - ROW_PADDING_PX - left;
    let galley = truncated(ui, &label, &TextStyle::Body, palette.text_secondary, width);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text_secondary,
    );
    if response.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        let hit = dc_app::Hit::Tab(id.clone());
        out.push(Action::Select(
            state.app.selection().clicked(&hit, mode(modifiers)),
        ));
    }
}
