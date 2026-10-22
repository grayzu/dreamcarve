// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Recovering SVG's circular arcs from usvg's Béziers (#70).
//!
//! usvg hands every `<circle>`, `<ellipse>`, rounded `<rect>` corner and
//! path `A` command over as cubic Béziers, made by kurbo's
//! `Arc::append_iter`: each piece sweeps at most 90° of the arc, its ends
//! lie on the arc, and its handles are tangent to it with length
//! (4/3)·tan(θ/4)·r. The tree keeps nothing else about the shape. Such a
//! cubic is recognised here by rebuilding those control points from its
//! ends and sweep and comparing them with the ones usvg stored, to f32
//! rounding: an exact conversion of the arc the file drew, not a fit.
//! Elliptical arcs, and circles under a transform that is not a
//! similarity, have different control points and are flattened as before.
//!
//! The pieces are joined back into arcs of up to 180°, and a closed
//! subpath that goes once round one circle becomes the two half-circle
//! arcs `Polyline::circle` and a DXF `CIRCLE` make.

use dc_geometry::{Point, Segment};
use kurbo::{Affine, Vec2};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// How closely a cubic's control points must match the circular arc's,
/// relative to the largest coordinate usvg stored for it, and how closely
/// a transform's axes must agree in length and squareness for it to keep
/// circles circular. Dimensionless. usvg stores coordinates and transforms
/// as f32, which rounds by up to 6e-8 of the value; the arc rebuilt from
/// the rounded ends amplifies that a few times. 1e-6 leaves a margin over
/// both and is still far below anything drawn on purpose.
const ARC_REL_TOLERANCE: f64 = 1e-6;

/// Slack on the 90° limit of one Bézier piece and the 180° limit of a
/// joined arc, radians: rounding of the sweep recovered from f32 points.
const SWEEP_EPS_RAD: f64 = 1e-6;

/// Bulges below this are straight: a Bézier with its handles at the
/// thirds of its chord is a line, and is left to the flattener.
const MIN_BULGE: f64 = 1e-9;

/// The uniform scale of `transform` when it maps circles to circles:
/// rotation, uniform scale, reflection and translation. `None` for a
/// non-uniform scale, a skew, or a singular transform.
pub(super) fn similarity_scale(transform: Affine) -> Option<f64> {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let (x_axis, y_axis) = (Vec2::new(a, b), Vec2::new(c, d));
    let size = x_axis.hypot2() + y_axis.hypot2();
    let similar = size > 0.0
        && (x_axis.hypot2() - y_axis.hypot2()).abs() <= ARC_REL_TOLERANCE * size
        && x_axis.dot(y_axis).abs() <= ARC_REL_TOLERANCE * size;
    let scale = (a * d - b * c).abs().sqrt();
    (similar && scale.is_finite() && scale > 0.0).then_some(scale)
}

/// The tolerance, mm, to recognise a cubic whose control points usvg
/// stored with `largest_coordinate` as their largest absolute coordinate,
/// drawn through a similarity of `scale`.
pub(super) fn recognition_tolerance_mm(largest_coordinate: f64, scale: f64) -> f64 {
    ARC_REL_TOLERANCE * largest_coordinate * scale
}

