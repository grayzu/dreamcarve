// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Parts of a design that nearly touch, merged into one shape (#479;
//! `docs/specs/phase-3/design.md`, "Algorithms → Inlay", step 2). A V-bit
//! cannot carve a gap narrower than its program can write: the female
//! leaves a wall standing to the stock top between the two pockets, the
//! male cuts no groove between the two parts of its plug, and the plug
//! stands proud on the wall. Merging the parts on both sides makes the
//! two agree: one pocket, one plug. It lives here so the safety pass
//! re-derives the merged design without calling a generator.

use crate::boolean::{difference, union};
use crate::carve::{Bays, closer_than, floor_regions, grown_contour, segment_box, single_region};
use crate::contour::Contour;
use crate::distance::{closest_points, segment_distance_mm};
use crate::offset::offset;
use crate::point::{EPS_MM, Point, Rect};
use crate::polyline::{Polyline, Segment};
use crate::region::{Region, lies_inside};

/// Two parts of a design that come closer than the merge distance, across
/// a gap outside the design ([`merge_near_parts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct NearParts {
    /// The input position of one part's outline that comes near
    /// ([`Region::outer_index`] or [`Region::hole_indices`]), the smaller
    /// of the two.
    pub first: usize,
    /// The input position of the other's.
    pub second: usize,
    /// The middle of the gap where the two come nearest, mm.
    pub at: Point,
    /// The gap there, mm.
    pub gap_mm: f64,
}

/// A design with its near parts merged ([`merge_near_parts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MergedParts {
    /// The regions: each region with no near part as it was given, and
    /// each group of near parts as the region (rarely regions) they merge
    /// into, in place of the group's first.
    pub regions: Vec<Region>,
    /// For each of `regions`, the positions in the input of the regions
    /// it came from: one for a region kept as it was, the group for a
    /// merged one.
    pub sources: Vec<Vec<usize>>,
    /// Every pair of outlines that come near, in input order.
    pub near: Vec<NearParts>,
    /// The holes filled (#511): each narrower than the merge distance
    /// everywhere, in input order.
    pub filled: Vec<FilledHole>,
}

/// A hole of a design narrower than the merge distance everywhere, filled
/// ([`merge_near_parts`], #511): the design's material on either side of it
/// comes closer than the distance, as near parts do across a gap.
#[derive(Debug, Clone, PartialEq)]
pub struct FilledHole {
    /// The hole's input position ([`Region::hole_indices`]).
    pub index: usize,
    /// The middle of its box, mm.
    pub at: Point,
    /// Its width, mm: twice its area over its perimeter, which for a
    /// sliver is its mean width.
    pub width_mm: f64,
}

