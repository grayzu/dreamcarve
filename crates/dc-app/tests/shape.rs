// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 8 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Parametric edits"): the Properties panel's Shape section, a size typed
//! there, sizes read as the shape is placed, stretching a shape that can
//! take it, and Convert to Path (functional capability 84; acceptance
//! 115, and 110's slot sizes).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use std::f64::consts::PI;

use dc_app::properties::{self, Field};
use dc_app::shape::{self, Section};
use dc_app::test_support::Harness;
use dc_app::tool::{CornerStyle, SizeField, ToolOption};
use dc_app::{App, Command, CommandError};
use dc_geometry::shapes::gear::{GearCrest, GearFlanks, GearRoot};
use dc_geometry::{Point, Transform};
use dc_project::test_support::drawing_project;
use dc_project::{
    Corner, FeatureDefinition, FeatureInstance, InstanceId, Measured, PlacedTab, Project, Shape,
    TabId, TabShape,
};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn harness() -> Harness {
    Harness::from_project(drawing_project())
}

fn instance<'a>(project: &'a Project, name: &str) -> &'a FeatureInstance {
    project
        .instances
        .iter()
        .find(|i| i.id.as_str() == name)
        .unwrap_or_else(|| panic!("no instance {name}"))
}

fn definition<'a>(project: &'a Project, name: &str) -> &'a FeatureDefinition {
    &project.definitions[&instance(project, name).definition]
}

fn shape_of(app: &App, name: &str) -> Shape {
    definition(app.project(), name)
        .shape
        .clone()
        .unwrap_or_else(|| panic!("{name} has no shape"))
}

fn section(app: &App, name: &str) -> Section {
    shape::section(app.project(), app.strings(), &id(name))
        .unwrap_or_else(|| panic!("{name} has no Shape section"))
}

/// The section's rows as (field, label, text).
fn rows(app: &App, name: &str) -> Vec<(SizeField, String, String)> {
    section(app, name)
        .rows
        .into_iter()
        .map(|row| (row.field, row.label, row.text))
        .collect()
}

fn row(field: SizeField, label: &str, text: &str) -> (SizeField, String, String) {
    (field, label.to_owned(), text.to_owned())
}

/// The section's choices as (label, the options' labels, the one chosen).
fn choices(app: &App, name: &str) -> Vec<(String, Vec<String>, usize)> {
    section(app, name)
        .choices
        .into_iter()
        .map(|choice| {
            (
                choice.label,
                choice.options.into_iter().map(|o| o.label).collect(),
                choice.selected,
            )
        })
        .collect()
}

/// Types `text` into the Shape section's `field` for `name` and runs the
/// command it makes; the undo label.
fn type_size(
    harness: &mut Harness,
    name: &str,
    field: SizeField,
    text: &str,
) -> Result<String, CommandError> {
    let app = &harness.app;
    let command = shape::set_size(app.project(), app.strings(), &id(name), field, text)?;
    harness.run(command)
}

fn choose(harness: &mut Harness, name: &str, option: ToolOption) -> Result<String, CommandError> {
    let app = &harness.app;
    let command = shape::set_option(app.project(), app.strings(), &id(name), &option)?;
    harness.run(command)
}

/// Types `text` into Properties' W or H for `names`, the proportion link
/// open or closed.
fn type_placement(
    harness: &mut Harness,
    names: &[&str],
    field: Field,
    text: &str,
    uniform: bool,
) -> Result<String, CommandError> {
    let ids: Vec<InstanceId> = names.iter().map(|name| id(name)).collect();
    let project = harness.app.project().clone();
    let placement = properties::placement(&project, &ids).unwrap();
    let edit = properties::parse(&placement, field, text, project.display_units, uniform)?;
    let command = properties::edit(&project, &ids, edit)?;
    harness.run(command)
}

/// The saved outline is the one the saved sizes give, here and to
/// validation.
#[track_caller]
fn assert_outline_is_the_shapes(app: &App, name: &str) {
    let definition = definition(app.project(), name);
    let tolerance = app.project().flatten_tolerance_mm;
    let shape = definition.shape.as_ref().expect("a parametric shape");
    assert_eq!(
        shape.outline_matches(&definition.profiles, tolerance),
        Some(true),
        "{name}"
    );
    let differing = dc_project::validate(app.project())
        .iter()
        .filter(|p| p.code() == "shape_outline_differs")
        .count();
    assert_eq!(differing, 0, "{name}");
}

fn refusal(result: Result<String, CommandError>) -> String {
    result.expect_err("refused").to_string()
}

