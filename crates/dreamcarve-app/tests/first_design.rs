// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 27 of phase 5 (`docs/specs/phase-5/functional.md`, exit
//! criteria): the hook plate is designed in the window, with no import.
//! The quick start "Your first design" is followed through the window's
//! own controls (`design_window`), and the project that leaves is the
//! one the chapter's steps build with no window (`design_steps`), which
//! `gui_equality::the_hook_plate_drawn_equals_the_cli` exports against
//! the command line on every post-processor.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

#[path = "../../dreamcarve/tests/app_steps/mod.rs"]
mod app_steps;
#[path = "../../dreamcarve/tests/design_steps/mod.rs"]
mod design_steps;
mod design_window;

use common::{QueuedDialogs, options, scratch};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

/// The exit criterion: every shape of the hook plate drawn and arranged
/// through the window, from *Draw from scratch*, is the shape the
/// chapter's steps give; then the tools and operations of the chapter
/// make a project that has nothing to report and plans its export.
#[test]
fn the_hook_plate_is_designed_in_the_window() {
    let dir = scratch("first-design");
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            dreamcarve_app::Shell::new(cc, options(&dir.join("config"), None, &dialogs))
                .expect("the shell starts")
        });
    h.run();
    let text = design_steps::chapter();
    h.get_by_label("Draw from scratch").click();
    h.run_steps(12);
    assert!(h.state().app().project().instances.is_empty());
    // The board first: the Stock step's three sizes, typed there, and
    // back to Design, where the drawing tool is still in hand.
    common::go_to(&mut h, "Stock");
    for (label, value) in app_steps::settings(&design_steps::part_cell(&text, "Stock")) {
        h.get_by_role_and_label(Role::TextInput, &label).focus();
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(2);
        h.event(egui::Event::Text(value));
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(12);
    }
    let stock = &h.state().app().project().stock;
    assert_eq!(
        (stock.width_mm, stock.height_mm, stock.thickness_mm),
        (180.0, 100.0, Some(12.0))
    );
    common::go_to(&mut h, "Design");
    h.key_press(egui::Key::F);
    h.run_steps(12);

    design_window::draw_all(&mut h, &text, &mut |_, _| {});

    let steps = design_steps::drawn(&text);
    design_window::assert_same_drawing(h.state().app().project(), steps.project());

    // The rest of the chapter, on the project the window drew.
    let lib = app_steps::library(&text);
    design_steps::finish(h.state_mut().app_mut(), &text, &lib);
    common::settle(&mut h);
    let app = h.state().app();
    assert_eq!(app.project().operations.len(), 5);
    assert!(app.project().bitmaps.is_empty(), "nothing was imported");
    let validation = app.validation();
    assert!(validation.errors.is_empty(), "{:?}", validation.errors);
}

/// The keys the chapter gives for a shape, with no pointer at all: the
/// tool's key, Tab into the bar's first field, a value, Tab, a value,
/// Enter for the start; then the same for the sizes.
#[test]
fn a_shape_is_drawn_from_the_keyboard_alone() {
    let dir = scratch("first-design-keys");
    let dialogs = QueuedDialogs::default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| {
            dreamcarve_app::Shell::new(cc, options(&dir.join("config"), None, &dialogs))
                .expect("the shell starts")
        });
    h.run();
    h.get_by_label("Draw from scratch").click();
    h.run_steps(12);
    let keys = |h: &mut Harness<'static, dreamcarve_app::Shell>, typed: &[&str]| {
        // Tab from the drawing goes to the bar's first field with its
        // text selected, and from a field to the next.
        for text in typed {
            h.key_press(egui::Key::Tab);
            h.run_steps(4);
            h.event(egui::Event::Text((*text).to_owned()));
            h.run_steps(4);
        }
        h.key_press(egui::Key::Enter);
        h.run_steps(12);
    };
    // The Rectangle is in hand after Draw from scratch.
    keys(&mut h, &["10", "10"]);
    assert!(h.state().app().project().instances.is_empty(), "the corner");
    keys(&mut h, &["160", "80"]);
    let project = h.state().app().project();
    assert_eq!(project.instances.len(), 1);
    let bounds = dc_app::properties::placement(project, &[project.instances[0].id.clone()])
        .unwrap()
        .bounds;
    assert_eq!(
        (bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y),
        (10.0, 10.0, 170.0, 90.0)
    );
    // A choice clicked, or an option typed and entered, leaves the
    // keyboard with the drawing, so Tab still goes to the first field.
    h.get_by_role_and_label(Role::RadioButton, "Rounded")
        .click();
    h.run_steps(12);
    h.key_press(egui::Key::Tab);
    h.run_steps(4);
    assert!(h.get_by_role_and_label(Role::TextInput, "X").is_focused());
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    // A circle the same way: C, its centre, its diameter.
    h.key_press(egui::Key::C);
    h.run_steps(12);
    keys(&mut h, &["18", "18"]);
    keys(&mut h, &["5"]);
    let project = h.state().app().project();
    assert_eq!(project.instances.len(), 2);
    assert_eq!(
        project.definitions[&project.instances[1].definition].shape,
        Some(dc_project::Shape::Circle { diameter_mm: 5.0 })
    );
    // A line: its start, a point by length and angle, and Enter again
    // with no new point to finish it.
    h.key_press(egui::Key::P);
    h.run_steps(12);
    keys(&mut h, &["90", "10"]);
    keys(&mut h, &["80", "90"]);
    assert_eq!(h.state().app().project().instances.len(), 2, "not yet");
    h.key_press(egui::Key::Enter);
    h.run_steps(12);
    assert_eq!(h.state().app().project().instances.len(), 3);
}
