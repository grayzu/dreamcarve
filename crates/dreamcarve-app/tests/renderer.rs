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

/// `experience.md` checklist item 15: "a fit view and a 4× zoom show the
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
    let edge = Point::new(85.0, 40.0);

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
