// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Bore operation checked on its emitted G-code (format 18; maintainer
//! decision 2026-10-24; functional spec acceptance 23a): the program is
//! parsed back into project space, independently of the generator, and the
//! hole the tool sweeps along it is measured. A 5 mm hole bored through
//! 11.8 mm stock with a 3.175 mm end mill keeps the tool centre within
//! 0.9125 mm of the hole's centre, falls at most the pitch a turn, ends
//! with a full circle at the depth, and comes out 5 mm across (±0.01 mm)
//! all the way through. Every bundled post-processor exports it through
//! the safety pass, and the quick-start plaque's screw holes come out the
//! size they are drawn.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

#[path = "support/heightfield.rs"]
mod heightfield;

use dc_cam::generate_bore;
use dc_gcode::{EmitOp, emit};
use dc_geometry::{Point, Polyline, Segment, Transform, point_segment_distance_mm};
use dc_project::test_support::sample_project;
use dc_project::{
    BoreOp, CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
    OperationId, Project, Stock, Tool, ToolId, ToolKind, latest_definitions,
};
use heightfield::Cut;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

const THICKNESS_MM: f64 = 11.8;
const TOOL_MM: f64 = 3.175;
const HOLE_MM: f64 = 5.0;
const PITCH_MM: f64 = 2.0;
/// 2.5 − 1.5875.
const HELIX_RADIUS_MM: f64 = 0.9125;
/// One unit in the last place of a millimetre program.
const QUANTUM_MM: f64 = 0.001;
const CENTRE: Point = Point { x: 40.0, y: 30.0 };
const DATE: &str = "2026-10-24";

/// A 80 × 60 × 11.8 mm project with one 5 mm circle at (40, 30), the
/// plaque's 3.175 mm end mill, and a Bore of it through the stock with
/// 0.2 mm through-cut allowance, climb, pitch from the 2 mm step-down.
fn five_mm_hole() -> (Project, BoreOp) {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 80.0,
        height_mm: 60.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    project.tools = vec![Tool {
        id: ToolId::new("t1"),
        name: "1/8 end mill".into(),
        vendor: None,
        product_id: None,
        number: Some(2),
        cutter: ToolKind::EndMill,
        diameter_mm: TOOL_MM,
        flutes: 2,
        feed_mm_min: Some(1000.0),
        plunge_mm_min: Some(300.0),
        rpm: Some(18000),
        step_down_mm: Some(PITCH_MM),
        step_over_fraction: Some(0.4),
    }];
    let definition = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Hole".into(),
        profiles: vec![Polyline::circle(Point::new(0.0, 0.0), HOLE_MM / 2.0)],
    };
    project.definitions = BTreeMap::from([(definition.id.clone(), definition)]);
    project.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: DefinitionId::new("d1"),
        transform: Transform::translate(CENTRE.x, CENTRE.y),
        name: "Hole".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(THICKNESS_MM),
        z_bottom_mm: Some(0.0),
    }];
    project.tabs = Vec::new();
    let op = BoreOp {
        id: OperationId::new("o1"),
        name: "Screw hole".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        depth_mm: Some(12.0),
        pitch_mm: None,
        direction: CutDirection::Climb,
        stock_to_leave_mm: 0.0,
        through_cut_allowance_mm: 0.2,
        safe_z_mm: 15.0,
    };
    project.operations = vec![Operation::Bore(op.clone())];
    (project, op)
}

/// The program `project` exports for its Bore `op` alone, through the
/// safety pass.
fn program(project: &Project, op: &BoreOp) -> String {
    let (toolpath, warnings) = generate_bore(project, op).unwrap_or_else(|e| panic!("{e}"));
    assert!(warnings.is_empty(), "{warnings:?}");
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .unwrap()
        .clone();
    let emit_op: EmitOp = (Operation::Bore(op.clone()), tool, toolpath).into();
    emit(project, &[emit_op], DATE)
        .unwrap_or_else(|e| panic!("the safety pass: {e}"))
        .0
        .text
}

/// The signed angle swept from `a` to `b` about `c`, in (−π, π].
fn turn(c: Point, a: Point, b: Point) -> f64 {
    let (u, v) = (a - c, b - c);
    (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y)
}

