// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operation editor's model (`docs/specs/phase-4/design.md`,
//! "Architecture → The editors"; functional spec, capabilities 66 to 68;
//! mock-up `05-operation-profile.svg`): the table of rows
//! ([`fields`]), what each row shows (its text, the tool's value as a
//! placeholder, the choice made, the file's located message), and the
//! command each edit is. Every edit is one `UpdateOperation` with the
//! whole record, built by [`fields::set`]; an edit that changes nothing is
//! no command, so leaving a field untouched is no undo step. The shell
//! draws the rows and runs the commands; nothing here holds state.
//!
//! A typed value shows in the project's display units and parses what the
//! file accepts ([`crate::units`]); a percentage is the file's fraction
//! times 100. An empty field is `null` where the file allows it; where the
//! value is the tool's while `null` (step-down, stepover, pitch), the
//! field shows `tool's: 3 mm` in grey and ⟲ writes `null` again.

pub mod fields;
mod set;
pub mod targets;

use dc_job::{Finding, Tier};
use dc_project::{Operation, Project, ToolId, Units};
use serde_json::Value;

pub use fields::{ChoiceOption, FieldKind, Group, Row, ToolDefault, Unit};

use crate::command::Command;
use crate::error::CommandError;
use crate::project::FieldStatus;
use crate::strings::{StrKey, Strings};
use crate::units;

/// The value at `row`'s path of `op`, `None` when the record lacks it.
fn value_of(op: &Operation, row: &Row) -> Option<Value> {
    fields::get(&fields::record(op).ok()?, row.key).cloned()
}

/// `number` with up to four decimals and no trailing zeros.
fn plain(number: f64) -> String {
    units::field_text(number, Units::Mm)
}

/// `row`'s unit as its field draws it after the value (`mm`, `°`, `%`).
#[must_use]
pub const fn unit_suffix(row: &Row, units: Units) -> &'static str {
    match row.unit() {
        Unit::None => "",
        Unit::Length => units::suffix(units),
        Unit::Rate => units::rate_suffix(units),
        Unit::Degrees => "°",
        Unit::Percent => "%",
        Unit::Seconds => "s",
    }
}

/// `value`, a number of `row`'s kind, as its field shows it in `units`.
fn number_text(row: &Row, value: f64, units: Units) -> String {
    match row.kind {
        FieldKind::Length => units::field_text(value, units),
        FieldKind::Rate => plain(units::to_display(value, units)),
        FieldKind::Percentage => plain(value * 100.0),
        _ => plain(value),
    }
}

/// The text `row`'s field shows for `op`: the value in the display units
/// with no unit, or empty for `null`.
#[must_use]
pub fn value_text(project: &Project, op: &Operation, row: &Row) -> String {
    match value_of(op, row) {
        Some(Value::Number(n)) => n
            .as_f64()
            .map(|v| number_text(row, v, project.display_units))
            .unwrap_or_default(),
        Some(Value::String(text)) => text,
        _ => String::new(),
    }
}

/// The grey placeholder of a tool-derived row while its value is `null`:
/// `tool's: 3 mm`, or `tool's: not set` when the tool has none. `None`
/// for every other row and value.
#[must_use]
pub fn placeholder(
    project: &Project,
    strings: &Strings,
    op: &Operation,
    row: &Row,
) -> Option<String> {
    let default = row.tool_default?;
    if !value_of(op, row)?.is_null() {
        return None;
    }
    let tool = project.tools.iter().find(|t| t.id == *op.tool());
    let Some(value) = tool.and_then(|t| default.of(t)) else {
        return Some(strings.get(StrKey::EditorToolsUnset).to_owned());
    };
    let units = project.display_units;
    let text = format!(
        "{} {}",
        number_text(row, value, units),
        unit_suffix(row, units)
    );
    Some(strings.fmt(StrKey::EditorToolsValue, &[("value", text.trim_end())]))
}

