// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Program names (`docs/specs/phase-4/design.md`, "Test plan → dc-job
//! (slice 1)"): as the CLI wrote them before the move.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_job::{ProgramSplit, program_names, program_suffixes};

use common::{DATE, load_dcv, repo_root};

/// The names the CLI wrote for a one-tool project (`<stem>.nc`), a
/// project with one program per tool run (`<stem>-<k>-T<n>.nc`) and an
/// Inlay's two boards (`<stem>-<board>-<k>-T<n>.nc`, #498), read off the
/// files the build before the move left (`expected/golden/`,
/// `expected/quick-start/`).
#[test]
fn program_names_match_the_cli() {
    let names = |path: &str, stem: &str| {
        let project = load_dcv(&repo_root().join(path));
        let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
        assert!(
            built.findings.errors.is_empty(),
            "{stem}: {:?}",
            built.findings
        );
        (
            program_names(&project, stem, &built.programs),
            program_suffixes(&project, &built.programs),
        )
    };
    // One tool: one program, named after the stem alone.
    let (single, suffixes) = names("tests/golden/profile-ramp/dreamcarve.dcv", "profile-ramp");
    assert_eq!(single, ["profile-ramp.nc"]);
    assert_eq!(suffixes, [""]);
    // Two tools, one program per run: the run's place and tool number.
    let (per_tool, _) = names("tests/fixtures/plaque.dcv", "plaque");
    assert_eq!(per_tool, ["plaque-1-T2.nc", "plaque-2-T1.nc"]);
    let (per_tool, _) = names("tests/fixtures/sign.dcv", "sign");
    assert_eq!(
        per_tool,
        [
            "sign-1-T2.nc",
            "sign-2-T1.nc",
            "sign-3-T3.nc",
            "sign-4-T1.nc"
        ]
    );
    // An Inlay: each board's programs, counted from 1 within the board.
    let (boards, suffixes) = names("tests/fixtures/dc.dcv", "dc");
    assert_eq!(
        boards,
        [
            "dc-female-1-T2.nc",
            "dc-female-2-T1.nc",
            "dc-male-1-T2.nc",
            "dc-male-2-T1.nc",
            "dc-male-3-T2.nc",
        ]
    );
    assert_eq!(suffixes[2], "-male-1-T2");
    // The stem is the caller's: the app names programs after the file it
    // saves to, as `-o` does.
    let (renamed, _) = names("tests/fixtures/plaque.dcv", "job");
    assert_eq!(renamed, ["job-1-T2.nc", "job-2-T1.nc"]);
}
