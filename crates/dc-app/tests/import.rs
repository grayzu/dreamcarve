// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 4 (#535): the Import drawing sheet's model
//! (`docs/specs/phase-4/design.md`, "Test plan → Import"; functional
//! spec, capability 55 and acceptance 64). The sheet reads the bytes the
//! shell hands it with `dc-import`, as `dreamcarve init` does with its
//! flags: the DXF units (required when the file declares none), the join
//! tolerance and the layers. What it imports is what the importer returns
//! for those options, placed as drawn, as one undo step, and its notes
//! reach the warnings once.
//!
//! The picture half (capability 56, acceptance 65): the Add picture
//! sheet's model decodes the bytes with `dc_import::bitmap`, takes a
//! width or a height with the other following the pixels, and imports the
//! bitmap `dreamcarve image add` adds. A file dropped on the window opens
//! the sheet for its kind, or the project.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::Path;

use dc_app::import::{
    DrawingFormat, DrawingImport, Dropped, FileKind, PictureImport, PictureSide, Purpose,
};
use dc_app::test_support::Harness;
use dc_app::{App, Command, CommandError, Selection, Strings};
use dc_geometry::Point;
use dc_import::{
    DxfImportOptions, DxfUnits, ImageFormat, ImportWarning, JOIN_TOLERANCE_DEFAULT_MM,
};
use dc_job::Drawing;
use dc_job::picture::{PictureSize, picture_into};
use dc_project::{BitmapId, Grayscale, Stock, Units};

use common::{fixture, read};

/// The sheet over `tests/fixtures/<path>`, to import into the open
/// project.
fn sheet(path: &str) -> DrawingImport {
    let file = fixture(path);
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let format = if path.ends_with(".dxf") {
        DrawingFormat::Dxf
    } else {
        DrawingFormat::Svg
    };
    DrawingImport::open(format, &name, read(&file), Purpose::Import)
}

/// What `dc_import::import_dxf` returns for `tests/fixtures/<path>` with
/// `options`: what `dreamcarve init` reads for the same flags.
fn imported(path: &str, options: &DxfImportOptions) -> (dc_import::DxfImport, Vec<ImportWarning>) {
    dc_import::import_dxf(&read(&fixture(path)), options).expect("the fixture imports")
}

/// The DXF the sheet would import, or `None` when *Import* is disabled.
fn sheet_dxf(sheet: &DrawingImport) -> Option<dc_import::DxfImport> {
    match sheet.command()? {
        Command::ImportDrawing {
            drawing: Drawing::Dxf(import),
            ..
        } => Some(import),
        other => panic!("not a DXF import: {other:?}"),
    }
}

/// An empty project, as *New* makes it.
fn empty() -> Harness {
    Harness::from_project(App::new_project().expect("a new project"))
}

