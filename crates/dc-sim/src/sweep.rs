// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The lowest a cutter's surface reaches over a point while its tip runs
//! along a move (`docs/specs/phase-6/design.md`, "Algorithms → The cut of
//! one move"): for a point `q` and a straight move `a → b`,
//!
//! ```text
//! low(q) = min over t in [0, 1] of  z(t) + h(|q − p(t)|)
//! ```
//!
//! with `h` the surface's height above the tip, infinite past the
//! cutter's radius. `h` is convex and increasing and the distance is
//! convex in `t`, so the least is at an end of the move or at one
//! stationary point, and each shape's is found in closed form. A move
//! that falls as it goes is therefore cut to the lowest pose that covers
//! the point, not to the pose nearest it.
//!
//! These are the closed forms phase 3 wrote for its tests
//! (`dc_cam::heightfield`, #452, #456, #679), moved here to be the
//! simulation's kernel; `dc-cam`'s tests call them still. A `LineSweep`
//! is the same arithmetic with a move's own values worked out once, for
//! the many cells a move is tested against.
//!
//! Everything is Y-up, Z-up, millimetres; nothing here has a handedness.

use dc_geometry::profile::BitProfile;
use dc_geometry::{Point, Point3};

use crate::cutter::{Cutter, RIM_SLACK_MM};

/// Under this length in XY, mm, a move is a plunge: it has no direction.
const PLUNGE_MM: f64 = 1e-12;

/// The lowest a V-bit cone of `slope` reaches over `q` while its tip runs
/// straight from `a` to `b`: the least over `t` of `z(t) + |q − p(t)| /
/// slope`, which is convex in `t`, so its stationary point clamped to the
/// move is its least. The cone has no cap: its height is the caller's
/// limit.
#[must_use]
pub fn cone_low_mm(a: Point3, b: Point3, q: Point, slope: f64) -> f64 {
    let at = |t: f64| {
        let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        a.z + (b.z - a.z) * t + q.distance_to(p) / slope
    };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length < PLUNGE_MM {
        return at(if a.z <= b.z { 0.0 } else { 1.0 });
    }
    let (ux, uy) = (dx / length, dy / length);
    let (qx, qy) = (q.x - a.x, q.y - a.y);
    // Along the move to the foot of `q`, mm, and the distance off it.
    let along = qx * ux + qy * uy;
    let off = (qx * uy - qy * ux).abs();
    // `f'(t) = dz + L·w / (slope·√(off² + w²))` with `w = L(t − u)`.
    let k = -(b.z - a.z) * slope / length;
    if k.abs() >= 1.0 {
        return at(0.0).min(at(1.0));
    }
    let w = k * off / (1.0 - k * k).sqrt();
    at(((along + w) / length).clamp(0.0, 1.0))
}

/// The lowest an end mill's flat bottom of `radius_mm` reaches over `q`
/// while its centre runs straight from `a` to `b`; infinite when it never
/// covers `q`.
#[must_use]
pub fn disc_low_mm(a: Point3, b: Point3, q: Point, radius_mm: f64) -> f64 {
    LineSweep::new(a, b).disc_low_mm(q, radius_mm)
}

/// The lowest a bit of `profile` reaches over `q` while its tip runs
/// straight from `a` to `b`: a V-bit's uncapped cone ([`cone_low_mm`]),
/// or a tapered ball nose's ball and cone, infinite past its cap.
#[must_use]
pub fn profile_low_mm(a: Point3, b: Point3, q: Point, profile: &BitProfile) -> f64 {
    match *profile {
        BitProfile::VBit { slope } => cone_low_mm(a, b, q, slope),
        BitProfile::TaperedBall { cap_radius_mm, .. } => {
            LineSweep::new(a, b).lathe_low_mm(q, &Lathe::tapered(profile, cap_radius_mm))
        }
    }
}

/// A turned profile in three zones out from the axis: a ball of radius
/// `rho` out to `w_t`, a cone of `slope` whose point is `delta` below the
/// tip out to `cap`, and nothing past `cap`. A V-bit has no ball (`rho`
/// and `w_t` 0, `delta` 0); a ball nose has no cone (`w_t` at `cap`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lathe {
    rho: f64,
    w_t: f64,
    delta: f64,
    slope: f64,
    cap: f64,
}

