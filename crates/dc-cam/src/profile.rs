// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts
//   src/engine/toolpaths/geometry.ts

//! Profile toolpaths (`docs/specs/phase-1/design.md`, "Algorithms → Profile
//! contour", steps 1–8; the tabs of step 7 are in [`crate::tabs`]).
//!
//! Re-derived for Y-up (ADR-0001) rather than copied: PureCutCNC works in
//! Y-down screen space, where every winding below is mirrored. Here outer
//! contours are counter-clockwise, holes clockwise, a positive bulge is a
//! counter-clockwise arc, and the direction rule is [`traversal_is_ccw`].

mod entries;
mod last_pass;
mod start;
#[cfg(test)]
mod start_tests;
mod waste;

use crate::block::CutBlock;
use crate::entry::{
    self, CutSide, EntryKind, EntryTool, EntryWarnings, FallbackReason, MIN_BOUNDARY_SAFETY_MM,
};
use crate::error::{CamError, FeedField, ShapeRef};
use crate::lead::{self, DomainSpace, DropReason, LeadArc, LeadIn, LeadOut, LeadWarnings};
use crate::overlap::{CutPath, overlap_warnings, region_holds_path, within};
use crate::relief;
use crate::sequence::{SequenceInput, clearance_z_mm, plan, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::{
    LapRates, Least, PathEnds, PathTabs, Reduced, Request, TabCut, Tabs, check_size,
    coverage_warning, cut_range_warnings, lap_cuts, overlap_warning,
};
use crate::toolpath::{Move, Toolpath};
use crate::warning::{OpenPathReason, TabOnCornerReason, Warning};
use dc_geometry::{
    Contour, EPS_MM, GeometryError, GeometryWarning, Point, Point3, Polyline, Region, SelfBand,
    distance_away_from_mm, distance_mm, inset_is_empty, meeting_points, nearest_point, nest,
    offset, offset_open, point_segment_distance_mm,
};
use dc_project::{
    CutDirection, Entry, InstanceId, Lead, OperationId, ProfileOp, Project, Side, TabId, Tool,
};
use entries::{
    EntrySetup, PassEntry, PathDomain, Space, Targets, off_material, path_entries, plunged,
    tab_seams, wall_domains,
};

/// One path's tab layout, and whether its spans overlapped; `None`
/// without tabs.
type PathLayout = Option<(PathTabs, bool)>;

/// Which side of the tool the uncut wall lies on, for direction purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wall {
    /// The tool runs outside the shape whose wall it cuts, as around a
    /// part: an outer contour's `+r` offset, a hole's `+r` offset (Inside
    /// mode), and an outer cut on the line.
    Outside,
    /// The tool runs inside the shape whose wall it cuts, as round a hole:
    /// every `−r` offset, and a hole cut on the line.
    Inside,
}

/// The direction rule: true when the tool centre must travel
/// counter-clockwise (viewed from +Z, Y-up) to cut `wall` with `direction`.
///
/// The spindle turns clockwise viewed from above, so a tooth meets the
/// material on the tool's right going against the feed: climb milling keeps
/// the uncut wall on the right of travel. Around an outside wall the shape
/// is inside the loop, so wall-on-the-right means clockwise; round an inside
/// wall the shape is outside the loop, so counter-clockwise. Conventional
/// is the reverse of climb.
#[must_use]
pub fn traversal_is_ccw(wall: Wall, direction: CutDirection) -> bool {
    let climb_is_ccw = match wall {
        Wall::Outside => false,
        Wall::Inside => true,
    };
    match direction {
        CutDirection::Climb => climb_is_ccw,
        CutDirection::Conventional => !climb_is_ccw,
    }
}

/// The open-path direction rule: true when an open path cut on `side`
/// with `direction` runs from its last vertex to its first.
///
/// Left and Right (#207) cut one flank of the bit against kept material,
/// so `direction` is the milling mode, as in [`traversal_is_ccw`]: climb
/// keeps the part on the right of travel (#287). Left runs the tool to the
/// path's left with the part beyond the path, which is on the tool's right
/// going from the first vertex to the last; Right is the mirror. On line
/// cuts with both flanks, so no milling mode exists and `direction` picks
/// the starting end as VCarve's does (#242): "Conventional starts cutting
/// at the Start point Climb cutting starts cutting at the end point".
fn open_runs_reversed(side: Side, direction: CutDirection) -> bool {
    let climb = direction == CutDirection::Climb;
    match side {
        Side::Left => !climb,
        Side::Right | Side::OnLine => climb,
        // Never asked: Inside and Outside cut no open path.
        Side::Inside | Side::Outside => climb,
    }
}

/// A contour's place in its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A region's counter-clockwise outer boundary.
    Outer,
    /// A clockwise hole inside a region.
    Hole,
}

/// Which way to offset a contour of `role` for `side`, and which wall the
/// tool then cuts. `Grow` moves away from the area the contour encloses,
/// as `dc_geometry::offset` does for a positive delta; for a hole that area
/// is the hole itself.
///
/// Inside and Outside are relative to the part (the region's area), so a
/// hole is offset the opposite way to its outer.
fn placement(side: Side, role: Role) -> (Offset, Wall) {
    match (side, role) {
        (Side::Outside, Role::Outer) | (Side::Inside, Role::Hole) => (Offset::Grow, Wall::Outside),
        (Side::Outside, Role::Hole) | (Side::Inside, Role::Outer) => (Offset::Shrink, Wall::Inside),
        (Side::OnLine, Role::Outer) => (Offset::None, Wall::Outside),
        (Side::OnLine, Role::Hole) => (Offset::None, Wall::Inside),
        // Never asked: Left and Right cut no closed outline (#207).
        (Side::Left | Side::Right, _) => (Offset::None, Wall::Outside),
    }
}

/// Which way a contour moves to become the tool-centre path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offset {
    /// By `+r`, away from the enclosed area.
    Grow,
    /// By `−r`, into the enclosed area.
    Shrink,
    /// Not at all: the tool centre follows the line.
    None,
}

/// A rate the user set to a positive number, or `None`.
fn usable_rate(rate_mm_min: Option<f64>) -> Option<f64> {
    rate_mm_min.filter(|r| r.is_finite() && *r > 0.0)
}

/// The rates every cut of this operation uses, exactly as the user set
/// them on the tool.
pub(crate) fn rates_of(tool: &Tool) -> Result<LapRates, CamError> {
    let missing = |field| CamError::MissingFeed {
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        field,
    };
    Ok(LapRates {
        feed_mm_min: usable_rate(tool.feed_mm_min).ok_or_else(|| missing(FeedField::Feed))?,
        plunge_mm_min: usable_rate(tool.plunge_mm_min).ok_or_else(|| missing(FeedField::Plunge))?,
    })
}

/// Generates the Profile toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Each target instance's closed profiles are placed by its transform,
/// nested into parts and holes, offset by the tool radius per `op.side`, and
/// oriented per [`traversal_is_ccw`]. Each tool-centre contour becomes one
/// [`CutBlock`] per depth pass (a straight plunge at the plunge rate, then
/// one lap at the feed rate ending at the start; arc segments stay arcs),
/// and [`sequence`] orders and links the blocks by `op.machining_order`:
/// shapes are visited nearest-neighbour from the first shape in drawing
/// order, with every hole cut before its outer contour and every part
/// inside another part's hole before that hole, at every nesting depth, so
/// a part is freed only by its last contour. Between shapes the tool
/// retracts to safe Z; between passes of one contour it plunges straight
/// from the previous pass.
///
/// Every placed tab of [`Project::tabs`] on an outline `op` cuts (#105,
/// ADR-0005) is cut round: every pass below the tab's top rises over its
/// span (`width + diameter` along the tool-centre contour, centred on the
/// contour's point nearest the tab), and a placed tab on a hole warns
/// [`Warning::TabOnHole`], unless it sits on this Profile's contour round
/// the hole, where only placing tabs asked for on holes puts one (#300).
/// A tab request (`op.tabs`) is placed first, as
/// `dreamcarve tabs place` places it (#109): per contour, none on a hole
/// unless `on_holes`, and on a contour whose spans would cover more than
/// half of it the most that cover at most half (a part's outline at
/// least one; a hole may get none), with
/// [`Warning::TabsCoverMostOfContour`]; placed tabs are cut as placed, and
/// warn the same when they cover more than half. Overlapping spans merge
/// with [`Warning::TabsOverlap`], and a tab top no pass reaches, or one
/// above the stock top, gives [`Warning::TabsOutsideCutRange`].
///
/// With `op.entry` a ramp or helix (design spec, "Algorithms → Entry"),
/// each pass reaches its depth at the contour start without a plunge:
/// Inside and Outside place the zigzag or helix in the waste domain
/// beside the wall ([`entry::plan_at_wall`]), the waste less every other
/// region's kerf and, below the tab top, less the tabs; On line ramps back
/// and forth along the path itself ([`entry::along_path`]). The first pass
/// descends from safe Z, later ones from the previous pass Z at the
/// contour start, where the previous lap ended. A contour without a lead
/// whose entry would plunge below the tab top at its start, under or
/// beside a tab, starts instead in the middle of the longest stretch
/// between tabs where it does not (#176), on every pass; the tabs stay at
/// their distances from the original start. An entry that does not fit
/// falls back to a ramp, then a plunge, with
/// [`Warning::EntryFallback`]; so does a contour whose start no
/// stretch frees.
///
/// With `op.lead` (design spec, "Algorithms → Lead-in and lead-out"),
/// Inside and Outside contours are re-seamed where an arc tangent to them
/// fits in the waste domain away from corners ([`crate::lead`]): each
/// pass descends at the arc's far end, its staging point, cuts the arc,
/// the lap from the arrival, the overcut and the exit arc at the feed
/// rate, and retracts. Below the tab top the arcs keep off the tabs, and
/// the tabs stay where they were along the outline. A lead that fits only
/// smaller warns [`Warning::LeadShrunk`]; one that does not fit, an
/// exit that does not fit, and any lead on an On line Profile warn
/// [`Warning::LeadDropped`].
///
/// With `op.corner_relief`, a dogbone or T-bone notch is cut in each
/// corner the round tool cannot reach (Inside: the hole's corners;
/// Outside: the part's concave corners), stepping down at the tool's
/// step-down among the shape's own passes, each level after the pass that
/// opened the column it plunges in and none after the pass that frees the
/// shape, whose levels are cut from inside its lap (the `relief` module,
/// #189, #193); On line warns
/// [`Warning::ReliefSkipped`].
///
/// With `op.last_pass` (#284, design spec addendum A8; VCarve's "Last
/// Pass Allowance") every pass but the last cuts each closed shape's
/// allowance path, its tool-centre path offset `allowance_mm` farther
/// into the waste, and the last pass cuts the shape's own path at full
/// depth, after the usual passes to `depth − thickness_mm` when a
/// thickness is set, the other way round when `reverse`. The tabs are laid
/// on the shape's path and carried onto the allowance path widened by the
/// allowance; entries, leads and waste domains are planned per path; every
/// relief level is cut from the last pass. A drawing the allowance passes
/// cannot follow is refused ([`CamError::BandNarrowerThanAllowance`],
/// [`CamError::ShapeNarrowerThanAllowance`]), and a last pass On line, or
/// out of range, is [`CamError::LastPassNeedsSide`] or
/// [`CamError::InvalidLastPass`]. A Left or Right Profile (#311) takes a
/// last pass too: each open path's allowance path is its offset by
/// `r + a` to the same side, ending square on the same end normals, so
/// no pass cuts past an end and the last pass takes the allowance off up
/// to each; the twin runs the way its path does, ramps along itself, and
/// carries the tabs as a closed shape's twin does. `reverse` runs the
/// last pass from the path's other end, where the earlier passes end. The
/// allowance paths must follow the line ([`last_pass::open_twins`],
/// [`CamError::ShapeNarrowerThanAllowance`]) and keep `r + a` from every
/// other line and kept strip ([`CamError::AllowanceCutsIntoKeptSide`]).
///
/// An On line Profile also cuts open paths (#175, as VCarve's Profile
/// "On" an open vector): each pass runs the path from its first vertex to
/// its last (conventional) or from its last to its first (climb, #242), with
/// the ramp along it and its tabs placed from its first vertex as drawn,
/// then retracts; it has no lead, no relief and frees nothing. Inside and
/// Outside skip open paths ([`Warning::OpenPathSkipped`]), as they
/// have no side.
///
/// A Left or Right Profile (#207, as VCarve's Profile to the left or right
/// of an open vector) cuts open paths only, each offset by the tool radius
/// to that side looking from its first vertex to its last
/// ([`offset_open`]: round on the outside of a turn, trimmed on the
/// inside, ending square on the normals through the path's ends), and
/// skips closed targets with [`Warning::SideNeedsOpenPath`]. A path whose
/// offset vanishes warns [`Warning::ToolTooLarge`]. Direction is the
/// milling mode (#287): climb keeps the part on the right of travel, so
/// Left climb runs from the end beside the first vertex and Right climb
/// from the end beside the last; conventional is the reverse. Ramp and
/// tabs are On line's, along the offset path; it has no lead or relief
/// and frees nothing. A cut that comes within the radius of another of its
/// lines, or of the strip a line keeps, fails with
/// [`CamError::CutsIntoKeptSide`]; D6's overlap warning, whose condition
/// that is, does not apply.
///
/// Open paths Inside and Outside, shapes the tool cannot fit, and
/// duplicate shapes (closed, or open paths drawn twice either way, #221)
/// are skipped with a [`Warning`]. Shapes in different
/// regions whose tool paths cut into each other give
/// [`Warning::ToolpathsOverlap`], in cut order; an open path cut On
/// line is a shape of its own unless it lies in a region's material
/// (#206, [`crate::overlap`]). Fails without a toolpath when a feed, plunge rate, or
/// step-down is missing, when the depth goes below the stock bottom plus
/// allowance, when targets cross, when an id does not resolve, when the
/// cutter would leave the stock ([`CamError::OffStock`]), when an entry's
/// angle or helix fraction is out of range ([`CamError::InvalidEntry`]),
/// when a lead's radius, sweep or overcut is ([`CamError::InvalidLead`]),
/// when an Inside path would come closer than the tool radius, or an On
/// line path closer than its diameter, to another line of its region, a
/// band narrower than the tool ([`CamError::BandNarrowerThanTool`], #192,
/// #199; a contour with no path at all is [`Warning::ToolTooLarge`]
/// and skipped; an open path a region holds is compared with that
/// region's lines and its other open paths away from where they meet,
/// #210; an On line line is also compared with its own stretches far
/// along it, #221, so a hairpin or a narrow neck is refused),
/// when an On line part is nowhere as wide as the tool, so the cut
/// removes it whole ([`CamError::PartNarrowerThanTool`], #223),
/// when an On line Profile asks for a helix
/// ([`CamError::HelixNeedsWasteSide`]), or when a descent or lead arc
/// would leave its waste domain ([`CamError::OffMaterial`], a defect).
#[must_use = "the toolpath is the whole point"]
pub fn generate_profile(
    project: &Project,
    op: &ProfileOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    // Outside and Inside offset by half the diameter, right only for a flat
    // end mill at every depth (#67). On the line the tool centre follows
    // the line, so any cutter cuts it as drawn.
    if op.side != Side::OnLine && !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    let rates = rates_of(tool)?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    entry::check(&op.id, &op.entry)?;
    if !op.side.has_waste_area() && matches!(op.entry, Entry::Helix { .. }) {
        return Err(CamError::HelixNeedsWasteSide { op: op.id.clone() });
    }
    let step_down_mm =
        op.step_down_mm
            .or(tool.step_down_mm)
            .ok_or_else(|| CamError::MissingStepDown {
                op: op.id.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
            })?;
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let depth_mm = op
        .depth_mm
        .ok_or_else(|| CamError::MissingDepth { op: op.id.clone() })?;
    // #284: a last pass, with its own depth when it sets a thickness.
    let last_pass = op.last_pass.as_ref();
    if let Some(last_pass) = last_pass {
        last_pass::check(op, last_pass, tool.diameter_mm, depth_mm, step_down_mm)?;
    }
    let pass_z = last_pass::pass_z(op, thickness_mm, depth_mm, step_down_mm, last_pass)?;
    let last_index = pass_z.len().saturating_sub(1);
    // The passes before the last run on the allowance paths; with no pass
    // before it there are none.
    let allowance_mm = last_pass
        .filter(|_| pass_z.len() > 1)
        .map(|l| l.allowance_mm);
    let reverse_last = last_pass.is_some_and(|l| l.reverse);
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let start_z_mm = entry::descent_start_z_mm(thickness_mm, safe_z_mm);

    // A tab request is checked before the targets, as phase 2 checked
    // its tab setting (#105: it is placed, then cut as placed tabs).
    let request = op
        .tabs
        .as_ref()
        .map(|spec| Request::new(&op.id, spec, tool.diameter_mm))
        .transpose()?;

    let mut warnings = Vec::new();
    // The tab range warnings come first, where phase 2 gave them.
    let range_at = warnings.len();
    // #175: On line cuts open paths too; Inside and Outside skip them.
    let (shapes, open_paths, regions) = target_geometry(project, op, &mut warnings)?;
    let radius_mm = tool.diameter_mm / 2.0;
    let reversed_open = open_runs_reversed(op.side, op.direction);
    let placed = centre_paths(
        op,
        tool,
        &regions,
        &shapes,
        &open_paths,
        project.flatten_tolerance_mm,
        allowance_mm,
        &mut warnings,
    )?;
    // #285: an explicit start re-seams each closed path, and its allowance
    // twin, nearest the start point, before the tabs are laid on it.
    let (placed, fixed) = start::seat(project, op, placed);
    let walls: Vec<(Wall, usize)> = placed.iter().map(|c| (c.wall, c.region)).collect();
    let holes: Vec<bool> = placed.iter().map(|c| c.hole).collect();
    let mut twins: Vec<Option<Polyline>> = Vec::with_capacity(placed.len());
    let mut paths: Vec<(Polyline, &ShapeRef)> = Vec::with_capacity(placed.len());
    for c in placed {
        twins.push(c.twin);
        paths.push((c.path, c.shape));
    }

    // Tab layout per path, and one block per path per pass (#105): the
    // placed tabs on each path's outline, and the tabs a request asks for.
    let TabPlan {
        tabs,
        layouts,
        reductions,
        tops_z_mm,
        on_holes,
        on_corners_mm,
        ..
    } = plan_tabs(
        project,
        op,
        request.as_ref(),
        tool.diameter_mm,
        &paths,
        &holes,
        reversed_open,
    )?;
    if tabs.is_some() {
        let range = cut_range_warnings(&op.id, &tops_z_mm, &pass_z, thickness_mm);
        warnings.splice(range_at..range_at, range);
    }
    // A tab placed on the hole for this cut holds the slug because the
    // user asked: `tabs place` warned when it placed it (#300).
    for (tab, shape, _) in on_holes.into_iter().filter(|(_, _, asked)| !asked) {
        warnings.push(Warning::TabOnHole {
            op: op.id.clone(),
            tab,
            shape,
        });
    }
    // #285: a requested tab with nowhere off its corner is cut there.
    for ((_, shape), on) in paths.iter().zip(&on_corners_mm) {
        warnings.extend(
            on.iter()
                .map(|&(distance_mm, reason)| Warning::TabOnCorner {
                    op: op.id.clone(),
                    shape: (*shape).clone(),
                    distance_mm,
                    reason,
                }),
        );
    }
    // #284: the part's `n` paths, then each one's allowance twin, which
    // the passes before the last cut; `twin_of` finds it. A twin's tabs
    // are its part path's, carried onto it and widened by the allowance;
    // a reversed last pass runs its path the other way from the same
    // start, its tabs where they were.
    let n = paths.len();
    let mut walls = walls;
    let mut layouts = layouts;
    let mut allowances: Vec<f64> = vec![0.0; n];
    let mut reversed: Vec<bool> = vec![false; n];
    let mut twin_of: Vec<Option<usize>> = vec![None; n];
    let mut fixed = fixed;
    if let Some(allowance_mm) = allowance_mm {
        // The last pass's floor, where the tabs it leaves stand from.
        let floor_z_mm = pass_z.last().copied().unwrap_or(0.0);
        for (i, twin) in twins.into_iter().enumerate() {
            let (Some(twin), Some((path, shape)), Some(wall), Some(layout)) =
                (twin, paths.get(i), walls.get(i), layouts.get(i))
            else {
                continue;
            };
            let carried = layout.as_ref().map(|(l, overlapped)| {
                (
                    last_pass::twin_tabs(path, l, &twin, radius_mm, floor_z_mm),
                    *overlapped,
                )
            });
            let (shape, wall) = (*shape, *wall);
            if let Some(slot) = twin_of.get_mut(i) {
                *slot = Some(paths.len());
            }
            paths.push((twin, shape));
            walls.push(wall);
            layouts.push(carried);
            allowances.push(allowance_mm);
            reversed.push(false);
            let own = fixed.get(i).copied().unwrap_or(false);
            fixed.push(own);
        }
    }
    if reverse_last {
        for i in 0..n {
            let (Some((path, _)), Some(layout)) = (paths.get_mut(i), layouts.get_mut(i)) else {
                continue;
            };
            // #311: an open path's last pass runs from its other end,
            // where the earlier passes end, its tabs measured from there.
            let back = if path.is_closed() {
                last_pass::reversed_from_start(path)
            } else {
                path.reversed()
            };
            if let Some((l, _)) = layout {
                *l = if path.is_closed() {
                    last_pass::carry_reversed(path, l, &back)
                } else {
                    l.reversed_open()
                };
            }
            *path = back;
            if let Some(flag) = reversed.get_mut(i) {
                *flag = true;
            }
        }
    }
    let quantum_mm = project
        .post_processor
        .decimal_places
        .quantum_mm(project.display_units);
    let entry_tool = |wall: Wall| EntryTool {
        diameter_mm: tool.diameter_mm,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        direction: op.direction,
        side: match wall {
            Wall::Inside => CutSide::Internal,
            Wall::Outside => CutSide::External,
        },
        quantum_mm,
    };
    let setup = EntrySetup {
        op,
        stock: &project.stock,
        regions: &regions,
        paths: &paths,
        walls: &walls,
        layouts: &layouts,
        tabs: tabs.as_ref(),
        pass_z: &pass_z,
        start_z_mm,
        entry_tool: &entry_tool,
        tolerance_mm: project.flatten_tolerance_mm,
        allowances: &allowances,
        reversed: &reversed,
    };
    let lead = op.lead.as_ref();
    if let Some(lead) = lead {
        lead::check(&op.id, lead)?;
        match op.side {
            Side::OnLine => warnings.push(lead::on_line_warning(&op.id)),
            Side::Left | Side::Right => warnings.push(lead::open_path_warning(&op.id)),
            Side::Inside | Side::Outside => {}
        }
    }
    // The waste domains, which the entries and the leads both use.
    let domains = if lead.is_some() || !matches!(op.entry, Entry::Plunge) {
        wall_domains(&setup)
    } else {
        Vec::new()
    };
    let below_tabs = |z_mm: f64| tabs.as_ref().is_some_and(|t| z_mm < t.top_z_mm() - EPS_MM);
    let mut lead_warnings = LeadWarnings::default();
    let leads = match lead {
        Some(lead) => plan_leads(
            &LeadSetup {
                op,
                lead,
                paths: &paths,
                domains: &domains,
                diameter_mm: tool.diameter_mm,
                tolerance_mm: project.flatten_tolerance_mm,
                any_above_tabs: pass_z.iter().any(|z| !below_tabs(*z)),
                any_below_tabs: pass_z.iter().any(|z| below_tabs(*z)),
                fixed: &fixed,
            },
            &mut lead_warnings,
        )?,
        None => paths.iter().map(|_| None).collect(),
    };
    // #176: a path whose entry has no room at its start for the tabs
    // starts where it has, on every pass; its tabs stay put. A path with a
    // lead keeps the lead's seam, and one with an explicit start its start
    // (#285). The domains are the same either way.
    let kept: Vec<bool> = leads
        .iter()
        .zip(&fixed)
        .map(|(lead, fixed)| lead.is_some() || *fixed)
        .collect();
    let seams = tab_seams(&setup, &domains, &kept);
    let (paths, layouts): (Vec<(Polyline, &ShapeRef)>, Vec<PathLayout>) = paths
        .iter()
        .zip(&layouts)
        .zip(&seams)
        .map(|(((path, shape), layout), seam)| match seam {
            Some(seam_mm) => (
                (lead::reseam(path, *seam_mm), *shape),
                layout
                    .as_ref()
                    .map(|(l, overlapped)| (l.reseamed(*seam_mm), *overlapped)),
            ),
            None => ((path.clone(), *shape), layout.clone()),
        })
        .unzip();
    let setup = EntrySetup {
        paths: &paths,
        layouts: &layouts,
        ..setup
    };
    let targets: Vec<Targets> = paths
        .iter()
        .zip(&leads)
        .map(|((path, _), led)| {
            let start = path
                .vertices()
                .first()
                .map_or(Point::new(0.0, 0.0), |v| v.point());
            let staging = led.as_ref().map(|l| l.lead_in.staging());
            Targets {
                plain: staging.unwrap_or(start),
                tabbed: led
                    .as_ref()
                    .filter(|l| l.below_tabs)
                    .map_or(start, |l| l.lead_in.staging()),
            }
        })
        .collect();
    let entries = path_entries(&setup, &domains, &targets);
    let mut entry_warnings = EntryWarnings::default();
    let mut blocks = Vec::with_capacity(n * pass_z.len());
    // The part's path each block cuts, for naming contours in cut order.
    let mut block_paths = Vec::with_capacity(blocks.capacity());
    for i in 0..n {
        // The path the previous pass cut, whose entry this pass's follows.
        let mut previous_path: Option<usize> = None;
        let mut previous: Option<(bool, EntryKind)> = None;
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            // #284: every pass but the last cuts the allowance twin.
            let k = match twin_of.get(i).copied().flatten() {
                Some(twin) if pass < last_index => twin,
                _ => i,
            };
            if previous_path != Some(k) {
                previous_path = Some(k);
                previous = None;
            }
            let (Some((path, shape)), Some(layout)) = (paths.get(k), layouts.get(k)) else {
                continue;
            };
            let path_tabs = match (&tabs, layout) {
                (Some(tabs), Some((layout, _))) => Some((tabs, layout)),
                _ => None,
            };
            let Some(start) = path.vertices().first().map(|v| v.point()) else {
                continue;
            };
            let below_tabs = below_tabs(z_mm);
            let led = leads
                .get(k)
                .and_then(Option::as_ref)
                .filter(|l| l.below_tabs || !below_tabs);
            let led_block = led.and_then(|l| {
                led_block(shape, pass, z_mm, l, path_tabs, &pass_z, safe_z_mm, rates)
            });
            // A lead whose lap would start on a tab (never, since the
            // arrival is kept off the tabs) is not cut, and nor is the
            // entry planned for its staging point.
            let entry_target_lost = led.is_some() && led_block.is_none();
            let (mut block, on_tab, lap_start_z_mm) = match led_block {
                Some(block) => (block, false, z_mm),
                None => {
                    let Some(lap) = lap_cuts(path, z_mm, path_tabs, rates) else {
                        continue;
                    };
                    let on_tab = lap.start_z_mm > z_mm + EPS_MM;
                    let lap_start_z_mm = lap.start_z_mm;
                    let block =
                        contour_block(shape, pass, z_mm, start, lap, &pass_z, safe_z_mm, rates);
                    (block, on_tab || entry_target_lost, lap_start_z_mm)
                }
            };
            // An open path cuts a slot; it frees nothing.
            block.frees &= path.is_closed();
            if let Some(entry) = entries.get(k).and_then(Option::as_ref) {
                let pass_entry = PassEntry {
                    path,
                    pass,
                    z_mm,
                    below_tabs,
                    start_z_mm,
                    tool: entries::tool_for(&setup, k),
                    tolerance_mm: project.flatten_tolerance_mm,
                };
                if on_tab {
                    // The start sits under a tab span: the lap starts on
                    // the tab, and the block comes down onto it, never
                    // into it (#337); only the first pass that reaches the
                    // tab top there plunges, warned.
                    if block.descent_from_z_mm - lap_start_z_mm > EPS_MM {
                        entry_warnings.record(
                            shape,
                            &plunged(start, EntryKind::of(&op.entry), FallbackReason::NoRoom),
                        );
                    }
                    previous = Some((false, EntryKind::Plunge));
                } else {
                    // #214: the first pass's ramp or helix starts just
                    // above the stock, not at safe Z.
                    let from_z_mm = if pass == 0 {
                        start_z_mm
                    } else {
                        block.descent_from_z_mm
                    };
                    let (descent, from_z_mm) = entry.descend(&pass_entry, from_z_mm, &mut previous);
                    entry_warnings.record(shape, &descent);
                    entry.check(op, shape, &pass_entry, &descent, from_z_mm, domains.get(k))?;
                    block.descent_from = descent.from;
                    block.descent_from_z_mm = from_z_mm;
                    block.descent = descent.moves;
                }
            }
            blocks.push(block);
            block_paths.push(i);
        }
    }
    warnings.extend(entry_warnings.into_warnings(&op.id));
    if let Some(lead) = lead {
        warnings.extend(lead_warnings.into_warnings(&op.id, lead));
    }
    let containment = containment_pairs(&regions, &shapes);
    let mut input = SequenceInput {
        blocks,
        order: op.machining_order,
        containment: &containment,
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        stock_top_mm: thickness_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        cut_before: &[],
    };

    // Warnings name contours by their position in the cut order, which
    // the plan sets.
    let cut_order = plan(&input)?;
    let mut path_order: Vec<usize> = Vec::with_capacity(n);
    for index in cut_order {
        if let Some(&path_index) = block_paths.get(index)
            && !path_order.contains(&path_index)
        {
            path_order.push(path_index);
        }
    }
    // Paths the plan never reached (none in practice) keep their place.
    for index in 0..n {
        if !path_order.contains(&index) {
            path_order.push(index);
        }
    }
    for (cut_index, &path_index) in path_order.iter().enumerate() {
        let (Some((path, shape)), Some(Some((_, overlapped)))) =
            (paths.get(path_index), layouts.get(path_index))
        else {
            continue;
        };
        let length_mm = path.length_mm();
        if *overlapped {
            warnings.push(overlap_warning(&op.id, cut_index, Some(shape), length_mm));
        }
        if let Some(Some((reduced, widths_mm))) = reductions.get(path_index) {
            warnings.push(coverage_warning(
                &op.id,
                cut_index,
                Some(shape),
                (tool.diameter_mm, widths_mm),
                length_mm,
                *reduced,
            ));
        }
    }
    // D6: paths in cut order, so the warnings follow it; each part path's
    // allowance twin, cut before it, with it (#284).
    let cut_paths: Vec<CutPath<'_>> = path_order
        .iter()
        .flat_map(|&i| std::iter::once(i).chain(twin_of.get(i).copied().flatten()))
        .filter_map(|i| paths.get(i))
        .map(|(path, shape)| (path, Some(*shape)))
        .collect();
    // #207: a Left or Right cut that comes within the radius of another
    // line, D6's condition, was refused above (`check_kept_sides`).
    if !op.side.is_open_path_side() {
        warnings.extend(overlap_warnings(
            &cut_paths,
            &shapes,
            &open_paths,
            &regions,
            op.side,
            tool,
            allowance_mm.unwrap_or(0.0),
            project.flatten_tolerance_mm,
        ));
    }

    // Corner relief among each shape's own passes, never after the pass
    // that frees it (`crate::relief`, #189); a freeing pass's notches are
    // cut from inside its lap (#193). With a last pass allowance only the
    // last pass runs on the part's paths, so it opens every notch's column
    // and cuts every level (#284).
    let geometry = relief::ProfileGeometry {
        regions: &regions,
        shapes: &shapes,
        paths: paths.get(..n).unwrap_or_default(),
        walls: walls.get(..n).unwrap_or_default(),
        only_pass: allowance_mm.map(|_| last_index),
    };
    let notches = relief::profile_notches(
        project,
        op,
        tool,
        &geometry,
        &mut input.blocks,
        &mut warnings,
    )?;
    input.blocks.extend(notches);
    let toolpath = sequence(&input)?;
    // #62: the cutter must stay on the stock.
    if let Some(overhang) =
        swept_bounds(&toolpath, radius_mm).and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.id.clone(),
            overhang,
        });
    }
    // #310 review: a one-pass cut has no pass to leave the allowance.
    if last_pass.is_some() && pass_z.len() < 2 {
        warnings.push(Warning::LastPassUnused { op: op.id.clone() });
    }
    Ok((toolpath, warnings))
}

