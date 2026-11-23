// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's wgpu line renderer (`docs/specs/phase-4/design.md`,
//! "Architecture → The Viewport renderer"; slice 4, #533), rendered
//! through egui's kittest harness on the machine's wgpu adapter. Skipped,
//! with the reason printed, where there is none.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeMap;

use common::{QueuedDialogs, fixture, in_pane, on_screen, options, scratch, wgpu_harness};
use dc_geometry::{Point, Polyline, Transform};
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId};
use dreamcarve_app::{Shell, theme};
use eframe::egui;
use egui_kittest::Harness;

/// How many pixels of `colour` the column at window point `at` holds
/// within 8 px of it, over `background`: the stroke's width there,
/// anti-aliased fringe included, in physical pixels.
fn stroke_width_px(
    pixel: &dyn Fn(u32, u32) -> [u8; 4],
    at: egui::Pos2,
    colour: egui::Color32,
    background: egui::Color32,
) -> f64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a pixel inside a 1280 × 800 image"
    )]
    let (x, y) = (at.x.round() as u32, at.y.round() as u32);
    let mut width = 0.0;
    for row in y - 8..=y + 8 {
        let pixel = pixel(x, row);
        let mut coverage = 0.0;
        let mut channels = 0.0;
        let channels_of = colour
            .to_array()
            .into_iter()
            .zip(background.to_array())
            .zip(pixel)
            .take(3);
        for ((fg, bg), got) in channels_of {
            let (fg, bg) = (f64::from(fg), f64::from(bg));
            if (fg - bg).abs() > 40.0 {
                coverage += (f64::from(got) - bg) / (fg - bg);
                channels += 1.0;
            }
        }
        width += coverage / channels;
    }
    width
}

