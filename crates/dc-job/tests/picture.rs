// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures in a project (`docs/specs/phase-3/design.md`, "CLI" and "3.5
//! Photo V-Carve", slice 23; functional spec, capability 46, acceptance
//! 58): the library twins of `crates/dreamcarve/tests/cli/picture.rs`, so
//! the app's *Import picture* (#544) gets what `init` and `image add` get.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::Point;
use dc_import::ImageFormat;
use dc_import::bitmap;
use dc_job::picture::{PictureSize, picture_into, picture_rows};
use dc_job::preview::render;
use dc_job::{JobError, ProgramSplit, project_from};
use dc_project::{
    BitmapId, Grayscale, Operation, OperationId, PhotoStrategy, PostProcessorRef, Severity, ToolId,
    ToolKind, Units, validate,
};

use common::{DATE, load_dcv, repo_root};

/// `tests/fixtures/photo/gradient.png`, decoded: 64 × 32 pixels.
fn gradient() -> Grayscale {
    let bytes = std::fs::read(repo_root().join("tests/fixtures/photo/gradient.png")).unwrap();
    let (pixels, warnings) = bitmap::import(&bytes, ImageFormat::Png).unwrap();
    assert_eq!(warnings, []);
    pixels
}

/// The generator's tests build the gradient in code
/// (`dc_project::test_support::gradient_bytes`); it must stay the
/// committed fixture's picture.
#[test]
fn gradient_fixture_matches_the_test_support_picture() {
    let pixels = gradient();
    assert_eq!((pixels.width_px, pixels.height_px), (64, 32));
    assert_eq!(pixels.bytes, dc_project::test_support::gradient_bytes());
}

fn post() -> PostProcessorRef {
    PostProcessorRef::latest("grblhal").unwrap()
}

/// Acceptance 58, the library half: a 64 × 32 picture 100 mm wide is one
/// bitmap of 100 × 50 mm, placed `STOCK_MARGIN_MM` in from the stock's
/// corner with the stock sized round it, carved by one Photo V-Carve
/// with `init`'s values; 50 mm tall gives the same project.
#[test]
fn picture_project_holds_the_bitmap_and_a_photo_v_carve() {
    let pixels = gradient();
    let project = project_from::picture(
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 100.0 },
        post(),
        Units::Mm,
    )
    .unwrap();

    let margin = project_from::STOCK_MARGIN_MM;
    assert_eq!(project.name, "gradient");
    assert_eq!(project.bitmaps.len(), 1);
    let picture = &project.bitmaps[0];
    assert_eq!(picture.id, BitmapId::new("b1"));
    assert_eq!(picture.name, "gradient");
    assert_eq!((picture.width_px, picture.height_px), (64, 32));
    assert_eq!((picture.width_mm, picture.height_mm), (100.0, 50.0));
    assert_eq!((picture.x_mm, picture.y_mm), (margin, margin));
    assert!(picture.visible && !picture.locked);
    assert_eq!(picture.pixels().unwrap(), pixels);
    assert_eq!(project.stock.width_mm, 100.0 + 2.0 * margin);
    assert_eq!(project.stock.height_mm, 50.0 + 2.0 * margin);
    assert_eq!(project.stock.thickness_mm, None);
    assert!(project.definitions.is_empty() && project.instances.is_empty());

    // One V-bit, `t1`, 60°, 6.35 mm, nothing a machine needs invented.
    assert_eq!(project.tools.len(), 1);
    let tool = &project.tools[0];
    assert_eq!(tool.id, ToolId::new("t1"));
    assert_eq!(
        tool.cutter,
        ToolKind::VBit {
            included_angle_deg: 60.0
        }
    );
    assert_eq!(tool.diameter_mm, 6.35);
    assert_eq!(
        (tool.feed_mm_min, tool.plunge_mm_min, tool.rpm),
        (None, None, None)
    );

    // One Photo V-Carve of it, with the values of the design spec.
    let [Operation::PhotoVCarve(op)] = project.operations.as_slice() else {
        panic!("one Photo V-Carve, not {:?}", project.operations);
    };
    assert_eq!(op.id, OperationId::new("op1"));
    assert!(op.enabled);
    assert_eq!(op.image, BitmapId::new("b1"));
    assert_eq!(op.tool, ToolId::new("t1"));
    assert_eq!(op.max_depth_mm, None);
    assert_eq!(op.start_depth_mm, 0.0);
    assert!(!op.start_plane_cleared_externally);
    assert_eq!(op.line_angle_deg, 22.5);
    assert_eq!(op.line_spacing_fraction, 1.1);
    assert_eq!(op.strategy, PhotoStrategy::Raster);
    assert!(!op.invert);
    assert_eq!(op.sample_mm, 0.25);
    assert_eq!(op.stepover_retract_mm, 1.0);
    assert_eq!(op.through_cut_allowance_mm, 0.0);
    assert_eq!(op.safe_z_mm, 5.0);

    // Every error is a value the user must set: the thickness, the max
    // depth and the tool's feeds and speed.
    let errors: Vec<_> = validate(&project)
        .into_iter()
        .filter(|p| p.severity() == Severity::Error)
        .collect();
    assert!(
        errors.iter().all(dc_project::Problem::is_unset_value),
        "{errors:?}"
    );
    let fields: Vec<String> = errors.iter().filter_map(|p| p.field()).collect();
    for field in [
        "stock.thickness_mm",
        "operations[op1].max_depth_mm",
        "tools[t1].feed_mm_min",
    ] {
        assert!(fields.iter().any(|f| f == field), "{field} in {fields:?}");
    }

    // 50 mm tall is the same project.
    let tall = project_from::picture(
        "gradient",
        &pixels,
        PictureSize::Height { height_mm: 50.0 },
        post(),
        Units::Mm,
    )
    .unwrap();
    assert_eq!(tall, project);
    // And it saves and loads as itself.
    let text = dc_project::save(&project).unwrap();
    assert_eq!(dc_project::load(text.as_bytes()).unwrap().project, project);
}