impl Lathe {
    /// A tapered ball nose's zones, capped at `cap`.
    fn tapered(profile: &BitProfile, cap: f64) -> Self {
        let rho = match *profile {
            BitProfile::TaperedBall { tip_radius_mm, .. } => tip_radius_mm,
            BitProfile::VBit { .. } => 0.0,
        };
        Self {
            rho,
            w_t: profile.tangent_half_width_mm(),
            delta: profile.apex_offset_mm(),
            slope: profile.slope(),
            cap,
        }
    }

    /// The zones of a cutter that is turned, with the rim's slack on its
    /// cap; `None` for a flat end, which [`LineSweep::disc_low_mm`] cuts.
    pub(crate) fn of(cutter: &Cutter) -> Option<Self> {
        match *cutter {
            Cutter::Flat { .. } => None,
            Cutter::Ball { radius_mm } => Some(Self {
                rho: radius_mm,
                w_t: radius_mm + RIM_SLACK_MM,
                delta: 0.0,
                slope: 1.0,
                cap: radius_mm + RIM_SLACK_MM,
            }),
            Cutter::Vee { slope, radius_mm } => Some(Self {
                rho: 0.0,
                w_t: 0.0,
                delta: 0.0,
                slope,
                cap: radius_mm + RIM_SLACK_MM,
            }),
            Cutter::TaperedBall { radius_mm, .. } => cutter
                .tapered()
                .map(|profile| Self::tapered(&profile, radius_mm + RIM_SLACK_MM)),
        }
    }

    /// `w⁻¹(r)`: the surface's height above the tip `r` from the axis,
    /// the cone's top edge at the cap and no reach beyond it.
    fn height(&self, r: f64) -> f64 {
        if r > self.cap {
            f64::INFINITY
        } else if r <= self.w_t {
            self.rho - ((self.rho - r) * (self.rho + r)).max(0.0).sqrt()
        } else {
            r / self.slope - self.delta
        }
    }
}

/// A straight move with its direction and length worked out once.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineSweep {
    a: Point3,
    dx: f64,
    dy: f64,
    dz: f64,
    /// The move's length in XY, mm.
    length: f64,
    /// Its direction in XY; unused for a plunge.
    ux: f64,
    uy: f64,
}

impl LineSweep {
    /// The move `a → b`.
    pub(crate) fn new(a: Point3, b: Point3) -> Self {
        let (dx, dy, dz) = (b.x - a.x, b.y - a.y, b.z - a.z);
        let length = dx.hypot(dy);
        let (ux, uy) = if length < PLUNGE_MM {
            (0.0, 0.0)
        } else {
            (dx / length, dy / length)
        };
        Self {
            a,
            dx,
            dy,
            dz,
            length,
            ux,
            uy,
        }
    }

    /// Along the move to the foot of `q`, mm, and the distance off it.
    fn foot(&self, q: Point) -> (f64, f64) {
        let (qx, qy) = (q.x - self.a.x, q.y - self.a.y);
        (
            qx * self.ux + qy * self.uy,
            (qx * self.uy - qy * self.ux).abs(),
        )
    }

    /// As the free [`disc_low_mm`].
    pub(crate) fn disc_low_mm(&self, q: Point, radius_mm: f64) -> f64 {
        let a = self.a;
        if self.length < PLUNGE_MM {
            return if q.distance_to(a.xy()) <= radius_mm {
                a.z.min(a.z + self.dz)
            } else {
                f64::INFINITY
            };
        }
        let (along, off) = self.foot(q);
        if off > radius_mm {
            return f64::INFINITY;
        }
        let half = (radius_mm * radius_mm - off * off).sqrt();
        let t0 = ((along - half) / self.length).max(0.0);
        let t1 = ((along + half) / self.length).min(1.0);
        if t0 > t1 {
            return f64::INFINITY;
        }
        let z = |t: f64| a.z + self.dz * t;
        z(t0).min(z(t1))
    }

