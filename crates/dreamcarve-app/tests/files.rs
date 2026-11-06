// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Recent list (functional spec, capability 51: "*Recent* lists the
//! last ten files").

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::PathBuf;

use common::{QueuedDialogs, command_key, harness, options, scratch};
use dreamcarve_app::files;
use dreamcarve_shell::{Paths, gui_prefs};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;

/// Twelve projects opened one after another: Recent keeps the last ten,
/// newest first, in the app and in `gui.toml`, and the File menu lists
/// them by name. A listed file that has since been deleted or moved is
/// dropped from the list, not shown to fail.
#[test]
fn recent_lists_ten_and_drops_missing() {
    let dir = scratch("recent");
    let config = dir.join("config");
    let plaque = std::fs::read(common::fixture("plaque.dcv")).unwrap();
    let paths: Vec<PathBuf> = (1..=12)
        .map(|n| {
            let path = dir.join(format!("project-{n:02}.dcv"));
            std::fs::write(&path, &plaque).unwrap();
            path
        })
        .collect();

    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&config, None, &dialogs), 1280.0, 800.0);
    for path in &paths {
        dialogs.answer(path);
        command_key(&h, egui::Key::O);
        h.run();
        assert_eq!(h.state().app().source(), Some(path.as_path()));
    }
    let newest_first: Vec<PathBuf> = paths.iter().rev().take(10).cloned().collect();
    assert_eq!(h.state().app().prefs().recent, newest_first);
    let saved = gui_prefs::load(&Paths::in_dir(config.clone())).prefs;
    assert_eq!(saved.recent, newest_first, "gui.toml keeps the same ten");

    // The menu lists the ten by file name.
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open Recent").hover();
    h.run();
    for path in &newest_first {
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            h.query_by_role_and_label(Role::Button, name).is_some(),
            "Recent lists {name}"
        );
    }
    for path in &paths[..2] {
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            h.query_by_role_and_label(Role::Button, name).is_none(),
            "{name} fell off the list"
        );
    }
    h.key_press(egui::Key::Escape);
    h.run();

    // Two of them go missing: the list drops them.
    std::fs::remove_file(&paths[11]).unwrap();
    std::fs::remove_file(&paths[5]).unwrap();
    let mut prefs = h.state().app().prefs().clone();
    let listed = files::recent(&mut prefs);
    let expected: Vec<PathBuf> = newest_first
        .iter()
        .filter(|p| **p != paths[11] && **p != paths[5])
        .cloned()
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(
        prefs.recent, expected,
        "the missing files leave the preferences too"
    );

    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open Recent").hover();
    h.run();
    assert!(
        h.query_by_role_and_label(Role::Button, "project-12.dcv")
            .is_none()
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "project-11.dcv")
            .is_some()
    );
    assert_eq!(h.state().app().prefs().recent, expected);
    std::fs::remove_dir_all(&dir).unwrap();
}
