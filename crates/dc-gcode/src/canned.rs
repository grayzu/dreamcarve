// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a controller does for a drilling canned cycle line (S11;
//! `docs/specs/phase-2/design.md`, "Algorithms → Canned cycles", "S11
//! expansion"). The parser expands every `G81`, `G82`, `G83` and `G73` line
//! into these moves, so the safety pass checks the motion the machine makes,
//! not the words that ask for it; the emitter uses the same model to decide
//! whether a Drill can be written as cycles at all.
//!
//! The model follows the RS274/NGC interpreter (NIST IR 6556, Kramer,
//! Proctor and Messina, 2000, section 3.5.16, "Canned Cycles") and the
//! LinuxCNC G-code reference (`gcode.html`, "G81", "G82", "G83", "G73",
//! "G98, G99", "Preliminary and In-Between Motion"):
//!
//! - preliminary motion: if Z is below the R plane, rapid up to it; rapid
//!   in XY at the current Z; rapid down to the R plane;
//! - `G81`: feed to `Z`; `G82`: feed to `Z`, dwell `P`;
//! - `G83`: feed down by `Q` (or to `Z`, whichever is less deep), rapid up
//!   to the R plane, rapid back down to the depth reached plus the
//!   controller's re-entry gap, repeat until `Z` is reached;
//! - `G73`: feed down by `Q`, rapid up by the controller's short retract,
//!   repeat until `Z` is reached;
//! - then rapid to the initial Z (`G98`: the Z before the first cycle of
//!   the series, or R if that is higher) or to the R plane (`G99`).
//!
//! Every peck is measured from the R plane, not the stock top. How far the
//! controller backs off for a `G83` re-entry or a `G73` retract is not a
//! word of the program: the definition states it as
//! `canned_cycles.reentry_gap_mm`, from the controller's documentation, and
//! a definition without it cannot write either cycle.

/// A drilling canned cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cycle {
    /// Drill: feed to the bottom, rapid out.
    G81,
    /// Drill with a dwell at the bottom.
    G82,
    /// Peck drill, retracting to the R plane after every peck.
    G83,
    /// Chip-break drill, retracting a short distance after every peck.
    G73,
}

impl Cycle {
    /// The cycle a `G` word's number selects, if any.
    pub(crate) fn of_code(value: f64) -> Option<Self> {
        [Self::G81, Self::G82, Self::G83, Self::G73]
            .into_iter()
            .find(|c| crate::parse::is_code(value, c.number()))
    }

    /// The `G` number.
    pub(crate) const fn number(self) -> f64 {
        match self {
            Self::G81 => 81.0,
            Self::G82 => 82.0,
            Self::G83 => 83.0,
            Self::G73 => 73.0,
        }
    }

    /// The code as written, `G81` for example.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::G81 => "G81",
            Self::G82 => "G82",
            Self::G83 => "G83",
            Self::G73 => "G73",
        }
    }

    /// Whether the cycle pecks, so it needs `Q` and the re-entry gap.
    pub(crate) const fn pecks(self) -> bool {
        matches!(self, Self::G83 | Self::G73)
    }
}

/// Where a cycle returns to after the hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retract {
    /// `G98`: the initial Z, or the R plane if that is higher.
    InitialZ,
    /// `G99`: the R plane.
    RPlane,
}

/// The words of one cycle line, machine Z in program units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CycleWords {
    pub cycle: Cycle,
    /// The hole bottom, `Z`.
    pub z: f64,
    /// The retract plane, `R`.
    pub r: f64,
    /// The peck, `Q`, for `G83` and `G73`.
    pub q: Option<f64>,
    /// The dwell, `P`, as written, for `G82`.
    pub p: Option<f64>,
}

/// One move the controller makes in Z at the hole, or a dwell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Step {
    /// Rapid to this Z.
    Rapid(f64),
    /// Feed to this Z.
    Feed(f64),
    /// Dwell, `P` as written.
    Dwell(f64),
}

/// Most pecks one cycle line may make; a line asking for more is refused
/// rather than expanded (a tiny `Q` would otherwise make millions).
pub(crate) const MAX_PECKS: usize = 10_000;

