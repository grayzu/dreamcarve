// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fill resolution for image prep (`docs/specs/phase-3/design.md`, "Image
//! prep" steps 3 and 4): the area subpaths fill under an SVG fill rule, the
//! areas of several paths joined as `init` reads them, and the art's
//! outline and the spaces between its lines. Pure functions on
//! [`Polyline`] and [`Region`]; results are canonical (see
//! [`crate::boolean`]).

use crate::GeometryError;
use crate::polyline::Polyline;
use crate::region::Region;

/// An SVG fill rule. Own type; `i_overlay`'s `FillRule` stays private.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    /// A point is inside when the winding number around it is not zero.
    NonZero,
    /// A point is inside when a ray from it crosses an odd number of edges.
    EvenOdd,
}

/// The area `subpaths` fill under `rule`.
#[must_use]
pub fn resolve_fill(_subpaths: &[Polyline], _rule: FillRule, _tolerance_mm: f64) -> Vec<Region> {
    Vec::new()
}

/// The areas of several paths joined as `init` would read them.
///
/// # Errors
/// [`GeometryError::Unresolved`] when contours still cross after the unions.
pub fn merge_fills(
    _paths: Vec<Vec<Region>>,
    _tolerance_mm: f64,
) -> Result<Vec<Region>, GeometryError> {
    Ok(Vec::new())
}

/// Every region's outer contour, holes dropped, united.
#[must_use]
pub fn ink_outline(_ink: &[Region], _tolerance_mm: f64) -> Vec<Region> {
    Vec::new()
}

/// The enclosed spaces between the lines: `difference(ink_outline(ink), ink)`.
#[must_use]
pub fn spaces_between(_ink: &[Region], _tolerance_mm: f64) -> Vec<Region> {
    Vec::new()
}

