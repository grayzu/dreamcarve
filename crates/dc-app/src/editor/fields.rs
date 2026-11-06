// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/cam/operationFields.ts

//! The operation editor's table (`docs/specs/phase-4/design.md`,
//! "Architecture → The editors"; functional spec, capabilities 66 and 68):
//! for each operation kind, its rows in order, each a `(group, key path,
//! field kind, unit, tool-derived default, validation key, help key)`. The
//! shell walks it with one renderer per [`FieldKind`]; there is no
//! per-kind panel. Upstream's idea, kept: position is a property of the
//! row, so a field's group and order are decided here and only here, and
//! adding a row means naming its group and having a renderer for its kind
//! (Rust's exhaustive `match` on [`FieldKind`] is the compile-time
//! obligation upstream's `Record<OperationFieldId, …>` was).
//!
//! What changed from upstream: the table is per [`Operation`] variant and
//! names the record's own key paths (`entry.ramp.angle_deg`), so a row
//! applies exactly when its path is in the record ([`get`]) and a choice's
//! sub-rows come and go with the variant chosen; a predicate is kept only
//! for a value the file holds and the generator ignores (a countersink's
//! `depth_mm`). The unit follows from the kind ([`Row::unit`]), and the
//! validation key is the key path: a located finding
//! (`operations[op1].depth_mm`) is the row's whose path is the longest that
//! holds it ([`row_for`]), and a tool-derived row also takes its tool's
//! field while it is `null`. PureCutCNC's kinds, passes, feeds and
//! trochoidal fields do not exist here.
//!
//! This slice (#539) fills every kind's Identity, Targets, Tool, depth and
//! Heights rows, and every Profile row; slices 11–15 (#540–#544) add the
//! other kinds' rows, a table edit each, plus a renderer for a block kind
//! where they need one.

use dc_project::{Operation, Project, Tool};
use serde_json::{Value, json};

use crate::operations::Kind;
use crate::strings::StrKey;

pub use super::set::{get, record, set};

/// An editor group, a collapsible section (capability 66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// Name, enabled.
    Identity,
    /// The target shapes.
    Targets,
    /// The tool.
    Tool,
    /// Depths, step-downs, allowances.
    Depth,
    /// How the cut covers its shapes.
    Strategy,
    /// How the bit enters and leaves.
    EntryAndLeads,
    /// Corner relief and tabs.
    CornersAndTabs,
    /// The machining order.
    Order,
    /// Safe Z and retracts.
    Heights,
}

impl Group {
    /// The groups in capability 66's order, the only order they render in.
    pub const ALL: [Self; 9] = [
        Self::Identity,
        Self::Targets,
        Self::Tool,
        Self::Depth,
        Self::Strategy,
        Self::EntryAndLeads,
        Self::CornersAndTabs,
        Self::Order,
        Self::Heights,
    ];

    /// Its heading.
    #[must_use]
    pub const fn title(self) -> StrKey {
        match self {
            Self::Identity => StrKey::EditorGroupIdentity,
            Self::Targets => StrKey::EditorGroupTargets,
            Self::Tool => StrKey::EditorGroupTool,
            Self::Depth => StrKey::EditorGroupDepth,
            Self::Strategy => StrKey::EditorGroupStrategy,
            Self::EntryAndLeads => StrKey::EditorGroupEntryAndLeads,
            Self::CornersAndTabs => StrKey::EditorGroupCornersAndTabs,
            Self::Order => StrKey::EditorGroupOrder,
            Self::Heights => StrKey::EditorGroupHeights,
        }
    }

    /// Its open state's name in `gui.toml`'s `sections`, shared by every
    /// operation (capability 66: "open state is remembered").
    #[must_use]
    pub const fn section(self) -> &'static str {
        match self {
            Self::Identity => "operation.identity",
            Self::Targets => "operation.targets",
            Self::Tool => "operation.tool",
            Self::Depth => "operation.depth",
            Self::Strategy => "operation.strategy",
            Self::EntryAndLeads => "operation.entry",
            Self::CornersAndTabs => "operation.corners",
            Self::Order => "operation.order",
            Self::Heights => "operation.heights",
        }
    }
}

