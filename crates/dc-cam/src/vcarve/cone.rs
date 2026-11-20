// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The walls of a carve and the exact cone test on them
//! (`docs/specs/phase-3/design.md`, "Safety pass", the S17 bullet, and
//! "V-Carve generator", steps 5 and 10; security review H2 and M1).
//!
//! A V-bit whose tip is at depth `d` below the start plane cuts a disc of
//! radius `d · slope` there, and it cuts nothing below the carve's final V
//! surface exactly when that disc stays inside the carved region: the
//! final surface at a point `q` is `dist(q, wall) / slope` deep, and the
//! cone at `p` reaches `q` at `d − |q − p| / slope`, which the triangle
//! inequality keeps above it everywhere when `d · slope ≤ dist(p, wall)`.
//!
//! A tapered ball nose (slice 27c, #679) cuts nested discs too, `w(d)`
//! across at depth `d` ([`BitProfile`]), so the same holds with `w(d)` in
//! place of `d · slope`.
//!
//! Along a straight move the least room to each straight wall segment is
//! found exactly, with no sampling step, by the shared
//! [`dc_geometry::profile::least_room`] (#678): the V-bit's closed forms,
//! the virtual apex on a TBN's cone, the ball's polynomial stationary
//! points. Arcs are flattened to chords within the caller's tolerance
//! first, as `dc_geometry::medial` flattens them, so the walls here are
//! the walls the medial axis measured its clearances to.

use dc_geometry::medial::point_in_loops;
use dc_geometry::profile::{BitProfile, WallPiece, least_room};
use dc_geometry::{
    EPS_MM, Point, Point3, Polyline, Region, Segment, Vec2, Vertex, segment_distance_mm,
};

/// Cells a side of the wall grid at most.
const MAX_CELLS: f64 = 128.0;

/// Pieces [`Walls::widening_clearance_holds`] holds a path to a drawn arc
/// in.
const WIDENING_PIECES: u32 = 16;

/// The least side of a wall grid cell, mm: walls that span nothing, or
/// none at all, still get a grid of cells with a size.
const MIN_CELL_MM: f64 = 1e-3;

/// A segment whose squared length, mm², is no more than this is a point:
/// the nearest point on it is its start.
const POINT_LENGTH_SQ_MM2: f64 = 1e-18;

/// Two segments whose directions' cross product, mm², is no more than
/// this are parallel for [`segment_distance`]: they cross nowhere, and
/// the distance is between their ends.
const PARALLEL_CROSS_MM2: f64 = 1e-18;

/// A path and a wall whose directions' cross product, mm², is no more
/// than this are parallel for [`Walls::excursion`]: the wall is not a
/// crossing of the path, only its ends within the slack are.
const EXCURSION_PARALLEL_CROSS_MM2: f64 = 1e-12;

/// How far past a wall's ends, as a fraction of its length, a path's
/// crossing still counts as on it: a path through a vertex crosses both
/// walls that meet there.
const WALL_END_SLACK: f64 = 1e-9;

/// Two cuts of a path closer than this fraction of its length have no
/// stretch between them for [`Walls::excursion`] to test.
const MIN_STRETCH: f64 = 1e-12;

/// A wall point bounds a disc tangent at `w` only when it is ahead of the
/// tangent by more than this, relative to its distance from `w` plus
/// 1 mm ([`Walls::ball_radius_mm`]).
const AHEAD_OF_TANGENT: f64 = 1e-9;

/// A divisor no larger than this is zero: the root it would give is left
/// out ([`Walls::ball_radius_mm`]).
const MIN_DIVISOR: f64 = 1e-300;

/// Points of a loop closer than this, mm, are one, as the medial axis
/// cleans its loops ([`clean_loop`]).
const SAME_POINT_MM: f64 = 1e-9;

/// The straight wall segments of a set of regions, bucketed in a uniform
/// grid, with the flattened loops for the even-odd inside test.
#[derive(Debug, Clone)]
pub(crate) struct Walls {
    /// Every wall, loop after loop and round each loop.
    walls: Vec<Wall>,
    /// The tolerance the arcs were flattened within, mm.
    tolerance_mm: f64,
    loops: Vec<Polyline>,
    origin: Point,
    cell_mm: f64,
    columns: usize,
    rows: usize,
    /// The grid's cells, row after row, `columns` to a row: each holds the
    /// walls whose box crosses it, themselves and not their places among
    /// `walls`, so a search of the grid looks nothing up and can miss
    /// nothing (code review finding 1.2).
    buckets: Vec<Vec<Wall>>,
}

