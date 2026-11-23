// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/featureLifecycleOps.test.ts

//! Slice 20 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Organising"): align and distribute (functional capability 98a). The
//! ports are of *Align / distribute — ≥3 features*; Top and Bottom are
//! re-derived for Y-up, where upstream's top is its least Y.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

use dc_app::arrange::{self, Action, Align, Axis, Disabled, Spacing};
use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::{App, Command, Selection, Strings};
use dc_geometry::{Point, Polyline, Rect, Transform};
use dc_project::InstanceId;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn rectangle(name: &str, (x, y): (f64, f64), (w, h): (f64, f64)) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(x, y), w, h)],
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    }
}

/// Upstream's three rectangles, 30 × 15, the second moved so that the
/// three are evenly spaced along neither axis: `i1` at (10, 10), `i2` at
/// (80, 40), `i3` at (50, 90). The stock is 100 × 100.
fn three() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(vec![
        rectangle("Rect1", (10.0, 10.0), (30.0, 15.0)),
        rectangle("Rect2", (80.0, 40.0), (30.0, 15.0)),
        rectangle("Rect3", (50.0, 90.0), (30.0, 15.0)),
    ]))
    .unwrap();
    app.select(Selection::Instances(vec![id("i1"), id("i2"), id("i3")]));
    app
}

fn box_of(app: &App, instance: &str) -> Rect {
    let project = app.project();
    let placed = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap();
    placed
        .transform
        .apply_polyline(&project.definitions[&placed.definition].profiles[0])
        .unwrap()
        .bounds()
        .unwrap()
}

fn boxes(app: &App) -> [Rect; 3] {
    [box_of(app, "i1"), box_of(app, "i2"), box_of(app, "i3")]
}

/// Runs `action` on the selection; the label of its undo step.
fn act(app: &mut App, action: Action) -> String {
    let selected = app.selection().instances().to_vec();
    let arranged = arrange::arrange(app.project(), &selected, 0, action).unwrap();
    app.run(arranged.command).unwrap()
}

fn align(side: Align) -> Action {
    Action::Align {
        side,
        to_stock: false,
    }
}

/// Ports of *alignFeatures left*, *right* and *center_horizontal*: each
/// shape's side meets the same side of the selection's box; one undo
/// step, and undo puts every shape back.
#[test]
fn align_left_right_and_centre() {
    let mut app = three();
    let before = boxes(&app);
    assert_eq!(act(&mut app, align(Align::Left)), "Align 3 shapes");
    assert!(
        boxes(&app).iter().all(|b| b.min.x == 10.0),
        "{:?}",
        boxes(&app)
    );
    assert!(
        boxes(&app)
            .iter()
            .zip(&before)
            .all(|(b, a)| b.min.y == a.min.y),
        "nothing moves along Y"
    );
    assert!(app.undo().is_some());
    assert_eq!(boxes(&app), before, "one undo step");

    act(&mut app, align(Align::Right));
    assert!(boxes(&app).iter().all(|b| b.max.x == 110.0));
    app.undo();

    act(&mut app, align(Align::Centre));
    assert!(
        boxes(&app)
            .iter()
            .all(|b| f64::midpoint(b.min.x, b.max.x) == 60.0)
    );
}

/// Top is the greatest Y (ADR-0001), where upstream's *top* is its least:
/// the shapes' upper sides meet the selection's upper side.
#[test]
fn top_is_the_greatest_y() {
    let mut app = three();
    act(&mut app, align(Align::Top));
    assert!(
        boxes(&app).iter().all(|b| b.max.y == 105.0),
        "{:?}",
        boxes(&app)
    );
    app.undo();
    act(&mut app, align(Align::Bottom));
    assert!(boxes(&app).iter().all(|b| b.min.y == 10.0));
    app.undo();
    act(&mut app, align(Align::Middle));
    assert!(
        boxes(&app)
            .iter()
            .all(|b| f64::midpoint(b.min.y, b.max.y) == 57.5)
    );
}

/// One shape selected, or *To stock* ticked: the stock is what the
/// shapes align to (PureCutCNC has no align to stock).
#[test]
fn to_stock() {
    let mut app = three();
    app.select(Selection::Instances(vec![id("i2")]));
    assert_eq!(act(&mut app, align(Align::Left)), "Align “Rect2”");
    assert_eq!(box_of(&app, "i2").min.x, 0.0);
    act(&mut app, align(Align::Top));
    assert_eq!(box_of(&app, "i2").max.y, 100.0);
    act(&mut app, align(Align::Centre));
    assert_eq!(box_of(&app, "i2").min.x, 35.0);

    app.select(Selection::Instances(vec![id("i1"), id("i2"), id("i3")]));
    act(
        &mut app,
        Action::Align {
            side: Align::Right,
            to_stock: true,
        },
    );
    assert!(boxes(&app).iter().all(|b| b.max.x == 100.0));
}

