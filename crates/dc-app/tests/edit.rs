// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slices 13 and 14 of phase 5 (`docs/specs/phase-5/design.md`, "Test
//! plan → Changing shapes"): **Shape ▸ Offset…**, **Weld**, **Subtract**
//! and **Intersect** (functional capabilities 94 and 95; acceptance 119
//! and 120).
//!
//! The offset and the boolean are `dc-geometry`'s, the engine's own, and
//! tested there; these tests hold what the app makes of them: which
//! shapes result, what they are called, where they are placed, and what
//! becomes of the shapes they were made from and of what cut those.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::edit::{self, Boolean, Offset, OffsetSide};
use dc_app::test_support::Harness;
use dc_app::{App, Command, CommandError};
use dc_geometry::{Point, Polyline, Rect, Transform, Vertex};
use dc_project::{FeatureDefinition, FeatureInstance, InstanceId, Shape};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

/// A rectangle of four lines from (`x`, `y`), `w` × `h`.
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

fn app_with(shapes: Vec<NewShape>) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(shapes)).unwrap();
    app
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

fn names(app: &App) -> Vec<&str> {
    app.project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

/// The area `name`'s outlines enclose: outers less holes.
fn area(app: &App, name: &str) -> f64 {
    definition(app, name)
        .profiles
        .iter()
        .map(Polyline::signed_area_mm2)
        .sum()
}

#[track_caller]
fn assert_box(outline: &Polyline, min: (f64, f64), max: (f64, f64)) {
    let Rect { min: lo, max: hi } = outline.bounds().unwrap();
    for (got, want) in [(lo.x, min.0), (lo.y, min.1), (hi.x, max.0), (hi.y, max.1)] {
        assert!((got - want).abs() < 1e-6, "box {lo:?} to {hi:?}");
    }
}

fn offset_of(distance_mm: f64, side: OffsetSide, keep_original: bool) -> Offset {
    Offset {
        distance_mm,
        side,
        keep_original,
    }
}

fn refusal(result: Result<impl std::fmt::Debug, CommandError>) -> String {
    result.expect_err("refused").to_string()
}

/// Acceptance 119: a 40 × 30 rectangle offset 5 outward is a 50 × 40
/// outline with four arcs of radius 5; inward 5 is a 30 × 20 rectangle.
/// The result is a new path named `<name> offset`, beside its source.
#[test]
fn offset_outward_rounds_corners() {
    let mut app = app_with(vec![
        rectangle("Plate", (10.0, 10.0), (40.0, 30.0)),
        rectangle("Other", (70.0, 10.0), (10.0, 10.0)),
    ]);
    let ids = [id("i1")];
    let made = edit::offset(
        app.project(),
        app.strings(),
        &ids,
        &offset_of(5.0, OffsetSide::Outward, true),
    )
    .unwrap();
    assert!(made.vanished.is_empty());
    assert_eq!(
        app.run(made.command).as_deref().ok(),
        Some("Offset “Plate”")
    );
    assert_eq!(names(&app), ["Plate", "Plate offset", "Other"]);
    let grown = &definition(&app, "Plate offset").profiles;
    assert_eq!(grown.len(), 1);
    assert!(grown[0].is_closed() && grown[0].signed_area_mm2() > 0.0);
    assert_box(&grown[0], (5.0, 5.0), (55.0, 45.0));
    let arcs: Vec<_> = grown[0].segments().filter_map(|s| s.arc()).collect();
    assert_eq!((grown[0].segments().count(), arcs.len()), (8, 4));
    assert!(arcs.iter().all(|arc| (arc.radius_mm - 5.0).abs() < 1e-6));
    assert_eq!(definition(&app, "Plate offset").shape, None, "a plain path");
    assert_eq!(
        instance(&app, "Plate offset").transform,
        instance(&app, "Plate").transform
    );
    // What was made is selected.
    assert_eq!(
        app.selection().instances(),
        [instance(&app, "Plate offset").id.clone()]
    );

    // Inward 5: a 30 × 20 rectangle, its corners sharp. The first
    // offset has the name, so this is the next.
    let made = edit::offset(
        app.project(),
        app.strings(),
        &ids,
        &offset_of(5.0, OffsetSide::Inward, true),
    )
    .unwrap();
    app.run(made.command).unwrap();
    let shrunk = &definition(&app, "Plate offset (2)").profiles[0];
    assert_box(shrunk, (15.0, 15.0), (45.0, 35.0));
    assert_eq!(shrunk.segments().filter(|s| s.arc().is_some()).count(), 0);

    // Both sides at once: two outlines in the one new shape.
    let both = edit::offset_outlines(app.project(), &ids, &offset_of(5.0, OffsetSide::Both, true));
    assert_eq!(both.len(), 2);
    // The preview is what the command makes, placed.
    assert_box(&both[0], (5.0, 5.0), (55.0, 45.0));
    assert_box(&both[1], (15.0, 15.0), (45.0, 35.0));
}

/// Acceptance 119: inward 20 makes nothing and says so. With several
/// selected, the ones that vanish are named and the others are offset.
#[test]
fn offset_inward_past_the_middle_makes_nothing() {
    let mut app = app_with(vec![
        rectangle("Plate", (10.0, 10.0), (40.0, 30.0)),
        rectangle("Big", (10.0, 50.0), (80.0, 45.0)),
    ]);
    let before = app.project().clone();
    let inward = offset_of(20.0, OffsetSide::Inward, true);
    assert_eq!(
        refusal(edit::offset(
            app.project(),
            app.strings(),
            &[id("i1")],
            &inward
        )),
        "nothing is left of Plate that far inside its outline"
    );
    assert!(edit::offset_outlines(app.project(), &[id("i1")], &inward).is_empty());
    let made = edit::offset(app.project(), app.strings(), &[id("i1"), id("i2")], &inward).unwrap();
    assert_eq!(made.vanished, ["Plate"]);
    app.run(made.command).unwrap();
    assert_eq!(names(&app), ["Plate", "Big", "Big offset"]);
    assert_box(
        &definition(&app, "Big offset").profiles[0],
        (30.0, 70.0),
        (70.0, 75.0),
    );
    // A distance that is none, and no selection.
    for distance in [0.0, -3.0, f64::NAN] {
        assert!(matches!(
            edit::offset(
                &before,
                app.strings(),
                &[id("i1")],
                &offset_of(distance, OffsetSide::Outward, true)
            ),
            Err(CommandError::InvalidValue { .. })
        ));
    }
    assert!(matches!(
        edit::offset(&before, app.strings(), &[], &inward),
        Err(CommandError::NothingSelected)
    ));
}

/// Acceptance 119: an open line offset Both gives two lines 10 apart.
/// Outward is the left of its direction, Inward the right. A shape with
/// a hole grows outward while its hole shrinks; a placed, scaled shape
/// is offset by the distance as it is seen.
#[test]
fn offset_of_an_open_path() {
    let line = NewShape {
        name: "Line".to_owned(),
        shape: None,
        profiles: vec![Polyline::new(
            vec![Vertex::line(10.0, 20.0), Vertex::line(60.0, 20.0)],
            false,
        )],
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    };
    let mut frame = rectangle("Frame", (10.0, 40.0), (40.0, 40.0));
    frame
        .profiles
        .push(Polyline::rect(Point::new(20.0, 50.0), 20.0, 20.0).reversed());
    let mut app = app_with(vec![line, frame]);
    let made = edit::offset(
        app.project(),
        app.strings(),
        &[id("i1")],
        &offset_of(5.0, OffsetSide::Both, true),
    )
    .unwrap();
    app.run(made.command).unwrap();
    let lines = &definition(&app, "Line offset").profiles;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| !line.is_closed()));
    // Left of a line running +X is +Y.
    let ys: Vec<f64> = lines.iter().map(|line| line.vertices()[0].y).collect();
    assert!(
        (ys[0] - 25.0).abs() < 1e-9 && (ys[1] - 15.0).abs() < 1e-9,
        "{ys:?}"
    );
    assert!((lines[0].length_mm() - 50.0).abs() < 1e-9);

    // The frame, outward 2: the outline grows to 44, the hole shrinks to
    // 16.
    let made = edit::offset(
        app.project(),
        app.strings(),
        &[id("i2")],
        &offset_of(2.0, OffsetSide::Outward, true),
    )
    .unwrap();
    app.run(made.command).unwrap();
    let grown = &definition(&app, "Frame offset").profiles;
    assert_eq!(grown.len(), 2);
    assert_box(&grown[0], (8.0, 38.0), (52.0, 82.0));
    assert_box(&grown[1], (22.0, 52.0), (38.0, 68.0));
    assert!(grown[1].signed_area_mm2() < 0.0, "the hole stays a hole");

    // Doubled, the plain rectangle below is 80 × 60 as it is placed; 5
    // outward as it is seen is 2.5 in its own space.
    let mut app = app_with(vec![rectangle("Plate", (10.0, 10.0), (40.0, 30.0))]);
    app.run(Command::TransformInstances {
        ids: vec![id("i1")],
        delta: Transform::scale(2.0, 2.0),
    })
    .unwrap();
    let made = edit::offset(
        app.project(),
        app.strings(),
        &[id("i1")],
        &offset_of(5.0, OffsetSide::Outward, true),
    )
    .unwrap();
    app.run(made.command).unwrap();
    let own = &definition(&app, "Plate offset").profiles[0];
    assert_box(own, (7.5, 7.5), (52.5, 42.5));
    let placed = instance(&app, "Plate offset")
        .transform
        .apply_polyline(own)
        .unwrap();
    let was = instance(&app, "Plate")
        .transform
        .apply_polyline(&definition(&app, "Plate").profiles[0])
        .unwrap()
        .bounds()
        .unwrap();
    assert_box(
        &placed,
        (was.min.x - 5.0, was.min.y - 5.0),
        (was.max.x + 5.0, was.max.y + 5.0),
    );
}

