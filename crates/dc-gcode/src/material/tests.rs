// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the S9 material model: segment geometry, the areas a cut may
//! reach, and that each lookup fails closed when the project does not
//! give what it needs.

mod stock_to_leave_band;

use super::*;
use crate::parse::{ArcWords, MachinePosition};
use dc_cam::Toolpath;
use dc_geometry::Vertex;
use dc_project::test_support::{profile_mut, sample_project, sample_tab};

/// Format 18: the farthest a cut comes from a Bore hole's centre is an
/// end of a line, or the arc's point straight away from the centre when
/// the arc passes it, so an arc that bulges between its ends and middle
/// is measured there.
#[test]
fn farthest_point_of_an_arc_is_where_it_bulges() {
    let centre = Point::new(0.0, 0.0);
    let line = Segment {
        start: Point::new(1.0, 0.0),
        end: Point::new(0.0, 2.0),
        bulge: 0.0,
    };
    assert!((farthest_mm(line, centre) - 2.0).abs() < 1e-12);
    // A counter-clockwise half circle about (1, 0), radius 1, from
    // (1, −1) to (1, 1): it passes (2, 0), 2 mm out.
    let half = Segment {
        start: Point::new(1.0, -1.0),
        end: Point::new(1.0, 1.0),
        bulge: 1.0,
    };
    assert!((farthest_mm(half, centre) - 2.0).abs() < 1e-9);
    // The other way round it passes (0, 0) and its ends are farthest.
    let back = Segment {
        start: Point::new(1.0, 1.0),
        end: Point::new(1.0, -1.0),
        bulge: 1.0,
    };
    assert!((farthest_mm(back, centre) - 2.0_f64.sqrt()).abs() < 1e-9);
    // About the centre itself an arc is its radius out everywhere.
    let round = Segment {
        start: Point::new(1.5, 0.0),
        end: Point::new(0.0, 1.5),
        bulge: (std::f64::consts::FRAC_PI_2 / 4.0).tan(),
    };
    assert!((farthest_mm(round, centre) - 1.5).abs() < 1e-9);
}

/// A 10 mm square at X 100, and a 30 mm square at the origin with a
/// 10 mm square hole in its middle (X and Y 10 to 20), as areas with
/// the waste inside.
fn square_and_frame() -> Vec<Area> {
    let contours = [
        Polyline::rect(Point::new(100.0, 0.0), 10.0, 10.0),
        Polyline::rect(Point::new(0.0, 0.0), 30.0, 30.0),
        Polyline::rect(Point::new(10.0, 10.0), 10.0, 10.0),
    ]
    .into_iter()
    .map(|p| Contour::new(p).unwrap())
    .collect();
    let (regions, _) = nest(contours).unwrap();
    areas_of(regions, true)
}

/// Review 1.1 (#711). `walls_allow` took a range of the area's edges
/// and read one outside them as no wall: the issue's
/// `walls_allow_refuses_a_scope_outside_the_area`, written against
/// that signature, failed (a cut through the wall was "clear" of the
/// edges `4..8` of a square). The range is gone, as every caller
/// meant the whole area, so the miss cannot be written: a cut is held
/// to every edge of the area, the outline's and each island's.
#[test]
fn walls_allow_holds_a_cut_to_every_edge_of_the_area() {
    let areas = square_and_frame();
    let frame = areas
        .iter()
        .find(|a| a.loops.len() == 2)
        .expect("the frame");
    let level = |from: (f64, f64), to: (f64, f64)| Segment {
        start: Point::new(from.0, from.1),
        end: Point::new(to.0, to.1),
        bulge: 0.0,
    };
    // In the band, 5 mm from the outline and from the island.
    assert!(walls_allow(
        frame,
        level((5.0, 5.0), (25.0, 5.0)),
        1.0,
        None
    ));
    // Through the outline, the first loop.
    assert!(!walls_allow(
        frame,
        level((5.0, 5.0), (35.0, 5.0)),
        1.0,
        None
    ));
    // Into the island, the second loop.
    assert!(!walls_allow(
        frame,
        level((5.0, 15.0), (15.0, 15.0)),
        1.0,
        None
    ));
}

