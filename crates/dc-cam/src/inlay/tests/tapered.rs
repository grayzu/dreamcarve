// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! An Inlay cut with a tapered ball nose (#681, slice 27e;
//! `docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose →
//! Inlay fit", "The taper's ±0.5°, and calibration", and "Test plan →
//! Sub-phase 3.6", the Inlay group; functional 49k, acceptance 61x and
//! 61y).
//!
//! The bench's bits: Ultra-High Detail (tip 0.762 mm) and Fine Detail (tip
//! 2.286 mm), both 6.7° per side, 19.05 mm of flute and a 6.35 mm cutting
//! diameter, as tool `t1` in place of the star part's V-bit. The expected
//! values are the tests' own, from the bit's formulas ([`Bench`]).

use super::*;
use crate::WarningCode;
use dc_geometry::profile::BitProfile;
use dc_project::test_support::{tool_tbn_fine, tool_tbn_ultra};
use dc_project::{CornerKind, CuttingLimit, rounded_design};

/// A bench bit by hand: tip radius `ρ`, `t = tan 6.7°`.
#[derive(Debug, Clone, Copy)]
struct Bench {
    rho: f64,
}

impl Bench {
    const ULTRA: Self = Self { rho: 0.381 };
    const FINE: Self = Self { rho: 1.143 };

    fn sin() -> f64 {
        6.7_f64.to_radians().sin()
    }

    fn t() -> f64 {
        6.7_f64.to_radians().tan()
    }

    /// `h_t = ρ (1 − sin β)`.
    fn h_t(self) -> f64 {
        self.rho * (1.0 - Self::sin())
    }

    /// `w_t = ρ cos β`.
    fn w_t(self) -> f64 {
        self.rho * 6.7_f64.to_radians().cos()
    }

    /// `δ = ρ (1/sin β − 1)`.
    fn delta(self) -> f64 {
        self.rho * (1.0 / Self::sin() - 1.0)
    }

    /// `w(d)`: the ball, then the cone (below the cap).
    fn w(self, d: f64) -> f64 {
        if d <= 0.0 {
            0.0
        } else if d <= self.h_t() {
            (2.0 * self.rho * d - d * d).sqrt()
        } else {
            (d + self.delta()) * Self::t()
        }
    }

    /// `R_min = (D − G)·t + w_t`.
    fn r_min(self, rise_mm: f64) -> f64 {
        rise_mm * Self::t() + self.w_t()
    }

    /// The bit as tool `t1`.
    fn tool(self) -> Tool {
        let tool = if self.rho < 1.0 {
            tool_tbn_ultra()
        } else {
            tool_tbn_fine()
        };
        Tool {
            id: ToolId::new("t1"),
            step_over_fraction: Some(0.3),
            ..tool
        }
    }

    fn name(self) -> &'static str {
        if self.rho < 1.0 { "Ultra" } else { "Fine" }
    }

    fn profile(self) -> BitProfile {
        self.tool().carve_profile().expect("every value set")
    }
}

/// `project` with its V-bit `t1` replaced by `bit`.
fn with_bit(mut project: Project, bit: Bench) -> Project {
    project.tools[0] = bit.tool();
    project
}

/// The star part's Inlay with `D`, `G` and `P`, the male board measured.
fn tapered_inlay(depth_mm: f64, glue_gap_mm: f64, plug_depth_mm: f64) -> InlayOp {
    InlayOp {
        depth_mm: Some(depth_mm),
        glue_gap_mm: Some(glue_gap_mm),
        plug_depth_mm: Some(plug_depth_mm),
        step_over_fraction: Some(0.3),
        ..inlay(Board::Male)
    }
}

/// A `w` × `h` rectangle at the origin whose corners are rounded `r`,
/// counter-clockwise.
fn rounded_rect(w: f64, h: f64, r: f64) -> Polyline {
    let bulge = (std::f64::consts::FRAC_PI_4 / 2.0).tan();
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

/// The coupon (design spec, "The taper's ±0.5°"): a 20 mm square rounded
/// 3 mm, at (40, 40) on the star's 100 × 100 × 12 mm stock, carved by
/// `bit`; no rounding needed with either bench bit.
fn coupon(bit: Bench) -> Project {
    let mut p = project_of(
        vec![rounded_rect(20.0, 20.0, 3.0)],
        (40.0, 40.0),
        100.0,
        100.0,
    );
    p.tools[0] = bit.tool();
    p
}

/// The rounded rectangle and a circle beside it, which no bench bit
/// rounds: 40 × 20 mm, corners 4 mm, at (20, 30), and an 8 mm circle at
/// (78, 40).
fn plain_design(bit: Bench) -> Project {
    let mut p = project_of(
        vec![
            rounded_rect(40.0, 20.0, 4.0),
            Polyline::circle(Point::new(58.0, 10.0), 8.0),
        ],
        (20.0, 30.0),
        100.0,
        100.0,
    );
    p.tools[0] = bit.tool();
    p
}

/// The "DC" fixture's project and Inlay with `bit` as its V-bit's tool.
fn dc_with(bit: Bench) -> (Project, InlayOp) {
    let (mut project, op) = fixture(include_str!("../../../../../tests/fixtures/dc.dcv"));
    let tool = project
        .tools
        .iter_mut()
        .find(|t| t.id == op.tool)
        .expect("the fixture's V-bit");
    *tool = Tool {
        id: op.tool.clone(),
        ..bit.tool()
    };
    (project, op)
}

/// The largest distance from a point of the outlines of `a` (arcs
/// flattened at 0.001 mm) to the nearest outline of `b`, mm.
fn outline_departure(a: &[Region], b: &[Region]) -> f64 {
    let walls = outlines(b);
    outlines(a)
        .iter()
        .flat_map(|o| {
            o.with_arcs_flattened(0.001)
                .vertices()
                .iter()
                .map(|v| v.point())
                .collect::<Vec<_>>()
        })
        .map(|p| distance_to(&walls, p))
        .fold(0.0, f64::max)
}

/// `regions` mirrored about X = `axis_x_mm`, nested again.
fn mirrored_regions(regions: &[Region], axis_x_mm: f64) -> Vec<Region> {
    let mirror = Transform::mirror_x_about(axis_x_mm);
    let contours = regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| {
            Contour::new(mirror.apply_polyline(c.polyline()).expect("placed")).expect("closed")
        })
        .collect();
    nest(contours).expect("nests").0
}

