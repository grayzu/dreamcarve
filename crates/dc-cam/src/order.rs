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
//! It also checks the start plane of every operation cut from one
//! (`docs/specs/phase-3/design.md`, "Start depth, `start_depth_mismatch`
//! and `start_depth_uncleared`"; capability 41): [`start_plane`] says what
//! the plane rests on, and [`check`] warns when an earlier Pocket clears
//! the operation's area to another depth.
//!
//! Nothing here depends on Y-up or Y-down: containment is symmetric under
//! the flip.

use crate::error::ShapeRef;
use crate::overlap::region_holds_path;
use crate::profile::{Hold, cut_outlines, same_contour};
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_geometry::{Contour, EPS_MM, GeometryWarning, Polyline, nest};
use dc_project::{Operation, OperationId, PocketOp, Project, ToolId};

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
///   later operation cuts deeper than half the stock (`cuts_deep`) and
///   the same earlier operation freed no outline round it (#269);
/// - [`Warning::ToolChangeAvoidable`] once per tool that the enabled
///   operations' tool runs, in order, use again after a different tool,
///   by the tool's first use; a carve's clearing passes are runs of their
///   own end mills before its V-bit's (#406), whether or not a pass
///   reaches anything;
/// - [`Warning::StartDepthMismatch`] for every enabled operation cut from
///   a start plane (a V-Carve) that an earlier Pocket at another depth
///   contains ([`start_plane`]), in project order. These read the project
///   only, not `generated`.
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
    warnings.extend(start_depth_warnings(project));
    warnings
}

/// What the start plane of an operation cut from one rests on
/// (`docs/specs/phase-3/design.md`, "Start depth, `start_depth_mismatch`
/// and `start_depth_uncleared`"), from its start depth and
/// [`dc_project::containing_pocket`], which validation and the generators
/// call too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartPlane<'p> {
    /// Start depth 0 and no earlier Pocket contains the targets: the stock
    /// top.
    StockTop,
    /// The containing Pocket is at the start depth (within [`EPS_MM`]): the
    /// plane is its floor.
    Cleared(&'p PocketOp),
    /// The containing Pocket is at another depth, or has none set:
    /// [`Warning::StartDepthMismatch`].
    Mismatch(&'p PocketOp),
    /// No earlier Pocket contains the targets, the start depth is above 0,
    /// and the operation says another program cleared the plane.
    ClearedExternally,
    /// No earlier Pocket contains the targets, the start depth is above 0,
    /// and the operation does not say the plane is cleared: validation's
    /// error `start_depth_uncleared`.
    Uncleared,
}

/// The start plane of `op`; `None` for an operation not cut from one, or
/// one whose start depth validation rejects (not finite, or below 0).
///
/// The containing Pocket is [`dc_project::containing_pocket`], whatever
/// the start depth: a carve from the stock top inside a Pocket is a
/// mismatch too. Without one, a start depth above 0 is uncleared unless
/// the operation vouches for the plane.
#[must_use]
pub fn start_plane<'p>(project: &'p Project, op: &Operation) -> Option<StartPlane<'p>> {
    let setting = start_setting(op)?;
    if !(setting.start_depth_mm.is_finite() && setting.start_depth_mm >= 0.0) {
        return None;
    }
    Some(match dc_project::containing_pocket(project, op) {
        Some(pocket) => {
            let at_start = pocket
                .depth_mm
                .is_some_and(|d| (d - setting.start_depth_mm).abs() <= EPS_MM);
            if at_start {
                StartPlane::Cleared(pocket)
            } else {
                StartPlane::Mismatch(pocket)
            }
        }
        None if setting.start_depth_mm <= 0.0 => StartPlane::StockTop,
        None if setting.cleared_externally => StartPlane::ClearedExternally,
        None => StartPlane::Uncleared,
    })
}

/// The start-plane setting of an operation cut from a start plane.
struct StartSetting {
    /// How far below the stock top the plane is.
    start_depth_mm: f64,
    /// `start_plane_cleared_externally`: the user's word that another
    /// program cleared the plane.
    cleared_externally: bool,
}

