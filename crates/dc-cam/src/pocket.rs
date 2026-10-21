// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts
//   src/engine/toolpaths/pocketPatterns.ts

//! Pocket toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Pocket",
//! steps 1–2, 4–8, 11–12): clear the inside of closed shapes with
//! concentric rings or raster lines, leaving nested shapes as islands. The
//! ring tree is in [`rings`], the raster lines in [`raster`]; pocket
//! allowance and rest machining land in later slices.
//!
//! Re-derived for Y-up (ADR-0001) rather than copied: PureCutCNC's "conventional
//! is a no-op on Clipper output" holds because its positive-area outers are
//! machine-clockwise in Y-down screen space. Here a positive-area (CCW)
//! outer ring is the climb direction on a pocket wall, and a clockwise
//! island ring is climb round an island, so conventional reverses both
//! ([`ring_is_ccw`]).

pub mod raster;
pub mod rings;

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::footprint::footprint_at;
use crate::passes::pass_z_mm;
use crate::profile::{
    contour_contains, crossing_error, rates_of, resolve_targets, shape_of, shape_warning,
};
use crate::sequence::{SequenceInput, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::lap_cuts;
use crate::toolpath::Toolpath;
use crate::warning::{Warning, WarningCode};
use dc_geometry::{
    Contour, EPS_MM, GeometryWarning, Point, Polyline, Region, area_mm2, difference, distance_mm,
    nest,
};
use dc_project::{
    ClearanceStrategy, CornerRelief, CutDirection, Entry, PocketOp, Project, Tool, WallPass,
};
use raster::RasterSettings;
use rings::{Ring, RingKind, RingTree, WallRings, inset_region};

/// Most rings one tool-centre region may have. Bounds the work a tiny
/// step-over can cause (a 0.01 step-over of a 6 mm tool on a 1 m wide
/// pocket is still under it).
pub const MAX_RINGS: usize = 10_000;

/// Which wall a ring runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingWall {
    /// The pocket wall, or an interior ring stepping in from it: the tool
    /// runs inside the loop, as round a hole.
    Pocket,
    /// An island, or an interior ring stepping out from it: the tool runs
    /// outside the loop, as around a part.
    Island,
}

/// The direction rule (design step 8): true when a ring along `wall` is
/// traversed counter-clockwise (viewed from +Z, Y-up) for `direction`.
///
/// Climb milling keeps the uncut material on the right of travel (the
/// spindle turns clockwise from above). Along the pocket wall the material
/// is outside the loop, so climb is counter-clockwise; round an island the
/// material is inside the loop, so climb is clockwise. Conventional is the
/// reverse of climb. Interior rings follow the wall they step in from.
#[must_use]
pub fn ring_is_ccw(wall: RingWall, direction: CutDirection) -> bool {
    let climb_is_ccw = match wall {
        RingWall::Pocket => true,
        RingWall::Island => false,
    };
    match direction {
        CutDirection::Climb => climb_is_ccw,
        CutDirection::Conventional => !climb_is_ccw,
    }
}

/// Generates the Pocket toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Each target instance's closed profiles are placed and nested; each
/// region is a pocket and its holes are islands, and a region inside an
/// island is a pocket of its own, cut after the pocket around it. The
/// tool-centre region is the pocket inset by the tool radius
/// ([`rings::inset_region`]); where it is empty the shape is skipped with
/// [`WarningCode::PocketNoRoom`], and an island closer than the tool's
/// diameter to the wall or another island warns
/// [`WarningCode::PocketIslandTooClose`]. With the offset strategy each
/// tool-centre region's rings step in by `diameter × step_over_fraction`
/// ([`RingTree`]); per depth pass they are cut innermost first, with the
/// wall and island rings last, first, or not at all per `op.wall_pass`,
/// each ring starting at its vertex nearest the previous ring's end and
/// running per [`ring_is_ccw`]. With the raster strategy each tool-centre
/// region is crossed by lines at the raster angle, one step-over apart and
/// alternating in direction ([`raster::raster_lines`]), whatever
/// `op.direction`; the wall and island rings follow (or precede) them as
/// for offset. [`sequence`] orders and links the rings and lines
/// (stay-down links inside the cleared tool-centre area, else a retract to
/// safe Z).
///
/// Fails without a toolpath when a rate, the step-down or the step-over is
/// missing, when the depth goes below the stock bottom plus allowance, when
/// the raster angle is outside [0, 180) ([`CamError::InvalidRasterAngle`]),
/// when targets cross, when an id does not resolve, when the cutter is not
/// a flat end mill, when the cutter would leave the stock
/// ([`CamError::OffStock`]) or its footprint would leave the pocket's
/// material (S9, [`CamError::OffMaterial`]). Not yet generated: an
/// allowance and rest machining fail with
/// [`CamError::NotSupportedYet`]; a ramp or helix entry plunges instead
/// ([`WarningCode::EntryFallback`]) and corner relief is skipped
/// ([`WarningCode::ReliefSkipped`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_pocket(
    project: &Project,
    op: &PocketOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    // The tool-centre region is the pocket inset by half the diameter,
    // right only for a flat end mill (#67).
    if !tool.cutter.is_flat() {
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
    if let ClearanceStrategy::Raster { angle_deg } = op.strategy
        && !(angle_deg.is_finite() && (0.0..180.0).contains(&angle_deg))
    {
        return Err(CamError::InvalidRasterAngle {
            op: op.id.clone(),
            angle_deg,
        });
    }
    refuse_later_slices(op)?;
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
    let step_over_fraction = op
        .step_over_fraction
        .or(tool.step_over_fraction)
        .ok_or_else(|| CamError::MissingStepOver {
            op: op.id.clone(),
            tool: tool.name.clone(),
        })?;
    let invalid_step_over = || CamError::InvalidStepOver {
        op: op.id.clone(),
        step_over_fraction,
    };
    if !(step_over_fraction.is_finite() && step_over_fraction > 0.0 && step_over_fraction <= 1.0) {
        return Err(invalid_step_over());
    }
    let stepover_mm = tool.diameter_mm * step_over_fraction;
    let radius_mm = tool.diameter_mm / 2.0;
    let tolerance_mm = project.flatten_tolerance_mm;

    let mut warnings = fallback_warnings(op);
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

    let wall = match op.wall_pass {
        WallPass::Last => WallRings::Last,
        WallPass::First => WallRings::First,
        WallPass::None => WallRings::Omitted,
    };
    // Every ring and raster line in cut order: its shape, its path (a ring
    // oriented and starting at its entry, a line from start to end), and
    // the tool-centre area it has cleared.
    let mut rings: Vec<(ShapeRef, Polyline, Option<Region>)> = Vec::new();
    let mut anchor: Option<Point> = None;
    let mut root_index = 0;
    for region in &regions {
        // `nest` keeps every contour or reverses it, so this always finds
        // the shape.
        let Some(shape) = shape_of(&shapes, region.outer()) else {
            continue;
        };
        warnings.extend(island_gap_warnings(op, tool, region, &shapes, shape));
        let tool_centre = inset_region(region, radius_mm, tolerance_mm);
        if tool_centre.is_empty() {
            warnings.push(
                shape_warning(WarningCode::PocketNoRoom, shape)
                    .with("op", &op.id)
                    .with("tool", &tool.name)
                    .with("diameter_mm", tool.diameter_mm),
            );
            continue;
        }
        for root in tool_centre {
            if let ClearanceStrategy::Raster { angle_deg } = op.strategy {
                let settings = RasterSettings {
                    stepover_mm,
                    radius_mm,
                    angle_deg,
                    tolerance_mm,
                    wall,
                    direction: op.direction,
                };
                let paths = raster::region_paths(&root, root_index, &settings, &mut anchor)
                    .ok_or_else(invalid_step_over)?;
                rings.extend(
                    paths
                        .into_iter()
                        .map(|(path, cleared)| (shape.clone(), path, cleared)),
                );
                root_index += 1;
                continue;
            }
            let tree = RingTree::build(root, stepover_mm, tolerance_mm, MAX_RINGS)
                .ok_or_else(invalid_step_over)?;
            for ring in tree.walk(wall, &mut anchor) {
                let Some(node) = tree.nodes.get(ring.node) else {
                    continue;
                };
                let cleared = Some(node.region.clone());
                rings.push((shape.clone(), ring_path(ring, op.direction), cleared));
            }
        }
    }

    let mut blocks = Vec::with_capacity(rings.len() * pass_z.len());
    for (pass, &z_mm) in pass_z.iter().enumerate() {
        let descent_from_z_mm = pass
            .checked_sub(1)
            .and_then(|k| pass_z.get(k).copied())
            .unwrap_or(safe_z_mm);
        for (shape, path, cleared) in &rings {
            let Some(start) = path.vertices().first().map(|v| v.point()) else {
                continue;
            };
            // A ring ends where it starts; a raster line at its far end.
            let exit = if path.is_closed() {
                start
            } else {
                path.vertices().last().map_or(start, |v| v.point())
            };
            let Some(lap) = lap_cuts(path, z_mm, None, rates) else {
                continue;
            };
            blocks.push(CutBlock {
                shape: shape.clone(),
                pass,
                z_mm,
                descent_from: start,
                descent_from_z_mm,
                descent: Vec::new(),
                entry: start,
                cuts: lap.moves,
                exit,
                // A pocket never cuts a part free.
                frees: false,
                cleared: cleared.clone(),
                group: pass,
            });
        }
    }
    let containment = nested_pockets(&regions, &shapes);
    let input = SequenceInput {
        blocks,
        order: op.machining_order,
        containment: &containment,
        safe_z_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
    };
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
    check_in_material(op, &toolpath, radius_mm, tolerance_mm, &regions, &shapes)?;
    Ok((toolpath, warnings))
}

