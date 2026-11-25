// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/entry.ts
//   src/engine/toolpaths/entry.test.ts

//! Ported from PureCutCNC `src/engine/toolpaths/entry.test.ts` unless a
//! test says otherwise. Upstream's fixture policy is kept: a 4 mm tool,
//! feed 800, plunge 200, conventional, internal, helix 80 % at 5°.

use super::*;
use crate::toolpath::MoveKind;
use dc_geometry::{
    Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point3, Polyline, Vertex, assert_approx_eq, nest,
};
use std::f64::consts::PI;

const D_MM: f64 = 4.0;
const FEED: f64 = 800.0;
const PLUNGE: f64 = 200.0;

fn rect(min: (f64, f64), w: f64, h: f64) -> Polyline {
    Polyline::rect(Point::new(min.0, min.1), w, h)
}

/// The region of `outer` with `islands` as holes.
fn region(outer: Polyline, islands: Vec<Polyline>) -> Region {
    let mut contours = vec![Contour::new(outer).unwrap()];
    contours.extend(islands.into_iter().map(|p| Contour::new(p).unwrap()));
    let (mut regions, _) = nest(contours).unwrap();
    regions.remove(0)
}

fn rectangle(w: f64, h: f64) -> Region {
    region(rect((0.0, 0.0), w, h), Vec::new())
}

/// Upstream's 10 × 10 with the 2 × 2 island at its centre.
fn with_island() -> Region {
    region(
        rect((0.0, 0.0), 10.0, 10.0),
        vec![rect((4.0, 4.0), 2.0, 2.0)],
    )
}

fn domain(region: Region) -> EntryDomain {
    EntryDomain::new(&[region], D_MM, FLATTEN_TOLERANCE_DEFAULT_MM)
}

fn tool() -> EntryTool {
    EntryTool {
        diameter_mm: D_MM,
        feed_mm_min: FEED,
        plunge_mm_min: PLUNGE,
        direction: CutDirection::Conventional,
        side: CutSide::Internal,
        quantum_mm: 0.001,
    }
}

fn helix(fraction: f64, angle_deg: f64) -> Entry {
    Entry::Helix {
        diameter_fraction: fraction,
        angle_deg,
    }
}

fn upstream_helix() -> Entry {
    helix(0.8, 5.0)
}

fn entry(region: Region, entry: &Entry, target: (f64, f64), start_z: f64, z: f64) -> Descent {
    synthesize(
        &domain(region),
        entry,
        Point::new(target.0, target.1),
        start_z,
        z,
        &tool(),
    )
}

/// The descent's start at `start_z`, then every move's end.
fn points(d: &Descent, start_z: f64) -> Vec<Point3> {
    let mut out = vec![Point3::new(d.from.x, d.from.y, start_z)];
    out.extend(d.moves.iter().map(|m| m.to));
    out
}

/// Each move with the point it starts from.
fn steps(d: &Descent, start_z: f64) -> Vec<(Point3, Move)> {
    let pts = points(d, start_z);
    pts.iter().copied().zip(d.moves.iter().copied()).collect()
}

fn descends(from: Point3, mv: &Move) -> bool {
    mv.to.z < from.z - 1e-9
}

fn len3(a: Point3, b: Point3) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
}

fn xy_len(a: Point3, b: Point3) -> f64 {
    a.xy().distance_to(b.xy())
}

/// Every descent ends at the target at the pass Z, every move a
/// `Linear` with a feed: the shape a `CutBlock` descent must have.
fn assert_ends_at_target(d: &Descent, target: (f64, f64), z: f64) {
    if d.moves.is_empty() {
        assert!(d.from.distance_to(Point::new(target.0, target.1)) <= EPS_MM);
        return;
    }
    let last = d.moves.last().unwrap().to;
    assert!(
        last.xy().distance_to(Point::new(target.0, target.1)) <= EPS_MM,
        "descent ends at {last:?}, not the target {target:?}"
    );
    assert_approx_eq!(last.z, z, EPS_MM);
    for (i, m) in d.moves.iter().enumerate() {
        assert_eq!(m.kind, MoveKind::Linear, "move {i}: {m:?}");
        assert!(m.feed_mm_min.is_some_and(|f| f > 0.0), "move {i}: {m:?}");
    }
}

