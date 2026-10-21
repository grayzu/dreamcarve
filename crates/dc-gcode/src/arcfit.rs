// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/arcFitting.ts
//   src/engine/toolpaths/arcReconstruction.ts
//   src/engine/gcode/arcFitting.test.ts

//! Arc fitting at export (`docs/specs/phase-2/design.md`, "Algorithms →
//! Arc fitting"; `docs/specs/phase-2/functional.md`, capability 24, S12).
//!
//! [`fit`] finds runs of consecutive straight cutting moves at one Z and
//! one feed that lie on a circle within the project's tolerance and
//! replaces each with sub-arcs of at most [`ARC_FIT_MAX_SWEEP_DEG`]. The
//! emitter then judges every sub-arc as the controller will once its
//! numbers are rounded ([`resolve_emitted_arc`]) and reverts any run with a
//! rejected sub-arc to its original moves ([`apply_fallback`]).
//!
//! Ported from `fitArcsInMachineMoves`, `splitArc`, `resolveEmittedArc`,
//! `runSurvivesFormatting` and `applyEmittedArcFallback` (`arcFitting.ts`)
//! and `findArcRunsInPoints` with its gates (`arcReconstruction.ts`).
//! Changes from upstream:
//!
//! - Fitting runs in project space (Y-up, mm) before the machine
//!   transform, which is a translation and a signed axis map, never a
//!   mirror; so a negative cross sum about the centre is clockwise and the
//!   emitter picks the command from that, as it does for toolpath arcs.
//!   Upstream fits machine coordinates after its Y flip.
//! - A toolpath arc whose sagitta is within the tolerance (the corner arcs
//!   an offset puts between the facets of an imported curve, which the
//!   coaster has 160 of per lap) joins a run as its chord. Its midpoint is
//!   checked against the fitted circle like a vertex, so the fitted arc
//!   stays within the tolerance of the original path. Upstream has no such
//!   arcs: its offsets are polylines.
//! - The arc must stay within the tolerance of every move it replaces
//!   along its whole length, not only at the vertices
//!   ([`arc_chord_deviation_mm`], #226, #229). Upstream's offsets are
//!   densely sampled polylines, so its vertex check is enough there;
//!   DreamCarve keeps long straight edges, and a square's four corners
//!   lie on one circle.
//! - A run with no known emitted start falls back to its moves instead of
//!   being judged from its own start.
//! - The Kasa fit is `dc_geometry::fit_circle_kasa`.

use std::cmp::Ordering;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use dc_cam::{Move, MoveKind};
use dc_geometry::{
    EPS_MM, Point, Point3, Segment, Vec2, approx_eq, fit_circle_kasa, point_segment_distance_mm,
};
use dc_project::ArcFormat;

/// Most one fitted sub-arc sweeps, degrees. A full circle becomes four.
pub const ARC_FIT_MAX_SWEEP_DEG: f64 = 90.0;

/// GRBL's arc radius check (`grbl/gcode.c`): an arc whose start and end
/// radii differ by at most this is accepted outright.
pub const ARC_RADIUS_CONSISTENCY_MM: f64 = 0.005;

/// Above this disagreement no relative allowance applies.
pub const ARC_RADIUS_GROSS_MM: f64 = 0.5;

/// Between the two, the disagreement may be this fraction of the radius.
pub const ARC_RADIUS_RELATIVE: f64 = 0.001;

/// Most points one fit considers: the greedy window. A 5° flattener gives
/// 73 points for a full circle, so this preserves that path while bounding
/// the search on a long non-circular polyline.
pub const MAX_CANDIDATE_POINTS: usize = 128;

/// Fewest chords a run replaces. A fitted arc in a program must cover at
/// least this many toolpath moves (S12).
pub const MIN_RUN_CHORDS: usize = 3;

/// Fewest points a run has: [`MIN_RUN_CHORDS`] chords.
const MIN_RUN_POINTS: usize = MIN_RUN_CHORDS + 1;

/// The smallest total sweep a run may have, radians (0.5°): a residual-only
/// fit would turn a very shallow bend into a huge-radius arc.
const MIN_TOTAL_SWEEP_RAD: f64 = PI / 360.0;

/// The largest angular step between consecutive points, radians.
const MAX_STEP_RAD: f64 = FRAC_PI_2;

/// The largest step may be at most this many times the median step: a long
/// straight move plus tiny corner chords can pass an endpoint-only fit.
const MAX_STEP_RATIO: f64 = 4.0;

/// A run's chord must be at least this fraction of the radius unless it
/// closes on itself.
const MIN_CHORD_RATIO: f64 = 0.15;

/// How far around the rounded I and J the snap looks, in output quanta.
const SNAP_SEARCH_STEPS: i32 = 2;

/// Two Z values this close are one level.
const SAME_Z_MM: f64 = EPS_MM;

/// Floating-point slack in sweep comparisons.
const SWEEP_EPS: f64 = 1e-12;

/// One item of a fitted toolpath.
#[derive(Debug, Clone, PartialEq)]
pub enum Fitted {
    /// A move written as it is: a rapid, a plunge, a toolpath arc, or a
    /// straight move no run took.
    Move(Move),
    /// A run of straight moves written as arcs.
    Arc(ArcRun),
}

/// A run of straight moves fitted to one circle, split into sub-arcs.
#[derive(Debug, Clone, PartialEq)]
pub struct ArcRun {
    /// The sub-arcs, in order, each at most [`ARC_FIT_MAX_SWEEP_DEG`]. The
    /// first starts at the run's first point and the last ends exactly at
    /// its last; the ends between are on the circle.
    pub arcs: Vec<SubArc>,
    /// The run's feed, mm/min.
    pub feed_mm_min: f64,
    /// The moves the run replaced, in order. Written instead when the
    /// controller would reject any sub-arc once rounded: a partial
    /// fallback would strand the tool at a sub-arc end no original move
    /// reaches.
    pub fallback: Vec<Move>,
}

/// One arc of a fitted run, project space, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubArc {
    /// Where it starts.
    pub start: Point3,
    /// Where it ends; the same Z as the start.
    pub end: Point3,
    /// The circle's centre.
    pub centre: Point,
    /// Clockwise viewed from +Z (Y-up): `G2` on a machine that does not
    /// mirror the plane.
    pub clockwise: bool,
}

impl SubArc {
    /// The I and J words: the centre relative to the start.
    #[must_use]
    pub fn centre_offsets(&self) -> Vec2 {
        self.centre - self.start.xy()
    }
}

/// Fits arcs to `moves`, a toolpath in project space, within
/// `tolerance_mm`: every vertex of a replaced run, and the midpoint of
/// every shallow corner arc in it, lies within `tolerance_mm` of the
/// fitted circle, and the fitted arc lies within `tolerance_mm` of every
/// move it replaces along its whole length. Runs are maximal sequences of consecutive straight cuts
/// at one Z with one feed, started from a known position; rapids,
/// plunges, Z-changing moves, and arcs deeper than the tolerance pass
/// through unchanged and end runs. Fewer than [`MIN_RUN_POINTS`] points
/// stay as they are.
#[must_use]
pub fn fit(moves: &[Move], tolerance_mm: f64) -> Vec<Fitted> {
    let mut out = Vec::with_capacity(moves.len());
    let mut position: Option<Point3> = None;
    let mut index = 0;
    while let Some(first) = moves.get(index) {
        let Some(start) = position else {
            // No known start: the move passes through.
            out.push(Fitted::Move(*first));
            position = Some(first.to);
            index += 1;
            continue;
        };
        // The longest run from `index`: the feed and Z of its first move.
        let run_feed = run_feed(first);
        let mut run: Vec<RunMove> = Vec::new();
        let mut last = start;
        for mv in moves.get(index..).unwrap_or_default() {
            let Some(member) = RunMove::new(last, mv, start.z, run_feed, tolerance_mm) else {
                break;
            };
            run.push(member);
            last = mv.to;
        }
        if run.len() < MIN_RUN_CHORDS {
            let taken = run.len().max(1);
            for mv in moves.get(index..index + taken).unwrap_or_default() {
                out.push(Fitted::Move(*mv));
            }
            position = Some(moves.get(index + taken - 1).map_or(last, |m| m.to));
            index += taken;
            continue;
        }

        let points: Vec<Point> = std::iter::once(start.xy())
            .chain(run.iter().map(|m| m.to.xy()))
            .collect();
        let mids: Vec<Option<Point>> = run.iter().map(|m| m.midpoint).collect();
        let found = find_arc_runs(&points, &mids, tolerance_mm);

        // Walk the run: moves before each found arc pass through, the arc
        // replaces its moves, and whatever follows the last arc passes.
        let feed_mm_min = run_feed.unwrap_or(0.0);
        let mut cursor = 0;
        for arc in &found {
            for m in run.get(cursor..arc.start_index).unwrap_or_default() {
                out.push(Fitted::Move(m.original));
            }
            let arc_start = run
                .get(arc.start_index.wrapping_sub(1))
                .map_or(start, |m| m.to);
            let arc_end = run.get(arc.end_index - 1).map_or(last, |m| m.to);
            let arcs = split_arc(arc_start, arc_end, arc.centre, arc.clockwise);
            let fallback = run
                .get(arc.start_index..arc.end_index)
                .unwrap_or_default()
                .iter()
                .map(|m| m.original)
                .collect();
            out.push(Fitted::Arc(ArcRun {
                arcs,
                feed_mm_min,
                fallback,
            }));
            cursor = arc.end_index;
        }
        for m in run.get(cursor..).unwrap_or_default() {
            out.push(Fitted::Move(m.original));
        }
        position = Some(last);
        index += run.len();
    }
    out
}

/// A cutting move's feed, if it is usable.
fn run_feed(mv: &Move) -> Option<f64> {
    mv.feed_mm_min.filter(|f| f.is_finite() && *f > 0.0)
}

/// One move of a candidate run.
#[derive(Debug, Clone, Copy)]
struct RunMove {
    original: Move,
    to: Point3,
    /// The midpoint of a shallow corner arc, checked like a vertex.
    midpoint: Option<Point>,
}

