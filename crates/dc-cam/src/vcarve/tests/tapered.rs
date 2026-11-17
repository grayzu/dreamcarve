// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of carving with a tapered ball nose (#679, slice 27c;
//! `docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose", steps
//! 1 to 6, 8 and 9, and "Test plan → Sub-phase 3.6", the dc-cam carving
//! group; functional 49i, 49j and acceptance 61u to 61w).
//!
//! The bench's bits: Ultra-High Detail (tip 0.762 mm) and Fine Detail (tip
//! 2.286 mm), both 6.7° per side, 19.05 mm of flute and a 6.35 mm cutting
//! diameter. The stock here is [`TBN_TOP_MM`] thick, so a carve as deep
//! as the flutes allow fits in it.

use super::clearing_passes::{clearing_run, end_mill, pass};
use super::*;
use crate::footprint::footprint_at;
use crate::heightfield::{Cutter, Heightfield, carved_profile_z_mm};
use crate::test_support::assert_run_safe;
use dc_geometry::profile::BitProfile;
use dc_geometry::{Rect, area_mm2, difference, floor_regions};
use dc_project::test_support::{tool_tbn_fine, tool_tbn_ultra};
use dc_project::{CuttingLimit, VCarveStrategy};

/// The stock's thickness and the start plane of every carve here, mm.
const TBN_TOP_MM: f64 = 30.0;

/// The taper per side of both bench bits, degrees.
const TAPER_DEG: f64 = 6.7;

/// The Ultra-High Detail bit as tool `t2`.
fn ultra() -> Tool {
    tool_tbn_ultra()
}

/// The Fine Detail bit as tool `t2`.
fn fine() -> Tool {
    Tool {
        id: ToolId::new("t2"),
        ..tool_tbn_fine()
    }
}

/// The profile of a bench bit of tip diameter `tip_mm`, by hand: the
/// tests' own numbers, not `Tool::carve_profile`'s.
fn bench_profile(tip_mm: f64) -> BitProfile {
    BitProfile::TaperedBall {
        tip_radius_mm: tip_mm / 2.0,
        slope: TAPER_DEG.to_radians().tan(),
        cap_radius_mm: 6.35 / 2.0,
    }
}

fn ultra_profile() -> BitProfile {
    bench_profile(0.762)
}

fn fine_profile() -> BitProfile {
    bench_profile(2.286)
}

/// `w⁻¹(r)`, the height above the tip where `profile` is `r` across.
fn inverse_width_mm(profile: &BitProfile, r: f64) -> f64 {
    profile.height_mm(r).expect("below the cap")
}

/// `profiles` placed at [`AT`] on stock [`TBN_TOP_MM`] thick, carved by
/// `tool` (id `t2`), medial, no flat depth, at the stock top.
fn tapered(profiles: Vec<Polyline>, tool: Tool) -> (Project, VCarveOp) {
    let mut p = project(profiles);
    p.stock.thickness_mm = Some(TBN_TOP_MM);
    for instance in &mut p.instances {
        instance.z_top_mm = Some(TBN_TOP_MM);
    }
    p.tools.retain(|t| t.id != ToolId::new("t2"));
    p.tools.push(tool);
    let op = carve_op(&p);
    (p, op)
}

/// A `w` × `h` rectangle whose corners are rounded `r`, counter-clockwise
/// from its bottom-left corner.
fn rounded_rect(w: f64, h: f64, r: f64) -> Polyline {
    let bulge = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
    Polyline::new(
        vec![
            Vertex::new(r, 0.0, 0.0),
            Vertex::new(w - r, 0.0, bulge),
            Vertex::new(w, r, 0.0),
            Vertex::new(w, h - r, bulge),
            Vertex::new(w - r, h, 0.0),
            Vertex::new(r, h, bulge),
            Vertex::new(0.0, h - r, 0.0),
            Vertex::new(0.0, r, bulge),
        ],
        true,
    )
}

/// The points the tip passes below the start plane on every move in XY
/// (chains, links and floor moves, not plunges), each move read every
/// 0.1 mm and at its ends.
fn carving_points(toolpath: &Toolpath) -> Vec<Point3> {
    let mut out = Vec::new();
    for (a, b) in xy_cuts(toolpath) {
        let length_mm = a.xy().distance_to(b.xy());
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a move's length in tenths of a millimetre"
        )]
        let n = (length_mm / 0.1).ceil().max(1.0) as u32;
        for k in 1..=n {
            let t = f64::from(k) / f64::from(n);
            out.push(Point3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            ));
        }
    }
    out.retain(|p| p.z < TBN_TOP_MM - 1e-6);
    out
}

