// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's grid (`docs/specs/phase-4/experience.md`, section 9,
//! "Background and grid"; functional spec, capability 72: "a 10 mm grid
//! that thins and coarsens with zoom"). The step is the smallest of 1, 10,
//! 100 and 1000 mm (0.1, 1 and 10 in for an inch project) whose spacing
//! on screen is at least 12 px; minor lines every step, major every ten;
//! the minor lines fade in from nothing at 12 px to full at 28 px. Lines
//! run from project zero, so they meet the stock's edges, and cover the
//! area given (the stock), never the whole pane.

use dc_geometry::{Point, Rect};
use dc_project::Units;

use crate::viewport::camera::Camera;

/// The least on-screen spacing of the minor lines, logical pixels.
const MIN_SPACING_PX: f64 = 12.0;
/// The spacing at which the minor lines reach full strength, logical
/// pixels.
const FULL_SPACING_PX: f64 = 28.0;
/// The steps a millimetre project chooses from, mm.
const MM_STEPS: [f64; 4] = [1.0, 10.0, 100.0, 1000.0];
/// The steps an inch project chooses from, mm (0.1, 1 and 10 in).
const INCH_STEPS: [f64; 3] = [2.54, 25.4, 254.0];
/// Every tenth line is major.
const MAJOR_EVERY: i64 = 10;
/// The most lines either axis draws; a finer step past it draws only the
/// major lines.
const MAX_LINES_PER_AXIS: i64 = 20_000;

/// The grid at one zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// The minor lines' spacing, mm.
    pub step_mm: f64,
    /// How strongly the minor lines draw, 0 to 1.
    pub minor_alpha: f64,
}

/// One grid line, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    /// One end, mm.
    pub from: Point,
    /// The other end, mm.
    pub to: Point,
    /// Every tenth line, drawn in `viewport.grid.major`.
    pub major: bool,
}

impl Grid {
    /// The grid `camera` shows in a project measured in `units`.
    #[must_use]
    pub fn at(camera: &Camera, units: Units) -> Self {
        let steps: &[f64] = match units {
            Units::Mm => &MM_STEPS,
            Units::Inch => &INCH_STEPS,
        };
        let step_mm = steps
            .iter()
            .copied()
            .find(|s| s * camera.px_per_mm >= MIN_SPACING_PX)
            .or_else(|| steps.last().copied())
            .unwrap_or(10.0);
        let spacing_px = step_mm * camera.px_per_mm;
        let minor_alpha =
            ((spacing_px - MIN_SPACING_PX) / (FULL_SPACING_PX - MIN_SPACING_PX)).clamp(0.0, 1.0);
        Self {
            step_mm,
            minor_alpha,
        }
    }

    /// The lines over `area` (mm), vertical then horizontal, each from one
    /// edge of the area to the other.
    #[must_use]
    pub fn lines(&self, area: Rect) -> Vec<GridLine> {
        let mut lines = Vec::new();
        let step = self.step_mm;
        if !(step.is_finite() && step > 0.0) {
            return lines;
        }
        let range = |lo: f64, hi: f64| -> (i64, i64) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "line indices of a stock under 6 m at a step of at least 1 mm"
            )]
            let r = ((lo / step).ceil() as i64, (hi / step).floor() as i64);
            r
        };
        let (x0, x1) = range(area.min.x, area.max.x);
        let (y0, y1) = range(area.min.y, area.max.y);
        let only_major = (x1 - x0).max(y1 - y0) > MAX_LINES_PER_AXIS;
        for k in x0..=x1 {
            let major = k % MAJOR_EVERY == 0;
            if only_major && !major {
                continue;
            }
            #[allow(clippy::cast_precision_loss, reason = "a line index under 2^52")]
            let x = k as f64 * step;
            lines.push(GridLine {
                from: Point::new(x, area.min.y),
                to: Point::new(x, area.max.y),
                major,
            });
        }
        for k in y0..=y1 {
            let major = k % MAJOR_EVERY == 0;
            if only_major && !major {
                continue;
            }
            #[allow(clippy::cast_precision_loss, reason = "a line index under 2^52")]
            let y = k as f64 * step;
            lines.push(GridLine {
                from: Point::new(area.min.x, y),
                to: Point::new(area.max.x, y),
                major,
            });
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{Vec2, approx_eq};

    fn camera(px_per_mm: f64) -> Camera {
        Camera {
            centre_mm: Point::new(0.0, 0.0),
            px_per_mm,
            viewport_px: Vec2::new(800.0, 600.0),
        }
    }

    /// Section 9: the smallest step at least 12 px apart, fading in to 28.
    #[test]
    fn step_coarsens_and_fades_with_zoom() {
        let g = Grid::at(&camera(12.0), Units::Mm);
        assert!(approx_eq(g.step_mm, 1.0, 1e-12) && approx_eq(g.minor_alpha, 0.0, 1e-12));
        let g = Grid::at(&camera(28.0), Units::Mm);
        assert!(approx_eq(g.step_mm, 1.0, 1e-12) && approx_eq(g.minor_alpha, 1.0, 1e-12));
        let g = Grid::at(&camera(2.0), Units::Mm);
        assert!(approx_eq(g.step_mm, 10.0, 1e-12) && approx_eq(g.minor_alpha, 0.5, 1e-12));
        let g = Grid::at(&camera(0.001), Units::Mm);
        assert!(approx_eq(g.step_mm, 1000.0, 1e-12));
        let g = Grid::at(&camera(1.0), Units::Inch);
        assert!(approx_eq(g.step_mm, 25.4, 1e-12));
    }

    /// Lines from project zero over the area only; every tenth major.
    #[test]
    fn lines_cover_the_area_from_zero() {
        let g = Grid {
            step_mm: 10.0,
            minor_alpha: 1.0,
        };
        let lines = g.lines(Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(170.0, 120.0),
        });
        assert_eq!(lines.len(), 18 + 13);
        assert_eq!(lines.iter().filter(|l| l.major).count(), 2 + 2);
        assert!(lines.iter().all(|l| l.from.x >= 0.0 && l.to.x <= 170.0));
    }
}