/// A tool value a `null` field defaults to (capability 66: step-down,
/// stepover, pitch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDefault {
    /// The tool's `step_down_mm`.
    StepDown,
    /// The tool's `step_over_fraction`.
    StepOver,
}

impl ToolDefault {
    /// The tool's value, when it has one.
    #[must_use]
    pub const fn of(self, tool: &Tool) -> Option<f64> {
        match self {
            Self::StepDown => tool.step_down_mm,
            Self::StepOver => tool.step_over_fraction,
        }
    }

    /// The tool's field, as validation names it (`tools[t1].step_down_mm`).
    #[must_use]
    pub const fn tool_field(self) -> &'static str {
        match self {
            Self::StepDown => "step_down_mm",
            Self::StepOver => "step_over_fraction",
        }
    }
}

/// A value built for the project at hand: a variant's data, a feature's
/// settings when it is switched on.
pub type Template = fn(&Project) -> Value;

/// What choosing an option writes.
#[derive(Debug, Clone, Copy)]
pub enum OptionValue {
    /// The option's tag as a plain string (`"outside"`).
    Tag,
    /// A one-key object, the tag over the template's data (`{ "ramp": {
    /// "angle_deg": 15.0 } }`).
    Variant(Template),
}

/// One option of a choice.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceOption {
    /// Its name.
    pub label: StrKey,
    /// The file's name for it: the string, or the one-key object's key.
    pub tag: &'static str,
    /// What choosing it writes.
    pub value: OptionValue,
}

impl ChoiceOption {
    /// The value choosing this option writes on `project`.
    #[must_use]
    pub fn value(&self, project: &Project) -> Value {
        match self.value {
            OptionValue::Tag => Value::from(self.tag),
            OptionValue::Variant(data) => {
                let mut variant = serde_json::Map::new();
                variant.insert(self.tag.to_owned(), data(project));
                Value::Object(variant)
            }
        }
    }
}

/// The tag of a choice's value: a string itself, a one-key object's key.
#[must_use]
pub fn tag_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(tag) => Some(tag),
        Value::Object(map) if map.len() == 1 => map.keys().next().map(String::as_str),
        _ => None,
    }
}

/// How a row is edited: one shell renderer each.
#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    /// Free text (the name).
    Text,
    /// A length, mm in the file, display units on screen.
    Length,
    /// A feed rate, mm/min in the file.
    Rate,
    /// An angle in degrees.
    Angle,
    /// A fraction in the file, a percentage on screen.
    Percentage,
    /// A whole number.
    Count,
    /// Seconds.
    Seconds,
    /// One of a few options: segmented for two to four short ones, a
    /// dropdown otherwise.
    Choice(&'static [ChoiceOption]),
    /// A yes or no.
    Checkbox,
    /// A feature that is `null` when off and the template's object when
    /// on; its settings are the rows under its key.
    Toggle(Template),
    /// A project tool, by id (a popover).
    ToolPicker,
    /// Instances, by id: the Targets group.
    InstanceList,
    /// A Profile's tab request (functional spec, capability 69).
    TabRequest,
    /// A carve's clearing passes (capability 70).
    ClearingList,
    /// An Inlay's cut-out.
    CutOut,
    /// An Inlay's male board (capability 68a).
    MaleBoard,
}

impl FieldKind {
    /// Whether the row edits its whole value as one block, so every path
    /// under its key is its own.
    #[must_use]
    pub const fn is_block(self) -> bool {
        matches!(
            self,
            Self::TabRequest | Self::ClearingList | Self::CutOut | Self::MaleBoard
        )
    }
}

/// The unit a row's value shows with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// None.
    None,
    /// The project's length unit.
    Length,
    /// The project's feed-rate unit.
    Rate,
    /// Degrees.
    Degrees,
    /// Per cent.
    Percent,
    /// Seconds.
    Seconds,
}

/// One row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// The section it is in.
    pub group: Group,
    /// Its key path in the operation's record, which is also its
    /// validation key: the `.dcv` field the help names and a located
    /// finding is matched on.
    pub key: &'static str,
    /// How it is edited.
    pub kind: FieldKind,
    /// Its label (the chapters' name).
    pub label: StrKey,
    /// The one-sentence help its label shows on hover.
    pub help: StrKey,
    /// The tool's value a `null` here defaults to.
    pub tool_default: Option<ToolDefault>,
    /// A further rule, for a value the file holds and the generator
    /// ignores; the row applies only when it holds of the record.
    pub applies: Option<fn(&Value) -> bool>,
}