/// The signed sweep, radians, of the circular arc whose kurbo Bézier is the
/// cubic `p0`, `c1`, `c2`, `p3` (mm), or `None` when the cubic is not one,
/// to `tolerance_mm`. Positive is counter-clockwise.
///
/// The sweep θ follows from the handles: each makes θ/2 with the chord.
/// The arc's handles are the chord rotated by ∓θ/2, of length
/// L / (3·cos²(θ/4)) for a chord of length L ((4/3)·tan(θ/4)·r with
/// r = L / (2·sin(θ/2))). The cubic is the arc's when its handles are those
/// to `tolerance_mm`.
pub(super) fn cubic_arc_sweep_rad(
    p0: kurbo::Point,
    c1: kurbo::Point,
    c2: kurbo::Point,
    p3: kurbo::Point,
    tolerance_mm: f64,
) -> Option<f64> {
    let chord = p3 - p0;
    let (start_handle, end_handle) = (c1 - p0, p3 - c2);
    let chord_mm = chord.hypot();
    if !(chord_mm > 4.0 * tolerance_mm
        && start_handle.hypot() > tolerance_mm
        && end_handle.hypot() > tolerance_mm)
    {
        return None;
    }
    let turn = |from: Vec2, to: Vec2| from.cross(to).atan2(from.dot(to));
    let sweep_rad = turn(start_handle, chord) + turn(chord, end_handle);
    if sweep_rad.abs() > FRAC_PI_2 + SWEEP_EPS_RAD || (sweep_rad / 4.0).tan().abs() < MIN_BULGE {
        return None;
    }
    let handle_mm = chord_mm / (3.0 * (sweep_rad / 4.0).cos().powi(2));
    let along = chord * (handle_mm / chord_mm);
    let rotated = |v: Vec2, angle_rad: f64| v.rotate_scale(Vec2::from_angle(angle_rad));
    let expected_c1 = p0 + rotated(along, -sweep_rad / 2.0);
    let expected_c2 = p3 - rotated(along, sweep_rad / 2.0);
    (c1.distance(expected_c1) <= tolerance_mm && c2.distance(expected_c2) <= tolerance_mm)
        .then_some(sweep_rad)
}

/// One circular arc of a profile, mm, before it becomes a bulge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ArcPiece {
    pub start: Point,
    pub end: Point,
    /// Signed sweep; positive is counter-clockwise.
    pub sweep_rad: f64,
    /// How far, mm, the piece's points may be from where they would be
    /// without usvg's f32 rounding.
    pub tolerance_mm: f64,
}

impl ArcPiece {
    /// The bulge of the piece, tan(θ/4).
    pub fn bulge(&self) -> f64 {
        (self.sweep_rad / 4.0).tan()
    }

    /// Centre and radius, mm; `None` for a chord too short to place them.
    fn circle(&self) -> Option<(Point, f64)> {
        let arc = Segment {
            start: self.start,
            end: self.end,
            bulge: self.bulge(),
        }
        .arc()?;
        Some((arc.centre, arc.radius_mm))
    }

    /// Whether the piece is the arc of the circle about `centre` of
    /// `radius_mm` between its ends, in its direction, to `tolerance_mm`:
    /// both ends on the circle, and its sagitta that of the circle's arc
    /// over the same chord.
    fn lies_on(&self, centre: Point, radius_mm: f64, tolerance_mm: f64) -> bool {
        let (from, to) = (self.start - centre, self.end - centre);
        let on_circle = |d: f64| (d - radius_mm).abs() <= tolerance_mm;
        if !(on_circle(from.length()) && on_circle(to.length())) {
            return false;
        }
        let mut circle_sweep_rad = from.cross(to).atan2(from.dot(to));
        if self.sweep_rad > 0.0 && circle_sweep_rad < 0.0 {
            circle_sweep_rad += TAU;
        } else if self.sweep_rad < 0.0 && circle_sweep_rad > 0.0 {
            circle_sweep_rad -= TAU;
        }
        let half_chord_mm = self.start.distance_to(self.end) / 2.0;
        let sagitta_gap_mm = half_chord_mm * ((circle_sweep_rad / 4.0).tan() - self.bulge()).abs();
        sagitta_gap_mm <= tolerance_mm
    }

    /// `self` followed by `next` as one arc, when `next` starts where
    /// `self` ends, both are arcs of one circle in one direction, and
    /// together they sweep at most 180°.
    pub fn joined(&self, next: &ArcPiece) -> Option<ArcPiece> {
        let sweep_rad = self.sweep_rad + next.sweep_rad;
        if self.sweep_rad * next.sweep_rad <= 0.0 || sweep_rad.abs() > PI + SWEEP_EPS_RAD {
            return None;
        }
        let joined = ArcPiece {
            start: self.start,
            end: next.end,
            sweep_rad,
            tolerance_mm: self.tolerance_mm.max(next.tolerance_mm),
        };
        let (centre, radius_mm) = joined.circle()?;
        let tolerance_mm = joined.tolerance_mm;
        (self.lies_on(centre, radius_mm, tolerance_mm)
            && next.lies_on(centre, radius_mm, tolerance_mm))
        .then_some(joined)
    }
}

