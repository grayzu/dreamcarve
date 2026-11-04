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