impl Row {
    /// What its value is measured in.
    #[must_use]
    pub const fn unit(&self) -> Unit {
        match self.kind {
            FieldKind::Length => Unit::Length,
            FieldKind::Rate => Unit::Rate,
            FieldKind::Angle => Unit::Degrees,
            FieldKind::Percentage => Unit::Percent,
            FieldKind::Seconds => Unit::Seconds,
            _ => Unit::None,
        }
    }

    /// Whether it applies to `record`: its path is there, and its rule
    /// holds.
    #[must_use]
    pub fn applies_to(&self, record: &Value) -> bool {
        get(record, self.key).is_some() && self.applies.is_none_or(|rule| rule(record))
    }
}

/// A row with no tool default and no rule.
const fn row(group: Group, key: &'static str, kind: FieldKind, label: StrKey, help: StrKey) -> Row {
    Row {
        group,
        key,
        kind,
        label,
        help,
        tool_default: None,
        applies: None,
    }
}

/// [`row`] defaulting to the tool's `default` while `null`.
const fn tool_row(
    group: Group,
    key: &'static str,
    kind: FieldKind,
    label: StrKey,
    help: StrKey,
    default: ToolDefault,
) -> Row {
    Row {
        tool_default: Some(default),
        ..row(group, key, kind, label, help)
    }
}

/// [`row`] that applies only where `rule` holds.
const fn ruled(
    group: Group,
    key: &'static str,
    kind: FieldKind,
    label: StrKey,
    help: StrKey,
    rule: fn(&Value) -> bool,
) -> Row {
    Row {
        applies: Some(rule),
        ..row(group, key, kind, label, help)
    }
}

// ── The rows every kind has. ───────────────────────────────────────────

const NAME: Row = row(
    Group::Identity,
    "name",
    FieldKind::Text,
    StrKey::EditorFieldName,
    StrKey::EditorFieldNameHelp,
);
const ENABLED: Row = row(
    Group::Identity,
    "enabled",
    FieldKind::Checkbox,
    StrKey::EditorFieldEnabled,
    StrKey::EditorFieldEnabledHelp,
);
const TARGETS: Row = row(
    Group::Targets,
    "targets",
    FieldKind::InstanceList,
    StrKey::EditorFieldTargets,
    StrKey::EditorFieldTargetsHelp,
);
const TOOL: Row = row(
    Group::Tool,
    "tool",
    FieldKind::ToolPicker,
    StrKey::EditorFieldTool,
    StrKey::EditorFieldToolHelp,
);
const DEPTH: Row = row(
    Group::Depth,
    "depth_mm",
    FieldKind::Length,
    StrKey::EditorFieldDepth,
    StrKey::EditorFieldDepthHelp,
);
const SAFE_Z: Row = row(
    Group::Heights,
    "safe_z_mm",
    FieldKind::Length,
    StrKey::EditorFieldSafeZ,
    StrKey::EditorFieldSafeZHelp,
);

// ── Choices. ───────────────────────────────────────────────────────────

/// `init`'s ramp angle, degrees: the angle a ramp or a helix starts at.
fn init_ramp_angle_deg() -> f64 {
    match dc_job::project_from::default_profile(Vec::new(), dc_project::ToolId::new("")).entry {
        dc_project::Entry::Ramp { angle_deg } | dc_project::Entry::Helix { angle_deg, .. } => {
            angle_deg
        }
        dc_project::Entry::Plunge => 0.0,
    }
}

/// A helix's diameter as a share of the bit's, as the plaque's Panel
/// pocket enters (the phase 2 quick start).
const HELIX_DIAMETER_FRACTION: f64 = 0.8;

/// The lead the plaque's Cut out has (the phase 2 quick start): 3 mm, 90°
/// (`init`'s sweep), out too, 1 mm past the start.
fn lead_template(_: &Project) -> Value {
    json!({ "radius_mm": 3.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 1.0 })
}

