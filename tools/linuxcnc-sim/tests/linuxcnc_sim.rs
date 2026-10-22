// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Issue #328: DreamCarve's LinuxCNC programs run under LinuxCNC's full
//! simulation, headless, as DreamCarve reads them: to completion on a
//! homed machine, with the motion task sends to motion, the `M0` pauses,
//! the `G53` lift and the end where DreamCarve puts them, and with a lift
//! beyond Z's travel refused.
//!
//! The tests that need LinuxCNC skip, saying so on standard error, unless
//! `DREAMCARVE_LINUXCNC` names one. To run them
//! (`tools/linuxcnc-sim/README.md`):
//!
//! ```sh
//! podman build -t dreamcarve-rs274 tools/rs274-motion
//! DREAMCARVE_LINUXCNC=podman:dreamcarve-rs274 cargo test -p linuxcnc-sim -- --nocapture
//! ```
//!
//! Each session starts LinuxCNC, so they run one at a time.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Mutex;

use dc_project::Units;
use linuxcnc_sim::check::{check_run, compare_log, expected, units_at, units_by_line};
use linuxcnc_sim::config::{HOSTILE_STARTUP, Machine, SHORT_Z_MAX_MM, Z_MIN_MM};
use linuxcnc_sim::motion_log::{LogEvent, parse_motion_log};
use linuxcnc_sim::runner::{ENV, LinuxCnc};
use linuxcnc_sim::{WORK_OFFSET_MM, check_cases, linuxcnc_cases, run_session, under_startup_code};
use rs274_motion::corpus::Case;
use rs274_motion::reference::{RefEvent, reference};

/// One LinuxCNC at a time: a native one shares the host's HAL memory,
/// and each container is a full LinuxCNC.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cases() -> Vec<Case> {
    linuxcnc_cases(&root()).expect("the corpus builds")
}

fn case(suffix: &str) -> Case {
    let found = cases().into_iter().find(|c| c.name.ends_with(suffix));
    assert!(found.is_some(), "no case {suffix} in the corpus");
    found.expect("found")
}

/// The LinuxCNC to run, or `None` after saying why the test is skipped.
fn linuxcnc_or_skip(test: &str) -> Option<LinuxCnc> {
    let found = LinuxCnc::detect();
    if found.is_none() {
        eprintln!(
            "{test}: skipped: no LinuxCNC. Set {ENV}=podman:dreamcarve-rs274 after \
             `podman build -t dreamcarve-rs274 tools/rs274-motion`, or {ENV} to the linuxcnc \
             script (tools/linuxcnc-sim/README.md)."
        );
    }
    found
}

fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            l.split(['(', ';'])
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_uppercase()
        })
        .collect()
}

fn has_word(case: &Case, word: &str) -> bool {
    code_lines(&case.text)
        .iter()
        .any(|l| l.split_whitespace().any(|w| w == word))
}

fn expectation(case: &Case) -> linuxcnc_sim::check::Expected {
    let events = reference(&case.text, &case.post, case.units).expect("DreamCarve reads it");
    let by_line = units_by_line(&case.text, &case.post, case.units).expect("DreamCarve reads it");
    expected(&events, &by_line)
}

/// The program set is not vacuous: both definitions, and each feature the
/// full simulation is there to check. The soft-limit test's premise holds
/// too: the lift is inside the normal machine's Z travel and beyond the
/// short one's, and every work move's machine Z is inside both. If an export change drops a feature or moves the lift, this
/// fails rather than the simulation quietly checking less.
#[test]
fn corpus_covers_what_the_full_simulation_checks() {
    let cases = cases();
    for id in ["linuxcnc", "generic"] {
        assert!(
            cases.iter().any(|c| c.post.id == id),
            "no {id} program in the corpus"
        );
    }
    for (word, what) in [
        ("G53", "a G53 machine-Z lift"),
        ("M0", "an M0 pause"),
        ("G20", "an inch program"),
        ("G81", "a G81 cycle"),
        ("G82", "a G82 cycle with dwell"),
        ("G2", "a clockwise arc"),
        ("G91.1", "incremental arc centres"),
    ] {
        assert!(
            cases.iter().any(|c| has_word(c, word)),
            "no program in the corpus has {what} ({word})"
        );
    }
    let startup: Vec<&Case> = cases.iter().filter(|c| under_startup_code(c)).collect();
    for (word, what) in [
        ("G20", "an inch program, against the startup G20"),
        ("G81", "a G81 cycle, against the startup G99"),
        ("G2", "an arc, against the startup G90.1 and G18"),
    ] {
        assert!(
            startup.iter().any(|c| has_word(c, word)),
            "no program under the startup code has {what} ({word})"
        );
    }
    let manual = case("plaque/linuxcnc/manual-machine-z/program-1");
    let e = expectation(&manual);
    assert!(
        e.stops.iter().any(|(_, [_, _, z])| *z == Some(-5.0)),
        "no M0 right after the G53 lift: {:?}",
        e.stops
    );
    let lifted = case("plaque/linuxcnc/machine-z/program-1");
    let e = expectation(&lifted);
    assert_eq!(e.lifts.len(), 1, "{:?}", e.lifts);
    let lift_mm = e.lifts[0].1;
    assert!(
        lift_mm <= Machine::Travel.z_max_mm() && lift_mm > SHORT_Z_MAX_MM,
        "the lift to machine Z {lift_mm} must be inside Travel and beyond ShortZ"
    );
    for c in &cases {
        let events = reference(&c.text, &c.post, c.units).expect("DreamCarve reads it");
        let by_line = units_by_line(&c.text, &c.post, c.units).expect("DreamCarve reads it");
        for event in &events {
            if let RefEvent::Move { line, to, .. } = event
                && let Some(z) = to.z
            {
                let mm = match units_at(&by_line, *line) {
                    Units::Mm => 1.0,
                    Units::Inch => 25.4,
                };
                let machine_z = z.mul_add(mm, WORK_OFFSET_MM[2]);
                assert!(
                    (Z_MIN_MM..=SHORT_Z_MAX_MM).contains(&machine_z),
                    "{} line {line}: work Z {z} is machine Z {machine_z}, outside the short travel",
                    c.name
                );
            }
        }
    }
}