/// A size that is not a positive length, or a footprint past the
/// coordinate limit, is refused.
#[test]
fn picture_size_must_be_a_positive_length() {
    let pixels = gradient();
    for size_mm in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        for size in [
            PictureSize::Width { width_mm: size_mm },
            PictureSize::Height { height_mm: size_mm },
        ] {
            let result = project_from::picture("g", &pixels, size, post(), Units::Mm);
            assert!(
                matches!(result, Err(JobError::PictureSize { .. })),
                "{size:?}: {result:?}"
            );
        }
    }
    let result = project_from::picture(
        "g",
        &pixels,
        PictureSize::Width {
            width_mm: dc_geometry::MAX_COORDINATE_MM,
        },
        post(),
        Units::Mm,
    );
    assert!(
        matches!(result, Err(JobError::StockTooLarge { .. })),
        "{result:?}"
    );
    let plaque = load_dcv(&repo_root().join("tests/fixtures/plaque.dcv"));
    let result = picture_into(
        &plaque,
        "g",
        &pixels,
        PictureSize::Width { width_mm: 60.0 },
        Point::new(dc_geometry::MAX_COORDINATE_MM, 0.0),
    );
    assert!(
        matches!(result, Err(JobError::PictureOutOfRange { .. })),
        "{result:?}"
    );
}

/// `image add`'s library half: a picture 60 mm wide at (20, 30) is the
/// next bitmap, `b1` then `b2`, with the footprint 20..80 × 30..60 mm; the
/// project's own content is untouched. The proportions are the pixels',
/// whichever side is given, rounded to the micrometre.
#[test]
fn picture_into_appends_the_next_bitmap() {
    let pixels = gradient();
    let plaque = load_dcv(&repo_root().join("tests/fixtures/plaque.dcv"));
    let (project, id) = picture_into(
        &plaque,
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 60.0 },
        Point::new(20.0, 30.0),
    )
    .unwrap();
    assert_eq!(id, BitmapId::new("b1"));
    let picture = &project.bitmaps[0];
    assert_eq!(
        (
            picture.x_mm,
            picture.y_mm,
            picture.x_mm + picture.width_mm,
            picture.y_mm + picture.height_mm
        ),
        (20.0, 30.0, 80.0, 60.0)
    );
    assert_eq!(picture.name, "gradient");
    let mut without = project.clone();
    without.bitmaps.clear();
    assert_eq!(without, plaque);

    let (twice, second) = picture_into(
        &project,
        "again",
        &pixels,
        PictureSize::Height { height_mm: 7.0 },
        Point::new(0.0, 0.0),
    )
    .unwrap();
    assert_eq!(second, BitmapId::new("b2"));
    assert_eq!(twice.bitmaps.len(), 2);
    assert_eq!(
        (twice.bitmaps[1].width_mm, twice.bitmaps[1].height_mm),
        (14.0, 7.0)
    );
    // Proportions that do not divide evenly: rounded, within the
    // validation's tolerance.
    let odd = Grayscale {
        width_px: 7,
        height_px: 3,
        bytes: vec![128; 21],
    };
    let (odd_project, _) = picture_into(
        &plaque,
        "odd",
        &odd,
        PictureSize::Width { width_mm: 100.0 },
        Point::new(0.0, 0.0),
    )
    .unwrap();
    assert_eq!(odd_project.bitmaps[0].height_mm, 42.857_143);
    assert!(
        validate(&odd_project)
            .iter()
            .all(|p| p.code() != "bitmap_footprint_aspect")
    );
}

