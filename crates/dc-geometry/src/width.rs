// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Widths and gaps (stub; implemented in the next commit).

use crate::contour::Contour;
use crate::region::Region;

/// The narrowest gap found by [`narrowest_gap`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    /// The gap, mm.
    pub width_mm: f64,
    /// X of the middle of the gap, mm.
    pub x_mm: f64,
    /// Y of the middle of the gap, mm.
    pub y_mm: f64,
}

/// Widths along a region (stub).
#[derive(Debug, Clone, PartialEq)]
pub struct WidthProfile {
    samples: Vec<(f64, f64)>,
}

impl WidthProfile {
    pub(crate) fn from_samples(samples: Vec<(f64, f64)>) -> Self {
        Self { samples }
    }
    /// Sorted samples.
    #[must_use]
    pub fn samples(&self) -> &[(f64, f64)] {
        &self.samples
    }
    /// Median (stub).
    #[must_use]
    pub fn median_mm(&self) -> f64 {
        0.0
    }
    /// Minimum (stub).
    #[must_use]
    pub fn min_mm(&self) -> f64 {
        0.0
    }
}

/// Stub.
#[must_use]
pub fn is_narrower_than(_region: &Region, _width_mm: f64, _tolerance_mm: f64) -> bool {
    false
}
/// Stub.
#[must_use]
pub fn hole_is_narrower_than(_hole: &Contour, _width_mm: f64, _tolerance_mm: f64) -> bool {
    false
}
/// Stub.
#[must_use]
pub fn local_widths(_region: &Region, _tolerance_mm: f64) -> Option<WidthProfile> {
    None
}
/// Stub.
#[must_use]
pub fn narrowest_gap(_regions: &[Region], _reach_mm: f64, _tolerance_mm: f64) -> Option<Gap> {
    None
}

#[cfg(test)]
mod tests;
