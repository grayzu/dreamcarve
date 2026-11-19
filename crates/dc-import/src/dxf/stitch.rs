// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts (stitchShapes, bestOpenProfileMergeCandidate,
//   mergeOpenProfiles, snapProfileClosed, dedupeIdenticalShapes)

//! Stitching of open shapes whose ends meet, and removal of duplicate
//! shapes, per layer (`docs/specs/phase-2/design.md`, "Algorithms → DXF
//! import", step 5). Shapes are bulge polylines, so a joined chain keeps
//! its arcs. Upstream's T-junction splitting is exported but never called
//! there and is not ported.
//!
//! Changed from upstream, which compares every shape with every shape:
//! the joins and their order are upstream's, but the ends near a chain's
//! end and the shapes that could repeat a shape are found in a grid of
//! cells, a chain is built in place, and the work is charged to a budget
//! ([`MAX_JOIN_COMPARISONS`]; review 1.5). The cell maps are `HashMap`s
//! that are only looked up, never iterated, so the result does not depend
//! on their order.

use std::collections::{BTreeMap, HashMap, VecDeque};

use dc_geometry::{Point, Polyline, Vec2, Vertex};

use super::entities::Shape;
use crate::ImportError;

/// Most comparisons [`dedupe`] and [`stitch`] make between them in one
/// import: a shape against a kept shape that could repeat it, and a
/// chain's end against a shape's end near it. A drawing costs a few per
/// shape; only ends crowded on one point by the thousand, or thousands of
/// different shapes in one box, cost more, and past this the import fails
/// with [`ImportError::DxfJoinTooMuchWork`] rather than run for minutes.
pub(super) const MAX_JOIN_COMPARISONS: usize = 100_000_000;

/// The work [`dedupe`] and [`stitch`] have done in one import, against a
/// limit ([`MAX_JOIN_COMPARISONS`]).
#[derive(Debug)]
pub(super) struct JoinBudget {
    comparisons: usize,
    limit: usize,
}

impl JoinBudget {
    pub(super) fn new() -> Self {
        Self {
            comparisons: 0,
            limit: MAX_JOIN_COMPARISONS,
        }
    }

    /// A budget of `limit` comparisons.
    #[cfg(test)]
    pub(super) fn with_limit(limit: usize) -> Self {
        Self {
            comparisons: 0,
            limit,
        }
    }

    /// Comparisons made so far.
    #[cfg(test)]
    pub(super) fn comparisons(&self) -> usize {
        self.comparisons
    }

    /// Takes `n` comparisons, before they are made.
    fn spend(&mut self, n: usize) -> Result<(), ImportError> {
        self.comparisons = self.comparisons.saturating_add(n);
        if self.comparisons > self.limit {
            return Err(ImportError::DxfJoinTooMuchWork { limit: self.limit });
        }
        Ok(())
    }
}

/// Two vertices closer than this, mm, with bulges closer than this, are
/// the same vertex for the dedupe.
const DEDUPE_EPS: f64 = 1e-6;

/// The grid cell of coordinate `v_mm` on a grid of `cell_mm`. Two
/// coordinates no further apart than half a cell are in the same cell or
/// in neighbouring ones, whatever the rounding of the division.
#[allow(
    clippy::cast_possible_truncation,
    reason = "coordinates are within the import's coordinate limit, and a cast past i64 \
              saturates, which only puts far cells together"
)]
fn cell_of(v_mm: f64, cell_mm: f64) -> i64 {
    (v_mm / cell_mm).floor() as i64
}

/// The cell (`x`, `y`) and the eight round it.
fn neighbourhood(x: i64, y: i64) -> impl Iterator<Item = (i64, i64)> {
    (-1..=1)
        .flat_map(move |dx| (-1..=1).map(move |dy| (x.saturating_add(dx), y.saturating_add(dy))))
}

/// Layer names as small numbers, so a cell's key holds no string.
#[derive(Debug, Default)]
struct Layers<'a> {
    ids: HashMap<&'a str, usize>,
}

impl<'a> Layers<'a> {
    fn id(&mut self, layer: &'a str) -> usize {
        let next = self.ids.len();
        *self.ids.entry(layer).or_insert(next)
    }
}

/// The lower-left and upper-right corners of `polyline`'s vertices (not
/// of its arcs); the origin twice for no vertices.
fn vertex_corners(polyline: &Polyline) -> (Point, Point) {
    let mut vertices = polyline.vertices().iter();
    let Some(first) = vertices.next() else {
        return (Point::new(0.0, 0.0), Point::new(0.0, 0.0));
    };
    vertices.fold((first.point(), first.point()), |(min, max), v| {
        (
            Point::new(min.x.min(v.x), min.y.min(v.y)),
            Point::new(max.x.max(v.x), max.y.max(v.y)),
        )
    })
}

/// What two shapes must share exactly before their vertices are compared:
/// layer, whether a hatch, how many inner loops, whether the outer loop is
/// closed and its vertex count, and the cell of its lower-left corner.
type DedupeKey = (usize, bool, usize, bool, usize, i64, i64);

