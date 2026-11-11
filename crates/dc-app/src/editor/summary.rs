// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A closed group's one-line summary and the groups' open state
//! (`docs/specs/phase-4/design.md`, "The editors"; functional spec,
//! capability 66; `experience.md`, section 7, "Disclosure section header";
//! slice 26, #618). Every group but Setup starts closed, and its header
//! says what it holds (`Strategy · Raster 0°, climb, wall last`), so the
//! whole operation reads on one screen.
//!
//! A summary is built from the same rows the fields draw, through the
//! same values ([`fields::get`], the tool's and the stock's defaults), so
//! it can never show a value the fields do not. Each row the operation
//! has in the group gives a phrase: a number with its label and unit
//! (`safe Z 5 mm`), a `null` that is the tool's as the tool's (`step-down
//! tool's 3 mm`), a choice as its option (`climb`), with its variant's
//! values (`Helix 80 % 15°`), a switch as on or off (`last pass off`). A
//! row may word itself ([`Row::summary`]: `wall last`). The first
//! [`PHRASES`] phrases, in the table's order, are the summary; the header
//! truncates what does not fit, and opening the group shows every field.

use std::collections::BTreeMap;

use dc_job::Tier;
use dc_project::{Operation, Project};
use serde_json::Value;

use super::fields::{self, FieldKind, Group, Row};
use super::{Located, Slot, clearing, earlier_pockets};
use crate::operations::Kind;
use crate::strings::{StrKey, Strings};
use crate::units;

/// How many phrases a summary holds: a glance, as the mock-ups' headers
/// read (`Raster 0°, climb, wall last`).
pub const PHRASES: usize = 3;

