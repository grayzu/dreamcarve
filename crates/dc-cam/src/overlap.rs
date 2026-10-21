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
//! least `r` from every line of it (`generate_profile` refuses a band
//! narrower than the tool, #192), so at least `r` from every other
//! region's line too, and only Outside and On line cuts can warn. A part
//! and its own holes are one region and are never compared here: Outside,
//! cutting close to them leaves a thin wall, not a gouge; On line, lines
//! of one region closer than the tool's diameter would cut through the
//! part between them, which `generate_profile` refuses (#199).
//!
//! An open path an On line Profile cuts (#175) is its own line and its own
//! tool-centre path, compared by the same On line rule (#206). It belongs
//! to the region whose material holds all of it, as a hole does, and is
//! not compared with that region's lines: there it cuts into the part, not
//! beside it, and `generate_profile`'s band check refuses one closer than
//! the diameter to them away from where they meet (#210). An open path held by no region (in the waste, in a hole's
//! slug, or crossing a line) is a group of its own and is compared with
//! every other shape, and with itself (#224): where two of its stretches
//! far apart along it come closer than the radius, as in a tight spiral or
//! a hairpin, it warns as two separate paths there would. Engrave takes no part: it cuts every line as drawn,
//! so lines that cross or meet are the design, not shapes too close. Nor
//! does a Left or Right Profile (#207): a cut of one that comes within the
//! radius of another line is `generate_profile`'s error, since the line's
//! other side is the part.

use crate::error::ShapeRef;
use crate::profile::MEETING_REACH_DIAMETERS;
use crate::warning::Warning;
use dc_geometry::{Contour, EPS_MM, Polyline, Rect, Region, SelfBand, distance_mm};
use dc_project::{Side, Tool};

/// One tool-centre path in cut order, with the shape it cuts.
pub(crate) type CutPath<'a> = (&'a Polyline, Option<&'a ShapeRef>);

