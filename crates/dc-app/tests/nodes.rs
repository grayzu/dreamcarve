// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/editInPlace.test.ts

//! Slice 10 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Edit nodes"): the Edit nodes tool, driven as the window drives it,
//! with pointer positions in project millimetres and no window
//! (functional capability 92; acceptance 117).
//!
//! `editInPlace.test.ts`'s case of a node moved through a placed copy is
//! the port; the rest is written from the behaviour the functional spec
//! takes from `moveFeatureControl`, upstream having no test that drives
//! a drag. The default camera is 5 px to the millimetre, so a node is
//! taken hold of within 1.8 mm and a press is a drag past 0.8 mm.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::tool::{Control, Event, Key, NodeAction, NodeRef, SizeField};
use dc_app::{App, Command, Pointer, Selection, Tool};
use dc_geometry::{HandleSide, Path, PathNode, Point, Polyline, Span, Transform, Vertex};
use dc_project::{
    FeatureDefinition, FeatureInstance, InstanceId, PlacedTab, Shape, TabId, TabShape,
};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A plain outline named `name`, placed where it lies.
fn outline(name: &str, points: &[(f64, f64)], closed: bool) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            closed,
        )],
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    }
}

/// A 40 × 30 rectangle of four lines from (10, 10), as a converted
/// rectangle is.
fn square() -> NewShape {
    outline(
        "Square",
        &[(10.0, 10.0), (50.0, 10.0), (50.0, 40.0), (10.0, 40.0)],
        true,
    )
}

/// A blank project with `shapes`, snapping off, the first selected and
/// the Edit nodes tool armed on it.
fn editing(shapes: Vec<NewShape>) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.prefs_mut().snap.enabled = false;
    app.run(Command::AddShapes(shapes)).unwrap();
    let first = app.project().instances[0].id.clone();
    app.select(Selection::Instances(vec![first]));
    app.arm(Tool::Nodes);
    app
}

fn press(app: &mut App, at: Point) -> dc_app::tool::Reply {
    app.tool_event(Event::Move(Pointer::at(at)));
    app.tool_event(Event::Press(Pointer::at(at)))
}

fn release(app: &mut App, at: Point) -> dc_app::tool::Reply {
    app.tool_event(Event::Release(Pointer::at(at)))
}

/// A click: the button down and up in one place.
fn click(app: &mut App, at: Point) {
    press(app, at);
    release(app, at);
}

fn shift_click(app: &mut App, at: Point) {
    let pointer = Pointer {
        shift: true,
        ..Pointer::at(at)
    };
    app.tool_event(Event::Move(pointer));
    app.tool_event(Event::Press(pointer));
    app.tool_event(Event::Release(pointer));
}

/// A drag from `from` to `to`; the undo label of what the release
/// committed.
fn drag(app: &mut App, from: Point, to: Point) -> Option<String> {
    press(app, from);
    app.tool_event(Event::Move(Pointer::at(to)));
    release(app, to).ran
}

fn selected(app: &App) -> Vec<usize> {
    app.selected_nodes().iter().map(|n| n.node).collect()
}

fn instance<'a>(app: &'a App, name: &str) -> &'a FeatureInstance {
    app.project()
        .instances
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("no shape {name}"))
}

fn definition<'a>(app: &'a App, name: &str) -> &'a FeatureDefinition {
    &app.project().definitions[&instance(app, name).definition]
}

/// The vertices of `name`'s one outline, in definition space.
fn vertices(app: &App, name: &str) -> Vec<(f64, f64)> {
    let definition = definition(app, name);
    assert_eq!(definition.profiles.len(), 1);
    definition.profiles[0]
        .vertices()
        .iter()
        .map(|v| (v.x, v.y))
        .collect()
}

#[track_caller]
fn assert_points(got: &[(f64, f64)], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9,
            "{got:?}, want {want:?}"
        );
    }
}

/// The tool bar's rows that are sizes of what is selected, less the
/// corner size, which is the tool's option and always there.
fn sizes(app: &App) -> Vec<(SizeField, String)> {
    app.tool_bar()
        .unwrap()
        .fields
        .into_iter()
        .filter(|row| !row.option)
        .map(|row| (row.field, row.text))
        .collect()
}

/// The tool bar's button `label`.
fn action(app: &App, label: &str) -> dc_app::tool::BarAction {
    app.tool_bar()
        .unwrap()
        .actions
        .into_iter()
        .find(|action| action.label == label)
        .unwrap_or_else(|| panic!("no {label} button"))
}

fn steps(app: &App) -> usize {
    app.history().undo_labels().count()
}