/// Acceptance 64: a DXF with no `$INSUNITS` disables *Import* until units
/// are chosen, with the message the CLI prints for `init` without
/// `--units`. Choosing them imports what `init --units` reads; a file that
/// declares units reads them, and the dropdown overrides them.
#[test]
fn dxf_without_units_blocks_until_chosen() {
    let mut sheet = sheet("dxf/no_units.dxf");
    assert!(sheet.units_required(), "the file declares no units");
    assert_eq!(sheet.declared_units(), None);
    // The importer's refusal, which `init` prints after `cannot import
    // <file>: ` (its `--units` hint names a flag the app does not have).
    let refusal = dc_import::import_dxf(
        &read(&fixture("dxf/no_units.dxf")),
        &DxfImportOptions::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(refusal.contains("$INSUNITS"), "{refusal}");
    assert_eq!(sheet.blocked().as_deref(), Some(refusal.as_str()));
    assert!(sheet.command().is_none(), "Import is disabled");
    assert!(sheet.summary().is_none(), "nothing is read yet");

    for units in [Units::Inch, Units::Mm] {
        sheet.set_units(Some(units));
        assert_eq!(sheet.blocked(), None, "{units:?} chosen");
        let options = DxfImportOptions {
            units: Some(units),
            ..DxfImportOptions::new()
        };
        assert_eq!(
            sheet_dxf(&sheet),
            Some(imported("dxf/no_units.dxf", &options).0),
            "{units:?}: what `init --units` reads"
        );
        assert!(sheet.summary().is_some());
        assert!(
            sheet.units_required(),
            "still the file's lack, not the choice"
        );
    }
    sheet.set_units(None);
    assert_eq!(sheet.blocked().as_deref(), Some(refusal.as_str()));
    assert!(
        sheet.command().is_none(),
        "unset again, Import is disabled again"
    );

    // The plaque declares millimetres: read from the file, overridable.
    let mut plaque = self::sheet("plaque.dxf");
    assert!(!plaque.units_required());
    assert_eq!(plaque.declared_units(), Some(DxfUnits::Millimetres));
    assert_eq!(plaque.blocked(), None);
    assert_eq!(
        sheet_dxf(&plaque),
        Some(imported("plaque.dxf", &DxfImportOptions::new()).0)
    );
    plaque.set_units(Some(Units::Inch));
    let inch = DxfImportOptions {
        units: Some(Units::Inch),
        ..DxfImportOptions::new()
    };
    assert_eq!(sheet_dxf(&plaque), Some(imported("plaque.dxf", &inch).0));

    // The join tolerance starts at the importer's 0.01 mm and is passed on.
    assert!((plaque.join_tolerance_mm() - JOIN_TOLERANCE_DEFAULT_MM).abs() < 1e-12);
    plaque.set_units(None);
    plaque.set_join_tolerance_mm(0.0);
    let exact = DxfImportOptions {
        join_tolerance_mm: 0.0,
        ..DxfImportOptions::new()
    };
    assert_eq!(sheet_dxf(&plaque), Some(imported("plaque.dxf", &exact).0));
    plaque.set_join_tolerance_mm(-1.0);
    assert!(
        plaque
            .blocked()
            .is_some_and(|m| m.contains("join tolerance")),
        "a negative tolerance is the importer's refusal: {:?}",
        plaque.blocked()
    );

    // An SVG has no options.
    let svg = self::sheet("sign.svg");
    assert_eq!(svg.format(), DrawingFormat::Svg);
    assert!(!svg.units_required());
    assert!(svg.layers().is_empty());
    assert_eq!(svg.blocked(), None);
}

/// Acceptance 64: every layer is listed with what it holds and starts
/// ticked; ticking one layer imports that layer only, as `init --layer`
/// does; with none ticked there is nothing to import. Imported into an
/// empty project, the shapes are the ones `init` reads, named by layer,
/// placed as drawn and selected.
#[test]
fn layer_subset() {
    let mut sheet = sheet("plaque.dxf");
    let (all, _) = imported("plaque.dxf", &DxfImportOptions::new());
    assert_eq!(sheet.layers(), all.layers.as_slice());
    let names: Vec<String> = sheet.layers().iter().map(|l| l.name.clone()).collect();
    assert!(names.len() > 1, "the plaque has several layers: {names:?}");
    assert!(
        names.iter().all(|n| sheet.is_ticked(n)),
        "every layer starts ticked"
    );
    let summary = sheet.summary().expect("the plaque reads");
    assert_eq!(summary.layers, names.len());
    assert_eq!(summary.closed + summary.open, all.definitions.len());
    assert_eq!(summary.closed, 9, "init targets nine closed shapes");
    assert_eq!(summary.open, 1, "and leaves one open path");

    for name in names.iter().filter(|n| *n != "holes") {
        sheet.set_layer(name, false);
    }
    assert!(sheet.is_ticked("holes"));
    let holes = DxfImportOptions {
        layers: Some(vec!["holes".to_owned()]),
        ..DxfImportOptions::new()
    };
    let (expected, _) = imported("plaque.dxf", &holes);
    assert_eq!(
        sheet_dxf(&sheet),
        Some(expected.clone()),
        "init --layer holes"
    );
    assert_eq!(
        sheet.layers(),
        all.layers.as_slice(),
        "the list is the file's"
    );
    let summary = sheet.summary().unwrap();
    assert_eq!((summary.closed, summary.open), (4, 0));

    let mut harness = empty();
    let label = harness
        .app
        .import_drawing(&sheet)
        .expect("the holes import");
    assert_eq!(label, "Import plaque.dxf");
    let project = harness.app.project();
    let got: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
    let want: Vec<&str> = expected.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(got, want, "named by layer, as init names them");
    let (placed, ids) =
        dc_job::drawing_into(&App::new_project().unwrap(), Drawing::Dxf(expected.clone()));
    assert_eq!(project, &placed, "placed as drawn");
    assert_eq!(harness.app.selection(), &Selection::Instances(ids));

    sheet.set_layer("holes", false);
    assert!(
        sheet.command().is_none(),
        "no layer ticked, nothing to import"
    );
    assert!(sheet.blocked().is_some());
    sheet.set_layer("holes", true);
    assert!(sheet.command().is_some());
}

/// Acceptance 64: entities the importer does not read are listed on their
/// layer, a layer of nothing else has no shapes to import, and the
/// warning reaches the Warnings panel's notes (`App::notes`, #538) once,
/// under the import's name, and stays once through later edits and
/// generation. Opening a project clears it.
#[test]
fn unread_entities_noted_once() {
    let sheet = sheet("dxf/unsupported.dxf");
    let notes = sheet
        .layers()
        .iter()
        .find(|l| l.name == "notes")
        .expect("the notes layer is listed");
    assert_eq!(notes.closed + notes.open, 0, "nothing on it is read");
    let unread: Vec<(&str, usize)> = notes
        .skipped
        .iter()
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    assert_eq!(unread, [("MTEXT", 1), ("TEXT", 1)]);
    let skipped: Vec<&ImportWarning> = sheet
        .warnings()
        .iter()
        .filter(|w| matches!(w, ImportWarning::DxfEntitiesSkipped { .. }))
        .collect();
    assert_eq!(skipped.len(), 1, "one warning for every unread entity");
    let warning = skipped[0].to_string();

    let noted = |harness: &Harness| -> Vec<String> {
        harness
            .app
            .notes()
            .iter()
            .filter(|note| note.contains("dxf_entities_skipped"))
            .cloned()
            .collect()
    };
    let mut harness = empty();
    assert!(noted(&harness).is_empty());
    harness
        .app
        .import_drawing(&sheet)
        .expect("the file imports");
    let found = noted(&harness);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0], format!("unsupported.dxf: {warning}"));
    assert_eq!(
        harness.app.notes().len(),
        sheet.warnings().len(),
        "one note per importer warning"
    );

    harness
        .run(Command::SetStock(Stock {
            width_mm: 200.0,
            height_mm: 150.0,
            thickness_mm: Some(12.0),
        }))
        .expect("stock");
    harness.generate();
    assert_eq!(noted(&harness).len(), 1, "still once after an edit");

    harness
        .app
        .open(&read(&fixture("plaque.dcv")), None)
        .expect("the plaque opens");
    assert!(noted(&harness).is_empty(), "another project, no notes");
}

