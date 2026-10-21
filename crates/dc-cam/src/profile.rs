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
mod waste;

use crate::block::CutBlock;
use crate::entry::{
    self, CutSide, EntryKind, EntryTool, EntryWarnings, FallbackReason, MIN_BOUNDARY_SAFETY_MM,
};
use crate::error::{CamError, FeedField, ShapeRef};
use crate::lead::{self, DomainSpace, DropReason, LeadArc, LeadIn, LeadOut, LeadWarnings};
use crate::overlap::{CutPath, overlap_warnings};
use crate::passes::pass_z_mm;
use crate::relief;
use crate::sequence::{SequenceInput, plan, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::{
    LapRates, PathTabs, Tabs, coverage_warning, cut_range_warnings, lap_cuts, overlap_warning,
};
use crate::toolpath::{Move, Toolpath};
use crate::warning::{Warning, WarningCode};
use dc_geometry::{
    Contour, EPS_MM, GeometryError, GeometryWarning, Point, Point3, Polyline, Region, nest, offset,
};
use dc_project::{
    CutDirection, Entry, InstanceId, Lead, OperationId, ProfileOp, Project, Side, Tool,
};
use entries::{
    EntrySetup, PassEntry, PathDomain, Space, Targets, off_material, path_entries, plunged,
    wall_domains,
};

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
/// With `op.tabs`, every pass below the tab top rises over each tab span
/// (`width + diameter` along the tool-centre contour); overlapping spans
/// merge with [`WarningCode::TabsOverlap`], spans covering more than half a
/// contour give [`WarningCode::TabsCoverMostOfContour`] (the tabs are still
/// cut as set), and a tab top no pass reaches,
/// or one above the stock top, gives [`WarningCode::TabsOutsideCutRange`].
///
/// With `op.entry` a ramp or helix (design spec, "Algorithms → Entry"),
/// each pass reaches its depth at the contour start without a plunge:
/// Inside and Outside place the zigzag or helix in the waste domain
/// beside the wall ([`entry::plan_at_wall`]), the waste less every other
/// region's kerf and, below the tab top, less the tabs; On line ramps back
/// and forth along the path itself ([`entry::along_path`]). The first pass
/// descends from safe Z, later ones from the previous pass Z at the
/// contour start, where the previous lap ended. An entry that does not
/// fit falls back to a ramp, then a plunge, with
/// [`WarningCode::EntryFallback`]; so does a contour whose start is under
/// a tab.
///
/// With `op.lead` (design spec, "Algorithms → Lead-in and lead-out"),
/// Inside and Outside contours are re-seamed where an arc tangent to them
/// fits in the waste domain away from corners ([`crate::lead`]): each
/// pass descends at the arc's far end, its staging point, cuts the arc,
/// the lap from the arrival, the overcut and the exit arc at the feed
/// rate, and retracts. Below the tab top the arcs keep off the tabs, and
/// the tabs stay where they were along the outline. A lead that fits only
/// smaller warns [`WarningCode::LeadShrunk`]; one that does not fit, an
/// exit that does not fit, and any lead on an On line Profile warn
/// [`WarningCode::LeadDropped`].
///
/// With `op.corner_relief`, a dogbone or T-bone notch is cut in each
/// corner the round tool cannot reach (Inside: the hole's corners;
/// Outside: the part's concave corners), stepping down at the tool's
/// step-down among the shape's own passes, each level after the pass that
/// opened the column it plunges in and none after the pass that frees the
/// shape (the `relief` module, #189); On line warns
/// [`WarningCode::ReliefSkipped`].
///
/// Open paths, shapes the tool cannot fit, and duplicate shapes are skipped
/// with a [`Warning`]. Shapes in different regions whose tool paths cut
/// into each other give [`WarningCode::ToolpathsOverlap`], in cut order. Fails without a toolpath when a feed, plunge rate, or
/// step-down is missing, when the depth goes below the stock bottom plus
/// allowance, when targets cross, when an id does not resolve, when the
/// cutter would leave the stock ([`CamError::OffStock`]), when an entry's
/// angle or helix fraction is out of range ([`CamError::InvalidEntry`]),
/// when a lead's radius, sweep or overcut is ([`CamError::InvalidLead`]),
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
            cutter: tool.cutter.describe(),
        });
    }
    let rates = rates_of(tool)?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
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
    if op.side == Side::OnLine && matches!(op.entry, Entry::Helix { .. }) {
        return Err(CamError::HelixNeedsWasteSide { op: op.id.clone() });
    }
    let step_down_mm =
        op.step_down_mm
            .or(tool.step_down_mm)
            .ok_or_else(|| CamError::MissingStepDown {
                op: op.id.clone(),
                tool: tool.name.clone(),
            })?;
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let depth_mm = op
        .depth_mm
        .ok_or_else(|| CamError::MissingDepth { op: op.id.clone() })?;
    let pass_z = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        step_down_mm,
        op.through_cut_allowance_mm,
    )?;
    let safe_z_mm = thickness_mm + op.safe_z_mm;

    let tabs = op
        .tabs
        .as_ref()
        .map(|spec| Tabs::new(&op.id, spec, tool.diameter_mm, project.flatten_tolerance_mm))
        .transpose()?;

    let mut warnings = Vec::new();
    if let Some(tabs) = &tabs {
        warnings.extend(cut_range_warnings(&op.id, tabs, &pass_z, thickness_mm));
    }
    let shapes = resolve_targets(project, &op.id, &op.targets, &mut warnings)?;
    let contours: Vec<Contour> = shapes.iter().map(|(_, c)| c.clone()).collect();
    let (regions, nest_warnings) = nest(contours).map_err(|e| crossing_error(e, &shapes))?;
    for warning in nest_warnings {
        let GeometryWarning::DuplicateContourDropped {
            index,
            duplicate_of,
        } = warning;
        if let (Some((dropped, _)), Some((kept, _))) = (shapes.get(index), shapes.get(duplicate_of))
        {
            warnings.push(
                shape_warning(WarningCode::DuplicateShapeDropped, dropped)
                    .with("duplicate_of_instance", &kept.instance)
                    .with("duplicate_of_profile", kept.profile),
            );
        }
    }

    let radius_mm = tool.diameter_mm / 2.0;
    // Each tool-centre path with the shape it cuts, the wall it cuts and
    // the index of its region, in drawing order.
    let mut placed: Vec<(Polyline, &ShapeRef, Wall, usize)> = Vec::new();
    for (region_index, region) in regions.iter().enumerate() {
        let ordered = region
            .holes()
            .iter()
            .map(|h| (h, Role::Hole))
            .chain(std::iter::once((region.outer(), Role::Outer)));
        for (contour, role) in ordered {
            let (how, wall) = placement(op.side, role);
            let centres = match how {
                Offset::Grow => offset(contour, radius_mm),
                Offset::Shrink => offset(contour, -radius_mm),
                Offset::None => vec![contour.clone()],
            };
            // `nest` keeps every contour or reverses it, so this always
            // finds the shape.
            let Some(shape) = shape_of(&shapes, contour) else {
                continue;
            };
            if centres.is_empty() {
                warnings.push(
                    shape_warning(WarningCode::ToolTooLarge, shape)
                        .with("tool", &tool.name)
                        .with("diameter_mm", tool.diameter_mm),
                );
                continue;
            }
            let ccw = traversal_is_ccw(wall, op.direction);
            placed.extend(centres.into_iter().map(|c| {
                let path = if c.is_ccw() == ccw {
                    c.into_polyline()
                } else {
                    c.reversed().into_polyline()
                };
                (path, shape, wall, region_index)
            }));
        }
    }
    // Drawing order is the order the targets were resolved in; stable, so
    // the loops of one shape keep their offset order.
    let drawing_index = |shape: &ShapeRef| shapes.iter().position(|(s, _)| s == shape);
    placed.sort_by_key(|(_, shape, _, _)| drawing_index(shape));
    let walls: Vec<(Wall, usize)> = placed.iter().map(|(_, _, w, r)| (*w, *r)).collect();
    let paths: Vec<(Polyline, &ShapeRef)> = placed.into_iter().map(|(p, s, _, _)| (p, s)).collect();

    // Tab layout per path, and one block per path per pass.
    let layouts: Vec<Option<(PathTabs, bool)>> = paths
        .iter()
        .map(|(path, _)| tabs.as_ref().map(|tabs| tabs.layout(path.length_mm())))
        .collect();
    let entry_tool = |wall: Wall| EntryTool {
        diameter_mm: tool.diameter_mm,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        direction: op.direction,
        side: match wall {
            Wall::Inside => CutSide::Internal,
            Wall::Outside => CutSide::External,
        },
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
        entry_tool: &entry_tool,
        tolerance_mm: project.flatten_tolerance_mm,
    };
    let lead = op.lead.as_ref();
    if let Some(lead) = lead {
        lead::check(&op.id, lead)?;
        if op.side == Side::OnLine {
            warnings.push(lead::on_line_warning(&op.id));
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
            },
            &mut lead_warnings,
        )?,
        None => paths.iter().map(|_| None).collect(),
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
    let mut blocks = Vec::with_capacity(paths.len() * pass_z.len());
    // The path each block cuts, for naming contours in cut order.
    let mut block_paths = Vec::with_capacity(blocks.capacity());
    for (i, ((path, shape), layout)) in paths.iter().zip(&layouts).enumerate() {
        let path_tabs = match (&tabs, layout) {
            (Some(tabs), Some((layout, _))) => Some((tabs, layout)),
            _ => None,
        };
        let Some(start) = path.vertices().first().map(|v| v.point()) else {
            continue;
        };
        let wall = walls.get(i).map_or(Wall::Outside, |(w, _)| *w);
        let mut previous: Option<(bool, EntryKind)> = None;
        for (pass, &z_mm) in pass_z.iter().enumerate() {
            let below_tabs = below_tabs(z_mm);
            let led = leads
                .get(i)
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
            if let Some(entry) = entries.get(i).and_then(Option::as_ref) {
                let pass_entry = PassEntry {
                    path,
                    pass,
                    z_mm,
                    below_tabs,
                    safe_z_mm,
                    tool: entry_tool(wall),
                    tolerance_mm: project.flatten_tolerance_mm,
                };
                if on_tab {
                    // The start sits under a tab span: the lap starts on
                    // the tab, and the block plunges onto it, as phase 1.
                    if block.descent_from_z_mm - lap_start_z_mm > EPS_MM {
                        entry_warnings.record(
                            shape,
                            &plunged(start, EntryKind::of(&op.entry), FallbackReason::NoRoom),
                        );
                    }
                    previous = Some((false, EntryKind::Plunge));
                } else {
                    let (descent, from_z_mm) =
                        entry.descend(&pass_entry, block.descent_from_z_mm, &mut previous);
                    entry_warnings.record(shape, &descent);
                    entry.check(op, shape, &pass_entry, &descent, from_z_mm, domains.get(i))?;
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
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
    };

    // Warnings name contours by their position in the cut order, which
    // the plan sets.
    let cut_order = plan(&input)?;
    let mut path_order: Vec<usize> = Vec::with_capacity(paths.len());
    for index in cut_order {
        if let Some(&path_index) = block_paths.get(index)
            && !path_order.contains(&path_index)
        {
            path_order.push(path_index);
        }
    }
    // Paths the plan never reached (none in practice) keep their place.
    for index in 0..paths.len() {
        if !path_order.contains(&index) {
            path_order.push(index);
        }
    }
    for (cut_index, &path_index) in path_order.iter().enumerate() {
        let (Some((path, shape)), Some(Some((layout, overlapped)))) =
            (paths.get(path_index), layouts.get(path_index))
        else {
            continue;
        };
        let Some(tabs) = &tabs else {
            continue;
        };
        let length_mm = path.length_mm();
        if *overlapped {
            warnings.push(overlap_warning(&op.id, cut_index, Some(shape), length_mm));
        }
        warnings.extend(coverage_warning(
            &op.id,
            cut_index,
            Some(shape),
            tabs,
            layout,
        ));
    }
    // D6: paths in cut order, so the warnings follow it.
    let cut_paths: Vec<CutPath<'_>> = path_order
        .iter()
        .filter_map(|&i| paths.get(i))
        .map(|(path, shape)| (path, Some(*shape)))
        .collect();
    warnings.extend(overlap_warnings(
        &cut_paths, &shapes, &regions, op.side, tool,
    ));

    // Corner relief among each shape's own passes, never after the pass
    // that frees it (`crate::relief`, #189).
    let geometry = relief::ProfileGeometry {
        regions: &regions,
        shapes: &shapes,
        paths: &paths,
        walls: &walls,
    };
    let notches =
        relief::profile_notches(project, op, tool, &geometry, &input.blocks, &mut warnings)?;
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
    Ok((toolpath, warnings))
}

/// The block that cuts one lap of a tool-centre contour at pass `pass`:
/// a straight plunge to `start` (into the lap's start Z, which a tab span
/// over the path start raises above `z_mm`), then the lap. The descent
/// starts at safe Z on the first pass and at the previous pass Z after.
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
    let descent_from_z_mm = pass
        .checked_sub(1)
        .and_then(|k| pass_z.get(k).copied())
        .unwrap_or(safe_z_mm);
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
}

/// Each path's lead, in order, or `None` where none fits (or On line).
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
        let plan_in = |space: &Space| {
            let leads = DomainSpace::new(&space.domain, d, setup.tolerance_mm);
            lead::plan_lead_in(path, setup.lead, d, &leads, from).map(|lead_in| {
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
        let Some(outer) = shape_of(shapes, region.outer()) else {
            continue;
        };
        for hole in region.holes() {
            if let Some(hole) = shape_of(shapes, hole) {
                pairs.push((hole.clone(), outer.clone()));
            }
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
            .flat_map(|r| r.holes())
            .find(|hole| contour_contains(hole, probe.point()));
        if let Some(hole) = enclosing_hole.and_then(|h| shape_of(shapes, h)) {
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
                warnings.push(shape_warning(WarningCode::OpenPathSkipped, &shape));
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

/// A warning about `shape`, with its `instance`, `instance_name`, and
/// `profile` params set.
pub(crate) fn shape_warning(code: WarningCode, shape: &ShapeRef) -> Warning {
    Warning::new(code)
        .with("instance", &shape.instance)
        .with("instance_name", &shape.instance_name)
        .with("profile", shape.profile)
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
        MachiningOrder, Operation, OperationId, Side, Stock, TabPlacement, TabShape, TabSpec,
        ToolId,
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
        warnings.iter().map(|w| w.code).collect()
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
        assert_eq!(warnings[0].param("instance"), Some("i1"));
        assert_eq!(warnings[0].param("profile"), Some("1"));
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
        assert_eq!(warnings[0].param("instance"), Some("i1"));
        assert_eq!(warnings[0].param("profile"), Some("1"));
        assert_eq!(warnings[0].param("tool"), Some("6.35 mm upcut"));
        for z in PASS_Z_MM {
            let laps = laps_at(&tp, z);
            assert_eq!(laps.len(), 1, "only the outer at Z {z}");
            assert_approx_eq!(laps[0].x_extent_mm(), 106.35, TOL);
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
            if mv.to.z < prev.to.z - EPS_MM {
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
        // Hole done, retract, rapid across at safe Z, outer starts.
        let rapids: Vec<&Move> = tp.moves.iter().filter(|m| m.is_rapid()).collect();
        assert_eq!(rapids.len(), 4);
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
        assert_eq!(warnings[0].param("instance"), Some("i2"));
        assert_eq!(warnings[0].param("duplicate_of_instance"), Some("i1"));
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
                field: FeedField::Feed
            }
        );

        let mut p = rect_project();
        p.tools[0].plunge_mm_min = None;
        assert_eq!(
            generate_profile(&p, &op).unwrap_err(),
            CamError::MissingFeed {
                tool: "6.35 mm upcut".into(),
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
        let mut expected = vec![Move::rapid(Point3::new(start.x, start.y, SAFE_Z_MM))];
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

    /// A tab warning's `contour` param counts contours in cut order, which
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
        });
        let (_, warnings) = generate(&p, &op);
        let covered: Vec<(Option<&str>, Option<&str>)> = warnings
            .iter()
            .filter(|w| w.code == WarningCode::TabsCoverMostOfContour)
            .map(|w| (w.param("contour"), w.param("instance")))
            .collect();
        assert_eq!(
            covered,
            [
                (Some("0"), Some("i1")),
                (Some("1"), Some("i3")),
                (Some("2"), Some("i2")),
            ]
        );
    }
}
