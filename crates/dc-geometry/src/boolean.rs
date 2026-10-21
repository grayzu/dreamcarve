// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Boolean operations on regions via i_overlay (`docs/specs/phase-2/design.md`,
//! "Algorithms → Pocket" steps 2–3 and the coverage check). i_overlay works
//! on polygons, so arcs are flattened to chords at a tolerance first and
//! every result is polygonal: no output vertex carries a bulge. Results are
//! canonical: outers counter-clockwise, holes clockwise, each contour
//! starting at its lowest-then-leftmost vertex, holes and regions ordered
//! by that vertex, so the same shapes give the same `Vec` whatever order
//! they were given in.

use crate::region::Region;

/// Which area a boolean keeps. `a` is the subject and `b` the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// Area in `a` or `b`.
    Union,
    /// Area in `a` and not in `b`.
    Difference,
    /// Area in both `a` and `b`.
    Intersection,
}

/// Applies `op` to the areas `a` and `b` cover and returns the result as
/// regions (outers CCW, holes CW; mm, Y-up). Arcs in the inputs are
/// flattened to chords no further than `tolerance_mm` from the arc
/// ([`crate::FLATTEN_TOLERANCE_DEFAULT_MM`] is the usual value), so the result
/// is polygonal and its area differs from the exact one by at most
/// `tolerance_mm` times the arc length. Regions within one side may
/// overlap; their union is that side's area (non-zero fill). Pieces whose
/// area rounds to zero are dropped.
#[must_use]
pub fn boolean(a: &[Region], b: &[Region], op: BooleanOp, tolerance_mm: f64) -> Vec<Region> {
    let _ = (a, b, op, tolerance_mm);
    Vec::new()
}

/// Area in `a` or `b`; see [`boolean`].
#[must_use]
pub fn union(a: &[Region], b: &[Region], tolerance_mm: f64) -> Vec<Region> {
    boolean(a, b, BooleanOp::Union, tolerance_mm)
}

/// Area in `a` and not in `b`; see [`boolean`].
#[must_use]
pub fn difference(a: &[Region], b: &[Region], tolerance_mm: f64) -> Vec<Region> {
    boolean(a, b, BooleanOp::Difference, tolerance_mm)
}