/// Capability 92, Select: the selected path shows its nodes; a click
/// selects one, `Shift` adds and takes away, a click on empty stock
/// clears; the one selected node's X and Y are in the tool bar.
#[test]
fn a_click_selects_a_node() {
    let mut app = editing(vec![square()]);
    assert_eq!(app.node_editing(), Some(InstanceId::new("i1")));
    let marks = app.draft().nodes.clone();
    assert_eq!(marks.len(), 4);
    assert!(marks.iter().all(|mark| !mark.selected && !mark.smooth));
    assert_eq!(marks[1].at_mm, pt(50.0, 10.0));
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "Drag a node or a handle; double-click a segment to add a node"
    );
    assert!(sizes(&app).is_empty() && bar.can_finish);

    // What the pointer takes hold of: a node within 9 px, then a span.
    assert_eq!(
        app.node_control_at(pt(51.5, 10.5)),
        Some(Control::Node(NodeRef { path: 0, node: 1 }))
    );
    assert!(matches!(
        app.node_control_at(pt(30.0, 11.0)),
        Some(Control::Span { path: 0, span: 0, t }) if (t - 0.5).abs() < 1e-9
    ));
    assert_eq!(app.node_control_at(pt(30.0, 25.0)), None);

    let before = steps(&app);
    click(&mut app, pt(50.5, 10.0));
    assert_eq!(selected(&app), [1]);
    assert!(app.draft().nodes[1].selected);
    assert_eq!(
        sizes(&app),
        [
            (SizeField::X, "50".to_owned()),
            (SizeField::Y, "10".to_owned())
        ]
    );
    shift_click(&mut app, pt(50.0, 40.0));
    assert_eq!(selected(&app), [1, 2]);
    assert!(sizes(&app).is_empty(), "two nodes");
    shift_click(&mut app, pt(50.0, 10.0));
    assert_eq!(selected(&app), [2], "Shift takes one away");
    click(&mut app, pt(10.0, 10.0));
    assert_eq!(selected(&app), [0], "a plain click selects it alone");
    click(&mut app, pt(80.0, 80.0));
    assert!(selected(&app).is_empty());
    app.tool_event(Event::Node(NodeAction::SelectAll));
    assert_eq!(selected(&app), [0, 1, 2, 3]);
    assert_eq!(steps(&app), before, "selecting is no undo step");
    assert_eq!(app.tool(), Tool::Nodes);
}

/// Capability 92, Select: a box dragged from empty stock selects the
/// nodes in it; with `Shift` it adds to what was selected.
#[test]
fn a_box_selects_several() {
    let mut app = editing(vec![square()]);
    press(&mut app, pt(45.0, 5.0));
    app.tool_event(Event::Move(Pointer::at(pt(55.0, 45.0))));
    assert_eq!(
        app.draft().marquee,
        Some((pt(45.0, 5.0), pt(55.0, 45.0))),
        "the box shows while it is dragged"
    );
    release(&mut app, pt(55.0, 45.0));
    assert_eq!(app.draft().marquee, None);
    assert_eq!(selected(&app), [1, 2]);

    let boxed = Pointer {
        shift: true,
        ..Pointer::at(pt(5.0, 35.0))
    };
    app.tool_event(Event::Press(boxed));
    let far = Pointer {
        shift: true,
        ..Pointer::at(pt(15.0, 45.0))
    };
    app.tool_event(Event::Move(far));
    app.tool_event(Event::Release(far));
    assert_eq!(selected(&app), [1, 2, 3], "Shift adds");
    // The two right-hand nodes move together.
    let label = drag(&mut app, pt(50.0, 10.0), pt(60.0, 10.0));
    assert_eq!(label.as_deref(), Some("Move 3 nodes of “Square”"));
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (60.0, 10.0), (60.0, 40.0), (20.0, 40.0)],
    );
}

/// Acceptance 117: dragging a corner node moves that vertex and nothing
/// else, previewed while it is dragged and one undo step on release;
/// dragging a straight side moves its two nodes; a click moves nothing.
#[test]
fn dragging_moves_one_vertex() {
    let mut app = editing(vec![square()]);
    let before = steps(&app);
    press(&mut app, pt(50.0, 10.0));
    app.tool_event(Event::Move(Pointer::at(pt(58.0, 6.0))));
    // Not yet a command: the outline as the release will leave it.
    assert_eq!(steps(&app), before);
    assert_eq!(app.draft().outlines.len(), 1);
    assert_eq!(
        app.draft().outlines[0].vertices()[1],
        Vertex::line(58.0, 6.0)
    );
    assert_eq!(app.draft().nodes[1].at_mm, pt(58.0, 6.0));
    let reply = release(&mut app, pt(58.0, 6.0));
    assert_eq!(reply.ran.as_deref(), Some("Move node of “Square”"));
    assert_eq!(steps(&app), before + 1);
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (58.0, 6.0), (50.0, 40.0), (10.0, 40.0)],
    );
    assert!(app.draft().outlines.is_empty(), "the preview is over");
    assert_eq!(definition(&app, "Square").shape, None, "lines stay lines");
    assert_eq!(selected(&app), [1], "the node stays selected");
    app.undo();
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (50.0, 10.0), (50.0, 40.0), (10.0, 40.0)],
    );

    // A press and a release in one place is a click: nothing moves.
    click(&mut app, pt(50.0, 10.0));
    assert_eq!(steps(&app), before);

    // The top side, dragged up 5: its two nodes go with it.
    let label = drag(&mut app, pt(30.0, 40.0), pt(30.0, 45.0));
    assert_eq!(label.as_deref(), Some("Move 2 nodes of “Square”"));
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (50.0, 10.0), (50.0, 45.0), (10.0, 45.0)],
    );

    // The arrows nudge the selected nodes, and a typed X places one.
    click(&mut app, pt(10.0, 10.0));
    app.tool_event(Event::Node(NodeAction::Nudge {
        dx_mm: -1.0,
        dy_mm: 0.0,
    }));
    assert_eq!(vertices(&app, "Square")[0], (9.0, 10.0));
    let reply = app.tool_event(Event::Field {
        field: SizeField::X,
        text: "12.5".to_owned(),
    });
    assert_eq!(reply.ran.as_deref(), Some("Move node of “Square”"));
    assert_eq!(vertices(&app, "Square")[0], (12.5, 10.0));
    app.tool_event(Event::Field {
        field: SizeField::Y,
        text: "wide".to_owned(),
    });
    let y = &app.tool_bar().unwrap().fields[1];
    assert!(y.problem.is_some(), "{y:?}");
}

