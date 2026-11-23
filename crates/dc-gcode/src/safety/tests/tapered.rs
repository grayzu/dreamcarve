// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17 for a tapered ball nose (#680, slice 27d;
//! `docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose", step
//! 7, "Safety pass", and "Test plan → Sub-phase 3.6", the dc-gcode S17
//! group; functional S17; security review M1).
//!
//! The bench's bits: Ultra-High Detail (tip 0.762 mm) and Fine Detail (tip
//! 2.286 mm), both 6.7° per side, 19.05 mm of flute and a 6.35 mm cutting
//! diameter. The hand-written programs carve on stock [`TOP_MM`] thick
//! with a stock-top origin, so machine Z is project Z less [`TOP_MM`] and
//! the start plane is machine Z 0. The expected values are the tests' own,
//! from the bit's formulas, never `BitProfile`'s.

use super::s17::{
    DATE, Drawing, carve_of, check_run, drawn, export_carve, large_shapes, offset, polygon,
    program_at, small_shapes, tool_of,
};
use super::*;
use crate::emit::EmitOp;
use dc_cam::{Move, RunLabel, Toolpath};
use dc_geometry::{Point, Point3, Polyline, Transform, Vertex};
use dc_project::test_support::{sample_pocket, tool_tbn_fine, tool_tbn_ultra};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InstanceId, OperationId, PocketOp, ToolId, VCarveOp,
};

/// The stock's thickness, and the start plane of every hand-written carve,
/// mm.
const TOP_MM: f64 = 30.0;

/// The slack of the projects here (`material::slack_mm`): one unit in the
/// third decimal, the 0.01 mm flatten and arc-fit tolerances and
/// `EPS_MM`.
const SLACK_MM: f64 = 0.001 + 0.01 + 0.01 + 1e-9;

/// A bench bit by hand: tip radius `ρ`, `t = tan 6.7°`, cap `R` 3.175 mm.
#[derive(Debug, Clone, Copy)]
struct Bench {
    rho: f64,
}

impl Bench {
    const ULTRA: Self = Self { rho: 0.381 };
    const FINE: Self = Self { rho: 1.143 };
    const CAP_MM: f64 = 3.175;
    const FLUTES_MM: f64 = 19.05;

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

    /// `δ = ρ (1/sin β − 1)`.
    fn delta(self) -> f64 {
        self.rho * (1.0 / Self::sin() - 1.0)
    }

    /// `h_R = R/t − δ`.
    fn h_cap(self) -> f64 {
        Self::CAP_MM / Self::t() - self.delta()
    }

    /// `w(d)`: the ball, the cone, the cylinder.
    fn w(self, d: f64) -> f64 {
        if d <= 0.0 {
            0.0
        } else if d <= self.h_t() {
            (2.0 * self.rho * d - d * d).sqrt()
        } else {
            ((d + self.delta()) * Self::t()).min(Self::CAP_MM)
        }
    }

    /// The tool `t2`.
    fn tool(self) -> Tool {
        let tool = if self.rho < 1.0 {
            tool_tbn_ultra()
        } else {
            tool_tbn_fine()
        };
        Tool {
            id: ToolId::new("t2"),
            ..tool
        }
    }
}

/// `drawings` on 200 × 150 mm stock [`TOP_MM`] thick, carved by `bit` as
/// `t2` ([`drawn`]'s V-bit `t2` replaced): the V-Carve `o6` of every
/// instance, medial, no flat depth, at the stock top, the project's only
/// operation.
fn tapered(bit: Bench, drawings: &[Drawing]) -> (Project, VCarveOp) {
    let mut p = drawn(drawings);
    p.stock.thickness_mm = Some(TOP_MM);
    p.tools.retain(|t| t.id.as_str() != "t2");
    p.tools.push(bit.tool());
    let op = carve_of(&p, "t2", |_| {});
    p.operations = vec![Operation::VCarve(op.clone())];
    (p, op)
}

/// [`tapered`] of the 50 × 30 mm rectangle at (10, 10): walls at X 10 and
/// 60, Y 10 and 40.
fn rectangle(bit: Bench) -> (Project, VCarveOp) {
    tapered(
        bit,
        &[(
            vec![Polyline::rect(Point::new(0.0, 0.0), 50.0, 30.0)],
            (10.0, 10.0),
        )],
    )
}

