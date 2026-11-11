// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 30 of phase 4 (#622; functional spec, capabilities 64 and 64a;
//! acceptance 101, first half), in the window: a library tool edited in
//! the Tool Library sheet writes the library file at once and never the
//! open project, and the library's own display units change only what the
//! sheet shows. Slice 31 (#623; capability 64b; acceptance 101, second
//! half): *Import…* shows a review before anything is written, with *Keep
//! mine* / *Replace with imported* only when a tool changed. The model's
//! tests (groups, *Revert changes*, *New tool*,
//! *Duplicate*) are `crates/dc-app/tests/library.rs`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::{Path, PathBuf};

use common::{QueuedDialogs, fixture, harness, options, scratch};
use dc_app::tools::ToolField;
use dc_library::Library;
use dc_project::Units;
use dreamcarve_app::Shell;
use dreamcarve_shell::{Paths, gui_prefs};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

/// The library `tools import fixtures/fusion/sample.tools` writes.
fn sample_library() -> Library {
    let bytes = std::fs::read(fixture("fusion/sample.tools")).unwrap();
    dc_app::library::import(&Library::default(), &bytes, false)
        .unwrap()
        .library
}

/// A config directory under `dir` holding `library` as its `tools.json`;
/// the directory and the bytes written.
fn config_with(dir: &Path, library: &Library) -> (PathBuf, String) {
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let written = dc_library::save(library).unwrap();
    std::fs::write(config.join("tools.json"), &written).unwrap();
    (config, written)
}

/// The window on the plaque (opened in place, never saved) with the Tool
/// Library sheet open.
fn open_sheet(config: &Path, dialogs: &QueuedDialogs) -> Harness<'static, Shell> {
    let mut h = harness(
        options(config, Some(fixture("plaque.dcv")), dialogs),
        1280.0,
        800.0,
    );
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    h
}

/// Selects library row `name` in the open sheet: selecting is editing.
fn select_row(h: &mut Harness<'_, Shell>, name: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, name)
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, name)
        .click();
    h.run();
}

/// Types `text` over the sheet's field `label` and leaves it with Enter.
fn type_into(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, label)
        .focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

/// The sheet's field `label`'s text.
fn field_value(h: &Harness<'_, Shell>, label: &str) -> String {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, label)
        .value()
        .unwrap_or_default()
}

/// Acceptance 101: setting a library tool's feed in the editing pane
/// writes `tools.json` at once, equal to `dc_library::save` of the library
/// after the same edit (what the CLI's library would be); the open
/// project, its undo history and its saved state do not change.
#[test]
fn edit_writes_the_library_and_not_the_project() {
    let dir = scratch("library-edit");
    let library = sample_library();
    let (config, written) = config_with(&dir, &library);
    let dialogs = QueuedDialogs::default();
    let mut h = open_sheet(&config, &dialogs);
    let project = h.state().app().project().clone();
    let undo: Vec<String> = h
        .state()
        .app()
        .history()
        .undo_labels()
        .map(str::to_owned)
        .collect();

    select_row(&mut h, "5 mm Drill");
    assert_eq!(field_value(&h, "Name"), "5 mm Drill", "selecting edits");
    assert_eq!(field_value(&h, "Feed"), "", "the unset feed stays empty");
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written,
        "selecting writes nothing"
    );

    type_into(&mut h, "Feed", "250");
    let drill = library
        .tools
        .iter()
        .find(|t| t.tool.name == "5 mm Drill")
        .unwrap()
        .id
        .clone();
    let expected =
        dc_app::library::edit(&library, &drill, ToolField::Feed, "250", Units::Mm).unwrap();
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        dc_library::save(&expected).unwrap(),
        "the library file after the edit, written at once"
    );
    assert_eq!(field_value(&h, "Feed"), "250");

    let app = h.state().app();
    assert_eq!(app.project(), &project, "the project keeps its tools");
    let after: Vec<String> = app.history().undo_labels().map(str::to_owned).collect();
    assert_eq!(after, undo, "a library edit is not in the Edit history");
    assert!(!app.is_dirty(), "the project is still saved");

    // *Revert changes* puts the drill back as the sheet opened it.
    h.get_by_role(Role::Dialog)
        .get_by_label("Revert changes")
        .click();
    h.run();
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written,
        "reverted, the file is as it was"
    );
    assert_eq!(field_value(&h, "Feed"), "");
    assert_eq!(h.state().app().project(), &project);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 101: switching the library's units to inches shows the
