// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs (#105, ADR-0005; `docs/specs/phase-2/design.md`, "Placed
//! tabs"): placing them from a tab request with the phase 2 layout rules,
//! and the project-level check that every tab is cut round.
//!
//! A placed tab is a point in the definition space of one instance, on
//! one of its outlines. Placing from a request stores the centre the
//! request lays on the tool path, taken back into definition space by the
//! inverse of the instance's transform; placing at distances along the
//! outline stores the outline's point there. Generation centres the tab
//! at the point of each tool path of that outline nearest it
//! ([`dc_geometry::nearest_point`]), so a tab placed from a request comes
//! back exactly where the request put it, even on the arc a path turns
//! round a corner, and a tab on the outline lands where the bit passes it.
//! Nothing here depends on Y-up or Y-down: nearest points and distances
//! along a line are the same under the flip.

use std::collections::BTreeMap;

use dc_geometry::{EPS_MM, Point, Polyline, approx_eq, nearest_point};
use dc_project::{Operation, OperationId, PlacedTab, ProfileOp, Project, TabId, TabShape, TabSpec};

use crate::error::{CamError, ShapeRef};
use crate::profile::{OpTabs, op_tabs};
use crate::tabs::{PathEnds, check_size, coverage_warning, overlap_warning};
use crate::toolpath::Toolpath;
use crate::warning::Warning;

/// Tabs to add to a project, and what placing them found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placement {
    /// The new tabs, with ids that follow the project's, none a copy of a
    /// tab the project already has.
    pub tabs: Vec<PlacedTab>,
    /// What placing found: [`Warning::TabsCoverMostOfContour`] where an
    /// outline got fewer tabs than asked, or tabs that cover most of it,
    /// and [`Warning::TabsOverlap`] where tabs on one path touch.
    pub warnings: Vec<Warning>,
}

/// The width, height and shape of tabs to place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSize {
    /// Tab length along the outline, mm.
    pub width_mm: f64,
    /// Tab top above the stock bottom, mm.
    pub height_mm: f64,
    /// Tab cross-section.
    pub shape: TabShape,
}

/// Places the tabs `spec` asks for on the outlines Profile `op` cuts, with
/// the phase 2 layout rules (#109): per tool path, `count` tabs evenly
/// spaced or tabs at `at_distances` along it from its start (the
/// Profile's start point when it sets one, #285); off corners where they
/// can be with `avoid_corners` ([`Warning::TabOnCorner`] for one that
/// cannot); none on a hole unless `on_holes`; and on a path too short for
/// them, the most that cover at most half of it, but at least one on a
/// part. Each tab is
/// placed at its centre on the tool path itself, taken into the
/// instance's definition space, so the Profile cuts it exactly where the
/// request did; it is crossed by every Profile that comes near where it
/// meets the outline.
///
/// Fails with [`CamError::UnknownOperation`] or
/// [`CamError::TabsNeedProfile`] when `op` is not a Profile of `project`,
/// with [`CamError::InvalidTab`] for a size that is not a positive number
/// or a distance that is not finite, and as [`crate::generate_profile`]
/// does when the tool or the targets cannot be cut.
pub fn place(project: &Project, op: &OperationId, spec: &TabSpec) -> Result<Placement, CamError> {
    let profile = profile_op(project, op)?;
    let asking = ProfileOp {
        tabs: Some(spec.clone()),
        ..profile.clone()
    };
    let cut = op_tabs(project, &asking)?;
    let mut placement = Placement::default();
    let mut next = next_number(project);
    for (j, (path, shape)) in cut.paths.iter().enumerate() {
        let Some(asked) = cut.plan.requested.get(j) else {
            continue;
        };
        let length_mm = path.length_mm();
        if let Some(Some((reduced, widths_mm))) = cut.plan.request_reductions.get(j) {
            placement.warnings.push(coverage_warning(
                op,
                j,
                Some(shape),
                (cut.diameter_mm, widths_mm),
                length_mm,
                *reduced,
            ));
        }
        if let Some(tabs) = &cut.plan.tabs
            && tabs.layout(asked, length_mm, ends(path, &cut)).1
        {
            placement
                .warnings
                .push(overlap_warning(op, j, Some(shape), length_mm));
        }
        // #285: a tab left on a corner is placed there, warned.
        if let Some(on) = cut.plan.on_corners_mm.get(j) {
            placement
                .warnings
                .extend(
                    on.iter()
                        .map(|&(distance_mm, reason)| Warning::TabOnCorner {
                            op: op.clone(),
                            shape: shape.clone(),
                            distance_mm,
                            reason,
                        }),
                );
        }
        for tab_cut in asked {
            // Centres on an open path are measured from its first vertex
            // as drawn.
            let along_mm = if path.is_closed() || !cut.reversed_open {
                tab_cut.centre_mm
            } else {
                length_mm - tab_cut.centre_mm
            };
            let Some((q, _)) = path.point_at(along_mm) else {
                continue;
            };
            let Some(centre) = definition_point(project, shape, q) else {
                continue;
            };
            let tab = PlacedTab {
                id: TabId::new(format!("tab{next}")),
                instance: shape.instance.clone(),
                profile: shape.profile,
                x_mm: centre.x,
                y_mm: centre.y,
                width_mm: tab_cut.width_mm,
                height_mm: tab_cut.top_z_mm,
                shape: tab_cut.shape,
            };
            if !has_copy(project, &placement.tabs, &tab) {
                placement.tabs.push(tab);
                next += 1;
            }
        }
    }
    Ok(placement)
}

