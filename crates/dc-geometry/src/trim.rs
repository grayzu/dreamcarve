// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trimming an outline between the crossings either side of a point
//! (`docs/specs/phase-5/design.md`, "Algorithms → Intersections and
//! Trim"; functional capability 96).
//!
//! PureCutCNC's trim (`trimFeatureSegment`) is two picks on open
//! profiles and cannot take a middle piece out; this one is written from
//! the functional spec on [`crate::crossings`], which is the port. The
//! pieces are cut from the outline's own segments, so an arc stays an
//! arc of its circle.

use crate::distance::nearest_point;
use crate::intersect::crossings;
use crate::point::{EPS_MM, Point};
use crate::polyline::{Polyline, Segment, Vertex};

/// An outline with a piece taken out of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Trimmed {
    /// The piece removed: from one crossing to the next, or to an open
    /// outline's end. Empty of length for a closed outline that has one
    /// crossing, which only opens there.
    pub removed: Polyline,
    /// What is left, open: none, one or two pieces of an open outline;
    /// one of a closed one, from the far crossing round to the near.
    pub kept: Vec<Polyline>,
}

/// The segments of `path` that have length, each with the distance along
/// the path it starts at.
fn runs(path: &Polyline) -> (Vec<(f64, Segment)>, f64) {
    let mut out = Vec::new();
    let mut along = 0.0;
    for segment in path.segments() {
        let length = segment.length_mm();
        if length > EPS_MM {
            out.push((along, segment));
            along += length;
        }
    }
    (out, along)
}

/// The distances along `path` at which it meets `others` or itself,
/// ascending, each once. An open outline's own two ends are no cut: there
/// is nothing past them to trim to.
fn cuts(path: &Polyline, others: &[Polyline]) -> Vec<f64> {
    let (runs, total) = runs(path);
    let mut cuts = Vec::new();
    let count = runs.len();
    for (i, (start, segment)) in runs.iter().enumerate() {
        let length = segment.length_mm();
        for other in others {
            for theirs in other.segments() {
                for crossing in crossings(*segment, theirs) {
                    cuts.push(start + crossing.along_a * length);
                }
            }
        }
        // Itself: every later segment but the one it runs into, which it
        // only shares a vertex with.
        for (j, (other_start, other)) in runs.iter().enumerate().skip(i + 1) {
            let adjacent = j == i + 1 || (path.is_closed() && i == 0 && j + 1 == count);
            if adjacent {
                continue;
            }
            for crossing in crossings(*segment, *other) {
                cuts.push(start + crossing.along_a * length);
                cuts.push(other_start + crossing.along_b * other.length_mm());
            }
        }
    }
    if path.is_closed() {
        // The seam is one point, not two.
        for cut in &mut cuts {
            if total - *cut <= EPS_MM {
                *cut = 0.0;
            }
        }
    } else {
        cuts.retain(|cut| *cut > EPS_MM && total - *cut > EPS_MM);
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= EPS_MM);
    cuts
}

/// The part of `path` from distance `from` to `to` along it, as an open
/// outline; `to` may run past the end of a closed one, round the seam.
fn part(path: &Polyline, from: f64, to: f64) -> Polyline {
    let (runs, total) = runs(path);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut end = None;
    // Once round, and for a closed outline a second time for the part
    // past the seam.
    let laps = if path.is_closed() { 2 } else { 1 };
    for lap in 0..laps {
        let offset = f64::from(lap) * total;
        for (start, segment) in &runs {
            let length = segment.length_mm();
            let (a, b) = (start + offset, start + offset + length);
            let (lo, hi) = (from.max(a), to.min(b));
            if hi - lo <= EPS_MM {
                continue;
            }
            let Some(piece) = segment.piece((lo - a) / length, (hi - a) / length) else {
                continue;
            };
            vertices.push(Vertex {
                x: piece.start.x,
                y: piece.start.y,
                bulge: piece.bulge,
            });
            end = Some(piece.end);
        }
    }
    if let Some(end) = end {
        vertices.push(Vertex::line(end.x, end.y));
    }
    Polyline::new(vertices, false)
}

