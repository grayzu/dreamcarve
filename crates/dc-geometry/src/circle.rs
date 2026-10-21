// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Circle recognition and fitting (`docs/specs/phase-2/design.md`,
//! "Algorithms → Drill" step 1 and "Algorithms → Arc fitting" step 2).
//! [`as_circle`] decides whether a closed polyline is a drillable hole;
//! [`fit_circle_kasa`] is the least-squares fit arc fitting starts from.

use crate::point::Point;
use crate::polyline::Polyline;

/// A closed polyline is a circle when every vertex and every segment
/// midpoint lies within `tolerance_mm` of the circle about the vertex
/// centroid with the mean vertex radius, and every arc segment's own centre
/// is within `tolerance_mm` of that centroid. Returns the centre and radius
/// in mm, or `None` for an open polyline, fewer than two vertices, a radius
/// at or below [`crate::EPS_MM`], or any point off the circle.
///
/// Segment midpoints are tested for lines as well as arcs: a square's four
/// corners are equidistant from its centre, but its edge midpoints are not,
/// so it is not mistaken for a hole. A polygon flattened from a circle at
/// `tolerance_mm` passes by construction; [`Polyline::circle`]'s two arcs
/// pass exactly.
#[must_use]
pub fn as_circle(polyline: &Polyline, tolerance_mm: f64) -> Option<(Point, f64)> {
    let _ = (polyline, tolerance_mm);
    None
}

