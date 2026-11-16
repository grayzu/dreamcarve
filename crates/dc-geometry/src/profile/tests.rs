// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The bit profile and the exact least room (`docs/specs/phase-3/design.md`,
//! "Test plan → Sub-phase 3.6", the dc-geometry profile group). No reference
//! program exists for a tapered ball nose, so every expected value is
//! analytic, or a dense brute force the exact answer may never exceed.
//! The bench bits are the fixtures' (Fine Detail: tip 2.286 mm, `ρ` 1.143;
//! Ultra-High Detail: tip 0.762 mm, `ρ` 0.381; both 6.7° per side, flutes
//! 19.05 mm, cutting diameter 6.35 mm).

#![allow(
    clippy::cast_precision_loss,
    reason = "sample counts are far below 2^52"
)]

use super::*;
use crate::assert_approx_eq;
use crate::distance::point_segment_distance_mm;
use crate::point::{Point, Vec2};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

/// The start plane every cut here is carved from, mm.
const SP: f64 = 10.0;

/// The bench bits' taper per side, degrees.
const TAPER_DEG: f64 = 6.7;

/// Half the bench bits' 6.35 mm cutting diameter, mm.
const CAP_MM: f64 = 3.175;

fn fine() -> BitProfile {
    BitProfile::TaperedBall {
        tip_radius_mm: 1.143,
        slope: TAPER_DEG.to_radians().tan(),
        cap_radius_mm: CAP_MM,
    }
}

fn ultra() -> BitProfile {
    BitProfile::TaperedBall {
        tip_radius_mm: 0.381,
        slope: TAPER_DEG.to_radians().tan(),
        cap_radius_mm: CAP_MM,
    }
}

fn v_bit(included_deg: f64) -> BitProfile {
    BitProfile::VBit {
        slope: (included_deg / 2.0).to_radians().tan(),
    }
}

/// The disc's radius at depth `d` below the start plane as the room reads
/// it: a V-bit's `d · slope` on either side of the plane (as both
/// existing copies read it), a TBN's `w(d)`.
fn width_mm(profile: &BitProfile, d: f64) -> f64 {
    match *profile {
        BitProfile::VBit { slope } => d * slope,
        BitProfile::TaperedBall { .. } => profile.half_width_mm(d),
    }
}

fn lerp(from: Point3, to: Point3, t: f64) -> Point3 {
    Point3::new(
        from.x + (to.x - from.x) * t,
        from.y + (to.y - from.y) * t,
        from.z + (to.z - from.z) * t,
    )
}

/// The room at `t` of the cut from `from` to `to`, read directly: the
/// tip's distance to the wall less the disc's radius at the start plane.
fn room_at(from: Point3, to: Point3, wall: WallPiece, profile: &BitProfile, t: f64) -> f64 {
    let p = lerp(from, to, t);
    point_segment_distance_mm(wall.segment(), p.xy()) - width_mm(profile, SP - p.z)
}

/// The least of `steps + 1` evenly spaced readings of the room, and
/// where.
fn sampled(
    from: Point3,
    to: Point3,
    wall: WallPiece,
    profile: &BitProfile,
    steps: u32,
) -> (f64, f64) {
    (0..=steps)
        .map(|k| {
            let t = f64::from(k) / f64::from(steps);
            (room_at(from, to, wall, profile, t), t)
        })
        .fold((f64::INFINITY, 0.0), |a, b| if b.0 < a.0 { b } else { a })
}

/// The arc about `centre`, radius `radius_mm`, from `start_deg` sweeping
/// `sweep_deg` (positive counter-clockwise), as a wall piece.
fn arc(centre: Point, radius_mm: f64, start_deg: f64, sweep_deg: f64) -> WallPiece {
    let at = |deg: f64| {
        let a = deg.to_radians();
        Point::new(
            centre.x + radius_mm * a.cos(),
            centre.y + radius_mm * a.sin(),
        )
    };
    WallPiece::from(Segment {
        start: at(start_deg),
        end: at(start_deg + sweep_deg),
        bulge: (sweep_deg.to_radians() / 4.0).tan(),
    })
}

