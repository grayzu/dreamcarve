// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17, the cone, on the parsed program (`docs/specs/phase-3/design.md`,
//! "Safety pass"; functional S17; security review H3, H4 and M1).
//!
//! A V-bit whose tip is `d` below the start plane cuts a disc of radius
//! `d · slope` there (`slope = tan(α/2)`), and it cuts nothing below the
//! carve's final V surface exactly when that disc stays inside the carved
//! regions: the final surface at a point `q` is `dist(q, wall) / slope`
//! deep, and the cone at `p` reaches `q` at `d − |q − p| / slope`, which the
//! triangle inequality keeps above it wherever `d · slope ≤ dist(p, wall)`.
//! So each cut is held to its **room**, the tip's distance to the walls
//! less the cone's radius, at every point along it, measured exactly:
//!
//! - **A cut at constant Z, or an arc:** segment against segment, arcs as
//!   arcs ([`segment_distance_mm`]), less the radius at its deeper end.
//!   That is exact at constant Z; an arc whose Z varies (no V toolpath has
//!   one) is held to its deeper end all along, which can only refuse more.
//! - **A straight cut whose Z varies:** for each wall segment, the least of
//!   `g(t) = dist(p(t), wall) − r(t)` over the cut, `r` linear in `t`, in
//!   closed form. The distance to a straight wall is the least of the
//!   distances to its two ends and, where the tip is abreast of it, to its
//!   line; the distance to an arc wall is the least of the distances to its
//!   two ends and, where the tip is within its sweep, to its circle. Each
//!   of those, less `r`, is either convex along the cut (to a point, to a
//!   line, to a circle from outside it), whose least value is where its
//!   derivative vanishes or at an end of its stretch, or concave (to a
//!   circle from inside it), whose least value is at an end of its
//!   stretch. No sampling step.
//!
//! The walls are the carved regions' outlines as drawn, arcs and all. The
//! slack is S9's (`material`'s module docs): one unit in the last written
//! place, the flatten tolerance (the generator measures its chains against
//! chords, which near a concave arc lie up to that far beyond the true
//! wall, #407; the floor trace runs at exactly `F · slope` from the walls,
//! #405, and the floor's chords where grown islands touch are within it),
//! the arc-fit tolerance when arcs are fitted (S12), and [`EPS_MM`].

use dc_geometry::{
    Arc, EPS_MM, Point, Point3, Segment, Vec2, closest_points, point_segment_distance_mm,
    segment_distance_mm,
};

use super::{Area, bounds_of, near, on_line, points_of};

/// A V-Carve's material for S17, every value re-derived from the project.
#[derive(Debug, Clone)]
pub(crate) struct Cone {
    /// The carved regions, outer and holes, from the targets' closed
    /// profiles.
    pub(super) areas: Vec<Area>,
    /// The start plane, mm, project space.
    pub(super) start_plane_z_mm: f64,
    /// `tan(α/2)`: the cone's radius per mm of depth.
    pub(super) slope: f64,
    /// The flat depth's floor, mm, project space; `None` without one.
    pub(super) floor_z_mm: Option<f64>,
    /// The Z the flank is engaged from, mm: the stock top, or the start
    /// plane when an enabled earlier Pocket at its depth contains the
    /// targets (security review H4).
    pub(super) engaged_from_z_mm: f64,
    /// The cone's height, mm: `diameter / (2 · slope)`.
    pub(super) cone_mm: f64,
}

/// Why a cut fails S17.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Fault {
    /// The cone, the tip or the cut reaches `overlap_mm` past the material
    /// at `at`, project space.
    Off { at: Point3, overlap_mm: f64 },
    /// The flank is engaged `engaged_mm` deep, past the cone.
    Shank { engaged_mm: f64 },
}

impl Cone {
    /// The cone's radius at the start plane for a tip at `z_mm`, mm;
    /// negative above the start plane.
    fn radius_mm(&self, z_mm: f64) -> f64 {
        (self.start_plane_z_mm - z_mm) * self.slope
    }

