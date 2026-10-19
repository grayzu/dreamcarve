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

use dc_geometry::{Rect, assert_approx_eq};

#[path = "../src/clock.rs"]
#[allow(dead_code, reason = "the test uses only the timestamp")]
mod clock;
use dc_project::{
    Loaded, Operation, OperationId, SpindleControl, ToolChangeMode, ToolId, load, save,
};

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

/// The quick start's measured plywood thickness, mm.
const MEASURED_THICKNESS_MM: f64 = 11.8;
/// The functional spec's test-part through-cut allowance, mm.
const THROUGH_CUT_ALLOWANCE_MM: f64 = 0.2;

/// Edits the `.dcv` at `path` as the quick start does: every tool gets
/// `feed`, `plunge`, and `rpm`; the stock gets the measured thickness; and
/// every Profile cuts through it by the test part's 0.2 mm allowance.
fn fill_in(path: &Path, feed_mm_min: f64, plunge_mm_min: f64, rpm: u32) {
    let mut project = load_file(path).project;
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(feed_mm_min);
        tool.plunge_mm_min = Some(plunge_mm_min);
        tool.rpm = Some(rpm);
    }
    project.stock.thickness_mm = Some(MEASURED_THICKNESS_MM);
    for dc_project::Operation::Profile(op) in &mut project.operations {
        op.depth_mm = Some(MEASURED_THICKNESS_MM + THROUGH_CUT_ALLOWANCE_MM);
        op.through_cut_allowance_mm = THROUGH_CUT_ALLOWANCE_MM;
    }
    std::fs::write(path, save(&project).expect("project saves")).expect("dcv written");
}

/// Runs `git args` in this crate's directory; `None` when git is missing or
/// fails.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// `--version` marks every build but the release tag's as work toward the
/// release, `<version>-dev.<UTC build time>+g<hash>`, so later builds sort
/// higher (`docs/plan.md`, "Versioning").
#[test]
fn version_flag_prints_dev_build_time_and_commit_hash() {
    let out = dreamcarve().arg("--version").output().expect("binary runs");
    assert!(out.status.success(), "exit status: {:?}", out.status);
    let printed = stdout(&out).trim().to_owned();
    let version = env!("CARGO_PKG_VERSION");
    let tag = format!("v{version}");
    let github_tag = std::env::var("GITHUB_REF_TYPE").is_ok_and(|t| t == "tag")
        && std::env::var("GITHUB_REF_NAME").is_ok_and(|n| n == tag);
    let git_tag = git(&["tag", "--points-at", "HEAD"]).is_some_and(|t| t.lines().any(|l| l == tag));
    if github_tag || git_tag {
        assert_eq!(printed, format!("dreamcarve {version}"));
        return;
    }
    let rest = printed
        .strip_prefix(&format!("dreamcarve {version}-dev."))
        .unwrap_or_else(|| panic!("not a dev version: {printed}"));
    let (built, hash) = match rest.split_once("+g") {
        Some((built, hash)) => (built, Some(hash)),
        None => (rest, None),
    };
    assert_eq!(hash, git(&["rev-parse", "--short=7", "HEAD"]).as_deref());
    assert_eq!(built.len(), 14, "YYYYMMDDHHMMSS: {built}");
    assert!(built.bytes().all(|b| b.is_ascii_digit()), "digits: {built}");
    assert!(
        built <= clock::timestamp_utc(clock::now_secs()).as_str(),
        "built in the future: {built}"
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
/// 500, 18000), the quick start's measured 11.8 mm thickness, and a depth
/// through it by the table's 0.2 mm allowance, then `export`. The program must match the committed
/// `tests/expected/coaster-grblhal.nc` byte for byte. Regenerate that file
/// only when the change to it is intended, and say why in the commit.
#[test]
fn init_then_export_coaster_matches_committed_program() {
    let dir = work_dir("init_then_export_coaster");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);

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
/// span plus the 10 mm `init` margin on each side: 110 × 110 mm.
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
}

/// #64: `init` cannot know the material, so like the feeds it leaves the
/// stock thickness and the Profile depth unset (`null` in the file) rather
/// than default them. A default thickness that is wrong moves every pass
/// and, with Z zero at the stock bottom, safe Z itself.
#[test]
fn init_project_has_no_thickness_or_depth() {
    let dir = work_dir("init_project_has_no_thickness_or_depth");
    let dcv = init_coaster(&dir);
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    assert!(text.contains("\"thickness_mm\": null"), "{text}");
    assert!(text.contains("\"depth_mm\": null"), "{text}");
    let project = load_file(&dcv).project;
    assert!(project.stock.thickness_mm.is_none(), "thickness is unset");
    for dc_project::Operation::Profile(op) in &project.operations {
        assert!(op.depth_mm.is_none(), "{} depth is unset", op.id);
    }
}

/// The fields a fresh coaster project leaves unset, in file order, with
/// the key each one's line holds.
const UNSET_FIELDS: [(&str, &str); 5] = [
    ("stock.thickness_mm", "thickness_mm"),
    ("tools[t1].feed_mm_min", "feed_mm_min"),
    ("tools[t1].plunge_mm_min", "plunge_mm_min"),
    ("tools[t1].rpm", "rpm"),
    ("operations[op1].depth_mm", "depth_mm"),
];

/// The lines of `output` that start `file:line:column: ` and name a field,
/// as `(line, column, rest)`.
fn located_lines<'a>(output: &'a str, file: &str) -> Vec<(usize, usize, &'a str)> {
    output
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix(file)?.strip_prefix(':')?;
            let (line, rest) = rest.split_once(':')?;
            let (column, rest) = rest.split_once(": ")?;
            Some((line.parse().ok()?, column.parse().ok()?, rest))
        })
        .collect()
}

