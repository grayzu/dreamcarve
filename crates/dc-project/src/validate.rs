// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project validation (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"). [`validate`] reports every problem it finds rather than
//! stopping at the first, so the CLI `check` command can print them all.
//!
//! Error-class problems make a project unsafe or impossible to machine
//! (dangling ids, non-positive dimensions, non-finite numbers, coordinates,
//! transforms and stock sides beyond [`MAX_COORDINATE_MM`], and every value
//! the user must supply that is unset: the stock thickness, each operation's
//! depth, each tool's feed, plunge rate and, under automatic spindle control,
//! RPM; nothing may default them, #64, #76). A tool's rates count only when
//! an enabled operation uses it: an unused tool is one warning instead, and
//! its unset rates come back as errors once an operation that uses it is
//! enabled. Warning-class problems still allow a correct result but need the
//! user's attention (disabled operations, unused tools, open paths).
//!
//! [`Problem::field`] names where in the file each problem is, so the CLI
//! can turn it into a `file:line:column` with [`crate::SourceMap`].

use crate::ids::{DefinitionId, InstanceId, OperationId, ToolId};
use crate::model::{Operation, OriginXy, Project, TabPlacement, Tool, ToolKind};
use crate::post_processor::{MAX_FILE_EXTENSION_CHARS, SpindleControl, is_safe_file_extension};
use dc_geometry::{GeometryError, MAX_COORDINATE_MM};
use std::collections::BTreeSet;
use std::fmt;

/// How serious a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The project cannot be machined safely as it stands.
    Error,
    /// The user should know, but the project is still usable.
    Warning,
}

/// One finding of [`validate`].
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// A definition's `id` differs from its key in `definitions`.
    DefinitionKeyMismatch {
        /// The map key.
        key: DefinitionId,
        /// The `id` field inside the definition.
        id: DefinitionId,
    },
    /// Two instances share an id.
    DuplicateInstanceId {
        /// The repeated id.
        id: InstanceId,
    },
    /// Two tools share an id.
    DuplicateToolId {
        /// The repeated id.
        id: ToolId,
    },
    /// Two operations share an id.
    DuplicateOperationId {
        /// The repeated id.
        id: OperationId,
    },
    /// An instance places a definition that does not exist.
    DanglingDefinition {
        /// The instance holding the reference.
        instance: InstanceId,
        /// The missing definition.
        definition: DefinitionId,
    },
    /// An operation cuts with a tool that does not exist.
    DanglingTool {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing tool.
        tool: ToolId,
    },
    /// An operation targets an instance that does not exist.
    DanglingInstance {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing instance.
        instance: InstanceId,
    },
    /// A number is outside the range its field allows.
    OutOfRange {
        /// Path to the field, such as `tools[t1].diameter_mm`.
        field: String,
        /// The value found.
        value: f64,
        /// The allowed range, in words.
        expected: &'static str,
    },
    /// A number is NaN or infinite.
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// A template line lacks a placeholder it must contain.
    MissingPlaceholder {
        /// Path to the field.
        field: String,
        /// The placeholder, such as `{seconds}`.
        placeholder: &'static str,
    },
    /// An operation is disabled and will not be exported.
    OperationDisabled {
        /// The disabled operation.
        operation: OperationId,
    },
    /// A tool has no value for a field export needs (feed, plunge or RPM).
    /// An error since #76, so `check` lists every unset value in one run.
    ToolValueUnset {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The unset field, such as `feed_mm_min`.
        field: &'static str,
    },
    /// A tool no enabled operation uses. Its unset rates are not reported
    /// (#84): nothing cuts with it, and they are never defaulted.
    UnusedTool {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
    },
    /// The stock thickness is unset. It sets depth, tab heights and, with
    /// Z zero at the stock bottom, safe Z, so it is never defaulted (#64).
    StockThicknessUnset,
    /// An operation has no value for a field export needs (its depth).
    OperationValueUnset {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The unset field, such as `depth_mm`.
        field: &'static str,
    },
    /// A tool's number is outside the range its field allows.
    ToolOutOfRange {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The field within the tool, such as `feed_mm_min`.
        field: &'static str,
        /// The value found.
        value: f64,
        /// The allowed range, in words.
        expected: &'static str,
    },
    /// A definition's profile is open. Profile cuts only closed shapes, so
    /// it is never cut (functional spec, capability 8).
    OpenProfile {
        /// The definition.
        definition: DefinitionId,
        /// The definition's display name.
        name: String,
        /// Index of the profile within the definition.
        profile: usize,
    },
    /// The post-processor's file extension could name the project file or a
    /// path (see [`is_safe_file_extension`]).
    UnsafeFileExtension {
        /// The extension found.
        extension: String,
    },
    /// An enabled operation cuts deeper per pass than its tool is wide
    /// (#67). A warning: the program is still what was asked for.
    StepDownAboveDiameter {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// Where the step-down in effect is set: the operation's
        /// `step_down_mm`, or else the tool's.
        field: String,
        /// The step-down in effect, mm.
        step_down_mm: f64,
        /// The tool's diameter, mm.
        diameter_mm: f64,
    },
    /// An enabled Profile cuts with a tool that is not a flat end mill
    /// (#67). Profile offsets by half the diameter, so any other cutter
    /// makes the part the wrong size: an error.
    CutterNotFlat {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `a V-bit`.
        cutter: &'static str,
    },
    /// An enabled operation targets nothing, so its program would start
    /// and stop the spindle without moving (#67). An error.
    OperationHasNoTargets {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
    },
    /// A used tool's spindle speed is above [`MAX_RPM`] (#67). A warning:
    /// GRBL and grblHAL clamp `S` to the machine's maximum (`$30`), and
    /// under manual control the speed is only a hint.
    SpindleSpeedHigh {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The speed set, RPM.
        rpm: u32,
    },
    /// A used tool plunges faster than it cuts (#67), which usually means
    /// the two rates are swapped. A warning.
    PlungeAboveFeed {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The plunge rate, mm/min.
        plunge_mm_min: f64,
        /// The feed rate, mm/min.
        feed_mm_min: f64,
    },
    /// A used tool's diameter is above [`MAX_TOOL_DIAMETER_MM`] (#67). A
    /// warning: a shape such a bit does not fit is already skipped with a
    /// warning, and around a part it cuts valid geometry.
    LargeDiameter {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The diameter, mm.
        diameter_mm: f64,
    },
}

/// The fastest feed or plunge rate a used tool may have, mm/min (#67).
/// Above it export fails: no hobby machine moves that fast, so the value is
/// almost surely a typo. Raise it if users report real machines beyond it.
pub const MAX_RATE_MM_MIN: f64 = 20_000.0;

/// Above this spindle speed a used tool gets a warning, RPM (#67).
pub const MAX_RPM: u32 = 60_000;

/// Above this diameter a used tool gets a warning, mm (#67).
pub const MAX_TOOL_DIAMETER_MM: f64 = 50.0;

