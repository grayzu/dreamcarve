// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The view settings (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app"): the camera, what the Viewport draws of a toolpath and how it
//! colours it, and which operations are hidden. View state, not project
//! state (functional spec, acceptance 81): nothing here is saved in the
//! `.dcv` or undone. The camera's methods (`to_screen`, the one Y flip)
//! are slice 4's (#533); this is the record they act on.

use std::collections::BTreeSet;

use dc_geometry::{Point, Vec2};
use dc_project::OperationId;

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

/// The Viewport's camera: a point of the project (mm, Y-up) at the centre
/// of a viewport of `viewport_px` logical pixels, at `px_per_mm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The project point at the viewport's centre, mm.
    pub centre_mm: Point,
    /// Logical pixels per millimetre.
    pub px_per_mm: f64,
    /// The viewport's size, logical pixels.
    pub viewport_px: Vec2,
}

impl Default for Camera {
    /// A 100 × 100 mm stock, the empty project's, filling most of an
    /// 800 × 600 viewport.
    fn default() -> Self {
        Self {
            centre_mm: Point::new(50.0, 50.0),
            px_per_mm: 5.0,
            viewport_px: Vec2::new(800.0, 600.0),
        }
    }
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
