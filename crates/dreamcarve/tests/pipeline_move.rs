// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Phase 4, slice 1 (`docs/specs/phase-4/design.md`, "Test plan → dc-job
//! (slice 1)"; #530): the pipeline moved into `dc-job` produces the bytes
//! the `dreamcarve` binary writes. Each test runs the CLI on a committed
//! project or drawing, in its own directory under `CARGO_TARGET_TMPDIR`,
//! and compares every file it wrote with what `dc-job` computes for the
//! same input in this process, so no second copy of any output is kept.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dc_import::{DxfImportOptions, SvgImportOptions, import_dxf, import_svg};
use dc_job::import::Drawing;
use dc_job::preview::render_generated;
use dc_job::tabs::place_request;
use dc_job::{ProgramSplit, program_names, project_from};
use dc_project::{OperationId, PostProcessorRef, Project, Units};

/// The date every compared program is exported with.
const DATE: &str = "2026-10-15";

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// Runs `dreamcarve args` in `dir` with no user config, and fails the test
/// on a non-zero exit.
fn run(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
        .current_dir(dir)
        .env("DREAMCARVE_CONFIG_DIR", dir.join("config"))
        .args(args)
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`dreamcarve {}` in {}:\n{}{}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn load_dcv(path: &Path) -> Project {
    dc_project::load(&std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .project
}

/// The first line where `actual` and `expected` differ, for a message.
fn first_difference(actual: &str, expected: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (actual.lines().collect(), expected.lines().collect());
    for (k, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            return format!("line {}: `{x}`, not `{y}`", k + 1);
        }
    }
    format!("{} lines, not {}", a.len(), b.len())
}

/// One committed project: its file stem (the CLI's program stem) and path.
type Fixture = (String, PathBuf);

/// The quick start chapters' projects (`tests/fixtures/*.dcv`), the golden
/// cases with a DreamCarve project (`tests/golden/<case>/dreamcarve.dcv`)
/// and five version 9 corpus projects with tab requests
/// (`tests/fixtures/tabs-migration/`), sorted within each group.
fn checked_projects() -> Vec<Fixture> {
    let root = repo_root();
    let mut all: Vec<Fixture> = ["plaque", "plaque-svg", "sign", "dc"]
        .into_iter()
        .map(|n| {
            (
                n.to_owned(),
                root.join("tests/fixtures").join(format!("{n}.dcv")),
            )
        })
        .collect();
    let mut golden: Vec<Fixture> = std::fs::read_dir(root.join("tests/golden"))
        .expect("tests/golden readable")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.join("dreamcarve.dcv").is_file())
        .map(|p| {
            let name = p.file_name().expect("case").to_string_lossy().into_owned();
            (name, p.join("dreamcarve.dcv"))
        })
        .collect();
    golden.sort();
    assert!(golden.len() >= 20, "{} golden projects", golden.len());
    all.extend(golden);
    all.extend(
        [
            "rect-outside-climb",
            "square-with-hole-on-holes",
            "circle-rotated",
            "star-with-hole",
            "two-parts-pass-first",
        ]
        .into_iter()
        .map(|n| {
            (
                n.to_owned(),
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/tabs-migration")
                    .join(format!("{n}.dcv")),
            )
        }),
    );
    all
}

/// Every committed project exported by the binary (`export --date`) gives
/// byte for byte the programs [`dc_job::build()`] emits for it, under the
/// names [`program_names`] gives them: the quick-start projects (two
/// tools, four tools, an Inlay's two boards), the golden cases, the
/// tab-request corpus.
#[test]
fn cli_programs_are_byte_identical_after_the_move() {
    let dir = work_dir("cli_programs_are_byte_identical_after_the_move");
    let mut compared = 0;
    for (stem, path) in checked_projects() {
        let file = format!("{stem}.dcv");
        std::fs::copy(&path, dir.join(&file)).expect("project copied");
        run(&dir, &["export", &file, "--date", DATE]);
        let project = load_dcv(&path);
        let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
        assert!(
            built.findings.errors.is_empty(),
            "{stem}: {:?}",
            built.findings.errors
        );
        assert!(!built.programs.is_empty(), "{stem}");
        for (name, program) in program_names(&project, &stem, &built.programs)
            .iter()
            .zip(&built.programs)
        {
            let written = read(&dir.join(name));
            assert!(
                program.text == written,
                "{stem}: {name} from dc_job::build differs from what the CLI wrote: {}",
                first_difference(&program.text, &written)
            );
            std::fs::remove_file(dir.join(name)).expect("program removed");
            compared += 1;
        }
    }
    assert!(compared >= 40, "{compared} programs compared");
}