/// One straight wall segment, with what a search needs of it.
#[derive(Debug, Clone, Copy)]
struct Wall {
    /// Where it starts, mm.
    a: Point,
    /// Where it ends, mm.
    b: Point,
    /// The place among the walls of its loop's first wall.
    first: usize,
    /// Its place in its loop: wall `k` of a loop runs from the loop's
    /// vertex `k` to its vertex `k + 1`.
    place: usize,
    /// How many walls its loop has.
    len: usize,
    /// The place of the wall as drawn that it is a chord of, among the
    /// regions' drawn walls.
    source: usize,
    /// That wall as drawn, an arc as an arc (#481).
    drawn: Segment,
}

impl Wall {
    /// Its place among the walls.
    fn index(&self) -> usize {
        self.first + self.place
    }
}

/// Where a straight tip path leaves the regions ([`Walls::excursion`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Excursion {
    /// The point of the path farthest outside, as a fraction of the way
    /// from its start.
    pub t: f64,
    /// How far outside the walls it is there, mm.
    pub outside_mm: f64,
    /// The wall vertex the path cuts across, when it leaves and comes back
    /// through one loop ([`Walls::cut_corner`]).
    pub corner: Option<Corner>,
}

/// A wall vertex a tip path cuts across ([`Walls::cut_corner`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Corner {
    /// The vertex, mm.
    pub p: Point,
    /// The unit vector into the region that bisects the vertex.
    pub bisector: Vec2,
}

