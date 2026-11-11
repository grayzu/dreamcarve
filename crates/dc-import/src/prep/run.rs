// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The image-prep pipeline (steps 1 to 8 of `docs/specs/phase-3/design.md`,
//! "Image prep"): [`prepare`] turns the bytes of an SVG into pieces and a
//! [`PrepReport`], and reads the SVG it would write back before returning
//! (the check lives here, not in `write_svg`, so no caller can skip it). A
//! pure function.

use super::findings::{
    Shape, bounds as findings_bounds, hidden, painted_height_mm, separate_holes, stroke_only,
};
use super::report::{PrepReport, ReadReport, StrokeReport, WidthFigure};
use super::stroke::{self, Budget, Stroked};
use super::walk::{HiddenReason, PaintedPath, Walk, walk};
use super::write;
use super::{
    BACKGROUND_COVER_FRACTION, DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT, FLATTEN_FRACTION,
    GAP_REACH_FRACTION, MAX_THRESHOLD_PCT, MIN_FLATTEN_TOLERANCE_MM, REGIONS_SUGGESTED_BELOW_PCT,
};
use crate::svg::{self, MAX_IMPORT_VERTICES, MmPath};
use crate::{ImportError, ImportWarning, NotConvertible};
use dc_geometry::width::{hole_is_narrower_than, is_narrower_than, local_widths, narrowest_gap};
use dc_geometry::{
    Contour, MAX_COORDINATE_MM, Region, ink_without_spaces, merge_fills, nest, resolve_fill,
    simplify::simplify_regions, spaces_between, union,
};
use kurbo::Shape as _;

/// What the inlay is made of (49c step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepMode {
    /// The ink itself: lettering and logos.
    Shapes,
    /// The enclosed spaces between the lines: line art.
    Regions,
}

/// The options of [`prepare`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrepOptions {
    /// The mode.
    pub mode: PrepMode,
    /// `--min-detail`, percent of the design's height, `0` to
    /// [`super::MAX_THRESHOLD_PCT`]; 0 turns the step off.
    pub min_detail_pct: f64,
    /// `--smooth`, percent of the design's height, likewise.
    pub smooth_pct: f64,
    /// The most vertices the flattened input may have, as
    /// [`crate::SvgImportOptions::max_vertices`]; the output is bounded by
    /// it too.
    pub max_vertices: usize,
}

impl Default for PrepOptions {
    fn default() -> Self {
        Self {
            mode: PrepMode::Shapes,
            min_detail_pct: DEFAULT_MIN_DETAIL_PCT,
            smooth_pct: DEFAULT_SMOOTH_PCT,
            max_vertices: MAX_IMPORT_VERTICES,
        }
    }
}

/// An SVG prepared (49b to 49d): the pieces, the page, the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The pieces in canonical order, Y-up, mm, origin at the page's
    /// bottom-left corner.
    pub pieces: Vec<Region>,
    /// Page width, mm.
    pub page_width_mm: f64,
    /// Page height, mm.
    pub page_height_mm: f64,
    /// What was done.
    pub report: PrepReport,
}

/// The flatten tolerance for a design `height_mm` tall: [`FLATTEN_FRACTION`]
/// of it, never finer than [`MIN_FLATTEN_TOLERANCE_MM`]. Relative to the
/// design because the user scales the result afterwards.
#[must_use]
pub fn flatten_tolerance_mm(height_mm: f64) -> f64 {
    (FLATTEN_FRACTION * height_mm).max(MIN_FLATTEN_TOLERANCE_MM)
}

