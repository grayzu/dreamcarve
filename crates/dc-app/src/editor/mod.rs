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
//!
//! Every read and edit is of a [`Slot`]: a row at its key path in the
//! record. An operation's own row is at its key (`depth_mm`); a row of one
//! item of a list is at the item's (a clearing pass's `clearing[1].tool`,
//! slice 13, #542), so a list's items are edited by the same functions and
//! drawn by the same renderers. The free functions are the [`Slot::of`]
//! case.

pub mod clearing;
pub mod fields;
mod set;
pub mod targets;

use std::borrow::Cow;

use dc_job::{Finding, Tier};
use dc_project::{Operation, OperationId, Project, Tool, ToolId, ToolKind, Units};
use serde_json::Value;

pub use fields::{ChoiceOption, FieldKind, Group, Row, ToolDefault, Unit};

use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;
use crate::operations::Kind;
use crate::project::FieldStatus;
use crate::strings::{StrKey, Strings};
use crate::units;

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
        FieldKind::Length | FieldKind::SwitchedLength => units::field_text(value, units),
        FieldKind::Rate => plain(units::to_display(value, units)),
        FieldKind::Percentage => plain(value * 100.0),
        _ => plain(value),
    }
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

/// `text` typed into a field of `row`'s kind at `key` as the file's
/// value: `null` for an empty field.
fn parse(project: &Project, row: &Row, key: &str, text: &str) -> Result<Value, CommandError> {
    let units = project.display_units;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    Ok(match row.kind {
        FieldKind::Text => Value::from(trimmed),
        FieldKind::Length | FieldKind::SwitchedLength => {
            Value::from(units::parse_length_mm(text, units)?)
        }
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
                field: key.to_owned(),
                reason: "it is not typed".to_owned(),
            });
        }
    })
}

/// A row at its key path in an operation's record: the operation's own
/// field (`depth_mm`, [`Slot::of`]), or a field of one item of a list row
/// (a clearing pass's `clearing[1].tool`, [`Slot::item`]). What the row
/// shows and every edit of it are read and written at that path.
#[derive(Debug, Clone)]
pub struct Slot<'r> {
    /// The row: its kind, label, help and unit.
    pub row: &'r Row,
    /// Its key path in the record.
    key: Cow<'r, str>,
}

impl<'r> From<&'r Row> for Slot<'r> {
    fn from(row: &'r Row) -> Self {
        Self::of(row)
    }
}

impl<'r> Slot<'r> {
    /// An operation's own row, at its key.
    #[must_use]
    pub fn of(row: &'r Row) -> Self {
        Self {
            row,
            key: Cow::Borrowed(row.key),
        }
    }

    /// `row` of item `index` of the list at `list`'s key: its path is
    /// `<list>[<index>].<row>`, as validation names it.
    #[must_use]
    pub fn item(list: &Row, index: usize, row: &'r Row) -> Self {
        Self {
            row,
            key: Cow::Owned(format!("{}[{index}].{}", list.key, row.key)),
        }
    }

    /// Its key path in the record: also its egui id's, with the
    /// operation's id, so a row keeps its id however the layout moves.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Whether it applies to `record`: its path is there, and its row's
    /// rule holds.
    #[must_use]
    pub fn applies_to(&self, record: &Value) -> bool {
        fields::get(record, &self.key).is_some() && self.row.applies.is_none_or(|rule| rule(record))
    }

    /// The value at its path of `op`, `None` when the record lacks it.
    fn value(&self, op: &Operation) -> Option<Value> {
        fields::get(&fields::record(op).ok()?, &self.key).cloned()
    }

    /// `op` with `value` at its path as a command, or `None` when that is
    /// what it holds.
    fn update(&self, op: &Operation, value: Value) -> Result<Option<Command>, CommandError> {
        if self.value(op).as_ref() == Some(&value) {
            return Ok(None);
        }
        fields::set(op, &self.key, value).map(|next| Some(Command::UpdateOperation(next)))
    }

    /// [`value_text`].
    #[must_use]
    pub fn value_text(&self, project: &Project, op: &Operation) -> String {
        match self.value(op) {
            Some(Value::Number(n)) => n
                .as_f64()
                .map(|v| number_text(self.row, v, project.display_units))
                .unwrap_or_default(),
            Some(Value::String(text)) => text,
            _ => String::new(),
        }
    }

    /// [`placeholder`].
    #[must_use]
    pub fn placeholder(
        &self,
        project: &Project,
        strings: &Strings,
        op: &Operation,
    ) -> Option<String> {
        let default = self.row.tool_default?;
        if !self.value(op)?.is_null() {
            return None;
        }
        let tool = project.tools.iter().find(|t| t.id == *op.tool());
        let Some(value) = tool.and_then(|t| default.of(t)) else {
            return Some(strings.get(StrKey::EditorToolsUnset).to_owned());
        };
        let units = project.display_units;
        let text = format!(
            "{} {}",
            number_text(self.row, value, units),
            unit_suffix(self.row, units)
        );
        Some(strings.fmt(StrKey::EditorToolsValue, &[("value", text.trim_end())]))
    }

