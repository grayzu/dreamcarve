// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Sets `DREAMCARVE_VERSION`, the version `--version` prints
//! (`docs/plan.md`, "Versioning"). A build of the release tag `v<version>`
//! prints the Cargo version alone. Any other build is work toward that
//! release: the SemVer pre-release `-dev.<UTC build time>`, which sorts later
//! builds higher, plus the commit hash as build metadata,
//! `0.1.0-dev.20261018234512+g1402e19`. Without git (a source archive) the
//! hash is left off. `SOURCE_DATE_EPOCH`, when set, replaces the build time.

use std::path::Path;
use std::process::Command;

#[path = "src/clock.rs"]
#[allow(dead_code, reason = "the build script uses only the timestamp")]
mod clock;

/// Runs `git args` in the package directory; `None` when git is missing or
/// fails.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Reruns this script when HEAD moves or gains a tag: on checkout (HEAD), on
/// commit (the branch ref), on tagging (refs/tags), and on `git gc` or
/// `git pack-refs` (packed-refs).
fn rerun_on_new_commit() {
    let mut paths = vec![
        "HEAD".to_owned(),
        "packed-refs".to_owned(),
        "refs/tags".to_owned(),
    ];
    if let Some(head_ref) = git(&["rev-parse", "--symbolic-full-name", "HEAD"])
        && head_ref.starts_with("refs/")
    {
        paths.push(head_ref);
    }
    for path in paths {
        if let Some(on_disk) = git(&["rev-parse", "--git-path", &path])
            && Path::new(&on_disk).exists()
        {
            println!("cargo:rerun-if-changed={on_disk}");
        }
    }
}

/// Whether this is a build of the release tag: HEAD carries it, or GitHub
/// Actions is running for a push of it (its checkout may not fetch tags).
fn at_release_tag(tag: &str) -> bool {
    let github_tag = std::env::var("GITHUB_REF_TYPE").is_ok_and(|t| t == "tag")
        && std::env::var("GITHUB_REF_NAME").is_ok_and(|n| n == tag);
    github_tag || git(&["tag", "--points-at", "HEAD"]).is_some_and(|t| t.lines().any(|l| l == tag))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_TYPE");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    rerun_on_new_commit();
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let version = if at_release_tag(&format!("v{cargo_version}")) {
        cargo_version
    } else {
        // A path that never exists makes Cargo rerun this script on every
        // build, so the build time is the time of this build.
        println!("cargo:rerun-if-changed=build-time-is-always-stale");
        let built = std::env::var("SOURCE_DATE_EPOCH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(clock::now_secs);
        let dev = format!("{cargo_version}-dev.{}", clock::timestamp_utc(built));
        match git(&["rev-parse", "--short=7", "HEAD"]) {
            Some(hash) => format!("{dev}+g{hash}"),
            None => dev,
        }
    };
    println!("cargo:rustc-env=DREAMCARVE_VERSION={version}");
}