/// Acceptance 61u: a 20 × 1.5 mm bar carved with the Ultra bit runs along
/// its centreline at Z −3.500 (the inverse width of its 0.75 mm half
/// width, on the cone: `0.75 / tan 6.7° − δ`), and every point the bit
/// cuts is at the depth whose cut at the start plane is twice its wall
/// distance, within 0.02 mm.
#[test]
fn tapered_bar_centreline_is_the_inverse_width() {
    let (p, op) = tapered(vec![rect(20.0, 1.5)], ultra());
    let (toolpath, _) = carve(&p, &op);
    let profile = ultra_profile();
    assert_approx_eq!(inverse_width_mm(&profile, 0.75), 3.5, 0.001);
    let centre: Vec<Point3> = carving_points(&toolpath)
        .into_iter()
        .filter(|q| (q.y - (AT.1 + 0.75)).abs() <= 1e-6 && q.x > AT.0 + 1.0 && q.x < AT.0 + 19.0)
        .collect();
    assert!(!centre.is_empty(), "the bar has a centreline");
    for q in &centre {
        assert!(
            (q.z - (TBN_TOP_MM - 3.5)).abs() <= 0.01,
            "a centreline point at {q:?}"
        );
    }
    let regions = regions_of(&with_op(&p, &op), &op);
    let points = carving_points(&toolpath);
    assert!(points.len() > 4, "{points:?}");
    for q in points {
        let wall_mm = wall_distance(&regions, q.xy());
        let want_z = TBN_TOP_MM - inverse_width_mm(&profile, wall_mm);
        assert!(
            (q.z - want_z).abs() <= 0.02,
            "{q:?} is {wall_mm} mm from a wall: want Z {want_z}"
        );
    }
}

/// Acceptance 61u: a 20 × 0.6 mm bar, all in the ball (`c` 0.3 under
/// `w_t` 0.378), is carved at Z −0.146: `ρ − √(ρ² − c²)`.
#[test]
fn tapered_hairline_is_cut_by_the_ball() {
    let (p, op) = tapered(vec![rect(20.0, 0.6)], ultra());
    let (toolpath, _) = carve(&p, &op);
    let rho = 0.381;
    let want_mm = rho - (rho * rho - 0.09_f64).sqrt();
    assert_approx_eq!(want_mm, 0.146, 0.001);
    assert_approx_eq!(min_z(&toolpath), TBN_TOP_MM - want_mm, 0.005);
    let centre: Vec<Point3> = carving_points(&toolpath)
        .into_iter()
        .filter(|q| q.x > AT.0 + 1.0 && q.x < AT.0 + 19.0)
        .collect();
    assert!(!centre.is_empty());
    for q in centre {
        assert!(
            (q.z - (TBN_TOP_MM - want_mm)).abs() <= 0.005,
            "a centreline point at {q:?}"
        );
    }
}

/// Acceptance 61u: the chains into the 1.5 mm bar's corners run to their
/// tips at the start plane: each corner is reached at Z ≥ −0.01.
#[test]
fn tapered_corners_are_reached_at_the_surface() {
    let (p, op) = tapered(vec![rect(20.0, 1.5)], ultra());
    let (toolpath, _) = carve(&p, &op);
    for corner in [at(0.0, 0.0), at(20.0, 0.0), at(20.0, 1.5), at(0.0, 1.5)] {
        assert!(
            surfaces_at(&toolpath, corner, TBN_TOP_MM, 0.01, 0.01),
            "the corner {corner:?} is not reached at the surface"
        );
    }
}

/// The sign's lettering (`tests/fixtures/sign.dcv`), the "DC" letters
/// (`tests/fixtures/dc.dcv`) and the star, carved with each bench bit at
/// a 2 mm flat depth: every program passes S1–S3 and the exact cone test,
/// S17 ([`assert_safe`]).
#[test]
fn tapered_cuts_pass_the_exact_cone_test() {
    let sign = dc_project::load(include_bytes!("../../../../../tests/fixtures/sign.dcv"))
        .expect("the sign loads")
        .project;
    let lettering = sign
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::VCarve(op) if op.name == "Lettering" => Some(op.clone()),
            _ => None,
        })
        .expect("the sign has its lettering");
    let dc = dc_project::load(include_bytes!("../../../../../tests/fixtures/dc.dcv"))
        .expect("the DC pair loads")
        .project;
    let dc_targets = dc
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::Inlay(op) => Some(op.targets.clone()),
            _ => None,
        })
        .expect("the DC pair has its Inlay");
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    let star = polygon(&[
        (60.0, 30.0),
        (30.0 + inner, 30.0 + inner),
        (30.0, 60.0),
        (30.0 - inner, 30.0 + inner),
        (0.0, 30.0),
        (30.0 - inner, 30.0 - inner),
        (30.0, 0.0),
        (30.0 + inner, 30.0 - inner),
    ]);
    for tool in [ultra(), fine()] {
        let bit = Tool {
            id: ToolId::new("t9"),
            ..tool
        };
        let mut cases = Vec::new();
        let mut sign = sign.clone();
        sign.tools.push(bit.clone());
        cases.push((
            "the sign's lettering",
            sign,
            VCarveOp {
                tool: bit.id.clone(),
                ..lettering.clone()
            },
        ));
        let mut dc = dc.clone();
        dc.tools.push(bit.clone());
        dc.operations.clear();
        cases.push((
            "the DC letters",
            dc,
            VCarveOp {
                targets: dc_targets.clone(),
                tool: bit.id.clone(),
                ..sample_v_carve()
            },
        ));
        let (mut star_p, star_op) = tapered(vec![star.clone()], ultra());
        star_p.tools.push(bit.clone());
        cases.push((
            "the star",
            star_p,
            VCarveOp {
                tool: bit.id.clone(),
                ..star_op
            },
        ));
        for (name, p, op) in cases {
            let op = VCarveOp {
                flat_depth_mm: Some(2.0),
                step_over_fraction: Some(0.3),
                ..op
            };
            let p = with_op(&p, &op);
            let (runs, _) = generate_vcarve(&p, &op)
                .unwrap_or_else(|e| panic!("{name} with {}: {e}", bit.name));
            for run in &runs {
                assert_run_safe(run, &p, &Operation::VCarve(op.clone()));
            }
        }
    }
}