#[test]
fn pitch_from_ramp_angle() {
    let expected = PI * 3.2 * (5.0_f64).to_radians().tan();
    assert_approx_eq!(helix_pitch_mm(3.2, 5.0), expected, 1e-12);
}

/// S10: the scale keeps `feed × scale × sin(angle)` at or under the
/// plunge rate, and every descending helix move's own vertical rate is
/// at or under it too (upstream "limits the entry feed so its vertical
/// component does not exceed plunge feed").
#[test]
fn feed_scale_keeps_vertical_rate_under_plunge() {
    let scale = plunge_limited_feed_scale(1000.0, 80.0, 15.0);
    let vertical = 1000.0 * scale * (15.0_f64).to_radians().sin();
    assert!(vertical <= 80.0 + 1e-9, "vertical feed {vertical}");
    assert!(scale < 1.0, "the scale applies: {scale}");
    assert_approx_eq!(plunge_limited_feed_scale(800.0, 200.0, 5.0), 1.0, 1e-12);

    let fast = EntryTool {
        feed_mm_min: 1000.0,
        plunge_mm_min: 80.0,
        ..tool()
    };
    let d = synthesize(
        &domain(rectangle(20.0, 20.0)),
        &helix(0.8, 15.0),
        Point::new(10.0, 10.0),
        5.0,
        0.0,
        &fast,
    );
    assert_eq!(d.used, EntryKind::Helix);
    let mut descending = 0;
    for (i, (from, mv)) in steps(&d, 5.0).into_iter().enumerate() {
        let f = mv.feed_mm_min.unwrap();
        assert!(f <= 1000.0 + 1e-9, "move {i} feeds above the feed: {f}");
        if descends(from, &mv) {
            descending += 1;
            let vertical = f * (from.z - mv.to.z) / len3(from, mv.to);
            assert!(
                vertical <= 80.0 + 1e-9,
                "move {i} descends at {vertical} mm/min: {mv:?}"
            );
        }
    }
    assert!(descending > 0);
}

#[test]
fn largest_clearance_circle_avoids_island() {
    let circle = domain(with_island()).largest_clearance_circle().unwrap();
    assert!(
        circle.radius_mm > 2.0,
        "the open corners are used: {circle:?}"
    );
    let c = circle.centre;
    assert!(
        !(c.x > 4.0 && c.x < 6.0 && c.y > 4.0 && c.y < 6.0),
        "centre on the island: {circle:?}"
    );
}

/// Upstream asks for 150 %; DreamCarve's validation stops at 100 %, but
/// the entry module still clamps whatever it is given to the tool's
/// diameter (the no-core limit) and reports the reduction.
#[test]
fn helix_diameter_clamped_to_tool_and_warns() {
    let d = entry(
        rectangle(20.0, 20.0),
        &helix(1.5, 5.0),
        (10.0, 10.0),
        6.0,
        0.0,
    );
    assert_eq!(d.used, EntryKind::Helix);
    let reduced = d.diameter_reduced().expect("the clamp is reported");
    assert_approx_eq!(reduced.requested_mm, 6.0, 1e-9);
    assert!(reduced.used_mm <= D_MM + 1e-9, "{reduced:?}");
    assert_ends_at_target(&d, (10.0, 10.0), 0.0);
}

#[test]
fn helix_bounded_by_clearance_and_warns() {
    let d = entry(rectangle(3.0, 3.0), &upstream_helix(), (1.5, 1.5), 4.0, 0.0);
    assert_eq!(d.used, EntryKind::Helix);
    assert!(d.diameter_reduced().is_some(), "{d:?}");
    for p in points(&d, 4.0) {
        assert!(
            (-1e-6..=3.0 + 1e-6).contains(&p.x) && (-1e-6..=3.0 + 1e-6).contains(&p.y),
            "helix point outside the clearance: {p:?}"
        );
    }
    assert_ends_at_target(&d, (1.5, 1.5), 0.0);
}

#[test]
fn helix_stays_outside_islands() {
    let d = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
    assert_eq!(d.used, EntryKind::Helix);
    for p in points(&d, 4.0) {
        assert!(
            !(p.x > 4.0 && p.x < 6.0 && p.y > 4.0 && p.y < 6.0),
            "helix point on the island: {p:?}"
        );
    }
    assert_ends_at_target(&d, (2.0, 5.0), 0.0);
}

