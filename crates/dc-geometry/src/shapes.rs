// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/types/project.ts (rectProfile, ellipseProfile, slotProfile, ngonProfile)
//   src/store/helpers/cannedRectProfiles.ts
//   src/types/slot.test.ts
//   src/types/ngon.test.ts
//   src/store/helpers/cannedRectProfiles.test.ts

//! The outlines of the shapes drawn by their sizes: rectangle, circle,
//! ellipse, regular polygon, slot and gear (`docs/specs/phase-5/design.md`,
//! "Algorithms → Shape outlines"; ADR-0006).
//!
//! Every outline is in the shape's own space, centred on the origin,
//! millimetres, Y-up (ADR-0001), closed and counter-clockwise, and starts
//! where its function says, so the same sizes always give the same
//! vertices. An instance's transform places it.
//!
//! Re-derived for Y-up: PureCutCNC's constructors wind differently from
//! one shape to the next (its rectangle one way, its circle and slot the
//! other) and leave consumers to normalise. Here each is written out
//! counter-clockwise, arcs are bulges, not a centre and a direction flag,
//! and a slot's start is below its centreline, where upstream's start is
//! on the `(-sin θ, cos θ)` side, below it on a Y-down screen.
//!
//! Sizes are the caller's to check (`dc-project` validates a shape's
//! sizes). A size that cannot be gives a degenerate outline, never a
//! panic.

pub mod gear;

use crate::point::{EPS_MM, Point};
use crate::polyline::{Polyline, Vertex};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_8, PI, TAU};

/// The fewest straight segments an [`ellipse`] is drawn with, however
/// coarse the tolerance.
const MIN_ELLIPSE_SEGMENTS: usize = 16;

/// The most straight segments an [`ellipse`] is drawn with, however fine
/// the tolerance: a 6 m ellipse at 1 µm needs about 7,700.
const MAX_ELLIPSE_SEGMENTS: usize = 16_384;

/// A rectangle `width_mm` × `height_mm` with sharp corners, from its
/// bottom-left corner (`rectProfile`).
#[must_use]
pub fn rectangle(width_mm: f64, height_mm: f64) -> Polyline {
    Polyline::rect(
        Point::new(-width_mm / 2.0, -height_mm / 2.0),
        width_mm,
        height_mm,
    )
}

/// A rectangle whose corners are rounded by arcs of `radius_mm`, each
/// tangent to both sides (`roundedRectProfile`): four lines and four
/// quarter-circle arcs, from the left end of the bottom side.
///
/// The radius is held to half the shorter side. There the two arcs of a
/// short end meet and the side between them has no length, so it is left
/// out: a 20 × 20 rectangle with 10 mm corners is a circle of four arcs.
/// A radius of 0 or less gives [`rectangle`].
#[must_use]
pub fn rounded_rectangle(width_mm: f64, height_mm: f64, radius_mm: f64) -> Polyline {
    let radius = radius_mm.min(width_mm.min(height_mm) / 2.0);
    if radius.is_nan() || radius <= 0.0 {
        return rectangle(width_mm, height_mm);
    }
    // tan(θ/4) of a quarter turn, counter-clockwise.
    let quarter = FRAC_PI_8.tan();
    let (x, y) = (width_mm / 2.0, height_mm / 2.0);
    cornered(&[
        Vertex::line(-x + radius, -y),
        Vertex::new(x - radius, -y, quarter),
        Vertex::line(x, -y + radius),
        Vertex::new(x, y - radius, quarter),
        Vertex::line(x - radius, y),
        Vertex::new(-x + radius, y, quarter),
        Vertex::line(-x, y - radius),
        Vertex::new(-x, -y + radius, quarter),
    ])
}