/// `regions` (nested by [`crate::nest`]) with every two parts that come
/// closer than `distance_mm` across a gap outside the design merged into
/// one shape, and the pairs of outlines that do. A gap is outside the
/// design between two outers side by side (in the same hole, or in none)
/// and between a hole and an island directly inside it; two holes, or a
/// hole and its own region's outer, have the design's material between
/// them, which is left as it is. Outlines that touch (within [`EPS_MM`])
/// are not near: they are separate parts, as [`crate::nest`] takes them.
///
/// A group of near parts is closed with round joins by half the distance
/// (`(X ⊕ r) ⊖ r`), and only the pieces the closing adds that lie against
/// two of the group's outlines are kept: the gaps between the parts. Each
/// is grown a little into the parts and held to the closing, so it
/// overlaps them, and united with them; the slivers the booleans leave
/// where outlines run a rounding error apart are dropped. The pieces
/// against one outline, an inward corner rounded, are not kept, so the
/// parts keep their shape everywhere but at the gap. Where the two
/// outlines part, the closing ends the fill with an arc of radius `r`;
/// its vertices off the parts are cut to the one farthest from them, so
/// the fill ends in a corner, which a carve's medial axis reaches (a
/// round end, at some gaps, left a male's V-bit short of the narrow
/// wedge beyond it). Where a gap `g` wide opens between two outward
/// corners, the closing's arc dips `r − √(r² − (g/2)²)` into it, under
/// a thousandth of a millimetre for gaps under 0.01 mm at the usual
/// distance. The group comes back
/// polygonal, its arcs flattened within `tolerance_mm` or an eighth of
/// the distance, whichever is finer, so the flattening cannot open a gap
/// the distance closes; regions with no near part keep their arcs.
///
/// First, a hole narrower than `distance_mm` all along that holds no other
/// region is filled (#511; [`MergedParts::filled`]): the design's material
/// either side of it comes that near across it.
/// Nothing is merged when `distance_mm` is not a positive finite number.
#[must_use]
pub fn merge_near_parts(regions: &[Region], distance_mm: f64, tolerance_mm: f64) -> MergedParts {
    if !(distance_mm.is_finite() && distance_mm > 0.0) {
        return unchanged_of(regions);
    }
    let (filled_regions, filled) = fill_thin_holes(regions, distance_mm, tolerance_mm);
    let regions = filled_regions.as_slice();
    let near = near_parts(regions, distance_mm);
    if near.is_empty() {
        return MergedParts {
            filled,
            ..unchanged_of(regions)
        };
    }
    // Which region each input outline belongs to.
    let region_of = |index: usize| {
        regions
            .iter()
            .position(|r| r.outer_index() == index || r.hole_indices().contains(&index))
    };
    let mut group: Vec<usize> = (0..regions.len()).collect();
    for pair in &near {
        if let (Some(a), Some(b)) = (region_of(pair.first), region_of(pair.second)) {
            let (ra, rb) = (root(&mut group, a), root(&mut group, b));
            if let Some(g) = group.get_mut(ra.max(rb)) {
                *g = ra.min(rb);
            }
        }
    }
    let fine_mm = if tolerance_mm.is_finite() && tolerance_mm > 0.0 {
        tolerance_mm.min(distance_mm / 8.0)
    } else {
        distance_mm / 8.0
    };
    let mut out = MergedParts {
        regions: Vec::new(),
        sources: Vec::new(),
        near,
        filled,
    };
    for k in 0..regions.len() {
        let first = root(&mut group, k);
        if first != k {
            continue;
        }
        let members: Vec<usize> = (0..regions.len())
            .filter(|&m| root(&mut group, m) == k)
            .collect();
        let parts: Vec<Region> = members
            .iter()
            .filter_map(|&m| regions.get(m).cloned())
            .collect();
        if parts.len() < 2 {
            out.regions.extend(parts);
            out.sources.push(members);
            continue;
        }
        for region in merged(&parts, distance_mm, fine_mm) {
            out.regions.push(region);
            out.sources.push(members.clone());
        }
    }
    out
}

/// `regions` as they are, each its own source.
fn unchanged_of(regions: &[Region]) -> MergedParts {
    MergedParts {
        regions: regions.to_vec(),
        sources: (0..regions.len()).map(|k| vec![k]).collect(),
        near: Vec::new(),
        filled: Vec::new(),
    }
}

/// `regions` with every hole narrower than `distance_mm` everywhere
/// filled (#511), and the holes filled: a hole whose own area, inset by
/// half the distance, is empty, and that holds no other region (an island
/// in it keeps its hole). A V-bit cannot carve such a sliver: the female
/// leaves a wall to the stock top there no wider than the hole, the
/// male's groove keeps half a written unit from each wall and is dropped,
/// and the wall holds the plug up. Filled, both sides carve the shape as
/// one, as near parts are merged across a gap.
fn fill_thin_holes(
    regions: &[Region],
    distance_mm: f64,
    tolerance_mm: f64,
) -> (Vec<Region>, Vec<FilledHole>) {
    let mut filled = Vec::new();
    let out = regions
        .iter()
        .map(|region| {
            let drop: Vec<usize> = region
                .holes()
                .iter()
                .enumerate()
                .filter(|(_, hole)| {
                    let Some(inside) = single_region(hole.reversed()) else {
                        return false;
                    };
                    let thin = floor_regions(
                        std::slice::from_ref(&inside),
                        distance_mm / 2.0,
                        tolerance_mm,
                    )
                    .is_empty();
                    let holds_a_region = regions.iter().any(|r| {
                        r != region
                            && r.outer()
                                .polyline()
                                .vertices()
                                .first()
                                .is_some_and(|v| inside.contains_point(v.point()))
                    });
                    thin && !holds_a_region
                })
                .map(|(k, _)| k)
                .collect();
            for &k in &drop {
                let (Some(hole), Some(&index)) =
                    (region.holes().get(k), region.hole_indices().get(k))
                else {
                    continue;
                };
                let polyline = hole.polyline();
                let perimeter_mm = polyline.length_mm();
                let at = polyline.bounds().map_or(Point::new(0.0, 0.0), |b| {
                    Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
                });
                filled.push(FilledHole {
                    index,
                    at,
                    width_mm: if perimeter_mm > 0.0 {
                        2.0 * hole.signed_area_mm2().abs() / perimeter_mm
                    } else {
                        0.0
                    },
                });
            }
            if drop.is_empty() {
                region.clone()
            } else {
                region.without_holes(&drop)
            }
        })
        .collect();
    filled.sort_by_key(|h| h.index);
    (out, filled)
}