/// S9 and S17 alone on hand-written `text`, as the V-bit run of `op`.
fn check(text: &str, project: &Project, op: &VCarveOp) -> Result<(), ExportError> {
    let tool = tool_of(project, op.tool.as_str());
    check_run(
        text,
        project,
        &Operation::VCarve(op.clone()),
        &RunLabel::Main,
        &tool,
    )
}

/// The line, worst point (project space) and overlap of a
/// [`ExportError::ConeOffMaterial`]; panics on anything else.
#[track_caller]
fn cone_off(result: Result<(), ExportError>) -> (usize, Point3, f64) {
    match result {
        Err(ExportError::ConeOffMaterial {
            line,
            x_mm,
            y_mm,
            z_mm,
            overlap_mm,
            ..
        }) => (line, Point3::new(x_mm, y_mm, z_mm), overlap_mm),
        other => panic!("expected ConeOffMaterial: {other:?}"),
    }
}

/// `x` to three decimals, rounded up: a coordinate as a program writes it,
/// never nearer the wall than `x`.
fn up3(x: f64) -> f64 {
    (x * 1000.0).ceil() / 1000.0
}

/// A cut whose disc at the start plane touches a wall, to the written
/// precision, passes: level along the wall at Y 10 in the ball and on the
/// cone, with both bits; the same cut 0.05 mm nearer the wall is refused
/// by that much.
#[test]
fn tapered_cut_touching_a_wall_passes() {
    for bit in [Bench::ULTRA, Bench::FINE] {
        let (p, op) = rectangle(bit);
        for d in [(bit.h_t() * 500.0).round() / 1000.0, 4.0, 12.0] {
            let y = up3(10.0 + bit.w(d));
            let text = program_at(
                30.0,
                25.0,
                &format!("G1 Z{:.3} F300\nY{y:.3} F1000\nX50\n", -d),
            );
            check(&text, &p, &op).unwrap_or_else(|e| panic!("{bit:?} at {d}: {e}"));
            let near = format!("{:.3}", y - 0.05);
            let text = program_at(
                30.0,
                25.0,
                &format!("G1 Z{:.3} F300\nY{near} F1000\nX50\n", -d),
            );
            let (line, _, overlap_mm) = cone_off(check(&text, &p, &op));
            assert_eq!(line, 7, "{bit:?} at {d}");
            assert!(
                (overlap_mm - (bit.w(d) - (y - 0.05 - 10.0))).abs() <= 1e-6,
                "{bit:?} at {d}: {overlap_mm}"
            );
        }
    }
}

/// M1: a descent all in the Fine bit's ball (`d` from 0 to 1 mm, under
/// `h_t` 1.010), moving away from the wall at Y 10 by 1.25 mm per mm of
/// depth from 0.36 mm off it. Its room is `0.36 + 1.25 d − √(2ρd − d²)`,
/// least where `r′ = 1.25`, at `d* = ρ (1 − 1.25 / √(1 + 1.25²))`: −0.041
/// mm, refused. Read at the old 1 mm sample points along the cut (and its
/// ends) it is at least 0.037 mm, which passes. The worst point reported
/// is the analytic one within 1e-6 mm.
#[test]
fn tapered_ball_zone_overlap_between_samples_is_refused() {
    let bit = Bench::FINE;
    let (p, op) = rectangle(bit);
    let (x0, y0, dx, dy): (f64, f64, f64, f64) = (20.0, 10.36, 1.5, 1.25);
    let room = |tau: f64| (y0 + dy * tau - 10.0) - bit.w(tau);
    let length_mm = dx.hypot(dy);
    for tau in [0.0, 1.0 / length_mm, 1.0] {
        assert!(room(tau) > 0.0, "the sample at {tau} reads {}", room(tau));
    }
    let a: f64 = dy;
    let rho = bit.rho;
    let d_star = rho * (1.0 - a / (1.0 + a * a).sqrt());
    assert!(d_star < bit.h_t());
    let want_mm = -(y0 - 10.0 + a * d_star - rho / (1.0 + a * a).sqrt());
    assert!((want_mm - 0.0409).abs() < 1e-3, "{want_mm}");
    assert!((-room(d_star) - want_mm).abs() < 1e-12);
    let text = program_at(
        x0,
        y0,
        &format!("G1 Z0 F300\nX{} Y{} Z-1 F1000\n", x0 + dx, y0 + dy),
    );
    let (line, at, overlap_mm) = cone_off(check(&text, &p, &op));
    assert_eq!(line, 7);
    assert!((overlap_mm - want_mm).abs() <= 1e-6, "{overlap_mm}");
    let want = Point3::new(x0 + dx * d_star, y0 + dy * d_star, TOP_MM - d_star);
    assert!(
        (at.x - want.x).abs() <= 1e-6 && (at.y - want.y).abs() <= 1e-6,
        "{at:?} vs {want:?}"
    );
    assert!((at.z - want.z).abs() <= 1e-6, "{at:?} vs {want:?}");
}

