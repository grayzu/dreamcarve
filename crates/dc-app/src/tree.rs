// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Design Tree's rows (`docs/specs/phase-4/design.md`, "The Design Tree
//! and placement"; functional spec, capabilities 57 and 59;
//! `experience.md`, sections 6 to 8). Built from the snapshot and nothing
//! else: the instances in file order, each with its eye, padlock, shape
//! glyph (a closed outline, a circle or an open path: never a word, slice
//! 25, #617), *linked* badge and the operations targeting it; then the
//! placed tabs grouped by the outline they sit on; then the pictures, one
//! row per bitmap with its eye, its padlock and the Photo V-Carves that
//! carve it (capability 56). An empty section is its header alone
//! ([`Tree::rows`]).
//! The shell draws the rows and turns a click, a menu item or a rename
//! into the [`Command`] these functions return; the selection is the
//! app's one [`Selection`](crate::Selection), shared with the Viewport.
//!
//! Behaviour read from PureCutCNC's `FeatureTree.tsx` (rows with a
//! visibility eye and a lock, a context menu, double-click to rename, a
//! section header acting on its rows); its folders, regions and
//! construction rows have no counterpart here.

use dc_geometry::{Transform, as_circle};
use dc_project::{BitmapId, FeatureDefinition, InstanceId, Operation, OperationId, Project, TabId};

use crate::command::Command;
use crate::editor::targets;
use crate::error::CommandError;
use crate::mixed::common_bool;
use crate::scene::TargetMarks;

/// What a shape row's glyph says the shape is (`experience.md`, section
/// 6: Lucide `square`, `circle` and `spline`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeGlyph {
    /// Closed outlines: what Profile and Pocket cut.
    Closed,
    /// One closed outline that is a circle, as Drill and Bore find it.
    Circle,
    /// Some outline is an open path: Engrave, or a Profile on the line,
    /// cuts it.
    Open,
}

impl ShapeGlyph {
    /// The glyph of `definition` placed by `transform`, a circle
    /// recognised as Drill reads a hole: the placed outline, within
    /// `tolerance_mm` (the project's flattening tolerance), so a circle
    /// stretched into an ellipse is a closed outline.
    #[must_use]
    pub fn of(definition: &FeatureDefinition, transform: Transform, tolerance_mm: f64) -> Self {
        if definition.profiles.iter().any(|p| !p.is_closed()) {
            Self::Open
        } else if let [one] = definition.profiles.as_slice()
            && let Ok(placed) = transform.apply_polyline(one)
            && as_circle(&placed, tolerance_mm).is_some()
        {
            Self::Circle
        } else {
            Self::Closed
        }
    }
}

/// An operation targeting a row's instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRef {
    /// The operation.
    pub id: OperationId,
    /// Its name, as the hover lists it.
    pub name: String,
}

/// One instance's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeRow {
    /// The instance.
    pub id: InstanceId,
    /// Its name.
    pub name: String,
    /// The eye: drawn on the Viewport.
    pub visible: bool,
    /// The padlock: refuses transforms.
    pub locked: bool,
    /// What the shape is, as its leading glyph says.
    pub glyph: ShapeGlyph,
    /// Another instance places the same definition (the `component`
    /// badge, CONTEXT: Linked Copy).
    pub linked: bool,
    /// The operations whose targets name it, in project order: the row's
    /// count, and its hover's list.
    pub operations: Vec<OperationRef>,
    /// It is a target of the marked operation (capability 67): that
    /// operation's place in the palette, for the bar on the row's left
    /// edge. `None` until [`Tree::with_targets`] marks the rows.
    pub target: Option<usize>,
}

/// The placed tabs on one outline, as one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRow {
    /// The instance whose outline holds them.
    pub instance: InstanceId,
    /// The outline: an index into the definition's profiles.
    pub profile: usize,
    /// The row's name: the instance's, with the outline's number when its
    /// definition has several.
    pub name: String,
    /// The tabs, in file order.
    pub tabs: Vec<TabId>,
}

