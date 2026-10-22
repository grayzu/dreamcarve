// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runs DreamCarve's grblHAL and GRBL programs through the controllers'
//! own firmware, built for a desktop: the grblHAL Simulator and grbl-sim
//! (issue #329). It is the grblHAL and GRBL counterpart of
//! `rs274-motion`.
//!
//! What it checks, for each program in the [`corpus`]:
//!
//! - **The validator accepts it** ([`validate`]): every line parses with no
//!   error (layer 1 of `docs/testing/post-processor-e2e.md`).
//! - **Check mode accepts it** ([`check_mode`]): streamed after `$C`, as
//!   the GRBL wiki tells a user to try a program, every line is `ok` and
//!   nothing moves.
//! - **The motion is DreamCarve's** ([`check_motion`]): the firmware runs
//!   it from its default settings under a G54 offset, resuming each `M0`
//!   (and grblHAL `M6`) as an operator would. After each line's motion
//!   the firmware's own report of where it is, its motion mode and its
//!   feed are compared with DreamCarve's reading of the program, the one
//!   the safety pass checks ([`compare`]), and so is where a `G53` lift
//!   lands in machine Z. The number of pauses and the final position are
//!   compared too.
//! - **The header sets the modal state** ([`modal_state_after`]): after a
//!   prelude that leaves the modes a user's MDI might (incremental, inches,
//!   another plane, another work offset, inverse time, a canned-cycle
//!   motion mode, a scale), the program's header lines leave `$G` as the
//!   header says.
//!
//! The simulators are GPL-3.0. They run as separate programs, natively or
//! in a container, and nothing of them is copied here.

pub mod compare;
pub mod corpus;
pub mod log;
pub mod runner;

use std::path::Path;

use dc_project::{PostProcessor, Units, latest_definition};
use rs274_motion::reference::reference;

use crate::compare::{Comparison, Difference, Frame, compare_final, compare_lines, targets};
use crate::corpus::{Case, program_units};
use crate::log::parse_modal_state;
use crate::runner::{Firmware, Line, Reply, Session, Settled, Sims, Sync};

/// The tolerance on positions, mm. DreamCarve writes three decimals in
/// millimetres and four in inches; at the 1000 steps/mm the harness sets,
/// the firmware lands within half a step of every target, and its status
/// report prints thousandths.
pub const LENGTH_TOLERANCE_MM: f64 = 5e-4;

/// The relative tolerance on feeds, on top of half the report's last
/// digit. Both firmwares hold the feed as a 32-bit float.
pub const RATE_TOLERANCE: f64 = 1e-5;

/// The G54 offset the harness sets before every program, mm: the
/// offset `rs274-motion` uses, (1, 2, -3) inches. A `G53` lift to machine
/// Z -5 mm is work Z 71.2 under it, so a lift that ignored `G53` would
/// show.
pub const WORK_OFFSET_MM: [f64; 3] = [25.4, 50.8, -76.2];

/// The line that sets [`WORK_OFFSET_MM`], sent before the program.
pub const SET_WORK_OFFSET: &str = "G10 L2 P1 X25.400 Y50.800 Z-76.200";

/// What one check came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The simulator ran it, and its motion is DreamCarve's, or differs as
    /// listed.
    Compared(Comparison),
    /// The firmware answered a line with an error, or raised an alarm.
    Rejected(String),
    /// The check could not be made: the simulator would not start or
    /// stopped answering, or the program could not be read.
    Failed(String),
}

impl Verdict {
    /// True if the simulator ran the program and its motion is
    /// DreamCarve's.
    #[must_use]
    pub fn agrees(&self) -> bool {
        matches!(self, Self::Compared(c) if c.differences.is_empty())
    }
}

/// The lines of `text` to send, numbered from 1, blank lines left out.
#[must_use]
pub fn program_lines(text: &str) -> Vec<Line> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(k, l)| Line {
            text: l.trim_end().to_owned(),
            source: Some(k + 1),
        })
        .collect()
}

fn code(line: &str) -> String {
    line.split(['(', ';'])
        .next()
        .unwrap_or("")
        .to_ascii_uppercase()
}

fn has_word(line: &str, word: &str) -> bool {
    code(line).split_whitespace().any(|w| w == word)
}

