// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A carving bit's cross-section and the exact least room of a cut
//! (`docs/specs/phase-3/design.md`, "Data model → Tapered ball nose" and
//! "Algorithms → Tapered ball nose", step 7; security review M1).
//!
//! A bit is a stack of discs about its axis that widen with height. A
//! [`BitProfile`] gives the disc's radius `w(d)` at height `d` above the
//! tip and its inverse `w⁻¹`. Two bits carve: the V-bit, `w(d) = d · s`,
//! and the tapered ball nose (TBN), a ball of radius `ρ` tangent to a cone
//! of half-angle `β` (`t = tan β`), capped at the cutting radius `R`:
//!
//! - `w(d) = √(2ρd − d²)` for `0 ≤ d ≤ h_t`, the ball, with
//!   `h_t = ρ (1 − sin β)`;
//! - `w(d) = (d + δ) · t` above it, the cone of the **virtual V-bit**
//!   whose point is `δ = ρ (1/sin β − 1)` below the TBN's tip;
//! - `w(d) = R` from `h_R = w⁻¹(R)` up, where the bit is a cylinder.
//!
//! Below `h_R` the TBN's `w` is C¹ at `h_t`, increasing and concave, with
//! `w′ ≥ t`.
//!
//! [`least_room`] is S17's test (the generator's and the safety pass's):
//! a cut is safe when its disc at the start plane stays inside the walls,
//! so its **room** at a point is `dist(tip, wall) − w(d)`, `d` the tip's
//! depth below the start plane, and the cut is held to the least room
//! along it. The least is found exactly, with no sampling step (M1): the
//! cut is split where `d` crosses the zones' bounds, each zone's stretch is
//! split per piece of the wall's distance (its line or circle where the tip
//! is abreast of it, and each end), and on each piece the least of the
//! room is at an end or a stationary point, which is found in closed form
//! (V-bit, cone, cylinder) or as a root of a polynomial of degree at most
//! 4 (the ball). [`least_room`]'s docs say how, and why the result can
//! only err towards too little room.

mod roots;
mod stretch;
#[cfg(test)]
mod tests;

use crate::distance::{closest_points, point_segment_distance_mm, segment_distance_mm};
use crate::point::{EPS_MM, Point, Point3};
use crate::polyline::{Arc, Segment};
use stretch::{Ball, Ramp, least_on};

/// A carving bit's cross-section: the radius `w(d)` of the disc it cuts at
/// height `d` above its tip, mm. Y-up, Z-up, millimetres (ADR-0001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BitProfile {
    /// A V-bit: `w(d) = d · slope`, `slope = tan(α/2)` for an included
    /// angle `α`. It has no cap: the cone's height is the caller's limit.
    VBit {
        /// `tan(α/2)`, the radius per mm of height.
        slope: f64,
    },
    /// A tapered ball nose: a ball of radius `tip_radius_mm` tangent to a
    /// cone of half-angle `β` (`slope = tan β`), the width capped at
    /// `cap_radius_mm`, half the cutting diameter. Meaningful for a
    /// positive, finite tip radius, slope and cap; [`least_room`] reads
    /// any other as no room.
    TaperedBall {
        /// The ball's radius `ρ`, mm (half the tip diameter).
        tip_radius_mm: f64,
        /// `tan β`, `β` the taper per side.
        slope: f64,
        /// The cap `R`, mm: half the cutting diameter. Above `h_R` the bit
        /// is a cylinder of this radius.
        cap_radius_mm: f64,
    },
}

impl BitProfile {
    /// `w(d)`: the radius of the disc the bit cuts `height_mm` above its
    /// tip, mm. 0 at and below the tip; capped at a TBN's cutting radius.
    /// For a V-bit, bit for bit `height_mm · slope` above the tip.
    #[must_use]
    pub fn half_width_mm(&self, height_mm: f64) -> f64 {
        if height_mm <= 0.0 {
            return 0.0;
        }
        match *self {
            Self::VBit { slope } => height_mm * slope,
            Self::TaperedBall { cap_radius_mm, .. } => {
                let uncapped_mm = self.uncapped_half_width_mm(height_mm);
                if uncapped_mm < cap_radius_mm {
                    uncapped_mm
                } else {
                    cap_radius_mm
                }
            }
        }
    }

