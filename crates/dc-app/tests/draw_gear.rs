// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/gearCreation.test.ts

//! Slice 7 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Drawing tools"): the Gear tool, driven as the window drives it
//! (functional capability 86; acceptance 111).
//!
//! `gearCreation.test.ts`'s cases that are about the tool are ported: a
//! gear is one shape, its bore a second outline of it, placed by its
//! centre, named *Gear N*, one undo step, and the tool arms again.
//! Upstream's cases about its add and subtract operations have no
//! counterpart: a DreamCarve shape has none until an operation cuts it.
//! The gear's own geometry is tested where it lives
//! (`dc_geometry::shapes::gear`).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key, Quantity, SizeField, ToolBar, ToolOption};
use dc_app::{App, Drawing, Pointer, Tool};
use dc_geometry::shapes::gear::{Gear, GearCrest, GearFlanks, GearRoot};
use dc_geometry::{Point, Transform};
use dc_project::{FeatureDefinition, FeatureInstance, Shape, Units};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// A blank project with the Gear tool armed and snapping off.
fn armed() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(Drawing::Gear));
    app
}

fn move_to(app: &mut App, at: Point) {
    app.tool_event(Event::Move(Pointer::at(at)));
}

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

fn bar(app: &App) -> ToolBar {
    app.tool_bar().expect("the Gear tool has a bar")
}

fn fields(app: &App) -> Vec<SizeField> {
    bar(app).fields.iter().map(|row| row.field).collect()
}

fn text_of(app: &App, field: SizeField) -> String {
    let bar = bar(app);
    let row = bar.fields.iter().find(|row| row.field == field);
    row.unwrap_or_else(|| panic!("no field {field:?}"))
        .text
        .clone()
}

fn problem_of(app: &App, field: SizeField) -> Option<String> {
    let bar = bar(app);
    let row = bar.fields.iter().find(|row| row.field == field);
    row.unwrap_or_else(|| panic!("no field {field:?}"))
        .problem
        .clone()
}

/// The centre at (60, 60) and the outside radius 20, by two clicks.
fn placed() -> App {
    let mut app = armed();
    click(&mut app, pt(60.0, 60.0));
    click(&mut app, pt(80.0, 60.0));
    app
}

fn last(app: &App) -> (&FeatureInstance, &FeatureDefinition) {
    let instance = app.project().instances.last().expect("a shape was drawn");
    (instance, &app.project().definitions[&instance.definition])
}

fn outline_problems(app: &App) -> usize {
    dc_project::validate(app.project())
        .iter()
        .filter(|p| p.code() == "shape_outline_differs")
        .count()
}