/// The number of times `text` should pause: its `M0` lines, and its `M6`
/// lines, where grblHAL's default tool-change mode waits in the `Tool`
/// state for cycle start. GRBL has no `M6`, and no GRBL program writes one.
#[must_use]
pub fn pauses(text: &str) -> usize {
    text.lines()
        .filter(|l| has_word(l, "M0") || has_word(l, "M6"))
        .count()
}

/// Whether a firmware's validator can check `case`. grblHAL's crashes on
/// every input at the pinned commit, and GRBL's waits forever at an `M0`,
/// having no cycle start (README, "What runs"). Check mode and the motion
/// run cover those programs.
#[must_use]
pub fn validator_applies(case: &Case) -> bool {
    case.firmware == Firmware::Grbl && !case.text.lines().any(|l| has_word(l, "M0"))
}

/// The header: the lines before the first that moves an axis or starts
/// the spindle.
#[must_use]
pub fn header_lines(text: &str) -> Vec<Line> {
    program_lines(text)
        .into_iter()
        .take_while(|l| {
            let c = code(&l.text);
            !c.split_whitespace()
                .any(|w| w.starts_with(['X', 'Y', 'Z']) || matches!(w, "M3" | "M4" | "M0" | "G4"))
        })
        .collect()
}

/// Runs `text`'s validator for `firmware`: `None` if it accepts the
/// program, else why not.
///
/// # Errors
///
/// Why the validator could not be run.
pub fn validate(
    text: &str,
    firmware: Firmware,
    sims: &Sims,
    dir: &Path,
) -> Result<Option<String>, String> {
    let run = sims
        .validate(firmware, text, dir)
        .map_err(|e| format!("cannot run {firmware}'s validator from {sims}: {e}"))?;
    Ok(run.rejection())
}

fn session_rejection(session: &Session) -> Option<String> {
    let mut why: Vec<String> = session
        .errors()
        .map(|e| match e.line.source {
            Some(l) => format!("line {l} `{}`: {:?}", e.line.text, e.reply),
            None => format!("`{}`: {:?}", e.line.text, e.reply),
        })
        .collect();
    why.extend(session.alarms.iter().cloned());
    (!why.is_empty()).then(|| why.join("; "))
}

/// Runs `case` through its firmware's simulator and compares the motion
/// with DreamCarve's reading.
#[must_use]
pub fn check_motion(case: &Case, sims: &Sims, dir: &Path) -> Verdict {
    match run_motion(case, sims, dir) {
        Ok(v) => v,
        Err(why) => Verdict::Failed(why),
    }
}

fn frame(units: Units) -> Frame {
    Frame {
        mm_per_unit: match units {
            Units::Mm => 1.0,
            Units::Inch => 25.4,
        },
        offset_mm: WORK_OFFSET_MM,
    }
}

fn run_motion(case: &Case, sims: &Sims, dir: &Path) -> Result<Verdict, String> {
    let events = reference(&case.text, &case.post, case.units)
        .map_err(|e| format!("DreamCarve cannot read it: {e}"))?;
    let targets = targets(&events, &frame(program_units(&case.text, case.units)));
    let mut lines = vec![Line::harness(SET_WORK_OFFSET)];
    lines.extend(program_lines(&case.text));
    let session = sims
        .stream(case.firmware, &lines, Sync::EachLine, dir)
        .map_err(|e| format!("cannot run {}'s simulator from {sims}: {e}", case.firmware))?;
    if let Some(why) = session_rejection(&session) {
        return Ok(Verdict::Rejected(why));
    }
    let observed: Vec<(usize, Settled)> = session
        .exchanges
        .iter()
        .filter_map(|e| Some((e.line.source?, e.settled.clone()?)))
        .collect();
    let mut comparison = compare_lines(&targets, &observed);
    match session.final_position_mm {
        Some(p) => comparison.differences.extend(compare_final(&targets, &p)),
        None => comparison.differences.push(Difference {
            line: None,
            what: "no final position reported".to_owned(),
        }),
    }
    let expected = pauses(&case.text);
    if session.resumes.len() != expected {
        comparison.differences.push(Difference {
            line: None,
            what: format!(
                "{expected} M0 or M6 line(s), but the controller paused {} time(s), after lines {:?}",
                session.resumes.len(),
                session.resumes
            ),
        });
    }
    Ok(Verdict::Compared(comparison))
}