/// A rectangle whose corners are cut off straight, `distance_mm` along
/// each side (`chamferedRectProfile`): eight lines, from the left end of
/// the bottom side.
///
/// The distance is held to half the shorter side, where the side between
/// two cuts has no length and is left out. A distance of 0 or less gives
/// [`rectangle`].
#[must_use]
pub fn chamfered_rectangle(width_mm: f64, height_mm: f64, distance_mm: f64) -> Polyline {
    let d = distance_mm.min(width_mm.min(height_mm) / 2.0);
    if d.is_nan() || d <= 0.0 {
        return rectangle(width_mm, height_mm);
    }
    let (x, y) = (width_mm / 2.0, height_mm / 2.0);
    cornered(&[
        Vertex::line(-x + d, -y),
        Vertex::line(x - d, -y),
        Vertex::line(x, -y + d),
        Vertex::line(x, y - d),
        Vertex::line(x - d, y),
        Vertex::line(-x + d, y),
        Vertex::line(-x, y - d),
        Vertex::line(-x, -y + d),
    ])
}

/// A closed outline through `vertices` without the straight sides that
/// have no length: a vertex whose line to the next vertex is shorter
/// than [`EPS_MM`] is left out. Arcs are kept whatever their length.
fn cornered(vertices: &[Vertex]) -> Polyline {
    let kept = vertices
        .iter()
        .zip(vertices.iter().cycle().skip(1))
        .filter(|(vertex, next)| {
            vertex.bulge.abs() > 0.0 || vertex.point().distance_to(next.point()) > EPS_MM
        })
        .map(|(vertex, _)| *vertex)
        .collect();
    Polyline::new(kept, true)
}

/// A circle of `diameter_mm`: two half-circle arcs, from its point on +X
/// ([`Polyline::circle`]).
#[must_use]
pub fn circle(diameter_mm: f64) -> Polyline {
    Polyline::circle(Point::new(0.0, 0.0), diameter_mm / 2.0)
}

/// An ellipse `width_mm` across X and `height_mm` across Y, as straight
/// segments no further than `tolerance_mm` from the true curve, from its
/// point on +X.
///
/// Its vertices lie on the ellipse, the four axis points among them, so
/// its bounding box is exactly its sizes and it is symmetric about both
/// axes. A polyline has no elliptical arc; the program gets arcs back
/// from arc fitting at export, as an imported curve does. PureCutCNC's
/// ellipse is four Béziers (`ellipseProfile`), flattened later at a fixed
/// count.
#[must_use]
pub fn ellipse(width_mm: f64, height_mm: f64, tolerance_mm: f64) -> Polyline {
    let (a, b) = (width_mm / 2.0, height_mm / 2.0);
    let per_quadrant = ellipse_segments(a.max(b), tolerance_mm) / 4;
    // The first quadrant's vertices, the +X point first and the +Y point
    // not among them; the other three quadrants mirror these, so the
    // outline is symmetric to the last bit.
    let quadrant: Vec<(f64, f64)> = (0..per_quadrant)
        .map(|k| {
            let angle = FRAC_PI_2 * to_f64(k) / to_f64(per_quadrant);
            (a * angle.cos(), b * angle.sin())
        })
        .collect();
    let mut vertices = Vec::with_capacity(per_quadrant * 4);
    vertices.extend(quadrant.iter().map(|&(x, y)| Vertex::line(x, y)));
    vertices.push(Vertex::line(0.0, b));
    vertices.extend(
        quadrant
            .iter()
            .skip(1)
            .rev()
            .map(|&(x, y)| Vertex::line(-x, y)),
    );
    vertices.extend(quadrant.iter().map(|&(x, y)| Vertex::line(-x, -y)));
    vertices.push(Vertex::line(0.0, -b));
    vertices.extend(
        quadrant
            .iter()
            .skip(1)
            .rev()
            .map(|&(x, y)| Vertex::line(x, -y)),
    );
    Polyline::new(vertices, true)
}