/// Capability 86 and `testGearWithoutBoreIsOneCannedFeature`: a click at
/// the centre and one for the outside radius; the bar then holds the
/// gear's sizes with upstream's defaults, previewed, and **Done** makes
/// one shape named *Gear 1*, placed by its centre, in one undo step.
#[test]
fn gear_previews_and_commits() {
    let mut app = armed();
    assert_eq!(bar(&app).prompt, "Click the centre");
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert!(bar(&app).choices.is_empty());
    click(&mut app, pt(60.0, 60.0));
    assert_eq!(
        bar(&app).prompt,
        "Click a point on the outside circle, or type its diameter"
    );
    assert_eq!(fields(&app), [SizeField::Diameter]);
    assert_eq!(bar(&app).fields[0].label, "Outside diameter");

    // Before the second click the gear is previewed with its defaults.
    move_to(&mut app, pt(80.0, 60.0));
    assert_eq!(text_of(&app, SizeField::Diameter), "40");
    let default_gear = Gear::with_defaults(20.0);
    let at_centre = Transform::translate(60.0, 60.0);
    let expected: Vec<_> = default_gear
        .outline()
        .unwrap()
        .iter()
        .map(|outline| at_centre.apply_polyline(outline).unwrap())
        .collect();
    assert_eq!(app.draft().outlines, expected);
    assert_eq!(app.draft().labels[0].text, "⌀ 40");
    assert!(!bar(&app).can_finish, "the radius is not taken yet");

    assert_eq!(
        click(&mut app, pt(80.0, 60.0)),
        None,
        "the sizes, not a gear"
    );
    assert!(app.project().instances.is_empty());
    let sizes = bar(&app);
    assert_eq!(sizes.prompt, "Set the gear's sizes, then Done");
    assert_eq!(
        fields(&app),
        [
            SizeField::Teeth,
            SizeField::Diameter,
            SizeField::PressureAngle,
            SizeField::WholeDepth,
            SizeField::RootFillet,
            SizeField::Bore,
        ],
        "a flat crest has no radius"
    );
    assert_eq!(text_of(&app, SizeField::Teeth), "24");
    assert_eq!(SizeField::Teeth.quantity(), Quantity::Count);
    assert_eq!(text_of(&app, SizeField::Diameter), "40");
    assert_eq!(text_of(&app, SizeField::PressureAngle), "20");
    assert_eq!(SizeField::PressureAngle.quantity(), Quantity::Angle);
    // 2.25 modules of 40 ÷ 26, and a fillet of 0.15.
    assert_eq!(text_of(&app, SizeField::WholeDepth), "3.4615");
    assert_eq!(text_of(&app, SizeField::RootFillet), "0.2308");
    assert_eq!(text_of(&app, SizeField::Bore), "", "no bore");
    assert_eq!(sizes.note.as_deref(), Some("Module 1.54"));
    let choices: Vec<(String, Vec<String>, usize)> = sizes
        .choices
        .iter()
        .map(|choice| {
            (
                choice.label.clone(),
                choice.options.iter().map(|o| o.label.clone()).collect(),
                choice.selected,
            )
        })
        .collect();
    let names = |names: &[&str]| names.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
    assert_eq!(
        choices,
        [
            ("Flanks".to_owned(), names(&["Involute", "Straight"]), 0),
            ("Root".to_owned(), names(&["Rounded", "Flat", "Sharp"]), 0),
            ("Crest".to_owned(), names(&["Flat", "Rounded"]), 0),
        ]
    );
    assert_eq!(
        sizes.choices[1].options[1].option,
        ToolOption::Root(GearRoot::Flat)
    );
    assert!(sizes.can_finish);
    // The preview stays where the gear is while the pointer wanders, and
    // a click in the Viewport changes nothing.
    move_to(&mut app, pt(150.0, 10.0));
    assert_eq!(app.draft().outlines, expected);
    assert_eq!(click(&mut app, pt(150.0, 10.0)), None);
    assert_eq!(text_of(&app, SizeField::Diameter), "40");

    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Draw “Gear 1”"));
    let (instance, definition) = last(&app);
    assert_eq!(instance.name, "Gear 1");
    assert_eq!(instance.transform, at_centre, "placed by its centre");
    assert_eq!(definition.shape, Some(Shape::from_gear(&default_gear)));
    assert_eq!(definition.profiles, default_gear.outline().unwrap());
    assert_eq!(definition.profiles.len(), 1, "no bore, one outline");
    assert_eq!(outline_problems(&app), 0);
    assert_eq!(app.undo_label(), Some("Draw “Gear 1”"));
    let saved = app.serialize().unwrap();
    assert_eq!(
        &dc_project::load(saved.as_bytes()).unwrap().project,
        app.project()
    );
    // The tool arms again, with nothing begun.
    assert_eq!(app.tool(), Tool::Draw(Drawing::Gear));
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert!(app.draft().outlines.is_empty());
    app.undo();
    assert!(app.project().instances.is_empty() && app.project().definitions.is_empty());
}