/// Acceptance 120: two overlapping 40 × 40 squares 20 apart: Weld is one
/// outline of area 2400; Subtract, the first less the overlap, area 800;
/// Intersect, the overlap, area 800; a circle subtracted from a square
/// inside it gives a square with a hole; two squares that do not touch:
/// Intersect changes nothing.
#[test]
fn weld_subtract_intersect_areas() {
    let squares = || {
        vec![
            rectangle("A", (10.0, 10.0), (40.0, 40.0)),
            rectangle("B", (30.0, 10.0), (40.0, 40.0)),
        ]
    };
    let both = [id("i1"), id("i2")];
    for (op, name, label, want_area) in [
        (Boolean::Weld, "A weld", "Weld 2 shapes", 2400.0),
        (
            Boolean::Subtract,
            "A subtract",
            "Subtract from 2 shapes",
            800.0,
        ),
        (
            Boolean::Intersect,
            "A intersect",
            "Intersect 2 shapes",
            800.0,
        ),
    ] {
        let mut app = app_with(squares());
        let command = edit::boolean(app.project(), app.strings(), &both, op).unwrap();
        assert_eq!(app.run(command).as_deref().ok(), Some(label));
        assert_eq!(names(&app), [name], "the originals are replaced");
        let made = definition(&app, name);
        assert_eq!(made.profiles.len(), 1);
        assert!((area(&app, name) - want_area).abs() < 1e-6, "{op:?}");
        assert_eq!(made.shape, None);
        // One undo step brings the two back.
        app.undo();
        assert_eq!(names(&app), ["A", "B"]);
    }
    // Subtract reads the selection in order: B less A is the other end.
    let mut app = app_with(squares());
    let command = edit::boolean(
        app.project(),
        app.strings(),
        &[id("i2"), id("i1")],
        Boolean::Subtract,
    )
    .unwrap();
    app.run(command).unwrap();
    assert_box(
        &definition(&app, "B subtract").profiles[0],
        (50.0, 10.0),
        (70.0, 50.0),
    );

    // A circle inside a square, subtracted: a square with a hole.
    let circle = Shape::Circle { diameter_mm: 20.0 };
    let hole = NewShape {
        name: "Circle 1".to_owned(),
        profiles: circle.outline(0.01).unwrap().unwrap(),
        shape: Some(circle),
        transform: Transform::translate(30.0, 30.0),
        construction: false,
        folder: None,
    };
    let mut app = app_with(vec![rectangle("A", (10.0, 10.0), (40.0, 40.0)), hole]);
    let command = edit::boolean(app.project(), app.strings(), &both, Boolean::Subtract).unwrap();
    app.run(command).unwrap();
    let holed = &definition(&app, "A subtract").profiles;
    assert_eq!(holed.len(), 2);
    assert!(holed[0].signed_area_mm2() > 0.0 && holed[1].signed_area_mm2() < 0.0);
    let disc = std::f64::consts::PI * 100.0;
    // The circle comes out as chords within the flatten tolerance.
    assert!((area(&app, "A subtract") - (1600.0 - disc)).abs() < 0.5);

    // Two squares that do not touch.
    let apart = vec![
        rectangle("A", (10.0, 10.0), (20.0, 20.0)),
        rectangle("B", (60.0, 10.0), (20.0, 20.0)),
    ];
    let mut app = app_with(apart);
    let before = app.project().clone();
    assert_eq!(
        refusal(edit::boolean(
            app.project(),
            app.strings(),
            &both,
            Boolean::Intersect
        )),
        "The shapes share no area, so there is nothing to keep."
    );
    assert_eq!(app.project(), &before);
    // Welded, they are one shape of two outlines.
    let command = edit::boolean(app.project(), app.strings(), &both, Boolean::Weld).unwrap();
    app.run(command).unwrap();
    assert_eq!(definition(&app, "A weld").profiles.len(), 2);
    assert!((area(&app, "A weld") - 800.0).abs() < 1e-6);

    // One shape, an open path, and a first shape wholly taken away.
    let line = NewShape {
        profiles: vec![Polyline::new(
            vec![Vertex::line(0.0, 0.0), Vertex::line(5.0, 5.0)],
            false,
        )],
        ..rectangle("Line", (0.0, 0.0), (1.0, 1.0))
    };
    let app = app_with(vec![
        rectangle("Small", (20.0, 20.0), (10.0, 10.0)),
        rectangle("Large", (10.0, 10.0), (40.0, 40.0)),
        line,
    ]);
    let refused = |ids: &[InstanceId], op: Boolean| {
        refusal(edit::boolean(app.project(), app.strings(), ids, op))
    };
    assert_eq!(
        refused(&[id("i1")], Boolean::Weld),
        "Select two or more closed shapes."
    );
    assert_eq!(
        refused(&[id("i1"), id("i3")], Boolean::Weld),
        "“Line” is not a closed outline, so it encloses no area to weld, subtract or intersect"
    );
    assert_eq!(
        refused(&both, Boolean::Subtract),
        "Nothing is left of the first shape."
    );
}