/// Drops every shape that repeats an earlier one on the same layer: the
/// same vertices and bulges in order, forwards or reversed, for each of
/// its loops. The first occurrence is kept, in place. A HATCH's boundary
/// is compared only with other hatches': against a shape drawn on its own
/// it is the hatch that gives way, whichever comes first
/// ([`drop_hatch_duplicates`]).
///
/// A shape is compared only with the kept shapes that could repeat it,
/// found by the cell its lower-left vertex corner falls in (review 1.5):
/// vertices that match within [`DEDUPE_EPS`], in either direction, have
/// corners that close, so a repeat is in that cell or one beside it. Each
/// shape looked at, and each vertex compared, is charged to `budget`.
///
/// # Errors
///
/// [`ImportError::DxfJoinTooMuchWork`] past the budget.
pub(super) fn dedupe(
    shapes: Vec<Shape>,
    budget: &mut JoinBudget,
) -> Result<Vec<Shape>, ImportError> {
    // Corners of matching shapes differ by at most `DEDUPE_EPS` on each
    // axis, a quarter of a cell.
    let cell_mm = 4.0 * DEDUPE_EPS;
    let mut layers = Layers::default();
    // Per key, the kept shapes: their index and upper-right vertex corner,
    // which rejects most of a cell's shapes unwalked.
    let mut index: HashMap<DedupeKey, Vec<(usize, Point)>> = HashMap::new();
    let mut repeats = vec![false; shapes.len()];
    for (i, (shape, repeat)) in shapes.iter().zip(&mut repeats).enumerate() {
        let layer = layers.id(&shape.layer);
        let (min, max) = vertex_corners(&shape.polyline);
        let key = |(x, y): (i64, i64)| -> DedupeKey {
            (
                layer,
                shape.hatch,
                shape.inner.len(),
                shape.polyline.is_closed(),
                shape.polyline.vertices().len(),
                x,
                y,
            )
        };
        let cell = (cell_of(min.x, cell_mm), cell_of(min.y, cell_mm));
        let vertex_count: usize = shape.loops().map(|l| l.vertices().len()).sum();
        'cells: for near in neighbourhood(cell.0, cell.1) {
            let Some(candidates) = index.get(&key(near)) else {
                continue;
            };
            for &(k, k_max) in candidates {
                budget.spend(1)?;
                let apart = |a: f64, b: f64| (a - b).abs() > 2.0 * DEDUPE_EPS;
                if apart(k_max.x, max.x) || apart(k_max.y, max.y) {
                    continue;
                }
                let Some(kept) = shapes.get(k) else {
                    continue;
                };
                // Forwards, then reversed, over every loop.
                budget.spend(vertex_count.saturating_mul(2))?;
                if kept
                    .loops()
                    .zip(shape.loops())
                    .all(|(a, b)| same_polyline(a, b) || same_polyline(a, &b.reversed()))
                {
                    *repeat = true;
                    break 'cells;
                }
            }
        }
        if !*repeat {
            index.entry(key(cell)).or_default().push((i, max));
        }
    }
    Ok(shapes
        .into_iter()
        .zip(repeats)
        .filter(|(_, repeat)| !repeat)
        .map(|(shape, _)| shape)
        .collect())
}

/// Most vertex comparisons the HATCH duplicate rule makes in one import.
/// Past it every remaining loop is taken as distinct: the import never
/// fails for it, it only may keep a duplicate that nesting then drops.
pub(super) const MAX_DUPLICATE_COMPARISONS: usize = 20_000_000;

/// The work [`drop_hatch_duplicates`] has done in one import, against
/// [`MAX_DUPLICATE_COMPARISONS`].
#[derive(Debug, Default)]
pub(super) struct MatchBudget {
    comparisons: usize,
}

impl MatchBudget {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Comparisons made so far.
    #[cfg(test)]
    pub(super) fn comparisons(&self) -> usize {
        self.comparisons
    }

    /// Whether the budget is used up.
    pub(super) fn exhausted(&self) -> bool {
        self.comparisons >= MAX_DUPLICATE_COMPARISONS
    }

    /// Takes `n` comparisons; `false`, taking none, when they would pass
    /// the budget.
    fn spend(&mut self, n: usize) -> bool {
        let after = self.comparisons.saturating_add(n);
        if after > MAX_DUPLICATE_COMPARISONS {
            self.comparisons = MAX_DUPLICATE_COMPARISONS;
            return false;
        }
        self.comparisons = after;
        true
    }
}

/// Most starting vertices tried per direction when matching two loops:
/// the vertices of one within the tolerance of the other's first vertex.
/// More only happen when a loop crowds vertices closer than the tolerance.
const MAX_ANCHORS: usize = 4;

/// A closed shape's size, to reject a match before any vertex is walked.
#[derive(Debug, Clone, Copy)]
struct Measure {
    area_mm2: f64,
    length_mm: f64,
}

impl Measure {
    fn of(p: &Polyline) -> Self {
        Self {
            area_mm2: p.signed_area_mm2().abs(),
            length_mm: p.length_mm(),
        }
    }

    /// Whether loops of these sizes could be the same within
    /// `tolerance_mm` on `n` vertices: moving each vertex, and each arc's
    /// middle, by the tolerance changes the length by at most four times
    /// the tolerance per vertex and the area by at most twice the length
    /// times it.
    fn close_to(self, other: Self, n: usize, tolerance_mm: f64) -> bool {
        #[allow(clippy::cast_precision_loss, reason = "a vertex count")]
        let n = n as f64;
        let length = self.length_mm.max(other.length_mm);
        (self.length_mm - other.length_mm).abs() <= 4.0 * n * tolerance_mm + 1e-9
            && (self.area_mm2 - other.area_mm2).abs() <= 4.0 * length * tolerance_mm + 1e-9
    }
}

/// Removes from each HATCH every boundary loop that repeats a closed shape
/// drawn by another entity (#668): an associative hatch's boundary is
/// often also drawn as its own LWPOLYLINE, and the drawn shape is kept. A
/// loop repeats a shape when both have the same vertices, from any
/// starting vertex and in either direction, each within `tolerance_mm`,
/// and arcs whose bulges differ by no more than that at their middles. A
/// hatch whose every loop repeats a shape is dropped; one with loops left
/// keeps them, its largest first. With `same_layer`, only shapes on the
/// hatch's own layer count. Returns the shapes and the layer of each loop
/// removed.
pub(super) fn drop_hatch_duplicates(
    shapes: Vec<Shape>,
    tolerance_mm: f64,
    same_layer: bool,
    budget: &mut MatchBudget,
) -> (Vec<Shape>, Vec<String>) {
    // The drawn closed shapes, by vertex count and the grid cell of their
    // bounds' lower corner, so a hatch is compared with the few that could
    // match rather than with every shape.
    let cell_mm = 4.0 * tolerance_mm.max(DEDUPE_EPS);
    let key = |p: &Polyline| -> Option<(usize, i64, i64)> {
        let b = p.bounds()?;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "coordinates are within the import's coordinate limit"
        )]
        let cell = |v: f64| (v / cell_mm).floor() as i64;
        Some((p.vertices().len(), cell(b.min.x), cell(b.min.y)))
    };
    let mut drawn: BTreeMap<(usize, i64, i64), Vec<(usize, Measure)>> = BTreeMap::new();
    if shapes.iter().any(|s| s.hatch) {
        for (i, shape) in shapes.iter().enumerate() {
            if !shape.hatch
                && shape.polyline.is_closed()
                && let Some(k) = key(&shape.polyline)
            {
                drawn
                    .entry(k)
                    .or_default()
                    .push((i, Measure::of(&shape.polyline)));
            }
        }
    }
    // Each candidate costs a comparison of its size; a vertex walk costs
    // one per vertex. Once the budget is gone, loops are distinct.
    let repeats_drawn = |hatch: &Shape, l: &Polyline, budget: &mut MatchBudget| {
        let Some((n, x, y)) = key(l) else {
            return false;
        };
        let measure = Measure::of(l);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(candidates) = drawn.get(&(n, x + dx, y + dy)) else {
                    continue;
                };
                for &(i, size) in candidates {
                    if budget.exhausted() || !budget.spend(1) {
                        return false;
                    }
                    let Some(s) = shapes.get(i) else {
                        continue;
                    };
                    if (!same_layer || s.layer == hatch.layer)
                        && size.close_to(measure, n, tolerance_mm)
                        && same_loop(&s.polyline, l, tolerance_mm, budget)
                    {
                        return true;
                    }
                }
            }
        }
        false
    };
    let mut removed = Vec::new();
    let mut out = Vec::with_capacity(shapes.len());
    for shape in &shapes {
        if !shape.hatch {
            out.push(shape.clone());
            continue;
        }
        let mut left: Vec<Polyline> = Vec::new();
        for l in shape.loops() {
            if repeats_drawn(shape, l, budget) {
                removed.push(shape.layer.clone());
            } else {
                left.push(l.clone());
            }
        }
        if left.is_empty() {
            continue;
        }
        let largest = left
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.signed_area_mm2()
                    .abs()
                    .total_cmp(&b.signed_area_mm2().abs())
            })
            .map_or(0, |(i, _)| i);
        let polyline = left.remove(largest);
        out.push(Shape {
            layer: shape.layer.clone(),
            polyline,
            inner: left,
            hatch: true,
        });
    }
    (out, removed)
}