/// The exact room is never above the brute force, is within `close` of
/// it, and is the room read directly at the `τ` it reports.
fn assert_matches_brute_force(
    case: &str,
    (from, to, wall, profile): (Point3, Point3, WallPiece, &BitProfile),
    steps: u32,
    close: f64,
) -> (f64, f64) {
    let (room_mm, t) = least_room(from, to, wall, profile, SP);
    let (fine_mm, fine_t) = sampled(from, to, wall, profile, steps);
    assert!(
        room_mm <= fine_mm + 1e-12,
        "{case}: exact {room_mm} at {t} above sampled {fine_mm} at {fine_t}"
    );
    assert!(
        fine_mm - room_mm <= close,
        "{case}: exact {room_mm} at {t} far below sampled {fine_mm} at {fine_t}"
    );
    let at_mm = room_at(from, to, wall, profile, t);
    assert!(
        (at_mm - room_mm).abs() <= 1e-9,
        "{case}: the room at {t} is {at_mm}, not {room_mm}"
    );
    (room_mm, t)
}

// ---------------------------------------------------------------------------
// The profile
// ---------------------------------------------------------------------------

/// A V-bit's width is `d · tan(α/2)` bit for bit, so no V-bit program can
/// move when the callers switch to the profile, and `height_mm` inverts it.
#[test]
fn v_bit_half_width_is_depth_times_slope() {
    for angle_deg in [15.0, 30.0, 60.0, 90.0, 120.0] {
        let slope = (angle_deg / 2.0_f64).to_radians().tan();
        let profile = BitProfile::VBit { slope };
        for d in [0.0, 1e-9, 0.1, 0.75, 1.299, 3.175, 6.35, 17.3, 40.0] {
            let w = profile.half_width_mm(d);
            assert_eq!(
                w.to_bits(),
                (d * slope).to_bits(),
                "{angle_deg}° at {d}: {w}"
            );
            let h = profile.height_mm(w).unwrap_or(f64::NAN);
            assert_eq!(h.to_bits(), (w / slope).to_bits(), "{angle_deg}° at {d}");
            assert_approx_eq!(h, d, 1e-12);
        }
        assert_approx_eq!(profile.half_width_mm(-1.0), 0.0, 0.0);
        assert_approx_eq!(profile.slope(), slope, 0.0);
        assert_approx_eq!(profile.tangent_height_mm(), 0.0, 0.0);
        assert_approx_eq!(profile.tangent_half_width_mm(), 0.0, 0.0);
        assert_approx_eq!(profile.apex_offset_mm(), 0.0, 0.0);
        assert!(profile.cap_height_mm().is_none(), "{angle_deg}°: no cap");
    }
}

/// The bench bits' derived values ("Algorithms → Tapered ball nose";
/// acceptance 61t to 61z): `h_t = ρ (1 − sin β)`, `w_t = ρ cos β`,
/// `δ = ρ (1/sin β − 1)`, `h_R = R/t − δ`, the cutting height
/// `min(L, h_R)` with flutes `L` 19.05 mm.
#[test]
fn bench_bits_derived_values() {
    let flutes_mm = 19.05;
    // (profile, h_t, w_t, δ, uncapped w(19.05), h_R, H)
    let cases = [
        (fine(), 1.0096, 1.1352, 8.6538, 3.2544, 18.374, 18.374),
        (ultra(), 0.3365, 0.3784, 2.8846, 2.5767, 24.143, 19.05),
    ];
    for (profile, h_t, w_t, delta, w_flutes, h_cap, cutting) in cases {
        let t = profile.slope();
        assert_approx_eq!(t, TAPER_DEG.to_radians().tan(), 1e-15);
        assert_approx_eq!(profile.tangent_height_mm(), h_t, 1e-3);
        assert_approx_eq!(profile.tangent_half_width_mm(), w_t, 1e-3);
        assert_approx_eq!(profile.apex_offset_mm(), delta, 1e-3);
        // Uncapped, the cone at the flutes' top: the virtual V-bit's width.
        assert_approx_eq!((flutes_mm + profile.apex_offset_mm()) * t, w_flutes, 1e-3);
        let cap = profile.cap_height_mm().unwrap_or(f64::NAN);
        assert_approx_eq!(cap, h_cap, 1e-3);
        assert_approx_eq!(flutes_mm.min(cap), cutting, 1e-3);
        assert_approx_eq!(profile.half_width_mm(h_t), w_t, 1e-3);
    }
}

