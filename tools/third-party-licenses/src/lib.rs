// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Generates `THIRD-PARTY-LICENSES` for the release archives and checks that it
//! covers every crate linked into the `dreamcarve` binary (issue #31).
//!
//! Generation is `cargo about generate` with the workspace's `about.toml` and
//! `about.hbs`. The check is independent of cargo-about: it resolves the
//! shipped crates with `cargo metadata --filter-platform` for each dist target
//! and requires each one to appear in the generated file under a non-empty
//! license text.
//!
//! This is a dev-only tool: it runs other programs and reads files, which the
//! `dc-*` library rules forbid but a tool needs.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The generated file's name, at the workspace root and in every archive.
pub const LICENSES_FILE: &str = "THIRD-PARTY-LICENSES";

/// The package whose binary the release archives ship.
pub const SHIPPED_PACKAGE: &str = "dreamcarve";

/// Files every release archive must carry at its root.
pub const ARCHIVE_FILES: [&str; 3] = ["LICENSE", "NOTICE", LICENSES_FILE];

/// Separator line between license sections in the generated file (`about.hbs`).
pub const SECTION_RULE: &str =
    "================================================================================";

/// A crate in `Cargo.lock`, by name and exact version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CrateId {
    /// Package name.
    pub name: String,
    /// Exact version from the lock file.
    pub version: String,
}

impl CrateId {
    /// Builds a crate id from its name and version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl fmt::Display for CrateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
}

/// Result of checking a generated file against the shipped crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Third-party crates linked into the binary on at least one dist target.
    pub shipped: BTreeSet<CrateId>,
    /// Shipped crates the file does not list under a license text.
    pub uncovered: Vec<CrateId>,
}

/// Failures of the generator and the checks.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A program could not be started (usually: not installed).
    #[error("cannot run `{command}`: {source}")]
    Spawn {
        /// The command line.
        command: String,
        /// The OS error.
        source: std::io::Error,
    },
    /// A program exited unsuccessfully.
    #[error("`{command}` failed ({status}):\n{stderr}")]
    Command {
        /// The command line.
        command: String,
        /// Its exit status.
        status: String,
        /// Its standard error.
        stderr: String,
    },
    /// A file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The OS error.
        source: std::io::Error,
    },
    /// Program output or a config file did not have the expected shape.
    #[error("{0}")]
    Format(String),
}

/// Reads the `targets = [...]` list from `dist-workspace.toml`, so the
/// licenses cover exactly the platforms dist builds.
pub fn dist_targets(dist_toml: &str) -> Result<Vec<String>, Error> {
    let _ = dist_toml;
    Err(Error::Format("not implemented".into()))
}

/// The third-party crates linked into `package`, from `cargo metadata` JSON:
/// every package reachable from `package` through normal (not dev or build)
/// dependencies, minus workspace members.
pub fn shipped_crates(metadata_json: &str, package: &str) -> Result<BTreeSet<CrateId>, Error> {
    let _ = (metadata_json, package);
    Err(Error::Format("not implemented".into()))
}

/// The crates a generated file lists under a non-empty license text.
pub fn licensed_crates(licenses_file: &str) -> BTreeSet<CrateId> {
    let _ = licenses_file;
    BTreeSet::new()
}

/// Shipped crates missing from `licensed`.
pub fn uncovered(shipped: &BTreeSet<CrateId>, licensed: &BTreeSet<CrateId>) -> Vec<CrateId> {
    let _ = (shipped, licensed);
    Vec::new()
}

/// Which of [`ARCHIVE_FILES`] are not at the root of an archive, given its
/// entry paths (dist puts everything under one top-level directory).
pub fn missing_archive_files(entries: &[String]) -> Vec<&'static str> {
    let _ = entries;
    ARCHIVE_FILES.to_vec()
}

/// Writes `output` with `cargo about generate`, for the dist targets.
/// Fails when a shipped crate's license cannot be resolved to a text.
pub fn generate(root: &Path, output: &Path) -> Result<(), Error> {
    let _ = (root, output);
    Err(Error::Format("not implemented".into()))
}

/// Checks `licenses_file` against the crates `cargo metadata` resolves into
/// the shipped binary on any dist target.
pub fn check(root: &Path, licenses_file: &Path) -> Result<Coverage, Error> {
    let _ = (root, licenses_file);
    Err(Error::Format("not implemented".into()))
}

/// The host target triple, from `rustc -vV`.
pub fn host_target() -> Result<String, Error> {
    Err(Error::Format("not implemented".into()))
}

/// Runs `dist build` for `target` and returns the archive it wrote.
pub fn dist_build_archive(root: &Path, target: &str) -> Result<PathBuf, Error> {
    let _ = (root, target, Command::new("dist"));
    Err(Error::Format("not implemented".into()))
}

