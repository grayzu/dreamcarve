// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Regions: an outer contour with zero or more holes, and [`nest`], which
//! groups loose contours into regions by even-odd containment
//! (`docs/specs/phase-1/design.md`, "Data model → dc-geometry").

use crate::contour::Contour;
use crate::point::Point;

/// An area to machine: one counter-clockwise outer contour and zero or more
/// clockwise holes inside it (ADR-0001). Coordinates are millimetres, Y-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    outer: Contour,
    holes: Vec<Contour>,
}

impl Region {
    /// The outer boundary; always counter-clockwise.
    #[must_use]
    pub fn outer(&self) -> &Contour {
        &self.outer
    }

    /// The holes, in the input order given to [`nest`]; each is clockwise
    /// and lies inside [`Region::outer`].
    #[must_use]
    pub fn holes(&self) -> &[Contour] {
        &self.holes
    }
}

/// Groups closed contours (millimetres, Y-up) into regions by even-odd
/// containment. A contour inside no other contour (depth 0) is an outer;
/// one inside exactly one (depth 1) is a hole of its innermost container;
/// depth 2 is an island and starts a new region, and so on. Outers come
/// back counter-clockwise and holes clockwise, whatever the input winding.
///
/// Contours are expected not to cross one another. Regions are ordered by
/// the input position of their outer contour, so the result does not
/// depend on anything but the input.
#[must_use]
pub fn nest(contours: Vec<Contour>) -> Vec<Region> {
    let _ = contours;
    todo!("nest contours into regions (#6)")
}

