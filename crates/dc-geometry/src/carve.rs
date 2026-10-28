// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Region constructions a carve and the safety pass both need
//! (`docs/specs/phase-3/design.md`, "dc-cam" → the two region
//! constructions; security review M3): they live here so `dc-gcode`
//! re-derives material without calling a generator.

use crate::boolean::difference;
use crate::contour::Contour;
use crate::offset::offset;
use crate::region::{Region, nest};

/// A carve's floor: each of `regions` inset by `inset_mm` (for a V-bit
/// carve with a flat depth `F`, `F · tan(α/2)`, the groove's half-width at
/// the floor). Each outer is offset inward and each hole outward by
/// `inset_mm`, with round joins, and the grown holes cut from the shrunk
/// outers. A region can split into several floors or leave none; the
/// result is empty when nothing is left. Outers come back counter-clockwise
/// and holes clockwise (mm, Y-up), in the order of `regions`.
///
/// When the grown holes lie apart inside the shrunk outer, the floors are
/// built from the offsets directly and keep their arcs; when they touch or
/// cross, the holes are cut with [`crate::difference`], which flattens
/// arcs to chords within `tolerance_mm`. The same construction as a
/// Pocket's ring inset, so the safety pass and the generators agree.
#[must_use]
pub fn floor_regions(regions: &[Region], inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    regions
        .iter()
        .flat_map(|r| inset_region(r, inset_mm, tolerance_mm))
        .collect()
}

/// [`floor_regions`] of one region.
fn inset_region(region: &Region, inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outers = offset(region.outer(), -inset_mm);
    if outers.is_empty() {
        return Vec::new();
    }
    // `offset` keeps a hole's clockwise winding and grows it for a
    // positive delta.
    let grown: Vec<Contour> = region
        .holes()
        .iter()
        .flat_map(|hole| offset(hole, inset_mm))
        .collect();
    if grown.is_empty() {
        return outers.into_iter().filter_map(single_region).collect();
    }
    let mut all = outers.clone();
    all.extend(grown.iter().cloned());
    if let Ok((nested, warnings)) = nest(all) {
        let holes: usize = nested.iter().map(|r| r.holes().len()).sum();
        // A grown hole that encloses the shrunk outer nests as the outer
        // of an inverted annulus; only the shrunk outers may be outers.
        let clean = warnings.is_empty()
            && nested.len() == outers.len()
            && holes == grown.len()
            && nested
                .iter()
                .all(|r| r.depth() == 0 && outers.contains(r.outer()));
        if clean {
            return nested;
        }
    }
    let subject: Vec<Region> = outers.into_iter().filter_map(single_region).collect();
    let clip: Vec<Region> = grown
        .iter()
        .filter_map(|c| single_region(c.reversed()))
        .collect();
    difference(&subject, &clip, tolerance_mm)
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
    use crate::contour::Contour;
    use crate::point::Point;
    use crate::polyline::Polyline;
    use crate::region::{area_mm2, nest};
    use std::f64::consts::PI;

    const TOL: f64 = crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(x, y), w, h)).expect("a rectangle")
    }

    /// A 40 × 20 mm plate with a 10 × 5 mm hole, inset 2 mm: the outer
    /// shrinks to 36 × 16 (its convex corners stay sharp), the hole grows
    /// to 14 × 9 with 2 mm round corners, so the floor's area is
    /// 576 − (50 + 2·2·(10 + 5) + 4π) mm², and it keeps one hole. A
    /// second, separate square becomes a floor of its own.
    #[test]
    fn floor_regions_inset_outers_and_grow_holes() {
        let (plate, _) = nest(vec![rect(0.0, 0.0, 40.0, 20.0), rect(15.0, 7.5, 10.0, 5.0)])
            .expect("plate nests");
        let (square, _) = nest(vec![rect(60.0, 0.0, 10.0, 10.0)]).expect("square nests");
        let regions = [plate, square].concat();

        let floor = floor_regions(&regions, 2.0, TOL);
        assert_eq!(floor.len(), 2, "one floor per region: {floor:?}");
        assert_eq!(floor[0].holes().len(), 1, "the grown hole stays a hole");
        let bounds = floor[0].outer().polyline().bounds().expect("bounds");
        assert!((bounds.min.x - 2.0).abs() < 1e-9 && (bounds.max.x - 38.0).abs() < 1e-9);
        assert!((bounds.min.y - 2.0).abs() < 1e-9 && (bounds.max.y - 18.0).abs() < 1e-9);
        let hole = floor[0].holes()[0]
            .polyline()
            .bounds()
            .expect("hole bounds");
        assert!((hole.min.x - 13.0).abs() < 1e-9 && (hole.max.x - 27.0).abs() < 1e-9);
        assert!((hole.min.y - 5.5).abs() < 1e-9 && (hole.max.y - 14.5).abs() < 1e-9);
        let expected = 36.0 * 16.0 - (50.0 + 4.0 * 15.0 + 4.0 * PI);
        assert!(
            (floor[0].area_mm2() - expected).abs() < 1e-6,
            "{}",
            floor[0].area_mm2()
        );
        assert!(
            (floor[1].area_mm2() - 36.0).abs() < 1e-9,
            "the square's floor is 6 × 6"
        );
        assert!(
            floor.iter().all(|r| r.outer().is_ccw()),
            "outers stay counter-clockwise"
        );
    }

    /// A 20 × 4 mm bar inset 2 mm or more leaves no floor, inset 1 mm an
    /// 18 × 2 one. A square ring with 3 mm walls keeps only its four corner
    /// pockets at an inset of 1.6 mm (the widest disc there has radius
    /// 3·(2 − √2) ≈ 1.757 mm, as in the medial ring test), and nothing at
    /// 2 mm, where the grown hole covers the shrunk outer.
    #[test]
    fn floor_regions_vanish_when_the_shape_is_too_narrow() {
        let (bar, _) = nest(vec![rect(0.0, 0.0, 20.0, 4.0)]).expect("bar nests");
        assert!(floor_regions(&bar, 2.0, TOL).is_empty());
        assert!(floor_regions(&bar, 2.5, TOL).is_empty());
        assert!(
            (area_mm2(&floor_regions(&bar, 1.0, TOL)) - 36.0).abs() < 1e-9,
            "1 mm leaves an 18 × 2 floor"
        );

        let (ring, _) =
            nest(vec![rect(0.0, 0.0, 20.0, 20.0), rect(3.0, 3.0, 14.0, 14.0)]).expect("ring nests");
        assert_eq!(
            floor_regions(&ring, 1.6, TOL).len(),
            4,
            "the corner pockets"
        );
        assert!(
            floor_regions(&ring, 2.0, TOL).is_empty(),
            "3 mm walls inset 2 mm from both sides"
        );
    }
}