/// Places a tab of `size` at each of `at_mm`, distances along the outline
/// from its first vertex as drawn (mm, definition space), on every
/// outline Profile `op` cuts: none on a hole unless `on_holes`. A closed
/// outline takes each distance modulo its length; an open one clamps it
/// to its ends. The tabs are cut as placed; the warnings say where they
/// would overlap or cover more than half of a tool path.
///
/// Fails as [`place`] does, and with [`CamError::InvalidTab`] for a
/// distance that is negative or not finite.
pub fn place_on_outlines(
    project: &Project,
    op: &OperationId,
    at_mm: &[f64],
    size: TabSize,
    on_holes: bool,
) -> Result<Placement, CamError> {
    let profile = profile_op(project, op)?;
    check_size(op, "width_mm", size.width_mm)?;
    check_size(op, "height_mm", size.height_mm)?;
    if let Some((i, d)) = at_mm
        .iter()
        .enumerate()
        .find(|(_, d)| !(d.is_finite() && **d >= 0.0))
    {
        return Err(CamError::InvalidTab {
            op: op.clone(),
            field: format!("at[{i}]"),
            value_mm: *d,
        });
    }
    let bare = ProfileOp {
        tabs: None,
        ..profile.clone()
    };
    let cut = op_tabs(project, &bare)?;
    let mut placement = Placement::default();
    let mut next = next_number(project);
    let mut done: Vec<&ShapeRef> = Vec::new();
    for ((_, shape), &hole) in cut.paths.iter().zip(&cut.holes) {
        if (hole && !on_holes) || done.contains(&shape) {
            continue;
        }
        done.push(shape);
        let Some(outline) = definition_profile(project, shape) else {
            continue;
        };
        let length_mm = outline.length_mm();
        for d in at_mm {
            let at_mm = if outline.is_closed() {
                d.rem_euclid(length_mm)
            } else {
                d.clamp(0.0, length_mm)
            };
            let Some((centre, _)) = outline.point_at(at_mm) else {
                continue;
            };
            let tab = PlacedTab {
                id: TabId::new(format!("tab{next}")),
                instance: shape.instance.clone(),
                profile: shape.profile,
                x_mm: centre.x,
                y_mm: centre.y,
                width_mm: size.width_mm,
                height_mm: size.height_mm,
                shape: size.shape,
            };
            if !has_copy(project, &placement.tabs, &tab) {
                placement.tabs.push(tab);
                next += 1;
            }
        }
    }
    // What the tabs will cut like, as generation lays them out.
    let mut with = project.clone();
    with.tabs.extend(placement.tabs.iter().cloned());
    let laid = op_tabs(&with, &bare)?;
    for (j, ((path, shape), layout)) in laid.paths.iter().zip(&laid.plan.layouts).enumerate() {
        let length_mm = path.length_mm();
        if layout.as_ref().is_some_and(|(_, overlapped)| *overlapped) {
            placement
                .warnings
                .push(overlap_warning(op, j, Some(shape), length_mm));
        }
        if let Some(Some((reduced, widths_mm))) = laid.plan.reductions.get(j) {
            placement.warnings.push(coverage_warning(
                op,
                j,
                Some(shape),
                (laid.diameter_mm, widths_mm),
                length_mm,
                *reduced,
            ));
        }
    }
    Ok(placement)
}

/// `project` with the tab request of every enabled Profile placed
/// ([`place`]) and cleared, as `dreamcarve` saves a project (#105,
/// ADR-0005), with what placing found. A disabled Profile keeps its
/// request, so it cuts as it would have when it is enabled again.
///
/// Fails as [`place`] does for the first request that cannot be placed.
pub fn place_requests(project: &Project) -> Result<(Project, Vec<Warning>), CamError> {
    let mut out = project.clone();
    let mut warnings = Vec::new();
    for i in 0..out.operations.len() {
        let Some(Operation::Profile(op)) = out.operations.get(i) else {
            continue;
        };
        let (Some(spec), true) = (op.tabs.clone(), op.enabled) else {
            continue;
        };
        let id = op.id.clone();
        let placement = place(&out, &id, &spec)?;
        out.tabs.extend(placement.tabs);
        warnings.extend(placement.warnings);
        if let Some(Operation::Profile(op)) = out.operations.get_mut(i) {
            op.tabs = None;
        }
    }
    Ok((out, warnings))
}

/// What [`replace`] did: the project after it, and which tabs it removed
/// and placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Replacement {
    /// The project with the tabs replaced.
    pub project: Project,
    /// The tabs removed, in project order.
    pub removed: Vec<PlacedTab>,
    /// The tabs placed, with ids after every tab the project had, and what
    /// placing found.
    pub placement: Placement,
}

/// `project` with the tabs Profile `op` crosses ([`crossed_by`], the set
/// `tabs clear <dcv> <op>` removes) replaced by what `place` places on the
/// project without them (#293). `place` sees the removed tabs gone, so a
/// tab at a removed tab's point is placed again; the tabs it places are
/// renumbered after every tab `project` had, so no removed tab's id names
/// a new tab.
///
/// Fails with [`CamError::UnknownOperation`] or
/// [`CamError::TabsNeedProfile`] when `op` is not a Profile of `project`,
/// as [`crossed_by`] does, and as `place` does.
pub fn replace<F>(project: &Project, op: &OperationId, place: F) -> Result<Replacement, CamError>
where
    F: FnOnce(&Project) -> Result<Placement, CamError>,
{
    let profile = profile_op(project, op)?;
    let crossed = crossed_by(project, profile)?;
    let mut cleared = project.clone();
    let (removed, kept): (Vec<PlacedTab>, Vec<PlacedTab>) = cleared
        .tabs
        .drain(..)
        .partition(|t| crossed.contains(&t.id));
    cleared.tabs = kept;
    let mut placement = place(&cleared)?;
    let mut next = next_number(project);
    for tab in &mut placement.tabs {
        tab.id = TabId::new(format!("tab{next}"));
        next = next.saturating_add(1);
    }
    cleared.tabs.extend(placement.tabs.iter().cloned());
    Ok(Replacement {
        project: cleared,
        removed,
        placement,
    })
}

/// The placed tabs Profile `op` crosses, in project order: each tab on an
/// outline it cuts whose nearest tool path runs within the tool radius
/// plus half the tab's width. Fails as [`crate::generate_profile`] does
/// when the tool or the targets cannot be cut.
pub fn crossed_by(project: &Project, op: &ProfileOp) -> Result<Vec<TabId>, CamError> {
    Ok(op_tabs(project, op)?.plan.crossed)
}

