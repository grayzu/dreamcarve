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

/// A male inlay's surround (`docs/specs/phase-3/design.md`, "Algorithms →
/// Inlay" step 4): the area within `margin_mm` outside the design that the
/// male carve removes around its plug. Every region's outer is offset
/// outward by `margin_mm` with round joins, the offsets are united, and
/// `regions` themselves are subtracted. Subtracting a region removes its
/// outer and gives back its holes, so a hole in the design (a circle
/// inside a star) comes back as a surround region of its own; holes are
/// never offset. Overlapping margins merge into one region.
///
/// The subtraction is [`crate::difference`], so the result is polygonal:
/// arcs are flattened to chords within `tolerance_mm`, and the area is off
/// the exact one by at most `tolerance_mm` times the arc length flattened.
/// Outers come back counter-clockwise and holes clockwise (mm, Y-up,
/// ADR-0001), in [`crate::boolean`]'s canonical order, so the same input
/// gives the same regions. Empty when `regions` is empty or `margin_mm`
/// is not a positive finite number (the caller validates the margin).
#[must_use]
pub fn surround(regions: &[Region], margin_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    if !(margin_mm.is_finite() && margin_mm > 0.0) {
        return Vec::new();
    }
    let grown: Vec<Region> = regions
        .iter()
        .flat_map(|r| offset(r.outer(), margin_mm))
        .filter_map(single_region)
        .collect();
    // The subject's regions overlap where margins meet; `difference` fills
    // non-zero, so they count as their union.
    difference(&grown, regions, tolerance_mm)
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
    use crate::polyline::{Polyline, Vertex};
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

    /// A four-point star, 60 mm tip to tip, centred at the origin: tips on
    /// the axes 30 mm out, inner vertices on the diagonals 12 mm out.
    fn star() -> Contour {
        let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
        let points = [
            (30.0, 0.0),
            (inner, inner),
            (0.0, 30.0),
            (-inner, inner),
            (-30.0, 0.0),
            (-inner, -inner),
            (0.0, -30.0),
            (inner, -inner),
        ];
        let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        Contour::new(Polyline::new(vertices, true)).expect("a star")
    }

    fn circle(centre: Point, radius_mm: f64) -> Contour {
        Contour::new(Polyline::circle(centre, radius_mm)).expect("a circle")
    }

    /// The star of the inlay test part with a 12 mm circle in its middle,
    /// margin 8 mm: the surround is the 8 mm band round the star (one
    /// region whose hole is the star) and the circle (a region of its own,
    /// carved on the male so the female's island has a socket).
    #[test]
    fn surround_of_a_star_with_a_hole_has_the_hole_as_a_region() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        assert_eq!(design.len(), 1, "a star with a hole");

        let s = surround(&design, 8.0, TOL);
        assert_eq!(s.len(), 2, "the band and the circle: {s:?}");
        let band = s
            .iter()
            .find(|r| r.holes().len() == 1)
            .expect("the band keeps the star as its hole");
        let disc = s
            .iter()
            .find(|r| r.holes().is_empty())
            .expect("the circle is a region of its own");

        let outer = band.outer().polyline().bounds().expect("band bounds");
        for v in [outer.max.x, outer.max.y, -outer.min.x, -outer.min.y] {
            assert!((v - 38.0).abs() < TOL, "tips grow by the margin: {outer:?}");
        }
        let hole = band.holes()[0].polyline().bounds().expect("hole bounds");
        for v in [hole.max.x, hole.max.y, -hole.min.x, -hole.min.y] {
            assert!((v - 30.0).abs() < 1e-9, "the hole is the star: {hole:?}");
        }
        let d = disc.outer().polyline().bounds().expect("disc bounds");
        for v in [d.max.x, d.max.y, -d.min.x, -d.min.y] {
            assert!((v - 6.0).abs() < TOL, "the circle, 6 mm radius: {d:?}");
        }
        assert!(
            (disc.area_mm2() - 36.0 * PI).abs() < TOL * 12.0 * PI,
            "{}",
            disc.area_mm2()
        );
        assert!(
            s.iter()
                .all(|r| r.outer().is_ccw() && r.holes().iter().all(|h| !h.is_ccw())),
            "outers counter-clockwise, holes clockwise"
        );
    }

    /// Two 10 mm squares 6 mm apart with a 4 mm margin: their margins
    /// overlap and merge into one surround with two holes. A third square
    /// far off gets a surround of its own.
    #[test]
    fn surround_of_disjoint_shapes_merges_overlapping_margins() {
        let (design, _) = nest(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(16.0, 0.0, 10.0, 10.0),
            rect(100.0, 0.0, 10.0, 10.0),
        ])
        .expect("squares nest");
        assert_eq!(design.len(), 3);

        let s = surround(&design, 4.0, TOL);
        assert_eq!(s.len(), 2, "the merged pair and the lone square: {s:?}");
        let mut holes: Vec<usize> = s.iter().map(|r| r.holes().len()).collect();
        holes.sort_unstable();
        assert_eq!(holes, [1, 2]);
        let pair = s
            .iter()
            .find(|r| r.holes().len() == 2)
            .expect("the merged pair");
        let b = pair.outer().polyline().bounds().expect("bounds");
        assert!((b.min.x + 4.0).abs() < TOL && (b.max.x - 30.0).abs() < TOL);
        assert!((b.min.y + 4.0).abs() < TOL && (b.max.y - 14.0).abs() < TOL);
        assert_eq!(
            surround(&design, 4.0, TOL),
            s,
            "the same input gives the same regions"
        );
    }

    /// The surround's area is the margin offset's area minus the design's:
    /// a 40 × 20 mm plate with a 10 × 5 mm hole and a separate 5 mm circle,
    /// margin 3 mm. The plate's offset covers 800 + 2·3·(40 + 20) + 9π mm²
    /// and the circle's 64π; the design covers 750 + 25π, so its hole is
    /// back in the surround. Flattening moves the area by at most the
    /// tolerance times the length of arc flattened.
    #[test]
    fn surround_area_is_the_offset_minus_the_design() {
        let (design, _) = nest(vec![
            rect(0.0, 0.0, 40.0, 20.0),
            rect(15.0, 7.5, 10.0, 5.0),
            circle(Point::new(80.0, 10.0), 5.0),
        ])
        .expect("design nests");
        let margin = 3.0;

        let s = surround(&design, margin, TOL);
        let offset_area = 800.0 + 2.0 * margin * 60.0 + PI * margin * margin + PI * 64.0;
        let design_area = 750.0 + PI * 25.0;
        let arcs_mm = 2.0 * PI * (margin + 8.0 + 5.0);
        let got = area_mm2(&s);
        assert!(
            (got - (offset_area - design_area)).abs() <= TOL * arcs_mm,
            "{got} vs {}",
            offset_area - design_area
        );
        assert!(
            surround(&design, 0.0, TOL).is_empty(),
            "no margin, no surround"
        );
    }
}
