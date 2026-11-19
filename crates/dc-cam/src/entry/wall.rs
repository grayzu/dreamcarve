// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/entry.ts
//   src/engine/toolpaths/edge.ts

//! Entry on a Profile's wall (`docs/specs/phase-2/design.md`, "Algorithms →
//! Entry", Profile ramp and On line ramp). A Profile's entry point lies on
//! its tool-centre path, the edge of the waste domain it may enter, and
//! only the kerf along that path is cut at each pass; the waste beside it
//! is not. So, unlike a Pocket's, a Profile's descents cannot start
//! anywhere in the domain at the previous pass Z: every pass after the
//! first starts at the entry point, where the previous lap ended.
//!
//! - [`plan_at_wall`] places upstream's zigzag (`findRampPlacement`, as
//!   `edge.ts`'s `bandEntryPolicy` and `outsideEntryPolicy` use it) with
//!   one end on the entry point instead of centred on it: the longest
//!   chord from the point into the domain, shrunk by the boundary safety at
//!   its far end, at most three diameters. Each pass's zigzag then starts
//!   where the tool already is. The helix is placed as upstream's.
//! - [`EntryPlan::rejoin`] reaches the helix start at the previous pass Z
//!   through the bore the previous pass's helix cut there.
//! - [`along_path`] is DreamCarve's On line ramp: back and forth along the
//!   path itself (VCarve's Zig Zag ramp), since an On line Profile removes
//!   nothing but its kerf.

use super::{
    Area, Descent, ENTRY_EPS, EntryDomain, EntryKind, EntryPlan, EntryTool, FallbackReason,
    MAX_RAMP_RUN_DIAMETERS, MIN_RAMP_RUN_FRACTION, RAMP_DIRECTIONS, RAMP_MIN_LAST_DROP_MM,
    RampPlacement, boundary_safety_mm, count_within_cap, is_descent_move, move_feed_mm_min,
    plan_with, segment_distance_sq,
};
use crate::toolpath::Move;
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Vec2};
use dc_project::Entry;
use std::cmp::Ordering;
use std::f64::consts::{PI, SQRT_2};

/// [`super::plan`] for a target on a Profile's tool-centre path, the edge
/// of `domain`: the helix as [`super::plan`] places it, the ramp from the
/// target along the longest chord into the domain over
/// [`RAMP_DIRECTIONS`] directions either way, stopped the boundary safety
/// short of the far boundary, at most [`MAX_RAMP_RUN_DIAMETERS`] diameters
/// long. Its zigzag starts and ends its runs at the target, so a descent
/// from the previous pass Z starts on the floor that pass cut there.
#[must_use]
pub fn plan_at_wall(
    domain: &EntryDomain,
    entry: &Entry,
    target: Point,
    tool: &EntryTool,
) -> EntryPlan {
    plan_with(domain, entry, target, tool, place_wall_ramp)
}

/// The longest chord from `target` into the domain, for [`plan_at_wall`]:
/// longest run first, then most room beyond it, then the first direction.
fn place_wall_ramp(
    domain: &EntryDomain,
    target: Point,
    tool_diameter_mm: f64,
) -> Option<RampPlacement> {
    let safety = boundary_safety_mm(tool_diameter_mm);
    let max_run = tool_diameter_mm * MAX_RAMP_RUN_DIAMETERS;
    let min_run = ENTRY_EPS.max(tool_diameter_mm * MIN_RAMP_RUN_FRACTION);
    let mut best: Option<(Point, f64, f64)> = None;
    for area in domain.candidates(target) {
        for i in 0..RAMP_DIRECTIONS {
            let angle = PI * i as f64 / RAMP_DIRECTIONS as f64;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let Some((min, max)) = area.line_interval(target, direction, ENTRY_EPS) else {
                continue;
            };
            for (sign, room) in [(1.0, max), (-1.0, -min)] {
                let length = (room - safety).min(max_run);
                if length <= min_run || length.is_nan() {
                    continue;
                }
                if let Some((_, best_length, best_room)) = best {
                    match length.total_cmp(&best_length) {
                        Ordering::Less => continue,
                        Ordering::Equal if room <= best_room => continue,
                        _ => {}
                    }
                }
                best = Some((target + direction * (sign * length), length, room));
            }
        }
    }
    best.map(|(end, _, _)| RampPlacement { start: target, end })
}