/// Every LinuxCNC and generic corpus program runs to completion under
/// `linuxcnc -r`, and what LinuxCNC does is what DreamCarve reads: every
/// logged move's end point, arc centre and feed within 5e-4 program units,
/// with the INI in mm, also under a startup code the header must
/// override; every `M0` pause where DreamCarve puts the machine; and the
/// end there too.
#[test]
fn full_sim_motion_matches_dreamcarve() {
    let Some(linuxcnc) = linuxcnc_or_skip("full_sim_motion_matches_dreamcarve") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cases = cases();
    let outcomes =
        check_cases(&cases, &linuxcnc, &work_dir("corpus")).expect("every session reports");
    let failures: Vec<String> = outcomes
        .iter()
        .filter(|o| !o.agrees())
        .map(|o| format!("{}:\n  {}", o.name, o.problems.join("\n  ")))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert!(
        outcomes.iter().map(|o| o.moves).sum::<usize>() > 1000,
        "too few moves compared: {outcomes:#?}"
    );
    assert!(outcomes.iter().any(|o| o.pauses > 0), "no M0 was checked");
}

/// Under the INI whose Z travel ends at machine -20 mm, the lift to
/// machine Z -5 mm ends the run with LinuxCNC's soft-limit error naming
/// the lift's line, before the lift: Z never got above the travel.
#[test]
fn full_sim_reports_a_lift_beyond_soft_limits() {
    let Some(linuxcnc) = linuxcnc_or_skip("full_sim_reports_a_lift_beyond_soft_limits") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lifted = case("plaque/linuxcnc/machine-z/program-1");
    let e = expectation(&lifted);
    let (lift_line, _) = e.lifts[0];
    let run = run_session(
        &linuxcnc,
        Machine::ShortZ,
        None,
        &[lifted.text.as_str()],
        &work_dir("short-z"),
    )
    .expect("the session reports");
    assert!(run.report.homed, "{}", run.output);
    let program = &run.report.programs[0];
    assert!(!program.completed(), "the lift ran: {program:#?}");
    let expected_error = format!("move on line {lift_line} would exceed Z's positive limit");
    assert!(
        program.errors.iter().any(|e| e.contains(&expected_error)),
        "no `{expected_error}`: {:#?}",
        program.errors
    );
    // Motion refuses the lift when task queues it, and task reads ahead,
    // so the program stops while motion is still on an earlier line: Z
    // never goes above its travel.
    let end = program.end.expect("the UI recorded the end");
    assert!(
        end.machine_mm[2] <= SHORT_Z_MAX_MM,
        "Z went above its travel: {end:?}"
    );
    // The same program on the normal machine completes, so the error is
    // the lift's.
    let travel = run_session(
        &linuxcnc,
        Machine::Travel,
        None,
        &[lifted.text.as_str()],
        &work_dir("travel-lift"),
    )
    .expect("the session reports");
    let program = &travel.report.programs[0];
    assert!(program.completed(), "{program:#?}");
    let end = program.end.expect("the UI recorded the end");
    assert!(
        (end.machine_mm[2] - e.lifts[0].1).abs() < 5e-4,
        "ended at machine Z {}, not the lift's {}",
        end.machine_mm[2],
        e.lifts[0].1
    );
}