#[test]
fn helix_descent_revolutions_plus_flat_bottom() {
    let d = entry(
        rectangle(20.0, 20.0),
        &upstream_helix(),
        (10.0, 10.0),
        5.0,
        0.0,
    );
    assert_eq!(d.used, EntryKind::Helix);
    let revolutions = 5.0 / helix_pitch_mm(3.2, 5.0);
    let want = (revolutions * 48.0).ceil() as usize;
    let s = steps(&d, 5.0);
    let descending = s.iter().filter(|(f, m)| descends(*f, m)).count();
    assert_eq!(descending, want, "descent segments");
    let flat = s
        .iter()
        .filter(|(f, m)| f.z.abs() <= 1e-9 && m.to.z.abs() <= 1e-9 && xy_len(*f, m.to) > 0.0)
        .count();
    assert!(flat >= 48, "a full flat revolution: {flat} flat moves");
    assert_ends_at_target(&d, (10.0, 10.0), 0.0);
}

/// The signed sweep, radians, of the descent's XY path about `c`.
fn sweep_about(d: &Descent, c: Point) -> f64 {
    let pts = points(d, 0.0);
    pts.windows(2)
        .map(|w| {
            let (u, v) = (w[0].xy() - c, w[1].xy() - c);
            u.cross(v).atan2(u.dot(v))
        })
        .sum()
}

/// The centre of the flat bottom revolution: the mean of its 48 ends.
fn flat_centre(d: &Descent, z: f64) -> Point {
    let s = steps(d, f64::INFINITY);
    let flat: Vec<Point> = s
        .iter()
        .filter(|(f, m)| (f.z - z).abs() <= 1e-9 && (m.to.z - z).abs() <= 1e-9)
        .take(HELIX_SEGMENTS_PER_REV)
        .map(|(_, m)| m.to.xy())
        .collect();
    assert_eq!(flat.len(), HELIX_SEGMENTS_PER_REV);
    let n = flat.len() as f64;
    Point::new(
        flat.iter().map(|p| p.x).sum::<f64>() / n,
        flat.iter().map(|p| p.y).sum::<f64>() / n,
    )
}

/// Port of "maps climb/conventional direction for internal and external
/// cuts", sign flipped: upstream returns −1 for an internal climb
/// because its positive rotation is clockwise once its Y-down project
/// space is shown Y-up. DreamCarve is Y-up (ADR-0001), so its positive
/// rotation is counter-clockwise and an internal climb is CCW, as the
/// pocket wall's climb ring is. The emitted helix turns that way.
#[test]
fn helix_sense_climb_inside_is_ccw() {
    assert!(helix_is_ccw(CutDirection::Climb, CutSide::Internal));
    assert!(!helix_is_ccw(CutDirection::Conventional, CutSide::Internal));
    let climb = EntryTool {
        direction: CutDirection::Climb,
        ..tool()
    };
    for (t, ccw) in [(climb, true), (tool(), false)] {
        let d = synthesize(
            &domain(rectangle(20.0, 20.0)),
            &upstream_helix(),
            Point::new(10.0, 10.0),
            5.0,
            0.0,
            &t,
        );
        let sweep = sweep_about(&d, flat_centre(&d, 0.0));
        assert_eq!(sweep > 0.0, ccw, "{:?}: sweep {sweep}", t.direction);
    }
}

/// The external half of the same upstream test, sign flipped for Y-up:
/// outside a boss climb is clockwise, as an outside profile's climb
/// lap is.
#[test]
fn helix_sense_climb_outside_is_cw() {
    assert!(!helix_is_ccw(CutDirection::Climb, CutSide::External));
    assert!(helix_is_ccw(CutDirection::Conventional, CutSide::External));
    let outside_climb = EntryTool {
        direction: CutDirection::Climb,
        side: CutSide::External,
        ..tool()
    };
    let d = synthesize(
        &domain(rectangle(20.0, 20.0)),
        &upstream_helix(),
        Point::new(10.0, 10.0),
        5.0,
        0.0,
        &outside_climb,
    );
    assert!(sweep_about(&d, flat_centre(&d, 0.0)) < 0.0);
}

