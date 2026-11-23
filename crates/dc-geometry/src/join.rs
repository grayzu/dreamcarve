// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Joining open outlines whose ends meet into chains
//! (`docs/specs/phase-5/design.md`, "Algorithms → Join"; functional
//! capability 97).
//!
//! The rule is the DXF importer's stitch (`dc-import`'s `dxf/stitch.rs`,
//! itself a port of PureCutCNC's `stitchShapes`): a chain grows from the
//! first outline in order, at either end, taking each time the nearest
//! free end within the tolerance. It differs where the user's word does
//! from a file's: a gap is bridged by a straight segment and nothing is
//! moved, where the importer moves the piece by the gap; and it is
//! written for a selection, a pass over every outline for each join,
//! where the importer's is built for a file of thousands, on a grid and
//! under a budget. The importer keeps its own.

use crate::point::{EPS_MM, Point};
use crate::polyline::Polyline;

/// What [`join_open`] made of a set of outlines.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    /// The chains, in the order of each one's first outline: several
    /// outlines end to end, or one closed on itself. Outlines that
    /// reached nothing are not here.
    pub chains: Vec<Chain>,
    /// The outlines that reached nothing and are as they were, by their
    /// place in the input.
    pub alone: Vec<usize>,
}

/// Outlines joined into one.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// The joined outline: open, or closed when its two ends met.
    pub outline: Polyline,
    /// The outlines it was made from, by their place in the input, the
    /// one it started from first.
    pub from: Vec<usize>,
}

fn first(p: &Polyline) -> Option<Point> {
    p.vertices().first().map(|v| v.point())
}

fn last(p: &Polyline) -> Option<Point> {
    p.vertices().last().map(|v| v.point())
}

/// `a` then `b`, as one open outline: where `a`'s end and `b`'s start are
/// within [`EPS_MM`] they are one vertex, else a straight segment
/// bridges them.
fn then(a: &Polyline, b: &Polyline) -> Polyline {
    let mut vertices = a.vertices().to_vec();
    let meets = last(a)
        .zip(first(b))
        .is_some_and(|(end, start)| end.distance_to(start) <= EPS_MM);
    if meets {
        vertices.pop();
    } else if let Some(end) = vertices.last_mut() {
        // An open outline's last bulge is unused: the bridge is a line.
        end.bulge = 0.0;
    }
    vertices.extend_from_slice(b.vertices());
    Polyline::new(vertices, false)
}

/// `chain` closed, when its two ends are within `tolerance_mm`: one
/// vertex where they meet within [`EPS_MM`], else a straight segment.
/// `None` when they are further apart, or it has too few vertices to
/// enclose anything.
fn closed(chain: &Polyline, tolerance_mm: f64) -> Option<Polyline> {
    let (start, end) = first(chain).zip(last(chain))?;
    let mut vertices = chain.vertices().to_vec();
    if start.distance_to(end) > tolerance_mm || vertices.len() < 3 {
        return None;
    }
    if start.distance_to(end) <= EPS_MM {
        vertices.pop();
    } else if let Some(end) = vertices.last_mut() {
        end.bulge = 0.0;
    }
    (vertices.len() >= 2).then(|| Polyline::new(vertices, true))
}