impl RunMove {
    /// `mv`, from `from`, as a member of a run at `z_mm` with `feed`, if it
    /// can be one: a straight cut at that Z and feed, or a toolpath arc
    /// whose sagitta is within `tolerance_mm`.
    fn new(
        from: Point3,
        mv: &Move,
        z_mm: f64,
        feed: Option<f64>,
        tolerance_mm: f64,
    ) -> Option<Self> {
        let feed = feed?;
        if !run_feed(mv).is_some_and(|f| approx_eq(f, feed, EPS_MM)) {
            return None;
        }
        if !approx_eq(mv.to.z, z_mm, SAME_Z_MM) || !approx_eq(from.z, z_mm, SAME_Z_MM) {
            return None;
        }
        let midpoint = match mv.kind {
            MoveKind::Rapid | MoveKind::Dwell { .. } => return None,
            MoveKind::Linear => None,
            MoveKind::ArcCw { centre } => Some(shallow_arc_midpoint(
                from.xy(),
                mv.to.xy(),
                centre,
                true,
                tolerance_mm,
            )?),
            MoveKind::ArcCcw { centre } => Some(shallow_arc_midpoint(
                from.xy(),
                mv.to.xy(),
                centre,
                false,
                tolerance_mm,
            )?),
        };
        Some(Self {
            original: *mv,
            to: mv.to,
            midpoint,
        })
    }
}

/// The midpoint of the arc from `start` to `end` about `centre` if the arc
/// is shallow: its sagitta, how far its middle bows from its chord, is
/// within `tolerance_mm`, so its chord stands for it within the tolerance.
/// `None` for a deeper arc, or one too small to have a direction.
#[must_use]
pub fn shallow_arc_midpoint(
    start: Point,
    end: Point,
    centre: Point,
    clockwise: bool,
    tolerance_mm: f64,
) -> Option<Point> {
    let radius_mm = start.distance_to(centre);
    if !matches!(radius_mm.partial_cmp(&EPS_MM), Some(Ordering::Greater)) {
        return None;
    }
    let start_rad = (start.y - centre.y).atan2(start.x - centre.x);
    let end_rad = (end.y - centre.y).atan2(end.x - centre.x);
    let sweep_rad = if clockwise {
        start_rad - end_rad
    } else {
        end_rad - start_rad
    }
    .rem_euclid(TAU);
    let sagitta_mm = radius_mm * (1.0 - (sweep_rad / 2.0).cos());
    if !matches!(
        sagitta_mm.partial_cmp(&tolerance_mm),
        Some(Ordering::Less | Ordering::Equal)
    ) {
        return None;
    }
    let mid_rad = start_rad + if clockwise { -1.0 } else { 1.0 } * sweep_rad / 2.0;
    Some(Point::new(
        centre.x + radius_mm * mid_rad.cos(),
        centre.y + radius_mm * mid_rad.sin(),
    ))
}

/// Interior points sampled along an arc by [`arc_chord_deviation_mm`], on
/// top of its ends and the point farthest from the chord's line.
const DEVIATION_SAMPLES: u32 = 8;

/// The farthest, mm, any point of the arc of the circle about `centre`
/// with radius `radius_mm`, from angle `from_rad` through `sweep_rad`
/// (counter-clockwise when positive), lies from the straight move `a` to
/// `b` it stands for (#226, #229). For a chord whose ends are on the
/// circle this is its sagitta, `r(1 − cos(sweep/2))`; for ends off it, it
/// also counts how far they are off. Checked at the arc's ends, at the
/// point of the arc farthest from the chord's line on either side, and at
/// [`DEVIATION_SAMPLES`] points between.
#[must_use]
pub fn arc_chord_deviation_mm(
    centre: Point,
    radius_mm: f64,
    from_rad: f64,
    sweep_rad: f64,
    a: Point,
    b: Point,
) -> f64 {
    let chord = Segment {
        start: a,
        end: b,
        bulge: 0.0,
    };
    let at = |angle: f64| {
        Point::new(
            centre.x + radius_mm * angle.cos(),
            centre.y + radius_mm * angle.sin(),
        )
    };
    let sweep = sweep_rad.abs();
    let on_arc = |angle: f64| {
        let turn = if sweep_rad >= 0.0 {
            angle - from_rad
        } else {
            from_rad - angle
        };
        turn.rem_euclid(TAU) <= sweep
    };
    // Towards the chord's middle: where the arc bows farthest from it.
    let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let normal_rad = (middle.y - centre.y).atan2(middle.x - centre.x);
    let samples = (0..=DEVIATION_SAMPLES + 1)
        .map(|k| sweep_rad * f64::from(k) / f64::from(DEVIATION_SAMPLES + 1))
        .map(|t| from_rad + t);
    let farthest = [normal_rad, normal_rad + PI]
        .into_iter()
        .filter(|&angle| on_arc(angle));
    samples
        .chain(farthest)
        .map(|angle| point_segment_distance_mm(chord, at(angle)))
        .fold(0.0, f64::max)
}

/// One arc found in a point sequence: points `start_index..=end_index`.
#[derive(Debug, Clone, Copy)]
struct FoundArc {
    start_index: usize,
    end_index: usize,
    centre: Point,
    clockwise: bool,
}

/// Greedy longest-first arc search over `points`, where `mids[k]` is the
/// midpoint of the shallow arc from `points[k]` to `points[k + 1]`, if
/// that move is one. From each position the longest window within
/// [`MAX_CANDIDATE_POINTS`] whose constrained fit passes every gate is
/// taken and the walk resumes at its end; otherwise the walk advances one
/// point. Ported from `findArcRunsInPoints` with `endpointConstrained`.
fn find_arc_runs(points: &[Point], mids: &[Option<Point>], tolerance_mm: f64) -> Vec<FoundArc> {
    let n = points.len();
    let mut found = Vec::new();
    let mut i = 0;
    while i + MIN_RUN_POINTS <= n {
        let min_end = i + MIN_RUN_POINTS - 1;
        let max_end = (n - 1).min(i + MAX_CANDIDATE_POINTS - 1);
        let mut best = None;
        for end in (min_end..=max_end).rev() {
            let Some(sub) = points.get(i..=end) else {
                continue;
            };
            let Some((centre, radius_mm)) = fit_circle_kasa(sub) else {
                continue;
            };
            let (Some(&first), Some(&last)) = (sub.first(), sub.last()) else {
                continue;
            };
            // Constrain before validating, so every gate judges the circle
            // that will be emitted: it passes exactly through both ends.
            let (centre, radius_mm) = constrain_to_endpoints(centre, radius_mm, first, last)
                .unwrap_or((centre, radius_mm));
            let sub_mids = mids.get(i..end).unwrap_or_default();
            if !gates_pass(sub, sub_mids, centre, radius_mm, tolerance_mm) {
                continue;
            }
            best = Some(FoundArc {
                start_index: i,
                end_index: end,
                centre,
                clockwise: cross_sum(sub, centre) < 0.0,
            });
            break;
        }
        match best {
            Some(arc) => {
                i = arc.end_index;
                found.push(arc);
            }
            None => i += 1,
        }
    }
    found
}

/// Moves a fitted circle onto the perpendicular bisector of `start` and
/// `end`: the closest such circle to the fit, through both points exactly.
/// `None` when the ends coincide (a closed run), where the bisector is
/// undefined and the fit is already continuous at its one end.
fn constrain_to_endpoints(
    centre: Point,
    radius_mm: f64,
    start: Point,
    end: Point,
) -> Option<(Point, f64)> {
    let d = end - start;
    let chord2 = d.dot(d);
    let tiny = radius_mm * 1e-6;
    if chord2 <= tiny * tiny {
        return None;
    }
    let mid = Point::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
    let along = d.perp();
    let t = (centre - mid).dot(along) / chord2;
    let constrained = mid + along * t;
    Some((constrained, start.distance_to(constrained)))
}

/// The gates of `validateArcFitPoints` for the export path: residual,
/// step angle, step uniformity, total sweep, and chord ratio; and, not
/// upstream's, two chord steps of one size at least (#213) and every move
/// within the tolerance of the arc along its whole length (#226, #229).
fn gates_pass(
    points: &[Point],
    mids: &[Option<Point>],
    centre: Point,
    radius_mm: f64,
    tolerance_mm: f64,
) -> bool {
    // 1. Every vertex, and every shallow arc's midpoint, is on the circle.
    let on_circle = |p: &Point| (p.distance_to(centre) - radius_mm).abs() <= tolerance_mm;
    if !points.iter().all(on_circle) || !mids.iter().flatten().all(on_circle) {
        return false;
    }
    // 2. No step over MAX_STEP_RAD; 3. the largest chord step within
    // MAX_STEP_RATIO of the median chord step, which tolerates one small
    // end fragment. A shallow corner arc is a joint between chords, not a
    // chord, so its small step is left out of the median: on a faceted
    // curve every other move is one, and counting them would fail every
    // window with an odd number of steps.
    let angle = |p: &Point| (p.y - centre.y).atan2(p.x - centre.x);
    let mut steps = Vec::with_capacity(points.len());
    let mut total_sweep = 0.0;
    // 2a. Every step turns the same way round the centre: a run that goes
    // back along the circle (a Profile ramp's return to its entry point,
    // then the lap) is not one arc, though every point is on it.
    let mut turn = 0.0_f64;
    for (k, pair) in points.windows(2).enumerate() {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
            return false;
        };
        let mut diff = angle(b) - angle(a);
        while diff > PI {
            diff -= TAU;
        }
        while diff <= -PI {
            diff += TAU;
        }
        let step = diff.abs();
        if step > MAX_STEP_RAD {
            return false;
        }
        // 1a. The arc stays within the tolerance of every move it
        // replaces, not only of its ends (#226, #229): a square's four
        // corners lie on one circle, which bows 1.45 mm off a 7 mm side.
        // A shallow corner arc counts as its chord: both bow the same way,
        // each by at most the tolerance.
        if arc_chord_deviation_mm(centre, radius_mm, angle(a), diff, *a, *b) > tolerance_mm {
            return false;
        }
        if step > SWEEP_EPS {
            if turn * diff < 0.0 {
                return false;
            }
            turn = diff;
        }
        if mids.get(k).is_none_or(Option::is_none) {
            steps.push(step);
        }
        total_sweep += step;
    }
    let Some(median) = median(&mut steps) else {
        return false;
    };
    let largest = steps.iter().copied().fold(0.0, f64::max);
    if median <= SWEEP_EPS || largest > median * MAX_STEP_RATIO {
        return false;
    }
    // 3a. At least two chord steps of the largest's size class (#213):
    // one long straight move, a short fragment and a shallow corner arc
    // put four points on any circle through the move's ends, and the
    // median of two steps tolerates the fragment, so a 40 mm wall side
    // became an arc bowing 1.3 mm into the wall.
    let full = steps
        .iter()
        .filter(|&&s| s * MAX_STEP_RATIO >= largest)
        .count();
    if full < 2 {
        return false;
    }
    // 4. Enough total sweep to be an arc at all.
    if total_sweep < MIN_TOTAL_SWEEP_RAD {
        return false;
    }
    // 5. A chord that is a fair part of the radius, unless the run closes.
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return false;
    };
    let chord = first.distance_to(*last);
    let closed = chord <= radius_mm * 1e-6;
    closed || chord >= radius_mm * MIN_CHORD_RATIO
}

