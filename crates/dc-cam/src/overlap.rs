// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The check that the tool paths of two shapes in one Profile do not cut
//! into each other (`docs/research/toolpath-ordering.md`, decision D6).
//! DreamCarve cuts each shape on its own rather than merging close outlines
//! as PureCutCNC's `edgeLevelRuns` does, so it warns instead.

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
        CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, OperationId,
        ProfileOp, Project, Side, Stock, ToolId,
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
                z_top_mm: THICKNESS_MM,
                z_bottom_mm: 0.0,
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
    /// square whose 3 mm-wide surround holds each XY rapid's end.
    fn cut_order(toolpath: &Toolpath, xs_mm: &[(&str, f64)]) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for m in &toolpath.moves {
            if m.kind != MoveKind::Rapid {
                continue;
            }
            let p = m.to;
            let square = xs_mm
                .iter()
                .find(|&&(_, x)| p.x >= x - 3.5 && p.x <= x + 23.5 && p.y >= 16.5 && p.y <= 43.5);
            if let Some(&(id, _)) = square
                && order.last().map(String::as_str) != Some(id)
            {
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