/// Kasa least-squares circle through `points` (mm): the circle minimising
/// the sum of squared differences between `|p − c|²` and `r²`. Returns the
/// centre and radius, or `None` with fewer than three points, when the
/// points are collinear (or coincident) so no circle is determined, or
/// when the fit yields no positive radius.
#[must_use]
pub fn fit_circle_kasa(points: &[Point]) -> Option<(Point, f64)> {
    let _ = points;
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;
    use crate::polyline::Vertex;
    use std::f64::consts::{PI, TAU};

    /// A closed regular `n`-gon inscribed in the circle at `centre`, radius `r`.
    fn regular_polygon(centre: Point, r: f64, n: usize) -> Polyline {
        let vertices = (0..n)
            .map(|k| {
                let angle = TAU * (k as f64) / (n as f64);
                Vertex::line(centre.x + r * angle.cos(), centre.y + r * angle.sin())
            })
            .collect();
        Polyline::new(vertices, true)
    }

    /// A 16-gon inscribed in r = 5 has edge midpoints 5·(1 − cos(π/16)) ≈
    /// 0.096 mm inside the circle, so it is a circle at 0.1 mm but not at
    /// the default flattening tolerance of 0.01 mm.
    #[test]
    fn sixteen_facets_are_a_circle_within_tolerance() {
        let centre = Point::new(2.0, 3.0);
        let polygon = regular_polygon(centre, 5.0, 16);
        let (c, r) = as_circle(&polygon, 0.1).expect("16-gon is a circle at 0.1 mm");
        assert_approx_eq!(c.x, centre.x, 0.1);
        assert_approx_eq!(c.y, centre.y, 0.1);
        assert_approx_eq!(r, 5.0, 0.1);
        assert_eq!(
            as_circle(&polygon, FLATTEN_TOLERANCE_DEFAULT_MM),
            None,
            "edge sagitta of 0.096 mm exceeds 0.01 mm"
        );
    }

    #[test]
    fn two_bulge_arcs_are_a_circle() {
        let centre = Point::new(-3.0, 4.0);
        let circle = Polyline::circle(centre, 7.5);
        let (c, r) =
            as_circle(&circle, FLATTEN_TOLERANCE_DEFAULT_MM).expect("two arcs are a circle");
        assert_approx_eq!(c.x, centre.x, 1e-9);
        assert_approx_eq!(c.y, centre.y, 1e-9);
        assert_approx_eq!(r, 7.5, 1e-9);
    }

    #[test]
    fn ellipse_is_not_a_circle() {
        let vertices = (0..32)
            .map(|k| {
                let angle = TAU * (k as f64) / 32.0;
                Vertex::line(10.0 * angle.cos(), 6.0 * angle.sin())
            })
            .collect();
        let ellipse = Polyline::new(vertices, true);
        assert_eq!(as_circle(&ellipse, 0.1), None);
        assert_eq!(as_circle(&ellipse, 1.0), None);
    }

    /// A stadium: two r = 5 semicircles about (±5, 0) joined by lines. Its
    /// four vertices are all √50 from the centroid, so only the arc
    /// midpoints and arc centres give it away.
    #[test]
    fn slot_is_not_a_circle() {
        let slot = Polyline::new(
            vec![
                Vertex::new(5.0, -5.0, 1.0),
                Vertex::line(5.0, 5.0),
                Vertex::new(-5.0, 5.0, 1.0),
                Vertex::line(-5.0, -5.0),
            ],
            true,
        );
        assert_approx_eq!(slot.signed_area_mm2(), 100.0 + PI * 25.0, 1e-9);
        assert_eq!(as_circle(&slot, 0.1), None);
    }

    /// Corners alone do not make a circle: a square's edge midpoints are
    /// 0.29 of the radius inside it.
    #[test]
    fn square_is_not_a_circle() {
        let square = Polyline::rect(Point::new(0.0, 0.0), 10.0, 10.0);
        assert_eq!(as_circle(&square, 0.1), None);
    }

    #[test]
    fn open_polyline_is_not_a_circle() {
        let open = Polyline::new(
            regular_polygon(Point::new(0.0, 0.0), 5.0, 16)
                .vertices()
                .to_vec(),
            false,
        );
        assert_eq!(as_circle(&open, 1.0), None);
    }

    #[test]
    fn kasa_fit_recovers_centre_and_radius() {
        let centre = Point::new(3.0, -4.0);
        let points: Vec<Point> = (0..12)
            .map(|k| {
                let angle = TAU * (k as f64) / 12.0 + 0.3;
                Point::new(centre.x + 7.0 * angle.cos(), centre.y + 7.0 * angle.sin())
            })
            .collect();
        let (c, r) = fit_circle_kasa(&points).expect("twelve points on a circle fit");
        assert_approx_eq!(c.x, centre.x, 1e-9);
        assert_approx_eq!(c.y, centre.y, 1e-9);
        assert_approx_eq!(r, 7.0, 1e-9);
    }

    /// Arc fitting (design, "Arc fitting" step 2) fits short runs of a
    /// pocket ring; a quarter arc's points must fit as well as a full
    /// circle's.
    #[test]
    fn kasa_fit_of_a_quarter_arc_recovers_the_circle() {
        let points: Vec<Point> = (0..=8)
            .map(|k| {
                let angle = (PI / 2.0) * (k as f64) / 8.0;
                Point::new(100.0 + 20.0 * angle.cos(), 50.0 + 20.0 * angle.sin())
            })
            .collect();
        let (c, r) = fit_circle_kasa(&points).expect("quarter arc fits");
        assert_approx_eq!(c.x, 100.0, 1e-6);
        assert_approx_eq!(c.y, 50.0, 1e-6);
        assert_approx_eq!(r, 20.0, 1e-6);
    }

    #[test]
    fn kasa_fit_of_collinear_points_is_none() {
        let line = [
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(3.0, 3.0),
        ];
        assert_eq!(fit_circle_kasa(&line), None);
        let same = [Point::new(1.0, 1.0); 3];
        assert_eq!(fit_circle_kasa(&same), None);
    }

    #[test]
    fn kasa_fit_needs_three_points() {
        assert_eq!(fit_circle_kasa(&[]), None);
        assert_eq!(
            fit_circle_kasa(&[Point::new(0.0, 0.0), Point::new(1.0, 0.0)]),
            None
        );
    }
}