/// Capability 84's table: each kind's sizes, in project units, with the
/// choices that are one of a few; a path and an imported outline have no
/// Shape section.
#[test]
fn rows_per_kind() {
    let harness = harness();
    let app = &harness.app;
    let corners = |selected: usize| {
        vec![(
            "Corners".to_owned(),
            vec![
                "Square".to_owned(),
                "Rounded".to_owned(),
                "Chamfered".to_owned(),
            ],
            selected,
        )]
    };

    assert_eq!(section(app, "i2").title, "Rectangle");
    assert_eq!(
        rows(app, "i2"),
        [
            row(SizeField::Width, "Width", "60"),
            row(SizeField::Height, "Height", "30"),
        ]
    );
    assert_eq!(choices(app, "i2"), corners(0));
    assert!(!section(app, "i2").read_only);
    assert_eq!(section(app, "i2").note, None);
    assert_eq!(
        rows(app, "i5"),
        [
            row(SizeField::Width, "Width", "60"),
            row(SizeField::Height, "Height", "30"),
            row(SizeField::CornerSize, "Corner size", "5"),
        ]
    );
    assert_eq!(choices(app, "i5"), corners(1));
    assert_eq!(choices(app, "i6"), corners(2));
    assert_eq!(rows(app, "i6").len(), 3);

    assert_eq!(section(app, "i3").title, "Circle");
    assert_eq!(
        rows(app, "i3"),
        [row(SizeField::Diameter, "Diameter", "20")]
    );
    assert!(choices(app, "i3").is_empty());

    assert_eq!(section(app, "i7").title, "Ellipse");
    assert_eq!(
        rows(app, "i7"),
        [
            row(SizeField::Width, "Width", "40"),
            row(SizeField::Height, "Height", "20"),
        ]
    );

    assert_eq!(section(app, "i8").title, "Hexagon");
    assert_eq!(
        rows(app, "i8"),
        [
            row(SizeField::Sides, "Sides", "6"),
            row(SizeField::Radius, "Radius", "10"),
        ]
    );
    assert_eq!(
        choices(app, "i8"),
        [(
            "Measure".to_owned(),
            vec!["To the corners".to_owned(), "To the flats".to_owned()],
            1
        )]
    );

    // Turned 30°, its sizes read the same.
    assert_eq!(section(app, "i9").title, "Slot");
    assert_eq!(
        rows(app, "i9"),
        [
            row(SizeField::Length, "Length", "40"),
            row(SizeField::SlotWidth, "Width", "12"),
        ]
    );

    let gear = section(app, "i10");
    assert_eq!(gear.title, "Gear");
    assert_eq!(
        gear.rows.iter().map(|row| row.field).collect::<Vec<_>>(),
        [
            SizeField::Teeth,
            SizeField::Diameter,
            SizeField::PressureAngle,
            SizeField::WholeDepth,
            SizeField::RootFillet,
            SizeField::Bore,
        ]
    );
    assert_eq!(gear.rows[0].text, "24");
    assert_eq!(gear.rows[1].label, "Outside diameter");
    assert_eq!(gear.rows[1].text, "40");
    assert_eq!(gear.rows[5].text, "6");
    assert_eq!(
        gear.choices
            .iter()
            .map(|choice| choice.label.as_str())
            .collect::<Vec<_>>(),
        ["Flanks", "Root", "Crest"]
    );
    assert_eq!(gear.note.as_deref(), Some("Module 1.54"));

    // A path keeps its nodes, not sizes; an imported outline has neither.
    assert_eq!(
        shape::section(app.project(), app.strings(), &id("i4")),
        None
    );
    assert_eq!(
        shape::section(app.project(), app.strings(), &id("i1")),
        None
    );
    assert_eq!(
        shape::section(app.project(), app.strings(), &id("nope")),
        None
    );
}