/// Acceptance 119 and capability 95: with Keep original off an offset
/// takes its source's place and what cut the source cuts the offset; a
/// boolean's result takes over what cut the first shape, and is placed
/// as the first is.
#[test]
fn a_result_inherits_the_first_shapes_operations() {
    let plaque = dc_app::test_support::Harness::from_project(common_plaque());
    let mut app = plaque.app;
    // The plaque's Cut out Profile targets its outline.
    let targets = |app: &App, op: &str| -> Vec<InstanceId> {
        app.project()
            .operations
            .iter()
            .find(|o| o.id().as_str() == op)
            .map(|o| o.targets().to_vec())
            .unwrap()
    };
    let outline = targets(&app, "op4")[0].clone();
    let name = app
        .project()
        .instances
        .iter()
        .find(|i| i.id == outline)
        .unwrap()
        .name
        .clone();
    let count = app.project().instances.len();
    let made = edit::offset(
        app.project(),
        app.strings(),
        std::slice::from_ref(&outline),
        &offset_of(2.0, OffsetSide::Outward, false),
    )
    .unwrap();
    app.run(made.command).unwrap();
    assert_eq!(app.project().instances.len(), count, "in its place");
    assert!(!app.project().instances.iter().any(|i| i.id == outline));
    let offset_name = format!("{name} offset");
    assert_eq!(
        targets(&app, "op4"),
        [instance(&app, &offset_name).id.clone()]
    );

    // A boolean of a turned first shape: the result is placed as it is.
    let mut app = app_with(vec![
        rectangle("A", (10.0, 10.0), (40.0, 40.0)),
        rectangle("B", (30.0, 10.0), (40.0, 40.0)),
    ]);
    app.run(Command::RotateInstances {
        ids: vec![id("i1")],
        angle_deg: 90.0,
    })
    .unwrap();
    let placed_as = instance(&app, "A").transform;
    let command = edit::boolean(
        app.project(),
        app.strings(),
        &[id("i1"), id("i2")],
        Boolean::Weld,
    )
    .unwrap();
    app.run(command).unwrap();
    assert_eq!(instance(&app, "A weld").transform, placed_as);
    // A square turned about its own centre covers what it did.
    assert!((area(&app, "A weld") - 2400.0).abs() < 1e-6);
    let placed = placed_as
        .apply_polyline(&definition(&app, "A weld").profiles[0])
        .unwrap();
    assert_box(&placed, (10.0, 10.0), (70.0, 50.0));
}

/// The plaque fixture, as the other suites read it.
fn common_plaque() -> dc_project::Project {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/plaque.dcv");
    let bytes = std::fs::read(path).expect("the plaque fixture");
    dc_project::load(&bytes).expect("it loads").project
}