/// How many regions of `ink` enclose no space (have no hole).
#[must_use]
pub fn ink_without_spaces(_ink: &[Region]) -> usize {
    usize::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::boolean::union;
    use crate::contour::Contour;
    use crate::point::Point;
    use crate::polyline::Vertex;
    use crate::region::{area_mm2, nest};
    use proptest::prelude::*;

    const TOL_MM: f64 = 0.001;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Polyline {
        Polyline::rect(Point::new(x, y), w, h)
    }

    fn region_of(polylines: Vec<Polyline>) -> Region {
        let contours = polylines
            .into_iter()
            .map(|p| Contour::new(p).unwrap())
            .collect();
        let (mut regions, _) = nest(contours).unwrap();
        assert_eq!(regions.len(), 1);
        regions.remove(0)
    }

    fn rect_region(x: f64, y: f64, w: f64, h: f64) -> Region {
        region_of(vec![rect(x, y, w, h)])
    }

    fn circle_region(x: f64, y: f64, r: f64) -> Region {
        region_of(vec![Polyline::circle(Point::new(x, y), r)])
    }

    fn assert_canonical(regions: &[Region]) {
        for region in regions {
            assert!(region.outer().is_ccw());
            assert!(region.holes().iter().all(|h| !h.is_ccw()));
            for c in std::iter::once(region.outer()).chain(region.holes()) {
                let v = c.polyline().vertices();
                let lowest = v
                    .iter()
                    .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
                    .unwrap();
                assert_eq!(v.first().unwrap(), lowest, "starts at lowest-then-leftmost");
            }
        }
    }

    #[test]
    fn nonzero_and_even_odd_differ_on_nested_same_winding() {
        let subpaths = [rect(0.0, 0.0, 20.0, 20.0), rect(5.0, 5.0, 10.0, 10.0)];
        let nonzero = resolve_fill(&subpaths, FillRule::NonZero, TOL_MM);
        assert_eq!(nonzero.len(), 1);
        assert!(nonzero[0].holes().is_empty());
        assert_approx_eq!(area_mm2(&nonzero), 400.0, 1e-6);
        let even_odd = resolve_fill(&subpaths, FillRule::EvenOdd, TOL_MM);
        assert_eq!(even_odd.len(), 1);
        assert_eq!(even_odd[0].holes().len(), 1);
        assert_approx_eq!(area_mm2(&even_odd), 300.0, 1e-6);
        assert_canonical(&nonzero);
        assert_canonical(&even_odd);
    }

    #[test]
    fn opposite_winding_leaves_a_hole_under_nonzero() {
        let subpaths = [
            rect(0.0, 0.0, 20.0, 20.0),
            rect(5.0, 5.0, 10.0, 10.0).reversed(),
        ];
        let out = resolve_fill(&subpaths, FillRule::NonZero, TOL_MM);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes().len(), 1);
        assert_approx_eq!(area_mm2(&out), 300.0, 1e-6);
    }

    #[test]
    fn open_subpath_is_closed_by_its_fill() {
        let triangle = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(10.0, 0.0),
                Vertex::line(0.0, 10.0),
            ],
            false,
        );
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let out = resolve_fill(std::slice::from_ref(&triangle), rule, TOL_MM);
            assert_eq!(out.len(), 1);
            assert_approx_eq!(area_mm2(&out), 50.0, 1e-6);
        }
    }

    #[test]
    fn self_overlapping_stroke_outline_resolves_to_one_region() {
        // A five-point star drawn in one stroke: its middle is wound twice,
        // as a stroker's outline is where it loops at a join.
        let star: Vec<Vertex> = (0..5)
            .map(|k| {
                let a = std::f64::consts::FRAC_PI_2 + f64::from(k) * 0.8 * std::f64::consts::PI;
                Vertex::line(10.0 * a.cos(), 10.0 * a.sin())
            })
            .collect();
        let star = Polyline::new(star, true);
        let nonzero = resolve_fill(std::slice::from_ref(&star), FillRule::NonZero, TOL_MM);
        assert_eq!(nonzero.len(), 1);
        assert!(nonzero[0].holes().is_empty());
        let even_odd = resolve_fill(std::slice::from_ref(&star), FillRule::EvenOdd, TOL_MM);
        assert_eq!(even_odd.len(), 1);
        assert_eq!(even_odd[0].holes().len(), 1);
        assert!(area_mm2(&nonzero) > area_mm2(&even_odd));
        assert_canonical(&nonzero);
    }

    #[test]
    fn result_is_canonical_whatever_the_input_order() {
        let a = [
            rect(40.0, 0.0, 5.0, 5.0),
            rect(0.0, 0.0, 20.0, 20.0),
            rect(5.0, 5.0, 10.0, 10.0).reversed(),
            Polyline::circle(Point::new(60.0, 30.0), 6.0),
            rect(30.0, 30.0, 4.0, 4.0).reversed(),
        ];
        let mut b = a.clone();
        b.reverse();
        let one = resolve_fill(&a, FillRule::NonZero, TOL_MM);
        let two = resolve_fill(&b, FillRule::NonZero, TOL_MM);
        assert_eq!(one.len(), 3);
        assert_eq!(one, two);
        assert_canonical(&one);
    }

    #[test]
    fn crossing_areas_are_united_and_nested_ones_are_holes() {
        let crossing = merge_fills(
            vec![
                vec![rect_region(0.0, 0.0, 10.0, 10.0)],
                vec![rect_region(5.0, 0.0, 10.0, 10.0)],
            ],
            TOL_MM,
        )
        .unwrap();
        assert_eq!(crossing.len(), 1);
        assert_approx_eq!(area_mm2(&crossing), 150.0, 1e-6);
        let island = merge_fills(
            vec![
                vec![rect_region(0.0, 0.0, 20.0, 20.0)],
                vec![rect_region(5.0, 5.0, 10.0, 10.0)],
            ],
            TOL_MM,
        )
        .unwrap();
        assert_eq!(island.len(), 1);
        assert_eq!(island[0].holes().len(), 1);
        assert_approx_eq!(area_mm2(&island), 300.0, 1e-6);
        assert_canonical(&crossing);
        assert_canonical(&island);
    }

    #[test]
    fn three_way_crossing_group_is_one_region() {
        // The outer two do not touch; the middle one crosses both.
        let out = merge_fills(
            vec![
                vec![rect_region(0.0, 0.0, 10.0, 10.0)],
                vec![rect_region(16.0, 0.0, 10.0, 10.0)],
                vec![rect_region(8.0, 0.0, 10.0, 10.0)],
            ],
            TOL_MM,
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_approx_eq!(area_mm2(&out), 260.0, 1e-6);
    }

    #[test]
    fn area_inside_a_hole_is_a_piece_of_its_own() {
        let ring = region_of(vec![rect(0.0, 0.0, 20.0, 20.0), rect(4.0, 4.0, 12.0, 12.0)]);
        let out = merge_fills(
            vec![vec![ring], vec![rect_region(8.0, 8.0, 4.0, 4.0)]],
            TOL_MM,
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out.iter().map(|r| r.holes().len()).sum::<usize>(), 1);
        assert_approx_eq!(area_mm2(&out), 400.0 - 144.0 + 16.0, 1e-6);
        assert_canonical(&out);
    }

    fn shape() -> impl Strategy<Value = Region> {
        prop_oneof![
            (0.0..40.0f64, 0.0..40.0f64, 1.0..25.0f64, 1.0..25.0f64)
                .prop_map(|(x, y, w, h)| rect_region(x, y, w, h)),
            (5.0..45.0f64, 5.0..45.0f64, 1.0..15.0f64).prop_map(|(x, y, r)| circle_region(x, y, r)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// No two contours of the merged result cross, so `nest` reads it.
        #[test]
        fn merge_fills_never_leaves_crossing_contours(
            shapes in proptest::collection::vec(shape(), 1..8)
        ) {
            let paths = shapes.into_iter().map(|r| vec![r]).collect();
            let out = merge_fills(paths, TOL_MM).unwrap();
            let contours: Vec<Contour> = out
                .iter()
                .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()).cloned())
                .collect();
            prop_assert!(nest(contours).is_ok());
        }
    }

    #[test]
    fn ink_outline_drops_holes_and_unites_components() {
        let ring = region_of(vec![rect(0.0, 0.0, 20.0, 20.0), rect(4.0, 4.0, 12.0, 12.0)]);
        let island = rect_region(8.0, 8.0, 4.0, 4.0);
        let apart = rect_region(30.0, 0.0, 5.0, 5.0);
        let out = ink_outline(&[ring, island, apart], TOL_MM);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.holes().is_empty()));
        assert_approx_eq!(area_mm2(&out), 400.0 + 25.0, 1e-6);
        assert_canonical(&out);
    }

    fn ring_and_cross() -> Vec<Region> {
        let ring = region_of(vec![
            Polyline::circle(Point::new(0.0, 0.0), 30.0),
            Polyline::circle(Point::new(0.0, 0.0), 25.0),
        ]);
        union(
            &[ring],
            &[
                rect_region(-28.0, -2.0, 56.0, 4.0),
                rect_region(-2.0, -28.0, 4.0, 56.0),
            ],
            TOL_MM,
        )
    }

    #[test]
    fn spaces_between_a_ring_and_a_cross_are_four_regions() {
        let ink = ring_and_cross();
        assert_eq!(ink.len(), 1);
        let spaces = spaces_between(&ink, TOL_MM);
        assert_eq!(spaces.len(), 4);
        assert!(spaces.iter().all(|r| r.holes().is_empty()));
        // The disc inside the ring less the cross's arms: the strip
        // |x| < 2 holds the integral of 2 sqrt(r^2 - x^2) over -2..2, and
        // the arms are two such strips less the doubled centre square.
        let r: f64 = 25.0;
        let strip = 2.0 * (2.0 * (r * r - 4.0).sqrt() + r * r * (2.0 / r).asin());
        let expected = std::f64::consts::PI * r * r - (2.0 * strip - 16.0);
        assert_approx_eq!(area_mm2(&spaces), expected, 1.0);
        let first = spaces[0].area_mm2();
        for s in &spaces {
            assert_approx_eq!(s.area_mm2(), first, 0.5);
        }
        assert_canonical(&spaces);
    }

    #[test]
    fn spaces_between_leaves_an_ink_island_as_a_hole() {
        let ring = region_of(vec![
            Polyline::circle(Point::new(0.0, 0.0), 30.0),
            Polyline::circle(Point::new(0.0, 0.0), 25.0),
        ]);
        let island = circle_region(0.0, 0.0, 5.0);
        let spaces = spaces_between(&[ring, island], TOL_MM);
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].holes().len(), 1);
        let expected = std::f64::consts::PI * (25.0 * 25.0 - 5.0 * 5.0);
        assert_approx_eq!(area_mm2(&spaces), expected, 0.5);
    }

    #[test]
    fn ink_without_spaces_counts_a_floating_cross() {
        let cross = union(
            &[rect_region(0.0, 0.0, 30.0, 4.0)],
            &[rect_region(13.0, -13.0, 4.0, 30.0)],
            TOL_MM,
        );
        assert_eq!(cross.len(), 1);
        assert_eq!(ink_without_spaces(&cross), 1);
        let ring = region_of(vec![
            rect(100.0, 0.0, 20.0, 20.0),
            rect(104.0, 4.0, 12.0, 12.0),
        ]);
        let both = [cross.clone(), vec![ring]].concat();
        assert_eq!(ink_without_spaces(&both), 1);
        assert_eq!(ink_without_spaces(&[]), 0);
    }
}
