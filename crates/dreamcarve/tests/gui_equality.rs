// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app and the CLI on the same input give the same bytes
//! (`docs/specs/phase-4/design.md`, "Test harness"; functional spec, S20).
//! Each test runs the `dreamcarve` binary in its own directory under
//! `CARGO_TARGET_TMPDIR` and compares its output with what `dc-app`
//! makes of the same input through the test harness.
//!
//! Slice 8 (#537, acceptance 71): the Tool Library window's import report
//! and library file are `tools import`'s, and a tool the app adds from the
//! library is the copy `tools add` writes.
//!
//! Slice 17 (#546, acceptance 78): the tabs the app places, drags, deletes
//! and adds export to the program the CLI writes for the saved file.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test helpers fail the test on a broken setup"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dc_app::test_support::Harness;
use dc_library::Library;

/// A fresh, empty working directory for test `name`.
fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("gui_equality")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("old work dir removed");
    }
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The user config directory of a test working in `dir`.
fn config_dir(dir: &Path) -> PathBuf {
    dir.join("config")
}

/// Runs `dreamcarve` with `args` in `dir`, its config directory
/// [`config_dir`]; fails the test unless it exits 0.
fn run(dir: &Path, args: &[&str]) -> String {
    let out: Output = Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
        .current_dir(dir)
        .env("DREAMCARVE_CONFIG_DIR", config_dir(dir))
        .args(args)
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Acceptance 71: `tools import tests/fixtures/fusion/sample.tools` (the
/// fixture read in place) prints the report the app shows, line for line,
/// and leaves the library file the app writes; importing again with
/// `--replace` agrees too.
#[test]
fn tools_import_report_equals_the_app() {
    let dir = work_dir("tools_import_report_equals_the_app");
    let sample = repo_root().join("tests/fixtures/fusion/sample.tools");
    let sample_arg = sample.to_str().expect("utf-8 path");
    let library_file = config_dir(&dir).join("tools.json");
    let library_text = library_file.display().to_string();
    let bytes = std::fs::read(&sample).expect("fixture readable");

    let mut lib = Library::default();
    for replace in [false, true] {
        let mut args = vec!["tools", "import", sample_arg];
        if replace {
            args.push("--replace");
        }
        let printed = run(&dir, &args);
        let merged = dc_app::library::import(&lib, &bytes, replace).expect("the app imports");
        let mut expected = merged.report.lines().join("\n");
        expected.push('\n');
        expected.push_str(&merged.report.summary(&library_text));
        expected.push('\n');
        assert_eq!(printed, expected, "replace {replace}");
        assert_eq!(
            std::fs::read_to_string(&library_file).expect("library written"),
            dc_library::save(&merged.library).unwrap(),
            "replace {replace}: the library file"
        );
        lib = merged.library;
    }
}

/// Acceptance 71: the tool the app adds from the library is the one
/// `tools add` writes, so the two project files are byte-identical.
#[test]
fn tools_add_equals_the_app() {
    let dir = work_dir("tools_add_equals_the_app");
    std::fs::copy(
        repo_root().join("fixtures/coaster.svg"),
        dir.join("coaster.svg"),
    )
    .expect("svg copied");
    run(&dir, &["init", "coaster.svg"]);
    let sample = repo_root().join("tests/fixtures/fusion/sample.tools");
    run(
        &dir,
        &["tools", "import", sample.to_str().expect("utf-8 path")],
    );
    let dcv = dir.join("coaster.dcv");
    let before = std::fs::read(&dcv).expect("dcv readable");

    run(
        &dir,
        &["tools", "add", "coaster.dcv", "1/4 in Flat End Mill"],
    );
    run(&dir, &["tools", "add", "coaster.dcv", "5 mm Drill"]);
    let cli = std::fs::read_to_string(&dcv).expect("dcv readable");

    let lib = dc_library::load(&std::fs::read(config_dir(&dir).join("tools.json")).unwrap())
        .expect("library loads");
    let mut harness = Harness::open(&before).expect("the project opens");
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        let entry = dc_library::find(&lib, name).expect("in the library");
        let selected = BTreeSet::from([entry.id.clone()]);
        let command = dc_app::tools::add_from_library(harness.app.project(), &lib, &selected)
            .expect("something to add");
        harness.run(command).expect("added");
    }
    assert_eq!(harness.save().expect("saves"), cli);
}

/// The date the tab test's programs are headed with: the harness's.
const DATE: &str = dc_app::test_support::DATE;

/// `init`'s coaster in `dir` with the quick start's numbers filled in
/// (the phase 1 test part's feeds, an 11.8 mm thickness and a depth
/// through it), so it exports: `coaster.dcv`, with its tab request.
fn ready_coaster(dir: &Path) -> PathBuf {
    std::fs::copy(
        repo_root().join("fixtures/coaster.svg"),
        dir.join("coaster.svg"),
    )
    .expect("svg copied");
    run(dir, &["init", "coaster.svg"]);
    let dcv = dir.join("coaster.dcv");
    let mut project = dc_project::load(&std::fs::read(&dcv).expect("dcv readable"))
        .expect("dcv loads")
        .project;
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.rpm = Some(18000);
        tool.step_down_mm = Some(2.0);
    }
    project.stock.thickness_mm = Some(11.8);
    for op in &mut project.operations {
        if let dc_project::Operation::Profile(p) = op {
            p.depth_mm = Some(12.0);
            p.through_cut_allowance_mm = 0.2;
        }
    }
    std::fs::write(&dcv, dc_project::save(&project).expect("saves")).expect("dcv written");
    dcv
}

