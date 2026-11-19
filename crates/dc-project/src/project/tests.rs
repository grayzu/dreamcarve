// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the whole-project geometry questions: the start plane an
//! earlier operation leaves cleared.

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

/// Slice 27c (#679): a V-Carve cut with a tapered ball nose has its
/// floor at the bit's half-width at the flat depth, `w(F)`, not `F ·
/// tan β`. The Fine bit carving the sample rectangle `i1` (x 10..60)
/// 2.5 mm flat has its floor from x 10 + `w(2.5)` = 11.310: a 5 mm
/// square from x 11.36 is on it (a V-bit of the same taper would put
/// the floor's edge at 10.294, and a 90° V-bit's at 12.5), one from
/// x 11.26 is over the slope.
#[test]
fn tapered_v_carve_floor_clears_its_start_plane() {
    let floor_edge_mm = 10.0 + (2.5 + 8.6538) * 6.7_f64.to_radians().tan();
    for (x, expected) in [
        (floor_edge_mm + 0.05, Some(("v1", Some(2.5)))),
        (floor_edge_mm - 0.05, None),
    ] {
        let mut p = shapes();
        p.tools.push(crate::model::Tool {
            id: crate::ids::ToolId::new("t9"),
            ..crate::test_support::tool_tbn_fine()
        });
        place_rect(&mut p, "edge", x, 15.0, 5.0, 5.0);
        let Operation::VCarve(mut tapered) = floored(&["i1"], Some(2.5), 0.0) else {
            unreachable!("a V-Carve")
        };
        tapered.tool = crate::ids::ToolId::new("t9");
        p.operations = vec![Operation::VCarve(tapered), carve(&["edge"])];
        assert_eq!(plane(&p), expected, "a square from x {x}");
    }
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