/// The least of the room of the straight cut `from` → `to` (project
/// space) against the ring of `walls` (centre, radius, whether the tip is
/// inside it), read every 1/400 000 of the way: the brute force the exact
/// test must match.
fn sampled_ring_room(bit: Bench, from: Point3, to: Point3, walls: &[(Point, f64, bool)]) -> f64 {
    const N: u32 = 400_000;
    (0..=N)
        .map(|k| {
            let tau = f64::from(k) / f64::from(N);
            let p = Point::new(
                from.x + (to.x - from.x) * tau,
                from.y + (to.y - from.y) * tau,
            );
            let z = from.z + (to.z - from.z) * tau;
            let wall_mm = walls
                .iter()
                .map(|&(c, r, inside)| {
                    let from_c = p.distance_to(c);
                    if inside { r - from_c } else { from_c - r }
                })
                .fold(f64::INFINITY, f64::min);
            wall_mm - bit.w(TOP_MM - z)
        })
        .fold(f64::INFINITY, f64::min)
}

/// An arc wall is held exactly (M1): a ring about (50, 50), its outer
/// wall a circle of radius 8, its island one of radius 3, drawn as arcs.
/// Descents in the Fine bit's ball and onto its cone, beside the outer
/// wall (inside an arc) and beside the island (outside one), are refused
/// by the least room a 400 000-sample brute force reads, within 1e-5 mm
/// and never by less, at a point on the cut.
#[test]
fn tapered_arc_wall_overlap_is_refused_exactly() {
    let c = Point::new(50.0, 50.0);
    let ring = vec![
        Polyline::circle(Point::new(0.0, 0.0), 8.0),
        Polyline::circle(Point::new(0.0, 0.0), 3.0),
    ];
    let walls = [(c, 8.0, true), (c, 3.0, false)];
    // (from, to) in machine X, Y, Z: beside the outer wall moving round
    // it, and beside the island moving past it, each in the ball and from
    // the ball onto the cone.
    let cuts = [
        ((50.0, 57.62, 0.0), (51.6, 56.6, -1.0)),
        ((47.5, 57.2, 0.0), (52.5, 56.9, -2.0)),
        ((53.25, 49.0, 0.0), (53.4, 51.5, -1.0)),
        ((53.3, 47.5, 0.0), (53.5, 52.5, -2.5)),
    ];
    let bit = Bench::FINE;
    let (p, op) = tapered(bit, &[(ring, (50.0, 50.0))]);
    for ((x0, y0, z0), (x1, y1, z1)) in cuts {
        let from = Point3::new(x0, y0, TOP_MM + z0);
        let to = Point3::new(x1, y1, TOP_MM + z1);
        let want_mm = -sampled_ring_room(bit, from, to, &walls);
        assert!(want_mm > SLACK_MM + 0.01, "{from:?} → {to:?}: {want_mm}");
        let text = program_at(x0, y0, &format!("G1 Z{z0} F300\nX{x1} Y{y1} Z{z1} F1000\n"));
        let (line, at, overlap_mm) = cone_off(check(&text, &p, &op));
        assert_eq!(line, 7, "{from:?} → {to:?}");
        assert!(
            overlap_mm >= want_mm - 1e-9 && overlap_mm - want_mm <= 1e-5,
            "{from:?} → {to:?}: {overlap_mm} against {want_mm}"
        );
        // The worst point is on the cut, and its room is the overlap.
        let tau = (at.z - from.z) / (to.z - from.z);
        assert!((0.0..=1.0).contains(&tau), "{at:?}");
        let on = Point::new(
            from.x + (to.x - from.x) * tau,
            from.y + (to.y - from.y) * tau,
        );
        assert!(on.distance_to(at.xy()) <= 1e-6, "{at:?} off the cut");
    }
}

