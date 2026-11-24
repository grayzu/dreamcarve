// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 17 of phase 5 (`docs/specs/phase-5/design.md`, "The window
//! (slice 17)"): the Shape menu's commands that take a value first,
//! **Offset…**, **Fillet Corners…** and **Join**, as modes of the
//! Viewport with the tool bar for their popover (functional capabilities
//! 93, 94 and 97). What each command makes is `dc_app::edit`'s and is
//! tested in `tests/edit.rs` and `tests/nodes.rs`; these tests hold the
//! mode: what the bar shows, the live preview, Done and Escape.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::edit::OffsetSide;
use dc_app::test_support::Harness;
use dc_app::tool::{Adjust, Event, Key, SizeField, ToolOption};
use dc_app::{App, Command, Selection, Tool};
use dc_geometry::{Point, Polyline, Rect, Transform, Vertex};
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
        group: None,
    }
}

fn line(name: &str, points: &[(f64, f64)]) -> NewShape {
    NewShape {
        profiles: vec![Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )],
        ..rectangle(name, (0.0, 0.0), (1.0, 1.0))
    }
}

/// A blank project with `shapes`, those named `selected` selected.
fn app_with(shapes: Vec<NewShape>, selected: &[&str]) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(shapes)).unwrap();
    app.select(Selection::Instances(
        selected.iter().map(|name| id(name)).collect(),
    ));
    app
}

fn names(app: &App) -> Vec<&str> {
    app.project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

fn type_value(app: &mut App, field: SizeField, text: &str) {
    app.tool_event(Event::Field {
        field,
        text: text.to_owned(),
    });
}

#[track_caller]
fn assert_box(outline: &Polyline, min: (f64, f64), max: (f64, f64)) {
    let Rect { min: lo, max: hi } = outline.bounds().unwrap();
    for (got, want) in [(lo.x, min.0), (lo.y, min.1), (hi.x, max.0), (hi.y, max.1)] {
        assert!((got - want).abs() < 1e-6, "box {lo:?} to {hi:?}");
    }
}

/// Capability 94: Offset… shows the distance, the side and whether the
/// original stays; the result previews as the values change; Done makes
/// it, one undo step, and the mode is over with the result selected.
#[test]
fn offset_previews_and_commits() {
    let mut app = app_with(
        vec![rectangle("Plate", (10.0, 10.0), (40.0, 30.0))],
        &["i1"],
    );
    app.arm(Tool::Adjust(Adjust::Offset));
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "Offset the selection: set the distance and the side, then Done"
    );
    assert_eq!(bar.fields.len(), 1);
    let distance = &bar.fields[0];
    assert_eq!(
        (
            distance.field,
            distance.label.as_str(),
            distance.text.as_str()
        ),
        (SizeField::Distance, "Distance", "5")
    );
    let choices: Vec<(String, Vec<String>, usize)> = bar
        .choices
        .iter()
        .map(|c| {
            (
                c.label.clone(),
                c.options.iter().map(|o| o.label.clone()).collect(),
                c.selected,
            )
        })
        .collect();
    let labels = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
    assert_eq!(
        choices,
        [
            ("Side".to_owned(), labels(&["Outward", "Inward", "Both"]), 0),
            ("Original".to_owned(), labels(&["Keep", "Replace"]), 0),
        ]
    );
    assert!(bar.can_finish);
    // The preview, as armed and as the values change.
    assert_eq!(app.draft().outlines.len(), 1);
    assert_box(&app.draft().outlines[0], (5.0, 5.0), (55.0, 45.0));
    type_value(&mut app, SizeField::Distance, "3");
    assert_box(&app.draft().outlines[0], (7.0, 7.0), (53.0, 43.0));
    app.tool_event(Event::Option(ToolOption::OffsetSide(OffsetSide::Inward)));
    assert_box(&app.draft().outlines[0], (13.0, 13.0), (47.0, 37.0));
    assert_eq!(app.tool_bar().unwrap().choices[0].selected, 1);
    assert_eq!(app.project().instances.len(), 1, "nothing is made yet");

    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Offset “Plate”"));
    assert_eq!(reply.refused, None);
    assert_eq!(names(&app), ["Plate", "Plate offset"]);
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.selection().instances(), [id("i2")]);
    assert!(app.draft().outlines.is_empty());

    // The values are kept for the next offset, and Replace takes the
    // original's place.
    app.select(Selection::Instances(vec![id("i1")]));
    app.arm(Tool::Adjust(Adjust::Offset));
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.fields[0].text, "3");
    assert_eq!(bar.choices[0].selected, 1);
    app.tool_event(Event::Option(ToolOption::KeepOriginal(false)));
    assert_eq!(app.tool_bar().unwrap().choices[1].selected, 1);
    app.tool_event(Event::Key(Key::Enter));
    assert_eq!(names(&app), ["Plate offset (2)", "Plate offset"]);

    // A distance that leaves nothing is said at the field, and Done is
    // off; Escape leaves with nothing done.
    app.select(Selection::Instances(vec![id("i2")]));
    app.arm(Tool::Adjust(Adjust::Offset));
    type_value(&mut app, SizeField::Distance, "20");
    let bar = app.tool_bar().unwrap();
    assert!(!bar.can_finish);
    assert_eq!(
        bar.fields[0].problem.as_deref(),
        Some("nothing is left of Plate offset that far inside its outline")
    );
    assert!(app.draft().outlines.is_empty());
    let reply = app.tool_event(Event::Done);
    assert!(reply.ran.is_none() && reply.refused.is_some());
    assert_eq!(app.tool(), Tool::Adjust(Adjust::Offset), "still asking");
    type_value(&mut app, SizeField::Distance, "none");
    assert!(app.tool_bar().unwrap().fields[0].problem.is_some());
    let count = app.project().instances.len();
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.project().instances.len(), count);

    // With nothing selected the bar asks for a selection.
    app.select(Selection::Nothing);
    app.arm(Tool::Adjust(Adjust::Offset));
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "Select the shapes first, then choose this again"
    );
    assert!(!bar.can_finish);
}

