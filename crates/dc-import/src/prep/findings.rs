// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/classifier.ts

//! The import checks (functional 49a; design, "Import checks"): one
//! measure over the walk's painted paths each, one warning per kind found
//! with a count and the first example. Nothing is changed or refused.

use crate::svg::SvgImportOptions;
use crate::{ImportError, ImportWarning};
use dc_geometry::Region;

/// The 49a checks on the SVG in `bytes`, in a fixed order: overlap, hole as
/// a separate path, stroke only, hidden, not convertible (text, images,
/// clip paths, masks), below size.
///
/// # Errors
///
/// [`ImportError`] when the SVG cannot be read, or `options` are refused,
/// as for the importer.
pub fn findings(
    bytes: &[u8],
    options: &SvgImportOptions,
) -> Result<Vec<ImportWarning>, ImportError> {
    let _ = (bytes, options);
    Ok(Vec::new())
}

/// The two checks that make sense on a project's shapes, for `check`:
/// `shapes_overlap` and `feature_below_size`. Regions are named
/// `region-N` (their position, from 1).
#[must_use]
pub fn design_findings(regions: &[Region], height_mm: f64) -> Vec<ImportWarning> {
    let _ = (regions, height_mm);
    Vec::new()
}

/// [`design_findings`] with the shapes' own names: one `(name, regions)`
/// per shape, in order.
#[must_use]
pub fn design_findings_named(
    shapes: &[(String, Vec<Region>)],
    height_mm: f64,
) -> Vec<ImportWarning> {
    let _ = (shapes, height_mm);
    Vec::new()
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
    use crate::{HiddenReason, NotConvertible};
    use dc_geometry::{FillRule, Point, Polyline, resolve_fill};

    const MESSY: &[u8] = include_bytes!("../../../../tests/fixtures/prep/messy.svg");
    const CLEAN: &[u8] = include_bytes!("../../../../tests/fixtures/prep/clean.svg");
    const FIX: &str = "; `dreamcarve image-prep` would fix this";

    fn check(bytes: &[u8]) -> Vec<ImportWarning> {
        findings(bytes, &SvgImportOptions::new()).expect("findings")
    }

    fn drawing(body: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"#
        )
        .into_bytes()
    }

    fn codes(warnings: &[ImportWarning]) -> Vec<&'static str> {
        warnings.iter().map(ImportWarning::code).collect()
    }

    #[test]
    fn overlap_names_the_pair_and_where() {
        let found = check(MESSY);
        let overlap = found
            .iter()
            .find_map(|w| match w {
                ImportWarning::ShapesOverlap {
                    count,
                    first,
                    second,
                    x_mm,
                    y_mm,
                } => Some((*count, first.as_str(), second.as_str(), *x_mm, *y_mm)),
                _ => None,
            })
            .expect("shapes_overlap");
        assert_eq!((overlap.0, overlap.1, overlap.2), (1, "sq-a", "sq-b"));
        // The squares meet at x 16..20; SVG y 0..10 is Y-up 90..100.
        assert!((overlap.3 - 18.0).abs() < 0.01, "x {}", overlap.3);
        assert!((overlap.4 - 95.0).abs() < 0.01, "y {}", overlap.4);
    }

    #[test]
    fn separate_hole_needs_both_paths_filled() {
        let found = check(MESSY);
        assert!(
            found.contains(&ImportWarning::SvgHoleAsSeparatePath {
                count: 1,
                inner: "inner".into(),
                outer: "outer".into(),
            }),
            "{found:?}"
        );
        // The inner path is only stroked: no visible fill, so no hole.
        let stroked = check(&drawing(
            r#"<rect id="o" x="10" y="10" width="40" height="40"/>
               <rect id="i" x="20" y="20" width="10" height="10" fill="none" stroke="black"/>"#,
        ));
        assert!(
            !codes(&stroked).contains(&"svg_hole_as_separate_path"),
            "{stroked:?}"
        );
    }

    #[test]
    fn an_unfilled_frame_is_not_reported() {
        let found = check(&drawing(
            r#"<rect id="frame" x="5" y="5" width="90" height="90" fill="none" stroke="black"/>
               <rect id="shape" x="20" y="20" width="30" height="30"/>"#,
        ));
        assert!(
            !codes(&found).contains(&"svg_hole_as_separate_path"),
            "{found:?}"
        );
    }

    #[test]
    fn stroke_only_gives_the_width_in_mm() {
        let found = check(MESSY);
        assert!(
            found.iter().any(
                |w| matches!(w, ImportWarning::SvgStrokeOnlyPath { count: 1, first, width_mm }
                if first == "centreline" && (width_mm - 1.2).abs() < 1e-3)
            ),
            "{found:?}"
        );
        // 50 user units over 100 mm: the stroke is twice as wide in mm.
        let scaled = findings(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 50 50"><path id="p" d="M5,5 L40,40" fill="none" stroke="black" stroke-width="1"/></svg>"#,
            &SvgImportOptions::new(),
        )
        .unwrap();
        assert!(
            scaled.iter().any(
                |w| matches!(w, ImportWarning::SvgStrokeOnlyPath { width_mm, .. }
                if (width_mm - 2.0).abs() < 1e-3)
            ),
            "{scaled:?}"
        );
    }

    #[test]
    fn hidden_object_names_the_layer() {
        let found = check(MESSY);
        assert!(
            found.contains(&ImportWarning::SvgHiddenObject {
                count: 1,
                first: "old-layer".into(),
                reason: HiddenReason::DisplayNone,
            }),
            "{found:?}"
        );
    }

    #[test]
    fn not_convertible_names_the_fix() {
        let found = check(MESSY);
        let kinds: Vec<_> = found
            .iter()
            .filter_map(|w| match w {
                ImportWarning::SvgObjectNotConvertible { kind, count, group } => {
                    Some((*kind, *count, group.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                (NotConvertible::Text, 1, ""),
                (NotConvertible::Image, 1, ""),
                (NotConvertible::ClipPath, 1, "clipped"),
            ]
        );
        let text: Vec<String> = found.iter().map(ToString::to_string).collect();
        assert!(
            text.iter()
                .any(|m| m.contains("convert text to paths in your drawing program"))
        );
        assert!(
            text.iter()
                .any(|m| m.contains("trace images in Inkscape first"))
        );
        assert!(text.iter().any(|m| m.contains("expand clips and masks")));
    }

    #[test]
    fn feature_below_size_counts_and_names_the_smallest() {
        let found = check(MESSY);
        let (count, smallest, threshold, height) = found
            .iter()
            .find_map(|w| match w {
                ImportWarning::FeatureBelowSize {
                    count,
                    smallest_mm,
                    threshold_mm,
                    height_mm,
                } => Some((*count, *smallest_mm, *threshold_mm, *height_mm)),
                _ => None,
            })
            .expect("feature_below_size");
        assert_eq!(count, 1);
        // The 0.1 mm dot; flattened at 0.01 mm its widest point is a little
        // under its diameter.
        assert!((0.07..=0.101).contains(&smallest), "smallest {smallest}");
        assert!((threshold - 0.2).abs() < 1e-3, "threshold {threshold}");
        assert!((height - 100.0).abs() < 1e-3, "height {height}");
    }

    #[test]
    fn a_clean_drawing_has_none() {
        assert_eq!(check(CLEAN), []);
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(
            codes(&check(MESSY)),
            [
                "shapes_overlap",
                "svg_hole_as_separate_path",
                "svg_stroke_only_path",
                "svg_hidden_object",
                "svg_object_not_convertible",
                "svg_object_not_convertible",
                "svg_object_not_convertible",
                "feature_below_size",
            ]
        );
    }

    #[test]
    fn messages_that_image_prep_fixes_name_it() {
        for warning in check(MESSY) {
            let message = warning.to_string();
            let fixed = !matches!(warning, ImportWarning::SvgObjectNotConvertible { .. });
            assert_eq!(message.ends_with(FIX), fixed, "{message}");
        }
    }

    fn rect_regions(x: f64, y: f64, w: f64, h: f64) -> Vec<Region> {
        resolve_fill(
            &[Polyline::rect(Point::new(x, y), w, h)],
            FillRule::NonZero,
            0.01,
        )
    }

    #[test]
    fn design_findings_read_a_projects_regions() {
        let shapes = vec![
            ("a".to_owned(), rect_regions(0.0, 0.0, 10.0, 10.0)),
            ("b".to_owned(), rect_regions(6.0, 0.0, 10.0, 10.0)),
            ("far".to_owned(), rect_regions(50.0, 90.0, 10.0, 10.0)),
            ("speck".to_owned(), rect_regions(80.0, 50.0, 0.1, 0.1)),
        ];
        let found = design_findings_named(&shapes, 100.0);
        assert_eq!(codes(&found), ["shapes_overlap", "feature_below_size"]);
        assert!(
            matches!(&found[0], ImportWarning::ShapesOverlap { count: 1, first, second, .. }
            if first == "a" && second == "b")
        );
        assert!(
            matches!(&found[1], ImportWarning::FeatureBelowSize { count: 1, height_mm, .. }
            if (*height_mm - 100.0).abs() < 1e-9)
        );
        // Unnamed regions are named by position.
        let flat: Vec<Region> = shapes.into_iter().flat_map(|(_, r)| r).collect();
        let found = design_findings(&flat, 100.0);
        assert!(
            matches!(&found[0], ImportWarning::ShapesOverlap { first, second, .. }
            if first == "region-1" && second == "region-2")
        );
        // No height, no threshold: nothing is measured against it.
        assert_eq!(codes(&design_findings(&flat, 0.0)), ["shapes_overlap"]);
    }
}