/// The group `k` is in: the first region of it, following `group` to a
/// region that is its own, and pointing `k` there.
fn root(group: &mut [usize], k: usize) -> usize {
    let mut r = k;
    while let Some(&up) = group.get(r) {
        if up == r {
            break;
        }
        r = up;
    }
    if let Some(g) = group.get_mut(k) {
        *g = r;
    }
    r
}

/// One outline of a region, for [`near_parts`].
struct Outline<'r> {
    /// The region it bounds, by position.
    region: usize,
    /// Its input position.
    index: usize,
    contour: &'r Contour,
    /// Whether it is a hole of its region.
    hole: bool,
    bounds: Option<Rect>,
}

/// The pairs of `regions`' outlines that come closer than `distance_mm`,
/// and further than [`EPS_MM`], across a gap outside the design
/// ([`merge_near_parts`]).
fn near_parts(regions: &[Region], distance_mm: f64) -> Vec<NearParts> {
    let outlines: Vec<Outline<'_>> = regions
        .iter()
        .enumerate()
        .flat_map(|(region, r)| {
            std::iter::once((r.outer(), r.outer_index(), false))
                .chain(
                    r.holes()
                        .iter()
                        .zip(r.hole_indices())
                        .map(|(h, &i)| (h, i, true)),
                )
                .map(move |(contour, index, hole)| Outline {
                    region,
                    index,
                    contour,
                    hole,
                    bounds: contour.polyline().bounds(),
                })
        })
        .collect();
    // The hole each region lies directly in, as (region, hole position),
    // or `None` for a part inside nothing.
    let container: Vec<Option<(usize, usize)>> = regions
        .iter()
        .map(|r| {
            let parent_depth = r.depth().checked_sub(2)?;
            regions.iter().enumerate().find_map(|(j, p)| {
                if p.depth() != parent_depth {
                    return None;
                }
                p.holes()
                    .iter()
                    .position(|h| lies_inside(r.outer(), h))
                    .map(|h| (j, h))
            })
        })
        .collect();
    let hole_of = |o: &Outline<'_>| {
        regions
            .get(o.region)
            .and_then(|r| r.hole_indices().iter().position(|&i| i == o.index))
            .map(|h| (o.region, h))
    };
    let mut near = Vec::new();
    for (i, a) in outlines.iter().enumerate() {
        for b in outlines.iter().skip(i + 1) {
            if a.region == b.region || !boxes_within(a.bounds, b.bounds, distance_mm) {
                continue;
            }
            let container_of = |o: &Outline<'_>| container.get(o.region).copied().flatten();
            let outside = match (a.hole, b.hole) {
                (false, false) => container_of(a) == container_of(b),
                (true, false) => container_of(b).is_some() && container_of(b) == hole_of(a),
                (false, true) => container_of(a).is_some() && container_of(a) == hole_of(b),
                (true, true) => false,
            };
            if !outside {
                continue;
            }
            let Some((p, q)) = nearest(a.contour, b.contour, distance_mm) else {
                continue;
            };
            let gap_mm = p.distance_to(q);
            if gap_mm > EPS_MM && gap_mm < distance_mm {
                near.push(NearParts {
                    first: a.index.min(b.index),
                    second: a.index.max(b.index),
                    at: Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
                    gap_mm,
                });
            }
        }
    }
    near.sort_by_key(|n| (n.first, n.second));
    near
}