/// #711: each loop holds its own edges, in order, and together they
/// are the area's, so the lap and On line rules read a loop's edges
/// without a range into the area's that could miss.
#[test]
fn each_loop_owns_its_edges() {
    for area in square_and_frame() {
        let owned: Vec<Segment> = area
            .loops
            .iter()
            .flat_map(|l| l.edges.iter().map(|e| e.segment))
            .collect();
        let all: Vec<Segment> = area.edges.iter().map(|e| e.segment).collect();
        assert_eq!(owned, all);
        assert!(area.loops.iter().all(|l| l.edges.len() == 4));
    }
}

/// #711: an area with no edge has no bounds to be far from. A point
/// at the origin stood in for them, and an outside Profile's cut
/// inside a part elsewhere passed that part by; with bounds that are
/// everywhere, the part is looked at, and the cut is in it.
#[test]
fn outside_regions_fails_closed_on_an_area_without_bounds() {
    let part = Contour::new(Polyline::rect(Point::new(500.0, 500.0), 10.0, 10.0)).unwrap();
    let (regions, _) = nest(vec![part]).unwrap();
    let region = regions.into_iter().next().unwrap();
    let area = |bounds: Rect| Area {
        region: region.clone(),
        edges: Vec::new(),
        corners: Vec::new(),
        loops: Vec::new(),
        bounds,
    };
    let inside = Segment {
        start: Point::new(504.0, 505.0),
        end: Point::new(506.0, 505.0),
        bulge: 0.0,
    };
    let at_origin = area(Rect::from_point(Point::new(0.0, 0.0)));
    assert!(outside_regions(&[at_origin], inside, 1.0, None));
    assert!(!outside_regions(&[area(EVERYWHERE)], inside, 1.0, None));
}

/// The sample project's Profile, its tool, and the machine transform.
fn sample() -> (Project, Operation, Tool, MachineTransform) {
    let project = sample_project();
    let op = project.operations.first().cloned().unwrap();
    let tool = project.tools.first().cloned().unwrap();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    (project, op, tool, transform)
}

/// #711: an inside or outside Profile is held to the trench rule,
/// which needs the stock top. Without a thickness the rule was left
/// out and the Profile held without it; now it has no material.
#[test]
fn material_of_fails_closed_on_a_profile_without_a_stock_top() {
    let (mut project, _, _, _) = sample();
    for side in [Side::Outside, Side::Inside] {
        project.stock.thickness_mm = Some(6.0);
        profile_mut(&mut project).side = side;
        let op = project.operations.first().cloned().unwrap();
        assert!(
            matches!(
                Material::of(&project, &op),
                Material::Outside { .. }
                    | Material::Inside {
                        trench: Some(_),
                        ..
                    }
            ),
            "{side:?}"
        );
        project.stock.thickness_mm = None;
        assert!(
            matches!(Material::of(&project, &op), Material::Unresolved),
            "{side:?}"
        );
    }
}

/// #711: the tab rule tells a straight plunge by where a cut starts
/// and ends. A cut whose start is not known cannot be told from one,
/// so in a span with tabs it does not keep them (it passed before).
#[test]
fn keeps_tabs_fails_closed_on_an_unknown_position() {
    let (project, op, tool, transform) = sample();
    let tabs = tab_spines(&project, &op).unwrap();
    assert_eq!(tabs.len(), 4);
    let material = Material::of(&project, &op);
    let toolpath = Toolpath::default();
    let span = |tabs| OpSpan {
        op: &op,
        tool: &tool,
        toolpath: &toolpath,
        material: &material,
        tabs,
        lines: 1..2,
    };
    // A plunge to the stock bottom in a corner, far from every tab.
    let known = cut(
        &transform,
        1,
        Point3::new(90.0, 90.0, 11.0),
        Point3::new(90.0, 90.0, 0.0),
    );
    assert!(keeps_tabs(&known, &transform, &span(&tabs), 0.01));
    for axis in 0..3 {
        let mut unknown = known;
        match axis {
            0 => unknown.from.x = None,
            1 => unknown.from.y = None,
            _ => unknown.from.z = None,
        }
        assert!(!keeps_tabs(&unknown, &transform, &span(&tabs), 0.01));
        // A span with no tab has none to keep.
        assert!(keeps_tabs(&unknown, &transform, &span(&[]), 0.01));
    }
}