/// `editInPlace.test.ts`, "moving an anchor of a transformed instance":
/// a node dragged on a placed linked copy lands under the pointer, and
/// the edit is the definition's, so the other copy changes by the same
/// in its own place. The same through a copy that is turned and scaled.
#[test]
fn editing_through_a_placed_copy_lands_under_the_pointer() {
    // Upstream's rectangle: 30 × 15 from (10, 20).
    let rect = outline(
        "Rect",
        &[(10.0, 20.0), (40.0, 20.0), (40.0, 35.0), (10.0, 35.0)],
        true,
    );
    let mut app = editing(vec![rect]);
    app.run(Command::DuplicateInstances(vec![InstanceId::new("i1")]))
        .unwrap();
    let copy = app.project().instances[1].id.clone();
    // Duplicate offsets by (10, 10): on to upstream's (80, 40).
    app.run(Command::TransformInstances {
        ids: vec![copy.clone()],
        delta: Transform::translate(70.0, 30.0),
    })
    .unwrap();
    app.select(Selection::Instances(vec![copy.clone()]));
    app.arm(Tool::Nodes);
    assert_eq!(app.draft().nodes[0].at_mm, pt(90.0, 60.0));

    let label = drag(&mut app, pt(90.0, 60.0), pt(95.0, 65.0));
    assert_eq!(label.as_deref(), Some("Move node of “Rect (2)”"));
    // The copy's node is where it was dropped; the original's has moved
    // the same in its own place: (95 − 80, 65 − 40).
    assert_eq!(app.draft().nodes[0].at_mm, pt(95.0, 65.0));
    assert_eq!(vertices(&app, "Rect")[0], (15.0, 25.0));
    assert_eq!(
        instance(&app, "Rect").definition,
        instance(&app, "Rect (2)").definition
    );

    // Turned a quarter and doubled, the node still lands under the
    // pointer.
    app.run(Command::TransformInstances {
        ids: vec![copy.clone()],
        delta: Transform::rotate(std::f64::consts::FRAC_PI_2).then(Transform::scale(2.0, 2.0)),
    })
    .unwrap();
    app.arm(Tool::Nodes);
    let at = app.draft().nodes[2].at_mm;
    let to = pt(at.x + 6.0, at.y - 4.0);
    drag(&mut app, at, to);
    let landed = app.draft().nodes[2].at_mm;
    assert!(
        (landed.x - to.x).abs() < 1e-9 && (landed.y - to.y).abs() < 1e-9,
        "{landed:?}"
    );
}

/// Capability 92, Arcs: an arc shows a handle at its middle; dragging it
/// changes the bulge and leaves the ends; the selected arc's radius is
/// in the tool bar and can be typed.
#[test]
fn an_arc_handle_changes_the_bulge_only() {
    // From (10, 10) to (50, 10), half a circle below the chord.
    let mut arc = outline("Arc", &[(10.0, 10.0), (50.0, 10.0)], false);
    arc.profiles = vec![Polyline::new(
        vec![
            Vertex {
                x: 10.0,
                y: 10.0,
                bulge: 1.0,
            },
            Vertex::line(50.0, 10.0),
        ],
        false,
    )];
    // Room below the stock's bottom edge is not needed: the camera shows
    // it all the same.
    let mut app = editing(vec![arc]);
    let handles = app.draft().handles.clone();
    assert_eq!(handles.len(), 1);
    assert!((handles[0].at_mm.x - 30.0).abs() < 1e-9 && (handles[0].at_mm.y + 10.0).abs() < 1e-9);
    assert_eq!(handles[0].from_mm, None, "an arc's middle has no stem");
    assert_eq!(
        app.node_control_at(pt(30.0, -10.0)),
        Some(Control::ArcMiddle { path: 0, span: 0 })
    );

    let label = drag(&mut app, pt(30.0, -10.0), pt(30.0, 0.0));
    assert_eq!(label.as_deref(), Some("Reshape “Arc”"));
    let outline = &definition(&app, "Arc").profiles[0];
    let v = outline.vertices();
    assert_eq!((v[0].x, v[0].y, v[1].x, v[1].y), (10.0, 10.0, 50.0, 10.0));
    // It rises 10 over a half chord of 20.
    assert!((v[0].bulge - 0.5).abs() < 1e-9, "{v:?}");

    // The arc, clicked away from its middle and its ends, is selected:
    // its radius shows, and takes a typed value.
    let on_arc = outline.point_at(outline.length_mm() / 4.0).unwrap().0;
    click(&mut app, on_arc);
    let radius = &app.tool_bar().unwrap().fields[0];
    assert_eq!(
        (radius.field, radius.text.as_str()),
        (SizeField::Radius, "25")
    );
    assert_eq!(app.draft().highlights.len(), 1, "the selected span shows");
    let reply = app.tool_event(Event::Field {
        field: SizeField::Radius,
        text: "40".to_owned(),
    });
    assert_eq!(reply.ran.as_deref(), Some("Reshape “Arc”"));
    let arc = definition(&app, "Arc").profiles[0]
        .segments()
        .next()
        .unwrap()
        .arc()
        .unwrap();
    assert!((arc.radius_mm - 40.0).abs() < 1e-9);
    // Under half the chord there is no such arc.
    app.tool_event(Event::Field {
        field: SizeField::Radius,
        text: "5".to_owned(),
    });
    assert!(app.tool_bar().unwrap().fields[0].problem.is_some());
}

