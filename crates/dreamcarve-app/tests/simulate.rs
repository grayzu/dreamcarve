// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Simulate step in the window (phase 6 functional spec, acceptance
//! 155 and 157; `docs/specs/phase-6/design.md`, slices 7 to 9 and 12):
//! through the window's accessibility tree, that the step lists the
//! operations in cut order, that Play advances the time and Pause holds
//! it, that a row's click shows the stock as its operation leaves it and
//! dims the rows after, that Quality, Colour and Layers are preferences,
//! that the male board is offered only with an Inlay, and that a line of
//! uncut material frames its places; and, rendered through wgpu where
//! the machine has an adapter, that a frame of the plaque is not blank
//! and differs between the start and the end of the job. The keys are
//! `kittest::keyboard_shortcuts_reach_their_commands`'s.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::Path;

use common::{
    QueuedDialogs, command_key, copy_fixture, fixture, go_to, harness, options, scratch, settle,
    wgpu_harness,
};
use dc_app::sim::{Colouring, View3};
use dc_app::steps::{State, Step};
use dc_job::sim::Quality;
use dc_project::{Board, CornerRelief, Operation};
use dreamcarve_app::{Shell, theme};
use eframe::egui;
use eframe::egui::accesskit::{Role, Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

type H = Harness<'static, Shell>;

/// The project at `path` open on the Simulate step in a window `height`
/// px tall, simulated.
fn simulated_at(path: std::path::PathBuf, dir: &Path, height: f32) -> H {
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        height,
    );
    settle(&mut h);
    go_to(&mut h, "Simulate");
    settle(&mut h);
    h
}

/// Fixture `name` open on the Simulate step, simulated.
fn simulated(name: &str, dir: &Path) -> H {
    simulated_at(copy_fixture(name, dir), dir, 800.0)
}