/// A Profile's targets placed in project space: its closed shapes, the
/// open paths it cuts (On line only, #175), and the closed shapes nested
/// into regions. Shapes drawn twice are dropped with a warning.
type TargetGeometry = (
    Vec<(ShapeRef, Contour)>,
    Vec<(ShapeRef, Polyline)>,
    Vec<Region>,
);

/// [`TargetGeometry`] of `op`, warning into `warnings`.
fn target_geometry(
    project: &Project,
    op: &ProfileOp,
    warnings: &mut Vec<Warning>,
) -> Result<TargetGeometry, CamError> {
    // #207: Left and Right cut open paths only and skip closed outlines.
    let mut open_paths = Vec::new();
    let keep_open = op.side.cuts_open_paths().then_some(&mut open_paths);
    let mut shapes = resolve_shapes(project, &op.id, &op.targets, keep_open, warnings)?;
    if op.side.is_open_path_side() {
        warnings.extend(
            shapes
                .drain(..)
                .map(|(shape, _)| Warning::SideNeedsOpenPath { shape }),
        );
    }
    let contours: Vec<Contour> = shapes.iter().map(|(_, c)| c.clone()).collect();
    let (regions, nest_warnings) = nest(contours).map_err(|e| crossing_error(e, &shapes))?;
    for warning in nest_warnings {
        let GeometryWarning::DuplicateContourDropped {
            index,
            duplicate_of,
        } = warning;
        if let (Some((dropped, _)), Some((kept, _))) = (shapes.get(index), shapes.get(duplicate_of))
        {
            warnings.push(Warning::DuplicateShapeDropped {
                shape: dropped.clone(),
                duplicate_of: kept.clone(),
            });
        }
    }
    // #221: an open path drawn twice is cut once, as a closed shape is.
    // #207: drawn the other way, its left is the other side, so under Left
    // and Right only a copy drawn the same way repeats it.
    let open_paths = drop_duplicate_paths(open_paths, !op.side.is_open_path_side(), warnings);
    Ok((shapes, open_paths, regions))
}

/// One tool-centre path.
struct CentrePath<'s> {
    /// The path.
    path: Polyline,
    /// The shape it cuts.
    shape: &'s ShapeRef,
    /// The wall it cuts.
    wall: Wall,
    /// The index of its region.
    region: usize,
    /// The shape is a hole.
    hole: bool,
    /// #284: the allowance path the passes before the last cut instead,
    /// `allowance_mm` farther from the shape, running the same way.
    twin: Option<Polyline>,
}

/// Every tool-centre path of `op` cut with `tool`, in drawing order:
/// each closed shape offset per `op.side` and oriented per
/// [`traversal_is_ccw`], then each open path as drawn (reversed under
/// climb, #242). Shapes the tool does not fit warn
/// [`Warning::ToolTooLarge`]; bands narrower than the tool fail. With a
/// last pass `allowance_mm` (#284) each closed shape's path has its
/// allowance twin, or the shape is refused ([`last_pass::twins`]); so has
/// each Left or Right piece of an open path (#311,
/// [`last_pass::open_twins`]).
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the paths; a struct would only rename them"
)]
fn centre_paths<'s>(
    op: &ProfileOp,
    tool: &Tool,
    regions: &[Region],
    shapes: &'s [(ShapeRef, Contour)],
    open_paths: &'s [(ShapeRef, Polyline)],
    tolerance_mm: f64,
    allowance_mm: Option<f64>,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<CentrePath<'s>>, CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let mut placed: Vec<CentrePath<'s>> = Vec::new();
    for (region_index, region) in regions.iter().enumerate() {
        let ordered = hole_shapes(region, shapes)
            .map(|(h, shape)| (h, shape, Role::Hole))
            .chain(outer_shape(region, shapes).map(|shape| (region.outer(), shape, Role::Outer)));
        for (contour, shape, role) in ordered {
            let (how, wall) = placement(op.side, role);
            let centres = match how {
                Offset::Grow => offset(contour, radius_mm),
                Offset::Shrink => offset(contour, -radius_mm),
                Offset::None => vec![contour.clone()],
            };
            if centres.is_empty() {
                warnings.push(Warning::ToolTooLarge {
                    shape: shape.clone(),
                    tool: tool.name.clone(),
                    tool_id: tool.id.clone(),
                    diameter_mm: tool.diameter_mm,
                });
                continue;
            }
            check_band(
                &op.id,
                op.side,
                tool,
                0.0,
                region,
                (contour, shape),
                &centres,
                shapes,
                tolerance_mm,
            )?;
            // #284: each part path's allowance twin.
            let twins: Vec<Option<Contour>> = match allowance_mm {
                Some(allowance_mm) => {
                    let outline = last_pass::Outline {
                        region,
                        contour,
                        shape,
                        how,
                        centres: &centres,
                    };
                    let twins =
                        last_pass::twins(op, tool, &outline, shapes, allowance_mm, tolerance_mm)?;
                    for twin in &twins {
                        last_pass::check_neighbours(
                            op,
                            tool,
                            regions,
                            shapes,
                            (twin.polyline(), shape, contour),
                            region_index,
                            allowance_mm,
                        )?;
                    }
                    twins.into_iter().map(Some).collect()
                }
                None => centres.iter().map(|_| None).collect(),
            };
            let ccw = traversal_is_ccw(wall, op.direction);
            let orient = |c: Contour| {
                if c.is_ccw() == ccw {
                    c.into_polyline()
                } else {
                    c.reversed().into_polyline()
                }
            };
            placed.extend(centres.into_iter().zip(twins).map(|(c, twin)| CentrePath {
                path: orient(c),
                shape,
                wall,
                region: region_index,
                hole: role == Role::Hole,
                twin: twin.map(orient),
            }));
        }
    }
    check_open_bands(op, tool, regions, shapes, open_paths, tolerance_mm)?;
    // #207: Left and Right cut each open path offset by the radius to that
    // side, looking from its first vertex to its last; a path whose
    // offset vanishes is not cut. The cut keeps the radius from every
    // other line and from the strip each line keeps.
    let side_mm = match op.side {
        Side::Left => Some(radius_mm),
        Side::Right => Some(-radius_mm),
        Side::Inside | Side::Outside | Side::OnLine => None,
    };
    let mut open_centres: Vec<(&ShapeRef, Vec<Polyline>)> = Vec::with_capacity(open_paths.len());
    // The line each entry of `open_centres` offsets.
    let mut open_lines: Vec<&Polyline> = Vec::with_capacity(open_paths.len());
    for (shape, path) in open_paths {
        let centres = match side_mm {
            Some(left_mm) => offset_open(path, left_mm),
            None => vec![path.clone()],
        };
        if centres.is_empty() {
            warnings.push(Warning::ToolTooLarge {
                shape: shape.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
                diameter_mm: tool.diameter_mm,
            });
            continue;
        }
        open_centres.push((shape, centres));
        open_lines.push(path);
    }
    // #311: each Left or Right piece's allowance twin, the path offset by
    // `r + a` to the same side, ending on the same end normals; then the
    // twins held to the kept-side rule with the radius grown by `a`.
    let mut open_twins: Vec<Vec<Option<Polyline>>> = open_centres
        .iter()
        .map(|(_, centres)| centres.iter().map(|_| None).collect())
        .collect();
    if let Some(left_mm) = side_mm {
        check_kept_sides(op, tool, open_paths, &open_centres, left_mm, 0.0)?;
        if let Some(allowance_mm) = allowance_mm {
            let mut twins: Vec<(&ShapeRef, Vec<Polyline>)> = Vec::with_capacity(open_centres.len());
            for ((shape, centres), path) in open_centres.iter().zip(&open_lines) {
                let found =
                    last_pass::open_twins(op, tool, (shape, path), centres, left_mm, allowance_mm)?;
                twins.push((shape, found));
            }
            check_kept_sides(op, tool, open_paths, &twins, left_mm, allowance_mm)?;
            open_twins = twins
                .into_iter()
                .map(|(_, found)| found.into_iter().map(Some).collect())
                .collect();
        }
    }
    // #175: an open path is cut On line as drawn, from the end
    // `open_runs_reversed` picks (#242 On line, #287 Left and Right); a
    // Left or Right offset of one (#207) from the end beside that vertex,
    // and its allowance twin (#311) the same way. It bounds no region, so
    // no containment and no waste domain; its wall is never asked.
    let reversed_open = open_runs_reversed(op.side, op.direction);
    let as_cut = |path: Polyline| if reversed_open { path.reversed() } else { path };
    for ((shape, centres), twins) in open_centres.into_iter().zip(open_twins) {
        for (path, twin) in centres.into_iter().zip(twins) {
            placed.push(CentrePath {
                path: as_cut(path),
                shape,
                wall: Wall::Outside,
                region: regions.len(),
                hole: false,
                twin: twin.map(as_cut),
            });
        }
    }
    // Drawing order is the order the targets were resolved in: target,
    // then profile. Stable, so the loops of one shape keep their offset
    // order.
    let drawing_index = |shape: &ShapeRef| {
        (
            op.targets.iter().position(|t| *t == shape.instance),
            shape.profile,
        )
    };
    placed.sort_by_key(|c| drawing_index(c.shape));
    Ok(placed)
}

/// The tabs a Profile cuts on each of its tool-centre paths (#105,
/// ADR-0005), in path order.
pub(crate) struct TabPlan {
    /// What the tabs share; `None` when the operation crosses no placed
    /// tab and requests none.
    pub tabs: Option<Tabs>,
    /// Each path's layout and whether its spans overlapped: `Some` for
    /// every path when `tabs` is.
    pub layouts: Vec<PathLayout>,
    /// Per path, when its tabs would cover more than half of it, what
    /// was asked and what is cut, with the width of each tab asked.
    pub reductions: Vec<Option<(Reduced, Vec<f64>)>>,
    /// The top of every tab cut, and the request's, for the cut range
    /// warnings.
    pub tops_z_mm: Vec<f64>,
    /// The placed tabs cut on a path round a hole, with the hole, and
    /// whether the tab sits on that path: where placing a request or
    /// `--count` that asked for tabs on holes puts it (#300).
    pub on_holes: Vec<(TabId, ShapeRef, bool)>,
    /// Every placed tab the operation crosses, in project order.
    pub crossed: Vec<TabId>,
    /// Per path, the tabs the request puts on it.
    pub requested: Vec<Vec<TabCut>>,
    /// Per path, what the request asked and cut when it cut some down.
    pub request_reductions: Vec<Option<(Reduced, Vec<f64>)>>,
    /// Per path, where each tab the request leaves on a corner is
    /// centred, mm along it as tabs are measured on it, and why (#285).
    pub on_corners_mm: Vec<Vec<(f64, TabOnCornerReason)>>,
}

/// Plans the tabs of `op` on `paths` (each with the shape it cuts; `holes`
/// says which are holes; open paths are cut from their last vertex when
/// `reversed_open`), cut with a tool `diameter_mm` wide.
///
/// A tab `request` is laid per path as phase 2 laid the tab setting
/// ([`Request::fit`], #109). Each placed tab of [`Project::tabs`] on an
/// outline `op` cuts goes on the path of that outline nearest it, centred
/// at the path's point nearest the tab ([`nearest_point`]), when that
/// path comes within the tool radius plus half the tab's width of where
/// the tab meets the outline (its foot, [`crate::placed_tabs`]): closer,
/// the cutter would cut into the tab, and when the cut goes below the
/// tab's top. A tab farther from every path, or taller than the cut is
/// deep, is not crossed. Fails with [`CamError::InvalidTab`] for a crossed tab whose
/// width or height is not a positive number.
fn plan_tabs(
    project: &Project,
    op: &ProfileOp,
    request: Option<&Request>,
    diameter_mm: f64,
    paths: &[(Polyline, &ShapeRef)],
    holes: &[bool],
    reversed_open: bool,
) -> Result<TabPlan, CamError> {
    let tolerance_mm = project.flatten_tolerance_mm;
    let ends_of = |path: &Polyline| {
        if path.is_closed() {
            PathEnds::Closed
        } else {
            PathEnds::Open {
                reversed: reversed_open,
            }
        }
    };
    let fitter = Tabs::new(
        request.map_or(0.0, Request::top_z_mm),
        diameter_mm,
        tolerance_mm,
    );
    let mut requested = Vec::with_capacity(paths.len());
    let mut reductions = Vec::with_capacity(paths.len());
    let mut on_corners = Vec::with_capacity(paths.len());
    for ((path, _), &hole) in paths.iter().zip(holes) {
        let Some(request) = request.filter(|r| !hole || r.on_holes()) else {
            requested.push(Vec::new());
            reductions.push(None);
            on_corners.push(Vec::new());
            continue;
        };
        // #109: a part keeps a tab; a hole or an open path may not.
        let least = if hole || !path.is_closed() {
            Least::Zero
        } else {
            Least::One
        };
        let fit = request.fit(&fitter, path, ends_of(path), least);
        requested.push(fit.cuts);
        reductions.push(fit.reduced.map(|r| (r, vec![request.width_mm(); r.asked])));
        on_corners.push(fit.on_corners_mm);
    }
    let request_reductions = reductions.clone();

    let radius_mm = diameter_mm / 2.0;
    let mut placed: Vec<Vec<TabCut>> = vec![Vec::new(); paths.len()];
    let mut crossed = Vec::new();
    let mut on_holes = Vec::new();
    // The cut's floor, mm above the stock bottom: a Profile whose cut
    // stops at or above a tab's top crosses nothing of it. Unknown while
    // the thickness or depth is unset, when every tab in reach counts.
    let floor_z_mm = project
        .stock
        .thickness_mm
        .zip(op.depth_mm)
        .map(|(thickness_mm, depth_mm)| thickness_mm - depth_mm);
    for tab in project
        .tabs
        .iter()
        .filter(|t| op.targets.contains(&t.instance))
        .filter(|t| floor_z_mm.is_none_or(|floor| floor < t.height_mm - EPS_MM))
    {
        let Some(point) = crate::placed_tabs::tab_point(project, tab) else {
            continue;
        };
        let nearest = paths
            .iter()
            .enumerate()
            .filter(|(_, (_, shape))| {
                shape.instance == tab.instance && shape.profile == tab.profile
            })
            .filter_map(|(j, (path, shape))| {
                nearest_point(path, point).map(|n| (j, path, *shape, n))
            })
            .min_by(|a, b| a.3.gap_mm.total_cmp(&b.3.gap_mm));
        let Some((j, path, shape, near)) = nearest else {
            continue;
        };
        // Crossed when the path runs within the tool radius plus half the
        // tab of where the tab meets the outline: a tab placed from
        // another Profile's path sits a tool radius off it.
        let foot = crate::placed_tabs::outline_foot(project, tab).unwrap_or(point);
        let reach_mm = nearest_point(path, foot).map_or(f64::INFINITY, |n| n.gap_mm);
        if reach_mm > radius_mm + tab.width_mm / 2.0 + EPS_MM {
            continue;
        }
        check_size(&op.id, &format!("`{}` width_mm", tab.id), tab.width_mm)?;
        check_size(&op.id, &format!("`{}` height_mm", tab.id), tab.height_mm)?;
        // An open path's tabs are measured from its first vertex as drawn.
        let centre_mm = if path.is_closed() || !reversed_open {
            near.along_mm
        } else {
            path.length_mm() - near.along_mm
        };
        if let Some(cuts) = placed.get_mut(j) {
            cuts.push(TabCut {
                centre_mm,
                width_mm: tab.width_mm,
                top_z_mm: tab.height_mm,
                shape: tab.shape,
            });
        }
        crossed.push(tab.id.clone());
        if holes.get(j).copied().unwrap_or(false) {
            // Only `place` with `on_holes` puts a tab on this path round
            // a hole, so a tab still on it was asked for (#300).
            on_holes.push((tab.id.clone(), shape.clone(), near.gap_mm <= EPS_MM));
        }
    }

    let tops_z_mm: Vec<f64> = request
        .map(Request::top_z_mm)
        .into_iter()
        .chain(placed.iter().flatten().map(|c| c.top_z_mm))
        .collect();
    let tabs = (request.is_some() || !crossed.is_empty()).then(|| {
        let top_z_mm = tops_z_mm.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Tabs::new(top_z_mm, diameter_mm, tolerance_mm)
    });
    let mut layouts = Vec::with_capacity(paths.len());
    for (j, (path, _)) in paths.iter().enumerate() {
        let Some(tabs) = &tabs else {
            layouts.push(None);
            continue;
        };
        let own = placed.get(j).map_or(&[][..], Vec::as_slice);
        let mut cuts = requested.get(j).cloned().unwrap_or_default();
        cuts.extend_from_slice(own);
        let length_mm = path.length_mm();
        let (layout, overlapped) = tabs.layout(&cuts, length_mm, ends_of(path));
        // Placed tabs are cut as placed: a layout that covers more than
        // half the path warns, with nothing cut down.
        if let Some(slot) = reductions.get_mut(j)
            && slot.is_none()
            && !own.is_empty()
            && layout.covered_mm() > length_mm / 2.0 + EPS_MM
        {
            let n = layout.tab_count();
            *slot = Some((
                Reduced {
                    asked: n,
                    cut: n,
                    placed: true,
                },
                cuts.iter().map(|c| c.width_mm).collect(),
            ));
        }
        layouts.push(Some((layout, overlapped)));
    }
    Ok(TabPlan {
        tabs,
        layouts,
        reductions,
        tops_z_mm,
        on_holes,
        crossed,
        requested,
        request_reductions,
        on_corners_mm: on_corners,
    })
}

/// The tool-centre paths of a Profile with the shape each cuts, and its
/// tab plan ([`plan_tabs`]), without its toolpath (#105).
pub(crate) struct OpTabs {
    /// Each tool-centre path with the shape it cuts, in drawing order.
    pub paths: Vec<(Polyline, ShapeRef)>,
    /// Whether each path is cut round a hole.
    pub holes: Vec<bool>,
    /// The operation cuts open paths from their last vertex (#242).
    pub reversed_open: bool,
    /// The tool's diameter, mm.
    pub diameter_mm: f64,
    /// The tabs on each path.
    pub plan: TabPlan,
}

/// [`OpTabs`] of `op` in `project`. Fails as [`generate_profile`] does
/// when the tool or the targets cannot be cut; feeds, depths and the
/// stock need not be set.
pub(crate) fn op_tabs(project: &Project, op: &ProfileOp) -> Result<OpTabs, CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    if op.side != Side::OnLine && !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    let request = op
        .tabs
        .as_ref()
        .map(|spec| Request::new(&op.id, spec, tool.diameter_mm))
        .transpose()?;
    let mut ignored = Vec::new();
    let (shapes, open_paths, regions) = target_geometry(project, op, &mut ignored)?;
    // Tabs are laid on the part's paths (#284), whatever the last pass.
    let centres = centre_paths(
        op,
        tool,
        &regions,
        &shapes,
        &open_paths,
        project.flatten_tolerance_mm,
        None,
        &mut ignored,
    )?;
    // #285: tabs a request lays count from the start point.
    let (centres, _) = start::seat(project, op, centres);
    let holes: Vec<bool> = centres.iter().map(|c| c.hole).collect();
    let paths: Vec<(Polyline, &ShapeRef)> =
        centres.into_iter().map(|c| (c.path, c.shape)).collect();
    let reversed_open = open_runs_reversed(op.side, op.direction);
    let plan = plan_tabs(
        project,
        op,
        request.as_ref(),
        tool.diameter_mm,
        &paths,
        &holes,
        reversed_open,
    )?;
    Ok(OpTabs {
        paths: paths.into_iter().map(|(p, s)| (p, s.clone())).collect(),
        holes,
        reversed_open,
        diameter_mm: tool.diameter_mm,
        plan,
    })
}

/// #192, #199: the band between two lines of one region, its outline
/// and its islands, must be at least as wide as the tool.
///
/// - **Inside**, the band is waste and the material beyond every line is
///   kept. Each tool-centre path of `contour` (`centres`) lies `r` from
///   `contour` by construction; one that comes closer than `r` to another
///   line of `region` would cut into the material kept beyond that line,
///   as an island's lap does in a band narrower than the tool.
/// - **On line**, the band is the part itself (a ring between an outline
///   and its hole, a web between two holes). The path is `contour`, and
///   the bit cuts `r` of the band beside every line, so one that comes
///   closer than the diameter to another line of `region` cuts through
///   the band together with that line's cut: the part loses material it
///   keeps. `contour` is also measured against itself
///   ([`check_self_band`], #221; `tolerance_mm` is the project's flatten
///   tolerance, where it meets itself). A region's outline whose inset by
///   `r` is empty bounds a part the cut removes whole, however short its
///   outline: [`CamError::PartNarrowerThanTool`] (#223). Its holes need no
///   such check: one within `d` of the outline is refused above.
/// - **Outside** needs no check: there the outline's path runs outside
///   the region and every hole's inside its hole, so each is at least `r`
///   from every other line of the region, and a band between them is a
///   thin wall, not a gouge.
///
/// [`CamError::BandNarrowerThanTool`] names the two shapes, the one drawn
/// first first. Paths in different regions are D6's
/// [`Warning::ToolpathsOverlap`]; under Inside never closer than `r`
/// once this check passes.
///
/// With a last pass `allowance_mm` (#284, more than 0), `centres` are the
/// allowance paths, `r + a` from `contour`, and Inside each must keep
/// `r + a` from every other line, so it cuts neither that line's skin nor
/// the part: the band must be `D + 2a` wide, or
/// [`CamError::BandNarrowerThanAllowance`].
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the band; a struct would only rename them"
)]
pub(super) fn check_band(
    op: &OperationId,
    side: Side,
    tool: &Tool,
    allowance_mm: f64,
    region: &Region,
    (contour, shape): (&Contour, &ShapeRef),
    centres: &[Contour],
    shapes: &[(ShapeRef, Contour)],
    tolerance_mm: f64,
) -> Result<(), CamError> {
    let reach_mm = match side {
        Side::Inside => tool.diameter_mm / 2.0 + allowance_mm,
        Side::OnLine => tool.diameter_mm,
        // Left and Right cut no closed outline (#207).
        Side::Outside | Side::Left | Side::Right => return Ok(()),
    };
    for (line, b) in region_shapes(region, shapes).filter(|(l, _)| *l != contour) {
        if !centres
            .iter()
            .any(|c| within(c.polyline(), line.polyline(), reach_mm))
        {
            continue;
        }
        let a = shape;
        let drawn = |s: &ShapeRef| shapes.iter().position(|(x, _)| x == s);
        let (shape, other) = if drawn(a) <= drawn(b) { (a, b) } else { (b, a) };
        let band_mm = distance_mm(contour.polyline(), line.polyline());
        return Err(if allowance_mm > 0.0 {
            CamError::BandNarrowerThanAllowance {
                op: op.clone(),
                shape: shape.clone(),
                other: Box::new(other.clone()),
                band_mm,
                diameter_mm: tool.diameter_mm,
                allowance_mm,
                needed_mm: tool.diameter_mm + 2.0 * allowance_mm,
            }
        } else {
            CamError::BandNarrowerThanTool {
                op: op.clone(),
                shape: shape.clone(),
                other: Box::new(other.clone()),
                band_mm,
                diameter_mm: tool.diameter_mm,
            }
        });
    }
    if side != Side::OnLine {
        return Ok(());
    }
    // #223: a part nowhere as wide as the bit is removed whole, however
    // short its outline (the self-band check below compares only
    // stretches `√2·d` apart along it, and a small circle has none).
    if contour == region.outer() && inset_is_empty(contour, tool.diameter_mm / 2.0) {
        return Err(CamError::PartNarrowerThanTool {
            op: op.clone(),
            shape: shape.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    let line = contour.polyline();
    let between = |p: Point, q: Point| material_between(region, line, p, q);
    check_self_band(op, shape, line, tool, tolerance_mm, between)
}

/// Whether the part lies between points `p` and `q` of `line`, a line of
/// `region` (#221): the point midway lies in the region's material, a
/// quarter of their gap or more from `line`. Across a notch or a hole's
/// neck it lies in the waste; between two points along one edge, as
/// either side of a notch's mouth, it lies on the line.
fn material_between(region: &Region, line: &Polyline, p: Point, q: Point) -> bool {
    let middle = p + (q - p) * 0.5;
    let depth_mm = p.distance_to(q) / 4.0;
    region.contains_point(middle)
        && line
            .segments()
            .all(|s| point_segment_distance_mm(s, middle) >= depth_mm)
}

/// #221: #199's and #210's On line band check for one line against
/// itself. Two points of `line` more than [`MEETING_REACH_DIAMETERS`]`·d`
/// apart along it whose cuts come closer than the tool's diameter `d`
/// (less [`EPS_MM`]) cut through the part between them, as a hairpin, a
/// spiral with tight turns, or an outline with a narrow neck does:
/// [`CamError::BandNarrowerThanTool`] naming `shape` twice. Neighbouring
/// stretches are not compared, so a smooth curve and a corner of 90° or
/// more pass. A corner sharper than 90° is where two lines meet (#210):
/// its arms are set aside within `√2·d` of its tip, so a V of 45° or more
/// passes and a sharper one is refused. So are stretches near where the
/// line touches or crosses itself. `counts` takes the closest points of a
/// pair and says whether the part lies between them: always for an open
/// path a region holds, and for a closed line as [`material_between`]
/// says, so a notch or a hole's neck, waste between two stretches, is
/// cut.
fn check_self_band(
    op: &OperationId,
    shape: &ShapeRef,
    line: &Polyline,
    tool: &Tool,
    tolerance_mm: f64,
    counts: impl Fn(Point, Point) -> bool,
) -> Result<(), CamError> {
    let diameter_mm = tool.diameter_mm;
    let band = SelfBand::new(
        line,
        MEETING_REACH_DIAMETERS * diameter_mm,
        tolerance_mm.max(EPS_MM),
    );
    match band.narrowest_mm(diameter_mm - EPS_MM, counts) {
        Some(band_mm) => Err(CamError::BandNarrowerThanTool {
            op: op.clone(),
            shape: shape.clone(),
            other: Box::new(shape.clone()),
            band_mm,
            diameter_mm,
        }),
        None => Ok(()),
    }
}

/// #221: `open` without the paths that repeat an earlier one exactly,
/// the same vertices in the same direction or, when `either_way`, the
/// reverse direction, each dropped with [`Warning::DuplicateShapeDropped`]
/// naming the one kept, as `nest` drops a closed shape drawn twice.
fn drop_duplicate_paths(
    open: Vec<(ShapeRef, Polyline)>,
    either_way: bool,
    warnings: &mut Vec<Warning>,
) -> Vec<(ShapeRef, Polyline)> {
    let mut kept: Vec<(ShapeRef, Polyline)> = Vec::with_capacity(open.len());
    for (shape, path) in open {
        let reversed = path.reversed();
        match kept
            .iter()
            .find(|(_, k)| *k == path || (either_way && *k == reversed))
        {
            Some((original, _)) => warnings.push(Warning::DuplicateShapeDropped {
                shape,
                duplicate_of: original.clone(),
            }),
            None => kept.push((shape, path)),
        }
    }
    kept
}

/// #207: the band rule for a Left or Right Profile. The side its cutter
/// runs on is waste and the other side is the part, so the bit must not
/// cross another line of the operation, nor reach the material a line
/// keeps beside it. Each path of `centres` (the open paths `open` offset
/// by `left_mm`, as cut) must keep the tool radius (less [`EPS_MM`]) from
/// every other line of `open`, and from every line's kept edge: the line
/// offset by the radius to its other side, so the strip one radius wide
/// beside it is never cut. A path is held to its own kept edge too, which
/// catches a spiral or hairpin whose cut reaches what the line keeps
/// further along; it keeps its own line by construction
/// ([`offset_open`]). Lines that meet are not set aside, unlike On line's
/// #210: a square end that runs onto the next line of a part drawn as
/// separate lines cuts into it. [`CamError::CutsIntoKeptSide`], the
/// shapes in drawing order.
///
/// With a last pass `allowance_mm` (#311, more than 0), `centres` are the
/// allowance paths, `r + a` from their lines, and each must keep `r + a`
/// from every other line and every kept edge, so it takes neither another
/// line's skin, which that line's last pass is to cut, nor any of the
/// strip it keeps: [`CamError::AllowanceCutsIntoKeptSide`]. The kept
/// edges stay one radius off their lines; S9 holds every pass to the
/// radius from them.
fn check_kept_sides(
    op: &ProfileOp,
    tool: &Tool,
    open: &[(ShapeRef, Polyline)],
    centres: &[(&ShapeRef, Vec<Polyline>)],
    left_mm: f64,
    allowance_mm: f64,
) -> Result<(), CamError> {
    let reach_mm = left_mm.abs() + allowance_mm;
    let kept: Vec<(&ShapeRef, &Polyline, Vec<Polyline>)> = open
        .iter()
        .map(|(shape, line)| (shape, line, offset_open(line, -left_mm)))
        .collect();
    let drawn = |s: &ShapeRef| (op.targets.iter().position(|t| *t == s.instance), s.profile);
    for &(shape, ref pieces) in centres {
        for &(other, line, ref edges) in &kept {
            let crosses = other != shape && pieces.iter().any(|c| within(c, line, reach_mm));
            let enters = || {
                pieces
                    .iter()
                    .any(|c| edges.iter().any(|e| within(c, e, reach_mm)))
            };
            if !crosses && !enters() {
                continue;
            }
            let (first, second) = if drawn(shape) <= drawn(other) {
                (shape, other)
            } else {
                (other, shape)
            };
            return Err(if allowance_mm > 0.0 {
                CamError::AllowanceCutsIntoKeptSide {
                    op: op.id.clone(),
                    shape: first.clone(),
                    other: Box::new(second.clone()),
                    diameter_mm: tool.diameter_mm,
                    allowance_mm,
                }
            } else {
                CamError::CutsIntoKeptSide {
                    op: op.id.clone(),
                    shape: first.clone(),
                    other: Box::new(second.clone()),
                    diameter_mm: tool.diameter_mm,
                }
            });
        }
    }
    Ok(())
}

/// How far from a point where two lines of one region meet their On line
/// cuts may come closer than the tool's diameter, as a multiple of the
/// diameter (#210): √2, so two straight lines that meet at 45° or more
/// pass, and a line that meets another and then runs alongside it does
/// not. S9 (`dc-gcode`'s material check) holds programs to the same rule.
pub(crate) const MEETING_REACH_DIAMETERS: f64 = std::f64::consts::SQRT_2;

/// #210: #199's On line band check for the open paths an On line Profile
/// cuts. An open path that a region's material holds
/// ([`region_holds_path`], #206) is compared with every line of that
/// region, its outline and its holes, and with every later open path the
/// same region holds. Where it comes closer than the tool's diameter `d`
/// (less [`EPS_MM`]) to one, the two cuts together cut through the part
/// between them, as in a ring narrower than the bit:
/// [`CamError::BandNarrowerThanTool`].
///
/// Lines that meet (touch or cross within `tolerance_mm`: a slot drawn
/// from the edge, a saw cut across the part, two slots joined at a corner
/// or crossing) are cut together on purpose, so the stretch of both within
/// [`MEETING_REACH_DIAMETERS`]` · d` of a meeting point is set aside
/// ([`distance_away_from_mm`]); the rest is held to `d`. `band_mm` is the
/// narrowest gap so measured. Open paths no region holds are D6's
/// ([`crate::overlap`]).
fn check_open_bands(
    op: &ProfileOp,
    tool: &Tool,
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
    open: &[(ShapeRef, Polyline)],
    tolerance_mm: f64,
) -> Result<(), CamError> {
    let diameter_mm = tool.diameter_mm;
    let reach_mm = MEETING_REACH_DIAMETERS * diameter_mm;
    let tolerance_mm = tolerance_mm.max(EPS_MM);
    let held: Vec<Option<usize>> = open
        .iter()
        .map(|(_, path)| regions.iter().position(|r| region_holds_path(r, path)))
        .collect();
    let drawn = |s: &ShapeRef| (op.targets.iter().position(|t| *t == s.instance), s.profile);
    for (index, ((shape, path), region_index)) in open.iter().zip(&held).enumerate() {
        let Some(region) = region_index.and_then(|i| regions.get(i)) else {
            continue;
        };
        let lines = region_shapes(region, shapes).map(|(c, shape)| (shape, c.polyline()));
        let later = open
            .iter()
            .zip(&held)
            .skip(index + 1)
            .filter(|(_, h)| **h == *region_index)
            .map(|((s, p), _)| (s, p));
        for (other, line) in lines.chain(later) {
            if !within(path, line, diameter_mm) {
                continue;
            }
            let meetings = meeting_points(path, line, tolerance_mm);
            let band_mm = distance_away_from_mm(path, line, &meetings, reach_mm);
            if band_mm >= diameter_mm - EPS_MM {
                continue;
            }
            let (first, second) = if drawn(shape) <= drawn(other) {
                (shape, other)
            } else {
                (other, shape)
            };
            return Err(CamError::BandNarrowerThanTool {
                op: op.id.clone(),
                shape: first.clone(),
                other: Box::new(second.clone()),
                band_mm,
                diameter_mm,
            });
        }
        // #221: and with itself. The region holds all of it, so the part
        // lies on both sides of every stretch.
        check_self_band(&op.id, shape, path, tool, tolerance_mm, |_, _| true)?;
    }
    Ok(())
}

/// The block that cuts one lap of a tool-centre contour at pass `pass`:
/// a straight plunge to `start` (into the lap's start Z, which a tab span
/// over the path start raises above `z_mm`), then the lap. The descent
/// starts at safe Z on the first pass and after it at the previous pass Z
/// or the lap's start Z, whichever is higher (#337: on a tab, never below
/// its height).
/// It frees material on the final pass when that reaches the stock bottom.
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the lap; a struct would only rename them"
)]
fn contour_block(
    shape: &ShapeRef,
    pass: usize,
    z_mm: f64,
    start: Point,
    lap: crate::tabs::LapCuts,
    pass_z: &[f64],
    safe_z_mm: f64,
    rates: LapRates,
) -> CutBlock {
    let descent = if (lap.start_z_mm - z_mm).abs() > EPS_MM {
        vec![Move::linear(
            Point3::new(start.x, start.y, lap.start_z_mm),
            rates.plunge_mm_min,
        )]
    } else {
        Vec::new()
    };
    let exit = lap.moves.last().map_or(start, |m| m.to.xy());
    // #337: a lap that starts on a tab starts at the tab's height. The
    // pass before left the tab there, and a lap that also started on it
    // ended there, so the descent never goes below that height at the
    // start: it would plunge into the tab. The lap comes down off the tab
    // at the tab's end, as every lap does.
    let descent_from_z_mm = pass
        .checked_sub(1)
        .and_then(|k| pass_z.get(k).copied())
        .map_or(safe_z_mm, |above| above.max(lap.start_z_mm));
    CutBlock {
        shape: shape.clone(),
        pass,
        z_mm,
        descent_from: start,
        descent_from_z_mm,
        descent,
        entry: start,
        cuts: lap.moves,
        exit,
        frees: pass + 1 == pass_z.len() && z_mm <= EPS_MM,
        cleared: None,
        group: 0,
    }
}

/// A path's lead (`docs/specs/phase-2/design.md`, "Algorithms → Lead-in
/// and lead-out"): the arc in, the re-seamed contour, the overcut and the
/// arc out, the same on every pass that cuts it.
struct PathLead {
    lead_in: LeadIn,
    lead_out: LeadOut,
    /// Planned in the domain less the tabs, so the passes below the tab
    /// top cut it too; otherwise only the passes above.
    below_tabs: bool,
}

/// What [`plan_leads`] needs from the generator.
struct LeadSetup<'a, 'b> {
    op: &'a ProfileOp,
    lead: &'a Lead,
    paths: &'a [(Polyline, &'b ShapeRef)],
    /// Each path's waste domains; empty for On line, which has none.
    domains: &'a [PathDomain],
    diameter_mm: f64,
    tolerance_mm: f64,
    any_above_tabs: bool,
    any_below_tabs: bool,
    /// The paths whose start is set (#285): a lead arrives there or not
    /// at all.
    fixed: &'a [bool],
}