/// The start-plane setting of `op`, for the operations cut from a start
/// plane; `None` for the others. This is the list of start-plane
/// operations: a countersink Drill (sub-phase 3.2) and a Photo V-Carve
/// (3.4) join it here, and [`start_plane`] and [`check`] then cover them.
/// An Inlay is exempt (its start plane is the stock top or the plug's own
/// design). The match is exhaustive so a new operation must say.
fn start_setting(op: &Operation) -> Option<StartSetting> {
    match op {
        Operation::VCarve(carve) => Some(StartSetting {
            start_depth_mm: carve.start_depth_mm,
            cleared_externally: carve.start_plane_cleared_externally,
        }),
        Operation::Profile(_)
        | Operation::Pocket(_)
        | Operation::Drill(_)
        | Operation::Bore(_)
        | Operation::Engrave(_) => None,
    }
}

/// [`Warning::StartDepthMismatch`] for every enabled start-plane operation
/// of `project` whose containing Pocket is at another depth, in project
/// order. An uncleared plane adds nothing: it is validation's error
/// `start_depth_uncleared`, reported once, at the start depth, and a
/// project with an error never reaches the generators.
fn start_depth_warnings(project: &Project) -> Vec<Warning> {
    project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .filter_map(|op| match start_plane(project, op)? {
            StartPlane::Mismatch(pocket) => Some(Warning::StartDepthMismatch {
                op: op.id().clone(),
                pocket: pocket.id.clone(),
                start_depth_mm: start_setting(op)?.start_depth_mm,
                pocket_depth_mm: pocket.depth_mm,
            }),
            StartPlane::StockTop
            | StartPlane::Cleared(_)
            | StartPlane::ClearedExternally
            | StartPlane::Uncleared => None,
        })
        .collect()
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
/// those with no placed tab (#105) and none its tab request places (its
/// holes unless `on_holes`, and outlines too short for one, #109). The
/// outlines it cuts with tabs are held only by them (#269).
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
/// in project space, in target then profile order: every closed profile
/// (none for a Left or Right Profile, which skips them, #207), and the
/// open paths an Engrave or an On line, Left or Right Profile cuts along
/// or beside (#175, #206, #207). Other operations skip open paths, so they
/// are left out, as are
/// open paths with no length and anything validation already rejects (a
/// missing instance or definition, a degenerate outline): this check only
/// warns.
fn targets_of(project: &Project, op: &Operation) -> Vec<(ShapeRef, Target)> {
    let cuts_open = match op {
        Operation::Engrave(_) => true,
        Operation::Profile(profile) => profile.side.cuts_open_paths(),
        Operation::Pocket(_) | Operation::Drill(_) | Operation::Bore(_) | Operation::VCarve(_) => {
            false
        }
    };
    // #207: Left and Right skip closed outlines.
    let cuts_closed = match op {
        Operation::Profile(profile) => !profile.side.is_open_path_side(),
        Operation::Pocket(_)
        | Operation::Drill(_)
        | Operation::Bore(_)
        | Operation::Engrave(_)
        | Operation::VCarve(_) => true,
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
                if !cuts_closed {
                    continue;
                }
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
    // Every tool run in cut order (#406): a carve's clearing passes cut
    // with their own end mills before its V-bit.
    let uses: Vec<(&ToolId, &OperationId)> = project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .flat_map(|op| op.run_tools().into_iter().map(move |tool| (tool, op.id())))
        .collect();
    let mut by_first_use: Vec<(&ToolId, Vec<&OperationId>, usize)> = Vec::new();
    for &(tool, op) in &uses {
        match by_first_use.iter_mut().find(|(t, ..)| *t == tool) {
            Some((_, ops, count)) => {
                *count += 1;
                if ops.last() != Some(&op) {
                    ops.push(op);
                }
            }
            None => by_first_use.push((tool, vec![op], 1)),
        }
    }
    by_first_use
        .into_iter()
        .filter(|(tool, _, count)| {
            // The tool's uses are one run when no other tool's run sits
            // between its first and last.
            let run_len = uses
                .iter()
                .skip_while(|(t, _)| t != tool)
                .take_while(|(t, _)| t == tool)
                .count();
            run_len < *count
        })
        .map(|(tool, ops, _)| (tool, ops))
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
    use dc_project::test_support::{
        sample_pocket, sample_project, sample_v_carve, tool_6_35_upcut, tool_v_bit_90,
    };
    use dc_project::{
        DefinitionId, EngraveOp, FeatureDefinition, FeatureInstance, InstanceId, MachiningOrder,
        OperationId, PocketOp, Problem, ProfileOp, Severity, Side, Stock, TabPlacement, TabShape,
        TabSpec, ToolId, VCarveOp, validate,
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
            avoid_corners: false,
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
                    Operation::Bore(bore) => crate::bore::generate_bore(project, bore),
                    Operation::Drill(_) => panic!("these tests use no Drill operations"),
                    Operation::VCarve(_) => panic!("these tests use no V-Carve operations"),
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

    /// Format 18: a Bore is an inside cut that frees nothing, so a Bore of
    /// a hole after the Profile that freed its part warns as a Pocket does,
    /// and before it is quiet.
    #[test]
    fn bore_after_cut_out_warns_cut_inside_freed_part() {
        let bore = || {
            Operation::Bore(dc_project::BoreOp {
                id: OperationId::new("o2"),
                targets: vec![InstanceId::new("i2")],
                ..dc_project::test_support::sample_bore()
            })
        };
        let mut project = part_and_hole();
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            bore(),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        assert_eq!(found.len(), 1, "one warning: {warnings:?}");
        let (op, earlier_op, shape, _) = freed_of(found[0]);
        assert_eq!((op, earlier_op), ("o2", "o1"));
        assert_eq!(shape.instance.as_str(), "i2");
        // The Bore through the stock frees nothing itself.
        project.operations = vec![
            bore(),
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
        ];
        let warnings = check_all(&project);
        assert!(
            with_code(&warnings, WarningCode::CutInsideFreedPart).is_empty(),
            "quiet: {warnings:?}"
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
    /// skips open paths), so it is quiet. #207: a Left Profile of the line
    /// cuts beside it, in the loose part too; a Left Profile of the
    /// rectangle through the stock cuts nothing (it skips closed shapes),
    /// so it frees nothing and the Engrave after it is quiet.
    #[test]
    fn open_path_inside_freed_part_warns() {
        let mut project = part_and_line((20.0, 15.0), (50.0, 15.0));
        project.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Outside, 6.0)),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
            Operation::Profile(profile("o3", "i3", Side::OnLine, 1.0)),
            Operation::Profile(profile("o4", "i3", Side::Inside, 1.0)),
            Operation::Profile(profile("o5", "i3", Side::Left, 1.0)),
        ];
        let warnings = check_all(&project);
        let found = with_code(&warnings, WarningCode::CutInsideFreedPart);
        let ops: Vec<_> = found.iter().map(|w| ops_of(w)).collect();
        assert_eq!(
            ops,
            vec![("o2", "o1"), ("o3", "o1"), ("o5", "o1")],
            "{warnings:?}"
        );
        let mut skipped = part_and_line((20.0, 15.0), (50.0, 15.0));
        skipped.operations = vec![
            Operation::Profile(profile("o1", "i1", Side::Left, 6.0)),
            Operation::Engrave(engrave("o2", "i3", 1.0)),
        ];
        let quiet = check_all(&skipped);
        assert!(
            with_code(&quiet, WarningCode::CutInsideFreedPart).is_empty(),
            "{quiet:?}"
        );
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

    /// #406: a carve's clearing passes are tool runs of their own end
    /// mills, before its V-bit's. A Pocket with T1, then a carve clearing
    /// with T1 and carving with the V-bit T2: T1's runs are adjacent, so no
    /// hint. A carve clearing with T3 before a Pocket with T3: the V-bit
    /// runs between them, so a hint for T3 naming both operations, which
    /// reading each operation's one tool would miss.
    #[test]
    fn clearing_runs_count_for_the_tool_change_hint() {
        let mut project = part_and_hole();
        project.tools.push(tool_v_bit_90());
        let mut t3 = tool_6_35_upcut();
        t3.id = ToolId::new("t3");
        t3.name = "3.175 mm end mill".into();
        t3.diameter_mm = 3.175;
        project.tools.push(t3);
        let clearing = |tool: &str| dc_project::ClearingPass {
            tool: ToolId::new(tool),
            strategy: dc_project::ClearanceStrategy::Offset,
            direction: dc_project::CutDirection::Climb,
            entry: dc_project::Entry::Plunge,
        };
        let carve = |passes: Vec<dc_project::ClearingPass>| VCarveOp {
            id: OperationId::new("o6"),
            targets: vec![InstanceId::new("i1")],
            flat_depth_mm: Some(1.0),
            clearing: passes,
            ..sample_v_carve()
        };
        let pocket_with = |id: &str, tool: &str| PocketOp {
            id: OperationId::new(id),
            tool: ToolId::new(tool),
            ..sample_pocket()
        };

        project.operations = vec![
            Operation::Pocket(pocket_with("o2", "t1")),
            Operation::VCarve(carve(vec![clearing("t1")])),
        ];
        let warnings = check(&project, &[]);
        assert!(
            with_code(&warnings, WarningCode::ToolChangeAvoidable).is_empty(),
            "the Pocket and the clearing run share T1: {warnings:?}"
        );

        project.operations = vec![
            Operation::VCarve(carve(vec![clearing("t3")])),
            Operation::Pocket(pocket_with("o7", "t3")),
        ];
        let warnings = check(&project, &[]);
        let hints = with_code(&warnings, WarningCode::ToolChangeAvoidable);
        assert_eq!(hints.len(), 1, "one hint: {warnings:?}");
        let Warning::ToolChangeAvoidable { tool_id, ops, .. } = hints[0] else {
            panic!("{:?}", hints[0]);
        };
        assert_eq!(tool_id.as_str(), "t3");
        assert_eq!(ops, &[OperationId::new("o6"), OperationId::new("o7")]);
    }

    // Start depth (`docs/specs/phase-3/design.md`, "Start depth,
    // `start_depth_mismatch` and `start_depth_uncleared`"; capability 41).

    /// The sample rectangle `i1` (50 × 30 mm at (10, 10)) with a 10 mm
    /// square `i2` at (20, 20) inside it, the 90° V-bit `t2`, and a V-Carve
    /// `o6` of the square with a 1 mm flat depth (so a 2 mm start depth
    /// counted from the stock top stays within the 3.175 mm cone)
    /// starting `start_depth_mm` below the stock top; before it, when
    /// `pocket_depth_mm` is given, the Pocket `o2` of the rectangle that
    /// deep with the end mill `t1` (which is otherwise left out).
    fn carve_in_pocket(pocket_depth_mm: Option<f64>, start_depth_mm: f64) -> Project {
        let mut project = sample_project();
        project.tools.push(tool_v_bit_90());
        let square = FeatureDefinition {
            id: DefinitionId::new("d2"),
            name: "Square".into(),
            profiles: vec![Polyline::rect(Point { x: 0.0, y: 0.0 }, 10.0, 10.0)],
        };
        project.definitions.insert(square.id.clone(), square);
        project.instances.push(FeatureInstance {
            id: InstanceId::new("i2"),
            definition: DefinitionId::new("d2"),
            transform: Transform::translate(20.0, 20.0),
            name: "Square".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        project.operations.clear();
        match pocket_depth_mm {
            Some(depth_mm) => project.operations.push(Operation::Pocket(PocketOp {
                depth_mm: Some(depth_mm),
                ..sample_pocket()
            })),
            // No operation uses the end mill.
            None => project.tools.retain(|t| t.id.as_str() != "t1"),
        }
        project.operations.push(Operation::VCarve(VCarveOp {
            targets: vec![InstanceId::new("i2")],
            flat_depth_mm: Some(1.0),
            start_depth_mm,
            ..sample_v_carve()
        }));
        project
    }

    /// The V-Carve of [`carve_in_pocket`], to edit.
    fn carve_of(project: &mut Project) -> &mut VCarveOp {
        project
            .operations
            .iter_mut()
            .find_map(|op| match op {
                Operation::VCarve(carve) => Some(carve),
                _ => None,
            })
            .expect("the project has a V-Carve")
    }

    /// The Pocket `o2` of [`carve_in_pocket`], to edit.
    fn pocket_of(project: &mut Project) -> &mut PocketOp {
        project
            .operations
            .iter_mut()
            .find_map(|op| match op {
                Operation::Pocket(pocket) if pocket.id.as_str() == "o2" => Some(pocket),
                _ => None,
            })
            .expect("the project has the Pocket o2")
    }

    /// The V-Carve `o6` of `project`.
    fn the_carve(project: &Project) -> &Operation {
        project
            .operations
            .iter()
            .find(|op| op.id().as_str() == "o6")
            .expect("the project has the V-Carve o6")
    }

    /// What [`start_plane`] says of the V-Carve `o6`, with the containing
    /// Pocket by id.
    fn plane_of(project: &Project) -> Option<(&'static str, Option<String>)> {
        start_plane(project, the_carve(project)).map(|plane| match plane {
            StartPlane::StockTop => ("stock_top", None),
            StartPlane::Cleared(pocket) => ("cleared", Some(pocket.id.to_string())),
            StartPlane::Mismatch(pocket) => ("mismatch", Some(pocket.id.to_string())),
            StartPlane::ClearedExternally => ("cleared_externally", None),
            StartPlane::Uncleared => ("uncleared", None),
        })
    }

    /// [`check`]'s `start_depth_mismatch` warnings for `project`.
    fn mismatches(project: &Project) -> Vec<Warning> {
        check(project, &[])
            .into_iter()
            .filter(|w| w.code() == WarningCode::StartDepthMismatch)
            .collect()
    }

    /// Validation's `start_depth_uncleared` errors for `project`.
    fn uncleared(project: &Project) -> Vec<Problem> {
        validate(project)
            .into_iter()
            .filter(|p| matches!(p, Problem::StartDepthUncleared { .. }))
            .collect()
    }

    /// Acceptance 53: the carve starts on the floor of a Pocket of the
    /// same depth that runs before it, so nothing is reported.
    #[test]
    fn start_depth_matching_an_earlier_pocket_is_quiet() {
        let project = carve_in_pocket(Some(2.0), 2.0);
        assert_eq!(validate(&project), vec![]);
        assert_eq!(plane_of(&project), Some(("cleared", Some("o2".into()))));
        assert_eq!(check(&project, &[]), vec![]);
        // Within EPS_MM is the same depth.
        let project = carve_in_pocket(Some(2.0 + EPS_MM / 2.0), 2.0);
        assert_eq!(plane_of(&project), Some(("cleared", Some("o2".into()))));
        assert_eq!(mismatches(&project), vec![]);
    }

    /// Security review M2: with no Pocket and a start depth above 0, the
    /// plane is uncleared, which is validation's error
    /// `start_depth_uncleared`, located at the start depth. It is reported
    /// once: `check` adds no warning for it (and `export` never reaches
    /// `check` or the generator past a validation error). From the stock
    /// top nothing needs clearing.
    #[test]
    fn start_depth_without_a_pocket_is_an_error_unless_acknowledged() {
        let mut project = carve_in_pocket(None, 2.0);
        assert_eq!(plane_of(&project), Some(("uncleared", None)));
        let errors = uncleared(&project);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].severity(), Severity::Error);
        assert_eq!(
            errors[0].field().as_deref(),
            Some("operations[o6].start_depth_mm")
        );
        assert_eq!(check(&project, &[]), vec![]);

        carve_of(&mut project).start_plane_cleared_externally = true;
        assert_eq!(plane_of(&project), Some(("cleared_externally", None)));
        assert_eq!(uncleared(&project), vec![]);

        let project = carve_in_pocket(None, 0.0);
        assert_eq!(plane_of(&project), Some(("stock_top", None)));
        assert_eq!(validate(&project), vec![]);
        assert_eq!(check(&project, &[]), vec![]);
    }

    /// The user's word that another program cleared the start plane
    /// silences both the error and the check.
    #[test]
    fn acknowledged_start_depth_is_quiet() {
        let mut project = carve_in_pocket(None, 2.0);
        carve_of(&mut project).start_plane_cleared_externally = true;
        assert_eq!(validate(&project), vec![]);
        assert_eq!(plane_of(&project), Some(("cleared_externally", None)));
        assert_eq!(check(&project, &[]), vec![]);
    }

    /// Acceptance 53: a Pocket shallower than the start depth contains the
    /// carve, so `check` warns `start_depth_mismatch` naming it, with both
    /// depths. A carve from the stock top inside a Pocket warns too; a
    /// disabled carve is not checked.
    #[test]
    fn start_depth_with_a_shallower_pocket_names_it() {
        let project = carve_in_pocket(Some(1.0), 2.0);
        assert_eq!(uncleared(&project), vec![]);
        assert_eq!(plane_of(&project), Some(("mismatch", Some("o2".into()))));
        assert_eq!(
            mismatches(&project),
            vec![Warning::StartDepthMismatch {
                op: OperationId::new("o6"),
                pocket: OperationId::new("o2"),
                start_depth_mm: 2.0,
                pocket_depth_mm: Some(1.0),
            }]
        );

        let project = carve_in_pocket(Some(1.0), 0.0);
        assert_eq!(
            mismatches(&project),
            vec![Warning::StartDepthMismatch {
                op: OperationId::new("o6"),
                pocket: OperationId::new("o2"),
                start_depth_mm: 0.0,
                pocket_depth_mm: Some(1.0),
            }]
        );

        let mut project = carve_in_pocket(Some(1.0), 2.0);
        carve_of(&mut project).enabled = false;
        assert_eq!(mismatches(&project), vec![]);
    }

    /// A disabled Pocket clears nothing: the carve's plane is uncleared
    /// (validation's error), not a mismatch with it.
    #[test]
    fn disabled_pocket_does_not_count() {
        let mut project = carve_in_pocket(Some(1.0), 2.0);
        pocket_of(&mut project).enabled = false;
        assert_eq!(plane_of(&project), Some(("uncleared", None)));
        assert_eq!(uncleared(&project).len(), 1);
        assert_eq!(mismatches(&project), vec![]);

        let mut project = carve_in_pocket(Some(2.0), 2.0);
        pocket_of(&mut project).enabled = false;
        assert_eq!(plane_of(&project), Some(("uncleared", None)));

        let mut project = carve_in_pocket(Some(1.0), 0.0);
        pocket_of(&mut project).enabled = false;
        assert_eq!(plane_of(&project), Some(("stock_top", None)));
        assert_eq!(mismatches(&project), vec![]);
    }

    /// Validation and `check` share `dc_project::containing_pocket`, so
    /// they agree on every layout: validation reports
    /// `start_depth_uncleared` exactly when [`start_plane`] says
    /// uncleared, and the Pocket `check` names is the containing one.
    #[test]
    fn validation_and_check_agree_on_the_containing_pocket() {
        let mut cases: Vec<(&str, Project, &str)> = vec![
            ("matching", carve_in_pocket(Some(2.0), 2.0), "cleared"),
            ("shallower", carve_in_pocket(Some(1.0), 2.0), "mismatch"),
            ("deeper", carve_in_pocket(Some(3.0), 2.0), "mismatch"),
            ("none", carve_in_pocket(None, 2.0), "uncleared"),
            ("stock top", carve_in_pocket(None, 0.0), "stock_top"),
            ("top in pocket", carve_in_pocket(Some(1.0), 0.0), "mismatch"),
        ];
        let mut acknowledged = carve_in_pocket(None, 2.0);
        carve_of(&mut acknowledged).start_plane_cleared_externally = true;
        cases.push(("acknowledged", acknowledged, "cleared_externally"));
        let mut disabled = carve_in_pocket(Some(2.0), 2.0);
        pocket_of(&mut disabled).enabled = false;
        cases.push(("disabled", disabled, "uncleared"));
        // The Pocket after the carve does not clear its plane.
        let mut after = carve_in_pocket(Some(2.0), 2.0);
        after.operations.rotate_left(1);
        cases.push(("after", after, "uncleared"));
        // The square straddling the rectangle's right edge (x 60).
        let mut crossing = carve_in_pocket(Some(2.0), 2.0);
        crossing.instances[1].transform = Transform::translate(55.0, 20.0);
        cases.push(("crossing", crossing, "uncleared"));
        // The square outside the rectangle.
        let mut outside = carve_in_pocket(Some(2.0), 2.0);
        outside.instances[1].transform = Transform::translate(70.0, 70.0);
        cases.push(("outside", outside, "uncleared"));
        // Two Pockets contain it: the last one before the carve counts.
        let mut two = carve_in_pocket(Some(1.0), 2.0);
        two.operations.insert(
            1,
            Operation::Pocket(PocketOp {
                id: OperationId::new("o3"),
                depth_mm: Some(2.0),
                ..sample_pocket()
            }),
        );
        cases.push(("two", two, "cleared"));

        for (name, project, expected) in &cases {
            let containing = dc_project::containing_pocket(project, the_carve(project))
                .map(|p| p.id.to_string());
            let (kind, pocket) = plane_of(project).expect("a V-Carve has a start plane");
            assert_eq!(kind, *expected, "{name}");
            assert_eq!(
                !uncleared(project).is_empty(),
                kind == "uncleared",
                "{name}: validation and check disagree"
            );
            if matches!(kind, "cleared" | "mismatch") {
                assert_eq!(pocket, containing, "{name}");
            } else {
                assert_eq!(containing, None, "{name}");
            }
            let named: Vec<String> = mismatches(project)
                .iter()
                .filter_map(|w| match w {
                    Warning::StartDepthMismatch { pocket, .. } => Some(pocket.to_string()),
                    _ => None,
                })
                .collect();
            let expected_named: Vec<String> = if kind == "mismatch" {
                containing.into_iter().collect()
            } else {
                Vec::new()
            };
            assert_eq!(named, expected_named, "{name}");
        }
    }
}