impl EntryPlan {
    /// The target this plan enters at.
    #[must_use]
    pub fn target(&self) -> Point {
        self.target
    }

    /// `descent`, a helix from `z_mm` that this plan placed, started from
    /// the target instead of the helix start: the tool, at the target at
    /// `z_mm` where the previous lap ended, moves at the feed to the helix
    /// end (back along the handoff the previous pass cut) and across to
    /// the helix start, both inside the bore the previous pass's helix cut
    /// at `z_mm` (its flat revolution swept the whole circle, and a helix
    /// no wider than the tool leaves no core). The caller must know that
    /// the previous pass descended to `z_mm` on this plan's helix. Any
    /// other descent is returned as it is.
    #[must_use]
    pub fn rejoin(&self, mut descent: Descent, z_mm: f64) -> Descent {
        let Some(h) = self.helix.filter(|_| descent.used == EntryKind::Helix) else {
            return descent;
        };
        let feed = self.tool.feed_mm_min;
        let mut moves = Vec::with_capacity(descent.moves.len() + 2);
        let mut at = self.target;
        for to in [h.endpoint, descent.from] {
            if to.distance_to(at) > EPS_MM {
                moves.push(Move::linear(Point3::new(to.x, to.y, z_mm), feed));
                at = to;
            }
        }
        moves.append(&mut descent.moves);
        descent.moves = moves;
        descent.from = self.target;
        descent
    }

    /// The ends of the ramp run this plan placed, its target first; `None`
    /// when it placed none.
    #[must_use]
    pub fn ramp_line(&self) -> Option<(Point, Point)> {
        self.ramp.map(|r| (r.start, r.end))
    }

    /// #352: this plan with its ramp run from its target toward `end`
    /// instead, as far along that line as `domain` holds it, stopped the
    /// boundary safety short of the far boundary and never past `end`,
    /// and with no helix: a pass below the tab top ramps along the line
    /// the passes above it ramped on, so through stock they cut, as far as
    /// the tabs leave it. `None` when that stretch is no longer than a ramp
    /// run may be short (`MIN_RAMP_RUN_FRACTION` of the diameter).
    #[must_use]
    pub fn following(&self, domain: &EntryDomain, end: Point) -> Option<Self> {
        let length_mm = self.target.distance_to(end);
        if length_mm <= ENTRY_EPS || length_mm.is_nan() {
            return None;
        }
        let direction = (end - self.target) * (1.0 / length_mm);
        let room_mm = domain
            .candidates(self.target)
            .iter()
            .filter_map(|area| area.line_interval(self.target, direction, ENTRY_EPS))
            .map(|(_, max)| max)
            .fold(f64::NEG_INFINITY, f64::max);
        let diameter_mm = self.tool.diameter_mm;
        let run_mm = length_mm.min(room_mm - boundary_safety_mm(diameter_mm));
        let min_run_mm = ENTRY_EPS.max(diameter_mm * MIN_RAMP_RUN_FRACTION);
        (run_mm > min_run_mm).then(|| Self {
            helix: None,
            ramp: Some(RampPlacement {
                start: self.target,
                end: self.target + direction * run_mm,
            }),
            ..self.clone()
        })
    }

    /// #356: this plan with its ramp run cut short to `run_mm` from its
    /// start, along the same line, and with no helix: a pass ramps only as
    /// far along the line as the pass above cut it below the stock top.
    /// The plan itself when its run is no longer; `None` when it places no
    /// ramp, or when `run_mm` is no longer than a ramp run may be short
    /// (`MIN_RAMP_RUN_FRACTION` of the diameter).
    #[must_use]
    pub fn cut_short(&self, run_mm: f64) -> Option<Self> {
        let r = self.ramp?;
        let length_mm = r.start.distance_to(r.end);
        if length_mm <= run_mm {
            return Some(self.clone());
        }
        let min_run_mm = ENTRY_EPS.max(self.tool.diameter_mm * MIN_RAMP_RUN_FRACTION);
        (run_mm > min_run_mm).then(|| Self {
            helix: None,
            ramp: Some(RampPlacement {
                start: r.start,
                end: r.start + (r.end - r.start) * (run_mm / length_mm),
            }),
            ..self.clone()
        })
    }