/// Past `h_R` the Fine bit is a cylinder of the 6.35 mm cutting
/// diameter, and no height gives that radius or more.
#[test]
fn half_width_is_capped_at_the_cutting_diameter() {
    let profile = fine();
    assert_approx_eq!(profile.half_width_mm(18.0), 3.131, 1e-3);
    assert_approx_eq!(profile.half_width_mm(18.5), CAP_MM, 0.0);
    assert_approx_eq!(profile.half_width_mm(19.0), CAP_MM, 0.0);
    assert_approx_eq!(profile.half_width_mm(1e6), CAP_MM, 0.0);
    assert!(profile.height_mm(CAP_MM).is_none(), "at the cap");
    assert!(profile.height_mm(CAP_MM + 1.0).is_none(), "past the cap");
    let below = profile.height_mm(3.1).unwrap_or(f64::NAN);
    assert!(below < 18.374, "3.1 mm is below the cap: {below}");
}

/// Ultra: `w(0.1) = √(2 · 0.381 · 0.1 − 0.1²)` in the ball; `w(2) =
/// (2 + δ) · tan 6.7°` on the cone.
#[test]
fn tapered_half_width_is_ball_then_cone() {
    let profile = ultra();
    assert_approx_eq!(profile.half_width_mm(0.1), (0.0762_f64 - 0.01).sqrt(), 1e-4);
    assert_approx_eq!(profile.half_width_mm(0.1), 0.2573, 1e-4);
    assert_approx_eq!(profile.half_width_mm(2.0), 0.5738, 1e-4);
    assert_approx_eq!(profile.half_width_mm(0.0), 0.0, 0.0);
    assert_approx_eq!(profile.half_width_mm(-0.5), 0.0, 0.0);
}

/// At `h_t` the ball's slope `(ρ − d) / w(d)` and the cone's are both
/// `t`, and the two formulas meet.
#[test]
fn tapered_half_width_is_c1_at_the_tangent() {
    for profile in [fine(), ultra()] {
        let BitProfile::TaperedBall { tip_radius_mm, .. } = profile else {
            continue;
        };
        let (h_t, t) = (profile.tangent_height_mm(), profile.slope());
        let w_t = profile.half_width_mm(h_t);
        let ball = (tip_radius_mm - h_t) / w_t;
        // The cone is linear, so a difference across it is its slope.
        let cone = (profile.half_width_mm(h_t + 1.0) - profile.half_width_mm(h_t + 0.5)) / 0.5;
        assert_approx_eq!(ball, t, 1e-9);
        assert_approx_eq!(cone, t, 1e-9);
        let ball_w = (2.0 * tip_radius_mm * h_t - h_t * h_t).sqrt();
        let cone_w = (h_t + profile.apex_offset_mm()) * t;
        assert_approx_eq!(ball_w, cone_w, 1e-12);
        assert_approx_eq!(w_t, profile.tangent_half_width_mm(), 1e-12);
    }
}

/// A TBN with tip radius 0.05–3 mm, taper 1°–30° per side, its cap
/// 0.2–4 mm past the tangent's radius; or one of the bench bits.
fn tapered() -> impl Strategy<Value = BitProfile> {
    prop_oneof![
        Just(fine()),
        Just(ultra()),
        (0.05..3.0_f64, 1.0..30.0_f64, 0.2..4.0_f64).prop_map(|(rho, deg, over)| {
            let slope = deg.to_radians().tan();
            BitProfile::TaperedBall {
                tip_radius_mm: rho,
                slope,
                cap_radius_mm: rho * deg.to_radians().cos() + over,
            }
        }),
    ]
}

/// The seed every run starts from (as `tests/properties.rs`).
const SEED: u64 = 0x0678_d0ca_4e00_0678;

fn config(cases: u32) -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(cases);
    config.rng_seed = RngSeed::Fixed(SEED);
    config.failure_persistence = None;
    config
}

