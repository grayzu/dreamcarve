// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/svg.ts
//   src/import/normalize.ts
//   src/import/svg.test.ts

//! SVG import: each SVG path becomes one feature definition with one
//! instance (`docs/specs/phase-1/design.md`, "Algorithms → SVG import").
//!
//! Parsing, CSS, `use`, shapes, and transforms are delegated to usvg, which
//! hands back every shape as a path in CSS pixels (96 per inch) with its
//! absolute transform. The unit detection, the closing rule for subpaths
//! that return to their start, and the one-definition-per-path grouping
//! follow PureCutCNC. Re-derived for Y-up (ADR-0001): PureCutCNC keeps SVG's
//! Y-down coordinates; here every point is flipped once, `y' = height_mm −
//! y`, so the page's bottom-left corner is the origin.

use crate::{ImportError, ImportWarning};
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Rect};
use dc_project::{FeatureDefinition, FeatureInstance};

/// The units the root `<svg>` declares for its `width` and `height`.
/// Informational: usvg converts every unit to CSS pixels, and the importer
/// converts pixels to millimetres at 96 per inch, so geometry is correct
/// for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootUnits {
    /// `mm`.
    Millimetres,
    /// `cm`.
    Centimetres,
    /// `in`.
    Inches,
    /// `pt`, 72 per inch.
    Points,
    /// `pc`, 6 per inch.
    Picas,
    /// `px`, unitless, percentages, or absent: 96 user units per inch
    /// (CSS), the assumption when nothing physical is declared.
    CssPixels,
}

impl std::fmt::Display for RootUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Millimetres => "mm",
            Self::Centimetres => "cm",
            Self::Inches => "in",
            Self::Points => "pt",
            Self::Picas => "pc",
            Self::CssPixels => "px (96 per inch)",
        })
    }
}

/// Parameters of an SVG import.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgImportOptions {
    /// Furthest a flattened polyline may stray from the source curve, mm.
    pub flatten_tolerance_mm: f64,
    /// Stock thickness, mm: every instance's `z_top_mm`.
    pub thickness_mm: f64,
}

impl SvgImportOptions {
    /// Options for stock `thickness_mm` thick at the default flattening
    /// tolerance, [`FLATTEN_TOLERANCE_DEFAULT_MM`].
    #[must_use]
    pub const fn new(thickness_mm: f64) -> Self {
        Self {
            flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
            thickness_mm,
        }
    }
}

/// The result of an SVG import, in millimetres, Y-up, origin at the page's
/// bottom-left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgImport {
    /// One definition per SVG path that has usable geometry, in document
    /// order. Ids are `d1`, `d2`, ...; names are the path's SVG id, or
    /// `path-N` (N counts every path in document order, from 1).
    pub definitions: Vec<FeatureDefinition>,
    /// One instance per definition, same order. Ids are `i1`, `i2`, ...;
    /// identity transform, `z_top_mm` = stock thickness, `z_bottom_mm` = 0.
    pub instances: Vec<FeatureInstance>,
    /// Bounds of all imported polylines, mm; `None` when nothing imported.
    pub bounds: Option<Rect>,
    /// Page width from the root `width` (or `viewBox`), mm.
    pub page_width_mm: f64,
    /// Page height from the root `height` (or `viewBox`), mm. The Y flip is
    /// `y' = page_height_mm − y`.
    pub page_height_mm: f64,
    /// The units the root declared.
    pub units: RootUnits,
    /// Millimetres per SVG user unit along X: the root `width` over the
    /// `viewBox` width, or 25.4 / 96 when there is no `viewBox`.
    pub mm_per_user_unit: f64,
}