/// How far from `centre`, along the ray at `angle`, the cutter of radius
/// `radius_mm` cuts at or below `z_mm` along `cuts`: the boundary of the
/// swept area, found by bisection between `inner_mm`, which must be cut,
/// and `outer_mm`, which must not.
fn cut_reach_mm(
    cuts: &[Cut],
    centre: Point,
    angle: f64,
    radius_mm: f64,
    z_mm: f64,
    (inner_mm, outer_mm): (f64, f64),
) -> f64 {
    let segments: Vec<Segment> = cuts
        .iter()
        .flat_map(|c| c.samples.windows(2))
        .filter(|w| w[0].z <= z_mm + 1e-9 && w[1].z <= z_mm + 1e-9)
        .map(|w| Segment {
            start: w[0].xy(),
            end: w[1].xy(),
            bulge: 0.0,
        })
        .collect();
    let cut_at = |t: f64| {
        let p = centre + dc_geometry::Vec2::new(angle.cos(), angle.sin()) * t;
        segments
            .iter()
            .any(|s| point_segment_distance_mm(*s, p) <= radius_mm)
    };
    assert!(cut_at(inner_mm), "{inner_mm} mm out at {angle} is not cut");
    assert!(!cut_at(outer_mm), "{outer_mm} mm out at {angle} is cut");
    let (mut lo, mut hi) = (inner_mm, outer_mm);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if cut_at(mid) { lo = mid } else { hi = mid }
    }
    lo
}

/// The hole's diameter as cut at or below `z_mm`: twice the least and the
/// most reach over 360 rays, which every point inside is cut to.
fn hole_diameters_mm(cuts: &[Cut], centre: Point, z_mm: f64) -> (f64, f64) {
    let reaches: Vec<f64> = (0..360)
        .map(|k| {
            let angle = f64::from(k) * TAU / 360.0;
            cut_reach_mm(cuts, centre, angle, TOOL_MM / 2.0, z_mm, (0.0, 3.0))
        })
        .collect();
    let least = reaches.iter().copied().fold(f64::INFINITY, f64::min);
    let most = reaches.iter().copied().fold(0.0, f64::max);
    (2.0 * least, 2.0 * most)
}

/// Acceptance 23a: the program's helix stays within 0.9125 mm of the
/// centre (plus a unit in the last place), falls at most the 2 mm pitch
/// in each turn, starts 1 mm above the stock top, ends with a full circle
/// at Z −0.2 and a move to the centre, and the hole the tool sweeps is
/// 5 mm across (±0.01 mm) at every depth through the stock.
#[test]
fn five_mm_through_hole_program() {
    let (project, op) = five_mm_hole();
    let text = program(&project, &op);
    let cuts = heightfield::cuts(&project, &text, 0.005);
    assert!(cuts.len() > 100, "{} cuts", cuts.len());

    // Every cut stays within the helix radius of the centre.
    for c in &cuts {
        for s in &c.samples {
            let r = s.xy().distance_to(CENTRE);
            assert!(
                r <= HELIX_RADIUS_MM + QUANTUM_MM,
                "a cut reaches {r:.4} mm from the centre: {:?}",
                c.to
            );
        }
    }

    // The helix: from 1 mm above the top, at most a pitch a turn.
    let first = &cuts[0];
    assert!(
        (first.from.z - (THICKNESS_MM + 1.0)).abs() <= QUANTUM_MM,
        "the helix starts at {}",
        first.from.z
    );
    assert!(
        (first.from.xy().distance_to(CENTRE) - HELIX_RADIUS_MM).abs() <= QUANTUM_MM,
        "the helix starts on its circle"
    );
    let descending: Vec<&Cut> = cuts.iter().take_while(|c| c.to.z < c.from.z).collect();
    let mut points = vec![(0.0, first.from.z)];
    let mut swept = 0.0;
    for c in &descending {
        swept += turn(CENTRE, c.from.xy(), c.to.xy()).abs();
        points.push((swept, c.to.z));
    }
    for (i, &(a, z)) in points.iter().enumerate() {
        for &(b, w) in &points[i + 1..] {
            if b - a <= TAU + 1e-6 {
                assert!(
                    z - w <= PITCH_MM + 2.0 * QUANTUM_MM,
                    "the helix falls {} mm in one turn",
                    z - w
                );
            }
        }
    }
    let turns = swept / TAU;
    let expected = (THICKNESS_MM + 1.0 + 0.2) / PITCH_MM;
    assert!((turns - expected).abs() < 0.01, "{turns} turns");

    // The circle at the bottom, then the centre.
    let rest = &cuts[descending.len()..];
    assert_eq!(
        rest.len(),
        5,
        "four quarter arcs and the move to the centre"
    );
    let mut circle = 0.0;
    for c in &rest[..4] {
        let centre = c.centre.expect("the circle is written as arcs");
        assert!(centre.distance_to(CENTRE) <= QUANTUM_MM, "{centre:?}");
        assert!((c.from.z + 0.2).abs() <= QUANTUM_MM && (c.to.z + 0.2).abs() <= QUANTUM_MM);
        circle += turn(CENTRE, c.from.xy(), c.samples[c.samples.len() / 2].xy())
            + turn(CENTRE, c.samples[c.samples.len() / 2].xy(), c.to.xy());
    }
    assert!((circle - TAU).abs() < 1e-3, "the circle sweeps {circle}");
    assert!(rest[4].to.xy().distance_to(CENTRE) <= QUANTUM_MM);

    // The hole: 5 mm at the bottom of the stock, and no wider anywhere.
    for z_mm in [0.0, THICKNESS_MM] {
        let (least, most) = hole_diameters_mm(&cuts, CENTRE, z_mm);
        println!("at Z {z_mm}: {least:.4} to {most:.4} mm across");
        assert!(
            (least - HOLE_MM).abs() <= 0.01 && (most - HOLE_MM).abs() <= 0.01,
            "at Z {z_mm} the hole is {least:.4} to {most:.4} mm across"
        );
    }
}

