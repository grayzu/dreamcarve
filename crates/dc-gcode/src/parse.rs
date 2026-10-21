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
use dc_project::{PostProcessor, Units};

use crate::ExportError;
use crate::canned::{Cycle, CycleWords, Retract, Step, steps_at_hole};
use crate::transform::mm_per_unit;

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

/// One non-empty line of a parsed program, or one move the controller makes
/// for a canned-cycle line.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// 1-based line number in the program.
    pub line: usize,
    /// The words outside comments, in order. For a block the parser
    /// synthesised from a canned cycle, the words of the plain line that
    /// makes the same move (`G0 Z1`, `G1 Z-3`, `G4 P0.5`).
    pub words: Vec<Word>,
    /// The move this line makes, if any.
    pub motion: Option<MachineMove>,
    /// True for a move or dwell the controller makes for a `G81`, `G82`,
    /// `G83` or `G73` line (S11), which follows that line's own block with
    /// the same line number. The line's own block has the cycle's words and
    /// no motion.
    pub synthetic: bool,
    /// For a `G53 G0 Z<v>` line (S16, #283), the machine Z it lifts to,
    /// millimetres: a move in machine coordinates, which is not a
    /// [`MachineMove`] (those are work coordinates). Work Z is unknown
    /// after it.
    pub machine_lift_mm: Option<f64>,
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

/// What the parser must know of the controller that the program does not
/// state: to expand canned cycles (S11), the distance it backs off for a
/// `G83` re-entry and a `G73` retract
/// ([`dc_project::CannedCycles::reentry_gap_mm`]), without which a `G83`
/// or `G73` line is unparseable; and the units it reads lengths in until
/// a `G20` or `G21`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CycleModel {
    /// The re-entry gap, mm.
    pub reentry_gap_mm: Option<f64>,
    /// The units lengths are in before any `G20` or `G21`: millimetres,
    /// unless the controller has no units words (UCCNC), when the program
    /// is in the units the controller is set up in.
    pub units: Units,
}

impl Default for CycleModel {
    fn default() -> Self {
        Self {
            reentry_gap_mm: None,
            units: Units::Mm,
        }
    }
}

impl CycleModel {
    /// The model `post`'s definition states, lengths in millimetres until
    /// the program selects units.
    #[must_use]
    pub fn of(post: &PostProcessor) -> Self {
        Self {
            reentry_gap_mm: post.canned_cycles.as_ref().and_then(|c| c.reentry_gap_mm),
            units: Units::Mm,
        }
    }
}

/// Parses `text` into blocks, one per line that has words, with no
/// controller model: [`parse_program_with`] and [`CycleModel::default`].
///
/// # Errors
///
/// As [`parse_program_with`].
pub fn parse_program(text: &str) -> Result<Vec<Block>, ExportError> {
    parse_program_with(text, &CycleModel::default())
}

/// Parses `text` into blocks, one per line that has words. A canned-cycle
/// line (`G81`, `G82`, `G83`, `G73`) is followed by one synthetic block per
/// move or dwell the controller makes for it ([`crate::canned`]), using
/// `model` for what the program does not say.
///
/// # Errors
///
/// [`ExportError::Unparseable`] naming the first line that is not G-code
/// the controller would accept, or a canned cycle the parser cannot expand
/// exactly: one with Z unknown, no `G98`/`G99` in force, a missing `Z`,
/// `R`, `Q` or `P` word on the line, or a gap `model` does not state.
pub fn parse_program_with(text: &str, model: &CycleModel) -> Result<Vec<Block>, ExportError> {
    let mut state = State {
        mode: None,
        pos: MachinePosition::default(),
        feed: None,
        units: model.units,
        retract: None,
        initial_z: None,
        model: *model,
    };
    let mut blocks = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        // A line holding only `%` delimits the program (RS274/NGC, NIST IR
        // 6556, 3.1); it is not run.
        if raw.trim() == "%" {
            continue;
        }
        let words = split_words(raw).map_err(|reason| ExportError::Unparseable { line, reason })?;
        if words.is_empty() {
            continue;
        }
        state
            .apply(line, words, &mut blocks)
            .map_err(|reason| ExportError::Unparseable { line, reason })?;
    }
    Ok(blocks)
}

/// Letters the parser accepts as words. `N` is a line number. `Q` is
/// grblHAL's, for `M61 Q<n>`, which the safety pass accepts only from a
/// definition that changes tools with `M6`, and a canned cycle's peck. `R`
/// is an arc radius, or a canned cycle's retract plane.
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

/// The modal motion: a move kind, or a canned cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Move(MotionKind),
    Cycle(Cycle),
}