/// Asserts that line `line`, column `column` of `text` is the `null` value
/// of `key`.
fn assert_points_at_null(text: &str, line: usize, column: usize, key: &str) {
    let source = text.lines().nth(line - 1).expect("line exists");
    let before: String = source.chars().take(column - 1).collect();
    let at: String = source.chars().skip(column - 1).collect();
    assert!(at.starts_with("null"), "{line}:{column} is `{at}`");
    assert!(
        before.trim_end().ends_with(&format!("\"{key}\":")),
        "{line}:{column} is the value of {key}: `{source}`"
    );
}

/// #76: `init` lists every value the user must set, one per line, as
/// `coaster.dcv:line:column:`, and each position is that value's `null` in
/// the file written. Thickness and depth (#64) come with the tool's feed,
/// plunge rate and RPM.
#[test]
fn init_lists_every_required_value_with_location() {
    let dir = work_dir("init_lists_every_required_value_with_location");
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let out = run(&dir, &["init", "coaster.svg"]);
    assert_eq!(out.status.code(), Some(0), "stderr was {}", stderr(&out));
    let stdout = stdout(&out);
    let text = std::fs::read_to_string(dir.join("coaster.dcv")).expect("dcv readable");
    let located = located_lines(&stdout, "coaster.dcv");
    assert_eq!(located.len(), UNSET_FIELDS.len(), "{stdout}");
    for ((line, column, message), (field, key)) in located.iter().zip(UNSET_FIELDS) {
        assert!(
            message.starts_with(&format!("{field} is not set")),
            "{message}"
        );
        assert_points_at_null(&text, *line, *column, key);
    }
    let next = stdout
        .lines()
        .find(|l| l.starts_with("Next:"))
        .unwrap_or_else(|| panic!("init prints a Next: line: {stdout}"));
    assert!(next.contains("listed above"), "{next}");
}

/// #76: `check` on a fresh project reports every missing value in one run,
/// each as an error located at its `null`, and exits 1.
#[test]
fn check_reports_all_missing_values_at_once() {
    let dir = work_dir("check_reports_all_missing_values_at_once");
    let dcv = init_coaster(&dir);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let stdout = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "exits 1: {stdout}");
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    let located = located_lines(&stdout, "coaster.dcv");
    assert_eq!(located.len(), UNSET_FIELDS.len(), "{stdout}");
    for ((line, column, message), (field, key)) in located.iter().zip(UNSET_FIELDS) {
        assert!(
            message.starts_with(&format!("error: {field} is not set")),
            "{message}"
        );
        assert_points_at_null(&text, *line, *column, key);
    }
    assert!(stdout.contains("5 error(s), 0 warning(s)"), "{stdout}");
}

/// #76: the locations are found in the file as the user has it, here
/// re-ordered and re-indented by hand, not where `init` put the values.
#[test]
fn check_locates_values_in_a_reformatted_file() {
    let dir = work_dir("check_locates_values_in_a_reformatted_file");
    let dcv = init_coaster(&dir);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dcv).expect("dcv readable"))
            .expect("dcv is JSON");
    let serde_json::Value::Object(map) = value else {
        panic!("root is an object")
    };
    let reversed: serde_json::Map<_, _> = map.into_iter().rev().collect();
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(reversed))
        .expect("serializes")
        .replace("  ", "\t");
    std::fs::write(&dcv, &text).expect("dcv written");
    let out = run(&dir, &["check", "coaster.dcv"]);
    let stdout = stdout(&out);
    let located = located_lines(&stdout, "coaster.dcv");
    assert_eq!(located.len(), UNSET_FIELDS.len(), "{stdout}");
    for (line, column, message) in located {
        let (_, key) = UNSET_FIELDS
            .iter()
            .find(|(field, _)| message.starts_with(&format!("error: {field} ")))
            .unwrap_or_else(|| panic!("{message}"));
        assert_points_at_null(&text, line, column, key);
    }
}

/// #64, #76: exporting a fresh project reports the unset thickness, depth
/// and feeds as errors in the same run, so one edit fixes them all.
/// Nothing is written.
#[test]
fn export_of_fresh_project_reports_thickness_depth_and_feeds() {
    let dir = work_dir("export_of_fresh_project_reports_all_unset");
    init_coaster(&dir);
    let out = run(&dir, &["export", "coaster.dcv"]);
    let stderr = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "exits 1: {stderr}");
    let located = located_lines(&stderr, "coaster.dcv");
    assert_eq!(located.len(), UNSET_FIELDS.len(), "{stderr}");
    for ((_, _, message), (field, _)) in located.iter().zip(UNSET_FIELDS) {
        assert!(
            message.starts_with(&format!("error: {field} is not set")),
            "{message}"
        );
    }
    assert!(!dir.join("coaster.nc").exists(), "no program is written");
}

/// True when `value_mm` is a multiple of 0.01 mm, within 1e-9.
fn is_hundredth(value_mm: f64) -> bool {
    ((value_mm * 100.0).round() / 100.0 - value_mm).abs() <= 1e-9
}

