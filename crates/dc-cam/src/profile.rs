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

#[cfg(test)]
mod defect_tests;
mod entries;
mod last_pass;
mod reached;
mod start;
#[cfg(test)]
mod start_tests;
mod tab_descent;
mod waste;

use crate::block::CutBlock;
use crate::context::{self, OpContext, check_diameter, check_safe_z, rates_of, tool_of};
use crate::entry::{
    self, CutSide, EntryKind, EntryTool, EntryWarnings, FallbackReason, MIN_BOUNDARY_SAFETY_MM,
};
use crate::error::{CamError, ShapeRef, at, at_mut};
use crate::lead::{self, DomainSpace, DropReason, LeadArc, LeadIn, LeadOut, LeadWarnings};
use crate::overlap::{CutPath, overlap_warnings, region_holds_path, within};
use crate::relief;
use crate::sequence::{SequenceInput, plan, sequence};
use crate::tabs::{
    LapRates, Least, PathEnds, PathTabs, Reduced, Request, TabCut, Tabs, check_size,
    coverage_warning, cut_range_warnings, lap_cuts, overlap_warning,
};
use crate::targets::target_profiles;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::warning::{OpenPathReason, TabOnCornerReason, Warning};
use dc_geometry::{
    Contour, EPS_MM, GeometryError, GeometryWarning, Point, Point3, Polyline, Region, SelfBand,
    Vertex, distance_away_from_mm, distance_mm, inset_is_empty, meeting_points, nearest_point,
    nest, offset, offset_open, point_segment_distance_mm, strip_ends,
};
use dc_project::{
    CutDirection, Entry, InstanceId, Lead, OperationId, ProfileOp, Project, Side, TabId, Tool,
};
use entries::{
    CutEntry, EntrySetup, PassEntry, PathDomain, PathEntry, Space, Targets, off_material,
    path_entries, plunged, tab_seams, wall_domains,
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
/// hole is offset the opposite way to its outer. Left and Right cut no
/// closed outline (#207), and their closed targets are set aside before
/// any is placed: [`CamError::Internal`] for one, which would otherwise
/// be cut on its line.
fn placement(side: Side, role: Role) -> Result<(Offset, Wall), CamError> {
    match (side, role) {
        (Side::Outside, Role::Outer) | (Side::Inside, Role::Hole) => {
            Ok((Offset::Grow, Wall::Outside))
        }
        (Side::Outside, Role::Hole) | (Side::Inside, Role::Outer) => {
            Ok((Offset::Shrink, Wall::Inside))
        }
        (Side::OnLine, Role::Outer) => Ok((Offset::None, Wall::Outside)),
        (Side::OnLine, Role::Hole) => Ok((Offset::None, Wall::Inside)),
        (Side::Left | Side::Right, _) => Err(CamError::internal(
            "a Left or Right Profile was asked to cut a closed outline",
        )),
    }
}

/// How far, mm, a Profile's last pass may step from the path its earlier
/// passes cut onto its own (#339): the most it may take off anywhere, a
/// third of the bit `diameter_mm` or a square corner's `√2 · a` plus
/// 0.05 mm, and 0.05 mm for the sampling that holds it there. The safety
/// pass's S9 holds a Left or Right last pass's step to the same bound.
#[must_use]
pub fn last_pass_step_reach_mm(diameter_mm: f64, stock_to_leave_mm: f64) -> f64 {
    last_pass::step_reach_mm(diameter_mm, stock_to_leave_mm)
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
/// [`Warning::TabOnHole`], unless it holds the slug: its `holds_slug`
/// says so (#302), or, unset as a format 14 file leaves it, it sits on
/// this Profile's contour round the hole, where only placing tabs asked
/// for on holes puts one (#300).
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
/// fits in the waste domain away from corners (the `lead` module): each
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
/// Pass Stock to leave") every pass but the last cuts each closed shape's
/// stock path, its tool-centre path offset `stock_to_leave_mm` farther
/// into the waste, and the last pass cuts the shape's own path at full
/// depth, after the usual passes to `depth − thickness_mm` when a
/// thickness is set, the other way round when `reverse`. The tabs are laid
/// on the shape's path and carried onto the stock path widened by the
/// stock to leave; entries, leads and waste domains are planned per path; every
/// relief level is cut from the last pass. A drawing the stock passes
/// cannot follow is refused ([`CamError::BandNarrowerThanStockToLeave`],
/// [`CamError::ShapeNarrowerThanStockToLeave`]), and a last pass On line, or
/// out of range, is [`CamError::LastPassNeedsSide`] or
/// [`CamError::InvalidLastPass`]. A Left or Right Profile (#311) takes a
/// last pass too: each open path's stock path is its offset by
/// `r + a` to the same side, ending square on the same end normals, so
/// no pass cuts past an end and the last pass takes the stock to leave off up
/// to each; the twin runs the way its path does, ramps along itself, and
/// carries the tabs as a closed shape's twin does. `reverse` runs the
/// last pass from the path's other end, where the earlier passes end.
/// The last pass never goes straight down through the stock to leave (#339,
/// `last_pass_block`): it descends as one more earlier pass would, on
/// the stock path where the pass before ended, by the entry (a ramp
/// along that path), and steps across the stock to leave onto its own path. The
/// stock paths must follow the line (`last_pass::open_twins`,
/// [`CamError::ShapeNarrowerThanStockToLeave`]) and keep `r + a` from every
/// other line and kept strip ([`CamError::StockToLeaveCutsIntoKeptSide`]).
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
/// (#206, the `overlap` module). Fails without a toolpath when a feed, plunge rate, or
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
    generate_profile_below(project, op, 0.0)
}

/// [`generate_profile`] with its passes starting `start_depth_mm` below
/// the stock top (#443, a male Inlay's cut-out, whose floor the carve has
/// already cleared): the first pass is a step-down below that plane, and
/// no pass cuts above it. Everything else, safe Z, the rapids' clearance
/// above the stock top, the tab heights and the through-cut limit, keeps
/// to the stock as it is. `start_depth_mm` is 0 for every Profile, and
/// below `op.depth_mm`.
pub(crate) fn generate_profile_below(
    project: &Project,
    op: &ProfileOp,
    start_depth_mm: f64,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    profile_below(project, op, start_depth_mm).map_err(|e| e.for_op(&op.id))
}

/// [`generate_profile_below`], before a defect it finds
/// ([`CamError::Internal`]) names `op`.
fn profile_below(
    project: &Project,
    op: &ProfileOp,
    start_depth_mm: f64,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = tool_of(project, &op.id, &op.tool)?;
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
    check_diameter(tool)?;
    check_safe_z(&op.id, op.safe_z_mm)?;
    entry::check(&op.id, &op.entry)?;
    if !op.side.has_waste_area() && matches!(op.entry, Entry::Helix { .. }) {
        return Err(CamError::HelixNeedsWasteSide { op: op.id.clone() });
    }
    let step_down_mm = context::step_down_mm(&op.id, op.step_down_mm, tool)?;
    let thickness_mm = context::thickness_mm(project)?;
    let depth_mm = context::depth_mm(&op.id, op.depth_mm)?;
    // #284: a last pass, with its own depth when it sets a thickness.
    let last_pass = op.last_pass.as_ref();
    if let Some(last_pass) = last_pass {
        last_pass::check(op, last_pass, tool.diameter_mm, depth_mm, step_down_mm)?;
    }
    // #443: from a plane below the stock top, the passes step down from
    // it as from a thinner stock's top.
    let pass_z = last_pass::pass_z(
        op,
        thickness_mm - start_depth_mm,
        depth_mm - start_depth_mm,
        step_down_mm,
        last_pass,
    )?;
    let last_index = pass_z.len().saturating_sub(1);
    // The passes before the last run on the stock paths; with no pass
    // before it there are none.
    let stock_to_leave_mm = last_pass
        .filter(|_| pass_z.len() > 1)
        .map(|l| l.stock_to_leave_mm);
    let reverse_last = last_pass.is_some_and(|l| l.reverse);
    let op_ctx = OpContext::new(project, &op.id, tool, rates, thickness_mm, op.safe_z_mm);
    let safe_z_mm = op_ctx.safe_z_mm;
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
        stock_to_leave_mm,
        &mut warnings,
    )?;
    // #285: an explicit start re-seams each closed path, and its stock to leave
    // twin, nearest the start point, before the tabs are laid on it.
    let (placed, fixed) = start::seat(project, op, placed)?;
    let walls: Vec<(Wall, Option<usize>)> = placed.iter().map(|c| (c.wall, c.region)).collect();
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
    // A hole tab that holds its slug does because the user asked:
    // `tabs place` warned when it placed it (#300, #302).
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
    // #284: the part's `n` paths, then each one's stock to leave twin, which
    // the passes before the last cut; `twin_of` finds it. A twin's tabs
    // are its part path's, carried onto it and widened by the stock to leave;
    // a reversed last pass runs its path the other way from the same
    // start, its tabs where they were.
    let n = paths.len();
    let mut walls = walls;
    let mut layouts = layouts;
    // #362: a lap rises over every tab its bit reaches, not only its own
    // outline's. Each part path, and further down each one's stock to leave
    // twin, rises over the tabs the other part paths' last passes leave,
    // standing from their floor (`reached`).
    let floor_z_mm = *pass_z
        .last()
        .ok_or_else(|| CamError::internal("a Profile has no pass, so no floor"))?;
    let sources: Vec<(Polyline, Option<PathTabs>)> = paths
        .iter()
        .zip(&layouts)
        .map(|((path, _), layout)| (path.clone(), layout.as_ref().map(|(l, _)| l.clone())))
        .collect();
    let over_reached = |i: usize, path: &Polyline, layout: &PathTabs| {
        let others = sources
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .filter_map(|(_, (p, l))| l.as_ref().map(|l| (p, l)));
        reached::over_reached_tabs(path, layout, others, radius_mm, floor_z_mm)
    };
    for (i, ((path, _), layout)) in paths.iter().zip(layouts.iter_mut()).enumerate() {
        if let Some((layout, _)) = layout
            && let Some(raised) = over_reached(i, path, layout)
        {
            *layout = raised;
        }
    }
    let mut stocks_to_leave: Vec<f64> = vec![0.0; n];
    let mut reversed: Vec<bool> = vec![false; n];
    let mut twin_of: Vec<Option<usize>> = vec![None; n];
    let mut fixed = fixed;
    if let Some(stock_to_leave_mm) = stock_to_leave_mm {
        // The tabs each last pass leaves stand from its floor.
        for (i, twin) in twins.into_iter().enumerate() {
            // A path with no stock path is cut as it is on every pass.
            let Some(twin) = twin else {
                continue;
            };
            let (path, shape) = at(&paths, i, "a stock path's part path is missing")?;
            let wall = *at(&walls, i, "a stock path's part path has no wall")?;
            let layout = at(
                &layouts,
                i,
                "a stock path's part path has no tab layout slot",
            )?;
            let carried = match layout {
                Some((l, overlapped)) => {
                    let own = last_pass::twin_tabs(path, l, &twin, radius_mm, floor_z_mm);
                    // No tab of another path in reach: its own, as carried.
                    let raised = over_reached(i, &twin, &own).unwrap_or(own);
                    Some((raised, *overlapped))
                }
                None => None,
            };
            let shape = *shape;
            *at_mut(
                &mut twin_of,
                i,
                "a part path has no slot for its stock path",
            )? = Some(paths.len());
            paths.push((twin, shape));
            walls.push(wall);
            layouts.push(carried);
            stocks_to_leave.push(stock_to_leave_mm);
            reversed.push(false);
            let own = *at(
                &fixed,
                i,
                "a part path does not say whether its start is set",
            )?;
            fixed.push(own);
        }
    }
    if reverse_last {
        // The part paths come first, `n` of them, one layout and flag each.
        let part_paths = paths
            .iter_mut()
            .zip(layouts.iter_mut())
            .zip(reversed.iter_mut())
            .take(n);
        for (((path, _), layout), flag) in part_paths {
            // #311: an open path's last pass runs from its other end,
            // where the earlier passes end, its tabs measured from there.
            let back = if path.is_closed() {
                last_pass::reversed_from_start(path)
            } else {
                path.reversed()
            };
            if let Some((l, _)) = layout {
                *l = if path.is_closed() {
                    last_pass::carry_reversed(path, l, &back)?
                } else {
                    l.reversed_open()
                };
            }
            *path = back;
            *flag = true;
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
        stocks_to_leave: &stocks_to_leave,
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
        wall_domains(&setup)?
    } else {
        Vec::new()
    };
    let below_tabs = |z_mm: f64| tabs.as_ref().is_some_and(|t| z_mm < t.top_z_mm() - EPS_MM);
    // #339: each stock to leave twin's part path, whose last pass enters from
    // the twin.
    let mut part_of: Vec<Option<usize>> = vec![None; paths.len()];
    for (i, twin) in twin_of.iter().enumerate() {
        if let Some(t) = *twin {
            *at_mut(
                &mut part_of,
                t,
                "a stock path has no slot for its part path",
            )? = Some(i);
        }
    }
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
                part_of: &part_of,
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
    let mut seams = tab_seams(&setup, &domains, &kept)?;
    // #339: a closed part path whose seam is free starts within a step
    // of where its twin starts, where the last pass enters from the
    // twin's kerf: as it is when it already does, else at its point
    // nearest there.
    for (i, twin) in twin_of.iter().enumerate() {
        // A path with no stock path keeps its seam.
        let Some(t) = *twin else {
            continue;
        };
        let (path, _) = at(&paths, i, "a part path with a stock path is missing")?;
        if *at(
            &kept,
            i,
            "a part path does not say whether its seam is kept",
        )? || !path.is_closed()
        {
            continue;
        }
        let (twin, _) = at(&paths, t, "a part path's stock path is missing")?;
        let twin_seam_mm = *at(&seams, t, "a stock path has no seam slot")?;
        let own_seam_mm = *at(&seams, i, "a part path has no seam slot")?;
        // Only a path with a stock path is here, so the Profile leaves stock.
        let stock_to_leave_mm = stock_to_leave_mm.ok_or_else(|| {
            CamError::internal("a path has a stock path, but the Profile leaves no stock")
        })?;
        // `None` for a path with no vertex or no length, which has no
        // start to step from.
        let twin_start = twin_seam_mm.map_or_else(
            || twin.vertices().first().map(|v| v.point()),
            |s| lead::reseam(twin, s).vertices().first().map(|v| v.point()),
        );
        let own_start = own_seam_mm.map_or_else(
            || path.vertices().first().map(|v| v.point()),
            |s| path.point_at(s).map(|(p, _)| p),
        );
        let reach_mm = last_pass::step_reach_mm(tool.diameter_mm, stock_to_leave_mm);
        if let (Some(at), Some(own)) = (twin_start, own_start)
            && own.distance_to(at) <= reach_mm
        {
            continue;
        }
        let seam_mm = twin_start
            .and_then(|at| nearest_point(path, at))
            .map(|near| near.along_mm)
            .filter(|&d| d > EPS_MM && d < path.length_mm() - EPS_MM);
        *at_mut(&mut seams, i, "a part path has no seam slot")? = seam_mm;
    }
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
    // Where each path's entries end: its start, or its lead's staging
    // point on the passes the lead serves.
    let targets: Vec<Targets> = paths
        .iter()
        .zip(&leads)
        .map(|((path, _), led)| {
            let start = path_start(path)?;
            let staging = led.as_ref().map(|l| l.lead_in.staging());
            Ok(Targets {
                plain: staging.unwrap_or(start),
                tabbed: led
                    .as_ref()
                    .filter(|l| l.below_tabs)
                    .map_or(start, |l| l.lead_in.staging()),
            })
        })
        .collect::<Result<_, CamError>>()?;
    let entries = path_entries(&setup, &domains, &targets)?;
    let mut entry_warnings = EntryWarnings::default();
    let mut blocks = Vec::with_capacity(n * pass_z.len());
    // The part's path each block cuts, for naming contours in cut order.
    let mut block_paths = Vec::with_capacity(blocks.capacity());
    // #335: each shape's tightest inside arc held at the floor feed.
    let mut tight_arcs: Vec<(ShapeRef, f64)> = Vec::new();
    // #350: each shape's tightest bend of straight cuts.
    let mut bends: Vec<(ShapeRef, last_pass::FlattenedBend)> = Vec::new();
    let ctx = PassCtx {
        op,
        tabs: tabs.as_ref(),
        pass_z: &pass_z,
        step_down_mm,
        safe_z_mm,
        start_z_mm,
        top_z_mm: thickness_mm,
        rates,
        tolerance_mm: project.flatten_tolerance_mm,
        regions: &regions,
    };
    // Path `k` with everything a pass cuts it by. Each list has one entry
    // per path, but `domains`, which is empty when nothing asks for a
    // waste domain (a plunge with no lead; On line, Left and Right).
    let path_at = |k: usize| -> Result<PassPath<'_>, CamError> {
        let (path, shape) = at(&paths, k, "a pass has no path to cut")?;
        Ok(PassPath {
            path,
            shape,
            layout: at(&layouts, k, "a path a pass cuts has no tab layout slot")?,
            lead: at(&leads, k, "a path a pass cuts has no lead slot")?.as_ref(),
            entry: at(&entries, k, "a path a pass cuts has no entry slot")?.as_ref(),
            domain: domains.get(k),
            tool: entries::tool_for(&setup, k)?,
        })
    };
    for (i, twin) in twin_of.iter().copied().enumerate() {
        // The entry the previous pass cut, which this pass's follows.
        let mut previous: Option<CutEntry> = None;
        let shape = at(&paths, i, "a part path is missing")?.1;
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            let block = match twin {
                // #339: the last pass enters from its twin's kerf.
                Some(t) if pass == last_index => {
                    let entered = last_pass_block(
                        &ctx,
                        &LastPassPaths {
                            own: path_at(i)?,
                            twin: path_at(t)?,
                            reverse: reverse_last,
                            domain: domains.get(i),
                            stock_to_leave_mm: stock_to_leave_mm.ok_or_else(|| {
                                CamError::internal(
                                    "a path has a stock path, but the Profile leaves no stock",
                                )
                            })?,
                        },
                        pass,
                        z_mm,
                        &mut previous,
                        &mut entry_warnings,
                    )?;
                    if entered.lead_dropped {
                        lead_warnings.record_dropped(shape, DropReason::NoRoom);
                    }
                    if let Some(rho) = entered.tight_mm {
                        match tight_arcs.iter_mut().find(|(s, _)| s == shape) {
                            Some((_, tightest)) => *tightest = tightest.min(rho),
                            None => tight_arcs.push((shape.clone(), rho)),
                        }
                    }
                    if let Some(bend) = entered.bend {
                        match bends.iter_mut().find(|(s, _)| s == shape) {
                            Some((_, tightest)) if tightest.edge_ratio < bend.edge_ratio => {
                                *tightest = bend;
                            }
                            Some(_) => {}
                            None => bends.push((shape.clone(), bend)),
                        }
                    }
                    entered.block
                }
                // #284: every pass but the last cuts the stock to leave twin.
                _ => {
                    let p = path_at(twin.unwrap_or(i))?;
                    pass_block(&ctx, &p, pass, z_mm, &mut previous, &mut entry_warnings)?
                        .map(|(block, _)| block)
                }
            };
            // A path with no vertex has no block.
            if let Some(block) = block {
                blocks.push(block);
                block_paths.push(i);
            }
        }
    }
    warnings.extend(entry_warnings.into_warnings(&op.id));
    if let Some(stock_to_leave_mm) = stock_to_leave_mm {
        let floor_fraction = stock_to_leave_mm / (stock_to_leave_mm + radius_mm);
        warnings.extend(tight_arcs.into_iter().map(|(shape, arc_radius_mm)| {
            Warning::LastPassEdgeFaster {
                op: op.id.clone(),
                shape,
                radius_mm: arc_radius_mm,
                stock_to_leave_mm,
                feed_mm_min: rates.feed_mm_min * floor_fraction,
                edge_ratio: floor_fraction * (arc_radius_mm + radius_mm) / arc_radius_mm,
            }
        }));
    }
    warnings.extend(
        bends
            .into_iter()
            .map(|(shape, bend)| Warning::LastPassFlattenedCurve {
                op: op.id.clone(),
                shape,
                radius_mm: bend.radius_mm,
                x_mm: bend.at.x,
                y_mm: bend.at.y,
                edge_ratio: bend.edge_ratio,
            }),
    );
    if let Some(lead) = lead {
        warnings.extend(lead_warnings.into_warnings(&op.id, lead));
    }
    let containment = containment_pairs(&regions, &shapes)?;
    let mut input = SequenceInput {
        order: op.machining_order,
        containment: &containment,
        ..SequenceInput::new(&op_ctx, blocks)
    };

    // Warnings name contours by their position in the cut order, which
    // the plan sets.
    let cut_order = plan(&input)?;
    let mut path_order: Vec<usize> = Vec::with_capacity(n);
    for index in cut_order {
        let path_index = *at(
            &block_paths,
            index,
            "the cut order names a block of no path",
        )?;
        if !path_order.contains(&path_index) {
            path_order.push(path_index);
        }
    }
    // A path with no vertex has no block, so the plan never reached it:
    // it keeps its place.
    for index in 0..n {
        if !path_order.contains(&index) {
            path_order.push(index);
        }
    }
    // D6: paths in cut order, so the warnings follow it; each part path's
    // stock to leave twin, cut before it, with it (#284).
    let mut cut_paths: Vec<CutPath<'_>> = Vec::with_capacity(paths.len());
    for (cut_index, &path_index) in path_order.iter().enumerate() {
        let (path, shape) = at(&paths, path_index, "the cut order names a missing path")?;
        cut_paths.push((path, Some(*shape)));
        if let Some(t) = *at(&twin_of, path_index, "a part path has no stock path slot")? {
            let (twin, shape) = at(&paths, t, "a part path's stock path is missing")?;
            cut_paths.push((twin, Some(*shape)));
        }
        // A path with no tabs has neither warning.
        let layout = at(&layouts, path_index, "a path has no tab layout slot")?;
        let Some((_, overlapped)) = layout else {
            continue;
        };
        let length_mm = path.length_mm();
        if *overlapped {
            warnings.push(overlap_warning(&op.id, cut_index, Some(shape), length_mm));
        }
        let reduction = at(&reductions, path_index, "a path has no tab reduction slot")?;
        if let Some((reduced, widths_mm)) = reduction {
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
            stock_to_leave_mm.unwrap_or(0.0),
            project.flatten_tolerance_mm,
        ));
    }

    // Corner relief among each shape's own passes, never after the pass
    // that frees it (`crate::relief`, #189); a freeing pass's notches are
    // cut from inside its lap (#193). With a last pass stock to leave only the
    // last pass runs on the part's paths, so it opens every notch's column
    // and cuts every level (#284).
    // The part paths, the first `n`, that cut a region's wall: relief
    // looks each region's paths up by its index, and an open path bounds
    // no region.
    let mut relief_paths = Vec::with_capacity(n);
    let mut relief_walls = Vec::with_capacity(n);
    for ((path, shape), (wall, region)) in paths.iter().zip(&walls).take(n) {
        if let Some(region) = region {
            relief_paths.push((path.clone(), *shape));
            relief_walls.push((*wall, *region));
        }
    }
    let geometry = relief::ProfileGeometry {
        regions: &regions,
        shapes: &shapes,
        paths: &relief_paths,
        walls: &relief_walls,
        only_pass: stock_to_leave_mm.map(|_| last_index),
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
    op_ctx.check_on_stock(&toolpath, radius_mm)?;
    // #310 review: a one-pass cut has no pass to leave the stock to leave.
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
        // `nest` numbers the contours as it was given them, one a shape.
        let (dropped, _) = at(&shapes, index, "a duplicate contour is of no shape")?;
        let (kept, _) = at(&shapes, duplicate_of, "a duplicated contour is of no shape")?;
        warnings.push(Warning::DuplicateShapeDropped {
            shape: dropped.clone(),
            duplicate_of: kept.clone(),
        });
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
    /// The index of its region; `None` for an open path, which bounds
    /// none.
    region: Option<usize>,
    /// The shape is a hole.
    hole: bool,
    /// #284: the stock path the passes before the last cut instead,
    /// `stock_to_leave_mm` farther from the shape, running the same way.
    twin: Option<Polyline>,
}

