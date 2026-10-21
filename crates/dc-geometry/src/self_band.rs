// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! One line measured against itself (#221): how close two stretches of
//! one line come where they lie far apart along it, as in a hairpin, a
//! spiral, or an outline with a narrow neck.
//!
//! Neighbouring stretches of a line always touch, so they are never
//! compared: only points more than a reach apart along the line are. A
//! stretch whose direction turns by at most a right angle is never closer
//! than `1/√2` of its length end to end, so with a reach of `√2·d` a
//! smooth curve and a corner of 90° or more never come within `d` of
//! themselves. A sharper corner is where two lines meet, as #210 treats
//! them: its arms are set aside within the reach of the tip.

use crate::distance::{closest_points, segment_distance_mm, segment_meetings};
use crate::point::{Point, Rect};
use crate::polyline::{Polyline, Segment};

/// A line is cut into stretches at most this fraction of the reach long,
/// so where along the line a stretch lies is known to within an eighth of
/// the reach.
const STRETCHES_PER_REACH: f64 = 8.0;

/// The shortest stretch, mm, so a tiny reach cannot cut a long line into
/// millions of pieces.
const MIN_STRETCH_MM: f64 = 0.01;

/// Tangents whose dot product is below this turn by more than a right
/// angle (dimensionless): a little below 0, so a square corner is not a
/// sharp one through rounding.
const SHARP_DOT: f64 = -1e-9;

/// A piece of one segment of a line, and where it lies along the line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stretch {
    segment: Segment,
    bounds: Rect,
    /// Distance along the line to its start, mm.
    from_mm: f64,
    /// Distance along the line to its end, mm.
    to_mm: f64,
}

/// One line cut into short stretches, with the points where it meets
/// itself, for measuring it against itself (module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct SelfBand {
    stretches: Vec<Stretch>,
    closed: bool,
    reach_mm: f64,
    meetings: Vec<Point>,
}

impl SelfBand {
    /// `line` cut into stretches, compared only where they lie more than
    /// `reach_mm` apart along it (the shorter way round a closed line).
    /// The line meets itself at every vertex where it turns by more than a
    /// right angle, and wherever two of those far-apart stretches touch,
    /// cross, or come within `tolerance_mm` of each other.
    #[must_use]
    pub fn new(line: &Polyline, reach_mm: f64, tolerance_mm: f64) -> Self {
        let max_mm = (reach_mm / STRETCHES_PER_REACH).max(MIN_STRETCH_MM);
        let mut stretches = Vec::new();
        let mut at_mm = 0.0;
        for segment in line.segments() {
            let length_mm = segment.length_mm();
            let count = pieces(length_mm, max_mm);
            for k in 0..count {
                let (from, to) = (fraction(k, count), fraction(k + 1, count));
                let Some(piece) = segment.piece(from, to) else {
                    continue;
                };
                let to_mm = if k + 1 == count {
                    at_mm + length_mm
                } else {
                    at_mm + length_mm * to
                };
                let from_mm = stretches.last().map_or(0.0, |s: &Stretch| s.to_mm);
                stretches.push(Stretch {
                    segment: piece,
                    bounds: bounds_of(piece),
                    from_mm,
                    to_mm,
                });
            }
            at_mm += length_mm;
        }
        let mut band = Self {
            stretches,
            closed: line.is_closed(),
            reach_mm,
            meetings: sharp_corners(line),
        };
        let mut touches = Vec::new();
        band.near_pairs(tolerance_mm, |band, i, j| {
            if band.far_apart(i, j)
                && let (Some(a), Some(b)) = (band.stretches.get(i), band.stretches.get(j))
            {
                segment_meetings(a.segment, b.segment, tolerance_mm, &mut touches);
            }
        });
        band.meetings.extend(touches);
        band
    }

    /// The points where the line meets itself: its sharp corners and
    /// where far-apart stretches touch, cross, or nearly touch.
    #[must_use]
    pub fn meetings(&self) -> &[Point] {
        &self.meetings
    }