/// The panel's rows, top to bottom: each operation's name, and whether
/// the playback has reached it.
fn rows(h: &H) -> Vec<(String, bool)> {
    let mut rows: Vec<(String, bool, f32)> = h
        .query_all_by_role(Role::ListItem)
        .map(|node| {
            (
                node.accesskit_node().label().unwrap_or_default(),
                node.accesskit_node().toggled() == Some(Toggled::True),
                node.rect().min.y,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.2.total_cmp(&b.2));
    rows.into_iter().map(|(name, on, _)| (name, on)).collect()
}

/// The time the playback shows, seconds.
fn time(h: &H) -> f64 {
    h.state().sim_time_s().0
}

/// Acceptance 155 and 157: the step lists the enabled operations in cut
/// order, every one reached, since it opens on the finished stock; its
/// summary is the count and the time; it is complete once built; and it
/// has no Inspector.
#[test]
fn simulate_lists_the_operations_in_cut_order() {
    let dir = scratch("simulate-rows");
    let h = simulated("plaque.dcv", &dir);
    let names: Vec<String> = h
        .state()
        .app()
        .project()
        .operations
        .iter()
        .filter(|op| op.enabled())
        .map(|op| op.name().to_owned())
        .collect();
    assert_eq!(names.len(), 5);
    let listed = rows(&h);
    assert_eq!(
        listed.iter().map(|row| row.0.clone()).collect::<Vec<_>>(),
        names
    );
    assert!(listed.iter().all(|row| row.1), "{listed:?}");
    assert!(
        h.query_by_label_contains("5 operations · ").is_some(),
        "the summary is the count and the time"
    );
    assert_eq!(h.state().step(), Step::Simulate);
    assert_eq!(h.state().step_states().of(Step::Simulate), State::Complete);
    assert!(h.state().layout().inspector.is_none(), "no Inspector");
    let (at, total) = h.state().sim_time_s();
    assert!(total > 60.0 && (at - total).abs() < 1e-6, "{at} of {total}");
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 113: Play starts from uncut stock when the job is at its
/// end and the time then advances frame by frame; Pause holds it.
#[test]
fn play_advances_and_pause_holds() {
    let dir = scratch("simulate-play");
    let mut h = simulated("plaque.dcv", &dir);
    let total = h.state().sim_time_s().1;
    h.get_by_role_and_label(Role::Button, "Play").click();
    for _ in 0..4 {
        h.step();
    }
    assert!(h.state().sim_playing());
    let first = time(&h);
    assert!(
        first < total / 2.0,
        "it starts again from the start: {first}"
    );
    for _ in 0..6 {
        h.step();
    }
    let later = time(&h);
    assert!(later > first, "the time advances: {first} then {later}");
    assert!(
        rows(&h).iter().skip(1).all(|row| !row.1),
        "the operations not yet reached are dimmed"
    );

    h.get_by_role_and_label(Role::Button, "Pause").click();
    for _ in 0..3 {
        h.step();
    }
    assert!(!h.state().sim_playing());
    let held = time(&h);
    for _ in 0..6 {
        h.step();
    }
    assert!((time(&h) - held).abs() < 1e-9, "Pause holds the time");
    assert!(h.query_by_role_and_label(Role::Button, "Play").is_some());
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 114: a click on a row shows the stock as that operation
/// leaves it; the rows after it are dimmed; Next goes on from there.
#[test]
fn a_row_click_dims_the_rows_after_it() {
    let dir = scratch("simulate-row-click");
    let mut h = simulated("plaque.dcv", &dir);
    let total = h.state().sim_time_s().1;
    let second = rows(&h)[1].0.clone();
    h.get_by_role_and_label(Role::ListItem, &second).click();
    h.run();
    let reached: Vec<bool> = rows(&h).iter().map(|row| row.1).collect();
    assert_eq!(reached, [true, true, false, false, false]);
    let after_two = time(&h);
    assert!(after_two > 0.0 && after_two < total);

    h.get_by_role_and_label(Role::Button, "Next operation")
        .click();
    h.run();
    let reached: Vec<bool> = rows(&h).iter().map(|row| row.1).collect();
    assert_eq!(reached, [true, true, true, false, false]);
    assert!(time(&h) > after_two);
    h.get_by_role_and_label(Role::Button, "Previous operation")
        .click();
    h.run();
    assert!((time(&h) - after_two).abs() < 1e-6);
    std::fs::remove_dir_all(&dir).ok();
}

/// Capabilities 112, 116, 117 and 124: Quality makes the simulation
/// again on another grid, Colour and Layers change what is drawn, and
/// all three are the app's preferences, written to `gui.toml`.
#[test]
fn quality_colour_and_layers_are_preferences() {
    let dir = scratch("simulate-display");
    // Tall, so the Display section is in view under the rows.
    let mut h = simulated_at(copy_fixture("plaque.dcv", &dir), &dir, 1500.0);
    let grid = |h: &H| {
        let built = h.state().app().sim_built().expect("a simulation");
        built.finished.grid().nx().max(built.finished.grid().ny())
    };
    assert_eq!(grid(&h), 1024, "Standard until chosen");

    h.get_by_role_and_label(Role::RadioButton, "Draft").click();
    h.run();
    settle(&mut h);
    assert_eq!(h.state().app().prefs().sim.quality, Quality::Draft);
    assert_eq!(grid(&h), 512);
    assert!(
        h.query_by_label_contains("Cells of ").is_some(),
        "the cell size is shown under Quality"
    );

    // Colour is on the Viewport's pill and in the panel: either sets it.
    h.query_all_by_role_and_label(Role::RadioButton, "By depth")
        .next()
        .expect("Colour's choices")
        .click();
    h.run();
    assert_eq!(h.state().app().prefs().sim.colouring, Colouring::Depth);

    assert!(!h.state().app().prefs().sim.layers.rapids);
    h.get_by_role_and_label(Role::CheckBox, "Rapids").click();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Bed").click();
    h.run();
    let layers = h.state().app().prefs().sim.layers;
    assert!(layers.rapids && !layers.bed && layers.tool && layers.origin);

    let written = std::fs::read_to_string(dir.join("config").join("gui.toml")).unwrap();
    for line in [
        "[simulation]",
        "quality = \"draft\"",
        "colour = \"depth\"",
        "rapids = true",
        "bed = false",
    ] {
        assert!(written.contains(line), "{line} in:\n{written}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 118: the board choice is offered only with an Inlay, and
/// the male board is simulated by its own runs.
#[test]
fn the_male_board_is_offered_with_an_inlay() {
    let dir = scratch("simulate-male");
    let h = simulated("plaque.dcv", &dir);
    assert!(
        h.query_by_role_and_label(Role::RadioButton, "Male board")
            .is_none(),
        "no Inlay, no choice"
    );
    drop(h);

    let mut h = simulated("dc.dcv", &dir);
    assert_eq!(h.state().app().sim_board(), Board::Female);
    let stock = h.state().app().sim_built().unwrap().plan.clone();
    assert_eq!(stock.board, Board::Female);
    h.get_by_role_and_label(Role::RadioButton, "Male board")
        .click();
    h.run();
    settle(&mut h);
    assert_eq!(h.state().app().sim_board(), Board::Male);
    let male = h.state().app().sim_built().unwrap().plan.clone();
    assert_eq!(male.board, Board::Male);
    assert_ne!(male.runs.len(), 0);
    assert!(
        male.runs
            .iter()
            .all(|run| stock.runs.iter().all(|s| s.key != run.key)),
        "each board is cut by its own runs"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 120 (acceptance 147 in the window): uncut material is off
/// until turned on; on, the plaque's square recess with its corner
/// relief off lists its four corners, and a click on the line frames
/// them from the Top.
#[test]
fn the_uncut_list_frames_its_places() {
    let dir = scratch("simulate-uncut");
    let bytes = std::fs::read(fixture("plaque.dcv")).unwrap();
    let mut project = dc_project::load(&bytes).unwrap().project;
    let mut recess = None;
    for operation in &mut project.operations {
        if let Operation::Pocket(pocket) = operation
            && pocket.corner_relief == CornerRelief::Dogbone
        {
            pocket.corner_relief = CornerRelief::None;
            recess = Some(pocket.name.clone());
        }
    }
    let recess = recess.expect("the plaque's square recess has dogbones");
    let path = dir.join("plaque-square.dcv");
    std::fs::write(&path, dc_project::save(&project).unwrap()).unwrap();

    let mut h = simulated_at(path, &dir, 1500.0);
    assert!(
        h.query_by_label_contains(" places · ").is_none(),
        "off until turned on"
    );
    h.get_by_role_and_label(Role::CheckBox, "Uncut material")
        .click();
    h.run();
    assert!(h.state().app().prefs().sim.uncut_shown);
    let line = h.get_by_label_contains(" · 4 places · ");
    let said = line.accesskit_node().label().unwrap_or_default();
    assert!(said.starts_with(&recess), "{said}");
    assert!(said.contains(" · up to "), "{said}");
    let fitted = h.state().sim_camera().expect("a camera").distance_mm;
    line.click();
    h.run();
    let camera = h.state().sim_camera().unwrap();
    assert_eq!(camera.view(), Some(View3::Top), "from the Top");
    assert!(
        camera.distance_mm < fitted / 2.0,
        "closer, on the recess: {} then {}",
        fitted,
        camera.distance_mm
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Acceptance 157: a frame of the plaque rendered through wgpu is not
/// blank, and differs between the start and the end of the job.
#[test]
fn a_frame_of_the_plaque_is_not_blank_and_changes() {
    let dir = scratch("simulate-frame");
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = wgpu_harness(
        "a_frame_of_the_plaque_is_not_blank_and_changes",
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    ) else {
        std::fs::remove_dir_all(&dir).ok();
        return;
    };
    settle(&mut h);
    command_key(&h, egui::Key::Num5);
    h.run();
    settle(&mut h);
    let pane = h.state().layout().viewport;
    let background = theme::Palette::of(h.ctx.theme()).viewport_bg.to_array();
    // The picture's middle, clear of the pills at its edges.
    let middle = pane.shrink2(egui::vec2(pane.width() * 0.2, pane.height() * 0.2));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixels inside a 1280 × 800 image"
    )]
    let (x0, x1, y0, y1) = (
        middle.left() as u32,
        middle.right() as u32,
        middle.top() as u32,
        middle.bottom() as u32,
    );
    let sample = |h: &mut H| -> Vec<[u8; 4]> {
        let image = h.render().expect("a frame renders");
        let mut pixels = Vec::new();
        for y in (y0..y1).step_by(2) {
            for x in (x0..x1).step_by(2) {
                pixels.push(image.get_pixel(x, y).0);
            }
        }
        pixels
    };
    let far = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).take(3).any(|(a, b)| a.abs_diff(b) > 12);

    let end = sample(&mut h);
    let drawn = end.iter().filter(|p| far(**p, background)).count();
    assert!(
        drawn * 2 > end.len(),
        "the stock fills the middle of the card: {drawn} of {}",
        end.len()
    );
    // Lit faces, not a flat fill: the top, three floors each a little
    // darker for its depth, and walls facing two ways.
    let mut shades: Vec<[u8; 4]> = end.clone();
    shades.sort_unstable();
    shades.dedup();
    assert!(shades.len() > 10, "{} shades", shades.len());

    for _ in 0..6 {
        h.key_press(egui::Key::OpenBracket);
        h.run();
    }
    assert!(h.state().sim_time_s().0 <= 0.0, "back at the start");
    let start = sample(&mut h);
    let changed = start
        .iter()
        .zip(&end)
        .filter(|(a, b)| far(**a, **b))
        .count();
    assert!(
        changed * 50 > end.len(),
        "the cut stock differs from the uncut: {changed} of {}",
        end.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Capability 126: Space plays and pauses, whether the keyboard focus is
/// nowhere, on the stock, or where a click on a control left it; a
/// control reached with Tab keeps Space for itself.
#[test]
fn space_plays_and_pauses() {
    let dir = scratch("simulate-space");
    let mut h = simulated("plaque.dcv", &dir);
    let total = h.state().sim_time_s().1;

    // The rail's button has the focus, from the click that opened the
    // step: Space is the Viewport's all the same.
    h.key_press(egui::Key::Space);
    h.step();
    h.step();
    assert!(h.state().sim_playing(), "Space plays");
    assert_eq!(h.state().step(), Step::Simulate, "and presses nothing");
    assert!(time(&h) < total, "from the start");
    h.key_press(egui::Key::Space);
    h.step();
    h.step();
    assert!(!h.state().sim_playing(), "Space pauses");

    // A control reached with Tab keeps Space: it presses the control.
    h.key_press(egui::Key::Tab);
    h.run();
    let before = (h.state().sim_playing(), time(&h));
    h.key_press(egui::Key::Space);
    h.step();
    h.step();
    assert_eq!(
        (h.state().sim_playing(), time(&h)),
        before,
        "Space was the focused control's"
    );
    std::fs::remove_dir_all(&dir).ok();
}
