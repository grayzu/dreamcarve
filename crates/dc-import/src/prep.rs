// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Image prep (sub-phase 3.4, `docs/specs/phase-3/design.md`, "Image
//! prep"): the [`walk`] of an SVG's visible geometry, the import checks
//! built on it ([`findings()`] for `init`, [`design_findings`] for `check`)
//! and [`prepare()`], which turns an SVG into pieces and a [`PrepReport`].

#[cfg(test)]
mod defaults;
mod dxf;
mod findings;
mod report;
mod run;
pub(crate) mod stroke;
pub mod walk;
mod write;

pub use crate::svg::MIN_FLATTEN_TOLERANCE_MM;
pub use dxf::prepare_dxf;
pub use findings::{design_findings, design_findings_named, findings};
pub use report::{PrepReport, PrepSource, ReadReport, StrokeReport, WidthFigure};
pub use run::{PrepMode, PrepOptions, Prepared, flatten_tolerance_mm, prepare};
pub use write::write_svg;

/// The default minimum detail, percent of the design's height (functional
/// 49d). Calibrated by slice 21f (`tests/fixtures/prep/README.md`; open
/// question 16); the maintainer confirms it on their own art. Pieces
/// and holes whose widest point is under this share of the height are
/// reported by `feature_below_size`.
pub const DEFAULT_MIN_DETAIL_PCT: f64 = 0.2;
/// The default `--smooth`, percent of the design's height (open question
/// 17). Calibrated by the same sweep; held at most half of
/// [`DEFAULT_MIN_DETAIL_PCT`] by a test.
pub const DEFAULT_SMOOTH_PCT: f64 = 0.05;
/// The largest threshold either flag takes, percent.
pub const MAX_THRESHOLD_PCT: f64 = 5.0;
/// Prepared outlines are polygons within this fraction of the design's
/// height of the true curves (0.005 %), never finer than
/// [`MIN_FLATTEN_TOLERANCE_MM`].
pub const FLATTEN_FRACTION: f64 = 5.0e-5;
/// `--regions` is suggested when the ink's median width is under this
/// percentage of the height (49c, open question 18).
pub const REGIONS_SUGGESTED_BELOW_PCT: f64 = 2.0;
/// A filled piece whose box covers this fraction of the drawing's box is
/// flagged as a probable page background.
pub const BACKGROUND_COVER_FRACTION: f64 = 0.98;
/// The report's guide width, mm (49e); informational.
pub const REPORT_MIN_FEATURE_MM: f64 = 1.5;
/// The heights the report scales the widths to, mm.
pub const REPORT_HEIGHTS_MM: [f64; 3] = [100.0, 200.0, 300.0];
/// `narrowest_gap` searches this fraction of the height.
pub const GAP_REACH_FRACTION: f64 = 0.25;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

    use super::{PrepOptions, prepare};
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    /// Art as a trace leaves it: 100 000 vertices in fifty jittered
    /// outlines, and a fringe of specks.
    fn traced_scale_art() -> String {
        let mut state = 0x2545_f491_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            f64::from(state) / f64::from(u32::MAX)
        };
        let mut body = String::new();
        let per_ring = 2_000;
        for ring in 0..50 {
            let (cx, cy) = (
                100.0 + 150.0 * f64::from(ring % 10),
                100.0 + 150.0 * f64::from(ring / 10),
            );
            let mut d = String::new();
            for i in 0..per_ring {
                let a = std::f64::consts::TAU * f64::from(i) / f64::from(per_ring);
                let r = 60.0 + 0.1 * (next() - 0.5);
                let _ = write!(
                    d,
                    "{}{:.3},{:.3} ",
                    if i == 0 { "M " } else { "L " },
                    cx + r * a.cos(),
                    cy + r * a.sin()
                );
            }
            let _ = write!(body, r#"<path d="{d}Z"/>"#);
        }
        for _ in 0..200 {
            let _ = write!(
                body,
                r#"<circle cx="{:.3}" cy="{:.3}" r="{:.3}"/>"#,
                50.0 + 1400.0 * next(),
                50.0 + 800.0 * next(),
                0.02 + 0.08 * next()
            );
        }
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1500mm" height="900mm" viewBox="0 0 1500 900">{body}</svg>"#
        )
    }

    /// A traced picture's worth of vertices prepares in seconds (a loose
    /// bound, as `s17_check_is_fast_on_the_sign` is: about 4 s locally, 11 s on
    /// a shared Windows runner, so 60 s catches only a blow-up).
    #[test]
    fn runs_in_seconds_on_traced_scale_art() {
        let art = traced_scale_art();
        let started = Instant::now();
        let (prepared, _) = prepare(art.as_bytes(), &PrepOptions::default()).unwrap();
        let elapsed = started.elapsed();
        assert!(!prepared.pieces.is_empty());
        assert!(
            elapsed < Duration::from_secs(60),
            "100 000 vertices took {elapsed:?}"
        );
    }

    // ── DXF input (#669) ─────────────────────────────────────────────────

    use super::{PrepSource, prepare_dxf, write_svg};
    use crate::{DxfUnits, ImportWarning, SvgImportOptions, import_svg};

    macro_rules! prep_fixture {
        ($name:literal) => {
            include_bytes!(concat!("../../../tests/fixtures/prep/", $name))
        };
    }

    /// The tray (`tools/parity/prep-dxf-fixture.ts`): overlapping pockets
    /// merge into one piece, the speck is dropped, the circle drawn inside
    /// the outline becomes its hole, and the hatched pocket keeps its
    /// island. The SVG written imports with no warning from `init`'s
    /// importer or its checks, as three shapes, two with a hole.
    #[test]
    fn dxf_input_writes_an_svg_init_reads_cleanly() {
        let (prepared, warnings) =
            prepare_dxf(prep_fixture!("tool-tray.dxf"), &PrepOptions::default()).unwrap();
        let codes: Vec<&str> = warnings.iter().map(ImportWarning::code).collect();
        assert!(codes.contains(&"svg_hole_as_separate_path"), "{codes:?}");
        assert!(codes.contains(&"prep_speck_dropped"), "{codes:?}");
        let report = &prepared.report;
        assert_eq!((report.pieces, report.holes), (3, 2), "{report:?}");
        assert_eq!(report.dropped_pieces, 1);
        assert!((report.design_height_mm - 38.1).abs() < 1e-6, "{report:?}");

        let svg = write_svg(&prepared);
        let (imported, import_warnings) =
            import_svg(svg.as_bytes(), &SvgImportOptions::new()).unwrap();
        assert!(import_warnings.is_empty(), "{import_warnings:?}");
        let checks = super::findings(svg.as_bytes(), &SvgImportOptions::new()).unwrap();
        assert!(checks.is_empty(), "{checks:?}");
        let mut profiles: Vec<usize> = imported
            .definitions
            .iter()
            .map(|d| d.profiles.len())
            .collect();
        profiles.sort_unstable();
        assert_eq!(profiles, [1, 2, 2]);
    }

    /// The tray in inches and its millimetre twin prepare to the same SVG,
    /// and the report says which units each declared.
    #[test]
    fn dxf_units_are_honoured() {
        let options = PrepOptions::default();
        let (inch, _) = prepare_dxf(prep_fixture!("tool-tray.dxf"), &options).unwrap();
        let (mm, _) = prepare_dxf(prep_fixture!("tool-tray-mm.dxf"), &options).unwrap();
        assert_eq!(write_svg(&inch), write_svg(&mm));
        assert!(
            matches!(
                inch.report.source,
                PrepSource::Dxf { units: DxfUnits::Inches, mm_per_unit, closed_shapes: 6, open_paths: 0 }
                    if (mm_per_unit - 25.4).abs() < 1e-12
            ),
            "{:?}",
            inch.report.source
        );
        assert!(matches!(
            mm.report.source,
            PrepSource::Dxf {
                units: DxfUnits::Millimetres,
                ..
            }
        ));
        assert!(
            (inch.page_width_mm - 6.75 * 25.4).abs() < 1e-6,
            "{}",
            inch.page_width_mm
        );
    }

    /// A DXF line has no width, so open paths are reported, not outlined:
    /// only the closed square is prepared.
    #[test]
    fn dxf_open_paths_are_reported_not_outlined() {
        let (prepared, warnings) =
            prepare_dxf(prep_fixture!("open-paths.dxf"), &PrepOptions::default()).unwrap();
        assert_eq!(
            warnings.first(),
            Some(&ImportWarning::DxfOpenPathNotConvertible {
                count: 2,
                first: "cut".into()
            }),
            "{warnings:?}"
        );
        assert_eq!(prepared.report.pieces, 1);
        // usvg reads the page size as f32.
        assert!(
            (prepared.page_width_mm - 20.0).abs() < 1e-4,
            "{}",
            prepared.page_width_mm
        );
        assert!(matches!(
            prepared.report.source,
            PrepSource::Dxf {
                closed_shapes: 1,
                open_paths: 2,
                ..
            }
        ));
        // A drawing of open paths only has nothing to prepare.
        let only_open = b"0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n10.0\n21\n10.0\n0\nENDSEC\n0\nEOF\n";
        assert!(matches!(
            prepare_dxf(only_open, &PrepOptions::default()),
            Err(crate::ImportError::PrepNothingLeft)
        ));
    }
}
