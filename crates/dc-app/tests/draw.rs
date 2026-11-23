// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 5 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Drawing tools"): Rectangle, Circle, Ellipse, Polygon and Slot, driven
//! as the window drives them, with pointer positions in project
//! millimetres and no window (functional capabilities 83 to 85c and 90;
//! acceptance 108 to 110, 114 and 129).
//!
//! PureCutCNC has no test that drives a tool's click sequence (digest
//! §11), so these are written from the behaviour the functional spec
//! takes from it; `stickyDrawing.test.ts`'s cases are the one port.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::test_support::Harness;
use dc_app::tool::{CornerStyle, Event, Key, SizeField, ToolOption};
use dc_app::{App, Drawing, Pointer, Tool};
use dc_geometry::{Point, Polyline, Rect, Segment};
use dc_project::{Corner, FeatureDefinition, FeatureInstance, Measured, Shape, Units};

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
    click_with(app, Pointer::at(at))
}

fn click_with(app: &mut App, pointer: Pointer) -> Option<String> {
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

fn enter(app: &mut App) -> Option<String> {
    app.tool_event(Event::Key(Key::Enter)).ran
}

/// The last shape drawn: its instance and its definition.
fn last(app: &App) -> (&FeatureInstance, &FeatureDefinition) {
    let instance = app.project().instances.last().expect("a shape was drawn");
    (instance, &app.project().definitions[&instance.definition])
}

/// The last shape's outline, placed.
fn placed(app: &App) -> Polyline {
    let (instance, definition) = last(app);
    assert_eq!(definition.profiles.len(), 1);
    instance
        .transform
        .apply_polyline(&definition.profiles[0])
        .unwrap()
}

#[track_caller]
fn assert_box(outline: &Polyline, min: (f64, f64), max: (f64, f64)) {
    let Rect { min: lo, max: hi } = outline.bounds().unwrap();
    for (got, want) in [(lo.x, min.0), (lo.y, min.1), (hi.x, max.0), (hi.y, max.1)] {
        assert!((got - want).abs() < 1e-9, "box {lo:?} to {hi:?}");
    }
}

fn field_texts(app: &App) -> Vec<(SizeField, String, bool)> {
    app.tool_bar()
        .expect("a drawing tool has a bar")
        .fields
        .into_iter()
        .map(|row| (row.field, row.text, row.pinned))
        .collect()
}

/// Acceptance 108: a Rectangle from (10, 10) to (70, 40) is a definition
/// with `shape` rectangle 60 × 30, square corners, and a closed,
/// counter-clockwise outline of four lines whose box is those corners,
/// named *Rectangle 1*, one undo step named for it.
#[test]
fn rectangle_by_two_clicks() {
    let mut app = armed(Drawing::Rectangle);
    assert_eq!(click(&mut app, pt(10.0, 10.0)), None, "the first corner");
    assert!(app.project().instances.is_empty());
    let label = click(&mut app, pt(70.0, 40.0));
    assert_eq!(label.as_deref(), Some("Draw “Rectangle 1”"));
    let (instance, definition) = last(&app);
    assert_eq!(instance.name, "Rectangle 1");
    assert_eq!(definition.name, "Rectangle 1");
    assert!(instance.visible && !instance.locked && !instance.construction);
    assert_eq!(
        definition.shape,
        Some(Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Square,
        })
    );
    let outline = placed(&app);
    assert!(outline.is_closed());
    assert!(outline.signed_area_mm2() > 0.0);
    assert_eq!(outline.segments().filter(Segment::is_line).count(), 4);
    assert_box(&outline, (10.0, 10.0), (70.0, 40.0));
    // The outline is the one its sizes give: validation has nothing to say.
    assert_eq!(
        dc_project::validate(app.project())
            .iter()
            .filter(|p| p.code() == "shape_outline_differs")
            .count(),
        0
    );
    // One undo step, and the file holds it.
    assert_eq!(app.undo_label(), Some("Draw “Rectangle 1”"));
    let saved = app.serialize().unwrap();
    assert_eq!(
        &dc_project::load(saved.as_bytes()).unwrap().project,
        app.project()
    );
    app.undo();
    assert!(app.project().instances.is_empty() && app.project().definitions.is_empty());
    // Drawn the other way, the same rectangle.
    click(&mut app, pt(70.0, 40.0));
    click(&mut app, pt(10.0, 10.0));
    assert_box(&placed(&app), (10.0, 10.0), (70.0, 40.0));
}