/// Streams `case` after `$C`, GRBL's check mode: what went wrong, if
/// anything. Every line must be `ok`, and the firmware must not move.
///
/// # Errors
///
/// Why the simulator could not be run.
pub fn check_mode(case: &Case, sims: &Sims, dir: &Path) -> Result<Vec<String>, String> {
    let mut lines = vec![Line::harness("$C")];
    lines.extend(program_lines(&case.text));
    let session = sims
        .stream(case.firmware, &lines, Sync::Streamed, dir)
        .map_err(|e| format!("cannot run {}'s simulator from {sims}: {e}", case.firmware))?;
    let mut problems: Vec<String> = session_rejection(&session).into_iter().collect();
    let enabled = session
        .exchanges
        .iter()
        .find(|e| e.line.text == "$C")
        .is_some_and(|e| e.reply == Reply::Ok && e.messages.iter().any(|m| m.contains("Enabled")));
    if !enabled {
        problems.push("$C did not enable check mode".to_owned());
    }
    match session.final_position_mm {
        Some(p) if p.iter().all(|v| v.abs() < LENGTH_TOLERANCE_MM) => {}
        other => problems.push(format!("the machine moved in check mode: {other:?}")),
    }
    Ok(problems)
}

/// The `$G` words after `prelude` and then `header`, sent to `firmware`.
///
/// # Errors
///
/// Why the simulator could not be run, a line it refused, or no `$G`
/// report.
pub fn modal_state_after(
    firmware: Firmware,
    prelude: &[&str],
    header: &[Line],
    sims: &Sims,
    dir: &Path,
) -> Result<Vec<String>, String> {
    let mut lines: Vec<Line> = prelude.iter().map(|l| Line::harness(l)).collect();
    lines.extend(header.iter().cloned());
    lines.push(Line::harness("$G"));
    let session = sims
        .stream(firmware, &lines, Sync::Streamed, dir)
        .map_err(|e| format!("cannot run {firmware}'s simulator from {sims}: {e}"))?;
    if let Some(why) = session_rejection(&session) {
        return Err(why);
    }
    session
        .exchanges
        .last()
        .and_then(|e| e.messages.iter().find_map(|m| parse_modal_state(m)))
        .ok_or_else(|| "no [GC:...] report for $G".to_owned())
}

/// The probe every simulator test runs first: a program with a known
/// result (`G0 X1 Y2 Z3` under the work offset, so machine (26.4, 52.8,
/// -73.2)). If its motion is not what DreamCarve reads, the simulator or
/// the harness is misconfigured, and nothing else it says means anything.
///
/// # Errors
///
/// Why the probe failed, starting "misconfigured".
pub fn probe(firmware: Firmware, sims: &Sims, dir: &Path) -> Result<(), String> {
    let post: PostProcessor = latest_definition(firmware.definition_id())
        .map_err(|e| e.to_string())?
        .post_processor()
        .clone();
    let case = Case {
        name: "probe".to_owned(),
        post,
        firmware,
        units: Units::Mm,
        text: "G21 G90\nG0 X1 Y2 Z3\nM2\n".to_owned(),
    };
    match check_motion(&case, sims, dir) {
        Verdict::Compared(c) if c.differences.is_empty() && c.moves == 1 => Ok(()),
        other => Err(format!(
            "misconfigured: {firmware} probe G0 X1 Y2 Z3: {other:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_their_source_numbers() {
        let lines = program_lines("G90\n\nG0 Z5\r\n");
        assert_eq!(
            lines,
            vec![
                Line {
                    text: "G90".to_owned(),
                    source: Some(1)
                },
                Line {
                    text: "G0 Z5".to_owned(),
                    source: Some(3)
                },
            ]
        );
    }

    #[test]
    fn the_header_ends_at_the_first_move_or_spindle_start() {
        let text = "( Project )\nG90\nG17\nG21\nG54\nG0 Z5.000\nM3 S1000\n";
        let header: Vec<String> = header_lines(text).into_iter().map(|l| l.text).collect();
        assert_eq!(header, ["( Project )", "G90", "G17", "G21", "G54"]);
        let manual = "G90\nG21\nM0 (MSG, Turn the router ON.)\nG0 Z5\n";
        assert_eq!(header_lines(manual).len(), 2);
    }

    #[test]
    fn pauses_counts_m0_and_m6_lines_not_comments() {
        assert_eq!(
            pauses("M0 (MSG, on)\n(M0 in a comment)\nT2 M6 (MSG, Fit T2)\nM30\n"),
            2
        );
    }
}