/// Acceptance 61u: with the Ultra bit, whose flutes (19.05 mm) end below
/// its cap (24.14 mm), a 20 × 5 mm bar needs 18.397 mm and exports; a
/// 20 × 6 mm bar would need 22.65 mm and is `VBitConeExceeded`, naming
/// the flutes.
#[test]
fn tapered_flute_length_bounds_the_engagement() {
    let (p, op) = tapered(vec![rect(20.0, 5.0)], ultra());
    let (toolpath, _) = carve(&p, &op);
    assert_approx_eq!(min_z(&toolpath), TBN_TOP_MM - 18.397, 0.01);

    let (p, op) = tapered(vec![rect(20.0, 6.0)], ultra());
    match try_carve(&p, &op) {
        Err(
            e @ CamError::VBitConeExceeded {
                depth_mm,
                cone_mm,
                limit,
                ..
            },
        ) => {
            assert_approx_eq!(depth_mm, 22.65, 0.01);
            assert_approx_eq!(cone_mm, 19.05, 1e-9);
            assert_eq!(limit, CuttingLimit::Flutes);
            let message = e.to_string();
            assert!(message.contains("19.050 mm flutes"), "{message}");
            assert!(message.contains("22.65"), "{message}");
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }
}

/// Acceptance 61u and question 32: the Fine bit's cone reaches its
/// 6.35 mm cutting diameter 18.374 mm up, below its flutes. A 20 × 6 mm
/// bar is 16.884 mm deep; from a start plane 1.6 mm down, vouched for
/// but engaged from the stock top, the flank is engaged 18.484 mm, which
/// is `VBitConeExceeded` naming the cutting diameter; from 1.4 mm down it
/// is engaged 18.284 mm and exports. A 20 × 7 mm bar, whose centreline is
/// farther than 3.175 mm from its walls, no depth of the taper reaches,
/// and it is refused too.
#[test]
fn tapered_cut_diameter_bounds_the_engagement() {
    let (p, op) = tapered(vec![rect(20.0, 6.0)], fine());
    assert_approx_eq!(inverse_width_mm(&fine_profile(), 3.0), 16.884, 0.001);
    let from = |start_depth_mm: f64| VCarveOp {
        start_depth_mm,
        start_plane_cleared_externally: true,
        ..op.clone()
    };
    match try_carve(&p, &from(1.6)) {
        Err(
            e @ CamError::VBitConeExceeded {
                depth_mm,
                cone_mm,
                limit,
                ..
            },
        ) => {
            assert_approx_eq!(depth_mm, 18.484, 0.01);
            assert_approx_eq!(cone_mm, 18.374, 0.001);
            assert_eq!(limit, CuttingLimit::CuttingDiameter { diameter_mm: 6.35 });
            let message = e.to_string();
            assert!(message.contains("18.374"), "{message}");
            assert!(message.contains("6.350 mm cutting diameter"), "{message}");
        }
        other => panic!("expected VBitConeExceeded, got {other:?}"),
    }
    let (toolpath, _) = carve(&p, &from(1.4));
    assert_approx_eq!(min_z(&toolpath), TBN_TOP_MM - 1.4 - 16.884, 0.01);

    let (p, op) = tapered(vec![rect(20.0, 7.0)], fine());
    assert!(
        matches!(
            try_carve(&p, &op),
            Err(CamError::VBitConeExceeded {
                limit: CuttingLimit::CuttingDiameter { .. },
                ..
            })
        ),
        "{:?}",
        try_carve(&p, &op).map(|_| ())
    );
}

/// The flat depth of the floor tests, mm.
const TBN_F_MM: f64 = 2.0;

/// Acceptance 61v's rectangle: 30 × 10 mm, the Fine bit, `flat_depth_mm`
/// 2, at `step_over_fraction`.
fn tapered_floor(step_over_fraction: f64) -> (Project, VCarveOp) {
    let (p, op) = tapered(vec![rect(30.0, 10.0)], fine());
    let op = VCarveOp {
        flat_depth_mm: Some(TBN_F_MM),
        step_over_fraction: Some(step_over_fraction),
        ..op
    };
    (p, op)
}

/// The XY cuts of `toolpath` at the floor, [`TBN_F_MM`] below the top.
fn tapered_floor_cuts(toolpath: &Toolpath) -> Vec<(Point, Point)> {
    let z_mm = TBN_TOP_MM - TBN_F_MM;
    xy_cuts(toolpath)
        .into_iter()
        .filter(|(a, b)| (a.z - z_mm).abs() <= 1e-6 && (b.z - z_mm).abs() <= 1e-6)
        .map(|(a, b)| (a.xy(), b.xy()))
        .collect()
}

/// Acceptance 61v: nothing below Z −2, and the floor boundary traced is
/// the rectangle inset by `w(2)` = 1.2515 mm, 27.497 × 7.497 mm, at Z −2
/// within 0.01 mm: every point of it within 0.01 of a cut at −2, and no
/// cut at −2 outside it.
#[test]
fn tapered_floor_inset_is_the_width_at_the_flat_depth() {
    let (p, op) = tapered_floor(0.4);
    let (toolpath, _) = carve(&p, &op);
    assert_approx_eq!(min_z(&toolpath), TBN_TOP_MM - TBN_F_MM, 1e-6);
    let inset_mm = fine_profile().half_width_mm(TBN_F_MM);
    assert_approx_eq!(30.0 - 2.0 * inset_mm, 27.497, 0.001);
    let floor = tapered_floor_cuts(&toolpath);
    let (x0, y0, x1, y1) = (inset_mm, inset_mm, 30.0 - inset_mm, 10.0 - inset_mm);
    for ((ax, ay), (bx, by)) in [
        ((x0, y0), (x1, y0)),
        ((x1, y0), (x1, y1)),
        ((x1, y1), (x0, y1)),
        ((x0, y1), (x0, y0)),
    ] {
        for k in 0..=100 {
            let t = f64::from(k) / 100.0;
            let q = at(ax + (bx - ax) * t, ay + (by - ay) * t);
            let d = super::floor::distance_to_cuts(&floor, q);
            assert!(d <= 0.01, "floor boundary point {q:?} is {d} mm from a cut");
        }
    }
    for (a, b) in &floor {
        for q in [a, b] {
            assert!(
                q.x >= AT.0 + x0 - 0.01
                    && q.x <= AT.0 + x1 + 0.01
                    && q.y >= AT.1 + y0 - 0.01
                    && q.y <= AT.1 + y1 + 0.01,
                "a floor cut at {q:?} outside the floor"
            );
        }
    }
}

/// The heightfield of `runs` cut with `cutter` over `window`, cells
/// `step_mm` apart, from a stock top at [`TBN_TOP_MM`].
fn heightfield_of(runs: &[ToolRun], cutter: Cutter, window: Rect, step_mm: f64) -> Heightfield {
    let mut field = Heightfield::new(window, step_mm, TBN_TOP_MM, None);
    field.cut_runs(runs, |_| cutter);
    field
}

/// The Fine bit as the heightfield cuts it.
fn fine_cutter() -> Cutter {
    let BitProfile::TaperedBall {
        tip_radius_mm,
        slope,
        cap_radius_mm,
    } = fine_profile()
    else {
        unreachable!("a tapered ball nose")
    };
    Cutter::TaperedBall {
        tip_radius_mm,
        slope,
        cap_radius_mm,
    }
}

/// Acceptance 61v, by heightfield: across the middle of the rectangle's
/// long side, the wall is the cone from the outline down to Z −0.990 (`F −
/// h_t`), `y / tan 6.7°` deep at `y` from the wall, then the ball's arc,
/// radius 1.143 mm, into the floor at Z −2, within 0.02 mm.
#[test]
fn tapered_floor_has_the_ball_fillet() {
    let (p, op) = tapered_floor(0.4);
    let p = with_op(&p, &op);
    let (runs, _) = generate_vcarve(&p, &op).expect("the carve generates");
    let profile = fine_profile();
    let t = profile.slope();
    let inset_mm = profile.half_width_mm(TBN_F_MM);
    let w_t = profile.tangent_half_width_mm();
    let cone_to_mm = inset_mm - w_t;
    assert_approx_eq!(cone_to_mm / t, 0.990, 0.001);
    let step_mm = 0.01;
    let window = Rect {
        min: at(14.9, -0.1),
        max: at(15.1, 2.0),
    };
    let field = heightfield_of(&runs, fine_cutter(), window, step_mm);
    let (nx, ny) = field.size();
    let i = (0..nx)
        .min_by(|&a, &b| {
            (field.cell(a, 0).x - at(15.0, 0.0).x)
                .abs()
                .total_cmp(&(field.cell(b, 0).x - at(15.0, 0.0).x).abs())
        })
        .expect("cells");
    let mut checked = 0;
    for j in 0..ny {
        let y = field.cell(i, j).y - AT.1;
        if !(0.0..=inset_mm).contains(&y) {
            continue;
        }
        let want_z = if y <= cone_to_mm {
            TBN_TOP_MM - y / t
        } else {
            TBN_TOP_MM - TBN_F_MM + inverse_width_mm(&profile, inset_mm - y)
        };
        let got_z = field.z_mm(i, j);
        assert!(
            (got_z - want_z).abs() <= 0.02,
            "{y} mm from the wall: Z {got_z}, want {want_z}"
        );
        checked += 1;
    }
    assert!(checked > 100, "{checked} cells");
}

/// Acceptance 61v: at `step_over_fraction` 0.4 the step-over is 0.4 of
/// the bit's width at the floor, 1.001 mm, and two grooves that far apart
/// leave the ball's scallop, `ρ − √(ρ² − s²/4)` = 0.116 mm: what
/// `floor_ridges` reports and what the heightfield of the floor's
/// cross-section shows, within 0.005 mm.
#[test]
fn tapered_floor_ridge_is_the_ball_scallop() {
    let (p, op) = tapered_floor(0.4);
    let p = with_op(&p, &op);
    let reported = floor_ridges(&p, &op).expect("a floor");
    assert_approx_eq!(reported.straight_mm, 0.116, 0.005);
    let rho = 1.143;
    let s = 0.4 * 2.0 * fine_profile().half_width_mm(TBN_F_MM);
    assert_approx_eq!(
        reported.straight_mm,
        rho - (rho * rho - s * s / 4.0).sqrt(),
        1e-9
    );
    let (runs, _) = generate_vcarve(&p, &op).expect("the carve generates");
    let inset_mm = fine_profile().half_width_mm(TBN_F_MM);
    let window = Rect {
        min: at(14.99, inset_mm),
        max: at(15.01, 10.0 - inset_mm),
    };
    let field = heightfield_of(&runs, fine_cutter(), window, 0.005);
    let (nx, ny) = field.size();
    let floor_z_mm = TBN_TOP_MM - TBN_F_MM;
    let highest_mm = (0..ny)
        .flat_map(|j| (0..nx).map(move |i| (i, j)))
        .map(|(i, j)| field.z_mm(i, j) - floor_z_mm)
        .fold(0.0, f64::max);
    assert!(
        (highest_mm - reported.straight_mm).abs() <= 0.005,
        "the floor's highest ridge is {highest_mm} mm, reported {}",
        reported.straight_mm
    );
}

/// Acceptance 61v: every point of the floor region lies within one
/// step-over of a cut at the floor, at fractions 0.4 and 0.1, so no
/// ridge stands higher than the corner ridge allows.
#[test]
fn tapered_floor_covers_the_floor_region() {
    for fraction in [0.4, 0.1] {
        let (p, op) = tapered_floor(fraction);
        let (toolpath, _) = carve(&p, &op);
        let floor = tapered_floor_cuts(&toolpath);
        let inset_mm = fine_profile().half_width_mm(TBN_F_MM);
        let s = fraction * 2.0 * inset_mm;
        for i in 0..=100 {
            for j in 0..=30 {
                let x = inset_mm + (30.0 - 2.0 * inset_mm) * f64::from(i) / 100.0;
                let y = inset_mm + (10.0 - 2.0 * inset_mm) * f64::from(j) / 30.0;
                let q = at(x, y);
                let d = super::floor::distance_to_cuts(&floor, q);
                assert!(
                    d <= s + 0.01,
                    "fraction {fraction}: floor point {q:?} is {d} mm from a cut"
                );
            }
        }
    }
}

/// Acceptance 61v: a 3.175 mm end mill's clearing pass takes the carve's
/// floor regions at `w(F)`: its footprint stays inside the 27.497 ×
/// 7.497 mm floor.
#[test]
fn tapered_clearing_pass_takes_the_floor_inset() {
    let (mut p, op) = tapered_floor(0.4);
    p.tools.push(end_mill("t3", 3.175));
    let op = VCarveOp {
        clearing: vec![pass("t3")],
        ..op
    };
    let p = with_op(&p, &op);
    let (runs, _) = generate_vcarve(&p, &op).expect("the carve generates");
    for run in &runs {
        assert_run_safe(run, &p, &Operation::VCarve(op.clone()));
    }
    let tolerance_mm = p.flatten_tolerance_mm;
    let inset_mm = fine_profile().half_width_mm(TBN_F_MM);
    let floors = floor_regions(&regions_of(&p, &op), inset_mm, tolerance_mm);
    let footprint = footprint_at(clearing_run(&runs, "t3"), 3.175 / 2.0, tolerance_mm);
    assert!(!footprint.is_empty(), "the pass cuts");
    let outside = area_mm2(&difference(&footprint, &floors, tolerance_mm));
    assert!(
        outside <= 2.0 * tolerance_mm * (2.0 * (27.497 + 7.497)),
        "the footprint is {outside} mm² outside the floor"
    );
    let bounds = footprint
        .iter()
        .filter_map(|r| r.outer().polyline().bounds())
        .reduce(|a, b| Rect {
            min: Point::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
            max: Point::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
        })
        .expect("a footprint");
    assert!(bounds.min.x >= AT.0 + inset_mm - 0.01, "{bounds:?}");
    assert!(bounds.min.y >= AT.1 + inset_mm - 0.01, "{bounds:?}");
    assert!(bounds.max.x <= AT.0 + 30.0 - inset_mm + 0.01, "{bounds:?}");
    assert!(bounds.max.y <= AT.1 + 10.0 - inset_mm + 0.01, "{bounds:?}");
}

/// Acceptance 61v's offset carve: a 6 mm square, `strategy: "offset"`,
/// `step_over_mm` 0.2, the Ultra bit, with a 10 mm flat depth so the
/// carve stays inside the flutes (its rings stop at `w(10)`, 1.514 mm in).
fn tapered_offset() -> (Project, VCarveOp) {
    let (p, op) = tapered(vec![rect(6.0, 6.0)], ultra());
    let op = VCarveOp {
        strategy: VCarveStrategy::Offset {
            step_over_mm: 0.2,
            direction: dc_project::CutDirection::Climb,
        },
        flat_depth_mm: Some(10.0),
        step_over_fraction: Some(0.3),
        ..op
    };
    (p, op)
}

/// Acceptance 61v: ring `k` of the offset carve is inset `0.2 k` at Z
/// `−w⁻¹(0.2 k)`, within 0.01 mm: rings 1 in the ball, 2 to 7 on the cone.
#[test]
fn tapered_offset_rings_follow_the_profile() {
    let (p, op) = tapered_offset();
    let (toolpath, _) = carve(&p, &op);
    let regions = regions_of(&with_op(&p, &op), &op);
    let profile = ultra_profile();
    let floor_z_mm = TBN_TOP_MM - 10.0;
    let mut seen = [false; 8];
    let ring_points = xy_cuts(&toolpath)
        .into_iter()
        .filter(|(a, b)| (a.z - b.z).abs() <= 1e-9 && b.z < TBN_TOP_MM - 1e-6)
        .map(|(_, b)| b);
    for q in ring_points {
        if (q.z - floor_z_mm).abs() <= 1e-6 {
            continue;
        }
        let inset_mm = wall_distance(&regions, q.xy());
        let k = (inset_mm / 0.2).round();
        assert!(
            (inset_mm - 0.2 * k).abs() <= 0.01 && (1.0..=7.0).contains(&k),
            "{q:?} is inset {inset_mm} mm, not a ring's"
        );
        let want_z = TBN_TOP_MM - inverse_width_mm(&profile, 0.2 * k);
        assert!(
            (q.z - want_z).abs() <= 0.01,
            "ring {k} at {q:?}: want Z {want_z}"
        );
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "k is a ring number from 1 to 7"
        )]
        if let Some(s) = seen.get_mut(k as usize) {
            *s = true;
        }
    }
    assert!(seen.iter().skip(1).all(|s| *s), "rings cut: {seen:?}");
}