/// The parts of `project` an enabled Profile cuts through the stock with
/// no tab, by that Profile: each closed outline it cuts as a part (not as
/// a hole) on whose every tool path it places no tab and crosses none, so
/// its last pass frees it. A Profile whose depth or the stock's thickness
/// is unset, or that cannot be cut, is left out. In operation order, then
/// drawing order. `tabs clear` compares this before and after to say which
/// parts the tabs it removed held (#105).
#[must_use]
pub fn freed_parts(project: &Project) -> Vec<(OperationId, ShapeRef)> {
    let mut freed = Vec::new();
    for op in &project.operations {
        let Operation::Profile(profile) = op else {
            continue;
        };
        let through = match (project.stock.thickness_mm, profile.depth_mm) {
            (Some(thickness_mm), Some(depth_mm)) => depth_mm >= thickness_mm - EPS_MM,
            _ => false,
        };
        if !profile.enabled || !through {
            continue;
        }
        let Ok(cut) = op_tabs(project, profile) else {
            continue;
        };
        let mut parts: Vec<(&ShapeRef, bool)> = Vec::new();
        for (((path, shape), &hole), layout) in
            cut.paths.iter().zip(&cut.holes).zip(&cut.plan.layouts)
        {
            if hole || !path.is_closed() {
                continue;
            }
            let tabbed = layout.as_ref().is_some_and(|(l, _)| !l.is_untabbed());
            match parts.iter_mut().find(|(s, _)| *s == shape) {
                Some((_, any)) => *any |= tabbed,
                None => parts.push((shape, tabbed)),
            }
        }
        freed.extend(
            parts
                .into_iter()
                .filter(|(_, tabbed)| !tabbed)
                .map(|(shape, _)| (profile.id.clone(), shape.clone())),
        );
    }
    freed
}

/// The project-level tab warnings, in project order:
/// [`Warning::TabNotCrossed`] for every placed tab of `project` that no
/// Profile of `generated` crosses ([`crossed_by`]), and
/// [`Warning::TabOffOutline`] for one that is crossed but whose point is
/// more than [`off_outline_limit_mm`] from its outline
/// ([`off_outline_mm`], #300). `generated` holds every enabled operation
/// whose toolpath was generated, as for [`crate::order::check`].
#[must_use]
pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning> {
    // The widest bit that crosses each tab, mm.
    let mut crossed: BTreeMap<TabId, f64> = BTreeMap::new();
    for (op, _) in generated {
        let Operation::Profile(profile) = op else {
            continue;
        };
        let Some(diameter_mm) = project
            .tools
            .iter()
            .find(|t| t.id == profile.tool)
            .map(|t| t.diameter_mm)
        else {
            continue;
        };
        for id in crossed_by(project, profile).unwrap_or_default() {
            let widest = crossed.entry(id).or_insert(diameter_mm);
            *widest = widest.max(diameter_mm);
        }
    }
    let mut warnings = Vec::new();
    for tab in &project.tabs {
        let Some(instance) = project.instances.iter().find(|i| i.id == tab.instance) else {
            continue;
        };
        let shape = ShapeRef {
            instance: instance.id.clone(),
            instance_name: instance.name.clone(),
            profile: tab.profile,
        };
        let Some(&diameter_mm) = crossed.get(&tab.id) else {
            warnings.push(Warning::TabNotCrossed {
                tab: tab.id.clone(),
                shape,
            });
            continue;
        };
        let limit_mm = off_outline_limit_mm(tab, diameter_mm);
        if let Some(off_mm) = off_outline_mm(project, tab)
            && off_mm > limit_mm + EPS_MM
        {
            warnings.push(Warning::TabOffOutline {
                tab: tab.id.clone(),
                shape,
                off_mm,
                limit_mm,
            });
        }
    }
    warnings
}

/// How far a tab's point may be from its outline before
/// [`Warning::TabOffOutline`], mm: its width plus `diameter_mm`, the
/// widest bit that cuts round it (#300). A tab placed on the outline is on
/// it, and one placed from a request or `--count` a bit radius off, so
/// neither warns; a point farther than a whole tab and bit away is not one
/// a placing command wrote, and is likely a hand edit or left behind
/// after its outline changed.
#[must_use]
pub fn off_outline_limit_mm(tab: &PlacedTab, diameter_mm: f64) -> f64 {
    tab.width_mm + diameter_mm
}

/// How far `tab`'s point is from its outline, mm in project space (as it
/// is cut, after its instance's transform): the distance from
/// [`tab_point`] to the outline's point nearest it, where the tab is cut.
/// `None` when its instance, definition or profile is not in the project.
#[must_use]
pub fn off_outline_mm(project: &Project, tab: &PlacedTab) -> Option<f64> {
    Some(tab_point(project, tab)?.distance_to(outline_foot(project, tab)?))
}

/// The placed tabs Profile `op` crosses on a tool path round a hole, with
/// the hole, in project order: those generation warns
/// [`Warning::TabOnHole`] for, and those placed there for this cut, which
/// it does not (#300). `tabs place` warns for the ones it places. Fails as
/// [`crossed_by`] does.
pub fn on_holes(project: &Project, op: &ProfileOp) -> Result<Vec<(TabId, ShapeRef)>, CamError> {
    Ok(op_tabs(project, op)?
        .plan
        .on_holes
        .into_iter()
        .map(|(tab, shape, _)| (tab, shape))
        .collect())
}

/// Where `tab`'s centre is in project space: its point placed by its
/// instance's transform. `None` when its instance is not in the project.
#[must_use]
pub fn tab_point(project: &Project, tab: &PlacedTab) -> Option<Point> {
    let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
    Some(instance.transform.apply(Point::new(tab.x_mm, tab.y_mm)))
}

/// Where `tab` meets its outline in project space: the outline's point
/// nearest the tab's. A tab placed on the outline is its own foot; one
/// placed from a request on an Inside or Outside path is a tool radius
/// from it. `None` when its instance, definition or profile is not in the
/// project, or cannot be placed.
pub(crate) fn outline_foot(project: &Project, tab: &PlacedTab) -> Option<Point> {
    let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
    let profile = project
        .definitions
        .get(&instance.definition)?
        .profiles
        .get(tab.profile)?;
    let outline = instance.transform.apply_polyline(profile).ok()?;
    Some(nearest_point(&outline, tab_point(project, tab)?)?.point)
}

/// How far along its profile from the first vertex as drawn `tab` is, mm
/// in definition space: the distance of the profile's point nearest the
/// tab, and how far the tab is from it. `None` when its instance,
/// definition or profile is not in the project.
#[must_use]
pub fn along_outline_mm(project: &Project, tab: &PlacedTab) -> Option<(f64, f64)> {
    let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
    let profile = project
        .definitions
        .get(&instance.definition)?
        .profiles
        .get(tab.profile)?;
    let near = nearest_point(profile, Point::new(tab.x_mm, tab.y_mm))?;
    Some((near.along_mm, near.gap_mm))
}