/// Capability 92, Curves: a curve's node shows its two handles. Dragging
/// one keeps the other in line with it, each its own length; with `Alt`
/// it moves alone and the node becomes a corner. The path keeps its
/// curves (ADR-0006) and its outline is flattened from them.
#[test]
fn a_smooth_handle_carries_its_twin_alt_does_not() {
    let points = [pt(10.0, 10.0), pt(30.0, 30.0), pt(50.0, 10.0)];
    let path = Path::spline(&points, false);
    let curve = NewShape {
        name: "Curve".to_owned(),
        profiles: vec![path.to_polyline(0.01)],
        shape: Some(Shape::Path {
            paths: vec![path.clone()],
        }),
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    };
    let mut app = editing(vec![curve]);
    let nodes = |app: &App| -> Vec<PathNode> {
        let Some(Shape::Path { paths }) = &definition(app, "Curve").shape else {
            panic!("the path keeps its curves");
        };
        paths[0].nodes().to_vec()
    };
    let handles = |node: &PathNode, before: &PathNode| -> (Point, Point) {
        let (Span::Cubic { handle_out, .. }, Span::Cubic { handle_in, .. }) =
            (node.span, before.span)
        else {
            panic!("cubics either side");
        };
        (handle_out, handle_in)
    };
    assert_eq!(app.draft().handles.len(), 4, "two spans, two handles each");
    assert!(app.draft().nodes[1].smooth);
    let (out, into) = handles(&nodes(&app)[1], &nodes(&app)[0]);
    assert_eq!(
        app.node_control_at(out),
        Some(Control::Handle {
            node: NodeRef { path: 0, node: 1 },
            side: HandleSide::Out
        })
    );
    let in_length = into.distance_to(points[1]);

    // The out handle, dragged up: the in handle turns to stay in line.
    let to = pt(out.x, out.y + 6.0);
    let label = drag(&mut app, out, to);
    assert_eq!(label.as_deref(), Some("Reshape “Curve”"));
    let (out_now, into_now) = handles(&nodes(&app)[1], &nodes(&app)[0]);
    assert!((out_now.x - to.x).abs() < 1e-9 && (out_now.y - to.y).abs() < 1e-9);
    let (a, b) = (out_now - points[1], into_now - points[1]);
    assert!(
        a.cross(b).abs() < 1e-9 && a.dot(b) < 0.0,
        "in line, opposite"
    );
    assert!((b.length() - in_length).abs() < 1e-9, "its own length");
    assert!(nodes(&app)[1].smooth);
    // The outline is the curve's, flattened.
    let definition_now = definition(&app, "Curve");
    let Some(shape) = &definition_now.shape else {
        panic!("a path");
    };
    assert_eq!(
        shape.outline_matches(&definition_now.profiles, 0.01),
        Some(true)
    );

    // With Alt the handle moves alone, and the node is a corner.
    let alone = pt(out_now.x + 5.0, out_now.y);
    let held = |at: Point| Pointer {
        alt: true,
        ..Pointer::at(at)
    };
    app.tool_event(Event::Press(held(out_now)));
    app.tool_event(Event::Move(held(alone)));
    app.tool_event(Event::Release(held(alone)));
    let (_, into_after) = handles(&nodes(&app)[1], &nodes(&app)[0]);
    assert_eq!(into_after, into_now, "the other handle stays");
    assert!(!nodes(&app)[1].smooth);
    assert!(!app.draft().nodes[1].smooth);
}

/// Acceptance 117: adding a node at a side's middle leaves the outline's
/// length unchanged; deleting it restores the four vertices. `Ctrl`/`⌘`
/// on a span adds one too. An outline is never left with fewer than
/// three nodes closed, or two open.
#[test]
fn add_and_delete_a_node() {
    let mut app = editing(vec![square()]);
    let length = definition(&app, "Square").profiles[0].length_mm();
    click(&mut app, pt(30.0, 10.0));
    click(&mut app, pt(30.0, 10.0));
    let reply = app.tool_event(Event::DoubleClick(Pointer::at(pt(30.0, 10.0))));
    assert_eq!(reply.ran.as_deref(), Some("Add node to “Square”"));
    assert_points(
        &vertices(&app, "Square"),
        &[
            (10.0, 10.0),
            (30.0, 10.0),
            (50.0, 10.0),
            (50.0, 40.0),
            (10.0, 40.0),
        ],
    );
    assert!((definition(&app, "Square").profiles[0].length_mm() - length).abs() < 1e-9);
    assert_eq!(selected(&app), [1], "the new node is selected");

    let reply = app.tool_event(Event::Key(Key::Delete));
    assert_eq!(reply.ran.as_deref(), Some("Delete node of “Square”"));
    assert_eq!(vertices(&app, "Square").len(), 4);
    assert!(selected(&app).is_empty());

    // Ctrl or ⌘ and a click on a span.
    let held = Pointer {
        command: true,
        ..Pointer::at(pt(50.0, 25.0))
    };
    let reply = app.tool_event(Event::Press(held));
    assert_eq!(reply.ran.as_deref(), Some("Add node to “Square”"));
    app.tool_event(Event::Release(held));
    assert_eq!(vertices(&app, "Square")[2], (50.0, 25.0));

    // Two at once, then down to a triangle, which keeps its three.
    app.tool_event(Event::Node(NodeAction::SelectAll));
    shift_click(&mut app, pt(10.0, 10.0));
    shift_click(&mut app, pt(50.0, 10.0));
    shift_click(&mut app, pt(10.0, 40.0));
    assert_eq!(selected(&app), [2, 3]);
    let reply = app.tool_event(Event::Key(Key::Backspace));
    assert_eq!(reply.ran.as_deref(), Some("Delete 2 nodes of “Square”"));
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (50.0, 10.0), (10.0, 40.0)],
    );
    click(&mut app, pt(10.0, 10.0));
    let reply = app.tool_event(Event::Key(Key::Delete));
    assert_eq!(reply.ran, None);
    assert_eq!(
        reply.refused.as_deref(),
        Some("An outline keeps at least two nodes, three when it is closed.")
    );
    assert_eq!(vertices(&app, "Square").len(), 3);
}