/// On the cone the TBN is its virtual V-bit, whose point is `δ` below the
/// tip: the Ultra bit 4 mm deep cuts `(4 + δ) t` = 0.809 mm at the start
/// plane, not a V-bit's `4 t` = 0.470 mm. A level cut 0.7 mm off the wall
/// is refused by 0.109 mm, and a ramp from 2 mm deep, 2 mm off it, to 5 mm
/// deep, 0.85 mm off it, by `(5 + δ) t − 0.85` at its deep end.
#[test]
fn tapered_virtual_v_bit_overlap_on_the_cone_is_refused() {
    let bit = Bench::ULTRA;
    let (p, op) = rectangle(bit);
    let t = Bench::t();
    let want_mm = (4.0 + bit.delta()) * t - 0.7;
    assert!(4.0 * t < 0.7 - SLACK_MM, "a V-bit of the taper would pass");
    let (line, at, overlap_mm) = cone_off(check(
        &program_at(30.0, 25.0, "G1 Z-4 F300\nY10.7 F1000\nX50\n"),
        &p,
        &op,
    ));
    assert_eq!(line, 7);
    assert!(
        (overlap_mm - want_mm).abs() <= 1e-9,
        "{overlap_mm} vs {want_mm}"
    );
    assert!((at.z - (TOP_MM - 4.0)).abs() <= 1e-9, "{at:?}");

    let want_mm = (5.0 + bit.delta()) * t - 0.85;
    assert!(want_mm > SLACK_MM + 0.05, "{want_mm}");
    let (line, at, overlap_mm) = cone_off(check(
        &program_at(20.0, 25.0, "G1 Z-2 F300\nY12 F1000\nX40 Y10.85 Z-5\n"),
        &p,
        &op,
    ));
    assert_eq!(line, 8);
    assert!(
        (overlap_mm - want_mm).abs() <= 1e-9,
        "{overlap_mm} vs {want_mm}"
    );
    assert!(
        (at.x - 40.0).abs() <= 1e-9 && (at.y - 10.85).abs() <= 1e-9,
        "{at:?}"
    );
    check(
        &program_at(20.0, 25.0, "G1 Z-2 F300\nY12 F1000\nX40 Y10.93 Z-5\n"),
        &p,
        &op,
    )
    .unwrap();
}

/// The shank's limit is the cutting height `min(L, h_R)`: the Ultra bit's
/// flutes, 19.05 mm (its cap is 24.14 mm up), and the Fine bit's cap,
/// 18.374 mm, where it reaches its 6.35 mm cutting diameter below its
/// flutes. A plunge a hair inside it passes; one past it is
/// `ConeHeightExceeded` naming that height.
#[test]
fn tapered_shank_is_the_cutting_height() {
    for (bit, height_mm) in [(Bench::ULTRA, 19.05), (Bench::FINE, 18.374)] {
        assert!(
            (bit.h_cap().min(Bench::FLUTES_MM) - height_mm).abs() < 1e-3,
            "{bit:?}"
        );
        let (p, op) = rectangle(bit);
        let plunge = |d: f64| program_at(35.0, 25.0, &format!("G1 Z{:.3} F300\n", -d));
        check(&plunge(height_mm - 0.01), &p, &op).unwrap_or_else(|e| panic!("{bit:?}: {e}"));
        match check(&plunge(height_mm + 0.2), &p, &op) {
            Err(ExportError::ConeHeightExceeded {
                line: 6,
                engaged_mm,
                cone_mm,
                ..
            }) => {
                assert!(
                    (engaged_mm - (height_mm + 0.2)).abs() < 1e-6,
                    "{engaged_mm}"
                );
                assert!((cone_mm - height_mm).abs() < 1e-3, "{bit:?}: {cone_mm}");
            }
            other => panic!("{bit:?}: expected ConeHeightExceeded, got {other:?}"),
        }
    }
}

