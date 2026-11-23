// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The selection (`docs/specs/phase-4/design.md`, "Data model → dc-app"):
//! instances, tabs, or one operation. View state, not project state
//! (functional spec, capability 52): selecting is never an undo step. When
//! a snapshot drops what was selected, the selection drops it too.
//!
//! Pictures are selected too (slice 15, #544; capability 57), alone, as
//! tabs are: [`Selection::Bitmaps`], read through
//! [`crate::App::selected_pictures`] and changed by a click
//! ([`Selection::clicked`] on a [`Hit::Bitmap`], [`pictures_clicked`]) or
//! a marquee ([`Selection::boxed_pictures`]).

use dc_project::{BitmapId, InstanceId, OperationId, Project, TabId};

use crate::viewport::pick::Hit;

/// How a click on the Viewport changes the selection (functional spec,
/// capability 75; `experience.md`, section 8, "Selection").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    /// A plain click: the hit alone, or nothing.
    Replace,
    /// Shift-click: the hit added.
    Add,
    /// Ctrl/⌘-click: the hit in or out.
    Toggle,
}

/// `selected` after a click on `id`: it alone, it added, or it in or out,
/// in selection order.
fn clicked_among<T: Clone + PartialEq>(selected: &[T], id: &T, mode: ClickMode) -> Vec<T> {
    let mut ids = selected.to_vec();
    match mode {
        ClickMode::Replace => ids = vec![id.clone()],
        ClickMode::Add => {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ClickMode::Toggle => {
            if ids.contains(id) {
                ids.retain(|i| i != id);
            } else {
                ids.push(id.clone());
            }
        }
    }
    ids
}

/// The selected pictures after a click on picture `id`, on its row or on
/// the Viewport (capability 57): it alone, added with Shift, or in or out
/// with Ctrl/⌘, as [`Selection::clicked`] treats a shape.
#[must_use]
pub fn pictures_clicked(selected: &[BitmapId], id: &BitmapId, mode: ClickMode) -> Vec<BitmapId> {
    clicked_among(selected, id, mode)
}

/// What the panels and the Viewport act on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Selection {
    /// Nothing.
    #[default]
    Nothing,
    /// One or more instances, in selection order.
    Instances(Vec<InstanceId>),
    /// One or more placed tabs.
    Tabs(Vec<TabId>),
    /// One operation (the Operations list's row).
    Operation(OperationId),
    /// One or more pictures, in selection order (capability 57).
    Bitmaps(Vec<BitmapId>),
}

impl Selection {
    /// The selected instances, or none.
    #[must_use]
    pub fn instances(&self) -> &[InstanceId] {
        match self {
            Self::Instances(ids) => ids,
            _ => &[],
        }
    }

