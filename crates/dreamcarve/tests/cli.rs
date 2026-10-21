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
use std::process::{Command, Output, Stdio};

use dc_geometry::{Point, Polyline, Rect, Transform, assert_approx_eq};

#[path = "../src/clock.rs"]
#[allow(dead_code, reason = "the test uses only the timestamp")]
mod clock;
use dc_gcode::MachineMove;
use dc_project::test_support::{sample_drill, sample_pocket};
use dc_project::{
    CornerRelief, DefinitionId, DrillCycle, EngraveOp, Entry, FeatureDefinition, FeatureInstance,
    InstanceId, Lead, Loaded, MachiningOrder, Operation, OperationId, PeckRetract, ProfileOp,
    Project, Side, SpindleControl, TabPlacement, TabShape, TabSpec, ToolChangeMode, ToolId,
    ToolKind, load, save,
};

/// The date every compared program is exported with.
const FIXED_DATE: &str = "2026-10-15";

fn dreamcarve() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
}

/// The environment variable that overrides the user config directory
/// (`docs/specs/phase-2/design.md`, "User config (CLI only)").
const CONFIG_DIR_VAR: &str = "DREAMCARVE_CONFIG_DIR";

/// The user config directory every test runs with: `config/` under its
/// working directory, so no test reads the developer's own `config.toml`
/// and tests can write one of their own. It need not exist.
fn config_dir(dir: &Path) -> PathBuf {
    dir.join("config")
}

/// `dreamcarve` set up to run in `dir`, with [`config_dir`] as its user
/// config directory. Tests that need another environment edit it.
fn command(dir: &Path) -> Command {
    let mut command = dreamcarve();
    command
        .current_dir(dir)
        .env(CONFIG_DIR_VAR, config_dir(dir));
    command
}

/// Runs `dreamcarve` with `args` in `dir`.
fn run(dir: &Path, args: &[&str]) -> Output {
    command(dir).args(args).output().expect("binary runs")
}

/// Writes `text` as the user config file of a test working in `dir` and
/// returns its path.
fn write_config(dir: &Path, text: &str) -> PathBuf {
    let config = config_dir(dir);
    std::fs::create_dir_all(&config).expect("config dir created");
    let path = config.join("config.toml");
    std::fs::write(&path, text).expect("config written");
    path
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

/// `op` as the Profile every `init` project starts with.
fn as_profile(op: &Operation) -> &ProfileOp {
    match op {
        Operation::Profile(op) => op,
        other => panic!("a Profile operation, not {other:?}"),
    }
}

/// [`as_profile`], to edit.
fn as_profile_mut(op: &mut Operation) -> &mut ProfileOp {
    match op {
        Operation::Profile(op) => op,
        other => panic!("a Profile operation, not {other:?}"),
    }
}

/// `init`'s tab request (#105): four 8 × 3 mm rect tabs, none on holes.
fn init_tab_request() -> TabSpec {
    TabSpec {
        placement: TabPlacement::Count { count: 4 },
        width_mm: 8.0,
        height_mm: 3.0,
        shape: TabShape::Rect,
        on_holes: false,
    }
}

/// The quick start's measured plywood thickness, mm.
const MEASURED_THICKNESS_MM: f64 = 11.8;
/// The functional spec's test-part through-cut allowance, mm.
const THROUGH_CUT_ALLOWANCE_MM: f64 = 0.2;

/// The quick start's depth per pass, set on the tool (#98), mm.
const STEP_DOWN_MM: f64 = 4.0;

/// Edits the `.dcv` at `path` as the quick start does: every tool gets
/// `feed`, `plunge`, `rpm` and the 4 mm step-down; the stock gets the
/// measured thickness; and every Profile cuts through it by the test part's
/// 0.2 mm allowance.
fn fill_in(path: &Path, feed_mm_min: f64, plunge_mm_min: f64, rpm: u32) {
    let mut project = load_file(path).project;
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(feed_mm_min);
        tool.plunge_mm_min = Some(plunge_mm_min);
        tool.rpm = Some(rpm);
        tool.step_down_mm = Some(STEP_DOWN_MM);
    }
    project.stock.thickness_mm = Some(MEASURED_THICKNESS_MM);
    for op in project.operations.iter_mut().map(as_profile_mut) {
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
    // The circle is two exact arcs (#70); the tolerance is what phase 1
    // allowed its flattened vertices, which fell short of the extremes.
    let tol = 2.0 * project.flatten_tolerance_mm;
    assert_approx_eq!(bounds.min.x, 5.0, tol);
    assert_approx_eq!(bounds.min.y, 5.0, tol);
    assert_approx_eq!(bounds.max.x, 95.0, tol);
    assert_approx_eq!(bounds.max.y, 95.0, tol);
    assert_approx_eq!(project.stock.width_mm, 110.0, tol);
    assert_approx_eq!(project.stock.height_mm, 110.0, tol);
}

/// #70: the coaster's `<circle>` is imported as the circle it is, two
/// half-circle bulge arcs (bulge −1: usvg draws circles clockwise as seen
/// on the page), not 160 flattened edges. Placed on the stock it is the
/// 90 mm circle centred at (55, 55), and Drill's circle test finds it
/// exactly.
#[test]
fn init_imports_coaster_as_two_bulge_vertices() {
    let dir = work_dir("init_imports_coaster_as_two_bulge_vertices");
    let project = load_file(&init_coaster(&dir)).project;
    assert_eq!(project.definitions.len(), 1, "one definition");
    let definition = project.definitions.values().next().expect("a definition");
    assert_eq!(definition.profiles.len(), 1, "one profile");
    let profile = &definition.profiles[0];
    assert!(profile.is_closed());
    assert_eq!(profile.vertices().len(), 2, "{:?}", profile.vertices());
    for v in profile.vertices() {
        assert_approx_eq!(v.bulge, -1.0, 0.0);
    }
    let instance = &project.instances[0];
    let placed = instance
        .transform
        .apply_polyline(profile)
        .expect("transform applies");
    let (centre, radius_mm) = dc_geometry::as_circle(&placed, 1e-6).expect("a circle");
    assert_approx_eq!(centre.x, 55.0, 1e-4);
    assert_approx_eq!(centre.y, 55.0, 1e-4);
    assert_approx_eq!(radius_mm, 45.0, 1e-4);
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
    for op in project.operations.iter().map(as_profile) {
        assert!(op.depth_mm.is_none(), "{} depth is unset", op.id);
    }
}

/// #98: an operation's step-down overrides the tool's, so a value `init`
/// guessed would silently beat the one the user sets on the tool. `init`
/// leaves it `null`, like every other cutting value.
#[test]
fn init_leaves_operation_step_down_unset() {
    let dir = work_dir("init_leaves_operation_step_down_unset");
    let dcv = init_coaster(&dir);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&dcv).expect("dcv readable"))
            .expect("dcv is JSON");
    assert_eq!(
        value["operations"][0]["step_down_mm"],
        serde_json::Value::Null,
        "{value}"
    );
    assert!(
        value["operations"][0]
            .as_object()
            .expect("operation is an object")
            .contains_key("step_down_mm"),
        "the key is written, as null"
    );
}

/// #98: `check` on a fresh project lists the unset step-down once, located
/// at the tool's `null`, alongside the feed, plunge rate and RPM.
#[test]
fn check_after_init_lists_missing_step_down() {
    let dir = work_dir("check_after_init_lists_missing_step_down");
    let dcv = init_coaster(&dir);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let stdout = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "exits 1: {stdout}");
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    let located = located_lines(&stdout, "coaster.dcv");
    let step_down: Vec<_> = located
        .iter()
        .filter(|(_, _, m)| m.contains("step_down_mm"))
        .collect();
    let [(line, column, message)] = step_down[..] else {
        panic!("one step-down error: {stdout}");
    };
    assert_eq!(
        *message,
        "error: tools[t1].step_down_mm is not set (mm) for tool `Set me`"
    );
    assert_points_at_null(&text, *line, *column, "step_down_mm");
    let source = text.lines().nth(line - 1).expect("line exists");
    let rpm_line = located
        .iter()
        .find(|(_, _, m)| m.starts_with("error: tools[t1].rpm "))
        .unwrap_or_else(|| panic!("rpm error listed: {stdout}"))
        .0;
    assert_eq!(
        *line,
        rpm_line + 1,
        "the tool's step-down follows its rpm: {source}"
    );
    for field in ["feed_mm_min", "plunge_mm_min", "rpm"] {
        assert!(
            located
                .iter()
                .any(|(_, _, m)| m.starts_with(&format!("error: tools[t1].{field} is not set"))),
            "{field}: {stdout}"
        );
    }
}

/// #99: `init` cannot know a feature's depth, so every instance leaves
/// `z_top_mm` and `z_bottom_mm` unset (`null` in the file) instead of a
/// placeholder that would read as a decision.
#[test]
fn init_leaves_instance_depths_unset() {
    let dir = work_dir("init_leaves_instance_depths_unset");
    let dcv = init_coaster(&dir);
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    assert!(text.contains("\"z_top_mm\": null"), "{text}");
    assert!(text.contains("\"z_bottom_mm\": null"), "{text}");
    let project = load_file(&dcv).project;
    assert!(!project.instances.is_empty(), "init writes instances");
    for instance in &project.instances {
        assert_eq!(instance.z_top_mm, None, "{} z_top_mm", instance.id);
        assert_eq!(instance.z_bottom_mm, None, "{} z_bottom_mm", instance.id);
    }
}