/// A press, a drag and a release give the two corners as two clicks do
/// (Graphite's shape tool); a press and a release in one place give only
/// the first.
#[test]
fn rectangle_by_a_drag() {
    let mut app = armed(Drawing::Rectangle);
    app.tool_event(Event::Press(Pointer::at(pt(10.0, 10.0))));
    move_to(&mut app, pt(40.0, 25.0));
    assert!(app.project().instances.is_empty(), "still dragging");
    assert_eq!(app.draft().outlines.len(), 1, "the rectangle so far");
    let reply = app.tool_event(Event::Release(Pointer::at(pt(70.0, 40.0))));
    assert_eq!(reply.ran.as_deref(), Some("Draw “Rectangle 1”"));
    assert_box(&placed(&app), (10.0, 10.0), (70.0, 40.0));
    // A release within 4 px of the press (0.8 mm at 5 px to the mm) is a
    // click.
    app.tool_event(Event::Press(Pointer::at(pt(20.0, 20.0))));
    let reply = app.tool_event(Event::Release(Pointer::at(pt(20.5, 20.0))));
    assert_eq!(reply.ran, None);
    assert_eq!(app.project().instances.len(), 1);
}

/// Acceptance 108: `Shift` makes the second point (40, 40), a square;
/// `Alt` centres the rectangle on the first point.
#[test]
fn shift_squares_alt_centres() {
    let mut app = armed(Drawing::Rectangle);
    click(&mut app, pt(10.0, 10.0));
    let shift = Pointer {
        shift: true,
        ..Pointer::at(pt(40.0, 22.0))
    };
    click_with(&mut app, shift);
    assert_box(&placed(&app), (10.0, 10.0), (40.0, 40.0));
    click(&mut app, pt(50.0, 50.0));
    let alt = Pointer {
        alt: true,
        ..Pointer::at(pt(60.0, 55.0))
    };
    click_with(&mut app, alt);
    assert_box(&placed(&app), (40.0, 45.0), (60.0, 55.0));
    // Both: a square about the first point, down and to the left as the
    // pointer is.
    click(&mut app, pt(50.0, 50.0));
    let both = Pointer {
        shift: true,
        alt: true,
        ..Pointer::at(pt(42.0, 47.0))
    };
    click_with(&mut app, both);
    assert_box(&placed(&app), (42.0, 42.0), (58.0, 58.0));
}