/// One bitmap's row, in the Pictures section (capabilities 56 and 57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapRow {
    /// The bitmap.
    pub id: BitmapId,
    /// Its name: the picture file's stem, until it is renamed.
    pub name: String,
    /// The eye: drawn on the Viewport.
    pub visible: bool,
    /// The padlock.
    pub locked: bool,
    /// The picture's width, pixels.
    pub width_px: u32,
    /// The picture's height, pixels.
    pub height_px: u32,
    /// The Photo V-Carves that carve it, in project order: the row's
    /// count, and its hover's list.
    pub operations: Vec<OperationRef>,
    /// It is the picture the marked Photo V-Carve carves (capability 67;
    /// slice 15, #544): that operation's place in the palette, for the
    /// bar on the row's left edge. `None` until [`Tree::with_targets`]
    /// marks the rows.
    pub target: Option<usize>,
}

/// The tree's three sections, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The instances.
    Shapes,
    /// The placed tabs, by outline.
    Tabs,
    /// The bitmaps.
    Pictures,
}

/// One row of the tree as the shell draws it, top to bottom
/// ([`Tree::rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a> {
    /// A section's header, drawn whether or not it has rows.
    Header(Section),
    /// An instance's row.
    Shape(&'a ShapeRow),
    /// An outline's placed tabs.
    Tab(&'a TabRow),
    /// A bitmap's row.
    Bitmap(&'a BitmapRow),
}

/// What a section header's eye and padlock show: the rows' common state,
/// `None` when they differ or there are no rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionState {
    /// Every row shown (`Some(true)`), every row hidden, or mixed.
    pub visible: Option<bool>,
    /// Every row locked (`Some(true)`), every row unlocked, or mixed.
    pub locked: Option<bool>,
}

/// The Design Tree of one snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tree {
    /// The Shapes section: one row per instance, in file order.
    pub shapes: Vec<ShapeRow>,
    /// The Tabs section: one row per outline with placed tabs, in the
    /// order of the instances, then of the outlines.
    pub tabs: Vec<TabRow>,
    /// The Pictures section: one row per bitmap, in file order.
    pub bitmaps: Vec<BitmapRow>,
}

impl Tree {
    /// The rows of `project`.
    #[must_use]
    pub fn build(project: &Project) -> Self {
        let shapes = project
            .instances
            .iter()
            .map(|instance| {
                let definition = project.definitions.get(&instance.definition);
                ShapeRow {
                    id: instance.id.clone(),
                    name: instance.name.clone(),
                    visible: instance.visible,
                    locked: instance.locked,
                    glyph: definition.map_or(ShapeGlyph::Closed, |d| {
                        ShapeGlyph::of(d, instance.transform, project.flatten_tolerance_mm)
                    }),
                    linked: project.instances.iter().any(|other| {
                        other.id != instance.id && other.definition == instance.definition
                    }),
                    operations: project
                        .operations
                        .iter()
                        .filter(|op| op.targets().contains(&instance.id))
                        .map(|op| OperationRef {
                            id: op.id().clone(),
                            name: op.name().to_owned(),
                        })
                        .collect(),
                    target: None,
                }
            })
            .collect();

        let mut tabs: Vec<TabRow> = Vec::new();
        for instance in &project.instances {
            let profiles = project
                .definitions
                .get(&instance.definition)
                .map_or(0, |d| d.profiles.len());
            let mut on: Vec<(usize, Vec<TabId>)> = Vec::new();
            for tab in project.tabs.iter().filter(|t| t.instance == instance.id) {
                match on.iter_mut().find(|(profile, _)| *profile == tab.profile) {
                    Some((_, ids)) => ids.push(tab.id.clone()),
                    None => on.push((tab.profile, vec![tab.id.clone()])),
                }
            }
            on.sort_by_key(|(profile, _)| *profile);
            tabs.extend(on.into_iter().map(|(profile, ids)| TabRow {
                instance: instance.id.clone(),
                profile,
                name: if profiles > 1 {
                    format!("{} #{}", instance.name, profile.saturating_add(1))
                } else {
                    instance.name.clone()
                },
                tabs: ids,
            }));
        }

        let bitmaps = project
            .bitmaps
            .iter()
            .map(|bitmap| BitmapRow {
                id: bitmap.id.clone(),
                name: bitmap.name.clone(),
                visible: bitmap.visible,
                locked: bitmap.locked,
                width_px: bitmap.width_px,
                height_px: bitmap.height_px,
                operations: project
                    .operations
                    .iter()
                    .filter(|op| matches!(op, Operation::PhotoVCarve(p) if p.image == bitmap.id))
                    .map(|op| OperationRef {
                        id: op.id().clone(),
                        name: op.name().to_owned(),
                    })
                    .collect(),
                target: None,
            })
            .collect();

        Self {
            shapes,
            tabs,
            bitmaps,
        }
    }

