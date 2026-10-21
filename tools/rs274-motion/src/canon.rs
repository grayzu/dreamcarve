// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads the canonical machining calls `rs274 -g` prints on standard
//! output into moves, dwells and stops.
//!
//! Each line is a counter, the `N` of the block the call comes from
//! (`N.....` for none), and the call, as `src/emc/sai/saicanon.cc` prints
//! it:
//!
//! ```text
//!    26 N16    STRAIGHT_FEED(103.9890, 73.6420, -4.0000, 0.0000, 0.0000, 0.0000)
//!    28 N18    ARC_FEED(6.8250, 55.0000, 55.0000, 55.0000, -1, -4.0000, 0.0000, 0.0000, 0.0000)
//! ```
//!
//! Positions are work coordinates in the program's units; the work offset
//! is printed separately (`SET_G5X_OFFSET`, `SET_G92_OFFSET`), in the units
//! in force when it is printed. `ARC_FEED` is `(first_end, second_end,
//! first_axis, second_axis, rotation, axis_end_point, a, b, c)`: in the
//! XY plane the end X and Y, the centre X and Y, the turn (negative is
//! clockwise) and the end Z. A long number (`N123456`) runs into the call
//! name; the reader allows for that.

use dc_gcode::parse::MotionKind;
use dc_project::Units;

/// One move `rs274` makes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanonMove {
    /// The source line, from the block's `N` word; `None` for a call made
    /// outside any block.
    pub line: Option<usize>,
    /// The motion mode.
    pub kind: MotionKind,
    /// The end, work coordinates, program units.
    pub to: [f64; 3],
    /// The feed in force, program units per minute (rapids too).
    pub feed: f64,
    /// For an arc, the centre X and Y, work coordinates, program units.
    pub centre: Option<[f64; 2]>,
    /// The machine position of work zero, program units: the work (`G5x`)
    /// and `G92` offsets. Work position plus this is machine position.
    pub offset: [f64; 3],
    /// The units in force.
    pub units: Units,
}

/// One thing `rs274` does that the comparison checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanonEvent {
    /// A move.
    Move(CanonMove),
    /// `DWELL(seconds)`.
    Dwell {
        /// The source line.
        line: Option<usize>,
        /// How long, seconds.
        seconds: f64,
    },
    /// `PROGRAM_STOP()`, for `M0`.
    Stop {
        /// The source line.
        line: Option<usize>,
    },
    /// `PROGRAM_END()`, for `M2` and `M30`.
    End {
        /// The source line.
        line: Option<usize>,
    },
}

impl CanonEvent {
    /// The source line of the call.
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        match self {
            Self::Move(m) => m.line,
            Self::Dwell { line, .. } | Self::Stop { line } | Self::End { line } => *line,
        }
    }
}

/// Why `rs274`'s output could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonError {
    /// A call this reader needs had arguments it could not read.
    #[error("rs274 output line {line}: cannot read `{text}`")]
    Malformed {
        /// 1-based line of the output.
        line: usize,
        /// The line.
        text: String,
    },
    /// An arc outside the XY plane; DreamCarve writes only `G17` arcs.
    #[error("rs274 output line {line}: arc outside the XY plane")]
    Plane {
        /// 1-based line of the output.
        line: usize,
    },
}

/// The modal state the reader carries between calls.
struct State {
    feed: f64,
    units: Units,
    xy_plane: bool,
    /// Work and `G92` offsets, millimetres.
    g5x_mm: [f64; 3],
    g92_mm: [f64; 3],
}

