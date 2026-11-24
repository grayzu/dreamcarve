// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/sketch/featureDistribution.test.ts

//! Slice 19 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Organising"): arrays (functional capability 98). The grid and the
//! circular plans are ports; the angle is re-derived for Y-up, where a
//! positive one turns anticlockwise.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::array::{self, Array, Circular, Grid, GridSpacing};
use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::tool::{Adjust, ArrayKind, Event, Key, SizeField, ToolOption};
use dc_app::{App, Command, CommandError, Pointer, Selection, Strings, Tool};
use dc_geometry::{Point, Polyline, Rect, Transform};
use dc_project::InstanceId;

const EPS: f64 = 1e-9;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

/// A box of 20 × 10 with its middle at `(x, y)`.
fn around(x: f64, y: f64) -> Rect {
    Rect::from_point(Point::new(x - 10.0, y - 5.0)).including(Point::new(x + 10.0, y + 5.0))
}

/// Where each placement puts the middle of `bounds`.
fn middles(placements: &[Transform], bounds: Rect) -> Vec<(f64, f64)> {
    let middle = Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    );
    placements
        .iter()
        .map(|placed| {
            let at = placed.apply(middle);
            ((at.x * 1e6).round() / 1e6, (at.y * 1e6).round() / 1e6)
        })
        .collect()
}

fn grid(columns: u32, rows: u32, x_mm: f64, y_mm: f64, spacing: GridSpacing) -> Array {
    Array::Grid(Grid {
        columns,
        rows,
        x_mm,
        y_mm,
        spacing,
    })
}

fn circular(count: u32, centre: (f64, f64), total_deg: f64, turn: bool) -> Array {
    Array::Circular(Circular {
        count,
        centre: Point::new(centre.0, centre.1),
        total_deg,
        turn,
    })
}

/// Port of *grid keeps the source as origin*: a 2 × 3 grid is every cell
/// but the source's, row by row. By pitch the spacing is from one copy's
/// side to the next one's same side; by gap, the selection's own width
/// and height are added.
#[test]
fn grid_places_by_pitch_and_by_gap() {
    let bounds = around(10.0, 20.0);
    let plan = array::placements(&grid(3, 2, 4.0, 5.0, GridSpacing::Pitch), bounds).unwrap();
    assert_eq!(
        middles(&plan, bounds),
        [
            (14.0, 20.0),
            (18.0, 20.0),
            (10.0, 25.0),
            (14.0, 25.0),
            (18.0, 25.0)
        ]
    );
    // A box 20 wide and 10 high, 4 and 5 apart.
    let plan = array::placements(&grid(2, 2, 4.0, 5.0, GridSpacing::Gap), bounds).unwrap();
    assert_eq!(
        middles(&plan, bounds),
        [(34.0, 20.0), (10.0, 35.0), (34.0, 35.0)]
    );
    // A spacing below nothing goes the other way.
    let plan = array::placements(&grid(2, 1, -30.0, 0.0, GridSpacing::Pitch), bounds).unwrap();
    assert_eq!(middles(&plan, bounds), [(-20.0, 20.0)]);

    // One cell is no array, and a copy laid on its source is none.
    for refused in [
        grid(1, 1, 4.0, 5.0, GridSpacing::Pitch),
        grid(0, 3, 4.0, 5.0, GridSpacing::Pitch),
        grid(3, 1, 0.0, 5.0, GridSpacing::Pitch),
        grid(1, 2, 4.0, 0.0, GridSpacing::Pitch),
        grid(2, 1, -20.0, 0.0, GridSpacing::Gap),
        grid(2000, 2000, 4.0, 5.0, GridSpacing::Pitch),
    ] {
        assert!(array::placements(&refused, bounds).is_err(), "{refused:?}");
    }
}

