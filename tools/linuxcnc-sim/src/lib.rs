// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runs DreamCarve's LinuxCNC and generic programs through LinuxCNC's full
//! simulation, headless, and checks what `rs274` alone cannot (issue
//! #328, layer 3 of `docs/testing/post-processor-e2e.md`):
//!
//! - the program runs to completion under task and motion, on a homed
//!   machine with soft limits;
//! - every move task sends to motion is the move DreamCarve reads: kind,
//!   end point, arc centre and feed, and the machine Z of a `G53` lift,
//!   with the INI in millimetres and the program in either unit;
//! - the same holds when the INI's startup code sets every mode the
//!   header must reset;
//! - each `M0` pauses with the machine where DreamCarve reads it, and the
//!   run carries on when the UI resumes it;
//! - the run ends where DreamCarve reads it;
//! - a `G53` lift beyond Z's travel stops the program with a soft-limit
//!   error before the lift;
//! - a machine that is not homed does not run a program.
//!
//! The pieces:
//!
//! - [`config`] writes a session: the INI (from `sim/machine.ini.in`), a
//!   HAL file, the headless UI `sim/dreamcarve-ui.py`, a parameter file
//!   with a G54 offset, and the programs.
//! - [`runner`] runs `linuxcnc -r`, natively or in the `rs274-motion`
//!   container.
//! - [`report`] reads the UI's record; [`motion_log`] reads
//!   motion-logger's log.
//! - [`check`] holds both against DreamCarve's reading, reusing
//!   `rs274-motion`'s comparison.
//!
//! LinuxCNC is GPL-2.0. It runs as a separate program, and nothing of it
//! is copied here.

pub mod check;
pub mod config;
pub mod motion_log;
pub mod report;
pub mod runner;

use std::fs;
use std::path::Path;

use dc_project::Units;
use rs274_motion::corpus::{Case, CorpusError, build_corpus};
use rs274_motion::reference::{RefEvent, reference};

use crate::check::{Expected, check_run, compare_log, expected, units_by_line};
use crate::config::{HOSTILE_STARTUP, Machine, Session};
use crate::report::ProgramReport;
use crate::runner::{LinuxCnc, RunError};

/// The G54 offset the parameter file sets, millimetres: the same as
/// `rs274-motion`'s (1, 2, -3) inches, so a `G53` move to machine Z -5 is
/// work Z 71.2, and a work move is told apart from a machine one.
pub const WORK_OFFSET_MM: [f64; 3] = [25.4, 50.8, -76.2];

/// The tolerance on a machine position, millimetres.
pub const LENGTH_TOLERANCE_MM: f64 = 5e-4;

/// The definitions whose programs LinuxCNC reads as written.
pub const POSTS: &[&str] = &["linuxcnc", "generic"];

/// The `rs274-motion` corpus programs for [`POSTS`].
///
/// # Errors
///
/// [`CorpusError`] when the corpus does not build.
pub fn linuxcnc_cases(root: &Path) -> Result<Vec<Case>, CorpusError> {
    Ok(build_corpus(root)?
        .into_iter()
        .filter(|c| POSTS.contains(&c.post.id.as_str()))
        .collect())
}

/// One program's outcome across the sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// The case's name.
    pub name: String,
    /// Moves compared with the motion log, every session together.
    pub moves: usize,
    /// `M0` pauses checked on the motion module.
    pub pauses: usize,
    /// Every problem found, each naming its session.
    pub problems: Vec<String>,
}

impl Outcome {
    /// True if no session found a problem.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.problems.is_empty()
    }
}

/// True for a case [`check_cases`] also runs under [`HOSTILE_STARTUP`]: a
/// LinuxCNC program that is committed, in inches (against the startup
/// `G20`), or with `G81` cycles (against its `G99`). The other plaque
/// variants have the same header, so they would only add minutes.
#[must_use]
pub fn under_startup_code(case: &Case) -> bool {
    case.post.id == "linuxcnc"
        && (!case.name.starts_with("plaque/")
            || case.name.contains("/inch/")
            || case.name.contains("/canned-g81/"))
}

/// What DreamCarve reads in a case: its events, units by line, and what
/// the simulation should show.
struct Reading {
    events: Vec<RefEvent>,
    by_line: Vec<Units>,
    expected: Expected,
}

fn read(case: &Case) -> Result<Reading, String> {
    let events = reference(&case.text, &case.post, case.units)
        .map_err(|e| format!("DreamCarve cannot read it: {e}"))?;
    let by_line = units_by_line(&case.text, &case.post, case.units)
        .map_err(|e| format!("DreamCarve cannot read it: {e}"))?;
    let expected = expected(&events, &by_line);
    Ok(Reading {
        events,
        by_line,
        expected,
    })
}

/// Runs `texts` on `machine` with `startup` in `dir` (created if
/// missing).
///
/// # Errors
///
/// [`RunError`] when the session gives no complete record.
pub fn run_session(
    linuxcnc: &LinuxCnc,
    machine: Machine,
    startup: Option<&str>,
    texts: &[&str],
    dir: &Path,
) -> Result<runner::SessionRun, RunError> {
    fs::create_dir_all(dir)?;
    let mut session = Session::new(machine, texts.iter().copied());
    session.startup = startup.map(str::to_owned);
    linuxcnc.run(&session, dir)
}

