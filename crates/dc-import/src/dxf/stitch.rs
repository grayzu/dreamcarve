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

use std::collections::BTreeMap;

use dc_geometry::{Point, Polyline, Vertex};

use super::entities::Shape;

/// Two vertices closer than this, mm, with bulges closer than this, are
/// the same vertex for the dedupe.
const DEDUPE_EPS: f64 = 1e-6;

/// Drops every shape that repeats an earlier one on the same layer: the
/// same vertices and bulges in order, forwards or reversed, for each of
/// its loops. The first occurrence is kept, in place. A HATCH's boundary
/// is compared only with other hatches': against a shape drawn on its own
/// it is the hatch that gives way, whichever comes first
/// ([`drop_hatch_duplicates`]).
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
    let mut drawn: BTreeMap<(usize, i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, shape) in shapes.iter().enumerate() {
        if !shape.hatch
            && shape.polyline.is_closed()
            && let Some(k) = key(&shape.polyline)
        {
            drawn.entry(k).or_default().push(i);
        }
    }
    let repeats_drawn = |hatch: &Shape, l: &Polyline| {
        let Some((n, x, y)) = key(l) else {
            return false;
        };
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                drawn.get(&(n, x + dx, y + dy)).is_some_and(|candidates| {
                    candidates.iter().any(|&i| {
                        shapes.get(i).is_some_and(|s| {
                            (!same_layer || s.layer == hatch.layer)
                                && same_loop(&s.polyline, l, tolerance_mm)
                        })
                    })
                })
            })
        })
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
            if repeats_drawn(shape, l) {
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
/// tolerance (half the chord times the bulge difference).
fn same_loop(a: &Polyline, b: &Polyline, tolerance_mm: f64) -> bool {
    let n = a.vertices().len();
    if !(a.is_closed() && b.is_closed()) || b.vertices().len() != n || n == 0 {
        return false;
    }
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
    let reversed = b.reversed();
    [b, &reversed]
        .into_iter()
        .any(|b| (0..n).any(|shift| matches_from(b, shift)))
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
pub(super) fn stitch(shapes: Vec<Shape>, tolerance_mm: f64) -> Vec<Shape> {
    let mut consumed = vec![false; shapes.len()];
    let mut out = Vec::with_capacity(shapes.len());
    for index in 0..shapes.len() {
        if consumed.get(index).copied().unwrap_or(true) {
            continue;
        }
        if let Some(done) = consumed.get_mut(index) {
            *done = true;
        }
        let Some(shape) = shapes.get(index) else {
            continue;
        };
        if shape.polyline.is_closed() {
            out.push(shape.clone());
            continue;
        }
        let mut current = shape.polyline.clone();
        while let Some((candidate, join)) =
            best_join(&current, &shape.layer, &shapes, &consumed, tolerance_mm)
        {
            if let Some(done) = consumed.get_mut(candidate) {
                *done = true;
            }
            if let Some(other) = shapes.get(candidate) {
                current = join.apply(&current, &other.polyline);
            }
        }
        out.push(Shape::plain(
            shape.layer.clone(),
            snap_closed(current, tolerance_mm),
        ));
    }
    out
}

/// Which ends of the chain and a candidate meet.
#[derive(Debug, Clone, Copy)]
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

/// The unconsumed open shape on `layer` with an end nearest to an end of
/// `chain`, within `tolerance_mm`, and how to join it.
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
        if consumed.get(index).copied().unwrap_or(true)
            || shape.polyline.is_closed()
            || shape.layer != layer
        {
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

/// `left` followed by `right`, with `right` translated so its start lands
/// on `left`'s end (upstream moves the joined piece by the gap). The last
/// vertex of `left` takes `right`'s first bulge: it now starts that segment.
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

/// Closes an open chain whose last vertex lies within `tolerance_mm` of
/// its first: the last vertex is dropped and its predecessor's segment
/// becomes the closing one, ending on the first vertex.
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;
    use dc_geometry::assert_approx_eq;

    fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> Shape {
        Shape::plain(
            layer.into(),
            Polyline::new(vec![Vertex::line(a.0, a.1), Vertex::line(b.0, b.1)], false),
        )
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
        let out = stitch(vec![arc, back], 0.01);
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
        let out = stitch(vec![a, b], 0.01);
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
        let out = stitch(vec![chain, far, near], 0.01);
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

    #[test]
    fn dedupe_keeps_first_of_equal_shapes_per_layer() {
        let shapes = vec![
            line("a", (0.0, 0.0), (10.0, 0.0)),
            line("a", (10.0, 0.0), (0.0, 0.0)),
            line("b", (0.0, 0.0), (10.0, 0.0)),
            line("a", (0.0, 0.0), (10.0, 0.0)),
        ];
        let out = dedupe(shapes);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].layer, "a");
        assert_eq!(out[1].layer, "b");
    }
}
