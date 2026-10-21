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

use crate::error::ShapeRef;
use crate::overlap::region_holds_path;
use crate::profile::same_contour;
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_geometry::{Contour, EPS_MM, GeometryWarning, Polyline, nest};
use dc_project::{Operation, OperationId, Project, Side, ToolId};

/// Checks the order of the operations of `project`. `generated` holds, in
/// project order, every enabled operation whose toolpath was generated,
/// with that toolpath (project space, mm, Y-up, Z-up); operations that
/// failed are simply absent and take no part.
///
/// Returns, in a deterministic order:
///
/// - [`Warning::CutInsideFreedPart`] for every target of a later
///   operation that lies inside an outline an earlier Profile cut through
///   the stock, by operation order, then target order, then the freeing
///   operation's order. Targets are closed outlines, and the open paths
///   an Engrave or an On line Profile cuts (#206);
/// - [`Warning::ToolChangeAvoidable`] once per tool that the enabled
///   operations, in order, use again after a different tool, by the
///   tool's first use.
#[must_use]
pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning> {
    let targets: Vec<Vec<(ShapeRef, Target)>> = generated
        .iter()
        .map(|(op, _)| targets_of(project, op))
        .collect();
    let freeing: Vec<bool> = generated
        .iter()
        .map(|(op, toolpath)| frees_its_targets(op, toolpath))
        .collect();

    let mut warnings = Vec::new();
    for (j, (later, _)) in generated.iter().enumerate() {
        let Some(shapes) = targets.get(j) else {
            continue;
        };
        for (shape, target) in shapes {
            for (i, (earlier, _)) in generated.iter().enumerate().take(j) {
                if !freeing.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let freed = targets.get(i).and_then(|outlines| {
                    outlines.iter().find(|(_, o)| match o {
                        Target::Outline(outline) => target.lies_inside(outline),
                        Target::Path(_) => false,
                    })
                });
                if let Some((freed_shape, _)) = freed {
                    warnings.push(Warning::CutInsideFreedPart {
                        op: later.id().clone(),
                        earlier_op: earlier.id().clone(),
                        shape: shape.clone(),
                        freed: freed_shape.clone(),
                    });
                }
            }
        }
    }
    warnings.extend(tool_change_hints(project));
    warnings
}

/// True when `op`, with the toolpath generated for it, leaves whatever is
/// inside its target outlines loose: a Profile whose final pass reaches
/// the stock bottom (a cutting move ends at Z ≤ 0, so its depth is at
/// least the thickness) and that leaves no tabs. Any side frees: Outside
/// and On line free the part, Inside frees the hole's slug. Tabs hold the
/// part through the final pass, so a tabbed Profile frees nothing.
fn frees_its_targets(op: &Operation, toolpath: &Toolpath) -> bool {
    let Operation::Profile(profile) = op else {
        return false;
    };
    if profile.tabs.is_some() {
        return false;
    }
    toolpath
        .moves
        .iter()
        .any(|m| !m.is_rapid() && m.to.z <= EPS_MM)
}

/// One target as [`check`] reads it.
enum Target {
    /// A closed profile: the outline a Profile frees, or a shape cut
    /// inside one.
    Outline(Contour),
    /// An open path the operation cuts along (#206). It frees nothing.
    Path(Polyline),
}

impl Target {
    /// True when this target lies inside `outer`: an outline as
    /// [`lies_inside`] says, an open path when all its points lie inside
    /// or on `outer` ([`region_holds_path`]), so one that crosses it does
    /// not count, as crossing outlines do not.
    fn lies_inside(&self, outer: &Contour) -> bool {
        match self {
            Self::Outline(inner) => lies_inside(inner, outer),
            Self::Path(path) => nest(vec![outer.clone()])
                .ok()
                .and_then(|(regions, _)| regions.into_iter().next())
                .is_some_and(|region| region_holds_path(&region, path)),
        }
    }
}

/// True when `inner` lies within `outer` (edges may touch) or is the same
/// outline, judged on the true curves as [`nest`] does; false when the
/// two cross or are apart. Which is inside which is read from which one
/// `nest` makes the outer, so a shape that encloses the freed outline
/// does not count as inside it.
fn lies_inside(inner: &Contour, outer: &Contour) -> bool {
    let Ok((regions, warnings)) = nest(vec![outer.clone(), inner.clone()]) else {
        return false;
    };
    if warnings
        .iter()
        .any(|w| matches!(w, GeometryWarning::DuplicateContourDropped { .. }))
    {
        return true;
    }
    match regions.as_slice() {
        [region] => region.holes().len() == 1 && same_contour(outer, region.outer()),
        _ => false,
    }
}