    /// [`commit_text`].
    ///
    /// # Errors
    ///
    /// As [`commit_text`].
    pub fn commit_text(
        &self,
        project: &Project,
        op: &Operation,
        text: &str,
    ) -> Result<Option<Command>, CommandError> {
        if matches!(self.row.kind, FieldKind::Text) && text.trim().is_empty() {
            return Ok(None);
        }
        let value = parse(project, self.row, &self.key, text)?;
        let empty = value.is_null();
        self.update(op, value).map_err(|e| {
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

    /// [`choice`].
    #[must_use]
    pub fn choice(&self, op: &Operation) -> Option<usize> {
        let FieldKind::Choice(options) = self.row.kind else {
            return None;
        };
        let value = self.value(op)?;
        let tag = fields::tag_of(&value)?;
        options.iter().position(|o| o.tag == tag)
    }

    /// [`choose`].
    ///
    /// # Errors
    ///
    /// As [`choose`].
    pub fn choose(
        &self,
        project: &Project,
        op: &Operation,
        index: usize,
    ) -> Result<Option<Command>, CommandError> {
        let FieldKind::Choice(options) = self.row.kind else {
            return Err(CommandError::FieldRefused {
                field: self.key.to_string(),
                reason: "it is not a choice".to_owned(),
            });
        };
        let option = options
            .get(index)
            .ok_or_else(|| CommandError::FieldRefused {
                field: self.key.to_string(),
                reason: format!("it has no option {index}"),
            })?;
        if self.choice(op) == Some(index) {
            return Ok(None);
        }
        self.update(op, option.value(project))
    }

    /// [`checked`].
    #[must_use]
    pub fn checked(&self, op: &Operation) -> bool {
        self.value(op).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    /// [`set_checked`].
    ///
    /// # Errors
    ///
    /// As [`set_checked`].
    pub fn set_checked(&self, op: &Operation, on: bool) -> Result<Option<Command>, CommandError> {
        self.update(op, Value::Bool(on))
    }

    /// [`switched`].
    #[must_use]
    pub fn switched(&self, op: &Operation) -> bool {
        self.value(op).is_some_and(|v| !v.is_null())
    }

    /// [`switch`].
    ///
    /// # Errors
    ///
    /// As [`switch`].
    pub fn switch(
        &self,
        project: &Project,
        op: &Operation,
        on: bool,
    ) -> Result<Option<Command>, CommandError> {
        let value = match self.row.kind {
            FieldKind::Toggle(template) => {
                if self.switched(op) == on {
                    return Ok(None);
                }
                if on { template(project) } else { Value::Null }
            }
            // On writes nothing until a length is typed (S20).
            FieldKind::SwitchedLength if on => return Ok(None),
            FieldKind::SwitchedLength => Value::Null,
            _ => {
                return Err(CommandError::FieldRefused {
                    field: self.key.to_string(),
                    reason: "it is not a switch".to_owned(),
                });
            }
        };
        self.update(op, value)
    }

    /// [`can_reset`].
    #[must_use]
    pub fn can_reset(&self, op: &Operation) -> bool {
        self.row.tool_default.is_some() && self.value(op).is_some_and(|v| !v.is_null())
    }

    /// [`reset`].
    ///
    /// # Errors
    ///
    /// As [`reset`].
    pub fn reset(&self, op: &Operation) -> Result<Option<Command>, CommandError> {
        if !self.can_reset(op) {
            return Ok(None);
        }
        self.update(op, Value::Null)
    }

    /// [`set_tool`].
    ///
    /// # Errors
    ///
    /// As [`set_tool`].
    pub fn set_tool(&self, op: &Operation, tool: &ToolId) -> Result<Option<Command>, CommandError> {
        self.update(op, Value::from(tool.as_str()))
    }

    /// The tool its value names, for a tool picker: the operation's, or
    /// a clearing pass's.
    #[must_use]
    pub fn tool<'p>(&self, project: &'p Project, op: &Operation) -> Option<&'p Tool> {
        let id = self.value(op)?;
        project
            .tools
            .iter()
            .find(|t| Some(t.id.as_str()) == id.as_str())
    }
}

/// The text `row`'s field shows for `op`: the value in the display units
/// with no unit, or empty for `null`.
#[must_use]
pub fn value_text(project: &Project, op: &Operation, row: &Row) -> String {
    Slot::of(row).value_text(project, op)
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
    Slot::of(row).placeholder(project, strings, op)
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
    Slot::of(row).commit_text(project, op, text)
}

/// Which of a choice row's options `op` holds.
#[must_use]
pub fn choice(op: &Operation, row: &Row) -> Option<usize> {
    Slot::of(row).choice(op)
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
    Slot::of(row).choose(project, op, index)
}

/// Whether a checkbox row is ticked.
#[must_use]
pub fn checked(op: &Operation, row: &Row) -> bool {
    Slot::of(row).checked(op)
}

/// A checkbox row ticked or not.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn set_checked(op: &Operation, row: &Row, on: bool) -> Result<Option<Command>, CommandError> {
    Slot::of(row).set_checked(op, on)
}

/// Whether a switch row's feature is on: its value is not `null`.
#[must_use]
pub fn switched(op: &Operation, row: &Row) -> bool {
    Slot::of(row).switched(op)
}

/// A switch row turned on (its template, built for `project`) or off
/// (`null`); `None` when it is so already. A switched length writes
/// nothing when turned on: its field opens empty, and what is typed is
/// its value (S20).
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
    Slot::of(row).switch(project, op, on)
}