/// The Profile `op` of `project`.
fn profile_op<'p>(project: &'p Project, op: &OperationId) -> Result<&'p ProfileOp, CamError> {
    match project.operations.iter().find(|o| o.id() == op) {
        Some(Operation::Profile(profile)) => Ok(profile),
        Some(other) => Err(CamError::TabsNeedProfile {
            op: op.clone(),
            kind: other.kind(),
        }),
        None => Err(CamError::UnknownOperation { op: op.clone() }),
    }
}

/// How `path` of `op_tabs` runs, for laying tabs on it.
fn ends(path: &Polyline, op_tabs: &OpTabs) -> PathEnds {
    if path.is_closed() {
        PathEnds::Closed
    } else {
        PathEnds::Open {
            reversed: op_tabs.reversed_open,
        }
    }
}

/// The definition-space profile `shape` is cut from.
fn definition_profile<'p>(project: &'p Project, shape: &ShapeRef) -> Option<&'p Polyline> {
    let instance = project.instances.iter().find(|i| i.id == shape.instance)?;
    project
        .definitions
        .get(&instance.definition)?
        .profiles
        .get(shape.profile)
}

/// `q`, a point in project space, in the definition space of `shape`'s
/// instance. `None` when the instance is not in the project or its
/// transform is singular.
fn definition_point(project: &Project, shape: &ShapeRef, q: Point) -> Option<Point> {
    let instance = project.instances.iter().find(|i| i.id == shape.instance)?;
    Some(instance.transform.inverse()?.apply(q))
}

/// The number after `tab` in the next free tab id: one more than the
/// largest `tab<n>` in `project`.
fn next_number(project: &Project) -> u64 {
    project
        .tabs
        .iter()
        .filter_map(|t| t.id.as_str().strip_prefix("tab")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1))
}