proptest! {
    #![proptest_config(config(512))]

    /// `w⁻¹(w(d)) = d` everywhere below the cap, in the ball and on the
    /// cone.
    #[test]
    fn height_inverts_half_width(profile in tapered(), d in 0.0..40.0_f64) {
        let w = profile.half_width_mm(d);
        let BitProfile::TaperedBall { cap_radius_mm, .. } = profile else {
            return Ok(());
        };
        prop_assume!(w < cap_radius_mm);
        let back = profile.height_mm(w);
        prop_assert!(back.is_some(), "{profile:?} at {d}: w {w} has no height");
        let back = back.unwrap_or(f64::NAN);
        prop_assert!((back - d).abs() <= 1e-9, "{profile:?}: w⁻¹(w({d})) = {back}");
    }

    /// The carved surface never rises faster than the flank: every secant
    /// of `w⁻¹` below the cap is at most `1 / t`.
    #[test]
    fn surface_rises_no_faster_than_the_flank(
        profile in tapered(),
        a in 0.0..1.0_f64,
        b in 0.0..1.0_f64,
    ) {
        let BitProfile::TaperedBall { cap_radius_mm, .. } = profile else {
            return Ok(());
        };
        let (r1, r2) = (a.min(b) * cap_radius_mm, a.max(b) * cap_radius_mm);
        prop_assume!(r2 - r1 >= 1e-4 && r2 < cap_radius_mm);
        let (h1, h2) = (profile.height_mm(r1), profile.height_mm(r2));
        prop_assert!(h1.is_some() && h2.is_some(), "{profile:?}: {r1} {r2}");
        let rise = (h2.unwrap_or(f64::NAN) - h1.unwrap_or(f64::NAN)) / (r2 - r1);
        let t = profile.slope();
        prop_assert!(rise <= (1.0 / t) * (1.0 + 1e-9), "{profile:?}: {rise} > 1/{t}");
        prop_assert!(rise >= 0.0, "{profile:?}: falls, {rise}");
    }
}

// ---------------------------------------------------------------------------
// The least room
// ---------------------------------------------------------------------------

/// Walls of every kind: lines across, beside and along the cuts, a
/// zero-length wall, and arcs seen from outside and inside.
fn walls() -> Vec<WallPiece> {
    vec![
        WallPiece::line(Point::new(0.0, 0.0), Point::new(10.0, 0.0)),
        WallPiece::line(Point::new(5.0, -5.0), Point::new(5.0, 5.0)),
        WallPiece::line(Point::new(-1.0, 3.0), Point::new(2.0, 7.0)),
        WallPiece::line(Point::new(4.0, 2.0), Point::new(4.0, 2.0)),
        arc(Point::new(5.0, 0.0), 5.0, 0.0, 180.0),
        arc(Point::new(5.0, 0.0), 5.0, 0.0, -180.0),
        arc(Point::new(3.0, 3.0), 2.0, 200.0, 90.0),
        arc(Point::new(5.5, 4.0), 7.0, -40.0, 120.0),
    ]
}

/// A move with `d ≥ h_t` throughout cuts what the virtual V-bit cuts: the
/// same room as a V-bit of slope `t` whose tip is `δ` lower.
#[test]
fn least_room_on_the_cone_is_the_virtual_v_bit() {
    for profile in [fine(), ultra()] {
        let (h_t, delta) = (profile.tangent_height_mm(), profile.apex_offset_mm());
        let virtual_v = BitProfile::VBit {
            slope: profile.slope(),
        };
        let cuts = [
            ((-3.0, 2.0), (13.0, 2.5), h_t + 0.2, h_t + 2.5),
            ((5.0, 1.0), (5.0, 9.0), h_t + 3.0, h_t),
            ((-2.0, -2.0), (12.0, 7.0), h_t, h_t + 4.0),
            ((2.0, 1.0), (8.0, 1.5), h_t + 1.0, h_t + 1.0),
            ((4.0, 2.0), (6.0, 3.0), 12.0, h_t + 0.5),
        ];
        for wall in walls() {
            for &((ax, ay), (bx, by), da, db) in &cuts {
                let from = Point3::new(ax, ay, SP - da);
                let to = Point3::new(bx, by, SP - db);
                let (room_mm, _) = least_room(from, to, wall, &profile, SP);
                let lowered = |p: Point3| Point3::new(p.x, p.y, p.z - delta);
                let (v_mm, _) = least_room(lowered(from), lowered(to), wall, &virtual_v, SP);
                assert!(
                    (room_mm - v_mm).abs() <= 1e-12,
                    "{profile:?} {wall:?} {from:?}–{to:?}: {room_mm} vs the virtual V-bit's {v_mm}"
                );
            }
        }
    }
}