/// Acceptance 115: setting each size writes `shape` and the outline it
/// gives, about the shape's own centre, in one undo step that names the
/// shape and the size.
#[test]
fn a_size_is_one_undo_step_named_for_it() {
    let mut harness = harness();
    let rectangle = |width_mm: f64, height_mm: f64, corner: Corner| Shape::Rectangle {
        width_mm,
        height_mm,
        corner,
    };
    let cases: Vec<(&str, SizeField, &str, &str, Shape)> = vec![
        (
            "i2",
            SizeField::Width,
            "80",
            "Set width of “Rectangle 1”",
            rectangle(80.0, 30.0, Corner::Square),
        ),
        (
            "i2",
            SizeField::Height,
            "40",
            "Set height of “Rectangle 1”",
            rectangle(80.0, 40.0, Corner::Square),
        ),
        (
            "i5",
            SizeField::CornerSize,
            "8",
            "Set corner size of “Rectangle 2”",
            rectangle(60.0, 30.0, Corner::Rounded { radius_mm: 8.0 }),
        ),
        (
            "i6",
            SizeField::CornerSize,
            "2.5",
            "Set corner size of “Rectangle 3”",
            rectangle(60.0, 30.0, Corner::Chamfered { distance_mm: 2.5 }),
        ),
        (
            "i3",
            SizeField::Diameter,
            "25",
            "Set diameter of “Circle 1”",
            Shape::Circle { diameter_mm: 25.0 },
        ),
        (
            "i7",
            SizeField::Width,
            "50",
            "Set width of “Ellipse 1”",
            Shape::Ellipse {
                width_mm: 50.0,
                height_mm: 20.0,
            },
        ),
        (
            "i7",
            SizeField::Height,
            "10",
            "Set height of “Ellipse 1”",
            Shape::Ellipse {
                width_mm: 50.0,
                height_mm: 10.0,
            },
        ),
        (
            "i8",
            SizeField::Sides,
            "8",
            "Set sides of “Hexagon 1”",
            Shape::Polygon {
                sides: 8,
                radius_mm: 10.0,
                measured: Measured::Flats,
            },
        ),
        (
            "i8",
            SizeField::Radius,
            "12",
            "Set radius of “Hexagon 1”",
            Shape::Polygon {
                sides: 8,
                radius_mm: 12.0,
                measured: Measured::Flats,
            },
        ),
        // Acceptance 110: a new width keeps both centres, a new length
        // the slot's middle: the placement is not touched.
        (
            "i9",
            SizeField::SlotWidth,
            "8",
            "Set width of “Slot 1”",
            Shape::Slot {
                length_mm: 40.0,
                width_mm: 8.0,
            },
        ),
        (
            "i9",
            SizeField::Length,
            "50",
            "Set length of “Slot 1”",
            Shape::Slot {
                length_mm: 50.0,
                width_mm: 8.0,
            },
        ),
    ];
    for (name, field, text, label, want) in cases {
        let before = harness.app.project().clone();
        let steps = harness.app.history().undo_labels().count();
        assert_eq!(
            type_size(&mut harness, name, field, text).ok().as_deref(),
            Some(label),
            "{name} {field:?}"
        );
        assert_eq!(shape_of(&harness.app, name), want);
        assert_outline_is_the_shapes(&harness.app, name);
        assert_eq!(
            instance(harness.app.project(), name).transform,
            instance(&before, name).transform,
            "about its own centre"
        );
        assert_eq!(harness.app.history().undo_labels().count(), steps + 1);
        assert_eq!(harness.app.undo_label(), Some(label));
        // The field reads what was typed.
        let shown = section(&harness.app, name);
        let shown = shown.rows.iter().find(|row| row.field == field).unwrap();
        assert_eq!(shown.text, text);
    }

    // The gear's, each checked against the gear it gives.
    let gear_cases = [
        (SizeField::Teeth, "30", "Set teeth of “Gear 1”"),
        (
            SizeField::Diameter,
            "50",
            "Set outside diameter of “Gear 1”",
        ),
        (
            SizeField::PressureAngle,
            "25",
            "Set pressure angle of “Gear 1”",
        ),
        (SizeField::WholeDepth, "3", "Set whole depth of “Gear 1”"),
        (SizeField::RootFillet, "0.3", "Set root fillet of “Gear 1”"),
        (SizeField::Bore, "8", "Set bore diameter of “Gear 1”"),
    ];
    for (field, text, label) in gear_cases {
        assert_eq!(
            type_size(&mut harness, "i10", field, text).ok().as_deref(),
            Some(label)
        );
        assert_outline_is_the_shapes(&harness.app, "i10");
    }
    let Shape::Gear {
        teeth,
        outside_diameter_mm,
        pressure_angle_deg,
        whole_depth_mm,
        root_fillet_mm,
        bore_diameter_mm,
        ..
    } = shape_of(&harness.app, "i10")
    else {
        panic!("a gear");
    };
    assert_eq!(
        (teeth, outside_diameter_mm, pressure_angle_deg),
        (30, 50.0, 25.0)
    );
    assert_eq!(
        (whole_depth_mm, root_fillet_mm, bore_diameter_mm),
        (3.0, 0.3, Some(8.0))
    );
    assert_eq!(definition(harness.app.project(), "i10").profiles.len(), 2);
    // An empty bore, or 0, takes the bore away.
    assert!(type_size(&mut harness, "i10", SizeField::Bore, "").is_ok());
    assert_eq!(definition(harness.app.project(), "i10").profiles.len(), 1);
    assert_eq!(section(&harness.app, "i10").rows[5].text, "");

    // Undo puts a size back, outline and all.
    let after = harness.app.project().clone();
    harness.app.undo();
    assert_eq!(definition(harness.app.project(), "i10").profiles.len(), 2);
    harness.app.redo();
    assert_eq!(harness.app.project(), &after);
}

