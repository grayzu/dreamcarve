// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Questions about a whole project's geometry that validation and the
//! generators must answer the same way (`docs/specs/phase-3/design.md`,
//! "Algorithms → Start depth"). They live here, not in `dc-cam`, because
//! validation may not depend on `dc-cam` (`docs/rust-guidelines.md`,
//! "Crate boundaries"); `dc-cam` calls them.

use crate::ids::InstanceId;
use crate::model::{Operation, PocketOp, Project};
use crate::validate::MAX_FLATTEN_TOLERANCE_MM;
use dc_geometry::{
    Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Region, area_mm2, as_circle, difference, nest,
    point_segment_distance_mm,
};

/// The Pocket that clears the start plane of `op` (functional spec,
/// capability 41): the last enabled Pocket before `op` in the operation
/// list whose regions hold the whole footprint of `op`, inside a region
/// and outside its islands. `None` when there is no such Pocket, when `op`
/// is not in `project` or is not cut from a start plane, or when its
/// footprint does not resolve (a missing instance or tool, a degenerate
/// or crossing outline, no hole: validation or the generator reports
/// those).
///
/// The footprint is what `op` cuts at its start plane:
///
/// - **A V-Carve:** the closed outlines of its targets, as placed; open
///   paths have no area and are left out. Containment is checked two
///   ways, both needed: every vertex of every outline is in a region by
///   [`dc_geometry::Region::contains_point`], and the area the outlines
///   enclose, nested even-odd as the carve nests them, less the Pocket's
///   regions, is nothing within the flattening slack (the project's
///   flatten tolerance times the outlines' length). The second catches an
///   outline whose vertices are all inside but whose edges cross a gap or
///   an island; an outline crossing a region boundary does not count.
/// - **A countersink Drill** (format 21, #425): the disc of its tool's
///   radius at the centre of each target circle (closed profiles that
///   are circles within the flatten tolerance, as the Drill finds its
///   holes), not the hole circle. That is the bit's widest section above
///   the start plane, in the Pocket's air: a Pocket that holds the hole
///   but not the bit round it would let the bit cut its wall. A disc is
///   held when its centre is in a region and at least the radius, less
///   the flatten tolerance, from the region's outline and islands,
///   arcs measured as arcs. Every circle counts, a hole the countersink
///   skips (`countersink_mouth_not_wider_than_hole`) included; the
///   targets that are no circle are not cut, so they are left out.
///
/// The Pocket's depth is not compared: a Pocket at another depth still
/// contains the targets, which `start_depth_mismatch` reports.
#[must_use]
pub fn containing_pocket<'p>(project: &'p Project, op: &Operation) -> Option<&'p PocketOp> {
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
        .find_map(|earlier| match earlier {
            Operation::Pocket(pocket) if pocket.enabled => {
                let regions = closed_outlines(project, &pocket.targets)
                    .and_then(|contours| nest(contours).ok())
                    .map(|(regions, _)| regions)?;
                footprint.held_by(&regions).then_some(pocket)
            }
            _ => None,
        })
}

/// What an operation cut from a start plane occupies at that plane, which
/// the regions of an earlier operation must hold for its floor to be the
/// plane ([`containing_pocket`]).
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
    use crate::model::{FeatureDefinition, FeatureInstance};
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
        containing_pocket(project, op).map(|p| p.id.as_str())
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
            containing_pocket(&p, &op).map(|p| p.id.as_str()),
            None,
            "a Pocket after the carve clears nothing before it"
        );
        // An operation not in the project has no earlier Pocket.
        assert_eq!(containing_pocket(&shapes(), &op), None);
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
}