    /// The narrowest gap, mm, between two points of the line more than
    /// the reach apart along it, when it is less than `below_mm` and
    /// `counts` accepts the pair: `None` when there is none. A pair of
    /// which both points lie within the reach of a meeting point is set
    /// aside, as [`crate::distance_away_from_mm`] sets aside the stretches
    /// of two lines near where they meet. `counts` sees the closest points
    /// of two short pieces, one on each stretch.
    #[must_use]
    pub fn narrowest_mm(
        &self,
        below_mm: f64,
        counts: impl Fn(Point, Point) -> bool,
    ) -> Option<f64> {
        let away: Vec<Vec<Segment>> = self
            .stretches
            .iter()
            .map(|s| s.segment.outside_discs(&self.meetings, self.reach_mm))
            .collect();
        let mut best: Option<f64> = None;
        self.near_pairs(below_mm, |band, i, j| {
            if !band.far_apart(i, j) {
                return;
            }
            let (Some(a), Some(b), Some(away_a), Some(away_b)) = (
                band.stretches.get(i),
                band.stretches.get(j),
                away.get(i),
                away.get(j),
            ) else {
                return;
            };
            let pairs = away_a
                .iter()
                .map(|&piece| (piece, b.segment))
                .chain(away_b.iter().map(|&piece| (a.segment, piece)));
            for (s, t) in pairs {
                let (p, q) = closest_points(s, t);
                let gap_mm = p.distance_to(q);
                if gap_mm < below_mm && best.is_none_or(|b| gap_mm < b) && counts(p, q) {
                    best = Some(gap_mm);
                }
            }
        });
        best
    }

    /// The stretches of the line, whole, that lie more than the reach
    /// along it from every stretch within `slack_mm` of `path` (the
    /// stretch `path` runs on), among those whose bounds come within
    /// `within_mm` of `path`'s: what a cut along `path` is measured
    /// against.
    #[must_use]
    pub fn far_from(&self, path: Segment, slack_mm: f64, within_mm: f64) -> Vec<Segment> {
        let bounds = bounds_of(path);
        let own: Vec<usize> = self
            .stretches
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                near(bounds, s.bounds, slack_mm) && segment_distance_mm(s.segment, path) <= slack_mm
            })
            .map(|(i, _)| i)
            .collect();
        self.stretches
            .iter()
            .enumerate()
            .filter(|(j, s)| {
                near(bounds, s.bounds, within_mm) && own.iter().all(|&i| self.far_apart(i, *j))
            })
            .map(|(_, s)| s.segment)
            .collect()
    }

    /// Whether stretches `i` and `j` lie more than the reach apart along
    /// the line, the shorter way round a closed one.
    fn far_apart(&self, i: usize, j: usize) -> bool {
        let (Some(a), Some(b)) = (self.stretches.get(i), self.stretches.get(j)) else {
            return false;
        };
        let (first, second) = if a.from_mm <= b.from_mm {
            (a, b)
        } else {
            (b, a)
        };
        let ahead_mm = second.from_mm - first.to_mm;
        let gap_mm = match self.stretches.last() {
            Some(last) if self.closed => ahead_mm.min(last.to_mm - second.to_mm + first.from_mm),
            _ => ahead_mm,
        };
        i != j && gap_mm > self.reach_mm
    }

    /// Calls `f` with every pair of stretches whose bounds come within
    /// `margin_mm` of each other, each pair once.
    fn near_pairs(&self, margin_mm: f64, mut f: impl FnMut(&Self, usize, usize)) {
        let mut order: Vec<usize> = (0..self.stretches.len()).collect();
        let min_x = |i: usize| self.stretches.get(i).map_or(0.0, |s| s.bounds.min.x);
        order.sort_by(|&a, &b| min_x(a).total_cmp(&min_x(b)));
        for (k, &i) in order.iter().enumerate() {
            let Some(a) = self.stretches.get(i) else {
                continue;
            };
            for &j in order.get(k + 1..).unwrap_or_default() {
                let Some(b) = self.stretches.get(j) else {
                    continue;
                };
                if b.bounds.min.x > a.bounds.max.x + margin_mm {
                    break;
                }
                if near(a.bounds, b.bounds, margin_mm) {
                    f(self, i, j);
                }
            }
        }
    }
}

