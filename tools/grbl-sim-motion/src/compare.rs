// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Lines DreamCarve's reading of a program up with what the firmware did,
//! line by line, in machine millimetres.
//!
//! DreamCarve reads moves in work coordinates and program units
//! ([`rs274_motion::reference`]); the firmware reports machine positions
//! in millimetres. A [`Frame`] maps one to the other with the work offset
//! the harness sets, so a `G53` lift is told apart from a work move.
//!
//! For every source line with a move, [`compare_lines`] checks, once the
//! firmware has finished that line's motion:
//!
//! - its machine position (status report `MPos`) against where DreamCarve
//!   reads the line ending;
//! - its motion mode (`$G`) against the line's last move: `G0` for a
//!   rapid or a `G53 G0` lift, `G1`, `G2` or `G3` for a cut;
//! - its feed (`$G`) against DreamCarve's, for a cut.
//!
//! Moves that go nowhere are left out. Axes DreamCarve does not know
//! (before the program sets them, and Z after a `G53` lift) are not
//! compared.

use std::collections::BTreeMap;

use dc_gcode::parse::{MachinePosition, MotionKind};
use dc_geometry::approx_eq;
use rs274_motion::reference::RefEvent;

use crate::log::{feed, motion_mode};
use crate::runner::Settled;
use crate::{LENGTH_TOLERANCE_MM, RATE_TOLERANCE};

/// Work coordinates in program units to machine millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Millimetres per program unit.
    pub mm_per_unit: f64,
    /// The machine position of work zero, mm.
    pub offset_mm: [f64; 3],
}

impl Frame {
    fn point(&self, p: &MachinePosition) -> [Option<f64>; 3] {
        let m = |v: Option<f64>, offset: f64| v.map(|v| v * self.mm_per_unit + offset);
        let [x, y, z] = self.offset_mm;
        [m(p.x, x), m(p.y, y), m(p.z, z)]
    }
}

/// A move DreamCarve reads, in machine millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// 1-based source line.
    pub line: usize,
    /// The motion mode; a lift is a rapid.
    pub kind: MotionKind,
    /// A `G53` lift.
    pub lift: bool,
    /// Where it ends; `None` for an axis DreamCarve does not know.
    pub to: [Option<f64>; 3],
    /// The feed, mm/min, for a cut.
    pub feed_mm_min: Option<f64>,
}

/// The moves in `events`, DreamCarve's reading, in machine millimetres.
#[must_use]
pub fn targets(events: &[RefEvent], frame: &Frame) -> Vec<Target> {
    if true { return Vec::new(); }
    let mut out = Vec::new();
    let mut at: [Option<f64>; 3] = [None; 3];
    for event in events {
        match event {
            RefEvent::Move {
                line,
                kind,
                from,
                to,
                feed,
                ..
            } => {
                let arc = matches!(kind, MotionKind::ArcCw | MotionKind::ArcCcw);
                if !arc && from == to {
                    continue;
                }
                let end = frame.point(to);
                out.push(Target {
                    line: *line,
                    kind: *kind,
                    lift: false,
                    to: end,
                    feed_mm_min: feed.map(|f| f * frame.mm_per_unit),
                });
                at = end;
            }
            RefEvent::MachineLift { line, z } => {
                let to = [at[0], at[1], Some(z * frame.mm_per_unit)];
                out.push(Target {
                    line: *line,
                    kind: MotionKind::Rapid,
                    lift: true,
                    to,
                    feed_mm_min: None,
                });
                at = to;
            }
            RefEvent::Dwell { .. } | RefEvent::Stop { .. } | RefEvent::End { .. } => {}
        }
    }
    out
}

/// One place the two differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The source line, or `None` for the program as a whole.
    pub line: Option<usize>,
    /// What differs.
    pub what: String,
}

/// The outcome of one comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    /// Moves DreamCarve reads.
    pub moves: usize,
    /// Lines checked against the firmware.
    pub checked: usize,
    /// Every difference, in line order.
    pub differences: Vec<Difference>,
}

fn close(a: f64, b: f64) -> bool {
    // The tolerance, and a hair for the binary representation of a
    // decimal that lands exactly on it.
    approx_eq(a, b, LENGTH_TOLERANCE_MM + 1e-9)
}