    /// S17 for one cutting move: `path` its tip path in XY, `from_z_mm` and
    /// `to_z_mm` its ends' Z, project space, within `slack_mm`.
    ///
    /// - Its tip stays in the regions: its ends and middle are inside one
    ///   or within the slack of an outline, and it meets no outline away
    ///   from its ends.
    /// - **A straight descent** (no XY motion beyond the slack) that stays
    ///   above the start plane runs through the air over the carve: the
    ///   start plane is the stock top, or a Pocket or the user cleared it.
    ///   Any other descent is held at its lowest point, the one point
    ///   where its cone is widest, which is stricter than H3's exemption
    ///   (module docs of `material`).
    /// - **Any other cut** runs no higher than the start plane.
    /// - No cut reaches below the flat depth, nor engages the flank deeper
    ///   than the cone (the shank, H4).
    /// - Its room is at least `−slack_mm` all along (module docs).
    pub(super) fn check(
        &self,
        path: Segment,
        (from_z_mm, to_z_mm): (f64, f64),
        slack_mm: f64,
    ) -> Result<(), Fault> {
        // The tip's Z at `p` on the path: along a line, as far as `p` is
        // along it; on an arc (no V toolpath has one with varying Z), the
        // deeper end's, whose cone is the wider.
        let z_at = |p: Point| {
            let chord_mm = path.start.distance_to(path.end);
            if !path.is_line() || chord_mm <= EPS_MM {
                return from_z_mm.min(to_z_mm);
            }
            let t = (path.start.distance_to(p) / chord_mm).clamp(0.0, 1.0);
            from_z_mm + (to_z_mm - from_z_mm) * t
        };
        let (low, high) = if from_z_mm <= to_z_mm {
            (
                Point3::new(path.start.x, path.start.y, from_z_mm),
                Point3::new(path.end.x, path.end.y, to_z_mm),
            )
        } else {
            (
                Point3::new(path.end.x, path.end.y, to_z_mm),
                Point3::new(path.start.x, path.start.y, from_z_mm),
            )
        };
        if let Some((p, outside_mm)) = self.tip_off(path, slack_mm) {
            let z_mm = z_at(p);
            return Err(Fault::Off {
                at: Point3::new(p.x, p.y, z_mm),
                overlap_mm: outside_mm + self.radius_mm(z_mm).max(0.0),
            });
        }
        let vertical = path.is_line() && path.start.distance_to(path.end) <= slack_mm;
        let start_plane_z_mm = self.start_plane_z_mm;
        if vertical && low.z > start_plane_z_mm + slack_mm {
            return Ok(());
        }
        if !vertical && high.z > start_plane_z_mm + slack_mm {
            return Err(Fault::Off {
                at: high,
                overlap_mm: high.z - start_plane_z_mm,
            });
        }
        if let Some(floor_z_mm) = self.floor_z_mm
            && low.z < floor_z_mm - slack_mm
        {
            return Err(Fault::Off {
                at: low,
                overlap_mm: floor_z_mm - low.z,
            });
        }
        let engaged_mm = self.engaged_from_z_mm - low.z;
        if engaged_mm > self.cone_mm + slack_mm {
            return Err(Fault::Shank { engaged_mm });
        }
        match self.least_room(
            path,
            (from_z_mm, to_z_mm),
            vertical.then_some(low),
            slack_mm,
        ) {
            Some((room_mm, at)) if room_mm < -slack_mm => Err(Fault::Off {
                at,
                overlap_mm: -room_mm,
            }),
            _ => Ok(()),
        }
    }

