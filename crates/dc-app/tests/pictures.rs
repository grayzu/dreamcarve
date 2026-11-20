// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 4 (#535; functional spec, capabilities 56 and 57):
//! what the window draws of a project's bitmaps. Each is a row of the
//! Design Tree's Pictures section, with a name, an eye and a padlock, and
//! each visible one is in the Scene at its footprint, with its pixels, for
//! the Viewport to draw under the shapes.
//!
//! Slice 15 (#544; capabilities 56 to 58): a picture is a thing to select
//! and place. A click inside its footprint picks it, under everything
//! else; it is selected from its row or the Viewport; its Properties are
//! its position, its size and its pixels; it moves as one undo step, and
//! the Photo V-Carve that carves it follows.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::sync::Arc;

use dc_app::import::{PictureImport, Purpose};
use dc_app::operations::{self, Kind};
use dc_app::properties::{self, Edit, Field};
use dc_app::scene::{StrokeKind, strokes};
use dc_app::test_support::Harness;
use dc_app::tree::{self, MenuItem, Row, Section, Tree};
use dc_app::viewport::camera::Camera;
use dc_app::viewport::pick::{self, Hit, PICK_TOLERANCE_PX};
use dc_app::{App, ClickMode, Command, CommandError, Selection, pictures};
use dc_geometry::{Point, Rect, Vec2};
use dc_import::ImageFormat;
use dc_job::picture::PictureSize;
use dc_project::{BitmapId, Operation, Stock, Units};

use common::{fixture, plaque, read};

fn b(id: &str) -> BitmapId {
    BitmapId::new(id)
}

/// The Add picture sheet over `tests/fixtures/photo/<name>`, `width_mm`
/// wide.
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

/// A project from the gradient picture (bitmap `b1`, 100 × 50 mm at
/// 10,10, carved by the Photo V-Carve `op1`) with the colour picture
/// added (bitmap `b2`, 20 × 20 mm at the origin, carved by nothing).
fn two_pictures() -> Harness {
    let mut harness = Harness::from_project(App::new_project().expect("a new project"));
    harness
        .app
        .new_from_picture(&sheet("gradient.png", 100.0, Purpose::New))
        .expect("a project from the gradient");
    harness
        .app
        .import_picture(&sheet("colour.jpg", 20.0, Purpose::Import))
        .expect("the colour picture imports");
    harness
}

/// Capability 57: below the shapes and the tabs, the Pictures section
/// lists the bitmaps in file order, each with its name, eye and padlock,
/// its size in pixels and the Photo V-Carves that carve it. The header's
/// eye and padlock read the rows' common state.
#[test]
fn bitmaps_are_rows_of_the_pictures_section() {
    let mut harness = two_pictures();
    let tree = Tree::build(harness.app.project());
    let ids: Vec<&str> = tree.bitmaps.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["b1", "b2"]);
    let gradient = tree.bitmap(&b("b1")).expect("b1's row");
    assert_eq!(gradient.name, "gradient");
    assert!(gradient.visible && !gradient.locked);
    assert_eq!((gradient.width_px, gradient.height_px), (64, 32));
    let carving: Vec<(&str, &str)> = gradient
        .operations
        .iter()
        .map(|o| (o.id.as_str(), o.name.as_str()))
        .collect();
    assert_eq!(carving, [("op1", "Photo V-Carve")]);
    let colour = tree.bitmap(&b("b2")).expect("b2's row");
    assert_eq!(colour.name, "colour");
    assert_eq!((colour.width_px, colour.height_px), (16, 16));
    assert!(colour.operations.is_empty(), "nothing carves it");
    assert!(tree.bitmap(&b("b9")).is_none());

    // Top to bottom: the three headers, the bitmaps under the last.
    assert_eq!(
        tree.rows(),
        [
            Row::Header(Section::Shapes),
            Row::Header(Section::Tabs),
            Row::Header(Section::Pictures),
            Row::Bitmap(gradient),
            Row::Bitmap(colour),
        ]
    );
    let header = tree.bitmaps_header();
    assert_eq!((header.visible, header.locked), (Some(true), Some(false)));

    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: Some(false),
            locked: Some(true),
        })
        .expect("hides and locks");
    let tree = Tree::build(harness.app.project());
    let colour = tree.bitmap(&b("b2")).unwrap();
    assert!(!colour.visible && colour.locked);
    let header = tree.bitmaps_header();
    assert_eq!((header.visible, header.locked), (None, None), "mixed");

    // A project of shapes: its rows, then the Pictures header alone.
    let tree = Tree::build(&plaque());
    assert!(tree.bitmaps.is_empty());
    let rows = tree.rows();
    assert_eq!(rows.len(), 3 + tree.shapes.len() + tree.tabs.len());
    assert_eq!(rows.last(), Some(&Row::Header(Section::Pictures)));
    assert_eq!(rows[0], Row::Header(Section::Shapes));
    assert_eq!(rows[1], Row::Shape(&tree.shapes[0]));
    let header = tree.bitmaps_header();
    assert_eq!((header.visible, header.locked), (None, None), "no rows");
}

