// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests that a carve reaches every convex corner of its regions (#456;
//! `docs/specs/phase-3/design.md`, "Algorithms → Medial axis", "Corner
//! reach"). Each reads the programs as a heightfield does
//! ([`crate::heightfield::carved_z_mm`]): a corner is reached within `δ`
//! when some move's cone comes within `δ` of it at the start plane.

use super::*;
use crate::heightfield::carved_z_mm;
use dc_geometry::medial::convex_corners;
use dc_project::{InlayOp, InlaySide};

/// How near a carve must come to each corner, mm (#456).
const REACH_MM: f64 = 0.01;

/// The least a vertex must turn to count as a corner: 20°. A drawing's
/// curves, flattened within the project's tolerance, turn less at each
/// vertex; how near the carve comes to the walls between corners is #457's.
const MIN_TURN_RAD: f64 = 20.0 * std::f64::consts::PI / 180.0;

/// The convex corners of `regions` that `toolpath`, cut with a V-bit of
/// `slope` from the start plane `start_z_mm`, misses by more than
/// [`REACH_MM`], each with how far, mm; and how many corners there are.
fn missed_corners(
    toolpath: &Toolpath,
    regions: &[Region],
    start_z_mm: f64,
    slope: f64,
    tolerance_mm: f64,
) -> (Vec<(Point, f64)>, usize) {
    let corners: Vec<Point> = regions
        .iter()
        .flat_map(|r| convex_corners(r, MIN_TURN_RAD, tolerance_mm))
        .map(|c| c.p)
        .collect();
    let missed = corners
        .iter()
        .map(|&p| (p, (carved_z_mm(toolpath, p, slope) - start_z_mm) * slope))
        .filter(|&(_, gap_mm)| gap_mm > REACH_MM)
        .collect();
    (missed, corners.len())
}

/// The sign's letter `name` (`tests/fixtures/sign.dcv`) raised in a
/// rectangle 4 mm round its box: the rectangle and the letter carved
/// together with the sign's 90° V-bit, a 1.5 mm flat depth, as the sign's
/// background is round its raised oval.
fn raised_letter(name: &str) -> (Project, VCarveOp) {
    let mut sign = dc_project::load(include_bytes!("../../../../../tests/fixtures/sign.dcv"))
        .expect("the sign loads")
        .project;
    let letter = sign
        .instances
        .iter()
        .find(|i| i.name == name)
        .cloned()
        .expect("the sign has the letter");
    let mut bounds: Option<dc_geometry::Rect> = None;
    for profile in &sign.definitions[&letter.definition].profiles {
        let placed = letter
            .transform
            .apply_polyline(profile)
            .expect("the letter places");
        let b = placed.bounds().expect("a profile has a box");
        bounds = Some(bounds.map_or(b, |all| all.including(b.min).including(b.max)));
    }
    let b = bounds.expect("the letter has a profile");
    let frame = FeatureDefinition {
        id: DefinitionId::new("frame"),
        name: "Frame".into(),
        profiles: vec![Polyline::rect(
            Point::new(b.min.x - 4.0, b.min.y - 4.0),
            b.width_mm() + 8.0,
            b.height_mm() + 8.0,
        )],
    };
    let frame_instance = FeatureInstance {
        id: InstanceId::new("frame"),
        definition: frame.id.clone(),
        transform: Transform::translate(0.0, 0.0),
        name: "Frame".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    };
    sign.definitions.insert(frame.id.clone(), frame);
    sign.instances = vec![frame_instance, letter.clone()];
    sign.operations.clear();
    let op = VCarveOp {
        targets: vec![InstanceId::new("frame"), letter.id],
        tool: ToolId::new("t4"),
        flat_depth_mm: Some(1.5),
        step_over_fraction: Some(0.4),
        ..sample_v_carve()
    };
    (sign, op)
}