/// How many straight segments an ellipse of greatest radius
/// `radius_mm` needs to stay within `tolerance_mm`, a multiple of four.
///
/// The ellipse is a circle of that radius squeezed along one axis, which
/// brings no chord further from its curve. A circle's chords of angle
/// `θ` stray `r·(1 − cos(θ/2))` from it, so `θ = 2·acos(1 − t/r)`.
fn ellipse_segments(radius_mm: f64, tolerance_mm: f64) -> usize {
    let fraction = tolerance_mm / radius_mm;
    if !(fraction > 0.0 && fraction < 1.0) {
        // No radius, no tolerance, or a tolerance as large as the shape.
        return MIN_ELLIPSE_SEGMENTS;
    }
    let step = 2.0 * (1.0 - fraction).acos();
    let needed = (TAU / step).ceil();
    // `needed` is finite and at least 1: `step` is in (0, π).
    let needed = if needed >= to_f64(MAX_ELLIPSE_SEGMENTS) {
        MAX_ELLIPSE_SEGMENTS
    } else {
        // In range of `usize` by the test above.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive whole number under MAX_ELLIPSE_SEGMENTS"
        )]
        let needed = needed as usize;
        needed
    };
    needed
        .clamp(MIN_ELLIPSE_SEGMENTS, MAX_ELLIPSE_SEGMENTS)
        .div_ceil(4)
        * 4
}

/// A count as a float, exactly: every count here is far below 2⁵³.
#[allow(
    clippy::cast_precision_loss,
    reason = "segment and side counts, far below 2^53"
)]
const fn to_f64(count: usize) -> f64 {
    count as f64
}

/// A regular polygon of `sides` sides whose corners are `corner_radius_mm`
/// from its centre, the first corner on +X (`ngonProfile`). Fewer than
/// three sides give an outline that encloses nothing.
#[must_use]
pub fn regular_polygon(sides: u32, corner_radius_mm: f64) -> Polyline {
    let vertices = (0..sides)
        .map(|i| {
            let angle = TAU * f64::from(i) / f64::from(sides);
            Vertex::line(
                corner_radius_mm * angle.cos(),
                corner_radius_mm * angle.sin(),
            )
        })
        .collect();
    Polyline::new(vertices, true)
}

/// The corner radius of the regular polygon of `sides` sides whose sides'
/// middles are `flats_radius_mm` from its centre: half a hexagon's
/// spanner size is its flats radius. PureCutCNC sizes a polygon by its
/// corners only.
#[must_use]
pub fn corner_radius_of_flats(sides: u32, flats_radius_mm: f64) -> f64 {
    flats_radius_mm / (PI / f64::from(sides)).cos()
}

