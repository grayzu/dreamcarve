// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures (sub-phase 3.5, slice 23, #645; functional spec, capability
//! 46, acceptance 58; `docs/specs/phase-3/design.md`, "CLI"): `init` on a
//! PNG or JPEG, `image add`, `image list`, and the picture under the
//! preview. The library halves are `crates/dc-job/tests/picture.rs`.

use super::*;

use dc_import::ImageFormat;
use dc_project::{BitmapId, PhotoStrategy, ToolKind};

/// Copies `tests/fixtures/photo/<name>` into `dir` as `as_name`.
fn copy_photo(dir: &Path, name: &str, as_name: &str) -> PathBuf {
    copy_fixture(dir, &format!("photo/{name}"), as_name)
}

/// Sets what `init` leaves unset on a picture's project, so it exports:
/// the thickness, the Photo V-Carve's max depth, the V-bit's feeds and
/// speed.
fn set_photo_values(dcv: &Path) {
    let mut project = load_file(dcv).project;
    project.stock.thickness_mm = Some(12.0);
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(1000.0);
        tool.plunge_mm_min = Some(300.0);
        tool.rpm = Some(18_000);
    }
    for op in &mut project.operations {
        if let Operation::PhotoVCarve(op) = op {
            op.max_depth_mm = Some(1.5);
        }
    }
    std::fs::write(dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// Acceptance 58: `init gradient.png --width 100` writes one bitmap of
/// 100 × 50 mm (placed with `init`'s margin, the design spec's rule) and
/// one Photo V-Carve naming it, with the design spec's values, and lists
/// what to set; `--height 50` writes the same file; both flags, or
/// neither, exit 2 naming both.
#[test]
fn init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve() {
    let dir = work_dir("init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve");
    copy_photo(&dir, "gradient.png", "gradient.png");
    let out = run(&dir, &["init", "gradient.png", "--width", "100"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    let project = load_file(&dir.join("gradient.dcv")).project;

    let [picture] = project.bitmaps.as_slice() else {
        panic!("one bitmap, not {:?}", project.bitmaps);
    };
    assert_eq!(picture.id, BitmapId::new("b1"));
    assert_eq!(picture.name, "gradient");
    assert_eq!((picture.width_px, picture.height_px), (64, 32));
    assert_eq!((picture.width_mm, picture.height_mm), (100.0, 50.0));
    // Inside the stock with `init`'s margin all round.
    assert_eq!((picture.x_mm, picture.y_mm), (10.0, 10.0));
    assert_eq!(
        (project.stock.width_mm, project.stock.height_mm),
        (120.0, 70.0)
    );
    let png = std::fs::read(dir.join("gradient.png")).expect("png readable");
    let (pixels, _) = dc_import::bitmap::import(&png, ImageFormat::Png).expect("png reads");
    assert_eq!(picture.pixels().expect("stored picture reads"), pixels);

    let [tool] = project.tools.as_slice() else {
        panic!("one tool, not {:?}", project.tools);
    };
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
    let [Operation::PhotoVCarve(op)] = project.operations.as_slice() else {
        panic!("one Photo V-Carve, not {:?}", project.operations);
    };
    assert_eq!(op.image, BitmapId::new("b1"));
    assert_eq!(op.tool, tool.id);
    assert_eq!(op.max_depth_mm, None);
    assert_eq!(op.start_depth_mm, 0.0);
    assert_eq!(op.line_angle_deg, 22.5);
    assert_eq!(op.line_spacing_fraction, 1.1);
    assert_eq!(op.strategy, PhotoStrategy::Raster);
    assert!(!op.invert);
    assert_eq!(op.sample_mm, 0.25);
    assert_eq!(op.stepover_retract_mm, 1.0);
    assert_eq!(op.safe_z_mm, 5.0);

    // The report names the values to set, each at its `null`.
    for value in ["thickness_mm", "max_depth_mm", "feed_mm_min", "rpm"] {
        assert!(
            report
                .lines()
                .any(|l| l.starts_with("gradient.dcv:") && l.contains(value)),
            "{value} in {report}"
        );
    }
    assert!(report.contains("64 × 32 px"), "{report}");

    // `--height 50` is the same file.
    let tall = run(
        &dir,
        &["init", "gradient.png", "--height", "50", "-o", "tall.dcv"],
    );
    assert_eq!(tall.status.code(), Some(0), "{}", stderr(&tall));
    assert_eq!(
        std::fs::read_to_string(dir.join("tall.dcv")).expect("tall written"),
        std::fs::read_to_string(dir.join("gradient.dcv")).expect("dcv written")
    );

    // Both flags, or neither: a usage error naming both, nothing written.
    for args in [
        &["init", "gradient.png", "-o", "x.dcv"][..],
        &[
            "init",
            "gradient.png",
            "--width",
            "100",
            "--height",
            "50",
            "-o",
            "x.dcv",
        ][..],
    ] {
        let out = run(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        let message = stderr(&out);
        assert!(
            message.contains("--width") && message.contains("--height"),
            "{args:?}: {message}"
        );
        assert!(!dir.join("x.dcv").exists(), "{args:?}");
    }

    // A drawing takes neither flag, and still writes no picture.
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let out = run(&dir, &["init", "coaster.svg", "--width", "100"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--width"), "{}", stderr(&out));
    let coaster = init_coaster(&dir);
    let text = std::fs::read_to_string(&coaster).expect("dcv written");
    assert!(text.contains("\"bitmaps\": []"), "{text}");
    assert!(
        !load_file(&coaster)
            .project
            .operations
            .iter()
            .any(|op| matches!(op, Operation::PhotoVCarve(_)))
    );
}

/// The `image list` row of bitmap `id`, as whitespace-separated cells.
fn list_row(listing: &str, id: &str) -> Vec<String> {
    listing
        .lines()
        .map(|l| l.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .find(|cells| cells.first().is_some_and(|c| c == id))
        .unwrap_or_else(|| panic!("no row {id} in {listing}"))
}

/// Acceptance 58: `image add plaque.dcv gradient.png --width 60 --at
/// 20,30` adds a bitmap whose footprint is 20..80 × 30..60 mm, leaving
/// the plaque's own content alone; `image list` shows it, with `-` for
/// the operations that carve it. A second picture is `b2`, named by
/// `--name`.
#[test]
fn image_add_places_a_picture_and_image_list_shows_it() {
    let dir = work_dir("image_add_places_a_picture_and_image_list_shows_it");
    let dcv = copy_fixture(&dir, "plaque.dcv", "plaque.dcv");
    copy_photo(&dir, "gradient.png", "gradient.png");
    let before = load_file(&dcv).project;
    let out = run(
        &dir,
        &[
            "image",
            "add",
            "plaque.dcv",
            "gradient.png",
            "--width",
            "60",
            "--at",
            "20,30",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(said.contains("b1"), "{said}");
    assert!(said.contains("64 × 32 px"), "{said}");
    assert!(said.contains("60.000 × 30.000 mm"), "{said}");
    assert_eq!(stderr(&out), "", "no warning for a gray PNG");

    let after = load_file(&dcv).project;
    let [picture] = after.bitmaps.as_slice() else {
        panic!("one bitmap, not {:?}", after.bitmaps);
    };
    assert_eq!(picture.id, BitmapId::new("b1"));
    assert_eq!(picture.name, "gradient");
    assert_eq!(
        (
            picture.x_mm,
            picture.y_mm,
            picture.x_mm + picture.width_mm,
            picture.y_mm + picture.height_mm
        ),
        (20.0, 30.0, 80.0, 60.0)
    );
    assert_eq!(after.definitions, before.definitions);
    assert_eq!(after.instances, before.instances);
    assert_eq!(after.operations.len(), before.operations.len());
    assert_eq!(after.tools, before.tools);

    let out = run(&dir, &["image", "list", "plaque.dcv"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let listing = stdout(&out);
    let header: Vec<&str> = listing
        .lines()
        .next()
        .expect("a header")
        .split_whitespace()
        .collect();
    assert_eq!(
        header,
        ["ID", "NAME", "PX", "FOOTPRINT_MM", "AT_MM", "USED_BY"]
    );
    assert_eq!(
        list_row(&listing, "b1"),
        ["b1", "gradient", "64x32", "60.000x30.000", "20.000,30.000", "-"]
    );

    // The default corner is the origin; --name names it.
    let out = run(
        &dir,
        &[
            "image",
            "add",
            "plaque.dcv",
            "gradient.png",
            "--height",
            "10",
            "--name",
            "logo",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = run(&dir, &["image", "list", "plaque.dcv"]);
    assert_eq!(
        list_row(&stdout(&out), "b2"),
        ["b2", "logo", "64x32", "20.000x10.000", "0.000,0.000", "-"]
    );

    // A Photo V-Carve naming a bitmap is listed under it.
    let mut project = load_file(&dcv).project;
    let mut photo = dc_project::test_support::sample_photo_v_carve();
    photo.image = BitmapId::new("b2");
    photo.tool = project.tools[0].id.clone();
    project.operations.push(Operation::PhotoVCarve(photo));
    std::fs::write(&dcv, save(&project).expect("saves")).expect("written");
    let out = run(&dir, &["image", "list", "plaque.dcv"]);
    assert_eq!(list_row(&stdout(&out), "b2")[5], "o8");
    assert_eq!(list_row(&stdout(&out), "b1")[5], "-");

    // No size, or both: clap's usage error, naming both flags.
    for args in [
        &["image", "add", "plaque.dcv", "gradient.png"][..],
        &[
            "image",
            "add",
            "plaque.dcv",
            "gradient.png",
            "--width",
            "1",
            "--height",
            "1",
        ][..],
    ] {
        let out = run(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let message = stderr(&out);
        assert!(
            message.contains("--width") && message.contains("--height"),
            "{args:?}: {message}"
        );
    }
    assert_eq!(load_file(&dcv).project.bitmaps.len(), 2);
}

/// Acceptance 58: a colour JPEG adds with `image_colour_converted`
/// printed. A picture whose extension lies, or a format DreamCarve does
/// not read, adds nothing.
#[test]
fn image_add_converts_colour_and_says_so() {
    let dir = work_dir("image_add_converts_colour_and_says_so");
    let dcv = copy_fixture(&dir, "plaque.dcv", "plaque.dcv");
    copy_photo(&dir, "colour.jpg", "colour.jpg");
    let out = run(
        &dir,
        &["image", "add", "plaque.dcv", "colour.jpg", "--width", "16"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let warnings = stderr(&out);
    assert!(
        warnings.contains("warning: image_colour_converted: "),
        "{warnings}"
    );
    let project = load_file(&dcv).project;
    assert_eq!(project.bitmaps.len(), 1);
    assert_eq!(project.bitmaps[0].name, "colour");
    let header = project.bitmaps[0].header().expect("header reads");
    assert!(header.is_grayscale_8());

    // A PNG named .jpg, and a picture DreamCarve does not read.
    copy_photo(&dir, "gradient.png", "gradient.jpg");
    let out = run(
        &dir,
        &["image", "add", "plaque.dcv", "gradient.jpg", "--width", "16"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("named as a JPEG picture but holds a PNG"),
        "{}",
        stderr(&out)
    );
    copy_photo(&dir, "gradient.png", "gradient.gif");
    let out = run(
        &dir,
        &["image", "add", "plaque.dcv", "gradient.gif", "--width", "16"],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("PNG or JPEG"), "{}", stderr(&out));
    assert_eq!(load_file(&dcv).project.bitmaps.len(), 1);
}

/// The `<image>` half of the preview test, which needs no generator: a
/// picture added to the plaque is drawn at its footprint, Y flipped,
/// before the plaque's toolpaths, from a data URI of the pixels
/// re-encoded (security review L5).
#[test]
fn preview_draws_the_picture_of_image_add() {
    let dir = work_dir("preview_draws_the_picture_of_image_add");
    let dcv = copy_fixture(&dir, "plaque.dcv", "plaque.dcv");
    copy_photo(&dir, "gradient.png", "gradient.png");
    let out = run(
        &dir,
        &[
            "image",
            "add",
            "plaque.dcv",
            "gradient.png",
            "--width",
            "60",
            "--at",
            "20,30",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = run(&dir, &["check", "plaque.dcv", "--preview", "preview.svg"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let svg = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");
    let project = load_file(&dcv).project;
    let images = svg_elements(&svg, "picture");
    assert_eq!(images.len(), 1, "{svg}");
    let image = images[0];
    assert_eq!(svg_attr(image, "x"), "20.000");
    assert_eq!(
        svg_attr(image, "y"),
        format!("{:.3}", project.stock.height_mm - 60.0)
    );
    assert_eq!(svg_attr(image, "width"), "60.000");
    assert_eq!(svg_attr(image, "height"), "30.000");
    assert_eq!(svg_attr(image, "preserveAspectRatio"), "none");
    let pixels = project.bitmaps[0].pixels().expect("pixels read");
    let uri = format!(
        "data:image/png;base64,{}",
        dc_import::bitmap::stored(&pixels).expect("encodes")
    );
    assert_eq!(svg_attr(image, "href"), uri);
    let at = svg.find("<image").expect("an image");
    let first_run = svg.find(" class=\"operation").expect("a toolpath");
    assert!(at < first_run, "the picture is under the toolpaths");
}

/// The design spec's `cli::preview_draws_the_bitmap_under_the_grooves`:
/// `init` on the gradient, set up, exported with `--preview`: the picture
/// is drawn at its footprint before the Photo V-Carve's grooves.
#[test]
#[ignore = "needs #646"]
fn preview_draws_the_bitmap_under_the_grooves() {
    let dir = work_dir("preview_draws_the_bitmap_under_the_grooves");
    copy_photo(&dir, "gradient.png", "gradient.png");
    let out = run(&dir, &["init", "gradient.png", "--width", "100"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let dcv = dir.join("gradient.dcv");
    set_photo_values(&dcv);
    let out = run(
        &dir,
        &[
            "export",
            "gradient.dcv",
            "--preview",
            "preview.svg",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let svg = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");
    let project = load_file(&dcv).project;
    let images = svg_elements(&svg, "picture");
    assert_eq!(images.len(), 1, "{svg}");
    let image = images[0];
    // The stock is 120 × 70 mm; the footprint 100 × 50 mm at (10, 10).
    assert_eq!(svg_attr(image, "x"), "10.000");
    assert_eq!(svg_attr(image, "y"), "10.000");
    assert_eq!(svg_attr(image, "width"), "100.000");
    assert_eq!(svg_attr(image, "height"), "50.000");
    assert_eq!(svg_attr(image, "preserveAspectRatio"), "none");
    let pixels = project.bitmaps[0].pixels().expect("pixels read");
    let uri = format!(
        "data:image/png;base64,{}",
        dc_import::bitmap::stored(&pixels).expect("encodes")
    );
    assert_eq!(svg_attr(image, "href"), uri);
    // The grooves are drawn, after the picture.
    let grooves = svg.find("<g id=\"op1\"").expect("the Photo V-Carve's run");
    assert!(svg.find("<image").expect("an image") < grooves);
    assert!(svg[grooves..].contains("<path"), "{svg}");
}

/// Acceptance 58 (capability 46, the project embeds its pictures):
/// copying only the `.dcv` elsewhere exports the same program.
#[test]
#[ignore = "needs #646"]
fn moved_dcv_exports_the_same_program() {
    let dir = work_dir("moved_dcv_exports_the_same_program");
    copy_photo(&dir, "gradient.png", "gradient.png");
    let out = run(&dir, &["init", "gradient.png", "--width", "100"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    set_photo_values(&dir.join("gradient.dcv"));
    let out = run(&dir, &["export", "gradient.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let programs = programs_in(&dir);
    assert!(!programs.is_empty());

    let moved = dir.join("elsewhere");
    std::fs::create_dir_all(&moved).expect("dir made");
    std::fs::copy(dir.join("gradient.dcv"), moved.join("gradient.dcv")).expect("copied");
    std::fs::remove_file(dir.join("gradient.png")).expect("picture removed");
    let out = run(&moved, &["export", "gradient.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(programs_in(&moved), programs);
    for name in &programs {
        let here = std::fs::read(dir.join(name)).expect("program");
        let there = std::fs::read(moved.join(name)).expect("program");
        assert!(here == there, "{name} differs");
    }
}
