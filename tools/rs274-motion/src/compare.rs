// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Lines up DreamCarve's reading of a program with `rs274`'s, source line
//! by source line, and lists every difference.
//!
//! Moves that go nowhere are left out on both sides: a straight move to
//! where the tool already is moves nothing, and DreamCarve and `rs274`
//! need not agree on writing one. Arcs are always kept, since a full
//! circle ends where it starts. Axes DreamCarve does not know yet (before
//! the program sets them, and Z after a `G53` lift) are not compared;
//! `rs274` starts from a known position and prints one.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use dc_gcode::parse::{MachinePosition, MotionKind};
use dc_geometry::approx_eq;

use crate::canon::{CanonEvent, CanonMove};
use crate::reference::RefEvent;
use crate::{LENGTH_TOLERANCE, RATE_TOLERANCE};

/// One place the two readings differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The source line, or `None` for a call `rs274` made outside any.
    pub line: Option<usize>,
    /// What differs.
    pub what: String,
}

/// The outcome of one comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    /// Source lines with at least one event, on either side.
    pub lines: usize,
    /// Moves compared.
    pub moves: usize,
    /// Every difference, in line order.
    pub differences: Vec<Difference>,
}

/// A canonical event and, for a move, where `rs274` was before it.
type Located = (CanonEvent, Option<[f64; 3]>);

/// The other reading: what the report calls it, and how closely its feeds
/// can match DreamCarve's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reader {
    /// The name in the report.
    pub name: &'static str,
    /// The relative tolerance on feeds, which depends on how many digits
    /// the reader prints.
    pub rate_tolerance: f64,
}

impl Reader {
    /// `rs274 -g`, which prints four decimal places.
    pub const RS274: Self = Self {
        name: "rs274",
        rate_tolerance: RATE_TOLERANCE,
    };
}

/// Compares `reference`, DreamCarve's reading, with `canon`, `rs274`'s.
#[must_use]
pub fn compare(reference: &[RefEvent], canon: &[CanonEvent]) -> Comparison {
    compare_with(reference, canon, Reader::RS274)
}

/// Compares `reference`, DreamCarve's reading, with `canon`, the reading
/// of `reader`, which made the events in [`CanonEvent`]'s terms.
#[must_use]
pub fn compare_with(reference: &[RefEvent], canon: &[CanonEvent], reader: Reader) -> Comparison {
    let mut by_line: BTreeMap<Option<usize>, (Vec<RefEvent>, Vec<Located>)> = BTreeMap::new();
    for event in reference {
        if let RefEvent::Move { kind, from, to, .. } = event
            && !is_arc(*kind)
            && from == to
        {
            continue;
        }
        by_line
            .entry(Some(event.line()))
            .or_default()
            .0
            .push(*event);
    }
    let mut at: Option<[f64; 3]> = None;
    for event in canon {
        let from = at;
        if let CanonEvent::Move(m) = event {
            at = Some(m.to);
            if !is_arc(m.kind) && from == Some(m.to) {
                continue;
            }
        }
        by_line
            .entry(event.line())
            .or_default()
            .1
            .push((*event, from));
    }

    let mut out = Comparison {
        lines: by_line.len(),
        ..Comparison::default()
    };
    for (line, (ours, theirs)) in &by_line {
        if ours.len() != theirs.len() {
            out.differences.push(Difference {
                line: *line,
                what: format!(
                    "DreamCarve reads {} event(s), {} makes {}:\n  DreamCarve: {}\n  {:<11} {}",
                    ours.len(),
                    reader.name,
                    theirs.len(),
                    list(ours.iter().map(describe_ref)),
                    format!("{}:", reader.name),
                    list(theirs.iter().map(|(e, _)| describe_canon(e))),
                ),
            });
            continue;
        }
        for (ours, (theirs, from)) in ours.iter().zip(theirs) {
            if matches!(ours, RefEvent::Move { .. } | RefEvent::MachineLift { .. }) {
                out.moves += 1;
            }
            if let Some(what) = differ(ours, theirs, *from, reader) {
                out.differences.push(Difference { line: *line, what });
            }
        }
    }
    out
}

