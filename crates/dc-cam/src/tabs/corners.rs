// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tabs off corners (#285; `docs/specs/phase-2/design.md`, "Phase 2
//! addendum", A9; functional spec capability 35).
//!
//! A tab whose stretch (its width plus the bit's diameter, centred on it)
//! lies where the bit's path turns sharply holds the part at a corner,
//! where it is weakest and hardest to clean up. A position along the path
//! is *on a corner* when some window one bit diameter long, inside the
//! tab's stretch widened by half a diameter each side, turns at least
//! [`TAB_CORNER_TURN_DEG`]. The turn of a stretch of path is the sum of
//! the absolute tangent turns in it: each vertex's turn plus each arc's
//! sweep. A circle of radius `R` turns `D / R` per window, under 30° for
//! `R > 1.91 D`, so a large circle has no corner.
//!
//! The search is exact, not sampled. The turn of the window from `a` to
//! `a + D` is piecewise linear in `a` between the positions where either
//! end meets a vertex or an arc end, so the windows that turn at least the
//! limit form intervals found by solving each linear piece. A centre `c`
//! is on a corner when such a window lies in `[c − h, c + h]`, `h` half
//! the widened stretch: the corner windows' centres grown by half the
//! stretch. Each tab on a corner, in path order, moves to the nearest
//! position (forward first on a tie) that is off every corner, whose
//! stretch does not touch another tab's (as already placed), and that is at
//! most half the gap to its neighbour that way, measured before any tab
//! moved; on an open path it never passes an end. One that has no such
//! position stays where it is.
//!
//! VCarve documents no corner rule for its tabs; the 30° per diameter rule
//! is DreamCarve's. Fusion's 2D Contour "Segment" tab positioning puts tabs
//! between the points of the contour, which keeps them off its corners
//! (<https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/MFG-REF-2D-CONTOUR-TABS.htm>).
//! PureCutCNC places tabs by hand and has no rule to port.

use crate::warning::TabOnCornerReason as Stuck;
use dc_geometry::{EPS_MM, Polyline};

/// The least turn, degrees, within one bit diameter of path that makes a
/// corner a tab moves off.
pub const TAB_CORNER_TURN_DEG: f64 = 30.0;

/// How far past the edge of a corner, or of another tab's stretch, a
/// moved tab is put, mm: enough that it is off it, and that its stretch
/// does not touch the other's (spans merge within [`EPS_MM`]).
const CLEAR_MM: f64 = 1e-5;

/// Turns below this, radians, are none: the joins of a smooth path.
const TURN_EPS_RAD: f64 = 1e-9;

/// Lengths along the path closer than this, mm, are one: break points,
/// interval ends and ties.
const LENGTH_EPS_MM: f64 = 1e-9;

/// One vertex of a path where it turns.
#[derive(Debug, Clone, Copy)]
struct VertexTurn {
    /// Where it is along the path, mm.
    at_mm: f64,
    /// How far the path turns there, radians, at least 0.
    turn_rad: f64,
}

/// One arc of a path, whose sweep counts spread evenly along it.
#[derive(Debug, Clone, Copy)]
struct ArcTurn {
    /// Where it starts along the path, mm.
    start_mm: f64,
    /// Where it ends along the path, mm.
    end_mm: f64,
    /// Its sweep, radians, at least 0.
    sweep_rad: f64,
}

/// How one path turns along its length.
#[derive(Debug, Clone)]
struct Turns {
    vertices: Vec<VertexTurn>,
    arcs: Vec<ArcTurn>,
    length_mm: f64,
    closed: bool,
}

/// One segment of a path longer than [`EPS_MM`], for [`Turns::of`].
#[derive(Debug, Clone, Copy)]
struct Live {
    at_mm: f64,
    length_mm: f64,
    start: dc_geometry::Vec2,
    end: dc_geometry::Vec2,
    sweep_rad: f64,
}