/// The choices of the Shape section: a rectangle's corners, what a
/// polygon's radius reaches, a gear's forms.
#[test]
fn a_choice_is_one_undo_step_too() {
    let mut harness = harness();
    // Square to rounded takes the default corner, 5 mm.
    assert_eq!(
        choose(
            &mut harness,
            "i2",
            ToolOption::Corners(CornerStyle::Rounded)
        )
        .ok()
        .as_deref(),
        Some("Set corners of “Rectangle 1”")
    );
    assert_eq!(
        shape_of(&harness.app, "i2"),
        Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Rounded { radius_mm: 5.0 },
        }
    );
    assert_outline_is_the_shapes(&harness.app, "i2");
    // Rounded to chamfered keeps the size; back to square drops it.
    type_size(&mut harness, "i2", SizeField::CornerSize, "3").unwrap();
    choose(
        &mut harness,
        "i2",
        ToolOption::Corners(CornerStyle::Chamfered),
    )
    .unwrap();
    assert_eq!(
        shape_of(&harness.app, "i2"),
        Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Chamfered { distance_mm: 3.0 },
        }
    );
    choose(&mut harness, "i2", ToolOption::Corners(CornerStyle::Square)).unwrap();
    assert_eq!(rows(&harness.app, "i2").len(), 2);
    assert!(matches!(
        choose(&mut harness, "i2", ToolOption::Corners(CornerStyle::Square)),
        Err(CommandError::NothingToDo)
    ));

    // The polygon stays the polygon it is: its radius is re-read to
    // where it now reaches.
    let before = definition(harness.app.project(), "i8").profiles.clone();
    assert_eq!(
        choose(&mut harness, "i8", ToolOption::Measured(Measured::Corners))
            .ok()
            .as_deref(),
        Some("Set measure of “Hexagon 1”")
    );
    let Shape::Polygon {
        radius_mm,
        measured,
        ..
    } = shape_of(&harness.app, "i8")
    else {
        panic!("a polygon");
    };
    assert_eq!(measured, Measured::Corners);
    assert!((radius_mm - 10.0 / (PI / 6.0).cos()).abs() < 1e-9);
    let after = &definition(harness.app.project(), "i8").profiles;
    for (was, is) in before[0].vertices().iter().zip(after[0].vertices()) {
        assert!((was.x - is.x).abs() < 1e-9 && (was.y - is.y).abs() < 1e-9);
    }
    assert_outline_is_the_shapes(&harness.app, "i8");

    for (option, label) in [
        (ToolOption::Root(GearRoot::Flat), "Set root of “Gear 1”"),
        (
            ToolOption::Crest(GearCrest::Rounded),
            "Set crest of “Gear 1”",
        ),
        (
            ToolOption::Flanks(GearFlanks::Straight),
            "Set flanks of “Gear 1”",
        ),
    ] {
        assert_eq!(
            choose(&mut harness, "i10", option).ok().as_deref(),
            Some(label)
        );
        assert_outline_is_the_shapes(&harness.app, "i10");
    }
    let fields: Vec<SizeField> = section(&harness.app, "i10")
        .rows
        .iter()
        .map(|row| row.field)
        .collect();
    assert_eq!(
        fields,
        [
            SizeField::Teeth,
            SizeField::Diameter,
            SizeField::WholeDepth,
            SizeField::CrestRadius,
            SizeField::Bore,
        ]
    );
    // A choice a shape does not have is nothing to do.
    assert!(matches!(
        choose(
            &mut harness,
            "i3",
            ToolOption::Corners(CornerStyle::Rounded)
        ),
        Err(CommandError::NothingToDo)
    ));
}

