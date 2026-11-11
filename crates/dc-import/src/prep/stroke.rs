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
use crate::svg::{self, MM_PER_PX, MmPath};
use crate::{ImportError, ImportWarning};
use dc_geometry::Region;
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

/// What the stroker made of one painted path.
#[derive(Debug, Clone)]
pub(crate) enum Stroked {
    /// Nothing to stroke: the path has no painted stroke, or it draws
    /// nothing (a zero-length subpath with butt caps).
    Nothing,
    /// The stroker returned no outline; the warning says so.
    Failed(ImportWarning),
    /// The nonzero-winding outline, in millimetres, Y-up.
    Outline(MmPath),
}

/// The stroke of `path` as an outline in page millimetres.
///
/// # Errors
///
/// [`ImportError::TooManyVertices`] as [`outline`].
pub(crate) fn outline_mm(
    _path: &PaintedPath,
    _tolerance_mm: f64,
    _budget: &mut Budget,
) -> Result<Stroked, ImportError> {
    Ok(Stroked::Nothing)
}

/// The area of a stroke outline, resolved nonzero.
///
/// # Errors
///
/// [`ImportError::TooManyVertices`] when flattening goes over the budget.
pub(crate) fn area(
    _outline: &MmPath,
    _tolerance_mm: f64,
    _budget: &mut Budget,
) -> Result<Vec<Region>, ImportError> {
    Ok(Vec::new())
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

    // ---- stroke to area (slice 21d, acceptance 61c) ----

    const TOL: f64 = 0.001;

    /// An SVG on a 100 mm page where one user unit is one millimetre.
    fn page(body: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"#
        )
    }

    fn stroke_regions(body: &str) -> Vec<Region> {
        let p = painted(&page(body));
        let mut budget = Budget::new(0, 5_000_000);
        match outline_mm(&p, TOL, &mut budget).expect("within budget") {
            Stroked::Outline(o) => area(&o, TOL, &mut budget).expect("within budget"),
            other => panic!("no outline: {other:?}"),
        }
    }

    fn total(regions: &[Region]) -> f64 {
        dc_geometry::area_mm2(regions)
    }

    fn bbox(regions: &[Region]) -> (f64, f64, f64, f64) {
        let b = regions
            .iter()
            .filter_map(|r| r.outer().polyline().bounds())
            .fold(None::<dc_geometry::Rect>, |acc, b| {
                Some(acc.map_or(b, |a| dc_geometry::Rect {
                    min: dc_geometry::Point::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
                    max: dc_geometry::Point::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
                }))
            })
            .expect("a region");
        (b.min.x, b.min.y, b.max.x, b.max.y)
    }

    fn line(extra: &str) -> Vec<Region> {
        stroke_regions(&format!(
            r#"<path d="M10,50 L30,50" fill="none" stroke="black" stroke-width="2" {extra}/>"#
        ))
    }

    #[test]
    fn round_caps_add_pi() {
        let a = total(&line(r#"stroke-linecap="round""#));
        assert!((a - 43.14).abs() < 0.05, "{a}");
    }

    #[test]
    fn butt_caps() {
        let a = total(&line(""));
        assert!((a - 40.0).abs() < 1e-6, "{a}");
    }

    #[test]
    fn square_caps() {
        let a = total(&line(r#"stroke-linecap="square""#));
        assert!((a - 44.0).abs() < 1e-6, "{a}");
    }

    fn corner(extra: &str) -> Vec<Region> {
        stroke_regions(&format!(
            r#"<path d="M10,10 L30,10 L30,30" fill="none" stroke="black" stroke-width="2" {extra}/>"#
        ))
    }

    #[test]
    fn miter_join_at_a_right_angle() {
        let r = corner(r#"stroke-linejoin="miter""#);
        assert_eq!(r.len(), 1);
        assert!((total(&r) - 80.0).abs() < 1e-6, "{}", total(&r));
    }

    #[test]
    fn round_join_at_a_right_angle() {
        let a = total(&corner(r#"stroke-linejoin="round""#));
        assert!((a - 79.79).abs() < 0.05, "{a}");
    }

    /// Past the miter limit the join is a bevel (the SVG rule); within it
    /// the spike is kept.
    #[test]
    fn miter_limit_falls_back_to_a_bevel() {
        let sharp = |limit: &str| {
            stroke_regions(&format!(
                r#"<path d="M10,50 L50,50 L10,55" fill="none" stroke="black" stroke-width="2" stroke-miterlimit="{limit}"/>"#
            ))
        };
        let (_, _, beveled_x, _) = bbox(&sharp("4"));
        let (_, _, mitered_x, _) = bbox(&sharp("100"));
        assert!(beveled_x < 51.01, "{beveled_x}");
        assert!(mitered_x > 55.0, "{mitered_x}");
    }

    #[test]
    fn dashes_give_one_piece_each() {
        let r = stroke_regions(
            r#"<path d="M10,50 L85,50" fill="none" stroke="black" stroke-width="2" stroke-dasharray="10 5"/>"#,
        );
        assert_eq!(r.len(), 5);
        for piece in &r {
            assert!(
                (piece.area_mm2() - 20.0).abs() < 1e-6,
                "{}",
                piece.area_mm2()
            );
            let b = piece.outer().polyline().bounds().expect("bounds");
            assert!((b.max.x - b.min.x - 10.0).abs() < 1e-6);
        }
    }

    /// The transform is applied to the stroked shape: scaling x by 3
    /// leaves a horizontal line 2 mm wide and makes a vertical one 6 mm.
    #[test]
    fn a_non_uniform_scale_widens_unevenly() {
        let horizontal = stroke_regions(
            r#"<g transform="scale(3,1)"><path d="M3,50 L10,50" fill="none" stroke="black" stroke-width="2"/></g>"#,
        );
        let (x0, y0, x1, y1) = bbox(&horizontal);
        assert!((x1 - x0 - 21.0).abs() < 1e-6 && (y1 - y0 - 2.0).abs() < 1e-6);
        let vertical = stroke_regions(
            r#"<g transform="scale(3,1)"><path d="M10,10 L10,20" fill="none" stroke="black" stroke-width="2"/></g>"#,
        );
        let (x0, y0, x1, y1) = bbox(&vertical);
        assert!((x1 - x0 - 6.0).abs() < 1e-6 && (y1 - y0 - 10.0).abs() < 1e-6);
    }

    #[test]
    fn stroke_under_rotation_keeps_its_width() {
        let r = stroke_regions(
            r#"<g transform="rotate(45 50 50)"><path d="M40,50 L60,50" fill="none" stroke="black" stroke-width="2"/></g>"#,
        );
        assert!((total(&r) - 40.0).abs() < 1e-4, "{}", total(&r));
        let (x0, y0, x1, y1) = bbox(&r);
        let side = 22.0 / std::f64::consts::SQRT_2;
        assert!((x1 - x0 - side).abs() < 1e-3 && (y1 - y0 - side).abs() < 1e-3);
    }

    #[test]
    fn a_zero_width_stroke_is_skipped() {
        let p = painted(&page(
            r#"<path d="M10,10 L30,10 L30,30 Z" fill="black" stroke="black" stroke-width="0"/>"#,
        ));
        assert!(p.stroke.is_none());
        let mut budget = Budget::new(0, 5_000_000);
        let stroked = outline_mm(&p, TOL, &mut budget).expect("within budget");
        assert!(matches!(stroked, Stroked::Nothing), "{stroked:?}");
    }

    /// SVG: a zero-length subpath is a dot under round or square caps and
    /// nothing under butt caps.
    #[test]
    fn a_zero_length_subpath_is_a_dot_only_with_round_or_square_caps() {
        let dot = |cap: &str| {
            let walked = walk(
                page(&format!(
                    r#"<path d="M50,50 L50,50" fill="none" stroke="black" stroke-width="2" stroke-linecap="{cap}"/>"#
                ))
                .as_bytes(),
            )
            .expect("walks");
            let mut budget = Budget::new(0, 5_000_000);
            walked.painted.first().map_or(0.0, |p| {
                match outline_mm(p, TOL, &mut budget).expect("within budget") {
                    Stroked::Outline(o) => total(&area(&o, TOL, &mut budget).expect("area")),
                    _ => 0.0,
                }
            })
        };
        assert!(
            (dot("round") - std::f64::consts::PI).abs() < 0.01,
            "{}",
            dot("round")
        );
        assert!((dot("square") - 4.0).abs() < 1e-6, "{}", dot("square"));
        assert!(dot("butt") < 1e-9, "{}", dot("butt"));
    }

    /// A stroke the stroker returns nothing for is a warning naming the
    /// path, not an error.
    #[test]
    fn stroker_failure_warns_and_skips() {
        let p = painted(&page(
            r#"<path id="wide" d="M10,50 L30,50" fill="none" stroke="black" stroke-width="3e38"/>"#,
        ));
        let mut budget = Budget::new(0, 5_000_000);
        match outline_mm(&p, TOL, &mut budget).expect("within budget") {
            Stroked::Failed(w) => {
                assert_eq!(w.code(), "prep_stroke_failed");
                assert_eq!(
                    w,
                    ImportWarning::PrepStrokeFailed {
                        path: "wide".into()
                    }
                );
            }
            other => panic!("expected a failure: {other:?}"),
        }
    }
}
