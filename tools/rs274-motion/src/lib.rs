// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Compares the motion DreamCarve reads from its own LinuxCNC, Mach3, UCCNC
//! and generic programs with the motion LinuxCNC's standalone RS-274/NGC
//! interpreter, `rs274`, makes from them (issue #299).
//!
//! `dc-gcode` checks the safety rules on its own reading of a program
//! ([`dc_gcode::parse`]). That reading is DreamCarve's model of the
//! controller; if the model is wrong, the safety pass checks the wrong
//! motion. `rs274` is an independent model, the one LinuxCNC runs, and it
//! prints every move it would make as a canonical machining call
//! (`STRAIGHT_TRAVERSE`, `STRAIGHT_FEED`, `ARC_FEED`, `DWELL`). This crate
//! lines the two up, source line by source line, and reports every
//! difference in move kind, end point, arc centre, feed, dwell, canned
//! cycle expansion and `G53` machine-Z lift.
//!
//! The pieces:
//!
//! - [`bridge`] rewrites a program for `rs274`: it numbers every line with
//!   its source line, so each canonical call names the line it came from,
//!   and it rewrites the few words a dialect has that LinuxCNC does not
//!   (Mach3 and UCCNC's `G50` and `G69`, UCCNC's millisecond dwells and its
//!   missing units word). Every rewrite is listed in the report: those
//!   words are what `rs274` did not check.
//! - [`runner`] runs `rs274`, natively or in a container, with a work
//!   offset set so a `G53` move is told apart from a work move.
//! - [`canon`] reads `rs274`'s output.
//! - [`reference`] reads the original program with `dc-gcode`.
//! - [`compare`] lines the two up.
//! - [`corpus`] is the set of programs the test runs: the committed golden
//!   and expected programs, and the quick-start plaque exported in-process
//!   for each controller with canned cycles, a `G53` lift and inches.
//!
//! What this does not show: that Mach3 or UCCNC read a program as LinuxCNC
//! does. Both say they follow RS274/NGC for the words DreamCarve writes
//! (`crates/dc-project/src/definitions/README.md`), and this checks the
//! motion of those words under the reference interpreter. The rewritten
//! words, and anything the controller's own settings change, are for the
//! manual procedure in `docs/testing/post-processor-e2e.md`.

pub mod bridge;
pub mod canon;
pub mod compare;
pub mod corpus;
pub mod reference;
pub mod runner;

/// The tolerance on positions, program units. `rs274` prints four decimal
/// places and DreamCarve writes three in millimetres and four in inches,
/// so a real difference is far larger than this.
pub const LENGTH_TOLERANCE: f64 = 5e-4;

/// The relative tolerance on feeds.
pub const RATE_TOLERANCE: f64 = 1e-6;

use std::path::Path;

use dc_project::{PostProcessor, Units};

use crate::bridge::{Rewrite, bridge};
use crate::canon::parse_canon;
use crate::compare::{Comparison, compare};
use crate::reference::reference;
use crate::runner::Rs274;

/// What one program came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// `rs274` ran it, and its motion is DreamCarve's, or differs as
    /// listed.
    Compared(Comparison),
    /// `rs274` rejected the bridged program: the text says why.
    Rejected(String),
    /// The comparison could not be made: `rs274` would not start, or its
    /// output, or the program, could not be read.
    Failed(String),
}

/// One program's outcome, with what the bridge changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The lines `rs274` read other than as written.
    pub rewrites: Vec<Rewrite>,
    /// The verdict.
    pub verdict: Verdict,
}

impl Outcome {
    /// True if `rs274` ran the program and its motion is DreamCarve's.
    #[must_use]
    pub fn agrees(&self) -> bool {
        matches!(&self.verdict, Verdict::Compared(c) if c.differences.is_empty())
    }
}

/// Runs `text`, written with definition `post` (the controller set up in
/// `units` where the definition has no units word), through `rs274` in
/// `dir`, and compares its motion with DreamCarve's reading.
#[must_use]
pub fn check(text: &str, post: &PostProcessor, units: Units, rs274: &Rs274, dir: &Path) -> Outcome {
    let bridged = bridge(text, post, units);
    let verdict = match run_and_compare(text, &bridged.text, post, units, rs274, dir) {
        Ok(verdict) => verdict,
        Err(why) => Verdict::Failed(why),
    };
    Outcome {
        rewrites: bridged.rewrites,
        verdict,
    }
}

fn run_and_compare(
    text: &str,
    bridged: &str,
    post: &PostProcessor,
    units: Units,
    rs274: &Rs274,
    dir: &Path,
) -> Result<Verdict, String> {
    let ours =
        reference(text, post, units).map_err(|e| format!("DreamCarve cannot read it: {e}"))?;
    let run = rs274
        .run(bridged, dir)
        .map_err(|e| format!("cannot run {rs274}: {e}"))?;
    if let Some(why) = run.rejection() {
        return Ok(Verdict::Rejected(why));
    }
    let theirs = parse_canon(&run.stdout).map_err(|e| e.to_string())?;
    Ok(Verdict::Compared(compare(&ours, &theirs)))
}
