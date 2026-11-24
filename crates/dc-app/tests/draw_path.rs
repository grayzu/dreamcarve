// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Drawing tools"): Line, Arc and Spline, one path drawn a point at a
//! time, driven as the window drives them (functional capabilities 87 to
//! 89; acceptance 112).
//!
//! PureCutCNC has no test that drives its composite tool's clicks
//! (digest §11); these are written from the behaviour the functional
//! spec takes from `pendingAddSlice.ts`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key, SizeField};
use dc_app::{App, Drawing, Pointer, Tool};
use dc_geometry::{Path, Point, Polyline, Span, Transform, Vertex, nearest_point};
use dc_project::{FeatureDefinition, FeatureInstance, Shape};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A blank project with `drawing` armed and snapping off, so a point is
/// where the test puts it.
fn armed(drawing: Drawing) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(drawing));
    app
}

fn move_to(app: &mut App, at: Point) {
    app.tool_event(Event::Move(Pointer::at(at)));
}

/// A click: the button down and up in one place. Returns the undo label
/// of what it committed.
fn click(app: &mut App, at: Point) -> Option<String> {
    let pointer = Pointer::at(at);
    app.tool_event(Event::Move(pointer));
    let pressed = app.tool_event(Event::Press(pointer));
    let released = app.tool_event(Event::Release(pointer));
    assert_eq!(pressed.refused, None);
    pressed.ran.or(released.ran)
}

fn type_size(app: &mut App, field: SizeField, text: &str) {
    app.tool_event(Event::Field {
        field,
        text: text.to_owned(),
    });
}

fn key(app: &mut App, key: Key) -> Option<String> {
    app.tool_event(Event::Key(key)).ran
}

fn enter(app: &mut App) -> Option<String> {
    key(app, Key::Enter)
}

/// The last shape drawn: its instance and its definition.
fn last(app: &App) -> (&FeatureInstance, &FeatureDefinition) {
    let instance = app.project().instances.last().expect("a shape was drawn");
    (instance, &app.project().definitions[&instance.definition])
}

/// The last shape's one outline. A path is saved where it lies.
fn outline(app: &App) -> &Polyline {
    let (instance, definition) = last(app);
    assert_eq!(instance.transform, Transform::IDENTITY);
    assert_eq!(definition.profiles.len(), 1);
    &definition.profiles[0]
}

#[track_caller]
fn assert_vertices(outline: &Polyline, want: &[(f64, f64, f64)]) {
    let got = outline.vertices();
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (vertex, (x, y, bulge)) in got.iter().zip(want) {
        assert!(
            (vertex.x - x).abs() < 1e-9
                && (vertex.y - y).abs() < 1e-9
                && (vertex.bulge - bulge).abs() < 1e-9,
            "{got:?}"
        );
    }
}

fn fields(app: &App) -> Vec<SizeField> {
    app.tool_bar()
        .expect("a drawing tool has a bar")
        .fields
        .iter()
        .map(|row| row.field)
        .collect()
}

fn outline_problems(app: &App) -> usize {
    dc_project::validate(app.project())
        .iter()
        .filter(|p| p.code() == "shape_outline_differs")
        .count()
}

