// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The stroker's resource guard (sub-phase 3.4): the outline of a painted
//! path's stroke, built in user space by tiny-skia's stroker, the one the
//! SVG renderers draw with, and charged to the import's vertex cap
//! *before* it is built, so a short SVG with a long path, a tiny dash
//! period and a wide stroke cannot allocate millions of vertices.
//!
//! Slice 21d (stroke to area, #597) builds on [`outline`] and [`Budget`].

use super::walk::{PaintedPath, StrokePaint, mean_axis_scale};
use crate::ImportError;
use crate::svg::{self, MM_PER_PX};
use usvg::tiny_skia_path::Path;

/// The vertices an import may still produce: the running total against
/// the cap ([`crate::SvgImportOptions::max_vertices`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    used: usize,
    limit: usize,
}

impl Budget {
    /// A budget of `limit` vertices with `used` already spent.
    pub(crate) const fn new(used: usize, limit: usize) -> Self {
        Self { used, limit }
    }

    /// Spends `vertices`.
    ///
    /// # Errors
    ///
    /// [`ImportError::TooManyVertices`] when that goes over the limit.
    pub(crate) fn spend(&mut self, vertices: usize) -> Result<(), ImportError> {
        self.used = self.used.saturating_add(vertices);
        if self.used > self.limit {
            Err(ImportError::TooManyVertices { limit: self.limit })
        } else {
            Ok(())
        }
    }
}

/// Vertices one round join or cap may add per input segment, a bound.
const PER_SEGMENT_FACTOR: usize = 12;
/// Vertices one dash may add (its two ends and caps), a bound.
const PER_DASH_VERTICES: usize = 16;

/// The stroke outline of `path`, in user space; `None` when the stroker
/// cannot produce one. Charges `budget` an estimate before any outline is
/// built (the path's segments times a per-segment bound, and the dash
/// count, from the path's length over the dash period), then replaces it
/// with the outline's own count.
///
/// # Errors
///
/// [`ImportError::TooManyVertices`] when the estimate or the outline goes
/// over the budget.
pub(crate) fn outline(
    path: &PaintedPath,
    stroke: &StrokePaint,
    tolerance_mm: f64,
    budget: &mut Budget,
) -> Result<Option<Path>, ImportError> {
    let mut skia = stroke.stroke.to_tiny_skia();
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a resolution hint for the stroker"
    )]
    let res_scale = (mean_axis_scale(path.to_mm) / MM_PER_PX).clamp(1e-3, 1e6) as f32;

    let segments =
        svg::estimated_vertices(&path.geometry.bez, tolerance_mm).max(path.user_path.len());
    let mut estimate = segments.saturating_mul(PER_SEGMENT_FACTOR);
    if skia.dash.is_some() {
        // Every dash is its own subpath. The control polygon is at least
        // as long as the path, so this over-counts, never under-counts.
        let period: f64 = stroke
            .stroke
            .dasharray()
            .map_or(0.0, |d| d.iter().map(|v| f64::from(*v)).sum());
        let pieces = stroke.stroke.dasharray().map_or(0, <[f32]>::len);
        let dashes = if period > 0.0 && period.is_finite() {
            let n = control_length(&path.user_path) / period * pieces as f64;
            if n.is_finite() && n < usize::MAX as f64 {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a bounded, non-negative estimate"
                )]
                let n = n.ceil() as usize;
                n
            } else {
                usize::MAX
            }
        } else {
            // A dash array that sums to nothing is not applied.
            0
        };
        estimate = estimate.saturating_add(dashes.saturating_mul(PER_DASH_VERTICES));
    }
    // Refuse before anything is allocated.
    let mut trial = *budget;
    trial.spend(estimate)?;

    let dashed;
    let source = match skia.dash.take() {
        Some(dash) => {
            let Some(p) = path.user_path.dash(&dash, res_scale) else {
                return Ok(None);
            };
            dashed = p;
            &dashed
        }
        None => &path.user_path,
    };
    let Some(outline) = source.stroke(&skia, res_scale) else {
        return Ok(None);
    };
    budget.spend(outline.len())?;
    Ok(Some(outline))
}

/// The length of the path's control polygon in user units: at least the
/// path's own length.
fn control_length(path: &Path) -> f64 {
    let points = path.points();
    points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(f64::from(b.x - a.x).hypot(f64::from(b.y - a.y))),
            _ => None,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "tests"
    )]

    use super::super::walk::walk;
    use super::*;

    fn painted(svg: &str) -> PaintedPath {
        walk(svg.as_bytes())
            .expect("walks")
            .painted
            .into_iter()
            .next()
            .expect("one painted path")
    }

    /// A tiny dash period on a long path is refused from an estimate,
    /// quickly and without building the outline.
    #[test]
    fn tiny_dash_period_on_a_long_path_is_refused() {
        let path = painted(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="M0,0 L50000,0" fill="none" stroke="black" stroke-width="50" stroke-dasharray="0.001"/></svg>"#,
        );
        let stroke = path.stroke.clone().expect("stroked");
        let started = std::time::Instant::now();
        let mut budget = Budget::new(0, 5_000_000);
        let result = outline(&path, &stroke, 0.01, &mut budget);
        assert!(
            matches!(
                result,
                Err(ImportError::TooManyVertices { limit: 5_000_000 })
            ),
            "{result:?}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    /// An ordinary stroke is built and charged for its outline.
    #[test]
    fn an_ordinary_stroke_is_built_and_charged() {
        let path = painted(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="M10,10 L90,90" fill="none" stroke="black" stroke-width="2"/></svg>"#,
        );
        let stroke = path.stroke.clone().expect("stroked");
        let mut budget = Budget::new(0, 5_000_000);
        let built = outline(&path, &stroke, 0.01, &mut budget).expect("within budget");
        assert!(built.is_some());
        assert!(budget.used > 0 && budget.used < 100);
    }
}