/// Acceptance 108: with Rounded 5 the outline is four lines and four
/// arcs of radius 5; with Chamfered 5, eight lines. The option and its
/// size are kept for the next rectangle; a corner is held to half the
/// shorter side.
#[test]
fn rounded_and_chamfered_corners() {
    let mut app = armed(Drawing::Rectangle);
    assert_eq!(app.prefs().drawing.corners, CornerStyle::Square);
    app.tool_event(Event::Option(ToolOption::Corners(CornerStyle::Rounded)));
    click(&mut app, pt(10.0, 10.0));
    click(&mut app, pt(70.0, 40.0));
    let outline = placed(&app);
    let arcs: Vec<_> = outline.segments().filter_map(|s| s.arc()).collect();
    assert_eq!((outline.segments().count(), arcs.len()), (8, 4));
    for arc in &arcs {
        assert!((arc.radius_mm - 5.0).abs() < 1e-9, "the 5 mm default");
    }
    assert_box(&outline, (10.0, 10.0), (70.0, 40.0));
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Rounded { radius_mm: 5.0 },
        })
    );

    app.tool_event(Event::Option(ToolOption::Corners(CornerStyle::Chamfered)));
    app.tool_event(Event::Option(ToolOption::CornerSize("1/4 in".to_owned())));
    click(&mut app, pt(10.0, 50.0));
    click(&mut app, pt(70.0, 80.0));
    let outline = placed(&app);
    assert_eq!(outline.segments().filter(Segment::is_line).count(), 8);
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Rectangle {
            width_mm: 60.0,
            height_mm: 30.0,
            corner: Corner::Chamfered { distance_mm: 6.35 },
        })
    );
    // Kept for the next, and held to half the shorter side.
    assert_eq!(app.prefs().drawing.corners, CornerStyle::Chamfered);
    click(&mut app, pt(80.0, 10.0));
    click(&mut app, pt(90.0, 14.0));
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Rectangle {
            width_mm: 10.0,
            height_mm: 4.0,
            corner: Corner::Chamfered { distance_mm: 2.0 },
        })
    );
    // A size that is no length leaves it as it was.
    app.tool_event(Event::Option(ToolOption::CornerSize("wide".to_owned())));
    assert_eq!(app.prefs().drawing.corner_mm, Some(6.35));
    // An inch project's default is 0.2 in.
    assert!(
        (dc_app::prefs::DrawingPrefs::default().corner_size_mm(Units::Inch) - 5.08).abs() < 1e-12
    );
}

/// Acceptance 108 and 114: typing W 60 and H 30 after the first click
/// gives the file the clicks give; a pinned width follows the pointer in
/// height only; the fields show what the pointer gives until one is
/// typed.
#[test]
fn rectangle_by_typed_sizes() {
    let mut clicked = armed(Drawing::Rectangle);
    click(&mut clicked, pt(10.0, 10.0));
    click(&mut clicked, pt(70.0, 40.0));

    let mut app = armed(Drawing::Rectangle);
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::X, String::new(), false),
            (SizeField::Y, String::new(), false)
        ],
        "before the pointer is over the Viewport"
    );
    move_to(&mut app, pt(10.0, 10.0));
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::X, "10".to_owned(), false),
            (SizeField::Y, "10".to_owned(), false)
        ]
    );
    click(&mut app, pt(10.0, 10.0));
    move_to(&mut app, pt(25.0, 22.5));
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::Width, "15".to_owned(), false),
            (SizeField::Height, "12.5".to_owned(), false)
        ]
    );
    type_size(&mut app, SizeField::Width, "60");
    assert_eq!(
        field_texts(&app),
        [
            (SizeField::Width, "60".to_owned(), true),
            (SizeField::Height, "12.5".to_owned(), false)
        ]
    );
    // The preview is 60 wide and as high as the pointer says.
    assert_box(&app.draft().outlines[0], (10.0, 10.0), (70.0, 22.5));
    move_to(&mut app, pt(12.0, 50.0));
    assert_box(&app.draft().outlines[0], (10.0, 10.0), (70.0, 50.0));
    type_size(&mut app, SizeField::Height, "30");
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Rectangle 1”"));
    assert_eq!(app.serialize().unwrap(), clicked.serialize().unwrap());
    // The next rectangle starts with nothing pinned.
    assert!(field_texts(&app).iter().all(|(_, _, pinned)| !pinned));
}