/// Whether ⟲ is shown: a tool-derived row holding a value of its own.
#[must_use]
pub fn can_reset(op: &Operation, row: &Row) -> bool {
    Slot::of(row).can_reset(op)
}

/// ⟲: a tool-derived row back to `null`, the tool's value; `None` for any
/// other row, or one already `null`.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn reset(op: &Operation, row: &Row) -> Result<Option<Command>, CommandError> {
    Slot::of(row).reset(op)
}

/// The tool picker's choice.
///
/// # Errors
///
/// [`CommandError::FieldRefused`] when the file refuses it.
pub fn set_tool(op: &Operation, row: &Row, tool: &ToolId) -> Result<Option<Command>, CommandError> {
    Slot::of(row).set_tool(op, tool)
}

/// The tools the Tool row's picker offers for `op`, in project order: the
/// V-bits for a kind that carves with one (V-Carve, Inlay, Photo
/// V-Carve; slice 13, #542), every tool otherwise. A file's tool of
/// another type still shows as chosen, with validation's message under
/// the row.
#[must_use]
pub fn offered_tools<'p>(project: &'p Project, op: &Operation) -> Vec<&'p Tool> {
    let v_bits = matches!(Kind::of(op), Kind::VCarve | Kind::Inlay | Kind::PhotoVCarve);
    project
        .tools
        .iter()
        .filter(|t| !v_bits || matches!(t.cutter, ToolKind::VBit { .. }))
        .collect()
}

/// The line under a tool picker showing `tool`: a V-bit's cone height,
/// `cone height 3.175 mm`, in the display units (the V-Carve chapter's
/// `diameter / (2 tan(α/2))`, the depth past which the shank cuts); `None`
/// for any other cutter, or a V-bit whose angle or diameter has no cone.
#[must_use]
pub fn tool_note(project: &Project, strings: &Strings, tool: &Tool) -> Option<String> {
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    let usable = included_angle_deg > 0.0
        && included_angle_deg < 180.0
        && tool.diameter_mm.is_finite()
        && tool.diameter_mm > 0.0;
    let cone_mm = tool.diameter_mm / (2.0 * (included_angle_deg.to_radians() / 2.0).tan());
    if !(usable && cone_mm.is_finite()) {
        return None;
    }
    let units = project.display_units;
    let text = format!(
        "{} {}",
        units::field_text(cone_mm, units),
        units::suffix(units)
    );
    Some(strings.fmt(StrKey::EditorToolCone, &[("value", &text)]))
}

/// `check`'s carve summary for operation `op` (slice 13, #542): the lines
/// `check` prints for it ([`dc_job::describe_carves`]: its deepest cut,
/// its flat floor, and the ridges where no clearing pass reaches; a line
/// per board for an Inlay), from the generation result the app holds,
/// which the worker described when it generated it. Empty for a kind
/// `check` prints no such line for, and while the operation has no
/// current result (before it generates, while it regenerates, or when
/// validation stops generation, as `check` then prints none).
#[must_use]
pub fn carve_summary(app: &App, op: &OperationId) -> Vec<String> {
    let generation = app.generation();
    generation
        .result(op)
        .filter(|_| generation.is_current(op))
        .map(|result| result.carves.clone())
        .unwrap_or_default()
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
        self.all(row).into_iter().next()
    }

    /// Every message located on `row`, errors first, then warnings, then
    /// hints, each tier in the findings' order: what a block row (a
    /// clearing list) shows under itself, its items' findings among them
    /// (`clearing_not_descending` at `clearing[1].tool`).
    #[must_use]
    pub fn all(&self, row: &Row) -> Vec<FieldStatus<'a>> {
        [Tier::Error, Tier::Warning, Tier::Hint]
            .into_iter()
            .flat_map(|tier| {
                self.found
                    .iter()
                    .filter(move |(r, f)| r.key == row.key && f.tier == tier)
                    .map(|(_, f)| FieldStatus {
                        tier: f.tier,
                        message: &f.message,
                    })
            })
            .collect()
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