    /// [`EntryPlan::descend`] without the helix: the chain from the ramp,
    /// a requested helix reported as not used for `reason`.
    #[must_use]
    pub fn descend_without_helix(
        &self,
        start_z_mm: f64,
        z_mm: f64,
        reason: FallbackReason,
    ) -> Descent {
        self.descend_with(start_z_mm, z_mm, Some(reason))
    }
}

impl EntryDomain {
    /// The signed distance from `p` to the nearest wall of the region
    /// holding it, mm: positive inside a region (in its outer, in none of
    /// its holes), negative outside every region, by the distance to the
    /// nearest. Arcs are the chords [`EntryDomain::new`] flattened them to.
    pub(crate) fn clearance_mm(&self, p: Point) -> f64 {
        self.areas
            .iter()
            .map(|area| area.signed_distance(p))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// S9 for one descent: the index in `moves`, cut from `from`, of the
    /// first descent move ([`is_descent_move`]) whose tool centre leaves
    /// every region of the domain by more than `slack_mm`.
    pub(crate) fn first_move_outside(
        &self,
        from: Point3,
        moves: &[Move],
        slack_mm: f64,
    ) -> Option<usize> {
        let mut at = from;
        for (i, mv) in moves.iter().enumerate() {
            if is_descent_move(at, mv) {
                let (a, b) = (at.xy(), mv.to.xy());
                let inside = self.areas.iter().any(|area: &Area| {
                    area.contains_within(a, slack_mm)
                        && area.contains_within(b, slack_mm)
                        && area.segment_inside(a, b, slack_mm)
                });
                if !inside {
                    return Some(i);
                }
            }
            at = mv.to;
        }
        None
    }
}

/// A path flattened to points, with the distance along it of each.
struct Flat {
    points: Vec<Point>,
    at_mm: Vec<f64>,
}

impl Flat {
    /// `path` with arcs split into chords within `tolerance_mm`; a closed
    /// path repeats its first point at the end.
    fn new(path: &Polyline, tolerance_mm: f64) -> Self {
        let flat = path.with_arcs_flattened(tolerance_mm);
        let mut points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
        if flat.is_closed()
            && let Some(&first) = points.first()
        {
            points.push(first);
        }
        points.dedup_by(|b, a| a.distance_to(*b) <= EPS_MM);
        let mut at_mm = Vec::with_capacity(points.len());
        let mut walked = 0.0;
        let mut previous: Option<Point> = None;
        for &p in &points {
            if let Some(q) = previous {
                walked += q.distance_to(p);
            }
            at_mm.push(walked);
            previous = Some(p);
        }
        Self { points, at_mm }
    }

    fn length_mm(&self) -> f64 {
        self.at_mm.last().copied().unwrap_or(0.0)
    }

    /// The point `s_mm` along the path, clamped to it.
    fn point_at(&self, s_mm: f64) -> Point {
        let i = self.at_mm.partition_point(|&a| a < s_mm);
        match (
            i.checked_sub(1)
                .and_then(|k| self.at_mm.get(k).zip(self.points.get(k))),
            self.at_mm.get(i).zip(self.points.get(i)),
        ) {
            (Some((&a0, &p0)), Some((&a1, &p1))) if a1 - a0 > 0.0 => {
                p0 + (p1 - p0) * ((s_mm - a0) / (a1 - a0))
            }
            (_, Some((_, &p))) | (Some((_, &p)), None) => p,
            (None, None) => Point::new(0.0, 0.0),
        }
    }

