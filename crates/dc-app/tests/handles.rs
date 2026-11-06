// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 5 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan →
//! Viewport geometry"; #534): the transform handles on the Viewport
//! (functional capability 58, acceptance 66; `experience.md`, section 8,
//! "Transform handles"). A corner drag scales about the selection's
//! centre, keeping proportions unless the padlock is open; the rotation
//! handle rotates about the centre; `drag_delta` is the `Transform` that
//! `TransformInstances` applies there.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::f64::consts::FRAC_PI_2;

use dc_app::Command;
use dc_app::properties;
use dc_app::viewport::camera::Camera;
use dc_app::viewport::handles::{self, Corner, Grab, Handles};
use dc_geometry::{Point, Rect, Transform, Vec2};
use dc_project::{InstanceId, Project};

use common::{assert_near, assert_same_transform, plaque};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn centre(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// A camera at 4 px/mm on a 1000 × 800 pane, framing the plaque.
fn camera() -> Camera {
    Camera {
        centre_mm: Point::new(85.0, 60.0),
        px_per_mm: 4.0,
        viewport_px: Vec2::new(1000.0, 800.0),
    }
}

fn square_handles(project: &Project) -> Handles {
    handles::handles(project, &[id("i4")], &camera()).expect("the square has handles")
}

fn bounds_of(project: &Project, ids: &[InstanceId]) -> Rect {
    properties::placement(project, ids)
        .expect("a placement")
        .bounds
}

/// Experience section 8: the handles sit on the selection's box, four
/// corners and a rotation handle 20 px above the top's centre; a corner
/// drag along the diagonal scales by the ratio of the distances from the
/// centre, and the scaled box keeps its centre.
#[test]
fn corner_scale_is_about_the_centre() {
    let project = plaque();
    let h = square_handles(&project);
    let box_ = bounds_of(&project, &[id("i4")]);
    assert_near(h.bounds.min, box_.min, 1e-9, "the handles' box");
    assert_near(h.bounds.max, box_.max, 1e-9, "the handles' far corner");
    let c = centre(box_);
    assert_near(h.corner(Corner::TopRight), box_.max, 1e-9, "top-right");
    assert_near(h.corner(Corner::BottomLeft), box_.min, 1e-9, "bottom-left");
    let stem_mm = 20.0 / camera().px_per_mm;
    assert_near(
        h.rotation,
        Point::new(c.x, box_.max.y + stem_mm),
        1e-9,
        "the rotation handle",
    );
    assert!(!h.locked);

    // The corner, grabbed on screen.
    let at = camera().to_screen(h.corner(Corner::TopRight));
    assert_eq!(
        handles::grab_at(&h, &camera(), at),
        Some(Grab::Corner(Corner::TopRight))
    );
    let inside = camera().to_screen(c);
    assert_eq!(handles::grab_at(&h, &camera(), inside), Some(Grab::Move));

    // Dragged out along the diagonal to one and a half times the distance.
    let from = h.corner(Corner::TopRight);
    let to = Point::new(c.x + (from.x - c.x) * 1.5, c.y + (from.y - c.y) * 1.5);
    let delta = handles::drag_delta(&h, Grab::Corner(Corner::TopRight), from, to, true);
    assert_same_transform(delta, Transform::scale(1.5, 1.5), 1e-9, "the drag's delta");
    let after = Command::TransformInstances {
        ids: vec![id("i4")],
        delta,
    }
    .apply(&project)
    .expect("scales");
    let scaled = bounds_of(&after, &[id("i4")]);
    assert_near(centre(scaled), c, 1e-9, "the centre stays");
    assert!((scaled.width_mm() - box_.width_mm() * 1.5).abs() < 1e-9);
    assert!((scaled.height_mm() - box_.height_mm() * 1.5).abs() < 1e-9);

    // The preview draws what the command will do.
    let preview = handles::about_centre(&h, delta);
    let placed = instance_of_transform(&project, "i4").then(preview);
    assert_same_transform(
        instance_of_transform(&after, "i4"),
        placed,
        1e-9,
        "the preview equals the result",
    );

    // Dragged through the centre: never a collapse or a flip.
    let past = Point::new(c.x - 1.0, c.y - 1.0);
    let delta = handles::drag_delta(&h, Grab::Corner(Corner::TopRight), from, past, true);
    let [a, _, _, d, _, _] = delta.coeffs();
    assert!(a > 0.0 && d > 0.0, "{delta:?}");

    // A move is the pointer's travel.
    let delta = handles::drag_delta(
        &h,
        Grab::Move,
        Point::new(80.0, 30.0),
        Point::new(92.5, 21.0),
        true,
    );
    assert_same_transform(delta, Transform::translate(12.5, -9.0), 1e-9, "a move");
}

fn instance_of_transform(project: &Project, s: &str) -> Transform {
    common::instance_of(project, s).transform
}

/// Experience section 8: the rotation handle's drag turns the selection by
/// the angle the pointer sweeps about the centre (counter-clockwise
/// positive, Y-up), snapping to 15° with Shift; the centre stays.
#[test]
fn rotation_handle_rotates_about_the_centre() {
    let project = plaque();
    let h = square_handles(&project);
    let c = centre(h.bounds);
    let at = camera().to_screen(h.rotation);
    assert_eq!(
        handles::grab_at(&h, &camera(), at),
        Some(Grab::Rotate { snap: false })
    );

    // From above the centre to its left: a quarter turn counter-clockwise.
    let from = h.rotation;
    let r = from.y - c.y;
    let to = Point::new(c.x - r, c.y);
    let delta = handles::drag_delta(&h, Grab::Rotate { snap: false }, from, to, true);
    assert_same_transform(delta, Transform::rotate(FRAC_PI_2), 1e-9, "a quarter turn");
    let after = Command::TransformInstances {
        ids: vec![id("i4")],
        delta,
    }
    .apply(&project)
    .expect("rotates");
    assert_near(
        centre(bounds_of(&after, &[id("i4")])),
        c,
        1e-9,
        "the centre stays",
    );
    let rotation = properties::placement(&after, &[id("i4")])
        .and_then(|p| p.rotation_deg)
        .expect("one shape has one rotation");
    assert!((rotation - 90.0).abs() < 1e-9, "{rotation}");

    // 47° swept, snapped: 45°.
    let angle = 47f64.to_radians();
    let swept = Point::new(c.x - r * angle.sin(), c.y + r * angle.cos());
    let snapped = handles::drag_delta(&h, Grab::Rotate { snap: true }, from, swept, true);
    assert_same_transform(
        snapped,
        Transform::rotate(45f64.to_radians()),
        1e-9,
        "snapped to 15°",
    );
    let free = handles::drag_delta(&h, Grab::Rotate { snap: false }, from, swept, true);
    assert_same_transform(free, Transform::rotate(angle), 1e-9, "unsnapped");
}

/// Capability 58: a corner drag keeps proportions while the W/H padlock is
/// closed, whichever way the pointer moves; open (or with Alt), each axis
/// follows the pointer.
#[test]
fn uniform_unless_unlocked() {
    let project = plaque();
    let h = square_handles(&project);
    let c = centre(h.bounds);
    let from = h.corner(Corner::TopRight);
    // Out in X only, twice the half-width.
    let to = Point::new(c.x + (from.x - c.x) * 2.0, from.y);

    let uniform = handles::drag_delta(&h, Grab::Corner(Corner::TopRight), from, to, true);
    let [a, b, cc, d, e, f] = uniform.coeffs();
    assert!((a - d).abs() < 1e-9, "proportions kept: {uniform:?}");
    assert!(a > 1.0, "it grows: {uniform:?}");
    assert!(b.abs() < 1e-12 && cc.abs() < 1e-12 && e.abs() < 1e-12 && f.abs() < 1e-12);

    let free = handles::drag_delta(&h, Grab::Corner(Corner::TopRight), from, to, false);
    assert_same_transform(free, Transform::scale(2.0, 1.0), 1e-9, "X alone");

    // The bottom-left corner mirrors the sense: out to the left grows too.
    let from = h.corner(Corner::BottomLeft);
    let to = Point::new(c.x + (from.x - c.x) * 2.0, from.y);
    let free = handles::drag_delta(&h, Grab::Corner(Corner::BottomLeft), from, to, false);
    assert_same_transform(
        free,
        Transform::scale(2.0, 1.0),
        1e-9,
        "X alone, other corner",
    );

    // A locked selection's handles say so.
    let locked = Command::SetInstanceFlags {
        ids: vec![id("i4")],
        visible: None,
        locked: Some(true),
    }
    .apply(&project)
    .expect("locks");
    assert!(square_handles(&locked).locked);
}