/// A last pass with the Profile chapter's 0.5 mm skin and the usual depth
/// passes: no depth of the app's own (S20).
fn last_pass_template(_: &Project) -> Value {
    json!({ "stock_to_leave_mm": 0.5, "thickness_mm": null, "reverse": false })
}

/// A start point at the stock's centre, to be typed or picked.
fn near_template(project: &Project) -> Value {
    json!({ "x_mm": project.stock.width_mm / 2.0, "y_mm": project.stock.height_mm / 2.0 })
}

/// A ramp at `init`'s angle.
fn ramp_template(_: &Project) -> Value {
    json!({ "angle_deg": init_ramp_angle_deg() })
}

/// A helix at `init`'s angle.
fn helix_template(_: &Project) -> Value {
    json!({ "diameter_fraction": HELIX_DIAMETER_FRACTION, "angle_deg": init_ramp_angle_deg() })
}

/// An option written as its plain tag.
const fn tag(label: StrKey, tag: &'static str) -> ChoiceOption {
    ChoiceOption {
        label,
        tag,
        value: OptionValue::Tag,
    }
}

/// An option written as a one-key object over `data`.
const fn variant(label: StrKey, tag: &'static str, data: Template) -> ChoiceOption {
    ChoiceOption {
        label,
        tag,
        value: OptionValue::Variant(data),
    }
}

const SIDES: &[ChoiceOption] = &[
    tag(StrKey::EditorChoiceInside, "inside"),
    tag(StrKey::EditorChoiceOutside, "outside"),
    tag(StrKey::EditorChoiceOnLine, "on_line"),
    tag(StrKey::EditorChoiceLeft, "left"),
    tag(StrKey::EditorChoiceRight, "right"),
];
const DIRECTIONS: &[ChoiceOption] = &[
    tag(StrKey::EditorChoiceClimb, "climb"),
    tag(StrKey::EditorChoiceConventional, "conventional"),
];
const STARTS: &[ChoiceOption] = &[
    tag(StrKey::EditorChoiceAuto, "auto"),
    tag(StrKey::EditorChoiceFirstVertex, "first_vertex"),
    variant(StrKey::EditorChoiceNear, "near", near_template),
];
const ENTRIES: &[ChoiceOption] = &[
    tag(StrKey::EditorChoicePlunge, "plunge"),
    variant(StrKey::EditorChoiceRamp, "ramp", ramp_template),
    variant(StrKey::EditorChoiceHelix, "helix", helix_template),
];
const CORNER_RELIEFS: &[ChoiceOption] = &[
    tag(StrKey::EditorChoiceNone, "none"),
    tag(StrKey::EditorChoiceDogbone, "dogbone"),
    tag(StrKey::EditorChoiceTBone, "t_bone"),
];
const ORDERS: &[ChoiceOption] = &[
    tag(StrKey::EditorChoiceFullDepthFirst, "full_depth_first"),
    tag(StrKey::EditorChoicePassFirst, "pass_first"),
];

// ── The tables. ────────────────────────────────────────────────────────

