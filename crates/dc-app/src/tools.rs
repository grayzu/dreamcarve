// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The project's tools (functional spec, capability 63; acceptance 71):
//! what the Tools step reads and the commands it makes. A
//! field edit is a whole tool through [`set_field`] and
//! [`Command::UpdateTool`]; a tool from the library is the copy `tools
//! add` writes ([`add_from_library`]); *Duplicate* is a copy under the
//! next id; *New tool* is `init`'s placeholder named `tool<n>` ([`new_tool`]);
//! *Save to library* is [`dc_library::add`] on the library the
//! shell read. Feed, plunge rate and RPM are never filled in: an unset
//! one shows empty, with the file's message ([`unset_rates`]), until the
//! user types it (principle 5; S20).
//!
//! A tapered ball nose (format 28; #685) is a type of its own in the
//! dropdown, with three fields after the type: tip diameter, taper per
//! side and flute length ([`fields_for`]). `diameter_mm` keeps its label
//! and is its cutting diameter. Choosing the type writes the cutter with
//! nothing set, and each value it still needs is flagged at its field
//! ([`required`], from [`Tool::unset_values`]) as an unset rate is. A
//! row's second line ([`row_detail`]) and the library's table say which
//! angle they show: a V-bit's included, a tapered ball nose's per side
//! ([`angle_text`]).

use std::collections::BTreeSet;

use dc_job::Finding;
use dc_library::{AddOutcome, Library, LibraryError, LibraryToolId};
use dc_project::{
    MAX_TAPER_ANGLE_DEG, Problem, Project, TBN_FLUTES, TBN_TAPER, TBN_TIP, Tool, ToolId, ToolKind,
    Units, UnsetValue,
};

use crate::command::{Command, copy_name};
use crate::error::CommandError;
use crate::library::trimmed;
use crate::strings::{StrKey, Strings};
use crate::units;

/// The tool fields the editor shows (capability 63); [`fields_for`] gives
/// a type's, in the editor's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolField {
    /// `name`.
    Name,
    /// `diameter_mm`: the cutting diameter, for every type.
    Diameter,
    /// A V-bit's `included_angle_deg`.
    Angle,
    /// A tapered ball nose's `tip_diameter_mm`.
    TipDiameter,
    /// A tapered ball nose's `taper_angle_deg`, per side.
    Taper,
    /// A tapered ball nose's `flute_length_mm`.
    FluteLength,
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
    /// The field's name in the file, for the label's help line. A tapered
    /// ball nose's values are named where validation and
    /// [`Tool::unset_values`] locate them
    /// (`cutter.tapered_ball_nose.tip_diameter_mm`).
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Diameter => "diameter_mm",
            Self::Angle => "included_angle_deg",
            Self::TipDiameter => TBN_TIP,
            Self::Taper => TBN_TAPER,
            Self::FluteLength => TBN_FLUTES,
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

    /// The field's label in the editor. *Diameter* is every type's: a
    /// tapered ball nose's is its cutting diameter, and its taper says
    /// *per side*, since a V-bit's angle is included.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::Name => StrKey::AppToolsName,
            Self::Diameter => StrKey::AppToolsDiameter,
            Self::Angle => StrKey::AppToolsAngle,
            Self::TipDiameter => StrKey::AppToolsTipDiameter,
            Self::Taper => StrKey::AppToolsTaper,
            Self::FluteLength => StrKey::AppToolsFluteLength,
            Self::Flutes => StrKey::AppToolsFlutes,
            Self::Feed => StrKey::AppToolsFeed,
            Self::Plunge => StrKey::AppToolsPlunge,
            Self::Rpm => StrKey::AppToolsRpm,
            Self::StepDown => StrKey::AppToolsStepDown,
            Self::StepOver => StrKey::AppToolsStepOver,
            Self::Number => StrKey::AppToolsToolNumber,
            Self::Vendor => StrKey::AppToolsVendor,
            Self::ProductId => StrKey::AppToolsProductId,
        }
    }

    /// Every field a tool of any type has, in the editor's order of a
    /// tapered ball nose and then a V-bit's own.
    pub const ALL: [Self; 15] = [
        Self::Name,
        Self::Angle,
        Self::TipDiameter,
        Self::Taper,
        Self::FluteLength,
        Self::Diameter,
        Self::Flutes,
        Self::Feed,
        Self::Plunge,
        Self::Rpm,
        Self::StepDown,
        Self::StepOver,
        Self::Number,
        Self::Vendor,
        Self::ProductId,
    ];

    /// The field's help: what it is, its unit, the values it takes and
    /// what leaving it empty means. Its label and the field show it on
    /// hover, and the panel's help line while it is typed in.
    #[must_use]
    pub const fn help(self) -> StrKey {
        match self {
            Self::Name => StrKey::AppToolsNameHelp,
            Self::Diameter => StrKey::AppToolsDiameterHelp,
            Self::Angle => StrKey::AppToolsAngleHelp,
            Self::TipDiameter => StrKey::AppToolsTipDiameterHelp,
            Self::Taper => StrKey::AppToolsTaperHelp,
            Self::FluteLength => StrKey::AppToolsFluteLengthHelp,
            Self::Flutes => StrKey::AppToolsFlutesHelp,
            Self::Feed => StrKey::AppToolsFeedHelp,
            Self::Plunge => StrKey::AppToolsPlungeHelp,
            Self::Rpm => StrKey::AppToolsRpmHelp,
            Self::StepDown => StrKey::AppToolsStepDownHelp,
            Self::StepOver => StrKey::AppToolsStepOverHelp,
            Self::Number => StrKey::AppToolsToolNumberHelp,
            Self::Vendor => StrKey::AppToolsVendorHelp,
            Self::ProductId => StrKey::AppToolsProductIdHelp,
        }
    }

    /// The field [`Tool::unset_values`] names `file_name`, when it is one
    /// a tool may still need: a tapered ball nose's values and the rates.
    fn of_unset(file_name: &str) -> Option<Self> {
        [
            Self::TipDiameter,
            Self::Taper,
            Self::FluteLength,
            Self::Feed,
            Self::Plunge,
            Self::Rpm,
        ]
        .into_iter()
        .find(|field| field.file_name() == file_name)
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
    /// A tapered ball nose (format 28; #685), with its tip diameter,
    /// taper per side and flute length.
    TaperedBallNose,
}

