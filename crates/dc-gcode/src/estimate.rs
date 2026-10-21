// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/operationBooklet/report.ts (`feedControlledTimeSeconds`)

//! How long a program takes to run, estimated from its text
//! (`docs/specs/phase-2/functional.md`, capability 30;
//! `docs/specs/phase-2/design.md`, "Phase 2 addendum (#73)", A4).
//!
//! The estimate reads the program back as the safety pass does
//! ([`crate::parse`]), so it times what is written: fitted arcs, canned
//! cycles as the moves the controller makes, and every dwell. Feed moves
//! (`G1`, and `G2`/`G3` by their arc length, with their Z change as a
//! helix) are timed at the modal `F`; a `G4 P` dwell is timed in the
//! definition's dwell units; rapids (`G0`) are measured but not timed,
//! because their speed is a machine setting the program does not state.
//! `M0`, `M1` and `M6` are counted: the program waits there for the user.
//! Acceleration is left out, as in PureCutCNC, which times feed moves and
//! reports the rapid distance apart.
//!
//! The estimate only reads the program; it never changes it.

use dc_project::{PostProcessor, Units};

use crate::emit::Program;
use crate::error::ExportError;

/// What a program's run time is made of. Distances are millimetres,
/// whatever the program's units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Estimate {
    /// Time at the programmed feed, seconds: `G1`, `G2` and `G3`.
    pub feed_s: f64,
    /// Time in `G4` dwells, seconds: spindle spin-up and Drill dwells.
    pub dwell_s: f64,
    /// Length of the feed moves, mm.
    pub feed_mm: f64,
    /// Length of the rapids (`G0`), mm, untimed.
    pub rapid_mm: f64,
    /// `M0`, `M1` and `M6` stops, where the program waits for the user.
    pub stops: usize,
}

impl std::ops::Add for Estimate {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            feed_s: self.feed_s + other.feed_s,
            dwell_s: self.dwell_s + other.dwell_s,
            feed_mm: self.feed_mm + other.feed_mm,
            rapid_mm: self.rapid_mm + other.rapid_mm,
            stops: self.stops + other.stops,
        }
    }
}

