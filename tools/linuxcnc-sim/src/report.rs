// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads `result.txt`, the record `sim/dreamcarve-ui.py` writes of a
//! session: one tab-separated record per line, as that script's header
//! lists. Positions are machine coordinates, millimetres.

/// `RCS_DONE`, LinuxCNC's `stat.state` once a command finished without
/// error (`RCS_EXEC` is 2, `RCS_ERROR` 3).
pub const RCS_DONE: i32 = 1;

/// An `M0` pause, as the UI saw it once motion had stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pause {
    /// `stat.current_line`: the line of the last motion, not the `M0`'s.
    pub current_line: usize,
    /// `stat.motion_line`: the line motion last executed.
    pub motion_line: usize,
    /// `stat.interp_state` was `INTERP_PAUSED`.
    pub interp_paused: bool,
    /// Where the machine was, machine mm.
    pub machine_mm: [f64; 3],
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct End {
    /// `stat.state`: [`RCS_DONE`] when it ran without error.
    pub rcs_state: i32,
    /// `stat.current_line`.
    pub current_line: usize,
    /// `stat.motion_line`.
    pub motion_line: usize,
    /// Where the machine was, machine mm.
    pub machine_mm: [f64; 3],
}

/// One program's run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgramReport {
    /// Its file name.
    pub file: String,
    /// Every `M0` pause, in order.
    pub pauses: Vec<Pause>,
    /// Operator messages, such as `(MSG, ...)` text.
    pub messages: Vec<String>,
    /// Errors from task or motion.
    pub errors: Vec<String>,
    /// The UI aborted it after its timeout.
    pub timed_out: bool,
    /// How it ended.
    pub end: Option<End>,
}

impl ProgramReport {
    /// It ran to its end: no error, no timeout, and `stat.state` done.
    #[must_use]
    pub fn completed(&self) -> bool {
        self.errors.is_empty()
            && !self.timed_out
            && self.end.is_some_and(|e| e.rcs_state == RCS_DONE)
    }
}

/// A session's record.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionReport {
    /// Every joint was homed before the first program.
    pub homed: bool,
    /// Errors before the first program.
    pub startup_errors: Vec<String>,
    /// Each program, in order.
    pub programs: Vec<ProgramReport>,
    /// The UI got to the end of its list.
    pub done: bool,
}

/// A record that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("result.txt line {line}: cannot read `{text}`")]
pub struct ReportError {
    /// 1-based line.
    pub line: usize,
    /// The line.
    pub text: String,
}

/// Reads `text`, a `result.txt`.
///
/// # Errors
///
/// [`ReportError`] for a record it cannot read.
pub fn parse_report(text: &str) -> Result<SessionReport, ReportError> {
    let mut report = SessionReport::default();
    for (index, line) in text.lines().enumerate() {
        let bad = || ReportError {
            line: index + 1,
            text: line.to_owned(),
        };
        let fields: Vec<&str> = line.split('\t').collect();
        let Some((&head, rest)) = fields.split_first() else {
            continue;
        };
        let int = |k: usize| -> Result<i64, ReportError> {
            rest.get(k)
                .and_then(|f| f.trim().parse().ok())
                .ok_or_else(bad)
        };
        let line_no = |k: usize| int(k).and_then(|v| usize::try_from(v).map_err(|_| bad()));
        let position = |k: usize| -> Result<[f64; 3], ReportError> {
            let mut p = [0.0; 3];
            for (axis, value) in p.iter_mut().enumerate() {
                *value = rest
                    .get(k + axis)
                    .and_then(|f| f.trim().parse().ok())
                    .ok_or_else(bad)?;
            }
            Ok(p)
        };
        let text_field = || rest.join("\t");
        let current = report.programs.last_mut();
        match (head, current) {
            ("", _) => {}
            ("HOMED", _) => report.homed = int(0)? == 1,
            ("PROGRAM", _) => report.programs.push(ProgramReport {
                file: text_field(),
                ..ProgramReport::default()
            }),
            ("ERROR", None) => report.startup_errors.push(text_field()),
            ("MESSAGE", None) => {}
            ("ERROR", Some(p)) => p.errors.push(text_field()),
            ("MESSAGE", Some(p)) => p.messages.push(text_field()),
            ("PAUSE", Some(p)) => p.pauses.push(Pause {
                current_line: line_no(0)?,
                motion_line: line_no(1)?,
                interp_paused: int(2)? == 1,
                machine_mm: position(3)?,
            }),
            ("TIMEOUT", Some(p)) => p.timed_out = true,
            ("END", Some(p)) => {
                p.end = Some(End {
                    rcs_state: i32::try_from(int(0)?).map_err(|_| bad())?,
                    current_line: line_no(1)?,
                    motion_line: line_no(2)?,
                    machine_mm: position(3)?,
                });
            }
            ("DONE", _) => report.done = true,
            _ => return Err(bad()),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record LinuxCNC 2.9.4 left: the pause program, then a lift
    /// beyond Z's travel under the short-Z machine.
    const RECORD: &str = "HOMED\t1
PROGRAM\tp001.ngc
PAUSE\t2\t2\t1\t35.400000000000006\t60.800000000000004\t-86.2
END\t1\t5\t5\t45.4\t60.800000000000004\t-5.0
PROGRAM\tp002.ngc
PAUSE\t2\t2\t1\t35.4\t60.800000000000004\t-86.2
ERROR\tLinear move on line 5 would exceed Z's positive limit
ERROR\tLinear move on line 5 would exceed joint 2's positive limit
ERROR\tinvalid params in linear command
END\t3\t5\t4\t35.65\t60.800000000000004\t-86.2
DONE
";

    #[test]
    fn reads_a_session_record() {
        let r = parse_report(RECORD).expect("reads");
        assert!(r.homed && r.done);
        let [ok, refused] = r.programs.as_slice() else {
            panic!("{r:#?}");
        };
        assert!(ok.completed());
        assert_eq!(ok.pauses.len(), 1);
        assert!(ok.pauses[0].interp_paused);
        assert_eq!(ok.end.map(|e| e.machine_mm[2]), Some(-5.0));
        assert!(!refused.completed());
        assert_eq!(refused.errors.len(), 3);
        assert!(refused.errors[0].contains("would exceed Z's positive limit"));
        assert_eq!(refused.end.map(|e| e.rcs_state), Some(3));
    }

    #[test]
    fn an_unknown_record_is_an_error() {
        let e = parse_report("HOMED\t1\nWHAT\t3\n").expect_err("refuses");
        assert_eq!(e.line, 2);
        assert!(parse_report("PROGRAM\ta\nPAUSE\t2\tx\n").is_err());
    }
}