/// Acceptance 114: a typed X and Y place the first point, measured from
/// the origin; a pointer to the left of the first point puts a typed
/// width there; `1/4 in` in a millimetre project is 6.35; empty text
/// unpins.
#[test]
fn the_first_point_and_units_can_be_typed() {
    let mut app = armed(Drawing::Rectangle);
    type_size(&mut app, SizeField::X, "20");
    type_size(&mut app, SizeField::Y, "30");
    assert_eq!(enter(&mut app), None, "the first point is taken");
    move_to(&mut app, pt(5.0, 80.0));
    type_size(&mut app, SizeField::Width, "1/4 in");
    type_size(&mut app, SizeField::Height, "10");
    enter(&mut app);
    // Left of the first point and above it, as the pointer was.
    assert_box(&placed(&app), (20.0 - 6.35, 30.0), (20.0, 40.0));

    // From a centre origin, X and Y are measured from the stock's middle.
    let mut project = App::new_project().unwrap();
    project.origin.xy = dc_project::OriginXy::Centre;
    let mut app = Harness::from_project(project).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(Drawing::Circle));
    move_to(&mut app, pt(60.0, 45.0));
    assert_eq!(field_texts(&app)[0].1, "10");
    assert_eq!(field_texts(&app)[1].1, "-5");
    type_size(&mut app, SizeField::X, "-20");
    // Only X is typed: Y is the pointer's.
    enter(&mut app);
    type_size(&mut app, SizeField::Diameter, "8");
    enter(&mut app);
    assert_box(&placed(&app), (26.0, 41.0), (34.0, 49.0));

    let mut app = armed(Drawing::Rectangle);
    click(&mut app, pt(10.0, 10.0));
    move_to(&mut app, pt(30.0, 30.0));
    type_size(&mut app, SizeField::Width, "50");
    assert!(field_texts(&app)[0].2);
    type_size(&mut app, SizeField::Width, "  ");
    assert_eq!(
        field_texts(&app)[0],
        (SizeField::Width, "20".to_owned(), false)
    );
}

/// Acceptance 114: a value the tool cannot use marks its field and makes
/// no shape; the next value typed clears the mark.
#[test]
fn a_refused_size_marks_its_field() {
    let mut app = armed(Drawing::Circle);
    click(&mut app, pt(50.0, 50.0));
    let problem = |app: &App| app.tool_bar().unwrap().fields[0].problem.clone();
    type_size(&mut app, SizeField::Diameter, "0");
    assert_eq!(
        problem(&app).as_deref(),
        Some("a size must be greater than 0")
    );
    assert!(!app.tool_bar().unwrap().fields[0].pinned);
    type_size(&mut app, SizeField::Diameter, "wide");
    assert!(
        problem(&app).is_some_and(|p| p.contains("wide")),
        "{:?}",
        problem(&app)
    );
    // Nothing is pinned and the pointer is off the Viewport: no circle.
    assert_eq!(enter(&mut app), None);
    assert!(app.project().instances.is_empty());
    type_size(&mut app, SizeField::Diameter, "20");
    assert_eq!(problem(&app), None);
    assert!(enter(&mut app).is_some());

    let mut app = armed(Drawing::Polygon);
    click(&mut app, pt(50.0, 50.0));
    type_size(&mut app, SizeField::Angle, "ninety");
    let angle = &app.tool_bar().unwrap().fields[1];
    assert_eq!(angle.field, SizeField::Angle);
    assert!(
        angle
            .problem
            .as_ref()
            .is_some_and(|p| p.contains("`ninety` is not an angle"))
    );
}

/// Acceptance 109: a Circle from (50, 50) to (60, 50) is two half-circle
/// arcs of radius 10 centred there; drawn by a typed diameter the same.
#[test]
fn circle() {
    let mut app = armed(Drawing::Circle);
    click(&mut app, pt(50.0, 50.0));
    assert_eq!(
        click(&mut app, pt(60.0, 50.0)).as_deref(),
        Some("Draw “Circle 1”")
    );
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Circle { diameter_mm: 20.0 })
    );
    let outline = placed(&app);
    let arcs: Vec<_> = outline.segments().filter_map(|s| s.arc()).collect();
    assert_eq!(arcs.len(), 2);
    for arc in arcs {
        assert!((arc.radius_mm - 10.0).abs() < 1e-9);
        assert!(arc.centre.distance_to(pt(50.0, 50.0)) < 1e-9);
        assert!((arc.sweep_rad - std::f64::consts::PI).abs() < 1e-9);
    }
    click(&mut app, pt(20.0, 20.0));
    type_size(&mut app, SizeField::Diameter, "12");
    enter(&mut app);
    assert_box(&placed(&app), (14.0, 14.0), (26.0, 26.0));
    assert_eq!(last(&app).0.name, "Circle 2");
}