#[test]
fn narrow_slot_falls_back_to_ramp() {
    let d = entry(
        rectangle(20.0, 0.1),
        &upstream_helix(),
        (0.0, 0.05),
        3.0,
        0.0,
    );
    assert_eq!(d.requested, EntryKind::Helix);
    assert_eq!(d.used, EntryKind::Ramp, "{d:?}");
    assert!(d.fell_back());
    assert!(!d.moves.is_empty(), "the ramp has moves");
    for (i, (from, mv)) in steps(&d, 3.0).into_iter().enumerate() {
        assert!(mv.to.z <= from.z + 1e-9, "move {i} climbs: {mv:?}");
        assert!(
            !(descends(from, &mv) && xy_len(from, mv.to) <= EPS_MM),
            "move {i} is a plunge: {mv:?}"
        );
    }
    assert_ends_at_target(&d, (0.0, 0.05), 0.0);
}

#[test]
fn no_room_falls_back_to_plunge() {
    let d = entry(
        rectangle(0.01, 0.01),
        &upstream_helix(),
        (0.005, 0.005),
        2.0,
        0.0,
    );
    assert_eq!(d.requested, EntryKind::Helix);
    assert_eq!(d.used, EntryKind::Plunge);
    assert!(d.fell_back());
    assert!(d.moves.is_empty(), "a plunge is the sequencer's: {d:?}");
    assert!(d.from.distance_to(Point::new(0.005, 0.005)) <= EPS_MM);
}

/// A 1 % helix at 0.1° needs far more than [`MAX_DESCENT_MOVES`]; the
/// fallback is a ramp whose move count is bounded.
#[test]
fn shallow_angle_move_count_is_bounded() {
    let d = entry(
        rectangle(20.0, 20.0),
        &helix(0.01, 0.1),
        (10.0, 10.0),
        0.0,
        -100.0,
    );
    assert_eq!(d.used, EntryKind::Ramp, "bounded fallback");
    assert!(d.fell_back());
    assert!(
        d.moves.len() < MAX_DESCENT_MOVES + 10,
        "{} moves",
        d.moves.len()
    );
}

#[test]
fn ramp_run_is_capped_at_three_diameters() {
    let ramp = Entry::Ramp { angle_deg: 5.0 };
    let d = entry(rectangle(200.0, 200.0), &ramp, (100.0, 100.0), 2.0, -2.0);
    assert_eq!(d.used, EntryKind::Ramp);
    // Three diameters of XY run, lengthened only by the descent.
    let cap = D_MM * 3.0 / (5.0_f64).to_radians().cos() + 1e-6;
    for (i, (from, mv)) in steps(&d, 2.0).into_iter().enumerate() {
        let l = len3(from, mv.to);
        assert!(l <= cap, "move {i} is {l} mm, over the {cap} mm cap");
    }
    assert_ends_at_target(&d, (100.0, 100.0), -2.0);
}

/// The drop of every descending move of `d` from `start_z`.
fn drops(d: &Descent, start_z: f64) -> Vec<f64> {
    steps(d, start_z)
        .into_iter()
        .filter(|(from, mv)| descends(*from, mv))
        .map(|(from, mv)| from.z - mv.to.z)
        .collect()
}

/// #234 (found by the dc-gcode property test): a ramp over 7.0018
/// legs' depth ended with an eighth leg falling 0.0019 mm over
/// 0.02 mm, which the program writes level at the ramp's
/// plunge-limited feed (S3). Now the last two legs share their
/// 1.0519 mm: each runs 50.09 % of the 12 mm run at 5°, out and back.
/// With 7.6 legs' depth the last leg falls 0.63 mm, more than
/// [`RAMP_MIN_LAST_DROP_MM`], and is cut short at 60 % of the run as
/// upstream's. Either way every leg runs at 5°, none falls less than
/// the least drop, and the drops sum to the depth.
#[test]
fn ramp_has_no_fragment_leg() {
    let ramp = Entry::Ramp { angle_deg: 5.0 };
    let tan = 5.0_f64.to_radians().tan();
    let run_mm = 3.0 * D_MM;
    let full_mm = run_mm * tan;
    let shared = (1.0 + 0.0018) / 2.0 * run_mm;
    let cases = [
        (
            7.0018,
            [
                run_mm, run_mm, run_mm, run_mm, run_mm, run_mm, shared, shared,
            ],
        ),
        (
            7.6,
            [
                run_mm,
                run_mm,
                run_mm,
                run_mm,
                run_mm,
                run_mm,
                run_mm,
                0.6 * run_mm,
            ],
        ),
    ];
    for (legs, runs) in cases {
        let depth_mm = legs * full_mm;
        let d = entry(
            rectangle(200.0, 200.0),
            &ramp,
            (100.0, 100.0),
            depth_mm,
            0.0,
        );
        assert_eq!(d.used, EntryKind::Ramp);
        let sloped: Vec<(f64, f64)> = steps(&d, depth_mm)
            .into_iter()
            .filter(|(from, mv)| descends(*from, mv))
            .map(|(from, mv)| (xy_len(from, mv.to), from.z - mv.to.z))
            .collect();
        assert_eq!(sloped.len(), runs.len(), "{legs}: {sloped:?}");
        for (i, ((run, drop), want_mm)) in sloped.iter().zip(runs).enumerate() {
            assert!(
                (run - want_mm).abs() <= 1e-9
                    && (drop - run * tan).abs() <= 1e-9
                    && *drop >= RAMP_MIN_LAST_DROP_MM,
                "{legs}: leg {i} falls {drop} mm over {run} mm, not {want_mm} mm at 5°"
            );
        }
        let total: f64 = sloped.iter().map(|(_, drop)| drop).sum();
        assert_approx_eq!(total, depth_mm, 1e-9);
        assert_ends_at_target(&d, (100.0, 100.0), 0.0);
    }
}

