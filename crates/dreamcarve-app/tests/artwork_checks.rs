// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The artwork checks in the window (functional spec, capabilities 55 and
//! 76, acceptance 106), through the accessibility tree: the Import
//! drawing sheet lists the checks `dreamcarve init` prints for an SVG and
//! the two `dreamcarve check` reads from a DXF's shapes, in the CLI's
//! words, with one line under them that points to `dreamcarve image-prep`
//! and the book's *Preparing artwork* chapter; the checks never disable
//! the sheet's button; and after the import the overlap is a row of the
//! Warnings drawer that selects the shapes it names on the Design step.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use common::{QueuedDialogs, fixture, harness, options, scratch};
use dc_app::steps::{Severity, State, Step};
use dreamcarve_app::Shell;
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

/// A file dropped on the window: a path on disk.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// A 1280 × 800 window on a new project, with `name` dropped on it: the
/// Import drawing sheet is open.
fn dropped(test: &str, name: &str) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(test);
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture(name))));
    h.run();
    (h, dir)
}

/// What the line under the sheet's checks says.
const ADVICE: &str = "The app does not clean a drawing. Where a V-Carve or an Inlay needs these \
                      fixed, run dreamcarve image-prep on the file and add the drawing it writes.";

/// The URLs the last frame asked the operating system to open.
fn opened(h: &Harness<'_, Shell>) -> Vec<String> {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect()
}