/// At a constant depth in the ball the room is the segment distance less
/// `w(d)`, exactly.
#[test]
fn least_room_parallel_to_a_wall_is_distance_less_width() {
    let profile = fine();
    let wall = WallPiece::line(Point::new(-5.0, 0.0), Point::new(15.0, 0.0));
    for d in [0.0, 0.05, 0.3, 0.9] {
        let from = Point3::new(0.0, 1.5, SP - d);
        let to = Point3::new(10.0, 1.5, SP - d);
        let (room_mm, _) = least_room(from, to, wall, &profile, SP);
        let path = Segment {
            start: from.xy(),
            end: to.xy(),
            bulge: 0.0,
        };
        let expected = crate::distance::segment_distance_mm(path, wall.segment())
            - profile.half_width_mm(SP - from.z);
        assert!(room_mm <= expected, "at {d}: {room_mm} above {expected}");
        assert_approx_eq!(room_mm, expected, 1e-12);
        assert_approx_eq!(room_mm, 1.5 - profile.half_width_mm(d), 1e-9);
    }
}

/// A descent through the ball moving away from a straight wall: the
/// ball's width grows fast at first (infinitely fast at the start plane),
/// then slower than the tip leaves the wall, so the room is least inside
/// the cut, where `n′ = r′`. With `n′ = n1`, `d′ = dd` and `u = ρ − d`,
/// `r′ = dd · u / √(ρ² − u²)`, so `u = ρ n1 / √(n1² + dd²)`.
#[test]
fn least_room_against_a_line_is_the_quadratic_root() {
    for (profile, rho) in [(fine(), 1.143), (ultra(), 0.381)] {
        let h_t = profile.tangent_height_mm();
        let wall = WallPiece::line(Point::new(-10.0, 0.0), Point::new(10.0, 0.0));
        for (n0, n1, d0, d1) in [
            (0.5, 0.4, 0.0, h_t),
            (2.0, 1.0, 0.01, 0.9 * h_t),
            (0.2, 0.6, 0.0, 0.5 * h_t),
        ] {
            let from = Point3::new(-1.0, n0, SP - d0);
            let to = Point3::new(1.0, n0 + n1, SP - d1);
            let dd = d1 - d0;
            let u = rho * n1 / n1.hypot(dd);
            let t_star = (rho - u - d0) / dd;
            assert!(
                0.01 < t_star && t_star < 0.99,
                "the case's least is interior: {t_star}"
            );
            let expected = (n0 + n1 * t_star) - (rho * rho - u * u).sqrt();
            let (room_mm, t) = least_room(from, to, wall, &profile, SP);
            assert_approx_eq!(room_mm, expected, 1e-12);
            assert_approx_eq!(t, t_star, 1e-9);
            assert_matches_brute_force(
                &format!("{profile:?} {n0} {n1}"),
                (from, to, wall, &profile),
                1_000_000,
                1e-9,
            );
        }
    }
}

/// The quartic cases, each with its least inside the cut: a wall's end, an
/// arc seen from outside, and an arc seen from inside (where the room is
/// not convex). Each matches a 10⁶-sample brute force and is never above
/// it.
#[test]
fn least_room_against_a_point_and_an_arc() {
    let profile = fine();
    let h_t = profile.tangent_height_mm();
    let cases = [
        (
            "a wall's end",
            WallPiece::line(Point::new(0.0, 0.8), Point::new(0.0, 6.0)),
            Point3::new(-1.0, 0.0, SP),
            Point3::new(2.0, -0.5, SP - h_t),
        ),
        (
            "an arc from outside",
            arc(Point::new(0.0, 3.0), 2.0, 200.0, 140.0),
            Point3::new(-1.5, 0.0, SP),
            Point3::new(1.5, -0.6, SP - 0.95 * h_t),
        ),
        (
            "an arc from inside",
            arc(Point::new(0.0, 0.0), 4.0, -60.0, 120.0),
            Point3::new(2.5, 0.3, SP),
            Point3::new(0.5, -0.2, SP - h_t),
        ),
    ];
    for (case, wall, from, to) in cases {
        let (_, t) = assert_matches_brute_force(case, (from, to, wall, &profile), 1_000_000, 1e-9);
        assert!(0.01 < t && t < 0.99, "{case}: the least is at {t}");
    }
}