/// #234's helix half: its `ceil` count of chords splits the depth
/// evenly already (Z linear in the angle), so a depth a sliver over a
/// whole number of chords leaves no fragment either: every chord drops
/// the same, and the drops sum to the depth.
#[test]
fn helix_has_no_fragment_segment() {
    let chord_mm = helix_pitch_mm(3.2, 5.0) / HELIX_SEGMENTS_PER_REV as f64;
    let depth_mm = 100.0018 * chord_mm;
    let d = entry(
        rectangle(20.0, 20.0),
        &upstream_helix(),
        (10.0, 10.0),
        depth_mm,
        0.0,
    );
    assert_eq!(d.used, EntryKind::Helix);
    let drops = drops(&d, depth_mm);
    assert_eq!(drops.len(), 101, "{drops:?}");
    for (i, drop) in drops.iter().enumerate() {
        assert!(
            (drop - depth_mm / 101.0).abs() <= 1e-9,
            "chord {i} drops {drop} mm of {depth_mm} mm"
        );
    }
    assert_approx_eq!(drops.iter().sum::<f64>(), depth_mm, 1e-9);
}

/// #265 (found by the dc-gcode Profile property, `PROPTEST_RNG_SEED=108`):
/// a 0.05 helix of a 0.5508 mm tool is 0.0275 mm across; at 14.14°
/// each of its chords falls 0.00045 mm over 0.0018 mm, less than one
/// unit of a 3-place mm program either way, so most are written level
/// at the plunge-limited feed, which S3 refuses. Such a helix is not
/// cut: the chain falls back to the ramp, reason `too_small`. The same
/// tool's 0.8 helix (0.44 mm across) is a helix, every chord of it
/// falling at least a unit and running at least √2 units in XY, so its
/// ends round apart on both counts.
#[test]
fn tiny_helix_chords_are_writable() {
    let quantum_mm = 0.001;
    let small = EntryTool {
        diameter_mm: 0.550_832_666_619_620_7,
        feed_mm_min: 3_840.280_223_564_292_6,
        plunge_mm_min: 645.988_501_885_48,
        ..tool()
    };
    let domain = EntryDomain::new(
        &[rectangle(20.0, 20.0)],
        small.diameter_mm,
        FLATTEN_TOLERANCE_DEFAULT_MM,
    );
    let target = Point::new(10.0, 10.0);
    let (start_z, z) = (1.436_449_925_824_323_2, 0.0);
    let d = synthesize(
        &domain,
        &helix(0.05, 14.137_048_663_595_39),
        target,
        start_z,
        z,
        &small,
    );
    assert_eq!(d.used, EntryKind::Ramp, "{:?}", d.reason);
    assert!(d.fell_back());
    assert_eq!(d.reason.map(FallbackReason::as_str), Some("too_small"));
    assert_ends_at_target(&d, (10.0, 10.0), z);

    let d = synthesize(
        &domain,
        &helix(0.8, 14.137_048_663_595_39),
        target,
        start_z,
        z,
        &small,
    );
    assert_eq!(d.used, EntryKind::Helix, "{:?}", d.reason);
    let sloped: Vec<(Point3, Move)> = steps(&d, start_z)
        .into_iter()
        .filter(|(from, mv)| descends(*from, mv))
        .collect();
    assert!(!sloped.is_empty());
    for (i, (from, mv)) in sloped.iter().enumerate() {
        let (drop, run) = (from.z - mv.to.z, xy_len(*from, mv.to));
        assert!(
            drop >= quantum_mm && run >= std::f64::consts::SQRT_2 * quantum_mm,
            "chord {i} falls {drop} mm over {run} mm"
        );
    }
}