/// Whether closed polylines `a` and `b` are the same loop within
/// `tolerance_mm`: the same vertices from some starting vertex of `b`, in
/// either direction, and bulges whose arcs' middles lie within the
/// tolerance (half the chord times the bulge difference). Only the
/// vertices of `b` near `a`'s first are tried as its start, at most
/// [`MAX_ANCHORS`] per direction, and every vertex looked at is charged to
/// `budget`; `false` once it runs out.
fn same_loop(a: &Polyline, b: &Polyline, tolerance_mm: f64, budget: &mut MatchBudget) -> bool {
    let n = a.vertices().len();
    if !(a.is_closed() && b.is_closed()) || b.vertices().len() != n || n == 0 {
        return false;
    }
    let Some(first) = a.vertices().first() else {
        return false;
    };
    let matches_from = |b: &Polyline, shift: usize| {
        (0..n).all(|i| {
            let (Some(p), Some(q)) = (a.vertices().get(i), b.vertices().get((i + shift) % n))
            else {
                return false;
            };
            let next = a.vertices().get((i + 1) % n).unwrap_or(p);
            let chord = p.point().distance_to(next.point());
            p.point().distance_to(q.point()) <= tolerance_mm
                && (p.bulge - q.bulge).abs() * chord / 2.0 <= tolerance_mm
        })
    };
    for reverse in [false, true] {
        // The scan for anchors, then each walk, costs `n`.
        if !budget.spend(n) {
            return false;
        }
        let b = if reverse { b.reversed() } else { b.clone() };
        let anchors = b
            .vertices()
            .iter()
            .enumerate()
            .filter(|(_, v)| v.point().distance_to(first.point()) <= tolerance_mm)
            .map(|(shift, _)| shift)
            .take(MAX_ANCHORS);
        for shift in anchors {
            if !budget.spend(n) {
                return false;
            }
            if matches_from(&b, shift) {
                return true;
            }
        }
    }
    false
}

fn same_polyline(a: &Polyline, b: &Polyline) -> bool {
    a.is_closed() == b.is_closed()
        && a.vertices().len() == b.vertices().len()
        && a.vertices().iter().zip(b.vertices()).all(|(p, q)| {
            p.point().distance_to(q.point()) <= DEDUPE_EPS
                && (p.bulge - q.bulge).abs() <= DEDUPE_EPS
        })
}

/// Joins open shapes on one layer whose ends lie within `tolerance_mm`,
/// nearest pair first, until no more join; a chain whose ends then meet
/// within the tolerance is closed. Closed shapes pass through. Output order
/// is the order of each chain's first shape.
///
/// A chain grows from its first shape in file order, at either end: each
/// step takes, of the free shapes on its layer with an end within the
/// tolerance of one of the chain's ends, the nearest; of equally near
/// ones the earliest in the file; and of one shape's ends, in the order
/// of [`Join`]. The piece joined at the chain's end is moved by the gap;
/// one joined at its start stays, and the chain moves.
///
/// The free ends near a point are found in a grid of cells ([`Ends`]),
/// not by a pass over every shape, and a chain is built in place
/// ([`Chain`]), not copied at every join (review 1.5). Each end looked at
/// is charged to `budget`.
///
/// # Errors
///
/// [`ImportError::DxfJoinTooMuchWork`] past the budget.
pub(super) fn stitch(
    shapes: Vec<Shape>,
    tolerance_mm: f64,
    budget: &mut JoinBudget,
) -> Result<Vec<Shape>, ImportError> {
    let mut ends = Ends::of(&shapes, tolerance_mm);
    let mut consumed = vec![false; shapes.len()];
    let mut out = Vec::with_capacity(shapes.len());
    for (index, shape) in shapes.iter().enumerate() {
        if consumed.get(index).copied().unwrap_or(true) {
            continue;
        }
        if let Some(done) = consumed.get_mut(index) {
            *done = true;
        }
        if shape.polyline.is_closed() {
            out.push(shape.clone());
            continue;
        }
        // An open shape of no vertices has no ends to join; the walker
        // makes none.
        let Some(mut chain) = Chain::of(&shape.polyline) else {
            out.push(shape.clone());
            continue;
        };
        let layer = ends.layer_of(index);
        while let Some((candidate, join)) = ends.nearest(&chain, layer, &consumed, budget)? {
            if let Some(done) = consumed.get_mut(candidate) {
                *done = true;
            }
            if let Some(other) = shapes.get(candidate) {
                chain.join(join, &other.polyline);
            }
        }
        out.push(Shape::plain(
            shape.layer.clone(),
            chain.into_polyline(tolerance_mm),
        ));
    }
    Ok(out)
}

