// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Region constructions a carve and the safety pass both need
//! (`docs/specs/phase-3/design.md`, "dc-cam" → the two region
//! constructions; security review M3): they live here so `dc-gcode`
//! re-derives material without calling a generator.

use crate::region::Region;

/// A carve's floor: each region inset by `inset_mm` (`flat · tan(α/2)`).
#[must_use]
pub fn floor_regions(_regions: &[Region], _inset_mm: f64, _tolerance_mm: f64) -> Vec<Region> {
    Vec::new()
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

    /// A 20 × 4 mm bar inset 2 mm or more leaves no floor; a hole grown
    /// over the whole of a ring's material leaves none either.
    #[test]
    fn floor_regions_vanish_when_the_shape_is_too_narrow() {
        let (bar, _) = nest(vec![rect(0.0, 0.0, 20.0, 4.0)]).expect("bar nests");
        assert!(floor_regions(&bar, 2.0, TOL).is_empty());
        assert!(floor_regions(&bar, 2.5, TOL).is_empty());
        assert!(
            area_mm2(&floor_regions(&bar, 1.0, TOL)) > 0.0,
            "1 mm leaves a 18 × 2 floor"
        );

        let (ring, _) =
            nest(vec![rect(0.0, 0.0, 20.0, 20.0), rect(3.0, 3.0, 14.0, 14.0)]).expect("ring nests");
        assert!(
            floor_regions(&ring, 1.6, TOL).is_empty(),
            "3 mm walls inset 1.6 from both sides"
        );
    }
}