/// The cut is split where `d` crosses `h_t`: across the split the least
/// room is the lesser of the two halves', each exact.
#[test]
fn least_room_splits_at_the_tangent_height() {
    for profile in [fine(), ultra()] {
        let h_t = profile.tangent_height_mm();
        for wall in walls() {
            let from = Point3::new(-2.0, 1.0, SP - 0.1 * h_t);
            let to = Point3::new(9.0, 2.5, SP - 3.0 * h_t);
            let t_split = (h_t - 0.1 * h_t) / (2.9 * h_t);
            let split = lerp(from, to, t_split);
            let (whole, _) = least_room(from, to, wall, &profile, SP);
            let (first, _) = least_room(from, split, wall, &profile, SP);
            let (second, _) = least_room(split, to, wall, &profile, SP);
            assert_approx_eq!(whole, first.min(second), 1e-12);
            assert_matches_brute_force(
                &format!("{profile:?} {wall:?}"),
                (from, to, wall, &profile),
                100_000,
                1e-3,
            );
        }
    }
}

/// With `d` 0 at an end the ball's `r′` is infinite there, so no
/// stationary point is sought at it: the end is read as an end, where the
/// room is the distance. A cut along the start plane is that distance all
/// along; a cut from it is never read above the distance at its end, nor
/// above a brute force.
#[test]
fn least_room_at_the_start_plane_end_is_the_distance() {
    for profile in [fine(), ultra()] {
        let wall = WallPiece::line(Point::new(-10.0, 0.0), Point::new(10.0, 0.0));
        let (level, _) = least_room(
            Point3::new(-1.0, 0.7, SP),
            Point3::new(1.0, 0.7, SP),
            wall,
            &profile,
            SP,
        );
        assert_approx_eq!(level, 0.7, 1e-15);
        // Up to the start plane, toward the wall; then down from it, away.
        let deep = SP - profile.tangent_height_mm();
        for (from, to) in [
            (Point3::new(-1.0, 2.0, deep), Point3::new(1.0, 0.4, SP)),
            (Point3::new(-1.0, 0.4, SP), Point3::new(1.0, 2.0, deep)),
            (Point3::new(0.0, 0.4, SP), Point3::new(0.0, 0.4, deep)),
        ] {
            let (room_mm, t) = least_room(from, to, wall, &profile, SP);
            assert!(room_mm.is_finite(), "{from:?}–{to:?}: {room_mm}");
            assert!(room_mm <= 0.4, "{from:?}–{to:?}: {room_mm} at {t}");
            assert_matches_brute_force(
                &format!("{profile:?} {from:?}"),
                (from, to, wall, &profile),
                1_000_000,
                1e-6,
            );
        }
    }
}

/// A cut that is level within [`EPS_MM`](crate::EPS_MM) is read at its
/// deeper end all along: never more room than it has.
#[test]
fn least_room_of_a_nearly_level_cut_reads_its_deeper_end() {
    let profile = fine();
    let wall = WallPiece::line(Point::new(-10.0, 0.0), Point::new(10.0, 0.0));
    let from = Point3::new(-1.0, 0.7, SP - 0.5e-6);
    let to = Point3::new(1.0, 0.7, SP);
    let (room_mm, _) = least_room(from, to, wall, &profile, SP);
    assert_approx_eq!(room_mm, 0.7 - profile.half_width_mm(SP - from.z), 1e-15);
}

/// A level cut across an arc that sweeps more than a half turn crosses
/// it: the room is the disc's radius below zero. `segment_distance_mm`
/// reads such an arc as far away (#686), so the level case also reads the
/// closed forms and keeps the lesser.
#[test]
fn least_room_of_a_level_cut_across_an_arc_past_a_half_turn() {
    for sweep_deg in [170.0, 200.0, 240.0, 300.0, 340.0] {
        // Centred on the downward direction, so every sweep holds both
        // points where the cut's line meets the circle.
        let wall = arc(
            Point::new(0.0, 0.0),
            3.0,
            270.0 - sweep_deg / 2.0,
            sweep_deg,
        );
        for profile in [fine(), v_bit(60.0)] {
            let d = 0.4;
            let from = Point3::new(-5.0, -0.5, SP - d);
            let to = Point3::new(5.0, -0.5, SP - d);
            let (room_mm, _) = least_room(from, to, wall, &profile, SP);
            let width_mm = width_mm(&profile, SP - from.z);
            assert_approx_eq!(room_mm, -width_mm, 1e-9);
        }
    }
}