/// Entry must not score the wall the finish pass leaves, so every ramp
/// point stays a real standoff (0.1 · D) inside the boundary.
#[test]
fn ramp_stays_clear_of_boundary() {
    let ramp = Entry::Ramp { angle_deg: 5.0 };
    let d = entry(rectangle(40.0, 40.0), &ramp, (20.0, 20.0), 2.0, -2.0);
    assert_eq!(d.used, EntryKind::Ramp);
    let standoff = D_MM * 0.1;
    for p in points(&d, 2.0) {
        let to_edge = p.x.min(40.0 - p.x).min(p.y).min(40.0 - p.y);
        assert!(
            to_edge >= standoff - 1e-6,
            "ramp point {p:?} is {to_edge} mm from the wall"
        );
    }
}

/// Upstream "still places a helix when the cut start sits on the
/// region boundary": the tangent placement cannot succeed there, so the
/// bounded reachable search must.
#[test]
fn helix_found_when_target_on_boundary() {
    let d = entry(
        rectangle(40.0, 40.0),
        &upstream_helix(),
        (0.0, 20.0),
        2.0,
        -2.0,
    );
    assert_eq!(d.used, EntryKind::Helix);
    assert!(steps(&d, 2.0).iter().any(|(f, m)| descends(*f, m)));
    assert_ends_at_target(&d, (0.0, 20.0), -2.0);
}

/// Upstream regression #581: the reachable helix sits next to the cut
/// start, not at the region's roomiest spot.
#[test]
fn helix_placed_near_target_not_roomiest_spot() {
    let d = entry(
        rectangle(200.0, 200.0),
        &upstream_helix(),
        (0.0, 100.0),
        2.0,
        -2.0,
    );
    assert_eq!(d.used, EntryKind::Helix);
    let target = Point::new(0.0, 100.0);
    for p in points(&d, 2.0) {
        let away = p.xy().distance_to(target);
        assert!(
            away <= D_MM * 2.0 + 1e-6,
            "helix strayed {away} mm from the cut start: {p:?}"
        );
    }
}

/// #272 (the Pocket property, `PROPTEST_RNG_SEED=108`): the
/// tool-centre region of a ring pocket (outer 13.7528 mm, island
/// 6.8532 mm, by a 3.2349 mm tool, rotated −5.49°), entered by a 0.976
/// helix at 9.316° at a raster start on the island's flattened circle.
/// The reachable placement's handoff to it was checked with the target
/// slack along its whole length, so it ran 0.0051 mm inside the
/// island's chords, 0.0151 mm inside its circle: past the flatten
/// tolerance S9 allows, into the island. The slack is for the piece
/// that ends on the target; the rest of the handoff stays in the
/// flattened region, so it keeps the island's radius less the
/// tolerance.
#[test]
fn helix_handoff_keeps_out_of_the_island() {
    let d_mm = 3.234_923_320_453_882;
    let (c, along) = (
        Point::new(40.0, 120.0),
        Vec2::new(0.995_419_341_093_451_9, -0.095_605_101_208_450_6),
    );
    let circle = |r: f64| {
        let (a, b) = (c + along * -r, c + along * r);
        Polyline::new(
            vec![Vertex::new(a.x, a.y, -1.0), Vertex::new(b.x, b.y, -1.0)],
            true,
        )
    };
    let island_mm = 6.853_196_837_408_963 + d_mm / 2.0;
    let ring = region(
        circle(13.752_766_298_916_859 - d_mm / 2.0),
        vec![circle(island_mm)],
    );
    let tool = EntryTool {
        diameter_mm: d_mm,
        direction: CutDirection::Climb,
        ..tool()
    };
    let target = Point::new(31.983_307, 117.293_382);
    let domain = EntryDomain::new(&[ring], d_mm, FLATTEN_TOLERANCE_DEFAULT_MM);
    let d = synthesize(
        &domain,
        &helix(0.976_100_214_112_066_5, 9.315_963_293_422_808),
        target,
        21.9,
        17.765,
        &tool,
    );
    assert_eq!(d.used, EntryKind::Helix);
    for (i, (from, mv)) in steps(&d, 21.9).iter().enumerate() {
        let near = segment_distance_sq(c, from.xy(), mv.to.xy()).sqrt();
        assert!(
            near >= island_mm - FLATTEN_TOLERANCE_DEFAULT_MM - 1e-9,
            "move {i} passes {near} mm from the island's centre: {mv:?}"
        );
    }
}

