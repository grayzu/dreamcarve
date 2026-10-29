// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Questions about a whole project's geometry that validation and the
//! generators must answer the same way (`docs/specs/phase-3/design.md`,
//! "Algorithms → Start depth"). They live here, not in `dc-cam`, because
//! validation may not depend on `dc-cam` (`docs/rust-guidelines.md`,
//! "Crate boundaries"); `dc-cam` calls them.

use crate::ids::InstanceId;
use crate::model::{Operation, Project, ToolKind, VCarveOp};
use crate::validate::MAX_FLATTEN_TOLERANCE_MM;
use dc_geometry::{
    Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Region, area_mm2, as_circle, difference,
    floor_regions, nest, point_segment_distance_mm,
};

/// The plane an earlier operation cleared under the footprint of an
/// operation cut from a start plane ([`cleared_start_plane`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearedPlane<'p> {
    /// The operation whose cut left the plane: an enabled Pocket, or an
    /// enabled V-Carve with a flat depth (its floor, #426).
    pub by: &'p Operation,
}

impl ClearedPlane<'_> {
    /// How far below the stock top the plane is, mm: the Pocket's depth
    /// (`None` when it has none set), or the V-Carve's start depth plus
    /// its flat depth ([`Operation::depth_mm`]).
    #[must_use]
    pub fn depth_mm(&self) -> Option<f64> {
        self.by.depth_mm()
    }

    /// Whether the plane is `start_depth_mm` below the stock top, within
    /// [`EPS_MM`]: then it is the start plane of the operation it holds,
    /// and that operation's flank is engaged from it (security review
    /// H4). At another depth it is `start_depth_mismatch`.
    #[must_use]
    pub fn is_at(&self, start_depth_mm: f64) -> bool {
        self.depth_mm()
            .is_some_and(|d| (d - start_depth_mm).abs() <= EPS_MM)
    }
}

/// The plane an earlier operation cleared under the start plane of `op`
/// (functional spec, capability 41): the last operation before `op` in
/// the operation list that is
///
/// - **an enabled Pocket** whose regions (its targets' closed outlines,
///   nested) hold the whole footprint of `op`; the plane is its depth; or
/// - **an enabled V-Carve with a flat depth `F`** (#426) whose floor
///   regions hold it: its targets' regions inset by `F · tan(α/2)` with
///   its own V-bit ([`dc_geometry::floor_regions`], less the pieces too
///   thin to cut, as the generator's floor and the safety pass derive
///   them). The plane is its start depth plus `F`. The floor is cut
///   whole by the V-bit's floor rings and any clearing passes, and above
///   it the carve's walls slope outward, so a footprint inside the floor
///   at floor level is in air all the way up.
///
/// Held means inside a region and outside its islands. `None` when there
/// is no such operation, when `op` is not in `project` or is not cut from
/// a start plane, or when its footprint does not resolve (a missing
/// instance or tool, a degenerate or crossing outline, no hole:
/// validation or the generator reports those). A V-Carve whose floor does
/// not resolve (no V-bit with an angle in (0°, 180°), a start depth below
/// 0) clears nothing.
///
/// The footprint is what `op` cuts at its start plane:
///
/// - **A V-Carve:** the closed outlines of its targets, as placed; open
///   paths have no area and are left out. Containment is checked two
///   ways, both needed: every vertex of every outline is in a region by
///   [`dc_geometry::Region::contains_point`], and the area the outlines
///   enclose, nested even-odd as the carve nests them, less the clearing
///   regions, is nothing within the flattening slack (the project's
///   flatten tolerance times the outlines' length). The second catches an
///   outline whose vertices are all inside but whose edges cross a gap or
///   an island; an outline crossing a region boundary does not count.
/// - **A countersink Drill** (format 21, #425): the disc of its tool's
///   radius at the centre of each target circle (closed profiles that
///   are circles within the flatten tolerance, as the Drill finds its
///   holes), not the hole circle. That is the bit's widest section above
///   the start plane, in the cleared air: a floor that holds the hole but
///   not the bit round it would let the bit cut its wall. A disc is held
///   when its centre is in a region and at least the radius, less the
///   flatten tolerance, from the region's outline and islands, arcs
///   measured as arcs. Every circle counts, a hole the countersink skips
///   (`countersink_mouth_not_wider_than_hole`) included; the targets
///   that are no circle are not cut, so they are left out.
///
/// The plane's depth is not compared with the start depth here: a floor
/// at another depth still holds the footprint, which
/// `start_depth_mismatch` reports ([`ClearedPlane::is_at`]).
#[must_use]
pub fn cleared_start_plane<'p>(project: &'p Project, op: &Operation) -> Option<ClearedPlane<'p>> {
    let index = project.operations.iter().position(|o| o.id() == op.id())?;
    let tolerance_mm =
        if project.flatten_tolerance_mm.is_finite() && project.flatten_tolerance_mm > 0.0 {
            project.flatten_tolerance_mm.min(MAX_FLATTEN_TOLERANCE_MM)
        } else {
            FLATTEN_TOLERANCE_DEFAULT_MM
        };
    let footprint = Footprint::of(project, op, tolerance_mm)?;
    project
        .operations
        .iter()
        .take(index)
        .rev()
        .find_map(|earlier| {
            let regions = match earlier {
                Operation::Pocket(pocket) if pocket.enabled => {
                    target_regions(project, &pocket.targets)?
                }
                Operation::VCarve(carve) if carve.enabled => {
                    v_carve_floor(project, carve, tolerance_mm)?
                }
                _ => return None,
            };
            footprint
                .held_by(&regions)
                .then_some(ClearedPlane { by: earlier })
        })
}

