// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/viewTransform.ts
//   src/components/canvas/usePointerGestures.ts (handleWheelEvent)

//! The Viewport's camera (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app", `Camera`; functional spec, capability 72): which project point
//! is at the pane's centre and how many logical pixels a millimetre takes.
//! [`Camera::to_screen`] is the one place Y flips
//! (`docs/rust-guidelines.md`, "GUI"): project space is Y-up millimetres,
//! a [`Pos`] is Y-down logical pixels from the pane's top-left corner.
//!
//! From PureCutCNC: zoom about the cursor (the world point under it stays
//! under it) and fit with a margin round the bounds. Upstream's canvas is
//! Y-down like its geometry, so it needs no flip; here the flip is
//! re-derived, and the camera keeps a centre and a scale rather than
//! upstream's zoom and pan over a stock-fitted base, which makes the zoom
//! limits absolute.

use dc_geometry::{Point, Rect, Vec2};
use dc_project::Project;

use crate::scene::Scene;

/// The space [`Camera::fit`] leaves round what it frames, logical pixels
/// (slice 3's stock fit used the same).
pub const FIT_MARGIN_PX: f64 = 48.0;
/// The farthest out the camera zooms: a 6-m sheet
/// (`dc_geometry::MAX_COORDINATE_MM`) in 300 px.
pub const MIN_PX_PER_MM: f64 = 0.05;
/// The closest in: 2000 px per mm, a micron a pixel at 2× scale.
pub const MAX_PX_PER_MM: f64 = 2000.0;
/// What [`Camera::fit`] treats as the least extent of what it frames, mm,
/// so a single point or a straight line still gives a finite zoom.
const MIN_FIT_EXTENT_MM: f64 = 1.0;
/// Screen Y per project Y: the one flip.
const SCREEN_Y_PER_PROJECT_Y: f64 = -1.0;

/// A point on the Viewport pane: logical pixels from its top-left corner,
/// Y down, as the window reports the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos {
    /// Right from the pane's left edge, logical pixels.
    pub x_px: f64,
    /// Down from the pane's top edge, logical pixels.
    pub y_px: f64,
}

impl Pos {
    /// The pane point (`x_px`, `y_px`).
    #[must_use]
    pub const fn new(x_px: f64, y_px: f64) -> Self {
        Self { x_px, y_px }
    }

    /// Distance to `other`, logical pixels.
    #[must_use]
    pub fn distance_to(self, other: Self) -> f64 {
        (other.x_px - self.x_px).hypot(other.y_px - self.y_px)
    }
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

impl Camera {
    /// Where project point `p` (mm, Y-up) is on the pane: the one Y flip.
    #[must_use]
    pub fn to_screen(&self, p: Point) -> Pos {
        Pos {
            x_px: self.viewport_px.x / 2.0 + (p.x - self.centre_mm.x) * self.px_per_mm,
            y_px: self.viewport_px.y / 2.0
                + SCREEN_Y_PER_PROJECT_Y * (p.y - self.centre_mm.y) * self.px_per_mm,
        }
    }

    /// The project point (mm, Y-up) at pane point `s`; the inverse of
    /// [`Camera::to_screen`].
    #[must_use]
    pub fn to_world(&self, s: Pos) -> Point {
        Point::new(
            self.centre_mm.x + (s.x_px - self.viewport_px.x / 2.0) / self.px_per_mm,
            self.centre_mm.y
                + SCREEN_Y_PER_PROJECT_Y * (s.y_px - self.viewport_px.y / 2.0) / self.px_per_mm,
        )
    }

    /// `px` logical pixels in millimetres at this zoom.
    #[must_use]
    pub fn px_to_mm(&self, px: f64) -> f64 {
        px / self.px_per_mm
    }

    /// Scales the zoom by `factor` about pane point `s`, so the project
    /// point under `s` stays there (a wheel notch, a pinch). The scale
    /// stops at [`MIN_PX_PER_MM`] and [`MAX_PX_PER_MM`]; a factor that is
    /// not finite and positive changes nothing.
    pub fn zoom_about(&mut self, s: Pos, factor: f64) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let fixed = self.to_world(s);
        self.px_per_mm = (self.px_per_mm * factor).clamp(MIN_PX_PER_MM, MAX_PX_PER_MM);
        // Put `fixed` back under `s` at the new scale.
        let drifted = self.to_world(s);
        self.centre_mm = self.centre_mm + (fixed - drifted);
    }

    /// Moves the content by (`dx_px`, `dy_px`) logical pixels on screen,
    /// as a drag does: the project point under the pointer follows it.
    pub fn pan(&mut self, dx_px: f64, dy_px: f64) {
        if !(dx_px.is_finite() && dy_px.is_finite()) {
            return;
        }
        self.centre_mm = Point::new(
            self.centre_mm.x - dx_px / self.px_per_mm,
            self.centre_mm.y - SCREEN_Y_PER_PROJECT_Y * dy_px / self.px_per_mm,
        );
    }

    /// Frames `bounds` (mm): centred, at the largest zoom that leaves
    /// [`FIT_MARGIN_PX`] round it on both axes, within the zoom limits.
    /// Extents under a millimetre count as one.
    pub fn fit(&mut self, bounds: Rect) {
        let room = |px: f64| (px - 2.0 * FIT_MARGIN_PX).max(1.0);
        let width_mm = bounds.width_mm().max(MIN_FIT_EXTENT_MM);
        let height_mm = bounds.height_mm().max(MIN_FIT_EXTENT_MM);
        let px_per_mm = (room(self.viewport_px.x) / width_mm)
            .min(room(self.viewport_px.y) / height_mm)
            .clamp(MIN_PX_PER_MM, MAX_PX_PER_MM);
        if !px_per_mm.is_finite() {
            return;
        }
        self.px_per_mm = px_per_mm;
        self.centre_mm = Point::new(
            (bounds.min.x + bounds.max.x) / 2.0,
            (bounds.min.y + bounds.max.y) / 2.0,
        );
    }
}

/// What Fit (F) frames (capability 72, "*Fit* (F) to the stock and
/// shapes"): the stock's footprint and every visible shape's outlines.
#[must_use]
pub fn fit_bounds(project: &Project, scene: &Scene) -> Rect {
    let stock = Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(project.stock.width_mm, project.stock.height_mm),
    };
    match scene.shapes_bounds() {
        Some(shapes) => stock.including(shapes.min).including(shapes.max),
        None => stock,
    }
}