    /// The tree with `marks`' targets marked (capability 67): the rows of
    /// the selected operation's targets, of a hovered row's, or of a
    /// target edit's draft, as the Scene holds them
    /// ([`crate::Scene::targets`]); with `None`, no row.
    #[must_use]
    pub fn with_targets(mut self, marks: Option<&TargetMarks>) -> Self {
        for row in &mut self.shapes {
            row.target = marks
                .filter(|m| m.targets.contains(&row.id))
                .map(|m| m.colour);
        }
        // A Photo V-Carve's target is its picture.
        for row in &mut self.bitmaps {
            row.target = marks
                .filter(|m| m.picture.as_ref() == Some(&row.id))
                .map(|m| m.colour);
        }
        self
    }

    /// Every row, top to bottom: each section's header, then its rows,
    /// the bitmaps under the Pictures header. An empty section is its
    /// header alone, with no *None* row (capability 57).
    #[must_use]
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows =
            Vec::with_capacity(3 + self.shapes.len() + self.tabs.len() + self.bitmaps.len());
        rows.push(Row::Header(Section::Shapes));
        rows.extend(self.shapes.iter().map(Row::Shape));
        rows.push(Row::Header(Section::Tabs));
        rows.extend(self.tabs.iter().map(Row::Tab));
        rows.push(Row::Header(Section::Pictures));
        rows.extend(self.bitmaps.iter().map(Row::Bitmap));
        rows
    }

    /// The Shapes header's eye and padlock.
    #[must_use]
    pub fn shapes_header(&self) -> SectionState {
        SectionState {
            visible: common_bool(&self.shapes, |r| Some(r.visible)),
            locked: common_bool(&self.shapes, |r| Some(r.locked)),
        }
    }

    /// The Pictures header's eye and padlock.
    #[must_use]
    pub fn bitmaps_header(&self) -> SectionState {
        SectionState {
            visible: common_bool(&self.bitmaps, |r| Some(r.visible)),
            locked: common_bool(&self.bitmaps, |r| Some(r.locked)),
        }
    }

    /// The row of instance `id`.
    #[must_use]
    pub fn shape(&self, id: &InstanceId) -> Option<&ShapeRow> {
        self.shapes.iter().find(|r| r.id == *id)
    }

    /// The row of bitmap `id`.
    #[must_use]
    pub fn bitmap(&self, id: &BitmapId) -> Option<&BitmapRow> {
        self.bitmaps.iter().find(|r| r.id == *id)
    }

    /// Every instance from `from` to `to` inclusive, in row order: what a
    /// Shift-click selects in the tree (`experience.md`, section 8,
    /// "Selection": "tree: range"). `to` alone when `from` has no row.
    #[must_use]
    pub fn range(&self, from: &InstanceId, to: &InstanceId) -> Vec<InstanceId> {
        let ids: Vec<&InstanceId> = self.shapes.iter().map(|r| &r.id).collect();
        between(&ids, from, to)
    }

    /// Every bitmap from `from` to `to` inclusive, in row order: what a
    /// Shift-click selects among the Pictures rows, as [`Tree::range`]
    /// among the shapes. `to` alone when `from` has no row.
    #[must_use]
    pub fn bitmap_range(&self, from: &BitmapId, to: &BitmapId) -> Vec<BitmapId> {
        let ids: Vec<&BitmapId> = self.bitmaps.iter().map(|r| &r.id).collect();
        between(&ids, from, to)
    }
}