/// Acceptance 112, lines: clicks at (0,0), (40,0), (40,30) and `Enter`
/// save an open path of two lines; a fourth click on the first point
/// saves a closed triangle. A line is its outline and has no `shape`.
#[test]
fn line_open_and_closed() {
    let mut app = armed(Drawing::Line);
    assert_eq!(app.tool_bar().unwrap().prompt, "Click the first point");
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert_eq!(enter(&mut app), None, "nothing to finish");
    assert_eq!(click(&mut app, pt(0.0, 0.0)), None);
    assert!(!app.tool_bar().unwrap().can_finish, "one point is no line");
    assert_eq!(enter(&mut app), None);
    assert_eq!(fields(&app), [SizeField::Length, SizeField::Angle]);
    assert_eq!(click(&mut app, pt(40.0, 0.0)), None);
    assert!(app.tool_bar().unwrap().can_finish);
    assert_eq!(click(&mut app, pt(40.0, 30.0)), None);
    assert!(app.project().instances.is_empty());
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Line 1”"));

    let (instance, definition) = last(&app);
    assert_eq!(instance.name, "Line 1");
    assert_eq!(definition.shape, None);
    assert!(!outline(&app).is_closed());
    assert_vertices(
        outline(&app),
        &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0), (40.0, 30.0, 0.0)],
    );
    // The tool is still armed, with nothing begun.
    assert_eq!(app.tool(), Tool::Draw(Drawing::Line));
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert!(app.draft().outlines.is_empty());

    // A fourth click on the first point, 11 px off it, closes.
    let px_per_mm = app.view().camera.px_per_mm;
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    // With two points the first is only a point: a click there adds one.
    click(&mut app, pt(40.0, 30.0));
    let label = click(&mut app, pt(11.0 / px_per_mm, 0.0));
    assert_eq!(label.as_deref(), Some("Draw “Line 2”"));
    assert!(outline(&app).is_closed());
    assert_vertices(
        outline(&app),
        &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0), (40.0, 30.0, 0.0)],
    );
    assert_eq!(last(&app).1.shape, None);
    assert_eq!(app.project().instances.len(), 2);

    // 13 px off the first point is a point of its own. (Drawn clear of
    // the open Line 1, whose end it would otherwise carry on.)
    click(&mut app, pt(0.0, 50.0));
    click(&mut app, pt(40.0, 50.0));
    click(&mut app, pt(40.0, 80.0));
    assert_eq!(click(&mut app, pt(13.0 / px_per_mm, 50.0)), None);
    assert!(enter(&mut app).is_some());
    assert_eq!(outline(&app).vertices().len(), 4);
    assert!(!outline(&app).is_closed());

    // One undo step each, and the file holds them.
    assert_eq!(app.undo_label(), Some("Draw “Line 3”"));
    let saved = app.serialize().unwrap();
    assert_eq!(
        &dc_project::load(saved.as_bytes()).unwrap().project,
        app.project()
    );
}

/// Capability 87: **Done** and a double-click finish a path open as
/// `Enter` does; a two-point line is a line; `Escape` discards the draft,
/// then leaves the tool.
#[test]
fn done_double_click_and_escape() {
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(10.0, 10.0));
    click(&mut app, pt(30.0, 10.0));
    assert_eq!(
        app.tool_event(Event::Done).ran.as_deref(),
        Some("Draw “Line 1”")
    );
    assert_vertices(outline(&app), &[(10.0, 10.0, 0.0), (30.0, 10.0, 0.0)]);

    // A double-click: the second press lands where the first did and adds
    // nothing; the path ends at the point clicked twice.
    click(&mut app, pt(10.0, 20.0));
    click(&mut app, pt(30.0, 20.0));
    click(&mut app, pt(30.0, 40.0));
    click(&mut app, pt(30.0, 40.0));
    let reply = app.tool_event(Event::DoubleClick(Pointer::at(pt(30.0, 40.0))));
    assert_eq!(reply.ran.as_deref(), Some("Draw “Line 2”"));
    assert_eq!(outline(&app).vertices().len(), 3);

    click(&mut app, pt(50.0, 50.0));
    click(&mut app, pt(60.0, 50.0));
    move_to(&mut app, pt(70.0, 60.0));
    assert_eq!(app.draft().outlines.len(), 1, "the path and the next span");
    assert_eq!(app.draft().outlines[0].vertices().len(), 3);
    key(&mut app, Key::Escape);
    assert_eq!(
        app.tool(),
        Tool::Draw(Drawing::Line),
        "the draft goes first"
    );
    assert!(app.draft().outlines.is_empty());
    assert_eq!(enter(&mut app), None);
    key(&mut app, Key::Escape);
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.project().instances.len(), 2);
}