/// Non-finite input is never read as room.
#[test]
fn least_room_of_non_finite_input_is_no_room() {
    let wall = WallPiece::line(Point::new(-10.0, 0.0), Point::new(10.0, 0.0));
    let p = Point3::new(0.0, 1.0, SP - 0.5);
    for (from, to, sp) in [
        (Point3::new(f64::NAN, 1.0, SP), p, SP),
        (p, Point3::new(1.0, 1.0, f64::INFINITY), SP),
        (p, Point3::new(1.0, 1.0, SP - 0.2), f64::NAN),
    ] {
        let (room_mm, _) = least_room(from, to, wall, &fine(), sp);
        assert!(
            room_mm.is_infinite() && room_mm < 0.0,
            "{from:?}–{to:?}: {room_mm}"
        );
    }
    let broken = BitProfile::TaperedBall {
        tip_radius_mm: f64::NAN,
        slope: 0.1,
        cap_radius_mm: 3.0,
    };
    let (room_mm, _) = least_room(p, Point3::new(1.0, 1.0, SP - 0.2), wall, &broken, SP);
    assert!(
        room_mm.is_infinite() && room_mm < 0.0,
        "a NaN tip: {room_mm}"
    );
}

// ---------------------------------------------------------------------------
// The M1 property
// ---------------------------------------------------------------------------

/// One wall for the property: random lines and arcs, and the degenerate
/// ones: a zero-length wall, a wall whose end lies on the cut (the point
/// quartic then has a double root where the tip passes it), an arc whose
/// centre lies on the cut (the circle's quartic likewise), and an arc that
/// grazes the cut's line a nanometre away.
#[derive(Debug, Clone, Copy)]
enum WallCase {
    Line(Point, Point),
    Arc(Point, Point, f64),
    ZeroLength(Point),
    EndOnCut(f64, Point),
    CentreOnCut(f64, f64, f64, f64),
    Grazing(f64, f64, f64, f64),
}

fn point_in(range: f64) -> impl Strategy<Value = Point> {
    (-range..range, -range..range).prop_map(|(x, y)| Point::new(x, y))
}

fn wall_case() -> impl Strategy<Value = WallCase> {
    prop_oneof![
        3 => (point_in(6.0), point_in(6.0)).prop_map(|(a, b)| WallCase::Line(a, b)),
        3 => (point_in(6.0), point_in(6.0), prop_oneof![-3.0..-0.01_f64, 0.01..3.0_f64])
            .prop_map(|(a, b, bulge)| WallCase::Arc(a, b, bulge)),
        1 => point_in(6.0).prop_map(WallCase::ZeroLength),
        1 => (0.0..=1.0_f64, point_in(6.0)).prop_map(|(s, b)| WallCase::EndOnCut(s, b)),
        1 => (0.0..=1.0_f64, 0.3..6.0_f64, -180.0..180.0_f64, 10.0..300.0_f64)
            .prop_map(|(s, r, a, sweep)| WallCase::CentreOnCut(s, r, a, sweep)),
        1 => (0.0..=1.0_f64, 0.3..6.0_f64, 10.0..120.0_f64, prop::bool::ANY.prop_map(|b| if b { 1.0 } else { -1.0 }))
            .prop_map(|(s, r, sweep, side)| WallCase::Grazing(s, r, sweep, side)),
    ]
}