/// A size the shape cannot have is refused with the reason, and nothing
/// changes; a corner is held to half the shorter side.
#[test]
fn a_size_a_shape_cannot_have_is_refused() {
    let mut harness = harness();
    let before = harness.app.project().clone();
    assert_eq!(
        refusal(type_size(&mut harness, "i2", SizeField::Width, "0")),
        "a size must be greater than 0"
    );
    assert_eq!(
        refusal(type_size(&mut harness, "i2", SizeField::Width, "-4")),
        "a size must be greater than 0"
    );
    assert!(type_size(&mut harness, "i2", SizeField::Width, "wide").is_err());
    assert_eq!(
        refusal(type_size(&mut harness, "i8", SizeField::Sides, "2")),
        "the number of sides must be 3 to 50"
    );
    assert_eq!(
        refusal(type_size(&mut harness, "i8", SizeField::Sides, "51")),
        "the number of sides must be 3 to 50"
    );
    assert_eq!(
        refusal(type_size(&mut harness, "i8", SizeField::Sides, "six")),
        "`six` is not a whole number"
    );
    assert_eq!(
        refusal(type_size(&mut harness, "i10", SizeField::Teeth, "4")),
        "the tooth count must be 6 to 200"
    );
    assert_eq!(
        refusal(type_size(&mut harness, "i10", SizeField::Bore, "35")),
        "the bore diameter must be smaller than the root diameter, 33.07"
    );
    // A size the shape does not have, and the value it has already.
    assert!(matches!(
        type_size(&mut harness, "i3", SizeField::Width, "10"),
        Err(CommandError::NothingToDo)
    ));
    assert!(matches!(
        type_size(&mut harness, "i3", SizeField::Diameter, "20"),
        Err(CommandError::NothingToDo)
    ));
    assert!(type_size(&mut harness, "i4", SizeField::Width, "10").is_err());
    assert_eq!(harness.app.project(), &before);

    // A locked shape's sizes are read-only, and so are a locked copy's.
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i3")],
            visible: None,
            locked: Some(true),
        })
        .unwrap();
    assert!(section(&harness.app, "i3").read_only);
    assert!(matches!(
        type_size(&mut harness, "i3", SizeField::Diameter, "30"),
        Err(CommandError::Locked { .. })
    ));

    // The corner is held to half the shorter side, typed or squeezed.
    type_size(&mut harness, "i5", SizeField::CornerSize, "40").unwrap();
    assert_eq!(
        shape_of(&harness.app, "i5"),
        Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Rounded { radius_mm: 15.0 },
        }
    );
    type_size(&mut harness, "i5", SizeField::Height, "10").unwrap();
    assert_eq!(
        shape_of(&harness.app, "i5"),
        Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 10.0,
            corner: Corner::Rounded { radius_mm: 5.0 },
        }
    );
    assert_outline_is_the_shapes(&harness.app, "i5");
}

/// Acceptance 115: a linked copy changes with it, whichever copy the size
/// was typed for; the step names that copy.
#[test]
fn linked_copies_follow() {
    let mut harness = harness();
    harness
        .run(Command::DuplicateInstances(vec![id("i2")]))
        .unwrap();
    // The copy is filed in the rectangle's group, beside it.
    let copy = harness
        .app
        .project()
        .instances
        .iter()
        .find(|instance| instance.name == "Rectangle 1 (2)")
        .expect("the copy")
        .clone();
    assert_eq!(
        type_size(&mut harness, copy.id.as_str(), SizeField::Width, "80")
            .ok()
            .as_deref(),
        Some("Set width of “Rectangle 1 (2)”")
    );
    let project = harness.app.project();
    assert_eq!(
        instance(project, "i2").definition,
        instance(project, copy.id.as_str()).definition
    );
    assert_eq!(rows(&harness.app, "i2")[0].2, "80");
    assert_eq!(rows(&harness.app, copy.id.as_str())[0].2, "80");
    assert_outline_is_the_shapes(&harness.app, "i2");

    // A locked copy holds the shape for them all.
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![copy.id.clone()],
            visible: None,
            locked: Some(true),
        })
        .unwrap();
    assert!(section(&harness.app, "i2").read_only);
    let refused = type_size(&mut harness, "i2", SizeField::Width, "90");
    assert!(
        matches!(&refused, Err(CommandError::Locked { names }) if names == &["Rectangle 1 (2)"]),
        "{refused:?}"
    );
}

/// Acceptance 115: the size shown for a shape scaled ×2 is twice the
/// drawn size, and typing it writes half. A shape whose placement is not
/// an even scale (a file's, since no command makes one) reads its drawn
/// sizes and takes none.
#[test]
fn sizes_read_as_placed() {
    let mut harness = harness();
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i3")],
            delta: Transform::scale(2.0, 2.0),
        })
        .unwrap();
    assert_eq!(
        shape_of(&harness.app, "i3"),
        Shape::Circle { diameter_mm: 20.0 }
    );
    assert_eq!(rows(&harness.app, "i3")[0].2, "40");
    type_size(&mut harness, "i3", SizeField::Diameter, "30").unwrap();
    assert_eq!(
        shape_of(&harness.app, "i3"),
        Shape::Circle { diameter_mm: 15.0 }
    );
    assert_eq!(rows(&harness.app, "i3")[0].2, "30");

    // A gear's lengths all scale, its counts and angles do not.
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i10")],
            delta: Transform::scale(0.5, 0.5),
        })
        .unwrap();
    let gear = section(&harness.app, "i10");
    assert_eq!(gear.rows[0].text, "24");
    assert_eq!(gear.rows[1].text, "20");
    assert_eq!(gear.rows[2].text, "20");
    assert_eq!(gear.rows[5].text, "3");
    assert_eq!(gear.note.as_deref(), Some("Module 0.77"));
    // A mirrored shape reads its sizes as a turned one does.
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i9")],
            delta: Transform::scale(-1.0, 1.0),
        })
        .unwrap();
    assert_eq!(rows(&harness.app, "i9")[0].2, "40");

    // Stretched in a file: read-only, and said.
    let mut project = drawing_project();
    let stretched = project
        .instances
        .iter_mut()
        .find(|i| i.id.as_str() == "i8")
        .unwrap();
    stretched.transform = Transform::scale(2.0, 1.0).then(stretched.transform);
    let mut harness = Harness::from_project(project);
    let hexagon = section(&harness.app, "i8");
    assert!(hexagon.read_only);
    assert_eq!(
        hexagon.note.as_deref(),
        Some("Stretched: convert to a path to edit it")
    );
    assert_eq!(hexagon.rows[1].text, "10", "as it was drawn");
    assert_eq!(
        refusal(type_size(&mut harness, "i8", SizeField::Radius, "12")),
        "“Hexagon 1” is stretched: convert it to a path to edit its sizes"
    );
}

