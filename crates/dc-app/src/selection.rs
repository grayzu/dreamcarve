// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The selection (`docs/specs/phase-4/design.md`, "Data model → dc-app"):
//! instances, tabs, or one operation. View state, not project state
//! (functional spec, capability 52): selecting is never an undo step. When
//! a snapshot drops what was selected, the selection drops it too.

use dc_project::{InstanceId, OperationId, Project, TabId};

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
