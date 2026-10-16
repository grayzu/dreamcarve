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
    let missing = || Error::Format("dist-workspace.toml has no `targets = [...]` line".into());
    let list = dist_toml
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let rest = line.strip_prefix("targets")?.trim_start();
            rest.strip_prefix('=').map(str::trim)
        })
        .ok_or_else(missing)?;
    let inner = list
        .strip_prefix('[')
        .and_then(|l| l.strip_suffix(']'))
        .ok_or_else(|| Error::Format(format!("dist `targets` is not a one-line list: {list}")))?;
    let targets: Vec<String> = inner
        .split(',')
        .map(|t| t.trim().trim_matches('"').to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    if targets.is_empty() {
        return Err(missing());
    }
    Ok(targets)
}

/// The third-party crates linked into `package`, from `cargo metadata` JSON:
/// every package reachable from `package` through normal (not dev or build)
/// dependencies, minus workspace members.
pub fn shipped_crates(metadata_json: &str, package: &str) -> Result<BTreeSet<CrateId>, Error> {
    let bad = |what: &str| Error::Format(format!("cargo metadata output: {what}"));
    let meta: Value =
        serde_json::from_str(metadata_json).map_err(|e| bad(&format!("not JSON ({e})")))?;
    let packages = field(&meta, "packages")
        .as_array()
        .ok_or_else(|| bad("no `packages`"))?;
    let nodes = field(field(&meta, "resolve"), "nodes")
        .as_array()
        .ok_or_else(|| bad("no `resolve.nodes`"))?;

    let find_package = |id: &str| {
        packages
            .iter()
            .find(|p| field(p, "id").as_str() == Some(id))
    };
    let root = packages
        .iter()
        .find(|p| field(p, "name").as_str() == Some(package) && field(p, "source").is_null())
        .and_then(|p| field(p, "id").as_str())
        .ok_or_else(|| bad(&format!("no workspace package `{package}`")))?;

    let mut seen: BTreeSet<&str> = BTreeSet::from([root]);
    let mut stack = vec![root];
    let mut shipped = BTreeSet::new();
    while let Some(id) = stack.pop() {
        let node = nodes
            .iter()
            .find(|n| field(n, "id").as_str() == Some(id))
            .ok_or_else(|| bad(&format!("no resolve node for `{id}`")))?;
        for dep in field(node, "deps").as_array().into_iter().flatten() {
            let is_normal = field(dep, "dep_kinds")
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| field(k, "kind").is_null());
            let Some(dep_id) = field(dep, "pkg").as_str() else {
                return Err(bad("a dependency without `pkg`"));
            };
            if !is_normal || !seen.insert(dep_id) {
                continue;
            }
            stack.push(dep_id);
            let pkg = find_package(dep_id).ok_or_else(|| bad(&format!("no package `{dep_id}`")))?;
            if field(pkg, "source").is_null() {
                continue; // a workspace member: DreamCarve's own code
            }
            let (Some(name), Some(version)) =
                (field(pkg, "name").as_str(), field(pkg, "version").as_str())
            else {
                return Err(bad(&format!("package `{dep_id}` has no name or version")));
            };
            shipped.insert(CrateId::new(name, version));
        }
    }
    Ok(shipped)
}

/// The crates a generated file lists under a non-empty license text.
///
/// The file is a sequence of sections separated by [`SECTION_RULE`] lines.
/// Each section has a `License: ...` line, a `Used by:` line, one
/// `- NAME VERSION [(URL)]` line per crate, a blank line, then the text.
pub fn licensed_crates(licenses_file: &str) -> BTreeSet<CrateId> {
    let mut licensed = BTreeSet::new();
    for section in licenses_file.split(SECTION_RULE).skip(1) {
        let mut lines = section.lines().skip_while(|l| l.trim().is_empty());
        if !lines.next().is_some_and(|l| l.starts_with("License: ")) {
            continue;
        }
        if lines.next() != Some("Used by:") {
            continue;
        }
        let mut crates = Vec::new();
        let mut text = String::new();
        let mut in_list = true;
        for line in lines {
            if in_list {
                if let Some(entry) = line.strip_prefix("- ") {
                    let mut words = entry.split_whitespace();
                    if let (Some(name), Some(version)) = (words.next(), words.next()) {
                        crates.push(CrateId::new(name, version));
                    }
                    continue;
                }
                in_list = false;
            }
            text.push_str(line);
        }
        if !text.trim().is_empty() {
            licensed.extend(crates);
        }
    }
    licensed
}

/// Shipped crates missing from `licensed`.
pub fn uncovered(shipped: &BTreeSet<CrateId>, licensed: &BTreeSet<CrateId>) -> Vec<CrateId> {
    shipped.difference(licensed).cloned().collect()
}