impl Problem {
    /// Whether this problem is an error or a warning.
    pub fn severity(&self) -> Severity {
        match self {
            Self::OperationDisabled { .. }
            | Self::UnusedTool { .. }
            | Self::OpenProfile { .. }
            | Self::StepDownAboveDiameter { .. }
            | Self::SpindleSpeedHigh { .. }
            | Self::PlungeAboveFeed { .. }
            | Self::LargeDiameter { .. } => Severity::Warning,
            Self::CutterNotFlat { .. } | Self::OperationHasNoTargets { .. } => Severity::Error,
            Self::DefinitionKeyMismatch { .. }
            | Self::DuplicateInstanceId { .. }
            | Self::DuplicateToolId { .. }
            | Self::DuplicateOperationId { .. }
            | Self::DanglingDefinition { .. }
            | Self::DanglingTool { .. }
            | Self::DanglingInstance { .. }
            | Self::OutOfRange { .. }
            | Self::NonFinite { .. }
            | Self::MissingPlaceholder { .. }
            | Self::ToolValueUnset { .. }
            | Self::StockThicknessUnset
            | Self::OperationValueUnset { .. }
            | Self::ToolOutOfRange { .. }
            | Self::UnsafeFileExtension { .. } => Severity::Error,
        }
    }

    /// Whether this problem is error-class.
    pub fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    /// Whether this problem is a value the user must supply and has not:
    /// the list `init` prints (#76).
    pub fn is_unset_value(&self) -> bool {
        matches!(
            self,
            Self::ToolValueUnset { .. }
                | Self::StockThicknessUnset
                | Self::OperationValueUnset { .. }
        )
    }

    /// The field this problem is about, written as the messages write
    /// fields (`stock.thickness_mm`, `tools[t1].feed_mm_min`), for
    /// [`crate::SourceMap::locate`]. `None` when no one value is at fault,
    /// as with an id used twice.
    #[must_use]
    pub fn field(&self) -> Option<String> {
        match self {
            Self::DefinitionKeyMismatch { key, .. } => Some(format!("definitions[{key}].id")),
            Self::DuplicateInstanceId { .. }
            | Self::DuplicateToolId { .. }
            | Self::DuplicateOperationId { .. } => None,
            Self::DanglingDefinition { instance, .. } => {
                Some(format!("instances[{instance}].definition"))
            }
            Self::DanglingTool { operation, .. } => Some(format!("operations[{operation}].tool")),
            Self::DanglingInstance { operation, .. } => {
                Some(format!("operations[{operation}].targets"))
            }
            Self::OutOfRange { field, .. }
            | Self::NonFinite { field }
            | Self::MissingPlaceholder { field, .. } => Some(field.clone()),
            Self::OperationDisabled { operation } => {
                Some(format!("operations[{operation}].enabled"))
            }
            Self::ToolValueUnset { tool, field, .. } | Self::ToolOutOfRange { tool, field, .. } => {
                Some(format!("tools[{tool}].{field}"))
            }
            Self::UnusedTool { tool, .. } => Some(format!("tools[{tool}]")),
            Self::StockThicknessUnset => Some(STOCK_THICKNESS.to_owned()),
            Self::OperationValueUnset {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::OpenProfile {
                definition,
                profile,
                ..
            } => Some(format!("definitions[{definition}].profiles[{profile}]")),
            Self::UnsafeFileExtension { .. } => Some("post_processor.file_extension".to_owned()),
            Self::StepDownAboveDiameter { field, .. } => Some(field.clone()),
            Self::CutterNotFlat { operation, .. } => Some(format!("operations[{operation}].tool")),
            Self::OperationHasNoTargets { operation, .. } => {
                Some(format!("operations[{operation}].targets"))
            }
            Self::SpindleSpeedHigh { tool, .. } => Some(format!("tools[{tool}].rpm")),
            Self::PlungeAboveFeed { tool, .. } => Some(format!("tools[{tool}].plunge_mm_min")),
            Self::LargeDiameter { tool, .. } => Some(format!("tools[{tool}].diameter_mm")),
        }
    }
}

const STOCK_THICKNESS: &str = "stock.thickness_mm";

/// The unit of a value the user must supply, for "is not set" messages.
fn unset_unit(field: &str) -> &'static str {
    match field {
        "feed_mm_min" | "plunge_mm_min" => "mm/min",
        "rpm" => "RPM",
        _ => "mm",
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefinitionKeyMismatch { key, id } => {
                write!(f, "definition keyed `{key}` has id `{id}`")
            }
            Self::DuplicateInstanceId { id } => write!(f, "instance id `{id}` is used twice"),
            Self::DuplicateToolId { id } => write!(f, "tool id `{id}` is used twice"),
            Self::DuplicateOperationId { id } => write!(f, "operation id `{id}` is used twice"),
            Self::DanglingDefinition {
                instance,
                definition,
            } => write!(
                f,
                "instance `{instance}` places definition `{definition}`, which does not exist"
            ),
            Self::DanglingTool { operation, tool } => write!(
                f,
                "operation `{operation}` uses tool `{tool}`, which does not exist"
            ),
            Self::DanglingInstance {
                operation,
                instance,
            } => write!(
                f,
                "operation `{operation}` targets instance `{instance}`, which does not exist"
            ),
            Self::OutOfRange {
                field,
                value,
                expected,
            } => write!(f, "{field} is {value}, expected {expected}"),
            Self::NonFinite { field } => write!(f, "{field} is not a finite number"),
            Self::MissingPlaceholder { field, placeholder } => {
                write!(f, "{field} must contain {placeholder}")
            }
            Self::OperationDisabled { operation } => {
                write!(
                    f,
                    "operation `{operation}` is disabled and will not be exported"
                )
            }
            Self::ToolValueUnset { tool, name, field } => write!(
                f,
                "tools[{tool}].{field} is not set ({}) for tool `{name}`",
                unset_unit(field)
            ),
            Self::UnusedTool { tool, name } => write!(
                f,
                "tool `{tool}` (`{name}`) is defined but not used by any enabled operation"
            ),
            Self::StockThicknessUnset => write!(f, "{STOCK_THICKNESS} is not set (mm)"),
            Self::OperationValueUnset {
                operation,
                name,
                field,
            } => write!(
                f,
                "operations[{operation}].{field} is not set ({}) for operation `{name}`",
                unset_unit(field)
            ),
            Self::ToolOutOfRange {
                tool,
                name,
                field,
                value,
                expected,
            } => write!(
                f,
                "tool `{name}` (`{tool}`): {field} is {value}, expected {expected}"
            ),
            Self::OpenProfile {
                definition,
                name,
                profile,
            } => write!(
                f,
                "`{name}` (definition `{definition}`, profile {profile}) is an open path; Profile cuts only closed shapes, so it is not cut"
            ),
            Self::UnsafeFileExtension { extension } => write!(
                f,
                "the post-processor's file extension `{}` is not allowed: it must be 1 to {MAX_FILE_EXTENSION_CHARS} letters or digits and not `dcv`",
                extension.escape_default()
            ),
            Self::StepDownAboveDiameter {
                operation,
                name,
                tool_name,
                step_down_mm,
                diameter_mm,
                ..
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts {step_down_mm} mm per pass, deeper than the {diameter_mm} mm diameter of tool `{tool_name}`; passes that deep can break the bit, so use a step-down of at most {diameter_mm} mm"
            ),
            Self::CutterNotFlat {
                operation,
                name,
                tool,
                tool_name,
                cutter,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts with tool `{tool_name}` (`{tool}`), {cutter}; Profile offsets the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size. Use an end mill"
            ),
            Self::OperationHasNoTargets { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
            ),
            Self::SpindleSpeedHigh { tool, name, rpm } => write!(
                f,
                "tool `{name}` (`{tool}`) is set to {rpm} RPM, faster than the {MAX_RPM} RPM a hobby spindle or router turns; check it for a typo"
            ),
            Self::PlungeAboveFeed {
                tool,
                name,
                plunge_mm_min,
                feed_mm_min,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) plunges at {plunge_mm_min} mm/min, faster than it cuts ({feed_mm_min} mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
            ),
            Self::LargeDiameter {
                tool,
                name,
                diameter_mm,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) is {diameter_mm} mm in diameter, larger than the {MAX_TOOL_DIAMETER_MM} mm of a hobby router bit; check it for a typo"
            ),
        }
    }
}