/// Capability 87: `Backspace` takes back the last point; in an arc, its
/// end first.
#[test]
fn backspace_takes_back_a_point() {
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    click(&mut app, pt(40.0, 30.0));
    key(&mut app, Key::Backspace);
    click(&mut app, pt(20.0, 20.0));
    assert!(enter(&mut app).is_some());
    assert_vertices(
        outline(&app),
        &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0), (20.0, 20.0, 0.0)],
    );

    // An arc's end that waits for its through point goes first.
    click(&mut app, pt(0.0, 50.0));
    click(&mut app, pt(40.0, 50.0));
    app.arm(Tool::Draw(Drawing::Arc));
    click(&mut app, pt(40.0, 80.0));
    assert_eq!(fields(&app), [SizeField::Radius]);
    key(&mut app, Key::Backspace);
    assert_eq!(fields(&app), [SizeField::Length, SizeField::Angle]);
    assert!(enter(&mut app).is_some());
    assert_vertices(outline(&app), &[(0.0, 50.0, 0.0), (40.0, 50.0, 0.0)]);

    // Back to nothing: the next click is a first point again.
    click(&mut app, pt(5.0, 5.0));
    key(&mut app, Key::Backspace);
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    key(&mut app, Key::Backspace);
    assert_eq!(app.tool(), Tool::Draw(Drawing::Arc), "nothing more to take");
}

/// Acceptance 112, arcs: with Arc armed before the third click, then
/// (40,30) and a through point, the second segment is an arc through
/// that point; three points in a line add nothing, and say why.
#[test]
fn arc_segment_through_a_point() {
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    // `A`: the path is kept, and the next span is an arc.
    app.arm(Tool::Draw(Drawing::Arc));
    assert_eq!(app.tool_bar().unwrap().prompt, "Click where the arc ends");
    assert_eq!(click(&mut app, pt(40.0, 30.0)), None);
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click a point the arc passes through, or type its radius"
    );

    // A through point on the chord's line makes no arc.
    let on_the_line = Pointer::at(pt(40.0, 15.0));
    app.tool_event(Event::Move(on_the_line));
    let reply = app.tool_event(Event::Press(on_the_line));
    assert_eq!(reply.ran, None);
    assert!(
        reply
            .refused
            .as_deref()
            .is_some_and(|why| why.starts_with("No arc passes through three points in a line")),
        "{reply:?}"
    );
    assert_eq!(fields(&app), [SizeField::Radius], "still the arc's step");

    // Through (55, 15): half a circle of radius 15, bowing to +X, which
    // is counter-clockwise from (40, 0) up to (40, 30).
    move_to(&mut app, pt(55.0, 15.0));
    assert_eq!(app.tool_bar().unwrap().fields[0].text, "15");
    assert_eq!(app.draft().labels[0].text, "R 15");
    assert_eq!(click(&mut app, pt(55.0, 15.0)), None);
    // `P`: back to lines.
    app.arm(Tool::Draw(Drawing::Line));
    click(&mut app, pt(0.0, 30.0));
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Path 1”"));
    assert_vertices(
        outline(&app),
        &[
            (0.0, 0.0, 0.0),
            (40.0, 0.0, 1.0),
            (40.0, 30.0, 0.0),
            (0.0, 30.0, 0.0),
        ],
    );
    assert_eq!(last(&app).1.shape, None, "lines and arcs are an outline");

    // Armed from the tools, Arc starts in arc mode: a lone arc is three
    // clicks and Enter. Through a point on the other side it bows the
    // other way.
    app.arm(Tool::Draw(Drawing::Arc));
    click(&mut app, pt(40.0, 50.0));
    click(&mut app, pt(40.0, 80.0));
    click(&mut app, pt(25.0, 65.0));
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Arc 1”"));
    assert_vertices(outline(&app), &[(40.0, 50.0, -1.0), (40.0, 80.0, 0.0)]);

    // An arc that ends on the first point closes the path.
    app.arm(Tool::Draw(Drawing::Line));
    click(&mut app, pt(100.0, 0.0));
    click(&mut app, pt(140.0, 0.0));
    click(&mut app, pt(140.0, 30.0));
    app.arm(Tool::Draw(Drawing::Arc));
    click(&mut app, pt(100.0, 0.0));
    let label = click(&mut app, pt(100.0, 30.0));
    assert_eq!(label.as_deref(), Some("Draw “Path 2”"));
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 3);
    assert!(outline(&app).vertices()[2].bulge.abs() > 0.1);
}

