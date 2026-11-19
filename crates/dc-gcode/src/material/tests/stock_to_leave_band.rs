// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the stock to leave band: the strip beside a side Profile's
//! wall that its passes before the last may cut in, at corners and at
//! the open ends of a path.

use super::*;
use dc_geometry::Vertex;

const RADIUS_MM: f64 = 1.0;
const STOCK_TO_LEAVE_MM: f64 = 0.3;
const SLACK_MM: f64 = 0.012;

fn line_path(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        false,
    )
}

/// The stock to leave band of a Left Profile of `path` with a last pass.
fn band(path: &Polyline) -> StockToLeaveBand {
    match side_profile(
        std::slice::from_ref(path),
        RADIUS_MM,
        Some(STOCK_TO_LEAVE_MM),
    ) {
        Material::SideProfile {
            steps: Some(band), ..
        } => band,
        other => panic!("not a Left Profile with a last pass: {other:?}"),
    }
}

/// The first point of `path` offset `left_mm`, which must be one piece.
fn offset_start(path: &Polyline, left_mm: f64) -> Point {
    let pieces = offset_open(path, left_mm);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    let v = pieces[0].vertices()[0];
    Point::new(v.x, v.y)
}

fn level(start: Point, end: Point, bulge: f64) -> Segment {
    Segment { start, end, bulge }
}

/// #358 case 1: an open path that nearly closes, cut Left (inside the
/// loop), its end vertex E 1.5 mm above the line its first segment
/// runs along. Both offsets start where they come `r` and `r + a`
/// from E, so the two passes' starts are not on one normal: the last
/// pass's step from the stock path's start onto its own ends
/// 0.52 mm from the stock path, farther than the stock to leave. It
/// still runs between the first segment's own two offsets, at least
/// `r` from every line, and is a step S9 accepts.
#[test]
fn last_pass_step_between_starts_trimmed_by_the_end_vertex_is_allowed() {
    let path = line_path(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 5.0),
        (-0.5, 5.0),
        (-0.5, 1.5),
    ]);
    let far = offset_start(&path, RADIUS_MM + STOCK_TO_LEAVE_MM);
    let near = offset_start(&path, RADIUS_MM);
    // Each pass starts on its circle round E, not on the first
    // segment's normal through (0, 0).
    let e = Point::new(-0.5, 1.5);
    assert!((far.distance_to(e) - (RADIUS_MM + STOCK_TO_LEAVE_MM)).abs() < 1e-6);
    assert!((near.distance_to(e) - RADIUS_MM).abs() < 1e-6);
    assert!((far.x - near.x).abs() > 0.4, "{far:?} {near:?}");
    let band = band(&path);
    let step = level(far, near, 0.0);
    assert!(step.length_mm() <= band.step_mm, "{}", step.length_mm());
    assert!(band.allows(step, true, SLACK_MM));
    // A step that is not level is no step.
    assert!(!band.allows(step, false, SLACK_MM));
}

/// The band at an open end stays the strip's. A level arc from the
/// stock path's end to the last pass's path that bows past the path's
/// end vertex, `r` to `r + a` from it, cuts past the open end and
/// still fails, and no point on the kept side, `r` to `r + a` from
/// the path, lies in the band.
#[test]
fn last_pass_step_past_an_open_end_or_on_the_kept_side_fails() {
    let path = line_path(&[(0.0, 0.0), (10.0, 0.0)]);
    let band = band(&path);
    let (far_end, near) = (Point::new(10.0, 1.3), Point::new(9.9, 1.0));
    assert!(band.allows(level(far_end, near, 0.0), true, SLACK_MM));
    let bowed = level(far_end, near, -1.5);
    let (middle, _) = bowed.point_at(bowed.length_mm() / 2.0).expect("an arc");
    assert!(middle.x > 10.1, "{middle:?}");
    let from_end_mm = middle.distance_to(Point::new(10.0, 0.0));
    assert!((RADIUS_MM..=RADIUS_MM + STOCK_TO_LEAVE_MM).contains(&from_end_mm));
    assert!(bowed.length_mm() <= band.step_mm, "{}", bowed.length_mm());
    assert!(!band.allows(bowed, true, SLACK_MM));
    assert!(!band.holds(Point::new(10.4, 1.15), SLACK_MM));
    assert!(!band.holds(Point::new(5.0, -1.15), SLACK_MM));
    assert!(band.holds(Point::new(5.0, 1.15), SLACK_MM));
}
