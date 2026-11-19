// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4: the app saves what the CLI saves (functional
//! acceptance 62) and knows when the file on disk is behind (acceptance 88).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::{App, Command, Prefs, Strings};
use dc_project::{Stock, Units};

use common::{checked_projects, plaque_bytes, read};

/// Acceptance 62: every committed project opened in the app and saved is
/// byte-identical to what the CLI writes for it (the original, migrated to
/// the current version), and the history is empty after open.
#[test]
fn save_unchanged_is_byte_identical() {
    for (stem, path) in checked_projects() {
        let bytes = read(&path);
        let harness = Harness::open(&bytes).unwrap_or_else(|e| panic!("{stem}: {e}"));
        let expected = dc_project::save(&dc_project::load(&bytes).unwrap().project).unwrap();
        let saved = harness
            .app
            .serialize()
            .unwrap_or_else(|e| panic!("{stem}: {e}"));
        assert_eq!(
            saved, expected,
            "{stem}: the app's file differs from the CLI's"
        );
        assert!(!harness.app.can_undo(), "{stem}: history is not empty");
        assert!(!harness.app.can_redo(), "{stem}");
    }
}

/// Acceptance 88: the project is dirty exactly when a save would change
/// the file: not after open, after an edit, not after undoing it, after
/// redoing it, not after the shell reports a save, and a new project is
/// clean until edited.
#[test]
fn dirty_tracks_the_saved_bytes() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    assert!(!harness.app.is_dirty(), "an opened project is clean");
    harness
        .run(Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(18.0),
        }))
        .expect("stock");
    assert!(harness.app.is_dirty());
    harness.app.undo();
    assert!(!harness.app.is_dirty(), "undo restored the saved bytes");
    harness.app.redo();
    assert!(harness.app.is_dirty());
    let text = harness.app.serialize().expect("serializes");
    harness.app.mark_saved(text.as_bytes(), None);
    assert!(!harness.app.is_dirty(), "the shell wrote the file");
    harness
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    assert!(harness.app.is_dirty());

    let mut fresh = App::new(
        App::new_project().expect("the defaults build"),
        Strings::default(),
        Prefs::default(),
    );
    assert!(!fresh.is_dirty(), "a new project is clean");
    fresh
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    assert!(fresh.is_dirty());
    fresh.undo();
    assert!(!fresh.is_dirty());
}

/// Capability 51: *New* makes an empty project with the phase 1 defaults.
#[test]
fn new_project_has_the_phase_1_defaults() {
    let project = App::new_project().expect("the defaults build");
    assert_eq!(project.version, dc_project::CURRENT_VERSION);
    assert_eq!(project.display_units, Units::Mm);
    assert_eq!(project.stock.width_mm.to_bits(), 100.0_f64.to_bits());
    assert_eq!(project.stock.height_mm.to_bits(), 100.0_f64.to_bits());
    assert_eq!(project.stock.thickness_mm, None);
    assert_eq!(project.origin.xy, dc_project::OriginXy::BottomLeft);
    assert_eq!(project.origin.z, dc_project::OriginZ::StockTop);
    assert_eq!(project.post_processor.id, "grblhal");
    assert_eq!(
        project.post_processor.definition_version,
        dc_project::latest_definition("grblhal").unwrap().version
    );
    assert!(project.tools.is_empty());
    assert!(project.operations.is_empty());
    assert!(project.instances.is_empty());
    // The one thing validation refuses is what capability 51 leaves unset:
    // the thickness, which only the user knows.
    let errors: Vec<Option<String>> = dc_project::validate(&project)
        .iter()
        .filter(|p| p.severity() == dc_project::Severity::Error)
        .map(dc_project::Problem::field)
        .collect();
    assert_eq!(errors, vec![Some("stock.thickness_mm".to_owned())]);
}

/// Acceptance 63 (capability 51, open question 1): *New from drawing* on
/// `fixtures/coaster.svg` yields the project `dreamcarve init` writes for
/// it with the placeholder tool and the Profile removed: the same stock,
/// instances and post-processor, no tools, no operations. It replaces the
/// open project and its history, and has no file yet.
#[test]
fn new_from_drawing_equals_init_without_defaults() {
    use dc_app::import::{DrawingFormat, DrawingImport, Purpose};

    let bytes = read(&common::repo_root().join("fixtures/coaster.svg"));
    let sheet = DrawingImport::open(
        DrawingFormat::Svg,
        "coaster.svg",
        bytes.clone(),
        Purpose::New,
    );
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 200.0,
            thickness_mm: Some(18.0),
        }))
        .expect("stock");
    harness
        .app
        .new_from_drawing(&sheet)
        .expect("a project from the coaster");

    // What `init coaster.svg` builds (`crates/dreamcarve/src/init.rs`), its
    // placeholder tool and Profile then removed.
    let (import, _) =
        dc_import::import_svg(&bytes, &dc_import::SvgImportOptions::default()).unwrap();
    let post = dc_project::latest_definition("grblhal")
        .unwrap()
        .default_ref();
    let mut init = dc_job::project_from::drawing(
        "coaster",
        dc_job::Drawing::Svg(import),
        post,
        Units::Mm,
        true,
    )
    .unwrap();
    assert_eq!((init.tools.len(), init.operations.len()), (1, 1));
    init.tools.clear();
    init.operations.clear();
    let project = harness.app.project();
    assert_eq!(project, &init);
    assert_eq!(
        dc_project::save(project).unwrap(),
        dc_project::save(&init).unwrap()
    );
    assert!(project.tools.is_empty() && project.operations.is_empty());
    assert!(!harness.app.can_undo(), "a new project has no history");
    assert_eq!(harness.app.source(), None, "and no file yet");
}