/// Reads `stdout`, `rs274 -g`'s standard output, into events in order.
///
/// # Errors
///
/// [`CanonError`] for a call it needs that it cannot read.
pub fn parse_canon(stdout: &str) -> Result<Vec<CanonEvent>, CanonError> {
    let mut state = State {
        feed: 0.0,
        units: Units::Mm,
        xy_plane: true,
        g5x_mm: [0.0; 3],
        g92_mm: [0.0; 3],
    };
    let mut events = Vec::new();
    for (index, text) in stdout.lines().enumerate() {
        let out_line = index + 1;
        let Some((line, name, args)) = split_call(text) else {
            continue;
        };
        let malformed = || CanonError::Malformed {
            line: out_line,
            text: text.to_owned(),
        };
        let numbers = || -> Result<Vec<f64>, CanonError> {
            args.split(',')
                .map(|a| a.trim().parse::<f64>().map_err(|_| malformed()))
                .collect()
        };
        let mm = mm_per_unit(state.units);
        match name {
            "USE_LENGTH_UNITS" => {
                state.units = match args.trim() {
                    "CANON_UNITS_MM" => Units::Mm,
                    "CANON_UNITS_INCHES" => Units::Inch,
                    _ => return Err(malformed()),
                };
            }
            "SELECT_PLANE" => state.xy_plane = args.trim() == "CANON_PLANE_XY",
            "SET_FEED_RATE" => {
                state.feed = *numbers()?.first().ok_or_else(malformed)?;
            }
            "SET_G5X_OFFSET" => {
                // (index, x, y, z, a, b, c)
                let [x, y, z] = three(&numbers()?, 1).ok_or_else(malformed)?;
                state.g5x_mm = [x * mm, y * mm, z * mm];
            }
            "SET_G92_OFFSET" => {
                let [x, y, z] = three(&numbers()?, 0).ok_or_else(malformed)?;
                state.g92_mm = [x * mm, y * mm, z * mm];
            }
            "STRAIGHT_TRAVERSE" | "STRAIGHT_FEED" => {
                let to = three(&numbers()?, 0).ok_or_else(malformed)?;
                let kind = if name == "STRAIGHT_TRAVERSE" {
                    MotionKind::Rapid
                } else {
                    MotionKind::Linear
                };
                events.push(CanonEvent::Move(state.make(line, kind, to, None)));
            }
            "ARC_FEED" => {
                if !state.xy_plane {
                    return Err(CanonError::Plane { line: out_line });
                }
                let n = numbers()?;
                let &[end_x, end_y, centre_x, centre_y, turn, end_z, ..] = n.as_slice() else {
                    return Err(malformed());
                };
                let kind = if turn < 0.0 {
                    MotionKind::ArcCw
                } else {
                    MotionKind::ArcCcw
                };
                events.push(CanonEvent::Move(state.make(
                    line,
                    kind,
                    [end_x, end_y, end_z],
                    Some([centre_x, centre_y]),
                )));
            }
            "DWELL" => {
                let seconds = *numbers()?.first().ok_or_else(malformed)?;
                events.push(CanonEvent::Dwell { line, seconds });
            }
            "PROGRAM_STOP" => events.push(CanonEvent::Stop { line }),
            "PROGRAM_END" => events.push(CanonEvent::End { line }),
            _ => {}
        }
    }
    Ok(events)
}

impl State {
    fn make(
        &self,
        line: Option<usize>,
        kind: MotionKind,
        to: [f64; 3],
        centre: Option<[f64; 2]>,
    ) -> CanonMove {
        let per_mm = 1.0 / mm_per_unit(self.units);
        let offset = [
            (self.g5x_mm[0] + self.g92_mm[0]) * per_mm,
            (self.g5x_mm[1] + self.g92_mm[1]) * per_mm,
            (self.g5x_mm[2] + self.g92_mm[2]) * per_mm,
        ];
        CanonMove {
            line,
            kind,
            to,
            feed: self.feed,
            centre,
            offset,
            units: self.units,
        }
    }
}

/// Three numbers of `n` from `start`.
fn three(n: &[f64], start: usize) -> Option<[f64; 3]> {
    match n.get(start..start + 3)? {
        &[a, b, c] => Some([a, b, c]),
        _ => None,
    }
}

/// Millimetres per program unit.
const fn mm_per_unit(units: Units) -> f64 {
    match units {
        Units::Mm => 1.0,
        Units::Inch => 25.4,
    }
}