impl Turns {
    /// The turns of `path`, walking only segments longer than [`EPS_MM`],
    /// as [`Polyline::point_at`] does. A closed path turns at its start
    /// too; an open one only between its segments.
    fn of(path: &Polyline) -> Self {
        let mut live: Vec<Live> = Vec::new();
        let mut walked_mm = 0.0;
        for segment in path.segments() {
            let length_mm = segment.length_mm();
            if length_mm <= EPS_MM {
                continue;
            }
            let (Some((_, start)), Some((_, end))) =
                (segment.point_at(0.0), segment.point_at(length_mm))
            else {
                continue;
            };
            live.push(Live {
                at_mm: walked_mm,
                length_mm,
                start,
                end,
                sweep_rad: segment.arc().map_or(0.0, |a| a.sweep_rad.abs()),
            });
            walked_mm += length_mm;
        }
        let closed = path.is_closed();
        let mut vertices = Vec::new();
        for (k, segment) in live.iter().enumerate() {
            let incoming = match k.checked_sub(1) {
                Some(j) => live.get(j),
                None if closed => live.last(),
                None => None,
            };
            if let Some(before) = incoming {
                let turn_rad = before
                    .end
                    .cross(segment.start)
                    .atan2(before.end.dot(segment.start))
                    .abs();
                if turn_rad > TURN_EPS_RAD {
                    vertices.push(VertexTurn {
                        at_mm: segment.at_mm,
                        turn_rad,
                    });
                }
            }
        }
        let arcs = live
            .iter()
            .filter(|s| s.sweep_rad > TURN_EPS_RAD)
            .map(|s| ArcTurn {
                start_mm: s.at_mm,
                end_mm: s.at_mm + s.length_mm,
                sweep_rad: s.sweep_rad,
            })
            .collect();
        Self {
            vertices,
            arcs,
            length_mm: walked_mm,
            closed,
        }
    }

    /// The shifts, mm, the features of the path repeat at for windows that
    /// start in `[0, L)` and are at most `L` long: once on an open path,
    /// twice round a closed one.
    fn shifts_mm(&self) -> Vec<f64> {
        if self.closed {
            vec![0.0, self.length_mm]
        } else {
            vec![0.0]
        }
    }

    /// The turn, radians, of the vertices from `from_mm` to `to_mm` along
    /// the path, ends included.
    fn vertex_turn_rad(&self, from_mm: f64, to_mm: f64) -> f64 {
        let mut total = 0.0;
        for shift_mm in self.shifts_mm() {
            for v in &self.vertices {
                let x = v.at_mm + shift_mm;
                if x >= from_mm - LENGTH_EPS_MM && x <= to_mm + LENGTH_EPS_MM {
                    total += v.turn_rad;
                }
            }
        }
        total
    }

    /// The turn, radians, of the arcs from `from_mm` to `to_mm` along the
    /// path.
    fn arc_turn_rad(&self, from_mm: f64, to_mm: f64) -> f64 {
        let mut total = 0.0;
        for shift_mm in self.shifts_mm() {
            for arc in &self.arcs {
                let (s, e) = (arc.start_mm + shift_mm, arc.end_mm + shift_mm);
                let overlap_mm = (to_mm.min(e) - from_mm.max(s)).max(0.0);
                total += arc.sweep_rad * overlap_mm / (e - s);
            }
        }
        total
    }

