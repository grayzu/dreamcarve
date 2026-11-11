// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Design Tree's rows (`docs/specs/phase-4/design.md`, "The Design Tree
//! and placement"; functional spec, capabilities 57 and 59;
//! `experience.md`, sections 6 to 8). Built from the snapshot and nothing
//! else: the instances in file order, each with its eye, padlock, shape
//! glyph (a closed outline, a circle or an open path: never a word, slice
//! 25, #617), *linked* badge and the operations targeting it; then the
//! placed tabs grouped by the outline they sit on; then the pictures, none
//! until phase 3.5. An empty section is its header alone ([`Tree::lines`]).
//! The shell draws the rows and turns a click, a menu item or a rename
//! into the [`Command`] these functions return; the selection is the
//! app's one [`Selection`](crate::Selection), shared with the Viewport.
//!
//! Behaviour read from PureCutCNC's `FeatureTree.tsx` (rows with a
//! visibility eye and a lock, a context menu, double-click to rename, a
//! section header acting on its rows); its folders, regions and
//! construction rows have no counterpart here.

use dc_geometry::{Transform, as_circle};
use dc_project::{FeatureDefinition, InstanceId, OperationId, Project, TabId};

use crate::command::Command;
use crate::mixed::common_bool;

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

/// A bitmap's row. Phase 3.5 brings the bitmap type; until then the
/// Pictures section has no rows, and this type has no values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureRow {}

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

/// One line of the tree as the shell draws it, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    /// A section's header, drawn whether or not it has rows.
    Header(Section),
    /// An instance's row.
    Shape(&'a ShapeRow),
    /// An outline's placed tabs.
    Tab(&'a TabRow),
    /// A bitmap's row.
    Picture(&'a PictureRow),
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
    /// The Pictures section.
    pub pictures: Vec<PictureRow>,
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

        Self {
            shapes,
            tabs,
            pictures: Vec::new(),
        }
    }

    /// Every line, top to bottom: each section's header, then its rows.
    /// An empty section is its header alone, with no *None* row
    /// (capability 57).
    #[must_use]
    pub fn lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::with_capacity(3 + self.shapes.len() + self.tabs.len());
        lines.push(Line::Header(Section::Shapes));
        lines.extend(self.shapes.iter().map(Line::Shape));
        lines.push(Line::Header(Section::Tabs));
        lines.extend(self.tabs.iter().map(Line::Tab));
        lines.push(Line::Header(Section::Pictures));
        lines.extend(self.pictures.iter().map(Line::Picture));
        lines
    }

    /// The Shapes header's eye and padlock.
    #[must_use]
    pub fn shapes_header(&self) -> SectionState {
        SectionState {
            visible: common_bool(&self.shapes, |r| Some(r.visible)),
            locked: common_bool(&self.shapes, |r| Some(r.locked)),
        }
    }

    /// The row of instance `id`.
    #[must_use]
    pub fn shape(&self, id: &InstanceId) -> Option<&ShapeRow> {
        self.shapes.iter().find(|r| r.id == *id)
    }

    /// Every instance from `from` to `to` inclusive, in row order: what a
    /// Shift-click selects in the tree (`experience.md`, section 8,
    /// "Selection": "tree: range"). `to` alone when `from` has no row.
    #[must_use]
    pub fn range(&self, from: &InstanceId, to: &InstanceId) -> Vec<InstanceId> {
        let at = |id: &InstanceId| self.shapes.iter().position(|r| r.id == *id);
        match (at(from), at(to)) {
            (Some(a), Some(b)) => {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.shapes
                    .get(lo..=hi)
                    .unwrap_or_default()
                    .iter()
                    .map(|r| r.id.clone())
                    .collect()
            }
            (None, Some(_)) => vec![to.clone()],
            _ => Vec::new(),
        }
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
    /// Linked copies (capability 59).
    Duplicate,
    /// Delete, with the targets and tabs that go with it (capability 59).
    Delete,
    /// Frame the rows on the Viewport: view state, no command.
    ZoomTo,
    /// *Add operation ▸*: the Operations tab's Add menu for the rows
    /// (`crate::operations::add_menu`).
    AddOperation,
}

/// Whether every one of `ids` is visible, and whether every one is locked.
fn all(project: &Project, ids: &[InstanceId]) -> (bool, bool) {
    let chosen = || project.instances.iter().filter(|i| ids.contains(&i.id));
    (chosen().all(|i| i.visible), chosen().all(|i| i.locked))
}

/// The menu for `ids`, as the shell lists it: Rename, Hide or Show (Show
/// when any is hidden), Lock or Unlock (Unlock when every one is locked),
/// Duplicate, Delete, Zoom to, Add operation.
#[must_use]
pub fn menu(project: &Project, ids: &[InstanceId]) -> [MenuItem; 7] {
    let (visible, locked) = all(project, ids);
    [
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
        MenuItem::Duplicate,
        MenuItem::Delete,
        MenuItem::ZoomTo,
        MenuItem::AddOperation,
    ]
}

/// The command `item` runs on `ids`; `None` for the items that are not a
/// change to the project (Rename, Zoom to, Add operation) or with nothing
/// chosen.
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
        MenuItem::Rename | MenuItem::ZoomTo | MenuItem::AddOperation => None,
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