/// One [`Warning::ToolpathsOverlap`] per pair of shapes, from different
/// regions, where either one's tool path comes closer than the tool radius
/// to the other's line. `paths` are in cut order; so are the warnings, by
/// the first path of each pair and then the second, and the shape cut
/// first is named first. `shapes` are the resolved closed targets,
/// `regions` what `nest` made of them, and `open` the open paths cut On
/// line, as drawn (#206).
///
/// An open path no region holds is also measured against itself (#224,
/// [`SelfBand`], with the project's flatten tolerance `tolerance_mm` for
/// where it meets itself): where two of its stretches more than
/// [`MEETING_REACH_DIAMETERS`]` · d` apart along it come closer than the
/// radius, it warns with itself as both shapes, before its pairs with
/// later paths. Its sharp corners and the points where it touches or
/// crosses itself are set aside as `generate_profile`'s band check sets
/// them aside (#221).
#[must_use]
pub(crate) fn overlap_warnings(
    paths: &[CutPath<'_>],
    shapes: &[(ShapeRef, Contour)],
    open: &[(ShapeRef, Polyline)],
    regions: &[Region],
    side: Side,
    tool: &Tool,
    tolerance_mm: f64,
) -> Vec<Warning> {
    let radius_mm = tool.diameter_mm / 2.0;
    // Each path that has a known shape, with that shape's line and the
    // group it is never compared within: a closed shape's region; for an
    // open path the region whose material holds it, or else a group of
    // its own past the regions.
    let placed: Vec<(&Polyline, &ShapeRef, &Polyline, usize)> = paths
        .iter()
        .filter_map(|(path, shape)| {
            let shape = (*shape)?;
            if let Some((_, contour)) = shapes.iter().find(|(s, _)| s == shape) {
                let region = regions.iter().position(|r| {
                    std::iter::once(r.outer())
                        .chain(r.holes())
                        .any(|c| c == contour || *c == contour.reversed())
                })?;
                return Some((*path, shape, contour.polyline(), region));
            }
            let (index, (_, line)) = open.iter().enumerate().find(|(_, (s, _))| s == shape)?;
            let group = regions
                .iter()
                .position(|r| region_holds_path(r, line))
                .unwrap_or(regions.len() + index);
            Some((*path, shape, line, group))
        })
        .collect();

    let warning = |a: &ShapeRef, b: &ShapeRef, gap_mm: f64| Warning::ToolpathsOverlap {
        shape: a.clone(),
        other: b.clone(),
        gap_mm,
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        diameter_mm: tool.diameter_mm,
        needed_mm: needed_gap_mm(side, tool.diameter_mm),
    };
    let mut warned: Vec<(&ShapeRef, &ShapeRef)> = Vec::new();
    let mut warnings = Vec::new();
    for (index, &(path_a, shape_a, line_a, region_a)) in placed.iter().enumerate() {
        // #224: an open path in the waste against itself, as D6 compares
        // two paths there: at the radius.
        if region_a >= regions.len() && !line_a.is_closed() && !warned.contains(&(shape_a, shape_a))
        {
            let band = SelfBand::new(
                line_a,
                MEETING_REACH_DIAMETERS * tool.diameter_mm,
                tolerance_mm.max(EPS_MM),
            );
            if let Some(gap_mm) = band.narrowest_mm(radius_mm - EPS_MM, |_, _| true) {
                warned.push((shape_a, shape_a));
                warnings.push(warning(shape_a, shape_a, gap_mm));
            }
        }
        for &(path_b, shape_b, line_b, region_b) in placed.iter().skip(index + 1) {
            if region_a == region_b
                || warned
                    .iter()
                    .any(|&(x, y)| (x, y) == (shape_a, shape_b) || (x, y) == (shape_b, shape_a))
            {
                continue;
            }
            if !(within(path_a, line_b, radius_mm) || within(path_b, line_a, radius_mm)) {
                continue;
            }
            warned.push((shape_a, shape_b));
            warnings.push(warning(shape_a, shape_b, distance_mm(line_a, line_b)));
        }
    }
    warnings
}

/// True when `path` comes closer than `radius_mm` to `line`, by more than
/// [`EPS_MM`]: shapes exactly a tool's reach apart are fine.
pub(crate) fn within(path: &Polyline, line: &Polyline, radius_mm: f64) -> bool {
    let reach_mm = radius_mm - EPS_MM;
    let near = match (path.bounds(), line.bounds()) {
        (Some(a), Some(b)) => boxes_within(a, b, reach_mm),
        _ => false,
    };
    near && distance_mm(path, line) < reach_mm
}

/// True when every point of the open path `path` lies in `region`'s
/// material or on its lines (#206): each vertex and the middle of each
/// segment, so a line or arc that runs out of the region between two
/// vertices inside it does not count.
pub(crate) fn region_holds_path(region: &Region, path: &Polyline) -> bool {
    let middles = path
        .segments()
        .filter_map(|s| s.point_at(s.length_mm() / 2.0).map(|(p, _)| p));
    path.vertices()
        .iter()
        .map(|v| v.point())
        .chain(middles)
        .all(|p| region.contains_point(p))
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
        Side::Outside | Side::Inside | Side::Left | Side::Right => diameter_mm,
        Side::OnLine => diameter_mm / 2.0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail loudly on a bad fixture")]
    #![allow(clippy::indexing_slicing, reason = "tests index fixed fixtures")]

    use crate::error::ShapeRef;
    use crate::profile::generate_profile;
    use crate::toolpath::{MoveKind, Toolpath};
    use crate::warning::{Warning, WarningCode};
    use dc_geometry::{Point, Polyline, Transform, Vertex};
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
            .filter(|w| w.code() == WarningCode::ToolpathsOverlap)
            .collect()
    }

    /// The shapes, gap and needed gap of a `toolpaths_overlap` warning.
    fn overlap(warning: &Warning) -> (&ShapeRef, &ShapeRef, f64, f64) {
        let Warning::ToolpathsOverlap {
            shape,
            other,
            gap_mm,
            needed_mm,
            ..
        } = warning
        else {
            panic!("{warning:?}");
        };
        (shape, other, *gap_mm, *needed_mm)
    }

    fn gap_mm(warning: &Warning) -> f64 {
        overlap(warning).2
    }

    fn needed_mm(warning: &Warning) -> f64 {
        overlap(warning).3
    }

    /// The instances a warning names, first then other.
    fn named(warning: &Warning) -> (&str, &str) {
        let (shape, other, _, _) = overlap(warning);
        (shape.instance.as_str(), other.instance.as_str())
    }

    /// The profiles a warning names, first then other.
    fn profiles(warning: &Warning) -> (usize, usize) {
        let (shape, other, _, _) = overlap(warning);
        (shape.profile, other.profile)
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
        assert_eq!(profiles(w), (0, 0));
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
        assert!((needed_mm(found[0]) - 3.0).abs() < TOL, "{:?}", found[0]);
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
            profiles(w).0
        } else {
            profiles(w).1
        };
        assert_eq!(frame_profile, 1, "the frame's hole: {w:?}");
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

    // ── #206: open paths cut On line ──

    /// An open line from `from` to `to`.
    fn line(from: (f64, f64), to: (f64, f64)) -> Polyline {
        Polyline::new(
            vec![Vertex::line(from.0, from.1), Vertex::line(to.0, to.1)],
            false,
        )
    }

    /// The 20 mm square part `a` at (20, 20) and one open line instance
    /// `l` per entry of `lines`, drawn in project space.
    fn square_and_lines(lines: &[(&str, Polyline)]) -> Project {
        let mut definitions = vec![(
            "square",
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)],
        )];
        definitions.extend(lines.iter().map(|(id, l)| (*id, vec![l.clone()])));
        let mut instances = vec![("a", "square", Transform::translate(20.0, 20.0))];
        instances.extend(lines.iter().map(|(id, _)| (*id, *id, Transform::IDENTITY)));
        project(&definitions, &instances)
    }

    /// #206: an open line cut On line beside a part, closer than the tool
    /// radius to its outline, or crossing it, warns as two close parts do.
    #[test]
    fn open_on_line_path_too_close_to_a_part_warns() {
        // 2 mm right of the square's right edge (x = 40), 3 mm radius.
        let beside = square_and_lines(&[("l", line((42.0, 20.0), (42.0, 40.0)))]);
        let (_, warnings) = generate_profile(&beside, &profile_op(&beside, Side::OnLine)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        let w = found[0];
        let (first, other) = named(w);
        let mut pair = [first, other];
        pair.sort_unstable();
        assert_eq!(pair, ["a", "l"]);
        assert_eq!(profiles(w), (0, 0));
        assert!((needed_mm(w) - 3.0).abs() < TOL, "{:?}", w);
        assert!((gap_mm(w) - 2.0).abs() < TOL, "{w:?}");

        // Across the square's left edge: the gap is nothing.
        let across = square_and_lines(&[("l", line((10.0, 30.0), (30.0, 30.0)))]);
        let (_, warnings) = generate_profile(&across, &profile_op(&across, Side::OnLine)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        assert!(gap_mm(found[0]).abs() < TOL, "{:?}", found[0]);
    }

    /// #206: a line a tool radius clear of the part is quiet.
    #[test]
    fn open_line_far_from_shapes_is_quiet() {
        let p = square_and_lines(&[("l", line((43.5, 20.0), (43.5, 40.0)))]);
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::OnLine)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    /// #206: two open lines closer than the radius, or crossing, are two
    /// shapes whose cuts meet; each has its own line, so it is never
    /// compared with itself.
    #[test]
    fn open_lines_too_close_to_each_other_warn() {
        let close = square_and_lines(&[
            ("l", line((100.0, 20.0), (100.0, 60.0))),
            ("m", line((102.0, 20.0), (102.0, 60.0))),
        ]);
        let (_, warnings) = generate_profile(&close, &profile_op(&close, Side::OnLine)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        let (first, other) = named(found[0]);
        let mut pair = [first, other];
        pair.sort_unstable();
        assert_eq!(pair, ["l", "m"]);
        assert!((gap_mm(found[0]) - 2.0).abs() < TOL, "{:?}", found[0]);

        let crossing = square_and_lines(&[
            ("l", line((100.0, 20.0), (100.0, 60.0))),
            ("m", line((80.0, 40.0), (120.0, 40.0))),
        ]);
        let (_, warnings) =
            generate_profile(&crossing, &profile_op(&crossing, Side::OnLine)).unwrap();
        let found = overlaps(&warnings);
        assert_eq!(found.len(), 1, "{warnings:?}");
        assert!(gap_mm(found[0]).abs() < TOL, "{:?}", found[0]);
    }

    /// #206: a line in a part's material belongs to that part's region,
    /// as a hole does, so it is not compared with the part's own lines
    /// (a cut into the part, not a second shape beside it). A slot from
    /// the square's left edge touches it, which D6 would warn about with
    /// a gap of 0; a line alongside a wall is #210's band check.
    #[test]
    fn open_path_in_a_part_is_not_compared_with_its_lines() {
        // From the square's left edge (x = 20) into its middle.
        let p = square_and_lines(&[("l", line((20.0, 30.0), (30.0, 30.0)))]);
        let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::OnLine)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    /// An open polyline through `points`.
    fn open(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    /// #224: one open path in the waste whose stretches far apart along it
    /// come closer than the tool radius warns `toolpaths_overlap` against
    /// itself, as two separate paths there do: a U with legs 2 mm apart,
    /// and a square spiral whose turns are 2 mm apart, both on bare stock
    /// beside the part. `shape` and `other` are the same path and `gap_mm`
    /// the narrowest gap. Only the radius counts, as for two paths: a U
    /// with legs 4 mm apart (closer than the 6 mm bit, wider than its
    /// radius) is quiet.
    #[test]
    fn self_close_open_path_in_the_waste_warns() {
        let spiral = open(&[
            (100.0, 100.0),
            (130.0, 100.0),
            (130.0, 130.0),
            (100.0, 130.0),
            (100.0, 102.0),
            (128.0, 102.0),
            (128.0, 128.0),
            (102.0, 128.0),
        ]);
        let cases = [
            (
                "a 2 mm U",
                open(&[(100.0, 20.0), (100.0, 60.0), (102.0, 60.0), (102.0, 20.0)]),
            ),
            ("a 2 mm spiral", spiral),
        ];
        for (case, path) in cases {
            let p = square_and_lines(&[("l", path)]);
            let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::OnLine)).unwrap();
            let found = overlaps(&warnings);
            assert_eq!(found.len(), 1, "{case}: {warnings:?}");
            let w = found[0];
            assert_eq!(named(w), ("l", "l"), "{case}");
            assert_eq!(profiles(w), (0, 0), "{case}");
            assert!((needed_mm(w) - 3.0).abs() < TOL, "{case}");
            assert!((gap_mm(w) - 2.0).abs() < TOL, "{case}: {w:?}");
        }
        let wide = square_and_lines(&[(
            "l",
            open(&[(100.0, 20.0), (100.0, 60.0), (104.0, 60.0), (104.0, 20.0)]),
        )]);
        let (_, warnings) = generate_profile(&wide, &profile_op(&wide, Side::OnLine)).unwrap();
        assert!(overlaps(&warnings).is_empty(), "{warnings:?}");
    }

    /// #224: a smooth curve, an L and a 60° V in the waste never come
    /// within the radius of themselves away from their neighbouring
    /// stretches and corners, so they are quiet.
    #[test]
    fn open_curve_in_the_waste_does_not_warn_against_itself() {
        let arc: Vec<(f64, f64)> = (0..=90)
            .map(|k| {
                let a = f64::from(180 - 2 * k).to_radians();
                (150.0 + 15.0 * a.cos(), 100.0 + 15.0 * a.sin())
            })
            .collect();
        let half = 30_f64.to_radians();
        let cases = [
            ("a half circle", open(&arc)),
            ("an L", open(&[(100.0, 20.0), (100.0, 60.0), (140.0, 60.0)])),
            (
                "a 60° V",
                open(&[
                    (150.0 - 30.0 * half.sin(), 150.0 - 30.0 * half.cos()),
                    (150.0, 150.0),
                    (150.0 + 30.0 * half.sin(), 150.0 - 30.0 * half.cos()),
                ]),
            ),
        ];
        for (case, path) in cases {
            let p = square_and_lines(&[("l", path)]);
            let (_, warnings) = generate_profile(&p, &profile_op(&p, Side::OnLine)).unwrap();
            assert!(overlaps(&warnings).is_empty(), "{case}: {warnings:?}");
        }
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
