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
use dc_geometry::{Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Point, area_mm2, difference, nest};

/// The Pocket that clears the start plane of `op` (functional spec,
/// capability 41): the last enabled Pocket before `op` in the operation
/// list whose regions contain every footprint outline of `op`, inside a
/// region and outside its islands. An outline crossing a region boundary
/// does not count. `None` when there is no such Pocket, when `op` is not
/// in `project`, or when its footprint does not resolve (a missing
/// instance, a degenerate or crossing outline: validation or the
/// generator reports those).
///
/// The footprint outlines are the closed outlines of `op`'s targets, as
/// placed; open paths have no area and are left out. Containment is
/// checked two ways, both needed: every vertex of every outline is in a
/// region by [`dc_geometry::Region::contains_point`], and the area the outlines enclose,
/// nested even-odd as the carve nests them, less the Pocket's regions, is
/// nothing within the flattening slack (the project's flatten tolerance
/// times the outlines' length). The second catches an outline whose
/// vertices are all inside but whose edges cross a gap or an island.
///
/// The Pocket's depth is not compared: a Pocket at another depth still
/// contains the targets, which `start_depth_mismatch` reports.
#[must_use]
pub fn containing_pocket<'p>(project: &'p Project, op: &Operation) -> Option<&'p PocketOp> {
    let index = project.operations.iter().position(|o| o.id() == op.id())?;
    let outlines = closed_outlines(project, op.targets())?;
    if outlines.is_empty() {
        return None;
    }
    let (footprint, _) = nest(outlines.clone()).ok()?;
    let tolerance_mm =
        if project.flatten_tolerance_mm.is_finite() && project.flatten_tolerance_mm > 0.0 {
            project.flatten_tolerance_mm.min(MAX_FLATTEN_TOLERANCE_MM)
        } else {
            FLATTEN_TOLERANCE_DEFAULT_MM
        };
    let slack_mm2 = tolerance_mm
        * outlines
            .iter()
            .map(|c| c.polyline().length_mm())
            .sum::<f64>();
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
                let vertices_inside = outlines.iter().all(|outline| {
                    outline.polyline().vertices().iter().all(|v| {
                        let p = Point::new(v.x, v.y);
                        regions.iter().any(|r| r.contains_point(p))
                    })
                });
                let outside_mm2 = area_mm2(&difference(&footprint, &regions, tolerance_mm));
                (vertices_inside && outside_mm2 <= slack_mm2).then_some(pocket)
            }
            _ => None,
        })
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
}
