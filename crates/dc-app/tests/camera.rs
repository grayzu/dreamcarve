// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 4 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan →
//! Viewport geometry"; #533): the Viewport's camera. Project space is
//! Y-up millimetres, the screen is Y-down logical pixels from the pane's
//! top-left corner, and the camera is the one place between them
//! (`docs/rust-guidelines.md`, "GUI": the viewport flips Y exactly once).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::viewport::camera::{self, Camera, FIT_MARGIN_PX, MAX_PX_PER_MM, MIN_PX_PER_MM, Pos};
use dc_app::{Command, Selection};
use dc_geometry::{Point, Rect, Transform, Vec2, approx_eq};
use dc_project::InstanceId;

use common::plaque;

/// Within a millionth of a pixel or millimetre.
const EPS: f64 = 1e-6;

fn camera(centre: Point, px_per_mm: f64) -> Camera {
    Camera {
        centre_mm: centre,
        px_per_mm,
        viewport_px: Vec2::new(800.0, 600.0),
    }
}

/// `experience.md`, section 9, "Depth legend" (#616): Fit leaves the
/// legend's strip at the pane's left free. With a 260-px strip the
/// bounds sit inside the rest of the pane, within the margin, and fill
/// its width; with no strip, or one that is not a number, it is
/// [`Camera::fit`].
#[test]
fn fit_beside_leaves_the_left_strip_free() {
    let viewport_px = Vec2::new(900.0, 640.0);
    let bounds = Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(170.0, 120.0),
    };
    let strip_px = 260.0;
    let mut cam = camera(Point::new(0.0, 0.0), 1.0);
    cam.viewport_px = viewport_px;
    cam.fit_beside(bounds, strip_px);
    let mut tight = false;
    for p in corners(bounds) {
        let s = cam.to_screen(p);
        assert!(
            s.x_px >= strip_px + FIT_MARGIN_PX - EPS
                && s.x_px <= viewport_px.x - FIT_MARGIN_PX + EPS,
            "{p:?} at {s:?} is under the strip"
        );
        assert!(s.y_px >= FIT_MARGIN_PX - EPS && s.y_px <= viewport_px.y - FIT_MARGIN_PX + EPS);
        tight |= approx_eq(s.x_px, strip_px + FIT_MARGIN_PX, 1e-6);
    }
    assert!(tight, "the fit fills the width beside the strip");

    let mut plain = cam;
    plain.fit(bounds);
    let mut none = cam;
    none.fit_beside(bounds, 0.0);
    assert_eq!(plain, none);
    let mut bad = cam;
    bad.fit_beside(bounds, f64::NAN);
    assert_eq!(plain, bad, "a strip that is not a number is none");
}

fn assert_point(got: Point, want: Point, what: &str) {
    assert!(
        approx_eq(got.x, want.x, EPS) && approx_eq(got.y, want.y, EPS),
        "{what}: got {got:?}, want {want:?}"
    );
}

fn assert_pos(got: Pos, want: Pos, what: &str) {
    assert!(
        approx_eq(got.x_px, want.x_px, EPS) && approx_eq(got.y_px, want.y_px, EPS),
        "{what}: got {got:?}, want {want:?}"
    );
}