/// Prepares the SVG in `bytes`: steps 1 to 7 of the design's "Image prep",
/// the walk, strokes to areas, fills resolved into the ink, the mode,
/// clean-up (holes, then pieces, then smoothing, then a last union) and
/// the report, with the pieces in canonical order, so the result does not
/// depend on the order the paths had in the file. Every threshold is a
/// share of the design's height.
///
/// Warnings come in a fixed order: the checks `image-prep` shares with
/// `init` (`svg_hole_as_separate_path`, `svg_stroke_only_path`,
/// `svg_hidden_object`, `svg_object_not_convertible`), then
/// `prep_stroke_failed`, `prep_background_shape`, the mode's
/// (`prep_regions_open_ink` or `prep_regions_suggested`),
/// `prep_smooth_exceeds_detail`, `prep_hole_filled`, `prep_speck_dropped`
/// and `prep_smooth_skipped`.
///
/// # Errors
///
/// - [`ImportError::PrepThresholdRange`] for a threshold outside `0` to
///   [`MAX_THRESHOLD_PCT`], before anything is read;
/// - [`ImportError::PrepNothingLeft`] when nothing visible has a height or
///   nothing survives the clean-up;
/// - [`ImportError::PrepOutputUnreadable`] when the pieces do not nest, or
///   the SVG [`write_svg`](super::write_svg) writes for them does not read
///   back through the importer as the same pieces and holes: a defect;
/// - [`ImportError::TooManyVertices`], and the errors of the walk and of
///   the importer's coordinate checks.
pub fn prepare(
    bytes: &[u8],
    options: &PrepOptions,
) -> Result<(Prepared, Vec<ImportWarning>), ImportError> {
    check_threshold("--min-detail", options.min_detail_pct)?;
    check_threshold("--smooth", options.smooth_pct)?;
    let walked = walk(bytes)?;

    // Step 1: the design's height, exact from the curves, then the
    // tolerance that follows from it.
    for path in &walked.painted {
        let elements = path.geometry.bez.elements();
        if !elements.iter().all(svg::element_is_finite) {
            return Err(ImportError::NonFinite {
                path: path.name.clone(),
            });
        }
        if !elements.iter().all(svg::element_is_within_limit) {
            return Err(ImportError::CoordinateOutOfRange {
                path: path.name.clone(),
                limit_mm: MAX_COORDINATE_MM,
            });
        }
    }
    // The outlines are built before the height is known, charged at the
    // tolerance a bound of it gives (the box of the paths grown by half
    // each stroke's width).
    let bound_mm = painted_height_mm(&walked.painted);
    if !(bound_mm.is_finite() && bound_mm > 0.0) {
        return Err(ImportError::PrepNothingLeft);
    }
    let provisional_mm = flatten_tolerance_mm(bound_mm);
    let mut budget = Budget::new(0, options.max_vertices);
    let mut extent: Option<kurbo::Rect> = None;
    let mut grow = |rect: kurbo::Rect| {
        extent = Some(extent.map_or(rect, |e| e.union(rect)));
    };
    let mut outlines: Vec<Option<MmPath>> = Vec::with_capacity(walked.painted.len());
    let mut warnings = Vec::new();
    let mut failed = Vec::new();
    for path in &walked.painted {
        if path.fill.is_some() {
            budget.spend(svg::estimated_vertices(&path.geometry.bez, provisional_mm))?;
            if !path.geometry.bez.elements().is_empty() {
                grow(path.geometry.bez.bounding_box());
            }
        }
        // Step 2: the stroke's outline, guarded.
        outlines.push(
            // Estimated at the finest tolerance, an upper bound whatever
            // the height turns out to be.
            match stroke::outline_mm(path, MIN_FLATTEN_TOLERANCE_MM, &mut budget)? {
                Stroked::Nothing => None,
                Stroked::Failed(warning) => {
                    failed.push(warning);
                    None
                }
                Stroked::Outline(mm) => {
                    if !mm.bez.elements().iter().all(svg::element_is_finite) {
                        return Err(ImportError::NonFinite {
                            path: path.name.clone(),
                        });
                    }
                    if !mm.bez.elements().is_empty() {
                        grow(mm.bez.bounding_box());
                    }
                    Some(mm)
                }
            },
        );
    }
    let height_mm = extent.map_or(0.0, |e| e.height());
    if !(height_mm.is_finite() && height_mm > 0.0) {
        return Err(ImportError::PrepNothingLeft);
    }
    let tolerance_mm = flatten_tolerance_mm(height_mm);

    // Step 3: each path's areas.
    let mut shapes: Vec<Shape> = Vec::with_capacity(walked.painted.len());
    let mut strokes = StrokeReport::default();
    for (path, outline) in walked.painted.iter().zip(&outlines) {
        let filled = match path.fill {
            Some(fill) => {
                // Refuse at the tolerance the flattening really uses
                // (finer than the provisional one when the design is
                // shorter than the painted bound), then charge what it
                // made.
                let mut trial = budget;
                trial.spend(svg::estimated_vertices(&path.geometry.bez, tolerance_mm))?;
                let profiles = svg::build_profiles(&path.geometry, tolerance_mm);
                budget.spend(profiles.iter().map(|p| p.vertices().len()).sum())?;
                resolve_fill(&profiles, fill.rule, tolerance_mm)
            }
            None => Vec::new(),
        };
        let stroked = match outline {
            Some(mm) => {
                let width_mm = path.stroke.as_ref().map_or(0.0, |s| s.width_mm);
                strokes.count += 1;
                strokes.least_width_mm =
                    Some(strokes.least_width_mm.map_or(width_mm, |w| w.min(width_mm)));
                strokes.greatest_width_mm = Some(
                    strokes
                        .greatest_width_mm
                        .map_or(width_mm, |w| w.max(width_mm)),
                );
                stroke::area(mm, tolerance_mm, &mut budget)?
            }
            None => Vec::new(),
        };
        shapes.push(Shape {
            name: path.name.clone(),
            filled,
            stroked,
        });
    }
    // The checks `init` also makes.
    warnings.extend(separate_holes(&shapes, tolerance_mm));
    warnings.extend(stroke_only(&walked.painted, &shapes));
    warnings.extend(hidden(&walked.hidden));
    warnings.extend(walked.not_convertible.iter().map(|u| {
        ImportWarning::SvgObjectNotConvertible {
            kind: u.kind,
            count: u.count,
            group: u.group.clone(),
        }
    }));
    warnings.extend(failed);

    // The filled areas nest as `init` reads them; the strokes are united
    // with them and with each other, never nested (#597).
    let nested = merge_fills(
        shapes.iter().map(|s| s.filled.clone()).collect(),
        tolerance_mm,
    )
    .map_err(unreadable)?;
    let stroke_areas: Vec<Region> = shapes
        .iter()
        .flat_map(|s| s.stroked.iter().cloned())
        .collect();
    let ink = union(&nested, &stroke_areas, tolerance_mm);
    if ink.is_empty() {
        return Err(ImportError::PrepNothingLeft);
    }
    warnings.extend(background_shapes(&walked.painted, &shapes, &nested, &ink));

    // Step 4: the mode.
    let ink_median_width_mm = median_width_mm(&ink, tolerance_mm);
    let mut pieces = match options.mode {
        PrepMode::Shapes => {
            if let Some(median_width_mm) = ink_median_width_mm
                && median_width_mm < REGIONS_SUGGESTED_BELOW_PCT / 100.0 * height_mm
            {
                warnings.push(ImportWarning::PrepRegionsSuggested {
                    median_width_mm,
                    height_mm,
                });
            }
            ink
        }
        PrepMode::Regions => {
            let open = ink_without_spaces(&ink);
            if open > 0 {
                warnings.push(ImportWarning::PrepRegionsOpenInk { count: open });
            }
            let spaces = spaces_between(&ink, tolerance_mm);
            if spaces.is_empty() {
                return Err(ImportError::PrepNothingLeft);
            }
            spaces
        }
    };

    // Step 5: clean-up, holes, then pieces, then smoothing.
    let min_detail_mm = options.min_detail_pct / 100.0 * height_mm;
    let smooth_mm = options.smooth_pct / 100.0 * height_mm;
    if options.min_detail_pct > 0.0 && options.smooth_pct > options.min_detail_pct / 2.0 {
        warnings.push(ImportWarning::PrepSmoothExceedsDetail {
            smooth_pct: options.smooth_pct,
            min_detail_pct: options.min_detail_pct,
        });
    }
    let (mut filled_holes, mut largest_hole_mm) = (0, 0.0_f64);
    let (mut dropped_pieces, mut largest_piece_mm) = (0, 0.0_f64);
    if min_detail_mm > 0.0 {
        let mut kept: Vec<Region> = Vec::with_capacity(pieces.len());
        for region in pieces {
            let narrow: Vec<&Contour> = region
                .holes()
                .iter()
                .filter(|hole| hole_is_narrower_than(hole, min_detail_mm, tolerance_mm))
                .collect();
            if narrow.is_empty() {
                kept.push(region);
                continue;
            }
            for hole in &narrow {
                filled_holes += 1;
                largest_hole_mm = largest_hole_mm.max(widest_mm(min_detail_mm, |w| {
                    hole_is_narrower_than(hole, w, tolerance_mm)
                }));
            }
            let contours: Vec<Contour> = std::iter::once(region.outer())
                .chain(region.holes().iter().filter(|h| !narrow.contains(h)))
                .cloned()
                .collect();
            let (rebuilt, _) = nest(contours).map_err(unreadable)?;
            kept.extend(rebuilt);
        }
        pieces = Vec::with_capacity(kept.len());
        for region in kept {
            if is_narrower_than(&region, min_detail_mm, tolerance_mm) {
                dropped_pieces += 1;
                largest_piece_mm = largest_piece_mm.max(widest_mm(min_detail_mm, |w| {
                    is_narrower_than(&region, w, tolerance_mm)
                }));
            } else {
                pieces.push(region);
            }
        }
        if pieces.is_empty() {
            return Err(ImportError::PrepNothingLeft);
        }
    }
    let mut smoothed_vertices = 0;
    let mut skipped = 0;
    if smooth_mm > 0.0 {
        let before = vertex_count(&pieces);
        let (simplified, put_back) = simplify_regions(&pieces, smooth_mm);
        smoothed_vertices = before.saturating_sub(vertex_count(&simplified));
        skipped = put_back.len();
        pieces = simplified;
    }
    if filled_holes > 0 {
        warnings.push(ImportWarning::PrepHoleFilled {
            count: filled_holes,
            largest_mm: largest_hole_mm,
        });
    }
    if dropped_pieces > 0 {
        warnings.push(ImportWarning::PrepSpeckDropped {
            count: dropped_pieces,
            largest_mm: largest_piece_mm,
            threshold_mm: min_detail_mm,
        });
    }
    if skipped > 0 {
        warnings.push(ImportWarning::PrepSmoothSkipped { count: skipped });
    }

    // Step 5d and step 7: united once to shed slivers, which also puts the
    // pieces in canonical order; they must still nest.
    let pieces = union(&pieces, &[], tolerance_mm);
    if pieces.is_empty() {
        return Err(ImportError::PrepNothingLeft);
    }
    let contours: Vec<Contour> = pieces
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()).cloned())
        .collect();
    let (renested, _) = nest(contours).map_err(unreadable)?;
    if renested.len() != pieces.len() {
        return Err(ImportError::PrepOutputUnreadable {
            reason: format!(
                "{} pieces nest as {} after the clean-up",
                pieces.len(),
                renested.len()
            ),
        });
    }
    // The output is bounded by the cap too.
    if vertex_count(&pieces) > options.max_vertices {
        return Err(ImportError::TooManyVertices {
            limit: options.max_vertices,
        });
    }

    // Step 6: the report, on the final pieces.
    let thinnest_mm = pieces
        .iter()
        .filter_map(|r| local_widths(r, tolerance_mm))
        .map(|profile| profile.median_mm())
        .reduce(f64::min);
    let gap = narrowest_gap(&pieces, GAP_REACH_FRACTION * height_mm, tolerance_mm);
    let report = PrepReport {
        mode: options.mode,
        read: read_report(&walked),
        strokes,
        design_height_mm: height_mm,
        pieces: pieces.len(),
        holes: pieces.iter().map(|r| r.holes().len()).sum(),
        dropped_pieces,
        filled_holes,
        min_detail_mm,
        smoothed_vertices,
        smooth_mm,
        ink_median_width_mm,
        thinnest_piece: thinnest_mm.and_then(|w| WidthFigure::new(w, height_mm)),
        narrowest_gap: gap.and_then(|g| WidthFigure::new(g.width_mm, height_mm)),
    };
    let prepared = Prepared {
        pieces,
        page_width_mm: walked.page_width_mm,
        page_height_mm: walked.page_height_mm,
        report,
    };
    // Step 8: the output is read back before anything is returned.
    write::verify(&prepared, options.max_vertices)?;
    Ok((prepared, warnings))
}