impl Walls {
    /// The outers and holes of `regions`, arcs flattened to chords within
    /// `tolerance_mm` as the medial axis flattens them, consecutive
    /// duplicates dropped.
    pub(crate) fn new(regions: &[Region], tolerance_mm: f64) -> Self {
        let mut walls: Vec<Wall> = Vec::new();
        let mut loops = Vec::new();
        let mut drawn_count = 0usize;
        for region in regions {
            for contour in std::iter::once(region.outer()).chain(region.holes()) {
                // Each drawn segment flattened on its own, which gives the
                // contour's flattened points (`with_arcs_flattened` takes
                // each segment's start and its arc's inner points), each
                // with the drawn segment its chord on stands for.
                let mut flat = Vec::new();
                for segment in contour.polyline().segments() {
                    let source = drawn_count;
                    drawn_count += 1;
                    let alone = Polyline::new(
                        vec![
                            Vertex::new(segment.start.x, segment.start.y, segment.bulge),
                            Vertex::line(segment.end.x, segment.end.y),
                        ],
                        false,
                    )
                    .with_arcs_flattened(tolerance_mm);
                    let points = alone.vertices();
                    flat.extend(
                        points
                            .iter()
                            .take(points.len().saturating_sub(1))
                            .map(|v| (v.point(), (source, segment))),
                    );
                }
                let points = clean_loop(flat);
                if points.len() < 2 {
                    continue;
                }
                let first = walls.len();
                let next = points.iter().cycle().skip(1);
                for (place, (&(a, (source, drawn)), &(b, _))) in points.iter().zip(next).enumerate()
                {
                    walls.push(Wall {
                        a,
                        b,
                        first,
                        place,
                        len: points.len(),
                        source,
                        drawn,
                    });
                }
                let points: Vec<Point> = points.into_iter().map(|(p, _)| p).collect();
                loops.push(Polyline::new(
                    points.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
                    true,
                ));
            }
        }
        let (mut min, mut max) = (
            Point::new(f64::INFINITY, f64::INFINITY),
            Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for wall in &walls {
            for p in [wall.a, wall.b] {
                min = Point::new(min.x.min(p.x), min.y.min(p.y));
                max = Point::new(max.x.max(p.x), max.y.max(p.y));
            }
        }
        // With no walls the grid is one cell, which may be anywhere.
        if walls.is_empty() {
            min = Point::new(0.0, 0.0);
            max = min;
        }
        let extent_mm = (max.x - min.x).max(max.y - min.y);
        let cell_mm = (extent_mm / MAX_CELLS).max(MIN_CELL_MM);
        let columns = count_of(((max.x - min.x) / cell_mm).floor()) + 1;
        let rows = count_of(((max.y - min.y) / cell_mm).floor()) + 1;
        let mut grid = Self {
            walls,
            tolerance_mm,
            loops,
            origin: min,
            cell_mm,
            columns,
            rows,
            buckets: vec![Vec::new(); columns * rows],
        };
        // Each wall goes into the cells its box crosses: from its low
        // corner's cell to its high corner's.
        let boxes: Vec<(Wall, [(usize, usize); 2])> = grid
            .walls
            .iter()
            .map(|&wall| {
                let (a, b) = (wall.a, wall.b);
                let low = grid.cell_of(Point::new(a.x.min(b.x), a.y.min(b.y)));
                let high = grid.cell_of(Point::new(a.x.max(b.x), a.y.max(b.y)));
                (wall, [low, high])
            })
            .collect();
        for (wall, [(c0, r0), (c1, r1)]) in boxes {
            let rows = grid.buckets.chunks_mut(columns);
            for row in rows.skip(r0).take(span(r0, r1)) {
                for bucket in row.iter_mut().skip(c0).take(span(c0, c1)) {
                    bucket.push(wall);
                }
            }
        }
        grid
    }

    /// How far a tip may stand outside these walls, mm: the flatten
    /// tolerance they were built at (a chord near a concave arc lies up
    /// to that far beyond the true wall) plus [`EPS_MM`]. The generator's
    /// own cone test ([`crate::vcarve::cone_in_material`]) allows this
    /// much, so fitting holds a chain's stretches to it too (#463).
    pub(crate) fn slack_mm(&self) -> f64 {
        self.tolerance_mm.max(0.0) + EPS_MM
    }

    /// The grid cell holding `p`, clamped to the grid.
    fn cell_of(&self, p: Point) -> (usize, usize) {
        (
            clamped_index((p.x - self.origin.x) / self.cell_mm, self.columns),
            clamped_index((p.y - self.origin.y) / self.cell_mm, self.rows),
        )
    }

    /// The walls that may come within `margin_mm` of the box `lo`–`hi`,
    /// in their order among the walls, each once.
    fn near(&self, lo: Point, hi: Point, margin_mm: f64) -> Vec<&Wall> {
        let (c0, r0) = self.cell_of(Point::new(lo.x - margin_mm, lo.y - margin_mm));
        let (c1, r1) = self.cell_of(Point::new(hi.x + margin_mm, hi.y + margin_mm));
        let rows = self.buckets.chunks(self.columns);
        let mut out: Vec<&Wall> = rows
            .skip(r0)
            .take(span(r0, r1))
            .flat_map(|row| row.iter().skip(c0).take(span(c0, c1)))
            .flatten()
            .collect();
        out.sort_unstable_by_key(|wall| wall.index());
        out.dedup_by_key(|wall| wall.index());
        out
    }

    /// The arcs as drawn that the walls `near` are chords of, in drawn
    /// order, each once.
    fn arcs(near: &[&Wall]) -> Vec<Segment> {
        let mut arcs: Vec<(usize, Segment)> = near
            .iter()
            .filter(|wall| wall.drawn.bulge != 0.0)
            .map(|wall| (wall.source, wall.drawn))
            .collect();
        arcs.sort_unstable_by_key(|&(source, _)| source);
        arcs.dedup_by_key(|&mut (source, _)| source);
        arcs.into_iter().map(|(_, arc)| arc).collect()
    }

    /// The exact distance, mm, from `p` to the nearest wall; infinite with
    /// no walls.
    pub(crate) fn clearance_mm(&self, p: Point) -> f64 {
        self.walls
            .iter()
            .map(|wall| point_segment(p, wall.a, wall.b))
            .fold(f64::INFINITY, f64::min)
    }

    /// The wall point nearest `p` and its distance, mm; `None` with no
    /// walls. The grid is searched outward until the nearest point found
    /// is no farther than the cells searched reach, so the answer is exact.
    pub(crate) fn nearest(&self, p: Point) -> Option<(f64, Point)> {
        self.nearest_segment(p).map(|(d, q, _)| (d, q))
    }

    /// The unit normal, into the regions, of the wall nearest `p` (#554):
    /// the regions lie left of every wall, outers counter-clockwise and
    /// holes clockwise (ADR-0001). Where `p` is on a wall, its nearest
    /// point is `p` itself, which gives no direction. At a vertex either
    /// wall's.
    pub(crate) fn inward_normal(&self, p: Point) -> Option<Vec2> {
        let (_, _, wall) = self.nearest_segment(p)?;
        let along: Vec2 = wall.b - wall.a;
        Vec2::new(-along.y, along.x).normalized()
    }

    /// The middle of every wall segment that may come within `margin_mm`
    /// of the box of `a` and `b`, with its unit normal into the regions
    /// (#705), as [`Walls::inward_normal`] reads it; a segment with no
    /// length has no normal and is left out.
    pub(crate) fn middles_near(&self, a: Point, b: Point, margin_mm: f64) -> Vec<(Point, Vec2)> {
        let lo = Point::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Point::new(a.x.max(b.x), a.y.max(b.y));
        self.near(lo, hi, margin_mm)
            .into_iter()
            .filter_map(|wall| {
                let along: Vec2 = wall.b - wall.a;
                let normal = Vec2::new(-along.y, along.x).normalized()?;
                Some((wall.a + along * 0.5, normal))
            })
            .collect()
    }

    /// [`Walls::nearest`] with the wall it lies on.
    fn nearest_segment(&self, p: Point) -> Option<(f64, Point, Wall)> {
        let foot = |a: Point, b: Point| {
            let ab: Vec2 = b - a;
            let length_sq = ab.dot(ab);
            let t = if length_sq > POINT_LENGTH_SQ_MM2 {
                ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            a + ab * t
        };
        let best_of = |walls: &[&Wall]| {
            walls
                .iter()
                .map(|&&wall| {
                    let q = foot(wall.a, wall.b);
                    (p.distance_to(q), q, wall)
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
        };
        let mut margin_mm = self.cell_mm;
        let span_mm = self.cell_mm * (self.columns + self.rows) as f64;
        while margin_mm <= 2.0 * span_mm {
            let best = best_of(&self.near(p, p, margin_mm));
            if best.is_some_and(|(d, ..)| d <= margin_mm) {
                return best;
            }
            margin_mm *= 2.0;
        }
        let all: Vec<&Wall> = self.walls.iter().collect();
        best_of(&all)
    }

    /// The radius, mm, of the largest disc inside the walls that touches
    /// the wall point `w` with its centre along the unit vector `u` from
    /// it, at most `cap_mm` (the ball-shrinking construction of a medial
    /// axis point: for each wall segment the least radius whose disc,
    /// tangent at `w`, reaches it, `|q − w|² / (2 (q − w)·u)` over the
    /// segment's points `q` ahead of `w`, found in closed form). The discs
    /// tangent at `w` are nested, so only the walls within the largest,
    /// `cap_mm` about `w + u · cap_mm`, can bound it.
    pub(crate) fn ball_radius_mm(&self, w: Point, u: Vec2, cap_mm: f64) -> f64 {
        let centre = w + u * cap_mm;
        let mut radius_mm = cap_mm;
        for wall in self.near(centre, centre, cap_mm + EPS_MM) {
            let d0: Vec2 = wall.a - w;
            let e: Vec2 = wall.b - wall.a;
            let ratio = |t: f64| {
                let d = Vec2::new(d0.x + e.x * t, d0.y + e.y * t);
                let ahead = 2.0 * d.dot(u);
                // A point at or behind `w`'s tangent bounds no disc.
                (ahead > AHEAD_OF_TANGENT * (1.0 + d.length())).then(|| d.dot(d) / ahead)
            };
            // The stationary points of the ratio along the segment.
            let (eu, du) = (e.dot(u), d0.dot(u));
            let qa = e.dot(e) * eu;
            let qb = 2.0 * e.dot(e) * du;
            let qc = 2.0 * d0.dot(e) * du - d0.dot(d0) * eu;
            // The roots of `qa t² + qb t + qc`, in the form that keeps
            // its precision when `qa` is tiny (a wall nearly parallel to
            // the tangent).
            let mut ts = vec![0.0, 1.0];
            let disc = qb * qb - 4.0 * qa * qc;
            if disc >= 0.0 {
                let q = -0.5 * (qb + qb.signum() * disc.sqrt());
                if q.abs() > MIN_DIVISOR {
                    ts.push(qc / q);
                }
                if qa.abs() > MIN_DIVISOR {
                    ts.push(q / qa);
                }
            }
            for t in ts {
                if (0.0..=1.0).contains(&t)
                    && let Some(r) = ratio(t)
                {
                    radius_mm = radius_mm.min(r);
                }
            }
        }
        radius_mm
    }

    /// How near the straight tip path `a`–`b` comes to a wall, mm, among
    /// the walls within `reach_mm` of it (#469): 0 where it meets one;
    /// `reach_mm` or more when none is nearer. The walls are the chords
    /// and also the arcs as drawn (#481): S17 holds the written path to
    /// the drawn outline, and beside an arc that bows into the region its
    /// chords lie up to the flatten tolerance beyond it, so a path clear of
    /// them can come within a hair of the arc and, once written, cross it.
    pub(crate) fn path_clearance_mm(&self, a: Point, b: Point, reach_mm: f64) -> f64 {
        let lo = Point::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Point::new(a.x.max(b.x), a.y.max(b.y));
        // An arc lies within the flatten tolerance of its chords, so the
        // chords that far nearer find every arc within reach.
        let near = self.near(lo, hi, reach_mm + self.tolerance_mm.max(0.0));
        let to_chords = near
            .iter()
            .map(|wall| segment_distance(a, b, wall.a, wall.b))
            .fold(reach_mm, f64::min);
        let path = Segment {
            start: a,
            end: b,
            bulge: 0.0,
        };
        Self::arcs(&near)
            .into_iter()
            .map(|arc| segment_distance_mm(path, arc))
            .fold(to_chords, f64::min)
    }

    /// Whether the straight tip path from `g` to `q` keeps, a fraction `t`
    /// of the way along, at least `t · margin_mm` from every wall, chords
    /// and arcs as drawn (#499). With `q` inside the regions, that holds
    /// exactly when the path from `g` as written to anywhere within
    /// `margin_mm` of `q` stays inside: `g` written exactly (a point of
    /// the program's grid), `q` moved by rounding. A `g` outside fails,
    /// since its path crosses a wall; one on a wall passes when the path
    /// leaves it at once.
    ///
    /// Against the chords it is the exact cone test with a cone that
    /// widens from 0 at `g` to `margin_mm` at `q`
    /// ([`Walls::least_room`]). An arc is held to it piecewise: each of
    /// [`WIDENING_PIECES`] pieces keeps from the arc the margin at the
    /// piece's far end, which asks a little more than the exact test.
    pub(crate) fn widening_clearance_holds(&self, g: Point, q: Point, margin_mm: f64) -> bool {
        let at = |z: f64, p: Point| Point3::new(p.x, p.y, z);
        if self
            .least_room(
                at(0.0, g),
                at(-margin_mm, q),
                0.0,
                &BitProfile::VBit { slope: 1.0 },
            )
            .0
            < -EPS_MM
        {
            return false;
        }
        let lo = Point::new(g.x.min(q.x), g.y.min(q.y));
        let hi = Point::new(g.x.max(q.x), g.y.max(q.y));
        let arcs = Self::arcs(&self.near(lo, hi, margin_mm + self.tolerance_mm.max(0.0)));
        let along = |t: f64| g + (q - g) * t;
        let pieces = f64::from(WIDENING_PIECES);
        arcs.into_iter().all(|arc| {
            (0..WIDENING_PIECES).all(|k| {
                let (t0, t1) = (f64::from(k) / pieces, f64::from(k + 1) / pieces);
                let piece = Segment {
                    start: along(t0),
                    end: along(t1),
                    bulge: 0.0,
                };
                segment_distance_mm(piece, arc) >= t1 * margin_mm - EPS_MM
            })
        })
    }

    /// Whether a chord of a drawn arc lies within `reach_mm` of `p` (#506):
    /// only there do chords drawn at another tolerance put the walls
    /// elsewhere.
    pub(crate) fn arc_within(&self, p: Point, reach_mm: f64) -> bool {
        self.near(p, p, reach_mm)
            .into_iter()
            .any(|wall| wall.drawn.bulge != 0.0 && point_segment(p, wall.a, wall.b) <= reach_mm)
    }

    /// Whether `p` lies inside the regions by the even-odd rule, or within
    /// `slack_mm` of a wall.
    pub(crate) fn holds(&self, p: Point, slack_mm: f64) -> bool {
        let loops: Vec<&Polyline> = self.loops.iter().collect();
        point_in_loops(p, &loops)
            || self
                .near(p, p, slack_mm)
                .into_iter()
                .any(|wall| point_segment(p, wall.a, wall.b) <= slack_mm)
    }

    /// Whether the straight tip path `a`–`b` stays inside the regions, or
    /// within `slack_mm` of them: the ends do, and so does the middle of
    /// every stretch between the points where the path meets a wall (or
    /// passes within `slack_mm` of a wall's end), so a path that leaves
    /// through a vertex is caught as well as one that crosses an edge.
    pub(crate) fn holds_path(&self, a: Point, b: Point, slack_mm: f64) -> bool {
        self.excursion(a, b, slack_mm).is_none()
    }

    /// Where the straight tip path `a`–`b` first leaves the regions by
    /// more than `slack_mm` ([`Walls::holds_path`]'s test), or `None` when
    /// it stays in (#463). An end outside is the excursion itself. A
    /// stretch outside between two crossings is read every 1/32 of its
    /// length for the point farthest from the walls, so the distance is
    /// what the path leaves by, not the 0 of the walls it crosses; and
    /// when it leaves and comes back through one loop, the vertex it cuts
    /// across is named ([`Walls::cut_corner`]).
    pub(crate) fn excursion(&self, a: Point, b: Point, slack_mm: f64) -> Option<Excursion> {
        for (t, p) in [(0.0, a), (1.0, b)] {
            if !self.holds(p, slack_mm) {
                return Some(Excursion {
                    t,
                    outside_mm: self.clearance_mm(p),
                    corner: None,
                });
            }
        }
        let ab = b - a;
        let length_sq = ab.dot(ab);
        if length_sq <= EPS_MM * EPS_MM {
            return None;
        }
        let lo = Point::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Point::new(a.x.max(b.x), a.y.max(b.y));
        // Where the path meets a wall: its crossings, each with the
        // segment crossed, and the feet of the wall ends within the slack
        // of it.
        let mut cuts: Vec<(f64, Option<&Wall>)> = vec![(0.0, None), (1.0, None)];
        for wall in self.near(lo, hi, slack_mm) {
            let (c, d) = (wall.a, wall.b);
            let cd = d - c;
            let denom = ab.cross(cd);
            if denom.abs() > EXCURSION_PARALLEL_CROSS_MM2 {
                let t = (c - a).cross(cd) / denom;
                let u = (c - a).cross(ab) / denom;
                if (-WALL_END_SLACK..=1.0 + WALL_END_SLACK).contains(&u) && (0.0..=1.0).contains(&t)
                {
                    cuts.push((t, Some(wall)));
                }
            }
            for end in [c, d] {
                if point_segment(end, a, b) <= slack_mm {
                    cuts.push((((end - a).dot(ab) / length_sq).clamp(0.0, 1.0), None));
                }
            }
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let at = |t: f64| a + ab * t;
        let mid = cuts.windows(2).find_map(|w| match w {
            [(s, _), (t, _)] if t - s > MIN_STRETCH && !self.holds(at((s + t) / 2.0), slack_mm) => {
                Some((s + t) / 2.0)
            }
            _ => None,
        })?;
        // The crossings the path leaves by and comes back in by.
        let exit = cuts
            .iter()
            .rev()
            .find_map(|&(u, seg)| seg.filter(|_| u <= mid).map(|seg| (u, seg)));
        let entry = cuts
            .iter()
            .find_map(|&(u, seg)| seg.filter(|_| u >= mid).map(|seg| (u, seg)));
        let (t0, t1) = (exit.map_or(0.0, |e| e.0), entry.map_or(1.0, |e| e.0));
        let (t, outside_mm) = (0..=32)
            .map(|k| t0 + (t1 - t0) * f64::from(k) / 32.0)
            .chain([mid])
            .map(|u| (u, self.clearance_mm(at(u))))
            .fold((mid, f64::NEG_INFINITY), |best, x| {
                if x.1 > best.1 { x } else { best }
            });
        let corner = exit
            .zip(entry)
            .and_then(|((u0, i), (u1, j))| self.cut_corner(a, b, (at(u0), i), (at(u1), j)));
        Some(Excursion {
            t,
            outside_mm,
            corner,
        })
    }

    /// The wall vertex the path from `a` to `b` cuts across where it
    /// leaves the regions at `exit` (the point and the segment crossed)
    /// and comes back at `entry` through another segment of the same loop
    /// (#463): of the loop's two ways between them, the one that closes
    /// the smaller polygon with the path (outside an outer, the bay the
    /// path crosses, not the region round it; in a hole, either side), and
    /// on it the vertex farthest from the path's line, with the unit
    /// vector that bisects the region's side of it (away from both its
    /// walls; straight out from the path's line where they run straight
    /// on). `None` when the two are on different loops or the same
    /// segment.
    fn cut_corner(
        &self,
        a: Point,
        b: Point,
        (x0, i): (Point, &Wall),
        (x1, j): (Point, &Wall),
    ) -> Option<Corner> {
        let (first, len) = (i.first, i.len);
        if first != j.first || i.place == j.place || len == 0 {
            return None;
        }
        // The loop's vertices: its wall `k` starts at vertex `k`.
        let ring: Vec<Point> = self
            .walls
            .iter()
            .skip(first)
            .take(len)
            .map(|wall| wall.a)
            .collect();
        // Wall `k` ends at vertex `k + 1`: the way on from the exit
        // visits vertices `i + 1 ..= j`, the way back `j + 1 ..= i`. Each
        // vertex comes with the one before it and the one after it round
        // the loop.
        let way = |from: usize, to: usize| -> Vec<(Point, Point, Point)> {
            let count = (to + len - from) % len;
            let round = |skip: usize| ring.iter().copied().cycle().skip(skip);
            round(from)
                .zip(round(from + 1))
                .zip(round(from + 2))
                .take(count)
                .map(|((before, p), after)| (before, p, after))
                .collect()
        };
        let area = |start: Point, way: &[(Point, Point, Point)], end: Point| {
            let ring: Vec<Point> = std::iter::once(start)
                .chain(way.iter().map(|&(_, p, _)| p))
                .chain(std::iter::once(end))
                .collect();
            let next = ring.iter().cycle().skip(1);
            ring.iter()
                .zip(next)
                .map(|(p, q)| p.x * q.y - q.x * p.y)
                .sum::<f64>()
                .abs()
        };
        let (i, j) = (i.place, j.place);
        let on = way(i, j);
        let back = way(j, i);
        let chunk = if area(x0, &on, x1) <= area(x1, &back, x0) {
            on
        } else {
            back
        };
        let ab = b - a;
        let length = ab.length();
        if length <= EPS_MM {
            return None;
        }
        let (prev, p, next) = chunk
            .into_iter()
            .max_by(|x, y| (ab.cross(x.1 - a).abs()).total_cmp(&ab.cross(y.1 - a).abs()))?;
        let foot = a + ab * ((p - a).dot(ab) / (length * length));
        let out = (p - foot).normalized()?;
        // The path cuts across `p`, so the region is on the far side of it
        // from its two walls, which run back toward the path.
        let bisector = match ((prev - p).normalized(), (next - p).normalized()) {
            (Some(u), Some(v)) => Vec2::new(-u.x - v.x, -u.y - v.y).normalized(),
            _ => None,
        }
        .filter(|w| w.dot(out) > 0.0)
        .unwrap_or(out);
        Some(Corner { p, bisector })
    }

    /// The least, along the straight move `a`–`b`, of the tip's distance
    /// to the walls less the radius of the disc `profile` cuts at the start
    /// plane `start_plane_z_mm` there, mm, with the point where it is
    /// least: `(start_plane − z) · slope` for a V-bit, `w(start_plane − z)`
    /// for a tapered ball nose. Exact for the walls within reach, by
    /// [`least_room`] against each wall segment; walls beyond it count as
    /// `2 · EPS_MM` of room, which is all a caller asks of them. A move
    /// with no XY motion is read at its lower end.
    pub(crate) fn least_room(
        &self,
        a: Point3,
        b: Point3,
        start_plane_z_mm: f64,
        profile: &BitProfile,
    ) -> (f64, Point3) {
        // A V-bit's radius reads linearly on either side of the plane, as
        // `least_room` reads it; a TBN's is 0 above it.
        let radius = |z: f64| {
            let d = start_plane_z_mm - z;
            match *profile {
                BitProfile::VBit { slope } => d * slope,
                BitProfile::TaperedBall { .. } => profile.half_width_mm(d),
            }
        };
        let (a, b) = if a.xy().distance_to(b.xy()) <= EPS_MM {
            let low = if a.z <= b.z { a } else { b };
            (low, low)
        } else {
            (a, b)
        };
        let reach_mm = radius(a.z).max(radius(b.z)).max(0.0) + 2.0 * EPS_MM;
        let (pa, pb) = (a.xy(), b.xy());
        let lo = Point::new(pa.x.min(pb.x), pa.y.min(pb.y));
        let hi = Point::new(pa.x.max(pb.x), pa.y.max(pb.y));
        let at = |t: f64| {
            Point3::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            )
        };
        let mut best = (reach_mm - radius(a.z).max(radius(b.z)), a);
        for wall in self.near(lo, hi, reach_mm) {
            let piece = WallPiece::line(wall.a, wall.b);
            let (room_mm, t) = least_room(a, b, piece, profile, start_plane_z_mm);
            if room_mm < best.0 {
                best = (room_mm, at(t));
            }
        }
        best
    }
}

/// A male Inlay's open band edge (#448; `docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 4): beyond these walls the board stands to
/// the stock top, so the bit may not cross them below it. The widest disc
/// a bit cuts below the stock top is the one at the stock top, `w(top −
/// z)` across for a tip at `z`.
#[derive(Debug, Clone)]
pub(crate) struct OpenEdgeWalls {
    /// The band's edge, `B`.
    pub walls: Walls,
    /// The stock top, mm.
    pub top_z_mm: f64,
    /// The bit's profile.
    pub profile: BitProfile,
}

impl OpenEdgeWalls {
    /// Whether the straight move `a`–`b` keeps the cone's disc at the
    /// stock top within the walls all along it, exactly
    /// ([`Walls::least_room`]).
    pub(crate) fn allows(&self, a: Point3, b: Point3) -> bool {
        self.walls.least_room(a, b, self.top_z_mm, &self.profile).0 >= 0.0
    }
}

/// The distance between segments `a`–`b` and `c`–`d`, mm: 0 where they
/// cross or touch.
fn segment_distance(a: Point, b: Point, c: Point, d: Point) -> f64 {
    let (ab, cd) = (b - a, d - c);
    let denom = ab.cross(cd);
    if denom.abs() > PARALLEL_CROSS_MM2 {
        let t = (c - a).cross(cd) / denom;
        let u = (c - a).cross(ab) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return 0.0;
        }
    }
    point_segment(a, c, d)
        .min(point_segment(b, c, d))
        .min(point_segment(c, a, b))
        .min(point_segment(d, a, b))
}

/// Distance, mm, from `p` to the segment `a`–`b`.
pub(crate) fn point_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab: Vec2 = b - a;
    let length_sq = ab.dot(ab);
    let t = if length_sq > POINT_LENGTH_SQ_MM2 {
        ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance_to(a + ab * t)
}

/// `points` without consecutive duplicates or an explicit closing vertex,
/// as the medial axis cleans its loops. Each point carries the drawn wall
/// its chord on stands for; a point kept for a dropped duplicate takes the
/// duplicate's, since the chord from it is now the duplicate's.
fn clean_loop<T: Copy>(points: Vec<(Point, T)>) -> Vec<(Point, T)> {
    let mut out: Vec<(Point, T)> = Vec::new();
    for (p, source) in points {
        if let Some(last) = out.last_mut()
            && last.0.distance_to(p) < SAME_POINT_MM
        {
            last.1 = source;
            continue;
        }
        out.push((p, source));
    }
    while let (Some(&(first, _)), Some(&(last, _))) = (out.first(), out.last()) {
        if out.len() >= 2 && first.distance_to(last) < SAME_POINT_MM {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// How many cells `from..=to` is; none when `to` is before `from`.
fn span(from: usize, to: usize) -> usize {
    (to + 1).saturating_sub(from)
}

/// `v` floored and clamped to `0..len`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is floored and clamped to 0..len first"
)]
fn clamped_index(v: f64, len: usize) -> usize {
    let last = len.saturating_sub(1);
    let f = v.floor();
    if f.is_nan() || f <= 0.0 {
        0
    } else if f >= last as f64 {
        last
    } else {
        f as usize
    }
}

/// A non-negative whole `v` as a count, saturating at [`MAX_CELLS`]; 0
/// for NaN or below.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=MAX_CELLS first"
)]
fn count_of(v: f64) -> usize {
    if v.is_nan() || v <= 0.0 {
        0
    } else {
        v.min(MAX_CELLS) as usize
    }
}
