// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Image prep (sub-phase 3.4, `docs/specs/phase-3/design.md`, "Image
//! prep"): the [`walk`] of an SVG's visible geometry, the import checks
//! built on it ([`findings()`] for `init`, [`design_findings`] for `check`)
//! and [`prepare()`], which turns an SVG into pieces and a [`PrepReport`].

mod findings;
mod report;
mod run;
pub(crate) mod stroke;
pub mod walk;

pub use crate::svg::MIN_FLATTEN_TOLERANCE_MM;
pub use findings::{design_findings, design_findings_named, findings};
pub use report::{PrepReport, ReadReport, StrokeReport, WidthFigure};
pub use run::{PrepMode, PrepOptions, Prepared, flatten_tolerance_mm, prepare};

/// The default minimum detail, percent of the design's height (functional
/// 49d). A proposal: slice 21f calibrates it (open question 16). Pieces
/// and holes whose widest point is under this share of the height are
/// reported by `feature_below_size`.
pub const DEFAULT_MIN_DETAIL_PCT: f64 = 0.2;
/// The default `--smooth`, percent of the design's height (open question
/// 17). A proposal: slice 21f calibrates it.
pub const DEFAULT_SMOOTH_PCT: f64 = 0.05;
/// The largest threshold either flag takes, percent.
pub const MAX_THRESHOLD_PCT: f64 = 5.0;
/// Prepared outlines are polygons within this fraction of the design's
/// height of the true curves (0.005 %), never finer than
/// [`MIN_FLATTEN_TOLERANCE_MM`].
pub const FLATTEN_FRACTION: f64 = 5.0e-5;
/// `--regions` is suggested when the ink's median width is under this
/// percentage of the height (49c, open question 18).
pub const REGIONS_SUGGESTED_BELOW_PCT: f64 = 2.0;
/// A filled piece whose box covers this fraction of the drawing's box is
/// flagged as a probable page background.
pub const BACKGROUND_COVER_FRACTION: f64 = 0.98;
/// The report's guide width, mm (49e); informational.
pub const REPORT_MIN_FEATURE_MM: f64 = 1.5;
/// The heights the report scales the widths to, mm.
pub const REPORT_HEIGHTS_MM: [f64; 3] = [100.0, 200.0, 300.0];
/// `narrowest_gap` searches this fraction of the height.
pub const GAP_REACH_FRACTION: f64 = 0.25;
