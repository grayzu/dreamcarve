// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads the log `motion-logger` writes: every command LinuxCNC's task
//! sends to motion. The moves are what matter here:
//!
//! ```text
//! SET_LINE x=128.575, y=105.8, z=-75.2, a=0, ..., id=13, motion_type=1, vel=10000, ...
//! SET_CIRCLE:
//!     pos: x=32.225, y=105.8, z=-80.2, a=0, ...
//!     center: x=80.4, y=105.8, z=-80.2
//!     normal: x=0, y=0, z=1
//!     id=16, motion_type=3, vel=25, ini_maxvel=6459.16, acc=1e+06, turn=-1
//! PAUSE
//! RESUME
//! ```
//!
//! This is the format of LinuxCNC 2.9.4's `src/emc/motion-logger/motion-logger.c`,
//! read from the log, not from a specification. Positions are machine
//! coordinates in the INI's units (millimetres here), printed with `%.6g`:
//! six significant digits, so three decimals below 1000 mm. `id` is the
//! program's 1-based source line. `motion_type` is 1 for a traverse, 2
//! for a feed and 3 for an arc. `vel` is the requested speed in units per
//! second. An arc in the XY plane has normal +Z, and `turn` is -1 for
//! `G2` and 0 or more (extra full turns) for `G3`.

use dc_gcode::parse::MotionKind;
use dc_project::Units;
use rs274_motion::canon::{CanonEvent, CanonMove};

use crate::WORK_OFFSET_MM;

/// One move task sent to motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoggedMove {
    /// The 1-based source line.
    pub line: usize,
    /// The motion mode.
    pub kind: MotionKind,
    /// The end, machine mm.
    pub machine_mm: [f64; 3],
    /// For an arc, its centre X and Y, machine mm.
    pub centre_mm: Option<[f64; 2]>,
    /// The requested speed, mm per minute.
    pub feed_mm_min: f64,
}

/// One command in the log that this tool reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogEvent {
    /// `SET_LINE` or `SET_CIRCLE`.
    Move(LoggedMove),
    /// `PAUSE`: task paused motion, as at an `M0`.
    Pause,
    /// `RESUME`.
    Resume,
}

/// A log line that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("motion log line {line}: {why}: `{text}`")]
pub struct LogError {
    /// 1-based line.
    pub line: usize,
    /// What was wrong.
    pub why: &'static str,
    /// The line.
    pub text: String,
}

/// `key=value` pairs of one log line.
fn fields(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.split(',').filter_map(|f| {
        let (k, v) = f.split_once('=')?;
        Some((k.trim(), v.trim()))
    })
}

fn number(text: &str, key: &str) -> Option<f64> {
    fields(text)
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse().ok())
}

fn xyz(text: &str) -> Option<[f64; 3]> {
    Some([number(text, "x")?, number(text, "y")?, number(text, "z")?])
}