/// Port of *radial starts at the source and avoids duplicating the
/// endpoint*: four copies round a whole turn are a quarter turn apart,
/// the source the first of them. A positive angle is anticlockwise.
#[test]
fn circular_starts_at_the_source() {
    let bounds = around(10.0, 0.0);
    let plan = array::placements(&circular(4, (0.0, 0.0), 360.0, true), bounds).unwrap();
    assert_eq!(
        middles(&plan, bounds),
        [(0.0, 10.0), (-10.0, 0.0), (0.0, -10.0)]
    );
    // Turned to face the centre: the first copy is a quarter turn round.
    let corner = plan[0].apply(Point::new(20.0, 5.0));
    assert!(
        (corner.x + 5.0).abs() < EPS && (corner.y - 20.0).abs() < EPS,
        "{corner:?}"
    );
    // Kept upright: moved, not turned.
    let upright = array::placements(&circular(4, (0.0, 0.0), 360.0, false), bounds).unwrap();
    assert_eq!(middles(&upright, bounds), middles(&plan, bounds));
    let corner = upright[0].apply(Point::new(20.0, 5.0));
    assert!(
        (corner.x - 10.0).abs() < EPS && (corner.y - 15.0).abs() < EPS,
        "{corner:?}"
    );

    // Less than a whole turn: the last copy is at the angle's end.
    let half = array::placements(&circular(3, (0.0, 0.0), 180.0, true), bounds).unwrap();
    assert_eq!(middles(&half, bounds), [(0.0, 10.0), (-10.0, 0.0)]);
    let clockwise = array::placements(&circular(3, (0.0, 0.0), -180.0, true), bounds).unwrap();
    assert_eq!(middles(&clockwise, bounds), [(0.0, -10.0), (-10.0, 0.0)]);

    // One copy is no array; a centre at the selection's own middle
    // leaves every copy on the source; no angle, the same.
    for refused in [
        circular(1, (0.0, 0.0), 360.0, true),
        circular(4, (10.0, 0.0), 360.0, true),
        circular(4, (0.0, 0.0), 0.0, true),
    ] {
        assert!(array::placements(&refused, bounds).is_err(), "{refused:?}");
    }
}

fn square() -> NewShape {
    NewShape {
        name: "square".to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(10.0, 10.0), 20.0, 20.0)],
        transform: Transform::IDENTITY,
        construction: false,
        group: None,
    }
}

fn app_with_a_square() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(vec![square()])).unwrap();
    app.select(Selection::Instances(vec![id("i1")]));
    app
}

fn bounds_of(app: &App, instance: &str) -> Rect {
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

/// **Create** makes linked copies of the selection, in one group named
/// for it with the source, in one undo step; an array is copies, not
/// remembered as an array.
#[test]
fn copies_are_linked_and_grouped() {
    let mut app = app_with_a_square();
    let strings = Strings::english();
    let command = array::command(
        app.project(),
        &strings,
        &[id("i1")],
        &grid(3, 1, 30.0, 0.0, GridSpacing::Pitch),
    )
    .unwrap();
    assert_eq!(app.run(command).unwrap(), "Array “square”");
    let project = app.project();
    let names: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["square", "square (2)", "square (3)"]);
    assert_eq!(project.definitions.len(), 1, "linked copies");
    assert_eq!(project.groups.len(), 1);
    let group = &project.groups[0];
    assert_eq!(
        (group.name.as_str(), group.together),
        ("square array", true)
    );
    assert!(
        project
            .instances
            .iter()
            .all(|i| i.group.as_ref() == Some(&group.id)),
        "the source is of the group"
    );
    assert_eq!(bounds_of(&app, "i3").min.x, 70.0);
    assert!(app.undo().is_some());
    assert_eq!(app.project().instances.len(), 1);
    assert!(app.project().groups.is_empty(), "one undo step");

    // Nothing selected, and an array that cannot be, are refused.
    assert!(matches!(
        array::command(
            app.project(),
            &strings,
            &[],
            &grid(3, 1, 30.0, 0.0, GridSpacing::Pitch)
        ),
        Err(CommandError::NothingSelected)
    ));
    let refused = array::command(
        app.project(),
        &strings,
        &[id("i1")],
        &grid(1, 1, 30.0, 0.0, GridSpacing::Pitch),
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "an array needs two copies or more: set the columns or the rows"
    );
}

fn field_texts(app: &App) -> Vec<(SizeField, String)> {
    app.tool_bar()
        .unwrap()
        .fields
        .iter()
        .map(|row| (row.field, row.text.clone()))
        .collect()
}

fn type_value(app: &mut App, field: SizeField, text: &str) {
    app.tool_event(Event::Field {
        field,
        text: text.to_owned(),
    });
}