/// The closed outlines of `targets` nested even-odd, as a Pocket and a
/// V-Carve nest them; `None` when they do not resolve.
fn target_regions(project: &Project, targets: &[InstanceId]) -> Option<Vec<Region>> {
    let (regions, _) = nest(closed_outlines(project, targets)?).ok()?;
    Some(regions)
}

/// The floor regions of V-Carve `carve` (#426): its targets' regions
/// inset by `F · tan(α/2)` with its own V-bit, less those too thin to cut
/// (area at most `tolerance_mm` times the perimeter, the generator's
/// noise rule). `None` without a flat depth above 0, without a V-bit
/// with an angle in (0°, 180°), with a start depth that is not 0 or more,
/// or when the targets do not resolve.
fn v_carve_floor(project: &Project, carve: &VCarveOp, tolerance_mm: f64) -> Option<Vec<Region>> {
    let flat_depth_mm = carve.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0)?;
    if !(carve.start_depth_mm.is_finite() && carve.start_depth_mm >= 0.0) {
        return None;
    }
    let tool = project.tools.iter().find(|t| t.id == carve.tool)?;
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    if !(included_angle_deg > 0.0 && included_angle_deg < 180.0) {
        return None;
    }
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    let inset_mm = flat_depth_mm * slope;
    if !(inset_mm.is_finite() && inset_mm > 0.0) {
        return None;
    }
    let regions = target_regions(project, &carve.targets)?;
    Some(
        floor_regions(&regions, inset_mm, tolerance_mm)
            .into_iter()
            .filter(|floor| {
                let perimeter_mm: f64 = std::iter::once(floor.outer())
                    .chain(floor.holes())
                    .map(|c| c.polyline().length_mm())
                    .sum();
                floor.area_mm2() > tolerance_mm * perimeter_mm
            })
            .collect(),
    )
}

/// What an operation cut from a start plane occupies at that plane, which
/// the regions of an earlier operation must hold for its floor to be the
/// plane ([`cleared_start_plane`]).
enum Footprint {
    /// A V-Carve's closed target outlines.
    Outlines {
        /// The outlines, as placed.
        outlines: Vec<Contour>,
        /// The outlines nested even-odd, as the carve nests them.
        regions: Vec<Region>,
        /// The flatten tolerance, mm, which the area test is within.
        tolerance_mm: f64,
        /// The area the test lets stand outside, mm²: the tolerance times
        /// the outlines' length.
        slack_mm2: f64,
    },
    /// A countersink's discs: each hole's centre with the tool's radius.
    Discs {
        /// The centres, as placed.
        centres: Vec<Point>,
        /// The tool's radius, mm.
        radius_mm: f64,
        /// How far, mm, a disc may reach past a wall.
        tolerance_mm: f64,
    },
}