/// #711: the trench rule tells a ramp by where a cut starts and ends.
/// A cut whose start is not known cannot be told from one, so it is
/// not in the trench (it passed before).
#[test]
fn trench_allows_fails_closed_on_an_unknown_position() {
    let (_, _, _, transform) = sample();
    let rule = TrenchRule {
        stock_top_mm: TOP_MM,
        stock_to_leave_mm: 0.0,
    };
    let known = cut(
        &transform,
        1,
        Point3::new(0.0, 0.0, TOP_MM + 0.5),
        Point3::new(20.0, 0.0, TOP_MM - 0.015),
    );
    assert!(Trench::new(rule, &[], RADIUS_MM).allows(&known, &transform, 0.01));
    let mut unknown = known;
    unknown.from.z = None;
    assert!(!Trench::new(rule, &[], RADIUS_MM).allows(&unknown, &transform, 0.01));
}

/// #711: a tab request the layout rule cannot place (here a width
/// below 0, which the generator refuses) left the Profile with no
/// tab to keep. Now the tabs do not resolve, and the run has no
/// material.
#[test]
fn tab_spines_fails_closed_on_a_request_that_cannot_be_placed() {
    let (mut project, _, tool, _) = sample();
    profile_mut(&mut project).tabs.as_mut().unwrap().width_mm = -1.0;
    let op = project.operations.first().cloned().unwrap();
    assert!(tab_spines(&project, &op).is_none());
    let (material, tabs) = Material::held_run(&project, &op, &RunLabel::Main, &tool, Board::Female);
    assert!(matches!(material, Material::Unresolved), "{material:?}");
    assert!(tabs.is_empty());
}

/// #711: a Profile with a tab on its target but no tool, no depth or
/// no stock thickness to build the tab's spine from kept no tab. Now
/// its tabs do not resolve. With no tab on its targets there is
/// nothing to resolve.
#[test]
fn tab_spines_fails_closed_on_a_missing_tool_or_floor() {
    let (mut project, _, _, _) = sample();
    profile_mut(&mut project).tabs = None;
    project.tabs = vec![sample_tab("tab1", 25.0, 0.0)];
    let spines =
        |project: &Project| tab_spines(project, &project.operations.first().cloned().unwrap());
    assert_eq!(spines(&project).map(|t| t.len()), Some(1));

    let mut no_tool = project.clone();
    profile_mut(&mut no_tool).tool = ToolId::new("gone");
    assert!(spines(&no_tool).is_none());
    let mut no_depth = project.clone();
    profile_mut(&mut no_depth).depth_mm = None;
    assert!(spines(&no_depth).is_none());
    let mut no_thickness = project.clone();
    no_thickness.stock.thickness_mm = None;
    assert!(spines(&no_thickness).is_none());

    no_tool.tabs.clear();
    assert_eq!(spines(&no_tool).map(|t| t.len()), Some(0));
}

/// #711: a placed tab whose outline is not in its definition, or
/// whose instance is gone, was dropped, and the Profile held to the
/// tabs that were left. Now the tabs do not resolve.
#[test]
fn tab_spine_fails_closed_on_an_outline_that_does_not_resolve() {
    let (mut project, _, _, _) = sample();
    profile_mut(&mut project).tabs = None;
    let spines =
        |project: &Project| tab_spines(project, &project.operations.first().cloned().unwrap());
    let mut no_profile = project.clone();
    no_profile.tabs = vec![PlacedTab {
        profile: 7,
        ..sample_tab("tab1", 25.0, 0.0)
    }];
    assert!(spines(&no_profile).is_none());

    let mut no_instance = project.clone();
    no_instance.tabs = vec![PlacedTab {
        instance: InstanceId::new("gone"),
        ..sample_tab("tab1", 25.0, 0.0)
    }];
    // A tab of another instance is none of this Profile's.
    assert_eq!(spines(&no_instance).map(|t| t.len()), Some(0));
    profile_mut(&mut no_instance)
        .targets
        .push(InstanceId::new("gone"));
    assert!(spines(&no_instance).is_none());
}

