// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the flat floor and the V-bit floor (#405;
//! `docs/specs/phase-3/design.md`, "V-Carve generator", step 6, and "Test
//! plan → dc-cam — vcarve (flat floor and clearing)"; functional
//! capability 39, acceptance 50).
//!
//! The floor blocks are the ones with a cleared area: medial runs have
//! none.

use super::*;
use dc_geometry::{Segment, point_segment_distance_mm};

/// Acceptance 50's flat depth, mm.
pub(super) const F_MM: f64 = 2.0;

/// Acceptance 50: a 30 × 10 mm rectangle, the 90° 6.35 mm bit (whose
/// `step_over_fraction` is 0.4), `flat_depth_mm: 2`, and the operation's
/// own `step_over_fraction`.
pub(super) fn acceptance_50(step_over_fraction: Option<f64>) -> (Project, VCarveOp) {
    with_90(vec![rect(30.0, 10.0)], step_over_fraction)
}

/// `profiles` carved with the 90° bit at flat depth [`F_MM`].
pub(super) fn with_90(
    profiles: Vec<Polyline>,
    step_over_fraction: Option<f64>,
) -> (Project, VCarveOp) {
    let mut p = project(profiles);
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(v_bit(90.0));
    let op = VCarveOp {
        flat_depth_mm: Some(F_MM),
        step_over_fraction,
        ..carve_op(&p)
    };
    (p, op)
}

/// The floor Z of a carve from the stock top.
pub(super) const FLOOR_Z_MM: f64 = TOP_MM - F_MM;

/// The XY cuts at the floor: both ends at [`FLOOR_Z_MM`].
pub(super) fn floor_cuts(toolpath: &Toolpath) -> Vec<(Point, Point)> {
    xy_cuts(toolpath)
        .into_iter()
        .filter(|(a, b)| (a.z - FLOOR_Z_MM).abs() <= 1e-6 && (b.z - FLOOR_Z_MM).abs() <= 1e-6)
        .map(|(a, b)| (a.xy(), b.xy()))
        .collect()
}

/// The XY distance from `q` to the nearest of `cuts`.
pub(super) fn distance_to_cuts(cuts: &[(Point, Point)], q: Point) -> f64 {
    cuts.iter()
        .map(|&(start, end)| {
            point_segment_distance_mm(
                Segment {
                    start,
                    end,
                    bulge: 0.0,
                },
                q,
            )
        })
        .fold(f64::INFINITY, f64::min)
}

