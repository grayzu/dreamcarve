// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 25 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Tracing and measuring"): a picture as a backdrop to trace over
//! (functional capability 101, acceptance 126). Its opacity is saved and
//! changes nothing that is carved; it is scaled by two points and the
//! real distance between them; and a locked picture is not picked on the
//! Viewport, so drawing over it never grabs it.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::import::{PictureImport, Purpose};
use dc_app::test_support::Harness;
use dc_app::tool::{Adjust, Event, Key, SizeField};
use dc_app::viewport::camera::Camera;
use dc_app::viewport::pick::{self, Hit, PICK_TOLERANCE_PX};
use dc_app::{App, Command, CommandError, Pointer, Selection, Tool, pictures};
use dc_geometry::{Point, Rect, Vec2};
use dc_import::ImageFormat;
use dc_project::BitmapId;

use common::{fixture, read};

fn b(id: &str) -> BitmapId {
    BitmapId::new(id)
}

fn sheet(name: &str, width_mm: f64, purpose: Purpose) -> PictureImport {
    let file = fixture(&format!("photo/{name}"));
    let format = file
        .extension()
        .and_then(|e| ImageFormat::from_extension(&e.to_string_lossy()))
        .expect("a picture's extension");
    let mut sheet = PictureImport::open(format, name, &read(&file), purpose);
    sheet.set_width_mm(width_mm);
    sheet
}

/// A project from the gradient picture: bitmap `b1`, 100 × 50 mm at
/// (10, 10), carved by the Photo V-Carve `op1`.
fn gradient() -> Harness {
    let mut harness = Harness::from_project(App::new_project().expect("a new project"));
    harness
        .app
        .new_from_picture(&sheet("gradient.png", 100.0, Purpose::New))
        .expect("a project from the gradient");
    harness
}

fn footprint(app: &App) -> Rect {
    let bitmap = &app.project().bitmaps[0];
    Rect {
        min: Point::new(bitmap.x_mm, bitmap.y_mm),
        max: Point::new(
            bitmap.x_mm + bitmap.width_mm,
            bitmap.y_mm + bitmap.height_mm,
        ),
    }
}

/// Acceptance 126: a picture's opacity is how strongly it shows. It is
/// saved with the project, the Scene carries it, and the Photo V-Carve
/// that carves the picture reads nothing of it.
#[test]
fn opacity_is_saved_and_cuts_nothing() {
    let mut harness = gradient();
    let op1 = harness.app.project().operations[0].id().clone();
    assert_eq!(harness.app.project().bitmaps[0].opacity_fraction, 1.0);
    let props = pictures::properties(harness.app.project(), &[b("b1")]).unwrap();
    assert_eq!(props.opacity_fraction, Some(1.0));
    let before = dc_app::generation::digest(harness.app.project(), &op1);

    let fade = |opacity_fraction: f64| Command::SetBitmapOpacity {
        ids: vec![b("b1")],
        opacity_fraction,
    };
    assert_eq!(harness.run(fade(0.4)).unwrap(), "Set opacity of “gradient”");
    assert_eq!(harness.app.project().bitmaps[0].opacity_fraction, 0.4);
    assert_eq!(harness.app.scene().bitmaps()[0].opacity_fraction, 0.4);
    // What is carved does not know.
    assert_eq!(
        dc_app::generation::digest(harness.app.project(), &op1),
        before
    );
    // Saved, and read back.
    let saved = dc_project::save(harness.app.project()).unwrap();
    assert!(saved.contains("\"opacity_fraction\": 0.4"), "in the file");
    assert_eq!(
        dc_project::load(saved.as_bytes()).unwrap().project.bitmaps[0].opacity_fraction,
        0.4
    );
    // The same again is no step; under a tenth, over the whole, and no
    // number are refused.
    assert!(matches!(
        harness.run(fade(0.4)),
        Err(CommandError::NothingToDo)
    ));
    for refused in [0.05, 1.5, f64::NAN] {
        assert!(harness.run(fade(refused)).is_err(), "{refused}");
    }
    // A locked picture still fades: a lock stops moves, not looks.
    let lock = dc_app::tree::toggle_bitmap_locked(harness.app.project(), &[b("b1")]).unwrap();
    harness.run(lock).unwrap();
    assert!(harness.run(fade(1.0)).is_ok());
    // Typed in Properties as a percentage.
    let strings = harness.app.strings();
    assert_eq!(
        pictures::opacity(harness.app.project(), strings, &[b("b1")], "25").unwrap(),
        fade(0.25)
    );
    assert_eq!(
        pictures::opacity(harness.app.project(), strings, &[b("b1")], "25 %").unwrap(),
        fade(0.25)
    );
    let refused = pictures::opacity(harness.app.project(), strings, &[b("b1")], "5");
    assert_eq!(
        refused.unwrap_err().to_string(),
        "`5` is not an opacity: type a percentage from 10 to 100"
    );
    assert!(pictures::opacity(harness.app.project(), strings, &[b("b1")], "half").is_err());
}

