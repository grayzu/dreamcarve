// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/entry.ts
//   src/engine/toolpaths/entry.test.ts

//! Entry (`docs/specs/phase-2/design.md`, "Algorithms → Entry
//! (dc-cam::entry)"; functional spec capability 16, S9, S10): how the tool
//! reaches each pass depth. Stubbed until the implementation commit.

use crate::toolpath::Move;
use dc_geometry::{Point, Region};
use dc_project::{CutDirection, Entry};

/// Upstream's default descent angle, degrees (`DEFAULT_ENTRY_RAMP_ANGLE`).
pub const DEFAULT_RAMP_ANGLE_DEG: f64 = 5.0;
/// Upstream's default helix diameter as a fraction of the tool diameter.
pub const DEFAULT_HELIX_FRACTION: f64 = 0.8;
/// Straight moves per helix revolution.
pub const HELIX_SEGMENTS_PER_REV: usize = 48;
/// Entry moves stay this fraction of the tool diameter inside the
/// clearance region's boundary.
pub const BOUNDARY_SAFETY_FRACTION: f64 = 0.1;
/// The least boundary safety, mm, for very small tools.
pub const MIN_BOUNDARY_SAFETY_MM: f64 = 1e-4;
/// A ramp run is at most this many tool diameters long.
pub const MAX_RAMP_RUN_DIAMETERS: f64 = 3.0;
/// Most moves one descent may have.
pub const MAX_DESCENT_MOVES: usize = 20_000;
/// Ramp directions tried, over a half circle.
pub const RAMP_DIRECTIONS: usize = 72;
/// Helix centres tried round the target for the tangent placement.
pub const HELIX_TANGENT_ANGLES: usize = 72;
/// Helix radii tried, each half the one before.
pub const MAX_HELIX_RADIUS_ATTEMPTS: usize = 4;
/// Legal helix centres tried by the reachable placement.
pub const MAX_HELIX_ENDPOINT_CANDIDATES: usize = 12;
/// Helix end angles tried per centre by the reachable placement.
pub const HELIX_ENDPOINT_ANGLE_SAMPLES: usize = 24;
/// Most quadtree cells one clearance search visits.
pub const MAX_CLEARANCE_CELLS: usize = 20_000;

/// Which side of the wall being cut the tool is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutSide {
    /// Inside a wall: a pocket, an inside profile, a hole.
    Internal,
    /// Outside a boss: an outside profile.
    External,
}

/// An entry style, as asked for or as used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// Straight down at the plunge rate.
    Plunge,
    /// A zigzag ramp.
    Ramp,
    /// A helix.
    Helix,
}

/// The largest circle inside a clearance region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearanceCircle {
    /// Its centre, mm.
    pub centre: Point,
    /// Its radius, mm.
    pub radius_mm: f64,
}

/// The tool-centre regions an entry may use at one pass.
#[derive(Debug, Clone)]
pub struct EntryDomain {}

impl EntryDomain {
    /// The domain of `regions`.
    #[must_use]
    pub fn new(_regions: &[Region], _tool_diameter_mm: f64, _tolerance_mm: f64) -> Self {
        Self {}
    }

    /// The largest clearance circle of any region.
    #[must_use]
    pub fn largest_clearance_circle(&self) -> Option<ClearanceCircle> {
        None
    }
}

/// The tool and cut an entry is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryTool {
    /// Tool diameter, mm.
    pub diameter_mm: f64,
    /// Cutting feed, mm/min.
    pub feed_mm_min: f64,
    /// Plunge rate, mm/min.
    pub plunge_mm_min: f64,
    /// Climb or conventional.
    pub direction: CutDirection,
    /// Inside a wall or outside a boss.
    pub side: CutSide,
}

/// Requested and used helix diameters, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixDiameter {
    /// The diameter asked for.
    pub requested_mm: f64,
    /// The diameter cut.
    pub used_mm: f64,
}

/// One block's descent.
#[derive(Debug, Clone, PartialEq)]
pub struct Descent {
    /// Where the descent starts.
    pub from: Point,
    /// The descent moves; empty for a plunge.
    pub moves: Vec<Move>,
    /// The entry asked for.
    pub requested: EntryKind,
    /// The entry cut.
    pub used: EntryKind,
    /// The helix diameters, when a helix is cut.
    pub helix: Option<HelixDiameter>,
}

impl Descent {
    /// True when the entry cut is not the one asked for.
    #[must_use]
    pub fn fell_back(&self) -> bool {
        false
    }

    /// The helix diameters when the helix cut is smaller than asked.
    #[must_use]
    pub fn diameter_reduced(&self) -> Option<HelixDiameter> {
        None
    }
}

/// A placed entry.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryPlan {
    target: Point,
}

impl EntryPlan {
    /// The descent from `start_z_mm` to `z_mm`.
    #[must_use]
    pub fn descend(&self, _start_z_mm: f64, _z_mm: f64) -> Descent {
        Descent {
            from: self.target,
            moves: Vec::new(),
            requested: EntryKind::Plunge,
            used: EntryKind::Plunge,
            helix: None,
        }
    }
}

/// Places `entry` at `target`.
#[must_use]
pub fn plan(_domain: &EntryDomain, _entry: &Entry, target: Point, _tool: &EntryTool) -> EntryPlan {
    EntryPlan { target }
}

/// [`plan`] then [`EntryPlan::descend`].
#[must_use]
pub fn synthesize(
    domain: &EntryDomain,
    entry: &Entry,
    target: Point,
    start_z_mm: f64,
    z_mm: f64,
    tool: &EntryTool,
) -> Descent {
    plan(domain, entry, target, tool).descend(start_z_mm, z_mm)
}

/// Helix pitch, mm per revolution.
#[must_use]
pub fn helix_pitch_mm(_path_diameter_mm: f64, _angle_deg: f64) -> f64 {
    0.0
}

/// The plunge-limited feed scale.
#[must_use]
pub fn plunge_limited_feed_scale(_feed_mm_min: f64, _plunge_mm_min: f64, _angle_deg: f64) -> f64 {
    1.0
}

/// True when the helix runs counter-clockwise.
#[must_use]
pub fn helix_is_ccw(_direction: CutDirection, _side: CutSide) -> bool {
    false
}

/// The boundary safety, mm.
#[must_use]
pub fn boundary_safety_mm(_tool_diameter_mm: f64) -> f64 {
    0.0
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "tests"
)]
mod tests {
    //! Ported from PureCutCNC `src/engine/toolpaths/entry.test.ts` unless a
    //! test says otherwise. Upstream's fixture policy is kept: a 4 mm tool,
    //! feed 800, plunge 200, conventional, internal, helix 80 % at 5°.

    use super::*;
    use crate::toolpath::MoveKind;
    use dc_geometry::{
        Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point3, Polyline, assert_approx_eq, nest,
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
}