/// A tab on an open outline has a spine for an On line Profile, which
/// cuts along it (#175): the outline and its open offsets by the tool
/// radius either side. A Left or Right Profile's has one too
/// (`tab_spine_on_an_open_path_of_a_left_or_right_profile_is_on_its_cut_side`).
/// An inside or outside Profile does not cut an open outline. A
/// closed outline that is no contour (a bow tie) leaves an On line
/// Profile's tabs unresolved; a Left Profile, which cuts open paths
/// only, has no tab on it.
#[test]
fn tab_spine_on_an_open_outline_is_an_on_line_profiles() {
    let (mut project, _, _, _) = sample();
    profile_mut(&mut project).tabs = None;
    project.tabs = vec![sample_tab("tab1", 20.0, 0.0)];
    let spines = |project: &Project, side: Side| {
        let mut project = project.clone();
        profile_mut(&mut project).side = side;
        tab_spines(&project, &project.operations.first().cloned().unwrap())
    };
    let outline = |project: &mut Project, vertices: &[(f64, f64)], closed: bool| {
        let vertices = vertices.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        project
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .unwrap()
            .profiles = vec![Polyline::new(vertices, closed)];
    };

    let mut open = project.clone();
    outline(&mut open, &[(0.0, 0.0), (40.0, 0.0)], false);
    let on_line = spines(&open, Side::OnLine).unwrap();
    assert_eq!(on_line.len(), 1);
    let spine = on_line.first().unwrap();
    assert_eq!(spine.paths.len(), 3, "the line and an offset either side");
    assert!(spine.paths.iter().all(|p| !p.is_closed()));
    assert!(spine.laps.is_empty());
    for side in [Side::Left, Side::Right] {
        assert_eq!(spines(&open, side).map(|t| t.len()), Some(1), "{side:?}");
    }
    for side in [Side::Inside, Side::Outside] {
        assert_eq!(spines(&open, side).map(|t| t.len()), Some(0), "{side:?}");
    }

    let mut bow_tie = project.clone();
    outline(
        &mut bow_tie,
        &[(0.0, 0.0), (40.0, 20.0), (40.0, 0.0), (0.0, 20.0)],
        true,
    );
    assert!(spines(&bow_tie, Side::OnLine).is_none());
    assert_eq!(spines(&bow_tie, Side::Left).map(|t| t.len()), Some(0));
}

/// A tab on an open path of a Left or Right Profile (#207) is measured
/// as the generator lays it: along the path offset by the tool radius
/// to the cut side, from the point of that offset nearest the tab. The
/// sample's 6.35 mm bit and a 40 mm line from (10, 10) in project
/// space, with one smooth tab 6 mm wide and 2 mm tall asked for at
/// distance 0: cut Left the spine's one path is the line 3.175 mm to
/// its left (Y 13.175), Right to its right (Y 6.825), with no path on
/// the line or on the other side, and with a 0.5 mm last pass stock to
/// leave its stock path is 3.675 mm off. The tab is centred on the
/// offset's first point, where its request puts it. The top is held
/// for the radius along the offset from there, then slopes to the
/// floor over half the tab's width: a cut at the centre or 2.5 mm
/// along may not go below the top; one 5.175 mm along, 2 mm down the
/// 3 mm slope and the margin (0.1 of the radius) further, may go to
/// 0.455 mm above the floor and no lower; one 6.5 mm along is clear.
/// A cut on the line itself or on the other side is on no path of
/// the tab's.
#[test]
fn tab_spine_on_an_open_path_of_a_left_or_right_profile_is_on_its_cut_side() {
    const R_MM: f64 = 6.35 / 2.0;
    let (mut project, _, _, _) = sample();
    project
        .definitions
        .get_mut(&DefinitionId::new("d1"))
        .unwrap()
        .profiles = vec![Polyline::new(
        vec![Vertex::line(0.0, 0.0), Vertex::line(40.0, 0.0)],
        false,
    )];
    {
        let op = profile_mut(&mut project);
        op.direction = dc_project::CutDirection::Conventional;
        let tabs = op.tabs.as_mut().unwrap();
        tabs.placement = dc_project::TabPlacement::AtDistances {
            distances_mm: vec![0.0],
        };
        tabs.shape = TabShape::Smooth;
    }
    let spines = |project: &Project, side: Side| {
        let mut project = project.clone();
        profile_mut(&mut project).side = side;
        tab_spines(&project, &project.operations.first().cloned().unwrap()).unwrap()
    };
    let line_at = |path: &Polyline, y_mm: f64| {
        let ends: Vec<Point> = path.vertices().iter().map(|v| v.point()).collect();
        assert_eq!(ends.len(), 2, "{path:?}");
        assert!(!path.is_closed());
        for (end, x_mm) in ends.iter().zip([10.0, 50.0]) {
            assert!(
                end.distance_to(Point::new(x_mm, y_mm)) < 1e-9,
                "{end:?} is not ({x_mm}, {y_mm})"
            );
        }
    };
    for (side, sign) in [(Side::Left, 1.0), (Side::Right, -1.0)] {
        let found = spines(&project, side);
        assert_eq!(found.len(), 1, "{side:?}");
        let spine = found.first().unwrap();
        let y_mm = 10.0 + sign * R_MM;
        assert_eq!(
            spine.paths.len(),
            1,
            "{side:?}: the cut side's offset alone"
        );
        line_at(&spine.paths[0], y_mm);
        assert_eq!(spine.laps, spine.paths);
        assert!(spine.stock_paths.is_empty());
        assert!(
            spine.point.distance_to(Point::new(10.0, y_mm)) < 1e-9,
            "{side:?}: the tab is centred at {:?}",
            spine.point
        );

        let cuts = |x_mm: f64, y_mm: f64, z_mm: f64| {
            spine.cuts_into(Point::new(x_mm, y_mm), z_mm, 0.01, &found)
        };
        // The top is held for the radius from the centre, less the
        // margin.
        for along_mm in [0.0, 2.5] {
            assert!(cuts(10.0 + along_mm, y_mm, 1.9), "{side:?}, {along_mm} mm");
            assert!(!cuts(10.0 + along_mm, y_mm, 2.0), "{side:?}, {along_mm} mm");
        }
        // On the slope: (5.175 + 0.3175 − 3.175) / 3 of the way down.
        let allowed_mm = 2.0 * (1.0 - (2.0 + 0.1 * R_MM) / 3.0);
        assert!((allowed_mm - 0.455).abs() < 1e-3);
        assert!(cuts(15.175, y_mm, allowed_mm - 0.02), "{side:?}");
        assert!(!cuts(15.175, y_mm, allowed_mm), "{side:?}");
        // Clear of the span, and off the tab's path.
        assert!(!cuts(16.5, y_mm, 0.0), "{side:?}");
        assert!(!cuts(10.0, 10.0, 0.0), "{side:?}: on the line");
        assert!(
            !cuts(10.0, 10.0 - sign * R_MM, 0.0),
            "{side:?}: the other side"
        );

        let mut with_last_pass = project.clone();
        profile_mut(&mut with_last_pass).last_pass = Some(dc_project::LastPass {
            stock_to_leave_mm: 0.5,
            thickness_mm: None,
            reverse: false,
        });
        let found = spines(&with_last_pass, side);
        let spine = found.first().unwrap();
        assert_eq!(spine.paths.len(), 1);
        assert_eq!(spine.stock_paths.len(), 1);
        line_at(&spine.stock_paths[0], 10.0 + sign * (R_MM + 0.5));
    }
}

