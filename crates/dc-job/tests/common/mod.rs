// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The committed projects the slice 1 tests run over
//! (`docs/specs/phase-4/design.md`, "Test plan → dc-job (slice 1)"), read
//! from where they live: `tests/fixtures/`, `tests/golden/` and the CLI's
//! fixtures. Nothing is copied here; the byte-for-byte comparisons with
//! the CLI's own output run the binary, in
//! `crates/dreamcarve/tests/pipeline_move.rs`.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::path::{Path, PathBuf};

use dc_project::Project;

/// The date every compared program is exported with.
pub const DATE: &str = "2026-10-15";

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// The quick start chapters' projects, `tests/fixtures/*.dcv`.
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
/// `tests/golden/<case>/dreamcarve.dcv`, sorted. (The phase 1 Profile
/// cases build theirs in `dc-gcode`'s golden test.)
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

/// Five version 9 corpus projects with tab requests
/// (`crates/dreamcarve/tests/fixtures/tabs-migration/`): a rectangle, a
/// square with a hole and tabs on it, a rotated circle (tab marks on
/// arcs), a star with a hole, and two parts cut pass first.
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

/// The three groups above.
pub fn checked_projects() -> Vec<Fixture> {
    let mut all = quick_start_projects();
    all.extend(golden_projects());
    all.extend(corpus_projects());
    all
}
