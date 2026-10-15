// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/tabProfile.ts

//! The Z profile a smooth tab imposes on one crossing of its span
//! (`docs/specs/phase-1/design.md`, "Algorithms → Profile contour", step 7).
//!
//! A rect tab steps the tool up to the tab top at the span boundary, rides
//! it flat, and steps back down. A smooth tab keeps the same span and the
//! same peak but rises and falls continuously, so the machine never stops
//! XY while Z moves. `u` is a normalised position across one crossing: 0 at
//! the span entry, 1 at its exit.
//!
//! PureCutCNC's truncated-crossing clamp (`clampCrossingFraction`) is not
//! ported: DreamCarve applies tabs only to closed tool-centre contours, so
//! every crossing has both its entry and its exit.

/// Fewest samples one crossing is split into: even, so `u = 0.5` is one.
pub const MIN_SAMPLE_COUNT: u32 = 4;

/// Most samples one crossing is ever split into.
pub const MAX_SAMPLE_COUNT: u32 = 512;

/// Normalised raised-cosine height: `u` in [0, 1] maps to [0, 1].
///
/// `h(0) = h(1) = 0` and `h(0.5) = 1` exactly; the slope is zero at both
/// ends and at the peak; it rises monotonically on [0, 0.5], falls on
/// [0.5, 1], and is never negative, so the tool never dips below the cut Z
/// it came in at. `u` outside [0, 1] is clamped.
#[must_use]
pub fn smooth_tab_height_fraction(u: f64) -> f64 {
    let _ = u;
    todo!()
}

/// Absolute Z, mm, at normalised position `u` across one crossing that
/// starts and ends at `base_z_mm` and peaks at `top_z_mm`. A top at or below
/// the base asks for no lift: the result is the base, never an inverted
/// ramp.
#[must_use]
pub fn smooth_tab_z_mm(u: f64, base_z_mm: f64, top_z_mm: f64) -> f64 {
    let _ = (u, base_z_mm, top_z_mm);
    todo!()
}

/// Number of equal sub-intervals one crossing must be split into so that
/// straight chords stay within `chord_tolerance_mm` of the curve in Z.
///
/// For `z(s) = A·(1 − cos(2πs/L))/2` the second derivative peaks at
/// `2π²A/L²`, and a chord across `Δs = L/n` deviates by at most
/// `π²A/(4n²)`, so `n ≥ (π/2)·√(A / tolerance)`. The crossing length drops
/// out: a longer crossing is a gentler curve. The count is even, so the
/// peak is a sample, and lies in [`MIN_SAMPLE_COUNT`, `MAX_SAMPLE_COUNT`];
/// a non-positive rise or tolerance gives the minimum.
#[must_use]
pub fn smooth_tab_sample_count(rise_mm: f64, chord_tolerance_mm: f64) -> u32 {
    let _ = (rise_mm, chord_tolerance_mm);
    todo!()
}

/// Normalised sample positions across one crossing, strictly increasing,
/// [`smooth_tab_sample_count`] intervals, always including 0, 0.5, and 1
/// exactly. Callers map them onto distance along the path, so XY follows
/// the path exactly and only Z is approximated.
#[must_use]
pub fn smooth_tab_sample_fractions(rise_mm: f64, chord_tolerance_mm: f64) -> Vec<f64> {
    let _ = (rise_mm, chord_tolerance_mm);
    todo!()
}