/// Each path's lead, in order, or `None` where none fits (or On line).
/// A path with an explicit start (#285) is led in only at its start,
/// with the radius and sweep ladders as elsewhere.
/// A lead that fits the domain less the tabs fits the whole domain, since
/// one holds the other, so it is planned there first and then serves
/// every pass; failing that, a lead in the whole domain serves the passes
/// above the tab top. Arrivals rank travel from the previous path's exit
/// in drawing order (the sequencer orders blocks after they are built).
/// Warns through `warnings`; fails with [`CamError::OffMaterial`] if a
/// lead arc leaves its domain (a defect).
fn plan_leads(
    setup: &LeadSetup<'_, '_>,
    warnings: &mut LeadWarnings,
) -> Result<Vec<Option<PathLead>>, CamError> {
    let mut out = Vec::with_capacity(setup.paths.len());
    let mut from: Option<Point> = None;
    for (i, (path, shape)) in setup.paths.iter().enumerate() {
        let Some(domain) = setup.domains.get(i) else {
            out.push(None);
            continue;
        };
        let d = setup.diameter_mm;
        let at_start = setup.fixed.get(i).copied().unwrap_or(false);
        let plan_in = |space: &Space| {
            let leads = DomainSpace::new(&space.domain, d, setup.tolerance_mm);
            let lead_in = if at_start {
                lead::plan_lead_in_at_start(path, setup.lead, d, &leads)
            } else {
                lead::plan_lead_in(path, setup.lead, d, &leads, from)
            };
            lead_in.map(|lead_in| {
                let lead_out = lead::plan_lead_out(&lead_in.seam, setup.lead, d, &leads);
                (lead_in, lead_out)
            })
        };
        let tabbed = domain
            .tabbed
            .as_ref()
            .filter(|_| setup.any_below_tabs)
            .map(|space| (space, plan_in(space)));
        let planned = match tabbed {
            Some((space, Some(plan))) => Some((space, plan, true)),
            Some((_, None)) => {
                warnings.record_dropped(shape, DropReason::NoRoom);
                setup
                    .any_above_tabs
                    .then(|| plan_in(&domain.plain).map(|p| (&domain.plain, p, false)))
                    .flatten()
            }
            None => {
                let plan = plan_in(&domain.plain).map(|p| (&domain.plain, p, false));
                if plan.is_none() {
                    warnings.record_dropped(shape, DropReason::NoRoom);
                }
                plan
            }
        };
        let start = path.vertices().first().map(|v| v.point());
        let Some((space, (lead_in, lead_out), below_tabs)) = planned else {
            from = start;
            out.push(None);
            continue;
        };
        for arc in std::iter::once(&lead_in.arc).chain(&lead_out.arc) {
            warnings.record_arc(shape, setup.lead, arc);
            check_lead_arc(setup.op, shape, arc, space, d / 2.0, setup.tolerance_mm)?;
        }
        if setup.lead.lead_out && lead_out.arc.is_none() {
            warnings.record_dropped(shape, DropReason::Exit);
        }
        from = Some(match (&lead_out.arc, &lead_out.overcut) {
            (Some(arc), _) => arc.to,
            (None, Some(overcut)) => overcut
                .vertices()
                .last()
                .map_or(lead_in.arrival(), |v| v.point()),
            (None, None) => lead_in.arrival(),
        });
        out.push(Some(PathLead {
            lead_in,
            lead_out,
            below_tabs,
        }));
    }
    Ok(out)
}

/// S9 for a lead arc planned in `space`: every sample of its tool centre
/// lies in the domain (within the flatten tolerance and the least
/// boundary safety, as a descent's), or the generator has a defect and no
/// toolpath is produced ([`CamError::OffMaterial`]).
fn check_lead_arc(
    op: &ProfileOp,
    shape: &ShapeRef,
    arc: &LeadArc,
    space: &Space,
    radius_mm: f64,
    tolerance_mm: f64,
) -> Result<(), CamError> {
    let slack_mm = tolerance_mm + MIN_BOUNDARY_SAFETY_MM;
    if lead::lead_samples(arc)
        .iter()
        .all(|p| space.domain.clearance_mm(*p) >= -slack_mm)
    {
        return Ok(());
    }
    let mv = Move {
        kind: arc.kind(),
        to: Point3::new(arc.to.x, arc.to.y, 0.0),
        feed_mm_min: Some(1.0),
    };
    let start = Point3::new(arc.from.x, arc.from.y, 0.0);
    Err(off_material(
        op,
        shape,
        start,
        mv,
        space,
        radius_mm,
        tolerance_mm,
    ))
}

/// The block that cuts one pass of a path with its lead: the lead-in arc
/// from the staging point (the block's entry, where its descent ends) to
/// the arrival, the lap of the re-seamed contour, the overcut and the
/// lead-out arc, all at the cutting feed. The tabs, measured from the
/// path's own start, are moved with the seam so they stay where they are;
/// they raise the lap and the overcut, never a lead arc. A lead-out after
/// an overcut that ends raised (on a tab) is left off. `None` when the
/// lap would start on a tab, or the path has no vertices.
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the pass; a struct would only rename them"
)]
fn led_block(
    shape: &ShapeRef,
    pass: usize,
    z_mm: f64,
    led: &PathLead,
    tabs: Option<(&Tabs, &PathTabs)>,
    pass_z: &[f64],
    safe_z_mm: f64,
    rates: LapRates,
) -> Option<CutBlock> {
    let seam = &led.lead_in.seam;
    let reseamed = tabs.map(|(t, layout)| (t, layout.reseamed(led.lead_in.arrival_mm)));
    let tabs = reseamed.as_ref().map(|(t, layout)| (*t, layout));
    let lap = lap_cuts(seam, z_mm, tabs, rates)?;
    if (lap.start_z_mm - z_mm).abs() > EPS_MM {
        return None;
    }
    let at = |p: Point, z: f64| Point3::new(p.x, p.y, z);
    let arc_move = |arc: &LeadArc, z: f64| Move {
        kind: arc.kind(),
        to: at(arc.to, z),
        feed_mm_min: Some(rates.feed_mm_min),
    };
    let mut cuts = Vec::with_capacity(lap.moves.len() + 4);
    cuts.push(arc_move(&led.lead_in.arc, z_mm));
    cuts.extend(lap.moves);
    if let Some(overcut) = &led.lead_out.overcut
        && let Some(more) = lap_cuts(overcut, z_mm, tabs, rates)
    {
        cuts.extend(more.moves);
    }
    let end_z_mm = cuts.last().map_or(z_mm, |m| m.to.z);
    if let Some(arc) = &led.lead_out.arc
        && (end_z_mm - z_mm).abs() <= EPS_MM
    {
        cuts.push(arc_move(arc, z_mm));
    }
    let staging = led.lead_in.staging();
    let exit = cuts.last().map_or(staging, |m| m.to.xy());
    let descent_from_z_mm = pass
        .checked_sub(1)
        .and_then(|k| pass_z.get(k).copied())
        .unwrap_or(safe_z_mm);
    Some(CutBlock {
        shape: shape.clone(),
        pass,
        z_mm,
        descent_from: staging,
        descent_from_z_mm,
        descent: Vec::new(),
        entry: staging,
        cuts,
        exit,
        frees: pass + 1 == pass_z.len() && z_mm <= EPS_MM,
        cleared: None,
        group: 0,
    })
}

/// The `(inner, encloser)` pairs `sequence` needs (invariant O1,
/// `docs/research/toolpath-ordering.md`): every hole before its region's
/// outer, and every region of depth 2 or more before the hole it sits in,
/// so a part inside another part's hole is cut out before that hole frees
/// the waste around it.
pub(crate) fn containment_pairs(
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
) -> Vec<(ShapeRef, ShapeRef)> {
    let mut pairs = Vec::new();
    for region in regions {
        let Some(outer) = outer_shape(region, shapes) else {
            continue;
        };
        for (_, hole) in hole_shapes(region, shapes) {
            pairs.push((hole.clone(), outer.clone()));
        }
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let Some(probe) = region.outer().polyline().vertices().first() else {
            continue;
        };
        let enclosing_hole = regions
            .iter()
            .filter(|r| r.depth() == parent_depth)
            .flat_map(|r| hole_shapes(r, shapes))
            .find(|(hole, _)| contour_contains(hole, probe.point()));
        if let Some((_, hole)) = enclosing_hole {
            pairs.push((outer.clone(), hole.clone()));
        }
    }
    pairs
}

/// True when `p` lies inside or on `contour`, whichever way it winds.
pub(crate) fn contour_contains(contour: &Contour, p: Point) -> bool {
    nest(vec![contour.clone()])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
        .is_some_and(|region| region.contains_point(p))
}

/// How the tabs of a tabbed Profile leave one outline it cuts
/// ([`cut_outlines`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold {
    /// Cut with no tab, so cut through the stock it comes loose (#109).
    Free,
    /// Cut with tabs, so cut through the stock only they hold it (#269).
    Tabs,
}

/// Every closed target outline that the Profile `op` cuts, in drawing
/// order, with how its tabs leave it: [`Hold::Free`] for those it cuts
/// with no tab (#109), such as its holes when it places no tab on them,
/// and [`Hold::Tabs`] for those with a tab on some path (#105). A shape
/// the tool does not fit is not cut, so it is left out. `None` when `op`
/// cuts no tab at all, and when its toolpath cannot be generated:
/// [`crate::order::check`] only warns.
pub(crate) fn cut_outlines(project: &Project, op: &ProfileOp) -> Option<Vec<(ShapeRef, Hold)>> {
    let OpTabs { paths, plan, .. } = op_tabs(project, op).ok()?;
    plan.tabs.as_ref()?;
    let mut cut: Vec<(ShapeRef, Hold)> = Vec::new();
    for ((path, shape), layout) in paths.iter().zip(&plan.layouts) {
        if !path.is_closed() {
            continue;
        }
        let tabbed = layout.as_ref().is_some_and(|(l, _)| !l.is_untabbed());
        match cut.iter_mut().find(|(s, _)| s == shape) {
            Some((_, hold)) if tabbed => *hold = Hold::Tabs,
            Some(_) => {}
            None => cut.push((shape.clone(), if tabbed { Hold::Tabs } else { Hold::Free })),
        }
    }
    Some(cut)
}

/// The shape `region`'s outer came from, `region` being one that `nest`
/// returned for the contours of `shapes`, in order.
pub(crate) fn outer_shape<'s>(
    region: &Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> Option<&'s ShapeRef> {
    shapes.get(region.outer_index()).map(|(s, _)| s)
}

/// Each hole of `region` with the shape it came from, in hole order;
/// `region` as for [`outer_shape`].
pub(crate) fn hole_shapes<'r, 's>(
    region: &'r Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> impl Iterator<Item = (&'r Contour, &'s ShapeRef)> {
    region
        .holes()
        .iter()
        .zip(region.hole_indices())
        .filter_map(|(hole, &i)| shapes.get(i).map(|(s, _)| (hole, s)))
}

/// The outer of `region` then its holes, each with the shape it came
/// from; `region` as for [`outer_shape`].
pub(crate) fn region_shapes<'r, 's>(
    region: &'r Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> impl Iterator<Item = (&'r Contour, &'s ShapeRef)> {
    outer_shape(region, shapes)
        .map(|s| (region.outer(), s))
        .into_iter()
        .chain(hole_shapes(region, shapes))
}

/// The shape `contour`, as `nest` returned it, came from.
pub(crate) fn shape_of<'a>(
    shapes: &'a [(ShapeRef, Contour)],
    contour: &Contour,
) -> Option<&'a ShapeRef> {
    shapes
        .iter()
        .find(|(_, c)| same_contour(c, contour))
        .map(|(s, _)| s)
}

/// Places every profile of every instance in `targets` (operation `op`),
/// in target then profile order. Open paths are skipped with a warning.
pub(crate) fn resolve_targets(
    project: &Project,
    op: &OperationId,
    targets: &[InstanceId],
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Contour)>, CamError> {
    resolve_shapes(project, op, targets, None, warnings)
}

/// [`resolve_targets`], with the open paths placed into `open` instead
/// when it is given (#175: an On line Profile cuts them). An open path
/// with no length has nothing to cut and is skipped with
/// [`Warning::OpenPathSkipped`] ([`OpenPathReason::NoLength`]).
fn resolve_shapes(
    project: &Project,
    op: &OperationId,
    targets: &[InstanceId],
    mut open: Option<&mut Vec<(ShapeRef, Polyline)>>,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Contour)>, CamError> {
    let mut shapes = Vec::new();
    for instance_id in targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op.clone(),
                instance: instance_id.clone(),
            })?;
        let definition = project
            .definitions
            .get(&instance.definition)
            .ok_or_else(|| CamError::UnknownDefinition {
                instance: instance.id.clone(),
                definition: instance.definition.clone(),
            })?;
        for (profile, polyline) in definition.profiles.iter().enumerate() {
            let shape = ShapeRef {
                instance: instance.id.clone(),
                instance_name: instance.name.clone(),
                profile,
            };
            if !polyline.is_closed() {
                let Some(open) = open.as_deref_mut() else {
                    warnings.push(Warning::OpenPathSkipped {
                        shape,
                        reason: OpenPathReason::NoSide,
                    });
                    continue;
                };
                let placed = instance
                    .transform
                    .apply_polyline(polyline)
                    .map_err(|source| CamError::InvalidTarget {
                        shape: shape.clone(),
                        source,
                    })?;
                if placed.length_mm() <= EPS_MM {
                    warnings.push(Warning::OpenPathSkipped {
                        shape,
                        reason: OpenPathReason::NoLength,
                    });
                    continue;
                }
                open.push((shape, placed));
                continue;
            }
            let contour = instance
                .transform
                .apply_polyline(polyline)
                .and_then(Contour::new)
                .map_err(|source| CamError::InvalidTarget {
                    shape: shape.clone(),
                    source,
                })?;
            shapes.push((shape, contour));
        }
    }
    Ok(shapes)
}

/// Maps `nest`'s input indices for crossing contours back to the shapes the
/// user drew.
pub(crate) fn crossing_error(error: GeometryError, shapes: &[(ShapeRef, Contour)]) -> CamError {
    if let GeometryError::ContoursCross { first, second } = error
        && let (Some((a, _)), Some((b, _))) = (shapes.get(first), shapes.get(second))
    {
        return CamError::TargetsCross {
            first: a.clone(),
            second: b.clone(),
        };
    }
    CamError::Geometry(error)
}