/// Issue #44: the imported outline's bounds carry float noise, so `init`
/// rounds the stock size up and the instance offset to 0.01 mm, keeping
/// the drawing at least the 10 mm margin in from every stock edge. The
/// margin is checked to 0.001 mm, the resolution programs are written with:
/// noise below that (the coaster's circle reaches 4.9999981 mm) is not worth
/// a 0.01 mm larger stock or a shifted drawing.
#[test]
fn init_writes_round_numbers() {
    let dir = work_dir("init_writes_round_numbers");
    let project = load_file(&init_coaster(&dir)).project;

    let stock = &project.stock;
    assert!(is_hundredth(stock.width_mm), "width {}", stock.width_mm);
    assert!(is_hundredth(stock.height_mm), "height {}", stock.height_mm);

    let mut drawing: Option<Rect> = None;
    for instance in &project.instances {
        let [_, _, _, _, e, f]: [f64; 6] = instance.transform.into();
        assert!(is_hundredth(e), "instance offset x {e}");
        assert!(is_hundredth(f), "instance offset y {f}");
        let definition = project
            .definitions
            .get(&instance.definition)
            .expect("instance's definition exists");
        for profile in &definition.profiles {
            let placed = instance
                .transform
                .apply_polyline(profile)
                .expect("transform applies");
            let bounds = placed.bounds().expect("profile has vertices");
            drawing = Some(match drawing {
                None => bounds,
                Some(r) => r.including(bounds.min).including(bounds.max),
            });
        }
    }
    let drawing = drawing.expect("the coaster has a profile");
    let margin_mm = 10.0 - 0.001;
    assert!(drawing.min.x >= margin_mm, "left margin {}", drawing.min.x);
    assert!(
        drawing.min.y >= margin_mm,
        "bottom margin {}",
        drawing.min.y
    );
    assert!(
        stock.width_mm - drawing.max.x >= margin_mm,
        "right margin {}",
        stock.width_mm - drawing.max.x
    );
    assert!(
        stock.height_mm - drawing.max.y >= margin_mm,
        "top margin {}",
        stock.height_mm - drawing.max.y
    );
}

/// Acceptance test 2 and S3: a tool with `feed_mm_min: 0` is refused. The
/// tool is identified by its id (`t1`, as `init` writes it) and the field by
/// its `.dcv` key, so the user can find both in the file.
#[test]
fn export_refuses_zero_feed_and_writes_nothing() {
    let dir = work_dir("export_refuses_zero_feed");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 0.0, 500.0, 18000);
    let before = std::fs::read(&dcv).expect("dcv readable");

    let out = run(&dir, &["export", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(1), "a validation error exits 1");
    let stderr = stderr(&out);
    assert!(
        stderr.contains("Set me") && stderr.contains("t1"),
        "stderr names the tool by name and id: {stderr}"
    );
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

/// #67: the quick start's coaster, filled in as the chapter does (feed
/// 1500, plunge 500, 18000 RPM, `init`'s 4 mm step-down and 6.35 mm end
/// mill), passes `check` with no error and no warning.
#[test]
fn quick_start_project_has_no_sanity_warnings() {
    let dir = work_dir("quick_start_project_has_no_sanity_warnings");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);

    let out = run(&dir, &["check", "coaster.dcv"]);
    let stdout = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", stderr(&out));
    assert!(stdout.contains("0 error(s), 0 warning(s)"), "{stdout}");
}

