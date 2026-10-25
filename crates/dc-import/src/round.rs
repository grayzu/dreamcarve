// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Rounding of imported polylines (#389, functional spec capability 20a).
//! Both importers round each polyline once its closure is decided, so a
//! project made from an SVG or a DXF has no coordinate finer than
//! [`dc_project::LENGTH_DECIMALS`] places of a millimetre. Bulges are kept
//! at full precision: rounding one keeps the arc's ends but moves its
//! centre, and so a program's I and J, by several micrometres
//! (maintainer decision 2026-10-25).

use dc_geometry::{Polyline, Vertex};
use dc_project::round_length_mm;

/// `polyline` with every coordinate rounded by [`round_length_mm`],
/// bulges unchanged, keeping its closure. A vertex that rounds onto the
/// one before it is dropped, its bulge taking the place of the zero-length
/// segment's; so is a closed polyline's last vertex when it rounds onto
/// the first. `None` when fewer than 2 vertices are left.
pub(crate) fn round_polyline(polyline: &Polyline) -> Option<Polyline> {
    let mut out: Vec<Vertex> = Vec::with_capacity(polyline.vertices().len());
    for v in polyline.vertices() {
        let rounded = Vertex::new(round_coordinate(v.x), round_coordinate(v.y), v.bulge);
        match out.last_mut() {
            Some(last) if last.point() == rounded.point() => last.bulge = rounded.bulge,
            _ => out.push(rounded),
        }
    }
    let ends_meet =
        out.len() > 1 && out.first().map(|v| v.point()) == out.last().map(|v| v.point());
    if polyline.is_closed() && ends_meet {
        out.pop();
    }
    (out.len() >= 2).then(|| Polyline::new(out, polyline.is_closed()))
}

