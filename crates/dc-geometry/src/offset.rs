// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Parallel offset of contours with round joins, via cavalier_contours in
//! f64 with bulge arcs (`docs/specs/phase-1/design.md`, "Data model →
//! dc-geometry"). Replaces PureCutCNC's Clipper offset; its integer scaling
//! has no counterpart here.

use crate::contour::Contour;
use crate::point::{EPS_MM, Point};
use crate::polyline::{Polyline, Segment, Vertex};
use cavalier_contours::polyline::{
    PlineCreation, PlineOffsetOptions, PlineSegIntr, PlineSource, PlineSourceMut, PlineVertex,
    pline_seg_intr,
};

pub(crate) type CavPolyline = cavalier_contours::polyline::Polyline<f64>;

/// Offsets `contour` by `delta_mm`. Positive `delta_mm` moves the contour
/// away from the area it encloses (grows it); negative moves it inward.
/// The rule is the same for a clockwise hole contour, so a hole offset by a
/// negative delta gets smaller. Results keep the input's orientation, and
/// corners become arcs (round joins). An inward offset can split a contour
/// into several or consume it: an empty result means nothing is left.
///
/// #465: `cavalier_contours` can return an offset as open pieces, failing
/// to close its loop: where a traced convex curve (its points a little off
/// any one circle) is offset inward by about its radius, so the curve's
/// offset collapses to about a point, and the slices there fall foul of
/// its distance check at the default tolerances. An open piece is no
/// contour, and dropped it took the whole loop with it (a male Inlay's
/// hole lost its floor). Such an offset is taken again with finer
/// tolerances (1e-6, then 1e-7, then 1e-8 mm), and the first that closes
/// every piece is kept.
#[must_use]
pub fn offset(contour: &Contour, delta_mm: f64) -> Vec<Contour> {
    let ccw = contour.is_ccw();
    let source = to_cavalier(contour.polyline());
    // cavalier_contours offsets to the left of travel for a positive value:
    // inward on a CCW contour, outward on a CW one.
    let cav_delta = if ccw { -delta_mm } else { delta_mm };
    let open = |pieces: &[CavPolyline]| pieces.iter().any(|p| !p.is_closed());
    let mut pieces = source.parallel_offset(cav_delta);
    if open(&pieces) {
        let closed = RETRY_EPS_MM.iter().find_map(|&eps| {
            let options = PlineOffsetOptions {
                pos_equal_eps: eps,
                offset_dist_eps: eps,
                ..PlineOffsetOptions::new()
            };
            let again = source.parallel_offset_opt(cav_delta, &options);
            (!open(&again)).then_some(again)
        });
        if let Some(closed) = closed {
            pieces = closed;
        }
    }
    pieces
        .iter()
        .filter_map(|p| Contour::oriented(from_cavalier(p)).ok())
        .map(|c| if ccw { c } else { c.reversed() })
        .collect()
}

/// The position and distance tolerances, mm, [`offset`] retries an offset
/// with when `cavalier_contours` leaves it open at its defaults (1e-5 and
/// 1e-4), finest last (#465).
const RETRY_EPS_MM: [f64; 3] = [1e-6, 1e-7, 1e-8];

/// Offsets the open path `path` sideways by `left_mm` (#207): positive
/// moves it to the left of travel from its first vertex to its last, which
/// in Y-up (ADR-0001) is the normal `(−dy, dx)` of each segment's
/// direction `(dx, dy)`; negative moves it to the right. Each piece runs
/// the same way as `path` and ends square, on the normals through the
/// path's end vertices. A turn away from the offset side becomes an arc
/// about the vertex (a round join); a turn toward it is trimmed where the
/// two offset edges cross; an arc whose radius is `|left_mm|` or less on
/// the side its centre lies on collapses. What is left is every stretch at
/// least `|left_mm|` from all of `path`, so the result may be several
/// pieces or none. For an open `path`; a closed contour is [`offset`]'s.
#[must_use]
pub fn offset_open(path: &Polyline, left_mm: f64) -> Vec<Polyline> {
    let source = to_cavalier(path);
    // As for a contour: cavalier_contours offsets to the left of travel for
    // a positive value (pinned by
    // `open_offset_moves_left_of_travel_and_ends_square`).
    source
        .parallel_offset(left_mm)
        .iter()
        .map(from_cavalier)
        .filter(|p| p.vertices().len() >= 2 && p.length_mm() > EPS_MM)
        .collect()
}