/// Runs `cases` through three sessions, in subdirectories of `dir`:
///
/// 1. motion-logger, LinuxCNC's default startup code: the motion of
///    every case;
/// 2. motion-logger, [`HOSTILE_STARTUP`]: the motion of the LinuxCNC
///    cases [`under_startup_code`] picks, whose header must override it
///    (generic writes no `G91.1`, so it would read `G90.1` arcs, as its
///    controller's INI would make it);
/// 3. the motion module, homed: completion, pauses and the end.
///
/// # Errors
///
/// [`RunError`] when a session gives no complete record.
pub fn check_cases(
    cases: &[Case],
    linuxcnc: &LinuxCnc,
    dir: &Path,
) -> Result<Vec<Outcome>, RunError> {
    let readings: Vec<Result<Reading, String>> = cases.iter().map(read).collect();
    let texts: Vec<&str> = cases.iter().map(|c| c.text.as_str()).collect();
    let linuxcnc_only: Vec<usize> = cases
        .iter()
        .enumerate()
        .filter(|(_, c)| under_startup_code(c))
        .map(|(k, _)| k)
        .collect();
    let startup_texts: Vec<&str> = cases
        .iter()
        .filter(|c| under_startup_code(c))
        .map(|c| c.text.as_str())
        .collect();

    // Three LinuxCNCs at once, each in its own directory (and container):
    // motion-logger answers about one command per 10 ms, so the logged
    // sessions take minutes on the plaque, and the sessions share nothing.
    let (logged, startup, travel) = std::thread::scope(|s| {
        let logged =
            s.spawn(|| run_session(linuxcnc, Machine::Logger, None, &texts, &dir.join("logger")));
        let startup = s.spawn(|| {
            run_session(
                linuxcnc,
                Machine::Logger,
                Some(HOSTILE_STARTUP),
                &startup_texts,
                &dir.join("startup"),
            )
        });
        let travel = run_session(linuxcnc, Machine::Travel, None, &texts, &dir.join("travel"));
        let join = |h: std::thread::ScopedJoinHandle<'_, Result<runner::SessionRun, RunError>>| {
            h.join().unwrap_or_else(|_| {
                Err(RunError::Io(std::io::Error::other(
                    "a session thread panicked",
                )))
            })
        };
        (join(logged), join(startup), travel)
    });
    let (logged, startup, travel) = (logged?, startup?, travel?);

    let mut out = Vec::new();
    for (k, (case, reading)) in cases.iter().zip(&readings).enumerate() {
        let mut outcome = Outcome {
            name: case.name.clone(),
            ..Outcome::default()
        };
        let reading = match reading {
            Ok(r) => r,
            Err(why) => {
                outcome.problems.push(why.clone());
                out.push(outcome);
                continue;
            }
        };
        if !travel.report.homed {
            outcome
                .problems
                .push("motion module: the machine did not home".to_owned());
        }
        let logs = [
            (
                "motion-logger".to_owned(),
                logged.report.programs.get(k),
                logged.logs.get(k),
            ),
            (
                format!("motion-logger, startup code `{HOSTILE_STARTUP}`"),
                linuxcnc_only
                    .iter()
                    .position(|&j| j == k)
                    .and_then(|j| startup.report.programs.get(j)),
                linuxcnc_only
                    .iter()
                    .position(|&j| j == k)
                    .and_then(|j| startup.logs.get(j)),
            ),
        ];
        for (label, run, log) in logs {
            let (Some(run), Some(log)) = (run, log) else {
                continue;
            };
            check_logged(&label, run, log, reading, &mut outcome);
        }
        if let Some(run) = travel.report.programs.get(k) {
            outcome.pauses += run.pauses.len();
            outcome.problems.extend(
                check_run(run, &reading.expected)
                    .into_iter()
                    .map(|p| format!("motion module: {p}")),
            );
        }
        out.push(outcome);
    }
    Ok(out)
}

fn check_logged(
    label: &str,
    run: &ProgramReport,
    log: &str,
    reading: &Reading,
    outcome: &mut Outcome,
) {
    if !run.completed() {
        outcome.problems.push(format!(
            "{label}: did not run to completion: {}",
            run.errors.join(" | ")
        ));
    }
    match compare_log(&reading.events, &reading.by_line, log) {
        Ok((comparison, pauses)) => {
            outcome.moves += comparison.moves;
            for d in comparison.differences {
                let line = d.line.map_or_else(|| "-".to_owned(), |l| l.to_string());
                outcome
                    .problems
                    .push(format!("{label}: line {line}: {}", d.what));
            }
            if pauses != reading.expected.stops.len() {
                outcome.problems.push(format!(
                    "{label}: task paused motion {pauses} time(s), DreamCarve reads {} M0",
                    reading.expected.stops.len()
                ));
            }
        }
        Err(why) => outcome.problems.push(format!("{label}: {why}")),
    }
}