/// 6.35 mm end mill as `0.25 in` and changes nothing else: the library
/// file stays in millimetres and the project keeps its units. The choice
/// is `Prefs::library_units`, kept in `gui.toml`.
#[test]
fn units_are_display_only() {
    let dir = scratch("library-units");
    let library = sample_library();
    let (config, written) = config_with(&dir, &library);
    let dialogs = QueuedDialogs::default();
    let mut h = open_sheet(&config, &dialogs);
    assert_eq!(h.state().app().project().display_units, Units::Mm);
    assert_eq!(
        h.state().app().prefs().library_units,
        None,
        "no choice yet: the library follows the project"
    );

    select_row(&mut h, "1/4 in Flat End Mill");
    assert_eq!(field_value(&h, "Diameter"), "6.35");
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "in")
        .click();
    h.run();
    assert_eq!(field_value(&h, "Diameter"), "0.25");
    assert_eq!(
        format!(
            "{} {}",
            dc_app::library::diameter_text(6.35, Units::Inch),
            dc_app::tools::field_unit(ToolField::Diameter, Units::Inch)
        ),
        "0.25 in",
        "the table's diameter and the field's unit"
    );

    let app = h.state().app();
    assert_eq!(app.prefs().library_units, Some(Units::Inch));
    assert_eq!(app.project().display_units, Units::Mm, "the project's own");
    assert!(!app.is_dirty(), "not a project change");
    assert_eq!(
        std::fs::read_to_string(config.join("tools.json")).unwrap(),
        written,
        "the library file stays in millimetres"
    );

    // A diameter typed in inches is stored in millimetres.
    type_into(&mut h, "Diameter", "0.125");
    let stored = dreamcarve_shell::library::read(&Paths::in_dir(config.clone()))
        .unwrap()
        .unwrap();
    let mill = stored
        .tools
        .iter()
        .find(|t| t.tool.name == "1/4 in Flat End Mill")
        .unwrap();
    assert!((mill.tool.diameter_mm - 3.175).abs() < 1e-9);

    // The choice survives a restart, through gui.toml.
    h.get_by_role(Role::Dialog).get_by_label("Close").click();
    h.run();
    common::press_close(&mut h);
    let loaded = gui_prefs::load(&Paths::in_dir(config.clone()));
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    let prefs = dreamcarve_app::files::prefs_from_file(&loaded.prefs);
    assert_eq!(prefs.library_units, Some(Units::Inch));
    let mut back = prefs.clone();
    back.library_units = Some(Units::Mm);
    assert_eq!(
        dreamcarve_app::files::prefs_from_file(&dreamcarve_app::files::prefs_to_file(&back)),
        back
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// Slice 31 (#623; functional 64b; acceptance 71 and 101, second half):
// the import review, shown in place of the panes before anything is
// written.

/// The library `tools import fixtures/fusion/vendors.json` writes: the
/// fixture library the sample is imported into.
fn vendors_library() -> Library {
    let bytes = std::fs::read(fixture("fusion/vendors.json")).unwrap();
    dc_app::library::import(&Library::default(), &bytes, false)
        .unwrap()
        .library
}

/// What `tools import [--replace] fixtures/fusion/sample.tools` makes of
/// `library`.
fn merge_sample(library: &Library, replace: bool) -> dc_library::import::Merged {
    let bytes = std::fs::read(fixture("fusion/sample.tools")).unwrap();
    let incoming = dc_library::import::read(&bytes).unwrap();
    dc_library::import::merge(library, incoming, replace).unwrap()
}

/// *Import…* in the open sheet, answered with `fixtures/fusion/sample.tools`.
fn import_sample(h: &mut Harness<'_, Shell>, dialogs: &QueuedDialogs) {
    dialogs.answer(&fixture("fusion/sample.tools"));
    h.get_by_role(Role::Dialog)
        .query_all_by_label("Import…")
        .next()
        .unwrap()
        .click();
    h.run();
}

/// The library file under `config`.
fn library_text(config: &Path) -> String {
    std::fs::read_to_string(config.join("tools.json")).unwrap()
}

/// Whether the open sheet has a node labelled `label`.
fn shows(h: &Harness<'_, Shell>, label: &str) -> bool {
    h.get_by_role(Role::Dialog).query_by_label(label).is_some()
}

/// Capability 64b: importing the sample into the fixture library shows
/// the review in place, with the counts `tools import` prints (New is
/// its imported, *Already in your library* its already present, Skipped
/// its skipped), and writes nothing until *Import*. *Import* writes what
/// `tools import` writes and returns to the table with the banner, whose
/// *Details* reopens the review read-only.
#[test]
fn import_review_counts_equal_tools_import() {
    let dir = scratch("library-import-review");
    let library = vendors_library();
    let (config, written) = config_with(&dir, &library);
    let dialogs = QueuedDialogs::default();
    let mut h = open_sheet(&config, &dialogs);
    let expected = merge_sample(&library, false);
    let report = &expected.report;
    assert!(report.imported > 0, "the sample adds tools");

    import_sample(&mut h, &dialogs);
    assert!(shows(&h, "Import sample.tools"), "the review's title");
    for label in [
        format!("{} New", report.imported),
        format!("{} Already in your library", report.present),
        "0 Changed".to_owned(),
        format!("{} Skipped", report.skipped.unwrap()),
    ] {
        assert!(shows(&h, &label), "the review counts `{label}`");
    }
    // Every item of the file is a row, with its status.
    assert!(shows(&h, "5 mm Drill · New"));
    assert!(shows(
        &h,
        "Probe, 2 mm Stylus · Skipped: unsupported tool type `probe`"
    ));
    assert!(!shows(&h, "Keep mine"), "nothing changed, so no choice");
    assert_eq!(library_text(&config), written, "nothing written yet");

    let primary = format!("Import {} tools", report.imported);
    h.get_by_role(Role::Dialog).get_by_label(&primary).click();
    h.run();
    assert_eq!(
        library_text(&config),
        dc_library::save(&expected.library).unwrap(),
        "the library `tools import` writes"
    );
    let banner = format!(
        "Imported {} tools from sample.tools. {} skipped.",
        report.imported,
        report.skipped.unwrap()
    );
    assert!(shows(&h, &banner), "the banner after the import");
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_role_and_label(Role::CheckBox, "5 mm Drill")
            .is_some(),
        "back on the table, with the new rows"
    );

    // *Details* reopens the review, read-only: the counts, no Import.
    h.get_by_role(Role::Dialog).get_by_label("Details").click();
    h.run();
    assert!(shows(&h, &format!("{} New", report.imported)));
    assert!(!shows(&h, &primary), "read-only");
    h.get_by_role(Role::Dialog).get_by_label("Back").click();
    h.run();
    assert!(shows(&h, &banner), "Back from Details keeps the banner");
    assert_eq!(
        library_text(&config),
        dc_library::save(&expected.library).unwrap()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 64b: after one imported tool is edited in the library, a
/// second import of its file shows it as *Changed* and offers *Keep mine*
/// (the default; `merge(replace: false)`, the file's version added beside
/// yours) or *Replace with imported* (`merge(replace: true)`, yours
/// overwritten in place).
#[test]
fn changed_tools_offer_keep_or_replace() {
    let dir = scratch("library-import-changed");
    let library = sample_library();
    let drill = library
        .tools
        .iter()
        .find(|t| t.tool.name == "5 mm Drill")
        .unwrap()
        .id
        .clone();
    let edited =
        dc_app::library::edit(&library, &drill, ToolField::Feed, "250", Units::Mm).unwrap();
    let (config, written) = config_with(&dir, &edited);
    let dialogs = QueuedDialogs::default();
    let mut h = open_sheet(&config, &dialogs);

    // Keep mine, the default.
    import_sample(&mut h, &dialogs);
    assert!(shows(&h, "1 Changed"), "the edited drill is changed");
    assert!(shows(&h, "12 Already in your library"));
    assert!(shows(&h, "0 New"));
    assert!(shows(&h, "1 tool changed since you last imported it:"));
    let keep = h
        .get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "Keep mine");
    assert_eq!(
        keep.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "the default"
    );
    assert!(shows(&h, "5 mm Drill · Changed: added as 5 mm Drill (2)"));
    assert_eq!(library_text(&config), written, "nothing written yet");
    h.get_by_role(Role::Dialog)
        .get_by_label("Import 1 tool")
        .click();
    h.run();
    assert_eq!(
        library_text(&config),
        dc_library::save(&merge_sample(&edited, false).library).unwrap(),
        "Keep mine writes what merge(replace: false) writes"
    );

    // Again from the edited library: Replace with imported.
    std::fs::write(config.join("tools.json"), &written).unwrap();
    import_sample(&mut h, &dialogs);
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "Replace with imported")
        .click();
    h.run();
    assert!(shows(&h, "5 mm Drill · Changed: replaces yours"));
    assert_eq!(library_text(&config), written, "choosing writes nothing");
    h.get_by_role(Role::Dialog)
        .get_by_label("Import 1 tool")
        .click();
    h.run();
    let replaced = merge_sample(&edited, true).library;
    assert_eq!(
        library_text(&config),
        dc_library::save(&replaced).unwrap(),
        "Replace with imported writes what merge(replace: true) writes"
    );
    assert_eq!(
        replaced, library,
        "the drill as the file has it, its id kept"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 64b: when nothing is *Changed*, *Keep mine* and *Replace
/// with imported* would write the same, so the choice is not shown; with
/// every tool already in the library there is nothing to import.
#[test]
fn no_choice_when_nothing_changed() {
    let dir = scratch("library-import-same");
    let library = sample_library();
    let (config, written) = config_with(&dir, &library);
    let dialogs = QueuedDialogs::default();
    let mut h = open_sheet(&config, &dialogs);
    import_sample(&mut h, &dialogs);
    assert!(shows(&h, "Import sample.tools"), "the review shows");
    assert!(shows(&h, "0 Changed"));
    assert!(shows(&h, "13 Already in your library"));
    assert!(shows(&h, "5 mm Drill · Same"));
    assert!(
        !shows(&h, "Keep mine") && !shows(&h, "Replace with imported"),
        "no choice when nothing changed"
    );
    assert!(
        h.get_by_role(Role::Dialog)
            .get_by_label("Nothing to import")
            .accesskit_node()
            .is_disabled(),
        "nothing would change the library"
    );
    assert_eq!(library_text(&config), written);
    std::fs::remove_dir_all(&dir).unwrap();
}