/// Whether `project` or `placed` already has a tab like `tab`: on the
/// same outline at the same point, with the same size and shape.
fn has_copy(project: &Project, placed: &[PlacedTab], tab: &PlacedTab) -> bool {
    project.tabs.iter().chain(placed).any(|t| {
        t.instance == tab.instance
            && t.profile == tab.profile
            && t.shape == tab.shape
            && approx_eq(t.x_mm, tab.x_mm, EPS_MM)
            && approx_eq(t.y_mm, tab.y_mm, EPS_MM)
            && approx_eq(t.width_mm, tab.width_mm, EPS_MM)
            && approx_eq(t.height_mm, tab.height_mm, EPS_MM)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{Point3, Transform, assert_approx_eq};
    use dc_project::test_support::{profile_mut, sample_pocket, sample_project, sample_tab};
    use dc_project::{
        CutDirection, DefinitionId, Entry, FeatureDefinition, InstanceId, Lead, Side, StartAt,
        TabPlacement,
    };

    const TOL: f64 = 1e-9;
    /// The tab top of the tabs these tests place, mm above the stock
    /// bottom: below the last pass only (the passes are at 4, 2 and 0).
    const TAB_TOP_MM: f64 = 1.0;

    /// [`sample_tab`] with its top at [`TAB_TOP_MM`].
    fn tab(id: &str, x_mm: f64, y_mm: f64) -> PlacedTab {
        PlacedTab {
            height_mm: TAB_TOP_MM,
            ..sample_tab(id, x_mm, y_mm)
        }
    }

    /// The sample's Profile, generated and checked for safety.
    fn generate(project: &Project) -> (Toolpath, Vec<Warning>) {
        let op = project
            .operations
            .iter()
            .find(|o| matches!(o, Operation::Profile(_)))
            .expect("a Profile");
        generate_of(project, op)
    }

    fn generate_of(project: &Project, op: &Operation) -> (Toolpath, Vec<Warning>) {
        let Operation::Profile(profile) = op else {
            panic!("a Profile, not {op:?}");
        };
        let (toolpath, warnings) = generate_profile(project, profile).unwrap();
        assert_safe(&toolpath, project, op);
        (toolpath, warnings)
    }

    /// The sample with no tab request: an untabbed Profile.
    fn untabbed() -> Project {
        let mut p = sample_project();
        profile_mut(&mut p).tabs = None;
        p
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(Warning::code).collect()
    }

    /// `a` and `b` are the same motion: the same moves of the same kinds,
    /// every coordinate and feed within [`TOL`].
    fn assert_same_motion(a: &Toolpath, b: &Toolpath, case: &str) {
        assert_eq!(a.moves.len(), b.moves.len(), "{case}: move count");
        let near = |p: Point3, q: Point3| {
            (p.x - q.x).abs() <= TOL && (p.y - q.y).abs() <= TOL && (p.z - q.z).abs() <= TOL
        };
        for (i, (m, n)) in a.moves.iter().zip(&b.moves).enumerate() {
            let kinds_match = match (m.kind, n.kind) {
                (MoveKind::ArcCw { centre: c }, MoveKind::ArcCw { centre: d })
                | (MoveKind::ArcCcw { centre: c }, MoveKind::ArcCcw { centre: d }) => {
                    c.distance_to(d) <= TOL
                }
                (k, l) => std::mem::discriminant(&k) == std::mem::discriminant(&l),
            };
            let feeds_match = match (m.feed_mm_min, n.feed_mm_min) {
                (Some(f), Some(g)) => (f - g).abs() <= TOL,
                (None, None) => true,
                _ => false,
            };
            assert!(
                kinds_match && feeds_match && near(m.to, n.to),
                "{case}: move {i} differs: {m:?} vs {n:?}"
            );
        }
    }

    /// Every cutting move with the point it starts from.
    fn cuts(tp: &Toolpath) -> Vec<(Point3, Move)> {
        tp.moves
            .windows(2)
            .filter_map(|w| match w {
                [from, mv] if !mv.is_rapid() => Some((from.to, *mv)),
                _ => None,
            })
            .collect()
    }

    /// The runs of consecutive cutting moves held at `z_mm` while moving
    /// in XY: one per tab top crossed.
    fn held_runs(tp: &Toolpath, z_mm: f64) -> Vec<Vec<(Point3, Move)>> {
        let mut runs: Vec<Vec<(Point3, Move)>> = Vec::new();
        let mut open = false;
        for (from, mv) in cuts(tp) {
            let held = (from.z - z_mm).abs() <= TOL
                && (mv.to.z - z_mm).abs() <= TOL
                && from.xy().distance_to(mv.to.xy()) > EPS_MM;
            match (held, open) {
                (true, true) => {
                    if let Some(run) = runs.last_mut() {
                        run.push((from, mv));
                    }
                }
                (true, false) => runs.push(vec![(from, mv)]),
                _ => {}
            }
            open = held;
        }
        runs
    }

    /// A change to the sample's Profile.
    type Edit = Box<dyn Fn(&mut ProfileOp)>;

    /// #105, ADR-0005: placing a request writes tabs that cut exactly
    /// what the request cut: Outside both ways, an odd count whose middle
    /// tab sits on the arc round a corner, Inside at distances, On line,
    /// and with a ramp and a lead.
    #[test]
    fn placed_requests_cut_what_the_requests_cut() {
        let smooth = |placement: TabPlacement| TabSpec {
            placement,
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Smooth,
            on_holes: false,
            avoid_corners: false,
        };
        let cases: Vec<(&str, Edit)> = vec![
            ("outside climb", Box::new(|_| {})),
            (
                "outside conventional, three smooth",
                Box::new(move |op| {
                    op.direction = CutDirection::Conventional;
                    op.tabs = Some(smooth(TabPlacement::Count { count: 3 }));
                }),
            ),
            (
                "inside at distances",
                Box::new(move |op| {
                    op.side = Side::Inside;
                    op.tabs = Some(smooth(TabPlacement::AtDistances {
                        distances_mm: vec![5.0, 40.0, 77.7, 300.0],
                    }));
                }),
            ),
            (
                "on line, five",
                Box::new(|op| {
                    op.side = Side::OnLine;
                    if let Some(tabs) = &mut op.tabs {
                        tabs.placement = TabPlacement::Count { count: 5 };
                    }
                }),
            ),
            (
                "ramp and lead",
                Box::new(move |op| {
                    op.entry = Entry::Ramp { angle_deg: 15.0 };
                    op.lead = Some(Lead {
                        radius_mm: 3.0,
                        sweep_deg: 90.0,
                        lead_out: true,
                        overcut_mm: 1.0,
                    });
                    op.tabs = Some(smooth(TabPlacement::Count { count: 4 }));
                }),
            ),
        ];
        for (case, edit) in cases {
            let mut p = sample_project();
            edit(profile_mut(&mut p));
            let (asked, _) = generate(&p);
            let (placed, _) = place_requests(&p).unwrap();
            assert_eq!(profile_mut(&mut placed.clone()).tabs, None, "{case}");
            assert!(!placed.tabs.is_empty(), "{case}: tabs placed");
            let (cut, _) = generate(&placed);
            assert_same_motion(&asked, &cut, case);
        }
    }

    /// #105, the issue's third test: a placed tab that no enabled Profile
    /// crosses warns `tab_not_crossed`: on an outline no Profile cuts, or
    /// cut only by a disabled Profile or by a Pocket. A tab the Profile
    /// cuts round does not warn.
    #[test]
    fn tab_no_enabled_operation_crosses_warns() {
        let mut p = untabbed();
        // A second rectangle at (70, 60), which the Profile does not cut.
        let mut other = p.instances[0].clone();
        other.id = InstanceId::new("i2");
        other.name = "Rectangle 2".into();
        other.transform = Transform::translate(70.0, 60.0);
        p.instances.push(other);
        // The middle of each rectangle's bottom edge.
        p.tabs = vec![
            tab("tab1", 25.0, 0.0),
            PlacedTab {
                instance: InstanceId::new("i2"),
                ..tab("tab2", 25.0, 0.0)
            },
        ];
        let op = p.operations[0].clone();
        let (tp, _) = generate(&p);
        let warnings = check(&p, &[(&op, &tp)]);
        assert_eq!(
            warnings,
            vec![Warning::TabNotCrossed {
                tab: TabId::new("tab2"),
                shape: ShapeRef {
                    instance: InstanceId::new("i2"),
                    instance_name: "Rectangle 2".into(),
                    profile: 0,
                },
            }]
        );
        assert!(warnings[0].op().is_none());
        // Disabled, it is not generated, so nothing crosses either tab.
        profile_mut(&mut p).enabled = false;
        assert_eq!(codes(&check(&p, &[])), vec![WarningCode::TabNotCrossed; 2]);
        // A Pocket of the outline does not cut round tabs.
        let pocket = Operation::Pocket(sample_pocket());
        let (pocket_tp, _) = crate::generate_pocket(&p, &sample_pocket()).unwrap();
        assert_eq!(
            codes(&check(&p, &[(&pocket, &pocket_tp)])),
            vec![WarningCode::TabNotCrossed; 2]
        );
    }

    /// #105: a tab is a point in its instance's definition space, so it
    /// moves with the instance. The sample turned a quarter turn and moved
    /// to (80, 20) puts the middle of its bottom edge at (80, 45), with
    /// the outward side towards +X: the last pass holds the tab top along
    /// X 83.175 for the tab width plus the bit, centred on Y 45.
    #[test]
    fn tab_follows_its_instance_when_it_moves() {
        let mut p = untabbed();
        p.instances[0].transform =
            Transform::rotate(std::f64::consts::FRAC_PI_2).then(Transform::translate(80.0, 20.0));
        p.tabs = vec![tab("tab1", 25.0, 0.0)];
        let (tp, warnings) = generate(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
        let runs = held_runs(&tp, TAB_TOP_MM);
        assert_eq!(runs.len(), 1, "{runs:?}");
        let ys: Vec<f64> = runs[0]
            .iter()
            .flat_map(|(from, mv)| [from.y, mv.to.y])
            .collect();
        for (from, mv) in &runs[0] {
            assert_approx_eq!(from.x, 83.175, 1e-6);
            assert_approx_eq!(mv.to.x, 83.175, 1e-6);
        }
        let lo = ys.iter().copied().fold(f64::MAX, f64::min);
        let hi = ys.iter().copied().fold(f64::MIN, f64::max);
        assert_approx_eq!((lo + hi) / 2.0, 45.0, 1e-6);
        assert_approx_eq!(hi - lo, 6.0 + 6.35, 1e-6);
    }

    /// #105, ADR-0005: a tab is material on the part, so every Profile
    /// that cuts its outline rises over it, here an Outside and an Inside
    /// cut of the rectangle.
    #[test]
    fn two_profiles_on_one_outline_both_rise_over_its_tabs() {
        let mut p = untabbed();
        p.tabs = vec![tab("tab1", 25.0, 0.0)];
        let mut inside = profile_mut(&mut p).clone();
        inside.id = OperationId::new("o2");
        inside.side = Side::Inside;
        p.operations.push(Operation::Profile(inside));
        for op in p.operations.clone() {
            let (tp, _) = generate_of(&p, &op);
            assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 1, "{}", op.id());
            assert_eq!(
                crossed_by(
                    &p,
                    match &op {
                        Operation::Profile(profile) => profile,
                        _ => unreachable!(),
                    }
                )
                .unwrap(),
                vec![TabId::new("tab1")]
            );
        }
    }

    /// #105: each tab rises to its own top. With passes at Z 4, 2 and 0, a
    /// 3 mm tab on the top edge is crossed by the passes at 2 and 0, and a
    /// 1 mm tab on the bottom edge only by the last.
    #[test]
    fn tabs_of_different_heights_rise_each_to_its_own_top() {
        let mut p = untabbed();
        p.tabs = vec![
            tab("low", 25.0, 0.0),
            PlacedTab {
                height_mm: 3.0,
                ..sample_tab("high", 25.0, 30.0)
            },
        ];
        let (tp, warnings) = generate(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(held_runs(&tp, 3.0).len(), 2);
        assert_eq!(held_runs(&tp, 1.0).len(), 1);
        for run in held_runs(&tp, 1.0) {
            assert!(run.iter().all(|(from, _)| from.y < 10.0), "{run:?}");
        }
    }

    /// The sample with its rectangle replaced by `profiles`, all cut by
    /// the one Profile.
    fn with_profiles(profiles: Vec<Polyline>) -> Project {
        let mut p = untabbed();
        p.definitions.insert(
            DefinitionId::new("d1"),
            FeatureDefinition {
                id: DefinitionId::new("d1"),
                name: "Frame".into(),
                profiles,
            },
        );
        p
    }

    fn four_tabs(on_holes: bool) -> TabSpec {
        TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Rect,
            on_holes,
            avoid_corners: false,
        }
    }

    /// #105 with #109's rules: `place` puts no tab on a hole unless
    /// `on_holes`, and on an outline too short for all its tabs, the most
    /// that cover at most half of it, with `tabs_cover_most_of_contour`.
    #[test]
    fn place_keeps_the_rules_for_holes_and_short_outlines() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let p = with_profiles(vec![square(28.0), square(12.0)]);
        let op = OperationId::new("o1");
        let off = place(&p, &op, &four_tabs(false)).unwrap();
        assert_eq!(off.tabs.len(), 4, "{off:?}");
        assert!(off.tabs.iter().all(|t| t.profile == 0), "{off:?}");
        assert!(off.warnings.is_empty(), "{:?}", off.warnings);
        let on = place(&p, &op, &four_tabs(true)).unwrap();
        let on_hole = on.tabs.iter().filter(|t| t.profile == 1).count();
        // The hole's 22.6 mm path takes one 12.35 mm span at most... none
        // in half of it: a hole may lose them all.
        assert_eq!(on_hole, 0, "{on:?}");
        assert!(
            on.warnings.iter().any(|w| matches!(
                w,
                Warning::TabsCoverMostOfContour {
                    tab_count: 4,
                    cut_count: 0,
                    ..
                }
            )),
            "{:?}",
            on.warnings
        );
        // A 10 mm part keeps the two that fit in half its 59.95 mm path.
        let small = with_profiles(vec![square(10.0)]);
        let placed = place(&small, &op, &four_tabs(false)).unwrap();
        assert_eq!(placed.tabs.len(), 2, "{placed:?}");
        assert_eq!(
            codes(&placed.warnings),
            vec![WarningCode::TabsCoverMostOfContour]
        );
    }

    /// #105: a tab on a hole of the part is cut, since it was placed, and
    /// warns `tab_on_hole`.
    #[test]
    fn tab_on_a_hole_is_cut_and_warns() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let mut p = with_profiles(vec![square(28.0), square(16.0)]);
        p.tabs = vec![PlacedTab {
            profile: 1,
            width_mm: 2.0,
            ..tab("tab1", 25.0, 7.0)
        }];
        let (tp, warnings) = generate(&p);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::TabOnHole],
            "{warnings:?}"
        );
        assert!(matches!(&warnings[0], Warning::TabOnHole { tab, .. } if tab.as_str() == "tab1"));
        assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 1);
    }

    /// #300: tabs placed on a hole for the Profile that cuts it, by a
    /// request or `--count` with `on_holes` (a version 8 file's migrated
    /// request among them), sit on that Profile's tool path round the
    /// hole. They hold the slug because the user asked, so generation does
    /// not warn `tab_on_hole` for them on every export; `on_holes` lists
    /// them, for `tabs place` to warn once. The same tab moved onto the
    /// hole's outline, where nothing places a tab for this cut unasked,
    /// still warns. A 28 mm hole in a 36 mm frame: the frame takes four
    /// tabs, and the hole's 21.65 mm square path the three that cover at
    /// most half of it.
    #[test]
    fn hole_tabs_placed_for_the_cut_do_not_warn() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let mut p = with_profiles(vec![square(36.0), square(28.0)]);
        let op = OperationId::new("o1");
        p.tabs = place(&p, &op, &four_tabs(true)).unwrap().tabs;
        let on_hole: Vec<TabId> = p
            .tabs
            .iter()
            .filter(|t| t.profile == 1)
            .map(|t| t.id.clone())
            .collect();
        assert_eq!(on_hole.len(), 3, "{:?}", p.tabs);
        let (tp, warnings) = generate(&p);
        assert!(
            !codes(&warnings).contains(&WarningCode::TabOnHole),
            "{warnings:?}"
        );
        assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 7);
        let profile = profile_mut(&mut p).clone();
        let listed: Vec<TabId> = on_holes(&p, &profile)
            .unwrap()
            .into_iter()
            .map(|(tab, _)| tab)
            .collect();
        assert_eq!(listed, on_hole);

        let moved = p
            .tabs
            .iter()
            .position(|t| t.profile == 1)
            .expect("a hole tab");
        let foot = outline_foot(&p, &p.tabs[moved]).unwrap();
        let foot = p.instances[0].transform.inverse().unwrap().apply(foot);
        p.tabs[moved].x_mm = foot.x;
        p.tabs[moved].y_mm = foot.y;
        let (_, warnings) = generate(&p);
        let warned: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code() == WarningCode::TabOnHole)
            .collect();
        assert_eq!(warned.len(), 1, "{warnings:?}");
        assert!(
            matches!(warned[0], Warning::TabOnHole { tab, .. } if *tab == p.tabs[moved].id),
            "{warned:?}"
        );
    }

    /// #300, the issue's first test: a tab whose point is far from its
    /// outline is cut at the outline's nearest point, and warns
    /// `tab_off_outline` with the distance. Far is more than the tab's
    /// width plus the widest bit that cuts round it: a tab placed from a
    /// request is a bit radius off, and one on the outline none. 20 mm
    /// below the sample's bottom edge is past 6 + 6.35 mm; the sample's own
    /// request, placed, is not.
    #[test]
    fn tab_far_from_its_outline_warns() {
        let mut p = untabbed();
        p.tabs = vec![tab("tab1", 25.0, -20.0)];
        let op = p.operations[0].clone();
        let (tp, _) = generate(&p);
        assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 1);
        assert_approx_eq!(off_outline_mm(&p, &p.tabs[0]).unwrap(), 20.0, TOL);
        let warnings = check(&p, &[(&op, &tp)]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let Warning::TabOffOutline {
            tab,
            off_mm,
            limit_mm,
            ..
        } = &warnings[0]
        else {
            panic!("tab_off_outline, not {warnings:?}");
        };
        assert_eq!(tab.as_str(), "tab1");
        assert_approx_eq!(*off_mm, 20.0, TOL);
        assert_approx_eq!(*limit_mm, 6.0 + 6.35, TOL);
        assert!(warnings[0].op().is_none());
        assert!(
            warnings[0]
                .to_string()
                .contains("is 20 mm from that outline"),
            "{}",
            warnings[0]
        );

        let (placed, _) = place_requests(&sample_project()).unwrap();
        assert!(!placed.tabs.is_empty());
        let op = placed.operations[0].clone();
        let (tp, _) = generate(&placed);
        for t in &placed.tabs {
            assert_approx_eq!(off_outline_mm(&placed, t).unwrap(), 6.35 / 2.0, 1e-6);
        }
        let warnings = check(&placed, &[(&op, &tp)]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// #105: one tab placed with `--at --on-holes` on a hole whose tool
    /// path is shorter than the tab's span covers all of it. It is cut as
    /// placed and warns `tabs_cover_most_of_contour` as a placed tab, not
    /// with the words for the one tab a part keeps; a single tab overlaps
    /// nothing, so there is no `tabs_overlap`.
    #[test]
    fn one_placed_tab_on_a_small_hole_warns_as_placed() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let mut p = with_profiles(vec![square(28.0), square(8.0)]);
        let size = TabSize {
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Rect,
        };
        let placed = place_on_outlines(&p, &OperationId::new("o1"), &[2.0], size, true).unwrap();
        let on_hole: Vec<&PlacedTab> = placed.tabs.iter().filter(|t| t.profile == 1).collect();
        assert_eq!(on_hole.len(), 1, "{placed:?}");
        p.tabs = placed
            .tabs
            .iter()
            .filter(|t| t.profile == 1)
            .cloned()
            .collect();
        let (_, generated) = generate(&p);
        for warnings in [&placed.warnings, &generated] {
            let found = codes(warnings);
            assert!(!found.contains(&WarningCode::TabsOverlap), "{warnings:?}");
            let cover: Vec<String> = warnings
                .iter()
                .filter(|w| w.code() == WarningCode::TabsCoverMostOfContour)
                .map(ToString::to_string)
                .collect();
            assert_eq!(cover.len(), 1, "{warnings:?}");
            assert!(cover[0].contains("It is cut, as placed"), "{}", cover[0]);
            assert!(!cover[0].contains("a part keeps"), "{}", cover[0]);
        }
    }

    /// #105 review: a Profile crosses a tab only when its cut goes below
    /// the tab's top. A 1 mm On line score pass of the rectangle stops at
    /// Z 5, above a 3 mm tab, so it does not cross it: it warns nothing
    /// about the tab's range, and alone it leaves the tab `tab_not_crossed`.
    /// The full-depth cut-out crosses it.
    #[test]
    fn a_profile_above_a_tab_does_not_cross_it() {
        let mut p = untabbed();
        p.tabs = vec![PlacedTab {
            height_mm: 3.0,
            ..sample_tab("tab1", 25.0, 0.0)
        }];
        let mut score = profile_mut(&mut p).clone();
        score.id = OperationId::new("o2");
        score.side = Side::OnLine;
        score.depth_mm = Some(1.0);
        let full = profile_mut(&mut p).clone();
        p.operations.push(Operation::Profile(score.clone()));
        assert!(crossed_by(&p, &score).unwrap().is_empty());
        assert_eq!(crossed_by(&p, &full).unwrap(), vec![TabId::new("tab1")]);
        let score_op = Operation::Profile(score);
        let (tp, warnings) = generate_of(&p, &score_op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            codes(&check(&p, &[(&score_op, &tp)])),
            vec![WarningCode::TabNotCrossed]
        );
    }

    /// #105: placing the same tabs again adds none, and new tabs take the
    /// next free ids.
    #[test]
    fn place_does_not_place_a_tab_twice() {
        let mut p = untabbed();
        let op = OperationId::new("o1");
        let first = place(&p, &op, &four_tabs(false)).unwrap();
        let ids: Vec<&str> = first.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["tab1", "tab2", "tab3", "tab4"]);
        p.tabs = first.tabs;
        assert!(place(&p, &op, &four_tabs(false)).unwrap().tabs.is_empty());
        let more = TabSpec {
            placement: TabPlacement::Count { count: 2 },
            ..four_tabs(false)
        };
        let second = place(&p, &op, &more).unwrap();
        let ids: Vec<&str> = second.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["tab5", "tab6"]);
    }

    /// #293: `replace` removes the tabs the Profile crosses, the set
    /// `tabs clear <dcv> <op>` removes, and keeps a tab on an outline it
    /// does not cut. The new tabs are placed as if the removed ones were
    /// gone, so one at a removed tab's point is placed again, and take ids
    /// after every tab the project had.
    #[test]
    fn replace_swaps_only_the_tabs_the_profile_crosses() {
        let mut p = untabbed();
        let mut other = p.instances[0].clone();
        other.id = InstanceId::new("i2");
        other.name = "Rectangle 2".into();
        other.transform = Transform::translate(70.0, 60.0);
        p.instances.push(other);
        let op = OperationId::new("o1");
        let first = place(&p, &op, &four_tabs(false)).unwrap();
        p.tabs = first.tabs.clone();
        p.tabs.push(PlacedTab {
            instance: InstanceId::new("i2"),
            ..tab("tab5", 25.0, 0.0)
        });
        let replaced = replace(&p, &op, |cleared| place(cleared, &op, &four_tabs(false))).unwrap();
        let ids =
            |tabs: &[PlacedTab]| -> Vec<String> { tabs.iter().map(|t| t.id.to_string()).collect() };
        assert_eq!(ids(&replaced.removed), ["tab1", "tab2", "tab3", "tab4"]);
        assert_eq!(
            ids(&replaced.placement.tabs),
            ["tab6", "tab7", "tab8", "tab9"]
        );
        assert_eq!(
            ids(&replaced.project.tabs),
            ["tab5", "tab6", "tab7", "tab8", "tab9"]
        );
        for (old, new) in first.tabs.iter().zip(&replaced.placement.tabs) {
            assert_approx_eq!(old.x_mm, new.x_mm, TOL);
            assert_approx_eq!(old.y_mm, new.y_mm, TOL);
        }
        let (cut, _) = generate(&replaced.project);
        assert_eq!(held_runs(&cut, TAB_TOP_MM).len(), 4);
    }

    /// #105: tabs are placed for a Profile; anything else is refused.
    #[test]
    fn place_needs_a_profile_of_the_project() {
        let mut p = untabbed();
        p.operations.push(Operation::Pocket(sample_pocket()));
        let err = place(&p, &OperationId::new("o9"), &four_tabs(false)).unwrap_err();
        assert!(matches!(err, CamError::UnknownOperation { .. }), "{err:?}");
        let err = place(&p, &OperationId::new("o2"), &four_tabs(false)).unwrap_err();
        assert!(
            matches!(err, CamError::TabsNeedProfile { kind: "pocket", .. }),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "operation `o2` is a pocket operation; only a Profile cuts tabs"
        );
    }

    /// #105: `--at` places a tab at each distance along the outline from
    /// its first vertex as drawn: the rectangle's bottom edge runs 0 to
    /// 50 mm, its right edge 50 to 80 mm.
    #[test]
    fn place_on_outlines_puts_a_tab_at_each_distance() {
        let p = untabbed();
        let size = TabSize {
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Smooth,
        };
        let placed = place_on_outlines(
            &p,
            &OperationId::new("o1"),
            &[25.0, 65.0, 185.0],
            size,
            false,
        )
        .unwrap();
        // 185 mm wraps round the 160 mm outline to 25 mm, a tab already
        // placed, so it is not placed twice.
        let points: Vec<(f64, f64)> = placed.tabs.iter().map(|t| (t.x_mm, t.y_mm)).collect();
        assert_eq!(points.len(), 2);
        for ((x, y), (ex, ey)) in points.iter().zip([(25.0, 0.0), (50.0, 15.0)]) {
            assert_approx_eq!(*x, ex, 1e-9);
            assert_approx_eq!(*y, ey, 1e-9);
        }
        assert!(placed.tabs.iter().all(|t| t.shape == TabShape::Smooth));
        let err = place_on_outlines(&p, &OperationId::new("o1"), &[-1.0], size, false).unwrap_err();
        assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
    }

    /// #285 (#296's note): `tabs.avoid_corners` and `start` act when a
    /// request is placed as when it is cut. Placing a request that keeps
    /// off corners, counted from the start it sets, writes tabs that cut
    /// exactly what the request cuts, none of them on an arc round a
    /// corner (without `avoid_corners` some are). A tab left on a corner
    /// warns `tab_on_corner` when placed, as when cut, and is placed.
    #[test]
    fn placing_keeps_tabs_off_corners_and_counts_from_the_start() {
        let request = |count: u8, avoid_corners: bool| TabSpec {
            placement: TabPlacement::Count { count },
            avoid_corners,
            ..four_tabs(false)
        };
        let on_arc = |tp: &Toolpath| {
            held_runs(tp, TAB_TOP_MM)
                .iter()
                .flatten()
                .any(|(_, mv)| !matches!(mv.kind, MoveKind::Linear))
        };
        let cases = [
            (
                4,
                StartAt::Near {
                    x_mm: 0.0,
                    y_mm: 0.0,
                },
            ),
            (5, StartAt::Auto),
            (7, StartAt::FirstVertex),
        ];
        let mut any_on_arc = false;
        for (count, start) in cases {
            let case = format!("{count} from {start:?}");
            let mut p = sample_project();
            let op = profile_mut(&mut p);
            op.start = start;
            op.tabs = Some(request(count, false));
            any_on_arc |= on_arc(&generate(&p).0);
            profile_mut(&mut p).tabs = Some(request(count, true));
            let (asked, warnings) = generate(&p);
            assert!(
                !codes(&warnings).contains(&WarningCode::TabOnCorner),
                "{case}: {warnings:?}"
            );
            assert!(!on_arc(&asked), "{case}");
            let (placed, warnings) = place_requests(&p).unwrap();
            assert!(
                !codes(&warnings).contains(&WarningCode::TabOnCorner),
                "{case}: {warnings:?}"
            );
            assert_eq!(placed.tabs.len(), usize::from(count), "{case}");
            let (cut, _) = generate(&placed);
            assert_same_motion(&asked, &cut, &case);
        }
        assert!(any_on_arc, "without avoid_corners some tab is on a corner");

        // An 8 mm square is a corner everywhere: its tab stays, warned.
        let small = with_profiles(vec![Polyline::rect(Point::new(21.0, 11.0), 8.0, 8.0)]);
        let spec = TabSpec {
            width_mm: 2.0,
            ..request(1, true)
        };
        let placed = place(&small, &OperationId::new("o1"), &spec).unwrap();
        assert_eq!(placed.tabs.len(), 1, "{placed:?}");
        assert_eq!(
            codes(&placed.warnings),
            vec![WarningCode::TabOnCorner],
            "{:?}",
            placed.warnings
        );
    }
}