/// Acceptance 115: stretching an unrotated rectangle to W 80 changes its
/// width and leaves its corner size and its transform's scale alone. An
/// ellipse and a slot take a stretch the same way, a shape turned a
/// quarter takes it on its other size, and several stretch about their
/// joint centre.
#[test]
fn stretching_a_rectangle_changes_its_sizes() {
    let mut harness = harness();
    let before = instance(harness.app.project(), "i5").transform;
    assert_eq!(
        type_placement(&mut harness, &["i5"], Field::W, "80", false)
            .ok()
            .as_deref(),
        Some("Scale “Rectangle 2”")
    );
    assert_eq!(
        shape_of(&harness.app, "i5"),
        Shape::Rectangle {
            width_mm: 80.0,
            height_mm: 30.0,
            corner: Corner::Rounded { radius_mm: 5.0 },
        }
    );
    assert_eq!(
        instance(harness.app.project(), "i5").transform,
        before,
        "the centre stays and nothing is scaled"
    );
    assert_outline_is_the_shapes(&harness.app, "i5");
    // One undo step, which puts the sizes back.
    harness.app.undo();
    assert_eq!(rows(&harness.app, "i5")[0].2, "60");
    harness.app.redo();

    // Turned a quarter, the box's width is the rectangle's height.
    harness
        .run(Command::RotateInstances {
            ids: vec![id("i5")],
            angle_deg: 90.0,
        })
        .unwrap();
    type_placement(&mut harness, &["i5"], Field::W, "40", false).unwrap();
    let Shape::Rectangle {
        width_mm,
        height_mm,
        ..
    } = shape_of(&harness.app, "i5")
    else {
        panic!("a rectangle");
    };
    assert!((width_mm - 80.0).abs() < 1e-9 && (height_mm - 40.0).abs() < 1e-9);
    assert_outline_is_the_shapes(&harness.app, "i5");
    // The link closed scales it evenly, as any shape: sizes as placed.
    type_placement(&mut harness, &["i5"], Field::W, "20", true).unwrap();
    assert_eq!(rows(&harness.app, "i5")[0].2, "40");
    assert_eq!(rows(&harness.app, "i5")[1].2, "20");

    // An ellipse.
    type_placement(&mut harness, &["i7"], Field::H, "30", false).unwrap();
    assert_eq!(
        shape_of(&harness.app, "i7"),
        Shape::Ellipse {
            width_mm: 40.0,
            height_mm: 30.0,
        }
    );
    assert_outline_is_the_shapes(&harness.app, "i7");

    // Two at once, about their joint centre: each keeps its share of the
    // box.
    let joint = |app: &App| properties::placement(app.project(), &[id("i2"), id("i6")]).unwrap();
    let was = joint(&harness.app).bounds;
    type_placement(&mut harness, &["i2", "i6"], Field::W, "200", false).unwrap();
    let is = joint(&harness.app).bounds;
    assert!((is.width_mm() - 200.0).abs() < 1e-9);
    assert!((is.height_mm() - was.height_mm()).abs() < 1e-9);
    let centre = |r: dc_geometry::Rect| (r.min.x + r.max.x) / 2.0;
    assert!((centre(is) - centre(was)).abs() < 1e-9);
    let factor = 200.0 / was.width_mm();
    assert_eq!(rows(&harness.app, "i2")[1].2, "30");
    let Shape::Rectangle { width_mm, .. } = shape_of(&harness.app, "i2") else {
        panic!("a rectangle");
    };
    assert!((width_mm - 60.0 * factor).abs() < 1e-9);
    assert_outline_is_the_shapes(&harness.app, "i2");
    assert_outline_is_the_shapes(&harness.app, "i6");

    // A slot square to the axes: across is its width, along is its
    // length less the two half circles.
    let mut project = drawing_project();
    project
        .instances
        .iter_mut()
        .find(|i| i.id.as_str() == "i9")
        .unwrap()
        .transform = Transform::translate(150.0, 175.0);
    let mut harness = Harness::from_project(project);
    type_placement(&mut harness, &["i9"], Field::H, "20", false).unwrap();
    assert_eq!(
        shape_of(&harness.app, "i9"),
        Shape::Slot {
            length_mm: 32.0,
            width_mm: 20.0,
        }
    );
    type_placement(&mut harness, &["i9"], Field::W, "100", false).unwrap();
    assert_eq!(
        shape_of(&harness.app, "i9"),
        Shape::Slot {
            length_mm: 80.0,
            width_mm: 20.0,
        }
    );
    assert_outline_is_the_shapes(&harness.app, "i9");
    // Squeezed until it is no longer than it is wide, it is no slot.
    assert_eq!(
        refusal(type_placement(&mut harness, &["i9"], Field::W, "15", false)),
        "“Slot 1” scales evenly only: a slot is longer than it is wide"
    );
}