/// `testAddGearHasSubtractBoreInTheSameFeature`, less the operations: a
/// gear with a bore is one shape of two outlines, the bore a circle of
/// its diameter about the centre. The sizes are typed, and the forms
/// chosen, in the bar; `Enter` commits as **Done** does.
#[test]
fn gear_sizes_are_typed() {
    let mut app = armed();
    // The centre and the diameter typed: (25, 30) from the origin.
    let origin = {
        let project = app.project();
        project.origin.xy.point_mm(&project.stock)
    };
    type_size(&mut app, SizeField::X, "25");
    type_size(&mut app, SizeField::Y, "30");
    app.tool_event(Event::Key(Key::Enter));
    type_size(&mut app, SizeField::Diameter, "40");
    assert_eq!(app.tool_event(Event::Key(Key::Enter)).ran, None);
    assert_eq!(text_of(&app, SizeField::Teeth), "24");

    type_size(&mut app, SizeField::Teeth, "30");
    assert_eq!(text_of(&app, SizeField::Teeth), "30");
    assert_eq!(bar(&app).note.as_deref(), Some("Module 1.25"));
    // The depth and the fillet are the ones the first radius gave: a new
    // tooth count does not re-derive them (`setPendingGearRadiusAt`).
    assert_eq!(text_of(&app, SizeField::WholeDepth), "3.4615");
    type_size(&mut app, SizeField::WholeDepth, "2.8");
    type_size(&mut app, SizeField::PressureAngle, "25");
    type_size(&mut app, SizeField::RootFillet, "0.2");
    type_size(&mut app, SizeField::Bore, "6");
    assert_eq!(app.draft().outlines.len(), 2, "the gear and its bore");

    // The forms: a field shows only while its form reads it.
    app.tool_event(Event::Option(ToolOption::Crest(GearCrest::Rounded)));
    assert!(fields(&app).contains(&SizeField::CrestRadius));
    assert_eq!(bar(&app).choices[2].selected, 1);
    type_size(&mut app, SizeField::CrestRadius, "0.1");
    app.tool_event(Event::Option(ToolOption::Root(GearRoot::Sharp)));
    assert!(!fields(&app).contains(&SizeField::RootFillet));
    app.tool_event(Event::Option(ToolOption::Flanks(GearFlanks::Straight)));
    assert!(!fields(&app).contains(&SizeField::PressureAngle));
    app.tool_event(Event::Option(ToolOption::Flanks(GearFlanks::Involute)));
    assert_eq!(text_of(&app, SizeField::PressureAngle), "25");

    let reply = app.tool_event(Event::Key(Key::Enter));
    assert_eq!(reply.ran.as_deref(), Some("Draw “Gear 1”"));
    let gear = Gear {
        teeth: 30,
        outside_radius_mm: 20.0,
        whole_depth_mm: 2.8,
        flanks: GearFlanks::Involute,
        pressure_angle_deg: 25.0,
        root: GearRoot::Sharp,
        root_fillet_mm: 0.2,
        crest: GearCrest::Rounded,
        crest_radius_mm: 0.1,
        bore_diameter_mm: Some(6.0),
    };
    let (instance, definition) = last(&app);
    assert_eq!(definition.shape, Some(Shape::from_gear(&gear)));
    assert_eq!(
        instance.transform,
        Transform::translate(origin.x + 25.0, origin.y + 30.0)
    );
    assert_eq!(definition.profiles.len(), 2);
    let bore = definition.profiles[1].bounds().unwrap();
    assert!((bore.width_mm() - 6.0).abs() < 1e-9 && (bore.height_mm() - 6.0).abs() < 1e-9);
    assert!((bore.min.x + 3.0).abs() < 1e-9, "about the centre");
    assert!(definition.profiles[0].signed_area_mm2() > 0.0);
    assert!(definition.profiles[1].signed_area_mm2() < 0.0, "a hole");
    assert_eq!(outline_problems(&app), 0);

    // A bore of 0, or none typed, is no bore.
    click(&mut app, pt(60.0, 60.0));
    click(&mut app, pt(80.0, 60.0));
    type_size(&mut app, SizeField::Bore, "6");
    type_size(&mut app, SizeField::Bore, "0");
    assert_eq!(text_of(&app, SizeField::Bore), "");
    assert_eq!(app.draft().outlines.len(), 1);
    type_size(&mut app, SizeField::Bore, "6");
    type_size(&mut app, SizeField::Bore, " ");
    assert_eq!(app.draft().outlines.len(), 1);
    // An empty field elsewhere leaves the size as it was.
    type_size(&mut app, SizeField::Teeth, "");
    assert_eq!(text_of(&app, SizeField::Teeth), "24");

    // An inch project reads and shows inches.
    let mut project = App::new_project().unwrap();
    project.display_units = Units::Inch;
    let mut app = Harness::from_project(project).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(Drawing::Gear));
    click(&mut app, pt(50.0, 50.0));
    type_size(&mut app, SizeField::Diameter, "2");
    app.tool_event(Event::Key(Key::Enter));
    assert_eq!(text_of(&app, SizeField::Diameter), "2");
    type_size(&mut app, SizeField::Bore, "1/4");
    app.tool_event(Event::Done);
    let Some(Shape::Gear {
        outside_diameter_mm,
        bore_diameter_mm,
        ..
    }) = last(&app).1.shape
    else {
        panic!("a gear");
    };
    assert!((outside_diameter_mm - 50.8).abs() < 1e-9);
    assert_eq!(bore_diameter_mm, Some(6.35));
}

