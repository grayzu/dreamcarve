// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The report of an image-prep run (functional 49e): data only, counts and
//! millimetres, with `None` for what could not be measured. Rendering it as
//! text is `dc-job`'s (`docs/specs/phase-3/design.md`, "Image prep").

use super::{PrepMode, REPORT_HEIGHTS_MM, REPORT_MIN_FEATURE_MM};

/// What the walk read and skipped (49c step 1), counts of objects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// Every path of the document: those drawn and those skipped (a
    /// `display:none` element counts the shapes in it).
    pub paths: usize,
    /// Paths whose `visibility` is not `visible`.
    pub hidden: usize,
    /// Paths that neither fill nor stroke anything that paints.
    pub unpainted: usize,
    /// Paths whose own or an ancestor's opacity is 0.
    pub zero_opacity: usize,
    /// Shapes in `display:none` elements (hidden layers).
    pub display_none: usize,
    /// Text elements, not converted.
    pub text: usize,
    /// Images, not converted.
    pub images: usize,
    /// Clip paths, not applied.
    pub clip_paths: usize,
    /// Masks, not applied.
    pub masks: usize,
}

/// The strokes made into areas (49c step 2).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StrokeReport {
    /// How many painted strokes became an area.
    pub count: usize,
    /// The narrowest stroke, mm, after the transform; `None` without
    /// strokes.
    pub least_width_mm: Option<f64>,
    /// The widest stroke, mm; `None` without strokes.
    pub greatest_width_mm: Option<f64>,
}

/// One width the report names, and what it becomes at other sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidthFigure {
    /// The width, mm, at the design's own height.
    pub width_mm: f64,
    /// The width at each of [`REPORT_HEIGHTS_MM`]: `width ÷ design height ×
    /// h`.
    pub at_heights_mm: [f64; 3],
    /// The design height at which the width reaches
    /// [`REPORT_MIN_FEATURE_MM`]: `REPORT_MIN_FEATURE_MM ÷ (width ÷
    /// design height)`. A guide, not a limit.
    pub reaches_guide_at_mm: f64,
}

impl WidthFigure {
    /// The figure for `width_mm` on a design `height_mm` tall; `None` when
    /// either is not a positive finite number.
    #[must_use]
    pub fn new(width_mm: f64, height_mm: f64) -> Option<Self> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !(positive(width_mm) && positive(height_mm)) {
            return None;
        }
        let share = width_mm / height_mm;
        Some(Self {
            width_mm,
            at_heights_mm: REPORT_HEIGHTS_MM.map(|h| share * h),
            reaches_guide_at_mm: REPORT_MIN_FEATURE_MM / share,
        })
    }
}