/// Acceptance 109: an Ellipse 40 × 20 has every outline vertex within
/// the flatten tolerance of the true ellipse and a box of 40 × 20;
/// `Shift` makes it a circle.
#[test]
fn ellipse() {
    let mut app = armed(Drawing::Ellipse);
    click(&mut app, pt(50.0, 50.0));
    assert_eq!(
        click(&mut app, pt(70.0, 60.0)).as_deref(),
        Some("Draw “Ellipse 1”")
    );
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Ellipse {
            width_mm: 40.0,
            height_mm: 20.0
        })
    );
    let outline = placed(&app);
    assert_box(&outline, (30.0, 40.0), (70.0, 60.0));
    let tolerance = app.project().flatten_tolerance_mm;
    for segment in outline.segments() {
        let (middle, _) = segment.point_at(segment.length_mm() / 2.0).unwrap();
        for p in [segment.start, middle] {
            // The implicit equation's value is 1 on the curve; off it by
            // `d` it changes by at least `2d / 20`, the greater radius.
            let value = ((p.x - 50.0) / 20.0).powi(2) + ((p.y - 50.0) / 10.0).powi(2);
            assert!((value - 1.0).abs() * 10.0 <= tolerance, "{p:?}");
        }
    }
    click(&mut app, pt(50.0, 50.0));
    let shift = Pointer {
        shift: true,
        ..Pointer::at(pt(58.0, 53.0))
    };
    click_with(&mut app, shift);
    assert_box(&placed(&app), (42.0, 42.0), (58.0, 58.0));
    // Typed: the full width and height.
    click(&mut app, pt(20.0, 20.0));
    type_size(&mut app, SizeField::Width, "30");
    type_size(&mut app, SizeField::Height, "10");
    enter(&mut app);
    assert_box(&placed(&app), (5.0, 15.0), (35.0, 25.0));
}

/// Acceptance 109: a Polygon of six sides, radius 10 to the corners, has
/// six vertices 10 from its centre, the first toward the second click;
/// to the flats, the middle of each side is 10 from the centre and a
/// side's middle is toward the click. Named for its sides.
#[test]
fn polygon_sides_and_flats() {
    let mut app = armed(Drawing::Polygon);
    let centre = pt(50.0, 50.0);
    click(&mut app, centre);
    assert_eq!(
        click(&mut app, pt(50.0, 60.0)).as_deref(),
        Some("Draw “Hexagon 1”")
    );
    let outline = placed(&app);
    assert_eq!(outline.vertices().len(), 6);
    for vertex in outline.vertices() {
        assert!((vertex.point().distance_to(centre) - 10.0).abs() < 1e-9);
    }
    assert!(outline.vertices()[0].point().distance_to(pt(50.0, 60.0)) < 1e-9);
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Polygon {
            sides: 6,
            radius_mm: 10.0,
            measured: Measured::Corners
        })
    );

    app.tool_event(Event::Option(ToolOption::Measured(Measured::Flats)));
    click(&mut app, centre);
    click(&mut app, pt(60.0, 50.0));
    let flats = placed(&app);
    let middles: Vec<Point> = flats
        .segments()
        .map(|side| {
            let (middle, _) = side.point_at(side.length_mm() / 2.0).unwrap();
            middle
        })
        .collect();
    assert!(
        middles
            .iter()
            .all(|m| (m.distance_to(centre) - 10.0).abs() < 1e-9)
    );
    assert!(
        middles.iter().any(|m| m.distance_to(pt(60.0, 50.0)) < 1e-9),
        "a flat faces the click"
    );

    // Sides are held to 3 to 50, kept between shapes, and name the shape.
    for (sides, held, name) in [
        (3, 3, "Triangle 1"),
        (2, 3, "Triangle 2"),
        (51, 50, "Polygon 1"),
        (8, 8, "Octagon 1"),
    ] {
        app.tool_event(Event::Option(ToolOption::Sides(sides)));
        assert_eq!(app.prefs().drawing.polygon_sides, held);
        click(&mut app, centre);
        type_size(&mut app, SizeField::Radius, "5");
        type_size(&mut app, SizeField::Angle, "90");
        enter(&mut app);
        assert_eq!(last(&app).0.name, name);
        assert_eq!(placed(&app).vertices().len(), held as usize);
    }
}