/// The median of `values`, which it sorts; `None` if empty.
fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let n = values.len();
    let middle = n / 2;
    if n.is_multiple_of(2) {
        let (a, b) = (values.get(middle.wrapping_sub(1))?, values.get(middle)?);
        Some((a + b) / 2.0)
    } else {
        values.get(middle).copied()
    }
}

/// The sum of cross products of consecutive points about `centre`:
/// negative when the points go round clockwise (Y-up).
fn cross_sum(points: &[Point], centre: Point) -> f64 {
    points
        .windows(2)
        .filter_map(|pair| Some((*pair.first()? - centre).cross(*pair.get(1)? - centre)))
        .sum()
}

/// The arc from `start` to `end` about `centre`, going `clockwise`, as
/// sub-arcs of at most [`ARC_FIT_MAX_SWEEP_DEG`]. A run that closes on
/// itself is a full turn, so no sub-arc ends where it starts. The last
/// sub-arc ends at `end` exactly; the ends between are on the circle at
/// `start`'s radius. Ported from `splitArc`.
fn split_arc(start: Point3, end: Point3, centre: Point, clockwise: bool) -> Vec<SubArc> {
    let radius_mm = start.xy().distance_to(centre);
    let max_sweep = ARC_FIT_MAX_SWEEP_DEG.to_radians();
    let a0 = (start.y - centre.y).atan2(start.x - centre.x);
    let a1 = (end.y - centre.y).atan2(end.x - centre.x);
    let mut sweep = a1 - a0;
    while sweep > PI {
        sweep -= TAU;
    }
    while sweep <= -PI {
        sweep += TAU;
    }
    if clockwise && sweep > 0.0 {
        sweep -= TAU;
    } else if !clockwise && sweep < 0.0 {
        sweep += TAU;
    }
    if sweep.abs() < SWEEP_EPS {
        sweep = if clockwise { -TAU } else { TAU };
    }
    // A hair under the boundary is one segment, not two, when the fitted
    // centre is slightly off.
    let count = (sweep.abs() / max_sweep - SWEEP_EPS).ceil().max(1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "at most a full turn over a quarter turn: four or so"
    )]
    let segments = count as usize;
    let step = sweep / count;
    let mut arcs = Vec::with_capacity(segments);
    let mut seg_start = start;
    for s in 1..=segments {
        #[allow(clippy::cast_precision_loss, reason = "s <= segments, a handful")]
        let angle = a0 + step * s as f64;
        let seg_end = if s == segments {
            Point3::new(end.x, end.y, start.z)
        } else {
            Point3::new(
                centre.x + radius_mm * angle.cos(),
                centre.y + radius_mm * angle.sin(),
                start.z,
            )
        };
        arcs.push(SubArc {
            start: seg_start,
            end: seg_end,
            centre,
            clockwise,
        });
        seg_start = seg_end;
    }
    arcs
}

/// How the emitter writes numbers, for judging an arc as the controller
/// reads it.
pub struct EmitOptions<'a> {
    /// Formats a value exactly as the post-processor writes it, read back
    /// as a number; `None` if it cannot be written.
    pub format: &'a dyn Fn(f64) -> Option<f64>,
    /// The post-processor's arc format.
    pub arc_format: ArcFormat,
    /// 1 for millimetre output, 25.4 for inch. Controllers check in
    /// millimetres, so inch output faces the same budget, on a coarser grid.
    pub mm_per_unit: f64,
    /// One unit in the last written place, output units. Enables I/J
    /// snapping; 0 disables it.
    pub quantum: f64,
}

/// The words an arc emits, and whether the controller accepts them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmittedArc {
    /// I, relative to the previously emitted position, output units.
    pub i: f64,
    /// J, likewise.
    pub j: f64,
    /// R, output units.
    pub radius: f64,
    /// Whether the controller would accept the formatted block.
    pub accepted: bool,
    /// The radius disagreement the controller computes, mm.
    pub delta_r_mm: f64,
}

/// The radius disagreement a controller computes from a formatted I/J
/// block: the end's distance from the centre against the start's.
fn radius_disagreement(start: Point, target: Point, i: f64, j: f64) -> f64 {
    ((target.x - (start.x + i)).hypot(target.y - (start.y + j)) - i.hypot(j)).abs()
}

/// Resolves the words of an arc to `end` about `centre` (machine
/// coordinates, output units) written from `previous_emitted`, the
/// formatted end of the preceding block, and decides whether the
/// controller will accept them, by reproducing its arithmetic on the
/// formatted values. The emitter writes exactly these words.
///
/// With I/J, the offsets are snapped within [`SNAP_SEARCH_STEPS`] quanta to
/// the pair that agrees best; accepted within
/// [`ARC_RADIUS_CONSISTENCY_MM`], rejected over [`ARC_RADIUS_GROSS_MM`],
/// else within [`ARC_RADIUS_RELATIVE`] of the radius. With R, a chord/2
/// rounding shortfall of up to the same quanta is repaired by rounding R
/// up; a larger gap is a different arc and is rejected. An arc whose
/// formatted end is its start is rejected: the controller cuts a full
/// circle.
#[must_use]
pub fn resolve_emitted_arc(
    end: Point,
    centre: Point,
    previous_emitted: Point,
    opts: &EmitOptions<'_>,
) -> EmittedArc {
    let format = opts.format;
    let raw_i = centre.x - previous_emitted.x;
    let raw_j = centre.y - previous_emitted.y;
    let rejected = EmittedArc {
        i: raw_i,
        j: raw_j,
        radius: raw_i.hypot(raw_j),
        accepted: false,
        delta_r_mm: f64::INFINITY,
    };
    let (Some(sx), Some(sy), Some(tx), Some(ty)) = (
        format(previous_emitted.x),
        format(previous_emitted.y),
        format(end.x),
        format(end.y),
    ) else {
        return rejected;
    };
    let start = Point::new(sx, sy);
    let target = Point::new(tx, ty);
    let same = if opts.quantum > 0.0 {
        opts.quantum / 2.0
    } else {
        EPS_MM
    };
    if (tx - sx).abs() < same && (ty - sy).abs() < same {
        return rejected;
    }

    if opts.arc_format == ArcFormat::R {
        let chord = target.distance_to(start);
        let Some(rounded) = format(raw_i.hypot(raw_j)) else {
            return rejected;
        };
        let spanning = if opts.quantum > 0.0 {
            (chord / 2.0 / opts.quantum).ceil() * opts.quantum
        } else {
            chord / 2.0
        };
        let repairable =
            opts.quantum > 0.0 && spanning - rounded <= f64::from(SNAP_SEARCH_STEPS) * opts.quantum;
        let radius = if repairable {
            rounded.max(spanning)
        } else {
            rounded
        };
        let Some(written) = format(radius) else {
            return rejected;
        };
        let overshoot_mm = (chord - 2.0 * written) * opts.mm_per_unit;
        return EmittedArc {
            i: raw_i,
            j: raw_j,
            radius,
            accepted: overshoot_mm <= 0.0,
            delta_r_mm: overshoot_mm.max(0.0),
        };
    }

    let Some((i, j)) = snap_offsets(start, target, raw_i, raw_j, opts) else {
        return rejected;
    };
    let start_r = i.hypot(j);
    let delta_r_mm = radius_disagreement(start, target, i, j) * opts.mm_per_unit;
    let accepted = if delta_r_mm <= ARC_RADIUS_CONSISTENCY_MM {
        true
    } else if delta_r_mm > ARC_RADIUS_GROSS_MM {
        false
    } else {
        delta_r_mm <= ARC_RADIUS_RELATIVE * start_r * opts.mm_per_unit
    };
    EmittedArc {
        i,
        j,
        radius: raw_i.hypot(raw_j),
        accepted,
        delta_r_mm,
    }
}

/// The I and J to write: the pair on the output grid within
/// [`SNAP_SEARCH_STEPS`] of the rounded offsets whose start and end radii
/// agree best; ties go to the smaller displacement. `None` if the offsets
/// cannot be written.
fn snap_offsets(
    start: Point,
    target: Point,
    raw_i: f64,
    raw_j: f64,
    opts: &EmitOptions<'_>,
) -> Option<(f64, f64)> {
    let format = opts.format;
    let (base_i, base_j) = (format(raw_i)?, format(raw_j)?);
    if opts.quantum <= 0.0 {
        return Some((base_i, base_j));
    }
    let mut best = (base_i, base_j);
    let mut best_delta = radius_disagreement(start, target, base_i, base_j);
    let mut best_steps = 0;
    for di in -SNAP_SEARCH_STEPS..=SNAP_SEARCH_STEPS {
        for dj in -SNAP_SEARCH_STEPS..=SNAP_SEARCH_STEPS {
            if di == 0 && dj == 0 {
                continue;
            }
            // Re-format so the candidate is exactly on the grid.
            let (Some(i), Some(j)) = (
                format(base_i + f64::from(di) * opts.quantum),
                format(base_j + f64::from(dj) * opts.quantum),
            ) else {
                continue;
            };
            let delta = radius_disagreement(start, target, i, j);
            let steps = di.abs() + dj.abs();
            if delta < best_delta - SWEEP_EPS
                || (delta <= best_delta + SWEEP_EPS && steps < best_steps)
            {
                best = (i, j);
                best_delta = delta;
                best_steps = steps;
            }
        }
    }
    Some(best)
}