/// Trims `path` at `at` against `others` (and against itself): the piece
/// of it under `at`, between the crossings either side, is taken out.
///
/// An open outline leaves the part before the piece and the part after
/// it, whichever have length. A closed one leaves the rest of the loop,
/// open, from the far crossing round to the near; with a single crossing
/// it is opened there and nothing is removed.
///
/// `None` when nothing crosses `path`: the piece would be all of it.
#[must_use]
pub fn trim(path: &Polyline, at: Point, others: &[Polyline]) -> Option<Trimmed> {
    let cuts = cuts(path, others);
    let (first, last) = (*cuts.first()?, *cuts.last()?);
    let total = runs(path).1;
    let here = nearest_point(path, at)?.along_mm;
    let before = cuts.iter().rev().find(|cut| **cut <= here).copied();
    let after = cuts.iter().find(|cut| **cut > here).copied();
    let long_enough = |piece: &Polyline| piece.length_mm() > EPS_MM;
    if path.is_closed() {
        if cuts.len() == 1 {
            // One crossing: the loop opens there.
            return Some(Trimmed {
                removed: Polyline::new(Vec::new(), false),
                kept: vec![part(path, first, first + total)],
            });
        }
        // The crossings either side of the point, round the seam.
        let from = before.unwrap_or(last - total);
        let to = after.unwrap_or(first + total);
        let (from, to) = if from < 0.0 {
            (from + total, to + total)
        } else {
            (from, to)
        };
        return Some(Trimmed {
            removed: part(path, from, to),
            kept: vec![part(path, to, from + total)],
        });
    }
    let from = before.unwrap_or(0.0);
    let to = after.unwrap_or(total);
    Some(Trimmed {
        removed: part(path, from, to),
        kept: [part(path, 0.0, from), part(path, to, total)]
            .into_iter()
            .filter(long_enough)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;

    fn line(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    fn ends(path: &Polyline) -> ((f64, f64), (f64, f64)) {
        let v = path.vertices();
        let (a, b) = (v[0], v[v.len() - 1]);
        ((a.x, a.y), (b.x, b.y))
    }

    #[track_caller]
    fn assert_ends(path: &Polyline, from: (f64, f64), to: (f64, f64)) {
        let (a, b) = ends(path);
        for (got, want) in [(a.0, from.0), (a.1, from.1), (b.0, to.0), (b.1, to.1)] {
            assert_approx_eq!(got, want, 1e-9);
        }
    }

    /// Two crossing lines: a click on one arm removes that arm up to the
    /// crossing.
    #[test]
    fn an_arm_up_to_the_crossing() {
        let across = line(&[(0.0, 10.0), (40.0, 10.0)]);
        let up = line(&[(30.0, 0.0), (30.0, 20.0)]);
        let trimmed = trim(&across, Point::new(36.0, 10.0), std::slice::from_ref(&up)).unwrap();
        assert_ends(&trimmed.removed, (30.0, 10.0), (40.0, 10.0));
        assert_eq!(trimmed.kept.len(), 1);
        assert_ends(&trimmed.kept[0], (0.0, 10.0), (30.0, 10.0));
        // The other arm.
        let trimmed = trim(&across, Point::new(5.0, 11.0), &[up]).unwrap();
        assert_ends(&trimmed.removed, (0.0, 10.0), (30.0, 10.0));
        assert_ends(&trimmed.kept[0], (30.0, 10.0), (40.0, 10.0));
    }

    /// A line crossed twice: a click on the middle part splits it in two.
    #[test]
    fn a_middle_piece_splits_the_line() {
        let across = line(&[(0.0, 10.0), (40.0, 10.0)]);
        let posts = [
            line(&[(10.0, 0.0), (10.0, 20.0)]),
            line(&[(30.0, 0.0), (30.0, 20.0)]),
        ];
        let trimmed = trim(&across, Point::new(20.0, 10.0), &posts).unwrap();
        assert_ends(&trimmed.removed, (10.0, 10.0), (30.0, 10.0));
        assert_eq!(trimmed.kept.len(), 2);
        assert_ends(&trimmed.kept[0], (0.0, 10.0), (10.0, 10.0));
        assert_ends(&trimmed.kept[1], (30.0, 10.0), (40.0, 10.0));
        // A bent path keeps its corner in the piece it is in.
        let bent = line(&[(0.0, 10.0), (20.0, 10.0), (20.0, 30.0)]);
        let rail = line(&[(10.0, 0.0), (10.0, 20.0)]);
        let trimmed = trim(&bent, Point::new(20.0, 25.0), &[rail]).unwrap();
        assert_eq!(trimmed.removed.vertices().len(), 3, "the corner goes too");
        assert_ends(&trimmed.removed, (10.0, 10.0), (20.0, 30.0));
        assert_ends(&trimmed.kept[0], (0.0, 10.0), (10.0, 10.0));
    }

    /// A circle crossed twice opens between the two crossings, and what
    /// is left is still an arc of it.
    #[test]
    fn a_circle_opens_between_two_crossings() {
        let circle = Polyline::circle(Point::new(0.0, 0.0), 10.0);
        let chord = line(&[(-20.0, 6.0), (20.0, 6.0)]);
        // The crossings are at (±8, 6); the click is on the cap above.
        let trimmed = trim(&circle, Point::new(0.0, 10.0), std::slice::from_ref(&chord)).unwrap();
        assert_eq!(trimmed.kept.len(), 1);
        let kept = &trimmed.kept[0];
        assert!(!kept.is_closed());
        let (a, b) = ends(kept);
        assert_approx_eq!(a.0.abs(), 8.0, 1e-9);
        assert_approx_eq!(a.1, 6.0, 1e-9);
        assert_approx_eq!(b.0.abs(), 8.0, 1e-9);
        assert_approx_eq!(b.1, 6.0, 1e-9);
        assert!((a.0 - b.0).abs() > 15.0, "from one crossing to the other");
        // The cap is the short way round: about 18.5 of 62.8.
        let cap = 10.0 * 2.0 * (8.0f64 / 10.0).asin();
        assert_approx_eq!(trimmed.removed.length_mm(), cap, 1e-9);
        assert_approx_eq!(kept.length_mm(), std::f64::consts::TAU * 10.0 - cap, 1e-9);
        for segment in kept.segments() {
            let arc = segment.arc().expect("an arc of the circle");
            assert_approx_eq!(arc.radius_mm, 10.0, 1e-9);
        }
        // Clicked below, the long way goes and the cap stays.
        let trimmed = trim(&circle, Point::new(0.0, -10.0), &[chord]).unwrap();
        assert_approx_eq!(trimmed.kept[0].length_mm(), cap, 1e-9);

        // Touched at one point only, a closed outline opens there.
        let square = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        let spur = line(&[(5.0, -5.0), (5.0, 0.0)]);
        let opened = trim(&square, Point::new(10.0, 5.0), &[spur]).unwrap();
        assert_eq!(opened.removed.vertices().len(), 0);
        assert_eq!(opened.kept.len(), 1);
        assert_approx_eq!(opened.kept[0].length_mm(), 40.0, 1e-9);
        assert_ends(&opened.kept[0], (5.0, 0.0), (5.0, 0.0));
    }

    /// With no crossing either side the piece is the whole outline, and
    /// nothing is trimmed.
    #[test]
    fn nothing_crosses_is_reported() {
        let across = line(&[(0.0, 10.0), (40.0, 10.0)]);
        let apart = line(&[(0.0, 20.0), (40.0, 20.0)]);
        assert_eq!(trim(&across, Point::new(20.0, 10.0), &[apart]), None);
        assert_eq!(trim(&across, Point::new(20.0, 10.0), &[]), None);
        // An outline that only meets another at its own end is not cut
        // there.
        let butting = line(&[(40.0, 0.0), (40.0, 20.0)]);
        assert_eq!(trim(&across, Point::new(20.0, 10.0), &[butting]), None);
    }

    /// An outline that crosses itself is trimmed at its own crossing.
    #[test]
    fn a_loop_is_trimmed_at_its_own_crossing() {
        // A figure like a ribbon: out, up, back across the first leg.
        let ribbon = line(&[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, -10.0),
        ]);
        let trimmed = trim(&ribbon, Point::new(20.0, 5.0), &[]).unwrap();
        // The loop between the two passes through (10, 0) goes.
        assert_ends(&trimmed.removed, (10.0, 0.0), (10.0, 0.0));
        assert_eq!(trimmed.kept.len(), 2);
        assert_ends(&trimmed.kept[0], (0.0, 0.0), (10.0, 0.0));
        assert_ends(&trimmed.kept[1], (10.0, 0.0), (10.0, -10.0));
    }
}