/// When `pieces`, the segments of a closed profile in order, go once round
/// one circle in one direction: its centre and the direction, +1 for
/// counter-clockwise or −1.
pub(super) fn full_circle(pieces: &[ArcPiece]) -> Option<(Point, f64)> {
    let largest = pieces
        .iter()
        .max_by(|a, b| a.sweep_rad.abs().total_cmp(&b.sweep_rad.abs()))?;
    let direction = largest.sweep_rad.signum();
    let total_rad: f64 = pieces.iter().map(|p| p.sweep_rad).sum();
    let tolerance_mm = pieces.iter().map(|p| p.tolerance_mm).fold(0.0, f64::max);
    let (centre, radius_mm) = largest.circle()?;
    let (first, last) = (pieces.first()?, pieces.last()?);
    // Closed, so the sweeps add up to a whole number of turns: one.
    let once_round = pieces.len() >= 2
        && last.end.distance_to(first.start) <= tolerance_mm
        && pieces.iter().all(|p| p.sweep_rad * direction > 0.0)
        && (total_rad - direction * TAU).abs() < PI
        && pieces
            .iter()
            .all(|p| p.lies_on(centre, radius_mm, tolerance_mm));
    once_round.then_some((centre, direction))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

    use super::*;
    use dc_geometry::assert_approx_eq;
    use kurbo::{ParamCurve, SvgArc};

    const TOL_MM: f64 = 1e-6;

    /// kurbo's Bézier pieces of the arc, as usvg makes them.
    fn pieces(arc: &kurbo::Arc) -> Vec<(kurbo::Point, kurbo::Point, kurbo::Point, kurbo::Point)> {
        let mut out = Vec::new();
        let mut current = arc.eval(0.0);
        arc.to_cubic_beziers(0.1, |c1, c2, p| {
            out.push((current, c1, c2, p));
            current = p;
        });
        out
    }

    fn arc(centre: (f64, f64), radius: f64, start_deg: f64, sweep_deg: f64) -> kurbo::Arc {
        kurbo::Arc::new(
            centre,
            (radius, radius),
            start_deg.to_radians(),
            sweep_deg.to_radians(),
            0.0,
        )
    }

    /// Every piece kurbo makes of a circular arc is recognised, with the
    /// sweep it was made with, in either direction.
    #[test]
    fn kurbo_arc_pieces_are_recognised_with_their_sweep() {
        for sweep_deg in [90.0, -90.0, 30.0, -200.0, 359.0] {
            let a = arc((10.0, -20.0), 45.0, 17.0, sweep_deg);
            let ps = pieces(&a);
            let each = sweep_deg.to_radians() / ps.len() as f64;
            for (i, (p0, c1, c2, p3)) in ps.into_iter().enumerate() {
                let got = cubic_arc_sweep_rad(p0, c1, c2, p3, TOL_MM)
                    .unwrap_or_else(|| panic!("{sweep_deg}°: piece {i} not recognised"));
                assert_approx_eq!(got, each, 1e-9);
            }
        }
    }

    /// An elliptical arc's pieces and a free cubic are not circular arcs.
    #[test]
    fn elliptical_and_free_cubics_are_not_recognised() {
        let ellipse = kurbo::Arc::new((0.0, 0.0), (40.0, 20.0), 0.0, FRAC_PI_2, 0.0);
        for (p0, c1, c2, p3) in pieces(&ellipse) {
            assert_eq!(cubic_arc_sweep_rad(p0, c1, c2, p3, TOL_MM), None);
        }
        let free =
            [(95.0, 88.0), (105.0, 65.0), (120.0, 100.0), (130.0, 75.0)].map(kurbo::Point::from);
        assert_eq!(
            cubic_arc_sweep_rad(free[0], free[1], free[2], free[3], TOL_MM),
            None
        );
        // A straight cubic is a line, left to the flattener.
        let straight = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)].map(kurbo::Point::from);
        assert_eq!(
            cubic_arc_sweep_rad(straight[0], straight[1], straight[2], straight[3], TOL_MM),
            None
        );
    }

    /// One cubic over a half circle is a poor approximation of it (1.8 %
    /// of r), so it is not taken for one: pieces sweep at most 90°.
    #[test]
    fn cubic_over_more_than_90_degrees_is_not_recognised() {
        let p0 = kurbo::Point::new(10.0, 0.0);
        let p3 = kurbo::Point::new(-10.0, 0.0);
        let h = 20.0 / 3.0 / (std::f64::consts::FRAC_PI_4).cos().powi(2);
        let (c1, c2) = (p0 + Vec2::new(0.0, h), p3 + Vec2::new(0.0, h));
        assert_eq!(cubic_arc_sweep_rad(p0, c1, c2, p3, TOL_MM), None);
    }

    fn piece(a: &kurbo::Arc) -> ArcPiece {
        let to = |p: kurbo::Point| Point::new(p.x, p.y);
        ArcPiece {
            start: to(a.eval(0.0)),
            end: to(a.eval(1.0)),
            sweep_rad: a.sweep_angle,
            tolerance_mm: TOL_MM,
        }
    }

    #[test]
    fn quarters_of_one_circle_join_up_to_a_half() {
        let q1 = piece(&arc((5.0, 5.0), 3.0, 0.0, 90.0));
        let q2 = piece(&arc((5.0, 5.0), 3.0, 90.0, 90.0));
        let q3 = piece(&arc((5.0, 5.0), 3.0, 180.0, 90.0));
        let half = q1.joined(&q2).expect("two quarters join");
        assert_approx_eq!(half.sweep_rad, PI, 1e-12);
        assert_approx_eq!(half.bulge(), 1.0, 1e-12);
        assert_eq!(half.joined(&q3), None, "past 180°");
    }

    #[test]
    fn arcs_of_different_circles_or_directions_do_not_join() {
        let q1 = piece(&arc((5.0, 5.0), 3.0, 0.0, 90.0));
        // Same ends, sweeping back: opposite direction.
        let back = piece(&arc((5.0, 5.0), 3.0, 90.0, -90.0));
        assert_eq!(q1.joined(&back), None);
        // Starts where q1 ends but is a tighter circle.
        let tighter = piece(&arc((5.0, 7.0), 1.0, 90.0, 90.0));
        assert_eq!(q1.joined(&tighter), None);
    }

    #[test]
    fn closed_run_of_one_circle_is_a_full_circle() {
        let quarters: Vec<ArcPiece> = (0..4)
            .map(|i| piece(&arc((5.0, 5.0), 3.0, -90.0 * f64::from(i), -90.0)))
            .collect();
        let (centre, direction) = full_circle(&quarters).expect("once round");
        assert_approx_eq!(centre.x, 5.0, 1e-9);
        assert_approx_eq!(centre.y, 5.0, 1e-9);
        assert_approx_eq!(direction, -1.0, 0.0);
        assert_eq!(full_circle(&quarters[..3]), None, "three quarters");
    }

    #[test]
    fn similarity_scale_accepts_rotation_reflection_and_uniform_scale() {
        let rotated = Affine::rotate(0.5) * Affine::scale(2.0);
        assert_approx_eq!(similarity_scale(rotated).expect("similar"), 2.0, 1e-12);
        let mirrored = Affine::new([-3.0, 0.0, 0.0, 3.0, 7.0, 8.0]);
        assert_approx_eq!(similarity_scale(mirrored).expect("similar"), 3.0, 1e-12);
        assert_eq!(similarity_scale(Affine::scale_non_uniform(2.0, 1.0)), None);
        assert_eq!(similarity_scale(Affine::skew(0.2, 0.0)), None);
        assert_eq!(similarity_scale(Affine::scale(0.0)), None);
    }

    #[test]
    fn svg_arc_conversion_round_trips() {
        // The path `A` command's route through kurbo: SvgArc → Arc → cubics.
        let svg = SvgArc {
            from: (10.0, 50.0).into(),
            to: (90.0, 50.0).into(),
            radii: Vec2::new(40.0, 40.0),
            x_rotation: 0.0,
            large_arc: false,
            sweep: true,
        };
        let a = kurbo::Arc::from_svg_arc(&svg).expect("an arc");
        let total: f64 = pieces(&a)
            .into_iter()
            .map(|(p0, c1, c2, p3)| cubic_arc_sweep_rad(p0, c1, c2, p3, TOL_MM).expect("an arc"))
            .sum();
        assert_approx_eq!(total, PI, 1e-9);
    }
}
