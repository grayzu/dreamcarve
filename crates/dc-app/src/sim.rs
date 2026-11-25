// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Simulate step without a window (phase 6 functional spec,
//! capabilities 107–124; `docs/specs/phase-6/design.md`, "dc-app"): the
//! job the shell hands its simulation worker and the bookkeeping round
//! it, the transport that plays a finished simulation, the 3D camera and
//! what the pointer is over, the data the renderer lights the stock
//! with, the tool's shape as triangles, the step's rows and result as text, and its preferences.
//!
//! Everything here is a reader (S22): it takes the runs generation made
//! and the project as it stands, and gives nothing back to either. The
//! engine is `dc-sim`, reached through `dc_job::sim`.

mod camera;
mod job;
mod mat;
mod prefs;
mod probe;
mod rows;
mod session;
mod shade;
mod tool;
mod transport;

pub use camera::{
    Box3, DISTANCE_RANGE_MM, FIT_MARGIN, FOV_Y_RAD, ORBIT_RAD_PER_PX, OrbitCamera, Ray, View3,
};
pub use job::{Built, SimJob, SimResult, SimSettings, SimState, Simulation, execute};
pub use mat::{Mat4, Vec3};
pub use prefs::{Colouring, Layers, SimPrefs, quality_name, quality_named};
pub use probe::{Hit, probe};
pub use rows::{
    Reached, Row, Rows, RunRow, UncutRow, clock, facts_lines, readout, rows, summary, uncut_bounds,
    uncut_lines, uncut_rows,
};
pub use session::{Blocked, SimSession};
pub use shade::{
    EAST_STEP, NORTH_STEP, SHALLOW_SLOPE_CELLS, SLOPE_CONTINUATION_RATIO, Shading, edge_is_step,
};
pub use tool::{
    CUT_LENGTH_DIAMETERS, SHANK_LENGTH_DIAMETERS, SHANK_RADIUS_RATIO, TURN_SEGMENTS, ToolMesh,
    ToolPart, ToolVertex, UNDERSIDE_STEPS, tool_mesh,
};
pub use transport::{FRAME_CELLS, OPENING_PLAY_S, Speed, Transport};