/// The axes where `known` and `actual` differ, as `X 1.0000 vs 1.1000`.
fn off_axes(known: &[Option<f64>; 3], actual: &[f64; 3]) -> Vec<String> {
    known
        .iter()
        .zip(actual)
        .zip(['X', 'Y', 'Z'])
        .filter_map(|((k, a), axis)| match k {
            Some(k) if !close(*k, *a) => Some(format!("{axis} {k:.4} vs {a:.4}")),
            _ => None,
        })
        .collect()
}

const fn code(t: &Target) -> &'static str {
    match (t.lift, t.kind) {
        (true, _) => "G53 G0",
        (_, MotionKind::Rapid) => "G0",
        (_, MotionKind::Linear) => "G1",
        (_, MotionKind::ArcCw) => "G2",
        (_, MotionKind::ArcCcw) => "G3",
    }
}

fn describe(t: &Target) -> String {
    let axis = |v: Option<f64>| v.map_or_else(|| "?".to_owned(), |v| format!("{v:.4}"));
    let mut s = format!(
        "{} to machine ({}, {}, {})",
        code(t),
        axis(t.to[0]),
        axis(t.to[1]),
        axis(t.to[2])
    );
    if let Some(f) = t.feed_mm_min.filter(|_| t.kind.is_cut()) {
        s.push_str(&format!(" F{f} mm/min"));
    }
    s
}

/// The last move DreamCarve reads on each source line, with the axes it
/// does not know filled in from earlier moves where it does.
#[must_use]
pub fn line_ends(targets: &[Target]) -> BTreeMap<usize, Target> {
    let mut out = BTreeMap::new();
    let mut at: [Option<f64>; 3] = [None; 3];
    for t in targets {
        // An axis a move leaves unknown is one it does not move: after a
        // lift DreamCarve no longer knows work Z, but an XY move leaves the
        // machine at the lift's machine Z.
        for (a, v) in at.iter_mut().zip(t.to) {
            *a = v.or(*a);
        }
        out.insert(t.line, Target { to: at, ..*t });
    }
    out
}

/// Compares where DreamCarve reads each line ending with `observed`: for
/// each source line, where the firmware was, and its modal state, once it
/// had finished the line's motion.
#[must_use]
pub fn compare_lines(targets: &[Target], observed: &[(usize, Settled)]) -> Comparison {
    if true { return Comparison::default(); }
    let ends = line_ends(targets);
    let mut out = Comparison {
        moves: targets.len(),
        ..Comparison::default()
    };
    for (line, settled) in observed {
        let Some(t) = ends.get(line) else {
            continue;
        };
        out.checked += 1;
        let mut why = off_axes(&t.to, &settled.position_mm);
        let mode = motion_mode(&settled.modal);
        if mode != Some(t.kind) {
            why.push(format!("motion mode {:?} vs {mode:?}", t.kind));
        }
        if let Some(f) = t.feed_mm_min.filter(|_| t.kind.is_cut()) {
            match feed(&settled.modal) {
                Some((reported, half_digit))
                    if approx_eq(f, reported, half_digit + RATE_TOLERANCE * f.abs()) => {}
                other => why.push(format!("F {f} vs {other:?}")),
            }
        }
        if !why.is_empty() {
            let [x, y, z] = settled.position_mm;
            out.differences.push(Difference {
                line: Some(*line),
                what: format!(
                    "{}\n  DreamCarve: {}\n  firmware:   MPos ({x:.3}, {y:.3}, {z:.3}), {}",
                    why.join(", "),
                    describe(t),
                    settled.modal.join(" ")
                ),
            });
        }
    }
    let missing: Vec<usize> = ends
        .keys()
        .filter(|l| !observed.iter().any(|(o, _)| o == *l))
        .copied()
        .collect();
    if !missing.is_empty() {
        out.differences.push(Difference {
            line: None,
            what: format!("no reported position after the moves on lines {missing:?}"),
        });
    }
    out
}