/// Typed errors for what later slices generate: an allowance and rest
/// machining (slice 6). Each would cut something other than what was
/// asked, so none is silently ignored.
fn refuse_later_slices(op: &PocketOp) -> Result<(), CamError> {
    let feature = if op.allowance_mm > 0.0 {
        Some("a pocket allowance (allowance_mm)")
    } else if op.rest_of.is_some() {
        Some("rest machining (rest_of)")
    } else {
        None
    };
    match feature {
        Some(feature) => Err(CamError::NotSupportedYet {
            op: op.id.clone(),
            feature: feature.into(),
        }),
        None => Ok(()),
    }
}

/// Warnings for the entry and corner relief this build replaces with a
/// plunge and nothing (slices 9 and 12). Both still give the pocket asked
/// for, so they warn rather than fail.
fn fallback_warnings(op: &PocketOp) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let entry = match op.entry {
        Entry::Plunge => None,
        Entry::Ramp { .. } => Some("ramp"),
        Entry::Helix { .. } => Some("helix"),
    };
    if let Some(requested) = entry {
        warnings.push(
            Warning::new(WarningCode::EntryFallback)
                .with("op", &op.id)
                .with("requested", requested)
                .with("used", "plunge")
                .with("reason", "not_supported_yet"),
        );
    }
    let relief = match op.corner_relief {
        CornerRelief::None => None,
        CornerRelief::Dogbone => Some("dogbone"),
        CornerRelief::TBone => Some("t_bone"),
    };
    if let Some(requested) = relief {
        warnings.push(
            Warning::new(WarningCode::ReliefSkipped)
                .with("op", &op.id)
                .with("requested", requested)
                .with("reason", "not_supported_yet"),
        );
    }
    warnings
}

/// [`WarningCode::PocketIslandTooClose`] for every island of `region`
/// nearer than the tool's diameter to the wall or to a later island.
fn island_gap_warnings(
    op: &PocketOp,
    tool: &Tool,
    region: &Region,
    shapes: &[(ShapeRef, Contour)],
    shape: &ShapeRef,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let warn = |island: &ShapeRef, other: String, gap_mm: f64| {
        shape_warning(WarningCode::PocketIslandTooClose, shape)
            .with("op", &op.id)
            .with("island", island.profile)
            .with("other", other)
            .with("gap_mm", gap_mm)
            .with("tool", &tool.name)
            .with("diameter_mm", tool.diameter_mm)
    };
    let holes = region.holes();
    for (i, hole) in holes.iter().enumerate() {
        let Some(island) = shape_of(shapes, hole) else {
            continue;
        };
        let gap_mm = distance_mm(hole.polyline(), region.outer().polyline());
        if gap_mm < tool.diameter_mm - EPS_MM {
            warnings.push(warn(island, "wall".into(), gap_mm));
        }
        for other in holes.iter().skip(i + 1) {
            let Some(other_shape) = shape_of(shapes, other) else {
                continue;
            };
            let gap_mm = distance_mm(hole.polyline(), other.polyline());
            if gap_mm < tool.diameter_mm - EPS_MM {
                warnings.push(warn(island, other_shape.profile.to_string(), gap_mm));
            }
        }
    }
    warnings
}

/// `ring` as a path: wound by the direction rule ([`ring_is_ccw`]) for
/// `direction`, starting at its start vertex.
fn ring_path(ring: Ring, direction: CutDirection) -> Polyline {
    let side = match ring.kind {
        RingKind::Outer => RingWall::Pocket,
        RingKind::Hole => RingWall::Island,
    };
    let path = if ring.contour.is_ccw() == ring_is_ccw(side, direction) {
        ring.contour.into_polyline()
    } else {
        ring.contour.reversed().into_polyline()
    };
    starting_at(&path, ring.start)
}

/// `path` (closed) rotated to start at its vertex at `start`
/// (`rotateContourToNearestEntry`); unchanged when no vertex is there.
fn starting_at(path: &Polyline, start: Point) -> Polyline {
    let mut vertices = path.vertices().to_vec();
    let index = vertices
        .iter()
        .position(|v| v.point().distance_to(start) <= EPS_MM)
        .unwrap_or(0);
    // A vertex carries the bulge of the segment it starts, so rotating the
    // list keeps every arc.
    vertices.rotate_left(index);
    Polyline::new(vertices, true)
}

/// Containment pairs for `sequence`: a pocket inside an island (a region of
/// depth 2 or more) is cut after the pocket that holds the island, so each
/// pair is `(pocket around, pocket inside)`. Pockets free nothing, so this
/// orders the work only; it is the design's "after the island".
fn nested_pockets(regions: &[Region], shapes: &[(ShapeRef, Contour)]) -> Vec<(ShapeRef, ShapeRef)> {
    let mut pairs = Vec::new();
    for region in regions {
        let Some(parent_depth) = region.depth().checked_sub(2) else {
            continue;
        };
        let (Some(inner), Some(probe)) = (
            shape_of(shapes, region.outer()),
            region.outer().polyline().vertices().first(),
        ) else {
            continue;
        };
        let around = regions
            .iter()
            .filter(|r| r.depth() == parent_depth)
            .find(|r| r.holes().iter().any(|h| contour_contains(h, probe.point())));
        if let Some(outer) = around.and_then(|r| shape_of(shapes, r.outer())) {
            pairs.push((outer.clone(), inner.clone()));
        }
    }
    pairs
}