/// Capability 68's Profile line, every field.
const PROFILE: &[Row] = &[
    NAME,
    ENABLED,
    TARGETS,
    TOOL,
    DEPTH,
    tool_row(
        Group::Depth,
        "step_down_mm",
        FieldKind::Length,
        StrKey::EditorFieldStepDown,
        StrKey::EditorFieldStepDownHelp,
        ToolDefault::StepDown,
    ),
    row(
        Group::Depth,
        "through_cut_allowance_mm",
        FieldKind::Length,
        StrKey::EditorFieldThroughCutAllowance,
        StrKey::EditorFieldThroughCutAllowanceHelp,
    ),
    row(
        Group::Strategy,
        "side",
        FieldKind::Choice(SIDES),
        StrKey::EditorFieldSide,
        StrKey::EditorFieldSideHelp,
    ),
    row(
        Group::Strategy,
        "direction",
        FieldKind::Choice(DIRECTIONS),
        StrKey::EditorFieldDirection,
        StrKey::EditorFieldDirectionHelp,
    ),
    row(
        Group::Strategy,
        "start",
        FieldKind::Choice(STARTS),
        StrKey::EditorFieldStart,
        StrKey::EditorFieldStartHelp,
    ),
    row(
        Group::Strategy,
        "start.near.x_mm",
        FieldKind::Length,
        StrKey::EditorFieldStartX,
        StrKey::EditorFieldStartXHelp,
    ),
    row(
        Group::Strategy,
        "start.near.y_mm",
        FieldKind::Length,
        StrKey::EditorFieldStartY,
        StrKey::EditorFieldStartYHelp,
    ),
    row(
        Group::Strategy,
        "last_pass",
        FieldKind::Toggle(last_pass_template),
        StrKey::EditorFieldLastPass,
        StrKey::EditorFieldLastPassHelp,
    ),
    row(
        Group::Strategy,
        "last_pass.stock_to_leave_mm",
        FieldKind::Length,
        StrKey::EditorFieldLastPassStock,
        StrKey::EditorFieldLastPassStockHelp,
    ),
    row(
        Group::Strategy,
        "last_pass.thickness_mm",
        FieldKind::Length,
        StrKey::EditorFieldLastPassThickness,
        StrKey::EditorFieldLastPassThicknessHelp,
    ),
    row(
        Group::Strategy,
        "last_pass.reverse",
        FieldKind::Checkbox,
        StrKey::EditorFieldLastPassReverse,
        StrKey::EditorFieldLastPassReverseHelp,
    ),
    row(
        Group::EntryAndLeads,
        "entry",
        FieldKind::Choice(ENTRIES),
        StrKey::EditorFieldEntry,
        StrKey::EditorFieldEntryHelp,
    ),
    row(
        Group::EntryAndLeads,
        "entry.ramp.angle_deg",
        FieldKind::Angle,
        StrKey::EditorFieldRampAngle,
        StrKey::EditorFieldRampAngleHelp,
    ),
    row(
        Group::EntryAndLeads,
        "entry.helix.diameter_fraction",
        FieldKind::Percentage,
        StrKey::EditorFieldHelixDiameter,
        StrKey::EditorFieldHelixDiameterHelp,
    ),
    row(
        Group::EntryAndLeads,
        "entry.helix.angle_deg",
        FieldKind::Angle,
        StrKey::EditorFieldHelixAngle,
        StrKey::EditorFieldHelixAngleHelp,
    ),
    row(
        Group::EntryAndLeads,
        "lead",
        FieldKind::Toggle(lead_template),
        StrKey::EditorFieldLead,
        StrKey::EditorFieldLeadHelp,
    ),
    row(
        Group::EntryAndLeads,
        "lead.radius_mm",
        FieldKind::Length,
        StrKey::EditorFieldLeadRadius,
        StrKey::EditorFieldLeadRadiusHelp,
    ),
    row(
        Group::EntryAndLeads,
        "lead.sweep_deg",
        FieldKind::Angle,
        StrKey::EditorFieldLeadSweep,
        StrKey::EditorFieldLeadSweepHelp,
    ),
    row(
        Group::EntryAndLeads,
        "lead.lead_out",
        FieldKind::Checkbox,
        StrKey::EditorFieldLeadOut,
        StrKey::EditorFieldLeadOutHelp,
    ),
    row(
        Group::EntryAndLeads,
        "lead.overcut_mm",
        FieldKind::Length,
        StrKey::EditorFieldLeadOvercut,
        StrKey::EditorFieldLeadOvercutHelp,
    ),
    row(
        Group::CornersAndTabs,
        "corner_relief",
        FieldKind::Choice(CORNER_RELIEFS),
        StrKey::EditorFieldCornerRelief,
        StrKey::EditorFieldCornerReliefHelp,
    ),
    row(
        Group::CornersAndTabs,
        "tabs",
        FieldKind::TabRequest,
        StrKey::EditorFieldTabs,
        StrKey::EditorFieldTabsHelp,
    ),
    row(
        Group::Order,
        "machining_order",
        FieldKind::Choice(ORDERS),
        StrKey::EditorFieldMachiningOrder,
        StrKey::EditorFieldMachiningOrderHelp,
    ),
    SAFE_Z,
];

/// The rows slice 10 gives a Pocket; slice 11 (#540) adds the rest.
const POCKET: &[Row] = &[NAME, ENABLED, TARGETS, TOOL, DEPTH, SAFE_Z];

