// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Image prep, the part of it that reads an SVG without changing it
//! (sub-phase 3.4, `docs/specs/phase-3/design.md`, "Image prep"): the
//! [`walk`] of an SVG's visible geometry and the import checks built on it,
//! [`findings()`] for `init` and [`design_findings`] for `check`.

mod findings;
pub(crate) mod stroke;
pub mod walk;

pub use findings::{design_findings, design_findings_named, findings};

/// The default minimum detail, percent of the design's height (functional
/// 49d). A proposal: slice 21f calibrates it (open question 16). Pieces
/// and holes whose widest point is under this share of the height are
/// reported by `feature_below_size`.
pub const DEFAULT_MIN_DETAIL_PCT: f64 = 0.2;