/// Acceptance 117: Break opens a closed outline at the node, and splits
/// an open one into two shapes; deleting a segment opens a closed
/// outline there, and of an open path makes two shapes. What cut the
/// shape cuts the first part.
#[test]
fn break_and_delete_segment() {
    let line = outline(
        "Line",
        &[(60.0, 10.0), (70.0, 10.0), (80.0, 10.0), (90.0, 10.0)],
        false,
    );
    let mut app = editing(vec![square(), line]);
    assert!(!action(&app, "Break").enabled, "no node yet");
    click(&mut app, pt(50.0, 40.0));
    assert!(action(&app, "Break").enabled);
    let reply = app.tool_event(Event::Node(NodeAction::Break));
    assert_eq!(reply.ran.as_deref(), Some("Break “Square”"));
    let opened = &definition(&app, "Square").profiles[0];
    assert!(!opened.is_closed());
    // It starts at the node and ends at a second node in the same place.
    assert_points(
        &vertices(&app, "Square"),
        &[
            (50.0, 40.0),
            (10.0, 40.0),
            (10.0, 10.0),
            (50.0, 10.0),
            (50.0, 40.0),
        ],
    );
    assert_eq!(app.project().instances.len(), 2, "still one shape");
    // At its end there is nothing to break, and one node is needed.
    click(&mut app, pt(10.0, 40.0));
    shift_click(&mut app, pt(10.0, 10.0));
    let reply = app.tool_event(Event::Node(NodeAction::Break));
    assert_eq!(
        reply.refused.as_deref(),
        Some("Select the one node to break the path at.")
    );

    // The open line, broken at its second node: two shapes.
    app.select(Selection::Instances(vec![
        instance(&app, "Line").id.clone(),
    ]));
    app.arm(Tool::Nodes);
    click(&mut app, pt(60.0, 10.0));
    let reply = app.tool_event(Event::Node(NodeAction::Break));
    assert_eq!(
        reply.refused.as_deref(),
        Some("A path cannot be broken at its end.")
    );
    click(&mut app, pt(70.0, 10.0));
    let reply = app.tool_event(Event::Node(NodeAction::Break));
    assert_eq!(reply.ran.as_deref(), Some("Break “Line”"));
    assert_eq!(app.project().instances.len(), 3);
    assert_points(&vertices(&app, "Line"), &[(60.0, 10.0), (70.0, 10.0)]);
    assert_points(
        &vertices(&app, "Line (2)"),
        &[(70.0, 10.0), (80.0, 10.0), (90.0, 10.0)],
    );
    // One undo step puts the line back.
    app.undo();
    assert_eq!(vertices(&app, "Line").len(), 4);

    // Its middle segment deleted: two shapes, the gap between them.
    app.select(Selection::Instances(vec![
        instance(&app, "Line").id.clone(),
    ]));
    app.arm(Tool::Nodes);
    click(&mut app, pt(75.0, 10.0));
    assert_eq!(app.draft().highlights.len(), 1);
    let reply = app.tool_event(Event::Key(Key::Delete));
    assert_eq!(reply.ran.as_deref(), Some("Delete segment of “Line”"));
    assert_points(&vertices(&app, "Line"), &[(60.0, 10.0), (70.0, 10.0)]);
    assert_points(&vertices(&app, "Line (2)"), &[(80.0, 10.0), (90.0, 10.0)]);

    // A closed outline with a segment deleted opens there.
    let mut app = editing(vec![square()]);
    click(&mut app, pt(30.0, 10.0));
    let reply = app.tool_event(Event::Key(Key::Delete));
    assert_eq!(reply.ran.as_deref(), Some("Delete segment of “Square”"));
    let opened = &definition(&app, "Square").profiles[0];
    assert!(!opened.is_closed());
    assert_points(
        &vertices(&app, "Square"),
        &[(50.0, 10.0), (50.0, 40.0), (10.0, 40.0), (10.0, 10.0)],
    );
}

