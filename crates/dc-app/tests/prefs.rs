// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4: view state is not project state (functional
//! acceptance 81). Visibility, colour mode, camera, panel layout and
//! preferences change nothing in the file and add nothing to the history.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::PathBuf;

use dc_app::test_support::Harness;
use dc_app::{ColourMode, Prefs, Theme, Visibility, WindowGeometry};
use dc_geometry::{Point, Vec2};
use dc_project::OperationId;

use common::plaque_bytes;

/// Acceptance 81: toggling every visibility control, panning, zooming,
/// resizing panels and changing every preference leaves the file unchanged
/// on save and adds nothing to the undo history.
#[test]
fn view_state_is_not_project_state() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.serialize().expect("serializes");
    let project = harness.app.project().clone();

    {
        let view = harness.app.view_mut();
        let all_off = Visibility {
            cuts: false,
            rapids: false,
            plunges: false,
            arrows: false,
            start_points: false,
            tab_marks: false,
            grid: false,
            extents: false,
            only_selected: false,
        };
        view.visibility = all_off;
        view.visibility.arrows = true;
        view.colour = ColourMode::ByDepth;
        view.visibility.only_selected = true;
        view.visibility.extents = true;
        view.hidden_ops.insert(OperationId::new("op4"));
        view.camera.centre_mm = Point::new(123.0, 45.0);
        view.camera.px_per_mm *= 4.0;
        view.camera.viewport_px = Vec2::new(1920.0, 1080.0);
    }
    {
        let prefs = harness.app.prefs_mut();
        prefs.theme = Theme::Dark;
        prefs.window = Some(WindowGeometry {
            x: 10.0,
            y: 20.0,
            width: 1600.0,
            height: 900.0,
            maximized: false,
        });
        prefs.panels.left_width = 400.0;
        prefs.panels.left_open = false;
        prefs.panels.warnings_open = true;
        prefs.sections.insert("project.stock".into(), false);
        prefs.visibility.grid = false;
        prefs.colour = ColourMode::ByDepth;
        prefs.recent.push(PathBuf::from("/tmp/plaque.dcv"));
        prefs
            .export_dirs
            .insert(PathBuf::from("/tmp/plaque.dcv"), PathBuf::from("/tmp/out"));
    }

    assert_eq!(harness.app.project(), &project, "the project changed");
    assert_eq!(
        harness.app.serialize().expect("serializes"),
        before,
        "the file changed"
    );
    assert!(!harness.app.is_dirty(), "view state made the project dirty");
    assert!(!harness.app.can_undo(), "view state is an undo step");
    assert_eq!(harness.app.undo(), None);
    assert_eq!(harness.app.undo_label(), None);
}

/// The preferences' defaults are `gui.toml`'s (`dreamcarve-shell`, which
/// owns the file): a 1280 × 800 window, panels of 280 and 320 px open and
/// the Warnings drawer closed, everything but the arrows visible, colour
/// by operation, the system theme, no recent files.
#[test]
fn prefs_defaults_match_the_file_defaults() {
    let prefs = Prefs::default();
    assert_eq!(prefs.theme, Theme::System);
    assert_eq!(prefs.window, None);
    assert_eq!(
        WindowGeometry::default().width.to_bits(),
        1280.0_f64.to_bits()
    );
    assert_eq!(
        WindowGeometry::default().height.to_bits(),
        800.0_f64.to_bits()
    );
    assert_eq!(prefs.panels.left_width.to_bits(), 280.0_f64.to_bits());
    assert_eq!(prefs.panels.right_width.to_bits(), 320.0_f64.to_bits());
    assert!(prefs.panels.left_open && prefs.panels.right_open);
    assert!(!prefs.panels.warnings_open);
    assert_eq!(
        prefs.visibility,
        Visibility {
            cuts: true,
            rapids: true,
            plunges: true,
            arrows: false,
            start_points: true,
            tab_marks: true,
            grid: true,
            extents: false,
            only_selected: false,
        }
    );
    assert_eq!(prefs.colour, ColourMode::ByOperation);
    assert!(prefs.recent.is_empty());
    assert!(prefs.sections.is_empty());
    assert!(prefs.export_dirs.is_empty());
}

/// Capability 51: *Recent* lists the last ten files, most recent first,
/// each once.
#[test]
fn recent_keeps_ten_most_recent_first() {
    let mut prefs = Prefs::default();
    for k in 0..12 {
        prefs.push_recent(PathBuf::from(format!("/p/{k}.dcv")));
    }
    prefs.push_recent(PathBuf::from("/p/5.dcv"));
    assert_eq!(prefs.recent.len(), Prefs::MAX_RECENT);
    assert_eq!(prefs.recent[0], PathBuf::from("/p/5.dcv"));
    assert_eq!(prefs.recent[1], PathBuf::from("/p/11.dcv"));
    assert_eq!(
        prefs.recent.iter().filter(|p| p.ends_with("5.dcv")).count(),
        1
    );
}