/// Lists an archive's entries with `tar -tf` (bsdtar also reads zip).
pub fn archive_entries(archive: &Path) -> Result<Vec<String>, Error> {
    let _ = (archive, Value::Null);
    Err(Error::Format("not implemented".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[(&str, &str)]) -> BTreeSet<CrateId> {
        ids.iter().map(|(n, v)| CrateId::new(*n, *v)).collect()
    }

    #[test]
    fn dist_targets_reads_the_targets_list() {
        let toml = "[dist]\n# Target platforms\ntargets = [\"aarch64-apple-darwin\", \"x86_64-pc-windows-msvc\"]\ninstallers = [\"shell\"]\n";
        assert_eq!(
            dist_targets(toml).expect("targets parse"),
            vec!["aarch64-apple-darwin", "x86_64-pc-windows-msvc"],
            "both targets, in order"
        );
    }

    #[test]
    fn dist_targets_fails_without_a_targets_list() {
        assert!(
            dist_targets("[dist]\nci = \"github\"\n").is_err(),
            "a missing list is an error, not an empty target set"
        );
    }

    /// root -> a (normal) -> b (normal); root -> c (build only);
    /// root -> d (dev only); root -> ws (workspace member, normal) -> e.
    /// Shipped: a, b and e; c and d never link into the binary, ws is ours.
    const METADATA: &str = r#"{
      "packages": [
        {"id": "root", "name": "dreamcarve", "version": "0.0.1", "source": null},
        {"id": "a", "name": "a", "version": "1.0.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
        {"id": "b", "name": "b", "version": "2.1.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
        {"id": "c", "name": "c", "version": "0.3.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
        {"id": "d", "name": "d", "version": "0.4.0", "source": "registry+https://github.com/rust-lang/crates.io-index"},
        {"id": "ws", "name": "dc-geometry", "version": "0.0.1", "source": null},
        {"id": "e", "name": "e", "version": "5.0.0", "source": "registry+https://github.com/rust-lang/crates.io-index"}
      ],
      "resolve": {
        "root": null,
        "nodes": [
          {"id": "root", "deps": [
            {"name": "a", "pkg": "a", "dep_kinds": [{"kind": null, "target": null}]},
            {"name": "c", "pkg": "c", "dep_kinds": [{"kind": "build", "target": null}]},
            {"name": "d", "pkg": "d", "dep_kinds": [{"kind": "dev", "target": null}]},
            {"name": "dc_geometry", "pkg": "ws", "dep_kinds": [{"kind": null, "target": null}]}
          ]},
          {"id": "a", "deps": [
            {"name": "b", "pkg": "b", "dep_kinds": [{"kind": null, "target": null}]}
          ]},
          {"id": "b", "deps": []},
          {"id": "c", "deps": []},
          {"id": "d", "deps": []},
          {"id": "ws", "deps": [
            {"name": "e", "pkg": "e", "dep_kinds": [{"kind": "dev", "target": null}, {"kind": null, "target": null}]}
          ]},
          {"id": "e", "deps": []}
        ]
      }
    }"#;

    #[test]
    fn shipped_crates_follow_normal_dependencies_only() {
        assert_eq!(
            shipped_crates(METADATA, "dreamcarve").expect("metadata parses"),
            set(&[("a", "1.0.0"), ("b", "2.1.0"), ("e", "5.0.0")]),
        );
    }

    #[test]
    fn shipped_crates_fails_for_an_unknown_package() {
        assert!(
            shipped_crates(METADATA, "nope").is_err(),
            "an unknown root must not read as 'ships nothing'"
        );
    }

    fn section(name: &str, crates: &[&str], text: &str) -> String {
        let mut s = format!("{SECTION_RULE}\nLicense: {name}\nUsed by:\n");
        for c in crates {
            s.push_str(&format!("- {c}\n"));
        }
        s.push('\n');
        s.push_str(text);
        s.push('\n');
        s
    }

    #[test]
    fn licensed_crates_reads_each_section() {
        let file = format!(
            "THIRD-PARTY SOFTWARE LICENSES\n\nIntro text.\n\n{}{}",
            section(
                "MIT License (MIT)",
                &["strsim 0.11.1 (https://github.com/rapidfuzz/strsim-rs)", "a 1.0.0"],
                "Permission is hereby granted...\n- not a crate line\n"
            ),
            section("Apache License 2.0 (Apache-2.0)", &["b 2.1.0"], "TERMS AND CONDITIONS"),
        );
        assert_eq!(
            licensed_crates(&file),
            set(&[("a", "1.0.0"), ("b", "2.1.0"), ("strsim", "0.11.1")]),
            "crate lines end at the blank line; '- ' lines in a license text are not crates"
        );
    }

    #[test]
    fn licensed_crates_skips_sections_without_text() {
        let file = section("MIT License (MIT)", &["a 1.0.0"], "   \n");
        assert!(
            licensed_crates(&file).is_empty(),
            "a crate listed under an empty text has no license text"
        );
    }

    #[test]
    fn uncovered_lists_shipped_crates_missing_from_the_file() {
        let shipped = set(&[("a", "1.0.0"), ("b", "2.1.0")]);
        let licensed = set(&[("a", "1.0.0"), ("b", "2.0.0"), ("z", "9.9.9")]);
        assert_eq!(
            uncovered(&shipped, &licensed),
            vec![CrateId::new("b", "2.1.0")],
            "versions must match: b 2.0.0 does not cover b 2.1.0"
        );
    }

    #[test]
    fn missing_archive_files_checks_the_archive_root() {
        let entries: Vec<String> = [
            "dreamcarve-x86_64-unknown-linux-gnu/",
            "dreamcarve-x86_64-unknown-linux-gnu/dreamcarve",
            "dreamcarve-x86_64-unknown-linux-gnu/LICENSE",
            "dreamcarve-x86_64-unknown-linux-gnu/docs/NOTICE",
            "dreamcarve-x86_64-unknown-linux-gnu/THIRD-PARTY-LICENSES",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            missing_archive_files(&entries),
            vec!["NOTICE"],
            "a NOTICE in a subdirectory is not at the archive root"
        );
    }

    #[test]
    fn missing_archive_files_accepts_a_flat_archive() {
        let entries: Vec<String> = ["dreamcarve.exe", "LICENSE", "NOTICE", "THIRD-PARTY-LICENSES"]
            .map(String::from)
            .to_vec();
        assert!(
            missing_archive_files(&entries).is_empty(),
            "zip archives may have no top-level directory"
        );
    }
}