/// Checks `project` and returns every problem found, errors and warnings,
/// in a deterministic order: project fields in declaration order, which is
/// the order a saved file holds them.
#[must_use]
pub fn validate(project: &Project) -> Vec<Problem> {
    let mut v = Validator::default();
    v.project(project);
    v.problems
}

const POSITIVE: &str = "a value greater than 0";
const NON_NEGATIVE: &str = "a value of 0 or more";
const FRACTION: &str = "a value greater than 0 and at most 1";
const ANGLE: &str = "an angle greater than 0 and less than 180 degrees";
// The limits below spell out `MAX_COORDINATE_MM`; a test checks they agree.
const STOCK_SIDE: &str = "a value greater than 0 and at most 6000 mm";
const COORDINATE: &str = "a value from -6000 to 6000 mm";
const COEFFICIENT: &str = "a value from -6000 to 6000";
const REACH: &str = "a shape that stays within 6000 mm of the origin";
// Spells out `MAX_RATE_MM_MIN`; a test checks they agree.
const RATE: &str = "a value greater than 0 and at most 20000 mm/min";

#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn project(&mut self, p: &Project) {
        self.positive("flatten_tolerance_mm", p.flatten_tolerance_mm);
        self.stock_side("stock.width_mm", p.stock.width_mm);
        self.stock_side("stock.height_mm", p.stock.height_mm);
        match p.stock.thickness_mm {
            Some(thickness_mm) => self.stock_side("stock.thickness_mm", thickness_mm),
            None => self.problems.push(Problem::StockThicknessUnset),
        }
        if let OriginXy::Custom { x_mm, y_mm } = p.origin.xy {
            self.finite("origin.xy.custom.x_mm", x_mm);
            self.finite("origin.xy.custom.y_mm", y_mm);
        }

        // In the order the file holds them (#76): settings, then geometry.
        // Tools an enabled operation cuts with; only their rates are required.
        let used: BTreeSet<&ToolId> = p
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(Operation::tool)
            .collect();
        let mut seen = BTreeSet::new();
        for tool in &p.tools {
            let in_use = used.contains(&tool.id);
            if !in_use {
                self.problems.push(Problem::UnusedTool {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                });
            }
            if !seen.insert(&tool.id) {
                self.problems.push(Problem::DuplicateToolId {
                    id: tool.id.clone(),
                });
            }
            let at = format!("tools[{}]", tool.id);
            if let ToolKind::VBit { included_angle_deg } = tool.cutter {
                let field = "cutter.v_bit.included_angle_deg";
                if self.finite(&format!("{at}.{field}"), included_angle_deg)
                    && !(included_angle_deg > 0.0 && included_angle_deg < 180.0)
                {
                    self.tool_out_of_range(tool, field, included_angle_deg, ANGLE);
                }
            }
            self.tool_positive(tool, "diameter_mm", tool.diameter_mm);
            if tool.flutes == 0 {
                self.tool_out_of_range(tool, "flutes", 0.0, POSITIVE);
            }
            self.required_positive(tool, in_use, "feed_mm_min", tool.feed_mm_min);
            self.required_positive(tool, in_use, "plunge_mm_min", tool.plunge_mm_min);
            // With the router switched by hand the speed is only a hint in
            // the pause message (#34), so it may be unset.
            let rpm_required =
                in_use && p.post_processor.feed_speed.spindle_control == SpindleControl::Automatic;
            match tool.rpm {
                None if rpm_required => self.problems.push(Problem::ToolValueUnset {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                    field: "rpm",
                }),
                Some(0) => self.tool_out_of_range(tool, "rpm", 0.0, POSITIVE),
                None | Some(_) => {}
            }
            if let Some(step) = tool.step_down_mm {
                self.tool_positive(tool, "step_down_mm", step);
            }
            if let Some(fraction) = tool.step_over_fraction {
                let field = "step_over_fraction";
                if self.finite(&format!("{at}.{field}"), fraction)
                    && !(fraction > 0.0 && fraction <= 1.0)
                {
                    self.tool_out_of_range(tool, field, fraction, FRACTION);
                }
            }
        }

        let tool_ids: BTreeSet<_> = p.tools.iter().map(|t| &t.id).collect();
        let instance_ids: BTreeSet<_> = p.instances.iter().map(|i| &i.id).collect();
        let mut seen = BTreeSet::new();
        for op in &p.operations {
            let Operation::Profile(op) = op;
            if !seen.insert(&op.id) {
                self.problems
                    .push(Problem::DuplicateOperationId { id: op.id.clone() });
            }
            if !op.enabled {
                self.problems.push(Problem::OperationDisabled {
                    operation: op.id.clone(),
                });
            }
            if !tool_ids.contains(&op.tool) {
                self.problems.push(Problem::DanglingTool {
                    operation: op.id.clone(),
                    tool: op.tool.clone(),
                });
            }
            for target in &op.targets {
                if !instance_ids.contains(target) {
                    self.problems.push(Problem::DanglingInstance {
                        operation: op.id.clone(),
                        instance: target.clone(),
                    });
                }
            }
            let at = format!("operations[{}]", op.id);
            match op.depth_mm {
                Some(depth_mm) => self.positive(&format!("{at}.depth_mm"), depth_mm),
                None => self.problems.push(Problem::OperationValueUnset {
                    operation: op.id.clone(),
                    name: op.name.clone(),
                    field: "depth_mm",
                }),
            }
            if let Some(step) = op.step_down_mm {
                self.positive(&format!("{at}.step_down_mm"), step);
            }
            self.non_negative(
                &format!("{at}.through_cut_allowance_mm"),
                op.through_cut_allowance_mm,
            );
            self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
            if let Some(tabs) = &op.tabs {
                match &tabs.placement {
                    TabPlacement::Count { count } => {
                        if *count == 0 {
                            self.out_of_range(format!("{at}.tabs.placement.count"), 0.0, POSITIVE);
                        }
                    }
                    TabPlacement::AtDistances { distances_mm } => {
                        for (i, d) in distances_mm.iter().enumerate() {
                            self.non_negative(
                                &format!("{at}.tabs.placement.distances_mm[{i}]"),
                                *d,
                            );
                        }
                    }
                }
                self.positive(&format!("{at}.tabs.width_mm"), tabs.width_mm);
                self.positive(&format!("{at}.tabs.height_mm"), tabs.height_mm);
            }
        }

        // Definitions with a coordinate out of range; their instances are
        // not checked again, so each mistake is reported once.
        let mut out_of_range = BTreeSet::new();
        for (key, def) in &p.definitions {
            if *key != def.id {
                self.problems.push(Problem::DefinitionKeyMismatch {
                    key: key.clone(),
                    id: def.id.clone(),
                });
            }
            for (i, profile) in def.profiles.iter().enumerate() {
                if !profile.is_closed() {
                    self.problems.push(Problem::OpenProfile {
                        definition: def.id.clone(),
                        name: def.name.clone(),
                        profile: i,
                    });
                }
                let mut ok = true;
                for (j, vertex) in profile.vertices().iter().enumerate() {
                    let at = format!("definitions[{key}].profiles[{i}].vertices[{j}]");
                    ok &= self.coordinate(&format!("{at}.x"), vertex.x);
                    ok &= self.coordinate(&format!("{at}.y"), vertex.y);
                    ok &= self.finite(&format!("{at}.bulge"), vertex.bulge);
                }
                // An arc can bow past the limit between vertices inside it.
                let reach_mm = profile.reach_mm();
                if ok && reach_mm > MAX_COORDINATE_MM {
                    self.out_of_range(format!("definitions[{key}].profiles[{i}]"), reach_mm, REACH);
                    ok = false;
                }
                if !ok {
                    out_of_range.insert(key);
                }
            }
        }

        let mut seen = BTreeSet::new();
        for inst in &p.instances {
            if !seen.insert(&inst.id) {
                self.problems.push(Problem::DuplicateInstanceId {
                    id: inst.id.clone(),
                });
            }
            if !p.definitions.contains_key(&inst.definition) {
                self.problems.push(Problem::DanglingDefinition {
                    instance: inst.id.clone(),
                    definition: inst.definition.clone(),
                });
            }
            let at = format!("instances[{}]", inst.id);
            let coeffs = inst.transform.as_kurbo().as_coeffs();
            let mut ok = true;
            for (i, c) in coeffs.iter().enumerate() {
                let field = format!("{at}.transform[{i}]");
                if !self.finite(&field, *c) {
                    ok = false;
                } else if c.abs() > MAX_COORDINATE_MM {
                    self.out_of_range(field, *c, COEFFICIENT);
                    ok = false;
                }
            }
            // Each coefficient can be in range and still carry a shape past
            // the limit. Other transform errors (singular, arcs under a
            // non-uniform scale) are the generators' to report.
            let def = p.definitions.get(&inst.definition);
            if let Some(def) = def.filter(|_| ok && !out_of_range.contains(&inst.definition)) {
                let reach_mm = def
                    .profiles
                    .iter()
                    .filter_map(|profile| match inst.transform.apply_polyline(profile) {
                        Err(GeometryError::CoordinateOutOfRange { reach_mm, .. }) => Some(reach_mm),
                        _ => None,
                    })
                    .reduce(f64::max);
                if let Some(reach_mm) = reach_mm {
                    self.out_of_range(format!("{at}.transform"), reach_mm, REACH);
                }
            }
            self.finite(&format!("{at}.z_top_mm"), inst.z_top_mm);
            self.finite(&format!("{at}.z_bottom_mm"), inst.z_bottom_mm);
        }

        if !is_safe_file_extension(&p.post_processor.file_extension) {
            self.problems.push(Problem::UnsafeFileExtension {
                extension: p.post_processor.file_extension.clone(),
            });
        }

        let dwell = &p.post_processor.feed_speed.spindle_on_dwell;
        let at = "post_processor.feed_speed.spindle_on_dwell";
        self.non_negative(&format!("{at}.duration_s"), dwell.duration_s);
        if !dwell.command.contains("{seconds}") {
            self.problems.push(Problem::MissingPlaceholder {
                field: format!("{at}.command"),
                placeholder: "{seconds}",
            });
        }
    }

    /// Reports `field` if `value` is not finite; returns whether it is.
    fn finite(&mut self, field: &str, value: f64) -> bool {
        let ok = value.is_finite();
        if !ok {
            self.problems.push(Problem::NonFinite {
                field: field.to_owned(),
            });
        }
        ok
    }

    /// Reports `field` if `value` is not finite or its magnitude exceeds
    /// [`MAX_COORDINATE_MM`]; returns whether it is in range.
    fn coordinate(&mut self, field: &str, value: f64) -> bool {
        if !self.finite(field, value) {
            return false;
        }
        let ok = value.abs() <= MAX_COORDINATE_MM;
        if !ok {
            self.out_of_range(field.to_owned(), value, COORDINATE);
        }
        ok
    }

    /// A stock side: positive and at most [`MAX_COORDINATE_MM`].
    fn stock_side(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && (value <= 0.0 || value > MAX_COORDINATE_MM) {
            self.out_of_range(field.to_owned(), value, STOCK_SIDE);
        }
    }

    fn positive(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && value <= 0.0 {
            self.out_of_range(field.to_owned(), value, POSITIVE);
        }
    }

    fn non_negative(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && value < 0.0 {
            self.out_of_range(field.to_owned(), value, NON_NEGATIVE);
        }
    }

    /// A tool value export needs: unset is an error when the tool is
    /// `in_use`, and a set one must be positive either way.
    fn required_positive(
        &mut self,
        tool: &Tool,
        in_use: bool,
        field: &'static str,
        value: Option<f64>,
    ) {
        match value {
            None if !in_use => {}
            None => self.problems.push(Problem::ToolValueUnset {
                tool: tool.id.clone(),
                name: tool.name.clone(),
                field,
            }),
            Some(v) => self.tool_positive(tool, field, v),
        }
    }

    /// [`Self::positive`] for a tool's field, naming the tool.
    fn tool_positive(&mut self, tool: &Tool, field: &'static str, value: f64) {
        if self.finite(&format!("tools[{}].{field}", tool.id), value) && value <= 0.0 {
            self.tool_out_of_range(tool, field, value, POSITIVE);
        }
    }

    fn tool_out_of_range(
        &mut self,
        tool: &Tool,
        field: &'static str,
        value: f64,
        expected: &'static str,
    ) {
        self.problems.push(Problem::ToolOutOfRange {
            tool: tool.id.clone(),
            name: tool.name.clone(),
            field,
            value,
            expected,
        });
    }

    fn out_of_range(&mut self, field: String, value: f64, expected: &'static str) {
        self.problems.push(Problem::OutOfRange {
            field,
            value,
            expected,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Operation, ToolKind};
    use crate::test_support::sample_project;
    use dc_geometry::{MAX_COORDINATE_MM, Polyline, Transform, Vertex, approx_eq};

    fn errors(problems: &[Problem]) -> Vec<&Problem> {
        problems.iter().filter(|p| p.is_error()).collect()
    }

    #[test]
    fn sample_project_has_no_problems() {
        assert_eq!(validate(&sample_project()), vec![]);
    }

    #[test]
    fn dangling_tool_id_is_error() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.tool = ToolId::new("missing");
        let problems = validate(&p);
        // t1 is now used by nothing, which is a warning of its own.
        assert_eq!(
            problems,
            vec![
                Problem::UnusedTool {
                    tool: ToolId::new("t1"),
                    name: "6.35 mm upcut".into(),
                },
                Problem::DanglingTool {
                    operation: OperationId::new("o1"),
                    tool: ToolId::new("missing"),
                },
            ]
        );
        assert_eq!(problems[1].severity(), Severity::Error);
    }

    /// #76: an unset feed is an error, so `check` lists it with the other
    /// unset values in one run instead of failing later in export.
    #[test]
    fn unset_feed_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "feed_mm_min",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(
            problems[0].to_string(),
            "tools[t1].feed_mm_min is not set (mm/min) for tool `6.35 mm upcut`"
        );
    }

    #[test]
    fn unset_plunge_and_rpm_are_errors() {
        let mut p = sample_project();
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        let problems = validate(&p);
        assert_eq!(problems.len(), 2);
        assert_eq!(errors(&problems).len(), 2);
        assert_eq!(
            problems[1].to_string(),
            "tools[t1].rpm is not set (RPM) for tool `6.35 mm upcut`"
        );
    }

    /// #34, S3 (manual): with the router switched by hand, the speed is
    /// only a hint in the pause message, so an unset one is not reported.
    /// Feed and plunge still are.
    #[test]
    fn unset_rpm_is_not_reported_with_manual_spindle_control() {
        let mut p = sample_project();
        p.post_processor.feed_speed.spindle_control = SpindleControl::Manual;
        p.tools[0].rpm = None;
        assert_eq!(validate(&p), vec![]);
        p.tools[0].plunge_mm_min = None;
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "plunge_mm_min",
            }]
        );
    }

    #[test]
    fn dangling_definition_id_is_error() {
        let mut p = sample_project();
        p.instances[0].definition = DefinitionId::new("missing");
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DanglingDefinition {
                instance: InstanceId::new("i1"),
                definition: DefinitionId::new("missing"),
            }]
        );
        assert!(problems[0].is_error());
    }

    #[test]
    fn dangling_instance_id_is_error() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.targets.push(InstanceId::new("missing"));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DanglingInstance {
                operation: OperationId::new("o1"),
                instance: InstanceId::new("missing"),
            }]
        );
    }

    #[test]
    fn non_positive_stock_dimension_is_error() {
        let mut p = sample_project();
        p.stock.thickness_mm = Some(0.0);
        let problems = validate(&p);
        assert_eq!(problems.len(), 1);
        assert!(
            matches!(&problems[0], Problem::OutOfRange { field, .. } if field == "stock.thickness_mm")
        );
        assert!(problems[0].is_error());
    }

    /// #64: thickness drives depth, tab Z and, with the origin at the stock
    /// bottom, safe Z itself, so an unset one is an error like a missing
    /// feed is at export, never a default. The message says export refuses.
    #[test]
    fn missing_thickness_is_an_error() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        let problems = validate(&p);
        assert_eq!(problems, vec![Problem::StockThicknessUnset]);
        assert!(problems[0].is_error());
        assert_eq!(
            problems[0].to_string(),
            "stock.thickness_mm is not set (mm)"
        );
    }

    /// #64: an unset Profile depth is an error naming the operation.
    #[test]
    fn missing_depth_is_an_error() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.depth_mm = None;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OperationValueUnset {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                field: "depth_mm",
            }]
        );
        assert!(problems[0].is_error());
        assert_eq!(
            problems[0].to_string(),
            "operations[o1].depth_mm is not set (mm) for operation `Cut out`"
        );
    }

    /// #64: one run reports the unset thickness, depth and tool rates
    /// together, so the user fixes them in one edit.
    #[test]
    fn missing_thickness_depth_and_feeds_are_reported_together() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        p.tools[0].feed_mm_min = None;
        let Operation::Profile(op) = &mut p.operations[0];
        op.depth_mm = None;
        let problems = validate(&p);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.contains(&Problem::StockThicknessUnset));
        assert!(problems.iter().any(
            |p| matches!(p, Problem::OperationValueUnset { field, .. } if *field == "depth_mm")
        ));
        assert!(problems.iter().any(
            |p| matches!(p, Problem::ToolValueUnset { field, .. } if *field == "feed_mm_min")
        ));
    }

    /// #76: every unset value is listed, in the order the file holds them,
    /// each with the field that locates it.
    #[test]
    fn unset_values_are_listed_in_file_order_with_fields() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        p.tools[0].feed_mm_min = None;
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        let Operation::Profile(op) = &mut p.operations[0];
        op.depth_mm = None;
        let problems = validate(&p);
        assert!(problems.iter().all(Problem::is_unset_value), "{problems:?}");
        assert!(problems.iter().all(Problem::is_error), "{problems:?}");
        let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
        assert_eq!(
            fields,
            [
                "stock.thickness_mm",
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm",
                "operations[o1].depth_mm",
            ]
        );
    }

    /// #76: a problem's field is where it sits in the saved file.
    #[test]
    fn problem_fields_locate_in_the_saved_file() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = 0.0;
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 200.0,
        };
        p.instances[0].definition = DefinitionId::new("missing");
        p.post_processor.file_extension = "dcv".into();
        let Operation::Profile(op) = &mut p.operations[0];
        op.safe_z_mm = 0.0;
        op.enabled = false;
        let problems = validate(&p);
        assert_eq!(problems.len(), 7, "{problems:?}");
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        for problem in &problems {
            let field = problem.field().unwrap();
            assert!(map.locate(&field).is_some(), "{field} in {text}");
        }
        assert!(
            Problem::DuplicateToolId {
                id: ToolId::new("t1")
            }
            .field()
            .is_none()
        );
        assert!(!problems.iter().any(Problem::is_unset_value));
    }

    /// A second tool with no rates set.
    fn unset_tool(id: &str) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: "3 mm downcut".into(),
            feed_mm_min: None,
            plunge_mm_min: None,
            rpm: None,
            ..crate::test_support::tool_6_35_upcut()
        }
    }

    /// PR #84: a tool nothing cuts with is one warning, located at the
    /// tool; its unset rates are not errors, and nothing defaults them.
    #[test]
    fn unused_tool_with_unset_values_is_a_warning_not_an_error() {
        let mut p = sample_project();
        p.tools.push(unset_tool("t2"));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::UnusedTool {
                tool: ToolId::new("t2"),
                name: "3 mm downcut".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(
            problems[0].to_string(),
            "tool `t2` (`3 mm downcut`) is defined but not used by any enabled operation"
        );
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        let at = map.locate(&problems[0].field().unwrap()).unwrap();
        let line = text.lines().nth(at.line - 1).unwrap();
        assert_eq!(line.trim(), "{", "points at the tool object: {line}");
        let next = text.lines().nth(at.line).unwrap();
        assert_eq!(next.trim(), "\"id\": \"t2\",");
    }

    /// PR #84: a tool only disabled operations use is unused, like the
    /// operations themselves; enabling one brings its errors back.
    #[test]
    fn tool_used_only_by_disabled_operation_is_unused() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].rpm = None;
        let Operation::Profile(op) = &mut p.operations[0];
        op.enabled = false;
        let problems = validate(&p);
        assert!(errors(&problems).is_empty(), "{problems:?}");
        assert!(problems.contains(&Problem::UnusedTool {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
        }));
        let Operation::Profile(op) = &mut p.operations[0];
        op.enabled = true;
        let problems = validate(&p);
        assert_eq!(errors(&problems).len(), 2, "{problems:?}");
        assert!(
            !problems
                .iter()
                .any(|p| matches!(p, Problem::UnusedTool { .. }))
        );
    }

    /// PR #84: a tool an enabled operation uses still reports every unset
    /// rate as an error, beside an unused tool's warning.
    #[test]
    fn used_tool_with_unset_values_is_still_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        p.tools.push(unset_tool("t2"));
        let problems = validate(&p);
        let fields: Vec<_> = errors(&problems)
            .into_iter()
            .filter_map(Problem::field)
            .collect();
        assert_eq!(
            fields,
            [
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm"
            ]
        );
        assert_eq!(problems.len(), 4, "one warning for t2: {problems:?}");
    }

    /// A zero feed is wrong even on an unused tool.
    #[test]
    fn unused_tool_still_rejects_bad_values() {
        let mut p = sample_project();
        let mut t2 = unset_tool("t2");
        t2.feed_mm_min = Some(0.0);
        p.tools.push(t2);
        let problems = validate(&p);
        assert!(
            problems.iter().any(|p| matches!(p, Problem::ToolOutOfRange { tool, field, .. } if tool.as_str() == "t2" && *field == "feed_mm_min")),
            "{problems:?}"
        );
    }

    #[test]
    fn non_positive_tool_diameter_is_error() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = -1.0;
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::ToolOutOfRange { tool, field, .. }] if tool.as_str() == "t1" && *field == "diameter_mm"),
            "{problems:?}"
        );
    }

    #[test]
    fn non_positive_depth_is_error() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.depth_mm = Some(0.0);
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field, .. }] if field == "operations[o1].depth_mm"),
            "{problems:?}"
        );
    }

    #[test]
    fn disabled_operation_is_warning() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.enabled = false;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![
                Problem::UnusedTool {
                    tool: ToolId::new("t1"),
                    name: "6.35 mm upcut".into(),
                },
                Problem::OperationDisabled {
                    operation: OperationId::new("o1"),
                },
            ]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
    }

    #[test]
    fn non_finite_value_is_error() {
        let mut p = sample_project();
        p.stock.width_mm = f64::NAN;
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: f64::INFINITY,
        };
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![
                Problem::NonFinite {
                    field: "stock.width_mm".into()
                },
                Problem::NonFinite {
                    field: "tools[t1].cutter.v_bit.included_angle_deg".into()
                },
            ]
        );
    }

    #[test]
    fn duplicate_tool_id_is_error() {
        let mut p = sample_project();
        p.tools.push(p.tools[0].clone());
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DuplicateToolId {
                id: ToolId::new("t1")
            }]
        );
    }

    #[test]
    fn definition_key_mismatch_is_error() {
        let mut p = sample_project();
        if let Some(def) = p.definitions.values_mut().next() {
            def.id = DefinitionId::new("other");
        }
        let problems = validate(&p);
        assert_eq!(problems.len(), 1);
        assert!(matches!(problems[0], Problem::DefinitionKeyMismatch { .. }));
    }

    #[test]
    fn problem_messages_name_the_subject() {
        let problem = Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "Set me".into(),
            field: "feed_mm_min",
        };
        assert_eq!(
            problem.to_string(),
            "tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`"
        );
    }

    const DWELL: &str = "post_processor.feed_speed.spindle_on_dwell";

    #[test]
    fn negative_spindle_dwell_is_error() {
        let mut p = sample_project();
        p.post_processor.feed_speed.spindle_on_dwell.duration_s = -1.0;
        let problems = validate(&p);
        let field = format!("{DWELL}.duration_s");
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field: f, .. }] if *f == field),
            "{problems:?}"
        );
    }

    #[test]
    fn non_finite_spindle_dwell_is_error() {
        let mut p = sample_project();
        p.post_processor.feed_speed.spindle_on_dwell.duration_s = f64::NAN;
        let field = format!("{DWELL}.duration_s");
        assert_eq!(validate(&p), vec![Problem::NonFinite { field }]);
    }

    #[test]
    fn dwell_command_without_seconds_placeholder_is_error() {
        let mut p = sample_project();
        p.post_processor.feed_speed.spindle_on_dwell.command = "G4 P3".into();
        assert_eq!(
            validate(&p),
            vec![Problem::MissingPlaceholder {
                field: format!("{DWELL}.command"),
                placeholder: "{seconds}",
            }]
        );
    }

    #[test]
    fn zero_spindle_dwell_is_allowed() {
        let mut p = sample_project();
        p.post_processor.feed_speed.spindle_on_dwell.duration_s = 0.0;
        assert!(validate(&p).is_empty());
    }

    /// Functional spec acceptance test 2 (#40): a tool problem names the
    /// tool as the user sees it, not only by id.
    #[test]
    fn tool_problems_name_the_tool() {
        let mut p = sample_project();
        p.tools[0].name = "Set me".into();
        p.tools[0].feed_mm_min = Some(0.0);
        p.tools[0].plunge_mm_min = None;
        let messages: Vec<String> = validate(&p).iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        for (message, field) in messages.iter().zip(["feed_mm_min", "plunge_mm_min"]) {
            for part in ["Set me", "t1", field] {
                assert!(message.contains(part), "`{message}` names {part}");
            }
        }
    }

    /// Functional spec capability 8 (#40): open paths are imported but
    /// Profile never cuts them, and `check` reports each one.
    #[test]
    fn open_profile_is_warning_naming_the_definition() {
        let mut p = sample_project();
        assert!(
            !validate(&p)
                .iter()
                .any(|q| matches!(q, Problem::OpenProfile { .. })),
            "the sample's closed rectangle is not reported"
        );
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles.push(Polyline::new(
            vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
            false,
        ));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OpenProfile {
                definition: DefinitionId::new("d1"),
                name: "Rectangle".into(),
                profile: 1,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        let message = problems[0].to_string();
        assert!(
            message.contains("Rectangle") && message.contains("open"),
            "{message}"
        );
    }

    /// The fields of every error-class problem, in order.
    fn error_fields(problems: &[Problem]) -> Vec<String> {
        problems
            .iter()
            .filter(|p| p.is_error())
            .map(|p| match p {
                Problem::OutOfRange { field, .. } | Problem::NonFinite { field } => field.clone(),
                other => panic!("unexpected problem {other:?}"),
            })
            .collect()
    }

    /// #59: a finite but huge transform passed validation and then
    /// panicked `cavalier_contours`. Scale and translation are each
    /// rejected, naming the instance's transform.
    #[test]
    fn huge_transform_is_rejected() {
        for transform in [
            Transform::scale(1e15, 1e15),
            Transform::translate(1e12, 0.0),
            Transform::from([1.0, 0.0, 0.0, 1.0, 0.0, -1e12]),
        ] {
            let mut p = sample_project();
            p.instances[0].transform = transform;
            let fields = error_fields(&validate(&p));
            assert!(!fields.is_empty(), "{transform:?} is rejected");
            assert!(
                fields
                    .iter()
                    .all(|f| f.starts_with("instances[i1].transform")),
                "{transform:?}: {fields:?}"
            );
        }
    }

    /// #59: an instance whose placed shape reaches just past the limit is
    /// rejected even though each coefficient is in range; one that reaches
    /// exactly the limit is accepted. The sample rectangle is 50 × 30 mm.
    #[test]
    fn placement_past_coordinate_limit_is_rejected() {
        let mut p = sample_project();
        p.stock.width_mm = MAX_COORDINATE_MM;
        p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 50.0, 10.0);
        assert_eq!(validate(&p), vec![], "reaching the limit is accepted");
        p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 49.5, 10.0);
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field, value, .. }]
                if field == "instances[i1].transform"
                    && approx_eq(*value, MAX_COORDINATE_MM + 0.5, 1e-9)),
            "{problems:?}"
        );
    }

    /// #59: vertex coordinates are bounded by the limit, inclusive.
    #[test]
    fn huge_vertex_is_rejected() {
        let mut p = sample_project();
        p.instances[0].transform = Transform::IDENTITY;
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(MAX_COORDINATE_MM, 0.0),
                Vertex::line(MAX_COORDINATE_MM, 10.0),
            ],
            true,
        )];
        assert!(errors(&validate(&p)).is_empty(), "the limit is accepted");
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(1e12, 0.0),
                Vertex::line(1e12, -10.0),
            ],
            true,
        )];
        assert_eq!(
            error_fields(&validate(&p)),
            vec![
                "definitions[d1].profiles[0].vertices[1].x",
                "definitions[d1].profiles[0].vertices[2].x",
            ]
        );
    }

    /// #59: an arc whose vertices are in range can still bow past the
    /// limit; the profile is reported.
    #[test]
    fn arc_bulging_past_coordinate_limit_is_rejected() {
        let mut p = sample_project();
        p.instances[0].transform = Transform::IDENTITY;
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::circle(
            dc_geometry::Point::new(0.0, MAX_COORDINATE_MM - 0.5),
            1.0,
        )];
        let fields = error_fields(&validate(&p));
        assert!(
            fields.contains(&"definitions[d1].profiles[0]".to_owned()),
            "{fields:?}"
        );
    }

    /// #59: stock sides are bounded by the same limit, inclusive.
    #[test]
    fn huge_stock_is_rejected() {
        let mut p = sample_project();
        p.stock.width_mm = MAX_COORDINATE_MM;
        p.stock.height_mm = MAX_COORDINATE_MM;
        p.stock.thickness_mm = Some(MAX_COORDINATE_MM);
        assert_eq!(validate(&p), vec![], "the limit is accepted");
        p.stock.width_mm = MAX_COORDINATE_MM + 0.5;
        p.stock.height_mm = 1e15;
        p.stock.thickness_mm = Some(1e300);
        assert_eq!(
            error_fields(&validate(&p)),
            vec!["stock.width_mm", "stock.height_mm", "stock.thickness_mm"]
        );
    }

    /// The limit phrases spell out `MAX_COORDINATE_MM` by hand.
    #[test]
    fn limit_phrases_match_the_constant() {
        let limit = format!("{MAX_COORDINATE_MM}");
        for phrase in [STOCK_SIDE, COORDINATE, COEFFICIENT, REACH] {
            assert!(phrase.contains(&limit), "`{phrase}` states {limit}");
        }
    }

    /// #59: the message states the limit, so the user knows what to aim for.
    #[test]
    fn limit_messages_state_the_limit() {
        let mut p = sample_project();
        p.stock.width_mm = 1e15;
        p.instances[0].transform = Transform::translate(1e12, 0.0);
        let limit = MAX_COORDINATE_MM.to_string();
        let problems = validate(&p);
        assert_eq!(problems.len(), 2, "{problems:?}");
        for problem in problems {
            let message = problem.to_string();
            assert!(message.contains(&limit), "`{message}` states {limit}");
        }
    }

    /// #36 security review (#40): a shared `.dcv` chooses its output
    /// extension, so it must not name the project file or a path.
    #[test]
    fn unsafe_file_extension_is_error() {
        for extension in ["dcv", "DCV", "", "/../x", "n c", "toolong12"] {
            let mut p = sample_project();
            p.post_processor.file_extension = extension.into();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::UnsafeFileExtension {
                    extension: extension.into()
                }],
                "`{extension}` is refused"
            );
            assert!(problems[0].is_error(), "`{extension}` is an error");
        }
        for extension in ["nc", "gcode", "tap", "NGC"] {
            let mut p = sample_project();
            p.post_processor.file_extension = extension.into();
            assert_eq!(validate(&p), vec![], "`{extension}` is allowed");
        }
    }

    // #67: sanity checks on dangerous but valid values.

    fn op_mut(p: &mut Project) -> &mut crate::model::ProfileOp {
        let Operation::Profile(op) = &mut p.operations[0];
        op
    }

    /// Asserts `problems` is exactly one problem with `severity`, whose
    /// field locates in the saved `p`; returns its message.
    fn only_located(p: &Project, problems: &[Problem], severity: Severity) -> String {
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].severity(), severity, "{problems:?}");
        let text = crate::save(p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        let field = problems[0].field().unwrap();
        assert!(map.locate(&field).is_some(), "{field} in {text}");
        problems[0].to_string()
    }

    /// #67: a 50 mm step-down with a 6.35 mm end mill is one full-depth
    /// pass; it is a warning naming the operation and the tool, located at
    /// the step-down in effect.
    #[test]
    fn step_down_above_diameter_warns() {
        let mut p = sample_project();
        op_mut(&mut p).step_down_mm = Some(50.0);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].step_down_mm");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) cuts 50 mm per pass, deeper than the 6.35 mm diameter of tool `6.35 mm upcut`; passes that deep can break the bit, so use a step-down of at most 6.35 mm"
        );

        // The tool's step-down, when the operation sets none.
        let mut p = sample_project();
        p.tools[0].step_down_mm = Some(7.0);
        let problems = validate(&p);
        only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].step_down_mm");

        // Equal to the diameter is fine.
        let mut p = sample_project();
        op_mut(&mut p).step_down_mm = Some(6.35);
        assert_eq!(validate(&p), vec![]);
    }

    /// #67: Profile offsets by half the diameter, which is right only for
    /// a flat end mill; any other cutter makes the part the wrong size.
    #[test]
    fn v_bit_in_profile_is_an_error() {
        let mut p = sample_project();
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].tool");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) cuts with tool `6.35 mm upcut` (`t1`), a V-bit; Profile offsets the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size. Use an end mill"
        );
    }

    #[test]
    fn ball_nose_and_engraver_in_profile_are_errors() {
        for (cutter, words) in [
            (ToolKind::BallNose, "a ball-nose bit"),
            (ToolKind::Engraver, "an engraving bit"),
        ] {
            let mut p = sample_project();
            p.tools[0].cutter = cutter;
            let problems = validate(&p);
            let message = only_located(&p, &problems, Severity::Error);
            assert!(message.contains(words), "{message}");
        }
    }

    /// #67 (maintainer comment): a feed over the limit fails export. The
    /// error names the tool, the field and the limit.
    #[test]
    fn feed_above_limit_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(1e9);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].feed_mm_min");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`): feed_mm_min is 1000000000, expected a value greater than 0 and at most 20000 mm/min"
        );
        p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn plunge_above_limit_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
        p.tools[0].plunge_mm_min = Some(MAX_RATE_MM_MIN + 1.0);
        let problems = validate(&p);
        // Also faster than the feed, which is a warning of its own.
        let errors = errors(&problems);
        assert_eq!(errors.len(), 1, "{problems:?}");
        assert_eq!(errors[0].field().unwrap(), "tools[t1].plunge_mm_min");
    }

    /// The rate limit phrase spells out `MAX_RATE_MM_MIN` by hand.
    #[test]
    fn rate_limit_phrase_matches_the_constant() {
        assert!(
            RATE.contains(&format!("{MAX_RATE_MM_MIN} mm/min")),
            "{RATE}"
        );
    }

    /// #67: GRBL and grblHAL clamp `S` to `$30`, so a huge speed is a
    /// warning, not an error.
    #[test]
    fn rpm_above_limit_warns() {
        let mut p = sample_project();
        p.tools[0].rpm = Some(1_000_000);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].rpm");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) is set to 1000000 RPM, faster than the 60000 RPM a hobby spindle or router turns; check it for a typo"
        );
        p.tools[0].rpm = Some(MAX_RPM);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn plunge_above_feed_warns() {
        let mut p = sample_project();
        p.tools[0].plunge_mm_min = Some(1200.0);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].plunge_mm_min");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) plunges at 1200 mm/min, faster than it cuts (1000 mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
        );
        p.tools[0].plunge_mm_min = Some(1000.0);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn diameter_above_limit_warns() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = 200.0;
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].diameter_mm");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) is 200 mm in diameter, larger than the 50 mm of a hobby router bit; check it for a typo"
        );
        p.tools[0].diameter_mm = MAX_TOOL_DIAMETER_MM;
        assert_eq!(validate(&p), vec![]);
    }

    /// #67: an increment of 0 writes `N0` on every line.
    #[test]
    fn zero_line_increment_is_rejected() {
        let mut p = sample_project();
        p.post_processor.program.line_number_increment = 0;
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(
            message,
            "post_processor.program.line_number_increment is 0, expected a value greater than 0"
        );
    }

    /// #67: an enabled operation with no targets would write a program
    /// that starts the spindle and stops it without moving.
    #[test]
    fn empty_operation_is_an_error() {
        let mut p = sample_project();
        op_mut(&mut p).targets.clear();
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].targets");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
        );
        // Disabled, it is only the disabled warning (and its unused tool).
        op_mut(&mut p).enabled = false;
        assert!(errors(&validate(&p)).is_empty());
    }

    /// #67: only tools an enabled operation uses are checked; an unused
    /// tool is already a warning of its own.
    #[test]
    fn unused_tool_gets_no_sanity_findings() {
        let mut p = sample_project();
        p.tools.push(Tool {
            id: ToolId::new("t2"),
            name: "Odd".into(),
            cutter: ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            diameter_mm: 200.0,
            feed_mm_min: Some(1e9),
            plunge_mm_min: Some(2e9),
            rpm: Some(1_000_000),
            step_down_mm: Some(500.0),
            ..crate::test_support::tool_6_35_upcut()
        });
        assert_eq!(
            validate(&p),
            vec![Problem::UnusedTool {
                tool: ToolId::new("t2"),
                name: "Odd".into(),
            }]
        );
    }

    /// #67: every sanity problem is reported in one run, in file order.
    #[test]
    fn sanity_problems_are_reported_together() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(1e9);
        p.tools[0].plunge_mm_min = Some(2e9);
        p.tools[0].rpm = Some(1_000_000);
        p.post_processor.program.line_number_increment = 0;
        op_mut(&mut p).targets.clear();
        let problems = validate(&p);
        let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
        assert_eq!(
            fields,
            [
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm",
                "tools[t1].plunge_mm_min",
                "operations[o1].targets",
                "post_processor.program.line_number_increment",
            ],
            "{problems:?}"
        );
        assert_eq!(errors(&problems).len(), 4, "{problems:?}");
    }

    /// #67: the quick start's coaster, as the chapter fills it in (feed
    /// 1500, plunge 500, 18000 RPM, 4 mm step-down, 6.35 mm end mill),
    /// draws no sanity finding. `dreamcarve`'s CLI test
    /// `quick_start_project_has_no_sanity_warnings` checks the real file.
    #[test]
    fn quick_start_values_have_no_sanity_findings() {
        let mut p = sample_project();
        let tool = &mut p.tools[0];
        tool.cutter = ToolKind::EndMill;
        tool.diameter_mm = 6.35;
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.rpm = Some(18000);
        tool.step_down_mm = None;
        p.stock.thickness_mm = Some(11.8);
        let op = op_mut(&mut p);
        op.step_down_mm = Some(4.0);
        op.depth_mm = Some(12.0);
        op.through_cut_allowance_mm = 0.2;
        assert_eq!(validate(&p), vec![]);
    }
}