    /// `w⁻¹(r)`: the height above the tip where the bit's disc is
    /// `half_width_mm` across, mm; 0 for a radius of 0 or less. `None` at
    /// or past a TBN's cap, where no height gives that radius. For a V-bit,
    /// bit for bit `half_width_mm / slope`.
    #[must_use]
    pub fn height_mm(&self, half_width_mm: f64) -> Option<f64> {
        if let Self::TaperedBall { cap_radius_mm, .. } = *self
            && (half_width_mm >= cap_radius_mm || half_width_mm.is_nan())
        {
            return None;
        }
        if half_width_mm <= 0.0 {
            return Some(0.0);
        }
        Some(self.uncapped_height_mm(half_width_mm))
    }

    /// `h_R`, the height where a TBN's width reaches its cap, mm; `None`
    /// for a V-bit, which has no cap.
    #[must_use]
    pub fn cap_height_mm(&self) -> Option<f64> {
        match *self {
            Self::VBit { .. } => None,
            Self::TaperedBall { cap_radius_mm, .. } => Some(self.uncapped_height_mm(cap_radius_mm)),
        }
    }

    /// The flank's slope: `tan(α/2)` for a V-bit, `tan β` for a TBN. It
    /// bounds how fast the carved surface rises: `(w⁻¹)′ ≤ 1 / slope`.
    #[must_use]
    pub fn slope(&self) -> f64 {
        match *self {
            Self::VBit { slope } | Self::TaperedBall { slope, .. } => slope,
        }
    }

    /// `h_t = ρ (1 − sin β)`, the height where a TBN's ball meets its
    /// cone, mm; 0 for a V-bit.
    #[must_use]
    pub fn tangent_height_mm(&self) -> f64 {
        match *self {
            Self::VBit { .. } => 0.0,
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                ..
            } => tip_radius_mm * (1.0 - sin_cos(slope).0),
        }
    }

    /// `w_t = ρ cos β`, a TBN's radius where its ball meets its cone, mm;
    /// 0 for a V-bit.
    #[must_use]
    pub fn tangent_half_width_mm(&self) -> f64 {
        match *self {
            Self::VBit { .. } => 0.0,
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                ..
            } => tip_radius_mm * sin_cos(slope).1,
        }
    }

    /// `δ = ρ (1/sin β − 1)`, how far below a TBN's tip the point of its
    /// virtual V-bit is, mm: above `h_t` the TBN cuts `(d + δ) · tan β`.
    /// 0 for a V-bit.
    #[must_use]
    pub fn apex_offset_mm(&self) -> f64 {
        match *self {
            Self::VBit { .. } => 0.0,
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                ..
            } => tip_radius_mm * (slope.hypot(1.0) / slope - 1.0),
        }
    }

    /// `w(d)` for `d > 0` without the cap: the ball to `h_t`, the cone
    /// above it.
    fn uncapped_half_width_mm(&self, height_mm: f64) -> f64 {
        match *self {
            Self::VBit { slope } => height_mm * slope,
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                ..
            } => {
                if height_mm <= self.tangent_height_mm() {
                    (height_mm * (2.0 * tip_radius_mm - height_mm)).sqrt()
                } else {
                    (height_mm + self.apex_offset_mm()) * slope
                }
            }
        }
    }

    /// `w⁻¹(r)` for `r > 0` without the cap: `ρ − √(ρ² − r²)` in the ball,
    /// written `r² / (ρ + √((ρ − r)(ρ + r)))` to keep its digits near the
    /// tip; `r / t − δ` on the cone.
    fn uncapped_height_mm(&self, half_width_mm: f64) -> f64 {
        match *self {
            Self::VBit { slope } => half_width_mm / slope,
            Self::TaperedBall {
                tip_radius_mm: rho,
                slope,
                ..
            } => {
                let r = half_width_mm;
                if r <= self.tangent_half_width_mm() {
                    r * r / (rho + ((rho - r) * (rho + r)).max(0.0).sqrt())
                } else {
                    r / slope - self.apex_offset_mm()
                }
            }
        }
    }

    /// Whether every number is finite and a TBN's are positive.
    fn is_valid(&self) -> bool {
        match *self {
            Self::VBit { slope } => slope.is_finite(),
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                cap_radius_mm,
            } => [tip_radius_mm, slope, cap_radius_mm]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0),
        }
    }

    /// The radius [`least_room`] reads at depth `d` below the start plane:
    /// a V-bit's `d · slope` on either side of the plane, as both
    /// existing copies of S17 read it, so no V-bit result can move; a
    /// TBN's `w(d)`.
    fn room_width_mm(&self, d: f64) -> f64 {
        match *self {
            Self::VBit { slope } => d * slope,
            Self::TaperedBall { .. } => self.half_width_mm(d),
        }
    }
}