/// Acceptance 110: a Slot from (20, 50) to (60, 50), width 12: four
/// segments, line, arc, line, arc; the arcs are half circles of radius 6
/// centred on the two clicked points; closed and counter-clockwise; at
/// 45° the same holds.
#[test]
fn slot_by_three_clicks() {
    let mut app = armed(Drawing::Slot);
    click(&mut app, pt(20.0, 50.0));
    assert_eq!(click(&mut app, pt(60.0, 50.0)), None, "the second centre");
    // Before the third click the width is the pointer's distance from the
    // centreline, twice over.
    move_to(&mut app, pt(33.0, 53.0));
    assert_eq!(
        field_texts(&app),
        [(SizeField::SlotWidth, "6".to_owned(), false)]
    );
    assert_eq!(
        click(&mut app, pt(40.0, 44.0)).as_deref(),
        Some("Draw “Slot 1”")
    );
    assert_eq!(
        last(&app).1.shape,
        Some(Shape::Slot {
            length_mm: 40.0,
            width_mm: 12.0
        })
    );
    let outline = placed(&app);
    assert!(outline.is_closed() && outline.signed_area_mm2() > 0.0);
    let kinds: Vec<bool> = outline.segments().map(|s| s.is_line()).collect();
    assert_eq!(kinds, [true, false, true, false]);
    let arcs: Vec<_> = outline.segments().filter_map(|s| s.arc()).collect();
    for (arc, centre) in arcs.iter().zip([pt(60.0, 50.0), pt(20.0, 50.0)]) {
        assert!(arc.centre.distance_to(centre) < 1e-9, "{arc:?}");
        assert!((arc.radius_mm - 6.0).abs() < 1e-9);
        assert!((arc.sweep_rad - std::f64::consts::PI).abs() < 1e-9);
    }

    // At 45°.
    let (a, b) = (pt(10.0, 10.0), pt(40.0, 40.0));
    click(&mut app, a);
    click(&mut app, b);
    type_size(&mut app, SizeField::SlotWidth, "8");
    enter(&mut app);
    let outline = placed(&app);
    let arcs: Vec<_> = outline.segments().filter_map(|s| s.arc()).collect();
    assert_eq!(arcs.len(), 2);
    assert!(arcs[0].centre.distance_to(b) < 1e-9 && arcs[1].centre.distance_to(a) < 1e-9);
    assert!((arcs[0].radius_mm - 4.0).abs() < 1e-9);
}

/// Capability 83: a slot's length, angle and width typed after the first
/// centre make the whole slot at once (`useCreationWorkflow.ts`), the
/// angle anticlockwise from +X; without a width the third step asks for
/// it. `Backspace` steps back from the width to the axis.
#[test]
fn slot_by_typed_sizes() {
    let mut app = armed(Drawing::Slot);
    click(&mut app, pt(50.0, 20.0));
    assert_eq!(
        field_texts(&app).iter().map(|f| f.0).collect::<Vec<_>>(),
        [SizeField::Length, SizeField::Angle, SizeField::SlotWidth]
    );
    type_size(&mut app, SizeField::Length, "30");
    type_size(&mut app, SizeField::Angle, "90");
    type_size(&mut app, SizeField::SlotWidth, "6");
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Slot 1”"));
    // Up the Viewport from (50, 20): ends at (50, 20) and (50, 50).
    assert_box(&placed(&app), (47.0, 17.0), (53.0, 53.0));

    click(&mut app, pt(10.0, 80.0));
    type_size(&mut app, SizeField::Length, "20");
    type_size(&mut app, SizeField::Angle, "0");
    assert_eq!(
        enter(&mut app),
        None,
        "the axis; the width is still to come"
    );
    assert_eq!(field_texts(&app).len(), 1);
    app.tool_event(Event::Key(Key::Backspace));
    assert_eq!(field_texts(&app).len(), 3, "back at the axis");
    type_size(&mut app, SizeField::Length, "20");
    type_size(&mut app, SizeField::Angle, "0");
    enter(&mut app);
    type_size(&mut app, SizeField::SlotWidth, "4");
    assert_eq!(enter(&mut app).as_deref(), Some("Draw “Slot 2”"));
    assert_box(&placed(&app), (8.0, 78.0), (32.0, 82.0));
}