/// Capability 88: a typed Radius replaces the second click, the arc
/// bowing to the pointer's side; a radius under half the chord is
/// refused at its field.
#[test]
fn arc_by_a_typed_radius() {
    let mut app = armed(Drawing::Arc);
    click(&mut app, pt(40.0, 0.0));
    click(&mut app, pt(40.0, 30.0));
    move_to(&mut app, pt(20.0, 10.0));
    type_size(&mut app, SizeField::Radius, "10");
    let row = &app.tool_bar().unwrap().fields[0];
    assert_eq!(
        row.problem.as_deref(),
        Some("an arc between these ends has a radius of 15 or more")
    );
    assert!(!row.pinned);
    type_size(&mut app, SizeField::Radius, "15");
    assert!(app.tool_bar().unwrap().fields[0].pinned);
    // The preview is the arc as it would be made: to the pointer's side.
    assert!(app.draft().outlines[0].vertices()[0].bulge < 0.0);
    assert_eq!(enter(&mut app), None, "Enter takes the arc, not the path");
    assert_eq!(fields(&app), [SizeField::Length, SizeField::Angle]);
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Arc 1”"));
    assert_vertices(outline(&app), &[(40.0, 0.0, -1.0), (40.0, 30.0, 0.0)]);

    // The other side, and a wider radius: the short way round.
    click(&mut app, pt(0.0, 100.0));
    click(&mut app, pt(30.0, 100.0));
    move_to(&mut app, pt(15.0, 90.0));
    type_size(&mut app, SizeField::Radius, "25");
    enter(&mut app);
    enter(&mut app);
    let vertices = outline(&app).vertices();
    // Sagitta 25 − √(25² − 15²) = 5 over a half chord of 15.
    assert!(
        (vertices[0].bulge - 5.0 / 15.0).abs() < 1e-9,
        "{vertices:?}"
    );
}

/// Capability 83: the next point of a line by its typed Length and
/// Angle, degrees anticlockwise from +X; the first by X and Y from the
/// origin. `Enter` takes the point while a size is typed, and finishes
/// the path when none is.
#[test]
fn line_by_typed_sizes() {
    let mut app = armed(Drawing::Line);
    let origin = {
        let project = app.project();
        project.origin.xy.point_mm(&project.stock)
    };
    type_size(&mut app, SizeField::X, "10");
    type_size(&mut app, SizeField::Y, "10");
    assert_eq!(enter(&mut app), None);
    assert_eq!(fields(&app), [SizeField::Length, SizeField::Angle]);
    move_to(&mut app, pt(origin.x + 50.0, origin.y + 10.0));
    let texts: Vec<String> = app
        .tool_bar()
        .unwrap()
        .fields
        .into_iter()
        .map(|row| row.text)
        .collect();
    assert_eq!(texts, ["40", "0"]);
    assert_eq!(app.draft().labels[0].text, "40");
    type_size(&mut app, SizeField::Length, "20");
    type_size(&mut app, SizeField::Angle, "90");
    assert_eq!(enter(&mut app), None, "the point, not the path");
    // Only a length: along the pointer's direction.
    move_to(&mut app, pt(origin.x - 40.0, origin.y + 30.0));
    type_size(&mut app, SizeField::Length, "5");
    enter(&mut app);
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Line 1”"));
    let (x, y) = (origin.x, origin.y);
    assert_vertices(
        outline(&app),
        &[
            (x + 10.0, y + 10.0, 0.0),
            (x + 10.0, y + 30.0, 0.0),
            (x + 5.0, y + 30.0, 0.0),
        ],
    );
}