/// Capability 56: a bitmap's row hides, locks, renames and deletes, each
/// as one undo step named for the picture; the section header acts on
/// every row. A Photo V-Carve that loses its picture stays and says so.
#[test]
fn a_bitmap_row_hides_locks_renames_and_deletes() {
    let mut harness = two_pictures();
    let one = [b("b2")];
    let both = [b("b1"), b("b2")];

    assert_eq!(
        tree::bitmap_menu(harness.app.project(), &one),
        [
            MenuItem::Rename,
            MenuItem::Hide,
            MenuItem::Lock,
            MenuItem::Delete,
            MenuItem::ZoomTo,
            MenuItem::AddOperation,
        ]
    );
    let before = harness.app.project().clone();
    let hide = tree::toggle_bitmap_visible(&before, &one).expect("a command");
    assert_eq!(harness.run(hide).expect("it runs"), "Hide “colour”");
    assert!(!harness.app.project().bitmaps[1].visible);
    assert!(harness.app.project().bitmaps[0].visible, "b1 is untouched");
    let lock = tree::toggle_bitmap_locked(harness.app.project(), &one).expect("a command");
    assert_eq!(harness.run(lock).expect("it runs"), "Lock “colour”");
    assert!(harness.app.project().bitmaps[1].locked);
    assert_eq!(
        tree::bitmap_menu(harness.app.project(), &one),
        [
            MenuItem::Rename,
            MenuItem::Show,
            MenuItem::Unlock,
            MenuItem::Delete,
            MenuItem::ZoomTo,
            MenuItem::AddOperation,
        ]
    );
    // The header's eye: one row is hidden, so it shows them all.
    let show = tree::toggle_bitmap_visible(harness.app.project(), &both).expect("a command");
    assert_eq!(harness.run(show).expect("it runs"), "Show 2 pictures");
    assert!(harness.app.project().bitmaps.iter().all(|x| x.visible));
    harness.app.undo();
    harness.app.undo();
    harness.app.undo();
    assert_eq!(harness.app.project(), &before, "three steps, three undos");

    // Rename: the typed name, trimmed; empty or unchanged is no step.
    let project = harness.app.project().clone();
    assert_eq!(tree::rename_bitmap(&project, &b("b2"), "  "), None);
    assert_eq!(tree::rename_bitmap(&project, &b("b2"), " colour "), None);
    assert_eq!(tree::rename_bitmap(&project, &b("b9"), "x"), None);
    let rename = tree::rename_bitmap(&project, &b("b2"), " portrait ").expect("a command");
    assert_eq!(
        rename,
        Command::RenameBitmap {
            id: b("b2"),
            name: "portrait".into(),
        }
    );
    assert_eq!(harness.run(rename).expect("it runs"), "Rename “colour”");
    assert_eq!(harness.app.project().bitmaps[1].name, "portrait");

    // Delete from the menu: the bitmap goes, its Photo V-Carve stays and
    // validation names what it lost.
    let lost = |harness: &Harness| {
        harness
            .app
            .validation()
            .errors
            .iter()
            .any(|f| f.field.as_deref() == Some("operations[op1].image"))
    };
    assert!(!lost(&harness), "op1 has its picture");
    let delete = tree::bitmap_menu_command(harness.app.project(), MenuItem::Delete, &[b("b1")])
        .expect("a command");
    assert_eq!(delete, Command::DeleteBitmaps(vec![b("b1")]));
    assert_eq!(harness.run(delete).expect("it runs"), "Delete “gradient”");
    let project = harness.app.project();
    let left: Vec<&str> = project.bitmaps.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(left, ["b2"]);
    assert_eq!(project.operations.len(), 1, "the Photo V-Carve stays");
    assert!(
        lost(&harness),
        "and says its picture is gone: {:?}",
        harness.app.validation().errors
    );
    let tree = Tree::build(project);
    assert_eq!(tree.bitmaps.len(), 1);

    // A bitmap the project does not have, or none: refused.
    assert!(matches!(
        harness.run(Command::DeleteBitmaps(vec![b("b1")])),
        Err(CommandError::UnknownBitmap { id }) if id == b("b1")
    ));
    assert!(matches!(
        harness.run(Command::DeleteBitmaps(Vec::new())),
        Err(CommandError::NothingSelected)
    ));
    assert!(matches!(
        harness.run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: None,
            locked: None,
        }),
        Err(CommandError::NothingToDo)
    ));
    assert_eq!(
        tree::bitmap_menu_command(harness.app.project(), MenuItem::Delete, &[b("b1")]),
        None,
        "no command for a row that is gone"
    );
    for item in [MenuItem::Rename, MenuItem::ZoomTo] {
        assert_eq!(
            tree::bitmap_menu_command(harness.app.project(), item, &[b("b2")]),
            None,
            "{item:?} is the window's"
        );
    }
}