    /// The window starts, mm along the path, in `[0, L)` round a closed
    /// path and in `[−window, L]` along an open one, whose window
    /// `window_mm` long turns at least `limit_rad`, as closed intervals in
    /// order.
    fn corner_windows(&self, window_mm: f64, limit_rad: f64) -> Vec<(f64, f64)> {
        let l = self.length_mm;
        let (lo, hi) = if self.closed {
            (0.0, l)
        } else {
            (-window_mm, l)
        };
        let mut breaks = vec![lo, hi];
        for shift_mm in self.shifts_mm() {
            let points = self
                .vertices
                .iter()
                .map(|v| v.at_mm)
                .chain(self.arcs.iter().flat_map(|a| [a.start_mm, a.end_mm]));
            for x in points {
                for b in [x + shift_mm, x + shift_mm - window_mm] {
                    if b > lo && b < hi {
                        breaks.push(b);
                    }
                }
            }
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|b, a| *b - *a <= LENGTH_EPS_MM);
        let limit = limit_rad - TURN_EPS_RAD;
        let turn =
            |a: f64| self.vertex_turn_rad(a, a + window_mm) + self.arc_turn_rad(a, a + window_mm);
        let mut out: Vec<(f64, f64)> = Vec::new();
        let mut push = |from: f64, to: f64| match out.last_mut() {
            Some(last) if from <= last.1 + LENGTH_EPS_MM => last.1 = last.1.max(to),
            _ => out.push((from, to)),
        };
        for pair in breaks.windows(2) {
            let &[p, q] = pair else { continue };
            if turn(p) >= limit {
                push(p, p);
            }
            // Between breaks no vertex enters or leaves the window, and the
            // arcs' share of it changes linearly.
            let mid = (p + q) / 2.0;
            let vertices = self.vertex_turn_rad(mid, mid + window_mm);
            let (fp, fq) = (
                vertices + self.arc_turn_rad(p, p + window_mm),
                vertices + self.arc_turn_rad(q, q + window_mm),
            );
            match (fp >= limit, fq >= limit) {
                (true, true) => push(p, q),
                (true, false) => push(p, p + (q - p) * (fp - limit) / (fp - fq)),
                (false, true) => push(q - (q - p) * (fq - limit) / (fq - fp), q),
                (false, false) => {}
            }
        }
        if let Some(&last) = breaks.last()
            && turn(last) >= limit
        {
            push(last, last);
        }
        out
    }
}

/// Where along one path a tab of one stretch may not be centred: on a
/// corner ([`Corners::of`]).
#[derive(Debug, Clone)]
pub(super) struct Corners {
    /// The centres on a corner, mm along the path, closed intervals in
    /// order; on a closed path they may run past either end, and repeat
    /// round it.
    intervals: Vec<(f64, f64)>,
    length_mm: f64,
    closed: bool,
    /// Every centre is on a corner: the window is as long as a closed
    /// path.
    everywhere: bool,
}

impl Corners {
    /// The corners of `path` (as tabs are measured along it) for a bit
    /// `diameter_mm` across and tabs whose stretch is `stretch_mm` long:
    /// the centres from which some window one diameter long, within the
    /// stretch widened by half a diameter each side, turns at least
    /// [`TAB_CORNER_TURN_DEG`].
    pub(super) fn of(path: &Polyline, diameter_mm: f64, stretch_mm: f64) -> Self {
        let turns = Turns::of(path);
        let l = turns.length_mm;
        let everywhere = turns.closed && diameter_mm >= l;
        let intervals = if everywhere || l <= EPS_MM {
            Vec::new()
        } else {
            let half_mm = stretch_mm / 2.0;
            turns
                .corner_windows(diameter_mm, TAB_CORNER_TURN_DEG.to_radians())
                .into_iter()
                .map(|(a0, a1)| {
                    (
                        a0 + diameter_mm / 2.0 - half_mm,
                        a1 + diameter_mm / 2.0 + half_mm,
                    )
                })
                .collect()
        };
        Self {
            intervals,
            length_mm: l,
            closed: turns.closed,
            everywhere,
        }
    }

    /// The corner intervals near `around_mm`, unwrapped round a closed
    /// path so that they cover every position within one path length of
    /// it.
    fn near(&self, around_mm: f64) -> Vec<(f64, f64)> {
        if !self.closed {
            return self.intervals.clone();
        }
        let l = self.length_mm;
        let k0 = (around_mm / l).floor();
        (-2..=2)
            .map(|k| (k0 + f64::from(k)) * l)
            .flat_map(|shift| {
                self.intervals
                    .iter()
                    .map(move |&(a, b)| (a + shift, b + shift))
            })
            .collect()
    }