/// Acceptance 112, splines: a Spline through four points saves `shape`
/// path with cubic segments and an outline within the flatten tolerance
/// of them, passing through the four points.
#[test]
fn spline_is_a_path_with_cubics() {
    let points = [pt(0.0, 0.0), pt(30.0, 20.0), pt(60.0, 0.0), pt(90.0, 20.0)];
    let mut app = armed(Drawing::Spline);
    for point in points {
        assert_eq!(click(&mut app, point), None);
    }
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click the next point of the curve; Enter finishes, a click on the first point closes"
    );
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Spline 1”"));
    let tolerance_mm = app.project().flatten_tolerance_mm;
    let (_, definition) = last(&app);
    let Some(Shape::Path { paths }) = &definition.shape else {
        panic!("a curve keeps its nodes: {:?}", definition.shape);
    };
    assert_eq!(paths.as_slice(), [Path::spline(&points, false)]);
    let path = &paths[0];
    assert!(!path.is_closed());
    assert_eq!(path.span_count(), 3);
    assert!(
        path.nodes()
            .iter()
            .take(3)
            .all(|node| matches!(node.span, Span::Cubic { .. }))
    );
    // The outline is the path flattened, and passes through the points.
    let flattened = outline(&app);
    assert_eq!(flattened, &path.to_polyline(tolerance_mm));
    assert!(flattened.vertices().len() > 4);
    for point in points {
        assert!(nearest_point(flattened, point).unwrap().gap_mm < 1e-9);
    }
    // Every point of the outline is within the tolerance of the curve.
    for vertex in flattened.vertices() {
        let hit = path.nearest(pt(vertex.x, vertex.y)).unwrap();
        assert!(hit.gap_mm <= tolerance_mm, "{hit:?}");
    }
    assert_eq!(outline_problems(&app), 0);
    let saved = app.serialize().unwrap();
    assert_eq!(
        &dc_project::load(saved.as_bytes()).unwrap().project,
        app.project()
    );

    // A click on the first point closes the curve: one closed spline.
    let ring = [pt(0.0, 50.0), pt(30.0, 50.0), pt(30.0, 80.0), pt(0.0, 80.0)];
    for point in ring {
        click(&mut app, point);
    }
    assert_eq!(click(&mut app, ring[0]).as_deref(), Some("Draw “Spline 2”"));
    let Some(Shape::Path { paths }) = &last(&app).1.shape else {
        panic!("a curve keeps its nodes");
    };
    assert_eq!(paths.as_slice(), [Path::spline(&ring, true)]);
    assert!(outline(&app).is_closed());
    assert_eq!(outline_problems(&app), 0);

    // Two points make a straight line, as upstream's do, and no `shape`.
    click(&mut app, pt(100.0, 0.0));
    click(&mut app, pt(120.0, 0.0));
    enter(&mut app);
    assert_eq!(last(&app).1.shape, None);
    assert_vertices(outline(&app), &[(100.0, 0.0, 0.0), (120.0, 0.0, 0.0)]);
}

/// Capability 89: while drawing a Line, `S` makes the following segments
/// curves. The run of curves is smooth through its own points and meets
/// the lines at corners.
#[test]
fn a_line_can_go_on_as_a_curve() {
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    app.arm(Tool::Draw(Drawing::Spline));
    assert_eq!(app.prefs().drawing_tool, Drawing::Spline);
    click(&mut app, pt(60.0, 20.0));
    click(&mut app, pt(80.0, 0.0));
    app.arm(Tool::Draw(Drawing::Line));
    click(&mut app, pt(120.0, 0.0));
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Path 1”"));
    let Some(Shape::Path { paths }) = &last(&app).1.shape else {
        panic!("a path with a curve keeps its nodes");
    };
    let nodes = paths[0].nodes();
    assert_eq!(nodes.len(), 5);
    assert_eq!(nodes[0].span, Span::Line);
    assert!(matches!(nodes[1].span, Span::Cubic { .. }));
    assert!(matches!(nodes[2].span, Span::Cubic { .. }));
    assert_eq!(nodes[3].span, Span::Line);
    // The curve's two spans are the open spline through its three points.
    let curve = Path::spline(&[pt(40.0, 0.0), pt(60.0, 20.0), pt(80.0, 0.0)], false);
    assert_eq!(nodes[1].span, curve.nodes()[0].span);
    assert_eq!(nodes[2].span, curve.nodes()[1].span);
    assert_eq!(
        nodes.iter().map(|node| node.smooth).collect::<Vec<_>>(),
        [false, false, true, false, false]
    );
    assert_eq!(outline_problems(&app), 0);
    let first = outline(&app).vertices()[0];
    assert_eq!(first, Vertex::line(0.0, 0.0));

    // Arming a tool that is not one of the three drops the path.
    click(&mut app, pt(0.0, 50.0));
    click(&mut app, pt(10.0, 50.0));
    app.arm(Tool::Draw(Drawing::Rectangle));
    app.arm(Tool::Draw(Drawing::Line));
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert_eq!(app.project().instances.len(), 1);
}