/// Capability 56: every visible bitmap is in the Scene, in file order,
/// at its footprint in project space with its pixels, top row first, for
/// the Viewport to draw under the shapes; a hidden one is absent. The
/// pixels are decoded when the picture changes, not on every edit, and
/// each picture has a key the renderer can cache its texture under.
#[test]
fn bitmaps_are_in_the_scene_at_their_footprints() {
    let mut harness = two_pictures();
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    let ids: Vec<&str> = scene.bitmaps().iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["b1", "b2"], "file order: the last is drawn on top");
    for (drawn, bitmap) in scene.bitmaps().iter().zip(&project.bitmaps) {
        assert_eq!(drawn.name, bitmap.name);
        assert_eq!(drawn.locked, bitmap.locked);
        assert_eq!(
            drawn.footprint,
            Rect {
                min: Point::new(bitmap.x_mm, bitmap.y_mm),
                max: Point::new(
                    bitmap.x_mm + bitmap.width_mm,
                    bitmap.y_mm + bitmap.height_mm
                ),
            },
            "{}",
            bitmap.id
        );
        assert_eq!(
            (drawn.width_px, drawn.height_px),
            (bitmap.width_px, bitmap.height_px)
        );
        let pixels = drawn.pixels.as_deref().expect("the picture decodes");
        assert_eq!(pixels, &bitmap.pixels().unwrap(), "{}", bitmap.id);
    }
    let gradient = scene.bitmap(&b("b1")).expect("b1 is drawn").clone();
    let colour = scene.bitmap(&b("b2")).expect("b2 is drawn").clone();
    assert_eq!(
        gradient.footprint,
        Rect {
            min: Point::new(10.0, 10.0),
            max: Point::new(110.0, 60.0),
        }
    );
    assert_ne!(gradient.picture, colour.picture, "two pictures, two keys");
    // No shape, so the content is the pictures.
    assert_eq!(scene.shapes_bounds(), None);
    assert_eq!(
        scene.content_bounds(),
        Some(Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(110.0, 60.0),
        })
    );

    // An edit that leaves the pictures alone rebuilds the Scene without
    // decoding them again.
    let revision = scene.revision();
    harness
        .run(Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(12.0),
        }))
        .expect("stock");
    let scene = harness.app.scene();
    assert_ne!(scene.revision(), revision);
    let again = scene.bitmap(&b("b1")).unwrap();
    assert_eq!(again.picture, gradient.picture);
    assert!(
        Arc::ptr_eq(
            again.pixels.as_ref().unwrap(),
            gradient.pixels.as_ref().unwrap()
        ),
        "the same pixels, shared"
    );

    // Hidden: absent, and out of the content's bounds. Locked: drawn.
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: Some(false),
            locked: None,
        })
        .expect("hides");
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b1")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let scene = harness.app.scene();
    assert!(scene.bitmap(&b("b2")).is_none());
    assert!(scene.bitmap(&b("b1")).expect("still drawn").locked);
    assert_eq!(scene.content_bounds(), Some(gradient.footprint));

    // With shapes, the content is both.
    let mut harness = Harness::from_project(plaque());
    assert!(harness.app.scene().bitmaps().is_empty());
    let shapes = harness.app.scene().shapes_bounds().expect("the plaque");
    assert_eq!(harness.app.scene().content_bounds(), Some(shapes));
    harness
        .app
        .import_picture(&sheet("gradient.png", 400.0, Purpose::Import))
        .expect("imports");
    let scene = harness.app.scene();
    assert_eq!(scene.shapes_bounds(), Some(shapes), "shapes only");
    let content = scene.content_bounds().expect("content");
    assert_eq!(content.min, Point::new(0.0, 0.0));
    assert_eq!(content.max.x.to_bits(), 400.0_f64.to_bits());
    assert!(content.max.y >= shapes.max.y);
    // Undo takes the picture off the Viewport.
    harness.app.undo();
    assert!(harness.app.scene().bitmaps().is_empty());
}

/// A camera at 4 px/mm whose screen point for project (0, 0) is
/// (100, 500).
fn camera() -> Camera {
    let mut camera = Camera {
        centre_mm: Point::new(0.0, 0.0),
        px_per_mm: 4.0,
        viewport_px: Vec2::new(2000.0, 1000.0),
    };
    camera.pan(-900.0, 0.0);
    camera
}

/// [`two_pictures`] with what generation needs set: the stock's
/// thickness, the V-bit's rates and the Photo V-Carve's maximum depth.
fn two_pictures_ready() -> Harness {
    let mut harness = two_pictures();
    let mut stock = harness.app.project().stock.clone();
    stock.thickness_mm = Some(12.0);
    harness.run(Command::SetStock(stock)).expect("a thickness");
    let mut tool = harness.app.project().tools[0].clone();
    tool.feed_mm_min = Some(1000.0);
    tool.plunge_mm_min = Some(300.0);
    tool.rpm = Some(18000);
    harness.run(Command::UpdateTool(tool)).expect("the rates");
    let mut op = harness.app.project().operations[0].clone();
    let Operation::PhotoVCarve(photo) = &mut op else {
        panic!("op1 is the Photo V-Carve");
    };
    photo.max_depth_mm = Some(2.0);
    harness
        .run(Command::UpdateOperation(op))
        .expect("a maximum depth");
    assert!(
        !harness.app.validation().has_errors(),
        "{:?}",
        harness.app.validation().errors
    );
    harness
}

