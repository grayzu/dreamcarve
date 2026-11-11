// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 34 of phase 4's review redesign (`docs/specs/phase-4/design.md`,
//! "Review redesign slices"; #626): the empty state's example cards show
//! a thumbnail drawn from the example itself (functional spec, capability
//! 79), never a stored image. The examples are read from where they live,
//! as the app embeds them: `fixtures/coaster.svg` made a project the way
//! `dreamcarve init` makes it, `tests/fixtures/plaque.dcv` and
//! `tests/fixtures/sign.dcv`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::onboarding::{
    self, THUMBNAIL_HEIGHT_PX, THUMBNAIL_INSET_PX, THUMBNAIL_WIDTH_PX, Thumbnail,
};
use dc_import::SvgImportOptions;
use dc_job::Drawing;
use dc_project::Project;

use common::{fixture, load_dcv, read, repo_root};

/// How far a thumbnail's box may sit from the exact fit, px: the arcs are
/// drawn as chords, which can fall this far short of the arc's extreme.
const FIT_PX: f32 = 0.25;

/// The coaster as the app's example makes it: the drawing, with the stock
/// round it.
fn coaster() -> Project {
    let bytes = read(&repo_root().join("fixtures/coaster.svg"));
    let (import, _warnings) =
        dc_import::import_svg(&bytes, &SvgImportOptions::default()).expect("the coaster imports");
    let post = dc_project::latest_definition("grblhal")
        .expect("grblHAL is built in")
        .default_ref();
    dc_job::project_from::drawing(
        "coaster",
        Drawing::Svg(import),
        post,
        dc_project::Units::Mm,
        true,
    )
    .expect("the coaster has shapes")
}

/// The three examples of the empty state.
fn examples() -> Vec<(&'static str, Project)> {
    vec![
        ("coaster", coaster()),
        ("plaque", load_dcv(&fixture("plaque.dcv"))),
        ("sign", load_dcv(&fixture("sign.dcv"))),
    ]
}

/// The number of outlines `project`'s visible instances draw: one per
/// profile of each one's definition.
fn visible_outlines(project: &Project) -> usize {
    project
        .instances
        .iter()
        .filter(|i| i.visible)
        .map(|i| project.definitions[&i.definition].profiles.len())
        .sum()
}

/// The box of every point of `thumbnail`, px: left, top, right, bottom.
fn drawn_box(thumbnail: &Thumbnail) -> (f32, f32, f32, f32) {
    let points = thumbnail.outlines.iter().flat_map(|o| o.points_px.iter());
    points.fold(
        (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ),
        |(l, t, r, b), p| (l.min(p[0]), t.min(p[1]), r.max(p[0]), b.max(p[1])),
    )
}

/// Asserts `thumbnail` is `project`'s visible outlines fitted to the
/// 96 × 56 thumbnail with its 4-px inset.
fn assert_fits(name: &str, project: &Project, thumbnail: &Thumbnail) {
    assert_eq!(
        thumbnail.outlines.len(),
        visible_outlines(project),
        "{name}: one outline per visible profile"
    );
    for outline in &thumbnail.outlines {
        assert!(outline.points_px.len() >= 2, "{name}: an outline is a line");
    }

    let harness = dc_app::test_support::Harness::from_project(project.clone());
    let bounds = harness
        .app
        .scene()
        .shapes_bounds()
        .expect("the example has shapes");
    let (left, top, right, bottom) = drawn_box(thumbnail);
    let (width, height) = (right - left, bottom - top);

    // Same bounds ratio as the shapes themselves.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "millimetres of a small drawing, compared in px"
    )]
    let (shapes_w, shapes_h) = (bounds.width_mm() as f32, bounds.height_mm() as f32);
    let scale = width.max(height) / shapes_w.max(shapes_h);
    assert!(
        (width - shapes_w * scale).abs() <= FIT_PX && (height - shapes_h * scale).abs() <= FIT_PX,
        "{name}: {width} × {height} px is not {shapes_w} × {shapes_h} mm scaled"
    );

    // Inside the inset, touching it on the tighter axis, centred on both.
    let room_w = THUMBNAIL_WIDTH_PX - 2.0 * THUMBNAIL_INSET_PX;
    let room_h = THUMBNAIL_HEIGHT_PX - 2.0 * THUMBNAIL_INSET_PX;
    assert!(
        left >= THUMBNAIL_INSET_PX - FIT_PX
            && top >= THUMBNAIL_INSET_PX - FIT_PX
            && right <= THUMBNAIL_WIDTH_PX - THUMBNAIL_INSET_PX + FIT_PX
            && bottom <= THUMBNAIL_HEIGHT_PX - THUMBNAIL_INSET_PX + FIT_PX,
        "{name}: ({left}, {top})–({right}, {bottom}) leaves the inset"
    );
    let fill = (width / room_w).max(height / room_h);
    assert!(
        (fill - 1.0).abs() <= FIT_PX / room_h,
        "{name}: the shapes fill {fill} of the room"
    );
    assert!(
        ((left + right) / 2.0 - THUMBNAIL_WIDTH_PX / 2.0).abs() <= FIT_PX
            && ((top + bottom) / 2.0 - THUMBNAIL_HEIGHT_PX / 2.0).abs() <= FIT_PX,
        "{name}: centred"
    );
}

/// Each thumbnail's outlines are the example's visible instances'
/// outlines fitted to 96 × 56 with a 4-px inset: the same count, the same
/// bounds ratio.
#[test]
fn example_thumbnails_draw_the_examples_shapes() {
    assert_eq!(
        (THUMBNAIL_WIDTH_PX, THUMBNAIL_HEIGHT_PX, THUMBNAIL_INSET_PX),
        (96.0, 56.0, 4.0)
    );
    for (name, project) in examples() {
        assert!(visible_outlines(&project) > 0, "{name} has shapes");
        let thumbnail = onboarding::thumbnail(&project);
        assert_fits(name, &project, &thumbnail);
    }
}

/// The thumbnail is the drawing as the Viewport shows it: a hidden
/// instance is left out and the rest are fitted again; Y is up in the
/// project and down on screen, so the shape nearest the stock's bottom is
/// the lowest on the thumbnail.
#[test]
fn thumbnail_follows_visibility_and_flips_y() {
    let mut project = load_dcv(&fixture("plaque.dcv"));
    let all = onboarding::thumbnail(&project);

    // The plaque's outline spans the drawing, so its bottom edge is the
    // thumbnail's lowest line: the largest y on a Y-down screen.
    let harness = dc_app::test_support::Harness::from_project(project.clone());
    let scene = harness.app.scene();
    let bounds = scene.shapes_bounds().expect("shapes");
    let (lowest_index, _) = scene
        .shapes()
        .iter()
        .flat_map(|s| s.outlines.iter())
        .enumerate()
        .filter_map(|(i, o)| Some((i, o.bounds()?.min.y)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("an outline");
    let (_, _, _, bottom) = drawn_box(&all);
    let lowest = all.outlines[lowest_index]
        .points_px
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (lowest - bottom).abs() <= FIT_PX,
        "the outline at y {} mm is drawn at the bottom",
        bounds.min.y
    );

    project.instances[0].visible = false;
    let fewer = onboarding::thumbnail(&project);
    assert_fits("plaque, one shape hidden", &project, &fewer);
    assert!(fewer.outlines.len() < all.outlines.len());

    // Nothing visible: nothing drawn, and nothing to divide by.
    for instance in &mut project.instances {
        instance.visible = false;
    }
    assert!(onboarding::thumbnail(&project).outlines.is_empty());
}
