// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Release checks for issue #31: every release archive carries the license
//! texts of the crates linked into the `dreamcarve` binary.
//!
//! Both tests are `#[ignore]`d so `cargo test --all` stays offline and needs no
//! extra tools. CI runs them with `--ignored`:
//!
//! - `third_party_licenses_cover_lockfile` needs `cargo-about` on `PATH`. The
//!   `third-party licenses` job in `.github/workflows/ci.yml` runs it on every
//!   PR, so a `Cargo.lock` change that adds an unresolvable license fails CI.
//! - `release_archive_contains_third_party_licenses` also needs `dist`, and
//!   does a full release build. `.github/workflows/release-archive.yml` runs
//!   it on PRs that touch the release configuration and on release tags.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use third_party_licenses::{
    ARCHIVE_FILES, LICENSES_FILE, archive_entries, check, dist_build_archive, generate,
    host_target, licensed_crates, missing_archive_files,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The generated file names every non-workspace crate that `cargo tree`
/// resolves into the `dreamcarve` binary on any dist target, each under a
/// non-empty license text. `generate` itself fails (`cargo about --fail`) when
/// a license cannot be resolved to a text.
#[test]
#[ignore = "needs cargo-about; CI runs it with --ignored"]
fn third_party_licenses_cover_lockfile() {
    let root = workspace_root();
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(LICENSES_FILE);

    generate(&root, &out).expect("cargo about generates THIRD-PARTY-LICENSES");
    let coverage = check(&root, &out).expect("cargo tree resolves the shipped crates");

    assert!(
        !coverage.shipped.is_empty(),
        "dreamcarve links third-party crates (clap at least), so the shipped set is not empty"
    );
    assert!(
        coverage.uncovered.is_empty(),
        "every shipped crate has a license text in {LICENSES_FILE}; missing: {:?}",
        coverage.uncovered
    );

    let text = fs::read_to_string(&out).expect("generated file is readable");
    let listed = licensed_crates(&text);
    assert!(
        listed.iter().any(|c| c.name == "clap"),
        "clap, the CLI's direct dependency, is listed with its license"
    );
    assert!(
        listed.iter().all(|c| c.name != "dreamcarve"),
        "the file lists third-party crates only, not DreamCarve itself"
    );
}

/// A `dist build` archive for the host target carries the license files at its
/// root: LICENSE (dist's auto-include), NOTICE and THIRD-PARTY-LICENSES (both
/// from `include` in dist-workspace.toml).
#[test]
#[ignore = "needs cargo-about and dist, and runs a release build; CI runs it with --ignored"]
fn release_archive_contains_third_party_licenses() {
    let root = workspace_root();
    generate(&root, &root.join(LICENSES_FILE)).expect("cargo about generates THIRD-PARTY-LICENSES");

    let target = host_target().expect("rustc reports the host target");
    let archive = dist_build_archive(&root, &target).expect("dist builds the host archive");
    let entries = archive_entries(&archive).expect("tar lists the archive");

    assert_eq!(
        missing_archive_files(&entries),
        Vec::<&str>::new(),
        "{} contains {ARCHIVE_FILES:?}; entries: {entries:?}",
        archive.display()
    );
}