/// A line's source line, call name and arguments, or `None` if it is not
/// a call.
fn split_call(text: &str) -> Option<(Option<usize>, &str, &str)> {
    let rest = text.trim_start();
    let counter = rest.find(|c: char| !c.is_ascii_digit())?;
    if counter == 0 {
        return None;
    }
    let rest = rest.get(counter..)?.trim_start().strip_prefix('N')?;
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let label = rest.get(..end)?;
    let line = label.parse::<usize>().ok();
    let call = rest.get(end..)?.trim();
    let open = call.find('(')?;
    let close = call.rfind(')')?;
    let name = call.get(..open)?;
    let args = call.get(open + 1..close)?;
    Some((line, name, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    fn assert_near(a: [f64; 3], b: [f64; 3]) {
        for (a, b) in a.into_iter().zip(b) {
            assert_approx_eq!(a, b, 1e-9);
        }
    }

    /// Captured from `rs274 -g` 2.9.4 (Debian 13) on a probe program with a
    /// G54 offset of (1, 2, -3) inches in its parameter file.
    const CAPTURED: &str = "\
    1 N..... USE_LENGTH_UNITS(CANON_UNITS_MM)
    2 N..... SET_G5X_OFFSET(1, 25.4000, 50.8000, -76.2000, 0.0000, 0.0000, 0.0000)
    3 N..... SET_G92_OFFSET(0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000)
    7 N1     COMMENT(\"comment, with (parens)\")
   14 N4     STRAIGHT_TRAVERSE(-25.4000, -50.8000, 5.0000, 0.0000, 0.0000, 0.0000)
   17 N7     SET_FEED_RATE(300.0000)
   21 N7     STRAIGHT_FEED(20.0000, 20.0000, -3.0000, 0.0000, 0.0000, 0.0000)
   28 N18    ARC_FEED(6.8250, 55.0000, 55.0000, 55.0000, -1, -4.0000, 0.0000, 0.0000, 0.0000)
   39 N10    DWELL(0.5000)
   44 N12    PROGRAM_STOP()
   65 N123456STRAIGHT_TRAVERSE(0.0000, 0.0000, 1.0000, 0.0000, 0.0000, 0.0000)
   73 N18    PROGRAM_END()
   74 N..... ON_RESET()
";

    #[test]
    fn reads_moves_dwells_and_stops_with_their_source_lines() {
        let events = parse_canon(CAPTURED).expect("reads");
        assert_eq!(events.len(), 7, "{events:#?}");
        let CanonEvent::Move(rapid) = events[0] else {
            panic!("{:?}", events[0]);
        };
        assert_eq!(rapid.line, Some(4));
        assert_eq!(rapid.kind, MotionKind::Rapid);
        assert_near(rapid.to, [-25.4, -50.8, 5.0]);
        assert_near(rapid.offset, [25.4, 50.8, -76.2]);
        let CanonEvent::Move(feed) = events[1] else {
            panic!("{:?}", events[1]);
        };
        assert_eq!((feed.line, feed.kind), (Some(7), MotionKind::Linear));
        assert_approx_eq!(feed.feed, 300.0, 1e-9);
        let CanonEvent::Move(arc) = events[2] else {
            panic!("{:?}", events[2]);
        };
        assert_eq!(arc.kind, MotionKind::ArcCw);
        assert_near(arc.to, [6.825, 55.0, -4.0]);
        let [cx, cy] = arc.centre.expect("an arc has a centre");
        assert_near([cx, cy, 0.0], [55.0, 55.0, 0.0]);
        let CanonEvent::Dwell { line, seconds } = events[3] else {
            panic!("{:?}", events[3]);
        };
        assert_eq!(line, Some(10));
        assert_approx_eq!(seconds, 0.5, 1e-9);
        assert_eq!(events[4], CanonEvent::Stop { line: Some(12) });
        assert_eq!(
            events[5].line(),
            Some(123_456),
            "a long N runs into the name"
        );
        assert_eq!(events[6], CanonEvent::End { line: Some(18) });
    }

    /// An inch program's moves carry the offset in inches, whatever units
    /// it was printed in.
    #[test]
    fn offset_follows_the_units_in_force() {
        let text = "\
    1 N..... SET_G5X_OFFSET(1, 0.0000, 0.0000, -76.2000, 0.0000, 0.0000, 0.0000)
    2 N3     USE_LENGTH_UNITS(CANON_UNITS_INCHES)
    3 N4     STRAIGHT_TRAVERSE(0.0000, 0.0000, 1.0000, 0.0000, 0.0000, 0.0000)
";
        let events = parse_canon(text).expect("reads");
        let CanonEvent::Move(m) = events[0] else {
            panic!("{events:?}");
        };
        assert_approx_eq!(m.offset[2], -3.0, 1e-12);
        assert_eq!(m.units, Units::Inch);
    }

    #[test]
    fn rejects_a_move_it_cannot_read() {
        let text = "    1 N2     STRAIGHT_FEED(1.0, x, 2.0)\n";
        assert!(matches!(
            parse_canon(text),
            Err(CanonError::Malformed { line: 1, .. })
        ));
    }
}
