// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts (findKnotSpan, evaluateSplinePoint, sampledSplinePoints)

//! Rational B-spline evaluation by de Boor's algorithm and sampling to a
//! distance tolerance, for `SPLINE` entities (`docs/specs/phase-2/design.md`,
//! "Algorithms → DXF import", step 2). Upstream samples a fixed eight points
//! per knot span; here each span is bisected until the chords stay within
//! the tolerance, so the result honours the project's flatten tolerance.
//!
//! Indexing: every index below is bounded by the length checks in
//! [`BSpline::new`] (knots ≥ control points + degree + 1, control points >
//! degree), which de Boor's recurrence relies on.
#![allow(
    clippy::indexing_slicing,
    reason = "indices are bounded by the checks in BSpline::new; see the module doc"
)]

use dc_geometry::Point;

/// Deepest bisection of one knot span: 2^12 chords per span. With the
/// coordinate limit and the minimum tolerance, no span needs more.
const MAX_SUBDIVISION_DEPTH: u32 = 12;

/// Spans narrower than this in parameter space are empty (repeated knots).
const KNOT_EPS: f64 = 1e-12;

/// A rational B-spline in homogeneous form: each control point carries its
/// weight, and evaluation divides out the weight at the end.
pub(super) struct BSpline {
    degree: usize,
    knots: Vec<f64>,
    /// `(x · w, y · w, w)` per control point.
    control: Vec<(f64, f64, f64)>,
}

impl BSpline {
    /// A spline of `degree` over `knots` through the weighted `points`.
    /// `None` when the data cannot describe one: degree below 1, fewer
    /// control points than the degree needs, too few knots, a weight that
    /// is not positive, or an empty domain. Missing weights are 1.
    pub(super) fn new(
        degree: usize,
        knots: &[f64],
        points: &[Point],
        weights: &[f64],
    ) -> Option<Self> {
        if degree == 0 || points.len() <= degree || knots.len() < points.len() + degree + 1 {
            return None;
        }
        if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
            return None;
        }
        let n = points.len() - 1;
        if knots[n + 1] - knots[degree] <= KNOT_EPS {
            return None;
        }
        let control = points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let w = weights.get(i).copied().unwrap_or(1.0);
                (w.is_finite() && w > 0.0).then_some((p.x * w, p.y * w, w))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            degree,
            knots: knots.to_vec(),
            control,
        })
    }

    /// The parameter range the curve is defined on.
    fn domain(&self) -> (f64, f64) {
        let n = self.control.len() - 1;
        (self.knots[self.degree], self.knots[n + 1])
    }

    /// Index of the knot span holding `u`, in `degree ..= n`.
    fn knot_span(&self, u: f64) -> usize {
        let n = self.control.len() - 1;
        if u >= self.knots[n + 1] {
            // The last non-empty span owns the domain's end.
            return (self.degree..=n)
                .rev()
                .find(|&i| self.knots[i + 1] - self.knots[i] > KNOT_EPS)
                .unwrap_or(n);
        }
        if u <= self.knots[self.degree] {
            return self.degree;
        }
        let (mut low, mut high) = (self.degree, n + 1);
        let mut mid = (low + high) / 2;
        while u < self.knots[mid] || u >= self.knots[mid + 1] {
            if u < self.knots[mid] {
                high = mid;
            } else {
                low = mid;
            }
            mid = (low + high) / 2;
        }
        mid
    }

    /// The point at parameter `u` (clamped to the domain) by de Boor's
    /// algorithm on the homogeneous control points.
    pub(super) fn point_at(&self, u: f64) -> Point {
        let (start, end) = self.domain();
        let u = u.clamp(start, end);
        let span = self.knot_span(u);
        let p = self.degree;
        let mut d: Vec<(f64, f64, f64)> = (0..=p).map(|j| self.control[span - p + j]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = span - p + j;
                let denominator = self.knots[i + p - r + 1] - self.knots[i];
                let alpha = if denominator.abs() <= KNOT_EPS {
                    0.0
                } else {
                    (u - self.knots[i]) / denominator
                };
                let (ax, ay, aw) = d[j - 1];
                let (bx, by, bw) = d[j];
                d[j] = (
                    (1.0 - alpha) * ax + alpha * bx,
                    (1.0 - alpha) * ay + alpha * by,
                    (1.0 - alpha) * aw + alpha * bw,
                );
            }
        }
        let (x, y, w) = d[p];
        if w.abs() <= KNOT_EPS {
            // Weights are positive and convex combinations stay positive;
            // only rounding could land here, and then the point is lost.
            return Point::new(f64::NAN, f64::NAN);
        }
        Point::new(x / w, y / w)
    }

    /// Points along the curve, from its start to its end, such that no
    /// chord between neighbours strays more than `tolerance_mm` from the
    /// curve (checked at the quarter points of each chord, which the
    /// bisection makes a safe test for a curve this smooth).
    pub(super) fn sample(&self, tolerance_mm: f64) -> Vec<Point> {
        let (start, end) = self.domain();
        let mut points = vec![self.point_at(start)];
        let n = self.control.len() - 1;
        for i in self.degree..=n {
            let (u0, u1) = (self.knots[i], self.knots[i + 1]);
            if u1 - u0 <= KNOT_EPS {
                continue;
            }
            let (u0, u1) = (u0.max(start), u1.min(end));
            let p0 = self.point_at(u0);
            let p1 = self.point_at(u1);
            self.subdivide(u0, u1, p0, p1, tolerance_mm, 0, &mut points);
        }
        points
    }

    /// Appends the points after `p0` up to and including `p1` so the chords
    /// stay within `tolerance_mm`.
    #[allow(
        clippy::too_many_arguments,
        reason = "a recursion carrying its interval, ends and output"
    )]
    fn subdivide(
        &self,
        u0: f64,
        u1: f64,
        p0: Point,
        p1: Point,
        tolerance_mm: f64,
        depth: u32,
        out: &mut Vec<Point>,
    ) {
        let um = 0.5 * (u0 + u1);
        let pm = self.point_at(um);
        let flat = depth >= MAX_SUBDIVISION_DEPTH
            || (depth >= 1
                && [0.25_f64, 0.5, 0.75].iter().all(|&t| {
                    let p = if (t - 0.5).abs() < f64::EPSILON {
                        pm
                    } else {
                        self.point_at(u0 + t * (u1 - u0))
                    };
                    chord_distance(p, p0, p1) <= tolerance_mm
                }));
        if flat {
            out.push(p1);
        } else {
            self.subdivide(u0, um, p0, pm, tolerance_mm, depth + 1, out);
            self.subdivide(um, u1, pm, p1, tolerance_mm, depth + 1, out);
        }
    }
}