/// The rows slice 10 gives an Engrave; slice 11 (#540) adds the rest.
const ENGRAVE: &[Row] = &[NAME, ENABLED, TARGETS, TOOL, DEPTH, SAFE_Z];

/// Whether a Drill's record is not a countersink, whose depth follows
/// from its tool and mouth (format 21), so its `depth_mm` is unused.
fn not_countersink(record: &Value) -> bool {
    get(record, "cycle.countersink").is_none()
}

/// The rows slice 10 gives a Drill; slice 12 (#541) adds the rest.
const DRILL: &[Row] = &[
    NAME,
    ENABLED,
    TARGETS,
    TOOL,
    ruled(
        Group::Depth,
        "depth_mm",
        FieldKind::Length,
        StrKey::EditorFieldDepth,
        StrKey::EditorFieldDepthHelp,
        not_countersink,
    ),
    SAFE_Z,
];

/// The rows slice 10 gives a Bore; slice 12 (#541) adds the rest.
const BORE: &[Row] = &[NAME, ENABLED, TARGETS, TOOL, DEPTH, SAFE_Z];

/// The rows slice 10 gives a V-Carve, which has no `depth_mm`; slice 13
/// (#542) adds the rest.
const V_CARVE: &[Row] = &[NAME, ENABLED, TARGETS, TOOL, SAFE_Z];

/// The rows slice 10 gives an Inlay; slice 14 (#543) adds the rest.
const INLAY: &[Row] = &[NAME, ENABLED, TARGETS, TOOL, DEPTH, SAFE_Z];

/// Every row of `kind`, in render order (capability 68). Empty for a
/// Photo V-Carve, which has no record before phase 3.4 (slice 15, #544).
#[must_use]
pub const fn table(kind: Kind) -> &'static [Row] {
    match kind {
        Kind::Profile => PROFILE,
        Kind::Pocket => POCKET,
        Kind::Drill => DRILL,
        Kind::Bore => BORE,
        Kind::Engrave => ENGRAVE,
        Kind::VCarve => V_CARVE,
        Kind::Inlay => INLAY,
        Kind::PhotoVCarve => &[],
    }
}

/// The rows that apply to `op`, in render order: its kind's whose key
/// path is in its record and whose rule holds.
#[must_use]
pub fn rows(op: &Operation) -> Vec<&'static Row> {
    let Ok(record) = record(op) else {
        return Vec::new();
    };
    table(Kind::of(op))
        .iter()
        .filter(|r| r.applies_to(&record))
        .collect()
}

/// The groups `op` shows, in [`Group::ALL`]'s order, each with its rows;
/// a group with no row for it is left out (capability 66).
#[must_use]
pub fn groups(op: &Operation) -> Vec<(Group, Vec<&'static Row>)> {
    let rows = rows(op);
    Group::ALL
        .into_iter()
        .map(|group| {
            let of: Vec<&'static Row> = rows.iter().copied().filter(|r| r.group == group).collect();
            (group, of)
        })
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

/// The row of `op` a finding located at `field` belongs under: of the
/// rows that apply, the one whose key path is the longest that holds the
/// finding's path under `operations[<id>].`; or, for a tool-derived row
/// whose value is `null`, its tool's field (`tools[<tool>].step_down_mm`).
/// `None` for another operation's or the project's fields.
#[must_use]
pub fn row_for(op: &Operation, field: &str) -> Option<&'static Row> {
    let record = record(op).ok()?;
    row_among(&rows(op), &record, op, field)
}

/// [`row_for`] among `applicable`, the rows that apply to `op`, whose
/// record is `record`.
pub(crate) fn row_among(
    applicable: &[&'static Row],
    record: &Value,
    op: &Operation,
    field: &str,
) -> Option<&'static Row> {
    let own = format!("operations[{}].", op.id());
    if let Some(path) = field.strip_prefix(&own) {
        return applicable
            .iter()
            .copied()
            .filter(|r| {
                path.strip_prefix(r.key)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
            })
            .max_by_key(|r| r.key.len());
    }
    let tool = format!("tools[{}].", op.tool());
    let path = field.strip_prefix(&tool)?;
    applicable.iter().copied().find(|r| {
        r.tool_default.is_some_and(|d| d.tool_field() == path)
            && get(record, r.key).is_some_and(Value::is_null)
    })
}