/// Reads `log` into its moves, pauses and resumes, in order.
///
/// # Errors
///
/// [`LogError`] for a move it cannot read, a motion type other than
/// traverse, feed or arc, or an arc outside the XY plane.
pub fn parse_motion_log(log: &str) -> Result<Vec<LogEvent>, LogError> {
    let lines: Vec<&str> = log.lines().collect();
    let mut events = Vec::new();
    let mut k = 0;
    while let Some(&text) = lines.get(k) {
        let bad = |why| LogError {
            line: k + 1,
            why,
            text: text.to_owned(),
        };
        if text == "PAUSE" {
            events.push(LogEvent::Pause);
        } else if text == "RESUME" {
            events.push(LogEvent::Resume);
        } else if let Some(rest) = text.strip_prefix("SET_LINE ") {
            let machine_mm = xyz(rest).ok_or_else(|| bad("no end point"))?;
            let (line, kind, feed_mm_min) = motion(rest).ok_or_else(|| bad("no id or speed"))?;
            let kind = match kind {
                1 => MotionKind::Rapid,
                2 => MotionKind::Linear,
                _ => return Err(bad("a line that is neither a traverse nor a feed")),
            };
            events.push(LogEvent::Move(LoggedMove {
                line,
                kind,
                machine_mm,
                centre_mm: None,
                feed_mm_min,
            }));
        } else if text == "SET_CIRCLE:" {
            let part = |n: usize, label: &str| {
                lines
                    .get(k + n)
                    .and_then(|l| l.trim().strip_prefix(label))
                    .map(str::to_owned)
            };
            let end = part(1, "pos:").and_then(|p| xyz(&p));
            let centre = part(2, "center:").and_then(|p| xyz(&p));
            let normal = part(3, "normal:").and_then(|p| xyz(&p));
            let tail = lines.get(k + 4).map(|l| l.trim()).unwrap_or("");
            let (Some(machine_mm), Some(centre), Some(normal), Some((line, 3, feed_mm_min))) =
                (end, centre, normal, motion(tail))
            else {
                return Err(bad("an arc that cannot be read"));
            };
            let turn = number(tail, "turn").ok_or_else(|| bad("an arc with no turn"))?;
            let up = match normal {
                [x, y, z] if x == 0.0 && y == 0.0 && z > 0.0 => true,
                [x, y, z] if x == 0.0 && y == 0.0 && z < 0.0 => false,
                _ => return Err(bad("an arc outside the XY plane")),
            };
            let kind = if (turn >= 0.0) == up {
                MotionKind::ArcCcw
            } else {
                MotionKind::ArcCw
            };
            events.push(LogEvent::Move(LoggedMove {
                line,
                kind,
                machine_mm,
                centre_mm: Some([centre[0], centre[1]]),
                feed_mm_min,
            }));
            k += 4;
        }
        k += 1;
    }
    Ok(events)
}

/// `id`, `motion_type` and `vel` (as mm per minute) of a move line.
fn motion(text: &str) -> Option<(usize, u8, f64)> {
    let id = number(text, "id")?;
    let kind = number(text, "motion_type")?;
    let vel = number(text, "vel")?;
    // An id is a source line: a small whole number.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (id, kind) = (id as usize, kind as u8);
    Some((id, kind, vel * 60.0))
}

/// Millimetres per program unit.
const fn mm_per_unit(units: Units) -> f64 {
    match units {
        Units::Mm => 1.0,
        Units::Inch => 25.4,
    }
}