/// The modal state a controller carries from line to line.
struct State {
    mode: Option<Mode>,
    pos: MachinePosition,
    feed: Option<f64>,
    units: Units,
    /// `G98` or `G99`, once one is written.
    retract: Option<Retract>,
    /// The Z before the first cycle of the current series of canned
    /// cycles, which `G98` returns to.
    initial_z: Option<f64>,
    model: CycleModel,
}

impl State {
    /// Applies one line's words and pushes its block, and for a canned
    /// cycle the blocks of the moves the controller makes, onto `blocks`.
    fn apply(
        &mut self,
        line: usize,
        words: Vec<Word>,
        blocks: &mut Vec<Block>,
    ) -> Result<(), String> {
        if words
            .iter()
            .any(|w| w.letter == 'G' && is_code(w.value, 53.0))
        {
            return self.machine_lift(line, words, blocks);
        }
        let mut dwell = false;
        for w in &words {
            match w.letter {
                'G' => {
                    let mode = match w.value {
                        v if is_code(v, 0.0) => Some(Mode::Move(MotionKind::Rapid)),
                        v if is_code(v, 1.0) => Some(Mode::Move(MotionKind::Linear)),
                        v if is_code(v, 2.0) => Some(Mode::Move(MotionKind::ArcCw)),
                        v if is_code(v, 3.0) => Some(Mode::Move(MotionKind::ArcCcw)),
                        v => Cycle::of_code(v).map(Mode::Cycle),
                    };
                    if let Some(mode) = mode {
                        if !matches!(mode, Mode::Cycle(_)) {
                            // Any other motion ends a series of cycles.
                            self.initial_z = None;
                        }
                        self.mode = Some(mode);
                    } else if is_code(w.value, 80.0) {
                        // `G80` cancels the motion mode: axis words need a
                        // new one.
                        self.mode = None;
                        self.initial_z = None;
                    } else if is_code(w.value, 4.0) {
                        dwell = true;
                    } else if is_code(w.value, 20.0) {
                        self.units = Units::Inch;
                    } else if is_code(w.value, 21.0) {
                        self.units = Units::Mm;
                    } else if is_code(w.value, 98.0) {
                        self.retract = Some(Retract::InitialZ);
                    } else if is_code(w.value, 99.0) {
                        self.retract = Some(Retract::RPlane);
                    }
                }
                'F' => self.feed = Some(w.value),
                _ => {}
            }
        }
        let first = |letter: char| words.iter().find(|w| w.letter == letter).map(|w| w.value);
        let (x, y, z) = (first('X'), first('Y'), first('Z'));
        let has_axis = x.is_some() || y.is_some() || z.is_some();
        if let (Some(Mode::Cycle(cycle)), false, true) = (self.mode, dwell, has_axis) {
            let hole = self.cycle(line, cycle, &words)?;
            blocks.push(Block {
                line,
                words,
                motion: None,
                synthetic: false,
                machine_lift_mm: None,
            });
            blocks.extend(hole.into_iter().map(|(words, motion)| Block {
                line,
                words,
                motion,
                synthetic: true,
                machine_lift_mm: None,
            }));
            return Ok(());
        }
        let arc = ArcWords {
            i: first('I'),
            j: first('J'),
            r: first('R'),
        };
        let has_centre = arc.i.is_some() || arc.j.is_some() || arc.r.is_some();
        let motion = if dwell || !(has_axis || has_centre) {
            None
        } else {
            let kind = match self.mode {
                Some(Mode::Move(kind)) => kind,
                Some(Mode::Cycle(_)) => return Err("cycle words with no X, Y or Z".to_owned()),
                None => return Err("axis words with no motion mode".to_owned()),
            };
            if has_centre && !matches!(kind, MotionKind::ArcCw | MotionKind::ArcCcw) {
                return Err("arc words on a straight move".to_owned());
            }
            let to = MachinePosition {
                x: x.or(self.pos.x),
                y: y.or(self.pos.y),
                z: z.or(self.pos.z),
            };
            Some(self.make(line, kind, to, arc))
        };
        blocks.push(Block {
            line,
            words,
            motion,
            synthetic: false,
            machine_lift_mm: None,
        });
        Ok(())
    }