/// The 3D distance from `q` to the segment `a`–`b`.
fn segment_distance_3d(a: Point3, b: Point3, q: Point3) -> f64 {
    let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
    let aq = [q.x - a.x, q.y - a.y, q.z - a.z];
    let length_sq: f64 = ab.iter().map(|v| v * v).sum();
    let t = if length_sq > 0.0 {
        (ab.iter().zip(&aq).map(|(u, v)| u * v).sum::<f64>() / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d: f64 = ab.iter().zip(&aq).map(|(u, v)| (v - u * t).powi(2)).sum();
    d.sqrt()
}

/// True when some cut passes within `tolerance_mm` of `q` in XYZ.
fn cut_through(toolpath: &Toolpath, q: Point3, tolerance_mm: f64) -> bool {
    cuts(toolpath)
        .iter()
        .any(|&(a, b)| segment_distance_3d(a, b, q) <= tolerance_mm)
}

/// The blocks of `op` in `p` (which gains `op`), from [`blocks`].
fn blocks_of(p: &Project, op: &VCarveOp) -> Vec<CutBlock> {
    let p = with_op(p, op);
    let tool = p
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the bit")
        .clone();
    let targets = carve_regions(&p, &op.id, &op.targets, &mut Vec::new()).expect("targets");
    let carve = Carve {
        regions: targets.regions,
        shapes: targets.shapes,
        start_plane_z_mm: TOP_MM,
        engaged_from_z_mm: TOP_MM,
        flat_depth_mm: op.flat_depth_mm,
        strategy: &VCarveStrategy::Medial,
        step_over_fraction: op.step_over_fraction.or(tool.step_over_fraction),
        cleared_by: Vec::new(),
    };
    blocks(&p, &op.id, &tool, &carve, &mut Vec::new()).expect("blocks")
}

/// Each block's moves from its entry, as `(from, to)`.
pub(super) fn block_cuts(block: &CutBlock) -> Vec<(Point3, Point3)> {
    let mut from = Point3::new(block.entry.x, block.entry.y, block.entry_z_mm);
    block
        .cuts
        .iter()
        .map(|m| {
            let cut = (from, m.to);
            from = m.to;
            cut
        })
        .collect()
}

/// Acceptance 50: nothing below Z −2; the 26 × 6 mm floor boundary is
/// traced at −2 (every point of it within 0.01 of a cut at −2); from each
/// floor corner the carve rises along the bisector to the rectangle's
/// corner at the surface (every point of the diagonal within 0.02 of a
/// cut, at its own depth).
#[test]
fn floor_boundary_is_traced_at_the_flat_depth() {
    let (p, op) = acceptance_50(None);
    let (toolpath, warnings) = carve(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_approx_eq!(min_z(&toolpath), FLOOR_Z_MM, 1e-6);

    let floor = floor_cuts(&toolpath);
    let (x0, y0, x1, y1) = (2.0, 2.0, 28.0, 8.0);
    let sides = [
        ((x0, y0), (x1, y0)),
        ((x1, y0), (x1, y1)),
        ((x1, y1), (x0, y1)),
        ((x0, y1), (x0, y0)),
    ];
    for ((ax, ay), (bx, by)) in sides {
        for k in 0..=100 {
            let t = f64::from(k) / 100.0;
            let q = at(ax + (bx - ax) * t, ay + (by - ay) * t);
            let d = distance_to_cuts(&floor, q);
            assert!(
                d <= 0.01,
                "floor boundary point {q:?} is {d} mm from a cut at −2"
            );
        }
    }

    for (cx, cy, fx, fy) in [
        (0.0, 0.0, x0, y0),
        (30.0, 0.0, x1, y0),
        (30.0, 10.0, x1, y1),
        (0.0, 10.0, x0, y1),
    ] {
        for k in 0..=20 {
            let t = f64::from(k) / 20.0;
            let xy = at(cx + (fx - cx) * t, cy + (fy - cy) * t);
            let q = Point3::new(xy.x, xy.y, TOP_MM - F_MM * t);
            assert!(
                cut_through(&toolpath, q, 0.02),
                "the bisector from ({fx}, {fy}) to ({cx}, {cy}) is not cut at {q:?}"
            );
        }
    }
}

/// Acceptance 50: the union of the bit's 4 mm wide footprint at Z −2
/// covers the 26 × 6 mm floor within 0.05 mm, at `step_over_fraction` 0.4
/// and 0.1: every floor point is within 2.05 mm of a cut at −2.
#[test]
fn v_bit_floor_covers_the_floor_region() {
    for fraction in [0.4, 0.1] {
        let (p, op) = acceptance_50(Some(fraction));
        let (toolpath, _) = carve(&p, &op);
        let floor = floor_cuts(&toolpath);
        for i in 0..=260 {
            for j in 0..=60 {
                let q = at(2.0 + f64::from(i) * 0.1, 2.0 + f64::from(j) * 0.1);
                let d = distance_to_cuts(&floor, q);
                assert!(
                    d <= 2.0 + 0.05,
                    "fraction {fraction}: floor point {q:?} is {d} mm from a cut at −2"
                );
            }
        }
    }
}

/// Step 6: between two grooves `s` apart the ridge is `s / (2 tan(α/2))`
/// high, `step_over_fraction · F`: 0.8 mm at 0.4 and 0.2 mm at 0.1 for
/// acceptance 50, which `floor_ridge_height_mm` reports for `check`. On
/// the floor's cross-section through the middle the ridges left are no
/// higher (the distance to the nearest groove over tan 45°). No flat
/// depth, no step-over, or no region wide enough for a floor: nothing to
/// report.
#[test]
fn floor_ridge_height_is_reported() {
    for (fraction, ridge_mm) in [(0.4, 0.8), (0.1, 0.2)] {
        let (p, op) = acceptance_50(Some(fraction));
        let reported = floor_ridge_height_mm(&with_op(&p, &op), &op).expect("a floor");
        assert_approx_eq!(reported, ridge_mm, 1e-9);
        let (toolpath, _) = carve(&p, &op);
        let floor = floor_cuts(&toolpath);
        for j in 0..=600 {
            let q = at(15.0, 2.0 + f64::from(j) * 0.01);
            let height_mm = distance_to_cuts(&floor, q) / slope_of(90.0);
            assert!(
                height_mm <= ridge_mm + 0.02,
                "fraction {fraction}: a ridge {height_mm} mm high at {q:?}"
            );
        }
    }

    let (p, op) = acceptance_50(None);
    let no_flat = VCarveOp {
        flat_depth_mm: None,
        ..op.clone()
    };
    assert_eq!(
        floor_ridge_height_mm(&with_op(&p, &no_flat), &no_flat),
        None
    );
    let mut no_step_over = p.clone();
    for tool in &mut no_step_over.tools {
        tool.step_over_fraction = None;
    }
    assert_eq!(
        floor_ridge_height_mm(&with_op(&no_step_over, &op), &op),
        None
    );
    // A 2 mm wide bar: half-width 1 mm, under F · tan 45° = 2 mm.
    let (narrow, narrow_op) = with_90(vec![rect(20.0, 2.0)], None);
    assert_eq!(
        floor_ridge_height_mm(&with_op(&narrow, &narrow_op), &narrow_op),
        None
    );
}

/// Step 6: medial chains that lie wholly inside the cleared floor are
/// dropped. Acceptance 50's centreline (y 5, x 5…25, all at −2 inside the
/// floor) is not among the medial runs; a circle's single centre node is
/// dropped, leaving only floor blocks.
#[test]
fn chains_inside_the_cleared_floor_are_dropped() {
    let (p, op) = acceptance_50(None);
    let blocks = blocks_of(&p, &op);
    assert!(
        blocks.iter().any(|b| b.cleared.is_some()),
        "no floor blocks"
    );
    for block in blocks.iter().filter(|b| b.cleared.is_none()) {
        for (a, b) in block_cuts(block) {
            let on_centreline = (a.y - (AT.1 + 5.0)).abs() <= 1e-6
                && (b.y - (AT.1 + 5.0)).abs() <= 1e-6
                && (a.x - b.x).abs() > 0.5;
            assert!(
                !on_centreline,
                "the centreline chain is cut: {a:?} -> {b:?}"
            );
        }
    }

    let (circle, circle_op) = with_90(vec![Polyline::circle(Point::new(8.0, 8.0), 8.0)], None);
    let blocks = blocks_of(&circle, &circle_op);
    assert!(!blocks.is_empty());
    assert!(
        blocks.iter().all(|b| b.cleared.is_some()),
        "a medial block is left in the circle's floor"
    );
    carve(&circle, &circle_op);
}

/// Step 6: chains that reach a corner or a narrow part stay. Acceptance
/// 50's four corners are reached at the surface; two 10 mm squares joined
/// by a 2 mm wide neck keep the neck's chain, 1 mm deep at its middle.
#[test]
fn chains_reaching_corners_are_kept() {
    let (p, op) = acceptance_50(None);
    let (toolpath, _) = carve(&p, &op);
    for (x, y) in [(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (0.0, 10.0)] {
        assert!(
            surfaces_at(&toolpath, at(x, y), TOP_MM, 0.01, 0.01),
            "corner ({x}, {y}) is not reached at the surface"
        );
    }

    let dumbbell = polygon(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 4.0),
        (20.0, 4.0),
        (20.0, 0.0),
        (30.0, 0.0),
        (30.0, 10.0),
        (20.0, 10.0),
        (20.0, 6.0),
        (10.0, 6.0),
        (10.0, 10.0),
        (0.0, 10.0),
    ]);
    let (p, op) = with_90(vec![dumbbell], None);
    let (toolpath, _) = carve(&p, &op);
    let neck = at(15.0, 5.0);
    assert!(
        cut_through(&toolpath, Point3::new(neck.x, neck.y, TOP_MM - 1.0), 0.02),
        "the neck is not carved 1 mm deep at its middle"
    );
    assert!(
        !floor_cuts(&toolpath).is_empty(),
        "the squares have no floor"
    );
}

/// The floor's rings and trace pass `cone_in_material`, alone, on shapes
/// whose floors keep their arcs (a circle, an L's reflex corner, a square
/// ring's grown hole) checked against the walls flattened to 1 µm, and on
/// "Rag" at the project's tolerance.
#[test]
fn floor_rings_pass_the_cone_test() {
    let l_shape = polygon(&[
        (0.0, 0.0),
        (30.0, 0.0),
        (30.0, 10.0),
        (10.0, 10.0),
        (10.0, 30.0),
        (0.0, 30.0),
    ]);
    let square_ring = vec![
        rect(30.0, 30.0),
        Polyline::rect(Point::new(10.0, 10.0), 10.0, 10.0),
    ];
    let mut cases: Vec<(&str, Project, VCarveOp, f64)> = vec![];
    for (name, profiles) in [
        ("rectangle", vec![rect(30.0, 10.0)]),
        ("circle", vec![Polyline::circle(Point::new(8.0, 8.0), 8.0)]),
        ("L", vec![l_shape]),
        ("square ring", square_ring),
    ] {
        let (p, op) = with_90(profiles, Some(0.3));
        cases.push((name, p, op, 1e-6));
    }
    let rag = rag_project();
    let rag_op = VCarveOp {
        flat_depth_mm: Some(0.5),
        ..carve_op(&rag)
    };
    cases.push(("Rag, flat 0.5", rag, rag_op, 0.01));

    for (name, p, op, tolerance_mm) in cases {
        let tool = p
            .tools
            .iter()
            .find(|t| t.id == op.tool)
            .expect("the bit")
            .clone();
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            panic!("a V-bit");
        };
        let slope = slope_of(included_angle_deg);
        let cone_mm = cone_height_mm(&tool).expect("a cone");
        let regions = regions_of(&p, &op);
        let floor_blocks: Vec<CutBlock> = blocks_of(&p, &op)
            .into_iter()
            .filter(|b| b.cleared.is_some())
            .collect();
        assert!(!floor_blocks.is_empty(), "{name}: no floor blocks");
        for block in &floor_blocks {
            assert_approx_eq!(block.z_mm, TOP_MM - op.flat_depth_mm.expect("flat"), 1e-9);
            let mut moves = vec![Move::rapid(Point3::new(
                block.entry.x,
                block.entry.y,
                block.entry_z_mm,
            ))];
            moves.extend(block.cuts.iter().copied());
            let toolpath = Toolpath { moves };
            if let Err(violation) = cone_in_material(
                &toolpath,
                &regions,
                TOP_MM,
                TOP_MM,
                slope,
                cone_mm,
                tolerance_mm,
            ) {
                panic!("{name}: a floor block fails the cone test: {violation:?}");
            }
        }
        carve(&p, &op);
    }
}

/// Decision on #405: the V-bit floor's step-over comes from the operation,
/// else the tool; with neither the carve is refused with
/// `MissingStepOver` rather than given an invented one, and a fraction
/// outside (0, 1] is `InvalidStepOver`. With no floor to cut none is
/// needed.
#[test]
fn floor_without_a_step_over_is_refused() {
    let (mut p, op) = acceptance_50(None);
    for tool in &mut p.tools {
        tool.step_over_fraction = None;
    }
    match try_carve(&p, &op) {
        Err(CamError::MissingStepOver {
            op: id, tool_id, ..
        }) => {
            assert_eq!(id, op.id);
            assert_eq!(tool_id, ToolId::new("t2"));
        }
        other => panic!("expected MissingStepOver, got {other:?}"),
    }
    let too_wide = VCarveOp {
        step_over_fraction: Some(1.5),
        ..op.clone()
    };
    assert!(matches!(
        try_carve(&p, &too_wide),
        Err(CamError::InvalidStepOver { .. })
    ));

    let (mut narrow, narrow_op) = with_90(vec![rect(20.0, 2.0)], None);
    for tool in &mut narrow.tools {
        tool.step_over_fraction = None;
    }
    carve(&narrow, &narrow_op);
}