/// The female's and the male's (mirrored back) designs of `op` in
/// `project`, as the generator resolves them.
fn both_designs(project: &Project, op: &InlayOp) -> (Vec<Region>, Vec<Region>) {
    let p = with_op(project, Operation::Inlay(op.clone()));
    let female = side(&p, op, Board::Female, &mut Vec::new()).expect("the female resolves");
    let male = side(&p, op, Board::Male, &mut Vec::new()).expect("the male resolves");
    let axis = male.mirror_x_mm.expect("a mirror");
    (female.targets.regions, mirrored_regions(&male.design, axis))
}

/// How far the cones of a bit of `profile` along `toolpath`, at height
/// `z_mm`, stop short of `q`, mm: [`disc_gap_mm`] with the disc
/// `w(z_mm − z_p)` in place of a V-bit's.
fn profile_gap_mm(toolpath: &Toolpath, q: Point, z_mm: f64, profile: &BitProfile) -> f64 {
    let mut best = f64::INFINITY;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        if !mv.is_cut() || !matches!(mv.kind, MoveKind::Linear) {
            continue;
        }
        let b = mv.to;
        let (lo, hi) = if (b.z - a.z).abs() < 1e-12 {
            if a.z > z_mm {
                continue;
            }
            (0.0, 1.0)
        } else {
            let t = (z_mm - a.z) / (b.z - a.z);
            if b.z > a.z {
                (0.0, t.min(1.0))
            } else {
                (t.max(0.0), 1.0)
            }
        };
        if lo > hi {
            continue;
        }
        let at = |t: f64| {
            let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let z = a.z + (b.z - a.z) * t;
            q.distance_to(p) - profile.half_width_mm(z_mm - z)
        };
        let seg = dc_geometry::Segment {
            start: a.xy(),
            end: b.xy(),
            bulge: 0.0,
        };
        let bound = dc_geometry::point_segment_distance_mm(seg, q)
            - profile.half_width_mm(z_mm - a.z.min(b.z));
        if bound >= best {
            continue;
        }
        // Distance is convex along the move and `−w` of a linear height is
        // convex (`w` is concave), so the gap is convex: searched.
        let (mut x0, mut x1) = (lo, hi);
        for _ in 0..80 {
            let m0 = x0 + (x1 - x0) / 3.0;
            let m1 = x1 - (x1 - x0) / 3.0;
            if at(m0) <= at(m1) {
                x1 = m1;
            } else {
                x0 = m0;
            }
        }
        best = best.min(at(lo)).min(at(hi)).min(at((x0 + x1) / 2.0));
    }
    best
}

/// Acceptance 61x: with a tapered ball nose, the female is a V-Carve of
/// its targets with `flat_depth_mm: D` from the stock top, move for move,
/// with and without a clearing pass, on a design the bit need not round.
#[test]
fn tapered_female_equals_a_flat_depth_v_carve() {
    for bit in [Bench::ULTRA, Bench::FINE] {
        let project = plain_design(bit);
        for clearing in [Vec::new(), vec![clearing_pass()]] {
            // A glue gap above either bit's tangent height, so nothing warns
            // of the ball.
            let op = InlayOp {
                clearing: clearing.clone(),
                glue_gap_mm: Some(1.5),
                step_over_fraction: Some(0.3),
                ..inlay(Board::Female)
            };
            let carve = VCarveOp {
                id: op.id.clone(),
                name: op.name.clone(),
                enabled: true,
                targets: op.targets.clone(),
                tool: op.tool.clone(),
                strategy: VCarveStrategy::Medial,
                clearing,
                flat_depth_mm: Some(DEPTH_MM),
                start_depth_mm: 0.0,
                start_plane_cleared_externally: false,
                step_over_fraction: op.step_over_fraction,
                machining_order: op.machining_order,
                through_cut_allowance_mm: op.through_cut_allowance_mm,
                safe_z_mm: op.safe_z_mm,
            };
            let (female, female_warnings) = inlay_runs(&project, &op);
            let (v_carve, v_carve_warnings) =
                generate_vcarve(&with_op(&project, Operation::VCarve(carve.clone())), &carve)
                    .expect("the V-Carve generates");
            assert_eq!(female.len(), v_carve.len(), "{}", bit.name());
            for (a, b) in female.iter().zip(&v_carve) {
                assert_eq!(a.label, b.label);
                assert_eq!(a.toolpath, b.toolpath, "{} {:?}", bit.name(), a.label);
            }
            assert_eq!(female_warnings, v_carve_warnings, "{}", bit.name());
        }
    }
}

