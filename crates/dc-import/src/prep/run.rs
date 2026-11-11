// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The image-prep pipeline (steps 1 to 7 of `docs/specs/phase-3/design.md`,
//! "Image prep"): [`prepare`] turns the bytes of an SVG into pieces and a
//! [`PrepReport`]. A pure function; writing the SVG is slice 21g's.

use super::report::PrepReport;
use super::{DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT};
use crate::svg::MAX_IMPORT_VERTICES;
use crate::{ImportError, ImportWarning};
use dc_geometry::Region;

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

/// The flatten tolerance for a design `height_mm` tall.
#[must_use]
pub fn flatten_tolerance_mm(height_mm: f64) -> f64 {
    let _ = height_mm;
    0.0
}

/// Prepares the SVG in `bytes`.
///
/// # Errors
///
/// [`ImportError`].
pub fn prepare(
    bytes: &[u8],
    options: &PrepOptions,
) -> Result<(Prepared, Vec<ImportWarning>), ImportError> {
    let _ = (bytes, options);
    Err(ImportError::PrepOutputUnreadable {
        reason: "not implemented".to_owned(),
    })
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
        assert!(has(40.0, 31.6));
        assert!(has(0.0, 30.0));
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
        assert!(prepare(RING_CROSS, &edge).is_ok());
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
            r#"<path d="M0,0 L50000,0" fill="none" stroke="black" stroke-width="50" stroke-dasharray="0.001"/>"#,
        );
        assert!(matches!(
            prepare(&art, &PrepOptions::default()),
            Err(ImportError::TooManyVertices { .. })
        ));
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
