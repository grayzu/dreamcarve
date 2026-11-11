// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operation and tool commands (functional spec, capabilities 63 and
//! 65): finding, reordering and duplicating operations, the fields every
//! operation kind shares, which tool an operation uses, and the field an
//! edit changed, for its label.

use dc_project::{InstanceId, Operation, OperationId, Project, ToolId};

use crate::error::CommandError;
use crate::strings::{StrKey, Strings};

/// Runs `$body` with `$inner` bound to whichever operation record `$op`
/// holds. Every kind has the fields the body reads.
macro_rules! with_record {
    ($op:expr, |$inner:ident| $body:expr) => {
        match $op {
            Operation::Profile($inner) => $body,
            Operation::Pocket($inner) => $body,
            Operation::Drill($inner) => $body,
            Operation::Bore($inner) => $body,
            Operation::Engrave($inner) => $body,
            Operation::VCarve($inner) => $body,
            Operation::Inlay($inner) => $body,
            Operation::PhotoVCarve($inner) => $body,
        }
    };
}

/// Where operation `id` sits in `project`'s list.
pub(super) fn index_of(project: &Project, id: &OperationId) -> Result<usize, CommandError> {
    project
        .operations
        .iter()
        .position(|o| o.id() == id)
        .ok_or_else(|| CommandError::UnknownOperation { id: id.clone() })
}

/// The operation's `enabled` flag, to change.
pub(super) fn enabled_mut(operation: &mut Operation) -> &mut bool {
    with_record!(operation, |op| &mut op.enabled)
}

/// The operation's targets, to change; `None` for a Photo V-Carve, which
/// carves a bitmap and targets no instance (format 27).
pub(super) fn targets_mut(operation: &mut Operation) -> Option<&mut Vec<InstanceId>> {
    match operation {
        Operation::Profile(op) => Some(&mut op.targets),
        Operation::Pocket(op) => Some(&mut op.targets),
        Operation::Drill(op) => Some(&mut op.targets),
        Operation::Bore(op) => Some(&mut op.targets),
        Operation::Engrave(op) => Some(&mut op.targets),
        Operation::VCarve(op) => Some(&mut op.targets),
        Operation::Inlay(op) => Some(&mut op.targets),
        Operation::PhotoVCarve(_) => None,
    }
}

/// Gives the operation another id and name, for a copy.
fn rename(operation: &mut Operation, id: OperationId, name: String) {
    with_record!(operation, |op| {
        op.id = id;
        op.name = name;
    });
}

/// `ReorderOperation`: `id` moved to position `to`.
pub(super) fn reorder(
    project: &mut Project,
    id: &OperationId,
    to: usize,
) -> Result<(), CommandError> {
    let from = index_of(project, id)?;
    let len = project.operations.len();
    if to >= len {
        return Err(CommandError::IndexOutOfRange { index: to, len });
    }
    let operation = project.operations.remove(from);
    project.operations.insert(to, operation);
    Ok(())
}

/// `DuplicateOperation`: a copy under the next free `op<n>`, named
/// `<name> (2)` or the next free suffix, right after the original.
pub(super) fn duplicate(project: &Project, id: &OperationId) -> Result<Project, CommandError> {
    let at = index_of(project, id)?;
    let mut next = project.clone();
    let Some(original) = project.operations.get(at) else {
        return Err(CommandError::UnknownOperation { id: id.clone() });
    };
    let taken_ids: Vec<&str> = project.operations.iter().map(|o| o.id().as_str()).collect();
    let taken_names: Vec<&str> = project.operations.iter().map(Operation::name).collect();
    let mut copy = original.clone();
    rename(
        &mut copy,
        OperationId::new(super::instances::next_free_id(&taken_ids, "op")),
        super::instances::copy_name(&taken_names, original.name()),
    );
    next.operations.insert(at + 1, copy);
    Ok(next)
}

/// `RemoveTool`: refused, naming the operations, while any uses the tool
/// in a run (its own, a clearing pass's, a cut-out's).
pub(super) fn remove_tool(project: &mut Project, id: &ToolId) -> Result<(), CommandError> {
    let Some(at) = project.tools.iter().position(|t| t.id == *id) else {
        return Err(CommandError::UnknownTool { id: id.clone() });
    };
    let users: Vec<String> = project
        .operations
        .iter()
        .filter(|o| o.run_tools().contains(&id))
        .map(|o| o.name().to_owned())
        .collect();
    if !users.is_empty() {
        return Err(CommandError::ToolInUse {
            tool: project
                .tools
                .get(at)
                .map_or_else(|| id.as_str().to_owned(), |t| t.name.clone()),
            operations: users,
        });
    }
    project.tools.remove(at);
    Ok(())
}

/// The one field `edited` changes against `before`'s operation of the same
/// id, in words (`depth`, `step down`), or `None` when none or several
/// changed, or the kind did.
pub(super) fn changed_field(before: &Project, edited: &Operation) -> Option<String> {
    let at = index_of(before, edited.id()).ok()?;
    let original = before.operations.get(at)?;
    let (serde_json::Value::Object(old), serde_json::Value::Object(new)) = (
        serde_json::to_value(original).ok()?,
        serde_json::to_value(edited).ok()?,
    ) else {
        return None;
    };
    let mut changed: Vec<&String> = old
        .iter()
        .filter(|(key, value)| new.get(*key) != Some(value))
        .map(|(key, _)| key)
        .collect();
    changed.extend(new.keys().filter(|key| !old.contains_key(*key)));
    match changed.as_slice() {
        [key] if *key != "kind" => Some(field_words(key)),
        _ => None,
    }
}

/// `step_down_mm` → `step down`: the unit suffix off, underscores to
/// spaces.
fn field_words(key: &str) -> String {
    let base = ["_mm_min", "_mm", "_deg", "_fraction", "_s"]
        .iter()
        .find_map(|suffix| key.strip_suffix(suffix))
        .unwrap_or(key);
    base.replace('_', " ")
}

/// `“{name}”` for operation `id`, as a label's subject; an id the project
/// does not have shows as the id.
pub(super) fn subject(strings: &Strings, project: &Project, id: &OperationId) -> String {
    let name = project
        .operations
        .iter()
        .find(|o| o.id() == id)
        .map_or_else(|| id.as_str().to_owned(), |o| o.name().to_owned());
    strings.fmt(StrKey::CommandSubjectOne, &[("name", &name)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_words_drop_the_unit() {
        assert_eq!(field_words("depth_mm"), "depth");
        assert_eq!(field_words("step_down_mm"), "step down");
        assert_eq!(field_words("feed_mm_min"), "feed");
        assert_eq!(field_words("angle_deg"), "angle");
        assert_eq!(field_words("step_over_fraction"), "step over");
        assert_eq!(field_words("name"), "name");
    }
}