/// Draws an open path through `points` and finishes it with `Enter`.
fn draw(app: &mut App, points: &[(f64, f64)]) -> Option<String> {
    for (x, y) in points {
        assert_eq!(click(app, pt(*x, *y)), None);
    }
    enter(app)
}

/// The maintainer's review of 2026-11-23: a path drawn from or to the end
/// of an open path is that path, carried on; one that brings the ends
/// together closes it. Nothing has to be joined afterwards with Edit
/// nodes. Each drawing is still one undo step.
#[test]
fn a_path_drawn_from_an_end_carries_the_path_on() {
    let mut app = armed(Drawing::Line);
    assert_eq!(
        draw(&mut app, &[(0.0, 0.0), (40.0, 0.0)]).as_deref(),
        Some("Draw “Line 1”")
    );
    // From its end: the same shape, one point longer.
    let label = draw(&mut app, &[(40.0, 0.0), (40.0, 30.0)]);
    assert_eq!(label.as_deref(), Some("Extend “Line 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert_eq!(app.project().definitions.len(), 1);
    assert_eq!(last(&app).0.name, "Line 1");
    assert!(!outline(&app).is_closed());
    assert_vertices(
        outline(&app),
        &[(0.0, 0.0, 0.0), (40.0, 0.0, 0.0), (40.0, 30.0, 0.0)],
    );
    // To its start, drawn the other way: still the one shape.
    let label = draw(&mut app, &[(-20.0, 10.0), (0.0, 0.0)]);
    assert_eq!(label.as_deref(), Some("Extend “Line 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert_eq!(outline(&app).vertices().len(), 4);
    assert!(!outline(&app).is_closed());
    // From one end to the other: the shape closes.
    let label = draw(&mut app, &[(40.0, 30.0), (-20.0, 10.0)]);
    assert_eq!(label.as_deref(), Some("Close “Line 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 4);
    assert_eq!(outline_problems(&app), 0);
    // One undo step for the drawing and the joining of it.
    app.undo();
    assert_eq!(app.project().instances.len(), 1);
    assert!(!outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 4);
    // The file holds it.
    app.redo();
    let saved = app.serialize().unwrap();
    assert_eq!(
        &dc_project::load(saved.as_bytes()).unwrap().project,
        app.project()
    );
}

/// A path drawn between the ends of two open paths makes the three one
/// shape, named and listed as the one its first point was put on.
#[test]
fn a_path_drawn_between_two_ends_joins_them() {
    let mut app = armed(Drawing::Line);
    draw(&mut app, &[(0.0, 0.0), (40.0, 0.0)]);
    draw(&mut app, &[(40.0, 30.0), (0.0, 30.0)]);
    assert_eq!(app.project().instances.len(), 2);
    let label = draw(&mut app, &[(40.0, 30.0), (40.0, 0.0)]);
    assert_eq!(label.as_deref(), Some("Extend “Line 2”"));
    assert_eq!(app.project().instances.len(), 1);
    assert_eq!(last(&app).0.name, "Line 2");
    assert_eq!(outline(&app).vertices().len(), 4);
    assert!(!outline(&app).is_closed());
    // And the last side closes the four.
    let label = draw(&mut app, &[(0.0, 0.0), (0.0, 30.0)]);
    assert_eq!(label.as_deref(), Some("Close “Line 2”"));
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 4);
    assert!((outline(&app).signed_area_mm2().abs() - 1200.0).abs() < 1e-9);
}

/// A curve carried on keeps its curves: the shape is still a path with
/// cubic spans, and its outline is the one they give.
#[test]
fn a_spline_carried_on_keeps_its_curves() {
    let mut app = armed(Drawing::Spline);
    draw(&mut app, &[(0.0, 0.0), (20.0, 15.0), (40.0, 0.0)]);
    app.arm(Tool::Draw(Drawing::Line));
    let label = draw(&mut app, &[(40.0, 0.0), (60.0, 0.0)]);
    assert_eq!(label.as_deref(), Some("Extend “Spline 1”"));
    assert_eq!(app.project().instances.len(), 1);
    let (_, definition) = last(&app);
    let Some(Shape::Path { paths }) = &definition.shape else {
        panic!("the curves are kept: {:?}", definition.shape);
    };
    assert_eq!(paths.len(), 1);
    assert!(paths[0].has_curves());
    assert!(!paths[0].is_closed());
    assert_eq!(outline_problems(&app), 0);
}

/// What a drawn path leaves alone: a closed shape whose corner it starts
/// on, the middle of an open path, a locked path, and a path that is
/// construction geometry when the drawn one is not (and the other way).
#[test]
fn a_path_joins_only_an_open_end_of_its_own_kind() {
    let mut app = armed(Drawing::Line);
    // A closed triangle.
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    click(&mut app, pt(40.0, 30.0));
    assert!(click(&mut app, pt(0.0, 0.0)).is_some());
    assert_eq!(
        draw(&mut app, &[(40.0, 0.0), (80.0, 0.0)]).as_deref(),
        Some("Draw “Line 2”"),
        "a closed shape's corner is no end"
    );
    // The middle of the open line.
    assert_eq!(
        draw(&mut app, &[(60.0, 0.0), (60.0, 20.0)]).as_deref(),
        Some("Draw “Line 3”")
    );
    assert_eq!(app.project().instances.len(), 3);
    // A locked path's end.
    let line = app.project().instances[1].id.clone();
    app.run(dc_app::Command::SetInstanceFlags {
        ids: vec![line.clone()],
        visible: None,
        locked: Some(true),
    })
    .unwrap();
    assert_eq!(
        draw(&mut app, &[(80.0, 0.0), (90.0, 10.0)]).as_deref(),
        Some("Draw “Line 4”")
    );
    app.run(dc_app::Command::SetInstanceFlags {
        ids: vec![line],
        visible: None,
        locked: Some(false),
    })
    .unwrap();
    // Construction geometry is not carried on by a line to be cut.
    app.tool_event(Event::Option(dc_app::tool::ToolOption::Construction(true)));
    assert_eq!(
        draw(&mut app, &[(60.0, 20.0), (70.0, 30.0)]).as_deref(),
        Some("Draw “Line 5”")
    );
    assert_eq!(app.project().instances.len(), 5);
}

/// Draws a lone arc from `from` to `to` through `through`, with the Arc
/// tool, and finishes it; the tool in hand before is armed again.
fn draw_arc(
    app: &mut App,
    from: (f64, f64),
    to: (f64, f64),
    through: (f64, f64),
) -> Option<String> {
    let before = app.tool();
    app.arm(Tool::Draw(Drawing::Arc));
    for (x, y) in [from, to, through] {
        assert_eq!(click(app, pt(x, y)), None);
    }
    let label = enter(app);
    app.arm(before);
    label
}

/// The maintainer's report of 2026-11-24: an arc and a line drawn end to
/// end were left two shapes, where a line and a spline joined. An arc is
/// carried on by a line and a line by an arc, and a line across an arc's
/// two ends closes it: a D, of two nodes, an arc and a straight side.
#[test]
fn an_arc_and_a_line_are_carried_on_and_closed() {
    // An arc, then a line from its end.
    let mut app = armed(Drawing::Line);
    assert_eq!(
        draw_arc(&mut app, (0.0, 0.0), (40.0, 0.0), (20.0, 20.0)).as_deref(),
        Some("Draw “Arc 1”")
    );
    let label = draw(&mut app, &[(40.0, 0.0), (60.0, -10.0)]);
    assert_eq!(label.as_deref(), Some("Extend “Arc 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert_eq!(outline(&app).vertices().len(), 3);

    // A line, then an arc from its end.
    let mut app = armed(Drawing::Line);
    draw(&mut app, &[(0.0, 0.0), (40.0, 0.0)]);
    let label = draw_arc(&mut app, (40.0, 0.0), (80.0, 0.0), (60.0, 20.0));
    assert_eq!(label.as_deref(), Some("Extend “Line 1”"));
    assert_eq!(app.project().instances.len(), 1);

    // An arc, then a line across its two ends: a closed D.
    let mut app = armed(Drawing::Line);
    draw_arc(&mut app, (0.0, 0.0), (40.0, 0.0), (20.0, 20.0));
    let label = draw(&mut app, &[(40.0, 0.0), (0.0, 0.0)]);
    assert_eq!(label.as_deref(), Some("Close “Arc 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 2);
    // Half a disc of radius 20.
    let area = outline(&app).signed_area_mm2().abs();
    assert!((area - std::f64::consts::PI * 200.0).abs() < 1e-6, "{area}");
    assert_eq!(outline_problems(&app), 0);

    // A line, then an arc across its two ends: the same D.
    let mut app = armed(Drawing::Line);
    draw(&mut app, &[(0.0, 0.0), (40.0, 0.0)]);
    let label = draw_arc(&mut app, (40.0, 0.0), (0.0, 0.0), (20.0, 20.0));
    assert_eq!(label.as_deref(), Some("Close “Line 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert!(outline(&app).is_closed());

    // Two arcs end to end, and closed: a circle of two arcs.
    let mut app = armed(Drawing::Line);
    draw_arc(&mut app, (0.0, 0.0), (40.0, 0.0), (20.0, 20.0));
    let label = draw_arc(&mut app, (40.0, 0.0), (0.0, 0.0), (20.0, -20.0));
    assert_eq!(label.as_deref(), Some("Close “Arc 1”"));
    assert_eq!(app.project().instances.len(), 1);
    assert!(outline(&app).is_closed());
}

/// The same within one drawing: an arc, then a click back on the first
/// point with the Line tool, closes a D of two points; and a line, then
/// an arc that ends on the first point. Two points joined by straight
/// spans alone still close nothing.
#[test]
fn an_arc_and_a_line_close_in_one_drawing() {
    let mut app = armed(Drawing::Arc);
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    assert_eq!(click(&mut app, pt(20.0, 20.0)), None);
    app.arm(Tool::Draw(Drawing::Line));
    let label = click(&mut app, pt(0.0, 0.0));
    assert_eq!(label.as_deref(), Some("Draw “Path 1”"));
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 2);
    let area = outline(&app).signed_area_mm2().abs();
    assert!((area - std::f64::consts::PI * 200.0).abs() < 1e-6, "{area}");

    // A line, then an arc back to its start.
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(0.0, 50.0));
    click(&mut app, pt(40.0, 50.0));
    app.arm(Tool::Draw(Drawing::Arc));
    assert_eq!(click(&mut app, pt(0.0, 50.0)), None, "the arc's end");
    let label = click(&mut app, pt(20.0, 70.0));
    assert_eq!(label.as_deref(), Some("Draw “Path 1”"));
    assert!(outline(&app).is_closed());
    assert_eq!(outline(&app).vertices().len(), 2);

    // Two points and straight spans: a click on the first is no close.
    let mut app = armed(Drawing::Line);
    click(&mut app, pt(0.0, 0.0));
    click(&mut app, pt(40.0, 0.0));
    assert_eq!(click(&mut app, pt(0.0, 0.0)), None);
    assert!(app.project().instances.is_empty());
}