/// The ids of `rows` from `from` to `to` inclusive, in row order; `to`
/// alone when `from` is not among them, and none when `to` is not.
fn between<T: Clone + PartialEq>(rows: &[&T], from: &T, to: &T) -> Vec<T> {
    let at = |id: &T| rows.iter().position(|r| *r == id);
    match (at(from), at(to)) {
        (Some(a), Some(b)) => {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            rows.get(lo..=hi)
                .unwrap_or_default()
                .iter()
                .map(|r| (*r).clone())
                .collect()
        }
        (None, Some(_)) => vec![to.clone()],
        _ => Vec::new(),
    }
}

/// The context menu's items (capability 57).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    /// Rename the row the menu opened on, in place: view state until the
    /// typed name commits as [`rename`]'s command.
    Rename,
    /// Hide the rows.
    Hide,
    /// Show the rows.
    Show,
    /// Lock the rows.
    Lock,
    /// Unlock the rows.
    Unlock,
    /// **Cut** (phase 5, capability 98): the rows to the clipboard and
    /// out of the project ([`crate::App::cut_selection`]).
    Cut,
    /// **Copy**: the rows to the clipboard ([`crate::App::copy_selection`]).
    Copy,
    /// **Paste**: the clipboard's shapes ([`crate::App::paste`]); listed
    /// while it holds any.
    Paste,
    /// Linked copies (capability 59).
    Duplicate,
    /// Delete, with the targets and tabs that go with it (capability 59).
    Delete,
    /// **Make Unique** (capability 98): listed while a row is a linked
    /// copy.
    MakeUnique,
    /// **Convert to Path** (capability 84): listed while a row keeps its
    /// sizes.
    ConvertToPath,
    /// Frame the rows on the Viewport: view state, no command.
    ZoomTo,
    /// *Add operation ▸*: the Operations step's Add menu for the rows
    /// (`crate::operations::add_menu`).
    AddOperation,
    /// *Add to "Panel"*: the rows added to the selected operation's
    /// targets ([`target_command`]).
    AddTo,
    /// *Remove from "Panel"*: the rows taken out of the selected
    /// operation's targets, which deletes nothing.
    RemoveFrom,
}

/// Whether every one of `ids` is visible, and whether every one is locked.
fn all(project: &Project, ids: &[InstanceId]) -> (bool, bool) {
    let chosen = || project.instances.iter().filter(|i| ids.contains(&i.id));
    (chosen().all(|i| i.visible), chosen().all(|i| i.locked))
}

/// The menu for `ids`, as the shell lists it: Rename, Hide or Show (Show
/// when any is hidden), Lock or Unlock (Unlock when every one is locked),
/// Cut, Copy, Paste while `can_paste`, Duplicate, Delete, Make Unique
/// while one of `ids` is a linked copy, Convert to Path while one keeps
/// its sizes, Zoom to, Add operation, and, while operation `op` is
/// selected, *Add to* it when it lacks one of `ids` and *Remove from* it
/// when it targets one (capabilities 57, 84 and 98).
#[must_use]
pub fn menu(
    project: &Project,
    ids: &[InstanceId],
    op: Option<&Operation>,
    can_paste: bool,
) -> Vec<MenuItem> {
    let (visible, locked) = all(project, ids);
    let chosen = || project.instances.iter().filter(|i| ids.contains(&i.id));
    let linked = chosen().any(|instance| {
        project
            .instances
            .iter()
            .any(|other| other.id != instance.id && other.definition == instance.definition)
    });
    let sized = chosen().any(|instance| {
        project
            .definitions
            .get(&instance.definition)
            .and_then(|definition| definition.shape.as_ref())
            .is_some_and(crate::command::keeps_sizes)
    });
    let mut items = vec![
        MenuItem::Rename,
        if visible {
            MenuItem::Hide
        } else {
            MenuItem::Show
        },
        if locked {
            MenuItem::Unlock
        } else {
            MenuItem::Lock
        },
        MenuItem::Cut,
        MenuItem::Copy,
    ];
    if can_paste {
        items.push(MenuItem::Paste);
    }
    items.extend([MenuItem::Duplicate, MenuItem::Delete]);
    if linked {
        items.push(MenuItem::MakeUnique);
    }
    if sized {
        items.push(MenuItem::ConvertToPath);
    }
    items.extend([MenuItem::ZoomTo, MenuItem::AddOperation]);
    if let Some(op) = op {
        if ids.iter().any(|id| !op.targets().contains(id)) {
            items.push(MenuItem::AddTo);
        }
        if ids.iter().any(|id| op.targets().contains(id)) {
            items.push(MenuItem::RemoveFrom);
        }
    }
    items
}