/// Which ends of the chain and a candidate meet. Between two ways to join
/// that are equally near, the one declared first here is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Join {
    /// The chain's end meets the candidate's start: chain then candidate.
    EndToStart,
    /// The chain's end meets the candidate's end: chain then the reversed
    /// candidate.
    EndToEnd,
    /// The chain's start meets the candidate's end: candidate then chain.
    StartToEnd,
    /// The chain's start meets the candidate's start: the reversed
    /// candidate then chain.
    StartToStart,
}

/// One end of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    End,
}

impl Join {
    /// The join of the chain's `chain` end to a candidate's `candidate`
    /// end.
    const fn of(chain: Side, candidate: Side) -> Self {
        match (chain, candidate) {
            (Side::End, Side::Start) => Self::EndToStart,
            (Side::End, Side::End) => Self::EndToEnd,
            (Side::Start, Side::End) => Self::StartToEnd,
            (Side::Start, Side::Start) => Self::StartToStart,
        }
    }
}

fn start_of(p: &Polyline) -> Point {
    p.vertices()
        .first()
        .map_or(Point::new(0.0, 0.0), |v| v.point())
}

fn end_of(p: &Polyline) -> Point {
    p.vertices()
        .last()
        .map_or(Point::new(0.0, 0.0), |v| v.point())
}

/// One end of an open shape.
#[derive(Debug, Clone, Copy)]
struct End {
    /// The shape's index.
    shape: usize,
    side: Side,
    at: Point,
}

/// The ends of every open shape, by layer and grid cell, so the ends near
/// a point are those of nine cells. A cell is twice the join tolerance
/// wide: an end within the tolerance of a point is in the point's cell or
/// one beside it. Ends of shapes already joined are dropped from a cell
/// when it is next looked at.
#[derive(Debug)]
struct Ends {
    tolerance_mm: f64,
    cell_mm: f64,
    /// Each shape's layer, as a number.
    layers: Vec<usize>,
    cells: HashMap<(usize, i64, i64), Vec<End>>,
}

impl Ends {
    fn of(shapes: &[Shape], tolerance_mm: f64) -> Self {
        // No finer than the dedupe's, so a tolerance of zero still has
        // cells.
        let cell_mm = 2.0 * tolerance_mm.max(DEDUPE_EPS);
        let mut names = Layers::default();
        let mut layers = Vec::with_capacity(shapes.len());
        let mut cells: HashMap<(usize, i64, i64), Vec<End>> = HashMap::new();
        for (shape, s) in shapes.iter().enumerate() {
            let layer = names.id(&s.layer);
            layers.push(layer);
            if s.polyline.is_closed() || s.polyline.vertices().is_empty() {
                continue;
            }
            let ends = [
                (Side::Start, start_of(&s.polyline)),
                (Side::End, end_of(&s.polyline)),
            ];
            for (side, at) in ends {
                let key = (layer, cell_of(at.x, cell_mm), cell_of(at.y, cell_mm));
                cells.entry(key).or_default().push(End { shape, side, at });
            }
        }
        Self {
            tolerance_mm,
            cell_mm,
            layers,
            cells,
        }
    }

    /// The layer of shape `index`.
    fn layer_of(&self, index: usize) -> usize {
        self.layers.get(index).copied().unwrap_or(usize::MAX)
    }

    /// The free shape on `layer` with an end nearest to an end of `chain`,
    /// within the tolerance, and how to join it: the nearest; of equally
    /// near ones the lowest index; of one shape's joins the first in
    /// [`Join`]'s order.
    fn nearest(
        &mut self,
        chain: &Chain,
        layer: usize,
        consumed: &[bool],
        budget: &mut JoinBudget,
    ) -> Result<Option<(usize, Join)>, ImportError> {
        let mut best: Option<(f64, usize, Join)> = None;
        for (chain_side, from) in [(Side::End, chain.end), (Side::Start, chain.start)] {
            let (x, y) = (cell_of(from.x, self.cell_mm), cell_of(from.y, self.cell_mm));
            for (x, y) in neighbourhood(x, y) {
                let Some(cell) = self.cells.get_mut(&(layer, x, y)) else {
                    continue;
                };
                budget.spend(cell.len())?;
                cell.retain(|end| !consumed.get(end.shape).copied().unwrap_or(true));
                for end in cell.iter() {
                    let distance = from.distance_to(end.at);
                    if distance <= self.tolerance_mm {
                        let join = Join::of(chain_side, end.side);
                        let nearer = best.is_none_or(|(d, shape, j)| {
                            distance
                                .total_cmp(&d)
                                .then_with(|| (end.shape, join).cmp(&(shape, j)))
                                .is_lt()
                        });
                        if nearer {
                            best = Some((distance, end.shape, join));
                        }
                    }
                }
            }
        }
        Ok(best.map(|(_, shape, join)| (shape, join)))
    }
}

/// A chain of joined pieces, grown at either end without copying what it
/// already holds.
///
/// A piece joined at the end is moved by the gap as it is added. A piece
/// joined at the start stays where it is and every vertex already in the
/// chain moves by the gap instead; that move is only recorded (`shifts`)
/// and each vertex notes how many were recorded before it was added, so
/// [`Chain::into_polyline`] adds to each vertex the sum of the ones after.
/// `start` and `end` are kept as the moves would leave them, each gap
/// added in turn, since they decide the next join.
#[derive(Debug)]
struct Chain {
    /// Each vertex as it was when added, and the length of `shifts` then.
    vertices: VecDeque<(Vertex, usize)>,
    /// The gap of each join at the start, in order.
    shifts: Vec<Vec2>,
    start: Point,
    end: Point,
}

impl Chain {
    /// A chain of the open `seed` alone; `None` when it has no vertices.
    fn of(seed: &Polyline) -> Option<Self> {
        let (first, last) = (seed.vertices().first()?, seed.vertices().last()?);
        Some(Self {
            vertices: seed.vertices().iter().map(|v| (*v, 0)).collect(),
            shifts: Vec::new(),
            start: first.point(),
            end: last.point(),
        })
    }

    fn join(&mut self, join: Join, candidate: &Polyline) {
        match join {
            Join::EndToStart => self.append(candidate.vertices()),
            Join::EndToEnd => self.append(candidate.reversed().vertices()),
            Join::StartToEnd => self.prepend(candidate.vertices()),
            Join::StartToStart => self.prepend(candidate.reversed().vertices()),
        }
    }