/// Capability 51: *New from picture* on `fixtures/photo/gradient.png` at
/// width 100 yields the project `dreamcarve init gradient.png --width 100`
/// writes: the stock sized round the picture with `init`'s margin, the
/// bitmap `b1` placed that margin in, the 60° V-bit and one Photo V-Carve
/// with `init`'s defaults. It replaces the open project and its history,
/// has no file yet, and carries the importer's notes.
#[test]
fn new_from_picture_equals_init() {
    use dc_app::import::{PictureImport, Purpose};
    use dc_import::ImageFormat;
    use dc_job::picture::PictureSize;

    let strings = Strings::default();
    let bytes = read(&common::fixture("photo/gradient.png"));
    let mut sheet = PictureImport::open(ImageFormat::Png, "gradient.png", &bytes, Purpose::New);
    assert_eq!(sheet.purpose(), Purpose::New);
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    let edited = harness.app.project().clone();
    let replaced = harness.app.replaced();

    // No size yet: nothing is made, the open project stays.
    assert!(sheet.blocked(&strings).is_some());
    assert!(harness.app.new_from_picture(&sheet).is_err());
    assert_eq!(harness.app.project(), &edited);
    assert!(harness.app.can_undo());

    sheet.set_width_mm(100.0);
    assert_eq!(sheet.blocked(&strings), None);
    harness
        .app
        .new_from_picture(&sheet)
        .expect("a project from the gradient");

    // What `init gradient.png --width 100` builds
    // (`crates/dreamcarve/src/init.rs`).
    let (pixels, warnings) = dc_import::bitmap::import(&bytes, ImageFormat::Png).unwrap();
    assert!(warnings.is_empty());
    let post = dc_project::latest_definition("grblhal")
        .unwrap()
        .default_ref();
    let init = dc_job::project_from::picture(
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 100.0 },
        post,
        Units::Mm,
    )
    .unwrap();
    let project = harness.app.project();
    assert_eq!(project, &init);
    assert_eq!(
        dc_project::save(project).unwrap(),
        dc_project::save(&init).unwrap()
    );
    assert_eq!(project.name, "gradient");
    assert_eq!(
        (project.stock.width_mm, project.stock.height_mm),
        (120.0, 70.0),
        "the picture's 100 × 50 mm with a 10 mm margin all round"
    );
    let bitmap = &project.bitmaps[0];
    assert_eq!((bitmap.x_mm, bitmap.y_mm), (10.0, 10.0));
    assert_eq!((project.tools.len(), project.operations.len()), (1, 1));
    assert!(matches!(
        &project.operations[0],
        dc_project::Operation::PhotoVCarve(photo) if photo.image == bitmap.id
    ));
    assert!(project.instances.is_empty());
    assert!(!harness.app.can_undo(), "a new project has no history");
    assert!(!harness.app.is_dirty());
    assert_eq!(harness.app.source(), None, "and no file yet");
    assert_eq!(harness.app.replaced(), replaced + 1);
    assert!(harness.app.notes().is_empty(), "a gray picture has no note");

    // A colour picture's conversion is noted in the new project.
    let jpg = read(&common::fixture("photo/colour.jpg"));
    let mut colour = PictureImport::open(ImageFormat::Jpeg, "colour.jpg", &jpg, Purpose::New);
    colour.set_height_mm(25.0);
    harness
        .app
        .new_from_picture(&colour)
        .expect("a project from the colour picture");
    assert_eq!(harness.app.project().name, "colour");
    assert_eq!(
        harness.app.notes(),
        [format!(
            "colour.jpg: image_colour_converted: {}",
            dc_import::ImportWarning::ImageColourConverted
        )]
    );

    // A picture whose stock would pass the coordinate limit is `init`'s
    // refusal, in the sheet and from the app; the project stays.
    let kept = harness.app.project().clone();
    colour.set_width_mm(dc_geometry::MAX_COORDINATE_MM);
    let refusal = dc_job::project_from::picture(
        "colour",
        &dc_import::bitmap::import(&jpg, ImageFormat::Jpeg)
            .unwrap()
            .0,
        PictureSize::Width {
            width_mm: dc_geometry::MAX_COORDINATE_MM,
        },
        dc_project::latest_definition("grblhal")
            .unwrap()
            .default_ref(),
        Units::Mm,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(colour.blocked(&strings), Some(refusal));
    assert!(harness.app.new_from_picture(&colour).is_err());
    assert_eq!(harness.app.project(), &kept);
}
