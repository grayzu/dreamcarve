// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Supply-chain checks on `.github/` (issue #45): every action is pinned to a
//! commit SHA, and Dependabot watches the pins. They live beside the license
//! header check because both are repository-hygiene checks over checked-in
//! files that `cargo test --all` runs on every PR.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `*.yml` / `*.yaml` file under `dir`, sorted for stable messages.
fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", current.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The value of a `uses:` key on `line`, without quotes or a trailing
/// comment, or `None` when the line has no `uses:` key.
fn uses_value(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let rest = trimmed.strip_prefix("uses:")?;
    let value = rest.split(" #").next().unwrap_or(rest).trim();
    Some(value.trim_matches(|c| c == '"' || c == '\''))
}

/// A `uses:` value is pinned when it is a local path (`./…`) or
/// `owner/repo[/path]@<40 lowercase hex chars>`.
fn is_pinned(value: &str) -> bool {
    if value.starts_with("./") {
        return true;
    }
    let Some((action, reference)) = value.split_once('@') else {
        return false;
    };
    let segments: Vec<&str> = action.split('/').collect();
    segments.len() >= 2
        && segments.iter().all(|s| !s.is_empty())
        && !action.contains(':')
        && reference.len() == 40
        && reference
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

#[test]
fn pin_check_accepts_sha_and_local_and_rejects_tags() {
    let sha = "11d5960a326750d5838078e36cf38b85af677262";
    assert!(is_pinned(&format!("actions/checkout@{sha}")));
    assert!(is_pinned(&format!("github/codeql-action/init@{sha}")));
    assert!(is_pinned("./.github/actions/setup"));
    assert!(!is_pinned("actions/checkout@v4"));
    assert!(!is_pinned("dtolnay/rust-toolchain@stable"));
    assert!(!is_pinned(&format!("actions/checkout@{}", &sha[..39])));
    assert!(!is_pinned(&format!(
        "actions/checkout@{}",
        sha.to_uppercase()
    )));
    assert!(!is_pinned("docker://alpine:3.20"));
    assert!(!is_pinned(&format!("checkout@{sha}")));
}

#[test]
fn uses_value_strips_quotes_dash_and_comment() {
    assert_eq!(
        uses_value("      - uses: actions/checkout@abc # v4.4.0"),
        Some("actions/checkout@abc")
    );
    assert_eq!(
        uses_value("        uses: \"taiki-e/install-action@v2\""),
        Some("taiki-e/install-action@v2")
    );
    assert_eq!(uses_value("        run: cargo test"), None);
}

/// Issue #45: a moving tag lets whoever controls it change what runs, so
/// every `uses:` in `.github/` names a full commit SHA or a local action.
#[test]
fn workflow_actions_are_pinned() {
    let github = repo_root().join(".github");
    let files = yaml_files(&github);
    assert!(
        !files.is_empty(),
        "no YAML files under {}",
        github.display()
    );

    let mut unpinned = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        for (index, line) in text.lines().enumerate() {
            if let Some(value) = uses_value(line)
                && !is_pinned(value)
            {
                let shown = file.strip_prefix(repo_root()).unwrap_or(file);
                unpinned.push(format!("{}:{}: {value}", shown.display(), index + 1));
            }
        }
    }
    assert!(
        unpinned.is_empty(),
        "actions not pinned to a 40-char commit SHA:\n  {}",
        unpinned.join("\n  ")
    );
}

/// Issue #45: pinned SHAs only get updates when Dependabot watches the
/// `github-actions` ecosystem.
#[test]
fn dependabot_watches_actions() {
    let path = repo_root().join(".github/dependabot.yml");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let watches = text.lines().any(|line| {
        let trimmed = line.trim_start();
        let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        trimmed
            .strip_prefix("package-ecosystem:")
            .map(|v| v.split(" #").next().unwrap_or(v).trim())
            .map(|v| v.trim_matches(|c| c == '"' || c == '\''))
            == Some("github-actions")
    });
    assert!(
        watches,
        "{} does not list package-ecosystem: \"github-actions\"",
        path.display()
    );
}