/// Slice 15 (#544); capabilities 56 and 57: a click inside a picture's
/// footprint picks it: the one drawn on top where two overlap (the last
/// in file order), a locked one too, a hidden one never. A picture is
/// under everything else, so it is picked only where the click lands on
/// no shape, tab or toolpath.
#[test]
fn a_click_inside_a_footprint_picks_the_picture_under_everything_else() {
    let mut harness = two_pictures();
    let camera = camera();
    let at = |x: f64, y: f64| camera.to_screen(Point::new(x, y));
    let scene = harness.app.scene().clone();
    // b1 is 100 × 50 at (10, 10); b2 is 20 × 20 at the origin, on top.
    assert_eq!(
        pick::picture_at(&scene, &camera, at(50.0, 30.0)),
        Some(b("b1"))
    );
    assert_eq!(
        pick::picture_at(&scene, &camera, at(5.0, 5.0)),
        Some(b("b2"))
    );
    assert_eq!(
        pick::picture_at(&scene, &camera, at(15.0, 15.0)),
        Some(b("b2")),
        "where they overlap, the one drawn on top"
    );
    assert_eq!(pick::picture_at(&scene, &camera, at(150.0, 30.0)), None);
    assert_eq!(pick::picture_at(&scene, &camera, at(5.0, 30.0)), None);
    assert_eq!(
        pick::picture_at(&scene, &camera, at(110.0, 60.0)),
        Some(b("b1")),
        "its edge is its own"
    );

    // Hidden: not picked, so the picture under it is. Locked: picked.
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: Some(false),
            locked: None,
        })
        .expect("hides");
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b1")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let scene = harness.app.scene().clone();
    assert_eq!(pick::picture_at(&scene, &camera, at(5.0, 5.0)), None);
    assert_eq!(
        pick::picture_at(&scene, &camera, at(15.0, 15.0)),
        Some(b("b1"))
    );

    // Under the shapes: on the plaque with a picture across it, a click
    // on a shape is the shape's, and the picture's only on empty stock.
    let mut harness = Harness::from_project(plaque());
    harness
        .app
        .import_picture(&sheet("gradient.png", 400.0, Purpose::Import))
        .expect("imports");
    let scene = harness.app.scene().clone();
    let shapes = scene.shapes_bounds().expect("the plaque's shapes");
    let outline = scene.shapes()[0].outlines[0].vertices()[0].point();
    let on_shape = camera.to_screen(outline);
    let hit = pick::pick(&scene, &camera, on_shape, PICK_TOLERANCE_PX, 0);
    assert!(matches!(hit, Hit::Instance(_)), "{hit:?}");
    assert_eq!(
        pick::picture_at(&scene, &camera, on_shape),
        Some(b("b1")),
        "the picture lies under it"
    );
    // The shape takes the click (`hit` above); where nothing does, the
    // pick is the picture, last of all; off every footprint, nothing.
    let empty = at(shapes.max.x + 50.0, shapes.max.y + 50.0);
    let hit = pick::pick(&scene, &camera, empty, PICK_TOLERANCE_PX, 0);
    assert_eq!(hit, Hit::Bitmap(b("b1")));
    let off = at(450.0, 30.0);
    let hit = pick::pick(&scene, &camera, off, PICK_TOLERANCE_PX, 0);
    assert_eq!(hit, Hit::Nothing);
    // A click cycle picks it the same way, every time.
    let mut cycle = pick::ClickCycle::default();
    for _ in 0..2 {
        assert_eq!(
            cycle.click(&scene, &camera, empty, PICK_TOLERANCE_PX),
            Hit::Bitmap(b("b1"))
        );
    }
}

/// Capability 57 ("pictures are selected on their own, as tabs are"): a
/// marquee takes every visible picture whose footprint it touches, in
/// file order, and the selection it makes holds pictures alone: those,
/// those added with Shift, each toggled with Ctrl. One that touches no
/// picture clears. b1 is 100 × 50 at (10, 10); b2 is 20 × 20 at the
/// origin.
#[test]
fn a_marquee_takes_the_pictures_it_touches() {
    let harness = two_pictures();
    let scene = harness.app.scene();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Rect {
        min: Point::new(x0, y0),
        max: Point::new(x1, y1),
    };
    let both = pick::pictures_touching(scene, rect(-5.0, -5.0, 15.0, 15.0));
    assert_eq!(both, [b("b1"), b("b2")]);
    let first = pick::pictures_touching(scene, rect(60.0, 40.0, 200.0, 200.0));
    assert_eq!(first, [b("b1")]);
    assert!(pick::pictures_touching(scene, rect(200.0, 200.0, 300.0, 300.0)).is_empty());

    let none = Selection::Nothing;
    assert_eq!(
        none.boxed_pictures(&both, ClickMode::Replace),
        Selection::Bitmaps(both.clone())
    );
    assert_eq!(
        none.boxed_pictures(&[], ClickMode::Replace),
        Selection::Nothing
    );
    let one = Selection::Bitmaps(vec![b("b2")]);
    assert_eq!(
        one.boxed_pictures(&first, ClickMode::Add),
        Selection::Bitmaps(vec![b("b2"), b("b1")])
    );
    assert_eq!(
        one.boxed_pictures(&both, ClickMode::Toggle),
        Selection::Bitmaps(vec![b("b1")]),
        "b1 in, b2 out"
    );
    // A click on a picture is a click among pictures: shapes selected go.
    let shapes = Selection::Instances(vec![dc_project::InstanceId::new("i1")]);
    assert_eq!(
        shapes.clicked(&Hit::Bitmap(b("b1")), ClickMode::Add),
        Selection::Bitmaps(vec![b("b1")])
    );
}