/// A number typed into a field whose unit is `suffix`, the suffix allowed.
fn parse_plain(text: &str, suffix: &str) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    let bare = trimmed.strip_suffix(suffix).unwrap_or(trimmed).trim();
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    };
    if bare.contains(',') {
        return Err(refused("use a dot for decimals".to_owned()));
    }
    let value: f64 = bare
        .parse()
        .map_err(|_| refused(format!("`{trimmed}` is not a number")))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(refused(format!("`{trimmed}` is not a finite number")))
    }
}

/// `text` typed into `row`'s field as the file's value: `null` for an
/// empty field.
fn parse(project: &Project, row: &Row, text: &str) -> Result<Value, CommandError> {
    let units = project.display_units;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    Ok(match row.kind {
        FieldKind::Text => Value::from(trimmed),
        FieldKind::Length => Value::from(units::parse_length_mm(text, units)?),
        FieldKind::Rate => Value::from(units::parse_rate_mm_min(text, units)?),
        FieldKind::Angle => Value::from(parse_plain(text, "°")?),
        FieldKind::Percentage => Value::from(parse_plain(text, "%")? / 100.0),
        FieldKind::Seconds => Value::from(parse_plain(text, "s")?),
        FieldKind::Count => {
            let count: u64 = trimmed.parse().map_err(|_| CommandError::NotAQuantity {
                text: text.to_owned(),
                reason: format!("`{trimmed}` is not a whole number"),
            })?;
            Value::from(count)
        }
        _ => {
            return Err(CommandError::FieldRefused {
                field: row.key.to_owned(),
                reason: "it is not typed".to_owned(),
            });
        }
    })
}

/// `op` with `value` at `row`'s path as a command, or `None` when that is
/// what it holds.
fn update(op: &Operation, row: &Row, value: Value) -> Result<Option<Command>, CommandError> {
    if value_of(op, row).as_ref() == Some(&value) {
        return Ok(None);
    }
    fields::set(op, row.key, value).map(|next| Some(Command::UpdateOperation(next)))
}

/// `text`, typed into `row`'s field and left (capability 66: "validates on
/// leave"), as a command; `None` when it changes nothing. An empty name is
/// no change, as an empty rename is.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not what the field
/// takes, or empty where the file needs a value;
/// [`CommandError::FieldRefused`] when the file's reader refuses it.
pub fn commit_text(
    project: &Project,
    op: &Operation,
    row: &Row,
    text: &str,
) -> Result<Option<Command>, CommandError> {
    if matches!(row.kind, FieldKind::Text) && text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse(project, row, text)?;
    let empty = value.is_null();
    update(op, row, value).map_err(|e| {
        if empty {
            CommandError::NotAQuantity {
                text: text.to_owned(),
                reason: "nothing was typed".to_owned(),
            }
        } else {
            e
        }
    })
}

/// Which of a choice row's options `op` holds.
#[must_use]
pub fn choice(op: &Operation, row: &Row) -> Option<usize> {
    let FieldKind::Choice(options) = row.kind else {
        return None;
    };
    let value = value_of(op, row)?;
    let tag = fields::tag_of(&value)?;
    options.iter().position(|o| o.tag == tag)
}

/// Choosing option `index` of a choice row: its value, built for
/// `project`; `None` when `op` holds it already.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the row is not a choice or has no
/// such option, or the file refuses it.
pub fn choose(
    project: &Project,
    op: &Operation,
    row: &Row,
    index: usize,
) -> Result<Option<Command>, CommandError> {
    let FieldKind::Choice(options) = row.kind else {
        return Err(CommandError::FieldRefused {
            field: row.key.to_owned(),
            reason: "it is not a choice".to_owned(),
        });
    };
    let option = options
        .get(index)
        .ok_or_else(|| CommandError::FieldRefused {
            field: row.key.to_owned(),
            reason: format!("it has no option {index}"),
        })?;
    if choice(op, row) == Some(index) {
        return Ok(None);
    }
    update(op, row, option.value(project))
}

/// Whether a checkbox row is ticked.
#[must_use]
pub fn checked(op: &Operation, row: &Row) -> bool {
    value_of(op, row).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// A checkbox row ticked or not.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn set_checked(op: &Operation, row: &Row, on: bool) -> Result<Option<Command>, CommandError> {
    update(op, row, Value::Bool(on))
}