/// Joins the open outlines of `paths` whose ends lie within
/// `tolerance_mm` of each other, end to end.
///
/// A chain grows from the first free outline, in order: at each step, of
/// the free outlines with an end within the tolerance of one of the
/// chain's two ends, the nearest is joined (of equally near ones, the
/// earliest), turned round if it must be; a gap is bridged by a straight
/// segment. A chain, or an outline by itself, whose own two ends are
/// then within the tolerance is closed. Closed outlines, and those with
/// fewer than two vertices, are no part of it and are reported alone.
#[must_use]
pub fn join_open(paths: &[Polyline], tolerance_mm: f64) -> Joined {
    let usable = |p: &Polyline| !p.is_closed() && p.vertices().len() >= 2;
    let mut free: Vec<bool> = paths.iter().map(usable).collect();
    let mut chains = Vec::new();
    let mut alone = Vec::new();
    for start in 0..paths.len() {
        let Some(seed) = paths.get(start) else {
            continue;
        };
        if !free.get(start).copied().unwrap_or(false) {
            if !usable(seed) {
                alone.push(start);
            }
            continue;
        }
        if let Some(slot) = free.get_mut(start) {
            *slot = false;
        }
        let mut chain = seed.clone();
        let mut from = vec![start];
        while let (Some(head), Some(tail)) = (first(&chain), last(&chain)) {
            // The nearest free end to either end of the chain: (gap,
            // outline, joins the chain's tail, the outline's start meets
            // the chain).
            let mut best: Option<(f64, usize, bool, bool)> = None;
            for (index, path) in paths.iter().enumerate() {
                if !free.get(index).copied().unwrap_or(false) {
                    continue;
                }
                let (Some(a), Some(b)) = (first(path), last(path)) else {
                    continue;
                };
                for (gap, at_tail, by_start) in [
                    (tail.distance_to(a), true, true),
                    (tail.distance_to(b), true, false),
                    (head.distance_to(b), false, false),
                    (head.distance_to(a), false, true),
                ] {
                    if gap <= tolerance_mm && best.is_none_or(|(least, ..)| gap < least) {
                        best = Some((gap, index, at_tail, by_start));
                    }
                }
            }
            let Some((_, index, at_tail, by_start)) = best else {
                break;
            };
            let Some(path) = paths.get(index) else {
                break;
            };
            if let Some(slot) = free.get_mut(index) {
                *slot = false;
            }
            from.push(index);
            chain = match (at_tail, by_start) {
                // On from the tail, the outline as it runs or reversed.
                (true, true) => then(&chain, path),
                (true, false) => then(&chain, &path.reversed()),
                // Before the head.
                (false, false) => then(path, &chain),
                (false, true) => then(&path.reversed(), &chain),
            };
        }
        match closed(&chain, tolerance_mm) {
            Some(outline) => chains.push(Chain { outline, from }),
            None if from.len() > 1 => chains.push(Chain {
                outline: chain,
                from,
            }),
            None => alone.push(start),
        }
    }
    alone.sort_unstable();
    Joined { chains, alone }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::polyline::Vertex;

    fn line(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    fn points(path: &Polyline) -> Vec<(f64, f64)> {
        path.vertices().iter().map(|v| (v.x, v.y)).collect()
    }

    /// Ends that meet become one vertex; ends within the tolerance are
    /// bridged by a straight segment; an outline is turned round when its
    /// far end is the one that meets.
    #[test]
    fn ends_within_tolerance_join() {
        let paths = [
            line(&[(0.0, 0.0), (10.0, 0.0)]),
            line(&[(10.0, 0.0), (10.0, 10.0)]),
            // Runs the other way, and stops 0.05 short.
            line(&[(20.0, 10.0), (10.05, 10.0)]),
        ];
        let joined = join_open(&paths, 0.1);
        assert!(joined.alone.is_empty());
        assert_eq!(joined.chains.len(), 1);
        let chain = &joined.chains[0];
        assert_eq!(chain.from, [0, 1, 2]);
        assert!(!chain.outline.is_closed());
        assert_eq!(
            points(&chain.outline),
            [
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (10.05, 10.0),
                (20.0, 10.0)
            ]
        );
        // An outline that meets the chain's start goes before it.
        let paths = [
            line(&[(10.0, 0.0), (20.0, 0.0)]),
            line(&[(0.0, 0.0), (10.0, 0.0)]),
        ];
        let joined = join_open(&paths, 0.1);
        assert_eq!(joined.chains[0].from, [0, 1]);
        assert_eq!(
            points(&joined.chains[0].outline),
            [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]
        );
        // An arc keeps its bulge, and turns the other way when reversed.
        let arc = Polyline::new(
            vec![
                Vertex {
                    x: 20.0,
                    y: 0.0,
                    bulge: 0.5,
                },
                Vertex::line(10.0, 0.0),
            ],
            false,
        );
        let joined = join_open(&[line(&[(0.0, 0.0), (10.0, 0.0)]), arc], 0.1);
        let v = joined.chains[0].outline.vertices().to_vec();
        assert_eq!(v.len(), 3);
        assert_approx_eq!(v[1].bulge, -0.5, 1e-12);
    }

    /// A chain whose two ends meet closes, and so does one outline whose
    /// own ends do.
    #[test]
    fn a_chain_closes() {
        let paths = [
            line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]),
            line(&[(10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
        ];
        let joined = join_open(&paths, 0.1);
        let chain = &joined.chains[0];
        assert!(chain.outline.is_closed());
        assert_eq!(
            points(&chain.outline),
            [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        );
        assert_approx_eq!(chain.outline.signed_area_mm2(), 100.0, 1e-9);
        // One outline, its ends 0.05 apart: closed by a short segment.
        let nearly = line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.05, 0.0)]);
        let joined = join_open(&[nearly], 0.1);
        assert_eq!(joined.chains.len(), 1);
        assert_eq!(joined.chains[0].from, [0]);
        assert!(joined.chains[0].outline.is_closed());
        assert_eq!(joined.chains[0].outline.vertices().len(), 4);
    }

    /// Outlines that reach nothing are left alone and counted; so are
    /// closed ones.
    #[test]
    fn far_ends_are_left_alone() {
        let paths = [
            line(&[(0.0, 0.0), (10.0, 0.0)]),
            line(&[(10.5, 0.0), (20.0, 0.0)]),
            Polyline::rect(Point::new(30.0, 0.0), 5.0, 5.0),
            line(&[(20.0, 0.0), (20.0, 10.0)]),
        ];
        let joined = join_open(&paths, 0.1);
        // The gap of 0.5 is past the tolerance; the last two outlines
        // meet.
        assert_eq!(joined.alone, [0, 2]);
        assert_eq!(joined.chains.len(), 1);
        assert_eq!(joined.chains[0].from, [1, 3]);
        // Wider, all three open ones join, in the order they were found.
        let joined = join_open(&paths, 1.0);
        assert_eq!(joined.alone, [2]);
        assert_eq!(joined.chains[0].from, [0, 1, 3]);
        assert_eq!(join_open(&[], 0.1).chains.len(), 0);
    }
}
