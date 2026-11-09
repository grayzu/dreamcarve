// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The project's tools (functional spec, capability 63; acceptance 71):
//! what the CAM panel's Tools tab reads and the commands it makes. A
//! field edit is a whole tool through [`set_field`] and
//! [`Command::UpdateTool`]; a tool from the library is the copy `tools
//! add` writes ([`add_from_library`]); *Duplicate* is a copy under the
//! next id; *Save to library* is [`dc_library::add`] on the library the
//! shell read. Feed, plunge rate and RPM are never filled in: an unset
//! one shows empty, with the file's message ([`unset_rates`]), until the
//! user types it (principle 5; S20).

use std::collections::BTreeSet;

use dc_job::Finding;
use dc_library::{AddOutcome, Library, LibraryError, LibraryToolId};
use dc_project::{Problem, Project, Tool, ToolId, ToolKind, Units};

use crate::command::{Command, copy_name};
use crate::error::CommandError;
use crate::library::trimmed;
use crate::units;

/// The tool fields the editor shows, in its order (capability 63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolField {
    /// `name`.
    Name,
    /// `diameter_mm`.
    Diameter,
    /// A V-bit's `included_angle_deg`.
    Angle,
    /// `flutes`.
    Flutes,
    /// `feed_mm_min`.
    Feed,
    /// `plunge_mm_min`.
    Plunge,
    /// `rpm`.
    Rpm,
    /// `step_down_mm`.
    StepDown,
    /// `step_over_fraction`, shown as a percentage.
    StepOver,
    /// `number`, the `T` number.
    Number,
    /// `vendor`.
    Vendor,
    /// `product_id`.
    ProductId,
}

impl ToolField {
    /// The field's name in the file, for the label's help line.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Diameter => "diameter_mm",
            Self::Angle => "included_angle_deg",
            Self::Flutes => "flutes",
            Self::Feed => "feed_mm_min",
            Self::Plunge => "plunge_mm_min",
            Self::Rpm => "rpm",
            Self::StepDown => "step_down_mm",
            Self::StepOver => "step_over_fraction",
            Self::Number => "number",
            Self::Vendor => "vendor",
            Self::ProductId => "product_id",
        }
    }
}

/// The cutter types the editor's dropdown offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindChoice {
    /// An end mill.
    EndMill,
    /// A ball nose.
    BallNose,
    /// A V-bit, with its included angle.
    VBit,
    /// An engraver.
    Engraver,
    /// A drill.
    Drill,
}

impl KindChoice {
    /// The choices, in the dropdown's order.
    pub const ALL: [Self; 5] = [
        Self::EndMill,
        Self::BallNose,
        Self::VBit,
        Self::Engraver,
        Self::Drill,
    ];

    /// The choice `kind` is.
    #[must_use]
    pub const fn of(kind: &ToolKind) -> Self {
        match kind {
            ToolKind::EndMill => Self::EndMill,
            ToolKind::BallNose => Self::BallNose,
            ToolKind::VBit { .. } => Self::VBit,
            ToolKind::Engraver => Self::Engraver,
            ToolKind::Drill => Self::Drill,
        }
    }
}

/// What choosing a cutter type in the editor does to a tool.
#[derive(Debug, Clone, PartialEq)]
pub enum KindSwitch {
    /// Nothing: the tool already has that type.
    Unchanged,
    /// The tool with its new type: one [`Command::UpdateTool`].
    Update(Box<Tool>),
    /// The switch to V-bit is held: the project does not change until an
    /// included angle is typed ([`finish_v_bit`]). The angle sets how deep
    /// a V-Carve cuts, so a default would be an invented value, as a feed
    /// would be (S20; #581).
    HoldForAngle,
}

