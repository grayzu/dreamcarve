// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 26 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Tracing and measuring"): the Measure tool (functional capability 103,
//! acceptance 126) and the size a selection reads while it is dragged or
//! resized (capability 83a).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key, measurement, shape_size};
use dc_app::viewport::handles::{self, Corner, Grab};
use dc_app::{App, Command, Pointer, Selection, Tool};
use dc_geometry::{Point, Polyline, Rect, Transform, Vertex};
use dc_project::{InstanceId, Units};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn shape(name: &str, profiles: Vec<Polyline>, transform: Transform) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles,
        transform,
        construction: false,
        folder: None,
    }
}

/// A new project with a 40 × 30 rectangle, its corner at (10, 10).
fn app_with_a_rectangle() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    let rect = Polyline::rect(Point::new(10.0, 10.0), 40.0, 30.0);
    app.run(Command::AddShapes(vec![shape(
        "plate",
        vec![rect],
        Transform::IDENTITY,
    )]))
    .unwrap();
    app
}

fn click(app: &mut App, x: f64, y: f64) {
    let at = Pointer::at(Point::new(x, y));
    app.tool_event(Event::Move(at));
    app.tool_event(Event::Press(at));
    app.tool_event(Event::Release(at));
}

/// The bar's readings, label then text.
fn readings(app: &App) -> Vec<(String, String)> {
    app.tool_bar()
        .unwrap()
        .readings
        .into_iter()
        .map(|reading| (reading.label, reading.text))
        .collect()
}

fn pairs(readings: &[(&str, &str)]) -> Vec<(String, String)> {
    readings
        .iter()
        .map(|(label, text)| ((*label).to_owned(), (*text).to_owned()))
        .collect()
}

/// Acceptance 126: from (0, 0) to (30, 40) reads 50, ΔX 30, ΔY 40 and
/// 53.13°. The angle is from +X, anticlockwise, 0 up to 360; the deltas
/// are signed, from the first point to the second.
#[test]
fn distance_deltas_angle() {
    let read = measurement(Point::new(0.0, 0.0), Point::new(30.0, 40.0));
    assert_eq!(
        (read.distance_mm, read.dx_mm, read.dy_mm),
        (50.0, 30.0, 40.0)
    );
    assert!((read.angle_deg - 53.130_102).abs() < 1e-6);
    let angle = |x: f64, y: f64| measurement(Point::new(5.0, 5.0), Point::new(5.0 + x, 5.0 + y));
    assert_eq!(angle(10.0, 0.0).angle_deg, 0.0);
    assert_eq!(angle(0.0, 10.0).angle_deg, 90.0);
    assert_eq!(angle(-10.0, 0.0).angle_deg, 180.0);
    assert_eq!(angle(0.0, -10.0).angle_deg, 270.0);
    assert_eq!(angle(-10.0, 0.0).dx_mm, -10.0);
    // No distance has no direction.
    let none = angle(0.0, 0.0);
    assert_eq!((none.distance_mm, none.angle_deg), (0.0, 0.0));
}