/// Acceptance 61x, the fit property with a tapered ball nose: on the star
/// rounded for each bench bit, `D` 4, `G` 1.5, `P` 2, for `h` in `[0, D −
/// G]` the female's wall `h` below the stock top stands `w(D) − w(D − h)`
/// inside the outline and the plug's `h` above its start plane `w(P + h)
/// − w(P)` outside it, read from the two V runs, and the two agree within
/// [`FIT_MM`]. Sampled along the star's edges, 0.3 to 0.7 of each, where
/// the rounding leaves the outline as drawn.
#[test]
fn tapered_walls_mate() {
    let (depth_mm, glue_gap_mm, plug_depth_mm) = (4.0, 1.5, 2.0);
    let rise_mm = depth_mm - glue_gap_mm;
    let corners: Vec<(f64, f64)> = star_points()
        .iter()
        .map(|&(x, y)| (x + CENTRE.0, y + CENTRE.1))
        .collect();
    let samples = edge_samples(&corners, &[0.3, 0.4, 0.5, 0.6, 0.7]);
    for bit in [Bench::ULTRA, Bench::FINE] {
        let project = with_bit(star_project(THICKNESS_MM), bit);
        let op = tapered_inlay(depth_mm, glue_gap_mm, plug_depth_mm);
        let (female, male, warnings) = both_boards(&project, &op);
        let male = mirrored_back(main_run(&male), mirror_axis(&warnings));
        let female = main_run(&female);
        let profile = bit.profile();
        let start_plane_z_mm = THICKNESS_MM - rise_mm;
        let mut worst_mm: f64 = 0.0;
        for &q in &samples {
            for step in 0..=10 {
                let h = rise_mm * f64::from(step) / 10.0;
                let pocket_inset = bit.w(depth_mm) - bit.w(depth_mm - h);
                let plug_inset = bit.w(plug_depth_mm + h) - bit.w(plug_depth_mm);
                let pocket = profile_gap_mm(female, q, THICKNESS_MM - h, &profile);
                let plug = -profile_gap_mm(&male, q, start_plane_z_mm + h, &profile);
                assert!(
                    (pocket - pocket_inset).abs() <= FIT_MM,
                    "{} {q:?} h {h}: pocket {pocket}, w(D) − w(D − h) {pocket_inset}",
                    bit.name()
                );
                assert!(
                    (plug - plug_inset).abs() <= FIT_MM,
                    "{} {q:?} h {h}: plug {plug}, w(P + h) − w(P) {plug_inset}",
                    bit.name()
                );
                assert!(
                    (pocket - plug).abs() <= FIT_MM,
                    "{} {q:?} h {h}: pocket {pocket}, plug {plug}",
                    bit.name()
                );
                worst_mm = worst_mm.max((pocket - plug).abs());
            }
        }
        println!("{}: worst mismatch {worst_mm:.5} mm", bit.name());
    }
}

/// Acceptance 61x: the rounded star and the "DC" letters after
/// `rounded_for_bit` seat within [`SEAT_MM`] by heightfield, with each
/// bench bit, with and without a clearing pass and a cut-out.
#[test]
fn tapered_plug_seats_flush() {
    let star_ops = [
        ("no clearing", tapered_inlay(4.0, 1.0, 2.0)),
        (
            "clearing and cut-out",
            InlayOp {
                clearing: vec![clearing_pass()],
                cut_out: Some(dc_project::InlayCutOut {
                    tool: ToolId::new("t2"),
                    tabs: None,
                    through_cut_allowance_mm: 0.2,
                }),
                ..tapered_inlay(4.0, 1.0, 2.0)
            },
        ),
    ];
    for bit in [Bench::ULTRA, Bench::FINE] {
        let star = with_bit(star_project(THICKNESS_MM), bit);
        for (what, op) in &star_ops {
            let fit = fit_of(&star, op, 0.1);
            report_seat(&format!("{} star, {what}", bit.name()), fit);
        }
        let (dc, dc_op) = dc_with(bit);
        let plain = InlayOp {
            clearing: Vec::new(),
            cut_out: None,
            ..dc_op.clone()
        };
        for (what, op) in [("as the fixture", dc_op), ("no clearing", plain)] {
            let fit = fit_of(&dc, &op, 0.2);
            report_seat(&format!("{} DC, {what}", bit.name()), fit);
        }
    }
}