/// `value_mm` rounded, with `-0` written as `0`.
fn round_coordinate(value_mm: f64) -> f64 {
    round_length_mm(value_mm) + 0.0
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp,
        reason = "tests; rounded values are compared exactly"
    )]

    use crate::{DxfImportOptions, SvgImportOptions, import_dxf, import_svg};
    use dc_geometry::{Polyline, Vertex};
    use dc_project::{FeatureDefinition, round_length_mm};
    use std::f64::consts::PI;

    const PLAQUE_SVG: &[u8] = include_bytes!("../../../tests/fixtures/plaque.svg");
    const PLAQUE_DXF: &[u8] = include_bytes!("../../../tests/fixtures/plaque.dxf");

    fn svg(bytes: &[u8]) -> Vec<FeatureDefinition> {
        let (import, warnings) = import_svg(bytes, &SvgImportOptions::new()).expect("imports");
        assert!(warnings.is_empty(), "{warnings:?}");
        import.definitions
    }

    fn dxf(bytes: &[u8]) -> Vec<FeatureDefinition> {
        let (import, warnings) = import_dxf(bytes, &DxfImportOptions::new()).expect("imports");
        assert!(warnings.is_empty(), "{warnings:?}");
        import.definitions
    }

    /// Every vertex of every profile, with the definition's name.
    fn vertices(definitions: &[FeatureDefinition]) -> Vec<(&str, Vertex)> {
        definitions
            .iter()
            .flat_map(|d| {
                d.profiles
                    .iter()
                    .flat_map(|p| p.vertices().iter().map(|v| (d.name.as_str(), *v)))
            })
            .collect()
    }

    fn profile<'a>(definitions: &'a [FeatureDefinition], name: &str) -> &'a Polyline {
        let definition = definitions
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no definition named {name}"));
        assert_eq!(definition.profiles.len(), 1, "{name} has one profile");
        &definition.profiles[0]
    }

    fn assert_coordinates_rounded(definitions: &[FeatureDefinition]) {
        for (name, v) in vertices(definitions) {
            assert!(
                round_length_mm(v.x) == v.x && round_length_mm(v.y) == v.y,
                "{name}: ({}, {}) is not rounded to micrometres",
                v.x,
                v.y
            );
        }
    }

    /// No segment of `polyline`, the closing one included, has zero length.
    fn assert_no_zero_length_segment(polyline: &Polyline) {
        for (i, segment) in polyline.segments().enumerate() {
            assert!(
                segment.start != segment.end,
                "segment {i} is zero-length at {:?}",
                segment.start
            );
        }
    }

    /// #389: the SVG's user-unit-to-mm transform leaves noise such as
    /// `10.00000027815501` in the coordinates; every one is rounded to 6
    /// decimals, and the outline's corner that was `10.00000027815501` is
    /// exactly 10. (The transform's noise grows with distance from the
    /// origin, 2.8e-8 of it, so `150.00000417` still rounds to
    /// `150.000004`.)
    #[test]
    fn svg_import_rounds_coordinates_to_micrometres() {
        let definitions = svg(PLAQUE_SVG);
        assert_coordinates_rounded(&definitions);
        let outline = profile(&definitions, "outline");
        assert_eq!(
            outline.vertices()[0].x,
            10.0,
            "the outline's top-left corner: {:?}",
            outline.vertices()
        );
    }

    /// #389: the plaque's DXF coordinates are exact, but the points
    /// flattened from its SPLINE are not; those are rounded too.
    #[test]
    fn dxf_import_rounds_flattened_points() {
        let definitions = dxf(PLAQUE_DXF);
        assert_coordinates_rounded(&definitions);
        let squiggle = profile(&definitions, "squiggle");
        assert!(
            squiggle.vertices().len() > 4,
            "the spline is flattened to points: {:?}",
            squiggle.vertices()
        );
    }

    /// #389: bulges are not lengths and are not rounded (maintainer
    /// decision 2026-10-25: rounding one moves the arc's centre by
    /// micrometres). The DXF's quarter arcs are tan(π/8) exactly; the
    /// SVG's, rebuilt from Bézier curves, are within 1e-12 of it and keep
    /// every digit they were computed with.
    #[test]
    fn import_keeps_bulges_unrounded() {
        let quarter = (PI / 8.0).tan();
        for (format, definitions) in [("svg", svg(PLAQUE_SVG)), ("dxf", dxf(PLAQUE_DXF))] {
            let quarters: Vec<f64> = vertices(&definitions)
                .into_iter()
                .map(|(_, v)| v.bulge.abs())
                .filter(|b| (b - quarter).abs() < 1e-4)
                .collect();
            // Four rounded corners on each of outline, pocket and border.
            assert_eq!(quarters.len(), 12, "{format}: quarter arcs");
            for b in quarters {
                assert_ne!(
                    b, 0.414214,
                    "{format}: a quarter arc's bulge is not rounded"
                );
                if format == "dxf" {
                    assert_eq!(b, quarter, "{format}: the file's bulge, to every digit");
                } else {
                    assert!((b - quarter).abs() < 1e-12, "{format}: {b}");
                }
            }
        }
    }

    /// #389: rounding decides nothing about closure. The path ends 0.35 µm
    /// from its start without `Z`, so it is closed; its last point rounds
    /// onto the start as well, and the point before that lies 1.4 µm from
    /// the start (kept when the path is built) but rounds onto it too. Both
    /// are dropped, so the square imports closed with four corners and no
    /// zero-length segment.
    #[test]
    fn rounding_keeps_closed_paths_closed() {
        // 25.4 mm over a 96-unit viewBox: one user unit is one CSS pixel
        // with no f32 scale noise, so the offsets below land as written:
        // 1.85e-6 px is 0.49 µm.
        let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="25.4mm" height="25.4mm" viewBox="0 0 96 96">
  <path id="square" d="M 0.00000185 0.00000185 L 40 0 L 40 40 L 0 40 L -0.00000185 -0.00000185 L 0.00000075 0.00000113"/>
</svg>"#;
        let definitions = svg(bytes);
        assert_coordinates_rounded(&definitions);
        let square = profile(&definitions, "square");
        assert!(square.is_closed(), "the path returned to its start");
        assert_eq!(square.vertices().len(), 4, "{:?}", square.vertices());
        assert_no_zero_length_segment(square);
    }
}