/// The vertices of `line` where it turns by more than a right angle,
/// its arms meeting at less than 90°; a closed line's first vertex
/// included. Zero-length segments are passed over.
fn sharp_corners(line: &Polyline) -> Vec<Point> {
    let segments: Vec<Segment> = line.segments().filter(|s| s.length_mm() > 0.0).collect();
    let mut corners = Vec::new();
    let pairs = segments.windows(2).filter_map(|w| match w {
        [a, b] => Some((*a, *b)),
        _ => None,
    });
    let wrap = match (segments.last(), segments.first()) {
        (Some(last), Some(first)) if line.is_closed() && segments.len() > 1 => {
            Some((*last, *first))
        }
        _ => None,
    };
    for (before, after) in pairs.chain(wrap) {
        if let (Some((_, incoming)), Some((_, outgoing))) =
            (before.point_at(before.length_mm()), after.point_at(0.0))
            && incoming.dot(outgoing) < SHARP_DOT
        {
            corners.push(after.start);
        }
    }
    corners
}

/// How many equal pieces a segment `length_mm` long is cut into so none
/// is longer than `max_mm`: at least one.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a piece count: positive, and bounded by the length over the shortest stretch"
)]
fn pieces(length_mm: f64, max_mm: f64) -> usize {
    let count = (length_mm / max_mm).ceil();
    if count.is_finite() && count >= 1.0 {
        count as usize
    } else {
        1
    }
}

/// `k / count` as a fraction of a segment.
fn fraction(k: usize, count: usize) -> f64 {
    k as f64 / count as f64
}

/// The bounds of `segment`, arc bulge included.
fn bounds_of(segment: Segment) -> Rect {
    let mut rect = Rect::from_point(segment.start).including(segment.end);
    if let Some(arc) = segment.arc() {
        for extreme in arc.axis_extremes() {
            rect = rect.including(extreme);
        }
    }
    rect
}

