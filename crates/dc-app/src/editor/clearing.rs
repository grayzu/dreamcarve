// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A carve's clearing passes (`docs/specs/phase-4/functional.md`,
//! capability 70, acceptance 75; mock-up `06-operation-vcarve.svg`; slice
//! 13, #542): the list at a [`FieldKind::ClearingList`] row's key path, a
//! V-Carve's or an Inlay's `clearing`, as one block. *Add pass* appends a
//! pass of the next smaller end mill ([`next_smaller_end_mill`]), with an
//! offset clearance and `init`'s direction and entry; *Remove* drops one;
//! a drag is [`reorder`], one step. Each edit is the whole list written
//! back as one `UpdateOperation`, so each is one undo step. Each pass's
//! own fields are [`PASS`]'s rows at the pass's key path
//! (`clearing[1].strategy`, [`rows`]), read and written through
//! [`Slot`] as every row is.
//!
//! Nothing here is V-Carve's alone: every function takes the list row, so
//! the Inlay's `clearing` (slice 14, #543) is the same list.
//!
//! The list does not reorder itself: the file's rule that passes descend
//! in diameter is validation's (`clearing_not_descending`), shown under the
//! list as `check` reports it, so a list written by hand reads the same.

use dc_project::{ClearanceStrategy, ClearingPass, Operation, Project, Tool, ToolId, ToolKind};
use serde_json::Value;

use super::Slot;
pub use super::fields::CLEARING_PASS as PASS;
use super::fields::{self, FieldKind, Row};
use crate::command::Command;
use crate::error::CommandError;

/// The passes of `op`'s list at `list`'s key, in cut order; empty when the
/// record has none there.
#[must_use]
pub fn passes(op: &Operation, list: &Row) -> Vec<ClearingPass> {
    fields::record(op)
        .ok()
        .and_then(|record| fields::get(&record, list.key).cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// The tools a pass's picker offers: the project's flat end mills, in
/// project order (a clearing pass is cut as a Pocket, which only a flat
/// end mill cuts to size; validation's `clearing_needs_end_mill`).
#[must_use]
pub fn tools(project: &Project) -> Vec<&Tool> {
    project
        .tools
        .iter()
        .filter(|t| t.cutter == ToolKind::EndMill)
        .collect()
}

/// The end mill *Add pass* picks after `passes`: of the project's end
/// mills smaller than every pass's tool, the largest, the first listed of
/// equals; the largest end mill when no pass names a tool the project
/// has. `None` when there is no such end mill: the new pass names none,
/// and validation says so (`clearing_dangling_tool`).
#[must_use]
pub fn next_smaller_end_mill(project: &Project, passes: &[ClearingPass]) -> Option<ToolId> {
    let smallest_mm = passes
        .iter()
        .filter_map(|p| project.tools.iter().find(|t| t.id == p.tool))
        .map(|t| t.diameter_mm)
        .filter(|d| d.is_finite())
        .reduce(f64::min);
    tools(project)
        .into_iter()
        .filter(|t| t.diameter_mm.is_finite() && smallest_mm.is_none_or(|s| t.diameter_mm < s))
        .fold(None::<&Tool>, |best, t| match best {
            Some(b) if b.diameter_mm >= t.diameter_mm => Some(b),
            _ => Some(t),
        })
        .map(|t| t.id.clone())
}

/// The pass *Add pass* appends after `passes`: the next smaller end mill
/// (or none), an offset clearance, and `init`'s direction and entry, as a
/// new Pocket has (S20: nothing of the app's own).
fn new_pass(project: &Project, passes: &[ClearingPass]) -> ClearingPass {
    let init = dc_job::project_from::default_profile(Vec::new(), ToolId::new(""));
    ClearingPass {
        tool: next_smaller_end_mill(project, passes).unwrap_or_else(|| ToolId::new("")),
        strategy: ClearanceStrategy::Offset,
        direction: init.direction,
        entry: init.entry,
    }
}

/// `list`, refused unless it is a clearing list.
fn clearing_list(list: &Row) -> Result<(), CommandError> {
    if matches!(list.kind, FieldKind::ClearingList) {
        Ok(())
    } else {
        Err(CommandError::FieldRefused {
            field: list.key.to_owned(),
            reason: "it is not a clearing list".to_owned(),
        })
    }
}

/// `op` with `passes` as its list at `list`, as one command.
fn write(
    op: &Operation,
    list: &Row,
    passes: &[ClearingPass],
) -> Result<Option<Command>, CommandError> {
    let value = serde_json::to_value(passes).map_err(|e| CommandError::FieldRefused {
        field: list.key.to_owned(),
        reason: e.to_string(),
    })?;
    fields::set(op, list.key, value).map(|next| Some(Command::UpdateOperation(next)))
}

/// *Add pass*: a pass of the next smaller end mill appended to the list.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when `list` is not a clearing list, or
/// the file refuses the result.
pub fn add(project: &Project, op: &Operation, list: &Row) -> Result<Option<Command>, CommandError> {
    clearing_list(list)?;
    let mut all = passes(op, list);
    all.push(new_pass(project, &all));
    write(op, list, &all)
}

/// The refusal of pass `index` (from 0) of a list of `count`.
fn no_pass(list: &Row, index: usize, count: usize) -> CommandError {
    CommandError::FieldRefused {
        field: format!("{}[{index}]", list.key),
        reason: format!("the list has {count} passes"),
    }
}

/// *Remove* on pass `index`, from 0.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when `list` is not a clearing list or
/// has no such pass.
pub fn remove(op: &Operation, list: &Row, index: usize) -> Result<Option<Command>, CommandError> {
    clearing_list(list)?;
    let mut all = passes(op, list);
    if index >= all.len() {
        return Err(no_pass(list, index, all.len()));
    }
    all.remove(index);
    write(op, list, &all)
}

/// A drag of pass `from` to position `to` (both from 0, `to` where it
/// lands once taken out); `None` when it lands where it was.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when `list` is not a clearing list or
/// either position is past its end.
pub fn reorder(
    op: &Operation,
    list: &Row,
    from: usize,
    to: usize,
) -> Result<Option<Command>, CommandError> {
    clearing_list(list)?;
    let mut all = passes(op, list);
    for at in [from, to] {
        if at >= all.len() {
            return Err(no_pass(list, at, all.len()));
        }
    }
    if from == to {
        return Ok(None);
    }
    let moved = all.remove(from);
    all.insert(to, moved);
    write(op, list, &all)
}

/// The rows of pass `index` that apply, in [`PASS`]'s order, each at the
/// pass's key path: a variant's own rows (a raster's angle, a ramp's)
/// come and go with the choice, as an operation's do.
#[must_use]
pub fn rows(op: &Operation, list: &Row, index: usize) -> Vec<Slot<'static>> {
    let record = fields::record(op).unwrap_or(Value::Null);
    PASS.iter()
        .map(|row| Slot::item(list, index, row))
        .filter(|slot| slot.applies_to(&record))
        .collect()
}
