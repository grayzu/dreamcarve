// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What `dreamcarve image-prep` prints about a run, line for line
//! (functional 49e; `docs/specs/phase-3/design.md`, "Image prep"). Here and
//! not in the CLI so the app's Image Prep panel shows the same lines. The
//! shell prints them; nothing here does I/O.

use dc_import::ImportWarning;
use dc_import::prep::{PrepMode, PrepReport, REPORT_MIN_FEATURE_MM, WidthFigure};

/// The report's lines for a run that read `input` and wrote `output` (the
/// names as the shell shows them), in order: the header, then, indented, a
/// line each for what was read, the strokes made into areas, the design's
/// height, the result, what was dropped, the thinnest piece, the narrowest
/// gap, those widths at 100, 200 and 300 mm tall, and the height at which
/// the narrower one reaches the guide width. A line with nothing to say is
/// left out.
#[must_use]
pub fn lines(report: &PrepReport, input: &str, output: &str) -> Vec<String> {
    let height = report.design_height_mm;
    let mode = match report.mode {
        PrepMode::Shapes => "shapes",
        PrepMode::Regions => "regions",
    };
    let mut lines = vec![format!("Prepared {input} \u{2192} {output} ({mode})")];
    lines.push(format!("  {}", read_line(report)));
    let strokes = &report.strokes;
    if strokes.count > 0 {
        let widths = match (strokes.least_width_mm, strokes.greatest_width_mm) {
            (Some(least), Some(greatest)) if mm(least) != mm(greatest) => {
                format!(", {} to {} mm wide", mm(least), mm(greatest))
            }
            (Some(width), _) | (None, Some(width)) => format!(", {} mm wide", mm(width)),
            (None, None) => String::new(),
        };
        lines.push(format!(
            "  Strokes made into areas: {}{widths}",
            strokes.count
        ));
    }
    lines.push(format!("  Design height: {} mm", mm(height)));
    lines.push(format!(
        "  Result: {}, {}",
        count(report.pieces, "piece", "pieces"),
        count(report.holes, "hole", "holes")
    ));
    if let Some(dropped) = dropped_line(report) {
        lines.push(format!("  {dropped}"));
    }
    let gap_label = match report.mode {
        PrepMode::Shapes => "Narrowest gap",
        PrepMode::Regions => "Narrowest gap, the board ridge between pockets",
    };
    if let Some(figure) = &report.thinnest_piece {
        lines.push(format!(
            "  Thinnest piece: {}",
            width(figure, height, " of the height")
        ));
    }
    if let Some(figure) = &report.narrowest_gap {
        lines.push(format!("  {gap_label}: {}", width(figure, height, "")));
    }
    let scaled = |label: &str, figure: &WidthFigure| {
        let widths: Vec<String> = figure.at_heights_mm.iter().map(|w| mm(*w)).collect();
        format!("{label} {} mm", widths.join(" / "))
    };
    let at: Vec<String> = [
        report
            .thinnest_piece
            .as_ref()
            .map(|f| scaled("thinnest piece", f)),
        report
            .narrowest_gap
            .as_ref()
            .map(|f| scaled("narrowest gap", f)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !at.is_empty() {
        lines.push(format!("  At 100 / 200 / 300 mm tall: {}", at.join("; ")));
    }
    // The narrower of the two is the one that limits the size.
    let limiting = match (&report.thinnest_piece, &report.narrowest_gap) {
        (Some(piece), Some(gap)) if gap.reaches_guide_at_mm > piece.reaches_guide_at_mm => {
            Some(("narrowest gap", gap))
        }
        (Some(piece), _) => Some(("thinnest piece", piece)),
        (None, Some(gap)) => Some(("narrowest gap", gap)),
        (None, None) => None,
    };
    if let Some((name, figure)) = limiting {
        lines.push(format!(
            "  The {name} reaches {REPORT_MIN_FEATURE_MM} mm at {:.0} mm tall.",
            figure.reaches_guide_at_mm
        ));
    }
    lines
}

/// The report as text, `\n` between lines and none after the last.
#[must_use]
pub fn render(report: &PrepReport, input: &str, output: &str) -> String {
    lines(report, input, output).join("\n")
}

/// `warning: <code>: <message>` for a warning of an `image-prep` run. The
/// import checks that `init` words as advice to run `image-prep` are
/// worded here as what the run did, since the user has run it.
#[must_use]
pub fn warning_line(warning: &ImportWarning) -> String {
    let message = match warning {
        ImportWarning::SvgHoleAsSeparatePath {
            count,
            inner,
            outer,
        } => format!(
            "{count} filled path(s) lie wholly inside another filled path; the first is `{inner}` inside `{outer}`. Each is a hole of the path that contains it, as `init` reads nested paths"
        ),
        ImportWarning::SvgStrokeOnlyPath {
            count,
            first,
            width_mm,
        } => format!(
            "{count} path(s) have a stroke and no fill; the first is `{first}`, stroked {width_mm:.3} mm wide. Their strokes were made into areas"
        ),
        ImportWarning::SvgHiddenObject {
            count,
            first,
            reason,
        } => format!(
            "{count} object(s) are not drawn ({}) and were skipped; the first is `{first}`",
            reason.describe()
        ),
        other => other.message_without_advice(),
    };
    format!("warning: {}: {message}", warning.code())
}

/// `Read:`, always: the paths, what was skipped, what cannot be converted.
fn read_line(report: &PrepReport) -> String {
    let read = &report.read;
    let skipped: Vec<String> = [
        (read.hidden, "hidden"),
        (read.unpainted, "unpainted"),
        (read.zero_opacity, "zero-opacity"),
        (read.display_none, "in hidden layers"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect();
    let of_them = if skipped.is_empty() {
        String::new()
    } else {
        format!(", of them {} skipped", join_and(&skipped))
    };
    format!(
        "Read: {}{of_them}; {} text, {} images, {} clip paths, {} masks",
        count(read.paths, "path", "paths"),
        read.text,
        read.images,
        read.clip_paths,
        read.masks
    )
}

/// `Dropped:`, when anything was dropped, filled or smoothed.
fn dropped_line(report: &PrepReport) -> Option<String> {
    let height = report.design_height_mm;
    let threshold = |threshold_mm: f64, of: &str| {
        format!(
            "{} %{of} ({} mm)",
            percent(threshold_mm, height),
            mm_fine(threshold_mm)
        )
    };
    let mut removed = Vec::new();
    if report.dropped_pieces > 0 {
        removed.push(format!(
            "{} below {}",
            count(report.dropped_pieces, "piece", "pieces"),
            threshold(report.min_detail_mm, " of the height")
        ));
    }
    if report.filled_holes > 0 {
        let below = if report.dropped_pieces == 0 {
            format!(
                " below {}",
                threshold(report.min_detail_mm, " of the height")
            )
        } else {
            String::new()
        };
        removed.push(format!(
            "{} filled{below}",
            count(report.filled_holes, "hole", "holes")
        ));
    }
    let mut parts = Vec::new();
    if !removed.is_empty() {
        parts.push(removed.join(", "));
    }
    if report.smoothed_vertices > 0 {
        parts.push(format!(
            "smoothed within {}",
            threshold(report.smooth_mm, "")
        ));
    }
    (!parts.is_empty()).then(|| format!("Dropped: {}", parts.join("; ")))
}

/// `3.10 mm (3.15 % of the height)`: the share of the height is named
/// once, on the first width, and the next says `0.56 %`.
fn width(figure: &WidthFigure, height_mm: f64, of: &str) -> String {
    format!(
        "{} mm ({} %{of})",
        mm(figure.width_mm),
        percent(figure.width_mm, height_mm)
    )
}

/// Millimetres to two decimals, three when under 0.1 so a thin width is
/// not shown as 0.00.
fn mm(value: f64) -> String {
    if value != 0.0 && value.abs() < 0.1 {
        format!("{value:.3}")
    } else {
        format!("{value:.2}")
    }
}

/// Millimetres to three decimals: the thresholds, which are sub-millimetre.
fn mm_fine(value: f64) -> String {
    format!("{value:.3}")
}

/// `part` as a percentage of `whole`, two decimals.
fn percent(part: f64, whole: f64) -> String {
    let share = if whole > 0.0 {
        part / whole * 100.0
    } else {
        0.0
    };
    format!("{share:.2}")
}

/// `1 piece`, `2 pieces`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `a`, `a and b`, `a, b and c`.
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
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