/// Capability 81 (`stickyDrawing.test.ts`): a tool stays armed after
/// each shape, with its options; `Escape` steps back out of the shape
/// begun, then out of the tool; shapes are numbered by what is free.
#[test]
fn a_tool_stays_armed_with_its_options() {
    let mut app = armed(Drawing::Rectangle);
    app.tool_event(Event::Option(ToolOption::Corners(CornerStyle::Rounded)));
    for (i, x) in [10.0, 40.0, 70.0].into_iter().enumerate() {
        click(&mut app, pt(x, 10.0));
        click(&mut app, pt(x + 20.0, 30.0));
        assert_eq!(app.tool(), Tool::Draw(Drawing::Rectangle), "still armed");
        assert_eq!(app.project().instances.len(), i + 1);
        assert!(matches!(
            last(&app).1.shape,
            Some(Shape::Rectangle {
                corner: Corner::Rounded { .. },
                ..
            })
        ));
    }
    let names: Vec<&str> = app
        .project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(names, ["Rectangle 1", "Rectangle 2", "Rectangle 3"]);
    // The shape just drawn is the selection.
    assert_eq!(
        app.selection().instances(),
        [app.project().instances[2].id.clone()]
    );

    // Escape with a corner taken drops the corner and keeps the tool.
    click(&mut app, pt(10.0, 50.0));
    move_to(&mut app, pt(30.0, 70.0));
    assert_eq!(app.draft().outlines.len(), 1);
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Draw(Drawing::Rectangle));
    assert!(app.draft().outlines.is_empty());
    assert_eq!(app.project().instances.len(), 3);
    assert_eq!(app.tool_bar().unwrap().fields[0].field, SizeField::X);
    // Escape with nothing begun leaves the tool.
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(app.tool_bar(), None);
    // The freed number is taken again.
    let second = app.project().instances[1].id.clone();
    app.run(dc_app::Command::DeleteInstances(vec![second]))
        .unwrap();
    app.arm(Tool::Draw(Drawing::Rectangle));
    click(&mut app, pt(10.0, 50.0));
    click(&mut app, pt(30.0, 70.0));
    assert_eq!(last(&app).0.name, "Rectangle 2");
}

/// Capability 90: a shape under 0.01 mm on a side is not made, and the
/// tool stays as it was; the Construction switch marks the shapes that
/// follow until it is turned off, across a change of tool.
#[test]
fn too_small_is_not_made_and_construction_is_marked() {
    let mut app = armed(Drawing::Rectangle);
    click(&mut app, pt(10.0, 10.0));
    assert_eq!(click(&mut app, pt(10.0, 10.0)), None, "no size at all");
    assert_eq!(click(&mut app, pt(40.0, 10.005)), None, "5 µm high");
    assert!(app.project().instances.is_empty());
    assert_eq!(
        app.tool_bar().unwrap().fields[0].field,
        SizeField::Width,
        "the first corner is still taken"
    );
    assert!(click(&mut app, pt(40.0, 10.02)).is_some());

    assert!(!app.drawing_construction());
    app.tool_event(Event::Option(ToolOption::Construction(true)));
    app.arm(Tool::Draw(Drawing::Circle));
    assert!(app.drawing_construction(), "kept across a change of tool");
    click(&mut app, pt(50.0, 50.0));
    click(&mut app, pt(60.0, 50.0));
    assert!(last(&app).0.construction);
    app.tool_event(Event::Option(ToolOption::Construction(false)));
    click(&mut app, pt(80.0, 80.0));
    click(&mut app, pt(85.0, 80.0));
    assert!(!last(&app).0.construction);
}

