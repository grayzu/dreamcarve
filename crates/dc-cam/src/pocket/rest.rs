// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/restRegions.ts
//   src/engine/toolpaths/finishFloorCoverage.ts

//! Rest machining (`docs/specs/phase-2/design.md`, "Algorithms → Pocket",
//! step 3): what an earlier Pocket of the same region left, and where this
//! operation's tool centre must go to clear it.
//!
//! The earlier Pocket is known only by its tool radius `R` and stock to leave
//! `A`, read from the project; its toolpath is never trusted (S14). The
//! area it reached is its tool-centre region (the region inset by `R + A`)
//! grown by `R`, as upstream `generateAreaRestRegionDrafts` builds it
//! (`buildInsetRegions` by `toolRadius + radialLeave`, then a round offset by
//! `toolRadius`). What it left is the region, less this operation's own
//! stock to leave, minus that.
//!
//! Upstream turns the leftover into draft features (corner triangles and
//! residual blobs) that a later operation pockets as regions of their own.
//! Here the leftover only selects tool centres: this operation's own
//! tool-centre region (the region inset by `r + a`) keeps the points within
//! `r` of the leftover, the positions from which the tool touches material
//! that is still there. Every cut then stays inside the operation's own
//! region whatever the reference says, and the leftover's edge against the
//! cleared area is not a wall the tool must stand off (upstream's
//! `finishFloorCoverage.ts` intersects a floor root with its coverage for
//! the same reason).
//!
//! Boolean results carry rounding and flattening slivers. A piece whose
//! area is under `tolerance × perimeter` (a strip thinner than twice the
//! tolerance) is noise, not material, and is dropped: upstream's
//! `restrictFloorRoot` noise rule, in mm.

use super::rings::inset_region;
use dc_geometry::{Contour, Region, difference, distance_mm, intersection, nest, offset, union};

/// The Pocket a rest operation rests on, as far as rest machining needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reference {
    /// Its tool's radius, mm.
    pub radius_mm: f64,
    /// Its `stock_to_leave_mm`.
    pub stock_to_leave_mm: f64,
}

/// `region` grown by `by_mm` (≥ 0): its outer offset outward and its holes
/// shrunk, holes that vanish dropped. Polygonal, as booleans return it.
///
/// An outer shaped like a C whose mouth is narrower than `2 · by_mm` grows
/// over its mouth; the bay inside it, the points more than `by_mm` from
/// the region, is a hole of the result (#493).
#[must_use]
pub fn grow_region(region: &Region, by_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outers = grown_outer(region.outer(), by_mm);
    // `offset` keeps a hole's clockwise winding and shrinks it for a
    // negative delta.
    let holes: Vec<Region> = region
        .holes()
        .iter()
        .flat_map(|hole| offset(hole, -by_mm))
        .filter_map(|c| single_region(c.reversed()))
        .collect();
    difference(&outers, &holes, tolerance_mm)
}

/// The area `outer` encloses grown by `by_mm`, as regions. When the outer
/// grows over a bay's mouth, `offset` gives back its outline and a loop
/// round the bay, both counter-clockwise; nested, the bay is a hole of the
/// outline's region. Before #493 each loop was a region of its own, so a
/// tool's reach took in a bay its centre never came within its radius of.
fn grown_outer(outer: &Contour, by_mm: f64) -> Vec<Region> {
    let loops = offset(outer, by_mm);
    if loops.len() > 1
        && let Ok((regions, _)) = nest(loops.clone())
    {
        return regions;
    }
    loops.into_iter().filter_map(single_region).collect()
}

/// The union of every region of `regions` grown by `by_mm`.
pub(super) fn grow_all(regions: &[Region], by_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let grown: Vec<Region> = regions
        .iter()
        .flat_map(|r| grow_region(r, by_mm, tolerance_mm))
        .collect();
    if grown.is_empty() {
        return grown;
    }
    union(&grown, &[], tolerance_mm)
}

/// The area of `region` the reference's tool reached: its tool-centre
/// region grown by its radius.
#[must_use]
pub fn reachable(region: &Region, reference: Reference, tolerance_mm: f64) -> Vec<Region> {
    let centre = inset_region(
        region,
        reference.radius_mm + reference.stock_to_leave_mm,
        tolerance_mm,
    );
    grow_all(&centre, reference.radius_mm, tolerance_mm)
}