/// Acceptance 64: one import is one undo step, labelled with the file's
/// name; undo returns the project as it was, redo brings the shapes back.
/// Into a project that has shapes, the new ones are appended under new
/// ids and become the selection.
#[test]
fn one_import_is_one_undo_step() {
    let mut harness = Harness::open(&read(&fixture("plaque.dcv"))).expect("the plaque opens");
    let before = harness.app.project().clone();
    let sheet = sheet("plaque.dxf");
    harness
        .app
        .import_drawing(&sheet)
        .expect("the drawing imports");
    let after = harness.app.project().clone();
    let (all, _) = imported("plaque.dxf", &DxfImportOptions::new());
    let (expected, ids) = dc_job::drawing_into(&before, Drawing::Dxf(all));
    assert_eq!(after, expected);
    assert_eq!(harness.app.selection(), &Selection::Instances(ids));
    assert_eq!(
        after.instances.len(),
        before.instances.len() + sheet.summary().map_or(0, |s| s.closed + s.open)
    );
    assert_eq!(harness.app.undo_label(), Some("Import plaque.dxf"));

    assert_eq!(harness.app.undo().as_deref(), Some("Import plaque.dxf"));
    assert_eq!(harness.app.project(), &before, "one step undoes the import");
    assert!(!harness.app.can_undo(), "and it was the only step");
    assert_eq!(harness.app.redo().as_deref(), Some("Import plaque.dxf"));
    assert_eq!(harness.app.project(), &after);
}

/// The Add picture sheet over `tests/fixtures/<path>`.
fn picture(path: &str, purpose: Purpose) -> PictureImport {
    let file = fixture(path);
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    PictureImport::open(picture_format(&file), &name, &read(&file), purpose)
}

/// The picture format `file`'s extension names.
fn picture_format(file: &Path) -> ImageFormat {
    file.extension()
        .and_then(|e| ImageFormat::from_extension(&e.to_string_lossy()))
        .expect("a picture's extension")
}

/// What `dc_import::bitmap::import` decodes `tests/fixtures/<path>` to:
/// the pixels `dreamcarve image add` places, and its warnings.
fn decoded(path: &str) -> (Grayscale, Vec<ImportWarning>) {
    let file = fixture(path);
    dc_import::bitmap::import(&read(&file), picture_format(&file)).expect("the fixture decodes")
}

