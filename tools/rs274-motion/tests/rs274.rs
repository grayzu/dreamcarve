// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Issue #299: DreamCarve's reading of its LinuxCNC, Mach3, UCCNC and
//! generic programs is the motion LinuxCNC's `rs274` makes from them.
//!
//! The tests that need `rs274` skip, saying so on standard error, when
//! there is none: no `DREAMCARVE_RS274` and no `rs274` on `PATH`. To run
//! them, install `linuxcnc-uspace`, or build the image beside this crate
//! and point the variable at it (`tools/rs274-motion/README.md`):
//!
//! ```sh
//! podman build -t dreamcarve-rs274 tools/rs274-motion
//! DREAMCARVE_RS274=podman:dreamcarve-rs274 cargo test -p rs274-motion -- --nocapture
//! ```

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use rs274_motion::bridge::bridge;
use rs274_motion::canon::parse_canon;
use rs274_motion::compare::compare;
use rs274_motion::corpus::{Case, build_corpus};
use rs274_motion::reference::reference;
use rs274_motion::runner::{ENV, Rs274};
use rs274_motion::{Verdict, check};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> Vec<Case> {
    build_corpus(&root()).expect("the corpus builds")
}

/// The `rs274` to run, or `None` after saying why the test is skipped.
fn rs274_or_skip(test: &str) -> Option<Rs274> {
    let found = Rs274::detect();
    if found.is_none() {
        eprintln!(
            "{test}: skipped: no rs274. Install linuxcnc-uspace, or set {ENV}=podman:<image> \
             after `podman build -t dreamcarve-rs274 tools/rs274-motion` \
             (tools/rs274-motion/README.md)."
        );
    }
    found
}

fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

fn code_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().map(|l| {
        l.split(['(', ';'])
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
    })
}

fn has_word(case: &Case, word: &str) -> bool {
    code_lines(&case.text).any(|l| l.split_whitespace().any(|w| w == word))
}

/// The corpus is not vacuous: it has every controller, and programs with
/// each feature the comparison is there to check. If an export change
/// drops one (canned cycles stop being written, say), this fails rather
/// than the comparison quietly checking less.
#[test]
fn corpus_covers_each_controller_and_feature() {
    let cases = corpus();
    for id in ["linuxcnc", "generic", "mach3", "uccnc"] {
        assert!(
            cases.iter().any(|c| c.post.id == id),
            "no {id} program in the corpus"
        );
    }
    for (word, what) in [
        ("G81", "a G81 drill cycle"),
        ("G82", "a G82 cycle with dwell"),
        ("G98", "G98 retract"),
        ("G53", "a G53 machine-Z lift"),
        ("G20", "an inch program"),
        ("G2", "a clockwise arc"),
        ("G4", "a dwell"),
        ("G50", "a Mach3 or UCCNC header"),
        ("M0", "an M0 pause (manual spindle control)"),
    ] {
        assert!(
            cases.iter().any(|c| has_word(c, word)),
            "no program in the corpus has {what} ({word})"
        );
    }
    assert!(
        cases
            .iter()
            .any(|c| c.post.id == "uccnc" && c.text.contains("G4 P3000")),
        "no UCCNC program with a millisecond dwell"
    );
}

/// Every corpus program runs through `rs274` and its motion, line by
/// line, is DreamCarve's: move kinds, end points, arc centres, feeds,
/// dwells, canned cycles expanded, and `G53` lifts at machine Z under a
/// work offset.
#[test]
fn corpus_motion_matches_rs274() {
    let Some(rs274) = rs274_or_skip("corpus_motion_matches_rs274") else {
        return;
    };
    let dir = work_dir("corpus");
    let mut failures = Vec::new();
    for case in corpus() {
        let outcome = check(&case.text, &case.post, case.units, &rs274, &dir);
        match outcome.verdict {
            Verdict::Compared(c) if c.differences.is_empty() => {}
            Verdict::Compared(c) => failures.push(format!("{}: {:#?}", case.name, c.differences)),
            Verdict::Rejected(why) => {
                failures.push(format!("{}: rs274 rejected it: {why}", case.name))
            }
            Verdict::Failed(why) => failures.push(format!("{}: {why}", case.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The comparison can fail: `rs274` given the LinuxCNC golden with one
/// cut 0.1 mm deeper is reported from that line on (Z is modal, so the
/// moves after it are deeper too), against DreamCarve's reading of the
/// original. Without this, a comparison that compared nothing would pass.
#[test]
fn a_cut_rs274_makes_deeper_is_reported() {
    let Some(rs274) = rs274_or_skip("a_cut_rs274_makes_deeper_is_reported") else {
        return;
    };
    let case = corpus()
        .into_iter()
        .find(|c| c.name.ends_with("post-linuxcnc-coaster/program.ngc"))
        .expect("the LinuxCNC golden is in the corpus");
    let line = case
        .text
        .lines()
        .position(|l| l.contains("Z-4.000"))
        .expect("the golden cuts to Z-4")
        + 1;
    let bridged = bridge(&case.text, &case.post, case.units);
    let mutated: String = bridged
        .text
        .lines()
        .enumerate()
        .map(|(k, l)| {
            let l = if k + 1 == line {
                l.replace("Z-4.000", "Z-4.100")
            } else {
                l.to_owned()
            };
            l + "\n"
        })
        .collect();
    let run = rs274
        .run(&mutated, &work_dir("mutated"))
        .expect("rs274 runs");
    assert_eq!(run.rejection(), None, "{}", run.stderr);
    let ours = reference(&case.text, &case.post, case.units).expect("DreamCarve reads it");
    let theirs = parse_canon(&run.stdout).expect("rs274's output reads");
    let c = compare(&ours, &theirs);
    assert!(!c.differences.is_empty(), "the deeper cut was not caught");
    assert_eq!(c.differences[0].line, Some(line), "{:#?}", c.differences);
    assert!(
        c.differences[0].what.contains("Z -4 vs -4.1"),
        "{}",
        c.differences[0].what
    );
}