/// What choosing `choice` in the type dropdown does to `tool`. A V-bit
/// keeps its angle; any other tool turned into one is held until its
/// angle is typed.
#[must_use]
pub fn set_kind(tool: &Tool, choice: KindChoice) -> KindSwitch {
    let cutter = match (choice, &tool.cutter) {
        (KindChoice::VBit, ToolKind::VBit { .. }) => return KindSwitch::Unchanged,
        (KindChoice::VBit, _) => return KindSwitch::HoldForAngle,
        (KindChoice::EndMill, _) => ToolKind::EndMill,
        (KindChoice::BallNose, _) => ToolKind::BallNose,
        (KindChoice::Engraver, _) => ToolKind::Engraver,
        (KindChoice::Drill, _) => ToolKind::Drill,
    };
    if cutter == tool.cutter {
        return KindSwitch::Unchanged;
    }
    KindSwitch::Update(Box::new(Tool {
        cutter,
        ..tool.clone()
    }))
}

/// The held switch of `tool` to V-bit ([`KindSwitch::HoldForAngle`]),
/// finished with the included angle typed in its field: the switch and
/// the angle as one command, so one undo step. `None` when the field was
/// left empty, which discards the switch.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not an angle in
/// (0°, 180°).
pub fn finish_v_bit(tool: &Tool, text: &str) -> Result<Option<Command>, CommandError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let included_angle_deg = included_angle(text)?;
    Ok(Some(Command::UpdateTool(Tool {
        cutter: ToolKind::VBit { included_angle_deg },
        ..tool.clone()
    })))
}

/// An included angle as typed, degrees, with or without `°` or `deg`:
/// strictly between 0° and 180°, as V-Carve and Inlay need it.
fn included_angle(text: &str) -> Result<f64, CommandError> {
    let angle = decimal(without(without(text, "°"), "deg"))?;
    if angle > 0.0 && angle < 180.0 {
        Ok(angle)
    } else {
        Err(refused(
            text,
            "an included angle is more than 0° and less than 180°",
        ))
    }
}

/// Decimals a length or rate shows in a field: enough for a thousandth
/// of an inch in mm and a tenth of a thou in inches; trailing zeros go.
const FIELD_DECIMALS: usize = 4;

/// A length or rate in the display units, as a field shows it.
fn display(value_mm: f64, units: Units) -> String {
    trimmed(units::to_display(value_mm, units), FIELD_DECIMALS)
}

/// The text `field` of `tool` shows, without its unit: empty when the
/// file holds `null`. A feed, plunge rate or RPM is never prefilled.
#[must_use]
pub fn field_text(tool: &Tool, field: ToolField, units: Units) -> String {
    let length = |v: Option<f64>| v.map_or_else(String::new, |mm| display(mm, units));
    match field {
        ToolField::Name => tool.name.clone(),
        ToolField::Diameter => display(tool.diameter_mm, units),
        ToolField::Angle => match tool.cutter {
            ToolKind::VBit { included_angle_deg } => trimmed(included_angle_deg, 2),
            _ => String::new(),
        },
        ToolField::Flutes => tool.flutes.to_string(),
        ToolField::Feed => length(tool.feed_mm_min),
        ToolField::Plunge => length(tool.plunge_mm_min),
        ToolField::Rpm => tool.rpm.map_or_else(String::new, |rpm| rpm.to_string()),
        ToolField::StepDown => length(tool.step_down_mm),
        ToolField::StepOver => tool
            .step_over_fraction
            .map_or_else(String::new, |f| trimmed(f * 100.0, 2)),
        ToolField::Number => tool.number.map_or_else(String::new, |n| n.to_string()),
        ToolField::Vendor => tool.vendor.clone().unwrap_or_default(),
        ToolField::ProductId => tool.product_id.clone().unwrap_or_default(),
    }
}

/// The unit `field` shows after its value, inside the field: `mm` or
/// `in`, `mm/min` or `in/min`, `RPM`, `%`, `°`, or nothing.
#[must_use]
pub const fn field_unit(field: ToolField, units: Units) -> &'static str {
    match field {
        ToolField::Diameter | ToolField::StepDown => units::suffix(units),
        ToolField::Feed | ToolField::Plunge => units::rate_suffix(units),
        ToolField::Rpm => "RPM",
        ToolField::StepOver => "%",
        ToolField::Angle => "°",
        ToolField::Name
        | ToolField::Flutes
        | ToolField::Number
        | ToolField::Vendor
        | ToolField::ProductId => "",
    }
}