/// The command *Add to* or *Remove from* runs on `ids` for the selected
/// operation `op`: one `UpdateOperation`, labelled *Set targets of
/// "Panel"*; `None` for any other item, or when nothing would change.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn target_command(
    op: &Operation,
    item: MenuItem,
    ids: &[InstanceId],
) -> Result<Option<Command>, CommandError> {
    match item {
        MenuItem::AddTo => targets::add(op, ids),
        MenuItem::RemoveFrom => targets::remove(op, ids),
        _ => Ok(None),
    }
}

/// The command `item` runs on `ids`; `None` for the items that are not a
/// change to the project (Rename, Zoom to, Add operation), for the two
/// that change an operation ([`target_command`]), or with nothing chosen.
#[must_use]
pub fn menu_command(project: &Project, item: MenuItem, ids: &[InstanceId]) -> Option<Command> {
    if ids.is_empty()
        || !ids
            .iter()
            .all(|id| project.instances.iter().any(|i| i.id == *id))
    {
        return None;
    }
    let ids = ids.to_vec();
    let flags = |visible, locked| Command::SetInstanceFlags {
        ids: ids.clone(),
        visible,
        locked,
    };
    match item {
        MenuItem::Hide => Some(flags(Some(false), None)),
        MenuItem::Show => Some(flags(Some(true), None)),
        MenuItem::Lock => Some(flags(None, Some(true))),
        MenuItem::Unlock => Some(flags(None, Some(false))),
        MenuItem::Duplicate => Some(Command::DuplicateInstances(ids)),
        MenuItem::Delete => Some(Command::DeleteInstances(ids)),
        MenuItem::MakeUnique => Some(Command::MakeUnique(ids)),
        MenuItem::ConvertToPath => Some(Command::ConvertToPath(ids)),
        MenuItem::Rename
        | MenuItem::Cut
        | MenuItem::Copy
        | MenuItem::Paste
        | MenuItem::ZoomTo
        | MenuItem::AddOperation
        | MenuItem::AddTo
        | MenuItem::RemoveFrom => None,
    }
}

/// The eye of a row or a section header clicked: hide `ids` when every one
/// is shown, else show them all.
#[must_use]
pub fn toggle_visible(project: &Project, ids: &[InstanceId]) -> Option<Command> {
    let item = if all(project, ids).0 {
        MenuItem::Hide
    } else {
        MenuItem::Show
    };
    menu_command(project, item, ids)
}

/// The padlock of a row or a section header clicked: unlock `ids` when
/// every one is locked, else lock them all.
#[must_use]
pub fn toggle_locked(project: &Project, ids: &[InstanceId]) -> Option<Command> {
    let item = if all(project, ids).1 {
        MenuItem::Unlock
    } else {
        MenuItem::Lock
    };
    menu_command(project, item, ids)
}

/// Whether every one of the bitmaps `ids` is visible, and whether every
/// one is locked.
fn all_bitmaps(project: &Project, ids: &[BitmapId]) -> (bool, bool) {
    let chosen = || project.bitmaps.iter().filter(|b| ids.contains(&b.id));
    (chosen().all(|b| b.visible), chosen().all(|b| b.locked))
}