/// The fields a fresh coaster project leaves unset, in file order, with
/// the key each one's line holds.
const UNSET_FIELDS: [(&str, &str); 6] = [
    ("stock.thickness_mm", "thickness_mm"),
    ("tools[t1].feed_mm_min", "feed_mm_min"),
    ("tools[t1].plunge_mm_min", "plunge_mm_min"),
    ("tools[t1].rpm", "rpm"),
    ("tools[t1].step_down_mm", "step_down_mm"),
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
    assert!(stdout.contains("6 error(s), 0 warning(s)"), "{stdout}");
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
/// noise below that (usvg's f32 rounding: a few millionths of a mm) is not worth
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
        stderr.contains("coaster.dcv:29:22: error: tool `Set me` (`t1`): feed_mm_min is 1000000000, expected a value greater than 0 and at most 20000 mm/min"),
        "{stderr}"
    );
    assert!(
        stderr.contains("coaster.dcv:31:14: warning: spindle_speed_high: tool `Set me` (`t1`) is set to 1000000 RPM"),
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

/// Capability 5 and phase 2 capability 23: the six bundled
/// post-processors are listed by id, grblHAL first, as
/// `dc_project::latest_definitions` orders them, each with its name.
#[test]
fn posts_lists_six() {
    let out = dreamcarve().arg("posts").output().expect("binary runs");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let stdout = stdout(&out);
    let ids: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(
        ids,
        ["grblhal", "grbl", "linuxcnc", "mach3", "uccnc", "generic"],
        "stdout was: {stdout}"
    );
    for name in [
        "grblHAL",
        "GRBL 1.1",
        "LinuxCNC",
        "Mach3",
        "UCCNC",
        "Generic RS274/NGC",
    ] {
        assert!(stdout.contains(name), "{name} in {stdout}");
    }
}

/// Runs `dreamcarve` with `args` in `dir` with its stdout a pipe whose
/// read end is closed before the binary starts, so its first write fails
/// with a broken pipe (`EPIPE`; `ERROR_NO_DATA` on Windows), as when the
/// reader of `dreamcarve … | head` has quit.
fn run_into_closed_pipe(dir: &Path, args: &[&str]) -> Output {
    let (reader, writer) = std::io::pipe().expect("pipe created");
    drop(reader);
    command(dir)
        .args(args)
        .stdout(writer)
        .output()
        .expect("binary runs")
}

/// Runs `dreamcarve` with `args` in `dir`, reads one line of its stdout
/// and closes the pipe, as `dreamcarve … | head -1` does. Whether the
/// binary has written the rest by then depends on timing.
fn run_closed_after_one_line(dir: &Path, args: &[&str]) -> Output {
    use std::io::BufRead as _;
    let mut child = command(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    let stdout = child.stdout.take().expect("stdout piped");
    let mut line = String::new();
    std::io::BufReader::new(stdout)
        .read_line(&mut line)
        .expect("one line read");
    assert!(!line.is_empty(), "the command printed a line");
    child.wait_with_output().expect("binary exits")
}

/// Asserts that a run whose stdout closed early did not panic and exited
/// with `code`, the command's own exit code.
fn assert_quiet_exit(out: &Output, code: i32, what: &str) {
    let stderr = stderr(out);
    assert!(!stderr.contains("panicked"), "{what}: {stderr}");
    assert!(!stderr.contains("Broken pipe"), "{what}: {stderr}");
    assert_eq!(out.status.code(), Some(code), "{what}: {stderr}");
}

/// #216: when the reader of stdout goes away (`dreamcarve … | head`), the
/// binary stops printing, finishes the command, and exits with the
/// command's own code: 0 for `posts`, 1 for `check` on a project with
/// values to set, 0 once they are set. It neither panics (exit 101) nor
/// prints anything about the pipe.
#[test]
fn broken_pipe_does_not_panic() {
    let dir = work_dir("broken_pipe_does_not_panic");
    let out = run_into_closed_pipe(&dir, &["posts"]);
    assert_quiet_exit(&out, 0, "posts");
    let out = run_closed_after_one_line(&dir, &["posts"]);
    assert_quiet_exit(&out, 0, "posts | head -1");

    let dcv = init_coaster(&dir);
    let out = run_into_closed_pipe(&dir, &["check", "coaster.dcv"]);
    assert_quiet_exit(&out, 1, "check with values to set");
    let out = run_closed_after_one_line(&dir, &["check", "coaster.dcv"]);
    assert_quiet_exit(&out, 1, "check with values to set | head -1");

    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run_into_closed_pipe(&dir, &["check", "coaster.dcv"]);
    assert_quiet_exit(&out, 0, "check of a complete project");

    // Work done before or after the output is not cut short: the program
    // is written although its summary cannot be printed.
    let out = run_into_closed_pipe(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_quiet_exit(&out, 0, "export");
    assert!(dir.join("coaster.nc").exists(), "program written");
}

/// #216: any other error writing stdout (here a full disk, `/dev/full`)
/// fails the command with exit 1 and says so on stderr.
#[cfg(target_os = "linux")]
#[test]
fn stdout_write_error_fails_with_message() {
    let dir = work_dir("stdout_write_error_fails_with_message");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full opens");
    let out = command(&dir)
        .arg("posts")
        .stdout(full)
        .output()
        .expect("binary runs");
    let stderr = stderr(&out);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with("error: cannot write to standard output: "),
        "{stderr}"
    );
}

/// The coaster with four 8 mm holes inside it, drilled through with the
/// `simple` cycle, on post-processor `post`, with canned cycles
/// `canned`. Returns the exported program.
fn coaster_with_holes(name: &str, post: &str, canned: bool) -> String {
    let dir = work_dir(name);
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let out = run(&dir, &["init", "coaster.svg", "--post", post]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("coaster.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let hole = FeatureDefinition {
        id: DefinitionId::new("hole"),
        name: "Hole".into(),
        profiles: vec![Polyline::circle(Point::new(0.0, 0.0), 4.0)],
    };
    let mut targets = Vec::new();
    for (k, (x, y)) in [(35.0, 35.0), (65.0, 35.0), (65.0, 65.0), (35.0, 65.0)]
        .into_iter()
        .enumerate()
    {
        let id = InstanceId::new(format!("hole{}", k + 1));
        project.instances.push(FeatureInstance {
            id: id.clone(),
            definition: hole.id.clone(),
            transform: Transform::translate(x, y),
            name: format!("Hole {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: Some(MEASURED_THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
        targets.push(id);
    }
    project.definitions.insert(hole.id.clone(), hole);
    let mut drill = sample_drill();
    drill.id = OperationId::new("o2");
    drill.targets = targets;
    drill.tool = project
        .operations
        .first()
        .expect("init writes a Profile")
        .tool()
        .clone();
    drill.cycle = DrillCycle::Simple;
    drill.dwell_s = 0.0;
    drill.depth_mm = Some(MEASURED_THICKNESS_MM + THROUGH_CUT_ALLOWANCE_MM);
    drill.through_cut_allowance_mm = THROUGH_CUT_ALLOWANCE_MM;
    // The holes are inside the part, so they are cut before it is freed.
    project.operations.insert(0, Operation::Drill(drill));
    project.post_processor.use_canned_cycles = canned;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    let report = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "export: {report}");
    assert!(!report.contains("canned_cycle_expanded"), "{report}");
    let extension = &project.post_processor.file_extension;
    std::fs::read_to_string(dir.join(format!("coaster.{extension}"))).expect("program written")
}

/// The code words of `line`, outside comments, upper-cased.
fn code_words(line: &str) -> Vec<String> {
    let mut code = String::new();
    let mut in_comment = false;
    for c in line.chars() {
        match c {
            '(' => in_comment = true,
            ')' => in_comment = false,
            c if !in_comment => code.push(c),
            _ => {}
        }
    }
    code.split_whitespace().map(str::to_uppercase).collect()
}

/// What the machine does for `text`, canned cycles expanded as the
/// definition `post` states: each move's kind and end, and each cut's
/// feed. Moves that go nowhere are left out.
fn machine_motion(text: &str, post: &str) -> Vec<String> {
    let resolved = dc_project::PostProcessorRef::latest(post)
        .expect("bundled")
        .resolve()
        .expect("resolves");
    dc_gcode::parse_motion_with(text, &dc_gcode::CycleModel::of(&resolved))
        .expect("program parses")
        .into_iter()
        .filter(|m| m.from != m.to)
        .map(|m| {
            let feed = if m.kind.is_cut() { m.feed } else { None };
            format!("{:?} {:?} {:?}", m.kind, m.to, feed)
        })
        .collect()
}

/// Acceptance 24 on the real LinuxCNC definition (slice 17, #164): with
/// canned cycles on, the simple Drill is one `G81` line per hole after
/// `G98`, then `G80`, and the program's motion, read back as LinuxCNC
/// performs the cycles, is the plain-moves program's. With canned cycles
/// off no cycle word appears (the header's `G80` only cancels one).
#[test]
fn canned_cycles_linuxcnc_opt_in() {
    let canned = coaster_with_holes("canned_cycles_linuxcnc_on", "linuxcnc", true);
    let plain = coaster_with_holes("canned_cycles_linuxcnc_off", "linuxcnc", false);
    let lines: Vec<Vec<String>> = canned.lines().map(code_words).collect();
    let has = |line: &[String], word: &str| line.iter().any(|w| w == word);
    let g81: Vec<usize> = (0..lines.len())
        .filter(|&i| has(&lines[i], "G81"))
        .collect();
    assert_eq!(g81.len(), 1, "G81 is modal, written once:\n{canned}");
    let cycle_lines: Vec<usize> = (g81[0]..lines.len())
        .take_while(|&i| !has(&lines[i], "G80"))
        .filter(|&i| lines[i].iter().any(|w| w.starts_with('X')))
        .collect();
    assert_eq!(cycle_lines.len(), 4, "one line per hole:\n{canned}");
    assert!(has(&lines[g81[0] - 1], "G98"), "G98 first:\n{canned}");
    let after = cycle_lines.last().copied().unwrap_or_default() + 1;
    assert!(
        lines.get(after).is_some_and(|l| has(l, "G80")),
        "G80 last:\n{canned}"
    );
    assert_eq!(
        machine_motion(&canned, "linuxcnc"),
        machine_motion(&plain, "linuxcnc"),
        "canned:\n{canned}\nplain:\n{plain}"
    );
    for line in plain.lines().map(code_words) {
        for cycle in ["G73", "G81", "G82", "G83", "G98", "G99"] {
            assert!(!has(&line, cycle), "{cycle} with cycles off:\n{plain}");
        }
    }
}

/// `tests/golden/post-<id>-coaster` (design spec, "Test plan → golden"):
/// the quick-start coaster exported for each slice 17 post-processor. The
/// committed program was captured from DreamCarve after its definition was
/// checked against the controller's documentation (each case's README),
/// not from PureCutCNC. Its motion must be the grblHAL coaster's
/// (`tests/expected/coaster-grblhal.nc`, acceptance 36).
mod golden {
    use super::*;

    fn check(id: &str, extension: &str) {
        let dir = work_dir(&format!("golden_post_{id}_coaster"));
        let dcv = init_coaster(&dir);
        fill_in(&dcv, 1500.0, 500.0, 18000);
        let out = run(
            &dir,
            &["export", "coaster.dcv", "--post", id, "--date", FIXED_DATE],
        );
        assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
        let program = std::fs::read_to_string(dir.join(format!("coaster.{extension}")))
            .expect("program written");
        let case = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/golden/post-{id}-coaster"));
        let expected = std::fs::read_to_string(case.join(format!("program.{extension}")))
            .expect("golden program present");
        assert!(
            program == expected,
            "the {id} coaster differs from tests/golden/post-{id}-coaster:\n{program}"
        );
        let hal = include_str!("expected/coaster-grblhal.nc");
        assert_eq!(
            machine_motion(&program, id),
            machine_motion(hal, "grblhal"),
            "{id}: motion differs from grblHAL's"
        );
    }

    #[test]
    fn post_linuxcnc_coaster() {
        check("linuxcnc", "ngc");
    }

    #[test]
    fn post_mach3_coaster() {
        check("mach3", "tap");
    }

    #[test]
    fn post_uccnc_coaster() {
        check("uccnc", "txt");
    }

    #[test]
    fn post_generic_coaster() {
        check("generic", "nc");
    }
}

/// The filled-in coaster with post-processor `id` at version 1, as a
/// project saved before #286 names it.
fn coaster_at_v1(dir: &Path, id: &str) {
    coaster_at(dir, id, 1);
}

/// The filled-in coaster with post-processor `id` at `version`.
fn coaster_at(dir: &Path, id: &str, version: u32) {
    let dcv = init_coaster(dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor = dc_project::bundled_definition(id, version)
        .expect("the version is bundled")
        .default_ref();
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// `program`'s lines without Mach3's `N` line numbers, which shift when a
/// header line is added.
fn unnumbered(program: &str) -> Vec<String> {
    program
        .lines()
        .map(|l| match l.split_once(' ') {
            Some((n, rest))
                if n.len() > 1
                    && n.starts_with('N')
                    && n[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                rest.to_owned()
            }
            _ => l.to_owned(),
        })
        .collect()
}

/// `lines` with `G50` and `G69` after the header's modal line, the one
/// with `G80`: what version 3 of Mach3 and UCCNC adds (#312).
fn with_scale_rotate_cancel(mut lines: Vec<String>) -> Vec<String> {
    let modal = lines
        .iter()
        .position(|l| l.split_whitespace().any(|w| w == "G80"))
        .expect("the header has a G80 line");
    lines.splice(modal + 1..modal + 1, ["G50".to_owned(), "G69".to_owned()]);
    lines
}

/// Whether Mach3 line numbers run `N10`, `N20`, ... with no gap, every
/// line but `%` numbered.
fn numbered_in_tens(program: &str) -> bool {
    program
        .lines()
        .filter(|l| *l != "%")
        .enumerate()
        .all(|(i, l)| l.starts_with(&format!("N{} ", (i + 1) * 10)))
}

/// #312, ADR-0004: a project that names version 2 of Mach3 exports the
/// bytes it did before version 3 added `G50` and `G69` to the header
/// (`tests/expected/coaster-mach3-v2.tap`, the golden program as it was
/// captured with version 2).
#[test]
fn mach3_v2_project_still_exports_byte_identically() {
    let dir = work_dir("mach3_v2_project_still_exports_byte_identically");
    coaster_at(&dir, "mach3", 2);
    assert_exports_match(&dir, &[], &[("coaster.tap", "coaster-mach3-v2.tap")]);
}

/// #312: the same for UCCNC.
#[test]
fn uccnc_v2_project_still_exports_byte_identically() {
    let dir = work_dir("uccnc_v2_project_still_exports_byte_identically");
    coaster_at(&dir, "uccnc", 2);
    assert_exports_match(&dir, &[], &[("coaster.txt", "coaster-uccnc-v2.txt")]);
}

/// #312: `export --post` upgrades a version 2 Mach3 or UCCNC project to
/// version 3, and the only change is a `G50` line and a `G69` line after
/// the header's modal line (Mach3 numbers every line, so the numbers
/// after them move on by 20).
#[test]
fn mach3_and_uccnc_v2_projects_upgrade_to_v3() {
    for (id, program) in [("mach3", "coaster.tap"), ("uccnc", "coaster.txt")] {
        let dir = work_dir(&format!("v2_project_upgrades_to_v3_{id}"));
        coaster_at(&dir, id, 2);
        let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let v2 = std::fs::read_to_string(dir.join(program)).expect("program written");
        let out = run(
            &dir,
            &["export", "coaster.dcv", "--post", id, "--date", FIXED_DATE],
        );
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let report = stdout(&out);
        assert!(
            report.contains(&format!("post-processor {id} v2 → v3,")),
            "{id}: {report}"
        );
        let v3 = std::fs::read_to_string(dir.join(program)).expect("program rewritten");
        assert_eq!(
            unnumbered(&v3),
            with_scale_rotate_cancel(unnumbered(&v2)),
            "{id}: only G50 and G69 join the program"
        );
        if id == "mach3" {
            assert!(numbered_in_tens(&v3), "{v3}");
        }
    }
}

/// #286, ADR-0004: a project that names version 1 of LinuxCNC exports the
/// bytes it did before version 2 took `G49` out of the header
/// (`tests/expected/coaster-linuxcnc-v1.ngc`, the golden program as it was
/// captured with version 1).
#[test]
fn linuxcnc_v1_project_still_exports_byte_identically() {
    let dir = work_dir("linuxcnc_v1_project_still_exports_byte_identically");
    coaster_at_v1(&dir, "linuxcnc");
    assert_exports_match(&dir, &[], &[("coaster.ngc", "coaster-linuxcnc-v1.ngc")]);
}

/// #286: the same for Mach3 and UCCNC, whose version 2 also drops `G49`.
#[test]
fn mach3_and_uccnc_v1_projects_still_export_byte_identically() {
    for (id, program, expected) in [
        ("mach3", "coaster.tap", "coaster-mach3-v1.tap"),
        ("uccnc", "coaster.txt", "coaster-uccnc-v1.txt"),
    ] {
        let dir = work_dir(&format!("v1_project_still_exports_byte_identically_{id}"));
        coaster_at_v1(&dir, id);
        assert_exports_match(&dir, &[], &[(program, expected)]);
    }
}

/// #286: `export --post` upgrades a version 1 LinuxCNC, Mach3 or UCCNC
/// project to the latest version, and the header's modal line loses
/// `G49`. On LinuxCNC (version 2) nothing else changes. Mach3 and UCCNC go
/// to version 3, which also adds `G50` and `G69` (#312).
#[test]
fn slice17_v1_project_upgrades_to_latest_without_g49() {
    for (id, program, latest) in [
        ("linuxcnc", "coaster.ngc", 2),
        ("mach3", "coaster.tap", 3),
        ("uccnc", "coaster.txt", 3),
    ] {
        let dir = work_dir(&format!("slice17_v1_project_upgrades_{id}"));
        coaster_at_v1(&dir, id);
        let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let v1 = std::fs::read_to_string(dir.join(program)).expect("program written");
        let out = run(
            &dir,
            &["export", "coaster.dcv", "--post", id, "--date", FIXED_DATE],
        );
        assert_eq!(out.status.code(), Some(0), "{id}: {}", stderr(&out));
        let report = stdout(&out);
        assert!(
            report.contains(&format!("post-processor {id} v1 → v{latest},")),
            "{id}: {report}"
        );
        let upgraded = std::fs::read_to_string(dir.join(program)).expect("program rewritten");
        assert!(!upgraded.contains("G49"), "{id}:\n{upgraded}");
        let without_g49 = v1.replace(" G49", "");
        let expected = if latest == 3 {
            with_scale_rotate_cancel(unnumbered(&without_g49))
        } else {
            unnumbered(&without_g49)
        };
        assert_eq!(
            unnumbered(&upgraded),
            expected,
            "{id}: only G49 leaves the program"
        );
        assert_ne!(v1, upgraded, "{id}: v1 had G49");
    }
}

// Phase 2, #117: format version 8.

/// A fresh project carries every phase 2 field at the value that cuts as
/// phase 1 did: it plunges (the ramp generator lands in a later slice),
/// has no lead or corner relief, no tool number, canned cycles off, and
/// arc fitting at 0.01 mm. Its tabs leave holes untabbed (#109, format
/// version 9).
#[test]
fn init_writes_phase_2_defaults() {
    let dir = work_dir("init_writes_phase_2_defaults");
    let dcv = init_coaster(&dir);
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    let version = format!("\"version\": {},", dc_project::CURRENT_VERSION);
    assert!(text.contains(&version), "{text}");
    assert!(text.contains("\"on_holes\": false"), "{text}");
    assert!(text.contains("\"arc_fit_tolerance_mm\": 0.01,"), "{text}");
    assert!(text.contains("\"number\": null,"), "{text}");
    assert!(text.contains("\"lead\": null,"), "{text}");
    assert!(text.contains("\"use_canned_cycles\": false"), "{text}");
    let project = load_file(&dcv).project;
    assert!(
        project
            .arc_fit_tolerance_mm
            .is_some_and(|t| (t - 0.01).abs() < 1e-12)
    );
    for op in project.operations.iter().map(as_profile) {
        assert_eq!(op.machining_order, MachiningOrder::FullDepthFirst);
        assert_eq!(op.lead, None);
        assert_eq!(op.corner_relief, CornerRelief::None);
    }
    assert!(!project.post_processor.use_canned_cycles);
    assert!(project.tools.iter().all(|t| t.number.is_none()));
    // Only the values `init` has always left to the user are reported.
    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    for word in ["entry", "number", "canned", "arc_fit"] {
        assert!(!report.contains(word), "{word} in {report}");
    }
}

/// #119: a new project's Profile ramps in at 15° (functional spec,
/// capability 16, open question 6), written as the one-key object, and
/// `check` has nothing to say about it.
#[test]
fn init_writes_ramp_entry() {
    let dir = work_dir("init_writes_ramp_entry");
    let dcv = init_coaster(&dir);
    let text = std::fs::read_to_string(&dcv).expect("dcv readable");
    let compact: String = text.split_whitespace().collect();
    assert!(
        compact.contains("\"entry\":{\"ramp\":{\"angle_deg\":15.0}}"),
        "{text}"
    );
    let project = load_file(&dcv).project;
    for op in project.operations.iter().map(as_profile) {
        assert_eq!(op.entry, Entry::Ramp { angle_deg: 15.0 });
    }
    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert!(!report.contains("entry"), "{report}");
}

/// The spec's `v6_fixture_exports_phase_1_bytes_with_fitting_off`, for
/// the version this repository is at: a phase 1 project opened by this
/// build exports the same program whether it is still a version 7 file or
/// has been saved at the current version. (Arc fitting does not exist yet, so the
/// tolerance the migration writes changes nothing.)
#[test]
fn v7_project_exports_the_same_program() {
    let dir = work_dir("v7_project_exports_the_same_program");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dc-project/tests/fixtures/dcv/v7/sample.dcv");
    let v7 = dir.join("v7.dcv");
    std::fs::copy(&fixture, &v7).expect("fixture copied");
    let text = std::fs::read_to_string(&v7).expect("dcv readable");
    assert!(
        text.contains("\"version\": 7,"),
        "the fixture is a version 7 file"
    );
    let out = run(&dir, &["export", "v7.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export v7: {}", stderr(&out));
    let from_v7 = std::fs::read_to_string(dir.join("v7.nc")).expect("v7.nc written");

    let project = load_file(&v7).project;
    let v8 = dir.join("v8.dcv");
    std::fs::write(&v8, save(&project).expect("project saves")).expect("dcv written");
    let saved = std::fs::read_to_string(&v8).expect("dcv readable");
    let version = format!("\"version\": {},", dc_project::CURRENT_VERSION);
    assert!(saved.contains(&version), "{saved}");
    let out = run(&dir, &["export", "v8.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export v8: {}", stderr(&out));
    let from_v8 = std::fs::read_to_string(dir.join("v8.nc")).expect("v8.nc written");
    assert!(from_v7 == from_v8, "v7:\n{from_v7}\nv8:\n{from_v8}");
    assert!(!from_v7.is_empty());
}

/// Functional spec capability 12, S9 (slice 4, #137): a project with an
/// offset Pocket of the coaster before its cut-out exports one program,
/// which the emitter's safety pass (S1–S7) accepted; `check` finds no
/// error. The Pocket's rings are in the program at its 3 mm depth.
#[test]
fn pocket_exports_and_passes_safety() {
    let dir = work_dir("pocket_exports_and_passes_safety");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let mut pocket = sample_pocket();
    pocket.targets = project.operations[0].targets().to_vec();
    pocket.tool = project.operations[0].tool().clone();
    pocket.step_over_fraction = Some(0.4);
    // Inside the part, so cut before the Profile frees it.
    project.operations.insert(0, Operation::Pocket(pocket));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "{report}");
    assert!(report.contains(" 0 error(s)"), "{report}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    assert!(program.contains("Pocket"), "the operation is named");
    // Z is from the stock top (init's origin), so the pocket floor is -3.
    assert!(
        program.lines().any(|l| l.contains("Z-3")),
        "the pocket reaches its depth"
    );
}

/// #229, S12: an On line Profile of a plain 7 mm or 20 mm square exports
/// with arc fitting on (the default). The square's four corners lie on one
/// circle, and its sides are far longer than the tolerance allows a chord
/// to be, so the lap stays four straight moves; before, it was written as
/// four 90° arcs bowing off the line and the export failed S9.
#[test]
fn on_line_square_exports_with_arc_fitting_on() {
    for size_mm in [7.0, 20.0] {
        let dir = work_dir(&format!(
            "on_line_square_exports_with_arc_fitting_on_{size_mm}"
        ));
        let dcv = init_coaster(&dir);
        fill_in(&dcv, 1500.0, 500.0, 18000);
        let mut project = load_file(&dcv).project;
        assert!(project.arc_fit_tolerance_mm.is_some(), "arc fitting is on");
        for def in project.definitions.values_mut() {
            def.profiles = vec![Polyline::rect(Point::new(20.0, 20.0), size_mm, size_mm)];
        }
        for op in project.operations.iter_mut().map(as_profile_mut) {
            op.side = Side::OnLine;
            op.tabs = None;
            op.depth_mm = Some(1.0);
        }
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

        let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{size_mm} mm square: {}",
            stderr(&out)
        );
        let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
        let arcs: Vec<&str> = program
            .lines()
            .filter(|l| code_words(l).iter().any(|w| w == "G2" || w == "G3"))
            .collect();
        assert!(arcs.is_empty(), "{size_mm} mm square has arcs: {arcs:?}");
    }
}

/// Slice 6 (#163), capabilities 12 and 13, S9 and S14 end to end: a 40 ×
/// 20 mm rectangle with a 6 × 6 mm notch inside the coaster, roughed with
/// the coaster's 6.35 mm bit at a 0.5 mm allowance and then rest-machined
/// with a 3.175 mm bit, checks with no error and exports one program per
/// tool run, which the emitter's safety pass accepted. The rest program
/// cuts only near the walls and in the notch, where the rough bit cannot
/// go, never in the middle, and its tool centre stays a radius inside the
/// shape.
#[test]
fn pocket_rough_then_rest_exports_and_passes_safety() {
    const REST_RADIUS_MM: f64 = 3.175 / 2.0;
    let dir = work_dir("pocket_rough_then_rest_exports_and_passes_safety");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let v = dc_geometry::Vertex::line;
    let notched = FeatureDefinition {
        id: DefinitionId::new("notched"),
        name: "Notched".into(),
        profiles: vec![Polyline::new(
            vec![
                v(30.0, 35.0),
                v(70.0, 35.0),
                v(70.0, 55.0),
                v(53.0, 55.0),
                v(53.0, 61.0),
                v(47.0, 61.0),
                v(47.0, 55.0),
                v(30.0, 55.0),
            ],
            true,
        )],
    };
    project.instances.push(FeatureInstance {
        id: InstanceId::new("notched1"),
        definition: notched.id.clone(),
        transform: Transform::IDENTITY,
        name: "Notched".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(MEASURED_THICKNESS_MM),
        z_bottom_mm: Some(0.0),
    });
    project.definitions.insert(notched.id.clone(), notched);
    let mut small = project.tools[0].clone();
    small.id = ToolId::new("t2");
    small.name = "3mm finisher".into();
    small.diameter_mm = 3.175;
    small.step_down_mm = Some(3.0);
    project.tools.push(small);
    let mut rough = sample_pocket();
    rough.id = OperationId::new("rough");
    rough.name = "Rough".into();
    rough.targets = vec![InstanceId::new("notched1")];
    rough.tool = project.operations[0].tool().clone();
    rough.step_over_fraction = Some(0.4);
    rough.allowance_mm = 0.5;
    let mut rest = rough.clone();
    rest.id = OperationId::new("rest");
    rest.name = "Rest".into();
    rest.tool = ToolId::new("t2");
    rest.allowance_mm = 0.0;
    rest.rest_of = Some(rough.id.clone());
    project.operations.insert(0, Operation::Pocket(rough));
    project.operations.insert(1, Operation::Pocket(rest));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "{report}");
    assert!(report.contains(" 0 error(s)"), "{report}");
    assert!(!report.contains("rest_of_clears_nothing"), "{report}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster-2-T2.nc")).expect("rest program");
    assert!(program.contains("Rest"), "the rest operation is named");
    let moves = dc_gcode::parse_motion(&program).expect("program parses");
    let cuts: Vec<&MachineMove> = moves
        .iter()
        .filter(|m| m.kind.is_cut() && (m.from.x != m.to.x || m.from.y != m.to.y))
        .collect();
    assert!(!cuts.is_empty(), "the rest Pocket cuts:\n{program}");
    let xy = |m: &MachineMove| Point::new(m.to.x.expect("X known"), m.to.y.expect("Y known"));
    let shape = Rect {
        min: Point::new(30.0 + REST_RADIUS_MM, 35.0 + REST_RADIUS_MM),
        max: Point::new(70.0 - REST_RADIUS_MM, 61.0 - REST_RADIUS_MM),
    };
    let middle = Rect {
        min: Point::new(33.5, 38.5),
        max: Point::new(66.5, 51.5),
    };
    let inside = |r: Rect, p: Point| {
        p.x >= r.min.x - 1e-3
            && p.x <= r.max.x + 1e-3
            && p.y >= r.min.y - 1e-3
            && p.y <= r.max.y + 1e-3
    };
    for m in &cuts {
        let p = xy(m);
        assert!(inside(shape, p), "line {}: {p:?} off the shape", m.line);
        assert!(
            !(p.x > middle.min.x && p.x < middle.max.x && p.y > middle.min.y && p.y < middle.max.y),
            "line {}: {p:?} recuts the middle the rough cleared",
            m.line
        );
    }
    assert!(
        cuts.iter().any(|m| xy(m).y > 55.0 + REST_RADIUS_MM),
        "the rest cuts into the notch"
    );
    let deepest = moves.iter().filter_map(|m| m.to.z).fold(f64::MAX, f64::min);
    assert_approx_eq!(deepest, -3.0, 1e-3);
}

/// Slice 12 (#181), capability 18, acceptance 29 and S9 end to end: a 40 ×
/// 30 mm rectangle inside the coaster pocketed with dogbones checks with
/// no error and exports on grblHAL, so the full safety pass, S9's notch
/// rule included, accepted the notches. Each of the four corners has a
/// cut ending one tool radius from it along its bisector.
#[test]
fn pocket_dogbone_exports_and_passes_safety() {
    let dir = work_dir("pocket_dogbone_exports_and_passes_safety");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let panel = FeatureDefinition {
        id: DefinitionId::new("panel"),
        name: "Panel".into(),
        profiles: vec![Polyline::rect(Point::new(30.0, 35.0), 40.0, 30.0)],
    };
    project.instances.push(FeatureInstance {
        id: InstanceId::new("panel1"),
        definition: panel.id.clone(),
        transform: Transform::IDENTITY,
        name: "Panel".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(MEASURED_THICKNESS_MM),
        z_bottom_mm: Some(0.0),
    });
    project.definitions.insert(panel.id.clone(), panel);
    let mut pocket = sample_pocket();
    pocket.targets = vec![InstanceId::new("panel1")];
    pocket.tool = project.operations[0].tool().clone();
    pocket.step_over_fraction = Some(0.4);
    pocket.corner_relief = CornerRelief::Dogbone;
    // #189, #177: then the same rectangle cut through inside, with no tabs,
    // with dogbones too, before the coaster's own Profile.
    let mut hole = as_profile(&project.operations[0]).clone();
    hole.id = OperationId::new("panel_hole");
    hole.name = "Panel hole".into();
    hole.targets = vec![InstanceId::new("panel1")];
    hole.side = Side::Inside;
    hole.tabs = None;
    hole.corner_relief = CornerRelief::Dogbone;
    project.operations.insert(0, Operation::Pocket(pocket));
    project.operations.insert(1, Operation::Profile(hole));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "{report}");
    assert!(report.contains(" 0 error(s)"), "{report}");
    assert!(!report.contains("relief_"), "{report}");

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
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    let moves = dc_gcode::parse_motion(&program).expect("program parses");
    let r = 6.35 / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    for (cx, cy, sx, sy) in [
        (30.0, 35.0, 1.0, 1.0),
        (70.0, 35.0, -1.0, 1.0),
        (70.0, 65.0, -1.0, -1.0),
        (30.0, 65.0, 1.0, -1.0),
    ] {
        let end = Point::new(cx + sx * r, cy + sy * r);
        assert!(
            moves.iter().any(|m| m.kind.is_cut()
                && m.to.z.is_some_and(|z| (z + 3.0).abs() < 1e-3)
                && m.to
                    .x
                    .zip(m.to.y)
                    .is_some_and(|(x, y)| { Point::new(x, y).distance_to(end) < 1e-3 })),
            "no notch ends at {end:?}:\n{program}"
        );
    }

    // #189, #193: the hole's notches reach the bottom, and every one of
    // them is cut before the lap that frees the slug closes: the bottom
    // level from within that lap, a full lap of the 33.65 × 23.65 mm
    // tool-centre rectangle at the bottom, part of which follows the last
    // notch.
    let bottom_z = -(MEASURED_THICKNESS_MM + THROUGH_CUT_ALLOWANCE_MM);
    let at_bottom = |m: &MachineMove| {
        m.from
            .z
            .zip(m.to.z)
            .is_some_and(|(a, b)| (a - bottom_z).abs() < 1e-3 && (b - bottom_z).abs() < 1e-3)
    };
    let in_panel = |m: &MachineMove| {
        m.to.x
            .zip(m.to.y)
            .is_some_and(|(x, y)| (30.0..=70.0).contains(&x) && (35.0..=65.0).contains(&y))
    };
    let ends: Vec<Point> = [
        (30.0, 35.0, 1.0, 1.0),
        (70.0, 35.0, -1.0, 1.0),
        (70.0, 65.0, -1.0, -1.0),
        (30.0, 65.0, 1.0, -1.0),
    ]
    .into_iter()
    .map(|(cx, cy, sx, sy)| Point::new(cx + sx * r, cy + sy * r))
    .collect();
    let at_end = |x: Option<f64>, y: Option<f64>| {
        x.zip(y)
            .is_some_and(|(x, y)| ends.iter().any(|e| Point::new(x, y).distance_to(*e) < 1e-3))
    };
    let mut last_notch = 0;
    for end in &ends {
        let notch = moves.iter().rposition(|m| {
            m.kind.is_cut()
                && at_bottom(m)
                && m.to
                    .x
                    .zip(m.to.y)
                    .is_some_and(|(x, y)| Point::new(x, y).distance_to(*end) < 1e-3)
        });
        let notch = notch.unwrap_or_else(|| panic!("no notch at the bottom ends at {end:?}"));
        last_notch = last_notch.max(notch);
    }
    // The freeing lap's own moves: at the bottom in the panel, off every
    // notch.
    let lap_moves: Vec<(usize, f64)> = moves
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.kind.is_cut()
                && at_bottom(m)
                && in_panel(m)
                && !at_end(m.from.x, m.from.y)
                && !at_end(m.to.x, m.to.y)
        })
        .filter_map(|(i, m)| {
            let (x0, y0) = m.from.x.zip(m.from.y)?;
            let (x1, y1) = m.to.x.zip(m.to.y)?;
            Some((i, Point::new(x0, y0).distance_to(Point::new(x1, y1))))
        })
        .filter(|(_, length)| *length > 1e-6)
        .collect();
    let lap = 2.0 * ((40.0 - 6.35) + (30.0 - 6.35));
    let total: f64 = lap_moves.iter().map(|(_, length)| length).sum();
    // The ramp's last leg, back to the lap's start, runs at the bottom
    // too.
    assert!(
        total >= lap - 1e-2,
        "only {total:.3} mm of the {lap:.3} mm freeing lap is cut:\n{program}"
    );
    assert!(
        lap_moves.iter().any(|(i, _)| *i > last_notch),
        "the freeing lap closes before the last notch at line {}:\n{program}",
        moves[last_notch].line
    );
}

/// Slice 9 (#147), capability 16 and S10 end to end: the coaster's Pocket
/// with a helix entry at 80 % and 5°, on a tool whose plunge rate (100)
/// is under the helix's vertical rate at full feed (1500 × sin 5° = 131),
/// checks with no entry warning and exports. The helix moves run at the
/// plunge-limited feed, between the plunge rate and the feed, and the
/// safety pass accepted them.
#[test]
fn pocket_helix_entry_exports_and_passes_safety() {
    let dir = work_dir("pocket_helix_entry_exports_and_passes_safety");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 100.0, 18000);
    let mut project = load_file(&dcv).project;
    let mut pocket = sample_pocket();
    pocket.targets = project.operations[0].targets().to_vec();
    pocket.tool = project.operations[0].tool().clone();
    pocket.step_over_fraction = Some(0.4);
    pocket.entry = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    project.operations.insert(0, Operation::Pocket(pocket));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "{report}");
    assert!(!report.contains("entry_fallback"), "{report}");
    assert!(!report.contains("helix_diameter_reduced"), "{report}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    let scaled: Vec<f64> = program
        .lines()
        .filter_map(|l| l.split_whitespace().find_map(|w| w.strip_prefix('F')))
        .filter_map(|f| f.parse::<f64>().ok())
        .filter(|f| *f > 100.0 + 1e-6 && *f < 1500.0 - 1e-6)
        .collect();
    assert!(
        !scaled.is_empty(),
        "the helix runs at a plunge-limited feed:\n{program}"
    );
    // 100 / sin 5° = 1147 mm/min, a little less on the chords. The
    // Profile's 15° ramp (init's entry) is plunge-limited too, at
    // 100 / sin 15° = 386 mm/min.
    assert!(
        scaled.iter().any(|f| (1100.0..1150.0).contains(f)),
        "the helix is cut: {scaled:?}"
    );
    for f in scaled {
        assert!(
            (1100.0..1150.0).contains(&f) || (f - 100.0 / 15f64.to_radians().sin()).abs() < 0.01,
            "helix or ramp feed {f}"
        );
    }
}

/// The number of lines of `program` that move on an arc (`G2`/`G3`, as
/// either `G2` or `G02`).
fn arc_lines(program: &str) -> usize {
    program
        .lines()
        .map(code_words)
        .filter(|words| {
            words
                .iter()
                .any(|w| matches!(w.as_str(), "G2" | "G3" | "G02" | "G03"))
        })
        .count()
}

/// Slice 11 (#180), capability 17 end to end: the coaster's Profile with
/// a 2 mm lead-in, lead-out and 1 mm overcut checks with no lead warning
/// and exports on grblHAL and on LinuxCNC; each program passed the
/// emitter's safety pass (export refuses one that fails it) and has more
/// arc moves than the same project without the lead: the lead arcs.
#[test]
fn profile_lead_exports_and_passes_safety() {
    for post in ["grblhal", "linuxcnc"] {
        let mut programs = Vec::new();
        for with_lead in [false, true] {
            let dir = work_dir(&format!("profile_lead_{post}_{with_lead}"));
            let dcv = init_coaster(&dir);
            fill_in(&dcv, 1500.0, 500.0, 18000);
            let mut project = load_file(&dcv).project;
            if with_lead {
                for op in project.operations.iter_mut().map(as_profile_mut) {
                    op.lead = Some(Lead {
                        radius_mm: 2.0,
                        sweep_deg: 90.0,
                        lead_out: true,
                        overcut_mm: 1.0,
                    });
                }
            }
            std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
            let check = run(&dir, &["check", "coaster.dcv"]);
            let report = format!("{}{}", stdout(&check), stderr(&check));
            assert_eq!(check.status.code(), Some(0), "{report}");
            assert!(!report.contains("lead_"), "{post}: {report}");
            let out = run(
                &dir,
                &[
                    "export",
                    "coaster.dcv",
                    "--post",
                    post,
                    "--date",
                    FIXED_DATE,
                ],
            );
            assert_eq!(out.status.code(), Some(0), "{post}: {}", stderr(&out));
            let program = std::fs::read_dir(&dir)
                .expect("dir readable")
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| {
                    p.extension()
                        .is_some_and(|x| x != "dcv" && x != "svg" && x != "toml")
                })
                .map(|p| std::fs::read_to_string(p).expect("program readable"))
                .expect("program written");
            programs.push(program);
        }
        assert!(
            arc_lines(&programs[1]) > arc_lines(&programs[0]),
            "{post}: the lead adds arcs:\n{}",
            programs[1]
        );
    }
}

/// Slice 7 (#138), acceptance test 23 on the CLI: the coaster with an
/// 8 mm hole at its centre, drilled through in 4 mm pecks with a full
/// retract and a 0.25 s dwell, checks clean and exports. The pecks count
/// from the retract height 1 mm above the 11.8 mm stock (#153): to 8.8,
/// 4.8, 0.8 and −0.2 mm. The program dwells
/// at the bottom of each of the four pecks, reaches the coaster's depth at
/// the hole, and moves in XY only at its highest Z (S2); the safety pass,
/// S6 and S15 included, accepted it.
#[test]
fn drill_exports_and_passes_safety() {
    let dir = work_dir("drill_exports_and_passes_safety");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let hole = FeatureDefinition {
        id: DefinitionId::new("hole"),
        name: "Hole".into(),
        profiles: vec![Polyline::circle(Point::new(0.0, 0.0), 4.0)],
    };
    project.instances.push(FeatureInstance {
        id: InstanceId::new("hole1"),
        definition: hole.id.clone(),
        transform: Transform::translate(50.0, 50.0),
        name: "Hole".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(MEASURED_THICKNESS_MM),
        z_bottom_mm: Some(0.0),
    });
    project.definitions.insert(hole.id.clone(), hole);
    let mut drill = sample_drill();
    drill.id = OperationId::new("o2");
    drill.targets = vec![InstanceId::new("hole1")];
    drill.tool = project.operations[0].tool().clone();
    drill.cycle = DrillCycle::Peck {
        peck_depth_mm: Some(4.0),
        retract: PeckRetract::Full,
    };
    drill.dwell_s = 0.25;
    drill.depth_mm = Some(MEASURED_THICKNESS_MM + THROUGH_CUT_ALLOWANCE_MM);
    drill.through_cut_allowance_mm = THROUGH_CUT_ALLOWANCE_MM;
    // Drilled before the tabbed cut-out, while the stock still holds the
    // coaster: after it, the hole would be cut in a tab-held part (#269).
    project.operations.insert(0, Operation::Drill(drill));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "{report}");
    assert!(report.contains("0 error(s), 0 warning(s)"), "{report}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    let dwells = program.lines().filter(|l| l.trim() == "G4 P0.250").count();
    assert_eq!(dwells, 4, "one dwell per peck:\n{program}");

    let moves = dc_gcode::parse_motion(&program).expect("program parses");
    let z = |m: &MachineMove| m.to.z.expect("Z is known");
    let top = moves.iter().map(z).fold(f64::MIN, f64::max);
    let deepest = moves
        .iter()
        .filter(|m| m.kind.is_cut())
        .map(z)
        .fold(f64::MAX, f64::min);
    // The hole is where the tool is at the first drill dwell.
    let first_dwell = 1 + program
        .lines()
        .position(|l| l.trim() == "G4 P0.250")
        .expect("a drill dwell");
    let at_dwell = moves
        .iter()
        .rev()
        .find(|m| m.line < first_dwell)
        .expect("a move before the dwell");
    let hole_cuts: Vec<f64> = moves
        .iter()
        .filter(|m| m.kind.is_cut() && m.to.x == at_dwell.to.x && m.to.y == at_dwell.to.y)
        .map(z)
        .collect();
    assert_eq!(hole_cuts.len(), 4, "four pecks at the hole: {hole_cuts:?}");
    assert_approx_eq!(
        hole_cuts.iter().copied().fold(f64::MAX, f64::min),
        deepest,
        1e-3
    );
    for (i, m) in moves.iter().enumerate() {
        let moves_xy = m.from.x != m.to.x || m.from.y != m.to.y;
        if m.kind == dc_gcode::parse::MotionKind::Rapid && moves_xy {
            assert_approx_eq!(m.from.z.unwrap_or(top), top, 1e-6);
            assert_approx_eq!(z(m), top, 1e-6);
            assert!(i > 0, "first move is an XY rapid");
        }
    }
}

/// The output extension is a setting in the `.dcv`, so a shared project
/// chooses it. An extension that is a path (`../x`) or names the
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
    for op in project.operations.iter_mut().map(as_profile_mut) {
        if let Some(tabs) = &mut op.tabs {
            tabs.height_mm = thickness_mm + 1.0;
            // Two tabs 5 mm apart overlap on every outline; with the
            // third they would cover most of the small hole, which then
            // keeps the first two (#109).
            tabs.placement = TabPlacement::AtDistances {
                distances_mm: vec![10.0, 15.0, 40.0],
            };
            tabs.on_holes = true;
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
        "would not cut free",
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
    project.post_processor.spindle_control = SpindleControl::Manual;
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
        project.post_processor.spindle_control = control;
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
        "warning: unused_tool: tool `t2` (`Spare bit`) is defined but not used by any enabled operation"
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
/// fail, and write nothing. `--reset-settings` drops the manual control,
/// which a plain `--post` keeps (#277).
#[test]
fn post_override_revalidates() {
    let dir = work_dir("post_override_revalidates");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor.spindle_control = SpindleControl::Manual;
    for tool in &mut project.tools {
        tool.rpm = None;
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "manual post passes: {report}");

    let before = std::fs::read(&dcv).expect("dcv readable");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--reset-settings",
        ],
    );
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
/// `check`, but `export --post grblhal --reset-settings` replaces that
/// post and its settings and exports. A plain `--post` keeps the
/// extension the user set (#277), so it fails the same check and writes
/// nothing.
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

    let before = std::fs::read(&dcv).expect("dcv readable");
    let out = run(&dir, &["export", "coaster.dcv", "--post", "grblhal"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "kept extension: {}",
        stdout(&out)
    );
    assert!(stderr(&out).contains("`dcv`"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read(&dcv).expect("dcv readable"),
        before,
        "the .dcv is untouched"
    );

    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--reset-settings",
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
    let [a, b, c, d, _, f] = instance.transform.coeffs();
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

/// Functional acceptance 37, through `init`: with
/// `arc_fit_tolerance_mm: null` nothing is fitted. Since #70 the
/// quick-start coaster's `<circle>` imports as two exact arcs, leaving
/// nothing to fit, so this uses the coaster drawn as four Béziers that are
/// not the circle's exact arc (`tests/fixtures/coaster-bezier.svg`), which
/// is flattened as phase 1 flattened every circle.
/// `tests/expected/coaster-bezier-grblhal-unfitted.nc` is its program with
/// every lap its straight moves; with `init`'s 0.01 mm the fitted program
/// is a fraction of the length. (It ramps into each pass, as `init`
/// projects do since #119, so it is not the phase 1 program; the migrated
/// phase 1 fixture keeps that, `v7_project_exports_the_same_program`.)
#[test]
fn null_arc_fit_tolerance_exports_the_unfitted_program() {
    let dir = work_dir("null_arc_fit_tolerance");
    copy_fixture(&dir, "coaster-bezier.svg", "coaster.svg");
    let out = run(&dir, &["init", "coaster.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("coaster.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    assert_eq!(project.arc_fit_tolerance_mm, Some(0.01), "init's default");
    let facets = project
        .definitions
        .values()
        .next()
        .expect("a definition")
        .profiles[0]
        .vertices()
        .len();
    assert!(facets > 100, "the Bézier circle is flattened: {facets}");
    project.arc_fit_tolerance_mm = None;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    assert_exports_match(
        &dir,
        &[],
        &[("coaster.nc", "coaster-bezier-grblhal-unfitted.nc")],
    );
    let unfitted = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");

    project.arc_fit_tolerance_mm = Some(0.01);
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--date",
            FIXED_DATE,
            "-o",
            "fitted.nc",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let fitted = std::fs::read_to_string(dir.join("fitted.nc")).expect("fitted.nc written");
    assert!(
        fitted.lines().count() < unfitted.lines().count() / 4,
        "fitted {} lines, unfitted {}",
        fitted.lines().count(),
        unfitted.lines().count()
    );
}

/// #62: `export` and `check` print the program's extents in machine
/// coordinates, rounded as the program is. Z is exactly the range of the Z
/// words (Z min is the deepest cut). X and Y reach at least as far as the
/// X and Y words, and further where an arc bulges past its end words: the
/// coaster's lap is two half-circle arcs (#70), so the extents are the
/// tool-centre circle, radius 45 + 3.175 mm about (55, 55), within the
/// rounding of the written arcs.
#[test]
fn export_prints_extents() {
    /// The tool-centre circle's reach, mm.
    const CIRCLE_MIN_MM: f64 = 55.0 - 48.175;
    /// See [`CIRCLE_MIN_MM`].
    const CIRCLE_MAX_MM: f64 = 55.0 + 48.175;
    /// How far the extents read from the written arcs may differ from it.
    const SLACK_MM: f64 = 0.01;
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
            // The entry ramp's straight moves may reach past the circle.
            assert!(
                min <= word_min && (min - CIRCLE_MIN_MM.min(word_min)).abs() <= SLACK_MM,
                "{command}: {axis} min {min} vs words {word_min}: {summary}"
            );
            assert!(
                max >= word_max && (max - CIRCLE_MAX_MM.max(word_max)).abs() <= SLACK_MM,
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
    let mut finish = as_profile(project.operations.first().expect("the coaster profile")).clone();
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
    project.post_processor.tool_change_mode = mode;
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
    let mut again = as_profile(&project.operations[0]).clone();
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

/// Sets the `number` of each tool of the `.dcv` at `dcv`, in list order.
fn set_tool_numbers(dcv: &Path, numbers: &[Option<u32>]) {
    let mut project = load_file(dcv).project;
    for (tool, &number) in project.tools.iter_mut().zip(numbers) {
        tool.number = number;
    }
    std::fs::write(dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// #120, capability 21a: a tool's set `number` names its tool run's
/// program, `<stem>-<k>-T<number>`, and is the number the summary tells
/// the user to fit and the program's comments carry. Each program's
/// cutter edge is its own tool's, found by id, not by number.
#[test]
fn tool_number_names_program_files() {
    let dir = work_dir("tool_number_names_program_files");
    let dcv = two_tool_coaster(&dir);
    set_tool_numbers(&dcv, &[Some(5), Some(9)]);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    for (name, comment) in [
        ("coaster-1-T5.nc", "( Tool 5: Set me )"),
        ("coaster-2-T9.nc", "( Tool 9: 3mm finisher )"),
    ] {
        let program = std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{name} written: {e}"));
        assert!(program.lines().any(|l| l == comment), "{name}: {program}");
    }
    for name in ["coaster-1-T1.nc", "coaster-2-T2.nc"] {
        assert!(!dir.join(name).exists(), "{name} not written");
    }
    let summary = stdout(&out);
    let expected = "Wrote 2 programs; run them in this order:\n  \
        1. coaster-1-T5.nc  Before you start: fit T5 Set me and set work zero (X, Y and Z) with it.\n  \
        2. coaster-2-T9.nc  Before you start: fit T9 3mm finisher and set Z zero with it. Keep X and Y zero.\n";
    assert!(summary.contains(expected), "{summary}");
    let edges = summary
        .lines()
        .filter(|l| l.contains("The cutter's edge reaches"))
        .count();
    assert_eq!(
        edges, 2,
        "both programs name their cutter's edge: {summary}"
    );

    let out = run(&dir, &["check", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(0), "check: {}", stderr(&out));
    let report = stdout(&out);
    for line in ["Program 1 (T5):", "Program 2 (T9):"] {
        assert!(report.lines().any(|l| l == line), "{line}: {report}");
    }
}

/// #120: the tool the user fits before starting is named by its set
/// `number`: for a one-tool program, and for one program with `M6`, whose
/// `M61 Q<n>` and `T<n> M6` use the numbers too.
#[test]
fn before_you_start_names_the_tool_number() {
    let dir = work_dir("before_you_start_names_the_tool_number");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    set_tool_numbers(&dcv, &[Some(4)]);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let summary = stdout(&out);
    assert!(
        summary.contains("Before you start: fit T4 Set me and set work zero with it."),
        "{summary}"
    );

    let dir = work_dir("before_you_start_names_the_tool_number_m6");
    let dcv = two_tool_coaster(&dir);
    set_tool_change_mode(&dcv, ToolChangeMode::M6);
    set_tool_numbers(&dcv, &[Some(5), Some(9)]);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let summary = stdout(&out);
    assert!(
        summary.contains("Before you start: fit T5 Set me and set work zero with it."),
        "{summary}"
    );
    assert!(summary.contains("Fit T9 3mm finisher."), "{summary}");
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(program.lines().any(|l| l == "M61 Q5"), "{program}");
    assert!(
        program.lines().any(|l| l.starts_with("T9 M6 ")),
        "{program}"
    );
}

/// #95: GRBL has no `M6`, so `"m6"` on its definition is an error located
/// at the field, which `check` reports and on which export writes nothing.
#[test]
fn m6_mode_on_grbl_is_invalid() {
    let dir = work_dir("m6_mode_on_grbl_is_invalid");
    let dcv = two_tool_coaster(&dir);
    let mut project = load_file(&dcv).project;
    project.post_processor = dc_project::PostProcessorRef::latest("grbl").expect("grbl is bundled");
    project.post_processor.tool_change_mode = ToolChangeMode::M6;
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
    assert_eq!(at, "\"m6\",", "{line}:{column} is the mode: `{source}`");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(1), "export fails: {}", stderr(&out));
    assert!(!dir.join("coaster.nc").exists());
    assert!(!dir.join("coaster-1-T1.nc").exists());
}

/// The `post_processor` block of the `.dcv` at `dcv`, as raw JSON.
fn post_processor_block(dcv: &Path) -> serde_json::Map<String, serde_json::Value> {
    let text = std::fs::read_to_string(dcv).expect("dcv readable");
    let root: serde_json::Value = serde_json::from_str(&text).expect("dcv is JSON");
    root.get("post_processor")
        .and_then(serde_json::Value::as_object)
        .expect("post_processor is an object")
        .clone()
}

/// The keys of a project's `post_processor` block, in the order written
/// (#106).
const POST_PROCESSOR_KEYS: [&str; 8] = [
    "id",
    "definition_version",
    "file_extension",
    "spindle_control",
    "spindle_on_dwell_s",
    "tool_change_mode",
    "decimal_places",
    "use_canned_cycles",
];

/// #106: `init` writes a reference to the latest bundled grblHAL
/// definition and the default settings, not a copy of the definition.
#[test]
fn init_writes_post_processor_reference() {
    let dir = work_dir("init_writes_post_processor_reference");
    let dcv = init_coaster(&dir);
    let block = post_processor_block(&dcv);
    let keys: Vec<&str> = block.keys().map(String::as_str).collect();
    assert_eq!(keys, POST_PROCESSOR_KEYS);
    let latest = dc_project::latest_definition("grblhal").expect("grblHAL is bundled");
    assert_eq!(
        serde_json::Value::Object(block),
        serde_json::json!({
            "id": "grblhal",
            "definition_version": latest.version,
            "file_extension": "nc",
            "spindle_control": "automatic",
            "spindle_on_dwell_s": 3.0,
            "tool_change_mode": "separate_programs",
            "decimal_places": { "mm": 3, "inch": 4 },
            "use_canned_cycles": false
        })
    );
}

/// #171: `init` writes the latest grblHAL version, whose program opens
/// with fixed text, so the project name never starts a comment.
#[test]
fn init_writes_latest_grblhal() {
    let dir = work_dir("init_writes_latest_grblhal");
    let dcv = init_coaster(&dir);
    let block = post_processor_block(&dcv);
    let latest = dc_project::latest_definition("grblhal").expect("grblHAL is bundled");
    assert_eq!(block.get("id"), Some(&serde_json::json!("grblhal")));
    assert_eq!(
        block.get("definition_version"),
        Some(&serde_json::json!(latest.version))
    );
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert_eq!(program.lines().next(), Some("( Project: coaster )"));
}

/// The lines of an older grblHAL or GRBL program as version 3 writes
/// them: the first line `first`, and `G80`, `G40` after `G94` (#279).
fn as_v3(program: &str, first: &str) -> Vec<String> {
    let mut lines: Vec<String> = program.lines().map(str::to_owned).collect();
    if let Some(line) = lines.first_mut() {
        first.clone_into(line);
    }
    let g94 = lines
        .iter()
        .position(|l| l == "G94")
        .expect("the header has G94");
    lines.splice(g94 + 1..g94 + 1, ["G80".to_owned(), "G40".to_owned()]);
    lines
}

/// `lines` with `G50` after every line that is `G40`: what grblHAL
/// version 4 adds to version 3, in the header and in the setup after a
/// stop (#316).
fn with_g50(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len() + 1);
    for line in lines {
        let g40 = line == "G40";
        out.push(line);
        if g40 {
            out.push("G50".to_owned());
        }
    }
    out
}

/// The lines of an older program of `id` as the latest version of `id`
/// writes them, and that version: GRBL version 3 ([`as_v3`]), grblHAL
/// version 4 (version 3 [`with_g50`]).
fn as_latest(id: &str, program: &str, first: &str) -> (u32, Vec<String>) {
    let v3 = as_v3(program, first);
    if id == "grblhal" {
        (4, with_g50(v3))
    } else {
        (3, v3)
    }
}

/// Writes the coaster project in `dir`, filled in as the quick start does,
/// naming version `version` of `id`'s definition with its default
/// settings.
fn coaster_at_version(dir: &Path, id: &str, version: u32) -> PathBuf {
    let dcv = init_coaster(dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor = dc_project::bundled_definition(id, version)
        .expect("the version is bundled")
        .default_ref();
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// #171, #279, ADR-0004: a project that names version 1 of grblHAL or
/// GRBL exports the bytes it did before version 2 existed
/// (`tests/expected/coaster-<id>-v1.nc`, committed before #171; its ramp
/// re-captured in #214, which changed the motion, not the format) until the
/// user runs `export --post <id>`. That upgrades it to the latest version,
/// says so, and changes only the program's first line and the header's
/// `G80`, `G40` (and on grblHAL, version 4, `G50`, #316).
#[test]
fn v1_project_exports_v1_bytes_until_upgraded() {
    for (id, expected, first) in [
        ("grblhal", "coaster-grblhal-v1.nc", "( Project: coaster )"),
        ("grbl", "coaster-grbl-v1.nc", "; Project: coaster"),
    ] {
        let dir = work_dir(&format!("v1_project_exports_v1_bytes_{id}"));
        let dcv = coaster_at_version(&dir, id, 1);
        assert_exports_match(&dir, &[], &[("coaster.nc", expected)]);
        let v1 = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");

        let out = run(
            &dir,
            &["export", "coaster.dcv", "--post", id, "--date", FIXED_DATE],
        );
        assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
        let (latest, expected) = as_latest(id, &v1, first);
        let report = stdout(&out);
        assert!(
            report.contains(&format!("post-processor {id} v1 → v{latest},")),
            "{report}"
        );
        let block = post_processor_block(&dcv);
        assert_eq!(
            block.get("definition_version"),
            Some(&serde_json::json!(latest))
        );
        let upgraded =
            std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc rewritten");
        assert!(
            upgraded.lines().eq(expected.iter().map(String::as_str)),
            "{id}: only the first line and G80, G40 (and on grblHAL G50) change\n\
             v1:\n{v1}\nv{latest}:\n{upgraded}"
        );
    }
}

/// #279, acceptance 41, ADR-0004: a project that names version 2 of
/// grblHAL or GRBL exports the bytes it did before version 3 existed
/// (`tests/expected/coaster-<id>-v2.nc`, committed before #279) until
/// `export --post <id>` upgrades it, which adds only `G80` and `G40`
/// after `G94` (and on grblHAL, version 4, `G50` after them, #316).
#[test]
fn v2_project_exports_v2_bytes() {
    for (id, expected, first) in [
        ("grblhal", "coaster-grblhal-v2.nc", "( Project: coaster )"),
        ("grbl", "coaster-grbl-v2.nc", "; Project: coaster"),
    ] {
        let dir = work_dir(&format!("v2_project_exports_v2_bytes_{id}"));
        let dcv = coaster_at_version(&dir, id, 2);
        assert_exports_match(&dir, &[], &[("coaster.nc", expected)]);
        let v2 = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
        assert!(!v2.lines().any(|l| l == "G80" || l == "G40"), "{id}: {v2}");

        let out = run(
            &dir,
            &["export", "coaster.dcv", "--post", id, "--date", FIXED_DATE],
        );
        assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
        let (latest, expected) = as_latest(id, &v2, first);
        let report = stdout(&out);
        assert!(
            report.contains(&format!("post-processor {id} v2 → v{latest},")),
            "{report}"
        );
        let block = post_processor_block(&dcv);
        assert_eq!(
            block.get("definition_version"),
            Some(&serde_json::json!(latest))
        );
        let upgraded =
            std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc rewritten");
        assert!(
            upgraded.lines().eq(expected.iter().map(String::as_str)),
            "{id}: only G80, G40 (and on grblHAL G50) are added\n\
             v2:\n{v2}\nv{latest}:\n{upgraded}"
        );
    }
}

/// #316, ADR-0004: `init` writes grblHAL version 4, and a project that
/// names version 3 exports the bytes it did before version 4 added `G50`
/// to the header (`tests/expected/coaster-grblhal-v3.nc`, the golden
/// program as version 3 wrote it).
#[test]
fn grblhal_v3_project_still_exports_byte_identically() {
    let dir = work_dir("grblhal_v3_project_still_exports_byte_identically_init");
    let dcv = init_coaster(&dir);
    let block = post_processor_block(&dcv);
    assert_eq!(block.get("definition_version"), Some(&serde_json::json!(4)));

    let dir = work_dir("grblhal_v3_project_still_exports_byte_identically");
    coaster_at_version(&dir, "grblhal", 3);
    assert_exports_match(&dir, &[], &[("coaster.nc", "coaster-grblhal-v3.nc")]);
    let v3 = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(!v3.lines().any(|l| l == "G50"), "{v3}");
}

/// #316: `export --post grblhal` upgrades a version 3 project to version
/// 4, says so, and the only change is a `G50` line after the header's
/// `G40`.
#[test]
fn grblhal_v3_project_upgrades_to_v4() {
    let dir = work_dir("grblhal_v3_project_upgrades_to_v4");
    let dcv = coaster_at_version(&dir, "grblhal", 3);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let v3 = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
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
    let report = stdout(&out);
    assert!(
        report.contains("post-processor grblhal v3 → v4,"),
        "{report}"
    );
    let block = post_processor_block(&dcv);
    assert_eq!(block.get("definition_version"), Some(&serde_json::json!(4)));
    let v4 = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc rewritten");
    let expected = with_g50(v3.lines().map(str::to_owned).collect());
    assert!(
        v4.lines().eq(expected.iter().map(String::as_str)),
        "only G50 is added\nv3:\n{v3}\nv4:\n{v4}"
    );
}

/// The note `export` prints for grblHAL and GRBL programs (functional
/// capability 29): the program leaves the tool length offset alone.
const TOOL_LENGTH_NOTE: &str = "This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.";

/// #279: grblHAL and GRBL fold an active tool length offset into the work
/// zero the user sets, so their programs leave it alone and `export` says
/// so, once, for one program and for one program per tool run. The note
/// names grblHAL's and GRBL's `G43.1`, so it is not printed for LinuxCNC
/// (functional spec, capability 29).
#[test]
fn export_grblhal_prints_tool_length_note() {
    for (post, notes) in [("grblhal", 1), ("grbl", 1), ("linuxcnc", 0)] {
        let dir = work_dir(&format!("export_tool_length_note_{post}"));
        let dcv = init_coaster(&dir);
        fill_in(&dcv, 1500.0, 500.0, 18000);
        let out = run(
            &dir,
            &[
                "export",
                "coaster.dcv",
                "--post",
                post,
                "--date",
                FIXED_DATE,
            ],
        );
        assert_eq!(out.status.code(), Some(0), "{post}: {}", stderr(&out));
        let report = stdout(&out);
        assert_eq!(
            report.matches(TOOL_LENGTH_NOTE).count(),
            notes,
            "{post}: {report}"
        );
    }
    let dir = work_dir("export_tool_length_note_runs");
    two_tool_coaster(&dir);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    assert!(report.contains("Wrote 2 programs"), "{report}");
    assert_eq!(report.matches(TOOL_LENGTH_NOTE).count(), 1, "{report}");
}

/// The note `export` prints for a grblHAL program whose definition writes
/// `G50` (#320): grblHAL's `G50` does not clear a scale that leaves X at 1
/// (`definitions/README.md`, "Scaling on grblHAL").
const SCALING_NOTE: &str = "If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.";

/// #320: a grblHAL v4 export prints the scaling note once, for one program
/// and for one program per tool run, after the tool length note. The note
/// is printed, not written: the programs are the committed ones.
#[test]
fn export_grblhal_v4_prints_scaling_note() {
    let dir = work_dir("export_grblhal_v4_prints_scaling_note");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let block = post_processor_block(&dcv);
    assert_eq!(block.get("id"), Some(&serde_json::json!("grblhal")));
    assert_eq!(block.get("definition_version"), Some(&serde_json::json!(4)));
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(report.matches(SCALING_NOTE).count(), 1, "{report}");
    assert!(
        report.contains(&format!("{TOOL_LENGTH_NOTE}\n{SCALING_NOTE}\n")),
        "the scaling note follows the tool length note:\n{report}"
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(
        program == include_str!("expected/coaster-grblhal.nc"),
        "program bytes changed:\n{program}"
    );

    let dir = work_dir("export_grblhal_v4_prints_scaling_note_runs");
    two_tool_coaster(&dir);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout(&out);
    assert!(report.contains("Wrote 2 programs"), "{report}");
    assert_eq!(report.matches(SCALING_NOTE).count(), 1, "{report}");
    assert!(
        report.contains(&format!("{TOOL_LENGTH_NOTE}\n{SCALING_NOTE}\n")),
        "the scaling note follows the tool length note:\n{report}"
    );
    assert_exports_match(
        &dir,
        &[],
        &[
            ("coaster-1-T1.nc", "coaster-two-tool-grblhal-1-T1.nc"),
            ("coaster-2-T2.nc", "coaster-two-tool-grblhal-2-T2.nc"),
        ],
    );
}

/// #320: the scaling note is only for a grblHAL definition that writes
/// `G50`. GRBL and LinuxCNC have no scaling, Mach3's and UCCNC's `G50`
/// clears every factor, and grblHAL v3 writes no `G50`.
#[test]
fn export_without_g50_prints_no_scaling_note() {
    for post in ["grbl", "linuxcnc", "mach3", "uccnc"] {
        let dir = work_dir(&format!("export_without_g50_prints_no_scaling_note_{post}"));
        let dcv = init_coaster(&dir);
        fill_in(&dcv, 1500.0, 500.0, 18000);
        let out = run(
            &dir,
            &[
                "export",
                "coaster.dcv",
                "--post",
                post,
                "--date",
                FIXED_DATE,
            ],
        );
        assert_eq!(out.status.code(), Some(0), "{post}: {}", stderr(&out));
        let report = stdout(&out);
        assert!(!report.contains("G51"), "{post}: {report}");
    }
    let dir = work_dir("export_without_g50_prints_no_scaling_note_grblhal_v3");
    coaster_at_version(&dir, "grblhal", 3);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "grblhal v3: {}", stderr(&out));
    let report = stdout(&out);
    assert!(report.contains(TOOL_LENGTH_NOTE), "grblhal v3: {report}");
    assert!(!report.contains("G51"), "grblhal v3: {report}");
}

/// The motion of `program`: every move, as where it goes, at what feed,
/// and its arc centre.
fn motion(program: &str) -> Vec<String> {
    dc_gcode::parse_motion(program)
        .expect("program parses")
        .into_iter()
        .map(|m| format!("{:?} {:?} {:?} {:?}", m.kind, m.to, m.feed, m.arc))
        .collect()
}

/// #106, acceptance test 13: `export --post grbl` rewrites the reference
/// to GRBL's latest version, and the program has the same motion as
/// grblHAL's. The settings the user changed (an extension, manual spindle
/// control) are kept (#277, acceptance 39); the others take GRBL's
/// defaults.
#[test]
fn export_post_switch_writes_reference() {
    let dir = work_dir("export_post_switch_writes_reference");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor.file_extension = "tap".into();
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    set_spindle_control(&dcv, SpindleControl::Manual);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let grblhal = std::fs::read_to_string(dir.join("coaster.tap")).expect("coaster.tap written");

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

    let block = post_processor_block(&dcv);
    let keys: Vec<&str> = block.keys().map(String::as_str).collect();
    assert_eq!(keys, POST_PROCESSOR_KEYS);
    let latest = dc_project::latest_definition("grbl").expect("GRBL is bundled");
    assert_eq!(
        serde_json::Value::Object(block),
        serde_json::json!({
            "id": "grbl",
            "definition_version": latest.version,
            "file_extension": "tap",
            "spindle_control": "manual",
            "spindle_on_dwell_s": 3.0,
            "tool_change_mode": "separate_programs",
            "decimal_places": { "mm": 3, "inch": 4 },
            "use_canned_cycles": false
        })
    );
    let grbl = std::fs::read_to_string(dir.join("coaster.tap")).expect("coaster.tap rewritten");
    assert_ne!(grbl, grblhal, "the dialects differ");
    assert_eq!(motion(&grbl), motion(&grblhal));
}

/// Sets the spindle control of the `.dcv` at `dcv`.
fn set_spindle_control(dcv: &Path, control: SpindleControl) {
    let mut project = load_file(dcv).project;
    project.post_processor.spindle_control = control;
    std::fs::write(dcv, save(&project).expect("project saves")).expect("dcv written");
}

/// Exports the `.dcv` in `dir` with `extra` arguments and the fixed date,
/// then checks that each program in `outputs` equals the committed
/// `tests/expected/` file it names, byte for byte.
fn assert_exports_match(dir: &Path, extra: &[&str], outputs: &[(&str, &str)]) {
    let mut args = vec!["export", "coaster.dcv", "--date", FIXED_DATE];
    args.extend_from_slice(extra);
    let out = run(dir, &args);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let expected_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected");
    for &(name, expected_name) in outputs {
        let program = std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{name} written: {e}"));
        let expected = std::fs::read_to_string(expected_dir.join(expected_name))
            .unwrap_or_else(|e| panic!("tests/expected/{expected_name} readable: {e}"));
        assert!(
            program == expected,
            "{name} differs from tests/expected/{expected_name}:\n{program}"
        );
    }
}

/// A five-pointed star (points 40 mm and notches 20 mm from its centre)
/// and, as a separate path, a 10 mm circle at its centre: a part with a
/// hole too small for tabs (#104).
const STAR_AND_HOLE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <path id="star" d="M 50 10 L 38.244 33.82 L 11.958 37.639 L 30.979 56.18 L 26.489 82.361 L 50 70 L 73.511 82.361 L 69.021 56.18 L 88.042 37.639 L 61.756 33.82 Z"/>
  <circle id="hole" cx="50" cy="50" r="5"/>
</svg>
"#;

/// The star and hole as one SVG path: the circle is a second subpath.
const STAR_WITH_HOLE_ONE_PATH_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <path id="star" d="M 50 10 L 38.244 33.82 L 11.958 37.639 L 30.979 56.18 L 26.489 82.361 L 50 70 L 73.511 82.361 L 69.021 56.18 L 88.042 37.639 L 61.756 33.82 Z M 55 50 A 5 5 0 1 0 45 50 A 5 5 0 1 0 55 50 Z"/>
</svg>
"#;

/// The hole's radius in the star fixtures, mm.
const HOLE_RADIUS_MM: f64 = 5.0;
/// Half the 6.35 mm bit `init` gives the tool, mm.
const BIT_RADIUS_MM: f64 = 3.175;
/// How far a cut may sit from its ideal radius: the circle is flattened to
/// 0.01 mm and the program is rounded to 0.001 mm.
const RADIUS_TOLERANCE_MM: f64 = 0.02;

/// Writes `svg` as `star.svg` in `dir`, runs `init` on it, and fills the
/// project in as the quick start does. Returns the `.dcv`'s path.
fn init_star(dir: &Path, svg: &str) -> PathBuf {
    std::fs::write(dir.join("star.svg"), svg).expect("svg written");
    let out = run(dir, &["init", "star.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("star.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    dcv
}

/// The instance named `name` in `project`.
fn instance_named<'a>(project: &'a Project, name: &str) -> &'a FeatureInstance {
    project
        .instances
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("an instance named {name}"))
}

/// The machine position of the SVG's (50, 50), the hole's centre, in the
/// instance named `name`: (50, 50) moved by the instance's translation.
/// `origin.xy` is `bottom_left`, so machine X and Y are project X and Y.
fn hole_centre(project: &Project, name: &str) -> (f64, f64) {
    let [_, _, _, _, e, f] = instance_named(project, name).transform.coeffs();
    (50.0 + e, 50.0 + f)
}

/// Exports the `.dcv` in `dir` as `star.nc` and returns its moves.
fn export_star(dir: &Path) -> Vec<MachineMove> {
    let out = run(dir, &["export", "star.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("star.nc")).expect("star.nc written");
    dc_gcode::parse_motion(&program).expect("program parses")
}

/// How far `m` ends from `centre` in XY, mm.
fn distance_from(m: &MachineMove, centre: (f64, f64)) -> f64 {
    let x = m.to.x.expect("X is known after the first move");
    let y = m.to.y.expect("Y is known after the first move");
    (x - centre.0).hypot(y - centre.1)
}

/// The indices of the cutting moves of `moves` that end within `radius_mm`
/// of `centre`, and of those that end further out.
fn split_cuts(
    moves: &[MachineMove],
    centre: (f64, f64),
    radius_mm: f64,
) -> (Vec<usize>, Vec<usize>) {
    let (near, far): (Vec<_>, Vec<_>) = moves
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind.is_cut())
        .partition(|(_, m)| distance_from(m, centre) <= radius_mm);
    (
        near.into_iter().map(|(k, _)| k).collect(),
        far.into_iter().map(|(k, _)| k).collect(),
    )
}

/// Whether `m` rises from below `z_mm` to `z_mm`.
fn rises_to(m: &MachineMove, z_mm: f64) -> bool {
    match (m.from.z, m.to.z) {
        (Some(from), Some(to)) => from < z_mm - 1e-6 && (to - z_mm).abs() < 1e-6,
        _ => false,
    }
}

/// #104: the Tabs chapter's recipe for a part with a hole too small for
/// tabs. Operation 1 cuts the hole alone, `inside`, no tabs; operation 2
/// cuts the star alone, `outside`, 6 tabs, same tool. The hole is cut
/// first, inside the circle (tool centre 5 − 3.175 mm from its centre),
/// without rising over any tab; the star's last pass rises to the tab top
/// exactly 6 times.
#[test]
fn part_and_hole_as_two_operations() {
    let dir = work_dir("part_and_hole_as_two_operations");
    let dcv = init_star(&dir, STAR_AND_HOLE_SVG);
    let mut project = load_file(&dcv).project;
    let star = instance_named(&project, "star").id.clone();
    let hole = instance_named(&project, "hole").id.clone();
    let centre = hole_centre(&project, "hole");
    let mut hole_op = as_profile(&project.operations[0]).clone();
    hole_op.name = "Hole".into();
    hole_op.targets = vec![hole];
    hole_op.side = Side::Inside;
    hole_op.tabs = None;
    // Every hole move is checked against the tool-centre circle, so no
    // ramp inside it (the entry has its own tests in dc-cam).
    hole_op.entry = Entry::Plunge;
    let mut star_op = as_profile(&project.operations[0]).clone();
    star_op.id = OperationId::new("op2");
    star_op.name = "Star".into();
    star_op.targets = vec![star];
    star_op.side = Side::Outside;
    let tabs = star_op.tabs.as_mut().expect("init gives the profile tabs");
    tabs.placement = TabPlacement::Count { count: 6 };
    let tab_top_z = tabs.height_mm - MEASURED_THICKNESS_MM;
    project.operations = vec![Operation::Profile(hole_op), Operation::Profile(star_op)];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let moves = export_star(&dir);
    let (hole_cuts, star_cuts) = split_cuts(&moves, centre, HOLE_RADIUS_MM);
    let (Some(&first_hole), Some(&last_hole)) = (hole_cuts.first(), hole_cuts.last()) else {
        panic!("the hole is cut");
    };
    let Some(&first_star) = star_cuts.first() else {
        panic!("the star is cut");
    };
    assert!(last_hole < first_star, "the hole is cut before the star");

    let inside_mm = HOLE_RADIUS_MM - BIT_RADIUS_MM;
    for &k in &hole_cuts {
        let radius = distance_from(&moves[k], centre);
        assert!(
            (radius - inside_mm).abs() <= RADIUS_TOLERANCE_MM,
            "the hole's tool path is {inside_mm} mm from its centre, not {radius} mm"
        );
    }
    let deepest_z = star_cuts
        .iter()
        .filter_map(|&k| moves[k].to.z)
        .fold(f64::INFINITY, f64::min);
    let hole_deepest_z = hole_cuts
        .iter()
        .filter_map(|&k| moves[k].to.z)
        .fold(f64::INFINITY, f64::min);
    assert_approx_eq!(hole_deepest_z, deepest_z, 1e-6);
    // Tabs on a hole this small would cover its whole path and hold every
    // pass below the tab top at it, so no hole move may end there.
    for m in &moves[first_hole..=last_hole] {
        assert!(m.kind.is_cut(), "the hole is one cut: {m:?}");
        assert!(
            m.to.z.is_none_or(|z| (z - tab_top_z).abs() > 1e-6),
            "the hole has no tab: {m:?}"
        );
    }

    let last_pass = star_cuts
        .iter()
        .copied()
        .find(|&k| moves[k].to.z.is_some_and(|z| (z - deepest_z).abs() < 1e-6))
        .expect("the star reaches full depth");
    let rises = moves[last_pass..]
        .iter()
        .filter(|m| rises_to(m, tab_top_z))
        .count();
    assert_eq!(rises, 6, "the star's last pass rises over 6 tabs");
}

/// #104: whether an outline is a part or a hole is decided among one
/// operation's targets. Targeted alone, the 10 mm circle is an outer
/// outline: `outside` cuts around it, 5 + 3.175 mm from its centre, so the
/// hole comes out one bit diameter too large. This is why the recipe uses
/// `inside` for the hole.
#[test]
fn hole_targeted_alone_with_outside_is_oversize() {
    let dir = work_dir("hole_targeted_alone_with_outside_is_oversize");
    let dcv = init_star(&dir, STAR_AND_HOLE_SVG);
    let mut project = load_file(&dcv).project;
    let hole = instance_named(&project, "hole").id.clone();
    let centre = hole_centre(&project, "hole");
    let op = as_profile_mut(&mut project.operations[0]);
    op.targets = vec![hole];
    op.side = Side::Outside;
    op.tabs = None;
    // Every cut is checked against the tool-centre circle: no ramp.
    op.entry = Entry::Plunge;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let moves = export_star(&dir);
    let outside_mm = HOLE_RADIUS_MM + BIT_RADIUS_MM;
    let cuts: Vec<&MachineMove> = moves.iter().filter(|m| m.kind.is_cut()).collect();
    assert!(!cuts.is_empty(), "the circle is cut");
    for m in cuts {
        let radius = distance_from(m, centre);
        assert!(
            (radius - outside_mm).abs() <= RADIUS_TOLERANCE_MM,
            "cut at {radius} mm from the centre, not {outside_mm} mm: {m:?}"
        );
    }
}

/// #106: how the project names its post-processor must never change the
/// G-code. With the router switched by hand, the coaster's program is the
/// committed one, byte for byte.
#[test]
fn manual_spindle_export_matches_committed_program() {
    let dir = work_dir("manual_spindle_export_matches_committed_program");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    set_spindle_control(&dcv, SpindleControl::Manual);
    assert_exports_match(&dir, &[], &[("coaster.nc", "coaster-grblhal-manual.nc")]);
}

/// #106: the two-tool coaster's programs, one per tool run, are the
/// committed pair, byte for byte.
#[test]
fn multi_tool_exports_match_committed_programs() {
    let dir = work_dir("multi_tool_exports_match_committed_programs");
    two_tool_coaster(&dir);
    assert_exports_match(
        &dir,
        &[],
        &[
            ("coaster-1-T1.nc", "coaster-two-tool-grblhal-1-T1.nc"),
            ("coaster-2-T2.nc", "coaster-two-tool-grblhal-2-T2.nc"),
        ],
    );
}

/// #106: the two-tool coaster as one program with `M6` is the committed
/// one, byte for byte.
#[test]
fn m6_export_matches_committed_program() {
    let dir = work_dir("m6_export_matches_committed_program");
    let dcv = two_tool_coaster(&dir);
    set_tool_change_mode(&dcv, ToolChangeMode::M6);
    assert_exports_match(
        &dir,
        &[],
        &[("coaster.nc", "coaster-two-tool-grblhal-m6.nc")],
    );
}

/// #106: GRBL's programs, after `--post grbl`, are the committed ones,
/// byte for byte: the coaster with automatic and with manual spindle
/// control, and the two-tool coaster's programs per tool run.
#[test]
fn grbl_exports_match_committed_programs() {
    let dir = work_dir("grbl_exports_match_committed_programs");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    assert_exports_match(
        &dir,
        &["--post", "grbl"],
        &[("coaster.nc", "coaster-grbl.nc")],
    );
    set_spindle_control(&dcv, SpindleControl::Manual);
    assert_exports_match(&dir, &[], &[("coaster.nc", "coaster-grbl-manual.nc")]);

    let dir = work_dir("grbl_exports_match_committed_programs_two_tool");
    two_tool_coaster(&dir);
    assert_exports_match(
        &dir,
        &["--post", "grbl"],
        &[
            ("coaster-1-T1.nc", "coaster-two-tool-grbl-1-T1.nc"),
            ("coaster-2-T2.nc", "coaster-two-tool-grbl-2-T2.nc"),
        ],
    );
}

/// #104: the claims the recipe rests on. A star and its hole drawn as one
/// SVG path import as one instance, which no operation can split; cut
/// `outside` in one operation, the circle is a hole of the star and is cut
/// inside it, 5 − 3.175 mm from its centre. Operations run in file order:
/// with the star's operation first, the whole star is cut before the hole.
#[test]
fn hole_role_is_decided_within_one_operation() {
    let dir = work_dir("hole_role_is_decided_within_one_operation");
    let dcv = init_star(&dir, STAR_WITH_HOLE_ONE_PATH_SVG);
    let mut project = load_file(&dcv).project;
    // Every hole move is checked against the tool-centre circle, so no
    // ramp inside it (the entry has its own tests in dc-cam).
    as_profile_mut(&mut project.operations[0]).entry = Entry::Plunge;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    assert_eq!(project.instances.len(), 1, "one path is one instance");
    let definition = project
        .definitions
        .values()
        .next()
        .expect("the star's definition");
    assert_eq!(definition.profiles.len(), 2, "with both outlines");
    let centre = hole_centre(&project, "star");
    let moves = export_star(&dir);
    let (hole_cuts, star_cuts) = split_cuts(&moves, centre, HOLE_RADIUS_MM);
    assert!(
        !hole_cuts.is_empty() && !star_cuts.is_empty(),
        "both are cut"
    );
    let inside_mm = HOLE_RADIUS_MM - BIT_RADIUS_MM;
    for &k in &hole_cuts {
        let radius = distance_from(&moves[k], centre);
        assert!(
            (radius - inside_mm).abs() <= RADIUS_TOLERANCE_MM,
            "the hole is cut inside, {inside_mm} mm from its centre, not {radius} mm"
        );
    }

    let dir = work_dir("hole_role_is_decided_within_one_operation_order");
    let dcv = init_star(&dir, STAR_AND_HOLE_SVG);
    let mut project = load_file(&dcv).project;
    let star = instance_named(&project, "star").id.clone();
    let hole = instance_named(&project, "hole").id.clone();
    let centre = hole_centre(&project, "hole");
    let mut star_op = as_profile(&project.operations[0]).clone();
    star_op.targets = vec![star];
    star_op.tabs = None;
    let mut hole_op = star_op.clone();
    hole_op.id = OperationId::new("op2");
    hole_op.targets = vec![hole];
    hole_op.side = Side::Inside;
    hole_op.entry = Entry::Plunge;
    project.operations = vec![Operation::Profile(star_op), Operation::Profile(hole_op)];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let moves = export_star(&dir);
    let (hole_cuts, star_cuts) = split_cuts(&moves, centre, HOLE_RADIUS_MM);
    let (Some(&last_star), Some(&first_hole)) = (star_cuts.last(), hole_cuts.first()) else {
        panic!("both are cut");
    };
    assert!(last_star < first_hole, "file order: the star first");
}

/// The `tool_change_mode` of the `.dcv` at `dcv`, as written in the file.
fn written_tool_change_mode(dcv: &Path) -> String {
    let block = post_processor_block(dcv);
    block
        .get("tool_change_mode")
        .and_then(serde_json::Value::as_str)
        .expect("tool_change_mode is a string")
        .to_owned()
}

/// Acceptance 33: the user config's `tool_change_mode` is what `init` and
/// `export --post --reset-settings` write into a new post-processor
/// reference; nothing else reads it, so a plain `export` leaves the
/// project's mode alone (a plain `--post` applies it only to a setting it
/// resets, #277). A `"m6"` default on GRBL, which has no `M6`, is refused
/// naming the config file and the post-processor, so a bad default never
/// lands in a project.
#[test]
fn config_tool_change_mode_seeds_init() {
    let dir = work_dir("config_tool_change_mode_seeds_init");
    // No config file: the definition's default.
    let dcv = init_coaster(&dir);
    assert_eq!(written_tool_change_mode(&dcv), "separate_programs");

    let config = write_config(&dir, "[export]\ntool_change_mode = \"m6\"\n");
    let out = run(&dir, &["init", "coaster.svg", "-o", "m6.dcv"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    assert_eq!(written_tool_change_mode(&dir.join("m6.dcv")), "m6");

    // `export --post --reset-settings` writes the configured mode with the
    // new settings.
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--reset-settings",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "export --post: {}",
        stderr(&out)
    );
    assert_eq!(written_tool_change_mode(&dcv), "m6");

    // A plain `export` does not read the config: the project's mode stays.
    set_tool_change_mode(&dcv, ToolChangeMode::SeparatePrograms);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    assert_eq!(written_tool_change_mode(&dcv), "separate_programs");

    // GRBL has no `M6`: the configured default cannot apply, and the error
    // names the file to fix.
    let out = run(
        &dir,
        &["init", "coaster.svg", "-o", "grbl.dcv", "--post", "grbl"],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "init --post grbl: {}",
        stdout(&out)
    );
    let message = stderr(&out);
    assert!(
        message.contains(&config.display().to_string()) && message.contains("grbl"),
        "{message}"
    );
    assert!(!dir.join("grbl.dcv").exists(), "nothing written");
    let before = std::fs::read_to_string(&dcv).expect("dcv readable");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grbl",
            "--reset-settings",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "export --post grbl: {}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains(&config.display().to_string()),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        before,
        "the project is untouched"
    );
}

/// The filled-in coaster with its post-processor set to `post`, saved.
fn coaster_with_post(dir: &Path, post: dc_project::PostProcessorRef) -> PathBuf {
    let dcv = init_coaster(dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.post_processor = post;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// The bundled reference `id` v`version` with its default settings.
fn default_post(id: &str, version: u32) -> dc_project::PostProcessorRef {
    dc_project::bundled_definition(id, version)
        .expect("definition is bundled")
        .default_ref()
}

/// The latest reference of `id` with its default settings, as JSON.
fn latest_default_block(id: &str) -> serde_json::Value {
    let latest = dc_project::latest_definition(id).expect("definition is bundled");
    serde_json::to_value(latest.default_ref()).expect("reference serializes")
}

/// Acceptance 39 (#277): a grblHAL v1 project with a raised spin-up dwell,
/// exported with `--post grblhal`, is saved at the latest grblHAL version
/// with the dwell kept and every other setting unchanged, its program
/// dwells for it, and the summary says the settings were kept.
#[test]
fn export_post_keeps_dwell() {
    let dir = work_dir("export_post_keeps_dwell");
    let mut post = default_post("grblhal", 1);
    post.spindle_on_dwell_s = 6.0;
    let dcv = coaster_with_post(&dir, post);
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
    let mut expected = latest_default_block("grblhal");
    expected["spindle_on_dwell_s"] = serde_json::json!(6.0);
    assert_eq!(
        serde_json::Value::Object(post_processor_block(&dcv)),
        expected
    );
    let latest = dc_project::latest_definition("grblhal").expect("grblHAL is bundled");
    let report = stdout(&out);
    assert!(
        report.contains(&format!(
            "Updated coaster.dcv: post-processor grblhal v1 → v{}, keeping your settings.",
            latest.version
        )),
        "{report}"
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(
        program.lines().any(|l| l.trim() == "G4 P6.000"),
        "{program}"
    );
}

/// #277: a setting the new post-processor cannot use is reset to its
/// default, never kept to fail, and the summary names it, both values and
/// why: GRBL has no `M6` tool change (#57).
#[test]
fn export_post_reports_reset_setting() {
    let dir = work_dir("export_post_reports_reset_setting");
    let mut post = dc_project::latest_definition("grblhal")
        .expect("grblHAL is bundled")
        .default_ref();
    post.tool_change_mode = ToolChangeMode::M6;
    let dcv = coaster_with_post(&dir, post);
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
    assert_eq!(written_tool_change_mode(&dcv), "separate_programs");
    let report = stdout(&out);
    let line = report
        .lines()
        .find(|l| l.contains("tool_change_mode"))
        .unwrap_or_else(|| panic!("a line names the reset setting: {report}"));
    assert!(
        line.contains("\"m6\" → \"separate_programs\""),
        "both values: {line}"
    );
    assert!(
        line.contains("grbl") && line.contains("no M6"),
        "why: {line}"
    );
}

/// #277: `--reset-settings` keeps the old way: every setting from the new
/// definition's defaults, with the user config's `tool_change_mode` on
/// top. It needs `--post`.
#[test]
fn export_post_reset_settings_restores_defaults() {
    let dir = work_dir("export_post_reset_settings_restores_defaults");
    let mut post = default_post("grblhal", 1);
    post.spindle_on_dwell_s = 6.0;
    post.file_extension = "gcode".into();
    post.spindle_control = SpindleControl::Manual;
    let dcv = coaster_with_post(&dir, post);
    write_config(&dir, "[export]\ntool_change_mode = \"m6\"\n");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--post",
            "grblhal",
            "--reset-settings",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let mut expected = latest_default_block("grblhal");
    expected["tool_change_mode"] = serde_json::json!("m6");
    assert_eq!(
        serde_json::Value::Object(post_processor_block(&dcv)),
        expected
    );
    let report = stdout(&out);
    assert!(report.contains("with its default settings."), "{report}");

    let before = std::fs::read_to_string(&dcv).expect("dcv readable");
    let out = run(&dir, &["export", "coaster.dcv", "--reset-settings"]);
    assert_eq!(out.status.code(), Some(2), "usage error: {}", stdout(&out));
    assert!(stderr(&out).contains("--post"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        before,
        "the project is untouched"
    );
}

/// #277, functional capability 27: the user config's `tool_change_mode`
/// applies only to a setting `--post` resets, so a configured `"m6"` does
/// not override a project's kept `"separate_programs"`.
#[test]
fn export_post_config_applies_only_to_reset_settings() {
    let dir = work_dir("export_post_config_applies_only_to_reset_settings");
    let dcv = coaster_with_post(&dir, default_post("grbl", 1));
    write_config(&dir, "[export]\ntool_change_mode = \"m6\"\n");
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
    assert_eq!(written_tool_change_mode(&dcv), "separate_programs");
    assert_eq!(
        serde_json::Value::Object(post_processor_block(&dcv)),
        latest_default_block("grblhal")
    );
}

/// Acceptance 33: `--one-program` on grblHAL writes one file with `T2 M6`
/// between the tools, and on GRBL is an error naming the post-processor;
/// `--programs-per-tool` on a project in `"m6"` mode writes one file per
/// tool run. Neither flag changes the project file. Both together is a
/// usage error.
#[test]
fn one_program_and_programs_per_tool_flags() {
    let dir = work_dir("one_program_and_programs_per_tool_flags");
    let dcv = two_tool_coaster(&dir);
    let before = std::fs::read_to_string(&dcv).expect("dcv readable");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--one-program",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "--one-program: {}",
        stderr(&out)
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc written");
    assert!(program.lines().any(|l| l.starts_with("T2 M6")), "{program}");
    assert!(!dir.join("coaster-1-T1.nc").exists(), "one file only");
    assert!(
        stdout(&out).contains("sender must stop at M6"),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        before,
        "--one-program is not saved"
    );
    std::fs::remove_file(dir.join("coaster.nc")).expect("program removed");

    // GRBL has no `M6`.
    let mut project = load_file(&dcv).project;
    project.post_processor = dc_project::PostProcessorRef::latest("grbl").expect("grbl is bundled");
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--one-program",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "--one-program on grbl: {}",
        stdout(&out)
    );
    assert!(stderr(&out).contains("`grbl`"), "{}", stderr(&out));
    assert!(!dir.join("coaster.nc").exists(), "nothing written");
    assert!(!dir.join("coaster-1-T1.nc").exists(), "nothing written");

    // `"m6"` in the project, one program per tool run asked for.
    project.post_processor =
        dc_project::PostProcessorRef::latest("grblhal").expect("grblhal is bundled");
    project.post_processor.tool_change_mode = ToolChangeMode::M6;
    let text = save(&project).expect("project saves");
    std::fs::write(&dcv, &text).expect("dcv written");
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--programs-per-tool",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "--programs-per-tool: {}",
        stderr(&out)
    );
    for name in ["coaster-1-T1.nc", "coaster-2-T2.nc"] {
        let run_program = std::fs::read_to_string(dir.join(name)).expect("run program written");
        assert!(!has_code_word(&run_program, "M6"), "{name} has M6");
        assert!(!has_code_word(&run_program, "M61"), "{name} has M61");
    }
    assert!(!dir.join("coaster.nc").exists(), "no single program");
    assert_eq!(
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        text,
        "--programs-per-tool is not saved"
    );

    // The flags contradict each other.
    let out = run(
        &dir,
        &[
            "export",
            "coaster.dcv",
            "--one-program",
            "--programs-per-tool",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "usage error: {}", stderr(&out));
    let usage = stderr(&out);
    assert!(
        usage.contains("--one-program") && usage.contains("--programs-per-tool"),
        "{usage}"
    );
}

/// The index of every line of `program` that cuts: a line with an axis
/// word while the modal motion is `G1`, `G2` or `G3`.
fn cutting_lines(program: &str) -> Vec<usize> {
    let mut cutting = false;
    let mut out = Vec::new();
    for (i, line) in program.lines().enumerate() {
        let words = code_words(line);
        for w in &words {
            match w.as_str() {
                "G0" | "G00" => cutting = false,
                "G1" | "G01" | "G2" | "G02" | "G3" | "G03" => cutting = true,
                _ => {}
            }
        }
        let moves = words.iter().any(|w| w.starts_with(['X', 'Y', 'Z']));
        if cutting && moves {
            out.push(i);
        }
    }
    out
}

/// Acceptance 33 (#252): the plaque (operations Pocket, Pocket, Drill,
/// Profile "Cut out" with T1, then Engrave with T2) exported with
/// `--one-program --post grblhal` is one program with exactly one
/// `T2 M6`, after the Profile's last cutting move and before the
/// Engrave's first. The project file is unchanged (it is saved first in
/// the canonical layout, which `--post` writes back: the committed
/// fixture is hand-formatted).
#[test]
fn plaque_one_program_changes_tool_before_the_engrave() {
    let dir = work_dir("plaque_one_program_changes_tool_before_the_engrave");
    let dcv = copy_fixture(&dir, "plaque.dcv", "plaque.dcv");
    let project = load_file(&dcv).project;
    let names: Vec<&str> = project.operations.iter().map(Operation::name).collect();
    assert_eq!(
        names,
        [
            "Panel",
            "Square recess",
            "Screw holes",
            "Cut out",
            "Border and squiggle"
        ]
    );
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let before = std::fs::read(&dcv).expect("dcv readable");
    let out = run(
        &dir,
        &[
            "export",
            "plaque.dcv",
            "--one-program",
            "--post",
            "grblhal",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let programs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("dir readable")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "nc"))
        .collect();
    assert_eq!(
        programs,
        [dir.join("plaque.nc")],
        "exactly one program: {programs:?}"
    );
    let program = std::fs::read_to_string(dir.join("plaque.nc")).expect("plaque.nc written");
    let lines: Vec<&str> = program.lines().collect();
    let changes: Vec<usize> = (0..lines.len())
        .filter(|&i| code_words(lines[i]).starts_with(&["T2".to_owned(), "M6".to_owned()]))
        .collect();
    assert_eq!(changes.len(), 1, "one T2 M6: {changes:?}\n{program}");
    let change = changes[0];
    let header = |name: &str| {
        lines
            .iter()
            .position(|l| l.starts_with("( Operation ") && l.ends_with(&format!(": {name} )")))
            .unwrap_or_else(|| panic!("no operation comment for {name}:\n{program}"))
    };
    let (profile, engrave) = (header("Cut out"), header("Border and squiggle"));
    assert!(profile < engrave, "the Profile comes before the Engrave");
    let cuts = cutting_lines(&program);
    let last_profile_cut = cuts
        .iter()
        .copied()
        .filter(|&i| profile < i && i < engrave)
        .max()
        .unwrap_or_else(|| panic!("the Profile cuts:\n{program}"));
    let first_engrave_cut = cuts
        .iter()
        .copied()
        .find(|&i| i > engrave)
        .unwrap_or_else(|| panic!("the Engrave cuts:\n{program}"));
    assert!(
        last_profile_cut < change && change < first_engrave_cut,
        "T2 M6 at line {} lies between the Profile's last cut (line {}) and the Engrave's first (line {})",
        change + 1,
        last_profile_cut + 1,
        first_engrave_cut + 1
    );
    assert_eq!(
        std::fs::read(&dcv).expect("dcv readable"),
        before,
        "the project file is unchanged"
    );
}

/// `config path` prints the config directory, the config file and the
/// tool library file, each with whether it exists, so a user can find
/// them (design, "Tool library CLI").
#[test]
fn config_path_prints_locations() {
    let dir = work_dir("config_path_prints_locations");
    let config = config_dir(&dir);
    let expected = |state: &str| {
        vec![
            format!("Config directory: {} ({state})", config.display()),
            format!(
                "Config file:      {} ({state})",
                config.join("config.toml").display()
            ),
            format!(
                "Tool library:     {} ({state})",
                config.join("tools.json").display()
            ),
        ]
    };
    let out = run(&dir, &["config", "path"]);
    assert_eq!(out.status.code(), Some(0), "config path: {}", stderr(&out));
    let lines: Vec<String> = stdout(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines, expected("not found"));

    write_config(&dir, "[export]\ntool_change_mode = \"m6\"\n");
    // `tools import` creates the library file on first write.
    import_sample_library(&dir);
    assert!(
        config.join("tools.json").exists(),
        "the import created the library"
    );
    let out = run(&dir, &["config", "path"]);
    assert_eq!(out.status.code(), Some(0), "config path: {}", stderr(&out));
    let lines: Vec<String> = stdout(&out).lines().map(str::to_owned).collect();
    assert_eq!(lines, expected("exists"));
}

/// The first line of `config path`, the config directory, run with the
/// environment edits `edit` makes.
fn config_dir_line(dir: &Path, edit: impl FnOnce(&mut Command)) -> Output {
    let mut cmd = command(dir);
    cmd.env_remove(CONFIG_DIR_VAR).env_remove("XDG_CONFIG_HOME");
    edit(&mut cmd);
    cmd.args(["config", "path"]).output().expect("binary runs")
}

/// `DREAMCARVE_CONFIG_DIR` replaces the platform directory, which comes
/// from the environment: macOS `$HOME/Library/Application Support`,
/// Windows `%APPDATA%`, elsewhere `$XDG_CONFIG_HOME` or `$HOME/.config`,
/// each with `dreamcarve` under it (design, "User config (CLI only)"). A
/// missing base variable is an error naming it.
#[test]
fn config_dir_env_override() {
    let dir = work_dir("config_dir_env_override");
    let custom = dir.join("elsewhere");
    let out = config_dir_line(&dir, |cmd| {
        cmd.env(CONFIG_DIR_VAR, &custom);
    });
    assert_eq!(out.status.code(), Some(0), "config path: {}", stderr(&out));
    assert_eq!(
        stdout(&out).lines().next().unwrap_or_default(),
        format!("Config directory: {} (not found)", custom.display())
    );

    // Without the override: the platform directory under the base set for
    // this run.
    let base = dir.join("base");
    let (var, platform) = if cfg!(target_os = "windows") {
        ("APPDATA", base.join("dreamcarve"))
    } else if cfg!(target_os = "macos") {
        (
            "HOME",
            base.join("Library")
                .join("Application Support")
                .join("dreamcarve"),
        )
    } else {
        ("HOME", base.join(".config").join("dreamcarve"))
    };
    let out = config_dir_line(&dir, |cmd| {
        cmd.env(var, &base);
    });
    assert_eq!(out.status.code(), Some(0), "config path: {}", stderr(&out));
    assert_eq!(
        stdout(&out).lines().next().unwrap_or_default(),
        format!("Config directory: {} (not found)", platform.display())
    );

    // Where XDG applies, `XDG_CONFIG_HOME` beats `$HOME/.config`.
    if !cfg!(any(target_os = "windows", target_os = "macos")) {
        let xdg = dir.join("xdg");
        let out = config_dir_line(&dir, |cmd| {
            cmd.env(var, &base).env("XDG_CONFIG_HOME", &xdg);
        });
        assert_eq!(out.status.code(), Some(0), "config path: {}", stderr(&out));
        assert_eq!(
            stdout(&out).lines().next().unwrap_or_default(),
            format!(
                "Config directory: {} (not found)",
                xdg.join("dreamcarve").display()
            )
        );
    }

    // No base variable and no override: an error naming the variable.
    let out = config_dir_line(&dir, |cmd| {
        cmd.env_remove(var);
    });
    assert_eq!(out.status.code(), Some(1), "config path: {}", stdout(&out));
    assert!(
        stderr(&out).contains(var) && stderr(&out).contains(CONFIG_DIR_VAR),
        "{}",
        stderr(&out)
    );
}

/// A config file that does not parse fails the commands that read it,
/// naming the file and the line, and nothing is written.
#[test]
fn malformed_config_fails_init_naming_file_and_line() {
    let dir = work_dir("malformed_config_fails_init_naming_file_and_line");
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let config = write_config(&dir, "[export]\ntool_change_mode = m6\n");
    let out = run(&dir, &["init", "coaster.svg"]);
    assert_eq!(out.status.code(), Some(1), "init: {}", stdout(&out));
    let message = stderr(&out);
    assert!(
        message.contains(&format!("{}:2:", config.display())),
        "names the file and line 2: {message}"
    );
    assert!(!dir.join("coaster.dcv").exists(), "nothing written");

    // An unknown key is a typo the user needs to see, not a silent default.
    write_config(&dir, "[export]\ntool_change_mod = \"m6\"\n");
    let out = run(&dir, &["init", "coaster.svg"]);
    assert_eq!(out.status.code(), Some(1), "init: {}", stdout(&out));
    assert!(
        stderr(&out).contains("tool_change_mod"),
        "names the key: {}",
        stderr(&out)
    );
    assert!(!dir.join("coaster.dcv").exists(), "nothing written");
}

// ── DXF import (phase 2, slice 19; functional spec capability 20, ─────────
// ── acceptance test 34) ──────────────────────────────────────────────────

/// The repository's `tests/fixtures/` directory.
fn tests_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Copies the fixture `tests/fixtures/<name>` into `dir` as `as_name`.
fn copy_fixture(dir: &Path, name: &str, as_name: &str) -> PathBuf {
    let dest = dir.join(as_name);
    std::fs::copy(tests_fixture(name), &dest).expect("fixture copied");
    dest
}

/// The one definition named `name` in `project`.
fn definition_named<'a>(project: &'a Project, name: &str) -> &'a dc_project::FeatureDefinition {
    project
        .definitions
        .values()
        .find(|d| d.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = project
                .definitions
                .values()
                .map(|d| d.name.as_str())
                .collect();
            panic!("no definition named {name}; have {names:?}")
        })
}

/// Definition names in `project`, in instance order.
fn definition_names(project: &Project) -> Vec<String> {
    project
        .instances
        .iter()
        .map(|i| {
            project
                .definitions
                .get(&i.definition)
                .expect("instance's definition exists")
                .name
                .clone()
        })
        .collect()
}

/// Acceptance test 34: a DXF with no `$INSUNITS` fails `init` asking for
/// `--units`; with `--units inch` a 1-unit line is 25.4 mm long, and the
/// project's display units are inches.
#[test]
fn init_dxf_without_units_asks_for_flag() {
    let dir = work_dir("init_dxf_without_units_asks_for_flag");
    copy_fixture(&dir, "dxf/no_units.dxf", "part.dxf");

    let out = run(&dir, &["init", "part.dxf"]);
    assert_eq!(out.status.code(), Some(1), "init: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("--units"), "names the flag: {err}");
    assert!(err.contains("mm|inch"), "and its values: {err}");
    assert!(!dir.join("part.dcv").exists(), "nothing is written");

    let out = run(&dir, &["init", "part.dxf", "--units", "inch"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let summary = stdout(&out);
    assert!(summary.contains("part.dxf"), "{summary}");
    assert!(summary.contains("25.4"), "says what one unit is: {summary}");
    let project = load_file(&dir.join("part.dcv")).project;
    assert_eq!(project.display_units, dc_project::Units::Inch);
    assert_eq!(project.definitions.len(), 1);
    let line = &definition_named(&project, "0").profiles[0];
    assert!(!line.is_closed());
    assert_approx_eq!(line.length_mm(), 25.4, 1e-6);
}

/// `--layer` keeps only the named layers; `--join-tolerance` closes a chain
/// the default leaves open, and `check` reports the open one by name
/// (acceptance test 34); the three flags are DXF-only, so on an SVG they
/// are a usage error (exit 2).
#[test]
fn init_dxf_layer_and_join_flags() {
    let dir = work_dir("init_dxf_layer_and_join_flags");

    // Layers: `a` holds two lines, `b` a circle, `0` a circle.
    copy_fixture(&dir, "dxf/layers.dxf", "layers.dxf");
    let out = run(&dir, &["init", "layers.dxf", "--layer", "a"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let project = load_file(&dir.join("layers.dcv")).project;
    assert_eq!(definition_names(&project), ["a-1", "a-2"]);
    let out = run(
        &dir,
        &[
            "init",
            "layers.dxf",
            "-o",
            "two-layers.dcv",
            "--layer",
            "b",
            "--layer",
            "0",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let project = load_file(&dir.join("two-layers.dcv")).project;
    assert_eq!(definition_names(&project), ["b", "0"]);

    // Joining: four lines whose last end stops 0.5 mm short of the start.
    copy_fixture(&dir, "dxf/stitch_gap.dxf", "gap.dxf");
    let out = run(&dir, &["init", "gap.dxf"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let project = load_file(&dir.join("gap.dcv")).project;
    assert_eq!(definition_names(&project), ["box"]);
    let chain = &definition_named(&project, "box").profiles[0];
    assert!(!chain.is_closed(), "0.5 mm is beyond the 0.01 mm default");
    assert_eq!(chain.vertices().len(), 5);
    assert!(
        project.operations[0].targets().is_empty(),
        "an open chain is not targeted"
    );
    fill_in(&dir.join("gap.dcv"), 1500.0, 500.0, 18000);
    let out = run(&dir, &["check", "gap.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    let line = report
        .lines()
        .find(|l| l.contains("box"))
        .unwrap_or_else(|| panic!("check names the open chain: {report}"));
    assert!(line.contains("open"), "says it is open: {line}");

    let out = run(
        &dir,
        &[
            "init",
            "gap.dxf",
            "-o",
            "joined.dcv",
            "--join-tolerance",
            "1",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let project = load_file(&dir.join("joined.dcv")).project;
    let shape = &definition_named(&project, "box").profiles[0];
    assert!(shape.is_closed(), "joined within 1 mm and closed");
    assert_eq!(shape.vertices().len(), 4);
    assert_eq!(project.operations[0].targets().len(), 1);

    // The flags mean nothing for an SVG.
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    for flags in [
        &["--layer", "a"][..],
        &["--join-tolerance", "1"][..],
        &["--layer", "a", "--join-tolerance", "1"][..],
    ] {
        let mut args = vec!["init", "coaster.svg"];
        args.extend_from_slice(flags);
        let out = run(&dir, &args);
        assert_eq!(out.status.code(), Some(2), "{flags:?}: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            err.contains("DXF"),
            "{flags:?}: says the flag is for DXF: {err}"
        );
        assert!(
            !dir.join("coaster.dcv").exists(),
            "{flags:?}: nothing written"
        );
    }
}

/// Acceptance test 34, the `init` half: `plaque.dxf` and `plaque.svg`
/// (the phase 2 test part) produce projects with the same shapes, named
/// alike, in the same order, placed alike, within 0.01 mm. The export half
/// is `plaque_dxf_exports_same_programs_as_svg`.
#[test]
fn plaque_dxf_imports_same_shapes_as_svg() {
    let dir = work_dir("plaque_dxf_imports_same_shapes_as_svg");
    copy_fixture(&dir, "plaque.dxf", "plaque.dxf");
    copy_fixture(&dir, "plaque.svg", "plaque.svg");
    let out = run(&dir, &["init", "plaque.dxf", "-o", "from-dxf.dcv"]);
    assert_eq!(out.status.code(), Some(0), "init dxf: {}", stderr(&out));
    assert!(
        stderr(&out).is_empty(),
        "the plaque imports without warnings: {}",
        stderr(&out)
    );
    let out = run(&dir, &["init", "plaque.svg", "-o", "from-svg.dcv"]);
    assert_eq!(out.status.code(), Some(0), "init svg: {}", stderr(&out));

    let from_dxf = load_file(&dir.join("from-dxf.dcv")).project;
    let from_svg = load_file(&dir.join("from-svg.dcv")).project;
    assert_eq!(definition_names(&from_dxf), definition_names(&from_svg));
    assert_eq!(from_dxf.definitions.len(), 10);
    assert_approx_eq!(from_dxf.stock.width_mm, from_svg.stock.width_mm, 0.01);
    assert_approx_eq!(from_dxf.stock.height_mm, from_svg.stock.height_mm, 0.01);
    for (a, b) in from_dxf.instances.iter().zip(&from_svg.instances) {
        assert_eq!(a.name, b.name);
        let (ta, tb): ([f64; 6], [f64; 6]) = (a.transform.into(), b.transform.into());
        for (x, y) in ta.iter().zip(&tb) {
            assert_approx_eq!(*x, *y, 0.01);
        }
        let da = definition_named(&from_dxf, &a.name);
        let db = definition_named(&from_svg, &b.name);
        assert_eq!(da.profiles.len(), 1, "{}", a.name);
        assert_eq!(db.profiles.len(), 1, "{}", b.name);
        assert_eq!(
            da.profiles[0].is_closed(),
            db.profiles[0].is_closed(),
            "{}",
            a.name
        );
        let (ra, rb) = (
            da.profiles[0].bounds().expect("bounds"),
            db.profiles[0].bounds().expect("bounds"),
        );
        for (x, y) in [
            (ra.min.x, rb.min.x),
            (ra.min.y, rb.min.y),
            (ra.max.x, rb.max.x),
            (ra.max.y, rb.max.y),
        ] {
            assert_approx_eq!(x, y, 0.01);
        }
    }
    // The DXF keeps its arcs: the pocket's corners and the island.
    let pocket = &definition_named(&from_dxf, "pocket").profiles[0];
    assert_eq!(pocket.segments().filter(|s| !s.is_line()).count(), 4);
    let island = &definition_named(&from_dxf, "island").profiles[0];
    assert_eq!(island.vertices().len(), 2);
    // Both target the same shapes with the Profile `init` writes.
    assert_eq!(
        from_dxf.operations[0].targets().len(),
        from_svg.operations[0].targets().len()
    );
    assert_eq!(
        from_dxf.operations[0].targets().len(),
        9,
        "all but the squiggle"
    );
}

/// How far two exports of one part may differ, mm: each drawing is within
/// the 0.01 mm flatten tolerance of the true curve, so two flattenings of
/// one curve (the squiggle: an SVG cubic and a DXF SPLINE) are within
/// twice that of each other, and arc fitting stays within 0.01 mm of each.
/// Measured: 0.0165 mm on the squiggle, under 0.01 mm everywhere else.
const PARITY_TOLERANCE_MM: f64 = 0.03;

/// The sampled tool-centre path of `m`, from its start (excluded) to its
/// end: lines every 0.5 mm, arcs finely enough that the chords are within
/// 0.0005 mm of the arc.
#[allow(
    clippy::cast_possible_truncation,
    reason = "sample counts are small positive whole numbers"
)]
fn sample_move(m: &MachineMove) -> Vec<[f64; 3]> {
    let (fx, fy, fz) = (
        m.from.x.unwrap_or(0.0),
        m.from.y.unwrap_or(0.0),
        m.from.z.unwrap_or(0.0),
    );
    let (tx, ty, tz) = (
        m.to.x.unwrap_or(fx),
        m.to.y.unwrap_or(fy),
        m.to.z.unwrap_or(fz),
    );
    let arc = matches!(
        m.kind,
        dc_gcode::parse::MotionKind::ArcCw | dc_gcode::parse::MotionKind::ArcCcw
    );
    let (Some(i), Some(j), true) = (m.arc.i, m.arc.j, arc) else {
        let length = (tx - fx).hypot(ty - fy).hypot(tz - fz);
        let n = (length / 0.5).ceil().max(1.0) as usize;
        return (1..=n)
            .map(|k| {
                let t = k as f64 / n as f64;
                [fx + (tx - fx) * t, fy + (ty - fy) * t, fz + (tz - fz) * t]
            })
            .collect();
    };
    let (cx, cy) = (fx + i, fy + j);
    let r = i.hypot(j);
    let a0 = (fy - cy).atan2(fx - cx);
    let a1 = (ty - cy).atan2(tx - cx);
    let tau = std::f64::consts::TAU;
    let ccw = m.kind == dc_gcode::parse::MotionKind::ArcCcw;
    let mut sweep = if ccw {
        (a1 - a0).rem_euclid(tau)
    } else {
        (a0 - a1).rem_euclid(tau)
    };
    if sweep < 1e-9 {
        sweep = tau;
    }
    let step = (8.0 * r * 0.0005).sqrt().min(0.5);
    let n = (r * sweep / step).ceil().max(1.0) as usize;
    (1..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let a = if ccw { a0 + sweep * t } else { a0 - sweep * t };
            [cx + r * a.cos(), cy + r * a.sin(), fz + (tz - fz) * t]
        })
        .collect()
}

/// The distance from `p` to the polyline through `path`, mm.
#[allow(clippy::indexing_slicing, reason = "windows of two, and a path of one")]
fn distance_to_path(p: [f64; 3], path: &[[f64; 3]]) -> f64 {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut best = f64::INFINITY;
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let ab = sub(b, a);
        let len2 = dot(ab, ab);
        let t = if len2 > 0.0 {
            (dot(sub(p, a), ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
        best = best.min(dot(sub(p, q), sub(p, q)).sqrt());
    }
    if path.len() == 1 {
        best = dot(sub(p, path[0]), sub(p, path[0])).sqrt();
    }
    best
}

/// A program cut into what a reader compares: its lines that make no move
/// (comments, spindle, dwell, tool change, modes), in order; and, per
/// operation, where each rapid ends and each run of cutting moves between
/// two rapids, sampled, with the feeds it uses in order.
struct ProgramShape {
    words: Vec<String>,
    operations: Vec<OperationShape>,
}

#[derive(Default)]
struct OperationShape {
    rapids: Vec<[f64; 3]>,
    runs: Vec<(Vec<[f64; 3]>, Vec<f64>)>,
}

fn program_shape(program: &str) -> ProgramShape {
    let words = program
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.starts_with('(') || !l.split_whitespace().any(|w| w.starts_with(['X', 'Y', 'Z']))
        })
        .map(str::to_owned)
        .collect();
    // The operation each line belongs to: 0 before the first.
    let mut operation_of_line = vec![0_usize];
    let mut current = 0;
    for line in program.lines() {
        if line.starts_with("( Operation ") {
            current += 1;
        }
        operation_of_line.push(current);
    }
    let mut operations: Vec<OperationShape> = Vec::new();
    operations.resize_with(current + 1, OperationShape::default);
    let mut open = false;
    let mut last_operation = 0;
    for m in dc_gcode::parse_motion(program).expect("program parses") {
        let index = operation_of_line.get(m.line).copied().unwrap_or(current);
        if index != last_operation {
            open = false;
            last_operation = index;
        }
        let operation = operations.get_mut(index).expect("an operation per line");
        let end = [
            m.to.x.unwrap_or(0.0),
            m.to.y.unwrap_or(0.0),
            m.to.z.unwrap_or(0.0),
        ];
        if !m.kind.is_cut() {
            operation.rapids.push(end);
            open = false;
            continue;
        }
        if !open {
            let start = [
                m.from.x.unwrap_or(0.0),
                m.from.y.unwrap_or(0.0),
                m.from.z.unwrap_or(0.0),
            ];
            operation.runs.push((vec![start], Vec::new()));
            open = true;
        }
        let run = operation.runs.last_mut().expect("a run is open");
        run.0.extend(sample_move(&m));
        let feed = m.feed.expect("a cut has a feed");
        if run.1.last().is_none_or(|f| (f - feed).abs() > 0.01) {
            run.1.push(feed);
        }
    }
    ProgramShape { words, operations }
}

/// Whether two points are within 0.01 mm on every axis.
fn same_point(p: &[f64; 3], q: &[f64; 3]) -> bool {
    p.iter().zip(q).all(|(u, v)| (u - v).abs() <= 0.01)
}

/// The largest distance from a point of either run to the other run, mm.
fn runs_apart(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    a.iter()
        .map(|&p| distance_to_path(p, b))
        .chain(b.iter().map(|&p| distance_to_path(p, a)))
        .fold(0.0, f64::max)
}

/// #125, acceptance test 34's export half: the finished plaque project
/// built from `plaque.dxf` (`tests/fixtures/plaque.dcv`, the quick start's
/// result) and from `plaque.svg` (`tests/fixtures/plaque-svg.dcv`, the
/// same steps on the SVG twin) export the same programs: the same files
/// in the same order; in each, the same lines that make no move (header,
/// operations in order, tools, spindle, dwells, footer); and in each
/// operation the same rapids within 0.01 mm and the same runs of cutting
/// moves, in the same order, at the same feeds, each run's path within
/// `PARITY_TOLERANCE_MM` of its counterpart's both ways. The SVG's
/// coordinates carry noise of about 1e-6 mm (usvg reads them as `f32`);
/// nearest-first ties within `NEAREST_TIE_MM` keep drawing order (#215),
/// so the noise no longer settles a tie between two equally near
/// corner-relief notches the other way. The squiggle is flattened differently from an SVG cubic and a DXF SPLINE,
/// and the DXF keeps exact arcs where the SVG's are rebuilt from Bézier
/// curves; nothing else differs.
#[test]
#[allow(clippy::indexing_slicing, reason = "two exports, checked above")]
fn plaque_dxf_exports_same_programs_as_svg() {
    let dir = work_dir("plaque_dxf_exports_same_programs_as_svg");
    let mut exported: Vec<Vec<(String, String)>> = Vec::new();
    for (fixture, sub) in [("plaque.dcv", "dxf"), ("plaque-svg.dcv", "svg")] {
        let folder = dir.join(sub);
        std::fs::create_dir_all(&folder).expect("folder created");
        copy_fixture(&folder, fixture, "plaque.dcv");
        let out = run(&folder, &["export", "plaque.dcv", "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{sub}: {}", stderr(&out));
        let mut programs: Vec<(String, String)> = std::fs::read_dir(&folder)
            .expect("folder readable")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "nc"))
            .map(|p| {
                let name = p
                    .file_name()
                    .expect("a file")
                    .to_string_lossy()
                    .into_owned();
                (name, std::fs::read_to_string(&p).expect("program readable"))
            })
            .collect();
        programs.sort();
        exported.push(programs);
    }
    assert_eq!(exported.len(), 2);
    let (from_dxf, from_svg) = (&exported[0], &exported[1]);
    let names = |p: &[(String, String)]| p.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    assert_eq!(names(from_dxf), ["plaque-1-T1.nc", "plaque-2-T2.nc"]);
    assert_eq!(names(from_dxf), names(from_svg));

    for ((name, dxf), (_, svg)) in from_dxf.iter().zip(from_svg) {
        let (a, b) = (program_shape(dxf), program_shape(svg));
        assert_eq!(a.words, b.words, "{name}: the lines that make no move");
        assert_eq!(a.operations.len(), b.operations.len(), "{name}");
        for (k, (oa, ob)) in a.operations.iter().zip(&b.operations).enumerate() {
            let at = format!("{name}, operation {k}");
            assert_eq!(oa.rapids.len(), ob.rapids.len(), "{at}: rapids");
            for (i, (p, q)) in oa.rapids.iter().zip(&ob.rapids).enumerate() {
                assert!(
                    same_point(p, q),
                    "{at}: rapid {i} ends at {p:?} from the DXF, {q:?} from the SVG"
                );
            }
            assert_eq!(oa.runs.len(), ob.runs.len(), "{at}: runs of cuts");
            for (i, ((path, feeds), (other, other_feeds))) in
                oa.runs.iter().zip(&ob.runs).enumerate()
            {
                assert!(
                    same_point(&path[0], &other[0]),
                    "{at}: run {i} starts at {:?} from the DXF, {:?} from the SVG",
                    path[0],
                    other[0]
                );
                assert!(
                    feeds.len() == other_feeds.len()
                        && feeds
                            .iter()
                            .zip(other_feeds)
                            .all(|(u, v)| (u - v).abs() <= 0.01),
                    "{at}: run {i} at feeds {feeds:?} from the DXF, {other_feeds:?} from the SVG"
                );
                let apart_mm = runs_apart(path, other);
                assert!(
                    apart_mm <= PARITY_TOLERANCE_MM,
                    "{at}: run {i} from {:?} is {apart_mm} mm from the SVG's",
                    path[0]
                );
            }
        }
    }
}

/// The SVG of acceptance test 25: a closed rectangle and an open polyline.
const ENGRAVE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <rect id="border" x="10" y="10" width="60" height="40"/>
  <path id="squiggle" d="M 20 80 L 40 70 L 60 80 L 80 70"/>
</svg>
"#;

/// The placed vertices of the instance named `name`, project space.
fn placed_vertices(project: &Project, name: &str) -> Vec<(f64, f64)> {
    let instance = instance_named(project, name);
    let definition = project
        .definitions
        .get(&instance.definition)
        .expect("the instance has a definition");
    definition
        .profiles
        .iter()
        .flat_map(|profile| {
            instance
                .transform
                .apply_polyline(profile)
                .expect("the instance places")
                .vertices()
                .iter()
                .map(|v| (v.x, v.y))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `m` ends within 0.001 mm of one of `points` in XY.
fn ends_on(m: &MachineMove, points: &[(f64, f64)]) -> bool {
    let x = m.to.x.expect("X is known after the first move");
    let y = m.to.y.expect("Y is known after the first move");
    points
        .iter()
        .any(|&(px, py)| (x - px).abs() <= 1e-3 && (y - py).abs() <= 1e-3)
}

/// Whether `m` moves in XY (a cut along the drawing rather than a plunge).
fn moves_xy(m: &MachineMove) -> bool {
    match (m.from.x, m.from.y, m.to.x, m.to.y) {
        (Some(fx), Some(fy), Some(tx), Some(ty)) => {
            (fx - tx).abs() > 1e-9 || (fy - ty).abs() > 1e-9
        }
        _ => true,
    }
}

/// Acceptance test 25: an open polyline and a closed rectangle engraved
/// 1 mm deep in 0.5 mm passes with a 60° V-bit and `alternate_direction`.
/// Every cut along the drawing ends on one of its vertices (the tool
/// centre is on the line), each path is cut at two depths, the open path's
/// second pass runs backwards with no rapid between its passes, the
/// rectangle's passes run the same way both times, and the program passes
/// the safety rules: every rapid at safe Z and nothing below the stock
/// bottom. Z zero is the stock top, as `init` writes it.
#[test]
fn engrave_exports_and_passes_safety() {
    let dir = work_dir("engrave_exports_and_passes_safety");
    std::fs::write(dir.join("plaque.svg"), ENGRAVE_SVG).expect("svg written");
    let out = run(&dir, &["init", "plaque.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("plaque.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.tools[0].cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    project.operations = vec![Operation::Engrave(EngraveOp {
        id: OperationId::new("op1"),
        name: "Engrave".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: project.tools[0].id.clone(),
        depth_mm: Some(1.0),
        step_down_mm: Some(0.5),
        alternate_direction: true,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    })];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let border = placed_vertices(&project, "border");
    let squiggle = placed_vertices(&project, "squiggle");

    let check = run(&dir, &["check", "plaque.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "check: {report}");
    assert!(report.contains("0 error(s)"), "{report}");
    assert!(
        !report.contains("open_profile"),
        "the engraved open path is not reported as never cut: {report}"
    );
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("plaque.nc")).expect("plaque.nc written");
    let moves = dc_gcode::parse_motion(&program).expect("program parses");

    // Every cut along the drawing ends on a vertex of it.
    let traces: Vec<(usize, &MachineMove)> = moves
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind.is_cut() && moves_xy(m))
        .collect();
    assert!(!traces.is_empty(), "the program cuts: {program}");
    for (k, m) in &traces {
        assert!(
            ends_on(m, &border) || ends_on(m, &squiggle),
            "cut {k} leaves the drawing: {m:?}"
        );
    }
    // Two passes, 0.5 mm and 1 mm below the stock top.
    let mut depths: Vec<f64> = traces.iter().filter_map(|(_, m)| m.to.z).collect();
    depths.sort_by(f64::total_cmp);
    depths.dedup_by(|a, b| (*a - *b).abs() <= 1e-6);
    assert_eq!(depths.len(), 2, "{depths:?}");
    assert_approx_eq!(depths[0], -1.0, 1e-6);
    assert_approx_eq!(depths[1], -0.5, 1e-6);

    // The cuts of one path at one depth: each one's index and end point.
    let pass = |points: &[(f64, f64)], z: f64| -> Vec<(usize, (f64, f64))> {
        traces
            .iter()
            .filter(|(_, m)| ends_on(m, points) && m.to.z.is_some_and(|mz| (mz - z).abs() <= 1e-6))
            .map(|(k, m)| (*k, (m.to.x.unwrap_or(0.0), m.to.y.unwrap_or(0.0))))
            .collect()
    };
    let same = |a: &[(usize, (f64, f64))], b: &[(usize, (f64, f64))]| -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|((_, p), (_, q))| (p.0 - q.0).abs() <= 1e-3 && (p.1 - q.1).abs() <= 1e-3)
    };
    let (s1, s2) = (pass(&squiggle, -0.5), pass(&squiggle, -1.0));
    assert_eq!(s1.len(), 3, "three cuts along the squiggle: {s1:?}");
    assert_eq!(s2.len(), 3, "three cuts back along it: {s2:?}");
    // Forwards the cuts end on vertices 1, 2, 3; backwards on 2, 1, 0.
    let mut backwards: Vec<(usize, (f64, f64))> = s1.iter().rev().skip(1).copied().collect();
    backwards.push((0, squiggle[0]));
    assert!(
        same(&backwards, &s2),
        "the second pass runs backwards: {s1:?} then {s2:?}"
    );
    let (Some(&(last_first, _)), Some(&(first_second, _))) = (s1.last(), s2.first()) else {
        panic!("both squiggle passes are cut");
    };
    assert!(last_first < first_second, "the first pass is cut first");
    for m in &moves[last_first + 1..first_second] {
        assert!(
            m.kind.is_cut(),
            "no rapid between the squiggle's passes: {m:?}"
        );
    }
    let (b1, b2) = (pass(&border, -0.5), pass(&border, -1.0));
    assert_eq!(b1.len(), 4, "four cuts round the border: {b1:?}");
    assert!(
        same(&b1, &b2),
        "the border's passes run the same way: {b1:?} then {b2:?}"
    );

    // Safety: rapids at safe Z (5 mm above the stock top), except a rapid
    // straight down from there to 1 mm above the stock top or above the
    // pass before's floor, before a plunge (#227); nothing below the stock
    // bottom.
    for (k, m) in moves.iter().enumerate() {
        let z = m.to.z.expect("Z is known after the first move");
        assert!(
            z >= -MEASURED_THICKNESS_MM - 1e-6,
            "move {k} below the stock: {m:?}"
        );
        if !m.kind.is_cut() {
            let down = m.from.x == m.to.x && m.from.y == m.to.y && m.from.z >= Some(5.0 - 1e-6);
            let floor = if down { -0.5 + 1.0 } else { 5.0 };
            assert!(z >= floor - 1e-6, "rapid {k} below safe Z: {m:?}");
            if down && z < 5.0 - 1e-6 {
                let next = &moves[k + 1];
                assert!(
                    next.kind.is_cut() && next.to.z < Some(z),
                    "a plunge follows rapid {k}: {next:?}"
                );
            }
        }
    }
}

/// The warning lines of `report`: those starting `warning:` or carrying
/// `: warning: ` after a file position.
fn warning_lines(report: &str) -> Vec<&str> {
    report
        .lines()
        .filter(|l| l.starts_with("warning:") || l.contains(": warning: "))
        .collect()
}

/// The `snake_case` code of a printed warning line, the word between
/// `warning: ` and the next `: `, when it is one.
fn warning_code(line: &str) -> Option<&str> {
    let rest = line.split_once("warning: ")?.1;
    let (code, _) = rest.split_once(": ")?;
    let is_code = !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    is_code.then_some(code)
}

/// Acceptance test 32 (research O8, D7): the star cut `outside` through
/// the stock with no tabs, then its hole cut `inside`: `check` warns
/// `cut_inside_freed_part` naming both operations and still exits 0, since
/// the order is the user's to keep. With a second tool on the hole and a
/// third operation back on the first tool (T1, T2, T1), it also hints
/// `tool_change_avoidable` for T1, naming its two operations.
#[test]
fn check_reports_order_warnings() {
    let dir = work_dir("check_reports_order_warnings");
    let dcv = init_star(&dir, STAR_AND_HOLE_SVG);
    let mut project = load_file(&dcv).project;
    let star = instance_named(&project, "star").id.clone();
    let hole = instance_named(&project, "hole").id.clone();
    let mut star_op = as_profile(&project.operations[0]).clone();
    star_op.name = "Star".into();
    star_op.targets = vec![star];
    star_op.side = Side::Outside;
    star_op.tabs = None;
    let mut hole_op = star_op.clone();
    hole_op.id = OperationId::new("op2");
    hole_op.name = "Hole".into();
    hole_op.targets = vec![hole];
    hole_op.side = Side::Inside;
    project.operations = vec![
        Operation::Profile(star_op.clone()),
        Operation::Profile(hole_op.clone()),
    ];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "star.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "warnings exit 0: {report}");
    let freed: Vec<&str> = warning_lines(&report)
        .into_iter()
        .filter(|l| warning_code(l) == Some("cut_inside_freed_part"))
        .collect();
    assert_eq!(freed.len(), 1, "one order warning: {report}");
    let line = freed[0];
    assert!(
        line.contains("`op1`"),
        "names the freeing operation: {line}"
    );
    assert!(line.contains("`op2`"), "names the later operation: {line}");
    assert!(line.contains("`hole`"), "names the shape: {line}");
    assert!(
        !report.contains("tool_change_avoidable"),
        "one tool, no hint: {report}"
    );

    let mut second = project.tools[0].clone();
    second.id = ToolId::new("t2");
    second.name = "second bit".into();
    project.tools.push(second);
    hole_op.tool = ToolId::new("t2");
    let mut again = star_op.clone();
    again.id = OperationId::new("op3");
    again.name = "Star again".into();
    again.side = Side::OnLine;
    again.depth_mm = Some(1.0);
    project.operations = vec![
        Operation::Profile(star_op),
        Operation::Profile(hole_op),
        Operation::Profile(again),
    ];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "star.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "hints exit 0: {report}");
    let hints: Vec<&str> = warning_lines(&report)
        .into_iter()
        .filter(|l| warning_code(l) == Some("tool_change_avoidable"))
        .collect();
    assert_eq!(hints.len(), 1, "one hint: {report}");
    let line = hints[0];
    assert!(line.contains("`t1`"), "names the tool: {line}");
    assert!(
        line.contains("`op1`") && line.contains("`op3`"),
        "names the tool's operations: {line}"
    );
    assert!(!line.contains("`op2`"), "not the other tool's: {line}");

    let out = run(&dir, &["export", "star.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stderr(&out);
    for code in ["cut_inside_freed_part", "tool_change_avoidable"] {
        assert!(
            warning_lines(&report)
                .iter()
                .any(|l| warning_code(l) == Some(code)),
            "export prints {code}: {report}"
        );
    }
}

/// #269: the plaque engraves 1 mm deep after its tabbed cut-out, a light
/// cut on a tab-held part that is normal practice, so `check` gives it no
/// order warning: its five warnings are the unused tool and the four
/// drilled holes, as the quick start shows.
#[test]
fn plaque_engrave_after_tabbed_cut_out_is_quiet() {
    let dir = work_dir("plaque_engrave_after_tabbed_cut_out_is_quiet");
    copy_fixture(&dir, "plaque.dcv", "plaque.dcv");
    let out = run(&dir, &["check", "plaque.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert!(
        !report.contains("cut_inside_tab_held_part"),
        "the engrave is light: {report}"
    );
    assert!(report.contains("0 error(s), 5 warning(s)"), "{report}");
}

/// #269: the maintainer's two-operation star from before format v9
/// (`tests/manual/star_with_hole`): op1 cuts the star `outside` through
/// the stock with `init`'s tabs, then op2 cuts its hole `inside`. The hole
/// is cut in a star held only by its tabs, so `check` and `export` warn
/// `cut_inside_tab_held_part` once, naming both operations and both
/// shapes, and `check` still exits 0. The tabs hold the star, so it is not
/// `cut_inside_freed_part`. The hole first, or the star and its hole in
/// `init`'s one operation (#109), is quiet.
#[test]
fn check_warns_hole_cut_in_tab_held_star() {
    let dir = work_dir("check_warns_hole_cut_in_tab_held_star");
    let dcv = init_star(&dir, STAR_AND_HOLE_SVG);
    let mut project = load_file(&dcv).project;
    let one_operation = project.operations.clone();
    let star = instance_named(&project, "star").id.clone();
    let hole = instance_named(&project, "hole").id.clone();
    let mut star_op = as_profile(&project.operations[0]).clone();
    star_op.name = "Star".into();
    star_op.targets = vec![star];
    star_op.side = Side::Outside;
    assert!(star_op.tabs.is_some(), "init gives the profile tabs");
    let mut hole_op = star_op.clone();
    hole_op.id = OperationId::new("op2");
    hole_op.name = "Hole".into();
    hole_op.targets = vec![hole];
    hole_op.side = Side::Inside;
    hole_op.tabs = None;
    let order_codes = ["cut_inside_tab_held_part", "cut_inside_freed_part"];
    let order_lines = |report: &str| -> Vec<String> {
        warning_lines(report)
            .into_iter()
            .filter(|l| warning_code(l).is_some_and(|c| order_codes.contains(&c)))
            .map(str::to_owned)
            .collect()
    };

    project.operations = vec![
        Operation::Profile(star_op.clone()),
        Operation::Profile(hole_op.clone()),
    ];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["check", "star.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "warnings exit 0: {report}");
    let found = order_lines(&report);
    assert_eq!(found.len(), 1, "one order warning: {report}");
    let line = &found[0];
    assert_eq!(
        warning_code(line),
        Some("cut_inside_tab_held_part"),
        "{line}"
    );
    for needle in ["`op1`", "`op2`", "`hole`", "`star`"] {
        assert!(line.contains(needle), "names {needle}: {line}");
    }
    let out = run(&dir, &["export", "star.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    assert_eq!(order_lines(&stderr(&out)), found, "export prints it too");

    for (what, operations) in [
        (
            "the hole first",
            vec![Operation::Profile(hole_op), Operation::Profile(star_op)],
        ),
        ("one operation", one_operation),
    ] {
        project.operations = operations;
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
        let out = run(&dir, &["check", "star.dcv"]);
        let report = stdout(&out);
        assert_eq!(out.status.code(), Some(0), "{what}: {report}");
        assert!(order_lines(&report).is_empty(), "{what} is quiet: {report}");
    }
}

/// Capability 25: every warning `check` and `export` print is
/// `warning: <code>: <message>`, the code a stable `snake_case` word,
/// after the file position when the warning is about one value. The
/// project raises toolpath warnings (an open path, a duplicate, a hole the
/// bit does not fit, tabs that overlap, cover most of a hole and sit above
/// the stock top), order warnings (a hole cut after the tabbed op1 and
/// after the untabbed op2), and a validation warning (an unused tool), and
/// each code is the one the Warnings chapter lists.
#[test]
fn warnings_print_code_and_message() {
    let dir = work_dir("warnings_print_code_and_message");
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
    let outline = instance_named(&project, "outline").id.clone();
    let small_hole = instance_named(&project, "small-hole").id.clone();
    let first = as_profile_mut(&mut project.operations[0]);
    if let Some(tabs) = &mut first.tabs {
        tabs.height_mm = thickness_mm + 1.0;
        // Two tabs 5 mm apart overlap on every outline; with the third,
        // they would cover more than half of the small hole's path, so
        // the hole keeps the first two (#109).
        tabs.placement = TabPlacement::AtDistances {
            distances_mm: vec![10.0, 15.0, 40.0],
        };
        tabs.on_holes = true;
    }
    let mut outline_op = first.clone();
    outline_op.id = OperationId::new("op2");
    outline_op.name = "Outline".into();
    outline_op.targets = vec![outline];
    outline_op.side = Side::Outside;
    outline_op.tabs = None;
    let mut hole_op = outline_op.clone();
    hole_op.id = OperationId::new("op3");
    hole_op.name = "Small hole".into();
    hole_op.targets = vec![small_hole];
    hole_op.side = Side::Inside;
    project
        .operations
        .extend([Operation::Profile(outline_op), Operation::Profile(hole_op)]);
    let mut spare = project.tools[0].clone();
    spare.id = ToolId::new("t2");
    spare.name = "Spare bit".into();
    project.tools.push(spare);
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let check = run(&dir, &["check", "part.dcv"]);
    assert_eq!(check.status.code(), Some(0), "check: {}", stdout(&check));
    let export = run(&dir, &["export", "part.dcv", "--date", FIXED_DATE]);
    assert_eq!(export.status.code(), Some(0), "export: {}", stderr(&export));
    for report in [stdout(&check), stderr(&export)] {
        let lines = warning_lines(&report);
        assert!(!lines.is_empty(), "warnings printed: {report}");
        let mut codes = Vec::new();
        for line in &lines {
            let Some(code) = warning_code(line) else {
                panic!("`warning: <code>: <message>`: {line}");
            };
            let message = line.split_once(&format!("{code}: ")).expect("code found").1;
            assert!(
                message.chars().next().is_some_and(|c| c != ' '),
                "a message follows the code: {line}"
            );
            codes.push(code);
        }
        // The open path is reported by validation (`init` never targets
        // one), so its code is `open_profile`, not `open_path_skipped`.
        for expected in [
            "open_profile",
            "duplicate_shape_dropped",
            "tool_too_large",
            "tabs_overlap",
            "tabs_cover_most_of_contour",
            "tabs_outside_cut_range",
            "cut_inside_freed_part",
            "cut_inside_tab_held_part",
            "unused_tool",
        ] {
            assert!(
                codes.contains(&expected),
                "{expected} is printed; codes were {codes:?}: {report}"
            );
        }
    }
}

// ── Tool library (phase 2, capability 21; design, "Tool library CLI") ──

/// The library file every test's `dreamcarve` reads and writes.
fn library_file(dir: &Path) -> PathBuf {
    config_dir(dir).join("tools.json")
}

/// A 1/4" end mill with every value and a tool number, the tool
/// acceptance test 35 adds to a project.
fn quarter_inch_end_mill() -> dc_project::Tool {
    dc_project::Tool {
        id: ToolId::new("t1"),
        name: "1/4 end mill".into(),
        number: Some(3),
        cutter: dc_project::ToolKind::EndMill,
        diameter_mm: 6.35,
        flutes: 2,
        feed_mm_min: Some(1500.0),
        plunge_mm_min: Some(500.0),
        rpm: Some(18000),
        step_down_mm: Some(3.0),
        step_over_fraction: Some(0.45),
    }
}

/// A 60° V-bit tagged as coming from a Fusion library.
fn vbit_60() -> dc_project::Tool {
    dc_project::Tool {
        id: ToolId::new("t1"),
        name: "60° V-bit".into(),
        number: None,
        cutter: dc_project::ToolKind::VBit {
            included_angle_deg: 60.0,
        },
        diameter_mm: 12.7,
        flutes: 2,
        feed_mm_min: Some(800.0),
        plunge_mm_min: Some(200.0),
        rpm: Some(16000),
        step_down_mm: Some(1.0),
        step_over_fraction: None,
    }
}

/// The source the V-bit is tagged with.
const VBIT_SOURCE: &str = "fusion:0000-guid:preset-1";

/// A DreamCarve library file holding `tools`, each with its source,
/// written as `name` in `dir`.
fn write_library_file(
    dir: &Path,
    name: &str,
    tools: &[(dc_project::Tool, Option<&str>)],
) -> PathBuf {
    let mut lib = dc_library::Library::default();
    for (tool, source) in tools {
        dc_library::add(&mut lib, tool.clone(), source.map(str::to_owned)).expect("valid tool");
    }
    let path = dir.join(name);
    std::fs::write(&path, dc_library::save(&lib).expect("library saves")).expect("file written");
    path
}

/// Imports the two sample tools into the test's library and returns the
/// import output.
fn import_sample_library(dir: &Path) -> Output {
    write_library_file(
        dir,
        "sample.json",
        &[
            (quarter_inch_end_mill(), None),
            (vbit_60(), Some(VBIT_SOURCE)),
        ],
    );
    let out = run(dir, &["tools", "import", "sample.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    out
}

/// The library as `dreamcarve` left it on disk.
fn read_library(dir: &Path) -> dc_library::Library {
    dc_library::load(&std::fs::read(library_file(dir)).expect("library readable"))
        .expect("library loads")
}

/// `tools list` says when there is no library yet, lists every tool once
/// there is one, and `--json` prints the file.
#[test]
fn tools_list_shows_every_tool() {
    let dir = work_dir("tools_list_shows_every_tool");
    let out = run(&dir, &["tools", "list"]);
    assert_eq!(out.status.code(), Some(0), "list: {}", stderr(&out));
    assert!(
        stdout(&out).contains(&library_file(&dir).display().to_string()),
        "names the missing file: {}",
        stdout(&out)
    );
    assert!(!library_file(&dir).exists(), "list creates nothing");

    let out = import_sample_library(&dir);
    let text = stdout(&out);
    assert!(
        text.contains("1/4 end mill") && text.contains("imported"),
        "{text}"
    );
    assert!(text.contains("60° V-bit"), "{text}");
    assert!(text.contains("2 imported"), "{text}");
    assert!(library_file(&dir).exists(), "the library was created");

    let out = run(&dir, &["tools", "list"]);
    assert_eq!(out.status.code(), Some(0), "list: {}", stderr(&out));
    let text = stdout(&out);
    for needle in [
        "lt1",
        "1/4 end mill",
        "lt2",
        "60° V-bit",
        "6.35",
        "1500",
        VBIT_SOURCE,
    ] {
        assert!(text.contains(needle), "list lacks {needle}:\n{text}");
    }
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "a header and one line per tool:\n{text}");
    assert!(
        lines[1].starts_with("lt1") && lines[2].starts_with("lt2"),
        "{text}"
    );

    let out = run(&dir, &["tools", "list", "--json"]);
    assert_eq!(out.status.code(), Some(0), "list --json: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        std::fs::read_to_string(library_file(&dir)).expect("library readable")
    );
}

/// Acceptance 35: `tools add` copies the tool into the project under a
/// fresh project id, keeping its number and every other value; the
/// library id never appears in the project. By id works too; an unknown
/// name fails naming it and writes nothing.
#[test]
fn tools_add_copies_into_project_with_new_id() {
    let dir = work_dir("tools_add_copies_into_project_with_new_id");
    let dcv = init_coaster(&dir);
    let before = std::fs::read(&dcv).expect("dcv readable");

    // No library yet: an error naming the file, and the project untouched.
    let out = run(&dir, &["tools", "add", "coaster.dcv", "1/4 end mill"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stderr(&out).contains(&library_file(&dir).display().to_string()),
        "{}",
        stderr(&out)
    );
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), before);

    import_sample_library(&dir);
    let out = run(&dir, &["tools", "add", "coaster.dcv", "1/4 end mill"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    assert!(stdout(&out).contains("t2"), "{}", stdout(&out));
    let project = load_file(&dcv).project;
    assert_eq!(project.tools.len(), 2, "{:?}", project.tools);
    let added = &project.tools[1];
    assert_eq!(added.id, ToolId::new("t2"), "next free project id");
    let mut expected = quarter_inch_end_mill();
    expected.id = ToolId::new("t2");
    assert_eq!(*added, expected, "every field copied, the number kept");
    assert!(
        !std::fs::read_to_string(&dcv)
            .expect("dcv readable")
            .contains("lt1"),
        "the library id is not in the project"
    );
    // The project's own tool and operations are as they were.
    assert_eq!(project.tools[0].id, ToolId::new("t1"));
    assert_eq!(project.operations.len(), 1);

    // By id, and the next free id is t3.
    let out = run(&dir, &["tools", "add", "coaster.dcv", "lt2"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    let project = load_file(&dcv).project;
    assert_eq!(project.tools[2].id, ToolId::new("t3"));
    assert_eq!(project.tools[2].name, "60° V-bit");

    let written = std::fs::read(&dcv).expect("dcv readable");
    let out = run(&dir, &["tools", "add", "coaster.dcv", "no such tool"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stderr(&out).contains("no such tool"), "{}", stderr(&out));
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), written);
}

/// Acceptance 35: projects embed copies (ADR-0002). Removing the tool
/// from the library, and re-importing it with other feeds, leave the
/// project file byte-identical.
#[test]
fn library_edit_does_not_change_project() {
    let dir = work_dir("library_edit_does_not_change_project");
    let dcv = init_coaster(&dir);
    import_sample_library(&dir);
    let out = run(&dir, &["tools", "add", "coaster.dcv", "1/4 end mill"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    let with_tool = std::fs::read(&dcv).expect("dcv readable");

    let out = run(&dir, &["tools", "remove", "lt1"]);
    assert_eq!(out.status.code(), Some(0), "remove: {}", stderr(&out));
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), with_tool);

    let mut faster = quarter_inch_end_mill();
    faster.feed_mm_min = Some(2500.0);
    write_library_file(&dir, "faster.json", &[(faster, None)]);
    let out = run(&dir, &["tools", "import", "faster.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), with_tool);
    let project = load_file(&dcv).project;
    assert_eq!(
        project.tools[1].feed_mm_min,
        Some(1500.0),
        "the copy kept its feed"
    );
}

/// Acceptance 35: `tools export` then `tools import` of the result
/// reports every tool as already present and leaves the library
/// byte-identical. A changed tool with the same source is a new tool
/// unless `--replace`, which overwrites it in place.
#[test]
fn tools_export_import_round_trip() {
    let dir = work_dir("tools_export_import_round_trip");
    import_sample_library(&dir);
    let out = run(&dir, &["tools", "export", "out.json"]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let exported = std::fs::read(dir.join("out.json")).expect("export readable");
    assert_eq!(
        exported,
        std::fs::read(library_file(&dir)).expect("library readable"),
        "the export is the library file"
    );

    let out = run(&dir, &["tools", "import", "out.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(text.matches("already present").count(), 3, "{text}");
    assert!(!text.contains("imported as"), "{text}");
    assert_eq!(
        std::fs::read(library_file(&dir)).expect("library readable"),
        exported,
        "re-import changed the library"
    );

    // The same source with other feeds: without --replace a second tool
    // under a suffixed name; with it, the existing tool is overwritten.
    let mut changed = vbit_60();
    changed.feed_mm_min = Some(900.0);
    write_library_file(&dir, "changed.json", &[(changed, Some(VBIT_SOURCE))]);
    let out = run(&dir, &["tools", "import", "changed.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    assert!(stdout(&out).contains("60° V-bit (2)"), "{}", stdout(&out));
    let lib = read_library(&dir);
    assert_eq!(lib.tools.len(), 3);
    assert_eq!(lib.tools[2].id, dc_library::LibraryToolId::new("lt3"));
    assert_eq!(lib.tools[2].tool.name, "60° V-bit (2)");

    let out = run(&dir, &["tools", "remove", "lt3"]);
    assert_eq!(out.status.code(), Some(0), "remove: {}", stderr(&out));
    let out = run(&dir, &["tools", "import", "changed.json", "--replace"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    assert!(stdout(&out).contains("replaced"), "{}", stdout(&out));
    let lib = read_library(&dir);
    assert_eq!(lib.tools.len(), 2, "replaced in place");
    assert_eq!(lib.tools[1].id, dc_library::LibraryToolId::new("lt2"));
    assert_eq!(lib.tools[1].tool.feed_mm_min, Some(900.0));
    assert_eq!(lib.tools[1].source.as_deref(), Some(VBIT_SOURCE));
    assert_eq!(
        lib.tools[1].tool.id,
        ToolId::new("lt2"),
        "the inner id follows"
    );
}

/// `tools remove` takes a tool out and leaves the rest; removing it again
/// is an error naming it.
#[test]
fn tools_remove() {
    let dir = work_dir("tools_remove");
    import_sample_library(&dir);
    let out = run(&dir, &["tools", "remove", "lt1"]);
    assert_eq!(out.status.code(), Some(0), "remove: {}", stderr(&out));
    assert!(stdout(&out).contains("1/4 end mill"), "{}", stdout(&out));
    let lib = read_library(&dir);
    assert_eq!(lib.tools.len(), 1);
    assert_eq!(lib.tools[0].id, dc_library::LibraryToolId::new("lt2"));
    let out = run(&dir, &["tools", "list"]);
    assert!(!stdout(&out).contains("lt1"), "{}", stdout(&out));

    let out = run(&dir, &["tools", "remove", "lt1"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stderr(&out).contains("lt1"), "{}", stderr(&out));
    assert_eq!(read_library(&dir).tools.len(), 1, "nothing else removed");
}

// ── Fusion import (phase 2, slice 21; functional spec capability 22, ─────
// ── acceptance test 35) ──────────────────────────────────────────────────

/// The tools `tests/fixtures/fusion/sample.json` imports as, in order.
const FUSION_IMPORTED: [&str; 13] = [
    "6 mm Flat End Mill",
    "3 mm Ball End Mill",
    "8 mm Bull Nose End Mill (bull nose r1)",
    "20 mm Slot Mill",
    "12 mm 90-Degree Chamfer Mill",
    "5 mm Drill",
    "6 mm 90-Degree Spot Drill",
    "10 mm 90-Degree Countersink",
    "8 mm Counter Bore",
    "6 mm Reamer",
    "1/4 in Flat End Mill",
    "3.175 mm Flat End Mill (Plywood)",
    "3.175 mm Flat End Mill (Acrylic)",
];

/// The items of the sample Fusion library that are skipped, with their
/// types.
const FUSION_SKIPPED: [(&str, &str); 10] = [
    ("6 mm Thread Mill, 1 mm Pitch", "thread mill"),
    ("M6 Right-Hand Tap", "tap right hand"),
    ("50 mm Face Mill", "face mill"),
    ("Ogee Form Router Bit", "form mill"),
    ("6 mm Lollipop Mill", "lollipop mill"),
    ("12 mm 14-Degree Dovetail Mill", "dovetail mill"),
    ("3 mm Tapered Ball Mill, 2-Degree Taper", "tapered mill"),
    ("General Turning Tool", "turning general"),
    ("Probe, 2 mm Stylus", "probe"),
    ("ER20 Collet Holder", "holder"),
];

/// The library tool named `name`.
fn library_tool<'a>(lib: &'a dc_library::Library, name: &str) -> &'a dc_library::LibraryTool {
    lib.tools
        .iter()
        .find(|t| t.tool.name == name)
        .unwrap_or_else(|| panic!("no library tool `{name}`"))
}

/// Acceptance 35: `tools import sample.tools` reports each tool as
/// imported or skipped with its reason, notes unset feeds, then counts.
/// The flat end mill keeps its diameter, flutes, number and preset; the
/// chamfer mill with `TA` 45 is a 90° V-bit; the two-preset tool is two
/// tools. A damaged archive fails naming the file and writes nothing.
#[test]
fn tools_import_fusion_reports_each_tool() {
    let dir = work_dir("tools_import_fusion_reports_each_tool");
    std::fs::write(dir.join("broken.tools"), b"PK\x03\x04not really a zip").expect("written");
    let out = run(&dir, &["tools", "import", "broken.tools"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("broken.tools") && stderr(&out).contains("archive"),
        "{}",
        stderr(&out)
    );
    assert!(!library_file(&dir).exists(), "nothing written");

    copy_fixture(&dir, "fusion/sample.tools", "sample.tools");
    let out = run(&dir, &["tools", "import", "sample.tools"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        FUSION_IMPORTED.len() + FUSION_SKIPPED.len() + 1,
        "one line per tool, then the counts:\n{text}"
    );
    for (k, name) in FUSION_IMPORTED.iter().enumerate() {
        let expected = format!("`{name}`: imported (lt{})", k + 1);
        assert!(
            lines.iter().any(|line| line.starts_with(&expected)),
            "no line `{expected}`:\n{text}"
        );
    }
    for (description, kind) in FUSION_SKIPPED {
        let expected = format!("`{description}`: skipped: unsupported tool type `{kind}`");
        assert!(
            lines.contains(&expected.as_str()),
            "no line {expected}:\n{text}"
        );
    }
    assert!(
        lines.contains(&"`5 mm Drill`: imported (lt6); feed unset"),
        "the drill's unset feed is noted:\n{text}"
    );
    assert!(
        lines.contains(&"`6 mm Flat End Mill`: imported (lt1)"),
        "a complete preset has no note:\n{text}"
    );
    assert!(
        lines.last().is_some_and(
            |l| l.starts_with("13 imported, 0 already present, 0 replaced, 10 skipped")
        ),
        "{text}"
    );

    let lib = read_library(&dir);
    assert_eq!(lib.tools.len(), 13);
    let flat = library_tool(&lib, "6 mm Flat End Mill");
    assert_eq!(
        flat.source.as_deref(),
        Some("fusion:dc000001-0000-4000-8000-000000000001:dc0000p1-0000-4000-8000-000000000001")
    );
    let tool = &flat.tool;
    assert_eq!(tool.cutter, ToolKind::EndMill);
    assert_approx_eq!(tool.diameter_mm, 6.0, 1e-9);
    assert_eq!(tool.flutes, 2);
    assert_eq!(tool.number, Some(5));
    assert_eq!(tool.rpm, Some(18000));
    assert_eq!(tool.feed_mm_min, Some(1200.0));
    assert_eq!(tool.plunge_mm_min, Some(400.0));
    assert_eq!(tool.step_down_mm, Some(2.0));
    assert_approx_eq!(tool.step_over_fraction.expect("stepover set"), 0.4, 1e-9);

    let chamfer = &library_tool(&lib, "12 mm 90-Degree Chamfer Mill").tool;
    assert_eq!(
        chamfer.cutter,
        ToolKind::VBit {
            included_angle_deg: 90.0
        },
        "TA 45 is the half angle"
    );
    let inch = &library_tool(&lib, "1/4 in Flat End Mill").tool;
    assert_approx_eq!(inch.diameter_mm, 6.35, 1e-9);
    assert_approx_eq!(inch.feed_mm_min.expect("feed set"), 1524.0, 1e-9);
    let plywood = &library_tool(&lib, "3.175 mm Flat End Mill (Plywood)").tool;
    let acrylic = &library_tool(&lib, "3.175 mm Flat End Mill (Acrylic)").tool;
    assert_eq!((plywood.number, acrylic.number), (Some(7), Some(7)));
    assert_eq!(acrylic.feed_mm_min, Some(800.0));
    let drill = &library_tool(&lib, "5 mm Drill").tool;
    assert_eq!(drill.cutter, ToolKind::Drill);
    assert_eq!(drill.feed_mm_min, None, "never defaulted");
    assert_eq!(drill.plunge_mm_min, Some(300.0));
}

/// Acceptance 35: importing the same library again, zipped or not,
/// reports every tool as already present (and the skipped ones skipped
/// again) and leaves the library byte-identical, with or without
/// `--replace`.
#[test]
fn tools_import_twice_is_already_present() {
    let dir = work_dir("tools_import_twice_is_already_present");
    copy_fixture(&dir, "fusion/sample.tools", "sample.tools");
    copy_fixture(&dir, "fusion/sample.json", "sample.json");
    let out = run(&dir, &["tools", "import", "sample.tools"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    let first = std::fs::read(library_file(&dir)).expect("library readable");

    for args in [
        &["tools", "import", "sample.json"][..],
        &["tools", "import", "sample.tools", "--replace"][..],
    ] {
        let out = run(&dir, args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", stderr(&out));
        let text = stdout(&out);
        assert_eq!(text.matches(": already present (lt").count(), 13, "{text}");
        assert_eq!(text.matches(": skipped: ").count(), 10, "{text}");
        assert!(!text.contains(": imported"), "{text}");
        assert!(
            text.contains("0 imported, 13 already present, 0 replaced, 10 skipped"),
            "{text}"
        );
        assert_eq!(
            std::fs::read(library_file(&dir)).expect("library readable"),
            first,
            "{args:?} changed the library"
        );
    }

    // A malformed DreamCarve file is still an error naming it.
    std::fs::write(dir.join("bad.json"), b"{\"version\": 1, \"tools\": [{}]}").expect("written");
    let out = run(&dir, &["tools", "import", "bad.json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stderr(&out).contains("bad.json"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read(library_file(&dir)).expect("library readable"),
        first
    );
}

// ── #105: placed tabs ──

/// The repository's corpus of version 9 projects with tab settings, and
/// the programs they exported before tabs were placed objects (#105).
fn tabs_migration_corpus() -> Vec<(String, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tabs-migration");
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .expect("corpus readable")
        .map(|e| e.expect("corpus entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "dcv"))
        .map(|p| {
            let stem = p.file_stem().expect("stem").to_string_lossy().into_owned();
            (stem, p)
        })
        .collect();
    names.sort();
    names
}

/// Every program `export` writes for the project `name.dcv` in `dir`,
/// with the expected file each is compared with.
fn exported_programs(dir: &Path, name: &str) -> Vec<(String, String)> {
    let mut programs: Vec<(String, String)> = std::fs::read_dir(dir)
        .expect("dir readable")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "nc"))
        .filter_map(|p| {
            let file = p.file_name()?.to_string_lossy().into_owned();
            (file == format!("{name}.nc") || file.starts_with(&format!("{name}-")))
                .then(|| (file, std::fs::read_to_string(&p).expect("program readable")))
        })
        .collect();
    programs.sort();
    programs
}

/// Asserts the programs `name` exported in `dir` are byte for byte the
/// committed `tests/expected/tabs-migration/` ones.
fn assert_corpus_programs(dir: &Path, name: &str, case: &str) {
    let expected_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected/tabs-migration");
    let programs = exported_programs(dir, name);
    assert!(!programs.is_empty(), "{case}: {name} exported nothing");
    for (file, text) in programs {
        let expected = std::fs::read_to_string(expected_dir.join(&file))
            .unwrap_or_else(|e| panic!("{case}: expected {file}: {e}"));
        assert!(
            text == expected,
            "{case}: {file} differs from the program before #105:\n{text}"
        );
    }
}

/// #105, the issue's first test, on every project of the corpus (13 cut
/// for this, the plaque fixtures and a phase 1 grblHAL file): a version 9
/// file with tab settings exports byte-identical G-code, both as it is
/// (its settings are tab requests, placed when it is cut) and after
/// `tabs place` has written them as placed tabs and saved the file. The
/// expected programs were exported by the build before #105.
#[test]
fn version_9_tab_settings_export_byte_identical_programs() {
    let dir = work_dir("version_9_tab_settings_export_byte_identical_programs");
    for (name, path) in tabs_migration_corpus() {
        let file = format!("{name}.dcv");
        std::fs::copy(&path, dir.join(&file)).expect("corpus copied");
        let text = std::fs::read_to_string(dir.join(&file)).expect("dcv readable");
        assert!(text.contains("\"tabs\": {"), "{name} has a tab setting");

        let out = run(&dir, &["export", &file, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        assert_corpus_programs(&dir, &name, "as loaded");

        let project = load_file(&dir.join(&file)).project;
        let op = project
            .operations
            .iter()
            .find_map(|op| match op {
                Operation::Profile(p) if p.enabled && p.tabs.is_some() => Some(p.id.clone()),
                _ => None,
            })
            .expect("a Profile with tabs");
        let out = run(&dir, &["tabs", "place", &file, op.as_str()]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        let placed = load_file(&dir.join(&file)).project;
        assert!(!placed.tabs.is_empty(), "{name}: tabs placed");
        assert!(
            placed.operations.iter().all(|op| match op {
                Operation::Profile(p) => !p.enabled || p.tabs.is_none(),
                _ => true,
            }),
            "{name}: every enabled request is placed"
        );
        for entry in std::fs::read_dir(&dir).expect("dir readable") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|x| x == "nc") {
                std::fs::remove_file(path).expect("program removed");
            }
        }
        let out = run(&dir, &["export", &file, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        assert_corpus_programs(&dir, &name, "placed");
    }
}

// ── #284: the last pass allowance ──

/// #284 (`migrate::v12_to_v13_adds_null_last_pass`): every project of
/// the corpus saved at format 12 (the #105 corpus without its version 6
/// file, six golden Profile and Pocket projects, and the `dc-project`
/// sample fixture) migrates to format 13 with `last_pass: null` on every
/// Profile and exports byte for byte the programs the build before #284
/// wrote, committed under `tests/expected/last-pass-migration/`.
#[test]
fn version_12_projects_export_byte_identical_programs() {
    let dir = work_dir("version_12_projects_export_byte_identical_programs");
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/last-pass-migration");
    let expected_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected/last-pass-migration");
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&corpus)
        .expect("corpus readable")
        .map(|e| e.expect("corpus entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "dcv"))
        .map(|p| {
            let stem = p.file_stem().expect("stem").to_string_lossy().into_owned();
            (stem, p)
        })
        .collect();
    names.sort();
    assert_eq!(names.len(), 22, "{names:?}");
    for (name, path) in names {
        let file = format!("{name}.dcv");
        std::fs::copy(&path, dir.join(&file)).expect("corpus copied");
        let text = std::fs::read_to_string(dir.join(&file)).expect("dcv readable");
        assert!(
            text.contains("\"version\": 12"),
            "{name} is a version 12 file"
        );
        assert!(!text.contains("last_pass"), "{name} has no last pass");

        let project = load_file(&dir.join(&file)).project;
        assert_eq!(project.version, dc_project::CURRENT_VERSION);
        for op in &project.operations {
            if let Operation::Profile(p) = op {
                assert_eq!(p.last_pass, None, "{name}: {}", p.id);
            }
        }
        let out = run(&dir, &["export", &file, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        let programs = exported_programs(&dir, &name);
        assert!(!programs.is_empty(), "{name} exported nothing");
        for (program, text) in programs {
            let expected = std::fs::read_to_string(expected_dir.join(&program))
                .unwrap_or_else(|e| panic!("{name}: expected {program}: {e}"));
            assert!(
                text == expected,
                "{name}: {program} differs from the program before #284:\n{text}"
            );
        }
    }
}

/// #284 (acceptance 46) from the CLI: the coaster with a 0.5 mm last pass
/// exports; every pass but the last runs 0.5 mm farther out, and the last
/// on the phase 1 path, which the program without a last pass cuts on
/// every pass. A 2.2 mm allowance with the 6.35 mm bit, or a last pass on
/// an On line Profile, fails `check` and `export` naming the setting.
#[test]
fn last_pass_exports_and_refuses_what_cannot_work() {
    let dir = work_dir("last_pass_exports_and_refuses_what_cannot_work");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let plain = std::fs::read_to_string(dir.join("coaster.nc")).expect("program");

    let set_last_pass = |side: Side, allowance_mm: f64| {
        let mut project = load_file(&dcv).project;
        for op in &mut project.operations {
            if let Operation::Profile(p) = op {
                p.side = side;
                p.last_pass = Some(dc_project::LastPass {
                    allowance_mm,
                    thickness_mm: None,
                    reverse: false,
                });
            }
        }
        std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    };
    set_last_pass(Side::Outside, 0.5);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let with = std::fs::read_to_string(dir.join("coaster.nc")).expect("program");
    assert_ne!(with, plain);
    assert!(
        load_file(&dcv).project.version == dc_project::CURRENT_VERSION,
        "saved at the current version"
    );

    for (side, allowance_mm, needle) in [
        (Side::Outside, 2.2, "a third of the bit"),
        (Side::OnLine, 0.5, "Inside or Outside"),
    ] {
        set_last_pass(side, allowance_mm);
        if dir.join("coaster.nc").exists() {
            std::fs::remove_file(dir.join("coaster.nc")).expect("program removed");
        }
        for args in [
            &["check", "coaster.dcv"][..],
            &["export", "coaster.dcv", "--date", FIXED_DATE],
        ] {
            let out = run(&dir, args);
            let report = format!("{}{}", stdout(&out), stderr(&out));
            assert_eq!(out.status.code(), Some(1), "{args:?}: {report}");
            assert!(report.contains(needle), "{args:?}: {report}");
            assert!(report.contains("last_pass"), "{args:?}: {report}");
        }
        assert!(!dir.join("coaster.nc").exists(), "export wrote nothing");
    }
}

/// #105: `init` asks for four tabs on each part with a tab request and
/// places none: its one Profile cuts every closed shape, which the user
/// then cuts with operations of their own. `tabs place` with no count
/// writes the request as four placed tabs on the coaster, each 8 × 3 mm
/// rect, and sets the request to `null`.
#[test]
fn init_asks_for_four_tabs_that_tabs_place_writes() {
    let dir = work_dir("init_asks_for_four_tabs_that_tabs_place_writes");
    let dcv = init_coaster(&dir);
    let project = load_file(&dcv).project;
    assert!(project.tabs.is_empty(), "{:?}", project.tabs);
    for op in project.operations.iter().map(as_profile) {
        assert_eq!(op.tabs, Some(init_tab_request()));
    }
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let project = load_file(&dcv).project;
    assert_eq!(project.tabs.len(), 4, "{:?}", project.tabs);
    for tab in &project.tabs {
        assert_eq!(tab.instance.as_str(), "i1");
        assert_approx_eq!(tab.width_mm, 8.0, 1e-12);
        assert_approx_eq!(tab.height_mm, 3.0, 1e-12);
        assert_eq!(tab.shape, TabShape::Rect);
    }
    for op in project.operations.iter().map(as_profile) {
        assert_eq!(op.tabs, None);
    }
}

/// #105: `tabs place --count` places tabs with the phase 2 rules and
/// saves them; `tabs list` shows each with the operation that cuts round
/// it; `tabs clear` removes them all.
#[test]
fn tabs_place_list_and_clear() {
    let dir = work_dir("tabs_place_list_and_clear");
    let dcv = init_coaster(&dir);
    let out = run(&dir, &["tabs", "clear", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Removed 4 tab(s)"),
        "{}",
        stdout(&out)
    );
    assert!(load_file(&dcv).project.tabs.is_empty());

    let out = run(
        &dir,
        &[
            "tabs",
            "place",
            "coaster.dcv",
            "op1",
            "--count",
            "3",
            "--width",
            "6",
            "--height",
            "2.5",
            "--shape",
            "smooth",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = stdout(&out);
    assert_eq!(printed.matches("Placed `tab").count(), 3, "{printed}");
    assert!(
        printed.contains("6 mm wide, top 2.5 mm above the stock bottom, smooth"),
        "{printed}"
    );
    let project = load_file(&dcv).project;
    assert_eq!(project.tabs.len(), 3);
    assert!(project.tabs.iter().all(|t| t.shape == TabShape::Smooth));

    let out = run(&dir, &["tabs", "list", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let listed = stdout(&out);
    let lines: Vec<&str> = listed.lines().collect();
    assert!(lines[0].starts_with("ID"), "{listed}");
    assert!(lines[0].contains("CUT_BY"), "{listed}");
    assert_eq!(lines.len(), 4, "{listed}");
    for line in &lines[1..] {
        assert!(line.ends_with("op1"), "cut round by op1: {line}");
        assert!(line.contains("smooth"), "{line}");
    }

    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "--tab", "tab2"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let ids: Vec<String> = load_file(&dcv)
        .project
        .tabs
        .iter()
        .map(|t| t.id.to_string())
        .collect();
    assert_eq!(ids, ["tab1", "tab3"]);
    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(load_file(&dcv).project.tabs.is_empty());
    let out = run(&dir, &["tabs", "list", "coaster.dcv"]);
    assert!(stdout(&out).contains("has no tabs"), "{}", stdout(&out));
}

/// #105: `tabs place --at` puts a tab at each distance along the
/// outline from its first vertex. The coaster's circle, centred at
/// (55, 55), starts at its rightmost point, (100, 55), and runs clockwise
/// as the SVG draws it, 282.74 mm round, so a quarter of the way is its
/// bottom, (55, 10): a chord of 45√2 mm from the start.
#[test]
fn tabs_place_at_puts_tabs_along_the_outline() {
    let dir = work_dir("tabs_place_at_puts_tabs_along_the_outline");
    let dcv = init_coaster(&dir);
    run(&dir, &["tabs", "clear", "coaster.dcv"]);
    let quarter = format!("{}", std::f64::consts::PI * 90.0 / 4.0);
    let at = format!("0,{quarter}");
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1", "--at", &at]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let project = load_file(&dcv).project;
    assert_eq!(project.tabs.len(), 2, "{:?}", project.tabs);
    let instance = &project.instances[0];
    let points: Vec<Point> = project
        .tabs
        .iter()
        .map(|t| instance.transform.apply(Point::new(t.x_mm, t.y_mm)))
        .collect();
    let first = project.definitions[&instance.definition].profiles[0].vertices()[0];
    let start = instance.transform.apply(Point::new(first.x, first.y));
    assert!(points[0].distance_to(start) < 1e-9, "{points:?}");
    assert!(
        (points[1].distance_to(start) - 45.0 * std::f64::consts::SQRT_2).abs() < 1e-6,
        "a quarter turn on: {points:?}"
    );
}

/// #105: `tabs place` names the operation it needs: one the project does
/// not have, one that is not a Profile, or a Profile with no request when
/// neither --count nor --at is given, all fail and change nothing.
#[test]
fn tabs_place_refuses_what_it_cannot_place() {
    let dir = work_dir("tabs_place_refuses_what_it_cannot_place");
    let dcv = init_coaster(&dir);
    // Placed and cleared: the Profile has no tab request left.
    run(&dir, &["tabs", "clear", "coaster.dcv"]);
    let before = std::fs::read_to_string(&dcv).expect("dcv readable");
    let out = run(
        &dir,
        &["tabs", "place", "coaster.dcv", "op9", "--count", "2"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("operation `op9` is not in"),
        "{}",
        stderr(&out)
    );
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("has no tab request"),
        "{}",
        stderr(&out)
    );
    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "--tab", "tab9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("tab `tab9` is not in"),
        "{}",
        stderr(&out)
    );
    // No count of 0, and no size or shape with nothing to place.
    for args in [
        &["--count", "0"][..],
        &["--width", "5"][..],
        &["--height", "2"][..],
        &["--shape", "smooth"][..],
        &["--on-holes"][..],
    ] {
        let mut all = vec!["tabs", "place", "coaster.dcv", "op1"];
        all.extend_from_slice(args);
        let out = run(&dir, &all);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
    }
    assert_eq!(std::fs::read_to_string(&dcv).expect("dcv readable"), before);
}

/// #105: `check` warns `tab_not_crossed` for a tab no enabled Profile
/// cuts round, here once the coaster's Profile is disabled.
#[test]
fn check_warns_tab_not_crossed() {
    let dir = work_dir("check_warns_tab_not_crossed");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!report.contains("tab_not_crossed"), "{report}");
    let mut project = load_file(&dcv).project;
    for op in &mut project.operations {
        as_profile_mut(op).enabled = false;
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        report.matches("warning: tab_not_crossed:").count(),
        4,
        "{report}"
    );
    assert!(
        report.contains(
            "tab `tab1` on `coaster` (instance `i1`, profile 0) is crossed by no enabled Profile operation"
        ),
        "{report}"
    );
}

/// #300, the issue's second test: `tabs list` shows how far each tab's
/// point is from its outline, as cut, in `OFF_MM`. The coaster's request
/// places its four tabs on the tool path, a bit radius (3.175 mm) off the
/// circle. A tab put by hand 20 mm out from the circle's first vertex, its
/// rightmost point, lists 20, and `check` warns `tab_off_outline` for it
/// alone.
#[test]
fn tabs_list_shows_how_far_each_tab_is_off_its_outline() {
    let dir = work_dir("tabs_list_shows_how_far_each_tab_is_off_its_outline");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let mut project = load_file(&dcv).project;
    let instance = &project.instances[0];
    let first = project.definitions[&instance.definition].profiles[0].vertices()[0];
    let mut far = project.tabs[0].clone();
    far.id = dc_project::TabId::new("far");
    far.x_mm = first.x + 20.0;
    far.y_mm = first.y;
    project.tabs.push(far);
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["tabs", "list", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let listed = stdout(&out);
    let rows: Vec<Vec<&str>> = listed
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    let column = rows[0]
        .iter()
        .position(|h| *h == "OFF_MM")
        .unwrap_or_else(|| panic!("an OFF_MM column: {listed}"));
    assert_eq!(rows[0][column - 1], "AT_MM", "{listed}");
    let off: Vec<(&str, &str)> = rows[1..].iter().map(|r| (r[0], r[column])).collect();
    assert_eq!(
        off,
        [
            ("tab1", "3.175"),
            ("tab2", "3.175"),
            ("tab3", "3.175"),
            ("tab4", "3.175"),
            ("far", "20"),
        ],
        "{listed}"
    );

    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        report.matches("warning: tab_off_outline:").count(),
        1,
        "{report}"
    );
    assert!(
        report.contains(
            "tab `far` on `coaster` (instance `i1`, profile 0) is 20 mm from that outline"
        ),
        "{report}"
    );
}

/// #300, the issue's third test: a version 8 file whose part has a hole
/// migrates with `"on_holes": true` (#109), so `tabs place` puts tabs on
/// the hole as the file asked. It warns `tab_on_hole` once for each, as it
/// places them; `export` then warns none, so a user who keeps the tabs is
/// not told on every export. A tab put on the hole's outline by hand, which
/// no placing asked for, still warns. The fixture is the tabs-migration
/// corpus's square with a hole, written back as version 8.
#[test]
fn placed_v8_hole_tabs_export_without_tab_on_hole() {
    let dir = work_dir("placed_v8_hole_tabs_export_without_tab_on_hole");
    let v9 = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tabs-migration/square-with-hole-on-holes.dcv"),
    )
    .expect("fixture readable");
    let v8 = v9
        .replacen("\"version\": 9,", "\"version\": 8,", 1)
        .replacen(",\n        \"on_holes\": true", "", 1);
    assert!(
        !v8.contains("on_holes") && v8.contains("\"version\": 8,"),
        "{v8}"
    );
    std::fs::write(dir.join("square.dcv"), v8).expect("v8 written");

    let out = run(&dir, &["tabs", "place", "square.dcv", "o1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let project = load_file(&dir.join("square.dcv")).project;
    let hole_tabs = project.tabs.iter().filter(|t| t.profile == 1).count();
    assert!(hole_tabs > 0, "{:?}", project.tabs);
    assert_eq!(
        stderr(&out).matches("warning: tab_on_hole:").count(),
        hole_tabs,
        "{}",
        stderr(&out)
    );

    let out = run(&dir, &["export", "square.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!report.contains("tab_on_hole"), "{report}");

    // By hand, on the middle of the hole's bottom edge.
    let mut project = load_file(&dir.join("square.dcv")).project;
    let mut by_hand = project.tabs[0].clone();
    by_hand.id = dc_project::TabId::new("by-hand");
    by_hand.profile = 1;
    by_hand.x_mm = 0.0;
    by_hand.y_mm = -15.0;
    project.tabs.push(by_hand);
    std::fs::write(
        dir.join("square.dcv"),
        save(&project).expect("project saves"),
    )
    .expect("dcv written");
    let out = run(&dir, &["export", "square.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        report.matches("warning: tab_on_hole:").count(),
        1,
        "{report}"
    );
    assert!(report.contains("tab `by-hand` is on"), "{report}");
}

/// #293 with #300: `tabs place --replace --on-holes` warns `tab_on_hole`
/// once for each new tab it puts on the square's hole, and none for the
/// tabs it removed; `export` then warns none. Both parts keep tabs, so
/// there is no note that a part comes loose. Replacing again without
/// `--on-holes` leaves the hole with no tabs: nothing warns, and the hole's
/// slug coming free is no part coming loose, so there is no note either.
#[test]
fn tabs_place_replace_onto_a_hole_warns_once() {
    let dir = work_dir("tabs_place_replace_onto_a_hole_warns_once");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tabs-migration/square-with-hole-on-holes.dcv"),
        dir.join("square.dcv"),
    )
    .expect("fixture copied");
    let out = run(&dir, &["tabs", "place", "square.dcv", "o1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let first = load_file(&dir.join("square.dcv")).project.tabs;

    let out = run(
        &dir,
        &[
            "tabs",
            "place",
            "square.dcv",
            "o1",
            "--count",
            "2",
            "--width",
            "4",
            "--on-holes",
            "--replace",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = stdout(&out);
    let project = load_file(&dir.join("square.dcv")).project;
    let hole_tabs: Vec<&dc_project::PlacedTab> =
        project.tabs.iter().filter(|t| t.profile == 1).collect();
    assert_eq!(hole_tabs.len(), 2, "{:?}", project.tabs);
    assert!(
        printed.contains(&format!("{} tab(s) removed and 4 placed", first.len())),
        "{printed}"
    );
    assert!(!printed.contains("Note:"), "{printed}");
    let warned = stderr(&out);
    assert_eq!(
        warned.matches("warning: tab_on_hole:").count(),
        2,
        "{warned}"
    );
    for tab in &hole_tabs {
        assert_eq!(
            warned.matches(&format!("tab `{}` is on", tab.id)).count(),
            1,
            "{warned}"
        );
    }
    let out = run(&dir, &["export", "square.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert!(!report.contains("tab_on_hole"), "{report}");

    let out = run(
        &dir,
        &[
            "tabs",
            "place",
            "square.dcv",
            "o1",
            "--count",
            "2",
            "--replace",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!stderr(&out).contains("tab_on_hole"), "{}", stderr(&out));
    assert!(!stdout(&out).contains("Note:"), "{}", stdout(&out));
    let project = load_file(&dir.join("square.dcv")).project;
    assert!(
        project.tabs.iter().all(|t| t.profile == 0),
        "{:?}",
        project.tabs
    );
}

/// #207: an SVG line from (20, 30) to (80, 30), drawn left to right, cut
/// Left: the cut runs on the side the drawing shows as the line's left
/// (up the page), which after the importer's flip to Y-up (ADR-0001) is
/// one bit radius above the placed line. `check` reports nothing about the
/// open path, `export` passes the safety pass (S9 holds the cut to its
/// offset path), and climb starts at the end beside the first vertex,
/// keeping the part (below the line) on the right of travel (#287).
#[test]
fn left_profile_of_an_svg_line_exports_on_its_left() {
    let dir = work_dir("left_profile_of_an_svg_line_exports_on_its_left");
    std::fs::write(
        dir.join("edge.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <path id="edge" d="M 20 30 L 80 30"/>
</svg>
"#,
    )
    .expect("svg written");
    let out = run(&dir, &["init", "edge.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("edge.dcv");
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let targets = project.instances.iter().map(|i| i.id.clone()).collect();
    let op = as_profile_mut(&mut project.operations[0]);
    // `init` targets closed outlines only.
    op.targets = targets;
    op.side = Side::Left;
    op.direction = dc_project::CutDirection::Climb;
    op.tabs = None;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let line = placed_vertices(&project, "edge");
    let [(x0, y0), (x1, y1)] = line[..] else {
        panic!("a two-point line: {line:?}");
    };
    assert!(
        x0 < x1 && (y0 - y1).abs() <= 1e-9,
        "still drawn left to right: {line:?}"
    );

    let check = run(&dir, &["check", "edge.dcv"]);
    let report = format!("{}{}", stdout(&check), stderr(&check));
    assert_eq!(check.status.code(), Some(0), "check: {report}");
    assert!(report.contains("0 error(s)"), "{report}");
    assert!(!report.contains("open_profile"), "{report}");
    let out = run(&dir, &["export", "edge.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("edge.nc")).expect("edge.nc written");
    let moves = dc_gcode::parse_motion(&program).expect("program parses");
    let cuts: Vec<&MachineMove> = moves
        .iter()
        .filter(|m| m.kind.is_cut() && moves_xy(m))
        .collect();
    assert!(!cuts.is_empty(), "the program cuts: {program}");
    for m in &cuts {
        let (x, y) = (m.to.x.expect("X known"), m.to.y.expect("Y known"));
        assert!(
            (y - (y0 + BIT_RADIUS_MM)).abs() <= 1e-3,
            "cut off the line's left: {m:?}"
        );
        assert!(
            x >= x0 - 1e-3 && x <= x1 + 1e-3,
            "cut past the line's ends: {m:?}"
        );
    }
    let first = cuts[0];
    assert!(
        (first.from.x.expect("X known") - x0).abs() <= 1e-3,
        "climb starts beside the first vertex: {first:?}"
    );
}

/// #105 review: `tabs clear <dcv> <op>` removes only the tabs that Profile
/// cuts round, refuses an operation that is not a Profile, and says when
/// the tabs it removed leave a part a through cut frees with no tab. The
/// coaster's four tabs are crossed by its cut-out `op1`; a 1 mm On line
/// score pass `op3` stops above the tab tops, so it crosses none and
/// removes none; clearing `op1` removes all four and names the part.
#[test]
fn tabs_clear_op_removes_only_the_tabs_it_cuts_round() {
    let dir = work_dir("tabs_clear_op_removes_only_the_tabs_it_cuts_round");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let mut project = load_file(&dcv).project;
    let mut score = as_profile(&project.operations[0]).clone();
    score.id = OperationId::new("op3");
    score.side = Side::OnLine;
    score.depth_mm = Some(1.0);
    score.through_cut_allowance_mm = 0.0;
    project.operations.push(Operation::Pocket(sample_pocket()));
    project.operations.push(Operation::Profile(score));
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let before = std::fs::read_to_string(&dcv).expect("dcv readable");

    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "o2"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stderr(&out).contains("only a Profile"), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(&dcv).expect("dcv readable"), before);

    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "op3"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Removed 0 tab(s)"),
        "{}",
        stdout(&out)
    );
    assert_eq!(load_file(&dcv).project.tabs.len(), 4);

    let out = run(&dir, &["tabs", "clear", "coaster.dcv", "op1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = stdout(&out);
    assert!(printed.contains("Removed 4 tab(s)"), "{printed}");
    assert!(
        printed.contains("`coaster` (instance `i1`, profile 0)") && printed.contains("no tab"),
        "names the part op1 now cuts free: {printed}"
    );
}

/// The tab points of the project at `dcv`, by id.
fn tab_points(dcv: &Path) -> Vec<(String, Point)> {
    load_file(dcv)
        .project
        .tabs
        .iter()
        .map(|t| (t.id.to_string(), Point::new(t.x_mm, t.y_mm)))
        .collect()
}

/// #293, the issue's first test: `tabs place --replace` swaps the tabs
/// the operation cuts round (the set `tabs clear <dcv> <op>` removes) for
/// the new ones in one save, and prints both. Four tabs on the coaster,
/// then six with `--replace`: the file holds the six, at none of the first
/// four's points and with fresh ids, so an id read off an earlier `tabs
/// list` names no new tab. The part keeps tabs, so there is no note that
/// it comes loose. `--replace` with nothing to place is a usage error.
#[test]
fn tabs_place_replace_swaps_the_operations_tabs() {
    let dir = work_dir("tabs_place_replace_swaps_the_operations_tabs");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(
        &dir,
        &["tabs", "place", "coaster.dcv", "op1", "--count", "4"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let first = tab_points(&dcv);
    assert_eq!(first.len(), 4, "{first:?}");

    let out = run(
        &dir,
        &[
            "tabs",
            "place",
            "coaster.dcv",
            "op1",
            "--count",
            "6",
            "--replace",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = stdout(&out);
    assert_eq!(printed.matches("Removed `tab").count(), 4, "{printed}");
    assert_eq!(printed.matches("Placed `tab").count(), 6, "{printed}");
    assert!(
        printed.contains("4 tab(s) removed and 6 placed; coaster.dcv saved with 6 tab(s)."),
        "{printed}"
    );
    assert!(!printed.contains("Note:"), "{printed}");
    let second = tab_points(&dcv);
    assert_eq!(second.len(), 6, "{second:?}");
    for (id, point) in &second {
        assert!(
            first.iter().all(|(old, _)| old != id),
            "new tab reuses id {id}: {second:?}"
        );
        assert!(
            first.iter().all(|(_, old)| old.distance_to(*point) > 1e-6),
            "new tab {id} at an old tab's point: {second:?}"
        );
    }

    let before = std::fs::read_to_string(&dcv).expect("dcv readable");
    let out = run(&dir, &["tabs", "place", "coaster.dcv", "op1", "--replace"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("--replace needs --count or --at"),
        "{}",
        stderr(&out)
    );
    assert_eq!(std::fs::read_to_string(&dcv).expect("dcv readable"), before);
}

/// #293, the issue's second test: without `--replace`, `tabs place` keeps
/// adding: four tabs then six leave ten on the coaster (none of the six
/// lands on one of the four), and it notes that the outline already had
/// tabs and how to swap them instead.
#[test]
fn tabs_place_without_replace_notes_existing_tabs() {
    let dir = work_dir("tabs_place_without_replace_notes_existing_tabs");
    init_coaster(&dir);
    let dcv = dir.join("coaster.dcv");
    let out = run(
        &dir,
        &["tabs", "place", "coaster.dcv", "op1", "--count", "4"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = run(
        &dir,
        &["tabs", "place", "coaster.dcv", "op1", "--count", "6"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(load_file(&dcv).project.tabs.len(), 10);
    let printed = stdout(&out);
    assert!(
        printed.contains(
            "Note: `coaster` (instance `i1`, profile 0) already had 4 tab(s); this adds 6 more. To swap them instead, give --replace."
        ),
        "{printed}"
    );
}

/// One open, straight line 98 mm long.
const ESTIMATE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="120mm" height="40mm" viewBox="0 0 120 40">
  <path id="line" d="M 10 20 L 108 20"/>
</svg>
"#;

/// A project whose program is, besides setup lines, acceptance test 42's:
/// one Engrave of a 98 mm line, 1 mm deep in one pass, at safe Z 25 mm,
/// feed and plunge 600 mm/min and the 3 s spin-up dwell. So the program
/// feeds 100 mm (the 2 mm plunge from 1 mm above the stock, then the
/// line), 10 s, and rapids 50 mm: 24 mm down to 1 mm above the stock and
/// 26 mm back up. The first rapid starts where the machine was left, so it
/// has no length.
fn estimate_project(dir: &Path) -> PathBuf {
    std::fs::write(dir.join("line.svg"), ESTIMATE_SVG).expect("svg written");
    let out = run(dir, &["init", "line.svg"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let dcv = dir.join("line.dcv");
    fill_in(&dcv, 600.0, 600.0, 18000);
    let mut project = load_file(&dcv).project;
    project.operations = vec![Operation::Engrave(EngraveOp {
        id: OperationId::new("op1"),
        name: "Engrave".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: project
            .tools
            .first()
            .expect("init writes a tool")
            .id
            .clone(),
        depth_mm: Some(1.0),
        step_down_mm: Some(1.0),
        alternate_direction: false,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 25.0,
    })];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// The line `export` prints for [`estimate_project`] with no `[estimate]`.
const ESTIMATE_LINE: &str = "Estimated time: 13 s (cutting 10 s, dwells 3 s); 50 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.";

/// The line of `report` that starts with `prefix`, trimmed.
fn line_starting<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
    report
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(prefix))
}

/// Capability 30 on the quick start's coaster, whose program is pinned in
/// `tests/expected/coaster-grblhal.nc`: its estimate is pinned with it.
/// The rapids are the 4 mm down to the ramp's start and the 17 mm back up
/// to safe Z. With manual spindle control the program pauses twice, to
/// turn the router on and off, and the line says those are left out.
#[test]
fn coaster_prints_estimate() {
    let dir = work_dir("coaster_prints_estimate");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(
            "Estimated time: 46 s (cutting 43 s, dwells 3 s); 21 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration."
        ),
        "{report}"
    );
    set_spindle_control(&dcv, SpindleControl::Manual);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    let line = line_starting(&report, "Estimated time:")
        .unwrap_or_else(|| panic!("an estimate: {report}"));
    assert!(
        line.ends_with("Excludes acceleration and 2 pauses."),
        "{line}"
    );
}

/// Acceptance 42 (functional capability 30): a program of 100 mm of feed
/// at F600, a 3 s spin-up dwell and 50 mm of rapids is estimated at 10 s
/// cutting and 3 s dwell, and with no rapid rate configured the 50 mm is
/// named and left out, with how to include it. `check` prints the same
/// line once the program builds.
#[test]
fn export_prints_estimate() {
    let dir = work_dir("export_prints_estimate");
    estimate_project(&dir);
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(ESTIMATE_LINE),
        "{report}"
    );
    let check = run(&dir, &["check", "line.dcv"]);
    assert_eq!(check.status.code(), Some(0), "check: {}", stdout(&check));
    let report = stdout(&check);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(ESTIMATE_LINE),
        "{report}"
    );
}

/// Acceptance 42: with `rapid_mm_min = 3000`, the 50 mm of rapids take
/// 1 s, which the estimate adds.
#[test]
fn estimate_with_rapid_rate() {
    let dir = work_dir("estimate_with_rapid_rate");
    estimate_project(&dir);
    write_config(&dir, "[estimate]\nrapid_mm_min = 3000\n");
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(
            "Estimated time: 14 s (cutting 10 s, dwells 3 s, rapids 1 s at 3000 mm/min). Excludes acceleration."
        ),
        "{report}"
    );
}

/// Acceptance 42: `scale_factor = 2` doubles every time, the rapids' too.
#[test]
fn estimate_scale_factor_multiplies() {
    let dir = work_dir("estimate_scale_factor_multiplies");
    estimate_project(&dir);
    write_config(&dir, "[estimate]\nscale_factor = 2\n");
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(
            "Estimated time: 26 s (cutting 20 s, dwells 6 s; scale factor 2); 50 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration."
        ),
        "{report}"
    );
    write_config(&dir, "[estimate]\nscale_factor = 2\nrapid_mm_min = 3000\n");
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        line_starting(&report, "Estimated time:"),
        Some(
            "Estimated time: 28 s (cutting 20 s, dwells 6 s, rapids 2 s at 3000 mm/min; scale factor 2). Excludes acceleration."
        ),
        "{report}"
    );
}

/// Acceptance 42: the estimate never changes a program. The bytes are the
/// same with and without `[estimate]`. A bad `[estimate]` value fails the
/// export at its line and column, and nothing is written.
#[test]
fn estimate_config_does_not_change_program() {
    let dir = work_dir("estimate_config_does_not_change_program");
    estimate_project(&dir);
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let without = std::fs::read(dir.join("line.nc")).expect("line.nc written");
    write_config(&dir, "[estimate]\nrapid_mm_min = 3000\nscale_factor = 2\n");
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let with = std::fs::read(dir.join("line.nc")).expect("line.nc written");
    assert!(with == without, "the program changed with [estimate]");

    std::fs::remove_file(dir.join("line.nc")).expect("line.nc removed");
    let config = write_config(&dir, "[estimate]\nrapid_mm_min = 0\n");
    let out = run(&dir, &["export", "line.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let message = stderr(&out);
    assert!(
        message.contains(&format!("{}:2:16:", config.display())),
        "{message}"
    );
    assert!(!dir.join("line.nc").exists(), "nothing is written");
}

/// Capability 30: one program per tool run gets an estimate each, then
/// the total of them.
#[test]
fn estimate_per_program_and_total() {
    let dir = work_dir("estimate_per_program_and_total");
    two_tool_coaster(&dir);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let report = stdout(&out);
    let lines: Vec<&str> = report
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("Estimated time:"))
        .collect();
    assert_eq!(lines.len(), 2, "one per program: {report}");
    let total = line_starting(&report, "Estimated total time:")
        .unwrap_or_else(|| panic!("a total: {report}"));
    assert!(total.contains("rapids not included"), "{total}");
    // The total is the programs' sum, to the second.
    let seconds = |line: &str| -> u64 {
        let time = line
            .split(':')
            .nth(1)
            .and_then(|rest| rest.split('(').next())
            .unwrap_or_else(|| panic!("a time in {line}"))
            .trim();
        let mut words = time.split_whitespace();
        let mut sum = 0;
        while let (Some(n), Some(unit)) = (words.next(), words.next()) {
            let n: u64 = n.parse().unwrap_or_else(|_| panic!("{line}"));
            sum += n * match unit {
                "h" => 3600,
                "min" => 60,
                "s" => 1,
                other => panic!("unit {other} in {line}"),
            };
        }
        sum
    };
    let parts: u64 = lines.iter().map(|l| seconds(l)).sum();
    assert!(
        seconds(total).abs_diff(parts) <= 1,
        "{total} is not the sum of {lines:?}"
    );
}

/// The text of every element of `svg` whose `class` is `class`, from its
/// name to its closing `>`.
fn svg_elements<'a>(svg: &'a str, class: &str) -> Vec<&'a str> {
    let wanted = format!(" class=\"{class}\"");
    svg.split('<').filter(|e| e.contains(&wanted)).collect()
}

/// The value of attribute `name` in `element`.
fn svg_attr<'a>(element: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let (_, rest) = element.split_once(&key).expect("attribute present");
    rest.split('"').next().expect("attribute closed")
}

/// The end point of each command of SVG path data `d` written as the
/// preview writes it: `M x y`, `L x y`, `A rx ry rot large sweep x y`.
fn svg_path_points(d: &str) -> Vec<(f64, f64)> {
    let mut tokens = d.split_whitespace();
    let mut points = Vec::new();
    let number = |t: Option<&str>| -> f64 {
        t.expect("a number follows")
            .parse()
            .expect("path numbers parse")
    };
    while let Some(command) = tokens.next() {
        match command {
            "M" | "L" => {}
            "A" => {
                for _ in 0..5 {
                    tokens.next();
                }
            }
            other => panic!("unexpected path command {other} in {d}"),
        }
        let x = number(tokens.next());
        let y = number(tokens.next());
        points.push((x, y));
    }
    points
}

/// Acceptance test 43: the coaster's preview is one SVG framed by the
/// stock whose cut paths pass through every point of the toolpath, Y
/// flipped, within 0.001 mm; one tab mark per tab on the one pass below
/// the tab top, one start circle per pass, and the same bytes twice.
#[test]
fn export_preview_coaster() {
    let dir = work_dir("export_preview_coaster");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let args = [
        "export",
        "coaster.dcv",
        "--preview",
        "preview.svg",
        "--date",
        FIXED_DATE,
    ];
    let out = run(&dir, &args);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    assert!(stdout(&out).contains("preview.svg"), "{}", stdout(&out));
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    assert!(program == include_str!("expected/coaster-grblhal.nc"));
    let svg = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");

    let project = load_file(&dcv).project;
    let (width, height) = (project.stock.width_mm, project.stock.height_mm);
    assert!(
        svg.contains(&format!(" viewBox=\"0 0 {width:.3} {height:.3}\"")),
        "{svg}"
    );
    assert!(!svg.contains("transform"), "{svg}");

    let drawn: Vec<(f64, f64)> = svg_elements(&svg, "cut")
        .iter()
        .flat_map(|cut| svg_path_points(svg_attr(cut, "d")))
        .collect();
    let (toolpath, _) =
        dc_cam::generate_profile(&project, as_profile(&project.operations[0])).expect("generates");
    let mut at: Option<(f64, f64)> = None;
    let mut checked = 0;
    for m in &toolpath.moves {
        let to = (m.to.x, m.to.y);
        let moved = at.is_some_and(|(x, y)| (to.0 - x).hypot(to.1 - y) > 1e-9);
        if m.is_cut() && moved {
            let flipped = (to.0, height - to.1);
            assert!(
                drawn
                    .iter()
                    .any(|p| (p.0 - flipped.0).abs() <= 1e-3 && (p.1 - flipped.1).abs() <= 1e-3),
                "toolpath point {to:?} not drawn at {flipped:?}"
            );
            checked += 1;
        }
        at = Some(to);
    }
    assert!(checked > 10, "the coaster cuts: {checked}");
    // The 12 mm cut in 4 mm passes rises over the 4 tabs on its last pass
    // only: the tabs stand 3 mm, above the 8 mm pass's floor.
    assert_eq!(svg_elements(&svg, "tab").len(), 4, "{svg}");
    let starts = svg_elements(&svg, "start");
    assert_eq!(starts.len(), 3, "one per pass: {svg}");
    assert_eq!(svg_attr(starts[0], "r"), "3.175");

    let out = run(&dir, &args);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let again = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");
    assert!(again == svg, "the preview changed between exports");
}

/// Addendum A5: `check --preview` writes the picture and no program.
#[test]
fn check_preview_writes_file() {
    let dir = work_dir("check_preview_writes_file");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let before = std::fs::read(&dcv).expect("dcv readable");
    let out = run(&dir, &["check", "coaster.dcv", "--preview", "preview.svg"]);
    assert_eq!(out.status.code(), Some(0), "check: {}", stdout(&out));
    assert!(stdout(&out).contains("preview.svg"), "{}", stdout(&out));
    let svg = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");
    assert!(svg.starts_with("<?xml"), "{svg}");
    assert_eq!(svg_elements(&svg, "tab").len(), 4, "{svg}");
    assert!(!dir.join("coaster.nc").exists(), "check wrote a program");
    assert_eq!(std::fs::read(&dcv).expect("dcv readable"), before);

    // An operation that cannot generate: no preview.
    let fresh = work_dir("check_preview_writes_file_fresh");
    init_coaster(&fresh);
    let out = run(
        &fresh,
        &["check", "coaster.dcv", "--preview", "preview.svg"],
    );
    assert_eq!(out.status.code(), Some(1), "check: {}", stdout(&out));
    assert!(
        !fresh.join("preview.svg").exists(),
        "preview of a project with errors"
    );
}

/// Addendum A5: `--preview` naming the project or a program is refused,
/// and nothing is written.
#[test]
fn export_preview_refuses_dcv_path() {
    let dir = work_dir("export_preview_refuses_dcv_path");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let before = std::fs::read(&dcv).expect("dcv readable");
    let names_before = {
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir readable")
            .map(|e| e.expect("entry").file_name())
            .collect();
        names.sort();
        names
    };
    for (command, preview) in [
        ("export", "coaster.dcv"),
        ("export", "coaster.nc"),
        ("export", "other.dcv"),
        ("check", "coaster.dcv"),
        ("check", "coaster.nc"),
        // The drawing the project came from: an SVG, but not a preview.
        ("export", "coaster.svg"),
        ("check", "coaster.svg"),
    ] {
        let out = run(&dir, &[command, "coaster.dcv", "--preview", preview]);
        assert_eq!(out.status.code(), Some(1), "{command} --preview {preview}");
        let err = stderr(&out);
        assert!(err.contains("--preview"), "{command} {preview}: {err}");
        assert_eq!(std::fs::read(&dcv).expect("dcv readable"), before);
        assert_eq!(
            std::fs::read(dir.join("coaster.svg")).expect("drawing readable"),
            std::fs::read(repo_fixture("coaster.svg")).expect("fixture readable"),
            "{command} --preview {preview} changed the drawing"
        );
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir readable")
            .map(|e| e.expect("entry").file_name())
            .collect();
        names.sort();
        assert_eq!(names, names_before, "{command} --preview {preview} wrote");
    }
    // The program's own `-o` path is refused too.
    let args = [
        "export",
        "coaster.dcv",
        "-o",
        "part.svg",
        "--preview",
        "part.svg",
    ];
    let out = run(&dir, &args);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!dir.join("part.svg").exists());
}

/// The coaster in an inch project, every value set as the quick start
/// sets it but in inches: `(field, inch string, the same value in mm)`,
/// the millimetres being the product the loader computes (addendum A6).
const COASTER_INCH_VALUES: [(&str, &str, f64); 7] = [
    ("thickness_mm", "1/2 in", 0.5 * 25.4),
    ("depth_mm", "1/2 in", 0.5 * 25.4),
    ("through_cut_allowance_mm", "1/64 in", (1.0 / 64.0) * 25.4),
    ("feed_mm_min", "60 in/min", 60.0 * 25.4),
    ("plunge_mm_min", "20 in/min", 20.0 * 25.4),
    ("step_down_mm", "1/8 in", 0.125 * 25.4),
    ("safe_z_mm", "0.2 in", 0.2 * 25.4),
];

/// The millimetres [`COASTER_INCH_VALUES`] lists for `field`.
fn coaster_inch_mm(field: &str) -> f64 {
    COASTER_INCH_VALUES
        .iter()
        .find(|(f, _, _)| *f == field)
        .map(|(_, _, mm)| *mm)
        .expect("value listed")
}

/// The inch string [`COASTER_INCH_VALUES`] lists for `field`, as JSON.
fn coaster_inch_string(field: &str) -> serde_json::Value {
    COASTER_INCH_VALUES
        .iter()
        .find(|(f, _, _)| *f == field)
        .map(|(_, text, _)| serde_json::Value::String((*text).to_owned()))
        .expect("value listed")
}

/// `init --units inch` on the coaster, then its values filled in twice:
/// `coaster-mm.dcv` with the millimetre numbers and `coaster-in.dcv` with
/// the inch strings of [`COASTER_INCH_VALUES`]. Returns the two paths.
#[allow(
    clippy::indexing_slicing,
    reason = "indexing a JSON object inserts the key; a saved project has these"
)]
fn coaster_inch_twins(dir: &Path) -> (PathBuf, PathBuf) {
    std::fs::copy(repo_fixture("coaster.svg"), dir.join("coaster.svg")).expect("svg copied");
    let out = run(dir, &["init", "coaster.svg", "--units", "inch"]);
    assert_eq!(out.status.code(), Some(0), "init: {}", stderr(&out));
    let mut project = load_file(&dir.join("coaster.dcv")).project;
    project.stock.thickness_mm = Some(coaster_inch_mm("thickness_mm"));
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(coaster_inch_mm("feed_mm_min"));
        tool.plunge_mm_min = Some(coaster_inch_mm("plunge_mm_min"));
        tool.rpm = Some(18000);
        tool.step_down_mm = Some(coaster_inch_mm("step_down_mm"));
    }
    for op in project.operations.iter_mut().map(as_profile_mut) {
        op.depth_mm = Some(coaster_inch_mm("depth_mm"));
        op.through_cut_allowance_mm = coaster_inch_mm("through_cut_allowance_mm");
        op.safe_z_mm = coaster_inch_mm("safe_z_mm");
    }
    let text = save(&project).expect("project saves");
    let mm_path = dir.join("coaster-mm.dcv");
    std::fs::write(&mm_path, &text).expect("dcv written");

    // The same project with the values as strings.
    let mut root: serde_json::Value = serde_json::from_str(&text).expect("json");
    root["stock"]["thickness_mm"] = coaster_inch_string("thickness_mm");
    for tool in root["tools"].as_array_mut().expect("tools") {
        for field in ["feed_mm_min", "plunge_mm_min", "step_down_mm"] {
            tool[field] = coaster_inch_string(field);
        }
    }
    for op in root["operations"].as_array_mut().expect("operations") {
        for field in ["depth_mm", "through_cut_allowance_mm", "safe_z_mm"] {
            op[field] = coaster_inch_string(field);
        }
    }
    let in_path = dir.join("coaster-in.dcv");
    let text = serde_json::to_string_pretty(&root).expect("json written");
    std::fs::write(&in_path, text).expect("dcv written");
    (mm_path, in_path)
}

/// Acceptance test 44: a project whose lengths and feeds are written in
/// inches exports the program its millimetre twin exports, byte for byte.
#[test]
fn inch_input_exports_same_program() {
    let dir = work_dir("inch_input_exports_same_program");
    let (mm_path, in_path) = coaster_inch_twins(&dir);
    let in_text = std::fs::read_to_string(&in_path).expect("dcv readable");
    assert!(in_text.contains("\"1/2 in\""), "{in_text}");
    assert!(in_text.contains("\"60 in/min\""), "{in_text}");
    let export = |dcv: &Path, nc: &str| {
        let dcv = dcv.to_str().expect("utf-8 path");
        let out = run(&dir, &["export", dcv, "-o", nc, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "export {dcv}: {}", stderr(&out));
        std::fs::read_to_string(dir.join(nc)).expect("program written")
    };
    let from_mm = export(&mm_path, "from-mm.nc");
    let from_in = export(&in_path, "from-in.nc");
    assert!(from_mm.contains("G20"), "an inch program: {from_mm}");
    assert!(
        from_in == from_mm,
        "programs differ:\n{from_in}\n---\n{from_mm}"
    );
    // `check` reads the strings too, and finds nothing wrong.
    let out = run(&dir, &["check", in_path.to_str().expect("utf-8 path")]);
    assert_eq!(out.status.code(), Some(0), "check: {}", stdout(&out));
}

/// Addendum A6: export's summary states each operation's depth in the
/// project's display units, with that unit's decimal places.
#[test]
fn inch_project_summary_in_inches() {
    let dir = work_dir("inch_project_summary_in_inches");
    let (_, in_path) = coaster_inch_twins(&dir);
    let dcv = in_path.to_str().expect("utf-8 path");
    let out = run(&dir, &["export", dcv, "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let summary = stdout(&out);
    assert!(summary.contains(", 0.5000 in deep"), "{summary}");
    assert!(!summary.contains("mm deep"), "{summary}");

    // A millimetre project keeps millimetres, at their decimal places.
    let dir = work_dir("inch_project_summary_in_inches_mm");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let summary = stdout(&out);
    assert!(summary.contains(", 12.000 mm deep"), "{summary}");
}

// ── #283: clearance, tool-change and park heights ──

/// The plaque (`tests/fixtures/plaque.dcv`) copied into `dir`, saved with
/// `heights` set, as `plaque.dcv`.
fn plaque_with_heights(dir: &Path, heights: dc_project::Heights) -> PathBuf {
    let dcv = copy_fixture(dir, "plaque.dcv", "plaque.dcv");
    let mut project = load_file(&dcv).project;
    project.heights = heights;
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// Acceptance 45's heights: clearance 20 mm, machine Z −5 mm, park (0, 0).
fn acceptance_45_heights() -> dc_project::Heights {
    dc_project::Heights {
        clearance_mm: Some(20.0),
        tool_change_machine_z_mm: Some(-5.0),
        park: Some(dc_project::ParkXy {
            x_mm: 0.0,
            y_mm: 0.0,
        }),
    }
}

/// Each line of `program` with code, comments removed, with its 1-based
/// line number and the operation whose header comment it follows.
fn code_by_operation(program: &str) -> Vec<(usize, Option<usize>, String)> {
    let mut op = None;
    let mut out = Vec::new();
    for (i, line) in program.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("( Operation ") {
            op = rest.split(':').next().and_then(|n| n.parse().ok());
            continue;
        }
        let code = match line.find('(') {
            Some(at) => line[..at].trim(),
            None => line.trim(),
        };
        if !code.is_empty() {
            out.push((i + 1, op, code.to_owned()));
        }
    }
    out
}

/// Acceptance 45 (#283, functional capability 33): the two-tool plaque on
/// grblHAL in one program, with `clearance_mm: 20`,
/// `tool_change_machine_z_mm: -5` and `park: (0, 0)`. Every move in XY
/// between parts and between operations is at Z 20 (work zero at the
/// stock top), and between passes of one part at its safe Z of 15: the
/// four drilled holes, each its own part, are crossed at 20; the Cut out
/// Profile's passes at 15. The tool change is the rise to 20, `G53 G0
/// Z-5.000`, `M5`, `T2 M6`; the program ends with a rapid to the park
/// point at Z 20, then the lift, then the footer. Nothing goes below the
/// material bottom less the through-cut allowance (S1), and the export
/// says to home the machine.
#[test]
fn heights_plaque_one_program() {
    let dir = work_dir("heights_plaque_one_program");
    plaque_with_heights(&dir, acceptance_45_heights());
    let out = run(
        &dir,
        &[
            "export",
            "plaque.dcv",
            "--one-program",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let program = std::fs::read_to_string(dir.join("plaque.nc")).expect("program written");
    let moves = dc_gcode::parse_motion(&program).expect("program parses");
    let op_of: Vec<(usize, Option<usize>, String)> = code_by_operation(&program);
    let op_at = |line: usize| {
        op_of
            .iter()
            .find(|(l, ..)| *l == line)
            .and_then(|(_, op, _)| *op)
    };

    // S1: the plaque is 11.8 mm thick, cut through with 0.2 mm allowance.
    for mv in &moves {
        if let Some(z) = mv.to.z {
            assert!(
                z >= -12.0 - 1e-9,
                "line {} below the floor: {mv:?}",
                mv.line
            );
        }
    }
    // Every rapid in XY, by operation, at the clearance or safe Z.
    let xy_rapids: Vec<(usize, Option<usize>, f64)> = moves
        .iter()
        .filter(|m| m.kind == dc_gcode::parse::MotionKind::Rapid)
        .filter(|m| m.to.x.is_some() && (m.from.x != m.to.x || m.from.y != m.to.y))
        .map(|m| (m.line, op_at(m.line), m.to.z.expect("Z known")))
        .collect();
    for &(line, _, z) in &xy_rapids {
        assert!(
            (z - 20.0).abs() < 1e-9 || (z - 15.0).abs() < 1e-9,
            "line {line}: XY rapid at Z {z}"
        );
    }
    // Into each operation at the clearance height.
    for op in 1..=5 {
        let first = xy_rapids
            .iter()
            .find(|(_, o, _)| *o == Some(op))
            .unwrap_or_else(|| panic!("operation {op} has an XY rapid"));
        assert!(
            (first.2 - 20.0).abs() < 1e-9,
            "into operation {op}: {first:?}"
        );
    }
    // Drill: four holes, four parts, each reached at 20.
    let drill: Vec<f64> = xy_rapids
        .iter()
        .filter(|(_, o, _)| *o == Some(3))
        .map(|r| r.2)
        .collect();
    assert!(
        drill.len() == 4 && drill.iter().all(|z| (z - 20.0).abs() < 1e-9),
        "{xy_rapids:?}"
    );
    // Cut out: one part, its passes linked at 15.
    let cut_out: Vec<f64> = xy_rapids
        .iter()
        .filter(|(_, o, _)| *o == Some(4))
        .map(|r| r.2)
        .skip(1)
        .collect();
    assert!(
        !cut_out.is_empty() && cut_out.iter().all(|&z| (z - 15.0).abs() < 1e-9),
        "{cut_out:?}"
    );

    let code: Vec<&str> = op_of.iter().map(|(_, _, c)| c.as_str()).collect();
    let change = code
        .iter()
        .position(|c| c.starts_with("T2 M6"))
        .expect("a tool change");
    assert!(
        ["Z20.000", "G0 Z20.000"].contains(&code[change - 3]),
        "{:?}",
        &code[change - 3..=change]
    );
    assert_eq!(code[change - 2..=change], ["G53 G0 Z-5.000", "M5", "T2 M6"]);
    let n = code.len();
    assert_eq!(
        code[n - 5..],
        ["Z20.000", "X0.000 Y0.000", "G53 G0 Z-5.000", "M5", "M30"],
        "{program}"
    );
    assert!(
        stdout(&out).contains(
            "Before you start: home the machine; this program moves to machine Z -5.000 mm (G53)."
        ),
        "{}",
        stdout(&out)
    );
}

/// Acceptance 45, the errors: on Mach3, whose manual does not establish
/// what its `G53` does with a tool length offset (#306), the machine Z is
/// an error naming the setting at its place in the file; a clearance
/// below an enabled operation's safe Z (15 on the plaque) is an error
/// naming the operation. With all three unset the version 11 plaque
/// exports the programs it did before #283.
#[test]
fn heights_errors_and_unset_heights() {
    let dir = work_dir("heights_errors_and_unset_heights");
    let dcv = plaque_with_heights(&dir, acceptance_45_heights());
    let mut project = load_file(&dcv).project;
    project.post_processor =
        dc_project::PostProcessorRef::latest("mach3").expect("mach3 is bundled");
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["check", "plaque.dcv"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("error: heights.tool_change_machine_z_mm is set, but post-processor `mach3`"),
        "{text}"
    );
    assert!(text.contains("plaque.dcv:"), "located: {text}");

    plaque_with_heights(
        &dir,
        dc_project::Heights {
            clearance_mm: Some(10.0),
            ..dc_project::Heights::unset()
        },
    );
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stderr(&out).contains(
            "heights.clearance_mm is 10 mm, below the 15 mm safe_z_mm of operation `Panel`"
        ),
        "{}",
        stderr(&out)
    );

    // The plaque as the build before #283 saved it (version 11, grblHAL
    // v2), against the expected programs of that build.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/heights-migration/plaque.dcv"),
        &dcv,
    )
    .expect("corpus plaque copied");
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_corpus_programs(&dir, "plaque", "heights unset");
    assert!(
        !stdout(&out).contains("home the machine"),
        "{}",
        stdout(&out)
    );
}

/// #306: acceptance 45's heights on LinuxCNC. `check` accepts the
/// machine Z, and `export` writes one program per tool run (LinuxCNC has
/// no `M6`), each ending with the rise to the clearance height, the park
/// rapid, `G53 G0 Z-5.000`, then the footer `M5`, `M2`, and says to home
/// the machine.
#[test]
fn heights_on_linuxcnc() {
    let dir = work_dir("heights_on_linuxcnc");
    let dcv = plaque_with_heights(&dir, acceptance_45_heights());
    let mut project = load_file(&dcv).project;
    project.post_processor =
        dc_project::PostProcessorRef::latest("linuxcnc").expect("linuxcnc is bundled");
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["check", "plaque.dcv"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "export: {}", stderr(&out));
    let mut programs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("dir readable")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "ngc"))
        .collect();
    programs.sort();
    assert_eq!(programs.len(), 2, "{programs:?}");
    for path in &programs {
        let program = std::fs::read_to_string(path).expect("program written");
        let code: Vec<String> = code_by_operation(&program)
            .into_iter()
            .map(|(_, _, c)| c)
            .collect();
        let n = code.len();
        assert_eq!(
            code[n - 5..],
            ["Z20.000", "X0.000 Y0.000", "G53 G0 Z-5.000", "M5", "M2"],
            "{program}"
        );
        assert_eq!(
            code.iter().filter(|c| c.contains("G53")).count(),
            1,
            "{program}"
        );
    }
    assert_eq!(
        stdout(&out)
            .matches(
                "Before you start: home the machine; each program moves to machine Z -5.000 mm (G53)."
            )
            .count(),
        1,
        "{}",
        stdout(&out)
    );
}

/// Design addendum A7: `export` says to home the machine when its
/// programs lift with `G53`, since a machine position means nothing on an
/// unhomed machine and DreamCarve cannot check; with one program per tool
/// run, once for all of them. Without the setting it says nothing.
#[test]
fn export_prints_home_note() {
    let dir = work_dir("export_prints_home_note");
    plaque_with_heights(
        &dir,
        dc_project::Heights {
            tool_change_machine_z_mm: Some(-5.0),
            ..dc_project::Heights::unset()
        },
    );
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(
        text.matches(
            "Before you start: home the machine; each program moves to machine Z -5.000 mm (G53)."
        )
        .count(),
        1,
        "{text}"
    );
    let out = run(
        &dir,
        &[
            "export",
            "plaque.dcv",
            "--one-program",
            "--date",
            FIXED_DATE,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains(
            "Before you start: home the machine; this program moves to machine Z -5.000 mm (G53)."
        ),
        "{}",
        stdout(&out)
    );
    plaque_with_heights(&dir, dc_project::Heights::unset());
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        !stdout(&out).contains("home the machine"),
        "{}",
        stdout(&out)
    );
}

/// #308 on LinuxCNC (#306): `$20` is GRBL's, so the export note names
/// LinuxCNC's condition: its soft limits apply only once the machine is
/// homed, which it enforces unless `[TRAJ] NO_FORCE_HOMING` is set
/// (definitions README, "Machine-coordinate moves").
#[test]
fn export_machine_z_note_on_linuxcnc() {
    let dir = work_dir("export_machine_z_note_on_linuxcnc");
    let dcv = plaque_with_heights(
        &dir,
        dc_project::Heights {
            tool_change_machine_z_mm: Some(-5.0),
            ..dc_project::Heights::unset()
        },
    );
    let mut project = load_file(&dcv).project;
    project.post_processor =
        dc_project::PostProcessorRef::latest("linuxcnc").expect("linuxcnc is bundled");
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    let note = "Before you start: home the machine; each program moves to machine Z -5.000 mm (G53). \
                LinuxCNC applies soft limits only once homed: leave [TRAJ] NO_FORCE_HOMING unset.";
    assert_eq!(text.lines().filter(|l| *l == note).count(), 1, "{text}");
    assert!(!text.contains("$20"), "{text}");
}

/// #308: on a machine that homes Z downward every negative machine Z is
/// below the travel (GRBL `system.c` 340-344, grblHAL `machine_limits.c`
/// 129-131), and only soft limits stop the lift from driving Z down, so
/// the export note names them, `$20=1`, for one program and for a run per
/// tool. Unset, it says nothing about them.
#[test]
fn export_machine_z_note_names_soft_limits() {
    let dir = work_dir("export_machine_z_note_names_soft_limits");
    plaque_with_heights(
        &dir,
        dc_project::Heights {
            tool_change_machine_z_mm: Some(-5.0),
            ..dc_project::Heights::unset()
        },
    );
    for (args, note) in [
        (
            &["export", "plaque.dcv", "--date", FIXED_DATE][..],
            "Before you start: home the machine; each program moves to machine Z -5.000 mm (G53). \
             Z must home upward, or soft limits must be on ($20=1).",
        ),
        (
            &[
                "export",
                "plaque.dcv",
                "--one-program",
                "--date",
                FIXED_DATE,
            ][..],
            "Before you start: home the machine; this program moves to machine Z -5.000 mm (G53). \
             Z must home upward, or soft limits must be on ($20=1).",
        ),
    ] {
        let out = run(&dir, args);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let text = stdout(&out);
        assert_eq!(
            text.lines().filter(|l| *l == note).count(),
            1,
            "{args:?}\n{text}"
        );
        assert_eq!(text.matches("$20=1").count(), 1, "{args:?}\n{text}");
    }
    plaque_with_heights(&dir, dc_project::Heights::unset());
    let out = run(&dir, &["export", "plaque.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!stdout(&out).contains("$20"), "{}", stdout(&out));
}

/// #283: the repository's corpus of version 11 projects, saved with
/// placed tabs by the build before #283 (`tabs place` on each project of
/// the tabs-migration corpus), with the programs they export.
fn heights_migration_corpus() -> Vec<(String, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/heights-migration");
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .expect("corpus readable")
        .map(|e| e.expect("corpus entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "dcv"))
        .map(|p| {
            let stem = p.file_stem().expect("stem").to_string_lossy().into_owned();
            (stem, p)
        })
        .collect();
    names.sort();
    names
}

/// #283, design addendum A7: every version 11 project of the corpus
/// migrates to version 12 with its heights unset and exports byte for
/// byte the programs the build before #283 exported (the tabs-migration
/// expected programs, which that build reproduced), as loaded and after
/// it is saved at version 12.
#[test]
fn version_11_projects_export_byte_identical_programs() {
    let dir = work_dir("version_11_projects_export_byte_identical_programs");
    let corpus = heights_migration_corpus();
    assert_eq!(corpus.len(), 16, "{corpus:?}");
    for (name, path) in corpus {
        let file = format!("{name}.dcv");
        std::fs::copy(&path, dir.join(&file)).expect("corpus copied");
        let text = std::fs::read_to_string(dir.join(&file)).expect("dcv readable");
        assert!(text.contains("\"version\": 11,"), "{name} is version 11");
        assert!(!text.contains("\"heights\""), "{name} has no heights");

        let out = run(&dir, &["export", &file, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        assert_corpus_programs(&dir, &name, "version 11");

        let project = load_file(&dir.join(&file)).project;
        assert_eq!(project.version, dc_project::CURRENT_VERSION);
        assert_eq!(project.heights, dc_project::Heights::unset(), "{name}");
        std::fs::write(dir.join(&file), save(&project).expect("saves")).expect("dcv written");
        for entry in std::fs::read_dir(&dir).expect("dir readable") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|x| x == "nc") {
                std::fs::remove_file(path).expect("program removed");
            }
        }
        let out = run(&dir, &["export", &file, "--date", FIXED_DATE]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", stderr(&out));
        assert_corpus_programs(&dir, &name, "version 12");
    }
}
