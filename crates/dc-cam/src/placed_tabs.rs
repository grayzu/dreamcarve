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
use std::ops::Range;

use dc_geometry::{EPS_MM, Point, Polyline, approx_eq, nearest_point};
use dc_project::{
    InstanceId, Operation, OperationId, PlacedTab, ProfileOp, Project, TabId, TabShape, TabSpec,
};

use crate::error::{CamError, ShapeRef, at, at_mut};
use crate::profile::{OpTabs, op_tabs};
use crate::tabs::{Fit, Least, PathEnds, Request, check_size, coverage_warning, overlap_warning};
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
/// cannot); none on a hole unless `on_holes`, and those it puts there
/// with `holds_slug: true` (#302), every other with `false`; and on a path
/// too short for them, the most that cover at most half of it, but at
/// least one on a part. Each tab is
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
    place_shared(project, op, spec, &[])
}

/// [`place`] for a Profile that cuts an outline in pieces (#710): a male
/// Inlay's cut-out, where the board's edge frees the plug and leaves its
/// outline as several open paths ([`crate::inlay::cut_out_paths`]). Each
/// of `pieces` is the profiles, in the definition of a target of `op`,
/// that one outline was left as, in its order. They take the request
/// between them, not each for itself: `count` tabs shared by length, with
/// the largest remainders rounded up, and `at_distances` counted along the
/// pieces end to end (`Request::shared`). Each piece then lays its share
/// by the rules of any open path: off its ends, and fewer, with
/// [`Warning::TabsCoverMostOfContour`], when it is too short for them.
///
/// Every other profile is laid as [`place`] lays it, and so is an outline
/// left as one piece; with no `pieces` this is [`place`]. A request for
/// tabs per outline is still per outline (phase 2 functional spec,
/// capability 26): the pieces are one outline.
///
/// Fails as [`place`] does.
pub fn place_shared(
    project: &Project,
    op: &OperationId,
    spec: &TabSpec,
    pieces: &[Range<usize>],
) -> Result<Placement, CamError> {
    let profile = profile_op(project, op)?;
    let asking = ProfileOp {
        tabs: Some(spec.clone()),
        ..profile.clone()
    };
    let cut = op_tabs(project, &asking)?;
    // What the request lays on each tool path: for itself, then anew on
    // the pieces that share it.
    // The plan holds one of each per path: a path with none would get no
    // tab, and its part come loose.
    let named = |e: CamError| e.for_op(op);
    let mut fits: Vec<Fit> = Vec::with_capacity(cut.paths.len());
    for j in 0..cut.paths.len() {
        let plan = &cut.plan;
        let asked = at(
            &plan.requested,
            j,
            "a path has no slot for the tabs asked of it",
        );
        let reduced = at(
            &plan.request_reductions,
            j,
            "a path has no tab reduction slot",
        );
        let on_corners = at(
            &plan.on_corners_mm,
            j,
            "a path has no slot for tabs on corners",
        );
        fits.push(Fit {
            cuts: asked.map_err(named)?.clone(),
            reduced: reduced
                .map_err(named)?
                .as_ref()
                .map(|(reduced, _)| *reduced),
            on_corners_mm: on_corners.map_err(named)?.clone(),
        });
    }
    share(op, spec, &cut, &profile.targets, pieces, &mut fits)?;
    let mut placement = Placement::default();
    let mut next = next_number(project);
    for (j, ((path, shape), fit)) in cut.paths.iter().zip(&fits).enumerate() {
        let length_mm = path.length_mm();
        // #302: a tab the request puts on a hole, which it does only with
        // `on_holes`, is there to hold the slug.
        let on_hole = *at(
            &cut.holes,
            j,
            "a path does not say whether it is cut round a hole",
        )
        .map_err(named)?;
        let holds_slug = spec.on_holes && on_hole;
        if let Some(reduced) = fit.reduced {
            placement.warnings.push(coverage_warning(
                op,
                j,
                Some(shape),
                (cut.diameter_mm, &vec![spec.width_mm; reduced.asked]),
                length_mm,
                reduced,
            ));
        }
        if let Some(tabs) = &cut.plan.tabs
            && tabs.layout(&fit.cuts, length_mm, ends(path, &cut)).1
        {
            placement
                .warnings
                .push(overlap_warning(op, j, Some(shape), length_mm));
        }
        // #285: a tab left on a corner is placed there, warned.
        placement
            .warnings
            .extend(
                fit.on_corners_mm
                    .iter()
                    .map(|&(distance_mm, reason)| Warning::TabOnCorner {
                        op: op.clone(),
                        shape: shape.clone(),
                        distance_mm,
                        reason,
                    }),
            );
        for tab_cut in &fit.cuts {
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
            // The shape was placed by its instance to be cut, so the
            // instance is there and its transform has an inverse.
            let centre = definition_point(project, shape, q).ok_or_else(|| {
                CamError::internal("a tab's shape has no instance to place the tab in").for_op(op)
            })?;
            let tab = PlacedTab {
                id: TabId::new(format!("tab{next}")),
                instance: shape.instance.clone(),
                profile: shape.profile,
                x_mm: centre.x,
                y_mm: centre.y,
                width_mm: tab_cut.width_mm,
                height_mm: tab_cut.top_z_mm,
                shape: tab_cut.shape,
                holds_slug: Some(holds_slug),
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
/// outline Profile `op` cuts: none on a hole unless `on_holes`, and those
/// it puts there with `holds_slug: true` (#302), every other with
/// `false`. A closed
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
        // The shape is one the Profile cuts, so the project has it.
        let outline = definition_profile(project, shape).ok_or_else(|| {
            CamError::internal("a shape a Profile cuts is not in the project").for_op(op)
        })?;
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
                // #302: only `on_holes` puts a tab on a hole, to hold
                // its slug.
                holds_slug: Some(hole),
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
    // One layout and one reduction per path, as `plan_tabs` builds them.
    let per_path = laid
        .paths
        .iter()
        .zip(&laid.plan.layouts)
        .zip(&laid.plan.reductions);
    for (j, (((path, shape), layout), reduction)) in per_path.enumerate() {
        let length_mm = path.length_mm();
        if layout.as_ref().is_some_and(|(_, overlapped)| *overlapped) {
            placement
                .warnings
                .push(overlap_warning(op, j, Some(shape), length_mm));
        }
        if let Some((reduced, widths_mm)) = reduction {
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
    // By index: each request is placed on the project as the requests
    // before it left it.
    for i in 0..out.operations.len() {
        let operation = at(
            &out.operations,
            i,
            "an operation left the project being placed",
        )?;
        // Only an enabled Profile with a request has tabs to place.
        let Operation::Profile(op) = operation else {
            continue;
        };
        let (Some(spec), true) = (op.tabs.clone(), op.enabled) else {
            continue;
        };
        let id = op.id.clone();
        let placement = place(&out, &id, &spec)?;
        out.tabs.extend(placement.tabs);
        warnings.extend(placement.warnings);
        let placed = at_mut(
            &mut out.operations,
            i,
            "a Profile whose tabs were placed left the project",
        )
        .map_err(|e| e.for_op(&id))?;
        if let Operation::Profile(op) = placed {
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
/// [`Warning::TabOnHole`] for, and those that hold their slugs, which it
/// does not (#300, #302: `holds_slug`, or unset and on this Profile's path
/// round the hole). `tabs place` warns for the ones it places. Fails as
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

/// Lays `spec` anew on the tool paths of every outline `pieces` names
/// among the profiles of each of `targets` (#710, [`place_shared`]), in
/// `fits`, one for each path of `cut`: the paths of one outline take the
/// request between them (`Request::shared`), and each lays its share as
/// [`place`] lays a request on a path. A hole's path takes none without
/// `on_holes`, as in [`place`]; an outline with one path keeps what it
/// has.
fn share(
    op: &OperationId,
    spec: &TabSpec,
    cut: &OpTabs,
    targets: &[InstanceId],
    pieces: &[Range<usize>],
    fits: &mut [Fit],
) -> Result<(), CamError> {
    let Some(tabs) = &cut.plan.tabs else {
        return Ok(());
    };
    let request = Request::new(op, spec, cut.diameter_mm)?;
    for (target, outline) in targets
        .iter()
        .flat_map(|t| pieces.iter().map(move |p| (t, p)))
    {
        // The outline's tool paths, in its order: drawing order.
        let of: Vec<(&Polyline, bool, &mut Fit)> = cut
            .paths
            .iter()
            .zip(&cut.holes)
            .zip(fits.iter_mut())
            .filter(|(((_, shape), _), _)| {
                shape.instance == *target && outline.contains(&shape.profile)
            })
            .filter(|((_, hole), _)| spec.on_holes || !**hole)
            .map(|(((path, _), hole), fit)| (path, *hole, fit))
            .collect();
        if of.len() < 2 {
            continue;
        }
        let lengths_mm: Vec<f64> = of.iter().map(|(path, _, _)| path.length_mm()).collect();
        for ((path, hole, fit), request) in of.into_iter().zip(request.shared(&lengths_mm)) {
            // #109: a part keeps a tab; a hole or an open path may not.
            let least = if hole || !path.is_closed() {
                Least::Zero
            } else {
                Least::One
            };
            *fit = request.fit(tabs, path, ends(path, cut), least);
        }
    }
    Ok(())
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
mod tests;