    /// Whether a tab centred `centre_mm` along the path is on a corner.
    pub(super) fn contains(&self, centre_mm: f64) -> bool {
        self.everywhere
            || self
                .near(centre_mm)
                .iter()
                .any(|&(a, b)| centre_mm >= a - LENGTH_EPS_MM && centre_mm <= b + LENGTH_EPS_MM)
    }

    /// Whether a tab with a stretch `stretch_mm` long has a place off
    /// every corner anywhere on the path, its stretch within an open
    /// path's ends.
    fn has_room(&self, stretch_mm: f64) -> bool {
        if self.everywhere {
            return false;
        }
        let range = if self.closed {
            (0.0, self.length_mm)
        } else {
            (stretch_mm / 2.0, self.length_mm - stretch_mm / 2.0)
        };
        range.0 <= range.1 && nearest_free(range.0, range, &self.near(range.0)).is_some()
    }
}

/// Tabs centred at `centres_mm` (in placement order, along the path the
/// corners were found on) with stretches `stretch_mm` long, each one on a
/// corner moved off it where it can be ([`Corners`], the module's rule):
/// the new centres in the same order, and the index of each tab left on a
/// corner with why. A tab moved on an open path keeps its whole stretch
/// within the path's ends; one placed nearer an end than half its stretch
/// and off every corner stays, its stretch cut short there as before.
pub(super) fn avoid(
    centres_mm: &[f64],
    corners: &Corners,
    stretch_mm: f64,
) -> (Vec<f64>, Vec<(usize, Stuck)>) {
    let n = centres_mm.len();
    let l = corners.length_mm;
    let mut placed = centres_mm.to_vec();
    let mut blocked = Vec::new();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        centres_mm
            .get(a)
            .zip(centres_mm.get(b))
            .map_or(std::cmp::Ordering::Equal, |(x, y)| x.total_cmp(y))
    });
    let room = corners.has_room(stretch_mm);
    for (rank, &i) in order.iter().enumerate() {
        let Some(&c) = centres_mm.get(i) else {
            continue;
        };
        if !corners.contains(c) {
            continue;
        }
        if !room {
            blocked.push((i, Stuck::Everywhere));
            continue;
        }
        let original = |r: usize| order.get(r).and_then(|&j| centres_mm.get(j)).copied();
        // Half the gap to each neighbour, as placed before any tab moved;
        // on an open path, not so far that the stretch passes an end.
        let range = if corners.closed {
            let (back_mm, ahead_mm) = if n == 1 {
                (l / 2.0, l / 2.0)
            } else {
                let prev = original((rank + n - 1) % n).unwrap_or(c);
                let next = original((rank + 1) % n).unwrap_or(c);
                (
                    (c - prev).rem_euclid(l) / 2.0,
                    (next - c).rem_euclid(l) / 2.0,
                )
            };
            (c - back_mm, c + ahead_mm)
        } else {
            let back_mm = rank
                .checked_sub(1)
                .and_then(original)
                .map_or(c, |prev| (c - prev) / 2.0);
            let ahead_mm = original(rank + 1).map_or(l - c, |next| (next - c) / 2.0);
            let half_mm = stretch_mm / 2.0;
            ((c - back_mm).max(half_mm), (c + ahead_mm).min(l - half_mm))
        };
        let mut blocks = corners.near(c);
        // Every other tab's stretch, as placed so far, round the path.
        for (j, &other) in placed.iter().enumerate() {
            if j == i {
                continue;
            }
            let shifts: Vec<f64> = if corners.closed {
                let k0 = ((c - other) / l).round();
                (-1..=1).map(|k| (k0 + f64::from(k)) * l).collect()
            } else {
                vec![0.0]
            };
            blocks.extend(
                shifts
                    .into_iter()
                    .map(|s| (other + s - stretch_mm, other + s + stretch_mm)),
            );
        }
        let to = (range.0 <= range.1)
            .then(|| nearest_free(c, range, &blocks))
            .flatten();
        match to {
            Some(to) => {
                if let Some(slot) = placed.get_mut(i) {
                    *slot = if corners.closed { to.rem_euclid(l) } else { to };
                }
            }
            None => blocked.push((i, Stuck::NoRoom)),
        }
    }
    (placed, blocked)
}