/// The design's camera (`Camera::to_screen`, "the one Y flip"): the
/// centre is the pane's middle; +X in project space is +X on screen;
/// +Y in project space is *up* the screen, so screen Y falls as project Y
/// rises, by `px_per_mm` per millimetre, and only Y is negated. `to_world`
/// undoes it exactly, so a point survives the round trip either way.
#[test]
fn to_screen_flips_y_once_and_to_world_inverts_it() {
    let cam = camera(Point::new(50.0, 50.0), 5.0);
    assert_pos(
        cam.to_screen(Point::new(50.0, 50.0)),
        Pos::new(400.0, 300.0),
        "centre",
    );
    assert_pos(
        cam.to_screen(Point::new(60.0, 50.0)),
        Pos::new(450.0, 300.0),
        "10 mm right is 50 px right",
    );
    assert_pos(
        cam.to_screen(Point::new(50.0, 60.0)),
        Pos::new(400.0, 250.0),
        "10 mm up is 50 px up the screen",
    );
    // The stock's bottom-left corner is low on the screen, its top-right
    // high: the Viewport shows the stock as the operator sees it.
    let bottom_left = cam.to_screen(Point::new(0.0, 0.0));
    let top_right = cam.to_screen(Point::new(100.0, 100.0));
    assert!(bottom_left.x_px < top_right.x_px && bottom_left.y_px > top_right.y_px);

    for cam in [
        camera(Point::new(50.0, 50.0), 5.0),
        camera(Point::new(-12.5, 340.25), 0.37),
        camera(Point::new(1500.0, -20.0), 120.0),
    ] {
        for p in [
            Point::new(0.0, 0.0),
            Point::new(13.7, -4.2),
            Point::new(-250.0, 999.5),
        ] {
            assert_point(cam.to_world(cam.to_screen(p)), p, "world → screen → world");
        }
        for s in [
            Pos::new(0.0, 0.0),
            Pos::new(799.0, 1.0),
            Pos::new(123.4, 567.8),
        ] {
            assert_pos(cam.to_screen(cam.to_world(s)), s, "screen → world → screen");
        }
    }
}

/// Wheel and pinch zoom about the cursor (PureCutCNC
/// `usePointerGestures.ts`, `handleWheelEvent`: the world point under the
/// cursor before the zoom is under it after). Holds for any cursor
/// position, in and out, and at the clamp: past the limits the scale
/// stops at `MIN_PX_PER_MM` or `MAX_PX_PER_MM` and the point still stays.
/// A factor that is not a finite positive number changes nothing.
#[test]
fn zoom_about_the_cursor_keeps_it_fixed() {
    for at in [
        Pos::new(400.0, 300.0),
        Pos::new(10.0, 590.0),
        Pos::new(733.0, 41.0),
    ] {
        for factor in [1.25, 0.8, 4.0, 1e-9, 1e9] {
            let mut cam = camera(Point::new(50.0, 50.0), 5.0);
            let before = cam.to_world(at);
            cam.zoom_about(at, factor);
            assert_point(
                cam.to_world(at),
                before,
                &format!("zoom ×{factor} about {at:?}"),
            );
            let want = (5.0 * factor).clamp(MIN_PX_PER_MM, MAX_PX_PER_MM);
            assert!(
                approx_eq(cam.px_per_mm, want, EPS),
                "zoom ×{factor}: {} px/mm, want {want}",
                cam.px_per_mm
            );
        }
    }
    let start = camera(Point::new(50.0, 50.0), 5.0);
    for factor in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        let mut cam = start;
        cam.zoom_about(Pos::new(100.0, 100.0), factor);
        assert_eq!(cam, start, "zoom ×{factor} is ignored");
    }

    // Panning moves the content with the pointer: the point under the
    // pointer before the drag is under it after.
    let mut cam = start;
    let grabbed = cam.to_world(Pos::new(200.0, 200.0));
    cam.pan(30.0, -45.0);
    assert_point(cam.to_world(Pos::new(230.0, 155.0)), grabbed, "pan");
}

/// The plaque's corners on screen, for `fit`.
fn corners(rect: Rect) -> [Point; 4] {
    [
        rect.min,
        Point::new(rect.max.x, rect.min.y),
        rect.max,
        Point::new(rect.min.x, rect.max.y),
    ]
}

