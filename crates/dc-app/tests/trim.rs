// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 15 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Changing shapes"): the Trim tool, driven as the window drives it
//! (functional capability 96; acceptance 121). The trimming itself is
//! `dc_geometry::trim`'s and is tested there; these tests hold what the
//! tool makes of it: which outline is taken, what is highlighted, and
//! what becomes of the shape.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key};
use dc_app::{App, Command, Pointer, Tool};
use dc_geometry::{Point, Polyline, Transform, Vertex};
use dc_project::{FeatureDefinition, InstanceId, Shape};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn line(name: &str, points: &[(f64, f64)]) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )],
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    }
}

/// A blank project with `shapes` and the Trim tool armed.
fn trimming(shapes: Vec<NewShape>) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(shapes)).unwrap();
    app.arm(Tool::Trim);
    app
}

fn click(app: &mut App, at: Point) -> dc_app::tool::Reply {
    app.tool_event(Event::Move(Pointer::at(at)));
    app.tool_event(Event::Press(Pointer::at(at)))
}

fn definition<'a>(app: &'a App, name: &str) -> &'a FeatureDefinition {
    let instance = app
        .project()
        .instances
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("no shape {name}"));
    &app.project().definitions[&instance.definition]
}

fn ends(app: &App, name: &str) -> ((f64, f64), (f64, f64)) {
    let outline = &definition(app, name).profiles[0];
    let v = outline.vertices();
    ((v[0].x, v[0].y), (v[v.len() - 1].x, v[v.len() - 1].y))
}

fn names(app: &App) -> Vec<&str> {
    app.project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

/// Acceptance 121: two crossing lines: the part under the pointer is
/// highlighted, and a click on one arm removes that arm up to the
/// crossing; a click on the middle part of a line crossed twice splits
/// it in two; a line nothing crosses is left, and the status bar says
/// so.
#[test]
fn a_click_removes_the_piece_between_crossings() {
    let mut app = trimming(vec![
        line("Across", &[(10.0, 30.0), (90.0, 30.0)]),
        line("Post", &[(30.0, 10.0), (30.0, 50.0)]),
        line("Post 2", &[(70.0, 10.0), (70.0, 50.0)]),
        line("Apart", &[(10.0, 80.0), (90.0, 80.0)]),
    ]);
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click the part of an outline to trim away, between two crossings"
    );
    // The piece a click would remove shows as the pointer moves.
    app.tool_event(Event::Move(Pointer::at(pt(80.0, 30.5))));
    let piece = app.draft().highlights.clone();
    assert_eq!(piece.len(), 1);
    assert_eq!(piece[0].vertices()[0], Vertex::line(70.0, 30.0));
    app.tool_event(Event::Move(Pointer::at(pt(80.0, 60.0))));
    assert!(app.draft().highlights.is_empty(), "nothing in reach");

    // The middle, between the posts: two shapes, in the one's place.
    let reply = click(&mut app, pt(50.0, 30.0));
    assert_eq!(reply.ran.as_deref(), Some("Trim “Across”"));
    assert_eq!(
        names(&app),
        ["Across", "Across (2)", "Post", "Post 2", "Apart"]
    );
    assert_eq!(ends(&app, "Across"), ((10.0, 30.0), (30.0, 30.0)));
    assert_eq!(ends(&app, "Across (2)"), ((70.0, 30.0), (90.0, 30.0)));
    assert_eq!(app.tool(), Tool::Trim, "the tool stays armed");
    app.undo();
    assert_eq!(app.project().instances.len(), 4);

    // The right-hand arm: the line is shortened, and is still one shape.
    let reply = click(&mut app, pt(80.0, 30.0));
    assert_eq!(reply.ran.as_deref(), Some("Trim “Across”"));
    assert_eq!(ends(&app, "Across"), ((10.0, 30.0), (70.0, 30.0)));
    assert_eq!(app.project().instances.len(), 4);
    // Its end now only touches the second post, which is no crossing:
    // the part past the first post goes whole.
    let reply = click(&mut app, pt(50.0, 30.0));
    assert!(reply.ran.is_some());
    assert_eq!(ends(&app, "Across"), ((10.0, 30.0), (30.0, 30.0)));

    // Nothing crosses the line apart.
    let before = app.project().clone();
    let reply = click(&mut app, pt(50.0, 80.0));
    assert_eq!(reply.ran, None);
    assert_eq!(
        reply.refused.as_deref(),
        Some("Nothing crosses this outline")
    );
    assert_eq!(app.project(), &before);
    // A click on nothing is nothing.
    let reply = click(&mut app, pt(50.0, 65.0));
    assert!(reply.ran.is_none() && reply.refused.is_none());
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
}

/// Acceptance 121 and capability 96: a circle crossed by a line opens
/// between the crossings; being a shape that keeps its sizes, it becomes
/// a plain path in the same undo step, and stays the one shape with what
/// cut it.
#[test]
fn a_circle_opens_and_becomes_a_path() {
    let circle = Shape::Circle { diameter_mm: 40.0 };
    let disc = NewShape {
        name: "Circle 1".to_owned(),
        profiles: circle.outline(0.01).unwrap().unwrap(),
        shape: Some(circle),
        transform: Transform::translate(50.0, 50.0),
        construction: false,
        folder: None,
    };
    let mut app = trimming(vec![disc, line("Chord", &[(10.0, 62.0), (90.0, 62.0)])]);
    // The cap above the chord, clicked at its top.
    let reply = click(&mut app, pt(50.0, 70.0));
    assert_eq!(reply.ran.as_deref(), Some("Trim “Circle 1”"));
    let trimmed = definition(&app, "Circle 1");
    assert_eq!(trimmed.shape, None, "no longer a circle of 40");
    assert_eq!(trimmed.profiles.len(), 1);
    let outline = &trimmed.profiles[0];
    assert!(!outline.is_closed());
    // What is left is arcs of the circle, from one crossing to the other,
    // in the shape's own space: its centre is the origin there.
    for segment in outline.segments() {
        let arc = segment.arc().expect("an arc");
        assert!((arc.radius_mm - 20.0).abs() < 1e-9);
        assert!(arc.centre.x.abs() < 1e-9 && arc.centre.y.abs() < 1e-9);
    }
    let cap = 20.0 * 2.0 * (16.0f64 / 20.0).asin();
    assert!((outline.length_mm() - (std::f64::consts::TAU * 20.0 - cap)).abs() < 1e-9);
    assert_eq!(app.project().instances[0].id, InstanceId::new("i1"));
    // One undo step: the circle and its size are back.
    app.undo();
    assert!(definition(&app, "Circle 1").shape.is_some());
}