/// `image list`'s rows: each bitmap's id, name, pixel size from its PNG's
/// header, footprint, corner, and the Photo V-Carves naming it.
#[test]
fn picture_rows_name_the_photo_v_carves_using_each() {
    let mut project = dc_project::test_support::photo_project();
    let (with_two, _) = picture_into(
        &project,
        "gradient",
        &gradient(),
        PictureSize::Width { width_mm: 60.0 },
        Point::new(20.0, 30.0),
    )
    .unwrap();
    project = with_two;
    let rows = picture_rows(&project).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, BitmapId::new("b1"));
    assert_eq!(rows[0].name, "ramp");
    assert_eq!((rows[0].width_px, rows[0].height_px), (4, 2));
    assert_eq!((rows[0].width_mm, rows[0].height_mm), (40.0, 20.0));
    assert_eq!((rows[0].x_mm, rows[0].y_mm), (15.0, 15.0));
    assert_eq!(rows[0].used_by, [OperationId::new("o8")]);
    assert_eq!(rows[1].id, BitmapId::new("b2"));
    assert_eq!((rows[1].width_px, rows[1].height_px), (64, 32));
    assert!(rows[1].used_by.is_empty());
}

/// Design spec, "CLI" (security review L5): the preview draws each bitmap
/// as an `<image>` at its footprint, Y flipped, stretched to it
/// (`preserveAspectRatio="none"`), under the shapes and the toolpaths,
/// with a data URI re-encoded from the decoded pixels, not the file's
/// string.
#[test]
fn preview_draws_each_bitmap_under_the_toolpaths() {
    let plaque = load_dcv(&repo_root().join("tests/fixtures/plaque.dcv"));
    let pixels = gradient();
    let (mut project, _) = picture_into(
        &plaque,
        "gradient",
        &pixels,
        PictureSize::Width { width_mm: 60.0 },
        Point::new(20.0, 30.0),
    )
    .unwrap();
    // The stored text is padded with a second encoding's bytes the
    // preview must not copy: the same pixels, a different PNG.
    let reencoded = bitmap::stored(&pixels).unwrap();
    project.bitmaps[0].png_base64 = other_png_of(&pixels);
    assert_ne!(project.bitmaps[0].png_base64, reencoded);

    let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    let generated = built.generated.expect("plaque generates");
    let svg = dc_job::preview::render_generated(&project, &generated);
    let images: Vec<&str> = svg.lines().filter(|l| l.contains("<image")).collect();
    assert_eq!(images.len(), 1, "{svg}");
    let image = images[0];
    let height = project.stock.height_mm;
    assert!(image.contains(r#"x="20.000""#), "{image}");
    assert!(
        image.contains(&format!(r#"y="{:.3}""#, height - 60.0)),
        "{image}"
    );
    assert!(image.contains(r#"width="60.000""#), "{image}");
    assert!(image.contains(r#"height="30.000""#), "{image}");
    assert!(image.contains(r#"preserveAspectRatio="none""#), "{image}");
    assert!(
        image.contains(&format!(r#"href="data:image/png;base64,{reencoded}""#)),
        "{image}"
    );
    // Under everything else drawn on the stock.
    let at = svg.find("<image").unwrap();
    for later in [r#"class="shapes""#, r#"class="operation"#] {
        let found = svg
            .find(later)
            .unwrap_or_else(|| panic!("{later} in {svg}"));
        assert!(at < found, "the picture is drawn before {later}");
    }
    // A project with no bitmap draws no image.
    assert!(!render(&plaque, &[]).contains("<image"));
}

/// The pixels as a PNG another program might write, at another
/// compression, so its bytes are not the stored encoding's.
fn other_png_of(pixels: &Grayscale) -> String {
    use base64::Engine as _;
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixels.width_px, pixels.height_px);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fastest);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels.bytes).unwrap();
    writer.finish().unwrap();
    base64::engine::general_purpose::STANDARD.encode(out)
}