/// The two ends of the strip `|left_mm|` wide beside the open path
/// `path` on the side of `left_mm` (#342; positive is the left of travel
/// from its first vertex to its last, as for [`offset_open`]): for its
/// first vertex, then its last, the line from the vertex along the normal
/// to that side, `|left_mm|` long, cut short where it first meets one of
/// `lines` (crossing or touching it) farther than [`EPS_MM`] from the
/// vertex. With `path` and its [`offset_open`] by `left_mm` they bound
/// the strip; a strip bounded only by those two is open at each end, so
/// a disc can reach into it past an end without coming near either.
/// Cutting each end short at the first line it meets keeps it to the
/// strip where the end segment is shorter than `|left_mm|` and another
/// segment runs across its normal. Empty for a closed path, one of no
/// length, or a `left_mm` no wider than [`EPS_MM`].
#[must_use]
pub fn strip_ends(path: &Polyline, left_mm: f64, lines: &[&Polyline]) -> Vec<Polyline> {
    let length_mm = path.length_mm();
    if path.is_closed() || length_mm <= EPS_MM || left_mm.is_nan() || left_mm.abs() <= EPS_MM {
        return Vec::new();
    }
    [0.0, length_mm]
        .into_iter()
        .filter_map(|at_mm| {
            let (vertex, tangent) = path.point_at(at_mm)?;
            let normal = tangent.perp() * left_mm.signum();
            let far = vertex + normal * left_mm.abs();
            let end = Segment {
                start: vertex,
                end: far,
                bulge: 0.0,
            };
            let reach_mm = lines
                .iter()
                .flat_map(|line| line.segments())
                .flat_map(|s| crossings(end, s))
                .map(|p| p.distance_to(vertex))
                .filter(|&d| d > EPS_MM)
                .fold(left_mm.abs(), f64::min);
            let tip = vertex + normal * reach_mm;
            Some(Polyline::new(
                vec![Vertex::line(vertex.x, vertex.y), Vertex::line(tip.x, tip.y)],
                false,
            ))
        })
        .collect()
}

/// Where segments `s` and `t` cross or touch, within [`EPS_MM`]: both
/// ends of a stretch where they overlap.
fn crossings(s: Segment, t: Segment) -> Vec<Point> {
    let vertex = |p: Point, bulge: f64| PlineVertex::new(p.x, p.y, bulge);
    if s.start.distance_to(s.end) <= EPS_MM || t.start.distance_to(t.end) <= EPS_MM {
        return Vec::new();
    }
    match pline_seg_intr(
        vertex(s.start, s.bulge),
        vertex(s.end, 0.0),
        vertex(t.start, t.bulge),
        vertex(t.end, 0.0),
        EPS_MM,
    ) {
        PlineSegIntr::NoIntersect => Vec::new(),
        PlineSegIntr::TangentIntersect { point: p } | PlineSegIntr::OneIntersect { point: p } => {
            vec![Point::new(p.x, p.y)]
        }
        PlineSegIntr::TwoIntersects { point1, point2 }
        | PlineSegIntr::OverlappingLines { point1, point2 }
        | PlineSegIntr::OverlappingArcs { point1, point2 } => {
            vec![
                Point::new(point1.x, point1.y),
                Point::new(point2.x, point2.y),
            ]
        }
    }
}