    /// The chain followed by `right`, with `right` translated so its start
    /// lands on the chain's end (upstream moves the joined piece by the
    /// gap). The chain's last vertex takes `right`'s first bulge: it now
    /// starts that segment.
    fn append(&mut self, right: &[Vertex]) {
        let Some((first, rest)) = right.split_first() else {
            return;
        };
        let gap = self.end - first.point();
        if let Some((last, _)) = self.vertices.back_mut() {
            last.bulge = first.bulge;
        }
        let shifts = self.shifts.len();
        for v in rest {
            let moved = Vertex::new(v.x + gap.x, v.y + gap.y, v.bulge);
            self.end = moved.point();
            self.vertices.push_back((moved, shifts));
        }
    }

    /// `left` followed by the chain, with the chain translated so its
    /// start lands on `left`'s end. `left`'s last vertex takes the place
    /// of the chain's first, and its bulge.
    fn prepend(&mut self, left: &[Vertex]) {
        let (Some(first), Some((last, rest))) = (left.first(), left.split_last()) else {
            return;
        };
        let Some((replaced, _)) = self.vertices.pop_front() else {
            return;
        };
        let gap = last.point() - self.start;
        self.end = if self.vertices.is_empty() {
            last.point()
        } else {
            Point::new(self.end.x + gap.x, self.end.y + gap.y)
        };
        self.start = first.point();
        self.shifts.push(gap);
        let shifts = self.shifts.len();
        self.vertices.push_front((
            Vertex {
                bulge: replaced.bulge,
                ..*last
            },
            shifts,
        ));
        for v in rest.iter().rev() {
            self.vertices.push_front((*v, shifts));
        }
    }