/// The moves the controller makes at the hole once it is over it at the R
/// plane: the cycle itself, then the rapid to `clear_z`. `gap` is the
/// definition's re-entry gap in program units.
///
/// # Errors
///
/// Why the line cannot be expanded: `Z` above `R`, a missing or non-positive
/// `Q`, a missing or negative `P`, a pecking cycle with no stated gap, or
/// more than [`MAX_PECKS`] pecks.
pub(crate) fn steps_at_hole(
    words: &CycleWords,
    gap: Option<f64>,
    clear_z: f64,
) -> Result<Vec<Step>, String> {
    let CycleWords { cycle, z, r, q, p } = *words;
    let code = cycle.code();
    if z > r {
        return Err(format!("{code} has Z above R"));
    }
    let mut steps = Vec::new();
    match cycle {
        Cycle::G81 => steps.push(Step::Feed(z)),
        Cycle::G82 => {
            let p = p
                .filter(|p| *p >= 0.0)
                .ok_or_else(|| format!("{code} needs a P dwell of at least 0"))?;
            steps.push(Step::Feed(z));
            steps.push(Step::Dwell(p));
        }
        Cycle::G83 | Cycle::G73 => {
            let q = q
                .filter(|q| *q > 0.0)
                .ok_or_else(|| format!("{code} needs a positive Q peck"))?;
            let gap = gap.filter(|g| *g > 0.0).ok_or_else(|| {
                format!("{code} needs the controller's re-entry gap, which the post-processor does not state")
            })?;
            let mut depth = r;
            for _ in 0..MAX_PECKS {
                let next = (depth - q).max(z);
                steps.push(Step::Feed(next));
                if next <= z {
                    steps.push(Step::Rapid(clear_z));
                    return Ok(steps);
                }
                if cycle == Cycle::G83 {
                    steps.push(Step::Rapid(r));
                }
                steps.push(Step::Rapid(next + gap));
                depth = next;
            }
            return Err(format!("{code} makes more than {MAX_PECKS} pecks"));
        }
    }
    steps.push(Step::Rapid(clear_z));
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(cycle: Cycle, q: Option<f64>, p: Option<f64>) -> CycleWords {
        CycleWords {
            cycle,
            z: -10.0,
            r: 1.0,
            q,
            p,
        }
    }

    #[test]
    fn g81_feeds_to_bottom_and_rapids_out() {
        let steps = steps_at_hole(&words(Cycle::G81, None, None), None, 5.0).unwrap();
        assert_eq!(steps, [Step::Feed(-10.0), Step::Rapid(5.0)]);
    }

    #[test]
    fn g82_dwells_at_the_bottom() {
        let steps = steps_at_hole(&words(Cycle::G82, None, Some(0.5)), None, 5.0).unwrap();
        assert_eq!(
            steps,
            [Step::Feed(-10.0), Step::Dwell(0.5), Step::Rapid(5.0)]
        );
        assert!(steps_at_hole(&words(Cycle::G82, None, None), None, 5.0).is_err());
    }

    /// Pecks are measured from R: 1 − 4 = −3, −7, then −10.
    #[test]
    fn g83_pecks_from_r_and_reenters_by_the_gap() {
        let steps = steps_at_hole(&words(Cycle::G83, Some(4.0), None), Some(0.5), 5.0).unwrap();
        assert_eq!(
            steps,
            [
                Step::Feed(-3.0),
                Step::Rapid(1.0),
                Step::Rapid(-2.5),
                Step::Feed(-7.0),
                Step::Rapid(1.0),
                Step::Rapid(-6.5),
                Step::Feed(-10.0),
                Step::Rapid(5.0),
            ]
        );
    }

    #[test]
    fn g73_retracts_by_the_gap_only() {
        let steps = steps_at_hole(&words(Cycle::G73, Some(4.0), None), Some(0.5), 5.0).unwrap();
        assert_eq!(
            steps,
            [
                Step::Feed(-3.0),
                Step::Rapid(-2.5),
                Step::Feed(-7.0),
                Step::Rapid(-6.5),
                Step::Feed(-10.0),
                Step::Rapid(5.0),
            ]
        );
    }

    #[test]
    fn bad_words_are_refused() {
        for (w, gap) in [
            (words(Cycle::G83, None, None), Some(0.5)),
            (words(Cycle::G83, Some(0.0), None), Some(0.5)),
            (words(Cycle::G73, Some(4.0), None), None),
            (words(Cycle::G83, Some(1e-6), None), Some(0.5)),
            (
                CycleWords {
                    z: 2.0,
                    ..words(Cycle::G81, None, None)
                },
                None,
            ),
        ] {
            assert!(steps_at_hole(&w, gap, 5.0).is_err(), "{w:?}");
        }
    }
}