/// Capability 55, acceptance 106: the sheet over the messy SVG lists
/// every check `init` prints for it, each with its message whole and its
/// code, and under them the line that names `dreamcarve image-prep` with
/// a link to the *Preparing artwork* chapter. A code opens its entry in
/// the Warnings chapter. The eight checks make the list longer than the
/// sheet: it scrolls, and the button stays inside the sheet and the
/// window, enabled. *Add* imports the seven shapes all the same, and the
/// Design step is marked with a warning.
#[test]
fn import_sheet_lists_an_svgs_checks_and_points_to_image_prep() {
    let (mut h, dir) = dropped("artwork-checks-svg", "prep/messy.svg");
    let bytes = std::fs::read(fixture("prep/messy.svg")).unwrap();
    let checks = dc_job::checks::svg(&bytes).expect("the checks run");
    assert_eq!(checks.len(), 8);

    let sheet = h.get_by_role(Role::Dialog);
    for check in &checks {
        assert!(
            sheet.query_all_by_label(&check.message).next().is_some(),
            "the sheet says, whole: {}",
            check.message
        );
        assert!(
            sheet.query_all_by_label(check.code).next().is_some(),
            "and gives its code: {}",
            check.code
        );
    }
    assert!(
        sheet.query_by_label(ADVICE).is_some(),
        "one line says what to do"
    );
    // The sheet reports no rectangle of its own: its corner grip is its
    // bottom right corner.
    let corner = sheet
        .get_by_role_and_label(Role::Splitter, "Resize the sheet")
        .rect()
        .max;
    let add = sheet.get_by_label("Add 7 shapes");
    assert!(
        !add.accesskit_node().is_disabled(),
        "a check never disables the button"
    );
    assert!(
        add.rect().bottom() < corner.y && corner.y <= 800.0,
        "the button stays in the sheet, and the sheet in the window: {:?} above {corner:?}",
        add.rect()
    );
    let last = sheet
        .query_all_by_label(&checks[7].message)
        .next()
        .expect("the last check")
        .rect();
    assert!(
        last.bottom() > add.rect().top(),
        "the list is longer than the sheet shows at once, so it scrolls: {last:?}"
    );

    // The chapter link, and a code's.
    h.get_by_role(Role::Dialog)
        .get_by_label("Preparing artwork")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_label("Preparing artwork")
        .click();
    h.step();
    assert_eq!(opened(&h), [dc_app::import::PREPARING_ARTWORK_URL]);
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_label("svg_stroke_only_path")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_label("svg_stroke_only_path")
        .click();
    h.step();
    assert_eq!(
        opened(&h),
        ["https://grayzu.github.io/dreamcarve/user-guide/warnings.html#svg_stroke_only_path"]
    );
    h.run();

    h.get_by_role(Role::Dialog)
        .get_by_label("Add 7 shapes")
        .click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none(), "the sheet closed");
    assert_eq!(h.state().app().project().instances.len(), 7);
    assert_eq!(h.state().app().artwork_checks().len(), 2);
    assert_eq!(
        h.state().step_states().of(Step::Design),
        State::Attention(Severity::Warning)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capability 55, acceptance 106: a clean drawing's sheet lists no check
/// and has no line about `image-prep`.
#[test]
fn a_clean_drawings_sheet_says_nothing_of_image_prep() {
    let (h, dir) = dropped("artwork-checks-clean", "prep/clean.svg");
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label(ADVICE).is_none());
    assert!(sheet.query_by_label("Preparing artwork").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The quick start's first minute: *New from drawing…* on the coaster, a
/// circle drawn as a hairline with no fill. It closes, so it is imported
/// whole as a closed outline, and the sheet lists no check and no line
/// about `image-prep` (the testers of 0.6.0-alpha.1 were told such a
/// drawing could not be carved, and it could). *Create project* makes
/// the project, with no note, and it opens on Stock with Design complete.
#[test]
fn the_coasters_outline_is_not_a_check_and_the_project_opens_on_stock() {
    let dir = scratch("artwork-checks-coaster");
    let dialogs = QueuedDialogs::default();
    let mut h = harness(options(&dir.join("config"), None, &dialogs), 1280.0, 800.0);
    dialogs.answer(&common::repo_root().join("fixtures/coaster.svg"));
    h.get_by_role_and_label(Role::Button, "New from drawing…")
        .click();
    h.run();
    let sheet = h.get_by_role(Role::Dialog);
    assert!(sheet.query_by_label("svg_stroke_only_path").is_none());
    assert!(sheet.query_by_label(ADVICE).is_none());
    sheet.get_by_label("Create project").click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none(), "the sheet closed");
    let app = h.state().app();
    assert_eq!(app.project().instances.len(), 1);
    assert!(app.notes().is_empty(), "{:?}", app.notes());
    assert!(app.artwork_checks().is_empty());
    assert_eq!(h.state().step_states().of(Step::Design), State::Complete);
    assert_eq!(h.state().step(), Step::Stock);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The checks make the sheet scroll, and a sheet scrolled to its end and
/// closed used to open on the next drawing still scrolled there, its
/// preview out of sight. Each sheet opens at its top.
#[test]
fn a_sheet_opens_at_its_top_whatever_the_last_one_was_scrolled_to() {
    let (mut h, dir) = dropped("artwork-checks-scroll", "prep/messy.svg");
    let preview_top = |h: &Harness<'_, Shell>| {
        h.get_by_role(Role::Dialog)
            .get_by_label("Preview")
            .rect()
            .top()
    };
    let at_open = preview_top(&h);
    h.get_by_role(Role::Dialog)
        .get_by_label("Preparing artwork")
        .scroll_to_me();
    h.run();
    assert!(
        preview_top(&h) < at_open - 50.0,
        "the list is scrolled to its end: {} from {at_open}",
        preview_top(&h)
    );
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none());
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(fixture("prep/messy.svg"))));
    h.run();
    assert!(
        (preview_top(&h) - at_open).abs() <= 0.5,
        "the next sheet opens at its top: {} for {at_open}",
        preview_top(&h)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Capabilities 55 and 76, acceptance 106: the sheet over the tray's DXF
/// lists the overlap of its two pockets and its speck, read from the
/// shapes it would add, and follows the layers: with *pockets* unticked
/// the overlap is off the list. Imported whole, the overlap is a warning
/// row of the Warnings drawer; clicking it opens the Design step with
/// the two pockets selected.
#[test]
fn a_dxfs_checks_follow_the_layers_and_the_drawer_row_selects_the_shapes() {
    let (mut h, dir) = dropped("artwork-checks-dxf", "prep/tool-tray.dxf");
    let listed = |h: &Harness<'_, Shell>, code: &str| {
        h.get_by_role(Role::Dialog)
            .query_all_by_label(code)
            .next()
            .is_some()
    };
    assert!(listed(&h, "shapes_overlap") && listed(&h, "feature_below_size"));
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label_contains("the first is `pockets-1` and `pockets-2`")
            .is_some(),
        "the overlap names the pockets"
    );
    assert!(h.get_by_role(Role::Dialog).query_by_label(ADVICE).is_some());

    let pockets = "pockets · 2 closed, 0 open";
    h.get_by_role(Role::Dialog).get_by_label(pockets).click();
    h.run();
    assert!(
        !listed(&h, "shapes_overlap"),
        "no pockets, no overlap to list"
    );
    assert!(listed(&h, "feature_below_size"));
    h.get_by_role(Role::Dialog).get_by_label(pockets).click();
    h.run();
    assert!(listed(&h, "shapes_overlap"));

    h.get_by_role(Role::Dialog)
        .get_by_label("Add 6 shapes")
        .click();
    h.run();
    assert!(h.query_by_role(Role::Dialog).is_none(), "the sheet closed");
    assert_eq!(
        h.state().step_states().of(Step::Design),
        State::Attention(Severity::Warning)
    );

    // Away from Design, then to it by the row.
    common::go_to(&mut h, "Stock");
    h.state_mut().app_mut().select(dc_app::Selection::Nothing);
    h.run();
    h.get_by_label_contains("Warnings:").click();
    h.run();
    assert!(h.state().warnings_open());
    let row = dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|group| group.rows)
        .find(|row| row.code.as_deref() == Some("shapes_overlap"))
        .expect("the overlap's row");
    assert_eq!(row.level, dc_app::warnings::Level::Warning);
    h.get_by_label(&row.name).click();
    h.run();
    assert_eq!(h.state().step(), Step::Design);
    let selected: Vec<String> = h
        .state()
        .app()
        .selection()
        .instances()
        .iter()
        .map(|id| {
            h.state()
                .app()
                .project()
                .instances
                .iter()
                .find(|i| &i.id == id)
                .unwrap()
                .name
                .clone()
        })
        .collect();
    assert_eq!(selected, ["pockets-1", "pockets-2"]);
    std::fs::remove_dir_all(&dir).unwrap();
}