/// Prints `fit` and asserts it within [`SEAT_MM`].
fn report_seat(name: &str, fit: Fit) {
    println!(
        "{name}: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
        fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
    );
    assert!(
        fit.male_mm <= SEAT_MM,
        "{name}: the male stands {:.3} mm proud of the female's design at {:?}",
        fit.male_mm,
        fit.male_at
    );
    assert!(
        fit.seat_mm <= SEAT_MM,
        "{name}: the plug stands {:.3} mm proud, binding at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// Functional 49k: with the Fine bit and a glue gap of 0.25 mm, below its
/// 1.010 mm tangent height, both boards warn `inlay_glue_gap_below_ball`
/// once, naming the tangent height, the lift `e` (`w_ball(G + e) = (G +
/// δ)·t`, 0.432 mm) and the wall gap `e·t` (0.051 mm). The seat check
/// measures the lift: the coupon's plug stands that far proud, within
/// 0.05 mm.
#[test]
fn glue_gap_below_the_ball_warns() {
    let project = coupon(Bench::FINE);
    let op = tapered_inlay(4.0, 0.25, 2.0);
    let (_, _, warnings) = both_boards(&project, &op);
    let found: Vec<(f64, f64, f64, f64)> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayGlueGapBelowBall {
                glue_gap_mm,
                tangent_height_mm,
                lift_mm,
                wall_gap_mm,
                ..
            } => Some((*glue_gap_mm, *tangent_height_mm, *lift_mm, *wall_gap_mm)),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{warnings:?}");
    let (glue_gap_mm, tangent_height_mm, lift_mm, wall_gap_mm) = found[0];
    assert!((glue_gap_mm - 0.25).abs() < 1e-12);
    assert!(
        (tangent_height_mm - 1.010).abs() < 5e-4,
        "{tangent_height_mm}"
    );
    assert!((lift_mm - 0.432).abs() < 5e-4, "{lift_mm}");
    assert!((wall_gap_mm - 0.051).abs() < 5e-4, "{wall_gap_mm}");
    let warning = warnings
        .iter()
        .find(|w| w.code() == WarningCode::InlayGlueGapBelowBall)
        .expect("the warning");
    assert_eq!(
        warning.field().as_deref(),
        Some("operations[o1].glue_gap_mm")
    );
    let fit = fit_of(&project, &op, 0.1);
    println!(
        "the coupon's plug stands {:.3} mm proud at {:?}",
        fit.seat_mm, fit.seat_at
    );
    assert!(
        (fit.seat_mm - lift_mm).abs() <= 0.05,
        "the seat {:.3} mm, the lift {lift_mm:.3} mm",
        fit.seat_mm
    );
}

/// Functional 49k: with the Fine bit and a plug depth of 0.5 mm, below
/// its 1.010 mm tangent height, `inlay_plug_depth_below_ball` names the
/// widest gap at the plug's wall, `(P + δ)·t − w(P)` = 0.130 mm.
#[test]
fn plug_depth_below_the_ball_warns() {
    let project = coupon(Bench::FINE);
    let op = tapered_inlay(4.0, 1.5, 0.5);
    let (_, _, warnings) = both_boards(&project, &op);
    let found: Vec<(f64, f64, f64)> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayPlugDepthBelowBall {
                plug_depth_mm,
                tangent_height_mm,
                wall_gap_mm,
                ..
            } => Some((*plug_depth_mm, *tangent_height_mm, *wall_gap_mm)),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{warnings:?}");
    let (plug_depth_mm, tangent_height_mm, wall_gap_mm) = found[0];
    assert!((plug_depth_mm - 0.5).abs() < 1e-12);
    assert!((tangent_height_mm - Bench::FINE.h_t()).abs() < 1e-9);
    let expected = (0.5 + Bench::FINE.delta()) * Bench::t() - Bench::FINE.w(0.5);
    assert!((wall_gap_mm - expected).abs() < 1e-9, "{wall_gap_mm}");
    assert!((wall_gap_mm - 0.130).abs() < 5e-4, "{wall_gap_mm}");
    // At the tangent height and above, neither warns.
    let (_, _, warnings) = both_boards(&project, &tapered_inlay(4.0, 1.1, 1.1));
    assert!(
        !warnings.iter().any(|w| matches!(
            w.code(),
            WarningCode::InlayGlueGapBelowBall | WarningCode::InlayPlugDepthBelowBall
        )),
        "{warnings:?}"
    );
}

/// Acceptance 61y: the star's points are rounded on both boards. With the
/// Ultra bit and `D − G` 3, `R_min` is 0.731 mm: each tip of the female's
/// design and of the male's, mirrored back, is an arc of that radius, so
/// the tip stands `R_min (1/S − 1)` back along its bisector (`S` the sine
/// of half the tip's angle), and the two designs are one outline. The
/// generator warns `inlay_rounded_for_bit` once for the points, naming a
/// tip, that distance and `R_min`.
#[test]
fn sharp_point_is_rounded_on_both_boards() {
    let bit = Bench::ULTRA;
    let project = with_bit(star_project(THICKNESS_MM), bit);
    let op = tapered_inlay(4.0, 1.0, 2.0);
    let r_min = bit.r_min(3.0);
    assert!((r_min - 0.731).abs() < 5e-4, "{r_min}");
    let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    let half_angle = (inner / (30.0 - inner)).atan();
    let setback_mm = r_min * (1.0 / half_angle.sin() - 1.0);

    let (female, male) = both_designs(&project, &op);
    let max_x = |regions: &[Region]| {
        outlines(regions)
            .iter()
            .flat_map(|o| o.with_arcs_flattened(0.001).vertices().to_vec())
            .map(|v| v.point().x)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let tip_x = CENTRE.0 + 30.0;
    assert!(
        (max_x(&female) - (tip_x - setback_mm)).abs() < 0.01,
        "the female's tip at X {}, expected {}",
        max_x(&female),
        tip_x - setback_mm
    );
    assert!(
        (max_x(&male) - (tip_x - setback_mm)).abs() < 0.01,
        "the male's tip at X {}",
        max_x(&male)
    );
    let apart = outline_departure(&female, &male).max(outline_departure(&male, &female));
    assert!(apart < 0.01, "the two designs part by {apart} mm");

    let (_, _, warnings) = both_boards(&project, &op);
    let points: Vec<&Warning> = warnings
        .iter()
        .filter(|w| {
            matches!(
                w,
                Warning::InlayRoundedForBit {
                    kind: CornerKind::Point,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(points.len(), 1, "{warnings:?}");
    let Warning::InlayRoundedForBit {
        x_mm,
        y_mm,
        change_mm,
        radius_mm,
        ..
    } = points[0]
    else {
        unreachable!()
    };
    assert!((radius_mm - r_min).abs() < 1e-9, "{radius_mm}");
    assert!(
        (change_mm - setback_mm).abs() < 0.01,
        "{change_mm}, expected {setback_mm}"
    );
    let tips = [(80.0, 50.0), (50.0, 80.0), (20.0, 50.0), (50.0, 20.0)];
    assert!(
        tips.iter()
            .any(|&(x, y)| Point::new(x, y).distance_to(Point::new(*x_mm, *y_mm)) < 0.01),
        "({x_mm}, {y_mm}) is no tip"
    );
}

/// Acceptance 61y: a slot narrower than `2 w_t` is closed on both boards.
/// A 20 mm square at (40, 40) with a slot 0.5 mm wide and 5 mm deep cut in
/// from its top edge, Ultra bit (`2 w_t` 0.757 mm), `D − G` 3: both
/// designs fill the slot, the same outline, the male's mirrored back, and
/// `inlay_rounded_for_bit` names a notch and `w_t`, the slot's bottom
/// moved most.
#[test]
fn narrow_notch_is_closed_on_both_boards() {
    let bit = Bench::ULTRA;
    let slotted = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 20.0),
        (10.25, 20.0),
        (10.25, 15.0),
        (9.75, 15.0),
        (9.75, 20.0),
        (0.0, 20.0),
    ]);
    let mut project = project_of(vec![slotted], (40.0, 40.0), 100.0, 100.0);
    project.tools[0] = bit.tool();
    let op = tapered_inlay(4.0, 1.0, 2.0);
    let (female, male) = both_designs(&project, &op);
    // The slot's bottom is inside both rounded designs.
    for (name, design) in [("female", &female), ("male", &male)] {
        assert!(
            design
                .iter()
                .any(|r| r.contains_point(Point::new(50.0, 57.0))),
            "the {name}'s slot is open"
        );
    }
    let apart = outline_departure(&female, &male).max(outline_departure(&male, &female));
    assert!(apart < 0.01, "the two designs part by {apart} mm");
    let (_, _, warnings) = both_boards(&project, &op);
    let notches: Vec<&Warning> = warnings
        .iter()
        .filter(|w| {
            matches!(
                w,
                Warning::InlayRoundedForBit {
                    kind: CornerKind::Notch,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(notches.len(), 1, "{warnings:?}");
    let Warning::InlayRoundedForBit {
        y_mm,
        change_mm,
        radius_mm,
        ..
    } = notches[0]
    else {
        unreachable!()
    };
    assert!((radius_mm - bit.w_t()).abs() < 1e-9, "{radius_mm}");
    assert!((y_mm - 55.0).abs() < 0.01, "the slot's bottom: Y {y_mm}");
    assert!(*change_mm > 4.0, "{change_mm}");
}

/// Acceptance 61y, the rounding's property: the rounded design is already
/// open by `R_min` and closed by `w_t`, so rounding it again moves it no
/// more than the flatten tolerance (`rounded_for_bit` gives no change),
/// for the star, the "DC" letters and the slotted square, with both bits.
#[test]
fn rounded_design_is_open_and_closed() {
    for bit in [Bench::ULTRA, Bench::FINE] {
        let (dc, dc_op) = dc_with(bit);
        let cases = [
            (
                "star",
                with_bit(star_project(THICKNESS_MM), bit),
                tapered_inlay(4.0, 1.0, 2.0),
            ),
            ("DC", dc, dc_op),
        ];
        for (name, project, op) in cases {
            let p = with_op(&project, Operation::Inlay(op.clone()));
            let female = side(&p, &op, Board::Female, &mut Vec::new()).expect("resolves");
            let again = rounded_design(&p, &op, &female.targets.regions).expect("still rounds");
            assert!(
                again
                    .changes
                    .iter()
                    .all(|c| c.change_mm <= p.flatten_tolerance_mm),
                "{} {name}: {:?}",
                bit.name(),
                again.changes
            );
            let apart = outline_departure(&again.regions, &female.targets.regions)
                .max(outline_departure(&female.targets.regions, &again.regions));
            assert!(
                apart <= p.flatten_tolerance_mm,
                "{} {name}: moved {apart} mm",
                bit.name()
            );
        }
    }
}

/// Acceptance 61y: a 1 mm stroke, narrower than `2 R_min` (1.462 mm with
/// the Ultra bit and `D − G` 3), cannot be rounded without removing it:
/// `InlayTooNarrowForBit` names a part 1.000 mm wide needing 1.462 mm, at
/// the stroke, on either board.
#[test]
fn part_too_narrow_for_the_bit_is_an_error() {
    let bit = Bench::ULTRA;
    let mut project = project_of(
        vec![
            rounded_rect(20.0, 20.0, 3.0),
            Polyline::rect(Point::new(30.0, 5.0), 20.0, 1.0),
        ],
        (20.0, 40.0),
        100.0,
        100.0,
    );
    project.tools[0] = bit.tool();
    for side in [Board::Female, Board::Male] {
        let op = InlayOp {
            depth_mm: Some(4.0),
            glue_gap_mm: Some(1.0),
            ..inlay(side)
        };
        match try_inlay(&project, &op) {
            Err(CamError::InlayTooNarrowForBit {
                kind,
                x_mm,
                y_mm,
                width_mm,
                needed_mm,
                ..
            }) => {
                assert_eq!(kind, CornerKind::Point);
                assert!((width_mm - 1.000).abs() < 5e-4, "{width_mm}");
                assert!((needed_mm - 1.462).abs() < 5e-4, "{needed_mm}");
                assert!((50.0..=70.0).contains(&x_mm), "{side:?}: X {x_mm}");
                assert!((y_mm - 45.5).abs() < 0.6, "{side:?}: Y {y_mm}");
            }
            other => panic!("{side:?}: expected InlayTooNarrowForBit, got {other:?}"),
        }
    }
}

/// Acceptance 61y: two parts 0.5 mm apart, closer than `2 w_t` (0.757 mm
/// with the Ultra bit), would be joined by the closing: `InlayTooNarrowForBit`
/// names a gap 0.500 mm wide needing 0.757 mm, between them.
#[test]
fn gap_too_narrow_for_the_bit_is_an_error() {
    let bit = Bench::ULTRA;
    let mut project = project_of(
        vec![
            rounded_rect(10.0, 10.0, 2.0),
            Polyline::rect(Point::new(10.5, 0.0), 10.0, 10.0),
        ],
        (40.0, 45.0),
        100.0,
        100.0,
    );
    project.tools[0] = bit.tool();
    let op = InlayOp {
        depth_mm: Some(4.0),
        glue_gap_mm: Some(1.0),
        ..inlay(Board::Female)
    };
    match try_inlay(&project, &op) {
        Err(CamError::InlayTooNarrowForBit {
            kind,
            x_mm,
            width_mm,
            needed_mm,
            ..
        }) => {
            assert_eq!(kind, CornerKind::Notch);
            assert!((width_mm - 0.500).abs() < 5e-3, "{width_mm}");
            assert!((needed_mm - 0.757).abs() < 5e-4, "{needed_mm}");
            assert!((x_mm - 50.25).abs() < 0.05, "X {x_mm}");
        }
        other => panic!("expected InlayTooNarrowForBit, got {other:?}"),
    }
}

/// A V-bit Inlay is never rounded: the "DC" fixture's design comes back
/// from `rounded_for_bit` as it went in, and its boards warn nothing of
/// it. (Its programs are held byte for byte by
/// `cli::lettered_inlay_exports_both_boards` and the V-bit digests.)
#[test]
fn v_bit_inlay_is_not_rounded() {
    let (dc, op) = fixture(include_str!("../../../../../tests/fixtures/dc.dcv"));
    let p = with_op(&dc, Operation::Inlay(op.clone()));
    for board in [Board::Female, Board::Male] {
        let resolved = side(&p, &op, board, &mut Vec::new()).expect("resolves");
        let design = match board {
            Board::Female => resolved.targets.regions.clone(),
            Board::Male => resolved.design.clone(),
        };
        let rounded = rounded_design(&p, &op, &design).expect("a V-bit rounds nothing");
        assert_eq!(rounded.regions, design, "{board:?}");
        assert!(rounded.changes.is_empty());
    }
    let (_, _, warnings) = both_boards(&dc, &op);
    assert!(
        !warnings
            .iter()
            .any(|w| w.code() == WarningCode::InlayRoundedForBit),
        "{warnings:?}"
    );
}

/// Acceptance 61y, the band on the profile: with the Ultra bit (`D` 4,
/// `G` 1, `P` 2) the plug's foot is `w(P)` and the flank at the stock top
/// `w(P) + w(D − G + P)` out; the band's edge is open exactly when `M −
/// w(P) ≥ 2r` and `M ≥ w(P) + w(D − G + P)`, else a V wall whose
/// `InlayBandEdgeVWall` names `max(w(P) + 2r, w(P) + w(D − G + P))`; the
/// cut-out needs `w(P) + d` of margin with an open edge and `2 w(P) + d`
/// with a V wall, and runs `w(P) + r` inside a V-walled edge; the board
/// room reads the bit's cut radius, 2.577 mm.
#[test]
fn tapered_band_reads_the_profile() {
    let bit = Bench::ULTRA;
    let mut project = with_bit(star_project(30.0), bit);
    project.tools.push(Tool {
        id: ToolId::new("t3"),
        name: "1 mm end mill".into(),
        diameter_mm: 1.0,
        ..end_mill()
    });
    let foot = bit.w(2.0);
    let band_of = |depth_mm: f64, margin_mm: f64, tool: &str| {
        let op = InlayOp {
            depth_mm: Some(depth_mm),
            margin_mm: Some(margin_mm),
            clearing: vec![ClearingPass {
                tool: ToolId::new(tool),
                ..clearing_pass()
            }],
            male_board: MaleBoard {
                width_mm: None,
                height_mm: None,
                thickness_mm: Some(30.0),
            },
            ..tapered_inlay(depth_mm, 1.0, 2.0)
        };
        (
            dc_project::male_band(&with_op(&project, Operation::Inlay(op.clone())), &op)
                .expect("the band resolves"),
            op,
        )
    };
    // D 4: the flank `w(P) + w(5)` = 1.500 mm, inside any margin the
    // 3.175 mm end mill fits; it fits from `w(P) + 3.175` = 3.749 mm.
    let flank = foot + bit.w(5.0);
    let (band, _) = band_of(4.0, 3.8, "t2");
    assert!((band.foot_mm - foot).abs() < 1e-9, "{}", band.foot_mm);
    assert!((band.flank_mm - flank).abs() < 1e-9, "{}", band.flank_mm);
    assert!(band.is_open());
    assert!(
        (band.v_bit_radius_mm - 2.5767).abs() < 1e-3,
        "{}",
        band.v_bit_radius_mm
    );
    assert!((band.cut_out_margin_needed_mm(3.175) - (foot + 3.175)).abs() < 1e-9);
    assert!((band.cut_out_inset_mm(3.175) - 1.5875).abs() < 1e-9);
    // The V-bit's `w(P) + R_v` with `R_v` its cut radius, or the edge
    // pass's floor ring `w(P) + 2r`, whichever is more.
    assert!((band.board_room_mm(None) - (foot + 2.5767).max(foot + 3.175)).abs() < 1e-3);
    let (band, _) = band_of(4.0, 3.7, "t2");
    assert!(!band.is_open());
    let BandEdge::VWall {
        margin_needed_mm: Some(needed),
    } = band.edge
    else {
        panic!("a V wall with a margin to name: {:?}", band.edge);
    };
    assert!((needed - (foot + 3.175)).abs() < 1e-9, "{needed}");
    assert!((band.cut_out_margin_needed_mm(3.175) - (2.0 * foot + 3.175)).abs() < 1e-9);
    assert!((band.cut_out_inset_mm(3.175) - (foot + 1.5875)).abs() < 1e-9);
    // D 8, the 1 mm end mill: the flank `w(P) + w(9)` = 1.970 mm binds. A
    // margin of 1.8 mm fits the end mill (`M − w(P)` 1.226) but not the
    // flank: a V wall naming 1.970; 2.0 mm opens it.
    let flank = foot + bit.w(9.0);
    let (band, op) = band_of(8.0, 1.8, "t3");
    assert!(!band.is_open(), "{band:?}");
    let BandEdge::VWall {
        margin_needed_mm: Some(needed),
    } = band.edge
    else {
        panic!("a V wall with a margin to name: {:?}", band.edge);
    };
    assert!((needed - flank).abs() < 1e-9, "{needed}, expected {flank}");
    let (_, warnings) = inlay_runs(&project, &op);
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::InlayBandEdgeVWall { margin_needed_mm, .. } if (margin_needed_mm - flank).abs() < 1e-9
        )),
        "{warnings:?}"
    );
    let (band, _) = band_of(8.0, 2.0, "t3");
    assert!(band.is_open());
    // The cut-out's path runs `w(P) + r` inside a V-walled edge.
    let (_, op) = band_of(4.0, 3.7, "t2");
    let op = InlayOp {
        clearing: Vec::new(),
        margin_mm: Some(8.0),
        cut_out: Some(dc_project::InlayCutOut {
            tool: ToolId::new("t2"),
            tabs: None,
            through_cut_allowance_mm: 0.2,
        }),
        ..op
    };
    let p = with_op(&project, Operation::Inlay(op.clone()));
    let male = side(&p.on_male_board(&op), &op, Board::Male, &mut Vec::new());
    let male = male.expect("the male resolves");
    let band = male.band.clone().expect("a band");
    assert!(!band.is_open());
    let cut = cut_out_of(&p, &op);
    let edge: Vec<Polyline> = male
        .band_edge
        .iter()
        .map(|r| r.outer().polyline().clone())
        .collect();
    for point in cut_points(&cut) {
        let gap = distance_to(&edge, point.xy());
        assert!(
            (gap - (foot + 1.5875)).abs() < 0.02,
            "a cut-out point {gap} mm inside the band's edge, expected {}",
            foot + 1.5875
        );
    }
}

/// Acceptance 61y: the male's engagement from its board's top, `D − G +
/// P`, is held to the bit's cutting height: past the Ultra bit's 19.05 mm
/// flutes it is `VBitConeExceeded` naming them, past the Fine bit's
/// 18.374 mm cap its cutting diameter.
#[test]
fn tapered_male_engagement_is_held_to_the_flute_length() {
    for (bit, cone_mm, limit) in [
        (Bench::ULTRA, 19.05, CuttingLimit::Flutes),
        (
            Bench::FINE,
            18.374,
            CuttingLimit::CuttingDiameter { diameter_mm: 6.35 },
        ),
    ] {
        let project = with_bit(star_project(30.0), bit);
        let op = InlayOp {
            male_board: MaleBoard {
                width_mm: None,
                height_mm: None,
                thickness_mm: Some(30.0),
            },
            ..tapered_inlay(12.0, 1.0, 9.0)
        };
        match try_inlay(&project, &op) {
            Err(CamError::VBitConeExceeded {
                depth_mm,
                cone_mm: found,
                limit: named,
                ..
            }) => {
                assert!((depth_mm - 20.0).abs() < 1e-9, "{depth_mm}");
                assert!((found - cone_mm).abs() < 1e-3, "{found}");
                assert_eq!(named, limit, "{}", bit.name());
            }
            other => panic!("{}: expected VBitConeExceeded, got {other:?}", bit.name()),
        }
    }
}

/// How far the plug's start plane stops above the female's surface, mm,
/// held by the plug proper alone: [`fit`]'s seat over the cells inside the
/// female's design, so a plug that sinks is measured where its walls meet
/// the pocket's, not where its band's wall below the start plane would
/// meet the female's top.
fn plug_seat_mm(project: &Project, op: &InlayOp, runs: &[ToolRun], step_mm: f64) -> f64 {
    use crate::test_support::seat::{beyond_band, seat};
    let female_side = side(project, op, Board::Female, &mut Vec::new()).expect("resolves");
    let male_side = side(project, op, Board::Male, &mut Vec::new()).expect("resolves");
    let male_project = project.on_male_board(op);
    let b = outlines(&male_side.design)
        .iter()
        .filter_map(Polyline::bounds)
        .reduce(|a, b| a.including(b.min).including(b.max))
        .expect("a design");
    let grow_mm = male_side.band.as_ref().expect("a band").margin_mm + 2.0;
    let bounds = dc_geometry::Rect {
        min: Point::new(b.min.x - grow_mm, b.min.y - grow_mm),
        max: Point::new(b.max.x + grow_mm, b.max.y + grow_mm),
    };
    let (female_runs, male_runs): (Vec<ToolRun>, Vec<ToolRun>) =
        runs.iter().cloned().partition(|r| r.board == Board::Female);
    let mut female = Heightfield::new(bounds, step_mm, THICKNESS_MM, None);
    female.cut_runs(&female_runs, |run| cutter_of(project, run));
    let mut male = Heightfield::new(bounds, step_mm, THICKNESS_MM, male_side.mirror_x_mm);
    male.cut_runs(&male_runs, |run| cutter_of(&male_project, run));
    let beyond = beyond_band(&male, &male_side.targets.regions);
    seat(&female, &male, male_side.start_plane_z_mm, &beyond)
        .into_iter()
        .filter(|c| {
            female_side
                .targets
                .regions
                .iter()
                .any(|r| r.contains_point(c.at))
        })
        .map(|c| c.need_mm)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// The table of "The taper's ±0.5°" by heightfield: the coupon's pair is
/// generated for the bench's 6.7° but cut by a bit 0.5° wider or narrower
/// (the heightfield's tool), so its plug sinks or stands proud by the seat
/// line's figure, 0.40 mm with the Ultra bit and 0.30 mm with the Fine at
/// `D` 4 and `P` 2, within 0.03 mm (the mean of the two ways, the
/// formula being their first order), measured over the plug proper
/// ([`plug_seat_mm`]).
#[test]
fn seat_shift_matches_the_formula() {
    for (bit, expected_mm) in [(Bench::ULTRA, 0.40), (Bench::FINE, 0.30)] {
        let project = coupon(bit);
        // `G` does not enter the figure; 1.5 mm, the chapter's `h_t + 0.5`
        // for the Fine bit, keeps a sinking plug's top off the pocket's
        // fillet, so every contact is on the cone, where the figure holds.
        let op = tapered_inlay(4.0, 1.5, 2.0);
        let (female, male, _) = both_boards(&project, &op);
        let runs: Vec<ToolRun> = female.into_iter().chain(male).collect();
        let modelled = with_op(&project, Operation::Inlay(op.clone()));
        let cut_by = |taper_deg: f64| {
            let mut p = modelled.clone();
            if let ToolKind::TaperedBallNose {
                taper_angle_deg, ..
            } = &mut p.tools[0].cutter
            {
                *taper_angle_deg = Some(taper_deg);
            }
            plug_seat_mm(&p, &op, &runs, 0.1)
        };
        let nominal = plug_seat_mm(&modelled, &op, &runs, 0.1);
        let wider = cut_by(7.2);
        let narrower = cut_by(6.2);
        println!(
            "{}: seat {nominal:.3} mm as modelled, {wider:.3} cut 0.5° wider, {narrower:.3} 0.5° narrower",
            bit.name()
        );
        let shift = ((narrower - nominal) - (wider - nominal)) / 2.0;
        assert!(
            (shift - expected_mm).abs() <= 0.03,
            "{}: the seat moves {shift:.3} mm per 0.5°, expected {expected_mm}",
            bit.name()
        );
    }
}
