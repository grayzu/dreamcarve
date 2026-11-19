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
//! - A run also falls back when its arcs as written, from their rounded
//!   ends about the centre their snapped I and J (or R) give, stray from
//!   the moves they replace by more than the tolerance plus one unit in
//!   the last place, the budget S12 judges them by (#355). The snap moves
//!   a centre up to two quanta each way, 0.0072 mm in inch.
//! - The Kasa fit is `dc_geometry::fit_circle_kasa`.
//! - The search skips, before its fit, a window whose moves differ too
//!   much in length for any circle to pass the step uniformity gate, and
//!   the gates run cheapest first (#480). Neither changes which arcs are
//!   found: a V-bit run's long level stretch of moves from 0.0002 to 2 mm
//!   took 41 s to fit, and now takes 0.2 s.

use std::cmp::Ordering;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use dc_cam::{Move, MoveKind};
use dc_geometry::{
    EPS_MM, Point, Point3, Segment, approx_eq, fit_circle_kasa, point_segment_distance_mm,
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

/// The smallest radius an arc is written with, in units of the last
/// written place (#506): 0.005 mm at three millimetre places. A V-Carve
/// chain round a tip a few thousandths across is fitted with arcs whose
/// radius is a unit or two, whose I and J round to the same units; the
/// safety pass could not match such a written arc to its moves
/// (`ArcFitViolation`, line art `0x54`). Its moves are written instead.
pub const MIN_ARC_RADIUS_QUANTA: f64 = 5.0;

/// Most points one fit considers: the greedy window. A 5° flattener gives
/// 73 points for a full circle, so this preserves that path while bounding
/// the search on a long non-circular polyline.
pub const MAX_CANDIDATE_POINTS: usize = 128;

/// Fewest chords a run replaces. A fitted arc in a program must cover at
/// least this many toolpath moves (S12).
pub const MIN_RUN_CHORDS: usize = 3;

/// Fewest points a run has: [`MIN_RUN_CHORDS`] chords.
const MIN_RUN_POINTS: usize = MIN_RUN_CHORDS + 1;

/// The smallest radius a run is fitted on, in tolerances (#527). On a
/// circle a few tolerances across, the tolerance is a fair part of the
/// radius, so points that are only near it fit: a V-bit floor's leftover
/// loop a few hundredths across, with its link in and out of the ring's
/// lap, went round circles about 0.04 mm in radius, and once their I and J
/// were snapped to the written places the arcs no longer followed the
/// moves (`ArcFitViolation`, lettering `0x37` in inches, line art `0x9b`).
/// Such a run is a few moves long and is written as its moves. Six keeps
/// #262's flat helix revolutions (radius 0.084 mm at 0.01) fitted.
const MIN_RADIUS_TOLERANCES: f64 = 6.0;

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

/// A run whose chord is at most this fraction of the radius closes on
/// itself: a whole turn.
const CLOSED_CHORD_RATIO: f64 = 1e-6;

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

/// Fits arcs to `moves`, a toolpath in project space, within
/// `tolerance_mm`: every vertex of a replaced run, and the midpoint of
/// every shallow corner arc in it, lies within `tolerance_mm` of the
/// fitted circle, and the fitted arc lies within `tolerance_mm` of every
/// move it replaces along its whole length. Runs are maximal sequences of consecutive straight cuts
/// at one Z with one feed, started from a known position; rapids,
/// plunges, Z-changing moves, and arcs deeper than the tolerance pass
/// through unchanged and end runs. Fewer than [`MIN_RUN_CHORDS`] + 1 points
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
/// `DEVIATION_SAMPLES` points between.
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
    // Where the arc bows farthest from the chord's line: square to the
    // chord, where the arc's tangent is parallel to it (#453). Towards the
    // chord's middle is the same only when both ends are equally far from
    // the centre; a run's vertices may each be off the circle by up to the
    // tolerance, and there the arc strayed 0.00002 mm past it unseen.
    let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let middle_rad = (middle.y - centre.y).atan2(middle.x - centre.x);
    let square_rad = (a.x - b.x).atan2(b.y - a.y);
    let samples = (0..=DEVIATION_SAMPLES + 1)
        .map(|k| sweep_rad * f64::from(k) / f64::from(DEVIATION_SAMPLES + 1))
        .map(|t| from_rad + t);
    let farthest = [middle_rad, middle_rad + PI, square_rad, square_rad + PI]
        .into_iter()
        .filter(|&angle| on_arc(angle));
    samples
        .chain(farthest)
        .map(|angle| point_segment_distance_mm(chord, at(angle)))
        .fold(0.0, f64::max)
}