/// Whether a switch row's feature is on: its value is not `null`.
#[must_use]
pub fn switched(op: &Operation, row: &Row) -> bool {
    value_of(op, row).is_some_and(|v| !v.is_null())
}

/// A switch row turned on (its template, built for `project`) or off
/// (`null`); `None` when it is so already.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the row is not a switch, or the
/// file refuses it.
pub fn switch(
    project: &Project,
    op: &Operation,
    row: &Row,
    on: bool,
) -> Result<Option<Command>, CommandError> {
    let FieldKind::Toggle(template) = row.kind else {
        return Err(CommandError::FieldRefused {
            field: row.key.to_owned(),
            reason: "it is not a switch".to_owned(),
        });
    };
    if switched(op, row) == on {
        return Ok(None);
    }
    update(op, row, if on { template(project) } else { Value::Null })
}

/// Whether ⟲ is shown: a tool-derived row holding a value of its own.
#[must_use]
pub fn can_reset(op: &Operation, row: &Row) -> bool {
    row.tool_default.is_some() && value_of(op, row).is_some_and(|v| !v.is_null())
}

/// ⟲: a tool-derived row back to `null`, the tool's value; `None` for any
/// other row, or one already `null`.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn reset(op: &Operation, row: &Row) -> Result<Option<Command>, CommandError> {
    if !can_reset(op, row) {
        return Ok(None);
    }
    update(op, row, Value::Null)
}

/// The tool picker's choice.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn set_tool(op: &Operation, row: &Row, tool: &ToolId) -> Result<Option<Command>, CommandError> {
    update(op, row, Value::from(tool.as_str()))
}

/// The findings located on an operation's rows, matched once
/// ([`fields::row_for`]), for the editor to draw under each row and count
/// on each closed group's header.
#[derive(Debug, Clone, Default)]
pub struct Located<'a> {
    found: Vec<(&'static Row, &'a Finding)>,
}

impl<'a> Located<'a> {
    /// The findings of `findings` located on `op`'s rows.
    #[must_use]
    pub fn of(findings: &'a [Finding], op: &Operation) -> Self {
        let rows = fields::rows(op);
        let record = fields::record(op).ok();
        let found = findings
            .iter()
            .filter_map(|f| {
                let field = f.field.as_deref()?;
                fields::row_among(&rows, record.as_ref()?, op, field).map(|row| (row, f))
            })
            .collect();
        Self { found }
    }

    /// The message under `row`: the first error located on it, else the
    /// first warning, else the first hint.
    #[must_use]
    pub fn status(&self, row: &Row) -> Option<FieldStatus<'a>> {
        [Tier::Error, Tier::Warning, Tier::Hint]
            .into_iter()
            .find_map(|tier| {
                self.found
                    .iter()
                    .find(|(r, f)| r.key == row.key && f.tier == tier)
            })
            .map(|(_, f)| FieldStatus {
                tier: f.tier,
                message: &f.message,
            })
    }

    /// What a closed group's header shows: the worst tier of the findings
    /// located on its rows, and how many.
    #[must_use]
    pub fn group(&self, group: Group) -> Option<(Tier, usize)> {
        let tiers: Vec<Tier> = self
            .found
            .iter()
            .filter(|(r, _)| r.group == group)
            .map(|(_, f)| f.tier)
            .collect();
        [Tier::Error, Tier::Warning, Tier::Hint]
            .into_iter()
            .find(|tier| tiers.contains(tier))
            .map(|tier| (tier, tiers.len()))
    }
}

/// The message under `row` ([`Located::status`]).
#[must_use]
pub fn status<'a>(
    findings: &'a [Finding],
    _project: &Project,
    op: &Operation,
    row: &Row,
) -> Option<FieldStatus<'a>> {
    Located::of(findings, op).status(row)
}

/// What a closed group's header shows ([`Located::group`]).
#[must_use]
pub fn group_status(
    findings: &[Finding],
    _project: &Project,
    op: &Operation,
    group: Group,
) -> Option<(Tier, usize)> {
    Located::of(findings, op).group(group)
}
