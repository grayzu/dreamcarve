// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The selection (`docs/specs/phase-4/design.md`, "Data model → dc-app"):
//! instances, tabs, or one operation. View state, not project state
//! (functional spec, capability 52): selecting is never an undo step. When
//! a snapshot drops what was selected, the selection drops it too.

use dc_project::{InstanceId, OperationId, Project, TabId};

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

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Nothing => true,
            Self::Instances(ids) => ids.is_empty(),
            Self::Tabs(ids) => ids.is_empty(),
            Self::Operation(_) => false,
        }
    }

    /// The selection after a click that hit `hit`. A plain click on
    /// nothing clears; Shift or Ctrl on nothing keeps what there is. Adding
    /// or toggling an instance to a selection of tabs or an operation
    /// starts an instance selection. A shape on the male inlay board
    /// selects its Inlay, whatever the modifier (capability 72).
    #[must_use]
    pub fn clicked(&self, hit: &Hit, mode: ClickMode) -> Self {
        if let Hit::MaleBoardShape(op) = hit {
            return Self::Operation(op.clone());
        }
        let Hit::Instance(id) = hit else {
            return match mode {
                ClickMode::Replace => Self::Nothing,
                ClickMode::Add | ClickMode::Toggle => self.clone(),
            };
        };
        let mut ids = self.instances().to_vec();
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
        if ids.is_empty() {
            Self::Nothing
        } else {
            Self::Instances(ids)
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
        }
        if self.is_empty() {
            *self = Self::Nothing;
        }
    }
}