/// How `ours` and `theirs` differ, or `None` if they agree.
fn differ(
    ours: &RefEvent,
    theirs: &CanonEvent,
    from: Option<[f64; 3]>,
    reader: Reader,
) -> Option<String> {
    let name = reader.name;
    let mut why = Vec::new();
    match (ours, theirs) {
        (
            RefEvent::Move {
                kind,
                to,
                feed,
                centre,
                ..
            },
            CanonEvent::Move(m),
        ) => {
            if *kind != m.kind {
                why.push(format!("kind {kind:?} vs {:?}", m.kind));
            }
            for (axis, ours, theirs) in axes(to, m) {
                if let Some(ours) = ours
                    && !approx_eq(ours, theirs, LENGTH_TOLERANCE)
                {
                    why.push(format!("{axis} {ours} vs {theirs}"));
                }
            }
            match feed {
                Some(f) if !rate_eq(*f, m.feed, reader.rate_tolerance) => {
                    why.push(format!("F {f} vs {}", m.feed))
                }
                None if kind.is_cut() => why.push("DreamCarve reads no feed".to_owned()),
                _ => {}
            }
            if let (Some([x, y]), Some([cx, cy])) = (centre, m.centre)
                && !(approx_eq(*x, cx, LENGTH_TOLERANCE) && approx_eq(*y, cy, LENGTH_TOLERANCE))
            {
                why.push(format!("arc centre ({x}, {y}) vs ({cx}, {cy})"));
            }
        }
        (RefEvent::MachineLift { z, .. }, CanonEvent::Move(m)) => {
            if m.kind != MotionKind::Rapid {
                why.push(format!("G53 lift is a {:?} under {name}", m.kind));
            }
            let machine_z = m.to[2] + m.offset[2];
            if !approx_eq(*z, machine_z, LENGTH_TOLERANCE) {
                why.push(format!("machine Z {z} vs {machine_z}"));
            }
            if let Some([x, y, _]) = from
                && !(approx_eq(x, m.to[0], LENGTH_TOLERANCE)
                    && approx_eq(y, m.to[1], LENGTH_TOLERANCE))
            {
                why.push(format!(
                    "G53 lift moves XY under {name}: ({x}, {y}) to ({}, {})",
                    m.to[0], m.to[1]
                ));
            }
        }
        (
            RefEvent::Dwell { seconds, .. },
            CanonEvent::Dwell {
                seconds: theirs, ..
            },
        ) => {
            if !approx_eq(*seconds, *theirs, LENGTH_TOLERANCE) {
                why.push(format!("dwell {seconds} s vs {theirs} s"));
            }
        }
        (RefEvent::Stop { .. }, CanonEvent::Stop { .. })
        | (RefEvent::End { .. }, CanonEvent::End { .. }) => {}
        _ => why.push(format!(
            "DreamCarve reads {}, {name} makes {}",
            describe_ref(ours),
            describe_canon(theirs)
        )),
    }
    if why.is_empty() {
        None
    } else {
        Some(format!(
            "{}: {}\n  DreamCarve: {}\n  {:<11} {}",
            describe_ref(ours).split(' ').next().unwrap_or(""),
            why.join(", "),
            describe_ref(ours),
            format!("{name}:"),
            describe_canon(theirs)
        ))
    }
}

fn axes(to: &MachinePosition, m: &CanonMove) -> [(char, Option<f64>, f64); 3] {
    [
        ('X', to.x, m.to[0]),
        ('Y', to.y, m.to[1]),
        ('Z', to.z, m.to[2]),
    ]
}

const fn is_arc(kind: MotionKind) -> bool {
    matches!(kind, MotionKind::ArcCw | MotionKind::ArcCcw)
}