/// Acceptance 65: importing `fixtures/photo/gradient.png` at width 100
/// adds the bitmap `image add --width 100` adds, at the origin, with the
/// grayscale note for a colour file. The project is otherwise as it was,
/// the file the app saves is the one `image add` writes, and a second
/// picture takes the next id.
#[test]
fn picture_at_width_equals_image_add() {
    let strings = Strings::default();
    let mut sheet = picture("photo/gradient.png", Purpose::Import);
    assert_eq!(sheet.format(), ImageFormat::Png);
    assert_eq!(sheet.name(), "gradient.png");
    assert_eq!(sheet.stem(), "gradient");
    assert_eq!(sheet.size_px(), Some((64, 32)));
    assert!(sheet.warnings().is_empty(), "a gray picture needs no note");
    sheet.set_width_mm(100.0);
    assert_eq!(sheet.blocked(&strings), None);

    let mut harness = Harness::open(&read(&fixture("plaque.dcv"))).expect("the plaque opens");
    let before = harness.app.project().clone();
    let notes_before = harness.app.notes().to_vec();
    let label = harness
        .app
        .import_picture(&sheet)
        .expect("the picture imports");
    assert_eq!(label, "Import gradient.png");

    // `image add plaque.dcv gradient.png --width 100`: the decoded
    // picture, named by the file's stem, at 0,0
    // (`crates/dreamcarve/src/image.rs`).
    let (pixels, _) = decoded("photo/gradient.png");
    let (expected, id) = picture_into(
        &before,
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 100.0 },
        Point::new(0.0, 0.0),
    )
    .expect("image add places it");
    assert_eq!(id, BitmapId::new("b1"));
    let project = harness.app.project().clone();
    assert_eq!(project, expected, "the project `image add` writes");
    assert_eq!(
        dc_project::save(&project).unwrap(),
        dc_project::save(&expected).unwrap(),
        "byte for byte"
    );
    let bitmap = project.bitmaps.last().expect("a bitmap");
    assert_eq!(
        (bitmap.x_mm, bitmap.y_mm, bitmap.width_mm, bitmap.height_mm),
        (0.0, 0.0, 100.0, 50.0),
        "at the origin, 100 mm wide, the height from the pixels"
    );
    assert_eq!((bitmap.width_px, bitmap.height_px), (64, 32));
    assert!(bitmap.visible && !bitmap.locked);
    assert_eq!(bitmap.pixels().expect("the stored picture"), pixels);
    assert_eq!(project.stock, before.stock, "the stock is left alone");
    assert_eq!(project.instances, before.instances);
    assert_eq!(project.operations, before.operations);
    assert_eq!(harness.app.notes(), notes_before, "nothing to note");

    // A colour file: the note `image add` prints (`warning: <code>:
    // <message>`), in the sheet before the import and in the Warnings
    // panel's notes after it, once, under the file's name.
    let mut colour = picture("photo/colour.jpg", Purpose::Import);
    assert_eq!(colour.format(), ImageFormat::Jpeg);
    assert_eq!(colour.warnings(), [ImportWarning::ImageColourConverted]);
    colour.set_height_mm(20.0);
    harness
        .app
        .import_picture(&colour)
        .expect("the colour picture imports");
    let (gray, warnings) = decoded("photo/colour.jpg");
    assert_eq!(warnings, [ImportWarning::ImageColourConverted]);
    let noted: Vec<String> = harness
        .app
        .notes()
        .iter()
        .skip(notes_before.len())
        .cloned()
        .collect();
    assert_eq!(
        noted,
        [format!(
            "colour.jpg: image_colour_converted: {}",
            ImportWarning::ImageColourConverted
        )]
    );
    let (expected, id) = picture_into(
        &project,
        "colour",
        &gray,
        PictureSize::Height { height_mm: 20.0 },
        Point::new(0.0, 0.0),
    )
    .unwrap();
    assert_eq!(id, BitmapId::new("b2"), "the next free id");
    assert_eq!(harness.app.project(), &expected);
}

/// Capability 56: the sheet asks for the width *or* the height, in
/// millimetres here (the window converts from the project's units), and
/// the other follows the pixels; *Import* is disabled until one is typed,
/// and with a size no picture can have.
#[test]
fn picture_size_follows_the_pixels() {
    let strings = Strings::default();
    let mut sheet = picture("photo/gradient.png", Purpose::Import);
    assert_eq!(sheet.size(), None, "no size is invented");
    assert_eq!(sheet.footprint_mm(), None);
    assert!(sheet.command().is_none(), "Import is disabled");
    assert_eq!(
        sheet.blocked(&strings).as_deref(),
        Some("Type the picture's width or its height")
    );

    sheet.set_width_mm(100.0);
    assert_eq!(sheet.side(), PictureSide::Width);
    assert_eq!(sheet.size(), Some(PictureSize::Width { width_mm: 100.0 }));
    assert_eq!(sheet.footprint_mm(), Some((100.0, 50.0)));
    assert_eq!(sheet.blocked(&strings), None);
    assert!(sheet.command().is_some());

    sheet.set_height_mm(30.0);
    assert_eq!(sheet.side(), PictureSide::Height);
    assert_eq!(sheet.size(), Some(PictureSize::Height { height_mm: 30.0 }));
    assert_eq!(sheet.footprint_mm(), Some((60.0, 30.0)));
    // What the command carries is what `image add --height 30` places.
    let (pixels, _) = decoded("photo/gradient.png");
    match sheet.command().expect("Import is enabled") {
        Command::ImportPicture {
            name,
            pixels: carried,
            size,
        } => {
            assert_eq!(name, "gradient.png");
            assert_eq!(*carried, pixels);
            assert_eq!(size, PictureSize::Height { height_mm: 30.0 });
        }
        other => panic!("not a picture import: {other:?}"),
    }

    // A size that is no length is the refusal `image add` prints.
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        sheet.set_width_mm(bad);
        let refusal = PictureSize::Width { width_mm: bad }
            .footprint_mm(64, 32)
            .unwrap_err()
            .to_string();
        assert_eq!(sheet.blocked(&strings), Some(refusal), "{bad}");
        assert!(sheet.command().is_none(), "{bad}");
        assert_eq!(sheet.footprint_mm(), None, "{bad}");
    }
    // A footprint past the coordinate limit is `image add`'s refusal too.
    sheet.set_width_mm(1.0e9);
    let refusal = picture_into(
        &App::new_project().unwrap(),
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 1.0e9 },
        Point::new(0.0, 0.0),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(sheet.blocked(&strings), Some(refusal));
    assert!(sheet.command().is_none());

    sheet.clear_size();
    assert_eq!(sheet.size(), None);
    assert!(sheet.blocked(&strings).is_some());
}

