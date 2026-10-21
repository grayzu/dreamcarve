// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The check that the tool paths of two shapes in one Profile do not cut
//! into each other (`docs/research/toolpath-ordering.md`, decision D6).
//! DreamCarve cuts each shape on its own rather than merging close outlines
//! as PureCutCNC's `edgeLevelRuns` does, so it warns instead.
//!
//! The tool sweeps a band `r` wide either side of its centre path. That band
//! reaches the material another shape keeps exactly when the path comes
//! closer than `r` to that shape's line: next to the line, its kept side is
//! the material of the region it bounds (the part, or for a hole the frame
//! around it). Under Inside every path stays inside its own region, at
//! least `r` from every other region's line, so only Outside and On line
//! cuts can warn. A part and its own holes are one region and are never
//! compared: cutting close to them leaves a thin wall, not a gouge.

use crate::error::ShapeRef;
use crate::warning::{Warning, WarningCode};
use dc_geometry::{Contour, EPS_MM, Polyline, Rect, Region, distance_mm};
use dc_project::{Side, Tool};

/// One tool-centre path in cut order, with the shape it cuts.
pub(crate) type CutPath<'a> = (Polyline, Option<&'a ShapeRef>);

/// One [`WarningCode::ToolpathsOverlap`] per pair of shapes, from different
/// regions, where either one's tool path comes closer than the tool radius
/// to the other's line. `paths` are in cut order; so are the warnings, by
/// the first path of each pair and then the second, and the shape cut
/// first is named first. `shapes` are the resolved targets and `regions`
/// what `nest` made of them.
#[must_use]
pub(crate) fn overlap_warnings(
    paths: &[CutPath<'_>],
    shapes: &[(ShapeRef, Contour)],
    regions: &[Region],
    side: Side,
    tool: &Tool,
) -> Vec<Warning> {
    let radius_mm = tool.diameter_mm / 2.0;
    // Each path that has a known shape, with that shape's line and region.
    let placed: Vec<(&Polyline, &ShapeRef, &Contour, usize)> = paths
        .iter()
        .filter_map(|(path, shape)| {
            let shape = (*shape)?;
            let (_, contour) = shapes.iter().find(|(s, _)| s == shape)?;
            let region = regions.iter().position(|r| {
                std::iter::once(r.outer())
                    .chain(r.holes())
                    .any(|c| c == contour || *c == contour.reversed())
            })?;
            Some((path, shape, contour, region))
        })
        .collect();

    let mut warned: Vec<(&ShapeRef, &ShapeRef)> = Vec::new();
    let mut warnings = Vec::new();
    for (index, &(path_a, shape_a, line_a, region_a)) in placed.iter().enumerate() {
        for &(path_b, shape_b, line_b, region_b) in placed.iter().skip(index + 1) {
            if region_a == region_b
                || warned
                    .iter()
                    .any(|&(x, y)| (x, y) == (shape_a, shape_b) || (x, y) == (shape_b, shape_a))
            {
                continue;
            }
            if !(within(path_a, line_b.polyline(), radius_mm)
                || within(path_b, line_a.polyline(), radius_mm))
            {
                continue;
            }
            warned.push((shape_a, shape_b));
            warnings.push(
                Warning::new(WarningCode::ToolpathsOverlap)
                    .with("instance", &shape_a.instance)
                    .with("instance_name", &shape_a.instance_name)
                    .with("profile", shape_a.profile)
                    .with("other_instance", &shape_b.instance)
                    .with("other_instance_name", &shape_b.instance_name)
                    .with("other_profile", shape_b.profile)
                    .with("gap_mm", distance_mm(line_a.polyline(), line_b.polyline()))
                    .with("tool", &tool.name)
                    .with("diameter_mm", tool.diameter_mm)
                    .with("needed_mm", needed_gap_mm(side, tool.diameter_mm)),
            );
        }
    }
    warnings
}

/// True when `path` comes closer than `radius_mm` to `line`, by more than
/// [`EPS_MM`]: shapes exactly a tool's reach apart are fine.
fn within(path: &Polyline, line: &Polyline, radius_mm: f64) -> bool {
    let reach_mm = radius_mm - EPS_MM;
    let near = match (path.bounds(), line.bounds()) {
        (Some(a), Some(b)) => boxes_within(a, b, reach_mm),
        _ => false,
    };
    near && distance_mm(path, line) < reach_mm
}

/// True when boxes `a` and `b` are less than `reach_mm` apart on each axis.
fn boxes_within(a: Rect, b: Rect, reach_mm: f64) -> bool {
    a.min.x - b.max.x < reach_mm
        && b.min.x - a.max.x < reach_mm
        && a.min.y - b.max.y < reach_mm
        && b.min.y - a.max.y < reach_mm
}

/// The gap two shapes need for this side not to cut into each other: both
/// tools run between them on Outside, half of each on the line.
fn needed_gap_mm(side: Side, diameter_mm: f64) -> f64 {
    match side {
        Side::Outside | Side::Inside => diameter_mm,
        Side::OnLine => diameter_mm / 2.0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail loudly on a bad fixture")]
    #![allow(clippy::indexing_slicing, reason = "tests index fixed fixtures")]

    use crate::profile::generate_profile;
    use crate::toolpath::{MoveKind, Toolpath};
    use crate::warning::{Warning, WarningCode};
    use dc_geometry::{Point, Polyline, Transform};
    use dc_project::test_support::sample_project;
    use dc_project::{
        CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, OperationId, ProfileOp, Project, Side, Stock, ToolId,
    };
    use std::collections::BTreeMap;

    const TOL: f64 = 1e-6;
    const THICKNESS_MM: f64 = 6.0;
    const TOOL_MM: f64 = 6.0;

    /// A 300 × 200 × 6 mm project with a 6 mm tool, the given definitions
    /// (id, profiles), and instances (id, definition id, transform).
    fn project(
        definitions: &[(&str, Vec<Polyline>)],
        instances: &[(&str, &str, Transform)],
    ) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        p.tools[0].diameter_mm = TOOL_MM;
        p.definitions = definitions
            .iter()
            .map(|(id, profiles)| {
                let def = FeatureDefinition {
                    id: DefinitionId::new(*id),
                    name: (*id).into(),
                    profiles: profiles.clone(),
                };
                (def.id.clone(), def)
            })
            .collect::<BTreeMap<_, _>>();
        p.instances = instances
            .iter()
            .map(|&(id, definition, transform)| FeatureInstance {
                id: InstanceId::new(id),
                definition: DefinitionId::new(definition),
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

    /// Profile of every instance, `side`, through the stock, no tabs.
    fn profile_op(project: &Project, side: Side) -> ProfileOp {
        ProfileOp {
            id: OperationId::new("o1"),
            name: "Profile".into(),
            enabled: true,
            targets: project.instances.iter().map(|i| i.id.clone()).collect(),
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(THICKNESS_MM),
            step_down_mm: None,
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: None,
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: CornerRelief::None,
        }
    }

    /// 20 mm squares, one per x offset, all at y = 20.
    fn squares(xs_mm: &[(&str, f64)]) -> Project {
        let instances: Vec<(&str, &str, Transform)> = xs_mm
            .iter()
            .map(|&(id, x)| (id, "square", Transform::translate(x, 20.0)))
            .collect();
        project(
            &[(
                "square",
                vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)],
            )],
            &instances,
        )
    }

    fn overlaps(warnings: &[Warning]) -> Vec<&Warning> {
        warnings
            .iter()
            .filter(|w| w.code == WarningCode::ToolpathsOverlap)
            .collect()
    }

    fn gap_mm(warning: &Warning) -> f64 {
        warning.param("gap_mm").unwrap().parse().unwrap()
    }

    /// The instances a warning names, first then other.
    fn named(warning: &Warning) -> (&str, &str) {
        (
            warning.param("instance").unwrap(),
            warning.param("other_instance").unwrap(),
        )
    }

    #[test]
    fn close_parts_warn_toolpaths_overlap() {
        let p = squares(&[("a", 20.0), ("b", 42.0)]);
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::Outside)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        let w = found[0];
        let (first, other) = named(w);
        let mut pair = [first, other];
        pair.sort_unstable();
        assert_eq!(pair, ["a", "b"]);
        assert_eq!(w.param("profile"), Some("0"));
        assert_eq!(w.param("other_profile"), Some("0"));
        assert!((gap_mm(w) - 2.0).abs() < TOL, "{w:?}");
        let text = w.to_string();
        assert!(
            text.contains("Shape a") && text.contains("Shape b"),
            "{text}"
        );
        assert!(text.contains("2 mm"), "{text}");
    }

    #[test]
    fn parts_a_tool_width_apart_are_quiet() {
        let p = squares(&[("a", 20.0), ("b", 46.5)]);
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::Outside)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    #[test]
    fn inside_cuts_of_close_parts_are_quiet() {
        let p = squares(&[("a", 20.0), ("b", 42.0)]);
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::Inside)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    #[test]
    fn on_line_cuts_need_only_the_radius() {
        let close = squares(&[("a", 20.0), ("b", 42.0)]);
        let (_, warnings) = generate_profile(&close, &profile_op(&close, Side::OnLine)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        assert_eq!(found[0].param("needed_mm"), Some("3"));
        let apart = squares(&[("a", 20.0), ("b", 43.5)]);
        let (_, warnings) = generate_profile(&apart, &profile_op(&apart, Side::OnLine)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    #[test]
    fn part_and_its_own_hole_are_not_compared() {
        // A 40 mm square with an 18 mm square hole 1 mm from its left edge.
        let p = project(
            &[(
                "ring",
                vec![
                    Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
                    Polyline::rect(Point::new(1.0, 11.0), 18.0, 18.0),
                ],
            )],
            &[("r", "ring", Transform::translate(20.0, 20.0))],
        );
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::Outside)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    #[test]
    fn island_inside_hole_too_close_warns() {
        // A 100 mm frame with a 60 mm square hole; a 58 mm round coaster in
        // the hole, 1 mm from each of its walls.
        let p = project(
            &[
                (
                    "frame",
                    vec![
                        Polyline::rect(Point::new(0.0, 0.0), 100.0, 100.0),
                        Polyline::rect(Point::new(20.0, 20.0), 60.0, 60.0),
                    ],
                ),
                (
                    "coaster",
                    vec![Polyline::circle(Point::new(50.0, 50.0), 29.0)],
                ),
            ],
            &[
                ("frame", "frame", Transform::translate(20.0, 20.0)),
                ("coaster", "coaster", Transform::translate(20.0, 20.0)),
            ],
        );
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::Outside)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        let w = found[0];
        let (first, other) = named(w);
        let mut pair = [first, other];
        pair.sort_unstable();
        assert_eq!(pair, ["coaster", "frame"]);
        let frame_profile = if first == "frame" {
            w.param("profile")
        } else {
            w.param("other_profile")
        };
        assert_eq!(frame_profile, Some("1"), "the frame's hole: {w:?}");
        assert!((gap_mm(w) - 1.0).abs() < TOL, "{w:?}");
    }

    /// The instance each contour of `toolpath` cuts, in cut order: the
    /// square whose centre is nearest the mean X of the cutting moves
    /// between two rapids.
    fn cut_order(toolpath: &Toolpath, xs_mm: &[(&str, f64)]) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        let mut cut_xs: Vec<f64> = Vec::new();
        for m in &toolpath.moves {
            if m.kind != MoveKind::Rapid {
                cut_xs.push(m.to.x);
                continue;
            }
            if cut_xs.is_empty() {
                continue;
            }
            #[allow(clippy::cast_precision_loss, reason = "a few hundred moves")]
            let mean_x = cut_xs.iter().sum::<f64>() / cut_xs.len() as f64;
            cut_xs.clear();
            let nearest = xs_mm.iter().min_by(|&&(_, a), &&(_, b)| {
                (a + 10.0 - mean_x)
                    .abs()
                    .total_cmp(&(b + 10.0 - mean_x).abs())
            });
            if let Some(&(id, _)) = nearest {
                order.push(id.to_owned());
            }
        }
        order
    }

    #[test]
    fn overlap_warning_is_deterministic() {
        let xs = [("a", 20.0), ("b", 42.0), ("c", 64.0), ("d", 86.0)];
        let p = squares(&xs);
        let op = profile_op(&p, Side::Outside);
        let (toolpath, first) = generate_profile(&p, &op).unwrap();
        let (_, second) = generate_profile(&p, &op).unwrap();
        assert_eq!(first, second);

        let order = cut_order(&toolpath, &xs);
        let index = |id: &str| order.iter().position(|o| o == id).unwrap();
        let pairs: Vec<(usize, usize)> = overlaps(&first)
            .iter()
            .map(|w| {
                let (a, b) = named(w);
                (index(a), index(b))
            })
            .collect();
        assert_eq!(pairs.len(), 3, "{first:?}");
        assert!(
            pairs.iter().all(|(a, b)| a < b),
            "first named is cut first: {pairs:?}"
        );
        assert!(
            pairs.windows(2).all(|w| w[0] < w[1]),
            "in cut order: {pairs:?}"
        );
    }
}
