// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Image prep of a DXF (#669): the drawing is read by the DXF importer,
//! with its units and its warnings, HATCH boundaries included, and its
//! closed shapes become the filled paths of an SVG in millimetres that
//! [`prepare`] then prepares as it would any SVG: fills resolved, specks
//! dropped, smoothing, `--regions`. Each shape is one path filled by
//! even-odd, so a hatch's island stays a hole, and a shape drawn inside
//! another is resolved as the SVG pipeline resolves a filled path inside
//! another. Open paths cannot be outlined, since DXF gives a line no
//! width, and are reported instead.

use std::fmt::Write as _;

use dc_geometry::{Polyline, Rect};

use super::report::PrepSource;
use super::run::{PrepOptions, Prepared, check_threshold, prepare};
use crate::{DxfImportOptions, ImportError, ImportWarning, import_dxf};

/// Prepares the DXF in `bytes`, as [`prepare`] prepares an SVG. The
/// warnings are the DXF importer's, then
/// [`ImportWarning::DxfOpenPathNotConvertible`] when open paths were left
/// out, then the pipeline's. The report's `source` names the DXF and its
/// units.
///
/// # Errors
///
/// [`ImportError::PrepThresholdRange`] before anything is read; the DXF
/// importer's errors, [`ImportError::UnitsUnknown`] among them, since
/// image-prep takes no units of its own; [`ImportError::PrepNothingLeft`]
/// when the drawing has no closed shape; and the errors of [`prepare`].
pub fn prepare_dxf(
    bytes: &[u8],
    options: &PrepOptions,
) -> Result<(Prepared, Vec<ImportWarning>), ImportError> {
    check_threshold("--min-detail", options.min_detail_pct)?;
    check_threshold("--smooth", options.smooth_pct)?;
    let dxf_options = DxfImportOptions {
        max_vertices: options.max_vertices,
        ..DxfImportOptions::new()
    };
    let (import, mut warnings) = import_dxf(bytes, &dxf_options)?;
    // Unreachable without units: the import fails first.
    let units = import.units.ok_or(ImportError::UnitsUnknown {
        declared: "unitless".to_owned(),
    })?;

    let mut closed: Vec<(&str, Vec<&Polyline>)> = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for definition in &import.definitions {
        let (loops, lines): (Vec<&Polyline>, Vec<&Polyline>) =
            definition.profiles.iter().partition(|p| p.is_closed());
        if !lines.is_empty() {
            open.push(&definition.name);
        }
        if !loops.is_empty() {
            closed.push((&definition.name, loops));
        }
    }
    if let Some(first) = open.first() {
        warnings.push(ImportWarning::DxfOpenPathNotConvertible {
            count: open.len(),
            first: (*first).to_owned(),
        });
    }
    let svg = svg_of(&closed).ok_or(ImportError::PrepNothingLeft)?;
    let (mut prepared, more) = prepare(svg.as_bytes(), options)?;
    prepared.report.source = PrepSource::Dxf {
        units,
        mm_per_unit: import.mm_per_unit,
        closed_shapes: closed.len(),
        open_paths: open.len(),
    };
    warnings.extend(more);
    Ok((prepared, warnings))
}

/// An SVG of `shapes`, each a black path filled by even-odd and named by
/// its id, in millimetres on a page that is their box: Y flipped, the
/// box's lower-left corner the page's bottom-left. `None` when there is
/// no shape with a size.
fn svg_of(shapes: &[(&str, Vec<&Polyline>)]) -> Option<String> {
    let bounds = shapes
        .iter()
        .flat_map(|(_, loops)| loops.iter().filter_map(|p| p.bounds()))
        .reduce(|a: Rect, b| a.including(b.min).including(b.max))?;
    let (width_mm, height_mm) = (bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y);
    if !(width_mm > 0.0 && height_mm > 0.0) {
        return None;
    }
    let at = |x: f64, y: f64| (x - bounds.min.x, bounds.max.y - y);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width_mm}mm\" height=\"{height_mm}mm\" viewBox=\"0 0 {width_mm} {height_mm}\">\n"
    );
    for (name, loops) in shapes {
        let mut d = String::new();
        for polyline in loops {
            let Some(first) = polyline.vertices().first() else {
                continue;
            };
            let (x, y) = at(first.x, first.y);
            let _ = write!(d, "M {x} {y} ");
            for segment in polyline.segments() {
                let (x, y) = at(segment.end.x, segment.end.y);
                match segment.arc() {
                    Some(arc) => {
                        let large = u8::from(arc.sweep_rad.abs() > std::f64::consts::PI);
                        // Y is flipped, so counter-clockwise (positive
                        // sweep) is SVG's negative-angle direction.
                        let sweep = u8::from(arc.sweep_rad < 0.0);
                        let r = arc.radius_mm;
                        let _ = write!(d, "A {r} {r} 0 {large} {sweep} {x} {y} ");
                    }
                    None => {
                        let _ = write!(d, "L {x} {y} ");
                    }
                }
            }
            d.push_str("Z ");
        }
        let _ = writeln!(
            svg,
            "<path id=\"{}\" fill=\"#000000\" fill-rule=\"evenodd\" d=\"{}\"/>",
            escape(name),
            d.trim_end()
        );
    }
    svg.push_str("</svg>\n");
    Some(svg)
}

/// `text` safe in an XML attribute.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;
    use crate::{SvgImportOptions, import_svg};
    use dc_geometry::{Vertex, assert_approx_eq};

    /// Arcs keep their side through the Y flip: a 20 × 10 rectangle whose
    /// top is a half circle bulging up, and one whose top bows down into
    /// it, read back from the SVG with the areas they had.
    #[test]
    fn arcs_keep_their_side_through_the_flip() {
        for bulge in [1.0, -0.5] {
            // Counter-clockwise: along the bottom, up, then the top from
            // (20, 10) to (0, 10) as an arc.
            let shape = Polyline::new(
                vec![
                    Vertex::line(0.0, 0.0),
                    Vertex::line(20.0, 0.0),
                    Vertex::new(20.0, 10.0, bulge),
                    Vertex::line(0.0, 10.0),
                ],
                true,
            );
            let want = shape.signed_area_mm2().abs();
            let svg = svg_of(&[("a&b", vec![&shape])]).unwrap();
            assert!(svg.contains(r#"id="a&amp;b""#), "{svg}");
            let (import, _) = import_svg(svg.as_bytes(), &SvgImportOptions::new()).unwrap();
            let got = import.definitions[0].profiles[0].signed_area_mm2().abs();
            assert_approx_eq!(got, want, 0.01);
        }
    }
}