/// Fit (F, functional capability 72) frames the stock and every visible
/// shape, with `FIT_MARGIN_PX` round them and the tighter axis filled:
/// on the plaque as opened, and with a shape moved off the stock, which
/// Fit must still show. A hidden shape is not part of it. Zoom to
/// selection (Shift F) frames the selection's bounds the same way.
#[test]
fn fit_shows_stock_and_shapes() {
    let mut harness = Harness::from_project(plaque());
    let viewport_px = Vec2::new(900.0, 640.0);

    let check = |harness: &Harness, label: &str| {
        let app = &harness.app;
        let bounds = camera::fit_bounds(app.project(), app.scene());
        let stock = &app.project().stock;
        let stock_rect = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(stock.width_mm, stock.height_mm),
        };
        for p in corners(stock_rect) {
            assert!(
                bounds.min.x <= p.x + EPS
                    && bounds.min.y <= p.y + EPS
                    && bounds.max.x + EPS >= p.x
                    && bounds.max.y + EPS >= p.y,
                "{label}: stock corner {p:?} outside {bounds:?}"
            );
        }
        let mut cam = Camera {
            centre_mm: Point::new(-500.0, 900.0),
            px_per_mm: 0.5,
            viewport_px,
        };
        cam.fit(bounds);
        let mut tight = false;
        for p in corners(bounds) {
            let s = cam.to_screen(p);
            for (coord, size) in [(s.x_px, viewport_px.x), (s.y_px, viewport_px.y)] {
                assert!(
                    coord >= FIT_MARGIN_PX - EPS && coord <= size - FIT_MARGIN_PX + EPS,
                    "{label}: corner {p:?} at {s:?} is not inside the margin"
                );
                tight |= approx_eq(coord, FIT_MARGIN_PX, 1e-6);
            }
        }
        assert!(tight, "{label}: the fit fills one axis to the margin");
        for shape in app.scene().shapes() {
            let Some(b) = shape.bounds else { continue };
            for p in corners(b) {
                let s = cam.to_screen(p);
                assert!(
                    s.x_px >= FIT_MARGIN_PX - EPS
                        && s.x_px <= viewport_px.x - FIT_MARGIN_PX + EPS
                        && s.y_px >= FIT_MARGIN_PX - EPS
                        && s.y_px <= viewport_px.y - FIT_MARGIN_PX + EPS,
                    "{label}: {} at {s:?} is off screen",
                    shape.instance
                );
            }
        }
        bounds
    };

    let as_opened = check(&harness, "as opened");

    // A shape moved 300 mm right of the stock widens what Fit shows.
    let first = harness.app.project().instances[0].id.clone();
    harness
        .run(Command::TransformInstances {
            ids: vec![first.clone()],
            delta: Transform::translate(300.0, 0.0),
        })
        .unwrap();
    let moved = check(&harness, "a shape off the stock");
    assert!(moved.max.x > as_opened.max.x + 250.0, "{moved:?}");

    // Hidden, it no longer counts.
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![first.clone()],
            visible: Some(false),
            locked: None,
        })
        .unwrap();
    let hidden = check(&harness, "the moved shape hidden");
    assert!(hidden.max.x < moved.max.x - 250.0, "{hidden:?}");

    // Zoom to selection: the selection's bounds fill the pane instead.
    let second: InstanceId = harness.app.project().instances[1].id.clone();
    harness
        .app
        .select(Selection::Instances(vec![second.clone()]));
    let selected = harness
        .app
        .scene()
        .bounds_of(harness.app.selection().instances())
        .expect("the selection has bounds");
    let shape = harness.app.scene().shape(&second).expect("in the scene");
    assert_eq!(Some(selected), shape.bounds);
    let mut cam = Camera {
        viewport_px,
        ..Camera::default()
    };
    cam.fit(selected);
    let low = cam.to_screen(selected.min);
    let high = cam.to_screen(selected.max);
    let filled = approx_eq(
        high.x_px - low.x_px,
        viewport_px.x - 2.0 * FIT_MARGIN_PX,
        1e-6,
    ) || approx_eq(
        low.y_px - high.y_px,
        viewport_px.y - 2.0 * FIT_MARGIN_PX,
        1e-6,
    );
    assert!(filled, "zoom to selection fills the pane: {low:?} {high:?}");
}