/// Every profile of every target instance of `op` that `op` cuts, placed
/// in project space, in target then profile order: every closed profile,
/// and the open paths an Engrave or an On line Profile cuts along (#175,
/// #206). Other operations skip open paths, so they are left out, as are
/// open paths with no length and anything validation already rejects (a
/// missing instance or definition, a degenerate outline): this check only
/// warns.
fn targets_of(project: &Project, op: &Operation) -> Vec<(ShapeRef, Target)> {
    let cuts_open = match op {
        Operation::Engrave(_) => true,
        Operation::Profile(profile) => profile.side == Side::OnLine,
        Operation::Pocket(_) | Operation::Drill(_) => false,
    };
    let mut shapes = Vec::new();
    for instance_id in op.targets() {
        let Some(instance) = project.instances.iter().find(|i| &i.id == instance_id) else {
            continue;
        };
        let Some(definition) = project.definitions.get(&instance.definition) else {
            continue;
        };
        for (profile, polyline) in definition.profiles.iter().enumerate() {
            let target = if polyline.is_closed() {
                instance
                    .transform
                    .apply_polyline(polyline)
                    .and_then(Contour::new)
                    .ok()
                    .map(Target::Outline)
            } else if cuts_open {
                instance
                    .transform
                    .apply_polyline(polyline)
                    .ok()
                    .filter(|path| path.length_mm() > EPS_MM)
                    .map(Target::Path)
            } else {
                None
            };
            let Some(target) = target else {
                continue;
            };
            shapes.push((
                ShapeRef {
                    instance: instance.id.clone(),
                    instance_name: instance.name.clone(),
                    profile,
                },
                target,
            ));
        }
    }
    shapes
}