/// True when `nested` is `input` as `nest` returns it: unchanged, or
/// reversed to fix its winding. Both are exact copies of the same vertices,
/// so exact equality is the right test.
pub(crate) fn same_contour(input: &Contour, nested: &Contour) -> bool {
    input == nested || &input.reversed() == nested
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{FeedField, ShapeRef};
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point, Polyline, Transform, Vertex, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        CornerRelief, DefinitionId, Entry, FeatureDefinition, FeatureInstance, InstanceId,
        LastPass, MachiningOrder, Operation, OperationId, Side, Stock, TabPlacement, TabShape,
        TabSpec, ToolId,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::{PI, TAU};

    const TOL: f64 = 1e-6;
    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    /// Depth 6 in steps of 2 on 6 mm stock.
    const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];

    fn rect_100x50() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)
    }

    /// A 300 × 200 × 6 mm project with one definition made of `profiles`,
    /// placed once per `(instance id, transform)`.
    fn project(profiles: Vec<Polyline>, placements: &[(&str, Transform)]) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Shape".into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = placements
            .iter()
            .map(|&(id, transform)| FeatureInstance {
                id: InstanceId::new(id),
                definition: DefinitionId::new("d1"),
                transform,
                name: format!("Shape {id}"),
                visible: true,
                locked: false,
                z_top_mm: Some(THICKNESS_MM),
                z_bottom_mm: Some(0.0),
            })
            .collect();
        p.operations.clear();
        p
    }

    fn at_20_20() -> Transform {
        Transform::translate(20.0, 20.0)
    }

    fn rect_project() -> Project {
        project(vec![rect_100x50()], &[("i1", at_20_20())])
    }

    /// The 100 × 50 rect with a 20 mm circular hole in its middle.
    fn ring_project(hole_radius_mm: f64) -> Project {
        project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), hole_radius_mm),
            ],
            &[("i1", at_20_20())],
        )
    }

    /// Profile of every instance in `project`, through the stock in 2 mm
    /// steps (the tool's step-down), no tabs.
    fn profile_op(project: &Project, side: Side, direction: CutDirection) -> ProfileOp {
        ProfileOp {
            id: OperationId::new("o1"),
            name: "Profile".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(THICKNESS_MM),
            step_down_mm: None,
            direction,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: None,
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: CornerRelief::None,
            last_pass: None,
            start: dc_project::StartAt::Auto,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_profile(project, op).unwrap();
        assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
        (toolpath, warnings)
    }

    /// One lap of a contour at one depth: the cutting moves between a
    /// descent and the next non-cutting move.
    #[derive(Debug)]
    struct Lap {
        z_mm: f64,
        /// Start point, then the end of every move.
        points: Vec<Point>,
        /// Signed area enclosed, arcs included; negative is clockwise.
        area_mm2: f64,
        moves: Vec<Move>,
    }

    impl Lap {
        fn x_extent_mm(&self) -> f64 {
            let xs = self.points.iter().map(|p| p.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        }

        fn y_extent_mm(&self) -> f64 {
            let ys = self.points.iter().map(|p| p.y);
            ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min)
        }
    }

    /// Signed sweep, radians, of the arc from `a` to `b` about `c`.
    fn sweep_rad(a: Point, b: Point, c: Point, ccw: bool) -> f64 {
        let (u, v) = (a - c, b - c);
        let raw = u.cross(v).atan2(u.dot(v));
        if ccw {
            if raw > 0.0 { raw } else { raw + TAU }
        } else if raw < 0.0 {
            raw
        } else {
            raw - TAU
        }
    }

    /// Splits the toolpath into laps: runs of XY cutting moves at one Z.
    fn laps(toolpath: &Toolpath) -> Vec<Lap> {
        let mut laps: Vec<Lap> = Vec::new();
        let mut current: Option<Lap> = None;
        let mut pos = match toolpath.moves.first() {
            Some(m) => m.to,
            None => return laps,
        };
        for mv in &toolpath.moves {
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
            let cutting = !mv.is_rapid() && level && xy;
            if cutting {
                let (a, b) = (pos.xy(), mv.to.xy());
                let lap = current.get_or_insert_with(|| Lap {
                    z_mm: mv.to.z,
                    points: vec![a],
                    area_mm2: 0.0,
                    moves: Vec::new(),
                });
                lap.area_mm2 += a.x * b.y - b.x * a.y;
                let arc = match mv.kind {
                    MoveKind::ArcCcw { centre } => Some((centre, true)),
                    MoveKind::ArcCw { centre } => Some((centre, false)),
                    _ => None,
                };
                if let Some((c, ccw)) = arc {
                    let r = a.distance_to(c);
                    let theta = sweep_rad(a, b, c, ccw);
                    // Twice the circular segment between chord and arc.
                    lap.area_mm2 += r * r * (theta - theta.sin());
                }
                lap.points.push(b);
                lap.moves.push(*mv);
            } else if let Some(mut lap) = current.take() {
                lap.area_mm2 /= 2.0;
                laps.push(lap);
            }
            pos = mv.to;
        }
        if let Some(mut lap) = current.take() {
            lap.area_mm2 /= 2.0;
            laps.push(lap);
        }
        laps
    }

    fn laps_at(toolpath: &Toolpath, z_mm: f64) -> Vec<Lap> {
        laps(toolpath)
            .into_iter()
            .filter(|l| (l.z_mm - z_mm).abs() <= EPS_MM)
            .collect()
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code()).collect()
    }

    #[test]
    fn direction_rule() {
        use CutDirection::{Climb, Conventional};
        assert!(!traversal_is_ccw(Wall::Outside, Climb));
        assert!(traversal_is_ccw(Wall::Outside, Conventional));
        assert!(traversal_is_ccw(Wall::Inside, Climb));
        assert!(!traversal_is_ccw(Wall::Inside, Conventional));
    }

    #[test]
    fn outside_rect_offsets_outward() {
        let p = rect_project();
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = laps(&tp);
        assert_eq!(laps.len(), PASS_Z_MM.len());
        for (lap, z) in laps.iter().zip(PASS_Z_MM) {
            assert_approx_eq!(lap.z_mm, z, TOL);
            assert_approx_eq!(lap.x_extent_mm(), 106.35, TOL);
            assert_approx_eq!(lap.y_extent_mm(), 56.35, TOL);
            let min_x = lap.points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
            assert_approx_eq!(min_x, 20.0 - 3.175, TOL);
        }
    }

    /// #227, S2: with `entry: plunge` the tool rapids straight down from
    /// safe Z to 1 mm above the stock top and plunges at the plunge rate
    /// from there, not from safe Z through the air. A later pass after a
    /// lift (the rect's next pass follows at its start, so none here) would
    /// stop 1 mm above the floor already cut.
    #[test]
    fn plunge_entry_rapids_to_just_above_the_stock() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let first = tp.moves[0];
        assert!(first.is_rapid(), "{first:?}");
        assert_approx_eq!(first.to.z, SAFE_Z_MM, TOL);
        let (rapid, plunge) = (tp.moves[1], tp.moves[2]);
        assert_eq!(
            rapid,
            Move::rapid(Point3::new(first.to.x, first.to.y, THICKNESS_MM + 1.0)),
            "{:?}",
            &tp.moves[..4]
        );
        assert_eq!(
            plunge,
            Move::linear(
                Point3::new(first.to.x, first.to.y, PASS_Z_MM[0]),
                PLUNGE_MM_MIN
            )
        );
    }

    #[test]
    fn inside_rect_offsets_inward() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
        let laps = laps(&tp);
        assert_eq!(laps.len(), PASS_Z_MM.len());
        for lap in &laps {
            assert_approx_eq!(lap.x_extent_mm(), 93.65, TOL);
            assert_approx_eq!(lap.y_extent_mm(), 43.65, TOL);
        }
    }

    #[test]
    fn on_line_follows_the_shape() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
        for lap in &laps(&tp) {
            assert_approx_eq!(lap.x_extent_mm(), 100.0, TOL);
            assert_approx_eq!(lap.y_extent_mm(), 50.0, TOL);
            assert!(lap.area_mm2 < 0.0, "outer on the line, climb: clockwise");
        }
    }

    /// #62: the outside cut of the rect at (20, 20) sweeps X 13.65..126.35
    /// with the 6.35 mm tool. On 120 mm wide stock that is 6.35 mm past
    /// the right edge; the inside cut, which sweeps to 120 exactly, fits.
    #[test]
    fn toolpath_off_stock_is_an_error() {
        let mut p = rect_project();
        p.stock.width_mm = 120.0;
        let err =
            generate_profile(&p, &profile_op(&p, Side::Outside, CutDirection::Climb)).unwrap_err();
        let CamError::OffStock { op, overhang } = &err else {
            panic!("expected OffStock, got {err:?}");
        };
        assert_eq!(op.as_str(), "o1");
        assert_approx_eq!(overhang.max_x_mm, 6.35, TOL);
        assert_approx_eq!(overhang.min_x_mm, 0.0, TOL);
        assert_approx_eq!(overhang.max_y_mm, 0.0, TOL);
        assert_eq!(
            err.to_string(),
            "operation `o1` cuts off the stock: 6.350 mm past the +X (right) edge"
        );
        generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
    }

    #[test]
    fn ring_cuts_outer_outside_and_hole_inside() {
        let p = ring_project(10.0);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            let laps = laps_at(&tp, z);
            assert_eq!(laps.len(), 2, "two contours at Z {z}");
            let hole = &laps[0];
            let outer = &laps[1];
            assert_approx_eq!(outer.x_extent_mm(), 106.35, TOL);
            assert_approx_eq!(outer.y_extent_mm(), 56.35, TOL);
            assert_approx_eq!(hole.x_extent_mm(), 20.0 - 6.35, TOL);
            assert_approx_eq!(hole.area_mm2.abs(), PI * 6.825 * 6.825, 1e-6);
        }
    }

    #[test]
    fn holes_are_cut_before_their_outer() {
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let laps = laps(&tp);
        assert_eq!(laps.len(), 6);
        // Every pass of the hole, then every pass of the outer.
        for lap in &laps[..3] {
            assert!(lap.x_extent_mm() < 20.0, "hole first: {lap:?}");
        }
        for lap in &laps[3..] {
            assert!(lap.x_extent_mm() > 100.0, "outer last: {lap:?}");
        }
    }

    /// Names each lap, in cut order, by the `(name, x extent mm)` its
    /// x extent is nearest to. Each contour is cut at every pass before the
    /// next contour starts, so the result is the contour cut order with one
    /// entry per pass.
    fn lap_names(toolpath: &Toolpath, contours: &[(&'static str, f64)]) -> Vec<&'static str> {
        laps(toolpath)
            .iter()
            .map(|lap| {
                let off = |w: f64| (lap.x_extent_mm() - w).abs();
                let (name, width_mm) = contours
                    .iter()
                    .copied()
                    .min_by(|a, b| off(a.1).total_cmp(&off(b.1)))
                    .unwrap();
                assert!(off(width_mm) < TOL, "lap {lap:?} matches no contour");
                name
            })
            .collect()
    }

    /// Each name repeated once per pass, for comparing with [`lap_names`].
    fn every_pass(names: &[&'static str]) -> Vec<&'static str> {
        names
            .iter()
            .flat_map(|&n| std::iter::repeat_n(n, PASS_Z_MM.len()))
            .collect()
    }

    /// O1 (`docs/research/toolpath-ordering.md`): a 200 × 150 frame (outline
    /// A, 160 × 110 hole B) with a 60 mm coaster C inside the hole. Whatever
    /// the drawing order, the coaster is cut first, then the hole that frees
    /// the waste around it, then the frame's outline.
    #[test]
    fn island_is_cut_before_the_hole_around_it() {
        let a = Polyline::rect(Point::new(0.0, 0.0), 200.0, 150.0);
        let b = Polyline::rect(Point::new(20.0, 20.0), 160.0, 110.0);
        let c = Polyline::circle(Point::new(100.0, 75.0), 30.0);
        // Outside: A and C grow by the radius, the hole B shrinks.
        let widths = [("A", 206.35), ("B", 153.65), ("C", 66.35)];
        let orders = [
            vec![a.clone(), b.clone(), c.clone()],
            vec![c.clone(), b.clone(), a.clone()],
            vec![b, c, a],
        ];
        for profiles in orders {
            let p = project(profiles, &[("i1", at_20_20())]);
            let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(lap_names(&tp, &widths), every_pass(&["C", "B", "A"]));
        }
    }

    /// Four nested contours, drawn outermost first: outer, hole, island,
    /// hole in the island. The innermost is cut first, and each hole still
    /// comes before its own outline.
    #[test]
    fn four_levels_cut_innermost_first() {
        let p = project(
            vec![
                Polyline::rect(Point::new(0.0, 0.0), 200.0, 150.0),
                Polyline::rect(Point::new(20.0, 20.0), 160.0, 110.0),
                Polyline::rect(Point::new(40.0, 40.0), 120.0, 70.0),
                Polyline::rect(Point::new(60.0, 60.0), 80.0, 30.0),
            ],
            &[("i1", at_20_20())],
        );
        let widths = [
            ("outer", 206.35),
            ("hole", 153.65),
            ("island", 126.35),
            ("island hole", 73.65),
        ];
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            lap_names(&tp, &widths),
            every_pass(&["island hole", "island", "hole", "outer"])
        );
    }

    /// Two parts side by side, neither inside the other, are cut in the
    /// order they were drawn, either way round.
    #[test]
    fn apart_regions_keep_drawing_order() {
        let small = Polyline::rect(Point::new(0.0, 0.0), 40.0, 30.0);
        let big = Polyline::rect(Point::new(100.0, 0.0), 60.0, 30.0);
        let widths = [("small", 46.35), ("big", 66.35)];
        for (profiles, expected) in [
            (vec![small.clone(), big.clone()], ["small", "big"]),
            (vec![big, small], ["big", "small"]),
        ] {
            let p = project(profiles, &[("i1", at_20_20())]);
            let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
            assert_eq!(lap_names(&tp, &widths), every_pass(&expected));
        }
    }

    #[test]
    fn climb_outside_is_clockwise() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        for lap in &laps(&tp) {
            assert!(lap.area_mm2 < 0.0, "{lap:?}");
            assert_approx_eq!(
                lap.area_mm2,
                -(106.35 * 56.35 - (4.0 - PI) * 3.175 * 3.175),
                1e-6
            );
        }
    }

    #[test]
    fn conventional_outside_is_counter_clockwise() {
        let p = rect_project();
        let (tp, _) = generate(
            &p,
            &profile_op(&p, Side::Outside, CutDirection::Conventional),
        );
        for lap in &laps(&tp) {
            assert!(lap.area_mm2 > 0.0, "{lap:?}");
        }
    }

    #[test]
    fn climb_inside_is_counter_clockwise() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
        for lap in &laps(&tp) {
            assert!(lap.area_mm2 > 0.0, "{lap:?}");
        }
    }

    #[test]
    fn climb_round_a_hole_is_counter_clockwise() {
        // Outside mode cuts the hole's wall from inside the hole.
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let laps = laps_at(&tp, PASS_Z_MM[0]);
        assert!(laps[0].area_mm2 > 0.0, "hole: {:?}", laps[0]);
        assert!(laps[1].area_mm2 < 0.0, "outer: {:?}", laps[1]);
    }

    #[test]
    fn inside_mode_cuts_around_the_hole_clockwise_when_climbing() {
        // Inside mode cuts the hole's wall from outside the hole: an
        // outside wall, so climb is clockwise, and the hole grows by r.
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
        let laps = laps_at(&tp, PASS_Z_MM[0]);
        assert_eq!(laps.len(), 2);
        assert_approx_eq!(laps[0].x_extent_mm(), 20.0 + 6.35, TOL);
        assert!(laps[0].area_mm2 < 0.0, "hole: {:?}", laps[0]);
        assert!(laps[1].area_mm2 > 0.0, "outer: {:?}", laps[1]);
    }

    #[test]
    fn arcs_turn_with_the_traversal() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        for lap in &laps(&tp) {
            let cw = lap
                .moves
                .iter()
                .filter(|m| matches!(m.kind, MoveKind::ArcCw { .. }))
                .count();
            let ccw = lap
                .moves
                .iter()
                .filter(|m| matches!(m.kind, MoveKind::ArcCcw { .. }))
                .count();
            assert_eq!((cw, ccw), (4, 0), "round corners of a clockwise lap");
        }
        let (tp, _) = generate(
            &p,
            &profile_op(&p, Side::Outside, CutDirection::Conventional),
        );
        let first = &laps(&tp)[0];
        assert!(
            first
                .moves
                .iter()
                .all(|m| !matches!(m.kind, MoveKind::ArcCw { .. }))
        );
    }

    #[test]
    fn open_path_is_skipped_with_warning() {
        let open = Polyline::new(
            vec![Vertex::line(0.0, 60.0), Vertex::line(100.0, 60.0)],
            false,
        );
        let p = project(vec![rect_100x50(), open], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
        let Warning::OpenPathSkipped { shape, reason } = &warnings[0] else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 1));
        assert_eq!(*reason, OpenPathReason::NoSide);
        // #175: the message says what does cut it.
        let message = warnings[0].to_string();
        assert!(
            message.contains("On line") && message.contains("Engrave"),
            "{message}"
        );
        let laps = laps(&tp);
        assert_eq!(laps.len(), PASS_Z_MM.len(), "only the rect is cut");
        for lap in &laps {
            assert!(lap.points.iter().all(|p| p.y < 20.0 + 60.0 - 1.0));
        }
    }

    #[test]
    fn tool_larger_than_hole_warns_and_skips() {
        let p = ring_project(2.0);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::ToolTooLarge]);
        let Warning::ToolTooLarge { shape, tool, .. } = &warnings[0] else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!((shape.instance.as_str(), shape.profile), ("i1", 1));
        assert_eq!(tool, "6.35 mm upcut");
        for z in PASS_Z_MM {
            let laps = laps_at(&tp, z);
            assert_eq!(laps.len(), 1, "only the outer at Z {z}");
            assert_approx_eq!(laps[0].x_extent_mm(), 106.35, TOL);
        }
    }

    /// #192: an Inside Profile of the 100 × 50 rect with a radius-22
    /// island leaves a 3 mm band above and below the island, narrower than
    /// the 6.35 mm bit. The island's lap would run 3.175 mm outside it, to
    /// Y 19.825, past the rect's bottom edge at Y 20, and gouge the part
    /// around the ring. No toolpath is produced: the error names the
    /// operation, both shapes, the band and the tool.
    #[test]
    fn inside_cut_in_band_narrower_than_tool_is_refused() {
        let p = ring_project(22.0);
        let err = generate_profile(&p, &profile_op(&p, Side::Inside, CutDirection::Climb))
            .expect_err("a band narrower than the bit is refused");
        let CamError::BandNarrowerThanTool {
            op,
            shape,
            other,
            band_mm,
            diameter_mm,
        } = &err
        else {
            panic!("expected BandNarrowerThanTool, got {err:?}");
        };
        assert_eq!(op.as_str(), "o1");
        assert_eq!((shape.profile, other.profile), (0, 1), "outline first");
        assert_approx_eq!(*band_mm, 3.0, TOL);
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        let text = err.to_string();
        for needle in ["`o1`", "profile 0", "profile 1", "3.000 mm", "6.35 mm tool"] {
            assert!(text.contains(needle), "{needle:?} in {text}");
        }
    }

    /// #192: a band exactly as wide as the bit is cut: each lap's cutter
    /// just touches the other line. Outside, the same narrow ring is a
    /// thin wall around the hole, not a gouge, and is cut too.
    #[test]
    fn inside_band_as_wide_as_the_tool_is_cut() {
        let p = ring_project(25.0 - 6.35);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Inside, CutDirection::Climb));
        for z in PASS_Z_MM {
            assert_eq!(laps_at(&tp, z).len(), 2, "both contours at Z {z}");
        }
        let p = ring_project(22.0);
        generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
    }

    /// #192 and `ToolTooLarge`: Inside, a 6 mm square outline is too small
    /// for the 6.35 mm bit and is skipped, but the lap round the 2 mm
    /// island in it would run outside the outline, into the part. That is
    /// refused, not skipped: the island's path exists, and would gouge.
    #[test]
    fn inside_island_in_an_outline_too_small_for_the_tool_is_refused() {
        let p = project(
            vec![
                Polyline::rect(Point::new(0.0, 0.0), 6.0, 6.0),
                Polyline::rect(Point::new(2.0, 2.0), 2.0, 2.0),
            ],
            &[("i1", at_20_20())],
        );
        let err = generate_profile(&p, &profile_op(&p, Side::Inside, CutDirection::Climb))
            .expect_err("the island's lap would leave the outline");
        let text = err.to_string();
        assert!(
            text.contains("profile 0") && text.contains("profile 1"),
            "{text}"
        );
    }

    /// The band a refused Profile of `p` names: its two shapes' profiles
    /// and its width.
    fn refused_band(p: &Project, side: Side) -> (usize, usize, f64) {
        let err = generate_profile(p, &profile_op(p, side, CutDirection::Climb))
            .expect_err("a band narrower than the bit is refused");
        let CamError::BandNarrowerThanTool {
            op,
            shape,
            other,
            band_mm,
            diameter_mm,
        } = &err
        else {
            panic!("expected BandNarrowerThanTool, got {err:?}");
        };
        assert_eq!(op.as_str(), "o1");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        (shape.profile, other.profile, *band_mm)
    }

    /// #199: On line, the tool centre follows each line and the bit cuts
    /// `r` of the ring on both sides of it, so the outline's cut and the
    /// hole's together cut through a ring narrower than the bit: the part
    /// loses material it keeps, as in #192's Inside band. A 3 mm ring
    /// (closer than `r`, the issue's case) and a 4.5 mm one (wider than
    /// `r`, narrower than the 6.35 mm bit) are both refused, and so is a
    /// 2 mm web between two holes of one part.
    #[test]
    fn on_line_ring_narrower_than_tool_is_refused() {
        for band_mm in [3.0, 4.5] {
            let (shape, other, band) = refused_band(&ring_project(25.0 - band_mm), Side::OnLine);
            assert_eq!((shape, other), (0, 1), "outline first");
            assert_approx_eq!(band, band_mm, TOL);
        }
        let web = project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(30.0, 25.0), 10.0),
                Polyline::circle(Point::new(52.0, 25.0), 10.0),
            ],
            &[("i1", at_20_20())],
        );
        let (shape, other, band) = refused_band(&web, Side::OnLine);
        assert_eq!((shape, other), (1, 2), "the two holes, in drawing order");
        assert_approx_eq!(band, 2.0, TOL);
    }

    /// #199: an On line ring wider than the bit is cut as drawn, both
    /// lines at every pass, with no finding.
    #[test]
    fn on_line_ring_wider_than_the_tool_is_cut() {
        let p = ring_project(18.0);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            assert_eq!(laps_at(&tp, z).len(), 2, "both lines at Z {z}");
        }
    }

    #[test]
    fn every_cut_move_has_feed_and_every_rapid_has_none() {
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert!(!tp.moves.is_empty());
        for mv in &tp.moves {
            match mv.kind {
                MoveKind::Rapid => assert_eq!(mv.feed_mm_min, None, "{mv:?}"),
                _ => {
                    let feed = mv.feed_mm_min.unwrap_or_else(|| panic!("no feed: {mv:?}"));
                    assert!(
                        [FEED_MM_MIN, PLUNGE_MM_MIN]
                            .iter()
                            .any(|f| (feed - f).abs() <= EPS_MM),
                        "{mv:?}"
                    );
                }
            }
        }
        for lap in &laps(&tp) {
            for mv in &lap.moves {
                assert_approx_eq!(mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
            }
        }
    }

    #[test]
    fn first_move_is_rapid_at_safe_z() {
        let p = rect_project();
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let first = tp.moves[0];
        assert!(first.is_rapid(), "{first:?}");
        assert_approx_eq!(first.to.z, SAFE_Z_MM, TOL);
        let lap_start = laps(&tp)[0].points[0];
        assert_approx_eq!(first.to.x, lap_start.x, TOL);
        assert_approx_eq!(first.to.y, lap_start.y, TOL);
        let last = tp.moves.last().unwrap();
        assert!(last.is_rapid(), "{last:?}");
        assert_approx_eq!(last.to.z, SAFE_Z_MM, TOL);
    }

    #[test]
    fn descent_uses_plunge_feed() {
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let mut descents = 0;
        for pair in tp.moves.windows(2) {
            let (prev, mv) = (pair[0], pair[1]);
            if mv.is_rapid() && mv.to.z < prev.to.z - EPS_MM {
                // #227: from safe Z straight down to 1 mm above the stock.
                assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
                assert_approx_eq!(mv.to.z, THICKNESS_MM + 1.0, TOL);
                assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM, "vertical");
            } else if mv.to.z < prev.to.z - EPS_MM {
                descents += 1;
                assert_eq!(mv.kind, MoveKind::Linear, "{mv:?}");
                assert_approx_eq!(mv.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
                assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM, "vertical");
            }
        }
        assert_eq!(descents, 2 * PASS_Z_MM.len(), "one per pass per contour");
    }

    #[test]
    fn retracts_to_safe_z_between_contours() {
        let p = ring_project(10.0);
        let (tp, _) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let mut retracts = 0;
        for (i, pair) in tp.moves.windows(2).enumerate() {
            let (prev, mv) = (pair[0], pair[1]);
            if !mv.is_rapid() {
                continue;
            }
            if mv.to.z < prev.to.z - EPS_MM {
                // #227: down from safe Z to 1 mm above the stock, in Z
                // alone, before a plunge.
                assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
                assert_approx_eq!(mv.to.z, THICKNESS_MM + 1.0, TOL);
                assert!(prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM);
                continue;
            }
            assert_approx_eq!(mv.to.z, SAFE_Z_MM, TOL);
            if prev.to.z < SAFE_Z_MM - EPS_MM {
                retracts += 1;
                assert!(
                    prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM,
                    "retract {i} is vertical"
                );
            } else {
                assert_approx_eq!(prev.to.z, SAFE_Z_MM, TOL);
            }
        }
        assert_eq!(retracts, 2, "one retract after each contour");
        // Hole done, retract, rapid across at safe Z, outer starts; and a
        // rapid down before each contour's first plunge (#227).
        let rapids: Vec<&Move> = tp.moves.iter().filter(|m| m.is_rapid()).collect();
        assert_eq!(rapids.len(), 4 + 2);
    }

    #[test]
    fn crossing_targets_fail_naming_both_instances() {
        let p = project(
            vec![rect_100x50()],
            &[("i1", at_20_20()), ("i2", Transform::translate(70.0, 45.0))],
        );
        let err =
            generate_profile(&p, &profile_op(&p, Side::Outside, CutDirection::Climb)).unwrap_err();
        let CamError::TargetsCross { first, second } = &err else {
            panic!("expected TargetsCross, got {err:?}");
        };
        assert_eq!(
            first,
            &ShapeRef {
                instance: InstanceId::new("i1"),
                instance_name: "Shape i1".into(),
                profile: 0,
            }
        );
        assert_eq!(second.instance, InstanceId::new("i2"));
        let message = err.to_string();
        assert!(
            message.contains("i1") && message.contains("i2"),
            "{message}"
        );
    }

    #[test]
    fn duplicate_target_is_cut_once_with_warning() {
        let p = project(
            vec![rect_100x50()],
            &[("i1", at_20_20()), ("i2", at_20_20())],
        );
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
        let Warning::DuplicateShapeDropped {
            shape,
            duplicate_of,
        } = &warnings[0]
        else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!(shape.instance.as_str(), "i2");
        assert_eq!(duplicate_of.instance.as_str(), "i1");
        for z in PASS_Z_MM {
            assert_eq!(laps_at(&tp, z).len(), 1, "one contour at Z {z}");
        }
    }

    #[test]
    fn missing_feed_is_error_not_invented() {
        let mut p = rect_project();
        p.tools[0].feed_mm_min = None;
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingFeed {
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
                field: FeedField::Feed
            }
        );

        let mut p = rect_project();
        p.tools[0].plunge_mm_min = None;
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingFeed {
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
                field: FeedField::Plunge
            }
        );

        let mut p = rect_project();
        p.tools[0].feed_mm_min = Some(0.0);
        assert!(matches!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingFeed { .. }
        ));
    }

    /// #71: two tools may share a name, so an error about a tool carries
    /// its id as well as the name it prints.
    #[test]
    fn tool_errors_carry_the_tool_id() {
        let tool_id = |err: CamError| match err {
            CamError::MissingFeed { tool_id, .. }
            | CamError::CutterNotFlat { tool_id, .. }
            | CamError::MissingStepDown { tool_id, .. }
            | CamError::InvalidToolDiameter { tool_id, .. } => tool_id,
            other => panic!("a tool error, not {other:?}"),
        };
        let t1 = ToolId::new("t1");
        let op = profile_op(&rect_project(), Side::Outside, CutDirection::Climb);
        let fails = |change: &dyn Fn(&mut Project)| {
            let mut p = rect_project();
            change(&mut p);
            assert_eq!(p.tools[0].id, t1);
            generate_profile(&p, &op).unwrap_err()
        };
        let errors = [
            fails(&|p| p.tools[0].feed_mm_min = None),
            fails(&|p| p.tools[0].cutter = dc_project::ToolKind::BallNose),
            fails(&|p| p.tools[0].diameter_mm = 0.0),
            fails(&|p| p.tools[0].step_down_mm = None),
        ];
        for err in errors {
            assert_eq!(tool_id(err.clone()), t1, "{err:?}");
        }
    }

    /// #67: Profile offsets by half the diameter, right only for a flat end
    /// mill. Validation reports it first; the generator refuses too, so no
    /// caller gets a wrong-size part.
    #[test]
    fn v_bit_in_profile_is_an_error() {
        let mut p = rect_project();
        p.tools[0].cutter = dc_project::ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::CutterNotFlat {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
                cutter: "a V-bit",
            }
        );
        for cutter in [
            dc_project::ToolKind::BallNose,
            dc_project::ToolKind::Engraver,
        ] {
            p.tools[0].cutter = cutter;
            for side in [Side::Outside, Side::Inside] {
                let op = profile_op(&p, side, CutDirection::Climb);
                assert!(matches!(
                    generate_profile(&p, &op).unwrap_err(),
                    CamError::CutterNotFlat { .. }
                ));
            }
        }
    }

    /// #67 (maintainer, PR #96): on the line the tool centre follows the
    /// line, so any cutter cuts it as drawn.
    #[test]
    fn v_bit_on_line_is_allowed() {
        for cutter in [
            dc_project::ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            dc_project::ToolKind::BallNose,
            dc_project::ToolKind::Engraver,
        ] {
            let mut p = rect_project();
            p.tools[0].cutter = cutter;
            let op = profile_op(&p, Side::OnLine, CutDirection::Climb);
            let (_, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn missing_step_down_is_error() {
        let mut p = rect_project();
        p.tools[0].step_down_mm = None;
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingStepDown {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
                tool_id: ToolId::new("t1"),
            }
        );
    }

    #[test]
    fn operation_step_down_overrides_tool() {
        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.step_down_mm = Some(3.0);
        let (tp, _) = generate(&p, &op);
        let z: Vec<f64> = laps(&tp).iter().map(|l| l.z_mm).collect();
        assert_eq!(z.len(), 2);
        assert_approx_eq!(z[0], 3.0, TOL);
        assert_approx_eq!(z[1], 0.0, TOL);
    }

    #[test]
    fn through_cut_goes_exactly_to_the_allowance() {
        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.depth_mm = Some(THICKNESS_MM + 0.5);
        op.through_cut_allowance_mm = 0.5;
        let (tp, _) = generate(&p, &op);
        let lowest = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        assert_approx_eq!(lowest, -0.5, TOL);

        op.through_cut_allowance_mm = 0.0;
        assert!(matches!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::BelowMaterialBottom { .. }
        ));
    }

    /// #64: an unset thickness or depth fails generation; neither is
    /// defaulted. Validation reports both first, so this is the generator
    /// protecting itself.
    #[test]
    fn missing_thickness_or_depth_is_an_error() {
        let mut p = rect_project();
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        p.stock.thickness_mm = None;
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingThickness
        );

        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.depth_mm = None;
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingDepth {
                op: OperationId::new("o1")
            }
        );
    }

    #[test]
    fn unknown_ids_are_errors() {
        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.tool = ToolId::new("nope");
        assert!(matches!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::UnknownTool { .. }
        ));
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.targets.push(InstanceId::new("nope"));
        assert!(matches!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::UnknownInstance { .. }
        ));
    }

    #[test]
    fn non_positive_safe_z_is_error() {
        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        op.safe_z_mm = 0.0;
        assert!(matches!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::InvalidSafeZ { .. }
        ));
    }

    #[test]
    fn output_is_deterministic() {
        let p = ring_project(10.0);
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        assert_eq!(
            generate_profile(&p, &op).unwrap(),
            generate_profile(&p, &op).unwrap()
        );
    }

    /// O6 (acceptance test 31): a one-shape operation is the phase 1
    /// recipe move for move, whatever the machining order: a rapid to the
    /// start at safe Z, then per pass a plunge at the plunge rate and one
    /// lap at the feed rate, then a rapid up. The expected lap is built here
    /// from the offset contour directly, not through the sequencer.
    /// (changed in #227: the first plunge starts from a rapid straight down
    /// to 1 mm above the stock, not from safe Z.)
    #[test]
    fn one_shape_golden_is_byte_identical_after_sequence() {
        let p = rect_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        let (full, _) = generate(&p, &op);
        op.machining_order = MachiningOrder::PassFirst;
        let (pass, _) = generate(&p, &op);
        assert_eq!(full, pass);

        let placed = at_20_20().apply_polyline(&rect_100x50()).unwrap();
        let centre = offset(&Contour::new(placed).unwrap(), 3.175).remove(0);
        // Outside, climb: clockwise.
        let path = if centre.is_ccw() {
            centre.reversed().into_polyline()
        } else {
            centre.into_polyline()
        };
        let start = path.vertices()[0].point();
        let mut expected = vec![
            Move::rapid(Point3::new(start.x, start.y, SAFE_Z_MM)),
            Move::rapid(Point3::new(start.x, start.y, THICKNESS_MM + 1.0)),
        ];
        for z in PASS_Z_MM {
            expected.push(Move::linear(
                Point3::new(start.x, start.y, z),
                PLUNGE_MM_MIN,
            ));
            for segment in path.segments() {
                let kind = match segment.arc() {
                    Some(arc) if arc.sweep_rad > 0.0 => MoveKind::ArcCcw { centre: arc.centre },
                    Some(arc) => MoveKind::ArcCw { centre: arc.centre },
                    None => MoveKind::Linear,
                };
                expected.push(Move {
                    kind,
                    to: Point3::new(segment.end.x, segment.end.y, z),
                    feed_mm_min: Some(FEED_MM_MIN),
                });
            }
        }
        expected.push(Move::rapid(Point3::new(start.x, start.y, SAFE_Z_MM)));
        assert_eq!(full.moves, expected);
    }

    /// Three 20 mm squares drawn at X = 10, 50, 30, placed by three
    /// instances.
    fn three_squares_project() -> Project {
        project(
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)],
            &[
                ("i1", Transform::translate(10.0, 20.0)),
                ("i2", Transform::translate(50.0, 20.0)),
                ("i3", Transform::translate(30.0, 20.0)),
            ],
        )
    }

    /// The lowest X of each lap, in cut order.
    fn lap_min_x(toolpath: &Toolpath) -> Vec<f64> {
        laps(toolpath)
            .iter()
            .map(|lap| lap.points.iter().map(|p| p.x).fold(f64::MAX, f64::min))
            .collect()
    }

    /// Acceptance test 31 (D5): squares drawn at X = 10, 50, 30 are cut at
    /// 10, 30, 50 under full depth first. Under pass first each layer's
    /// walk starts where the previous one ended, so the second layer runs
    /// back 50, 30, 10.
    #[test]
    fn nearest_neighbour_visits_the_closest_part_next() {
        let p = three_squares_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        let r = 3.175;
        let at = |xs: &[f64]| -> Vec<f64> { xs.iter().map(|x| x - r).collect() };

        let (tp, _) = generate(&p, &op);
        let expected = at(&[10.0, 10.0, 10.0, 30.0, 30.0, 30.0, 50.0, 50.0, 50.0]);
        let got = lap_min_x(&tp);
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, e) in got.iter().zip(&expected) {
            assert_approx_eq!(*g, *e, TOL);
        }

        op.machining_order = MachiningOrder::PassFirst;
        let (tp, _) = generate(&p, &op);
        let expected = at(&[10.0, 30.0, 50.0, 50.0, 30.0, 10.0, 10.0, 30.0, 50.0]);
        let got = lap_min_x(&tp);
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, e) in got.iter().zip(&expected) {
            assert_approx_eq!(*g, *e, TOL);
        }
    }

    /// Acceptance test 30: three squares 6 mm deep in 2 mm passes. Under
    /// pass first the Z of successive plunges is 4, 4, 4, 2, 2, 2, 0, 0, 0;
    /// under full depth first it is 4, 2, 0 three times.
    #[test]
    fn pass_first_plunges_every_shape_before_stepping_down() {
        let p = three_squares_project();
        let plunge_zs = |tp: &Toolpath| -> Vec<f64> {
            tp.moves
                .windows(2)
                .filter(|pair| {
                    let (prev, mv) = (pair[0], pair[1]);
                    !mv.is_rapid()
                        && mv.to.z < prev.to.z - EPS_MM
                        && prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM
                })
                .map(|pair| pair[1].to.z)
                .collect()
        };
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        let (tp, _) = generate(&p, &op);
        let got = plunge_zs(&tp);
        let expected = [4.0, 2.0, 0.0, 4.0, 2.0, 0.0, 4.0, 2.0, 0.0];
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, e) in got.iter().zip(expected) {
            assert_approx_eq!(*g, e, TOL);
        }

        op.machining_order = MachiningOrder::PassFirst;
        let (tp, _) = generate(&p, &op);
        let got = plunge_zs(&tp);
        let expected = [4.0, 4.0, 4.0, 2.0, 2.0, 2.0, 0.0, 0.0, 0.0];
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, e) in got.iter().zip(expected) {
            assert_approx_eq!(*g, e, TOL);
        }
    }

    /// A tab warning's `contour` counts contours in cut order, which
    /// nearest neighbour sets: the square at X = 30 (instance `i3`) is cut
    /// second, so it is contour 1.
    #[test]
    fn tab_warnings_index_contours_in_cut_order() {
        let p = three_squares_project();
        let mut op = profile_op(&p, Side::Outside, CutDirection::Climb);
        // Four 10 mm tabs plus the 6.35 mm diameter cover 65 mm of a
        // 100 mm path: more than half, so every square warns.
        op.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 10.0,
            height_mm: 3.0,
            shape: TabShape::Rect,
            on_holes: false,
            avoid_corners: false,
        });
        let (_, warnings) = generate(&p, &op);
        let covered: Vec<(usize, &str)> = warnings
            .iter()
            .filter_map(|w| match w {
                Warning::TabsCoverMostOfContour {
                    contour,
                    shape: Some(shape),
                    ..
                } => Some((*contour, shape.instance.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(covered, [(0, "i1"), (1, "i3"), (2, "i2")]);
    }

    // ── #109: a part and its hole in one operation ──

    const STAR_CENTRE: Point = Point::new(50.0, 50.0);
    const STAR_HOLE_CENTRE: Point = Point::new(50.0, 70.0);

    /// The maintainer's manual case (`tests/manual/star_with_hole`, #109),
    /// redrawn with straight edges: a five-pointed star (points 40 mm and
    /// notches 22 mm from (50, 50), one point straight up) as instance
    /// `i1`, and a 7 mm circle 20 mm up its top arm as instance `i2`,
    /// leaving a 6.6 mm web. 100 × 100 × 12 mm stock.
    fn star_with_hole_project() -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 100.0,
            height_mm: 100.0,
            thickness_mm: Some(12.0),
        };
        let star = (0..10u32)
            .map(|k| {
                let angle_rad = PI / 2.0 + f64::from(k) * PI / 5.0;
                let r_mm = if k % 2 == 0 { 40.0 } else { 22.0 };
                Vertex::line(
                    STAR_CENTRE.x + r_mm * angle_rad.cos(),
                    STAR_CENTRE.y + r_mm * angle_rad.sin(),
                )
            })
            .collect();
        let definitions = [
            ("d1", "Star", Polyline::new(star, true)),
            ("d2", "Hole", Polyline::circle(STAR_HOLE_CENTRE, 3.5)),
        ];
        p.definitions = definitions
            .iter()
            .map(|(id, name, profile)| {
                let def = FeatureDefinition {
                    id: DefinitionId::new(*id),
                    name: (*name).into(),
                    profiles: vec![profile.clone()],
                };
                (def.id.clone(), def)
            })
            .collect();
        p.instances = [("i1", "d1", "star"), ("i2", "d2", "hole")]
            .iter()
            .map(|(id, def, name)| FeatureInstance {
                id: InstanceId::new(*id),
                definition: DefinitionId::new(*def),
                transform: Transform::IDENTITY,
                name: (*name).into(),
                visible: true,
                locked: false,
                z_top_mm: Some(12.0),
                z_bottom_mm: Some(0.0),
            })
            .collect();
        p.operations.clear();
        p
    }

    /// #109: one Outside Profile of the star and its hole, with four
    /// smooth 6 × 3 mm tabs and the 6.35 mm bit, 12 mm deep in 4 mm
    /// passes, as the manual file set it. Containment cuts the hole first; the
    /// hole, a slug of waste, gets no tabs and is cut free; only the
    /// star's last pass rises over tabs, four times. Before #109 this
    /// took two operations in the right order.
    #[test]
    fn star_and_its_hole_cut_in_one_operation() {
        let p = star_with_hole_project();
        let op = ProfileOp {
            targets: vec![InstanceId::new("i1"), InstanceId::new("i2")],
            depth_mm: Some(12.0),
            step_down_mm: Some(4.0),
            through_cut_allowance_mm: 0.01,
            tabs: Some(TabSpec {
                placement: TabPlacement::Count { count: 4 },
                width_mm: 6.0,
                height_mm: 3.0,
                shape: TabShape::Smooth,
                on_holes: false,
                avoid_corners: false,
            }),
            ..profile_op(&p, Side::Outside, CutDirection::Climb)
        };
        let (tp, warnings) = generate(&p, &op);
        assert!(
            !warnings.iter().any(|w| matches!(
                w.code(),
                WarningCode::TabsCoverMostOfContour | WarningCode::TabsOverlap
            )),
            "{warnings:?}"
        );

        // Every move along a path, with where it starts.
        let mut cuts: Vec<(Point3, Move)> = Vec::new();
        for pair in tp.moves.windows(2) {
            let (from, mv) = (pair[0].to, pair[1]);
            if !mv.is_rapid() && from.xy().distance_to(mv.to.xy()) > EPS_MM {
                cuts.push((from, mv));
            }
        }
        let in_hole = |(from, mv): &(Point3, Move)| {
            from.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
                && mv.to.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
        };
        let last_hole = cuts.iter().rposition(in_hole).expect("the hole is cut");
        let first_star = cuts
            .iter()
            .position(|c| !in_hole(c))
            .expect("the star is cut");
        assert!(last_hole < first_star, "the hole is cut before the star");

        // The depth is the stock: the last pass is at the stock bottom.
        let floor_mm = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        assert_approx_eq!(floor_mm, 0.0, TOL);
        let on_last_pass = |(from, mv): &(Point3, Move)| from.z < 3.5 && mv.to.z < 3.5;
        let level = |(from, mv): &(Point3, Move)| {
            (from.z - floor_mm).abs() <= TOL && (mv.to.z - floor_mm).abs() <= TOL
        };

        let hole_last: Vec<_> = cuts[..=last_hole]
            .iter()
            .filter(|c| on_last_pass(c))
            .collect();
        assert!(!hole_last.is_empty(), "the hole's last pass is cut");
        for cut in &hole_last {
            assert!(level(cut), "the hole has no tabs: {cut:?}");
        }

        let star_last: Vec<_> = cuts[first_star..]
            .iter()
            .filter(|c| on_last_pass(c))
            .collect();
        let raised: Vec<bool> = star_last.iter().map(|c| !level(c)).collect();
        let mut tabs = raised.windows(2).filter(|w| !w[0] && w[1]).count();
        if raised.first() == Some(&true) && raised.last() != Some(&true) {
            tabs += 1;
        }
        assert_eq!(tabs, 4, "the star rises over its four tabs");
        let top_mm = star_last
            .iter()
            .map(|(from, mv)| from.z.max(mv.to.z))
            .fold(f64::MIN, f64::max);
        assert_approx_eq!(top_mm, 3.0, TOL);
    }

    /// #105, the issue's second test: one Outside Profile of the star and
    /// its hole with no tab request, and four smooth 6 × 3 mm tabs placed
    /// on the star only (`tabs place --at`, holes left out): the hole is
    /// cut free and only the star's last pass rises, four times, whatever
    /// `on_holes` would have said. A tab placed on the hole as well is cut,
    /// so the hole's last pass rises too, and warns `tab_on_hole`.
    #[test]
    fn star_with_tabs_placed_only_on_the_star_lifts_only_on_the_star() {
        let mut p = star_with_hole_project();
        let op = ProfileOp {
            targets: vec![InstanceId::new("i1"), InstanceId::new("i2")],
            depth_mm: Some(12.0),
            step_down_mm: Some(4.0),
            through_cut_allowance_mm: 0.01,
            tabs: None,
            ..profile_op(&p, Side::Outside, CutDirection::Climb)
        };
        p.operations = vec![Operation::Profile(op.clone())];
        let star_mm = p.definitions[&DefinitionId::new("d1")].profiles[0].length_mm();
        let at: Vec<f64> = [0.5, 1.5, 2.5, 3.5].map(|k| star_mm * k / 4.0).to_vec();
        let size = crate::placed_tabs::TabSize {
            width_mm: 6.0,
            height_mm: 3.0,
            shape: TabShape::Smooth,
        };
        let placed = crate::placed_tabs::place_on_outlines(&p, &op.id, &at, size, false).unwrap();
        assert_eq!(placed.tabs.len(), 4, "{placed:?}");
        assert!(placed.tabs.iter().all(|t| t.instance.as_str() == "i1"));
        p.tabs = placed.tabs;

        let (tp, warnings) = generate(&p, &op);
        assert!(
            !codes(&warnings).contains(&WarningCode::TabOnHole),
            "{warnings:?}"
        );
        let (hole_rises, star_tabs, top_mm) = star_and_hole_rises(&tp);
        assert!(!hole_rises, "the hole has no tabs");
        assert_eq!(star_tabs, 4, "the star rises over its four tabs");
        assert_approx_eq!(top_mm, 3.0, TOL);

        let mut on_hole = p.tabs[0].clone();
        on_hole.id = dc_project::TabId::new("tab9");
        on_hole.instance = InstanceId::new("i2");
        on_hole.x_mm = STAR_HOLE_CENTRE.x + 3.5;
        on_hole.y_mm = STAR_HOLE_CENTRE.y;
        on_hole.width_mm = 1.0;
        p.tabs.push(on_hole);
        let (tp, warnings) = generate(&p, &op);
        assert!(
            codes(&warnings).contains(&WarningCode::TabOnHole),
            "{warnings:?}"
        );
        let (hole_rises, star_tabs, _) = star_and_hole_rises(&tp);
        assert!(hole_rises, "the hole's tab holds its slug");
        assert_eq!(star_tabs, 4);
    }

    /// The last pass of a star-and-hole toolpath: whether the hole's
    /// leaves the floor, how many times the star's rises, and the highest
    /// Z the star's reaches.
    fn star_and_hole_rises(tp: &Toolpath) -> (bool, usize, f64) {
        let mut cuts: Vec<(Point3, Move)> = Vec::new();
        for pair in tp.moves.windows(2) {
            let (from, mv) = (pair[0].to, pair[1]);
            if !mv.is_rapid() && from.xy().distance_to(mv.to.xy()) > EPS_MM {
                cuts.push((from, mv));
            }
        }
        let in_hole = |(from, mv): &(Point3, Move)| {
            from.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
                && mv.to.xy().distance_to(STAR_HOLE_CENTRE) < 3.5
        };
        let floor_mm = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        let on_last_pass = |(from, mv): &(Point3, Move)| from.z < 3.5 && mv.to.z < 3.5;
        let level = |(from, mv): &(Point3, Move)| {
            (from.z - floor_mm).abs() <= TOL && (mv.to.z - floor_mm).abs() <= TOL
        };
        let hole_rises = cuts
            .iter()
            .filter(|c| in_hole(c) && on_last_pass(c))
            .any(|c| !level(c));
        let star_last: Vec<_> = cuts
            .iter()
            .filter(|c| !in_hole(c) && on_last_pass(c))
            .collect();
        let raised: Vec<bool> = star_last.iter().map(|c| !level(c)).collect();
        let mut tabs = raised.windows(2).filter(|w| !w[0] && w[1]).count();
        if raised.first() == Some(&true) && raised.last() != Some(&true) {
            tabs += 1;
        }
        let top_mm = star_last
            .iter()
            .map(|(from, mv)| from.z.max(mv.to.z))
            .fold(f64::MIN, f64::max);
        (hole_rises, tabs, top_mm)
    }

    // ── #176: the start moves off a tab ──

    fn ramp_15() -> Entry {
        Entry::Ramp { angle_deg: 15.0 }
    }

    /// One `shape` tab 8 mm wide and 3 mm tall (so the passes at Z 2 and 0
    /// rise over it), centred `at_mm` along the path.
    fn tab_at(at_mm: f64, shape: TabShape) -> TabSpec {
        TabSpec {
            placement: TabPlacement::AtDistances {
                distances_mm: vec![at_mm],
            },
            width_mm: 8.0,
            height_mm: TAB_TOP_MM,
            shape,
            on_holes: false,
            avoid_corners: false,
        }
    }

    const TAB_TOP_MM: f64 = 3.0;
    /// Length of path one 8 mm tab raises with the 6.35 mm tool.
    const TAB_SPAN_MM: f64 = 8.0 + 6.35;

    /// Every straight-down feed move that ends below the stock top, with
    /// the point it starts from.
    fn plunges_below_top(tp: &Toolpath) -> Vec<(Point3, Move)> {
        tp.moves
            .windows(2)
            .filter(|w| {
                let (from, mv) = (w[0].to, w[1]);
                !mv.is_rapid()
                    && mv.to.z < from.z - EPS_MM
                    && from.xy().distance_to(mv.to.xy()) <= EPS_MM
                    && mv.to.z < THICKNESS_MM - EPS_MM
            })
            .map(|w| (w[0].to, w[1]))
            .collect()
    }

    /// Where the first move that falls while it moves in XY (the first
    /// ramp leg) starts.
    fn first_ramp_start(tp: &Toolpath) -> Point {
        tp.moves
            .windows(2)
            .find(|w| {
                let (from, mv) = (w[0].to, w[1]);
                !mv.is_rapid()
                    && mv.to.z < from.z - EPS_MM
                    && from.xy().distance_to(mv.to.xy()) > EPS_MM
            })
            .map(|w| w[0].to.xy())
            .expect("the toolpath ramps")
    }

    /// The level cutting moves at the tab top, as `(from, to)` in XY, and
    /// their length along the path (arcs as arcs).
    fn held_at_tab_top(tp: &Toolpath) -> (Vec<(Point, Point)>, f64) {
        let mut held = Vec::new();
        let mut length_mm = 0.0;
        for w in tp.moves.windows(2) {
            let (from, mv) = (w[0].to, w[1]);
            if mv.is_rapid()
                || (from.z - TAB_TOP_MM).abs() > EPS_MM
                || (mv.to.z - TAB_TOP_MM).abs() > EPS_MM
                || from.xy().distance_to(mv.to.xy()) <= EPS_MM
            {
                continue;
            }
            let (a, b) = (from.xy(), mv.to.xy());
            length_mm += match mv.kind {
                MoveKind::ArcCcw { centre } => {
                    a.distance_to(centre) * sweep_rad(a, b, centre, true)
                }
                MoveKind::ArcCw { centre } => {
                    -a.distance_to(centre) * sweep_rad(a, b, centre, false)
                }
                _ => a.distance_to(b),
            };
            held.push((a, b));
        }
        (held, length_mm)
    }

    /// #176: with a tab centred on the path's start, a ramp below the tab
    /// top would run through the tab, so slice 10 plunged onto it with
    /// `entry_fallback`. The start moves to where the ramp fits clear of
    /// the tab: no entry falls back and no straight plunge goes below the
    /// stock top, on every side. A start the ramp already fits at stays.
    #[test]
    fn start_under_a_tab_moves_so_the_ramp_fits() {
        let p = rect_project();
        for side in [Side::Inside, Side::Outside, Side::OnLine] {
            let mut op = profile_op(&p, side, CutDirection::Climb);
            op.entry = ramp_15();
            let (untabbed, _) = generate(&p, &op);
            let start = first_ramp_start(&untabbed);

            op.tabs = Some(tab_at(0.0, TabShape::Smooth));
            let (tp, warnings) = generate(&p, &op);
            assert!(
                !codes(&warnings).contains(&WarningCode::EntryFallback),
                "{side:?}: {warnings:?}"
            );
            assert!(
                plunges_below_top(&tp).is_empty(),
                "{side:?}: {:?}",
                plunges_below_top(&tp)
            );
            assert!(
                first_ramp_start(&tp).distance_to(start) > TAB_SPAN_MM / 2.0,
                "{side:?}: the start left the tab"
            );

            // Two tabs, a quarter and three quarters round: the ramp fits
            // at the start, which is kept.
            op.tabs = Some(TabSpec {
                placement: TabPlacement::Count { count: 2 },
                ..tab_at(0.0, TabShape::Smooth)
            });
            let (kept, warnings) = generate(&p, &op);
            assert!(
                !codes(&warnings).contains(&WarningCode::EntryFallback),
                "{side:?}: {warnings:?}"
            );
            assert!(
                first_ramp_start(&kept).distance_to(start) <= EPS_MM,
                "{side:?}: the start is kept"
            );
        }
    }

    /// #176: moving the start does not move the tabs. A tab is placed by
    /// its distance from the path's own start, where the plunge entry
    /// (which keeps the start) cuts it; with a ramp the start moves and
    /// the tab top is held over exactly the same stretch.
    #[test]
    fn tab_positions_do_not_move_when_the_seam_moves() {
        let p = rect_project();
        for side in [Side::Inside, Side::Outside, Side::OnLine] {
            let mut op = profile_op(&p, side, CutDirection::Climb);
            op.tabs = Some(tab_at(0.0, TabShape::Rect));
            let (plunged, _) = generate(&p, &op);
            // The plunge entry starts at the path start, the tab's centre.
            let centre = plunged.moves[0].to.xy();
            op.entry = ramp_15();
            let (ramped, warnings) = generate(&p, &op);
            assert!(
                !codes(&warnings).contains(&WarningCode::EntryFallback),
                "{side:?}: {warnings:?}"
            );
            assert!(
                ramped.moves[0].to.xy().distance_to(centre) > TAB_SPAN_MM / 2.0,
                "{side:?}: the seam moved off the tab"
            );
            let ((_, before_mm), (after, after_mm)) =
                (held_at_tab_top(&plunged), held_at_tab_top(&ramped));
            // Two passes (Z 2 and 0) below the tab top, one span each.
            assert_approx_eq!(before_mm, 2.0 * TAB_SPAN_MM, 1e-6);
            assert_approx_eq!(after_mm, before_mm, 1e-6);
            for (a, b) in after {
                for q in [a, b] {
                    assert!(
                        q.distance_to(centre) <= TAB_SPAN_MM / 2.0 + 1e-6,
                        "{side:?}: {q:?} is held off the tab at {centre:?}"
                    );
                }
            }
        }
    }

    // ── #175: On line Profile of an open path ──

    /// A 100 mm open line from (20, 20) to (120, 20).
    fn line_project() -> Project {
        project(
            vec![Polyline::new(
                vec![Vertex::line(0.0, 0.0), Vertex::line(100.0, 0.0)],
                false,
            )],
            &[("i1", at_20_20())],
        )
    }

    /// Every move that cuts in XY, with the point it starts from.
    fn xy_cuts(tp: &Toolpath) -> Vec<(Point3, Move)> {
        tp.moves
            .windows(2)
            .filter(|w| !w[1].is_rapid() && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM)
            .map(|w| (w[0].to, w[1]))
            .collect()
    }

    /// #175: an On line Profile cuts an open path, as VCarve's Profile
    /// "On" cuts an open vector. Each pass runs the line from its first
    /// vertex to its last at the pass Z (conventional), then the tool
    /// retracts and comes back to the first vertex for the next. (changed
    /// in #242: climb ran first to last; VCarve starts conventional there.)
    #[test]
    fn on_line_cuts_an_open_path() {
        let p = line_project();
        let (tp, warnings) = generate(
            &p,
            &profile_op(&p, Side::OnLine, CutDirection::Conventional),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = laps(&tp);
        assert_eq!(laps.len(), PASS_Z_MM.len(), "{laps:?}");
        for (lap, z) in laps.iter().zip(PASS_Z_MM) {
            assert_approx_eq!(lap.z_mm, z, TOL);
            assert_approx_eq!(lap.x_extent_mm(), 100.0, TOL);
            assert!(lap.points.iter().all(|q| (q.y - 20.0).abs() <= TOL));
            assert!(lap.points[0].distance_to(Point::new(20.0, 20.0)) <= TOL);
            let end = lap.points[lap.points.len() - 1];
            assert!(end.distance_to(Point::new(120.0, 20.0)) <= TOL);
        }
        for (from, mv) in xy_cuts(&tp) {
            assert!(mv.to.x > from.x, "every cut runs first to last: {mv:?}");
        }
        // Inside and Outside still skip it: an open path has no side.
        for side in [Side::Inside, Side::Outside] {
            let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
            assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
            assert!(tp.moves.is_empty(), "{side:?}: nothing is cut");
        }
    }

    /// #175: an open path with no length has nothing to cut On line; it
    /// is skipped with `open_path_skipped` ([`OpenPathReason::NoLength`]).
    #[test]
    fn on_line_open_path_with_no_length_is_skipped() {
        let dot = Polyline::new(vec![Vertex::line(5.0, 5.0), Vertex::line(5.0, 5.0)], false);
        let p = project(vec![rect_100x50(), dot], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::OpenPathSkipped]);
        let Warning::OpenPathSkipped { shape, reason } = &warnings[0] else {
            panic!("{:?}", warnings[0]);
        };
        assert_eq!(*reason, OpenPathReason::NoLength);
        assert_eq!(shape.profile, 1);
        assert!(warnings[0].to_string().contains("no length"));
        assert_eq!(laps(&tp).len(), PASS_Z_MM.len(), "only the rect is cut");
    }

    /// The direction picks the end an open path's cut starts at, as
    /// VCarve's does ("Conventional starts cutting at the Start point
    /// Climb cutting starts cutting at the end point"). Checks that every
    /// pass of the line, cut `direction`, descends at `start` (the line's
    /// X there), runs straight to `end` and ends there.
    fn assert_open_line_runs(direction: CutDirection, start: f64, end: f64) {
        let p = line_project();
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, direction));
        assert!(warnings.is_empty(), "{direction:?}: {warnings:?}");
        let laps = laps(&tp);
        assert_eq!(laps.len(), PASS_Z_MM.len(), "{direction:?}: {laps:?}");
        for (lap, z) in laps.iter().zip(PASS_Z_MM) {
            assert_approx_eq!(lap.z_mm, z, TOL);
            assert!(lap.points[0].distance_to(Point::new(start, 20.0)) <= TOL);
            let last = lap.points[lap.points.len() - 1];
            assert!(last.distance_to(Point::new(end, 20.0)) <= TOL, "{last:?}");
        }
        let toward = (end - start).signum();
        for (from, mv) in xy_cuts(&tp) {
            assert!((mv.to.x - from.x) * toward > 0.0, "{direction:?}: {mv:?}");
        }
        let plunges = plunges_below_top(&tp);
        assert!(!plunges.is_empty());
        for (from, _) in plunges {
            assert!(from.xy().distance_to(Point::new(start, 20.0)) <= TOL);
        }
    }

    /// #242: conventional cuts an open path from its first vertex to its
    /// last, as VCarve's "Conventional starts cutting at the Start point".
    /// (Replaces #175's `open_path_direction_picks_the_end_it_starts_at`,
    /// which pinned the opposite ends.)
    #[test]
    fn open_on_line_conventional_starts_at_first_vertex() {
        assert_open_line_runs(CutDirection::Conventional, 20.0, 120.0);
    }

    /// #242: climb cuts an open path from its last vertex to its first,
    /// as VCarve's "Climb cutting starts cutting at the end point". A tab
    /// at a distance stays at the same XY whichever end the cut starts at.
    #[test]
    fn open_on_line_climb_starts_at_last_vertex() {
        assert_open_line_runs(CutDirection::Climb, 120.0, 20.0);
        let p = line_project();
        let held_xs = |direction| {
            let mut op = profile_op(&p, Side::OnLine, direction);
            op.tabs = Some(tab_at(30.0, TabShape::Rect));
            let (tp, _) = generate(&p, &op);
            let mut xs: Vec<f64> = held_at_tab_top(&tp)
                .0
                .into_iter()
                .flat_map(|(a, b)| [a.x.min(b.x), a.x.max(b.x)])
                .collect();
            xs.sort_by(f64::total_cmp);
            xs
        };
        let (climb, conventional) = (
            held_xs(CutDirection::Climb),
            held_xs(CutDirection::Conventional),
        );
        assert!(!climb.is_empty());
        assert_eq!(climb.len(), conventional.len());
        for (a, b) in climb.iter().zip(&conventional) {
            assert_approx_eq!(*a, *b, TOL);
        }
        assert_approx_eq!(climb[0], 50.0 - TAB_SPAN_MM / 2.0, TOL);
        assert_approx_eq!(climb[climb.len() - 1], 50.0 + TAB_SPAN_MM / 2.0, TOL);
    }

    /// Acceptance 26 (#175): an On line Profile of a 100 mm line ramps
    /// back and forth along the line itself, at most three diameters from
    /// its first vertex, then cuts the whole line at the pass Z. A pass
    /// after the first comes down the slot the pass above cut, to that
    /// pass's Z, before it ramps. At 5° a leg falls 1.67 mm, less than
    /// every pass's descent, so each turns back. (changed in #214: at 15°
    /// the first pass, from safe Z, turned back; from 1 mm above the stock
    /// one leg reaches it. changed in #242: cut climb, which now starts at
    /// the last vertex; conventional starts at the first, as VCarve.)
    #[test]
    fn on_line_open_path_ramps_back_and_forth_along_the_line() {
        let p = line_project();
        let mut op = profile_op(&p, Side::OnLine, CutDirection::Conventional);
        op.entry = Entry::Ramp { angle_deg: 5.0 };
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let ramp: Vec<(Point3, Move)> = tp
            .moves
            .windows(2)
            .filter(|w| {
                !w[1].is_rapid()
                    && w[1].to.z < w[0].to.z - EPS_MM
                    && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM
            })
            .map(|w| (w[0].to, w[1]))
            .collect();
        assert!(!ramp.is_empty());
        let mut turned = false;
        for pair in ramp.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            turned |= (a.1.to.x - a.0.x) * (b.1.to.x - b.0.x) < 0.0;
        }
        assert!(turned, "back and forth: {ramp:?}");
        for (from, mv) in &ramp {
            assert_approx_eq!(mv.to.y, 20.0, TOL);
            assert!(mv.to.x >= 20.0 - TOL && mv.to.x <= 20.0 + 3.0 * 6.35 + TOL);
            let slope = (from.z - mv.to.z)
                .atan2(from.xy().distance_to(mv.to.xy()))
                .to_degrees();
            assert!((slope - 5.0).abs() <= 0.1, "slope {slope}");
        }
        for z in PASS_Z_MM {
            assert!(
                ramp.iter().any(|(_, mv)| (mv.to.z - z).abs() <= EPS_MM),
                "pass {z} is ramped into"
            );
            let full = laps_at(&tp, z)
                .iter()
                .any(|lap| (lap.points[lap.points.len() - 1].x - 120.0).abs() <= TOL);
            assert!(full, "pass {z} cuts to the line's end");
        }
        // Straight down only into the slot already cut: to the Z of the
        // pass above, at the first vertex.
        for (from, mv) in plunges_below_top(&tp) {
            assert!(from.xy().distance_to(Point::new(20.0, 20.0)) <= TOL);
            assert!(
                PASS_Z_MM.iter().any(|z| (mv.to.z - z).abs() <= EPS_MM)
                    && mv.to.z > PASS_Z_MM[PASS_Z_MM.len() - 1] + EPS_MM,
                "{mv:?}"
            );
        }
    }

    /// #175: tabs on an open path are placed by distance from its first
    /// vertex as drawn, whichever end the cut starts at.
    #[test]
    fn open_path_tabs_are_measured_from_its_first_vertex() {
        let p = line_project();
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let mut op = profile_op(&p, Side::OnLine, direction);
            op.tabs = Some(tab_at(30.0, TabShape::Rect));
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{direction:?}: {warnings:?}");
            let (held, length_mm) = held_at_tab_top(&tp);
            assert_approx_eq!(length_mm, 2.0 * TAB_SPAN_MM, 1e-6);
            for (a, b) in held {
                for q in [a, b] {
                    assert!(
                        (q.x - 50.0).abs() <= TAB_SPAN_MM / 2.0 + TOL,
                        "{direction:?}: {q:?}"
                    );
                }
            }
        }
    }

    // ── #207: Left and Right of an open path ──

    /// The 6.35 mm tool's radius.
    const R_MM: f64 = 6.35 / 2.0;

    /// A Profile of `p` on `side`, `direction`, with `edit` applied.
    fn sided_op(
        p: &Project,
        side: Side,
        direction: CutDirection,
        edit: impl FnOnce(&mut ProfileOp),
    ) -> ProfileOp {
        let mut op = profile_op(p, side, direction);
        edit(&mut op);
        op
    }

    /// Checks that every lap of `tp` runs at `fixed` across (Y when
    /// `along_x`, else X), from `start` to `end` along the other axis.
    fn assert_laps_run(
        tp: &Toolpath,
        along_x: bool,
        fixed: f64,
        (start, end): (f64, f64),
        case: &str,
    ) {
        let split = |q: Point| if along_x { (q.y, q.x) } else { (q.x, q.y) };
        let laps = laps(tp);
        assert_eq!(laps.len(), PASS_Z_MM.len(), "{case}: {laps:?}");
        for (lap, z) in laps.iter().zip(PASS_Z_MM) {
            assert_approx_eq!(lap.z_mm, z, TOL);
            for &q in &lap.points {
                assert!(
                    (split(q).0 - fixed).abs() <= TOL,
                    "{case}: {q:?} is off {fixed}"
                );
            }
            let first = lap.points[0];
            let last = lap.points[lap.points.len() - 1];
            assert!(
                (split(first).1 - start).abs() <= TOL,
                "{case}: starts at {first:?}"
            );
            assert!(
                (split(last).1 - end).abs() <= TOL,
                "{case}: ends at {last:?}"
            );
        }
    }

    /// #207: Left puts the tool centre one radius to the left of the path,
    /// looking from its first vertex to its last: in Y-up the normal
    /// `(−dy, dx)`. The line from (20, 20) to (120, 20) is cut at Y 20 + r
    /// over its whole length, the cut ending square on the normals through
    /// its ends; drawn the other way, its left is −Y; drawn upward, −X.
    /// Climb, so each pass runs from the first vertex (#287).
    #[test]
    fn open_path_left_offsets_to_the_left() {
        let climb = CutDirection::Climb;
        let p = line_project();
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Left, climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_laps_run(&tp, true, 20.0 + R_MM, (20.0, 120.0), "left, drawn +X");

        let back = back_line_project();
        let (tp, _) = generate(&back, &profile_op(&back, Side::Left, climb));
        assert_laps_run(&tp, true, 20.0 - R_MM, (120.0, 20.0), "left, drawn −X");

        let up = project(
            vec![open(&[(0.0, 0.0), (0.0, 50.0)])],
            &[("i1", at_20_20())],
        );
        let (tp, _) = generate(&up, &profile_op(&up, Side::Left, climb));
        assert_laps_run(&tp, false, 20.0 - R_MM, (20.0, 70.0), "left, drawn +Y");
    }

    /// #207: Right is the mirror of Left: the tool centre one radius to
    /// the right of the path as drawn, `(dy, −dx)`. Conventional, so each
    /// pass runs from the first vertex (#287).
    #[test]
    fn open_path_right_offsets_to_the_right() {
        let conventional = CutDirection::Conventional;
        let p = line_project();
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Right, conventional));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_laps_run(&tp, true, 20.0 - R_MM, (20.0, 120.0), "right, drawn +X");

        let back = back_line_project();
        let (tp, _) = generate(&back, &profile_op(&back, Side::Right, conventional));
        assert_laps_run(&tp, true, 20.0 + R_MM, (120.0, 20.0), "right, drawn −X");

        let up = project(
            vec![open(&[(0.0, 0.0), (0.0, 50.0)])],
            &[("i1", at_20_20())],
        );
        let (tp, _) = generate(&up, &profile_op(&up, Side::Right, conventional));
        assert_laps_run(&tp, false, 20.0 + R_MM, (20.0, 70.0), "right, drawn +Y");
    }

    /// #207: Left and Right are sides of an open path only. A closed
    /// target is skipped with `side_needs_open_path` naming it, not
    /// mapped to Inside or Outside by its winding, which the drawing does
    /// not show; the open paths are cut. Inside and Outside still skip
    /// open paths, and their warning now names Left and Right too.
    #[test]
    fn left_right_on_closed_target_is_skipped_with_warning() {
        let line = open(&[(0.0, 70.0), (100.0, 70.0)]);
        let p = project(vec![rect_100x50(), line], &[("i1", at_20_20())]);
        for side in [Side::Left, Side::Right] {
            let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
            assert_eq!(
                codes(&warnings),
                vec![WarningCode::SideNeedsOpenPath],
                "{side:?}"
            );
            let Warning::SideNeedsOpenPath { shape } = &warnings[0] else {
                panic!("{:?}", warnings[0]);
            };
            assert_eq!(shape.profile, 0);
            let message = warnings[0].to_string();
            assert!(message.contains("closed"), "{message}");
            assert!(message.contains("profile 0"), "{message}");
            assert_eq!(laps(&tp).len(), PASS_Z_MM.len(), "{side:?}: only the line");
        }
        let rect = rect_project();
        let (tp, warnings) = generate(&rect, &profile_op(&rect, Side::Left, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::SideNeedsOpenPath]);
        assert!(tp.moves.is_empty(), "nothing is cut");

        let (_, warnings) = generate(&p, &profile_op(&p, Side::Outside, CutDirection::Climb));
        let message = warnings[0].to_string();
        assert!(
            message.contains("Left") && message.contains("Right"),
            "{message}"
        );
    }

    /// The 100 mm line of [`line_project`] drawn the other way, from
    /// (120, 20) to (20, 20).
    fn back_line_project() -> Project {
        project(
            vec![open(&[(100.0, 0.0), (0.0, 0.0)])],
            &[("i1", at_20_20())],
        )
    }

    /// Checks that `p`'s line along Y 20, cut on `side` with `direction`,
    /// runs every pass at `y` from X `start` to X `end`, plunging at
    /// `start`, and that the line, with the part beyond it, lies on the
    /// right of every cutting move when `part_on_right` and on its left
    /// otherwise.
    fn assert_side_cut(
        p: &Project,
        (side, direction): (Side, CutDirection),
        y: f64,
        (start, end): (f64, f64),
        part_on_right: bool,
    ) {
        let case = format!("{side:?} {direction:?}, {start} to {end}");
        let (tp, warnings) = generate(p, &profile_op(p, side, direction));
        assert!(warnings.is_empty(), "{case}: {warnings:?}");
        assert_laps_run(&tp, true, y, (start, end), &case);
        let cuts = xy_cuts(&tp);
        assert!(!cuts.is_empty(), "{case}");
        let toward = (end - start).signum();
        for (from, mv) in cuts {
            let (dx, dy) = (mv.to.x - from.x, mv.to.y - from.y);
            assert!(dx * toward > 0.0, "{case}: {mv:?}");
            // The cross product of travel `(dx, dy)` with the way to the
            // line, `(0, 20 − y)`, is negative when the line is on the
            // right of travel; the cut is level, so `dy` is zero.
            assert!(dy.abs() <= TOL, "{case}: {mv:?}");
            let cross = dx * (20.0 - from.y);
            assert_eq!(cross < 0.0, part_on_right, "{case}: {mv:?}");
        }
        let plunges = plunges_below_top(&tp);
        assert!(!plunges.is_empty(), "{case}");
        for (from, _) in plunges {
            assert!(
                from.xy().distance_to(Point::new(start, y)) <= TOL,
                "{case}: plunge at {from:?}"
            );
        }
    }

    /// #287: on a Left cut only the bit's flank toward the line meets kept
    /// material, so `direction` is the milling mode, as for closed shapes:
    /// the spindle turns clockwise from above, so climb keeps the part on
    /// the right of travel. The tool runs left of the line and the part is
    /// beyond the line, so climb runs from the end beside the first vertex
    /// to the end beside the last. The line from (20, 20) to (120, 20) is
    /// cut at Y 20 + r moving +X, the part (below) on its right; drawn the
    /// other way, at Y 20 − r moving −X, the part (above) on its right.
    #[test]
    fn left_climb_keeps_the_part_on_the_right_of_travel() {
        let climb = (Side::Left, CutDirection::Climb);
        assert_side_cut(&line_project(), climb, 20.0 + R_MM, (20.0, 120.0), true);
        let back = back_line_project();
        assert_side_cut(&back, climb, 20.0 - R_MM, (120.0, 20.0), true);
    }

    /// #287: on a Right cut the part is on the line's left, so climb, the
    /// part on the right of travel, runs from the end beside the last
    /// vertex to the end beside the first: at Y 20 − r moving −X, the part
    /// (above) on its right; drawn the other way, at Y 20 + r moving +X.
    #[test]
    fn right_climb_keeps_the_part_on_the_right_of_travel() {
        let climb = (Side::Right, CutDirection::Climb);
        assert_side_cut(&line_project(), climb, 20.0 - R_MM, (120.0, 20.0), true);
        let back = back_line_project();
        assert_side_cut(&back, climb, 20.0 + R_MM, (20.0, 120.0), true);
    }

    /// #287: conventional is the reverse of climb, the part on the left
    /// of travel: Left conventional runs from the end beside the last
    /// vertex, Right conventional from the end beside the first.
    #[test]
    fn left_conventional_is_the_reverse() {
        let p = line_project();
        let left = (Side::Left, CutDirection::Conventional);
        assert_side_cut(&p, left, 20.0 + R_MM, (120.0, 20.0), false);
        let back = back_line_project();
        assert_side_cut(&back, left, 20.0 - R_MM, (20.0, 120.0), false);
        let right = (Side::Right, CutDirection::Conventional);
        assert_side_cut(&p, right, 20.0 - R_MM, (20.0, 120.0), false);
        assert_side_cut(&back, right, 20.0 + R_MM, (120.0, 20.0), false);
    }

    /// #207: VCarve allows ramps and tabs on open profile toolpaths, and
    /// no leads. A Left or Right ramp runs back and forth along the offset
    /// path, as On line's runs along the line (acceptance 26), within
    /// three diameters of its start; tabs are measured along the offset
    /// path from the end beside the first vertex as drawn, whichever end
    /// the cut starts at. A lead warns `lead_dropped` and corner relief
    /// `relief_skipped`, both for an open path, and a helix, which needs a
    /// waste area, is refused.
    #[test]
    fn left_right_ramp_and_tabs() {
        let p = line_project();
        // The direction that starts beside the first vertex (#287).
        for (side, y, forward) in [
            (Side::Left, 20.0 + R_MM, CutDirection::Climb),
            (Side::Right, 20.0 - R_MM, CutDirection::Conventional),
        ] {
            let op = sided_op(&p, side, forward, |op| {
                op.entry = Entry::Ramp { angle_deg: 5.0 };
            });
            let (tp, warnings) = generate(&p, &op);
            assert!(warnings.is_empty(), "{side:?}: {warnings:?}");
            let ramp: Vec<(Point3, Move)> = tp
                .moves
                .windows(2)
                .filter(|w| {
                    !w[1].is_rapid()
                        && w[1].to.z < w[0].to.z - EPS_MM
                        && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM
                })
                .map(|w| (w[0].to, w[1]))
                .collect();
            assert!(!ramp.is_empty(), "{side:?}");
            let mut turned = false;
            for pair in ramp.windows(2) {
                turned |= (pair[0].1.to.x - pair[0].0.x) * (pair[1].1.to.x - pair[1].0.x) < 0.0;
            }
            assert!(turned, "{side:?}: back and forth: {ramp:?}");
            for (_, mv) in &ramp {
                assert_approx_eq!(mv.to.y, y, TOL);
                assert!(
                    mv.to.x >= 20.0 - TOL && mv.to.x <= 20.0 + 3.0 * 6.35 + TOL,
                    "{side:?}: {mv:?}"
                );
            }

            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                let op = sided_op(&p, side, direction, |op| {
                    op.tabs = Some(tab_at(30.0, TabShape::Rect));
                });
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{side:?} {direction:?}: {warnings:?}");
                let (held, length_mm) = held_at_tab_top(&tp);
                assert_approx_eq!(length_mm, 2.0 * TAB_SPAN_MM, 1e-6);
                for (a, b) in held {
                    for q in [a, b] {
                        assert!(
                            (q.x - 50.0).abs() <= TAB_SPAN_MM / 2.0 + TOL && (q.y - y).abs() <= TOL,
                            "{side:?} {direction:?}: {q:?}"
                        );
                    }
                }
            }

            let op = sided_op(&p, side, CutDirection::Climb, |op| {
                op.lead = Some(Lead {
                    radius_mm: 4.0,
                    sweep_deg: 90.0,
                    lead_out: true,
                    overcut_mm: 0.0,
                });
                op.corner_relief = CornerRelief::Dogbone;
            });
            let (_, warnings) = generate(&p, &op);
            assert_eq!(
                codes(&warnings),
                vec![WarningCode::LeadDropped, WarningCode::ReliefSkipped],
                "{side:?}"
            );
            assert!(matches!(
                warnings[0],
                Warning::LeadDropped {
                    reason: crate::warning::LeadDroppedReason::OpenPath,
                    ..
                }
            ));
            assert!(matches!(
                warnings[1],
                Warning::ReliefSkipped {
                    reason: crate::warning::ReliefSkippedReason::OpenPath,
                    ..
                }
            ));

            let op = sided_op(&p, side, CutDirection::Climb, |op| {
                op.entry = Entry::Helix {
                    diameter_fraction: 0.8,
                    angle_deg: 5.0,
                };
            });
            let err = generate_profile(&p, &op).unwrap_err();
            assert!(
                matches!(err, CamError::HelixNeedsWasteSide { .. }),
                "{side:?}: {err:?}"
            );
        }
    }

    /// #207: the side a Left or Right cut runs on is waste, the other side
    /// is the part. A cut that comes within the tool radius of another
    /// line of the operation crosses into what that line keeps; one that
    /// comes within the radius of the strip one radius wide that a line
    /// keeps beside it (its kept edge) cuts into it too. Both are refused
    /// with `CutsIntoKeptSide`, the shapes in drawing order: two lines
    /// 4 mm apart, two lines 5 mm apart whose cut sides face each other,
    /// a line drawn twice in opposite directions (each copy keeps what the
    /// other cuts), and a square of four separate lines cut on its
    /// inside, where each line's square end runs onto the next. The same
    /// square cut on its outside, lines 20 mm apart and the line drawn
    /// twice the same way (dropped as a duplicate) are cut.
    #[test]
    fn left_right_cut_into_the_kept_side_is_refused() {
        let square = |clockwise: bool| {
            let mut corners = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)];
            if clockwise {
                corners.reverse();
            }
            (0..4)
                .map(|k| open(&[corners[k], corners[(k + 1) % 4]]))
                .collect::<Vec<_>>()
        };
        let line = |from: (f64, f64), to: (f64, f64)| open(&[from, to]);
        let refused: Vec<(&str, Vec<Polyline>, Side)> = vec![
            (
                "4 mm apart",
                vec![
                    line((0.0, 0.0), (100.0, 0.0)),
                    line((0.0, 4.0), (100.0, 4.0)),
                ],
                Side::Left,
            ),
            (
                "facing cuts 5 mm apart",
                vec![
                    line((0.0, 0.0), (100.0, 0.0)),
                    line((100.0, 5.0), (0.0, 5.0)),
                ],
                Side::Left,
            ),
            (
                "drawn twice, opposite ways",
                vec![
                    line((0.0, 0.0), (100.0, 0.0)),
                    line((100.0, 0.0), (0.0, 0.0)),
                ],
                Side::Right,
            ),
            ("a square of lines, inside", square(false), Side::Left),
        ];
        for (case, profiles, side) in refused {
            let p = project(profiles, &[("i1", at_20_20())]);
            let err =
                generate_profile(&p, &profile_op(&p, side, CutDirection::Climb)).expect_err(case);
            let CamError::CutsIntoKeptSide {
                shape,
                other,
                diameter_mm,
                ..
            } = &err
            else {
                panic!("{case}: expected CutsIntoKeptSide, got {err:?}");
            };
            assert!(
                shape.profile < other.profile,
                "{case}: drawing order {err:?}"
            );
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
        }
        let cut: Vec<(&str, Vec<Polyline>, Side, usize)> = vec![
            ("a square of lines, outside", square(false), Side::Right, 4),
            (
                "a square of lines drawn clockwise, outside",
                square(true),
                Side::Left,
                4,
            ),
            (
                "20 mm apart",
                vec![
                    line((0.0, 0.0), (100.0, 0.0)),
                    line((0.0, 20.0), (100.0, 20.0)),
                ],
                Side::Left,
                2,
            ),
        ];
        for (case, profiles, side, lines) in cut {
            let p = project(profiles, &[("i1", at_20_20())]);
            let (tp, warnings) = generate(&p, &profile_op(&p, side, CutDirection::Climb));
            assert!(warnings.is_empty(), "{case}: {warnings:?}");
            assert_eq!(laps(&tp).len(), lines * PASS_Z_MM.len(), "{case}");
        }
        let twice = project(
            vec![
                line((0.0, 0.0), (100.0, 0.0)),
                line((0.0, 0.0), (100.0, 0.0)),
            ],
            &[("i1", at_20_20())],
        );
        let (tp, warnings) = generate(&twice, &profile_op(&twice, Side::Left, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
        assert_eq!(laps(&tp).len(), PASS_Z_MM.len());
    }

    /// #207: an offset that vanishes leaves nothing to cut: a half circle
    /// of radius 2 mm cut on the side its centre lies on, with a 6.35 mm
    /// bit, warns `tool_too_large` and is not cut. Cut on its other side
    /// it is.
    #[test]
    fn left_right_tool_too_large_for_a_tight_arc_warns() {
        let arc = Polyline::new(
            vec![Vertex::new(52.0, 25.0, 1.0), Vertex::line(48.0, 25.0)],
            false,
        );
        let p = project(vec![arc], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Left, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::ToolTooLarge]);
        assert!(tp.moves.is_empty());
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::Right, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(laps(&tp).len(), PASS_Z_MM.len());
    }

    // ── #210: an open path in a part, beside the part's own lines ──

    /// An open polyline through `points`.
    fn open(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    /// The 100 × 50 rect at (20, 20) with `extra` profiles drawn in it,
    /// in the rect's own coordinates.
    fn part_with(extra: Vec<Polyline>) -> Project {
        let mut profiles = vec![rect_100x50()];
        profiles.extend(extra);
        project(profiles, &[("i1", at_20_20())])
    }

    /// Generates an On line Profile of `p` and checks every line is cut
    /// at every pass, with no finding.
    fn every_line_cut(case: &str, p: &Project) {
        let lines = p
            .definitions
            .values()
            .map(|d| d.profiles.len())
            .sum::<usize>();
        let (tp, warnings) = generate(p, &profile_op(p, Side::OnLine, CutDirection::Climb));
        assert!(warnings.is_empty(), "{case}: {warnings:?}");
        assert_eq!(
            laps(&tp).len(),
            lines * PASS_Z_MM.len(),
            "{case}: every line at every pass"
        );
    }

    /// #210: an open path that runs alongside a line of its own part,
    /// closer than the 6.35 mm bit, cuts through the wall between them
    /// together with that line's cut, as #199's narrow ring: refused with
    /// `BandNarrowerThanTool`, the two shapes in drawing order and the band
    /// the narrowest gap away from where the lines meet. Meeting the
    /// outline excuses only the stretch near the meeting point (within
    /// `√2·d` of it, so a slot meeting the outline at 45° or more is
    /// cut): a slot from the bottom edge that runs 2 mm beside the left
    /// edge, or turns to run 2 mm above the bottom edge, or leaves it at
    /// 10°, still cuts through a wall.
    #[test]
    fn open_path_alongside_its_part_wall_is_refused() {
        let shallow_rad = 10_f64.to_radians();
        let reach_mm = std::f64::consts::SQRT_2 * 6.35;
        // The case, the profiles drawn in the part, the two profiles named
        // and the band.
        type Case = (&'static str, Vec<Polyline>, (usize, usize), f64);
        let cases: Vec<Case> = vec![
            (
                "1 mm inside the left edge",
                vec![open(&[(1.0, 10.0), (1.0, 40.0)])],
                (0, 1),
                1.0,
            ),
            (
                "from the bottom edge, 2 mm beside the left edge",
                vec![open(&[(2.0, 0.0), (2.0, 30.0)])],
                (0, 1),
                2.0,
            ),
            (
                "from the bottom edge, then along it",
                vec![open(&[(50.0, 0.0), (50.0, 2.0), (90.0, 2.0)])],
                (0, 1),
                2.0,
            ),
            (
                "from the bottom edge at 10°",
                vec![open(&[
                    (50.0, 0.0),
                    (50.0 + 40.0 * shallow_rad.cos(), 40.0 * shallow_rad.sin()),
                ])],
                (0, 1),
                reach_mm * shallow_rad.sin(),
            ),
            (
                "3 mm from a hole",
                vec![
                    Polyline::circle(Point::new(50.0, 25.0), 10.0),
                    open(&[(63.0, 15.0), (63.0, 35.0)]),
                ],
                (1, 2),
                3.0,
            ),
            (
                "2 mm from another line",
                vec![
                    open(&[(30.0, 10.0), (30.0, 40.0)]),
                    open(&[(32.0, 10.0), (32.0, 40.0)]),
                ],
                (1, 2),
                2.0,
            ),
        ];
        for (case, extra, pair, band_mm) in cases {
            let p = part_with(extra);
            let err = generate_profile(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb))
                .expect_err(case);
            let CamError::BandNarrowerThanTool {
                shape,
                other,
                band_mm: band,
                diameter_mm,
                ..
            } = &err
            else {
                panic!("{case}: expected BandNarrowerThanTool, got {err:?}");
            };
            assert_eq!((shape.profile, other.profile), pair, "{case}");
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
            assert!((band - band_mm).abs() <= TOL, "{case}: band {band}");
        }
    }

    /// #210: an open path that meets a line of its part, a kerf slot from
    /// the edge or a saw cut across the part, opens into that line's cut
    /// as the user drew it: cut, with no finding. So are a slot meeting
    /// the edge at 60°, two slots joined at a corner, two lines crossing,
    /// and a slot from a hole's edge.
    #[test]
    fn open_slot_from_the_edge_is_cut() {
        let steep_rad = 60_f64.to_radians();
        let cases: Vec<(&str, Vec<Polyline>)> = vec![
            (
                "square to the edge",
                vec![open(&[(50.0, 0.0), (50.0, 20.0)])],
            ),
            (
                "a saw cut edge to edge",
                vec![open(&[(50.0, 0.0), (50.0, 50.0)])],
            ),
            (
                "at 60° to the edge",
                vec![open(&[
                    (50.0, 0.0),
                    (50.0 + 20.0 * steep_rad.cos(), 20.0 * steep_rad.sin()),
                ])],
            ),
            (
                "two slots joined at a corner",
                vec![
                    open(&[(50.0, 0.0), (50.0, 20.0)]),
                    open(&[(50.0, 20.0), (70.0, 20.0)]),
                ],
            ),
            (
                "two lines crossing",
                vec![
                    open(&[(30.0, 25.0), (70.0, 25.0)]),
                    open(&[(50.0, 10.0), (50.0, 40.0)]),
                ],
            ),
            (
                "from a hole's edge",
                vec![
                    Polyline::circle(Point::new(50.0, 25.0), 10.0),
                    open(&[(60.0, 25.0), (80.0, 25.0)]),
                ],
            ),
        ];
        for (case, extra) in cases {
            every_line_cut(case, &part_with(extra));
        }
    }

    /// #210: an open path at least the bit's diameter from every other
    /// line of its part is cut; a band exactly `d` wide is cut, as #199's
    /// ring is.
    #[test]
    fn open_path_far_from_walls_is_cut() {
        let cases: Vec<(&str, Vec<Polyline>)> = vec![
            ("in the middle", vec![open(&[(20.0, 25.0), (80.0, 25.0)])]),
            (
                "a tool width inside the left edge",
                vec![open(&[(6.35, 10.0), (6.35, 40.0)])],
            ),
            (
                "a tool width from a hole and from another line",
                vec![
                    Polyline::circle(Point::new(50.0, 25.0), 10.0),
                    open(&[(66.35, 15.0), (66.35, 35.0)]),
                    open(&[(72.7, 15.0), (72.7, 35.0)]),
                ],
            ),
        ];
        for (case, extra) in cases {
            every_line_cut(case, &part_with(extra));
        }
    }

    // ── #221: a line closer than the bit to itself ──

    /// Generates an On line Profile of `p`, expects `BandNarrowerThanTool`
    /// naming profile `profile` of `i1` as both shapes, and returns the
    /// band.
    fn refused_self_band(case: &str, p: &Project, profile: usize) -> f64 {
        let err =
            generate_profile(p, &profile_op(p, Side::OnLine, CutDirection::Climb)).expect_err(case);
        let CamError::BandNarrowerThanTool {
            shape,
            other,
            band_mm,
            diameter_mm,
            ..
        } = &err
        else {
            panic!("{case}: expected BandNarrowerThanTool, got {err:?}");
        };
        assert_eq!(shape, &**other, "{case}: the shape is named twice");
        assert_eq!(shape.profile, profile, "{case}");
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        *band_mm
    }

    /// The point `length_mm` from `from` at `angle_deg` from +X.
    fn towards(from: (f64, f64), angle_deg: f64, length_mm: f64) -> (f64, f64) {
        let a = angle_deg.to_radians();
        (from.0 + length_mm * a.cos(), from.1 + length_mm * a.sin())
    }

    /// An open V with its tip at `tip`, pointing up, its two 30 mm arms
    /// `angle_deg` apart.
    fn v_turn(tip: (f64, f64), angle_deg: f64) -> Polyline {
        let half = angle_deg / 2.0;
        open(&[
            towards(tip, -90.0 - half, 30.0),
            tip,
            towards(tip, -90.0 + half, 30.0),
        ])
    }

    /// #221: two stretches of one line, more than `√2·d` apart along it,
    /// that come closer than the 6.35 mm bit cut through the part between
    /// them, as two lines do (#199, #210): refused with
    /// `BandNarrowerThanTool` naming the shape twice. A U with square
    /// corners and legs 2 mm apart, one with a round end and legs 3 mm
    /// apart, and a closed outline with a 3 mm neck (a dumbbell). A sharp
    /// corner is where two lines meet (#210): its arms are set aside
    /// within `√2·d` of the tip, so a 10° spike is refused with the band
    /// `√2·d·sin 10°`, as a slot meeting the edge at 10° is.
    #[test]
    fn on_line_hairpin_narrower_than_tool_is_refused() {
        let reach_mm = std::f64::consts::SQRT_2 * 6.35;
        let round_end = Polyline::new(
            vec![
                Vertex::line(50.0, 10.0),
                Vertex::new(50.0, 38.0, -1.0),
                Vertex::line(53.0, 38.0),
                Vertex::line(53.0, 10.0),
            ],
            false,
        );
        let cases: Vec<(&str, Polyline, f64)> = vec![
            (
                "a U with legs 2 mm apart",
                open(&[(30.0, 10.0), (30.0, 40.0), (32.0, 40.0), (32.0, 10.0)]),
                2.0,
            ),
            ("a U with a round end, legs 3 mm apart", round_end, 3.0),
            (
                "a 10° spike",
                v_turn((80.0, 40.0), 10.0),
                reach_mm * 10_f64.to_radians().sin(),
            ),
        ];
        for (case, line, band_mm) in cases {
            let band = refused_self_band(case, &part_with(vec![line]), 1);
            assert!((band - band_mm).abs() <= TOL, "{case}: band {band}");
        }
        let dumbbell = Polyline::new(
            [
                (0.0, 0.0),
                (30.0, 0.0),
                (30.0, 13.5),
                (50.0, 13.5),
                (50.0, 0.0),
                (80.0, 0.0),
                (80.0, 30.0),
                (50.0, 30.0),
                (50.0, 16.5),
                (30.0, 16.5),
                (30.0, 30.0),
                (0.0, 30.0),
            ]
            .into_iter()
            .map(|(x, y)| Vertex::line(x, y))
            .collect(),
            true,
        );
        let p = project(vec![dumbbell], &[("i1", at_20_20())]);
        let band = refused_self_band("a dumbbell", &p, 0);
        assert_approx_eq!(band, 3.0, TOL);
    }

    /// #221: an open spiral in a part whose turns are 4 mm apart, closer
    /// than the 6.35 mm bit, cuts through the part between its turns:
    /// refused naming the spiral twice, drawn as fine chords or as half
    /// circles about two centres.
    #[test]
    fn on_line_spiral_with_tight_turns_is_refused() {
        // r = 4 + 4·θ/2π about (50, 25), three turns in 5° chords.
        let chords: Vec<(f64, f64)> = (0..=216)
            .map(|k| {
                let theta = f64::from(k) * 5_f64.to_radians();
                let r = 4.0 + 4.0 * theta / TAU;
                (50.0 + r * theta.cos(), 25.0 + r * theta.sin())
            })
            .collect();
        let band = refused_self_band("fine chords", &part_with(vec![open(&chords)]), 1);
        assert!(band > 3.5 && band < 4.0, "fine chords: band {band}");
        // Half turns about (50, 25) above the axis and (52, 25) below it,
        // each 2 mm wider: turns 4 mm apart.
        let halves = Polyline::new(
            [52.0, 48.0, 56.0, 44.0, 60.0, 40.0, 64.0, 36.0]
                .into_iter()
                .map(|x| Vertex::new(x, 25.0, 1.0))
                .collect(),
            false,
        );
        let band = refused_self_band("half circles", &part_with(vec![halves]), 1);
        assert_approx_eq!(band, 4.0, TOL);
    }

    /// #221: neighbouring stretches of one line are not compared with each
    /// other, only stretches more than `√2·d` apart along it, so a smooth
    /// curve, a corner of 90° or more, and a round hole are cut as drawn.
    /// A sharp corner sets its arms aside within `√2·d` of the tip, as
    /// where two lines meet (#210), so a V of 60°, or of exactly 45°, is
    /// cut too.
    #[test]
    fn on_line_smooth_curve_is_not_compared_with_itself() {
        let wave: Vec<(f64, f64)> = (10..=90)
            .map(|x| {
                let x = f64::from(x);
                (x, 25.0 + 8.0 * (x / 10.0).sin())
            })
            .collect();
        let half_circle: Vec<(f64, f64)> = (0..=90)
            .map(|k| {
                let a = f64::from(180 - 2 * k).to_radians();
                (50.0 + 15.0 * a.cos(), 20.0 + 15.0 * a.sin())
            })
            .collect();
        let cases: Vec<(&str, Vec<Polyline>)> = vec![
            ("a wave of fine chords", vec![open(&wave)]),
            ("a half circle of fine chords", vec![open(&half_circle)]),
            (
                "an L",
                vec![open(&[(30.0, 10.0), (30.0, 40.0), (60.0, 40.0)])],
            ),
            (
                "a square corner turned back",
                vec![open(&[
                    (30.0, 10.0),
                    (30.0, 40.0),
                    (60.0, 40.0),
                    (60.0, 10.0),
                ])],
            ),
            (
                "a round hole",
                vec![Polyline::circle(Point::new(50.0, 25.0), 10.0)],
            ),
            ("a V of 60°", vec![v_turn((50.0, 40.0), 60.0)]),
            ("a V of 45°", vec![v_turn((50.0, 40.0), 45.0)]),
        ];
        for (case, extra) in cases {
            every_line_cut(case, &part_with(extra));
        }
    }

    /// #221: only the part between two stretches of one line is kept. A
    /// 3 mm notch cut into an outline, and a hole with a 3 mm neck, have
    /// waste between their stretches, which the cut may clear: both are
    /// cut as drawn.
    #[test]
    fn on_line_narrow_notch_in_an_outline_is_cut() {
        let closed = |points: &[(f64, f64)]| {
            Polyline::new(
                points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
                true,
            )
        };
        let notched = closed(&[
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 50.0),
            (51.5, 50.0),
            (51.5, 25.0),
            (48.5, 25.0),
            (48.5, 50.0),
            (0.0, 50.0),
        ]);
        every_line_cut("a notch", &project(vec![notched], &[("i1", at_20_20())]));
        let necked_hole = closed(&[
            (20.0, 15.0),
            (40.0, 15.0),
            (40.0, 23.5),
            (60.0, 23.5),
            (60.0, 15.0),
            (80.0, 15.0),
            (80.0, 35.0),
            (60.0, 35.0),
            (60.0, 26.5),
            (40.0, 26.5),
            (40.0, 35.0),
            (20.0, 35.0),
        ]);
        every_line_cut("a necked hole", &part_with(vec![necked_hole]));
    }

    /// #221: an open path drawn twice, the same vertices in either
    /// direction, is cut once and the copies are dropped with
    /// `duplicate_shape_dropped`, as a closed shape's are, before the band
    /// check (which would see a band of 0 between them).
    #[test]
    fn duplicate_open_paths_are_dropped_with_a_warning() {
        let line = open(&[(30.0, 10.0), (30.0, 40.0), (60.0, 40.0)]);
        let p = part_with(vec![line.clone(), line.clone(), line.reversed()]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
        assert_eq!(
            codes(&warnings),
            vec![
                WarningCode::DuplicateShapeDropped,
                WarningCode::DuplicateShapeDropped
            ]
        );
        for (warning, profile) in warnings.iter().zip([2, 3]) {
            let Warning::DuplicateShapeDropped {
                shape,
                duplicate_of,
            } = warning
            else {
                panic!("{warning:?}");
            };
            assert_eq!(shape.profile, profile);
            assert_eq!(
                (duplicate_of.instance.as_str(), duplicate_of.profile),
                ("i1", 1)
            );
        }
        assert_eq!(
            laps(&tp).len(),
            2 * PASS_Z_MM.len(),
            "the outline and one copy of the line"
        );
        // Outside a part too: two copies in the waste are cut once, with
        // no `toolpaths_overlap` between them.
        let waste = open(&[(150.0, 10.0), (150.0, 40.0)]);
        let p = part_with(vec![waste.clone(), waste]);
        let (tp, warnings) = generate(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::DuplicateShapeDropped]);
        assert_eq!(laps(&tp).len(), 2 * PASS_Z_MM.len());
    }

    // ── #223: a part the bit removes whole ──

    /// A part of one closed outline, `outline`, placed at (20, 20).
    fn part_of(outline: Polyline) -> Project {
        project(vec![outline], &[("i1", at_20_20())])
    }

    /// #223: a part whose outline cut On line lies everywhere within the
    /// tool radius of the line (the part inset by `r` is empty) is removed
    /// whole by the cut: refused with `PartNarrowerThanTool` naming the
    /// outline. That holds for a part too short round for the self-band
    /// check to compare any two of its stretches (a 2.5 mm radius circle,
    /// a 3 mm square, under `2·√2·d` round), and for longer ones it would
    /// otherwise refuse as a band (a 2.9 mm radius circle, a 3 × 50 mm
    /// strip), so the answer no longer depends on the perimeter.
    #[test]
    fn on_line_part_smaller_than_the_bit_is_refused() {
        let cases = [
            (
                "a 2.5 mm radius circle",
                Polyline::circle(Point::new(10.0, 10.0), 2.5),
            ),
            (
                "a 2.9 mm radius circle",
                Polyline::circle(Point::new(10.0, 10.0), 2.9),
            ),
            (
                "a 3 mm square",
                Polyline::rect(Point::new(10.0, 10.0), 3.0, 3.0),
            ),
            (
                "a 3 × 50 mm strip",
                Polyline::rect(Point::new(10.0, 10.0), 50.0, 3.0),
            ),
            (
                "a 6 mm circle drawn the other way",
                Polyline::circle(Point::new(10.0, 10.0), 3.0).reversed(),
            ),
        ];
        for (case, outline) in cases {
            let p = part_of(outline);
            let err = generate_profile(&p, &profile_op(&p, Side::OnLine, CutDirection::Climb))
                .expect_err(case);
            let CamError::PartNarrowerThanTool {
                op,
                shape,
                diameter_mm,
            } = &err
            else {
                panic!("{case}: expected PartNarrowerThanTool, got {err:?}");
            };
            assert_eq!(op.as_str(), "o1", "{case}");
            assert_eq!(shape.instance.as_str(), "i1", "{case}");
            assert_eq!(shape.profile, 0, "{case}");
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
            let text = err.to_string();
            assert!(text.contains("whole part"), "{case}: {text}");
        }
        // A small hole's inside is waste, and a small island in a hole is a
        // part of its own.
        let island = project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 15.0),
                Polyline::circle(Point::new(50.0, 25.0), 2.5),
            ],
            &[("i1", at_20_20())],
        );
        let err = generate_profile(
            &island,
            &profile_op(&island, Side::OnLine, CutDirection::Climb),
        )
        .expect_err("the island");
        assert!(
            matches!(&err, CamError::PartNarrowerThanTool { shape, .. } if shape.profile == 2),
            "{err:?}"
        );
    }

    /// #223: a part a little wider than the bit everywhere (a 3.3 mm
    /// radius circle, a 7 mm square) keeps material inside the cut and is
    /// cut as drawn, as is a 2.5 mm radius hole.
    #[test]
    fn on_line_part_just_larger_than_the_bit_is_cut() {
        let cases = [
            (
                "a 3.3 mm radius circle",
                Polyline::circle(Point::new(10.0, 10.0), 3.3),
            ),
            (
                "a 7 mm square",
                Polyline::rect(Point::new(10.0, 10.0), 7.0, 7.0),
            ),
        ];
        for (case, outline) in cases {
            every_line_cut(case, &part_of(outline));
        }
        every_line_cut(
            "a 2.5 mm radius hole",
            &part_with(vec![Polyline::circle(Point::new(50.0, 25.0), 2.5)]),
        );
    }

    // ── #284: the last pass allowance ──

    const ALLOWANCE_MM: f64 = 0.5;
    /// Acceptance 46: 12 mm stock cut through in 4 mm passes.
    const LAST_STOCK_MM: f64 = 12.0;
    const LAST_PASS_Z_MM: [f64; 3] = [8.0, 4.0, 0.0];
    const LAST_SAFE_Z_MM: f64 = LAST_STOCK_MM + 5.0;

    fn rect_60x40() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 60.0, 40.0)
    }

    /// Acceptance 46's part, `profiles` (the 60 × 40 mm rectangle first)
    /// placed at (20, 20) on 12 mm stock.
    fn last_pass_project_of(profiles: Vec<Polyline>) -> Project {
        let mut p = project(profiles, &[("i1", at_20_20())]);
        p.stock.thickness_mm = Some(LAST_STOCK_MM);
        for instance in &mut p.instances {
            instance.z_top_mm = Some(LAST_STOCK_MM);
        }
        p
    }

    fn last_pass_project() -> Project {
        last_pass_project_of(vec![rect_60x40()])
    }

    fn allowance(allowance_mm: f64) -> LastPass {
        LastPass {
            allowance_mm,
            thickness_mm: None,
            reverse: false,
        }
    }

    /// Acceptance 46's Profile: 12 mm deep in 4 mm passes, climb, with a
    /// 0.5 mm last pass allowance.
    fn last_pass_op(p: &Project, side: Side) -> ProfileOp {
        ProfileOp {
            depth_mm: Some(LAST_STOCK_MM),
            step_down_mm: Some(4.0),
            last_pass: Some(allowance(ALLOWANCE_MM)),
            ..profile_op(p, side, CutDirection::Climb)
        }
    }

    /// The placed outline of the 60 × 40 mm rectangle.
    fn placed_rect() -> Polyline {
        at_20_20().apply_polyline(&rect_60x40()).unwrap()
    }

    /// Distance from `p` to `line`.
    fn gap_mm(line: &Polyline, p: Point) -> f64 {
        nearest_point(line, p).unwrap().gap_mm
    }

    /// The middle of `mv`, from `from`, in XY (arcs along the arc).
    fn middle(from: Point, mv: &Move) -> Point {
        let arc = match mv.kind {
            MoveKind::ArcCcw { centre } => Some((centre, true)),
            MoveKind::ArcCw { centre } => Some((centre, false)),
            _ => None,
        };
        match arc {
            Some((centre, ccw)) => {
                let sweep = sweep_rad(from, mv.to.xy(), centre, ccw);
                let radius = from.distance_to(centre);
                let angle = (from.y - centre.y).atan2(from.x - centre.x) + sweep / 2.0;
                Point::new(
                    centre.x + radius * angle.cos(),
                    centre.y + radius * angle.sin(),
                )
            }
            None => from + (mv.to.xy() - from) * 0.5,
        }
    }

    /// The nearest and farthest any cutting move of `moves` (after the
    /// move at `from`) comes to `line`, sampled at each move's middle and
    /// end. `None` when no move cuts.
    fn gap_range_mm(line: &Polyline, from: Point3, moves: &[Move]) -> Option<(f64, f64)> {
        let mut at = from;
        let mut range: Option<(f64, f64)> = None;
        for mv in moves {
            if !mv.is_rapid() {
                for p in [middle(at.xy(), mv), mv.to.xy()] {
                    let gap = gap_mm(line, p);
                    range = Some(range.map_or((gap, gap), |(lo, hi)| (lo.min(gap), hi.max(gap))));
                }
            }
            at = mv.to;
        }
        range
    }

    /// Where the last pass starts in a one-shape, full-depth-first
    /// toolpath: the rise to safe Z before the first move below
    /// `roughing_floor_mm`, the deepest pass before the last.
    fn last_pass_start(tp: &Toolpath, roughing_floor_mm: f64, safe_z_mm: f64) -> usize {
        let below = tp
            .moves
            .iter()
            .position(|m| m.to.z < roughing_floor_mm - EPS_MM)
            .expect("the last pass goes deeper");
        tp.moves[..below]
            .iter()
            .rposition(|m| m.is_rapid() && m.to.z >= safe_z_mm - EPS_MM)
            .expect("the last pass starts from safe Z")
    }

    /// Acceptance 46: the first two passes run 0.5 mm farther from the
    /// part than the third, which is the path the Profile cuts with no
    /// last pass, move for move; every cut of the first two keeps
    /// `r + 0.5` from the part, and the third cuts the wall at `r`, at
    /// full depth, so it takes an even 0.5 mm skin off the whole wall.
    #[test]
    fn last_pass_allowance_offsets_earlier_passes() {
        let p = last_pass_project();
        let op = last_pass_op(&p, Side::Outside);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let (plain, _) = generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op.clone()
            },
        );
        let laps = laps(&tp);
        let plain_laps = laps_at(&plain, 0.0);
        assert_eq!(laps.len(), 3);
        for (lap, z) in laps.iter().zip(LAST_PASS_Z_MM) {
            assert_approx_eq!(lap.z_mm, z, TOL);
        }
        let grown = 2.0 * (R_MM + ALLOWANCE_MM);
        for lap in &laps[..2] {
            assert_approx_eq!(lap.x_extent_mm(), 60.0 + grown, TOL);
            assert_approx_eq!(lap.y_extent_mm(), 40.0 + grown, TOL);
        }
        assert_approx_eq!(laps[2].x_extent_mm(), 66.35, TOL);
        assert_approx_eq!(laps[2].y_extent_mm(), 46.35, TOL);
        assert_eq!(laps[2].moves, plain_laps[0].moves, "the phase 1 path");

        let outline = placed_rect();
        let split = last_pass_start(&tp, LAST_PASS_Z_MM[1], LAST_SAFE_Z_MM);
        let (near, far) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split]).unwrap();
        assert_approx_eq!(near, R_MM + ALLOWANCE_MM, TOL);
        assert_approx_eq!(far, R_MM + ALLOWANCE_MM, TOL);
        let (near, far) =
            gap_range_mm(&outline, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
        assert_approx_eq!(near, R_MM, TOL);
        assert_approx_eq!(far, R_MM, TOL);
        // The last pass plunges at its own start, through the column the
        // earlier passes cleared, to the full depth.
        assert!(
            tp.moves[split..]
                .iter()
                .any(|m| !m.is_rapid() && (m.to.z - 0.0).abs() <= TOL),
            "the last pass reaches the full depth"
        );
    }

    /// Acceptance 46: `thickness_mm: 1` cuts 4, 8 and 11 mm deep on the
    /// allowance path and only the 12 mm pass on the part's path.
    #[test]
    fn last_pass_thickness_adds_final_depth() {
        let p = last_pass_project();
        let op = ProfileOp {
            last_pass: Some(LastPass {
                thickness_mm: Some(1.0),
                ..allowance(ALLOWANCE_MM)
            }),
            ..last_pass_op(&p, Side::Outside)
        };
        let (tp, _) = generate(&p, &op);
        let laps = laps(&tp);
        let zs: Vec<f64> = laps.iter().map(|l| l.z_mm).collect();
        assert_eq!(laps.len(), 4, "{zs:?}");
        for (z, expected) in zs.iter().zip([8.0, 4.0, 1.0, 0.0]) {
            assert_approx_eq!(*z, expected, TOL);
        }
        for lap in &laps[..3] {
            assert_approx_eq!(lap.x_extent_mm(), 67.35, TOL);
        }
        assert_approx_eq!(laps[3].x_extent_mm(), 66.35, TOL);

        // As thick as a step-down: the usual depths.
        let op = ProfileOp {
            last_pass: Some(LastPass {
                thickness_mm: Some(4.0),
                ..allowance(ALLOWANCE_MM)
            }),
            ..last_pass_op(&p, Side::Outside)
        };
        let usual: Vec<f64> = super::tests::laps(&generate(&p, &op).0)
            .iter()
            .map(|l| l.z_mm)
            .collect();
        assert_eq!(usual.len(), 3, "{usual:?}");
        for (z, expected) in usual.iter().zip(LAST_PASS_Z_MM) {
            assert_approx_eq!(*z, expected, TOL);
        }

        // The coordinator's rule (#310 review): the last pass is never
        // thicker than the step-down, so 10 mm, or all 12 mm, of a cut in
        // 4 mm steps is refused, not cut in one go.
        for thickness_mm in [10.0, LAST_STOCK_MM] {
            let op = ProfileOp {
                last_pass: Some(LastPass {
                    thickness_mm: Some(thickness_mm),
                    ..allowance(ALLOWANCE_MM)
                }),
                ..last_pass_op(&p, Side::Outside)
            };
            let err = generate_profile(&p, &op).expect_err("thicker than the step-down");
            assert!(
                matches!(&err, CamError::InvalidLastPass { field: "thickness_mm", value, .. }
                    if (*value - thickness_mm).abs() <= TOL),
                "{err:?}"
            );
            assert!(err.to_string().contains("4 mm step-down"), "{err}");
        }
    }

    /// Acceptance 46: with `reverse` the last pass runs the other way
    /// round the part (conventional where the others climb); the earlier
    /// passes are unchanged.
    #[test]
    fn last_pass_reverse_flips_direction() {
        let p = last_pass_project();
        let forward = last_pass_op(&p, Side::Outside);
        let reverse = ProfileOp {
            last_pass: Some(LastPass {
                reverse: true,
                ..allowance(ALLOWANCE_MM)
            }),
            ..forward.clone()
        };
        let forward = laps(&generate(&p, &forward).0);
        let reverse = laps(&generate(&p, &reverse).0);
        assert_eq!(reverse.len(), 3);
        for (a, b) in forward.iter().zip(&reverse).take(2) {
            assert_eq!(a.moves, b.moves, "the allowance passes are unchanged");
            assert!(b.area_mm2 < 0.0, "climb outside is clockwise");
        }
        assert!(
            reverse[2].area_mm2 > 0.0,
            "the last pass runs counter-clockwise"
        );
        assert_approx_eq!(reverse[2].area_mm2, -forward[2].area_mm2, 1e-6);
        assert_approx_eq!(reverse[2].x_extent_mm(), 66.35, TOL);
        // It starts where the forward last pass starts.
        assert_eq!(reverse[2].points[0], forward[2].points[0]);
    }

    /// The `[from, to]` X ranges of the level cuts at `top_mm` on the
    /// horizontal line `y_mm`, in toolpath order.
    fn held_on_line(tp: &Toolpath, y_mm: f64, top_mm: f64) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for w in tp.moves.windows(2) {
            let (from, mv) = (w[0].to, w[1]);
            let on_line = (from.y - y_mm).abs() <= 1e-6 && (mv.to.y - y_mm).abs() <= 1e-6;
            let held = (from.z - top_mm).abs() <= EPS_MM && (mv.to.z - top_mm).abs() <= EPS_MM;
            if mv.is_rapid() || !on_line || !held || (from.x - mv.to.x).abs() <= EPS_MM {
                continue;
            }
            let (a, b) = (from.x.min(mv.to.x), from.x.max(mv.to.x));
            match out.last_mut() {
                Some(last) if (a - last.1).abs() <= 1e-6 || (b - last.0).abs() <= 1e-6 => {
                    *last = (last.0.min(a), last.1.max(b));
                }
                _ => out.push((a, b)),
            }
        }
        out
    }

    /// #284 (design A8, step 3): tabs are laid on the part's path, and on
    /// the allowance path each raised stretch is the part path's,
    /// projected and widened by the allowance each way, so every pass
    /// below the tab top keeps off the tab the last pass leaves. Placed
    /// tabs and a request, rect and smooth, with and without
    /// `thickness_mm`: on the bottom edge each allowance pass below the
    /// 5 mm tab top holds the tab top over the last pass's held stretch
    /// and as far beyond it as its cutter could reach the tab (smooth:
    /// over at least the same stretch), and the last pass rises over the
    /// tab too. (changed in the #310 review: the stretch is found from the
    /// tab's material, not widened by `a`;
    /// [`last_pass_tabs_keep_off_the_tab_material`] checks it anywhere.)
    #[test]
    fn last_pass_tabs_cover_same_stretch() {
        const TOP_MM: f64 = 5.0;
        let final_y = 20.0 - R_MM;
        let allowance_y = final_y - ALLOWANCE_MM;
        let p = last_pass_project();
        // The request's distance along the part's path to the middle of
        // the bottom edge, measured from the path's start.
        let path = {
            let centre = offset(&Contour::new(placed_rect()).unwrap(), R_MM).remove(0);
            if centre.is_ccw() {
                centre.reversed().into_polyline()
            } else {
                centre.into_polyline()
            }
        };
        let bottom_mm = nearest_point(&path, Point::new(50.0, final_y))
            .unwrap()
            .along_mm;
        for shape in [TabShape::Rect, TabShape::Smooth] {
            for thickness_mm in [None, Some(1.0)] {
                let requested = ProfileOp {
                    tabs: Some(TabSpec {
                        placement: TabPlacement::AtDistances {
                            distances_mm: vec![bottom_mm],
                        },
                        width_mm: 8.0,
                        height_mm: TOP_MM,
                        shape,
                        on_holes: false,
                        avoid_corners: false,
                    }),
                    last_pass: Some(LastPass {
                        thickness_mm,
                        ..allowance(ALLOWANCE_MM)
                    }),
                    ..last_pass_op(&p, Side::Outside)
                };
                let mut placed = p.clone();
                placed.tabs = vec![dc_project::PlacedTab {
                    id: dc_project::TabId::new("tab1"),
                    instance: InstanceId::new("i1"),
                    profile: 0,
                    x_mm: 30.0,
                    y_mm: 0.0,
                    width_mm: 8.0,
                    height_mm: TOP_MM,
                    shape,
                }];
                let placed_op = ProfileOp {
                    tabs: None,
                    ..requested.clone()
                };
                for (case, project, op) in [
                    ("requested", &p, &requested),
                    ("placed", &placed, &placed_op),
                ] {
                    let case = format!("{case} {shape:?} {thickness_mm:?}");
                    let (tp, _) = generate(project, op);
                    let last = held_on_line(&tp, final_y, TOP_MM);
                    assert_eq!(last.len(), 1, "{case}: the last pass rises: {last:?}");
                    let (fx1, fx2) = last[0];
                    let expected_mm = match shape {
                        TabShape::Rect => 8.0 + 6.35,
                        TabShape::Smooth => 6.35,
                    };
                    assert_approx_eq!(fx2 - fx1, expected_mm, 1e-6);
                    assert_approx_eq!((fx1 + fx2) / 2.0, 50.0, 1e-6);
                    let rough = held_on_line(&tp, allowance_y, TOP_MM);
                    // Passes at Z 4 (and 1) are below the tab top; Z 8 is not.
                    let below = if thickness_mm.is_some() { 2 } else { 1 };
                    assert_eq!(rough.len(), below, "{case}: {rough:?}");
                    for (ax1, ax2) in rough {
                        match shape {
                            // The allowance cutter reaches the tab's
                            // corner where it meets the waste
                            // √(2ar − a²) = 1.71 mm before the stretch,
                            // so it rises that soon, less the sliver of
                            // the corner thinner than a sampling step
                            // (0.26 mm), and not much sooner.
                            TabShape::Rect => {
                                let reach_mm = (2.0 * ALLOWANCE_MM * R_MM
                                    - ALLOWANCE_MM * ALLOWANCE_MM)
                                    .sqrt();
                                for (rise_mm, end) in [(fx1 - ax1, "before"), (ax2 - fx2, "after")]
                                {
                                    assert!(
                                        rise_mm >= reach_mm - 0.26 && rise_mm <= reach_mm + 1.0,
                                        "{case}: raised {rise_mm} mm {end} the stretch"
                                    );
                                }
                            }
                            TabShape::Smooth => {
                                assert!(ax1 <= fx1 + 1e-6 && ax2 >= fx2 - 1e-6, "{case}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// A 5-point star of `outer_mm` and `inner_mm` radii centred at
    /// (40, 40), counter-clockwise from a tip on +X.
    fn star(outer_mm: f64, inner_mm: f64) -> Polyline {
        let points: Vec<Vertex> = (0..10)
            .map(|k| {
                let radius = if k % 2 == 0 { outer_mm } else { inner_mm };
                let angle = f64::from(k) * PI / 5.0;
                Vertex::line(40.0 + radius * angle.cos(), 40.0 + radius * angle.sin())
            })
            .collect();
        Polyline::new(points, true)
    }

    /// A closed outline through `points`.
    fn closed(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            true,
        )
    }

    /// An equilateral triangle of 60 mm sides: three 60° corners.
    fn triangle_60() -> Polyline {
        closed(&[(0.0, 0.0), (60.0, 0.0), (30.0, 30.0 * 3f64.sqrt())])
    }

    /// A right isosceles triangle of 60 mm legs: two 45° corners.
    fn triangle_45() -> Polyline {
        closed(&[(0.0, 0.0), (60.0, 0.0), (0.0, 60.0)])
    }

    /// The 60 × 40 mm rectangle with a V notch of `angle_deg`, 20 mm deep,
    /// in the middle of its top edge.
    fn vee_notched(angle_deg: f64) -> Polyline {
        let half = (angle_deg / 2.0).to_radians().tan() * 20.0;
        closed(&[
            (0.0, 0.0),
            (60.0, 0.0),
            (60.0, 40.0),
            (30.0 + half, 40.0),
            (30.0, 20.0),
            (30.0 - half, 40.0),
            (0.0, 40.0),
        ])
    }

    /// Points along every cutting move of `moves`, from `from`, at most
    /// `step_mm` apart, arcs along the arc, Z interpolated.
    fn cut_points(from: Point3, moves: &[Move], step_mm: f64) -> Vec<Point3> {
        let mut out = Vec::new();
        let mut at = from;
        for mv in moves {
            if !mv.is_rapid() {
                let arc = match mv.kind {
                    MoveKind::ArcCcw { centre } => Some((centre, true)),
                    MoveKind::ArcCw { centre } => Some((centre, false)),
                    _ => None,
                };
                let (length_mm, point_at): (f64, Box<dyn Fn(f64) -> Point>) = match arc {
                    Some((centre, ccw)) => {
                        let sweep = sweep_rad(at.xy(), mv.to.xy(), centre, ccw);
                        let radius = at.xy().distance_to(centre);
                        let start = (at.y - centre.y).atan2(at.x - centre.x);
                        (
                            radius * sweep.abs(),
                            Box::new(move |f: f64| {
                                let angle = start + sweep * f;
                                Point::new(
                                    centre.x + radius * angle.cos(),
                                    centre.y + radius * angle.sin(),
                                )
                            }),
                        )
                    }
                    None => {
                        let (a, b) = (at.xy(), mv.to.xy());
                        (a.distance_to(b), Box::new(move |f: f64| a + (b - a) * f))
                    }
                };
                let steps = (length_mm / step_mm).ceil().max(1.0);
                let mut k = 0.0;
                while k <= steps {
                    let f = k / steps;
                    let p = point_at(f);
                    out.push(Point3::new(p.x, p.y, at.z + (mv.to.z - at.z) * f));
                    k += 1.0;
                }
            }
            at = mv.to;
        }
        out
    }

    /// The independent tab check the #310 review asked for, on the
    /// emitted moves: over every point of the stock the last pass leaves
    /// standing above its floor (a tab's material: within the radius of
    /// its cuts, none of which within the radius is that low), no cut of
    /// an earlier pass comes within the radius below that height. Points
    /// are on a 0.15 mm lattice; the radius is held 0.03 mm short and the
    /// height 0.01 mm, for the lattice and the moves' sampling. Returns
    /// the worst cut below a standing height, mm, and how many lattice
    /// points stand.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lattice indices of coordinates within the stock"
    )]
    fn earlier_cuts_into_tabs(tp: &Toolpath, split: usize, floor_mm: f64) -> (f64, usize) {
        const LATTICE_MM: f64 = 0.15;
        let last = cut_points(tp.moves[split].to, &tp.moves[split + 1..], 0.05);
        let earlier = cut_points(tp.moves[0].to, &tp.moves[..split], 0.05);
        let cell = R_MM;
        let key = |p: Point| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        let mut buckets: BTreeMap<(i64, i64), Vec<Point3>> = BTreeMap::new();
        for p in &last {
            buckets.entry(key(p.xy())).or_default().push(*p);
        }
        let mut earlier_buckets: BTreeMap<(i64, i64), Vec<Point3>> = BTreeMap::new();
        for p in &earlier {
            earlier_buckets.entry(key(p.xy())).or_default().push(*p);
        }
        let near = |map: &BTreeMap<(i64, i64), Vec<Point3>>, p: Point, reach: f64| {
            let (i, j) = key(p);
            let mut found: Vec<Point3> = Vec::new();
            for di in -1..=1 {
                for dj in -1..=1 {
                    if let Some(points) = map.get(&(i + di, j + dj)) {
                        found.extend(points.iter().filter(|q| q.xy().distance_to(p) < reach));
                    }
                }
            }
            found
        };
        // Lattice points round every raised cut of the last pass.
        let mut lattice: std::collections::BTreeSet<(i64, i64)> = Default::default();
        let steps = (R_MM / LATTICE_MM).ceil() as i64;
        for p in last.iter().filter(|p| p.z > floor_mm + 1e-6) {
            let (ci, cj) = (
                (p.x / LATTICE_MM).round() as i64,
                (p.y / LATTICE_MM).round() as i64,
            );
            for di in -steps..=steps {
                for dj in -steps..=steps {
                    lattice.insert((ci + di, cj + dj));
                }
            }
        }
        let mut worst: f64 = 0.0;
        let mut standing = 0;
        for (i, j) in lattice {
            let p = Point::new(i as f64 * LATTICE_MM, j as f64 * LATTICE_MM);
            let cuts = near(&buckets, p, R_MM);
            let Some(height) = cuts.iter().map(|q| q.z).reduce(f64::min) else {
                continue;
            };
            if height <= floor_mm + 1e-6 {
                continue;
            }
            standing += 1;
            for q in near(&earlier_buckets, p, R_MM - 0.03) {
                worst = worst.max(height - 0.01 - q.z);
            }
        }
        (worst, standing)
    }

    /// The #310 review's repro: four 8 × 6 mm tabs, 12 mm deep in 2 mm
    /// steps with a 0.5 mm allowance, so the passes at Z 4 and 2 are
    /// earlier passes below the tab top. The star of radii 35 and 15
    /// Outside and Inside, whose 42° tips the last pass cuts since #324,
    /// and the 60° and 45° triangles both ways. Rect and smooth, requested
    /// and placed: no earlier pass cuts below what the last pass leaves
    /// standing, anywhere over a tab.
    #[test]
    fn last_pass_tabs_keep_off_the_tab_material() {
        for (side, outline) in [
            (Side::Outside, star(35.0, 15.0)),
            (Side::Inside, star(35.0, 15.0)),
            (Side::Inside, triangle_60()),
            (Side::Outside, triangle_60()),
            (Side::Inside, triangle_45()),
            (Side::Outside, triangle_45()),
        ] {
            let mut p = last_pass_project_of(vec![outline]);
            p.stock.width_mm = 300.0;
            for shape in [TabShape::Rect, TabShape::Smooth] {
                let op = ProfileOp {
                    step_down_mm: Some(2.0),
                    tabs: Some(TabSpec {
                        placement: TabPlacement::Count { count: 4 },
                        width_mm: 8.0,
                        height_mm: 6.0,
                        shape,
                        on_holes: false,
                        avoid_corners: false,
                    }),
                    ..last_pass_op(&p, side)
                };
                let placed = crate::placed_tabs::place_requests(&{
                    let mut q = p.clone();
                    q.operations = vec![Operation::Profile(op.clone())];
                    q
                })
                .unwrap()
                .0;
                let placed_op = match &placed.operations[0] {
                    Operation::Profile(op) => op.clone(),
                    _ => unreachable!("a Profile"),
                };
                for (case, project, op) in [("requested", &p, &op), ("placed", &placed, &placed_op)]
                {
                    let case = format!("{side:?} {shape:?} {case}");
                    let (tp, _) = generate(project, op);
                    let split = last_pass_start(&tp, 2.0, LAST_SAFE_Z_MM);
                    let (worst_mm, standing) = earlier_cuts_into_tabs(&tp, split, 0.0);
                    assert!(
                        standing > 100,
                        "{case}: the last pass leaves tabs: {standing}"
                    );
                    assert!(
                        worst_mm <= 0.0,
                        "{case}: an earlier pass cuts {worst_mm} mm into a tab"
                    );
                }
            }
        }
    }

    /// The #310 review, kept by #324: a notch or tip the earlier passes
    /// skip, which the last pass would cut whole at full depth, is
    /// refused: Outside, a 7 × 3 mm notch with 0.5 mm (the last pass
    /// would take 1.0 mm off, as thin as a 60° corner's, but the earlier
    /// passes never enter the notch) and a 10 × 4 mm notch with 2.1 mm;
    /// Inside, a tip of 10° half-angle with 0.5 mm, which the last pass
    /// would cut 2.9 mm thick, more than a third of the bit. Square
    /// corners are cut at any allowance up to a third of the bit: a
    /// rectangle Inside and Outside and an L Outside at 2.1 mm.
    #[test]
    fn last_pass_refuses_a_last_pass_thicker_than_the_allowance() {
        let points = |pts: &[(f64, f64)]| {
            Polyline::new(pts.iter().map(|&(x, y)| Vertex::line(x, y)).collect(), true)
        };
        let notched = |width: f64, depth: f64| {
            let (l, r) = (30.0 - width / 2.0, 30.0 + width / 2.0);
            points(&[
                (0.0, 0.0),
                (60.0, 0.0),
                (60.0, 40.0),
                (r, 40.0),
                (r, 40.0 - depth),
                (l, 40.0 - depth),
                (l, 40.0),
                (0.0, 40.0),
            ])
        };
        let half = 10.0_f64.to_radians().tan() * 30.0;
        let tipped = points(&[
            (0.0, 20.0),
            (30.0, 20.0 - half),
            (30.0, 0.0),
            (70.0, 0.0),
            (70.0, 40.0),
            (30.0, 40.0),
            (30.0, 20.0 + half),
        ]);
        let refused = [
            ("7 × 3 notch", Side::Outside, notched(7.0, 3.0), 0.5),
            ("10 × 4 notch", Side::Outside, notched(10.0, 4.0), 2.1),
            ("10° tip", Side::Inside, tipped.clone(), 0.5),
        ];
        for (case, side, outline, allowance_mm) in refused {
            let p = last_pass_project_of(vec![outline]);
            let op = ProfileOp {
                last_pass: Some(allowance(allowance_mm)),
                ..last_pass_op(&p, side)
            };
            let err = generate_profile(&p, &op).expect_err(case);
            assert!(
                matches!(err, CamError::ShapeNarrowerThanAllowance { .. }),
                "{case}: {err:?}"
            );
            generate(
                &p,
                &ProfileOp {
                    last_pass: None,
                    ..op
                },
            );
        }
        let ell = points(&[
            (0.0, 0.0),
            (60.0, 0.0),
            (60.0, 20.0),
            (30.0, 20.0),
            (30.0, 40.0),
            (0.0, 40.0),
        ]);
        for (case, side, outline) in [
            ("rectangle", Side::Inside, rect_60x40()),
            ("rectangle", Side::Outside, rect_60x40()),
            ("L", Side::Outside, ell),
        ] {
            let p = last_pass_project_of(vec![outline]);
            let op = ProfileOp {
                last_pass: Some(allowance(2.1)),
                ..last_pass_op(&p, side)
            };
            generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case} {side:?}: {e}"));
        }
        // #324: with 0.2 mm the tip's wedge is 1.15 mm, within a third of
        // the bit, and the notch's cut no more than a square corner's.
        for (case, side, outline) in [
            ("10° tip", Side::Inside, tipped),
            ("7 × 3 notch", Side::Outside, notched(7.0, 3.0)),
        ] {
            let p = last_pass_project_of(vec![outline]);
            let op = ProfileOp {
                last_pass: Some(allowance(0.2)),
                ..last_pass_op(&p, side)
            };
            generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case} at 0.2: {e}"));
        }
    }

    /// One squared distance transform along a line of the lattice
    /// (Felzenszwalb and Huttenlocher's lower envelope of parabolas):
    /// `f` is 0 where a point is not removed and huge where it is.
    #[allow(
        clippy::cast_precision_loss,
        reason = "lattice indices of coordinates within the stock"
    )]
    fn squared_distances(f: &[f64]) -> Vec<f64> {
        let n = f.len();
        let mut out = vec![0.0; n];
        if n == 0 {
            return out;
        }
        let at = |q: usize| q as f64;
        let mut v = vec![0usize; n];
        let mut z = vec![0.0; n + 1];
        let mut k = 0usize;
        z[0] = f64::NEG_INFINITY;
        z[1] = f64::INFINITY;
        let meet = |q: usize, p: usize| {
            ((f[q] + at(q) * at(q)) - (f[p] + at(p) * at(p))) / (2.0 * (at(q) - at(p)))
        };
        for q in 1..n {
            let mut s = meet(q, v[k]);
            // z[0] is −∞, so this stops at k = 0.
            while s <= z[k] {
                k -= 1;
                s = meet(q, v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
        }
        k = 0;
        for (q, slot) in out.iter_mut().enumerate() {
            while z[k + 1] < at(q) {
                k += 1;
            }
            let p = v[k];
            *slot = (at(q) - at(p)).powi(2) + f[p];
        }
        out
    }

    /// The #324 swept-area check, on the emitted moves, independent of
    /// the generator's own measure: the stock at `layer_z_mm`, on a
    /// 0.05 mm lattice, cut by every earlier cut through that layer (the
    /// moves before `split`), then by the last pass's (the moves after
    /// it). Returns the diameter of the widest disc in what the last pass
    /// removes there, less one lattice step for the lattice (the thickest
    /// it cuts, anywhere, mm), and the area it removes, mm².
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "lattice indices of coordinates within the stock"
    )]
    fn last_pass_removal(tp: &Toolpath, split: usize, layer_z_mm: f64) -> (f64, f64) {
        const LATTICE_MM: f64 = 0.05;
        let through = |from: Point3, moves: &[Move], step_mm: f64| -> Vec<Point> {
            cut_points(from, moves, step_mm)
                .into_iter()
                .filter(|p| p.z <= layer_z_mm)
                .map(|p| p.xy())
                .collect()
        };
        let earlier = through(tp.moves[0].to, &tp.moves[..split], 0.1);
        let last = through(tp.moves[split].to, &tp.moves[split + 1..], 0.05);
        let margin = R_MM + 2.0 * LATTICE_MM;
        let (mut lo, mut hi) = (
            Point::new(f64::MAX, f64::MAX),
            Point::new(f64::MIN, f64::MIN),
        );
        for p in &last {
            lo = Point::new(lo.x.min(p.x - margin), lo.y.min(p.y - margin));
            hi = Point::new(hi.x.max(p.x + margin), hi.y.max(p.y + margin));
        }
        let nx = ((hi.x - lo.x) / LATTICE_MM).ceil() as usize + 1;
        let ny = ((hi.y - lo.y) / LATTICE_MM).ceil() as usize + 1;
        // 0: stock; 1: cut before the last pass; 2: cut by the last pass.
        let mut cells = vec![0u8; nx * ny];
        let stamp = |cells: &mut [u8], c: Point, set: u8| {
            let j0 = ((c.y - R_MM - lo.y) / LATTICE_MM).floor().max(0.0) as usize;
            let j1 = (((c.y + R_MM - lo.y) / LATTICE_MM).ceil() as usize).min(ny - 1);
            for j in j0..=j1 {
                let dy = lo.y + j as f64 * LATTICE_MM - c.y;
                let reach = R_MM * R_MM - dy * dy;
                if reach < 0.0 {
                    continue;
                }
                let half = reach.sqrt();
                let i0 = ((c.x - half - lo.x) / LATTICE_MM).ceil().max(0.0) as usize;
                let i1 = (((c.x + half - lo.x) / LATTICE_MM).floor() as usize).min(nx - 1);
                for cell in &mut cells[j * nx + i0..=j * nx + i1] {
                    if *cell == 0 {
                        *cell = set;
                    }
                }
            }
        };
        for p in &earlier {
            stamp(&mut cells, *p, 1);
        }
        for p in &last {
            stamp(&mut cells, *p, 2);
        }
        let removed = cells.iter().filter(|&&c| c == 2).count();
        // More than any squared distance on the lattice.
        let far = ((nx + ny) as f64).powi(2);
        let mut f: Vec<f64> = cells
            .iter()
            .map(|&c| if c == 2 { far } else { 0.0 })
            .collect();
        for row in f.chunks_mut(nx) {
            let d = squared_distances(row);
            row.copy_from_slice(&d);
        }
        for i in 0..nx {
            let column: Vec<f64> = (0..ny).map(|j| f[j * nx + i]).collect();
            for (j, d) in squared_distances(&column).into_iter().enumerate() {
                f[j * nx + i] = d;
            }
        }
        let widest = f.iter().copied().fold(0.0, f64::max).sqrt();
        (
            (2.0 * widest - 1.0).max(0.0) * LATTICE_MM,
            removed as f64 * LATTICE_MM * LATTICE_MM,
        )
    }

    /// #324: sharp corners are cut, not refused. Inside the 60° and 45°
    /// triangles and the star of radii 35 and 15 (42° tips), Outside that
    /// star and 80° and 60° V notches, at 0.2 and 0.5 mm with the 6.35 mm
    /// bit: each is generated (and passes `assert_safe`), the earlier
    /// passes keep `r + a` from the part and the last pass exactly `r`.
    /// The material check ([`last_pass_removal`]) finds the last pass
    /// cutting the corner's wedge, `a / sin(θ/2)` thick, and nowhere more
    /// than the bound: a third of the bit, or `√2 · a + 0.05` where that
    /// is more.
    #[test]
    fn last_pass_allows_sharp_corners() {
        let bound_mm = |a: f64| (6.35 / 3.0_f64).max(2f64.sqrt() * a + 0.05);
        let star_tip_deg = 2.0
            * (15.0 * (0.2 * PI).sin())
                .atan2(35.0 - 15.0 * (0.2 * PI).cos())
                .to_degrees();
        for (case, side, outline, corner_deg) in [
            ("60° triangle", Side::Inside, triangle_60(), 60.0),
            ("45° triangle", Side::Inside, triangle_45(), 45.0),
            ("star", Side::Inside, star(35.0, 15.0), star_tip_deg),
            ("star", Side::Outside, star(35.0, 15.0), 114.2),
            ("80° V notch", Side::Outside, vee_notched(80.0), 80.0),
            ("60° V notch", Side::Outside, vee_notched(60.0), 60.0),
        ] {
            let part = at_20_20().apply_polyline(&outline).unwrap();
            for a in [0.2, 0.5] {
                let case = format!("{case} {side:?} at {a} mm");
                let p = last_pass_project_of(vec![outline.clone()]);
                let op = ProfileOp {
                    last_pass: Some(allowance(a)),
                    ..last_pass_op(&p, side)
                };
                let (tp, _) = generate_profile(&p, &op).unwrap_or_else(|e| panic!("{case}: {e}"));
                assert_safe(&tp, &p, &Operation::Profile(op.clone()));
                let split = last_pass_start(&tp, LAST_PASS_Z_MM[1], LAST_SAFE_Z_MM);
                let (near, _) = gap_range_mm(&part, tp.moves[0].to, &tp.moves[..split]).unwrap();
                assert!(near >= R_MM + a - 1e-3, "{case}: an earlier pass at {near}");
                let (near, _) =
                    gap_range_mm(&part, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
                assert!(
                    (near - R_MM).abs() <= 1e-3,
                    "{case}: the last pass at {near}"
                );
                let (widest_mm, removed_mm2) =
                    last_pass_removal(&tp, split, LAST_PASS_Z_MM[1] + 0.01);
                let wedge_mm = a / (corner_deg / 2.0_f64).to_radians().sin();
                assert!(removed_mm2 > 0.0, "{case}");
                assert!(
                    widest_mm >= wedge_mm.max(a) - 0.15,
                    "{case}: the last pass cuts {widest_mm} mm, not the {wedge_mm} mm wedge"
                );
                assert!(
                    widest_mm <= bound_mm(a) + 0.1,
                    "{case}: the last pass cuts {widest_mm} mm thick, over {}",
                    bound_mm(a)
                );
            }
        }
    }

    /// #310 review: a cut of one pass has no pass before the last to
    /// leave the allowance, so it cuts as without a last pass and warns
    /// `last_pass_unused`.
    #[test]
    fn last_pass_on_a_one_pass_cut_warns_unused() {
        let p = rect_project();
        let op = ProfileOp {
            depth_mm: Some(2.0),
            ..profile_op(&p, Side::Outside, CutDirection::Climb)
        };
        let (plain, plain_warnings) = generate(&p, &op);
        assert!(plain_warnings.is_empty(), "{plain_warnings:?}");
        let (with, warnings) = generate(
            &p,
            &ProfileOp {
                last_pass: Some(allowance(ALLOWANCE_MM)),
                ..op
            },
        );
        assert_eq!(with, plain);
        assert_eq!(codes(&warnings), vec![WarningCode::LastPassUnused]);
        assert!(
            warnings[0].to_string().contains("one pass"),
            "{}",
            warnings[0]
        );
    }

    /// #284: Inside, an island 7 mm from the outline leaves room for the
    /// 6.35 mm bit but not for the bit plus a 0.5 mm allowance on both
    /// lines (7.35 mm): refused, naming the allowance, where the plain
    /// cut and a 0.3 mm allowance are cut.
    #[test]
    fn last_pass_band_too_narrow_for_allowance() {
        let mut p = ring_project(25.0 - 7.0);
        p.stock.thickness_mm = Some(LAST_STOCK_MM);
        let op = |allowance_mm: Option<f64>| ProfileOp {
            last_pass: allowance_mm.map(allowance),
            ..last_pass_op(&p, Side::Inside)
        };
        generate(&p, &op(None));
        generate(&p, &op(Some(0.3)));
        let err = generate_profile(&p, &op(Some(ALLOWANCE_MM)))
            .expect_err("a band narrower than the bit and allowance is refused");
        let CamError::BandNarrowerThanAllowance {
            op: id,
            shape,
            other,
            band_mm,
            diameter_mm,
            allowance_mm,
            needed_mm,
        } = &err
        else {
            panic!("expected BandNarrowerThanAllowance, got {err:?}");
        };
        assert_eq!(id.as_str(), "o1");
        assert_eq!((shape.profile, other.profile), (0, 1), "outline first");
        assert_approx_eq!(*band_mm, 7.0, TOL);
        assert_approx_eq!(*diameter_mm, 6.35, TOL);
        assert_approx_eq!(*allowance_mm, ALLOWANCE_MM, TOL);
        assert_approx_eq!(*needed_mm, 7.35, TOL);
        let text = err.to_string();
        for needle in [
            "`o1`",
            "7.000 mm",
            "7.350 mm",
            "6.35 mm",
            "0.5 mm",
            "allowance",
        ] {
            assert!(text.contains(needle), "{needle:?} in {text}");
        }
    }

    /// #284: Inside a shape whose neck, or whose slot, is wider than the
    /// bit but narrower than the bit plus the allowance on both sides,
    /// the allowance passes would skip it and leave the last pass a full
    /// slot at full depth there; a hole that only the bit fits leaves the
    /// allowance passes no path at all. Each is refused naming the shape
    /// and the allowance; a 0.2 mm allowance fits all three.
    #[test]
    fn last_pass_neck_slot_or_hole_too_narrow_for_allowance() {
        let points = |pts: &[(f64, f64)]| {
            Polyline::new(pts.iter().map(|&(x, y)| Vertex::line(x, y)).collect(), true)
        };
        // Two 30 mm squares joined by a 7 mm wide neck.
        let dumbbell = points(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 11.5),
            (40.0, 11.5),
            (40.0, 0.0),
            (70.0, 0.0),
            (70.0, 30.0),
            (40.0, 30.0),
            (40.0, 18.5),
            (30.0, 18.5),
            (30.0, 30.0),
            (0.0, 30.0),
        ]);
        // A 60 × 40 mm rectangle with a 7 mm wide, 20 mm long slot.
        let slotted = points(&[
            (0.0, 0.0),
            (60.0, 0.0),
            (60.0, 40.0),
            (33.5, 40.0),
            (33.5, 60.0),
            (26.5, 60.0),
            (26.5, 40.0),
            (0.0, 40.0),
        ]);
        let small_hole = Polyline::rect(Point::new(0.0, 0.0), 7.0, 7.0);
        for (case, outline) in [("neck", dumbbell), ("slot", slotted), ("hole", small_hole)] {
            let p = last_pass_project_of(vec![outline]);
            let op = last_pass_op(&p, Side::Inside);
            let err = generate_profile(&p, &op).expect_err(case);
            let CamError::ShapeNarrowerThanAllowance {
                shape,
                diameter_mm,
                allowance_mm,
                ..
            } = &err
            else {
                panic!("{case}: expected ShapeNarrowerThanAllowance, got {err:?}");
            };
            assert_eq!(shape.profile, 0, "{case}");
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
            assert_approx_eq!(*allowance_mm, ALLOWANCE_MM, TOL);
            assert!(err.to_string().contains("allowance"), "{case}: {err}");
            generate(
                &p,
                &ProfileOp {
                    last_pass: Some(allowance(0.2)),
                    ..op.clone()
                },
            );
            generate(
                &p,
                &ProfileOp {
                    last_pass: None,
                    ..op
                },
            );
        }
    }

    /// #284: Outside, a part inside another part's 40 mm hole, 6.6 mm
    /// from its walls: the plain cut keeps the radius from every line,
    /// but the part's allowance passes, 0.5 mm farther out, would cut
    /// 0.25 mm into the frame round the hole, which S9 rejects. Refused,
    /// naming the hole and the part; a 0.2 mm allowance fits.
    #[test]
    fn last_pass_allowance_into_a_neighbour_is_refused() {
        let p = last_pass_project_of(vec![
            Polyline::rect(Point::new(0.0, 0.0), 80.0, 80.0),
            Polyline::rect(Point::new(20.0, 20.0), 40.0, 40.0),
            Polyline::rect(Point::new(26.6, 26.6), 26.8, 26.8),
        ]);
        let op = last_pass_op(&p, Side::Outside);
        let (_, warnings) = generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op.clone()
            },
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        generate(
            &p,
            &ProfileOp {
                last_pass: Some(allowance(0.2)),
                ..op.clone()
            },
        );
        let err = generate_profile(&p, &op).expect_err("the allowance passes would gouge");
        let CamError::BandNarrowerThanAllowance {
            shape,
            other,
            band_mm,
            ..
        } = &err
        else {
            panic!("expected BandNarrowerThanAllowance, got {err:?}");
        };
        assert_eq!((shape.profile, other.profile), (1, 2), "{err}");
        assert_approx_eq!(*band_mm, 6.6, 1e-9);

        // The #310 review's property case: 0.3 mm from the hole's wall,
        // the part's laps already cut into each other (D6 warns), and its
        // allowance passes run wholly inside the frame, and the hole's
        // inside the part, more than a radius past either line.
        let p = last_pass_project_of(vec![
            Polyline::rect(Point::new(0.0, 0.0), 80.0, 80.0),
            Polyline::rect(Point::new(20.0, 20.0), 40.0, 40.0),
            Polyline::rect(Point::new(20.3, 20.3), 39.4, 39.4),
        ]);
        let op = last_pass_op(&p, Side::Outside);
        let (_, warnings) = generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op.clone()
            },
        );
        assert!(
            codes(&warnings).contains(&WarningCode::ToolpathsOverlap),
            "{warnings:?}"
        );
        let err = generate_profile(&p, &op).expect_err("the allowance passes would cut a part");
        assert!(
            matches!(err, CamError::BandNarrowerThanAllowance { .. }),
            "{err:?}"
        );
    }

    /// Acceptance 46 Inside: the first two passes run 0.5 mm farther
    /// into the waste than the third, which is the plain Inside path.
    #[test]
    fn last_pass_inside_rectangle() {
        let p = last_pass_project();
        let op = last_pass_op(&p, Side::Inside);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let (plain, _) = generate(
            &p,
            &ProfileOp {
                last_pass: None,
                ..op.clone()
            },
        );
        let laps = laps(&tp);
        assert_eq!(laps.len(), 3);
        let shrunk = 2.0 * (R_MM + ALLOWANCE_MM);
        for lap in &laps[..2] {
            assert_approx_eq!(lap.x_extent_mm(), 60.0 - shrunk, TOL);
            assert_approx_eq!(lap.y_extent_mm(), 40.0 - shrunk, TOL);
            assert!(lap.area_mm2 > 0.0, "climb inside is counter-clockwise");
        }
        assert_approx_eq!(laps[2].x_extent_mm(), 60.0 - 6.35, TOL);
        assert_eq!(laps[2].moves, laps_at(&plain, 0.0)[0].moves);
        let outline = placed_rect();
        let split = last_pass_start(&tp, LAST_PASS_Z_MM[1], LAST_SAFE_Z_MM);
        let (near, _) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split]).unwrap();
        assert_approx_eq!(near, R_MM + ALLOWANCE_MM, TOL);
        let (near, far) =
            gap_range_mm(&outline, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
        assert_approx_eq!(near, R_MM, TOL);
        assert_approx_eq!(far, R_MM, TOL);
    }

    /// #284: with `last_pass: null` the toolpath is the one the Profile
    /// cut before: move for move the phase 1 recipe of
    /// [`one_shape_golden_is_byte_identical_after_sequence`], and the same
    /// as with a last pass whose cut has only one pass, which the last
    /// pass cuts on the part's path. (`dreamcarve`'s migration corpus
    /// holds whole programs to the bytes the build before #284 wrote.)
    #[test]
    fn no_last_pass_is_byte_identical() {
        let p = rect_project();
        let op = profile_op(&p, Side::Outside, CutDirection::Climb);
        assert_eq!(op.last_pass, None);
        let (plain, _) = generate(&p, &op);
        let placed = at_20_20().apply_polyline(&rect_100x50()).unwrap();
        let centre = offset(&Contour::new(placed).unwrap(), R_MM).remove(0);
        let path = if centre.is_ccw() {
            centre.reversed().into_polyline()
        } else {
            centre.into_polyline()
        };
        let start = path.vertices()[0].point();
        let laps = laps(&plain);
        assert_eq!(laps.len(), PASS_Z_MM.len());
        for lap in &laps {
            assert_eq!(lap.points[0], start);
            assert_eq!(lap.moves.len(), path.segments().count());
        }
        // One pass in all: the last pass is the cut, as without one.
        let deep = ProfileOp {
            depth_mm: Some(2.0),
            ..op.clone()
        };
        let (one, _) = generate(&p, &deep);
        let (with, _) = generate(
            &p,
            &ProfileOp {
                last_pass: Some(allowance(ALLOWANCE_MM)),
                ..deep
            },
        );
        assert_eq!(one, with);
    }

    /// #284 with leads, ramps, helixes and corner relief, Inside and
    /// Outside, climb and conventional: every cut of the
    /// allowance passes, their descents and lead arcs included, keeps
    /// `r + a` from the part (within the waste domain's slack); the last
    /// pass visits every vertex of the part's path at full depth; relief
    /// notches are cut from the last pass, never into the allowance
    /// skin; and the whole toolpath is safe (S1–S3: depth, rapids, every
    /// feed and plunge rate the tool's).
    #[test]
    fn last_pass_with_leads_ramps_and_relief() {
        let lead = Lead {
            radius_mm: 4.0,
            sweep_deg: 90.0,
            lead_out: true,
            overcut_mm: 1.0,
        };
        let helix = Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        };
        let outline = placed_rect();
        let p = last_pass_project();
        for side in [Side::Outside, Side::Inside] {
            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                for (entry, lead, relief) in [
                    (ramp_15(), None, CornerRelief::None),
                    (ramp_15(), Some(lead.clone()), CornerRelief::None),
                    (helix.clone(), Some(lead.clone()), CornerRelief::Dogbone),
                    (Entry::Plunge, None, CornerRelief::TBone),
                ] {
                    let case = format!("{side:?} {direction:?} {entry:?} {lead:?} {relief:?}");
                    let op = ProfileOp {
                        direction,
                        entry,
                        lead,
                        corner_relief: relief,
                        ..last_pass_op(&p, side)
                    };
                    let (tp, _) = generate(&p, &op);
                    let split = last_pass_start(&tp, LAST_PASS_Z_MM[1], LAST_SAFE_Z_MM);
                    let (near, _) = gap_range_mm(&outline, tp.moves[0].to, &tp.moves[..split])
                        .unwrap_or_else(|| panic!("{case}: the allowance passes cut"));
                    assert!(
                        near >= R_MM + ALLOWANCE_MM - 1e-3,
                        "{case}: an allowance pass comes {near} mm from the part"
                    );
                    let centre = match side {
                        Side::Inside => -R_MM,
                        _ => R_MM,
                    };
                    let path = offset(&Contour::new(outline.clone()).unwrap(), centre).remove(0);
                    let last = &tp.moves[split..];
                    for v in path.polyline().vertices() {
                        assert!(
                            last.iter().any(|m| {
                                !m.is_rapid()
                                    && m.to.z.abs() <= TOL
                                    && m.to.xy().distance_to(v.point()) <= 1e-6
                            }) || last.windows(2).any(|w| {
                                !w[1].is_rapid()
                                    && w[0].to.z.abs() <= TOL
                                    && w[1].to.z.abs() <= TOL
                                    && dc_geometry::point_segment_distance_mm(
                                        dc_geometry::Segment {
                                            start: w[0].to.xy(),
                                            end: w[1].to.xy(),
                                            bulge: 0.0,
                                        },
                                        v.point(),
                                    ) <= 1e-6
                            }),
                            "{case}: the last pass misses {:?} at full depth",
                            v.point()
                        );
                    }
                    if relief != CornerRelief::None && side == Side::Inside {
                        // A notch reaches past the part's path towards each
                        // corner, at full depth, after the allowance passes.
                        let corners = outline.vertices().len();
                        let notched = outline
                            .vertices()
                            .iter()
                            .filter(|c| {
                                last.iter().any(|m| {
                                    !m.is_rapid()
                                        && m.to.z.abs() <= TOL
                                        && m.to.xy().distance_to(c.point())
                                            < R_MM * 2f64.sqrt() - 0.1
                                })
                            })
                            .count();
                        assert_eq!(notched, corners, "{case}");
                    }
                }
            }
        }
    }

    // ── #311: the last pass on Left and Right Profiles ──

    /// The 60 mm open line of #311's tests, placed from (20, 20) to
    /// (80, 20).
    fn line_60() -> Polyline {
        open(&[(0.0, 0.0), (60.0, 0.0)])
    }

    /// An open L: 60 mm along +X, then 40 mm up, a left turn. Left cuts
    /// inside its corner, Right round it.
    fn ell_path() -> Polyline {
        open(&[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0)])
    }

    /// An open V: 50 mm along +X, then 50 mm back at `angle_deg` to the
    /// first arm, a left turn. Left cuts inside its tip, Right round it.
    fn vee_path(angle_deg: f64) -> Polyline {
        let angle = angle_deg.to_radians();
        open(&[
            (0.0, 0.0),
            (50.0, 0.0),
            (50.0 - 50.0 * angle.cos(), 50.0 * angle.sin()),
        ])
    }

    /// Which way `side` puts the tool from a line drawn along +X: +1 for
    /// Left (+Y), −1 for Right.
    fn side_sign(side: Side) -> f64 {
        if side == Side::Left { 1.0 } else { -1.0 }
    }

    /// The direction that cuts `side` from the end beside the first
    /// vertex (#287): Left climb, Right conventional.
    fn from_first_vertex(side: Side) -> CutDirection {
        if side == Side::Left {
            CutDirection::Climb
        } else {
            CutDirection::Conventional
        }
    }

    /// Acceptance 46's Profile (12 mm deep in 4 mm passes, a 0.5 mm last
    /// pass allowance) cut on `side` with `direction`.
    fn side_last_pass_op(p: &Project, side: Side, direction: CutDirection) -> ProfileOp {
        ProfileOp {
            direction,
            ..last_pass_op(p, side)
        }
    }

    /// #311: a 60 mm open line cut Left, 12 mm deep in 4 mm passes with a
    /// 0.5 mm allowance: passes 1–2 run `r + 0.5` to the left, pass 3 is
    /// the plain Left path, move for move. Every pass starts and ends on
    /// the normals through the line's ends (X 20 and 80): the allowance
    /// passes are neither shorter nor longer than the last. Right is the
    /// mirror, and the other direction runs each pass from the other end.
    #[test]
    fn last_pass_left_offsets_earlier_passes() {
        let p = last_pass_project_of(vec![line_60()]);
        for side in [Side::Left, Side::Right] {
            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                let case = format!("{side:?} {direction:?}");
                let op = side_last_pass_op(&p, side, direction);
                let (tp, warnings) = generate(&p, &op);
                assert!(warnings.is_empty(), "{case}: {warnings:?}");
                let (plain, _) = generate(
                    &p,
                    &ProfileOp {
                        last_pass: None,
                        ..op.clone()
                    },
                );
                let (start, end) = if direction == from_first_vertex(side) {
                    (20.0, 80.0)
                } else {
                    (80.0, 20.0)
                };
                let laps = laps(&tp);
                assert_eq!(laps.len(), 3, "{case}: {laps:?}");
                for (k, (lap, z)) in laps.iter().zip(LAST_PASS_Z_MM).enumerate() {
                    assert_approx_eq!(lap.z_mm, z, TOL);
                    let away_mm = if k < 2 { R_MM + ALLOWANCE_MM } else { R_MM };
                    let y = 20.0 + side_sign(side) * away_mm;
                    for q in &lap.points {
                        assert!(
                            (q.y - y).abs() <= TOL,
                            "{case}: pass {k} at {q:?}, not Y {y}"
                        );
                    }
                    let (first, last) = (lap.points[0], lap.points[lap.points.len() - 1]);
                    assert!(
                        (first.x - start).abs() <= TOL && (last.x - end).abs() <= TOL,
                        "{case}: pass {k} runs {first:?} to {last:?}, not X {start} to {end}"
                    );
                }
                assert_eq!(
                    laps[2].moves,
                    laps_at(&plain, 0.0)[0].moves,
                    "{case}: the plain path"
                );
            }
        }
    }

    /// #311: an open path's tabs are laid on its last pass's path and
    /// carried onto the allowance path as on a closed shape
    /// (`last_pass::twin_tabs`, the #310 review's rule: the issue's
    /// "widened by a" was #284's, which it replaced). The 60 mm line with
    /// a 5 mm tab 8 mm wide at its middle (X 50), requested and placed,
    /// rect and smooth, Left and Right, cut from either end: the last pass
    /// holds the tab top over the tab plus the bit (smooth: the bit),
    /// centred on X 50, and the allowance pass at Z 4, the only earlier
    /// pass below the tab top, over that stretch and, rect, as far beyond
    /// it as its cutter reaches the tab's corner, `√(2ar − a²)`.
    #[test]
    fn last_pass_left_right_tabs_cover_same_stretch() {
        const TOP_MM: f64 = 5.0;
        let p = last_pass_project_of(vec![line_60()]);
        let reach_mm = (2.0 * ALLOWANCE_MM * R_MM - ALLOWANCE_MM * ALLOWANCE_MM).sqrt();
        for side in [Side::Left, Side::Right] {
            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                for shape in [TabShape::Rect, TabShape::Smooth] {
                    let requested = ProfileOp {
                        tabs: Some(TabSpec {
                            placement: TabPlacement::AtDistances {
                                distances_mm: vec![30.0],
                            },
                            width_mm: 8.0,
                            height_mm: TOP_MM,
                            shape,
                            on_holes: false,
                            avoid_corners: false,
                        }),
                        ..side_last_pass_op(&p, side, direction)
                    };
                    let mut placed = p.clone();
                    placed.tabs = vec![dc_project::PlacedTab {
                        id: dc_project::TabId::new("tab1"),
                        instance: InstanceId::new("i1"),
                        profile: 0,
                        x_mm: 30.0,
                        y_mm: 0.0,
                        width_mm: 8.0,
                        height_mm: TOP_MM,
                        shape,
                    }];
                    let placed_op = ProfileOp {
                        tabs: None,
                        ..requested.clone()
                    };
                    for (how, project, op) in [
                        ("requested", &p, &requested),
                        ("placed", &placed, &placed_op),
                    ] {
                        let case = format!("{side:?} {direction:?} {shape:?} {how}");
                        let (tp, _) = generate(project, op);
                        let final_y = 20.0 + side_sign(side) * R_MM;
                        let allowance_y = 20.0 + side_sign(side) * (R_MM + ALLOWANCE_MM);
                        let last = held_on_line(&tp, final_y, TOP_MM);
                        assert_eq!(last.len(), 1, "{case}: the last pass rises: {last:?}");
                        let (fx1, fx2) = last[0];
                        let expected_mm = match shape {
                            TabShape::Rect => 8.0 + 6.35,
                            TabShape::Smooth => 6.35,
                        };
                        assert_approx_eq!(fx2 - fx1, expected_mm, 1e-6);
                        assert_approx_eq!((fx1 + fx2) / 2.0, 50.0, 1e-6);
                        let rough = held_on_line(&tp, allowance_y, TOP_MM);
                        assert_eq!(rough.len(), 1, "{case}: {rough:?}");
                        let (ax1, ax2) = rough[0];
                        match shape {
                            TabShape::Rect => {
                                for (rise_mm, end) in [(fx1 - ax1, "before"), (ax2 - fx2, "after")]
                                {
                                    assert!(
                                        rise_mm >= reach_mm - 0.26 && rise_mm <= reach_mm + 1.0,
                                        "{case}: raised {rise_mm} mm {end} the stretch"
                                    );
                                }
                            }
                            TabShape::Smooth => {
                                assert!(ax1 <= fx1 + 1e-6 && ax2 >= fx2 - 1e-6, "{case}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// #311, the #310 review's tab check on open paths: three 8 × 6 mm
    /// tabs on an L and on a 60° V, cut Left (inside the corner, where
    /// the allowance path runs past the corner beside the other arm's
    /// tabs) and Right (round it), 12 mm deep in 2 mm steps, rect and
    /// smooth, requested and placed: no earlier pass cuts below what the
    /// last pass leaves standing, anywhere over a tab.
    #[test]
    fn last_pass_left_right_tabs_keep_off_the_tab_material() {
        for (name, path) in [("L", ell_path()), ("60° V", vee_path(60.0))] {
            let p = last_pass_project_of(vec![path]);
            for side in [Side::Left, Side::Right] {
                for shape in [TabShape::Rect, TabShape::Smooth] {
                    let op = ProfileOp {
                        step_down_mm: Some(2.0),
                        tabs: Some(TabSpec {
                            placement: TabPlacement::Count { count: 3 },
                            width_mm: 8.0,
                            height_mm: 6.0,
                            shape,
                            on_holes: false,
                            avoid_corners: false,
                        }),
                        ..side_last_pass_op(&p, side, CutDirection::Climb)
                    };
                    let placed = crate::placed_tabs::place_requests(&{
                        let mut q = p.clone();
                        q.operations = vec![Operation::Profile(op.clone())];
                        q
                    })
                    .unwrap()
                    .0;
                    let placed_op = match &placed.operations[0] {
                        Operation::Profile(op) => op.clone(),
                        _ => unreachable!("a Profile"),
                    };
                    for (how, project, op) in
                        [("requested", &p, &op), ("placed", &placed, &placed_op)]
                    {
                        let case = format!("{name} {side:?} {shape:?} {how}");
                        let (tp, _) = generate(project, op);
                        let split = last_pass_start(&tp, 2.0, LAST_SAFE_Z_MM);
                        let (worst_mm, standing) = earlier_cuts_into_tabs(&tp, split, 0.0);
                        assert!(
                            standing > 100,
                            "{case}: the last pass leaves tabs: {standing}"
                        );
                        assert!(
                            worst_mm <= 0.0,
                            "{case}: an earlier pass cuts {worst_mm} mm into a tab"
                        );
                    }
                }
            }
        }
    }

    /// #311: with `reverse` an open path's last pass runs from its other
    /// end, so it mills the other way, as a closed shape's does: from X 80
    /// to X 20 where the earlier passes run from X 20 to X 80. The
    /// earlier passes are unchanged, and a tab 20 mm from the first
    /// vertex (X 40) is held over the same stretch either way.
    #[test]
    fn last_pass_left_right_reverse_runs_from_the_other_end() {
        const TOP_MM: f64 = 5.0;
        let p = last_pass_project_of(vec![line_60()]);
        for side in [Side::Left, Side::Right] {
            for direction in [CutDirection::Climb, CutDirection::Conventional] {
                let case = format!("{side:?} {direction:?}");
                let forward = side_last_pass_op(&p, side, direction);
                let reverse = ProfileOp {
                    last_pass: Some(LastPass {
                        reverse: true,
                        ..allowance(ALLOWANCE_MM)
                    }),
                    ..forward.clone()
                };
                let f = laps(&generate(&p, &forward).0);
                let r = laps(&generate(&p, &reverse).0);
                assert_eq!(r.len(), 3, "{case}");
                for (a, b) in f.iter().zip(&r).take(2) {
                    assert_eq!(a.moves, b.moves, "{case}: the earlier passes");
                }
                let (fa, fb) = (f[2].points[0], f[2].points[f[2].points.len() - 1]);
                let (ra, rb) = (r[2].points[0], r[2].points[r[2].points.len() - 1]);
                assert!(
                    ra.distance_to(fb) <= TOL && rb.distance_to(fa) <= TOL,
                    "{case}: {fa:?}–{fb:?} reversed is {ra:?}–{rb:?}"
                );
                // It starts where the earlier passes end.
                let earlier_end = r[1].points[r[1].points.len() - 1];
                assert!(
                    (ra.x - earlier_end.x).abs() <= TOL,
                    "{case}: {ra:?} {earlier_end:?}"
                );

                let tabbed = |op: &ProfileOp| ProfileOp {
                    tabs: Some(TabSpec {
                        placement: TabPlacement::AtDistances {
                            distances_mm: vec![20.0],
                        },
                        width_mm: 8.0,
                        height_mm: TOP_MM,
                        shape: TabShape::Rect,
                        on_holes: false,
                        avoid_corners: false,
                    }),
                    ..op.clone()
                };
                let final_y = 20.0 + side_sign(side) * R_MM;
                let held = |op: &ProfileOp| held_on_line(&generate(&p, op).0, final_y, TOP_MM);
                let (f, r) = (held(&tabbed(&forward)), held(&tabbed(&reverse)));
                assert_eq!(f.len(), 1, "{case}: {f:?}");
                assert_eq!(r.len(), 1, "{case}: {r:?}");
                assert_approx_eq!(f[0].0, r[0].0, 1e-6);
                assert_approx_eq!(f[0].1, r[0].1, 1e-6);
                assert_approx_eq!((f[0].0 + f[0].1) / 2.0, 40.0, 1e-6);
            }
        }
    }

    /// #311: every #284 refusal that applies to an open path cut on its
    /// side. Each case is cut without a last pass and with 0.2 mm, and
    /// refused at 0.5 mm:
    ///
    /// - narrowness: the inside of a U 7 mm wide fits the bit but not the
    ///   bit and the allowance on both arms (7.35 mm), so the allowance
    ///   offset vanishes (`ShapeNarrowerThanAllowance`);
    /// - corners only: a 7 × 3 mm notch the allowance passes bridge, which
    ///   the last pass would cut whole (`ShapeNarrowerThanAllowance`);
    /// - the thickness cap: the tip of a 20° V, whose wedge is 2.88 mm at
    ///   0.5 mm, more than a third of the bit (`ShapeNarrowerThanAllowance`);
    /// - neighbours: two lines 6.85 mm apart cut towards each other, where
    ///   each allowance pass would take the other line's skin, and a line
    ///   10.3 mm below another, cut towards its kept side, where the
    ///   allowance pass would come within `r + a` of its kept strip
    ///   (`AllowanceCutsIntoKeptSide`, the lines in drawing order).
    ///
    /// A square of four lines cut on its outside, whose lines meet at its
    /// corners, is cut at 0.5 mm.
    #[test]
    fn last_pass_left_right_refuses_what_the_earlier_passes_cannot_follow() {
        let u = open(&[(0.0, 0.0), (30.0, 0.0), (30.0, 7.0), (0.0, 7.0)]);
        let notch = open(&[
            (0.0, 0.0),
            (26.5, 0.0),
            (26.5, -3.0),
            (33.5, -3.0),
            (33.5, 0.0),
            (60.0, 0.0),
        ]);
        let with = |p: &Project, allowance_mm: Option<f64>| ProfileOp {
            last_pass: allowance_mm.map(allowance),
            ..side_last_pass_op(p, Side::Left, CutDirection::Climb)
        };
        for (case, path) in [("U", u), ("notch", notch), ("20° V", vee_path(20.0))] {
            let mut p = last_pass_project_of(vec![path]);
            p.instances[0].transform = Transform::translate(20.0, 40.0);
            generate(&p, &with(&p, None));
            generate(&p, &with(&p, Some(0.2)));
            let err = generate_profile(&p, &with(&p, Some(ALLOWANCE_MM))).expect_err(case);
            let CamError::ShapeNarrowerThanAllowance {
                shape,
                diameter_mm,
                allowance_mm,
                ..
            } = &err
            else {
                panic!("{case}: expected ShapeNarrowerThanAllowance, got {err:?}");
            };
            assert_eq!(shape.profile, 0, "{case}");
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
            assert_approx_eq!(*allowance_mm, ALLOWANCE_MM, TOL);
        }
        let facing = vec![line_60(), open(&[(60.0, 6.85), (0.0, 6.85)])];
        let below_kept = vec![line_60(), open(&[(0.0, 10.3), (60.0, 10.3)])];
        for (case, lines) in [("facing", facing), ("below a kept strip", below_kept)] {
            let p = last_pass_project_of(lines);
            generate(&p, &with(&p, None));
            generate(&p, &with(&p, Some(0.2)));
            let err = generate_profile(&p, &with(&p, Some(ALLOWANCE_MM))).expect_err(case);
            let CamError::AllowanceCutsIntoKeptSide {
                shape,
                other,
                diameter_mm,
                allowance_mm,
                ..
            } = &err
            else {
                panic!("{case}: expected AllowanceCutsIntoKeptSide, got {err:?}");
            };
            assert_eq!((shape.profile, other.profile), (0, 1), "{case}");
            assert_approx_eq!(*diameter_mm, 6.35, TOL);
            assert_approx_eq!(*allowance_mm, ALLOWANCE_MM, TOL);
            let text = err.to_string();
            for needle in ["`o1`", "0.5 mm", "allowance", "6.35 mm"] {
                assert!(text.contains(needle), "{case}: {needle:?} in {text}");
            }
        }
        let corners = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)];
        let square: Vec<Polyline> = (0..4)
            .map(|k| open(&[corners[k], corners[(k + 1) % 4]]))
            .collect();
        let p = last_pass_project_of(square);
        let op = side_last_pass_op(&p, Side::Right, CutDirection::Climb);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(laps(&tp).len(), 4 * 3);
    }

    /// #311 (#324's rule on an open path): a sharp corner is cut. Inside
    /// a 60° V and an L (cut Left) and round them (cut Right), at 0.2 and
    /// 0.5 mm: each is generated and safe, the earlier passes keep
    /// `r + a` from the line and the last pass exactly `r`, and the
    /// material check ([`last_pass_removal`]) finds the last pass
    /// cutting the corner's wedge (`a / sin(θ/2)` inside, `a` round it)
    /// and nowhere more than the bound, at the ends included.
    #[test]
    fn last_pass_left_right_allows_sharp_corners() {
        let bound_mm = |a: f64| (6.35 / 3.0_f64).max(2f64.sqrt() * a + 0.05);
        for (case, path, side, corner_deg) in [
            ("60° V", vee_path(60.0), Side::Left, 60.0),
            ("60° V", vee_path(60.0), Side::Right, 180.0),
            ("L", ell_path(), Side::Left, 90.0),
            ("L", ell_path(), Side::Right, 180.0),
        ] {
            let line = at_20_20().apply_polyline(&path).unwrap();
            for a in [0.2, 0.5] {
                let case = format!("{case} {side:?} at {a} mm");
                let p = last_pass_project_of(vec![path.clone()]);
                let op = ProfileOp {
                    last_pass: Some(allowance(a)),
                    ..side_last_pass_op(&p, side, CutDirection::Climb)
                };
                let (tp, _) = generate(&p, &op);
                let split = last_pass_start(&tp, LAST_PASS_Z_MM[1], LAST_SAFE_Z_MM);
                let (near, far) = gap_range_mm(&line, tp.moves[0].to, &tp.moves[..split]).unwrap();
                assert!(
                    (near - (R_MM + a)).abs() <= 1e-3 && (far - (R_MM + a)).abs() <= 1e-3,
                    "{case}: an earlier pass from {near} to {far}"
                );
                let (near, far) =
                    gap_range_mm(&line, tp.moves[split].to, &tp.moves[split + 1..]).unwrap();
                assert!(
                    (near - R_MM).abs() <= 1e-3 && (far - R_MM).abs() <= 1e-3,
                    "{case}: the last pass from {near} to {far}"
                );
                let (widest_mm, removed_mm2) =
                    last_pass_removal(&tp, split, LAST_PASS_Z_MM[1] + 0.01);
                let wedge_mm = a / (corner_deg / 2.0_f64).to_radians().sin();
                assert!(removed_mm2 > 0.0, "{case}");
                assert!(
                    widest_mm >= wedge_mm - 0.15,
                    "{case}: the last pass cuts {widest_mm} mm, not the {wedge_mm} mm wedge"
                );
                assert!(
                    widest_mm <= bound_mm(a) + 0.1,
                    "{case}: the last pass cuts {widest_mm} mm thick, over {}",
                    bound_mm(a)
                );
            }
        }
    }
}
