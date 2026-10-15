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

use crate::error::{CamError, FeedField, ShapeRef};
use crate::passes::pass_z_mm;
use crate::tabs::{LapRates, PathTabs, Tabs, cut_range_warnings, emit_lap, overlap_warning};
use crate::toolpath::{Move, Toolpath};
use crate::warning::{Warning, WarningCode};
use dc_geometry::{Contour, GeometryError, GeometryWarning, Point3, Polyline, nest, offset};
use dc_project::{CutDirection, ProfileOp, Project, Side, Tool};

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
fn rates_of(tool: &Tool) -> Result<LapRates, CamError> {
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
/// oriented per [`traversal_is_ccw`]. Within each part its holes are cut
/// before its outer contour, so the part is freed last. Each tool-centre
/// contour is cut at every depth pass before the next: one rapid to its
/// start at safe Z, then per pass a straight descent at the plunge rate and
/// one lap at the feed rate ending at the start, then a rapid retract to
/// safe Z. Arc segments stay arcs.
///
/// With `op.tabs`, every pass below the tab top rises over each tab span
/// (`width + diameter` along the tool-centre contour); overlapping spans
/// merge with [`WarningCode::TabsOverlap`], and a tab top no pass reaches,
/// or one above the stock top, gives [`WarningCode::TabsOutsideCutRange`].
///
/// Open paths, shapes the tool cannot fit, and duplicate shapes are skipped
/// with a [`Warning`]. Fails without a toolpath when a feed, plunge rate, or
/// step-down is missing, when the depth goes below the stock bottom plus
/// allowance, when targets cross, or when an id does not resolve.
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
    let step_down_mm =
        op.step_down_mm
            .or(tool.step_down_mm)
            .ok_or_else(|| CamError::MissingStepDown {
                op: op.id.clone(),
                tool: tool.name.clone(),
            })?;
    let thickness_mm = project.stock.thickness_mm;
    let pass_z = pass_z_mm(
        &op.id,
        thickness_mm,
        op.depth_mm,
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
    let shapes = resolve_targets(project, op, &mut warnings)?;
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
    let mut paths: Vec<Polyline> = Vec::new();
    for region in &regions {
        // Holes first, so a through-cut part is freed only by its last contour.
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
            if centres.is_empty() {
                if let Some((shape, _)) = shapes.iter().find(|(_, c)| same_contour(c, contour)) {
                    warnings.push(
                        shape_warning(WarningCode::ToolTooLarge, shape)
                            .with("tool", &tool.name)
                            .with("diameter_mm", tool.diameter_mm),
                    );
                }
                continue;
            }
            let ccw = traversal_is_ccw(wall, op.direction);
            paths.extend(centres.into_iter().map(|c| {
                if c.is_ccw() == ccw {
                    c.into_polyline()
                } else {
                    c.reversed().into_polyline()
                }
            }));
        }
    }

    let mut toolpath = Toolpath::default();
    for (index, path) in paths.iter().enumerate() {
        let layout = tabs.as_ref().map(|tabs| {
            let length_mm = path.length_mm();
            let (layout, overlapped) = tabs.layout(length_mm);
            if overlapped {
                warnings.push(overlap_warning(&op.id, index, length_mm));
            }
            (tabs, layout)
        });
        let tabs = layout.as_ref().map(|(tabs, layout)| (*tabs, layout));
        emit_contour(&mut toolpath.moves, path, &pass_z, safe_z_mm, rates, tabs);
    }
    Ok((toolpath, warnings))
}

/// Places every profile of every target instance, in target then profile
/// order. Open paths are skipped with a warning.
fn resolve_targets(
    project: &Project,
    op: &ProfileOp,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Contour)>, CamError> {
    let mut shapes = Vec::new();
    for instance_id in &op.targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op.id.clone(),
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
fn crossing_error(error: GeometryError, shapes: &[(ShapeRef, Contour)]) -> CamError {
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
fn shape_warning(code: WarningCode, shape: &ShapeRef) -> Warning {
    Warning::new(code)
        .with("instance", &shape.instance)
        .with("instance_name", &shape.instance_name)
        .with("profile", shape.profile)
}

/// True when `nested` is `input` as `nest` returns it: unchanged, or
/// reversed to fix its winding. Both are exact copies of the same vertices,
/// so exact equality is the right test.
fn same_contour(input: &Contour, nested: &Contour) -> bool {
    input == nested || &input.reversed() == nested
}

/// Appends the moves that cut `path` at every pass Z (steps 6 and 7): a
/// rapid to its start at safe Z, then per pass a vertical descent at the
/// plunge rate and one lap at the feed rate back to the start, rising over
/// `tabs` on passes below the tab top, then a vertical rapid retract to safe
/// Z. Between passes the tool is already at the start, so it descends
/// straight to the next pass without retracting.
fn emit_contour(
    moves: &mut Vec<Move>,
    path: &Polyline,
    pass_z: &[f64],
    safe_z_mm: f64,
    rates: LapRates,
    tabs: Option<(&Tabs, &PathTabs)>,
) {
    let Some(start) = path.vertices().first().map(|v| v.point()) else {
        return;
    };
    moves.push(Move::rapid(Point3::new(start.x, start.y, safe_z_mm)));
    for &z_mm in pass_z {
        emit_lap(moves, path, z_mm, tabs, rates);
    }
    moves.push(Move::rapid(Point3::new(start.x, start.y, safe_z_mm)));
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
        DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, OperationId, Side, Stock,
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
            thickness_mm: THICKNESS_MM,
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
                z_top_mm: THICKNESS_MM,
                z_bottom_mm: 0.0,
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
            depth_mm: THICKNESS_MM,
            step_down_mm: None,
            direction,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: None,
        }
    }

    /// Generates, and checks the result with [`assert_safe`].
    fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_profile(project, op).unwrap();
        assert_safe(&toolpath, project, op);
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
        op.depth_mm = THICKNESS_MM + 0.5;
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
}