/// A file that is not the picture its name says is the importer's refusal,
/// which `image add` prints after `cannot import <file>: `; the sheet
/// shows it, nothing imports, and the app refuses with the same reason.
#[test]
fn unreadable_picture_blocks_with_the_cli_message() {
    let strings = Strings::default();
    let png = read(&fixture("photo/gradient.png"));
    let cases: [(&str, ImageFormat, Vec<u8>); 2] = [
        ("gradient.jpg", ImageFormat::Jpeg, png),
        ("notes.png", ImageFormat::Png, b"not a picture".to_vec()),
    ];
    for (name, format, bytes) in cases {
        let refusal = dc_import::bitmap::import(&bytes, format)
            .unwrap_err()
            .to_string();
        let mut sheet = PictureImport::open(format, name, &bytes, Purpose::Import);
        assert_eq!(sheet.size_px(), None, "{name}");
        assert_eq!(sheet.blocked(&strings), Some(refusal.clone()), "{name}");
        sheet.set_width_mm(100.0);
        assert_eq!(
            sheet.blocked(&strings),
            Some(refusal.clone()),
            "{name}: a size does not make it readable"
        );
        assert!(sheet.command().is_none(), "{name}");

        let mut harness = empty();
        let before = harness.app.project().clone();
        match harness.app.import_picture(&sheet) {
            Err(CommandError::NotImportable { reason }) => assert_eq!(reason, refusal, "{name}"),
            other => panic!("{name}: expected a refusal, got {other:?}"),
        }
        assert_eq!(harness.app.project(), &before, "{name}");
        assert!(!harness.app.can_undo(), "{name}");
    }
}

/// One picture import is one undo step, labelled with the file's name;
/// undo returns the project as it was, redo brings the bitmap back. The
/// new picture becomes the selection, alone, as imported shapes do, so it
/// can be moved at once (capability 56).
#[test]
fn one_picture_import_is_one_undo_step() {
    let mut harness = Harness::open(&read(&fixture("plaque.dcv"))).expect("the plaque opens");
    let before = harness.app.project().clone();
    let selected = Selection::Instances(vec![before.instances[0].id.clone()]);
    harness.app.select(selected);
    let mut sheet = picture("photo/gradient.png", Purpose::Import);
    sheet.set_width_mm(40.0);
    harness
        .app
        .import_picture(&sheet)
        .expect("the picture imports");
    let after = harness.app.project().clone();
    assert_eq!(after.bitmaps.len(), before.bitmaps.len() + 1);
    let added = after.bitmaps.last().expect("the new bitmap").id.clone();
    assert_eq!(harness.app.selection(), &Selection::Bitmaps(vec![added]));
    assert_eq!(harness.app.undo_label(), Some("Import gradient.png"));
    assert!(harness.app.is_dirty());

    assert_eq!(harness.app.undo().as_deref(), Some("Import gradient.png"));
    assert_eq!(harness.app.project(), &before, "one step undoes the import");
    assert!(!harness.app.can_undo(), "and it was the only step");
    assert_eq!(harness.app.redo().as_deref(), Some("Import gradient.png"));
    assert_eq!(harness.app.project(), &after);
}