/// Imports the SVG document in `bytes`.
///
/// Every `<path>`, `<rect>`, `<circle>`, `<ellipse>`, `<line>`,
/// `<polyline>` and `<polygon>` becomes one feature definition whose
/// profiles are its subpaths flattened to `options.flatten_tolerance_mm`.
/// Closed subpaths (ending in `Z`, or returning to their start point) become
/// closed polylines; others stay open. Closed polylines with no area are
/// dropped with a warning. Text, images, gradients, clip paths, and masks
/// are skipped or ignored with a warning; paint never affects geometry.
///
/// # Errors
///
/// [`ImportError`] when the bytes are not a readable SVG, a coordinate is
/// not finite, or an option is not a positive finite length.
pub fn import_svg(
    bytes: &[u8],
    options: &SvgImportOptions,
) -> Result<(SvgImport, Vec<ImportWarning>), ImportError> {
    let _ = (bytes, options);
    todo!("#13")
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
    use dc_geometry::{Point, Polyline, Transform, assert_approx_eq};
    use dc_project::{DefinitionId, InstanceId};

    /// Coordinates pass through usvg as f32; at workpiece scale that is a
    /// few micrometres of rounding.
    const TOL_MM: f64 = 1e-4;

    fn import(bytes: &[u8]) -> (SvgImport, Vec<ImportWarning>) {
        import_svg(bytes, &SvgImportOptions::new(12.0)).expect("import succeeds")
    }

    fn def<'a>(result: &'a SvgImport, name: &str) -> &'a FeatureDefinition {
        result
            .definitions
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no definition named {name}"))
    }

    fn bounds(profiles: &[Polyline]) -> Rect {
        profiles
            .iter()
            .filter_map(Polyline::bounds)
            .reduce(|a, b| a.including(b.min).including(b.max))
            .expect("profiles have bounds")
    }

    fn assert_rect(r: Rect, min: (f64, f64), max: (f64, f64)) {
        assert_approx_eq!(r.min.x, min.0, TOL_MM);
        assert_approx_eq!(r.min.y, min.1, TOL_MM);
        assert_approx_eq!(r.max.x, max.0, TOL_MM);
        assert_approx_eq!(r.max.y, max.1, TOL_MM);
    }

    fn names(result: &SvgImport) -> Vec<&str> {
        result.definitions.iter().map(|d| d.name.as_str()).collect()
    }

    // ── units ────────────────────────────────────────────────────────────

    #[test]
    fn mm_sized_root_uses_declared_units() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/mm_root.svg"));
        assert_eq!(result.units, RootUnits::Millimetres);
        assert_approx_eq!(result.mm_per_user_unit, 1.0, 1e-9);
        assert_approx_eq!(result.page_width_mm, 100.0, TOL_MM);
        assert_approx_eq!(result.page_height_mm, 50.0, TOL_MM);
        // SVG y 10..20 on a 50 mm page flips to 30..40.
        assert_rect(
            bounds(&def(&result, "box").profiles),
            (10.0, 30.0),
            (30.0, 40.0),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn unitless_root_assumes_96_dpi() {
        let (result, _) = import(include_bytes!("../tests/fixtures/unitless_root.svg"));
        assert_eq!(result.units, RootUnits::CssPixels);
        assert_approx_eq!(result.mm_per_user_unit, 25.4 / 96.0, 1e-9);
        assert_approx_eq!(result.page_width_mm, 50.8, TOL_MM);
        assert_approx_eq!(result.page_height_mm, 25.4, TOL_MM);
        // 96 × 48 px is one inch by half an inch, at the page's top-left.
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (0.0, 12.7),
            (25.4, 25.4),
        );
    }

    #[test]
    fn inch_sized_root() {
        let (result, _) = import(include_bytes!("../tests/fixtures/inch_root.svg"));
        assert_eq!(result.units, RootUnits::Inches);
        assert_approx_eq!(result.mm_per_user_unit, 0.254, 1e-9);
        assert_approx_eq!(result.page_width_mm, 50.8, TOL_MM);
        assert_approx_eq!(result.page_height_mm, 25.4, TOL_MM);
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (25.4, 0.0),
            (50.8, 25.4),
        );
    }

    #[test]
    fn physical_size_without_viewbox_reads_user_units_as_css_pixels() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/no_viewbox_mm.svg"));
        assert_eq!(result.units, RootUnits::Millimetres);
        assert_approx_eq!(result.mm_per_user_unit, 25.4 / 96.0, 1e-9);
        // 96 user units are 96 px, one inch, per the SVG specification.
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (0.0, 100.0 - 25.4),
            (25.4, 100.0),
        );
        assert!(
            warnings.contains(&ImportWarning::PhysicalSizeWithoutViewBox {
                units: RootUnits::Millimetres
            })
        );
    }

    // ── placement ────────────────────────────────────────────────────────

    #[test]
    fn y_is_flipped() {
        let (result, _) = import(include_bytes!("../tests/fixtures/y_flip.svg"));
        // Near the top of the drawing (small SVG y) means large Y-up y.
        assert_rect(
            bounds(&def(&result, "top").profiles),
            (10.0, 90.0),
            (20.0, 95.0),
        );
        assert_rect(
            bounds(&def(&result, "bottom").profiles),
            (10.0, 5.0),
            (20.0, 10.0),
        );
    }

    #[test]
    fn drawing_bounds_cover_all_imported_geometry() {
        let (result, _) = import(include_bytes!("../tests/fixtures/y_flip.svg"));
        assert_rect(result.bounds.expect("bounds"), (10.0, 5.0), (20.0, 95.0));
    }

    #[test]
    fn transforms_are_applied() {
        let (result, _) = import(include_bytes!("../tests/fixtures/translated_group.svg"));
        // translate(30,20) moves the 10 mm square to SVG (30..40, 20..30),
        // which flips to y 70..80.
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (30.0, 70.0),
            (40.0, 80.0),
        );
    }

    #[test]
    fn nested_group_transforms_compose() {
        let (result, _) = import(include_bytes!("../tests/fixtures/nested_groups.svg"));
        // translate(10,0) · scale(2): rect (5..10) lands at x 20..30, SVG y
        // 10..20, flipped to 80..90.
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (20.0, 80.0),
            (30.0, 90.0),
        );
    }

    // ── geometry ─────────────────────────────────────────────────────────

    #[test]
    fn closed_and_open_subpaths_preserved() {
        let (result, _) = import(include_bytes!("../tests/fixtures/closed_and_open.svg"));
        assert_eq!(names(&result), ["mixed", "slot"]);

        let mixed = &def(&result, "mixed").profiles;
        let closed: Vec<bool> = mixed.iter().map(Polyline::is_closed).collect();
        assert_eq!(closed, [true, false]);
        assert_eq!(mixed[0].vertices().len(), 3);
        assert_eq!(mixed[1].vertices().len(), 3);

        let slot = &def(&result, "slot").profiles;
        assert_eq!(slot.len(), 1);
        assert!(!slot[0].is_closed());
        assert_rect(bounds(slot), (50.0, 50.0), (80.0, 50.0));
    }

    #[test]
    fn circle_flattens_within_tolerance() {
        let options = SvgImportOptions::new(12.0);
        let (result, _) =
            import_svg(include_bytes!("../tests/fixtures/circle.svg"), &options).unwrap();
        assert_eq!(result.definitions.len(), 1);
        let profiles = &result.definitions[0].profiles;
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_closed());
        assert!(profiles[0].vertices().len() > 8);

        // usvg turns the circle into four cubic Béziers before we flatten;
        // that approximation strays up to 2.7e-4 × r (0.0122 mm at r = 45)
        // from the true circle. Flattening adds at most the tolerance.
        let bezier_error_mm = 2.75e-4 * 45.0;
        let allowed_mm = options.flatten_tolerance_mm + bezier_error_mm + TOL_MM;
        let centre = Point::new(50.0, 50.0);
        for seg in profiles[0].segments() {
            assert!(seg.is_line());
            for p in [seg.start, seg.start + (seg.end - seg.start) * 0.5] {
                let err = (p.distance_to(centre) - 45.0).abs();
                assert!(err <= allowed_mm, "{p:?} is {err} mm off the circle");
            }
        }
    }

    #[test]
    fn zero_area_closed_path_dropped_with_warning() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/zero_area.svg"));
        // `flat` keeps its triangle; `only-flat` has nothing left.
        assert_eq!(names(&result), ["flat"]);
        let flat = &def(&result, "flat").profiles;
        assert_eq!(flat.len(), 1);
        assert!(flat[0].is_closed());
        assert_rect(bounds(flat), (10.0, 40.0), (20.0, 50.0));

        assert!(warnings.contains(&ImportWarning::ZeroAreaDropped {
            path: "flat".into(),
            subpath: 0
        }));
        assert!(warnings.contains(&ImportWarning::ZeroAreaDropped {
            path: "only-flat".into(),
            subpath: 0
        }));
        assert!(warnings.contains(&ImportWarning::EmptyPath {
            path: "only-flat".into()
        }));
    }

    // ── skipped content ──────────────────────────────────────────────────

    #[test]
    fn text_and_images_warn_and_skip() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/text_and_image.svg"));
        assert_eq!(names(&result), ["kept"]);
        assert!(warnings.contains(&ImportWarning::TextIgnored { count: 1 }));
        assert!(warnings.contains(&ImportWarning::ImagesIgnored { count: 1 }));
    }

    #[test]
    fn gradients_and_clip_paths_warn_but_keep_geometry() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/paint_and_clip.svg"));
        // The clip path's own rect is not a shape on the page.
        assert_eq!(names(&result), ["graded", "inside-clip"]);
        // Clipping is ignored: the full 30 mm square is imported.
        assert_rect(
            bounds(&def(&result, "inside-clip").profiles),
            (50.0, 20.0),
            (80.0, 50.0),
        );
        assert!(warnings.contains(&ImportWarning::PaintServerIgnored {
            path: "graded".into()
        }));
        assert!(warnings.contains(&ImportWarning::ClipPathIgnored {
            group: "clipped".into()
        }));
    }

    // ── definitions and instances ────────────────────────────────────────

    #[test]
    fn definitions_named_from_id_or_path_index() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
            <rect id="a" x="0" y="0" width="10" height="10"/>
            <rect x="20" y="0" width="10" height="10"/>
            <rect id="c" x="40" y="0" width="10" height="10"/>
        </svg>"#;
        let (result, _) = import(svg);
        assert_eq!(names(&result), ["a", "path-2", "c"]);
        let ids: Vec<&str> = result.definitions.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["d1", "d2", "d3"]);
    }

    #[test]
    fn one_instance_per_definition_at_stock_thickness() {
        let (result, _) = import_svg(
            include_bytes!("../tests/fixtures/y_flip.svg"),
            &SvgImportOptions::new(18.5),
        )
        .unwrap();
        assert_eq!(result.instances.len(), result.definitions.len());
        for (n, (inst, def)) in result.instances.iter().zip(&result.definitions).enumerate() {
            assert_eq!(inst.id, InstanceId::new(format!("i{}", n + 1)));
            assert_eq!(inst.definition, def.id);
            assert_eq!(def.id, DefinitionId::new(format!("d{}", n + 1)));
            assert_eq!(inst.name, def.name);
            assert_eq!(inst.transform, Transform::IDENTITY);
            assert_approx_eq!(inst.z_top_mm, 18.5, 0.0);
            assert_approx_eq!(inst.z_bottom_mm, 0.0, 0.0);
            assert!(inst.visible);
            assert!(!inst.locked);
        }
    }

    #[test]
    fn import_is_deterministic() {
        let bytes = include_bytes!("../tests/fixtures/closed_and_open.svg");
        assert_eq!(import(bytes), import(bytes));
    }

    #[test]
    fn coaster_fixture_is_one_90_mm_circle() {
        let (result, warnings) = import(include_bytes!("../../../fixtures/coaster.svg"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names(&result), ["coaster"]);
        let profiles = &result.definitions[0].profiles;
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_closed());
        let b = bounds(profiles);
        assert_approx_eq!(b.width_mm(), 90.0, 0.03);
        assert_approx_eq!(b.height_mm(), 90.0, 0.03);
        assert_approx_eq!(b.min.x, 5.0, 0.03);
        assert_approx_eq!(b.min.y, 5.0, 0.03);
    }

    // ── errors ───────────────────────────────────────────────────────────

    #[test]
    fn malformed_svg_is_an_error() {
        let err = import_svg(b"<svg", &SvgImportOptions::new(12.0)).unwrap_err();
        assert!(matches!(err, ImportError::Svg(_)), "{err:?}");
    }

    #[test]
    fn non_positive_or_non_finite_options_are_rejected() {
        let bytes = include_bytes!("../tests/fixtures/mm_root.svg");
        for tolerance_mm in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            let options = SvgImportOptions {
                flatten_tolerance_mm: tolerance_mm,
                thickness_mm: 12.0,
            };
            assert!(matches!(
                import_svg(bytes, &options),
                Err(ImportError::InvalidTolerance { .. })
            ));
        }
        for thickness_mm in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
            assert!(matches!(
                import_svg(bytes, &SvgImportOptions::new(thickness_mm)),
                Err(ImportError::InvalidThickness { .. })
            ));
        }
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        // A scale this large overflows f32 inside usvg or f64 after
        // conversion; either way nothing non-finite may come out.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
            <g transform="scale(1e38)"><g transform="scale(1e38)"><rect width="10" height="10"/></g></g>
        </svg>"#;
        match import_svg(svg, &SvgImportOptions::new(12.0)) {
            Err(ImportError::NonFinite { .. }) => {}
            Ok((result, _)) => {
                for p in result.definitions.iter().flat_map(|d| &d.profiles) {
                    for v in p.vertices() {
                        assert!(v.x.is_finite() && v.y.is_finite());
                    }
                }
            }
            Err(e) => panic!("unexpected error {e:?}"),
        }
    }

    // ── ported from PureCutCNC src/import/svg.test.ts ────────────────────
    //
    // Upstream checks paint classification (hasFill / hasStroke) and that
    // each snippet yields exactly one shape with the right open/closed
    // state. Only the geometry half is ported: DreamCarve imports geometry
    // whatever the paint. The snippets gain an `xmlns` (usvg ignores
    // un-namespaced elements, browsers' DOMParser in upstream's test does
    // not). Upstream asserts no coordinates, so the Y-up flip changes no
    // expected value here.

    fn parse(markup: &str) -> SvgImport {
        let svg = markup.replacen("<svg", r#"<svg xmlns="http://www.w3.org/2000/svg""#, 1);
        import(svg.as_bytes()).0
    }

    fn only_profile(result: &SvgImport) -> &Polyline {
        assert_eq!(result.definitions.len(), 1, "expected one shape");
        assert_eq!(
            result.definitions[0].profiles.len(),
            1,
            "expected one profile"
        );
        &result.definitions[0].profiles[0]
    }

    #[test]
    fn every_paint_variant_imports_exactly_one_shape() {
        // The `shapes.length === 1` assertion of each upstream paint test.
        let cases = [
            r#"<svg><rect width="10" height="10"/></svg>"#,
            r#"<svg><g fill="red"><rect width="10" height="10"/></g></svg>"#,
            r#"<svg><g stroke="blue"><line x1="0" y1="0" x2="10" y2="10"/></g></svg>"#,
            r#"<svg><circle cx="5" cy="5" r="3" fill="none" stroke="blue"/></svg>"#,
            r#"<svg><path d="M0,0 L10,10" fill="none" stroke="red"/></svg>"#,
            r#"<svg><rect x="0" y="0" width="10" height="10" fill="green" stroke="black"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="blue" style="fill:red"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="blue" style="fill:none"/></svg>"#,
            r#"<svg><rect width="10" height="10" stroke="blue" style="stroke:red"/></svg>"#,
            r#"<svg><circle cx="5" cy="5" r="3" fill="transparent"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="None"/></svg>"#,
            r#"<svg><g opacity="0"><rect width="10" height="10" fill="red"/></g></svg>"#,
            r#"<svg><g opacity="0"><g><rect width="10" height="10" fill="red" stroke="blue"/></g></g></svg>"#,
            r#"<svg><rect width="10" height="10" fill="red" opacity="0"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="red" fill-opacity="0"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="red" style="fill-opacity:0"/></svg>"#,
            r#"<svg><circle cx="5" cy="5" r="3" fill="none" stroke="blue" stroke-opacity="0"/></svg>"#,
            r#"<svg><g fill-opacity="0"><rect width="10" height="10" fill="red"/></g></svg>"#,
            r#"<svg><g stroke-opacity="0"><line x1="0" y1="0" x2="10" y2="10" stroke="blue"/></g></svg>"#,
            r#"<svg><g fill-opacity="0"><rect width="10" height="10" fill="red" fill-opacity="1"/></g></svg>"#,
            r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0)"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="rgba(255 0 0 / 0)"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="hsla(0, 100%, 50%, 0)"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="hsla(0 100% 50% / 0)"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0%)"/></svg>"#,
            r##"<svg><rect width="10" height="10" fill="#ff000000"/></svg>"##,
            r##"<svg><rect width="10" height="10" fill="#ff0000ff"/></svg>"##,
            r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0.5)"/></svg>"#,
            r#"<svg><rect width="10" height="10" fill="hsla(0, 100%, 50%, 0.3)"/></svg>"#,
            r#"<svg><g opacity="0.5"><g opacity="0.5"><rect width="10" height="10" fill="red"/></g></g></svg>"#,
            r#"<svg><rect width="10" height="10" fill="red" fill-opacity="0" stroke="blue"/></svg>"#,
        ];
        for markup in cases {
            let result = parse(markup);
            assert_eq!(result.definitions.len(), 1, "{markup}");
        }
    }

    #[test]
    fn stroke_only_circle_is_closed() {
        // test_stroke_only_circle
        let result = parse(r#"<svg><circle cx="5" cy="5" r="3" fill="none" stroke="blue"/></svg>"#);
        assert!(only_profile(&result).is_closed());
    }

    #[test]
    fn open_polyline_stays_open() {
        // test_open_polyline_has_stroke
        let result =
            parse(r#"<svg><polyline points="0,0 10,10 20,0" fill="none" stroke="red"/></svg>"#);
        let profile = only_profile(&result);
        assert!(!profile.is_closed());
        assert_eq!(profile.vertices().len(), 3);
    }

    #[test]
    fn open_path_stays_open() {
        // test_open_path_has_stroke
        let result = parse(r#"<svg><path d="M0,0 L10,10" fill="none" stroke="blue"/></svg>"#);
        let profile = only_profile(&result);
        assert!(!profile.is_closed());
        assert_eq!(profile.vertices().len(), 2);
    }

    #[test]
    fn closed_filled_path_is_closed() {
        // test_closed_filled_path_is_closed
        let result = parse(r#"<svg><path d="M0,0 L10,0 L10,10 L0,10 Z" fill="green"/></svg>"#);
        let profile = only_profile(&result);
        assert!(profile.is_closed());
        assert_eq!(profile.vertices().len(), 4);
    }

    #[test]
    fn path_returning_to_start_is_closed() {
        // From upstream svg.ts `parsePathProfiles`: a subpath whose last
        // point is its start point is closed even without `Z`, and the
        // duplicate closing point is not kept.
        let result =
            parse(r#"<svg><path d="M0,0 L10,0 L10,10 L0,0" fill="none" stroke="red"/></svg>"#);
        let profile = only_profile(&result);
        assert!(profile.is_closed());
        assert_eq!(profile.vertices().len(), 3);
    }
}
