// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads an emitted program back as a controller would, so the safety pass
//! checks the text that reaches the machine rather than the toolpath that
//! produced it (`docs/specs/phase-1/design.md`, "Emission", step 7;
//! `docs/specs/phase-1/functional.md`, S1).
//!
//! The reader is independent of the post-processor definition: it follows
//! standard G-code semantics for the words GRBL and grblHAL accept. `( )`
//! and `;` comments are skipped, whitespace is ignored, motion mode,
//! coordinates, feed, spindle speed, and units are modal. A word it does
//! not know, or a number it cannot read (an exponent, for example), makes
//! the line unparseable, which `emit` reports as an error.

use dc_geometry::approx_eq;
use dc_project::Units;

use crate::ExportError;

/// One word of a block: a letter and its number, as in `X12.5`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Word {
    /// The upper-case letter.
    pub letter: char,
    /// The number after it.
    pub value: f64,
}

/// A machine position whose axes may not be known yet: at program start
/// the controller is wherever it was left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MachinePosition {
    /// Machine X, program units, if known.
    pub x: Option<f64>,
    /// Machine Y, program units, if known.
    pub y: Option<f64>,
    /// Machine Z, program units, if known.
    pub z: Option<f64>,
}

/// The motion mode of a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind {
    /// `G0`.
    Rapid,
    /// `G1`.
    Linear,
    /// `G2`, clockwise viewed from +Z.
    ArcCw,
    /// `G3`, counter-clockwise viewed from +Z.
    ArcCcw,
}

impl MotionKind {
    /// True for `G1`, `G2`, and `G3`: moves that cut at the feed rate.
    #[must_use]
    pub const fn is_cut(self) -> bool {
        !matches!(self, Self::Rapid)
    }
}

/// The centre words of an arc move, program units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ArcWords {
    /// `I`, X offset of the centre from the start.
    pub i: Option<f64>,
    /// `J`, Y offset of the centre from the start.
    pub j: Option<f64>,
    /// `R`, the radius.
    pub r: Option<f64>,
}

/// One move of a parsed program, in machine coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MachineMove {
    /// 1-based line number in the program.
    pub line: usize,
    /// The motion mode.
    pub kind: MotionKind,
    /// Where the move starts, program units.
    pub from: MachinePosition,
    /// Where the move ends, program units.
    pub to: MachinePosition,
    /// Active feed, program units per minute, if one has been set.
    pub feed: Option<f64>,
    /// Active units.
    pub units: Units,
    /// Centre words, for arcs.
    pub arc: ArcWords,
}

/// One non-empty line of a parsed program.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// 1-based line number in the program.
    pub line: usize,
    /// The words outside comments, in order.
    pub words: Vec<Word>,
    /// The move this line makes, if any.
    pub motion: Option<MachineMove>,
}

impl Block {
    /// True if the block has word `letter` with value `value`.
    #[must_use]
    pub fn has(&self, letter: char, value: f64) -> bool {
        self.words
            .iter()
            .any(|w| w.letter == letter && is_code(w.value, value))
    }

    /// The value of the first `letter` word, if any.
    #[must_use]
    pub fn value(&self, letter: char) -> Option<f64> {
        self.words
            .iter()
            .find(|w| w.letter == letter)
            .map(|w| w.value)
    }
}

/// Parses `text` into blocks, one per line that has words.
///
/// # Errors
///
/// [`ExportError::Unparseable`] naming the first line that is not G-code
/// GRBL or grblHAL would accept.
pub fn parse_program(text: &str) -> Result<Vec<Block>, ExportError> {
    let mut state = State {
        mode: None,
        pos: MachinePosition::default(),
        feed: None,
        units: Units::Mm,
    };
    let mut blocks = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let words = split_words(raw).map_err(|reason| ExportError::Unparseable { line, reason })?;
        if words.is_empty() {
            continue;
        }
        let motion = state
            .apply(line, &words)
            .map_err(|reason| ExportError::Unparseable { line, reason })?;
        blocks.push(Block {
            line,
            words,
            motion,
        });
    }
    Ok(blocks)
}

/// Letters GRBL and grblHAL accept as words. `N` is a line number. `Q`
/// is grblHAL's, for `M61 Q<n>`, which the safety pass accepts only from a
/// definition that changes tools with `M6`.
const LETTERS: &str = "GMXYZIJKRFSPTNLQ";

/// The words of `line` outside comments, upper-cased.
fn split_words(line: &str) -> Result<Vec<Word>, String> {
    let mut code = String::new();
    let mut in_comment = false;
    for c in line.chars() {
        if in_comment {
            in_comment = c != ')';
            continue;
        }
        match c {
            '(' => in_comment = true,
            ';' => break,
            c if c.is_whitespace() => {}
            c => code.push(c.to_ascii_uppercase()),
        }
    }
    if in_comment {
        return Err("a `(` comment is not closed".to_owned());
    }
    let mut words = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(letter) = chars.next() {
        if !LETTERS.contains(letter) {
            return Err(format!("`{letter}` is not a G-code word"));
        }
        let mut number = String::new();
        while let Some(&c) = chars.peek() {
            if !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')) {
                break;
            }
            number.push(c);
            chars.next();
        }
        let value = number
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("`{letter}{number}` has no valid number"))?;
        words.push(Word { letter, value });
    }
    Ok(words)
}

/// The modal state a controller carries from line to line.
struct State {
    mode: Option<MotionKind>,
    pos: MachinePosition,
    feed: Option<f64>,
    units: Units,
}