/// Acceptance 115: stretching a circle makes an ellipse.
#[test]
fn a_circle_stretched_is_an_ellipse() {
    let mut harness = harness();
    let before = instance(harness.app.project(), "i3").clone();
    type_placement(&mut harness, &["i3"], Field::H, "30", false).unwrap();
    assert_eq!(
        shape_of(&harness.app, "i3"),
        Shape::Ellipse {
            width_mm: 20.0,
            height_mm: 30.0,
        }
    );
    let after = instance(harness.app.project(), "i3");
    assert_eq!(after.transform, before.transform);
    assert_eq!(after.name, "Circle 1", "its name is the user's");
    assert_eq!(section(&harness.app, "i3").title, "Ellipse");
    assert_outline_is_the_shapes(&harness.app, "i3");
}

/// Acceptance 115: a polygon and a gear scale evenly or not at all, and
/// so does a shape that is not square to the axes.
#[test]
fn a_polygon_refuses_a_stretch() {
    let mut harness = harness();
    let before = harness.app.project().clone();
    assert_eq!(
        refusal(type_placement(&mut harness, &["i8"], Field::W, "30", false)),
        "“Hexagon 1” scales evenly only: a polygon keeps its proportions"
    );
    assert_eq!(
        refusal(type_placement(
            &mut harness,
            &["i10"],
            Field::H,
            "30",
            false
        )),
        "“Gear 1” scales evenly only: a gear keeps its proportions"
    );
    // The slot is turned 30°.
    assert_eq!(
        refusal(type_placement(&mut harness, &["i9"], Field::W, "30", false)),
        "“Slot 1” scales evenly only: it is not square to the axes"
    );
    // One that cannot holds back the others selected with it.
    assert!(type_placement(&mut harness, &["i2", "i8"], Field::W, "300", false).is_err());
    assert_eq!(harness.app.project(), &before);
    // Evenly, each scales.
    assert!(type_placement(&mut harness, &["i8"], Field::W, "30", true).is_ok());
    assert!(type_placement(&mut harness, &["i9"], Field::W, "30", true).is_ok());
    assert_eq!(
        shape_of(&harness.app, "i8"),
        definition(&before, "i8").shape.clone().unwrap()
    );
    // A path's lines stretch through its placement, as they always have.
    assert!(type_placement(&mut harness, &["i1"], Field::W, "30", false).is_ok());
}

/// Acceptance 115: stretching a linked copy is refused with its reason.
#[test]
fn a_linked_copy_refuses_a_stretch() {
    let mut harness = harness();
    harness
        .run(Command::DuplicateInstances(vec![id("i2")]))
        .unwrap();
    let copy = harness.app.project().instances.last().unwrap().id.clone();
    let before = harness.app.project().clone();
    let refused = refusal(type_placement(&mut harness, &["i2"], Field::W, "80", false));
    assert_eq!(
        refused,
        "Make Unique to stretch this copy: “Rectangle 1” shares its shape with 1 other"
    );
    assert!(type_placement(&mut harness, &[copy.as_str()], Field::H, "80", false).is_err());
    // Both together are still two copies of one shape.
    assert!(type_placement(&mut harness, &["i2", copy.as_str()], Field::W, "300", false).is_err());
    assert_eq!(harness.app.project(), &before);
    // Evenly, a copy scales alone, as before.
    assert!(type_placement(&mut harness, &["i2"], Field::W, "80", true).is_ok());
    assert_eq!(
        shape_of(&harness.app, "i2"),
        definition(&before, "i2").shape.clone().unwrap()
    );
}