/// A refusal for `text`, with `reason`.
fn refused(text: &str, reason: impl Into<String>) -> CommandError {
    CommandError::NotAQuantity {
        text: text.to_owned(),
        reason: reason.into(),
    }
}

/// `text` without a trailing `suffix` (any case), trimmed.
fn without<'a>(text: &'a str, suffix: &str) -> &'a str {
    let trimmed = text.trim();
    let cut = trimmed.len().saturating_sub(suffix.len());
    match (trimmed.get(..cut), trimmed.get(cut..)) {
        (Some(head), Some(tail)) if tail.eq_ignore_ascii_case(suffix) => head.trim_end(),
        _ => trimmed,
    }
}

/// A whole number that fits the field's type, as typed.
fn whole<T: std::str::FromStr>(text: &str) -> Result<T, CommandError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(refused(text, "nothing was typed"));
    }
    trimmed
        .parse::<T>()
        .map_err(|_| refused(text, format!("`{trimmed}` is not a whole number in range")))
}

/// A decimal number, as typed: a dot for the decimal point.
fn decimal(text: &str) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(refused(text, "nothing was typed"));
    }
    if trimmed.contains(',') {
        return Err(refused(text, "use a dot for the decimal point"));
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => Err(refused(text, format!("`{trimmed}` is not a number"))),
    }
}

/// `None` for blank text, else `parse` of it: an optional field cleared
/// back to `null`.
fn optional<T>(
    text: &str,
    parse: impl FnOnce(&str) -> Result<T, CommandError>,
) -> Result<Option<T>, CommandError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        parse(text).map(Some)
    }
}

/// `tool` with `field` set from `text`, typed in a project whose display
/// units are `units` (capability 53): a length or rate with a unit means
/// what it means in a file, a bare one is in `units`; a step-over is a
/// percentage; blank text clears an optional field to `null`.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a value of the
/// field's kind, or is blank for a field the file requires (name,
/// diameter, flutes, angle).
pub fn set_field(
    tool: &Tool,
    field: ToolField,
    text: &str,
    units: Units,
) -> Result<Tool, CommandError> {
    let mut next = tool.clone();
    let words = |t: &str| {
        let t = t.trim();
        (!t.is_empty()).then(|| t.to_owned())
    };
    match field {
        ToolField::Name => {
            next.name = words(text).ok_or_else(|| refused(text, "a tool needs a name"))?;
        }
        ToolField::Diameter => next.diameter_mm = units::parse_length_mm(text, units)?,
        ToolField::Angle => {
            if let ToolKind::VBit { .. } = tool.cutter {
                next.cutter = ToolKind::VBit {
                    included_angle_deg: included_angle(text)?,
                };
            }
        }
        ToolField::Flutes => next.flutes = whole(text)?,
        ToolField::Feed => {
            next.feed_mm_min = optional(text, |t| units::parse_rate_mm_min(t, units))?;
        }
        ToolField::Plunge => {
            next.plunge_mm_min = optional(text, |t| units::parse_rate_mm_min(t, units))?;
        }
        ToolField::Rpm => next.rpm = optional(text, |t| whole(without(t, "rpm")))?,
        ToolField::StepDown => {
            next.step_down_mm = optional(text, |t| units::parse_length_mm(t, units))?;
        }
        ToolField::StepOver => {
            next.step_over_fraction =
                optional(text, |t| decimal(without(t, "%")))?.map(|percent| percent / 100.0);
        }
        ToolField::Number => next.number = optional(text, whole)?,
        ToolField::Vendor => next.vendor = words(text),
        ToolField::ProductId => next.product_id = words(text),
    }
    Ok(next)
}