/// A tab's span on an open path runs along it only. A rect tab 4 mm
/// wide for a 1.5 mm bit holds 2.75 mm either side of its centre. On a
/// 20 mm open line with the tab 1 mm from its start, a plunge below
/// the tab's top 0.5 mm from the far end is 18.5 mm along from the
/// tab, clear of it; measured round, as on a closed path, it would be
/// 1.5 mm from it. A plunge 3 mm along is in the span, one 5 mm along
/// is clear, and one off the line is not on the path.
#[test]
fn tab_span_does_not_wrap_round_an_open_path() {
    let line = Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(20.0, 0.0)], false);
    let spine = TabSpine {
        outline_id: (InstanceId::new("i1"), 0),
        paths: vec![line],
        laps: Vec::new(),
        stock_paths: Vec::new(),
        point: Point::new(1.0, 0.0),
        hold_mm: 2.75,
        slope_mm: 0.0,
        margin_mm: 0.075,
        shape: TabShape::Rect,
        floor_z_mm: 0.0,
        reach_mm: 2.75,
        radius_mm: 0.75,
        top_z_mm: 2.0,
    };
    let cuts =
        |x: f64, y: f64| spine.cuts_into(Point::new(x, y), 0.0, 0.01, std::slice::from_ref(&spine));
    assert!(cuts(1.0, 0.0));
    assert!(cuts(3.0, 0.0));
    assert!(!cuts(5.0, 0.0));
    assert!(!cuts(19.5, 0.0));
    assert!(!cuts(1.0, 1.0));
    // The same line closed (back along itself) wraps: 19.5 is 1.5
    // from the tab round its start.
    let closed = TabSpine {
        paths: vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(20.0, 0.0),
                Vertex::line(20.0, 0.5),
                Vertex::line(0.0, 0.5),
            ],
            true,
        )],
        ..spine.clone()
    };
    assert!(closed.cuts_into(
        Point::new(0.0, 0.5),
        0.0,
        0.01,
        std::slice::from_ref(&closed)
    ));
}