/// Design step 6: a link between two rings stays down only on the cone,
/// where the surface the rings cut is straight along a corner's bisector;
/// where either ring is in the ball (`w⁻¹(0.2)` above the tip, under
/// `h_t`), the surface is convex and a straight link would run below it,
/// so the tool retracts.
#[test]
fn tapered_offset_links_in_the_ball_retract() {
    let (p, op) = tapered_offset();
    let (toolpath, _) = carve(&p, &op);
    let h_t = ultra_profile().tangent_height_mm();
    let links: Vec<(Point3, Point3)> = xy_cuts(&toolpath)
        .into_iter()
        .filter(|(a, b)| (a.z - b.z).abs() > 1e-6)
        .collect();
    assert!(!links.is_empty(), "the cone's rings are linked tool-down");
    for (a, b) in links {
        for q in [a, b] {
            assert!(
                TBN_TOP_MM - q.z >= h_t - 1e-6,
                "a link {a:?} → {b:?} runs in the ball"
            );
        }
    }
}

/// Acceptance 61w: a 20 × 20 mm square with the Ultra bit warns
/// `tapered_corners_softened` once, counting its 4 corners, the farthest
/// change at a corner, `w_t (1/sin 45° − 1)` = 0.157 mm from the opening by
/// `w_t` = 0.378 mm; the same square with its corners rounded 0.5 mm does
/// not warn.
#[test]
fn tapered_corners_softened_warns_once() {
    let flat = |profiles: Vec<Polyline>| {
        let (p, op) = tapered(profiles, ultra());
        let op = VCarveOp {
            flat_depth_mm: Some(2.0),
            step_over_fraction: Some(0.3),
            ..op
        };
        (p, op)
    };
    let (p, op) = flat(vec![rect(20.0, 20.0)]);
    let (_, warnings) = carve(&p, &op);
    let softened: Vec<&Warning> = warnings
        .iter()
        .filter(|w| w.code() == WarningCode::TaperedCornersSoftened)
        .collect();
    assert_eq!(softened.len(), 1, "{warnings:?}");
    assert_eq!(
        WarningCode::TaperedCornersSoftened.as_str(),
        "tapered_corners_softened"
    );
    let Warning::TaperedCornersSoftened {
        op: id,
        count,
        x_mm,
        y_mm,
        change_mm,
        radius_mm,
    } = softened[0]
    else {
        unreachable!("filtered by code")
    };
    assert_eq!(id, &op.id);
    assert_eq!(*count, 4);
    let w_t = ultra_profile().tangent_half_width_mm();
    assert_approx_eq!(*radius_mm, 0.378, 0.001);
    assert_approx_eq!(*radius_mm, w_t, 1e-9);
    assert_approx_eq!(*change_mm, w_t * (std::f64::consts::SQRT_2 - 1.0), 0.01);
    let corner = Point::new(*x_mm, *y_mm);
    assert!(
        [at(0.0, 0.0), at(20.0, 0.0), at(20.0, 20.0), at(0.0, 20.0)]
            .iter()
            .any(|c| c.distance_to(corner) <= 0.01),
        "{corner:?} is at no corner"
    );

    let (p, op) = flat(vec![rounded_rect(20.0, 20.0, 0.5)]);
    let (_, warnings) = carve(&p, &op);
    assert!(
        warnings
            .iter()
            .all(|w| w.code() != WarningCode::TaperedCornersSoftened),
        "{warnings:?}"
    );
}

