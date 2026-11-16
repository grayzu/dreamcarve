// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The least room of one stretch of a straight cut against one wall piece
//! (`docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose", step
//! 7). On a stretch the disc's radius is one formula: linear in the cut's
//! parameter (a V-bit, the TBN's cone, its cylinder, the air above the
//! start plane), a [`Ramp`]; or the ball's, a [`Ball`].
//!
//! The wall's distance is the least of the distances to its two ends
//! and, where the tip is abreast of it, to its line, or where the tip is
//! within its sweep, to its circle. Each piece of that, less the radius,
//! is least at an end of its stretch, at a corner of the distance (where
//! the tip crosses the line or the circle, or passes through the point),
//! or where its derivative vanishes, which [`Stretch`] supplies. Every
//! candidate is read with the room's own formula, so the result is a real
//! value of the room, and any extra candidate can only lower it towards
//! the least, never past it.
//!
//! The splitting and the V-bit's closed forms are `dc-gcode`'s
//! `material::cone::Ramp` (#407, #448) copied bit for bit, with the whole
//! cut `[0, 1]` generalised to a stretch `[lo, hi]` of it; on `[0, 1]`
//! every operation is the same, so the result is identical.

use super::WallPiece;
use super::roots::{mul, roots_in, sub};
use crate::point::{Point, Vec2};
use crate::polyline::Arc;

/// A stretch's radius and the stationary points of its room.
pub(super) trait Stretch: Copy {
    /// The tip at `t = 0`.
    fn from(self) -> Point;
    /// The tip's motion per unit `t`.
    fn along(self) -> Vec2;
    /// The disc's radius at `t`, mm.
    fn radius_mm(self, t: f64) -> f64;
    /// Pushes onto `out` the `t` in [`lo`, `hi`] where `|tip − e| − r` may
    /// be least, besides `lo` and `hi`.
    fn point_turns(self, e: Point, lo: f64, hi: f64, out: &mut Vec<f64>);
    /// Pushes onto `out` the `t` in [`lo`, `hi`] where `|n0 + n1 t| − r`
    /// may be least, besides `lo` and `hi`.
    fn line_turns(self, n0: f64, n1: f64, lo: f64, hi: f64, out: &mut Vec<f64>);
    /// Pushes onto `out` the `t` in [`lo`, `hi`] where `ρ − |tip − c| − r`
    /// (inside a circle) may be least, besides `lo` and `hi`.
    fn inside_turns(self, c: Point, lo: f64, hi: f64, out: &mut Vec<f64>);

    /// The tip at `t`.
    fn tip(self, t: f64) -> Point {
        self.from() + self.along() * t
    }
}

/// A stretch whose radius is linear in `t`: `from_r_mm + t · per_t_mm`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ramp {
    pub from: Point,
    pub along: Vec2,
    pub from_r_mm: f64,
    pub per_t_mm: f64,
}

impl Stretch for Ramp {
    fn from(self) -> Point {
        self.from
    }

    fn along(self) -> Vec2 {
        self.along
    }

    fn radius_mm(self, t: f64) -> f64 {
        self.per_t_mm.mul_add(t, self.from_r_mm)
    }

    /// `|tip − e| − r` is convex: least where its slope is 0 when that
    /// lies in the stretch, else at an end.
    fn point_turns(self, e: Point, lo: f64, hi: f64, out: &mut Vec<f64>) {
        let l2 = self.along.dot(self.along);
        let k = self.per_t_mm;
        if l2 > 0.0 && k * k < l2 {
            // |tip − e| = √(l²(t − tc)² + h²); its slope equals k where
            // l(t − tc) = k·h / √(l² − k²).
            let l = l2.sqrt();
            let to_e = e - self.from;
            let tc = to_e.dot(self.along) / l2;
            let h = self.along.cross(to_e).abs() / l;
            let w = k * h / (l2 - k * k).sqrt();
            out.push((tc + w / l).clamp(lo, hi));
        }
    }

    /// Convex and piecewise linear: at an end or where the tip crosses the
    /// line.
    fn line_turns(self, n0: f64, n1: f64, lo: f64, hi: f64, out: &mut Vec<f64>) {
        if n1 != 0.0 {
            out.push((-n0 / n1).clamp(lo, hi));
        }
    }

    /// Concave inside a circle: at an end.
    fn inside_turns(self, _c: Point, _lo: f64, _hi: f64, _out: &mut Vec<f64>) {}
}

/// A stretch in the TBN's ball: the tip `d = d0 + t · dd` below the start
/// plane, the radius `√(2ρd − d²)`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ball {
    pub from: Point,
    pub along: Vec2,
    /// `ρ`, mm.
    pub tip_radius_mm: f64,
    /// `d` at `t = 0`, mm.
    pub d0_mm: f64,
    /// `d`'s change per unit `t`, mm.
    pub dd_mm: f64,
}