/// `(sin β, cos β)` for `slope = tan β`.
fn sin_cos(slope: f64) -> (f64, f64) {
    let h = slope.hypot(1.0);
    (slope / h, 1.0 / h)
}

/// One piece of a wall: a straight segment or a circular arc, as a
/// [`Segment`] carries it (a bulge of 0 is a line), mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallPiece {
    segment: Segment,
    arc: Option<Arc>,
}

impl WallPiece {
    /// The straight wall from `start` to `end`, mm.
    #[must_use]
    pub fn line(start: Point, end: Point) -> Self {
        Self::from(Segment {
            start,
            end,
            bulge: 0.0,
        })
    }

    /// The wall as a segment.
    #[must_use]
    pub const fn segment(&self) -> Segment {
        self.segment
    }
}

impl From<Segment> for WallPiece {
    /// The wall `segment`: an arc when it has one ([`Segment::arc`]), else
    /// the straight line between its ends.
    fn from(segment: Segment) -> Self {
        Self {
            segment,
            arc: segment.arc(),
        }
    }
}

/// S17's least room (`docs/specs/phase-3/design.md`, "Algorithms →
/// Tapered ball nose", step 7; security review M1): along the straight
/// cut from `from` to `to`, the least of the tip's distance to `wall` less
/// the radius of the disc `profile` cuts at the start plane
/// `start_plane_z_mm`, mm, and where it is least as the fraction `τ` in
/// [0, 1] of the way from `from`. Negative where the disc crosses the
/// wall. Exact, with no sampling step:
///
/// - **No XY motion:** the distance from the tip less the deeper end's
///   radius, at the deeper end.
/// - **A level cut** (Z within [`EPS_MM`]): the segment-to-segment
///   distance less the deeper end's radius, as both existing copies read
///   it, which is exact at constant Z and can only read less room than a
///   cut within [`EPS_MM`] of level has; or, when lower, the closed forms
///   below at that constant radius, which also hold against an arc that
///   sweeps more than a half turn.
/// - **A V-bit:** the radius is linear in `τ`, and each piece of the wall's
///   distance less it is convex, or concave inside an arc: the closed
///   forms of `dc-gcode`'s cone test, bit for bit. Its radius is read
///   linearly above the start plane too, as there.
/// - **A TBN:** the cut is split where its depth crosses the start plane,
///   `h_t` and `h_R`. Above the plane the radius is 0, on the cone it is
///   the virtual V-bit's and on the cylinder `R`, all linear, so the V-bit's
///   closed forms run on those stretches. In the ball the room against
///   each piece is `C¹` but for the corners of the distance, which are
///   candidates, so its least is at an end, a corner or a root of the
///   squared stationary condition: a quadratic against a line, solved in
///   closed form, or a quartic against a point or a circle, whose roots
///   are isolated by its derivatives and bisected to the last bit. Squaring
///   adds roots that are not stationary points; the room read there is
///   still a real value of it, so it can only lower the result towards the
///   least, never below it.
///
/// Every candidate is read with the room's own formula, so the result is
/// the room at the `τ` returned, and it can exceed the true least only by
/// how far a root's rounding moves the room from a stationary point:
/// quadratically small there (below 1e-15 mm at workpiece scale). A
/// non-finite input or an invalid TBN (a tip radius, slope or cap that is
/// not positive) reads as `f64::NEG_INFINITY`, no room.
#[must_use]
pub fn least_room(
    from: Point3,
    to: Point3,
    wall: WallPiece,
    profile: &BitProfile,
    start_plane_z_mm: f64,
) -> (f64, f64) {
    let segment = wall.segment;
    let finite = [
        from.x,
        from.y,
        from.z,
        to.x,
        to.y,
        to.z,
        start_plane_z_mm,
        segment.start.x,
        segment.start.y,
        segment.end.x,
        segment.end.y,
        segment.bulge,
    ]
    .iter()
    .all(|v| v.is_finite());
    if !finite || !profile.is_valid() {
        return (f64::NEG_INFINITY, 0.0);
    }
    let (room_mm, t) = least_room_of_finite(from, to, wall, profile, start_plane_z_mm);
    if room_mm.is_nan() {
        (f64::NEG_INFINITY, t)
    } else {
        (room_mm, t)
    }
}

