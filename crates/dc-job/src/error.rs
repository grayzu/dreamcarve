// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type: what a job step cannot do with the project or
//! drawing it was given. Generation and emission report their problems as
//! [`crate::Findings`], not as this error, so that every problem of a
//! project is reported at once.

use dc_cam::CamError;
use dc_project::OperationId;

/// Why a job step could not produce its result.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// A drawing with no shapes makes no project
    /// ([`crate::project_from::drawing`]).
    #[error("the drawing contains no shapes to import")]
    NoShapes,
    /// The drawing plus the stock margin is larger than
    /// [`dc_geometry::MAX_COORDINATE_MM`], which validation would reject
    /// ([`crate::project_from::drawing`]).
    #[error(
        "with a {margin_mm} mm margin the drawing needs a {width_mm:.2} × {height_mm:.2} mm stock, \
         larger than the {limit_mm} mm limit; make the drawing smaller"
    )]
    StockTooLarge {
        /// The stock width the drawing would need, mm.
        width_mm: f64,
        /// The stock height the drawing would need, mm.
        height_mm: f64,
        /// The margin added on each side, mm.
        margin_mm: f64,
        /// The coordinate limit, mm.
        limit_mm: f64,
    },
    /// Tabs were asked for a Profile that holds no tab request
    /// ([`crate::tabs::place_request`]).
    #[error("operation `{op}` has no tab request to place")]
    NoTabRequest {
        /// The Profile.
        op: OperationId,
    },
    /// Placing tabs failed in `dc-cam`.
    #[error(transparent)]
    Cam(#[from] CamError),
}