/// Capabilities 51, 55 and 56: what a file dropped on the window is, by
/// its name, and what the app makes of it. A `.dcv` opens, a drawing
/// opens the Add drawing sheet's model, a picture the Add picture sheet's,
/// each to import into the open project; anything else is refused with a
/// message naming the file, and nothing changes.
#[test]
fn a_dropped_file_opens_its_sheet_or_the_project() {
    assert_eq!(FileKind::of("plaque.dcv"), Some(FileKind::Project));
    assert_eq!(
        FileKind::of("sign.svg"),
        Some(FileKind::Drawing(DrawingFormat::Svg))
    );
    assert_eq!(
        FileKind::of("PLAQUE.DXF"),
        Some(FileKind::Drawing(DrawingFormat::Dxf))
    );
    assert_eq!(
        FileKind::of("photo.png"),
        Some(FileKind::Picture(ImageFormat::Png))
    );
    for name in ["photo.jpg", "photo.JPEG"] {
        assert_eq!(
            FileKind::of(name),
            Some(FileKind::Picture(ImageFormat::Jpeg)),
            "{name}"
        );
    }
    for name in ["notes.txt", "README", "plaque.dcv.bak", "svg"] {
        assert_eq!(FileKind::of(name), None, "{name}");
    }

    let drop = |harness: &mut Harness, path: &str| {
        let file = fixture(path);
        harness.app.drop_file(&file, read(&file))
    };
    let mut harness = empty();
    let untouched = harness.app.project().clone();

    for path in ["sign.svg", "plaque.dxf"] {
        match drop(&mut harness, path).expect("a drawing's sheet") {
            Dropped::Drawing(sheet) => {
                assert_eq!(sheet.name(), path);
                assert_eq!(sheet.purpose(), Purpose::Import);
                assert_eq!(*sheet, self::sheet(path), "{path}: File ▸ Add's sheet");
                assert!(sheet.command().is_some(), "{path}: ready to import");
            }
            other => panic!("{path}: not a drawing's sheet: {other:?}"),
        }
        assert_eq!(harness.app.project(), &untouched, "{path}: nothing yet");
    }

    for path in ["photo/gradient.png", "photo/colour.jpg"] {
        match drop(&mut harness, path).expect("a picture's sheet") {
            Dropped::Picture(sheet) => {
                assert_eq!(sheet.purpose(), Purpose::Import);
                assert_eq!(*sheet, picture(path, Purpose::Import), "{path}");
                assert!(sheet.size_px().is_some(), "{path}: decoded");
                assert_eq!(sheet.size(), None, "{path}: the size is the user's");
            }
            other => panic!("{path}: not a picture's sheet: {other:?}"),
        }
        assert_eq!(harness.app.project(), &untouched, "{path}: nothing yet");
        assert!(!harness.app.can_undo(), "{path}");
    }

    let file = fixture("plaque.dcv");
    match drop(&mut harness, "plaque.dcv").expect("the project opens") {
        Dropped::Project(opened) => assert!(
            opened
                .problems
                .iter()
                .all(|p| p.severity() != dc_project::Severity::Error)
        ),
        other => panic!("not a project: {other:?}"),
    }
    assert_eq!(harness.app.project(), &common::plaque());
    assert_eq!(harness.app.source(), Some(file.as_path()));
    assert!(!harness.app.is_dirty() && !harness.app.can_undo());

    // Not a file DreamCarve reads, and a `.dcv` that is not a project:
    // refused, the open project kept.
    let plaque = harness.app.project().clone();
    match harness
        .app
        .drop_file(Path::new("notes.txt"), b"hello".to_vec())
    {
        Err(CommandError::NotImportable { reason }) => {
            assert!(reason.contains("notes.txt"), "{reason}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(matches!(
        harness
            .app
            .drop_file(Path::new("broken.dcv"), b"not json".to_vec()),
        Err(CommandError::Load(_))
    ));
    assert_eq!(harness.app.project(), &plaque);
    assert_eq!(harness.app.source(), Some(file.as_path()));
}

/// Slice 35 (#627), capability 55 and acceptance 103: the sheet's preview
/// is the drawing alone, for an import and for a new project: its bounds
/// are the drawing's, as drawn (no shift onto a stock), and it holds one
/// outline per profile the drawing has, so no stock outline.
#[test]
fn preview_is_the_drawing_alone() {
    for path in ["plaque.dxf", "sign.svg"] {
        for purpose in [Purpose::Import, Purpose::New] {
            let file = fixture(path);
            let format = if path.ends_with(".dxf") {
                DrawingFormat::Dxf
            } else {
                DrawingFormat::Svg
            };
            let model = DrawingImport::open(format, path, read(&file), purpose);
            let drawing = model.drawing().expect("the fixture imports");
            let preview = model.preview().expect("a preview");
            assert_eq!(
                Some(preview.bounds),
                drawing.bounds(),
                "{path} {purpose:?}: the preview's bounds are the drawing's"
            );
            let (definitions, instances) = match drawing {
                Drawing::Svg(import) => (&import.definitions, &import.instances),
                Drawing::Dxf(import) => (&import.definitions, &import.instances),
            };
            let profiles: usize = instances
                .iter()
                .filter_map(|i| definitions.iter().find(|d| d.id == i.definition))
                .map(|d| d.profiles.len())
                .sum();
            assert_eq!(
                preview.outlines.len(),
                profiles,
                "{path} {purpose:?}: one outline per profile, no stock outline"
            );
            let stock = App::new_project().expect("a new project").stock;
            let fitted = dc_job::project_from::fit_stock(
                preview.bounds,
                dc_job::project_from::STOCK_MARGIN_MM,
            );
            for outline in &preview.outlines {
                let b = outline.bounds().expect("an outline has bounds");
                for (w, h) in [
                    (stock.width_mm, stock.height_mm),
                    (fitted.width_mm, fitted.height_mm),
                ] {
                    assert!(
                        !(b.min.x.abs() < 1e-9
                            && b.min.y.abs() < 1e-9
                            && (b.max.x - w).abs() < 1e-9
                            && (b.max.y - h).abs() < 1e-9),
                        "{path} {purpose:?}: an outline is a stock box"
                    );
                }
            }
        }
    }
}

/// The lines `dreamcarve init` prints for `checks`: `warning: <code>:
/// <message>`.
fn check_lines(checks: &[dc_job::Check]) -> Vec<String> {
    checks.iter().map(dc_job::Check::line).collect()
}

/// The codes of `checks`, in order.
fn check_codes(checks: &[dc_job::Check]) -> Vec<&'static str> {
    checks.iter().map(|c| c.code).collect()
}