/// Capabilities 93 and 97: Fillet Corners… takes a radius and one shape;
/// An open path has no inside: its sides are named Left and Right (of
/// its direction), which is what Outward and Inward mean for it. A
/// selection with a closed shape in it keeps Outward and Inward.
#[test]
fn an_open_path_offsets_left_and_right() {
    let sides = |app: &App| -> Vec<String> {
        app.tool_bar().unwrap().choices[0]
            .options
            .iter()
            .map(|o| o.label.clone())
            .collect()
    };
    let mut app = app_with(
        vec![
            line("Line 1", &[(10.0, 10.0), (50.0, 10.0)]),
            rectangle("Rectangle 1", (10.0, 30.0), (40.0, 20.0)),
        ],
        &["i1"],
    );
    app.arm(Tool::Adjust(Adjust::Offset));
    assert_eq!(sides(&app), ["Left", "Right", "Both"]);
    // Left of a line drawn along +X is above it.
    type_value(&mut app, SizeField::Distance, "2");
    let above = app.draft().outlines[0].bounds().unwrap();
    assert!((above.min.y - 12.0).abs() < 1e-9, "{above:?}");
    app.tool_event(Event::Option(ToolOption::OffsetSide(OffsetSide::Inward)));
    let below = app.draft().outlines[0].bounds().unwrap();
    assert!((below.min.y - 8.0).abs() < 1e-9, "{below:?}");
    assert_eq!(app.tool_bar().unwrap().choices[0].selected, 1);

    app.arm(Tool::Select);
    app.select(Selection::Instances(vec![id("i1"), id("i2")]));
    app.arm(Tool::Adjust(Adjust::Offset));
    assert_eq!(sides(&app), ["Outward", "Inward", "Both"]);
}

/// Join takes a tolerance, and says how many paths it left alone.
#[test]
fn fillet_corners_and_join_take_a_value() {
    let mut app = app_with(
        vec![
            rectangle("Plate", (10.0, 10.0), (40.0, 30.0)),
            rectangle("Other", (60.0, 10.0), (20.0, 20.0)),
        ],
        &["i1"],
    );
    app.arm(Tool::Adjust(Adjust::FilletCorners));
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "Round every corner of the shape that can take the radius, then Done"
    );
    assert_eq!(
        (bar.fields[0].field, bar.fields[0].text.as_str()),
        (SizeField::Radius, "5")
    );
    assert!(bar.choices.is_empty() && bar.can_finish);
    type_value(&mut app, SizeField::Radius, "3");
    let reply = app.tool_event(Event::Key(Key::Enter));
    assert_eq!(reply.ran.as_deref(), Some("Fillet 4 corners of “Plate”"));
    assert_eq!(app.tool(), Tool::Select);
    // Two shapes: one at a time.
    app.select(Selection::Instances(vec![id("i1"), id("i2")]));
    app.arm(Tool::Adjust(Adjust::FilletCorners));
    let bar = app.tool_bar().unwrap();
    assert!(!bar.can_finish);
    assert_eq!(
        bar.fields[0].problem.as_deref(),
        Some("Fillet Corners takes one shape at a time: select one.")
    );

    let mut app = app_with(
        vec![
            line("A", &[(10.0, 10.0), (30.0, 10.0)]),
            line("B", &[(30.05, 10.0), (50.0, 10.0)]),
            line("Far", &[(60.0, 10.0), (80.0, 10.0)]),
        ],
        &["i1", "i2", "i3"],
    );
    app.arm(Tool::Adjust(Adjust::Join));
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        (bar.fields[0].field, bar.fields[0].text.as_str()),
        (SizeField::Tolerance, "0.1")
    );
    assert!(bar.can_finish);
    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Join 2 shapes"));
    assert_eq!(
        reply.refused.as_deref(),
        Some("1 path reached nothing and was left alone.")
    );
    assert_eq!(names(&app), ["A", "Far"]);
    // Nothing more within 0.1: Done is off until the tolerance reaches.
    app.select(Selection::Instances(vec![
        app.project().instances[0].id.clone(),
        id("i3"),
    ]));
    app.arm(Tool::Adjust(Adjust::Join));
    assert!(!app.tool_bar().unwrap().can_finish);
    type_value(&mut app, SizeField::Tolerance, "12");
    assert!(app.tool_bar().unwrap().can_finish);
    app.tool_event(Event::Done);
    assert_eq!(names(&app), ["A"]);
}