/// The rates export needs that `tool` lacks, each with the file's message
/// (`dc_project::Problem::ToolValueUnset`, the line `check` prints):
/// feed, plunge rate, RPM, in that order. Empty when all three are set.
#[must_use]
pub fn unset_rates(tool: &Tool) -> Vec<(&'static str, String)> {
    [
        ("feed_mm_min", tool.feed_mm_min.is_none()),
        ("plunge_mm_min", tool.plunge_mm_min.is_none()),
        ("rpm", tool.rpm.is_none()),
    ]
    .into_iter()
    .filter(|(_, unset)| *unset)
    .map(|(field, _)| {
        let problem = Problem::ToolValueUnset {
            tool: tool.id.clone(),
            name: tool.name.clone(),
            field,
        };
        (field, problem.to_string())
    })
    .collect()
}

/// Whether `tool` lacks a feed, plunge rate or RPM: the row's red mark
/// (capability 63).
#[must_use]
pub fn lacks_rate(tool: &Tool) -> bool {
    tool.feed_mm_min.is_none() || tool.plunge_mm_min.is_none() || tool.rpm.is_none()
}

/// The names of the operations that cut with tool `id` (their own tool, a
/// clearing pass's, a cut-out's), in project order: what *Remove*'s
/// refusal names.
#[must_use]
pub fn users(project: &Project, id: &ToolId) -> Vec<String> {
    project
        .operations
        .iter()
        .filter(|op| op.run_tools().contains(&id))
        .map(|op| op.name().to_owned())
        .collect()
}

/// The `unused_tool` hint about tool `id` among `findings`, as `check`
/// prints it, when there is one.
#[must_use]
pub fn unused_hint<'a>(findings: &'a [Finding], id: &ToolId) -> Option<&'a Finding> {
    let field = format!("tools[{id}]");
    findings
        .iter()
        .find(|f| f.code.as_deref() == Some("unused_tool") && f.field.as_deref() == Some(&field))
}

/// *Add to project*: the ticked library tools copied into `project` under
/// new ids, as `tools add` copies them, as one command; `None` when
/// nothing is ticked.
#[must_use]
pub fn add_from_library(
    project: &Project,
    lib: &Library,
    selected: &BTreeSet<LibraryToolId>,
) -> Option<Command> {
    let tools = crate::library::build_add(lib, &project.tools, selected);
    (!tools.is_empty()).then_some(Command::AddTools(tools))
}

/// *Duplicate*: a copy of tool `id` under the next free project id, named
/// `<name> (2)` or the next free suffix, every value kept.
///
/// # Errors
///
/// [`CommandError::UnknownTool`] when the project has no such tool.
pub fn duplicate(project: &Project, id: &ToolId) -> Result<Command, CommandError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == *id)
        .ok_or_else(|| CommandError::UnknownTool { id: id.clone() })?;
    let taken: Vec<&str> = project.tools.iter().map(|t| t.name.as_str()).collect();
    let copy = Tool {
        name: copy_name(&taken, &tool.name),
        ..dc_library::project_copy(&project.tools, tool)
    };
    Ok(Command::AddTools(vec![copy]))
}

/// *Save to library*: `lib` with a copy of project tool `tool` added as
/// [`dc_library::add`] adds it (a new `lt<n>` id, no source, a name made
/// unique), and what that did. The project is not changed; the shell
/// writes the library file.
///
/// # Errors
///
/// [`LibraryError`] when the library refuses the tool, naming the field.
pub fn save_to_library(lib: &Library, tool: &Tool) -> Result<(Library, AddOutcome), LibraryError> {
    let mut next = lib.clone();
    let outcome = dc_library::add(&mut next, tool.clone(), None)?;
    Ok((next, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes_come_off_in_any_case() {
        assert_eq!(without("18000 RPM", "rpm"), "18000");
        assert_eq!(without("40 %", "%"), "40");
        assert_eq!(without("40", "%"), "40");
        assert_eq!(without("90°", "°"), "90");
        assert_eq!(without("°", "°"), "");
    }

    #[test]
    fn numbers_refuse_what_they_cannot_read() {
        assert!(whole::<u32>("2.5").is_err());
        assert!(whole::<u8>("300").is_err());
        assert!(decimal("6,35").is_err());
        assert!(decimal("inf").is_err());
        assert!(decimal(" 45 ").is_ok());
    }
}