    /// The chain as a polyline, closed when its ends meet within
    /// `tolerance_mm` and it has at least three vertices: the last vertex
    /// is then dropped and its predecessor's segment becomes the closing
    /// one, ending on the first vertex.
    fn into_polyline(self, tolerance_mm: f64) -> Polyline {
        // `pending[k]` is the sum of the shifts from the `k`th on: what a
        // vertex added after `k` of them still has to move by. One shift
        // is added as it is; several are summed last first, which can
        // differ in the last bits from adding each to the vertex in turn.
        let mut pending: Vec<Vec2> = Vec::with_capacity(self.shifts.len());
        let mut sum: Option<Vec2> = None;
        for shift in self.shifts.iter().rev() {
            let total = match sum {
                Some(later) => Vec2::new(shift.x + later.x, shift.y + later.y),
                None => *shift,
            };
            sum = Some(total);
            pending.push(total);
        }
        pending.reverse();
        let mut vertices: Vec<Vertex> = self
            .vertices
            .into_iter()
            .map(|(v, shifts)| match pending.get(shifts) {
                Some(shift) => Vertex::new(v.x + shift.x, v.y + shift.y, v.bulge),
                None => v,
            })
            .collect();
        if vertices.len() < 3 || self.start.distance_to(self.end) > tolerance_mm {
            return Polyline::new(vertices, false);
        }
        vertices.pop();
        Polyline::new(vertices, true)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "tests"
    )]

    use super::*;
    use dc_geometry::assert_approx_eq;

    fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> Shape {
        Shape::plain(
            layer.into(),
            Polyline::new(vec![Vertex::line(a.0, a.1), Vertex::line(b.0, b.1)], false),
        )
    }

    /// [`stitch`] within the import's own budget.
    fn stitched(shapes: Vec<Shape>, tolerance_mm: f64) -> Vec<Shape> {
        stitch(shapes, tolerance_mm, &mut JoinBudget::new()).unwrap()
    }

    #[test]
    fn reversed_pieces_join_and_keep_bulges() {
        // An arc from (0,0) to (10,0) bulging below, then a line drawn
        // backwards from (10,10) to (10,0).
        let arc = Shape::plain(
            "a".into(),
            Polyline::new(
                vec![Vertex::new(0.0, 0.0, -0.5), Vertex::line(10.0, 0.0)],
                false,
            ),
        );
        let back = line("a", (10.0, 10.0), (10.0, 0.0));
        let out = stitched(vec![arc, back], 0.01);
        assert_eq!(out.len(), 1);
        let p = &out[0].polyline;
        assert!(!p.is_closed());
        assert_eq!(p.vertices().len(), 3);
        assert_approx_eq!(p.vertices()[0].bulge, -0.5, 1e-12);
        assert_approx_eq!(p.vertices()[1].bulge, 0.0, 1e-12);
        assert_eq!(p.vertices()[2].point(), Point::new(10.0, 10.0));
    }

    #[test]
    fn join_translates_by_the_gap() {
        let a = line("a", (0.0, 0.0), (10.0, 0.0));
        let b = line("a", (10.005, 0.0), (20.0, 0.0));
        let out = stitched(vec![a, b], 0.01);
        assert_eq!(out.len(), 1);
        let p = &out[0].polyline;
        assert_eq!(p.vertices().len(), 3);
        assert_approx_eq!(p.vertices()[2].x, 20.0 - 0.005, 1e-12);
    }

    /// Two lines start near the chain's end; the nearer joins. Its far end
    /// moves by the 0.001 mm gap. The other line's start is then next to an
    /// interior vertex, not an end, so it stays a shape of its own.
    #[test]
    fn nearest_end_wins() {
        let chain = line("a", (0.0, 0.0), (10.0, 0.0));
        let near = line("a", (10.001, 0.0), (10.0, 5.0));
        let far = line("a", (10.005, 0.0), (10.0, -5.0));
        let out = stitched(vec![chain, far, near], 0.01);
        assert_eq!(out.len(), 2);
        let joined = &out[0].polyline;
        assert_eq!(joined.vertices().len(), 3);
        assert_approx_eq!(joined.vertices()[2].x, 9.999, 1e-12);
        assert_approx_eq!(joined.vertices()[2].y, 5.0, 1e-12);
        assert_eq!(
            out[1].polyline.vertices()[1].point(),
            Point::new(10.0, -5.0)
        );
    }

    /// A piece that joins the chain's start moves the chain, not itself,
    /// by the gap (upstream moves the joined piece's far side): the piece
    /// keeps its own coordinates and every chain vertex after the join
    /// shifts, here twice.
    #[test]
    fn joins_at_the_start_shift_the_chain_by_each_gap() {
        let seed = line("a", (0.0, 0.0), (10.0, 0.0));
        let before = line("a", (-10.0, 0.0), (-0.004, 0.0));
        let first = line("a", (-20.0, 0.0), (-10.002, 0.0));
        let out = stitched(vec![seed, before, first], 0.01);
        assert_eq!(out.len(), 1);
        let xs: Vec<f64> = out[0].polyline.vertices().iter().map(|v| v.x).collect();
        assert_eq!(xs.len(), 4);
        assert_approx_eq!(xs[0], -20.0, 1e-12);
        assert_approx_eq!(xs[1], -10.002, 1e-12);
        assert_approx_eq!(xs[2], -0.004 - 0.002, 1e-12);
        assert_approx_eq!(xs[3], 10.0 - 0.004 - 0.002, 1e-12);
    }

    #[test]
    fn dedupe_keeps_first_of_equal_shapes_per_layer() {
        let shapes = vec![
            line("a", (0.0, 0.0), (10.0, 0.0)),
            line("a", (10.0, 0.0), (0.0, 0.0)),
            line("b", (0.0, 0.0), (10.0, 0.0)),
            line("a", (0.0, 0.0), (10.0, 0.0)),
        ];
        let out = dedupe(shapes, &mut JoinBudget::new()).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].layer, "a");
        assert_eq!(out[1].layer, "b");
    }

    /// The dedupe and the stitch as they were before review 1.5, which
    /// compared every shape with every shape: the definition the indexed
    /// ones are checked against, on inputs small enough for it.
    mod reference {
        use super::super::{DEDUPE_EPS, Shape, end_of, start_of};
        use dc_geometry::{Polyline, Vertex};

        fn same_polyline(a: &Polyline, b: &Polyline) -> bool {
            a.is_closed() == b.is_closed()
                && a.vertices().len() == b.vertices().len()
                && a.vertices().iter().zip(b.vertices()).all(|(p, q)| {
                    p.point().distance_to(q.point()) <= DEDUPE_EPS
                        && (p.bulge - q.bulge).abs() <= DEDUPE_EPS
                })
        }

        pub(super) fn dedupe(shapes: Vec<Shape>) -> Vec<Shape> {
            let mut kept: Vec<Shape> = Vec::with_capacity(shapes.len());
            for shape in shapes {
                let seen = kept.iter().any(|k| {
                    k.layer == shape.layer
                        && k.hatch == shape.hatch
                        && k.inner.len() == shape.inner.len()
                        && k.loops()
                            .zip(shape.loops())
                            .all(|(a, b)| same_polyline(a, b) || same_polyline(a, &b.reversed()))
                });
                if !seen {
                    kept.push(shape);
                }
            }
            kept
        }

        pub(super) fn stitch(shapes: Vec<Shape>, tolerance_mm: f64) -> Vec<Shape> {
            let mut consumed = vec![false; shapes.len()];
            let mut out = Vec::with_capacity(shapes.len());
            for index in 0..shapes.len() {
                if consumed[index] {
                    continue;
                }
                consumed[index] = true;
                let shape = &shapes[index];
                if shape.polyline.is_closed() {
                    out.push(shape.clone());
                    continue;
                }
                let mut current = shape.polyline.clone();
                while let Some((candidate, join)) =
                    best_join(&current, &shape.layer, &shapes, &consumed, tolerance_mm)
                {
                    consumed[candidate] = true;
                    current = join.apply(&current, &shapes[candidate].polyline);
                }
                out.push(Shape::plain(
                    shape.layer.clone(),
                    snap_closed(current, tolerance_mm),
                ));
            }
            out
        }

        #[derive(Debug, Clone, Copy)]
        enum Join {
            EndToStart,
            EndToEnd,
            StartToEnd,
            StartToStart,
        }

        impl Join {
            fn apply(self, chain: &Polyline, candidate: &Polyline) -> Polyline {
                match self {
                    Self::EndToStart => merge(chain, candidate),
                    Self::EndToEnd => merge(chain, &candidate.reversed()),
                    Self::StartToEnd => merge(candidate, chain),
                    Self::StartToStart => merge(&candidate.reversed(), chain),
                }
            }
        }

        fn best_join(
            chain: &Polyline,
            layer: &str,
            shapes: &[Shape],
            consumed: &[bool],
            tolerance_mm: f64,
        ) -> Option<(usize, Join)> {
            let (chain_start, chain_end) = (start_of(chain), end_of(chain));
            let mut best: Option<(f64, usize, Join)> = None;
            for (index, shape) in shapes.iter().enumerate() {
                if consumed[index] || shape.polyline.is_closed() || shape.layer != layer {
                    continue;
                }
                let (start, end) = (start_of(&shape.polyline), end_of(&shape.polyline));
                let options = [
                    (chain_end.distance_to(start), Join::EndToStart),
                    (chain_end.distance_to(end), Join::EndToEnd),
                    (chain_start.distance_to(end), Join::StartToEnd),
                    (chain_start.distance_to(start), Join::StartToStart),
                ];
                for (distance, join) in options {
                    if distance <= tolerance_mm && best.is_none_or(|(d, _, _)| distance < d) {
                        best = Some((distance, index, join));
                    }
                }
            }
            best.map(|(_, index, join)| (index, join))
        }

        fn merge(left: &Polyline, right: &Polyline) -> Polyline {
            let gap = end_of(left) - start_of(right);
            let mut vertices: Vec<Vertex> = left.vertices().to_vec();
            let mut right_vertices = right.vertices().iter();
            if let (Some(last), Some(first)) = (vertices.last_mut(), right_vertices.next()) {
                last.bulge = first.bulge;
            }
            vertices.extend(right_vertices.map(|v| Vertex::new(v.x + gap.x, v.y + gap.y, v.bulge)));
            Polyline::new(vertices, false)
        }

        fn snap_closed(polyline: Polyline, tolerance_mm: f64) -> Polyline {
            if polyline.is_closed() || polyline.vertices().len() < 3 {
                return polyline;
            }
            if start_of(&polyline).distance_to(end_of(&polyline)) > tolerance_mm {
                return polyline;
            }
            let mut vertices = polyline.vertices().to_vec();
            vertices.pop();
            Polyline::new(vertices, true)
        }
    }

    /// A small fixed generator (xorshift64*), so every run draws the same
    /// cases.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        /// A number in −1 to 1.
        fn signed(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        }
    }

    /// A polyline of one to four vertices on a 5 mm grid six points a
    /// side, each moved by up to `jitter_mm` along each axis, some
    /// segments arcs; one in ten closed (given three vertices).
    fn random_polyline(rng: &mut Rng, jitter_mm: f64) -> Polyline {
        let bulges = [0.0, 0.0, 0.0, 0.5, -0.3, 1.0];
        let n = match rng.below(20) {
            0 => 1,
            1..=11 => 2,
            12..=16 => 3,
            _ => 4,
        };
        let vertices: Vec<Vertex> = (0..n)
            .map(|_| {
                Vertex::new(
                    5.0 * rng.below(6) as f64 + jitter_mm * rng.signed(),
                    5.0 * rng.below(6) as f64 + jitter_mm * rng.signed(),
                    bulges[rng.below(bulges.len())],
                )
            })
            .collect();
        let closed = n >= 3 && rng.below(10) == 0;
        Polyline::new(vertices, closed)
    }

    fn random_shapes(rng: &mut Rng, jitter_mm: f64) -> Vec<Shape> {
        let n = 2 + rng.below(60);
        (0..n)
            .map(|_| {
                let layer = ["a", "b"][rng.below(2)];
                Shape::plain(layer.into(), random_polyline(rng, jitter_mm))
            })
            .collect()
    }

    /// Ends that coincide exactly, as CAD programs write them: every gap
    /// is zero, and the stitch gives the reference's shapes to the bit, in
    /// its order, whatever joins at whichever end.
    #[test]
    fn stitch_matches_the_reference_on_coinciding_ends() {
        let mut rng = Rng(0x5EED_0001);
        let mut joins = 0;
        for case in 0..400 {
            let shapes = random_shapes(&mut rng, 0.0);
            let tolerance_mm = [0.0, 0.01, 6.0][case % 3];
            let want = reference::stitch(shapes.clone(), tolerance_mm);
            joins += shapes.len() - want.len();
            assert_eq!(
                stitched(shapes, tolerance_mm),
                want,
                "case {case}, tolerance {tolerance_mm}"
            );
        }
        assert!(joins > 4000, "the cases join little: {joins} joins");
    }

    /// Ends up to 0.0113 mm apart about the 0.01 mm tolerance: the same
    /// joins in the same order as the reference (the same shapes, each
    /// with the same vertex count, closure and bulges). Coordinates agree
    /// to the bit where a vertex was shifted by at most one gap after it
    /// joined; where it was shifted by several, the gaps are summed before
    /// they are added, which may differ from adding them one by one in the
    /// last bits, far below the 1e-9 mm asked here.
    #[test]
    fn stitch_matches_the_reference_on_ends_within_the_tolerance() {
        let mut rng = Rng(0x5EED_0002);
        let mut joins = 0;
        for case in 0..400 {
            let shapes = random_shapes(&mut rng, 0.004);
            let tolerance_mm = [0.01, 0.005, 6.0][case % 3];
            let got = stitched(shapes.clone(), tolerance_mm);
            let want = reference::stitch(shapes.clone(), tolerance_mm);
            joins += shapes.len() - want.len();
            assert_eq!(got.len(), want.len(), "case {case}");
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g.layer, w.layer, "case {case}, shape {i}");
                assert_eq!(
                    g.polyline.is_closed(),
                    w.polyline.is_closed(),
                    "case {case}, shape {i}"
                );
                assert_eq!(
                    g.polyline.vertices().len(),
                    w.polyline.vertices().len(),
                    "case {case}, shape {i}"
                );
                for (k, (p, q)) in g
                    .polyline
                    .vertices()
                    .iter()
                    .zip(w.polyline.vertices())
                    .enumerate()
                {
                    assert_eq!(
                        p.bulge.to_bits(),
                        q.bulge.to_bits(),
                        "case {case}, shape {i}, vertex {k}"
                    );
                    assert!(
                        p.point().distance_to(q.point()) <= 1e-9,
                        "case {case}, shape {i}, vertex {k}: {p:?} against {q:?}"
                    );
                }
            }
        }
        assert!(joins > 4000, "the cases join little: {joins} joins");
    }

    /// A chain joined only at its end, or once at its start, never sums
    /// gaps: it is the reference's to the bit even where ends do not
    /// coincide.
    #[test]
    fn stitch_is_exact_when_a_vertex_is_shifted_once() {
        let pieces = vec![
            line("a", (0.0, 0.0), (10.0, 0.0)),
            line("a", (10.003, 0.001), (20.0, 0.1)),
            line("a", (30.0, 0.0), (20.004, 0.097)),
            line("a", (-10.0, 0.3), (-0.002, 0.003)),
        ];
        assert_eq!(
            stitched(pieces.clone(), 0.01),
            reference::stitch(pieces, 0.01)
        );
    }

    /// The dedupe keeps the shapes the reference keeps: copies of a few
    /// shapes, some reversed, some with a vertex or a bulge moved by less
    /// than the 1e-6 it allows or by more, on two layers, some as hatches
    /// with islands.
    #[test]
    fn dedupe_matches_the_reference() {
        let mut rng = Rng(0x5EED_0003);
        let mut dropped = 0;
        for case in 0..300 {
            let pool: Vec<Polyline> = (0..6).map(|_| random_polyline(&mut rng, 0.0)).collect();
            let copy = |rng: &mut Rng| {
                let base = &pool[rng.below(pool.len())];
                let base = if rng.below(3) == 0 {
                    base.reversed()
                } else {
                    base.clone()
                };
                let moved_mm = [0.0, 0.0, 4e-7, 8e-7, 3e-6][rng.below(5)];
                let vertices = base
                    .vertices()
                    .iter()
                    .map(|v| {
                        Vertex::new(
                            v.x + moved_mm * rng.signed().signum(),
                            v.y + moved_mm * rng.signed().signum(),
                            v.bulge + [0.0, 0.0, 5e-7, 2e-6][rng.below(4)],
                        )
                    })
                    .collect();
                Polyline::new(vertices, base.is_closed())
            };
            let n = 2 + rng.below(50);
            let shapes: Vec<Shape> = (0..n)
                .map(|_| {
                    let hatch = rng.below(4) == 0;
                    let inner = if hatch {
                        (0..rng.below(3)).map(|_| copy(&mut rng)).collect()
                    } else {
                        Vec::new()
                    };
                    Shape {
                        layer: ["a", "b"][rng.below(2)].into(),
                        polyline: copy(&mut rng),
                        inner,
                        hatch,
                    }
                })
                .collect();
            let got = dedupe(shapes.clone(), &mut JoinBudget::new()).unwrap();
            let want = reference::dedupe(shapes);
            dropped += n - want.len();
            assert_eq!(got, want, "case {case}");
        }
        assert!(dropped > 500, "the cases repeat little: {dropped} dropped");
    }

    /// `chains` rows of `per_chain` one-millimetre lines end to end, row
    /// `c` along y = 10c, every third line drawn backwards, in a shuffled
    /// order, so a chain grows from somewhere in its middle at both ends.
    fn shuffled_rows(chains: usize, per_chain: usize) -> Vec<((f64, f64), (f64, f64))> {
        let mut lines: Vec<((f64, f64), (f64, f64))> = (0..chains * per_chain)
            .map(|i| {
                let (c, j) = (i / per_chain, i % per_chain);
                let (a, b) = (
                    (j as f64, 10.0 * c as f64),
                    ((j + 1) as f64, 10.0 * c as f64),
                );
                if j % 3 == 0 { (b, a) } else { (a, b) }
            })
            .collect();
        let mut rng = Rng(0x5EED_0004);
        for i in (1..lines.len()).rev() {
            lines.swap(i, rng.below(i + 1));
        }
        lines
    }

    /// Review 1.5: 100 000 separate lines that make 100 chains of 1000
    /// are deduped and stitched in at most 4 comparisons a line, where
    /// comparing every shape with every shape took 10¹⁰. Each chain comes
    /// out whole: 1001 vertices, 1000 mm long, on its own row.
    #[test]
    fn long_chains_take_a_few_comparisons_a_line() {
        let (chains, per_chain) = (100, 1000);
        let shapes: Vec<Shape> = shuffled_rows(chains, per_chain)
            .into_iter()
            .map(|(a, b)| line("0", a, b))
            .collect();
        let n = shapes.len();
        let mut budget = JoinBudget::new();
        let deduped = dedupe(shapes, &mut budget).unwrap();
        assert_eq!(deduped.len(), n);
        let out = stitch(deduped, 0.01, &mut budget).unwrap();
        assert!(
            budget.comparisons() <= 4 * n,
            "{} comparisons for {n} lines",
            budget.comparisons()
        );
        assert_eq!(out.len(), chains);
        for (i, shape) in out.iter().enumerate() {
            let p = &shape.polyline;
            assert!(!p.is_closed(), "chain {i}");
            assert_eq!(p.vertices().len(), per_chain + 1, "chain {i}");
            assert_approx_eq!(p.length_mm(), per_chain as f64, 1e-9);
            let y = p.vertices()[0].y;
            assert!(
                p.vertices().iter().all(|v| v.y.to_bits() == y.to_bits()),
                "chain {i} leaves its row"
            );
        }
    }

    /// 100 000 copies of one line are one shape, each found by one
    /// comparison with the copy kept (charged as 5: the look and both
    /// directions of its two vertices): the dedupe looks a shape up by
    /// its corner, and never walks the shapes kept.
    #[test]
    fn many_repeats_take_one_lookup_each() {
        let n = 100_000;
        let shapes: Vec<Shape> = (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    line("0", (1.0, 2.0), (3.0, 4.0))
                } else {
                    line("0", (3.0, 4.0), (1.0, 2.0))
                }
            })
            .collect();
        let mut budget = JoinBudget::new();
        let out = dedupe(shapes, &mut budget).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            budget.comparisons() <= 5 * n,
            "{} comparisons",
            budget.comparisons()
        );
    }

    /// `n` lines from the origin to a circle of 100 mm: every one's start
    /// is a candidate for every chain's start.
    fn star(n: usize) -> Vec<((f64, f64), (f64, f64))> {
        (0..n)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / n as f64;
                ((0.0, 0.0), (100.0 * angle.cos(), 100.0 * angle.sin()))
            })
            .collect()
    }

    /// Lines that all end on one point cannot be indexed apart: each
    /// chain's search still looks at every free end there. Past the
    /// budget the stitch stops with the typed error, here after 100 000
    /// comparisons of the 250 000 that 1000 lines would take.
    #[test]
    fn ends_crowded_on_one_point_past_the_budget_are_refused() {
        let shapes: Vec<Shape> = star(1000)
            .into_iter()
            .map(|(a, b)| line("0", a, b))
            .collect();
        let mut budget = JoinBudget::with_limit(100_000);
        let result = stitch(shapes.clone(), 0.01, &mut budget);
        assert!(
            matches!(
                result,
                Err(ImportError::DxfJoinTooMuchWork { limit: 100_000 })
            ),
            "{result:?}"
        );
        // Within a budget that covers it, the same star stitches: 500
        // pairs of lines joined at the centre.
        let mut budget = JoinBudget::with_limit(400_000);
        assert_eq!(stitch(shapes, 0.01, &mut budget).unwrap().len(), 500);
    }

    /// Different shapes with the same corners land in one cell of the
    /// dedupe's index, and each is compared with the ones before it; past
    /// the budget that stops with the typed error too.
    #[test]
    fn shapes_in_one_box_past_the_budget_are_refused() {
        let shapes: Vec<Shape> = (0..1000)
            .map(|i| {
                Shape::plain(
                    "0".into(),
                    Polyline::new(
                        vec![
                            Vertex::line(0.0, 0.0),
                            Vertex::line(1.0 + 0.008 * f64::from(i), 5.0),
                            Vertex::line(10.0, 10.0),
                        ],
                        false,
                    ),
                )
            })
            .collect();
        let mut budget = JoinBudget::with_limit(100_000);
        let result = dedupe(shapes.clone(), &mut budget);
        assert!(
            matches!(
                result,
                Err(ImportError::DxfJoinTooMuchWork { limit: 100_000 })
            ),
            "{result:?}"
        );
        let mut budget = JoinBudget::with_limit(10_000_000);
        assert_eq!(dedupe(shapes, &mut budget).unwrap().len(), 1000);
    }

    /// The import's budget is [`MAX_JOIN_COMPARISONS`].
    #[test]
    fn the_import_budget_is_the_stated_limit() {
        let mut budget = JoinBudget::new();
        assert!(budget.spend(MAX_JOIN_COMPARISONS).is_ok());
        assert!(matches!(
            budget.spend(1),
            Err(ImportError::DxfJoinTooMuchWork {
                limit: MAX_JOIN_COMPARISONS
            })
        ));
    }
}