    /// The distances of the vertices strictly between `from_mm` and
    /// `to_mm`, in the order travelled from one to the other.
    fn between(&self, from_mm: f64, to_mm: f64) -> Vec<f64> {
        let (lo, hi) = if from_mm <= to_mm {
            (from_mm, to_mm)
        } else {
            (to_mm, from_mm)
        };
        let mut out: Vec<f64> = self
            .at_mm
            .iter()
            .copied()
            .filter(|&a| a > lo + EPS_MM && a < hi - EPS_MM)
            .collect();
        if from_mm > to_mm {
            out.reverse();
        }
        out
    }
}

/// The On line ramp (design spec, "On line ramp"): from the start vertex
/// of `path` at `start_z_mm`, forward along the path for `run = min(3·D,
/// L, max_run_mm)` and back, again and again, each leg descending at
/// `angle_deg` until `z_mm`, the final leg cut short there, or sharing the
/// depth with the leg before when it would fall less than
/// [`RAMP_MIN_LAST_DROP_MM`] (`plan_legs`; #234, #273); arcs are split
/// into chords within `tolerance_mm` while Z changes, and a chord the
/// program cannot write is merged into its neighbour
/// (`merge_unwritable`), else the ramp falls back to a plunge with
/// [`FallbackReason::TooSmall`] (#273). The tool then
/// returns along the path at `z_mm` to the start vertex, where the lap
/// starts, so the ramped stretch is cut again at full depth; a piece of
/// that return shorter than the program can write is merged as the
/// descent's are (#321). On an open
/// path shorter than a leg the legs turn at its end. Every descending move
/// runs at the feed scaled so its vertical rate is at most the plunge rate
/// (S10). A run of no more than `0.02·D`, or one that would need more than
/// [`super::MAX_DESCENT_MOVES`] legs, falls back to a plunge. `max_run_mm`
/// keeps the ramp short of a tab. `None` for a path with no vertex, which
/// has nowhere to start: the descent was once started at the origin.
#[must_use]
pub fn along_path(
    path: &Polyline,
    angle_deg: f64,
    max_run_mm: f64,
    start_z_mm: f64,
    z_mm: f64,
    tool: &EntryTool,
    tolerance_mm: f64,
) -> Option<Descent> {
    let flat = Flat::new(path, tolerance_mm);
    // The flattened path has the path's vertices, so its first.
    let from = *flat.points.first()?;
    let mut descent = Descent {
        from,
        moves: Vec::new(),
        requested: EntryKind::Ramp,
        used: EntryKind::Ramp,
        reason: None,
        helix: None,
    };
    let depth_mm = start_z_mm - z_mm;
    if depth_mm <= EPS_MM || depth_mm.is_nan() {
        return Some(descent);
    }
    let run_mm = (tool.diameter_mm * MAX_RAMP_RUN_DIAMETERS)
        .min(flat.length_mm())
        .min(max_run_mm);
    let min_run = ENTRY_EPS.max(tool.diameter_mm * MIN_RAMP_RUN_FRACTION);
    let slope = angle_deg.to_radians().tan();
    let legs = if run_mm > min_run && slope > ENTRY_EPS {
        count_within_cap(depth_mm / (run_mm * slope)).ok_or(FallbackReason::TooManyMoves)
    } else {
        Err(FallbackReason::NoRoom)
    };
    let legs = match legs {
        Ok(n) => n,
        Err(reason) => {
            descent.used = EntryKind::Plunge;
            descent.reason = Some(reason);
            return Some(descent);
        }
    };
    let mut current = Point3::new(from.x, from.y, start_z_mm);
    let mut moves = Vec::new();
    let leg_plan = plan_legs(legs, run_mm, slope, depth_mm);
    let mut s_mm = 0.0;
    for (i, &(leg_from, leg_to, leg_slope)) in leg_plan.iter().enumerate() {
        let leg_z = current.z;
        let last = i + 1 == leg_plan.len();
        let mut points = vec![current];
        let mut stops = flat.between(leg_from, leg_to);
        stops.push(leg_to);
        for stop in stops {
            let p = flat.point_at(stop);
            let z = leg_z - (stop - leg_from).abs() * leg_slope;
            points.push(Point3::new(p.x, p.y, z));
        }
        if last && let Some(end) = points.last_mut() {
            end.z = z_mm;
        }
        // #273: a piece of the leg the program cannot write is merged into
        // its neighbour, or the ramp is not cut.
        if !merge_unwritable(&mut points, tool.quantum_mm) {
            descent.used = EntryKind::Plunge;
            descent.reason = Some(FallbackReason::TooSmall);
            return Some(descent);
        }
        for next in points.into_iter().skip(1) {
            moves.push(Move::linear(next, move_feed_mm_min(tool, current, next)));
            current = next;
        }
        s_mm = leg_to;
    }
    // Back along the path to the start vertex, at the pass Z.
    let mut stops = flat.between(s_mm, 0.0);
    stops.push(0.0);
    let mut back = vec![current];
    for stop in stops {
        let p = flat.point_at(stop);
        if back
            .last()
            .is_some_and(|last| p.distance_to(last.xy()) > EPS_MM)
        {
            back.push(Point3::new(p.x, p.y, z_mm));
        }
    }
    // #321: a leg that ends just past a chord vertex leaves a level piece
    // to that vertex too short to write. The program writes it as nothing,
    // so arc fitting would count a chord S12 does not see. It is merged
    // as the descent's pieces are; if it cannot be, the way back is kept.
    let mut merged = back.clone();
    if merge_short(&mut merged, tool.quantum_mm, |a, b| {
        a.xy().distance_to(b.xy()) < SQRT_2 * tool.quantum_mm
    }) {
        back = merged;
    }
    for next in back.into_iter().skip(1) {
        moves.push(Move::linear(next, tool.feed_mm_min));
    }
    descent.moves = moves;
    Some(descent)
}

/// The legs of an On line ramp of `legs` legs over `depth_mm`, along a run
/// of `run_mm` at `slope` (tan of the angle): each leg's start and end,
/// distances along the path, and its slope. Legs alternate out from the
/// start vertex and back, each falling `run_mm × slope`; the last is cut
/// short where it reaches the depth. When that last leg would fall less
/// than [`RAMP_MIN_LAST_DROP_MM`], the last two legs share their depth
/// instead, each at the angle, the first cut short and the second back to
/// where the first began, as the Pocket ramp's do (#234); a ramp of one
/// such leg runs the whole leg, shallower. (Changed in #273: the shallow
/// last leg of #234 fell so gently that its chords along an arc each fell
/// less than an inch program's last place.)
fn plan_legs(legs: usize, run_mm: f64, slope: f64, depth_mm: f64) -> Vec<(f64, f64, f64)> {
    let full_mm = run_mm * slope;
    let rest_mm = depth_mm - legs.saturating_sub(1) as f64 * full_mm;
    let ends = |leg: usize| {
        if leg.is_multiple_of(2) {
            (0.0, run_mm)
        } else {
            (run_mm, 0.0)
        }
    };
    let toward = |from: f64, to: f64, reach_mm: f64| {
        if to >= from {
            from + reach_mm
        } else {
            from - reach_mm
        }
    };
    let mut plan: Vec<(f64, f64, f64)> = (0..legs)
        .map(|leg| {
            let (from, to) = ends(leg);
            (from, to, slope)
        })
        .collect();
    if rest_mm >= RAMP_MIN_LAST_DROP_MM {
        if let Some(last) = plan.last_mut() {
            last.1 = toward(last.0, last.1, (rest_mm / slope).min(run_mm));
        }
    } else if legs >= 2 {
        let reach_mm = (full_mm + rest_mm) / (2.0 * slope);
        let (from, to) = ends(legs - 2);
        let turn = toward(from, to, reach_mm.min(run_mm));
        plan.truncate(legs - 2);
        plan.push((from, turn, slope));
        plan.push((turn, from, slope));
    } else if let Some(only) = plan.first_mut() {
        only.2 = depth_mm / run_mm;
    }
    plan
}

/// #273: merges each piece of `points`, one leg's descent through its
/// stops from its start, that falls less than `quantum_mm` or runs less
/// than √2 of it in XY, into a neighbour, so every move the program writes
/// keeps two Z values and two XY points and so its slope (S3, S10, as
/// [`super::helix_writable`] for the helix). The stop between them goes:
/// the piece's end, or its start when the piece ends the leg; a leg's ends
/// stay. False when a leg of one piece is still too short, or when a stop
/// lies more than a unit off the merged move, which would leave the path.
fn merge_unwritable(points: &mut Vec<Point3>, quantum_mm: f64) -> bool {
    merge_short(points, quantum_mm, |a, b| {
        a.z - b.z < quantum_mm || a.xy().distance_to(b.xy()) < SQRT_2 * quantum_mm
    })
}

/// Merges each piece of `points` that is `short` into a neighbour, as
/// [`merge_unwritable`] says; false as it is.
fn merge_short(
    points: &mut Vec<Point3>,
    quantum_mm: f64,
    short: impl Fn(&Point3, &Point3) -> bool,
) -> bool {
    while let Some(i) = points
        .windows(2)
        .position(|w| matches!(w, [a, b] if short(a, b)))
    {
        let gone = if i + 2 < points.len() { i + 1 } else { i };
        let (Some(before), Some(at), Some(after)) = (
            gone.checked_sub(1).and_then(|k| points.get(k)),
            points.get(gone),
            points.get(gone + 1),
        ) else {
            return false;
        };
        if segment_distance_sq(at.xy(), before.xy(), after.xy()).sqrt() > quantum_mm {
            return false;
        }
        points.remove(gone);
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests;
