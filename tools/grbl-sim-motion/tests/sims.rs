// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Issue #329: grblHAL and GRBL run DreamCarve's grblHAL and GRBL programs
//! as DreamCarve reads them.
//!
//! The tests that need the simulators skip, saying so on standard error,
//! when there are none: no `DREAMCARVE_GRBL_SIM` and no `grblHAL_sim` on
//! `PATH`. To run them, build the image beside this crate and point the
//! variable at it (`tools/grbl-sim-motion/README.md`):
//!
//! ```sh
//! podman build -t dreamcarve-grbl-sim tools/grbl-sim-motion
//! DREAMCARVE_GRBL_SIM=podman:dreamcarve-grbl-sim cargo test -p grbl-sim-motion -- --nocapture
//! ```
//!
//! Each of them first runs the probe ([`grbl_sim_motion::probe`]): if a
//! program with a known result does not come out right, the setup is
//! wrong, and the test fails as misconfigured rather than blaming a
//! program.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use grbl_sim_motion::compare::{compare_lines, targets};
use grbl_sim_motion::corpus::{Case, build_corpus};
use grbl_sim_motion::runner::{ENV, Firmware, Line, Reply, Sims, Sync};
use grbl_sim_motion::{
    SET_WORK_OFFSET, Verdict, check_mode, check_motion, header_lines, modal_state_after, probe,
    program_lines, validate,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> Vec<Case> {
    build_corpus(&root()).expect("the corpus builds")
}

fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

/// The simulators, checked with the probe, or `None` after saying why the
/// test is skipped.
fn sims_or_skip(test: &str, firmware: Firmware) -> Option<(Sims, PathBuf)> {
    let Some(sims) = Sims::detect() else {
        eprintln!(
            "{test}: skipped: no grblHAL Simulator or grbl-sim. Set {ENV}=podman:<image> after \
             `podman build -t dreamcarve-grbl-sim tools/grbl-sim-motion` \
             (tools/grbl-sim-motion/README.md)."
        );
        return None;
    };
    let dir = work_dir(test);
    if let Err(why) = probe(firmware, &sims, &dir) {
        panic!("{test}: {why}");
    }
    Some((sims, dir))
}

fn code_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().flat_map(|l| {
        l.split(['(', ';'])
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    })
}

fn has_word(case: &Case, word: &str) -> bool {
    code_words(&case.text).any(|w| w == word)
}

fn of(firmware: Firmware) -> Vec<Case> {
    corpus()
        .into_iter()
        .filter(|c| c.firmware == firmware)
        .collect()
}

/// The corpus is not vacuous: it has both controllers and programs with
/// each feature the simulators are there to check, and no grblHAL or GRBL
/// program has a canned cycle. If an export change drops a feature, or
/// starts writing cycles, this fails rather than the simulators quietly
/// checking less, or GRBL refusing a cycle on a user's machine.
#[test]
fn corpus_covers_each_controller_and_feature() {
    let cases = corpus();
    for firmware in [Firmware::GrblHal, Firmware::Grbl] {
        let mine: Vec<&Case> = cases.iter().filter(|c| c.firmware == firmware).collect();
        assert!(!mine.is_empty(), "no {firmware} program in the corpus");
        for (word, what) in [
            ("G53", "a G53 machine-Z lift"),
            ("G20", "an inch program"),
            ("G2", "a clockwise arc"),
            ("G4", "a dwell"),
            ("M0", "an M0 pause"),
            ("G80", "a header that cancels a motion mode"),
            ("G40", "a header that cancels cutter compensation"),
        ] {
            assert!(
                mine.iter().any(|c| has_word(c, word)),
                "no {firmware} program in the corpus has {what} ({word})"
            );
        }
    }
    for (word, what) in [
        ("G50", "a grblHAL v4 header that cancels scaling"),
        ("M6", "an M6 tool change"),
    ] {
        assert!(
            cases
                .iter()
                .any(|c| c.firmware == Firmware::GrblHal && has_word(c, word)),
            "no grblHAL program in the corpus has {what} ({word})"
        );
    }
    for case in &cases {
        for cycle in ["G73", "G81", "G82", "G83", "G85", "G86", "G89", "G98", "G99"] {
            assert!(
                !has_word(case, cycle),
                "{}: {cycle}: GRBL has no canned cycles, and no grblHAL or GRBL definition \
                 documents them",
                case.name
            );
        }
    }
}