/// S9: the tool's footprint along `toolpath` lies in the pockets'
/// material, inside every region and outside every island. Flattening
/// moves both outlines by up to `tolerance_mm`, so area within that band
/// of the boundaries is allowed.
fn check_in_material(
    op: &PocketOp,
    toolpath: &Toolpath,
    radius_mm: f64,
    tolerance_mm: f64,
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
) -> Result<(), CamError> {
    let swept = footprint_at(toolpath, radius_mm, tolerance_mm);
    if swept.is_empty() {
        return Ok(());
    }
    let outside = difference(&swept, regions, tolerance_mm);
    let boundary_mm: f64 = regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().length_mm())
        .sum();
    let allowed_mm2 = (tolerance_mm + EPS_MM) * boundary_mm;
    let area = area_mm2(&outside);
    if area <= allowed_mm2 {
        return Ok(());
    }
    // Name the pocket whose outline holds the stray area, else the first.
    let probe = outside
        .iter()
        .find_map(|r| r.outer().polyline().vertices().first().map(|v| v.point()));
    let named = regions
        .iter()
        .find(|r| probe.is_some_and(|p| contour_contains(r.outer(), p)))
        .or_else(|| regions.first())
        .and_then(|r| shape_of(shapes, r.outer()));
    match named {
        Some(shape) => Err(CamError::OffMaterial {
            op: op.id.clone(),
            shape: shape.clone(),
            area_mm2: area,
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CamError;
    use crate::footprint::footprint;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind, Toolpath};
    use crate::warning::WarningCode;
    use dc_geometry::{
        Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Rect, Region, Transform,
        area_mm2, assert_approx_eq, difference, intersection, nest, offset,
    };
    use dc_project::test_support::sample_project;
    use dc_project::{
        ClearanceStrategy, CornerRelief, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, OperationId, Stock, ToolId, WallPass,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::TAU;

    const TOL: f64 = 1e-6;
    /// Rings and bounds are compared to this: the design's "± 0.01".
    const RING_TOL: f64 = 0.01;
    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    const DIAMETER_MM: f64 = 6.35;
    const RADIUS_MM: f64 = DIAMETER_MM / 2.0;
    /// 40 % of the diameter.
    const STEPOVER_MM: f64 = 2.54;
    /// Depth 4 in steps of 2 on 6 mm stock.
    const DEPTH_MM: f64 = 4.0;
    const PASS_Z_MM: [f64; 2] = [4.0, 2.0];
    const FLAT_TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

    /// The 100 × 50 rect every pocket test starts from, placed at (20, 20).
    fn rect_100x50() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)
    }

    fn at_20_20() -> Transform {
        Transform::translate(20.0, 20.0)
    }

    /// The placed rect's bounds.
    fn wall() -> Rect {
        Rect {
            min: Point::new(20.0, 20.0),
            max: Point::new(120.0, 70.0),
        }
    }

    /// `rect` shrunk by `inset_mm` on every side.
    fn inset(rect: Rect, inset_mm: f64) -> Rect {
        Rect {
            min: Point::new(rect.min.x + inset_mm, rect.min.y + inset_mm),
            max: Point::new(rect.max.x - inset_mm, rect.max.y - inset_mm),
        }
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

    fn rect_project() -> Project {
        project(vec![rect_100x50()], &[("i1", at_20_20())])
    }

    /// The rect with a 20 mm circular island at its centre, (70, 45).
    fn island_project() -> Project {
        project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ],
            &[("i1", at_20_20())],
        )
    }

    /// An offset Pocket of every instance in `project`, 4 mm deep in the
    /// tool's 2 mm steps, 40 % step-over, wall pass last, plunging.
    fn pocket_op(project: &Project, direction: CutDirection) -> PocketOp {
        PocketOp {
            id: OperationId::new("o1"),
            name: "Pocket".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: Some(DEPTH_MM),
            step_down_mm: None,
            step_over_fraction: Some(0.4),
            direction,
            allowance_mm: 0.0,
            rest_of: None,
            entry: Entry::Plunge,
            corner_relief: CornerRelief::None,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &PocketOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_pocket(project, op).unwrap();
        assert_safe(
            &toolpath,
            project,
            &dc_project::Operation::Pocket(op.clone()),
        );
        (toolpath, warnings)
    }

    /// One closed ring of cutting moves at one Z, in cut order.
    #[derive(Debug, Clone)]
    struct Ring {
        z_mm: f64,
        /// Start point, then the end of every move.
        points: Vec<Point>,
        /// Signed area enclosed, arcs included; negative is clockwise.
        area_mm2: f64,
        bounds: Rect,
    }

    impl Ring {
        fn start(&self) -> Point {
            self.points[0]
        }

        fn width_mm(&self) -> f64 {
            self.bounds.width_mm()
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

    /// The signed area contribution of the cutting move `mv` from `from`,
    /// arcs included, and the extra extreme points of an arc.
    fn move_area_and_extremes(from: Point, mv: &Move) -> (f64, Vec<Point>) {
        let to = mv.to.xy();
        let mut area = from.x * to.y - to.x * from.y;
        let mut extremes = Vec::new();
        let arc = match mv.kind {
            MoveKind::ArcCcw { centre } => Some((centre, true)),
            MoveKind::ArcCw { centre } => Some((centre, false)),
            _ => None,
        };
        if let Some((c, ccw)) = arc {
            let r = from.distance_to(c);
            let theta = sweep_rad(from, to, c, ccw);
            area += r * r * (theta - theta.sin());
            extremes.extend(dc_geometry::Arc::from_ends(from, to, c, ccw).axis_extremes());
        }
        (area, extremes)
    }

    /// True when the path from move `start` onward returns to `target`
    /// before it returns to `other` (or never returns to `other`), without
    /// leaving the Z level or hitting a rapid.
    fn returns_to_first(moves: &[Move], start: usize, target: Point, other: Point) -> bool {
        let z = moves[start].to.z;
        for mv in &moves[start..] {
            if mv.is_rapid() || (mv.to.z - z).abs() > EPS_MM {
                return false;
            }
            let p = mv.to.xy();
            if p.distance_to(target) <= EPS_MM {
                return true;
            }
            if p.distance_to(other) <= EPS_MM {
                return false;
            }
        }
        false
    }

    /// Splits the toolpath into closed rings: runs of XY cutting moves at
    /// one Z that return to their start. A stay-down link between two rings
    /// is a cutting move too; it is told from a ring's first segment by
    /// whether the path returns to the link's end before its start.
    fn rings(toolpath: &Toolpath) -> Vec<Ring> {
        let moves = &toolpath.moves;
        let mut rings = Vec::new();
        let mut pos = match moves.first() {
            Some(m) => m.to,
            None => return rings,
        };
        let mut current: Option<Ring> = None;
        let mut i = 0;
        while i < moves.len() {
            let mv = moves[i];
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
            if mv.is_rapid() || !level || !xy {
                assert!(current.is_none(), "ring left open at move {i}: {current:?}");
                pos = mv.to;
                i += 1;
                continue;
            }
            let from = pos.xy();
            let ring = match current.as_mut() {
                Some(ring) => ring,
                None => {
                    // Either this move starts a ring at `from`, or it is a
                    // link to the start of the next ring.
                    let to = mv.to.xy();
                    if i + 1 < moves.len() && returns_to_first(moves, i + 1, to, from) {
                        pos = mv.to;
                        i += 1;
                        continue;
                    }
                    current.insert(Ring {
                        z_mm: mv.to.z,
                        points: vec![from],
                        area_mm2: 0.0,
                        bounds: Rect::from_point(from),
                    })
                }
            };
            let (area, extremes) = move_area_and_extremes(from, &mv);
            ring.area_mm2 += area;
            ring.points.push(mv.to.xy());
            ring.bounds = ring.bounds.including(mv.to.xy());
            for p in extremes {
                ring.bounds = ring.bounds.including(p);
            }
            if mv.to.xy().distance_to(ring.start()) <= EPS_MM {
                let mut done = current.take().unwrap();
                done.area_mm2 /= 2.0;
                rings.push(done);
            }
            pos = mv.to;
            i += 1;
        }
        assert!(
            current.is_none(),
            "toolpath ends inside a ring: {current:?}"
        );
        rings
    }

    fn rings_at(toolpath: &Toolpath, z_mm: f64) -> Vec<Ring> {
        rings(toolpath)
            .into_iter()
            .filter(|r| (r.z_mm - z_mm).abs() <= EPS_MM)
            .collect()
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code).collect()
    }

    fn assert_rect_eq(got: Rect, want: Rect, tol: f64, what: &str) {
        let close = (got.min.x - want.min.x).abs() <= tol
            && (got.min.y - want.min.y).abs() <= tol
            && (got.max.x - want.max.x).abs() <= tol
            && (got.max.y - want.max.y).abs() <= tol;
        assert!(close, "{what}: got {got:?}, want {want:?} within {tol}");
    }

    /// The region of one placed polyline.
    fn region_of(polylines: &[Polyline], placement: Transform) -> Region {
        let contours = polylines
            .iter()
            .map(|p| Contour::new(placement.apply_polyline(p).unwrap()).unwrap())
            .collect();
        nest(contours).unwrap().0.remove(0)
    }

    /// The direction rule itself (design step 8): climb is CCW on the
    /// pocket wall and CW round an island; conventional is the reverse.
    #[test]
    fn direction_rule() {
        use CutDirection::{Climb, Conventional};
        assert!(ring_is_ccw(RingWall::Pocket, Climb));
        assert!(!ring_is_ccw(RingWall::Pocket, Conventional));
        assert!(!ring_is_ccw(RingWall::Island, Climb));
        assert!(ring_is_ccw(RingWall::Island, Conventional));
    }

    /// Acceptance test 19: the 100 × 50 rect with the 6.35 mm tool at 40 %
    /// gives, per pass, concentric rings 2.54 mm apart whose outermost is
    /// the wall inset by the tool radius. The tool-centre rect is 93.65 ×
    /// 43.65; 8 insets of 2.54 fit in its 21.825 half-height, so 9 rings.
    #[test]
    fn offset_rings_are_concentric_and_spaced_by_stepover() {
        let p = rect_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            let mut rings = rings_at(&tp, z);
            assert_eq!(rings.len(), 9, "rings at Z {z}: {rings:?}");
            rings.sort_by(|a, b| a.area_mm2.abs().total_cmp(&b.area_mm2.abs()));
            let tool_centre = inset(wall(), RADIUS_MM);
            for (k, ring) in rings.iter().rev().enumerate() {
                let want = inset(tool_centre, k as f64 * STEPOVER_MM);
                assert_rect_eq(ring.bounds, want, RING_TOL, "ring");
                let centre_x = (ring.bounds.min.x + ring.bounds.max.x) / 2.0;
                let centre_y = (ring.bounds.min.y + ring.bounds.max.y) / 2.0;
                assert_approx_eq!(centre_x, 70.0, RING_TOL);
                assert_approx_eq!(centre_y, 45.0, RING_TOL);
            }
        }
    }

    /// Port of `testPocketOffsetCutsInnerFirst` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`, "offset cuts inner loops
    /// before wall-adjacent loops"): in cut order the first ring of a pass
    /// is the smallest and lies strictly inside the last, which reaches
    /// the wall inset; climb and conventional first rings wind opposite
    /// ways. Upstream's 20 × 20 pocket with a 4 mm tool at 50 % becomes
    /// the rect fixture; the assertions are the same.
    #[test]
    fn offset_cuts_inner_rings_before_the_wall() {
        let p = rect_project();
        let mut first_areas = Vec::new();
        for direction in [CutDirection::Conventional, CutDirection::Climb] {
            let (tp, warnings) = generate(&p, &pocket_op(&p, direction));
            assert!(warnings.is_empty(), "{warnings:?}");
            let rings = rings_at(&tp, PASS_Z_MM[0]);
            assert!(rings.len() >= 3, "{direction:?}: {}", rings.len());
            let first = &rings[0];
            let last = &rings[rings.len() - 1];
            assert!(
                first.area_mm2.abs() < last.area_mm2.abs(),
                "{direction:?}: first {first:?} is smaller than last {last:?}"
            );
            assert!(first.bounds.min.x > last.bounds.min.x, "{direction:?}");
            assert!(first.bounds.min.y > last.bounds.min.y, "{direction:?}");
            assert!(first.bounds.max.x < last.bounds.max.x, "{direction:?}");
            assert!(first.bounds.max.y < last.bounds.max.y, "{direction:?}");
            assert_rect_eq(last.bounds, inset(wall(), RADIUS_MM), RING_TOL, "wall ring");
            first_areas.push(first.area_mm2);
        }
        assert!(
            first_areas[0] * first_areas[1] < 0.0,
            "climb and conventional wind opposite ways: {first_areas:?}"
        );
    }

    /// `wall_pass`: `last` cuts the wall ring after the inner rings,
    /// `first` before them, and `none` not at all, so the pocket is
    /// undersize by one step-over.
    #[test]
    fn wall_pass_first_last_none() {
        let p = rect_project();
        let tool_centre = inset(wall(), RADIUS_MM);
        let is_wall = |r: &Ring| {
            (r.bounds.min.x - tool_centre.min.x).abs() <= RING_TOL
                && (r.bounds.max.y - tool_centre.max.y).abs() <= RING_TOL
        };
        for (wall_pass, position) in [(WallPass::Last, 8usize), (WallPass::First, 0)] {
            let mut op = pocket_op(&p, CutDirection::Climb);
            op.wall_pass = wall_pass;
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let rings = rings_at(&tp, z);
                assert_eq!(rings.len(), 9, "{wall_pass:?} at Z {z}");
                let walls: Vec<usize> = (0..rings.len()).filter(|&i| is_wall(&rings[i])).collect();
                assert_eq!(walls, [position], "{wall_pass:?} at Z {z}: {rings:?}");
            }
        }
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.wall_pass = WallPass::None;
        let (tp, _) = generate(&p, &op);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            assert_eq!(rings.len(), 8, "none at Z {z}");
            assert!(rings.iter().all(|r| !is_wall(r)), "no wall ring: {rings:?}");
            let outermost = rings
                .iter()
                .max_by(|a, b| a.area_mm2.abs().total_cmp(&b.area_mm2.abs()))
                .unwrap();
            assert_rect_eq(
                outermost.bounds,
                inset(tool_centre, STEPOVER_MM),
                RING_TOL,
                "outermost ring with no wall pass",
            );
        }
    }

    /// S9: the tool's footprint along every cutting move stays inside the
    /// rect. The footprint's area is the rect's less the four corner
    /// wedges a round tool cannot reach, `(4 − π) r²` in all.
    #[test]
    fn footprint_stays_inside_region() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let fp = footprint(&tp, RADIUS_MM);
        assert!(!fp.is_empty());
        let region = region_of(&[rect_100x50()], at_20_20());
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        assert!(
            area_mm2(&outside) <= 1e-3,
            "footprint outside the rect: {} mm²",
            area_mm2(&outside)
        );
        let corners_mm2 = (4.0 - std::f64::consts::PI) * RADIUS_MM * RADIUS_MM;
        assert_approx_eq!(area_mm2(&fp), 5000.0 - corners_mm2, 0.5);
    }

    /// S9, acceptance test 20: with the 20 mm circle island no cutting
    /// move's footprint enters the circle. Checked exactly on the tool
    /// centres (never nearer the island's centre than its radius plus the
    /// tool's) and on the area the footprint shares with the island.
    #[test]
    fn footprint_stays_outside_island() {
        let p = island_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let centre = Point::new(70.0, 45.0);
        let mut nearest = f64::MAX;
        for (i, mv) in tp.moves.iter().enumerate() {
            if !mv.is_rapid() {
                // A move's ends; a ring's arcs bulge away from the island.
                let d = mv.to.xy().distance_to(centre);
                assert!(
                    d >= 10.0 + RADIUS_MM - FLAT_TOL,
                    "move {i} brings the tool centre {d} mm from the island centre: {mv:?}"
                );
                nearest = nearest.min(d);
            }
        }
        assert!(
            nearest <= 10.0 + RADIUS_MM + RING_TOL,
            "the island ring wraps the island: nearest centre {nearest}"
        );
        let island = region_of(
            &[Polyline::circle(Point::new(50.0, 25.0), 10.0)],
            at_20_20(),
        );
        let fp = footprint(&tp, RADIUS_MM);
        let inside = intersection(&fp, std::slice::from_ref(&island), FLAT_TOL);
        // The flattened footprint and island each stray a tolerance from
        // the true curves; anything beyond that band is a real gouge.
        let band_mm2 = 2.0 * FLAT_TOL * TAU * 10.0;
        assert!(
            area_mm2(&inside) <= band_mm2,
            "footprint inside the island: {} mm²",
            area_mm2(&inside)
        );
    }

    /// Acceptance test 19, in the spirit of PureCutCNC
    /// `pocketClearance.test.ts` ("no stock left"): the union of the
    /// footprints covers the rect inset by the tool radius, the reachable
    /// area, within 0.05 mm (as area: 0.05 × its perimeter).
    #[test]
    fn offset_coverage_reaches_inset_region() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let fp = footprint(&tp, RADIUS_MM);
        let outer = Contour::new(at_20_20().apply_polyline(&rect_100x50()).unwrap()).unwrap();
        let reachable = nest(offset(&outer, -RADIUS_MM)).unwrap().0;
        assert_eq!(reachable.len(), 1);
        let uncut = difference(&reachable, &fp, FLAT_TOL);
        let perimeter_mm = reachable[0].outer().polyline().length_mm();
        assert!(
            area_mm2(&uncut) <= 0.05 * perimeter_mm,
            "{} mm² of the reachable area is not cut: {uncut:?}",
            area_mm2(&uncut)
        );
        // Coverage holds at every pass, not only in the union of passes.
        for z in PASS_Z_MM {
            let at_z = Toolpath {
                moves: tp
                    .moves
                    .iter()
                    .filter(|m| !m.is_rapid() && (m.to.z - z).abs() <= EPS_MM)
                    .copied()
                    .collect(),
            };
            let fp = footprint(&at_z, RADIUS_MM);
            let uncut = difference(&reachable, &fp, FLAT_TOL);
            assert!(
                area_mm2(&uncut) <= 0.05 * perimeter_mm,
                "at Z {z}: {} mm² uncut",
                area_mm2(&uncut)
            );
        }
    }

    /// The wall ring (93.65 mm wide) and the island ring (the 20 mm circle
    /// grown by the tool radius: 26.35 mm wide) of the island fixture at
    /// `z_mm`.
    fn wall_and_island_rings(tp: &Toolpath, z_mm: f64) -> (Ring, Ring) {
        let rings = rings_at(tp, z_mm);
        let wall = rings
            .iter()
            .find(|r| (r.width_mm() - (100.0 - DIAMETER_MM)).abs() <= RING_TOL)
            .unwrap_or_else(|| panic!("no wall ring: {rings:?}"))
            .clone();
        let island = rings
            .iter()
            .find(|r| (r.width_mm() - (20.0 + DIAMETER_MM)).abs() <= RING_TOL)
            .unwrap_or_else(|| panic!("no island ring: {rings:?}"))
            .clone();
        (wall, island)
    }

    /// Port of PureCutCNC `pocketFinishIslandDirection.test.ts`: for one
    /// `direction` the island ring winds opposite to the pocket wall, so
    /// the cutter meets the material on the same side on both. Upstream's
    /// table reads CCW wall / CW island for conventional in Y-down; here
    /// the senses are mirrored by the Y flip (see the direction tests), but
    /// "opposite" is what this test pins, and that survives the flip.
    #[test]
    fn island_wall_runs_opposite_to_pocket_wall() {
        let p = island_project();
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            let (tp, _) = generate(&p, &pocket_op(&p, direction));
            for z in PASS_Z_MM {
                let (wall, island) = wall_and_island_rings(&tp, z);
                assert!(
                    wall.area_mm2 * island.area_mm2 < 0.0,
                    "{direction:?} at Z {z}: wall {} island {}",
                    wall.area_mm2,
                    island.area_mm2
                );
            }
        }
    }

    /// Y-up re-derivation: climb on the pocket wall is counter-clockwise
    /// (phase 1's inside rule), and every interior ring follows it.
    #[test]
    fn climb_on_pocket_wall_is_counter_clockwise() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let rings = rings(&tp);
        assert!(!rings.is_empty());
        for ring in &rings {
            assert!(ring.area_mm2 > 0.0, "{ring:?}");
        }
    }

    /// Y-up re-derivation: climb round an island is clockwise, an outside
    /// wall, while the pocket's own rings stay counter-clockwise.
    #[test]
    fn climb_on_island_is_clockwise() {
        let p = island_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        for z in PASS_Z_MM {
            let (wall, island) = wall_and_island_rings(&tp, z);
            assert!(wall.area_mm2 > 0.0, "wall {wall:?}");
            assert!(island.area_mm2 < 0.0, "island {island:?}");
        }
    }

    /// Conventional is the reverse of climb on both walls.
    #[test]
    fn conventional_reverses_both() {
        let p = island_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Conventional));
        for z in PASS_Z_MM {
            let (wall, island) = wall_and_island_rings(&tp, z);
            assert!(wall.area_mm2 < 0.0, "wall {wall:?}");
            assert!(island.area_mm2 > 0.0, "island {island:?}");
        }
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Conventional));
        for ring in &rings(&tp) {
            assert!(ring.area_mm2 < 0.0, "{ring:?}");
        }
    }

    /// A 5 mm wide slot is narrower than the 6.35 mm tool: `PocketNoRoom`
    /// names it and it is not cut, while the rect beside it is.
    #[test]
    fn tool_wider_than_region_warns_and_skips() {
        let slot = Polyline::rect(Point::new(130.0, 0.0), 5.0, 40.0);
        let p = project(vec![rect_100x50(), slot], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert_eq!(codes(&warnings), vec![WarningCode::PocketNoRoom]);
        assert_eq!(warnings[0].param("instance"), Some("i1"));
        assert_eq!(warnings[0].param("profile"), Some("1"));
        assert_eq!(warnings[0].param("tool"), Some("6.35 mm upcut"));
        assert_eq!(warnings[0].param("op"), Some("o1"));
        for ring in &rings(&tp) {
            assert!(
                ring.bounds.max.x <= wall().max.x - RADIUS_MM + RING_TOL,
                "a ring in the slot: {ring:?}"
            );
        }
        assert_eq!(rings_at(&tp, PASS_Z_MM[0]).len(), 9, "the rect is cut");
    }

    /// A 10 mm bar island reaching to 1 mm of the top and bottom walls
    /// leaves no room for the tool past it, so the tool-centre region is
    /// two pieces, left and right of the bar, each with its own rings. The
    /// 1 mm gaps warn `PocketIslandTooClose`, and every move between the
    /// halves retracts to safe Z: no cutting move crosses the bar.
    #[test]
    fn island_splits_region_into_two_tool_centre_regions() {
        let bar = Polyline::rect(Point::new(45.0, 1.0), 10.0, 48.0);
        let p = project(vec![rect_100x50(), bar], &[("i1", at_20_20())]);
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let close: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code == WarningCode::PocketIslandTooClose)
            .collect();
        assert!(!close.is_empty(), "{warnings:?}");
        assert_eq!(close[0].param("island"), Some("1"));
        assert_eq!(close[0].param("other"), Some("wall"));
        assert_approx_eq!(
            close[0].param("gap_mm").unwrap().parse::<f64>().unwrap(),
            1.0,
            RING_TOL
        );
        assert!(
            warnings
                .iter()
                .all(|w| w.code == WarningCode::PocketIslandTooClose),
            "{warnings:?}"
        );
        // The bar spans X 65..75; rings are wholly left or right of it.
        let (bar_min_x, bar_max_x) = (65.0, 75.0);
        for z in PASS_Z_MM {
            let rings = rings_at(&tp, z);
            let left = rings
                .iter()
                .filter(|r| r.bounds.max.x <= bar_min_x - RADIUS_MM + RING_TOL);
            let right = rings
                .iter()
                .filter(|r| r.bounds.min.x >= bar_max_x + RADIUS_MM - RING_TOL);
            assert_eq!(left.count() + right.count(), rings.len(), "{rings:?}");
            assert!(
                rings.iter().any(|r| r.bounds.max.x < bar_min_x),
                "a left ring"
            );
            assert!(
                rings.iter().any(|r| r.bounds.min.x > bar_max_x),
                "a right ring"
            );
            // The widest ring of each half hugs the bar: 45 − 6.35 wide.
            let widest = rings.iter().map(Ring::width_mm).fold(0.0, f64::max);
            assert_approx_eq!(widest, 45.0 - DIAMETER_MM, RING_TOL);
        }
        let mut pos = tp.moves[0].to;
        for (i, mv) in tp.moves.iter().enumerate() {
            if !mv.is_rapid() {
                let (a, b) = (pos.xy().x, mv.to.xy().x);
                assert!(
                    (a <= bar_min_x && b <= bar_min_x) || (a >= bar_max_x && b >= bar_max_x),
                    "move {i} crosses the bar: {pos:?} -> {mv:?}"
                );
            }
            pos = mv.to;
        }
    }

    /// A 10 mm square inside a 30 mm circular island inside the rect is a
    /// pocket of its own (depth 2). Under full depth first every ring of
    /// it comes after every ring of the rect, which cuts the island's
    /// wall. The square's tool-centre square is 3.65 mm wide, less than
    /// two step-overs, so it is one ring per pass.
    #[test]
    fn pocket_inside_island_is_cut_after_island() {
        let p = project(
            vec![
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 15.0),
                Polyline::rect(Point::new(45.0, 20.0), 10.0, 10.0),
            ],
            &[("i1", at_20_20())],
        );
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let rings = rings(&tp);
        let island_centre = Point::new(70.0, 45.0);
        let in_island = |r: &Ring| r.bounds.min.distance_to(island_centre) < 15.0;
        let inner: Vec<usize> = (0..rings.len()).filter(|&i| in_island(&rings[i])).collect();
        let outer: Vec<usize> = (0..rings.len())
            .filter(|&i| !in_island(&rings[i]))
            .collect();
        assert_eq!(inner.len(), PASS_Z_MM.len(), "one ring per pass: {rings:?}");
        assert!(outer.len() >= 2 * PASS_Z_MM.len(), "{rings:?}");
        assert!(
            outer.iter().max() < inner.iter().min(),
            "outer pocket first: outer {outer:?}, inner {inner:?}"
        );
        // The inner pocket's wall ring is 10 − 6.35 wide.
        let widest = inner
            .iter()
            .map(|&i| rings[i].width_mm())
            .fold(0.0, f64::max);
        assert_approx_eq!(widest, 10.0 - DIAMETER_MM, RING_TOL);
    }

    /// Step-down and step-over come from the operation, else the tool;
    /// neither set is an error, never a default.
    #[test]
    fn step_down_from_tool_when_operation_unset() {
        let p = rect_project();
        let op = pocket_op(&p, CutDirection::Climb);
        let (tp, _) = generate(&p, &op);
        let mut zs: Vec<f64> = rings(&tp).iter().map(|r| r.z_mm).collect();
        zs.dedup_by(|a, b| (*a - *b).abs() <= EPS_MM);
        assert_eq!(zs.len(), 2, "{zs:?}");
        assert_approx_eq!(zs[0], 4.0, TOL);
        assert_approx_eq!(zs[1], 2.0, TOL);

        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_down_mm = Some(4.0);
        let (tp, _) = generate(&p, &op);
        let rings_z: Vec<f64> = rings(&tp).iter().map(|r| r.z_mm).collect();
        assert!(
            rings_z.iter().all(|z| (z - 2.0).abs() <= TOL),
            "{rings_z:?}"
        );

        let mut p = rect_project();
        p.tools[0].step_down_mm = None;
        let op = pocket_op(&p, CutDirection::Climb);
        assert_eq!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::MissingStepDown {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
            }
        );

        // Step-over: the tool's 0.4 when the operation sets none; the
        // operation's 0.5 (3.175 mm) overrides it; neither is an error.
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_over_fraction = None;
        let (tp, _) = generate(&p, &op);
        assert_eq!(rings_at(&tp, PASS_Z_MM[0]).len(), 9);
        op.step_over_fraction = Some(0.5);
        let (tp, _) = generate(&p, &op);
        let mut rings = rings_at(&tp, PASS_Z_MM[0]);
        rings.sort_by(|a, b| b.area_mm2.abs().total_cmp(&a.area_mm2.abs()));
        assert_approx_eq!(
            rings[0].bounds.min.x + 3.175,
            rings[1].bounds.min.x,
            RING_TOL
        );
        let mut p = rect_project();
        p.tools[0].step_over_fraction = None;
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.step_over_fraction = None;
        assert_eq!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::MissingStepOver {
                op: OperationId::new("o1"),
                tool: "6.35 mm upcut".into(),
            }
        );
        op.step_over_fraction = Some(1.5);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::InvalidStepOver { .. }
        ));
    }

    /// S1: a depth beyond the stock plus the allowance is an error.
    #[test]
    fn depth_below_bottom_is_error() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.depth_mm = Some(THICKNESS_MM + 0.5);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::BelowMaterialBottom { .. }
        ));
        op.through_cut_allowance_mm = 0.5;
        let (tp, _) = generate(&p, &op);
        let lowest = tp.moves.iter().map(|m| m.to.z).fold(f64::MAX, f64::min);
        assert_approx_eq!(lowest, -0.5, TOL);
    }

    /// Port of `testPocketLevelTransitionsPlungeVerticallyFromSafeZ`
    /// (PureCutCNC `src/engine/toolpaths/toolpaths.test.ts`, "level
    /// transitions rapid at safe Z before plunging"): the first cut of the
    /// second pass is preceded by a vertical plunge at the plunge rate that
    /// ends where that cut starts, and the plunge comes down from a rapid
    /// at safe Z. Upstream's `-4` is the second pass Z of a 4 mm cut in 2
    /// mm steps; here that is Z 2 above the bottom of 6 mm stock.
    #[test]
    fn levels_rapid_at_safe_z_before_plunging() {
        let p = rect_project();
        let (tp, warnings) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        assert!(warnings.is_empty(), "{warnings:?}");
        let m = &tp.moves;
        let first_deep = m
            .iter()
            .enumerate()
            .position(|(i, mv)| {
                !mv.is_rapid()
                    && (mv.to.z - PASS_Z_MM[1]).abs() <= EPS_MM
                    && i > 0
                    && m[i - 1].to.xy().distance_to(mv.to.xy()) > EPS_MM
            })
            .expect("a cut at the second pass");
        let plunge = m[first_deep - 1];
        let before = m[first_deep - 2];
        assert_eq!(plunge.kind, MoveKind::Linear, "{plunge:?}");
        assert_approx_eq!(plunge.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
        assert!(
            before.to.xy().distance_to(plunge.to.xy()) <= EPS_MM,
            "the plunge is vertical: {before:?} -> {plunge:?}"
        );
        assert_approx_eq!(plunge.to.z, PASS_Z_MM[1], TOL);
        assert!(before.is_rapid(), "{before:?}");
        assert_approx_eq!(before.to.z, SAFE_Z_MM, TOL);
        // And the ring continues from the plunge at the feed rate.
        assert_approx_eq!(m[first_deep].feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
    }

    /// What this slice refuses or falls back on: allowance and rest are
    /// typed errors naming the feature; a ramp entry and corner relief
    /// generate with a warning each and a plain plunge.
    #[test]
    fn allowance_and_rest_are_not_supported_yet() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.allowance_mm = 0.5;
        let err = generate_pocket(&p, &op).unwrap_err();
        assert!(matches!(
            &err,
            CamError::NotSupportedYet { feature, .. } if feature.contains("allowance")
        ));
        assert!(err.to_string().contains("not supported by this build yet"));

        let mut op = pocket_op(&p, CutDirection::Climb);
        op.rest_of = Some(OperationId::new("o0"));
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::NotSupportedYet { ref feature, .. } if feature.contains("rest")
        ));

        let mut op = pocket_op(&p, CutDirection::Climb);
        op.entry = Entry::Ramp { angle_deg: 15.0 };
        op.corner_relief = CornerRelief::Dogbone;
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::EntryFallback, WarningCode::ReliefSkipped]
        );
        assert_eq!(warnings[0].param("requested"), Some("ramp"));
        assert_eq!(warnings[0].param("used"), Some("plunge"));
        assert_eq!(warnings[0].param("reason"), Some("not_supported_yet"));
        assert_eq!(warnings[1].param("requested"), Some("dogbone"));
        assert_eq!(rings_at(&tp, PASS_Z_MM[0]).len(), 9);
    }

    #[test]
    fn output_is_deterministic() {
        let p = island_project();
        let op = pocket_op(&p, CutDirection::Climb);
        assert_eq!(
            generate_pocket(&p, &op).unwrap(),
            generate_pocket(&p, &op).unwrap()
        );
    }

    /// Every ring starts at the vertex nearest the previous ring's end
    /// (`rotateContourToNearestEntry`): the stay-down link between two
    /// rings of one pass is never longer than one step-over and a bit.
    #[test]
    fn rings_start_nearest_the_previous_ring() {
        let p = rect_project();
        let (tp, _) = generate(&p, &pocket_op(&p, CutDirection::Climb));
        let rings = rings_at(&tp, PASS_Z_MM[0]);
        for pair in rings.windows(2) {
            let link_mm = pair[0].start().distance_to(pair[1].start());
            assert!(
                link_mm <= STEPOVER_MM * std::f64::consts::SQRT_2 + RING_TOL,
                "link of {link_mm} mm between {:?} and {:?}",
                pair[0].start(),
                pair[1].start()
            );
        }
    }

    #[test]
    fn unknown_tool_and_other_cutters_are_errors() {
        let p = rect_project();
        let mut op = pocket_op(&p, CutDirection::Climb);
        op.tool = ToolId::new("nope");
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::UnknownTool { .. }
        ));
        let mut p = rect_project();
        p.tools[0].cutter = dc_project::ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let op = pocket_op(&p, CutDirection::Climb);
        assert!(matches!(
            generate_pocket(&p, &op).unwrap_err(),
            CamError::CutterNotFlat { .. }
        ));
        let p = rect_project();
        let op = pocket_op(&p, CutDirection::Climb);
        let err = generate_pocket(
            &{
                let mut p = p.clone();
                p.tools[0].feed_mm_min = None;
                p
            },
            &op,
        )
        .unwrap_err();
        assert!(matches!(err, CamError::MissingFeed { .. }), "{err:?}");
    }

    // ---- Raster strategy (design step 7, acceptance 21) ----

    /// The rect's tool-centre rect.
    fn tool_centre() -> Rect {
        inset(wall(), RADIUS_MM)
    }

    /// The longest a stay-down link may be: one diameter plus a step-over.
    const LINK_MAX_MM: f64 = DIAMETER_MM + STEPOVER_MM;

    /// A raster Pocket at `angle_deg` with `wall_pass`, otherwise as
    /// [`pocket_op`].
    fn raster_op(
        project: &Project,
        direction: CutDirection,
        angle_deg: f64,
        wall_pass: WallPass,
    ) -> PocketOp {
        let mut op = pocket_op(project, direction);
        op.strategy = ClearanceStrategy::Raster { angle_deg };
        op.wall_pass = wall_pass;
        op
    }

    /// One XY cutting move at one Z: where it starts and ends, and its
    /// index in the toolpath.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Cut {
        index: usize,
        z_mm: f64,
        from: Point,
        to: Point,
    }

    impl Cut {
        fn length_mm(&self) -> f64 {
            self.from.distance_to(self.to)
        }

        fn unit(&self) -> dc_geometry::Vec2 {
            (self.to - self.from).normalized().unwrap()
        }

        fn mid(&self) -> Point {
            Point::new(
                (self.from.x + self.to.x) / 2.0,
                (self.from.y + self.to.y) / 2.0,
            )
        }
    }

    /// Every non-rapid move that travels in XY at a constant Z, in order.
    fn cuts(tp: &Toolpath) -> Vec<Cut> {
        let mut out = Vec::new();
        let mut pos = tp.moves[0].to;
        for (index, mv) in tp.moves.iter().enumerate() {
            let level = (mv.to.z - pos.z).abs() <= EPS_MM;
            if !mv.is_rapid() && level && pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                out.push(Cut {
                    index,
                    z_mm: mv.to.z,
                    from: pos.xy(),
                    to: mv.to.xy(),
                });
            }
            pos = mv.to;
        }
        out
    }

    /// The cuts at `z_mm` longer than any stay-down link may be: the
    /// raster lines, and the wall ring's sides when there is a wall pass.
    fn long_cuts_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        cuts(tp)
            .into_iter()
            .filter(|c| (c.z_mm - z_mm).abs() <= EPS_MM && c.length_mm() > LINK_MAX_MM + RING_TOL)
            .collect()
    }

    /// True when both ends of `cut` lie on one side of `rect`.
    fn on_side(cut: &Cut, rect: Rect) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= RING_TOL;
        (near(cut.from.x, rect.min.x) && near(cut.to.x, rect.min.x))
            || (near(cut.from.x, rect.max.x) && near(cut.to.x, rect.max.x))
            || (near(cut.from.y, rect.min.y) && near(cut.to.y, rect.min.y))
            || (near(cut.from.y, rect.max.y) && near(cut.to.y, rect.max.y))
    }

    /// The raster lines of the rect fixture at `z_mm`, in cut order: the
    /// long cuts that are not sides of the wall ring.
    fn raster_lines_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        long_cuts_at(tp, z_mm)
            .into_iter()
            .filter(|c| !on_side(c, tool_centre()))
            .collect()
    }

    /// The wall ring's sides on the rect fixture at `z_mm`, in cut order.
    fn wall_sides_at(tp: &Toolpath, z_mm: f64) -> Vec<Cut> {
        long_cuts_at(tp, z_mm)
            .into_iter()
            .filter(|c| on_side(c, tool_centre()))
            .collect()
    }

    /// The distance from `p` to the segment `a → b`.
    fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
        let ab = b - a;
        let len2 = ab.dot(ab);
        let t = if len2 <= 0.0 {
            0.0
        } else {
            ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
        };
        p.distance_to(a + ab * t)
    }

    /// Asserts consecutive values of `offsets` (sorted) are one step-over
    /// apart.
    fn assert_spaced_by_stepover(mut offsets: Vec<f64>, what: &str) {
        offsets.sort_by(f64::total_cmp);
        for pair in offsets.windows(2) {
            assert_approx_eq!(pair[1] - pair[0], STEPOVER_MM, RING_TOL);
        }
        assert!(offsets.len() >= 2, "{what}: {offsets:?}");
    }

    /// Design step 7 at 0°: every clearance line is parallel to X, the
    /// lines are one step-over apart, the first half a step-over above the
    /// bottom of the tool-centre rect, and each spans the rect. The
    /// tool-centre rect is 43.65 high: lines at 1.27 + 2.54 k for k = 0 to
    /// 16 lie below 43.65 − 1.27, so 17 lines.
    #[test]
    fn raster_lines_at_zero_degrees_are_horizontal_and_spaced() {
        let p = rect_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let tc = tool_centre();
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            assert_eq!(lines.len(), 17, "lines at Z {z}: {lines:?}");
            for line in &lines {
                assert_approx_eq!(line.from.y, line.to.y, TOL);
                assert_approx_eq!(line.from.x.min(line.to.x), tc.min.x, RING_TOL);
                assert_approx_eq!(line.from.x.max(line.to.x), tc.max.x, RING_TOL);
            }
            let ys: Vec<f64> = lines.iter().map(|l| l.from.y).collect();
            let lowest = ys.iter().copied().fold(f64::MAX, f64::min);
            assert_approx_eq!(lowest, tc.min.y + STEPOVER_MM / 2.0, RING_TOL);
            assert_spaced_by_stepover(ys, "0°");
        }
    }

    /// Design step 7 at 45°, Y-up: every line runs along (1, 1), up and to
    /// the right as drawn (PureCutCNC's −45°), the lines one step-over
    /// apart across their direction. 135° runs along (−1, 1).
    #[test]
    fn raster_lines_at_45_degrees() {
        let p = rect_project();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        for (angle_deg, along) in [
            (45.0, Point::new(half, half)),
            (135.0, Point::new(-half, half)),
        ] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            let along = along - Point::new(0.0, 0.0);
            let across = along.perp();
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert!(lines.len() > 20, "{angle_deg}° at Z {z}: {lines:?}");
                for line in &lines {
                    let d = line.unit();
                    assert_approx_eq!(d.cross(along), 0.0, 1e-6);
                }
                let offsets = lines
                    .iter()
                    .map(|l| (l.from - Point::new(0.0, 0.0)).dot(across))
                    .collect();
                assert_spaced_by_stepover(offsets, "45°");
            }
        }
    }

    /// Design step 7: consecutive lines in cut order run opposite ways.
    #[test]
    fn raster_lines_alternate_direction() {
        let p = rect_project();
        for angle_deg in [0.0, 45.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert!(lines.len() > 2);
                for pair in lines.windows(2) {
                    assert!(
                        pair[0].unit().dot(pair[1].unit()) < -1.0 + 1e-6,
                        "{angle_deg}°: {:?} then {:?}",
                        pair[0],
                        pair[1]
                    );
                }
            }
        }
    }

    /// Asserts that, at `z_mm`, everything from the first raster line to
    /// the last is cut at that Z with no rapid, and every link between
    /// lines is a straight feed move at most one diameter plus a step-over
    /// long with both ends inside the tool-centre rect.
    fn assert_lines_linked_without_retract(tp: &Toolpath, lines: &[Cut], z_mm: f64) {
        let (first, last) = (lines[0].index, lines[lines.len() - 1].index);
        for (i, mv) in tp.moves.iter().enumerate().take(last + 1).skip(first) {
            assert!(!mv.is_rapid(), "move {i} retracts between lines: {mv:?}");
            assert_approx_eq!(mv.to.z, z_mm, TOL);
        }
        let tc = tool_centre();
        for pair in lines.windows(2) {
            let links: Vec<Cut> = cuts(tp)
                .into_iter()
                .filter(|c| c.index > pair[0].index && c.index < pair[1].index)
                .collect();
            for link in links {
                assert!(link.length_mm() <= LINK_MAX_MM + EPS_MM, "{link:?}");
                assert_eq!(tp.moves[link.index].kind, MoveKind::Linear);
                assert_approx_eq!(
                    tp.moves[link.index].feed_mm_min.unwrap(),
                    FEED_MM_MIN,
                    EPS_MM
                );
                for p in [link.from, link.to] {
                    assert!(
                        p.x >= tc.min.x - RING_TOL
                            && p.x <= tc.max.x + RING_TOL
                            && p.y >= tc.min.y - RING_TOL
                            && p.y <= tc.max.y + RING_TOL,
                        "link leaves the tool-centre rect: {link:?}"
                    );
                }
            }
        }
    }

    /// Design step 7: on the rect, the link from one line to the next is a
    /// short cut inside the tool-centre region, never a retract.
    #[test]
    fn raster_link_stays_down_inside_region() {
        let p = rect_project();
        for angle_deg in [0.0, 45.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::None);
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let lines = long_cuts_at(&tp, z);
                assert_lines_linked_without_retract(&tp, &lines, z);
            }
        }
    }

    /// The tool centres along `tp` never come nearer the island fixture's
    /// circle than its radius plus the tool's, less the flattening: along
    /// every straight cut, and at the ends of every arc (the island ring's
    /// arcs bulge away from the island).
    fn assert_clear_of_island(tp: &Toolpath) {
        let centre = Point::new(70.0, 45.0);
        for cut in cuts(tp) {
            let d = if tp.moves[cut.index].kind == MoveKind::Linear {
                segment_distance(centre, cut.from, cut.to)
            } else {
                centre.distance_to(cut.from).min(centre.distance_to(cut.to))
            };
            assert!(
                d >= 10.0 + RADIUS_MM - FLAT_TOL,
                "{cut:?} brings the tool centre {d} mm from the island centre"
            );
        }
    }

    /// Design step 7: with the 20 mm circle island at 0°, the lines through
    /// it are split, and the move between the two halves of a line lifts
    /// to safe Z instead of cutting across the island.
    #[test]
    fn raster_retracts_across_island() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_clear_of_island(&tp);
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            let (first, last) = (lines[0].index, lines[lines.len() - 1].index);
            let retracts = tp.moves[first..=last]
                .iter()
                .filter(|m| m.is_rapid() && (m.to.z - SAFE_Z_MM).abs() <= TOL)
                .count();
            // Every line through the island's grown circle lifts once
            // between its halves; at least the lines near Y 45 do.
            assert!(retracts >= 2, "retracts at Z {z}: {retracts}");
            for pair in lines.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let same_line = (a.from.y - b.from.y).abs() <= TOL;
                if same_line {
                    let between = &tp.moves[a.index + 1..b.index];
                    assert!(
                        between
                            .iter()
                            .any(|m| m.is_rapid() && (m.to.z - SAFE_Z_MM).abs() <= TOL),
                        "no retract between {a:?} and {b:?}"
                    );
                }
            }
        }
    }

    /// Design step 7: a line through the island's grown circle (radius
    /// 13.175 about (70, 45)) is two segments, ending and starting on that
    /// circle; a line clear of it is one segment across the rect.
    #[test]
    fn raster_segments_split_around_island() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::None);
        let (tp, _) = generate(&p, &op);
        let grown = 10.0 + RADIUS_MM;
        let tc = tool_centre();
        let mut split = 0;
        for z in PASS_Z_MM {
            let lines = long_cuts_at(&tp, z);
            let mut ys: Vec<f64> = lines.iter().map(|l| l.from.y).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup_by(|a, b| (*a - *b).abs() <= TOL);
            assert_eq!(ys.len(), 17, "{ys:?}");
            for y in ys {
                let mut on_line: Vec<(f64, f64)> = lines
                    .iter()
                    .filter(|l| (l.from.y - y).abs() <= TOL)
                    .map(|l| (l.from.x.min(l.to.x), l.from.x.max(l.to.x)))
                    .collect();
                on_line.sort_by(|a, b| a.0.total_cmp(&b.0));
                let dy = (y - 45.0).abs();
                if dy < grown - RING_TOL {
                    assert_eq!(on_line.len(), 2, "Y {y}: {on_line:?}");
                    assert_approx_eq!(on_line[0].0, tc.min.x, RING_TOL);
                    assert_approx_eq!(on_line[1].1, tc.max.x, RING_TOL);
                    // The inner ends lie on the grown circle, flattened:
                    // on a chord at most the tolerance inside it.
                    for x in [on_line[0].1, on_line[1].0] {
                        let r = Point::new(x, y).distance_to(Point::new(70.0, 45.0));
                        assert!(
                            r >= grown - FLAT_TOL - EPS_MM && r <= grown + EPS_MM,
                            "Y {y}: end at X {x} is {r} mm from the island centre"
                        );
                    }
                    assert!(on_line[0].1 < 70.0 && on_line[1].0 > 70.0);
                    split += 1;
                } else {
                    assert_eq!(on_line.len(), 1, "Y {y}: {on_line:?}");
                }
            }
        }
        assert!(split >= 2 * PASS_Z_MM.len(), "{split}");
    }

    /// Design step 8: raster lines are the same for climb and conventional;
    /// the wall ring runs counter-clockwise for climb and clockwise for
    /// conventional (twice its signed area, from its four sides).
    #[test]
    fn raster_ignores_direction_wall_pass_honours_it() {
        let p = rect_project();
        let climb = generate(
            &p,
            &raster_op(&p, CutDirection::Climb, 30.0, WallPass::Last),
        )
        .0;
        let conventional = generate(
            &p,
            &raster_op(&p, CutDirection::Conventional, 30.0, WallPass::Last),
        )
        .0;
        for z in PASS_Z_MM {
            let ends = |tp: &Toolpath| -> Vec<(Point, Point)> {
                raster_lines_at(tp, z)
                    .iter()
                    .map(|l| (l.from, l.to))
                    .collect()
            };
            let lines = ends(&climb);
            assert!(lines.len() > 10, "{lines:?}");
            assert_eq!(lines, ends(&conventional), "lines at Z {z}");
            let twice_area = |tp: &Toolpath| -> f64 {
                let sides = wall_sides_at(tp, z);
                assert_eq!(sides.len(), 4, "{sides:?}");
                sides
                    .iter()
                    .map(|s| (s.from - Point::new(0.0, 0.0)).cross(s.to - Point::new(0.0, 0.0)))
                    .sum()
            };
            assert!(twice_area(&climb) > 0.0, "climb wall is CCW");
            assert!(twice_area(&conventional) < 0.0, "conventional wall is CW");
        }
    }

    /// S9 on the raster: the footprint stays inside the rect and out of
    /// the island, at an angle that meets the island's curve everywhere.
    #[test]
    fn raster_footprint_stays_inside_region() {
        let p = island_project();
        let op = raster_op(&p, CutDirection::Climb, 45.0, WallPass::Last);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_clear_of_island(&tp);
        let fp = footprint(&tp, RADIUS_MM);
        let region = region_of(
            &[
                rect_100x50(),
                Polyline::circle(Point::new(50.0, 25.0), 10.0),
            ],
            at_20_20(),
        );
        let outside = difference(&fp, std::slice::from_ref(&region), FLAT_TOL);
        let boundary_mm = region.outer().polyline().length_mm() + TAU * 10.0;
        assert!(
            area_mm2(&outside) <= 2.0 * FLAT_TOL * boundary_mm,
            "footprint outside the material: {} mm²",
            area_mm2(&outside)
        );
    }

    /// Acceptance test 21, as the offset coverage test: at every pass the
    /// footprints cover the rect inset by the tool radius within 0.05 mm.
    #[test]
    fn raster_coverage_reaches_inset_region() {
        let p = rect_project();
        let outer = Contour::new(at_20_20().apply_polyline(&rect_100x50()).unwrap()).unwrap();
        let reachable = nest(offset(&outer, -RADIUS_MM)).unwrap().0;
        let perimeter_mm = reachable[0].outer().polyline().length_mm();
        for angle_deg in [0.0, 45.0, 100.0] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
            let (tp, _) = generate(&p, &op);
            for z in PASS_Z_MM {
                let at_z = Toolpath {
                    moves: tp
                        .moves
                        .iter()
                        .filter(|m| !m.is_rapid() && (m.to.z - z).abs() <= EPS_MM)
                        .copied()
                        .collect(),
                };
                let fp = footprint(&at_z, RADIUS_MM);
                let uncut = difference(&reachable, &fp, FLAT_TOL);
                assert!(
                    area_mm2(&uncut) <= 0.05 * perimeter_mm,
                    "{angle_deg}° at Z {z}: {} mm² uncut",
                    area_mm2(&uncut)
                );
            }
        }
    }

    /// Acceptance test 21: the rect rastered at 0° has every clearance
    /// line parallel to X, 2.54 mm apart; at 45° every line is at 45°.
    /// Consecutive lines alternate and are joined by a cut inside the
    /// region, not a retract. The wall pass is last by default and first
    /// with `wall_pass: first`.
    #[test]
    fn raster_acceptance_21() {
        let p = rect_project();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let op = raster_op(&p, CutDirection::Climb, 0.0, WallPass::Last);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        for z in PASS_Z_MM {
            let lines = raster_lines_at(&tp, z);
            assert!(lines.iter().all(|l| (l.from.y - l.to.y).abs() <= TOL));
            assert_spaced_by_stepover(lines.iter().map(|l| l.from.y).collect(), "0°");
        }
        for (wall_pass, wall_last) in [(WallPass::Last, true), (WallPass::First, false)] {
            for angle_deg in [0.0, 45.0] {
                let op = raster_op(&p, CutDirection::Climb, angle_deg, wall_pass);
                let (tp, _) = generate(&p, &op);
                for z in PASS_Z_MM {
                    let lines = raster_lines_at(&tp, z);
                    assert!(lines.len() > 10, "{lines:?}");
                    if angle_deg > 0.0 {
                        for line in &lines {
                            let d = line.unit();
                            assert_approx_eq!(d.x.abs(), half, 1e-6);
                            assert_approx_eq!(d.y.abs(), half, 1e-6);
                            assert!(d.x * d.y > 0.0, "{line:?} is at 45°");
                        }
                    }
                    for pair in lines.windows(2) {
                        assert!(pair[0].unit().dot(pair[1].unit()) < 0.0);
                    }
                    assert_lines_linked_without_retract(&tp, &lines, z);
                    let sides = wall_sides_at(&tp, z);
                    assert_eq!(sides.len(), 4, "{wall_pass:?} {angle_deg}°: {sides:?}");
                    let (first_line, last_line) = (lines[0].index, lines[lines.len() - 1].index);
                    if wall_last {
                        assert!(sides.iter().all(|s| s.index > last_line), "{wall_pass:?}");
                    } else {
                        assert!(sides.iter().all(|s| s.index < first_line), "{wall_pass:?}");
                    }
                    // Lines lie strictly inside the tool-centre rect.
                    let tc = tool_centre();
                    for line in &lines {
                        let m = line.mid();
                        assert!(
                            m.x > tc.min.x && m.x < tc.max.x && m.y > tc.min.y && m.y < tc.max.y
                        );
                    }
                }
            }
        }
    }

    /// The raster angle is degrees from +X in [0, 180); outside it the
    /// generator refuses rather than guessing, as validation does.
    #[test]
    fn raster_angle_outside_range_is_error() {
        let p = rect_project();
        for angle_deg in [-1.0, 180.0, 270.0, f64::NAN] {
            let op = raster_op(&p, CutDirection::Climb, angle_deg, WallPass::Last);
            let err = generate_pocket(&p, &op).unwrap_err();
            assert!(
                matches!(err, CamError::InvalidRasterAngle { ref op, .. } if op.as_str() == "o1"),
                "{angle_deg}: {err:?}"
            );
            assert!(
                err.to_string().contains("raster angle"),
                "{angle_deg}: {err}"
            );
        }
    }
}