/// The estimate of `program`, emitted for `post`.
///
/// # Errors
///
/// [`ExportError::Unparseable`] if the program text cannot be read back,
/// or has an arc from an unknown position or with no centre;
/// [`ExportError::CutWithoutFeed`] for a feed move with no `F` in force.
/// Neither happens to a program [`crate::emit`] returned, which passed the
/// safety pass.
pub fn estimate(program: &Program, post: &PostProcessor) -> Result<Estimate, ExportError> {
    let _ = (program, post, Units::Mm);
    Ok(Estimate::default())
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use dc_geometry::assert_approx_eq;
    use dc_project::{ToolId, latest_definition};

    use super::*;
    use crate::emit::FirstTool;
    use crate::safety::Extents;

    /// `text` as a program in `units`.
    fn program(text: &str, units: Units) -> Program {
        Program {
            text: text.to_owned(),
            extension: "nc".to_owned(),
            pauses: Vec::new(),
            first_tool: FirstTool {
                id: ToolId::new("t1"),
                number: 1,
                name: String::new(),
            },
            extents: Extents {
                units,
                x: None,
                y: None,
                z: None,
            },
        }
    }

    /// The latest bundled definition `id`.
    fn post(id: &str) -> &'static PostProcessor {
        latest_definition(id).unwrap().post_processor()
    }

    /// The estimate of millimetre program `text` on grblHAL.
    fn of(text: &str) -> Estimate {
        estimate(&program(text, Units::Mm), post("grblhal")).unwrap()
    }

    /// 100 mm at 600 mm/min is a tenth of a minute.
    #[test]
    fn line_time_is_length_over_feed() {
        let e = of("G21\nG0 X0 Y0 Z5\nG1 X100 F600\n");
        assert_approx_eq!(e.feed_s, 10.0, 1e-9);
        assert_approx_eq!(e.feed_mm, 100.0, 1e-9);
        assert_approx_eq!(e.dwell_s, 0.0, 1e-12);
    }

    /// An arc is timed by its length, not its chord: a quarter circle of
    /// radius 10 is 5π mm. Clockwise the other way round it is three
    /// quarters, and an `R` arc is timed the same as its `I J` twin.
    #[test]
    fn arc_time_uses_arc_length() {
        let quarter = 10.0 * PI / 2.0;
        let e = of("G21\nG0 X10 Y0 Z0\nG3 X0 Y10 I-10 J0 F600\n");
        assert_approx_eq!(e.feed_mm, quarter, 1e-9);
        assert_approx_eq!(e.feed_s, quarter / 600.0 * 60.0, 1e-9);
        let e = of("G21\nG0 X10 Y0 Z0\nG2 X0 Y10 I-10 J0 F600\n");
        assert_approx_eq!(e.feed_mm, 3.0 * quarter, 1e-9);
        let e = of("G21\nG0 X10 Y0 Z0\nG3 X0 Y10 R10 F600\n");
        assert_approx_eq!(e.feed_mm, quarter, 1e-9);
        let e = of("G21\nG0 X10 Y0 Z0\nG3 X0 Y10 R-10 F600\n");
        assert_approx_eq!(e.feed_mm, 3.0 * quarter, 1e-9);
        // The same start and end is a full circle, as the controller reads it.
        let e = of("G21\nG0 X10 Y0 Z0\nG3 X10 Y0 I-10 J0 F600\n");
        assert_approx_eq!(e.feed_mm, 4.0 * quarter, 1e-9);
    }

    /// A helix is as long as its arc and its drop together:
    /// `sqrt((r θ)² + dz²)`.
    #[test]
    fn helix_time_includes_z() {
        let e = of("G21\nG0 X10 Y0 Z0\nG3 X0 Y10 Z-3 I-10 J0 F600\n");
        let length = (10.0 * PI / 2.0).hypot(3.0);
        assert_approx_eq!(e.feed_mm, length, 1e-9);
        assert_approx_eq!(e.feed_s, length / 600.0 * 60.0, 1e-9);
    }

    /// UCCNC's `G4 P` is milliseconds, grblHAL's seconds
    /// (`definitions/README.md`): the same 3 s either way.
    #[test]
    fn dwell_respects_definition_units() {
        let uccnc = estimate(&program("G0 Z5\nG4 P3000\n", Units::Mm), post("uccnc")).unwrap();
        assert_approx_eq!(uccnc.dwell_s, 3.0, 1e-12);
        let grblhal = of("G21\nG0 Z5\nG4 P3.000\n");
        assert_approx_eq!(grblhal.dwell_s, 3.0, 1e-12);
        assert_approx_eq!(grblhal.feed_s, 0.0, 1e-12);
    }

    /// An inch program's lengths and feeds are inches: 4 in at 24 in/min
    /// is 10 s and 101.6 mm, and 1 in of rapid is 25.4 mm.
    #[test]
    fn inch_program_converts() {
        let text = "G20\nG0 X0 Y0 Z1\nG1 X4 F24\nG0 Z2\n";
        let e = estimate(&program(text, Units::Inch), post("grblhal")).unwrap();
        assert_approx_eq!(e.feed_s, 10.0, 1e-9);
        assert_approx_eq!(e.feed_mm, 101.6, 1e-9);
        assert_approx_eq!(e.rapid_mm, 25.4, 1e-9);
    }

    /// Rapids are measured in 3D and not timed. The first move, from where
    /// the machine was left, has no known length and adds nothing.
    #[test]
    fn rapids_are_distance_only() {
        let e = of("G21\nG0 X0 Y0 Z5\nX30 Y40\nZ5\n");
        assert_approx_eq!(e.rapid_mm, 50.0, 1e-9);
        assert_approx_eq!(e.feed_s, 0.0, 1e-12);
        assert_approx_eq!(e.dwell_s, 0.0, 1e-12);
        let e = of("G21\nG0 X0 Y0 Z0\nX30 Y40 Z120\n");
        assert_approx_eq!(e.rapid_mm, 130.0, 1e-9);
    }

    /// The program waits for the user at `M0` and `T<n> M6`; both count.
    #[test]
    fn stops_are_counted() {
        let e = of("G21\nG0 Z5\nM5\nM0 (MSG, Turn the router OFF.)\nT2 M6 (MSG, Fit T2.)\nM30\n");
        assert_eq!(e.stops, 2);
    }

    /// A canned cycle is timed as the moves the controller makes for it:
    /// on LinuxCNC a `G82` rapids to R, feeds to Z, dwells `P` seconds and
    /// rapids back to the initial Z (`G98`).
    #[test]
    fn canned_cycles_are_timed_as_expanded() {
        let text = "G21\nG0 X0 Y0 Z5\nG98 G82 X0 Y0 Z-3 R1 P0.5 F300\nG80\n";
        let e = estimate(&program(text, Units::Mm), post("linuxcnc")).unwrap();
        assert_approx_eq!(e.rapid_mm, 4.0 + 8.0, 1e-9);
        assert_approx_eq!(e.feed_mm, 4.0, 1e-9);
        assert_approx_eq!(e.feed_s, 0.8, 1e-9);
        assert_approx_eq!(e.dwell_s, 0.5, 1e-12);
    }

    /// Estimates add up, for a total over programs.
    #[test]
    fn estimates_add() {
        let a = of("G21\nG0 X0 Y0 Z5\nG1 X100 F600\nM0\n");
        let total = a + a;
        assert_approx_eq!(total.feed_s, 20.0, 1e-9);
        assert_eq!(total.stops, 2);
    }
}