/// Whether every point inside `contour` lies within `inset_mm` of it: the
/// contour offset inward by `inset_mm` is empty (#223, a part a bit of
/// radius `inset_mm` cut on its line removes whole).
///
/// The points inside a closed line of length `L` and within `t` of it
/// cover at most `L·t`: each inner parallel curve is no longer than the
/// line itself. So an area above `L · inset_mm` always keeps a point
/// farther in, and only a smaller one is offset to find out, which keeps a
/// large outline of many chords from being offset at all.
#[must_use]
pub fn inset_is_empty(contour: &Contour, inset_mm: f64) -> bool {
    let area_mm2 = contour.signed_area_mm2().abs();
    if inset_mm.is_nan() || inset_mm <= 0.0 || area_mm2 > contour.polyline().length_mm() * inset_mm
    {
        return false;
    }
    offset(contour, -inset_mm).is_empty()
}

pub(crate) fn to_cavalier(polyline: &Polyline) -> CavPolyline {
    let mut out = CavPolyline::with_capacity(polyline.vertices().len(), polyline.is_closed());
    for v in polyline.vertices() {
        out.add(v.x, v.y, v.bulge);
    }
    out
}

fn from_cavalier(polyline: &CavPolyline) -> Polyline {
    let vertices = polyline
        .iter_vertexes()
        .map(|v| Vertex::new(v.x, v.y, v.bulge))
        .collect();
    Polyline::new(vertices, polyline.is_closed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;
    use crate::point::Point;
    use crate::polyline::Polyline;

    const TOL: f64 = 1e-6;
    const TOOL_RADIUS_MM: f64 = 3.175;

    fn rect_100x50() -> Contour {
        Contour::new(Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0)).unwrap()
    }

    #[test]
    fn outside_rect_grows_bounds_by_diameter() {
        let out = offset(&rect_100x50(), TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 106.35, TOL);
        assert_approx_eq!(b.height_mm(), 56.35, TOL);
        assert_approx_eq!(b.min.x, -TOOL_RADIUS_MM, TOL);
        assert_approx_eq!(b.min.y, -TOOL_RADIUS_MM, TOL);
    }

    #[test]
    fn inside_rect_shrinks_bounds_by_diameter() {
        let out = offset(&rect_100x50(), -TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 93.65, TOL);
        assert_approx_eq!(b.height_mm(), 43.65, TOL);
    }

    #[test]
    fn inside_offset_larger_than_shape_vanishes() {
        assert!(offset(&rect_100x50(), -30.0).is_empty());
    }

    /// A circle of radius 2.5 and a 3 mm square lie within 3.175 mm of
    /// their lines everywhere; a 3.3 mm circle and a 7 mm square do not,
    /// nor does a large circle of many chords, which the area bound settles
    /// without an offset.
    #[test]
    fn inset_is_empty_when_the_part_lies_within_the_inset() {
        let contour = |p: Polyline| Contour::new(p).unwrap();
        let at = Point::new(0.0, 0.0);
        assert!(inset_is_empty(
            &contour(Polyline::circle(at, 2.5)),
            TOOL_RADIUS_MM
        ));
        assert!(inset_is_empty(
            &contour(Polyline::rect(at, 3.0, 3.0)),
            TOOL_RADIUS_MM
        ));
        assert!(inset_is_empty(
            &contour(Polyline::rect(at, 50.0, 3.0)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(
            &contour(Polyline::circle(at, 3.3)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(
            &contour(Polyline::rect(at, 7.0, 7.0)),
            TOOL_RADIUS_MM
        ));
        let chords = (0..2000)
            .map(|k| {
                let a = f64::from(k) * std::f64::consts::TAU / 2000.0;
                Vertex::line(14.0 * a.cos(), 14.0 * a.sin())
            })
            .collect();
        assert!(!inset_is_empty(
            &contour(Polyline::new(chords, true)),
            TOOL_RADIUS_MM
        ));
        assert!(!inset_is_empty(&contour(Polyline::circle(at, 2.5)), 0.0));
    }

    #[test]
    fn round_joins_produce_arc_vertices() {
        let out = offset(&rect_100x50(), TOOL_RADIUS_MM);
        let arcs = out[0]
            .polyline()
            .segments()
            .filter(|s| !s.is_line())
            .count();
        assert_eq!(arcs, 4);
    }

    #[test]
    fn offset_keeps_ccw_orientation() {
        for delta in [TOOL_RADIUS_MM, -TOOL_RADIUS_MM] {
            assert!(offset(&rect_100x50(), delta).iter().all(Contour::is_ccw));
        }
    }

    #[test]
    fn negative_offset_of_cw_hole_shrinks_it_and_stays_cw() {
        let hole = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 15.0))
            .unwrap()
            .reversed();
        let out = offset(&hole, -TOOL_RADIUS_MM);
        assert_eq!(out.len(), 1);
        assert!(!out[0].is_ccw());
        let b = out[0].polyline().bounds().unwrap();
        assert_approx_eq!(b.width_mm(), 30.0 - 6.35, TOL);
    }

    fn open(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    fn ends(p: &Polyline) -> (Point, Point) {
        let vs = p.vertices();
        (vs[0].point(), vs[vs.len() - 1].point())
    }

    /// #207: a positive `left_mm` moves an open path to the left of
    /// travel from its first vertex to its last, which in Y-up is the
    /// normal `(−dy, dx)`; a negative one to the right. The result runs
    /// the same way as the path and ends square, on the normals through
    /// the path's end vertices.
    #[test]
    fn open_offset_moves_left_of_travel_and_ends_square() {
        let line = open(&[(0.0, 0.0), (100.0, 0.0)]);
        for (left_mm, y) in [
            (TOOL_RADIUS_MM, TOOL_RADIUS_MM),
            (-TOOL_RADIUS_MM, -TOOL_RADIUS_MM),
        ] {
            let out = offset_open(&line, left_mm);
            assert_eq!(out.len(), 1, "{left_mm}: {out:?}");
            assert!(!out[0].is_closed());
            let (start, end) = ends(&out[0]);
            assert!(start.distance_to(Point::new(0.0, y)) <= TOL, "{start:?}");
            assert!(end.distance_to(Point::new(100.0, y)) <= TOL, "{end:?}");
        }
        // Drawn the other way, left is the other side.
        let back = offset_open(&line.reversed(), TOOL_RADIUS_MM);
        let (start, end) = ends(&back[0]);
        assert!(start.distance_to(Point::new(100.0, -TOOL_RADIUS_MM)) <= TOL);
        assert!(end.distance_to(Point::new(0.0, -TOOL_RADIUS_MM)) <= TOL);
        // Upward, left is −X.
        let up = offset_open(&open(&[(0.0, 0.0), (0.0, 50.0)]), TOOL_RADIUS_MM);
        let (start, _) = ends(&up[0]);
        assert!(start.distance_to(Point::new(-TOOL_RADIUS_MM, 0.0)) <= TOL);
    }

    /// #207: on the outside of a turn the offset rounds the corner with
    /// an arc about the vertex, as a closed offset does; on the inside the
    /// two offset edges are trimmed where they cross. Either way every
    /// point of the result keeps the distance from the path.
    #[test]
    fn open_offset_rounds_outer_turns_and_trims_inner_ones() {
        // Turns left at (50, 0): left is the inside of the turn.
        let l_path = open(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]);
        let inner = offset_open(&l_path, TOOL_RADIUS_MM);
        assert_eq!(inner.len(), 1, "{inner:?}");
        let corner = Point::new(50.0 - TOOL_RADIUS_MM, TOOL_RADIUS_MM);
        assert!(
            inner[0]
                .vertices()
                .iter()
                .any(|v| v.point().distance_to(corner) <= TOL),
            "trimmed at {corner:?}: {inner:?}"
        );
        assert!(inner[0].segments().all(|s| s.is_line()));
        let outer = offset_open(&l_path, -TOOL_RADIUS_MM);
        assert_eq!(outer.len(), 1, "{outer:?}");
        assert_eq!(outer[0].segments().filter(|s| !s.is_line()).count(), 1);
        let (start, end) = ends(&outer[0]);
        assert!(start.distance_to(Point::new(0.0, -TOOL_RADIUS_MM)) <= TOL);
        assert!(end.distance_to(Point::new(50.0 + TOOL_RADIUS_MM, 50.0)) <= TOL);
        for piece in inner.iter().chain(&outer) {
            for s in piece.segments() {
                for k in 0..=8 {
                    let (p, _) = s.point_at(s.length_mm() * f64::from(k) / 8.0).unwrap();
                    let d = l_path
                        .segments()
                        .map(|t| crate::point_segment_distance_mm(t, p))
                        .fold(f64::INFINITY, f64::min);
                    assert!(d >= TOOL_RADIUS_MM - TOL, "{p:?} is {d} from the path");
                }
            }
        }
    }

    /// #207: an arc whose radius is less than the offset on the side its
    /// centre lies on collapses: nothing of the path is left to cut.
    #[test]
    fn open_offset_of_a_tight_arc_toward_its_centre_vanishes() {
        // A half circle of radius 2 from (2, 0) to (−2, 0) counter-clockwise
        // round the origin: its centre is on its left.
        let arc = Polyline::new(
            vec![Vertex::new(2.0, 0.0, 1.0), Vertex::line(-2.0, 0.0)],
            false,
        );
        assert!(offset_open(&arc, TOOL_RADIUS_MM).is_empty());
        let grown = offset_open(&arc, -TOOL_RADIUS_MM);
        assert_eq!(grown.len(), 1);
        assert_approx_eq!(
            grown[0].length_mm(),
            std::f64::consts::PI * (2.0 + TOOL_RADIUS_MM),
            1e-6
        );
    }

    /// #342: the ends of the strip an open path keeps run from its first
    /// and last vertices along their normals to the strip's side, the
    /// strip's width long, and stop where they first meet a line. A 100 mm
    /// line's strip on its right ends at X 0 and X 100, `r` down. A 30° V
    /// drawn down a 20 mm arm and up a 2 mm one keeps its inside when cut
    /// round it: the end at the 2 mm arm's tip meets the first arm
    /// `2·tan 30°` from the tip, so stops there; the end at the first
    /// vertex is `r` long. Closed paths and zero widths have none.
    #[test]
    fn strip_ends_run_along_the_end_normals_to_the_first_line() {
        let line = open(&[(0.0, 0.0), (100.0, 0.0)]);
        let ends = strip_ends(&line, -TOOL_RADIUS_MM, &[&line]);
        assert_eq!(ends.len(), 2, "{ends:?}");
        for (end, x) in ends.iter().zip([0.0, 100.0]) {
            let (from, to) = self::ends(end);
            assert!(from.distance_to(Point::new(x, 0.0)) <= TOL, "{end:?}");
            assert!(
                to.distance_to(Point::new(x, -TOOL_RADIUS_MM)) <= TOL,
                "{end:?}"
            );
        }
        let (sin, cos) = 15f64.to_radians().sin_cos();
        let v = open(&[
            (-20.0 * sin, 20.0 * cos),
            (0.0, 0.0),
            (2.0 * sin, 2.0 * cos),
        ]);
        let ends = strip_ends(&v, TOOL_RADIUS_MM, &[&v]);
        assert_eq!(ends.len(), 2, "{ends:?}");
        assert_approx_eq!(ends[0].length_mm(), TOOL_RADIUS_MM, TOL);
        assert_approx_eq!(ends[1].length_mm(), 2.0 * 30f64.to_radians().tan(), TOL);
        let (from, _) = self::ends(&ends[1]);
        assert!(from.distance_to(Point::new(2.0 * sin, 2.0 * cos)) <= TOL);
        assert!(strip_ends(&Polyline::rect(Point::new(0.0, 0.0), 5.0, 5.0), 1.0, &[]).is_empty());
        assert!(strip_ends(&line, 0.0, &[]).is_empty());
    }

    #[test]
    fn circle_offset_stays_a_circle() {
        let c = Contour::new(Polyline::circle(Point::new(0.0, 0.0), 45.0)).unwrap();
        let out = offset(&c, TOOL_RADIUS_MM);
        let r = 45.0 + TOOL_RADIUS_MM;
        assert_approx_eq!(out[0].signed_area_mm2(), std::f64::consts::PI * r * r, 1e-6);
    }

    /// #384: a Pocket's swept toolpath loop (`dc_cam::footprint::band`),
    /// grown by the tool radius. Its raw offset has sub-micron segments
    /// round the concave chords, and cavalier_contours 0.9.0 gives one
    /// dissected slice an end point on the segment after the one it names,
    /// 4.2 µm off it, which its debug build asserts on
    /// (`EndPointNotOnSegment`). The offset is right all the same: every
    /// point of a 10 µm grid is in it exactly when it is in the loop or
    /// within the radius of it, bar points within 1 µm of that boundary.
    #[test]
    fn offset_of_a_loop_with_sub_micron_raw_offset_segments_is_the_buffer() {
        let points = [
            (36.730_799_198_150_635, 47.185_787_320_137_024),
            (36.578_314_304_351_81, 47.337_913_334_369_66),
            (36.487_584_143_877_03, 47.536_300_778_388_98),
            (36.472_042_560_577_39, 47.753_896_743_059_16),
            (36.533_656_269_311_905, 47.963_165_193_796_16),
            (36.641_338_974_237_44, 48.106_601_804_494_86),
            (36.121_772_110_462_19, 47.880_961_924_791_336),
            (35.390_199_989_080_43, 47.476_627_320_051_19),
            (34.713_660_866_022_11, 47.013_674_765_825_27),
            (34.884_462_267_160_416, 47.041_286_617_517_47),
            (35.094_031_214_714_05, 47.001_096_695_661_545),
            (35.277_141_183_614_73, 46.891_528_844_833_374),
            (35.411_620_944_738_39, 46.725_849_747_657_776),
            (35.481_187_492_609_024, 46.524_120_032_787_32),
            (35.478_218_138_217_926, 46.356_073_677_539_825),
            (35.627_252_638_339_996, 46.603_415_012_359_62),
            (35.793_080_419_301_99, 46.819_530_189_037_32),
            (36.002_840_667_963_03, 46.993_326_753_377_914),
            (36.246_015_191_078_186, 47.116_089_791_059_494),
            (36.510_410_159_826_28, 47.181_663_513_183_594),
        ];
        let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        let contour = Contour::new(Polyline::new(vertices, true)).unwrap();
        let radius_mm = 0.603_240_318_194_811_6;
        let out = offset(&contour, radius_mm);
        assert_eq!(out.len(), 1, "{out:?}");
        let (grown, _) = crate::nest(out).unwrap();
        let (source, _) = crate::nest(vec![contour.clone()]).unwrap();
        let edges: Vec<Segment> = contour.polyline().segments().collect();
        let distance_mm = |p: Point| {
            edges
                .iter()
                .map(|e| {
                    let along = e.end - e.start;
                    let t = ((p - e.start).dot(along) / along.dot(along)).clamp(0.0, 1.0);
                    p.distance_to(e.start + along * t)
                })
                .fold(f64::INFINITY, f64::min)
        };
        let step_mm = 0.01;
        for i in 0..450 {
            for j in 0..450 {
                let p = Point::new(33.5 + f64::from(i) * step_mm, 45.0 + f64::from(j) * step_mm);
                let d = distance_mm(p);
                let inside_source = source[0].contains_point(p);
                if !inside_source && (d - radius_mm).abs() <= 1e-3 {
                    continue;
                }
                let expected = inside_source || d < radius_mm;
                assert_eq!(grown[0].contains_point(p), expected, "{p:?} at {d} mm");
            }
        }
    }
}