/// Slice 15 (#544); capability 57: clicking a picture's row or the
/// picture selects it; Shift adds, Ctrl toggles. Pictures are selected
/// alone, as tabs are: selecting one drops the shapes, tabs or operation
/// selected, and selecting any of those drops the pictures. Selecting is
/// view state, never an undo step, and a picture that goes leaves the
/// selection.
#[test]
fn a_picture_is_selected_from_its_row_or_the_viewport() {
    let mut harness = two_pictures();
    let steps = harness.app.history().undo_labels().count();
    // The picture just imported is the selection, so it can be moved at
    // once (capability 56).
    assert_eq!(harness.app.selected_pictures(), [b("b2")]);
    harness.app.select(Selection::Nothing);
    assert!(harness.app.selected_pictures().is_empty());

    harness.app.click_picture(&b("b1"), ClickMode::Replace);
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
    assert_eq!(
        harness.app.selection(),
        &Selection::Bitmaps(vec![b("b1")]),
        "the app's one selection holds the pictures"
    );
    harness.app.click_picture(&b("b2"), ClickMode::Add);
    assert_eq!(harness.app.selected_pictures(), [b("b1"), b("b2")]);
    harness.app.click_picture(&b("b2"), ClickMode::Add);
    assert_eq!(harness.app.selected_pictures(), [b("b1"), b("b2")]);
    harness.app.click_picture(&b("b1"), ClickMode::Toggle);
    assert_eq!(harness.app.selected_pictures(), [b("b2")]);
    harness.app.click_picture(&b("b1"), ClickMode::Toggle);
    assert_eq!(harness.app.selected_pictures(), [b("b2"), b("b1")]);
    harness.app.click_picture(&b("b1"), ClickMode::Replace);
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
    harness.app.click_picture(&b("b1"), ClickMode::Toggle);
    assert!(harness.app.selected_pictures().is_empty());
    // A picture the project does not have is not selected.
    harness.app.select_pictures(vec![b("b9"), b("b2")]);
    assert_eq!(harness.app.selected_pictures(), [b("b2")]);
    harness.app.click_picture(&b("b9"), ClickMode::Add);
    assert_eq!(harness.app.selected_pictures(), [b("b2")]);
    // Shift-click in the tree: the rows between.
    let tree = Tree::build(harness.app.project());
    assert_eq!(
        tree.bitmap_range(&b("b2"), &b("b1")),
        [b("b1"), b("b2")],
        "in row order"
    );
    assert_eq!(tree.bitmap_range(&b("b9"), &b("b2")), [b("b2")]);

    // One kind at a time: an operation selected drops the pictures, and
    // a picture selected drops the operation; the editor keeps showing it.
    let op1 = harness.app.project().operations[0].id().clone();
    harness.app.select(Selection::Operation(op1.clone()));
    assert!(harness.app.selected_pictures().is_empty());
    harness.app.click_picture(&b("b2"), ClickMode::Replace);
    assert_eq!(harness.app.selection(), &Selection::Bitmaps(vec![b("b2")]));
    assert_eq!(harness.app.editing(), Some(&op1));
    // A plain click on nothing clears it.
    harness.app.select(Selection::Nothing);
    assert!(harness.app.selected_pictures().is_empty());

    assert_eq!(
        harness.app.history().undo_labels().count(),
        steps,
        "selecting is not an undo step"
    );

    // A picture that goes leaves the selection, and coming back by undo
    // does not select it again.
    harness.app.select_pictures(vec![b("b1"), b("b2")]);
    harness
        .run(Command::DeleteBitmaps(vec![b("b2")]))
        .expect("deletes");
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
    harness.app.undo();
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
    // While an operation's targets are edited nothing else is selected.
    assert!(harness.app.edit_targets(&op1));
    harness.app.click_picture(&b("b2"), ClickMode::Replace);
    assert_eq!(harness.app.selected_pictures(), [b("b1")]);
    harness.app.cancel_targets();
    // A new project has no selection.
    harness
        .app
        .reset(App::new_project().expect("a new project"));
    assert!(harness.app.selected_pictures().is_empty());
}

/// Slice 15 (#544); capabilities 56 and 58: a selected picture's
/// Properties are its footprint's X and Y (its bottom-left corner), W and
/// H, its size in pixels (read-only), its eye and padlock, and the Photo
/// V-Carves that carve it; several pictures show their joint box. A
/// locked selection's fields are read-only, with *Unlock*.
#[test]
fn a_pictures_properties_are_its_position_size_and_pixels() {
    let mut harness = two_pictures();
    let strings = harness.app.strings().clone();
    let project = harness.app.project().clone();
    let one = pictures::properties(&project, &[b("b1")]).expect("b1's properties");
    assert_eq!(one.name.as_deref(), Some("gradient"));
    assert_eq!(one.pixels, Some((64, 32)));
    assert_eq!(
        one.placement.bounds,
        Rect {
            min: Point::new(10.0, 10.0),
            max: Point::new(110.0, 60.0),
        }
    );
    assert_eq!(one.placement.count, 1);
    assert_eq!(one.placement.visible, Some(true));
    assert_eq!(one.placement.locked, Some(false));
    assert!(!one.placement.any_locked);
    assert_eq!(
        one.placement.rotation_deg,
        Some(0.0),
        "a picture never turns"
    );
    assert_eq!(one.placement.targeted_by, ["Photo V-Carve"]);
    assert_eq!(pictures::title(&strings, &one), "Properties · gradient");
    assert_eq!(strings.get(pictures::PIXELS_LABEL), "Pixels");
    assert_eq!(
        pictures::pixels_text(&strings, &one).as_deref(),
        Some("64 × 32 px")
    );
    // The fields are a shape's, with a shape's text.
    let text = |field| properties::field_text(&one.placement, field, Units::Mm);
    assert_eq!(properties::fields(&one.placement), &properties::FIELDS);
    assert_eq!(
        properties::FIELDS.map(text),
        ["10", "10", "100", "50"].map(str::to_owned)
    );
    assert_eq!(
        properties::field_text(&one.placement, Field::W, Units::Inch),
        "3.937"
    );
    assert!(!pictures::read_only(&one, Field::X));
    assert!(!pictures::read_only(&one, Field::Y));

    // Several: their joint box, no one name or pixel size.
    let both = pictures::properties(&project, &[b("b1"), b("b2")]).expect("both");
    assert_eq!(
        both.placement.bounds,
        Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(110.0, 60.0),
        }
    );
    assert_eq!(both.placement.count, 2);
    assert_eq!((both.name.as_deref(), both.pixels), (None, None));
    assert_eq!(pictures::title(&strings, &both), "Properties · 2 pictures");
    assert_eq!(pictures::pixels_text(&strings, &both), None);
    assert_eq!(both.placement.targeted_by, ["Photo V-Carve"]);
    let colour = pictures::properties(&project, &[b("b2")]).expect("b2");
    assert!(colour.placement.targeted_by.is_empty(), "nothing carves it");
    assert_eq!(
        pictures::bounds(&project, &[b("b2")]),
        Some(colour.placement.bounds),
        "what Zoom to frames"
    );

    // Nothing selected, or a picture the project lacks: no properties.
    assert_eq!(pictures::properties(&project, &[]), None);
    assert_eq!(pictures::properties(&project, &[b("b9")]), None);
    assert_eq!(pictures::bounds(&project, &[]), None);

    // Hidden and locked: mixed flags over several, read-only fields, and
    // *Unlock* unlocks the locked ones in one step.
    assert_eq!(pictures::unlock(&project, &[b("b1"), b("b2")]), None);
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: Some(false),
            locked: Some(true),
        })
        .expect("hides and locks");
    let project = harness.app.project().clone();
    let both = pictures::properties(&project, &[b("b1"), b("b2")]).expect("both");
    assert_eq!(
        (both.placement.visible, both.placement.locked),
        (None, None),
        "mixed"
    );
    assert!(both.placement.any_locked);
    assert!(properties::read_only(&both.placement));
    for field in properties::FIELDS {
        assert!(pictures::read_only(&both, field), "{field:?}");
    }
    let unlock = pictures::unlock(&project, &[b("b1"), b("b2")]).expect("a command");
    assert_eq!(
        unlock,
        Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: None,
            locked: Some(false),
        }
    );
    assert_eq!(harness.run(unlock).expect("unlocks"), "Unlock “colour”");
}

