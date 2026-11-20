// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The book as a whole (#550; `docs/specs/phase-4/functional.md`,
//! "Documentation deliverables" and exit criteria): it builds, and every
//! picture a chapter shows exists.
//!
//! `mdbook build docs/book` is a CI step (`.github/workflows/ci.yml`),
//! and mdBook is not a build dependency, so a machine without it could
//! break the book and not know. This test checks what `mdbook build`
//! needs with no mdBook: every chapter `SUMMARY.md` names exists, and
//! every chapter on disk is in the summary (mdBook creates a missing
//! chapter as an empty file and leaves an unlisted one out, both
//! silently). Then it checks what `mdbook build` does not: every image a
//! chapter shows is a file, with alt text, and every screenshot of the
//! app in a `figures/app/` folder is shown by some chapter, so none is
//! left behind when a chapter changes. When `mdbook` is on the `PATH`
//! it also runs the build itself, into a scratch directory.
//!
//! The links between chapters are `quick_start::book_links_resolve`'s.
//! The screenshots are written by `dreamcarve-app`'s `tests/book.rs`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The book's sources.
fn book_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/book/src")
}

/// The files under `dir` with extension `ext`, sorted.
fn files_with(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("book directory reads") {
            let path = entry.expect("book entry reads").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == ext) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `path` without `.` and `..` components, so two spellings of one file
/// compare equal.
fn tidy(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The targets of `[text](target)` in `line`, each with whether it is an
/// image (`![alt](target)`) and its text. Code spans are skipped: they
/// hold brackets that are not links.
fn links(line: &str) -> Vec<(bool, String, String)> {
    let prose: Vec<&str> = line.split('`').step_by(2).collect();
    let prose = prose.join(" ");
    let mut found = Vec::new();
    let mut rest = prose.as_str();
    while let Some(at) = rest.find("](") {
        let before = &rest[..at];
        let open = before.rfind('[').unwrap_or(0);
        let image = before[..open].ends_with('!');
        let text = before[open..].trim_start_matches('[').to_owned();
        let after = &rest[at + 2..];
        let end = after.find(')').unwrap_or(after.len());
        let target = after[..end].split_whitespace().next().unwrap_or("");
        found.push((image, text, target.to_owned()));
        rest = &after[end..];
    }
    found
}

/// The lines of a chapter outside fenced blocks, numbered from 1.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    let mut lines = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            lines.push((n + 1, line));
        }
    }
    lines
}

/// Whether `path` starts as a PNG file does.
fn is_png(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(&[0x89, b'P', b'N', b'G']))
}

/// Exit criterion "Quick start … and every chapter above published, with
/// screenshots from the shipped build" (#550): the book builds, and every
/// screenshot a chapter shows exists.
///
/// - Every chapter `SUMMARY.md` lists is a file, and every chapter under
///   `docs/book/src` is listed.
/// - Every image a chapter shows is a file under the book's sources, and
///   has alt text, which is what a reader who cannot see the picture
///   gets.
/// - Every `.png` under a `figures/app/` folder (the screenshots
///   `dreamcarve-app`'s `tests/book.rs` writes) is a PNG and is shown by
///   a chapter.
/// - The quick start "From SVG to G-code in the app" shows a screenshot
///   in each of its numbered steps.
/// - With `mdbook` installed, `mdbook build docs/book` succeeds.
#[test]
fn book_builds_and_every_screenshot_exists() {
    let src = book_src();
    let summary = std::fs::read_to_string(src.join("SUMMARY.md")).expect("SUMMARY.md reads");
    let mut problems = Vec::new();

    // The summary and the chapters on disk name the same files.
    let listed: BTreeSet<PathBuf> = prose_lines(&summary)
        .into_iter()
        .flat_map(|(_, line)| links(line))
        .map(|(_, _, target)| tidy(&src.join(target)))
        .collect();
    assert!(listed.len() > 30, "the summary's chapters are found");
    for chapter in &listed {
        if !chapter.is_file() {
            problems.push(format!(
                "SUMMARY.md lists {}, which does not exist",
                chapter.display()
            ));
        }
    }
    let chapters = files_with(&src, "md");
    for chapter in &chapters {
        let on_disk = tidy(chapter);
        if on_disk.file_name().is_some_and(|n| n == "SUMMARY.md") {
            continue;
        }
        if !listed.contains(&on_disk) {
            problems.push(format!("{} is not in SUMMARY.md", chapter.display()));
        }
    }

    // Every image shown exists and has alt text.
    let mut shown = BTreeSet::new();
    for chapter in &chapters {
        let text = std::fs::read_to_string(chapter).expect("chapter reads");
        let name = chapter.strip_prefix(&src).unwrap_or(chapter).display();
        for (n, line) in prose_lines(&text) {
            for (image, alt, target) in links(line) {
                if !image || target.contains("://") {
                    continue;
                }
                let path = tidy(&chapter.parent().unwrap_or(&src).join(&target));
                if !path.is_file() {
                    problems.push(format!("{name}:{n}: the image {target} does not exist"));
                }
                if alt.trim().is_empty() {
                    problems.push(format!("{name}:{n}: the image {target} has no alt text"));
                }
                shown.insert(path);
            }
        }
    }

    // Every screenshot of the app is a PNG some chapter shows.
    let screenshots: Vec<PathBuf> = files_with(&src, "png")
        .into_iter()
        .filter(|p| p.parent().is_some_and(|d| d.ends_with("figures/app")))
        .collect();
    assert!(
        screenshots.len() >= 10,
        "the app's screenshots are found ({})",
        screenshots.len()
    );
    for screenshot in &screenshots {
        if !is_png(screenshot) {
            problems.push(format!("{} is not a PNG", screenshot.display()));
        }
        if !shown.contains(&tidy(screenshot)) {
            problems.push(format!(
                "{} is shown by no chapter; delete it, or show it",
                screenshot.display()
            ));
        }
    }

    // The app quick start shows the build at every numbered step.
    let quick_start =
        std::fs::read_to_string(src.join("quick-start/app.md")).expect("the app quick start");
    let mut step: Option<(String, bool)> = None;
    let mut steps = 0;
    for (_, line) in prose_lines(&quick_start).into_iter().chain([(0, "## end")]) {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some((title, false)) = &step {
                problems.push(format!(
                    "quick-start/app.md: step `{title}` shows no screenshot"
                ));
            }
            let numbered = heading.chars().next().is_some_and(|c| c.is_ascii_digit());
            step = numbered.then(|| (heading.to_owned(), false));
            steps += usize::from(numbered);
        } else if let Some((_, has)) = &mut step {
            *has |= links(line)
                .iter()
                .any(|(image, _, target)| *image && target.contains("figures/app/"));
        }
    }
    assert!(steps >= 6, "the app quick start's numbered steps are found");

    assert!(
        problems.is_empty(),
        "the book has problems:\n{}",
        problems.join("\n")
    );

    // The build itself, where mdBook is installed (CI builds the book in
    // its own job).
    let installed = Command::new("mdbook")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if !installed {
        println!("mdbook is not installed, so `mdbook build` was not run");
        return;
    }
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("docs-book");
    let book = src.parent().expect("the book's directory");
    let out = Command::new("mdbook")
        .arg("build")
        .arg(book)
        .arg("--dest-dir")
        .arg(&out_dir)
        .output()
        .expect("mdbook runs");
    assert!(
        out.status.success(),
        "mdbook build failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out_dir.join("quick-start/app.html").is_file(),
        "the built book has the app quick start"
    );
}