/// The material of `region` the references left that this operation, at
/// `stock_to_leave_mm`, should remove: the region less the stock to leave,
/// minus what any of them reached ([`reachable`]), noise dropped. Empty when
/// they left nothing. A `rest_of` Pocket has one reference; a carve's
/// clearing pass has every pass before it (#406), and each reached what
/// it reached whether or not the passes between cut there, so the
/// leftover is the same as cutting them in turn.
#[must_use]
pub fn left_by(
    region: &Region,
    stock_to_leave_mm: f64,
    references: &[Reference],
    tolerance_mm: f64,
) -> Vec<Region> {
    let material = if stock_to_leave_mm > 0.0 {
        inset_region(region, stock_to_leave_mm, tolerance_mm)
    } else {
        vec![region.clone()]
    };
    let reached: Vec<Region> = references
        .iter()
        .flat_map(|&reference| reachable(region, reference, tolerance_mm))
        .collect();
    without_noise(difference(&material, &reached, tolerance_mm), tolerance_mm)
}

/// The tool-centre region of a rest operation: the points of `own_centre`
/// (this operation's tool-centre region of the pocket) within `radius_mm`
/// of `left` (from [`left_by`]), noise dropped. Empty when the tool reaches
/// none of what is left.
#[must_use]
pub fn rest_centre(
    own_centre: &[Region],
    left: &[Region],
    radius_mm: f64,
    tolerance_mm: f64,
) -> Vec<Region> {
    if left.is_empty() || own_centre.is_empty() {
        return Vec::new();
    }
    let touching = grow_all(left, radius_mm, tolerance_mm);
    without_noise(
        intersection(own_centre, &touching, tolerance_mm),
        tolerance_mm,
    )
}

/// Whether a tool of radius `radius_mm` whose centre runs along `ring`
/// cuts none of `left` (from [`left_by`]), so its whole swept area is
/// material the reference already removed (#167): `ring` lies outside every
/// piece of `left` and at least `radius_mm − tolerance_mm` from each. The
/// tolerance allows for the flattened edge of [`rest_centre`]'s boolean
/// result, whose chords come up to the tolerance nearer the leftover than
/// the round offset they stand for; a ring dropped for it would leave a
/// strip under the tolerance thick, noise by the rule below.
///
/// Only a ring on the edge of a rest tool-centre region can be in air,
/// where that edge is the leftover grown by `radius_mm` (the band's inner
/// edge against the cleared area): every point inside the region is nearer
/// than that to the leftover.
#[must_use]
pub fn in_air(ring: &Contour, left: &[Region], radius_mm: f64, tolerance_mm: f64) -> bool {
    let clear_mm = radius_mm - tolerance_mm;
    if clear_mm.is_nan() || clear_mm <= 0.0 {
        return false;
    }
    let Some(on_ring) = ring.polyline().vertices().first().map(|v| v.point()) else {
        return false;
    };
    left.iter().all(|piece| {
        // Far from every edge of the piece, the ring is wholly inside or
        // wholly outside it; one point tells which.
        !piece.contains_point(on_ring)
            && std::iter::once(piece.outer())
                .chain(piece.holes())
                .all(|edge| distance_mm(ring.polyline(), edge.polyline()) >= clear_mm)
    })
}

/// `regions` without the pieces thinner than twice `tolerance_mm`: area
/// under `tolerance_mm` times the perimeter.
fn without_noise(regions: Vec<Region>, tolerance_mm: f64) -> Vec<Region> {
    regions
        .into_iter()
        .filter(|r| {
            let perimeter_mm: f64 = std::iter::once(r.outer())
                .chain(r.holes())
                .map(|c| c.polyline().length_mm())
                .sum();
            r.area_mm2() > tolerance_mm * perimeter_mm
        })
        .collect()
}

