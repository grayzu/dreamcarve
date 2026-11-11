// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fill resolution for image prep (`docs/specs/phase-3/design.md`, "Image
//! prep" steps 3 and 4): the area subpaths fill under an SVG fill rule, the
//! areas of several paths joined as `init` reads them, and the art's
//! outline and the spaces between its lines. Pure functions on
//! [`Polyline`] and [`Region`]; results are canonical (see
//! [`crate::boolean`]).

use crate::GeometryError;
use crate::boolean::{difference, from_shapes, intersection, to_shapes, union};
use crate::contour::Contour;
use crate::point::{EPS_MM, Rect};
use crate::polyline::Polyline;
use crate::region::{Region, area_mm2, nest, touching_pairs};
use i_overlay::core::fill_rule::FillRule as OverlayFillRule;
use i_overlay::float::simplify::SimplifyShape;

/// An SVG fill rule. Own type; `i_overlay`'s `FillRule` stays private.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    /// A point is inside when the winding number around it is not zero.
    NonZero,
    /// A point is inside when a ray from it crosses an odd number of edges.
    EvenOdd,
}

/// The area `subpaths` fill under `rule` (mm, Y-up): open subpaths are
/// closed, as an SVG fill closes them, arcs are flattened within
/// `tolerance_mm`, and self-overlap and winding are resolved in one
/// `i_overlay` pass over the subject alone (`SimplifyShape`, which is
/// `FloatOverlay` with `OverlayRule::Subject`). The result is disjoint
/// regions with their holes, canonical (outers CCW, holes CW, each
/// contour from its lowest-then-leftmost vertex, ordered by it), so the
/// same subpaths in any order and winding give the same `Vec`. A subpath
/// with fewer than three vertices or a coordinate that is not finite
/// fills nothing and is skipped.
#[must_use]
pub fn resolve_fill(subpaths: &[Polyline], rule: FillRule, tolerance_mm: f64) -> Vec<Region> {
    let rings: Vec<Vec<[f64; 2]>> = subpaths
        .iter()
        .filter_map(|subpath| {
            let ring: Vec<[f64; 2]> = subpath
                .with_arcs_flattened(tolerance_mm)
                .vertices()
                .iter()
                .map(|v| [v.x, v.y])
                .collect();
            let finite = ring.iter().all(|p| p.iter().all(|c| c.is_finite()));
            (ring.len() >= 3 && finite).then_some(ring)
        })
        .collect();
    if rings.is_empty() {
        return Vec::new();
    }
    let rule = match rule {
        FillRule::NonZero => OverlayFillRule::NonZero,
        FillRule::EvenOdd => OverlayFillRule::EvenOdd,
    };
    from_shapes(rings.simplify_shape(rule))
}

/// Paths' areas, each from [`resolve_fill`], joined as `init` would read
/// them. Areas that cross (share area, neither inside the other's
/// material) are united, in groups by transitive crossing, one union per
/// group; then every contour goes through [`nest`], so an area wholly
/// inside another's material is a hole of it (an island path) and one
/// inside a hole is a piece of its own. Nesting decides holes, not paint
/// order or colour. The result is canonical.
///
/// # Errors
/// [`GeometryError::Unresolved`] if contours still cross after the
/// unions, which is a defect in this function and not a property of the
/// input.
pub fn merge_fills(
    paths: Vec<Vec<Region>>,
    tolerance_mm: f64,
) -> Result<Vec<Region>, GeometryError> {
    let regions: Vec<Region> = paths.into_iter().flatten().collect();
    let boxes: Vec<Option<Rect>> = regions
        .iter()
        .map(|r| r.outer().polyline().bounds())
        .collect();
    // `label[i]` names the group region `i` is in; pairs come in a fixed
    // order, so the groups depend on nothing but the input.
    let mut label: Vec<usize> = (0..regions.len()).collect();
    for (i, j) in touching_pairs(&boxes, 0.0) {
        let (Some(a), Some(b)) = (regions.get(i), regions.get(j)) else {
            continue;
        };
        let (Some(&to), Some(&from)) = (label.get(i), label.get(j)) else {
            continue;
        };
        if from != to && crosses(a, b, tolerance_mm) {
            for l in &mut label {
                if *l == from {
                    *l = to;
                }
            }
        }
    }
    let mut merged: Vec<Region> = Vec::new();
    for group in 0..regions.len() {
        let members: Vec<Region> = regions
            .iter()
            .zip(&label)
            .filter(|&(_, &l)| l == group)
            .map(|(r, _)| r.clone())
            .collect();
        match members.len() {
            0 => {}
            1 => merged.extend(members),
            _ => merged.extend(union(&members, &[], tolerance_mm)),
        }
    }
    let contours: Vec<Contour> = merged
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()).cloned())
        .collect();
    let (nested, _) = nest(contours).map_err(|_| GeometryError::Unresolved)?;
    Ok(from_shapes(to_shapes(&nested, tolerance_mm)))
}

/// True when `a` and `b` share area and neither lies wholly in the
/// other's material: the shared area is neither nothing nor all of one.
fn crosses(a: &Region, b: &Region, tolerance_mm: f64) -> bool {
    let shared_mm2 = area_mm2(&intersection(
        std::slice::from_ref(a),
        std::slice::from_ref(b),
        tolerance_mm,
    ));
    let perimeter_mm = |r: &Region| {
        std::iter::once(r.outer())
            .chain(r.holes())
            .map(|c| c.polyline().length_mm())
            .sum::<f64>()
    };
    let slack_mm2 = EPS_MM * (perimeter_mm(a) + perimeter_mm(b));
    shared_mm2 > slack_mm2
        && (a.area_mm2() - shared_mm2).abs() > slack_mm2
        && (b.area_mm2() - shared_mm2).abs() > slack_mm2
}

/// The art's outline (49c, step 4): every region's outer contour, holes
/// dropped, united.
#[must_use]
pub fn ink_outline(ink: &[Region], tolerance_mm: f64) -> Vec<Region> {
    let outers: Vec<Region> = ink
        .iter()
        .map(|r| r.without_holes(&(0..r.holes().len()).collect::<Vec<_>>()))
        .collect();
    union(&outers, &[], tolerance_mm)
}

/// The enclosed spaces between the lines: `difference(ink_outline(ink),
/// ink)`, the `--regions` mode of image prep.
#[must_use]
pub fn spaces_between(ink: &[Region], tolerance_mm: f64) -> Vec<Region> {
    difference(&ink_outline(ink, tolerance_mm), ink, tolerance_mm)
}

/// How many regions of `ink` enclose no space (have no hole).
#[must_use]
pub fn ink_without_spaces(ink: &[Region]) -> usize {
    ink.iter().filter(|r| r.holes().is_empty()).count()
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
        // Even-odd drops the doubly wound middle, leaving the five points.
        assert_eq!(even_odd.len(), 5);
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
        // The ring, the small square, the circle and the clockwise square
        // (a lone clockwise subpath still fills under NonZero).
        assert_eq!(one.len(), 4);
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