/// Acceptance 126: two points on the picture and the real distance
/// between them: the picture is resized about the first point so that
/// they are that far apart.
#[test]
fn scale_by_two_points() {
    let mut harness = gradient();
    let before = footprint(&harness.app);
    assert_eq!(
        (before.min.x, before.min.y, before.width_mm()),
        (10.0, 10.0, 100.0)
    );
    // Two points 50 mm apart that are, in truth, 120 mm apart: 2.4 times.
    let (first, second) = (Point::new(30.0, 20.0), Point::new(80.0, 20.0));
    let command =
        pictures::scale_by_two_points(harness.app.project(), &b("b1"), first, second, 120.0)
            .unwrap();
    assert_eq!(harness.run(command).unwrap(), "Scale “gradient”");
    let after = footprint(&harness.app);
    assert_eq!(after.width_mm(), 240.0);
    assert_eq!(
        after.height_mm(),
        120.0,
        "its proportions are the picture's"
    );
    // The first point of the picture is where it was: it was 20 mm from
    // the left edge and 10 mm from the bottom, and is now 48 and 24.
    assert_eq!((after.min.x, after.min.y), (-18.0, -4.0));
    assert!(harness.app.undo().is_some());
    assert_eq!(footprint(&harness.app), before, "one undo step");

    // Points that are one point, and a distance that is none.
    let project = harness.app.project().clone();
    assert!(pictures::scale_by_two_points(&project, &b("b1"), first, first, 80.0).is_err());
    for distance in [0.0, -5.0, f64::NAN] {
        assert!(
            pictures::scale_by_two_points(&project, &b("b1"), first, second, distance).is_err(),
            "{distance}"
        );
    }
    // The distance it has is nothing to do; a locked picture is refused.
    assert!(matches!(
        pictures::scale_by_two_points(&project, &b("b1"), first, second, 50.0),
        Err(CommandError::NothingToDo)
    ));
    let lock = dc_app::tree::toggle_bitmap_locked(&project, &[b("b1")]).unwrap();
    harness.run(lock).unwrap();
    assert!(matches!(
        pictures::scale_by_two_points(harness.app.project(), &b("b1"), first, second, 80.0),
        Err(CommandError::PictureLocked { .. })
    ));
}

/// *Set scale…* on the Viewport: two clicks on the picture, the distance
/// between them typed, Done.
#[test]
fn set_scale_is_two_clicks_and_a_distance() {
    let mut harness = gradient();
    let app = &mut harness.app;
    app.select(Selection::Bitmaps(vec![b("b1")]));
    app.arm(Tool::Adjust(Adjust::PictureScale));
    let click = |app: &mut App, x: f64, y: f64| {
        let at = Pointer {
            at_mm: Point::new(x, y),
            shift: false,
            alt: false,
            command: false,
        };
        app.tool_event(Event::Press(at));
        app.tool_event(Event::Release(at));
    };
    let bar = app.tool_bar().unwrap();
    assert_eq!(bar.prompt, "Click a point on the picture");
    assert!(bar.fields.is_empty() && !bar.can_finish);
    click(app, 30.0, 20.0);
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click a second point, a known distance from the first"
    );
    click(app, 70.0, 20.0);
    let bar = app.tool_bar().unwrap();
    assert_eq!(
        bar.prompt,
        "Type the real distance between the points, then Done"
    );
    assert_eq!(
        (bar.fields[0].field, bar.fields[0].text.as_str()),
        (SizeField::Distance, "40")
    );
    assert!(!bar.can_finish, "the distance it has changes nothing");
    assert_eq!(app.draft().outlines.len(), 1, "the line between the points");
    assert_eq!(app.draft().nodes.len(), 2, "and a mark on each");
    app.tool_event(Event::Field {
        field: SizeField::Distance,
        text: "80".to_owned(),
    });
    assert!(app.tool_bar().unwrap().can_finish);
    let reply = app.tool_event(Event::Done);
    assert_eq!(reply.ran.as_deref(), Some("Scale “gradient”"));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(footprint(app).width_mm(), 200.0);
    assert_eq!(app.selected_pictures(), [b("b1")]);

    // Escape takes a point back, then leaves; with no picture selected
    // the bar says to select one.
    app.arm(Tool::Adjust(Adjust::PictureScale));
    click(app, 0.0, 0.0);
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Click a point on the picture"
    );
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    app.select(Selection::Nothing);
    app.arm(Tool::Adjust(Adjust::PictureScale));
    assert_eq!(
        app.tool_bar().unwrap().prompt,
        "Select one picture first, then choose this again"
    );
}

/// A locked picture is not picked by a click or a marquee on the
/// Viewport, so drawing and selecting over a backdrop never grab it; its
/// row in the Design Tree still selects it.
#[test]
fn a_locked_picture_is_not_picked() {
    let mut harness = gradient();
    let mut camera = Camera {
        centre_mm: Point::new(0.0, 0.0),
        px_per_mm: 4.0,
        viewport_px: Vec2::new(2000.0, 1000.0),
    };
    camera.pan(-900.0, 0.0);
    let at = camera.to_screen(Point::new(60.0, 35.0));
    let over = Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(200.0, 200.0),
    };
    let scene = harness.app.scene().clone();
    assert_eq!(
        pick::pick(&scene, &camera, at, PICK_TOLERANCE_PX, 0),
        Hit::Bitmap(b("b1"))
    );
    assert_eq!(pick::pictures_touching(&scene, over), [b("b1")]);

    let lock = dc_app::tree::toggle_bitmap_locked(harness.app.project(), &[b("b1")]).unwrap();
    harness.run(lock).unwrap();
    let scene = harness.app.scene().clone();
    assert_eq!(
        pick::pick(&scene, &camera, at, PICK_TOLERANCE_PX, 0),
        Hit::Nothing
    );
    assert!(pick::pictures_touching(&scene, over).is_empty());
    // Its row still selects it.
    harness.app.select(Selection::Bitmaps(vec![b("b1")]));
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
}