/// Area in both `a` and `b`; see [`boolean`].
#[must_use]
pub fn intersection(a: &[Region], b: &[Region], tolerance_mm: f64) -> Vec<Region> {
    boolean(a, b, BooleanOp::Intersection, tolerance_mm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::contour::Contour;
    use crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;
    use crate::point::{EPS_MM, Point};
    use crate::polyline::Polyline;
    use crate::region::{area_mm2, nest};
    use std::f64::consts::PI;

    const TOL_MM: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

    fn region_of(polylines: Vec<Polyline>) -> Region {
        let contours = polylines
            .into_iter()
            .map(|p| Contour::new(p).unwrap())
            .collect();
        let (mut regions, warnings) = nest(contours).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(regions.len(), 1);
        regions.remove(0)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Region {
        region_of(vec![Polyline::rect(Point::new(x, y), w, h)])
    }

    fn circle(x: f64, y: f64, r: f64) -> Region {
        region_of(vec![Polyline::circle(Point::new(x, y), r)])
    }

    fn assert_canonical_winding(regions: &[Region]) {
        for (i, region) in regions.iter().enumerate() {
            assert!(region.outer().is_ccw(), "outer {i} must be CCW");
            for (j, hole) in region.holes().iter().enumerate() {
                assert!(!hole.is_ccw(), "hole {j} of region {i} must be CW");
            }
            for contour in std::iter::once(region.outer()).chain(region.holes()) {
                assert!(
                    contour.polyline().segments().all(|s| s.is_line()),
                    "boolean output is polygonal"
                );
            }
        }
    }

    #[test]
    fn difference_of_rect_and_circle_has_one_hole() {
        let out = difference(
            &[rect(0.0, 0.0, 100.0, 50.0)],
            &[circle(50.0, 25.0, 10.0)],
            TOL_MM,
        );
        assert_eq!(out.len(), 1);
        let region = &out[0];
        assert_eq!(region.holes().len(), 1);
        assert_canonical_winding(&out);
        assert_approx_eq!(region.outer().signed_area_mm2(), 5000.0, 1e-6);
        // Chords lie inside the arc, so the flattened circle is smaller
        // than πr² by at most the tolerance times its circumference.
        let hole_mm2 = region.holes()[0].signed_area_mm2();
        assert!(hole_mm2 < 0.0, "hole area is signed negative: {hole_mm2}");
        assert_approx_eq!(-hole_mm2, PI * 100.0, TOL_MM * 2.0 * PI * 10.0 + 1e-6);
    }

    #[test]
    fn union_of_touching_rects_is_one_region() {
        let out = union(
            &[rect(0.0, 0.0, 10.0, 10.0)],
            &[rect(10.0, 0.0, 10.0, 10.0)],
            TOL_MM,
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].holes().is_empty());
        assert_approx_eq!(out[0].outer().signed_area_mm2(), 200.0, 1e-6);
        let b = out[0].outer().polyline().bounds().unwrap();
        assert_approx_eq!(b.min.x, 0.0, 1e-6);
        assert_approx_eq!(b.max.x, 20.0, 1e-6);
        assert_canonical_winding(&out);
    }

    #[test]
    fn intersection_of_disjoint_is_empty() {
        let out = intersection(
            &[rect(0.0, 0.0, 10.0, 10.0)],
            &[rect(20.0, 0.0, 10.0, 10.0)],
            TOL_MM,
        );
        assert!(out.is_empty(), "got {out:?}");
    }

    #[test]
    fn intersection_of_overlapping_rects_is_their_overlap() {
        let out = intersection(
            &[rect(0.0, 0.0, 10.0, 10.0)],
            &[rect(5.0, 5.0, 10.0, 10.0)],
            TOL_MM,
        );
        assert_eq!(out.len(), 1);
        assert_approx_eq!(out[0].outer().signed_area_mm2(), 25.0, 1e-6);
        let b = out[0].outer().polyline().bounds().unwrap();
        assert_approx_eq!(b.min.x, 5.0, 1e-6);
        assert_approx_eq!(b.min.y, 5.0, 1e-6);
        assert_approx_eq!(b.max.x, 10.0, 1e-6);
        assert_approx_eq!(b.max.y, 10.0, 1e-6);
    }

    /// An island wider than the pocket splits the tool-centre region
    /// (design, "Pocket" step 2: "an island can split a pocket into
    /// several").
    #[test]
    fn difference_can_split_a_region_in_two() {
        let out = difference(
            &[rect(0.0, 0.0, 100.0, 50.0)],
            &[rect(45.0, -1.0, 10.0, 52.0)],
            TOL_MM,
        );
        assert_eq!(out.len(), 2);
        assert_canonical_winding(&out);
        for region in &out {
            assert_approx_eq!(region.outer().signed_area_mm2(), 45.0 * 50.0, 1e-6);
        }
        // Lowest-then-leftmost first.
        assert!(out[0].outer().polyline().bounds().unwrap().max.x < 50.0);
        assert!(out[1].outer().polyline().bounds().unwrap().min.x > 50.0);
    }

    #[test]
    fn results_are_ccw_outers_and_cw_holes() {
        let frame = region_of(vec![
            Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
            Polyline::rect(Point::new(10.0, 10.0), 20.0, 20.0).reversed(),
        ]);
        let bar = rect(-5.0, 18.0, 50.0, 4.0);
        let disc = circle(20.0, 20.0, 12.0);
        for op in [
            BooleanOp::Union,
            BooleanOp::Difference,
            BooleanOp::Intersection,
        ] {
            let out = boolean(&[frame.clone(), disc.clone()], &[bar.clone()], op, TOL_MM);
            assert!(!out.is_empty(), "{op:?} gives something");
            assert_canonical_winding(&out);
        }
        // Clockwise input is normalised like any other.
        let cw_disc = region_of(vec![Polyline::circle(Point::new(0.0, 0.0), 5.0).reversed()]);
        let out = union(&[cw_disc], &[], TOL_MM);
        assert_eq!(out.len(), 1);
        assert_canonical_winding(&out);
    }

    #[test]
    fn output_order_does_not_depend_on_input_order() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(30.0, 5.0, 10.0, 10.0);
        let c = rect(15.0, -20.0, 10.0, 10.0);
        let d = circle(50.0, 50.0, 4.0);
        let first = union(&[a.clone(), b.clone(), c.clone()], &[d.clone()], TOL_MM);
        let second = union(&[d, c, a], &[b], TOL_MM);
        assert_eq!(first.len(), 4);
        assert_eq!(first, second);
        // Lowest Y first, then lowest X: c (y = −20), a (0, 0), b (30, 5), d.
        let mins: Vec<(f64, f64)> = first
            .iter()
            .map(|r| {
                let b = r.outer().polyline().bounds().unwrap();
                (b.min.x, b.min.y)
            })
            .collect();
        assert_approx_eq!(mins[0].1, -20.0, 1e-6);
        assert_approx_eq!(mins[1].0, 0.0, 1e-6);
        assert_approx_eq!(mins[2].0, 30.0, 1e-6);
        assert_approx_eq!(mins[3].0, 46.0, 1e-6);
    }

    #[test]
    fn same_input_twice_is_identical() {
        let a = [rect(0.0, 0.0, 100.0, 50.0), circle(90.0, 40.0, 20.0)];
        let b = [circle(50.0, 25.0, 10.0), rect(-5.0, 20.0, 30.0, 5.0)];
        let first = difference(&a, &b, TOL_MM);
        let second = difference(&a, &b, TOL_MM);
        assert_eq!(first, second);
    }

    #[test]
    fn arcs_are_flattened_within_tolerance() {
        let out = union(&[circle(0.0, 0.0, 20.0)], &[], TOL_MM);
        assert_eq!(out.len(), 1);
        assert_canonical_winding(&out);
        let outer = out[0].outer().polyline();
        assert!(outer.vertices().len() > 8, "arcs become many chords");
        for seg in outer.segments() {
            let mid = seg.start + (seg.end - seg.start) * 0.5;
            // Snapping to i_overlay's grid adds far less than the tolerance.
            let slack = 1e-6;
            assert_approx_eq!(seg.start.distance_to(Point::default()), 20.0, slack);
            let sagitta = 20.0 - mid.distance_to(Point::default());
            assert!(
                (-slack..=TOL_MM + slack).contains(&sagitta),
                "chord midpoint strays {sagitta} mm from the circle"
            );
        }
        assert_approx_eq!(
            outer.signed_area_mm2(),
            PI * 400.0,
            TOL_MM * 2.0 * PI * 20.0 + 1e-6
        );
    }

    #[test]
    fn coarser_tolerance_gives_fewer_vertices() {
        let fine = union(&[circle(0.0, 0.0, 20.0)], &[], 0.001);
        let coarse = union(&[circle(0.0, 0.0, 20.0)], &[], 0.5);
        let count = |r: &[Region]| r[0].outer().polyline().vertices().len();
        assert!(count(&fine) > 2 * count(&coarse));
        assert!(count(&coarse) >= 4);
    }

    #[test]
    fn empty_sides_behave_like_empty_sets() {
        let a = [rect(0.0, 0.0, 10.0, 10.0)];
        let identity = |out: Vec<Region>| {
            assert_eq!(out.len(), 1);
            assert_approx_eq!(out[0].outer().signed_area_mm2(), 100.0, 1e-6);
        };
        identity(union(&a, &[], TOL_MM));
        identity(union(&[], &a, TOL_MM));
        identity(difference(&a, &[], TOL_MM));
        assert!(difference(&[], &a, TOL_MM).is_empty());
        assert!(intersection(&a, &[], TOL_MM).is_empty());
        assert!(intersection(&[], &a, TOL_MM).is_empty());
        assert!(union(&[], &[], TOL_MM).is_empty());
    }

    /// Overlapping regions on one side are that side's union (non-zero
    /// fill), not their symmetric difference.
    #[test]
    fn overlapping_regions_on_one_side_merge() {
        let a = [rect(0.0, 0.0, 10.0, 10.0), rect(5.0, 0.0, 10.0, 10.0)];
        let out = union(&a, &[], TOL_MM);
        assert_eq!(out.len(), 1);
        assert_approx_eq!(area_mm2(&out), 150.0, 1e-6);
    }

    /// A pocket inside an island inside a pocket: the island's region is
    /// nested two deep in the output and keeps that depth.
    #[test]
    fn nested_output_keeps_depth() {
        let outer = region_of(vec![
            Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
            Polyline::rect(Point::new(10.0, 10.0), 20.0, 20.0).reversed(),
        ]);
        let inner = rect(15.0, 15.0, 10.0, 10.0);
        let out = union(&[outer], &[inner], TOL_MM);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].depth(), 0);
        assert_eq!(out[0].holes().len(), 1);
        assert_eq!(out[1].depth(), 2);
        assert_approx_eq!(area_mm2(&out), 1600.0 - 400.0 + 100.0, 1e-6);
    }

    #[test]
    fn area_and_contains_point_agree_with_region() {
        let region = region_of(vec![
            Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
            Polyline::rect(Point::new(40.0, 20.0), 20.0, 10.0).reversed(),
        ]);
        assert_approx_eq!(region.area_mm2(), 4800.0, EPS_MM);
        assert_approx_eq!(
            area_mm2(&[region.clone(), rect(200.0, 0.0, 10.0, 10.0)]),
            4900.0,
            EPS_MM
        );
        assert!(region.contains_point(Point::new(10.0, 10.0)));
        assert!(
            !region.contains_point(Point::new(50.0, 25.0)),
            "in the hole"
        );
        assert!(!region.contains_point(Point::new(200.0, 0.0)), "outside");
        assert!(
            region.contains_point(Point::new(0.0, 25.0)),
            "on the outer edge"
        );
        assert!(
            region.contains_point(Point::new(40.0, 25.0)),
            "on the hole edge"
        );
    }

    /// Pocket coverage (design, "Pocket" coverage check): the union of the
    /// footprint pieces must cover the inset region; the difference's area
    /// is the gap.
    #[test]
    fn difference_measures_uncovered_area() {
        let target = rect(0.0, 0.0, 100.0, 50.0);
        let covered = [rect(0.0, 0.0, 60.0, 50.0), rect(55.0, 0.0, 40.0, 50.0)];
        let gap = difference(&[target], &covered, TOL_MM);
        assert_eq!(gap.len(), 1);
        assert_approx_eq!(area_mm2(&gap), 5.0 * 50.0, 1e-6);
    }
}
