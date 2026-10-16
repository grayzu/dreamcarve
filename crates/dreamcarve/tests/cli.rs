// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Behaviour tests for the `dreamcarve` binary: `--version` (phase 0,
//! capability 1) and `init`, `export`, `check`, `posts`
//! (`docs/specs/phase-1/functional.md`, capabilities 1–5, acceptance tests 1
//! and 2; `docs/specs/phase-1/design.md`, "CLI").
//!
//! Each test works in its own directory under `CARGO_TARGET_TMPDIR`.
//! Programs are compared with a fixed date passed through the hidden
//! `export --date` option, so no test depends on today's date.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken setup"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dc_geometry::assert_approx_eq;
use dc_project::{Loaded, load, save};

/// The date every compared program is exported with.
const FIXED_DATE: &str = "2026-10-15";

fn dreamcarve() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
}

/// Runs `dreamcarve` with `args` in `dir`.
fn run(dir: &Path, args: &[&str]) -> Output {
    dreamcarve()
        .current_dir(dir)
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

/// A fresh, empty working directory for test `name`.
fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("old work dir removed");
    }
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

/// The repository's `fixtures/` directory.
fn repo_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Copies `fixtures/coaster.svg` into `dir` and runs `init` on it.
fn init_coaster(dir: &Path) -> PathBuf {
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let out = run(dir, &["init", "coaster.svg"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "init failed: stderr was {}",
        stderr(&out)
    );
    dir.join("coaster.dcv")
}

fn load_file(path: &Path) -> Loaded {
    load(&std::fs::read(path).expect("dcv readable")).expect("dcv loads")
}

/// Edits the `.dcv` at `path` as a user would: every tool gets `feed`,
/// `plunge`, and `rpm`.
fn set_rates(path: &Path, feed_mm_min: f64, plunge_mm_min: f64, rpm: u32) {
    let mut project = load_file(path).project;
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(feed_mm_min);
        tool.plunge_mm_min = Some(plunge_mm_min);
        tool.rpm = Some(rpm);
    }
    std::fs::write(path, save(&project).expect("project saves")).expect("dcv written");
}