/// S7 widens a TBN's cut by its cut radius `min(w(L), R)`: 2.577 mm for
/// the Ultra bit, whose flutes end below its cap, and 3.175 mm for the
/// Fine. A carve whose region runs to the stock's left edge, cut 1 mm
/// deep (S17 passes) along X 2.6, passes with the Ultra bit and overhangs
/// by 0.575 mm with the Fine; along X 2.5 the Ultra overhangs by 0.077,
/// and along X 3.2 the Fine passes. Through the whole safety pass.
#[test]
fn tapered_stock_edge_reads_the_cut_radius() {
    let cut_radius = |bit: Bench| bit.w(Bench::FLUTES_MM).min(Bench::CAP_MM);
    assert!((cut_radius(Bench::ULTRA) - 2.577).abs() < 1e-3);
    assert!((cut_radius(Bench::FINE) - 3.175).abs() < 1e-9);
    let edge = |bit: Bench, x: f64| {
        let (p, op) = tapered(
            bit,
            &[(
                vec![Polyline::rect(Point::new(0.0, 0.0), 40.0, 80.0)],
                (0.0, 10.0),
            )],
        );
        let tool = tool_of(&p, "t2");
        let z = TOP_MM - 1.0;
        let toolpath = Toolpath {
            moves: vec![
                Move::rapid(Point3::new(x, 40.0, TOP_MM + 5.0)),
                Move::linear(Point3::new(x, 40.0, z), 300.0),
                Move::linear(Point3::new(x, 60.0, z), 1000.0),
                Move::rapid(Point3::new(x, 60.0, TOP_MM + 5.0)),
            ],
        };
        let ops: Vec<EmitOp> = vec![(Operation::VCarve(op), tool, toolpath).into()];
        crate::emit::emit(&p, &ops, DATE).map(|_| ())
    };
    let overhang = |result: Result<(), ExportError>| match result {
        Err(ExportError::OffStock {
            side: "-X",
            overhang_mm,
            ..
        }) => overhang_mm,
        other => panic!("expected OffStock at -X, got {other:?}"),
    };
    edge(Bench::ULTRA, 2.6).unwrap();
    let ultra_mm = overhang(edge(Bench::ULTRA, 2.5));
    assert!(
        (ultra_mm - (cut_radius(Bench::ULTRA) - 2.5)).abs() < 1e-6,
        "{ultra_mm}"
    );
    let fine_mm = overhang(edge(Bench::FINE, 2.6));
    assert!((fine_mm - 0.575).abs() < 1e-6, "{fine_mm}");
    edge(Bench::FINE, 3.2).unwrap();
}

/// Below the cap the TBN is a cylinder of radius `R` 3.175 mm, the end
/// mill's case: with the Fine bit, whose cap (18.374 mm) is its cutting
/// height, a cut 18.385 mm deep (past the cap, within the slack of the
/// shank's limit) 2.675 mm off the wall is refused by 0.5 mm, `R` less
/// the distance, not by the cone's line extended (0.5013); a ramp from the
/// cone (17 mm deep, 4 mm off the wall) onto the cylinder (2.9 mm off it)
/// by 0.275 mm. Moved to `R` off the wall, the level cut passes.
#[test]
fn tapered_cylinder_stretch_is_the_end_mill_test() {
    let bit = Bench::FINE;
    let d = 18.385;
    assert!(d > bit.h_cap() && d < bit.h_cap() + SLACK_MM);
    let cone_line_mm = (d + bit.delta()) * Bench::t();
    assert!(cone_line_mm - Bench::CAP_MM > 1e-3, "{cone_line_mm}");
    let (p, op) = rectangle(bit);
    let (line, at, overlap_mm) = cone_off(check(
        &program_at(30.0, 25.0, "G1 Z-18.385 F300\nY12.675 F1000\nX50\n"),
        &p,
        &op,
    ));
    assert_eq!(line, 7);
    assert!((overlap_mm - 0.5).abs() <= 1e-9, "{overlap_mm}");
    assert!((at.z - (TOP_MM - d)).abs() <= 1e-9, "{at:?}");

    let (line, at, overlap_mm) = cone_off(check(
        &program_at(20.0, 25.0, "G1 Z-17 F300\nY14 F1000\nX40 Y12.9 Z-18.385\n"),
        &p,
        &op,
    ));
    assert_eq!(line, 8);
    assert!((overlap_mm - 0.275).abs() <= 1e-9, "{overlap_mm}");
    assert!(
        (at.x - 40.0).abs() <= 1e-9 && (at.y - 12.9).abs() <= 1e-9,
        "{at:?}"
    );

    check(
        &program_at(30.0, 25.0, "G1 Z-18.385 F300\nY13.175 F1000\nX50\n"),
        &p,
        &op,
    )
    .unwrap();
}