/// `experience.md` checklist item 85: "a fit view and a 4× zoom show the
/// same stroke widths". One 100 × 40 mm rectangle on the stock, the grid
/// off: its bottom edge measures 1.25 px (the shape's weight, section 9)
/// at fit and after a 4× zoom about it, and 2 px selected, at both.
/// Widths live in the style table and the shader expands each segment to
/// a quad of that many pixels, so the zoom changes only the camera.
#[test]
fn segment_quad_width_is_screen_constant() {
    let dir = scratch("renderer-width");
    let bytes = std::fs::read(fixture("plaque.dcv")).unwrap();
    let mut project = dc_project::load(&bytes).unwrap().project;
    project.operations.clear();
    project.tabs.clear();
    let definition = DefinitionId::new("d1");
    project.definitions = BTreeMap::from([(
        definition.clone(),
        FeatureDefinition {
            id: definition.clone(),
            name: "panel".to_owned(),
            shape: None,
            profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 100.0, 40.0)],
        },
    )]);
    project.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition,
        transform: Transform::translate(35.0, 40.0),
        name: "panel".to_owned(),
        visible: true,
        locked: false,
        construction: false,
        folder: None,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    let path = dir.join("panel.dcv");
    std::fs::write(&path, dc_project::save(&project).unwrap()).unwrap();

    let dialogs = QueuedDialogs::default();
    let Some(mut h) = wgpu_harness(
        "segment_quad_width_is_screen_constant",
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        800.0,
    ) else {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    };
    h.state_mut().app_mut().view_mut().visibility.grid = false;
    h.run();
    let palette = theme::Palette::of(h.ctx.theme());
    // A quarter of the way along the bottom edge: clear of the corners'
    // handles and of the one at the side's middle.
    let edge = Point::new(60.0, 40.0);

    let measure = |h: &mut Harness<'_, Shell>, colour: egui::Color32| {
        let at = on_screen(h, edge);
        let image = h.render().expect("a frame renders");
        stroke_width_px(
            &|x, y| image.get_pixel(x, y).0,
            at,
            colour,
            palette.viewport_stock_fill,
        )
    };
    let fit = measure(&mut h, palette.viewport_shape);
    let at = on_screen(&h, edge);
    let pane_at = in_pane(&h, at);
    h.state_mut()
        .app_mut()
        .view_mut()
        .camera
        .zoom_about(pane_at, 4.0);
    h.run();
    let zoomed = measure(&mut h, palette.viewport_shape);
    assert!(
        (fit - 1.25).abs() < 0.3 && (zoomed - 1.25).abs() < 0.3,
        "a shape is 1.25 px wide: {fit:.3} at fit, {zoomed:.3} at 4×"
    );
    assert!(
        (fit - zoomed).abs() < 0.1,
        "the width does not change with zoom: {fit:.3} at fit, {zoomed:.3} at 4×"
    );

    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![InstanceId::new("i1")]));
    h.run();
    let selected_zoomed = measure(&mut h, palette.viewport_shape_selected);
    h.key_press(egui::Key::F);
    h.run();
    let selected_fit = measure(&mut h, palette.viewport_shape_selected);
    assert!(
        (selected_fit - 2.0).abs() < 0.3 && (selected_zoomed - 2.0).abs() < 0.3,
        "a selected shape is 2 px wide: {selected_fit:.3} at fit, {selected_zoomed:.3} at 4×"
    );
    assert!(
        (selected_fit - selected_zoomed).abs() < 0.1,
        "the selection's width does not change with zoom: {selected_fit:.3}, {selected_zoomed:.3}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// How many of `pixels` are within `tolerance` of `colour` in every
/// channel.
fn pixels_near(
    pixels: impl Iterator<Item = [u8; 4]>,
    colour: egui::Color32,
    tolerance: u8,
) -> usize {
    let want = colour.to_array();
    pixels
        .filter(|p| (0..3).all(|k| p[k].abs_diff(want[k]) <= tolerance))
        .count()
}

/// Capabilities 72 and 73 (#545), rendered: on the generated plaque the
/// Viewport draws each operation's cuts in its colour of the theme's
/// operation set, the preview's order; *By depth* recolours the cuts from
/// the ramp, its deepest stop where the cut-out and the bores go through,
/// with no buffer rebuilt (the colour mode is the camera uniform's).
#[test]
fn toolpaths_draw_in_operation_colours_and_by_depth() {
    let dir = scratch("renderer-toolpaths");
    let plaque = common::copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = wgpu_harness(
        "toolpaths_draw_in_operation_colours_and_by_depth",
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    ) else {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.state_mut().app_mut().view_mut().visibility.grid = false;
    h.run();
    let palette = theme::Palette::of(h.ctx.theme());
    let image = h.render().expect("a frame renders");
    let runs = h.state().app().scene().operations().len();
    for k in 0..runs {
        let found = pixels_near(image.pixels().map(|p| p.0), palette.operation(k), 6);
        assert!(found > 20, "run {k}'s colour: {found} pixels");
    }
    let deepest = *palette.depth_ramp.last().unwrap();
    assert!(
        pixels_near(image.pixels().map(|p| p.0), deepest, 6) < 20,
        "by operation, no cut takes the ramp's colour"
    );

    h.state_mut()
        .app_mut()
        .set_colour(dc_app::ColourMode::ByDepth);
    h.run();
    let image = h.render().expect("a frame renders");
    let found = pixels_near(image.pixels().map(|p| p.0), deepest, 6);
    assert!(
        found > 50,
        "the through cuts take the deepest stop: {found}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 87's clause "the stale toolpaths show faded", rendered. On
/// the generated plaque each operation's cuts paint in its colour. After
/// an edit every generator reads (the stock widened), with generation
/// stopped so nothing replaces them, the operations are stale and keep
/// their runs: the Viewport holds under a tenth of the pixels it held in
/// the runs' full colours, yet the toolpaths are still there, since the
/// frame differs from the one drawn with every toolpath layer off. Like
/// every rendering test it renders nothing without a wgpu adapter;
/// `layers::tests::a_stale_operations_toolpath_is_drawn_faded` asserts
/// the draw list everywhere.
#[test]
fn stale_toolpaths_render_faded() {
    let dir = scratch("renderer-stale");
    let plaque = common::copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = wgpu_harness(
        "stale_toolpaths_render_faded",
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    ) else {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while h.state().app().generation().busy() || h.state().worker_busy() {
        assert!(std::time::Instant::now() < deadline, "the plaque generates");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.state_mut().app_mut().view_mut().visibility.grid = false;
    h.run();
    let palette = theme::Palette::of(h.ctx.theme());
    let runs = h.state().app().scene().operations().len();
    // Counted on the Viewport alone: the Operations list and the Layers
    // popover draw each operation's colour on their own swatches.
    let pane = h.state().layout().viewport;
    #[allow(clippy::cast_precision_loss, reason = "a pixel of a 1280 × 800 image")]
    let on_viewport =
        move |x: u32, y: u32| pane.shrink(2.0).contains(egui::pos2(x as f32, y as f32));
    let in_colours = |h: &mut Harness<'static, Shell>| -> Vec<usize> {
        let image = h.render().expect("a frame renders");
        (0..runs)
            .map(|k| {
                let pixels = image
                    .enumerate_pixels()
                    .filter(|(x, y, _)| on_viewport(*x, *y))
                    .map(|(_, _, p)| p.0);
                pixels_near(pixels, palette.operation(k), 6)
            })
            .collect()
    };
    let fresh = in_colours(&mut h);

    // Stale, and kept so: the edit, then Stop before a frame hands the
    // worker anything.
    let mut stock = h.state().app().project().stock.clone();
    stock.width_mm += 5.0;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::SetStock(stock))
        .expect("the stock widens");
    h.state_mut().app_mut().stop();
    h.run();
    let operations = h.state().app().scene().operations();
    assert_eq!(operations.len(), runs, "the old drawings stay");
    assert!(
        operations.iter().all(|op| op.stale && !op.runs.is_empty()),
        "every operation is stale and keeps its runs"
    );
    let stale = in_colours(&mut h);
    let faded = h.render().expect("a frame renders");
    let mut visibility = h.state().app().view().visibility;
    visibility.cuts = false;
    visibility.rapids = false;
    visibility.plunges = false;
    visibility.arrows = false;
    visibility.start_points = false;
    visibility.tab_marks = false;
    h.state_mut().app_mut().set_visibility(visibility);
    h.run();
    // Counted over what is there with no toolpath drawn, and summed over
    // the runs: one run's colour at 35 % over the stock can come within
    // the tolerance of another's at full strength, so a single colour's
    // count need not fall, where the sum does, tenfold.
    let none = in_colours(&mut h);
    for k in 0..runs {
        assert!(
            fresh[k] > none[k] + 20,
            "run {k} draws in its colour when current: {fresh:?} over {none:?}"
        );
    }
    let over = |counts: &[usize]| -> usize {
        counts
            .iter()
            .zip(&none)
            .map(|(count, base)| count.saturating_sub(*base))
            .sum()
    };
    let (current, waiting) = (over(&fresh), over(&stale));
    assert!(
        waiting * 10 < current,
        "the stale toolpaths are faded: {waiting} pixels in a run's full colour ({stale:?}), \
         {current} when current ({fresh:?})"
    );

    // Faded, not gone: with the toolpath layers off the frame is another.
    let hidden = h.render().expect("a frame renders");
    let differing = faded
        .pixels()
        .zip(hidden.pixels())
        .filter(|(a, b)| (0..3).any(|c| a.0[c].abs_diff(b.0[c]) > 6))
        .count();
    assert!(
        differing > 200,
        "the stale toolpaths are still drawn: {differing} pixels differ from none drawn"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 56, rendered (slice D6): a picture draws at its footprint
/// the right way up and unmirrored, in its own grays. The picture is
/// four flat quarters, each a gray of its own (top-left 240, top-right
/// 170, bottom-left 100, bottom-right 30), so a flip or a mirror moves a
/// gray to the wrong quarter. In a Y-up project the picture's top row
/// lies along the footprint's greatest y. With a shape selected the
/// picture draws at 60 % over the stock (`experience.md`, section 5,
/// `viewport.picture`); deselected it is back at full strength. Moving it
/// moves the quad without another texture.
#[test]
fn picture_draws_the_right_way_up_under_the_shapes() {
    let dir = scratch("renderer-picture");
    let plaque = common::copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = wgpu_harness(
        "picture_draws_the_right_way_up_under_the_shapes",
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    ) else {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    };
    // Sixteen samples a side, so the middle of a quarter is far from the
    // next gray whatever the filtering.
    let side = 16_u32;
    let grays = [[240_u8, 170], [100, 30]];
    let mut bytes = Vec::new();
    for row in 0..side {
        for column in 0..side {
            bytes.push(grays[usize::from(row >= side / 2)][usize::from(column >= side / 2)]);
        }
    }
    // The toolpaths and the grid would lie over the quarters' middles.
    for op in h.state().app().project().operations.clone() {
        h.state_mut().app_mut().toggle_hidden(op.id());
    }
    h.state_mut().app_mut().view_mut().visibility.grid = false;
    // 40 × 40 mm with its corner at (100, 60): clear of every shape's
    // outline at the four points sampled.
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::ImportPicture {
            name: "quarters.png".to_owned(),
            pixels: std::sync::Arc::new(dc_project::Grayscale {
                width_px: side,
                height_px: side,
                bytes,
            }),
            size: dc_job::picture::PictureSize::Width { width_mm: 40.0 },
        })
        .expect("the picture imports");
    let id = dc_project::BitmapId::new("b1");
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::MoveBitmaps {
            ids: vec![id.clone()],
            dx_mm: 124.0,
            dy_mm: 74.0,
        })
        .expect("the picture moves");
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.run();

    let gray_at = |h: &mut Harness<'_, Shell>, p: Point| {
        let at = on_screen(h, p);
        let image = h.render().expect("a frame renders");
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a pixel inside a 1280 × 800 image"
        )]
        let pixel = image.get_pixel(at.x.round() as u32, at.y.round() as u32).0;
        [pixel[0], pixel[1], pixel[2]]
    };
    let is_gray = |pixel: [u8; 3], want: u8| pixel.iter().all(|c| c.abs_diff(want) <= 3);
    // The quarters' middles, in project space: the footprint is x 124 to
    // 164, y 74 to 114.
    let (left, right, low, high) = (139.0, 154.0, 84.0, 106.0);
    let quarters = [
        ("top-left", Point::new(left, high), 240_u8),
        ("top-right", Point::new(right, high), 170),
        ("bottom-left", Point::new(left, low), 100),
        ("bottom-right", Point::new(right, low), 30),
    ];
    for (name, at, want) in quarters {
        let got = gray_at(&mut h, at);
        assert!(
            is_gray(got, want),
            "the picture's {name} quarter is at the footprint's {name}: {got:?}, not {want}"
        );
    }

    // A shape selected: the picture at 60 % over the stock's fill.
    let palette = theme::Palette::of(h.ctx.theme());
    let stock = f32::from(palette.viewport_stock_fill.r());
    h.state_mut()
        .app_mut()
        .select(dc_app::Selection::Instances(vec![InstanceId::new("i1")]));
    h.run();
    let [dimmed, ..] = gray_at(&mut h, Point::new(left, high));
    // Blended in the framebuffer's own space, so between the two grays
    // and well off both.
    assert!(
        f32::from(dimmed) < 240.0 - 20.0 && f32::from(dimmed) > stock + 20.0,
        "dimmed while a shape is selected: {dimmed} between {stock} and 240"
    );
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.run();
    assert!(is_gray(gray_at(&mut h, Point::new(left, high)), 240));

    // Moved: the same picture at its new footprint, the old one clear.
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::MoveBitmaps {
            ids: vec![id],
            dx_mm: -60.0,
            dy_mm: 0.0,
        })
        .expect("the picture moves");
    h.run();
    assert!(
        is_gray(gray_at(&mut h, Point::new(left - 60.0, high)), 240),
        "the picture is where it was moved to"
    );
    assert!(
        !is_gray(gray_at(&mut h, Point::new(left, high)), 240),
        "and no longer where it was"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
