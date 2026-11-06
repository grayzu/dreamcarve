// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The view settings (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app"): the camera, what the Viewport draws of a toolpath and how it
//! colours it, and which operations are hidden. View state, not project
//! state (functional spec, acceptance 81): nothing here is saved in the
//! `.dcv` or undone. The camera is [`crate::viewport::camera`]'s.

use std::collections::BTreeSet;

use dc_project::OperationId;

pub use crate::viewport::camera::Camera;

/// What the user is looking at and how.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    /// Where the Viewport looks and how closely.
    pub camera: Camera,
    /// What the Viewport draws of a toolpath.
    pub visibility: Visibility,
    /// How the Viewport colours toolpaths.
    pub colour: ColourMode,
    /// Draw only the selected operation's toolpath.
    pub only_selected: bool,
    /// Draw each program's extents box.
    pub extents: bool,
    /// Operations whose toolpath is hidden (the eye in the list).
    pub hidden_ops: BTreeSet<OperationId>,
}

/// What the Viewport draws of a toolpath. Everything but the arrows is on
/// by default, as in `gui.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    /// Cutting moves.
    pub cuts: bool,
    /// Rapid moves.
    pub rapids: bool,
    /// Plunges.
    pub plunges: bool,
    /// Direction arrows along the cuts.
    pub arrows: bool,
    /// Where each descent starts.
    pub start_points: bool,
    /// The stretches over tabs.
    pub tab_marks: bool,
    /// The grid.
    pub grid: bool,
    /// Each program's extents box.
    pub extents: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            cuts: true,
            rapids: true,
            plunges: true,
            arrows: false,
            start_points: true,
            tab_marks: true,
            grid: true,
            extents: false,
        }
    }
}

/// How the Viewport colours toolpaths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColourMode {
    /// One colour per operation, the preview's palette.
    #[default]
    ByOperation,
    /// By cutting depth.
    ByDepth,
}