/// The 30 × 10 rectangle of acceptance 61v and the 6 mm square of its
/// offset carve, placed apart.
fn floor_shapes() -> Vec<Drawing> {
    vec![
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 30.0, 10.0)],
            (10.0, 110.0),
        ),
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 6.0, 6.0)],
            (50.0, 110.0),
        ),
    ]
}

/// The bars of slice 27c: 20 × 1.5 (on the cone), 20 × 0.6 (in the
/// ball) and 20 × 5 mm, carved with no flat depth.
fn bars() -> Vec<Drawing> {
    vec![
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 1.5)],
            (10.0, 10.0),
        ),
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 0.6)],
            (40.0, 10.0),
        ),
        (
            vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 5.0)],
            (70.0, 10.0),
        ),
    ]
}

/// A `w` × `h` rectangle whose corners are rounded `r`.
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

/// The project in `bytes` (a committed fixture) with `bit` as `t9`, and
/// its V-Carve "Lettering", or a V-Carve of its Inlay's targets in place
/// of its operations, with that bit at a 2 mm flat depth.
fn fixture_carve(bytes: &[u8], bit: Bench) -> (Project, VCarveOp) {
    let mut p = dc_project::load(bytes).expect("the fixture loads").project;
    let tool = Tool {
        id: ToolId::new("t9"),
        ..bit.tool()
    };
    p.tools.push(tool);
    let carve = p.operations.iter().find_map(|o| match o {
        Operation::VCarve(op) if op.name == "Lettering" => Some(op.clone()),
        _ => None,
    });
    let inlay_targets = p.operations.iter().find_map(|o| match o {
        Operation::Inlay(op) => Some(op.targets.clone()),
        _ => None,
    });
    let op = match (&carve, inlay_targets) {
        (Some(op), _) => op.clone(),
        (None, Some(targets)) => VCarveOp {
            targets,
            ..dc_project::test_support::sample_v_carve()
        },
        (None, None) => panic!("the fixture has no carve"),
    };
    if carve.is_none() {
        p.operations.clear();
    }
    let op = VCarveOp {
        tool: ToolId::new("t9"),
        flat_depth_mm: Some(2.0),
        step_over_fraction: Some(0.3),
        ..op
    };
    (p, op)
}