    /// Where tip path `path` leaves the regions, and how far outside it is
    /// there, mm (0 where it crosses an outline): an end or its middle
    /// outside every region beyond `slack_mm`, or a point away from its
    /// ends where it meets an outline. `None` when it stays in.
    fn tip_off(&self, path: Segment, slack_mm: f64) -> Option<(Point, f64)> {
        let bounds = bounds_of(path);
        let near_areas = || {
            self.areas
                .iter()
                .filter(move |a| near(bounds, a.bounds, slack_mm))
        };
        for p in points_of(path) {
            let held = self.areas.iter().any(|a| a.region.contains_point(p))
                || near_areas().any(|a| on_line(&a.edges, p, slack_mm));
            if !held {
                let outside_mm = self
                    .areas
                    .iter()
                    .flat_map(|a| &a.edges)
                    .map(|e| point_segment_distance_mm(e.segment, p))
                    .fold(f64::INFINITY, f64::min);
                return Some((
                    p,
                    if outside_mm.is_finite() {
                        outside_mm
                    } else {
                        0.0
                    },
                ));
            }
        }
        near_areas()
            .flat_map(|a| &a.edges)
            .filter(|e| near(bounds, e.bounds, slack_mm))
            .find_map(|e| {
                let (p, q) = closest_points(path, e.segment);
                (p.distance_to(q) <= EPS_MM
                    && p.distance_to(path.start) > slack_mm
                    && p.distance_to(path.end) > slack_mm)
                    .then_some((p, 0.0))
            })
    }

    /// The least room along the cut (module docs), mm, and where it is
    /// least, among the walls within reach of the cut; `None` when no wall
    /// is, or no wall can bring the room below `−slack_mm`. A straight
    /// descent (`descent` its lowest point) is read at that point.
    fn least_room(
        &self,
        path: Segment,
        (from_z_mm, to_z_mm): (f64, f64),
        descent: Option<Point3>,
        slack_mm: f64,
    ) -> Option<(f64, Point3)> {
        let low_z_mm = from_z_mm.min(to_z_mm);
        let widest_mm = self.radius_mm(low_z_mm);
        let reach_mm = widest_mm.max(0.0) + slack_mm;
        let bounds = bounds_of(path);
        let level = (to_z_mm - from_z_mm).abs() <= EPS_MM;
        let mut least: Option<(f64, Point3)> = None;
        let mut keep = |room_mm: f64, at: Point3| {
            if least.is_none_or(|(r, _)| room_mm < r) {
                least = Some((room_mm, at));
            }
        };
        let walls = self
            .areas
            .iter()
            .filter(|a| near(bounds, a.bounds, reach_mm))
            .flat_map(|a| &a.edges)
            .filter(|e| near(bounds, e.bounds, reach_mm));
        for wall in walls {
            if let Some(low) = descent {
                let p = Point::new(low.x, low.y);
                keep(point_segment_distance_mm(wall.segment, p) - widest_mm, low);
                continue;
            }
            // At constant Z, or on an arc, the room at the deeper end all
            // along: exact at constant Z, and a bound below the room for an
            // arc whose Z varies.
            let bound_mm = segment_distance_mm(path, wall.segment) - widest_mm;
            if level || !path.is_line() {
                let (p, _) = closest_points(path, wall.segment);
                keep(bound_mm, Point3::new(p.x, p.y, low_z_mm));
                continue;
            }
            if bound_mm >= -slack_mm {
                // This wall cannot fail the cut.
                continue;
            }
            let ramp = Ramp {
                from: path.start,
                along: path.end - path.start,
                from_r_mm: self.radius_mm(from_z_mm),
                per_t_mm: self.radius_mm(to_z_mm) - self.radius_mm(from_z_mm),
            };
            let (room_mm, t) = ramp.least_room(wall.segment);
            let p = path.start + ramp.along * t;
            keep(
                room_mm,
                Point3::new(p.x, p.y, from_z_mm + (to_z_mm - from_z_mm) * t),
            );
        }
        least
    }
}

/// A straight cut whose cone grows or shrinks along it: the tip at
/// `from + t · along`, the cone's radius `from_r_mm + t · per_t_mm`, for
/// `t` in [0, 1].
#[derive(Debug, Clone, Copy)]
struct Ramp {
    from: Point,
    along: Vec2,
    from_r_mm: f64,
    per_t_mm: f64,
}

impl Ramp {
    fn tip(self, t: f64) -> Point {
        self.from + self.along * t
    }

    fn radius_mm(self, t: f64) -> f64 {
        self.per_t_mm.mul_add(t, self.from_r_mm)
    }