/// **Arrange ▸ Array…** takes its values in the tool bar: the kind, the
/// counts and the spacing, the copies previewed as they change; Done
/// makes them.
#[test]
fn the_array_bar_previews_and_creates_a_grid() {
    let mut app = app_with_a_square();
    app.arm(Tool::Adjust(Adjust::Array));
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.prompt, "Set the copies and their spacing, then Done");
    let kinds: Vec<&str> = bar.choices[0]
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(
        (bar.choices[0].label.as_str(), kinds),
        ("Kind", vec!["Grid", "Circular"])
    );
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::Columns, "2".to_owned()),
            (SizeField::Rows, "1".to_owned()),
            (SizeField::SpacingX, "20".to_owned()),
            (SizeField::SpacingY, "20".to_owned()),
        ]
    );
    let spacing: Vec<&str> = bar.choices[1]
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(spacing, ["Gap", "Pitch"]);
    assert!(bar.can_finish);
    assert_eq!(app.draft().outlines.len(), 1, "one copy, previewed");
    assert_eq!(
        app.draft().outlines[0].bounds().unwrap().min.x,
        50.0,
        "20 of gap"
    );

    type_value(&mut app, SizeField::Columns, "3");
    type_value(&mut app, SizeField::Rows, "2");
    type_value(&mut app, SizeField::SpacingX, "5");
    assert_eq!(app.draft().outlines.len(), 5);
    app.tool_event(Event::Option(ToolOption::ArraySpacing(GridSpacing::Pitch)));
    assert_eq!(app.draft().outlines[0].bounds().unwrap().min.x, 15.0);
    // A count that is none is said at its field, and Done waits.
    type_value(&mut app, SizeField::Columns, "0");
    let bar = app.tool_bar().unwrap();
    assert!(bar.fields[0].problem.is_some() && !bar.can_finish);
    type_value(&mut app, SizeField::Columns, "2");
    type_value(&mut app, SizeField::Rows, "1");

    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Array “square”"));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.project().instances.len(), 2);
    assert_eq!(bounds_of(&app, "i2").min.x, 15.0);
    assert_eq!(
        app.selection().instances(),
        [id("i1"), id("i2")],
        "the array is selected, as a click on it would select it"
    );
    app.select(Selection::Instances(vec![id("i1")]));
    // The values are offered again.
    app.arm(Tool::Adjust(Adjust::Array));
    assert_eq!(field_texts(&app)[2], (SizeField::SpacingX, "5".to_owned()));
}

/// A circular array's centre is typed or clicked on the Viewport; until
/// it has one there is nothing to preview and Done waits.
#[test]
fn the_array_bar_takes_a_circular_centre() {
    let mut app = app_with_a_square();
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Adjust(Adjust::Array));
    app.tool_event(Event::Option(ToolOption::ArrayKind(ArrayKind::Circular)));
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.prompt, "Click the centre of the circle, or type it");
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::Copies, "4".to_owned()),
            (SizeField::X, String::new()),
            (SizeField::Y, String::new()),
            (SizeField::Angle, "360".to_owned()),
        ]
    );
    let turn: Vec<&str> = bar.choices[1]
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(
        (bar.choices[1].label.as_str(), turn),
        ("Copies", vec!["Turn", "Keep upright"])
    );
    assert!(!bar.can_finish);
    assert!(app.draft().outlines.is_empty());

    // A click is the centre: (60, 20), 40 from the square's middle.
    let at = Pointer {
        at_mm: Point::new(60.0, 20.0),
        shift: false,
        alt: false,
        command: false,
    };
    app.tool_event(Event::Press(at));
    app.tool_event(Event::Release(at));
    assert_eq!(
        field_texts(&app)[1..3],
        [
            (SizeField::X, "60".to_owned()),
            (SizeField::Y, "20".to_owned())
        ]
    );
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Set the copies and the angle, then Done"
    );
    assert_eq!(app.draft().outlines.len(), 3);
    type_value(&mut app, SizeField::Copies, "2");
    type_value(&mut app, SizeField::Angle, "180");
    app.tool_event(Event::Option(ToolOption::ArrayTurn(false)));
    assert_eq!(app.draft().outlines.len(), 1);
    let reply = app.tool_event(Event::Key(Key::Enter));
    assert_eq!(reply.ran.as_deref(), Some("Array “square”"));
    // Half a turn round (60, 20) from (20, 20) is (100, 20), upright.
    let copy = bounds_of(&app, "i2");
    assert!(
        (copy.min.x - 90.0).abs() < 1e-6 && (copy.min.y - 10.0).abs() < 1e-6,
        "{copy:?}"
    );

    // Escape leaves with nothing made.
    app.select(Selection::Instances(vec![id("i1")]));
    app.arm(Tool::Adjust(Adjust::Array));
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.project().instances.len(), 2);
}