/// Climb writes the circle as `G3` (counter-clockwise, Y-up), and
/// conventional as `G2`.
#[test]
fn climb_writes_g3_conventional_g2() {
    let (mut project, mut op) = five_mm_hole();
    for (direction, word, other) in [
        (CutDirection::Climb, "G3", "G2"),
        (CutDirection::Conventional, "G2", "G3"),
    ] {
        op.direction = direction;
        project.operations = vec![Operation::Bore(op.clone())];
        let text = program(&project, &op);
        let words: Vec<&str> = text
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        // `G2` and `G3` are modal: the first quarter names it, the other
        // three follow with their centre words alone.
        assert_eq!(
            words.iter().filter(|w| **w == word).count(),
            1,
            "{direction:?}:\n{text}"
        );
        assert!(!words.contains(&other), "{direction:?}:\n{text}");
        assert_eq!(
            text.lines().filter(|l| l.contains(" I")).count(),
            4,
            "{direction:?}:\n{text}"
        );
    }
}

/// Every bundled post-processor, at its latest version, exports the
/// 5 mm bore through its safety pass, in millimetres and in inches.
#[test]
fn exports_on_every_post() {
    let (mut project, op) = five_mm_hole();
    let definitions = latest_definitions().expect("the bundled definitions");
    assert!(definitions.len() >= 6, "{} definitions", definitions.len());
    for definition in definitions {
        for units in [dc_project::Units::Mm, dc_project::Units::Inch] {
            project.post_processor = definition.default_ref();
            project.display_units = units;
            let text = program(&project, &op);
            assert!(text.lines().count() > 100, "{:?}", definition.default_ref());
        }
    }
}

/// The quick-start plaque's "Screw holes" (`tests/fixtures/plaque.dcv`):
/// a Bore with the 3.175 mm end mill, each of its four 5 mm circles cut
/// 5 mm across (±0.01 mm) through the stock, from the program `export`
/// writes for it.
#[test]
fn plaque_screw_holes_are_cut_to_their_drawn_size() {
    let project = dc_project::load(include_bytes!("../../../tests/fixtures/plaque.dcv"))
        .expect("the plaque loads")
        .project;
    let op = project
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::Bore(b) if b.name == "Screw holes" => Some(b.clone()),
            _ => None,
        })
        .expect("the plaque's Screw holes are a Bore");
    let tool = project.tools.iter().find(|t| t.id == op.tool).unwrap();
    assert!(
        (tool.diameter_mm - TOOL_MM).abs() < 1e-12,
        "the 3.175 mm bit"
    );
    assert_eq!(op.targets.len(), 4);
    let text = program(&project, &op);
    let cuts = heightfield::cuts(&project, &text, 0.005);
    for target in &op.targets {
        let instance = project.instances.iter().find(|i| &i.id == target).unwrap();
        let circle = instance
            .transform
            .apply_polyline(&project.definitions[&instance.definition].profiles[0])
            .unwrap();
        let (centre, radius_mm) =
            dc_geometry::as_circle(&circle, project.flatten_tolerance_mm).expect("a circle");
        assert!(
            (2.0 * radius_mm - HOLE_MM).abs() < 1e-6,
            "{target}: drawn 5 mm"
        );
        let near: Vec<Cut> = cuts
            .iter()
            .filter(|c| {
                c.from.xy().distance_to(centre) < 2.0 && c.to.xy().distance_to(centre) < 2.0
            })
            .map(|c| Cut {
                from: c.from,
                to: c.to,
                feed: c.feed,
                radius_mm: c.radius_mm,
                centre: c.centre,
                samples: c.samples.clone(),
            })
            .collect();
        let (least, most) = hole_diameters_mm(&near, centre, 0.0);
        println!("{target}: {least:.4} to {most:.4} mm across");
        assert!(
            (least - HOLE_MM).abs() <= 0.01 && (most - HOLE_MM).abs() <= 0.01,
            "{target}: the hole is {least:.4} to {most:.4} mm across"
        );
    }
}