/// One hint per tool that the enabled operations, in project order, come
/// back to after a different tool, by the tool's first use. Operations
/// are never reordered (D7); the hint lists the tool's operations so the
/// user can put them together.
fn tool_change_hints(project: &Project) -> Vec<Warning> {
    let enabled: Vec<&Operation> = project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .collect();
    let mut by_first_use: Vec<(&ToolId, Vec<&OperationId>)> = Vec::new();
    for op in &enabled {
        match by_first_use.iter_mut().find(|(tool, _)| *tool == op.tool()) {
            Some((_, ops)) => ops.push(op.id()),
            None => by_first_use.push((op.tool(), vec![op.id()])),
        }
    }
    by_first_use
        .into_iter()
        .filter(|(tool, ops)| {
            // The tool's uses are one run when no other tool's operation
            // sits between its first and last.
            let run_len = enabled
                .iter()
                .skip_while(|op| op.tool() != *tool)
                .take_while(|op| op.tool() == *tool)
                .count();
            run_len < ops.len()
        })
        .map(|(tool, ops)| {
            let name = project
                .tools
                .iter()
                .find(|t| &t.id == tool)
                .map_or("?", |t| t.name.as_str());
            Warning::ToolChangeAvoidable {
                tool_id: tool.clone(),
                tool: name.to_owned(),
                ops: ops.into_iter().cloned().collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engrave::generate_engrave;
    use crate::error::ShapeRef;
    use crate::profile::generate_profile;
    use crate::warning::WarningCode;
    use dc_geometry::{Point, Polyline, Transform, Vertex};
    use dc_project::test_support::{sample_project, tool_6_35_upcut};
    use dc_project::{
        DefinitionId, EngraveOp, FeatureDefinition, FeatureInstance, InstanceId, MachiningOrder,
        OperationId, ProfileOp, Side, TabPlacement, TabShape, TabSpec, ToolId,
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
                let generated = match op {
                    Operation::Profile(profile) => generate_profile(project, profile),
                    Operation::Engrave(engrave) => generate_engrave(project, engrave),
                    _ => panic!("these tests use Profile and Engrave operations"),
                };
                let (toolpath, _) =
                    generated.unwrap_or_else(|e| panic!("{} generates: {e}", op.id()));
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
        warnings.iter().filter(|w| w.code() == code).collect()
    }

    /// The operation, earlier operation, shape and freed outline of a
    /// `cut_inside_freed_part` warning.
    fn freed_of(w: &Warning) -> (&str, &str, &ShapeRef, &ShapeRef) {
        let Warning::CutInsideFreedPart {
            op,
            earlier_op,
            shape,
            freed,
        } = w
        else {
            panic!("{w:?}");
        };
        (op.as_str(), earlier_op.as_str(), shape, freed)
    }

    /// The operation and the earlier one of a `cut_inside_freed_part`
    /// warning.
    fn ops_of(w: &Warning) -> (&str, &str) {
        let (op, earlier_op, _, _) = freed_of(w);
        (op, earlier_op)
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
        let (op, earlier_op, shape, freed) = freed_of(w);
        assert_eq!((op, earlier_op), ("o2", "o1"));
        assert_eq!(
            (
                shape.instance.as_str(),
                shape.instance_name.as_str(),
                shape.profile
            ),
            ("i2", "Hole", 0)
        );
        assert_eq!(
            (
                freed.instance.as_str(),
                freed.instance_name.as_str(),
                freed.profile
            ),
            ("i1", "Rectangle 1", 0)
        );
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
        assert_eq!(ops_of(found[0]), ("o2", "o1"));
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
            .map(|w| ops_of(w))
            .collect();
        assert_eq!(ops, vec![("o2", "o1"), ("o3", "o1")]);
        assert_eq!(warnings, check_all(&project), "deterministic");
    }

    /// [`part_and_hole`] with an open line `i3` from `from` to `to`.
    fn part_and_line(from: (f64, f64), to: (f64, f64)) -> Project {
        let mut project = part_and_hole();
        let line = FeatureDefinition {
            id: DefinitionId::new("d3"),
            name: "Line".into(),
            profiles: vec![Polyline::new(
                vec![Vertex::line(from.0, from.1), Vertex::line(to.0, to.1)],
                false,
            )],
        };
        project.definitions.insert(line.id.clone(), line);
        project.instances.push(FeatureInstance {
            id: InstanceId::new("i3"),
            definition: DefinitionId::new("d3"),
            transform: Transform::IDENTITY,
            name: "Slot".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        project
    }

    /// An Engrave `id` on `target`, `depth_mm` deep, cutting with `t1`.
    fn engrave(id: &str, target: &str, depth_mm: f64) -> EngraveOp {
        EngraveOp {
            id: OperationId::new(id),
            name: id.to_uppercase(),
            enabled: true,
            targets: vec![InstanceId::new(target)],
            tool: ToolId::new("t1"),
            depth_mm: Some(depth_mm),
            step_down_mm: None,
            alternate_direction: false,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// #206, O8: an Engrave and an On line Profile of an open line inside
    /// the rectangle, after an Outside Profile freed it, both cut in a
    /// loose part. An Inside Profile of the line cuts nothing there (it
    /// skips open paths), so it is quiet.
    #[test]
    fn open_path_inside_freed_part_warns() {
        let mut project = part_and_line((20.0, 15.0), (50.0, 15.0));
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
            Operation::Profile(profile("o3", "i3", Side::OnLine, 1.0)),
            Operation::Profile(profile("o4", "i3", Side::Inside, 1.0)),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        let ops: Vec<_> = found.iter().map(|w| ops_of(w)).collect();
        assert_eq!(ops, vec![("o2", "o1"), ("o3", "o1")], "{warnings:?}");
        for w in found {
            let (_, _, shape, freed) = freed_of(w);
            assert_eq!(
                (
                    shape.instance.as_str(),
                    shape.instance_name.as_str(),
                    shape.profile
                ),
                ("i3", "Slot", 0)
            );
            assert_eq!(freed.instance.as_str(), "i1");
            assert!(w.to_string().contains("`Slot`"), "{w}");
        }
    }

    /// #206, O8: an open line that leaves the freed outline is not inside
    /// it, as crossing closed shapes are not; nor is one cut before the
    /// outline is freed.
    #[test]
    fn open_path_crossing_a_freed_outline_is_quiet() {
        let mut project = part_and_line((5.0, 15.0), (30.0, 15.0));
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "crossing: {warnings:?}"
        );

        let mut project = part_and_line((20.0, 15.0), (50.0, 15.0));
        project.operations = vec![
            Operation::Engrave(engrave("o1", "i3", 1.0)),
            Operation::Profile(profile("o2", "i1", Side::Outside, 6.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "cut first: {warnings:?}"
        );
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
        let Warning::ToolChangeAvoidable { tool_id, tool, ops } = hints[0] else {
            panic!("{:?}", hints[0]);
        };
        assert_eq!(tool_id.as_str(), "t1");
        assert_eq!(tool, "6.35 mm upcut");
        assert_eq!(ops, &[OperationId::new("o1"), OperationId::new("o3")]);
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