/// Capability 55, acceptance 106: the sheet over an SVG lists the checks
/// `dreamcarve init` prints for the file, in its order and its words. The
/// messy drawing has one of each kind; the four messages below are the
/// book's (*Preparing artwork*, "The checks `init` prints"), which its
/// doc-test holds to the binary. The checks do not disable *Import*, the
/// importer's own warnings about what a check counts are not listed
/// twice, and a clean drawing lists none and points nowhere.
#[test]
fn svg_sheet_lists_the_checks_init_prints() {
    let sheet = sheet("prep/messy.svg");
    let bytes = read(&fixture("prep/messy.svg"));
    let printed: Vec<String> =
        dc_import::prep::findings(&bytes, &dc_import::SvgImportOptions::new())
            .expect("the checks run")
            .iter()
            .map(|w| format!("warning: {}: {w}", w.code()))
            .collect();
    assert_eq!(check_lines(sheet.checks()), printed);
    assert_eq!(
        check_codes(sheet.checks()),
        [
            "shapes_overlap",
            "svg_hole_as_separate_path",
            "svg_stroke_only_path",
            "svg_hidden_object",
            "svg_object_not_convertible",
            "svg_object_not_convertible",
            "svg_object_not_convertible",
            "feature_below_size",
        ]
    );
    let message = |code: &str| -> &str {
        &sheet
            .checks()
            .iter()
            .find(|c| c.code == code)
            .unwrap_or_else(|| panic!("no {code}"))
            .message
    };
    assert_eq!(
        message("shapes_overlap"),
        "1 pair(s) of shapes overlap; the first is `sq-a` and `sq-b`, meeting near (18.00, \
         95.00) mm; unite them or move them apart; `dreamcarve image-prep` would fix this"
    );
    assert_eq!(
        message("svg_stroke_only_path"),
        "1 path(s) have a stroke and no fill; the first is `centreline`, stroked 1.200 mm \
         wide. Only its centreline is imported, an open path no Inlay or V-Carve can cut; turn \
         strokes into shapes; `dreamcarve image-prep` would fix this"
    );
    assert_eq!(
        message("svg_hidden_object"),
        "1 object(s) are not drawn (display:none) but are imported and would be cut; the \
         first is `old-layer`; skip them; `dreamcarve image-prep` would fix this"
    );
    assert_eq!(
        message("feature_below_size"),
        "1 piece(s) or hole(s) are narrower than 0.200 mm at their widest, 0.2% of the \
         design's 100.0 mm height; the smallest is 0.087 mm. Specks that small are not worth \
         carving; `dreamcarve image-prep` would fix this"
    );
    assert!(
        sheet
            .checks()
            .iter()
            .all(|c| c.tier() == dc_job::Tier::Warning),
        "the CLI prints each as a warning"
    );

    assert_eq!(sheet.blocked(), None, "a check never disables Import");
    assert!(sheet.command().is_some());
    assert!(sheet.advises_prep(), "the sheet points to image-prep");
    assert!(
        !sheet
            .warnings()
            .iter()
            .any(dc_job::checks::counted_by_svg_checks),
        "the checks stand in for the importer's text, image and clip warnings: {:?}",
        sheet.warnings()
    );

    let clean = self::sheet("prep/clean.svg");
    assert!(clean.checks().is_empty(), "{:?}", clean.checks());
    assert!(!clean.advises_prep());
}

/// Capability 55, acceptance 106: a DXF has no paint, so its sheet lists
/// the two checks `check` reads from shapes, read from the shapes the
/// import would add. The tray's two pockets overlap and its speck is too
/// small; unticking the layer that holds the pockets takes the overlap
/// off the list and ticking it brings it back, so the list is of what
/// *Import* adds. The lines are what `dc_job::checks::drawing` reads from
/// the importer's result for the same options.
#[test]
fn dxf_sheet_lists_the_checks_of_the_shapes_it_would_add() {
    let mut sheet = sheet("prep/tool-tray.dxf");
    assert_eq!(
        check_codes(sheet.checks()),
        ["shapes_overlap", "feature_below_size"]
    );
    assert_eq!(
        sheet.checks()[0].message,
        "1 pair(s) of shapes overlap; the first is `pockets-1` and `pockets-2`, meeting near \
         (98.43, 14.29) mm; unite them or move them apart; `dreamcarve image-prep` would fix this"
    );
    let (all, _) = imported("prep/tool-tray.dxf", &DxfImportOptions::new());
    assert_eq!(
        check_lines(sheet.checks()),
        check_lines(&dc_job::checks::drawing(&Drawing::Dxf(all)))
    );
    assert!(sheet.advises_prep());
    assert_eq!(sheet.blocked(), None, "a check never disables Import");

    sheet.set_layer("pockets", false);
    assert_eq!(check_codes(sheet.checks()), ["feature_below_size"]);
    let (subset, _) = imported(
        "prep/tool-tray.dxf",
        &DxfImportOptions {
            layers: Some(vec!["hatched".into(), "speck".into(), "tray".into()]),
            ..DxfImportOptions::new()
        },
    );
    assert_eq!(
        check_lines(sheet.checks()),
        check_lines(&dc_job::checks::drawing(&Drawing::Dxf(subset)))
    );
    sheet.set_layer("speck", false);
    assert!(sheet.checks().is_empty(), "{:?}", sheet.checks());
    assert!(!sheet.advises_prep(), "nothing fired, nothing to prepare");

    sheet.set_layer("pockets", true);
    assert_eq!(check_codes(sheet.checks()), ["shapes_overlap"]);

    // A file that cannot be read yet has no checks.
    let no_units = self::sheet("dxf/no_units.dxf");
    assert!(no_units.blocked().is_some());
    assert!(no_units.checks().is_empty());
}