fn rate_eq(a: f64, b: f64, tolerance: f64) -> bool {
    approx_eq(a, b, tolerance * a.abs().max(b.abs()).max(1.0))
}

const fn code(kind: MotionKind) -> &'static str {
    match kind {
        MotionKind::Rapid => "G0",
        MotionKind::Linear => "G1",
        MotionKind::ArcCw => "G2",
        MotionKind::ArcCcw => "G3",
    }
}

fn axis(value: Option<f64>) -> String {
    value.map_or_else(|| "?".to_owned(), |v| format!("{v:.4}"))
}

/// DreamCarve's event, for the report.
#[must_use]
pub fn describe_ref(event: &RefEvent) -> String {
    match event {
        RefEvent::Move {
            kind,
            to,
            feed,
            centre,
            ..
        } => {
            let mut s = format!(
                "{} to ({}, {}, {})",
                code(*kind),
                axis(to.x),
                axis(to.y),
                axis(to.z)
            );
            if let Some([x, y]) = centre {
                let _ = write!(s, " about ({x:.4}, {y:.4})");
            }
            if let Some(f) = feed {
                let _ = write!(s, " F{f}");
            }
            s
        }
        RefEvent::MachineLift { z, .. } => format!("G53 to machine Z {z:.4}"),
        RefEvent::Dwell { seconds, .. } => format!("dwell {seconds} s"),
        RefEvent::Stop { .. } => "M0".to_owned(),
        RefEvent::End { .. } => "end".to_owned(),
    }
}

/// `rs274`'s event, for the report.
#[must_use]
pub fn describe_canon(event: &CanonEvent) -> String {
    match event {
        CanonEvent::Move(m) => {
            let mut s = format!(
                "{} to ({:.4}, {:.4}, {:.4})",
                code(m.kind),
                m.to[0],
                m.to[1],
                m.to[2]
            );
            if let Some([x, y]) = m.centre {
                let _ = write!(s, " about ({x:.4}, {y:.4})");
            }
            if m.kind.is_cut() {
                let _ = write!(s, " F{}", m.feed);
            }
            if m.offset[2].abs() > 0.0 {
                let _ = write!(s, " (machine Z {:.4})", m.to[2] + m.offset[2]);
            }
            s
        }
        CanonEvent::Dwell { seconds, .. } => format!("dwell {seconds} s"),
        CanonEvent::Stop { .. } => "M0".to_owned(),
        CanonEvent::End { .. } => "end".to_owned(),
    }
}