/// Capability 92, Join ends: an open path's end dropped on its own other
/// end closes it; dropped on another open path's end, within 14 px, the
/// two become one shape.
#[test]
fn dropping_an_end_on_an_end_joins() {
    let hook = outline("Hook", &[(10.0, 10.0), (40.0, 10.0), (40.0, 30.0)], false);
    let tail = outline("Tail", &[(60.0, 30.0), (60.0, 50.0), (80.0, 50.0)], false);
    let mut app = editing(vec![hook, tail]);

    // 2 mm from the tail's start is within 14 px at 5 px to the mm.
    let label = drag(&mut app, pt(40.0, 30.0), pt(58.5, 31.0));
    assert_eq!(label.as_deref(), Some("Join 2 shapes"));
    assert_eq!(app.project().instances.len(), 1);
    assert_points(
        &vertices(&app, "Hook"),
        &[
            (10.0, 10.0),
            (40.0, 10.0),
            (60.0, 30.0),
            (60.0, 50.0),
            (80.0, 50.0),
        ],
    );
    assert!(!definition(&app, "Hook").profiles[0].is_closed());
    // What was made is selected, and still edited.
    assert_eq!(app.draft().nodes.len(), 5);

    // Its last end dropped on its first: the outline closes, the two
    // ends one node.
    let label = drag(&mut app, pt(80.0, 50.0), pt(11.0, 11.0));
    assert_eq!(label.as_deref(), Some("Close “Hook”"));
    let closed = &definition(&app, "Hook").profiles[0];
    assert!(closed.is_closed());
    assert_points(
        &vertices(&app, "Hook"),
        &[(10.0, 10.0), (40.0, 10.0), (60.0, 30.0), (60.0, 50.0)],
    );

    // Dropped away from any end, an end just moves.
    let lone = outline("Lone", &[(10.0, 10.0), (40.0, 10.0)], false);
    let other = outline("Other", &[(60.0, 30.0), (60.0, 50.0)], false);
    let mut app = editing(vec![lone, other]);
    let label = drag(&mut app, pt(40.0, 10.0), pt(50.0, 20.0));
    assert_eq!(label.as_deref(), Some("Move node of “Lone”"));
    assert_eq!(app.project().instances.len(), 2);
}

/// A tab keeps its place on an outline whose nodes are edited, and goes
/// with the first part of a path that is broken in two.
#[test]
fn tabs_follow_an_edited_outline() {
    let mut app = editing(vec![square()]);
    let tab = PlacedTab {
        id: TabId::new("tab1"),
        instance: InstanceId::new("i1"),
        profile: 0,
        x_mm: 50.0,
        y_mm: 25.0,
        width_mm: 6.0,
        height_mm: 2.0,
        shape: TabShape::Rect,
        holds_slug: None,
    };
    app.run(Command::AddTab(tab)).unwrap();
    app.select(Selection::Instances(vec![InstanceId::new("i1")]));
    app.arm(Tool::Nodes);
    // The right side moved out to x 60: the tab goes with it.
    drag(&mut app, pt(50.0, 25.0), pt(60.0, 25.0));
    let moved = &app.project().tabs[0];
    assert!((moved.x_mm - 60.0).abs() < 1e-9 && (moved.y_mm - 25.0).abs() < 1e-9);
    assert_eq!(moved.instance, InstanceId::new("i1"));
}

/// Capability 92: a shape that keeps its sizes is converted first, at
/// the user's word; `Enter`, `Escape` and a double-click on empty stock
/// leave the tool with the shape selected; a press on another path edits
/// that one.
#[test]
fn a_sized_shape_is_converted_and_the_tool_is_left() {
    let rectangle = Shape::Rectangle {
        width_mm: 40.0,
        height_mm: 30.0,
        corner: dc_project::Corner::Square,
    };
    let sized = NewShape {
        name: "Rectangle 1".to_owned(),
        profiles: rectangle.outline(0.01).unwrap().unwrap(),
        shape: Some(rectangle),
        transform: Transform::translate(30.0, 25.0),
        construction: false,
        folder: None,
    };
    let line = outline("Line", &[(60.0, 60.0), (90.0, 60.0)], false);
    let mut app = editing(vec![sized, line]);
    assert_eq!(app.node_editing(), None);
    assert!(app.draft().nodes.is_empty());
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "“Rectangle 1” keeps its sizes: convert it to a path to edit its nodes"
    );
    assert_eq!(bar.actions.len(), 1);
    assert_eq!(bar.actions[0].label, "Convert to Path");
    assert_eq!(bar.actions[0].action, NodeAction::Convert);
    // A drag on it moves nothing.
    assert_eq!(drag(&mut app, pt(50.0, 40.0), pt(60.0, 50.0)), None);

    let reply = app.tool_event(Event::Node(NodeAction::Convert));
    assert_eq!(
        reply.ran.as_deref(),
        Some("Convert “Rectangle 1” to a path")
    );
    assert_eq!(app.draft().nodes.len(), 4);
    assert_eq!(app.node_editing(), Some(InstanceId::new("i1")));

    // A press on the other path edits that one.
    click(&mut app, pt(75.0, 60.0));
    assert_eq!(app.node_editing(), Some(InstanceId::new("i2")));
    assert_eq!(app.draft().nodes.len(), 2);

    // Leaving: the shape stays selected.
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.selection().instances(), [InstanceId::new("i2")]);
    assert!(app.draft().nodes.is_empty());
    app.arm(Tool::Nodes);
    app.tool_event(Event::Key(Key::Enter));
    assert_eq!(app.tool(), Tool::Select);
    app.arm(Tool::Nodes);
    app.tool_event(Event::DoubleClick(Pointer::at(pt(20.0, 80.0))));
    assert_eq!(app.tool(), Tool::Select, "a double-click on empty stock");

    // With nothing selected the tool waits for a path.
    app.select(Selection::Nothing);
    app.arm(Tool::Nodes);
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click a path to edit its nodes"
    );
    click(&mut app, pt(75.0, 60.0));
    assert_eq!(app.node_editing(), Some(InstanceId::new("i2")));
}