/// Every GRBL program in the corpus passes grbl-sim's validator, GRBL
/// 1.1h's own parser.
#[test]
fn grbl_validator_accepts_the_corpus() {
    let Some((sims, dir)) = sims_or_skip("grbl_validator_accepts_the_corpus", Firmware::Grbl)
    else {
        return;
    };
    let failures: Vec<String> = of(Firmware::Grbl)
        .iter()
        .filter_map(|case| {
            match validate(&case.text, Firmware::Grbl, &sims, &dir).expect("the validator runs") {
                None => None,
                Some(why) => Some(format!("{}: {why}", case.name)),
            }
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The validator can fail: an arc whose end is 1 mm off its circle is
/// rejected (error 33, invalid target).
#[test]
fn grbl_validator_rejects_a_bad_arc() {
    let Some((sims, dir)) = sims_or_skip("grbl_validator_rejects_a_bad_arc", Firmware::Grbl)
    else {
        return;
    };
    let why = validate(
        "G21 G90 G17\nG0 X0 Y0\nG2 X11 Y0 I5 J0 F600\nM2\n",
        Firmware::Grbl,
        &sims,
        &dir,
    )
    .expect("the validator runs");
    assert!(why.as_ref().is_some_and(|w| w.contains("error:33")), "{why:?}");
}

/// GRBL refuses a canned cycle (error 20, unsupported command), which is
/// why no GRBL definition writes one.
#[test]
fn grbl_rejects_canned_cycles() {
    let Some((sims, dir)) = sims_or_skip("grbl_rejects_canned_cycles", Firmware::Grbl) else {
        return;
    };
    let why = validate(
        "G21 G90\nG0 Z5\nG98 G81 X1 Y1 Z-1 R1 F100\nM2\n",
        Firmware::Grbl,
        &sims,
        &dir,
    )
    .expect("the validator runs");
    assert!(why.as_ref().is_some_and(|w| w.contains("error:20")), "{why:?}");
}

fn assert_check_mode_accepts(firmware: Firmware, test: &str) {
    let Some((sims, dir)) = sims_or_skip(test, firmware) else {
        return;
    };
    let failures: Vec<String> = of(firmware)
        .iter()
        .filter_map(|case| {
            let problems = check_mode(case, &sims, &dir).expect("the simulator runs");
            (!problems.is_empty()).then(|| format!("{}: {}", case.name, problems.join("; ")))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every grblHAL program in the corpus streams through `$C` check mode
/// with every line `ok` and nothing moving. This is layer 1 for grblHAL:
/// the issue asked for `grblHAL_validator`, which crashes on every input
/// at the pinned commit (README, "What runs"), and check mode runs the
/// same parser on the full firmware.
#[test]
fn grblhal_check_mode_accepts_the_corpus() {
    assert_check_mode_accepts(Firmware::GrblHal, "grblhal_check_mode_accepts_the_corpus");
}

/// As for grblHAL, on GRBL.
#[test]
fn grbl_check_mode_accepts_the_corpus() {
    assert_check_mode_accepts(Firmware::Grbl, "grbl_check_mode_accepts_the_corpus");
}

/// Check mode can fail: an arc whose end is 1 mm off its circle gets
/// error 33 on grblHAL too.
#[test]
fn grblhal_check_mode_rejects_a_bad_arc() {
    let Some((sims, dir)) =
        sims_or_skip("grblhal_check_mode_rejects_a_bad_arc", Firmware::GrblHal)
    else {
        return;
    };
    let lines: Vec<Line> = ["$C", "G21 G90 G17", "G0 X0 Y0", "G2 X11 Y0 I5 J0 F600"]
        .iter()
        .map(|l| Line::harness(l))
        .collect();
    let session = sims
        .stream(Firmware::GrblHal, &lines, Sync::Streamed, &dir)
        .expect("the simulator runs");
    let arc = session.exchanges.last().expect("the arc was sent");
    assert_eq!(arc.reply, Reply::Error(33), "{session:#?}");
}

fn assert_motion_matches(firmware: Firmware, test: &str) {
    let Some((sims, dir)) = sims_or_skip(test, firmware) else {
        return;
    };
    let mut failures = Vec::new();
    for case in of(firmware) {
        match check_motion(&case, &sims, &dir) {
            Verdict::Compared(c) if c.differences.is_empty() => {
                eprintln!("{test}: {}: {} moves, {} checked", case.name, c.moves, c.checked);
            }
            Verdict::Compared(c) => failures.push(format!("{}: {:#?}", case.name, c.differences)),
            Verdict::Rejected(why) => {
                failures.push(format!("{}: the firmware rejected it: {why}", case.name));
            }
            Verdict::Failed(why) => failures.push(format!("{}: {why}", case.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Every grblHAL program runs through the grblHAL Simulator. After each
/// line, the machine position grblHAL reports is where DreamCarve reads
/// the line ending, within 5e-4 mm, and its motion mode and feed are
/// DreamCarve's; a `G53` lift lands at machine Z under a work offset. Each
/// `M0` and `M6` pauses once, and the run ends where DreamCarve reads it
/// ending.
#[test]
fn grblhal_sim_motion_matches_dreamcarve() {
    assert_motion_matches(Firmware::GrblHal, "grblhal_sim_motion_matches_dreamcarve");
}

/// As for grblHAL, every GRBL program through grbl-sim.
#[test]
fn grbl_sim_motion_matches_dreamcarve() {
    assert_motion_matches(Firmware::Grbl, "grbl_sim_motion_matches_dreamcarve");
}

/// The comparison can fail: the grblHAL coaster with one cut 0.1 mm
/// deeper is reported on that line, against DreamCarve's reading of the
/// original.
#[test]
fn a_cut_the_simulator_makes_deeper_is_reported() {
    let Some((sims, dir)) =
        sims_or_skip("a_cut_the_simulator_makes_deeper_is_reported", Firmware::GrblHal)
    else {
        return;
    };
    let case = of(Firmware::GrblHal)
        .into_iter()
        .find(|c| c.name.ends_with("/coaster-grblhal.nc"))
        .expect("the grblHAL coaster is in the corpus");
    let line = case
        .text
        .lines()
        .position(|l| l.contains("Z-4.000"))
        .expect("the coaster cuts to Z-4")
        + 1;
    let mut lines = vec![Line::harness(SET_WORK_OFFSET)];
    lines.extend(program_lines(&case.text).into_iter().map(|mut l| {
        if l.source == Some(line) {
            l.text = l.text.replace("Z-4.000", "Z-4.100");
        }
        l
    }));
    let session = sims
        .stream(Firmware::GrblHal, &lines, Sync::EachLine, &dir)
        .expect("the simulator runs");
    let events = rs274_motion::reference::reference(&case.text, &case.post, case.units)
        .expect("DreamCarve reads it");
    let frame = grbl_sim_motion::compare::Frame {
        mm_per_unit: 1.0,
        offset_mm: grbl_sim_motion::WORK_OFFSET_MM,
    };
    let observed: Vec<_> = session
        .exchanges
        .iter()
        .filter_map(|e| Some((e.line.source?, e.settled.clone()?)))
        .collect();
    let c = compare_lines(&targets(&events, &frame), &observed);
    let first = c.differences.first().expect("the deeper cut was caught");
    assert_eq!(first.line, Some(line), "{:#?}", c.differences);
    assert!(first.what.contains("vs -80.3000"), "{}", first.what);
}

/// The modes a user's MDI may leave, which a header must reset.
const GRBLHAL_PRELUDE: &[&str] = &[
    "G21 G90 G98",
    "G81 X0 Y0 Z-1 R1 F100",
    "G91",
    "G20",
    "G18",
    "G55",
    "G93",
    "G51 X2 Y2 Z2",
];
const GRBL_PRELUDE: &[&str] = &["G91", "G20", "G18", "G55", "G93"];

/// The `G` words a header writes on its own lines.
fn header_modes(case: &Case) -> Vec<String> {
    header_lines(&case.text)
        .iter()
        .flat_map(|l| code_words(&l.text).collect::<Vec<_>>())
        .filter(|w| w.starts_with('G'))
        .collect()
}

fn assert_header_sets_the_modal_state(firmware: Firmware, prelude: &[&str], test: &str) {
    let Some((sims, dir)) = sims_or_skip(test, firmware) else {
        return;
    };
    let mut failures = Vec::new();
    for case in of(firmware) {
        let modes = header_modes(&case);
        let state = modal_state_after(firmware, prelude, &header_lines(&case.text), &sims, &dir)
            .expect("the header runs");
        for mode in &modes {
            if !state.contains(mode) {
                failures.push(format!("{}: header {mode}, but $G reports {state:?}", case.name));
            }
        }
        for left in ["G91", "G18", "G55", "G93", "G81"] {
            if state.iter().any(|w| w == left) {
                failures.push(format!("{}: {left} survives the header: {state:?}", case.name));
            }
        }
        let units = if modes.iter().any(|m| m == "G20") { "G20" } else { "G21" };
        if !state.iter().any(|w| w == units) {
            failures.push(format!("{}: $G reports {state:?}, not {units}", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// After MDI leaves incremental mode, inches, the XZ plane, G55, inverse
/// time, a `G81` motion mode and a scale, each grblHAL header leaves `$G`
/// as it says: every mode it writes, none of those left over.
#[test]
fn grblhal_header_sets_the_modal_state() {
    assert_header_sets_the_modal_state(
        Firmware::GrblHal,
        GRBLHAL_PRELUDE,
        "grblhal_header_sets_the_modal_state",
    );
}

/// As for grblHAL, on GRBL, which has neither cycles nor scaling.
#[test]
fn grbl_header_sets_the_modal_state() {
    assert_header_sets_the_modal_state(
        Firmware::Grbl,
        GRBL_PRELUDE,
        "grbl_header_sets_the_modal_state",
    );
}

fn grblhal_v4_header() -> Vec<Line> {
    let case = of(Firmware::GrblHal)
        .into_iter()
        .find(|c| c.name.ends_with("/coaster-grblhal.nc"))
        .expect("the grblHAL coaster is in the corpus");
    assert!(has_word(&case, "G50"), "the latest grblHAL header writes G50");
    header_lines(&case.text)
}

/// grblHAL v4's `G50` clears a scale that includes X, the case the
/// definitions README says it does ("Scaling on grblHAL").
#[test]
fn grblhal_g50_clears_a_scale_that_includes_x() {
    let Some((sims, dir)) =
        sims_or_skip("grblhal_g50_clears_a_scale_that_includes_x", Firmware::GrblHal)
    else {
        return;
    };
    let header = grblhal_v4_header();
    for scale in ["G51 X2", "G51 X-1 Z-1", "G51 X2 Y2 Z2"] {
        let state = modal_state_after(Firmware::GrblHal, &[scale], &header, &sims, &dir)
            .expect("the header runs");
        assert!(
            state.iter().any(|w| w == "G50"),
            "after {scale} and the header, $G reports {state:?}"
        );
    }
}

/// The known grblHAL bug the definitions README documents ("Scaling on
/// grblHAL"): `G50` does not clear a scale that leaves X at 1, such as
/// `G51 Z-1`, because `set_scaling` reports only whether X changed. This
/// test records what the simulator does and never fails on it: the header
/// cannot work around it, and the export note tells the user to
/// soft-reset. When it prints "fixed", the README and the export note can
/// name the first grblHAL commit with the fix.
#[test]
fn grblhal_g50_and_a_z_only_scale() {
    let Some((sims, dir)) = sims_or_skip("grblhal_g50_and_a_z_only_scale", Firmware::GrblHal)
    else {
        return;
    };
    let header = grblhal_v4_header();
    for scale in ["G51 Z-1", "G51 Y-1"] {
        let state = modal_state_after(Firmware::GrblHal, &[scale], &header, &sims, &dir)
            .expect("the header runs");
        if state.iter().any(|w| w.starts_with("G51")) {
            eprintln!(
                "grblhal_g50_and_a_z_only_scale: known grblHAL bug still present: after {scale} \
                 and the v4 header's G50, $G reports {state:?}"
            );
        } else {
            eprintln!(
                "grblhal_g50_and_a_z_only_scale: fixed: after {scale} and the v4 header's G50, \
                 $G reports {state:?}; update the definitions README"
            );
        }
    }
}

/// The `G53` lift lands at machine Z -5 mm under a G54 offset of Z
/// -76.2 mm, where a lift that ignored `G53` would land at -81.2.
#[test]
fn g53_lift_lands_at_machine_z() {
    for firmware in [Firmware::GrblHal, Firmware::Grbl] {
        let Some((sims, dir)) = sims_or_skip("g53_lift_lands_at_machine_z", firmware) else {
            return;
        };
        let case = of(firmware)
            .into_iter()
            .find(|c| c.name.contains("machine-z") && has_word(c, "G53"))
            .expect("a machine-z plaque");
        let lift = case
            .text
            .lines()
            .position(|l| l.starts_with("G53 G0 Z"))
            .expect("the lift")
            + 1;
        let mut lines = vec![Line::harness(SET_WORK_OFFSET)];
        lines.extend(program_lines(&case.text));
        let session = sims
            .stream(firmware, &lines, Sync::EachLine, &dir)
            .expect("the simulator runs");
        let at = session
            .exchanges
            .iter()
            .find(|e| e.line.source == Some(lift))
            .and_then(|e| e.settled.as_ref().map(|s| s.position_mm))
            .expect("a position after the lift");
        assert!(
            (at[2] + 5.0).abs() < 1e-3,
            "{firmware}: after `{}`, machine Z is {}",
            case.text.lines().nth(lift - 1).unwrap_or(""),
            at[2]
        );
    }
}