/// Every committed project's `check --preview` SVG is byte for byte
/// [`render_generated`] of [`dc_job::build()`]'s runs: straight and arc
/// tab marks, start points, drill holes, the male board beside the stock,
/// the legend.
#[test]
fn render_is_unchanged_after_the_move() {
    let dir = work_dir("render_is_unchanged_after_the_move");
    let mut compared = 0;
    for (stem, path) in checked_projects() {
        let file = format!("{stem}.dcv");
        let svg = format!("{stem}.svg");
        std::fs::copy(&path, dir.join(&file)).expect("project copied");
        run(&dir, &["check", &file, "--preview", &svg]);
        let project = load_dcv(&path);
        let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
        let generated = built
            .generated
            .unwrap_or_else(|| panic!("{stem}: {:?}", built.findings));
        let rendered = render_generated(&project, &generated);
        let written = read(&dir.join(&svg));
        assert!(
            rendered == written,
            "{stem}: the preview from dc_job differs from what the CLI wrote: {}",
            first_difference(&rendered, &written)
        );
        compared += 1;
    }
    assert!(compared >= 25, "{compared} previews compared");
}

/// The drawing at `path` imported as `init` imports it, with no flags.
fn import(path: &Path) -> Drawing {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|x| x == "dxf") {
        Drawing::Dxf(import_dxf(&bytes, &DxfImportOptions::new()).unwrap().0)
    } else {
        Drawing::Svg(import_svg(&bytes, &SvgImportOptions::new()).unwrap().0)
    }
}

/// Acceptance 63, first half: for the coaster, the plaque (DXF and SVG),
/// the sign and the inlay letters, `init` writes byte for byte the project
/// [`project_from::drawing`] builds with defaults.
#[test]
fn drawing_with_defaults_equals_init() {
    let dir = work_dir("drawing_with_defaults_equals_init");
    let root = repo_root();
    for (name, drawing) in [
        ("coaster", root.join("fixtures/coaster.svg")),
        ("plaque", root.join("tests/fixtures/plaque.dxf")),
        ("plaque", root.join("tests/fixtures/plaque.svg")),
        ("sign", root.join("tests/fixtures/sign.svg")),
        ("dc-inlay", root.join("tests/fixtures/dc-inlay.svg")),
    ] {
        let file = drawing.file_name().unwrap().to_string_lossy().into_owned();
        let written = format!("{}.dcv", file.replace('.', "-"));
        std::fs::copy(&drawing, dir.join(&file)).expect("drawing copied");
        run(&dir, &["init", &file, "-o", &written]);
        let post = PostProcessorRef::latest("grblhal").unwrap();
        let project = project_from::drawing(name, import(&drawing), post, Units::Mm, true)
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let text = dc_project::save(&project).unwrap();
        let init = read(&dir.join(&written));
        assert!(
            text == init,
            "{file}: project_from::drawing differs from what `init` wrote: {}",
            first_difference(&text, &init)
        );
    }
}

/// Acceptance 78, first half: for each corpus project with a tab request,
/// `tabs place <dcv> o1` saves byte for byte the project
/// [`place_request`] gives: the request placed as tabs and cleared.
#[test]
fn place_request_equals_tabs_place() {
    let dir = work_dir("place_request_equals_tabs_place");
    let op = OperationId::new("o1");
    for name in [
        "rect-outside-climb",
        "square-with-hole-on-holes",
        "circle-rotated",
        "star-with-hole",
    ] {
        let file = format!("{name}.dcv");
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tabs-migration")
            .join(&file);
        std::fs::copy(&path, dir.join(&file)).expect("project copied");
        run(&dir, &["tabs", "place", &file, "o1"]);
        let project = load_dcv(&path);
        let (placed, _warnings) =
            place_request(&project, &op).unwrap_or_else(|e| panic!("{name}: {e}"));
        let text = dc_project::save(&placed).unwrap();
        let saved = read(&dir.join(&file));
        assert!(
            text == saved,
            "{name}: place_request differs from what `tabs place` saved: {}",
            first_difference(&text, &saved)
        );
        assert!(!placed.tabs.is_empty(), "{name}");
        assert!(
            matches!(
                place_request(&placed, &op),
                Err(dc_job::JobError::NoTabRequest { .. })
            ),
            "{name}: placed once, nothing is left to place"
        );
    }
}
