// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 4 (#535; functional spec, capabilities 56 and 57):
//! what the window draws of a project's bitmaps. Each is a row of the
//! Design Tree's Pictures section, with a name, an eye and a padlock, and
//! each visible one is in the Scene at its footprint, with its pixels, for
//! the Viewport to draw under the shapes.

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
use dc_app::test_support::Harness;
use dc_app::tree::{self, MenuItem, Row, Section, Tree};
use dc_app::{App, Command, CommandError};
use dc_geometry::{Point, Rect};
use dc_import::ImageFormat;
use dc_project::{BitmapId, Stock};

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