/// Acceptance 61w and design step 9: along a 90° corner's bisector the
/// cavity's corner at Z −2 stands back from a V-bit's of the same taper
/// (the outline inset by `h · t`, `h t / sin 45°` along the bisector) by
/// `Δx = (u + δ) t / S − ρ sin φ`, `tan φ = S / t`, `u = ρ (1 − cos φ)`:
/// 0.156 mm with the Ultra bit and 0.469 mm with the Fine, within
/// 0.01 mm, read from the exact surface the program leaves (the
/// heightfield's per-cell height, [`carved_profile_z_mm`]).
#[test]
fn tapered_corner_setback_matches_the_formula() {
    for (tool, profile, want_mm) in [
        (ultra(), ultra_profile(), 0.156),
        (fine(), fine_profile(), 0.469),
    ] {
        let BitProfile::TaperedBall {
            tip_radius_mm: rho,
            slope: t,
            ..
        } = profile
        else {
            unreachable!("a tapered ball nose")
        };
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let phi = (s / t).atan();
        let u = rho * (1.0 - phi.cos());
        let formula_mm = (u + profile.apex_offset_mm()) * t / s - rho * phi.sin();
        assert_approx_eq!(formula_mm, want_mm, 0.001);

        let (p, op) = tapered(vec![rect(20.0, 20.0)], tool);
        let op = VCarveOp {
            flat_depth_mm: Some(4.0),
            step_over_fraction: Some(0.3),
            ..op
        };
        let (toolpath, _) = carve(&p, &op);
        let h = 2.0;
        let along = |x: f64| at(x * s, x * s);
        let z_at = |x: f64| carved_profile_z_mm(&toolpath, along(x), &profile);
        // The carved surface falls from the corner's tip inwards: find
        // where it crosses Z −2 by bisection.
        let (mut lo, mut hi) = (0.0, 3.0);
        assert!(z_at(lo) > TBN_TOP_MM - h && z_at(hi) < TBN_TOP_MM - h);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if z_at(mid) > TBN_TOP_MM - h {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let ideal_mm = h * t / s;
        let setback_mm = 0.5 * (lo + hi) - ideal_mm;
        assert!(
            (setback_mm - want_mm).abs() <= 0.01,
            "the corner stands {setback_mm} mm back, want {want_mm}"
        );
    }
}

/// FNV-1a of `text`.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The digest of every run `op` generates in `p`: each move's kind, end
/// and feed, every number written in full (`{:?}` of an `f64` reads back
/// to the same bits).
fn program_digest(p: &Project, op: &VCarveOp) -> u64 {
    let p = with_op(p, op);
    let (runs, _) = generate_vcarve(&p, op).expect("the carve generates");
    let text: String = runs
        .iter()
        .flat_map(|r| r.toolpath.moves.iter())
        .map(|m| format!("{:?}|{:?}|{:?}\n", m.kind, m.to, m.feed_mm_min))
        .collect();
    fnv1a(&text)
}

/// Slice 27c changes no V-bit program: every V site reads the profile,
/// which for a V-bit reads exactly what it read before. Bar, star, floor,
/// offset and clearing V-bit carves keep the digests recorded on `main`
/// before the slice (after #474 and #527) (the goldens, `golden::vcarve_*` and
/// `golden::phase_1_and_2_cases_are_unchanged`, and the CLI's sign test
/// hold the written programs byte for byte). The digests were recorded on
/// macOS, where they must match bit for bit; elsewhere the platform's
/// maths library may round a sine or a tangent differently in its last
/// bit, which the digest of every bit sees, so there the goldens are the
/// check (as the CLI's `inlay-migration` programs are held, #498).
#[test]
fn v_bit_programs_are_unchanged() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let bar = project(vec![rect(20.0, 4.0)]);
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    let star_shape = polygon(&[
        (30.0, 15.0),
        (15.0 + inner / 2.0, 15.0 + inner / 2.0),
        (15.0, 30.0),
        (15.0 - inner / 2.0, 15.0 + inner / 2.0),
        (0.0, 15.0),
        (15.0 - inner / 2.0, 15.0 - inner / 2.0),
        (15.0, 0.0),
        (15.0 + inner / 2.0, 15.0 - inner / 2.0),
    ]);
    let star = project(vec![star_shape]);
    let star_op = VCarveOp {
        flat_depth_mm: Some(2.0),
        step_over_fraction: Some(0.3),
        ..carve_op(&star)
    };
    let (floor_p, floor_op) = super::floor::acceptance_50(Some(0.4));
    let square = project(vec![rect(6.0, 6.0)]);
    let offset_op = VCarveOp {
        strategy: VCarveStrategy::Offset {
            step_over_mm: 0.3,
            direction: dc_project::CutDirection::Climb,
        },
        ..carve_op(&square)
    };
    let (mut clear_p, clear_op) = super::floor::acceptance_50(Some(0.3));
    clear_p.tools.push(end_mill("t3", 3.175));
    let clear_op = VCarveOp {
        clearing: vec![pass("t3")],
        ..clear_op
    };
    let cases = [
        (
            "bar",
            program_digest(&bar, &carve_op(&bar)),
            0x10b5_03d5_bc71_2622_u64,
        ),
        (
            "star",
            program_digest(&star, &star_op),
            0x984e_249f_7d15_15cc,
        ),
        (
            "floor",
            program_digest(&floor_p, &floor_op),
            0x5004_612d_02fb_2584,
        ),
        (
            "offset",
            program_digest(&square, &offset_op),
            0x815a_3168_1875_04d2,
        ),
        (
            "clearing",
            program_digest(&clear_p, &clear_op),
            0xf1c2_f1ef_d3b6_cdcb,
        ),
    ];
    let changed: Vec<String> = cases
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(name, got, want)| format!("{name}: {got:#x}, recorded {want:#x}"))
        .collect();
    assert!(changed.is_empty(), "{changed:#?}");
}