/// [`least_room`] of finite input and a valid profile.
fn least_room_of_finite(
    from: Point3,
    to: Point3,
    wall: WallPiece,
    profile: &BitProfile,
    start_plane_z_mm: f64,
) -> (f64, f64) {
    let (d0, d1) = (start_plane_z_mm - from.z, start_plane_z_mm - to.z);
    let deeper_mm = d0.max(d1);
    let path = Segment {
        start: from.xy(),
        end: to.xy(),
        bulge: 0.0,
    };
    let along = path.end - path.start;
    let l2 = along.dot(along);
    if l2 <= 0.0 {
        let t = if d1 > d0 { 1.0 } else { 0.0 };
        let room_mm =
            point_segment_distance_mm(wall.segment, path.start) - profile.room_width_mm(deeper_mm);
        return (room_mm, t);
    }
    let ramp = |from_r_mm: f64, to_r_mm: f64| Ramp {
        from: path.start,
        along,
        from_r_mm,
        per_t_mm: to_r_mm - from_r_mm,
    };
    if (to.z - from.z).abs() <= EPS_MM {
        // Today's segment distance, and the closed forms at a constant
        // radius, which also hold for an arc past a half turn, where the
        // segment distance can read too far (#686): the lesser.
        let width_mm = profile.room_width_mm(deeper_mm);
        let room_mm = segment_distance_mm(path, wall.segment) - width_mm;
        let exact = least_on(ramp(width_mm, width_mm), 0.0, 1.0, wall);
        if exact.0 < room_mm || exact.0.is_nan() {
            return exact;
        }
        let (p, _) = closest_points(path, wall.segment);
        let t = (p.distance_to(path.start) / l2.sqrt()).clamp(0.0, 1.0);
        return (room_mm, t);
    }
    let BitProfile::TaperedBall {
        tip_radius_mm,
        slope,
        cap_radius_mm,
    } = *profile
    else {
        let slope = profile.slope();
        return least_on(ramp(d0 * slope, d1 * slope), 0.0, 1.0, wall);
    };
    // The zones' bounds in depth: the start plane, the ball's top (the
    // cap's height when the cap is in the ball) and the cap.
    let cap_mm = profile.cap_height_mm().unwrap_or(f64::INFINITY);
    let ball_top_mm = profile.tangent_height_mm().min(cap_mm);
    let delta = profile.apex_offset_mm();
    let dd = d1 - d0;
    let mut knots = vec![0.0, 1.0];
    knots.extend(
        [0.0, ball_top_mm, cap_mm]
            .iter()
            .map(|h| (h - d0) / dd)
            .filter(|t| 0.0 < *t && *t < 1.0),
    );
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut least = (f64::INFINITY, 0.0);
    for pair in knots.windows(2) {
        let &[a, b] = pair else { continue };
        let middle_mm = dd.mul_add((a + b) / 2.0, d0);
        let found = if middle_mm <= 0.0 {
            least_on(ramp(0.0, 0.0), a, b, wall)
        } else if middle_mm < ball_top_mm {
            let ball = Ball {
                from: path.start,
                along,
                tip_radius_mm,
                d0_mm: d0,
                dd_mm: dd,
            };
            least_on(ball, a, b, wall)
        } else if middle_mm < cap_mm {
            least_on(ramp((d0 + delta) * slope, (d1 + delta) * slope), a, b, wall)
        } else {
            least_on(ramp(cap_radius_mm, cap_radius_mm), a, b, wall)
        };
        if found.0.is_nan() {
            return found;
        }
        if found.0 < least.0 {
            least = found;
        }
    }
    least
}
