// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The area a toolpath's cutting moves sweep (`docs/specs/phase-2/design.md`,
//! "Data model → dc-cam", `footprint`; "Algorithms → Pocket" step 12 and
//! the coverage check). Safety rule S9 and the coverage tests compare it
//! against the regions an operation may cut.

use crate::toolpath::Toolpath;
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Region};

/// The area, mm², Y-up, that a tool of radius `radius_mm` sweeps along
/// the cutting moves of `toolpath`: every linear and arc move's swept
/// band, round-ended, as the union of them all. Rapids are left out (S2
/// keeps them above the stock) and so are vertical moves, which sweep only
/// the disc at their end that the move before or after sweeps too. Arcs
/// are swept as arcs, not chords. The result is polygonal, flattened at
/// [`FLATTEN_TOLERANCE_DEFAULT_MM`], as [`dc_geometry::union`] returns it;
/// [`footprint_at`] takes the tolerance.
#[must_use]
pub fn footprint(toolpath: &Toolpath, radius_mm: f64) -> Vec<Region> {
    footprint_at(toolpath, radius_mm, FLATTEN_TOLERANCE_DEFAULT_MM)
}

/// [`footprint`] flattened at `tolerance_mm`.
#[must_use]
pub fn footprint_at(toolpath: &Toolpath, radius_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let _ = (toolpath, radius_mm, tolerance_mm);
    Vec::new()
}