    /// The lowest a turned profile reaches over `q` (phase 3 design,
    /// "Algorithms → Tapered ball nose", step 8): the least over `t` of
    /// `z(t) + w⁻¹(|q − p(t)|)`, which is convex in `t` (`w⁻¹` is convex
    /// and increasing, the distance convex), infinite past the cap, where
    /// the bit is a cylinder that reaches no lower than the cap's height
    /// over a cell beside it. The move is split where the distance
    /// crosses `w_t` and the cap, and on each piece the least is at an
    /// end or the piece's stationary point clamped to it, in closed form:
    /// on the cone the V-bit's with the virtual apex (`w⁻¹(r) = r / t −
    /// δ`, whose offset moves no stationary point), in the ball the
    /// lowest point where the vertical through `q` meets the capsule the
    /// ball sweeps, from `L s = −dz √(ρ² − off² − s²)` squared (a
    /// quadratic in the along-move offset `s`).
    ///
    /// Changed from phase 3's: a pose inside a covered piece is read no
    /// further out than the cap. A move that falls as it leaves a cell is
    /// lowest over it at its last pose, on the rim, where rounding could
    /// otherwise put the distance a bit past the cap and the cut would
    /// be lost.
    pub(crate) fn lathe_low_mm(&self, q: Point, lathe: &Lathe) -> f64 {
        let a = self.a;
        if self.length < PLUNGE_MM {
            return a.z.min(a.z + self.dz) + lathe.height(q.distance_to(a.xy()));
        }
        // A pose of a piece the cutter covers: at most the cap away.
        let at = |t: f64| {
            let p = Point::new(a.x + self.dx * t, a.y + self.dy * t);
            a.z + self.dz * t + lathe.height(q.distance_to(p).min(lathe.cap))
        };
        let length = self.length;
        let (along, off) = self.foot(q);
        // The move's ends and where the distance crosses each zone's
        // edge: six knots at most, kept in order.
        let mut knots = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut count = 2;
        for r in [lathe.w_t, lathe.cap] {
            if off < r {
                let half = ((r - off) * (r + off)).sqrt();
                for s in [along - half, along + half] {
                    let t = s / length;
                    if 0.0 < t
                        && t < 1.0
                        && let Some(slot) = knots.get_mut(count)
                    {
                        *slot = t;
                        count += 1;
                    }
                }
            }
        }
        let knots = knots.get_mut(..count).unwrap_or(&mut []);
        knots.sort_unstable_by(f64::total_cmp);
        let mut low = f64::INFINITY;
        for pair in knots.windows(2) {
            let &[lo, hi] = pair else { continue };
            let mid = 0.5 * (lo + hi);
            let r_mid = q.distance_to(Point::new(a.x + self.dx * mid, a.y + self.dy * mid));
            let stationary = if r_mid <= lathe.w_t {
                // The ball: `f′ = dz + L s / √(ρ² − off² − s²)`, `s = L t
                // − along`.
                let s = -self.dz * ((lathe.rho - off) * (lathe.rho + off)).max(0.0).sqrt()
                    / length.hypot(self.dz);
                Some((along + s) / length)
            } else if r_mid <= lathe.cap {
                // The cone, as [`cone_low_mm`]: a move that falls faster
                // than the cone widens is least at an end.
                let k = -self.dz * lathe.slope / length;
                (k.abs() < 1.0).then(|| (along + k * off / (1.0 - k * k).sqrt()) / length)
            } else {
                continue;
            };
            for t in [lo, hi]
                .into_iter()
                .chain(stationary.map(|t| t.clamp(lo, hi)))
            {
                low = low.min(at(t));
            }
        }
        low
    }
}

impl Cutter {
    /// The lowest the cutter's surface reaches over `q` while its tip
    /// runs straight from `a` to `b`, mm; `None` when it is never over
    /// `q`.
    #[must_use]
    pub fn low_over_mm(&self, a: Point3, b: Point3, q: Point) -> Option<f64> {
        let low = CutterSweep::new(self).low_mm(&LineSweep::new(a, b), q);
        low.is_finite().then_some(low)
    }
}

/// A cutter as the kernel cuts with it: its zones worked out once.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CutterSweep {
    /// A flat end of this radius, the rim's slack added.
    Disc(f64),
    /// A turned profile.
    Lathe(Lathe),
}

impl CutterSweep {
    /// `cutter`, ready to cut.
    pub(crate) fn new(cutter: &Cutter) -> Self {
        Lathe::of(cutter).map_or_else(
            || Self::Disc(cutter.radius_mm() + RIM_SLACK_MM),
            Self::Lathe,
        )
    }

    /// The lowest the surface reaches over `q` along `line`; infinite
    /// when it is never over `q`.
    pub(crate) fn low_mm(&self, line: &LineSweep, q: Point) -> f64 {
        match self {
            Self::Disc(radius_mm) => line.disc_low_mm(q, *radius_mm),
            Self::Lathe(lathe) => line.lathe_low_mm(q, lathe),
        }
    }