    /// The selected pictures, or none.
    #[must_use]
    pub fn bitmaps(&self) -> &[BitmapId] {
        match self {
            Self::Bitmaps(ids) => ids,
            _ => &[],
        }
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Nothing => true,
            Self::Instances(ids) => ids.is_empty(),
            Self::Tabs(ids) => ids.is_empty(),
            Self::Operation(_) => false,
            Self::Bitmaps(ids) => ids.is_empty(),
        }
    }

    /// A selection of the pictures `ids`, or nothing when there are none.
    #[must_use]
    pub fn of_bitmaps(ids: Vec<BitmapId>) -> Self {
        if ids.is_empty() {
            Self::Nothing
        } else {
            Self::Bitmaps(ids)
        }
    }

    /// The selection after a click that hit `hit`. A plain click on
    /// nothing clears; Shift or Ctrl on nothing keeps what there is. Adding
    /// or toggling an instance to a selection of tabs or an operation
    /// starts an instance selection. A shape on the male inlay board
    /// selects its Inlay, whatever the modifier (capability 72). A
    /// picture is selected among pictures, as a tab among tabs
    /// (capability 57).
    #[must_use]
    pub fn clicked(&self, hit: &Hit, mode: ClickMode) -> Self {
        if let Hit::MaleBoardShape(op) = hit {
            return Self::Operation(op.clone());
        }
        if let Hit::Tab(tab) = hit {
            return self.tab_clicked(tab, mode);
        }
        if let Hit::Bitmap(id) = hit {
            return Self::of_bitmaps(pictures_clicked(self.bitmaps(), id, mode));
        }
        // A toolpath opens its operation in the editor (`App::edit`);
        // the shapes stay selected (#576).
        if let Hit::Toolpath(_) = hit {
            return self.clone();
        }
        let Hit::Instance(id) = hit else {
            return match mode {
                ClickMode::Replace => Self::Nothing,
                ClickMode::Add | ClickMode::Toggle => self.clone(),
            };
        };
        let ids = clicked_among(self.instances(), id, mode);
        if ids.is_empty() {
            Self::Nothing
        } else {
            Self::Instances(ids)
        }
    }

    /// [`Selection::clicked`] on the Viewport, where a click on a shape
    /// of a group is a click on the group (phase 5, capability 99;
    /// CONTEXT: Group): its shapes are selected alone, added, or, with a
    /// toggle, taken out when every one was selected and added when not.
    #[must_use]
    pub fn clicked_in(&self, project: &Project, hit: &Hit, mode: ClickMode) -> Self {
        let Hit::Instance(id) = hit else {
            return self.clicked(hit, mode);
        };
        let members = crate::tree::with_groups(project, std::slice::from_ref(id));
        if members.len() < 2 {
            return self.clicked(hit, mode);
        }
        let held = self.instances();
        let ids: Vec<InstanceId> = match mode {
            ClickMode::Replace => members,
            ClickMode::Toggle if members.iter().all(|member| held.contains(member)) => held
                .iter()
                .filter(|id| !members.contains(id))
                .cloned()
                .collect(),
            ClickMode::Add | ClickMode::Toggle => {
                let mut ids = held.to_vec();
                for member in members {
                    if !ids.contains(&member) {
                        ids.push(member);
                    }
                }
                ids
            }
        };
        if ids.is_empty() {
            Self::Nothing
        } else {
            Self::Instances(ids)
        }
    }

    /// [`Selection::boxed`] on the Viewport: a marquee that touches a
    /// shape of a group takes the group.
    #[must_use]
    pub fn boxed_in(&self, project: &Project, ids: &[InstanceId], mode: ClickMode) -> Self {
        self.boxed(&crate::tree::with_groups(project, ids), mode)
    }

    /// The selected placed tabs, or none.
    #[must_use]
    pub fn tabs(&self) -> &[TabId] {
        match self {
            Self::Tabs(ids) => ids,
            _ => &[],
        }
    }

    /// The selection after a click on placed tab `tab` (slice 17): it
    /// alone, added, or toggled, as an instance is. Adding or toggling a
    /// tab to a selection of anything else starts a tab selection.
    fn tab_clicked(&self, tab: &TabId, mode: ClickMode) -> Self {
        let ids = clicked_among(self.tabs(), tab, mode);
        if ids.is_empty() {
            Self::Nothing
        } else {
            Self::Tabs(ids)
        }
    }

    /// The selection after a marquee took `ids`: those alone, those added,
    /// or each toggled, as [`Selection::clicked`] treats one.
    #[must_use]
    pub fn boxed(&self, ids: &[InstanceId], mode: ClickMode) -> Self {
        match mode {
            ClickMode::Replace if ids.is_empty() => Self::Nothing,
            ClickMode::Replace => Self::Instances(ids.to_vec()),
            ClickMode::Add | ClickMode::Toggle => ids.iter().fold(self.clone(), |sel, id| {
                sel.clicked(&Hit::Instance(id.clone()), mode)
            }),
        }
    }

    /// The selection after a marquee that touched no shape took the
    /// pictures `ids` (capability 57: pictures are selected on their own,
    /// so a marquee selects them only where it touches no shape): those
    /// alone, those added, or each toggled.
    #[must_use]
    pub fn boxed_pictures(&self, ids: &[BitmapId], mode: ClickMode) -> Self {
        match mode {
            ClickMode::Replace => Self::of_bitmaps(ids.to_vec()),
            ClickMode::Add | ClickMode::Toggle => ids.iter().fold(self.clone(), |sel, id| {
                sel.clicked(&Hit::Bitmap(id.clone()), mode)
            }),
        }
    }

    /// Drops whatever `project` no longer has; an emptied selection is
    /// [`Selection::Nothing`].
    pub(crate) fn retain_existing(&mut self, project: &Project) {
        match self {
            Self::Nothing => {}
            Self::Instances(ids) => {
                ids.retain(|id| project.instances.iter().any(|i| i.id == *id));
            }
            Self::Tabs(ids) => ids.retain(|id| project.tabs.iter().any(|t| t.id == *id)),
            Self::Operation(id) => {
                if !project.operations.iter().any(|o| o.id() == id) {
                    *self = Self::Nothing;
                }
            }
            Self::Bitmaps(ids) => {
                ids.retain(|id| project.bitmaps.iter().any(|b| b.id == *id));
            }
        }
        if self.is_empty() {
            *self = Self::Nothing;
        }
    }
}