/// What an image-prep run did (49e): informational, nothing in it is
/// enforced.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepReport {
    /// The mode the run was in.
    pub mode: PrepMode,
    /// What was read and skipped.
    pub read: ReadReport,
    /// The strokes made into areas.
    pub strokes: StrokeReport,
    /// The design's height, mm: the visible geometry's box, strokes
    /// included; every threshold is a share of it.
    pub design_height_mm: f64,
    /// Pieces in the result.
    pub pieces: usize,
    /// Holes in the result's pieces.
    pub holes: usize,
    /// Pieces dropped under the minimum detail.
    pub dropped_pieces: usize,
    /// Holes filled under the minimum detail.
    pub filled_holes: usize,
    /// The minimum detail, mm (0 when off).
    pub min_detail_mm: f64,
    /// Vertices smoothing removed.
    pub smoothed_vertices: usize,
    /// The smoothing threshold, mm (0 when off).
    pub smooth_mm: f64,
    /// The length-weighted median width of the ink, mm, before the mode
    /// was applied; `None` when it could not be measured.
    pub ink_median_width_mm: Option<f64>,
    /// The thinnest piece: the least of the final pieces' median widths,
    /// so a tapering tip does not count. `None` when no piece could be
    /// measured.
    pub thinnest_piece: Option<WidthFigure>,
    /// The narrowest gap between the final pieces, within a quarter of the
    /// height; in regions mode that is the board ridge between pockets.
    /// `None` when no two outlines lie that close.
    pub narrowest_gap: Option<WidthFigure>,
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
    use crate::prep::{PrepOptions, prepare};

    const RING_CROSS: &[u8] = include_bytes!("../../../../tests/fixtures/prep/ring-cross.svg");

    fn report(mode: PrepMode) -> PrepReport {
        let options = PrepOptions {
            mode,
            ..PrepOptions::default()
        };
        prepare(RING_CROSS, &options).expect("prepare").0.report
    }

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn design_height_and_counts() {
        let r = report(PrepMode::Shapes);
        assert!(
            near(r.design_height_mm, 60.0, 0.005),
            "{}",
            r.design_height_mm
        );
        assert_eq!(r.mode, PrepMode::Shapes);
        assert_eq!(r.read.paths, 3);
        assert_eq!(
            (
                r.read.hidden,
                r.read.unpainted,
                r.read.zero_opacity,
                r.read.display_none
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(r.strokes.count, 3);
        assert!(near(r.strokes.least_width_mm.unwrap(), 2.0, 1e-3));
        assert!(near(r.strokes.greatest_width_mm.unwrap(), 2.0, 1e-3));
        assert_eq!((r.pieces, r.holes), (1, 4));
        // Counts that agree with 61f: nothing here is under 0.2 % of 60 mm.
        assert_eq!((r.dropped_pieces, r.filled_holes), (0, 0));
        assert!(near(r.min_detail_mm, 0.12, 1e-6), "{}", r.min_detail_mm);
        assert!(near(r.smooth_mm, 0.03, 1e-6), "{}", r.smooth_mm);
        assert!(near(r.ink_median_width_mm.unwrap(), 2.0, 0.02));
        let regions = report(PrepMode::Regions);
        assert_eq!(regions.mode, PrepMode::Regions);
        assert_eq!((regions.pieces, regions.holes), (4, 0));
    }

    #[test]
    fn widths_scale_to_100_200_300() {
        let gap = report(PrepMode::Regions).narrowest_gap.unwrap();
        for (got, want) in gap.at_heights_mm.iter().zip([3.33, 6.67, 10.00]) {
            assert!(near(*got, want, 0.01), "{:?}", gap.at_heights_mm);
        }
        let thinnest = report(PrepMode::Shapes).thinnest_piece.unwrap();
        for (got, want) in thinnest.at_heights_mm.iter().zip([3.33, 6.67, 10.00]) {
            assert!(near(*got, want, 0.01), "{:?}", thinnest.at_heights_mm);
        }
    }

    #[test]
    fn reaching_height_for_the_guide_width() {
        let gap = report(PrepMode::Regions).narrowest_gap.unwrap();
        assert!(
            near(gap.reaches_guide_at_mm, 45.0, 0.5),
            "{}",
            gap.reaches_guide_at_mm
        );
        // The formula: 1.5 mm at a width that is a share of the height.
        let figure = WidthFigure::new(2.0, 60.0).unwrap();
        assert!(near(figure.reaches_guide_at_mm, 45.0, 1e-9));
        assert!(near(figure.at_heights_mm[1], 6.666_666_666, 1e-6));
    }

    #[test]
    fn ridge_is_the_narrowest_gap_in_regions_mode() {
        let gap = report(PrepMode::Regions).narrowest_gap.unwrap();
        assert!(near(gap.width_mm, 2.0, 0.02), "{}", gap.width_mm);
    }

    #[test]
    fn thinnest_piece_is_the_least_median() {
        let thinnest = report(PrepMode::Shapes).thinnest_piece.unwrap();
        assert!(near(thinnest.width_mm, 2.0, 0.02), "{}", thinnest.width_mm);
    }

    #[test]
    fn unmeasurable_widths_are_none() {
        // One lone rectangle: no second outline within reach, so no gap.
        let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20"><rect width="40" height="20"/></svg>"#;
        let (p, _) = prepare(bytes, &PrepOptions::default()).unwrap();
        assert_eq!(p.report.narrowest_gap, None);
        assert!(p.report.thinnest_piece.is_some());
        // Nothing to scale: a width or a height that is not positive.
        for (width, height) in [
            (0.0, 10.0),
            (-1.0, 10.0),
            (1.0, 0.0),
            (f64::NAN, 10.0),
            (1.0, f64::INFINITY),
        ] {
            assert_eq!(WidthFigure::new(width, height), None, "{width} {height}");
        }
    }
}