/// Capability 103: two clicks show the distance between them, its ΔX and
/// ΔY and its angle, in the tool bar and beside a dashed line. The
/// measurement stays until the next click starts another; `Escape`
/// leaves the tool. Nothing of it is project data.
#[test]
fn the_tape_is_two_clicks_and_stays_until_the_next() {
    let mut app = app_with_a_rectangle();
    app.prefs_mut().snap.enabled = false;
    // Nothing selected: a selected shape adds its own readings.
    app.select(Selection::Nothing);
    let before = app.project().clone();
    let steps = app.undo_label().map(str::to_owned);
    app.arm(Tool::Measure);
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.tool, Tool::Measure);
    assert_eq!(bar.prompt, "Click the point to measure from");
    assert!(bar.fields.is_empty() && bar.readings.is_empty() && !bar.can_finish);
    assert!(app.measurement().is_none());

    click(&mut app, 0.0, 0.0);
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click the point to measure to"
    );
    assert_eq!(app.draft().nodes.len(), 1, "the first point is marked");
    // The tape follows the pointer.
    app.tool_event(Event::Move(Pointer::at(Point::new(30.0, 0.0))));
    assert_eq!(app.measurement().unwrap().distance_mm, 30.0);
    assert_eq!(app.draft().dashed.len(), 1);
    click(&mut app, 30.0, 40.0);
    let read = app.measurement().unwrap();
    assert_eq!(
        (read.from_mm, read.to_mm),
        (Point::new(0.0, 0.0), Point::new(30.0, 40.0))
    );
    assert_eq!(
        readings(&app),
        pairs(&[
            ("Distance", "50 mm"),
            ("ΔX", "30 mm"),
            ("ΔY", "40 mm"),
            ("Angle", "53.13°"),
        ])
    );
    let draft = app.draft();
    assert_eq!(draft.dashed.len(), 1, "a dashed line between the points");
    assert_eq!(draft.nodes.len(), 2, "a mark on each");
    assert_eq!(draft.labels.len(), 1);
    assert_eq!(draft.labels[0].text, "50 · ΔX 30 · ΔY 40 · 53.13°");
    assert_eq!(draft.labels[0].at_mm, Point::new(15.0, 20.0));
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click to measure again; Escape ends"
    );
    // It stays while the pointer moves, until the next click.
    app.tool_event(Event::Move(Pointer::at(Point::new(80.0, 5.0))));
    assert_eq!(app.measurement().unwrap().distance_mm, 50.0);
    click(&mut app, 80.0, 5.0);
    assert!(app.measurement().is_none(), "another is begun");
    assert_eq!(app.draft().nodes.len(), 1);

    // In inches the same tape reads inches.
    click(&mut app, 80.0, 30.4);
    let mut inches = app.project().clone();
    inches.display_units = Units::Inch;
    let mut other = Harness::from_project(inches).app;
    other.prefs_mut().snap.enabled = false;
    other.arm(Tool::Measure);
    click(&mut other, 0.0, 0.0);
    click(&mut other, 25.4, 0.0);
    assert_eq!(
        readings(&other)[0],
        ("Distance".to_owned(), "1 in".to_owned())
    );

    // Escape leaves the tool, and the measurement with it.
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert!(app.measurement().is_none());
    assert_eq!(app.draft(), &dc_app::tool::Draft::default());
    assert_eq!(app.project(), &before, "a measurement is not project data");
    assert_eq!(app.undo_label().map(str::to_owned), steps);
}

/// Each click snaps as any point does: near a corner of a shape, the
/// tape is from the corner.
#[test]
fn the_tape_snaps_to_what_is_drawn() {
    let mut app = app_with_a_rectangle();
    app.arm(Tool::Measure);
    click(&mut app, 10.2, 10.1);
    click(&mut app, 49.9, 9.8);
    let read = app.measurement().unwrap();
    assert_eq!(
        (read.from_mm, read.to_mm),
        (Point::new(10.0, 10.0), Point::new(50.0, 10.0))
    );
    assert_eq!(read.distance_mm, 40.0);
}