/// #711: a path of a tab's spine with no length has no nearest point,
/// so a plunge cannot be measured along it. That read as "not on this
/// path"; now the plunge goes below the tab. The same for a lap the
/// tab cannot be found on.
#[test]
fn cuts_into_fails_closed_on_a_path_without_length() {
    let (project, op, _, _) = sample();
    let spines = tab_spines(&project, &op).unwrap();
    let spine = spines.first().cloned().unwrap();
    // A corner of the stock, far from every tab.
    let far = Point::new(90.0, 90.0);
    assert!(!spine.cuts_into(far, 0.0, 0.01, &spines));
    assert!(!spine.cuts_material(far, 0.0, 0.01, &spines));

    let dot = Polyline::new(vec![Vertex::line(1.0, 1.0), Vertex::line(1.0, 1.0)], false);
    assert!(nearest_point(&dot, far).is_none());
    let mut on_path = spine.clone();
    on_path.paths.push(dot.clone());
    assert!(on_path.cuts_into(far, 0.0, 0.01, std::slice::from_ref(&on_path)));
    let mut on_lap = spine;
    on_lap.laps = vec![dot];
    assert!(on_lap.cuts_material(far, 0.0, 0.01, std::slice::from_ref(&on_lap)));
}

const TOP_MM: f64 = 20.0;
const RADIUS_MM: f64 = 3.6;

/// The cut from `from` to `to`, project space, written in mm on line
/// `line`.
fn cut(transform: &MachineTransform, line: usize, from: Point3, to: Point3) -> MachineMove {
    let at = |p: Point3| {
        let m = transform.to_machine(p);
        MachinePosition {
            x: Some(m.x),
            y: Some(m.y),
            z: Some(m.z),
        }
    };
    MachineMove {
        line,
        kind: MotionKind::Linear,
        from: at(from),
        to: at(to),
        feed: Some(1000.0),
        units: Units::Mm,
        arc: ArcWords::default(),
    }
}

/// #356: a first pass's ramp leg from 0.5 mm above the stock top to
/// 0.015 mm below it, 20 mm along X, then a later pass's ramp leg back
/// from its end, from Z `TOP_MM` − 0.1 to X `back_to_x`: whether the
/// trench rule allows that later leg, with `arc_fit_mm`. Only the last
/// 0.58 mm of the first leg (X 19.42 to 20) is below the stock top.
fn later_leg_allowed(back_to_x: f64, arc_fit_mm: Option<f64>) -> bool {
    let project = sample_project();
    let post = project.post_processor.resolve().unwrap();
    let transform = MachineTransform::new(&project, &post).unwrap();
    let tolerances = Tolerances {
        flatten_mm: project.flatten_tolerance_mm,
        arc_fit_mm,
    };
    let slack = slack_mm(&post, Units::Mm, tolerances);
    let rule = TrenchRule {
        stock_top_mm: TOP_MM,
        stock_to_leave_mm: 0.0,
    };
    let mut trench = Trench::new(rule, &[], RADIUS_MM);
    let first = cut(
        &transform,
        1,
        Point3::new(0.0, 0.0, TOP_MM + 0.5),
        Point3::new(20.0, 0.0, TOP_MM - 0.015),
    );
    assert!(trench.allows(&first, &transform, slack), "a fresh ramp");
    trench.lift();
    let later = cut(
        &transform,
        2,
        Point3::new(20.0, 0.0, TOP_MM - 0.1),
        Point3::new(back_to_x, 0.0, TOP_MM - 0.2),
    );
    trench.allows(&later, &transform, slack)
}

/// #356: a ramp leg above the stock top but for its last 0.015 mm is
/// trench only along that last part. A later ramp leg within that part
/// is allowed; one that runs past it, along the part above the top,
/// fails S9, with arc fitting off and on alike.
#[test]
fn only_the_part_of_a_cut_below_the_top_is_trench() {
    for arc_fit_mm in [None, Some(0.01)] {
        assert!(later_leg_allowed(19.6, arc_fit_mm), "{arc_fit_mm:?}");
        assert!(!later_leg_allowed(10.0, arc_fit_mm), "{arc_fit_mm:?}");
        assert!(!later_leg_allowed(19.2, arc_fit_mm), "{arc_fit_mm:?}");
    }
}