/// Capabilities 82 and 83a: a drawn point snaps (a corner on the grid, a
/// second circle on the first's centre), and the shape being drawn shows
/// its sizes beside it in project units, as does the prompt of each step.
#[test]
fn points_snap_and_sizes_are_shown() {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.arm(Tool::Draw(Drawing::Rectangle));
    assert_eq!(app.tool_bar().unwrap().prompt, "Click the first corner");
    // The default camera is 5 px to the mm: a 2 mm grid.
    click(&mut app, pt(10.3, 9.6));
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click the opposite corner, or type its size"
    );
    move_to(&mut app, pt(69.7, 40.4));
    assert_eq!(app.draft().snap.unwrap().kind, Some(dc_app::SnapKind::Grid));
    assert_eq!(app.draft().labels.len(), 1);
    assert_eq!(app.draft().labels[0].text, "60 × 30");
    click(&mut app, pt(69.7, 40.4));
    assert_box(&placed(&app), (10.0, 10.0), (70.0, 40.0));

    app.arm(Tool::Draw(Drawing::Circle));
    click(&mut app, pt(30.0, 60.0));
    click(&mut app, pt(30.0, 70.0));
    // A second circle: its centre snaps to the first's.
    move_to(&mut app, pt(30.9, 59.3));
    assert_eq!(
        app.draft().snap.unwrap().kind,
        Some(dc_app::SnapKind::Centre)
    );
    click(&mut app, pt(30.9, 59.3));
    move_to(&mut app, pt(30.0, 64.0));
    assert_eq!(app.draft().labels[0].text, "⌀ 8");
    click(&mut app, pt(30.0, 64.0));
    assert_box(&placed(&app), (26.0, 56.0), (34.0, 64.0));

    // An inch project shows inches.
    let mut project = App::new_project().unwrap();
    project.display_units = Units::Inch;
    let mut app = Harness::from_project(project).app;
    app.prefs_mut().snap.enabled = false;
    app.arm(Tool::Draw(Drawing::Rectangle));
    click(&mut app, pt(0.0, 0.0));
    move_to(&mut app, pt(50.8, 25.4));
    assert_eq!(app.draft().labels[0].text, "2 × 1");
    assert_eq!(field_texts(&app)[0].1, "2");
    type_size(&mut app, SizeField::Height, "0.5");
    enter(&mut app);
    assert_box(&placed(&app), (0.0, 0.0), (50.8, 12.7));
}

/// A new shape goes in the folder that holds the selection (capability
/// 90), and at the end of the Design Tree.
#[test]
fn a_new_shape_joins_the_selections_folder() {
    let project = dc_project::test_support::drawing_project();
    let mut app = Harness::from_project(project).app;
    app.prefs_mut().snap.enabled = false;
    let in_folder = dc_project::InstanceId::new("i2");
    app.select(dc_app::Selection::Instances(vec![in_folder]));
    app.arm(Tool::Draw(Drawing::Circle));
    click(&mut app, pt(200.0, 50.0));
    click(&mut app, pt(205.0, 50.0));
    let (instance, _) = last(&app);
    assert_eq!(instance.folder, Some(dc_project::FolderId::new("f1")));
    assert_eq!(instance.name, "Circle 2", "the fixture has a Circle 1");
    // With the new circle selected, also in the folder, the next joins it
    // too; with nothing selected, none.
    click(&mut app, pt(220.0, 50.0));
    click(&mut app, pt(225.0, 50.0));
    assert_eq!(last(&app).0.folder, Some(dc_project::FolderId::new("f1")));
    app.select(dc_app::Selection::Nothing);
    click(&mut app, pt(240.0, 50.0));
    click(&mut app, pt(245.0, 50.0));
    assert_eq!(last(&app).0.folder, None);
}