/// A slot: two straight sides `width_mm` apart joined by two half
/// circles centred `length_mm` apart on the X axis (`slotProfile`;
/// CONTEXT: Slot). Line, arc, line, arc, from the left end of the bottom
/// side; each arc is an exact half circle (a bulge of 1), so it reaches
/// the program as one G2 or G3.
#[must_use]
pub fn slot(length_mm: f64, width_mm: f64) -> Polyline {
    let (x, y) = (length_mm / 2.0, width_mm / 2.0);
    Polyline::new(
        vec![
            Vertex::line(-x, -y),
            Vertex::new(x, -y, 1.0),
            Vertex::line(x, y),
            Vertex::new(-x, y, 1.0),
        ],
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::polyline::Segment;

    const TOL: f64 = 1e-9;

    fn lines_and_arcs(outline: &Polyline) -> (usize, usize) {
        let lines = outline.segments().filter(Segment::is_line).count();
        (lines, outline.segments().count() - lines)
    }

    fn assert_box(outline: &Polyline, width_mm: f64, height_mm: f64) {
        let bounds = outline.bounds().unwrap();
        assert_approx_eq!(bounds.min.x, -width_mm / 2.0, TOL);
        assert_approx_eq!(bounds.max.x, width_mm / 2.0, TOL);
        assert_approx_eq!(bounds.min.y, -height_mm / 2.0, TOL);
        assert_approx_eq!(bounds.max.y, height_mm / 2.0, TOL);
    }

    /// Every outline is closed and counter-clockwise (ADR-0001), which
    /// upstream's constructors are not as a set: its circle and slot wind
    /// the other way from its rectangle (digest §10.2).
    #[test]
    fn every_outline_is_closed_and_counter_clockwise() {
        let outlines = [
            ("rectangle", rectangle(60.0, 30.0)),
            ("rounded", rounded_rectangle(60.0, 30.0, 5.0)),
            ("chamfered", chamfered_rectangle(60.0, 30.0, 5.0)),
            ("circle", circle(20.0)),
            ("ellipse", ellipse(40.0, 20.0, 0.01)),
            ("polygon", regular_polygon(6, 10.0)),
            ("slot", slot(40.0, 12.0)),
        ];
        for (name, outline) in outlines {
            assert!(outline.is_closed(), "{name}");
            assert!(outline.signed_area_mm2() > 0.0, "{name} is clockwise");
        }
    }

    // --- roundedRectProfile (cannedRectProfiles.test.ts) ---

    /// `testRoundedSegmentCount`, `testRoundedClosure`: four lines and
    /// four arcs. (Upstream's rectangle is given by two corners; here it
    /// is centred, and the instance's transform places it.)
    #[test]
    fn rounded_segment_count() {
        let outline = rounded_rectangle(100.0, 60.0, 10.0);
        assert_eq!(lines_and_arcs(&outline), (4, 4));
        assert!(outline.is_closed());
    }

    /// `testRoundedBoundsPreserved`.
    #[test]
    fn rounded_bounds_preserved() {
        assert_box(&rounded_rectangle(100.0, 60.0, 10.0), 100.0, 60.0);
    }

    /// `testRoundedArcRadii`, and more than upstream asks: each arc is a
    /// quarter turn tangent to the two sides it joins, so the outline has
    /// no corner.
    #[test]
    fn rounded_corners_are_tangent_arcs() {
        let radius = 8.0;
        let outline = rounded_rectangle(100.0, 60.0, radius);
        let segments: Vec<Segment> = outline.segments().collect();
        for (i, segment) in segments.iter().enumerate() {
            let Some(arc) = segment.arc() else { continue };
            assert_approx_eq!(arc.radius_mm, radius, 1e-9);
            assert_approx_eq!(arc.sweep_rad, FRAC_PI_2, 1e-9);
            // The arc leaves along the line before it and arrives along
            // the line after it.
            let before = segments[(i + segments.len() - 1) % segments.len()];
            let after = segments[(i + 1) % segments.len()];
            let (_, leaving) = segment.point_at(0.0).unwrap();
            let (_, arriving) = segment.point_at(segment.length_mm()).unwrap();
            let along_before = (before.end - before.start).normalized().unwrap();
            let along_after = (after.end - after.start).normalized().unwrap();
            assert_approx_eq!(leaving.cross(along_before), 0.0, 1e-9);
            assert_approx_eq!(arriving.cross(along_after), 0.0, 1e-9);
            assert!(leaving.dot(along_before) > 0.0 && arriving.dot(along_after) > 0.0);
        }
    }

    /// `testRoundedZeroRadius`.
    #[test]
    fn rounded_zero_radius_returns_plain_rect() {
        let outline = rounded_rectangle(100.0, 60.0, 0.0);
        assert_eq!(lines_and_arcs(&outline), (4, 0));
        assert_eq!(outline, rectangle(100.0, 60.0));
    }

    /// `testRoundedOversizedClamp`: width 100, height 60, so a radius of
    /// 50 is held to 30. Upstream stops a micrometre short of half the
    /// side to keep a sliver of line; here the two arcs of each short
    /// end meet and the side between them is left out.
    #[test]
    fn rounded_oversized_radius_is_held_to_half_the_shorter_side() {
        let outline = rounded_rectangle(100.0, 60.0, 50.0);
        assert_eq!(lines_and_arcs(&outline), (2, 4));
        for arc in outline.segments().filter_map(|s| s.arc()) {
            assert_approx_eq!(arc.radius_mm, 30.0, 1e-9);
        }
        assert_box(&outline, 100.0, 60.0);
        // A square with corners of half its side is a circle.
        let round = rounded_rectangle(20.0, 20.0, 10.0);
        assert_eq!(lines_and_arcs(&round), (0, 4));
        assert_approx_eq!(round.length_mm(), TAU * 10.0, 1e-9);
    }

    /// `testRoundedZeroSizeRect`: nothing to round, and no panic.
    #[test]
    fn rounded_zero_size_rect_is_a_plain_rect() {
        let outline = rounded_rectangle(0.0, 0.0, 10.0);
        assert_eq!(lines_and_arcs(&outline), (4, 0));
        let nan = rounded_rectangle(f64::NAN, 10.0, 2.0);
        assert_eq!(nan.vertices().len(), 4);
    }

    // --- chamferedRectProfile ---

    /// `testChamferedSegmentCount`, `testChamferedClosure`,
    /// `testChamferedBoundsPreserved`.
    #[test]
    fn chamfered_segment_count_and_bounds() {
        let outline = chamfered_rectangle(100.0, 60.0, 10.0);
        assert_eq!(lines_and_arcs(&outline), (8, 0));
        assert!(outline.is_closed());
        assert_box(&outline, 100.0, 60.0);
    }

    /// `testChamferedSymmetricCuts`: four cuts of `d·√2` and four sides
    /// of the rectangle less two cuts.
    #[test]
    fn chamfered_cuts_are_symmetric() {
        let outline = chamfered_rectangle(100.0, 60.0, 10.0);
        let mut lengths: Vec<f64> = outline.segments().map(|s| s.length_mm()).collect();
        lengths.sort_by(f64::total_cmp);
        let cut = 10.0 * std::f64::consts::SQRT_2;
        for (length, expected) in lengths
            .iter()
            .zip([cut, cut, cut, cut, 40.0, 40.0, 80.0, 80.0])
        {
            assert_approx_eq!(*length, expected, 1e-9);
        }
    }

    /// `testChamferedZeroDistance`, `testChamferedOversizedClamp`.
    #[test]
    fn chamfered_zero_and_oversized_distances() {
        assert_eq!(
            chamfered_rectangle(100.0, 60.0, 0.0),
            rectangle(100.0, 60.0)
        );
        let outline = chamfered_rectangle(100.0, 60.0, 50.0);
        // Held to 30: the short sides are gone, six lines are left.
        assert_eq!(lines_and_arcs(&outline), (6, 0));
        assert_box(&outline, 100.0, 60.0);
    }

    // --- slotProfile (slot.test.ts) ---

    /// `testSegmentCountAndTypes`.
    #[test]
    fn slot_segment_count_and_types() {
        let kinds: Vec<bool> = slot(10.0, 4.0).segments().map(|s| s.is_line()).collect();
        assert_eq!(kinds, [true, false, true, false], "line, arc, line, arc");
    }

    /// `testArcCenters`, `testArcRadii`, `testConnectivity`: the arcs are
    /// centred on the two end centres, `length_mm` apart on the X axis,
    /// with a radius of half the width.
    #[test]
    fn slot_arcs_are_half_circles_about_the_end_centres() {
        let outline = slot(10.0, 4.0);
        let arcs: Vec<_> = outline.segments().filter_map(|s| s.arc()).collect();
        assert_eq!(arcs.len(), 2);
        for (arc, centre_x) in arcs.iter().zip([5.0, -5.0]) {
            assert_approx_eq!(arc.centre.x, centre_x, 1e-10);
            assert_approx_eq!(arc.centre.y, 0.0, 1e-10);
            assert_approx_eq!(arc.radius_mm, 2.0, 1e-10);
            assert_approx_eq!(arc.sweep_rad, PI, 1e-10);
        }
        // Exact half circles: a bulge of one, not one and a rounding.
        assert_eq!(outline.vertices()[1].bulge.to_bits(), 1.0f64.to_bits());
        assert_eq!(outline.vertices()[3].bulge.to_bits(), 1.0f64.to_bits());
    }

    /// `testClosure`, `testHorizontalSlot`. Upstream's start is the point
    /// A on the `(-sin θ, cos θ)` side of the first centre; in Y-up a
    /// counter-clockwise slot starts on the other side, below the
    /// centreline.
    #[test]
    fn slot_is_closed_and_starts_below_its_first_centre() {
        let outline = slot(10.0, 4.0);
        assert!(outline.is_closed());
        let start = outline.vertices()[0];
        assert_approx_eq!(start.x, -5.0, 1e-10);
        assert_approx_eq!(start.y, -2.0, 1e-10);
        assert_box(&outline, 14.0, 4.0);
        assert_approx_eq!(outline.length_mm(), 20.0 + TAU * 2.0, 1e-9);
    }

    /// `testRotated45`: upstream builds a slot at an angle; here the
    /// angle is the instance's transform, and the slot keeps its arcs
    /// under it.
    #[test]
    fn slot_rotated_45_degrees_keeps_its_arcs() {
        let turned = crate::Transform::rotate(std::f64::consts::FRAC_PI_4)
            .apply_polyline(&slot(10.0, 4.0))
            .unwrap();
        let arcs: Vec<_> = turned.segments().filter_map(|s| s.arc()).collect();
        assert_eq!(arcs.len(), 2);
        let half = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert_approx_eq!(arcs[0].centre.x, half, 1e-9);
        assert_approx_eq!(arcs[0].centre.y, half, 1e-9);
        assert_approx_eq!(arcs[1].centre.x, -half, 1e-9);
        assert_approx_eq!(arcs[1].radius_mm, 2.0, 1e-9);
    }

    // --- ngonProfile (ngon.test.ts) ---

    /// `testSegmentCount`, `testEquilateral`, `testCircumradius`,
    /// `testClosure`.
    #[test]
    fn polygon_sides_are_equal_and_corners_on_the_radius() {
        for sides in [3, 4, 5, 6, 8, 12, 50] {
            let outline = regular_polygon(sides, 10.0);
            assert!(outline.is_closed());
            assert_eq!(outline.vertices().len(), sides as usize);
            let side = 2.0 * 10.0 * (PI / f64::from(sides)).sin();
            for segment in outline.segments() {
                assert!(segment.is_line(), "{sides} sides");
                assert_approx_eq!(segment.length_mm(), side, 1e-9);
            }
            for vertex in outline.vertices() {
                assert_approx_eq!(vertex.point().distance_to(Point::new(0.0, 0.0)), 10.0, 1e-9);
            }
        }
    }

    /// `testFirstVertexAngle`: the first corner is on +X; the angle
    /// upstream takes as a parameter is the instance's rotation here.
    #[test]
    fn polygon_first_corner_is_on_plus_x() {
        let first = regular_polygon(6, 10.0).vertices()[0];
        assert_approx_eq!(first.x, 10.0, 1e-12);
        assert_approx_eq!(first.y, 0.0, 1e-12);
    }

    /// `testTriangleArea`, `testHexagonArea`.
    #[test]
    fn polygon_areas() {
        let r: f64 = 10.0;
        let triangle = 3.0 * 3f64.sqrt() / 4.0 * r * r;
        assert_approx_eq!(regular_polygon(3, r).signed_area_mm2(), triangle, 1e-9);
        let hexagon = 3.0 * 3f64.sqrt() / 2.0 * r * r;
        assert_approx_eq!(regular_polygon(6, r).signed_area_mm2(), hexagon, 1e-9);
    }

    /// Measured to the flats, the middle of every side is at the radius:
    /// a 13 mm hexagon across the flats has a flats radius of 6.5.
    #[test]
    fn polygon_to_the_flats() {
        for sides in [3, 4, 6, 7] {
            let outline = regular_polygon(sides, corner_radius_of_flats(sides, 6.5));
            for segment in outline.segments() {
                let (middle, _) = segment.point_at(segment.length_mm() / 2.0).unwrap();
                assert_approx_eq!(middle.distance_to(Point::new(0.0, 0.0)), 6.5, 1e-9);
            }
        }
        assert_approx_eq!(corner_radius_of_flats(6, 6.5), 13.0 / 3f64.sqrt(), 1e-12);
    }

    // --- ellipse ---

    /// Every vertex is on the ellipse and every chord's middle within the
    /// tolerance of it; the box is the sizes exactly; the outline starts
    /// on +X and is symmetric about both axes.
    #[test]
    fn an_ellipse_stays_within_tolerance() {
        for (width_mm, height_mm, tolerance_mm) in [
            (40.0, 20.0, 0.01),
            (20.0, 40.0, 0.01),
            (300.0, 5.0, 0.001),
            (1.0, 1.0, 0.1),
        ] {
            let outline = ellipse(width_mm, height_mm, tolerance_mm);
            let (a, b) = (width_mm / 2.0, height_mm / 2.0);
            assert_box(&outline, width_mm, height_mm);
            let vertices = outline.vertices();
            assert_eq!(vertices.len() % 4, 0);
            assert_approx_eq!(vertices[0].x, a, 1e-12);
            assert_approx_eq!(vertices[0].y, 0.0, 1e-12);
            // The implicit equation's value is 1 on the curve; off it by
            // `d` along the normal it changes by at least `2d / max(a, b)`.
            let off_by = |p: Point| {
                let value = (p.x / a).powi(2) + (p.y / b).powi(2);
                (value - 1.0).abs() * a.max(b) / 2.0
            };
            for segment in outline.segments() {
                assert!(segment.is_line());
                assert!(off_by(segment.start) <= 1e-9, "a vertex is off the curve");
                let (middle, _) = segment.point_at(segment.length_mm() / 2.0).unwrap();
                let off = off_by(middle);
                assert!(
                    off <= tolerance_mm,
                    "{width_mm} × {height_mm}: a chord strays {off} mm"
                );
            }
            let n = vertices.len();
            for (i, v) in vertices.iter().enumerate() {
                let across = vertices[(n / 2 + i) % n];
                assert_eq!((v.x + across.x).to_bits(), 0.0f64.to_bits(), "vertex {i}");
                assert_eq!((v.y + across.y).to_bits(), 0.0f64.to_bits(), "vertex {i}");
            }
        }
    }

    /// A finer tolerance gives more segments, within the two limits, and
    /// no size makes the count run away.
    #[test]
    fn an_ellipses_segment_count_follows_the_tolerance() {
        let coarse = ellipse(40.0, 20.0, 0.1).vertices().len();
        let fine = ellipse(40.0, 20.0, 0.001).vertices().len();
        assert!(fine > coarse * 5, "{coarse} then {fine}");
        assert_eq!(
            ellipse(40.0, 20.0, 100.0).vertices().len(),
            MIN_ELLIPSE_SEGMENTS
        );
        assert_eq!(
            ellipse(40.0, 20.0, 0.0).vertices().len(),
            MIN_ELLIPSE_SEGMENTS
        );
        assert_eq!(
            ellipse(0.0, 0.0, 0.01).vertices().len(),
            MIN_ELLIPSE_SEGMENTS
        );
        assert_eq!(
            ellipse(12_000.0, 1.0, 1e-9).vertices().len(),
            MAX_ELLIPSE_SEGMENTS
        );
        assert_eq!(
            ellipse(f64::NAN, 1.0, 0.01).vertices().len(),
            MIN_ELLIPSE_SEGMENTS
        );
    }

    #[test]
    fn a_circle_is_two_half_circle_arcs() {
        let outline = circle(20.0);
        assert_eq!(lines_and_arcs(&outline), (0, 2));
        assert_box(&outline, 20.0, 20.0);
        assert_approx_eq!(outline.vertices()[0].x, 10.0, 1e-12);
    }
}