    /// The surface's height above the tip `r` from the axis; infinite
    /// past the rim.
    pub(crate) fn height_mm(&self, r: f64) -> f64 {
        match self {
            Self::Disc(radius_mm) => {
                if r <= *radius_mm {
                    0.0
                } else {
                    f64::INFINITY
                }
            }
            Self::Lathe(lathe) => lathe.height(r),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use dc_geometry::assert_approx_eq;

    /// The bench's Ultra-High Detail bit: tip radius 0.381 mm, 6.7° per
    /// side, cutting diameter 6.35 mm.
    fn ultra() -> BitProfile {
        BitProfile::TaperedBall {
            tip_radius_mm: 0.381,
            slope: 6.7_f64.to_radians().tan(),
            cap_radius_mm: 3.175,
        }
    }

    /// `low(q)` by trying 10⁵ poses along the move.
    fn brute(a: Point3, b: Point3, q: Point, height: &dyn Fn(f64) -> f64) -> f64 {
        let n = 100_000;
        (0..=n)
            .map(|k| {
                let t = f64::from(k) / f64::from(n);
                let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                a.z + (b.z - a.z) * t + height(q.distance_to(p))
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Moves that fall and rise as they go, a plunge among them, and
    /// cells on the path, beside it, at each zone and out of reach.
    fn cases() -> (Vec<(Point3, Point3)>, Vec<Point>) {
        (
            vec![
                (Point3::new(0.0, 0.0, 10.0), Point3::new(4.0, 1.0, 9.6)),
                (Point3::new(1.0, -1.0, 9.0), Point3::new(-2.0, 0.5, 8.0)),
                (Point3::new(0.0, 0.0, 10.0), Point3::new(0.3, 0.0, 9.0)),
                (Point3::new(-1.0, 0.0, 9.95), Point3::new(1.0, 0.0, 9.9)),
                (Point3::new(-1.0, 0.5, 9.0), Point3::new(3.0, 0.5, 9.0)),
                (Point3::new(0.5, 0.5, 11.0), Point3::new(0.5, 0.5, 7.0)),
                (Point3::new(2.0, 2.0, 7.0), Point3::new(-1.0, -2.0, 10.5)),
            ],
            vec![
                Point::new(0.1, 0.05),
                Point::new(2.0, 0.6),
                Point::new(2.0, 0.2),
                Point::new(0.15, -0.1),
                Point::new(-1.0, 2.0),
                Point::new(3.0, -3.5),
                Point::new(0.0, 0.0),
                Point::new(0.5, 0.5),
                Point::new(1.9, 1.1),
            ],
        )
    }

    /// The closed form is never above the brute force, and within the
    /// brute force's own sampling error below it.
    fn assert_exact(name: &str, exact: f64, brute: f64, a: Point3, b: Point3, q: Point) {
        assert!(
            exact <= brute + 1e-12 || (exact.is_infinite() && brute.is_infinite()),
            "{name}, {a:?} → {b:?} over {q:?}: {exact} above the brute force's {brute}"
        );
        assert!(
            exact.is_infinite() || exact >= brute - 1e-4,
            "{name}, {a:?} → {b:?} over {q:?}: {exact} far below the brute force's {brute}"
        );
    }

    /// Phase 3 design, step 8 (moved from `dc_cam::heightfield`): a
    /// cell's height under a straight move that descends as it goes is
    /// the least of `z(τ) + w⁻¹(|q − p(τ)|)`, exactly: never above a
    /// 10⁵-sample brute force of it, and within its sampling error
    /// below, for cells in the ball's reach, on the cone and past the
    /// cap.
    #[test]
    fn tapered_descending_move_is_exact() {
        let profile = ultra();
        let height = |r: f64| profile.height_mm(r).unwrap_or(f64::INFINITY);
        let (moves, cells) = cases();
        for &(a, b) in &moves {
            for &q in &cells {
                let exact = profile_low_mm(a, b, q, &profile);
                assert_exact("tapered", exact, brute(a, b, q, &height), a, b, q);
            }
        }
    }

    /// Acceptance 136: for every shape the simulation cuts with, a move
    /// that falls or rises as it goes leaves the lowest height the
    /// surface reaches anywhere along it, not the height at the pose
    /// nearest the cell (upstream's rule, which leaves a ramp up to
    /// `slope × radius` too high; the survey, section 5.3).
    #[test]
    fn every_shape_is_cut_to_its_lowest_pose() {
        let shapes = [
            ("flat", Cutter::Flat { radius_mm: 1.5 }),
            ("ball", Cutter::Ball { radius_mm: 2.0 }),
            (
                "V 60°",
                Cutter::Vee {
                    slope: 30.0_f64.to_radians().tan(),
                    radius_mm: 3.0,
                },
            ),
            (
                "V 90°",
                Cutter::Vee {
                    slope: 1.0,
                    radius_mm: 1.2,
                },
            ),
            (
                "tapered",
                Cutter::TaperedBall {
                    tip_radius_mm: 0.381,
                    slope: 6.7_f64.to_radians().tan(),
                    radius_mm: 3.175,
                },
            ),
        ];
        let (moves, cells) = cases();
        for (name, cutter) in shapes {
            let height = |r: f64| cutter.height_mm(r).unwrap_or(f64::INFINITY);
            let mut reached = 0;
            for &(a, b) in &moves {
                for &q in &cells {
                    let exact = cutter.low_over_mm(a, b, q).unwrap_or(f64::INFINITY);
                    assert_exact(name, exact, brute(a, b, q, &height), a, b, q);
                    reached += usize::from(exact.is_finite());
                }
            }
            assert!(reached > 10, "{name} reached only {reached} cells");
        }
    }

    /// The ramp of upstream's own kernel test, with the exact answer: a
    /// flat cutter of radius 1.5 running from (0, 0, 0) to (10, 0, −2)
    /// has, over the cell at (5, 0), fallen as far as its trailing edge
    /// lets it, at x = 6.5: z = −1.3. Upstream's nearest pose gives −1.0.
    #[test]
    fn a_ramp_is_cut_to_the_lowest_pose_not_the_nearest() {
        let (a, b) = (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, -2.0));
        let flat = Cutter::Flat { radius_mm: 1.5 };
        let on_path = flat.low_over_mm(a, b, Point::new(5.0, 0.0)).unwrap();
        assert_approx_eq!(on_path, -1.3, 1e-9);
        // Beside the path the covered stretch is shorter: √(1.5² − 1.2²)
        // = 0.9 either side of the foot.
        let beside = flat.low_over_mm(a, b, Point::new(5.0, 1.2)).unwrap();
        assert_approx_eq!(beside, -(5.0 + 0.9) / 10.0 * 2.0, 1e-9);
        // A ball on the path axis is lowest a little past the foot.
        let ball = Cutter::Ball { radius_mm: 1.5 };
        let z = ball.low_over_mm(a, b, Point::new(5.0, 0.0)).unwrap();
        assert!(z < -1.0 && z > -1.3, "the ball's {z}");
    }

    /// A level move is the profile at the nearest point of the segment:
    /// PureCutCNC's kernel (`replay.ts`, `applyMoveKernel`), which is
    /// exact there.
    #[test]
    fn a_level_move_is_the_profile_at_the_nearest_point() {
        let (a, b) = (Point3::new(1.0, 1.0, 5.0), Point3::new(7.0, 3.0, 5.0));
        let nearest_mm = |q: Point| {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            q.distance_to(Point::new(a.x + dx * t, a.y + dy * t))
        };
        for cutter in [
            Cutter::Flat { radius_mm: 1.5 },
            Cutter::Ball { radius_mm: 2.0 },
            Cutter::Vee {
                slope: 1.0,
                radius_mm: 3.0,
            },
        ] {
            for q in [
                Point::new(0.0, 0.0),
                Point::new(4.0, 2.0),
                Point::new(4.3, 3.1),
                Point::new(8.0, 3.5),
                Point::new(4.0, 9.0),
            ] {
                let want = cutter.height_mm(nearest_mm(q)).map(|h| 5.0 + h);
                let got = cutter.low_over_mm(a, b, q);
                match (want, got) {
                    (Some(w), Some(g)) => assert_approx_eq!(g, w, 1e-9),
                    (None, None) => {}
                    _ => panic!("{cutter:?} over {q:?}: {got:?}, want {want:?}"),
                }
            }
        }
    }

    /// Acceptance 134: a V-bit deeper than its cone cuts nothing past its
    /// radius, so it leaves a wall at its diameter.
    #[test]
    fn a_v_deeper_than_its_cone_reaches_no_further_than_its_radius() {
        let v = Cutter::Vee {
            slope: 1.0,
            radius_mm: 2.0,
        };
        let (a, b) = (Point3::new(0.0, 0.0, 4.0), Point3::new(10.0, 0.0, 4.0));
        assert_approx_eq!(
            v.low_over_mm(a, b, Point::new(5.0, 1.9)).unwrap(),
            5.9,
            1e-9
        );
        assert_eq!(v.low_over_mm(a, b, Point::new(5.0, 2.1)), None);
        // The uncapped cone phase 3's tests use has no such edge.
        assert_approx_eq!(cone_low_mm(a, b, Point::new(5.0, 2.1), 1.0), 6.1, 1e-9);
    }
}