/// Whether every sub-arc of a run survives formatting, walking the
/// formatted position from `start` (machine coordinates) through each
/// sub-arc's end. `to_machine` maps project XY to machine XY.
fn run_survives_formatting(
    arcs: &[SubArc],
    start: Point,
    opts: &EmitOptions<'_>,
    to_machine: &dyn Fn(Point) -> Point,
) -> bool {
    let mut position = start;
    for arc in arcs {
        let end = to_machine(arc.end.xy());
        let centre = to_machine(arc.centre);
        if !resolve_emitted_arc(end, centre, position, opts).accepted {
            return false;
        }
        let (Some(x), Some(y)) = ((opts.format)(end.x), (opts.format)(end.y)) else {
            return false;
        };
        position = Point::new(x, y);
    }
    true
}

/// Resolves `fitted` into what can be emitted: every run the controller
/// would reject once formatted, or that has no known emitted start, is
/// replaced by its original moves. `start` is the formatted machine XY
/// the controller is at when the sequence begins, if known; `to_machine`
/// maps project XY to machine XY. Returns the sequence and how many runs
/// fell back. Ported from `applyEmittedArcFallback`.
#[must_use]
pub fn apply_fallback(
    fitted: Vec<Fitted>,
    opts: &EmitOptions<'_>,
    start: Option<Point>,
    to_machine: &dyn Fn(Point) -> Point,
) -> (Vec<Fitted>, usize) {
    let formatted = |p: Point| -> Option<Point> {
        let m = to_machine(p);
        Some(Point::new((opts.format)(m.x)?, (opts.format)(m.y)?))
    };
    let mut out = Vec::with_capacity(fitted.len());
    let mut fallback_runs = 0;
    let mut position = start;
    for item in fitted {
        match item {
            Fitted::Move(mv) => {
                position = formatted(mv.to.xy());
                out.push(Fitted::Move(mv));
            }
            Fitted::Arc(run) => {
                let survives = position
                    .is_some_and(|p| run_survives_formatting(&run.arcs, p, opts, to_machine));
                if survives {
                    position = run.arcs.last().and_then(|a| formatted(a.end.xy()));
                    out.push(Fitted::Arc(run));
                } else {
                    fallback_runs += 1;
                    position = run.fallback.last().and_then(|m| formatted(m.to.xy()));
                    out.extend(run.fallback.into_iter().map(Fitted::Move));
                }
            }
        }
    }
    (out, fallback_runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

    use dc_cam::{Move, MoveKind, generate_profile};
    use dc_geometry::{Point, Point3, Polyline, Transform, Vertex, assert_approx_eq};
    use dc_project::test_support::{profile_mut, sample_project};
    use dc_project::{
        ArcFormat, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
        PostProcessorRef, Project, Stock, TabPlacement, TabShape, TabSpec,
    };

    use crate::emit::{ExportWarning, emit};
    use crate::parse::{MotionKind, parse_motion};

    const TOL: f64 = 0.01;
    const FEED: f64 = 1000.0;

    fn pt(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    /// `n` chords of the circle of radius `r` about the origin from angle
    /// `from_rad` to `to_rad`, at `z`, after a rapid to the first point.
    fn chords(r: f64, n: usize, from_rad: f64, to_rad: f64, z: f64) -> Vec<Move> {
        let mut moves = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let angle = from_rad + (to_rad - from_rad) * k as f64 / n as f64;
            let p = pt(r * angle.cos(), r * angle.sin(), z);
            if k == 0 {
                moves.push(Move::rapid(p));
            } else {
                moves.push(Move::linear(p, FEED));
            }
        }
        moves
    }

    fn arcs_of(fitted: &[Fitted]) -> Vec<&SubArc> {
        fitted
            .iter()
            .filter_map(|f| match f {
                Fitted::Arc(run) => Some(run.arcs.iter()),
                Fitted::Move(_) => None,
            })
            .flatten()
            .collect()
    }

    fn moves_of(fitted: &[Fitted]) -> Vec<&Move> {
        fitted
            .iter()
            .filter_map(|f| match f {
                Fitted::Move(m) => Some(m),
                Fitted::Arc(_) => None,
            })
            .collect()
    }

    fn lines_of(fitted: &[Fitted]) -> usize {
        moves_of(fitted)
            .iter()
            .filter(|m| m.kind == MoveKind::Linear)
            .count()
    }

    fn runs_of(fitted: &[Fitted]) -> usize {
        fitted
            .iter()
            .filter(|f| matches!(f, Fitted::Arc(_)))
            .count()
    }

    fn sweep_deg(arc: &SubArc) -> f64 {
        let a0 = (arc.start.y - arc.centre.y).atan2(arc.start.x - arc.centre.x);
        let a1 = (arc.end.y - arc.centre.y).atan2(arc.end.x - arc.centre.x);
        let mut sweep = (a1 - a0).abs();
        if sweep > PI {
            sweep = TAU - sweep;
        }
        sweep.to_degrees()
    }

    #[test]
    fn accepted_circular_run_gives_four_arcs() {
        let moves = chords(10.0, 72, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4, "{fitted:?}");
        assert!(arcs.iter().all(|a| !a.clockwise), "counter-clockwise");
        let last = arcs.last().unwrap();
        assert_approx_eq!(last.end.x, 10.0, 1e-9);
        assert_approx_eq!(last.end.y, 0.0, 1e-9);
        // The run replaces every chord; the rapid passes through.
        assert_eq!(lines_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 1);
    }

    /// A run that goes out along a circle and back (a Profile's ramp
    /// returning to its entry point before the lap) lies on the circle
    /// but is not one arc: every fitted run turns one way, so the moves
    /// it replaces are the moves it follows.
    #[test]
    fn run_that_turns_back_is_not_one_arc() {
        let mut moves = chords(10.0, 18, 0.0, FRAC_PI_2, 0.0);
        moves.extend(chords(10.0, 18, FRAC_PI_2, 0.0, 0.0).into_iter().skip(1));
        moves.extend(chords(10.0, 36, 0.0, PI, 0.0).into_iter().skip(1));
        let fitted = fit(&moves, TOL);
        assert!(runs_of(&fitted) > 0, "the one-way stretches still fit");
        for f in &fitted {
            let Fitted::Arc(run) = f else { continue };
            let mut from = run.arcs[0].start;
            let mut sense = 0.0_f64;
            for mv in &run.fallback {
                let o = Point::new(0.0, 0.0);
                let turn = (from.xy() - o).cross(mv.to.xy() - o);
                assert!(sense * turn >= 0.0, "a run turns back: {run:?}");
                sense = turn;
                from = mv.to;
            }
        }
    }

    #[test]
    fn clockwise_is_g2() {
        let moves = chords(10.0, 72, 0.0, -TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert!(!arcs.is_empty());
        assert!(arcs.iter().all(|a| a.clockwise), "clockwise in Y-up is G2");
    }

    #[test]
    fn rejects_straight_run() {
        let moves = vec![
            Move::rapid(pt(0.0, 0.0, 0.0)),
            Move::linear(pt(1.0, 0.0, 0.0), FEED),
            Move::linear(pt(2.0, 0.0, 0.0), FEED),
            Move::linear(pt(3.0, 0.0, 0.0), FEED),
            Move::linear(pt(4.0, 0.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(lines_of(&fitted), 4);
        assert_eq!(moves_of(&fitted).len(), 5);
    }

    /// #213: the plaque's stay-down link into its wall ring, 0.006 mm, the
    /// ring's shallow corner arc (1.825 mm radius, 2.7°) and its 40 mm
    /// side all lie on a 154.75 mm circle, which bows 1.3 mm off the side
    /// into the wall. One full chord step is not a run.
    #[test]
    fn long_side_after_a_fragment_is_not_an_arc() {
        let moves = vec![
            Move::rapid(pt(131.817, 56.915, 0.0)),
            Move::linear(pt(131.823, 56.915, 0.0), FEED),
            Move {
                kind: MoveKind::ArcCcw {
                    centre: Point::new(130.0, 57.0),
                },
                to: pt(131.825, 57.0, 0.0),
                feed_mm_min: Some(FEED),
            },
            Move::linear(pt(131.825, 97.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
    }

    /// A chord between two points on the circle is bowed off by the arc's
    /// sagitta, `r(1 − cos(sweep/2))`, either way round; ends off the
    /// circle add how far they are off.
    #[test]
    fn arc_chord_deviation_is_the_sagitta() {
        let o = Point::new(0.0, 0.0);
        let (r, sweep) = (10.0, FRAC_PI_2);
        let (a, b) = (Point::new(r, 0.0), Point::new(0.0, r));
        let sagitta = r * (1.0 - (sweep / 2.0).cos());
        assert_approx_eq!(
            arc_chord_deviation_mm(o, r, 0.0, sweep, a, b),
            sagitta,
            1e-9
        );
        assert_approx_eq!(
            arc_chord_deviation_mm(o, r, FRAC_PI_2, -sweep, b, a),
            sagitta,
            1e-9
        );
        let inside = Point::new(r - 0.5, 0.0);
        assert!(arc_chord_deviation_mm(o, r, 0.0, sweep, inside, b) >= 0.5 - 1e-9);
        // A 40 mm side under a 154.75 mm arc: 1.3 mm.
        let r = 154.75;
        let half = (20.0_f64 / r).asin();
        let (a, b) = (
            Point::new(r * half.cos(), -20.0),
            Point::new(r * half.cos(), 20.0),
        );
        let bow = arc_chord_deviation_mm(o, r, -half, 2.0 * half, a, b);
        assert_approx_eq!(bow, r * (1.0 - half.cos()), 1e-9);
        assert!(bow > 1.29, "{bow}");
    }

    /// #229: a 7 mm square's four corners lie on one circle, with even 90°
    /// steps, so every vertex gate passes; but each side is a chord that
    /// circle bows 1.45 mm off. A run's every straight move must lie within
    /// the tolerance of its arc, not only its ends.
    #[test]
    fn square_is_not_fitted_as_a_circle() {
        let moves = vec![
            Move::rapid(pt(30.0, 20.0, 0.0)),
            Move::linear(pt(37.0, 20.0, 0.0), FEED),
            Move::linear(pt(37.0, 27.0, 0.0), FEED),
            Move::linear(pt(30.0, 27.0, 0.0), FEED),
            Move::linear(pt(30.0, 20.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
        assert_eq!(lines_of(&fitted), 4);
    }

    /// #233: an 8.45 mm straight move, a 0.03 mm corner arc and a
    /// 0.002 mm straight move (the moves of the Pocket below) all lie on
    /// one 33 mm circle, and the step gate leaves the corner arc out of
    /// its median. The long chord's own deviation, 0.27 mm under that
    /// circle, refuses the run, as does #213's gate of two full chord
    /// steps; the Pocket below needs the deviation gate for its other runs.
    #[test]
    fn long_chord_with_tiny_corner_is_not_fitted() {
        let z = 5.0;
        let moves = vec![
            Move::rapid(pt(111.538_237_913_749_14, 79.670_914_550_191_23, z)),
            Move::linear(pt(114.736_761_571_381_44, 71.845_191_023_101_12, z), FEED),
            Move {
                kind: MoveKind::ArcCcw {
                    centre: Point::new(118.204_085_241_192_94, 73.262_352_867_390_8),
                },
                to: pt(114.748_638_699_674_87, 71.816_472_401_080_93, z),
                feed_mm_min: Some(FEED),
            },
            Move::linear(pt(114.750_569_127_027_2, 71.817_280_159_837_1, z), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0, "{fitted:?}");
        assert_eq!(moves_of(&fitted).len(), 4);
    }

    /// #233's minimal case, from the Pocket property test: a raster Pocket
    /// round a rectangular island, rotated, exports in mm and in inch with
    /// arc fitting on. Before, a fitted arc bowed 0.27 mm off a straight
    /// move, and the safety pass refused it (S9 in mm, S12 in inch).
    #[test]
    fn rotated_raster_pocket_with_island_exports_with_arc_fitting() {
        let mut project = sample_project();
        project.stock = Stock {
            width_mm: 200.0,
            height_mm: 200.0,
            thickness_mm: Some(6.0),
        };
        project.arc_fit_tolerance_mm = Some(0.01);
        let (hx, hy) = (5.557_357_878_931_814, 4.211_528_476_684_152);
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Pocket".into(),
            profiles: vec![
                Polyline::rect(
                    Point::new(-16.293_109_341_906_767, -17.302_636_203_267_785),
                    2.0 * 16.293_109_341_906_767,
                    2.0 * 17.302_636_203_267_785,
                ),
                Polyline::rect(Point::new(-hx, -hy), 2.0 * hx, 2.0 * hy),
            ],
        };
        project.definitions = std::collections::BTreeMap::from([(def.id.clone(), def)]);
        project.instances = vec![FeatureInstance {
            transform: Transform::rotate(
                0.925_667_480_965_779_2_f64.atan2(-0.378_338_095_732_710_93),
            )
            .then(Transform::translate(120.0, 80.0)),
            definition: DefinitionId::new("d1"),
            ..project.instances[0].clone()
        }];
        let tool = &mut project.tools[0];
        tool.diameter_mm = 7.491_510_161_281_357;
        tool.feed_mm_min = Some(3982.583);
        tool.plunge_mm_min = Some(596.066);
        let pocket = dc_project::PocketOp {
            targets: vec![project.instances[0].id.clone()],
            tool: project.tools[0].id.clone(),
            strategy: dc_project::ClearanceStrategy::Raster {
                angle_deg: 51.035_950_808_994_54,
            },
            step_over_fraction: Some(0.992_635_621_218_441_3),
            depth_mm: Some(1.0),
            ..dc_project::test_support::sample_pocket()
        };
        project.operations = vec![Operation::Pocket(pocket.clone())];
        for units in [dc_project::Units::Mm, dc_project::Units::Inch] {
            project.display_units = units;
            let (toolpath, _) = dc_cam::generate_pocket(&project, &pocket).unwrap();
            emit(
                &project,
                &[(
                    Operation::Pocket(pocket.clone()),
                    project.tools[0].clone(),
                    toolpath,
                )
                    .into()],
                "2026-10-21",
            )
            .unwrap_or_else(|e| panic!("{units:?}: {e:?}"));
        }
    }

    /// #229: a regular octagon of 10 mm radius, its chords bowing
    /// 0.76 mm from the circle through its corners, stays eight lines; the
    /// same circle as 72 chords (0.0095 mm each) is fitted.
    #[test]
    fn regular_octagon_of_long_chords_is_not_fitted() {
        assert_eq!(runs_of(&fit(&chords(10.0, 8, 0.0, TAU, 0.0), TOL)), 0);
        assert_eq!(runs_of(&fit(&chords(10.0, 72, 0.0, TAU, 0.0), TOL)), 1);
    }

    #[test]
    fn rejects_z_changing_run() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(pt(7.07, 7.07, -0.5), FEED),
            Move::linear(pt(0.0, 10.0, -1.0), FEED),
            Move::linear(pt(-7.07, 7.07, -1.5), FEED),
            Move::linear(pt(-10.0, 0.0, -2.0), FEED),
        ];
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    #[test]
    fn rejects_mixed_feed_run() {
        let mut moves = chords(10.0, 72, 0.0, TAU, 0.0);
        for (k, m) in moves.iter_mut().skip(1).enumerate() {
            m.feed_mm_min = Some(if k % 2 == 0 { 500.0 } else { FEED });
        }
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    /// A toolpath has no gaps (each move starts where the last ended), so
    /// a discontinuity is a move the run cannot include: here a vertical
    /// plunge between two chords of one circle.
    #[test]
    fn rejects_discontinuous_run() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(
                pt(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 0.0),
                FEED,
            ),
            Move::linear(pt(0.0, 10.0, 0.0), FEED),
            Move::linear(pt(0.0, 10.0, -1.0), 300.0),
            Move::linear(
                pt(-10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), -1.0),
                FEED,
            ),
            Move::linear(pt(-10.0, 0.0, -1.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 6);
    }

    #[test]
    fn splits_around_rapids() {
        let mut moves = chords(10.0, 18, 0.0, FRAC_PI_2, 0.0);
        moves.extend(chords(10.0, 18, PI, PI + FRAC_PI_2, 0.0));
        let fitted = fit(&moves, TOL);
        assert_eq!(arcs_of(&fitted).len(), 2, "{fitted:?}");
        let rest = moves_of(&fitted);
        assert_eq!(rest.len(), 2);
        assert!(rest.iter().all(|m| m.kind == MoveKind::Rapid));
    }

    #[test]
    fn short_run_stays_linear() {
        let moves = vec![
            Move::rapid(pt(10.0, 0.0, 0.0)),
            Move::linear(pt(7.0, 7.0, 0.0), FEED),
            Move::linear(pt(0.0, 10.0, 0.0), FEED),
        ];
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(lines_of(&fitted), 2);
    }

    #[test]
    fn rejects_ambiguous_direction() {
        let moves = vec![
            Move::rapid(pt(0.0, 0.0, 0.0)),
            Move::linear(pt(1.0, 1.0, 0.0), FEED),
            Move::linear(pt(2.0, 0.0, 0.0), FEED),
            Move::linear(pt(3.0, 1.0, 0.0), FEED),
            Move::linear(pt(4.0, 0.0, 0.0), FEED),
        ];
        assert_eq!(runs_of(&fit(&moves, TOL)), 0);
    }

    #[test]
    fn rejects_high_residual_ellipse() {
        let (a, b, n) = (10.0, 5.0, 8);
        let mut moves = Vec::new();
        for k in 0..=n {
            let angle = TAU * f64::from(k) / f64::from(n);
            let p = pt(a * angle.cos(), b * angle.sin(), 0.0);
            moves.push(if k == 0 {
                Move::rapid(p)
            } else {
                Move::linear(p, FEED)
            });
        }
        assert_eq!(runs_of(&fit(&moves, 0.01)), 0);
    }

    #[test]
    fn splits_large_arc_at_90_degrees() {
        let moves = chords(10.0, 64, 0.0, 3.0 * FRAC_PI_2, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 3, "{fitted:?}");
        for arc in arcs {
            assert!(sweep_deg(arc) <= 90.1, "{}", sweep_deg(arc));
        }
    }

    #[test]
    fn split_arc_offsets_relative() {
        let moves = chords(10.0, 72, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert!(arcs.len() >= 2);
        let c0 = arcs[0].centre;
        let mut distinct = 0;
        let (i0, j0) = (c0.x - arcs[0].start.x, c0.y - arcs[0].start.y);
        for arc in &arcs {
            assert_approx_eq!(arc.centre.x, c0.x, 1e-6);
            assert_approx_eq!(arc.centre.y, c0.y, 1e-6);
            let (i, j) = (arc.centre.x - arc.start.x, arc.centre.y - arc.start.y);
            if (i - i0).abs() > 1e-6 || (j - j0).abs() > 1e-6 {
                distinct += 1;
            }
        }
        assert!(distinct > 0, "I and J are per sub-arc");
    }

    #[test]
    fn rejects_near_collinear_run() {
        let (huge_r, n) = (100_000.0, 4);
        let shallow = PI / 720.0;
        let points: Vec<Point> = (0..=n)
            .map(|k| {
                let angle = shallow * (f64::from(k) / f64::from(n) - 0.5);
                Point::new(huge_r * angle.cos(), huge_r * angle.sin())
            })
            .collect();
        let (cx, cy) = (points[0].x, (points[0].y + points[n as usize].y) / 2.0);
        let mut moves = Vec::new();
        for (k, p) in points.iter().enumerate() {
            let p = pt(p.x - cx, p.y - cy, 0.0);
            moves.push(if k == 0 {
                Move::rapid(p)
            } else {
                Move::linear(p, FEED)
            });
        }
        let fitted = fit(&moves, TOL);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 5);
    }

    #[test]
    fn rejects_long_straight_with_corner_fragment() {
        let moves = vec![
            Move::rapid(pt(2.625, 1.77, 0.0)),
            Move::linear(pt(1.375, 1.77, 0.0), FEED),
            Move::linear(
                pt(1.362_362_417_301_589_6, 1.769_448_231_223_303, 0.0),
                FEED,
            ),
            Move::linear(pt(1.349_821_014_238_295_1, 1.767_797_124_186_77, 0.0), FEED),
        ];
        let fitted = fit(&moves, 0.01 / 25.4);
        assert_eq!(runs_of(&fitted), 0);
        assert_eq!(moves_of(&fitted).len(), 4);
    }

    #[test]
    fn accepts_ordinary_arc_after_collinear_gate() {
        let moves = chords(5.0, 6, 0.0, PI / 6.0, 0.0);
        assert_eq!(arcs_of(&fit(&moves, TOL)).len(), 1);
    }

    #[test]
    fn full_circle_loop_is_four_arcs() {
        let moves = chords(10.0, 72, 0.0, TAU, 0.0);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4);
        assert_eq!(lines_of(&fitted), 0);
        for arc in arcs {
            assert!(
                arc.start.xy().distance_to(arc.end.xy()) > 1.0,
                "no sub-arc ends where it starts: {arc:?}"
            );
        }
    }

    #[test]
    fn partial_arc_with_straight_leads() {
        let r = 10.0;
        let mut moves = vec![
            Move::rapid(pt(-15.0, 0.0, 0.0)),
            Move::linear(pt(-10.0, 0.0, 0.0), FEED),
            Move::linear(pt(-5.0, 0.0, 0.0), FEED),
            Move::linear(pt(r, 0.0, 0.0), FEED),
        ];
        moves.extend(chords(r, 36, 0.0, PI, 0.0).into_iter().skip(1));
        moves.extend([
            Move::linear(pt(-13.0, 0.0, 0.0), FEED),
            Move::linear(pt(-16.0, 0.0, 0.0), FEED),
            Move::linear(pt(-19.0, 0.0, 0.0), FEED),
        ]);
        let fitted = fit(&moves, TOL);
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 2, "{fitted:?}");
        assert_eq!(lines_of(&fitted), 6);
        assert_approx_eq!(arcs[0].centre.x, 0.0, 1e-6);
        assert_approx_eq!(arcs[0].centre.y, 0.0, 1e-6);
        assert_approx_eq!(arcs[0].start.x, r, 1e-9);
        assert_approx_eq!(arcs[0].start.y, 0.0, 1e-9);
        assert_approx_eq!(arcs[1].end.x, -r, 1e-9);
        assert_approx_eq!(arcs[1].end.y, 0.0, 1e-9);
    }

    /// The 14 contiguous cut moves from PureCutCNC issue #447, after a
    /// rapid to the first point.
    fn issue_447_moves() -> Vec<Move> {
        let xy = [
            (122.394_176_793_950_8, 167.172_783_309_121_77),
            (122.375_566_801_907_44, 167.103_329_873_287_52),
            (122.369_300_000_024_96, 167.0317),
            (122.366_778_981_394_65, 166.960_070_126_712_48),
            (122.376_601_152_925_22, 166.890_616_690_878_23),
            (122.398_201_059_425_5, 166.825_450_000_000_05),
            (122.430_655_385_187_21, 166.766_550_111_004_34),
            (122.472_711_008_465_3, 166.715_706_667_213_45),
            (122.522_823_076_948_27, 166.674_464_520_938_95),
            (122.579_201_947_313_66, 166.644_076_793_925_7),
            (122.639_867_562_635_06, 166.625_466_801_882_57),
            (122.702_709_615_409_9, 166.619_200_000_000_1),
            (122.765_551_668_184_74, 166.625_466_801_882_57),
            (122.826_217_283_506_14, 166.644_076_793_925_7),
            (122.882_596_153_871_53, 166.674_464_520_938_95),
        ];
        xy.iter()
            .enumerate()
            .map(|(k, &(x, y))| {
                let p = pt(x, y, -10.5);
                if k == 0 {
                    Move::rapid(p)
                } else {
                    Move::linear(p, FEED)
                }
            })
            .collect()
    }

    #[test]
    fn adjacent_runs_share_endpoints_exactly() {
        let fitted = fit(&issue_447_moves(), TOL);
        assert!(runs_of(&fitted) >= 2, "{fitted:?}");
        let mut previous: Option<Point3> = None;
        for item in &fitted {
            match item {
                Fitted::Arc(run) => {
                    for arc in &run.arcs {
                        if let Some(p) = previous {
                            assert!(
                                p.x.to_bits() == arc.start.x.to_bits()
                                    && p.y.to_bits() == arc.start.y.to_bits(),
                                "arc starts exactly where the last item ended"
                            );
                        }
                        previous = Some(arc.end);
                    }
                }
                Fitted::Move(m) => previous = Some(m.to),
            }
        }
    }

    #[test]
    fn fitted_arcs_pass_through_source_endpoints() {
        let moves = issue_447_moves();
        let fitted = fit(&moves, TOL);
        let end = match fitted.last().unwrap() {
            Fitted::Arc(run) => run.arcs.last().unwrap().end,
            Fitted::Move(m) => m.to,
        };
        let target = moves.last().unwrap().to;
        assert!(end.xy().distance_to(target.xy()) < 1e-9);
    }

    /// Three-decimal millimetre formatting, as GRBL-family definitions use.
    fn mm3(value: f64) -> Option<f64> {
        Some((value * 1000.0).round() / 1000.0)
    }

    fn inch4(value: f64) -> Option<f64> {
        Some((value * 10_000.0).round() / 10_000.0)
    }

    fn mm3_opts(arc_format: ArcFormat) -> EmitOptions<'static> {
        EmitOptions {
            format: &mm3,
            arc_format,
            mm_per_unit: 1.0,
            quantum: 1e-3,
        }
    }

    fn inch4_opts() -> EmitOptions<'static> {
        EmitOptions {
            format: &inch4,
            arc_format: ArcFormat::Ij,
            mm_per_unit: 25.4,
            quantum: 1e-4,
        }
    }

    fn identity(p: Point) -> Point {
        p
    }

    /// The controller's own radius check on a formatted block.
    fn controller_delta(start: Point, target: Point, i: f64, j: f64) -> f64 {
        ((target.x - (start.x + i)).hypot(target.y - (start.y + j)) - i.hypot(j)).abs()
    }

    /// The worst radius disagreement any emitted arc presents, walking the
    /// formatted position from item to item as the emitter does.
    fn worst_emitted_delta(fitted: &[Fitted], opts: &EmitOptions<'_>) -> f64 {
        let f = |v: f64| (opts.format)(v).unwrap();
        let mut position: Option<Point> = None;
        let mut worst: f64 = 0.0;
        for item in fitted {
            match item {
                Fitted::Move(m) => position = Some(Point::new(f(m.to.x), f(m.to.y))),
                Fitted::Arc(run) => {
                    for arc in &run.arcs {
                        let start = position.unwrap_or(Point::new(f(arc.start.x), f(arc.start.y)));
                        let target = Point::new(f(arc.end.x), f(arc.end.y));
                        let emitted = resolve_emitted_arc(arc.end.xy(), arc.centre, start, opts);
                        worst =
                            worst.max(controller_delta(start, target, f(emitted.i), f(emitted.j)));
                        position = Some(target);
                    }
                }
            }
        }
        worst
    }

    #[test]
    fn ij_derived_from_previous_emitted_point() {
        // The exporter's un-rounded idea of the start is 10.0004; the
        // controller is at the formatted 10.000.
        let emitted = resolve_emitted_arc(
            Point::new(0.0, 10.0),
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            &mm3_opts(ArcFormat::Ij),
        );
        assert_approx_eq!(emitted.i, -10.0, 1e-9);
        assert!(emitted.accepted);
    }

    /// A run the controller must reject: the end sits 0.02 mm off the
    /// circle its own I/J declares. Built by hand, so the test pins the
    /// fallback and not the fitter's choice of run boundaries.
    fn inconsistent_run() -> ArcRun {
        ArcRun {
            arcs: vec![SubArc {
                start: pt(10.0, 0.0, 0.0),
                end: pt(0.0, 10.02, 0.0),
                centre: Point::new(0.0, 0.0),
                clockwise: false,
            }],
            feed_mm_min: FEED,
            fallback: vec![
                Move::linear(pt(7.07, 7.07, 0.0), FEED),
                Move::linear(pt(0.0, 10.02, 0.0), FEED),
            ],
        }
    }

    #[test]
    fn invalid_run_falls_back_to_original_moves() {
        let fitted = vec![
            Fitted::Move(Move::rapid(pt(10.0, 0.0, 0.0))),
            Fitted::Arc(inconsistent_run()),
        ];
        let (out, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::Ij), None, &identity);
        assert_eq!(runs, 1);
        assert_eq!(runs_of(&out), 0);
        assert_eq!(out.len(), 3, "the run's original moves, one for one");
        let Fitted::Move(last) = out.last().unwrap() else {
            panic!("fallback output is moves");
        };
        assert_eq!(last.to, pt(0.0, 10.02, 0.0));
    }

    #[test]
    fn fallback_leaves_valid_runs_alone() {
        let mut moves = chords(10.0, 72, 0.0, TAU, -10.5);
        moves.extend(issue_447_moves());
        let fitted = fit(&moves, TOL);
        let good = arcs_of(&fitted).len();
        assert!(good > 0);
        let opts = mm3_opts(ArcFormat::Ij);
        let (clean, runs) = apply_fallback(fitted.clone(), &opts, None, &identity);
        assert_eq!(runs, 0, "well-formed spans do not fall back");
        assert!(worst_emitted_delta(&clean, &opts) <= ARC_RADIUS_CONSISTENCY_MM);

        let mut mixed = fitted;
        mixed.push(Fitted::Move(Move::rapid(pt(10.0, 0.0, 0.0))));
        mixed.push(Fitted::Arc(inconsistent_run()));
        let (out, runs) = apply_fallback(mixed, &opts, None, &identity);
        assert_eq!(runs, 1, "only the offending run falls back");
        assert_eq!(arcs_of(&out).len(), good);
    }

    #[test]
    fn r_format_rejects_unrepresentable_chord() {
        let bad = resolve_emitted_arc(
            Point::new(10.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 0.0),
            &mm3_opts(ArcFormat::R),
        );
        assert!(!bad.accepted, "a chord longer than the diameter");
        let fitted = fit(&issue_447_moves(), TOL);
        let (_, runs) = apply_fallback(fitted, &mm3_opts(ArcFormat::R), None, &identity);
        assert_eq!(runs, 0, "the #447 span is representable in R format");
    }

    #[test]
    fn inch_output_uses_millimetre_budget() {
        // 0.004 in of radius disagreement is 0.1016 mm: legal if the budget
        // were applied in inches, far past GRBL's 0.005 mm.
        let as_inch = resolve_emitted_arc(
            Point::new(0.0, 1.004),
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            &inch4_opts(),
        );
        assert!(!as_inch.accepted, "{}", as_inch.delta_r_mm);
    }

    #[test]
    fn snapping_never_worsens_agreement() {
        for (label, opts, scale) in [
            ("mm @3dp", mm3_opts(ArcFormat::Ij), 1.0),
            ("inch @4dp", inch4_opts(), 1.0 / 25.4),
        ] {
            let f = |v: f64| (opts.format)(v).unwrap();
            let moves: Vec<Move> = issue_447_moves()
                .into_iter()
                .map(|m| Move {
                    to: pt(m.to.x * scale, m.to.y * scale, m.to.z * scale),
                    ..m
                })
                .collect();
            let fitted = fit(&moves, TOL * scale);
            let mut position: Option<Point> = None;
            let mut improved = false;
            for item in &fitted {
                match item {
                    Fitted::Move(m) => position = Some(Point::new(f(m.to.x), f(m.to.y))),
                    Fitted::Arc(run) => {
                        for arc in &run.arcs {
                            let start =
                                position.unwrap_or(Point::new(f(arc.start.x), f(arc.start.y)));
                            let resolved =
                                resolve_emitted_arc(arc.end.xy(), arc.centre, start, &opts);
                            let naive_i = f(arc.centre.x - start.x);
                            let naive_j = f(arc.centre.y - start.y);
                            let target = Point::new(f(arc.end.x), f(arc.end.y));
                            let naive = controller_delta(start, target, naive_i, naive_j)
                                * opts.mm_per_unit;
                            assert!(
                                resolved.delta_r_mm <= naive + 1e-12,
                                "{label}: snapped {} vs naive {naive}",
                                resolved.delta_r_mm
                            );
                            if resolved.delta_r_mm < naive - 1e-12 {
                                improved = true;
                            }
                            position = Some(target);
                        }
                    }
                }
            }
            assert!(improved, "{label}: snapping improves at least one arc");
        }
    }

    #[test]
    fn r_format_repairs_rounding_shortfall_only() {
        let opts = mm3_opts(ArcFormat::R);
        // A chord a hair longer than the rounded diameter: a rounding
        // artifact, repaired by one grid step.
        let repaired = resolve_emitted_arc(
            Point::new(1.0004, 0.0),
            Point::new(0.5002, 0.0),
            Point::new(0.0, 0.0),
            &opts,
        );
        assert!(repaired.accepted);
        assert!(repaired.radius >= 0.5, "{}", repaired.radius);
        assert!(repaired.radius < 0.51, "{}", repaired.radius);
        // Radius 1, chord 10: inconsistent by a factor of five, not by
        // rounding.
        let rejected = resolve_emitted_arc(
            Point::new(10.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 0.0),
            &opts,
        );
        assert!(!rejected.accepted);
    }

    /// The quick start's coaster: a 90 mm circle imported as 160 facets on
    /// 110 × 110 × 11.8 mm stock, cut through by 0.2 mm in 4 mm passes with
    /// four 8 × 3 mm rect tabs, on grblHAL.
    fn coaster_project() -> Project {
        let mut project = sample_project();
        project.post_processor = PostProcessorRef::latest("grblhal").unwrap();
        project.stock = Stock {
            width_mm: 110.0,
            height_mm: 110.0,
            thickness_mm: Some(11.8),
        };
        let facets: Vec<Vertex> = (0..160)
            .map(|k| {
                let angle = TAU * f64::from(k) / 160.0;
                Vertex::line(45.0 * angle.cos(), 45.0 * angle.sin())
            })
            .collect();
        project.definitions.clear();
        project.definitions.insert(
            DefinitionId::new("d1"),
            FeatureDefinition {
                id: DefinitionId::new("d1"),
                name: "Circle".into(),
                profiles: vec![Polyline::new(facets, true)],
            },
        );
        project.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(55.0, 55.0),
            name: "Circle".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(11.8),
            z_bottom_mm: Some(0.0),
        }];
        let tool = &mut project.tools[0];
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.step_down_mm = Some(4.0);
        let op = profile_mut(&mut project);
        op.depth_mm = Some(12.0);
        op.through_cut_allowance_mm = 0.2;
        op.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 8.0,
            height_mm: 3.0,
            shape: TabShape::Rect,
            on_holes: false,
        });
        project
    }

    /// Emits the coaster and returns the program text, its warnings, and
    /// the toolpath.
    fn export_coaster(project: &Project) -> (String, Vec<ExportWarning>, dc_cam::Toolpath) {
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("the coaster has a profile operation");
        };
        let tool = project.tools[0].clone();
        let (toolpath, warnings) = generate_profile(project, &op).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        dc_cam::test_support::assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
        let (program, warnings) = emit(
            project,
            &[(Operation::Profile(op), tool, toolpath.clone()).into()],
            "2026-10-15",
        )
        .unwrap();
        (program.text, warnings, toolpath)
    }

    /// Functional acceptance 37: the coaster exports as at most 8 arcs per
    /// lap, every original vertex within the tolerance of its arc (S6
    /// passes, or `emit` would have failed), with no fallback.
    #[test]
    fn coaster_circle_exports_as_at_most_eight_arcs_per_lap() {
        let project = coaster_project();
        assert_eq!(project.arc_fit_tolerance_mm, Some(0.01));
        let (text, warnings, _) = export_coaster(&project);
        assert!(warnings.is_empty(), "{warnings:?}");
        let moves = parse_motion(&text).unwrap();
        let arcs = moves
            .iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count();
        let lines = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .count();
        // Three laps (12 mm in 4 mm passes).
        assert!(arcs <= 3 * 8, "{arcs} arcs in {text}");
        assert!(arcs >= 3, "each lap is fitted: {text}");
        // The 160 facets and their corner arcs per lap are gone.
        assert!(lines < 3 * 160, "{lines} straight moves in {text}");
    }

    /// The coaster's program moves split into laps: a lap starts with the
    /// first move that cuts deeper than any before it (the plunge to its
    /// pass) and runs to the next such move or the retract. Rapids are
    /// left out.
    fn laps(text: &str) -> Vec<Vec<crate::parse::MachineMove>> {
        let mut laps: Vec<Vec<crate::parse::MachineMove>> = Vec::new();
        let mut deepest = f64::INFINITY;
        for m in parse_motion(text).unwrap() {
            if m.kind == MotionKind::Rapid {
                continue;
            }
            let z = m.to.z.unwrap();
            if z < deepest - 1e-6 {
                deepest = z;
                laps.push(Vec::new());
            }
            laps.last_mut()
                .expect("a lap starts at the first cut")
                .push(m);
        }
        laps
    }

    /// Functional acceptance 37 (#253), per lap: the coaster's three laps
    /// (12 mm in 4 mm passes) are two full untabbed laps of at most 8
    /// `G2`/`G3` each, and the final pass, whose four tabs (3 mm high, so
    /// their tops at Z −8.8 lie above it) split the lap into 5 runs: one
    /// run per segment between tabs, so at least 9 arcs.
    #[test]
    fn coaster_arcs_counted_per_lap() {
        let project = coaster_project();
        let (text, warnings, _) = export_coaster(&project);
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = laps(&text);
        let floors: Vec<f64> = laps.iter().map(|lap| lap[0].to.z.unwrap()).collect();
        assert_eq!(floors.len(), 3, "three laps: {floors:?}\n{text}");
        for (k, z) in [-4.0, -8.0, -12.0].into_iter().enumerate() {
            assert_approx_eq!(floors[k], z, 1e-6);
        }
        let arcs = |lap: &[crate::parse::MachineMove]| {
            lap.iter()
                .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
                .count()
        };
        for (k, lap) in laps.iter().take(2).enumerate() {
            let tab_moves = lap
                .iter()
                .filter(|m| (m.to.z.unwrap() - floors[k]).abs() > 1e-6)
                .count();
            assert_eq!(tab_moves, 0, "lap {k} at Z {} has no tabs", floors[k]);
            let n = arcs(lap);
            assert!(
                (1..=8).contains(&n),
                "lap {k} at Z {} has {n} arcs, want 1..=8",
                floors[k]
            );
        }
        let tabbed = &laps[2];
        let on_tabs = tabbed
            .iter()
            .filter(|m| (m.to.z.unwrap() - (-8.8)).abs() <= 1e-6)
            .count();
        assert!(on_tabs >= 4, "the final lap rises onto 4 tabs: {on_tabs}");
        let n = arcs(tabbed);
        assert!(n >= 9, "the tabbed lap has {n} arcs, want at least 9");
    }

    /// Functional acceptance 37 (#253): "the same number of `G1` moves as
    /// before on a post-processor without arc support". With
    /// `arc_interpolation` off, `arc_fit_tolerance_mm: 0.01` writes as many
    /// `G1` moves as `null`, and neither writes a `G2` or `G3`.
    #[test]
    fn no_arc_support_keeps_the_g1_count() {
        let mut counts = Vec::new();
        for tolerance in [Some(0.01), None] {
            let mut project = coaster_project();
            project.arc_fit_tolerance_mm = tolerance;
            let mut post = project.post_processor.resolve().unwrap();
            post.motion.arc_interpolation = false;
            let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
                panic!("the coaster has a profile operation");
            };
            let tool = project.tools[0].clone();
            let (toolpath, _) = generate_profile(&project, &op).unwrap();
            let (program, _) = crate::emit::emit_with(
                &project,
                &post,
                &[(Operation::Profile(op), tool, toolpath).into()],
                "2026-10-15",
            )
            .unwrap();
            let moves = parse_motion(&program.text).unwrap();
            let arcs = moves
                .iter()
                .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
                .count();
            assert_eq!(arcs, 0, "{tolerance:?}: no G2/G3\n{}", program.text);
            let lines = moves
                .iter()
                .filter(|m| m.kind == MotionKind::Linear)
                .count();
            counts.push(lines);
        }
        assert_eq!(
            counts[0], counts[1],
            "G1 moves with tolerance 0.01 vs null: {counts:?}"
        );
        assert!(counts[0] >= 3 * 160, "every facet is a G1: {counts:?}");
    }

    /// Functional acceptance 37: `arc_fit_tolerance_mm: null` gives the
    /// phase 1 program, move for move.
    #[test]
    fn null_tolerance_reproduces_phase_1_program() {
        let mut project = coaster_project();
        project.arc_fit_tolerance_mm = None;
        let (text, warnings, toolpath) = export_coaster(&project);
        assert!(warnings.is_empty(), "{warnings:?}");
        let moves = parse_motion(&text).unwrap();
        let program_arcs = moves
            .iter()
            .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
            .count();
        let program_lines = moves
            .iter()
            .filter(|m| m.kind == MotionKind::Linear)
            .count();
        let toolpath_arcs = toolpath
            .moves
            .iter()
            .filter(|m| matches!(m.kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }))
            .count();
        let toolpath_lines = toolpath
            .moves
            .iter()
            .filter(|m| m.kind == MoveKind::Linear)
            .count();
        assert_eq!(program_arcs, toolpath_arcs, "{text}");
        assert_eq!(program_lines, toolpath_lines, "{text}");
        assert!(
            toolpath_arcs >= 3 * 160,
            "the corner arcs are written as arcs"
        );
    }

    /// A helix descent changes Z on every chord, so it stays lines; the
    /// flat revolution at the bottom is fitted.
    #[test]
    fn helix_descent_is_not_fitted_flat_revolution_is() {
        let n = 72;
        let mut moves = vec![Move::rapid(pt(10.0, 0.0, 5.0))];
        for k in 1..=n {
            let angle = TAU * f64::from(k) / f64::from(n);
            let z = 5.0 - 2.0 * f64::from(k) / f64::from(n);
            moves.push(Move::linear(
                pt(10.0 * angle.cos(), 10.0 * angle.sin(), z),
                300.0,
            ));
        }
        moves.extend(chords(10.0, n as usize, 0.0, TAU, 3.0).into_iter().skip(1));
        let fitted = fit(&moves, TOL);
        assert_eq!(lines_of(&fitted), n as usize, "the descent stays lines");
        let arcs = arcs_of(&fitted);
        assert_eq!(arcs.len(), 4, "{fitted:?}");
        for arc in arcs {
            assert_approx_eq!(arc.start.z, 3.0, 1e-9);
            assert_approx_eq!(arc.end.z, 3.0, 1e-9);
        }
    }

    /// One turn of a tiny helix ending at `end_angle_rad` round `centre`
    /// at `floor_z_mm`, each of its 48 chords falling `drop_mm`, then its
    /// flat revolution back to where it ended, as `dc_cam::entry` writes
    /// them; plunged into at the helix start from `safe_z_mm` and left by
    /// a rapid up to it.
    struct TinyHelix {
        centre: Point,
        radius_mm: f64,
        end_angle_rad: f64,
        ccw: bool,
        floor_z_mm: f64,
        drop_mm: f64,
    }

    impl TinyHelix {
        fn toolpath(
            &self,
            feed_mm_min: f64,
            plunge_mm_min: f64,
            safe_z_mm: f64,
        ) -> dc_cam::Toolpath {
            let n = dc_cam::entry::HELIX_SEGMENTS_PER_REV;
            let sense = if self.ccw { 1.0 } else { -1.0 };
            let at = |angle: f64| {
                self.centre + dc_geometry::Vec2::new(angle.cos(), angle.sin()) * self.radius_mm
            };
            let endpoint = at(self.end_angle_rad);
            let start_angle = self.end_angle_rad - sense * TAU;
            let start_z = self.floor_z_mm + self.drop_mm * n as f64;
            let from = at(start_angle);
            let mut moves = vec![
                Move::rapid(pt(from.x, from.y, safe_z_mm)),
                Move::linear(pt(from.x, from.y, start_z), plunge_mm_min),
            ];
            let mut current = pt(from.x, from.y, start_z);
            for k in 1..=n {
                let next = if k == n {
                    pt(endpoint.x, endpoint.y, self.floor_z_mm)
                } else {
                    let t = k as f64 / n as f64;
                    let p = at(start_angle + sense * TAU * t);
                    pt(p.x, p.y, start_z - self.drop_mm * n as f64 * t)
                };
                let (dx, dy, dz) = (next.x - current.x, next.y - current.y, current.z - next.z);
                let length = (dx * dx + dy * dy + dz * dz).sqrt();
                moves.push(Move::linear(
                    next,
                    feed_mm_min.min(plunge_mm_min * length / dz),
                ));
                current = next;
            }
            for k in 1..=n {
                let p = if k == n {
                    endpoint
                } else {
                    at(self.end_angle_rad + sense * TAU * k as f64 / n as f64)
                };
                moves.push(Move::linear(pt(p.x, p.y, self.floor_z_mm), feed_mm_min));
            }
            moves.push(Move::rapid(pt(endpoint.x, endpoint.y, safe_z_mm)));
            dc_cam::Toolpath { moves }
        }
    }

    /// The sample project on 240 × 160 × `thickness_mm` stock with
    /// `origin`, written for `post` in `units` with arc fitting at
    /// 0.01 mm, its tool cutting at `feed` and plunging at `plunge`; its
    /// one operation the sample Pocket of a 220 × 140 mm rectangle at
    /// (10, 10), so a helix anywhere near (40, 40) is in its material.
    fn tiny_helix_project(
        thickness_mm: f64,
        origin: dc_project::MachineOrigin,
        post: &str,
        units: dc_project::Units,
        (feed, plunge): (f64, f64),
    ) -> Project {
        let mut project = sample_project();
        project.stock = Stock {
            width_mm: 240.0,
            height_mm: 160.0,
            thickness_mm: Some(thickness_mm),
        };
        project.origin = origin;
        project.display_units = units;
        project.arc_fit_tolerance_mm = Some(0.01);
        project.post_processor = PostProcessorRef::latest(post).unwrap();
        project.tools[0].feed_mm_min = Some(feed);
        project.tools[0].plunge_mm_min = Some(plunge);
        if let Some(definition) = project.definitions.values_mut().next() {
            definition.profiles = vec![Polyline::rect(Point::new(0.0, 0.0), 220.0, 140.0)];
        }
        for instance in &mut project.instances {
            instance.z_top_mm = Some(thickness_mm);
        }
        project.operations = vec![Operation::Pocket(dc_project::test_support::sample_pocket())];
        project
    }

    /// #262, from the Pocket property's committed seed (`9cea68b9…`) and
    /// `PROPTEST_RNG_SEED=105`: a helix 0.17 mm across and its flat
    /// revolution. The flat revolution is fitted as a closed run of four
    /// 90° arcs, which S12 refused: its chords are 7.5° and its pieces'
    /// angle slack (the tolerance over the radius) as much, so a vertex
    /// one chord past a piece's end counted as on that piece, and the
    /// next chord's deviation was measured from the start of the next
    /// piece, not from that vertex: over arc the chord does not span,
    /// 0.011023 mm off it against 0.011001 allowed. Both cases export in
    /// inch and in mm.
    #[test]
    fn small_helix_flat_revolution_in_inches_exports() {
        use dc_project::{MachineOrigin, OriginXy, OriginZ, Units};
        // The seed `9cea68b9…`: LinuxCNC, top left and stock top, inch;
        // its moves 23720 to 23816.
        let inch_case = (
            TinyHelix {
                centre: Point::new(40.206_913_519_649_284, 39.550_644_954_268_115),
                radius_mm: 40.293_501_470_256 - 40.206_913_519_649_284,
                end_angle_rad: 0.0,
                ccw: true,
                floor_z_mm: 1.252_841_074_242_111_2,
                drop_mm: 0.001_689_111_677_942_3,
            },
            5.872_515_626_754_48,
            MachineOrigin {
                xy: OriginXy::TopLeft,
                z: OriginZ::StockTop,
            },
            "linuxcnc",
            (1_787.111_112_547_554_7, 2_136.427_614_162_320_4),
        );
        // `PROPTEST_RNG_SEED=105`: Mach3, bottom left and stock bottom, mm.
        let mm_case = (
            TinyHelix {
                centre: Point::new(43.157_111_168_893_31, 47.102_805_434_202_09),
                radius_mm: 43.240_669_225_108_84 - 43.157_111_168_893_31,
                end_angle_rad: 0.0,
                ccw: false,
                floor_z_mm: 3.145_858_777_472_146_7,
                drop_mm: 0.008_047_045_301_816,
            },
            3.980_585_106_286_197_7,
            MachineOrigin {
                xy: OriginXy::BottomLeft,
                z: OriginZ::StockBottom,
            },
            "mach3",
            (942.682_590_680_295_1, 2_284.956_191_250_548_7),
        );
        let mut failures = Vec::new();
        for (helix, thickness_mm, origin, post, rates) in [inch_case, mm_case] {
            for units in [Units::Inch, Units::Mm] {
                let project = tiny_helix_project(thickness_mm, origin.clone(), post, units, rates);
                let op = project.operations[0].clone();
                let toolpath = helix.toolpath(rates.0, rates.1, thickness_mm + op.safe_z_mm());
                let tool = project.tools[0].clone();
                let result = emit(&project, &[(op, tool, toolpath).into()], "2026-10-21");
                let program = match result {
                    Ok((program, _)) => program,
                    Err(e) => {
                        failures.push(format!("{post} in {units:?}: {e:?}"));
                        continue;
                    }
                };
                let arcs = parse_motion(&program.text)
                    .unwrap()
                    .iter()
                    .filter(|m| matches!(m.kind, MotionKind::ArcCw | MotionKind::ArcCcw))
                    .count();
                assert!(
                    arcs > 0,
                    "{post} in {units:?}: the flat revolution is fitted"
                );
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
