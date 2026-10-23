// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What DreamCarve's reading of a program says LinuxCNC will do, and the
//! checks of each session's record against it.

use dc_gcode::parse::parse_program_with;
use dc_gcode::{CycleModel, ExportError};
use dc_geometry::approx_eq;
use dc_project::{PostProcessor, Units};
use rs274_motion::compare::{Comparison, Reader, compare_with};
use rs274_motion::reference::RefEvent;

use crate::motion_log::{LogEvent, parse_motion_log, to_canon};
use crate::report::ProgramReport;
use crate::{LENGTH_TOLERANCE_MM, WORK_OFFSET_MM};

/// LinuxCNC's motion log, for the comparison: its speeds have six
/// significant digits (`%.6g`), so a feed matches to 1e-5, not
/// `rs274`'s 1e-6.
pub const LINUXCNC: Reader = Reader {
    name: "LinuxCNC",
    rate_tolerance: 1e-5,
};

/// The units in force on each source line of `text`, index `line - 1`,
/// as `dc-gcode` reads it: `units` until a `G20` or `G21`.
///
/// # Errors
///
/// [`ExportError::Unparseable`] where `dc-gcode` cannot read the program.
pub fn units_by_line(
    text: &str,
    post: &PostProcessor,
    units: Units,
) -> Result<Vec<Units>, ExportError> {
    let model = CycleModel {
        units,
        ..CycleModel::of(post)
    };
    let mut out = vec![units; text.lines().count().max(1)];
    for block in parse_program_with(text, &model)? {
        let selected = if block.has('G', 20.0) {
            Units::Inch
        } else if block.has('G', 21.0) {
            Units::Mm
        } else {
            continue;
        };
        if let Some(rest) = out.get_mut(block.line.saturating_sub(1)..) {
            rest.fill(selected);
        }
    }
    Ok(out)
}

/// The units on `line` of a program whose lines have `by_line`.
#[must_use]
pub fn units_at(by_line: &[Units], line: usize) -> Units {
    by_line
        .get(line.saturating_sub(1))
        .or_else(|| by_line.last())
        .copied()
        .unwrap_or(Units::Mm)
}

const fn mm_per_unit(units: Units) -> f64 {
    match units {
        Units::Mm => 1.0,
        Units::Inch => 25.4,
    }
}

/// Where DreamCarve's reading puts the machine, machine mm, for each axis
/// it knows.
pub type Known = [Option<f64>; 3];

/// What DreamCarve's reading says the full simulation will show.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expected {
    /// For each `M0`, its line and where the machine is when it pauses.
    pub stops: Vec<(usize, Known)>,
    /// Where the machine is at the end.
    pub end: Known,
    /// Machine Z of each `G53` lift, mm, with its line.
    pub lifts: Vec<(usize, f64)>,
}

/// Follows `reference` in machine coordinates under [`WORK_OFFSET_MM`]:
/// work positions plus the offset, and a `G53` lift's own machine Z.
#[must_use]
pub fn expected(reference: &[RefEvent], by_line: &[Units]) -> Expected {
    let mut at: Known = [None; 3];
    let mut out = Expected::default();
    for event in reference {
        let k = mm_per_unit(units_at(by_line, event.line()));
        match event {
            RefEvent::Move { to, .. } => {
                for ((known, value), offset) in
                    at.iter_mut().zip([to.x, to.y, to.z]).zip(WORK_OFFSET_MM)
                {
                    if let Some(v) = value {
                        *known = Some(v.mul_add(k, offset));
                    }
                }
            }
            RefEvent::MachineLift { line, z } => {
                at[2] = Some(z * k);
                out.lifts.push((*line, z * k));
            }
            RefEvent::Stop { line } => out.stops.push((*line, at)),
            RefEvent::Dwell { .. } | RefEvent::End { .. } => {}
        }
    }
    out.end = at;
    out
}

/// The moves and lifts of `reference`: what motion-logger can see. It
/// sees no dwell (task waits them out), and no stop or end (task pauses
/// motion, or ends the program, without a move).
#[must_use]
pub fn motion_only(reference: &[RefEvent]) -> Vec<RefEvent> {
    reference
        .iter()
        .filter(|e| matches!(e, RefEvent::Move { .. } | RefEvent::MachineLift { .. }))
        .copied()
        .collect()
}

/// Compares a motion-logger `log` with DreamCarve's `reference`.
///
/// # Errors
///
/// The log line that could not be read.
pub fn compare_log(
    reference: &[RefEvent],
    by_line: &[Units],
    log: &str,
) -> Result<(Comparison, usize), String> {
    let events = parse_motion_log(log).map_err(|e| e.to_string())?;
    let pauses = events.iter().filter(|e| **e == LogEvent::Pause).count();
    let canon = to_canon(&events, |line| units_at(by_line, line));
    Ok((
        compare_with(&motion_only(reference), &canon, LINUXCNC),
        pauses,
    ))
}