/// Whether boxes `a` and `b` come within `reach_mm` of each other.
fn boxes_within(a: Option<Rect>, b: Option<Rect>, reach_mm: f64) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    a.min.x - reach_mm <= b.max.x
        && b.min.x - reach_mm <= a.max.x
        && a.min.y - reach_mm <= b.max.y
        && b.min.y - reach_mm <= a.max.y
}

/// The nearest points of `a` and `b`, the first on `a`, following arcs,
/// when they come within `reach_mm`.
fn nearest(a: &Contour, b: &Contour, reach_mm: f64) -> Option<(Point, Point)> {
    let candidates: Vec<(Segment, Rect)> = b
        .polyline()
        .segments()
        .map(|t| (t, segment_box(t)))
        .collect();
    let mut best: Option<(f64, Segment, Segment)> = None;
    for s in a.polyline().segments() {
        let s_box = segment_box(s);
        for &(t, t_box) in &candidates {
            if !boxes_within(Some(s_box), Some(t_box), reach_mm) {
                continue;
            }
            let gap_mm = segment_distance_mm(s, t);
            if gap_mm < reach_mm && best.is_none_or(|(b, ..)| gap_mm < b) {
                best = Some((gap_mm, s, t));
            }
        }
    }
    best.map(|(_, s, t)| closest_points(s, t))
}

/// `parts`, a group of near parts, merged into one shape: united with the
/// pieces their closing by `distance_mm / 2` adds that lie against two
/// of their outlines, within `fine_mm` ([`merge_near_parts`]). The closing
/// grows by `fine_mm` more, as the chords the booleans flatten arcs to
/// lie up to that inside them.
fn merged(parts: &[Region], distance_mm: f64, fine_mm: f64) -> Vec<Region> {
    let radius_mm = distance_mm / 2.0 + fine_mm;
    let grown: Vec<Region> = parts
        .iter()
        .flat_map(|r| dilated(r, radius_mm, fine_mm))
        .collect();
    let closed = floor_regions(&union(&grown, &[], fine_mm), radius_mm, fine_mm);
    let outlines: Vec<&Contour> = parts
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .collect();
    let reach_mm = 2.0 * fine_mm;
    // The pieces the closing adds against two outlines: the gaps. Each is
    // grown by `fine_mm` into the parts, and held to the closing, so it
    // overlaps them: one that only met them, along edges the booleans
    // place a rounding error apart, might not join them.
    let gaps: Vec<Region> = difference(&closed, parts, fine_mm)
        .into_iter()
        .filter(|piece| {
            let against = outlines
                .iter()
                .filter(|o| closer_than(piece.outer().polyline(), o.polyline(), reach_mm))
                .count();
            against >= 2
        })
        .flat_map(|piece| grown_contour(piece.outer(), fine_mm, Bays::Island))
        .collect();
    if gaps.is_empty() {
        return parts.to_vec();
    }
    let bridges = crate::boolean::intersection(&gaps, &closed, fine_mm);
    let united = without_slivers(&union(parts, &bridges, fine_mm), fine_mm);
    let flattened: Vec<Polyline> = outlines
        .iter()
        .map(|o| o.polyline().with_arcs_flattened(fine_mm))
        .collect();
    sharpened(&united, &flattened).unwrap_or(united)
}

/// `regions` less the slivers the booleans leave where the closing's
/// outline and the parts' run a rounding error apart: a region, or a
/// hole, whose area is at most `fine_mm` times its outline's length (on
/// average no more than `2·fine_mm` wide, far narrower than any gap the
/// merge leaves open). Left in, a sliver hole is a wall inside the merged
/// part that the carve must keep clear of.
fn without_slivers(regions: &[Region], fine_mm: f64) -> Vec<Region> {
    let real = |c: &Contour| c.signed_area_mm2().abs() > fine_mm * c.polyline().length_mm();
    let contours: Vec<Contour> = regions
        .iter()
        .filter(|r| real(r.outer()))
        .flat_map(|r| {
            std::iter::once(r.outer().clone()).chain(r.holes().iter().filter(|h| real(h)).cloned())
        })
        .collect();
    match crate::region::nest(contours) {
        Ok((cleaned, _)) => cleaned,
        Err(_) => regions.to_vec(),
    }
}