    /// S16 (#283): a line with `G53` must be `G53 G0 Z<v>` and nothing
    /// else but a line number: one rapid in Z alone in machine
    /// coordinates, which grblHAL, GRBL and LinuxCNC run ignoring work and
    /// tool offsets (grblHAL `gcode.c` 3254-3277, GRBL `gcode.c` 592-597,
    /// LinuxCNC `interp_find.cc` 170-200; definitions README,
    /// "Machine-coordinate moves"). It
    /// leaves `G0` the motion mode and work Z unknown; X and Y do not move.
    /// Any other `G53` is refused rather than modelled.
    fn machine_lift(
        &mut self,
        line: usize,
        words: Vec<Word>,
        blocks: &mut Vec<Block>,
    ) -> Result<(), String> {
        let refuse = || "`G53` is accepted only as `G53 G0 Z…`, alone on its line (S16)".to_owned();
        let count = |letter: char, value: Option<f64>| {
            words
                .iter()
                .filter(|w| w.letter == letter && value.is_none_or(|v| is_code(w.value, v)))
                .count()
        };
        let only_these = words.iter().all(|w| matches!(w.letter, 'G' | 'Z' | 'N'));
        if !(only_these
            && count('G', Some(53.0)) == 1
            && count('G', Some(0.0)) == 1
            && count('G', None) == 2
            && count('Z', None) == 1
            && count('N', None) <= 1)
        {
            return Err(refuse());
        }
        let z = words
            .iter()
            .find(|w| w.letter == 'Z')
            .map(|w| w.value)
            .ok_or_else(refuse)?;
        self.mode = Some(Mode::Move(MotionKind::Rapid));
        self.initial_z = None;
        self.pos.z = None;
        blocks.push(Block {
            line,
            words,
            motion: None,
            synthetic: false,
            machine_lift_mm: Some(z * mm_per_unit(self.units)),
        });
        Ok(())
    }

    /// The move to `to` of `kind` from where the controller is, which it
    /// then is at.
    fn make(
        &mut self,
        line: usize,
        kind: MotionKind,
        to: MachinePosition,
        arc: ArcWords,
    ) -> MachineMove {
        let from = self.pos;
        self.pos = to;
        MachineMove {
            line,
            kind,
            from,
            to,
            feed: self.feed,
            units: self.units,
            arc,
        }
    }

    /// The moves and dwells, each with the words of the plain line that
    /// makes it, the controller makes for `cycle` line `words` (S11; see
    /// [`crate::canned`]). The cycle's own words must all be on the line:
    /// the parser does not rely on which of them a controller keeps modal.
    fn cycle(
        &mut self,
        line: usize,
        cycle: Cycle,
        words: &[Word],
    ) -> Result<Vec<Synthetic>, String> {
        let code = cycle.code();
        let first = |letter: char| words.iter().find(|w| w.letter == letter).map(|w| w.value);
        if first('I').is_some() || first('J').is_some() {
            return Err(format!("{code} with I or J"));
        }
        let need = |letter: char| first(letter).ok_or_else(|| format!("{code} without {letter}"));
        let cycle_words = CycleWords {
            cycle,
            z: need('Z')?,
            r: need('R')?,
            q: if cycle.pecks() {
                Some(need('Q')?)
            } else {
                None
            },
            p: if cycle == Cycle::G82 {
                Some(need('P')?)
            } else {
                None
            },
        };
        let retract = self
            .retract
            .ok_or_else(|| format!("{code} with neither G98 nor G99 in force"))?;
        let z_now = self.pos.z.ok_or_else(|| format!("{code} with Z unknown"))?;
        let initial_z = *self.initial_z.get_or_insert(z_now);
        let r = cycle_words.r;
        let clear_z = match retract {
            Retract::InitialZ => initial_z.max(r),
            Retract::RPlane => r,
        };
        let gap = self
            .model
            .reentry_gap_mm
            .map(|gap_mm| gap_mm / mm_per_unit(self.units));
        let steps = steps_at_hole(&cycle_words, gap, clear_z)?;

        let mut out = Vec::with_capacity(steps.len() + 3);
        // Preliminary motion: up to R if below it, over the hole at that
        // Z, down to R.
        if z_now < r {
            out.extend(self.z_move(line, MotionKind::Rapid, r));
        }
        let (x, y) = (first('X').or(self.pos.x), first('Y').or(self.pos.y));
        if differs(x, self.pos.x) || differs(y, self.pos.y) {
            let to = MachinePosition {
                x,
                y,
                z: self.pos.z,
            };
            let mut xy = vec![word('G', 0.0)];
            xy.extend(x.map(|x| word('X', x)));
            xy.extend(y.map(|y| word('Y', y)));
            let mv = self.make(line, MotionKind::Rapid, to, ArcWords::default());
            out.push((xy, Some(mv)));
        }
        out.extend(self.z_move(line, MotionKind::Rapid, r));
        for step in steps {
            match step {
                Step::Rapid(z) => out.extend(self.z_move(line, MotionKind::Rapid, z)),
                Step::Feed(z) => out.extend(self.z_move(line, MotionKind::Linear, z)),
                Step::Dwell(p) => out.push((vec![word('G', 4.0), word('P', p)], None)),
            }
        }
        Ok(out)
    }

