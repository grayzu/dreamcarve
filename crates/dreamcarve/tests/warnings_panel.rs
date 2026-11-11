// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 82 against the binary (phase 4 design, "Warnings" and "Test
//! harness"; #538): the app's Warnings panel, copied, is what
//! `dreamcarve check` prints for the same file: the migration notes, the
//! errors located at their `file:line:column`, the warnings and hints,
//! and the count line, in `check`'s order. `dc-app`'s own tests compare
//! the warning lines with the CLI's committed expectations; these run the
//! binary, so errors and notes are compared too. Each project is read
//! from where it lives and copied into a scratch folder, where `check`
//! runs on it by its file name, as a user in that folder would.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code reports failure by panicking"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use dc_app::test_support::Harness;

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

/// `dreamcarve args` in `dir` with no user config: its exit code and
/// standard output.
fn run(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
        .current_dir(dir)
        .env("DREAMCARVE_CONFIG_DIR", dir.join("config"))
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.code(),
        String::from_utf8(out.stdout).expect("stdout is text"),
    )
}

/// The lines of `check`'s output for `file` that the panel lists: its
/// notes, errors and warnings (located or not), its carve notes (slice
/// 28, #620: `V-Carve …` and `Inlay …`) and its count line, not its
/// extents and estimates.
fn panel_lines(stdout: &str, file: &str) -> Vec<String> {
    let located = |line: &str| {
        line.strip_prefix(&format!("{file}:")).is_some_and(|rest| {
            let mut parts = rest.splitn(3, ':');
            let (Some(l), Some(c), Some(tail)) = (parts.next(), parts.next(), parts.next()) else {
                return false;
            };
            l.chars().all(|ch| ch.is_ascii_digit())
                && c.chars().all(|ch| ch.is_ascii_digit())
                && (tail.starts_with(" error: ") || tail.starts_with(" warning: "))
        })
    };
    stdout
        .lines()
        .filter(|line| {
            line.starts_with(&format!("{file}: note: "))
                || line.starts_with("error: ")
                || line.starts_with("warning: ")
                || located(line)
                || line.starts_with("V-Carve ")
                || line.starts_with("Inlay ")
                || (line.starts_with(&format!("{file}: ")) && line.contains(" error(s), "))
        })
        .map(str::to_owned)
        .collect()
}

/// Acceptance 82: for the coaster as `init` writes it (every unset value
/// an error, located in the file), the phase 2 and 3 quick starts, a
/// two-file Inlay's male that the format 25 migration notes, with its
/// carve note (slice 28), the one-file Inlay, the Pocket
/// and Bore project with its hints, and two golden cases, the app's
/// *Copy* is `check`'s text line for line, and the app counts as `check`
/// exits: 1 with an error.
#[test]
fn copy_equals_check_on_the_binary() {
    let dir = work_dir("copy_equals_check_on_the_binary");
    let root = repo_root();
    std::fs::copy(root.join("fixtures/coaster.svg"), dir.join("coaster.svg")).unwrap();
    let (code, _) = run(&dir, &["init", "coaster.svg"]);
    assert_eq!(code, Some(0), "init writes the coaster");

    let mut files = vec!["coaster.dcv".to_owned()];
    let copies: [(PathBuf, &str); 7] = [
        (root.join("tests/fixtures/plaque.dcv"), "plaque.dcv"),
        (root.join("tests/fixtures/sign.dcv"), "sign.dcv"),
        (root.join("tests/fixtures/dc.dcv"), "dc.dcv"),
        (
            root.join("crates/dreamcarve/tests/fixtures/inlay-migration/star-male.dcv"),
            "star-male.dcv",
        ),
        (
            root.join("crates/dreamcarve/tests/fixtures/stock-to-leave-migration/pocket-bore.dcv"),
            "pocket-bore.dcv",
        ),
        (
            root.join("tests/golden/pocket-rough-then-rest/dreamcarve.dcv"),
            "pocket-rough-then-rest.dcv",
        ),
        (
            root.join("tests/golden/drill-peck/dreamcarve.dcv"),
            "drill-peck.dcv",
        ),
    ];
    for (from, name) in &copies {
        std::fs::copy(from, dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        files.push((*name).to_owned());
    }

    let (mut errors, mut notes, mut carves) = (0, 0, 0);
    for file in &files {
        let (code, stdout) = run(&dir, &["check", file]);
        let expected = panel_lines(&stdout, file);
        let bytes = std::fs::read(dir.join(file)).unwrap();
        let mut harness = Harness::from_project(dc_project::test_support::sample_project());
        harness
            .app
            .open(&bytes, Some(PathBuf::from(file)))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        harness.generate();
        let copied: Vec<String> = dc_app::warnings::copy_text(&harness.app)
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(copied, expected, "{file}: the panel's text is not check's");
        let counts = dc_app::warnings::counts(&harness.app);
        assert_eq!(
            code,
            Some(i32::from(counts.errors > 0)),
            "{file}: check's exit"
        );
        carves += copied
            .iter()
            .filter(|l| l.starts_with("V-Carve ") || l.starts_with("Inlay "))
            .count();
        errors += counts.errors;
        notes += counts.notes;
    }
    assert!(errors > 0, "the coaster's unset values are errors");
    assert!(notes > 0, "the male's migration is noted");
    assert!(
        carves >= 4,
        "the sign's and the Inlays' carve notes are copied"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
