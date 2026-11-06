// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Viewport geometry (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app" and "Module layout"): the camera, picking, the grid and the
//! transform handles, as pure functions on the [`Scene`](crate::Scene),
//! the project and the [`Camera`](camera::Camera). The shell's renderer
//! only draws what these compute. Direction arrows and tab dragging join
//! with slices 16 and 17.

pub mod camera;
pub mod grid;
pub mod handles;
pub mod pick;