/// With manual spindle control the program pauses at each `M0` until the
/// UI resumes it. At each pause `linuxcncrsh` reports the program paused
/// on a line before the `M0`'s, and the machine is where DreamCarve reads
/// it: after the lift, at the lift's machine Z.
#[test]
fn full_sim_stops_at_m0() {
    let Some(linuxcnc) = linuxcnc_or_skip("full_sim_stops_at_m0") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let manual = case("plaque/linuxcnc/manual-machine-z/program-1");
    let e = expectation(&manual);
    assert!(e.stops.len() >= 2, "{:?}", e.stops);
    let run = run_session(
        &linuxcnc,
        Machine::Travel,
        None,
        &[manual.text.as_str()],
        &work_dir("m0"),
    )
    .expect("the session reports");
    assert!(
        run.report.startup_errors.is_empty(),
        "{:#?}",
        run.report.startup_errors
    );
    let program = &run.report.programs[0];
    assert_eq!(program.pauses.len(), e.stops.len(), "{program:#?}");
    let problems = check_run(program, &e);
    assert!(problems.is_empty(), "{problems:#?}");
    let last = program.pauses.last().expect("paused");
    assert!(
        (last.machine_mm[2] - e.lifts[0].1).abs() < 5e-4,
        "the router-off pause is not at the lift's machine Z: {last:?}"
    );
    assert!(
        program
            .messages
            .iter()
            .any(|m| m.contains("Turn the router OFF")),
        "the M0's message was not shown: {:#?}",
        program.messages
    );
}

/// A machine that has not homed does not run the program (LinuxCNC
/// forces homing unless `[TRAJ] NO_FORCE_HOMING` is set, which the
/// export note asks users to leave unset).
#[test]
fn full_sim_refuses_to_run_unhomed() {
    let Some(linuxcnc) = linuxcnc_or_skip("full_sim_refuses_to_run_unhomed") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lifted = case("plaque/linuxcnc/machine-z/program-1");
    let run = run_session(
        &linuxcnc,
        Machine::Unhomed,
        None,
        &[lifted.text.as_str()],
        &work_dir("unhomed"),
    )
    .expect("the session reports");
    assert!(!run.report.homed);
    let program = &run.report.programs[0];
    assert!(!program.completed(), "{program:#?}");
    assert!(
        program.errors.iter().any(|e| e.contains("not homed")),
        "{:#?}",
        program.errors
    );
    let end = program.end.expect("the UI recorded the end");
    assert!(
        end.machine_mm.iter().all(|v| v.abs() < 1e-9),
        "the machine moved: {end:?}"
    );
}

/// The startup code is in force, so the session that runs under it is not
/// vacuous: a program with no header moves in inches, incrementally.
#[test]
fn startup_code_applies_without_a_header() {
    let Some(linuxcnc) = linuxcnc_or_skip("startup_code_applies_without_a_header") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = run_session(
        &linuxcnc,
        Machine::Logger,
        Some(HOSTILE_STARTUP),
        &["G0 X1\nG0 X1\nM2\n"],
        &work_dir("startup"),
    )
    .expect("the session reports");
    let moves: Vec<[f64; 3]> = parse_motion_log(&run.logs[0])
        .expect("the log reads")
        .into_iter()
        .filter_map(|e| match e {
            LogEvent::Move(m) => Some(m.machine_mm),
            _ => None,
        })
        .collect();
    let xs: Vec<f64> = moves.iter().map(|m| m[0]).collect();
    assert_eq!(xs.len(), 2, "{moves:?}");
    assert!(
        xs.iter()
            .zip([25.4, 50.8])
            .all(|(x, e)| (x - e).abs() < 1e-9),
        "X 1 inch from 0, then 1 inch more: {moves:?}"
    );
}

/// The comparison can fail: the LinuxCNC golden with one cut 0.1 mm
/// deeper, run under motion-logger, is reported from that line against
/// DreamCarve's reading of the original.
#[test]
fn a_cut_linuxcnc_makes_deeper_is_reported() {
    let Some(linuxcnc) = linuxcnc_or_skip("a_cut_linuxcnc_makes_deeper_is_reported") else {
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let golden = case("post-linuxcnc-coaster/program.ngc");
    let line = golden
        .text
        .lines()
        .position(|l| l.contains("Z-4.000"))
        .expect("the golden cuts to Z-4")
        + 1;
    let mutated: String = golden
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
    let run = run_session(
        &linuxcnc,
        Machine::Logger,
        None,
        &[mutated.as_str()],
        &work_dir("mutated"),
    )
    .expect("the session reports");
    let events = reference(&golden.text, &golden.post, Units::Mm).expect("DreamCarve reads it");
    let by_line = units_by_line(&golden.text, &golden.post, Units::Mm).expect("reads");
    let (c, _) = compare_log(&events, &by_line, &run.logs[0]).expect("the log reads");
    assert!(!c.differences.is_empty(), "the deeper cut was not caught");
    assert_eq!(c.differences[0].line, Some(line), "{:#?}", c.differences);
    // Work Z is machine Z less the offset, so -4.1 comes back as
    // -4.0999...
    assert!(
        c.differences[0].what.contains("Z -4 vs -4.1")
            || c.differences[0].what.contains("Z -4 vs -4.09999"),
        "{}",
        c.differences[0].what
    );
}