fn list(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        "nothing".to_owned()
    } else {
        items.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::parse_canon;
    use crate::reference::reference;
    use dc_project::{Units, latest_definition};

    fn ours(text: &str, id: &str) -> Vec<RefEvent> {
        let post = latest_definition(id).expect("bundled").post_processor();
        reference(text, post, Units::Mm).expect("reads")
    }

    /// `rs274 -g` 2.9.4's output for [`PROGRAM`], with a G54 offset of
    /// (0, 0, -76.2) mm.
    const PROGRAM: &str = "N1 G21 G90\nN2 G0 Z5\nN3 X10 Y10\nN4 G98\nN5 G81 X20 Y20 Z-3 R1 F300\nN6 G80\nN7 G53 G0 Z-5\nN8 M2\n";
    const CANON: &str = "\
    1 N..... USE_LENGTH_UNITS(CANON_UNITS_MM)
    2 N..... SET_G5X_OFFSET(1, 0.0000, 0.0000, -76.2000, 0.0000, 0.0000, 0.0000)
    3 N1     USE_LENGTH_UNITS(CANON_UNITS_MM)
    4 N2     STRAIGHT_TRAVERSE(0.0000, 0.0000, 5.0000, 0.0000, 0.0000, 0.0000)
    5 N3     STRAIGHT_TRAVERSE(10.0000, 10.0000, 5.0000, 0.0000, 0.0000, 0.0000)
    6 N5     SET_FEED_RATE(300.0000)
    7 N5     STRAIGHT_TRAVERSE(20.0000, 20.0000, 5.0000, 0.0000, 0.0000, 0.0000)
    8 N5     STRAIGHT_TRAVERSE(20.0000, 20.0000, 1.0000, 0.0000, 0.0000, 0.0000)
    9 N5     STRAIGHT_FEED(20.0000, 20.0000, -3.0000, 0.0000, 0.0000, 0.0000)
   10 N5     STRAIGHT_TRAVERSE(20.0000, 20.0000, 5.0000, 0.0000, 0.0000, 0.0000)
   11 N7     STRAIGHT_TRAVERSE(20.0000, 20.0000, 71.2000, 0.0000, 0.0000, 0.0000)
   12 N8     PROGRAM_END()
";

    #[test]
    fn agreeing_readings_have_no_differences() {
        let c = compare(
            &ours(PROGRAM, "linuxcnc"),
            &parse_canon(CANON).expect("reads"),
        );
        assert!(c.differences.is_empty(), "{:#?}", c.differences);
        assert_eq!(c.moves, 7, "two moves, four for the hole, one lift");
    }

    /// A cycle whose depth DreamCarve reads differently is caught on the
    /// cycle's line.
    #[test]
    fn a_different_cycle_depth_is_reported_on_its_line() {
        let canon = CANON.replace("20.0000, -3.0000", "20.0000, -3.5000");
        let c = compare(
            &ours(PROGRAM, "linuxcnc"),
            &parse_canon(&canon).expect("reads"),
        );
        assert_eq!(c.differences.len(), 1, "{:#?}", c.differences);
        assert_eq!(c.differences[0].line, Some(5));
        assert!(
            c.differences[0].what.contains("Z -3 vs -3.5"),
            "{}",
            c.differences[0].what
        );
    }

    /// A `G53` lift `rs274` takes to another machine Z is caught: the
    /// work offset is what tells a machine move from a work one.
    #[test]
    fn a_lift_to_another_machine_z_is_reported() {
        let canon = CANON.replace("71.2000", "-5.0000");
        let c = compare(
            &ours(PROGRAM, "linuxcnc"),
            &parse_canon(&canon).expect("reads"),
        );
        assert_eq!(c.differences.len(), 1, "{:#?}", c.differences);
        assert!(
            c.differences[0].what.contains("machine Z -5 vs -81.2"),
            "{}",
            c.differences[0].what
        );
    }

    #[test]
    fn a_missing_move_and_a_wrong_feed_are_reported() {
        let canon = CANON
            .replace(
                "   10 N5     STRAIGHT_TRAVERSE(20.0000, 20.0000, 5.0000, 0.0000, 0.0000, 0.0000)\n",
                "",
            )
            .replace("SET_FEED_RATE(300.0000)", "SET_FEED_RATE(30.0000)");
        let c = compare(
            &ours(PROGRAM, "linuxcnc"),
            &parse_canon(&canon).expect("reads"),
        );
        let text: Vec<&str> = c.differences.iter().map(|d| d.what.as_str()).collect();
        assert!(
            text.iter().any(|t| t.contains("4 event(s), rs274 makes 3")),
            "{text:#?}"
        );
    }

    #[test]
    fn a_zero_length_move_is_left_out_on_both_sides() {
        let program = "N1 G21 G90\nN2 G0 Z5\nN3 G0 Z5\nN4 M2\n";
        let canon = "\
    1 N2     STRAIGHT_TRAVERSE(0.0000, 0.0000, 5.0000, 0.0000, 0.0000, 0.0000)
    2 N4     PROGRAM_END()
";
        let c = compare(
            &ours(program, "generic"),
            &parse_canon(canon).expect("reads"),
        );
        assert!(c.differences.is_empty(), "{:#?}", c.differences);
    }
}