/// The point of `range` nearest `c` that lies [`CLEAR_MM`] or more outside
/// every one of `blocks` (closed intervals), forward first on a tie; the
/// ends of `range` themselves are allowed. `None` when there is none.
fn nearest_free(c: f64, range: (f64, f64), blocks: &[(f64, f64)]) -> Option<f64> {
    let (lo, hi) = range;
    let mut blocks: Vec<(f64, f64)> = blocks
        .iter()
        .map(|&(a, b)| (a - CLEAR_MM, b + CLEAR_MM))
        .filter(|&(a, b)| b >= lo && a <= hi)
        .collect();
    blocks.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The free stretches of the range between the blocks.
    let mut free: Vec<(f64, f64)> = Vec::new();
    let mut from = lo;
    for (a, b) in blocks {
        if a > from {
            free.push((from, a.min(hi)));
        }
        from = from.max(b);
    }
    if from <= hi {
        free.push((from, hi));
    }
    let mut best: Option<f64> = None;
    for (a, b) in free.into_iter().filter(|(a, b)| b >= a) {
        let candidate = if c < a {
            a
        } else if c > b {
            b
        } else {
            c
        };
        let better = best.is_none_or(|x| {
            let (dc, dx) = ((candidate - c).abs(), (x - c).abs());
            dc < dx - LENGTH_EPS_MM || ((dc - dx).abs() <= LENGTH_EPS_MM && candidate > x)
        });
        if better {
            best = Some(candidate);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{Point, Vertex, assert_approx_eq};

    const D: f64 = 6.35;
    /// A 2 mm tab's stretch with the 6.35 mm bit.
    const STRETCH_MM: f64 = 2.0 + D;

    fn square() -> Polyline {
        Polyline::rect(Point::new(0.0, 0.0), 50.0, 50.0)
    }

    /// A sharp 90° corner at 50 mm is a corner for every centre within
    /// half a diameter plus half the stretch of it, both ways round a
    /// closed path, and nowhere else.
    #[test]
    fn sharp_corner_spans_half_a_diameter_and_half_a_stretch() {
        let corners = Corners::of(&square(), D, STRETCH_MM);
        let reach = D / 2.0 + STRETCH_MM / 2.0;
        for corner in [0.0, 50.0, 100.0, 150.0, 200.0] {
            assert!(corners.contains(corner));
            assert!(corners.contains(corner + reach - 1e-6));
            assert!(corners.contains(corner - reach + 1e-6));
            assert!(!corners.contains(corner + reach + 1e-6));
            assert!(!corners.contains(corner - reach - 1e-6));
        }
        assert!(!corners.contains(25.0));
    }

    /// An arc counts its sweep spread along it: a 90° arc of radius
    /// 3.175 mm (5 mm long) is a corner once a diameter window holds a
    /// third of it, and a circle of radius 2 D is no corner anywhere.
    #[test]
    fn arcs_count_their_share_of_the_sweep() {
        let corner = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::new(50.0, 0.0, (std::f64::consts::FRAC_PI_2 / 4.0).tan()),
                Vertex::line(50.0 + 3.175, 3.175),
                Vertex::line(50.0 + 3.175, 50.0),
            ],
            false,
        );
        let arc_mm = std::f64::consts::FRAC_PI_2 * 3.175;
        let corners = Corners::of(&corner, D, STRETCH_MM);
        // A window reaches a third of the arc from a centre this far
        // before the arc's start.
        let edge = 50.0 - (D / 2.0 + STRETCH_MM / 2.0) + arc_mm / 3.0;
        assert!(corners.contains(edge + 1e-6));
        assert!(!corners.contains(edge - 1e-6));
        let circle = Polyline::circle(Point::new(0.0, 0.0), 2.0 * D);
        let round = Corners::of(&circle, D, STRETCH_MM);
        assert!((0..100).all(|k| !round.contains(f64::from(k))));
        let tight = Corners::of(
            &Polyline::circle(Point::new(0.0, 0.0), 1.5 * D),
            D,
            STRETCH_MM,
        );
        assert!((0..50).all(|k| tight.contains(f64::from(k))));
    }

    /// A tab moves to the nearest place off the corner, forward on a tie;
    /// at most half the gap to each neighbour; never onto another tab;
    /// never past an open path's end; and stays when it cannot.
    #[test]
    fn avoid_moves_to_the_nearest_allowed_place() {
        let corners = Corners::of(&square(), D, STRETCH_MM);
        let off = D / 2.0 + STRETCH_MM / 2.0;
        let (moved, blocked) = avoid(&[50.0], &corners, STRETCH_MM);
        assert!(blocked.is_empty());
        assert_approx_eq!(moved[0], 50.0 + off, 1e-4);
        let (moved, _) = avoid(&[48.0], &corners, STRETCH_MM);
        assert_approx_eq!(moved[0], 50.0 - off, 1e-4);
        // Its neighbour at 40 keeps it from going back, so it goes ahead.
        let (moved, _) = avoid(&[40.0, 49.0, 80.0], &corners, STRETCH_MM);
        assert_approx_eq!(moved[1], 50.0 + off, 1e-4);
        assert_approx_eq!(moved[0], 40.0, 1e-12);
        assert_approx_eq!(moved[2], 80.0, 1e-12);
        // Neighbours too close both ways: it stays.
        let (moved, blocked) = avoid(&[40.0, 49.0, 60.0], &corners, STRETCH_MM);
        assert_eq!(blocked, vec![(1, Stuck::NoRoom)]);
        assert_approx_eq!(moved[1], 49.0, 1e-12);
        // Two overlapping tabs on one corner part, one each way.
        let (moved, blocked) = avoid(&[46.0, 54.0], &corners, STRETCH_MM);
        assert!(blocked.is_empty(), "{moved:?}");
        assert_approx_eq!(moved[0], 50.0 - off, 1e-4);
        assert_approx_eq!(moved[1], 50.0 + off, 1e-4);
        // An open path's end.
        let hook = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(50.0, 0.0),
                Vertex::line(50.0, 6.0),
            ],
            false,
        );
        let corners = Corners::of(&hook, D, STRETCH_MM);
        let (moved, _) = avoid(&[53.0], &corners, STRETCH_MM);
        assert_approx_eq!(moved[0], 50.0 - off, 1e-4);
        // Nor so near one that its stretch would run past it: a tab at 9
        // on a path that turns at 10 would come off soonest at 2.65, its
        // stretch past the start, so it goes on to 17.35.
        let start = Polyline::new(
            vec![
                Vertex::line(0.0, 10.0),
                Vertex::line(0.0, 0.0),
                Vertex::line(50.0, 0.0),
            ],
            false,
        );
        let corners = Corners::of(&start, D, STRETCH_MM);
        let (moved, blocked) = avoid(&[9.0], &corners, STRETCH_MM);
        assert!(blocked.is_empty());
        assert_approx_eq!(moved[0], 10.0 + off, 1e-4);
        // A small circle is a corner everywhere.
        let small = Corners::of(&Polyline::circle(Point::new(0.0, 0.0), D), D, STRETCH_MM);
        let (_, blocked) = avoid(&[3.0], &small, STRETCH_MM);
        assert_eq!(blocked, vec![(0, Stuck::Everywhere)]);
        // The placement order is kept.
        let corners = Corners::of(&square(), D, STRETCH_MM);
        let (moved, _) = avoid(&[150.0, 25.0], &corners, STRETCH_MM);
        assert_approx_eq!(moved[0], 150.0 + off, 1e-4);
        assert_approx_eq!(moved[1], 25.0, 1e-12);
    }
}