impl Footprint {
    /// The footprint of `op`; `None` for an operation not cut from a start
    /// plane, or one whose footprint does not resolve or is empty.
    fn of(project: &Project, op: &Operation, tolerance_mm: f64) -> Option<Self> {
        match op {
            Operation::VCarve(carve) => {
                let outlines = closed_outlines(project, &carve.targets)?;
                if outlines.is_empty() {
                    return None;
                }
                let (regions, _) = nest(outlines.clone()).ok()?;
                let slack_mm2 = tolerance_mm
                    * outlines
                        .iter()
                        .map(|c| c.polyline().length_mm())
                        .sum::<f64>();
                Some(Self::Outlines {
                    outlines,
                    regions,
                    tolerance_mm,
                    slack_mm2,
                })
            }
            Operation::Drill(drill) if drill.cycle.is_countersink() => {
                let tool = project.tools.iter().find(|t| t.id == drill.tool)?;
                let radius_mm = tool.diameter_mm / 2.0;
                if !(radius_mm.is_finite() && radius_mm > 0.0) {
                    return None;
                }
                let centres: Vec<Point> = closed_outlines(project, &drill.targets)?
                    .iter()
                    .filter_map(|c| as_circle(c.polyline(), project.flatten_tolerance_mm))
                    .map(|(centre, _)| centre)
                    .collect();
                (!centres.is_empty()).then_some(Self::Discs {
                    centres,
                    radius_mm,
                    tolerance_mm,
                })
            }
            Operation::Profile(_)
            | Operation::Pocket(_)
            | Operation::Drill(_)
            | Operation::Bore(_)
            | Operation::Engrave(_) => None,
        }
    }

    /// True when `regions` hold the whole footprint, inside a region and
    /// outside its islands.
    fn held_by(&self, regions: &[Region]) -> bool {
        match self {
            Self::Outlines {
                outlines,
                regions: footprint,
                tolerance_mm,
                slack_mm2,
            } => {
                let vertices_inside = outlines.iter().all(|outline| {
                    outline.polyline().vertices().iter().all(|v| {
                        let p = Point::new(v.x, v.y);
                        regions.iter().any(|r| r.contains_point(p))
                    })
                });
                vertices_inside
                    && area_mm2(&difference(footprint, regions, *tolerance_mm)) <= *slack_mm2
            }
            Self::Discs {
                centres,
                radius_mm,
                tolerance_mm,
            } => centres.iter().all(|&centre| {
                regions.iter().any(|region| {
                    region.contains_point(centre)
                        && std::iter::once(region.outer())
                            .chain(region.holes())
                            .flat_map(|wall| wall.polyline().segments())
                            .all(|seg| {
                                point_segment_distance_mm(seg, centre) >= radius_mm - tolerance_mm
                            })
                })
            }),
        }
    }
}

