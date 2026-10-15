// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project validation (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"). [`validate`] reports every problem it finds rather than
//! stopping at the first, so the CLI `check` command can print them all.
//!
//! Error-class problems make a project unsafe or impossible to machine
//! (dangling ids, non-positive dimensions, non-finite numbers). Warning-class
//! problems still allow a correct result but need the user's attention
//! (disabled operations, tools without feeds or RPM, which export rejects
//! when such a tool is actually used).

use crate::ids::{DefinitionId, InstanceId, OperationId, ToolId};
use crate::model::{Operation, OriginXy, Project, TabPlacement, Tool, ToolKind};
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
        /// The unset field, such as `feed_mm_min`.
        field: &'static str,
    },
}

impl Problem {
    /// Whether this problem is an error or a warning.
    pub fn severity(&self) -> Severity {
        match self {
            Self::OperationDisabled { .. } | Self::ToolValueUnset { .. } => Severity::Warning,
            Self::DefinitionKeyMismatch { .. }
            | Self::DuplicateInstanceId { .. }
            | Self::DuplicateToolId { .. }
            | Self::DuplicateOperationId { .. }
            | Self::DanglingDefinition { .. }
            | Self::DanglingTool { .. }
            | Self::DanglingInstance { .. }
            | Self::OutOfRange { .. }
            | Self::NonFinite { .. }
            | Self::MissingPlaceholder { .. } => Severity::Error,
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
            Self::ToolValueUnset { tool, field } => {
                write!(
                    f,
                    "tool `{tool}` has no {field}; export fails until it is set"
                )
            }
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

#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn project(&mut self, p: &Project) {
        self.positive("flatten_tolerance_mm", p.flatten_tolerance_mm);
        self.positive("stock.width_mm", p.stock.width_mm);
        self.positive("stock.height_mm", p.stock.height_mm);
        self.positive("stock.thickness_mm", p.stock.thickness_mm);
        if let OriginXy::Custom { x_mm, y_mm } = p.origin.xy {
            self.finite("origin.xy.x_mm", x_mm);
            self.finite("origin.xy.y_mm", y_mm);
        }

        for (key, def) in &p.definitions {
            if *key != def.id {
                self.problems.push(Problem::DefinitionKeyMismatch {
                    key: key.clone(),
                    id: def.id.clone(),
                });
            }
            for (i, profile) in def.profiles.iter().enumerate() {
                for (j, vertex) in profile.vertices().iter().enumerate() {
                    let at = format!("definitions[{key}].profiles[{i}].vertices[{j}]");
                    self.finite(&format!("{at}.x"), vertex.x);
                    self.finite(&format!("{at}.y"), vertex.y);
                    self.finite(&format!("{at}.bulge"), vertex.bulge);
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
            for (i, c) in coeffs.iter().enumerate() {
                self.finite(&format!("{at}.transform[{i}]"), *c);
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
                let field = format!("{at}.cutter.included_angle_deg");
                if self.finite(&field, included_angle_deg)
                    && !(included_angle_deg > 0.0 && included_angle_deg < 180.0)
                {
                    self.out_of_range(field, included_angle_deg, ANGLE);
                }
            }
            self.positive(&format!("{at}.diameter_mm"), tool.diameter_mm);
            if tool.flutes == 0 {
                self.out_of_range(format!("{at}.flutes"), 0.0, POSITIVE);
            }
            self.required_positive(tool, "feed_mm_min", tool.feed_mm_min);
            self.required_positive(tool, "plunge_mm_min", tool.plunge_mm_min);
            match tool.rpm {
                None => self.problems.push(Problem::ToolValueUnset {
                    tool: tool.id.clone(),
                    field: "rpm",
                }),
                Some(0) => self.out_of_range(format!("{at}.rpm"), 0.0, POSITIVE),
                Some(_) => {}
            }
            if let Some(step) = tool.step_down_mm {
                self.positive(&format!("{at}.step_down_mm"), step);
            }
            if let Some(fraction) = tool.step_over_fraction {
                let field = format!("{at}.step_over_fraction");
                if self.finite(&field, fraction) && !(fraction > 0.0 && fraction <= 1.0) {
                    self.out_of_range(field, fraction, FRACTION);
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
                field,
            }),
            Some(v) => self.positive(&format!("tools[{}].{field}", tool.id), v),
        }
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
        p.stock.thickness_mm = 0.0;
        let problems = validate(&p);
        assert_eq!(problems.len(), 1);
        assert!(
            matches!(&problems[0], Problem::OutOfRange { field, .. } if field == "stock.thickness_mm")
        );
        assert!(problems[0].is_error());
    }

    #[test]
    fn non_positive_tool_diameter_is_error() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = -1.0;
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field, .. }] if field == "tools[t1].diameter_mm"),
            "{problems:?}"
        );
    }

    #[test]
    fn non_positive_depth_is_error() {
        let mut p = sample_project();
        let Operation::Profile(op) = &mut p.operations[0];
        op.depth_mm = 0.0;
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
            field: "feed_mm_min",
        };
        assert_eq!(
            problem.to_string(),
            "tool `t1` has no feed_mm_min; export fails until it is set"
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
}