/// Distance from `p` to the segment `a`–`b`, mm.
fn chord_distance(p: Point, a: Point, b: Point) -> f64 {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 <= f64::EPSILON {
        return p.distance_to(a);
    }
    let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
    p.distance_to(a + d * t)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;
    use dc_geometry::assert_approx_eq;

    /// Degree 1 through three points is the polyline through them. Each
    /// span is bisected at least once, so the sample also carries the two
    /// midpoints, which lie on the polygon.
    #[test]
    fn degree_one_is_the_control_polygon() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        let spline = BSpline::new(1, &[0.0, 0.0, 1.0, 2.0, 2.0], &points, &[]).unwrap();
        let sampled = spline.sample(0.01);
        assert_eq!(sampled.len(), 5, "{sampled:?}");
        for (got, want) in [sampled[0], sampled[2], sampled[4]].iter().zip(points) {
            assert_approx_eq!(got.x, want.x, 1e-12);
            assert_approx_eq!(got.y, want.y, 1e-12);
        }
        assert_approx_eq!(sampled[1].x, 5.0, 1e-12);
        assert_approx_eq!(sampled[3].y, 5.0, 1e-12);
    }

    /// A clamped cubic with four control points is a Bézier: at u = 0.5 the
    /// point is (P0 + 3 P1 + 3 P2 + P3) / 8.
    #[test]
    fn clamped_cubic_is_a_bezier() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 0.0),
        ];
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let spline = BSpline::new(3, &knots, &points, &[]).unwrap();
        let p = spline.point_at(0.5);
        assert_approx_eq!(p.x, 5.0, 1e-12);
        assert_approx_eq!(p.y, 7.5, 1e-12);
    }

    #[test]
    fn invalid_data_is_none() {
        let points = [Point::new(0.0, 0.0), Point::new(1.0, 0.0)];
        assert!(BSpline::new(0, &[0.0, 0.0, 1.0, 1.0], &points, &[]).is_none());
        assert!(BSpline::new(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &points, &[]).is_none());
        assert!(BSpline::new(1, &[0.0, 0.0, 1.0], &points, &[]).is_none());
        assert!(BSpline::new(1, &[0.0, 0.0, 1.0, 1.0], &points, &[1.0, 0.0]).is_none());
        assert!(BSpline::new(1, &[1.0, 1.0, 1.0, 1.0], &points, &[]).is_none());
        assert!(BSpline::new(1, &[0.0, 0.0, 1.0, 1.0], &points, &[]).is_some());
    }
}
