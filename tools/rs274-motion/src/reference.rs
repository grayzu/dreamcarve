// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! DreamCarve's own reading of a program: `dc-gcode`'s parser, the one
//! the safety pass checks (S1-S16), turned into the events the comparison
//! lines up with `rs274`'s.

use dc_gcode::parse::{MachinePosition, MotionKind, parse_program_with};
use dc_gcode::{CycleModel, ExportError};
use dc_project::{DwellUnits, PostProcessor, Units};

/// One thing DreamCarve reads the controller doing on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefEvent {
    /// A move in work coordinates, program units. Axes the program has
    /// not set yet are `None`.
    Move {
        /// 1-based source line.
        line: usize,
        /// The motion mode.
        kind: MotionKind,
        /// Where it starts.
        from: MachinePosition,
        /// Where it ends.
        to: MachinePosition,
        /// The feed for a cut, program units per minute.
        feed: Option<f64>,
        /// For an arc with `I`, `J` from a known start, its centre.
        centre: Option<[f64; 2]>,
    },
    /// `G53 G0 Z<z>`: a rapid to machine Z `z`, program units (S16).
    MachineLift {
        /// 1-based source line.
        line: usize,
        /// Machine Z, program units.
        z: f64,
    },
    /// `G4`, or a `G82`'s dwell.
    Dwell {
        /// 1-based source line.
        line: usize,
        /// How long, seconds.
        seconds: f64,
    },
    /// `M0`.
    Stop {
        /// 1-based source line.
        line: usize,
    },
    /// `M2` or `M30`.
    End {
        /// 1-based source line.
        line: usize,
    },
}

impl RefEvent {
    /// The source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        match self {
            Self::Move { line, .. }
            | Self::MachineLift { line, .. }
            | Self::Dwell { line, .. }
            | Self::Stop { line }
            | Self::End { line } => *line,
        }
    }
}

/// Reads `text`, written with definition `post`, as `dc-gcode` does for
/// the safety pass: canned cycles expanded with the definition's model,
/// lengths in `units` until the program selects some.
///
/// # Errors
///
/// [`ExportError::Unparseable`] where `dc-gcode` cannot read the program.
pub fn reference(
    text: &str,
    post: &PostProcessor,
    units: Units,
) -> Result<Vec<RefEvent>, ExportError> {
    let model = CycleModel {
        units,
        ..CycleModel::of(post)
    };
    let dwell_scale = match post.feed_speed.spindle_on_dwell.units {
        DwellUnits::Seconds => 1.0,
        DwellUnits::Milliseconds => 1e-3,
    };
    let mut units = units;
    let mut events = Vec::new();
    for block in parse_program_with(text, &model)? {
        let line = block.line;
        if block.has('G', 20.0) {
            units = Units::Inch;
        }
        if block.has('G', 21.0) {
            units = Units::Mm;
        }
        // RS274/NGC order of execution: dwell, then motion, then stops
        // (NIST IR 6556, 3.8).
        if block.has('G', 4.0)
            && let Some(p) = block.value('P')
        {
            events.push(RefEvent::Dwell {
                line,
                seconds: p * dwell_scale,
            });
        }
        if let Some(z_mm) = block.machine_lift_mm {
            events.push(RefEvent::MachineLift {
                line,
                z: z_mm / mm_per_unit(units),
            });
        }
        if let Some(m) = block.motion {
            let centre = match (m.kind, m.from.x, m.from.y, m.arc.i, m.arc.j) {
                (MotionKind::ArcCw | MotionKind::ArcCcw, Some(x), Some(y), i, j)
                    if i.is_some() || j.is_some() =>
                {
                    Some([x + i.unwrap_or(0.0), y + j.unwrap_or(0.0)])
                }
                _ => None,
            };
            events.push(RefEvent::Move {
                line,
                kind: m.kind,
                from: m.from,
                to: m.to,
                feed: if m.kind.is_cut() { m.feed } else { None },
                centre,
            });
        }
        if block.has('M', 0.0) {
            events.push(RefEvent::Stop { line });
        }
        if block.has('M', 2.0) || block.has('M', 30.0) {
            events.push(RefEvent::End { line });
        }
    }
    Ok(events)
}

/// Millimetres per program unit.
const fn mm_per_unit(units: Units) -> f64 {
    match units {
        Units::Mm => 1.0,
        Units::Inch => 25.4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;
    use dc_project::latest_definition;

    fn post(id: &str) -> PostProcessor {
        latest_definition(id)
            .expect("bundled")
            .post_processor()
            .clone()
    }

    /// A `G81` line is read as the moves the controller makes, each on
    /// that line (S11), and a `G53` line as a machine-Z lift (S16).
    #[test]
    fn expands_cycles_and_reads_machine_lifts() {
        let text = "G21 G90\nG0 Z5\nX10 Y10\nG98\nG81 X20 Y20 Z-3 R1 F300\nG80\nG53 G0 Z-5\nM2\n";
        let events = reference(text, &post("linuxcnc"), Units::Mm).expect("reads");
        let on_cycle: Vec<&RefEvent> = events.iter().filter(|e| e.line() == 5).collect();
        assert_eq!(
            on_cycle.len(),
            4,
            "XY, down to R, feed, back up: {on_cycle:#?}"
        );
        assert!(
            matches!(events[events.len() - 2], RefEvent::MachineLift { line: 7, z } if (z + 5.0).abs() < 1e-12),
            "{events:#?}"
        );
        assert_eq!(events.last(), Some(&RefEvent::End { line: 8 }));
    }

    /// UCCNC's `P` is milliseconds (definitions README, "UCCNC").
    #[test]
    fn uccnc_dwell_is_read_in_milliseconds() {
        let events = reference("G4 P3000\n", &post("uccnc"), Units::Mm).expect("reads");
        let [RefEvent::Dwell { seconds, .. }] = events.as_slice() else {
            panic!("{events:?}");
        };
        assert_approx_eq!(*seconds, 3.0, 1e-12);
    }

    /// An inch program's machine lift is in inches, as `rs274` prints it.
    #[test]
    fn machine_lift_is_in_program_units() {
        let events = reference("G20\nG53 G0 Z-0.2\n", &post("linuxcnc"), Units::Mm).expect("reads");
        let [RefEvent::MachineLift { z, .. }] = events.as_slice() else {
            panic!("{events:?}");
        };
        assert_approx_eq!(*z, -0.2, 1e-12);
    }

    #[test]
    fn arc_centre_is_start_plus_offsets() {
        let text = "G21 G90 G91.1\nG0 X10 Y0 Z1\nG3 X0 Y10 I-10 J0 F600\n";
        let events = reference(text, &post("generic"), Units::Mm).expect("reads");
        let Some(RefEvent::Move {
            centre: Some([cx, cy]),
            ..
        }) = events.last()
        else {
            panic!("{events:?}");
        };
        assert_approx_eq!(*cx, 0.0, 1e-12);
        assert_approx_eq!(*cy, 0.0, 1e-12);
    }
}
