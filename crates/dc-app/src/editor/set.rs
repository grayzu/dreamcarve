// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reading and writing an operation by key path
//! (`docs/specs/phase-4/design.md`, "Architecture → The editors"): the
//! record is the operation as the file holds it, a `serde_json::Value`,
//! and a key path is its dotted object keys (`entry.ramp.angle_deg`), a
//! list's item named by its index as validation names it
//! (`clearing[1].tool`; slice 13, #542). An
//! edit sets the value at the path and reads the whole record back
//! through the file's own deserializer, so the editor can write exactly
//! what a hand-edited `.dcv` can hold and nothing else, and every edit is
//! one `UpdateOperation` with the whole record.

use dc_project::Operation;
use serde_json::Value;

use crate::error::CommandError;

/// `op` as the file holds it.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] if serde cannot serialize it, which no
/// model value does.
pub fn record(op: &Operation) -> Result<Value, CommandError> {
    serde_json::to_value(op).map_err(|e| CommandError::FieldRefused {
        field: op.id().to_string(),
        reason: e.to_string(),
    })
}

/// One step of a key path: an object's key, or a list's index.
enum Step<'k> {
    Key(&'k str),
    Index(usize),
}

/// The steps of `key`: its dotted parts, each an object key followed by
/// any `[i]` indices (`clearing[1].tool` is `clearing`, 1, `tool`);
/// `None` when an index is not a number or a bracket is unclosed.
fn steps(key: &str) -> Option<Vec<Step<'_>>> {
    let mut out = Vec::new();
    for part in key.split('.') {
        let (name, mut rest) = part.find('[').map_or((part, ""), |at| part.split_at(at));
        out.push(Step::Key(name));
        while let Some(open) = rest.strip_prefix('[') {
            let (index, after) = open.split_once(']')?;
            out.push(Step::Index(index.parse().ok()?));
            rest = after;
        }
        if !rest.is_empty() {
            return None;
        }
    }
    Some(out)
}

/// The value at `key` in `record`, `None` when the path is not there
/// (`Some(&Value::Null)` when it is, and is `null`).
#[must_use]
pub fn get<'a>(record: &'a Value, key: &str) -> Option<&'a Value> {
    steps(key)?
        .into_iter()
        .try_fold(record, |value, step| match step {
            Step::Key(part) => value.as_object()?.get(part),
            Step::Index(i) => value.as_array()?.get(i),
        })
}

/// `op` with the value at `key` replaced by `value`, read back as the
/// file reads it.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the record has no such path, when
/// the path is the record's `kind` or `id`, or when the file's reader
/// refuses the result (a string for a length, a side that is not one).
pub fn set(op: &Operation, key: &str, value: Value) -> Result<Operation, CommandError> {
    let refused = |reason: String| CommandError::FieldRefused {
        field: key.to_owned(),
        reason,
    };
    if key == "kind" || key == "id" {
        return Err(refused("it names the operation".to_owned()));
    }
    let mut record = record(op)?;
    let slot = steps(key)
        .and_then(|steps| {
            steps
                .into_iter()
                .try_fold(&mut record, |value, step| match step {
                    Step::Key(part) => value.as_object_mut()?.get_mut(part),
                    Step::Index(i) => value.as_array_mut()?.get_mut(i),
                })
        })
        .ok_or_else(|| refused("the operation has no such field".to_owned()))?;
    *slot = value;
    serde_json::from_value(record).map_err(|e| refused(e.to_string()))
}