/// #705 (the tapered sweep's lettering `0x60`, Fine bit, drawn 6×
/// larger): the carve reaches the walls of a point the Inlay rounded to
/// `R_min`, though its chain runs past the round end's centre. The
/// female's design, rounded for the bit, has a point of about 87° rounded
/// to 1.193 mm, drawn as chords about 0.12 mm long, about the centre `C`
/// (26.227, 162.750). The letter is large, so its medial axis is sampled
/// 0.93 mm apart, and the node at `C` was merged into the spine's node
/// 0.58 mm down the bisector: the graph ran straight from there to a node
/// 0.16 mm past `C`, on the axis toward one of the chords' corners (both
/// nodes here as the graph gave them). Fitting sought the axis only from
/// the nearest wall of each point it read on the stretch, and the stretch
/// passes `C` on the side away from the arc, so no point read had its
/// nearest wall on the arc and none found `C`: the carve stopped
/// 0.0126 mm short of the arc's middle, and the plug stood 0.107 mm proud
/// there. The carve reaches the middle of every chord of the arc, the
/// walls the disc at `C` touches, within the fit's axis tolerance.
#[test]
fn tapered_carve_reaches_a_rounded_point_past_its_centre() {
    let arc = [
        (25.969_10, 163.914_08),
        (25.874_14, 163.890_25),
        (25.748_47, 163.843_23),
        (25.628_85, 163.782_42),
        (25.516_81, 163.708_60),
        (25.413_73, 163.622_69),
        (25.320_93, 163.525_78),
        (25.239_57, 163.419_08),
        (25.170_67, 163.303_94),
        (25.115_11, 163.181_80),
        (25.073_57, 163.054_21),
        (25.046_59, 162.922_77),
        (25.034_51, 162.789_14),
        (25.037_46, 162.654_99),
        (25.050_58, 162.557_95),
    ];
    // The point's two walls, closed far beyond it.
    let mut outline: Vec<(f64, f64)> = vec![(37.6, 166.83)];
    outline.extend_from_slice(&arc);
    outline.extend_from_slice(&[(25.883_58, 158.236_63), (37.6, 158.24)]);
    let contour = Contour::new(polygon(&outline)).expect("an outline");
    let region = nest(vec![contour]).expect("one region").0.remove(0);
    let profile = fine_profile();
    let slope = profile.slope();
    let walls = Walls::new(
        std::slice::from_ref(&region),
        wall_tolerance_mm(dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM, slope),
    );
    let (spine, past) = (Point::new(26.7125, 162.4248), Point::new(26.1730, 162.9000));
    let chain = graph(
        &[
            (spine.x, spine.y, walls.clearance_mm(spine)),
            (past.x, past.y, walls.clearance_mm(past)),
        ],
        &[&[1], &[0]],
    );
    let top_mm = 12.0;
    let depth = Depth {
        start_plane_z_mm: top_mm,
        flat_depth_mm: Some(2.129),
        profile,
    };
    let axis_mm = wall_tolerance_mm(medial::EDGE_MM, slope);
    let tolerances = Tolerances {
        simplify_mm: axis_mm,
        redundancy_mm: axis_mm,
        deepen_mm: None,
        axis_mm: Some(axis_mm),
        written_mm: 0.0,
        grid: None,
    };
    let runs = medial::runs(&chain, &depth, tolerances, &walls, &[]);
    let mut moves: Vec<(Point3, Point3)> = Vec::new();
    for run in &runs {
        let mut from = run.start;
        for step in &run.steps {
            moves.push((from, step.to));
            from = step.to;
        }
    }
    // How far short of `q` the moves' discs at the start plane stop, each
    // move read every micrometre.
    let short_of = |q: Point| {
        let mut reach = f64::NEG_INFINITY;
        for &(a, b) in &moves {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a short move's length in micrometres"
            )]
            let n = (a.xy().distance_to(b.xy()) / 0.001).ceil().max(1.0) as u32;
            for k in 0..=n {
                let t = f64::from(k) / f64::from(n);
                let p = Point3::new(
                    a.x + (b.x - a.x) * t,
                    a.y + (b.y - a.y) * t,
                    a.z + (b.z - a.z) * t,
                );
                reach = reach.max(profile.half_width_mm(top_mm - p.z) - p.xy().distance_to(q));
            }
        }
        -reach
    };
    let (short_mm, at) = arc
        .windows(2)
        .map(|pair| {
            let [(x0, y0), (x1, y1)] = pair else {
                unreachable!("a window of two")
            };
            let q = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            (short_of(q), q)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .expect("the arc has chords");
    assert!(
        short_mm <= axis_mm + 1e-4,
        "the carve stops {short_mm:.4} mm short of the arc at {at:?}, past {axis_mm:.4}: {moves:?}"
    );
}