impl Stretch for Ball {
    fn from(self) -> Point {
        self.from
    }

    fn along(self) -> Vec2 {
        self.along
    }

    fn radius_mm(self, t: f64) -> f64 {
        let d = self.dd_mm.mul_add(t, self.d0_mm);
        (d * (2.0 * self.tip_radius_mm - d)).max(0.0).sqrt()
    }

    /// `g = √Q − r`, `Q = |tip − e|²`. Its derivative vanishes where
    /// `Q′ / (2√Q) = r′`, `r′ = dd (ρ − d) / r`; squared, with `u = ρ − d`
    /// and `r² = ρ² − u²`, that is the quartic `Q′² (ρ² − u²) = 4 Q dd² u²`.
    /// `√Q` has a corner where the tip passes through `e` (`Q = 0`): the
    /// tip's nearest approach to `e` is a candidate too.
    fn point_turns(self, e: Point, lo: f64, hi: f64, out: &mut Vec<f64>) {
        let l2 = self.along.dot(self.along);
        if l2 <= 0.0 {
            return;
        }
        let v0 = self.from - e;
        let b = v0.dot(self.along);
        out.push((-b / l2).clamp(lo, hi));
        // Q = l² t² + 2 b t + |v0|², Q′ / 2 = l² t + b.
        let q = [v0.dot(v0), 2.0 * b, l2];
        let half_q_slope = [b, l2];
        let (rho, d0, dd) = (self.tip_radius_mm, self.d0_mm, self.dd_mm);
        // ρ² − u², u = (ρ − d0) − dd t, its constant term written as
        // d0 (2ρ − d0) so it keeps its digits near the start plane.
        let u0 = rho - d0;
        let r2 = [d0 * (2.0 * rho - d0), 2.0 * u0 * dd, -dd * dd];
        let u2_dd2 = mul(&[u0 * dd, -dd * dd], &[u0 * dd, -dd * dd]);
        let quartic = sub(
            &mul(&mul(&half_q_slope, &half_q_slope), &r2),
            &mul(&q, &u2_dd2),
        );
        out.extend(roots_in(&quartic, lo, hi));
    }

    /// `g = |n| − r`, `n = n0 + n1 t`: a corner where the tip crosses the
    /// line, and where `n1² = r′²`, `n1² (ρ² − u²) = dd² u²`, so
    /// `u = ±ρ |n1| / √(n1² + dd²)`, the quadratic's two roots.
    fn line_turns(self, n0: f64, n1: f64, lo: f64, hi: f64, out: &mut Vec<f64>) {
        if n1 != 0.0 {
            out.push((-n0 / n1).clamp(lo, hi));
        }
        let (rho, d0, dd) = (self.tip_radius_mm, self.d0_mm, self.dd_mm);
        let h = n1.hypot(dd);
        if dd != 0.0 && h > 0.0 {
            let u = rho * n1.abs() / h;
            for u in [u, -u] {
                out.push(((rho - u - d0) / dd).clamp(lo, hi));
            }
        }
    }

    /// `g = ρ_c − √Q − r` has the same squared stationary condition as the
    /// point's, `e` the circle's centre.
    fn inside_turns(self, c: Point, lo: f64, hi: f64, out: &mut Vec<f64>) {
        self.point_turns(c, lo, hi, out);
    }
}

/// The least room of stretch `s`, `t` in [`lo`, `hi`], against `wall`, and
/// the `t` where it is least.
pub(super) fn least_on(s: impl Stretch, lo: f64, hi: f64, wall: WallPiece) -> (f64, f64) {
    let segment = wall.segment;
    let mut least = to_point(s, segment.start, lo, hi);
    let mut keep = |found: (f64, f64)| {
        if found.0 < least.0 || found.0.is_nan() && !least.0.is_nan() {
            least = found;
        }
    };
    keep(to_point(s, segment.end, lo, hi));
    match wall.arc {
        Some(arc) => to_circle(s, arc, wall, lo, hi)
            .into_iter()
            .for_each(&mut keep),
        None => to_line(s, wall, lo, hi).into_iter().for_each(&mut keep),
    }
    least
}

/// The least of `|tip(t) − e| − radius(t)` over [`lo`, `hi`] and where.
fn to_point(s: impl Stretch, e: Point, lo: f64, hi: f64) -> (f64, f64) {
    let room = |t: f64| s.tip(t).distance_to(e) - s.radius_mm(t);
    let mut candidates = vec![lo, hi];
    s.point_turns(e, lo, hi, &mut candidates);
    least_of(&candidates, room)
}

