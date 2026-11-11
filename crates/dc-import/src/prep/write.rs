// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The writer of image prep (step 8 of `docs/specs/phase-3/design.md`,
//! "Image prep"): [`write_svg`] turns [`Prepared`] into the SVG text the
//! command saves, and [`verify`] reads that text back through the importer
//! so [`prepare`](super::prepare) never returns pieces it cannot prove the
//! importer reads as they are.

use std::fmt::Write as _;

use super::{MIN_FLATTEN_TOLERANCE_MM, Prepared};
use crate::{ImportError, SvgImportOptions, import_svg};
use dc_geometry::{Contour, Polyline, Region, nest};

/// The SVG text of `prepared`: the page at the input's physical size in
/// millimetres with a one-to-one `viewBox`, and one filled `<path>` per
/// piece (`piece-1`, `piece-2`, … in the pieces' canonical order), its
/// outline first and its holes after as further subpaths, `evenodd` so a
/// hole is a hole whatever its winding. `M`, `L` and `Z` only; numbers to
/// four decimals (0.1 µm) without trailing zeros, negative zero written
/// `0`; `\n` line ends; Y flipped back to the SVG's Y-down
/// (`y_svg = page_height_mm − y`). No metadata, description, version
/// string, time or stroke attribute, so the same pieces always give the
/// same text.
///
/// [`prepare`](super::prepare) has proven this text reads back as the
/// pieces it was written from.
#[must_use]
pub fn write_svg(prepared: &Prepared) -> String {
    let mut out = String::new();
    let (width, height) = (prepared.page_width_mm, prepared.page_height_mm);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">",
        w = number(width),
        h = number(height),
    );
    for (index, piece) in prepared.pieces.iter().enumerate() {
        let _ = write!(out, "  <path id=\"piece-{}\" d=\"", index + 1);
        let contours = std::iter::once(piece.outer()).chain(piece.holes());
        for (n, contour) in contours.enumerate() {
            if n > 0 {
                out.push(' ');
            }
            subpath(&mut out, contour, height);
        }
        out.push_str("\" fill=\"#000000\" fill-rule=\"evenodd\"/>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// Appends `M x y L x y … Z` for `contour`, Y flipped about the page.
fn subpath(out: &mut String, contour: &Contour, page_height_mm: f64) {
    let polyline = contour.polyline();
    // Prepared pieces are polygons; an arc, were one ever to appear, is
    // written as the line segments the importer would flatten it to.
    let flattened;
    let polyline = if polyline.vertices().iter().any(|v| v.bulge != 0.0) {
        flattened = polyline.with_arcs_flattened(MIN_FLATTEN_TOLERANCE_MM);
        &flattened
    } else {
        polyline
    };
    for (n, vertex) in polyline.vertices().iter().enumerate() {
        let _ = write!(
            out,
            "{} {} {}",
            if n == 0 { "M" } else { " L" },
            number(vertex.x),
            number(page_height_mm - vertex.y),
        );
    }
    out.push_str(" Z");
}

/// `value` to four decimals, trailing zeros and a bare point dropped, and
/// negative zero (and anything that rounds to it) written `0`.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    if text == "-0" || text.is_empty() {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// Reads the text [`write_svg`] writes for `prepared` back through the
/// importer (default options, but for the vertex cap) and `nest`, and
/// requires what it must be: one definition per piece, each reading as one
/// piece with the same number of holes and an area within the piece's
/// perimeter times the importer's flattening tolerance of the piece's.
///
/// # Errors
///
/// [`ImportError::PrepOutputUnreadable`], naming what differed.
pub(crate) fn verify(prepared: &Prepared, max_vertices: usize) -> Result<(), ImportError> {
    if prepared.pieces.is_empty() {
        return Err(unreadable("there are no pieces to write"));
    }
    let text = write_svg(prepared);
    let options = SvgImportOptions {
        max_vertices,
        ..SvgImportOptions::new()
    };
    let (import, _) = import_svg(text.as_bytes(), &options).map_err(unreadable)?;
    if import.definitions.len() != prepared.pieces.len() {
        return Err(unreadable(format!(
            "{} pieces were written and the importer reads {} shapes",
            prepared.pieces.len(),
            import.definitions.len()
        )));
    }
    for (index, (piece, definition)) in prepared.pieces.iter().zip(&import.definitions).enumerate()
    {
        let which = index + 1;
        let contours = definition
            .profiles
            .iter()
            .map(|profile| Contour::new(profile.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| unreadable(format!("piece-{which}: {e}")))?;
        let (read, _) = nest(contours).map_err(|e| unreadable(format!("piece-{which}: {e}")))?;
        let [read] = read.as_slice() else {
            return Err(unreadable(format!(
                "piece-{which} reads as {} pieces, not one",
                read.len()
            )));
        };
        if read.holes().len() != piece.holes().len() {
            return Err(unreadable(format!(
                "piece-{which} has {} holes and reads back with {}",
                piece.holes().len(),
                read.holes().len()
            )));
        }
        let allowed = perimeter_mm(piece) * options.flatten_tolerance_mm;
        let differs = (read.area_mm2() - piece.area_mm2()).abs();
        // A NaN area is a mismatch, not a pass.
        let within = differs.is_finite() && differs <= allowed;
        if !within {
            return Err(unreadable(format!(
                "piece-{which} has an area of {:.4} mm² and reads back as {:.4} mm²",
                piece.area_mm2(),
                read.area_mm2()
            )));
        }
    }
    Ok(())
}

fn perimeter_mm(piece: &Region) -> f64 {
    std::iter::once(piece.outer())
        .chain(piece.holes())
        .map(|c| Polyline::length_mm(c.polyline()))
        .sum()
}

fn unreadable(reason: impl std::fmt::Display) -> ImportError {
    ImportError::PrepOutputUnreadable {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests"
    )]

    use super::*;
    use crate::prep::{PrepMode, PrepOptions, findings, prepare};
    use crate::{ImportError, MAX_IMPORT_VERTICES, SvgImportOptions, import_svg};
    use dc_geometry::{Contour, area_mm2, nest};

    const RING_CROSS: &[u8] = include_bytes!("../../../../tests/fixtures/prep/ring-cross.svg");
    const MESSY: &[u8] = include_bytes!("../../../../tests/fixtures/prep/messy.svg");
    const CLEAN: &[u8] = include_bytes!("../../../../tests/fixtures/prep/clean.svg");
    const COMPASS: &[u8] = include_bytes!("../../../../tests/fixtures/prep/compass.svg");
    const COMPASS_REGIONS: &[u8] =
        include_bytes!("../../../../tests/fixtures/prep/compass-regions.svg");

    fn svg(width: f64, height: f64, body: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}mm" height="{height}mm" viewBox="0 0 {width} {height}">{body}</svg>"#
        )
        .into_bytes()
    }

    fn options(mode: PrepMode) -> PrepOptions {
        PrepOptions {
            mode,
            ..PrepOptions::default()
        }
    }

    fn prepared(bytes: &[u8], mode: PrepMode) -> Prepared {
        prepare(bytes, &options(mode)).expect("prepare").0
    }

    fn text(bytes: &[u8], mode: PrepMode) -> String {
        write_svg(&prepared(bytes, mode))
    }

    /// The pieces the importer reads from `text`, nested.
    fn read_back(text: &str) -> Vec<dc_geometry::Region> {
        let (import, _) = import_svg(text.as_bytes(), &SvgImportOptions::new()).expect("imports");
        let contours: Vec<Contour> = import
            .definitions
            .iter()
            .flat_map(|d| d.profiles.iter())
            .map(|p| Contour::new(p.clone()).expect("contour"))
            .collect();
        nest(contours).expect("nests").0
    }

    fn same_box(a: dc_geometry::Rect, b: dc_geometry::Rect) {
        for (x, y) in [
            (a.min.x, b.min.x),
            (a.min.y, b.min.y),
            (a.max.x, b.max.x),
            (a.max.y, b.max.y),
        ] {
            assert!((x - y).abs() < 0.01, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn declares_millimetres_and_the_input_page() {
        let out = text(
            &svg(
                200.0,
                100.0,
                r#"<rect x="10" y="10" width="50" height="30"/>"#,
            ),
            PrepMode::Shapes,
        );
        assert!(
            out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
            "{out}"
        );
        assert!(
            out.contains(r#"xmlns="http://www.w3.org/2000/svg""#),
            "{out}"
        );
        assert!(out.contains(r#"width="200mm""#), "{out}");
        assert!(out.contains(r#"height="100mm""#), "{out}");
        assert!(out.contains(r#"viewBox="0 0 200 100""#), "{out}");
    }

    #[test]
    fn paths_are_filled_and_never_stroked() {
        for bytes in [RING_CROSS, MESSY, CLEAN, COMPASS] {
            let out = text(bytes, PrepMode::Shapes);
            assert!(!out.contains("stroke"), "{out}");
            assert!(!out.contains("<text") && !out.contains("<image"), "{out}");
            assert!(!out.contains("clip") && !out.contains("mask"), "{out}");
            let paths = out.matches("<path ").count();
            assert!(paths > 0);
            assert_eq!(
                out.matches(r##"fill="#000000" fill-rule="evenodd""##)
                    .count(),
                paths
            );
            assert_eq!(out.matches("<path id=\"piece-").count(), paths);
        }
    }

    #[test]
    fn y_is_flipped_back_so_an_f_is_an_f() {
        // An F near the top of a 100 mm page (SVG is Y-down): its stem
        // runs from y = 10 to 60, so it is 90 mm up at the top.
        let f = svg(
            100.0,
            100.0,
            r#"<path d="M 10 10 L 40 10 L 40 18 L 18 18 L 18 30 L 34 30 L 34 38 L 18 38 L 18 60 L 10 60 Z"/>"#,
        );
        let input = import_svg(&f, &SvgImportOptions::new()).expect("imports").0;
        let out = text(&f, PrepMode::Shapes);
        let after = import_svg(out.as_bytes(), &SvgImportOptions::new())
            .expect("imports")
            .0;
        let (a, b) = (input.bounds.expect("box"), after.bounds.expect("box"));
        same_box(a, b);
        assert!((b.max.y - 90.0).abs() < 0.01, "{b:?}");
        assert!((b.min.y - 40.0).abs() < 0.01, "{b:?}");
        // The arms are on the right: the top bar is the widest row.
        assert!((b.max.x - 40.0).abs() < 0.01, "{b:?}");
        // The area is the F's: 30 x 8 bar, 50 x 8 stem less overlap, and
        // a 16 x 8 middle arm.
        let expected = 30.0 * 8.0 + 8.0 * 42.0 + 16.0 * 8.0;
        // (usvg keeps coordinates as f32, so the importer reads to ~1e-6.)
        assert!((area_mm2(&read_back(&out)) - expected).abs() < 0.01);
    }

    #[test]
    fn a_lone_rectangle_round_trips() {
        let rect = svg(
            200.0,
            100.0,
            r#"<rect x="20" y="30" width="50" height="25"/>"#,
        );
        let input = import_svg(&rect, &SvgImportOptions::new())
            .expect("imports")
            .0;
        let out = text(&rect, PrepMode::Shapes);
        let regions = read_back(&out);
        assert_eq!(regions.len(), 1);
        let contour_box = regions[0].outer().polyline().bounds().expect("box");
        same_box(input.bounds.expect("box"), contour_box);
        assert!(
            (area_mm2(&regions) - 50.0 * 25.0).abs() < 0.01,
            "{}",
            area_mm2(&regions)
        );
    }

    #[test]
    fn numbers_are_normalised() {
        // A corner on the page's edge writes 0, never -0, and no number
        // has trailing zeros.
        let out = text(
            &svg(
                100.0,
                100.0,
                r#"<rect x="0" y="0" width="40" height="40"/>"#,
            ),
            PrepMode::Shapes,
        );
        assert!(out.contains("M 0 ") || out.contains(" 0 "), "{out}");
        let body = out.split_once('\n').expect("a declaration line").1;
        for token in body.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')) {
            assert_ne!(token, "-0", "{out}");
            assert!(
                !token.contains('.') || !token.ends_with('0'),
                "{token} in {out}"
            );
        }
        assert!(!out.contains('\r'));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn no_version_time_or_metadata() {
        for bytes in [RING_CROSS, MESSY, CLEAN] {
            let out = text(bytes, PrepMode::Shapes);
            for word in ["metadata", "<desc", "<title", "generator", "<!--", "2026"] {
                assert!(!out.contains(word), "{word} in {out}");
            }
            assert_eq!(
                out.matches("version").count(),
                1,
                "only the XML declaration: {out}"
            );
        }
    }

    #[test]
    fn output_reads_back_through_the_importer_and_nest() {
        for bytes in [RING_CROSS, MESSY, CLEAN, COMPASS, COMPASS_REGIONS] {
            for mode in [PrepMode::Shapes, PrepMode::Regions] {
                let Ok((p, _)) = prepare(bytes, &options(mode)) else {
                    continue;
                };
                let regions = read_back(&write_svg(&p));
                assert_eq!(regions.len(), p.pieces.len());
                assert_eq!(
                    regions.iter().map(|r| r.holes().len()).sum::<usize>(),
                    p.pieces.iter().map(|r| r.holes().len()).sum::<usize>()
                );
                assert!(verify(&p, MAX_IMPORT_VERTICES).is_ok());
            }
        }
    }

    #[test]
    fn unreadable_output_is_an_error_and_nothing_is_returned() {
        let good = prepared(RING_CROSS, PrepMode::Shapes);
        assert!(verify(&good, MAX_IMPORT_VERTICES).is_ok());
        // A page the importer refuses.
        let mut broken = good.clone();
        broken.page_width_mm = 0.0;
        assert!(matches!(
            verify(&broken, MAX_IMPORT_VERTICES),
            Err(ImportError::PrepOutputUnreadable { .. })
        ));
        broken.page_width_mm = f64::NAN;
        assert!(matches!(
            verify(&broken, MAX_IMPORT_VERTICES),
            Err(ImportError::PrepOutputUnreadable { .. })
        ));
        // A cap the output exceeds.
        assert!(matches!(
            verify(&good, 3),
            Err(ImportError::PrepOutputUnreadable { .. })
        ));
        // Nothing to read back is a mismatch too.
        let mut lost = good;
        lost.pieces.clear();
        assert!(matches!(
            verify(&lost, MAX_IMPORT_VERTICES),
            Err(ImportError::PrepOutputUnreadable { .. })
        ));
    }

    #[test]
    fn init_on_the_output_prints_no_findings() {
        for (bytes, mode) in [
            (RING_CROSS, PrepMode::Regions),
            (RING_CROSS, PrepMode::Shapes),
            (MESSY, PrepMode::Shapes),
            (CLEAN, PrepMode::Shapes),
        ] {
            let p = prepared(bytes, mode);
            let out = write_svg(&p);
            let (import, import_warnings) =
                import_svg(out.as_bytes(), &SvgImportOptions::new()).expect("imports");
            assert!(import_warnings.is_empty(), "{import_warnings:?}");
            assert_eq!(import.definitions.len(), p.pieces.len());
            let found = findings(out.as_bytes(), &SvgImportOptions::new()).expect("checks");
            assert!(found.is_empty(), "{found:?}");
        }
    }

    // 61i: the same bytes whatever the input's order or line ends.

    #[test]
    fn same_input_twice_reverse_order_and_crlf_give_identical_text() {
        assert_eq!(
            text(RING_CROSS, PrepMode::Regions),
            text(RING_CROSS, PrepMode::Regions)
        );
        let lines = [
            r##"<circle cx="30" cy="30" r="29" fill="none" stroke="#000" stroke-width="2"/>"##,
            r##"<line x1="1" y1="30" x2="59" y2="30" stroke="#000" stroke-width="2"/>"##,
            r##"<line x1="30" y1="1" x2="30" y2="59" stroke="#000" stroke-width="2"/>"##,
        ];
        let mut reversed = lines.to_vec();
        reversed.reverse();
        let (a, b) = (
            svg(60.0, 60.0, &lines.concat()),
            svg(60.0, 60.0, &reversed.concat()),
        );
        let crlf = String::from_utf8(RING_CROSS.to_vec())
            .unwrap()
            .replace('\n', "\r\n");
        for mode in [PrepMode::Shapes, PrepMode::Regions] {
            assert_eq!(text(&a, mode), text(&b, mode));
            assert_eq!(text(RING_CROSS, mode), text(crlf.as_bytes(), mode));
        }
    }
}
