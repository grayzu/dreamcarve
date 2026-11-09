// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Viewport geometry (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app" and "Module layout"): the camera, picking, the grid and the
//! transform handles, as pure functions on the [`Scene`](crate::Scene),
//! the project and the [`Camera`](camera::Camera). The shell's renderer
//! only draws what these compute: the direction arrows' placement
//! ([`arrows`], slice 16) and the placed tabs ([`tabs`], slice 17) too.

pub mod arrows;
pub mod camera;
pub mod grid;
pub mod handles;
pub mod pick;
pub mod tabs;
