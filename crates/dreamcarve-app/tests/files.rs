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

use common::{QueuedDialogs, command_key, copy_fixture, harness, options, scratch};
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

/// Slice D9 (design, "Preferences"): a `gui.toml` that cannot be read is
/// the defaults, said once and never written over by the app's own
/// bookkeeping. The file here has a typo on its second line. The window
/// opens the plaque at start, dark (the default, not the file's light),
/// and the status bar's message is about the preferences, naming the file
/// and the line, where *Opened plaque* used to replace it; the Warnings
/// drawer lists the same as a hint, which *Copy* leaves out, since it is
/// not `check`'s. Opening the project made it the newest recent file and
/// closing the window records its place, and neither rewrites the file:
/// it is byte for byte what the user left. Then the user changes a
/// preference (the appearance): the file is written, with it, and the
/// hint goes.
#[test]
fn a_damaged_gui_toml_is_said_once_and_kept_until_a_preference_changes() {
    let dir = scratch("damaged-prefs");
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let damaged = "theme = \"light\"\n[panels\nleft_width = 250.0\n";
    let file = config.join("gui.toml");
    std::fs::write(&file, damaged).unwrap();
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&config, Some(plaque.clone()), &dialogs),
        1280.0,
        800.0,
    );
    assert_eq!(h.state().app().source(), Some(plaque.as_path()));
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Dark);

    let message = h
        .state()
        .message()
        .expect("the status bar says so")
        .to_owned();
    let at = format!("{}:2:", file.display());
    assert!(
        message.contains(&at) && message.starts_with("Defaults are in use"),
        "it names the file and the line: {message}"
    );
    assert_eq!(h.state().app().hints(), std::slice::from_ref(&message));
    let hints = dc_app::warnings::counts(h.state().app()).hints;
    h.get_by_label_contains("Warnings:").click();
    h.run();
    assert!(
        h.query_by_label(&format!("hint: {message}")).is_some(),
        "the drawer lists it as a hint"
    );
    assert!(
        !dc_app::warnings::copy_text(h.state().app()).contains("gui.toml"),
        "Copy is check's text"
    );

    // The recent list changed, and so does the window's place on exit.
    assert_eq!(h.state().app().prefs().recent, [plaque]);
    h.run();
    eframe::App::on_exit(h.state_mut());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        damaged,
        "the damaged file is as the user left it"
    );

    // The user changes a preference: now it is theirs to write.
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Appearance").hover();
    h.run();
    h.get_by_role_and_label(Role::Button, "Light").click();
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Light);
    let written = gui_prefs::load(&Paths::in_dir(config.clone()));
    assert_eq!(written.unread, None, "the file reads again");
    assert_eq!(written.prefs.theme, gui_prefs::Theme::Light);
    assert!(h.state().app().hints().is_empty(), "and the hint goes");
    assert_eq!(
        dc_app::warnings::counts(h.state().app()).hints,
        hints - 1,
        "from the drawer too"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
