// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The fixtures the slice 1 tests share (`docs/specs/phase-4/design.md`,
//! "Test plan → dc-job (slice 1)"): the committed projects, and under
//! `tests/expected/` what the `dreamcarve` CLI wrote for them before the
//! pipeline moved here, captured from the build at `main` (834bb70) with
//! `export --date 2026-10-15`, `check --preview`, `init` and `tabs place`.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::path::{Path, PathBuf};

use dc_project::Project;

/// The date every compared program was exported with.
pub const DATE: &str = "2026-10-15";

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `tests/expected/<group>/<name>` of this crate.
pub fn expected(group: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected")
        .join(group)
        .join(name)
}

/// The text of the file at `path`, or a failure naming it.
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The project in the `.dcv` at `path`, migrated.
pub fn load_dcv(path: &Path) -> Project {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    dc_project::load(&bytes)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .project
}

/// One committed project: its file stem (the CLI's program stem) and path.
pub type Fixture = (String, PathBuf);

/// The quick start chapters' projects, `tests/fixtures/*.dcv`
/// (`expected/quick-start/`).
pub fn quick_start_projects() -> Vec<Fixture> {
    ["plaque", "plaque-svg", "sign", "dc"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                repo_root()
                    .join("tests/fixtures")
                    .join(format!("{name}.dcv")),
            )
        })
        .collect()
}

/// The golden cases that have a DreamCarve project,
/// `tests/golden/<case>/dreamcarve.dcv` (`expected/golden/`), sorted.
pub fn golden_projects() -> Vec<Fixture> {
    let mut cases: Vec<Fixture> = std::fs::read_dir(repo_root().join("tests/golden"))
        .expect("tests/golden readable")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.join("dreamcarve.dcv").is_file())
        .map(|p| {
            let name = p.file_name().expect("case").to_string_lossy().into_owned();
            (name, p.join("dreamcarve.dcv"))
        })
        .collect();
    cases.sort();
    assert!(cases.len() >= 20, "{} golden projects", cases.len());
    cases
}

/// The version 9 corpus projects with tab requests
/// (`crates/dreamcarve/tests/fixtures/tabs-migration/`) whose previews are
/// committed under `expected/corpus/` and whose programs under the CLI's
/// `tests/expected/tabs-migration/`.
pub fn corpus_projects() -> Vec<Fixture> {
    [
        "rect-outside-climb",
        "square-with-hole-on-holes",
        "circle-rotated",
        "star-with-hole",
        "two-parts-pass-first",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            repo_root()
                .join("crates/dreamcarve/tests/fixtures/tabs-migration")
                .join(format!("{name}.dcv")),
        )
    })
    .collect()
}

/// Every project with committed outputs: the three groups above.
pub fn checked_projects() -> Vec<Fixture> {
    let mut all = quick_start_projects();
    all.extend(golden_projects());
    all.extend(corpus_projects());
    all
}

/// The first line where `actual` and `expected` differ, for a message.
pub fn first_difference(actual: &str, expected: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (actual.lines().collect(), expected.lines().collect());
    for (k, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            return format!("line {}: `{x}`, not `{y}`", k + 1);
        }
    }
    format!("{} lines, not {}", a.len(), b.len())
}