/// `value_pct` is a threshold the command takes: `0` to
/// [`MAX_THRESHOLD_PCT`].
fn check_threshold(flag: &'static str, value_pct: f64) -> Result<(), ImportError> {
    if (0.0..=MAX_THRESHOLD_PCT).contains(&value_pct) {
        Ok(())
    } else {
        Err(ImportError::PrepThresholdRange { flag, value_pct })
    }
}

/// A geometry failure that the pipeline's own steps should have made
/// impossible.
fn unreadable(error: dc_geometry::GeometryError) -> ImportError {
    ImportError::PrepOutputUnreadable {
        reason: error.to_string(),
    }
}

/// The vertices of every contour of `regions`.
fn vertex_count(regions: &[Region]) -> usize {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().vertices().len())
        .sum()
}

/// The widest point of something `is_narrower(w)` says is under `w`,
/// found by bisection below `limit_mm`, mm.
fn widest_mm(limit_mm: f64, is_narrower: impl Fn(f64) -> bool) -> f64 {
    let (mut low, mut high) = (0.0, limit_mm);
    for _ in 0..14 {
        let mid = f64::midpoint(low, high);
        if is_narrower(mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    f64::midpoint(low, high)
}

/// The length-weighted median of `local_widths` over every region of
/// `ink`, mm; `None` when no region could be measured.
fn median_width_mm(ink: &[Region], tolerance_mm: f64) -> Option<f64> {
    let mut samples: Vec<(f64, f64)> = ink
        .iter()
        .filter_map(|r| local_widths(r, tolerance_mm))
        .flat_map(|profile| profile.samples().to_vec())
        .collect();
    samples.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let total_mm: f64 = samples.iter().map(|s| s.1).sum();
    let mut below_mm = 0.0;
    for &(width_mm, length_mm) in &samples {
        below_mm += length_mm;
        if below_mm >= total_mm / 2.0 {
            return Some(width_mm);
        }
    }
    None
}

/// `prep_background_shape` for every painted path with a filled region
/// whose box covers [`BACKGROUND_COVER_FRACTION`] of the ink's box and that
/// has holes (its own, or after nesting) or sits behind the paths drawn
/// after it.
fn background_shapes(
    painted: &[PaintedPath],
    shapes: &[Shape],
    nested: &[Region],
    ink: &[Region],
) -> Vec<ImportWarning> {
    let Some(ink_box) = ink
        .iter()
        .filter_map(findings_bounds)
        .reduce(|a, b| a.including(b.min).including(b.max))
    else {
        return Vec::new();
    };
    let ink_area_mm2 = (ink_box.max.x - ink_box.min.x) * (ink_box.max.y - ink_box.min.y);
    if !(ink_area_mm2.is_finite() && ink_area_mm2 > 0.0) {
        return Vec::new();
    }
    let covers = |region: &Region| {
        findings_bounds(region).map_or(0.0, |b| {
            (b.max.x - b.min.x) * (b.max.y - b.min.y) / ink_area_mm2
        })
    };
    let nested_hole = nested
        .iter()
        .any(|r| covers(r) >= BACKGROUND_COVER_FRACTION && !r.holes().is_empty());
    let mut out = Vec::new();
    for (i, (path, shape)) in painted.iter().zip(shapes).enumerate() {
        let behind = i + 1 < painted.len();
        let found = shape.filled.iter().map(|r| (covers(r), r)).find(|&(c, r)| {
            c >= BACKGROUND_COVER_FRACTION && (behind || nested_hole || !r.holes().is_empty())
        });
        if let Some((covers, _)) = found {
            out.push(ImportWarning::PrepBackgroundShape {
                path: path.name.clone(),
                covers: covers.min(1.0),
            });
        }
    }
    out
}

/// What the walk read and skipped, as counts.
fn read_report(walked: &Walk) -> ReadReport {
    let count = |reason: HiddenReason| {
        walked
            .hidden
            .iter()
            .filter(|o| o.reason == reason)
            .map(|o| o.shapes)
            .sum()
    };
    let not_convertible = |kind: NotConvertible| {
        walked
            .not_convertible
            .iter()
            .filter(|u| u.kind == kind)
            .map(|u| u.count)
            .sum()
    };
    ReadReport {
        paths: walked.painted.len() + walked.hidden.iter().map(|o| o.shapes).sum::<usize>(),
        hidden: count(HiddenReason::Hidden),
        unpainted: count(HiddenReason::Unpainted),
        zero_opacity: count(HiddenReason::ZeroOpacity),
        display_none: count(HiddenReason::DisplayNone),
        text: not_convertible(NotConvertible::Text),
        images: not_convertible(NotConvertible::Image),
        clip_paths: not_convertible(NotConvertible::ClipPath),
        masks: not_convertible(NotConvertible::Mask),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests"
    )]

    use super::*;
    use crate::prep::{FLATTEN_FRACTION, MAX_THRESHOLD_PCT, MIN_FLATTEN_TOLERANCE_MM};
    use dc_geometry::{area_mm2, intersection};

    const RING_CROSS: &[u8] = include_bytes!("../../../../tests/fixtures/prep/ring-cross.svg");
    const MESSY: &[u8] = include_bytes!("../../../../tests/fixtures/prep/messy.svg");

    /// An SVG page `width` x `height` mm, one user unit a mm, with `body`.
    fn svg(width: f64, height: f64, body: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}mm" height="{height}mm" viewBox="0 0 {width} {height}">{body}</svg>"#
        )
        .into_bytes()
    }

    fn rect(id: &str, x: f64, y: f64, w: f64, h: f64) -> String {
        format!(r#"<rect id="{id}" x="{x}" y="{y}" width="{w}" height="{h}"/>"#)
    }

    fn circle(id: &str, cx: f64, cy: f64, r: f64) -> String {
        format!(r#"<circle id="{id}" cx="{cx}" cy="{cy}" r="{r}"/>"#)
    }

    fn run_with(bytes: &[u8], options: &PrepOptions) -> (Prepared, Vec<ImportWarning>) {
        prepare(bytes, options).expect("prepare")
    }

    fn run(bytes: &[u8]) -> (Prepared, Vec<ImportWarning>) {
        run_with(bytes, &PrepOptions::default())
    }

    fn regions_options() -> PrepOptions {
        PrepOptions {
            mode: PrepMode::Regions,
            ..PrepOptions::default()
        }
    }

    fn holes(p: &Prepared) -> usize {
        p.pieces.iter().map(|r| r.holes().len()).sum()
    }

    fn codes(warnings: &[ImportWarning]) -> Vec<&'static str> {
        warnings.iter().map(ImportWarning::code).collect()
    }

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    /// The ring-and-cross art with strokes `width` mm wide, on a page just
    /// big enough, optionally with the cross cut `short` mm short of the
    /// ring's centre line at each end.
    fn ring_cross(width: f64, short: f64) -> Vec<u8> {
        let half = 29.0 - short;
        svg(
            60.0,
            60.0,
            &format!(
                r##"<g fill="none" stroke="#000" stroke-width="{width}"><circle cx="30" cy="30" r="29"/><line x1="{a}" y1="30" x2="{b}" y2="30"/><line x1="30" y1="{a}" x2="30" y2="{b}"/></g>"##,
                a = 30.0 - half,
                b = 30.0 + half,
            ),
        )
    }

    /// 100 mm tall art with the four things of acceptance 61f: a body with
    /// a 0.1 mm and a 0.3 mm hole, and a 0.1 mm and a 0.3 mm dot beside it.
    fn cleanup_art() -> Vec<u8> {
        svg(
            200.0,
            100.0,
            &[
                rect("body", 0.0, 0.0, 40.0, 100.0),
                circle("hole-small", 10.0, 20.0, 0.05),
                circle("hole-large", 10.0, 60.0, 0.15),
                circle("dot-small", 60.0, 20.0, 0.05),
                circle("dot-large", 70.0, 20.0, 0.15),
            ]
            .concat(),
        )
    }

    // 61d: fills are resolved.

    #[test]
    fn overlapping_squares_are_one_piece() {
        let (p, _) = run(&svg(
            20.0,
            10.0,
            &[
                rect("a", 0.0, 0.0, 10.0, 10.0),
                rect("b", 5.0, 0.0, 10.0, 10.0),
            ]
            .concat(),
        ));
        assert_eq!(p.pieces.len(), 1);
        assert!(
            near(area_mm2(&p.pieces), 150.0, 0.01),
            "{}",
            area_mm2(&p.pieces)
        );
        assert_eq!(holes(&p), 0);
    }

    #[test]
    fn an_island_path_becomes_a_hole() {
        let (p, _) = run(&svg(
            20.0,
            20.0,
            &[
                rect("outer", 0.0, 0.0, 20.0, 20.0),
                rect("inner", 5.0, 5.0, 10.0, 10.0),
            ]
            .concat(),
        ));
        assert_eq!(p.pieces.len(), 1);
        assert_eq!(holes(&p), 1);
        assert!(near(area_mm2(&p.pieces), 300.0, 0.01));
    }

    #[test]
    fn fill_rule_is_per_path() {
        let squares = "M0 0 H20 V20 H0 Z M5 5 H15 V15 H5 Z";
        for (rule, area, hole_count) in [("nonzero", 400.0, 0), ("evenodd", 300.0, 1)] {
            let (p, _) = run(&svg(
                20.0,
                20.0,
                &format!(r#"<path d="{squares}" fill-rule="{rule}"/>"#),
            ));
            assert_eq!(p.pieces.len(), 1, "{rule}");
            assert!(near(area_mm2(&p.pieces), area, 0.01), "{rule}");
            assert_eq!(holes(&p), hole_count, "{rule}");
        }
    }

    #[test]
    fn a_stroke_over_a_fill_is_one_piece_not_a_hole() {
        // A 20 mm square stroked 2 mm: the stroke straddles the edge, so
        // the ink is the 22 mm square (mitre corners), not a ring round a
        // hole.
        let (p, _) = run(&svg(
            22.0,
            22.0,
            r##"<rect x="1" y="1" width="20" height="20" fill="#000" stroke="#000" stroke-width="2"/>"##,
        ));
        assert_eq!(p.pieces.len(), 1);
        assert_eq!(holes(&p), 0);
        assert!(
            near(area_mm2(&p.pieces), 484.0, 0.05),
            "{}",
            area_mm2(&p.pieces)
        );
    }

    #[test]
    fn a_stroked_ring_round_a_filled_dot_keeps_both() {
        let (p, _) = run(&svg(
            22.0,
            22.0,
            &format!(
                r##"<circle cx="11" cy="11" r="10" fill="none" stroke="#000" stroke-width="1"/>{}"##,
                circle("dot", 11.0, 11.0, 3.0)
            ),
        ));
        assert_eq!(p.pieces.len(), 2, "the ring and the dot");
        assert_eq!(holes(&p), 1, "only the ring has a hole");
        let expected = std::f64::consts::PI * (10.5 * 10.5 - 9.5 * 9.5 + 9.0);
        assert!(
            near(area_mm2(&p.pieces), expected, 0.3),
            "{}",
            area_mm2(&p.pieces)
        );
    }

    // 61e: the ring and the cross.

    #[test]
    fn shapes_mode_of_the_ring_and_cross_is_one_piece_with_four_holes() {
        let (p, warnings) = run(RING_CROSS);
        assert_eq!(p.pieces.len(), 1);
        assert_eq!(holes(&p), 4);
        assert!(
            near(area_mm2(&p.pieces), 584.3, 1.0),
            "{}",
            area_mm2(&p.pieces)
        );
        assert!(
            !codes(&warnings).contains(&"prep_regions_suggested"),
            "{warnings:?}"
        );
    }

    #[test]
    fn regions_mode_of_the_ring_and_cross() {
        let (ink, _) = run(RING_CROSS);
        let (p, _) = run_with(RING_CROSS, &regions_options());
        assert_eq!(p.pieces.len(), 4);
        assert!(
            near(area_mm2(&p.pieces), 2243.0, 1.0),
            "{}",
            area_mm2(&p.pieces)
        );
        let areas: Vec<f64> = p.pieces.iter().map(Region::area_mm2).collect();
        for a in &areas {
            assert!(near(*a, areas[0], 0.5), "{areas:?}");
        }
        let disc = std::f64::consts::PI * 30.0 * 30.0;
        assert!(
            near(area_mm2(&p.pieces) + area_mm2(&ink.pieces), disc, 1.0),
            "pieces and ink together are the disc"
        );
    }

    #[test]
    fn regions_mode_with_a_floating_cross_warns_open_ink() {
        let (p, warnings) = run_with(&ring_cross(2.0, 2.0), &regions_options());
        assert_eq!(p.pieces.len(), 1, "one pocket");
        assert_eq!(holes(&p), 1, "with the cross as its hole");
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::PrepRegionsOpenInk { count: 1 })),
            "{warnings:?}"
        );
    }

    #[test]
    fn regions_mode_output_never_overlaps_ink() {
        let (ink, _) = run(RING_CROSS);
        let (p, _) = run_with(RING_CROSS, &regions_options());
        let shared = intersection(&p.pieces, &ink.pieces, 0.003);
        assert!(area_mm2(&shared) < 0.01, "{}", area_mm2(&shared));
    }

    // 61f: clean-up.

    #[test]
    fn specks_and_hairlines_are_dropped() {
        let art = svg(
            200.0,
            100.0,
            &[
                rect("body", 0.0, 0.0, 40.0, 100.0),
                circle("dot-small", 60.0, 20.0, 0.05),
                circle("dot-large", 70.0, 20.0, 0.15),
                rect("hairline", 80.0, 10.0, 0.1, 40.0),
                rect("line", 90.0, 10.0, 0.5, 40.0),
            ]
            .concat(),
        );
        let (p, warnings) = run(&art);
        assert_eq!(
            p.pieces.len(),
            3,
            "body, the 0.3 mm dot and the 0.5 mm line"
        );
        assert_eq!(p.report.dropped_pieces, 2);
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ImportWarning::PrepSpeckDropped { count: 2, threshold_mm, .. }
                if near(*threshold_mm, 0.2, 1e-6)
            )),
            "{warnings:?}"
        );
        let keeps = |x: f64, y: f64| {
            p.pieces
                .iter()
                .any(|r| r.contains_point(dc_geometry::Point::new(x, y)))
        };
        assert!(keeps(70.0, 80.0), "the 0.3 mm dot");
        assert!(keeps(90.25, 60.0), "the 0.5 mm line");
        assert!(!keeps(60.0, 80.0), "the 0.1 mm dot");
        assert!(!keeps(80.05, 60.0), "the hairline");
    }

    #[test]
    fn small_holes_are_filled() {
        let (p, warnings) = run(&cleanup_art());
        let body = p
            .pieces
            .iter()
            .find(|r| r.area_mm2() > 3000.0)
            .expect("body");
        assert_eq!(body.holes().len(), 1, "the 0.3 mm hole is kept");
        assert_eq!(p.report.filled_holes, 1);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::PrepHoleFilled { count: 1, .. })),
            "{warnings:?}"
        );
    }

    #[test]
    fn pieces_at_the_threshold_are_kept() {
        // 0.2 % of 100 mm is 0.2 mm: a bar and a hole exactly that wide
        // stay.
        let art = svg(
            100.0,
            100.0,
            &[
                rect("body", 0.0, 0.0, 40.0, 100.0),
                rect("hole", 10.0, 10.0, 0.2, 5.0),
                rect("bar", 60.0, 10.0, 0.2, 40.0),
            ]
            .concat(),
        );
        let (p, warnings) = run(&art);
        assert_eq!(p.pieces.len(), 2, "{warnings:?}");
        assert_eq!(holes(&p), 1);
        assert_eq!(p.report.dropped_pieces, 0);
        assert_eq!(p.report.filled_holes, 0);
    }

    #[test]
    fn zero_min_detail_keeps_everything() {
        let options = PrepOptions {
            min_detail_pct: 0.0,
            smooth_pct: 0.0,
            ..PrepOptions::default()
        };
        let (p, warnings) = run_with(&cleanup_art(), &options);
        assert_eq!(p.pieces.len(), 3, "the body and both dots");
        assert_eq!(holes(&p), 2, "both holes");
        assert_eq!(p.report.dropped_pieces, 0);
        assert_eq!(p.report.filled_holes, 0);
        assert!(near(p.report.min_detail_mm, 0.0, 0.0));
        assert!(
            !codes(&warnings).iter().any(|c| c.starts_with("prep_")),
            "{warnings:?}"
        );
    }

    #[test]
    fn narrow_necks_in_big_pieces_are_not_repaired() {
        // Two 40 x 100 mm blocks joined by a neck 0.1 mm wide: narrower
        // than the threshold, in a piece that is not.
        let art = svg(
            100.0,
            100.0,
            &[
                rect("left", 0.0, 0.0, 40.0, 100.0),
                rect("right", 60.0, 0.0, 40.0, 100.0),
                rect("neck", 39.0, 49.95, 22.0, 0.1),
            ]
            .concat(),
        );
        let options = PrepOptions {
            smooth_pct: 0.0,
            ..PrepOptions::default()
        };
        let (p, warnings) = run_with(&art, &options);
        assert_eq!(p.pieces.len(), 1);
        assert!(near(area_mm2(&p.pieces), 8000.0 + 20.0 * 0.1, 0.01));
        assert!(
            !codes(&warnings).contains(&"prep_speck_dropped"),
            "{warnings:?}"
        );
    }

    /// A piece 40 mm wide whose top edge rises from (0, 30) to (40, 31.6)
    /// as a staircase of 0.5 mm steps 0.02 mm high, beside a tall bar that
    /// makes the design 100 mm tall. SVG coordinates are Y-down, so the
    /// piece sits at the bottom of the page.
    fn staircase_art() -> Vec<u8> {
        let mut d = String::from("M0 100 H40 V68.4");
        for k in (0..80).rev() {
            let x = f64::from(k) * 0.5;
            d.push_str(&format!(
                " H{:.1} V{:.2}",
                x + 0.5,
                70.0 - f64::from(k) * 0.02
            ));
            d.push_str(&format!(" H{x:.1}"));
        }
        d.push_str(" Z");
        svg(
            100.0,
            100.0,
            &format!(
                r#"<path id="stairs" d="{d}"/>{}"#,
                rect("bar", 60.0, 0.0, 5.0, 100.0)
            ),
        )
    }

    #[test]
    fn smooth_removes_a_staircase_and_keeps_corners() {
        let (p, _) = run(&staircase_art());
        let stairs = p
            .pieces
            .iter()
            .find(|r| r.contains_point(dc_geometry::Point::new(20.0, 10.0)))
            .expect("the staircase piece");
        let vertices = stairs.outer().polyline().vertices();
        assert_eq!(vertices.len(), 4, "a straight top edge: {vertices:?}");
        let has = |x: f64, y: f64| {
            vertices
                .iter()
                .any(|v| near(v.x, x, 0.001) && near(v.y, y, 0.001))
        };
        assert!(has(0.0, 0.0), "the 90 degree corner stays");
        assert!(has(40.0, 0.0));
        assert!(has(0.0, 30.0));
        // The staircase's own first step is a spike of 0.02 mm on the top
        // corner, which is within the threshold and goes with the rest.
        assert!(
            vertices
                .iter()
                .any(|v| near(v.x, 40.0, 0.001) && near(v.y, 31.6, 0.05)),
            "{vertices:?}"
        );
        assert!(p.report.smoothed_vertices > 100);
    }

    #[test]
    fn smooth_zero_leaves_the_staircase() {
        let options = PrepOptions {
            smooth_pct: 0.0,
            ..PrepOptions::default()
        };
        let (p, _) = run_with(&staircase_art(), &options);
        let stairs = p
            .pieces
            .iter()
            .find(|r| r.contains_point(dc_geometry::Point::new(20.0, 10.0)))
            .expect("the staircase piece");
        assert!(stairs.outer().polyline().vertices().len() > 160);
        assert_eq!(p.report.smoothed_vertices, 0);
        assert!(near(p.report.smooth_mm, 0.0, 0.0));
    }

    #[test]
    fn smooth_that_would_cross_is_skipped_with_a_warning() {
        // A slab whose top edge is a square wave 0.02 mm high, between two
        // highs, with a pillar standing in one dip 0.1 mm from the highs:
        // straightening the edge to the highs runs it through the pillar.
        let mut d = String::from("M0 100 V90");
        for k in 0..81 {
            d.push_str(&format!(" H{:.1}", f64::from(k + 1) * 0.5));
            if k < 80 {
                let next = if k % 2 == 0 { 90.02 } else { 90.0 };
                d.push_str(&format!(" V{next}"));
            }
        }
        d.push_str(" V100 Z");
        let art = svg(
            41.0,
            100.0,
            &format!(
                r#"<path id="slab" d="{d}"/>{}"#,
                rect("pillar", 0.6, 0.0, 0.3, 90.015)
            ),
        );
        let options = PrepOptions {
            min_detail_pct: 0.0,
            ..PrepOptions::default()
        };
        let (p, warnings) = run_with(&art, &options);
        assert_eq!(p.pieces.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::PrepSmoothSkipped { count: 1 })),
            "{warnings:?}"
        );
    }

    #[test]
    fn threshold_out_of_range_is_an_error() {
        for (min_detail_pct, smooth_pct, flag, value) in [
            (6.0, 0.05, "--min-detail", 6.0),
            (0.2, -1.0, "--smooth", -1.0),
            (-0.1, 0.0, "--min-detail", -0.1),
            (
                0.2,
                MAX_THRESHOLD_PCT + 0.01,
                "--smooth",
                MAX_THRESHOLD_PCT + 0.01,
            ),
        ] {
            let options = PrepOptions {
                min_detail_pct,
                smooth_pct,
                ..PrepOptions::default()
            };
            match prepare(RING_CROSS, &options) {
                Err(ImportError::PrepThresholdRange { flag: f, value_pct }) => {
                    assert_eq!((f, value_pct), (flag, value));
                    let e = ImportError::PrepThresholdRange { flag: f, value_pct };
                    assert_eq!(e.code(), Some("prep_threshold_range"));
                }
                other => panic!("{other:?}"),
            }
        }
        let nan = PrepOptions {
            smooth_pct: f64::NAN,
            ..PrepOptions::default()
        };
        assert!(matches!(
            prepare(RING_CROSS, &nan),
            Err(ImportError::PrepThresholdRange { .. })
        ));
        // The range is closed.
        let edge = PrepOptions {
            min_detail_pct: MAX_THRESHOLD_PCT,
            smooth_pct: 0.0,
            ..PrepOptions::default()
        };
        let block = svg(100.0, 100.0, &rect("block", 0.0, 0.0, 100.0, 100.0));
        assert!(prepare(&block, &edge).is_ok());
    }

    #[test]
    fn smooth_above_half_the_detail_warns() {
        let options = PrepOptions {
            smooth_pct: 0.2,
            ..PrepOptions::default()
        };
        let (_, warnings) = run_with(RING_CROSS, &options);
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ImportWarning::PrepSmoothExceedsDetail { smooth_pct, min_detail_pct }
                if near(*smooth_pct, 0.2, 1e-12) && near(*min_detail_pct, 0.2, 1e-12)
            )),
            "{warnings:?}"
        );
        let (_, quiet) = run(RING_CROSS);
        assert!(!codes(&quiet).contains(&"prep_smooth_exceeds_detail"));
        // With the detail step off there is nothing for smoothing to
        // remove that the other keeps.
        let off = PrepOptions {
            min_detail_pct: 0.0,
            smooth_pct: 0.2,
            ..PrepOptions::default()
        };
        let (_, quiet) = run_with(RING_CROSS, &off);
        assert!(!codes(&quiet).contains(&"prep_smooth_exceeds_detail"));
    }

    // Errors.

    #[test]
    fn nothing_visible_is_an_error() {
        let hidden = svg(
            10.0,
            10.0,
            r#"<rect width="5" height="5" fill="none"/><g style="display:none"><rect width="5" height="5"/></g><rect width="5" height="5" opacity="0"/>"#,
        );
        for bytes in [hidden, svg(10.0, 10.0, "")] {
            match prepare(&bytes, &PrepOptions::default()) {
                Err(e @ ImportError::PrepNothingLeft) => {
                    assert_eq!(e.code(), Some("prep_nothing_left"));
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn everything_dropped_is_an_error() {
        // Two 0.1 mm dots 100 mm apart: a 100 mm tall design with nothing
        // in it that is 0.2 mm wide.
        let art = svg(
            10.0,
            100.0,
            &[circle("a", 5.0, 0.05, 0.05), circle("b", 5.0, 99.95, 0.05)].concat(),
        );
        assert!(matches!(
            prepare(&art, &PrepOptions::default()),
            Err(ImportError::PrepNothingLeft)
        ));
    }

    // 61h: warnings.

    #[test]
    fn background_shape_is_flagged() {
        let art = svg(
            100.0,
            100.0,
            &[
                r##"<rect id="page" width="100" height="100" fill="#fff"/>"##.to_owned(),
                rect("mark", 30.0, 30.0, 40.0, 40.0),
            ]
            .concat(),
        );
        let (_, warnings) = run(&art);
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ImportWarning::PrepBackgroundShape { path, covers }
                if path == "page" && *covers > 0.98
            )),
            "{warnings:?}"
        );
        // A lone rectangle is the design, not its background.
        let (_, warnings) = run(&svg(100.0, 100.0, &rect("only", 0.0, 0.0, 100.0, 100.0)));
        assert!(
            !codes(&warnings).contains(&"prep_background_shape"),
            "{warnings:?}"
        );
    }

    #[test]
    fn regions_suggested_for_thin_ink_and_not_for_thick() {
        let suggested = |width: f64| {
            let (_, warnings) = run(&ring_cross(width, 0.0));
            warnings
                .iter()
                .any(|w| matches!(w, ImportWarning::PrepRegionsSuggested { .. }))
        };
        assert!(suggested(1.0), "1 mm strokes are about 1.7 % of the height");
        assert!(!suggested(2.0));
        // Only default mode suggests it.
        let (_, warnings) = run_with(&ring_cross(1.0, 0.0), &regions_options());
        assert!(!codes(&warnings).contains(&"prep_regions_suggested"));
    }

    // Limits.

    #[test]
    fn flatten_tolerance_scales_with_the_height() {
        assert!(near(
            flatten_tolerance_mm(100.0),
            100.0 * FLATTEN_FRACTION,
            1e-12
        ));
        assert!(near(
            flatten_tolerance_mm(1000.0),
            1000.0 * FLATTEN_FRACTION,
            1e-12
        ));
        assert!(near(
            flatten_tolerance_mm(10.0),
            MIN_FLATTEN_TOLERANCE_MM,
            1e-12
        ));
        // The same circle ten times the size is as round: about as many
        // vertices, not ten times the number.
        let options = PrepOptions {
            smooth_pct: 0.0,
            ..PrepOptions::default()
        };
        let count = |r: f64| {
            let art = svg(2.0 * r, 2.0 * r, &circle("c", r, r, r));
            let (p, _) = run_with(&art, &options);
            p.pieces[0].outer().polyline().vertices().len()
        };
        let (small, big) = (count(50.0), count(500.0));
        assert!(small > 100, "{small}");
        assert!(small.abs_diff(big) <= 4, "{small} against {big}");
    }

    #[test]
    fn a_coarse_bound_does_not_undercharge_a_fill() {
        // The zero-length stroke draws nothing but inflates the painted
        // bound to 5000 mm, a coarse provisional tolerance; the design is
        // 10 mm tall, so the curves flatten far finer than that.
        let mut d = String::from("M0,0");
        for k in 0..300 {
            let x = f64::from(k % 2) * 10.0;
            d.push_str(&format!(
                " C{x},3 {},7 {x},{}",
                10.0 - x,
                10.0 * f64::from(k + 1) / 300.0
            ));
        }
        let art = svg(
            10.0,
            10.0,
            &format!(
                r#"<path d="{d} Z"/><path d="M5,5 L5,5" stroke="black" stroke-width="5000"/>"#
            ),
        );
        let options = PrepOptions {
            max_vertices: 3000,
            ..PrepOptions::default()
        };
        let result = prepare(&art, &options);
        assert!(
            matches!(result, Err(ImportError::TooManyVertices { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn vertex_cap_is_enforced() {
        let options = PrepOptions {
            max_vertices: 20,
            ..PrepOptions::default()
        };
        let art = svg(100.0, 100.0, &circle("c", 50.0, 50.0, 50.0));
        assert!(matches!(
            prepare(&art, &options),
            Err(ImportError::TooManyVertices { .. })
        ));
        // A stroke is charged before it is built (#597).
        let art = svg(
            100.0,
            100.0,
            r#"<path d="M0,0 L5000,0" fill="none" stroke="black" stroke-width="50" stroke-dasharray="0.001"/>"#,
        );
        let result = prepare(&art, &PrepOptions::default());
        assert!(
            matches!(result, Err(ImportError::TooManyVertices { .. })),
            "{result:?}"
        );
    }

    // 61i: determinism.

    #[test]
    fn output_is_independent_of_document_order() {
        let parts = [
            circle("c", 40.0, 40.0, 30.0),
            rect("a", 10.0, 10.0, 20.0, 20.0),
            rect("b", 20.0, 20.0, 20.0, 20.0),
            r##"<line x1="5" y1="75" x2="95" y2="75" stroke="#000" stroke-width="3"/>"##.to_owned(),
            rect("island", 38.0, 38.0, 4.0, 4.0),
        ];
        let forward = svg(100.0, 100.0, &parts.concat());
        let mut reversed = parts.to_vec();
        reversed.reverse();
        let backward = svg(100.0, 100.0, &reversed.concat());
        let (a, _) = run(&forward);
        let (b, _) = run(&backward);
        assert_eq!(a.pieces, b.pieces);
        assert_eq!(a.report, b.report);
        // Line art, in both modes.
        let lines = [
            r##"<circle cx="30" cy="30" r="29" fill="none" stroke="#000" stroke-width="2"/>"##,
            r##"<line x1="1" y1="30" x2="59" y2="30" stroke="#000" stroke-width="2"/>"##,
            r##"<line x1="30" y1="1" x2="30" y2="59" stroke="#000" stroke-width="2"/>"##,
        ];
        let forward = svg(60.0, 60.0, &lines.concat());
        let mut reversed = lines.to_vec();
        reversed.reverse();
        let backward = svg(60.0, 60.0, &reversed.concat());
        for mode in [PrepMode::Shapes, PrepMode::Regions] {
            let options = PrepOptions {
                mode,
                ..PrepOptions::default()
            };
            assert_eq!(
                run_with(&forward, &options).0.pieces,
                run_with(&backward, &options).0.pieces
            );
        }
    }

    #[test]
    fn same_input_twice_is_identical() {
        for bytes in [RING_CROSS, MESSY] {
            let first = run(bytes);
            let second = run(bytes);
            assert_eq!(first.0, second.0);
            assert_eq!(first.1, second.1);
        }
    }

    #[test]
    fn crlf_input_gives_the_same_bytes() {
        let text = String::from_utf8(RING_CROSS.to_vec()).unwrap();
        let crlf = text.replace('\n', "\r\n");
        assert_ne!(crlf.as_bytes(), RING_CROSS);
        assert_eq!(run(RING_CROSS).0, run(crlf.as_bytes()).0);
    }

    /// The sign's stroked outlines union into numerical slivers (#611):
    /// they are measured without a panic and never reach the pieces.
    #[test]
    fn the_sign_prepares_without_a_panic() {
        let sign = include_bytes!("../../../../tests/fixtures/sign.svg");
        for mode in [PrepMode::Shapes, PrepMode::Regions] {
            let options = PrepOptions {
                mode,
                ..PrepOptions::default()
            };
            match prepare(sign, &options) {
                Ok((p, _)) => assert!(!p.pieces.is_empty()),
                Err(ImportError::PrepNothingLeft) => {}
                Err(other) => panic!("{other:?}"),
            }
        }
    }

    // 61b: visible geometry only.

    #[test]
    fn messy_svg_reports_what_it_skipped_and_leaves_it_out() {
        let (p, warnings) = run(MESSY);
        let r = p.report.read;
        assert_eq!(r.display_none, 1, "the hidden layer's one object");
        assert_eq!((r.text, r.images, r.clip_paths, r.masks), (1, 1, 1, 0));
        assert_eq!((r.hidden, r.unpainted, r.zero_opacity), (0, 0, 0));
        assert!(r.paths >= 7, "{r:?}");
        let found = codes(&warnings);
        for code in [
            "svg_hidden_object",
            "svg_object_not_convertible",
            "svg_hole_as_separate_path",
            "svg_stroke_only_path",
            "prep_speck_dropped",
        ] {
            assert!(found.contains(&code), "{code} in {found:?}");
        }
        assert_eq!(
            found
                .iter()
                .filter(|c| **c == "svg_object_not_convertible")
                .count(),
            3
        );
        // The squares (160), the 20 mm square with its hole (300), the
        // 1.2 mm stroke 50 mm long (60) and the clipped square, imported
        // unclipped (400); the 0.1 mm dot is dropped and the hidden square
        // is not there.
        assert!(
            near(area_mm2(&p.pieces), 920.0, 0.5),
            "{}",
            area_mm2(&p.pieces)
        );
        // The hidden square is at x 70 to 80, y 20 to 30 down the page.
        let ghost = dc_geometry::Point::new(75.0, 75.0);
        assert!(!p.pieces.iter().any(|r| r.contains_point(ghost)));
        assert_eq!(p.report.strokes.count, 1);
    }
}