/// The one-line summary of `group` of `op` in `project`'s display units,
/// as its closed header shows it after the title; empty for Setup, which
/// never closes, and for a group with nothing to say.
#[must_use]
pub fn group(project: &Project, strings: &Strings, op: &Operation, group: Group) -> String {
    if !group.collapses() {
        return String::new();
    }
    let Ok(record) = fields::record(op) else {
        return String::new();
    };
    let rows: Vec<&'static Row> = fields::rows(op)
        .into_iter()
        .filter(|r| r.group == group)
        .collect();
    let cx = Cx {
        project,
        strings,
        op,
        record: &record,
    };
    // Rows a choice's or a switch's phrase already holds.
    let mut folded: Vec<String> = Vec::new();
    let mut phrases: Vec<String> = Vec::new();
    for row in &rows {
        if folded
            .iter()
            .any(|prefix| row.key.starts_with(prefix.as_str()))
        {
            continue;
        }
        if let Some(phrase) = cx.phrase(row, &rows) {
            phrases.push(phrase);
        }
        if matches!(
            row.kind,
            FieldKind::Choice(_) | FieldKind::Toggle(_) | FieldKind::TabRequest
        ) {
            folded.push(format!("{}.", row.key));
        }
    }
    capitalised(
        &phrases
            .into_iter()
            .take(PHRASES)
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Whether `group` of an operation of `kind` is open when the editor
/// shows it, by `sections` (the preferences' open states): Setup always;
/// every other group as last left, and closed until it has been opened.
#[must_use]
pub fn open(sections: &BTreeMap<String, bool>, kind: Kind, group: Group) -> bool {
    !group.collapses() || sections.get(&group.section(kind)).copied().unwrap_or(false)
}

/// The groups of `op` that open by themselves: those holding a located
/// error or warning (`located`), in render order. Setup is always open,
/// and a hint opens nothing.
#[must_use]
pub fn opened(op: &Operation, located: &Located<'_>) -> Vec<Group> {
    fields::groups(op)
        .into_iter()
        .map(|(group, _)| group)
        .filter(|group| {
            group.collapses()
                && located
                    .group(*group)
                    .is_some_and(|(tier, _)| tier != Tier::Hint)
        })
        .collect()
}

/// What every phrase reads.
struct Cx<'a> {
    project: &'a Project,
    strings: &'a Strings,
    op: &'a Operation,
    record: &'a Value,
}

impl Cx<'_> {
    /// `row`'s phrase, `None` when it has nothing to say (a box not ticked,
    /// no Pocket to rest on, a value derived elsewhere). `rows` are the
    /// group's, for a choice's variant values.
    fn phrase(&self, row: &Row, rows: &[&'static Row]) -> Option<String> {
        let value = fields::get(self.record, row.key)?;
        match row.kind {
            FieldKind::InstanceList
            | FieldKind::PointPick
            | FieldKind::InlayDerived
            | FieldKind::CountersinkDepth => None,
            FieldKind::Checkbox => value
                .as_bool()
                .is_some_and(|on| on)
                .then(|| self.label(row)),
            FieldKind::Toggle(_) | FieldKind::TabRequest => Some(if value.is_null() {
                self.off(row)
            } else {
                self.label(row)
            }),
            FieldKind::SwitchedLength if value.is_null() => Some(self.off(row)),
            FieldKind::Choice(options) => {
                let option = options.get(Slot::of(row).choice(self.op)?)?;
                let mut words = vec![lowered(self.strings.get(option.label))];
                let nested = format!("{}.", row.key);
                words.extend(
                    rows.iter()
                        .filter(|r| r.key.starts_with(&nested))
                        .filter_map(|r| self.value(r)),
                );
                let words = words.join(" ");
                Some(match row.summary {
                    Some(template) => self.strings.fmt(template, &[("value", &words)]),
                    None => words,
                })
            }
            FieldKind::ToolPicker => {
                let tool = Slot::of(row).tool(self.project, self.op)?;
                Some(self.labelled(row, &tool.name))
            }
            FieldKind::EarlierPocket => {
                let id = value.as_str()?;
                let name = earlier_pockets(self.project, self.op)
                    .into_iter()
                    .find(|(pocket, _)| pocket.as_str() == id)
                    .map_or_else(|| id.to_owned(), |(_, name)| name);
                Some(self.labelled(row, &name))
            }
            FieldKind::ClearingList => Some(match clearing::passes(self.op, row).len() {
                0 => self.strings.get(StrKey::EditorSummaryNoPasses).to_owned(),
                1 => self.strings.get(StrKey::EditorSummaryOnePass).to_owned(),
                n => self
                    .strings
                    .fmt(StrKey::EditorSummaryPasses, &[("count", &n.to_string())]),
            }),
            FieldKind::Length
            | FieldKind::SwitchedLength
            | FieldKind::LengthList
            | FieldKind::Rate
            | FieldKind::Angle
            | FieldKind::Percentage
            | FieldKind::Count
            | FieldKind::Seconds => {
                let text = self.value(row)?;
                Some(self.labelled(row, &text))
            }
        }
    }

    /// `row`'s value with its unit (`5 mm`, `15°`, `40 %`), the tool's
    /// or the stock's while `null` (`tool's 3 mm`); `None` for a `null`
    /// with neither, or an empty list.
    fn value(&self, row: &Row) -> Option<String> {
        let units = self.project.display_units;
        let number = |value: f64| match row.kind {
            FieldKind::Length
            | FieldKind::SwitchedLength
            | FieldKind::LengthList
            | FieldKind::CountersinkDepth => {
                format!(
                    "{} {}",
                    units::summary_text(value, units),
                    units::suffix(units)
                )
            }
            FieldKind::Rate => format!(
                "{} {}",
                units::summary_text(value, units),
                units::rate_suffix(units)
            ),
            FieldKind::Angle => format!("{}°", plain(value)),
            FieldKind::Percentage => format!("{} %", plain(value * 100.0)),
            FieldKind::Seconds => format!("{} s", plain(value)),
            _ => plain(value),
        };
        match fields::get(self.record, row.key)? {
            Value::Number(n) => n.as_f64().map(number),
            Value::Array(items) => {
                let values: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
                let (last, rest) = values.split_last()?;
                let mut text: Vec<String> = rest
                    .iter()
                    .map(|v| units::summary_text(*v, units))
                    .collect();
                text.push(number(*last));
                Some(text.join(", "))
            }
            Value::Null => {
                if let Some(stock) = row.stock_default {
                    let value = number(stock.of(&self.project.stock));
                    return Some(
                        self.strings
                            .fmt(StrKey::EditorSummaryStock, &[("value", &value)]),
                    );
                }
                let default = row.tool_default?;
                let tool = self.project.tools.iter().find(|t| t.id == *self.op.tool());
                let value = tool.and_then(|t| default.of(t)).map_or_else(
                    || self.strings.get(StrKey::EditorSummaryUnset).to_owned(),
                    number,
                );
                Some(
                    self.strings
                        .fmt(StrKey::EditorSummaryTools, &[("value", &value)]),
                )
            }
            _ => None,
        }
    }

    /// `row`'s label as a phrase starts: `safe Z`, `step-down`.
    fn label(&self, row: &Row) -> String {
        lowered(self.strings.get(row.label))
    }

    /// `row`'s label and `value`, or its own wording of the value.
    fn labelled(&self, row: &Row, value: &str) -> String {
        match row.summary {
            Some(template) => self.strings.fmt(template, &[("value", value)]),
            None => format!("{} {value}", self.label(row)),
        }
    }

    /// A switch that is off: `last pass off`.
    fn off(&self, row: &Row) -> String {
        self.strings
            .fmt(StrKey::EditorSummaryOff, &[("label", &self.label(row))])
    }
}

/// A plain number to two places, trailing zeros dropped.
fn plain(value: f64) -> String {
    units::summary_text(value, dc_project::Units::Mm)
}

/// `text` with its first letter lower-case, unless it starts an acronym
/// or a name whose second letter is a capital (`Safe Z` → `safe Z`, `XY`
/// stays).
fn lowered(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if !second.is_uppercase() => {
            first.to_lowercase().chain(text.chars().skip(1)).collect()
        }
        _ => text.to_owned(),
    }
}

/// `text` with its first letter capital, as a header's summary starts.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowered_keeps_acronyms() {
        assert_eq!(lowered("Safe Z"), "safe Z");
        assert_eq!(lowered("Step-down"), "step-down");
        assert_eq!(lowered("XY zero"), "XY zero");
        assert_eq!(lowered(""), "");
        assert_eq!(capitalised("safe Z 5 mm"), "Safe Z 5 mm");
        assert_eq!(capitalised(""), "");
    }
}
