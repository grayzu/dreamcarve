// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! An operation's targets (functional spec, capability 67, as the review
//! reworked it; mock-up `15-targets.svg`; slice 27, #619). Targets are
//! shown on the drawing, not listed in the editor: Setup's Targets row is
//! one line ([`row_summary`]: the count, the names, a warning flag), and
//! the targets change in **target editing** ([`Editing`]), a mode on the
//! drawing that holds a draft: a click toggles a shape, a marquee adds
//! what it touches, an Alt-marquee removes, *Done* writes the draft as
//! one undo step and *Cancel* drops it. The row's menu has *Use
//! selection* ([`replace`]) and *Add selection* ([`add`]); the Design
//! Tree's context menu adds and removes ([`add`], [`remove`]).
//!
//! Every change is one `UpdateOperation` of the whole record
//! ([`super::fields::set`] on `targets`), labelled *Set targets of
//! "Panel"*. A shape the kind cannot cut is accepted, as a hand-edited
//! file's is, and carries the warning `check` gives it.

use dc_job::Tier;
use dc_project::{InstanceId, Operation, OperationId, Project};
use serde_json::Value;

use super::fields;
use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;
use crate::strings::StrKey;

/// The names of `ids`, in order; an id the project lacks stands for
/// itself (validation says so).
fn named(project: &Project, ids: &[InstanceId]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            project
                .instances
                .iter()
                .find(|i| i.id == *id)
                .map_or_else(|| id.as_str().to_owned(), |i| i.name.clone())
        })
        .collect()
}

/// Setup's Targets row (capability 67): one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSummary {
    /// The count and the names, `2 shapes · pocket, island`; the shell
    /// truncates it at the row's end.
    pub text: String,
    /// Every name, for the row's tooltip.
    pub names: String,
    /// A target carries an error or a warning (an open path under a
    /// Pocket, a non-circle under a Drill): the row shows the warning
    /// glyph, and the message is under it.
    pub warning: bool,
}

/// The Targets row of `op` in `app`: its targets, or, while they are
/// being edited, the draft, so the row follows each click.
#[must_use]
pub fn row_summary(app: &App, op: &Operation) -> RowSummary {
    let project = app.project();
    let strings = app.strings();
    let ids = app
        .target_editing()
        .filter(|editing| editing.op() == op.id())
        .map_or_else(|| op.targets(), Editing::targets);
    let names = named(project, ids).join(", ");
    let text = match ids.len() {
        0 => strings.get(StrKey::EditorTargetsNone).to_owned(),
        1 => strings.fmt(StrKey::EditorTargetsOne, &[("names", &names)]),
        n => strings.fmt(
            StrKey::EditorTargetsMany,
            &[("count", &n.to_string()), ("names", &names)],
        ),
    };
    let warning = fields::rows(op)
        .into_iter()
        .find(|row| row.key == "targets")
        .and_then(|row| super::Located::of(app.warnings(), op).status(row))
        .is_some_and(|status| status.tier != Tier::Hint);
    RowSummary {
        text,
        names,
        warning,
    }
}

/// `op` targeting `ids`, or `None` when it does already.
fn targeting(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    if op.targets() == ids {
        return Ok(None);
    }
    let value = Value::Array(ids.iter().map(|i| Value::from(i.as_str())).collect());
    fields::set(op, "targets", value).map(|next| Some(Command::UpdateOperation(next)))
}

/// `targets` with each of `ids` it lacks appended, in `ids`' order.
fn with(mut targets: Vec<InstanceId>, ids: &[InstanceId]) -> Vec<InstanceId> {
    for id in ids {
        if !targets.contains(id) {
            targets.push(id.clone());
        }
    }
    targets
}

/// *Add selection*, and the Design Tree's *Add to "Panel"*: `ids`
/// appended where `op` lacks them; `None` when it has them all.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn add(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    targeting(op, &with(op.targets().to_vec(), ids))
}

/// *Use selection*: `ids` alone; `None` when they are the targets
/// already, or nothing is selected.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn replace(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    if ids.is_empty() {
        return Ok(None);
    }
    targeting(op, &with(Vec::with_capacity(ids.len()), ids))
}

/// The Design Tree's *Remove from "Panel"*: `op` without `ids`; `None`
/// when it targets none of them.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn remove(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    let targets: Vec<InstanceId> = op
        .targets()
        .iter()
        .filter(|t| !ids.contains(t))
        .cloned()
        .collect();
    targeting(op, &targets)
}

/// Target editing (capability 67): the draft of one operation's targets,
/// from *Edit* to *Done* or *Cancel*. Nothing is written while it stands;
/// [`Editing::command`] is the one step *Done* runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    op: OperationId,
    targets: Vec<InstanceId>,
}

impl Editing {
    /// A draft of `op`'s targets as they are.
    #[must_use]
    pub fn of(op: &Operation) -> Self {
        Self {
            op: op.id().clone(),
            targets: op.targets().to_vec(),
        }
    }

    /// The operation whose targets are being edited.
    #[must_use]
    pub fn op(&self) -> &OperationId {
        &self.op
    }

    /// The draft: the targets as *Done* would write them.
    #[must_use]
    pub fn targets(&self) -> &[InstanceId] {
        &self.targets
    }

    /// A click on shape `id` (on the Viewport or its Design Tree row): in
    /// when it was out, at the end; out when it was in.
    pub fn toggle(&mut self, id: &InstanceId) {
        if self.targets.contains(id) {
            self.targets.retain(|t| t != id);
        } else {
            self.targets.push(id.clone());
        }
    }

    /// A marquee over `ids`: each added once, after the targets there
    /// are; with Alt held (`remove`), each taken out.
    pub fn boxed(&mut self, ids: &[InstanceId], remove: bool) {
        if remove {
            self.targets.retain(|t| !ids.contains(t));
        } else {
            self.targets = with(std::mem::take(&mut self.targets), ids);
        }
    }

    /// *Done*: the operation targeting the draft, as one
    /// `UpdateOperation`; `None` when the draft is what it targets, or
    /// the project no longer has it.
    ///
    /// # Errors
    ///
    /// [`CommandError::FieldRefused`] if the file refuses the record.
    pub fn command(&self, project: &Project) -> Result<Option<Command>, CommandError> {
        match project.operations.iter().find(|o| *o.id() == self.op) {
            Some(op) => targeting(op, &self.targets),
            None => Ok(None),
        }
    }
}