#[test]
fn version_flag_prints_name_and_cargo_version() {
    let out = dreamcarve().arg("--version").output().expect("binary runs");
    assert!(out.status.success(), "exit status: {:?}", out.status);
    assert_eq!(
        stdout(&out).trim(),
        format!("dreamcarve {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_arguments_prints_usage_and_fails() {
    let out = dreamcarve().output().expect("binary runs");
    assert_eq!(out.status.code(), Some(2), "no subcommand is a usage error");
    let stderr = stderr(&out);
    assert!(stderr.contains("Usage"), "stderr was: {stderr}");
}

/// End to end on the phase 1 test part: `init` on `fixtures/coaster.svg`,
/// the feeds from the functional spec's "Test part" table filled in (1500,
/// 500, 18000), then `export`. The program must match the committed
/// `tests/expected/coaster-grblhal.nc` byte for byte. Regenerate that file
/// only when the change to it is intended, and say why in the commit.
#[test]
fn init_then_export_coaster_matches_committed_program() {
    let dir = work_dir("init_then_export_coaster");
    let dcv = init_coaster(&dir);
    set_rates(&dcv, 1500.0, 500.0, 18000);

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "export failed: stderr was {}",
        stderr(&out)
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    let expected = include_str!("expected/coaster-grblhal.nc");
    assert!(
        program == expected,
        "exported program differs from tests/expected/coaster-grblhal.nc:\n{program}"
    );
}

/// Acceptance test 1. The coaster is a 90 mm circle centred on a 100 mm
/// page, so its one profile spans 5..95 mm on both axes in SVG page space
/// (Y flipped, which a centred circle does not change). The stock is that
/// span plus the 10 mm `init` margin on each side: 110 × 110 × 12 mm.
#[test]
fn init_round_trip() {
    let dir = work_dir("init_round_trip");
    let dcv = init_coaster(&dir);
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    let loaded = load(text.as_bytes()).expect("dcv loads");
    assert_eq!(
        save(&loaded.project).expect("project saves"),
        text,
        "the .dcv init writes must load back identically"
    );

    let project = loaded.project;
    assert_eq!(project.definitions.len(), 1, "one definition");
    let definition = project.definitions.values().next().expect("a definition");
    assert_eq!(definition.profiles.len(), 1, "one profile");
    let profile = &definition.profiles[0];
    assert!(profile.is_closed(), "the circle is a closed profile");
    let bounds = profile.bounds().expect("profile has vertices");
    // Flattened vertices lie on the circle, so the bounds may fall short of
    // the true extremes by up to the flatten tolerance.
    let tol = 2.0 * project.flatten_tolerance_mm;
    assert_approx_eq!(bounds.min.x, 5.0, tol);
    assert_approx_eq!(bounds.min.y, 5.0, tol);
    assert_approx_eq!(bounds.max.x, 95.0, tol);
    assert_approx_eq!(bounds.max.y, 95.0, tol);
    assert_approx_eq!(project.stock.width_mm, 110.0, tol);
    assert_approx_eq!(project.stock.height_mm, 110.0, tol);
    assert_approx_eq!(project.stock.thickness_mm, 12.0, 0.0);
}

/// Acceptance test 2 and S3: a tool with `feed_mm_min: 0` is refused. The
/// tool is identified by its id (`t1`, as `init` writes it) and the field by
/// its `.dcv` key, so the user can find both in the file.
#[test]
fn export_refuses_zero_feed_and_writes_nothing() {
    let dir = work_dir("export_refuses_zero_feed");
    let dcv = init_coaster(&dir);
    set_rates(&dcv, 0.0, 500.0, 18000);
    let before = std::fs::read(&dcv).expect("dcv readable");

    let out = run(&dir, &["export", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(1), "a validation error exits 1");
    let stderr = stderr(&out);
    assert!(stderr.contains("t1"), "stderr names the tool: {stderr}");
    assert!(
        stderr.contains("feed_mm_min"),
        "stderr names the field: {stderr}"
    );
    assert!(!dir.join("coaster.nc").exists(), "no program is written");
    let names: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir readable")
        .map(|e| e.expect("dir entry").file_name())
        .collect();
    assert_eq!(
        names.len(),
        2,
        "only coaster.svg and coaster.dcv remain, no temporary file: {names:?}"
    );
    assert_eq!(
        std::fs::read(&dcv).expect("dcv readable"),
        before,
        "the .dcv is untouched"
    );
}

/// Capability 8: an open path is imported but not targeted, and `check`
/// reports it by name. With rates set there is nothing else wrong, so
/// `check` exits 0: an open path is a warning, not an error.
#[test]
fn check_reports_open_paths() {
    let dir = work_dir("check_reports_open_paths");
    std::fs::write(
        dir.join("part.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <rect id="outline" x="10" y="10" width="60" height="40"/>
  <path id="guide-line" d="M 20 80 L 80 80"/>
</svg>
"#,
    )
    .expect("svg written");
    let out = run(&dir, &["init", "part.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    set_rates(&dir.join("part.dcv"), 1500.0, 500.0, 18000);

    let out = run(&dir, &["check", "part.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "check output: {report}");
    let line = report
        .lines()
        .find(|l| l.contains("guide-line"))
        .unwrap_or_else(|| panic!("check names the open path: {report}"));
    assert!(line.contains("warning"), "reported as a warning: {line}");
    assert!(line.contains("open"), "says the path is open: {line}");
    assert!(
        !report.contains("outline"),
        "the closed rectangle is not reported: {report}"
    );
}

/// Capability 5: both bundled post-processors are listed by id, grblHAL
/// first, as `dc_gcode::post::bundled` orders them.
#[test]
fn posts_lists_grblhal_and_grbl() {
    let out = dreamcarve().arg("posts").output().expect("binary runs");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let stdout = stdout(&out);
    let ids: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(ids, ["grblhal", "grbl"], "stdout was: {stdout}");
}

/// A `.dcv` embeds its post-processor, so a shared project chooses the
/// output extension. An extension that is a path (`../x`) or names the
/// project file itself (`dcv`) must not let `export` write outside the
/// project's folder or overwrite the project: export fails and writes
/// nothing.
#[test]
fn export_refuses_unsafe_file_extension() {
    for (case, extension) in [
        ("dcv", "dcv"),
        ("traversal", "/../../escaped"),
        ("empty", ""),
    ] {
        let dir = work_dir(&format!("export_refuses_extension_{case}"));
        let dcv = init_coaster(&dir);
        set_rates(&dcv, 1500.0, 500.0, 18000);
        let mut project = load_file(&dcv).project;
        project.post_processor.file_extension = extension.into();
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
        let before = std::fs::read(&dcv).expect("dcv readable");

        let out = run(&dir, &["export", "coaster.dcv"]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{case}: exits 1: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("file extension"),
            "{case}: stderr names the extension: {}",
            stderr(&out)
        );
        assert_eq!(
            std::fs::read(&dcv).expect("dcv readable"),
            before,
            "{case}: .dcv untouched"
        );
        let names: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir readable")
            .map(|e| e.expect("dir entry").file_name())
            .collect();
        assert_eq!(names.len(), 2, "{case}: nothing new written: {names:?}");
        assert!(
            !dir.parent().unwrap().join("escaped").exists(),
            "{case}: nothing escaped"
        );
    }
}

/// `--output` naming the project file would replace the project with
/// G-code; export refuses it.
#[test]
fn export_refuses_to_overwrite_the_project() {
    let dir = work_dir("export_refuses_overwrite_project");
    let dcv = init_coaster(&dir);
    set_rates(&dcv, 1500.0, 500.0, 18000);
    let before = std::fs::read(&dcv).expect("dcv readable");

    let out = run(
        &dir,
        &["export", "coaster.dcv", "--output", "./coaster.dcv"],
    );
    assert_eq!(out.status.code(), Some(1), "exits 1: {}", stderr(&out));
    assert_eq!(
        std::fs::read(&dcv).expect("dcv readable"),
        before,
        ".dcv untouched"
    );
}