/// Capability 86: a size the gear cannot have is refused at its field
/// with PureCutCNC's reason, and the gear is not made while one stands.
#[test]
fn a_size_the_gear_cannot_have_is_refused_at_its_field() {
    let mut app = placed();
    type_size(&mut app, SizeField::Teeth, "4");
    assert_eq!(
        problem_of(&app, SizeField::Teeth).as_deref(),
        Some("the tooth count must be 6 to 200")
    );
    assert_eq!(text_of(&app, SizeField::Teeth), "4", "held as typed");
    assert!(!bar(&app).can_finish);
    // No gear to preview: the outside circle marks where it is.
    assert_eq!(app.draft().outlines.len(), 1);
    assert_eq!(app.draft().outlines[0].vertices().len(), 2);
    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran, None);
    assert_eq!(
        reply.refused.as_deref(),
        Some("the tooth count must be 6 to 200")
    );
    assert!(app.project().instances.is_empty());

    // Text that is no number leaves the size, and says so.
    type_size(&mut app, SizeField::Teeth, "many");
    assert_eq!(
        problem_of(&app, SizeField::Teeth).as_deref(),
        Some("`many` is not a whole number")
    );
    assert_eq!(text_of(&app, SizeField::Teeth), "4");
    type_size(&mut app, SizeField::Teeth, "24");
    assert_eq!(problem_of(&app, SizeField::Teeth), None);
    assert!(bar(&app).can_finish);

    // The limits that depend on the other sizes name what fits, in
    // project units: the root circle is 40 − 2 × 3.4615 across.
    type_size(&mut app, SizeField::Bore, "35");
    assert_eq!(
        problem_of(&app, SizeField::Bore).as_deref(),
        Some("the bore diameter must be smaller than the root diameter, 33.07")
    );
    type_size(&mut app, SizeField::Bore, "");
    type_size(&mut app, SizeField::RootFillet, "3");
    assert!(problem_of(&app, SizeField::RootFillet).is_some_and(|why| {
        why.starts_with(
            "the root fillet is too large for the tooth spacing; the largest that fits is ",
        )
    }),);
    // A flat root does not read the fillet: nothing is wrong any more.
    app.tool_event(Event::Option(ToolOption::Root(GearRoot::Flat)));
    assert!(bar(&app).can_finish);
    type_size(&mut app, SizeField::WholeDepth, "25");
    assert_eq!(
        problem_of(&app, SizeField::WholeDepth).as_deref(),
        Some("the whole depth must be smaller than the outside radius")
    );
    type_size(&mut app, SizeField::WholeDepth, "3");
    type_size(&mut app, SizeField::PressureAngle, "50");
    assert_eq!(
        problem_of(&app, SizeField::PressureAngle).as_deref(),
        Some("the pressure angle must be 10 to 35 degrees")
    );
    type_size(&mut app, SizeField::PressureAngle, "steep");
    assert!(
        problem_of(&app, SizeField::PressureAngle)
            .is_some_and(|why| why.contains("is not an angle"))
    );
    type_size(&mut app, SizeField::PressureAngle, "20");
    type_size(&mut app, SizeField::Diameter, "-4");
    assert!(problem_of(&app, SizeField::Diameter).is_some());
    type_size(&mut app, SizeField::Diameter, "40");
    assert!(app.tool_event(Event::Done).ran.is_some());
}

/// The steps back: `Backspace` one step, `Escape` out of the draft and
/// then out of the tool; a gear of no size is not begun; a drag from the
/// centre gives the radius as a second click does.
#[test]
fn gear_steps_back() {
    let mut app = placed();
    app.tool_event(Event::Key(Key::Backspace));
    assert_eq!(fields(&app), [SizeField::Diameter], "back to the radius");
    app.tool_event(Event::Key(Key::Backspace));
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);

    click(&mut app, pt(60.0, 60.0));
    assert_eq!(click(&mut app, pt(60.0, 60.0)), None);
    assert_eq!(fields(&app), [SizeField::Diameter], "no radius, no gear");
    click(&mut app, pt(80.0, 60.0));
    assert_eq!(fields(&app)[0], SizeField::Teeth);
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Draw(Drawing::Gear));
    assert_eq!(fields(&app), [SizeField::X, SizeField::Y]);
    assert!(app.draft().outlines.is_empty());
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert!(app.project().instances.is_empty());

    app.arm(Tool::Draw(Drawing::Gear));
    app.tool_event(Event::Press(Pointer::at(pt(60.0, 60.0))));
    app.tool_event(Event::Move(Pointer::at(pt(60.0, 85.0))));
    app.tool_event(Event::Release(Pointer::at(pt(60.0, 85.0))));
    assert_eq!(text_of(&app, SizeField::Diameter), "50");
    assert_eq!(fields(&app)[0], SizeField::Teeth);
    // The Construction switch marks a gear as it does any shape.
    app.tool_event(Event::Option(ToolOption::Construction(true)));
    assert!(app.tool_event(Event::Done).ran.is_some());
    assert!(last(&app).0.construction);
}