impl KindChoice {
    /// The choices, in the dropdown's order.
    pub const ALL: [Self; 6] = [
        Self::EndMill,
        Self::BallNose,
        Self::VBit,
        Self::Engraver,
        Self::Drill,
        Self::TaperedBallNose,
    ];

    /// The type's name, in the dropdown and the tables.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::EndMill => StrKey::AppKindEndMill,
            Self::BallNose => StrKey::AppKindBallNose,
            Self::VBit => StrKey::AppKindVBit,
            Self::Engraver => StrKey::AppKindEngraver,
            Self::Drill => StrKey::AppKindDrill,
            Self::TaperedBallNose => StrKey::AppKindTaperedBallNose,
        }
    }

    /// The choice `kind` is.
    #[must_use]
    pub const fn of(kind: &ToolKind) -> Self {
        match kind {
            ToolKind::EndMill => Self::EndMill,
            ToolKind::BallNose => Self::BallNose,
            ToolKind::VBit { .. } => Self::VBit,
            ToolKind::Engraver => Self::Engraver,
            ToolKind::Drill => Self::Drill,
            ToolKind::TaperedBallNose { .. } => Self::TaperedBallNose,
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
/// angle is typed. A tapered ball nose keeps its values; any other tool
/// turned into one gets the cutter with its tip, taper and flute length
/// unset (#685): the file holds `null` for each, so nothing is invented
/// and nothing is held, and each shows as required ([`required`]) until
/// it is typed.
#[must_use]
pub fn set_kind(tool: &Tool, choice: KindChoice) -> KindSwitch {
    let cutter = match (choice, &tool.cutter) {
        (KindChoice::VBit, ToolKind::VBit { .. })
        | (KindChoice::TaperedBallNose, ToolKind::TaperedBallNose { .. }) => {
            return KindSwitch::Unchanged;
        }
        (KindChoice::VBit, _) => return KindSwitch::HoldForAngle,
        (KindChoice::TaperedBallNose, _) => ToolKind::TaperedBallNose {
            tip_diameter_mm: None,
            taper_angle_deg: None,
            flute_length_mm: None,
        },
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

/// A tapered ball nose's taper as typed, degrees per side, with or
/// without `°`, `deg` or `/side`: more than 0° and at most
/// [`MAX_TAPER_ANGLE_DEG`], as the file takes it.
fn taper_angle(text: &str) -> Result<f64, CommandError> {
    let angle = decimal(without(without(without(text, "/side"), "°"), "deg"))?;
    if angle > 0.0 && angle <= MAX_TAPER_ANGLE_DEG {
        Ok(angle)
    } else {
        Err(refused(
            text,
            format!("a taper per side is more than 0° and at most {MAX_TAPER_ANGLE_DEG}°"),
        ))
    }
}

/// The rows every type has after its own, in the editor's order.
const COMMON_FIELDS: [ToolField; 10] = [
    ToolField::Diameter,
    ToolField::Flutes,
    ToolField::Feed,
    ToolField::Plunge,
    ToolField::Rpm,
    ToolField::StepDown,
    ToolField::StepOver,
    ToolField::Number,
    ToolField::Vendor,
    ToolField::ProductId,
];

/// The editor's rows for a tool of type `choice`, in order (capability
/// 63): the name, the type's own values (a V-bit's angle; a tapered ball
/// nose's tip diameter, taper and flute length), then the rows every type
/// has. The type dropdown sits after the name; the editor draws it.
#[must_use]
pub fn fields_for(choice: KindChoice) -> Vec<ToolField> {
    let own: &[ToolField] = match choice {
        KindChoice::VBit => &[ToolField::Angle],
        KindChoice::TaperedBallNose => &[
            ToolField::TipDiameter,
            ToolField::Taper,
            ToolField::FluteLength,
        ],
        KindChoice::EndMill | KindChoice::BallNose | KindChoice::Engraver | KindChoice::Drill => {
            &[]
        }
    };
    std::iter::once(ToolField::Name)
        .chain(own.iter().copied())
        .chain(COMMON_FIELDS)
        .collect()
}

/// The editor's rows for `tool` ([`fields_for`] its type). While a switch
/// to V-bit is held ([`KindSwitch::HoldForAngle`]) the editor draws
/// `fields_for(KindChoice::VBit)` instead.
#[must_use]
pub fn fields(tool: &Tool) -> Vec<ToolField> {
    fields_for(KindChoice::of(&tool.cutter))
}

/// One row of the tool editor, ready to draw: the Tools step's Inspector
/// and the library's editing pane show the same rows (capabilities 63,
/// 64a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    /// The field.
    pub field: ToolField,
    /// Its label.
    pub label: StrKey,
    /// Its help ([`ToolField::help`]).
    pub help: StrKey,
    /// What the field shows, without its unit: empty for `null`.
    pub text: String,
    /// The unit drawn after the value ([`field_unit`]).
    pub unit: &'static str,
    /// The file's message when the tool still needs this value
    /// ([`required`]): the field takes the required-field look.
    pub required: Option<String>,
}

/// `tool`'s editor rows in `units` ([`fields`]), each with its label, its
/// text and unit, and whether it is still required.
#[must_use]
pub fn rows(tool: &Tool, units: Units) -> Vec<FieldRow> {
    rows_for(tool, KindChoice::of(&tool.cutter), units)
}

/// The editor's rows for `tool` drawn as a tool of type `choice`
/// ([`fields_for`]): [`rows`] for its own type, and, while its switch to
/// V-bit is held ([`KindSwitch::HoldForAngle`]), a V-bit's rows over the
/// tool the project still holds, whose *Angle* is then empty.
#[must_use]
pub fn rows_for(tool: &Tool, choice: KindChoice, units: Units) -> Vec<FieldRow> {
    let unset = unset_values(tool);
    fields_for(choice)
        .into_iter()
        .map(|field| FieldRow {
            field,
            label: field.label(),
            help: field.help(),
            text: field_text(tool, field, units),
            unit: field_unit(field, units),
            required: unset
                .iter()
                .find(|(name, _)| *name == field.file_name())
                .map(|(_, message)| message.clone()),
        })
        .collect()
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
    let (tip_mm, taper_deg, flute_length_mm) = tapered_values(tool);
    match field {
        ToolField::Name => tool.name.clone(),
        ToolField::Diameter => display(tool.diameter_mm, units),
        ToolField::Angle => match tool.cutter {
            ToolKind::VBit { included_angle_deg } => trimmed(included_angle_deg, 2),
            _ => String::new(),
        },
        ToolField::TipDiameter => length(tip_mm),
        ToolField::Taper => taper_deg.map_or_else(String::new, |deg| trimmed(deg, FIELD_DECIMALS)),
        ToolField::FluteLength => length(flute_length_mm),
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
        ToolField::Diameter
        | ToolField::StepDown
        | ToolField::TipDiameter
        | ToolField::FluteLength => units::suffix(units),
        ToolField::Feed | ToolField::Plunge => units::rate_suffix(units),
        ToolField::Rpm => "RPM",
        ToolField::StepOver => "%",
        ToolField::Angle | ToolField::Taper => "°",
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
/// percentage; blank text clears an optional field to `null`. A tapered
/// ball nose's tip diameter, taper and flute length are optional in the
/// file, so blank clears each; on any other type they change nothing, as
/// the angle does on a tool that is not a V-bit.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a value of the
/// field's kind (a taper outside (0°, 45°] among them), or is blank for a
/// field the file requires (name, diameter, flutes, angle).
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
        ToolField::TipDiameter => {
            if let ToolKind::TaperedBallNose {
                tip_diameter_mm, ..
            } = &mut next.cutter
            {
                *tip_diameter_mm = optional(text, |t| units::parse_length_mm(t, units))?;
            }
        }
        ToolField::Taper => {
            if let ToolKind::TaperedBallNose {
                taper_angle_deg, ..
            } = &mut next.cutter
            {
                *taper_angle_deg = optional(text, taper_angle)?;
            }
        }
        ToolField::FluteLength => {
            if let ToolKind::TaperedBallNose {
                flute_length_mm, ..
            } = &mut next.cutter
            {
                *flute_length_mm = optional(text, |t| units::parse_length_mm(t, units))?;
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

/// Decimal places a distance between passes shows, and one in the last
/// of them: a thousandth of a millimetre, a tenth of a thou.
const fn pass_decimals(units: Units) -> (usize, f64) {
    match units {
        Units::Mm => (3, 0.001),
        Units::Inch => (4, 0.0001),
    }
}

/// `distance_mm` between passes as a stepover's note says it in `units`
/// (`0.508 mm between passes`); a distance too small for the figures
/// shown is said as less than their last place, never as 0. `None` for a
/// distance that is not above 0.
#[must_use]
pub fn between_passes(strings: &Strings, distance_mm: f64, units: Units) -> Option<String> {
    if !(distance_mm.is_finite() && distance_mm > 0.0) {
        return None;
    }
    let (decimals, least) = pass_decimals(units);
    let shown = trimmed(units::to_display(distance_mm, units), decimals);
    let unit = units::suffix(units);
    Some(if shown == "0" {
        strings.fmt(
            StrKey::AppToolsStepOverNoteTiny,
            &[("distance", &trimmed(least, decimals)), ("unit", unit)],
        )
    } else {
        strings.fmt(
            StrKey::AppToolsStepOverNote,
            &[("distance", &shown), ("unit", unit)],
        )
    })
}

/// The note beside `tool`'s Stepover field while it holds `typed` (the
/// smoke test's item 7): the distance between passes that percentage of
/// the tool's diameter comes to, in `units`. `0.08 %` of a 6.35 mm bit
/// reads `0.005 mm between passes`, where `8 %` reads `0.508 mm`. `None`
/// while the field is empty or holds no percentage.
#[must_use]
pub fn step_over_note(strings: &Strings, tool: &Tool, typed: &str, units: Units) -> Option<String> {
    if typed.trim().is_empty() {
        return None;
    }
    let fraction = set_field(tool, ToolField::StepOver, typed, units)
        .ok()?
        .step_over_fraction?;
    between_passes(strings, fraction * tool.diameter_mm, units)
}

/// The findings located at `tool`'s own fields, each with the field it is
/// under, errors before warnings before hints, one a field: what the tool
/// editor shows under that field, as the operation editor shows a located
/// finding under its row. A value the tool still needs is left out: its
/// field says *Required*, and the file's message is its tooltip (66b).
#[must_use]
pub fn field_status<'a>(
    project: &Project,
    findings: &'a [Finding],
    tool: &Tool,
) -> Vec<(ToolField, crate::project::FieldStatus<'a>)> {
    let unset = unset_values(tool);
    let mut found: Vec<(ToolField, crate::project::FieldStatus<'a>)> = Vec::new();
    for tier in [
        dc_job::Tier::Error,
        dc_job::Tier::Warning,
        dc_job::Tier::Hint,
    ] {
        for finding in findings.iter().filter(|f| f.tier == tier) {
            let Some(path) = finding.field.as_deref() else {
                continue;
            };
            let Some((id, Some(field))) = located(project, path) else {
                continue;
            };
            let needed = unset.iter().any(|(name, _)| *name == field.file_name());
            if id != tool.id || needed || found.iter().any(|(f, _)| *f == field) {
                continue;
            }
            found.push((
                field,
                crate::project::FieldStatus {
                    tier,
                    message: &finding.message,
                },
            ));
        }
    }
    found
}

/// The rates export needs that `tool` lacks, each with the file's message
/// (`dc_project::Problem::ToolValueUnset`, the line `check` prints):
/// feed, plunge rate, RPM, in that order. Empty when all three are set.
#[must_use]
pub fn unset_rates(tool: &Tool) -> Vec<(&'static str, String)> {
    unset_with_messages(tool, |unset| !unset.is_cutter())
}

/// Whether `tool` lacks a feed, plunge rate or RPM.
#[must_use]
pub fn lacks_rate(tool: &Tool) -> bool {
    tool.feed_mm_min.is_none() || tool.plunge_mm_min.is_none() || tool.rpm.is_none()
}

/// The values of `tool` that pass `keep`, of those it still needs
/// ([`Tool::unset_values`]), each with the file's message.
fn unset_with_messages(
    tool: &Tool,
    keep: impl Fn(&UnsetValue) -> bool,
) -> Vec<(&'static str, String)> {
    tool.unset_values()
        .into_iter()
        .filter(keep)
        .map(|unset| {
            let problem = Problem::ToolValueUnset {
                tool: tool.id.clone(),
                name: tool.name.clone(),
                field: unset.field,
            };
            (unset.field, problem.to_string())
        })
        .collect()
}

/// Every value `tool` still needs before an enabled operation may use it
/// ([`Tool::unset_values`]; capability 66b; #685), in the file's order, a
/// tapered ball nose's tip, taper and flute length and then the rates:
/// each by its name in the file ([`ToolField::file_name`]) with the
/// file's message (`tools[t3].cutter.tapered_ball_nose.flute_length_mm is
/// not set (mm) for tool …`, the line `check` prints once an operation
/// uses the tool). Listed whether or not one does yet, so the field is
/// marked from the moment the tool is made. `diameter_mm` is never among
/// them: the file always holds it.
#[must_use]
pub fn unset_values(tool: &Tool) -> Vec<(&'static str, String)> {
    unset_with_messages(tool, |_| true)
}

/// The file's message for `field` of `tool` when the tool still needs it
/// ([`unset_values`]): the field shows the required-field look with this
/// message. `None` for a value that is set, and for a field that is
/// never required (the diameter, which is always present, among them).
#[must_use]
pub fn required(tool: &Tool, field: ToolField) -> Option<String> {
    unset_values(tool)
        .into_iter()
        .find(|(name, _)| *name == field.file_name())
        .map(|(_, message)| message)
}

/// The editor's fields `tool` still needs ([`unset_values`]), in the
/// file's order.
#[must_use]
pub fn unset_fields(tool: &Tool) -> Vec<ToolField> {
    tool.unset_values()
        .iter()
        .filter_map(|unset| ToolField::of_unset(unset.field))
        .collect()
}

/// Whether `tool` still needs a value ([`unset_values`]): a rate, or a
/// tapered ball nose's tip, taper or flute length. The row's red mark
/// (capability 63).
#[must_use]
pub fn lacks_value(tool: &Tool) -> bool {
    !tool.unset_values().is_empty()
}

/// A tapered ball nose's tip diameter, taper per side and flute length;
/// each `None` while unset, and all three for any other cutter.
const fn tapered_values(tool: &Tool) -> (Option<f64>, Option<f64>, Option<f64>) {
    match tool.cutter {
        ToolKind::TaperedBallNose {
            tip_diameter_mm,
            taper_angle_deg,
            flute_length_mm,
        } => (tip_diameter_mm, taper_angle_deg, flute_length_mm),
        _ => (None, None, None),
    }
}

/// Decimals an angle shows in a row or a table cell: a glance, not the
/// field.
const ANGLE_DECIMALS: usize = 2;

/// A tapered ball nose's tip diameter as a row or a table cell shows it
/// in `units`, with no unit (`0.762`); `None` for any other cutter, and
/// while the tip is unset.
#[must_use]
pub fn tip_text(tool: &Tool, units: Units) -> Option<String> {
    tapered_values(tool).0.map(|mm| display(mm, units))
}

/// The angle of `tool` as a row or a table cell shows it, saying which
/// angle it is (#685): a V-bit's included angle, `90°`; a tapered ball
/// nose's taper, per side, `6.7°/side`. One column holding 90° and 6.7°
/// unlabelled is the mix-up phase 3's research warns of. `None` for a
/// cutter with no angle, and while a taper is unset.
#[must_use]
pub fn angle_text(strings: &Strings, tool: &Tool) -> Option<String> {
    let (key, angle_deg) = match tool.cutter {
        ToolKind::VBit { included_angle_deg } => {
            (StrKey::AppToolsAngleIncluded, included_angle_deg)
        }
        ToolKind::TaperedBallNose {
            taper_angle_deg, ..
        } => (StrKey::AppToolsAnglePerSide, taper_angle_deg?),
        _ => return None,
    };
    Some(strings.fmt(key, &[("angle", &trimmed(angle_deg, ANGLE_DECIMALS))]))
}

/// The second line of `tool`'s row in the Tools step (capability 63), in
/// `units`: its diameter, which is the cutting diameter for every type; a
/// tapered ball nose's tip; its angle, labelled ([`angle_text`]); its
/// number. `6.35 mm · tip 0.762 mm · 6.7°/side · T2`. An unset value is
/// left out: the row's red mark says it is missing ([`lacks_value`]).
#[must_use]
pub fn row_detail(strings: &Strings, tool: &Tool, units: Units) -> String {
    let suffix = units::suffix(units);
    let mut parts = vec![format!("{} {suffix}", display(tool.diameter_mm, units))];
    if let Some(tip) = tip_text(tool, units) {
        let value = format!("{tip} {suffix}");
        parts.push(strings.fmt(StrKey::AppToolsDetailTip, &[("value", &value)]));
    }
    parts.extend(angle_text(strings, tool));
    if let Some(number) = tool.number {
        parts.push(strings.fmt(StrKey::AppToolsNumber, &[("number", &number.to_string())]));
    }
    parts.join(" · ")
}

/// The names of the operations that cut with tool `id` (their own tool, a
/// clearing pass's, a cut-out's), in project order: what *Delete*'s
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

/// The project tool a finding's field path names, as validation writes
/// it (`tools[t1].feed_mm_min`, `tools[t3].cutter.v_bit.included_angle_deg`,
/// or `tools[t1]` alone for `unused_tool`), and which of the editor's
/// fields the path is about, when it is one the tool's type shows: what
/// a Warnings row about a tool opens and focuses (capability 76). `None`
/// when the path is not a tool's or the project has no such tool.
#[must_use]
pub fn located(project: &Project, path: &str) -> Option<(ToolId, Option<ToolField>)> {
    let (id, rest) = path.strip_prefix("tools[")?.split_once(']')?;
    let tool = project.tools.iter().find(|t| t.id.as_str() == id)?;
    let field = rest.strip_prefix('.').and_then(|name| {
        fields(tool).into_iter().find(|field| {
            // A cutter's value is named under its variant.
            name.strip_suffix(field.file_name())
                .is_some_and(|before| before.is_empty() || before.ends_with('.'))
        })
    });
    Some((tool.id.clone(), field))
}

/// Whether `finding` names tool `id`: its field is the tool's
/// (`tools[t1]`) or one of the tool's (`tools[t1].rpm`).
fn names(finding: &Finding, id: &ToolId) -> bool {
    finding
        .field
        .as_deref()
        .and_then(|field| field.strip_prefix("tools["))
        .and_then(|rest| rest.strip_prefix(id.as_str()))
        .and_then(|rest| rest.strip_prefix(']'))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// The mark in a tool row's status cell (capability 63; `experience.md`,
/// section 7, "Status cell"): the worst of what is said of the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMark {
    /// Nothing to say.
    Clean,
    /// Only the `unused_tool` hint: the `info` glyph.
    Hint,
    /// This many warnings name it: the warning glyph and the count, as
    /// an operation's row shows its own.
    Warnings(usize),
    /// It still needs a value ([`lacks_value`]): the red dot.
    LacksValue,
    /// An error names it that is not a value still to set (a diameter
    /// out of range): the error mark of an operation's row.
    Error,
}

/// A tool row's status cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowStatus {
    /// The mark.
    pub mark: RowMark,
    /// What hovering it lists: the errors that name the tool, the values
    /// it still needs, then the warnings, then the hint, each as the
    /// Warnings drawer words it.
    pub messages: Vec<String>,
}

/// The status cell of `tool`'s row, from `findings` ([`App::warnings`]):
/// what the findings that name the tool come to, with the values it
/// still needs, which mark it whether or not an operation uses it yet.
///
/// [`App::warnings`]: crate::App::warnings
#[must_use]
pub fn row_status(tool: &Tool, findings: &[Finding]) -> RowStatus {
    let unset = unset_values(tool);
    let mine = |tier: dc_job::Tier| {
        findings
            .iter()
            .filter(move |f| f.tier == tier && names(f, &tool.id))
    };
    // An unset value's error is the dot's, said once, by `unset`.
    let is_unset = |f: &Finding| {
        unset.iter().any(|(name, _)| {
            f.field.as_deref() == Some(format!("tools[{}].{name}", tool.id).as_str())
        })
    };
    let errors: Vec<&Finding> = mine(dc_job::Tier::Error).filter(|f| !is_unset(f)).collect();
    let warnings: Vec<&Finding> = mine(dc_job::Tier::Warning).collect();
    let hints: Vec<&Finding> = mine(dc_job::Tier::Hint).collect();
    let mark = if !errors.is_empty() {
        RowMark::Error
    } else if !unset.is_empty() {
        RowMark::LacksValue
    } else if !warnings.is_empty() {
        RowMark::Warnings(warnings.len())
    } else if !hints.is_empty() {
        RowMark::Hint
    } else {
        RowMark::Clean
    };
    let messages = errors
        .iter()
        .map(|f| f.message.clone())
        .chain(unset.into_iter().map(|(_, message)| message))
        .chain(warnings.iter().chain(&hints).map(|f| f.message.clone()))
        .collect();
    RowStatus { mark, messages }
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

/// *New tool* (capability 63; slice 29, #621): a tool from scratch, for a
/// bit that is in no library. It is `init`'s placeholder (an end mill,
/// 6.35 mm, 2 flutes, no feed, plunge rate, RPM, step-down or stepover:
/// the app invents no rate, S20) under the next free project id, named
/// [`new_tool_name`]. One undo step, *New tool “tool4”*.
#[must_use]
pub fn new_tool(project: &Project) -> Command {
    let id = dc_library::next_project_tool_id(&project.tools);
    Command::NewTool(dc_job::project_from::placeholder_tool(
        id,
        new_tool_name(&project.tools),
    ))
}

/// The name a new tool takes among `tools`: `tool<n>`, where `<n>` is its
/// place in the list (one past the last) or, when that name is taken, the
/// next number whose name is free.
#[must_use]
pub fn new_tool_name(tools: &[Tool]) -> String {
    let taken = |name: &str| tools.iter().any(|t| t.name == name);
    let place = tools.len().saturating_add(1);
    // `tools.len()` names can take at most that many of these numbers.
    (place..=place.saturating_add(tools.len()))
        .map(|n| format!("tool{n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| format!("tool{place}"))
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