/// Every tool-centre path of `op` cut with `tool`, in drawing order:
/// each closed shape offset per `op.side` and oriented per
/// [`traversal_is_ccw`], then each open path as drawn (reversed under
/// climb, #242). Shapes the tool does not fit warn
/// [`Warning::ToolTooLarge`]; bands narrower than the tool fail. With a
/// last pass `stock_to_leave_mm` (#284) each closed shape's path has its
/// stock to leave twin, or the shape is refused ([`last_pass::twins`]); so has
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
    stock_to_leave_mm: Option<f64>,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<CentrePath<'s>>, CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let mut placed: Vec<CentrePath<'s>> = Vec::new();
    for (region_index, region) in regions.iter().enumerate() {
        let outer = (region.outer(), region_outer(region, shapes)?, Role::Outer);
        let ordered = hole_shapes(region, shapes)?
            .into_iter()
            .map(|(h, shape)| (h, shape, Role::Hole))
            .chain(std::iter::once(outer));
        for (contour, shape, role) in ordered {
            let (how, wall) = placement(op.side, role)?;
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
            // #284: each part path's stock to leave twin.
            let twins: Vec<Option<Contour>> = match stock_to_leave_mm {
                Some(stock_to_leave_mm) => {
                    let outline = last_pass::Outline {
                        region,
                        contour,
                        shape,
                        how,
                        centres: &centres,
                    };
                    let twins = last_pass::twins(
                        op,
                        tool,
                        &outline,
                        shapes,
                        stock_to_leave_mm,
                        tolerance_mm,
                    )?;
                    for twin in &twins {
                        last_pass::check_neighbours(
                            op,
                            tool,
                            regions,
                            shapes,
                            (twin.polyline(), shape, contour),
                            region_index,
                            stock_to_leave_mm,
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
                region: Some(region_index),
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
        if let Some(left_mm) = side_mm {
            check_open_ends(op, tool, (shape, path), &centres, left_mm, None)?;
        }
        open_centres.push((shape, centres));
        open_lines.push(path);
    }
    // #311: each Left or Right piece's stock to leave twin, the path offset by
    // `r + a` to the same side, ending on the same end normals; then the
    // twins held to the kept-side rule with the radius grown by `a`.
    let mut open_twins: Vec<Vec<Option<Polyline>>> = open_centres
        .iter()
        .map(|(_, centres)| centres.iter().map(|_| None).collect())
        .collect();
    if let Some(left_mm) = side_mm {
        check_kept_sides(op, tool, open_paths, &open_centres, left_mm, 0.0)?;
        if let Some(stock_to_leave_mm) = stock_to_leave_mm {
            let mut twins: Vec<(&ShapeRef, Vec<Polyline>)> = Vec::with_capacity(open_centres.len());
            for ((shape, centres), path) in open_centres.iter().zip(&open_lines) {
                let found = last_pass::open_twins(
                    op,
                    tool,
                    (shape, path),
                    centres,
                    left_mm,
                    stock_to_leave_mm,
                )?;
                twins.push((shape, found));
            }
            check_kept_sides(op, tool, open_paths, &twins, left_mm, stock_to_leave_mm)?;
            open_twins = twins
                .into_iter()
                .map(|(_, found)| found.into_iter().map(Some).collect())
                .collect();
        }
    }
    // #175: an open path is cut On line as drawn, from the end
    // `open_runs_reversed` picks (#242 On line, #287 Left and Right); a
    // Left or Right offset of one (#207) from the end beside that vertex,
    // and its stock to leave twin (#311) the same way. It bounds no region, so
    // no containment and no waste domain; its wall is never asked.
    let reversed_open = open_runs_reversed(op.side, op.direction);
    let as_cut = |path: Polyline| if reversed_open { path.reversed() } else { path };
    for ((shape, centres), twins) in open_centres.into_iter().zip(open_twins) {
        for (path, twin) in centres.into_iter().zip(twins) {
            placed.push(CentrePath {
                path: as_cut(path),
                shape,
                wall: Wall::Outside,
                region: None,
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

/// What [`plan_tabs`] reports for a path with no entry in `holes`.
const NO_HOLE_FLAG: &str = "a Profile path does not say whether it is cut round a hole";

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
    /// whether the tab holds the slug, so does not warn: its
    /// `holds_slug` (#302), or, unset, whether it sits on that path, where
    /// placing a request or `--count` that asked for tabs on holes puts it
    /// (#300).
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
/// width or height is not a positive number, and with
/// [`CamError::Internal`] when `holes` is not one per path.
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
    // The request's tabs are fitted at its own height; no request, none.
    let request = request.map(|r| (r, Tabs::new(r.top_z_mm(), diameter_mm, tolerance_mm)));
    let mut requested = Vec::with_capacity(paths.len());
    let mut reductions = Vec::with_capacity(paths.len());
    let mut on_corners = Vec::with_capacity(paths.len());
    for (j, (path, _)) in paths.iter().enumerate() {
        let hole = *at(holes, j, NO_HOLE_FLAG)?;
        // No request, or one that leaves holes alone: no tab asked here.
        let Some((request, fitter)) = request.as_ref().filter(|(r, _)| !hole || r.on_holes())
        else {
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
        let fit = request.fit(fitter, path, ends_of(path), least);
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
        // The path has a point nearest the tab, so one nearest its foot.
        let reach_mm = nearest_point(path, foot)
            .ok_or_else(|| {
                CamError::internal("a path nearest a tab has no point nearest the tab's foot")
            })?
            .gap_mm;
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
        at_mut(&mut placed, j, "a path a tab is cut on has no slot for it")?.push(TabCut {
            centre_mm,
            width_mm: tab.width_mm,
            top_z_mm: tab.height_mm,
            shape: tab.shape,
        });
        crossed.push(tab.id.clone());
        if *at(holes, j, NO_HOLE_FLAG)? {
            // #302: the tab says whether it holds the slug. A format 14
            // tab does not; only `place` with `on_holes` put one on this
            // path round a hole, so one still on it was asked for (#300).
            let holds_slug = tab.holds_slug.unwrap_or(near.gap_mm <= EPS_MM);
            on_holes.push((tab.id.clone(), shape.clone(), holds_slug));
        }
    }

    let tops_z_mm: Vec<f64> = request
        .as_ref()
        .map(|(r, _)| r.top_z_mm())
        .into_iter()
        .chain(placed.iter().flatten().map(|c| c.top_z_mm))
        .collect();
    let tabs = (request.is_some() || !crossed.is_empty()).then(|| {
        let top_z_mm = tops_z_mm.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Tabs::new(top_z_mm, diameter_mm, tolerance_mm)
    });
    let mut layouts = Vec::with_capacity(paths.len());
    // One of each list per path, built above.
    let per_path = paths
        .iter()
        .zip(&placed)
        .zip(&requested)
        .zip(reductions.iter_mut());
    for ((((path, _), own), asked), slot) in per_path {
        let Some(tabs) = &tabs else {
            layouts.push(None);
            continue;
        };
        let mut cuts = asked.clone();
        cuts.extend_from_slice(own);
        let length_mm = path.length_mm();
        let (layout, overlapped) = tabs.layout(&cuts, length_mm, ends_of(path));
        // Placed tabs are cut as placed: a layout that covers more than
        // half the path warns, with nothing cut down.
        if slot.is_none() && !own.is_empty() && layout.covered_mm() > length_mm / 2.0 + EPS_MM {
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
    op_tabs_unnamed(project, op).map_err(|e| e.for_op(&op.id))
}

/// [`op_tabs`], before a defect it finds ([`CamError::Internal`]) names
/// `op`.
fn op_tabs_unnamed(project: &Project, op: &ProfileOp) -> Result<OpTabs, CamError> {
    let tool = tool_of(project, &op.id, &op.tool)?;
    if op.side != Side::OnLine && !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    check_diameter(tool)?;
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
    let (centres, _) = start::seat(project, op, centres)?;
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
/// With a last pass `stock_to_leave_mm` (#284, more than 0), `centres` are the
/// stock paths, `r + a` from `contour`, and Inside each must keep
/// `r + a` from every other line, so it cuts neither that line's skin nor
/// the part: the band must be `D + 2a` wide, or
/// [`CamError::BandNarrowerThanStockToLeave`].
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the band; a struct would only rename them"
)]
pub(super) fn check_band(
    op: &OperationId,
    side: Side,
    tool: &Tool,
    stock_to_leave_mm: f64,
    region: &Region,
    (contour, shape): (&Contour, &ShapeRef),
    centres: &[Contour],
    shapes: &[(ShapeRef, Contour)],
    tolerance_mm: f64,
) -> Result<(), CamError> {
    let reach_mm = match side {
        Side::Inside => tool.diameter_mm / 2.0 + stock_to_leave_mm,
        Side::OnLine => tool.diameter_mm,
        // Left and Right cut no closed outline (#207).
        Side::Outside | Side::Left | Side::Right => return Ok(()),
    };
    let others = region_shapes(region, shapes)?;
    for (line, b) in others.into_iter().filter(|(l, _)| *l != contour) {
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
        return Err(if stock_to_leave_mm > 0.0 {
            CamError::BandNarrowerThanStockToLeave {
                op: op.clone(),
                shape: shape.clone(),
                other: Box::new(other.clone()),
                band_mm,
                diameter_mm: tool.diameter_mm,
                stock_to_leave_mm,
                needed_mm: tool.diameter_mm + 2.0 * stock_to_leave_mm,
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

/// How far a piece's end may lie from the square end point of an open
/// path's offset and still be it, mm: well above the error of
/// cavalier_contours' trim, well below any length a cut beside a segment
/// is worth making.
const SQUARE_END_MM: f64 = 1e-4;

/// #342: every piece of an open path's Left or Right offset `pieces`, by
/// `left_mm` (positive to the left, the radius, or the radius plus
/// `stock_to_leave_mm` for the passes before the last), must end square on the
/// normals through `path`'s end vertices (A8, "Ends"). The offset of the
/// segment that ends there runs out to the square end point, `|left_mm|`
/// along the normal from the vertex, and ends there; where that segment
/// is too short to have one, on the inside of the turn before it, the
/// offset of an earlier segment runs on until the circle of radius
/// `|left_mm|` round the vertex stops it, which is past the normal: its
/// cutter would take the strip the path keeps there, or the part past the
/// end. So a piece that runs to the vertex's circle (each piece runs as
/// `path` does: its last vertex for the path's last, its first for its
/// first) when none ends square there is refused:
/// [`CamError::EndSegmentHasNoRoom`] at that vertex. Clipped back to the
/// normal, where the normal crosses it at all, the piece would stop more
/// than the radius from the vertex and nothing would be cut beside the
/// end segment: the bit cannot follow that end. Where the end segment has
/// room the offset ends square already, and the strip's ends
/// ([`check_kept_sides`]) hold any other piece out of the strip.
fn check_open_ends(
    op: &ProfileOp,
    tool: &Tool,
    (shape, path): (&ShapeRef, &Polyline),
    pieces: &[Polyline],
    left_mm: f64,
    stock_to_leave_mm: Option<f64>,
) -> Result<(), CamError> {
    let length_mm = path.length_mm();
    let vertices = path.vertices();
    let firsts: Vec<Point> = pieces
        .iter()
        .filter_map(|p| p.vertices().first().map(|v| v.point()))
        .collect();
    let lasts: Vec<Point> = pieces
        .iter()
        .filter_map(|p| p.vertices().last().map(|v| v.point()))
        .collect();
    let ends = [
        (0, 0.0, firsts),
        (vertices.len().saturating_sub(1), length_mm, lasts),
    ];
    for (vertex, at_mm, piece_ends) in ends {
        let Some((end, tangent)) = path.point_at(at_mm) else {
            continue;
        };
        let square = end + tangent.perp() * left_mm;
        let (mut square_end, mut wrapped) = (false, false);
        for p in piece_ends {
            if p.distance_to(square) <= SQUARE_END_MM {
                square_end = true;
            } else if p.distance_to(end) <= left_mm.abs() + SQUARE_END_MM {
                wrapped = true;
            }
        }
        if wrapped && !square_end {
            let segment = if vertex == 0 {
                path.segments().find(|s| s.length_mm() > EPS_MM)
            } else {
                path.segments().filter(|s| s.length_mm() > EPS_MM).last()
            };
            return Err(CamError::EndSegmentHasNoRoom {
                op: op.id.clone(),
                shape: Box::new(shape.clone()),
                vertex,
                length_mm: segment.map_or(0.0, |s| s.length_mm()),
                diameter_mm: tool.diameter_mm,
                stock_to_leave_mm,
            });
        }
    }
    Ok(())
}

/// #207: the band rule for a Left or Right Profile. The side its cutter
/// runs on is waste and the other side is the part, so the bit must not
/// cross another line of the operation, nor reach the material a line
/// keeps beside it. Each path of `centres` (the open paths `open` offset
/// by `left_mm`, as cut) must keep the tool radius (less [`EPS_MM`]) from
/// every other line of `open`, and from every line's kept edge: the line
/// offset by the radius to its other side, so the strip one radius wide
/// beside it is never cut. The strip is closed at each end by the normal
/// through the end vertex, a radius long and cut short at the first line
/// it meets ([`strip_ends`], #342), so a cut that reaches past an end
/// into the strip is refused too, as S9 refuses it. A path is held to its own kept edge too, which
/// catches a spiral or hairpin whose cut reaches what the line keeps
/// further along; it keeps its own line by construction
/// ([`offset_open`]). Lines that meet are not set aside, unlike On line's
/// #210: a square end that runs onto the next line of a part drawn as
/// separate lines cuts into it. [`CamError::CutsIntoKeptSide`], the
/// shapes in drawing order.
///
/// With a last pass `stock_to_leave_mm` (#311, more than 0), `centres` are the
/// stock paths, `r + a` from their lines, and each must keep `r + a`
/// from every other line and every kept edge, so it takes neither another
/// line's skin, which that line's last pass is to cut, nor any of the
/// strip it keeps: [`CamError::StockToLeaveCutsIntoKeptSide`]. The kept
/// edges stay one radius off their lines; S9 holds every pass to the
/// radius from them.
fn check_kept_sides(
    op: &ProfileOp,
    tool: &Tool,
    open: &[(ShapeRef, Polyline)],
    centres: &[(&ShapeRef, Vec<Polyline>)],
    left_mm: f64,
    stock_to_leave_mm: f64,
) -> Result<(), CamError> {
    let reach_mm = left_mm.abs() + stock_to_leave_mm;
    let lines: Vec<&Polyline> = open.iter().map(|(_, line)| line).collect();
    let kept: Vec<(&ShapeRef, &Polyline, Vec<Polyline>)> = open
        .iter()
        .map(|(shape, line)| {
            let mut edges = offset_open(line, -left_mm);
            edges.extend(strip_ends(line, -left_mm, &lines));
            (shape, line, edges)
        })
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
            return Err(if stock_to_leave_mm > 0.0 {
                CamError::StockToLeaveCutsIntoKeptSide {
                    op: op.id.clone(),
                    shape: first.clone(),
                    other: Box::new(second.clone()),
                    diameter_mm: tool.diameter_mm,
                    stock_to_leave_mm,
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
    // The region that holds each open path, with its index; `None` for a
    // path no region holds, which D6 looks at.
    let held: Vec<Option<(usize, &Region)>> = open
        .iter()
        .map(|(_, path)| {
            regions
                .iter()
                .enumerate()
                .find(|(_, r)| region_holds_path(r, path))
        })
        .collect();
    let drawn = |s: &ShapeRef| (op.targets.iter().position(|t| *t == s.instance), s.profile);
    for (index, ((shape, path), holder)) in open.iter().zip(&held).enumerate() {
        let Some((region_index, region)) = *holder else {
            continue;
        };
        let lines = region_shapes(region, shapes)?
            .into_iter()
            .map(|(c, shape)| (shape, c.polyline()));
        let later = open
            .iter()
            .zip(&held)
            .skip(index + 1)
            .filter(|(_, h)| h.is_some_and(|(i, _)| i == region_index))
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
) -> Result<CutBlock, CamError> {
    let descent = if (lap.start_z_mm - z_mm).abs() > EPS_MM {
        vec![Move::linear(
            Point3::new(start.x, start.y, lap.start_z_mm),
            rates.plunge_mm_min,
        )]
    } else {
        Vec::new()
    };
    // The lap starts where the descent ends: on a tab, the tab's height.
    let entry_z_mm = descent.last().map_or(z_mm, |m| m.to.z);
    let exit = lap.moves.last().map_or(start, |m| m.to.xy());
    // #337: a lap that starts on a tab starts at the tab's height. The
    // pass before left the tab there, and a lap that also started on it
    // ended there, so the descent never goes below that height at the
    // start: it would plunge into the tab. The lap comes down off the tab
    // at the tab's end, as every lap does.
    let descent_from_z_mm =
        pass_above_z_mm(pass_z, pass)?.map_or(safe_z_mm, |above| above.max(lap.start_z_mm));
    Ok(CutBlock {
        shape: shape.clone(),
        pass,
        z_mm,
        descent_from: start,
        descent_from_z_mm,
        descent,
        entry: start,
        entry_z_mm,
        cuts: lap.moves,
        exit,
        frees: pass + 1 == pass_z.len() && z_mm <= EPS_MM,
        cleared: None,
        group: 0,
    })
}

/// The Z of the pass above pass `pass` of `pass_z`, mm, where the tool is
/// when `pass` begins; `None` for the first pass, which begins at safe Z.
/// [`CamError::Internal`] for a pass past the last, whose descent would
/// otherwise start at safe Z as a first pass's does.
fn pass_above_z_mm(pass_z: &[f64], pass: usize) -> Result<Option<f64>, CamError> {
    let Some(above) = pass.checked_sub(1) else {
        return Ok(None);
    };
    at(pass_z, above, "a pass after the first has no pass above it")
        .copied()
        .map(Some)
}

/// Where the tool-centre `path` starts. Every path a Profile cuts has a
/// vertex: [`CamError::Internal`] for one with none, which has nowhere
/// to aim an entry.
fn path_start(path: &Polyline) -> Result<Point, CamError> {
    path.vertices()
        .first()
        .map(|v| v.point())
        .ok_or_else(|| CamError::internal("a tool-centre path has no vertex to start at"))
}

/// What every pass block of an operation shares, for [`pass_block`].
struct PassCtx<'a> {
    op: &'a ProfileOp,
    tabs: Option<&'a Tabs>,
    pass_z: &'a [f64],
    /// The step-down in effect, the operation's or else the tool's, mm
    /// (#348: no plunge goes deeper into uncut material).
    step_down_mm: f64,
    safe_z_mm: f64,
    /// Where a descent starts with no pass above it (#214).
    start_z_mm: f64,
    /// The stock top, mm (#356).
    top_z_mm: f64,
    rates: LapRates,
    tolerance_mm: f64,
    /// The operation's regions: the lines no cut may come nearer than the
    /// bit's radius (#339 review: the last pass's step).
    regions: &'a [Region],
}

/// One tool-centre path, as [`pass_block`] cuts it.
struct PassPath<'p> {
    path: &'p Polyline,
    shape: &'p ShapeRef,
    layout: &'p PathLayout,
    lead: Option<&'p PathLead>,
    /// `None` for a plunge, or a block cut with no entry of its own.
    entry: Option<&'p PathEntry>,
    domain: Option<&'p PathDomain>,
    tool: EntryTool,
}

impl PassPath<'_> {
    /// Below the tab top, `z_mm` cuts round the tabs.
    fn below_tabs(ctx: &PassCtx<'_>, z_mm: f64) -> bool {
        ctx.tabs.is_some_and(|t| z_mm < t.top_z_mm() - EPS_MM)
    }
}

/// The block that cuts `p` at pass `pass`, `z_mm`, its descent by `p`'s
/// entry, and whether it is led in; `None` for a path with no vertices.
/// `previous` is the entry the path's pass before cut (see
/// [`PathEntry::descend`]). A lap that starts on a tab comes down onto it,
/// never into it (#337).
fn pass_block(
    ctx: &PassCtx<'_>,
    p: &PassPath<'_>,
    pass: usize,
    z_mm: f64,
    previous: &mut Option<CutEntry>,
    entry_warnings: &mut EntryWarnings,
) -> Result<Option<(CutBlock, bool)>, CamError> {
    let (op, shape, path) = (ctx.op, p.shape, p.path);
    let path_tabs = match (ctx.tabs, p.layout) {
        (Some(tabs), Some((layout, _))) => Some((tabs, layout)),
        _ => None,
    };
    let Some(start) = path.vertices().first().map(|v| v.point()) else {
        return Ok(None);
    };
    let below_tabs = PassPath::below_tabs(ctx, z_mm);
    let led = p.lead.filter(|l| l.below_tabs || !below_tabs);
    let led_block = match led {
        Some(l) => led_block(
            shape,
            pass,
            z_mm,
            l,
            path_tabs,
            ctx.pass_z,
            ctx.safe_z_mm,
            ctx.rates,
        )?,
        None => None,
    };
    // A lead whose lap would start on a tab is not cut, and nor is the
    // entry planned for its staging point: the arrival is kept off the
    // tabs, so this is for a lead planned where no tab stood.
    let entry_target_lost = led.is_some() && led_block.is_none();
    let is_led = led_block.is_some();
    let (mut block, on_tab, lap_start_z_mm) = match led_block {
        Some(block) => (block, false, z_mm),
        None => {
            let Some(lap) = lap_cuts(path, z_mm, path_tabs, ctx.rates) else {
                return Ok(None);
            };
            let on_tab = lap.start_z_mm > z_mm + EPS_MM;
            let lap_start_z_mm = lap.start_z_mm;
            let block = contour_block(
                shape,
                pass,
                z_mm,
                start,
                lap,
                ctx.pass_z,
                ctx.safe_z_mm,
                ctx.rates,
            )?;
            (block, on_tab || entry_target_lost, lap_start_z_mm)
        }
    };
    // An open path cuts a slot; it frees nothing.
    block.frees &= path.is_closed();
    if let Some(entry) = p.entry {
        let pass_entry = PassEntry {
            path,
            pass,
            z_mm,
            below_tabs,
            start_z_mm: ctx.start_z_mm,
            top_z_mm: ctx.top_z_mm,
            tool: p.tool,
            tolerance_mm: ctx.tolerance_mm,
        };
        if on_tab {
            // The start sits under a tab span: the lap starts on the tab,
            // and the block comes down onto it, never into it (#337); only
            // the first pass that reaches the tab top there plunges, warned.
            if block.descent_from_z_mm - lap_start_z_mm > EPS_MM {
                entry_warnings.record(
                    shape,
                    &plunged(start, EntryKind::of(&op.entry), FallbackReason::NoRoom),
                );
            }
            *previous = Some(CutEntry::new(false, EntryKind::Plunge));
        } else {
            // #214: the first pass's ramp or helix starts just above the
            // stock, not at safe Z.
            let from_z_mm = if pass == 0 {
                ctx.start_z_mm
            } else {
                block.descent_from_z_mm
            };
            let (descent, from_z_mm) = entry.descend(&pass_entry, from_z_mm, previous)?;
            entry_warnings.record(shape, &descent);
            entry.check(op, shape, &pass_entry, &descent, from_z_mm, p.domain)?;
            block.descent_from = descent.from;
            block.descent_from_z_mm = from_z_mm;
            block.descent = descent.moves;
        }
    }
    Ok(Some((block, is_led)))
}

/// A part path's last pass and its stock to leave twin, for
/// [`last_pass_block`].
struct LastPassPaths<'p> {
    /// The part path, cut by the last pass.
    own: PassPath<'p>,
    /// Its stock to leave twin, cut by every pass before.
    twin: PassPath<'p>,
    /// The last pass runs the other way (`LastPass::reverse`).
    reverse: bool,
    /// The part path's waste domains; `None` without (Left, Right).
    domain: Option<&'p PathDomain>,
    stock_to_leave_mm: f64,
}

/// What [`last_pass_block`] made.
struct Entered {
    block: Option<CutBlock>,
    /// The last pass's lead-in was left off: its staging point lies too far
    /// from the twin's kerf to step to.
    lead_dropped: bool,
    /// #335: the tightest inside arc whose feed is held at
    /// `EdgeFeed::floor_fraction`, where the edge moves faster than the feed.
    tight_mm: Option<f64>,
    /// #350: the tightest bend of straight cuts whose edge moves more
    /// than `last_pass::FLATTENED_EDGE_RATIO_MAX` times the feed.
    bend: Option<last_pass::FlattenedBend>,
}

/// #339: the block of a part path's last pass, entered from its stock to leave
/// twin's kerf (`last_pass::enter_from_twin`). The tool descends as one
/// more pass of the twin would, at the twin's start where the pass before
/// ended, by the operation's entry: a plunge, a helix rejoining the twin's
/// bore, or a ramp, which runs along the twin itself, in the kerf every
/// pass before cut (a Left or Right twin cut from its other end, where a
/// reversed last pass starts, is ramped from that end). Every plunge then
/// goes one step-down (or the last pass's thickness) into the twin's floor,
/// never through the stock to leave's uncut strip. `previous` is the twin's.
fn last_pass_block(
    ctx: &PassCtx<'_>,
    paths: &LastPassPaths<'_>,
    pass: usize,
    z_mm: f64,
    previous: &mut Option<CutEntry>,
    entry_warnings: &mut EntryWarnings,
) -> Result<Entered, CamError> {
    let op = ctx.op;
    let twin = &paths.twin;
    // A Left or Right twin, run from the end a reversed last pass starts
    // beside.
    let backwards = (paths.reverse && !twin.path.is_closed()).then(|| {
        (
            twin.path.reversed(),
            twin.layout
                .as_ref()
                .map(|(l, overlapped)| (l.reversed_open(), *overlapped)),
        )
    });
    let (path, layout) = backwards
        .as_ref()
        .map_or((twin.path, twin.layout), |(path, layout)| (path, layout));
    let along = match op.entry {
        Entry::Ramp { angle_deg } => Some(PathEntry::Along {
            angle_deg,
            clear_mm: layout.as_ref().map_or(f64::INFINITY, |(l, _)| {
                l.clear_from_start_mm() - entry::boundary_safety_mm(twin.tool.diameter_mm)
            }),
        }),
        _ => None,
    };
    // A led twin descends at its lead's staging point, where its passes
    // before descended too, by the entry planned there.
    let led = twin
        .lead
        .is_some_and(|l| l.below_tabs || !PassPath::below_tabs(ctx, z_mm));
    let entry = match op.entry {
        Entry::Ramp { .. } if !led => along.as_ref(),
        Entry::Ramp { .. } | Entry::Helix { .. } => twin.entry,
        Entry::Plunge => None,
    };
    let twin_pass = PassPath {
        path,
        shape: twin.shape,
        layout,
        lead: twin.lead,
        entry,
        domain: twin.domain,
        tool: twin.tool,
    };
    let own = PassPath {
        entry: None,
        ..paths.own
    };
    let twin_block = pass_block(ctx, &twin_pass, pass, z_mm, previous, entry_warnings)?;
    let own_block = pass_block(ctx, &own, pass, z_mm, &mut None, entry_warnings)?;
    let (Some((twin_block, _)), Some((mut own_block, own_led))) = (twin_block, own_block) else {
        return Ok(Entered {
            block: None,
            lead_dropped: false,
            tight_mm: None,
            bend: None,
        });
    };
    // #335: its lap runs round inside arcs at the feed its edge needs.
    let (tight_mm, bend) = own_edge_feeds(ctx, paths, &mut own_block, own_led)?;
    let radius_mm = own.tool.diameter_mm / 2.0;
    // #348: its lap comes off a rect tab taller than the step-down in
    // stages, each a step-down at most, running a radius along the lap
    // between them.
    tab_descent::stage_tab_descents(
        &mut own_block,
        ctx.pass_z,
        ctx.step_down_mm,
        radius_mm,
        ctx.rates.plunge_mm_min,
    )?;
    let slack_mm = ctx.tolerance_mm + MIN_BOUNDARY_SAFETY_MM;
    // #339 review: the step is held to what it sweeps: the parts, not
    // the entries' waste domain or tab keep-outs, and the tabs'
    // material, which it runs above.
    // The step comes no nearer any line of the operation than the bit's
    // radius, or than its ends already are (a lap D6 lets run nearer a
    // neighbour): it adds no cut into a part to what the two paths cut.
    let lines: Vec<&Polyline> = ctx
        .regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(Contour::polyline)
        .collect();
    let fits = |from: Point, to: Point| {
        if from.distance_to(to) <= EPS_MM {
            return true;
        }
        let step = Polyline::new(
            vec![Vertex::line(from.x, from.y), Vertex::line(to.x, to.y)],
            false,
        );
        lines.iter().all(|line| {
            let ends_mm = [from, to]
                .iter()
                .filter_map(|p| nearest_point(line, *p))
                .map(|n| n.gap_mm)
                .fold(f64::INFINITY, f64::min);
            distance_mm(&step, line) >= ends_mm.min(radius_mm) - slack_mm
        })
    };
    let clear_z_mm = |from: Point, to: Point| match (ctx.tabs, own.layout) {
        (Some(_), Some((layout, _))) => {
            last_pass::tab_clear_z_mm(own.path, layout, z_mm, radius_mm, (from, to))
        }
        _ => z_mm,
    };
    let rules = last_pass::StepRules {
        fits: &fits,
        clear_z_mm: &clear_z_mm,
    };
    let reach_mm = last_pass::step_reach_mm(own.tool.diameter_mm, paths.stock_to_leave_mm);
    match last_pass::enter_from_twin(&twin_block, own_block, own_led, reach_mm, ctx.rates, &rules) {
        Ok((block, lead_dropped)) => Ok(Entered {
            block: Some(block),
            lead_dropped,
            tight_mm,
            bend,
        }),
        Err(last_pass::StepRefused::OffWaste { from, to }) => {
            let space = paths.domain.map(|d| &d.plain);
            Err(match space {
                Some(space) => off_material(
                    op,
                    own.shape,
                    from,
                    Move::linear(to, ctx.rates.feed_mm_min),
                    space,
                    radius_mm,
                    ctx.tolerance_mm,
                ),
                None => CamError::OffMaterial {
                    op: op.id.clone(),
                    shape: own.shape.clone(),
                    area_mm2: 0.0,
                },
            })
        }
        // Never: every point of the part path lies within the reach of
        // the stock path (`cuts_only_corners`), and a lead-in out of
        // reach is left off.
        Err(last_pass::StepRefused::NoReach) => Err(CamError::ShapeNarrowerThanStockToLeave {
            op: op.id.clone(),
            shape: own.shape.clone(),
            diameter_mm: own.tool.diameter_mm,
            stock_to_leave_mm: paths.stock_to_leave_mm,
        }),
    }
}

/// #335: lowers the feed of the last pass's lap, and its overcut, round
/// inside arcs (`last_pass::EdgeFeed`), so its cutting edge moves at the
/// feed there; its lead arcs, which take up and leave the cut, keep the
/// feed. Returns the tightest arc held at the floor, if any, and (#350)
/// the tightest bend of straight cuts whose edge moves too fast
/// (`last_pass::EdgeFeed::flattened_bend`), whose feed it leaves alone.
/// [`CamError::Internal`] when the lap is not a run of the block's cuts,
/// which would leave its feed as it is.
fn own_edge_feeds(
    ctx: &PassCtx<'_>,
    paths: &LastPassPaths<'_>,
    block: &mut CutBlock,
    led: bool,
) -> Result<(Option<f64>, Option<last_pass::FlattenedBend>), CamError> {
    let edge = last_pass::EdgeFeed {
        wall_on_right: (ctx.op.direction == CutDirection::Climb) != paths.reverse,
        radius_mm: paths.own.tool.diameter_mm / 2.0,
        stock_to_leave_mm: paths.stock_to_leave_mm,
        feed_mm_min: ctx.rates.feed_mm_min,
    };
    let n = block.cuts.len();
    let lead_out = paths.own.lead.and_then(|l| l.lead_out.arc.as_ref());
    let end = match (led, lead_out, block.cuts.last()) {
        (true, Some(arc), Some(last))
            if last.to.xy().distance_to(arc.to) <= EPS_MM
                && matches!(last.kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }) =>
        {
            n - 1
        }
        _ => n,
    };
    let begin = usize::from(led).min(end);
    // Where the lap starts: where the lead-in arc before it ends, or
    // the descent; `None` after a straight plunge, at the pass Z.
    let lead_in = match begin.checked_sub(1) {
        Some(k) => Some(at(
            &block.cuts,
            k,
            "a led lap has no lead-in arc before it",
        )?),
        None => None,
    };
    let start = lead_in.or(block.descent.last()).map(|m| m.to);
    let start_z_mm = start.map_or(block.z_mm, |p| p.z);
    let lap = block
        .cuts
        .get_mut(begin..end)
        .ok_or_else(|| CamError::internal("a last pass's lap is not a run of its block's cuts"))?;
    // #350: its straight cuts keep the feed; a tight bend of them warns.
    let bend = match (start, lap.split_first()) {
        (Some(start), _) => edge.flattened_bend(lap, start),
        (None, Some((first, rest))) => edge.flattened_bend(rest, first.to),
        (None, None) => None,
    };
    Ok((edge.apply(lap, start_z_mm), bend))
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
    /// Each stock to leave twin's part path (#284), by index; `None` for a part
    /// path. One per path.
    part_of: &'a [Option<usize>],
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
        // On line, Left and Right have no waste domain, so no lead.
        let Some(domain) = setup.domains.get(i) else {
            out.push(None);
            continue;
        };
        let d = setup.diameter_mm;
        let at_start = *at(
            setup.fixed,
            i,
            "a Profile path does not say whether its start is set",
        )?;
        // #339: a stock to leave twin is led in beside where its part path's
        // lead arrives, so the last pass, which enters from the twin, finds
        // its lead's staging point beside the twin's. A part path comes
        // before its twin, so its lead is planned; it may have none.
        let part = *at(
            setup.part_of,
            i,
            "a Profile path does not say whether it is a stock path",
        )?;
        let beside = match part.filter(|_| !at_start) {
            Some(part) => at(&out, part, "a stock path is led before its part path")?
                .as_ref()
                .map(|part: &PathLead| part.lead_in.arrival()),
            None => None,
        };
        let plan_in = |space: &Space| {
            let leads = DomainSpace::new(&space.domain, d, setup.tolerance_mm);
            if let Some(lead_in) =
                beside.and_then(|at| lead_in_beside(path, at, setup.lead, d, &leads))
            {
                let lead_out = lead::plan_lead_out(&lead_in.seam, setup.lead, d, &leads);
                return Some((lead_in, lead_out));
            }
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

/// #339: a lead-in onto the closed `path` arriving at its point nearest
/// `at` ([`lead::plan_lead_in_at_start`] on the path re-seamed there), its
/// arrival measured from `path`'s own start; `None` when none fits there.
fn lead_in_beside(
    path: &Polyline,
    at: Point,
    lead: &Lead,
    diameter_mm: f64,
    space: &DomainSpace<'_>,
) -> Option<LeadIn> {
    let near = nearest_point(path, at)?;
    let seamed = lead::reseam_at(path, near.segment, near.along_segment_mm);
    let mut lead_in = lead::plan_lead_in_at_start(&seamed, lead, diameter_mm, space)?;
    let length_mm = path.length_mm();
    if length_mm > EPS_MM {
        lead_in.arrival_mm = (near.along_mm + lead_in.arrival_mm).rem_euclid(length_mm);
    }
    Some(lead_in)
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
/// lap would start on a tab, or the path has no vertices;
/// [`CamError::Internal`] as [`pass_above_z_mm`].
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
) -> Result<Option<CutBlock>, CamError> {
    let seam = &led.lead_in.seam;
    let reseamed = tabs.map(|(t, layout)| (t, layout.reseamed(led.lead_in.arrival_mm)));
    let tabs = reseamed.as_ref().map(|(t, layout)| (*t, layout));
    let Some(lap) = lap_cuts(seam, z_mm, tabs, rates) else {
        return Ok(None);
    };
    if (lap.start_z_mm - z_mm).abs() > EPS_MM {
        return Ok(None);
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
    let descent_from_z_mm = pass_above_z_mm(pass_z, pass)?.unwrap_or(safe_z_mm);
    Ok(Some(CutBlock {
        shape: shape.clone(),
        pass,
        z_mm,
        descent_from: staging,
        descent_from_z_mm,
        descent: Vec::new(),
        entry: staging,
        entry_z_mm: z_mm,
        cuts,
        exit,
        frees: pass + 1 == pass_z.len() && z_mm <= EPS_MM,
        cleared: None,
        group: 0,
    }))
}

/// The `(inner, encloser)` pairs `sequence` needs (invariant O1,
/// `docs/research/toolpath-ordering.md`): every hole before its region's
/// outer, and every region of depth 2 or more before the hole it sits in,
/// so a part inside another part's hole is cut out before that hole frees
/// the waste around it. `regions` are those `nest` returned for the
/// contours of `shapes`, in order: [`CamError::Internal`] otherwise, where
/// a pair left out would let a part be cut free before what is inside it.
pub(crate) fn containment_pairs(
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
) -> Result<Vec<(ShapeRef, ShapeRef)>, CamError> {
    let mut pairs = Vec::new();
    for region in regions {
        let outer = region_outer(region, shapes)?;
        for (_, hole) in hole_shapes(region, shapes)? {
            pairs.push((hole.clone(), outer.clone()));
        }
        // A region at depth 0 or 1 sits in no hole.
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let probe = region
            .outer()
            .polyline()
            .vertices()
            .first()
            .ok_or_else(|| CamError::internal("a region's outline has no vertex"))?
            .point();
        for parent in regions.iter().filter(|r| r.depth() == parent_depth) {
            let enclosing_hole = hole_shapes(parent, shapes)?
                .into_iter()
                .find(|(hole, _)| contour_contains(hole, probe));
            if let Some((_, hole)) = enclosing_hole {
                pairs.push((outer.clone(), hole.clone()));
                break;
            }
        }
    }
    Ok(pairs)
}

/// True when `p` lies inside or on `contour`, whichever way it winds.
pub(crate) fn contour_contains(contour: &Contour, p: Point) -> bool {
    contour.contains_point(p)
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
/// returned for the contours of `shapes`, in order: [`CamError::Internal`]
/// for a region whose outer is of no shape, which would otherwise not be
/// cut.
pub(crate) fn region_outer<'s>(
    region: &Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> Result<&'s ShapeRef, CamError> {
    let (shape, _) = at(
        shapes,
        region.outer_index(),
        "a region's outline is of no target shape",
    )?;
    Ok(shape)
}

/// Each hole of `region` with the shape it came from, in hole order;
/// `region` as for [`region_outer`], and [`CamError::Internal`] likewise
/// for a hole of no shape, which would otherwise not be cut.
pub(crate) fn hole_shapes<'r, 's>(
    region: &'r Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> Result<Vec<(&'r Contour, &'s ShapeRef)>, CamError> {
    region
        .holes()
        .iter()
        .zip(region.hole_indices())
        .map(|(hole, &i)| {
            let (shape, _) = at(shapes, i, "a region's hole is of no target shape")?;
            Ok((hole, shape))
        })
        .collect()
}

/// The outer of `region` then its holes, each with the shape it came
/// from; `region` and the errors as for [`region_outer`].
pub(crate) fn region_shapes<'r, 's>(
    region: &'r Region,
    shapes: &'s [(ShapeRef, Contour)],
) -> Result<Vec<(&'r Contour, &'s ShapeRef)>, CamError> {
    let mut all = vec![(region.outer(), region_outer(region, shapes)?)];
    all.extend(hole_shapes(region, shapes)?);
    Ok(all)
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
    for target in target_profiles(project, op, targets) {
        let target = target?;
        if !target.is_closed() {
            let Some(open) = open.as_deref_mut() else {
                warnings.push(Warning::OpenPathSkipped {
                    shape: target.shape,
                    reason: OpenPathReason::NoSide,
                });
                continue;
            };
            let placed = target.placed()?;
            if placed.length_mm() <= EPS_MM {
                warnings.push(Warning::OpenPathSkipped {
                    shape: target.shape,
                    reason: OpenPathReason::NoLength,
                });
                continue;
            }
            open.push((target.shape, placed));
            continue;
        }
        let contour = target.contour()?;
        shapes.push((target.shape, contour));
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
mod tests;