/// Acceptance 118 (ports `profileEdit.test.ts`'s fillet and chamfer
/// through the tool): a 5 mm fillet at the corner of a 40 × 30 rectangle
/// path shortens both sides by 5 and joins them with an arc of radius 5
/// tangent to both; a 5 mm chamfer joins points 5 mm along each side; a
/// size both sides cannot take is refused and named; a corner next to an
/// arc is refused.
#[test]
fn fillet_and_chamfer_a_corner() {
    let mut app = editing(vec![square()]);
    // The size is the tool's option: 5 mm until one is typed.
    let corner = app
        .tool_bar()
        .unwrap()
        .fields
        .into_iter()
        .find(|row| row.field == SizeField::CornerSize)
        .unwrap();
    assert!(corner.option);
    assert_eq!(corner.text, "5");
    assert!(!action(&app, "Fillet").enabled, "no corner selected");
    click(&mut app, pt(50.0, 10.0));
    assert!(action(&app, "Fillet").enabled && action(&app, "Chamfer").enabled);

    let reply = app.tool_event(Event::Node(NodeAction::Fillet));
    assert_eq!(reply.ran.as_deref(), Some("Fillet corner of “Square”"));
    assert_eq!(reply.refused, None);
    assert_points(
        &vertices(&app, "Square"),
        &[
            (10.0, 10.0),
            (45.0, 10.0),
            (50.0, 15.0),
            (50.0, 40.0),
            (10.0, 40.0),
        ],
    );
    let arcs: Vec<_> = definition(&app, "Square").profiles[0]
        .segments()
        .filter_map(|segment| segment.arc())
        .collect();
    assert_eq!(arcs.len(), 1);
    assert!((arcs[0].radius_mm - 5.0).abs() < 1e-9);
    // Tangent to both sides: its centre is 5 from each.
    assert!((arcs[0].centre.x - 45.0).abs() < 1e-9 && (arcs[0].centre.y - 15.0).abs() < 1e-9);
    assert!(selected(&app).is_empty(), "the nodes are renumbered");

    // A chamfer of 8 at the opposite corner, the size typed in the bar.
    app.tool_event(Event::Field {
        field: SizeField::CornerSize,
        text: "8".to_owned(),
    });
    assert_eq!(app.prefs().drawing.corner_mm, Some(8.0));
    click(&mut app, pt(10.0, 40.0));
    let reply = app.tool_event(Event::Node(NodeAction::Chamfer));
    assert_eq!(reply.ran.as_deref(), Some("Chamfer corner of “Square”"));
    assert_points(
        &vertices(&app, "Square"),
        &[
            (10.0, 10.0),
            (45.0, 10.0),
            (50.0, 15.0),
            (50.0, 40.0),
            (18.0, 40.0),
            (10.0, 32.0),
        ],
    );

    // A corner next to the arc, and a size the sides cannot take.
    click(&mut app, pt(45.0, 10.0));
    let reply = app.tool_event(Event::Node(NodeAction::Fillet));
    assert_eq!(reply.ran, None);
    assert_eq!(
        reply.refused.as_deref(),
        Some("a corner next to an arc or a curve cannot be rounded or chamfered")
    );
    app.tool_event(Event::Field {
        field: SizeField::CornerSize,
        text: "40".to_owned(),
    });
    click(&mut app, pt(10.0, 10.0));
    let before = steps(&app);
    let reply = app.tool_event(Event::Node(NodeAction::Chamfer));
    assert_eq!(reply.ran, None);
    // The shorter side at this corner is 22 long, to the chamfer.
    assert_eq!(
        reply.refused.as_deref(),
        Some("the sides are too short: this corner takes less than 22")
    );
    assert_eq!(steps(&app), before);

    // Several at once: those that can are cut, and the reply says one
    // was left.
    let mut app = editing(vec![square()]);
    app.tool_event(Event::Node(NodeAction::SelectAll));
    let reply = app.tool_event(Event::Node(NodeAction::Fillet));
    assert_eq!(reply.ran.as_deref(), Some("Fillet 4 corners of “Square”"));
    assert_eq!(reply.refused, None);
    assert_eq!(vertices(&app, "Square").len(), 8);
    let open = outline("Elbow", &[(10.0, 10.0), (40.0, 10.0), (40.0, 30.0)], false);
    let mut app = editing(vec![open]);
    app.tool_event(Event::Node(NodeAction::SelectAll));
    let reply = app.tool_event(Event::Node(NodeAction::Fillet));
    assert_eq!(reply.ran.as_deref(), Some("Fillet corner of “Elbow”"));
    assert_eq!(
        reply.refused.as_deref(),
        Some("Some corners were left as they are: the end of an open path is not a corner")
    );
    // With nothing selected there is nothing to cut.
    let reply = app.tool_event(Event::Node(NodeAction::Chamfer));
    assert!(reply.ran.is_none() && reply.refused.is_some());
}