/// Not a port: `Entry::Plunge` (every phase 1 project after migration)
/// gives an empty descent at the target, which the sequencer writes as
/// the phase 1 straight plunge, and no fallback.
#[test]
fn plunge_is_default_and_unchanged_from_phase_1() {
    let d = entry(
        rectangle(20.0, 20.0),
        &Entry::Plunge,
        (10.0, 10.0),
        5.0,
        0.0,
    );
    assert_eq!(d.requested, EntryKind::Plunge);
    assert_eq!(d.used, EntryKind::Plunge);
    assert!(!d.fell_back());
    assert!(d.diameter_reduced().is_none());
    assert!(d.moves.is_empty());
    assert!(d.from.distance_to(Point::new(10.0, 10.0)) <= EPS_MM);
}

#[test]
fn boundary_safety_is_a_tenth_of_the_diameter() {
    assert_approx_eq!(boundary_safety_mm(4.0), 0.4, 1e-12);
    assert_approx_eq!(boundary_safety_mm(1e-6), MIN_BOUNDARY_SAFETY_MM, 1e-12);
}

#[test]
fn output_is_deterministic() {
    let a = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
    let b = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
    assert_eq!(a, b);
}

/// The maintainer's tester, 2026-11-24: an 80 mm round pocket cut with a
/// 6 mm bit warned that there was no room for a ramp, but only when the
/// tool came to it from above. A ring that starts on the round wall of
/// the tool-centre region starts on the arc, a hair outside the chords
/// that stand for it; the room for a ramp is then on one side of the
/// start only, and for a start on the upper half that side is behind
/// every direction tried. Wherever on the wall a ring starts, and on
/// every ring inside it, the ramp is placed.
#[test]
fn a_ramp_fits_wherever_a_ring_starts_on_a_round_wall() {
    let disc = region(Polyline::circle(Point::new(0.0, 0.0), 37.0), Vec::new());
    let domain = EntryDomain::new(&[disc], 6.0, FLATTEN_TOLERANCE_DEFAULT_MM);
    let tool = EntryTool {
        diameter_mm: 6.0,
        feed_mm_min: 2000.0,
        plunge_mm_min: 800.0,
        direction: CutDirection::Climb,
        side: CutSide::Internal,
        quantum_mm: 0.001,
    };
    let ramp = Entry::Ramp { angle_deg: 15.0 };
    let mut plunged = Vec::new();
    for ring in 0..14 {
        let radius = 37.0 - 2.7 * f64::from(ring);
        for half_degree in 0..720 {
            let angle = f64::from(half_degree) * PI / 360.0;
            let target = Point::new(radius * angle.cos(), radius * angle.sin());
            let descent = plan(&domain, &ramp, target, &tool).descend(23.0, 12.0);
            if descent.fell_back() {
                plunged.push((ring, half_degree, descent.reason));
                continue;
            }
            // The ramp stays in the disc, clear of its wall, and ends on
            // the ring's start.
            let mut at = descent.from;
            for mv in &descent.moves {
                assert!(
                    at.distance_to(Point::new(0.0, 0.0)) <= 37.0 + EPS_MM,
                    "ring {ring} at {half_degree}: {at:?}"
                );
                at = mv.to.xy();
            }
            assert!(
                at.distance_to(target) <= 1e-6,
                "ring {ring} at {half_degree}"
            );
        }
    }
    assert!(
        plunged.is_empty(),
        "{} starts plunged: {:?}",
        plunged.len(),
        &plunged[..plunged.len().min(8)]
    );
}