/// Capabilities 55 and 76, acceptance 106: importing keeps the checks.
/// The two a project's shapes can be read for are the app's findings from
/// then on (`App::artwork_checks`), named as the project names the
/// shapes; the others, which only the drawing's paint could say, are
/// notes under the file's name with their codes, beside the importer's
/// own. Nothing is listed twice.
#[test]
fn importing_keeps_the_checks_as_findings() {
    let sheet = sheet("prep/messy.svg");
    let mut harness = empty();
    assert!(harness.app.artwork_checks().is_empty());
    harness
        .app
        .import_drawing(&sheet)
        .expect("the drawing imports");

    assert_eq!(
        check_codes(harness.app.artwork_checks()),
        ["shapes_overlap", "feature_below_size"]
    );
    assert_eq!(
        check_lines(harness.app.artwork_checks()),
        check_lines(&dc_job::checks::project(harness.app.project())),
        "what check prints for the project"
    );
    let overlap = &harness.app.artwork_checks()[0];
    let named: Vec<&str> = overlap
        .shapes
        .iter()
        .map(|id| {
            harness
                .app
                .project()
                .instances
                .iter()
                .find(|i| &i.id == id)
                .expect("the shape is in the project")
                .name
                .as_str()
        })
        .collect();
    assert_eq!(named, ["sq-a", "sq-b"]);

    let notes = harness.app.notes();
    assert_eq!(notes, sheet.notes(harness.app.strings()));
    let noted = |code: &str| notes.iter().filter(|n| n.contains(code)).count();
    assert_eq!(noted("messy.svg: svg_stroke_only_path: 1 path(s)"), 1);
    assert_eq!(noted("messy.svg: svg_hidden_object: 1 object(s)"), 1);
    assert_eq!(noted("messy.svg: svg_hole_as_separate_path: "), 1);
    assert_eq!(noted("messy.svg: svg_object_not_convertible: "), 3);
    assert_eq!(noted("shapes_overlap"), 0, "the project's own: {notes:?}");
    assert_eq!(noted("feature_below_size"), 0, "{notes:?}");
    assert_eq!(
        notes.len(),
        sheet.warnings().len() + 6,
        "the importer's warnings and the six checks of the drawing's paint"
    );

    // A new project round the drawing has them too, and a DXF's checks
    // are all the project's own: no note is added for them.
    let mut harness = empty();
    harness
        .app
        .new_from_drawing(&sheet)
        .expect("the project is made");
    assert_eq!(
        check_codes(harness.app.artwork_checks()),
        ["shapes_overlap", "feature_below_size"]
    );
    assert_eq!(harness.app.notes(), sheet.notes(harness.app.strings()));

    let tray = self::sheet("prep/tool-tray.dxf");
    let mut harness = empty();
    harness.app.import_drawing(&tray).expect("the tray imports");
    assert_eq!(
        check_lines(harness.app.artwork_checks()),
        check_lines(tray.checks()),
        "placed as drawn, the shapes read as the sheet read them"
    );
    assert_eq!(harness.app.notes().len(), tray.warnings().len());
}

/// Capability 55, acceptance 106: the quick start's coaster is a circle
/// drawn as a hairline with no fill, as most drawings for a cut-out are,
/// and `init` says so (`svg_stroke_only_path`). The sheet lists it and
/// the import keeps it as a note, but it is advice about a shape that was
/// imported whole, so it is not among what the import left out and the
/// Design step is complete: the first project a new user makes is not
/// marked as needing attention. What the messy drawing's import did
/// leave out (its text, its image, its clipped group) marks the step, as
/// the importer's warnings about them did.
#[test]
fn an_outline_drawn_as_a_stroke_is_noted_and_marks_nothing() {
    use dc_app::steps::{self, Session, State, Step};

    let bytes = read(&common::repo_root().join("fixtures/coaster.svg"));
    let coaster = DrawingImport::open(DrawingFormat::Svg, "coaster.svg", bytes, Purpose::New);
    assert_eq!(check_codes(coaster.checks()), ["svg_stroke_only_path"]);
    assert!(coaster.warnings().is_empty());
    assert_eq!(coaster.left_out(), 0, "the circle was imported whole");

    let mut harness = empty();
    harness
        .app
        .new_from_drawing(&coaster)
        .expect("the project is made");
    let notes = harness.app.notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("coaster.svg: svg_stroke_only_path: 1 path(s) have a stroke"),
        "{notes:?}"
    );
    assert!(harness.app.artwork_checks().is_empty());
    let session = Session {
        import_warnings: coaster.left_out(),
        ..Session::default()
    };
    let states = steps::states(&harness.app, &session);
    assert_eq!(states.of(Step::Design), State::Complete);
    assert_eq!(steps::opening(&states), Step::Stock);

    let messy = sheet("prep/messy.svg");
    assert_eq!(
        messy.left_out(),
        messy.warnings().len() + 3,
        "the importer's warnings, and the text, the image and the clipped group"
    );
}