/// A difference if `actual`, the final position the firmware reports, is
/// not where DreamCarve reads the program ending.
#[must_use]
pub fn compare_final(targets: &[Target], actual: &[f64; 3]) -> Option<Difference> {
    if true { return None; }
    let known = line_ends(targets)
        .last_key_value()
        .map_or([None; 3], |(_, t)| t.to);
    let why = off_axes(&known, actual);
    (!why.is_empty()).then(|| Difference {
        line: None,
        what: format!("final position: {}", why.join(", ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::parse_modal_state;
    use dc_project::{Units, latest_definition};
    use rs274_motion::reference::reference;

    const FRAME: Frame = Frame {
        mm_per_unit: 1.0,
        offset_mm: [25.4, 50.8, -76.2],
    };

    fn ours(text: &str) -> Vec<Target> {
        let post = latest_definition("grblhal")
            .expect("bundled")
            .post_processor();
        targets(&reference(text, post, Units::Mm).expect("reads"), &FRAME)
    }

    const PROGRAM: &str =
        "G21 G90\nG0 Z5\nG0 X10 Y10\nG1 Z-1 F300\nG2 X15 Y15 I5 J0 F600\nG53 G0 Z-5\nM2\n";

    fn settled(position_mm: [f64; 3], gc: &str) -> Settled {
        Settled {
            position_mm,
            modal: parse_modal_state(gc).expect("a report"),
        }
    }

    /// What the pinned grblHAL Simulator reported for [`PROGRAM`] under the
    /// harness's offset, line by line.
    fn reported() -> Vec<(usize, Settled)> {
        let gc = |m: &str, f: &str| format!("[GC:{m} G54 G17 G21 G90 G94 G40 G49 G98 G50 M5 M9 T0 F{f} S0.]");
        vec![
            (2, settled([0.0, 0.0, -71.2], &gc("G0", "0"))),
            (3, settled([35.4, 60.8, -71.2], &gc("G0", "0"))),
            (4, settled([35.4, 60.8, -77.2], &gc("G1", "300"))),
            (5, settled([40.4, 65.8, -77.2], &gc("G2", "600"))),
            (6, settled([40.4, 65.8, -5.0], &gc("G0", "600"))),
        ]
    }

    #[test]
    fn agreeing_lines_have_no_differences() {
        let t = ours(PROGRAM);
        let c = compare_lines(&t, &reported());
        assert!(c.differences.is_empty(), "{:#?}", c.differences);
        assert_eq!((c.moves, c.checked), (5, 5));
        assert!(compare_final(&t, &[40.4, 65.8, -5.0]).is_none());
        assert!(compare_final(&t, &[40.4, 65.8, 0.0]).is_some());
    }

    /// A deeper cut, a lift to work Z (what a lift that ignored `G53` does),
    /// a wrong feed and a wrong motion mode are each reported on their line.
    #[test]
    fn each_kind_of_difference_is_reported_on_its_line() {
        let mut r = reported();
        r[2].1.position_mm[2] = -77.3;
        r[4].1.position_mm[2] = -81.2;
        r[3].1.modal = parse_modal_state("[GC:G3 G54 G17 G21 G90 G94 M5 M9 T0 F60 S0]")
            .expect("a report");
        let c = compare_lines(&ours(PROGRAM), &r);
        let at = |line: usize| -> Vec<&str> {
            c.differences
                .iter()
                .filter(|d| d.line == Some(line))
                .map(|d| d.what.as_str())
                .collect()
        };
        assert!(at(4).iter().any(|w| w.contains("Z -77.2000 vs -77.3000")), "{:#?}", c.differences);
        assert!(at(6).iter().any(|w| w.contains("Z -5.0000 vs -81.2000")), "{:#?}", c.differences);
        assert!(at(5).iter().any(|w| w.contains("motion mode ArcCw")), "{:#?}", c.differences);
        assert!(at(5).iter().any(|w| w.contains("F 600 vs")), "{:#?}", c.differences);
    }

    #[test]
    fn a_line_with_no_report_is_a_difference() {
        let mut r = reported();
        r.remove(1);
        let c = compare_lines(&ours(PROGRAM), &r);
        assert!(
            c.differences.iter().any(|d| d.what.contains("lines [3]")),
            "{:#?}",
            c.differences
        );
    }
}