/// One arc found in a point sequence: points `start_index..=end_index`.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    find_arc_runs_counted(points, mids, tolerance_mm, &mut SearchWork::default())
}

/// The work [`find_arc_runs`] did, for the test that bounds it (#480).
#[derive(Debug, Default, Clone, Copy)]
struct SearchWork {
    /// Windows given a circle fit and judged by the gates.
    windows: usize,
}

/// [`find_arc_runs`], counting its work in `work`.
fn find_arc_runs_counted(
    points: &[Point],
    mids: &[Option<Point>],
    tolerance_mm: f64,
    work: &mut SearchWork,
) -> Vec<FoundArc> {
    let n = points.len();
    let lengths: Vec<f64> = points
        .windows(2)
        .map(|pair| match (pair.first(), pair.get(1)) {
            (Some(a), Some(b)) => a.distance_to(*b),
            _ => f64::NAN,
        })
        .collect();
    let mut steps = WindowSteps::default();
    // The gates read `mids` window by window; the skip below reads them as
    // the gates do only when every move has its entry.
    let every_mid = mids.len() + 1 >= n;
    let mut found = Vec::new();
    let mut i = 0;
    while i + MIN_RUN_POINTS <= n {
        let min_end = i + MIN_RUN_POINTS - 1;
        let max_end = (n - 1).min(i + MAX_CANDIDATE_POINTS - 1);
        steps.measure(&lengths, mids, i, max_end);
        let mut best = None;
        for end in (min_end..=max_end).rev() {
            let Some(sub) = points.get(i..=end) else {
                continue;
            };
            let (Some(&first), Some(&last)) = (sub.first(), sub.last()) else {
                continue;
            };
            // #480: a window whose moves differ too much in length for any
            // circle to make their steps even is rejected before its fit.
            if every_mid
                && steps
                    .of(end - i)
                    .is_some_and(|s| s.uneven_on_any_circle(first.distance_to(last), tolerance_mm))
            {
                continue;
            }
            work.windows += 1;
            let Some((centre, radius_mm)) = fit_circle_kasa(sub) else {
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

/// `sin(x) / x` at `x = π/4`, rounded down: for `x` up to `π/4`,
/// `sin(x) ≥ SIN_RATIO · x`.
const SIN_RATIO: f64 = 0.9;

/// Relative slack [`StepSpan::uneven_on_any_circle`] keeps over its
/// bound, for the rounding of the angles the gates measure (#480). With
/// it the bound holds while the median step is over [`SWEEP_EPS`], and a
/// window whose median step is not fails the gate anyway.
const UNEVEN_SLACK: f64 = 0.05;

/// The lengths of a window's moves, as the step uniformity gate sees them.
#[derive(Debug, Clone, Copy)]
struct StepSpan {
    /// The longest chord step's length, mm. A chord step is a move that is
    /// not a shallow corner arc: the gate's largest step and median are
    /// over chord steps alone.
    longest_mm: f64,
    /// The upper middle of the chord steps' lengths, mm: at least their
    /// median.
    median_mm: f64,
    /// The longest move of any kind, mm: the window spans at least this.
    span_mm: f64,
}

impl StepSpan {
    /// Whether the step uniformity gate (3 in [`gates_pass`]) rejects the
    /// window on every circle the residual gate (1) accepts, where
    /// `chord_mm` is the distance between its ends (#480). The search then
    /// skips it before its fit: the arcs found are the same.
    ///
    /// Within the residual gate every point is between `r − t` and `r + t`
    /// from the centre, so a step of angle `θ` over a move of length `L`
    /// has `4·ra·rb·sin²(θ/2) = L² − (ra − rb)²`, with `|ra − rb| ≤ 2t`:
    ///
    /// - `θ ≥ 2·sin(θ/2) ≥ √(L² − 4t²) / (r + t)`;
    /// - `θ ≤ L / (SIN_RATIO · (r − t))` for `θ` up to `π/2`, the most the
    ///   step angle gate (2) lets through.
    ///
    /// The largest step is at least the first bound over the longest move,
    /// the median at most the second over the median move. The gate
    /// rejects when the largest is over [`MAX_STEP_RATIO`] times the
    /// median, so it surely rejects when
    /// `√(L_max² − 4t²) · SIN_RATIO · (r − t) > MAX_STEP_RATIO · L_med · (r + t)`.
    /// The left side gains on the right as `r` grows, and every point is
    /// within `r + t` of the centre, so `r ≥ D/2 − t` for any distance `D`
    /// between two of them: the bound is checked at that least radius, and
    /// holds for every circle.
    fn uneven_on_any_circle(&self, chord_mm: f64, tolerance_mm: f64) -> bool {
        // The residual gate's own rounding, far inside this.
        let t = tolerance_mm * (1.0 + UNEVEN_SLACK) + EPS_MM * EPS_MM;
        let least_radius_mm = self.span_mm.max(chord_mm) / 2.0 - t;
        let spread2 = self.longest_mm * self.longest_mm - 4.0 * t * t;
        if !(least_radius_mm > t && spread2 > 0.0) {
            return false;
        }
        spread2.sqrt() * SIN_RATIO * (least_radius_mm - t)
            > MAX_STEP_RATIO * (1.0 + UNEVEN_SLACK) * self.median_mm * (least_radius_mm + t)
    }
}

/// The [`StepSpan`] of every window from one start, reused from start to
/// start.
#[derive(Debug, Default)]
struct WindowSteps {
    /// The chord steps' lengths so far, ascending.
    sorted: Vec<f64>,
    /// The span of the window of `k + 1` moves at `k`.
    spans: Vec<Option<StepSpan>>,
}

impl WindowSteps {
    /// Measures the windows from point `start` to each end up to
    /// `max_end`, where `lengths[k]` is the length of the move from point
    /// `k` and `mids[k]` its shallow arc's midpoint, if it is one.
    fn measure(&mut self, lengths: &[f64], mids: &[Option<Point>], start: usize, max_end: usize) {
        self.sorted.clear();
        self.spans.clear();
        let mut span_mm = 0.0_f64;
        for k in start..max_end {
            let length_mm = lengths.get(k).copied().unwrap_or(f64::NAN);
            span_mm = span_mm.max(length_mm);
            if mids.get(k).is_none_or(Option::is_none) {
                let at = self.sorted.partition_point(|&s| s < length_mm);
                self.sorted.insert(at, length_mm);
            }
            let span = match (self.sorted.last(), self.sorted.get(self.sorted.len() / 2)) {
                (Some(&longest_mm), Some(&median_mm)) => Some(StepSpan {
                    longest_mm,
                    median_mm,
                    span_mm,
                }),
                _ => None,
            };
            self.spans.push(span);
        }
    }

    /// The span of the window of `moves` moves from the start.
    fn of(&self, moves: usize) -> Option<&StepSpan> {
        self.spans.get(moves.checked_sub(1)?)?.as_ref()
    }
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
    let tiny = radius_mm * CLOSED_CHORD_RATIO;
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
/// upstream's, two chord steps of one size at least (#213), every move
/// within the tolerance of the arc along its whole length (#226, #229),
/// and at most one turn (#502).
///
/// A window passes only if it passes every gate, so their order changes
/// what a window costs, never whether it passes. They run cheapest first
/// (#480): the chord ratio from the ends alone, the residual, then one
/// pass over the steps for the step, turn and sweep gates, and last the
/// sampled deviation of every move from the arc, which costs a dozen
/// trigonometric calls a move. On a long level run of short moves almost
/// every window fails the step uniformity gate, and the deviation used to
/// be found for all of its moves first.
fn gates_pass(
    points: &[Point],
    mids: &[Option<Point>],
    centre: Point,
    radius_mm: f64,
    tolerance_mm: f64,
) -> bool {
    // 0. A circle more than a few tolerances across (#527).
    if radius_mm < MIN_RADIUS_TOLERANCES * tolerance_mm {
        return false;
    }
    // 5. A chord that is a fair part of the radius, unless the run closes.
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return false;
    };
    let chord = first.distance_to(*last);
    let closed = chord <= radius_mm * CLOSED_CHORD_RATIO;
    if !closed && chord < radius_mm * MIN_CHORD_RATIO {
        return false;
    }
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
    let angles: Vec<f64> = points
        .iter()
        .map(|p| (p.y - centre.y).atan2(p.x - centre.x))
        .collect();
    let mut diffs = Vec::with_capacity(points.len());
    let mut steps = Vec::with_capacity(points.len());
    let mut total_sweep = 0.0;
    // 2a. Every step turns the same way round the centre: a run that goes
    // back along the circle (a Profile ramp's return to its entry point,
    // then the lap) is not one arc, though every point is on it.
    let mut turn = 0.0_f64;
    for (k, pair) in angles.windows(2).enumerate() {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
            return false;
        };
        let mut diff = b - a;
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
        if step > SWEEP_EPS {
            if turn * diff < 0.0 {
                return false;
            }
            turn = diff;
        }
        if mids.get(k).is_none_or(Option::is_none) {
            steps.push(step);
        }
        diffs.push(diff);
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
    // 6. At most one turn (#502). `split_arc` takes the sweep from the
    // run's ends, which give it only modulo a turn: a run that goes on
    // round its circle past a full turn (a ring joined a few chords before
    // the point it laps from) would be written as its last part alone and
    // the lap before it never cut. A run that closes is one turn, not two.
    let turns = total_sweep / TAU;
    if !(if closed { turns < 1.5 } else { turns < 1.0 }) {
        return false;
    }
    // 1a. The arc stays within the tolerance of every move it replaces,
    // not only of its ends (#226, #229): a square's four corners lie on
    // one circle, which bows 1.45 mm off a 7 mm side. A shallow corner arc
    // counts as its chord: both bow the same way, each by at most the
    // tolerance.
    !points
        .windows(2)
        .zip(angles.iter().zip(&diffs))
        .any(
            |(pair, (&from_rad, &diff))| match (pair.first(), pair.get(1)) {
                (Some(&a), Some(&b)) => {
                    arc_chord_deviation_mm(centre, radius_mm, from_rad, diff, a, b) > tolerance_mm
                }
                _ => true,
            },
        )
}

/// The median of `values`, which it reorders; `None` if empty. It selects
/// the middle value rather than sorting (#480): every window of a long
/// level run takes a median of up to [`MAX_CANDIDATE_POINTS`] steps.
fn median(values: &mut [f64]) -> Option<f64> {
    let n = values.len();
    let middle = n / 2;
    if middle >= n {
        return None;
    }
    let order = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(Ordering::Equal);
    let (below, &mut upper, _) = values.select_nth_unstable_by(middle, order);
    if n.is_multiple_of(2) {
        let lower = below.iter().copied().max_by(order)?;
        Some((lower + upper) / 2.0)
    } else {
        Some(upper)
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
    // A run that closes is the whole turn `gates_pass` let through as
    // closed (#502): its ends may be a hair apart either way, and a hair
    // past the start is not a hair-long arc.
    let closed = start.xy().distance_to(end.xy()) <= radius_mm * CLOSED_CHORD_RATIO;
    if closed || sweep.abs() < SWEEP_EPS {
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
/// With I/J, the offsets are snapped within two output quanta to
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
    // #506: an arc a few units across is not written: its centre, start
    // and end are rounded to the same units, so its words describe a
    // different arc. Its moves are written instead.
    if opts.quantum > 0.0 && raw_i.hypot(raw_j) < MIN_ARC_RADIUS_QUANTA * opts.quantum {
        return rejected;
    }
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

/// Whether every sub-arc of `run` survives formatting, walking the
/// formatted position from `start` (machine coordinates) through each
/// sub-arc's end: the controller accepts it, and as written it follows the
/// moves it replaces ([`written_arcs_follow_moves`], #355). `to_machine`
/// maps project XY to machine XY.
fn run_survives_formatting(
    run: &ArcRun,
    start: Point,
    opts: &EmitOptions<'_>,
    to_machine: &dyn Fn(Point) -> Point,
    tolerance_mm: f64,
) -> bool {
    let machine_mirrors = mirrors(to_machine);
    let mut written = Vec::with_capacity(run.arcs.len());
    let mut position = start;
    for arc in &run.arcs {
        let end = to_machine(arc.end.xy());
        let centre = to_machine(arc.centre);
        let emitted = resolve_emitted_arc(end, centre, position, opts);
        if !emitted.accepted {
            return false;
        }
        let (Some(x), Some(y)) = ((opts.format)(end.x), (opts.format)(end.y)) else {
            return false;
        };
        let target = Point::new(x, y);
        let clockwise = arc.clockwise != machine_mirrors;
        let Some(written_centre) = written_centre(position, target, &emitted, clockwise, opts)
        else {
            return false;
        };
        written.push(WrittenArc::new(position, target, written_centre, clockwise));
        position = target;
    }
    let Some(first) = run.arcs.first() else {
        return false;
    };
    // The budget S12 gives a fitted arc: the tolerance plus one unit in
    // the last written place, in output units; and how near, on each
    // axis, it takes a vertex to be to the written end.
    let (budget, reach) = if opts.mm_per_unit > 0.0 {
        (
            tolerance_mm / opts.mm_per_unit + opts.quantum,
            opts.quantum + EPS_MM / opts.mm_per_unit,
        )
    } else {
        return false;
    };
    written_arcs_follow_moves(
        &written,
        first.start.xy(),
        &run.fallback,
        to_machine,
        tolerance_mm,
        (budget, reach),
    )
}

/// Whether `to_machine` mirrors the plane, so a clockwise arc in project
/// space is counter-clockwise on the machine.
fn mirrors(to_machine: &dyn Fn(Point) -> Point) -> bool {
    let origin = to_machine(Point::new(0.0, 0.0));
    let x = to_machine(Point::new(1.0, 0.0)) - origin;
    let y = to_machine(Point::new(0.0, 1.0)) - origin;
    x.cross(y) < 0.0
}

/// The centre the controller takes for an arc from `start` to `target`
/// (formatted, machine coordinates) written with `emitted`'s words: the
/// start plus the written I and J, or, with R, the centre on the
/// `clockwise` side of the chord at the written radius.
fn written_centre(
    start: Point,
    target: Point,
    emitted: &EmittedArc,
    clockwise: bool,
    opts: &EmitOptions<'_>,
) -> Option<Point> {
    if opts.arc_format == ArcFormat::R {
        let radius = (opts.format)(emitted.radius)?;
        return Some(crate::parse::r_centre(
            start.x, start.y, target.x, target.y, radius, clockwise,
        ));
    }
    Some(Point::new(
        start.x + (opts.format)(emitted.i)?,
        start.y + (opts.format)(emitted.j)?,
    ))
}

/// One sub-arc as the controller cuts it, machine coordinates: from its
/// written start, about its written centre, to its written end.
#[derive(Debug, Clone, Copy)]
struct WrittenArc {
    start: Point,
    end: Point,
    centre: Point,
    radius: f64,
    start_rad: f64,
    sweep_rad: f64,
    clockwise: bool,
}

impl WrittenArc {
    fn new(start: Point, end: Point, centre: Point, clockwise: bool) -> Self {
        let start_rad = (start.y - centre.y).atan2(start.x - centre.x);
        let end_rad = (end.y - centre.y).atan2(end.x - centre.x);
        let sweep_rad = if clockwise {
            start_rad - end_rad
        } else {
            end_rad - start_rad
        }
        .rem_euclid(TAU);
        Self {
            start,
            end,
            centre,
            radius: start.distance_to(centre),
            start_rad,
            sweep_rad,
            clockwise,
        }
    }

    /// The angle turned from the start, in the arc's direction, to `p`.
    fn turn(&self, p: Point) -> f64 {
        let angle = (p.y - self.centre.y).atan2(p.x - self.centre.x);
        if self.clockwise {
            self.start_rad - angle
        } else {
            angle - self.start_rad
        }
        .rem_euclid(TAU)
    }

    fn on_circle(&self, p: Point, budget: f64) -> bool {
        (p.distance_to(self.centre) - self.radius).abs() <= budget
    }

    /// Whether `p` lies on the arc within `budget`, its ends included to
    /// the angle `budget` makes at the radius.
    fn holds(&self, p: Point, budget: f64) -> bool {
        if !self.on_circle(p, budget) {
            return false;
        }
        let slack = self.slack(budget);
        let turn = self.turn(p);
        turn <= self.sweep_rad + slack || turn >= TAU - slack
    }

    /// The angle `budget` makes at the radius.
    fn slack(&self, budget: f64) -> f64 {
        if self.radius > 0.0 {
            budget / self.radius
        } else {
            TAU
        }
    }

    /// Whether `p` is past the arc's end by no more than the angle
    /// `budget` makes at the radius, as S12 places it.
    fn past_end(&self, p: Point, budget: f64) -> bool {
        let turn = self.turn(p);
        turn > self.sweep_rad && turn <= self.sweep_rad + self.slack(budget)
    }

    /// The turn to the point of the arc nearest `p` in angle: the start or
    /// the end for `p` beyond them.
    fn clamped_turn(&self, p: Point) -> f64 {
        let turn = self.turn(p);
        if turn <= self.sweep_rad {
            turn
        } else if turn - self.sweep_rad < TAU - turn {
            self.sweep_rad
        } else {
            0.0
        }
    }

    /// The farthest the arc between turns `from` and `to` lies from the
    /// straight move `a` to `b`.
    fn deviation(&self, from: f64, to: f64, a: Point, b: Point) -> f64 {
        let sign = if self.clockwise { -1.0 } else { 1.0 };
        arc_chord_deviation_mm(
            self.centre,
            self.radius,
            self.start_rad + sign * from,
            sign * (to - from),
            a,
            b,
        )
    }
}

/// Whether the `written` sub-arcs follow `moves`, the moves of the run
/// they replace, from `start` (project space), as S12 judges them (#355):
/// every vertex, and every shallow arc's midpoint, lies on a sub-arc in
/// order, and over the part of every sub-arc between a move's ends the
/// arc lies within `budget` (output units) of the move's chord. The
/// written centre is not the fitted one: the I/J snap moves it up to two
/// quanta each way so the rounded ends agree on the radius, which in inch
/// is 0.0072 mm, and the fit's chords may already be bowed by nearly the
/// whole tolerance. `tolerance_mm` decides which corner arcs are shallow.
///
/// S12 also needs [`MIN_RUN_CHORDS`] moves under the arcs, and counts
/// neither a first move that ends within `reach` (output units, each
/// axis) of the written start, which it reads as a straight move written
/// as nothing, nor any move after the first vertex within `reach` of the
/// last sub-arc's written end, where the run ends. So a run that begins
/// or ends with chords too short to write has fewer moves than the fit
/// counted (#355).
fn written_arcs_follow_moves(
    written: &[WrittenArc],
    start: Point,
    moves: &[Move],
    to_machine: &dyn Fn(Point) -> Point,
    tolerance_mm: f64,
    (budget, reach): (f64, f64),
) -> bool {
    let (Some(first), Some(last)) = (written.first(), written.last()) else {
        return false;
    };
    let near = |p: Point, q: Point| approx_eq(p.x, q.x, reach) && approx_eq(p.y, q.y, reach);
    let mut piece = 0;
    let mut covered = 0;
    let mut from = start;
    let mut vertex = to_machine(start);
    for mv in moves {
        let to = to_machine(mv.to.xy());
        let midpoint = match mv.kind {
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let cw = matches!(mv.kind, MoveKind::ArcCw { .. });
                match shallow_arc_midpoint(from, mv.to.xy(), centre, cw, tolerance_mm) {
                    Some(m) => Some(to_machine(m)),
                    None => return false,
                }
            }
            MoveKind::Linear => None,
            MoveKind::Rapid | MoveKind::Dwell { .. } => return false,
        };
        let first_piece = piece;
        for p in midpoint.into_iter().chain([to]) {
            loop {
                let Some(current) = written.get(piece) else {
                    return false;
                };
                if current.holds(p, budget) {
                    // As S12 (#453): past the piece's end, away from the
                    // joint, and on the next piece, `p` is on the next,
                    // though on a circle near the budget in radius it is
                    // still within this piece's angle slack.
                    let on_next = written
                        .get(piece + 1)
                        .is_some_and(|next| next.holds(p, budget));
                    if current.past_end(p, budget) && !near(p, current.end) && on_next {
                        piece += 1;
                        continue;
                    }
                    break;
                }
                if !current.on_circle(p, budget) || piece + 1 >= written.len() {
                    return false;
                }
                piece += 1;
            }
        }
        for arc in written.get(first_piece..=piece).unwrap_or_default() {
            let (a, b) = (arc.clamped_turn(vertex), arc.clamped_turn(to));
            if b > a && arc.deviation(a, b, vertex, to) > budget {
                return false;
            }
        }
        let counted = covered > 0 || !near(to, first.start);
        if piece + 1 == written.len() && near(to, last.end) {
            return covered + usize::from(counted) >= MIN_RUN_CHORDS;
        }
        covered += usize::from(counted);
        from = mv.to.xy();
        vertex = to;
    }
    covered >= MIN_RUN_CHORDS
}

/// Resolves `fitted` into what can be emitted: every run the controller
/// would reject once formatted, whose written arcs stray more than
/// `tolerance_mm` plus one unit in the last place from the moves they
/// replace (#355), or that has no known emitted start, is replaced by its
/// original moves. `start` is the formatted machine XY the controller is
/// at when the sequence begins, if known; `to_machine` maps project XY to
/// machine XY; `tolerance_mm` is the one `fitted` was fitted with.
/// Returns the sequence and how many runs fell back. Ported from
/// `applyEmittedArcFallback`.
#[must_use]
pub fn apply_fallback(
    fitted: Vec<Fitted>,
    opts: &EmitOptions<'_>,
    start: Option<Point>,
    to_machine: &dyn Fn(Point) -> Point,
    tolerance_mm: f64,
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
                let survives = position.is_some_and(|p| {
                    run_survives_formatting(&run, p, opts, to_machine, tolerance_mm)
                });
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
mod tests;