/// Every carve slice 27c generates with a TBN passes S17 and the
/// clearing run's S9, with both bench bits: the sign's lettering, the
/// "DC" letters and the star at a 2 mm flat depth (`dc-cam`'s
/// `tapered_cuts_pass_the_exact_cone_test`); the bars with no flat depth;
/// the 61v rectangle with a clearing pass (offset plunging, raster
/// ramping) and the 6 mm square's offset rings; a square with sharp and
/// with 0.5 mm rounded corners; S17's own shapes with floors, medial and
/// offset, from a start plane a Pocket cleared and one the user vouches
/// for; and the Fine bit's 20 × 6 bar 16.884 mm deep from a start plane
/// 1.4 mm down, engaged 18.284 mm, a hair inside its cutting height.
#[test]
fn generated_tapered_carves_pass_s17() {
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
    let pass = |tool: &str, strategy: ClearanceStrategy, entry: Entry| ClearingPass {
        tool: ToolId::new(tool),
        strategy,
        direction: CutDirection::Climb,
        entry,
    };
    let floor = |f: f64| {
        move |op: &mut VCarveOp| {
            op.flat_depth_mm = Some(f);
            op.step_over_fraction = Some(0.3);
        }
    };
    for bit in [Bench::ULTRA, Bench::FINE] {
        let mut cases: Vec<(String, Project, VCarveOp)> = Vec::new();
        let (p, op) = fixture_carve(
            include_bytes!("../../../../../tests/fixtures/sign.dcv"),
            bit,
        );
        cases.push(("the sign's lettering".into(), p, op));
        let (p, op) = fixture_carve(include_bytes!("../../../../../tests/fixtures/dc.dcv"), bit);
        cases.push(("the DC letters".into(), p, op));
        let (p, op) = tapered(bit, &[(vec![star.clone()], (20.0, 20.0))]);
        cases.push(("the star".into(), p, carve_of_with(&op, floor(2.0))));

        let (p, op) = tapered(bit, &bars());
        cases.push(("the bars".into(), p, op));

        // `drawn`'s end mills: `t3` 3.175 mm and `t4` 1.5 mm.
        let (p, _) = tapered(bit, &floor_shapes());
        let floor_rect = vec![InstanceId::new("i1")];
        let mut clearing = carve_of(&p, "t2", floor(2.0));
        clearing.targets.clone_from(&floor_rect);
        clearing.clearing = vec![
            pass("t3", ClearanceStrategy::Offset, Entry::Plunge),
            pass("t4", ClearanceStrategy::Offset, Entry::Plunge),
        ];
        cases.push(("61v with offset clearing".into(), p.clone(), clearing));
        let mut raster = carve_of(&p, "t2", floor(2.0));
        raster.targets.clone_from(&floor_rect);
        raster.strategy = offset(0.4, CutDirection::Conventional);
        raster.clearing = vec![pass(
            "t3",
            ClearanceStrategy::Raster { angle_deg: 0.0 },
            Entry::Ramp { angle_deg: 10.0 },
        )];
        cases.push(("61v offset with raster clearing".into(), p.clone(), raster));
        let mut rings = carve_of(&p, "t2", floor(10.0));
        rings.targets = vec![InstanceId::new("i2")];
        rings.strategy = offset(0.2, CutDirection::Climb);
        cases.push(("61v's offset square".into(), p, rings));

        let (p, op) = tapered(
            bit,
            &[
                (
                    vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)],
                    (10.0, 10.0),
                ),
                (vec![rounded_rect(20.0, 20.0, 0.5)], (40.0, 10.0)),
            ],
        );
        cases.push(("squares".into(), p, carve_of_with(&op, floor(2.0))));

        let mut both = small_shapes();
        both.extend(large_shapes());
        let (p, op) = tapered(bit, &both);
        cases.push((
            "S17's shapes, medial floor".into(),
            p.clone(),
            carve_of_with(&op, floor(1.5)),
        ));
        cases.push((
            "S17's shapes, offset floor".into(),
            p.clone(),
            carve_of_with(&op, |op| {
                floor(1.5)(op);
                op.strategy = offset(0.5, CutDirection::Climb);
            }),
        ));
        cases.push((
            "S17's shapes, vouched start plane".into(),
            p,
            carve_of_with(&op, |op| {
                floor(2.0)(op);
                op.start_depth_mm = 1.5;
                op.start_plane_cleared_externally = true;
            }),
        ));

        let (mut pocketed, op) = tapered(bit, &small_shapes());
        let around = FeatureDefinition {
            id: DefinitionId::new("dp"),
            name: "Around".into(),
            shape: None,
            profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 190.0, 30.0)],
        };
        pocketed.definitions.insert(around.id.clone(), around);
        pocketed.instances.push(FeatureInstance {
            id: InstanceId::new("ip"),
            definition: DefinitionId::new("dp"),
            transform: Transform::translate(5.0, 5.0),
            name: "Around".into(),
            visible: true,
            locked: false,
            construction: false,
            folder: None,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        pocketed.operations = vec![Operation::Pocket(PocketOp {
            id: OperationId::new("o2"),
            targets: vec![InstanceId::new("ip")],
            depth_mm: Some(2.0),
            ..sample_pocket()
        })];
        let cleared = carve_of_with(&op, |op| {
            floor(1.0)(op);
            op.start_depth_mm = 2.0;
        });
        cases.push(("in a Pocket".into(), pocketed, cleared));

        if bit.rho > 1.0 {
            let (p, op) = tapered(
                bit,
                &[(
                    vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 6.0)],
                    (10.0, 10.0),
                )],
            );
            let deep = carve_of_with(&op, |op| {
                op.start_depth_mm = 1.4;
                op.start_plane_cleared_externally = true;
            });
            cases.push(("the 20 × 6 bar near the cap".into(), p, deep));
        }

        for (name, project, op) in cases {
            if let Err(e) = export_carve(&project, &op) {
                panic!("{bit:?}, {name}: the safety pass refuses a generated carve: {e}\n{e:?}");
            }
        }
    }
}

/// `op` edited by `edit`.
fn carve_of_with(op: &VCarveOp, edit: impl FnOnce(&mut VCarveOp)) -> VCarveOp {
    let mut op = op.clone();
    edit(&mut op);
    op
}