    /// The least of `dist(tip(t), wall) − radius(t)` over [0, 1] and the
    /// `t` where it is least, exactly (module docs).
    fn least_room(self, wall: Segment) -> (f64, f64) {
        let mut least = self.to_point(wall.start, 0.0, 1.0);
        let mut keep = |(room_mm, t): (f64, f64)| {
            if room_mm < least.0 {
                least = (room_mm, t);
            }
        };
        keep(self.to_point(wall.end, 0.0, 1.0));
        match wall.arc() {
            Some(arc) => self.to_circle(arc, wall).into_iter().for_each(&mut keep),
            None => self.to_line(wall).into_iter().for_each(&mut keep),
        }
        least
    }

    /// The least of `|tip(t) − e| − radius(t)` over [`lo`, `hi`] and where:
    /// convex, so at the stationary point when it lies there, else at an
    /// end.
    fn to_point(self, e: Point, lo: f64, hi: f64) -> (f64, f64) {
        let room = |t: f64| self.tip(t).distance_to(e) - self.radius_mm(t);
        let mut candidates = [lo, hi, lo];
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
            candidates[2] = (tc + w / l).clamp(lo, hi);
        }
        least_of(&candidates, room)
    }

    /// The least of `|perp(tip(t), line)| − radius(t)` where the tip is
    /// abreast of straight wall `wall`, and where; `None` where it never
    /// is. Convex and piecewise linear: at an end of that stretch or where
    /// the tip crosses the line.
    fn to_line(self, wall: Segment) -> Option<(f64, f64)> {
        let u = wall.end - wall.start;
        let u2 = u.dot(u);
        if u2 <= 0.0 {
            return None;
        }
        // The tip's place along the wall, s = s0 + t·s1, and its signed
        // distance from the wall's line, n = n0 + t·n1.
        let (s0, s1) = ((self.from - wall.start).dot(u) / u2, self.along.dot(u) / u2);
        let (lo, hi) = abreast(s0, s1)?;
        let l = u2.sqrt();
        let (n0, n1) = (u.cross(self.from - wall.start) / l, u.cross(self.along) / l);
        let room = |t: f64| n1.mul_add(t, n0).abs() - self.radius_mm(t);
        let mut candidates = [lo, hi, lo];
        if n1 != 0.0 {
            candidates[2] = (-n0 / n1).clamp(lo, hi);
        }
        Some(least_of(&candidates, room))
    }

    /// The least of `| |tip(t) − c| − ρ | − radius(t)` where the tip lies
    /// within arc wall `wall`'s sweep about its centre `c` (radius `ρ`), and
    /// where; `None` where it never does. The cut is split where it
    /// crosses the rays through the arc's ends, passes nearest `c`, or
    /// crosses the circle; on each stretch within the sweep the distance
    /// to the circle is convex outside it and concave inside it.
    fn to_circle(self, arc: Arc, wall: Segment) -> Option<(f64, f64)> {
        let c = arc.centre;
        let rho = arc.radius_mm;
        let from_c = self.from - c;
        let l2 = self.along.dot(self.along);
        if l2 <= 0.0 {
            return None;
        }
        let mut cuts = vec![0.0, 1.0, (-from_c.dot(self.along) / l2).clamp(0.0, 1.0)];
        for end in [wall.start, wall.end] {
            let ray = end - c;
            let across = ray.cross(self.along);
            if across != 0.0 {
                cuts.push(-ray.cross(from_c) / across);
            }
        }
        // |from_c + t·along|² = ρ²
        let b = from_c.dot(self.along);
        let disc = b * b - l2 * (from_c.dot(from_c) - rho * rho);
        if disc >= 0.0 {
            let root = disc.sqrt();
            cuts.push((-b - root) / l2);
            cuts.push((-b + root) / l2);
        }
        cuts.retain(|t| (0.0..=1.0).contains(t));
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut least: Option<(f64, f64)> = None;
        for pair in cuts.windows(2) {
            let &[lo, hi] = pair else { continue };
            let middle = self.tip((lo + hi) / 2.0);
            if !within_sweep(arc, middle) {
                continue;
            }
            let found = if middle.distance_to(c) >= rho {
                let (room_mm, t) = self.to_point(c, lo, hi);
                (room_mm - rho, t)
            } else {
                let room = |t: f64| rho - self.tip(t).distance_to(c) - self.radius_mm(t);
                least_of(&[lo, hi], room)
            };
            if least.is_none_or(|(r, _)| found.0 < r) {
                least = Some(found);
            }
        }
        least
    }
}