/// The closed outlines of `targets`, placed in project space, in target
/// then profile order; `None` when one does not resolve.
fn closed_outlines(project: &Project, targets: &[InstanceId]) -> Option<Vec<Contour>> {
    let mut outlines = Vec::new();
    for id in targets {
        let instance = project.instances.iter().find(|i| &i.id == id)?;
        let definition = project.definitions.get(&instance.definition)?;
        for profile in definition.profiles.iter().filter(|p| p.is_closed()) {
            let placed = instance.transform.apply_polyline(profile).ok()?;
            outlines.push(Contour::new(placed).ok()?);
        }
    }
    Some(outlines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DefinitionId, InstanceId, OperationId};
    use crate::model::{FeatureDefinition, FeatureInstance, PocketOp};
    use crate::test_support::{sample_pocket, sample_project, sample_v_carve, tool_v_bit_90};
    use dc_geometry::{Point, Polyline, Transform};

    /// A `w` × `h` mm rectangle placed with its corner at (`x`, `y`) as
    /// instance `id` of its own definition.
    fn place_rect(project: &mut Project, id: &str, x: f64, y: f64, w: f64, h: f64) {
        let definition = DefinitionId::new(format!("d_{id}"));
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: id.into(),
                profiles: vec![Polyline::rect(Point { x: 0.0, y: 0.0 }, w, h)],
            },
        );
        project.instances.push(FeatureInstance {
            id: InstanceId::new(id),
            definition,
            transform: Transform::translate(x, y),
            name: id.into(),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
    }

    fn pocket(id: &str, targets: &[&str]) -> Operation {
        Operation::Pocket(PocketOp {
            id: OperationId::new(id),
            depth_mm: Some(2.0),
            targets: targets.iter().map(|t| InstanceId::new(*t)).collect(),
            ..sample_pocket()
        })
    }

    fn carve(targets: &[&str]) -> Operation {
        let mut op = sample_v_carve();
        op.targets = targets.iter().map(|t| InstanceId::new(*t)).collect();
        op.start_depth_mm = 2.0;
        Operation::VCarve(op)
    }

    /// The id of the Pocket containing the carve, the last operation.
    fn found(project: &Project) -> Option<&str> {
        let op = project.operations.last()?;
        cleared_start_plane(project, op).map(|c| c.by.id().as_str())
    }

    /// The sample's 50 × 30 rectangle `i1` at (10, 10) and, beside it, a
    /// 20 × 20 square `far` at (70, 10); inside `i1` a square `in1` at
    /// (20, 20), an island `isle` at (40, 20) and a square `in2` around
    /// the island; `cross` straddling `i1`'s right edge; `out` outside
    /// both; `in_far` inside `far`; `bridge` from `i1` across the gap to
    /// `far`, its corners in one region or the other.
    fn shapes() -> Project {
        let mut p = sample_project();
        p.tools.push(tool_v_bit_90());
        place_rect(&mut p, "far", 70.0, 10.0, 20.0, 20.0);
        place_rect(&mut p, "in1", 20.0, 20.0, 10.0, 10.0);
        place_rect(&mut p, "isle", 40.0, 20.0, 5.0, 5.0);
        place_rect(&mut p, "in2", 35.0, 15.0, 15.0, 15.0);
        place_rect(&mut p, "cross", 55.0, 20.0, 10.0, 10.0);
        place_rect(&mut p, "out", 70.0, 70.0, 10.0, 10.0);
        place_rect(&mut p, "in_far", 75.0, 15.0, 10.0, 10.0);
        place_rect(&mut p, "bridge", 50.0, 15.0, 30.0, 10.0);
        p
    }

    /// Design spec, "Start depth", step 2: every footprint outline must
    /// lie wholly inside one region of the Pocket, outside its islands;
    /// one outside, one crossing a boundary, one bridging two regions or
    /// one around an island does not count. A disabled Pocket and one
    /// after the carve do not count; of two, the later one is found.
    #[test]
    fn containing_pocket_needs_every_outline_inside_one_region() {
        let cases: &[(&[&str], &[&str], Option<&str>)] = &[
            (&["i1"], &["in1"], Some("p1")),
            (&["i1"], &["in1", "out"], None),
            (&["i1"], &["cross"], None),
            (&["i1"], &["out"], None),
            // Each outline inside one of the regions is enough.
            (&["i1", "far"], &["in1", "in_far"], Some("p1")),
            // Every corner is in a region, but the middle is in none.
            (&["i1", "far"], &["bridge"], None),
            // The island is a hole of the Pocket's region: a carve around
            // it is not on the cleared floor, one beside it is.
            (&["i1", "isle"], &["in2"], None),
            (&["i1", "isle"], &["in1"], Some("p1")),
            // The carve's own outline is the Pocket's: on its boundary.
            (&["i1"], &["i1"], Some("p1")),
        ];
        for (pocketed, carved, expected) in cases {
            let mut p = shapes();
            p.operations = vec![pocket("p1", pocketed), carve(carved)];
            assert_eq!(found(&p), *expected, "{pocketed:?} around {carved:?}");
        }

        let mut p = shapes();
        p.operations = vec![
            pocket("p1", &["i1"]),
            pocket("p2", &["i1"]),
            carve(&["in1"]),
        ];
        assert_eq!(found(&p), Some("p2"), "the last containing Pocket");
        if let Some(Operation::Pocket(p2)) = p.operations.get_mut(1) {
            p2.enabled = false;
        }
        assert_eq!(found(&p), Some("p1"), "a disabled Pocket does not count");
        if let Some(Operation::Pocket(p1)) = p.operations.get_mut(0) {
            p1.enabled = false;
        }
        assert_eq!(found(&p), None, "nor does the other once disabled");

        let mut p = shapes();
        p.operations = vec![carve(&["in1"]), pocket("p1", &["i1"])];
        let op = p.operations.first().cloned().unwrap();
        assert_eq!(
            cleared_start_plane(&p, &op).map(|c| c.by.id().as_str()),
            None,
            "a Pocket after the carve clears nothing before it"
        );
        // An operation not in the project has no earlier Pocket.
        assert_eq!(cleared_start_plane(&shapes(), &op), None);
    }

    /// A circle `diameter_mm` wide centred at (`x`, `y`) as instance `id`
    /// of its own definition.
    fn place_circle(project: &mut Project, id: &str, x: f64, y: f64, diameter_mm: f64) {
        let definition = DefinitionId::new(format!("d_{id}"));
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: id.into(),
                profiles: vec![Polyline::circle(
                    Point { x: 0.0, y: 0.0 },
                    diameter_mm / 2.0,
                )],
            },
        );
        project.instances.push(FeatureInstance {
            id: InstanceId::new(id),
            definition,
            transform: Transform::translate(x, y),
            name: id.into(),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
    }

    /// A 6 mm countersink of `targets` with the 6.35 mm V-bit `t2`, 2 mm
    /// down.
    fn countersink(targets: &[&str]) -> Operation {
        Operation::Drill(crate::model::DrillOp {
            id: OperationId::new("o3"),
            targets: targets.iter().map(|t| InstanceId::new(*t)).collect(),
            tool: tool_v_bit_90().id,
            depth_mm: None,
            cycle: crate::model::DrillCycle::Countersink {
                diameter_mm: 6.0,
                start_depth_mm: 2.0,
                start_plane_cleared_externally: false,
            },
            ..crate::test_support::sample_drill()
        })
    }

    /// The issue #425 decision: a countersink's footprint is the disc of
    /// its tool's radius at each hole, the bit's widest section above
    /// the start plane, not the hole circle. A Pocket that holds the
    /// 3.175 mm hole but not the 6.35 mm bit round it would let the
    /// countersink cut its wall, so it does not count; nor does one
    /// whose island the bit reaches. A bit touching the wall counts (the
    /// cut stops at it). The countersink's other targets are skipped, not
    /// cut, so they need no Pocket.
    #[test]
    fn countersink_containment_needs_the_whole_bit_inside_the_pocket() {
        let hole_mm = 3.175;
        let radius_mm = tool_v_bit_90().diameter_mm / 2.0;
        // The sample rectangle `i1` spans x 10..60, y 10..40.
        let cases: &[(&str, f64, f64, Option<&str>)] = &[
            ("middle", 35.0, 25.0, Some("p1")),
            // The hole is inside, 0.8 mm from the left wall, but the bit
            // reaches 1.6 mm past it.
            ("hole inside, bit outside", 12.0, 25.0, None),
            ("bit touching the wall", 10.0 + radius_mm, 25.0, Some("p1")),
            (
                "bit 0.05 mm over the wall",
                10.0 + radius_mm - 0.05,
                25.0,
                None,
            ),
            ("bit over the top wall", 35.0, 38.0, None),
            // The island `isle` spans x 40..45, y 20..25.
            ("bit over the island", 47.0, 22.5, None),
            ("bit beside the island", 50.0, 22.5, Some("p1")),
            ("outside", 80.0, 80.0, None),
        ];
        for (name, x, y, expected) in cases {
            let mut p = shapes();
            place_circle(&mut p, "hole", *x, *y, hole_mm);
            p.operations = vec![pocket("p1", &["i1", "isle"]), countersink(&["hole"])];
            assert_eq!(found(&p), *expected, "{name}");
        }

        // A square target is skipped by the countersink, so a Pocket that
        // holds the bit at the hole counts, wherever the square is.
        let mut p = shapes();
        place_circle(&mut p, "hole", 35.0, 25.0, hole_mm);
        p.operations = vec![pocket("p1", &["i1"]), countersink(&["hole", "out"])];
        assert_eq!(found(&p), Some("p1"), "a square target is no hole");
        // Every hole must be held.
        place_circle(&mut p, "far_hole", 80.0, 80.0, hole_mm);
        p.operations = vec![pocket("p1", &["i1"]), countersink(&["hole", "far_hole"])];
        assert_eq!(found(&p), None, "one hole outside");
        // Nothing to hold: no hole, or no tool to size the bit.
        p.operations = vec![pocket("p1", &["i1"]), countersink(&["out"])];
        assert_eq!(found(&p), None, "no hole");
        p.operations = vec![pocket("p1", &["i1"]), countersink(&["hole"])];
        p.tools.retain(|t| t.id != tool_v_bit_90().id);
        assert_eq!(found(&p), None, "no tool");

        // A simple Drill is not cut from a start plane at all.
        let mut p = shapes();
        place_circle(&mut p, "hole", 35.0, 25.0, hole_mm);
        let mut simple = countersink(&["hole"]);
        if let Operation::Drill(drill) = &mut simple {
            drill.cycle = crate::model::DrillCycle::Simple;
        }
        p.operations = vec![pocket("p1", &["i1"]), simple];
        assert_eq!(found(&p), None, "a simple Drill");
    }

    // #426: a V-Carve's flat floor clears a start plane.

    /// A V-Carve `v1` of `targets` with the 90° V-bit `t2` (slope 1),
    /// `flat_mm` flat depth and `start_mm` start depth.
    fn floored(targets: &[&str], flat_mm: Option<f64>, start_mm: f64) -> Operation {
        let mut op = sample_v_carve();
        op.id = OperationId::new("v1");
        op.targets = targets.iter().map(|t| InstanceId::new(*t)).collect();
        op.flat_depth_mm = flat_mm;
        op.start_depth_mm = start_mm;
        Operation::VCarve(op)
    }

    /// The id of the operation that cleared a plane, and its depth.
    type Plane<'a> = Option<(&'a str, Option<f64>)>;

    /// The id and depth of the plane cleared under the last operation.
    fn plane(project: &Project) -> Plane<'_> {
        let op = project.operations.last()?;
        cleared_start_plane(project, op).map(|c| (c.by.id().as_str(), c.depth_mm()))
    }

    /// `shapes()` with a 3.175 mm hole `hole` at (`x`, `y`).
    fn with_hole(x: f64, y: f64) -> Project {
        let mut p = shapes();
        place_circle(&mut p, "hole", x, y, 3.175);
        p
    }

    /// Issue #426: an enabled earlier V-Carve with a flat depth `F`
    /// clears the plane `s0 + F` down for a footprint inside its floor
    /// regions, the targets inset by `F · tan(α/2)` with its own V-bit.
    /// The sample rectangle `i1` (x 10..60, y 10..40) carved 2.5 mm flat
    /// with the 90° bit has its floor at x 12.5..57.5, y 12.5..37.5. A
    /// countersink's 6.35 mm bit in the middle is held; a hole inside
    /// the targets but outside the floor, on the slope, is not, nor is a
    /// hole on the floor whose bit reaches the slope. An island of the
    /// carve grows by the inset on its floor. A V-Carve's outline on the
    /// floor counts the same way.
    #[test]
    fn v_carve_floor_clears_its_start_plane() {
        let radius_mm = tool_v_bit_90().diameter_mm / 2.0;
        let cases: &[(&str, f64, f64, Plane<'_>)] = &[
            ("middle", 35.0, 25.0, Some(("v1", Some(2.5)))),
            ("on the slope", 11.5, 25.0, None),
            ("bit over the slope", 14.0, 25.0, None),
            (
                "bit touching the floor's edge",
                12.5 + radius_mm,
                25.0,
                Some(("v1", Some(2.5))),
            ),
            ("outside", 80.0, 80.0, None),
        ];
        for (name, x, y, expected) in cases {
            let mut p = with_hole(*x, *y);
            p.operations = vec![floored(&["i1"], Some(2.5), 0.0), countersink(&["hole"])];
            assert_eq!(plane(&p), *expected, "{name}");
        }

        // The plane is the V-Carve's start depth plus its flat depth.
        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![floored(&["i1"], Some(2.5), 1.0), countersink(&["hole"])];
        assert_eq!(plane(&p), Some(("v1", Some(3.5))));
        let sink = p.operations.last().cloned().unwrap();
        let cleared = cleared_start_plane(&p, &sink).unwrap();
        assert!(cleared.is_at(3.5) && !cleared.is_at(2.0));

        // The island `isle` (x 40..45, y 20..25) grows to x 37.5..47.5 on
        // the floor: a bit at x 35 reaches it, one at x 25 does not.
        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![
            floored(&["i1", "isle"], Some(2.5), 0.0),
            countersink(&["hole"]),
        ];
        assert_eq!(plane(&p), None, "bit over the grown island");
        let mut p = with_hole(25.0, 25.0);
        p.operations = vec![
            floored(&["i1", "isle"], Some(2.5), 0.0),
            countersink(&["hole"]),
        ];
        assert_eq!(plane(&p), Some(("v1", Some(2.5))), "beside the island");

        // A V-Carve carved into the floor: `in1` (x 20..30) is on it, a
        // 5 mm square from x 11 crosses its edge at 12.5.
        let mut p = shapes();
        place_rect(&mut p, "edge", 11.0, 15.0, 5.0, 5.0);
        p.operations = vec![floored(&["i1"], Some(2.5), 0.0), carve(&["in1"])];
        assert_eq!(
            plane(&p),
            Some(("v1", Some(2.5))),
            "an outline on the floor"
        );
        p.operations = vec![floored(&["i1"], Some(2.5), 0.0), carve(&["edge"])];
        assert_eq!(plane(&p), None, "an outline over the slope");
    }

    /// Issue #426: like a Pocket, a disabled V-Carve and one after the
    /// operation clear nothing; of an earlier Pocket and V-Carve that
    /// both hold the footprint, the later one is the plane.
    #[test]
    fn disabled_or_later_v_carve_does_not_clear() {
        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![floored(&["i1"], Some(2.5), 0.0), countersink(&["hole"])];
        assert_eq!(plane(&p), Some(("v1", Some(2.5))), "enabled, before");

        if let Some(Operation::VCarve(v1)) = p.operations.first_mut() {
            v1.enabled = false;
        }
        assert_eq!(plane(&p), None, "disabled");

        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![countersink(&["hole"]), floored(&["i1"], Some(2.5), 0.0)];
        let sink = p.operations.first().cloned().unwrap();
        assert_eq!(
            cleared_start_plane(&p, &sink).map(|c| c.by.id().as_str()),
            None,
            "after"
        );

        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![
            pocket("p1", &["i1"]),
            floored(&["i1"], Some(2.5), 0.0),
            countersink(&["hole"]),
        ];
        assert_eq!(plane(&p), Some(("v1", Some(2.5))), "the V-Carve is later");
        p.operations.swap(0, 1);
        assert_eq!(plane(&p), Some(("p1", Some(2.0))), "the Pocket is later");
    }

    /// Issue #426: a V-Carve with no flat depth has no floor, so it
    /// clears no plane; nor does one whose floor does not resolve (no
    /// V-bit to give its slope).
    #[test]
    fn v_carve_without_a_flat_depth_does_not_clear() {
        let mut p = with_hole(35.0, 25.0);
        p.operations = vec![floored(&["i1"], Some(2.5), 0.0), countersink(&["hole"])];
        assert_eq!(plane(&p), Some(("v1", Some(2.5))), "with one");

        p.operations = vec![floored(&["i1"], None, 0.0), countersink(&["hole"])];
        assert_eq!(plane(&p), None, "without one");

        p.operations = vec![floored(&["i1"], Some(2.5), 0.0), countersink(&["hole"])];
        if let Some(Operation::VCarve(v1)) = p.operations.first_mut() {
            v1.tool = crate::ids::ToolId::new("t1");
        }
        assert_eq!(plane(&p), None, "an end mill has no slope");
    }
}
