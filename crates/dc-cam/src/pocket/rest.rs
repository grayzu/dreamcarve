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
//! The earlier Pocket is known only by its tool radius `R` and allowance
//! `A`, read from the project; its toolpath is never trusted (S14). The
//! area it reached is its tool-centre region (the region inset by `R + A`)
//! grown by `R`, as upstream `generateAreaRestRegionDrafts` builds it
//! (`buildInsetRegions` by `toolRadius + radialLeave`, then a round offset by
//! `toolRadius`). What it left is the region, less this operation's own
//! allowance, minus that.
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
use dc_geometry::{Contour, Region, difference, intersection, nest, offset, union};

/// The Pocket a rest operation rests on, as far as rest machining needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reference {
    /// Its tool's radius, mm.
    pub radius_mm: f64,
    /// Its `allowance_mm`.
    pub allowance_mm: f64,
}

/// `region` grown by `by_mm` (≥ 0): its outer offset outward and its holes
/// shrunk, holes that vanish dropped. Polygonal, as booleans return it.
#[must_use]
pub fn grow_region(region: &Region, by_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outers: Vec<Region> = offset(region.outer(), by_mm)
        .into_iter()
        .filter_map(single_region)
        .collect();
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

/// The union of every region of `regions` grown by `by_mm`.
fn grow_all(regions: &[Region], by_mm: f64, tolerance_mm: f64) -> Vec<Region> {
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
        reference.radius_mm + reference.allowance_mm,
        tolerance_mm,
    );
    grow_all(&centre, reference.radius_mm, tolerance_mm)
}

/// The material of `region` the reference left that this operation, at
/// `allowance_mm`, should remove: the region less the allowance, minus
/// [`reachable`], noise dropped. Empty when the reference left nothing.
#[must_use]
pub fn left_by(
    region: &Region,
    allowance_mm: f64,
    reference: Reference,
    tolerance_mm: f64,
) -> Vec<Region> {
    let material = if allowance_mm > 0.0 {
        inset_region(region, allowance_mm, tolerance_mm)
    } else {
        vec![region.clone()]
    };
    let reached = reachable(region, reference, tolerance_mm);
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

    /// A reference with no allowance reaches the walls of a rect: only the
    /// four corner wedges are left, `(4 − π) R²` in all.
    #[test]
    fn reference_without_allowance_leaves_corner_wedges() {
        let region = rect(0.0, 0.0, 100.0, 50.0);
        let reference = Reference {
            radius_mm: 5.0,
            allowance_mm: 0.0,
        };
        let left = left_by(&region, 0.0, reference, TOL);
        assert_eq!(left.len(), 4, "{left:?}");
        // Flattening each quarter arc (7.85 mm) to chords within the
        // tolerance adds up to that much area per mm of arc.
        let want = (4.0 - std::f64::consts::PI) * 25.0;
        let arcs_mm = 4.0 * std::f64::consts::FRAC_PI_2 * 5.0;
        assert_approx_eq!(area_mm2(&left), want, TOL * arcs_mm);
    }

    /// With an allowance the reference leaves a band of that width too:
    /// one ring-shaped piece.
    #[test]
    fn reference_allowance_leaves_a_band() {
        let region = rect(0.0, 0.0, 100.0, 50.0);
        let reference = Reference {
            radius_mm: 5.0,
            allowance_mm: 0.5,
        };
        let left = left_by(&region, 0.0, reference, TOL);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].holes().len(), 1);
        // The band plus the corners beyond it.
        let band = 5000.0 - 99.0 * 49.0;
        assert!(area_mm2(&left) > band, "{}", area_mm2(&left));
        // At the same allowance as the reference only the corners are left.
        let left = left_by(&region, 0.5, reference, TOL);
        assert_eq!(left.len(), 4, "{left:?}");
    }
}