impl State {
    /// Applies one block's words; returns its move, if it makes one.
    fn apply(&mut self, line: usize, words: &[Word]) -> Result<Option<MachineMove>, String> {
        let mut dwell = false;
        for w in words {
            match w.letter {
                'G' if is_code(w.value, 0.0) => self.mode = Some(MotionKind::Rapid),
                'G' if is_code(w.value, 1.0) => self.mode = Some(MotionKind::Linear),
                'G' if is_code(w.value, 2.0) => self.mode = Some(MotionKind::ArcCw),
                'G' if is_code(w.value, 3.0) => self.mode = Some(MotionKind::ArcCcw),
                'G' if is_code(w.value, 4.0) => dwell = true,
                'G' if is_code(w.value, 20.0) => self.units = Units::Inch,
                'G' if is_code(w.value, 21.0) => self.units = Units::Mm,
                'F' => self.feed = Some(w.value),
                _ => {}
            }
        }
        let first = |letter: char| words.iter().find(|w| w.letter == letter).map(|w| w.value);
        let (x, y, z) = (first('X'), first('Y'), first('Z'));
        let arc = ArcWords {
            i: first('I'),
            j: first('J'),
            r: first('R'),
        };
        let has_axis = x.is_some() || y.is_some() || z.is_some();
        let has_centre = arc.i.is_some() || arc.j.is_some() || arc.r.is_some();
        if dwell || !(has_axis || has_centre) {
            return Ok(None);
        }
        let Some(kind) = self.mode else {
            return Err("axis words with no motion mode".to_owned());
        };
        if has_centre && !matches!(kind, MotionKind::ArcCw | MotionKind::ArcCcw) {
            return Err("arc words on a straight move".to_owned());
        }
        let from = self.pos;
        let to = MachinePosition {
            x: x.or(from.x),
            y: y.or(from.y),
            z: z.or(from.z),
        };
        self.pos = to;
        Ok(Some(MachineMove {
            line,
            kind,
            from,
            to,
            feed: self.feed,
            units: self.units,
            arc,
        }))
    }
}

/// True if a word's number is the code `code` (`G0` and `G00` alike).
pub(crate) fn is_code(value: f64, code: f64) -> bool {
    approx_eq(value, code, CODE_TOLERANCE)
}

/// How close a word's number must be to a code to be that code. Codes are
/// whole or tenths (`G38.2`), so anything under a tenth works.
const CODE_TOLERANCE: f64 = 1e-6;

/// The moves of `text`, in order: the parsed program reduced to motion.
///
/// # Errors
///
/// As [`parse_program`].
pub fn parse_motion(text: &str) -> Result<Vec<MachineMove>, ExportError> {
    Ok(parse_program(text)?
        .into_iter()
        .filter_map(|b| b.motion)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::emit;
    use crate::transform::MachineTransform;
    use dc_cam::generate_profile;
    use dc_geometry::{Point3, assert_approx_eq};
    use dc_project::Operation;
    use dc_project::test_support::sample_project;

    #[test]
    fn reads_modal_motion_and_comments() {
        let text = "( start )\nG21 G54\nG0 Z5\nX1 Y2 ; to start\nG1 Z-1 F300\nX4\nG2 X6 Y4 I2 J0\n";
        let moves = parse_motion(text).unwrap();
        assert_eq!(moves.len(), 5);
        assert_eq!(moves[0].kind, MotionKind::Rapid);
        assert_eq!(moves[0].from, MachinePosition::default());
        assert_eq!(moves[1].line, 4);
        assert_eq!(moves[1].kind, MotionKind::Rapid);
        assert_eq!(moves[1].to.z, Some(5.0));
        assert_eq!(moves[3].kind, MotionKind::Linear);
        assert_eq!(moves[3].feed, Some(300.0));
        assert_eq!(moves[3].to.x, Some(4.0));
        assert_eq!(moves[3].to.z, Some(-1.0));
        assert_eq!(moves[4].kind, MotionKind::ArcCw);
        assert_eq!(moves[4].arc.i, Some(2.0));
        assert_eq!(moves[4].units, Units::Mm);
    }

    #[test]
    fn rejects_exponents_and_unknown_words() {
        for bad in ["G1 X1e3", "G1 X1.2.3", "G1 X", "( open", "G1 E5", "G1 X-"] {
            assert!(
                matches!(
                    parse_program(bad),
                    Err(ExportError::Unparseable { line: 1, .. })
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn round_trips_emitted_program() {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        let tool = project.tools[0].clone();
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        dc_cam::test_support::assert_safe(&toolpath, &project, &op);
        let (program, _) = emit(
            &project,
            &[(Operation::Profile(op), tool, toolpath.clone())],
            "2026-10-15",
        )
        .unwrap();

        let t =
            MachineTransform::new(&project, &project.post_processor.resolve().unwrap()).unwrap();
        let parsed: Vec<Point3> = parse_motion(&program.text)
            .unwrap()
            .iter()
            .filter_map(|m| Some(Point3::new(m.to.x?, m.to.y?, m.to.z?)))
            .map(|m| t.to_project(m, Units::Mm))
            .collect();
        // Every toolpath point is reached, in order (a rapid may emit as two
        // lines, Z then XY, so the program can have more points).
        let mut cursor = 0;
        for (index, mv) in toolpath.moves.iter().enumerate() {
            let found = parsed[cursor..].iter().position(|p| {
                p.xy().distance_to(mv.to.xy()) <= 1e-3 && (p.z - mv.to.z).abs() <= 1e-3
            });
            let Some(offset) = found else {
                panic!("move {index} {mv:?} not in the program after point {cursor}");
            };
            cursor += offset;
            let p = parsed[cursor];
            assert_approx_eq!(p.x, mv.to.x, 1e-3);
            assert_approx_eq!(p.y, mv.to.y, 1e-3);
            assert_approx_eq!(p.z, mv.to.z, 1e-3);
        }
        assert_eq!(
            cursor,
            parsed.len() - 1,
            "the program ends where the toolpath ends"
        );
    }
}