/// The region `contour` alone encloses.
fn single_region(contour: Contour) -> Option<Region> {
    nest(vec![contour])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, area_mm2, assert_approx_eq};

    const TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Region {
        single_region(Contour::new(Polyline::rect(Point::new(x, y), w, h)).unwrap()).unwrap()
    }

    /// A 20 × 10 rect grown by 1 is 22 × 12 with round corners.
    #[test]
    fn grow_region_rounds_corners() {
        let grown = grow_region(&rect(0.0, 0.0, 20.0, 10.0), 1.0, TOL);
        assert_eq!(grown.len(), 1);
        let want = 200.0 + 2.0 * (20.0 + 10.0) + std::f64::consts::PI;
        assert_approx_eq!(area_mm2(&grown), want, 0.05);
    }

    /// A ring grown by more than half its hole loses the hole.
    #[test]
    fn grow_region_closes_small_holes() {
        let hole = Contour::new(Polyline::rect(Point::new(4.0, 4.0), 2.0, 2.0))
            .unwrap()
            .reversed();
        let outer = Contour::new(Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0)).unwrap();
        let (ring, _) = nest(vec![outer, hole]).unwrap();
        let grown = grow_region(&ring[0], 1.5, TOL);
        assert_eq!(grown.len(), 1);
        assert!(grown[0].holes().is_empty());
    }

    /// A reference with no stock to leave reaches the walls of a rect: only the
    /// four corner wedges are left, `(4 − π) R²` in all.
    #[test]
    fn reference_without_stock_to_leave_leaves_corner_wedges() {
        let region = rect(0.0, 0.0, 100.0, 50.0);
        let reference = Reference {
            radius_mm: 5.0,
            stock_to_leave_mm: 0.0,
        };
        let left = left_by(&region, 0.0, &[reference], TOL);
        assert_eq!(left.len(), 4, "{left:?}");
        // Flattening each quarter arc (7.85 mm) to chords within the
        // tolerance adds up to that much area per mm of arc.
        let want = (4.0 - std::f64::consts::PI) * 25.0;
        let arcs_mm = 4.0 * std::f64::consts::FRAC_PI_2 * 5.0;
        assert_approx_eq!(area_mm2(&left), want, TOL * arcs_mm);
    }

    /// A ring whose tool only grazes the leftover is in air; one that
    /// reaches into it, or runs inside it, is not.
    #[test]
    fn in_air_needs_a_radius_of_clearance() {
        let left = [rect(0.0, 0.0, 10.0, 10.0)];
        let around = |gap_mm: f64| {
            Contour::new(Polyline::rect(
                Point::new(-gap_mm, -gap_mm),
                10.0 + 2.0 * gap_mm,
                10.0 + 2.0 * gap_mm,
            ))
            .unwrap()
        };
        assert!(in_air(&around(2.0), &left, 2.0, TOL));
        assert!(in_air(&around(2.0 - TOL / 2.0), &left, 2.0, TOL));
        assert!(!in_air(&around(1.5), &left, 2.0, TOL));
        let inside = Contour::new(Polyline::rect(Point::new(4.0, 4.0), 2.0, 2.0)).unwrap();
        assert!(!in_air(&inside, &left, 2.0, TOL));
        assert!(!in_air(&around(2.0), &left, TOL, TOL));
    }

    /// With a stock to leave the reference leaves a band of that width too:
    /// one ring-shaped piece.
    #[test]
    fn reference_stock_to_leave_leaves_a_band() {
        let region = rect(0.0, 0.0, 100.0, 50.0);
        let reference = Reference {
            radius_mm: 5.0,
            stock_to_leave_mm: 0.5,
        };
        let left = left_by(&region, 0.0, &[reference], TOL);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].holes().len(), 1);
        // The band plus the corners beyond it.
        let band = 5000.0 - 99.0 * 49.0;
        assert!(area_mm2(&left) > band, "{}", area_mm2(&left));
        // At the same stock to leave as the reference only the corners are left.
        let left = left_by(&region, 0.5, &[reference], TOL);
        assert_eq!(left.len(), 4, "{left:?}");
    }

    /// #493: a C-shaped tool-centre region, a 20 mm square wall 4 mm thick
    /// from (10, 10) with a mouth 1 mm wide in its right side, grown by
    /// 1.5 mm closes over its mouth. The bay inside it, the 12 mm square
    /// less 1.5 mm from its walls, is a hole of the reach: a tool whose
    /// centre stays in the C never comes within its radius of the bay's
    /// middle. The reach still runs 1.5 mm into the bay from its walls.
    /// Before #493 `offset`'s loop round the bay was a region of its own,
    /// so the bay was counted reached.
    #[test]
    fn grown_reach_excludes_a_closed_bay() {
        let points = [
            (10.0, 10.0),
            (30.0, 10.0),
            (30.0, 19.5),
            (26.0, 19.5),
            (26.0, 14.0),
            (14.0, 14.0),
            (14.0, 26.0),
            (26.0, 26.0),
            (26.0, 20.5),
            (30.0, 20.5),
            (30.0, 30.0),
            (10.0, 30.0),
        ];
        let vertices = points
            .iter()
            .map(|&(x, y)| dc_geometry::Vertex::line(x, y))
            .collect();
        let c = single_region(Contour::new(Polyline::new(vertices, true)).unwrap()).unwrap();
        let grown = grow_region(&c, 1.5, TOL);
        let reached = |q: Point| grown.iter().any(|r| r.contains_point(q));
        for q in [
            Point::new(20.0, 20.0),
            Point::new(15.6, 20.0),
            Point::new(24.4, 24.4),
        ] {
            assert!(!reached(q), "the bay at {q:?} is counted reached");
        }
        for q in [Point::new(15.4, 20.0), Point::new(20.0, 24.6)] {
            assert!(reached(q), "{q:?}, 1.4 mm from the C, is not reached");
        }
        assert_eq!(grown.len(), 1, "{grown:?}");
        assert_eq!(grown[0].holes().len(), 1, "the bay is one hole");
        // The bay: the 9 mm square, its corners by the mouth notched by the
        // round joins about the mouth's corners (a sliver under 0.03 mm²).
        assert_approx_eq!(grown[0].holes()[0].signed_area_mm2().abs(), 81.0, 0.05);
    }
}