impl WallCase {
    fn piece(self, from: Point, to: Point) -> WallPiece {
        let on_cut =
            |s: f64| Point::new(from.x + (to.x - from.x) * s, from.y + (to.y - from.y) * s);
        match self {
            Self::Line(a, b) => WallPiece::line(a, b),
            Self::Arc(start, end, bulge) => WallPiece::from(Segment { start, end, bulge }),
            Self::ZeroLength(p) => WallPiece::line(p, p),
            Self::EndOnCut(s, b) => WallPiece::line(on_cut(s), b),
            Self::CentreOnCut(s, r, a, sweep) => arc(on_cut(s), r, a, sweep),
            Self::Grazing(s, r, sweep, side) => {
                // A circle tangent to the cut's line at `on_cut(s)`, 1 nm
                // off it, its arc centred on the near point.
                let along = to - from;
                let normal = along.perp().normalized().unwrap_or(Vec2::new(0.0, 1.0)) * side;
                let centre = on_cut(s) + normal * (r + 1e-6);
                let facing = (-normal.y).atan2(-normal.x).to_degrees();
                arc(centre, r, facing - sweep / 2.0, sweep)
            }
        }
    }
}

/// A profile for the property: a TBN (the bench bits or a random one) or a
/// V-bit of 20°–120° included.
fn any_profile() -> impl Strategy<Value = BitProfile> {
    prop_oneof![
        3 => tapered(),
        1 => (20.0..120.0_f64).prop_map(v_bit),
    ]
}

/// Depths for a cut's ends: through the ball and the cone, at the start
/// plane, at and about the tangent height and the cap height.
fn depth_for(profile: &BitProfile, pick: u8, x: f64) -> f64 {
    let h_t = profile.tangent_height_mm();
    let cap = profile.cap_height_mm().unwrap_or(8.0);
    match pick % 8 {
        0 => 0.0,
        1 => x * h_t,
        2 => h_t + (x - 0.5) * 1e-6,
        3 => cap + (x - 0.5) * 1e-6,
        4 => x * (cap + 1.0),
        5 => x * 3.0 * h_t.max(0.5),
        _ => x * cap,
    }
}

proptest! {
    #![proptest_config(config(1024))]

    /// The M1 guarantee: for random cuts through the ball and the cone
    /// against random walls, the exact least room is never above the
    /// least of 10⁴ evenly spaced readings (+ 1e-12), and it is a real
    /// reading: the room read directly at the `τ` it reports (within
    /// 1e-9). The spec's "≥ the sampling − 1e-6" holds where the room is
    /// smooth; where a cut crosses a wall or leaves the start plane the
    /// room has a corner or a `√d` rise the samples step over, so the
    /// reading at `τ` is the lower check instead.
    #[test]
    fn least_room_is_never_above_a_dense_sampling(
        profile in any_profile(),
        from in point_in(5.0),
        heading in 0.0..std::f64::consts::TAU,
        length in prop_oneof![1 => Just(0.0), 1 => 0.0..1e-3_f64, 6 => 0.0..8.0_f64],
        picks in (0_u8..8, 0_u8..8),
        xs in (0.0..=1.0_f64, 0.0..=1.0_f64),
        wall in wall_case(),
    ) {
        let to_xy = from + Vec2::new(heading.cos(), heading.sin()) * length;
        let (d0, d1) = (depth_for(&profile, picks.0, xs.0), depth_for(&profile, picks.1, xs.1));
        let (a, b) = (Point3::new(from.x, from.y, SP - d0), Point3::new(to_xy.x, to_xy.y, SP - d1));
        let piece = wall.piece(from, to_xy);
        let (room_mm, t) = least_room(a, b, piece, &profile, SP);
        let (fine_mm, fine_t) = sampled(a, b, piece, &profile, 10_000);
        prop_assert!(
            room_mm <= fine_mm + 1e-12,
            "{profile:?} {a:?}–{b:?} {wall:?}: exact {room_mm} at {t} above sampled {fine_mm} at {fine_t}"
        );
        let at_mm = room_at(a, b, piece, &profile, t);
        // A level cut is read at its deeper end, and against today's
        // segment distance, which reads a gap within `EPS_MM` as a touch:
        // both can only read less room than there is.
        let level = (b.z - a.z).abs() <= crate::point::EPS_MM;
        let slack = if level {
            (width_mm(&profile, SP - a.z) - width_mm(&profile, SP - b.z)).abs() + crate::point::EPS_MM
        } else {
            0.0
        };
        prop_assert!(
            room_mm - at_mm <= 1e-9 && at_mm - room_mm <= 1e-9 + slack,
            "{profile:?} {a:?}–{b:?} {wall:?}: the room at {t} is {at_mm}, not {room_mm}"
        );
    }
}