/// How far a vertex of a merged outline may lie off the parts' outlines,
/// as the booleans flattened them, and still be theirs, mm: well above
/// the booleans' rounding, well below the closing's arc.
const ON_PART_MM: f64 = 1e-5;

/// The shortest edge a merged outline keeps, mm: a vertex nearer the one
/// before goes.
const MIN_EDGE_MM: f64 = 1e-4;

/// `regions`, a group of parts merged, with each run of outline vertices
/// off the parts' `outlines` (flattened as the booleans flattened them;
/// further than [`ON_PART_MM`]) cut to the one vertex of it farthest from
/// them. Such a run is the arc the closing ends a gap's fill with; cut to
/// one vertex, the fill ends in a corner, which the medial axis reaches
/// (`medial::CornerReach::Every`): a male's V-bit left the narrow wedge
/// beyond a round end uncut, at some gaps, and the plug stood proud on
/// it. `None` when the result does not nest.
fn sharpened(regions: &[Region], outlines: &[Polyline]) -> Option<Vec<Region>> {
    let off_mm = ON_PART_MM;
    let away = |p: Point| {
        outlines
            .iter()
            .filter_map(|o| crate::distance::nearest_point(o, p))
            .map(|n| n.gap_mm)
            .fold(f64::INFINITY, f64::min)
    };
    let mut contours = Vec::new();
    for region in regions {
        for contour in std::iter::once(region.outer()).chain(region.holes()) {
            let vertices = contour.polyline().vertices();
            let gaps: Vec<f64> = vertices.iter().map(|v| away(v.point())).collect();
            let Some(start) = gaps.iter().position(|&g| g <= off_mm) else {
                contours.push(contour.clone());
                continue;
            };
            let n = vertices.len();
            let mut kept = Vec::with_capacity(n);
            let mut run: Option<usize> = None;
            for k in 0..=n {
                let i = (start + k) % n;
                let (Some(&v), Some(&g)) = (vertices.get(i), gaps.get(i)) else {
                    continue;
                };
                if g > off_mm && k < n {
                    let farther = run.is_none_or(|r| gaps.get(r).is_some_and(|&rg| g > rg));
                    if farther {
                        run = Some(i);
                    }
                    continue;
                }
                if let Some(r) = run.take()
                    && let Some(&rv) = vertices.get(r)
                {
                    kept.push(rv);
                }
                if k < n {
                    kept.push(v);
                }
            }
            let mut spaced: Vec<crate::polyline::Vertex> = Vec::with_capacity(kept.len());
            for v in kept {
                if spaced
                    .last()
                    .is_none_or(|l| l.point().distance_to(v.point()) > MIN_EDGE_MM)
                {
                    spaced.push(v);
                }
            }
            while spaced.len() > 3
                && spaced
                    .first()
                    .zip(spaced.last())
                    .is_some_and(|(f, l)| f.point().distance_to(l.point()) <= MIN_EDGE_MM)
            {
                spaced.pop();
            }
            contours.push(Contour::new(Polyline::new(spaced, true)).ok()?);
        }
    }
    crate::region::nest(contours).ok().map(|(r, _)| r)
}

