// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The origin of a selection (the maintainer's review of 2026-11-23): the
//! point of its box that a resize, a turn and a flip hold still. It is one
//! of nine, the box's corners, the middles of its sides and its centre,
//! and the centre until another is chosen, which is what every such
//! change was about before.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::Command;
use dc_app::arrange::{self, Action};
use dc_app::origin::Origin;
use dc_app::properties::{self, Edit};
use dc_app::test_support::Harness;
use dc_app::viewport::camera::Camera;
use dc_app::viewport::handles::{self, Corner, Grab};
use dc_geometry::{Point, Rect, Vec2};
use dc_project::InstanceId;

use common::{assert_near, plaque_bytes};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn camera() -> Camera {
    Camera {
        centre_mm: Point::new(85.0, 60.0),
        px_per_mm: 4.0,
        viewport_px: Vec2::new(1000.0, 800.0),
    }
}

fn square(harness: &Harness) -> Rect {
    properties::placement(harness.app.project(), &[id("i4")])
        .expect("the square is placed")
        .bounds
}

/// The nine points, and the centre by default.
#[test]
fn the_origin_is_one_of_nine_points_of_the_box() {
    let b = Rect {
        min: Point::new(10.0, 20.0),
        max: Point::new(50.0, 40.0),
    };
    assert_eq!(Origin::default(), Origin::Centre);
    assert_eq!(Origin::ALL.len(), 9);
    for (origin, at) in [
        (Origin::TopLeft, (10.0, 40.0)),
        (Origin::Top, (30.0, 40.0)),
        (Origin::TopRight, (50.0, 40.0)),
        (Origin::Left, (10.0, 30.0)),
        (Origin::Centre, (30.0, 30.0)),
        (Origin::Right, (50.0, 30.0)),
        (Origin::BottomLeft, (10.0, 20.0)),
        (Origin::Bottom, (30.0, 20.0)),
        (Origin::BottomRight, (50.0, 20.0)),
    ] {
        assert_eq!(origin.point(b), Point::new(at.0, at.1), "{origin:?}");
    }
    // The chooser reads as the box does: rows from the top, each from
    // the left.
    assert_eq!(
        Origin::ALL[..3],
        [Origin::TopLeft, Origin::Top, Origin::TopRight]
    );
    let strings = dc_app::Strings::english();
    assert_eq!(strings.get(Origin::BottomLeft.name()), "Bottom left");
    assert_eq!(strings.get(Origin::Centre.name()), "Centre");
}

/// A typed width holds the origin still: from the centre both sides
/// move, from the bottom-left corner only the right side does, and from
/// the right side's middle only the left.
#[test]
fn a_typed_size_holds_the_origin_still() {
    let harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = square(&harness);
    let wider = Edit::W {
        mm: before.width_mm() + 10.0,
        uniform: false,
    };
    let resized = |origin: Origin, edit: Edit| {
        let mut harness = Harness::open(&plaque_bytes()).unwrap();
        let command =
            properties::edit_about(harness.app.project(), &[id("i4")], edit, origin).unwrap();
        let label = harness.run(command).unwrap();
        assert_eq!(label, "Scale “square”", "{origin:?}");
        square(&harness)
    };
    let centred = resized(Origin::Centre, wider);
    assert_near(
        centred.min,
        Point::new(before.min.x - 5.0, before.min.y),
        1e-9,
        "from the centre, 5 mm each way",
    );
    let from_corner = resized(Origin::BottomLeft, wider);
    assert_near(from_corner.min, before.min, 1e-9, "the corner stays");
    assert_near(
        from_corner.max,
        Point::new(before.max.x + 10.0, before.max.y),
        1e-9,
        "the right side goes",
    );
    let from_right = resized(Origin::Right, wider);
    assert_near(from_right.max, before.max, 1e-9, "the right side stays");
    assert!((from_right.min.x - (before.min.x - 10.0)).abs() < 1e-9);
    // With the proportions kept, a taller square grows away from its
    // top-right corner.
    let taller = Edit::H {
        mm: before.height_mm() * 2.0,
        uniform: true,
    };
    let grown = resized(Origin::TopRight, taller);
    assert_near(grown.max, before.max, 1e-9, "the top-right corner stays");
    assert!((grown.width_mm() - before.width_mm() * 2.0).abs() < 1e-9);
    // `edit` is `edit_about` the centre, as it always was.
    assert_eq!(
        properties::edit(harness.app.project(), &[id("i4")], wider).unwrap(),
        properties::edit_about(harness.app.project(), &[id("i4")], wider, Origin::Centre).unwrap()
    );
    // A typed X or Y moves the shape whatever the origin.
    let moved = resized_x(&harness, Origin::TopRight);
    assert!((moved.min.x - 3.0).abs() < 1e-9);
}

fn resized_x(_: &Harness, origin: Origin) -> Rect {
    let mut harness = Harness::open(&plaque_bytes()).unwrap();
    let command = properties::edit_about(
        harness.app.project(),
        &[id("i4")],
        Edit::X { mm: 3.0 },
        origin,
    )
    .unwrap();
    harness.run(command).unwrap();
    square(&harness)
}