/// The `t` in [0, 1] where `s0 + t · s1` lies in [0, 1]; `None` when it
/// never does.
fn abreast(s0: f64, s1: f64) -> Option<(f64, f64)> {
    if s1 == 0.0 {
        return (0.0..=1.0).contains(&s0).then_some((0.0, 1.0));
    }
    let (a, b) = ((0.0 - s0) / s1, (1.0 - s0) / s1);
    let (lo, hi) = (a.min(b).max(0.0), a.max(b).min(1.0));
    (lo <= hi).then_some((lo, hi))
}

/// The least of `f` at `candidates`, and where.
fn least_of(candidates: &[f64], f: impl Fn(f64) -> f64) -> (f64, f64) {
    candidates
        .iter()
        .map(|&t| (f(t), t))
        .fold((f64::INFINITY, 0.0), |a, b| if b.0 < a.0 { b } else { a })
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;

    /// The least room of `ramp` against `wall`, read every `step` of `t`:
    /// the brute force the closed form must match from below.
    fn sampled(ramp: Ramp, wall: Segment, steps: u32) -> f64 {
        (0..=steps)
            .map(|k| {
                let t = f64::from(k) / f64::from(steps);
                point_segment_distance_mm(wall, ramp.tip(t)) - ramp.radius_mm(t)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// The closed form agrees with a fine sampling, never reading more
    /// room than it, for straight walls and arcs, convex and concave, and
    /// cuts that cross, pass beside, or run abreast of them.
    #[test]
    fn closed_form_room_matches_fine_sampling() {
        let walls = [
            Segment {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                bulge: 0.0,
            },
            Segment {
                start: Point::new(5.0, -5.0),
                end: Point::new(5.0, 5.0),
                bulge: 0.0,
            },
            // A half circle about (5, 0), radius 5, through (5, 5).
            Segment {
                start: Point::new(10.0, 0.0),
                end: Point::new(0.0, 0.0),
                bulge: 1.0,
            },
            // The same, clockwise, through (5, −5).
            Segment {
                start: Point::new(10.0, 0.0),
                end: Point::new(0.0, 0.0),
                bulge: -1.0,
            },
            // A quarter arc.
            Segment {
                start: Point::new(8.0, 3.0),
                end: Point::new(3.0, 8.0),
                bulge: (std::f64::consts::FRAC_PI_2 / 4.0).tan(),
            },
        ];
        let cuts = [
            ((-3.0, 2.0), (13.0, 2.5), 0.2, 2.5),
            ((5.0, 1.0), (5.0, 9.0), 3.0, 0.1),
            ((-2.0, -2.0), (12.0, 7.0), 0.0, 4.0),
            ((2.0, 1.0), (8.0, 1.5), 1.0, 1.0),
            ((4.0, 2.0), (6.0, 3.0), 0.5, 3.5),
            ((0.0, 9.0), (10.0, 9.0), 4.0, 0.0),
        ];
        for wall in walls {
            for &((ax, ay), (bx, by), ra, rb) in &cuts {
                let ramp = Ramp {
                    from: Point::new(ax, ay),
                    along: Point::new(bx, by) - Point::new(ax, ay),
                    from_r_mm: ra,
                    per_t_mm: rb - ra,
                };
                let (room_mm, t) = ramp.least_room(wall);
                let fine = sampled(ramp, wall, 20_000);
                assert!(
                    room_mm <= fine + 1e-12,
                    "{wall:?} {ramp:?}: {room_mm} > {fine}"
                );
                assert!(
                    fine - room_mm < 1e-3,
                    "{wall:?} {ramp:?}: {room_mm} ≪ {fine}"
                );
                let at = point_segment_distance_mm(wall, ramp.tip(t)) - ramp.radius_mm(t);
                assert!(
                    (at - room_mm).abs() < 1e-9,
                    "{wall:?} {ramp:?}: {at} at {t}"
                );
            }
        }
    }
}