/// `region` grown by `radius_mm` with round joins: its outer grown, a bay
/// it closes kept out ([`grown_contour`]), less its holes shrunk, within
/// `tolerance_mm`.
fn dilated(region: &Region, radius_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outer = grown_contour(region.outer(), radius_mm, Bays::Floor);
    let holes: Vec<Region> = region
        .holes()
        .iter()
        .flat_map(|h| offset(h, -radius_mm))
        .filter_map(single_region)
        .collect();
    if holes.is_empty() {
        outer
    } else {
        difference(&outer, &holes, tolerance_mm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{area_mm2, nest};

    /// The merge distance, mm (`dc_project::INLAY_MERGE_MM`).
    const MERGE_MM: f64 = 0.05;
    const TOL: f64 = crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(x, y), w, h)).expect("a rectangle")
    }

    fn nested(contours: Vec<Contour>) -> Vec<Region> {
        nest(contours).expect("it nests").0
    }

    /// Two 10 mm squares 0.001 mm apart merge into one 20.001 × 10 mm
    /// region, reported once, in the middle of the gap; their corners
    /// stay sharp. 0.1 mm apart they are left as they were.
    #[test]
    fn squares_a_hair_apart_merge() {
        let regions = nested(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(10.001, 0.0, 10.0, 10.0),
        ]);
        let merged = merge_near_parts(&regions, MERGE_MM, TOL);
        assert_eq!(merged.regions.len(), 1, "{merged:?}");
        assert_eq!(merged.sources, vec![vec![0, 1]]);
        assert!(merged.regions[0].holes().is_empty());
        crate::assert_approx_eq!(merged.regions[0].area_mm2(), 200.01, 1e-5);
        let b = merged.regions[0]
            .outer()
            .polyline()
            .bounds()
            .expect("bounds");
        crate::assert_approx_eq!(b.min.x, 0.0, 1e-6);
        crate::assert_approx_eq!(b.max.x, 20.001, 1e-6);
        assert_eq!(merged.near.len(), 1);
        let near = &merged.near[0];
        assert_eq!((near.first, near.second), (0, 1));
        crate::assert_approx_eq!(near.gap_mm, 0.001, 1e-9);
        crate::assert_approx_eq!(near.at.x, 10.0005, 1e-9);

        let apart = nested(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(10.1, 0.0, 10.0, 10.0),
        ]);
        let kept = merge_near_parts(&apart, MERGE_MM, TOL);
        assert_eq!(kept.regions, apart);
        assert!(kept.near.is_empty());
    }

    /// A 3 mm circle 0.005 mm from a square merges with it at the pinch:
    /// the merged area is the two shapes' (within the flattening) plus the
    /// sliver between them, and no more than a few thousandths of a mm².
    /// The near point is on the line between the circle's centre and the
    /// square's side. Unmerged regions keep their arcs.
    #[test]
    fn circle_at_a_pinch_merges() {
        let circle = Contour::new(Polyline::circle(Point::new(-3.0, 0.0), 3.0)).expect("circle");
        let other = Contour::new(Polyline::circle(Point::new(-30.0, 0.0), 3.0)).expect("circle");
        let regions = nested(vec![circle, rect(0.005, -5.0, 10.0, 10.0), other]);
        let merged = merge_near_parts(&regions, MERGE_MM, TOL);
        assert_eq!(merged.regions.len(), 2, "{merged:?}");
        assert_eq!(merged.sources, vec![vec![0, 1], vec![2]]);
        assert_eq!(merged.regions[1], regions[2], "kept with its arcs");
        let shapes_mm2 = std::f64::consts::PI * 9.0 + 100.0;
        let area = merged.regions[0].area_mm2();
        assert!(
            area > shapes_mm2 - 0.05 && area < shapes_mm2 + 0.001,
            "{area} against {shapes_mm2}"
        );
        assert_eq!(merged.near.len(), 1);
        crate::assert_approx_eq!(merged.near[0].gap_mm, 0.005, 1e-9);
        crate::assert_approx_eq!(merged.near[0].at.x, 0.0025, 1e-9);
        crate::assert_approx_eq!(merged.near[0].at.y, 0.0, 1e-9);
        // The merged shape covers the pinch.
        assert!(merged.regions[0].contains_point(Point::new(0.0025, 0.0)));
    }

    /// The fill between a 3 mm circle and a square 0.01 mm off ends, at
    /// each side, in one corner off the two outlines, where the closing's
    /// arc was: every other vertex of the merged outline is on the parts
    /// as the booleans flatten them.
    #[test]
    fn merging_ends_a_gap_in_a_corner() {
        let circle = Contour::new(Polyline::circle(Point::new(-3.0, 0.0), 3.0)).expect("circle");
        let regions = nested(vec![circle, rect(0.01, -5.0, 10.0, 10.0)]);
        let merged = merge_near_parts(&regions, MERGE_MM, TOL);
        assert_eq!(merged.regions.len(), 1, "{merged:?}");
        let fine_mm = MERGE_MM / 8.0;
        let flattened: Vec<Polyline> = regions
            .iter()
            .map(|r| r.outer().polyline().with_arcs_flattened(fine_mm))
            .collect();
        let off: Vec<Point> = merged.regions[0]
            .outer()
            .polyline()
            .vertices()
            .iter()
            .map(|v| v.point())
            .filter(|&p| {
                flattened
                    .iter()
                    .filter_map(|o| crate::distance::nearest_point(o, p))
                    .all(|n| n.gap_mm > ON_PART_MM)
            })
            .collect();
        assert_eq!(off.len(), 2, "{off:?}");
        for p in off {
            assert!(p.x > -0.02 && p.x < 0.02, "{p:?}");
            assert!(p.y.abs() > 0.3 && p.y.abs() < 0.8, "{p:?}");
        }
    }

    /// An island 0.002 mm inside its hole's edge merges with the part
    /// round the hole: the gap fills, with the closing's fillets where the
    /// island's sides meet it, about a thousandth of a mm². An
    /// island well inside a hole, and the hole's own part 0.002 mm from
    /// the hole (the design's material between them), are left as they
    /// were.
    #[test]
    fn island_near_its_hole_merges_and_a_thin_web_does_not() {
        let regions = nested(vec![
            rect(0.0, 0.0, 40.0, 20.0),
            rect(2.0, 2.0, 10.0, 10.0),
            rect(4.0, 2.002, 4.0, 4.0),
            rect(20.0, 2.0, 10.0, 17.998),
            rect(22.0, 4.0, 4.0, 4.0),
        ]);
        assert_eq!(regions.len(), 3, "a plate and two islands");
        let merged = merge_near_parts(&regions, MERGE_MM, TOL);
        assert_eq!(merged.near.len(), 1, "{:?}", merged.near);
        assert_eq!((merged.near[0].first, merged.near[0].second), (1, 2));
        assert_eq!(merged.regions.len(), 2, "{merged:?}");
        assert_eq!(merged.sources, vec![vec![0, 1], vec![2]]);
        let expected = 800.0 - 100.0 - 179.98 + 16.0 + 4.0 * 0.002;
        crate::assert_approx_eq!(area_mm2(&merged.regions[..1]), expected, 2e-3);
        assert_eq!(merged.regions[1], regions[2]);
    }

    /// Outlines that touch are separate parts, as `nest` takes them, and
    /// a distance that is not positive merges nothing.
    #[test]
    fn touching_parts_and_no_distance_merge_nothing() {
        let regions = nested(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(10.0, 0.0, 10.0, 10.0),
        ]);
        assert_eq!(merge_near_parts(&regions, MERGE_MM, TOL).regions, regions);
        let near = nested(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(10.001, 0.0, 10.0, 10.0),
        ]);
        assert_eq!(merge_near_parts(&near, 0.0, TOL).regions, near);
        assert_eq!(merge_near_parts(&near, f64::NAN, TOL).regions, near);
    }

    /// An L beside a square 0.01 mm off: the gap fills, but the L's
    /// inward corner away from the gap keeps its sharp point. Where the
    /// gap opens between two outward corners, the closing's arc dips
    /// `r − √(r² − (g/2)²)` (0.001 mm) into it.
    #[test]
    fn merging_keeps_inward_corners_away_from_the_gap() {
        let l_shape = Contour::new(Polyline::new(
            [
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 5.0),
                (5.0, 5.0),
                (5.0, 10.0),
                (0.0, 10.0),
            ]
            .iter()
            .map(|&(x, y)| crate::polyline::Vertex::line(x, y))
            .collect(),
            true,
        ))
        .expect("an L");
        let regions = nested(vec![l_shape, rect(10.01, 0.0, 5.0, 5.0)]);
        let merged = merge_near_parts(&regions, MERGE_MM, TOL);
        assert_eq!(merged.regions.len(), 1, "{merged:?}");
        let corner = merged.regions[0]
            .outer()
            .polyline()
            .vertices()
            .iter()
            .any(|v| (v.x - 5.0).abs() < 1e-6 && (v.y - 5.0).abs() < 1e-6);
        assert!(corner, "the inward corner is kept: {merged:?}");
        crate::assert_approx_eq!(area_mm2(&merged.regions), 75.0 + 25.0 + 0.05, 1e-4);
    }
}
