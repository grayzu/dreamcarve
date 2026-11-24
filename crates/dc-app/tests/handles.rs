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

/// Review feedback, 2026-11-23: a shape is stretched in one direction by
/// the handle at the middle of a side. The drag moves that side with the
/// pointer and leaves the opposite side where it is, whatever the
/// proportion link says; the other axis is untouched.
#[test]
fn a_side_handle_stretches_one_way_and_keeps_the_far_side() {
    use dc_app::viewport::handles::Side;
    let project = plaque();
    let h = square_handles(&project);
    let box_ = bounds_of(&project, &[id("i4")]);
    let c = centre(box_);
    // The four handles are at the middles of the sides, and each is
    // grabbed there; a corner still wins at a corner.
    for (side, at) in [
        (Side::Left, Point::new(box_.min.x, c.y)),
        (Side::Right, Point::new(box_.max.x, c.y)),
        (Side::Bottom, Point::new(c.x, box_.min.y)),
        (Side::Top, Point::new(c.x, box_.max.y)),
    ] {
        assert_near(h.side(side), at, 1e-9, "a side's handle");
        assert_eq!(
            handles::grab_at(&h, &camera(), camera().to_screen(at)),
            Some(Grab::Side(side)),
            "{side:?}"
        );
    }
    assert_eq!(
        handles::grab_at(&h, &camera(), camera().to_screen(box_.max)),
        Some(Grab::Corner(Corner::TopRight))
    );

    let stretched = |side: Side, from: Point, to: Point, uniform: bool| {
        let delta = handles::drag_delta(&h, Grab::Side(side), from, to, uniform);
        let after = Command::TransformInstances {
            ids: vec![id("i4")],
            delta,
        }
        .apply(&project)
        .expect("stretches");
        // The preview draws what the command will do.
        assert_same_transform(
            instance_of_transform(&after, "i4"),
            instance_of_transform(&project, "i4").then(handles::about_centre(&h, delta)),
            1e-9,
            "the preview equals the result",
        );
        bounds_of(&after, &[id("i4")])
    };
    // The right side dragged 7 mm out, the pointer wandering in Y: the
    // box is 7 mm wider to the right and no taller, link closed or open.
    for uniform in [true, false] {
        let from = h.side(Side::Right);
        let to = Point::new(from.x + 7.0, from.y + 3.0);
        let after = stretched(Side::Right, from, to, uniform);
        assert_near(
            after.min,
            box_.min,
            1e-9,
            "the left side and the bottom stay",
        );
        assert_near(
            after.max,
            Point::new(box_.max.x + 7.0, box_.max.y),
            1e-9,
            "the right side follows the pointer",
        );
    }
    // The left side dragged 4 mm in: the right side stays.
    let from = h.side(Side::Left);
    let after = stretched(Side::Left, from, Point::new(from.x + 4.0, from.y), true);
    assert_near(
        after.min,
        Point::new(box_.min.x + 4.0, box_.min.y),
        1e-9,
        "the left side follows",
    );
    assert_near(after.max, box_.max, 1e-9, "the right side stays");
    // The top dragged 5 mm up, the bottom 2 mm down.
    let from = h.side(Side::Top);
    let after = stretched(Side::Top, from, Point::new(from.x, from.y + 5.0), true);
    assert_near(after.min, box_.min, 1e-9, "the bottom stays");
    assert!((after.height_mm() - box_.height_mm() - 5.0).abs() < 1e-9);
    assert!((after.width_mm() - box_.width_mm()).abs() < 1e-9);
    let from = h.side(Side::Bottom);
    let after = stretched(Side::Bottom, from, Point::new(from.x, from.y - 2.0), true);
    assert_near(after.max, box_.max, 1e-9, "the top stays");
    assert!((after.height_mm() - box_.height_mm() - 2.0).abs() < 1e-9);

    // Dragged through the far side: never a collapse or a flip.
    let from = h.side(Side::Right);
    let past = Point::new(box_.min.x - 10.0, from.y);
    let delta = handles::drag_delta(&h, Grab::Side(Side::Right), from, past, true);
    let [a, _, _, d, _, _] = delta.coeffs();
    assert!(a > 0.0 && (d - 1.0).abs() < 1e-12, "{delta:?}");

    // The size written under the box while it is dragged is the new one.
    let strings = dc_app::Strings::english();
    let from = h.side(Side::Right);
    let delta = handles::drag_delta(
        &h,
        Grab::Side(Side::Right),
        from,
        Point::new(from.x + 7.0, from.y),
        true,
    );
    let label = handles::size_label(
        &strings,
        dc_project::Units::Mm,
        &h,
        Grab::Side(Side::Right),
        delta,
    )
    .expect("a stretch reads its size");
    let wanted = format!(
        "{} × {}",
        dc_app::units::summary_text(box_.width_mm() + 7.0, dc_project::Units::Mm),
        dc_app::units::summary_text(box_.height_mm(), dc_project::Units::Mm)
    );
    assert_eq!(label.text, wanted);
}

/// A side carries a handle only where there is room for one: a box seen
/// small has its corners alone, and a line has no side to stretch.
#[test]
fn a_short_side_has_no_handle() {
    use dc_app::viewport::handles::Side;
    let project = plaque();
    let h = square_handles(&project);
    assert_eq!(h.sides(&camera()).len(), 4);
    // Zoomed out until the square is 20 px across: its corners' handles
    // are 20 px apart, with no room for one between.
    let far = Camera {
        px_per_mm: 20.0 / h.bounds.width_mm(),
        ..camera()
    };
    assert!(h.sides(&far).is_empty());
    assert_eq!(
        handles::grab_at(&h, &far, far.to_screen(h.side(Side::Right))),
        Some(Grab::Move),
        "the middle of a short side is the box"
    );
    // A box with no height: nothing to stretch upwards, and its ends are
    // its corners.
    let mut flat = h;
    flat.bounds.max.y = flat.bounds.min.y;
    assert!(flat.sides(&camera()).is_empty());
}

/// The maintainer's review of 2026-11-23: with `Shift` held a move goes
/// level or upright, whichever the pointer has gone further along, so a
/// shape is slid along a line without drifting off it.
#[test]
fn shift_holds_a_move_level_or_upright() {
    let from = Point::new(10.0, 20.0);
    let level = handles::straight(from, Point::new(42.0, 23.5));
    assert_eq!(
        (level.x.to_bits(), level.y.to_bits()),
        (42f64.to_bits(), 20f64.to_bits())
    );
    let upright = handles::straight(from, Point::new(7.0, -31.0));
    assert_eq!(
        (upright.x.to_bits(), upright.y.to_bits()),
        (10f64.to_bits(), (-31f64).to_bits())
    );
    // As far each way: level.
    let even = handles::straight(from, Point::new(15.0, 25.0));
    assert_eq!(
        (even.x.to_bits(), even.y.to_bits()),
        (15f64.to_bits(), 20f64.to_bits())
    );
    // Nowhere yet: where it is.
    assert_eq!(handles::straight(from, from), from);
}