/// #456: a V-Carve round raised serif lettering cuts every convex corner
/// of its regions crisp, within [`REACH_MM`]: where a serif meets its
/// stem, the notches between a letter's arms, the frame's corners. Each of
/// the sign's letters stands in a rectangle, carved with the sign's 90°
/// bit and a flat depth. Before #456 a V-Carve kept upstream's medial
/// axis, which seeks a hole's corner through the hole and so leaves the
/// notches of a raised letter with no branch (#452).
#[test]
fn raised_text_corners_are_crisp() {
    let mut failures = Vec::new();
    for name in [
        "letter-D", "letter-r", "letter-e", "letter-a", "letter-m", "letter-C", "letter-v",
    ] {
        let (p, op) = raised_letter(name);
        let (toolpath, _) = carve(&p, &op);
        let regions = regions_of(&with_op(&p, &op), &op);
        let (missed, count) = missed_corners(
            &toolpath,
            &regions,
            p.stock.thickness_mm.expect("a thickness"),
            slope_of(90.0),
            p.flatten_tolerance_mm,
        );
        if !missed.is_empty() {
            failures.push(format!("{name}: {missed:.3?} of {count}"));
        }
    }
    assert!(failures.is_empty(), "corners missed: {failures:#?}");
}

/// The tutorial female (`tests/fixtures/dc-female.dcv`), and with its
/// values a ring: a regular octagon 40 mm across its corners, whose 135°
/// corners turn less than upstream's 50° tip angle, round a 20 mm square
/// island with a 23° notch 8 mm deep in its top edge, the serif M's notch
/// of #452 as a convex corner of the pocket at its island.
fn females() -> Vec<(&'static str, Project, InlayOp)> {
    let load = |bytes: &[u8]| dc_project::load(bytes).expect("the fixture loads").project;
    let inlay_of = |p: &Project| {
        p.operations
            .iter()
            .find_map(|o| match o {
                Operation::Inlay(op) => Some(op.clone()),
                _ => None,
            })
            .expect("the fixture has an Inlay")
    };
    let dc = load(include_bytes!(
        "../../../../../tests/fixtures/dc-female.dcv"
    ));
    let dc_op = inlay_of(&dc);
    let mut ring = dc.clone();
    let octagon: Vec<(f64, f64)> = (0..8)
        .map(|k| {
            let angle = std::f64::consts::FRAC_PI_4 * f64::from(k);
            (20.0 * angle.cos(), 20.0 * angle.sin())
        })
        .collect();
    let notch_mm = 8.0 * 11.5_f64.to_radians().tan();
    let design = FeatureDefinition {
        id: DefinitionId::new("ring"),
        name: "Ring".into(),
        profiles: vec![
            polygon(&octagon),
            polygon(&[
                (-10.0, -10.0),
                (10.0, -10.0),
                (10.0, 10.0),
                (notch_mm, 10.0),
                (0.0, 2.0),
                (-notch_mm, 10.0),
                (-10.0, 10.0),
            ]),
        ],
    };
    ring.definitions = BTreeMap::from([(design.id.clone(), design)]);
    ring.instances = vec![FeatureInstance {
        id: InstanceId::new("ring"),
        definition: DefinitionId::new("ring"),
        transform: Transform::translate(55.0, 40.0),
        name: "Ring".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    let ring_op = InlayOp {
        targets: vec![InstanceId::new("ring")],
        ..dc_op.clone()
    };
    ring.operations = vec![Operation::Inlay(ring_op.clone())];
    vec![("DC", dc, dc_op), ("ring", ring, ring_op)]
}

/// #456: a female Inlay cuts every convex corner of its pocket crisp,
/// within [`REACH_MM`], on the tutorial "DC" and on the ring, whose
/// octagon's corners and island's notch upstream's medial axis leaves
/// blunt. The pocket's walls are the
/// plug's mirror image, so a corner the female blunts leaves wood the
/// plug's corner meets.
#[test]
fn female_corners_reach_the_tip() {
    let mut failures = Vec::new();
    for (name, p, op) in females() {
        assert_eq!(op.side, InlaySide::Female, "{name}");
        let (runs, _) = crate::inlay::generate_inlay(&p, &op).expect("the female generates");
        let run = runs
            .iter()
            .find(|r| r.label == RunLabel::Main)
            .expect("a V-bit run");
        let tool = p
            .tools
            .iter()
            .find(|t| t.id == run.tool)
            .expect("the run's tool");
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            panic!("{name}: the main run is a V-bit's");
        };
        let side = crate::inlay::side(&p, &op, &mut Vec::new()).expect("the female resolves");
        let (missed, count) = missed_corners(
            &run.toolpath,
            &side.targets.regions,
            side.start_plane_z_mm,
            slope_of(included_angle_deg),
            p.flatten_tolerance_mm,
        );
        if !missed.is_empty() {
            failures.push(format!("{name}: {missed:.3?} of {count}"));
        }
    }
    assert!(failures.is_empty(), "corners missed: {failures:#?}");
}