    /// A move in Z alone of `kind` to `z`, with the words of the plain
    /// line that makes it; none if the controller is there already.
    fn z_move(&mut self, line: usize, kind: MotionKind, z: f64) -> Option<Synthetic> {
        if self.pos.z.is_some_and(|now| approx_eq(now, z, SAME_Z)) {
            return None;
        }
        let to = MachinePosition {
            z: Some(z),
            ..self.pos
        };
        let g = if kind == MotionKind::Rapid { 0.0 } else { 1.0 };
        let mv = self.make(line, kind, to, ArcWords::default());
        Some((vec![word('G', g), word('Z', z)], Some(mv)))
    }
}

/// A block the parser synthesises for a canned cycle: the words of the
/// plain line that makes the same move or dwell, and the move.
type Synthetic = (Vec<Word>, Option<MachineMove>);

/// The word `letter` with `value`.
const fn word(letter: char, value: f64) -> Word {
    Word { letter, value }
}

/// Synthesised moves closer than this to where the controller is are left
/// out, as a controller makes no move there.
const SAME_Z: f64 = 1e-9;

/// True unless both are known and equal.
fn differs(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !approx_eq(a, b, SAME_Z),
        (None, None) => false,
        _ => true,
    }
}

/// True if a word's number is the code `code` (`G0` and `G00` alike).
pub(crate) fn is_code(value: f64, code: f64) -> bool {
    approx_eq(value, code, CODE_TOLERANCE)
}

/// How close a word's number must be to a code to be that code. Codes are
/// whole or tenths (`G38.2`), so anything under a tenth works.
const CODE_TOLERANCE: f64 = 1e-6;

/// The moves of `text`, in order: the parsed program reduced to motion,
/// with no controller model.
///
/// # Errors
///
/// As [`parse_program`].
pub fn parse_motion(text: &str) -> Result<Vec<MachineMove>, ExportError> {
    parse_motion_with(text, &CycleModel::default())
}

/// The moves of `text`, in order, canned cycles expanded with `model`: the
/// motion the controller makes.
///
/// # Errors
///
/// As [`parse_program_with`].
pub fn parse_motion_with(text: &str, model: &CycleModel) -> Result<Vec<MachineMove>, ExportError> {
    Ok(parse_program_with(text, model)?
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

    /// S11: a `G81` line becomes its own block, with no motion, then one
    /// synthetic block per move the controller makes: over the hole, down
    /// to R, the feed to Z, back to the initial Z (`G98`). A cycle with
    /// neither `G98` nor `G99` in force, or with Z unknown, or missing a
    /// word, is not expanded.
    #[test]
    fn canned_cycle_lines_expand_into_the_controllers_moves() {
        let text = "G21\nG0 Z5\nX0 Y0\nG98 G81 X10 Y0 Z-3 R1 F300\nG80\n";
        let blocks = parse_program(text).unwrap();
        let cycle: Vec<&Block> = blocks.iter().filter(|b| b.line == 4).collect();
        assert_eq!(cycle.len(), 5);
        assert!(!cycle[0].synthetic && cycle[0].motion.is_none());
        let moves: Vec<(MotionKind, Option<f64>, Option<f64>)> = cycle[1..]
            .iter()
            .map(|b| {
                assert!(b.synthetic);
                let m = b.motion.unwrap();
                (m.kind, m.to.x, m.to.z)
            })
            .collect();
        assert_eq!(
            moves,
            [
                (MotionKind::Rapid, Some(10.0), Some(5.0)),
                (MotionKind::Rapid, Some(10.0), Some(1.0)),
                (MotionKind::Linear, Some(10.0), Some(-3.0)),
                (MotionKind::Rapid, Some(10.0), Some(5.0)),
            ]
        );
        assert_eq!(cycle[3].motion.unwrap().feed, Some(300.0));
        for bad in [
            "G21\nG0 Z5\nG81 X10 Y0 Z-3 R1 F300\n",
            "G21\nG98 G81 X10 Y0 Z-3 R1 F300\n",
            "G21\nG0 Z5\nG98 G81 X10 Y0 Z-3 F300\n",
            "G21\nG0 Z5\nG98 G83 X10 Y0 Z-3 R1 Q1 F300\n",
            "G21\nG0 Z5\nG98 G81 X10 Y0 Z-3 R1 F300\nG80\nX20\n",
        ] {
            assert!(
                matches!(parse_program(bad), Err(ExportError::Unparseable { .. })),
                "{bad}"
            );
        }
        // With the controller's gap a `G83` expands.
        let model = CycleModel {
            reentry_gap_mm: Some(0.5),
            ..CycleModel::default()
        };
        parse_program_with("G21\nG0 Z5\nG98 G83 X10 Y0 Z-3 R1 Q1 F300\n", &model).unwrap();
    }

    #[test]
    fn round_trips_emitted_program() {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        let tool = project.tools[0].clone();
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        dc_cam::test_support::assert_safe(&toolpath, &project, &Operation::Profile(op.clone()));
        let (program, _) = emit(
            &project,
            &[(Operation::Profile(op), tool, toolpath.clone()).into()],
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