/// Acceptance 118: Fillet Corners at 3 on an L-shaped path rounds all
/// six corners, in one undo step; a shape that keeps its sizes is
/// converted first, and a radius no corner can take is refused.
#[test]
fn fillet_corners_of_a_shape() {
    let ell = outline(
        "Ell",
        &[
            (10.0, 10.0),
            (50.0, 10.0),
            (50.0, 25.0),
            (30.0, 25.0),
            (30.0, 50.0),
            (10.0, 50.0),
        ],
        true,
    );
    let mut app = editing(vec![ell]);
    app.arm(Tool::Select);
    let id = InstanceId::new("i1");
    let command = dc_app::edit::fillet_corners(app.project(), app.strings(), &id, "3").unwrap();
    assert_eq!(
        app.run(command).as_deref().ok(),
        Some("Fillet 6 corners of “Ell”")
    );
    let outline_now = &definition(&app, "Ell").profiles[0];
    let arcs: Vec<_> = outline_now.segments().filter_map(|s| s.arc()).collect();
    assert_eq!(arcs.len(), 6);
    assert!(arcs.iter().all(|arc| (arc.radius_mm - 3.0).abs() < 1e-9));
    // Five turn with the outline and the inside corner against it.
    assert_eq!(arcs.iter().filter(|arc| arc.sweep_rad > 0.0).count(), 5);
    assert_eq!(outline_now.segments().count(), 12);
    assert_eq!(definition(&app, "Ell").shape, None);

    // Too large for any corner, no length, and a shape with sizes.
    let refused = |app: &App, text: &str| {
        dc_app::edit::fillet_corners(app.project(), app.strings(), &id, text)
            .expect_err("refused")
            .to_string()
    };
    app.undo();
    // 45 is more than the longest side: the first corner looked at is
    // the one whose shorter side is 20.
    assert_eq!(
        refused(&app, "45"),
        "the sides are too short: this corner takes less than 20"
    );
    assert_eq!(refused(&app, "0"), "a size must be greater than 0");
    assert!(dc_app::edit::fillet_corners(app.project(), app.strings(), &id, "wide").is_err());
    let rectangle = Shape::Rectangle {
        width_mm: 40.0,
        height_mm: 30.0,
        corner: dc_project::Corner::Square,
    };
    let sized = NewShape {
        name: "Rectangle 1".to_owned(),
        profiles: rectangle.outline(0.01).unwrap().unwrap(),
        shape: Some(rectangle),
        transform: Transform::translate(30.0, 25.0),
        construction: false,
        folder: None,
    };
    let app = editing(vec![sized]);
    assert_eq!(
        refused(&app, "3"),
        "“Rectangle 1” keeps its sizes: convert it to a path to edit its nodes"
    );
}

/// Capability 92, Segment kind: the selected segment is made straight,
/// an arc or a curve, its two ends where they were; the bar offers the
/// kinds it is not.
#[test]
fn segment_kinds() {
    let mut app = editing(vec![square()]);
    let reply = app.tool_event(Event::Node(NodeAction::SpanKind(
        dc_geometry::SpanKind::Arc,
    )));
    assert_eq!(
        reply.refused.as_deref(),
        Some("Select a segment to change what it is.")
    );
    click(&mut app, pt(30.0, 10.0));
    assert!(!action(&app, "Make Straight").enabled, "it is straight");
    assert!(action(&app, "Make Arc").enabled && action(&app, "Make Curve").enabled);

    // A line bows out by an eighth of its length, to be dragged.
    let reply = app.tool_event(Event::Node(NodeAction::SpanKind(
        dc_geometry::SpanKind::Arc,
    )));
    assert_eq!(
        reply.ran.as_deref(),
        Some("Make segment of “Square” an arc")
    );
    let v = definition(&app, "Square").profiles[0].vertices().to_vec();
    assert_eq!((v[0].x, v[0].y, v[1].x, v[1].y), (10.0, 10.0, 50.0, 10.0));
    assert!((v[0].bulge.abs() - 0.25).abs() < 1e-9, "{v:?}");
    assert_eq!(definition(&app, "Square").shape, None, "an arc is outline");
    assert!(!action(&app, "Make Arc").enabled);

    // To a curve: the path keeps its nodes now, and the outline is
    // flattened from them.
    let reply = app.tool_event(Event::Node(NodeAction::SpanKind(
        dc_geometry::SpanKind::Cubic,
    )));
    assert_eq!(
        reply.ran.as_deref(),
        Some("Make segment of “Square” a curve")
    );
    let Some(Shape::Path { paths }) = &definition(&app, "Square").shape else {
        panic!("a curve keeps its nodes");
    };
    assert!(matches!(paths[0].nodes()[0].span, Span::Cubic { .. }));
    assert_eq!(paths[0].nodes().len(), 4, "under a quarter turn: one curve");
    assert!(definition(&app, "Square").profiles[0].vertices().len() > 4);

    // And straight again: the four lines it was.
    let reply = app.tool_event(Event::Node(NodeAction::SpanKind(
        dc_geometry::SpanKind::Line,
    )));
    assert_eq!(reply.ran.as_deref(), Some("Straighten segment of “Square”"));
    assert_eq!(definition(&app, "Square").shape, None);
    assert_points(
        &vertices(&app, "Square"),
        &[(10.0, 10.0), (50.0, 10.0), (50.0, 40.0), (10.0, 40.0)],
    );
    let before = steps(&app);
    app.tool_event(Event::Node(NodeAction::SpanKind(
        dc_geometry::SpanKind::Line,
    )));
    assert_eq!(steps(&app), before, "already straight: nothing to do");
}