/// Whether `a` grown by `margin_mm` on every side meets `b`.
fn near(a: Rect, b: Rect, margin_mm: f64) -> bool {
    a.min.x - margin_mm <= b.max.x
        && b.min.x <= a.max.x + margin_mm
        && a.min.y - margin_mm <= b.max.y
        && b.min.y <= a.max.y + margin_mm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::polyline::Vertex;

    const TOL: f64 = 1e-9;
    /// √2 · 6.35 mm, the reach for a 6.35 mm bit.
    const REACH_MM: f64 = std::f64::consts::SQRT_2 * 6.35;

    fn polyline(points: &[(f64, f64)], closed: bool) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            closed,
        )
    }

    fn narrowest(line: &Polyline) -> Option<f64> {
        SelfBand::new(line, REACH_MM, 0.01).narrowest_mm(6.35, |_, _| true)
    }

    /// A U with legs 2 mm apart is 2 mm from itself; a U with legs 8 mm
    /// apart, an L, a square and a circle never come within 6.35 mm of
    /// themselves away from their neighbouring stretches.
    #[test]
    fn hairpins_are_narrow_and_square_corners_are_not() {
        let narrow = polyline(&[(0.0, 0.0), (0.0, 30.0), (2.0, 30.0), (2.0, 0.0)], false);
        assert_approx_eq!(narrowest(&narrow).unwrap_or(f64::NAN), 2.0, TOL);
        let wide = polyline(&[(0.0, 0.0), (0.0, 30.0), (8.0, 30.0), (8.0, 0.0)], false);
        assert_eq!(narrowest(&wide), None);
        let l = polyline(&[(0.0, 0.0), (0.0, 30.0), (30.0, 30.0)], false);
        assert_eq!(narrowest(&l), None);
        assert_eq!(
            narrowest(&Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)),
            None
        );
        assert_eq!(
            narrowest(&Polyline::circle(Point::new(0.0, 0.0), 5.0)),
            None
        );
    }

    /// A closed outline with a 3 mm neck is 3 mm from itself, measured the
    /// shorter way round; `counts` can turn the pair down.
    #[test]
    fn a_neck_in_a_closed_outline_is_narrow() {
        let dumbbell = polyline(
            &[
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 8.5),
                (40.0, 8.5),
                (40.0, 0.0),
                (60.0, 0.0),
                (60.0, 20.0),
                (40.0, 20.0),
                (40.0, 11.5),
                (20.0, 11.5),
                (20.0, 20.0),
                (0.0, 20.0),
            ],
            true,
        );
        assert_approx_eq!(narrowest(&dumbbell).unwrap_or(f64::NAN), 3.0, TOL);
        let band = SelfBand::new(&dumbbell, REACH_MM, 0.01);
        assert_eq!(band.narrowest_mm(6.35, |_, _| false), None);
    }

    /// A V sharper than a right angle meets itself at its tip; its arms
    /// are set aside within the reach of it, so a 60° V passes and a 10°
    /// one is `reach · sin 10°` from itself, as two lines meeting at 10°.
    #[test]
    fn sharp_corners_meet_and_set_their_arms_aside() {
        let v = |angle_deg: f64| {
            let half = (angle_deg / 2.0).to_radians();
            polyline(
                &[
                    (-30.0 * half.sin(), -30.0 * half.cos()),
                    (0.0, 0.0),
                    (30.0 * half.sin(), -30.0 * half.cos()),
                ],
                false,
            )
        };
        let band = SelfBand::new(&v(60.0), REACH_MM, 0.01);
        assert_eq!(band.meetings().len(), 1);
        assert!(band.meetings()[0].distance_to(Point::new(0.0, 0.0)) <= TOL);
        assert_eq!(narrowest(&v(60.0)), None);
        assert_approx_eq!(
            narrowest(&v(10.0)).unwrap_or(f64::NAN),
            REACH_MM * 10_f64.to_radians().sin(),
            1e-6
        );
        let square = SelfBand::new(
            &Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0),
            REACH_MM,
            0.01,
        );
        assert!(square.meetings().is_empty(), "a square corner is not sharp");
    }

    /// An open line that crosses itself meets itself at the crossing.
    #[test]
    fn a_crossing_is_a_meeting() {
        let loop_line = polyline(
            &[
                (0.0, 0.0),
                (30.0, 0.0),
                (30.0, 30.0),
                (15.0, 30.0),
                (15.0, -20.0),
            ],
            false,
        );
        let band = SelfBand::new(&loop_line, REACH_MM, 0.01);
        assert!(
            band.meetings()
                .iter()
                .any(|p| p.distance_to(Point::new(15.0, 0.0)) <= 1e-6),
            "{:?}",
            band.meetings()
        );
        assert_eq!(narrowest(&loop_line), None);
    }

    /// A cut along one leg of the narrow U is measured against the other
    /// leg, not against the stretches it runs on or next to.
    #[test]
    fn far_from_leaves_out_the_stretches_near_the_cut() {
        let narrow = polyline(&[(0.0, 0.0), (0.0, 30.0), (2.0, 30.0), (2.0, 0.0)], false);
        let band = SelfBand::new(&narrow, REACH_MM, 0.01);
        let cut = Segment {
            start: Point::new(0.0, 0.0),
            end: Point::new(0.0, 10.0),
            bulge: 0.0,
        };
        let far = band.far_from(cut, 0.01, 6.35);
        assert!(!far.is_empty());
        for s in far {
            assert_approx_eq!(s.start.x, 2.0, TOL);
            assert_approx_eq!(s.end.x, 2.0, TOL);
        }
    }
}