/// Acceptance 126: with one shape selected, the bar shows its outline's
/// length and, closed, the area it encloses: a 40 × 30 rectangle reads
/// 140 and 1200. Both are of the shape as it is placed.
#[test]
fn length_and_area_of_a_shape() {
    let mut app = app_with_a_rectangle();
    let size = shape_size(app.project(), &id("i1")).unwrap();
    assert_eq!((size.length_mm, size.area_mm2), (140.0, Some(1200.0)));
    app.select(Selection::Instances(vec![id("i1")]));
    app.arm(Tool::Measure);
    assert_eq!(
        readings(&app),
        pairs(&[("Length", "140 mm"), ("Area", "1200 mm²")])
    );
    // With a tape too, the tape's readings come first.
    app.prefs_mut().snap.enabled = false;
    click(&mut app, 0.0, 0.0);
    click(&mut app, 30.0, 40.0);
    let all = readings(&app);
    assert_eq!(all.len(), 6);
    assert_eq!(all[4], ("Length".to_owned(), "140 mm".to_owned()));

    // A hole is not area; an open path has a length and no area; a
    // shape placed at twice its size reads twice the length and four
    // times the area.
    let hole = Polyline::rect(Point::new(20.0, 20.0), 10.0, 10.0);
    let outer = Polyline::rect(Point::new(10.0, 10.0), 40.0, 30.0);
    let line = Polyline::new(
        vec![Vertex::line(0.0, 0.0), Vertex::line(30.0, 40.0)],
        false,
    );
    app.run(Command::AddShapes(vec![
        shape("frame", vec![outer.clone(), hole], Transform::IDENTITY),
        shape("line", vec![line], Transform::IDENTITY),
        shape("big", vec![outer], Transform::scale(2.0, 2.0)),
    ]))
    .unwrap();
    let of = |app: &App, id_: &str| shape_size(app.project(), &id(id_)).unwrap();
    let frame = of(&app, "i2");
    assert_eq!((frame.length_mm, frame.area_mm2), (180.0, Some(1100.0)));
    let line = of(&app, "i3");
    assert_eq!((line.length_mm, line.area_mm2), (50.0, None));
    let big = of(&app, "i4");
    assert_eq!((big.length_mm, big.area_mm2), (280.0, Some(4800.0)));
    app.arm(Tool::Measure);
    app.select(Selection::Instances(vec![id("i3")]));
    assert_eq!(readings(&app), pairs(&[("Length", "50 mm")]));
    // Several shapes, or none, have no one size to read.
    app.select(Selection::Instances(vec![id("i1"), id("i3")]));
    assert!(readings(&app).is_empty());
    app.select(Selection::Nothing);
    assert!(readings(&app).is_empty());
    assert!(shape_size(app.project(), &id("nobody")).is_none());

    // In inches: 140 mm is 5.512 in, 1200 mm² is 1.86 in².
    let mut inches = app.project().clone();
    inches.display_units = Units::Inch;
    let mut other = Harness::from_project(inches).app;
    other.select(Selection::Instances(vec![id("i1")]));
    other.arm(Tool::Measure);
    assert_eq!(
        readings(&other),
        pairs(&[("Length", "5.512 in"), ("Area", "1.86 in²")])
    );
}

/// Capability 83a: a selection shows its box's width and height beside
/// it while it is dragged or resized, as a shape being drawn shows its
/// sizes. A rotation shows none: the status bar has its angle.
#[test]
fn a_dragged_selection_reads_its_size() {
    let app = app_with_a_rectangle();
    let strings = app.strings();
    let camera = app.view().camera;
    let handles = handles::handles(app.project(), &[id("i1")], &camera).unwrap();
    let label = |grab: Grab, from: Point, to: Point, uniform: bool| {
        let delta = handles::drag_delta(&handles, grab, from, to, uniform);
        handles::size_label(strings, Units::Mm, &handles, grab, delta)
    };
    // Moved by (5, -20): the same size, under where the box is now.
    let moved = label(
        Grab::Move,
        Point::new(20.0, 20.0),
        Point::new(25.0, 0.0),
        true,
    )
    .unwrap();
    assert_eq!(moved.text, "40 × 30");
    assert_eq!(moved.at_mm, Point::new(35.0, -10.0));
    // Resized from its top-right corner to twice its size about its
    // centre (30, 25): 80 × 60, from (-10, -5).
    let resized = label(
        Grab::Corner(Corner::TopRight),
        Point::new(50.0, 40.0),
        Point::new(70.0, 55.0),
        true,
    )
    .unwrap();
    assert_eq!(resized.text, "80 × 60");
    assert_eq!(resized.at_mm, Point::new(30.0, -5.0));
    let turned = label(
        Grab::Rotate { snap: false },
        handles.rotation,
        Point::new(60.0, 25.0),
        true,
    );
    assert!(turned.is_none());
    // A picture's box reads the same way.
    let boxed = handles::box_label(
        strings,
        Units::Inch,
        Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(50.8, 25.4),
        },
    );
    assert_eq!(boxed.text, "2 × 1");
    assert_eq!(boxed.at_mm, Point::new(25.4, 0.0));
}
