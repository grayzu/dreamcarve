// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads `result.txt`, the record `sim/dreamcarve-ui.py` writes of a
//! session: one tab-separated record per line, as that script's header
//! lists. Positions are machine coordinates, millimetres, as
//! `linuxcncrsh`'s `get abs_act_pos` gives them with `linear_unit_conversion
//! mm`.

/// An `M0` pause, as the UI saw it once the machine had stopped:
/// `linuxcncrsh`'s `get program_status` said `PAUSED`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pause {
    /// `get program_line`: the line of the last move, not the `M0`'s.
    pub line: usize,
    /// Where the machine was, machine mm.
    pub machine_mm: [f64; 3],
}

/// Where a program ended, once the machine had stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct End {
    /// `get program_line`.
    pub line: usize,
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
    /// Errors from task or motion, and commands `linuxcncrsh` refused.
    pub errors: Vec<String>,
    /// The UI aborted it after its timeout.
    pub timed_out: bool,
    /// Where it ended.
    pub end: Option<End>,
}

impl ProgramReport {
    /// It ran to its end: no error, no refused command, no timeout.
    #[must_use]
    pub fn completed(&self) -> bool {
        self.errors.is_empty() && !self.timed_out && self.end.is_some()
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
        let line_no = |k: usize| -> Result<usize, ReportError> {
            rest.get(k)
                .and_then(|f| f.trim().parse().ok())
                .ok_or_else(bad)
        };
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
            ("HOMED", _) => report.homed = line_no(0)? == 1,
            ("PROGRAM", _) => report.programs.push(ProgramReport {
                file: text_field(),
                ..ProgramReport::default()
            }),
            ("ERROR", None) => report.startup_errors.push(text_field()),
            ("MESSAGE", None) => {}
            ("ERROR", Some(p)) => p.errors.push(text_field()),
            ("MESSAGE", Some(p)) => p.messages.push(text_field()),
            ("PAUSE", Some(p)) => p.pauses.push(Pause {
                line: line_no(0)?,
                machine_mm: position(1)?,
            }),
            ("TIMEOUT", Some(p)) => p.timed_out = true,
            ("END", Some(p)) => {
                p.end = Some(End {
                    line: line_no(0)?,
                    machine_mm: position(1)?,
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

    /// A record the UI left with LinuxCNC 2.9.4: the pause program, then
    /// a lift beyond Z's travel under the short-Z machine.
    const RECORD: &str = "HOMED\t1
PROGRAM\tp001.ngc
MESSAGE\tTurn the router OFF.
PAUSE\t2\t35.400000\t60.800000\t-86.200000
END\t5\t45.400000\t60.800000\t-5.000000
PROGRAM\tp002.ngc
ERROR\tLinear move on line 5 would exceed Z's positive limit
END\t4\t35.650000\t60.800000\t-86.200000
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
        assert_eq!(ok.pauses[0].line, 2);
        assert_eq!(ok.messages, ["Turn the router OFF."]);
        assert_eq!(ok.end.map(|e| e.line), Some(5));
        assert!(!refused.completed());
        assert!(refused.errors[0].contains("would exceed Z's positive limit"));
    }

    #[test]
    fn an_unknown_record_is_an_error() {
        let e = parse_report("HOMED\t1\nWHAT\t3\n").expect_err("refuses");
        assert_eq!(e.line, 2);
        assert!(parse_report("PROGRAM\ta\nPAUSE\t2\tx\n").is_err());
    }
}