/// Acceptance 115: Convert to Path leaves the outline equal and `shape`
/// `null`, for the definition and so for its linked copies, in one undo
/// step.
#[test]
fn convert_to_path_keeps_the_outline() {
    let mut harness = harness();
    harness
        .run(Command::DuplicateInstances(vec![id("i5")]))
        .unwrap();
    let copy = harness.app.project().instances.last().unwrap().id.clone();
    let before = harness.app.project().clone();
    assert_eq!(
        harness
            .run(Command::ConvertToPath(vec![id("i5")]))
            .ok()
            .as_deref(),
        Some("Convert “Rectangle 2” to a path")
    );
    let project = harness.app.project();
    assert_eq!(definition(project, "i5").shape, None);
    assert_eq!(
        definition(project, "i5").profiles,
        definition(&before, "i5").profiles
    );
    assert_eq!(project.instances, before.instances);
    assert_eq!(
        shape::section(project, harness.app.strings(), &id("i5")),
        None
    );
    assert_eq!(shape::section(project, harness.app.strings(), &copy), None);
    harness.app.undo();
    assert_eq!(harness.app.project(), &before);

    // Several at once; the ones that are paths already are left as they
    // are, curves and all.
    let label = harness
        .run(Command::ConvertToPath(vec![
            id("i2"),
            id("i8"),
            id("i4"),
            id("i1"),
        ]))
        .unwrap();
    assert_eq!(label, "Convert 2 shapes to paths");
    let project = harness.app.project();
    assert_eq!(definition(project, "i2").shape, None);
    assert_eq!(definition(project, "i8").shape, None);
    assert_eq!(
        definition(project, "i4").shape,
        definition(&before, "i4").shape
    );
    assert!(matches!(
        harness.run(Command::ConvertToPath(vec![id("i4"), id("i1")])),
        Err(CommandError::NothingToDo)
    ));
    assert!(matches!(
        harness.run(Command::ConvertToPath(Vec::new())),
        Err(CommandError::NothingSelected)
    ));
    // A converted polygon is lines, and stretches as a path does.
    assert!(type_placement(&mut harness, &["i8"], Field::W, "30", false).is_ok());
}

/// A tab keeps its place on the outline when the outline changes under
/// it: at the same share of the shape's box, on the nearest point of the
/// new outline.
#[test]
fn tabs_follow_the_outline() {
    let mut harness = harness();
    let tab = |name: &str, x_mm: f64, y_mm: f64| PlacedTab {
        id: TabId::new(name),
        instance: id("i2"),
        profile: 0,
        x_mm,
        y_mm,
        width_mm: 6.0,
        height_mm: 2.0,
        shape: TabShape::Rect,
        holds_slug: None,
    };
    // The middle of the right side, and a point of the top side.
    harness
        .run(Command::AddTab(tab("right", 30.0, 0.0)))
        .unwrap();
    harness
        .run(Command::AddTab(tab("top", 15.0, 15.0)))
        .unwrap();
    let at = |app: &App, name: &str| {
        let tab = app
            .project()
            .tabs
            .iter()
            .find(|t| t.id.as_str() == name)
            .unwrap();
        Point::new(tab.x_mm, tab.y_mm)
    };
    #[track_caller]
    fn assert_at(got: Point, x: f64, y: f64) {
        assert!(
            (got.x - x).abs() < 1e-9 && (got.y - y).abs() < 1e-9,
            "{got:?}"
        );
    }

    type_size(&mut harness, "i2", SizeField::Width, "80").unwrap();
    assert_at(at(&harness.app, "right"), 40.0, 0.0);
    assert_at(at(&harness.app, "top"), 20.0, 15.0);
    // Stretched by its box, the same.
    type_placement(&mut harness, &["i2"], Field::H, "60", false).unwrap();
    assert_at(at(&harness.app, "right"), 40.0, 0.0);
    assert_at(at(&harness.app, "top"), 20.0, 30.0);
    // Rounded corners: a tab in a corner's reach lands on the arc.
    harness
        .run(Command::AddTab(tab("corner", 40.0, 30.0)))
        .unwrap();
    choose(
        &mut harness,
        "i2",
        ToolOption::Corners(CornerStyle::Rounded),
    )
    .unwrap();
    let corner = at(&harness.app, "corner");
    let centre = Point::new(35.0, 25.0);
    assert!(
        (corner.distance_to(centre) - 5.0).abs() < 1e-9,
        "{corner:?}"
    );
    assert_eq!(harness.app.project().tabs.len(), 3, "none is lost");
    harness.app.undo();
    assert_at(at(&harness.app, "corner"), 40.0, 30.0);
}
