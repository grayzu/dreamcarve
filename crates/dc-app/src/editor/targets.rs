// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Targets group (functional spec, capability 67; mock-up
//! `05-operation-profile.svg`): the operation's target shapes by name, each
//! with a remove button; *Add selected*, *Replace with selected*, and
//! *Pick…*, whose clicks on the Viewport add one shape each. Each is one
//! `UpdateOperation` of the whole record ([`super::fields::set`] on
//! `targets`). A shape the kind cannot cut is accepted, as a hand-edited
//! file's is, and carries the warning `check` gives it.

use dc_project::{InstanceId, Operation, Project};
use serde_json::Value;

use super::fields;
use crate::command::Command;
use crate::error::CommandError;

/// One target as the group lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The instance.
    pub id: InstanceId,
    /// Its name, or its id when the project lacks it (validation says so).
    pub name: String,
}

/// `op`'s targets by name, in the record's order.
#[must_use]
pub fn list(project: &Project, op: &Operation) -> Vec<Target> {
    op.targets()
        .iter()
        .map(|id| Target {
            id: id.clone(),
            name: project
                .instances
                .iter()
                .find(|i| i.id == *id)
                .map_or_else(|| id.as_str().to_owned(), |i| i.name.clone()),
        })
        .collect()
}

/// `op` targeting `ids`, or `None` when it does already.
fn targeting(op: &Operation, ids: Vec<InstanceId>) -> Result<Option<Command>, CommandError> {
    if op.targets() == ids.as_slice() {
        return Ok(None);
    }
    let value = Value::Array(ids.iter().map(|i| Value::from(i.as_str())).collect());
    fields::set(op, "targets", value).map(|next| Some(Command::UpdateOperation(next)))
}

/// *Add selected* (and each *Pick…* click): `ids` appended where `op`
/// lacks them; `None` when it has them all.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn add(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    let mut targets = op.targets().to_vec();
    for id in ids {
        if !targets.contains(id) {
            targets.push(id.clone());
        }
    }
    targeting(op, targets)
}

/// *Replace with selected*: `ids` alone; `None` when they are the
/// targets already, or nothing is selected.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn replace(op: &Operation, ids: &[InstanceId]) -> Result<Option<Command>, CommandError> {
    if ids.is_empty() {
        return Ok(None);
    }
    let mut targets: Vec<InstanceId> = Vec::with_capacity(ids.len());
    for id in ids {
        if !targets.contains(id) {
            targets.push(id.clone());
        }
    }
    targeting(op, targets)
}

/// A target's remove button.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if the file refuses the record.
pub fn remove(op: &Operation, id: &InstanceId) -> Result<Option<Command>, CommandError> {
    let targets = op.targets().iter().filter(|t| *t != id).cloned().collect();
    targeting(op, targets)
}