/// The one program the harness finished, as text.
fn app_program(harness: &mut Harness) -> String {
    harness.generate();
    let programs = harness.programs();
    assert_eq!(programs.len(), 1, "{:?}", harness.warnings());
    programs[0].program.text.clone()
}

/// Acceptance 78 (functional capability 69; #546): *Place tabs* on the
/// coaster's Profile saves the file `tabs place coaster.dcv <op>` saves,
/// and the program the app finishes is the one `export` writes for it.
/// Then a tab dragged along the outline, one deleted and one added with
/// the Tab tool: the CLI exports the file the app saves to the program
/// the app shows, byte for byte.
#[test]
fn tabbed_coaster_exports_the_same_program() {
    let dir = work_dir("tabbed_coaster_exports_the_same_program");
    let dcv = ready_coaster(&dir);
    let before = std::fs::read(&dcv).expect("dcv readable");

    let mut harness = Harness::open(&before).expect("the project opens");
    let op = harness.app.project().operations[0].id().clone();
    run(&dir, &["tabs", "place", "coaster.dcv", op.as_str()]);
    harness.app.place_tabs(&op).expect("the request places");
    assert_eq!(
        harness.save().expect("saves"),
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        "the file `tabs place` saves"
    );
    run(&dir, &["export", "coaster.dcv", "--date", DATE]);
    let cli = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    assert!(
        app_program(&mut harness) == cli,
        "placed: the program differs"
    );

    // Drag the first tab an eighth of the way round, delete the second,
    // add one with the Tab tool's last-used size.
    let project = harness.app.project().clone();
    let first = project.tabs[0].clone();
    let instance = &project.instances[0];
    let outline = &project.definitions[&instance.definition].profiles[0];
    let (target, _) = outline
        .point_at(outline.length_mm() * 0.125)
        .expect("a point");
    let (x_mm, y_mm) =
        dc_app::viewport::tabs::tab_drag(&project, &first, instance.transform.apply(target));
    harness
        .run(dc_app::Command::MoveTab {
            id: first.id.clone(),
            x_mm,
            y_mm,
        })
        .expect("moved");
    harness
        .run(dc_app::Command::DeleteTabs(vec![
            project.tabs[1].id.clone(),
        ]))
        .expect("deleted");
    let (at_mm, _) = outline
        .point_at(outline.length_mm() * 0.7)
        .expect("a point");
    let add = dc_app::viewport::tabs::add_tab(
        harness.app.project(),
        &dc_app::viewport::tabs::OutlinePoint {
            instance: instance.id.clone(),
            profile: 0,
            at_mm,
        },
        harness.app.prefs().tab,
    );
    harness.run(add).expect("added");
    assert_eq!(harness.app.project().tabs.len(), 4);

    std::fs::write(dir.join("edited.dcv"), harness.save().expect("saves")).expect("written");
    run(&dir, &["export", "edited.dcv", "--date", DATE]);
    let cli = std::fs::read_to_string(dir.join("edited.nc")).expect("program written");
    assert!(
        app_program(&mut harness) == cli,
        "edited: the program differs"
    );
}