/// The logged moves as `rs274-motion`'s canonical events, so its
/// comparison can line them up with DreamCarve's reading: work
/// coordinates (machine less [`WORK_OFFSET_MM`]) in the units
/// `units_at(line)` gives for the move's source line. Pauses and resumes
/// are left out; [`crate::check`] checks them from the UI's record.
#[must_use]
pub fn to_canon(events: &[LogEvent], units_at: impl Fn(usize) -> Units) -> Vec<CanonEvent> {
    events
        .iter()
        .filter_map(|e| match e {
            LogEvent::Move(m) => Some(m),
            LogEvent::Pause | LogEvent::Resume => None,
        })
        .map(|m| {
            let units = units_at(m.line);
            let k = mm_per_unit(units);
            let [x, y, z] = m.machine_mm;
            let [ox, oy, oz] = WORK_OFFSET_MM;
            CanonEvent::Move(CanonMove {
                line: Some(m.line),
                kind: m.kind,
                to: [(x - ox) / k, (y - oy) / k, (z - oz) / k],
                feed: m.feed_mm_min / k,
                centre: m
                    .centre_mm
                    .map(|[x, y]| [(x - WORK_OFFSET_MM[0]) / k, (y - WORK_OFFSET_MM[1]) / k]),
                offset: WORK_OFFSET_MM.map(|o| o / k),
                units,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    /// From LinuxCNC 2.9.4's motion-logger, for the program
    ///
    /// ```text
    /// G21 G90 G91.1
    /// G0 X10 Y0 Z1
    /// G3 X0 Y10 I-10 J0 F600
    /// G2 X10 Y0 I0 J-10
    /// G3 X10 Y0 I-10 J0
    /// G20
    /// G1 X1 Y1 F10
    /// M0
    /// G0 X2
    /// M2
    /// ```
    ///
    /// under a G54 offset of (25.4, 50.8, -76.2) mm, from its first move
    /// on.
    const LOG: &str = "\
SET_SPINDLESYNC sync=0.000000, flags=0x00000000
SET_LINE x=35.4, y=50.8, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0, id=2, motion_type=1, vel=12953.5, ini_maxvel=12953.5, acc=1.29535e+06, turn=-1
SET_CIRCLE:
    pos: x=25.4, y=60.8, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0
    center: x=25.4, y=50.8, z=-75.2
    normal: x=0, y=0, z=1
    id=3, motion_type=3, vel=10, ini_maxvel=2942.83, acc=1e+06, turn=0
SET_CIRCLE:
    pos: x=35.4, y=50.8, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0
    center: x=25.4, y=50.8, z=-75.2
    normal: x=0, y=0, z=1
    id=4, motion_type=3, vel=10, ini_maxvel=2942.83, acc=1e+06, turn=-1
SET_CIRCLE:
    pos: x=35.4, y=50.8, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0
    center: x=25.4, y=50.8, z=-75.2
    normal: x=0, y=0, z=1
    id=5, motion_type=3, vel=10, ini_maxvel=2942.83, acc=1e+06, turn=0
SET_LINE x=50.8, y=76.2, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0, id=7, motion_type=2, vel=4.23333, ini_maxvel=11694.4, acc=1.16944e+06, turn=-1
PAUSE
RESUME
SET_LINE x=76.2, y=76.2, z=-75.2, a=0, b=0, c=0, u=0, v=0, w=0, id=9, motion_type=1, vel=10000, ini_maxvel=10000, acc=1e+06, turn=-1
SPINDLE_OFF
";

    #[test]
    fn reads_motion_logger_lines() {
        let events = parse_motion_log(LOG).expect("reads");
        let moves: Vec<LoggedMove> = events
            .iter()
            .filter_map(|e| match e {
                LogEvent::Move(m) => Some(*m),
                _ => None,
            })
            .collect();
        let kinds: Vec<(usize, MotionKind)> = moves.iter().map(|m| (m.line, m.kind)).collect();
        assert_eq!(
            kinds,
            [
                (2, MotionKind::Rapid),
                (3, MotionKind::ArcCcw),
                (4, MotionKind::ArcCw),
                (5, MotionKind::ArcCcw),
                (7, MotionKind::Linear),
                (9, MotionKind::Rapid),
            ]
        );
        for (got, want) in moves[1].machine_mm.into_iter().zip([25.4, 60.8, -75.2]) {
            assert_approx_eq!(got, want, 1e-9);
        }
        let [cx, cy] = moves[1].centre_mm.expect("an arc has a centre");
        assert_approx_eq!(cx, 25.4, 1e-9);
        assert_approx_eq!(cy, 50.8, 1e-9);
        assert_approx_eq!(moves[1].feed_mm_min, 600.0, 1e-9);
        assert_eq!(events.iter().filter(|e| **e == LogEvent::Pause).count(), 1);
    }

    #[test]
    fn work_coordinates_are_machine_less_the_offset_in_program_units() {
        let events = parse_motion_log(LOG).expect("reads");
        let canon = to_canon(
            &events,
            |line| if line >= 6 { Units::Inch } else { Units::Mm },
        );
        let CanonEvent::Move(arc) = canon[1] else {
            panic!("{canon:?}");
        };
        assert_approx_eq!(arc.to[0], 0.0, 1e-9);
        assert_approx_eq!(arc.to[1], 10.0, 1e-9);
        assert_approx_eq!(arc.to[2], 1.0, 1e-9);
        let CanonEvent::Move(inch) = canon[4] else {
            panic!("{canon:?}");
        };
        assert_approx_eq!(inch.to[0], 1.0, 1e-9);
        assert_approx_eq!(inch.to[1], 1.0, 1e-9);
        assert_approx_eq!(inch.feed, 10.0, 1e-4);
        assert_approx_eq!(inch.offset[2], -3.0, 1e-9);
        assert_eq!(canon.len(), 6, "pauses are left out");
    }

    #[test]
    fn an_arc_outside_xy_or_an_unknown_move_is_an_error() {
        let yz = LOG.replace(
            "normal: x=0, y=0, z=1\n    id=3",
            "normal: x=1, y=0, z=0\n    id=3",
        );
        assert!(parse_motion_log(&yz).is_err());
        let probe = LOG.replace("motion_type=2", "motion_type=5");
        assert!(parse_motion_log(&probe).is_err());
    }
}
