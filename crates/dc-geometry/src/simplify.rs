// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Douglas-Peucker simplification (stub; implemented in the next commit).

use crate::contour::Contour;
use crate::region::Region;

/// Stub.
#[must_use]
pub fn simplify(contour: &Contour, _tolerance_mm: f64) -> Contour {
    contour.clone()
}

/// Stub.
#[must_use]
pub fn simplify_regions(regions: &[Region], _tolerance_mm: f64) -> (Vec<Region>, Vec<usize>) {
    (regions.to_vec(), Vec::new())
}

#[cfg(test)]
mod tests;