/// A turn and a flip are about the origin. About the centre they are the
/// commands they always were; about another point the shape swings
/// round it.
#[test]
fn a_turn_and_a_flip_are_about_the_origin() {
    let harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let project = harness.app.project();
    let before = square(&harness);
    let ids = [id("i4")];
    // The centre: unchanged commands, and their labels.
    let centred = arrange::arrange_about(
        project,
        &ids,
        0,
        Action::RotateAnticlockwise,
        Origin::Centre,
    )
    .unwrap();
    assert!(matches!(centred.command, Command::RotateInstances { .. }));
    assert_eq!(
        arrange::arrange(project, &ids, 0, Action::RotateAnticlockwise).unwrap(),
        centred
    );
    let done = |action: Action, origin: Origin| {
        let mut harness = Harness::open(&plaque_bytes()).unwrap();
        let arranged =
            arrange::arrange_about(harness.app.project(), &ids, 0, action, origin).unwrap();
        let label = harness.run(arranged.command).unwrap();
        (square(&harness), label)
    };
    // A quarter turn anticlockwise about the bottom-left corner: the box
    // (a square) lands to the left of where it was, that corner now its
    // bottom-right.
    let (turned, label) = done(Action::RotateAnticlockwise, Origin::BottomLeft);
    assert_eq!(label, "Rotate “square”");
    assert_near(
        Point::new(turned.max.x, turned.min.y),
        before.min,
        1e-9,
        "the corner it turned about",
    );
    assert!((turned.width_mm() - before.height_mm()).abs() < 1e-9);
    // A typed angle about the top-right corner keeps that corner.
    let (turned, _) = done(Action::Rotate { angle_deg: 180.0 }, Origin::TopRight);
    assert_near(turned.min, before.max, 1e-9, "half a turn about a corner");
    // Flipped left to right about its left side, it lies on the other
    // side of that side; about the centre it stays in its box.
    let (flipped, label) = done(Action::FlipHorizontal, Origin::Left);
    assert_eq!(label, "Mirror “square”");
    assert!((flipped.max.x - before.min.x).abs() < 1e-9, "{flipped:?}");
    assert!((flipped.width_mm() - before.width_mm()).abs() < 1e-9);
    let (flipped, _) = done(Action::FlipVertical, Origin::Top);
    assert!((flipped.min.y - before.max.y).abs() < 1e-9, "{flipped:?}");
    let (flipped, label) = done(Action::FlipHorizontal, Origin::Centre);
    assert_eq!(label, "Flip “square” horizontally");
    assert_near(flipped.min, before.min, 1e-9, "in its own box");
    // A flip left to right about the box's top or bottom middle is about
    // the same line as the centre's.
    let (flipped, _) = done(Action::FlipHorizontal, Origin::Top);
    assert_near(flipped.min, before.min, 1e-9, "the same mirror line");
}

/// The handles scale and turn about the origin: a corner dragged away
/// from it grows the box from it, the corner that is the origin grows
/// it from the opposite corner, and the rotation handle swings the box
/// round it.
#[test]
fn the_handles_work_about_the_origin() {
    let harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let project = harness.app.project();
    let before = square(&harness);
    let centred = handles::handles(project, &[id("i4")], &camera()).unwrap();
    assert_eq!(centred.pivot, centred.centre(), "the centre until chosen");
    let h = centred.about(Origin::BottomLeft);
    assert_eq!(h.pivot, before.min);
    let after = |grab: Grab, from: Point, to: Point, uniform: bool| {
        let delta = handles::drag_delta(&h, grab, from, to, uniform);
        let next = Command::TransformInstances {
            ids: vec![id("i4")],
            delta,
        }
        .apply(project)
        .expect("applies");
        properties::placement(&next, &[id("i4")]).unwrap().bounds
    };
    // The top-right corner dragged to one and a half times its distance
    // from the origin.
    let from = h.corner(Corner::TopRight);
    let to = Point::new(
        before.min.x + before.width_mm() * 1.5,
        before.min.y + before.height_mm() * 1.5,
    );
    let grown = after(Grab::Corner(Corner::TopRight), from, to, true);
    assert_near(grown.min, before.min, 1e-9, "the origin stays");
    assert!((grown.width_mm() - before.width_mm() * 1.5).abs() < 1e-9);
    // The corner that is the origin has nothing to pull against there:
    // it scales from the opposite corner.
    let from = h.corner(Corner::BottomLeft);
    let to = Point::new(from.x - before.width_mm(), from.y - before.height_mm());
    let grown = after(Grab::Corner(Corner::BottomLeft), from, to, true);
    assert_near(grown.max, before.max, 1e-9, "the opposite corner stays");
    assert!((grown.width_mm() - before.width_mm() * 2.0).abs() < 1e-9);
    // The rotation handle, a quarter turn about the origin: the box
    // lands to its left, as the Arrange button's turn does.
    let from = Point::new(before.min.x + 10.0, before.min.y);
    let to = Point::new(before.min.x, before.min.y + 10.0);
    let turned = after(Grab::Rotate { snap: false }, from, to, true);
    assert_near(
        Point::new(turned.max.x, turned.min.y),
        before.min,
        1e-9,
        "swung round the origin",
    );
    // A side handle is one way whatever the origin: the far side stays.
    let from = h.side(handles::Side::Right);
    let stretched = after(
        Grab::Side(handles::Side::Right),
        from,
        Point::new(from.x + 6.0, from.y),
        true,
    );
    assert_near(stretched.min, before.min, 1e-9, "the left side stays");
    let top = h.about(Origin::Top);
    let from = top.side(handles::Side::Right);
    let delta = handles::drag_delta(
        &top,
        Grab::Side(handles::Side::Right),
        from,
        Point::new(from.x + 6.0, from.y),
        true,
    );
    let next = Command::TransformInstances {
        ids: vec![id("i4")],
        delta,
    }
    .apply(project)
    .unwrap();
    let stretched = properties::placement(&next, &[id("i4")]).unwrap().bounds;
    assert_near(
        stretched.min,
        before.min,
        1e-9,
        "also with the origin elsewhere",
    );
}