/// Ports of *distributeFeatures horizontal_gaps* and *vertical_centers*:
/// the outermost two stay, and the gaps, or the centres' steps, between
/// are made equal.
#[test]
fn distribute_by_gaps_and_by_centres() {
    let distribute = |axis, spacing| Action::Distribute { axis, spacing };
    // Along X the boxes are 10–40, 50–80 and 80–110: 10 of gap in all.
    let mut app = three();
    assert_eq!(
        act(&mut app, distribute(Axis::Horizontal, Spacing::Gaps)),
        "Distribute 3 shapes"
    );
    let [first, last, middle] = boxes(&app);
    assert_eq!(
        (first.min.x, last.min.x),
        (10.0, 80.0),
        "the outermost stay"
    );
    assert_eq!(middle.min.x - first.max.x, last.min.x - middle.max.x);
    assert_eq!(middle.min.x, 45.0);
    assert!(app.undo().is_some());

    // The centres along X are 25, 65 and 95: the middle one goes to 60.
    act(&mut app, distribute(Axis::Horizontal, Spacing::Centres));
    assert_eq!(box_of(&app, "i3").min.x, 45.0);
    app.undo();

    // Along Y the centres are 17.5, 47.5 and 97.5.
    act(&mut app, distribute(Axis::Vertical, Spacing::Centres));
    let [low, middle, high] = boxes(&app);
    let centre = |b: Rect| f64::midpoint(b.min.y, b.max.y);
    assert_eq!(centre(middle) - centre(low), centre(high) - centre(middle));
    assert_eq!((low.min.y, high.min.y), (10.0, 90.0));
    app.undo();
    act(&mut app, distribute(Axis::Vertical, Spacing::Gaps));
    assert_eq!(box_of(&app, "i2").min.y, 50.0);

    // Evenly spaced already: nothing to change, and no undo step.
    let again = arrange::arrange(
        app.project(),
        app.selection().instances(),
        0,
        distribute(Axis::Vertical, Spacing::Gaps),
    )
    .unwrap();
    assert!(matches!(
        app.run(again.command),
        Err(dc_app::CommandError::NothingToDo)
    ));

    // Two shapes have nothing between them to space.
    let two = [id("i1"), id("i2")];
    assert_eq!(
        arrange::arrange(
            app.project(),
            &two,
            0,
            distribute(Axis::Horizontal, Spacing::Gaps)
        ),
        Err(Disabled::TooFewToDistribute)
    );
    assert_eq!(
        Strings::english().get(Disabled::TooFewToDistribute.reason()),
        "Select three or more unlocked shapes to distribute them"
    );
}

/// A locked shape does not move and is named; the others still align to
/// the whole selection's box, the locked shape's side included.
#[test]
fn a_locked_shape_stays_and_is_named() {
    let mut app = three();
    app.run(Command::SetInstanceFlags {
        ids: vec![id("i1")],
        visible: None,
        locked: Some(true),
    })
    .unwrap();
    app.select(Selection::Instances(vec![id("i1"), id("i2"), id("i3")]));
    let selected = app.selection().instances().to_vec();
    let arranged = arrange::arrange(app.project(), &selected, 0, align(Align::Left)).unwrap();
    assert_eq!(arranged.locked, ["Rect1"]);
    assert_eq!(app.run(arranged.command).unwrap(), "Align 2 shapes");
    assert!(
        boxes(&app).iter().all(|b| b.min.x == 10.0),
        "{:?}",
        boxes(&app)
    );

    // Distribute spaces the unlocked ones: two are too few.
    assert_eq!(
        arrange::arrange(
            app.project(),
            &selected,
            0,
            Action::Distribute {
                axis: Axis::Horizontal,
                spacing: Spacing::Gaps
            }
        ),
        Err(Disabled::TooFewToDistribute)
    );
}

/// A turned shape aligns by the box it fills as it is placed.
#[test]
fn a_turned_shape_aligns_by_its_placed_box() {
    let mut app = three();
    app.select(Selection::Instances(vec![id("i2")]));
    act(&mut app, Action::RotateClockwise);
    let turned = box_of(&app, "i2");
    assert_eq!(turned.width_mm().round(), 15.0);
    app.select(Selection::Instances(vec![id("i1"), id("i2"), id("i3")]));
    act(&mut app, align(Align::Left));
    assert!(
        boxes(&app).iter().all(|b| (b.min.x - 10.0).abs() < 1e-9),
        "{:?}",
        boxes(&app)
    );
}