/// Slice 15 (#544); capabilities 56 and 58: a picture moves like a
/// shape. Typing X or Y, dragging it or nudging it is one undo step named
/// for the picture; several move as one; a locked picture refuses, and
/// the message names it. The Photo V-Carve that carves it regenerates,
/// its grooves moved with the picture.
#[test]
fn moving_a_picture_is_one_undo_step_and_takes_its_grooves_along() {
    let mut harness = two_pictures_ready();
    harness.generate();
    let op1 = harness.app.project().operations[0].id().clone();
    let before = harness.app.project().clone();
    let cuts_before: Vec<_> = {
        let run = &harness.app.scene().operation(&op1).expect("drawn").runs[0];
        strokes(&run.run.toolpath)
            .into_iter()
            .filter(|s| s.kind == StrokeKind::Cut)
            .collect()
    };
    assert!(!cuts_before.is_empty(), "the grooves");

    // Typed X: the footprint's left edge moves there.
    let one = [b("b1")];
    let props = pictures::properties(&before, &one).expect("b1");
    let typed =
        properties::parse(&props.placement, Field::X, "15", Units::Mm, true).expect("a length");
    assert_eq!(typed, Edit::X { mm: 15.0 });
    let command = pictures::edit(&before, &one, typed).expect("a move");
    assert_eq!(
        command,
        Command::MoveBitmaps {
            ids: one.to_vec(),
            dx_mm: 5.0,
            dy_mm: 0.0,
        }
    );
    assert_eq!(harness.run(command).expect("it moves"), "Move “gradient”");
    let typed = Edit::Y { mm: 13.0 };
    let command = pictures::edit(harness.app.project(), &one, typed).expect("a move");
    harness.run(command).expect("it moves");
    let moved = harness.app.project().bitmaps[0].clone();
    assert_eq!((moved.x_mm, moved.y_mm), (15.0, 13.0));
    assert_eq!(
        dc_project::Bitmap {
            x_mm: 10.0,
            y_mm: 10.0,
            ..moved
        },
        before.bitmaps[0],
        "nothing but its corner changed"
    );
    assert_eq!(harness.app.project().bitmaps[1], before.bitmaps[1]);
    // The value it has is no step.
    assert!(matches!(
        pictures::edit(harness.app.project(), &one, Edit::X { mm: 15.0 }),
        Err(CommandError::NothingToDo)
    ));

    // Its Photo V-Carve regenerates, every groove moved by (5, 3).
    assert!(!harness.app.generation().is_current(&op1), "stale");
    harness.generate();
    let run = &harness.app.scene().operation(&op1).expect("drawn").runs[0];
    let cuts: Vec<_> = strokes(&run.run.toolpath)
        .into_iter()
        .filter(|s| s.kind == StrokeKind::Cut)
        .collect();
    assert_eq!(cuts.len(), cuts_before.len());
    for (now, was) in cuts.iter().zip(&cuts_before) {
        let d = (now.to.x - was.to.x - 5.0)
            .abs()
            .max((now.to.y - was.to.y - 3.0).abs())
            .max((now.to.z - was.to.z).abs());
        assert!(d <= 1e-6, "{now:?} vs {was:?}");
    }
    harness.app.undo();
    harness.app.undo();
    assert_eq!(harness.app.project(), &before, "two steps, two undos");

    // A drag or a nudge: the selection by a vector, several as one.
    let both = [b("b1"), b("b2")];
    let command = pictures::move_by(&before, &both, 2.0, -1.5).expect("a move");
    assert_eq!(
        command,
        Command::MoveBitmaps {
            ids: both.to_vec(),
            dx_mm: 2.0,
            dy_mm: -1.5,
        }
    );
    assert_eq!(harness.run(command).expect("they move"), "Move 2 pictures");
    let corners: Vec<(f64, f64)> = harness
        .app
        .project()
        .bitmaps
        .iter()
        .map(|x| (x.x_mm, x.y_mm))
        .collect();
    assert_eq!(corners, [(12.0, 8.5), (2.0, -1.5)]);
    // Typed X on several moves their joint box's left edge.
    let command =
        pictures::edit(harness.app.project(), &both, Edit::X { mm: 0.0 }).expect("a move");
    harness.run(command).expect("they move");
    let xs: Vec<f64> = harness
        .app
        .project()
        .bitmaps
        .iter()
        .map(|x| x.x_mm)
        .collect();
    assert_eq!(xs, [10.0, 0.0]);
    harness.app.undo();
    harness.app.undo();

    // Nothing to move, nowhere to go, or not a place: refused.
    assert!(matches!(
        pictures::move_by(&before, &[], 1.0, 0.0),
        Err(CommandError::NothingSelected)
    ));
    assert!(matches!(
        pictures::move_by(&before, &one, 0.0, 0.0),
        Err(CommandError::NothingToDo)
    ));
    assert!(matches!(
        pictures::move_by(&before, &[b("b9")], 1.0, 0.0),
        Err(CommandError::UnknownBitmap { id }) if id == b("b9")
    ));
    for bad in [f64::NAN, f64::INFINITY, 1e12] {
        let refused = harness.run(Command::MoveBitmaps {
            ids: one.to_vec(),
            dx_mm: bad,
            dy_mm: 0.0,
        });
        assert!(refused.is_err(), "{bad}: {refused:?}");
    }
    assert_eq!(harness.app.project(), &before, "and nothing changed");

    // Locked: refused, naming the picture; the others stay too.
    harness
        .run(Command::SetBitmapFlags {
            ids: vec![b("b2")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let locked = harness.app.project().clone();
    let refusal = pictures::move_by(&locked, &both, 1.0, 1.0).expect_err("b2 is locked");
    assert_eq!(refusal.to_string(), "picture “colour” is locked");
    assert!(matches!(
        harness.run(Command::MoveBitmaps {
            ids: both.to_vec(),
            dx_mm: 1.0,
            dy_mm: 1.0,
        }),
        Err(CommandError::PictureLocked { names }) if names == ["colour"]
    ));
    assert_eq!(harness.app.project(), &locked);
    assert!(
        pictures::move_by(&locked, &one, 1.0, 1.0).is_ok(),
        "the unlocked one still moves"
    );
}

/// Slice 15 (#544); capabilities 56 and 58: one picture's W or H can be
/// retyped, and the other side follows the pixels, as at import, so the
/// picture is never stretched: the proportion link is always closed. The
/// footprint keeps its centre, as a shape's does. A resize is one undo
/// step named for the picture; a locked picture refuses it, a size that
/// is not a positive length is refused, and several pictures have no one
/// size to type. The Photo V-Carve that carves the picture regenerates.
#[test]
fn resizing_a_picture_keeps_its_proportions_and_its_centre() {
    let mut harness = two_pictures_ready();
    harness.generate();
    let op1 = harness.app.project().operations[0].id().clone();
    let before = harness.app.project().clone();
    let one = [b("b1")];
    let props = pictures::properties(&before, &one).expect("b1");
    for field in properties::FIELDS {
        assert!(!pictures::read_only(&props, field), "{field:?}");
    }

    // Typed W: 100 × 50 at (10, 10) becomes 50 × 25 about (60, 35).
    let typed =
        properties::parse(&props.placement, Field::W, "50", Units::Mm, true).expect("a length");
    assert_eq!(
        typed,
        Edit::W {
            mm: 50.0,
            uniform: true
        }
    );
    let command = pictures::edit(&before, &one, typed).expect("a resize");
    assert_eq!(
        command,
        Command::ResizeBitmap {
            id: b("b1"),
            size: PictureSize::Width { width_mm: 50.0 },
        }
    );
    assert_eq!(
        harness.run(command).expect("it resizes"),
        "Resize “gradient”"
    );
    let resized = harness.app.project().bitmaps[0].clone();
    assert_eq!((resized.width_mm, resized.height_mm), (50.0, 25.0));
    assert_eq!((resized.x_mm, resized.y_mm), (35.0, 22.5));
    assert_eq!(
        dc_project::Bitmap {
            x_mm: 10.0,
            y_mm: 10.0,
            width_mm: 100.0,
            height_mm: 50.0,
            ..resized
        },
        before.bitmaps[0],
        "its pixels, name and flags are untouched"
    );
    assert_eq!(harness.app.project().bitmaps[1], before.bitmaps[1]);
    assert!(
        !harness.app.validation().has_errors(),
        "the footprint keeps the pixels' proportions: {:?}",
        harness.app.validation().errors
    );
    assert!(
        !harness.app.generation().is_current(&op1),
        "its carve is stale"
    );
    harness.generate();
    assert!(harness.app.generation().is_current(&op1));

    // Typed H, the link drawn open or not: the width follows.
    let typed = Edit::H {
        mm: 10.0,
        uniform: false,
    };
    let command = pictures::edit(harness.app.project(), &one, typed).expect("a resize");
    assert_eq!(
        command,
        Command::ResizeBitmap {
            id: b("b1"),
            size: PictureSize::Height { height_mm: 10.0 },
        }
    );
    harness.run(command).expect("it resizes");
    let small = harness.app.project().bitmaps[0].clone();
    assert_eq!((small.width_mm, small.height_mm), (20.0, 10.0));
    assert_eq!((small.x_mm, small.y_mm), (50.0, 30.0));
    // A percentage of the size it has, as a shape's W takes.
    let props = pictures::properties(harness.app.project(), &one).expect("b1");
    let typed = properties::parse(&props.placement, Field::W, "250 %", Units::Mm, true)
        .expect("a percentage");
    let command = pictures::edit(harness.app.project(), &one, typed).expect("a resize");
    harness.run(command).expect("it resizes");
    let again = harness.app.project().bitmaps[0].clone();
    assert_eq!((again.width_mm, again.height_mm), (50.0, 25.0));
    assert_eq!((again.x_mm, again.y_mm), (35.0, 22.5));
    // A width the pixels do not divide evenly still validates.
    let command = pictures::resize(
        harness.app.project(),
        &b("b1"),
        PictureSize::Width { width_mm: 33.3 },
    )
    .expect("a resize");
    harness.run(command).expect("it resizes");
    let odd = harness.app.project().bitmaps[0].clone();
    assert_eq!((odd.width_mm, odd.height_mm), (33.3, 16.65));
    assert!(!harness.app.validation().has_errors());
    // The size it has is no step.
    assert!(matches!(
        pictures::edit(
            harness.app.project(),
            &one,
            Edit::W {
                mm: 33.3,
                uniform: true
            }
        ),
        Err(CommandError::NothingToDo)
    ));
    for _ in 0..4 {
        harness.app.undo().expect("a resize to undo");
    }
    assert_eq!(harness.app.project(), &before, "four steps, four undos");

    // Not a size, or past the coordinate limit: refused, nothing changes.
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY, 1e12] {
        let size = PictureSize::Width { width_mm: bad };
        assert!(
            matches!(
                pictures::resize(&before, &b("b1"), size),
                Err(CommandError::Job(_))
            ),
            "{bad}"
        );
        let refused = harness.run(Command::ResizeBitmap { id: b("b1"), size });
        assert!(refused.is_err(), "{bad}: {refused:?}");
    }
    assert!(matches!(
        pictures::resize(&before, &b("b9"), PictureSize::Width { width_mm: 5.0 }),
        Err(CommandError::UnknownBitmap { id }) if id == b("b9")
    ));
    assert_eq!(harness.app.project(), &before);

    // Several pictures: W and H are shown, not typed.
    let both = [b("b1"), b("b2")];
    let joint = pictures::properties(&before, &both).expect("both");
    assert!(!pictures::read_only(&joint, Field::X));
    assert!(pictures::read_only(&joint, Field::W));
    assert!(pictures::read_only(&joint, Field::H));
    assert!(matches!(
        pictures::edit(
            &before,
            &both,
            Edit::W {
                mm: 50.0,
                uniform: true
            }
        ),
        Err(CommandError::FieldRefused { .. })
    ));

    // Locked: read-only, and refused naming it.
    harness
        .run(Command::SetBitmapFlags {
            ids: one.to_vec(),
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let locked = harness.app.project().clone();
    let props = pictures::properties(&locked, &one).expect("b1");
    assert!(pictures::read_only(&props, Field::W));
    let size = PictureSize::Width { width_mm: 50.0 };
    let refusal = pictures::resize(&locked, &b("b1"), size).expect_err("it is locked");
    assert_eq!(refusal.to_string(), "picture “gradient” is locked");
    assert!(matches!(
        harness.run(Command::ResizeBitmap {
            id: b("b1"),
            size
        }),
        Err(CommandError::PictureLocked { names }) if names == ["gradient"]
    ));
    assert_eq!(harness.app.project(), &locked);
}

/// Slice 15 (#544); capability 67: a Photo V-Carve's target is its
/// picture, so while one is shown in the editor its picture's row carries
/// the target bar and the Scene names the picture; no shape is a target.
#[test]
fn a_photo_v_carves_picture_is_marked_as_its_target() {
    let mut harness = two_pictures();
    let op1 = harness.app.project().operations[0].id().clone();
    assert!(harness.app.scene().targets().is_none());
    harness.app.edit(Some(op1.clone()));
    let marks = harness
        .app
        .scene()
        .targets()
        .expect("op1 is marked")
        .clone();
    assert_eq!(marks.op, op1);
    assert!(marks.targets.is_empty(), "no shape is its target");
    assert_eq!(marks.picture, Some(b("b1")));
    let tree = Tree::build(harness.app.project()).with_targets(Some(&marks));
    assert_eq!(tree.bitmap(&b("b1")).unwrap().target, Some(marks.colour));
    assert_eq!(tree.bitmap(&b("b2")).unwrap().target, None);
    let unmarked = Tree::build(harness.app.project()).with_targets(None);
    assert_eq!(unmarked.bitmap(&b("b1")).unwrap().target, None);
    // Another picture chosen: the mark follows.
    let mut op = harness.app.project().operations[0].clone();
    let Operation::PhotoVCarve(photo) = &mut op else {
        panic!("op1 is the Photo V-Carve");
    };
    photo.image = b("b2");
    harness.run(Command::UpdateOperation(op)).expect("b2");
    let marks = harness.app.scene().targets().expect("still marked");
    assert_eq!(marks.picture, Some(b("b2")));
    harness.app.edit(None);
    assert!(harness.app.scene().targets().is_none());
}

/// Slice 15 (#544); capabilities 57 and 65: a Pictures row's menu offers
/// *Add operation ▸*, whose menu for the row is the Operations step's: a
/// Photo V-Carve on that picture.
#[test]
fn a_bitmap_rows_menu_adds_a_photo_v_carve_on_it() {
    let harness = two_pictures();
    let strings = harness.app.strings().clone();
    let project = harness.app.project();
    assert!(tree::bitmap_menu(project, &[b("b2")]).contains(&MenuItem::AddOperation));
    assert_eq!(
        tree::bitmap_menu_command(project, MenuItem::AddOperation, &[b("b2")]),
        None,
        "a submenu, not a command"
    );
    let menu = operations::add_menu(project, &strings, &[], &[b("b2")]);
    assert_eq!(menu.available, [Kind::PhotoVCarve]);
    let Some(Command::AddOperation(Operation::PhotoVCarve(photo))) =
        operations::add(project, &strings, Kind::PhotoVCarve, &[], &[b("b2")])
    else {
        panic!("a Photo V-Carve to add");
    };
    assert_eq!(photo.image, b("b2"));
    assert_eq!(photo.name, "Photo V-Carve 1", "op1 is `Photo V-Carve`");
    assert_eq!(photo.tool.as_str(), "t1", "init's V-bit");
}
