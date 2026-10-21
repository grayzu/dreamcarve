// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Operation-order warnings (`docs/specs/phase-2/design.md`, "Algorithms →
//! Operation-order warnings (dc-cam::order)"; `docs/research/toolpath-ordering.md`
//! §5.5, invariant O8, decision D7). Operations always run in the order the
//! project lists them; this module never reorders, it only says when that
//! order cuts inside a part an earlier operation has already freed or left
//! held only by tabs (#269), and when a tool is fitted more than once
//! because its operations are not adjacent.
//!
//! Nothing here depends on Y-up or Y-down: containment is symmetric under
//! the flip.

use crate::error::ShapeRef;
use crate::overlap::region_holds_path;
use crate::profile::{Hold, cut_outlines, same_contour};
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
/// - in the same order, [`Warning::CutInsideTabHeldPart`] for every such
///   target that lies inside an outline an earlier Profile cut through the
///   stock round with tabs, naming the innermost such outline, when the
///   later operation cuts deeper than half the stock ([`cuts_deep`]) and
///   the same earlier operation freed no outline round it (#269);
/// - [`Warning::ToolChangeAvoidable`] once per tool that the enabled
///   operations, in order, use again after a different tool, by the
///   tool's first use.
#[must_use]
pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning> {
    let targets: Vec<Vec<(ShapeRef, Target)>> = generated
        .iter()
        .map(|(op, _)| targets_of(project, op))
        .collect();
    let freeing: Vec<Freed> = generated
        .iter()
        .map(|(op, toolpath)| freed_by(project, op, toolpath))
        .collect();
    let deep: Vec<bool> = generated
        .iter()
        .map(|(op, _)| cuts_deep(project, op))
        .collect();

    let mut warnings = Vec::new();
    for (j, (later, _)) in generated.iter().enumerate() {
        let Some(shapes) = targets.get(j) else {
            continue;
        };
        for (shape, target) in shapes {
            for (i, (earlier, _)) in generated.iter().enumerate().take(j) {
                let Some(frees) = freeing.get(i) else {
                    continue;
                };
                let freed = targets.get(i).and_then(|outlines| {
                    outlines.iter().find(|(s, o)| match o {
                        Target::Outline(outline) => frees.has(s) && target.lies_inside(outline),
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
                    // A loose piece is the worse news; the tabs round it
                    // do not hold it.
                    continue;
                }
                if !deep.get(j).copied().unwrap_or(false) {
                    continue;
                }
                let held = targets.get(i).and_then(|outlines| {
                    outlines
                        .iter()
                        .filter_map(|(s, o)| match o {
                            Target::Outline(outline)
                                if frees.holds_by_tabs(s) && target.lies_inside(outline) =>
                            {
                                Some((s, outline.signed_area_mm2().abs()))
                            }
                            _ => None,
                        })
                        // Nested outlines round one target: the smallest is
                        // the innermost, the piece the cut is on.
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                });
                if let Some((held_shape, _)) = held {
                    warnings.push(Warning::CutInsideTabHeldPart {
                        op: later.id().clone(),
                        earlier_op: earlier.id().clone(),
                        shape: shape.clone(),
                        held: held_shape.clone(),
                    });
                }
            }
        }
    }
    warnings.extend(tool_change_hints(project));
    warnings
}

/// The target outlines an operation leaves loose, or held only by tabs,
/// with whatever is inside them.
enum Freed {
    /// None: not a Profile, or one that stops above the stock bottom.
    Nothing,
    /// Every outline: a Profile through the stock with no tabs.
    All,
    /// The outlines a tabbed Profile through the stock cuts, each loose
    /// when its tabs leave it untabbed (#109), else held by them (#269).
    Tabbed(Vec<(ShapeRef, Hold)>),
}

impl Freed {
    /// True when outline `shape` is left loose.
    fn has(&self, shape: &ShapeRef) -> bool {
        match self {
            Self::Nothing => false,
            Self::All => true,
            Self::Tabbed(outlines) => outlines
                .iter()
                .any(|(s, hold)| s == shape && *hold == Hold::Free),
        }
    }

    /// True when outline `shape` is held only by its tabs.
    fn holds_by_tabs(&self, shape: &ShapeRef) -> bool {
        match self {
            Self::Nothing | Self::All => false,
            Self::Tabbed(outlines) => outlines
                .iter()
                .any(|(s, hold)| s == shape && *hold == Hold::Tabs),
        }
    }
}

/// What `op`, with the toolpath generated for it, leaves loose: a Profile
/// whose final pass reaches the stock bottom (a cutting move ends at
/// Z ≤ 0, so its depth is at least the thickness) frees every outline it
/// leaves without tabs. Any side frees: Outside and On line free the part,
/// Inside frees the hole's slug. Tabs hold an outline through the final
/// pass, so a tabbed Profile frees only the outlines it cuts with no tab:
/// its holes unless `on_holes`, and outlines too short for one (#109).
/// The outlines it cuts with tabs are held only by them (#269).
fn freed_by(project: &Project, op: &Operation, toolpath: &Toolpath) -> Freed {
    let Operation::Profile(profile) = op else {
        return Freed::Nothing;
    };
    let through = toolpath
        .moves
        .iter()
        .any(|m| !m.is_rapid() && m.to.z <= EPS_MM);
    if !through {
        return Freed::Nothing;
    }
    match cut_outlines(project, profile) {
        None => Freed::All,
        Some(outlines) => Freed::Tabbed(outlines),
    }
}

/// True when `op` cuts deeper than half the stock, which takes in every
/// cut that reaches the stock bottom, through-cut allowance and all: a
/// cut heavy enough to flex a part held only by tabs, or break one
/// (#269). A light cut, such as a shallow engraving after a tabbed
/// cut-out, is normal practice. Reads the operation's `depth_mm` against
/// the stock thickness; false when either is unset, which validation
/// reports.
fn cuts_deep(project: &Project, op: &Operation) -> bool {
    match (op.depth_mm(), project.stock.thickness_mm) {
        (Some(depth_mm), Some(thickness_mm)) => depth_mm > thickness_mm / 2.0 + EPS_MM,
        _ => false,
    }
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
    use crate::pocket::generate_pocket;
    use crate::profile::generate_profile;
    use crate::warning::WarningCode;
    use dc_geometry::{Point, Polyline, Transform, Vertex};
    use dc_project::test_support::{sample_pocket, sample_project, tool_6_35_upcut};
    use dc_project::{
        DefinitionId, EngraveOp, FeatureDefinition, FeatureInstance, InstanceId, MachiningOrder,
        OperationId, PocketOp, ProfileOp, Side, Stock, TabPlacement, TabShape, TabSpec, ToolId,
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
            on_holes: false,
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
                    Operation::Pocket(pocket) => generate_pocket(project, pocket),
                    Operation::Drill(_) => panic!("these tests use no Drill operations"),
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

    /// Acceptance test 32's fixture: 160 × 120 × 6 mm stock, a 100 × 60 mm
    /// rectangle `i1` ("Frame") at (30, 30) and a 30 × 20 mm rectangle `i2`
    /// ("Recess") at (65, 50), inside it. No operations.
    fn frame_and_recess() -> Project {
        let mut project = sample_project();
        project.stock = Stock {
            width_mm: 160.0,
            height_mm: 120.0,
            thickness_mm: Some(6.0),
        };
        project.definitions.clear();
        project.instances.clear();
        for (n, name, (x, y), (w, h)) in [
            (1, "Frame", (30.0, 30.0), (100.0, 60.0)),
            (2, "Recess", (65.0, 50.0), (30.0, 20.0)),
        ] {
            let def = FeatureDefinition {
                id: DefinitionId::new(format!("d{n}")),
                name: name.into(),
                profiles: vec![Polyline::rect(Point { x: 0.0, y: 0.0 }, w, h)],
            };
            project.instances.push(FeatureInstance {
                id: InstanceId::new(format!("i{n}")),
                definition: def.id.clone(),
                transform: Transform::translate(x, y),
                name: name.into(),
                visible: true,
                locked: false,
                z_top_mm: Some(6.0),
                z_bottom_mm: Some(0.0),
            });
            project.definitions.insert(def.id.clone(), def);
        }
        project.operations.clear();
        project
    }

    /// A 3 mm deep offset Pocket `id` of `target`, cutting with `t1`.
    fn pocket(id: &str, target: &str) -> PocketOp {
        PocketOp {
            id: OperationId::new(id),
            name: id.to_uppercase(),
            targets: vec![InstanceId::new(target)],
            ..sample_pocket()
        }
    }

    /// O8, acceptance test 32 (#251): an Outside Profile through the stock
    /// (no tabs) frees the 100 × 60 frame; a later Pocket of the recess
    /// inside it cuts a loose part, so exactly one `cut_inside_freed_part`
    /// names both operations.
    #[test]
    fn pocket_inside_freed_part_warns() {
        let mut project = frame_and_recess();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Pocket(pocket("o2", "i2")),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        assert_eq!(found.len(), 1, "one warning: {warnings:?}");
        let w = found[0];
        let (op, earlier_op, shape, freed) = freed_of(w);
        assert_eq!((op, earlier_op), ("o2", "o1"));
        assert_eq!(
            (shape.instance.as_str(), shape.instance_name.as_str()),
            ("i2", "Recess")
        );
        assert_eq!(
            (freed.instance.as_str(), freed.instance_name.as_str()),
            ("i1", "Frame")
        );
        // The warning belongs to the Pocket (`op`); its text names the
        // Profile that freed the frame.
        let text = w.to_string();
        assert!(text.contains("`o1`"), "names the Profile: {text}");
        assert!(
            text.contains("`Recess`"),
            "names the pocketed shape: {text}"
        );
    }

    /// O8, acceptance test 32 (#251): "the reverse order is quiet". The
    /// Pocket is cut while the frame is still held by the stock.
    #[test]
    fn pocket_before_freeing_profile_is_quiet() {
        let mut project = frame_and_recess();
        project.operations = vec![
            Operation::Pocket(pocket("o1", "i2")),
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
    /// frees nothing: the later operation cuts a held part (which
    /// `cut_inside_tab_held_part_warns` covers, #269).
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

    /// The operation, earlier operation, shape and held outline of a
    /// `cut_inside_tab_held_part` warning.
    fn held_of(w: &Warning) -> (&str, &str, &ShapeRef, &ShapeRef) {
        let Warning::CutInsideTabHeldPart {
            op,
            earlier_op,
            shape,
            held,
        } = w
        else {
            panic!("{w:?}");
        };
        (op.as_str(), earlier_op.as_str(), shape, held)
    }

    /// The `(op, earlier_op, shape instance, held instance)` of every
    /// `cut_inside_tab_held_part` warning in `warnings`, in order.
    fn held_cuts(warnings: &[Warning]) -> Vec<(&str, &str, &str, &str)> {
        with_code(warnings, WarningCode::CutInsideTabHeldPart)
            .into_iter()
            .map(held_of)
            .map(|(op, earlier, shape, held)| {
                (op, earlier, shape.instance.as_str(), held.instance.as_str())
            })
            .collect()
    }

    /// #269: the two-operation star of `tests/manual/star_with_hole`
    /// redrawn. An Outside Profile through the stock with tabs on the
    /// rectangle holds it by its tabs only; a later Inside Profile of the
    /// circle inside it cuts that tab-held part, so exactly one
    /// `cut_inside_tab_held_part` names both operations and both shapes.
    /// The rectangle is held, not freed, so `cut_inside_freed_part` stays
    /// quiet. The message says what to do: move the operation earlier, or
    /// cut the hole with its part in one operation (#109).
    #[test]
    fn cut_inside_tab_held_part_warns() {
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
            "the tabs hold the part: {warnings:?}"
        );
        let found = with_code(&warnings, WarningCode::CutInsideTabHeldPart);
        assert_eq!(found.len(), 1, "one warning: {warnings:?}");
        let w = found[0];
        let (op, earlier_op, shape, held) = held_of(w);
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
                held.instance.as_str(),
                held.instance_name.as_str(),
                held.profile
            ),
            ("i1", "Rectangle 1", 0)
        );
        assert_eq!(w.op().map(OperationId::as_str), Some("o2"));
        let text = w.to_string();
        assert!(text.contains("`o1`"), "names the earlier operation: {text}");
        assert!(text.contains("`Hole`"), "names the shape: {text}");
        assert!(
            text.contains("`Rectangle 1`"),
            "names the held outline: {text}"
        );
        assert!(text.contains("tabs"), "says the tabs hold it: {text}");
        assert!(
            text.contains("move this operation before `o1`"),
            "suggests cutting inside first: {text}"
        );
        assert!(
            text.contains("same operation"),
            "suggests one operation for the part and its hole: {text}"
        );
    }

    /// #269: a Pocket through the stock and a 4 mm Engrave of an open line
    /// inside the tab-held rectangle warn too, as for a freed part (#206):
    /// each cuts deeper than half the 6 mm stock. A Profile with tabs that
    /// stops above the stock bottom leaves the part in the stock, so it
    /// holds nothing by its tabs alone.
    #[test]
    fn deep_cut_inside_tab_held_part_warns() {
        let mut project = part_and_line((20.0, 15.0), (50.0, 15.0));
        let mut outline = profile("o1", "i1", Side::Outside, 6.0);
        outline.tabs = Some(four_tabs());
        let mut through = pocket("o2", "i2");
        through.depth_mm = Some(6.0);
        project.operations = vec![
            Operation::Profile(outline.clone()),
            Operation::Pocket(through),
            Operation::Engrave(engrave("o3", "i3", 4.0)),
        ];
        let warnings = check_all(&project);
        assert_eq!(
            held_cuts(&warnings),
            vec![("o2", "o1", "i2", "i1"), ("o3", "o1", "i3", "i1")],
            "{warnings:?}"
        );

        outline.depth_mm = Some(3.0);
        project.operations[0] = Operation::Profile(outline);
        let warnings = check_all(&project);
        assert!(held_cuts(&warnings).is_empty(), "not through: {warnings:?}");
    }

    /// #269: a light cut on a tab-held part is normal practice, as the
    /// plaque's 1 mm Engrave after its tabbed cut-out: only a cut deeper
    /// than half the stock, or through it, risks flexing the part or
    /// breaking a tab. A 1 mm Engrave and a Pocket exactly half the 6 mm
    /// stock deep are quiet.
    #[test]
    fn shallow_engrave_inside_tab_held_part_is_quiet() {
        let mut project = part_and_line((20.0, 15.0), (50.0, 15.0));
        let mut outline = profile("o1", "i1", Side::Outside, 6.0);
        outline.tabs = Some(four_tabs());
        let mut half = pocket("o3", "i2");
        half.depth_mm = Some(3.0);
        project.operations = vec![
            Operation::Profile(outline),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
            Operation::Pocket(half),
        ];
        let warnings = check_all(&project);
        assert!(held_cuts(&warnings).is_empty(), "light cuts: {warnings:?}");
    }

    /// #269: the hole first, then the tabbed outline. The outline is not
    /// inside the hole, so nothing is cut in a tab-held part, and nothing
    /// in a freed one.
    #[test]
    fn inside_first_is_quiet() {
        let mut project = part_and_hole();
        let mut outline = profile("o2", "i1", Side::Outside, 6.0);
        outline.tabs = Some(four_tabs());
        project.operations = vec![
            Operation::Profile(profile("o1", "i2", Side::Inside, 6.0)),
            Operation::Profile(outline),
        ];
        let warnings = check_all(&project);
        for code in [
            WarningCode::CutInsideTabHeldPart,
            WarningCode::CutInsideFreedPart,
        ] {
            assert!(
                with_code(&warnings, code).is_empty(),
                "{code}: {warnings:?}"
            );
        }
    }

    /// #269, #109: one tabbed Profile of the rectangle and its hole cuts
    /// the hole first, inside its own operation, so neither order warning
    /// fires.
    #[test]
    fn one_operation_part_and_hole_is_quiet() {
        let mut project = part_and_hole();
        let mut both = profile("o1", "i1", Side::Outside, 6.0);
        both.targets.push(InstanceId::new("i2"));
        both.tabs = Some(four_tabs());
        project.operations = vec![Operation::Profile(both)];
        let warnings = check_all(&project);
        for code in [
            WarningCode::CutInsideTabHeldPart,
            WarningCode::CutInsideFreedPart,
        ] {
            assert!(
                with_code(&warnings, code).is_empty(),
                "{code}: {warnings:?}"
            );
        }
    }

    /// #269: a 4 mm cut on a hole's slug that the hole's own tabs hold
    /// (two of four fit the 24 mm hole's path, `on_holes`) names the hole, the
    /// innermost tab-held outline round it, not the part. A slug the
    /// tabbed Profile cuts free warns `cut_inside_freed_part` alone: the
    /// slug is loose, whatever holds the part round it.
    #[test]
    fn tab_held_warning_names_the_innermost_outline() {
        let mut project = slug_cut_after(true, 12.0);
        // Deeper than half the stock, so heavy enough to warn.
        if let Some(Operation::Engrave(dot)) = project.operations.get_mut(1) {
            dot.depth_mm = Some(4.0);
        }
        let warnings = check_all(&project);
        assert_eq!(
            held_cuts(&warnings),
            vec![("o2", "o1", "i3", "i2")],
            "{warnings:?}"
        );
        let warnings = check_all(&slug_cut_after(false, 5.0));
        assert!(held_cuts(&warnings).is_empty(), "freed slug: {warnings:?}");
        assert_eq!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).len(),
            1,
            "{warnings:?}"
        );
    }

    /// [`part_and_hole`] with the hole `hole_radius_mm` in radius, one
    /// Outside Profile `o1` of the rectangle and the hole together with
    /// [`four_tabs`] (`on_holes` as given), then an Engrave `o2` of a
    /// 3 mm circle `i3` at the hole's centre: a cut on the hole's slug.
    fn slug_cut_after(on_holes: bool, hole_radius_mm: f64) -> Project {
        let mut project = part_and_hole();
        if let Some(hole) = project.definitions.get_mut(&DefinitionId::new("d2")) {
            hole.profiles = vec![Polyline::circle(Point { x: 0.0, y: 0.0 }, hole_radius_mm)];
        }
        let dot = FeatureDefinition {
            id: DefinitionId::new("d3"),
            name: "Dot".into(),
            profiles: vec![Polyline::circle(Point { x: 0.0, y: 0.0 }, 1.5)],
        };
        project.definitions.insert(dot.id.clone(), dot);
        project.instances.push(FeatureInstance {
            id: InstanceId::new("i3"),
            definition: DefinitionId::new("d3"),
            transform: Transform::translate(35.0, 25.0),
            name: "Dot".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        let mut both = profile("o1", "i1", Side::Outside, 6.0);
        both.targets.push(InstanceId::new("i2"));
        both.tabs = Some(TabSpec {
            on_holes,
            ..four_tabs()
        });
        project.operations = vec![
            Operation::Profile(both),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
        ];
        project
    }

    /// #109: tabs hold only the outlines they are on. A tabbed Profile
    /// cuts its hole free when the hole gets no tabs, by default or
    /// because none fits its tool path, so a later cut on the slug warns;
    /// a hole that keeps its tabs is held. The tabbed rectangle is held
    /// either way.
    #[test]
    fn tabbed_profile_frees_the_outlines_it_leaves_untabbed() {
        for (on_holes, hole_radius_mm, freed) in [
            // Holes get no tabs by default.
            (false, 5.0, true),
            // The 10 mm hole's 11.5 mm path has room for no tab.
            (true, 5.0, true),
            // Two of the four fit the 24 mm hole's 55.4 mm path.
            (true, 12.0, false),
        ] {
            let warnings = check_all(&slug_cut_after(on_holes, hole_radius_mm));
            let found: Vec<_> = with_code(&warnings, WarningCode::CutInsideFreedPart)
                .into_iter()
                .map(freed_of)
                .map(|(op, earlier, shape, outline)| {
                    (
                        op,
                        earlier,
                        shape.instance.as_str(),
                        outline.instance.as_str(),
                    )
                })
                .collect();
            let expected = if freed {
                vec![("o2", "o1", "i3", "i2")]
            } else {
                vec![]
            };
            assert_eq!(found, expected, "on_holes {on_holes}, r {hole_radius_mm}");
        }
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