/// Which of [`ARCHIVE_FILES`] are not at the root of an archive, given its
/// entry paths. dist puts tar archives under one top-level directory and may
/// write zip archives flat, so a root file has one or two path components.
pub fn missing_archive_files(entries: &[String]) -> Vec<&'static str> {
    let at_root: BTreeSet<&str> = entries
        .iter()
        .filter_map(|entry| {
            let parts: Vec<&str> = entry
                .split(['/', '\\'])
                .filter(|p| !p.is_empty() && *p != ".")
                .collect();
            match parts.as_slice() {
                [name] | [_, name] => Some(*name),
                _ => None,
            }
        })
        .collect();
    ARCHIVE_FILES
        .into_iter()
        .filter(|f| !at_root.contains(f))
        .collect()
}

/// Writes `output` with `cargo about generate`, for the dist targets.
/// Fails when a shipped crate's license cannot be resolved to a text.
pub fn generate(root: &Path, output: &Path) -> Result<(), Error> {
    let targets = dist_targets(&read(&root.join("dist-workspace.toml"))?)?;
    let mut cmd = Command::new(cargo());
    cmd.current_dir(root)
        .args(["about", "generate", "--locked", "--fail"])
        .arg("--config")
        .arg(root.join("about.toml"))
        .arg("--manifest-path")
        .arg(root.join("crates").join(SHIPPED_PACKAGE).join("Cargo.toml"))
        .arg("--output-file")
        .arg(output)
        .arg(root.join("about.hbs"));
    for target in &targets {
        cmd.args(["--target", target]);
    }
    run(&mut cmd).map(drop)
}

/// The third-party crates linked into the `dreamcarve` binary on any dist
/// target, from `cargo metadata --filter-platform`.
pub fn shipped_crates_for_release(root: &Path) -> Result<BTreeSet<CrateId>, Error> {
    let targets = dist_targets(&read(&root.join("dist-workspace.toml"))?)?;
    let mut shipped = BTreeSet::new();
    for target in targets {
        let mut cmd = Command::new(cargo());
        cmd.current_dir(root)
            .args(["metadata", "--format-version", "1", "--locked"])
            .args(["--filter-platform", &target])
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml"));
        shipped.extend(shipped_crates(&run(&mut cmd)?, SHIPPED_PACKAGE)?);
    }
    Ok(shipped)
}

/// Checks `licenses_file` against the crates `cargo metadata` resolves into
/// the shipped binary on any dist target.
pub fn check(root: &Path, licenses_file: &Path) -> Result<Coverage, Error> {
    let shipped = shipped_crates_for_release(root)?;
    let licensed = licensed_crates(&read(licenses_file)?);
    let uncovered = uncovered(&shipped, &licensed);
    Ok(Coverage { shipped, uncovered })
}

/// The host target triple, from `rustc -vV`.
pub fn host_target() -> Result<String, Error> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let out = run(Command::new(rustc).arg("-vV"))?;
    out.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|h| h.trim().to_owned())
        .ok_or_else(|| Error::Format("`rustc -vV` printed no `host:` line".into()))
}

/// Runs `dist build` for `target` and returns the archive it wrote.
pub fn dist_build_archive(root: &Path, target: &str) -> Result<PathBuf, Error> {
    let mut cmd = Command::new("dist");
    cmd.current_dir(root)
        .args(["build", "--artifacts=local", "--output-format=json"])
        .args(["--target", target]);
    run(&mut cmd)?;
    let distrib = root.join("target").join("distrib");
    [".tar.xz", ".tar.gz", ".zip"]
        .iter()
        .map(|ext| distrib.join(format!("{SHIPPED_PACKAGE}-{target}{ext}")))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            Error::Format(format!(
                "dist build wrote no {SHIPPED_PACKAGE}-{target} archive in {}",
                distrib.display()
            ))
        })
}

/// Lists an archive's entries with `tar -tf` (bsdtar also reads zip).
pub fn archive_entries(archive: &Path) -> Result<Vec<String>, Error> {
    let out = run(Command::new("tar").arg("-tf").arg(archive))?;
    Ok(out.lines().map(str::to_owned).collect())
}

/// `value[key]` without indexing: `Null` when absent.
fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })
}

/// Runs `cmd`, returning its standard output when it succeeds.
fn run(cmd: &mut Command) -> Result<String, Error> {
    let command = format!("{cmd:?}");
    let out = cmd.output().map_err(|source| Error::Spawn {
        command: command.clone(),
        source,
    })?;
    if !out.status.success() {
        return Err(Error::Command {
            command,
            status: out.status.to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    String::from_utf8(out.stdout)
        .map_err(|_| Error::Format(format!("`{command}` printed non-UTF-8")))
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
                &[
                    "strsim 0.11.1 (https://github.com/rapidfuzz/strsim-rs)",
                    "a 1.0.0"
                ],
                "Permission is hereby granted...\n- not a crate line\n"
            ),
            section(
                "Apache License 2.0 (Apache-2.0)",
                &["b 2.1.0"],
                "TERMS AND CONDITIONS"
            ),
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
        let entries: Vec<String> = [
            "dreamcarve.exe",
            "LICENSE",
            "NOTICE",
            "THIRD-PARTY-LICENSES",
        ]
        .map(String::from)
        .to_vec();
        assert!(
            missing_archive_files(&entries).is_empty(),
            "zip archives may have no top-level directory"
        );
    }
}
