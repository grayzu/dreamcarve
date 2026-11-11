// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What `dreamcarve image-prep` prints about a run, line for line
//! (functional 49e; `docs/specs/phase-3/design.md`, "Image prep"). Here and
//! not in the CLI so the app's Image Prep panel shows the same lines. The
//! shell prints them; nothing here does I/O.

use dc_import::ImportWarning;
use dc_import::prep::PrepReport;

/// The report's lines.
#[must_use]
pub fn lines(_report: &PrepReport, _input: &str, _output: &str) -> Vec<String> {
    Vec::new()
}

/// The report as text, `\n` between lines and none after the last.
#[must_use]
pub fn render(report: &PrepReport, input: &str, output: &str) -> String {
    lines(report, input, output).join("\n")
}

/// `warning: <code>: <message>` for a warning of an `image-prep` run.
#[must_use]
pub fn warning_line(_warning: &ImportWarning) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, reason = "tests")]

    use super::*;
    use dc_import::prep::{PrepMode, PrepOptions, ReadReport, StrokeReport, WidthFigure, prepare};

    /// The report of functional 49e's example.
    fn eagle() -> PrepReport {
        PrepReport {
            mode: PrepMode::Regions,
            read: ReadReport {
                paths: 214,
                hidden: 3,
                unpainted: 12,
                ..ReadReport::default()
            },
            strokes: StrokeReport {
                count: 187,
                least_width_mm: Some(0.35),
                greatest_width_mm: Some(1.2),
            },
            design_height_mm: 98.4,
            pieces: 41,
            holes: 6,
            dropped_pieces: 17,
            filled_holes: 3,
            min_detail_mm: 0.197,
            smoothed_vertices: 2400,
            smooth_mm: 0.049,
            ink_median_width_mm: Some(1.0),
            thinnest_piece: WidthFigure::new(3.10, 98.4),
            narrowest_gap: WidthFigure::new(0.55, 98.4),
        }
    }

    #[test]
    fn render_matches_the_example() {
        let expected = "\
Prepared eagle.svg → eagle-inlay.svg (regions)
  Read: 214 paths, of them 3 hidden and 12 unpainted skipped; 0 text, 0 images, 0 clip paths, 0 masks
  Strokes made into areas: 187, 0.35 to 1.20 mm wide
  Design height: 98.40 mm
  Result: 41 pieces, 6 holes
  Dropped: 17 pieces below 0.20 % of the height (0.197 mm), 3 holes filled; smoothed within 0.05 % (0.049 mm)
  Thinnest piece: 3.10 mm (3.15 % of the height)
  Narrowest gap, the board ridge between pockets: 0.55 mm (0.56 %)
  At 100 / 200 / 300 mm tall: thinnest piece 3.15 / 6.30 / 9.45 mm; narrowest gap 0.56 / 1.12 / 1.68 mm
  The narrowest gap reaches 1.5 mm at 268 mm tall.";
        assert_eq!(render(&eagle(), "eagle.svg", "eagle-inlay.svg"), expected);
    }

    #[test]
    fn omits_lines_with_nothing_to_say() {
        let mut report = eagle();
        report.mode = PrepMode::Shapes;
        report.strokes = StrokeReport::default();
        report.dropped_pieces = 0;
        report.filled_holes = 0;
        report.smoothed_vertices = 0;
        report.read.hidden = 0;
        report.read.unpainted = 0;
        report.narrowest_gap = None;
        let text = render(&report, "a.svg", "a-inlay.svg");
        assert!(
            text.starts_with("Prepared a.svg → a-inlay.svg (shapes)\n"),
            "{text}"
        );
        assert!(
            text.contains("  Read: 214 paths; 0 text, 0 images, 0 clip paths, 0 masks\n"),
            "{text}"
        );
        assert!(!text.contains("Strokes"), "{text}");
        assert!(!text.contains("Dropped"), "{text}");
        assert!(!text.contains("Narrowest gap"), "{text}");
        assert!(!text.contains("ridge"), "{text}");
        assert!(
            text.contains("  At 100 / 200 / 300 mm tall: thinnest piece 3.15 / 6.30 / 9.45 mm\n"),
            "{text}"
        );
        assert!(
            text.ends_with("  The thinnest piece reaches 1.5 mm at 48 mm tall."),
            "{text}"
        );
        // Nothing measured: no width lines at all.
        report.thinnest_piece = None;
        let text = render(&report, "a.svg", "a-inlay.svg");
        assert!(
            !text.contains("Thinnest") && !text.contains("At 100") && !text.contains("reaches"),
            "{text}"
        );
    }

    #[test]
    fn the_ring_and_cross_report_names_its_figures() {
        let ring = include_bytes!("../../../tests/fixtures/prep/ring-cross.svg");
        let options = PrepOptions {
            mode: PrepMode::Regions,
            ..PrepOptions::default()
        };
        let (prepared, _) = prepare(ring, &options).expect("prepares");
        let text = render(&prepared.report, "ring-cross.svg", "ring-cross-inlay.svg");
        assert!(text.contains("Design height: 60.00 mm"), "{text}");
        assert!(
            text.contains("the board ridge between pockets: 2.00 mm (3.33 %)"),
            "{text}"
        );
        assert!(
            text.contains("narrowest gap 3.33 / 6.67 / 10.00 mm"),
            "{text}"
        );
        assert!(
            text.contains("The narrowest gap reaches 1.5 mm at 45 mm tall."),
            "{text}"
        );
    }

    #[test]
    fn warning_lines_drop_the_advice_to_run_image_prep() {
        let warning = ImportWarning::SvgStrokeOnlyPath {
            count: 2,
            first: "ring".to_owned(),
            width_mm: 2.0,
        };
        assert!(warning.to_string().ends_with("would fix this"));
        let line = warning_line(&warning);
        assert!(
            line.starts_with("warning: svg_stroke_only_path: "),
            "{line}"
        );
        assert!(!line.contains("image-prep"), "{line}");
        assert!(!line.ends_with(';'), "{line}");
        // A warning that never advised it is printed whole.
        let other = ImportWarning::PrepSmoothSkipped { count: 1 };
        assert_eq!(
            warning_line(&other),
            format!("warning: prep_smooth_skipped: {other}")
        );
    }
}