/// The context menu of the Pictures rows `ids` (capabilities 56 and 57):
/// Rename, Hide or Show (Show when any is hidden), Lock or Unlock (Unlock
/// when every one is locked), Delete, Zoom to, and *Add operation ▸*,
/// whose menu for the rows is [`crate::operations::add_menu`]: a
/// Photo V-Carve on the picture (slice 15, #544). A picture has no linked
/// copy, and its Photo V-Carve names it by the editor's Picture row, so a
/// shape's Duplicate, *Add to* and *Remove from* are absent.
#[must_use]
pub fn bitmap_menu(project: &Project, ids: &[BitmapId]) -> Vec<MenuItem> {
    let (visible, locked) = all_bitmaps(project, ids);
    vec![
        MenuItem::Rename,
        if visible {
            MenuItem::Hide
        } else {
            MenuItem::Show
        },
        if locked {
            MenuItem::Unlock
        } else {
            MenuItem::Lock
        },
        MenuItem::Delete,
        MenuItem::ZoomTo,
        MenuItem::AddOperation,
    ]
}

/// The command `item` runs on the bitmaps `ids`; `None` for the items
/// that are not a change to the project (Rename, which commits through
/// [`rename_bitmap`], Zoom to, and Add operation, a submenu whose items
/// run [`crate::operations::add`]), for an item [`bitmap_menu`] does not
/// offer, with nothing chosen, or when the project lacks one of `ids`.
#[must_use]
pub fn bitmap_menu_command(project: &Project, item: MenuItem, ids: &[BitmapId]) -> Option<Command> {
    if ids.is_empty()
        || !ids
            .iter()
            .all(|id| project.bitmaps.iter().any(|b| b.id == *id))
    {
        return None;
    }
    let ids = ids.to_vec();
    let flags = |visible, locked| Command::SetBitmapFlags {
        ids: ids.clone(),
        visible,
        locked,
    };
    match item {
        MenuItem::Hide => Some(flags(Some(false), None)),
        MenuItem::Show => Some(flags(Some(true), None)),
        MenuItem::Lock => Some(flags(None, Some(true))),
        MenuItem::Unlock => Some(flags(None, Some(false))),
        MenuItem::Delete => Some(Command::DeleteBitmaps(ids)),
        MenuItem::Rename
        | MenuItem::ZoomTo
        | MenuItem::Cut
        | MenuItem::Copy
        | MenuItem::Paste
        | MenuItem::MakeUnique
        | MenuItem::ConvertToPath
        | MenuItem::Duplicate
        | MenuItem::AddOperation
        | MenuItem::AddTo
        | MenuItem::RemoveFrom => None,
    }
}

/// The eye of a Pictures row or of the section's header clicked: hide
/// `ids` when every one is shown, else show them all.
#[must_use]
pub fn toggle_bitmap_visible(project: &Project, ids: &[BitmapId]) -> Option<Command> {
    let item = if all_bitmaps(project, ids).0 {
        MenuItem::Hide
    } else {
        MenuItem::Show
    };
    bitmap_menu_command(project, item, ids)
}

/// The padlock of a Pictures row or of the section's header clicked:
/// unlock `ids` when every one is locked, else lock them all.
#[must_use]
pub fn toggle_bitmap_locked(project: &Project, ids: &[BitmapId]) -> Option<Command> {
    let item = if all_bitmaps(project, ids).1 {
        MenuItem::Unlock
    } else {
        MenuItem::Lock
    };
    bitmap_menu_command(project, item, ids)
}

/// An inline rename of a Pictures row committed: the typed name, trimmed,
/// as one step; `None` when it is empty or unchanged.
#[must_use]
pub fn rename_bitmap(project: &Project, id: &BitmapId, typed: &str) -> Option<Command> {
    let name = typed.trim();
    let bitmap = project.bitmaps.iter().find(|b| b.id == *id)?;
    (!name.is_empty() && name != bitmap.name).then(|| Command::RenameBitmap {
        id: id.clone(),
        name: name.to_owned(),
    })
}

/// An inline rename committed (`experience.md`, section 8): the typed
/// name, trimmed, as one step; `None` when it is empty or unchanged, so
/// leaving the field untouched is no step.
#[must_use]
pub fn rename(project: &Project, id: &InstanceId, typed: &str) -> Option<Command> {
    let name = typed.trim();
    let instance = project.instances.iter().find(|i| i.id == *id)?;
    (!name.is_empty() && name != instance.name).then(|| Command::RenameInstance {
        id: id.clone(),
        name: name.to_owned(),
    })
}