/// The winding number of `contour` about `p`: +1 inside a counter-clockwise
/// contour, −1 inside a clockwise one, 0 outside. Arc segments count by
/// their true curve, not their chord. Undefined for `p` on the boundary.
fn winding_number(contour: &Contour, p: Point) -> i32 {
    let _ = (contour, p);
    todo!("winding number over line and arc segments (#6)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::point::EPS_MM;
    use crate::polyline::Polyline;
    use std::f64::consts::PI;

    fn circle(x: f64, y: f64, r: f64) -> Contour {
        Contour::new(Polyline::circle(Point::new(x, y), r)).unwrap()
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(x, y), w, h)).unwrap()
    }

    fn area(c: &Contour) -> f64 {
        c.signed_area_mm2()
    }

    #[test]
    fn circle_in_circle_becomes_hole() {
        let regions = nest(vec![circle(0.0, 0.0, 10.0), circle(0.0, 0.0, 5.0)]);
        assert_eq!(regions.len(), 1);
        let region = &regions[0];
        assert!(region.outer().is_ccw());
        assert_approx_eq!(area(region.outer()), PI * 100.0, 1e-9);
        assert_eq!(region.holes().len(), 1);
        assert!(!region.holes()[0].is_ccw(), "hole must be clockwise");
        assert_approx_eq!(area(&region.holes()[0]), -PI * 25.0, 1e-9);
    }

    #[test]
    fn disjoint_shapes_are_separate_regions() {
        let regions = nest(vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 10.0, 5.0)]);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.holes().is_empty()));
        assert!(regions.iter().all(|r| r.outer().is_ccw()));
        assert_approx_eq!(area(regions[0].outer()), 100.0, EPS_MM);
        assert_approx_eq!(area(regions[1].outer()), 50.0, EPS_MM);
    }

    #[test]
    fn island_in_hole_becomes_its_own_region() {
        let regions = nest(vec![
            circle(0.0, 0.0, 10.0),
            circle(0.0, 0.0, 7.0),
            circle(0.0, 0.0, 3.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), PI * 100.0, 1e-9);
        assert_eq!(regions[0].holes().len(), 1);
        assert_approx_eq!(area(&regions[0].holes()[0]), -PI * 49.0, 1e-9);
        assert_approx_eq!(area(regions[1].outer()), PI * 9.0, 1e-9);
        assert!(regions[1].outer().is_ccw());
        assert!(regions[1].holes().is_empty());
    }

    #[test]
    fn four_levels_alternate_outer_and_hole() {
        let regions = nest(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            rect(5.0, 5.0, 30.0, 30.0),
            rect(10.0, 10.0, 20.0, 20.0),
            rect(15.0, 15.0, 10.0, 10.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), 1600.0, EPS_MM);
        assert_approx_eq!(area(&regions[0].holes()[0]), -900.0, EPS_MM);
        assert_approx_eq!(area(regions[1].outer()), 400.0, EPS_MM);
        assert_approx_eq!(area(&regions[1].holes()[0]), -100.0, EPS_MM);
    }

    #[test]
    fn circle_inside_arc_bulge_is_hole() {
        // The outer circle is two half-circle arcs whose chords coincide on
        // the X axis; a chord-only test would see no area at all. The inner
        // circle sits wholly in the upper bulge.
        let regions = nest(vec![circle(0.0, 0.0, 10.0), circle(0.0, 6.0, 2.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
    }

    #[test]
    fn circle_outside_arc_bulge_is_separate() {
        // Inside the outer circle's bounding box but outside the curve.
        let regions = nest(vec![circle(0.0, 0.0, 10.0), circle(9.0, 9.0, 1.0)]);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.holes().is_empty()));
    }

    #[test]
    fn several_holes_in_one_outer_keep_input_order() {
        let regions = nest(vec![
            rect(0.0, 0.0, 100.0, 50.0),
            circle(20.0, 25.0, 5.0),
            circle(50.0, 25.0, 3.0),
            circle(80.0, 25.0, 4.0),
        ]);
        assert_eq!(regions.len(), 1);
        let holes = regions[0].holes();
        assert_eq!(holes.len(), 3);
        assert!(holes.iter().all(|h| !h.is_ccw()));
        assert_approx_eq!(area(&holes[0]), -PI * 25.0, 1e-9);
        assert_approx_eq!(area(&holes[1]), -PI * 9.0, 1e-9);
        assert_approx_eq!(area(&holes[2]), -PI * 16.0, 1e-9);
    }

    #[test]
    fn containment_does_not_depend_on_input_order() {
        let a = circle(0.0, 0.0, 10.0);
        let b = circle(0.0, 0.0, 7.0);
        let c = circle(0.0, 0.0, 3.0);
        let d = rect(30.0, 0.0, 5.0, 5.0);
        let orders = [
            vec![a.clone(), b.clone(), c.clone(), d.clone()],
            vec![c.clone(), b.clone(), a.clone(), d.clone()],
            vec![d.clone(), c.clone(), a.clone(), b.clone()],
            vec![b.clone(), d.clone(), c.clone(), a.clone()],
        ];
        for input in orders {
            // Areas rounded to whole mm², as strings so they compare exactly.
            let mut summary: Vec<String> = nest(input)
                .iter()
                .map(|r| {
                    let holes: Vec<String> = r
                        .holes()
                        .iter()
                        .map(|h| format!("{:.0}", area(h)))
                        .collect();
                    format!("{:.0}[{}]", area(r.outer()), holes.join(","))
                })
                .collect();
            summary.sort();
            assert_eq!(summary, vec!["25[]", "28[]", "314[-154]"]);
        }
    }

    #[test]
    fn regions_follow_input_order_of_outers() {
        // The hole comes first in the input; region order follows the outers.
        let regions = nest(vec![
            circle(50.0, 0.0, 2.0),
            rect(100.0, 0.0, 5.0, 5.0),
            circle(50.0, 0.0, 8.0),
        ]);
        assert_eq!(regions.len(), 2);
        assert_approx_eq!(area(regions[0].outer()), 25.0, EPS_MM);
        assert!(regions[0].holes().is_empty());
        assert_approx_eq!(area(regions[1].outer()), PI * 64.0, 1e-9);
        assert_eq!(regions[1].holes().len(), 1);
    }

    #[test]
    fn lone_contour_is_one_region_without_holes() {
        let regions = nest(vec![circle(3.0, 4.0, 2.0)]);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].outer().is_ccw());
        assert!(regions[0].holes().is_empty());
    }

    #[test]
    fn no_contours_give_no_regions() {
        assert!(nest(Vec::new()).is_empty());
    }

    #[test]
    fn clockwise_input_is_normalised() {
        let regions = nest(vec![
            rect(0.0, 0.0, 10.0, 10.0).reversed(),
            rect(2.0, 2.0, 2.0, 2.0),
        ]);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].outer().is_ccw());
        assert!(!regions[0].holes()[0].is_ccw());
    }

    #[test]
    fn winding_counts_arc_bulge_not_chord() {
        let c = circle(0.0, 0.0, 5.0);
        assert_eq!(winding_number(&c, Point::new(0.0, 4.0)), 1);
        assert_eq!(winding_number(&c, Point::new(0.0, -4.0)), 1);
        assert_eq!(winding_number(&c, Point::new(0.0, 0.5)), 1);
        assert_eq!(winding_number(&c, Point::new(4.0, 4.0)), 0);
        assert_eq!(winding_number(&c, Point::new(-20.0, 0.0)), 0);
        assert_eq!(winding_number(&c.reversed(), Point::new(0.0, 4.0)), -1);
    }

    #[test]
    fn winding_of_rect_is_one_inside_zero_outside() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(winding_number(&r, Point::new(5.0, 5.0)), 1);
        assert_eq!(winding_number(&r, Point::new(15.0, 5.0)), 0);
        assert_eq!(winding_number(&r, Point::new(5.0, -1.0)), 0);
    }
}