/// #67 (maintainer comment): a feed far beyond any hobby machine fails
/// export, located at the value like every other error, and is reported
/// together with the other sanity findings in one run.
#[test]
fn export_refuses_feed_above_limit_with_location() {
    let dir = work_dir("export_refuses_feed_above_limit");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1e9, 500.0, 1_000_000);

    let out = run(&dir, &["export", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(1), "an error exits 1");
    let stderr = stderr(&out);
    assert!(
        stderr.contains("coaster.dcv:22:22: error: tool `Set me` (`t1`): feed_mm_min is 1000000000, expected a value greater than 0 and at most 20000 mm/min"),
        "{stderr}"
    );
    assert!(
        stderr.contains("coaster.dcv:24:14: warning: tool `Set me` (`t1`) is set to 1000000 RPM"),
        "{stderr}"
    );
    assert!(!dir.join("coaster.nc").exists(), "no program is written");
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
    fill_in(&dir.join("part.dcv"), 1500.0, 500.0, 18000);

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

/// Issue #58: a closed outline that crosses itself (the bowtie (0,0)
/// (60,40) (60,0) (0,30)) has no clear inside, so `check` reports it as an
/// error naming its instance and exits 1, and `export` writes nothing.
#[test]
fn self_intersecting_target_fails_check() {
    let dir = work_dir("self_intersecting_target_fails_check");
    std::fs::write(
        dir.join("part.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <path id="bowtie" d="M 10 10 L 70 50 L 70 10 L 10 40 Z"/>
</svg>
"#,
    )
    .expect("svg written");
    let out = run(&dir, &["init", "part.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    fill_in(&dir.join("part.dcv"), 1500.0, 500.0, 18000);

    let out = run(&dir, &["check", "part.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(1), "check output: {report}");
    let line = report
        .lines()
        .find(|l| l.contains("bowtie"))
        .unwrap_or_else(|| panic!("check names the instance: {report}"));
    assert!(line.contains("error"), "reported as an error: {line}");
    assert!(line.contains("crosses itself"), "says why: {line}");

    let out = run(&dir, &["export", "part.dcv"]);
    assert_eq!(out.status.code(), Some(1), "export: {}", stderr(&out));
    assert!(
        stderr(&out).contains("bowtie"),
        "export names the instance: {}",
        stderr(&out)
    );
    assert!(!dir.join("part.nc").exists(), "no program is written");
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
        fill_in(&dcv, 1500.0, 500.0, 18000);
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
    fill_in(&dcv, 1500.0, 500.0, 18000);
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

/// #40: the extension rule is part of validation, so `check` reports an
/// unsafe extension as an error before any export is tried.
#[test]
fn check_reports_unsafe_file_extension() {
    let dir = work_dir("check_reports_unsafe_file_extension");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor.file_extension = "dcv".into();
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(1), "check output: {report}");
    assert!(
        report.contains("file extension"),
        "check names the file extension: {report}"
    );
}

/// The longest run of digits after a decimal point in `text`.
fn most_decimals(text: &str) -> usize {
    let mut most = 0;
    let mut run: Option<usize> = None;
    let mut prev_digit = false;
    for c in text.chars() {
        match (c, run) {
            ('.', _) if prev_digit => run = Some(0),
            (d, Some(n)) if d.is_ascii_digit() => {
                run = Some(n + 1);
                most = most.max(n + 1);
            }
            _ => run = None,
        }
        prev_digit = c.is_ascii_digit();
    }
    most
}

/// Every warning `check` and `export` print is a plain sentence: no
/// `key=value` pairs, and no value with more than 3 decimals. The project
/// raises every toolpath warning: an open path, a duplicate shape, a hole
/// the bit does not fit, tabs that overlap and cover most of a hole, and a
/// tab top above the stock top.
#[test]
fn warnings_are_readable() {
    let dir = work_dir("warnings_are_readable");
    std::fs::write(
        dir.join("part.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <rect id="outline" x="10" y="10" width="60" height="40"/>
  <rect id="outline-copy" x="10" y="10" width="60" height="40"/>
  <rect id="small-hole" x="15" y="15" width="20" height="20"/>
  <circle id="pin-hole" cx="55" cy="30" r="2"/>
  <path id="guide-line" d="M 20 80 L 80 80"/>
</svg>
"#,
    )
    .expect("svg written");
    let out = run(&dir, &["init", "part.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("part.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let thickness_mm = project.stock.thickness_mm.expect("fill_in sets it");
    for dc_project::Operation::Profile(op) in &mut project.operations {
        if let Some(tabs) = &mut op.tabs {
            tabs.height_mm = thickness_mm + 1.0;
        }
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "part.dcv"]);
    let export = run(&dir, &["export", "part.dcv", "--date", FIXED_DATE]);
    assert_eq!(export.status.code(), Some(0), "export: {}", stderr(&export));
    let report = format!("{}{}{}", stdout(&check), stderr(&check), stderr(&export));
    let warnings: Vec<&str> = report
        .lines()
        .filter(|l| l.starts_with("warning:") || l.contains(": warning: "))
        .collect();
    for needle in [
        "open path",
        "same outline",
        "does not fit",
        "overlap",
        "may not cut free",
        "stock top",
    ] {
        assert!(
            warnings.iter().any(|w| w.contains(needle)),
            "a warning says {needle:?}: {report}"
        );
    }
    for line in warnings {
        assert!(!line.contains('='), "no key=value: {line}");
        assert!(most_decimals(line) <= 3, "rounded: {line}");
    }
}

/// #59: a finite but huge transform used to pass validation and panic
/// inside the offset library. `check` now reports it as an error naming the
/// instance's transform and exits 1, for a huge scale and a huge
/// translation alike.
#[test]
fn check_rejects_huge_transform_without_panic() {
    let dir = work_dir("check_rejects_huge_transform");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let pristine = load_file(&dcv).project;
    for transform in [
        [1e15, 0.0, 0.0, 1e15, 0.0, 0.0],
        [1.0, 0.0, 0.0, 1.0, 1e12, 0.0],
    ] {
        let mut project = pristine.clone();
        project.instances[0].transform = transform.into();
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

        let out = run(&dir, &["check", "coaster.dcv"]);
        let report = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(out.status.code(), Some(1), "{transform:?}: {report}");
        let id = &project.instances[0].id;
        assert!(
            report.contains(&format!("instances[{id}].transform")),
            "{transform:?}: check names the transform: {report}"
        );
    }
}

/// #59: `init` adds a 10 mm margin around the drawing, so a drawing that
/// imports within the 6000 mm limit can still need a larger stock. `init`
/// refuses it and names the limit rather than write a project that `check`
/// would reject.
#[test]
fn init_refuses_drawing_too_large_for_stock_limit() {
    let dir = work_dir("init_refuses_drawing_too_large");
    std::fs::write(
        dir.join("big.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="5990mm" height="100mm" viewBox="0 0 5990 100">
  <rect id="long" x="0" y="10" width="5990" height="40"/>
</svg>
"#,
    )
    .expect("svg written");
    let out = run(&dir, &["init", "big.svg"]);
    let report = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "init: {report}");
    assert!(report.contains("6000"), "names the limit: {report}");
    assert!(!dir.join("big.dcv").exists(), "no project is written");
}

/// The text of `line`'s comment without a leading `MSG,`: what the user is
/// asked to do at a pause.
fn pause_message(line: &str) -> String {
    let comment = match (line.find('('), line.rfind(')')) {
        (Some(open), Some(close)) if open < close => &line[open + 1..close],
        _ => "",
    };
    let comment = comment.trim();
    comment
        .strip_prefix("MSG,")
        .unwrap_or(comment)
        .trim()
        .to_owned()
}

/// Asserts that `summary` lists every `M0` and `M6` of `program` by its
/// 1-based line number together with its message.
fn assert_lists_pauses(program: &str, summary: &str) {
    let pauses: Vec<(usize, &str)> = program
        .lines()
        .enumerate()
        .filter(|(_, l)| l.starts_with("M0") || l.split_whitespace().any(|w| w == "M6"))
        .map(|(i, l)| (i + 1, l))
        .collect();
    assert!(!pauses.is_empty(), "the program pauses at least once");
    for (number, line) in pauses {
        let tag = format!("line {number}:");
        let listed = summary
            .lines()
            .find(|l| l.contains(&tag))
            .unwrap_or_else(|| panic!("summary lists the M0 on line {number}: {summary}"));
        let message = pause_message(line);
        assert!(
            listed.contains(&message),
            "line {number} is listed with `{message}`: {listed}"
        );
    }
}

/// Design step 8 (#40): the controller or sender may not show a pause's
/// message (GRBL drops comments), so `export` lists every `M0` with its line
/// number and what it is for. #57: the coaster has one tool, so with
/// automatic spindle control it does not stop at all.
#[test]
fn export_lists_each_pause() {
    let dir = work_dir("export_lists_each_pause");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    let summary = stdout(&out);
    assert!(
        !program.lines().any(|l| l.starts_with("M0")),
        "no pause: {program}"
    );
    assert!(!summary.contains("Pauses"), "no pause listed: {summary}");

    let mut project = load_file(&dcv).project;
    project.post_processor.feed_speed.spindle_control = SpindleControl::Manual;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "manual export: {}",
        stderr(&out)
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    let summary = stdout(&out);
    assert_lists_pauses(&program, &summary);
    assert!(
        summary.contains("Turn the router ON") && summary.contains("Turn the router OFF"),
        "both manual pauses are listed: {summary}"
    );
}

/// #57: the program does not stop for the first tool. The user fits it and
/// sets work zero with it before starting, so `export` says so, with the
/// tool's number and name, in both spindle modes.
#[test]
fn export_tells_user_to_zero_before_start() {
    let dir = work_dir("export_tells_user_to_zero_before_start");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    for control in [SpindleControl::Automatic, SpindleControl::Manual] {
        let mut project = load_file(&dcv).project;
        project.post_processor.feed_speed.spindle_control = control;
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
        let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{control:?} export: {}",
            stderr(&out)
        );
        let summary = stdout(&out);
        assert!(
            summary.contains("Before you start: fit T1 Set me and set work zero with it."),
            "{control:?}: {summary}"
        );
    }
}

/// PR #84: a second tool that no enabled operation uses, with every rate
/// `null`, is one located warning, not an error, and the project still
/// exports, cutting only with the tool in use.
#[test]
fn unused_tool_with_unset_values_is_a_warning_not_an_error() {
    let dir = work_dir("unused_tool_with_unset_values_is_a_warning_not_an_error");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let mut spare = project.tools[0].clone();
    spare.id = dc_project::ToolId::new("t2");
    spare.name = "Spare bit".into();
    spare.feed_mm_min = None;
    spare.plunge_mm_min = None;
    spare.rpm = None;
    project.tools.push(spare);
    let text = save(&project).expect("project saves");
    std::fs::write(&dcv, &text).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let stdout = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "check passes: {stdout}");
    let located = located_lines(&stdout, "coaster.dcv");
    assert_eq!(located.len(), 1, "{stdout}");
    let (line, _, message) = located[0];
    assert_eq!(
        message,
        "warning: tool `t2` (`Spare bit`) is defined but not used by any enabled operation"
    );
    let next = text.lines().nth(line).expect("line after the tool");
    assert_eq!(next.trim(), "\"id\": \"t2\",", "located at the tool");
    assert!(stdout.contains("0 error(s), 1 warning(s)"), "{stdout}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "exports: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    assert!(!program.contains("T2"), "the unused tool is never fitted");
}

/// #61: `--post` replaces the post-processor, and the checks that depend
/// on it run again against the new one. A tool with no spindle speed is
/// fine while the router is switched by hand; switching to grblHAL, which
/// sets the speed with `S`, must report validation's missing-rpm problem,
/// fail, and write nothing.
#[test]
fn post_override_revalidates() {
    let dir = work_dir("post_override_revalidates");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor.feed_speed.spindle_control = SpindleControl::Manual;
    for tool in &mut project.tools {
        tool.rpm = None;
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "manual post passes: {report}");

    let before = std::fs::read(&dcv).expect("dcv readable");
    let out = run(&dir, &["export", "coaster.dcv", "--post", "grblhal"]);
    let stderr = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "exits 1: {stderr}");
    assert!(
        stderr.contains("tools[t1].rpm is not set (RPM) for tool `Set me`"),
        "validation of the new post names the missing rpm: {stderr}"
    );
    assert!(!dir.join("coaster.nc").exists(), "no program is written");
    assert_eq!(
        std::fs::read(&dcv).expect("dcv readable"),
        before,
        "the .dcv is untouched"
    );
}

/// #61, the reverse: a problem that only the embedded post-processor
/// causes (here an extension that would overwrite the project) fails
/// `check`, but `export --post grblhal` replaces that post and exports.
#[test]
fn post_override_to_valid_post_succeeds() {
    let dir = work_dir("post_override_to_valid_post_succeeds");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor.file_extension = "dcv".into();
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(1), "embedded post fails: {report}");

    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    assert!(dir.join("coaster.nc").exists(), "the program is written");
    let saved = load_file(&dcv);
    assert_eq!(saved.project.post_processor.file_extension, "nc");
    assert!(
        !saved.has_errors(),
        "the saved project is valid: {:?}",
        saved.problems
    );
}

/// The project-file chapter names the `.dcv` version this build writes and
/// lists that version in its Versions table, so the chapter cannot drift
/// from `dc_project::CURRENT_VERSION` again (issue #69).
#[test]
fn project_file_chapter_names_current_version() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/book/src/user-guide/project-file.md");
    let chapter = std::fs::read_to_string(&path).expect("project-file chapter reads");
    let version = dc_project::CURRENT_VERSION;

    let field_row = chapter
        .lines()
        .find(|line| line.starts_with("| `version` |"))
        .expect("the Top level table has a `version` row");
    assert!(
        field_row.contains(&format!("This build writes `{version}`.")),
        "the `version` row must say this build writes `{version}`: {field_row}"
    );

    let versions_row = format!("| {version} |");
    assert!(
        chapter.lines().any(|line| line.starts_with(&versions_row)),
        "the Versions table must have a row for version {version}"
    );
}

/// Writes the coaster at `dcv` back with its one instance's X translation,
/// transform component `e`, set to `e_mm`, as the project-file chapter
/// suggests for moving shapes. `init` sets it to 5.
fn set_coaster_e(dcv: &Path, e_mm: f64) {
    let mut project = load_file(dcv).project;
    let instance = project.instances.first_mut().expect("the coaster");
    let [a, b, c, d, _, f] = instance.transform.as_kurbo().as_coeffs();
    instance.transform = [a, b, c, d, e_mm, f].into();
    std::fs::write(dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// #62: the coaster's swept cut spans 3.65..106.35 mm on 110 mm stock.
/// With `e` = 80 (75 mm right) it runs 71.35 mm past the right (+X) edge;
/// `check` fails, naming the operation and the overhang, and `export`
/// writes nothing. So does a move just past the 3.65 mm margin.
#[test]
fn toolpath_off_stock_is_an_error() {
    let dir = work_dir("toolpath_off_stock_is_an_error");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);

    set_coaster_e(&dcv, 80.0);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(1), "check fails: {report}");
    let error = report
        .lines()
        .find(|l| l.starts_with("error:"))
        .unwrap_or_else(|| panic!("an error is reported: {report}"));
    assert!(error.contains("`op1`"), "names the operation: {error}");
    assert!(
        error.contains("71.35") && error.contains("+X"),
        "names the overhang and the side: {error}"
    );
    assert!(!error.contains("-X"), "only the right edge: {error}");

    let out = run(&dir, &["export", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(1), "export fails: {}", stderr(&out));
    assert!(!dir.join("coaster.nc").exists(), "no program is written");

    set_coaster_e(&dcv, 5.0 + 3.8);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(1), "0.15 mm over fails: {report}");
    assert!(report.contains("`op1`"), "{report}");
}

/// #62: the default coaster, and the coaster moved to within 0.15 mm of
/// either stock edge, stay on the stock.
#[test]
fn toolpath_just_inside_stock_passes() {
    let dir = work_dir("toolpath_just_inside_stock_passes");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    for dx_mm in [0.0, 3.5, -3.5] {
        set_coaster_e(&dcv, 5.0 + dx_mm);
        let out = run(&dir, &["check", "coaster.dcv"]);
        let report = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(out.status.code(), Some(0), "moved {dx_mm} mm: {report}");
        assert!(report.contains("0 error(s)"), "moved {dx_mm} mm: {report}");
    }
}

/// The lowest and highest value of axis word `letter` in `program`,
/// comments skipped.
fn word_range(program: &str, letter: char) -> (f64, f64) {
    let mut range = (f64::INFINITY, f64::NEG_INFINITY);
    for line in program.lines() {
        let code = line.split('(').next().unwrap_or_default();
        for word in code.split_whitespace() {
            if let Some(value) = word.strip_prefix(letter) {
                let value: f64 = value.parse().expect("axis word is a number");
                range = (range.0.min(value), range.1.max(value));
            }
        }
    }
    range
}

/// The `min..max` pair `summary` prints after `label` (`X`, `Y`, `Z`) on
/// its extents line.
fn printed_range(summary: &str, label: char) -> (f64, f64) {
    let line = summary
        .lines()
        .find(|l| l.contains("Extents"))
        .unwrap_or_else(|| panic!("an extents line is printed: {summary}"));
    let after = line
        .split(&format!("{label} "))
        .nth(1)
        .unwrap_or_else(|| panic!("{label} is printed: {line}"));
    let pair = after.split([',', ' ']).next().unwrap_or_default();
    let (min, max) = pair
        .split_once("..")
        .unwrap_or_else(|| panic!("{label} is min..max: {line}"));
    (
        min.parse().expect("min is a number"),
        max.parse().expect("max is a number"),
    )
}

/// #62: `export` and `check` print the program's extents in machine
/// coordinates, rounded as the program is. Z is exactly the range of the Z
/// words (Z min is the deepest cut). X and Y are the range of the X and Y
/// words, except that an arc may bulge past both its end words, so they may
/// reach further, by under 0.005 mm on the coaster.
#[test]
fn export_prints_extents() {
    let expected = include_str!("expected/coaster-grblhal.nc");
    let dir = work_dir("export_prints_extents");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let export = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(export.status.code(), Some(0), "export: {}", stderr(&export));
    let check = run(&dir, &["check", "coaster.dcv"]);
    assert_eq!(check.status.code(), Some(0), "check: {}", stdout(&check));
    for (command, out) in [("export", &export), ("check", &check)] {
        let summary = stdout(out);
        let (z_min, z_max) = printed_range(&summary, 'Z');
        let (word_min, word_max) = word_range(expected, 'Z');
        assert_approx_eq!(z_min, word_min, 0.0);
        assert_approx_eq!(z_max, word_max, 0.0);
        assert!(z_min < 0.0, "{command}: Z min is the deepest cut");
        for axis in ['X', 'Y'] {
            let (min, max) = printed_range(&summary, axis);
            let (word_min, word_max) = word_range(expected, axis);
            assert!(
                min <= word_min && min >= word_min - 0.005,
                "{command}: {axis} min {min} vs words {word_min}: {summary}"
            );
            assert!(
                max >= word_max && max <= word_max + 0.005,
                "{command}: {axis} max {max} vs words {word_max}: {summary}"
            );
        }
    }
}

/// The quick-start coaster, filled in, with a second tool `t2`,
/// `3mm finisher` (3.175 mm, 20000 RPM), and after `op1` a second Profile
/// `op2` that cuts the same circle with it (#95).
fn two_tool_coaster(dir: &Path) -> PathBuf {
    let dcv = init_coaster(dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let mut finisher = project.tools.first().expect("the coaster tool").clone();
    finisher.id = ToolId::new("t2");
    finisher.name = "3mm finisher".into();
    finisher.diameter_mm = 3.175;
    finisher.rpm = Some(20000);
    project.tools.push(finisher);
    let Operation::Profile(mut finish) = project
        .operations
        .first()
        .expect("the coaster profile")
        .clone();
    finish.id = OperationId::new("op2");
    finish.name = "Finish".into();
    finish.tool = ToolId::new("t2");
    // At most the 3.175 mm bit's diameter per pass, so the fixture has no
    // step-down warning (#67).
    finish.step_down_mm = Some(3.0);
    project.operations.push(Operation::Profile(finish));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// Whether `program` has a line with code word `word`, comments skipped.
fn has_code_word(program: &str, word: &str) -> bool {
    program.lines().any(|l| {
        l.split(['(', ';'])
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .any(|w| w == word)
    })
}

/// The spindle starts of `program`: its lines whose first word is `M3`.
fn spindle_starts(program: &str) -> Vec<&str> {
    program
        .lines()
        .filter(|l| l.split_whitespace().next() == Some("M3"))
        .collect()
}

/// Sets the tool-change mode of the `.dcv` at `dcv`.
fn set_tool_change_mode(dcv: &Path, mode: ToolChangeMode) {
    let mut project = load_file(dcv).project;
    project.post_processor.tool_change.mode = mode;
    std::fs::write(dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// #95: a project with two tools exports, by default, one complete program
/// per tool run, named `<stem>-<k>-T<n>.<ext>`, and no `coaster.nc`. Neither
/// program changes tools (no `M6`, no `M61`); each starts its own tool's
/// spindle once. The summary lists them in order with what to do before
/// each: the first sets work zero, the later one only Z zero.
#[test]
fn multi_tool_export_writes_one_file_per_tool_run() {
    let dir = work_dir("multi_tool_export_writes_one_file_per_tool_run");
    two_tool_coaster(&dir);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    assert!(!dir.join("coaster.nc").exists(), "no single program");
    for (name, start) in [
        ("coaster-1-T1.nc", "M3 S18000"),
        ("coaster-2-T2.nc", "M3 S20000"),
    ] {
        let program = std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{name} written: {e}"));
        for word in ["M6", "M61"] {
            assert!(!has_code_word(&program, word), "{name} has {word}");
        }
        assert_eq!(spindle_starts(&program), [start], "{name}");
    }
    let summary = stdout(&out);
    let expected = "Wrote 2 programs; run them in this order:\n  \
        1. coaster-1-T1.nc  Before you start: fit T1 Set me and set work zero (X, Y and Z) with it.\n  \
        2. coaster-2-T2.nc  Before you start: fit T2 3mm finisher and set Z zero with it. Keep X and Y zero.\n";
    assert!(summary.contains(expected), "{summary}");
}

/// #95: a project with one tool still writes the one `coaster.nc` it always
/// did, byte for byte the committed program.
#[test]
fn single_tool_export_is_unchanged() {
    let dir = work_dir("single_tool_export_is_unchanged");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(program == include_str!("expected/coaster-grblhal.nc"));
    assert!(!dir.join("coaster-1-T1.nc").exists());
    let summary = stdout(&out);
    assert!(summary.starts_with("Wrote coaster.nc ("), "{summary}");
}

/// The cutting moves (`G1`, `G2`, `G3`) of `program`, each as where it
/// goes, at what feed, and its arc centre: what the machine cuts.
fn cuts(program: &str) -> Vec<String> {
    dc_gcode::parse_motion(program)
        .expect("program parses")
        .into_iter()
        .filter(|m| m.kind.is_cut())
        .map(|m| format!("{:?} {:?} {:?} {:?}", m.kind, m.to, m.feed, m.arc))
        .collect()
}

/// #95: runs are consecutive operations with one tool, in operation order;
/// operations are never regrouped by tool, which would change the cut
/// order. T1, T2, T1 is three programs, and their cuts, in file order, are
/// the cuts of the one `"m6"` program.
#[test]
fn alternating_tools_keep_cut_order() {
    let dir = work_dir("alternating_tools_keep_cut_order");
    let dcv = two_tool_coaster(&dir);
    let mut project = load_file(&dcv).project;
    let Operation::Profile(mut again) = project.operations[0].clone();
    again.id = OperationId::new("op3");
    again.name = "Again".into();
    project.operations.push(Operation::Profile(again));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let mut separate = Vec::new();
    for name in ["coaster-1-T1.nc", "coaster-2-T2.nc", "coaster-3-T1.nc"] {
        let program = std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{name} written: {e}"));
        separate.extend(cuts(&program));
    }

    set_tool_change_mode(&dcv, ToolChangeMode::M6);
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "-o",
            "single.nc",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "m6 export: {}", stderr(&out));
    let single = std::fs::read_to_string(dir.join("single.nc")).expect("single.nc written");
    assert!(!separate.is_empty());
    assert_eq!(separate, cuts(&single));
}

/// #95: GRBL has no `M6`, but separate programs need none, so a two-tool
/// project now exports on GRBL.
#[test]
fn multi_tool_export_works_on_grbl() {
    let dir = work_dir("multi_tool_export_works_on_grbl");
    two_tool_coaster(&dir);
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grbl",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    for name in ["coaster-1-T1.nc", "coaster-2-T2.nc"] {
        assert!(dir.join(name).exists(), "{name} written");
    }
}

/// #95: `-o` names the stem, directory and extension of the programs.
#[test]
fn output_flag_sets_the_stem() {
    let dir = work_dir("output_flag_sets_the_stem");
    two_tool_coaster(&dir);
    std::fs::create_dir(dir.join("out")).expect("out dir created");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "-o",
            "out/job.nc",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    for name in ["out/job-1-T1.nc", "out/job-2-T2.nc"] {
        assert!(dir.join(name).exists(), "{name} written");
    }
    assert!(!dir.join("out/job.nc").exists());
    assert!(!dir.join("coaster-1-T1.nc").exists());
}

/// #95: writing is all or nothing. When the second run fails (its 30 mm
/// tool takes the cut off the stock), export fails and writes neither
/// program, and with `--post` leaves the project as it was.
#[test]
fn failed_tool_run_writes_no_files() {
    let dir = work_dir("failed_tool_run_writes_no_files");
    let dcv = two_tool_coaster(&dir);
    let mut project = load_file(&dcv).project;
    project.tools[1].diameter_mm = 30.0;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let before = std::fs::read(&dcv).expect("dcv readable");

    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(1), "export fails: {}", stderr(&out));
    for name in ["coaster.nc", "coaster-1-T1.nc", "coaster-2-T2.nc"] {
        assert!(!dir.join(name).exists(), "{name} not written");
    }
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), before);
}

/// #95: with `"m6"` in the post-processor, grblHAL gets the single
/// program: `M61 Q1` for the first tool and `T2 M6` at the change, and
/// export says the sender must stop at `M6`.
#[test]
fn m6_mode_writes_one_program() {
    let dir = work_dir("m6_mode_writes_one_program");
    let dcv = two_tool_coaster(&dir);
    set_tool_change_mode(&dcv, ToolChangeMode::M6);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(program.lines().any(|l| l == "M61 Q1"), "{program}");
    assert!(program.lines().any(|l| l.starts_with("T2 M6")), "{program}");
    assert!(!dir.join("coaster-1-T1.nc").exists());
    let summary = stdout(&out);
    assert!(summary.contains("sender must stop at M6"), "{summary}");
}

/// #95: GRBL has no `M6`, so `"m6"` on its definition is an error located
/// at the field, which `check` reports and on which export writes nothing.
#[test]
fn m6_mode_on_grbl_is_invalid() {
    let dir = work_dir("m6_mode_on_grbl_is_invalid");
    let dcv = two_tool_coaster(&dir);
    let mut project = load_file(&dcv).project;
    project.post_processor = dc_gcode::post::lookup("grbl").expect("grbl is bundled");
    project.post_processor.tool_change.mode = ToolChangeMode::M6;
    let text = save(&project).expect("project saves");
    std::fs::write(&dcv, &text).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "check fails: {report}");
    let located = located_lines(&report, "coaster.dcv");
    let [(line, column, message)] = located[..] else {
        panic!("one located finding: {report}");
    };
    assert!(message.starts_with("error: "), "{message}");
    let source = text.lines().nth(line - 1).expect("line exists");
    let at: String = source.chars().skip(column - 1).collect();
    assert_eq!(at, "\"m6\"", "{line}:{column} is the mode: `{source}`");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(1), "export fails: {}", stderr(&out));
    assert!(!dir.join("coaster.nc").exists());
    assert!(!dir.join("coaster-1-T1.nc").exists());
}
