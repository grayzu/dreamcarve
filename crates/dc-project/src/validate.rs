// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project validation (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"). [`validate`] reports every problem it finds rather than
//! stopping at the first, so the CLI `check` command can print them all.
//!
//! Error-class problems make a project unsafe or impossible to machine
//! (dangling ids, non-positive dimensions, non-finite numbers, coordinates,
//! transforms and stock sides beyond [`MAX_COORDINATE_MM`]). Warning-class
//! problems still allow a correct result but need the user's attention
//! (disabled operations, tools without feeds or RPM, which export rejects
//! when such a tool is actually used).

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
    ToolValueUnset {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The unset field, such as `feed_mm_min`.
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
}

impl Problem {
    /// Whether this problem is an error or a warning.
    pub fn severity(&self) -> Severity {
        match self {
            Self::OperationDisabled { .. }
            | Self::ToolValueUnset { .. }
            | Self::OpenProfile { .. } => Severity::Warning,
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
            | Self::ToolOutOfRange { .. }
            | Self::UnsafeFileExtension { .. } => Severity::Error,
        }
    }

    /// Whether this problem is error-class.
    pub fn is_error(&self) -> bool {
        self.severity() == Severity::Error
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
            Self::ToolValueUnset { tool, name, field } => {
                write!(
                    f,
                    "tool `{name}` (`{tool}`) has no {field}; export fails until it is set"
                )
            }
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
        }
    }
}

/// Checks `project` and returns every problem found, errors and warnings,
/// in a deterministic order (project fields in declaration order).
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

#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn project(&mut self, p: &Project) {
        self.positive("flatten_tolerance_mm", p.flatten_tolerance_mm);
        self.stock_side("stock.width_mm", p.stock.width_mm);
        self.stock_side("stock.height_mm", p.stock.height_mm);
        self.stock_side("stock.thickness_mm", p.stock.thickness_mm);
        if let OriginXy::Custom { x_mm, y_mm } = p.origin.xy {
            self.finite("origin.xy.x_mm", x_mm);
            self.finite("origin.xy.y_mm", y_mm);
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

        let mut seen = BTreeSet::new();
        for tool in &p.tools {
            if !seen.insert(&tool.id) {
                self.problems.push(Problem::DuplicateToolId {
                    id: tool.id.clone(),
                });
            }
            let at = format!("tools[{}]", tool.id);
            if let ToolKind::VBit { included_angle_deg } = tool.cutter {
                let field = "cutter.included_angle_deg";
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
            self.required_positive(tool, "feed_mm_min", tool.feed_mm_min);
            self.required_positive(tool, "plunge_mm_min", tool.plunge_mm_min);
            // With the router switched by hand the speed is only a hint in
            // the pause message (#34), so it may be unset.
            let rpm_required =
                p.post_processor.feed_speed.spindle_control == SpindleControl::Automatic;
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
            self.positive(&format!("{at}.depth_mm"), op.depth_mm);
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

    /// A tool value export needs: unset is a warning, set must be positive.
    fn required_positive(&mut self, tool: &Tool, field: &'static str, value: Option<f64>) {
        match value {
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
        assert_eq!(
            problems,
            vec![Problem::DanglingTool {
                operation: OperationId::new("o1"),
                tool: ToolId::new("missing"),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
    }

    #[test]
    fn unset_feed_is_warning_not_error() {
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
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert!(errors(&problems).is_empty());
    }

    #[test]
    fn unset_plunge_and_rpm_are_warnings() {
        let mut p = sample_project();
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        let problems = validate(&p);
        assert_eq!(problems.len(), 2);
        assert!(errors(&problems).is_empty());
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
            "the stock has no thickness_mm; export fails until it is set"
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
            "operation `Cut out` (`o1`) has no depth_mm; export fails until it is set"
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
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::OperationValueUnset { field, .. } if *field == "depth_mm"))
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::ToolValueUnset { field, .. } if *field == "feed_mm_min"))
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
            vec![Problem::OperationDisabled {
                operation: OperationId::new("o1"),
            }]
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
                    field: "tools[t1].cutter.included_angle_deg".into()
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
            "tool `Set me` (`t1`) has no feed_mm_min; export fails until it is set"
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
}