fn describe(known: &Known) -> String {
    let axis = |v: Option<f64>| v.map_or_else(|| "?".to_owned(), |v| format!("{v:.4}"));
    format!(
        "({}, {}, {})",
        axis(known[0]),
        axis(known[1]),
        axis(known[2])
    )
}

fn differs(known: &Known, actual: [f64; 3]) -> bool {
    known
        .iter()
        .zip(actual)
        .any(|(k, a)| k.is_some_and(|k| !approx_eq(k, a, LENGTH_TOLERANCE_MM)))
}

/// Checks a run on LinuxCNC's own motion module against `expected`: it
/// completed, paused at every `M0` with the machine where DreamCarve
/// reads it, and ended where DreamCarve reads it. Returns each problem.
#[must_use]
pub fn check_run(run: &ProgramReport, expected: &Expected) -> Vec<String> {
    let mut problems = Vec::new();
    if !run.completed() {
        problems.push(format!(
            "did not run to completion: {}{}",
            if run.timed_out { "timed out; " } else { "" },
            if run.errors.is_empty() {
                "no end recorded".to_owned()
            } else {
                run.errors.join(" | ")
            }
        ));
    }
    if run.pauses.len() == expected.stops.len() {
        for (pause, (line, known)) in run.pauses.iter().zip(&expected.stops) {
            if pause.line >= *line {
                problems.push(format!(
                    "M0 on line {line}: the program had reached line {}",
                    pause.line
                ));
            }
            if differs(known, pause.machine_mm) {
                problems.push(format!(
                    "M0 on line {line}: paused at machine {:?}, DreamCarve reads {}",
                    pause.machine_mm,
                    describe(known)
                ));
            }
        }
    } else {
        problems.push(format!(
            "paused {} time(s), DreamCarve reads {} M0",
            run.pauses.len(),
            expected.stops.len()
        ));
    }
    if let Some(end) = run.end
        && run.completed()
        && differs(&expected.end, end.machine_mm)
    {
        problems.push(format!(
            "ended at machine {:?}, DreamCarve reads {}",
            end.machine_mm,
            describe(&expected.end)
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{End, Pause};
    use dc_project::latest_definition;
    use rs274_motion::reference::reference;

    const PROGRAM: &str = "G21 G90\nG0 X10 Y10 Z-10\nM0\nG1 X20 F600\nG53 G0 Z-5\nM2\n";

    fn expected_of(text: &str) -> Expected {
        let post = latest_definition("linuxcnc")
            .expect("bundled")
            .post_processor()
            .clone();
        let r = reference(text, &post, Units::Mm).expect("reads");
        let by_line = units_by_line(text, &post, Units::Mm).expect("reads");
        expected(&r, &by_line)
    }

    #[test]
    fn follows_work_moves_and_lifts_in_machine_coordinates() {
        let e = expected_of(PROGRAM);
        assert_eq!(e.stops.len(), 1);
        let (line, [x, y, z]) = e.stops[0];
        assert_eq!(line, 3);
        assert!(approx_eq(x.expect("x"), 35.4, 1e-9));
        assert!(approx_eq(y.expect("y"), 60.8, 1e-9));
        assert!(approx_eq(z.expect("z"), -86.2, 1e-9));
        assert_eq!(e.lifts, [(5, -5.0)]);
        assert!(approx_eq(e.end[2].expect("z"), -5.0, 1e-9));
        assert!(approx_eq(e.end[0].expect("x"), 45.4, 1e-9));
    }

    #[test]
    fn inch_lines_are_converted() {
        let e = expected_of("G20 G90\nG0 X1 Y1 Z1\nG53 G0 Z-0.2\nM2\n");
        assert!(approx_eq(e.end[0].expect("x"), 50.8, 1e-9));
        assert!(approx_eq(e.lifts[0].1, -5.08, 1e-9));
    }

    fn run(pause_mm: [f64; 3], end_mm: [f64; 3]) -> ProgramReport {
        ProgramReport {
            file: "p001.ngc".to_owned(),
            pauses: vec![Pause {
                line: 2,
                machine_mm: pause_mm,
            }],
            end: Some(End {
                line: 5,
                machine_mm: end_mm,
            }),
            ..ProgramReport::default()
        }
    }

    #[test]
    fn a_run_where_dreamcarve_reads_it_passes() {
        let e = expected_of(PROGRAM);
        let problems = check_run(&run([35.4, 60.8, -86.2], [45.4, 60.8, -5.0]), &e);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_wrong_pause_or_end_or_a_missing_pause_is_reported() {
        let e = expected_of(PROGRAM);
        let problems = check_run(&run([35.4, 60.8, -80.0], [45.4, 60.8, -71.2]), &e);
        assert_eq!(problems.len(), 2, "{problems:#?}");
        assert!(problems[0].contains("M0 on line 3"));
        assert!(problems[1].contains("ended at machine"));
        let mut unpaused = run([35.4, 60.8, -86.2], [45.4, 60.8, -5.0]);
        unpaused.pauses.clear();
        assert!(check_run(&unpaused, &e)[0].contains("paused 0 time(s)"));
    }
}