/// The least of `|perp(tip(t), line)| − radius(t)` where the tip is abreast
/// of the straight wall, within [`lo`, `hi`], and where; `None` where it
/// never is.
fn to_line(s: impl Stretch, wall: WallPiece, lo: f64, hi: f64) -> Option<(f64, f64)> {
    let wall = wall.segment;
    let u = wall.end - wall.start;
    let u2 = u.dot(u);
    if u2 <= 0.0 {
        return None;
    }
    // The tip's place along the wall, s = s0 + t·s1, and its signed
    // distance from the wall's line, n = n0 + t·n1.
    let (from, along) = (s.from(), s.along());
    let (s0, s1) = ((from - wall.start).dot(u) / u2, along.dot(u) / u2);
    let (lo, hi) = abreast(s0, s1, lo, hi)?;
    let l = u2.sqrt();
    let (n0, n1) = (u.cross(from - wall.start) / l, u.cross(along) / l);
    let room = |t: f64| n1.mul_add(t, n0).abs() - s.radius_mm(t);
    let mut candidates = vec![lo, hi];
    s.line_turns(n0, n1, lo, hi, &mut candidates);
    Some(least_of(&candidates, room))
}

/// The least of `| |tip(t) − c| − ρ | − radius(t)` where the tip lies
/// within the arc's sweep about its centre `c` (radius `ρ`), within
/// [`lo`, `hi`], and where; `None` where it never does. The stretch is
/// split where it crosses the rays through the arc's ends, passes nearest
/// `c`, or crosses the circle; on each piece the tip is outside the circle
/// or inside it.
fn to_circle(s: impl Stretch, arc: Arc, wall: WallPiece, lo: f64, hi: f64) -> Option<(f64, f64)> {
    let wall = wall.segment;
    let c = arc.centre;
    let rho = arc.radius_mm;
    let along = s.along();
    let from_c = s.from() - c;
    let l2 = along.dot(along);
    if l2 <= 0.0 {
        return None;
    }
    let mut cuts = vec![lo, hi, (-from_c.dot(along) / l2).clamp(lo, hi)];
    for end in [wall.start, wall.end] {
        let ray = end - c;
        let across = ray.cross(along);
        if across != 0.0 {
            cuts.push(-ray.cross(from_c) / across);
        }
    }
    // |from_c + t·along|² = ρ²
    let b = from_c.dot(along);
    let disc = b * b - l2 * (from_c.dot(from_c) - rho * rho);
    if disc >= 0.0 {
        let root = disc.sqrt();
        cuts.push((-b - root) / l2);
        cuts.push((-b + root) / l2);
    }
    cuts.retain(|t| (lo..=hi).contains(t));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut least: Option<(f64, f64)> = None;
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        let middle = s.tip((a + b) / 2.0);
        if !within_sweep(arc, middle) {
            continue;
        }
        let found = if middle.distance_to(c) >= rho {
            let (room_mm, t) = to_point(s, c, a, b);
            (room_mm - rho, t)
        } else {
            let room = |t: f64| rho - s.tip(t).distance_to(c) - s.radius_mm(t);
            let mut candidates = vec![a, b];
            s.inside_turns(c, a, b, &mut candidates);
            least_of(&candidates, room)
        };
        if least.is_none_or(|(r, _)| found.0 < r || found.0.is_nan() && !r.is_nan()) {
            least = Some(found);
        }
    }
    least
}

/// The `t` in [`lo`, `hi`] where `s0 + t · s1` lies in [0, 1]; `None` when
/// it never does.
fn abreast(s0: f64, s1: f64, lo: f64, hi: f64) -> Option<(f64, f64)> {
    if s1 == 0.0 {
        return (0.0..=1.0).contains(&s0).then_some((lo, hi));
    }
    let (a, b) = ((0.0 - s0) / s1, (1.0 - s0) / s1);
    let (lo, hi) = (a.min(b).max(lo), a.max(b).min(hi));
    (lo <= hi).then_some((lo, hi))
}

/// The least of `f` at `candidates`, and where; NaN when `f` is NaN at
/// any of them, so a value that cannot be read is never read as room.
fn least_of(candidates: &[f64], f: impl Fn(f64) -> f64) -> (f64, f64) {
    let mut least = (f64::INFINITY, 0.0);
    for &t in candidates {
        let g = f(t);
        if g.is_nan() {
            return (g, t);
        }
        if g < least.0 {
            least = (g, t);
        }
    }
    least
}

/// Whether the direction from `arc`'s centre to `q` lies within its sweep.
fn within_sweep(arc: Arc, q: Point) -> bool {
    let tau = std::f64::consts::TAU;
    let to_q = q - arc.centre;
    let angle = to_q.y.atan2(to_q.x);
    let from_start = if arc.sweep_rad > 0.0 {
        (angle - arc.start_angle_rad).rem_euclid(tau)
    } else {
        (arc.start_angle_rad - angle).rem_euclid(tau)
    };
    from_start <= arc.sweep_rad.abs()
}
