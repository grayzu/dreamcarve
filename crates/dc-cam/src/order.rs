// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Operation-order warnings (`docs/specs/phase-2/design.md`, "Algorithms →
//! Operation-order warnings (dc-cam::order)"; `docs/research/toolpath-ordering.md`
//! §5.5, invariant O8, decision D7). Operations always run in the order the
//! project lists them; this module never reorders, it only says when that
//! order cuts inside a part an earlier operation has already freed, and when
//! a tool is fitted more than once because its operations are not adjacent.
//!
//! Nothing here depends on Y-up or Y-down: containment is symmetric under
//! the flip.

use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_project::{Operation, Project};

/// Checks the order of the operations of `project`. `generated` holds, in
/// project order, every enabled operation whose toolpath was generated,
/// with that toolpath (project space, mm, Y-up, Z-up); operations that
/// failed are simply absent and take no part.
///
/// Returns, in a deterministic order:
///
/// - [`crate::WarningCode::CutInsideFreedPart`] for every closed target of
///   a later operation that lies inside an outline an earlier Profile cut
///   through the stock, by operation order then target order;
/// - [`crate::WarningCode::ToolChangeAvoidable`] once per tool that the
///   enabled operations, in order, use again after a different tool, by
///   the tool's first use.
#[must_use]
pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning> {
    let _ = (project, generated);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::warning::WarningCode;
    use dc_geometry::{Point, Polyline, Transform};
    use dc_project::test_support::{sample_project, tool_6_35_upcut};
    use dc_project::{
        DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, OperationId, ProfileOp, Side,
        TabPlacement, TabShape, TabSpec, ToolId,
    };

    /// The sample project (100 × 100 × 6 mm stock, the 50 × 30 mm rectangle
    /// `i1` at (10, 10)) with a 10 mm circle `i2` centred in the rectangle
    /// and no operations: each test lists its own.
    fn part_and_hole() -> Project {
        let mut project = sample_project();
        let circle = FeatureDefinition {
            id: DefinitionId::new("d2"),
            name: "Circle".into(),
            profiles: vec![Polyline::circle(Point { x: 0.0, y: 0.0 }, 5.0)],
        };
        project.definitions.insert(circle.id.clone(), circle);
        project.instances.push(FeatureInstance {
            id: InstanceId::new("i2"),
            definition: DefinitionId::new("d2"),
            transform: Transform::translate(35.0, 25.0),
            name: "Hole".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        project.operations.clear();
        project
    }

    /// A Profile `id` on `target` with `side`, `depth_mm` deep, no tabs,
    /// cutting with `t1`.
    fn profile(id: &str, target: &str, side: Side, depth_mm: f64) -> ProfileOp {
        let sample = sample_project();
        let Some(Operation::Profile(template)) = sample.operations.first() else {
            panic!("the sample project starts with a Profile");
        };
        ProfileOp {
            id: OperationId::new(id),
            name: id.to_uppercase(),
            targets: vec![InstanceId::new(target)],
            side,
            depth_mm: Some(depth_mm),
            tabs: None,
            ..template.clone()
        }
    }

    /// Four 6 mm tabs 2 mm high, as the sample project has.
    fn four_tabs() -> TabSpec {
        TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 6.0,
            height_mm: 2.0,
            shape: TabShape::Rect,
        }
    }

    /// Generates every enabled operation of `project` as `export` does.
    fn generate(project: &Project) -> Vec<(Operation, Toolpath)> {
        project
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(|op| {
                let Operation::Profile(profile) = op else {
                    panic!("these tests use Profile operations");
                };
                let (toolpath, _) = generate_profile(project, profile)
                    .unwrap_or_else(|e| panic!("{} generates: {e}", op.id()));
                (op.clone(), toolpath)
            })
            .collect()
    }

    /// Runs [`check`] on `project` with every enabled operation generated.
    fn check_all(project: &Project) -> Vec<Warning> {
        let generated = generate(project);
        let refs: Vec<(&Operation, &Toolpath)> =
            generated.iter().map(|(op, tp)| (op, tp)).collect();
        check(project, &refs)
    }

    fn with_code(warnings: &[Warning], code: WarningCode) -> Vec<&Warning> {
        warnings.iter().filter(|w| w.code == code).collect()
    }

    /// O8, acceptance test 32: an Outside Profile through the stock on
    /// the rectangle, then an Inside Profile on the circle inside it. The
    /// first frees the rectangle, so the second cuts in a loose part.
    #[test]
    fn cut_inside_freed_part_warns() {
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Profile(profile("o2", "i2", Side::Inside, 6.0)),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        assert_eq!(found.len(), 1, "one warning: {warnings:?}");
        let w = found[0];
        assert_eq!(w.param("op"), Some("o2"));
        assert_eq!(w.param("earlier_op"), Some("o1"));
        assert_eq!(w.param("instance"), Some("i2"));
        assert_eq!(w.param("instance_name"), Some("Hole"));
        assert_eq!(w.param("profile"), Some("0"));
        assert_eq!(w.param("freed_instance"), Some("i1"));
        assert_eq!(w.param("freed_instance_name"), Some("Rectangle 1"));
        assert_eq!(w.param("freed_profile"), Some("0"));
        let text = w.to_string();
        assert!(text.contains("`o1`"), "names the earlier operation: {text}");
        assert!(text.contains("`Hole`"), "names the shape: {text}");
        assert!(
            text.contains("`Rectangle 1`"),
            "names the freed outline: {text}"
        );
    }

    /// O8: the hole first, then the outline. The outline is not inside the
    /// hole, so nothing is cut in a loose part.
    #[test]
    fn inside_before_outside_is_quiet() {
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i2", Side::Inside, 6.0)),
            Operation::Profile(profile("o2", "i1", Side::Outside, 6.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "quiet: {warnings:?}"
        );
    }

    /// An Inside Profile through the stock on the rectangle frees the
    /// rectangle's slug, so a later cut inside that outline warns too.
    #[test]
    fn inside_profile_through_frees_its_slug() {
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Inside, 6.0)),
            Operation::Profile(profile("o2", "i2", Side::OnLine, 1.0)),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        assert_eq!(found.len(), 1, "one warning: {warnings:?}");
        assert_eq!(found[0].param("op"), Some("o2"));
        assert_eq!(found[0].param("earlier_op"), Some("o1"));
    }

    /// A Profile that stops above the stock bottom frees nothing.
    #[test]
    fn profile_above_the_bottom_frees_nothing() {
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 3.0)),
            Operation::Profile(profile("o2", "i2", Side::Inside, 6.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "quiet: {warnings:?}"
        );
    }

    /// Tabs hold the part after the final pass, so a Profile with tabs
    /// frees nothing: the later operation cuts a held part.
    #[test]
    fn tabs_hold_the_part_so_nothing_is_freed() {
        let mut project = part_and_hole();
        let mut outline = profile("o1", "i1", Side::Outside, 6.0);
        outline.tabs = Some(four_tabs());
        project.operations = vec![
            Operation::Profile(outline),
            Operation::Profile(profile("o2", "i2", Side::Inside, 6.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "quiet: {warnings:?}"
        );
    }

    /// Operations are never reordered (D7): the warnings come back in
    /// project order, each naming the operations as listed, and the same
    /// input gives the same output.
    #[test]
    fn operations_keep_project_order() {
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Profile(profile("o2", "i2", Side::Inside, 3.0)),
            Operation::Profile(profile("o3", "i2", Side::OnLine, 1.0)),
        ];
        let warnings = check_all(&project);
        let ops: Vec<_> = with_code(&warnings, WarningCode::CutInsideFreedPart)
            .iter()
            .map(|w| (w.param("op"), w.param("earlier_op")))
            .collect();
        assert_eq!(
            ops,
            vec![(Some("o2"), Some("o1")), (Some("o3"), Some("o1"))]
        );
        assert_eq!(warnings, check_all(&project), "deterministic");
    }

    /// Enabled operations using T1, T2, T1: one hint for T1 listing both
    /// of its operations. The hint reads the project, not the toolpaths.
    #[test]
    fn non_adjacent_same_tool_hints() {
        let mut project = part_and_hole();
        let mut t2 = tool_6_35_upcut();
        t2.id = ToolId::new("t2");
        t2.name = "second bit".into();
        project.tools.push(t2);
        let mut middle = profile("o2", "i2", Side::Inside, 3.0);
        middle.tool = ToolId::new("t2");
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::OnLine, 1.0)),
            Operation::Profile(middle),
            Operation::Profile(profile("o3", "i1", Side::OnLine, 2.0)),
        ];
        let warnings = check(&project, &[]);
        let hints = with_code(&warnings, WarningCode::ToolChangeAvoidable);
        assert_eq!(hints.len(), 1, "one hint: {warnings:?}");
        assert_eq!(hints[0].param("tool"), Some("t1"));
        assert_eq!(hints[0].param("tool_name"), Some("6.35 mm upcut"));
        assert_eq!(hints[0].param("ops"), Some("`o1`, `o3`"));
        let text = hints[0].to_string();
        assert!(text.contains("`o1`") && text.contains("`o3`"), "{text}");
    }

    /// T1, T1, T2: every tool's operations are adjacent, so no hint. A
    /// disabled operation does not count: T1, T2 (disabled), T1 is quiet.
    #[test]
    fn adjacent_same_tool_is_quiet() {
        let mut project = part_and_hole();
        let mut t2 = tool_6_35_upcut();
        t2.id = ToolId::new("t2");
        project.tools.push(t2);
        let mut last = profile("o3", "i1", Side::OnLine, 2.0);
        last.tool = ToolId::new("t2");
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::OnLine, 1.0)),
            Operation::Profile(profile("o2", "i2", Side::Inside, 3.0)),
            Operation::Profile(last),
        ];
        let warnings = check(&project, &[]);
        assert!(
            with_code(&warnings, WarningCode::ToolChangeAvoidable).is_empty(),
            "quiet: {warnings:?}"
        );

        let mut disabled = profile("o2", "i2", Side::Inside, 3.0);
        disabled.tool = ToolId::new("t2");
        disabled.enabled = false;
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::OnLine, 1.0)),
            Operation::Profile(disabled),
            Operation::Profile(profile("o3", "i1", Side::OnLine, 2.0)),
        ];
        let warnings = check(&project, &[]);
        assert!(
            with_code(&warnings, WarningCode::ToolChangeAvoidable).is_empty(),
            "a disabled operation does not split a run: {warnings:?}"
        );
    }
}
